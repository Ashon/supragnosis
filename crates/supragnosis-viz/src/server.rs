//! The viewer's socket and its HTTP: binding, the accept loop, one connection from request to
//! answer, the SSE stream, and the headers every answer carries.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use supragnosis_engine::Engine;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

use crate::routes::{err_body, respond, Response};
use crate::{asset, ClientsView, FedStatus, NarrowShare};

/// Upper bound (bytes) for reading the request line + headers. GET-only, so there is no body; a
/// request exceeding this bound is treated as malicious/malformed and dropped.
const MAX_REQUEST_HEAD: usize = 16 * 1024;

/// Binds the viewer's unix socket at `path` and locks it down to the owning user.
///
/// - The parent directory is created 0700 (defense in depth: the dir already denies foreign users
///   before the socket mode is even consulted).
/// - A leftover socket file from a crashed process is replaced, but only after probing it: if
///   something still accepts connections there, this is a second live instance and binding fails
///   loud (Principle 5) instead of silently stealing the path.
/// - The bound socket is chmod 0600 - the socket file is the whole access control (F19: every
///   connection is attributable to the local principal, enforced by the OS).
pub async fn bind_uds(path: &Path) -> anyhow::Result<UnixListener> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() && !dir.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("failed to create viewer socket dir {}", dir.display()))?;
        }
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            anyhow::bail!(
                "another instance is already serving the viewer socket at {} - stop it first",
                path.display()
            );
        }
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove stale viewer socket {}", path.display()))?;
    }
    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind viewer socket {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod viewer socket {}", path.display()))?;
    Ok(listener)
}

/// Accepts connections on the injected listener and serves the viewer/graph API (infinite accept loop).
///
/// Binding is done by **the caller** (so a test can bind port 0 and look up the actual port).
/// Each connection is split off into a task, but an individual connection failure is swallowed
/// so it does not kill the server.
pub async fn serve(
    engine: Arc<Engine>,
    listener: UnixListener,
    events: broadcast::Sender<String>,
    fed: Option<FedStatus>,
    narrow: Option<NarrowShare>,
    clients: Option<ClientsView>,
) -> anyhow::Result<()> {
    loop {
        // Peer trust is settled before accept ever runs: the socket file is 0600, so the OS only
        // lets the owning user connect (F19) - there is no per-connection trust decision left.
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "viz accept failed - continuing");
                continue;
            }
        };
        let engine = Arc::clone(&engine);
        let events = events.clone();
        let fed = fed.clone();
        let narrow = narrow.clone();
        let clients = clients.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(engine, &events, stream, fed, narrow, clients).await {
                tracing::debug!(error = %e, "viz connection handling failed");
            }
        });
    }
}

/// One connection: parse the request line -> route -> respond, then close. The exception is
/// `/api/events`, which is an SSE stream: it is not closed and keeps streaming events.
///
/// Generic over the stream so tests can drive it with any duplex byte stream. There are no
/// browser-facing trust checks here: a unix socket is unreachable from a web page, so the Host
/// (DNS-rebinding) and CSRF gates the TCP listener needed do not apply - admission was already
/// decided by the socket file's 0600 mode.
async fn handle_conn<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    engine: Arc<Engine>,
    events: &broadcast::Sender<String>,
    mut stream: S,
    fed: Option<FedStatus>,
    narrow: Option<NarrowShare>,
    clients: Option<ClientsView>,
) -> anyhow::Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > MAX_REQUEST_HEAD {
            break;
        }
    }

    let head = String::from_utf8_lossy(&buf);
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let if_none_match = head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("if-none-match").then(|| v.trim().to_string())
    });

    // A shipped file - no store read, no policy beyond the page's own - straight from the binary.
    if method == "GET" {
        if let Some((kind, bytes)) = asset(path) {
            return write_asset(&mut stream, kind, bytes).await;
        }
    }

    // SSE: live MCP event stream - the response is not closed and events keep streaming.
    if method == "GET" && path == "/api/events" {
        return stream_events(stream, events.subscribe()).await;
    }

    // Every route below reads the store, folds the log or writes a file - synchronous work that can
    // take seconds on a large store (the merge band, a curation poll every 2.5s). Run on the async
    // worker it would hold that thread, which the MCP daemon shares, and stall its tool calls.
    // The graph, the hypergraph and the Log carry an ETag, a hash of their body, so a poll that finds
    // nothing new costs the page nothing (design-system.md Section 27.3, U33): the body is still
    // built, since no revision of the store covers every change to what they show, but an unchanged
    // one is answered 304 with no body, and the viewer keeps what it has.
    let conditional =
        method == "GET" && matches!(path, "/api/graph" | "/api/hypergraph" | "/api/observations");
    let (method, path, query) = (method.to_string(), path.to_string(), query.to_string());
    let (resp, etag) = tokio::task::spawn_blocking(move || {
        let r = respond(
            &engine,
            &method,
            &path,
            &query,
            fed.as_ref(),
            narrow.as_ref(),
            clients.as_ref(),
        );
        let etag = (conditional && r.status == "200 OK").then(|| body_etag(&r.body));
        (r, etag)
    })
    .await
    .unwrap_or_else(|e| {
        let r = Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&format!("the request's handler failed: {e}")),
        };
        (r, None)
    });
    match etag {
        Some(tag) if if_none_match.as_deref().is_some_and(|h| etag_matches(h, &tag)) => {
            write_not_modified(&mut stream, &tag).await
        }
        tag => write_response(&mut stream, &resp, tag.as_deref()).await,
    }
}

/// A strong ETag for a response body: its length and a hash of its bytes.
fn body_etag(body: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut h);
    format!("\"{:016x}-{:x}\"", h.finish(), body.len())
}

/// Whether an `If-None-Match` header names this ETag: one of its comma-separated tags, or `*`.
fn etag_matches(header: &str, tag: &str) -> bool {
    header.split(',').map(str::trim).any(|t| t == "*" || t == tag)
}

/// SSE stream: after the `text/event-stream` header, emit `data: {json}\n\n` per event.
/// The JSON is a single line, so the frame is simple. Terminates when the client disconnects (write fails).
async fn stream_events<S: AsyncWrite + Unpin>(
    mut stream: S,
    mut rx: broadcast::Receiver<String>,
) -> anyhow::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: ok\n\n",
        )
        .await?;
    stream.flush().await?;
    loop {
        match rx.recv().await {
            Ok(json) => {
                let frame = format!("data: {json}\n\n");
                if stream.write_all(frame.as_bytes()).await.is_err() {
                    break; // client disconnected
                }
                if stream.flush().await.is_err() {
                    break;
                }
            }
            // If a slow client falls behind, skip the dropped items and continue.
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}

/// The second line of defence against stored XSS in the viewer page.
///
/// The first is output escaping, and it is guarded: `esc()` wraps every untrusted value reaching an
/// innerHTML sink, `viz_source_escapes_untrusted_names` pins it, and ESLint's `no-unsanitized` runs
/// over the sinks in CI. But the page renders entity names, descriptions and proposal rationale, and
/// under federation those are synced, attacker-influenceable input - so the class that survives a
/// missed `esc()` is script execution with full reach over this socket. One escape is one mistake
/// away from being none; a policy the browser enforces does not depend on remembering.
///
/// `architecture.md` Section 10 recorded this as owed, and scoped the web-hardening checklist to the
/// Phase 3.5 hub surface - but the shell renders synced content today, so this half came due before
/// the surface that named it.
///
/// Directive by directive, against what the page actually does:
/// - `default-src 'none'` - nothing is fetchable unless named below.
/// - `script-src 'self' viz:` - `viewer.html` carries no inline `<script>` (only
///   `<script src="/viewer.js">`), so this needs no `'unsafe-inline'`, which is what makes it worth
///   having: an injected `<script>`, an `onclick=` attribute and a `javascript:` URI are all refused.
///   `viz:` is for the desktop shell, whose webview serves this page from a custom-scheme origin
///   (`viz://localhost`) - `'self'` should already cover it, and naming the scheme means a webview
///   that treats that origin as opaque degrades to "still works" rather than "blank window".
/// - `style-src 'self' 'unsafe-inline'` - six generated spans carry `style="background:<color>"`.
///   `'unsafe-inline'` buys CSS injection, whose exfiltration route is a URL fetch, and `img-src`
///   below is what closes that. It costs nothing on the script side, which is the class that matters.
/// - `img-src 'self' data:` - the favicon and the CSS noise texture are `data:` SVGs.
/// - `font-src 'self'` - the faces the page ships under `/fonts/` (design-system.md U11), and no
///   other host's.
/// - `connect-src 'self'` - polling and the SSE stream, same origin.
/// - `base-uri 'none'`, `form-action 'none'`, `frame-ancestors 'none'`, `object-src 'none'` - the
///   page has no `<base>`, no form and no plugin, and nothing should frame it. Denying what is
///   unused costs nothing and removes the redirect tricks that turn one injection into two.
pub const CSP: &str = "default-src 'none'; \
     script-src 'self' viz:; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     font-src 'self'; \
     connect-src 'self'; \
     base-uri 'none'; \
     form-action 'none'; \
     frame-ancestors 'none'; \
     object-src 'none'";

async fn write_response<S: AsyncWrite + Unpin>(
    stream: &mut S,
    r: &Response,
    etag: Option<&str>,
) -> anyhow::Result<()> {
    let etag = etag.map(|t| format!("ETag: {t}\r\n")).unwrap_or_default();
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\
         {etag}Content-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(r.body.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// An unchanged body: the status and its ETag, and no body (RFC 9110 Section 15.4.5).
async fn write_not_modified<S: AsyncWrite + Unpin>(
    stream: &mut S,
    tag: &str,
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 304 Not Modified\r\nETag: {tag}\r\nConnection: close\r\nCache-Control: no-store\r\n\
         Content-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\n\r\n"
    );
    stream.write_all(header.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// A shipped file. Its path names the release of the files it holds (`/fonts/pretendard-1.3.9/`), so
/// it is cached for a year: a new release of a face is a new path.
async fn write_asset<S: AsyncWrite + Unpin>(
    stream: &mut S,
    kind: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Cache-Control: {ASSET_CACHE}\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        bytes.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(bytes).await?;
    stream.flush().await?;
    Ok(())
}

/// How long a shipped file may be kept: its path changes when its bytes do.
pub const ASSET_CACHE: &str = "public, max-age=31536000, immutable";

/// Minimal percent decoding (`%XX` + `+` -> space). For spaces/special characters in workspace names.
/// Invalid sequences are left as-is (lenient).
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_basics() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        // Invalid sequences keep the original text.
        assert_eq!(percent_decode("a%zz"), "a%zz");
    }
}
