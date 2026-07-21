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
                EntityInput {
                    name: "supragnosis".into(),
                    kind: Some("Project".into()),
                },
                EntityInput {
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                },
            ],
            relations: vec![RelationInput {
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
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

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

    // Index HTML - the canvas viewer.
    let idx = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(idx.status(), 200);
    assert_eq!(
        idx.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = idx.text().await.unwrap();
    assert!(html.contains("<canvas"), "the viewer HTML must contain a canvas");
    assert!(html.contains("/api/graph"), "the viewer must poll the graph API");

    // Unknown path -> 404.
    let nf = client.get(format!("{base}/nope")).send().await.unwrap();
    assert_eq!(nf.status(), 404);
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
                entities: vec![EntityInput {
                    name: name.into(),
                    kind: None,
                }],
                relations: vec![],
            })
            .unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

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
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, tx.clone()));

    // After the SSE connect, read the header first (a signal the handler has finished subscribe - guarantees emit ordering).
    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(b"GET /api/events HTTP/1.1\r\nHost: x\r\n\r\n")
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
                EntityInput { name: "supragnosis".into(), kind: Some("Project".into()) },
                EntityInput { name: "rmcp".into(), kind: Some("Tool".into()) },
                EntityInput { name: "cozo".into(), kind: Some("Tool".into()) },
            ],
            relations: vec![],
        })
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

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
