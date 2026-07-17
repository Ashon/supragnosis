//! supragnosis-engine — 서비스(유스케이스) 계층.
//!
//! MCP 도구가 호출하는 결정론적 로직: 관측 적재 → 엔티티 해소 → 관계 링크 → 조회/검색.
//! 저장소는 [`supragnosis_core::KnowledgeStore`] 포트를 통해서만 접근한다.

use std::sync::Arc;

use serde::Serialize;
use supragnosis_core::{
    now_millis, Entity, KnowledgeStore, Observation, Provenance, Relation, SearchHit, StoreError,
    TraverseHit,
};

/// 적재 입력 (전송 DTO에서 매핑되는 도메인 입력).
pub struct ObserveInput {
    pub content: String,
    pub workspace: Option<String>,
    pub source_ref: Option<String>,
    pub confidence: Option<f32>,
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
            host: host.into(),
            default_workspace: default_workspace.into(),
        }
    }

    fn provenance(
        &self,
        workspace: &str,
        source_ref: Option<String>,
        confidence: Option<f32>,
    ) -> Provenance {
        Provenance {
            host: self.host.clone(),
            workspace: workspace.to_string(),
            source_ref,
            observed_at: now_millis(),
            confidence: confidence.unwrap_or(1.0),
        }
    }

    /// 지식 조각을 적재한다: 불변 관측 저장 + 제공된 엔티티/관계를 온톨로지에 링크.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, StoreError> {
        let workspace = input
            .workspace
            .unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(&workspace, input.source_ref, input.confidence);

        let obs = Observation::new(input.content, prov.clone());
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

    pub fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit> {
        self.store.search(query, workspace, limit)
    }

    /// 엔티티에서 관계 방향(from→to)을 따라 최대 `max_depth` 홉까지 이웃을 순회한다.
    pub fn traverse(&self, id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit> {
        self.store.traverse(id, max_depth.max(1), limit)
    }
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
                entities: vec![
                    EntityInput { name: "rmcp".into(), kind: Some("Tool".into()) },
                    EntityInput { name: "supragnosis".into(), kind: Some("Project".into()) },
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

        // 결정적 id로 재조회 → 관계도 함께.
        let rmcp_id = Entity::make_id("ws1", "rmcp");
        let view = engine.get_entity(&rmcp_id).expect("entity exists");
        assert_eq!(view.entity.canonical_name, "rmcp");
        assert_eq!(view.entity.kind, "Tool");
        assert_eq!(view.relations.len(), 1);

        // 재적재는 콘텐츠 주소라 동일 엔티티로 수렴(출처만 누적).
        let hits = engine.search("rust", Some("ws1"), 10);
        assert!(!hits.is_empty(), "keyword search should find the observation");

        // 다른 워크스페이스로는 안 보임.
        assert!(engine.search("rust", Some("other"), 10).is_empty());
    }
}
