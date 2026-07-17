//! supragnosis-mcp — MCP 표면(도구).
//!
//! rmcp 매크로로 도구를 정의하고 [`supragnosis_engine::Engine`] 으로 위임한다.
//! M0 도구: `observe`, `get_entity`, `search_knowledge`.

use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    ServerHandler,
};
use serde::{Deserialize, Serialize};

use supragnosis_engine::{
    Engine, EntityInput as EngineEntityInput, ObserveInput, RelationInput as EngineRelationInput,
};

// ── 전송 DTO (JSON Schema 자동 생성) ────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveRequest {
    /// 적재할 지식 조각(자연어 또는 구조화 텍스트).
    pub content: String,
    /// 작업 공간(생략 시 노드 기본값).
    #[serde(default)]
    pub workspace: Option<String>,
    /// 출처 참조(파일 경로/URL/툴 등).
    #[serde(default)]
    pub source_ref: Option<String>,
    /// 신뢰도 0.0~1.0 (생략 시 1.0).
    #[serde(default)]
    pub confidence: Option<f32>,
    /// (선택) 클라이언트가 추출한 엔티티.
    #[serde(default)]
    pub entities: Vec<EntityInput>,
    /// (선택) 클라이언트가 추출한 관계.
    #[serde(default)]
    pub relations: Vec<RelationInput>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityInput {
    /// 엔티티 정규 명칭.
    pub name: String,
    /// 엔티티 타입(예: Concept, Person, Project, Tool). 생략 시 Concept.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RelationInput {
    /// 시작 엔티티의 이름.
    pub from: String,
    /// 관계 타입(예: depends_on, part_of, relates_to).
    #[serde(rename = "type")]
    pub kind: String,
    /// 도착 엔티티의 이름.
    pub to: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetEntityRequest {
    /// 엔티티 id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRequest {
    /// 검색어.
    pub query: String,
    /// 작업 공간으로 범위 제한(생략 시 전체).
    #[serde(default)]
    pub workspace: Option<String>,
    /// 최대 결과 수(생략 시 20).
    #[serde(default)]
    pub limit: Option<usize>,
}

// ── 서버 ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SupragnosisServer {
    engine: Arc<Engine>,
    tool_router: ToolRouter<SupragnosisServer>,
}

impl SupragnosisServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl SupragnosisServer {
    #[tool(
        description = "지식 조각을 적재한다. 불변 관측(진실의 원천)으로 저장하고, 함께 준 엔티티/관계를 온톨로지에 링크한다. 결과로 관측 id와 링크된 엔티티/관계 id를 돌려준다."
    )]
    fn observe(&self, Parameters(req): Parameters<ObserveRequest>) -> String {
        let input = ObserveInput {
            content: req.content,
            workspace: req.workspace,
            source_ref: req.source_ref,
            confidence: req.confidence,
            entities: req
                .entities
                .into_iter()
                .map(|e| EngineEntityInput { name: e.name, kind: e.kind })
                .collect(),
            relations: req
                .relations
                .into_iter()
                .map(|r| EngineRelationInput { from: r.from, kind: r.kind, to: r.to })
                .collect(),
        };
        respond(self.engine.observe(input))
    }

    #[tool(description = "엔티티 id로 엔티티와 그 관계·출처를 조회한다.")]
    fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        match self.engine.get_entity(&req.id) {
            Some(view) => to_json(&view),
            None => err_json("entity not found"),
        }
    }

    #[tool(
        description = "지식(엔티티·관측)을 키워드로 검색한다. M0는 부분문자열 매칭이며, 의미(벡터) 검색은 이후 마일스톤에서 추가된다."
    )]
    fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let hits = self
            .engine
            .search(&req.query, req.workspace.as_deref(), req.limit.unwrap_or(20));
        to_json(&hits)
    }
}

#[tool_handler]
impl ServerHandler for SupragnosisServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "supragnosis: 여러 호스트/워크스페이스의 지식을 온톨로지화하는 MCP 서버. \
                 observe 로 지식을 적재하고 get_entity·search_knowledge 로 탐색한다."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}

// ── 직렬화 헬퍼 (도구는 JSON 문자열을 반환) ─────────────────────────────────

fn respond<T: Serialize, E: std::fmt::Display>(r: Result<T, E>) -> String {
    match r {
        Ok(v) => to_json(&v),
        Err(e) => err_json(&e.to_string()),
    }
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| err_json(&e.to_string()))
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
