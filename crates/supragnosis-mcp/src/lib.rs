//! supragnosis-mcp - MCP 표면(도구 + 리소스).
//!
//! rmcp 매크로로 도구를 정의하고 [`supragnosis_engine::Engine`] 으로 위임한다.
//! 도구: `observe`, `get_entity`, `search_knowledge`, `traverse`.
//! 리소스: `supragnosis://workspace/{ws}/graph` - 온톨로지 그래프(node-link) 읽기 뷰.

use std::future::Future;
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
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
    /// 신뢰도 0.0~1.0 (생략 시 1.0). 범위 밖 값은 적재가 거부된다.
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
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
    /// 관계 타입(예: depends_on, part_of, relates_to). 표기는 서버가 정준화한다
    /// (depends-on/dependsOn -> depends_on) - 표기 요동이 다른 엣지가 되지 않는다.
    #[serde(rename = "type")]
    pub kind: String,
    /// 도착 엔티티의 이름.
    pub to: String,
    /// (선택) 유효시간 시작, epoch millis. 관계가 세계에서 참이 된 시점 - 과거에
    /// 참이었던 사실을 소급 기록할 때 쓴다. 생략 시 관측 시점부터로 해석.
    #[serde(default)]
    pub valid_from: Option<u64>,
    /// (선택) 유효시간 종료, epoch millis. 이미 끝난 사실("지난달까지 참")을 기록할 때
    /// 쓴다. 생략 시 반증 전까지 참으로 해석.
    #[serde(default)]
    pub valid_to: Option<u64>,
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
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
        };
        respond(self.engine.observe(input))
    }

    #[tool(description = "엔티티 id로 엔티티와 그 관계/출처를 조회한다.")]
    fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        match self.engine.get_entity(&req.id) {
            Ok(Some(view)) => to_json(&view),
            // 열린 세계 가정(원칙 5): 부재는 거짓이 아니라 미지(unknown)다.
            // "찾지 못함"을 에러로 주지 않아 LLM 이 부재를 부정으로 오독하지 않게 한다.
            Ok(None) => serde_json::json!({
                "found": false,
                "id": req.id,
                "note": "unknown - not found is not a negation (open-world assumption)"
            })
            .to_string(),
            // 고장은 부재와 다르다(원칙 5) - "없음" 으로 오독되지 않게 명시 에러로.
            Err(e) => store_failure_json(&e),
        }
    }

    #[tool(
        description = "지식(엔티티/관측)을 검색한다. 응답의 mode 가 실제 사용 표면을 알린다: hybrid(의미+키워드) 또는 keyword(키워드 전용 degrade - 이 모드의 빈 결과는 회상 실패일 수 있다)."
    )]
    fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        match self.engine.search(
            &req.query,
            req.workspace.as_deref(),
            req.limit.unwrap_or(20),
        ) {
            Ok(out) => {
                let mut resp = serde_json::json!({ "mode": out.mode, "hits": out.hits });
                // 열린 세계 가정(원칙 5): 0건은 부정이 아니라 미지다. keyword degrade 의
                // 0건은 회상 실패 가능성이 더 높다는 것까지 알려 자기 교정을 돕는다(원칙 21).
                if out.hits.is_empty() {
                    resp["note"] = serde_json::Value::String(match out.mode {
                        supragnosis_engine::SearchMode::Hybrid => {
                            "no hits - absence is unknown, not a negation (open-world). \
                             The knowledge may not be ingested or phrased differently; \
                             try other terms or traverse from a related entity"
                                .into()
                        }
                        supragnosis_engine::SearchMode::Keyword => {
                            "no hits under keyword-only degrade (semantic recall UNAVAILABLE) \
                             - a miss here is weak evidence of absence, not a negation. \
                             Try exact terms the knowledge would contain"
                                .into()
                        }
                    });
                }
                resp.to_string()
            }
            Err(e) => store_failure_json(&e),
        }
    }

    #[tool(
        description = "엔티티에서 시작해 관계 방향(from->to)을 따라 그래프를 순회한다. 최대 홉(max_depth) 내에 도달하는 엔티티를 최단 거리와 함께 돌려준다."
    )]
    fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        match self.engine.traverse(
            &req.id,
            req.max_depth.unwrap_or(3),
            req.limit.unwrap_or(100),
        ) {
            Ok(hits) => {
                let mut resp = serde_json::json!({ "hits": hits });
                // 0건의 원인을 구별해 알린다(원칙 5/21): 시작 엔티티 부재(미지)와
                // "존재하지만 나가는 관계 없음"은 LLM 이 다르게 교정해야 할 상황이다.
                if hits.is_empty() {
                    resp["note"] = serde_json::Value::String(match self.engine.get_entity(&req.id)
                    {
                        Ok(Some(_)) => "start entity exists but reached no entities - it has \
                                        no outgoing relations within max_depth (absence of \
                                        edges is unknown, not a negation)"
                            .into(),
                        Ok(None) => "start entity id not found - unknown, not a negation \
                                     (open-world). Find the id via search_knowledge first"
                            .into(),
                        Err(_) => "empty result; start entity could not be checked due to a \
                                   storage failure - do not conclude absence"
                            .into(),
                    });
                }
                resp.to_string()
            }
            Err(e) => store_failure_json(&e),
        }
    }
}

#[tool_handler]
impl ServerHandler for SupragnosisServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "supragnosis: 여러 호스트/워크스페이스의 지식을 온톨로지화하는 MCP 서버. \
                 observe 로 지식을 적재하고 get_entity/search_knowledge 로 탐색한다. \
                 supragnosis://workspace/{ws}/graph 리소스로 온톨로지 그래프 전체(node-link)를 조회한다."
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    /// 구체 리소스 목록: 노드의 기본 워크스페이스 그래프 하나를 노출한다(다른 ws 는 템플릿으로).
    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        let ws = self.engine.default_workspace();
        let mut res = RawResource::new(
            format!("supragnosis://workspace/{ws}/graph"),
            format!("{ws} 온톨로지 그래프"),
        );
        res.description = Some(
            "엔티티(노드)+관계(엣지) node-link 그래프. provenance/신뢰 등급/유효구간 포함, 읽기 전용 파생 뷰."
                .to_string(),
        );
        res.mime_type = Some("application/json".to_string());
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![
            res.no_annotation(),
        ])))
    }

    /// 리소스 템플릿: 임의 워크스페이스의 그래프를 URI 패턴으로 조회하게 한다.
    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        let tmpl = RawResourceTemplate {
            uri_template: "supragnosis://workspace/{workspace}/graph".to_string(),
            name: "workspace-graph".to_string(),
            title: Some("워크스페이스 온톨로지 그래프".to_string()),
            description: Some(
                "특정 워크스페이스의 엔티티-관계 그래프(node-link). {workspace} 를 채워 조회한다."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            tmpl.no_annotation(),
        ])))
    }

    /// 리소스 읽기: URI 에서 워크스페이스를 파싱해 그래프 프로젝션 JSON 을 돌려준다.
    /// 알 수 없는 URI 는 resource_not_found(부재는 미지, 원칙 5) 로 자기 교정 힌트를 담아 준다.
    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, ErrorData>> + Send + '_ {
        let uri = request.uri;
        let result = match parse_graph_uri(&uri) {
            Some(ws) => match self.engine.graph(Some(ws)) {
                Ok(graph) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&graph), uri)],
                }),
                // 고장은 not_found(부재)가 아니라 내부 오류로 - 원칙 5 의 구별을 유지한다.
                Err(e) => Err(ErrorData::internal_error(
                    format!("저장소 백엔드 실패 (리소스 부재가 아니다): {e}"),
                    None,
                )),
            },
            None => Err(ErrorData::resource_not_found(
                format!(
                    "알 수 없는 리소스 URI: {uri} - supragnosis://workspace/{{workspace}}/graph 형태만 지원한다"
                ),
                None,
            )),
        };
        std::future::ready(result)
    }
}

/// `supragnosis://workspace/<ws>/graph` 에서 워크스페이스를 뽑는다. 형식이 어긋나면 None.
/// ws 에는 `/` 가 없어야 한다(경로 세그먼트 하나).
fn parse_graph_uri(uri: &str) -> Option<&str> {
    let ws = uri
        .strip_prefix("supragnosis://workspace/")?
        .strip_suffix("/graph")?;
    if ws.is_empty() || ws.contains('/') {
        return None;
    }
    Some(ws)
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

/// 저장소 고장의 응답 (원칙 5/21). 고장은 부재와 다른 사건이므로 "빈 결과가 아니라
/// 조회 불능"임을 명시하고, LLM 이 지식 부재로 결론짓지 않도록 다음 행동을 안내한다.
fn store_failure_json(e: &impl std::fmt::Display) -> String {
    serde_json::json!({
        "error": e.to_string(),
        "note": "storage backend failure - this is NOT an empty result. \
                 지식이 없다고 결론짓지 말라. 재시도하거나 사용자에게 저장소 상태 확인을 요청하라"
    })
    .to_string()
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
