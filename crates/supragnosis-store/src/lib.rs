//! supragnosis-store - 저장소 어댑터.
//!
//! 같은 [`supragnosis_core::KnowledgeStore`] 포트를 두 어댑터가 구현한다:
//! 프로세스 메모리 기반 `InMemoryStore`(테스트/비영속)와 Cozo(RocksDB) 기반 `CozoStore`(파일 영속).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use supragnosis_core::{
    cosine_similarity, Entity, KnowledgeStore, Observation, Relation, SearchHit, SearchHitKind,
    StoreError, TraverseHit,
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

    /// 관측 로그에서 id 로 관측을 꺼낸다 (테스트/검사용 - 로그가 진실의 원천임을 검증).
    pub fn observation(&self, id: &str) -> Option<Observation> {
        self.observations.read().unwrap().get(id).cloned()
    }
}

impl KnowledgeStore for InMemoryStore {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        // 같은 콘텐츠 주소의 재도착은 덮어쓰기가 아니라 단조 합집합으로 흡수한다
        // (원칙 3: 로그 불변 - provenance/계보가 파괴되지 않는다).
        match self.observations.write().unwrap().entry(obs.id.clone()) {
            Entry::Occupied(mut e) => e.get_mut().absorb(obs),
            Entry::Vacant(v) => {
                v.insert(obs);
            }
        }
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
        let mut rels: Vec<Relation> = self
            .relations
            .read()
            .unwrap()
            .values()
            .filter(|r| r.from == entity_id || r.to == entity_id)
            .cloned()
            .collect();
        // id 정렬 - HashMap 반복 순서가 응답에 새지 않게 한다(원칙 16: 재현성).
        rels.sort_by(|a, b| a.id.cmp(&b.id));
        rels
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
            let in_ws = workspace.is_none_or(|ws| o.workspace() == ws);
            if in_ws && o.content.to_lowercase().contains(&q) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id.clone(),
                    snippet: o.content.chars().take(160).collect(),
                    score: 0.7,
                });
            }
        }

        // 동점은 id 로 안정 정렬 - HashMap 반복 순서가 결과에 새지 않게 한다(원칙 16: 재현성).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
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
            // depth 단위로 이웃을 모아 id 정렬 후 방문/방출한다 - 출력은 (depth, id)
            // 순서로 결정적이고, limit 절단도 가까운 이웃(얕은 depth) 우선으로
            // 재현 가능하다 (원칙 16). HashMap 순회 순서가 결과에 새지 않는다.
            let mut next: Vec<String> = frontier
                .iter()
                .flat_map(|node| {
                    relations
                        .values()
                        .filter(move |r| &r.from == node)
                        .map(|r| r.to.clone())
                })
                .filter(|to| !visited.contains(to))
                .collect();
            next.sort_unstable();
            next.dedup();

            for to in &next {
                visited.insert(to.clone());
                let (name, kind) = entities
                    .get(to)
                    .map(|e| (e.canonical_name.clone(), e.kind.clone()))
                    .unwrap_or_default();
                out.push(TraverseHit {
                    id: to.clone(),
                    depth,
                    name,
                    kind,
                });
                if out.len() >= limit {
                    return out;
                }
            }
            frontier = next;
            depth += 1;
        }
        out
    }

    fn all_entities(&self, workspace: Option<&str>) -> Vec<Entity> {
        self.entities
            .read()
            .unwrap()
            .values()
            .filter(|e| {
                workspace.is_none_or(|ws| e.provenance.iter().any(|p| p.workspace == ws))
            })
            .cloned()
            .collect()
    }

    fn all_relations(&self, workspace: Option<&str>) -> Vec<Relation> {
        self.relations
            .read()
            .unwrap()
            .values()
            .filter(|r| workspace.is_none_or(|ws| r.provenance.workspace == ws))
            .cloned()
            .collect()
    }

    fn search_semantic(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Vec<SearchHit> {
        let mut hits: Vec<SearchHit> = self
            .observations
            .read()
            .unwrap()
            .values()
            .filter(|o| workspace.is_none_or(|ws| o.workspace() == ws))
            .filter_map(|o| {
                let emb = o.embedding.as_deref()?;
                Some(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id.clone(),
                    snippet: o.content.chars().take(160).collect(),
                    score: cosine_similarity(query_embedding, emb),
                })
            })
            .collect();

        // 동점은 id 로 안정 정렬 - HashMap 반복 순서가 결과에 새지 않게 한다(원칙 16: 재현성).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
    }

    fn search_semantic_entities(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Vec<SearchHit> {
        let mut hits: Vec<SearchHit> = self
            .entities
            .read()
            .unwrap()
            .values()
            .filter(|e| workspace.is_none_or(|ws| e.provenance.iter().any(|p| p.workspace == ws)))
            .filter_map(|e| {
                let emb = e.embedding.as_deref()?;
                Some(SearchHit {
                    kind: SearchHitKind::Entity,
                    id: e.id.clone(),
                    snippet: e.canonical_name.clone(),
                    score: cosine_similarity(query_embedding, emb),
                })
            })
            .collect();

        // 동점은 id 로 안정 정렬 - HashMap 반복 순서가 결과에 새지 않게 한다(원칙 16: 재현성).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        hits
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
            embedding: None,
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

    /// 원칙 3: 같은 콘텐츠 주소의 재도착은 attestation/계보 합집합으로 흡수되고
    /// (덮어쓰기 금지), 도착 순서와 무관하게 같은 로그로 수렴한다 (원칙 16).
    #[test]
    fn reobservation_accumulates_attestations() {
        let make = |host: &str, conf: f32, derived: &str| {
            let mut o = Observation::new(
                "same fact".into(),
                Provenance {
                    host: host.into(),
                    confidence: conf,
                    ..prov()
                },
            );
            o.derived_from = vec![derived.into()];
            o
        };

        let forward = InMemoryStore::new();
        forward.add_observation(make("host-a", 0.9, "o1")).unwrap();
        forward.add_observation(make("host-b", 0.1, "o2")).unwrap();

        let reverse = InMemoryStore::new();
        reverse.add_observation(make("host-b", 0.1, "o2")).unwrap();
        reverse.add_observation(make("host-a", 0.9, "o1")).unwrap();

        let id = make("host-a", 0.9, "o1").id;
        let f = forward.observation(&id).unwrap();
        let r = reverse.observation(&id).unwrap();

        // 두 attestation 이 모두 보존된다 - 첫 관측이 파괴되지 않는다.
        assert_eq!(f.provenance.len(), 2, "attestation 누적: {:?}", f.provenance);
        assert_eq!(f.derived_from, vec!["o1".to_string(), "o2".to_string()]);

        // 도착 순서 무관 수렴.
        let hosts = |o: &Observation| -> Vec<String> {
            o.provenance.iter().map(|p| p.host.clone()).collect()
        };
        assert_eq!(hosts(&f), hosts(&r));
        assert_eq!(f.derived_from, r.derived_from);
    }
}
