//! Ollama<->MCP bridge: spins up a live `SupragnosisServer` in-process, converts the tool
//! schemas into the OpenAI-compatible tool-calling format and hands them to a local Ollama
//! model, then executes the model's tool_calls as real MCP call_tool invocations.
//!
//! Ollama is an inference server, not an MCP client, so this bridge connects the two.
//! (Consolidates into one copy what used to be pasted into each of the
//! ollama_eval/delegation_eval/... test files.)

use std::sync::Arc;

use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt};
use rmcp::RoleClient;
use serde_json::{json, Map, Value};

use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;

/// Default address of the local Ollama.
pub const DEFAULT_BASE: &str = "http://localhost:11434";

/// Token usage of a single request (Ollama OpenAI-compatible usage field).
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

/// Spins up an MCP server carrying the engine over an in-process duplex and returns the client.
/// The server task goes down together with the client when it is cancelled.
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

/// Converts the MCP tool list into an Ollama (OpenAI-compatible) tools array.
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

/// Calls Ollama /v1/chat/completions. Returns (assistant message, token usage).
/// If `tools` is None this is a plain chat with no tools.
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

/// Extracts tool_calls from the assistant message as (id, name, args).
/// arguments usually arrives as a JSON string, occasionally as an object - accept both.
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

/// Actually executes the MCP tool and returns the result text (the execution step of the agent loop).
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

/// Pushes an arbitrary message Value onto the message history.
pub fn push_message(messages: &mut Value, msg: Value) {
    if let Some(arr) = messages.as_array_mut() {
        arr.push(msg);
    }
}

/// Pushes a tool execution result as a tool-role message.
pub fn push_tool_result(messages: &mut Value, tool_call_id: &str, content: &str) {
    push_message(
        messages,
        json!({ "role": "tool", "tool_call_id": tool_call_id, "content": content }),
    );
}

/// Quickly checks whether the local Ollama is up (the tags endpoint).
pub async fn ollama_reachable(http: &reqwest::Client, base: &str) -> bool {
    http.get(format!("{base}/api/tags"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
