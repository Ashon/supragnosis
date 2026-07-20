//! supragnosis-engine - 서비스(유스케이스) 계층.
//!
//! MCP 도구가 호출하는 결정론적 로직: 관측 적재 -> 엔티티 해소 -> 관계 링크 -> 조회/검색.
//! 저장소는 [`supragnosis_core::KnowledgeStore`] 포트를 통해서만 접근한다.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use supragnosis_core::{
    normalize_relation_kind, now_millis, Assertions, EmbeddingProvider, Entity, EntityAssertion,
    KnowledgeStore, Observation, Provenance, Relation, RelationAssertion, SearchHit, SearchHitKind,
    StoreError, Timestamp, TraverseHit, TrustTier,
};
// UI 관측가능성 포트/타입을 재노출 - mcp/viz 가 core 를 직접 의존하지 않고 쓴다.
pub use supragnosis_core::{Event, EventEnvelope, EventSink};

/// 적재 입력 (전송 DTO에서 매핑되는 도메인 입력).
pub struct ObserveInput {
    pub content: String,
    pub workspace: Option<String>,
    pub source_ref: Option<String>,
    pub confidence: Option<f32>,
    /// 위임 사슬(원칙 2): 이 관측을 acting host 가 대리하는 principal.
    pub on_behalf_of: Option<String>,
    /// 계보(원칙 18): 이 관측이 파생된 원천 관측 id들.
    pub derived_from: Vec<String>,
    pub entities: Vec<EntityInput>,
    pub relations: Vec<RelationInput>,
}

pub struct EntityInput {
    pub name: String,
    pub kind: Option<String>,
}

pub struct RelationInput {
    pub from: String,
    pub kind: String,
    pub to: String,
    /// 유효시간 시작 (원칙 4, 선택). 소급 관측을 적재 시점에 캡처한다.
    pub valid_from: Option<Timestamp>,
    /// 유효시간 종료 (원칙 4, 선택).
    pub valid_to: Option<Timestamp>,
}

#[derive(Serialize)]
pub struct ObserveOutput {
    pub observation_id: String,
    pub entities: Vec<String>,
    pub relations: Vec<String>,
}

/// 적재 실패. 검증 오류 메시지는 LLM 클라이언트가 자기 교정할 수 있게 쓴다 (원칙 21:
/// 왜 실패했고 무엇을 다르게 하면 되는지).
#[derive(Debug, thiserror::Error)]
pub enum ObserveError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// 검색이 실제로 사용한 표면 (원칙 16 4차 개정: 응답은 자신이 어느 표면에서 왔는지
/// 표기해, 클라이언트가 수렴 표면과 회상 보조를 구별할 수 있어야 한다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// 키워드(수렴 표면) + 의미 벡터(노드 로컬 회상 보조) 하이브리드.
    Hybrid,
    /// 키워드 전용. 임베더 부재/질의 임베딩 실패로 degrade 된 상태 포함 (원칙 19) -
    /// 이 모드의 0건은 하이브리드의 0건보다 "회상 실패" 가능성이 높다.
    Keyword,
}

/// 검색 응답: 사용 표면 + 히트.
#[derive(Serialize)]
pub struct SearchOutput {
    pub mode: SearchMode,
    pub hits: Vec<SearchHit>,
}

/// 엔티티 + 그 관계(조회 응답).
#[derive(Serialize)]
pub struct EntityView {
    #[serde(flatten)]
    pub entity: Entity,
    pub relations: Vec<Relation>,
}

/// 온톨로지 그래프 프로젝션(관측가능성/시각화의 읽기 뷰).
///
/// 관측 로그가 진실의 원천이고 이 뷰는 그 위에 계산된 **파생 뷰**다(원칙 1) - 아무것도
/// 쓰지 않는 순수 읽기다. 노드/엣지에 provenance 요약(신뢰 등급/출처 수)을 실어 "이 지식이
/// 어디서/얼마나 뒷받침되나"까지 보게 한다(원칙 2/18). 정렬은 결정적이다(원칙 16).
#[derive(Serialize)]
pub struct GraphView {
    /// 스코프한 워크스페이스. None 이면 전체.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
}

/// 그래프 노드 = 엔티티. 시각화 힌트(타입/degree/신뢰)를 함께 싣는다.
#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// 그래프에 포함된 엣지 중 이 노드에 연결된 수(양끝이 모두 노드 집합에 있는 엣지만).
    pub degree: usize,
    /// 이 엔티티에 누적된 출처(attestation) 수 - 여러 관측이 뒷받침할수록 크다.
    pub sources: usize,
    /// 출처들 중 **최고** 신뢰 등급(원칙 18) - 노드의 대표 신뢰도.
    pub trust_tier: TrustTier,
}

/// 그래프 엣지 = 타입된 관계. provenance 요약과 유효구간을 싣는다.
#[derive(Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub trust_tier: TrustTier,
    /// 무표기(None)는 표기 없음 그대로 - 1.0 으로 보이지 않는다 (원칙 2 4차).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// 유효구간 종료(원칙 4). Some 이면 대체/반증되어 현재는 참이 아님 - 뷰어가 흐리게 그린다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

/// 그래프 요약 지표(관측가능성의 첫 계량). 결정적 정렬 위해 BTreeMap.
#[derive(Serialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    /// 타입별 노드 수.
    pub type_counts: BTreeMap<String, usize>,
    /// 신뢰 등급별 노드 수(대표 등급 기준).
    pub trust_counts: BTreeMap<String, usize>,
}

/// TrustTier 의 안정적 문자열 라벨(직렬화 snake_case 와 일치). 지표 키에 쓴다.
fn tier_label(t: TrustTier) -> &'static str {
    match t {
        TrustTier::Unverified => "unverified",
        TrustTier::AgentExtracted => "agent_extracted",
        TrustTier::HostSigned => "host_signed",
        TrustTier::HumanConfirmed => "human_confirmed",
    }
}

pub struct Engine {
    store: Arc<dyn KnowledgeStore>,
    /// 임베딩 공급 포트 (원칙 19: 확률적 경계). 없으면 검색은 키워드로 degrade.
    embedder: Option<Arc<dyn EmbeddingProvider>>,
    /// UI 이벤트 싱크 (관측가능성, 선택). 없으면 emit 은 no-op.
    events: Option<Arc<dyn EventSink>>,
    /// 세션 id (발자국 그룹 키). 발행하는 모든 이벤트에 실린다 - 기본 "local".
    session: String,
    host: String,
    default_workspace: String,
}

impl Engine {
    pub fn new(
        store: Arc<dyn KnowledgeStore>,
        host: impl Into<String>,
        default_workspace: impl Into<String>,
    ) -> Self {
        Self {
            store,
            embedder: None,
            events: None,
            session: "local".to_string(),
            host: host.into(),
            default_workspace: default_workspace.into(),
        }
    }

    /// 임베딩 공급자를 붙인다(빌더). 붙이면 observe 가 관측에 임베딩을 달고
    /// search 가 벡터+키워드 하이브리드로 동작한다. 안 붙이면 키워드 전용(degrade).
    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// UI 이벤트 싱크를 붙인다(빌더, 관측가능성). 붙이면 [`Engine::emit`] 이 여기로
    /// 흘려보낸다 - 뷰어의 라이브 활동 로그/노드 강조용. 안 붙이면 emit 은 no-op.
    pub fn with_events(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.events = Some(sink);
        self
    }

    /// 세션 id 를 설정한다(빌더). 발행하는 모든 이벤트에 실려 대화 발자국의 그룹 키가
    /// 된다 - 뷰어가 "이 세션이 어떤 지식을 썼나"를 묶어 보여준다.
    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session = session.into();
        self
    }

    /// UI 이벤트를 발행한다. 싱크가 없으면 아무것도 하지 않는다(관측가능성은 선택).
    /// 세션 id 를 봉투에 실어 보낸다(발자국 그룹 키). 호출자(MCP 도구 핸들러)가 의도
    /// 단위로 부른다 - 저장/해소 로직과 무관한 부수 채널.
    pub fn emit(&self, event: Event) {
        if let Some(sink) = &self.events {
            sink.emit(&EventEnvelope {
                session: self.session.clone(),
                event,
            });
        }
    }

    fn provenance(
        &self,
        workspace: &str,
        source_ref: Option<String>,
        confidence: Option<f32>,
        on_behalf_of: Option<String>,
    ) -> Provenance {
        Provenance {
            host: self.host.clone(),
            on_behalf_of,
            workspace: workspace.to_string(),
            source_ref,
            observed_at: now_millis(),
            // 무표기는 무표기로 보존한다 (원칙 2 4차) - 기본값(1.0) 치환은 "주장 없음"과
            // "만점 주장"의 구별을 소실시키는 캡처 손실이다. 해석은 해소 정책(M3)의 몫.
            confidence,
            // 신뢰 등급 승격은 명시적 흐름(사람 확인/교차검증)에서만 - observe 는 기본값.
            trust_tier: TrustTier::default(),
        }
    }

    /// 지식 조각을 적재한다: 불변 관측 저장 + 제공된 엔티티/관계를 온톨로지에 링크.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, ObserveError> {
        // confidence 범위 강제 (원칙 2: 스키마 수준 강제). append-only 로그에 한번
        // 실린 값은 영구하므로 적재 전에 막는다. NaN 도 contains 가 false 라 걸린다.
        if let Some(c) = input.confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(ObserveError::Invalid(format!(
                    "confidence 는 0.0~1.0 이어야 한다 (받은 값: {c}). 확신이 낮으면 \
                     낮은 값을 주고, 평가가 불가하면 생략하라"
                )));
            }
        }
        // 정형성 검증 (원칙 1: 적재 검증은 정형성까지만). 빈 지시어는 "다르게 표기된
        // 주장"이 아니라 지시 대상이 없는 비주장이다 - 영구 로그에 실리기 전에 막는다.
        // 표기 자체는 검열하지 않는다: 거부는 변형이 아니며, 정규화는 프로젝션의 일.
        for e in &input.entities {
            if e.name.trim().is_empty() {
                return Err(ObserveError::Invalid(
                    "엔티티 이름이 비어 있다. 이름 없는 엔티티 주장은 성립하지 않는다 - \
                     지시할 이름을 넣거나 항목을 빼라"
                        .into(),
                ));
            }
            if e.kind.as_deref().is_some_and(|k| k.trim().is_empty()) {
                return Err(ObserveError::Invalid(format!(
                    "엔티티 '{}' 의 타입이 빈 문자열이다. 빈 타입 주장은 타입 미지정과 \
                     다른, 성립하지 않는 주장이다 - 타입을 모르면 type 을 생략하라",
                    e.name
                )));
            }
        }
        for r in &input.relations {
            if r.from.trim().is_empty() || r.to.trim().is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "관계의 끝점이 비어 있다 (from: {:?}, to: {:?}). 이름 없는 엔티티를 \
                     가리키는 관계 주장은 성립하지 않는다 - 양끝 엔티티 이름을 넣어라",
                    r.from, r.to
                )));
            }
            if normalize_relation_kind(&r.kind).is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "관계 타입이 비어 있다 (받은 값: {:?} - 정규화하면 빈 문자열). \
                     depends_on / part_of 처럼 의미가 읽히는 타입을 넣어라",
                    r.kind
                )));
            }
        }
        let workspace = input
            .workspace
            .unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(
            &workspace,
            input.source_ref,
            input.confidence,
            input.on_behalf_of,
        );

        // 구조화 주장은 관측 로그에 **원문 그대로** 동봉한다 (원칙 1: 로그가 진실의
        // 원천이고 그래프는 프로젝션 - 주장이 로그에 없으면 재프로젝션으로 그래프를
        // 복원할 수 없다). 정규화(kind 정준화 등)는 아래 프로젝션 단계의 일이다.
        let assertions = Assertions {
            entities: input
                .entities
                .iter()
                .map(|e| EntityAssertion {
                    name: e.name.clone(),
                    kind: e.kind.clone(),
                })
                .collect(),
            relations: input
                .relations
                .iter()
                .map(|r| RelationAssertion {
                    from: r.from.clone(),
                    kind: r.kind.clone(),
                    to: r.to.clone(),
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
        };
        let mut obs = Observation::with_assertions(input.content, prov.clone(), assertions);
        obs.derived_from = input.derived_from;
        // 임베딩 부착은 best-effort: 실패해도 관측 저장은 막지 않는다(원칙 19: degrade).
        // 단 degrade 는 침묵하지 않는다: 적재 시점 임베딩 실패는 이 관측을 의미 검색에서
        // 재시도 없이 제외하므로(같은 content 재관측 전까지) 흔적을 남긴다.
        if let Some(embedder) = &self.embedder {
            match embedder.embed_one(&obs.content) {
                Ok(vec) => obs.embedding = Some(vec),
                Err(e) => tracing::warn!(
                    observation_id = %obs.id,
                    error = %e,
                    "관측 임베딩 실패 - 키워드 검색으로만 회상된다 (degrade)"
                ),
            }
        }
        let observation_id = obs.id.clone();
        self.store.add_observation(obs)?;

        let mut entities = Vec::new();
        for e in input.entities {
            entities.push(self.upsert_named(&workspace, &e.name, e.kind, &prov)?);
        }

        let mut relations = Vec::new();
        for r in input.relations {
            let from = self.upsert_named(&workspace, &r.from, None, &prov)?;
            let to = self.upsert_named(&workspace, &r.to, None, &prov)?;
            // kind 는 정준형으로 프로젝션한다 - id 와 저장 표기가 항상 일치하도록
            // (id 만 정규화하면 같은 id 에 다른 표기가 last-write-wins 로 남는다).
            let kind = normalize_relation_kind(&r.kind);
            let rel = Relation {
                id: Relation::make_id(&from, &kind, &to),
                from,
                to,
                kind,
                provenance: prov.clone(),
                // 클라이언트가 명시한 유효구간을 그대로 프로젝션한다 (원칙 4 캡처).
                // 반증에 의한 valid_to 자동 종료 등 유도 로직은 M3.
                valid_from: r.valid_from,
                valid_to: r.valid_to,
            };
            let rid = rel.id.clone();
            self.store.add_relation(rel)?;
            relations.push(rid);
        }

        Ok(ObserveOutput {
            observation_id,
            entities,
            relations,
        })
    }

    /// M0 해소: 정규명 완전일치. 존재하면 출처만 덧붙이고, 없으면 생성.
    fn upsert_named(
        &self,
        workspace: &str,
        name: &str,
        kind: Option<String>,
        prov: &Provenance,
    ) -> Result<String, StoreError> {
        let id = Entity::make_id(workspace, name);
        let mut entity = self.store.get_entity(&id)?.unwrap_or_else(|| Entity {
            id: id.clone(),
            kind: kind.clone().unwrap_or_else(|| "Concept".to_string()),
            canonical_name: name.trim().to_string(),
            aliases: Vec::new(),
            properties: serde_json::Value::Null,
            provenance: Vec::new(),
            embedding: None,
        });
        if let Some(k) = kind {
            entity.kind = k;
        }
        entity.provenance.push(prov.clone());
        // 엔티티 이름/별칭을 임베딩해 시맨틱 검색이 노드에 **이름의 의미**로 도달하게 한다
        // (원칙 19: 회상 확장). 관측을 어휘로 언급하지 않는 노드의 회상 공백을 메운다.
        // 임베딩 부착은 best-effort: 실패해도 엔티티 저장은 막지 않는다(원칙 19: degrade).
        // 이름은 안정적이므로 없을 때 한 번만 계산한다(확률적 어댑터 호출 최소화).
        // 실패는 침묵하지 않는다 - 다음 관측이 이 엔티티를 만질 때 재시도되지만(여전히
        // None 이므로), 그때까지 이름의 의미로는 회상되지 않는다.
        if entity.embedding.is_none() {
            if let Some(embedder) = &self.embedder {
                match embedder.embed_one(&entity_text(&entity)) {
                    Ok(vec) => entity.embedding = Some(vec),
                    Err(e) => tracing::warn!(
                        entity_id = %entity.id,
                        name = %entity.canonical_name,
                        error = %e,
                        "엔티티 임베딩 실패 - 이름의 의미 회상 없이 저장한다 (degrade)"
                    ),
                }
            }
        }
        self.store.put_entity(entity)?;
        Ok(id)
    }

    /// 관측 역참조 (원칙 2/14): 검색 히트/파생 계보가 돌려준 관측 id 로 원문과
    /// provenance 전체, derived_from 계보에 도달한다 - "이 답이 어디서 왔는가"의 종점.
    pub fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        self.store.get_observation(id)
    }

    /// 엔티티 + 관계 조회. `Ok(None)` 은 부재(미지, 원칙 5), `Err` 는 저장소 고장 -
    /// 호출자(MCP 표면)가 둘을 구별해 전달할 수 있도록 실패를 삼키지 않는다.
    pub fn get_entity(&self, id: &str) -> Result<Option<EntityView>, StoreError> {
        match self.store.get_entity(id)? {
            Some(entity) => {
                let relations = self.store.relations_of(id)?;
                Ok(Some(EntityView { entity, relations }))
            }
            None => Ok(None),
        }
    }

    /// 하이브리드 검색: 키워드(부분일치) + 벡터(의미) 결과를 RRF 로 융합하고, 상위 엔티티
    /// 히트의 그래프 이웃으로 보강한다. 벡터 경로는 관측 본문과 엔티티 이름 **양쪽**을
    /// 시맨틱으로 회상하고(관측을 어휘로 언급하지 않는 엔티티 노드도 이름의 의미로 도달),
    /// 보강 단계는 매치된 엔티티의 1-hop 이웃을 채워 어휘/의미로는 안 걸리지만 그래프상
    /// 인접한 노드까지 회상한다(architecture 4.2 "그래프 문맥 보강"). 임베더가 없거나 질의
    /// 임베딩에 실패하면 키워드 결과만 융합한다(원칙 19: degrade). 확정 랭킹은 결정적이다.
    pub fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<SearchOutput, StoreError> {
        let keyword = self.store.search(query, workspace, limit)?;

        // 질의 임베딩은 한 번만 계산해 관측/엔티티 시맨틱 검색이 공유한다.
        // 임베딩 실패는 degrade(키워드 전용, 원칙 19)지만 저장소 실패는 Err 다 -
        // 확률적 어댑터의 부재/실패와 결정적 저장소의 고장은 다른 사건이다.
        let qvec = self.embedder.as_ref().and_then(|e| e.embed_one(query).ok());
        // mode 는 "의미 표면을 참조했는가" 다 - 의미 회상이 0건이어도 참조는 했으므로
        // hybrid 다 (0건의 인식론적 무게가 mode 에 따라 다르다, 원칙 5/16 4차).
        let mode = if qvec.is_some() {
            SearchMode::Hybrid
        } else {
            SearchMode::Keyword
        };
        let (semantic_obs, semantic_ent) = match &qvec {
            Some(v) => (
                self.store.search_semantic(v, workspace, limit)?,
                self.store.search_semantic_entities(v, workspace, limit)?,
            ),
            None => (Vec::new(), Vec::new()),
        };

        // 시맨틱 회상이 없으면(임베더 없음/미적재) 키워드 랭킹을 그대로, 있으면 RRF 융합.
        let fused = if semantic_obs.is_empty() && semantic_ent.is_empty() {
            keyword
        } else {
            fuse_rrf(&[keyword, semantic_obs, semantic_ent], limit)
        };

        // 그래프 문맥 보강: 상위 엔티티 히트의 1-hop 이웃을 여분 슬롯에 채운다.
        let hits = self.enrich_with_graph(fused, workspace, limit)?;
        Ok(SearchOutput { mode, hits })
    }

    /// 그래프 문맥 보강: 상위 엔티티 히트(시드)의 1-hop 이웃을 결과에 더한다. 이웃은 시드의
    /// 직접 매치보다 약한 신호이므로 시드 점수를 감쇠해 랭킹한다 - 1차 히트가 이웃보다 강하면
    /// 위에 남고, 강한 시드의 이웃은 약한 1차 히트보다 위로 올 수 있다(그래프 근접성 반영).
    /// 유계다: 시드 수/해소 이웃 수를 상한으로 잡아 활발한 노드가 결과를 뒤덮지 못한다.
    /// 결정적이다(원칙 16): 이웃 점수를 도달한 시드들의 최댓값으로 잡아 순회 순서에 무관하고,
    /// 최종 정렬을 (점수 desc, id asc)로 못박는다.
    fn enrich_with_graph(
        &self,
        mut results: Vec<SearchHit>,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        // 이미 결과에 있는 (kind, id) - 이웃 중복/1차 히트 재추가를 막는다.
        let present: HashSet<(SearchHitKind, String)> =
            results.iter().map(|h| (h.kind, h.id.clone())).collect();

        // 상위 엔티티 히트를 시드로 1-hop 이웃 점수를 모은다. 여러 시드에서 도달하면 최댓값
        // (도달 순서 무관 - 결정성). 관계의 반대편 엔드포인트가 이웃이다.
        let mut neighbor_score: HashMap<String, f32> = HashMap::new();
        for seed in results
            .iter()
            .filter(|h| h.kind == SearchHitKind::Entity)
            .take(GRAPH_ENRICH_SEEDS)
        {
            let contrib = seed.score * GRAPH_ENRICH_DECAY;
            for rel in self.store.relations_of(&seed.id)? {
                let neighbor = if rel.from == seed.id {
                    rel.to
                } else if rel.to == seed.id {
                    rel.from
                } else {
                    continue;
                };
                if present.contains(&(SearchHitKind::Entity, neighbor.clone())) {
                    continue;
                }
                let e = neighbor_score.entry(neighbor).or_insert(0.0);
                *e = e.max(contrib);
            }
        }

        // 해소 비용을 유계로: 상위 limit 개 이웃만 엔티티로 해소(이름/워크스페이스 확인).
        let mut candidates: Vec<(String, f32)> = neighbor_score.into_iter().collect();
        candidates.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        candidates.truncate(limit);

        for (id, score) in candidates {
            if let Some(entity) = self.store.get_entity(&id)? {
                // 워크스페이스가 지정되면 그 안의 노드만(교차 워크스페이스 이웃 누출 방지).
                let in_ws =
                    workspace.is_none_or(|ws| entity.provenance.iter().any(|p| p.workspace == ws));
                if !in_ws {
                    continue;
                }
                results.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    id,
                    snippet: entity.canonical_name,
                    score,
                });
            }
        }

        // 전역 재정렬(점수 desc, id asc) 후 limit - 1차 히트와 이웃을 한 랭킹으로 통합한다.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        results.truncate(limit);
        Ok(results)
    }

    /// 엔티티에서 관계 방향(from->to)을 따라 최대 `max_depth` 홉까지 이웃을 순회한다.
    pub fn traverse(
        &self,
        id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError> {
        self.store.traverse(id, max_depth.max(1), limit)
    }

    /// 이 노드의 기본 워크스페이스(MCP 리소스가 구체 URI 를 만들 때 참조).
    pub fn default_workspace(&self) -> &str {
        &self.default_workspace
    }

    /// 지식이 존재하는 워크스페이스 목록(정렬, 결정적 - 원칙 16). 프로젝션된 그래프
    /// (엔티티/관계)의 provenance.workspace 에서 유도한다 - 그래프를 그릴 수 있는
    /// 워크스페이스의 집합이다. 별도 저장소 열거 없이 기존 읽기 포트만으로 계산한다.
    /// BTreeSet 이 중복 제거 + 정렬을 동시에 주어 도착 순서에 무관한 결과를 보장한다.
    pub fn workspaces(&self) -> Result<Vec<String>, StoreError> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        for e in self.store.all_entities(None)? {
            for p in &e.provenance {
                set.insert(p.workspace.clone());
            }
        }
        for r in self.store.all_relations(None)? {
            set.insert(r.provenance.workspace.clone());
        }
        Ok(set.into_iter().collect())
    }

    /// 온톨로지 그래프를 node-link 뷰로 프로젝션한다(관측가능성/시각화의 읽기 경로).
    /// 순수 읽기 - 관측 로그를 건드리지 않는다(원칙 1). 엣지는 양끝이 모두 노드 집합에
    /// 있을 때만 포함해 닫힌(렌더 가능한) 그래프를 준다. 노드/엣지 순서는 결정적이다(원칙 16).
    pub fn graph(&self, workspace: Option<&str>) -> Result<GraphView, StoreError> {
        let entities = self.store.all_entities(workspace)?;
        let relations = self.store.all_relations(workspace)?;

        let node_ids: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();

        // degree 는 그래프에 실제로 포함된 엣지 기준으로만 센다.
        let mut degree: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for r in &relations {
            if node_ids.contains(r.from.as_str()) && node_ids.contains(r.to.as_str()) {
                *degree.entry(r.from.clone()).or_default() += 1;
                *degree.entry(r.to.clone()).or_default() += 1;
                edges.push(GraphEdge {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    kind: r.kind.clone(),
                    trust_tier: r.provenance.trust_tier,
                    confidence: r.provenance.confidence,
                    valid_to: r.valid_to,
                });
            }
        }

        let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut trust_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut nodes: Vec<GraphNode> = entities
            .iter()
            .map(|e| {
                // 대표 신뢰 = 출처들 중 최고 등급(원칙 18). 출처 없으면 기본값.
                let trust = e
                    .provenance
                    .iter()
                    .map(|p| p.trust_tier)
                    .max()
                    .unwrap_or_default();
                *type_counts.entry(e.kind.clone()).or_default() += 1;
                *trust_counts.entry(tier_label(trust).to_string()).or_default() += 1;
                GraphNode {
                    id: e.id.clone(),
                    name: e.canonical_name.clone(),
                    kind: e.kind.clone(),
                    degree: degree.get(&e.id).copied().unwrap_or(0),
                    sources: e.provenance.len(),
                    trust_tier: trust,
                }
            })
            .collect();

        // 결정적 순서(원칙 16): 노드는 id, 엣지는 (from, kind, to) 로 안정 정렬.
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.to.cmp(&b.to))
        });

        let stats = GraphStats {
            node_count: nodes.len(),
            edge_count: edges.len(),
            type_counts,
            trust_counts,
        };
        Ok(GraphView {
            workspace: workspace.map(String::from),
            nodes,
            edges,
            stats,
        })
    }
}

/// 그래프 문맥 보강에서 이웃에 적용하는 감쇠. 이웃은 시드(직접 매치)보다 약한 신호이므로
/// 시드 점수의 절반으로 랭킹해 1차 히트 아래에 두되, 강한 시드의 이웃이 약한 1차 히트보다
/// 위로 오는 것은 허용한다(그래프 근접성을 랭킹에 반영).
const GRAPH_ENRICH_DECAY: f32 = 0.5;
/// 이웃을 확장할 시드(상위 엔티티 히트) 수 상한 - 비용/정밀도 제어(활발한 노드가 결과를
/// 뒤덮지 못하게 유계로 잡는다).
const GRAPH_ENRICH_SEEDS: usize = 5;

/// 엔티티를 임베딩할 텍스트: 정규명 + 별칭(있으면). 이름의 의미로 시맨틱 회상을 연다.
/// 별칭이 표기 변형을 담으므로 함께 임베딩하면 같은 대상의 다른 표기에도 도달 폭이 넓어진다.
fn entity_text(entity: &Entity) -> String {
    if entity.aliases.is_empty() {
        entity.canonical_name.clone()
    } else {
        format!("{} {}", entity.canonical_name, entity.aliases.join(" "))
    }
}

/// Reciprocal Rank Fusion. 스케일이 다른 랭킹들(키워드 score vs 코사인 유사도)을
/// 순위만으로 융합해 스케일 정규화 없이 합친다. 같은 (kind, id) 는 기여를 합산한다.
/// 결정적 함수(원칙 16) - 입력 순위가 같으면 어느 노드에서든 같은 결과.
fn fuse_rrf(lists: &[Vec<SearchHit>], limit: usize) -> Vec<SearchHit> {
    // RRF 상수. 클수록 상위 순위의 우위가 완만해진다(정보검색 관례값 60).
    const K: f32 = 60.0;

    let mut acc: HashMap<(SearchHitKind, String), (SearchHit, f32)> = HashMap::new();
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let contrib = 1.0 / (K + rank as f32 + 1.0);
            let entry = acc
                .entry((hit.kind, hit.id.clone()))
                .or_insert_with(|| (hit.clone(), 0.0));
            entry.1 += contrib;
        }
    }

    let mut fused: Vec<SearchHit> = acc
        .into_values()
        .map(|(mut hit, score)| {
            hit.score = score;
            hit
        })
        .collect();
    // 동점은 id 로 안정 정렬해 결정성을 보장한다.
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    fused.truncate(limit);
    fused
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_store::InMemoryStore;

    #[test]
    fn observe_then_get_and_search() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "test-host", "ws1");

        let out = engine
            .observe(ObserveInput {
                content: "rmcp is the official Rust MCP SDK".into(),
                workspace: None,
                source_ref: Some("docs/architecture.md".into()),
                confidence: None,
                on_behalf_of: Some("ashon".into()),
                derived_from: vec![],
                entities: vec![
                    EntityInput {
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                    EntityInput {
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                ],
                relations: vec![RelationInput {
                    from: "supragnosis".into(),
                    kind: "depends_on".into(),
                    to: "rmcp".into(),
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .unwrap();

        assert_eq!(out.entities.len(), 2);
        assert_eq!(out.relations.len(), 1);

        // 결정적 id로 재조회 -> 관계도 함께.
        let rmcp_id = Entity::make_id("ws1", "rmcp");
        let view = engine.get_entity(&rmcp_id).unwrap().expect("entity exists");
        assert_eq!(view.entity.canonical_name, "rmcp");
        assert_eq!(view.entity.kind, "Tool");
        assert_eq!(view.relations.len(), 1);

        // 재적재는 콘텐츠 주소라 동일 엔티티로 수렴(출처만 누적).
        let out = engine.search("rust", Some("ws1"), 10).unwrap();
        assert!(
            !out.hits.is_empty(),
            "keyword search should find the observation"
        );
        // 임베더 없는 엔진의 mode 는 keyword (degrade 표기, 원칙 16 4차).
        assert_eq!(out.mode, SearchMode::Keyword);

        // 다른 워크스페이스로는 안 보임.
        assert!(engine
            .search("rust", Some("other"), 10)
            .unwrap()
            .hits
            .is_empty());
    }

    /// 관계 kind 의 표기 요동(depends_on/dependsOn/depends-on)은 같은 엣지 하나로
    /// 수렴하고, 관측 로그에는 주장이 **원문 표기 그대로** 남는다 (원칙 1).
    #[test]
    fn relation_kind_variants_converge_and_assertions_are_logged() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "test-host", "ws1");

        let mut relation_ids = Vec::new();
        for kind in ["depends_on", "dependsOn", "depends-on"] {
            let out = engine
                .observe(ObserveInput {
                    content: format!("supragnosis {kind} rmcp"),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![],
                    relations: vec![RelationInput {
                        from: "supragnosis".into(),
                        kind: kind.into(),
                        to: "rmcp".into(),
                        valid_from: None,
                        valid_to: None,
                    }],
                })
                .unwrap();
            relation_ids.push(out.relations[0].clone());
        }
        // 세 표기가 전부 같은 관계 id.
        assert_eq!(relation_ids[0], relation_ids[1]);
        assert_eq!(relation_ids[0], relation_ids[2]);

        // 프로젝션에는 정준형 kind 하나만 존재.
        let sup_id = Entity::make_id("ws1", "supragnosis");
        let view = engine.get_entity(&sup_id).unwrap().unwrap();
        assert_eq!(view.relations.len(), 1);
        assert_eq!(view.relations[0].kind, "depends_on");
    }

    /// 구조화 주장은 관측 로그에 동봉되고 id 에 반영된다 - 같은 텍스트에 다른 주장을
    /// 실어도 dedup 으로 소실되지 않는다 (원칙 1: 로그만으로 그래프 재구성 가능).
    #[test]
    fn observations_carry_assertions_in_log() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "test-host", "ws1");

        let observe_with_kind = |kind: &str| {
            engine
                .observe(ObserveInput {
                    content: "supragnosis is written in Rust".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: "supragnosis".into(),
                        kind: Some(kind.into()),
                    }],
                    relations: vec![],
                })
                .unwrap()
        };
        let first = observe_with_kind("Tool");
        let second = observe_with_kind("Project");

        // 같은 텍스트라도 주장이 다르면 다른 관측 - 타입 재배정의 흔적이 로그에 남는다.
        assert_ne!(first.observation_id, second.observation_id);
        let logged = store.get_observation(&second.observation_id).unwrap().unwrap();
        assert_eq!(logged.assertions.entities.len(), 1);
        assert_eq!(logged.assertions.entities[0].kind.as_deref(), Some("Project"));
    }

    /// 원칙 2 스키마 수준 강제: 범위 밖 confidence 는 append-only 로그에 실리기 전에
    /// 거부되고, 에러 메시지가 자기 교정을 유도한다 (원칙 21).
    #[test]
    fn confidence_out_of_range_is_rejected() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let observe_with_conf = |conf: f32| {
            engine.observe(ObserveInput {
                content: "fact".into(),
                workspace: None,
                source_ref: None,
                confidence: Some(conf),
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![],
            })
        };
        for bad in [-0.1f32, 1.5, f32::NAN] {
            let err = observe_with_conf(bad).err().expect("범위 밖은 거부");
            assert!(
                err.to_string().contains("0.0~1.0"),
                "자기 교정 힌트가 있어야 한다: {err}"
            );
        }
        // 경계값은 유효하다.
        assert!(observe_with_conf(0.0).is_ok());
        assert!(observe_with_conf(1.0).is_ok());
    }

    /// 원칙 1 정형성 검증: 빈 지시어(이름/타입/끝점/kind)는 비주장이므로 영구 로그에
    /// 실리기 전에 거부된다. 반면 표기 요동(공백 둘러싼 이름, 구분자 변형 kind)은
    /// 내용이므로 통과한다 - 거부는 정형성까지, 표기는 검열하지 않는다.
    #[test]
    fn formless_assertions_are_rejected_before_logging() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let observe = |entities: Vec<EntityInput>, relations: Vec<RelationInput>| {
            engine.observe(ObserveInput {
                content: "fact".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities,
                relations,
            })
        };
        let ent = |name: &str, kind: Option<&str>| EntityInput {
            name: name.into(),
            kind: kind.map(String::from),
        };
        let rel = |from: &str, kind: &str, to: &str| RelationInput {
            from: from.into(),
            kind: kind.into(),
            to: to.into(),
            valid_from: None,
            valid_to: None,
        };

        // 비주장: 빈/공백 이름, 빈 타입, 빈 끝점, 정규화하면 빈 kind - 전부 거부.
        for (label, entities, relations) in [
            ("빈 이름", vec![ent("", None)], vec![]),
            ("공백 이름", vec![ent("   ", None)], vec![]),
            ("빈 타입", vec![ent("thing", Some(""))], vec![]),
            ("빈 from", vec![], vec![rel("", "depends_on", "b")]),
            ("공백 to", vec![], vec![rel("a", "depends_on", "  ")]),
            ("빈 kind", vec![], vec![rel("a", "", "b")]),
            ("구분자만 kind", vec![], vec![rel("a", "-- __ ", "b")]),
        ] {
            let err = observe(entities, relations)
                .err()
                .unwrap_or_else(|| panic!("{label} 은 거부돼야 한다"));
            assert!(
                matches!(err, ObserveError::Invalid(_)),
                "{label}: 검증 오류여야 한다: {err}"
            );
        }

        // 표기 요동은 내용이다 - 통과 (정규화/보존은 로그와 프로젝션의 일).
        assert!(observe(
            vec![ent("  Padded Name  ", Some("Tool"))],
            vec![rel("a", "Depends-On", "b")],
        )
        .is_ok());
    }

    /// 원칙 4 캡처: 소급 관측("지난달까지 참이었다")의 유효구간이 관측 로그(주장)와
    /// 프로젝션(관계) 양쪽에 실린다. 표면이 못 받으면 로그에 없고, 로그에 없으면
    /// 재프로젝션으로도 복원할 수 없다 - 캡처는 이연할 수 없는 이유.
    #[test]
    fn relation_valid_interval_is_captured_in_log_and_projection() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "h", "ws1");

        let observe_with_interval = |valid_to: Option<Timestamp>| {
            engine
                .observe(ObserveInput {
                    content: "kim led team A until last month".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![],
                    relations: vec![RelationInput {
                        from: "kim".into(),
                        kind: "leads".into(),
                        to: "team A".into(),
                        valid_from: Some(100),
                        valid_to,
                    }],
                })
                .unwrap()
        };
        let out = observe_with_interval(Some(200));

        // 로그: 주장에 유효구간이 원문 그대로 동봉된다.
        let logged = store.get_observation(&out.observation_id).unwrap().unwrap();
        assert_eq!(logged.assertions.relations[0].valid_from, Some(100));
        assert_eq!(logged.assertions.relations[0].valid_to, Some(200));

        // 프로젝션: 관계가 유효구간을 지닌다.
        let kim = Entity::make_id("ws1", "kim");
        let rels = store.relations_of(&kim).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].valid_from, Some(100));
        assert_eq!(rels[0].valid_to, Some(200));

        // 유효구간이 다르면 다른 주장 - 다른 관측 id (내용 정체성에 포함).
        let out2 = observe_with_interval(None);
        assert_ne!(out.observation_id, out2.observation_id);
    }

    /// 원칙 3: 같은 content 재관측 시 로그가 attestation 을 모두 보존한다 - 엔티티
    /// provenance 만 누적되고 로그는 마지막 1건이던 "진실의 원천 역전" 회귀를 막는다
    /// (로그 재프로젝션으로 그래프의 attestation 을 복원할 수 있어야 한다).
    #[test]
    fn log_retains_all_attestations_on_reobservation() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "host", "ws1");

        let observe = |behalf: &str, conf: f32| {
            engine
                .observe(ObserveInput {
                    content: "repeated fact".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: Some(conf),
                    on_behalf_of: Some(behalf.into()),
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: "thing".into(),
                        kind: None,
                    }],
                    relations: vec![],
                })
                .unwrap()
        };
        let first = observe("alice", 0.9);
        let second = observe("bob", 0.1);
        assert_eq!(first.observation_id, second.observation_id, "콘텐츠 주소 dedup");

        let logged = store.get_observation(&first.observation_id).unwrap().unwrap();
        let entity = store
            .get_entity(&Entity::make_id("ws1", "thing"))
            .unwrap()
            .unwrap();

        // 로그와 프로젝션이 같은 attestation 수를 지닌다 - 로그가 진실의 원천.
        assert_eq!(logged.provenance.len(), 2, "로그에 두 attestation 보존");
        assert_eq!(entity.provenance.len(), 2);
        let behalfs: Vec<Option<String>> = logged
            .provenance
            .iter()
            .map(|p| p.on_behalf_of.clone())
            .collect();
        assert!(
            behalfs.contains(&Some("alice".into())) && behalfs.contains(&Some("bob".into())),
            "첫 관측의 provenance 가 파괴되면 안 된다: {behalfs:?}"
        );
    }

    fn observe_text(engine: &Engine, content: &str) {
        engine
            .observe(ObserveInput {
                content: content.into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![],
            })
            .unwrap();
    }

    /// 회상 회귀(부록 B): 임베더를 붙이면 하이브리드 검색이 키워드 부분일치가 놓치는
    /// 관측을 의미(어휘 중첩) 경로로 회상한다. degrade(임베더 없음)와 대조한다.
    #[test]
    fn hybrid_search_adds_semantic_recall() {
        use supragnosis_embed::HashingEmbedder;

        let store = Arc::new(InMemoryStore::new());
        let hybrid = Engine::new(store.clone(), "h", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default()));

        observe_text(&hybrid, "rust compiler emits fast native binaries");
        observe_text(&hybrid, "python interpreter runs bytecode");
        observe_text(&hybrid, "banana bread recipe with walnuts");

        // 질의는 어느 관측의 부분문자열도 아니다(단어 순서/형태가 다름).
        let q = "native binary compiler";

        // 키워드 전용(같은 저장소, 임베더 없음)은 이 질의를 놓친다.
        let keyword_only = Engine::new(store.clone(), "h", "ws");
        let keyword_out = keyword_only.search(q, Some("ws"), 10).unwrap();
        assert!(
            keyword_out.hits.is_empty(),
            "substring keyword search should miss this query"
        );
        assert_eq!(keyword_out.mode, SearchMode::Keyword, "degrade 는 keyword 표기");

        // 하이브리드는 어휘가 겹치는 rust 관측을 최상위로 회상한다.
        let out = hybrid.search(q, Some("ws"), 10).unwrap();
        assert_eq!(out.mode, SearchMode::Hybrid, "의미 표면 참조 시 hybrid 표기");
        let hits = out.hits;
        assert!(
            !hits.is_empty(),
            "hybrid search should recall via embedding"
        );
        assert!(
            hits[0].snippet.contains("native"),
            "semantic top hit should be the rust observation, got {:?}",
            hits.first()
        );
    }

    /// 원칙 19 degrade: 임베딩 어댑터가 매 호출 실패해도 적재는 막히지 않고
    /// (best-effort 부착 - 실패는 로그로만 알린다), 검색은 키워드 전용으로
    /// degrade 하며 mode 로 그 사실을 표기한다.
    #[test]
    fn embed_failure_degrades_without_blocking_ingest() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};

        struct FailingEmbedder;
        impl EmbeddingProvider for FailingEmbedder {
            fn dimensions(&self) -> usize {
                3
            }
            fn id(&self) -> String {
                "failing-3".into()
            }
            fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                Err(EmbedError::Provider("simulated failure".into()))
            }
        }

        let store = Arc::new(InMemoryStore::new());
        let engine =
            Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(FailingEmbedder));

        let out = engine
            .observe(ObserveInput {
                content: "rust compiles fast".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput {
                    name: "rust".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
            })
            .expect("임베딩 실패가 적재를 막으면 안 된다 (원칙 19: degrade)");

        // 관측/엔티티 모두 임베딩 없이 저장된다.
        let obs = store.get_observation(&out.observation_id).unwrap().unwrap();
        assert!(obs.embedding.is_none());
        let ent = store.get_entity(&out.entities[0]).unwrap().unwrap();
        assert!(ent.embedding.is_none());

        // 질의 임베딩도 실패하므로 검색은 keyword 로 degrade 하되 동작한다.
        let found = engine.search("rust", Some("ws1"), 10).unwrap();
        assert_eq!(found.mode, SearchMode::Keyword);
        assert!(!found.hits.is_empty());
    }

    /// 그래프 프로젝션: 관측이 만든 엔티티/관계를 node-link 뷰로 되돌린다.
    /// 워크스페이스 스코핑, 닫힌 그래프(고아 엣지 배제), degree/stats, 결정적 순서를 검증한다.
    #[test]
    fn graph_projection_nodes_edges_stats() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "ws1");

        // ws1: supragnosis --depends_on--> rmcp (엔티티 2, 관계 1).
        engine
            .observe(ObserveInput {
                content: "supragnosis depends on rmcp".into(),
                workspace: Some("ws1".into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput {
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                    EntityInput {
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                ],
                relations: vec![RelationInput {
                    from: "supragnosis".into(),
                    kind: "depends_on".into(),
                    to: "rmcp".into(),
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .unwrap();

        // 다른 워크스페이스의 지식 - ws1 그래프에 새면 안 된다.
        engine
            .observe(ObserveInput {
                content: "unrelated".into(),
                workspace: Some("other".into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput {
                    name: "elsewhere".into(),
                    kind: None,
                }],
                relations: vec![],
            })
            .unwrap();

        let g = engine.graph(Some("ws1")).unwrap();
        assert_eq!(g.stats.node_count, 2, "ws1 노드 2개");
        assert_eq!(g.stats.edge_count, 1, "ws1 엣지 1개");

        // 노드는 id 로 결정적 정렬.
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "노드는 id 오름차순(결정성)");

        // 관계 양끝의 degree 는 각각 1.
        for n in &g.nodes {
            assert_eq!(n.degree, 1, "각 노드는 관계 1개에 연결: {}", n.name);
        }

        // 엣지는 depends_on, 양끝이 모두 노드 집합에 있다.
        let e = &g.edges[0];
        assert_eq!(e.kind, "depends_on");
        assert!(ids.contains(&e.from.as_str()) && ids.contains(&e.to.as_str()));

        // 타입 분포.
        assert_eq!(g.stats.type_counts.get("Project"), Some(&1));
        assert_eq!(g.stats.type_counts.get("Tool"), Some(&1));

        // 워크스페이스 격리: other 의 엔티티는 없다.
        assert!(
            g.nodes.iter().all(|n| n.name != "elsewhere"),
            "다른 워크스페이스 노드가 새면 안 된다"
        );

        // 전체(None)로는 other 까지 포함해 노드 3개.
        assert_eq!(engine.graph(None).unwrap().stats.node_count, 3);
    }

    /// workspaces(): 지식이 존재하는 워크스페이스를 중복 없이 정렬해 돌려준다(원칙 16).
    #[test]
    fn workspaces_lists_distinct_sorted() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "alpha");

        assert!(engine.workspaces().unwrap().is_empty(), "빈 상태는 빈 목록");

        let observe_in = |ws: &str, name: &str| {
            engine
                .observe(ObserveInput {
                    content: format!("{name} in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: name.into(),
                        kind: None,
                    }],
                    relations: vec![],
                })
                .unwrap();
        };
        // 도착 순서를 일부러 뒤섞고 같은 ws 를 중복 적재한다.
        observe_in("gamma", "x");
        observe_in("alpha", "y");
        observe_in("gamma", "z");

        // 중복 제거 + 정렬(도착 순서 무관).
        assert_eq!(engine.workspaces().unwrap(), vec!["alpha", "gamma"]);
    }
}
