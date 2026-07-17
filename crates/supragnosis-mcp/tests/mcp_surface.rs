//! MCP 표면 통합 테스트 (LLM 없이, 결정적).
//!
//! 실제 rmcp 클라이언트를 in-process duplex 파이프로 `SupragnosisServer` 에 연결해
//! MCP 프로토콜 그대로 구동한다: 핸드셰이크 -> tools/list -> tools/call.
//! LLM 이 실제로 보게 될 표면(도구 이름/설명/JSON 스키마)과 각 도구의 종단 동작을
//! 검증한다. 어떤 LLM 평가(eval)든 이게 통과하는 표면 위에서만 의미가 있다.
//!
//! 이 테스트는 네트워크/모델이 필요 없어 기본 `cargo test` 에 포함된다.

use std::collections::BTreeSet;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::InMemoryStore;

/// 도구가 돌려준 첫 텍스트 컨텐츠를 JSON 으로 파싱한다.
/// (도구는 JSON 문자열을 반환하고 rmcp 가 이를 text content 로 감싼다.)
fn tool_json(res: &CallToolResult) -> Value {
    let text = res
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("tool should return one text content");
    serde_json::from_str(&text).expect("tool text should be valid JSON")
}

/// serde_json 객체 리터럴을 도구 인자(JsonObject)로.
fn args(v: Value) -> Option<Map<String, Value>> {
    v.as_object().cloned()
}

#[tokio::test]
async fn mcp_protocol_surface_end_to_end() {
    // 결정적 임베더를 붙여 하이브리드 검색 경로까지 프로토콜로 태운다(비영속 스토어).
    let engine = Arc::new(
        Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default())),
    );

    // in-process 양방향 파이프로 서버<->클라이언트를 연결한다.
    let (server_io, client_io) = tokio::io::duplex(8 * 1024);
    let server = tokio::spawn(async move {
        let running = SupragnosisServer::new(engine)
            .serve(server_io)
            .await
            .expect("server handshake");
        // 클라이언트가 끝날 때까지 서버를 살려 둔다.
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client handshake");

    // --- 1) tools/list: LLM 이 보게 될 표면 (원칙 21: 좁고 읽기 쉬운 표면) ---------
    let tools = client.list_all_tools().await.expect("list tools");
    let names: BTreeSet<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        BTreeSet::from(["observe", "get_entity", "search_knowledge", "traverse"]),
        "정확히 4개의 의도 단위 도구를 노출해야 한다"
    );
    for t in &tools {
        let desc = t.description.as_deref().unwrap_or("");
        assert!(
            !desc.trim().is_empty(),
            "도구 '{}' 는 LLM 이 읽을 설명이 있어야 한다",
            t.name
        );
        // 각 도구는 입력 JSON 스키마(object + properties)를 노출한다.
        assert_eq!(
            t.input_schema.get("type").and_then(Value::as_str),
            Some("object"),
            "도구 '{}' 의 input_schema 는 object 여야 한다",
            t.name
        );
    }
    // observe 스키마에 핵심 파라미터 content 가 노출되는지.
    let observe = tools.iter().find(|t| t.name == "observe").unwrap();
    let props = observe
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("observe schema has properties");
    assert!(props.contains_key("content"), "observe 는 content 파라미터 노출");

    // --- 2) observe: 지식 적재 (엔티티 2 + 관계 1) --------------------------------
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
        "observe 는 관측 id 를 돌려줘야 한다: {out}"
    );
    let entity_ids = out["entities"].as_array().expect("entities array");
    assert_eq!(entity_ids.len(), 2, "엔티티 2개가 링크돼야 한다: {out}");
    assert_eq!(
        out["relations"].as_array().map(Vec::len),
        Some(1),
        "관계 1개가 링크돼야 한다: {out}"
    );
    let supragnosis_id = entity_ids[0].as_str().unwrap().to_string();

    // --- 3) search_knowledge: 하이브리드 검색으로 적재한 지식을 회상 -------------
    let res = client
        .call_tool(CallToolRequestParams {
            meta: None,
            name: "search_knowledge".into(),
            arguments: args(json!({"query": "rust", "workspace": "ws"})),
            task: None,
        })
        .await
        .expect("search call");
    let hits = tool_json(&res);
    assert!(
        hits.as_array().is_some_and(|a| !a.is_empty()),
        "검색은 적재한 지식을 찾아야 한다: {hits}"
    );

    // --- 4) get_entity: observe 가 돌려준 id 로 재조회 (관계 포함) ----------------
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
        "id 로 엔티티를 되찾아야 한다: {ent}"
    );
    assert_eq!(
        ent["relations"].as_array().map(Vec::len),
        Some(1),
        "엔티티 조회에 관계가 함께 와야 한다: {ent}"
    );

    // --- 5) traverse: supragnosis -> rmcp (depends_on, 1홉) ----------------------
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
        reached
            .as_array()
            .is_some_and(|a| a.iter().any(|h| h["name"] == "rmcp")),
        "순회는 depends_on 이웃 rmcp 에 도달해야 한다: {reached}"
    );

    // --- 6) get_entity(미지 id): 열린 세계 - 에러가 아니라 unknown (원칙 5) -------
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
        "부재는 에러가 아니라 found:false 여야 한다(LLM 오독 방지): {unknown}"
    );

    // 정리: 클라이언트를 종료하면 서버 파이프가 닫히고 서버 태스크가 끝난다.
    client.cancel().await.expect("client shutdown");
    let _ = server.await;
}
