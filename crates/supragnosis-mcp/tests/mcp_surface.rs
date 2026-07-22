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
    CallToolRequestParams, CallToolResult, ReadResourceRequestParams, ResourceContents,
};
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
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
fn args(v: Value) -> Option<Map<String, Value>> {
    v.as_object().cloned()
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
        let running = SupragnosisServer::new(engine)
            .serve(server_io)
            .await
            .expect("server handshake");
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
        ]),
        "must expose the intent-level tools (workspace_map = orientation, define_type = T-Box, propose/review/list_proposals/get_proposal = the canon gate)"
    );
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "observe".into(),
            arguments: args(json!({
                "content": "supragnosis is a rust knowledge server built on rmcp",
                "workspace": "ws",
                "entities": [
                    {"name": "supragnosis", "type": "Project"},
                    {"name": "rmcp", "type": "Tool"}
                ],
                "relations": [
                    {"from": "supragnosis", "type": "depends_on", "to": "rmcp"}
                ]
            })),
            task: None,
        })
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "search_knowledge".into(),
            arguments: args(json!({"query": "rust", "workspace": "ws"})),
            task: None,
        })
        .await
        .expect("search call");
    let found = tool_json(&res);
    assert!(
        found["hits"].as_array().is_some_and(|a| !a.is_empty()),
        "search must find the ingested knowledge: {found}"
    );
    // The response reports the surface used (mode) (Principle 16, 4th revision) - this assembly has an embedder, so hybrid.
    assert_eq!(
        found["mode"].as_str(),
        Some("hybrid"),
        "the mode must be reported: {found}"
    );

    // --- 3b) empty search result: accompanied by an open-world note (Principle 5) ---
    // Hybrid returns the nearest neighbors with no similarity threshold, so we produce zero hits with an
    // empty-workspace scope (like the pre-sync partial knowledge of a distributed node).
    let res = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "search_knowledge".into(),
            arguments: args(json!({"query": "anything", "workspace": "empty-ws"})),
            task: None,
        })
        .await
        .expect("empty search call");
    let empty = tool_json(&res);
    assert!(
        empty["hits"].as_array().is_some_and(Vec::is_empty),
        "query that must yield zero hits: {empty}"
    );
    assert!(
        empty["note"]
            .as_str()
            .is_some_and(|n| n.contains("not a negation")),
        "an empty result must carry an absence!=negation note (to prevent LLM misreading): {empty}"
    );

    // --- 4) get_entity: re-query by the id observe returned (relations included) ---
    let res = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "get_entity".into(),
            arguments: args(json!({"id": supragnosis_id})),
            task: None,
        })
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "traverse".into(),
            arguments: args(json!({"id": supragnosis_id})),
            task: None,
        })
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "traverse".into(),
            arguments: args(json!({"id": "does-not-exist"})),
            task: None,
        })
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "get_entity".into(),
            arguments: args(json!({"id": "does-not-exist"})),
            task: None,
        })
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
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "workspace_map".into(),
            arguments: args(json!({"workspace": "ws"})),
            task: None,
        })
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
        let running = SupragnosisServer::new(engine)
            .serve(server_io)
            .await
            .expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    // Ingest knowledge: supragnosis --depends_on--> rmcp (2 nodes, 1 edge).
    let observed = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "observe".into(),
            arguments: args(json!({
                "content": "supragnosis depends on rmcp",
                "workspace": "ws",
                "on_behalf_of": "ashon",
                "entities": [
                    {"name": "supragnosis", "type": "Project"},
                    {"name": "rmcp", "type": "Tool"}
                ],
                "relations": [{"from": "supragnosis", "type": "depends_on", "to": "rmcp"}]
            })),
            task: None,
        })
        .await
        .expect("observe call");
    let observation_id = tool_json(&observed)["observation_id"]
        .as_str()
        .expect("observation id")
        .to_string();

    // --- 1) list_resources: expose the default workspace graph + workspace list resources ----
    let resources = client.list_all_resources().await.expect("list resources");
    let uris: Vec<&str> = resources.iter().map(|r| r.raw.uri.as_str()).collect();
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
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "supragnosis://workspaces".into(),
        })
        .await
        .expect("read workspaces resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let workspaces: Value = serde_json::from_str(&text).expect("workspaces resource is JSON");
    assert!(
        workspaces
            .as_array()
            .is_some_and(|a| a.iter().any(|w| w == "ws")),
        "the workspace list must contain the ingested 'ws': {workspaces}"
    );

    // --- 2) list_resource_templates: templates for querying any workspace ------------------
    let templates = client
        .list_all_resource_templates()
        .await
        .expect("list templates");
    assert!(
        templates
            .iter()
            .any(|t| t.raw.uri_template == "supragnosis://workspace/{workspace}/graph"),
        "must expose the graph resource template"
    );
    assert!(
        templates
            .iter()
            .any(|t| t.raw.uri_template == "supragnosis://workspace/{workspace}/hypergraph"),
        "must also expose the hypergraph resource template"
    );

    // --- 3) read_resource: receive node-link graph JSON and confirm the ingested knowledge -------------
    let read = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "supragnosis://workspace/ws/graph".into(),
        })
        .await
        .expect("read graph resource");
    let text = match read.contents.first().expect("one content") {
        ResourceContents::TextResourceContents { text, .. } => text.clone(),
        other => panic!("expected text resource contents, got {other:?}"),
    };
    let graph: Value = serde_json::from_str(&text).expect("graph resource is JSON");
    assert_eq!(
        graph["stats"]["node_count"].as_u64(),
        Some(2),
        "2 nodes in the graph: {graph}"
    );
    assert_eq!(
        graph["stats"]["edge_count"].as_u64(),
        Some(1),
        "1 edge in the graph: {graph}"
    );
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
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "supragnosis://workspace/ws/hypergraph".into(),
        })
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
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: format!("supragnosis://observation/{observation_id}"),
        })
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
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "supragnosis://observation/does-not-exist".into(),
        })
        .await;
    assert!(missing.is_err(), "an unknown observation id must be a not_found error");

    // --- 6) unknown URI: absence surfaces as an error (with a Principle 5 self-correction hint) ------------------
    let bad = client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: "supragnosis://nope".into(),
        })
        .await;
    assert!(bad.is_err(), "an unknown resource URI must be an error");

    client.cancel().await.expect("client shutdown");
    let _ = server.await;
}
