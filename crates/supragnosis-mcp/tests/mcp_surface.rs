//! MCP surface integration tests (no LLM, deterministic).
//!
//! Connects a real rmcp client to `SupragnosisServer` over an in-process duplex pipe and
//! drives the MCP protocol as-is: handshake -> tools/list -> tools/call.
//! Verifies the surface an LLM will actually see (tool names/descriptions/JSON schema) and the
//! end-to-end behavior of each tool. Any LLM eval is only meaningful on top of a surface that passes this.
//!
//! These tests need no network/model, so they are part of the default `cargo test`.

use std::collections::BTreeSet;
use std::sync::Arc;

use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ResourceContents,
};
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, ObserveInput};
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::InMemoryStore;

/// Parse the first text content a tool returned as JSON.
/// (Tools return a JSON string and rmcp wraps it as text content.)
fn tool_json(res: &CallToolResult) -> Value {
    let text = res
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("tool should return one text content");
    serde_json::from_str(&text).expect("tool text should be valid JSON")
}

/// Turn a serde_json object literal into tool arguments (JsonObject).
fn args(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("test arguments must be a JSON object")
}

#[tokio::test]
async fn mcp_protocol_surface_end_to_end() {
    // Attach a deterministic embedder to drive even the hybrid search path through the protocol (non-persistent store).
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );

    // Connect server<->client with an in-process bidirectional pipe.
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        // Keep the server alive until the client finishes.
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    // --- 1) tools/list: the surface an LLM will see (Principle 21: a narrow, readable surface) ---
    let tools = client.list_all_tools().await.expect("list tools");
    let names: BTreeSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "observe",
            "define_type",
            "get_entity",
            "search_knowledge",
            "traverse",
            "workspace_map",
            "propose",
            "review",
            "list_proposals",
            "get_proposal",
            "sync_status",
            "sync_pull",
            "sync_push",
        ]),
        "must expose the intent-level tools (workspace_map = orientation, define_type = T-Box, propose/review/list_proposals/get_proposal = the canon gate, sync_* = federation administration)"
    );
    // Every gated intent has to be reachable, and an agent only learns a kind exists by reading this
    // description. `entity_split` shipped in the engine before it was named here, which is the drift
    // this guards: a kind the fold accepts and the surface never mentions is a kind nobody uses.
    let propose_desc = tools
        .iter()
        .find(|t| t.name.as_ref() == "propose")
        .and_then(|t| t.description.as_deref())
        .expect("propose has a description");
    for kind in supragnosis_engine::PROPOSAL_KINDS {
        assert!(
            propose_desc.contains(kind),
            "propose must name every kind the engine accepts; '{kind}' is missing"
        );
    }

    for t in &tools {
        let desc = t.description.as_deref().unwrap_or("");
        assert!(
            !desc.trim().is_empty(),
            "tool '{}' must have a description for the LLM to read",
            t.name
        );
        // Each tool exposes an input JSON schema (object + properties).
        assert_eq!(
            t.input_schema.get("type").and_then(Value::as_str),
            Some("object"),
            "tool '{}' input_schema must be an object",
            t.name
        );
    }
    // Whether the key parameter content is exposed in the observe schema.
    let observe = tools.iter().find(|t| t.name == "observe").unwrap();
    let props = observe
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("observe schema has properties");
    assert!(props.contains_key("content"), "observe exposes the content parameter");

    // --- 2) observe: ingest knowledge (2 entities + 1 relation) -------------------
    let res = client
        .call_tool(CallToolRequestParams::new("observe").with_arguments(args(json!({
            "content": "supragnosis is a rust knowledge server built on rmcp",
            "workspace": "ws",
            "entities": [
                {"name": "supragnosis", "type": "Project"},
                {"name": "rmcp", "type": "Tool"}
            ],
            "relations": [
                {"from": "supragnosis", "type": "depends_on", "to": "rmcp"}
            ]
        }))))
        .await
        .expect("observe call");
    let out = tool_json(&res);
    assert!(
        out["observation_id"].as_str().is_some_and(|s| !s.is_empty()),
        "observe must return an observation id: {out}"
    );
    let entity_ids = out["entities"].as_array().expect("entities array");
    assert_eq!(entity_ids.len(), 2, "2 entities must be linked: {out}");
    assert_eq!(
        out["relations"].as_array().map(Vec::len),
        Some(1),
        "1 relation must be linked: {out}"
    );
    let supragnosis_id = entity_ids[0].as_str().unwrap().to_string();

    // --- 3) search_knowledge: recall the ingested knowledge via hybrid search -----
    let res = client
        .call_tool(
            CallToolRequestParams::new("search_knowledge")
                .with_arguments(args(json!({"query": "rust", "workspace": "ws"}))),
        )
        .await
        .expect("search call");
    let found = tool_json(&res);
    assert!(
        found["hits"].as_array().is_some_and(|a| !a.is_empty()),
        "search must find the ingested knowledge: {found}"
    );
    // The response reports the surface used (mode) (Principle 16, 4th revision) - this assembly has an embedder, so hybrid.
    assert_eq!(found["mode"].as_str(), Some("hybrid"), "the mode must be reported: {found}");

    // --- 3b) empty search result: accompanied by an open-world note (Principle 5) ---
    // Hybrid returns the nearest neighbors with no similarity threshold, so we produce zero hits with an
    // empty-workspace scope (like the pre-sync partial knowledge of a distributed node).
    let res = client
        .call_tool(
            CallToolRequestParams::new("search_knowledge")
                .with_arguments(args(json!({"query": "anything", "workspace": "empty-ws"}))),
        )
        .await
        .expect("empty search call");
    let empty = tool_json(&res);
    assert!(
        empty["hits"].as_array().is_some_and(Vec::is_empty),
        "query that must yield zero hits: {empty}"
    );
    assert!(
        empty["note"].as_str().is_some_and(|n| n.contains("not a negation")),
        "an empty result must carry an absence!=negation note (to prevent LLM misreading): {empty}"
    );

    // --- 4) get_entity: re-query by the id observe returned (relations included) ---
    let res = client
        .call_tool(
            CallToolRequestParams::new("get_entity")
                .with_arguments(args(json!({"id": supragnosis_id}))),
        )
        .await
        .expect("get_entity call");
    let ent = tool_json(&res);
    assert_eq!(
        ent["canonical_name"].as_str(),
        Some("supragnosis"),
        "must retrieve the entity by id: {ent}"
    );
    assert_eq!(
        ent["relations"].as_array().map(Vec::len),
        Some(1),
        "entity lookup must come with its relations: {ent}"
    );
    // The internal recall vector must not leak to the LLM surface (Principle 21: a narrow, readable surface).
    assert!(
        ent.get("embedding").is_none(),
        "the get_entity response must not expose the embedding vector (context contamination): {ent}"
    );

    // --- 5) traverse: supragnosis -> rmcp (depends_on, 1 hop) --------------------
    let res = client
        .call_tool(
            CallToolRequestParams::new("traverse")
                .with_arguments(args(json!({"id": supragnosis_id}))),
        )
        .await
        .expect("traverse call");
    let reached = tool_json(&res);
    assert!(
        reached["hits"]
            .as_array()
            .is_some_and(|a| a.iter().any(|h| h["name"] == "rmcp")),
        "traverse must reach the depends_on neighbor rmcp: {reached}"
    );

    // --- 5b) traverse an unknown id: empty result + cause-distinguishing note (Principles 5/21) ---
    let res = client
        .call_tool(
            CallToolRequestParams::new("traverse")
                .with_arguments(args(json!({"id": "does-not-exist"}))),
        )
        .await
        .expect("traverse unknown call");
    let empty_tr = tool_json(&res);
    assert!(
        empty_tr["note"]
            .as_str()
            .is_some_and(|n| n.contains("not found")),
        "zero hits from an unknown start point must carry a 'missing start entity' note: {empty_tr}"
    );

    // --- 6) get_entity(unknown id): open-world - unknown, not an error (Principle 5) ---
    let res = client
        .call_tool(
            CallToolRequestParams::new("get_entity")
                .with_arguments(args(json!({"id": "does-not-exist"}))),
        )
        .await
        .expect("get_entity unknown call");
    let unknown = tool_json(&res);
    assert_eq!(
        unknown["found"].as_bool(),
        Some(false),
        "absence must be found:false, not an error (to prevent LLM misreading): {unknown}"
    );

    // --- 7) workspace_map: survey co-occurrence clusters (Principle 11 second-order structure) ---
    // supragnosis + rmcp are asserted together in a single observation -> one size-2 cluster, exposed by name.
    let res = client
        .call_tool(
            CallToolRequestParams::new("workspace_map")
                .with_arguments(args(json!({"workspace": "ws"}))),
        )
        .await
        .expect("workspace_map call");
    let map = tool_json(&res);
    let clusters = map["clusters"].as_array().expect("clusters array");
    assert!(!clusters.is_empty(), "there must be a co-occurrence cluster: {map}");
    let concepts: Vec<&str> = clusters[0]["concepts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    assert!(
        concepts.contains(&"supragnosis") && concepts.contains(&"rmcp"),
        "a cluster must expose concepts by name, not id (LLM readability): {map}"
    );
    assert_eq!(clusters[0]["size"].as_u64(), Some(2), "co-occurrence size 2: {map}");
    // Names are for reading; the ids are what the next call takes (P2/P14): the cluster's own
    // hyperedge id, and one entity id per member in the same order as the names.
    assert_eq!(clusters[0]["id"].as_str().map(str::len), Some(64), "hyperedge id: {map}");
    let members: Vec<&str> = clusters[0]["members"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    assert_eq!(members.len(), concepts.len(), "one id per concept, same order: {map}");
    assert!(members.iter().all(|m| m.len() == 64), "member ids are entity ids: {map}");

    // Cleanup: shutting down the client closes the server pipe and ends the server task.
    client.cancel().await.expect("client shutdown");
    let _ = server.await;
}

/// Resource surface: verifies, over the protocol as-is, the path that exposes the ontology graph as an MCP resource.
/// Discovers via list_resources/list_resource_templates, receives node-link JSON via read_resource,
/// and checks that the ingested knowledge is reflected in the graph and that an unknown URI errors.
#[tokio::test]
async fn mcp_resource_graph_surface() {
    // Build the engine with default workspace "ws" (non-persistent).
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));

    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    // Ingest knowledge: supragnosis --depends_on--> rmcp (2 nodes, 1 edge).
    let observed = client
        .call_tool(CallToolRequestParams::new("observe").with_arguments(args(json!({
            "content": "supragnosis depends on rmcp",
            "workspace": "ws",
            "on_behalf_of": "ashon",
            "entities": [
                {"name": "supragnosis", "type": "Project"},
                {"name": "rmcp", "type": "Tool"}
            ],
            "relations": [{"from": "supragnosis", "type": "depends_on", "to": "rmcp"}]
        }))))
        .await
        .expect("observe call");
    let observation_id = tool_json(&observed)["observation_id"]
        .as_str()
        .expect("observation id")
        .to_string();

    // --- 1) list_resources: expose the default workspace graph + workspace list resources ----
    let resources = client.list_all_resources().await.expect("list resources");
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(
        uris.contains(&"supragnosis://workspace/ws/graph"),
        "must expose the default workspace graph resource: {uris:?}"
    );
    assert!(
        uris.contains(&"supragnosis://workspaces"),
        "must expose the workspace list resource (discovery entry point): {uris:?}"
    );
    assert!(
        uris.contains(&"supragnosis://workspace/ws/hypergraph"),
        "must also expose the default workspace hypergraph resource (discoverability): {uris:?}"
    );

    // --- 1b) read_resource(workspaces): array of workspace names that hold knowledge --------------
    let read = client
        .read_resource(ReadResourceRequestParams::new("supragnosis://workspaces"))
        .await
        .expect("read workspaces resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let workspaces: Value = serde_json::from_str(&text).expect("workspaces resource is JSON");
    assert!(
        workspaces.as_array().is_some_and(|a| a.iter().any(|w| w == "ws")),
        "the workspace list must contain the ingested 'ws': {workspaces}"
    );

    // --- 2) list_resource_templates: templates for querying any workspace ------------------
    let templates = client.list_all_resource_templates().await.expect("list templates");
    assert!(
        templates
            .iter()
            .any(|t| t.uri_template == "supragnosis://workspace/{workspace}/graph"),
        "must expose the graph resource template"
    );
    assert!(
        templates
            .iter()
            .any(|t| t.uri_template == "supragnosis://workspace/{workspace}/hypergraph"),
        "must also expose the hypergraph resource template"
    );

    // --- 3) read_resource: receive node-link graph JSON and confirm the ingested knowledge -------------
    let read = client
        .read_resource(ReadResourceRequestParams::new("supragnosis://workspace/ws/graph"))
        .await
        .expect("read graph resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let graph: Value = serde_json::from_str(&text).expect("graph resource is JSON");
    assert_eq!(graph["stats"]["node_count"].as_u64(), Some(2), "2 nodes in the graph: {graph}");
    assert_eq!(graph["stats"]["edge_count"].as_u64(), Some(1), "1 edge in the graph: {graph}");
    // The edge is depends_on and node names are carried in the graph.
    let names: Vec<&str> = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["name"].as_str())
        .collect();
    assert!(
        names.contains(&"supragnosis") && names.contains(&"rmcp"),
        "node names must be in the graph: {names:?}"
    );
    assert_eq!(graph["edges"][0]["type"].as_str(), Some("depends_on"));

    // --- 3b) hypergraph resource: co-occurrence second-order structure (Principle 11) - members exposed by name --------
    let read = client
        .read_resource(ReadResourceRequestParams::new("supragnosis://workspace/ws/hypergraph"))
        .await
        .expect("read hypergraph resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let hg: Value = serde_json::from_str(&text).expect("hypergraph resource is JSON");
    // supragnosis + rmcp are asserted together in a single observation -> 1 hyperedge (size 2).
    assert_eq!(
        hg["stats"]["hyperedge_count"].as_u64(),
        Some(1),
        "there must be 1 hyperedge: {hg}"
    );
    let member_names: Vec<&str> = hg["hyperedges"][0]["member_names"]
        .as_array()
        .expect("member_names array")
        .iter()
        .filter_map(|n| n.as_str())
        .collect();
    assert!(
        member_names.contains(&"supragnosis") && member_names.contains(&"rmcp"),
        "a hyperedge must expose members by name (not id-only): {hg}"
    );

    // --- 4) observation back-reference (Principles 2/14): query raw content+provenance+lineage by the id from a search hit/observe --
    let read = client
        .read_resource(ReadResourceRequestParams::new(format!(
            "supragnosis://observation/{observation_id}"
        )))
        .await
        .expect("read observation resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let obs: Value = serde_json::from_str(&text).expect("observation resource is JSON");
    assert_eq!(
        obs["content"].as_str(),
        Some("supragnosis depends on rmcp"),
        "the observation's raw content must come back: {obs}"
    );
    assert_eq!(
        obs["provenance"][0]["on_behalf_of"].as_str(),
        Some("ashon"),
        "provenance (including the delegation chain) must come back - the terminus of 'where did this answer come from': {obs}"
    );
    assert!(
        obs.get("embedding").is_none(),
        "the observation resource must not expose the embedding vector (Principle 21): {obs}"
    );

    // --- 5) unknown observation id: absence is not_found (with an open-world hint) -------------------
    let missing = client
        .read_resource(ReadResourceRequestParams::new("supragnosis://observation/does-not-exist"))
        .await;
    assert!(missing.is_err(), "an unknown observation id must be a not_found error");

    // --- 6) unknown URI: absence surfaces as an error (with a Principle 5 self-correction hint) ------------------
    let bad = client.read_resource(ReadResourceRequestParams::new("supragnosis://nope")).await;
    assert!(bad.is_err(), "an unknown resource URI must be an error");

    client.cancel().await.expect("client shutdown");
    let _ = server.await;
}

/// An empty node default workspace must read as a wrong scope, not as an absent ontology.
///
/// Knowledge is deliberately organized into named workspaces, so the node default is routinely empty by
/// design. A reader who surveys it and is told only "nothing here" concludes the node holds no knowledge -
/// the exact misreading this asserts against. The response must instead name the workspaces that do hold
/// knowledge, so the dead end corrects itself in the same call (Principle 5: absence is not negation).
#[tokio::test]
async fn empty_default_workspace_names_where_knowledge_lives() {
    // Node default is "default"; all knowledge is observed into the named workspace "supragnosis".
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "default"));
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    client
        .call_tool(CallToolRequestParams::new("observe").with_arguments(args(json!({
            "content": "the proposal gate stands between free ingest and the shared canon",
            "workspace": "supragnosis",
            "entities": [
                {"name": "Proposal Gate", "type": "Mechanism"},
                {"name": "Canon", "type": "Concept"}
            ],
            "relations": [
                {"from": "Proposal Gate", "type": "guards", "to": "Canon"}
            ]
        }))))
        .await
        .expect("observe call");

    // Survey with no workspace argument -> the node default, which is empty by design.
    let res = client
        .call_tool(CallToolRequestParams::new("workspace_map").with_arguments(args(json!({}))))
        .await
        .expect("workspace_map call");
    let map = tool_json(&res);
    assert_eq!(
        map["stats"]["node_count"].as_u64(),
        Some(0),
        "the node default workspace is empty in this fixture: {map}"
    );

    // The structured pointer: a machine reader must not have to parse prose to recover.
    let listed: Vec<&str> = map["knowledge_in_workspaces"]
        .as_array()
        .expect("an empty scope must report where knowledge does live")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        listed,
        vec!["supragnosis"],
        "the populated workspace must be named, and the queried empty scope excluded: {map}"
    );

    // The prose must not let "empty default" be read as "no knowledge on this node".
    let note = map["note"].as_str().unwrap_or_default();
    assert!(
        note.contains("supragnosis") && note.contains("scope miss"),
        "the note must name the populated workspace and frame the miss as scope, not absence: {map}"
    );

    // Scoping to the named workspace recovers the cluster that the default scope could not see.
    let res = client
        .call_tool(
            CallToolRequestParams::new("workspace_map")
                .with_arguments(args(json!({"workspace": "supragnosis"}))),
        )
        .await
        .expect("workspace_map scoped call");
    let scoped = tool_json(&res);
    let clusters = scoped["clusters"].as_array().expect("clusters array");
    assert!(
        !clusters.is_empty(),
        "the named workspace must expose the co-occurrence cluster: {scoped}"
    );

    client.cancel().await.expect("client shutdown");
    let _ = server.await;
}

/// Every list result carries the SEP-2549 cache hints, at every protocol version.
///
/// `#[tool_handler]` would emit these only once the negotiated version reaches 2026-07-28. A client
/// that validates them earlier gets a result with both fields absent and rejects the whole
/// response - which does not narrow the surface, it removes it: all thirteen tools vanish behind
/// one "tools fetch failed". This asserts the fields are present and says what they must be, so
/// that dropping the hand-written `list_tools` and letting the macro generate one again is a red
/// test rather than a surface that disappears for whoever upgrades their client first.
///
/// `ttl_ms` is 0 deliberately - see `list_tools` for why a fixed list still refuses to be cached.
#[tokio::test]
async fn every_list_result_carries_cache_hints() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    let tools = client.list_tools(None).await.expect("list tools");
    assert_eq!(
        tools.ttl_ms,
        Some(0),
        "tools/list must declare a freshness window, and it is none"
    );
    assert_eq!(
        tools.cache_scope,
        Some(CacheScope::Private),
        "a response reachable only with this node's bearer token is not shareable across \
         authorization contexts (Principle 17)"
    );

    let resources = client.list_resources(None).await.expect("list resources");
    assert_eq!(
        resources.ttl_ms,
        Some(0),
        "resources/list grows with the workspaces, so a freshness window is a window in which a \
         client is told a workspace does not exist (Principle 5)"
    );
    assert_eq!(resources.cache_scope, Some(CacheScope::Private));

    let templates = client.list_resource_templates(None).await.expect("list resource templates");
    assert_eq!(templates.ttl_ms, Some(0));
    assert_eq!(templates.cache_scope, Some(CacheScope::Private));

    client.cancel().await.expect("client shutdown");
    server.await.expect("server task");
}

/// The server tells a client who it is: supragnosis, at the version `supragnosis --version` prints.
/// It used to answer `rmcp 3.5.0` - the library's name and version, because rmcp's
/// `from_build_env()` expands `env!` in rmcp's own crate - so a client log or a bug report named the
/// wrong program (docs/compatibility.md Section 7).
#[tokio::test]
async fn the_server_names_itself_and_its_release() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let info = client.peer_info().expect("the server introduced itself");
    let server_info = info.server_info.as_ref().expect("serverInfo");
    assert_eq!(server_info.name, "supragnosis");
    assert_eq!(server_info.version, env!("CARGO_PKG_VERSION"));
    client.cancel().await.expect("client shutdown");
    server.await.expect("server task");
}

/// The tool surface is the agent's contract, pinned whole: names, descriptions, input schemas and
/// the list's cache hints, as `tools/list` sends them (docs/compatibility.md Section 7).
///
/// A change here is a diff of `tests/fixtures/tools.json` in review. Additive changes - a tool, an
/// optional argument, a description - only update the file. Removing or renaming a tool or an
/// argument, making an optional argument required, or narrowing a type breaks every agent that
/// learned the old shape, and is named under CHANGELOG.md's Breaking changes. To accept a change,
/// rerun with `SUPRAGNOSIS_BLESS=1` and review the file's diff.
#[tokio::test]
async fn the_tool_list_is_the_pinned_contract() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let listed = client.list_tools(None).await.expect("list tools");
    assert!(listed.next_cursor.is_none(), "the whole list arrives in one page");
    let live = serde_json::to_string_pretty(&listed).expect("serialize") + "\n";
    client.cancel().await.expect("client shutdown");
    server.await.expect("server task");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tools.json");
    if std::env::var_os("SUPRAGNOSIS_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("fixtures dir");
        std::fs::write(&path, &live).expect("bless");
        return;
    }
    let pinned = std::fs::read_to_string(&path).unwrap_or_default();
    let (pinned_v, live_v): (Value, Value) = (
        serde_json::from_str(&pinned).unwrap_or(Value::Null),
        serde_json::from_str(&live).expect("live"),
    );
    assert!(
        pinned_v == live_v,
        "tools/list differs from tests/fixtures/tools.json. If the change is intended, rerun with \
         SUPRAGNOSIS_BLESS=1 and review the diff; a removed or renamed tool or argument is a \
         breaking change for every agent (docs/compatibility.md Section 7)"
    );
}

/// Every `resources/read` answer carries the cache hints too, on every URI form.
///
/// The list methods above were made to emit these because a validating client rejects a response
/// that omits them. `read_resource` was left out and broke the same way once a client started
/// validating it: the read returned a schema error naming `ttlMs`/`cacheScope` instead of the
/// resource, so no resource could be read at all. This walks all five URI forms so that adding a
/// sixth without hints is a red test rather than a resource nobody can fetch.
///
/// The observation branch is asserted at `ttl_ms = 0` on purpose - see `read_resource` for why an
/// immutable observation still refuses to be cached (its provenance list and trust tier move).
#[tokio::test]
async fn every_resource_read_carries_cache_hints() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    // Seed one observation so the observation branch has a real id to read back.
    let observed = engine
        .observe(ObserveInput {
            content: "cache hints apply to the observation back-reference as well".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .expect("seed observation");

    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running =
            SupragnosisServer::new(engine).serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    for uri in [
        "supragnosis://workspaces".to_string(),
        "supragnosis://workspace/ws/graph".to_string(),
        "supragnosis://workspace/ws/hypergraph".to_string(),
        "supragnosis://workspace/ws/types".to_string(),
        format!("supragnosis://observation/{}", observed.observation_id),
    ] {
        let read = client
            .read_resource(ReadResourceRequestParams::new(uri.clone()))
            .await
            .unwrap_or_else(|e| panic!("read {uri} should succeed: {e}"));
        assert_eq!(
            read.ttl_ms,
            Some(0),
            "resources/read on {uri} must declare a freshness window, and it is none - omitting \
             the field makes a validating client reject the whole response"
        );
        assert_eq!(
            read.cache_scope,
            Some(CacheScope::Private),
            "a response reachable only with this node's bearer token is not shareable across \
             authorization contexts (Principle 17)"
        );
    }

    client.cancel().await.expect("client shutdown");
    server.await.expect("server task");
}

/// A narrowed round names the hosts it skipped, and skips only the one that refused.
///
/// This is F21 clause 5 and it needs no transport: the routing decision comes from the negotiated
/// map, which is ordinary state, so a context carrying two hosts - one that admits the workspace and
/// one that answered that it does not - exercises the response shape deterministically. The admitted
/// host is pointed at a closed port, so its own entry is an error; the assertion is about `skipped`,
/// which is the half that turns a silent narrowing into a legible one (negotiated-surface.md N5).
#[tokio::test]
async fn a_narrowed_round_names_the_hosts_it_skipped() {
    use supragnosis_core::NodeIdentity;
    use supragnosis_sync::{NegotiatedSurface, NegotiatedSurfaces, ServerLink, SyncNode};

    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let admits = "http://127.0.0.1:1".to_string(); // closed port: consulted, then fails fast
    let refuses = "http://127.0.0.1:2".to_string();

    let surfaces: NegotiatedSurfaces = Default::default();
    {
        let mut m = surfaces.write().expect("lock");
        m.insert(
            admits.clone(),
            NegotiatedSurface { admits: Some(vec!["ws".into()]), negotiated_at: Some(1) },
        );
        m.insert(
            refuses.clone(),
            NegotiatedSurface { admits: Some(vec!["other".into()]), negotiated_at: Some(1) },
        );
    }
    let sync = Arc::new(supragnosis_mcp::SyncContext {
        node: Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([7u8; 32]))),
        share_workspaces: vec!["ws".into()],
        serve_workspaces: Vec::new(),
        config_notes: Vec::new(),
        servers: vec![
            ServerLink { url: admits.clone(), auth_token: "t".into(), ..Default::default() },
            ServerLink { url: refuses.clone(), auth_token: "t".into(), ..Default::default() },
        ],
        surfaces,
        origin_keys: Default::default(),
        peer_registry: None,
    });

    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running = SupragnosisServer::new(engine)
            .with_sync(sync)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    for tool in ["sync_push", "sync_pull"] {
        let res = client
            .call_tool(
                CallToolRequestParams::new(tool).with_arguments(args(json!({"workspace": "ws"}))),
            )
            .await
            .unwrap_or_else(|e| panic!("{tool}: {e}"));
        let v = tool_json(&res);
        let skipped: Vec<&str> = v["skipped"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
            .unwrap_or_else(|| panic!("{tool} response carries no `skipped`: {v}"));
        assert_eq!(
            skipped,
            [refuses.as_str()],
            "{tool} must name the refusing host and only it - unknown and admitting hosts are consulted: {v}"
        );
    }

    client.cancel().await.ok();
    server.abort();
}

/// sync-correctness.md Section 9 (D7): a remote search sends its query only for a workspace this
/// node shares. For any other workspace no host is asked - the response says why - where it used to
/// ship the query text to every host regardless of the share list a push already honored.
#[tokio::test]
async fn a_remote_search_does_not_leave_for_an_unshared_workspace() {
    use supragnosis_core::NodeIdentity;
    use supragnosis_sync::{ServerLink, SyncNode};

    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let host = "http://127.0.0.1:1".to_string(); // closed port: an attempt shows up as an error
    let sync = Arc::new(supragnosis_mcp::SyncContext {
        node: Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([7u8; 32]))),
        share_workspaces: vec!["ws".into()],
        serve_workspaces: Vec::new(),
        config_notes: Vec::new(),
        servers: vec![ServerLink {
            url: host.clone(),
            auth_token: "t".into(),
            ..Default::default()
        }],
        surfaces: Default::default(),
        origin_keys: Default::default(),
        peer_registry: None,
    });
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running = SupragnosisServer::new(engine)
            .with_sync(sync)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let search = |ws: &'static str| {
        CallToolRequestParams::new("search_knowledge")
            .with_arguments(args(json!({"query": "plans", "workspace": ws, "scope": "remote"})))
    };

    let v = tool_json(&client.call_tool(search("private")).await.expect("search"));
    let remote = v["remote"].as_array().expect("remote results");
    assert!(remote.iter().all(|r| r.get("server").is_none()), "no host was asked: {v}");
    assert_eq!(remote[0]["skipped_workspace"], "private", "{v}");

    let v = tool_json(&client.call_tool(search("ws")).await.expect("search"));
    let asked = v["remote"].as_array().expect("remote results");
    assert!(
        asked.iter().any(|r| r["server"] == host.as_str()),
        "a shared workspace is asked: {v}"
    );

    client.cancel().await.ok();
    server.abort();
}

/// GIVEN knowledge in the log, WHEN a round is routed on the negotiated map, THEN the log is
/// byte-identical - nothing about the negotiation was recorded as knowledge.
///
/// F21 clause 3. The surface is per-link runtime state, and the reason that matters is Prop B: each
/// peer sees its own grants, so an observation carrying one would be a node-relative value inside a
/// structure that has to converge. `sync_pull` rather than `sync_push` on purpose - push stamps via
/// `backfill` before it exports, which enriches attestations by design (F4), and mixing that in
/// would make the assertion about the wrong act.
#[tokio::test]
async fn routing_on_the_negotiated_map_records_nothing() {
    use supragnosis_core::{AssertionStore, NodeIdentity};
    use supragnosis_sync::{NegotiatedSurface, NegotiatedSurfaces, ServerLink, SyncNode};

    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store.clone(), "test-host", "ws"));

    let surfaces: NegotiatedSurfaces = Default::default();
    {
        let mut m = surfaces.write().expect("lock");
        m.insert(
            "http://127.0.0.1:1".into(),
            NegotiatedSurface { admits: Some(vec!["ws".into()]), negotiated_at: Some(1) },
        );
        m.insert(
            "http://127.0.0.1:2".into(),
            NegotiatedSurface { admits: Some(vec!["other".into()]), negotiated_at: Some(1) },
        );
    }
    let sync = Arc::new(supragnosis_mcp::SyncContext {
        node: Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([8u8; 32]))),
        share_workspaces: vec!["ws".into()],
        serve_workspaces: Vec::new(),
        config_notes: Vec::new(),
        servers: vec![
            ServerLink {
                url: "http://127.0.0.1:1".into(),
                auth_token: "t".into(),
                ..Default::default()
            },
            ServerLink {
                url: "http://127.0.0.1:2".into(),
                auth_token: "t".into(),
                ..Default::default()
            },
        ],
        surfaces,
        origin_keys: Default::default(),
        peer_registry: None,
    });

    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running = SupragnosisServer::new(engine)
            .with_sync(sync)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    client
        .call_tool(CallToolRequestParams::new("observe").with_arguments(args(json!({
            "content": "knowledge that predates the round",
            "workspace": "ws",
            "entities": [{"name": "Alpha", "type": "Concept"}]
        }))))
        .await
        .expect("observe");

    let log_of = |s: &InMemoryStore| {
        let mut v: Vec<(String, String)> = s
            .all_observations(Some("ws"))
            .expect("observations")
            .into_iter()
            .map(|o| (o.id.clone(), o.content.clone()))
            .collect();
        v.sort();
        v
    };
    let before = log_of(store.as_ref());
    assert!(!before.is_empty(), "the case is vacuous without knowledge to leave alone");

    let res = client
        .call_tool(
            CallToolRequestParams::new("sync_pull")
                .with_arguments(args(json!({"workspace": "ws"}))),
        )
        .await
        .expect("sync_pull");
    let v = tool_json(&res);
    assert!(
        !v["skipped"].as_array().expect("skipped").is_empty(),
        "the round must have narrowed: {v}"
    );

    assert_eq!(
        before,
        log_of(store.as_ref()),
        "a routed round appended nothing: the negotiated surface is link-local state, and an \
         observation carrying it would be a node-relative value in a structure that must converge"
    );

    client.cancel().await.ok();
    server.abort();
}

/// A configuration this build worked around is reported where an operator will pass it.
///
/// P24: degrading is allowed, degrading invisibly is not. A startup log scrolls away, so the note
/// rides `sync_status` - the surface someone checks when asking why nothing is syncing. A degrade
/// whose only trace was one line at boot has become a silent one a week later, which is what P5
/// forbids.
#[tokio::test]
async fn a_configuration_workaround_reaches_the_operator_surface() {
    use supragnosis_core::NodeIdentity;
    use supragnosis_sync::SyncNode;

    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let note = "[sync] lists 1 server(s) but no auth_token - federation is OFF".to_string();
    let sync = Arc::new(supragnosis_mcp::SyncContext {
        node: Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([4u8; 32]))),
        share_workspaces: vec!["ws".into()],
        serve_workspaces: Vec::new(),
        config_notes: vec![note.clone()],
        servers: Vec::new(),
        surfaces: Default::default(),
        origin_keys: Default::default(),
        peer_registry: None,
    });

    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running = SupragnosisServer::new(engine)
            .with_sync(sync)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    let res = client
        .call_tool(CallToolRequestParams::new("sync_status").with_arguments(args(json!({}))))
        .await
        .expect("sync_status");
    let v = tool_json(&res);
    let notes: Vec<&str> = v["config_notes"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
        .unwrap_or_else(|| panic!("sync_status carries no `config_notes`: {v}"));
    assert_eq!(notes, [note.as_str()], "the workaround must be readable here, not only at boot");

    client.cancel().await.ok();
    server.abort();
}

/// docs/remote-server.md R3: every tool the server declares has a remote policy, and the policy
/// table names nothing the server does not declare. A tool added without deciding what principals
/// may do with it fails here - it never arrives open on the hub's listener.
#[test]
fn every_tool_has_a_remote_policy() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    let declared: BTreeSet<String> =
        SupragnosisServer::new(engine).tool_names().into_iter().collect();
    let classified: BTreeSet<String> =
        supragnosis_mcp::remote::POLICY.iter().map(|(n, _)| n.to_string()).collect();
    assert_eq!(declared, classified, "the remote policy table and the tool list disagree");
}

// --- Prompts (docs/prompts.md) ------------------------------------------------------------------

async fn prompt_server(
    engine: Arc<Engine>,
    remote: bool,
) -> (rmcp::service::RunningService<rmcp::RoleClient, ()>, tokio::task::JoinHandle<()>) {
    let (server_io, client_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let mut s = SupragnosisServer::new(engine);
        if remote {
            s = s.with_remote(supragnosis_mcp::remote::Surface {
                servable: Arc::new(|_: &str| Ok(())),
            });
        }
        let running = s.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    (().serve(client_io).await.expect("client handshake"), server)
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &str,
    v: Value,
) -> Value {
    let res = client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(args(v)))
        .await
        .unwrap_or_else(|e| panic!("{tool}: {e}"));
    tool_json(&res)
}

fn prompt_text(r: &rmcp::model::GetPromptResult) -> String {
    r.messages
        .iter()
        .filter_map(|m| m.content.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The prompt list is a contract like the tool list (docs/prompts.md Section 6): names and
/// arguments pinned whole, with the cache hints a validating client requires. Accepting a change
/// takes SUPRAGNOSIS_BLESS=1 and reaches review as a diff of tests/fixtures/prompts.json.
#[tokio::test]
async fn the_prompt_list_is_the_pinned_contract() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (client, server) = prompt_server(engine, false).await;
    let listed = client.list_prompts(None).await.expect("list prompts");
    assert_eq!(listed.ttl_ms, Some(0));
    assert_eq!(listed.cache_scope, Some(CacheScope::Private));
    let live = serde_json::to_string_pretty(&listed).expect("serialize") + "\n";
    client.cancel().await.ok();
    server.abort();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prompts.json");
    if std::env::var_os("SUPRAGNOSIS_BLESS").is_some() {
        std::fs::write(&path, &live).expect("bless");
        return;
    }
    let pinned: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
        .unwrap_or(Value::Null);
    let live: Value = serde_json::from_str(&live).expect("live");
    assert!(
        pinned == live,
        "prompts/list differs from tests/fixtures/prompts.json. If intended, rerun with \
         SUPRAGNOSIS_BLESS=1 and review the diff; a renamed prompt or argument is breaking"
    );
}

/// `brief` (PR1-PR5): the digest carries what is contested and what waits on a decision, fenced as
/// untrusted evidence; a section past its cap says how much it left out; the instruction never
/// asks for a merge; and getting the prompt writes nothing.
#[tokio::test]
async fn a_brief_is_fenced_bounded_and_writes_nothing() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store.clone(), "test-host", "ws"));
    let (client, server) = prompt_server(engine, false).await;

    // Two tier-tied readings of one entity's type: contested.
    for kind in ["Technology", "Database"] {
        call(
            &client,
            "observe",
            json!({"workspace": "ws", "content": format!("redb is a {kind}"),
            "entities": [{"name": "redb", "type": kind}]}),
        )
        .await;
    }
    // Twenty single-source agent claims: more weak claims than a section shows.
    for i in 0..20 {
        call(
            &client,
            "observe",
            json!({"workspace": "ws", "content": format!("fact {i}"),
            "entities": [{"name": format!("thing {i}"), "type": "Concept"}]}),
        )
        .await;
    }
    call(
        &client,
        "observe",
        json!({"workspace": "ws", "content": "redb-db is redb, spelled otherwise",
        "entities": [{"name": "redb-db", "type": "Technology"}]}),
    )
    .await;
    // Two entities in one observation: a co-occurrence cluster, which the brief reports as a theme.
    call(
        &client,
        "observe",
        json!({"workspace": "ws", "content": "redb and sled are both embedded stores",
        "entities": [{"name": "redb", "type": "Technology"}, {"name": "sled", "type": "Technology"}]}),
    )
    .await;
    let proposed = call(
        &client,
        "propose",
        json!({"workspace": "ws", "kind": "entity_merge",
        "targets": ["redb", "redb-db"], "into": "redb", "rationale": "one store, two spellings"}),
    )
    .await;
    let proposal = proposed["proposal_id"]
        .as_str()
        .or(proposed["id"].as_str())
        .unwrap_or_default()
        .to_string();

    // An agent's comment on it: a proposal event cast from the agent surface.
    call(
        &client,
        "review",
        json!({"workspace": "ws", "proposal": proposal, "decision": "comment",
        "note": "same store, two spellings"}),
    )
    .await;

    use supragnosis_core::AssertionStore;
    let before = store.all_observations(None).unwrap().len();
    let brief = client
        .get_prompt(
            rmcp::model::GetPromptRequestParams::new("brief")
                .with_arguments(args(json!({"workspace": "ws"}))),
        )
        .await
        .expect("brief");
    assert_eq!(
        store.all_observations(None).unwrap().len(),
        before,
        "PR1: a prompt writes nothing"
    );

    let text = prompt_text(&brief);
    let fenced = text
        .split("<supragnosis-evidence untrusted=\"true\">")
        .nth(1)
        .expect("PR4 fence");
    assert!(
        fenced.contains("\"contested\"") && fenced.contains("redb"),
        "contested in the digest"
    );
    assert!(fenced.contains("Database") && fenced.contains("Technology"), "both sides kept");
    assert!(
        !proposal.is_empty() && fenced.contains(&proposal),
        "the open proposal: {proposed}"
    );
    assert!(fenced.contains("more not shown"), "PR3: a capped section says what it left out");
    // P2: a theme is cited by its hyperedge id and a member by name AND id - the digest carries
    // the dereference path, not only a readable name.
    let digest: Value =
        serde_json::from_str(fenced.split("</supragnosis-evidence>").next().unwrap().trim())
            .expect("the digest is JSON");
    let theme = &digest["themes"][0];
    assert_eq!(theme["hyperedge"].as_str().map(str::len), Some(64), "theme id: {theme}");
    let member = &theme["members"][0];
    assert!(member["name"].as_str().is_some_and(|n| !n.is_empty()), "member name: {theme}");
    assert_eq!(member["id"].as_str().map(str::len), Some(64), "member entity id: {theme}");
    // What arrived last is two sections (prompts.md Section 4): a proposal event is never listed
    // as a knowledge row, and it is listed as an event with its kind, its targets by name, and the
    // surface it was cast from.
    let recent = digest["recent_observations"].as_array().expect("recent_observations");
    assert!(
        recent
            .iter()
            .all(|o| !o["text"].as_str().unwrap_or("").starts_with("proposal(")),
        "a proposal event listed as knowledge: {recent:?}"
    );
    let events = digest["recent_proposal_events"].as_array().expect("recent_proposal_events");
    let opened = events
        .iter()
        .find(|e| e["event"] == "opened" && e["proposal"] == proposal.as_str())
        .unwrap_or_else(|| panic!("the open proposal is an event: {events:?}"));
    assert_eq!(opened["kind"], "entity_merge", "{opened}");
    assert!(
        opened["targets"]
            .as_array()
            .is_some_and(|t| t.iter().any(|x| x["name"] == "redb")),
        "targets carry names: {opened}"
    );
    assert!(
        events.iter().any(|e| e["event"] == "comment" && e["surface"] == "agent"),
        "the agent's comment names its surface: {events:?}"
    );
    let instruction = text.split("<supragnosis-evidence untrusted").next().unwrap();
    assert!(instruction.contains("Never cast a merge verdict"), "PR5");
    assert!(instruction.contains("never as") && instruction.contains("instructions"), "PR4");

    client.cancel().await.ok();
    server.abort();
}

/// The other three prompts answer from their own digests, and refuse arguments they cannot use.
#[tokio::test]
async fn the_topic_curate_and_review_prompts_answer_or_refuse() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (client, server) = prompt_server(engine, false).await;
    call(
        &client,
        "observe",
        json!({"workspace": "ws", "content": "the hub serves federation",
        "entities": [{"name": "hub", "type": "Concept"}]}),
    )
    .await;
    call(
        &client,
        "observe",
        json!({"workspace": "ws", "content": "the hubs serve federation",
        "entities": [{"name": "hubs", "type": "Concept"}]}),
    )
    .await;
    let get = |name: &'static str, a: Value| {
        let client = &client;
        async move {
            client
                .get_prompt(rmcp::model::GetPromptRequestParams::new(name).with_arguments(args(a)))
                .await
        }
    };

    let topic = get("what-do-we-know-about", json!({"topic": "federation"}))
        .await
        .expect("topic");
    assert!(prompt_text(&topic).contains("\"hits\""), "the search hits are the digest");
    assert!(get("what-do-we-know-about", json!({})).await.is_err(), "topic is required");

    let curate = get("curate", json!({"workspace": "ws"})).await.expect("curate");
    assert!(prompt_text(&curate).contains("\"contradictions\""), "the curation report");
    assert!(prompt_text(&curate).contains("Never propose recall"));

    let proposed = call(
        &client,
        "propose",
        json!({"workspace": "ws", "kind": "entity_merge",
        "targets": ["hub", "hubs"], "into": "hub"}),
    )
    .await;
    let id = proposed["proposal_id"].as_str().or(proposed["id"].as_str()).unwrap_or_default();
    let review = get("review-proposal", json!({"id": id})).await.expect("review");
    assert!(prompt_text(&review).contains("belief_diff"), "the diff is the review artifact");
    assert!(get("review-proposal", json!({"id": "no-such"})).await.is_err(), "unknown id");
    assert!(get("no-such-prompt", json!({})).await.is_err(), "unknown prompt");

    client.cancel().await.ok();
    server.abort();
}

/// PR6: a hub's principals get no prompts until a digest is scoped to their grants - an empty list,
/// and a refusal that names why.
#[tokio::test]
async fn the_remote_surface_lists_no_prompt_and_refuses_each() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (client, server) = prompt_server(engine, true).await;
    assert!(client.list_prompts(None).await.expect("list").prompts.is_empty());
    let refused = client
        .get_prompt(rmcp::model::GetPromptRequestParams::new("brief"))
        .await
        .expect_err("refused remotely");
    assert!(refused.to_string().contains("remote surface"), "{refused}");
    client.cancel().await.ok();
    server.abort();
}

/// PR2: a digest is a deterministic function of the node's state and the arguments - each of the
/// four prompts, asked twice over one store, is the same bytes. The digests are assembled from
/// reads the P16 rows already pin, through sorted lists and ordered maps; this is the case that
/// would catch a hash map, a clock or a random source reaching one.
#[tokio::test]
async fn a_digest_is_the_same_bytes_twice_over_one_store() {
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws"));
    let (client, server) = prompt_server(engine, false).await;
    for (content, names) in [
        ("redb and sled are embedded stores", vec!["redb", "sled"]),
        ("sled is a store", vec!["sled"]),
        ("the hub serves federation", vec!["hub"]),
    ] {
        let entities: Vec<Value> =
            names.iter().map(|n| json!({"name": n, "type": "Concept"})).collect();
        call(
            &client,
            "observe",
            json!({"workspace": "ws", "content": content, "entities": entities}),
        )
        .await;
    }
    let proposed = call(
        &client,
        "propose",
        json!({"workspace": "ws", "kind": "entity_merge",
        "targets": ["redb", "sled"], "into": "redb", "rationale": "one store, two names"}),
    )
    .await;
    let id = proposed["proposal_id"]
        .as_str()
        .or(proposed["id"].as_str())
        .unwrap_or_default()
        .to_string();
    let asks = [
        ("brief", json!({"workspace": "ws"})),
        ("what-do-we-know-about", json!({"topic": "store"})),
        ("curate", json!({"workspace": "ws"})),
        ("review-proposal", json!({"id": id})),
    ];
    for (name, a) in asks {
        let get = |a: Value| {
            let client = &client;
            async move {
                client
                    .get_prompt(
                        rmcp::model::GetPromptRequestParams::new(name).with_arguments(args(a)),
                    )
                    .await
                    .expect(name)
            }
        };
        let first = prompt_text(&get(a.clone()).await);
        let second = prompt_text(&get(a).await);
        assert_eq!(first, second, "PR2: {name} is not the same bytes twice over one store");
    }
    client.cancel().await.ok();
    server.abort();
}
