//! Ollama eval: scores whether local lightweight models use the supragnosis MCP tool surface well.
//!
//! The Ollama edition of llm_eval.rs (Anthropic). Pulls the tool schemas from a live
//! `SupragnosisServer` over the protocol (= the real surface), hands them to Ollama's
//! OpenAI-compatible tool-calling API, and for each natural-language scenario scores (1) whether
//! it calls the right tool with the right arguments, and (2) whether, once the tool is actually
//! executed over MCP and its result fed back, the observe -> search knowledge flow turns.
//!
//! Ollama is an inference server, not an MCP client. So this harness acts as the bridge:
//! MCP tool schemas -> Ollama tools format conversion + the model's tool_calls -> MCP call_tool execution.
//!
//! Nondeterministic (model) and requiring a local Ollama, so excluded from the default run.
//! Run:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test ollama_eval -- --ignored --nocapture
//! Optional env:
//!   OLLAMA_BASE_URL (default http://localhost:11434)
//!   OLLAMA_MODELS   (comma-separated, default gemma4) - scores each model on the same scenarios and produces a comparison table.
//!
//! If Ollama is not up, it silently passes (skips) - so as not to break CI.

use std::sync::Arc;
use std::time::Duration;

use rmcp::service::RunningService;
use rmcp::RoleClient;
use serde_json::{json, Value};

use supragnosis_e2e::bridge::{
    chat, exec_tool, ollama_reachable, openai_tools, push_message, push_tool_result,
    serve_engine, tool_calls, DEFAULT_BASE,
};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_store::InMemoryStore;

const DEFAULT_MODELS: &str = "gemma4";

/// Single-turn scenario: natural-language request -> expected tool + argument predicate.
struct Scenario {
    name: &'static str,
    user: &'static str,
    expect_tool: &'static str,
    check_args: fn(&Value) -> Result<(), String>,
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "save_fact -> observe",
            user: "Remember this fact by saving it to the knowledge base: the tokio crate \
                   provides an asynchronous runtime for Rust.",
            expect_tool: "observe",
            check_args: |a| match a.get("content").and_then(Value::as_str) {
                Some(c) if c.to_lowercase().contains("tokio") => Ok(()),
                other => Err(format!("content should mention tokio, got {other:?}")),
            },
        },
        Scenario {
            name: "recall -> search_knowledge",
            user: "Search the knowledge base for what we know about async runtimes.",
            expect_tool: "search_knowledge",
            check_args: |a| match a.get("query").and_then(Value::as_str) {
                Some(q) if !q.trim().is_empty() => Ok(()),
                other => Err(format!("query should be non-empty, got {other:?}")),
            },
        },
        Scenario {
            name: "lookup -> get_entity",
            user: "Look up the entity with id \"ent-xyz-1\" in the knowledge base.",
            expect_tool: "get_entity",
            check_args: |a| match a.get("id").and_then(Value::as_str) {
                Some("ent-xyz-1") => Ok(()),
                other => Err(format!("id should be ent-xyz-1, got {other:?}")),
            },
        },
        Scenario {
            name: "neighbors -> traverse",
            user: "Starting from the entity with id \"ent-abc123\", walk the graph and list \
                   what it connects to.",
            expect_tool: "traverse",
            check_args: |a| match a.get("id").and_then(Value::as_str) {
                Some("ent-abc123") => Ok(()),
                other => Err(format!("id should be ent-abc123, got {other:?}")),
            },
        },
    ]
}

/// Scoring result for a single model.
struct Scorecard {
    model: String,
    reachable: bool,
    passed: Vec<String>,
    failed: Vec<String>,
}

impl Scorecard {
    fn score(&self) -> String {
        if !self.reachable {
            return "unavailable".to_string();
        }
        let total = self.passed.len() + self.failed.len();
        format!("{}/{}", self.passed.len(), total)
    }
}

/// Scores a single model on all scenarios (4 single-turn + 1 observe->search agent loop).
async fn eval_model(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    client: &RunningService<RoleClient, ()>,
    tools: &Value,
) -> Scorecard {
    let mut card = Scorecard {
        model: model.to_string(),
        reachable: true,
        passed: Vec::new(),
        failed: Vec::new(),
    };

    // --- single-turn: correct tool + argument choice ---
    for sc in scenarios() {
        let messages = json!([{ "role": "user", "content": sc.user }]);
        let msg = match chat(http, base, model, &messages, Some(tools)).await {
            Ok((m, _)) => m,
            Err(e) => {
                card.failed.push(format!("{}: {e}", sc.name));
                continue;
            }
        };
        match tool_calls(&msg).into_iter().next() {
            Some((_, name, args)) if name == sc.expect_tool => match (sc.check_args)(&args) {
                Ok(()) => {
                    eprintln!("  [pass] {} -> {name}({args})", sc.name);
                    card.passed.push(sc.name.to_string());
                }
                Err(why) => {
                    eprintln!("  [fail] {}: args - {why}", sc.name);
                    card.failed.push(format!("{}: args - {why}", sc.name));
                }
            },
            Some((_, name, args)) => {
                eprintln!("  [fail] {}: expected {}, got {name}({args})", sc.name, sc.expect_tool);
                card.failed
                    .push(format!("{}: got {name} want {}", sc.name, sc.expect_tool));
            }
            None => {
                eprintln!("  [fail] {}: no tool_call (answered with text only)", sc.name);
                card.failed.push(format!("{}: no tool_call", sc.name));
            }
        }
    }

    // --- agent loop: ingest a fact via observe -> execute -> recall via search -> execute -> check result ---
    let loop_name = "agent-loop: observe -> search";
    match agent_loop(http, base, model, client, tools).await {
        Ok(()) => {
            eprintln!("  [pass] {loop_name}");
            card.passed.push(loop_name.to_string());
        }
        Err(why) => {
            eprintln!("  [fail] {loop_name}: {why}");
            card.failed.push(format!("{loop_name}: {why}"));
        }
    }

    card
}

/// Agent-loop turn prompts (single source for execution/report).
const AGENT_TURN1: &str = "Save this fact to the knowledge base: the project uses CozoDB as \
its embedded storage engine.";
const AGENT_TURN2: &str = "Now search the knowledge base to find which database the project \
uses.";

/// observe -> (execute) -> search -> (execute) round-trip. Passes if the ingested fact comes back via search.
async fn agent_loop(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    client: &RunningService<RoleClient, ()>,
    tools: &Value,
) -> Result<(), String> {
    let mut messages = json!([{ "role": "user", "content": AGENT_TURN1 }]);

    // 1) expect the model to call observe -> actually execute it.
    let (msg1, _) = chat(http, base, model, &messages, Some(tools)).await?;
    let calls1 = tool_calls(&msg1);
    let observe = calls1
        .iter()
        .find(|(_, n, _)| n == "observe")
        .ok_or("did not call observe on turn 1")?;
    push_message(&mut messages, msg1.clone());
    let obs_result = exec_tool(client, "observe", &observe.2).await;
    push_tool_result(&mut messages, &observe.0, &obs_result);

    // 2) now have it search what was saved -> expect search_knowledge -> actually execute it.
    push_message(
        &mut messages,
        json!({ "role": "user", "content": AGENT_TURN2 }),
    );
    let (msg2, _) = chat(http, base, model, &messages, Some(tools)).await?;
    let calls2 = tool_calls(&msg2);
    let search = calls2
        .iter()
        .find(|(_, n, _)| n == "search_knowledge")
        .ok_or("did not call search_knowledge on turn 2")?;
    let search_result = exec_tool(client, "search_knowledge", &search.2).await;

    // 3) did the ingested fact (CozoDB/cozo) come back in the search result = did the knowledge flow actually turn.
    if search_result.to_lowercase().contains("cozo") {
        Ok(())
    } else {
        Err(format!("the ingested fact is absent from the search result: {search_result}"))
    }
}

#[tokio::test]
#[ignore = "requires local Ollama - manual eval of lightweight-model MCP tool use"]
async fn ollama_models_use_mcp_tools() {
    let base = std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let models_env = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| DEFAULT_MODELS.to_string());
    let models: Vec<&str> = models_env.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] cannot reach Ollama ({base}) - rerun after `ollama serve`");
        return;
    }

    // Spin up a live MCP server (non-persistent + deterministic embedder) over an in-process duplex.
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "ollama-eval", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let (client, server) = serve_engine(engine).await;
    let tools = openai_tools(&client).await;

    let mut cards: Vec<Scorecard> = Vec::new();
    for model in &models {
        eprintln!("\n=== model: {model} ===");
        cards.push(eval_model(&http, &base, model, &client, &tools).await);
    }

    // Comparison table.
    eprintln!("\n=== comparison (tool-use accuracy) ===");
    for c in &cards {
        eprintln!("  {:<20} {}", c.model, c.score());
        for f in &c.failed {
            eprintln!("      - fail: {f}");
        }
    }

    // Also leave the scorecard as a report artifact - it appears in the target/eval-reports/index.html table of contents.
    let mut md = String::from("# ollama tool-use eval scorecard\n\n| model | score |\n|---|---|\n");
    for c in &cards {
        md.push_str(&format!("| {} | {} |\n", c.model, c.score()));
    }
    md.push_str("\n## Prompts used (identical for all models)\n\n");
    for sc in scenarios() {
        md.push_str(&format!("- {}: \"{}\"\n", sc.name, sc.user));
    }
    md.push_str(&format!("- agent-loop turn 1: \"{AGENT_TURN1}\"\n"));
    md.push_str(&format!("- agent-loop turn 2: \"{AGENT_TURN2}\"\n"));

    md.push_str("\n## Scenario detail\n\n");
    for c in &cards {
        for p in &c.passed {
            md.push_str(&format!("- [o] {} : {}\n", c.model, p));
        }
        for f in &c.failed {
            md.push_str(&format!("- [x] {} : {}\n", c.model, f));
        }
    }
    let report_path = supragnosis_e2e::report::write_report("ollama_eval.md", &md);
    eprintln!("[report] {}", report_path.display());

    let _ = client.cancel().await;
    let _ = server.await;

    // Since the point of the assertion is to measure "do lightweight models use MCP well", even one
    // model correctly calling one tool means the bridge/surface works (it does not force every
    // scenario to pass - model quality surfaces in the comparison table). If no model can call any
    // tool, the bridge is broken, so fail.
    let any_tool_use = cards.iter().any(|c| !c.passed.is_empty());
    assert!(
        any_tool_use,
        "no model correctly called even one tool - check the MCP<->Ollama bridge"
    );
}
