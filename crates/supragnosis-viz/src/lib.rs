//! supragnosis-viz - live ontology visualization (localhost HTTP viewer).
//!
//! A **human-facing read channel**, distinct from the MCP tool surface (Principle 21). It rides
//! inside the server process and shares the same `Arc<Engine>` (cozo/RocksDB single-process
//! constraint), so it exposes the `engine.graph()` projection directly, without the lock conflict
//! that opening the db from a separate process would cause.
//!
//! - `GET /` -> self-contained canvas viewer (0 external CDNs). Polls `/api/graph` every few seconds to refresh.
//! - `GET /api/graph[?workspace=<ws>]` -> `engine.graph(ws)` JSON (Principle 16: deterministic ordering).
//!
//! Read-only - it never touches the observation log (Principle 1). The binding is forced to
//! loopback only to prevent remote exposure (Principle 17: knowledge sovereignty, limited to the
//! local trust surface until the sharing guard exists).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use supragnosis_engine::{Engine, EventEnvelope, EventSink};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// [`EventSink`] adapter that streams MCP events to the browser (SSE). Once attached to the engine,
/// tool calls are published here, and `/api/events` SSE connections subscribe via broadcast.
/// With no receivers (no open viewer), send is dropped - observability is optional (the spirit of Principle 19).
pub struct BroadcastSink {
    tx: broadcast::Sender<String>,
}

impl BroadcastSink {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self { tx }
    }
}

impl EventSink for BroadcastSink {
    fn emit(&self, env: &EventEnvelope) {
        // Called from a synchronous context (tool handler) - send is non-blocking. A serialization
        // failure or missing receiver is dropped silently (tool behavior must be unaffected even
        // when no viewer is open).
        if let Ok(json) = serde_json::to_string(env) {
            let _ = self.tx.send(json);
        }
    }
}

/// Upper bound (bytes) for reading the request line + headers. GET-only, so there is no body; a
/// request exceeding this bound is treated as malicious/malformed and dropped.
const MAX_REQUEST_HEAD: usize = 16 * 1024;

/// Parses `SUPRAGNOSIS_VIZ_ADDR` and **verifies it is loopback** (Principle 17).
///
/// Accepts only a `host:port` IP literal (e.g. `127.0.0.1:7373`). Non-loopback addresses are
/// rejected - remote exposure is not permitted until the sharing guard at the sync boundary
/// exists. A hostname (localhost) is not accepted because it would require DNS resolution
/// (removing ambiguity).
pub fn parse_local_addr(s: &str) -> anyhow::Result<SocketAddr> {
    parse_viz_addr(s, false)
}

/// Parses a viewer bind address. `allow_public = false` is the loopback invariant (Principle 17).
/// `allow_public = true` is the **interim read-only network exposure** (federation.md 6d): the owner
/// of the knowledge explicitly opts in (SUPRAGNOSIS_VIZ_PUBLIC=1) to serve the viewer beyond
/// loopback - reads only: the write endpoint (/api/review) stays gated per connection to loopback
/// peers (F19: a write is never accepted from a surface that cannot attribute it to a principal).
/// Superseded by the Phase 3.5 user-key read tier.
pub fn parse_viz_addr(s: &str, allow_public: bool) -> anyhow::Result<SocketAddr> {
    let addr: SocketAddr = s.trim().parse().with_context(|| {
        format!("invalid SUPRAGNOSIS_VIZ_ADDR: {s:?} - must be in IP:port form (e.g. 127.0.0.1:7373)")
    })?;
    if !addr.ip().is_loopback() && !allow_public {
        anyhow::bail!(
            "SUPRAGNOSIS_VIZ_ADDR {addr} is not loopback - the viewer rejects non-local binds \
             (Principle 17: knowledge sovereignty). Use 127.0.0.1:<port>, or set \
             SUPRAGNOSIS_VIZ_PUBLIC=1 to opt in to read-only network exposure (writes stay \
             loopback-gated; the authenticated read tier is federation Phase 3.5)"
        );
    }
    Ok(addr)
}

/// Accepts connections on the injected listener and serves the viewer/graph API (infinite accept loop).
///
/// Binding is done by **the caller** (so a test can bind port 0 and look up the actual port).
/// Each connection is split off into a task, but an individual connection failure is swallowed
/// so it does not kill the server.
/// Live federation status blob, maintained by the wiring layer (the CLI's status task) and served
/// verbatim at /api/federation - the viz stays decoupled from the sync crate (it renders JSON).
pub type FedStatus = Arc<std::sync::RwLock<serde_json::Value>>;

pub async fn serve(
    engine: Arc<Engine>,
    listener: TcpListener,
    events: broadcast::Sender<String>,
    fed: Option<FedStatus>,
) -> anyhow::Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "viz accept failed - continuing");
                continue;
            }
        };
        let engine = Arc::clone(&engine);
        let events = events.clone();
        let fed = fed.clone();
        // Per-connection trust: only a loopback peer may reach the write endpoint (/api/review).
        // Under the interim read-only network exposure a remote peer gets every read, never a write.
        let peer_loopback = peer.ip().is_loopback();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(&engine, &events, stream, peer_loopback, fed.as_ref()).await {
                tracing::debug!(error = %e, "viz connection handling failed");
            }
        });
    }
}

/// One connection: parse the request line plus the few headers the trust checks need (Host,
/// Sec-Fetch-Site) -> route -> respond, then close. The exception is `/api/events`, which is an SSE
/// stream: it is not closed and keeps streaming events.
async fn handle_conn(
    engine: &Engine,
    events: &broadcast::Sender<String>,
    mut stream: TcpStream,
    peer_loopback: bool,
    fed: Option<&FedStatus>,
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

    // DNS-rebinding defense: a browser tricked via a rebound hostname (attacker.com -> 127.0.0.1)
    // reaches us over loopback but still carries the attacker's Host header. A loopback peer must
    // therefore present a loopback Host; a mismatch means a foreign page is being treated as
    // same-origin with us, so refuse every path (reads included). A genuine remote reader in public
    // mode (peer not loopback) is exempt - we cannot know its expected hostname, and its surface is
    // read-only anyway (the write path stays loopback-gated below).
    if peer_loopback && !host_is_loopback(&head) {
        let resp = Response {
            status: "403 Forbidden",
            content_type: "application/json",
            body: err_body("Host header is not a loopback name - refusing (DNS-rebinding defense)"),
        };
        return write_response(&mut stream, &resp).await;
    }

    // SSE: live MCP event stream - the response is not closed and events keep streaming.
    if method == "GET" && path == "/api/events" {
        return stream_events(stream, events.subscribe()).await;
    }

    // Write gate (F19): the verdict endpoint is only reachable from a loopback peer - under the
    // interim read-only network exposure a remote peer gets 403 here, never a write.
    let resp = if path == "/api/federation" {
        // Federation status (hubs, health, per-workspace diff, known peers) - maintained by the
        // wiring layer; absent on a standalone node.
        Response {
            status: "200 OK",
            content_type: "application/json",
            body: fed
                .map(|f| f.read().map(|v| v.to_string()).unwrap_or_else(|_| "{}".into()))
                .unwrap_or_else(|| "{\"configured\":false}".to_string()),
        }
    } else if path == "/api/review" && !peer_loopback {
        Response {
            status: "403 Forbidden",
            content_type: "application/json",
            body: err_body(
                "the network-exposed viewer is read-only - verdicts require the local trust \
                 surface (loopback) or the authenticated tier (federation.md 6d, Phase 3.5/5)",
            ),
        }
    } else if path == "/api/review" && !csrf_ok(&head) {
        // CSRF defense: a verdict mutates the canon (entity_merge forwards ids), so a cross-site
        // <img>/fetch from a page the operator happens to be visiting must not be able to fire it
        // even though the request arrives over trusted loopback.
        Response {
            status: "403 Forbidden",
            content_type: "application/json",
            body: err_body(
                "cross-site request refused - a verdict must originate from the viewer page \
                 (CSRF defense: the X-Supragnosis-Viz request header is required)",
            ),
        }
    } else {
        route(engine, method, path, query)
    };
    write_response(&mut stream, &resp).await
}

/// SSE stream: after the `text/event-stream` header, emit `data: {json}\n\n` per event.
/// The JSON is a single line, so the frame is simple. Terminates when the client disconnects (write fails).
async fn stream_events(
    mut stream: TcpStream,
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

/// (status line, content-type, body) - the three fixed components of a response.
struct Response {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

/// Case-insensitive lookup of one request header value from the raw request head.
fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines()
        .skip(1) // the request line
        .take_while(|l| !l.is_empty()) // headers end at the blank line
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then_some(v.trim())
        })
}

/// True if the Host header names a loopback address, or is absent (HTTP/1.0 / non-browser client).
/// Basis of the DNS-rebinding defense: a rebound request carries a non-loopback Host.
fn host_is_loopback(head: &str) -> bool {
    let Some(h) = header_value(head, "host") else { return true };
    let hostname = if let Some(rest) = h.strip_prefix('[') {
        rest.split_once(']').map(|(a, _)| a).unwrap_or(rest) // IPv6 literal: [::1] or [::1]:port
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h) // host or host:port
    };
    matches!(hostname, "127.0.0.1" | "localhost" | "::1")
}

/// CSRF defense for the write path, robust on every browser (unlike a Sec-Fetch-Site-only check, which
/// is absent on pre-2023 browsers and would fail open there). The viewer's verdict fetch carries the
/// custom `X-Supragnosis-Viz` header. A cross-site `<img>`/`<form>` GET cannot set a custom header, so a
/// forged request lacks it and is refused; a cross-site `fetch` that tried to set it would trigger a
/// CORS preflight this server never approves (it sets no Access-Control-Allow-* headers), so the browser
/// blocks the real request before it is sent; a same-origin fetch from the viewer page sends it freely
/// (same-origin needs no preflight). The DNS-rebinding Host check above independently closes the
/// same-origin-after-rebinding case, and Sec-Fetch-Site is additionally required to be non-cross-site as
/// defense in depth where present. A non-browser client (curl) must send the header explicitly - it is a
/// write endpoint.
fn csrf_ok(head: &str) -> bool {
    let has_viewer_header = header_value(head, "x-supragnosis-viz").is_some();
    let not_cross_site = match header_value(head, "sec-fetch-site") {
        None => true, // non-browser client, or a browser without Fetch Metadata
        Some(s) => s.eq_ignore_ascii_case("same-origin") || s.eq_ignore_ascii_case("none"),
    };
    has_viewer_header && not_cross_site
}

fn route(engine: &Engine, method: &str, path: &str, query: &str) -> Response {
    if method != "GET" {
        return Response {
            status: "405 Method Not Allowed",
            content_type: "application/json",
            body: err_body("only GET is supported"),
        };
    }
    match path {
        "/" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: VIEWER_HTML.to_string(),
        },
        // The stylesheet and script are served as their own same-origin assets (compile-time embedded
        // via include_str!, so still a single binary and offline). Splitting them out of one inline
        // document gives the frontend real .css/.js files - editor tooling, linting (ESLint
        // no-unsanitized guards the innerHTML sinks), and clean diffs.
        "/viewer.css" => Response {
            status: "200 OK",
            content_type: "text/css; charset=utf-8",
            body: VIEWER_CSS.to_string(),
        },
        "/viewer.js" => Response {
            status: "200 OK",
            content_type: "text/javascript; charset=utf-8",
            body: VIEWER_JS.to_string(),
        },
        "/api/graph" => graph_response(engine, query),
        "/api/hypergraph" => hypergraph_response(engine, query),
        "/api/types" => types_response(engine, query),
        "/api/curation" => curation_response(engine, query),
        "/api/proposals" => proposals_response(engine, query),
        // Review verdict: a GET carrying the action in the query. GET-with-side-effect is intentional here -
        // the minimal server does not parse request bodies, and the effect is a gated append-only verdict
        // (engine.review_proposal records a verdict observation; the fold decides), which is idempotent for
        // merge (absorbing state, I14/I16). Loopback-only local trust surface (Principle 17). It routes
        // through the gate, never a direct projection/log write (I18 / proposal-workflow.md 14.3).
        "/api/review" => review_response(engine, query),
        "/api/workspaces" => workspaces_response(engine),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "unknown path - try /, /api/graph, /api/hypergraph, /api/types, /api/curation, /api/proposals, /api/review, /api/workspaces, or /api/events",
            ),
        },
    }
}

/// `/api/graph` - resolves the workspace from the query and produces the graph projection.
/// - unspecified -> the node's default workspace (scoped view)
/// - `*` / `all` / empty value -> everything (None)
///
/// A storage failure is 500 + error body (Principle 5: a failure is not an empty graph).
fn graph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.graph(ws_owned.as_deref()) {
        Ok(graph) => match serde_json::to_string(&graph) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty graph (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/hypergraph` - co-occurrence second-order structure (hyperedge) projection (Principle 11 second-order structure).
/// Workspace resolution is identical to `/api/graph`. A read-only derived view (Principle 1) that the viewer
/// consumes as a hull overlay. A storage failure is 500 + error body (Principle 5: a failure is not an empty graph).
fn hypergraph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.hypergraph(ws_owned.as_deref()) {
        Ok(hg) => match serde_json::to_string(&hg) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty hypergraph (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/types` - the workspace type glossary (T-Box: entity/relation type definitions - Principles 8/11).
/// Workspace resolution is identical to `/api/graph`. A read-only projection (Principle 1). A failure is 500 (Principle 5).
fn types_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.types(ws_owned.as_deref()) {
        Ok(types) => match serde_json::to_string(&types) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty glossary (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/curation` - read-only curation signals (merge candidates / grab-bags / orphans, Principle 7
/// "generate not commit"). Workspace resolution is identical to `/api/graph`. A pure projection
/// (Principle 1/16); it commits nothing. A failure is 500 (Principle 5).
fn curation_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.curation(ws_owned.as_deref()) {
        Ok(report) => match serde_json::to_string(&report) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty curation report (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/proposals` - the workspace's proposals with folded state (Principle 23). Read-only projection.
fn proposals_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.list_proposals(ws_owned.as_deref()) {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string(), "note": "storage backend failure (Principle 5)" }).to_string(),
        },
    }
}

/// `/api/review?proposal=<id>&decision=merge|reject|withdraw[&workspace=<ws>]` - cast a verdict from the
/// curation console. Goes through the gated verdict path (engine.review_proposal appends a verdict
/// observation, the fold decides) - never a direct projection/log write (I18). Self-attested (solo).
fn review_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let (Some(proposal), Some(decision)) = (param("proposal"), param("decision")) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("review needs ?proposal=<id>&decision=merge|reject|withdraw"),
        };
    };
    let workspace = param("workspace");
    match engine.review_proposal(workspace, proposal, decision, None, None) {
        Ok(id) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({ "observation_id": id }).to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// `/api/workspaces` - the list of workspaces that hold knowledge (sorted, Principle 16). The viewer's
/// workspace picker consumes it - letting you click to pick rather than type a name. A failure is 500 (Principle 5).
fn workspaces_response(engine: &Engine) -> Response {
    match engine.workspaces() {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty list (Principle 5)"
            })
            .to_string(),
        },
    }
}

async fn write_response(stream: &mut TcpStream, r: &Response) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(r.body.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn err_body(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// Minimal percent decoding (`%XX` + `+` -> space). For spaces/special characters in workspace names.
/// Invalid sequences are left as-is (lenient).
fn percent_decode(s: &str) -> String {
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

/// Self-contained live viewer (0 external CDNs). A canvas graph explorer: zoom/pan, hover neighbor
/// highlight, click focus/pin, search, fit-to-view, type-legend filter, label thinning. Colors come
/// from the dataviz skill's validated dark categorical palette (fixed order, "other" from the 9th
/// onward instead of cycling). alpha cooling + radius-based collision separation prevent overlap.
/// It polls `/api/graph` periodically for live refresh, and keeps node positions across polls by id
/// so the view does not jump.
// Embedded by build.rs from assets/ - verbatim in debug, minified in release (see build.rs).
const VIEWER_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.html"));
const VIEWER_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.css"));
const VIEWER_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.js"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_addr_accepts_loopback_rejects_public() {
        assert!(parse_local_addr("127.0.0.1:7373").is_ok());
        assert!(parse_local_addr("127.0.0.1:0").is_ok());
        assert!(parse_local_addr("[::1]:7373").is_ok());
        // Non-loopback binds are rejected (Principle 17).
        assert!(parse_local_addr("0.0.0.0:7373").is_err());
        assert!(parse_local_addr("192.168.1.10:7373").is_err());
        // Format error.
        assert!(parse_local_addr("localhost:7373").is_err());
        assert!(parse_local_addr("nonsense").is_err());
    }

    #[test]
    fn percent_decode_basics() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        // Invalid sequences keep the original text.
        assert_eq!(percent_decode("a%zz"), "a%zz");
    }
}
