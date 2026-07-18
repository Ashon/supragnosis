//! Ollama<->MCP 브리지: 살아있는 `SupragnosisServer` 를 in-process 로 띄우고, 도구
//! 스키마를 OpenAI 호환 tool-calling 포맷으로 변환해 로컬 Ollama 모델에 넘긴 뒤,
//! 모델의 tool_calls 를 실제 MCP call_tool 로 실행한다.
//!
//! Ollama 는 MCP 클라이언트가 아니라 추론 서버라서 이 브리지가 그 사이를 잇는다.
//! (기존에 ollama_eval/delegation_eval/... 각 테스트 파일에 복붙돼 있던 것을 한 벌로 통합.)

use std::sync::Arc;

use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt};
use rmcp::RoleClient;
use serde_json::{json, Map, Value};

use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;

/// 로컬 Ollama 기본 주소.
pub const DEFAULT_BASE: &str = "http://localhost:11434";

/// 한 요청의 토큰 사용량 (Ollama OpenAI 호환 usage 필드).
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt: u64,
    pub completion: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.prompt + self.completion
    }
}

/// 엔진을 실은 MCP 서버를 in-process duplex 로 띄우고 클라이언트를 돌려준다.
/// 서버 태스크는 클라이언트 cancel 시 함께 내려간다.
pub async fn serve_engine(
    engine: Arc<Engine>,
) -> (RunningService<RoleClient, ()>, tokio::task::JoinHandle<()>) {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        if let Ok(running) = SupragnosisServer::new(engine).serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve(client_io).await.expect("client handshake");
    (client, server)
}

/// MCP 도구 목록을 Ollama(OpenAI 호환) tools 배열로 변환한다.
pub async fn openai_tools(client: &RunningService<RoleClient, ()>) -> Value {
    let tools = client.list_all_tools().await.expect("list tools");
    Value::Array(
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description.as_deref().unwrap_or(""),
                        "parameters": &*t.input_schema,
                    }
                })
            })
            .collect(),
    )
}

/// Ollama /v1/chat/completions 호출. (assistant 메시지, 토큰 사용량)을 돌려준다.
/// `tools` 가 None 이면 도구 없는 일반 채팅이다.
pub async fn chat(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    messages: &Value,
    tools: Option<&Value>,
) -> Result<(Value, TokenUsage), String> {
    let mut body = json!({ "model": model, "messages": messages, "stream": false });
    if let Some(t) = tools {
        body["tools"] = t.clone();
        body["tool_choice"] = json!("auto");
    }
    let resp = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let payload: Value = resp.json().await.map_err(|e| format!("bad json: {e}"))?;
    if !status.is_success() {
        return Err(format!("api {status}: {payload}"));
    }
    let msg = payload
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .cloned()
        .ok_or_else(|| format!("no message in response: {payload}"))?;
    let usage = TokenUsage {
        prompt: payload
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        completion: payload
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    };
    Ok((msg, usage))
}

/// assistant 메시지에서 tool_calls 를 (id, name, args) 로 뽑는다.
/// arguments 는 보통 JSON 문자열, 드물게 객체로 오기도 한다 - 둘 다 수용.
pub fn tool_calls(msg: &Value) -> Vec<(String, String, Value)> {
    let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) else {
        return Vec::new();
    };
    calls
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let f = c.get("function")?;
            let name = f.get("name")?.as_str()?.to_string();
            let args = match f.get("arguments") {
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
                Some(v) => v.clone(),
                None => Value::Null,
            };
            let id = c
                .get("id")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("call_{i}"));
            Some((id, name, args))
        })
        .collect()
}

/// MCP 도구를 실제 실행해 결과 텍스트를 돌려준다(에이전트 루프의 실행 단계).
pub async fn exec_tool(
    client: &RunningService<RoleClient, ()>,
    name: &str,
    args: &Value,
) -> String {
    let arguments: Option<Map<String, Value>> = args.as_object().cloned();
    match client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: name.to_string().into(),
            arguments,
            task: None,
        })
        .await
    {
        Ok(res) => res
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.clone())
            .unwrap_or_default(),
        Err(e) => format!("{{\"error\":\"tool call failed: {e}\"}}"),
    }
}

/// 메시지 히스토리에 임의 메시지 Value 를 push 한다.
pub fn push_message(messages: &mut Value, msg: Value) {
    if let Some(arr) = messages.as_array_mut() {
        arr.push(msg);
    }
}

/// tool 실행 결과를 tool 역할 메시지로 push 한다.
pub fn push_tool_result(messages: &mut Value, tool_call_id: &str, content: &str) {
    push_message(
        messages,
        json!({ "role": "tool", "tool_call_id": tool_call_id, "content": content }),
    );
}

/// 로컬 Ollama 가 떠 있는지(태그 엔드포인트) 빠르게 확인한다.
pub async fn ollama_reachable(http: &reqwest::Client, base: &str) -> bool {
    http.get(format!("{base}/api/tags"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
