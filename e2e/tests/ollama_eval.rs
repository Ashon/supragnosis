//! Ollama eval: 로컬 경량 모델이 supragnosis MCP 도구 표면을 잘 쓰는지 채점한다.
//!
//! llm_eval.rs(Anthropic)의 Ollama 판. 살아있는 `SupragnosisServer` 에서 도구 스키마를
//! 프로토콜로 뽑아(= 실제 표면) Ollama 의 OpenAI 호환 tool-calling API 로 넘기고,
//! 자연어 시나리오마다 (1) 올바른 도구를 올바른 인자로 부르는지, (2) 도구를 MCP 로 실제
//! 실행해 결과를 되먹였을 때 observe -> search 지식 흐름이 도는지를 채점한다.
//!
//! Ollama 는 MCP 클라이언트가 아니라 추론 서버다. 그래서 이 하네스가 브리지 역할을 한다:
//! MCP 도구 스키마 -> Ollama tools 포맷 변환 + 모델의 tool_calls -> MCP call_tool 실행.
//!
//! 비결정적(모델)이고 로컬 Ollama 가 필요하므로 기본 실행에서 제외한다.
//! 실행:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test ollama_eval -- --ignored --nocapture
//! 선택 env:
//!   OLLAMA_BASE_URL (기본 http://localhost:11434)
//!   OLLAMA_MODELS   (콤마 구분, 기본 gemma4) - 각 모델을 같은 시나리오로 채점해 비교표를 낸다.
//!
//! Ollama 가 안 떠 있으면 조용히 통과(skip)한다 - CI 를 깨지 않기 위해서다.

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

/// 단일턴 시나리오: 자연어 요청 -> 기대 도구 + 인자 술어.
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

/// 한 모델의 채점 결과.
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

/// 한 모델을 모든 시나리오로 채점한다(단일턴 4 + observe->search 에이전트 루프 1).
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

    // --- 단일턴: 올바른 도구 + 인자 선택 ---
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
                    eprintln!("  [fail] {}: 인자 - {why}", sc.name);
                    card.failed.push(format!("{}: args - {why}", sc.name));
                }
            },
            Some((_, name, args)) => {
                eprintln!("  [fail] {}: 기대 {}, 실제 {name}({args})", sc.name, sc.expect_tool);
                card.failed
                    .push(format!("{}: got {name} want {}", sc.name, sc.expect_tool));
            }
            None => {
                eprintln!("  [fail] {}: tool_call 없음 (텍스트로만 답)", sc.name);
                card.failed.push(format!("{}: no tool_call", sc.name));
            }
        }
    }

    // --- 에이전트 루프: 사실을 observe 로 적재 -> 실행 -> search 로 회상 -> 실행 -> 결과 확인 ---
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

/// observe -> (실행) -> search -> (실행) 왕복. 적재한 사실이 검색으로 되돌아오면 통과.
async fn agent_loop(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    client: &RunningService<RoleClient, ()>,
    tools: &Value,
) -> Result<(), String> {
    let mut messages = json!([{
        "role": "user",
        "content": "Save this fact to the knowledge base: the project uses CozoDB as its \
                    embedded storage engine.",
    }]);

    // 1) 모델이 observe 를 부르길 기대 -> 실제 실행.
    let (msg1, _) = chat(http, base, model, &messages, Some(tools)).await?;
    let calls1 = tool_calls(&msg1);
    let observe = calls1
        .iter()
        .find(|(_, n, _)| n == "observe")
        .ok_or("1턴에서 observe 를 부르지 않음")?;
    push_message(&mut messages, msg1.clone());
    let obs_result = exec_tool(client, "observe", &observe.2).await;
    push_tool_result(&mut messages, &observe.0, &obs_result);

    // 2) 이제 저장한 걸 검색하게 한다 -> search_knowledge 기대 -> 실제 실행.
    push_message(
        &mut messages,
        json!({
            "role": "user",
            "content": "Now search the knowledge base to find which database the project uses.",
        }),
    );
    let (msg2, _) = chat(http, base, model, &messages, Some(tools)).await?;
    let calls2 = tool_calls(&msg2);
    let search = calls2
        .iter()
        .find(|(_, n, _)| n == "search_knowledge")
        .ok_or("2턴에서 search_knowledge 를 부르지 않음")?;
    let search_result = exec_tool(client, "search_knowledge", &search.2).await;

    // 3) 적재한 사실(CozoDB/cozo)이 검색 결과에 돌아왔는가 = 지식 흐름이 실제로 돌았는가.
    if search_result.to_lowercase().contains("cozo") {
        Ok(())
    } else {
        Err(format!("검색 결과에 적재한 사실이 없음: {search_result}"))
    }
}

#[tokio::test]
#[ignore = "로컬 Ollama 필요 - 경량 모델 MCP 도구 사용 수동 eval"]
async fn ollama_models_use_mcp_tools() {
    let base = std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let models_env = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| DEFAULT_MODELS.to_string());
    let models: Vec<&str> = models_env.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] Ollama 에 연결 불가({base}) - `ollama serve` 후 재실행");
        return;
    }

    // 살아있는 MCP 서버(비영속 + 결정적 임베더)를 in-process duplex 로 띄운다.
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "ollama-eval", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );
    let (client, server) = serve_engine(engine).await;
    let tools = openai_tools(&client).await;

    let mut cards: Vec<Scorecard> = Vec::new();
    for model in &models {
        eprintln!("\n=== 모델: {model} ===");
        cards.push(eval_model(&http, &base, model, &client, &tools).await);
    }

    // 비교표.
    eprintln!("\n=== 비교 (도구 사용 정확도) ===");
    for c in &cards {
        eprintln!("  {:<20} {}", c.model, c.score());
        for f in &c.failed {
            eprintln!("      - fail: {f}");
        }
    }

    // 채점표를 리포트 산출물로도 남긴다 - target/eval-reports/index.html 목차에 실린다.
    let mut md = String::from("# ollama tool-use eval 채점표\n\n| 모델 | 점수 |\n|---|---|\n");
    for c in &cards {
        md.push_str(&format!("| {} | {} |\n", c.model, c.score()));
    }
    md.push_str("\n## 시나리오 상세\n\n");
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

    // 검증 목적이 "경량 모델이 MCP 를 잘 쓰는가"의 측정이라, 하나라도 도구 하나를 제대로
    // 부르면 브리지/표면이 동작함을 뜻한다(전 시나리오 통과를 강제하지 않는다 - 모델 품질은
    // 비교표로 드러난다). 아무 모델도 아무 도구를 못 부르면 브리지가 깨진 것이라 실패.
    let any_tool_use = cards.iter().any(|c| !c.passed.is_empty());
    assert!(
        any_tool_use,
        "어떤 모델도 도구를 하나도 제대로 부르지 못함 - MCP<->Ollama 브리지 점검 필요"
    );
}
