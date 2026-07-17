//! supragnosis-engine - 서비스(유스케이스) 계층.
//!
//! MCP 도구가 호출하는 결정론적 로직: 관측 적재 -> 엔티티 해소 -> 관계 링크 -> 조회/검색.
//! 저장소는 [`supragnosis_core::KnowledgeStore`] 포트를 통해서만 접근한다.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use supragnosis_core::{
    now_millis, EmbeddingProvider, Entity, KnowledgeStore, Observation, Provenance, Relation,
    SearchHit, SearchHitKind, StoreError, TraverseHit, TrustTier,
};

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
}

#[derive(Serialize)]
pub struct ObserveOutput {
    pub observation_id: String,
    pub entities: Vec<String>,
    pub relations: Vec<String>,
}

/// 엔티티 + 그 관계(조회 응답).
#[derive(Serialize)]
pub struct EntityView {
    #[serde(flatten)]
    pub entity: Entity,
    pub relations: Vec<Relation>,
}

pub struct Engine {
    store: Arc<dyn KnowledgeStore>,
    /// 임베딩 공급 포트 (원칙 19: 확률적 경계). 없으면 검색은 키워드로 degrade.
    embedder: Option<Arc<dyn EmbeddingProvider>>,
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
            confidence: confidence.unwrap_or(1.0),
            // 신뢰 등급 승격은 명시적 흐름(사람 확인/교차검증)에서만 - observe 는 기본값.
            trust_tier: TrustTier::default(),
        }
    }

    /// 지식 조각을 적재한다: 불변 관측 저장 + 제공된 엔티티/관계를 온톨로지에 링크.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, StoreError> {
        let workspace = input
            .workspace
            .unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(
            &workspace,
            input.source_ref,
            input.confidence,
            input.on_behalf_of,
        );

        let mut obs = Observation::new(input.content, prov.clone());
        obs.derived_from = input.derived_from;
        // 임베딩 부착은 best-effort: 실패해도 관측 저장은 막지 않는다(원칙 19: degrade).
        if let Some(embedder) = &self.embedder {
            if let Ok(vec) = embedder.embed_one(&obs.content) {
                obs.embedding = Some(vec);
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
            let rel = Relation {
                id: Relation::make_id(&from, &r.kind, &to),
                from,
                to,
                kind: r.kind,
                provenance: prov.clone(),
                // 유효구간은 M3 에서 관측/반증으로부터 유도. 지금은 미지정(관측시점부터).
                valid_from: None,
                valid_to: None,
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
        let mut entity = self.store.get_entity(&id).unwrap_or_else(|| Entity {
            id: id.clone(),
            kind: kind.clone().unwrap_or_else(|| "Concept".to_string()),
            canonical_name: name.trim().to_string(),
            aliases: Vec::new(),
            properties: serde_json::Value::Null,
            provenance: Vec::new(),
        });
        if let Some(k) = kind {
            entity.kind = k;
        }
        entity.provenance.push(prov.clone());
        self.store.put_entity(entity)?;
        Ok(id)
    }

    pub fn get_entity(&self, id: &str) -> Option<EntityView> {
        self.store.get_entity(id).map(|entity| {
            let relations = self.store.relations_of(id);
            EntityView { entity, relations }
        })
    }

    /// 하이브리드 검색: 키워드(부분일치) + 벡터(의미) 결과를 RRF 로 융합한다.
    /// 임베더가 없거나 질의 임베딩에 실패하면 키워드 결과만 돌려준다(원칙 19: degrade).
    /// 벡터는 회상(recall)을 넓히는 데만 쓰고 확정 랭킹은 결정적 융합으로 한다.
    pub fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit> {
        let keyword = self.store.search(query, workspace, limit);

        let semantic = self
            .embedder
            .as_ref()
            .and_then(|e| e.embed_one(query).ok())
            .map(|qvec| self.store.search_semantic(&qvec, workspace, limit))
            .unwrap_or_default();

        if semantic.is_empty() {
            return keyword;
        }
        fuse_rrf(&[keyword, semantic], limit)
    }

    /// 엔티티에서 관계 방향(from->to)을 따라 최대 `max_depth` 홉까지 이웃을 순회한다.
    pub fn traverse(&self, id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit> {
        self.store.traverse(id, max_depth.max(1), limit)
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
                }],
            })
            .unwrap();

        assert_eq!(out.entities.len(), 2);
        assert_eq!(out.relations.len(), 1);

        // 결정적 id로 재조회 -> 관계도 함께.
        let rmcp_id = Entity::make_id("ws1", "rmcp");
        let view = engine.get_entity(&rmcp_id).expect("entity exists");
        assert_eq!(view.entity.canonical_name, "rmcp");
        assert_eq!(view.entity.kind, "Tool");
        assert_eq!(view.relations.len(), 1);

        // 재적재는 콘텐츠 주소라 동일 엔티티로 수렴(출처만 누적).
        let hits = engine.search("rust", Some("ws1"), 10);
        assert!(
            !hits.is_empty(),
            "keyword search should find the observation"
        );

        // 다른 워크스페이스로는 안 보임.
        assert!(engine.search("rust", Some("other"), 10).is_empty());
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
        assert!(
            keyword_only.search(q, Some("ws"), 10).is_empty(),
            "substring keyword search should miss this query"
        );

        // 하이브리드는 어휘가 겹치는 rust 관측을 최상위로 회상한다.
        let hits = hybrid.search(q, Some("ws"), 10);
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
}
