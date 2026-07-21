//! LLM eval: scores whether a real model uses the MCP tool surface well.
//!
//! Pulls the tool schemas from a live `SupragnosisServer` over the protocol (= the real surface),
//! hands them as-is to the Anthropic Messages API's tools, and, for each natural-language
//! scenario, verifies that the model calls the right tool with the right arguments. An empirical
//! measurement of "is the surface legible to the LLM" (Principle 21).
//!
//! Nondeterministic (model calls) and requiring network + credentials, so excluded from the default run.
//! Run:
//!   ANTHROPIC_API_KEY=... cargo test -p supragnosis-e2e --test llm_eval -- --ignored --nocapture
//! Optional env:
//!   SUPRAGNOSIS_EVAL_MODEL (default claude-haiku-4-5-20251001)
//!
//! If ANTHROPIC_API_KEY is unset, it silently passes (skips) - so as not to break CI.

use std::sync::Arc;

use rmcp::ServiceExt;
use serde_json::{json, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::InMemoryStore;

const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// A single scenario: natural-language request -> expected tool + argument predicate.
struct Scenario {
    name: &'static str,
    /// The user turn (natural language). The model must choose a tool from this.
    user: &'static str,
    /// The first tool_use must be this tool.
    expect_tool: &'static str,
    /// Predicate over the tool arguments. Ok(()) passes, Err(reason) fails.
    check_args: fn(&Value) -> Result<(), String>,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "save_fact -> observe",
            user: "Remember this: the tokio crate provides an async runtime for Rust. \
                   Save it to the knowledge base.",
            expect_tool: "observe",
            check_args: |a| match a.get("content").and_then(Value::as_str) {
                Some(c) if c.to_lowercase().contains("tokio") => Ok(()),
                other => Err(format!("content should mention tokio, got {other:?}")),
            },
        },
        Scenario {
            name: "recall -> search_knowledge",
            user: "What do we know about async runtimes? Search the knowledge base.",
            expect_tool: "search_knowledge",
            check_args: |a| match a.get("query").and_then(Value::as_str) {
                Some(q) if !q.trim().is_empty() => Ok(()),
                other => Err(format!("query should be non-empty, got {other:?}")),
            },
        },
        Scenario {
            name: "neighbors -> traverse",
            user: "Starting from the entity with id \"ent-abc123\", walk the graph and \
                   list what it connects to.",
            expect_tool: "traverse",
            check_args: |a| match a.get("id").and_then(Value::as_str) {
                Some("ent-abc123") => Ok(()),
                other => Err(format!("id should be ent-abc123, got {other:?}")),
            },
        },
    ]
}

/// Extracts the (name, input) of the first tool_use block from the response content.
fn first_tool_use(resp: &Value) -> Option<(String, Value)> {
    resp.get("content")?.as_array()?.iter().find_map(|block| {
        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
            let name = block.get("name")?.as_str()?.to_string();
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            Some((name, input))
        } else {
            None
        }
    })
}

#[tokio::test]
#[ignore = "real model call - requires network + ANTHROPIC_API_KEY (manual eval)"]
async fn model_uses_mcp_tools_correctly() {
    let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") else {
        eprintln!("[skip] ANTHROPIC_API_KEY unset - skipping the LLM eval");
        return;
    };
    let base = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    let model = std::env::var("SUPRAGNOSIS_EVAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    // Pull the tool schemas from a live MCP server over the protocol (= the real surface the model will see).
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "eval-host", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        if let Ok(running) = SupragnosisServer::new(engine).serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve(client_io).await.expect("client handshake");
    let mcp_tools = client.list_all_tools().await.expect("list tools");
    let tools = Value::Array(
        mcp_tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description.as_deref().unwrap_or(""),
                    "input_schema": &*t.input_schema,
                })
            })
            .collect(),
    );

    let http = reqwest::Client::new();

    let cases = scenarios();
    let mut passed = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for sc in &cases {
        let body = json!({
            "model": model,
            "max_tokens": 1024,
            "tools": tools,
            // Encourage tool use, but let the model choose which tool.
            "tool_choice": {"type": "any"},
            "messages": [{"role": "user", "content": sc.user}],
        });
        let resp = http
            .post(format!("{base}/v1/messages"))
            .header("x-api-key", &api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .expect("anthropic request");
        let status = resp.status();
        let payload: Value = resp.json().await.expect("anthropic json");
        assert!(
            status.is_success(),
            "[{}] API error {status}: {payload}",
            sc.name
        );

        match first_tool_use(&payload) {
            Some((name, input)) if name == sc.expect_tool => match (sc.check_args)(&input) {
                Ok(()) => {
                    passed += 1;
                    eprintln!("[pass] {} -> {name}({input})", sc.name);
                }
                Err(why) => failures.push(format!("[{}] argument mismatch: {why}", sc.name)),
            },
            Some((name, input)) => failures.push(format!(
                "[{}] wrong tool choice: expected {}, got {name}({input})",
                sc.name, sc.expect_tool
            )),
            None => failures.push(format!("[{}] no tool_use: {payload}", sc.name)),
        }
    }

    let _ = client.cancel().await;
    let _ = server.await;

    eprintln!("eval: {passed}/{} passed", cases.len());
    assert!(
        failures.is_empty(),
        "scenarios where the LLM misused the MCP tools:\n{}",
        failures.join("\n")
    );
}
