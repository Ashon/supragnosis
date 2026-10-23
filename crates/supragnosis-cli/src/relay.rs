//! `supragnosis bridge --viewer <socket>` - a remote server's read tier, relayed to the desktop app
//! (docs/remote-viewer.md Section 5).
//!
//! The shell speaks HTTP over a unix socket and holds no TLS client, by design. What reaches a
//! server correctly - the active profile, its CA, the credential read per request, verification
//! that cannot be turned off - lives here, beside the MCP bridge, and shares its client. The relay
//! binds a 0600 socket, forwards GETs of paths under `/viz/` with the credential as a header, asks
//! conditionally, and turns the three failures the shell must show into JSON with a `state`. It runs
//! until its stdin closes, so it cannot outlive the app that started it (V6).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio::io::AsyncRead;

/// The header the relay puts on a state it reports, and only there (Section 5).
pub const RELAY_STATE: &str = "x-supragnosis-relay";

/// How long one read may take before the relay says nothing answered. The hub stops a read at 60
/// seconds and says so itself (remote-viewer.md Section 3.8); this is the backstop behind it.
const READ_TIMEOUT: Duration = Duration::from_secs(70);
/// How many request targets the conditional cache keeps, and how many bytes of bodies.
const CACHE_TARGETS: usize = 64;
const CACHE_BYTES: usize = 32 << 20;
/// The hub's headers an answer keeps. The `ETag` is the relay's own business (it asks
/// conditionally on the page's behalf), and nothing else the hub sends is the shell's.
const KEPT: [HeaderName; 4] = [
    header::CONTENT_TYPE,
    header::CONTENT_SECURITY_POLICY,
    header::X_CONTENT_TYPE_OPTIONS,
    header::CACHE_CONTROL,
];

/// What the relay reads, and how it names it.
pub struct Config {
    /// The profile's name, as `supragnosis server` lists it.
    pub profile: String,
    /// The read tier's address, ending in `/viz/` ([`viz_url`]).
    pub viz: reqwest::Url,
    /// Reads the credential, on every request (V5).
    pub token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// Where the credential is read from, named when the server refuses it.
    pub token_source: String,
    /// A private CA's PEM bundle, as the profile names it.
    pub ca_pem: Option<Vec<u8>>,
}

/// The relay's configuration for the active profile. On the local profile there is nothing to
/// relay: the local viewer is the daemon's own socket.
pub fn config(
    target: crate::profile::Target,
    read_token: impl Fn(&Path) -> Option<String> + Send + Sync + 'static,
) -> Result<Config, String> {
    let crate::profile::Target::Remote { name, url, ca, token_file } = target else {
        return Err("the active server is this machine's daemon, whose viewer is its own socket - \
                    there is nothing to relay. `supragnosis server use <name>` picks a remote server"
            .into());
    };
    let viz = viz_url(&url)?;
    let ca_pem = match ca {
        Some(p) => Some(std::fs::read(&p).map_err(|e| format!("reading CA {}: {e}", p.display()))?),
        None => None,
    };
    let token_source = token_file.display().to_string();
    let file: PathBuf = token_file;
    Ok(Config {
        profile: name,
        viz,
        token: Arc::new(move || read_token(&file)),
        token_source,
        ca_pem,
    })
}

/// The read tier's address for a profile's MCP URL: its last path segment `mcp` replaced by
/// `viz/`. A URL that does not end in `mcp` is refused rather than guessed at, and so is one that
/// would carry the credential unencrypted off this machine (R8).
pub fn viz_url(mcp: &str) -> Result<reqwest::Url, String> {
    let u = reqwest::Url::parse(mcp).map_err(|e| format!("{mcp:?} is not a URL: {e}"))?;
    let loopback = u.host_str().is_some_and(crate::profile::is_loopback_host);
    if !(u.scheme() == "https" || u.scheme() == "http" && loopback) {
        return Err(format!(
            "{mcp:?} is neither https nor plain http to this machine - the credential would cross \
             the network unencrypted"
        ));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(format!(
            "the profile's URL for {} carries a username or password - the credential belongs in \
             its file, and a URL is shown to the page",
            u.host_str().unwrap_or("its server")
        ));
    }
    let path = u.path().trim_end_matches('/');
    let Some(base) = path.strip_suffix("/mcp") else {
        return Err(format!(
            "the profile's URL {mcp:?} does not end in /mcp, so where its viewer is cannot be told - \
             give the server's MCP URL"
        ));
    };
    let mut viz = u.clone();
    viz.set_path(&format!("{base}/viz/"));
    viz.set_query(None);
    viz.set_fragment(None);
    Ok(viz)
}

/// Refuses a socket path in a directory other users can enter: the socket's own 0600 mode is the
/// access control, and a shared directory would let another user replace the socket file.
pub fn check_socket_dir(socket: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let Some(dir) = socket.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return Err(format!("{} names no directory", socket.display()));
    };
    match std::fs::metadata(dir) {
        // Created 0700 when the socket is bound.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
        Ok(m) if m.permissions().mode() & 0o077 != 0 => Err(format!(
            "{} can be entered by other users (mode {:o}) - the relay's socket belongs in a \
             private directory, such as ~/.supragnosis",
            dir.display(),
            m.permissions().mode() & 0o777
        )),
        Ok(_) => Ok(()),
    }
}

/// After the bind: the socket's directory must belong to the user the socket does, and stay closed
/// to others. A directory someone else owns could be opened up after [`check_socket_dir`] looked,
/// and the socket in it swapped for one serving another page.
pub fn bound_privately(socket: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let sock =
        std::fs::symlink_metadata(socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let dir = socket.parent().unwrap_or(Path::new("."));
    let d = std::fs::metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if d.uid() != sock.uid() || d.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "{} is not a private directory of this user's - the relay's socket belongs in one, \
             such as ~/.supragnosis",
            dir.display()
        ));
    }
    Ok(())
}

/// One answer the conditional cache keeps.
#[derive(Clone)]
struct Cached {
    etag: String,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Bytes,
}

/// The last successful answer for each request target, least recently used first.
#[derive(Default)]
struct Cache {
    order: VecDeque<String>,
    entries: HashMap<String, Cached>,
    bytes: usize,
}

impl Cache {
    fn get(&mut self, key: &str) -> Option<Cached> {
        let hit = self.entries.get(key)?.clone();
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(hit)
    }

    fn put(&mut self, key: String, entry: Cached) {
        self.remove(&key);
        if entry.body.len() > CACHE_BYTES {
            return;
        }
        self.bytes += entry.body.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, entry);
        while self.entries.len() > CACHE_TARGETS || self.bytes > CACHE_BYTES {
            let Some(oldest) = self.order.front().cloned() else {
                break;
            };
            self.remove(&oldest);
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(old) = self.entries.remove(key) {
            self.bytes -= old.body.len();
            self.order.retain(|k| k != key);
        }
    }
}

struct Relay {
    cfg: Config,
    http: reqwest::Client,
    cache: Mutex<Cache>,
}

/// Serves the relay on `listener` until `input` ends, then unlinks `socket` - while the listener is
/// still held. Until then a second relay is refused the live path (`bind_uds` probes it), so the
/// file removed is always this relay's own, never the next profile's, which binds only after.
pub async fn run<R>(
    cfg: Config,
    listener: tokio::net::UnixListener,
    input: R,
    socket: Option<PathBuf>,
) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin,
{
    use std::future::IntoFuture;
    let http = crate::bridge::client_builder(cfg.ca_pem.as_deref())?
        .connect_timeout(Duration::from_secs(5))
        // A redirect is the hub's answer to show, not one to follow with the credential attached.
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let relay = Arc::new(Relay { cfg, http, cache: Mutex::default() });
    let app = axum::Router::new().fallback(handle).with_state(relay);
    let (mut input, mut sink) = (input, tokio::io::sink());
    let serve = axum::serve(listener, app).into_future();
    tokio::pin!(serve);
    let outcome = tokio::select! {
        served = &mut serve => served.map_err(anyhow::Error::from),
        // The app holds the other end of this pipe: when it quits or crashes, the relay stops,
        // open event streams and all (V6).
        _ = tokio::io::copy(&mut input, &mut sink) => Ok(()),
    };
    // `serve`, pinned on this frame, still holds the listener: it is let go only when the function
    // returns, after the unlink.
    if let Some(path) = socket {
        let _ = std::fs::remove_file(path);
    }
    outcome
}

async fn handle(
    State(relay): State<Arc<Relay>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if method != Method::GET {
        return said(
            StatusCode::METHOD_NOT_ALLOWED,
            "the relay forwards GET only - the read tier changes nothing (remote-viewer.md V2)",
        );
    }
    let path = uri.path();
    let Some(url) = relay.target(path, uri.query()) else {
        return said(
            StatusCode::BAD_REQUEST,
            "a path with a . or .. segment, a backslash or an encoded separator is not relayed - \
             every path stays under /viz/",
        );
    };
    if path == "/api/events" {
        return relay.stream(url).await;
    }
    let key = uri.path_and_query().map_or(path, |pq| pq.as_str()).to_string();
    let asked = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
    relay.read(&key, path, url, headers.get(header::ACCEPT).cloned(), asked).await
}

impl Relay {
    /// The hub's address for a request path, or None for one that could leave `/viz/`: a `.` or
    /// `..` segment, as typed or percent-encoded, a backslash, which a URL parser may read as a
    /// separator, or an encoded slash or backslash, which a decoding proxy in front of the hub could.
    fn target(&self, path: &str, query: Option<&str>) -> Option<reqwest::Url> {
        let dot = |seg: &str| {
            let s = seg.to_ascii_lowercase();
            let d = s.replace("%2e", ".");
            d == "." || d == ".." || s.contains("%2f") || s.contains("%5c")
        };
        if !path.starts_with('/') || path.contains('\\') || path.split('/').any(dot) {
            return None;
        }
        let mut s = format!("{}{path}", self.cfg.viz.as_str().trim_end_matches('/'));
        if let Some(q) = query {
            s.push('?');
            s.push_str(q);
        }
        let url = reqwest::Url::parse(&s).ok()?;
        let base = self.cfg.viz.path();
        let inside = url.path() == base.trim_end_matches('/') || url.path().starts_with(base);
        (inside && url.origin() == self.cfg.viz.origin()).then_some(url)
    }

    fn request(&self, url: reqwest::Url) -> reqwest::RequestBuilder {
        let req = self.http.get(url);
        // Read now, never kept: a credential replaced on disk is used by the next request (V5).
        match (self.cfg.token)() {
            Some(t) => req.bearer_auth(t),
            None => req,
        }
    }

    /// One read, asked conditionally when a body for it is held (Section 3.8), and answered to the
    /// page conditionally too: the hub's `ETag` goes with the answer, and a page asking with the tag
    /// of what it holds is answered 304 (design-system.md U33). The page's own tag is answered
    /// here, never sent on.
    async fn read(
        &self,
        key: &str,
        path: &str,
        url: reqwest::Url,
        accept: Option<HeaderValue>,
        asked: Option<&str>,
    ) -> Response {
        // The held answer itself, not only its tag: a 304 is then answered from it however the cache
        // changes while the hub is asked.
        let held = self.cache.lock().unwrap().get(key);
        let mut req = self.request(url).timeout(READ_TIMEOUT);
        if let Some(a) = &accept {
            req = req.header(header::ACCEPT, a);
        }
        if let Some(h) = &held {
            req = req.header(header::IF_NONE_MATCH, &h.etag);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return self.unreachable(&e),
        };
        let status = resp.status();
        if let Some(failure) = self.failure_of(status, path) {
            return failure;
        }
        if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
            return self.redirected(&resp);
        }
        let fresh = if status == StatusCode::NOT_MODIFIED {
            match held {
                Some(h) => h,
                None => {
                    return self
                        .unreachable_said("the server answered 304 to a read that held nothing")
                }
            }
        } else {
            let headers = kept(resp.headers());
            let etag = resp.headers().get(header::ETAG).and_then(|v| v.to_str().ok());
            let etag = etag.map(str::to_string);
            let body = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => return self.unreachable(&e),
            };
            let Some(etag) = etag.filter(|_| status == StatusCode::OK) else {
                return self.answer(path, status, headers, body);
            };
            let entry = Cached { etag, headers, body };
            self.cache.lock().unwrap().put(key.to_string(), entry.clone());
            entry
        };
        // `/api/surface` goes out changed (the profile is added), so the hub's tag, which is over
        // the hub's body, is not the page's: it goes with no tag and is never answered 304.
        let tag = HeaderValue::from_str(&fresh.etag).ok().filter(|_| path != "/api/surface");
        if tag.is_some() && asked == Some(fresh.etag.as_str()) {
            let mut headers = fresh.headers;
            headers.retain(|(h, _)| h != header::CONTENT_TYPE);
            headers.extend(tag.map(|t| (header::ETAG, t)));
            return respond(StatusCode::NOT_MODIFIED, headers, Body::empty());
        }
        let mut headers = fresh.headers;
        headers.extend(tag.map(|t| (header::ETAG, t)));
        self.answer(path, StatusCode::OK, headers, fresh.body)
    }

    /// The event stream, passed through as it arrives (Section 3.6). No timeout: the page holds it
    /// open on purpose, and the relay's stdin ends it.
    async fn stream(&self, url: reqwest::Url) -> Response {
        let req = self.request(url).header(header::ACCEPT, "text/event-stream");
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return self.unreachable(&e),
        };
        if let Some(failure) = self.failure_of(resp.status(), "/api/events") {
            return failure;
        }
        if resp.status().is_redirection() {
            return self.redirected(&resp);
        }
        let (status, headers) = (resp.status(), kept(resp.headers()));
        let chunks = futures_util::stream::unfold(resp, |mut r| async move {
            // An error ends the stream, as a closed one does; the page reconnects and polls anyway.
            match r.chunk().await {
                Ok(Some(b)) => Some((Ok::<Bytes, std::io::Error>(b), r)),
                _ => None,
            }
        });
        respond(status, headers, Body::from_stream(chunks))
    }

    /// The two failures a status alone tells (Section 5): a refused credential, and a hub older than
    /// the read tier. Any other answer is the hub's, and passes through.
    fn failure_of(&self, status: StatusCode, path: &str) -> Option<Response> {
        if status == StatusCode::UNAUTHORIZED {
            let name = &self.cfg.profile;
            return Some(self.failure(
                StatusCode::UNAUTHORIZED,
                "credential_refused",
                json!({
                    "credential_file": self.cfg.token_source,
                    "error": format!("{name} refused this machine's credential"),
                }),
            ));
        }
        if status == StatusCode::NOT_FOUND && (path == "/" || path == "/api/surface") {
            let error = format!(
                "{} runs Supragnosis older than 0.4.7, which has no viewer to show",
                self.cfg.profile
            );
            return Some(self.failure(StatusCode::NOT_FOUND, "too_old", json!({ "error": error })));
        }
        None
    }

    /// A redirect, reported rather than followed: the credential goes nowhere but the profile's
    /// address (Section 5).
    fn redirected(&self, resp: &reqwest::Response) -> Response {
        let to = resp.headers().get(header::LOCATION).and_then(|v| v.to_str().ok());
        self.unreachable_said(&format!(
            "the server answered {} toward {} - the relay follows no redirect, so the credential \
             goes nowhere but the profile's address",
            resp.status(),
            to.unwrap_or("nowhere it named")
        ))
    }

    fn unreachable(&self, e: &reqwest::Error) -> Response {
        self.unreachable_said(&describe(e))
    }

    fn unreachable_said(&self, detail: &str) -> Response {
        let error = format!("{} is not answering - {detail}", self.cfg.profile);
        self.failure(
            StatusCode::BAD_GATEWAY,
            "unreachable",
            json!({ "detail": detail, "error": error }),
        )
    }

    /// A state the shell shows as a page (Section 5), with `error`, the sentence a page's poll says
    /// when it meets the state in the middle of a visit (design-system.md Section 29.31).
    fn failure(&self, status: StatusCode, state: &str, extra: Value) -> Response {
        let mut body = json!({
            "state": state,
            "server": self.cfg.profile,
            "url": self.cfg.viz.as_str(),
        });
        if let (Some(b), Value::Object(more)) = (body.as_object_mut(), extra) {
            b.extend(more);
        }
        // The mark of an answer the relay made itself: the hub's answers pass with the headers KEPT
        // names only, so no hub can make the shell draw a state's page (design-system.md 29.31).
        let mark = HeaderValue::from_str(state).unwrap_or(HeaderValue::from_static("unreachable"));
        let headers = vec![
            (header::CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (HeaderName::from_static(RELAY_STATE), mark),
        ];
        respond(status, headers, Body::from(body.to_string()))
    }

    /// An answer to the shell. `/api/surface` gains the profile (Section 3.5): what this machine
    /// calls the server, which the hub cannot know.
    fn answer(
        &self,
        path: &str,
        status: StatusCode,
        headers: Vec<(HeaderName, HeaderValue)>,
        body: Bytes,
    ) -> Response {
        if path == "/api/surface" && status == StatusCode::OK {
            if let Ok(Value::Object(mut s)) = serde_json::from_slice::<Value>(&body) {
                s.insert(
                    "profile".into(),
                    json!({ "name": self.cfg.profile, "url": self.cfg.viz.as_str() }),
                );
                return respond(status, headers, Body::from(Value::Object(s).to_string()));
            }
        }
        respond(status, headers, Body::from(body))
    }
}

fn kept(from: &reqwest::header::HeaderMap) -> Vec<(HeaderName, HeaderValue)> {
    KEPT.iter().filter_map(|h| Some((h.clone(), from.get(h)?.clone()))).collect()
}

fn respond(status: StatusCode, headers: Vec<(HeaderName, HeaderValue)>, body: Body) -> Response {
    let mut r = (status, body).into_response();
    r.headers_mut().extend(headers);
    r
}

/// A refusal by the relay itself, in the read tier's shape.
fn said(status: StatusCode, why: &str) -> Response {
    let headers = vec![(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))];
    respond(status, headers, Body::from(json!({ "error": why }).to_string()))
}

/// What failed when nothing usable answered, in words: a refused connection, a certificate that did
/// not verify, or no answer in time. reqwest's own message names the URL; its sources say why.
fn describe(e: &reqwest::Error) -> String {
    let mut why = Vec::new();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        why.push(s.to_string());
        source = s.source();
    }
    let why = why.join(": ");
    if why.to_ascii_lowercase().contains("certificate") {
        format!(
            "the server's certificate did not verify ({why}) - it is checked against the root \
             bundle built into supragnosis, or the profile's CA when it names one, and nothing turns \
             that off"
        )
    } else if e.is_connect() {
        // Before the timeout: a connect that timed out is both, and it is the connect that failed.
        format!("nothing accepted the connection ({why})")
    } else if e.is_timeout() {
        format!("no answer within {} seconds", READ_TIMEOUT.as_secs())
    } else if why.is_empty() {
        e.to_string()
    } else {
        format!("{e} ({why})")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::profile::Target;
    use axum::http::header::{AUTHORIZATION, COOKIE, IF_NONE_MATCH};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A private directory for one test's socket and credential.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sg-relay-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::private_dir(&dir).unwrap();
        dir
    }

    /// What the stand-in hub was asked.
    #[derive(Default)]
    struct Seen {
        viz: bool,
        auth: Mutex<Vec<String>>,
        cookies: AtomicUsize,
        /// The `If-None-Match` and `Accept` of every request, as the hub received them.
        tags: Mutex<Vec<String>>,
        accepts: Mutex<Vec<String>>,
        graph_bodies: AtomicUsize,
        mcp: AtomicUsize,
    }

    /// A hub's read tier in miniature: alice's credential is `good-*`; the graph answers
    /// conditionally; the event stream sends one frame and stays open. With `viz` off it is a hub
    /// from before the read tier.
    async fn stand_in(State(seen): State<Arc<Seen>>, uri: Uri, headers: HeaderMap) -> Response {
        let auth = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
        seen.auth.lock().unwrap().push(auth.to_string());
        if headers.contains_key(COOKIE) {
            seen.cookies.fetch_add(1, Ordering::SeqCst);
        }
        let text = |h| headers.get(h).and_then(|v: &HeaderValue| v.to_str().ok()).map(String::from);
        seen.tags.lock().unwrap().extend(text(IF_NONE_MATCH));
        seen.accepts.lock().unwrap().extend(text(header::ACCEPT));
        let path = uri.path();
        if path == "/mcp" {
            seen.mcp.fetch_add(1, Ordering::SeqCst);
            return "mcp".into_response();
        }
        if !seen.viz || !path.starts_with("/viz/") {
            return (StatusCode::NOT_FOUND, "not found").into_response();
        }
        if !auth.starts_with("Bearer good") {
            return (StatusCode::UNAUTHORIZED, "who are you").into_response();
        }
        let json = |status: StatusCode, v: Value| {
            (status, [(header::CONTENT_TYPE, "application/json")], v.to_string()).into_response()
        };
        match path {
            "/viz/" => (
                [
                    (header::CONTENT_TYPE, "text/html"),
                    (header::CONTENT_SECURITY_POLICY, "default-src 'self'"),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                "<!doctype html>the page",
            )
                .into_response(),
            "/viz/api/surface" => {
                let tag = "\"s1\"";
                if headers.get(IF_NONE_MATCH).is_some_and(|v| v == tag) {
                    return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
                }
                let body = json!({"surface": "remote", "principal": "alice", "read_only": true});
                (
                    [(header::CONTENT_TYPE, "application/json"), (header::ETAG, tag)],
                    body.to_string(),
                )
                    .into_response()
            }
            "/viz/api/moved" => {
                (StatusCode::FOUND, [(header::LOCATION, "/mcp")], "over there").into_response()
            }
            "/viz/api/always304" => StatusCode::NOT_MODIFIED.into_response(),
            "/viz/api/graph" => {
                let tag = "\"g1\"";
                if headers.get(IF_NONE_MATCH).is_some_and(|v| v == tag) {
                    return (StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response();
                }
                seen.graph_bodies.fetch_add(1, Ordering::SeqCst);
                let body = json!({"nodes": [{"id": "n1"}], "query": uri.query()}).to_string();
                ([(header::CONTENT_TYPE, "application/json"), (header::ETAG, tag)], body)
                    .into_response()
            }
            "/viz/api/explain" => json(
                StatusCode::NOT_FOUND,
                json!({"error": "no entity with that id in the workspaces you may read"}),
            ),
            "/viz/api/events" => {
                use futures_util::StreamExt;
                let frame = Bytes::from_static(b"data: {\"kind\":\"observe\"}\n\n");
                let open =
                    futures_util::stream::once(async move { Ok::<_, std::io::Error>(frame) })
                        .chain(futures_util::stream::pending());
                ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(open))
                    .into_response()
            }
            _ => (StatusCode::NOT_FOUND, "not found").into_response(),
        }
    }

    fn stand_in_app(seen: Arc<Seen>) -> axum::Router {
        axum::Router::new().fallback(stand_in).with_state(seen)
    }

    /// Serves the stand-in on loopback plain HTTP; returns the profile URL a person would add.
    async fn hub(viz: bool) -> (String, Arc<Seen>) {
        let seen = Arc::new(Seen { viz, ..Default::default() });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = stand_in_app(seen.clone());
        tokio::spawn(async move { axum::serve(listener, app).await });
        (format!("http://{addr}/mcp"), seen)
    }

    struct Running {
        socket: PathBuf,
        token_file: PathBuf,
        /// The app's end of the relay's stdin: dropping it is the app quitting.
        stdin: Option<tokio::io::DuplexStream>,
        done: tokio::task::JoinHandle<anyhow::Result<()>>,
    }

    async fn relay_to(dir: &Path, mcp: &str, token: &str, ca: Option<Vec<u8>>) -> Running {
        let token_file = dir.join("home.token");
        std::fs::write(&token_file, token).unwrap();
        let target = Target::Remote {
            name: "home".into(),
            url: mcp.into(),
            ca: None,
            token_file: token_file.clone(),
        };
        let mut cfg = config(target, crate::read_secret).unwrap();
        cfg.ca_pem = ca;
        let socket = dir.join("v.sock");
        let listener = supragnosis_viz::bind_uds(&socket).await.unwrap();
        let (stdin, input) = tokio::io::duplex(64);
        let done = tokio::spawn(run(cfg, listener, input, Some(socket.clone())));
        Running { socket, token_file, stdin: Some(stdin), done }
    }

    /// One request over the relay's socket, as the shell sends it: (status, head, body).
    pub(crate) async fn ask(
        socket: &Path,
        method: &str,
        target: &str,
        extra: &str,
    ) -> (u16, String, String) {
        let mut s = tokio::net::UnixStream::connect(socket).await.unwrap();
        let req = format!(
            "{method} {target} HTTP/1.1\r\nHost: relay\r\n{extra}Connection: close\r\n\r\n"
        );
        s.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, head.to_ascii_lowercase(), body.to_string())
    }

    async fn head_of(socket: &Path, path: &str) -> String {
        ask(socket, "GET", path, "").await.1
    }

    fn state_of(body: &str) -> Value {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
    }

    /// remote-viewer.md V5/V6: the relay forwards GETs of paths under /viz/ only, with the
    /// credential read from its file on every request and sent only as a header; the page's own
    /// credential and cookies stay on this machine.
    #[tokio::test]
    async fn the_relay_forwards_gets_under_viz_with_a_credential_read_per_request() {
        let dir = scratch("forward");
        let (mcp, seen) = hub(true).await;
        let r = relay_to(&dir, &mcp, "good-1", None).await;

        let (status, head, body) = ask(&r.socket, "GET", "/", "").await;
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("the page"));
        for kept in [
            "content-security-policy: default-src 'self'",
            "x-content-type-options: nosniff",
            "cache-control: no-cache",
        ] {
            assert!(head.contains(kept), "the hub's {kept}: {head}");
        }
        std::fs::write(&r.token_file, "good-2").unwrap();
        let held =
            "Authorization: Bearer held-by-the-page\r\nCookie: a=b\r\nAccept: application/json\r\n";
        assert_eq!(ask(&r.socket, "GET", "/api/surface", held).await.0, 200);
        let accepts = seen.accepts.lock().unwrap().clone();
        assert_eq!(accepts.last().map(String::as_str), Some("application/json"), "{accepts:?}");
        assert_eq!(
            *seen.auth.lock().unwrap(),
            vec!["Bearer good-1".to_string(), "Bearer good-2".to_string()],
            "read on every request, and only the relay's credential"
        );
        assert_eq!(seen.cookies.load(Ordering::SeqCst), 0, "the page's cookies stay here");

        for method in ["POST", "PUT", "DELETE"] {
            assert_eq!(ask(&r.socket, method, "/api/graph", "").await.0, 405, "{method}");
        }
        for escape in [
            "/../mcp",
            "/%2e%2e/mcp",
            "/api/%2E%2E/%2e%2e/mcp",
            "/./api/graph",
            "/api\\..\\..\\mcp",
            "/api/..%2fmcp",
            "/api/..%5Cmcp",
        ] {
            let (status, _, body) = ask(&r.socket, "GET", escape, "").await;
            assert_eq!(status, 400, "{escape}: {body}");
        }
        assert_eq!(seen.mcp.load(Ordering::SeqCst), 0, "nothing reached /mcp");
        assert_eq!(seen.auth.lock().unwrap().len(), 2, "a refused request sends nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// remote-viewer.md Section 3.8 and design-system.md U33: the relay asks with the ETag it holds
    /// and answers a 304 with the body it held; it answers the page conditionally too, from the
    /// hub's tag, and never sends the page's tag on. Each request target keeps its own.
    #[tokio::test]
    async fn the_relay_asks_conditionally_and_answers_the_page_so() {
        let dir = scratch("etag");
        let (mcp, seen) = hub(true).await;
        let r = relay_to(&dir, &mcp, "good", None).await;
        let first = ask(&r.socket, "GET", "/api/graph?workspace=team", "").await;
        assert_eq!(first.0, 200);
        assert!(first.1.contains("etag: \"g1\""), "the hub's tag goes to the page: {}", first.1);
        assert!(first.2.contains("workspace=team"), "the query passes as it came: {}", first.2);
        let other = "If-None-Match: \"made-by-the-page\"\r\n";
        let second = ask(&r.socket, "GET", "/api/graph?workspace=team", other).await;
        assert_eq!((second.0, &second.2), (200, &first.2), "a tag that is not the hub's: the body");
        assert_eq!(seen.graph_bodies.load(Ordering::SeqCst), 1, "answered from the held body");
        let current = "If-None-Match: \"g1\"\r\n";
        let third = ask(&r.socket, "GET", "/api/graph?workspace=team", current).await;
        assert_eq!((third.0, third.2.as_str()), (304, ""), "unchanged: nothing for the page");
        assert!(third.1.contains("etag: \"g1\""));
        assert!(
            !seen.tags.lock().unwrap().iter().any(|t| t.contains("made-by-the-page")),
            "the page's tag is answered here, never sent on"
        );
        assert_eq!(seen.graph_bodies.load(Ordering::SeqCst), 1, "one transfer for three reads");
        assert_eq!(ask(&r.socket, "GET", "/api/graph?workspace=docs", "").await.0, 200);
        assert_eq!(seen.graph_bodies.load(Ordering::SeqCst), 2, "another target, another body");

        let (status, _, body) = ask(&r.socket, "GET", "/api/always304", "").await;
        assert_eq!((status, state_of(&body)["state"].as_str()), (502, Some("unreachable")));
        assert!(body.contains("held nothing"), "a 304 to a read that held nothing: {body}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// remote-viewer.md Section 5: a redirect is reported, never followed, so the credential goes
    /// nowhere but the profile's address - not even to another path of the same server.
    #[tokio::test]
    async fn the_relay_follows_no_redirect() {
        let dir = scratch("redirect");
        let (mcp, seen) = hub(true).await;
        let r = relay_to(&dir, &mcp, "good", None).await;
        let (status, _, body) = ask(&r.socket, "GET", "/api/moved", "").await;
        let v = state_of(&body);
        assert_eq!((status, v["state"].as_str()), (502, Some("unreachable")), "{body}");
        assert!(v["detail"].as_str().unwrap().contains("/mcp"), "the target is named: {body}");
        assert_eq!(seen.mcp.load(Ordering::SeqCst), 0, "and not followed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The conditional cache is bounded: 64 targets and 32 MiB, least recently used dropped first,
    /// and a body larger than the whole is not kept.
    #[test]
    fn the_cache_keeps_at_most_64_targets_and_32_mib() {
        let entry =
            |n: usize| Cached { etag: "e".into(), headers: vec![], body: vec![0u8; n].into() };
        let mut c = Cache::default();
        for i in 0..CACHE_TARGETS {
            c.put(format!("/t{i}"), entry(1));
        }
        assert!(c.get("/t0").is_some(), "t0 is now the most recent");
        c.put("/new".into(), entry(1));
        assert!(c.get("/t1").is_none(), "the least recently used went");
        assert!(c.get("/t0").is_some() && c.get("/new").is_some());
        assert_eq!(c.entries.len(), CACHE_TARGETS);

        let mut c = Cache::default();
        c.put("/a".into(), entry(CACHE_BYTES / 2));
        c.put("/b".into(), entry(CACHE_BYTES / 2));
        c.put("/c".into(), entry(1));
        assert!(c.get("/a").is_none(), "over the byte bound, the oldest went");
        assert!(c.bytes <= CACHE_BYTES);
        c.put("/huge".into(), entry(CACHE_BYTES + 1));
        assert!(c.get("/huge").is_none(), "too large to keep at all");
        assert!(c.get("/b").is_some(), "and it evicted nothing");
    }

    /// remote-viewer.md Section 5: each failure the shell must show comes back as JSON with its
    /// state, naming the profile and the address - never as an empty answer; any other answer is
    /// the hub's and passes through.
    #[tokio::test]
    async fn the_relay_names_each_failure_state() {
        let dir = scratch("states");
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let nobody = relay_to(&dir, &format!("http://{closed}/mcp"), "good", None).await;
        let (status, _, body) = ask(&nobody.socket, "GET", "/", "").await;
        let v = state_of(&body);
        assert_eq!((status, v["state"].as_str()), (502, Some("unreachable")), "{body}");
        assert_eq!(v["server"], "home");
        assert_eq!(v["url"], format!("http://{closed}/viz/"));
        assert!(v["detail"].as_str().unwrap().contains("nothing accepted"), "{body}");
        assert!(v["error"].as_str().unwrap().starts_with("home is not answering - "), "{body}");
        assert!(head_of(&nobody.socket, "/").await.contains("x-supragnosis-relay: unreachable"));
        drop(nobody);

        let dir = scratch("states-refused");
        let (mcp, _) = hub(true).await;
        let refused = relay_to(&dir, &mcp, "a-wrong-secret", None).await;
        let (status, _, body) = ask(&refused.socket, "GET", "/", "").await;
        let v = state_of(&body);
        assert_eq!((status, v["state"].as_str()), (401, Some("credential_refused")), "{body}");
        assert_eq!(v["credential_file"], refused.token_file.display().to_string());
        assert!(!body.contains("a-wrong-secret"), "the file is named, never its contents");
        assert_eq!(v["error"], "home refused this machine's credential");

        let dir = scratch("states-old");
        let (mcp, _) = hub(false).await;
        let old = relay_to(&dir, &mcp, "good", None).await;
        for path in ["/", "/api/surface"] {
            let (status, _, body) = ask(&old.socket, "GET", path, "").await;
            let v = state_of(&body);
            assert_eq!((status, v["state"].as_str()), (404, Some("too_old")));
            assert!(v["error"].as_str().unwrap().contains("older than 0.4.7"), "{body}");
        }

        let dir = scratch("states-pass");
        let (mcp, _) = hub(true).await;
        let ok = relay_to(&dir, &mcp, "good", None).await;
        let (status, head, body) = ask(&ok.socket, "GET", "/api/explain?entity=x", "").await;
        assert_eq!(status, 404);
        assert!(!head.contains("x-supragnosis-relay"), "the hub's answer carries no relay mark");
        assert!(state_of(&body)["state"].is_null(), "an unknown id is the hub's answer: {body}");
        assert!(body.contains("no entity with that id"));
        for tag in ["states", "states-refused", "states-old", "states-pass"] {
            let _ = std::fs::remove_dir_all(scratch(tag));
        }
    }

    /// remote-viewer.md Section 3.5: `/api/surface` gains the profile - this machine's name for the
    /// server and the address the relay reads - and keeps everything the hub said.
    #[tokio::test]
    async fn the_relay_adds_the_profile_to_the_surface() {
        let dir = scratch("surface");
        let (mcp, _) = hub(true).await;
        let r = relay_to(&dir, &mcp, "good", None).await;
        for read in ["fresh", "answered from the held body"] {
            let v = state_of(&ask(&r.socket, "GET", "/api/surface", "").await.2);
            assert_eq!(
                (v["principal"].as_str(), v["read_only"].as_bool()),
                (Some("alice"), Some(true))
            );
            assert_eq!(v["profile"]["name"], "home", "{read}");
            assert_eq!(v["profile"]["url"], mcp.replace("/mcp", "/viz/"), "{read}");
        }
        let (status, head, _) =
            ask(&r.socket, "GET", "/api/surface", "If-None-Match: \"s1\"\r\n").await;
        assert_eq!(status, 200, "the surface is changed on the way, so the hub's tag is not its");
        assert!(!head.contains("etag:"), "{head}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// remote-viewer.md Sections 3.6 and 5, V6: the event stream passes through as it arrives, and
    /// the relay stops when the app's end of its stdin closes, with a stream still open.
    #[tokio::test]
    async fn the_relay_streams_events_and_stops_with_its_stdin() {
        let dir = scratch("events");
        let (mcp, _) = hub(true).await;
        let mut r = relay_to(&dir, &mcp, "good", None).await;
        let mut s = tokio::net::UnixStream::connect(&r.socket).await.unwrap();
        s.write_all(b"GET /api/events HTTP/1.1\r\nHost: relay\r\n\r\n").await.unwrap();
        let mut seen = Vec::new();
        let arrived = tokio::time::timeout(Duration::from_secs(5), async {
            let mut buf = [0u8; 1024];
            while !String::from_utf8_lossy(&seen).contains("data: {\"kind\":\"observe\"}") {
                let n = s.read(&mut buf).await.unwrap();
                assert!(n > 0, "the stream closed early");
                seen.extend_from_slice(&buf[..n]);
            }
        })
        .await;
        assert!(arrived.is_ok(), "the frame arrived while the stream stayed open");
        assert!(String::from_utf8_lossy(&seen)
            .to_ascii_lowercase()
            .contains("text/event-stream"));

        drop(r.stdin.take());
        let stopped = tokio::time::timeout(Duration::from_secs(5), &mut r.done).await;
        assert!(matches!(stopped, Ok(Ok(Ok(())))), "the relay stopped with its stdin");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// remote-viewer.md V6 and R8: the relay reaches a server over TLS that verifies - against the
    /// root bundle built into supragnosis, or the profile's CA - and a certificate that does not
    /// verify is a failure the shell can show, with nothing sent.
    #[tokio::test]
    async fn the_relay_reaches_a_server_only_over_verified_tls() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = scratch("tls");
        let (cert, key) = crate::invite::make_certificate(
            &dir.join("hub"),
            &["localhost".into()],
            crate::invite::now(),
        )
        .unwrap();
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key).await.unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Seen { viz: true, ..Default::default() });
        let app = stand_in_app(seen.clone());
        tokio::spawn(async move {
            axum_server::from_tcp_rustls(listener, tls)
                .unwrap()
                .serve(app.into_make_service())
                .await
        });
        let mcp = format!("https://localhost:{port}/mcp");

        let untrusted = relay_to(&dir, &mcp, "good", None).await;
        let (status, _, body) = ask(&untrusted.socket, "GET", "/", "").await;
        let v = state_of(&body);
        assert_eq!((status, v["state"].as_str()), (502, Some("unreachable")), "{body}");
        assert!(v["detail"].as_str().unwrap().contains("certificate"), "{body}");
        assert!(seen.auth.lock().unwrap().is_empty(), "nothing crossed an unverified connection");
        drop(untrusted);

        let dir2 = scratch("tls-ca");
        let pem = std::fs::read(&cert).unwrap();
        let trusted = relay_to(&dir2, &mcp, "good", Some(pem)).await;
        let (status, _, body) = ask(&trusted.socket, "GET", "/", "").await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(*seen.auth.lock().unwrap(), vec!["Bearer good".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// remote-viewer.md Section 5: a relay unlinks its socket while it still holds it - until then
    /// a second relay is refused the live path, so the stopping relay never removes the next one's -
    /// and a socket in a directory others can enter is refused after the bind as before it.
    #[tokio::test]
    async fn a_relay_unlinks_its_socket_while_it_holds_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("unlink");
        let (mcp, _) = hub(true).await;
        let mut r = relay_to(&dir, &mcp, "good", None).await;
        bound_privately(&r.socket).unwrap();
        let refused = supragnosis_viz::bind_uds(&r.socket).await;
        assert!(refused.is_err(), "a second relay is refused the live socket");
        drop(r.stdin.take());
        let stopped = tokio::time::timeout(Duration::from_secs(5), &mut r.done).await;
        assert!(matches!(stopped, Ok(Ok(Ok(())))));
        assert!(!r.socket.exists(), "unlinked on the way out");
        let _next = supragnosis_viz::bind_uds(&r.socket).await.expect("the next relay binds");

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opened = bound_privately(&r.socket).unwrap_err();
        assert!(opened.contains("not a private directory"), "{opened}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// remote-viewer.md Section 5: on the local profile there is nothing to relay; the viewer's
    /// address is the MCP URL's last segment replaced, and a URL that does not say where cannot be
    /// guessed at; a socket goes in a directory only its user can enter.
    #[test]
    fn a_relay_needs_a_remote_profile_a_viewer_address_and_a_private_socket() {
        let Err(local) = config(Target::Local, crate::read_secret) else {
            panic!("the local profile has nothing to relay")
        };
        assert!(local.contains("nothing to relay"), "{local}");
        for (mcp, viz) in [
            ("https://hub.example:7420/mcp", "https://hub.example:7420/viz/"),
            ("https://hub.example/team/mcp/", "https://hub.example/team/viz/"),
            ("http://127.0.0.1:7420/mcp", "http://127.0.0.1:7420/viz/"),
        ] {
            assert_eq!(viz_url(mcp).unwrap().as_str(), viz, "{mcp}");
        }
        let odd = viz_url("https://hub.example:7420/sync").unwrap_err();
        assert!(odd.contains("does not end in /mcp"), "{odd}");
        let plain = viz_url("http://hub.example/mcp").unwrap_err();
        assert!(plain.contains("unencrypted"), "{plain}");
        let userinfo = viz_url("https://alice:pw@hub.example/mcp").unwrap_err();
        assert!(
            userinfo.contains("username or password") && !userinfo.contains("pw"),
            "{userinfo}"
        );

        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        assert!(check_socket_dir(&dir.join("v.sock")).is_ok());
        assert!(check_socket_dir(&dir.join("not-yet/v.sock")).is_ok(), "created 0700 at bind");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let shared = check_socket_dir(&dir.join("v.sock")).unwrap_err();
        assert!(shared.contains("other users"), "{shared}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
