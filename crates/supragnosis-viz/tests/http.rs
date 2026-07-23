//! viz unix-socket HTTP surface integration test. Assembles a deterministic Engine (InMemory +
//! hashing embedder) in-process, binds a real unix socket under a per-test temp path, and fires
//! raw HTTP/1.1 GETs over `UnixStream` (no TCP anywhere - the surface under test is the socket).

use std::path::PathBuf;
use std::sync::Arc;

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, EntityInput, Event, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::broadcast;

/// Event channel for tests - the broadcast Sender to pass to serve.
fn ev_channel() -> broadcast::Sender<String> {
    broadcast::channel::<String>(16).0
}

/// Per-test socket path under the OS temp dir. Short (macOS caps sun_path at 104 bytes) and unique
/// per process + test name, so parallel test runs cannot collide.
fn sock_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("supra-viz-{}-{name}.sock", std::process::id()))
}

/// Bring up the viewer on a fresh unix socket and return its path (the server task runs detached).
async fn serve_uds(
    name: &str,
    engine: Arc<Engine>,
    events: broadcast::Sender<String>,
) -> PathBuf {
    let path = sock_path(name);
    let _ = std::fs::remove_file(&path); // leftover from a previous run of this same test
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    tokio::spawn(supragnosis_viz::serve(engine, listener, events, None));
    path
}

/// One raw HTTP/1.1 GET over the unix socket; returns the full response text (head + body).
/// Responses are Connection: close, so read-to-EOF terminates.
async fn uds_get(path: &PathBuf, target: &str) -> String {
    let mut s = UnixStream::connect(path).await.expect("connect viewer socket");
    let req = format!("GET {target} HTTP/1.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).await.unwrap();
    resp
}

/// Splits a raw response into (status line, body) and asserts the expected status.
fn body_of<'a>(resp: &'a str, want_status: &str) -> &'a str {
    let status = resp.lines().next().unwrap_or("");
    assert!(
        status.contains(want_status),
        "expected {want_status}, got: {status}"
    );
    resp.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("")
}

fn json_get(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp, "200")).expect("valid JSON body")
}

fn observe_depends(engine: &Engine) {
    engine
        .observe(ObserveInput {
            content: "supragnosis depends on rmcp".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { description: None,
                    name: "supragnosis".into(),
                    kind: Some("Project".into()),
                },
                EntityInput { description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                },
            ],
            relations: vec![RelationInput { description: None,
                from: "supragnosis".into(),
                kind: "depends_on".into(),
                to: "rmcp".into(),
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe succeeds");
}

#[tokio::test]
async fn viz_serves_graph_index_and_404() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    observe_depends(&engine);
    let sock = serve_uds("graph", engine, ev_channel()).await;

    // /api/graph?workspace=ws -> 2 nodes, 1 edge.
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    assert_eq!(g["stats"]["node_count"], 2, "graph: {g}");
    assert_eq!(g["stats"]["edge_count"], 1);
    assert_eq!(g["edges"][0]["type"], "depends_on");

    // workspace unspecified -> the node's default ws ("ws") scope -> same 2 nodes.
    let g2 = json_get(&uds_get(&sock, "/api/graph").await);
    assert_eq!(g2["stats"]["node_count"], 2);

    // '*' -> everything (None) -> same.
    let g3 = json_get(&uds_get(&sock, "/api/graph?workspace=*").await);
    assert_eq!(g3["stats"]["node_count"], 2);

    // Index HTML - the canvas viewer, linking the split-out assets.
    let idx = uds_get(&sock, "/").await;
    assert!(idx.contains("Content-Type: text/html; charset=utf-8"), "index content-type");
    let html = body_of(&idx, "200");
    assert!(html.contains("<canvas"), "the viewer HTML must contain a canvas");
    // Path substrings (not full attribute text) so the assertion survives release HTML minification,
    // which may drop the attribute quotes.
    assert!(html.contains("/viewer.css"), "the index must link the stylesheet asset");
    assert!(html.contains("/viewer.js"), "the index must link the script asset");

    // The split-out JS asset is served (same origin) with a JS content type and drives the API.
    let js = uds_get(&sock, "/viewer.js").await;
    assert!(js.contains("Content-Type: text/javascript; charset=utf-8"), "js content-type");
    assert!(body_of(&js, "200").contains("/api/graph"), "the script must poll the graph API");

    // The split-out CSS asset is served with a CSS content type.
    let css = uds_get(&sock, "/viewer.css").await;
    assert!(css.contains("Content-Type: text/css; charset=utf-8"), "css content-type");
    assert!(!body_of(&css, "200").is_empty(), "the stylesheet must not be empty");

    // Unknown path -> 404.
    let nf = uds_get(&sock, "/nope").await;
    body_of(&nf, "404");

    let _ = std::fs::remove_file(&sock);
}

/// XSS regression (Principle 18): entity/type names come from untrusted observe calls and are
/// interpolated into the console's innerHTML/attributes. Assert against the viewer SOURCE (not the
/// served bytes, which are minified in release); ESLint no-unsanitized guards the same source in CI.
#[test]
fn viz_source_escapes_untrusted_names() {
    let js = include_str!("../assets/viewer.js");

    // esc() must escape quotes too, not just <&> - otherwise a name breaks out of a title="..."
    // attribute into an event handler (attribute-injection XSS).
    assert!(
        js.contains(r#"replace(/[<&>"']/g"#),
        "esc() must escape quotes for the attribute-injection defense"
    );
    assert!(
        js.contains("&quot;") && js.contains("&#39;"),
        "esc() map must translate double and single quotes"
    );
    // The node hover tooltip must route the name/type through esc(), never raw interpolation.
    assert!(
        js.contains("<b>${esc(n.name)}</b>"),
        "showTip must escape the node name (stored-XSS vector)"
    );
    assert!(
        !js.contains("<b>${n.name}</b>"),
        "showTip must not interpolate the raw node name into innerHTML"
    );
}

/// The socket file IS the access control (F19): bind_uds must chmod it 0600, refuse to steal a
/// live socket, and replace a stale one. The browser-facing gates (Host / CSRF) are gone with the
/// TCP listener - /api/review is reachable over the socket with no special headers.
#[tokio::test]
async fn viz_socket_is_owner_only_and_review_needs_no_browser_headers() {
    use std::os::unix::fs::PermissionsExt;
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let sock = serve_uds("perm", engine, ev_channel()).await;

    // 0600: only the owning user may connect.
    let mode = std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "socket must be owner-only");

    // A second bind on a LIVE socket must fail loud (two instances), not steal the path.
    let second = supragnosis_viz::bind_uds(&sock).await;
    assert!(second.is_err(), "binding a live socket must be refused");
    assert!(
        second.unwrap_err().to_string().contains("another instance"),
        "the refusal must name the cause"
    );

    // The write endpoint routes with no browser-trust headers: the verdict reaches the engine
    // (append-only verdict observation; the fold decides) instead of a 403 transport gate.
    let r = uds_get(&sock, "/api/review?proposal=missing&decision=merge").await;
    assert!(
        body_of(&r, "200").contains("observation_id"),
        "the verdict must reach the gated engine path: {r}"
    );

    let _ = std::fs::remove_file(&sock);

    // A STALE socket file (nothing accepting) is replaced on the next bind.
    let stale = sock_path("stale");
    let _ = std::fs::remove_file(&stale);
    drop(supragnosis_viz::bind_uds(&stale).await.expect("first bind")); // listener dropped -> stale file remains
    let rebound = supragnosis_viz::bind_uds(&stale).await;
    assert!(rebound.is_ok(), "a stale socket file must be replaced: {rebound:?}");
    let _ = std::fs::remove_file(&stale);
}

#[tokio::test]
async fn viz_lists_workspaces_sorted_distinct() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "alpha").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    // Load knowledge into two workspaces (arrival order shuffled).
    for (ws, name) in [("gamma", "x"), ("alpha", "y"), ("gamma", "z")] {
        engine
            .observe(ObserveInput {
                content: format!("{name} in {ws}"),
                workspace: Some(ws.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { description: None,
                    name: name.into(),
                    kind: None,
                }],
                relations: vec![],
            })
            .unwrap();
    }
    let sock = serve_uds("ws", engine, ev_channel()).await;

    let list = json_get(&uds_get(&sock, "/api/workspaces").await);
    // Deduplicated + sorted (Principle 16).
    assert_eq!(list, serde_json::json!(["alpha", "gamma"]));
    let _ = std::fs::remove_file(&sock);
}

/// SSE: whether engine events stream to /api/events - attach a BroadcastSink to the engine, give
/// the same channel to serve, then verify connect -> emit -> receiving a data: frame.
#[tokio::test]
async fn viz_streams_mcp_events_via_sse() {
    let tx = broadcast::channel::<String>(16).0;
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws")
            .with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone()))),
    );
    let sock = serve_uds("sse", engine.clone(), tx.clone()).await;

    // After the SSE connect, read the header first (a signal the handler has finished subscribe - guarantees emit ordering).
    let mut s = UnixStream::connect(&sock).await.unwrap();
    s.write_all(b"GET /api/events HTTP/1.1\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 1024];
    let n = s.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.contains("text/event-stream"), "SSE content-type: {head}");

    // Now emit an event -> it must arrive as an SSE data: frame.
    engine.emit(Event::GetEntity {
        id: "abc".into(),
        name: Some("rmcp".into()),
        found: true,
    });
    let mut got = String::new();
    for _ in 0..5 {
        let n = s.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        got.push_str(&String::from_utf8_lossy(&buf[..n]));
        if got.contains("data:") {
            break;
        }
    }
    assert!(
        got.contains("data:")
            && got.contains("get_entity")
            && got.contains("rmcp")
            && got.contains("\"session\""),
        "an SSE event frame (including session) must arrive: {got}"
    );
    let _ = std::fs::remove_file(&sock);
}

/// `/api/hypergraph`: the set of entities co-asserted in one observation surfaces as a hyperedge
/// (Principle 11 second-order structure). Guards routing + serialization + engine wiring end-to-end.
#[tokio::test]
async fn viz_serves_hypergraph() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    // One observation co-asserts three entities -> a single hyperedge (size 3), no binary relations.
    engine
        .observe(ObserveInput {
            content: "supragnosis, rmcp, cozo were discussed together".into(),
            workspace: Some("ws".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { description: None, name: "supragnosis".into(), kind: Some("Project".into()) },
                EntityInput { description: None, name: "rmcp".into(), kind: Some("Tool".into()) },
                EntityInput { description: None, name: "cozo".into(), kind: Some("Tool".into()) },
            ],
            relations: vec![],
        })
        .unwrap();
    let sock = serve_uds("hyper", engine, ev_channel()).await;

    let hg = json_get(&uds_get(&sock, "/api/hypergraph?workspace=ws").await);
    assert_eq!(hg["stats"]["node_count"], 3, "hypergraph: {hg}");
    assert_eq!(hg["stats"]["hyperedge_count"], 1);
    assert_eq!(hg["stats"]["max_size"], 3);
    assert_eq!(hg["hyperedges"][0]["size"], 3);
    // Members are 3 sorted entity ids (deterministic - Principle 16).
    assert_eq!(hg["hyperedges"][0]["members"].as_array().unwrap().len(), 3);
    let _ = std::fs::remove_file(&sock);
}
