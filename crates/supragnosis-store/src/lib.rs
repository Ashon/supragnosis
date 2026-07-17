//! supragnosis-store — 저장소 어댑터.
//!
//! M0: 프로세스 메모리 기반 `InMemoryStore`. M1에서 Cozo(RocksDB) 어댑터를 같은 포트로 추가한다.

use std::collections::HashMap;
use std::sync::RwLock;

use supragnosis_core::{
    Entity, KnowledgeStore, Observation, Relation, SearchHit, SearchHitKind, StoreError,
};

/// 메모리 기반 지식 저장소. 테스트/개발/M0 골격용.
#[derive(Default)]
pub struct InMemoryStore {
    observations: RwLock<HashMap<String, Observation>>,
    entities: RwLock<HashMap<String, Entity>>,
    relations: RwLock<HashMap<String, Relation>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KnowledgeStore for InMemoryStore {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        self.observations.write().unwrap().insert(obs.id.clone(), obs);
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Option<Entity> {
        self.entities.read().unwrap().get(id).cloned()
    }

    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        self.entities.write().unwrap().insert(entity.id.clone(), entity);
        Ok(())
    }

    fn add_relation(&self, rel: Relation) -> Result<(), StoreError> {
        self.relations.write().unwrap().insert(rel.id.clone(), rel);
        Ok(())
    }

    fn relations_of(&self, entity_id: &str) -> Vec<Relation> {
        self.relations
            .read()
            .unwrap()
            .values()
            .filter(|r| r.from == entity_id || r.to == entity_id)
            .cloned()
            .collect()
    }

    fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<SearchHit> = Vec::new();

        // 엔티티: 정규명/별칭 부분일치.
        for e in self.entities.read().unwrap().values() {
            let in_ws = workspace.map_or(true, |ws| e.provenance.iter().any(|p| p.workspace == ws));
            if !in_ws {
                continue;
            }
            let name_hit = e.canonical_name.to_lowercase().contains(&q)
                || e.aliases.iter().any(|a| a.to_lowercase().contains(&q));
            if name_hit {
                hits.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    id: e.id.clone(),
                    snippet: e.canonical_name.clone(),
                    score: 1.0,
                });
            }
        }

        // 관측: 본문 부분일치.
        for o in self.observations.read().unwrap().values() {
            let in_ws = workspace.map_or(true, |ws| o.provenance.workspace == ws);
            if in_ws && o.content.to_lowercase().contains(&q) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id.clone(),
                    snippet: o.content.chars().take(160).collect(),
                    score: 0.7,
                });
            }
        }

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        hits
    }
}
