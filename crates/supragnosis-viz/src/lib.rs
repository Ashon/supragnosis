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

/// One connection: parse only the request line (ignore headers/body) -> route -> respond, then close.
/// The exception is `/api/events`, which is an SSE stream: it is not closed and keeps streaming events.
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
const VIEWER_HTML: &str = r###"<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>supragnosis ontology viewer</title>
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Crect width='64' height='64' rx='12' fill='%230c0e14'/%3E%3Cg stroke='%23f0c469' stroke-width='5.5' stroke-linecap='round'%3E%3Cline x1='32' y1='32' x2='32' y2='17'/%3E%3Cline x1='32' y1='32' x2='46.3' y2='27.4'/%3E%3Cline x1='32' y1='32' x2='40.8' y2='44.1'/%3E%3Cline x1='32' y1='32' x2='23.2' y2='44.1'/%3E%3Cline x1='32' y1='32' x2='17.7' y2='27.4'/%3E%3C/g%3E%3C/svg%3E">
<style>
  /* Candlelight theme - the viewer speaks the same design language as site/ (the landing):
     warm gold on near-black ink, parchment text, mono chrome, serif prose, grain + glow.
     Fonts stay self-contained (no webfont fetch - the viewer must work offline): the landing's
     fallback stacks are used directly. */
  :root {
    color-scheme: dark;
    --surface:#08090d; --panel:#10131b; --panel-2:#131722;
    --glass:rgba(13,16,23,0.92); --glass-deep:rgba(9,11,16,0.95);
    --ink:#e9e4d6; --ink2:#aab1bd; --muted:#8e96a5; --faint:#5c6472;
    --line:#222836; --line-soft:#1a1f2b; --border:#1e2431;
    --accent:#d9a544; --gold-bright:#f0c469; --gold-dim:rgba(217,165,68,0.35);
    --gold-glass:rgba(217,165,68,0.12); --teal:#56b3a2;
    --line-hi:var(--gold-dim);
    --mono:"IBM Plex Mono",ui-monospace,"SF Mono",Menlo,Consolas,monospace;
    --prose:"Newsreader","Iowan Old Style",Georgia,serif;
  }
  * { box-sizing:border-box; }
  html,body { margin:0; height:100%; }
  body { background:var(--surface); color:var(--ink2); overflow:hidden;
         font:12.5px/1.5 var(--mono); }
  /* Atmosphere: candlelight glow beneath the (transparent) canvas, grain above it. Chrome (header,
     rails, z>=5) sits above both, so panels stay crisp while the graph floats in the atmosphere. */
  body::before { content:""; position:fixed; inset:0; pointer-events:none;
    background:radial-gradient(900px 520px at 50% -10%, rgba(217,165,68,0.09), transparent 65%),
               radial-gradient(700px 500px at 85% 8%, rgba(86,179,162,0.04), transparent 60%); }
  body::after { content:""; position:fixed; inset:0; pointer-events:none; opacity:0.045; z-index:2;
    background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='140' height='140'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2'/%3E%3C/filter%3E%3Crect width='140' height='140' filter='url(%23n)' opacity='1'/%3E%3C/svg%3E"); }
  canvas { display:block; position:fixed; inset:0; cursor:grab; }
  canvas.grabbing { cursor:grabbing; }
  header { position:fixed; top:0; left:0; right:0; z-index:5; padding:9px 14px;
           display:flex; gap:9px; align-items:center; flex-wrap:wrap;
           background:rgba(8,9,13,0.88); border-bottom:1px solid var(--line-soft);
           backdrop-filter:blur(6px); }
  h1 { font:600 13px/1.4 var(--mono); letter-spacing:0.04em; margin:0 6px 0 0; color:var(--ink); }
  h1::before { content:"* "; color:var(--accent); }
  input,button { background:var(--panel-2); border:1px solid var(--line); color:var(--ink);
                 padding:5px 9px; border-radius:5px; font:inherit; font-size:12px; }
  input::placeholder { color:var(--faint); }
  input:focus { outline:none; border-color:var(--gold-dim); }
  button { cursor:pointer; transition:border-color 0.15s ease, color 0.15s ease; }
  button:hover { border-color:var(--gold-dim); color:var(--gold-bright); }
  .hint { color:var(--muted); font-size:11.5px; }
  #status { color:var(--muted); font-size:11.5px; margin-left:auto; white-space:nowrap; }
  #wschips { display:flex; gap:6px; flex-wrap:wrap; align-items:center; margin:2px 0 4px; }
  .lbl { color:var(--muted); font-size:11px; margin-right:2px; }
  .chip,.lg { padding:2px 10px; border-radius:999px; background:var(--panel-2); border:1px solid var(--line-soft);
              cursor:pointer; font-size:11.5px; letter-spacing:0.03em; color:var(--ink2); user-select:none;
              transition:border-color 0.15s ease, color 0.15s ease; }
  .chip:hover { border-color:var(--gold-dim); color:var(--gold-bright); }
  .chip.on { background:var(--gold-glass); border-color:var(--gold-dim); color:var(--gold-bright); }
  .lg { display:inline-flex; align-items:center; gap:6px; }
  .lg:hover { border-color:var(--muted); }
  .lg.off { opacity:0.38; }
  .sw { width:10px; height:10px; border-radius:3px; display:inline-block; }
  #tip { position:fixed; pointer-events:none; z-index:10; display:none; max-width:320px;
         background:var(--glass-deep); border:1px solid var(--line); border-radius:8px;
         padding:7px 10px; font-size:12px; color:var(--ink2); box-shadow:0 8px 28px #000c; }
  #tip b { color:var(--ink); }
  #tip .k { color:var(--faint); }
  /* Type-definition tooltip (legend chips): the definition reads as prose, like the glossary. */
  #tip .tdef { font:12.5px/1.45 var(--prose); color:var(--ink); margin:3px 0 2px; word-break:break-word; }
  #tip .tdef.none { font-style:italic; color:var(--muted); }
  /* Camera controls: a vertical stack in the canvas's bottom-right corner (map-tool convention),
     above the statusbar. The right island grows downward from the top and the detail panel stops
     at right:324, so this corner is the stable empty spot. */
  #hud { position:fixed; right:12px; bottom:36px; z-index:8; display:flex; flex-direction:column; gap:6px; }
  #hud button { width:34px; height:34px; padding:0; font-size:15px; line-height:1;
                display:flex; align-items:center; justify-content:center; background:var(--glass); }
  #hud #fit { font-size:10.5px; letter-spacing:.03em; }
  #empty { position:fixed; inset:0; display:none; align-items:center; justify-content:center;
           color:var(--muted); font:italic 14px/1.5 var(--prose); pointer-events:none; }
  /* Layout loader: shown while the simulation is violently rearranging (alpha high). The graph is
     hidden until it settles, so the user sees a calm spinner instead of nodes flying around. */
  #loader { position:fixed; inset:0; display:none; flex-direction:column; align-items:center;
            justify-content:center; gap:13px; z-index:6; pointer-events:none;
            color:var(--muted); font-size:11.5px; letter-spacing:.14em; text-transform:uppercase; }
  #loader.on { display:flex; }
  #loader .spin { width:34px; height:34px; border-radius:50%; border:2px solid var(--line);
                  border-top-color:var(--accent); animation:ldspin 0.8s linear infinite; }
  @keyframes ldspin { to { transform:rotate(360deg); } }
  /* Toggle button state: off = dim (muted), on = gold-lit - state is visible at a glance.
     JS toggles only .on and keeps .tog. Action buttons like reload/zoom (no .tog) stay at their default. */
  button.tog { opacity:.5; color:var(--muted); }
  button.tog:hover { opacity:.8; }
  button.tog.on { opacity:1; background:var(--gold-glass); border-color:var(--gold-dim); color:var(--gold-bright); }
  #log { position:fixed; left:274px; top:56px; z-index:6; width:280px; max-width:38vw;
         display:flex; flex-direction:column; gap:4px; pointer-events:none; }
  #log .row { background:var(--glass); border:1px solid var(--line-soft); border-radius:6px;
              padding:4px 9px; font-size:11px; color:var(--ink2); animation:logfade 8s forwards; }
  #log .row b { color:var(--gold-bright); font-weight:600; }
  #log .row .t { color:var(--faint); margin-right:5px; }
  @keyframes logfade { 0%{opacity:0;transform:translateY(6px);} 6%{opacity:1;transform:none;}
                       82%{opacity:1;} 100%{opacity:0;} }
  /* Node detail: a wide panel docked center-bottom (between the rails, above the status bar). Header block
     (name / meta / description) stays put; the relations split into two scrolling columns (out | in). */
  #detail { position:fixed; left:274px; right:324px; bottom:36px; margin:0 auto; max-width:960px;
            z-index:8; max-height:36vh; overflow:hidden; display:none;
            background:var(--glass-deep); border:1px solid var(--line); border-radius:10px;
            padding:11px 15px 13px; font-size:12px; color:var(--ink2); box-shadow:0 14px 34px #000c; }
  #detail.on { display:flex; flex-direction:column; }
  #detail h2 { font:600 14px/1.4 var(--mono); margin:0 22px 2px 0; color:var(--gold-bright); word-break:break-word; }
  #detail .meta { color:var(--faint); font-size:11px; margin-bottom:4px; }
  #detail .desc { color:var(--ink); font:13px/1.45 var(--prose); margin:2px 0 6px; opacity:.9; word-break:break-word; }
  #detail .rels { display:flex; gap:20px; flex:1 1 auto; min-height:0; overflow:hidden; }
  #detail .relcol { flex:1 1 0; min-width:0; overflow-y:auto; }
  #detail .sec { color:var(--accent); font-size:10px; letter-spacing:.14em; text-transform:uppercase;
                 margin:0 0 4px; position:sticky; top:0; background:var(--glass-deep); padding-bottom:3px; }
  #detail .row { display:flex; align-items:center; gap:6px; padding:3px 5px; border-radius:5px; cursor:pointer; }
  #detail .row:hover { background:var(--gold-glass); }
  #detail .row .rel { color:var(--muted); font-size:10.5px; white-space:nowrap; }
  #detail .row .nm { color:var(--ink); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  #detail .dot { width:9px; height:9px; border-radius:3px; flex:0 0 auto; display:inline-block; }
  /* Icon close button (inline SVG X, round-capped strokes - same drawing language as the favicon
     asterisk). A square hit target with a soft hover fill, not a bare text glyph. */
  #detail .close { position:absolute; top:8px; right:8px; cursor:pointer; color:var(--muted);
                   border:none; background:none; border-radius:5px; padding:0;
                   width:22px; height:22px; display:flex; align-items:center; justify-content:center;
                   transition:color 0.15s ease, background 0.15s ease; }
  #detail .close:hover { color:var(--gold-bright); background:var(--gold-glass); }
  #detail .close svg { display:block; }
  #detail .empty { color:var(--muted); font:italic 12.5px/1.5 var(--prose); padding:2px 5px; }
  /* Control dock (left) - collapsible sections for layers/legend/glossary. detail panel stays on the
     right, so the two never collide. Toggled open/closed by the header 'panels' button. */
  /* Two full-height side rails: left = observe the graph (Layers + legends + stats), right = manage
     knowledge (proposals + review + glossary). Below the header, top to bottom, using the whole side. */
  /* Floating card islands: inset from every screen edge so the canvas visibly continues behind and
     around them - an overlay on the graph, not walls beside it. Height hugs the content and caps at
     the statusbar; the rail body scrolls inside. */
  .dock { position:fixed; top:56px; z-index:7; display:none; flex-direction:column;
          max-height:calc(100vh - 92px);
          background:var(--glass); border:1px solid var(--line);
          border-radius:12px; padding:10px 12px;
          box-shadow:0 18px 40px #000a, 0 2px 8px #0006; backdrop-filter:blur(7px); }
  #dockL { left:12px; width:250px; }
  /* The right island shares its screen edge with the camera HUD (bottom-right stack, ~114px tall
     at bottom:36): its height cap additionally reserves that corner (56 top + 36 bottom + 114 HUD
     + 12 gap = 218) so a full proposals list can never slide under the zoom buttons. */
  #dockR { right:12px; width:300px; max-height:calc(100vh - 218px); }
  .dock.on { display:flex; }
  .dock > #wschips, .dock > .tabs { flex:0 0 auto; }
  /* Left rail: Layers + type legends stacked (no tabs), scrolling as one column. */
  .railbody { flex:1 1 auto; min-height:0; overflow-y:auto; }
  .rsec { color:var(--accent); font-size:10px; text-transform:uppercase; letter-spacing:.14em;
          margin:11px 0 4px; padding-top:8px; border-top:1px solid var(--line-soft); }
  /* IDE-style bottom status bar (full width, below the rails). */
  #statusbar { position:fixed; left:0; right:0; bottom:0; height:24px; z-index:9; display:flex;
               align-items:center; gap:14px; padding:0 12px; background:var(--panel);
               border-top:1px solid var(--line-soft); font-size:10.5px; letter-spacing:.03em; color:var(--muted); }
  #statusbar #stats { white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
  #statusbar #status { margin-left:auto; white-space:nowrap; }
  #statusbar #session { white-space:nowrap; }
  /* Tabs instead of accordions: one section shows at a time in a fixed-height body, so expanding never
     pushes the bottom-anchored panel upward. */
  .dock .tabs { display:flex; gap:3px; margin:3px 0 5px; }
  .dock .tab { flex:1 1 0; min-width:0; padding:4px 3px; font-size:9.5px; letter-spacing:.03em;
               text-transform:uppercase; border:1px solid var(--line-soft); background:var(--panel-2);
               color:var(--muted); border-radius:5px; cursor:pointer; white-space:nowrap;
               overflow:hidden; text-overflow:ellipsis; }
  .dock .tab:hover { color:var(--gold-bright); }
  .dock .tab.on { background:var(--gold-glass); border-color:var(--gold-dim); color:var(--gold-bright); }
  .dock .tab .ct { opacity:.75; margin-left:3px; }
  .dock .panels { flex:1 1 auto; min-height:0; overflow-y:auto; }
  .dock .tabpanel { display:none; padding:2px; }
  .dock .tabpanel.on { display:block; }
  /* Grouped toggles inside the Layers section. */
  .grp { margin-bottom:7px; }
  .grp:last-child { margin-bottom:2px; }
  .ghdr { color:var(--accent); font-size:10px; letter-spacing:.14em; text-transform:uppercase; display:block; margin:2px 0 4px; }
  .grp .btns { display:flex; flex-wrap:wrap; gap:5px; }
  /* Legend chips live in the dock sections now. */
  #legendNodes,#legendEdges { display:flex; gap:6px; flex-wrap:wrap; align-items:center; }
  /* Glossary entries. */
  #glossaryBody .item { padding:4px 0 5px; border-top:1px solid var(--line-soft); }
  #glossaryBody .item:first-child { border-top:none; }
  #glossaryBody .gsec { color:var(--accent); font-size:10px; letter-spacing:.14em; text-transform:uppercase; margin:6px 0 3px; }
  #glossaryBody .gsec:first-child { margin-top:0; }
  #glossaryBody .nm { color:var(--gold-bright); font-weight:600; font-size:11.5px; }
  #glossaryBody .src { color:var(--faint); font-size:10px; margin-left:5px; }
  #glossaryBody .def { color:var(--ink2); font:12.5px/1.45 var(--prose); margin-top:1px; opacity:.95; word-break:break-word; }
  #glossaryBody .empty { color:var(--muted); font:italic 12.5px/1.5 var(--prose); padding:2px 0; }
  /* Curation (read-only signals). */
  #curationBody .csec { color:var(--accent); font-size:10px; letter-spacing:.14em; text-transform:uppercase; margin:8px 0 3px; }
  #curationBody .csec:first-child { margin-top:0; }
  #curationBody .grp { border-top:1px solid var(--line-soft); padding:4px 0; }
  #curationBody .grp:first-of-type { border-top:none; }
  #curationBody .gk { color:var(--ink); font-size:11.5px; }
  #curationBody .chips { display:flex; gap:4px; flex-wrap:wrap; margin-top:2px; }
  #curationBody .nchip { padding:1px 8px; border-radius:999px; background:var(--panel-2); border:1px solid var(--line-soft);
                         cursor:pointer; font-size:10.5px; color:var(--ink2); }
  #curationBody .nchip:hover { border-color:var(--gold-dim); color:var(--gold-bright); }
  #curationBody .nchip .ty { color:var(--faint); font-size:9.5px; margin-left:3px; }
  #curationBody .gb { padding:3px 0; border-top:1px solid var(--line-soft); font-size:11px; color:var(--ink2); }
  #curationBody .gb .sz { color:var(--gold-bright); font-weight:600; margin-right:5px; }
  #curationBody .empty { color:var(--muted); font:italic 12.5px/1.5 var(--prose); padding:2px 0; }
  /* Proposals (the gated curation console). */
  #proposalsBody .hint { color:var(--muted); font:italic 11.5px/1.4 var(--prose); margin-bottom:5px; }
  #proposalsBody .prop { border-top:1px solid var(--line-soft); padding:5px 4px 5px 6px; cursor:pointer; border-left:2px solid transparent; }
  #proposalsBody .prop:first-of-type { border-top:none; }
  #proposalsBody .prop:hover { background:rgba(217,165,68,0.05); }
  #proposalsBody .prop.sel { background:var(--gold-glass); border-left-color:var(--accent); }
  #proposalsBody .phead { display:flex; align-items:center; gap:6px; }
  #proposalsBody .pkind { color:var(--ink); font-weight:600; font-size:11px; }
  #proposalsBody .pstate { font-size:9px; text-transform:uppercase; letter-spacing:.06em; padding:1px 7px;
                           border-radius:999px; border:1px solid var(--line); color:var(--muted); margin-left:auto; }
  #proposalsBody .pstate.open { color:var(--gold-bright); border-color:var(--gold-dim); }
  #proposalsBody .pstate.merged { color:var(--teal); border-color:rgba(86,179,162,0.4); }
  #proposalsBody .prat { color:var(--ink2); font:12px/1.4 var(--prose); margin:2px 0; opacity:.9; word-break:break-word; }
  #proposalsBody .ptargets { display:flex; gap:4px; flex-wrap:wrap; margin:3px 0; }
  #proposalsBody .atypes { display:flex; gap:4px; flex-wrap:wrap; margin:3px 0 1px; }
  #proposalsBody .atype { font:10px var(--mono); color:var(--gold-bright); border:1px solid var(--gold-dim);
    border-radius:5px; padding:1px 6px; opacity:.92; }
  #proposalsBody .atype .ax { color:var(--muted); margin-left:5px; letter-spacing:.04em; }
  #proposalsBody .nchip.into { border-color:var(--gold-dim); color:var(--gold-bright); }
  #proposalsBody .pacts { display:flex; gap:5px; margin-top:3px; }
  #proposalsBody .pacts button { padding:1px 10px; font-size:10px; letter-spacing:.04em; border-radius:5px; }
  /* Accept is the gate's primary act - it gets the landing's solid-gold button voice. */
  #proposalsBody .pacts button[data-act="merge"] { background:var(--accent); border-color:var(--accent); color:#171204; font-weight:600; }
  #proposalsBody .pacts button[data-act="merge"]:hover { background:var(--gold-bright); border-color:var(--gold-bright); color:#171204; }
  #proposalsBody .empty { color:var(--muted); font:italic 12.5px/1.5 var(--prose); padding:2px 0; }
  #peersBody .fsec { color:var(--accent); font-size:10px; text-transform:uppercase; letter-spacing:.14em; margin:8px 0 4px; }
  #peersBody .fed { display:flex; align-items:center; gap:6px; padding:3px 2px; font-size:11px; color:var(--ink2); min-width:0; }
  #peersBody .fed .dot { width:8px; height:8px; border-radius:50%; flex:0 0 auto; }
  #peersBody .fed .furl { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  #peersBody .fws { color:var(--muted); font-size:10.5px; margin:0 0 2px 15px; }
  #peersBody .empty { color:var(--muted); font:italic 12.5px/1.5 var(--prose); }
</style>
<canvas id="c"></canvas>
<div id="empty">no nodes in this workspace - observe knowledge, or pick another workspace</div>
<div id="loader"><div class="spin"></div><span>settling layout...</span></div>
<header>
  <h1>supragnosis ontology</h1>
  <input id="search" placeholder="search nodes" size="16" autocomplete="off">
  <label class="hint">ws <input id="ws" placeholder="(default)" size="11" autocomplete="off"></label>
  <span class="hint">*=all</span>
  <button id="dockBtn" class="tog on" title="show/hide the side panels (layers, legend, glossary, proposals)">panels</button>
</header>
<div id="log"></div>
<div id="detail"></div>
<aside id="dockL" class="dock on">
  <div id="wschips"></div>
  <div class="railbody">
    <div class="grp"><span class="ghdr">Layout</span><div class="btns">
      <button id="followBtn" class="tog on" title="follow agent activity: workspace + camera">follow</button>
      <button id="peersBtn" class="tog" title="server mode: show federated peers as roaming cursor-dots that glide to the nodes they touch, instead of the camera chasing every remote hit (auto-on for a hub)">peers</button>
      <button id="clusterBtn" class="tog" title="group by type: type-circle layout, cross-group links kept visible (replaces the default hull organizer)">group</button>
      <button id="hyperBtn" class="tog on" title="draw the hyperedge hull overlay (co-occurrence sets, size>=3). The cohesion force is on by default - Principle 11">hulls</button>
    </div></div>
    <div class="grp"><span class="ghdr">Show</span><div class="btns">
      <button id="labelsBtn" class="tog on" title="toggle node/hull labels">labels</button>
      <button id="edgesBtn" class="tog on" title="toggle edges">edges</button>
      <button id="arrowsBtn" class="tog on" title="toggle edge direction arrowheads">arrows</button>
      <button id="footBtn" class="tog on" title="toggle session footprint rings">footprint</button>
      <button id="pulseBtn" class="tog on" title="toggle live activity pulses">pulses</button>
      <button id="histBtn" class="tog on" title="toggle superseded (past) edges">history</button>
    </div></div>
    <div class="grp"><div class="btns"><button id="reload">reload</button></div></div>
    <div class="rsec">Node types <span class="ct" id="nodeCt"></span></div>
    <div id="legendNodes"></div>
    <div class="rsec">Edge types <span class="ct" id="edgeCt"></span></div>
    <div id="legendEdges"></div>
  </div>
</aside>
<aside id="dockR" class="dock on">
  <div class="tabs">
    <button class="tab on" data-tab="proposals">Proposals<span class="ct" id="propCt"></span></button>
    <button class="tab" data-tab="review">Review<span class="ct" id="reviewCt"></span></button>
    <button class="tab" data-tab="glossary">Types<span class="ct" id="glossCt"></span></button>
    <button class="tab" data-tab="peers">Peers<span class="ct" id="peersCt"></span></button>
  </div>
  <div class="panels">
    <div class="tabpanel on" data-panel="proposals"><div id="proposalsBody"></div></div>
    <div class="tabpanel" data-panel="review"><div id="curationBody"></div></div>
    <div class="tabpanel" data-panel="glossary"><div id="glossaryBody"></div></div>
    <div class="tabpanel" data-panel="peers"><div id="peersBody"></div></div>
  </div>
</aside>
<div id="statusbar"><span id="stats"></span><span id="session" class="hint"></span><span id="status"></span></div>
<div id="tip"></div>
<div id="hud">
  <button id="zin" title="zoom in">+</button>
  <button id="zout" title="zoom out">-</button>
  <button id="fit" title="fit to view">fit</button>
</div>
<script>
"use strict";
// --- Categorical color generator (shared by node/edge/hull) -------------------------
// Spread hue as far apart as possible by the golden angle (137.508deg), and rotate
// saturation/lightness in steps so that even with many kinds (beyond a fixed palette's limit) they
// stay distinguishable. For dark-background readability, lightness 53-81% / saturation 62-92%.
// Edges are lines, so lightness is raised a little to distinguish them from nodes. Deterministic
// function (same index -> same color). Note: with very many categories, perceptual
// distinguishability has limits no matter the method.
function catColor(i, edge) {
  const h = (i * 137.508) % 360;
  // Lower saturation (roughly 50-74%) so it does not clash on either light/dark, and keep lightness
  // in the mid range (securing contrast on both backgrounds). Edges are lines, so only lightness is
  // raised a little to distinguish them from nodes.
  const l = edge ? [70, 62, 73][i % 3] : [58, 66, 50][i % 3];
  const s = [62, 50, 74][(i / 3 | 0) % 3];
  return `hsl(${h | 0}, ${s}%, ${l}%)`;
}
const OTHER = "#8e96a5", EDGE_OTHER = "#5c6472";   // defensive neutral color for types not in the map
const EDGE = "#2b3345", EDGE_HI = "#f0c469", EDGE_OLD = "#4d4340";
const EDGE_ALPHA = 0.42;         // edge base opacity (low - recedes in a dense graph; raised for the darker ink). On hover/focus, connected edges activate to 1.0
// The node stroke is proportional to the marker radius (scales with the marker on zoom - a
// consistent ratio) + a screen-px floor (so it does not vanish on zoom-out). It is a background-color
// halo, separating node/edge/neighbor (visibility).
const NODE_STROKE_RATIO = 0.35;  // stroke thickness ratio relative to radius (raised)
const NODE_STROKE_MIN = 2;       // minimum stroke thickness (screen px)
const NODE_STROKE_MAX = 5;       // maximum stroke thickness (screen px) - so a large hub does not thicken into a donut
// Canvas palette - mirrors the CSS tokens (the landing's candlelight theme). SURFACE doubles as the
// halo/cutout color, so it must match the body background exactly.
const INK = "#e9e4d6", INK2 = "#aab1bd", SURFACE = "#08090d";
const GOLD = "#d9a544", TEAL = "#56b3a2";

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
const wsInput = document.getElementById("ws"), searchEl = document.getElementById("search");
const chipBar = document.getElementById("wschips");
const legendNodesEl = document.getElementById("legendNodes"), legendEdgesEl = document.getElementById("legendEdges");
const nodeCtEl = document.getElementById("nodeCt"), edgeCtEl = document.getElementById("edgeCt");
const emptyEl = document.getElementById("empty"), logEl = document.getElementById("log");
const loaderEl = document.getElementById("loader");
const detailEl = document.getElementById("detail");
const dockLEl = document.getElementById("dockL"), dockREl = document.getElementById("dockR");
const glossaryBodyEl = document.getElementById("glossaryBody"), glossCtEl = document.getElementById("glossCt");
const curationBodyEl = document.getElementById("curationBody"), reviewCtEl = document.getElementById("reviewCt");
const proposalsBodyEl = document.getElementById("proposalsBody"), propCtEl = document.getElementById("propCt");
// A dock tab-panel is "live" when its tab is active and its dock is shown (replaces the old details.open).
function panelOn(name) {
  const p = document.querySelector('.tabpanel[data-panel="' + name + '"]');
  return !!(p && p.classList.contains("on") && p.closest(".dock").classList.contains("on"));
}

let glossaryTypes = [];          // [{target, name, description, sources, trust_tier}] - /api/types
let curation = null;             // read-only curation signals - /api/curation (Principle 7, generate-not-commit)
let proposals = [];              // proposals with folded state - /api/proposals (Principle 23 gate)
let proposalSel = null;          // the proposal currently previewed on the graph (click to select/toggle)
let follow = true;               // whether the camera follows the most recent agent-activity node
let peersOn = false;             // server mode: draw federated peers as roaming cursor-dots (hub)
let serverMode = false;          // this node is a hub (auto-detected from /api/federation role)
const peerMarkers = new Map();   // peer node_id -> {x,y,tx,ty,color,phase,flare,action,count,seen,sx,sy}
let clusterMode = false;         // group by type: type-circle layout + cross-group link emphasis (an alternative organizer)
let hullForce = true;            // hyperedge cohesion+separation physics (Principle 11) - the DEFAULT organizer; suppressed while group mode is on
let hyperMode = true;            // hyperedge hull OVERLAY (fills + labels) - on by default, visual only, independent of hullForce
let hyperedges = [];             // [{id, members:[nodeId], size, sources, trust_tier}] - /api/hypergraph
// Graphic-element visibility toggles (all default on). Pure render switches with no effect on layout.
let showLabels = true, showEdges = true, showArrows = true;
let flowPhase = 0;   // per-frame counter driving the marching-dash flow animation on active edges
let typeHl = null, edgeTypeHl = null;   // legend-chip hover highlight (node type / edge kind) - render-only
const EDGE_LABEL_MAX = 14;   // cap on relation labels shown for an active node's edges (overflow summarized as "+K more")
let showFootprint = true, showPulses = true, showSuperseded = true;
const bridgeSet = new Set();     // ids of nodes connected to another type (linking nodes that join groups)
const pulses = new Map();        // id -> remaining frames (event-node highlight ring animation)
const CLUSTER_PULL = 0.03;       // pull toward the group target point (stronger than the center attraction)
const HYPER_PULL = 0.03;         // base hyperedge centroid cohesion (scaled by hull size - see hullSizeNorm)
// Cohesion scales with member count: larger hulls pull their nodes tighter (small ones stay loose).
const HULL_SIZE_REF = 8;         // members at/above which a hull gets full cohesion weight
const HULL_COH_MIN = 0.4, HULL_COH_MAX = 1.15;    // cohesion factor range (x HYPER_PULL)
// Hull rendering: each hull is a single outward-offset rounded path (see roundedHullPath) filled once,
// directly on the canvas, at the opacity below. No stroke -> no fill/stroke seam; no offscreen -> cheap.
// A hull's whole area gets uniform transparency, while different hulls blend where they overlap.
const HULL_LAYER_ALPHA = 0.13;   // per-hull fill opacity (no node active) - tuned down for the darker candlelight ink, where the old value read as heavy plum slabs
const HULL_LAYER_DIM = 0.04;     // non-active hulls fade to this while a node is active (inspection view)
const HULL_ACTIVE_ALPHA = 0.34;  // the active node's own hulls, painted on top
const HULL_LABEL_BASE = 0.7;     // hull label opacity with no active node
const HULL_LABEL_HOVER_FADE = 0.15; // non-member hull labels fade to this while a node is active
const HULL_NODE_GAP = 14;        // world px: extra gap past the largest member glyph when expanding a hull
const HULL_PAD = 24;             // target gap between hulls (world px). Kept small to avoid over-separation at high density
const HULL_SEP = 0.008;          // separation force between hulls (gentle - scaled by cooling alpha)
const HULL_R_CAP = 160;          // upper bound on the hull radius used for separation - so a huge grab-bag cannot push the whole layout
const HULL_MAX_PUSH = 4;         // per-frame separation displacement cap per hull - prevents divergence accumulating across many pairs
let footprintSession = null;     // the session (conversation) the current footprint belongs to
const footprint = new Set();     // ids of nodes this session touched - the conversation's knowledge footprint
let nodes = [], edges = [], typeColor = {}, edgeTypeColor = {};
const posById = new Map();       // id -> {x,y,vx,vy} - layout stability across polls
const typeOff = new Set();       // node types hidden from the legend
const edgeTypeOff = new Set();   // edge kinds (relation kind) hidden from the legend
let spiralN = 0;
let drag = null, hover = null, focus = null;
let searchTerm = "";
// Camera: cam = current (drawn), camT = target. Each frame, ease cam toward camT to make
// zoom/pan/focus/fit smooth (removing instant jumps). Coordinates are CSS pixels (same system as mouse events).
let DPR = 1;
const cam = { s: 1, x: 0, y: 0 }, camT = { s: 1, x: 0, y: 0 };
let panning = null, downPos = null, userMoved = false, firstData = true, needFit = false;

// --- force simulation (alpha cooling + collision separation) ------------------------------------
let alpha = 1;
const ALPHA_DECAY = 0.0228, ALPHA_MIN = 0.02;
// Layout-loader gating: while the sim is reheated at/above SETTLE_ENTER (initial load, data change,
// group toggle - the big rearrangements), the graph is hidden behind a loader; it is revealed once
// alpha cools to REVEAL_ALPHA. Small wakes (drag/focus at 0.3) stay below SETTLE_ENTER, so those never
// trigger the loader. `settling` starts true so the first layout comes up settled, not mid-flight.
const SETTLE_ENTER = 0.5, REVEAL_ALPHA = 0.08;
// Reduced motion (same respect the landing pays to prefers-reduced-motion): instead of animating
// the violent early rearrangement behind a loader, burst-step the sim to convergence within one
// frame and reveal the layout already still.
const REDUCED_MOTION = matchMedia("(prefers-reduced-motion: reduce)").matches;
let settling = true;
let refitOnReveal = false;   // re-frame the graph after a sync-driven re-layout settles (follow mode)
// Base force parameters. The larger the graph, the wider it should spread, so stepSim scales by node count (spread).
const REPULSE = 7000, SPRING_LEN = 120, SPRING_K = 0.02;
const CENTER_BASE = 0.0015; // center-attraction base - weakened for large graphs (prevents central clumping)
const ANCHOR_K = 0.5;       // central-axis anchor: fraction of the centroid-offset corrected each frame (rigid recenter, positions only) - pins the whole cluster to the world center so it cannot drift off, even when dormant
const RANGE_BASE = 240;     // repulsion range base - widened for large graphs (pushes out farther)
const COLLIDE_PAD = 16, DAMPING = 0.85;
const MIN_SEP = 12;        // repulsion denominator floor - prevents force blowup (flinging) when very close
const MAX_V = 30;          // per-frame max speed base - raised for large graphs
const MAX_PUSH = 6;        // per-frame per-node collision displacement cap - prevents hub blowup
// Node size is proportional to neighbor count (degree) (sqrt, to flatten a wide range). The enlarged
// radius feeds directly into collision separation (minD), so spacing widens with neighbor count too,
// and nodes with few neighbors stay small and dense.
const NODE_R_BASE = 4;         // radius at degree 0
const NODE_R_SCALE = 3.4;      // sqrt(degree) coefficient
const NODE_R_MAX = 28;         // radius upper bound (prevents hub runaway)
const REPULSE_HUB_MAX = 2.5;   // hub-hub repulsion weight cap (prevents divergence)
function nodeRadius(n) { return Math.min(NODE_R_MAX, NODE_R_BASE + Math.sqrt(n.degree || 0) * NODE_R_SCALE); }
// Node stroke thickness (world units): proportional to radius + a screen-px floor. It reflects
// cam.s (current zoom), so it scales with the marker on zoom yet keeps a minimum thickness on
// zoom-out. Shared by draw and the edge endpoints.
function nodeStrokeW(n) { return Math.min(NODE_STROKE_MAX / cam.s, Math.max(nodeRadius(n) * NODE_STROKE_RATIO, NODE_STROKE_MIN / cam.s)); }
// Wake the simulation (discrete wakeup). Called only from events: new node/deletion (applyGraph),
// drag, focus. Never called from a continuous condition (overlap) - prevents endless reheating after settling.
function wake(a = 0.7) { alpha = Math.max(alpha, a); if (a >= SETTLE_ENTER) settling = true; }

// --- Camera (canvas is fullscreen, mouse uses client coordinates) ----------------------------
function toWorld(sx, sy) { return [(sx - cam.x) / cam.s, (sy - cam.y) / cam.s]; }
function easeCam() {
  const k = 0.22;
  cam.s += (camT.s - cam.s) * k; cam.x += (camT.x - cam.x) * k; cam.y += (camT.y - cam.y) * k;
  if (Math.abs(camT.s - cam.s) < 0.001) cam.s = camT.s;
  if (Math.abs(camT.x - cam.x) < 0.25) cam.x = camT.x;
  if (Math.abs(camT.y - cam.y) < 0.25) cam.y = camT.y;
}
// Change the target scale while keeping the world point under the cursor fixed (converges smoothly via easing).
function zoomAt(sx, sy, f) {
  const wx = (sx - camT.x) / camT.s, wy = (sy - camT.y) / camT.s;
  camT.s = Math.max(0.15, Math.min(4, camT.s * f));
  camT.x = sx - wx * camT.s; camT.y = sy - wy * camT.s; userMoved = true;
}
const TOP_INSET = 52;      // height occluded by the top header - compensated in centering/fit
const BOTTOM_INSET = 24;   // height occluded by the bottom status bar
const DOCK_L = 262, DOCK_R = 312;   // island inset (12) + card width (match the CSS) - reserved so content is not hidden under them
function insetL() { return dockLEl.classList.contains("on") ? DOCK_L : 0; }
function insetR() { return dockREl.classList.contains("on") ? DOCK_R : 0; }
// Bottom occlusion: the status bar, plus the detail panel when it is open (panel sits at bottom:30,
// measured live so centering keeps the focused node visible above it). Render detail before centerOn.
function insetB() {
  if (!detailEl.classList.contains("on")) return BOTTOM_INSET;
  const h = detailEl.getBoundingClientRect().height;
  return h ? 36 + h + 8 : BOTTOM_INSET;   // 36 = the detail panel's bottom offset (match the CSS)
}
// Smoothly bring a node to the screen center (focus-to-zoom). If zoomed too far out, zoom in slightly.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = (insetL() + innerWidth - insetR()) / 2 - n.x * camT.s;   // center in the strip between the rails
  camT.y = (innerHeight + TOP_INSET - insetB()) / 2 - n.y * camT.s; userMoved = true;   // above the detail panel
}

function assignColors() {
  const types = [...new Set(nodes.map(n => n.type))].sort();
  typeColor = {};
  types.forEach((t, i) => { typeColor[t] = catColor(i, false); });
  // Color per edge kind (relation kind) - deterministic (in sorted kind order), generated in the edge band.
  const ek = [...new Set(edges.map(e => e.type))].sort();
  edgeTypeColor = {};
  ek.forEach((t, i) => { edgeTypeColor[t] = catColor(i, true); });
}

function applyGraph(g) {
  const seen = new Set();
  let added = false;
  nodes = g.nodes.map(n => {
    let p = posById.get(n.id);
    if (!p) {
      added = true;
      const i = spiralN++, a = i * 2.39996, r = 60 + 30 * Math.sqrt(i);
      p = { x: innerWidth/2 + r*Math.cos(a), y: innerHeight/2 + r*Math.sin(a), vx:0, vy:0 };
      posById.set(n.id, p);
    }
    seen.add(n.id);
    return Object.assign(p, n);
  });
  let removed = 0;
  for (const id of [...posById.keys()]) if (!seen.has(id)) { posById.delete(id); removed++; }
  if (added || removed) wake();

  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  edges = g.edges.map(e => Object.assign({}, e, { a: byId[e.from], b: byId[e.to] }))
                 .filter(e => e.a && e.b);
  // Bridge nodes: nodes connected to another type (group) - linking/navigation points that join groups.
  bridgeSet.clear();
  for (const e of edges) if (e.a.type !== e.b.type) { bridgeSet.add(e.a.id); bridgeSet.add(e.b.id); }
  assignColors();
  renderLegend();
  // Keep the federation panel fresh while it is open (hub health / diff / peers move on their own).
  const peersPanel = document.querySelector('.tabpanel[data-panel="peers"]');
  if (peersPanel && peersPanel.classList.contains("on")) refreshPeers();
  const s = g.stats || {};
  statusEl.textContent = "updated " + new Date().toLocaleTimeString();
  document.getElementById("stats").textContent =
    `nodes ${s.node_count ?? nodes.length} / edges ${s.edge_count ?? edges.length}`
    + (clusterMode ? ` / groups ${Object.keys(typeColor).length}, bridges ${bridgeSet.size}` : "")
    + (s.type_counts ? " / " + Object.entries(s.type_counts).map(([t,c]) => `${t} ${c}`).join(", ") : "");
  emptyEl.style.display = nodes.length ? "none" : "flex";

  // If focused, refresh the detail inspector (reflect connection changes). If the focus node is gone, clear it.
  if (focus) { if (nodes.includes(focus)) renderDetail(focus); else { focus = null; renderDetail(null); } }

  // Initial auto-fit: once after the layout settles (cooling done), and only before user interaction (in draw).
  if (firstData && nodes.length) { firstData = false; needFit = true; }
}

// Federation panel: hubs (health + per-workspace diff vs this node) and, on a hub, the known-peer
// registry (who checked in, what they did, how long ago). Data = /api/federation (wiring-layer blob).
async function refreshPeers() {
  const host = document.getElementById("peersBody");
  const ct = document.getElementById("peersCt");
  try {
    const r = await fetch("/api/federation", { cache: "no-store" });
    const f = await r.json();
    if (!f || f.configured === false) {
      host.innerHTML = '<div class="empty">federation is not configured on this node</div>';
      ct.textContent = "";
      return;
    }
    let html = `<div class="hint">this node: ${esc(String(f.node_id || "").slice(0, 16))} (${esc(f.role || "client")})</div>`;
    const hubs = f.servers || [];
    if (hubs.length) {
      html += `<div class="fsec">Hubs</div>`;
      for (const s of hubs) {
        const dot = s.healthy ? TEAL : "#d96a5f";
        html += `<div class="fed"><span class="dot" style="background:${dot}"></span>`
          + `<span class="furl" title="${esc(s.url)}">${esc(s.url.replace(/^https?:\/\//, ""))}</span>`
          + (s.version ? `<span class="hint">v${esc(s.version)}</span>` : "") + `</div>`;
        for (const w of (s.workspaces || [])) {
          const insync = !(w.local_ahead | 0) && !(w.hub_ahead | 0);
          html += `<div class="fws">${esc(w.workspace)}: ` + (insync
            ? `<span style="color:${TEAL}">in sync</span>`
            : `local +${w.local_ahead | 0} / hub +${w.hub_ahead | 0}`) + `</div>`;
        }
      }
    }
    const peers = f.known_peers || [];
    if (peers.length) {
      html += `<div class="fsec">Known peers</div>`;
      for (const p of peers) {
        const ago = f.updated_ms && p.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - p.last_seen_ms) / 1000)) : null;
        html += `<div class="fed"><span class="dot" style="background:${GOLD}"></span>`
          + `<span class="furl" title="${esc(p.node_id)}">${esc(p.node_id.slice(0, 16))}</span>`
          + `<span class="hint">${esc(p.last_action)}${ago !== null ? " " + ago + "s ago" : ""} (${p.hits})</span></div>`;
      }
    } else if (f.role === "hub") {
      html += `<div class="fsec">Known peers</div><div class="empty">no peer has checked in yet</div>`;
    }
    host.innerHTML = html;
    const healthy = hubs.filter(s => s.healthy).length;
    ct.textContent = hubs.length ? `${healthy}/${hubs.length}` : (peers.length || "");
  } catch (e) {
    host.innerHTML = '<div class="empty">federation status unavailable</div>';
  }
}

function renderLegend() {
  // Node-type and edge-kind legends, each in its own dock section. Clicking a chip toggles that kind's
  // visibility (the off set). The section summary shows the count.
  // Chips are recreated on every render - clear any hover highlight (and the chip tooltip) so
  // neither can stick to a dead chip that will never fire mouseleave.
  typeHl = null; edgeTypeHl = null; tip.style.display = "none";
  const fill = (host, keys, colorOf, offSet, isEdge) => {
    host.innerHTML = "";
    if (!keys.length) { host.innerHTML = '<span class="lbl">none</span>'; return; }
    for (const t of keys) {
      const el = document.createElement("span");
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      el.appendChild(sw); el.appendChild(document.createTextNode(t || "(none)"));
      el.onclick = () => { if (offSet.has(t)) offSet.delete(t); else offSet.add(t); renderLegend(); };
      // Hovering a chip highlights its nodes/edges on the graph (render-only - no sim wake) and
      // shows the type's T-Box definition in the styled tooltip (Principle 8: a type has a stated
      // meaning - surfaced right where the type is read, not only in the Types tab).
      el.onmouseenter = () => { if (isEdge) edgeTypeHl = t; else typeHl = t; showTypeTip(el, t, isEdge); };
      el.onmouseleave = () => {
        if (isEdge) { if (edgeTypeHl === t) edgeTypeHl = null; }
        else if (typeHl === t) typeHl = null;
        tip.style.display = "none";
      };
      host.appendChild(el);
    }
  };
  const nodeKeys = Object.keys(typeColor).sort(), edgeKeys = Object.keys(edgeTypeColor).sort();
  fill(legendNodesEl, nodeKeys, t => typeColor[t], typeOff, false);
  fill(legendEdgesEl, edgeKeys, t => edgeTypeColor[t], edgeTypeOff, true);
  nodeCtEl.textContent = nodeKeys.length || "";
  edgeCtEl.textContent = edgeKeys.length || "";
}

// Legend chip tooltip: the type's glossary definition (T-Box), anchored beside the chip. A type
// with no recorded definition gets a nudge toward define_type instead of silence - curation as a
// micro-decision in the reading flow (Principle 22), not a separate chore.
function showTypeTip(el, t, isEdge) {
  const target = isEdge ? "relation" : "entity";
  const def = glossaryTypes.find(x => x.target === target && x.name === t);
  const r = el.getBoundingClientRect();
  tip.style.display = "block";
  tip.style.left = Math.min(r.right + 10, innerWidth - 330) + "px";
  tip.style.top = Math.max(6, r.top - 4) + "px";
  tip.innerHTML = `<b>${esc(t || "(none)")}</b> <span class="k">${target} type</span>`
    + (def
      ? `<div class="tdef">${esc(def.description)}</div><span class="k">${def.sources} src</span>`
      : `<div class="tdef none">no definition recorded - give this type a meaning with define_type</div>`);
}

// The set of nodes/edges to highlight from hover/focus/search. If none, null (everything highlighted equally).
function activeSet() {
  const anchor = focus || hover;
  if (anchor) {
    const ns = new Set([anchor.id]), es = new Set();
    for (let i = 0; i < edges.length; i++) {
      const e = edges[i];
      if (e.a.id === anchor.id || e.b.id === anchor.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
    }
    return { ns, es };
  }
  if (searchTerm) {
    const ns = new Set();
    for (const n of nodes) if (n.name.toLowerCase().includes(searchTerm)) ns.add(n.id);
    return { ns, es: new Set() };
  }
  return null;
}

async function poll() {
  const ws = wsInput.value.trim();
  const url = "/api/graph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
  try {
    const r = await fetch(url, { cache: "no-store" });
    if (!r.ok) { statusEl.textContent = "HTTP " + r.status; return; }
    const g = await r.json();
    if (g.error) { statusEl.textContent = g.error; return; }
    applyGraph(g);
    // Hyperedges (second-order structure) are fetched whenever the hull force or overlay needs them
    // (the force is on by default, suppressed only in group mode); otherwise cleared. As an auxiliary
    // channel, a failure still keeps the graph rendering (Principle 21: observability is optional).
    if (hyperMode || (hullForce && !clusterMode)) {
      try {
        const hurl = "/api/hypergraph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
        const hr = await fetch(hurl, { cache: "no-store" });
        if (hr.ok) {
          const hg = await hr.json();
          if (!hg.error) {
            hyperedges = hg.hyperedges || [];
            const drawn = hyperedges.filter(h => h.size >= 3).length;
            document.getElementById("stats").textContent += ` / hyperedges ${hyperedges.length} (hull ${drawn})`;
          }
        }
      } catch (e) { /* hull is auxiliary - the graph stays as-is */ }
    } else { hyperedges = []; }
    // Keep the type glossary + curation + proposals panels current (no-op while their sections are closed).
    refreshGlossary();
    refreshCuration();
    refreshProposals();
  } catch (e) { statusEl.textContent = "connection failed - check the server is running"; }
}

function currentWs() { return wsInput.value.trim(); }
// Clean workspace transition: reset per-workspace view state, raise the loader immediately (no
// flash of the old layout under a stale camera), and treat the new graph like a fresh load - the
// reveal ends with an auto-fit, so switching workspaces always lands framed and zoomed sensibly.
// Reflect the selected workspace in the URL (?workspace=...) - shareable, bookmarkable, and it
// survives a reload. Empty (the node default) keeps the URL clean; "*" (all) is kept as-is.
function syncUrlWorkspace() {
  const ws = wsInput.value.trim();
  const url = new URL(location.href);
  if (ws) url.searchParams.set("workspace", ws);
  else url.searchParams.delete("workspace");
  history.replaceState(null, "", url);
}

function beginWorkspaceTransition() {
  syncUrlWorkspace();
  focus = null; hover = null; renderDetail(null);
  proposalSel = null;
  pulses.clear();
  settling = true;
  needFit = true;
  userMoved = false;
  wake(1);
}

function renderChipsActive() {
  const cur = currentWs();
  chipBar.querySelectorAll(".chip").forEach(c => c.classList.toggle("on", c.dataset.ws === cur));
}
async function loadWorkspaces() {
  try {
    const r = await fetch("/api/workspaces", { cache: "no-store" });
    if (!r.ok) return;
    const list = await r.json();
    const cur = currentWs();
    const mk = (label, val) => {
      const c = document.createElement("span");
      c.className = "chip" + (val === cur ? " on" : "");
      c.dataset.ws = val; c.textContent = label;
      c.onclick = () => {
        if (wsInput.value.trim() === val) return;
        wsInput.value = val; beginWorkspaceTransition(); renderChipsActive(); poll();
      };
      return c;
    };
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = "workspaces:";
    chipBar.replaceChildren(lbl, mk("(all)", "*"), ...list.map(w => mk(w, w)));
  } catch (e) { /* server not up - retry next cycle */ }
}

// --- Live MCP activity (SSE) --------------------------------------------------------
function nodeById(id) { return nodes.find(n => n.id === id); }
function esc(s) { return String(s).replace(/[<&>]/g, c => ({ "<": "&lt;", "&": "&amp;", ">": "&gt;" }[c])); }

// Type glossary (T-Box) section body: entity types and relation types with their define_type definitions.
function renderGlossary() {
  const group = t => glossaryTypes.filter(x => x.target === t);
  const section = (title, items) => `<div class="gsec">${title} (${items.length})</div>`
    + (items.length
      ? items.map(x =>
          `<div class="item"><span class="nm">${esc(x.name)}</span>`
          + `<span class="src">${x.sources} src</span>`
          + `<div class="def">${esc(x.description)}</div></div>`).join("")
      : `<div class="empty">none defined - use define_type</div>`);
  glossaryBodyEl.innerHTML =
    section("entity types", group("entity")) + section("relation types", group("relation"));
  glossCtEl.textContent = glossaryTypes.length || "";
}

// Fetch the glossary for the current workspace, then render (only meaningful while the section is open).
async function refreshGlossary() {
  // Always fetched (not gated on the Types tab): the legend chip tooltips read glossaryTypes too,
  // so the vocabulary must be warm even when the glossary panel is closed. Tiny loopback GET.
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/types" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const t = await r.json(); if (Array.isArray(t)) glossaryTypes = t; }
  } catch (e) { /* glossary is auxiliary - keep the last render */ }
  renderGlossary();
}

// Read-only curation signals (Principle 7, generate-not-commit): merge candidates / grab-bags / orphans.
// Clicking a node chip only focuses it - the panel commits nothing (no gate).
function renderCuration() {
  if (!curation) { curationBodyEl.innerHTML = '<div class="empty">no signals yet</div>'; reviewCtEl.textContent = ""; return; }
  const nchip = n => `<span class="nchip" data-id="${esc(n.id)}" title="focus ${esc(n.name)} (deg ${n.degree}, ${n.sources} src)">${esc(n.name)}<span class="ty">${esc(n.type)}</span></span>`;
  const dup = curation.duplicates || [], gb = curation.grab_bags || [], orph = curation.orphans || [];
  let html = `<div class="csec">merge candidates (${dup.length})</div>`;
  html += dup.length
    ? dup.map(g => `<div class="grp"><span class="gk">${esc(g.key)}</span><div class="chips">${g.members.map(nchip).join("")}</div></div>`).join("")
    : `<div class="empty">none - no name collisions</div>`;
  html += `<div class="csec">grab-bag contexts (${gb.length})</div>`;
  html += gb.length
    ? gb.map(b => { const nm = b.member_names.slice(0, 10).join(", ") + (b.member_names.length > 10 ? ", ..." : ""); return `<div class="gb"><span class="sz">${b.size}</span>${esc(nm)}</div>`; }).join("")
    : `<div class="empty">none - no oversized clusters</div>`;
  html += `<div class="csec">orphans (${orph.length})</div>`;
  html += orph.length ? `<div class="chips">${orph.map(nchip).join("")}</div>` : `<div class="empty">none - all nodes linked</div>`;
  curationBodyEl.innerHTML = html;
  const s = curation.stats || {};
  reviewCtEl.textContent = (s.duplicate_groups || 0) + (s.grab_bags || 0) + (s.orphans || 0) || "";
  curationBodyEl.querySelectorAll(".nchip").forEach(c => {
    c.onclick = () => { const n = nodeById(c.dataset.id); if (n) { focus = n; renderDetail(n); centerOn(n); } };
  });
}

async function refreshCuration() {
  if (!panelOn("review")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/curation" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const c = await r.json(); if (!c.error) curation = c; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderCuration();
}

// Proposals panel (the gated curation console, Principle 23). Read + accept/reject. Accept goes through
// the gated verdict path (/api/review -> engine.review_proposal, a verdict observation), not a direct write.
function nameOf(id) { const n = nodeById(id); return n ? n.name : "(" + id.slice(0, 8) + ")"; }
function renderProposals() {
  const open = proposals.filter(p => p.state === "open");
  propCtEl.textContent = open.length || (proposals.length ? proposals.length : "");
  if (!proposals.length) { proposalsBodyEl.innerHTML = '<div class="empty">no proposals - open one with the propose tool, or from a merge candidate</div>'; return; }
  const chip = (id, into) => `<span class="nchip${id === into ? " into" : ""}" data-id="${esc(id)}" title="focus ${esc(nameOf(id))}${id === into ? " (canonical / into)" : ""}">${esc(nameOf(id))}</span>`;
  let html = `<div class="hint">click a proposal to preview the change on the graph; accept records a gated verdict</div>`;
  for (const p of proposals) {
    const st = esc(p.state);
    const sel = proposalSel && proposalSel.id === p.id ? " sel" : "";
    html += `<div class="prop${sel}" data-pid="${esc(p.id)}"><div class="phead"><span class="pkind">${esc(p.kind)}</span>`
      + `<span class="pstate ${st}">${st}${p.verdicts ? " " + p.verdicts + "v" : ""}</span></div>`;
    if (p.rationale) html += `<div class="prat">${esc(p.rationale)}</div>`;
    html += `<div class="ptargets">${(p.targets || []).map(id => chip(id, p.into)).join("")}</div>`;
    if (p.affected_types && p.affected_types.length) {   // tbox_change scope - what lights up on the graph
      const aty = a => `<span class="atype" title="${esc(a.target)} type"><span>${esc(a.name)}</span><span class="ax">${a.target === "relation" ? "edge" : "node"}</span></span>`;
      html += `<div class="atypes">${p.affected_types.map(aty).join("")}</div>`;
    }
    if (p.state === "open") {
      html += `<div class="pacts"><button data-act="merge" data-id="${esc(p.id)}">accept</button>`
        + `<button data-act="reject" data-id="${esc(p.id)}">reject</button></div>`;
    }
    html += `</div>`;
  }
  proposalsBodyEl.innerHTML = html;
  // Click a proposal row -> preview the change on the graph (belief-diff visualization). Chips/buttons
  // keep their own actions (stopPropagation), so only the row body toggles the preview.
  proposalsBodyEl.querySelectorAll(".prop").forEach(row => {
    row.onclick = () => selectProposal(proposals.find(x => x.id === row.dataset.pid));
  });
  proposalsBodyEl.querySelectorAll(".nchip").forEach(c => {
    c.onclick = (ev) => { ev.stopPropagation(); const n = nodeById(c.dataset.id); if (n) { focus = n; renderDetail(n); centerOn(n); } };
  });
  proposalsBodyEl.querySelectorAll(".pacts button").forEach(b => {
    b.onclick = async (ev) => {
      ev.stopPropagation();
      const ws = wsInput.value.trim();
      const q = "?proposal=" + encodeURIComponent(b.dataset.id) + "&decision=" + b.dataset.act + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
      try { await fetch("/api/review" + q, { cache: "no-store" }); } catch (e) { /* ignore */ }
      refreshProposals();
    };
  });
}

// The T-Box types a proposal touches, split by axis. Relation names match the graph's edge kinds
// (normalized at propose time); entity names match node types. Empty sets when the proposal declares none.
function affectedTypeSets(p) {
  const rel = new Set(), ent = new Set();
  for (const a of (p && p.affected_types) || []) {
    if (a.target === "relation") rel.add(a.name);
    else if (a.target === "entity") ent.add(a.name);
  }
  return { rel, ent };
}
// The nodes a tbox_change preview touches: endpoints of edges whose kind is (re)defined, plus nodes
// whose entity type is. Used to frame the preview (a tbox_change has no single `into` to center on).
function affectedNodes(p) {
  const { rel, ent } = affectedTypeSets(p);
  if (!rel.size && !ent.size) return [];
  const out = new Set();
  if (rel.size) for (const e of edges) if (rel.has(e.type)) { out.add(e.a); out.add(e.b); }
  if (ent.size) for (const n of nodes) if (ent.has(n.type)) out.add(n);
  return [...out];
}

// Select a proposal to preview on the graph (toggle). Centers on the canonical (`into`) node when
// present (entity_merge); otherwise frames the affected T-Box elements (tbox_change).
function selectProposal(p) {
  proposalSel = (proposalSel && p && proposalSel.id === p.id) ? null : p;
  if (proposalSel) {
    const into = nodeById(proposalSel.into);
    if (into) { focus = into; renderDetail(into); centerOn(into); }
    // No single canonical node (tbox_change): frame the affected members and mark the view user-driven
    // so a pending auto-fit does not stomp the preview (same as the search-result fit).
    else { const framed = affectedNodes(proposalSel); if (framed.length) { fitView(framed); userMoved = true; } }
  }
  renderProposals();
}

async function refreshProposals() {
  if (!panelOn("proposals")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/proposals" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const p = await r.json(); if (Array.isArray(p)) proposals = p; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderProposals();
}
function pulseNodes(ids) { for (const id of ids || []) if (posById.has(id)) pulses.set(id, 60); }
function logRow(html) {
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = `<span class="t">${new Date().toLocaleTimeString()}</span>${html}`;
  logEl.prepend(row);
  while (logEl.children.length > 8) logEl.lastChild.remove();
  setTimeout(() => row.remove(), 8000);
}
function primaryNode(ev) {
  const id = ev.kind === "observe" ? (ev.entities || [])[0]
    : ev.kind === "traverse" ? ev.start
    : ev.kind === "get_entity" ? (ev.found ? ev.id : null)
    : ev.kind === "search" ? (ev.nodes || [])[0] : null;
  return id ? nodeById(id) : null;
}
async function handleEvent(ev) {
  // If the session (conversation) changes, reset the footprint - track the new conversation's knowledge use from the start.
  if (ev.session && ev.session !== footprintSession) { footprintSession = ev.session; footprint.clear(); }
  // While following, if activity happens in a different workspace, switch to it - otherwise added
  // nodes/hits are outside the current scope and do not appear (the SSE event arrives, but the polling ws mismatches).
  const switched = follow && ev.workspace && currentWs() !== "*" && currentWs() !== ev.workspace;
  if (switched) { wsInput.value = ev.workspace; beginWorkspaceTransition(); renderChipsActive(); }
  let ids = [];
  if (ev.kind === "observe") {
    logRow(`<b>observe</b> +${(ev.entities||[]).length} ent, +${ev.relations||0} rel <span class="t">ws ${esc(ev.workspace)}</span>`);
    await poll();                       // wait for the new nodes to enter the graph, then pulse
    ids = ev.entities || [];
  } else if (ev.kind === "search") {
    logRow(`<b>search</b> "${esc(ev.query)}" -> ${ev.hits} hits <span class="t">${esc(ev.mode)}</span>`);
    if (switched) await poll();          // if the workspace switched, load that graph (so hits are visible)
    ids = ev.nodes || [];
  } else if (ev.kind === "get_entity") {
    logRow(`<b>get_entity</b> ${esc(ev.name || ev.id.slice(0,8))} <span class="t">${ev.found ? "found" : "unknown"}</span>`);
    ids = ev.found ? [ev.id] : [];
  } else if (ev.kind === "sync") {
    // Federation hit: who touched this store, which direction, how much - the live remote feed.
    logRow(`<b>sync</b> ${esc(ev.direction)} ${esc(ev.workspace)} &lt;-&gt; ${esc(String(ev.peer).slice(0, 18))} (${ev.count})`);
    let added = [];
    if (ev.count > 0) {
      // Knowledge landed: load it now, and re-frame once the re-layout settles (follow mode) so the
      // camera presents the grown graph instead of staring at a stale corner of it. Diff the node set
      // across the reload so a peer's cursor can glide onto exactly what it just contributed.
      const before = new Set(nodes.map(n => n.id));
      refitOnReveal = follow;
      await poll();
      added = nodes.filter(n => !before.has(n.id));
    }
    if (peersOn) notePeer(ev, added);   // server mode: move the peer's marker, not the camera
    ids = [];
  } else if (ev.kind === "traverse") {
    const sn = nodeById(ev.start);
    logRow(`<b>traverse</b> ${esc(sn ? sn.name : ev.start.slice(0,8))} -> ${(ev.reached||[]).length}`);
    ids = [ev.start, ...(ev.reached || [])];
  } else return;
  pulseNodes(ids);
  for (const id of ids) if (id) footprint.add(id);   // accumulate the conversation footprint (regardless of whether the node exists)
  // Reheat only when the event touched actual nodes - sync/hc chatter (now periodic via the
  // status loop) must never jiggle a settled layout.
  if (ids.length) wake(0.3);
  if (follow && !peersOn) {
    // Frame the WHOLE hit set: several hits fit into view together (pan + zoom as needed); a
    // single hit centers smoothly. The camera narrates what the agent touched. In server mode the
    // camera holds still and the peer markers move instead (a hub would otherwise jump on every hit).
    const hitNodes = ids.map(nodeById).filter(Boolean);
    if (hitNodes.length > 1) fitView(hitNodes, 130);
    else { const n = hitNodes[0] || primaryNode(ev); if (n) centerOn(n); }
  }
  const sEl = document.getElementById("session");
  if (sEl) sEl.textContent = footprintSession ? `session ${footprintSession.slice(0,22)} / ${footprint.size} used` : "";
}
// --- Peer markers (server mode) -----------------------------------------------------
// On a hub, many peers sync at once. Instead of the camera chasing every remote hit, each federated
// peer is a minimal cursor-dot that glides to the nodes it touched (like a remote cursor): a push shows
// the peer drifting onto the knowledge it just contributed. Deterministic per-peer color; hover for id.
function peerColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return `hsl(${h % 360} 68% 63%)`;
}
// Centroid of a node set (or the whole visible graph) in world coords; parks at the view center when empty.
function viewCentroid(list) {
  const src = (list && list.length ? list : nodes).filter(n => !typeOff.has(n.type));
  if (!src.length) {
    const wx = (insetL() + innerWidth - insetR()) / 2, wy = (innerHeight + TOP_INSET - insetB()) / 2;
    return [(wx - cam.x) / cam.s, (wy - cam.y) / cam.s];
  }
  let sx = 0, sy = 0; for (const n of src) { sx += n.x; sy += n.y; }
  return [sx / src.length, sy / src.length];
}
function peerMarker(id) {
  let m = peerMarkers.get(id);
  if (!m) {
    const [cx, cy] = viewCentroid();
    const a = ((parseInt(id.slice(0, 4), 16) || 0) / 65535) * 6.283;   // stable per-peer angle
    m = { x: cx + Math.cos(a) * 140, y: cy + Math.sin(a) * 140, tx: cx, ty: cy,
          color: peerColor(id), phase: a, flare: 0, action: "", count: 0, seen: 0, sx: null, sy: null };
    peerMarkers.set(id, m);
  }
  return m;
}
// A sync hit from `ev.peer`: glide its dot onto the nodes it just touched (added on a push), else toward
// the workspace centroid, with a small per-peer offset so several peers on one region do not stack.
function notePeer(ev, added) {
  if (!ev.peer) return;
  const m = peerMarker(ev.peer);
  const hasNodes = added && added.length;
  const [cx, cy] = viewCentroid(hasNodes ? added : null);
  const jit = hasNodes ? 14 : 64;
  m.tx = cx + Math.cos(m.phase) * jit; m.ty = cy + Math.sin(m.phase) * jit;
  m.flare = 1; m.action = ev.direction || ""; m.count = ev.count | 0; m.seen = Date.now();
  if (hasNodes) pulseNodes(added.map(n => n.id));   // light up what the peer contributed
}
function stepPeers() {
  if (!peersOn) return;
  for (const m of peerMarkers.values()) {
    m.x += (m.tx - m.x) * 0.07; m.y += (m.ty - m.y) * 0.07;
    m.flare = m.flare > 0.002 ? m.flare * 0.95 : 0;
  }
}
function drawPeers() {
  if (!peersOn || !peerMarkers.size) return;
  ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  const t = performance.now() / 1000, now = Date.now();
  for (const m of peerMarkers.values()) {
    const bob = Math.sin(t * 1.2 + m.phase) * 2.5;   // gentle idle drift so quiet peers still breathe
    const px = m.x * cam.s + cam.x, py = m.y * cam.s + cam.y + bob;
    m.sx = px; m.sy = py;
    const live = Math.max(0, 1 - (m.seen ? (now - m.seen) / 1000 : 999) / 90);   // dim as a peer goes quiet
    const r = 3.5 + m.flare * 5;
    const g = ctx.createRadialGradient(px, py, 0, px, py, r * 3.4);
    g.addColorStop(0, m.color); g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = 0.22 * (0.4 + 0.6 * live) + m.flare * 0.5;
    ctx.fillStyle = g; ctx.beginPath(); ctx.arc(px, py, r * 3.4, 0, 7); ctx.fill();
    ctx.globalAlpha = 0.45 + 0.55 * live;
    ctx.fillStyle = m.color; ctx.beginPath(); ctx.arc(px, py, r, 0, 7); ctx.fill();
    ctx.lineWidth = 1; ctx.strokeStyle = "rgba(0,0,0,0.55)"; ctx.stroke();
  }
  ctx.globalAlpha = 1;
}
function peerAt(cx, cy) {
  if (!peersOn) return null;
  for (const [id, m] of peerMarkers) if (m.sx != null && Math.hypot(cx - m.sx, cy - m.sy) <= 11) return { id, m };
  return null;
}
function showPeerTip(m, id, cx, cy) {
  tip.style.display = "block";
  tip.style.left = Math.min(cx + 14, innerWidth - 330) + "px";
  tip.style.top = (cy + 14) + "px";
  const ago = m.seen ? Math.max(0, Math.round((Date.now() - m.seen) / 1000)) : null;
  tip.innerHTML = `<b>peer ${esc(id.slice(0, 6))}</b><br>`
    + `<span class="k">node</span> ${esc(id.slice(0, 16))}<br>`
    + `<span class="k">last</span> ${esc(m.action || "-")}${ago !== null ? " " + ago + "s ago" : ""} `
    + `&nbsp; <span class="k">hits</span> ${m.count | 0}`;
}
// Seed/refresh markers from the peer roster so idle peers (that only advertise, which is not a live
// event) still appear and stay lit. Only metadata + `seen` are touched - a marker's glide target/flare
// belong to the live Sync events (notePeer), so a roster refresh never yanks a moving cursor.
function seedPeers(f) {
  for (const p of (f.known_peers || [])) {
    const m = peerMarker(p.node_id);
    m.action = p.last_action || ""; m.count = p.hits | 0;
    const ageMs = f.updated_ms && p.last_seen_ms ? Math.max(0, f.updated_ms - p.last_seen_ms) : 0;
    m.seen = Date.now() - ageMs;   // keep `seen` in client-clock terms (avoid hub/viewer clock skew)
  }
}
// A hub gathers peers - auto-enter server mode: draw peer cursors and stop the camera from chasing
// remote hits (the owner can still toggle either). On a client/loopback viewer this is a no-op.
async function detectServerMode() {
  try {
    const f = await (await fetch("/api/federation", { cache: "no-store" })).json();
    if (!f || f.role !== "hub") return;
    serverMode = true;
    peersOn = true; peersBtn.classList.add("on");
    follow = false; followBtn.classList.toggle("on", false);
    seedPeers(f);
  } catch (_) { /* federation status unavailable - stay in client mode */ }
}
// Keep the roster fresh on a hub so a peer that only heartbeats (advertise emits no live event) still
// shows up within a few seconds of checking in, and quiet peers do not fade out while still present.
async function refreshPeerRoster() {
  if (!serverMode) return;
  try { seedPeers(await (await fetch("/api/federation", { cache: "no-store" })).json()); } catch (_) {}
}
function connectEvents() {
  try {
    const es = new EventSource("/api/events");
    es.onmessage = e => { try { handleEvent(JSON.parse(e.data)); } catch (_) {} };
    // On error, EventSource reconnects automatically.
  } catch (_) { /* EventSource unsupported - works with polling alone */ }
}

// --- Detail inspector: shows the clicked node's connections (neighbors + relations), and click a neighbor to explore ---
function renderDetail(node) {
  if (!node) { detailEl.className = ""; detailEl.innerHTML = ""; return; }
  const outs = edges.filter(e => e.a === node && !typeOff.has(e.b.type));
  const ins = edges.filter(e => e.b === node && !typeOff.has(e.a.type));
  const rowHtml = (rel, other, dir, desc) =>
    `<div class="row" data-id="${esc(other.id)}" title="${desc ? esc(desc) : "focus " + esc(other.name)}">`
    + `<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>`
    + `<span class="rel">${dir} ${esc(rel)}</span>`
    + `<span class="nm">${esc(other.name)}</span></div>`;
  const list = (arr, dir) => arr.length
    ? arr.map(e => rowHtml(e.type, dir === "->" ? e.b : e.a, dir, e.description)).join("")
    : `<div class="empty">none</div>`;
  detailEl.innerHTML =
    `<button class="close" title="close" aria-label="close">`
      + `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`
      + `<path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>`
      + `</svg></button>`
    + `<h2>${esc(node.name)}</h2>`
    + `<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span> `
    + `${esc(node.type)} / deg ${node.degree || 0} / src ${node.sources} / ${esc(String(node.trust_tier))}</div>`
    + (node.aliases && node.aliases.length ? `<div class="meta">merged: ${esc(node.aliases.join(", "))}</div>` : "")
    + (node.origins && node.origins.length ? `<div class="meta">from: ${esc(node.origins.join(", "))}</div>` : "")
    + (node.description ? `<div class="desc">${esc(node.description)}</div>` : "")
    + `<div class="rels">`
    +   `<div class="relcol"><div class="sec">outgoing (${outs.length})</div>${list(outs, "->")}</div>`
    +   `<div class="relcol"><div class="sec">incoming (${ins.length})</div>${list(ins, "<-")}</div>`
    + `</div>`;
  detailEl.className = "on";
  detailEl.querySelector(".close").onclick = () => { focus = null; renderDetail(null); };
  detailEl.querySelectorAll(".row").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) { focus = n; renderDetail(n); centerOn(n); }
    };
  });
}

// Supersampling (HiDPI): scale the backing store by DPR and fix the CSS size to the viewport -> sharp.
function resize() {
  DPR = Math.min(window.devicePixelRatio || 1, 2);   // cap at 2x for performance
  canvas.width = Math.round(innerWidth * DPR);
  canvas.height = Math.round(innerHeight * DPR);
  canvas.style.width = innerWidth + "px";
  canvas.style.height = innerHeight + "px";
}
addEventListener("resize", resize);

// Set the target camera so the given node set fits on screen (smoothly, via easing). In CSS pixels.
function fitView(list, pad = 90) {
  const src = (list || nodes).filter(n => !typeOff.has(n.type));
  if (!src.length) return;
  let a = 1e9, b = 1e9, c = -1e9, d = -1e9;
  for (const n of src) { a = Math.min(a,n.x); b = Math.min(b,n.y); c = Math.max(c,n.x); d = Math.max(d,n.y); }
  const w = innerWidth, h = innerHeight, gw = Math.max(1, c-a), gh = Math.max(1, d-b);
  const il = insetL(), ir = insetR();   // fit into the strip between the side rails
  camT.s = Math.max(0.15, Math.min(2.5, Math.min((w - il - ir - pad*2) / gw, (h - pad*2 - TOP_INSET - BOTTOM_INSET) / gh)));
  camT.x = (il + w - ir)/2 - (a+c)/2*camT.s;
  camT.y = (h + TOP_INSET - BOTTOM_INSET)/2 - (b+d)/2*camT.s;
}

// hyperedge id -> palette color (deterministic hash). Overlapping hulls blend semi-transparently (C1: overlap = connective tissue).
// Size-scaled visual weight for a hyperedge (0 at min size 2, 1 at HULL_SIZE_REF+ members).
function hullSizeNorm(size) { return Math.max(0, Math.min(1, (size - 2) / (HULL_SIZE_REF - 2))); }
// Geometry of one hyperedge hull: the convex hull of the member centers, an outward expansion radius r
// (largest member glyph + gap), and the member centroid (for the label). Returns null when degenerate.
function hullGeom(ms) {
  let cx = 0, cy = 0, r = 0;
  for (const m of ms) { cx += m.x; cy += m.y; const g = nodeRadius(m) + nodeStrokeW(m) / 2; if (g > r) r = g; }
  cx /= ms.length; cy /= ms.length; r += HULL_NODE_GAP;
  const hull = convexHull(ms.map(m => ({ x: m.x, y: m.y })));
  if (hull.length < 3) return null;
  return { hull, r, cx, cy };
}
// Trace the outward-offset rounded hull as a SINGLE closed path: each edge is pushed out by r (past the
// node glyphs) and consecutive edges are joined by a corner arc sampled into short segments. Filling this
// one path gives the same rounded blob a thick round-join stroke would - but with no stroke, so there is
// no fill/stroke overlap (no seam) and no offscreen compositing is needed; the caller just fills it once
// at the hull's opacity, directly on the canvas, and overlapping hulls blend naturally.
function roundedHullPath(c, g) {
  const hull = g.hull, n = hull.length, r = g.r, cx = g.cx, cy = g.cy;
  const nrm = [];   // outward unit normal per edge i (edge hull[i]->hull[i+1])
  for (let i = 0; i < n; i++) {
    const a = hull[i], b = hull[(i + 1) % n];
    let nx = -(b.y - a.y), ny = (b.x - a.x); const L = Math.hypot(nx, ny) || 1; nx /= L; ny /= L;
    if (nx * ((a.x + b.x) / 2 - cx) + ny * ((a.y + b.y) / 2 - cy) < 0) { nx = -nx; ny = -ny; }  // point away from centroid
    nrm.push([nx, ny]);
  }
  c.beginPath();
  for (let i = 0; i < n; i++) {
    const a = hull[i], b = hull[(i + 1) % n], [nx, ny] = nrm[i];
    if (i === 0) c.moveTo(a.x + nx * r, a.y + ny * r); else c.lineTo(a.x + nx * r, a.y + ny * r);
    c.lineTo(b.x + nx * r, b.y + ny * r);
    // corner arc at vertex b, from this edge's normal to the next edge's normal (shortest signed sweep)
    const v = hull[(i + 1) % n], [mx, my] = nrm[(i + 1) % n];
    const a1 = Math.atan2(ny, nx); let da = Math.atan2(my, mx) - a1;
    while (da <= -Math.PI) da += 2 * Math.PI; while (da > Math.PI) da -= 2 * Math.PI;
    const steps = Math.max(1, Math.ceil(Math.abs(da) / 0.4));
    for (let k = 1; k <= steps; k++) { const t = a1 + da * k / steps; c.lineTo(v.x + Math.cos(t) * r, v.y + Math.sin(t) * r); }
  }
  c.closePath();
}
function hyperColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return catColor(h % 512, false);   // id hash -> generated palette index (removes the fixed 8-color limit)
}
// Convex hull (Andrew monotone chain). Deterministic (sorted input) - Principle 16. Returns as-is if fewer than 3 points.
function convexHull(pts) {
  if (pts.length < 3) return pts.slice();
  const p = pts.slice().sort((a, b) => a.x - b.x || a.y - b.y);
  const cross = (o, a, b) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lo = [];
  for (const q of p) { while (lo.length >= 2 && cross(lo[lo.length-2], lo[lo.length-1], q) <= 0) lo.pop(); lo.push(q); }
  const up = [];
  for (let i = p.length - 1; i >= 0; i--) { const q = p[i]; while (up.length >= 2 && cross(up[up.length-2], up[up.length-1], q) <= 0) up.pop(); up.push(q); }
  lo.pop(); up.pop();
  return lo.concat(up);
}
// Do the two hyperedges share a node (iterate the smaller set). Shared = a connected context, so they are not separated (C1).
function hullsShareMember(a, b) {
  const [s, l] = a.size <= b.size ? [a, b] : [b, a];
  for (const x of s) if (l.has(x)) return true;
  return false;
}

function stepSim() {
  const N = nodes.length;
  if (N === 0) return;
  const cooling = alpha >= ALPHA_MIN;
  if (cooling) alpha += (0 - alpha) * ALPHA_DECAY;
  const active = alpha >= ALPHA_MIN;   // dormant once cooling is done - no force is applied
  const pinned = v => v === drag || v === focus;

  // Scale by node count: more nodes spread wider (repulsion range/strength up, center attraction down).
  // Prevents a hairball (central clumping) - base is tuned for small graphs, large ones expand via spread.
  const spread = Math.min(4, Math.max(1, Math.sqrt(N / 20)));
  const range = RANGE_BASE * spread, centerG = CENTER_BASE / spread;
  const repulse = REPULSE * spread, maxV = MAX_V * Math.min(spread, 2);

  // Collision displacement is accumulated per node, then clamped to a cap - stops a hub overlapping
  // many neighbors from flinging far in a single frame (instead of moving directly).
  const cdx = new Array(N).fill(0), cdy = new Array(N).fill(0);
  // The radius (degree-proportional) is computed once per frame and shared by repulsion weighting/collision separation.
  const rad = new Array(N);
  for (let i = 0; i < N; i++) rad[i] = nodeRadius(nodes[i]);

  for (let i = 0; i < N; i++) for (let j = i + 1; j < N; j++) {
    const a = nodes[i], b = nodes[j];
    let dx = b.x - a.x, dy = b.y - a.y, d = Math.hypot(dx, dy);
    if (d < 0.5) {
      // (Nearly) coincident coordinates have a zero direction and cannot be pushed apart - separate in a deterministic direction (prevents degeneracy).
      const ang = ((i * 7 + j * 13) % 628) / 100;
      dx = Math.cos(ang); dy = Math.sin(ang); d = 0.5;
    } else { dx /= d; dy /= d; }
    const d2 = d * d;
    // Repulsion only when active (cooling), and near-range only (0 outside the range). Prevents the
    // problem where, when dormant, only repulsion remained and spread endlessly without the balance of cohesion (gravity/springs).
    if (active && d < range) {
      // The larger a node's neighbor count (radius), the more gently repulsion is weighted up - spacing around a hub widens (clamped to a cap).
      const w = Math.min(REPULSE_HUB_MAX, (rad[i] + rad[j]) / (2 * NODE_R_BASE));
      const rf = repulse * alpha * w * (1 - d / range) / Math.max(d2, MIN_SEP * MIN_SEP);
      a.vx -= rf*dx; a.vy -= rf*dy; b.vx += rf*dx; b.vy += rf*dy;
    }
    // Collision minimum gap = sum of the two radii + padding -> a node with more neighbors (larger) gets wider spacing around it.
    const minD = rad[i] + rad[j] + COLLIDE_PAD;
    if (d < minD) {
      const push = (minD - d) / 2;
      cdx[i] -= dx*push; cdy[i] -= dy*push; cdx[j] += dx*push; cdy[j] += dy*push;
    }
  }

  // Residual overlap is cleaned up by the collision displacement below, pushing each frame (position correction applies even while dormant).
  // Do not reheat here - wake only from 'events' like a new node/drag/resize
  // (removes the problem of reheating every frame after settling).
  if (active) {
    for (const e of edges) {
      let dx = e.b.x - e.a.x, dy = e.b.y - e.a.y, d = Math.hypot(dx,dy) || 1;
      const f = (d - SPRING_LEN) * SPRING_K * alpha; dx /= d; dy /= d;
      e.a.vx += f*dx; e.a.vy += f*dy; e.b.vx -= f*dx; e.b.vy -= f*dy;
    }
  }

  // Hyperedge layout (Principle 11 second-order structure "well cohered"): (1) pull members toward
  // each hyperedge's centroid to cohere the hull tightly, and (2) push apart the centroids of
  // non-overlapping hulls to widen the gap. Hulls that share a node (overlap) stay naturally close
  // because the shared node is pulled to both centroids at once, and the separation force cancels at
  // the shared node, preserving the overlap relationship (C1: overlap = connective tissue).
  // Default organizer; group mode takes precedence (suppressed while clusterMode is on) so the two never fight.
  if (hullForce && !clusterMode && active && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    // Geometry: members + centroid + mean radius (clamped to a cap - so a huge grab-bag cannot push the whole layout)
    // + member id set (for share detection).
    const hgs = [];
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(Boolean);
      if (ms.length < 2) continue;
      let cx = 0, cy = 0; for (const m of ms) { cx += m.x; cy += m.y; }
      cx /= ms.length; cy /= ms.length;
      let r = 0; for (const m of ms) r += Math.hypot(m.x - cx, m.y - cy);
      r = Math.min(r / ms.length, HULL_R_CAP);
      hgs.push({ ms, ids: new Set(ms.map(m => m.id)), cx, cy, r });
    }
    // (1) Cohesion: member -> its own centroid. Scaled by hull size - larger hulls pull tighter,
    // small ones stay loose.
    for (const g of hgs) {
      const cf = HYPER_PULL * (HULL_COH_MIN + hullSizeNorm(g.ms.length) * (HULL_COH_MAX - HULL_COH_MIN));
      for (const m of g.ms) {
        if (pinned(m)) continue;
        m.vx += (g.cx - m.x) * cf * alpha; m.vy += (g.cy - m.y) * cf * alpha;
      }
    }
    // (2) Separation: push apart only **disjoint** hull pairs (not sharing a node) - a shared hull is
    // a connected context and must stay attached (C1), and pushing every pair at high density blows up the whole layout.
    // Accumulate each hull's net displacement, clamp it to the cap (HULL_MAX_PUSH), then apply it to members to prevent divergence.
    const sepx = new Array(hgs.length).fill(0), sepy = new Array(hgs.length).fill(0);
    for (let i = 0; i < hgs.length; i++) for (let j = i + 1; j < hgs.length; j++) {
      const a = hgs[i], b = hgs[j];
      if (hullsShareMember(a.ids, b.ids)) continue;   // do not push a connected context (C1)
      const dx = b.cx - a.cx, dy = b.cy - a.cy, d = Math.hypot(dx, dy) || 0.01;
      const want = a.r + b.r + HULL_PAD;
      if (d < want) {
        const mag = (want - d) * HULL_SEP * alpha, ux = dx / d * mag, uy = dy / d * mag;
        sepx[i] -= ux; sepy[i] -= uy; sepx[j] += ux; sepy[j] += uy;
      }
    }
    for (let i = 0; i < hgs.length; i++) {
      let mx = sepx[i], my = sepy[i]; const mm = Math.hypot(mx, my);
      if (mm === 0) continue;
      if (mm > HULL_MAX_PUSH) { mx *= HULL_MAX_PUSH / mm; my *= HULL_MAX_PUSH / mm; }
      for (const m of hgs[i].ms) if (!pinned(m)) { m.vx += mx; m.vy += my; }
    }
  }

  const wcx = innerWidth/2, wcy = innerHeight/2;   // world coordinates (CSS pixel system) - independent of the camera
  // Group mode: place per-type target points on a circle to spatially separate groups (deterministic:
  // angle assigned in sorted type order). The group-target attraction replaces the center attraction, and
  // bridge edges (springs) pull linking nodes between groups so a "navigable" connection remains.
  let tgt = null;
  if (clusterMode && active) {
    const types = Object.keys(typeColor).sort(), k = Math.max(1, types.length);
    const R = Math.min(innerWidth, innerHeight) * 0.34;
    tgt = {};
    types.forEach((t, i) => { const a = (i / k) * Math.PI * 2; tgt[t] = [wcx + R * Math.cos(a), wcy + R * Math.sin(a)]; });
  }
  for (let k = 0; k < N; k++) {
    const v = nodes[k];
    if (pinned(v)) { v.vx = 0; v.vy = 0; continue; }
    if (active) {
      if (tgt) {
        const g = tgt[v.type] || [wcx, wcy];
        v.vx += (g[0] - v.x) * CLUSTER_PULL * alpha; v.vy += (g[1] - v.y) * CLUSTER_PULL * alpha;
      } else {
        v.vx += (wcx - v.x) * centerG * alpha; v.vy += (wcy - v.y) * centerG * alpha;
      }
    }
    v.vx *= DAMPING; v.vy *= DAMPING;
    // speed cap - even under a large force, nothing flies off-screen.
    const sp = Math.hypot(v.vx, v.vy);
    if (sp > maxV) { v.vx *= maxV/sp; v.vy *= maxV/sp; }
    // the collision displacement is also clamped to a per-node cap before adding.
    let mx = cdx[k], my = cdy[k]; const m = Math.hypot(mx, my);
    if (m > MAX_PUSH) { mx *= MAX_PUSH/m; my *= MAX_PUSH/m; }
    v.x += v.vx + mx; v.y += v.vy + my;
  }

  // Central-axis anchor: the pairwise forces are action-reaction symmetric (zero net force on the
  // whole-graph centroid), but the per-node / per-hull clamps (maxV, MAX_PUSH, HULL_MAX_PUSH) and the
  // collision push that keeps correcting while dormant break that symmetry, so the constellation
  // slowly drifts to one side. Rigidly translate every node so the centroid returns to the world
  // center - positions only (no velocity -> no momentum/oscillation), applied every frame including
  // when dormant, which fixes the cluster to the center after cooling and across simulation restarts.
  // Skipped while dragging (do not fight the pointer); pinned nodes (drag/focus) are held in place.
  if (!drag) {
    let sx = 0, sy = 0;
    for (const v of nodes) { sx += v.x; sy += v.y; }
    const offx = (wcx - sx / N) * ANCHOR_K, offy = (wcy - sy / N) * ANCHOR_K;
    if (offx || offy) for (const v of nodes) if (!pinned(v)) { v.x += offx; v.y += offy; }
  }
}

function draw() {
  stepSim();
  easeCam();
  // Reduced motion: settle synchronously (bounded - alpha decays multiplicatively, so convergence
  // takes ~110 steps; the cap only guards a pathological graph from locking the frame).
  if (settling && REDUCED_MOTION) { for (let i = 0; i < 600 && alpha > REVEAL_ALPHA; i++) stepSim(); }
  // Reveal transition: the layout has calmed enough to show. Frame it first (auto-fit + snap the
  // camera, before any user interaction) so the graph appears already fitted rather than mid-zoom.
  if (settling && alpha <= REVEAL_ALPHA) {
    settling = false;
    if (needFit && !userMoved) { needFit = false; fitView(); cam.s = camT.s; cam.x = camT.x; cam.y = camT.y; }
    else if (refitOnReveal) { fitView(); }   // smooth re-frame after synced knowledge landed
    refitOnReveal = false;
  }
  // While settling, keep stepping the sim (above) but hide the graph behind the loader - the user sees
  // a calm spinner instead of nodes flying around during the violent early rearrangement.
  if (settling && nodes.length) {
    ctx.setTransform(1,0,0,1,0,0); ctx.clearRect(0,0,canvas.width,canvas.height);
    loaderEl.classList.add("on");
    requestAnimationFrame(draw); return;
  }
  loaderEl.classList.remove("on");
  // Initial auto-fit: once after the layout settles (only before user interaction).
  if (needFit && alpha < ALPHA_MIN && !userMoved) { needFit = false; fitView(); }

  const act = activeSet();
  flowPhase++;   // advance the active-edge flow animation (rAF runs every frame)
  // Legend-chip hover highlight: for an edge-kind chip, collect the endpoint nodes of matching visible
  // edges once per frame (nodes to keep lit). Node-type chips match on n.type directly.
  let lgEndpoints = null;
  if (edgeTypeHl) {
    lgEndpoints = new Set();
    for (const e of edges) {
      if (e.type !== edgeTypeHl || typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
      if (e.valid_to && !showSuperseded) continue;
      lgEndpoints.add(e.a.id); lgEndpoints.add(e.b.id);
    }
  }
  const hlAnchor = focus || hover;   // the active node (click or hover) - drives hull emphasis + fade
  const anchor = focus || hover;
  ctx.setTransform(1,0,0,1,0,0);
  ctx.clearRect(0,0,canvas.width,canvas.height);

  // Edges + nodes use the world transform (zoom/pan, incl. DPR supersampling); labels use screen coordinates (keeping readability).
  ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);
  ctx.lineCap = "round"; ctx.lineJoin = "round";
  // Hyperedge hull overlay (laid behind edges/nodes). Only size>=3 is drawn - 2 converges to a binary
  // edge. Each hull is a SINGLE outward-offset rounded path filled once, directly on the canvas, at the
  // hull's opacity - no stroke (so no fill/stroke seam) and no offscreen compositing (cheap: N path fills,
  // not N large drawImage copies). Overlapping hulls blend naturally (C1: overlap = connective tissue).
  // While a node is active, its own hulls are emphasized and the rest fade. Labels are collected per hull.
  let hullLabels = [];
  if (hyperMode && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    const items = [];
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
      if (ms.length < 3) continue;
      const g = hullGeom(ms);
      if (!g) continue;
      const hot = hlAnchor && h.members.includes(hlAnchor.id);   // is the active (click/hover) node a member of this context
      // Legend-chip hover: does this context contain any highlighted member? Hulls without one fade,
      // so the highlighted type/relation stands out against the hull layer too (same inspection language).
      const lgHit = typeHl ? ms.some(m => m.type === typeHl)
        : edgeTypeHl ? ms.some(m => lgEndpoints.has(m.id))
        : false;
      const rep = ms.reduce((a, m) => (m.degree||0) > (a.degree||0) ? m : a, ms[0]);   // hub = highest-degree member
      items.push({ g, col: hyperColor(h.id), hot, lgHit, rep, size: ms.length });
    }
    const lgActive = !!(typeHl || edgeTypeHl);
    const paint = (it) => {
      roundedHullPath(ctx, it.g);
      ctx.globalAlpha = lgActive ? (it.lgHit ? HULL_LAYER_ALPHA : HULL_LAYER_DIM)
        : hlAnchor ? (it.hot ? HULL_ACTIVE_ALPHA : HULL_LAYER_DIM) : HULL_LAYER_ALPHA;
      ctx.fillStyle = it.col; ctx.fill();
      hullLabels.push({ cx: it.g.cx, cy: it.g.cy, text: it.rep.name + " (" + it.size + ")", col: it.col, hot: it.hot, lgHit: it.lgHit, size: it.size });
    };
    for (const it of items) if (!it.hot) paint(it);   // active hulls painted last so they sit on top
    for (const it of items) if (it.hot) paint(it);
    ctx.globalAlpha = 1;
    // Group labels: their own layer, right after the hulls (below edges/nodes). Greedy placement -
    // active first, then larger contexts - skips any label whose box overlaps one already placed, so
    // the map reads as spaced region names instead of a wall of overlapping text. A soft pill keeps
    // each name legible over the busy hull fills. Screen space, so restore the world transform after.
    if (showLabels && hullLabels.length) {
      ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0.3px";
      // Region-name style: font scales with context size (bigger domain -> bigger name), sorted so the
      // largest / active contexts win placement; anything overlapping an already-placed one is skipped.
      const cand = hullLabels
        .map(l => ({ l, px: l.cx*cam.s + cam.x, py: l.cy*cam.s + cam.y, fs: Math.min(30, Math.round(12 + Math.sqrt(l.size) * 2.2)) }))
        .filter(o => o.px > -80 && o.px < innerWidth + 80 && o.py > -30 && o.py < innerHeight + 30)
        .sort((a, b) => (b.l.hot - a.l.hot) || (b.l.size - a.l.size));
      const placed = [];
      for (const o of cand) {
        const l = o.l;
        ctx.font = (l.hot ? "600 " : "500 ") + o.fs + "px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
        const w = ctx.measureText(l.text).width + o.fs*0.5, h = o.fs*1.3, x = o.px - w/2, y = o.py - h/2;
        if (placed.some(p => x < p.x + p.w && x + w > p.x && y < p.y + p.h && y + h > p.y)) continue;
        placed.push({ x, y, w, h });
        // Blend into the map without a chip: a crisp background-color stroke (a cutout that matches the
        // canvas) keeps the group-color name sharp over the hull fills; lower idle opacity lets it recede.
        const a = lgActive ? (l.lgHit ? 0.75 : HULL_LABEL_HOVER_FADE)
          : hlAnchor ? (l.hot ? 1 : HULL_LABEL_HOVER_FADE) : 0.6;
        ctx.globalAlpha = a;
        ctx.lineWidth = Math.max(3, o.fs * 0.16); ctx.strokeStyle = SURFACE; ctx.strokeText(l.text, o.px, o.py);
        ctx.fillStyle = l.col; ctx.fillText(l.text, o.px, o.py);
      }
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";
      ctx.globalAlpha = 1; ctx.textAlign = "left"; ctx.textBaseline = "alphabetic";
      ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);   // back to world for edges/nodes
    }
  }
  // Relation labels for the active node's edges - collected during the edge draw (which has the curve
  // geometry), rendered later in the label pass. Only when a node is active and labels are on.
  const edgeLabels = (act && showLabels) ? [] : null;
  if (showEdges) {
    // Parallel-edge lanes: group visible edges by unordered node pair, so multiple links between the
    // same two nodes - a reciprocal pair (a<->b) OR several verbs in one direction (a->b) - fan out
    // into distinct arcs instead of stacking on one line. Same visibility filters as the loop.
    const pairList = new Map();
    for (let di = 0; di < edges.length; di++) {
      const de = edges[di];
      if (typeOff.has(de.a.type) || typeOff.has(de.b.type) || edgeTypeOff.has(de.type) || (de.valid_to && !showSuperseded)) continue;
      const pk = de.a.id < de.b.id ? de.a.id + "|" + de.b.id : de.b.id + "|" + de.a.id;
      if (!pairList.has(pk)) pairList.set(pk, []);
      pairList.get(pk).push(di);
    }
    for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
    if (edgeTypeOff.has(e.type)) continue;                 // edge-kind toggle (legend)
    if (e.valid_to && !showSuperseded) continue;           // hide superseded (past) edges (history toggle)
    const dx = e.b.x-e.a.x, dy = e.b.y-e.a.y, d = Math.hypot(dx,dy) || 1, ux = dx/d, uy = dy/d;
    // Grow each radius by half of that node's stroke so the edge meets outside the node stroke (the
    // stroke is radius-proportional, so it differs per endpoint) - the arrowhead tip touches the outer
    // stroke boundary, connecting with no gap/penetration, and it holds on zoom.
    const ar = nodeRadius(e.a) + nodeStrokeW(e.a)/2, br = nodeRadius(e.b) + nodeStrokeW(e.b)/2, room = d - ar - br;
    if (room <= 0.5) continue;   // (temporary) overlap - skip this frame's edge
    // Hovering an edge-kind chip treats its edges as hot (flow animation + weight) - same language as
    // node hover, so "what does this relation connect" reads instantly.
    const hot = act ? act.es.has(i) : (edgeTypeHl ? e.type === edgeTypeHl : false);
    // The line starts at the source node's edge and ends at the arrowhead base (or the tip if arrows are off).
    // Arrowhead length in WORLD units (proportional to the target node, bounded 7..16) so it scales
    // with zoom like the node markers. A fixed screen size looked tiny on large/zoomed-in nodes and
    // clunky when zoomed out. Still clamped to half the free gap so short edges never overrun it.
    const alen = Math.min(Math.max(7, Math.min(16, nodeRadius(e.b) * 0.7)), room * 0.5);
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    // Fan this edge onto its lane within the pair: lane index centered on 0, so a lone edge -> 0 ->
    // straight (quadratic control on the midpoint = the original straight line). sign uses a canonical
    // perpendicular (smaller id -> larger id) so lanes stay distinct regardless of each edge's
    // direction - a reciprocal pair opens into a lens, several one-way verbs into a fan.
    const pk = e.a.id < e.b.id ? e.a.id + "|" + e.b.id : e.b.id + "|" + e.a.id;
    const lst = pairList.get(pk) || [i], lane = lst.indexOf(i) - (lst.length - 1) / 2;
    const off = lane * Math.min(d * 0.30, 46) * (e.a.id < e.b.id ? 1 : -1);
    const cpx = (sx0 + tipx)/2 + (-uy)*off, cpy = (sy0 + tipy)/2 + ux*off;   // quadratic control point
    // Tip tangent = control -> tip; the line end and arrowhead align to it (reduces to ux,uy when straight).
    let tdx = tipx - cpx, tdy = tipy - cpy; const tdl = Math.hypot(tdx, tdy) || 1; tdx /= tdl; tdy /= tdl;
    const basex = tipx - tdx*alen, basey = tipy - tdy*alen;   // arrowhead base, back along the tip tangent
    // Active-edge label: relation type at the curve midpoint (quadratic B(0.5)). len = distance so the
    // shortest edges are labeled first when the count exceeds the cap.
    if (hot && edgeLabels) edgeLabels.push({
      mx: 0.25*sx0 + 0.5*cpx + 0.25*tipx, my: 0.25*sy0 + 0.5*cpy + 0.25*tipy,
      text: e.type, len: d, col: e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER),
    });
    // Group mode: make cross-group (different-type) edges stand out, in-group edges dim.
    const cross = clusterMode && e.a.type !== e.b.type;
    // The default is semi-transparent (EDGE_ALPHA); on hover/focus a connected edge (hot) activates to
    // 1.0 and the rest dim. Group mode is a separate emphasis that makes cross-group links stand out.
    // Legend hover outranks the rest: node-type chip -> both-endpoint edges bright, one-endpoint dim
    // halo, others faded; edge-kind chip -> matching edges full, others faded.
    ctx.globalAlpha = typeHl
      ? (e.a.type === typeHl && e.b.type === typeHl ? 0.9 : (e.a.type === typeHl || e.b.type === typeHl) ? 0.45 : 0.05)
      : edgeTypeHl
      ? (e.type === edgeTypeHl ? 1 : 0.05)
      : act ? (hot ? 1 : 0.06) : (clusterMode ? (cross ? 0.9 : 0.1) : EDGE_ALPHA);
    // Color is by relation kind - it reveals what kind of connection this is. A superseded edge is EDGE_OLD (a past signal, dashed).
    ctx.strokeStyle = e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER);
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // constant thickness on screen
    if (hot) {
      // Active edges show flow direction: marching dashes travel source -> target. Screen-constant
      // dash/speed (divide by cam.s) so it reads the same at any zoom. Negative offset -> forward flow.
      ctx.setLineDash([7/cam.s, 6/cam.s]);
      ctx.lineDashOffset = -(flowPhase * 0.6) / cam.s;
    } else {
      ctx.setLineDash(e.valid_to ? [5/cam.s, 5/cam.s] : []);
      ctx.lineDashOffset = 0;
    }
    // With arrows off, draw the curve to the node edge (tip); with arrows on, to the arrowhead base.
    const endx = showArrows ? basex : tipx, endy = showArrows ? basey : tipy;
    ctx.beginPath(); ctx.moveTo(sx0, sy0); ctx.quadraticCurveTo(cpx, cpy, endx, endy); ctx.stroke();
    ctx.setLineDash([]); ctx.lineDashOffset = 0;
    if (showArrows) {
      // Arrowhead: base -> tip along the tip tangent (points the way the curve arrives).
      const hw = alen * 0.55;
      ctx.beginPath(); ctx.moveTo(tipx, tipy);
      ctx.lineTo(basex - tdy*hw, basey + tdx*hw);
      ctx.lineTo(basex + tdy*hw, basey - tdx*hw);
      ctx.closePath(); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
    }
    }
  }
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = typeHl ? n.type === typeHl
      : edgeTypeHl ? lgEndpoints.has(n.id)
      : act ? act.ns.has(n.id) : true;
    ctx.globalAlpha = on ? 1 : 0.12;
    const r = nodeRadius(n);
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    // Default stroke (background-color halo): sharpens the node boundary and separates it from edges/neighbors (visibility).
    // Being the background color, it stays a cutout that matches the background even when the theme changes.
    ctx.lineWidth = nodeStrokeW(n); ctx.strokeStyle = SURFACE; ctx.stroke();
    if (n === anchor) { ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = INK; ctx.stroke(); }
    // Conversation footprint: nodes this session touched are marked with a persistent thin teal ring (footprint toggle).
    if (showFootprint && footprint.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 3.5, 0, 7);
      ctx.lineWidth = 1.5/cam.s; ctx.strokeStyle = TEAL; ctx.stroke();
    }
    // Group mode: bridge nodes (connected to another group) are marked with a faint ring - cross-group transit points.
    if (clusterMode && bridgeSet.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 2, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = INK; ctx.stroke();
    }
  }
  // Event pulses (nodes the agent touched) - an expanding, fading ring. rAF always runs, so it keeps
  // animating even after cooling.
  for (const [id, ttl] of pulses) {
    const n = nodeById(id);
    if (!n || ttl <= 0 || typeOff.has(n.type)) { pulses.delete(id); continue; }
    if (showPulses) {
      const t = 1 - ttl/60;
      ctx.globalAlpha = (1 - t) * 0.85;
      ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 3 + t*22, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = EDGE_HI; ctx.stroke();
    }
    pulses.set(id, ttl - 1);   // expiry advances even when hidden (avoids afterimages on toggle)
  }
  ctx.globalAlpha = 1;

  // Proposal preview (belief-diff visualization): when a proposal is selected, show on the graph what it
  // would change. For entity_merge: a dashed arrow from each fold-away target to the canonical `into`,
  // the target's incident edges accented (they rewire), a ring on each target and a canonical ring on
  // `into`. For an open proposal both nodes are still present (a before-preview); for a merged one the
  // targets are already folded away, so only the canonical is highlighted (the result).
  if (proposalSel && proposalSel.kind === "entity_merge") {
    const into = nodeById(proposalSel.into);
    const tgts = (proposalSel.targets || []).map(nodeById).filter(n => n && (!into || n.id !== into.id));
    const tgtIds = new Set(tgts.map(n => n.id));
    const ACC = GOLD, CANON = TEAL;
    ctx.setLineDash([5/cam.s, 4/cam.s]);
    for (const e of edges) {   // edges that will rewire onto the canonical
      if (tgtIds.has(e.a.id) || tgtIds.has(e.b.id)) {
        ctx.globalAlpha = 0.95; ctx.strokeStyle = ACC; ctx.lineWidth = 2/cam.s;
        ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke();
      }
    }
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
    for (const t of tgts) {
      ctx.beginPath(); ctx.arc(t.x, t.y, nodeRadius(t) + 4, 0, 7); ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = ACC; ctx.stroke();
      if (into) {   // fold arrow target -> into, with an arrowhead near into
        const dx = into.x - t.x, dy = into.y - t.y, d = Math.hypot(dx, dy) || 1, ux = dx/d, uy = dy/d;
        const tipx = into.x - ux*(nodeRadius(into) + 6), tipy = into.y - uy*(nodeRadius(into) + 6);
        ctx.strokeStyle = ACC; ctx.lineWidth = 2.5/cam.s;
        ctx.beginPath(); ctx.moveTo(t.x + ux*(nodeRadius(t) + 4), t.y + uy*(nodeRadius(t) + 4)); ctx.lineTo(tipx, tipy); ctx.stroke();
        const hl = Math.min(Math.max(8, nodeRadius(into) * 0.7), 16), hw = hl * 0.55;   // world units - scales with zoom like the edge arrowheads
        ctx.fillStyle = ACC; ctx.beginPath();
        ctx.moveTo(tipx, tipy);
        ctx.lineTo(tipx - ux*hl - uy*hw, tipy - uy*hl + ux*hw);
        ctx.lineTo(tipx - ux*hl + uy*hw, tipy - uy*hl - ux*hw);
        ctx.closePath(); ctx.fill();
      }
    }
    if (into) {
      ctx.beginPath(); ctx.arc(into.x, into.y, nodeRadius(into) + 6, 0, 7); ctx.lineWidth = 3/cam.s; ctx.strokeStyle = CANON; ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Proposal preview - T-Box change (belief-diff hint via affected_types): accent every edge whose
  // relation kind is being (re)defined and ring every node whose entity type is. A tbox_change edits
  // type definitions, which are edge kinds / node types (not first-class nodes), so the change shows as
  // a highlight over the members of those types rather than as a fold arrow. Kinds hidden via the legend
  // stay hidden (respect typeOff/edgeTypeOff) so the preview never contradicts the visible graph.
  if (proposalSel && proposalSel.affected_types && proposalSel.affected_types.length) {
    const { rel, ent } = affectedTypeSets(proposalSel);
    const ACC = GOLD;
    ctx.strokeStyle = ACC; ctx.globalAlpha = 0.95; ctx.lineWidth = 2.5/cam.s;
    if (rel.size) for (const e of edges) {
      if (typeOff.has(e.a.type) || typeOff.has(e.b.type) || edgeTypeOff.has(e.type)) continue;
      if (rel.has(e.type)) { ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke(); }
    }
    if (ent.size) for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      if (ent.has(n.type)) { ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 4, 0, 7); ctx.stroke(); }
    }
    ctx.globalAlpha = 1;
  }

  // Labels (nodes + hulls) - turned on/off by the labels toggle. In screen coordinates (DPR), so constant size regardless of zoom.
  if (showLabels) {
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    ctx.font = "12px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
    ctx.textBaseline = "middle";
    // Label thinning: everything when small (<=40) or zoomed in enough (cam.s>1.4); on a large graph,
    // only hubs (high degree >= cut) + hover/focus/active. Removes the hairball's wall of labels.
    const cut = (nodes.length <= 40 || cam.s > 1.4) ? 0 : Math.max(4, Math.round(nodes.length / 25));
    for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      // Legend hover: label exactly the highlighted set (bypasses the degree cut - hover is transient).
      const lg = typeHl ? n.type === typeHl : edgeTypeHl ? lgEndpoints.has(n.id) : null;
      if (lg === false) continue;
      const on = lg === true ? true : (act ? act.ns.has(n.id) : true);
      const show = on && (lg === true || n === hover || n === focus || (act && act.ns.has(n.id)) || (n.degree || 0) >= cut);
      if (!show) continue;
      const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = nodeRadius(n)*cam.s;
      ctx.globalAlpha = on ? 1 : 0.25;
      ctx.lineWidth = 3; ctx.strokeStyle = SURFACE; ctx.strokeText(n.name, px + r + 5, py);
      ctx.fillStyle = (n === focus || n === hover) ? INK : INK2; ctx.fillText(n.name, px + r + 5, py);
    }
    ctx.globalAlpha = 1;
    // (Group / hull labels are drawn in their own pass right after the hulls - see drawHullLabels below.)
    // Active-edge relation labels: only the hovered/focused node's edges, shortest first, capped so a
    // hub does not flood the canvas; the overflow is summarized as "+K more" under the active node.
    if (edgeLabels && edgeLabels.length) {
      edgeLabels.sort((p, q) => p.len - q.len);
      const shown = Math.min(edgeLabels.length, EDGE_LABEL_MAX);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.font = "10.5px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
      const pill = (px, py, w) => {
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(px - w/2 - 5, py - 8, w + 10, 16, 4);
        else ctx.rect(px - w/2 - 5, py - 8, w + 10, 16);
        ctx.fill();
      };
      for (let k = 0; k < shown; k++) {
        const L = edgeLabels[k];
        const px = L.mx*cam.s + cam.x, py = L.my*cam.s + cam.y, w = ctx.measureText(L.text).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = L.col; ctx.fillText(L.text, px, py);
      }
      const omitted = edgeLabels.length - shown, an = focus || hover;
      if (omitted > 0 && an) {
        ctx.font = "10px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
        const t = "+" + omitted + " more", px = an.x*cam.s + cam.x, py = an.y*cam.s + cam.y + nodeRadius(an)*cam.s + 15, w = ctx.measureText(t).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = INK2; ctx.fillText(t, px, py);
      }
      ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.globalAlpha = 1;
    }
  }
  // Peer cursor-dots (server mode) - drawn last so they float above the graph and labels.
  stepPeers(); drawPeers();
  requestAnimationFrame(draw);
}

function nodeAt(sx, sy) {
  const [wx, wy] = toWorld(sx, sy);
  let best = null, bd = 1e9;
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d < nodeRadius(n) + 6 && d < bd) { bd = d; best = n; }
  }
  return best;
}

function showTip(n, cx, cy) {
  if (!n) { tip.style.display = "none"; return; }
  tip.style.display = "block";
  tip.style.left = Math.min(cx + 14, innerWidth - 330) + "px";
  tip.style.top = (cy + 14) + "px";
  tip.innerHTML = `<b>${n.name}</b><br>`
    + `<span class="k">type</span> ${n.type} &nbsp; <span class="k">degree</span> ${n.degree || 0}<br>`
    + `<span class="k">sources</span> ${n.sources} &nbsp; <span class="k">trust</span> ${n.trust_tier}`;
}

// --- Interaction ---------------------------------------------------------------------
canvas.addEventListener("wheel", ev => {
  if (settling) return;   // graph is hidden behind the loader - ignore interaction until it settles
  ev.preventDefault();
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
}, { passive: false });

canvas.addEventListener("mousedown", ev => {
  if (settling) return;   // graph hidden (loading) - no drag/pan/select
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; }   // press alone does not reheat - wake only once the pointer actually drags (mousemove)
  else {
    // Clicking empty canvas clears any proposal preview.
    if (proposalSel) { proposalSel = null; renderProposals(); }
    panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing");
  }
});
addEventListener("mousemove", ev => {
  if (drag) { const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; wake(0.3); showTip(null); return; }
  if (panning) {
    // Panning is instant (1:1) - move cam and camT together so easing does not drag behind.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); return;
  }
  // Only canvas-targeted moves drive node hover from here on. Over the chrome (docks, header,
  // statusbar) the shared #tip belongs to whatever chrome element is showing it (legend chip
  // definitions) - and node hover through an opaque panel was wrong anyway.
  if (ev.target !== canvas) return;
  if (settling) { showTip(null); return; }   // no hover while the graph is hidden behind the loader
  const ph = peerAt(ev.clientX, ev.clientY);   // peer cursor-dots sit above the graph - test them first
  if (ph) { hover = null; showPeerTip(ph.m, ph.id, ev.clientX, ev.clientY); return; }
  hover = nodeAt(ev.clientX, ev.clientY);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  if (settling) { drag = null; panning = null; canvas.classList.remove("grabbing"); return; }   // drop any gesture that spanned into a reload
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    focus = n ? (focus === n ? null : n) : null;   // node click = toggle focus (pin), empty space = clear
    if (focus) centerOn(focus);    // node click just centers the camera - no reheat, the layout stays put
    renderDetail(focus);                           // show/clear the detail inspector
  }
  if (drag && moved) wake(0.3);   // settle neighbors only after a real drag, not a plain click
  drag = null;
  if (panning) { panning = null; canvas.classList.remove("grabbing"); }
  downPos = null;
});

searchEl.addEventListener("input", () => { searchTerm = searchEl.value.trim().toLowerCase(); });
searchEl.addEventListener("keydown", ev => {
  if (ev.key === "Enter" && searchTerm) {
    const hits = nodes.filter(n => n.name.toLowerCase().includes(searchTerm));
    if (hits.length) { fitView(hits, 140); userMoved = true; }
  }
});
document.getElementById("reload").onclick = () => { loadWorkspaces(); poll(); };
wsInput.addEventListener("keydown", e => { if (e.key === "Enter") { beginWorkspaceTransition(); renderChipsActive(); poll(); } });
document.getElementById("zin").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1.2);
document.getElementById("zout").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1/1.2);
document.getElementById("fit").onclick = () => { userMoved = true; fitView(); };
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; followBtn.classList.toggle("on", follow); };
const peersBtn = document.getElementById("peersBtn");
peersBtn.onclick = () => { peersOn = !peersOn; peersBtn.classList.toggle("on", peersOn); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; clusterBtn.classList.toggle("on", clusterMode); wake(0.6); poll(); };
const hyperBtn = document.getElementById("hyperBtn");
// Overlay is render-only (the draw loop shows it next frame) - do not reheat the sim. Fetch hyperedge
// data only if we do not already have it (e.g. when leaving group mode, where it was cleared).
hyperBtn.onclick = () => { hyperMode = !hyperMode; hyperBtn.classList.toggle("on", hyperMode); if (hyperMode && !hyperedges.length) poll(); };
const dockBtn = document.getElementById("dockBtn");
// The controls dock is a side panel (not a canvas layer) - toggling never reheats the sim.
dockBtn.onclick = () => { const on = !dockLEl.classList.contains("on"); dockLEl.classList.toggle("on", on); dockREl.classList.toggle("on", on); dockBtn.classList.toggle("on", on); };
// Fetch the glossary lazily when its section is expanded (and keep it fresh via poll while open).
// Dock tabs: clicking a tab shows only that panel (one at a time, fixed-height body - no upward growth),
// and refreshes the newly active data panel.
document.querySelectorAll(".dock .tabs").forEach(tabs => {
  tabs.addEventListener("click", ev => {
    const btn = ev.target.closest(".tab");
    if (!btn) return;
    const dock = tabs.closest(".dock");
    tabs.querySelectorAll(".tab").forEach(t => t.classList.toggle("on", t === btn));
    dock.querySelectorAll(".tabpanel").forEach(p => p.classList.toggle("on", p.dataset.panel === btn.dataset.tab));
    if (btn.dataset.tab === "glossary") refreshGlossary();
    if (btn.dataset.tab === "peers") refreshPeers();
    else if (btn.dataset.tab === "review") refreshCuration();
    else if (btn.dataset.tab === "proposals") refreshProposals();
  });
});
// Pure render toggles (no layout/data change - rAF reflects them every frame, so wake/poll is unnecessary).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; e.currentTarget.classList.toggle("on", showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; e.currentTarget.classList.toggle("on", showEdges); };
document.getElementById("arrowsBtn").onclick = e => { showArrows = !showArrows; e.currentTarget.classList.toggle("on", showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; e.currentTarget.classList.toggle("on", showFootprint); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; e.currentTarget.classList.toggle("on", showPulses); };
document.getElementById("histBtn").onclick = e => { showSuperseded = !showSuperseded; e.currentTarget.classList.toggle("on", showSuperseded); };

// Restore the workspace selection from the URL (deep link / reload) before the first poll.
{
  const qws = new URLSearchParams(location.search).get("workspace");
  if (qws !== null) wsInput.value = qws;
}
resize(); loadWorkspaces(); poll(); connectEvents(); detectServerMode();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); setInterval(refreshPeerRoster, 5000); draw();
</script>
"###;

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
