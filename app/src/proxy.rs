//! The transport: the `viz://` proxy onto HTTP over the daemon's unix socket, the policies on what
//! the shell serves, and the SSE bridge that re-emits the daemon's events.

use super::*;

/// What the daemon answered, minus the headers this proxy does not forward.
pub(crate) struct Fetched {
    pub(crate) status: u16,
    pub(crate) ctype: String,
    /// The daemon's Content-Security-Policy, forwarded verbatim rather than restated here.
    ///
    /// The shell is a separate workspace by design ("It shares no code with the server"), so a
    /// shared constant is not available - and a second copy of the policy would be a second thing to
    /// keep in step, which is the failure this proxy already avoids for content-type by reading it
    /// rather than assuming it. The daemon serves the page; the daemon's policy governs it.
    pub(crate) csp: Option<String>,
    /// The ETag of the graph and the hypergraph (design-system.md U33), so the viewer can ask with it.
    pub(crate) etag: Option<String>,
    pub(crate) body: Vec<u8>,
}

/// One GET over the socket. The daemon answers Connection: close, so read-to-EOF terminates.
/// /api/events must never come through here (an endless stream) - the protocol handler
/// short-circuits it.
pub(crate) async fn uds_fetch(sock: &Path, target: &str) -> anyhow::Result<Fetched> {
    uds_request(sock, "GET", target, None).await
}

/// The webview's `If-None-Match`, when it is one this proxy can pass on: printable ASCII and no line
/// break, since it is written into the request head as it comes.
pub(crate) fn forwarded_if_none_match(value: Option<&http::HeaderValue>) -> Option<String> {
    let v = value?.to_str().ok()?;
    (!v.is_empty() && v.bytes().all(|b| (0x20..0x7f).contains(&b))).then(|| v.to_string())
}

/// One request over the socket, with no body - the viewer takes its parameters in the query string,
/// POST included (its one POST is the console's narrowing act, `/api/peer/share`).
pub(crate) async fn uds_request(
    sock: &Path,
    method: &str,
    target: &str,
    if_none_match: Option<&str>,
) -> anyhow::Result<Fetched> {
    let mut s = UnixStream::connect(sock).await?;
    let conditional = if_none_match.map(|t| format!("If-None-Match: {t}\r\n")).unwrap_or_default();
    s.write_all(
        format!(
            "{method} {target} HTTP/1.1\r\n{conditional}Content-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )
    .await?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed response - no header terminator")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .context("malformed status line")?;
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let ctype = header("content-type").unwrap_or_else(|| "application/octet-stream".to_string());
    Ok(Fetched {
        status,
        ctype,
        csp: header("content-security-policy"),
        etag: header("etag"),
        body,
    })
}

/// The policy for responses the SHELL authors rather than proxies: the starting splash and the
/// error JSONs. Strictly tighter than the daemon's - none of them loads a script or an image, and
/// the splash's inline styles are the only reason `style-src` is not `'none'` too.
///
/// It is not a fallback for the viewer page. If the daemon ever answers without a policy, that is a
/// daemon that stopped sending one, and serving its HTML under a shell-authored guess would hide
/// exactly that.
pub(crate) const SHELL_CSP: &str =
    "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; \
     form-action 'none'; frame-ancestors 'none'; object-src 'none'";

/// The origins Tauri's IPC rides on, which the daemon has no reason to know about.
///
/// This is the one place the forwarded policy must be edited rather than passed through. Tauri
/// normally solves it by extending the CSP itself - but it does that for a policy declared in
/// `tauri.conf.json`, and this one arrives as a header on a proxied response, so nothing extends it
/// and `connect-src 'self'` would refuse the transport `window.__TAURI__.event.emit/listen` uses.
/// The window would render and then be deaf to the tray.
///
/// Note what does NOT need an entry: `shell-init.js` is injected as a webview user script rather
/// than fetched by the document, so it is not `script-src`'s business - which is why Tauri apps run
/// under policies with no `'unsafe-inline'` at all.
pub(crate) const TAURI_IPC_SOURCES: &str = "ipc:";

/// Adds the IPC origins to a forwarded policy's `connect-src`.
///
/// A duplicate directive would be ignored (a browser honours the first occurrence), so the existing
/// one is widened in place. A policy with no `connect-src` gets one, because `default-src 'none'`
/// would otherwise deny the fetch by inheritance.
pub(crate) fn with_ipc_sources(csp: &str) -> String {
    if csp.trim().is_empty() {
        return String::new(); // nothing to extend; forward the absence honestly
    }
    let mut found = false;
    let mut out: Vec<String> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            if d == "connect-src" || d.starts_with("connect-src ") {
                found = true;
                format!("{d} {TAURI_IPC_SOURCES}")
            } else {
                d.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("connect-src {TAURI_IPC_SOURCES}"));
    }
    out.join("; ")
}

/// The shell's own files on the viewer's origin (docs/settings-page.md Section 3.0): the chrome
/// stylesheet both pages share. The viewer is the daemon's page and its policy allows styles from
/// its own origin only, so the shell answers this path itself - never passing it to a daemon or a
/// hub - and the stylesheet is the same bytes the settings page links from the app's origin.
pub(crate) fn shell_asset(target: &str) -> Option<(&'static str, &'static [u8])> {
    let path = target.split('?').next().unwrap_or(target);
    match path {
        "/__shell/shell.css" => {
            Some(("text/css; charset=utf-8", include_bytes!("../assets/shell.css")))
        }
        _ => None,
    }
}

pub(crate) fn resp(status: u16, ctype: &str, csp: &str, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("content-type", ctype)
        // The daemon marks every response no-store; the proxy must not launder that away -
        // without it WKWebView disk-caches the viewer assets and serves STALE pages across
        // app restarts (live-updating data and a hand-deployed daemon make caching all wrong).
        .header("cache-control", "no-store")
        // Same laundering concern, higher stakes. The webview renders entity names, descriptions and
        // proposal rationale, which under federation are synced, attacker-influenceable input - so
        // the daemon's Content-Security-Policy is the second line of defence behind output escaping,
        // and a proxy that drops it leaves the shell (the surface a human actually looks at) with
        // only the first. It used to drop it, because it forwarded content-type and nothing else.
        .header("content-security-policy", csp)
        .header("x-content-type-options", "nosniff")
        .body(body)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// Holds the daemon's /api/events SSE stream and re-emits each `data:` frame as a "viz-event"
/// Tauri event (the webview side is the EventSource facade injected at init). Reconnects forever -
/// the daemon may not be up yet, or may restart underneath us; the viewer also polls, so a lost
/// frame degrades liveness, never correctness.
pub(crate) async fn sse_bridge(app: tauri::AppHandle<Rt>, sock: PathBuf) {
    loop {
        if let Ok(mut s) = UnixStream::connect(&sock).await {
            if s.write_all(b"GET /api/events HTTP/1.1\r\n\r\n").await.is_ok() {
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                // One quiet line per connection when the first real frame flows - the log-side
                // proof that daemon events are reaching the webview bridge.
                let mut streamed = false;
                loop {
                    let n = match s.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    // SSE frames end with a blank line ("\n\n"). The response header block ends
                    // with \r\n\r\n (no bare \n\n inside), so it is swept out with the first
                    // frame, and non-"data:" lines (headers, ": ok" keepalive) fall through.
                    while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                        let frame: Vec<u8> = buf.drain(..pos + 2).collect();
                        for line in String::from_utf8_lossy(&frame).lines() {
                            if let Some(json) = line.strip_prefix("data: ") {
                                if !streamed {
                                    streamed = true;
                                    tracing::info!("sse bridge live - first event frame forwarded to the webview");
                                }
                                let _ = app.emit("viz-event", json.to_string());
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
