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
    tokio::spawn(supragnosis_viz::serve(engine, listener, events, None, None));
    path
}

/// Same, but with the injected narrowing handler - the surface a hub's console drives.
async fn serve_uds_with_narrow(
    name: &str,
    engine: Arc<Engine>,
    events: broadcast::Sender<String>,
    narrow: supragnosis_viz::NarrowShare,
) -> PathBuf {
    let path = sock_path(name);
    let _ = std::fs::remove_file(&path);
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    tokio::spawn(supragnosis_viz::serve(engine, listener, events, None, Some(narrow)));
    path
}

/// One raw HTTP/1.1 request with an explicit method, for the endpoint that is not a GET.
async fn uds_request(path: &PathBuf, method: &str, target: &str) -> String {
    let mut s = UnixStream::connect(path).await.expect("connect viewer socket");
    let req = format!("{method} {target} HTTP/1.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).await.unwrap();
    resp
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

/// The full contested-belief mediation loop over the socket (M3a, resolution.md Section 4.2):
/// a tier-tied kind conflict surfaces in /api/curation and on the graph node; one GET to
/// /api/resolve opens a claim_promotion and casts the Console merge verdict; the re-folded graph
/// shows the confirmed kind at human_confirmed, no longer contested - and the proposal trail is
/// visible in /api/proposals (everything is gated appended events, never a direct write).
#[tokio::test]
async fn viz_resolve_settles_a_contested_belief() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "ws"));
    // Two observations disagree on cozo's kind (both local default tier -> tier-tied).
    let observe_kind = |kind: &str| ObserveInput {
        content: format!("cozo is a {kind}"),
        workspace: None,
        source_ref: None,
        confidence: None,
        on_behalf_of: None,
        derived_from: vec![],
        entities: vec![EntityInput { description: None, name: "cozo".into(), kind: Some(kind.into()) }],
        relations: vec![],
    };
    let first = engine.observe(observe_kind("Tool")).unwrap().observation_id;
    engine.observe(observe_kind("Library")).unwrap();
    let sock = serve_uds("resolve", engine, ev_channel()).await;

    // Surfaced: the curation report lists the conflict; the graph node is contested.
    let cur = json_get(&uds_get(&sock, "/api/curation?workspace=ws").await);
    assert_eq!(cur["stats"]["contradictions"], 1, "curation: {cur}");
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    assert_eq!(g["nodes"][0]["contested"], true, "graph: {g}");

    // One console act settles it: promote the Tool-asserting observation to human_confirmed.
    let r = json_get(
        &uds_get(&sock, &format!("/api/resolve?observation={first}&tier=human_confirmed&workspace=ws")).await,
    );
    assert!(r["proposal_id"].is_string(), "resolve: {r}");
    assert!(r["verdict_observation_id"].is_string());

    // Re-folded: kind = Tool at human_confirmed, contested cleared, loser still queryable (R7).
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let n = &g["nodes"][0];
    assert_eq!(n["type"], "Tool", "graph after resolve: {g}");
    assert_eq!(n["trust_tier"], "human_confirmed");
    assert!(n.get("contested").is_none() || n["contested"] == false);
    assert_eq!(n["competitors"][0]["value"], "Library");
    // The trail: a merged claim_promotion proposal exists.
    let props = json_get(&uds_get(&sock, "/api/proposals?workspace=ws").await);
    assert_eq!(props[0]["kind"], "claim_promotion", "proposals: {props}");
    assert_eq!(props[0]["state"], "merged");
    assert_eq!(props[0]["tier"], "human_confirmed");
    let _ = std::fs::remove_file(&sock);
}

/// The reify promotion path over the socket (Principle 11): a recurring co-occurrence context is
/// asserted as a group entity + member_of relations through /api/reify - an ordinary observation
/// carrying derived_from lineage - and the graph gains the first-class structure while the
/// hyperedge stays a derived view.
#[tokio::test]
async fn viz_reify_promotes_a_hyperedge_into_the_graph() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "ws"));
    for content in ["kernel loads driver", "driver runs in kernel space"] {
        engine
            .observe(ObserveInput {
                content: content.into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { description: None, name: "kernel".into(), kind: None },
                    EntityInput { description: None, name: "driver".into(), kind: None },
                ],
                relations: vec![],
            })
            .unwrap();
    }
    let sock = serve_uds("reify", engine, ev_channel()).await;

    let hg = json_get(&uds_get(&sock, "/api/hypergraph?workspace=ws").await);
    let hid = hg["hyperedges"][0]["id"].as_str().expect("hyperedge id").to_string();
    assert_eq!(hg["hyperedges"][0]["sources"], 2);

    let r = json_get(
        &uds_get(&sock, &format!("/api/reify?hyperedge={hid}&name=boot%20stack&workspace=ws")).await,
    );
    assert!(r["observation_id"].is_string(), "reify: {r}");

    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let group = g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "boot stack")
        .expect("group node");
    assert_eq!(group["type"], "Context");
    let member_edges = g["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "member_of")
        .count();
    assert_eq!(member_edges, 2, "graph: {g}");
    // An unknown id is a 400 with a self-correcting message, not a silent no-op (P21/P5).
    let resp = uds_get(&sock, "/api/reify?hyperedge=nope&workspace=ws").await;
    assert!(resp.lines().next().unwrap_or("").contains("400"), "got: {resp}");
    let _ = std::fs::remove_file(&sock);
}

/// The observation-log surface (source of truth, Principle 1) and the "why" explain surface: the log
/// is newest-first with provenance, `entity=` narrows to a node's evidence set, and explain reports
/// the per-field decision (winner/competitor) consistent with the graph. 400/404 on bad input (P5).
#[tokio::test]
async fn viz_serves_observation_log_and_explain() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    // observe_depends: supragnosis(Project) --depends_on--> rmcp(Tool). Then a conflicting kind for
    // supragnosis (System) - two distinct kinds tie at the top tier -> contested.
    observe_depends(&engine);
    engine
        .observe(ObserveInput {
            content: "supragnosis is a system".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { description: None, name: "supragnosis".into(), kind: Some("System".into()) }],
            relations: vec![],
        })
        .expect("observe 2");
    let sock = serve_uds("obslog", engine, ev_channel()).await;

    // The full log, newest-first, with provenance attached.
    let log = json_get(&uds_get(&sock, "/api/observations?workspace=ws").await);
    let arr = log.as_array().expect("log is an array");
    assert_eq!(arr.len(), 2, "two observations: {log}");
    assert!(arr[0]["attestations"][0]["host"].is_string(), "attestation host present: {log}");
    let (w0, w1) = (arr[0]["hlc"]["wall"].as_u64().unwrap(), arr[1]["hlc"]["wall"].as_u64().unwrap());
    assert!(w0 >= w1, "newest-first by hlc wall");

    // Resolve node ids from the graph, then the entity filter narrows the log.
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let node_id = |name: &str| {
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == name)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let sid = node_id("supragnosis");
    let rid = node_id("rmcp");
    let sfilt = json_get(&uds_get(&sock, &format!("/api/observations?workspace=ws&entity={sid}")).await);
    assert_eq!(sfilt.as_array().unwrap().len(), 2, "both observations touch supragnosis");
    let rfilt = json_get(&uds_get(&sock, &format!("/api/observations?workspace=ws&entity={rid}")).await);
    assert_eq!(rfilt.as_array().unwrap().len(), 1, "only the depends_on obs touches rmcp");

    // Explain: the per-field decision; supragnosis kind is contested (Project vs System).
    let ex = json_get(&uds_get(&sock, &format!("/api/explain?entity={sid}")).await);
    assert_eq!(ex["id"], sid);
    let kind_field =
        ex["fields"].as_array().unwrap().iter().find(|f| f["field"] == "kind").expect("kind field");
    assert_eq!(kind_field["contested"], true, "two kinds tie -> contested: {ex}");
    let roles: Vec<&str> = kind_field["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["role"].as_str().unwrap())
        .collect();
    assert!(roles.contains(&"winner") && roles.contains(&"competitor"), "winner + competitor: {ex}");
    assert_eq!(ex["supporting"].as_array().unwrap().len(), 2, "both obs support supragnosis");

    // Bad input: missing entity -> 400; an id that resolves to nothing -> 404 (P5).
    body_of(&uds_get(&sock, "/api/explain").await, "400");
    body_of(&uds_get(&sock, "/api/explain?entity=deadbeef").await, "404");

    let _ = std::fs::remove_file(&sock);
}

/// Principle 17, the OUTER of the two layers `bind_uds` establishes. Its sibling
/// `viz_socket_is_owner_only_and_review_needs_no_browser_headers` covers the socket's own 0600 mode;
/// this covers the directory, which the code creates 0700 as defense in depth so a foreign local
/// user is denied before the socket mode is ever consulted. That second layer was documented in
/// architecture.md Section 10 and in the bind comment, and asserted nowhere - and it is load-bearing
/// precisely because the viewer deleted its Host/CSRF gates when it left TCP, leaving file
/// permissions as the entire access control.
#[tokio::test]
async fn p17_socket_directory_denies_foreign_users_before_the_socket_mode() {
    use std::os::unix::fs::PermissionsExt;

    // A nested directory, so the parent this creates is one bind_uds made rather than the shared
    // temp dir (whose mode is the OS's business, not ours).
    let dir = std::env::temp_dir().join(format!("supra-viz-p17-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("viz.sock");

    let engine = Arc::new(Engine::new(
        Arc::new(InMemoryStore::new()),
        "h",
        "ws",
    ));
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    tokio::spawn(supragnosis_viz::serve(engine, listener, ev_channel(), None, None));

    let dir_mode = std::fs::metadata(&dir).expect("dir exists").permissions().mode() & 0o777;
    assert_eq!(
        dir_mode, 0o700,
        "the parent directory must deny a foreign user before the socket mode is consulted"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Narrowing a peer's shared workspaces: a POST, refused as a GET, and absent when there is no
/// federation role to narrow.
///
/// The endpoint is deliberately the one exception to this surface's GET-with-side-effect convention.
/// That convention is defensible - a 0600 unix socket has no third-party origin to ride - and it
/// still governs the verdict endpoints. But this act changes a sharing boundary (P17) rather than
/// appending a gated verdict, and the method is the cheapest place to record the difference.
#[tokio::test]
async fn narrowing_a_peer_is_a_post_and_needs_a_federation_role() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));

    // No handler injected = no server role on this node.
    let plain = serve_uds("narrow-none", engine.clone(), ev_channel()).await;
    let resp = uds_request(&plain, "POST", "/api/peer/share?node_id=x&workspaces=").await;
    assert!(resp.lines().next().unwrap_or("").contains("404"), "{resp}");

    /// What the injected handler was asked to do, so a refused request can be shown never to reach it.
    type Calls = Arc<std::sync::Mutex<Vec<(String, Vec<String>)>>>;
    let seen: Calls = Arc::default();
    let recorded = seen.clone();
    let narrow: supragnosis_viz::NarrowShare = Arc::new(move |node_id: &str, keep: &[String]| {
        if node_id == "unknown" {
            return Err("no allowlist entry for node_id unknown".into());
        }
        recorded.lock().unwrap().push((node_id.to_string(), keep.to_vec()));
        Ok(keep.to_vec())
    });
    let path = serve_uds_with_narrow("narrow-ok", engine, ev_channel(), narrow).await;

    // A GET is refused, so the convention cannot be extended here by habit.
    let resp = uds_request(&path, "GET", "/api/peer/share?node_id=a&workspaces=alpha").await;
    assert!(resp.lines().next().unwrap_or("").contains("405"), "{resp}");
    assert!(seen.lock().unwrap().is_empty(), "a refused method must not reach the handler");

    // A POST narrows and echoes what is now granted.
    let resp = uds_request(&path, "POST", "/api/peer/share?node_id=a&workspaces=alpha,beta").await;
    let v = json_get(&resp);
    assert_eq!(v["node_id"], "a");
    assert_eq!(v["shared_workspaces"], serde_json::json!(["alpha", "beta"]));

    // Empty is a narrowing to nothing - distinct from omitting the parameter, which is a 400.
    let resp = uds_request(&path, "POST", "/api/peer/share?node_id=a&workspaces=").await;
    let v = json_get(&resp);
    assert_eq!(v["shared_workspaces"], serde_json::json!([]));
    let resp = uds_request(&path, "POST", "/api/peer/share?node_id=a").await;
    assert!(resp.lines().next().unwrap_or("").contains("400"), "{resp}");

    // The handler's refusal reaches the caller as the reason, not as a bare failure.
    let resp = uds_request(&path, "POST", "/api/peer/share?node_id=unknown&workspaces=").await;
    let body = body_of(&resp, "400 Bad Request");
    assert!(body.contains("no allowlist entry"), "{body}");

    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 2, "only the two accepted POSTs reached the handler: {calls:?}");
}
