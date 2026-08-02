//! supragnosis-viz - live ontology visualization (local unix-socket viewer).
//!
//! A **human-facing read channel**, distinct from the MCP tool surface (Principle 21). It rides
//! inside the server process and shares the same `Arc<Engine>` (the embedded store is single-process
//! constraint), so it exposes the `engine.graph()` projection directly, without the lock conflict
//! that opening the db from a separate process would cause.
//!
//! - `GET /` -> self-contained canvas viewer (0 external CDNs). Polls `/api/graph` every few seconds to refresh.
//! - `GET /api/graph[?workspace=<ws>]` -> `engine.graph(ws)` JSON (Principle 16: deterministic ordering).
//!
//! It speaks HTTP over a **unix domain socket**, never TCP (Principle 17: knowledge sovereignty).
//! The socket file (0600, inside the 0700 `~/.supragnosis` dir) is the whole access control: only
//! the owning user can connect, so every request is attributable to the local principal (F19), and
//! the browser-borne attack classes a localhost port invites (DNS rebinding, CSRF, cross-site
//! fetch) cannot reach a unix socket at all. Clients are the desktop shell, or any HTTP-over-UDS
//! client (e.g. `curl --unix-socket`). The authenticated network read tier is federation Phase 3.5
//! and rides the sync crate's TLS stack, not this server.

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use supragnosis_engine::{Engine, EventEnvelope, EventSink};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
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
/// Live federation status blob, maintained by the wiring layer (the CLI's status task) and served
/// verbatim at /api/federation - the viz stays decoupled from the sync crate (it renders JSON).
pub type FedStatus = Arc<std::sync::RwLock<serde_json::Value>>;

/// Narrows one peer's shared workspaces: `(node_id, keep) -> Ok(now granted) | Err(reason)`.
///
/// Injected by the wiring layer rather than implemented here, for the same reason [`FedStatus`] is a
/// JSON blob: this crate does not depend on the sync crate or know what a config file is. It renders
/// and routes; the policy - narrow-only, re-read before write, update the live directory - lives
/// where the config and the admission directory already do.
pub type NarrowShare = Arc<dyn Fn(&str, &[String]) -> Result<Vec<String>, String> + Send + Sync>;

pub async fn serve(
    engine: Arc<Engine>,
    listener: UnixListener,
    events: broadcast::Sender<String>,
    fed: Option<FedStatus>,
    narrow: Option<NarrowShare>,
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
        tokio::spawn(async move {
            if let Err(e) = handle_conn(&engine, &events, stream, fed.as_ref(), narrow.as_ref()).await {
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
    engine: &Engine,
    events: &broadcast::Sender<String>,
    mut stream: S,
    fed: Option<&FedStatus>,
    narrow: Option<&NarrowShare>,
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

    let resp = if path == "/api/peer/share" {
        // POST, unlike every other endpoint here. The GET-with-side-effect convention the rest of
        // this surface uses is defensible because no third-party origin can reach a 0600 unix socket,
        // and that argument still holds - but this one changes a sharing boundary rather than
        // appending a gated verdict, and the method is the cheapest place to say so. Parameters stay
        // in the query string, so the minimal server still needs no body parser.
        narrow_share_response(method, query, narrow)
    } else if path == "/api/federation" {
        // Federation status (hubs, health, per-workspace diff, known peers) - maintained by the
        // wiring layer; absent on a standalone node.
        Response {
            status: "200 OK",
            content_type: "application/json",
            body: fed
                .map(|f| f.read().map(|v| v.to_string()).unwrap_or_else(|_| "{}".into()))
                .unwrap_or_else(|| "{\"configured\":false}".to_string()),
        }
    } else {
        route(engine, method, path, query)
    };
    write_response(&mut stream, &resp).await
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
        // merge (absorbing state, I14/I16). The unix socket's 0600 mode is the write gate (Principle 17 /
        // F19: only the owning user can connect). It routes through the gate, never a direct
        // projection/log write (I18 / proposal-workflow.md 14.3).
        //
        // What makes a state-changing GET safe here is that no THIRD-PARTY origin can reach the socket,
        // not that no browser can: the desktop shell proxies this surface into a webview (`viz://`), and
        // the console needs these very endpoints. With no attacker origin there is nothing for CSRF or
        // rebinding to ride, and the class that remains is stored XSS in the page we serve - which is
        // what the escaping guard and the `no-unsanitized` lint exist for (architecture.md Section 10).
        "/api/review" => review_response(engine, query),
        // One-click mediation for a contested belief (resolution.md Section 4.2): opens a
        // claim_promotion for the chosen observation(s) and immediately casts the Console merge
        // verdict - both are gated, appended events (propose + review), never a direct write. Solo
        // self-approval is the P23 exception; the Console surface is what permits human_confirmed.
        "/api/resolve" => resolve_response(engine, query),
        // Hyperedge promotion path (Principle 11): reify a co-occurrence context into a group
        // entity + member_of relations, as an ordinary lineage-bearing observation (free ingest,
        // P22 - trust promotion of the reified claim goes through the gate afterwards).
        "/api/reify" => reify_response(engine, query),
        // Merge-band action (resolution-identity.md Section 3): open an entity_merge proposal for a
        // suggested pair - the suggestion is a recall aid; committing goes through the gate (P23).
        // The opened proposal then rides the normal accept flow in the proposals panel (IR2).
        "/api/proposal" => proposal_response(engine, query),
        "/api/propose_merge" => propose_merge_response(engine, query),
        "/api/workspaces" => workspaces_response(engine),
        // The observation log (source of truth, Principle 1), newest-first; `entity=<id>` narrows to
        // the evidence set behind one node. A read-only projection (Principle 5: failure != empty).
        "/api/observations" => observations_response(engine, query),
        // "Why is this node projected this way" (resolution.md): per-field belief resolution
        // (evidence + decision) + the supporting log for one entity. Read-only, consistent with graph.
        "/api/explain" => explain_response(engine, query),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "unknown path - try /, /api/proposal, /api/graph, /api/hypergraph, /api/types, /api/curation, /api/proposals, /api/review, /api/resolve, /api/reify, /api/propose_merge, /api/workspaces, /api/observations, /api/explain, or /api/events",
            ),
        },
    }
}

/// A workspace to WRITE into. The read endpoints resolve `*`/`all`/empty to "every workspace" - a
/// VIEW, not a name, and a view is not somewhere an observation can be filed. Forwarding the
/// sentinel would create a workspace literally called `*` and quietly strand everything written
/// there. None means the node default, which is what an unspecified workspace has always meant on
/// the write path. Read and write must not disagree about what `*` is (Principle 17).
fn write_workspace(raw: Option<String>) -> Option<String> {
    raw.filter(|s| s != "*" && s != "all")
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

/// `/api/observations[?workspace=&entity=&limit=]` - the observation log (the source of truth,
/// Principle 1), newest-first. Workspace resolution is identical to `/api/graph`. `entity=<id>`
/// narrows to the evidence set behind one node (forwarded through accepted merges); `limit` keeps
/// the newest N. A storage failure is 500 + error body (Principle 5: a failure is not an empty log).
fn observations_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let ws_owned: Option<String> = match param("workspace").as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    let entity = param("entity").filter(|s| !s.is_empty());
    let limit = param("limit").and_then(|s| s.parse::<usize>().ok());
    match engine.observation_log(ws_owned.as_deref(), entity.as_deref(), limit) {
        Ok(log) => match serde_json::to_string(&log) {
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
                "note": "storage backend failure - NOT an empty log (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/explain?entity=<id>` - "why is this node projected this way" (resolution.md Section 4): the
/// per-field belief resolution (evidence + decision) and the supporting observation log for one
/// entity. The workspace is derived from the entity itself, so only the id is needed.
/// Consistent-by-construction with `/api/graph` (built on `get_entity`). A missing `entity` is 400;
/// an id that resolves to no entity is 404 (absence, Principle 5); a storage failure is 500.
fn explain_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let Some(entity) = param("entity").filter(|s| !s.is_empty()) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("explain needs ?entity=<id> (ids come from /api/graph nodes)"),
        };
    };
    match engine.explain_entity(&entity) {
        Ok(Some(ex)) => match serde_json::to_string(&ex) {
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
        Ok(None) => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body("no entity for that id (it may have been merged away, or never existed)"),
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an absent entity (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/proposals` - the workspace's proposals with folded state (Principle 23). Read-only projection.
/// `/api/proposal?id=<id>` - ONE proposal with its computed belief diff (proposal-workflow.md
/// Section 5). Separate from `/api/proposals` on purpose: a diff is two full belief folds, which is
/// the right cost for the proposal being reviewed and the wrong cost per row of a list. This is what
/// lets the console show what a verdict would change BEFORE casting it, rather than the canvas
/// overlay's guess from target ids alone.
fn proposal_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let Some(id) = param("id") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("proposal needs ?id=<proposal id>"),
        };
    };
    let ws = param("workspace").filter(|s| s != "*" && s != "all");
    match engine.get_proposal(ws.as_deref(), &id) {
        Ok(Some(view)) => match serde_json::to_string(&view) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        // Principle 5: an unknown id is not-found, not an error, and says which it is.
        Ok(None) => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body("no proposal with that id in this workspace"),
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&format!("store error: {e}")),
        },
    }
}

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
    let workspace = write_workspace(param("workspace"));
    // The Console surface (resolution.md Section 6): this server is reachable only through the 0600
    // unix socket, i.e. by the local OS principal - the engine stamps the console marker, which is
    // what permits a merged promotion to grant human_confirmed (a human's direct act, Principle 18).
    match engine.review_proposal(
        workspace,
        proposal,
        decision,
        None,
        None,
        supragnosis_engine::VerdictSurface::Console,
    ) {
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

/// `/api/resolve?observation=<id>[&observation=<id>...]&tier=<tier>[&workspace=<ws>][&rationale=<text>]` -
/// mediate a contested belief from the curation console (resolution.md Section 4.2): opens a
/// claim_promotion for the chosen observation(s) at the requested tier and immediately casts the
/// Console merge verdict. Both steps are gated appended events (I1/I18) - the projection changes only
/// because the fold consumes the verdict, never by a direct write. Solo self-approval is the P23
/// exception, and the verdict stays self-attested.
fn resolve_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let observations: Vec<String> = query
        .split('&')
        .filter_map(|kv| kv.strip_prefix("observation="))
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(tier) = param("tier") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("resolve needs ?observation=<id>&tier=<unverified|agent_extracted|host_signed|human_confirmed>"),
        };
    };
    if observations.is_empty() {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("resolve needs at least one ?observation=<id> (the asserting observation of the value you confirm)"),
        };
    }
    let workspace = write_workspace(param("workspace"));
    let rationale = param("rationale");
    let proposal = match engine.propose(supragnosis_engine::ProposeInput {
        workspace: workspace.clone(),
        kind: "claim_promotion".into(),
        targets: observations,
        into: None,
        tier: Some(tier),
        rationale: rationale.or_else(|| Some("confirmed from the curation console (contested belief mediation)".into())),
        affected_types: Vec::new(),
        source_ref: None,
        on_behalf_of: None,
    }) {
        Ok(id) => id,
        Err(e) => {
            return Response {
                status: "400 Bad Request",
                content_type: "application/json",
                body: err_body(&e.to_string()),
            }
        }
    };
    match engine.review_proposal(
        workspace,
        proposal.clone(),
        "merge".into(),
        None,
        None,
        supragnosis_engine::VerdictSurface::Console,
    ) {
        Ok(verdict) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({ "proposal_id": proposal, "verdict_observation_id": verdict }).to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            // The proposal was opened but the verdict failed - report both so the user can review it
            // by hand from the proposals panel (the proposal itself commits nothing, P23).
            body: serde_json::json!({ "error": e.to_string(), "proposal_id": proposal }).to_string(),
        },
    }
}

/// `/api/reify?hyperedge=<id>[&name=<text>][&kind=<type>][&workspace=<ws>]` - reify a co-occurrence
/// context into first-class structure (Principle 11's promotion path): asserts a group entity +
/// member_of relations through the normal observe ingest, with `derived_from` naming every
/// co-asserting observation (P18 lineage). The hyperedge itself is untouched (a derived view);
/// the reified claim enters at the default tier and rises only through the gate.
fn reify_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let Some(hyperedge) = param("hyperedge") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("reify needs ?hyperedge=<id> (ids come from /api/hypergraph)"),
        };
    };
    match engine.reify_hyperedge(supragnosis_engine::ReifyInput {
        workspace: write_workspace(param("workspace")),
        hyperedge,
        name: param("name"),
        kind: param("kind"),
        source_ref: None,
        on_behalf_of: None,
    }) {
        Ok(out) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({
                "observation_id": out.observation_id,
                "entities": out.entities,
                "relations": out.relations,
            })
            .to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// `/api/propose_merge?a=<id>&b=<id>[&workspace=<ws>]` - open an entity_merge proposal for a merge-
/// band suggestion (resolution-identity.md Section 3). `b` is the canonical (`into`) target the pair
/// folds into. This only OPENS the proposal (a gated appended event); the human accepts it in the
/// proposals panel, so the suggestion -> proposal -> verdict path stays gated (IR2/P23).
fn propose_merge_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let (Some(a), Some(b)) = (param("a"), param("b")) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("propose_merge needs ?a=<id>&b=<id> (ids from a merge suggestion)"),
        };
    };
    // Which surface produced the suggestion decides the recorded rationale. A proposal is an
    // immutable observation, so the reason it carries has to be TRUE: the deterministic name-variant
    // ladder needs no embedder, and filing its candidates as "embedding-near" would put a false
    // account of the evidence into the permanent log (Principle 2). Whitelisted rather than
    // free-text, so a client cannot shape what the log says about its own provenance (Principle 18).
    let rationale = match param("src").as_deref() {
        Some("variant:separator") => "name-variant ladder (separator/case normalization)",
        Some("variant:plural") => "name-variant ladder (plural fold)",
        Some("variant:alias") => "name-variant ladder (alias match)",
        _ => "merge-band suggestion (embedding-near names)",
    };
    match engine.propose(supragnosis_engine::ProposeInput {
        workspace: write_workspace(param("workspace")),
        kind: "entity_merge".into(),
        targets: vec![a, b.clone()],
        into: Some(b),
        tier: None,
        rationale: Some(rationale.into()),
        affected_types: Vec::new(),
        source_ref: None,
        on_behalf_of: None,
    }) {
        Ok(id) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({ "proposal_id": id }).to_string(),
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
/// `POST /api/peer/share?node_id=<id>&workspaces=a,b` - narrows what one federation peer may read.
///
/// Narrow-only, enforced by the injected handler: a request naming a workspace the peer does not
/// already hold is refused rather than partially applied. An empty `workspaces` is a legitimate
/// narrowing and means "admitted, but may read nothing" - which is why an absent parameter and an
/// empty one are distinguished here rather than both defaulting to "no change".
///
/// This does not remove the peer. Revocation, and the question of what it means for knowledge that
/// has already synced, is the deferred workflow in federation.md Section 11.
fn narrow_share_response(method: &str, query: &str, narrow: Option<&NarrowShare>) -> Response {
    if method != "POST" {
        return Response {
            status: "405 Method Not Allowed",
            content_type: "application/json",
            body: err_body("narrowing a peer's shared workspaces is a POST"),
        };
    }
    let Some(narrow) = narrow else {
        return Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "this node has no federation server role - there is no allowlist to narrow \
                 (docs/federation.md Section 9)",
            ),
        };
    };
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let Some(node_id) = param("node_id").filter(|s| !s.is_empty()) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("needs ?node_id=<peer node id>&workspaces=<comma separated, may be empty>"),
        };
    };
    // Absent means "you did not say", empty means "none" - collapsing them would turn a missing
    // parameter into a silent full revocation of reads.
    let Some(raw) = param("workspaces") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(
                "needs ?workspaces=... - pass it empty to narrow to nothing, omit nothing by accident",
            ),
        };
    };
    let keep: Vec<String> =
        raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();

    match narrow(&node_id, &keep) {
        Ok(now) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({"node_id": node_id, "shared_workspaces": now}).to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e),
        },
    }
}

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

async fn write_response<S: AsyncWrite + Unpin>(stream: &mut S, r: &Response) -> anyhow::Result<()> {
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
    fn percent_decode_basics() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        // Invalid sequences keep the original text.
        assert_eq!(percent_decode("a%zz"), "a%zz");
    }
}
