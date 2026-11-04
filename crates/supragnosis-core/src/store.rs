//! The storage ports: the replication wire unit, the assertion store every surface holds,
//! the full store only the projection holds, and the store error.

use serde::{Deserialize, Serialize};

use crate::{
    observation_content_id, Assertions, Entity, Observation, Provenance, Relation, SearchHit,
    TraverseHit, VersionVector,
};

#[cfg(doc)]
use crate::attestation_key;

/// The replication wire unit (docs/federation.md Section 3/5): one signed attestation bound to a
/// content id. The receiver recomputes the content id from (workspace, content, assertions) - it is
/// deliberately NOT carried, so a forged id cannot ride the wire - then CAS-dedups/absorbs (F3). The
/// origin's lineage declaration travels inside `attestation.sync.lineage` (signed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationEvent {
    pub content: String,
    #[serde(default, skip_serializing_if = "Assertions::is_empty")]
    pub assertions: Assertions,
    pub attestation: Provenance,
}

/// Storage port. Implemented by adapters such as in-memory / redb.
///
/// **Read contract** (the duty of every adapter):
/// - **Distinguishing absence from failure (Principle 5)**: a backend failure propagates as `Err`. Swallowing a failure as an
///   empty result (`Ok(vec![])`/`Ok(None)`) leaves the caller unable to distinguish "not found (unknown)" from "cannot
///   query (failure)" - the premise of distinguishing absence/negation/failure collapses at the storage layer. A partial
///   failure is also `Err`, not a partial result.
/// - **Reproducibility (Principle 16)**: the same query on the same state gives the same response. Sorting and limit
///   truncation are pinned to a stable key (id), and it is a contract violation if the iteration order of internal data
///   structures (hash map / row order) leaks into the response. The enumerations
///   ([`KnowledgeStore::all_entities`] / [`KnowledgeStore::all_relations`] / [`KnowledgeStore::all_observations`])
///   return rows in **ascending id order** - an enumeration is a response too, and it is the input to every fold on the
///   read path, so a fold that resolves a tie by "first row seen" would otherwise inherit an order the store never
///   promised. Naming the order is what makes the promise checkable
///   (`crates/supragnosis-store/tests/port_conformance.rs`).
pub trait AssertionStore: Send + Sync {
    /// Appends (or absorbs a re-arrival into) one log row. **Contract (crash-recovery.md K1):** the
    /// same store transaction that writes the row records it in the owed-projection ledger, so the
    /// append and the record that its projection is still owed commit together or not at all. The
    /// adapter does it rather than the caller because the sync crate appends through this handle and
    /// never sees the engine that would otherwise have to remember. An observation with no
    /// attestation is refused before anything is written ([`Observation::check_attested`]).
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    /// Restores an observation by id from the observation log - the back-reference path for search hits / derivation
    /// lineage (Principle 2/14: whoever knows the id can reach the entity and its provenance) and the reference read for
    /// re-arrival merging. A missing id is `Ok(None)`, a backend failure is `Err`.
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError>;
    /// A missing id is `Ok(None)` (absence, the unknown of Principle 5) - only a backend failure is `Err`.
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError>;
    /// Relations whose from or to is entity_id.
    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError>;
    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError>;
    /// Entities reachable from start_id following the direction (from->to) up to `max_depth` hops.
    fn traverse(
        &self,
        start_id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError>;
    /// Enumerates all entities in the workspace (the read path of the graph projection). `None` means all.
    /// The node set for ontology visualization/observability - a full enumeration, not a query term like search.
    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError>;
    /// Enumerates all relations in the workspace (the edge set of the graph projection). `None` means all.
    /// A relation's workspace is determined by provenance.workspace.
    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError>;
    /// Enumerates all observations in the workspace (the entire log). `None` means all. A full enumeration,
    /// not a query term like search - absence is an empty Vec. Since log replay / co-occurrence induction is structurally
    /// impossible with point gets alone, enumeration is placed on the port - it is the common read path for hyperedges
    /// (second-order structure, Principle 11) and reprojection (Principle 1).
    ///
    /// **degrade contract**: a restore failure of an individual row (some legacy/corrupted row) does not block the whole
    /// enumeration - it excludes that row and returns the rest but **does not stay silent** (it logs the exclusion,
    /// the degrade spirit of Principle 19). This is so a derived overlay (hyperedge) is not made entirely unusable because of
    /// one row. In contrast, a point-read ([`KnowledgeStore::get_observation`]) is fail-fast
    /// (being the reference read for re-arrival merging, mistaking a failure for absence destroys attestations, Principle 3).
    /// A query/schema-level backend failure (not an individual row) is still `Err`.
    /// Note: a reprojection that needs completeness (M3) must treat excluded rows as restore/recovery, not a drop.
    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError>;
    /// Whether `workspace` holds any observation - what makes a workspace exist. The agent surface
    /// asks it before a write, to say when the write makes a new one (design-system.md Section 29.20).
    /// The default reads the workspace's log; an adapter with a workspace index answers from it.
    fn holds_observations(&self, workspace: &str) -> Result<bool, StoreError> {
        Ok(!self.all_observations(Some(workspace))?.is_empty())
    }
    /// Every workspace that holds an observation, ascending and each once - the workspaces
    /// [`AssertionStore::holds_observations`] says exist, all at once. A workspace whose
    /// observations assert no entity is one of them (Principle 22). The default walks the whole
    /// log; an adapter with a workspace index answers from its keys without reading a row.
    fn observation_workspaces(&self) -> Result<Vec<String>, StoreError> {
        let set: std::collections::BTreeSet<String> =
            self.all_observations(None)?.iter().map(|o| o.workspace().to_string()).collect();
        Ok(set.into_iter().collect())
    }
    /// The sync delta read (docs/federation.md Section 5, M4 Phase 1): every sync-stamped attestation
    /// in `workspace` NOT yet covered by `since`, as wire-unit events, ordered deterministically by
    /// (origin_node, origin_seq, content id). Attestations without a sync block (pre-federation /
    /// local-unsigned) are local-only and never returned - they become exportable via backfill stamping
    /// (Phase 2). The default implementation scans [`KnowledgeStore::all_observations`]; adapters may
    /// override with an indexed scan.
    fn attestations_since(
        &self,
        workspace: &str,
        since: &VersionVector,
    ) -> Result<Vec<AttestationEvent>, StoreError> {
        let mut events = Vec::new();
        for obs in self.all_observations(Some(workspace))? {
            // Legacy-format guard: a stored id that no longer matches the current content-address
            // formula cannot verify remotely (signatures bind the recomputed id) - such rows are
            // local history and never cross the wire. See [`observation_content_id`] / migrate.
            if observation_content_id(workspace, &obs.content, &obs.assertions) != obs.id {
                continue;
            }
            for p in &obs.provenance {
                let Some(meta) = &p.sync else { continue };
                if since.covers(&meta.origin_node, workspace, meta.origin_seq) {
                    continue;
                }
                events.push(AttestationEvent {
                    content: obs.content.clone(),
                    assertions: obs.assertions.clone(),
                    attestation: p.clone(),
                });
            }
        }
        events.sort_by(|a, b| {
            let ka = a.attestation.sync.as_ref().map(|s| (s.origin_node.clone(), s.origin_seq));
            let kb = b.attestation.sync.as_ref().map(|s| (s.origin_node.clone(), s.origin_seq));
            ka.cmp(&kb).then_with(|| a.content.cmp(&b.content))
        });
        Ok(events)
    }
    /// Searches observations that have an embedding by cosine similarity to the query vector (Principle 19: recall expansion).
    /// Observations without an embedding are excluded from the candidates. `score` is cosine similarity (-1.0~1.0).
    /// The default implementation returns an empty result - an adapter that does not store vectors need not override it.
    fn search_semantic(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
    }

    /// Searches entities that have an embedding by cosine similarity to the query vector (Principle 19: recall expansion).
    /// Lets the node be reached by the **meaning of the entity's name/aliases** - it is recalled even if no observation
    /// mentions that node lexically (filling the recall gap of observation-only semantics). Returns `SearchHitKind::Entity`
    /// hits, and `score` is cosine similarity. The default implementation returns an empty result.
    fn search_semantic_entities(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
    }
}

/// The full store: [`AssertionStore`] plus the two writes that put a row into the entity/relation
/// projection directly.
///
/// The split is Principle 1's third enforcement demand made structural: *"no API may write a fact
/// that did not pass through an assertion directly into the graph"*. Only the engine's projection
/// folds hold this trait. Everything else - the sync crate applying replicated events, the MCP tools,
/// the CLI - is handed an `AssertionStore`, through which knowledge can only enter as an appended
/// assertion and the graph can only be read.
///
/// It was a convention before, and the convention held: the calls to these methods sit in the
/// projection's writers and the entity-embedding backfill. What it did not hold against is the
/// past. The author's own store carries 35 entity rows that no observation anywhere
/// asserts - written directly, in an era when something did reach for this handle - and they are
/// invisible to the log, survive every re-projection, and cannot cross the sync wire because there is
/// nothing to export. A replay cannot reproduce them because the log never knew about them.
///
/// A test cannot guard this: it would have to enumerate callers that do not exist yet. A supertrait
/// can, by not being reachable through the handle the engine hands out.
pub trait KnowledgeStore: AssertionStore {
    /// Upsert keyed on entity.id. Reachable only where the projection is computed.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    /// Upsert keyed on relation.id. Reachable only where the projection is computed.
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// Upserts a workspace's whole projection, entity rows then relation rows: a reproject's result
    /// (crash-recovery.md Section 3a). A store that can commits it in one transaction, so it pays
    /// one commit rather than one per row and no reader sees part of it. The default writes row by
    /// row.
    fn put_projection(
        &self,
        entities: Vec<Entity>,
        relations: Vec<Relation>,
    ) -> Result<(), StoreError> {
        for e in entities {
            self.put_entity(e)?;
        }
        for r in relations {
            self.add_relation(r)?;
        }
        Ok(())
    }
    /// The owed-projection ledger (crash-recovery.md Section 3): every observation whose append
    /// committed and whose projection has not been confirmed since, as `(observation id, workspace)`
    /// in id order. Empty is the normal state. Projection-side, so it lives on the handle only the
    /// engine holds.
    fn owed_projections(&self) -> Result<Vec<(String, String)>, StoreError>;
    /// Removes ledger entries once the projection that repays them is written (K2). Ids not in the
    /// ledger are ignored, so clearing is idempotent.
    fn clear_owed(&self, ids: &[String]) -> Result<(), StoreError>;
    /// Appends `obs` - absorbed into a stored row of the same id, as `add_observation` does - and
    /// writes the rows projected from it, in one transaction: all of it lands or none does
    /// (crash-recovery.md Section 3a). It records no ledger entry, since the projection commits with
    /// the row, and clears none: an entry already held for the id is another writer's debt.
    fn commit_projected(
        &self,
        obs: Observation,
        entities: Vec<Entity>,
        relations: Vec<Relation>,
    ) -> Result<(), StoreError>;
    /// Attaches a retrieval vector to each named observation that has none - the backfill's write
    /// (semantic-retrieval.md Section 12, SR6). Nothing else is written: not the row, not its
    /// attestations, not the owed-projection ledger, which re-appending the row would touch. An id
    /// with no row, or whose row already carries a vector, is skipped (SR7: a stored vector is never
    /// replaced). Returns how many vectors were attached.
    fn attach_observation_embeddings(
        &self,
        vectors: &[(String, Vec<f32>)],
    ) -> Result<usize, StoreError>;
    /// Records the app that wrote an attestation, under its [`attestation_key`] (design-system.md
    /// Section 29.8). Node-local: nothing that is signed, synced, ordered or folded reads it. A key
    /// already recorded keeps the name it has - a row is written once.
    fn record_attestation_app(&self, key: &str, app: &str) -> Result<(), StoreError>;
    /// The apps recorded under these keys, by key; a key with none is absent from the answer.
    fn attestation_apps(
        &self,
        keys: &[String],
    ) -> Result<std::collections::BTreeMap<String, String>, StoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}
