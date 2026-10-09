//! redb-backed store adapter - a pure-Rust embedded B-tree with a single writer and MVCC readers.
//!
//! **Why this replaced a Datalog store.** Through v0.1.21 the file-backed adapter was Cozo, which
//! reached the same data through Datalog, and nineteen query shapes was what that expressiveness got
//! spent on: point get/put on four relations, full scans with a workspace filter, a two-rule union
//! for `relations_of`, an ANN lookup, and exactly one genuinely recursive query (`traverse`'s
//! bounded BFS). No time-travel operator was used at all. That is a key-value workload with a graph
//! walk on top, and the engine never saw Datalog either way - the passthrough tool has deliberately
//! never been opened (Principle 12/21), so the query language was an implementation detail of this
//! layer alone, and replaceable on that evidence rather than on taste.
//!
//! **What the shape buys.** A B-tree keyed by id gives the port's ascending-id enumeration for free
//! rather than by sorting on the way out, and a workspace scan is a multimap lookup instead of a scan
//! plus a filter. Being pure Rust it also drops the C++ RocksDB bridge, which is the thing that puts
//! `clang`/`libclang-dev` in the build.
//!
//! **Layout.** Rows are JSON values under their id - the same encoding the Datalog store kept in its
//! `data` column, so the migration was a copy rather than a re-encode. Around them sit secondary indexes as redb
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
    cosine_similarity, AssertionStore, Entity, KeywordQuery, KnowledgeStore, Observation, Relation,
    SearchHit, SearchHitKind, StoreError, TraverseHit,
};

/// The log, the projection, and the adapter's own metadata - each row a JSON value under its id.
const OBSERVATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("observations");
const ENTITIES: TableDefinition<&str, &[u8]> = TableDefinition::new("entities");
const RELATIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("relations");
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
/// The owed-projection ledger (crash-recovery.md): observation id -> workspace, written in the same
/// transaction as the log row and removed once the engine has projected it. An older build never
/// opens this table, so after a downgrade and an upgrade it can name rows that were since projected;
/// that costs one needless reproject and loses nothing.
const OWED: TableDefinition<&str, &str> = TableDefinition::new("owed_projection");

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

/// The store format this build writes (docs/compatibility.md Section 3). Section 3.7 says what
/// raises it; a raise adds its step to [`upgrade_to`], its row to Section 3.2 and a golden store.
pub const FORMAT: u32 = 2;

/// The lowest format a binary must implement to share a store this build has written. Raised when
/// a binary of an earlier format, writing here, would lose or contradict something (Section 3.7).
pub const MIN_READER: u32 = 2;

/// The release that writes `format_by` when this build raises a store's numbers.
const RELEASE: &str = env!("CARGO_PKG_VERSION");

/// A store's era: what its `meta` table records, or what its structure implies when it predates the
/// record (compatibility.md Section 3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFormat {
    /// The highest format any writer of this store has used.
    pub format: u32,
    /// The lowest format a binary must implement to open it safely.
    pub min_reader: u32,
    /// The release that last raised either number. `None` for a store from before the record.
    pub format_by: Option<String>,
}

/// Reads a store's era before anything writes to it. `None` is a file with no tables at all - a
/// store being created now, which takes this build's format with no upgrade to run.
fn read_format(db: &Database) -> Result<Option<StoreFormat>, StoreError> {
    let txn = db.begin_read().map_err(backend)?;
    let tables: Vec<String> = txn
        .list_tables()
        .map_err(backend)?
        .map(|t| redb::TableHandle::name(&t).to_string())
        .collect();
    let multimaps = txn.list_multimap_tables().map_err(backend)?.count();
    if tables.is_empty() && multimaps == 0 {
        return Ok(None);
    }
    let has = |name: &str| tables.iter().any(|t| t == name);
    let mut recorded = (None, None, None);
    if has("meta") {
        let meta = txn.open_table(META).map_err(backend)?;
        let get = |k: &str| -> Result<Option<String>, StoreError> {
            Ok(meta.get(k).map_err(backend)?.map(|v| v.value().to_string()))
        };
        recorded = (get("format")?, get("min_reader")?, get("format_by")?);
    }
    let number = |key: &str, v: String| {
        v.parse::<u32>().map_err(|_| {
            StoreError::Backend(format!(
                "the store's {key} is '{v}', not a number. A later release may have changed what \
                 it means, and this build will not guess - nothing was opened or changed \
                 (docs/compatibility.md Section 3)"
            ))
        })
    };
    Ok(Some(match recorded {
        (Some(format), min_reader, format_by) => {
            let format = number("format", format)?;
            let min_reader = match min_reader {
                Some(m) => number("min_reader", m)?,
                None => format,
            };
            StoreFormat { format, min_reader, format_by }
        }
        // Before the record, the ledger table is the one structural mark of an era.
        (None, ..) => {
            let format = if has("owed_projection") { 2 } else { 1 };
            StoreFormat { format, min_reader: format, format_by: None }
        }
    }))
}

/// The refusal for a store a later release has changed in a way this build cannot handle safely.
fn too_new(path: &Path, found: &StoreFormat) -> StoreError {
    let by = found.format_by.as_deref().unwrap_or("a later release");
    StoreError::Backend(format!(
        "the store at {} was raised to format {} by supragnosis {by}, and opening it needs a build \
         that implements format {} or later. This build ({RELEASE}) implements format {FORMAT}.\n\n\
         Opening it here would hide rows this build cannot parse, and strip the fields it does not \
         know the next time it rewrote a row, so nothing was opened and nothing was changed. \
         Install supragnosis {by} or later (docs/compatibility.md Section 3.3).",
        path.display(),
        found.format,
        found.min_reader,
    ))
}

/// redb's own refusal of a file format newer than it reads arrives as "Corrupted", which is what an
/// operator would believe and act on. It is the same case as [`too_new`] one layer down - a later
/// release's redb wrote the file - so it is reported as one, minus the release, which the meta
/// table would have named and cannot be read.
fn open_error(path: &Path, e: redb::DatabaseError) -> StoreError {
    match &e {
        redb::DatabaseError::Storage(redb::StorageError::Corrupted(msg))
            if msg.contains("file format version") =>
        {
            StoreError::Backend(format!(
                "the store at {} was written by a newer storage engine than this build's ({msg}). \
                 A later supragnosis release wrote it; this build ({RELEASE}) cannot read it, and \
                 nothing was opened or changed. Install the release that wrote it, or a later one \
                 (docs/compatibility.md Section 3.7).",
                path.display()
            ))
        }
        _ => backend(e),
    }
}

/// The upgrade from the format before `to` (compatibility.md Section 3.3), run in the transaction
/// that records the new format.
fn upgrade_to(txn: &redb::WriteTransaction, to: u32) -> Result<(), StoreError> {
    match to {
        // Format 2 is the owed-projection ledger. A store from before it has no record of which
        // appends were projected, and nothing vouches for them, so every log row is owed once: one
        // reproject of each workspace, which repairs whatever an older build's crash left behind
        // (crash-recovery.md Section 4).
        2 => {
            use redb::ReadableMultimapTable;
            let by_ws = txn.open_multimap_table(OBS_BY_WS).map_err(backend)?;
            let mut owed = txn.open_table(OWED).map_err(backend)?;
            for entry in by_ws.iter().map_err(backend)? {
                let (ws, ids) = entry.map_err(backend)?;
                for id in ids {
                    owed.insert(id.map_err(backend)?.value(), ws.value()).map_err(backend)?;
                }
            }
            Ok(())
        }
        _ => Err(StoreError::Backend(format!("no upgrade step to store format {to}"))),
    }
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
///
/// The length guard is load-bearing rather than defensive: `as_chunks` discards a trailing partial
/// chunk silently, so without the early return a corrupt row would decode to a short vector and be
/// used as if it were whole. Rejecting the row is the reported outcome; a truncated one is not.
fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    Some(bytes.as_chunks::<4>().0.iter().copied().map(f32::from_le_bytes).collect())
}

/// A file-backed knowledge store on redb.
pub struct RedbStore {
    db: Database,
}

/// Whether some process holds the redb store at `path` open for writing (daemon-lifecycle.md
/// Section 11). A stdio server binds no port, so the lock is the only trace it leaves, and a
/// lifecycle command that looked only at the port would start a second writer beside it.
///
/// Probed with a read-only open, which redb refuses while a writer holds the file - its own lock
/// rules, not a reimplementation of them. Nothing is created or repaired: an absent file is not
/// held, and an open that fails for any other reason (a file that needs repair, say) is reported
/// as not held, because the question is only whether a writer is present.
pub fn redb_in_use(path: impl AsRef<Path>) -> bool {
    if !path.as_ref().exists() {
        return false;
    }
    matches!(
        redb::ReadOnlyDatabase::open(path),
        Err(redb::DatabaseError::DatabaseAlreadyOpen)
    )
}

impl RedbStore {
    /// Opens (creating if absent) the database at `path`.
    ///
    /// The store's era is read first, before anything writes (compatibility.md Section 3.3). A
    /// store a later release raised past this build is refused unchanged; an earlier one is upgraded
    /// in place and its new format recorded in the same transaction as the last step. Every table is
    /// then created up front: redb reports a never-written table as a missing-table error on read,
    /// and a store that answers "no observations yet" with an error would break the
    /// absence-is-not-failure contract (Principle 5) for the entire first run.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(backend)?;
            }
        }
        let mut db = Database::create(path).map_err(|e| open_error(path, e))?;
        let found = read_format(&db)?;
        if let Some(f) = &found {
            if f.min_reader > FORMAT {
                return Err(too_new(path, f));
            }
        }
        // Raising min_reader is what stops earlier releases opening this store, so the store as they
        // knew it is kept first: the one way back (compatibility.md Section 3.5).
        if let Some(f) = found.as_ref().filter(|f| f.min_reader < MIN_READER) {
            drop(db);
            let copy = path.with_file_name(format!(
                "{}.format-{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("knowledge.redb"),
                f.format
            ));
            std::fs::copy(path, &copy).map_err(backend)?;
            tracing::warn!(
                copy = %copy.display(),
                from = f.format,
                to = FORMAT,
                "store upgraded past what earlier releases can open - the store as they knew it is \
                 kept beside it (docs/compatibility.md Section 3.5)"
            );
            db = Database::create(path).map_err(|e| open_error(path, e))?;
        }
        let txn = db.begin_write().map_err(backend)?;
        {
            txn.open_table(OBSERVATIONS).map_err(backend)?;
            txn.open_table(ENTITIES).map_err(backend)?;
            txn.open_table(RELATIONS).map_err(backend)?;
            txn.open_table(META).map_err(backend)?;
            txn.open_table(OWED).map_err(backend)?;
            txn.open_table(OBS_VEC).map_err(backend)?;
            txn.open_table(ENT_VEC).map_err(backend)?;
            txn.open_multimap_table(OBS_BY_WS).map_err(backend)?;
            txn.open_multimap_table(ENT_BY_WS).map_err(backend)?;
            txn.open_multimap_table(REL_BY_WS).map_err(backend)?;
            txn.open_multimap_table(REL_BY_SRC).map_err(backend)?;
            txn.open_multimap_table(REL_BY_DST).map_err(backend)?;
        }
        let from = found.as_ref().map_or(FORMAT, |f| f.format);
        for to in from + 1..=FORMAT {
            upgrade_to(&txn, to)?;
        }
        let next = StoreFormat {
            format: from.max(FORMAT),
            min_reader: found.as_ref().map_or(MIN_READER, |f| f.min_reader.max(MIN_READER)),
            format_by: None,
        };
        let raised = found.as_ref().is_none_or(|f| {
            f.format_by.is_none() || (f.format, f.min_reader) != (next.format, next.min_reader)
        });
        if raised {
            let mut meta = txn.open_table(META).map_err(backend)?;
            meta.insert("format", next.format.to_string().as_str()).map_err(backend)?;
            meta.insert("min_reader", next.min_reader.to_string().as_str())
                .map_err(backend)?;
            meta.insert("format_by", RELEASE).map_err(backend)?;
        }
        txn.commit().map_err(backend)?;
        Ok(Self { db })
    }

    /// The era this store records (compatibility.md Section 3.1).
    pub fn format(&self) -> Result<StoreFormat, StoreError> {
        read_format(&self.db)?.ok_or_else(|| StoreError::Backend("store has no tables".into()))
    }

    /// Records the embedder identity, so reopening under a different model can be refused before its
    /// vectors mix with the stored ones.
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
/// representative, stored rather than recomputed on read.
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
            // K1: in this transaction, not beside it - the row and the record that its projection
            // is owed commit together, so no crash can leave one without the other.
            let mut owed = txn.open_table(OWED).map_err(backend)?;
            owed.insert(merged.id.as_str(), ws.as_str()).map_err(backend)?;
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
        // Attached because the port's contract is that a read reconstructs the vector out of its data
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
        // The matching rule is core's, shared with every adapter (architecture.md Section 4.2).
        let q = KeywordQuery::new(query);
        let mut hits: Vec<SearchHit> = Vec::new();

        // Canonical name and aliases, matched spelling by spelling. Both are inside the row, so
        // this is the same full scan the other adapters run - keyword recall is a scan on every
        // backend, and pretending otherwise would only hide where the cost is.
        for e in self.all_entities(workspace)? {
            let spellings = std::iter::once(e.canonical_name.as_str())
                .chain(e.aliases.iter().map(String::as_str));
            if let Some(m) = q.best_of(spellings) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    score: KeywordQuery::score(SearchHitKind::Entity, m),
                    id: e.id,
                    snippet: e.canonical_name,
                });
            }
        }
        for o in self.all_observations(workspace)? {
            if let Some(m) = q.matches(&o.content) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: o.id,
                    snippet: o.content.chars().take(160).collect(),
                    score: KeywordQuery::score(SearchHitKind::Observation, m),
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
    fn owed_projections(&self) -> Result<Vec<(String, String)>, StoreError> {
        let txn = self.db.begin_read().map_err(backend)?;
        let t = txn.open_table(OWED).map_err(backend)?;
        let mut out = Vec::new();
        for row in t.iter().map_err(backend)? {
            let (k, v) = row.map_err(backend)?;
            out.push((k.value().to_string(), v.value().to_string()));
        }
        Ok(out)
    }

    fn clear_owed(&self, ids: &[String]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let txn = self.db.begin_write().map_err(backend)?;
        {
            let mut t = txn.open_table(OWED).map_err(backend)?;
            for id in ids {
                t.remove(id.as_str()).map_err(backend)?;
            }
        }
        txn.commit().map_err(backend)
    }

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

    /// A store written before the ledger existed has no record of which appends were projected, so
    /// its first ledger-aware open owes every log row - and only that open: once the table exists,
    /// an empty ledger means nothing is owed.
    #[test]
    fn a_store_from_before_the_ledger_is_owed_in_full_once() {
        let path = tmp_path();
        let obs = Observation::new("written by an older build".into(), prov_in("ws1"));
        let id = obs.id.clone();
        {
            let store = RedbStore::open(&path).expect("open");
            store.add_observation(obs).expect("append");
            // What an older build leaves behind: the log, no ledger table, and no format record.
            let txn = store.db.begin_write().expect("txn");
            txn.delete_table(OWED).expect("drop the ledger");
            unstamp(&txn);
            txn.commit().expect("commit");
        }
        let store = RedbStore::open(&path).expect("first ledger-aware open");
        assert_eq!(
            store.owed_projections().expect("ledger"),
            vec![(id.clone(), "ws1".to_string())]
        );
        store.clear_owed(std::slice::from_ref(&id)).expect("repaid");
        drop(store);
        let store = RedbStore::open(&path).expect("a later open");
        assert!(
            store.owed_projections().expect("ledger").is_empty(),
            "seeded once, not every open"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Removes the format record, leaving a store as a release from before it wrote one.
    fn unstamp(txn: &redb::WriteTransaction) {
        let mut meta = txn.open_table(META).expect("meta");
        for key in ["format", "min_reader", "format_by"] {
            meta.remove(key).expect("remove");
        }
    }

    /// Writes a format record directly, as a later release would have.
    fn stamp(path: &Path, format: &str, min_reader: &str, by: &str) {
        let db = Database::create(path).expect("raw open");
        let txn = db.begin_write().expect("txn");
        {
            let mut meta = txn.open_table(META).expect("meta");
            meta.insert("format", format).expect("format");
            meta.insert("min_reader", min_reader).expect("min_reader");
            meta.insert("format_by", by).expect("format_by");
        }
        txn.commit().expect("commit");
    }

    fn copy_beside(path: &Path, format: u32) -> std::path::PathBuf {
        path.with_file_name(format!("knowledge.redb.format-{format}"))
    }

    /// compatibility.md Section 3.1: a store created now records this build's format, and the
    /// release that recorded it.
    #[test]
    fn a_new_store_records_this_builds_format() {
        let path = tmp_path();
        let store = RedbStore::open(&path).expect("open");
        assert_eq!(
            store.format().expect("format"),
            StoreFormat {
                format: FORMAT,
                min_reader: MIN_READER,
                format_by: Some(RELEASE.to_string())
            }
        );
        drop(store);
        assert!(!copy_beside(&path, FORMAT).exists(), "a new store keeps no copy");
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Section 3.3, the refusal: a store a later release raised past this build is not opened, and
    /// nothing in it changes - not even the tables an ordinary open would create.
    #[test]
    fn a_store_a_later_release_raised_is_refused_unchanged() {
        let path = tmp_path();
        let obs = Observation::new("written by a later release".into(), prov_in("ws1"));
        {
            let store = RedbStore::open(&path).expect("open");
            store.add_observation(obs).expect("append");
            let txn = store.db.begin_write().expect("txn");
            txn.delete_table(OWED).expect("a table this build would create");
            txn.commit().expect("commit");
        }
        let later = (FORMAT + 1).to_string();
        stamp(&path, &later, &later, "9.9.9");
        let Err(e) = RedbStore::open(&path) else {
            panic!("a store raised past this build must be refused");
        };
        let msg = e.to_string();
        assert!(msg.contains("9.9.9"), "names the release that raised it: {msg}");
        assert!(msg.contains("nothing was changed"), "says it changed nothing: {msg}");

        let db = Database::create(&path).expect("raw open");
        let txn = db.begin_read().expect("txn");
        assert!(
            !txn.list_tables()
                .expect("tables")
                .any(|t| redb::TableHandle::name(&t) == "owed_projection"),
            "the refused open created nothing"
        );
        let meta = txn.open_table(META).expect("meta");
        assert_eq!(
            meta.get("format_by").expect("get").map(|v| v.value().to_string()),
            Some("9.9.9".into())
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Section 3.3, the additive case: a later format this build can still share opens as usual,
    /// and the record is not lowered to this build's number.
    #[test]
    fn a_later_additive_format_opens_and_is_not_lowered() {
        let path = tmp_path();
        drop(RedbStore::open(&path).expect("create"));
        stamp(&path, &(FORMAT + 1).to_string(), &MIN_READER.to_string(), "9.9.9");
        let store = RedbStore::open(&path).expect("an additive later format opens");
        assert_eq!(
            store.format().expect("format"),
            StoreFormat {
                format: FORMAT + 1,
                min_reader: MIN_READER,
                format_by: Some("9.9.9".into())
            }
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Section 3.4: a store from before the record is dated by its structure. With the ledger it is
    /// format 2, which this build shares as it is - so it is recorded, and nothing is copied.
    #[test]
    fn a_store_from_before_the_record_with_a_ledger_is_format_2() {
        let path = tmp_path();
        {
            let store = RedbStore::open(&path).expect("open");
            let txn = store.db.begin_write().expect("txn");
            unstamp(&txn);
            txn.commit().expect("commit");
            assert_eq!(
                store.format().expect("format"),
                StoreFormat { format: 2, min_reader: 2, format_by: None }
            );
        }
        let store = RedbStore::open(&path).expect("reopen");
        assert_eq!(store.format().expect("format").format_by.as_deref(), Some(RELEASE));
        drop(store);
        assert!(!copy_beside(&path, 2).exists(), "min_reader did not move, so no copy");
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Sections 3.4 and 3.5: without the ledger a store is format 1. Upgrading it raises min_reader
    /// past what v0.4.3 and earlier can share, so the store as they knew it is copied aside first -
    /// and the copy is a format 1 store, with no ledger in it.
    #[test]
    fn a_format_1_store_is_upgraded_and_kept_beside() {
        let path = tmp_path();
        {
            let store = RedbStore::open(&path).expect("open");
            store
                .add_observation(Observation::new("from v0.4.3".into(), prov_in("ws1")))
                .expect("append");
            let txn = store.db.begin_write().expect("txn");
            txn.delete_table(OWED).expect("drop the ledger");
            unstamp(&txn);
            txn.commit().expect("commit");
        }
        let store = RedbStore::open(&path).expect("upgrade");
        assert_eq!(
            store.format().expect("format"),
            StoreFormat { format: 2, min_reader: 2, format_by: Some(RELEASE.to_string()) }
        );
        assert_eq!(store.owed_projections().expect("ledger").len(), 1, "the step from 1 to 2 ran");
        drop(store);
        let copy = copy_beside(&path, 1);
        let kept = Database::create(&copy).expect("the copy opens");
        let txn = kept.begin_read().expect("txn");
        assert!(
            !txn.list_tables()
                .expect("tables")
                .any(|t| redb::TableHandle::name(&t) == "owed_projection"),
            "the copy is the store before the upgrade"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// A record this build cannot parse is a meaning it does not know: refused, not guessed.
    #[test]
    fn a_format_record_that_is_not_a_number_is_refused() {
        let path = tmp_path();
        drop(RedbStore::open(&path).expect("create"));
        stamp(&path, "3-beta", "2", "9.9.9");
        let Err(e) = RedbStore::open(&path) else {
            panic!("an unparseable format must be refused");
        };
        assert!(e.to_string().contains("3-beta"), "{e}");
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// Section 3.7: redb's refusal of a newer file format reads as corruption, and is reported as
    /// what it is - a later release's file.
    #[test]
    fn a_newer_redb_file_format_is_reported_as_a_later_release() {
        let e = redb::DatabaseError::Storage(redb::StorageError::Corrupted(
            "Expected file format version <= 3, found 4".into(),
        ));
        let msg = open_error(Path::new("/x/knowledge.redb"), e).to_string();
        assert!(msg.contains("newer storage engine"), "{msg}");
        assert!(msg.contains("nothing was opened or changed"), "{msg}");
        let other = redb::DatabaseError::Storage(redb::StorageError::Corrupted("bad page".into()));
        assert!(!open_error(Path::new("/x"), other).to_string().contains("newer"));
    }

    /// The probe sees a writer and nothing else: held while a store is open, free once it is dropped,
    /// and an absent file is not held (and is not created by asking).
    #[test]
    fn redb_in_use_sees_a_writer_and_only_a_writer() {
        let path = tmp_path();
        assert!(!redb_in_use(&path), "absent");
        assert!(!path.exists(), "asking does not create the store");
        let store = RedbStore::open(&path).expect("open");
        assert!(redb_in_use(&path), "a writer holds it");
        drop(store);
        assert!(!redb_in_use(&path), "released when the writer closes");
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    /// The ledger is only worth anything if it outlives the process that wrote it - recovery reads
    /// it in the NEXT process (crash-recovery.md K3). An entry written and never cleared is there
    /// after a reopen; a cleared one is not.
    #[test]
    fn redb_owed_projections_survive_a_reopen() {
        let path = tmp_path();
        let (kept, cleared);
        {
            let store = RedbStore::open(&path).expect("open");
            let a = Observation::new("projected before the crash".into(), prov_in("ws1"));
            let b = Observation::new("appended, never projected".into(), prov_in("ws1"));
            cleared = a.id.clone();
            kept = b.id.clone();
            store.add_observation(a).expect("append a");
            store.add_observation(b).expect("append b");
            store.clear_owed(std::slice::from_ref(&cleared)).expect("clear a");
        }
        let store = RedbStore::open(&path).expect("reopen");
        assert_eq!(store.owed_projections().expect("ledger"), vec![(kept, "ws1".to_string())]);
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
    /// loudly. Reopening under a different embedder is refused because the failure has to happen at
    /// open, not at the first bad ranking.
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
