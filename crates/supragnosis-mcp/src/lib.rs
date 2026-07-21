//! supragnosis-mcp - MCP 표면(도구 + 리소스).
//!
//! rmcp 매크로로 도구를 정의하고 [`supragnosis_engine::Engine`] 으로 위임한다.
//! 도구: `observe`, `get_entity`, `search_knowledge`, `traverse`.
//! 리소스: `supragnosis://workspace/{ws}/graph` - 온톨로지 그래프(node-link) 읽기 뷰,
//! `supragnosis://observation/{id}` - 관측 역참조 (원문 + provenance + 계보, 원칙 2/14).

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
    Engine, EntityInput as EngineEntityInput, Event, ObserveInput,
    RelationInput as EngineRelationInput, SearchMode,
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
    /// 신뢰도 0.0~1.0. 생략하면 무표기로 보존된다(기본값 치환 없음) - 평가가 불가하면
    /// 생략하라. 범위 밖 값은 적재가 거부된다.
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceMapRequest {
    /// 작업 공간(생략 시 노드 기본값, '*'/'all' 은 전체).
    #[serde(default)]
    pub workspace: Option<String>,
    /// 최대 클러스터 수(생략 시 20). 큰 순(크기)으로 잘린다.
    #[serde(default)]
    pub limit: Option<usize>,
    /// 최소 클러스터 크기 = 공동출현 엔티티 수(생략 시 2). 3 이상이면 사소한 쌍을 제외한다.
    #[serde(default)]
    pub min_size: Option<usize>,
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
    async fn observe(&self, Parameters(req): Parameters<ObserveRequest>) -> String {
        // 실제 사용된 워크스페이스(생략 시 노드 기본값) - 이벤트에 실어 뷰어가 스코프를 안다.
        let workspace = req
            .workspace
            .clone()
            .unwrap_or_else(|| self.engine.default_workspace().to_string());
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
        // blocking 저장소 호출을 spawn_blocking 으로 - HTTP 동시 요청이 tokio 워커를 굶기지
        // 않게 한다(stdio 단일 클라이언트에선 무해했으나 데몬 다중 접속엔 필수).
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.observe(input)).await {
            Ok(Ok(out)) => {
                // UI 관측가능성: 적재 활동을 발행(뷰어 라이브 로그 + 새 노드 펄스).
                self.engine.emit(Event::Observe {
                    observation: out.observation_id.clone(),
                    entities: out.entities.clone(),
                    relations: out.relations.len(),
                    workspace,
                });
                to_json(&out)
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(description = "엔티티 id로 엔티티와 그 관계/출처를 조회한다.")]
    async fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        let id = req.id.clone();
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.get_entity(&id)).await {
            Ok(Ok(Some(view))) => {
                self.engine.emit(Event::GetEntity {
                    id: req.id.clone(),
                    name: Some(view.entity.canonical_name.clone()),
                    found: true,
                });
                to_json(&view)
            }
            // 열린 세계 가정(원칙 5): 부재는 거짓이 아니라 미지(unknown)다.
            // "찾지 못함"을 에러로 주지 않아 LLM 이 부재를 부정으로 오독하지 않게 한다.
            Ok(Ok(None)) => {
                self.engine.emit(Event::GetEntity {
                    id: req.id.clone(),
                    name: None,
                    found: false,
                });
                serde_json::json!({
                    "found": false,
                    "id": req.id,
                    "note": "unknown - not found is not a negation (open-world assumption)"
                })
                .to_string()
            }
            // 고장은 부재와 다르다(원칙 5) - "없음" 으로 오독되지 않게 명시 에러로.
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "지식(엔티티/관측)을 검색한다. 응답의 mode 가 실제 사용 표면을 알린다: hybrid(의미+키워드) 또는 keyword(키워드 전용 degrade - 이 모드의 빈 결과는 회상 실패일 수 있다). score 는 순위 비교 전용이다 - 스케일이 mode 마다 달라 절대값은 신뢰도가 아니다."
    )]
    async fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let query = req.query.clone();
        let ws = req.workspace.clone();
        let limit = req.limit.unwrap_or(20);
        let engine = self.engine.clone();
        let searched =
            tokio::task::spawn_blocking(move || engine.search(&query, ws.as_deref(), limit)).await;
        let searched = match searched {
            Ok(r) => r,
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        match searched {
            Ok(out) => {
                // UI 관측가능성: 검색 활동 발행(뷰어 로그 + 히트 노드 강조).
                self.engine.emit(Event::Search {
                    query: req.query.clone(),
                    workspace: req.workspace.clone(),
                    hits: out.hits.len(),
                    nodes: out.hits.iter().map(|h| h.id.clone()).collect(),
                    mode: match out.mode {
                        SearchMode::Hybrid => "hybrid",
                        SearchMode::Keyword => "keyword",
                    }
                    .to_string(),
                });
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
    async fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        let id = req.id.clone();
        let max_depth = req.max_depth.unwrap_or(3);
        let limit = req.limit.unwrap_or(100);
        let engine = self.engine.clone();
        let traversed =
            tokio::task::spawn_blocking(move || engine.traverse(&id, max_depth, limit)).await;
        let hits = match traversed {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // UI 관측가능성: 순회 활동 발행(시작 + 도달 노드 강조).
        self.engine.emit(Event::Traverse {
            start: req.id.clone(),
            reached: hits.iter().map(|h| h.id.clone()).collect(),
        });
        let mut resp = serde_json::json!({ "hits": hits });
        // 0건의 원인을 구별해 알린다(원칙 5/21): 시작 엔티티 부재(미지)와 "존재하지만
        // 나가는 관계 없음"은 LLM 이 다르게 교정해야 할 상황이다. 확인용 get_entity 도
        // blocking 이라 spawn_blocking 으로 오프로드.
        if hits.is_empty() {
            let id2 = req.id.clone();
            let engine2 = self.engine.clone();
            let exists = tokio::task::spawn_blocking(move || engine2.get_entity(&id2)).await;
            resp["note"] = serde_json::Value::String(match exists {
                Ok(Ok(Some(_))) => "start entity exists but reached no entities - it has \
                                    no outgoing relations within max_depth (absence of \
                                    edges is unknown, not a negation)"
                    .into(),
                Ok(Ok(None)) => "start entity id not found - unknown, not a negation \
                                 (open-world). Find the id via search_knowledge first"
                    .into(),
                _ => "empty result; start entity could not be checked due to a \
                      storage failure - do not conclude absence"
                    .into(),
            });
        }
        resp.to_string()
    }

    #[tool(
        description = "워크스페이스의 주요 공동출현 맥락(하이퍼엣지 - 한 관측에서 함께 주장된 엔티티 집합)을 개관한다. cold-start 오리엔테이션용: 검색 전에 '여기 무엇이 어떤 덩어리로 있나'를 이름으로 파악한다. 클러스터는 크기(공동출현 엔티티 수) 순으로 정렬되고 sources 는 뒷받침 관측 수다. 이는 주장된 관계가 아니라 방향성 신호다 - 실제 관계/상세는 search_knowledge/get_entity 로 확인하라."
    )]
    async fn workspace_map(&self, Parameters(req): Parameters<WorkspaceMapRequest>) -> String {
        // 워크스페이스 해석: 생략 -> 노드 기본, '*'/'all'/'' -> 전체(None) (graph 리소스와 동일).
        let ws_arg: Option<String> = match req.workspace.as_deref() {
            None => Some(self.engine.default_workspace().to_string()),
            Some("") | Some("*") | Some("all") => None,
            Some(s) => Some(s.to_string()),
        };
        let limit = req.limit.unwrap_or(20);
        // 최소 크기는 2 미만으로 못 내린다(size<2 는 하이퍼엣지가 아니다).
        let min_size = req.min_size.unwrap_or(2).max(2);
        let engine = self.engine.clone();
        let ws_call = ws_arg.clone();
        let mapped =
            tokio::task::spawn_blocking(move || engine.hypergraph(ws_call.as_deref())).await;
        let hg = match mapped {
            Ok(Ok(hg)) => hg,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // 이름 중심의 읽기 쉬운 요약(원칙 21). 하이퍼엣지는 이미 (크기 desc, id asc) 정렬.
        let qualifying = hg.hyperedges.iter().filter(|h| h.size >= min_size).count();
        let clusters: Vec<serde_json::Value> = hg
            .hyperedges
            .iter()
            .filter(|h| h.size >= min_size)
            .take(limit)
            .map(|h| {
                serde_json::json!({
                    "concepts": h.member_names,
                    "size": h.size,
                    "sources": h.sources,
                    "trust_tier": h.trust_tier,
                })
            })
            .collect();
        let shown = clusters.len();
        let mut resp = serde_json::json!({
            "workspace": ws_arg,
            "clusters": clusters,
            "stats": {
                "node_count": hg.stats.node_count,
                "hyperedge_count": hg.stats.hyperedge_count,
                "max_size": hg.stats.max_size,
                "shown": shown,
                "matched": qualifying,
            },
        });
        // 절단을 침묵하지 않는다(no silent caps) + 0건은 부재!=부정(원칙 5).
        if qualifying > shown {
            resp["note"] = serde_json::Value::String(format!(
                "showing top {shown} of {qualifying} clusters (by size). raise limit or lower \
                 min_size to see more. clusters are co-occurrence contexts (entities asserted \
                 together), not asserted relations - drill in with search_knowledge/get_entity \
                 by concept name"
            ));
        } else if shown == 0 {
            resp["note"] = serde_json::Value::String(
                "no co-occurrence clusters at this min_size - absence is unknown, not a negation \
                 (open-world). Lower min_size, widen workspace ('*'), or the workspace may be \
                 sparsely linked (entities observed alone). observe more, or use search_knowledge"
                    .into(),
            );
        }
        resp.to_string()
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
                 observe 로 지식을 적재하고 search_knowledge/get_entity/traverse 로 탐색한다. \
                 workspace_map 으로 워크스페이스의 주요 공동출현 맥락(클러스터)을 개관한다 \
                 (검색 전 오리엔테이션). 리소스: supragnosis://workspace/{ws}/graph 는 온톨로지 \
                 그래프 전체(node-link), supragnosis://workspace/{ws}/hypergraph 는 공동출현 \
                 이차 구조(하이퍼엣지), supragnosis://observation/{id} 는 관측 역참조(원문+출처+ \
                 계보 - 검색 히트의 근거를 확인할 때 사용)."
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
        // 워크스페이스 발견 진입점 - 어느 워크스페이스가 있는지 먼저 보게 한다.
        let mut ws_list = RawResource::new(
            "supragnosis://workspaces".to_string(),
            "워크스페이스 목록".to_string(),
        );
        ws_list.description = Some(
            "지식이 있는 워크스페이스 이름 목록(정렬). 어느 워크스페이스가 있는지 발견하는 진입점."
                .to_string(),
        );
        ws_list.mime_type = Some("application/json".to_string());

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

        // 공동출현 이차 구조(원칙 11) - 기본 워크스페이스의 하이퍼그래프도 구체 리소스로 노출.
        let mut hyper = RawResource::new(
            format!("supragnosis://workspace/{ws}/hypergraph"),
            format!("{ws} 하이퍼그래프(공동출현)"),
        );
        hyper.description = Some(
            "한 관측이 공동 주장한 엔티티 집합(하이퍼엣지)의 파생 뷰 - 이진 관계가 버린 맥락의 회복(원칙 11)."
                .to_string(),
        );
        hyper.mime_type = Some("application/json".to_string());
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![
            ws_list.no_annotation(),
            res.no_annotation(),
            hyper.no_annotation(),
        ])))
    }

    /// 리소스 템플릿: 임의 워크스페이스의 그래프와 관측 역참조를 URI 패턴으로 조회하게 한다.
    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        let graph = RawResourceTemplate {
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
        // 공동출현 이차 구조(원칙 11): 한 관측이 공동 주장한 엔티티 집합 = 하이퍼엣지.
        let hypergraph = RawResourceTemplate {
            uri_template: "supragnosis://workspace/{workspace}/hypergraph".to_string(),
            name: "workspace-hypergraph".to_string(),
            title: Some("워크스페이스 하이퍼그래프 (공동출현 이차 구조)".to_string()),
            description: Some(
                "한 관측이 공동 주장한 엔티티 집합을 하이퍼엣지로 되살린 파생 뷰 - 이진 관계가 \
                 버린 맥락(무엇이 함께 말해졌나)의 회복. {workspace} 를 채워 조회한다."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        // 관측 역참조 (원칙 2/14): 검색 히트의 관측 id 로 원문 + provenance + 계보에
        // 도달하는 경로 - "이 답이 어디서 왔는가"에 답하는 표면.
        let observation = RawResourceTemplate {
            uri_template: "supragnosis://observation/{id}".to_string(),
            name: "observation".to_string(),
            title: Some("관측 (원문 + 출처 + 계보)".to_string()),
            description: Some(
                "관측 id(search_knowledge 히트의 kind=observation id)로 원문 content, \
                 provenance attestation 전체, derived_from 계보를 조회한다."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            graph.no_annotation(),
            hypergraph.no_annotation(),
            observation.no_annotation(),
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
        let result = match parse_resource_uri(&uri) {
            // 워크스페이스 목록(발견용) - 지식이 있는 워크스페이스 이름 배열.
            Some(ResourceUri::Workspaces) => match self.engine.workspaces() {
                Ok(list) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&list), uri)],
                }),
                Err(e) => Err(ErrorData::internal_error(
                    format!("저장소 백엔드 실패 (리소스 부재가 아니다): {e}"),
                    None,
                )),
            },
            Some(ResourceUri::Graph(ws)) => match self.engine.graph(Some(ws)) {
                Ok(graph) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&graph), uri)],
                }),
                // 고장은 not_found(부재)가 아니라 내부 오류로 - 원칙 5 의 구별을 유지한다.
                Err(e) => Err(ErrorData::internal_error(
                    format!("저장소 백엔드 실패 (리소스 부재가 아니다): {e}"),
                    None,
                )),
            },
            // 공동출현 이차 구조(원칙 11): 관측이 공동 주장한 엔티티 집합을 하이퍼엣지로.
            Some(ResourceUri::Hypergraph(ws)) => match self.engine.hypergraph(Some(ws)) {
                Ok(hg) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&hg), uri)],
                }),
                Err(e) => Err(ErrorData::internal_error(
                    format!("저장소 백엔드 실패 (리소스 부재가 아니다): {e}"),
                    None,
                )),
            },
            // 관측 역참조 (원칙 2/14): id 를 아는 자는 실체(원문/provenance/계보)를 조회한다.
            Some(ResourceUri::Observation(id)) => match self.engine.get_observation(id) {
                Ok(Some(obs)) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&obs), uri)],
                }),
                Ok(None) => Err(ErrorData::resource_not_found(
                    format!(
                        "관측 id 를 찾지 못함: {id} - 부재는 부정이 아니다(열린 세계). \
                         search_knowledge 히트의 kind=observation id 를 사용하라"
                    ),
                    None,
                )),
                Err(e) => Err(ErrorData::internal_error(
                    format!("저장소 백엔드 실패 (리소스 부재가 아니다): {e}"),
                    None,
                )),
            },
            None => Err(ErrorData::resource_not_found(
                format!(
                    "알 수 없는 리소스 URI: {uri} - supragnosis://workspace/{{workspace}}/graph \
                     또는 supragnosis://observation/{{id}} 형태만 지원한다"
                ),
                None,
            )),
        };
        std::future::ready(result)
    }
}

/// 리소스 URI 의 종류.
enum ResourceUri<'a> {
    /// `supragnosis://workspaces` - 지식이 있는 워크스페이스 목록(도입부/발견용).
    Workspaces,
    /// `supragnosis://workspace/<ws>/graph` - 워크스페이스 온톨로지 그래프.
    Graph(&'a str),
    /// `supragnosis://workspace/<ws>/hypergraph` - 공동출현 이차 구조(원칙 11 이차 구조).
    Hypergraph(&'a str),
    /// `supragnosis://observation/<id>` - 관측 역참조 (원칙 2/14).
    Observation(&'a str),
}

/// 리소스 URI 파서. 형식이 어긋나면 None. 세그먼트에는 `/` 가 없어야 한다.
fn parse_resource_uri(uri: &str) -> Option<ResourceUri<'_>> {
    // 정확 일치가 먼저 - "workspaces"(복수) 는 "workspace/"(단수+슬래시) prefix 와 겹치지 않는다.
    if uri == "supragnosis://workspaces" {
        return Some(ResourceUri::Workspaces);
    }
    if let Some(rest) = uri.strip_prefix("supragnosis://workspace/") {
        // "/hypergraph" 를 먼저 본다 - "hypergraph" 는 "/graph" 로 끝나지 않으나(앞이 'r'),
        // 의도를 분명히 하려 명시 순서로 둔다. 세그먼트에 '/' 가 있으면 거부(단일 세그먼트).
        if let Some(ws) = rest.strip_suffix("/hypergraph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Hypergraph(ws));
        }
        if let Some(ws) = rest.strip_suffix("/graph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Graph(ws));
        }
        return None;
    }
    if let Some(id) = uri.strip_prefix("supragnosis://observation/") {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        return Some(ResourceUri::Observation(id));
    }
    None
}

// --- 직렬화 헬퍼 (도구는 JSON 문자열을 반환) --------------------------------

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
