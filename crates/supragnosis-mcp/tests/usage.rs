//! docs/usage.md: the MCP surface records every call it answers in the usage log - the function,
//! the workspace it named, what it came to - and nothing of what was asked or answered.

use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams};
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::{SupragnosisServer, UsageLog};
use supragnosis_store::InMemoryStore;

fn args(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("a JSON object")
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("supragnosis-usage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// UL4, UL2, UL1: a tool call, a resource read and a prompt each leave one line with the function
/// and the workspace the call named; an id that names nothing reads `not_found`, a refused call
/// `error`, and an observe from a commit carries its id. No query, content or id reaches the file,
/// and the file is the owner's alone.
#[tokio::test]
async fn every_call_is_recorded_with_its_workspace_and_no_text() {
    let dir = scratch("calls");
    let path = dir.join("usage.jsonl");
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "h", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let server = SupragnosisServer::new(engine).with_usage(Arc::new(UsageLog::at(&path)));
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let running = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let call = |name: &str, a: Value| {
        let p = CallToolRequestParams::new(name.to_string()).with_arguments(args(a));
        client.call_tool(p)
    };

    let observed = call(
        "observe",
        json!({ "workspace": "alpha", "content": "the zebra-quartz rule", "on_behalf_of": "quokka",
                "source_ref": "git:abc1234", "entities": [{"name": "Zebra", "type": "Thing"}] }),
    )
    .await
    .expect("observe answers");
    let text = observed.content[0].as_text().expect("text").text.clone();
    let obs_id = serde_json::from_str::<Value>(&text).unwrap()["observation_id"]
        .as_str()
        .expect("an observation id")
        .to_string();
    call("search_knowledge", json!({ "query": "zebra-quartz", "workspace": "alpha" }))
        .await
        .expect("search answers");
    call("search_knowledge", json!({ "query": "zebra-quartz" }))
        .await
        .expect("search answers");
    call("get_entity", json!({ "id": "ab".repeat(32) }))
        .await
        .expect("get_entity answers");
    call("workspace_map", json!({ "workspace": "nowhere" }))
        .await
        .expect("the map answers");
    let _ = call("no_such_tool", json!({})).await;
    call(
        "observe",
        json!({ "workspace": "alpha", "content": "plain", "on_behalf_of": "quokka",
                "source_ref": "git:not a commit but a sentence" }),
    )
    .await
    .expect("observe answers");
    call("review", json!({ "proposal": "nope", "decision": "reject" }))
        .await
        .expect("review answers");
    client
        .read_resource(ReadResourceRequestParams::new(format!(
            "supragnosis://observation/{obs_id}"
        )))
        .await
        .expect("the observation reads");
    client
        .read_resource(ReadResourceRequestParams::new("supragnosis://workspace/alpha/graph"))
        .await
        .expect("the graph reads");
    let _ = client
        .read_resource(ReadResourceRequestParams::new(format!(
            "supragnosis://entity/{}",
            "cd".repeat(32)
        )))
        .await;
    let mut brief = Map::new();
    brief.insert("workspace".into(), "alpha".into());
    client
        .get_prompt(GetPromptRequestParams::new("brief").with_arguments(brief))
        .await
        .expect("the brief builds");

    let file = std::fs::read_to_string(&path).expect("the usage log was written");
    let lines: Vec<Value> =
        file.lines().map(|l| serde_json::from_str(l).expect("a JSON line")).collect();
    let said: Vec<(String, String, Option<String>, String)> = lines
        .iter()
        .map(|l| {
            (
                l["kind"].as_str().unwrap().to_string(),
                l["name"].as_str().unwrap().to_string(),
                l["workspace"].as_str().map(str::to_string),
                l["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let line = |k: &str, n: &str, w: Option<&str>, o: &str| {
        (k.to_string(), n.to_string(), w.map(str::to_string), o.to_string())
    };
    assert_eq!(
        said,
        vec![
            line("tool", "observe", Some("alpha"), "ok"),
            line("tool", "search_knowledge", Some("alpha"), "ok"),
            line("tool", "search_knowledge", None, "ok"),
            line("tool", "get_entity", None, "not_found"),
            line("tool", "workspace_map", Some("nowhere"), "empty"),
            line("tool", "unknown", None, "error"),
            line("tool", "observe", Some("alpha"), "ok"),
            line("tool", "review", None, "error"),
            line("resource", "observation", None, "ok"),
            line("resource", "graph", Some("alpha"), "ok"),
            line("resource", "entity", None, "not_found"),
            line("prompt", "brief", Some("alpha"), "ok"),
        ],
        "{file}"
    );
    assert_eq!(lines[0]["commit"], "abc1234", "an observe from a commit names it");
    assert!(lines[6].get("commit").is_none(), "a source that is not a commit id is not kept");
    assert!(lines[1]["n"].as_u64() > Some(0) && lines[1]["mode"] == "hybrid", "{}", lines[1]);
    assert!(lines.iter().all(|l| l["t"].is_u64() && l["ms"].is_u64()), "{file}");
    for asked in [
        "zebra",
        "Zebra",
        obs_id.as_str(),
        "quokka",
        &"ab".repeat(32),
        &"cd".repeat(32),
        "no_such_tool",
        "sentence",
    ] {
        assert!(!file.contains(asked), "the usage log holds text it was given");
    }
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the usage log is the owner's alone");

    drop(client);
    let _ = running.await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// UL3: a usage log that cannot be written leaves the call's answer as it was.
#[tokio::test]
async fn a_usage_log_that_cannot_be_written_costs_the_call_nothing() {
    let dir = scratch("unwritable");
    let path = dir.join("no-such-dir").join("usage.jsonl");
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "ws"));
    let server = SupragnosisServer::new(engine).with_usage(Arc::new(UsageLog::at(&path)));
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let running = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");
    for _ in 0..2 {
        let res = client
            .call_tool(CallToolRequestParams::new("observe".to_string()).with_arguments(args(
                json!({ "workspace": "alpha", "content": "still answered", "on_behalf_of": "me" }),
            )))
            .await
            .expect("observe answers");
        assert_ne!(res.is_error, Some(true), "the call failed with the log");
    }
    assert!(!path.exists());
    drop(client);
    let _ = running.await;
    let _ = std::fs::remove_dir_all(&dir);
}
