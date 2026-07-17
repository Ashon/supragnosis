//! supragnosis-store - 저장소 어댑터.
//!
//! 같은 [`supragnosis_core::KnowledgeStore`] 포트를 두 어댑터가 구현한다:
//! 프로세스 메모리 기반 `InMemoryStore`(테스트/비영속)와 Cozo(RocksDB) 기반 `CozoStore`(파일 영속).

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use supragnosis_core::{
    Entity, KnowledgeStore, Observation, Relation, SearchHit, SearchHitKind, StoreError,
    TraverseHit,
};

mod cozo_store;
pub use cozo_store::CozoStore;

/// 메모리 기반 지식 저장소. 테스트/개발/비영속 실행용.
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
        self.observations
            .write()
            .unwrap()
            .insert(obs.id.clone(), obs);
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Option<Entity> {
        self.entities.read().unwrap().get(id).cloned()
    }

    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        self.entities
            .write()
            .unwrap()
            .insert(entity.id.clone(), entity);
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
            let in_ws = workspace.is_none_or(|ws| e.provenance.iter().any(|p| p.workspace == ws));
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
            let in_ws = workspace.is_none_or(|ws| o.provenance.workspace == ws);
            if in_ws && o.content.to_lowercase().contains(&q) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id.clone(),
                    snippet: o.content.chars().take(160).collect(),
                    score: 0.7,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        hits
    }

    fn traverse(&self, start_id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit> {
        let relations = self.relations.read().unwrap();
        let entities = self.entities.read().unwrap();

        let mut out: Vec<TraverseHit> = Vec::new();
        let mut visited: HashSet<String> = HashSet::from([start_id.to_string()]);
        let mut frontier: Vec<String> = vec![start_id.to_string()];

        let mut depth = 1usize;
        while depth <= max_depth && !frontier.is_empty() {
            let mut next = Vec::new();
            for node in &frontier {
                for r in relations.values().filter(|r| &r.from == node) {
                    if visited.insert(r.to.clone()) {
                        let (name, kind) = entities
                            .get(&r.to)
                            .map(|e| (e.canonical_name.clone(), e.kind.clone()))
                            .unwrap_or_default();
                        out.push(TraverseHit {
                            id: r.to.clone(),
                            depth,
                            name,
                            kind,
                        });
                        next.push(r.to.clone());
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
            frontier = next;
            depth += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::{Provenance, TrustTier};

    fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: "ws1".into(),
            source_ref: None,
            observed_at: 1,
            confidence: 1.0,
            trust_tier: TrustTier::default(),
        }
    }

    fn ent(name: &str) -> Entity {
        Entity {
            id: Entity::make_id("ws1", name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            properties: serde_json::Value::Null,
            provenance: vec![prov()],
        }
    }

    /// InMemoryStore 어댑터 직접 검증 - Cozo 어댑터와 동작 parity를 맞춘다.
    #[test]
    fn in_memory_get_relations_search_traverse() {
        let store = InMemoryStore::new();

        // 열린 세계(원칙 5): 없는 엔티티는 None (에러 아님).
        assert!(store.get_entity("missing").is_none());

        for n in ["a", "b", "c"] {
            store.put_entity(ent(n)).unwrap();
        }
        let (a, b, c) = (
            Entity::make_id("ws1", "a"),
            Entity::make_id("ws1", "b"),
            Entity::make_id("ws1", "c"),
        );
        for (from, to) in [(&a, &b), (&b, &c)] {
            store
                .add_relation(Relation {
                    id: Relation::make_id(from, "rel", to),
                    from: from.clone(),
                    to: to.clone(),
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
        }
        store
            .add_observation(Observation::new("hello rust world".into(), prov()))
            .unwrap();

        // 조회 + 관계(b는 a->b, b->c 두 관계에 등장).
        assert_eq!(store.get_entity(&a).unwrap().canonical_name, "a");
        assert_eq!(store.relations_of(&b).len(), 2);

        // 검색: 워크스페이스 스코프.
        assert!(!store.search("rust", Some("ws1"), 10).is_empty());
        assert!(store.search("rust", Some("other"), 10).is_empty());

        // 순회: a -> b(1홉), c(2홉). 방향(from->to)을 따른다.
        let hits = store.traverse(&a, 5, 100);
        let by_id: HashMap<_, _> = hits.iter().map(|h| (h.id.clone(), h.depth)).collect();
        assert_eq!(by_id.get(&b), Some(&1));
        assert_eq!(by_id.get(&c), Some(&2));
    }
}
