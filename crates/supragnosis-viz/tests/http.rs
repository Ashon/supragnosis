//! viz HTTP surface integration test. Assembles a deterministic Engine (InMemory + hashing
//! embedder) in-process and fires GETs via reqwest at a real listener bound to port 0
//! (following the in-process bring-up convention of crates/supragnosis-mcp/tests/mcp_surface.rs).

use std::sync::Arc;

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, EntityInput, Event, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// Event channel for tests - the broadcast Sender to pass to serve.
fn ev_channel() -> broadcast::Sender<String> {
    broadcast::channel::<String>(16).0
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

    // port 0 -> look up the actual OS-assigned port (deterministic/no collision).
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel(), None));

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // /api/graph?workspace=ws -> 2 nodes, 1 edge.
    let resp = client
        .get(format!("{base}/api/graph?workspace=ws"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let g: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(g["stats"]["node_count"], 2, "graph: {g}");
    assert_eq!(g["stats"]["edge_count"], 1);
    assert_eq!(g["edges"][0]["type"], "depends_on");

    // workspace unspecified -> the node's default ws ("ws") scope -> same 2 nodes.
    let g2: serde_json::Value = client
        .get(format!("{base}/api/graph"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(g2["stats"]["node_count"], 2);

    // '*' -> everything (None) -> same.
    let g3: serde_json::Value = client
        .get(format!("{base}/api/graph?workspace=*"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(g3["stats"]["node_count"], 2);

    // Index HTML - the canvas viewer, now linking the split-out assets.
    let idx = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(idx.status(), 200);
    assert_eq!(
        idx.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = idx.text().await.unwrap();
    assert!(html.contains("<canvas"), "the viewer HTML must contain a canvas");
    // Path substrings (not full attribute text) so the assertion survives release HTML minification,
    // which may drop the attribute quotes.
    assert!(html.contains("/viewer.css"), "the index must link the stylesheet asset");
    assert!(html.contains("/viewer.js"), "the index must link the script asset");

    // The split-out JS asset is served (same origin) with a JS content type and drives the API.
    let js = client.get(format!("{base}/viewer.js")).send().await.unwrap();
    assert_eq!(js.status(), 200);
    assert_eq!(js.headers()["content-type"], "text/javascript; charset=utf-8");
    let js_body = js.text().await.unwrap();
    assert!(js_body.contains("/api/graph"), "the script must poll the graph API");

    // The split-out CSS asset is served with a CSS content type.
    let css = client.get(format!("{base}/viewer.css")).send().await.unwrap();
    assert_eq!(css.status(), 200);
    assert_eq!(css.headers()["content-type"], "text/css; charset=utf-8");
    assert!(!css.text().await.unwrap().is_empty(), "the stylesheet must not be empty");

    // Unknown path -> 404.
    let nf = client.get(format!("{base}/nope")).send().await.unwrap();
    assert_eq!(nf.status(), 404);
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

/// Send a raw HTTP/1.1 GET with caller-controlled headers, return the full response text.
/// reqwest normalizes Host/Sec-Fetch-* headers, so the trust checks need a hand-built request.
async fn raw_get(addr: std::net::SocketAddr, path: &str, extra_headers: &str) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\n{extra_headers}Connection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).await.unwrap();
    resp
}

/// DNS-rebinding + CSRF defenses on the write path (Principle 17/23). A verdict mutates the canon,
/// so a foreign page must not reach it even over trusted loopback.
#[tokio::test]
async fn viz_write_path_rejects_rebinding_and_csrf() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel(), None));

    // DNS rebinding: loopback peer presents a foreign Host -> refuse every path, reads included.
    let rebind = raw_get(addr, "/api/graph", "Host: evil.example.com\r\n").await;
    assert!(rebind.starts_with("HTTP/1.1 403"), "rebound Host must be refused, got: {}", rebind.lines().next().unwrap_or(""));
    assert!(rebind.contains("DNS-rebinding"), "the refusal must name the defense");

    // A loopback Host reads fine.
    let ok = raw_get(addr, "/api/graph", "Host: 127.0.0.1\r\n").await;
    assert!(ok.starts_with("HTTP/1.1 200"), "a loopback Host must be served");

    // CSRF: a request lacking the viewer's custom header is refused - this is the <img>/<form>
    // vector (those cannot set custom headers), and it holds on every browser regardless of whether
    // Sec-Fetch-Site is sent. Loopback Host, no custom header -> 403.
    let no_header = raw_get(addr, "/api/review?proposal=x&decision=merge", "Host: 127.0.0.1\r\n").await;
    assert!(no_header.starts_with("HTTP/1.1 403"), "a verdict without the viewer header must be refused, got: {}", no_header.lines().next().unwrap_or(""));
    assert!(no_header.contains("CSRF defense"), "the refusal must name the defense");

    // Defense in depth: even if a cross-site request carried the header, an explicit cross-site
    // Sec-Fetch-Site is still refused.
    let cross = raw_get(
        addr,
        "/api/review?proposal=x&decision=merge",
        "Host: 127.0.0.1\r\nX-Supragnosis-Viz: 1\r\nSec-Fetch-Site: cross-site\r\n",
    )
    .await;
    assert!(cross.starts_with("HTTP/1.1 403"), "cross-site verdict must be refused, got: {}", cross.lines().next().unwrap_or(""));

    // The viewer's own request (custom header + same-origin) passes the CSRF gate.
    let same = raw_get(
        addr,
        "/api/review?proposal=x&decision=merge",
        "Host: 127.0.0.1\r\nX-Supragnosis-Viz: 1\r\nSec-Fetch-Site: same-origin\r\n",
    )
    .await;
    assert!(!same.contains("CSRF defense"), "the viewer's own verdict must not be CSRF-blocked");
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

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel(), None));

    let list: Vec<String> = reqwest::Client::new()
        .get(format!("http://{addr}/api/workspaces"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Deduplicated + sorted (Principle 16).
    assert_eq!(list, vec!["alpha", "gamma"]);
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, tx.clone(), None));

    // After the SSE connect, read the header first (a signal the handler has finished subscribe - guarantees emit ordering).
    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(b"GET /api/events HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = sock.read(&mut buf).await.unwrap();
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
        let n = sock.read(&mut buf).await.unwrap();
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
}

/// `/api/hypergraph`: the set of entities co-asserted in one observation surfaces as a hyperedge
/// (Principle 11 second-order structure). Guards routing + serialization + engine wiring end-to-end over HTTP.
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

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel(), None));

    let hg: serde_json::Value = reqwest::Client::new()
        .get(format!("http://{addr}/api/hypergraph?workspace=ws"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hg["stats"]["node_count"], 3, "hypergraph: {hg}");
    assert_eq!(hg["stats"]["hyperedge_count"], 1);
    assert_eq!(hg["stats"]["max_size"], 3);
    assert_eq!(hg["hyperedges"][0]["size"], 3);
    // Members are 3 sorted entity ids (deterministic - Principle 16).
    assert_eq!(hg["hyperedges"][0]["members"].as_array().unwrap().len(), 3);
}
