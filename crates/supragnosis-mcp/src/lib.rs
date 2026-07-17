//! supragnosis-mcp - MCP 표면(도구).
//!
//! rmcp 매크로로 도구를 정의하고 [`supragnosis_engine::Engine`] 으로 위임한다.
//! 도구: `observe`, `get_entity`, `search_knowledge`, `traverse`.

use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router, ServerHandler,
};
use serde::{Deserialize, Serialize};

use supragnosis_engine::{
    Engine, EntityInput as EngineEntityInput, ObserveInput, RelationInput as EngineRelationInput,
};

// --- 전송 DTO (JSON Schema 자동 생성) ---------------------------------------

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
    /// (선택) 위임 주체 - 이 에이전트가 대리하는 사람/주체 (예: "ashon"). 원칙 2.
    #[serde(default)]
    pub on_behalf_of: Option<String>,
    /// (선택) 이 지식이 파생된 원천 관측 id들 - 오염 추적용 계보. 원칙 18.
    #[serde(default)]
    pub derived_from: Vec<String>,
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TraverseRequest {
    /// 시작 엔티티 id.
    pub id: String,
    /// 최대 홉 수(생략 시 3).
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// 최대 결과 수(생략 시 100).
    #[serde(default)]
    pub limit: Option<usize>,
}

// --- 서버 --------------------------------------------------------------------

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
            on_behalf_of: req.on_behalf_of,
            derived_from: req.derived_from,
            entities: req
                .entities
                .into_iter()
                .map(|e| EngineEntityInput {
                    name: e.name,
                    kind: e.kind,
                })
                .collect(),
            relations: req
                .relations
                .into_iter()
                .map(|r| EngineRelationInput {
                    from: r.from,
                    kind: r.kind,
                    to: r.to,
                })
                .collect(),
        };
        respond(self.engine.observe(input))
    }

    #[tool(description = "엔티티 id로 엔티티와 그 관계/출처를 조회한다.")]
    fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        match self.engine.get_entity(&req.id) {
            Some(view) => to_json(&view),
            // 열린 세계 가정(원칙 5): 부재는 거짓이 아니라 미지(unknown)다.
            // "찾지 못함"을 에러로 주지 않아 LLM 이 부재를 부정으로 오독하지 않게 한다.
            None => serde_json::json!({
                "found": false,
                "id": req.id,
                "note": "unknown - not found is not a negation (open-world assumption)"
            })
            .to_string(),
        }
    }

    #[tool(
        description = "지식(엔티티/관측)을 검색한다. 임베딩이 있으면 의미(벡터)+키워드 하이브리드로, 없으면 키워드 부분일치로 동작한다."
    )]
    fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let hits = self.engine.search(
            &req.query,
            req.workspace.as_deref(),
            req.limit.unwrap_or(20),
        );
        to_json(&hits)
    }

    #[tool(
        description = "엔티티에서 시작해 관계 방향(from->to)을 따라 그래프를 순회한다. 최대 홉(max_depth) 내에 도달하는 엔티티를 최단 거리와 함께 돌려준다."
    )]
    fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        let hits = self.engine.traverse(
            &req.id,
            req.max_depth.unwrap_or(3),
            req.limit.unwrap_or(100),
        );
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
                 observe 로 지식을 적재하고 get_entity/search_knowledge 로 탐색한다."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}

// --- 직렬화 헬퍼 (도구는 JSON 문자열을 반환) --------------------------------

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
