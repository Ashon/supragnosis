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
    let addr: SocketAddr = s.trim().parse().with_context(|| {
        format!("invalid SUPRAGNOSIS_VIZ_ADDR: {s:?} - must be in IP:port form (e.g. 127.0.0.1:7373)")
    })?;
    if !addr.ip().is_loopback() {
        anyhow::bail!(
            "SUPRAGNOSIS_VIZ_ADDR {addr} is not loopback - the viewer rejects non-local binds \
             (Principle 17: knowledge sovereignty). Use 127.0.0.1:<port>"
        );
    }
    Ok(addr)
}

/// Accepts connections on the injected listener and serves the viewer/graph API (infinite accept loop).
///
/// Binding is done by **the caller** (so a test can bind port 0 and look up the actual port).
/// Each connection is split off into a task, but an individual connection failure is swallowed
/// so it does not kill the server.
pub async fn serve(
    engine: Arc<Engine>,
    listener: TcpListener,
    events: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "viz accept failed - continuing");
                continue;
            }
        };
        let engine = Arc::clone(&engine);
        let events = events.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(&engine, &events, stream).await {
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

    let resp = route(engine, method, path, query);
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
        "/api/workspaces" => workspaces_response(engine),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "unknown path - try /, /api/graph, /api/hypergraph, /api/workspaces, or /api/events",
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
<style>
  :root {
    color-scheme: dark;
    --surface:#1a1a19; --ink:#ffffff; --ink2:#c3c2b7; --muted:#898781;
    --line:#4a4a46; --line-hi:#8fb4e6; --border:rgba(255,255,255,0.10); --accent:#3987e5;
  }
  * { box-sizing:border-box; }
  html,body { margin:0; height:100%; }
  body { background:var(--surface); color:var(--ink2); overflow:hidden;
         font:13px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif; }
  canvas { display:block; position:fixed; inset:0; cursor:grab; }
  canvas.grabbing { cursor:grabbing; }
  header { position:fixed; top:0; left:0; right:0; z-index:5; padding:9px 14px;
           display:flex; gap:9px; align-items:center; flex-wrap:wrap;
           background:var(--surface); border-bottom:1px solid var(--border); }
  h1 { font-size:14px; margin:0 6px 0 0; font-weight:600; color:var(--ink); }
  input,button { background:#22221f; border:1px solid var(--border); color:var(--ink);
                 padding:5px 9px; border-radius:7px; font:inherit; font-size:12.5px; }
  input::placeholder { color:var(--muted); }
  button { cursor:pointer; } button:hover { border-color:var(--line-hi); }
  .hint { color:var(--muted); font-size:12px; }
  #status { color:var(--muted); font-size:12px; margin-left:auto; white-space:nowrap; }
  #chrome { position:fixed; top:47px; left:0; right:0; z-index:5; padding:6px 14px 0;
            display:flex; flex-direction:column; gap:5px; pointer-events:none; }
  #chrome > * { pointer-events:auto; }
  #wschips,#legend { display:flex; gap:6px; flex-wrap:wrap; align-items:center; }
  .lbl { color:var(--muted); font-size:11.5px; margin-right:2px; }
  .chip,.lg { padding:2px 9px; border-radius:11px; background:#22221f; border:1px solid var(--border);
              cursor:pointer; font-size:12px; color:var(--ink2); user-select:none; }
  .chip.on { background:#2b3a52; border-color:var(--accent); color:var(--ink); }
  .lg { display:inline-flex; align-items:center; gap:6px; }
  .lg.off { opacity:0.38; }
  .sw { width:10px; height:10px; border-radius:3px; display:inline-block; }
  #stats { color:var(--muted); font-size:11.5px; }
  #tip { position:fixed; pointer-events:none; z-index:10; display:none; max-width:320px;
         background:#0d0d0df2; border:1px solid var(--border); border-radius:8px;
         padding:7px 10px; font-size:12.5px; color:var(--ink2); box-shadow:0 6px 20px #000a; }
  #tip b { color:var(--ink); }
  #tip .k { color:var(--muted); }
  #hud { position:fixed; right:12px; bottom:12px; z-index:5; display:flex; gap:6px; }
  #hud button { width:34px; height:34px; padding:0; font-size:16px; line-height:1;
                display:flex; align-items:center; justify-content:center; }
  #empty { position:fixed; inset:0; display:none; align-items:center; justify-content:center;
           color:var(--muted); font-size:13px; pointer-events:none; }
  /* Toggle button state: off = dim (muted), on = accent-highlighted - state is visible at a glance.
     JS toggles only .on and keeps .tog. Action buttons like reload/zoom (no .tog) stay at their default. */
  button.tog { opacity:.5; color:var(--muted); }
  button.tog:hover { opacity:.8; }
  button.tog.on { opacity:1; background:#2b3a52; border-color:var(--accent); color:var(--ink); }
  #log { position:fixed; left:12px; bottom:12px; z-index:6; width:300px; max-width:42vw;
         display:flex; flex-direction:column; gap:4px; pointer-events:none; }
  #log .row { background:#0d0d0de6; border:1px solid var(--border); border-radius:7px;
              padding:4px 9px; font-size:11.5px; color:var(--ink2); animation:logfade 8s forwards; }
  #log .row b { color:var(--accent); font-weight:600; }
  #log .row .t { color:var(--muted); margin-right:5px; }
  @keyframes logfade { 0%{opacity:0;transform:translateY(6px);} 6%{opacity:1;transform:none;}
                       82%{opacity:1;} 100%{opacity:0;} }
  #detail { position:fixed; top:92px; right:12px; z-index:7; width:272px; max-width:44vw;
            max-height:calc(100vh - 150px); overflow-y:auto; display:none;
            background:#0d0d0df2; border:1px solid var(--border); border-radius:10px;
            padding:11px 13px; font-size:12.5px; color:var(--ink2); box-shadow:0 8px 28px #000a; }
  #detail.on { display:block; }
  #detail h2 { font-size:14px; margin:0 22px 2px 0; color:var(--ink); font-weight:600; word-break:break-word; }
  #detail .meta { color:var(--muted); font-size:11.5px; margin-bottom:4px; }
  #detail .sec { color:var(--muted); font-size:10.5px; letter-spacing:.05em; text-transform:uppercase; margin:10px 0 3px; }
  #detail .row { display:flex; align-items:center; gap:6px; padding:3px 5px; border-radius:6px; cursor:pointer; }
  #detail .row:hover { background:#ffffff14; }
  #detail .row .rel { color:var(--muted); font-size:11px; white-space:nowrap; }
  #detail .row .nm { color:var(--ink); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  #detail .dot { width:9px; height:9px; border-radius:3px; flex:0 0 auto; display:inline-block; }
  #detail .close { position:absolute; top:9px; right:11px; cursor:pointer; color:var(--muted);
                   border:none; background:none; font-size:15px; line-height:1; padding:0; }
  #detail .close:hover { color:var(--ink); }
  #detail .empty { color:var(--muted); font-style:italic; padding:2px 5px; }
</style>
<canvas id="c"></canvas>
<div id="empty">no nodes in this workspace - observe knowledge, or pick another workspace</div>
<header>
  <h1>supragnosis ontology</h1>
  <input id="search" placeholder="search nodes" size="16" autocomplete="off">
  <label class="hint">ws <input id="ws" placeholder="(default)" size="11" autocomplete="off"></label>
  <span class="hint">*=all</span>
  <button id="reload">reload</button>
  <button id="followBtn" class="tog on" title="follow agent activity: workspace + camera">follow</button>
  <button id="clusterBtn" class="tog" title="group by type; keep cross-group links visible">group</button>
  <button id="hyperBtn" class="tog" title="draw hyperedge hulls: co-occurrence sets (size>=3), Principle 11 second-order structure">hulls</button>
  <button id="labelsBtn" class="tog on" title="toggle node/hull labels">labels</button>
  <button id="edgesBtn" class="tog on" title="toggle edges">edges</button>
  <button id="arrowsBtn" class="tog on" title="toggle edge direction arrowheads">arrows</button>
  <button id="footBtn" class="tog on" title="toggle session footprint rings">footprint</button>
  <button id="pulseBtn" class="tog on" title="toggle live activity pulses">pulses</button>
  <button id="histBtn" class="tog on" title="toggle superseded (past) edges">history</button>
  <span id="session" class="hint"></span>
  <span id="status"></span>
</header>
<div id="log"></div>
<div id="detail"></div>
<div id="chrome">
  <div id="wschips"></div>
  <div id="legend"></div>
  <div id="stats"></div>
</div>
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
const OTHER = "#898781", EDGE_OTHER = "#6b6b64";   // defensive neutral color for types not in the map
const EDGE = "#4a4a46", EDGE_HI = "#8fb4e6", EDGE_OLD = "#5a4a55";
const EDGE_ALPHA = 0.3;          // edge base opacity (low - recedes in a dense graph). On hover/focus, connected edges activate to 1.0
// The node stroke is proportional to the marker radius (scales with the marker on zoom - a
// consistent ratio) + a screen-px floor (so it does not vanish on zoom-out). It is a background-color
// halo, separating node/edge/neighbor (visibility).
const NODE_STROKE_RATIO = 0.35;  // stroke thickness ratio relative to radius (raised)
const NODE_STROKE_MIN = 2;       // minimum stroke thickness (screen px)
const NODE_STROKE_MAX = 5;       // maximum stroke thickness (screen px) - so a large hub does not thicken into a donut
const INK = "#ffffff", INK2 = "#c3c2b7", SURFACE = "#1a1a19";

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
const wsInput = document.getElementById("ws"), searchEl = document.getElementById("search");
const chipBar = document.getElementById("wschips"), legendEl = document.getElementById("legend");
const emptyEl = document.getElementById("empty"), logEl = document.getElementById("log");
const detailEl = document.getElementById("detail");

let follow = true;               // whether the camera follows the most recent agent-activity node
let clusterMode = false;         // separate layout by type group + highlight cross-group links/bridges
let hyperMode = false;           // hyperedge (co-occurrence second-order structure) hull overlay - Principle 11
let hyperedges = [];             // [{id, members:[nodeId], size, sources, trust_tier}] - /api/hypergraph
// Graphic-element visibility toggles (all default on). Pure render switches with no effect on layout.
let showLabels = true, showEdges = true, showArrows = true;
let showFootprint = true, showPulses = true, showSuperseded = true;
const bridgeSet = new Set();     // ids of nodes connected to another type (linking nodes that join groups)
const pulses = new Map();        // id -> remaining frames (event-node highlight ring animation)
const CLUSTER_PULL = 0.03;       // pull toward the group target point (stronger than the center attraction)
const HYPER_PULL = 0.03;         // hyperedge centroid cohesion (packs nodes inside a hull tightly)
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
// Base force parameters. The larger the graph, the wider it should spread, so stepSim scales by node count (spread).
const REPULSE = 7000, SPRING_LEN = 120, SPRING_K = 0.02;
const CENTER_BASE = 0.0015; // center-attraction base - weakened for large graphs (prevents central clumping)
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
function wake(a = 0.7) { alpha = Math.max(alpha, a); }

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
const TOP_INSET = 96;   // height occluded by the top header/chrome - compensated in centering/fit
// Smoothly bring a node to the screen center (focus-to-zoom). If zoomed too far out, zoom in slightly.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = innerWidth / 2 - n.x * camT.s;
  camT.y = (innerHeight + TOP_INSET) / 2 - n.y * camT.s; userMoved = true;
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

function renderLegend() {
  legendEl.innerHTML = "";
  // A legend for node types and one for edge kinds. Clicking toggles that kind's visibility (the off set).
  const addGroup = (label, keys, colorOf, offSet, isEdge) => {
    if (!keys.length) return;
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = label;
    legendEl.appendChild(lbl);
    for (const t of keys) {
      const el = document.createElement("span");
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      el.appendChild(sw); el.appendChild(document.createTextNode(t || "(none)"));
      el.title = "click to toggle visibility";
      el.onclick = () => { if (offSet.has(t)) offSet.delete(t); else offSet.add(t); renderLegend(); };
      legendEl.appendChild(el);
    }
  };
  addGroup("nodes:", Object.keys(typeColor).sort(), t => typeColor[t], typeOff, false);
  addGroup("edges:", Object.keys(edgeTypeColor).sort(), t => edgeTypeColor[t], edgeTypeOff, true);
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
    // Hyperedges (second-order structure) are fetched only in hull mode - otherwise cleared. As an
    // auxiliary channel, a failure still keeps the graph rendering (Principle 21: observability is optional).
    if (hyperMode) {
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
  } catch (e) { statusEl.textContent = "connection failed - check the server is running"; }
}

function currentWs() { return wsInput.value.trim(); }
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
      c.onclick = () => { wsInput.value = val; renderChipsActive(); poll(); };
      return c;
    };
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = "workspaces:";
    chipBar.replaceChildren(lbl, mk("(all)", "*"), ...list.map(w => mk(w, w)));
  } catch (e) { /* server not up - retry next cycle */ }
}

// --- Live MCP activity (SSE) --------------------------------------------------------
function nodeById(id) { return nodes.find(n => n.id === id); }
function esc(s) { return String(s).replace(/[<&>]/g, c => ({ "<": "&lt;", "&": "&amp;", ">": "&gt;" }[c])); }
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
  if (switched) { wsInput.value = ev.workspace; renderChipsActive(); }
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
  } else if (ev.kind === "traverse") {
    const sn = nodeById(ev.start);
    logRow(`<b>traverse</b> ${esc(sn ? sn.name : ev.start.slice(0,8))} -> ${(ev.reached||[]).length}`);
    ids = [ev.start, ...(ev.reached || [])];
  } else return;
  pulseNodes(ids);
  for (const id of ids) if (id) footprint.add(id);   // accumulate the conversation footprint (regardless of whether the node exists)
  wake(0.3);
  if (follow) { const n = primaryNode(ev); if (n) centerOn(n); }
  const sEl = document.getElementById("session");
  if (sEl) sEl.textContent = footprintSession ? `session ${footprintSession.slice(0,22)} / ${footprint.size} used` : "";
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
  const rowHtml = (rel, other, dir) =>
    `<div class="row" data-id="${esc(other.id)}" title="focus ${esc(other.name)}">`
    + `<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>`
    + `<span class="rel">${dir} ${esc(rel)}</span>`
    + `<span class="nm">${esc(other.name)}</span></div>`;
  const list = (arr, dir) => arr.length
    ? arr.map(e => rowHtml(e.type, dir === "->" ? e.b : e.a, dir)).join("")
    : `<div class="empty">none</div>`;
  detailEl.innerHTML =
    `<button class="close" title="close">x</button>`
    + `<h2>${esc(node.name)}</h2>`
    + `<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span> `
    + `${esc(node.type)} / deg ${node.degree || 0} / src ${node.sources} / ${esc(String(node.trust_tier))}</div>`
    + `<div class="sec">outgoing (${outs.length})</div>${list(outs, "->")}`
    + `<div class="sec">incoming (${ins.length})</div>${list(ins, "<-")}`;
  detailEl.className = "on";
  detailEl.querySelector(".close").onclick = () => { focus = null; renderDetail(null); };
  detailEl.querySelectorAll(".row").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) { focus = n; wake(0.3); centerOn(n); renderDetail(n); }
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
  camT.s = Math.max(0.15, Math.min(2.5, Math.min((w - pad*2) / gw, (h - pad*2 - TOP_INSET) / gh)));
  camT.x = w/2 - (a+c)/2*camT.s;
  camT.y = (h + TOP_INSET)/2 - (b+d)/2*camT.s;
}

// hyperedge id -> palette color (deterministic hash). Overlapping hulls blend semi-transparently (C1: overlap = connective tissue).
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
  if (hyperMode && active && hyperedges.length) {
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
    // (1) Cohesion: member -> its own centroid.
    for (const g of hgs) {
      for (const m of g.ms) {
        if (pinned(m)) continue;
        m.vx += (g.cx - m.x) * HYPER_PULL * alpha; m.vy += (g.cy - m.y) * HYPER_PULL * alpha;
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
}

function draw() {
  stepSim();
  easeCam();
  // Initial auto-fit: once after the layout settles (only before user interaction).
  if (needFit && alpha < ALPHA_MIN && !userMoved) { needFit = false; fitView(); }

  const act = activeSet();
  const anchor = focus || hover;
  ctx.setTransform(1,0,0,1,0,0);
  ctx.clearRect(0,0,canvas.width,canvas.height);

  // Edges + nodes use the world transform (zoom/pan, incl. DPR supersampling); labels use screen coordinates (keeping readability).
  ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);
  ctx.lineCap = "round"; ctx.lineJoin = "round";
  // Hyperedge hull overlay (laid behind edges/nodes). Only size>=3 is drawn - 2 converges to a binary
  // edge and does not help ease density. Being semi-transparent, overlapping contexts blend (C1: overlap = connective tissue).
  // The hull a hovered node belongs to is highlighted, and the representative concept (highest-degree member) + size label is collected.
  const hullLabels = [];
  if (hyperMode && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    const PAD = 18;
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
      if (ms.length < 3) continue;
      const hull = convexHull(ms.map(m => ({ x: m.x, y: m.y })));
      if (hull.length < 3) continue;
      let cx = 0, cy = 0; for (const q of hull) { cx += q.x; cy += q.y; } cx /= hull.length; cy /= hull.length;
      const col = hyperColor(h.id);
      const hot = hover && h.members.includes(hover.id);   // does the hovered node belong to this context
      ctx.beginPath();
      hull.forEach((q, i) => {
        const dx = q.x - cx, dy = q.y - cy, d = Math.hypot(dx, dy) || 1;
        const px = q.x + dx/d*PAD, py = q.y + dy/d*PAD;
        if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      });
      ctx.closePath();
      // Fill only, no stroke (a soft area). To offset the missing outline, base alpha is raised a little
      // to keep individual hulls legible, and overlapping areas naturally darken via alpha accumulation (a density cue, C1).
      ctx.globalAlpha = hot ? 0.26 : 0.12; ctx.fillStyle = col; ctx.fill();
      // Representative concept = highest-degree member (the hub). The label is drawn later in screen coordinates (constant size).
      let anchor = ms[0]; for (const m of ms) if ((m.degree||0) > (anchor.degree||0)) anchor = m;
      hullLabels.push({ cx, cy, text: anchor.name + " (" + ms.length + ")", col, hot });
    }
    ctx.globalAlpha = 1;
  }
  if (showEdges) for (let i = 0; i < edges.length; i++) {
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
    const hot = act ? act.es.has(i) : false;
    // The line starts at the source node's edge and ends at the arrowhead base (or the tip if arrows are off).
    const alen = Math.min(9/cam.s, Math.max(3/cam.s, room * 0.5));   // ~9px on screen, shrinks when close
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    const basex = tipx - ux*alen, basey = tipy - uy*alen;
    // Group mode: make cross-group (different-type) edges stand out, in-group edges dim.
    const cross = clusterMode && e.a.type !== e.b.type;
    // The default is semi-transparent (EDGE_ALPHA); on hover/focus a connected edge (hot) activates to
    // 1.0 and the rest dim. Group mode is a separate emphasis that makes cross-group links stand out.
    ctx.globalAlpha = act ? (hot ? 1 : 0.06) : (clusterMode ? (cross ? 0.9 : 0.1) : EDGE_ALPHA);
    // Color is by relation kind - it reveals what kind of connection this is. A superseded edge is EDGE_OLD (a past signal, dashed).
    ctx.strokeStyle = e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER);
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // constant thickness on screen
    ctx.setLineDash(e.valid_to ? [5/cam.s, 5/cam.s] : []);
    // With arrows off, draw the line to the node edge (tip); with arrows on, to the arrowhead base.
    const endx = showArrows ? basex : tipx, endy = showArrows ? basey : tipy;
    ctx.beginPath(); ctx.moveTo(sx0, sy0); ctx.lineTo(endx, endy); ctx.stroke();
    ctx.setLineDash([]);
    if (showArrows) {
      // Arrowhead: base -> tip (the destination node's edge). The line ends at base so it does not overlap the triangle.
      const hw = alen * 0.55;
      ctx.beginPath(); ctx.moveTo(tipx, tipy);
      ctx.lineTo(basex - uy*hw, basey + ux*hw);
      ctx.lineTo(basex + uy*hw, basey - ux*hw);
      ctx.closePath(); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
    }
  }
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = act ? act.ns.has(n.id) : true;
    ctx.globalAlpha = on ? 1 : 0.12;
    const r = nodeRadius(n);
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    // Default stroke (background-color halo): sharpens the node boundary and separates it from edges/neighbors (visibility).
    // Being the background color, it stays a cutout that matches the background even when the theme changes.
    ctx.lineWidth = nodeStrokeW(n); ctx.strokeStyle = SURFACE; ctx.stroke();
    if (n === anchor) { ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = INK; ctx.stroke(); }
    // Conversation footprint: nodes this session touched are marked with a persistent thin purple ring (footprint toggle).
    if (showFootprint && footprint.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 3.5, 0, 7);
      ctx.lineWidth = 1.5/cam.s; ctx.strokeStyle = "#9085e9"; ctx.stroke();
    }
    // Group mode: bridge nodes (connected to another group) are marked with a faint ring - cross-group transit points.
    if (clusterMode && bridgeSet.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 2, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = "#c0caf5"; ctx.stroke();
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

  // Labels (nodes + hulls) - turned on/off by the labels toggle. In screen coordinates (DPR), so constant size regardless of zoom.
  if (showLabels) {
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    ctx.font = "12px system-ui,-apple-system,sans-serif";
    ctx.textBaseline = "middle";
    // Label thinning: everything when small (<=40) or zoomed in enough (cam.s>1.4); on a large graph,
    // only hubs (high degree >= cut) + hover/focus/active. Removes the hairball's wall of labels.
    const cut = (nodes.length <= 40 || cam.s > 1.4) ? 0 : Math.max(4, Math.round(nodes.length / 25));
    for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      const on = act ? act.ns.has(n.id) : true;
      const show = on && (n === hover || n === focus || (act && act.ns.has(n.id)) || (n.degree || 0) >= cut);
      if (!show) continue;
      const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = nodeRadius(n)*cam.s;
      ctx.globalAlpha = on ? 1 : 0.25;
      ctx.lineWidth = 3; ctx.strokeStyle = SURFACE; ctx.strokeText(n.name, px + r + 5, py);
      ctx.fillStyle = (n === focus || n === hover) ? INK : INK2; ctx.fillText(n.name, px + r + 5, py);
    }
    ctx.globalAlpha = 1;
    // Hyperedge hull labels (representative concept + size). The hovered hull is darker. textAlign is restored.
    if (hullLabels.length) {
      ctx.font = "11px system-ui,-apple-system,sans-serif";
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      for (const l of hullLabels) {
        const px = l.cx*cam.s + cam.x, py = l.cy*cam.s + cam.y;
        ctx.globalAlpha = l.hot ? 1 : 0.7;
        ctx.lineWidth = 3.5; ctx.strokeStyle = SURFACE; ctx.strokeText(l.text, px, py);
        ctx.fillStyle = l.col; ctx.fillText(l.text, px, py);
      }
      ctx.textAlign = "left"; ctx.globalAlpha = 1;
    }
  }
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
  ev.preventDefault();
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
}, { passive: false });

canvas.addEventListener("mousedown", ev => {
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; wake(0.3); }
  else { panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing"); }
});
addEventListener("mousemove", ev => {
  if (drag) { const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; showTip(null); return; }
  if (panning) {
    // Panning is instant (1:1) - move cam and camT together so easing does not drag behind.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); return;
  }
  hover = nodeAt(ev.clientX, ev.clientY);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    focus = n ? (focus === n ? null : n) : null;   // node click = toggle focus (pin), empty space = clear
    if (focus) { wake(0.3); centerOn(focus); }    // focus-to-zoom: smoothly center on that node
    renderDetail(focus);                           // show/clear the detail inspector
  }
  if (drag) wake(0.3);
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
wsInput.addEventListener("keydown", e => { if (e.key === "Enter") { renderChipsActive(); poll(); } });
document.getElementById("zin").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1.2);
document.getElementById("zout").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1/1.2);
document.getElementById("fit").onclick = () => { userMoved = true; fitView(); };
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; followBtn.classList.toggle("on", follow); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; clusterBtn.classList.toggle("on", clusterMode); wake(0.6); poll(); };
const hyperBtn = document.getElementById("hyperBtn");
hyperBtn.onclick = () => { hyperMode = !hyperMode; hyperBtn.classList.toggle("on", hyperMode); wake(0.6); poll(); };
// Pure render toggles (no layout/data change - rAF reflects them every frame, so wake/poll is unnecessary).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; e.currentTarget.classList.toggle("on", showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; e.currentTarget.classList.toggle("on", showEdges); };
document.getElementById("arrowsBtn").onclick = e => { showArrows = !showArrows; e.currentTarget.classList.toggle("on", showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; e.currentTarget.classList.toggle("on", showFootprint); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; e.currentTarget.classList.toggle("on", showPulses); };
document.getElementById("histBtn").onclick = e => { showSuperseded = !showSuperseded; e.currentTarget.classList.toggle("on", showSuperseded); };

resize(); loadWorkspaces(); poll(); connectEvents();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); draw();
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
