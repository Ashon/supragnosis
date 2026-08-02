//! redb-backed store adapter - a pure-Rust embedded B-tree with a single writer and MVCC readers.
//!
//! **Why a second file-backed adapter.** The Cozo adapter reaches the same data through Datalog, and
//! nineteen query shapes is what that expressiveness is spent on: point get/put on four relations,
//! full scans with a workspace filter, a two-rule union for `relations_of`, an ANN lookup, and
//! exactly one genuinely recursive query (`traverse`'s bounded BFS). No time-travel operator is used
//! at all. That is a key-value workload with a graph walk on top, and the engine never sees Datalog
//! either way - the passthrough tool has deliberately never been opened (Principle 12/21), so the
//! query language is an implementation detail of this layer alone.
//!
//! **What the shape buys.** A B-tree keyed by id gives the port's ascending-id enumeration for free
//! rather than by sorting on the way out, and a workspace scan is a multimap lookup instead of a scan
//! plus a filter. Being pure Rust it also drops the C++ RocksDB bridge, which is the thing that puts
//! `clang`/`libclang-dev` in the build.
//!
//! **Layout.** Rows are JSON values under their id, exactly the payload the Cozo `data` column holds,
//! so the two adapters reconstruct from the same encoding. Around them sit secondary indexes as redb
//! multimap tables (the DUPSORT analogue): workspace -> ids for each of the three enumerations, and
//! from/to -> relation ids for `relations_of` and for the traversal's out-edges. Multimap values come
//! back in sorted order, so every read path lands on ascending id without a sort.
//!
//! A secondary index is only correct if a re-put cannot strand its old entry: an upsert that moves a
//! row to a different workspace has to delete the stale membership. Every write here reads the
//! previous row first for exactly that reason.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use redb::{Database, MultimapTableDefinition, ReadableDatabase, ReadableTable, TableDefinition};
use supragnosis_core::{
    cosine_similarity, AssertionStore, Entity, KnowledgeStore, Observation, Relation, SearchHit,
    SearchHitKind, StoreError, TraverseHit,
};

/// The log, the projection, and the adapter's own metadata - each row a JSON value under its id.
const OBSERVATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("observations");
const ENTITIES: TableDefinition<&str, &[u8]> = TableDefinition::new("entities");
const RELATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("relations");
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");

/// Embeddings, in their own tables under the same id.
///
/// Not a layout preference - a requirement. Both `Observation::embedding` and `Entity::embedding`
/// carry `#[serde(skip)]`, deliberately: a vector must never ride out through the MCP surface and
/// bury an LLM's context in hundreds of floats (Principle 21). The core doc states the consequence
/// plainly - "persistence is handled by the store adapter with a hand-rolled encoding" - so an
/// adapter that persists a row by serializing the struct accepts every vector and stores none of
/// them. This one did, and the loss was invisible from outside: semantic reads answered "nothing
/// here", which is indistinguishable from a backend that simply has no vectors.
///
/// Values are little-endian f32, which is also what makes the split worth having on its own: a
/// vector is only read by the two semantic surfaces and by the projection's re-embed check, so the
/// folds that walk the log no longer carry 384 floats per row through a JSON parse they never look
/// at.
const OBS_VEC: TableDefinition<&str, &[u8]> = TableDefinition::new("obs_vec");
const ENT_VEC: TableDefinition<&str, &[u8]> = TableDefinition::new("ent_vec");

/// Secondary indexes. `workspace -> id` for the three enumerations, `endpoint -> relation id` for
/// `relations_of` (both directions) and for the traversal's out-edges (src only).
const OBS_BY_WS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("obs_by_ws");
const ENT_BY_WS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("ent_by_ws");
const REL_BY_WS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("rel_by_ws");
const REL_BY_SRC: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("rel_by_src");
const REL_BY_DST: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("rel_by_dst");

fn backend(e: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(e.to_string())
}

fn encode_vector(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Decodes a stored vector. A length that is not a multiple of four is a corrupt row rather than an
/// absent one, but a vector is a recall aid (Principle 19): losing one degrades recall, it does not
/// make the knowledge wrong, so this drops the vector rather than failing the read of the row.
fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

/// A file-backed knowledge store on redb.
pub struct RedbStore {
    db: Database,
}

impl RedbStore {
    /// Opens (creating if absent) the database at `path`. Every table is created up front in one
    /// transaction: redb reports a never-written table as a missing-table error on read, and a store
    /// that answers "no observations yet" with an error would break the absence-is-not-failure
    /// contract (Principle 5) for the entire first run.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(backend)?;
            }
        }
        let db = Database::create(path).map_err(backend)?;
        let txn = db.begin_write().map_err(backend)?;
        {
            txn.open_table(OBSERVATIONS).map_err(backend)?;
            txn.open_table(ENTITIES).map_err(backend)?;
            txn.open_table(RELATIONS).map_err(backend)?;
            txn.open_table(META).map_err(backend)?;
            txn.open_table(OBS_VEC).map_err(backend)?;
            txn.open_table(ENT_VEC).map_err(backend)?;
            txn.open_multimap_table(OBS_BY_WS).map_err(backend)?;
            txn.open_multimap_table(ENT_BY_WS).map_err(backend)?;
            txn.open_multimap_table(REL_BY_WS).map_err(backend)?;
            txn.open_multimap_table(REL_BY_SRC).map_err(backend)?;
            txn.open_multimap_table(REL_BY_DST).map_err(backend)?;
        }
        txn.commit().map_err(backend)?;
        Ok(Self { db })
    }

    /// Records the embedder identity, so reopening under a different model can be refused before its
    /// vectors mix with the stored ones (the same fail-fast the Cozo adapter applies).
    pub fn set_embedder(&self, embedder_id: &str) -> Result<(), StoreError> {
        if let Some(existing) = self.embedder()? {
            if existing != embedder_id {
                return Err(StoreError::Backend(format!(
                    "store was written with embedder '{existing}' but was opened with \
                     '{embedder_id}' - vectors from two models share no space, so mixing them \
                     silently degrades recall. Re-embed the store, or open it with the original model"
                )));
            }
            return Ok(());
        }
        let txn = self.db.begin_write().map_err(backend)?;
        {
            let mut t = txn.open_table(META).map_err(backend)?;
            t.insert("embedder", embedder_id).map_err(backend)?;
        }
        txn.commit().map_err(backend)
    }

    pub fn embedder(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let t = txn.open_table(META).map_err(backend)?;
        Ok(t.get("embedder").map_err(backend)?.map(|v| v.value().to_string()))
    }

    /// Every id in a workspace, ascending, or every id in the table when the scope is `None`. The two
    /// paths agree on order because a multimap's values and a table's keys are both sorted sets.
    fn ids_in(
        &self,
        txn: &redb::ReadTransaction,
        table: TableDefinition<&str, &[u8]>,
        index: MultimapTableDefinition<&str, &str>,
        workspace: Option<&str>,
    ) -> Result<Vec<String>, StoreError> {
        match workspace {
            Some(ws) => {
                let idx = txn.open_multimap_table(index).map_err(backend)?;
                let mut out = Vec::new();
                for v in idx.get(ws).map_err(backend)? {
                    out.push(v.map_err(backend)?.value().to_string());
                }
                Ok(out)
            }
            None => {
                let t = txn.open_table(table).map_err(backend)?;
                let mut out = Vec::new();
                for row in t.iter().map_err(backend)? {
                    let (k, _) = row.map_err(backend)?;
                    out.push(k.value().to_string());
                }
                Ok(out)
            }
        }
    }

    /// Loads rows by id, in the order given. A row whose JSON no longer parses is **excluded and
    /// logged, never fatal** - the enumeration degrade the port mandates (Principle 19), so one
    /// unreadable row cannot make a derived overlay unusable. A point read is fail-fast instead,
    /// because mistaking a failure for absence there would destroy attestations on the next absorb
    /// (Principle 3).
    fn load_rows<T: serde::de::DeserializeOwned>(
        txn: &redb::ReadTransaction,
        table: TableDefinition<&str, &[u8]>,
        ids: &[String],
        what: &'static str,
    ) -> Result<Vec<T>, StoreError> {
        let t = txn.open_table(table).map_err(backend)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(raw) = t.get(id.as_str()).map_err(backend)? else {
                continue;
            };
            match serde_json::from_slice::<T>(raw.value()) {
                Ok(v) => out.push(v),
                Err(e) => tracing::warn!(
                    row_id = %id,
                    kind = what,
                    error = %e,
                    "row reconstruction failed - excluded from enumeration (degrade). \
                     Original preserved in the store"
                ),
            }
        }
        Ok(out)
    }
}

/// The workspace a projected row belongs to. An entity carries a list of attestations but its id is
/// derived from (workspace, name), so every attestation on one row shares a workspace; the first is
/// representative. This matches the Cozo adapter, which stores that same value in a column.
fn entity_workspace(e: &Entity) -> String {
    e.provenance.first().map(|p| p.workspace.clone()).unwrap_or_default()
}

impl AssertionStore for RedbStore {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        // Read-absorb-write: a re-arrival at the same content address unions attestations and
        // lineage rather than replacing the row (Principle 3). Reading inside the write transaction
        // is what makes it atomic - redb admits one writer at a time, so no second observe can land
        // between the read and the insert and have its attestation dropped.
        let txn = self.db.begin_write().map_err(backend)?;
        {
            let mut t = txn.open_table(OBSERVATIONS).map_err(backend)?;
            let previous: Option<Observation> = match t.get(obs.id.as_str()).map_err(backend)? {
                // A row that will not parse is a failure, not an absence: absorbing onto a fresh
                // row here would silently drop whatever attestations the stored one held.
                Some(raw) => Some(serde_json::from_slice(raw.value()).map_err(backend)?),
                None => None,
            };
            // The stored vector is re-attached before the absorb, because absorb takes an embedding
            // only when it has none: a re-arrival carrying no vector would otherwise leave the
            // merged row empty and erase the one already held.
            let previous = match previous {
                Some(mut p) => {
                    let vt = txn.open_table(OBS_VEC).map_err(backend)?;
                    if let Some(raw) = vt.get(p.id.as_str()).map_err(backend)? {
                        p.embedding = decode_vector(raw.value());
                    }
                    Some(p)
                }
                None => None,
            };
            let merged = match previous {
                Some(mut existing) => {
                    existing.absorb(obs);
                    existing
                }
                None => obs,
            };
            // No stale-membership delete here, unlike the entity and relation writes. An
            // observation's workspace is INSIDE its content address, so every attestation on one id
            // shares it and an absorb cannot move the row - the insert is idempotent into a set. The
            // asymmetry is the model's, not an oversight: a projected row's workspace is mutable
            // (a re-key moves it) while a log row's is identity.
            let ws = merged.workspace().to_string();
            let bytes = serde_json::to_vec(&merged).map_err(backend)?;
            t.insert(merged.id.as_str(), bytes.as_slice()).map_err(backend)?;
            if let Some(vec) = &merged.embedding {
                let mut vt = txn.open_table(OBS_VEC).map_err(backend)?;
                vt.insert(merged.id.as_str(), encode_vector(vec).as_slice()).map_err(backend)?;
            }
            let mut idx = txn.open_multimap_table(OBS_BY_WS).map_err(backend)?;
            idx.insert(ws.as_str(), merged.id.as_str()).map_err(backend)?;
        }
        txn.commit().map_err(backend)
    }

    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let t = txn.open_table(OBSERVATIONS).map_err(backend)?;
        let Some(raw) = t.get(id).map_err(backend)? else {
            return Ok(None);
        };
        let mut obs: Observation = serde_json::from_slice(raw.value()).map_err(backend)?;
        let vt = txn.open_table(OBS_VEC).map_err(backend)?;
        if let Some(v) = vt.get(id).map_err(backend)? {
            obs.embedding = decode_vector(v.value());
        }
        Ok(Some(obs))
    }

    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let t = txn.open_table(ENTITIES).map_err(backend)?;
        let Some(raw) = t.get(id).map_err(backend)? else {
            return Ok(None);
        };
        let mut entity: Entity = serde_json::from_slice(raw.value()).map_err(backend)?;
        // The projection reads this back to skip re-embedding an entity whose text has not changed,
        // so a point get that dropped it would turn that optimization into a silent no-op.
        let vt = txn.open_table(ENT_VEC).map_err(backend)?;
        if let Some(v) = vt.get(id).map_err(backend)? {
            entity.embedding = decode_vector(v.value());
        }
        Ok(Some(entity))
    }

    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        // Both directions, unioned. A self-loop is indexed under the same id twice, so the set is
        // what keeps it from being reported as two edges - and it sorts, which is the order the port
        // promises.
        let mut ids: BTreeSet<String> = BTreeSet::new();
        for index in [REL_BY_SRC, REL_BY_DST] {
            let idx = txn.open_multimap_table(index).map_err(backend)?;
            for v in idx.get(entity_id).map_err(backend)? {
                ids.insert(v.map_err(backend)?.value().to_string());
            }
        }
        let ids: Vec<String> = ids.into_iter().collect();
        Self::load_rows::<Relation>(&txn, RELATIONS, &ids, "relation")
    }

    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let ids = self.ids_in(&txn, ENTITIES, ENT_BY_WS, workspace)?;
        let mut rows = Self::load_rows::<Entity>(&txn, ENTITIES, &ids, "entity")?;
        // Entity vectors are attached, unlike observation vectors: the merge band ranks candidate
        // pairs by name-embedding distance over this very enumeration, so withholding them would
        // silently empty the candidate list rather than make the read cheaper.
        let vt = txn.open_table(ENT_VEC).map_err(backend)?;
        for e in &mut rows {
            if let Some(v) = vt.get(e.id.as_str()).map_err(backend)? {
                e.embedding = decode_vector(v.value());
            }
        }
        Ok(rows)
    }

    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let ids = self.ids_in(&txn, RELATIONS, REL_BY_WS, workspace)?;
        Self::load_rows::<Relation>(&txn, RELATIONS, &ids, "relation")
    }

    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let ids = self.ids_in(&txn, OBSERVATIONS, OBS_BY_WS, workspace)?;
        let mut rows = Self::load_rows::<Observation>(&txn, OBSERVATIONS, &ids, "observation")?;
        // Attached for parity with the Cozo adapter, which reconstructs the vector out of its data
        // JSON here. It is a cost with no reader: no fold on the read path touches
        // `Observation::embedding` - only `search_semantic` does, and that reads the vector table
        // directly. Withholding it is the available optimization, but it is a change to what the
        // port returns rather than an adapter's choice to make on its own, so both adapters answer
        // the same thing until the port says otherwise.
        let vt = txn.open_table(OBS_VEC).map_err(backend)?;
        for o in &mut rows {
            if let Some(v) = vt.get(o.id.as_str()).map_err(backend)? {
                o.embedding = decode_vector(v.value());
            }
        }
        Ok(rows)
    }

    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<SearchHit> = Vec::new();

        // Substring match over canonical name and aliases. Both are inside the row, so this is the
        // same full scan the other adapters run - keyword recall is a scan on every backend, and
        // pretending otherwise would only hide where the cost is.
        for e in self.all_entities(workspace)? {
            let matched = e.canonical_name.to_lowercase().contains(&q)
                || e.aliases.iter().any(|a| a.to_lowercase().contains(&q));
            if matched {
                hits.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    id: e.id,
                    snippet: e.canonical_name,
                    score: 1.0,
                });
            }
        }
        for o in self.all_observations(workspace)? {
            if o.content.to_lowercase().contains(&q) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id,
                    snippet: o.content.chars().take(160).collect(),
                    score: 0.7,
                });
            }
        }

        // Ties break by id so that truncation is reproducible (Principle 16).
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
        let txn = self.db.begin_read().map_err(backend)?;
        let by_src = txn.open_multimap_table(REL_BY_SRC).map_err(backend)?;
        let rel_table = txn.open_table(RELATIONS).map_err(backend)?;
        let ent_table = txn.open_table(ENTITIES).map_err(backend)?;

        let mut out: Vec<TraverseHit> = Vec::new();
        let mut visited: HashSet<String> = HashSet::from([start_id.to_string()]);
        let mut frontier: Vec<String> = vec![start_id.to_string()];

        let mut depth = 1usize;
        while depth <= max_depth && !frontier.is_empty() {
            // Gather the whole ring, sort it, then emit - so the answer is in (depth, id) order and
            // truncation keeps the nearer neighbours. Emitting as the walk discovers would make the
            // result depend on index layout.
            let mut next: BTreeSet<String> = BTreeSet::new();
            for node in &frontier {
                for v in by_src.get(node.as_str()).map_err(backend)? {
                    let rid = v.map_err(backend)?;
                    let Some(raw) = rel_table.get(rid.value()).map_err(backend)? else {
                        continue;
                    };
                    let Ok(rel) = serde_json::from_slice::<Relation>(raw.value()) else {
                        continue;
                    };
                    if !visited.contains(&rel.to) {
                        next.insert(rel.to);
                    }
                }
            }

            for to in &next {
                visited.insert(to.clone());
                // An endpoint with no projected entity row is traversed THROUGH but never emitted:
                // reachability still runs past it, but there is nothing yet to describe, and a hit
                // with an empty name would be an invented node. Parity with the other adapters.
                let Some(raw) = ent_table.get(to.as_str()).map_err(backend)? else {
                    continue;
                };
                let Ok(e) = serde_json::from_slice::<Entity>(raw.value()) else {
                    continue;
                };
                out.push(TraverseHit {
                    id: to.clone(),
                    depth,
                    name: e.canonical_name,
                    kind: e.kind,
                });
                if out.len() >= limit {
                    return Ok(out);
                }
            }
            frontier = next.into_iter().collect();
            depth += 1;
        }
        Ok(out)
    }

    fn search_semantic(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let ids = self.ids_in(&txn, OBSERVATIONS, OBS_BY_WS, workspace)?;
        let vt = txn.open_table(OBS_VEC).map_err(backend)?;
        let rows = txn.open_table(OBSERVATIONS).map_err(backend)?;
        let mut hits: Vec<SearchHit> = Vec::new();
        for id in &ids {
            // A row with no vector is not a candidate (Principle 19: recall widening, never a
            // filter that invents membership). The vector table is consulted first, so a workspace
            // with no embeddings at all costs one miss per row instead of a full row parse.
            let Some(raw) = vt.get(id.as_str()).map_err(backend)? else {
                continue;
            };
            let Some(emb) = decode_vector(raw.value()) else {
                continue;
            };
            let Some(row) = rows.get(id.as_str()).map_err(backend)? else {
                continue;
            };
            let Ok(obs) = serde_json::from_slice::<Observation>(row.value()) else {
                continue;
            };
            hits.push(SearchHit {
                kind: SearchHitKind::Observation,
                id: obs.id,
                snippet: obs.content.chars().take(160).collect(),
                score: cosine_similarity(query_embedding, &emb),
            });
        }
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
            .all_entities(workspace)?
            .into_iter()
            .filter_map(|e| {
                let emb = e.embedding.as_deref()?;
                let score = cosine_similarity(query_embedding, emb);
                Some(SearchHit {
                    kind: SearchHitKind::Entity,
                    id: e.id,
                    snippet: e.canonical_name,
                    score,
                })
            })
            .collect();
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

impl KnowledgeStore for RedbStore {
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        let txn = self.db.begin_write().map_err(backend)?;
        {
            let mut t = txn.open_table(ENTITIES).map_err(backend)?;
            // The previous row's workspace has to be read before the overwrite: an upsert that moves
            // a row to another workspace would otherwise leave the old membership behind, and the
            // stale entry would make the row appear in two scoped enumerations at once.
            let stale_ws = match t.get(entity.id.as_str()).map_err(backend)? {
                Some(raw) => {
                    serde_json::from_slice::<Entity>(raw.value()).ok().map(|e| entity_workspace(&e))
                }
                None => None,
            };
            let ws = entity_workspace(&entity);
            let bytes = serde_json::to_vec(&entity).map_err(backend)?;
            t.insert(entity.id.as_str(), bytes.as_slice()).map_err(backend)?;
            {
                // An upsert that arrives without a vector clears the stored one, unlike an
                // observation absorb. A projected entity is rebuilt from the log rather than merged
                // into, so carrying a vector forward here would keep one whose text no longer
                // matches - which is the stale-embedding bug, not a saving.
                let mut vt = txn.open_table(ENT_VEC).map_err(backend)?;
                match &entity.embedding {
                    Some(vec) => {
                        vt.insert(entity.id.as_str(), encode_vector(vec).as_slice())
                            .map_err(backend)?;
                    }
                    None => {
                        vt.remove(entity.id.as_str()).map_err(backend)?;
                    }
                }
            }

            let mut idx = txn.open_multimap_table(ENT_BY_WS).map_err(backend)?;
            if let Some(old) = stale_ws.filter(|o| *o != ws) {
                idx.remove(old.as_str(), entity.id.as_str()).map_err(backend)?;
            }
            idx.insert(ws.as_str(), entity.id.as_str()).map_err(backend)?;
        }
        txn.commit().map_err(backend)
    }

    fn add_relation(&self, rel: Relation) -> Result<(), StoreError> {
        let txn = self.db.begin_write().map_err(backend)?;
        {
            let mut t = txn.open_table(RELATIONS).map_err(backend)?;
            // As with an entity, the endpoints and the workspace of the previous row are what the
            // stale index entries are keyed by. The relation id is derived from (from, kind, to), so
            // the endpoints cannot actually move - but the workspace can, and reading one row is
            // cheaper than a rule that has to stay true as the id formula evolves.
            let previous = match t.get(rel.id.as_str()).map_err(backend)? {
                Some(raw) => serde_json::from_slice::<Relation>(raw.value()).ok(),
                None => None,
            };
            let ws = rel.provenance.workspace.clone();
            let bytes = serde_json::to_vec(&rel).map_err(backend)?;
            t.insert(rel.id.as_str(), bytes.as_slice()).map_err(backend)?;

            let mut by_ws = txn.open_multimap_table(REL_BY_WS).map_err(backend)?;
            let mut by_src = txn.open_multimap_table(REL_BY_SRC).map_err(backend)?;
            let mut by_dst = txn.open_multimap_table(REL_BY_DST).map_err(backend)?;
            if let Some(old) = previous {
                if old.provenance.workspace != ws {
                    by_ws
                        .remove(old.provenance.workspace.as_str(), rel.id.as_str())
                        .map_err(backend)?;
                }
                if old.from != rel.from {
                    by_src.remove(old.from.as_str(), rel.id.as_str()).map_err(backend)?;
                }
                if old.to != rel.to {
                    by_dst.remove(old.to.as_str(), rel.id.as_str()).map_err(backend)?;
                }
            }
            by_ws.insert(ws.as_str(), rel.id.as_str()).map_err(backend)?;
            by_src.insert(rel.from.as_str(), rel.id.as_str()).map_err(backend)?;
            by_dst.insert(rel.to.as_str(), rel.id.as_str()).map_err(backend)?;
        }
        txn.commit().map_err(backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::{Provenance, TrustTier};

    fn tmp_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before the unix epoch")
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!("supragnosis-redb-{}-{nanos}-{seq}/knowledge.redb", std::process::id()))
    }

    fn prov_in(ws: &str) -> Provenance {
        Provenance {
            host: "host-a".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: ws.into(),
            source_ref: None,
            observed_at: 1,
            confidence: Some(1.0),
            trust_tier: TrustTier::default(),
            sync: None,
        }
    }

    fn ent_in(ws: &str, name: &str) -> Entity {
        Entity {
            id: Entity::make_id(ws, name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            description: None,
            properties: serde_json::Value::Null,
            provenance: vec![prov_in(ws)],
            embedding: None,
        }
    }

    /// The point of a file-backed adapter: the knowledge is still there after the process that wrote
    /// it is gone. The conformance suite cannot ask this - it holds one open store per case, and the
    /// in-memory adapter has no answer - so it belongs here.
    ///
    /// Reopening also re-runs table creation, which must be idempotent: a second `open` that wiped
    /// or refused the existing tables would lose the log, and would do it silently on the second run
    /// rather than the first.
    #[test]
    fn redb_knowledge_survives_a_close_and_reopen() {
        let path = tmp_path();
        let obs_id;
        {
            let store = RedbStore::open(&path).expect("open");
            store.put_entity(ent_in("ws1", "alpha")).expect("put");
            store
                .add_relation(Relation {
                    id: Relation::make_id(
                        &Entity::make_id("ws1", "alpha"),
                        "depends_on",
                        &Entity::make_id("ws1", "beta"),
                    ),
                    from: Entity::make_id("ws1", "alpha"),
                    to: Entity::make_id("ws1", "beta"),
                    kind: "depends_on".into(),
                    description: None,
                    provenance: prov_in("ws1"),
                    valid_from: None,
                    valid_to: None,
                })
                .expect("relation");
            let obs = Observation::new("a fact worth keeping".into(), prov_in("ws1"));
            obs_id = obs.id.clone();
            store.add_observation(obs).expect("observe");
        }

        let store = RedbStore::open(&path).expect("reopen");
        assert_eq!(
            store
                .get_entity(&Entity::make_id("ws1", "alpha"))
                .expect("get")
                .map(|e| e.canonical_name),
            Some("alpha".to_string()),
        );
        assert_eq!(store.all_relations(Some("ws1")).expect("relations").len(), 1);
        assert_eq!(store.all_observations(Some("ws1")).expect("log").len(), 1);
        assert!(store.get_observation(&obs_id).expect("get").is_some());
        // The secondary indexes survive too - a scoped read is served from them, so a scan that only
        // worked before the reopen would mean the index was rebuilt in memory and never persisted.
        assert_eq!(store.all_entities(Some("ws1")).expect("scoped").len(), 1);
        assert!(store.all_entities(Some("ws2")).expect("other").is_empty());

        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// An upsert that moves a row to another workspace must delete the old membership. Without it the
    /// row answers two scoped enumerations at once, and the union of the scoped views stops equalling
    /// the unscoped one - two reads of one store disagreeing, which is the shape of bug that the
    /// re-key path already produced once at the engine level.
    ///
    /// `rekey_workspace` is the operator act that reaches this, so it is not a hypothetical.
    #[test]
    fn redb_a_workspace_move_leaves_no_stale_index_entry() {
        let path = tmp_path();
        let store = RedbStore::open(&path).expect("open");

        // Same entity id, re-attested into a different workspace. The id is derived from the
        // ORIGINAL workspace, so this is precisely the shape a re-key produces: the row moves while
        // its key does not.
        let mut e = ent_in("ws1", "alpha");
        store.put_entity(e.clone()).expect("first");
        assert_eq!(store.all_entities(Some("ws1")).expect("before").len(), 1);

        e.provenance = vec![prov_in("ws2")];
        store.put_entity(e).expect("moved");

        assert!(
            store.all_entities(Some("ws1")).expect("old scope").is_empty(),
            "the old workspace must not still claim the row"
        );
        assert_eq!(store.all_entities(Some("ws2")).expect("new scope").len(), 1);
        assert_eq!(
            store.all_entities(None).expect("unscoped").len(),
            1,
            "one row, counted once - the unscoped view is the union of the scoped ones"
        );

        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Vectors from two models share no space, so mixing them degrades recall silently rather than
    /// loudly. Reopening under a different embedder is refused for the same reason the Cozo adapter
    /// refuses it - the failure has to happen at open, not at the first bad ranking.
    #[test]
    fn redb_refuses_a_reopen_under_a_different_embedder() {
        let path = tmp_path();
        {
            let store = RedbStore::open(&path).expect("open");
            store.set_embedder("bge-small-en-v1.5:384").expect("first embedder");
            store
                .set_embedder("bge-small-en-v1.5:384")
                .expect("same embedder is idempotent");
        }
        let store = RedbStore::open(&path).expect("reopen");
        let err = store.set_embedder("other-model:768").expect_err("mismatch must be refused");
        let msg = err.to_string();
        assert!(msg.contains("bge-small-en-v1.5:384"), "names what is stored: {msg}");
        assert!(msg.contains("other-model:768"), "names what was asked for: {msg}");

        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }
}
