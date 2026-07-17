//! LLM eval: 실제 모델이 MCP 도구 표면을 잘 쓰는지 채점한다.
//!
//! 살아있는 `SupragnosisServer` 에서 도구 스키마를 프로토콜로 뽑아(= 실제 표면) 그대로
//! Anthropic Messages API 의 tools 로 넘기고, 자연어 시나리오마다 모델이 올바른 도구를
//! 올바른 인자로 부르는지 검증한다. "표면이 LLM 에게 읽히는가"(원칙 21)의 경험적 측정.
//!
//! 비결정적(모델 호출)이고 네트워크+크레덴셜이 필요하므로 기본 실행에서 제외한다.
//! 실행:
//!   ANTHROPIC_API_KEY=... cargo test -p supragnosis-mcp --test llm_eval -- --ignored --nocapture
//! 선택 env:
//!   SUPRAGNOSIS_EVAL_MODEL (기본 claude-haiku-4-5-20251001)
//!
//! ANTHROPIC_API_KEY 가 없으면 조용히 통과(skip)한다 - CI 를 깨지 않기 위해서다.

use std::sync::Arc;

use rmcp::ServiceExt;
use serde_json::{json, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::InMemoryStore;

const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// 한 시나리오: 자연어 요청 -> 기대 도구 + 인자 술어.
struct Scenario {
    name: &'static str,
    /// 사용자 턴(자연어). 모델은 이걸 보고 도구를 골라야 한다.
    user: &'static str,
    /// 첫 tool_use 가 이 도구여야 한다.
    expect_tool: &'static str,
    /// 도구 인자에 대한 술어. Ok(()) 면 통과, Err(reason) 면 실패.
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

/// 응답 content 에서 첫 tool_use 블록의 (name, input) 을 꺼낸다.
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
#[ignore = "실제 모델 호출 - 네트워크 + ANTHROPIC_API_KEY 필요(수동 eval)"]
async fn model_uses_mcp_tools_correctly() {
    let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") else {
        eprintln!("[skip] ANTHROPIC_API_KEY 미설정 - LLM eval 을 건너뛴다");
        return;
    };
    let base = std::env::var("ANTHROPIC_BASE_URL")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    let model = std::env::var("SUPRAGNOSIS_EVAL_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    // 살아있는 MCP 서버에서 도구 스키마를 프로토콜로 뽑는다(= 모델이 볼 실제 표면).
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
            // 도구 사용을 유도하되 어떤 도구인지는 모델이 고르게 둔다.
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
            "[{}] API 오류 {status}: {payload}",
            sc.name
        );

        match first_tool_use(&payload) {
            Some((name, input)) if name == sc.expect_tool => match (sc.check_args)(&input) {
                Ok(()) => {
                    passed += 1;
                    eprintln!("[pass] {} -> {name}({input})", sc.name);
                }
                Err(why) => failures.push(format!("[{}] 인자 불일치: {why}", sc.name)),
            },
            Some((name, input)) => failures.push(format!(
                "[{}] 도구 선택 오류: 기대 {}, 실제 {name}({input})",
                sc.name, sc.expect_tool
            )),
            None => failures.push(format!("[{}] tool_use 없음: {payload}", sc.name)),
        }
    }

    let _ = client.cancel().await;
    let _ = server.await;

    eprintln!("eval: {passed}/{} 통과", cases.len());
    assert!(
        failures.is_empty(),
        "LLM 이 MCP 도구를 잘못 사용한 시나리오:\n{}",
        failures.join("\n")
    );
}
