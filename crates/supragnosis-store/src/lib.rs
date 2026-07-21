//! supragnosis-store - storage adapter.
//!
//! Two adapters implement the same [`supragnosis_core::KnowledgeStore`] port:
//! the process-memory `InMemoryStore` (test/non-persistent) and the Cozo(RocksDB)-backed `CozoStore` (file-persistent).

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use supragnosis_core::{
    cosine_similarity, Entity, KnowledgeStore, Observation, Relation, SearchHit, SearchHitKind,
    StoreError, TraverseHit,
};

mod cozo_store;
pub use cozo_store::CozoStore;

/// In-memory knowledge store. For test/development/non-persistent runs.
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
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        Ok(self.observations.read().unwrap().get(id).cloned())
    }

    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        // A re-arrival at the same content address is absorbed as a monotonic union, not an overwrite
        // (Principle 3: log immutability - provenance/lineage is not destroyed).
        match self.observations.write().unwrap().entry(obs.id.clone()) {
            Entry::Occupied(mut e) => e.get_mut().absorb(obs),
            Entry::Vacant(v) => {
                v.insert(obs);
            }
        }
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError> {
        Ok(self.entities.read().unwrap().get(id).cloned())
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

    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError> {
        let mut rels: Vec<Relation> = self
            .relations
            .read()
            .unwrap()
            .values()
            .filter(|r| r.from == entity_id || r.to == entity_id)
            .cloned()
            .collect();
        // Sort by id - keep HashMap iteration order from leaking into the response (Principle 16: reproducibility).
        rels.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(rels)
    }

    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<SearchHit> = Vec::new();

        // Entity: substring match on canonical name/alias.
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

        // Observation: substring match on content.
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

        // Break ties by id for a stable sort - keep HashMap iteration order from leaking into results (Principle 16: reproducibility).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn traverse(
        &self,
        start_id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError> {
        let relations = self.relations.read().unwrap();
        let entities = self.entities.read().unwrap();

        let mut out: Vec<TraverseHit> = Vec::new();
        let mut visited: HashSet<String> = HashSet::from([start_id.to_string()]);
        let mut frontier: Vec<String> = vec![start_id.to_string()];

        let mut depth = 1usize;
        while depth <= max_depth && !frontier.is_empty() {
            // Gather neighbors per depth, sort by id, then visit/emit - output is deterministic
            // in (depth, id) order, and limit truncation is reproducible with nearer neighbors
            // (shallower depth) preferred (Principle 16). HashMap traversal order does not leak into results.
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
                    return Ok(out);
                }
            }
            frontier = next;
            depth += 1;
        }
        Ok(out)
    }

    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError> {
        Ok(self
            .entities
            .read()
            .unwrap()
            .values()
            .filter(|e| {
                workspace.is_none_or(|ws| e.provenance.iter().any(|p| p.workspace == ws))
            })
            .cloned()
            .collect())
    }

    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError> {
        Ok(self
            .relations
            .read()
            .unwrap()
            .values()
            .filter(|r| workspace.is_none_or(|ws| r.provenance.workspace == ws))
            .cloned()
            .collect())
    }

    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        Ok(self
            .observations
            .read()
            .unwrap()
            .values()
            .filter(|o| workspace.is_none_or(|ws| o.workspace() == ws))
            .cloned()
            .collect())
    }

    fn search_semantic(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
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

        // Break ties by id for a stable sort - keep HashMap iteration order from leaking into results (Principle 16: reproducibility).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn search_semantic_entities(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
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

        // Break ties by id for a stable sort - keep HashMap iteration order from leaking into results (Principle 16: reproducibility).
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        hits.truncate(limit);
        Ok(hits)
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
            confidence: Some(1.0),
            trust_tier: TrustTier::default(),
        }
    }

    fn ent(name: &str) -> Entity {
        Entity {
            id: Entity::make_id("ws1", name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            description: None,
            properties: serde_json::Value::Null,
            provenance: vec![prov()],
            embedding: None,
        }
    }

    /// Directly verifies the InMemoryStore adapter - matches behavior parity with the Cozo adapter.
    #[test]
    fn in_memory_get_relations_search_traverse() {
        let store = InMemoryStore::new();

        // Open world (Principle 5): a missing entity is Ok(None) (absence is not an error - Err is for backend failure only).
        assert!(store.get_entity("missing").unwrap().is_none());

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
                .add_relation(Relation { description: None,
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

        // Lookup + relations (b appears in two relations, a->b and b->c).
        assert_eq!(
            store.get_entity(&a).unwrap().unwrap().canonical_name,
            "a"
        );
        assert_eq!(store.relations_of(&b).unwrap().len(), 2);

        // Search: workspace scope.
        assert!(!store.search("rust", Some("ws1"), 10).unwrap().is_empty());
        assert!(store.search("rust", Some("other"), 10).unwrap().is_empty());

        // Traverse: a -> b (1 hop), c (2 hops). Follows direction (from->to).
        let hits = store.traverse(&a, 5, 100).unwrap();
        let by_id: HashMap<_, _> = hits.iter().map(|h| (h.id.clone(), h.depth)).collect();
        assert_eq!(by_id.get(&b), Some(&1));
        assert_eq!(by_id.get(&c), Some(&2));
    }

    /// Principle 3: a re-arrival at the same content address is absorbed as an attestation/lineage union
    /// (no overwrite), and converges to the same log regardless of arrival order (Principle 16).
    #[test]
    fn reobservation_accumulates_attestations() {
        let make = |host: &str, conf: f32, derived: &str| {
            let mut o = Observation::new(
                "same fact".into(),
                Provenance {
                    host: host.into(),
                    confidence: Some(conf),
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
        let f = forward.get_observation(&id).unwrap().unwrap();
        let r = reverse.get_observation(&id).unwrap().unwrap();

        // Both attestations are preserved - the first observation is not destroyed.
        assert_eq!(f.provenance.len(), 2, "attestation accumulation: {:?}", f.provenance);
        assert_eq!(f.derived_from, vec!["o1".to_string(), "o2".to_string()]);

        // Convergence independent of arrival order.
        let hosts = |o: &Observation| -> Vec<String> {
            o.provenance.iter().map(|p| p.host.clone()).collect()
        };
        assert_eq!(hosts(&f), hosts(&r));
        assert_eq!(f.derived_from, r.derived_from);
    }
}
