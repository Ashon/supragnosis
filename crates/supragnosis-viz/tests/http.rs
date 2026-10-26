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
async fn serve_uds(name: &str, engine: Arc<Engine>, events: broadcast::Sender<String>) -> PathBuf {
    let path = sock_path(name);
    let _ = std::fs::remove_file(&path); // leftover from a previous run of this same test
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    tokio::spawn(supragnosis_viz::serve(engine, listener, events, None, None, None));
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
    tokio::spawn(supragnosis_viz::serve(engine, listener, events, None, Some(narrow), None));
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
    assert!(status.contains(want_status), "expected {want_status}, got: {status}");
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
                EntityInput {
                    description: None,
                    name: "supragnosis".into(),
                    kind: Some("Project".into()),
                },
                EntityInput { description: None, name: "rmcp".into(), kind: Some("Tool".into()) },
            ],
            relations: vec![RelationInput {
                description: None,
                from: "supragnosis".into(),
                kind: "depends_on".into(),
                to: "rmcp".into(),
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe succeeds");
}

/// The local console says which node it is (design-system.md Sections 5 and 9): the page keys the
/// layout it remembers by this id, and calls a host "this node" only when the id matches - every
/// unconfigured node's host id is `localhost`, so the name alone would claim other machines' hosts.
#[tokio::test]
async fn the_local_surface_names_its_node() {
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
    let sock = serve_uds("surface", engine, ev_channel()).await;
    let s = json_get(&uds_get(&sock, "/api/surface").await);
    assert_eq!(s["surface"], "local", "surface: {s}");
    assert_eq!(s["node"], "h", "the node's host id: {s}");
    // The workspace an omitted one resolves to, so the page calls the default by its name
    // (design-system.md Section 29.10).
    assert_eq!(s["default_workspace"], "ws", "the node's default: {s}");
    let _ = std::fs::remove_file(&sock);
}

/// Each hit of the viewer's search says how it was reached (design-system.md Section 8, U15): its
/// words matched, it is only near in meaning, or the graph's enrichment added it as a neighbor. The
/// groups are read from the lists the search consumed, so a hit that contains the query is never
/// filed under "related", and a neighbor that does not contain it is never filed under "contains".
#[tokio::test]
async fn every_viewer_search_hit_says_how_it_was_reached() {
    // Keyword mode reaches rmcp only through the relation; with an embedder, the semantic surface
    // finds it first, by its nearness to the query.
    for (embed, rmcp_reached) in [(false, "neighbor"), (true, "meaning")] {
        let store = Arc::new(InMemoryStore::new());
        let mut engine = Engine::new(store, "h", "ws");
        if embed {
            engine = engine.with_embedder(Arc::new(HashingEmbedder::default()));
        }
        let engine = Arc::new(engine);
        observe_depends(&engine);
        let sock =
            serve_uds(if embed { "search-meaning" } else { "search-words" }, engine, ev_channel())
                .await;
        let r = json_get(&uds_get(&sock, "/api/search?q=supragnosis&workspace=ws").await);
        let hits = r["hits"].as_array().expect("hits");
        let reached = |name: &str| {
            hits.iter()
                .find(|h| h["kind"] == "entity" && h["snippet"] == name)
                .map(|h| h["reached"].as_str().unwrap_or("").to_string())
        };
        assert_eq!(reached("supragnosis").as_deref(), Some("words"), "the word matched: {r}");
        assert_eq!(reached("rmcp").as_deref(), Some(rmcp_reached), "how rmcp was reached: {r}");
        for h in hits {
            let how = h["reached"].as_str().unwrap_or("");
            assert!(["words", "meaning", "neighbor"].contains(&how), "every hit says how: {h}");
            if h["snippet"].as_str().is_some_and(|s| s.to_lowercase().contains("supragnosis")) {
                assert_eq!(how, "words", "a hit holding the word is a word match: {h}");
            }
        }
        let _ = std::fs::remove_file(&sock);
    }
}

/// design-system.md U6 (Principles 5, 19, 21; inspector.md D5): the viewer's search answers with the
/// engine's ranking and mode label, and an entity hit is the entity the graph draws - an id an accepted
/// merge folded away forwards to its canonical one, so the page never re-resolves identity. A missing
/// query is refused, and an empty answer says absence is unknown.
#[tokio::test]
async fn the_viewer_search_answers_with_canonical_hits() {
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
    observe_depends(&engine);
    engine
        .observe(ObserveInput {
            content: "rmcp lib is the MCP client library".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                description: None,
                name: "rmcp lib".into(),
                kind: Some("Tool".into()),
            }],
            relations: vec![],
        })
        .expect("observe succeeds");
    let sock = serve_uds("search", engine.clone(), ev_channel()).await;
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let id_of = |name: &str| {
        g["nodes"].as_array().unwrap().iter().find(|n| n["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (lib, rmcp) = (id_of("rmcp lib"), id_of("rmcp"));
    let proposal = engine
        .propose(supragnosis_engine::ProposeInput {
            workspace: Some("ws".into()),
            kind: "entity_merge".into(),
            targets: vec![lib.clone(), rmcp.clone()],
            into: Some(rmcp.clone()),
            tier: None,
            rationale: Some("one library, two names".into()),
            affected_types: Vec::new(),
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose succeeds");
    engine
        .review_proposal(
            Some("ws".into()),
            proposal,
            "merge".into(),
            None,
            None,
            supragnosis_engine::VerdictSurface::Console,
        )
        .expect("the merge is accepted");

    let r = json_get(&uds_get(&sock, "/api/search?q=rmcp%20lib&workspace=ws").await);
    assert!(r["mode"].is_string(), "the mode label rides the answer: {r}");
    let ids: Vec<&str> = r["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["kind"] == "entity")
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&rmcp.as_str()),
        "the merged entity is found by its canonical id: {r}"
    );
    assert!(!ids.contains(&lib.as_str()), "a merged-away id never reaches the page: {r}");
    let mut seen = std::collections::HashSet::new();
    assert!(ids.iter().all(|id| seen.insert(*id)), "one hit per canonical entity: {r}");

    assert!(uds_get(&sock, "/api/search").await.starts_with("HTTP/1.1 400"));
    let _ = std::fs::remove_file(&sock);

    // Absence, on a keyword-only node: the semantic surface answers every query with its nearest
    // vectors, so an empty answer is the keyword surface's.
    let bare = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    observe_depends(&bare);
    let sock = serve_uds("search-bare", bare, ev_channel()).await;
    let none = json_get(&uds_get(&sock, "/api/search?q=zzqxwv&workspace=ws").await);
    assert_eq!(none["mode"], "keyword", "{none}");
    assert_eq!(none["hits"].as_array().map(Vec::len), Some(0), "{none}");
    assert!(
        none["note"].as_str().is_some_and(|n| n.contains("absence is unknown")),
        "{none}"
    );
    let _ = std::fs::remove_file(&sock);
}

/// design-system.md Section 6: the panels sit beside the map, so the camera frames, pans and zooms in
/// the map's window between them. v0.6.0 framed the whole canvas, as when the docks floated over it,
/// and a seven-node workspace zoomed to the cap ran under both panels.
#[test]
fn the_camera_works_in_the_map_window() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let body = |name: &str| {
        let start = js
            .find(&format!("\nfunction {name}("))
            .unwrap_or_else(|| panic!("viewer.js no longer defines {name}"));
        let rest = &js[start + 1..];
        rest[..rest.find("\n}\n").expect("the function closes")].to_string()
    };
    let fit = body("fitView");
    assert!(fit.contains("mapWindow()"), "fitView frames the map's window");
    assert!(
        !fit.contains("frameBox(box, 0, innerWidth, TOP_INSET, innerHeight - BOTTOM_INSET"),
        "fitView no longer frames the whole canvas on a wide window"
    );
    assert!(
        body("miniPan").contains("mapWindow()"),
        "the minimap pans to the window's centre"
    );
    assert!(
        body("zoomWindow").contains("mapWindow()"),
        "the zoom buttons zoom about the window"
    );
    assert!(
        js.contains(r#"getElementById("zin").onclick = e => { if (idle(e)) return; zoomWindow("#),
        "the zoom-in button uses the window's centre"
    );
}

/// The bar shows the engine's answer and makes none of its own (design-system.md Section 8): it asks
/// /api/search, the map lights the entity ids that answer returned, and no name is matched against
/// the typed text in the page - a second ranking would answer a person differently from the agent.
#[test]
fn the_search_bar_lights_only_what_the_engine_answered() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    assert!(js.contains(r#"fetch("/api/search?q=""#), "the bar asks the search path");
    let start = js.find("\nfunction activeSet(").expect("viewer.js no longer defines activeSet");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("activeSet has no closing brace")];
    assert!(
        body.contains("if (searchLit) return { ns: searchLit, es: new Set() };"),
        "the open search lights the answer's ids"
    );
    assert!(
        js.contains(r#"searchAns.hits.filter(h => h.kind === "entity").map(h => h.id)"#),
        "the lit ids are the answer's entity hits"
    );
}

/// First contact (docs/onboarding.md Section 5): the local socket serves what the daemon recorded,
/// and a viewer without the record says it keeps none rather than answering an empty list (P5).
#[tokio::test]
async fn first_contact_is_the_nodes_own() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    let path = sock_path("clients");
    let _ = std::fs::remove_file(&path);
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    let view: supragnosis_viz::ClientsView = Arc::new(
        || serde_json::json!({"clients": [{"name": "claude-code", "version": "2.1.0", "first": 1, "last": 2}]}),
    );
    tokio::spawn(supragnosis_viz::serve(
        engine.clone(),
        listener,
        ev_channel(),
        None,
        None,
        Some(view),
    ));
    let seen = json_get(&uds_get(&path, "/api/clients").await);
    assert_eq!(seen["clients"][0]["name"], "claude-code", "{seen}");

    let bare = serve_uds("clients-none", engine.clone(), ev_channel()).await;
    let none = json_get(&uds_get(&bare, "/api/clients").await);
    assert!(none["clients"].is_null() && none["note"].as_str().is_some(), "{none}");
}

#[tokio::test]
async fn viz_serves_graph_index_and_404() {
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
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

/// One raw GET with an `If-None-Match` header, as the viewer's poll sends it.
async fn uds_get_if_none_match(path: &PathBuf, target: &str, tag: &str) -> String {
    let mut s = UnixStream::connect(path).await.expect("connect viewer socket");
    let req = format!("GET {target} HTTP/1.1\r\nIf-None-Match: {tag}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).await.unwrap();
    resp
}

fn etag_of(resp: &str) -> String {
    resp.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix("ETag: "))
        .unwrap_or_else(|| panic!("no ETag in: {}", resp.lines().next().unwrap_or("")))
        .to_string()
}

/// design-system.md U33: a poll that finds nothing new costs the page nothing. The graph, the
/// hypergraph and the Log carry an ETag of their body; asked with it, an unchanged one answers 304 with no body,
/// and a changed one answers in full with a new tag.
#[tokio::test]
async fn a_poll_that_finds_nothing_new_costs_nothing() {
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
    observe_depends(&engine);
    let sock = serve_uds("etag", Arc::clone(&engine), ev_channel()).await;

    for target in [
        "/api/graph?workspace=ws",
        "/api/hypergraph?workspace=ws",
        "/api/observations?limit=201&workspace=ws",
    ] {
        let first = uds_get(&sock, target).await;
        body_of(&first, "200");
        let tag = etag_of(&first);
        assert!(tag.starts_with('"') && tag.ends_with('"'), "a strong ETag: {tag}");

        let same = uds_get_if_none_match(&sock, target, &tag).await;
        assert!(same.starts_with("HTTP/1.1 304 Not Modified"), "{target} unchanged: {same}");
        assert_eq!(etag_of(&same), tag, "a 304 names the tag it confirms");
        assert_eq!(same.split_once("\r\n\r\n").map(|(_, b)| b), Some(""), "a 304 has no body");
        assert!(
            uds_get_if_none_match(&sock, target, "*").await.starts_with("HTTP/1.1 304"),
            "* matches any body"
        );
        let other = uds_get_if_none_match(&sock, target, "\"stale\"").await;
        assert_eq!(etag_of(&other), tag, "another tag is answered in full");
        assert!(!body_of(&other, "200").is_empty());
    }

    let before = etag_of(&uds_get(&sock, "/api/graph?workspace=ws").await);
    engine
        .observe(ObserveInput {
            content: "supragnosis uses tokio".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                description: None,
                name: "tokio".into(),
                kind: Some("Tool".into()),
            }],
            relations: vec![RelationInput {
                description: None,
                from: "supragnosis".into(),
                kind: "uses".into(),
                to: "tokio".into(),
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe succeeds");
    let after = uds_get_if_none_match(&sock, "/api/graph?workspace=ws", &before).await;
    assert_eq!(
        json_get(&after)["stats"]["node_count"],
        3,
        "a changed graph is answered in full"
    );
    assert_ne!(etag_of(&after), before, "and with a new tag");

    let _ = std::fs::remove_file(&sock);
}

/// XSS regression (Principle 18): entity/type names come from untrusted observe calls and are
/// interpolated into the console's innerHTML/attributes. Assert against the viewer SOURCE (not the
/// served bytes, which are minified in release); ESLint no-unsanitized guards the same source in CI.
#[test]
fn viz_source_escapes_untrusted_names() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));

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
    // Markup is written only through the `html` tag, which sends every interpolation that is not
    // itself `html`-built markup through esc().
    assert!(
        js.contains("return esc(String(v));"),
        "the html tag must fall back to esc() for every value that is not markup"
    );
}

/// docs/inspector.md D1 (Principle 6): a contested belief is never silenced or folded. The silence
/// rule only ever REMOVES rows, so the way D1 breaks is not a deleted block but a contested one that
/// ends up behind one of the rule's conditions, or inside the fold that resolved conflicts get.
///
/// Asserted against the source, like the escaping guard above, and with the same honesty: this is
/// a tripwire on the shape, not a proof about every render. It pins three things - the contested
/// branch is the first decision `contestedBlock` makes and returns before any fold is built; the
/// function never consults the scope baseline; and `renderDetail` lists the block as an item of its
/// own, outside any condition.
#[test]
fn inspector_never_folds_a_contested_belief() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let body = |name: &str| -> &str {
        let start = js
            .find(&format!("\nfunction {name}("))
            .unwrap_or_else(|| panic!("viewer.js no longer defines {name}"));
        let rest = &js[start + 1..];
        let end = rest.find("\n}\n").unwrap_or_else(|| panic!("{name} has no closing brace"));
        &rest[..end]
    };

    let block = body("contestedBlock");
    let first_decision = block.find("if (").expect("contestedBlock makes no decision");
    assert!(
        block[first_decision..].starts_with("if (node.contested) return"),
        "contestedBlock must decide `contested` first and return before building anything else - \
         a contested belief must never reach the fold below it"
    );
    assert!(
        !block.contains("scopeBaseline"),
        "the silence rule must not reach the contested block (D1)"
    );
    let branch = &block[first_decision..];
    let branch = &branch[..branch.find(';').expect("the contested branch is one statement")];
    assert!(
        !branch.contains("folded"),
        "the contested branch must not render the folded form"
    );

    let detail = body("renderDetail");
    let call = detail
        .find("contestedBlock(node)")
        .expect("renderDetail must render the contested block");
    let line_start = detail[..call].rfind('\n').map_or(0, |i| i + 1);
    let line_end = detail[call..].find('\n').map_or(detail.len(), |i| call + i);
    assert_eq!(
        detail[line_start..line_end].trim(),
        "contestedBlock(node),",
        "contestedBlock(node) must be its own item of the panel, not a branch of a condition"
    );
}

/// guard (inspector.md D7): every HTML sink takes an `html` tagged template - which escapes what
/// it interpolates - or a plain literal with nothing in it, and no comment turns the lint off. The
/// `no-unsanitized` lint holds this in CI; this scan stands beside it, so a sink added where the
/// lint did not run still fails here. `innerHTML` is the only sink the viewer's scripts use.
#[test]
fn every_html_sink_takes_an_html_template() {
    /// A quoted literal with no interpolation, ended by the statement: `'...';` or `"...";`.
    fn plain_literal(value: &str) -> bool {
        let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') else {
            return false;
        };
        let body = &value[1..];
        let Some(end) = body.find(quote) else {
            return false;
        };
        !body[..end].contains('\\') && body[end + 1..].trim_start().starts_with(';')
    }
    for (file, js) in [
        ("viewer.js", include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"))),
        ("viewer-sim.js", include_str!("../assets/viewer-sim.js")),
        ("viewer-gl.js", include_str!("../assets/viewer-gl.js")),
    ] {
        assert!(!js.contains("eslint-disable"), "{file}: no comment turns the lint off");
        for sink in
            ["outerHTML", "insertAdjacentHTML", "document.write", "createContextualFragment"]
        {
            assert!(!js.contains(sink), "{file}: {sink} is a sink the viewer does not use");
        }
        let mut sinks = 0;
        for (at, _) in js.match_indices(".innerHTML") {
            let rest = js[at + ".innerHTML".len()..].trim_start();
            let Some(value) = rest.strip_prefix('=').filter(|v| !v.starts_with('=')) else {
                continue; // read, not written
            };
            let value = value.trim_start();
            sinks += 1;
            let line = js[at..].lines().next().unwrap_or_default();
            assert!(
                value.starts_with("html`") || plain_literal(value),
                "{file}: an innerHTML sink takes an html template or a plain literal: {line}"
            );
        }
        if file == "viewer.js" {
            assert!(sinks > 20, "the scan found the viewer's sinks ({sinks})");
        }
    }
}

/// guard (inspector.md D3, Section 5): silence is declared. In `renderDetail`, each field the
/// silence rule hides is said for the scope instead: the tier's text and the origin row show only
/// when the scope is not uniform, and when it is, the value goes into `scopeSaid`, which the scope
/// line prints whenever it holds anything. The workspace row hides only when the scope has one
/// workspace, which the workspace chips name.
#[test]
fn a_silenced_field_is_said_for_the_scope() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let start = js
        .find("\nfunction renderDetail(")
        .expect("viewer.js no longer defines renderDetail");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("renderDetail has no closing brace")];
    for (silenced, said) in [
        (
            "base.tier === null ? html`<span>${tierName(tier)}</span>`",
            "if (base.tier !== null) scopeSaid.push(tierName(base.tier));",
        ),
        (
            "const origin = base.origin === null && node.origins",
            "if (base.origin) scopeSaid.push(",
        ),
    ] {
        assert!(
            body.contains(silenced),
            "the field shows only when the scope differs: {silenced}"
        );
        assert!(body.contains(said), "and is said for the scope when it does not: {said}");
    }
    assert!(
        body.contains("scopeSaid.length ? html`<span class=\"scopeline\">All here: ${scopeSaid.join(\", \")}</span>`"),
        "the scope line prints whatever was silenced"
    );
    assert!(body.contains("const workspace = base.workspace === null"), "the workspace row");
}

/// guard (inspector.md D4): the silence rule's baseline is the loaded scope, never the
/// legend-filtered subset. A source tripwire: `nodes` is set in one place, from the /api/graph
/// response mapped whole, and `scopeBaseline` loops once, over `nodes`, naming none of the legend's
/// filters, the working set, or a helper that applies them - so hiding a type cannot make a row
/// appear on an unrelated node.
#[test]
fn the_silence_baseline_reads_the_whole_response() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let start = js
        .find("\nfunction scopeBaseline(")
        .expect("viewer.js no longer defines scopeBaseline");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("scopeBaseline has no closing brace")];
    assert_eq!(body.matches("for (").count(), 1, "scopeBaseline makes one pass:\n{body}");
    assert!(
        body.contains("for (const n of nodes)"),
        "scopeBaseline must read every loaded node:\n{body}"
    );
    for filtered in [
        "typeOff",
        "trustOff",
        "edgeTypeOff",
        "workSet",
        "listRows",
        "keptDrawn",
        ".filter(",
    ] {
        assert!(!body.contains(filtered), "scopeBaseline must not read {filtered}:\n{body}");
    }

    let sets: Vec<&str> = js
        .match_indices("nodes = ")
        .filter(|(i, _)| {
            let before = js[..*i].chars().next_back();
            !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        })
        .map(|(i, _)| js[i..].lines().next().unwrap())
        .collect();
    assert_eq!(
        sets,
        [
            "nodes = [], edges = [], typeColor = {}, edgeKinds = [];",
            "nodes = g.nodes.map(n => {"
        ],
        "`nodes` is declared empty and set only from the graph response, every node of it"
    );
}

/// design-system.md U2 (Principle 18): the map draws trust from the effective tier the server
/// computed, never anything else, and says what it reads. A tripwire on the shape: the ring reads
/// `n.trust_tier` and the scope's uniformity, the uniformity is taken over every loaded node (never the
/// legend-filtered subset, inspector.md D4).
#[test]
fn the_map_rings_read_the_effective_tier() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let body = |name: &str| -> &str {
        let start = js
            .find(&format!("\nfunction {name}("))
            .unwrap_or_else(|| panic!("viewer.js no longer defines {name}"));
        let rest = &js[start + 1..];
        &rest[..rest.find("\n}\n").unwrap_or_else(|| panic!("{name} has no closing brace"))]
    };
    let ring = body("trustRing");
    assert!(ring.contains("n.trust_tier"), "the ring must read the node's effective tier");
    assert!(ring.contains("trustUniform"), "a scope of one tier draws no ring (U2)");
    let note = body("noteTrustAndContest");
    assert!(
        note.contains("nodes.map(n => n.trust_tier)") && !note.contains("typeOff"),
        "uniformity is decided over every loaded node, never the legend-filtered subset"
    );
}

/// design-system.md U3 (Principle 6): a contested node is marked wherever it is drawn, and counted
/// whatever hides it. The badge pass runs after the nodes at full strength, at any size on the
/// screen, over the frame's drawn nodes - every node of a shown type the window holds, the window
/// grown by the badge (design-system.md Section 27.7) - and the status bar's count is taken over
/// every loaded node.
#[test]
fn a_contested_node_is_marked_and_counted_whatever_hides_it() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let pass = "ctx.globalAlpha = 1; bulk.globalAlpha = 1;\n  for (const n of vis) {\n    if (!n.contested) continue;";
    assert!(
        js.contains(pass),
        "the contested badge is drawn in its own full-strength pass, skipping only undrawn types"
    );
    assert!(
        js.contains("for (const k of few || nodesIn(vx0, vy0, vx1, vy1)) {\n    const n = nodes[k];\n    if (typeOff.has(n.type) || (few && (n.x < vx0 || n.x > vx1 || n.y < vy0 || n.y > vy1))) continue;")
            && js.contains("const reachOut = NODE_R_MAX + 16 + 9 / cam.s;"),
        "a node counts as drawn when its type is shown and it, its glow or its badge meets the window"
    );
    let count = js
        .lines()
        .find(|l| l.contains("contestedCount = ") && !l.contains("let "))
        .expect("viewer.js no longer counts contested nodes");
    assert!(
        count.contains("nodes.reduce") && !count.contains("typeOff"),
        "the contested count is over every loaded node, whatever the legend hides: {count}"
    );
}

/// design-system.md U8 (Principles 5 and 24): a status pill's absence means one thing - a known
/// zero, or a feature this node does not have set up. The contested pill hides only at a count of
/// zero, or on the local console, where Review's entry carries the count (U20); the sync pill hides
/// only off the local console (a read tier has no peers view) or where sync is not configured, and
/// draws "unavailable" rather than vanishing when the daemon does not answer.
#[test]
fn a_pill_is_absent_only_for_a_known_zero_or_nothing_set_up() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let start = js
        .find("\nasync function refreshSyncPill(")
        .expect("viewer.js no longer defines refreshSyncPill");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("refreshSyncPill has no closing brace")];
    let hides = body.matches("pill.hidden = true").count();
    assert_eq!(hides, 2, "the sync pill hides in exactly two places");
    assert!(
        body.contains("else if (f.configured === false) { pill.hidden = true; return; }"),
        "it hides where sync is not set up"
    );
    assert!(
        body.contains(r#"text = "Sync unavailable""#),
        "a daemon that does not answer is a state the pill draws"
    );
}

/// design-system.md U4 (Principle 23): on the Review page a decision acts on one item, and only once
/// that item's diff and checks are on screen. A tripwire on the shape: reviewDecide returns unless the
/// view it holds is the item's, refuses a merge a blocking check fails and asks a recall twice; and every
/// open or blocked proposal is listed.
#[test]
fn a_review_decision_takes_one_item_with_its_diff_on_screen() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let start = js
        .find("\nasync function reviewDecide(")
        .expect("viewer.js no longer defines reviewDecide");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("reviewDecide has no closing brace")];
    for (needle, what) in [
        (
            "if (!reviewView || reviewView.id !== p.id) return;",
            "the diff and checks on screen are the item's",
        ),
        (
            "reviewView.checks.some(c => c.blocking && !c.passed)",
            "a merge a blocking check fails is refused",
        ),
        (r#"p.kind === "recall" && recallArmed !== p.id"#, "a recall asks twice"),
    ] {
        assert!(body.contains(needle), "reviewDecide: {what}");
    }
    assert!(
        js.contains(r#"proposals.filter(p => p.state === "open" || p.state === "blocked")"#),
        "every open or blocked proposal is listed, whatever its diff"
    );
}

/// design-system.md U1 (Principle 24): every console write reports what happened. The writes used to
/// be bare fetches that dropped the server's answer, so a refusal - the hub's read tier answering 403,
/// a merge the fold would block answering 400 with its reason - looked exactly like a success that had
/// not arrived yet. A tripwire on the shape, like the two guards above. The console's writes - its
/// verdicts and the acts that write - each appear in the viewer's source only as the first argument
/// of `consoleWrite`, which reads the status and shows a notice on its refusal, unreachable and
/// success branches. The node's own operations (the settings dialog's peer narrowing among them)
/// keep their own error display.
#[test]
fn every_console_write_reports_its_outcome() {
    let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
    let writes = [
        "/api/review",
        "/api/resolve",
        "/api/reify",
        "/api/propose_merge",
        "/api/propose_split",
    ];
    for path in writes {
        let quoted = format!("\"{path}\"");
        let total = js.matches(&quoted).count();
        let reported = js.matches(&format!("consoleWrite({quoted}")).count();
        assert!(total > 0, "{path} is a console write, but viewer.js never calls it");
        assert_eq!(
            total, reported,
            "{path} must be written through consoleWrite, which reports the outcome - a bare fetch \
             drops the server's answer"
        );
    }
    let start = js
        .find("\nasync function consoleWrite(")
        .expect("viewer.js no longer defines consoleWrite");
    let rest = &js[start + 1..];
    let body = &rest[..rest.find("\n}\n").expect("consoleWrite has no closing brace")];
    assert!(body.contains("!r.ok"), "consoleWrite must read the response status");
    assert!(
        body.matches("toast(").count() >= 3,
        "consoleWrite must show a notice for success, refusal and an unreachable server"
    );
}

/// Every response carries a Content-Security-Policy, and the script half of it stays strict.
///
/// Escaping is the first defence and it is guarded by the test above; this is the one that survives
/// a missed `esc()`. The assertions are deliberately split: that the header EXISTS on both the page
/// and the API responses, and that `script-src` names neither `'unsafe-inline'` nor `'unsafe-eval'` -
/// because the way a CSP dies is not deletion, it is someone widening one directive to make a
/// feature work and leaving a header that still looks like a policy.
///
/// `style-src` is exempt from that second assertion on purpose: the viewer generates six
/// `style="background:<color>"` attributes, so it needs `'unsafe-inline'` there. What that buys an
/// attacker is CSS injection, whose exfiltration route is a URL fetch that `img-src 'self' data:`
/// and `default-src 'none'` refuse - and it costs nothing on the script side, which is the class
/// that turns a stored name into code execution.
#[tokio::test]
async fn every_response_carries_a_script_strict_csp() {
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
    let sock = serve_uds("csp", engine, ev_channel()).await;

    // The page, the script it loads, and a JSON API response: the header is unconditional, so a new
    // endpoint cannot forget it by being new.
    for target in ["/", "/viewer.js", "/api/graph", "/api/curation"] {
        let resp = uds_get(&sock, target).await;
        let head = resp.split("\r\n\r\n").next().unwrap_or_default().to_lowercase();
        let csp = head
            .lines()
            .find_map(|l| l.strip_prefix("content-security-policy:"))
            .unwrap_or_else(|| panic!("{target} carries no Content-Security-Policy header"))
            .trim()
            .to_string();

        assert!(
            csp.contains("default-src 'none'"),
            "{target}: the policy must deny by default, got: {csp}"
        );
        assert!(
            head.contains("x-content-type-options: nosniff"),
            "{target} must refuse content-type sniffing"
        );

        // Isolate script-src: 'unsafe-inline' elsewhere (style-src) is a deliberate concession and
        // must not make this assertion pass or fail.
        let script_src = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("script-src"))
            .unwrap_or_else(|| panic!("{target}: the policy names no script-src, got: {csp}"))
            .to_string();
        assert!(
            !script_src.contains("unsafe-inline") && !script_src.contains("unsafe-eval"),
            "{target}: script-src must stay strict - an injected name that reaches innerHTML is only \
             inert while it is. Got: {script_src}"
        );
    }
}

/// The socket file IS the access control (F19): bind_uds must chmod it 0600, refuse to steal a
/// live socket, and replace a stale one. The browser-facing gates (Host / CSRF) are gone with the
/// TCP listener - /api/review is reachable over the socket with no special headers.
#[tokio::test]
async fn viz_socket_is_owner_only_and_review_needs_no_browser_headers() {
    use std::os::unix::fs::PermissionsExt;
    let store = Arc::new(InMemoryStore::new());
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
    observe_depends(&engine);
    let proposal = engine
        .propose(supragnosis_engine::ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![
                supragnosis_core::Entity::make_id("ws", "rmcp"),
                supragnosis_core::Entity::make_id("ws", "supragnosis"),
            ],
            into: Some(supragnosis_core::Entity::make_id("ws", "supragnosis")),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
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
    let r = uds_get(&sock, &format!("/api/review?proposal={proposal}&decision=reject")).await;
    assert!(
        body_of(&r, "200").contains("observation_id"),
        "the verdict must reach the gated engine path: {r}"
    );
    // An id that names no proposal is the engine's refusal, with its reason, and not a transport
    // gate (design-system.md Section 29.15).
    let r = uds_get(&sock, "/api/review?proposal=missing&decision=merge").await;
    assert!(body_of(&r, "400").contains("no proposal missing"), "{r}");

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
                entities: vec![EntityInput { description: None, name: name.into(), kind: None }],
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

/// The store's most used types, names only, by count then name and at most sixteen, counted in every
/// workspace whatever the page's scope (design-system.md Section 29.9, D4): what a page gives the
/// type band's slots, once, so a type keeps one color in every view.
#[tokio::test]
async fn the_store_names_its_most_used_types_whatever_the_scope() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "alpha").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let observe = |ws: &str, entities: Vec<(String, String)>| {
        engine
            .observe(ObserveInput {
                content: format!("{} in {ws}", entities.len()),
                workspace: Some(ws.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: entities
                    .into_iter()
                    .map(|(name, kind)| EntityInput { description: None, name, kind: Some(kind) })
                    .collect(),
                relations: vec![],
            })
            .unwrap();
    };
    // Service is used three times across two workspaces, Tool twice, and eighteen types once.
    observe("a", vec![("s1".into(), "Service".into()), ("t1".into(), "Tool".into())]);
    observe("b", vec![("s2".into(), "Service".into()), ("s3".into(), "Service".into())]);
    observe("b", vec![("t2".into(), "Tool".into())]);
    observe("a", (0..18).map(|i| (format!("o{i}"), format!("Kind{i:02}"))).collect());
    let sock = serve_uds("common", engine, ev_channel()).await;

    let body = json_get(&uds_get(&sock, "/api/common_types?workspace=a").await);
    let types = body["types"].as_array().expect("a list of names");
    assert_eq!(body.as_object().unwrap().len(), 1, "names only, no counts: {body}");
    assert_eq!(types.len(), supragnosis_engine::COMMON_TYPES);
    assert_eq!(types[0], "Service", "the scope asked for does not narrow the count");
    assert_eq!(types[1], "Tool");
    assert_eq!(types[2], "Kind00", "a tie is broken by name");
    assert_eq!(types[15], "Kind13");
    let _ = std::fs::remove_file(&sock);
}

/// `/api/health` reports what the store owes and what the last open repaid (crash-recovery.md K5) -
/// the surface `supragnosis status` reads, so a recovery is visible after its startup log scrolled
/// away. A row appended through the log handle and never projected is owed until repaid.
#[tokio::test]
async fn health_reports_owed_projections_and_the_last_recovery() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "ws"));
    engine
        .store()
        .add_observation(supragnosis_core::Observation::new(
            "appended, never projected".into(),
            supragnosis_core::Provenance {
                host: "h".into(),
                on_behalf_of: None,
                workspace: "ws".into(),
                source_ref: None,
                observed_at: 1,
                confidence: None,
                trust_tier: Default::default(),
                sync: None,
            },
        ))
        .unwrap();
    let sock = serve_uds("health", engine.clone(), ev_channel()).await;

    let before = json_get(&uds_get(&sock, "/api/health").await);
    // `semantic` is null here: the test engine has no embedder (semantic-retrieval.md SR3).
    assert_eq!(
        before,
        serde_json::json!({"owed_projections": 1, "last_recovery": null, "semantic": null})
    );
    engine.repay_owed().unwrap();
    let after = json_get(&uds_get(&sock, "/api/health").await);
    assert_eq!(after["owed_projections"], 0);
    assert_eq!(after["last_recovery"]["workspaces"], serde_json::json!(["ws"]));
    assert_eq!(after["last_recovery"]["observations"], 1);
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
    engine.emit(Event::GetEntity { id: "abc".into(), name: Some("rmcp".into()), found: true });
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
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
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
                EntityInput {
                    description: None,
                    name: "supragnosis".into(),
                    kind: Some("Project".into()),
                },
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
        entities: vec![EntityInput {
            description: None,
            name: "cozo".into(),
            kind: Some(kind.into()),
        }],
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
        &uds_get(
            &sock,
            &format!("/api/resolve?observation={first}&tier=human_confirmed&workspace=ws"),
        )
        .await,
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

/// A confirmation made from the All view is filed in the observation's own workspace, and undoing it is
/// the demotion `kind=claim_demotion` casts (design-system.md Section 7, U18): from `workspace=*` the act
/// used to land in the node default, where the fold reading the observation's workspace never saw it.
#[tokio::test]
async fn viz_resolve_files_in_the_observations_workspace_and_can_be_undone() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "default"));
    let observe_kind = |kind: &str| ObserveInput {
        content: format!("cozo is a {kind}"),
        workspace: Some("billing".into()),
        source_ref: None,
        confidence: None,
        on_behalf_of: None,
        derived_from: vec![],
        entities: vec![EntityInput {
            description: None,
            name: "cozo".into(),
            kind: Some(kind.into()),
        }],
        relations: vec![],
    };
    let first = engine.observe(observe_kind("Tool")).unwrap().observation_id;
    engine.observe(observe_kind("Library")).unwrap();
    let sock = serve_uds("resolve-ws", engine, ev_channel()).await;

    let r = json_get(
        &uds_get(
            &sock,
            &format!("/api/resolve?observation={first}&tier=human_confirmed&workspace=*"),
        )
        .await,
    );
    assert!(r["proposal_id"].is_string(), "resolve: {r}");
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=billing").await);
    assert_eq!(g["nodes"][0]["type"], "Tool", "graph: {g}");
    assert_eq!(g["nodes"][0]["trust_tier"], "human_confirmed");
    let props = json_get(&uds_get(&sock, "/api/proposals?workspace=billing").await);
    assert_eq!(props.as_array().map(Vec::len), Some(1), "filed in billing: {props}");
    let none = json_get(&uds_get(&sock, "/api/proposals?workspace=default").await);
    assert_eq!(none.as_array().map(Vec::len), Some(0), "nothing filed in the default: {none}");

    // The explain read says what each value is without a person's verdict - what "Confirm instead"
    // would demote the confirmed one back to.
    let id = g["nodes"][0]["id"].as_str().expect("the node's id").to_string();
    let ex = json_get(&uds_get(&sock, &format!("/api/explain?entity={id}")).await);
    let kind = ex["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "kind")
        .expect("kind field");
    let tool = kind["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["value"] == "Tool")
        .expect("Tool row");
    assert_eq!(tool["trust_tier"], "human_confirmed", "explain: {ex}");
    assert_eq!(tool["base_tier"], "agent_extracted", "explain: {ex}");

    // Undo: the demotion back to the tier it had - the belief ties on trust again.
    let r = json_get(&uds_get(&sock, &format!("/api/resolve?observation={first}&tier=agent_extracted&kind=claim_demotion&workspace=*")).await);
    assert!(r["proposal_id"].is_string(), "undo: {r}");
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=billing").await);
    assert_eq!(g["nodes"][0]["contested"], true, "after undo: {g}");
    assert_eq!(g["nodes"][0]["trust_tier"], "agent_extracted");

    let bad = uds_get(
        &sock,
        &format!("/api/resolve?observation={first}&tier=agent_extracted&kind=recall"),
    )
    .await;
    assert!(bad.contains("400"), "an unknown kind is refused: {bad}");
    let _ = std::fs::remove_file(&sock);
}

/// design-system.md Section 29.2, U18: a settle's preview names the source a Confirm promotes, in
/// its own words, every entity whose ring moves with it and what else the source states, with the
/// checks - and writes nothing. Each value lists every source that asserts it, each with its tier
/// before any verdict. After the Confirm, the reverse act's preview says what Undo moves back.
#[tokio::test]
async fn a_settle_preview_names_the_source_and_its_reach() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "default"));
    let observe =
        |content: &str, entities: Vec<(&str, &str)>, relations: Vec<(&str, &str, &str)>| {
            ObserveInput {
                content: content.into(),
                workspace: Some("billing".into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: entities
                    .into_iter()
                    .map(|(name, kind)| EntityInput {
                        description: None,
                        name: name.into(),
                        kind: Some(kind.into()),
                    })
                    .collect(),
                relations: relations
                    .into_iter()
                    .map(|(from, kind, to)| RelationInput {
                        from: from.into(),
                        kind: kind.into(),
                        to: to.into(),
                        description: None,
                        valid_from: None,
                        valid_to: None,
                    })
                    .collect(),
            }
        };
    engine
        .observe(observe("The ledger is a table.", vec![("Ledger", "Table")], vec![]))
        .unwrap();
    let bundle = engine
        .observe(observe(
            "The nightly export sends the ledger to the finance team's warehouse.",
            vec![("Nightly export", "Job"), ("Ledger", "Concept"), ("Warehouse", "System")],
            vec![
                ("Nightly export", "reads", "Ledger"),
                ("Nightly export", "writes_to", "Warehouse"),
            ],
        ))
        .unwrap()
        .observation_id;
    let lone = engine
        .observe(observe(
            "The ledger is a concept of accounting.",
            vec![("Ledger", "Concept")],
            vec![],
        ))
        .unwrap()
        .observation_id;
    let sock = serve_uds("settle-preview", engine, ev_channel()).await;

    // Each value lists its sources: two say Concept, each with its tier before any verdict.
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=billing").await);
    let ledger = g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Ledger")
        .expect("Ledger");
    let id = ledger["id"].as_str().unwrap().to_string();
    let ex = json_get(&uds_get(&sock, &format!("/api/explain?entity={id}")).await);
    let kind = ex["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "kind")
        .expect("kind");
    let concept = kind["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["value"] == "Concept")
        .expect("Concept");
    let sources: Vec<&str> = concept["observations"]
        .as_array()
        .expect("observations")
        .iter()
        .map(|o| o["observation"].as_str().unwrap())
        .collect();
    assert_eq!(sources.len(), 2, "both sources of Concept: {concept}");
    assert!(sources.contains(&bundle.as_str()) && sources.contains(&lone.as_str()));
    assert_eq!(
        sources[0],
        concept["observation"].as_str().unwrap(),
        "the representative comes first"
    );
    for o in concept["observations"].as_array().unwrap() {
        assert_eq!(o["base_tier"], "agent_extracted", "{o}");
    }

    // The preview of confirming the bundle: its words, the rings it moves, what else it states.
    let before = json_get(&uds_get(&sock, "/api/observations?workspace=billing").await);
    let p = json_get(
        &uds_get(
            &sock,
            &format!("/api/resolve/preview?observation={bundle}&tier=human_confirmed"),
        )
        .await,
    );
    let d = &p["belief_diff"];
    assert_eq!(d["computable"], true, "preview: {p}");
    let row = &d["reach"][0];
    assert_eq!(row["observation"], bundle.as_str());
    assert!(row["said"].as_str().unwrap().starts_with("The nightly export sends"), "{row}");
    assert_eq!(
        (row["from"].as_str(), row["to"].as_str()),
        (Some("agent_extracted"), Some("human_confirmed"))
    );
    let rings: Vec<(String, String, String)> = row["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().into(),
                e["from"].as_str().unwrap().into(),
                e["to"].as_str().unwrap().into(),
            )
        })
        .collect();
    for name in ["Nightly export", "Warehouse", "Ledger"] {
        assert!(
            rings.contains(&(name.into(), "agent_extracted".into(), "human_confirmed".into())),
            "{name} rises with the bundle: {rings:?}"
        );
    }
    assert_eq!(row["relations"].as_array().map(Vec::len), Some(2), "{row}");
    assert!(
        p["checks"].as_array().unwrap().iter().all(|c| c["passed"] == true),
        "checks: {p}"
    );
    // It writes nothing.
    let after = json_get(&uds_get(&sock, "/api/observations?workspace=billing").await);
    assert_eq!(before, after, "the preview wrote to the log");
    let props = json_get(&uds_get(&sock, "/api/proposals?workspace=billing").await);
    assert_eq!(
        props.as_array().map(Vec::len),
        Some(0),
        "the preview opened a proposal: {props}"
    );

    // Confirm, then the reverse act's preview says what Undo moves back.
    let r = json_get(
        &uds_get(&sock, &format!("/api/resolve?observation={bundle}&tier=human_confirmed")).await,
    );
    assert!(r["proposal_id"].is_string(), "confirm: {r}");
    let undo = json_get(
        &uds_get(
            &sock,
            &format!("/api/resolve/preview?observation={bundle}&tier=agent_extracted&kind=claim_demotion"),
        )
        .await,
    );
    let back: Vec<(String, String)> = undo["belief_diff"]["reach"][0]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["from"] != e["to"])
        .map(|e| (e["name"].as_str().unwrap().into(), e["to"].as_str().unwrap().into()))
        .collect();
    assert!(
        back.contains(&("Nightly export".into(), "agent_extracted".into())),
        "undo: {undo}"
    );

    // Two acts are previewed as one: the demotion of the bundle and the confirmation of the lone
    // source leave Ledger confirmed through the lone source while Warehouse goes back.
    let pair = json_get(
        &uds_get(
            &sock,
            &format!(
                "/api/resolve/preview?observation={bundle}&tier=agent_extracted&kind=claim_demotion\
                 &observation2={lone}&tier2=human_confirmed"
            ),
        )
        .await,
    );
    let rows = pair["belief_diff"]["reach"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both sources move: {pair}");
    let ledger_ring = rows
        .iter()
        .flat_map(|r| r["entities"].as_array().unwrap().iter())
        .find(|e| e["name"] == "Ledger")
        .unwrap();
    assert_eq!(
        ledger_ring["to"], "human_confirmed",
        "Ledger stays confirmed through the lone source: {pair}"
    );

    let bad = uds_get(
        &sock,
        &format!("/api/resolve/preview?observation={bundle}&tier=human_confirmed&kind=recall"),
    )
    .await;
    assert!(bad.contains("400"), "an unknown kind is refused: {bad}");
    let _ = std::fs::remove_file(&sock);
}

/// design-system.md Section 29.3: a confirmation the console cast is said on its Decided item by the
/// source it confirmed, keeps the sentence its notice said as its rationale, and the undo read
/// answers the act that reverses it with that act's diff - one demotion, back to the tier the source
/// has without it. The Log names the same source. An id that is no merged gate proposal has nothing
/// to undo.
#[tokio::test]
async fn a_decided_confirmation_names_its_source_and_says_its_undo() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "default"));
    let bundle = engine
        .observe(ObserveInput {
            content: "The nightly export sends the ledger to the finance team's warehouse.".into(),
            workspace: Some("billing".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: ["Nightly export", "Ledger"]
                .into_iter()
                .map(|n| EntityInput { name: n.into(), kind: None, description: None })
                .collect(),
            relations: vec![],
        })
        .unwrap()
        .observation_id;
    let sock = serve_uds("decided-undo", engine, ev_channel()).await;

    let said = "Confirmed Concept as Ledger's type";
    let r = json_get(
        &uds_get(
            &sock,
            &format!(
                "/api/resolve?observation={bundle}&tier=human_confirmed&rationale={}",
                said.replace(' ', "%20").replace('\'', "%27")
            ),
        )
        .await,
    );
    let id = r["proposal_id"].as_str().expect("confirmed").to_string();
    let props = json_get(&uds_get(&sock, "/api/proposals?workspace=billing").await);
    let row = props
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.as_str())
        .expect("listed");
    assert_eq!(row["state"], "merged");
    assert_eq!(row["rationale"], said, "the notice's sentence is the rationale: {row}");
    assert_eq!(
        row["labels"][0],
        "\"The nightly export sends the ledger to the finance team's warehouse.\" (Nightly export, Ledger)",
        "the source in its own words and its entities: {row}"
    );

    let undo = json_get(&uds_get(&sock, &format!("/api/resolve/undo?proposal={id}")).await);
    let acts = undo["acts"].as_array().expect("acts");
    assert_eq!(acts.len(), 1, "{undo}");
    assert_eq!(acts[0]["observation"], bundle.as_str());
    assert_eq!(acts[0]["kind"], "claim_demotion");
    assert_eq!(acts[0]["tier"], "agent_extracted", "back to its tier without the act: {undo}");
    assert_eq!(undo["belief_diff"]["computable"], true, "{undo}");

    // The Log's event rows name the same source.
    let log = json_get(&uds_get(&sock, "/api/observations?workspace=billing").await);
    let verdict = log
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["proposal"]["event"] == "verdict")
        .expect("the verdict's row");
    assert_eq!(verdict["proposal"]["targets"][0]["name"], row["labels"][0], "{verdict}");

    let none = uds_get(&sock, "/api/resolve/undo?proposal=nothing").await;
    assert!(none.contains("404"), "nothing to undo: {none}");
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
        &uds_get(&sock, &format!("/api/reify?hyperedge={hid}&name=boot%20stack&workspace=ws"))
            .await,
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
    let engine =
        Arc::new(Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())));
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
            entities: vec![EntityInput {
                description: None,
                name: "supragnosis".into(),
                kind: Some("System".into()),
            }],
            relations: vec![],
        })
        .expect("observe 2");
    let sock = serve_uds("obslog", engine, ev_channel()).await;

    // The full log, newest-first, with provenance attached.
    let log = json_get(&uds_get(&sock, "/api/observations?workspace=ws").await);
    let arr = log.as_array().expect("log is an array");
    assert_eq!(arr.len(), 2, "two observations: {log}");
    assert!(arr[0]["attestations"][0]["host"].is_string(), "attestation host present: {log}");
    let (w0, w1) =
        (arr[0]["hlc"]["wall"].as_u64().unwrap(), arr[1]["hlc"]["wall"].as_u64().unwrap());
    assert!(w0 >= w1, "newest-first by hlc wall");

    // Resolve node ids from the graph, then the entity filter narrows the log.
    let g = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let node_id = |name: &str| {
        g["nodes"].as_array().unwrap().iter().find(|n| n["name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let sid = node_id("supragnosis");
    let rid = node_id("rmcp");
    let sfilt =
        json_get(&uds_get(&sock, &format!("/api/observations?workspace=ws&entity={sid}")).await);
    assert_eq!(sfilt.as_array().unwrap().len(), 2, "both observations touch supragnosis");
    let rfilt =
        json_get(&uds_get(&sock, &format!("/api/observations?workspace=ws&entity={rid}")).await);
    assert_eq!(rfilt.as_array().unwrap().len(), 1, "only the depends_on obs touches rmcp");

    // Explain: the per-field decision; supragnosis kind is contested (Project vs System).
    let ex = json_get(&uds_get(&sock, &format!("/api/explain?entity={sid}")).await);
    assert_eq!(ex["id"], sid);
    let kind_field = ex["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "kind")
        .expect("kind field");
    assert_eq!(kind_field["contested"], true, "two kinds tie -> contested: {ex}");
    let roles: Vec<&str> = kind_field["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["role"].as_str().unwrap())
        .collect();
    assert!(
        roles.contains(&"winner") && roles.contains(&"competitor"),
        "winner + competitor: {ex}"
    );
    assert_eq!(ex["supporting"].as_array().unwrap().len(), 2, "both obs support supragnosis");

    // Bad input: missing entity -> 400; an id that resolves to nothing -> 404 (P5).
    body_of(&uds_get(&sock, "/api/explain").await, "400");
    body_of(&uds_get(&sock, "/api/explain?entity=deadbeef").await, "404");

    let _ = std::fs::remove_file(&sock);
}

/// Whether `live` keeps every field of `example`, each with the same kind of value: the check that
/// an answer was not removed from or narrowed. An added field passes; an array is held to the
/// example element by element as far as both go.
fn keeps_fields(
    example: &serde_json::Value,
    live: &serde_json::Value,
    at: &str,
) -> Result<(), String> {
    use serde_json::Value;
    match (example, live) {
        (Value::Null, _) => Ok(()),
        (Value::Object(e), Value::Object(l)) => e.iter().try_for_each(|(k, v)| match l.get(k) {
            Some(lv) => keeps_fields(v, lv, &format!("{at}.{k}")),
            None => Err(format!("{at}.{k} is gone")),
        }),
        (Value::Array(e), Value::Array(l)) => {
            if !e.is_empty() && l.is_empty() {
                return Err(format!("{at} is empty where the example holds {}", e.len()));
            }
            e.iter()
                .zip(l)
                .enumerate()
                .try_for_each(|(i, (x, y))| keeps_fields(x, y, &format!("{at}[{i}]")))
        }
        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => Ok(()),
        _ => Err(format!("{at}: the example holds {example}, the viewer answers {live}")),
    }
}

/// guard (inspector.md D6, compatibility.md Section 8): no field of /api/graph, /api/explain or
/// /api/observations is removed or narrowed. Each has an example document under
/// tests/fixtures/api/, answered by the viewer over a fixed scenario - two observations, a relation
/// and a contested kind - and the live answer must keep every field of it with the same kind of
/// value. An added field passes; `SUPRAGNOSIS_WRITE_FIXTURES=1` rewrites the examples to include it.
#[tokio::test]
async fn the_viewer_api_keeps_its_fields() {
    // Every act in one fixed millisecond, so the contested kind falls to the same side each run and
    // the example's keys - a type count is keyed by the winning type - are the live answer's.
    struct Fixed;
    impl supragnosis_core::Clock for Fixed {
        fn now_millis(&self) -> supragnosis_core::Timestamp {
            1_792_000_000_000
        }
    }
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "ws").with_clock(Arc::new(Fixed)));
    observe_depends(&engine);
    engine
        .observe(ObserveInput {
            content: "supragnosis is a system".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                description: None,
                name: "supragnosis".into(),
                kind: Some("System".into()),
            }],
            relations: vec![],
        })
        .expect("observe");
    let sock = serve_uds("apifix", engine, ev_channel()).await;
    let graph = json_get(&uds_get(&sock, "/api/graph?workspace=ws").await);
    let sid = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "supragnosis")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let answers = [
        ("graph", graph),
        (
            "explain",
            json_get(&uds_get(&sock, &format!("/api/explain?entity={sid}")).await),
        ),
        (
            "observations",
            json_get(&uds_get(&sock, "/api/observations?workspace=ws").await),
        ),
    ];
    let _ = std::fs::remove_file(&sock);

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/api");
    let write = std::env::var("SUPRAGNOSIS_WRITE_FIXTURES").is_ok_and(|v| v == "1");
    for (name, live) in answers {
        let path = dir.join(format!("{name}.json"));
        if write {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&live).unwrap() + "\n").unwrap();
            continue;
        }
        let example: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        )
        .unwrap();
        keeps_fields(&example, &live, name).unwrap_or_else(|e| panic!("{e}"));
    }
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

    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    let listener = supragnosis_viz::bind_uds(&path).await.expect("bind_uds");
    tokio::spawn(supragnosis_viz::serve(engine, listener, ev_channel(), None, None, None));

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

/// guard (unmerge.md): the console can reverse a committed merge, and doing so is as gated as the
/// merge was - `/api/propose_split` OPENS a proposal and commits nothing. A route that exists and is
/// never exercised is a route that rots; this one is the only surface an operator has for the act.
#[tokio::test]
async fn viz_propose_split_opens_a_reversal_without_committing_it() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "host-a", "ws1"));
    observe_depends(&engine);
    // Ids from the graph rather than recomputed - viz has no core dependency, and a test that
    // re-derived the id formula would be asserting the formula rather than the route.
    let id_of = |name: &str| {
        engine
            .graph(Some("ws1"))
            .expect("graph")
            .nodes
            .into_iter()
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("{name} is in the graph"))
            .id
    };
    let (a, b) = (id_of("supragnosis"), id_of("rmcp"));
    let merge = engine
        .propose(supragnosis_engine::ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![a.clone(), b.clone()],
            into: Some(b.clone()),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose merge");
    engine
        .review_proposal(
            None,
            merge.clone(),
            "merge".into(),
            None,
            None,
            supragnosis_engine::VerdictSurface::Console,
        )
        .expect("accept the merge");

    let sock = serve_uds("split", engine.clone(), ev_channel()).await;
    let resp = uds_get(&sock, &format!("/api/propose_split?merge={merge}")).await;
    let id = json_get(&resp)["proposal_id"].as_str().expect("a proposal id").to_string();

    let view = engine.get_proposal(Some("ws1"), &id).expect("get").expect("view");
    assert_eq!(view.kind, "entity_split");
    assert_eq!(view.state, "open", "opening a reversal commits nothing - the human accepts it");
    assert!(
        engine.graph(Some("ws1")).expect("graph").nodes.iter().all(|n| n.id != a),
        "and the merge still stands until then"
    );

    // A missing parameter is refused with the fix named, not silently treated as no-op.
    let bad = uds_get(&sock, "/api/propose_split").await;
    assert!(bad.starts_with("HTTP/1.1 400"), "got: {}", bad.lines().next().unwrap_or(""));
}

/// A slow request holds a blocking thread, not the async worker. The viewer shares its runtime with
/// the MCP daemon, and its routes are synchronous - a store read, a fold over the log, the merge
/// band, a config file written - so run on the worker, one slow request stalled every other
/// connection and tool call on it. Here the narrowing hook takes 800ms, and a health check sent
/// while it runs is answered long before it ends. The test's runtime has one thread, so a handler
/// that held it would hold the health check too; elapsed time is wall-clock for the same reason.
#[tokio::test]
async fn a_slow_request_holds_no_other_one() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    let narrow: supragnosis_viz::NarrowShare = Arc::new(|_: &str, keep: &[String]| {
        std::thread::sleep(std::time::Duration::from_millis(800));
        Ok(keep.to_vec())
    });
    let path = serve_uds_with_narrow("slow", engine, ev_channel(), narrow).await;
    let started = std::time::Instant::now();
    let slow_path = path.clone();
    let slow = tokio::spawn(async move {
        uds_request(&slow_path, "POST", "/api/peer/share?node_id=a&workspaces=alpha").await
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let health = uds_get(&path, "/api/health").await;
    let answered = started.elapsed();
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(
        answered < std::time::Duration::from_millis(500),
        "the health check waited on the slow request: {answered:?}"
    );
    assert!(slow.await.expect("slow request").starts_with("HTTP/1.1 200"));
}

/// Every act the console takes under All is filed where its item lives (design-system.md Sections 7
/// and 29.15): a verdict in its proposal's workspace, a suggested merge in its pair's, a split in its
/// merge's and a named region in its observations' - none in the node default - and the console
/// opens no merge across workspaces.
#[tokio::test]
async fn every_act_under_all_is_filed_where_its_item_lives() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store.clone(), "h", "default"));
    let in_ws = |ws: &str, names: &[&str]| ObserveInput {
        content: format!("{} in {ws}", names.join(" and ")),
        workspace: Some(ws.into()),
        source_ref: None,
        confidence: None,
        on_behalf_of: None,
        derived_from: vec![],
        entities: names
            .iter()
            .map(|n| EntityInput { description: None, name: (*n).into(), kind: None })
            .collect(),
        relations: vec![],
    };
    engine
        .observe(in_ws("billing", &["Postgres", "PostgreSQL", "Ledger"]))
        .expect("observe");
    engine.observe(in_ws("web", &["Postgres"])).expect("observe");
    let id = |ws: &str, n: &str| supragnosis_core::Entity::make_id(ws, n);
    let sock = serve_uds("act-under-all", engine.clone(), ev_channel()).await;

    // A suggested merge under All is filed where its pair lives.
    let q = format!(
        "/api/propose_merge?a={}&b={}&src=variant:separator&workspace=*",
        id("billing", "Postgres"),
        id("billing", "PostgreSQL")
    );
    let merge = json_get(&uds_get(&sock, &q).await)["proposal_id"]
        .as_str()
        .expect("proposal id")
        .to_string();
    assert_eq!(engine.proposal_workspace(&merge).expect("read").as_deref(), Some("billing"));
    // Its accept under All is filed with it, so the merge reads Merged in billing itself.
    let r =
        uds_get(&sock, &format!("/api/review?proposal={merge}&decision=merge&workspace=*")).await;
    body_of(&r, "200");
    let state = |ws: Option<&str>| engine.get_proposal(ws, &merge).expect("get").map(|p| p.state);
    assert_eq!(state(Some("billing")).as_deref(), Some("merged"));
    assert_eq!(
        state(None),
        state(Some("billing")),
        "All reads the proposal as its workspace does"
    );
    // Un-merge under All is filed in the merge's workspace.
    let split =
        json_get(&uds_get(&sock, &format!("/api/propose_split?merge={merge}&workspace=*")).await)
            ["proposal_id"]
            .as_str()
            .expect("split id")
            .to_string();
    assert_eq!(engine.proposal_workspace(&split).expect("read").as_deref(), Some("billing"));
    // A pair across workspaces, and a source the server does not know, are refused.
    let q = format!(
        "/api/propose_merge?a={}&b={}&workspace=*",
        id("web", "Postgres"),
        id("billing", "PostgreSQL")
    );
    let r = uds_get(&sock, &q).await;
    assert!(
        body_of(&r, "400").contains("The console opens no merge across workspaces."),
        "{r}"
    );
    let q = format!(
        "/api/propose_merge?a={}&b={}&src=made-up",
        id("billing", "Ledger"),
        id("billing", "PostgreSQL")
    );
    body_of(&uds_get(&sock, &q).await, "400");
    // A verdict on an id no proposal has is refused, and the log keeps no row for it.
    body_of(
        &uds_get(&sock, "/api/review?proposal=nope&decision=reject&workspace=*").await,
        "400",
    );
    // A region named under All is asserted in its observations' workspace.
    let hg = engine.hypergraph(None).expect("hypergraph");
    let h = hg
        .hyperedges
        .iter()
        .find(|h| h.member_names.iter().any(|n| n == "Ledger"))
        .expect("billing's region");
    let r =
        uds_get(&sock, &format!("/api/reify?hyperedge={}&name=Storage&workspace=*", h.id)).await;
    body_of(&r, "200");
    assert!(
        engine.get_entity(&id("billing", "Storage")).expect("get").is_some(),
        "the region is not in billing"
    );
    assert!(
        engine.get_entity(&id("default", "Storage")).expect("get").is_none(),
        "the region went to the node default"
    );
    assert!(
        engine.list_proposals(Some("default")).expect("list").is_empty()
            && engine.curation(None).expect("curation").stray_verdicts.is_empty(),
        "an act under All was filed in the node default"
    );
    let _ = std::fs::remove_file(&sock);
}

/// The same name in other workspaces, on the local console (design-system.md Section 29.16): one
/// `in` per workspace the page lists, each a lookup; the entity's own workspace is never listed, an
/// unknown entity is said without spelling its id.
#[tokio::test]
async fn the_console_reads_the_same_name_in_other_workspaces() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "default"));
    for (ws, name) in [("infra", "Payment service"), ("atlas, web", "payment service")] {
        engine
            .observe(ObserveInput {
                content: format!("{name} in {ws}"),
                workspace: Some(ws.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput {
                    description: None,
                    name: name.into(),
                    kind: Some("Service".into()),
                }],
                relations: vec![],
            })
            .expect("observe");
    }
    let here = supragnosis_core::Entity::make_id("infra", "Payment service");
    let sock = serve_uds("namesakes", engine, ev_channel()).await;
    let r = uds_get(
        &sock,
        &format!("/api/namesakes?entity={here}&in=infra&in=atlas%2C%20web&in=default"),
    )
    .await;
    let list = json_get(&r)["namesakes"].clone();
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    assert_eq!(list[0]["workspace"], "atlas, web", "a comma in a workspace name split it");
    assert_eq!(list[0]["name"], "payment service");
    assert_eq!(list[0]["type"], "Service");
    let r = uds_get(&sock, &format!("/api/namesakes?entity={}&in=infra", "ab".repeat(32))).await;
    let body = body_of(&r, "404");
    assert!(body.contains("no entity by that id") && !body.contains("abab"), "{body}");
    body_of(&uds_get(&sock, "/api/namesakes?in=infra").await, "400");
    let _ = std::fs::remove_file(&sock);
}

/// Same as... and Keep ... instead open merges the console marks, with reasons that say why
/// (design-system.md Section 29.17): `src=picked` and `src=reverse` have their own, a re-merge after an
/// un-merge counts the un-merges so it is a different proposal (unmerge.md S10), and the un-merge the
/// console opens carries the mark too.
#[tokio::test]
async fn the_console_opens_marked_merges_that_can_follow_an_unmerge() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "infra"));
    engine
        .observe(ObserveInput {
            content: "Postgres and PostgreSQL".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: ["Postgres", "PostgreSQL"]
                .iter()
                .map(|n| EntityInput { description: None, name: (*n).into(), kind: None })
                .collect(),
            relations: vec![],
        })
        .expect("observe");
    let id = |n: &str| supragnosis_core::Entity::make_id("infra", n);
    let sock = serve_uds("same-as", engine.clone(), ev_channel()).await;
    let open = |src: &'static str| {
        let sock = sock.clone();
        let (a, b) = (id("Postgres"), id("PostgreSQL"));
        async move {
            let r =
                uds_get(&sock, &format!("/api/propose_merge?a={a}&b={b}&src={src}&workspace=*"))
                    .await;
            json_get(&r)["proposal_id"].as_str().expect("proposal id").to_string()
        }
    };
    let marked = |pid: &str| {
        engine
            .get_observation(pid)
            .expect("get")
            .expect("the opening")
            .provenance
            .iter()
            .all(|p| p.source_ref.as_deref() == Some(supragnosis_core::VERDICT_SURFACE_CONSOLE))
    };
    let reason =
        |pid: &str| engine.get_proposal(None, pid).expect("get").expect("proposal").rationale;
    let first = open("picked").await;
    assert!(marked(&first), "a merge the console opened carries no mark");
    assert_eq!(reason(&first).as_deref(), Some("two entities named as one from a search"));
    let reverse = uds_get(
        &sock,
        &format!("/api/propose_merge?a={}&b={}&src=reverse", id("PostgreSQL"), id("Postgres")),
    )
    .await;
    let reverse = json_get(&reverse)["proposal_id"].as_str().expect("id").to_string();
    assert_eq!(
        reason(&reverse).as_deref(),
        Some("the other name kept, instead of a merge the other way")
    );
    // Accept the first, un-merge it, accept the un-merge, and open the same merge again.
    body_of(
        &uds_get(&sock, &format!("/api/review?proposal={first}&decision=merge")).await,
        "200",
    );
    let split = uds_get(&sock, &format!("/api/propose_split?merge={first}")).await;
    let split = json_get(&split)["proposal_id"].as_str().expect("split id").to_string();
    assert!(marked(&split), "an un-merge the console opened carries no mark");
    body_of(
        &uds_get(&sock, &format!("/api/review?proposal={split}&decision=merge")).await,
        "200",
    );
    let again = open("picked").await;
    assert_ne!(again, first, "the re-merge is the merge the split reversed");
    assert_eq!(
        reason(&again).as_deref(),
        Some("two entities named as one from a search, again after 1 un-merge")
    );
    let _ = std::fs::remove_file(&sock);
}

/// design-system.md Section 29.23: the console's acts that open a proposal say what they found. A
/// merge or an un-merge that waits is found, not opened twice; a rejected one is asked again; and a
/// Confirm after an Undo confirms, while one already in force changes nothing and says so.
#[tokio::test]
async fn a_console_repeat_says_what_it_found() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, "h", "infra"));
    let obs = engine
        .observe(ObserveInput {
            content: "Postgres and PostgreSQL".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: ["Postgres", "PostgreSQL"]
                .iter()
                .map(|n| EntityInput {
                    description: None,
                    name: (*n).into(),
                    kind: Some("Database".into()),
                })
                .collect(),
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let id = |n: &str| supragnosis_core::Entity::make_id("infra", n);
    let sock = serve_uds("repeat", engine.clone(), ev_channel()).await;
    let merge_q = format!("/api/propose_merge?a={}&b={}", id("Postgres"), id("PostgreSQL"));
    let first = json_get(&uds_get(&sock, &merge_q).await);
    assert_eq!((first["state"].as_str(), first["found"].as_bool()), (Some("open"), Some(false)));
    let pid = first["proposal_id"].as_str().expect("id").to_string();
    let found = json_get(&uds_get(&sock, &merge_q).await);
    assert_eq!(found["proposal_id"], pid.as_str());
    assert_eq!((found["state"].as_str(), found["found"].as_bool()), (Some("open"), Some(true)));
    body_of(
        &uds_get(&sock, &format!("/api/review?proposal={pid}&decision=reject")).await,
        "200",
    );
    let asked = json_get(&uds_get(&sock, &merge_q).await);
    assert_ne!(asked["proposal_id"], pid.as_str(), "a rejected merge is asked again");
    assert_eq!(asked["found"], false);

    // Confirm, Undo, Confirm: the third act confirms; a fourth finds it in force.
    let resolve = |tier: &str, kind: &str, why: &str| {
        format!(
            "/api/resolve?observation={obs}&tier={tier}&kind={kind}&rationale={}",
            why.replace(' ', "%20")
        )
    };
    let tier = || engine.observation(&obs, None).expect("read").expect("obs").effective_tier;
    // A local act's order is its wall-clock millisecond (Hlc::legacy), and two in one millisecond
    // fall back to the proposal id; a person's acts are never that close, so these are not.
    let pause = || std::thread::sleep(std::time::Duration::from_millis(2));
    let before = tier();
    body_of(
        &uds_get(&sock, &resolve("human_confirmed", "claim_promotion", "Confirmed it")).await,
        "200",
    );
    assert_eq!(tier(), supragnosis_core::TrustTier::HumanConfirmed);
    pause();
    body_of(
        &uds_get(&sock, &resolve("agent_extracted", "claim_demotion", "Undid it")).await,
        "200",
    );
    assert_eq!(tier(), before);
    pause();
    let again = json_get(
        &uds_get(&sock, &resolve("human_confirmed", "claim_promotion", "Confirmed it")).await,
    );
    assert!(again.get("verdict_observation_id").is_some(), "{again}");
    assert_eq!(
        tier(),
        supragnosis_core::TrustTier::HumanConfirmed,
        "the Confirm after the Undo confirms"
    );
    let in_force = json_get(
        &uds_get(&sock, &resolve("human_confirmed", "claim_promotion", "Confirmed it")).await,
    );
    assert_eq!(
        (in_force["state"].as_str(), in_force["found"].as_bool()),
        (Some("merged"), Some(true))
    );
    assert!(in_force.get("verdict_observation_id").is_none(), "nothing cast: {in_force}");
    let _ = std::fs::remove_file(&sock);
}
