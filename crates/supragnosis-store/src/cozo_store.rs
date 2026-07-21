//! Cozo(RocksDB) adapter - file-based persistent store.
//!
//! Stores knowledge in 3 stored relations and queries with CozoScript (Datalog).
//! Composite fields (aliases/properties/provenance) are stored as JSON string columns.
//! `traverse` performs shortest-hop graph traversal with Cozo recursive Datalog (min aggregation).

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use cozo::{DataValue, Db, NamedRows, RocksDbStorage, ScriptMutability};
use serde_json::json;

use supragnosis_core::{
    cosine_similarity, Assertions, Entity, KnowledgeStore, Observation, Provenance, Relation,
    SearchHit, SearchHitKind, StoreError, TraverseHit,
};

pub struct CozoStore {
    db: Db<RocksDbStorage>,
    /// Vector index dimension. When Some(n), keeps the obs_vec relation + HNSW index and handles
    /// semantic search with native ANN. When None, keeps embeddings only in data JSON and brute-forces.
    vector_dim: Option<usize>,
    /// Atomicity guard for observation merge (read-merge-write). The port contract (`Send + Sync + &self`)
    /// allows concurrent calls, but the baseline read and the put are separate transactions, so if two
    /// threads enter concurrently with the same id the last put erases the other's attestation (Principle 3 violation).
    /// InMemory merges atomically inside the RwLock write guard - this mutex is that parity.
    merge_lock: Mutex<()>,
}

impl CozoStore {
    /// Opens without a vector index (semantic search is brute-force cosine).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_inner(path, None)
    }

    /// Opens with a specified embedder - creates the obs_vec/ent_vec relations + HNSW indexes to
    /// accelerate semantic search with native ANN. `embedder_id` (model name + dimension, [`EmbeddingProvider::id`])
    /// is recorded in meta, and reopening with a different embedder is **explicitly rejected** - this turns
    /// into fail-fast the silent errors where old/new vectors from different vector spaces mix in one index
    /// (same dimension, different model), or writes partially fail on a dimension mismatch (different dimension).
    pub fn open_with_embedder(
        path: impl AsRef<Path>,
        embedder_id: &str,
        dim: usize,
    ) -> Result<Self, StoreError> {
        Self::open_inner(path, Some((embedder_id.to_string(), dim)))
    }

    fn open_inner(
        path: impl AsRef<Path>,
        embedder: Option<(String, usize)>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref();
        std::fs::create_dir_all(path).map_err(|e| StoreError::Backend(e.to_string()))?;
        let path_str = path.to_string_lossy().to_string();
        let db =
            cozo::new_cozo_rocksdb(&path_str).map_err(|e| StoreError::Backend(e.to_string()))?;
        let store = Self {
            db,
            vector_dim: embedder.as_ref().map(|(_, d)| *d),
            merge_lock: Mutex::new(()),
        };
        store.ensure_schema()?;
        if let Some((id, _)) = &embedder {
            store.ensure_embedder(id)?;
        }
        Ok(store)
    }

    /// Checks the embedder identifier in meta: record if first, pass if same, reject if different.
    fn ensure_embedder(&self, embedder_id: &str) -> Result<(), StoreError> {
        let rows = self.run(
            "?[value] := *meta{key: \"embedder\", value}",
            BTreeMap::new(),
            false,
        )?;
        let stored = rows
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.get_str())
            .map(str::to_string);
        match stored {
            None => {
                let p = params(&[("value", embedder_id.to_string())]);
                self.run(
                    "?[key, value] <- [[\"embedder\", $value]]\n:put meta {key => value}",
                    p,
                    true,
                )?;
                Ok(())
            }
            Some(s) if s == embedder_id => Ok(()),
            Some(s) => Err(StoreError::Backend(format!(
                "embedder mismatch: this store was indexed with '{s}' but the current embedder is \
                 '{embedder_id}'. Revert to the same embedder (SUPRAGNOSIS_EMBED), or to reindex, \
                 reload into a new data directory (SUPRAGNOSIS_DATA_DIR) \
                 - mixing different vector spaces in one index makes similarity meaningless"
            ))),
        }
    }

    fn run(
        &self,
        script: &str,
        params: BTreeMap<String, DataValue>,
        mutable: bool,
    ) -> Result<NamedRows, StoreError> {
        let m = if mutable {
            ScriptMutability::Mutable
        } else {
            ScriptMutability::Immutable
        };
        self.db
            .run_script(script, params, m)
            .map_err(|e| StoreError::Backend(e.to_string()))
    }

    fn ensure_schema(&self) -> Result<(), StoreError> {
        let existing = self.run("::relations", BTreeMap::new(), false)?;
        let name_idx = existing
            .headers
            .iter()
            .position(|h| h == "name")
            .unwrap_or(0);
        let have: HashSet<String> = existing
            .rows
            .iter()
            .filter_map(|r| {
                r.get(name_idx)
                    .and_then(|v| v.get_str())
                    .map(str::to_string)
            })
            .collect();

        if !have.contains("observation") {
            self.run(
                ":create observation {id: String => content: String, workspace: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        if !have.contains("entity") {
            self.run(
                ":create entity {id: String => etype: String, name: String, workspace: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        if !have.contains("relation") {
            self.run(
                ":create relation {id: String => src: String, dst: String, rtype: String, data: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        // Store metadata (embedder identifier, etc.) - the baseline for detecting an adapter swap.
        if !have.contains("meta") {
            self.run(
                ":create meta {key: String => value: String}",
                BTreeMap::new(),
                true,
            )?;
        }
        // Vector index: obs_vec/ent_vec relations + HNSW. relation and index are created together, so
        // absence of the relation is treated as absence of both (create-together invariant). Observations
        // and entities are indexed separately so semantic search recalls from both observation content
        // and entity names (removing the recall gap).
        if let Some(dim) = self.vector_dim {
            for rel in ["obs_vec", "ent_vec"] {
                if !have.contains(rel) {
                    self.run(
                        &format!(":create {rel} {{id: String => vec: <F32; {dim}>}}"),
                        BTreeMap::new(),
                        true,
                    )?;
                    self.run(
                        &format!(
                            "::hnsw create {rel}:idx {{dim: {dim}, dtype: F32, fields: [vec], distance: Cosine, m: 16, ef_construction: 32}}"
                        ),
                        BTreeMap::new(),
                        true,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Builds a CozoScript list literal `[a,b,c]` from an f32 vector. Values are our own f32, so no injection risk.
    /// On :put it is coerced to a `<F32; N>` column, and in queries it is wrapped with `vec([..])`.
    fn list_literal(v: &[f32]) -> String {
        let mut s = String::from("[");
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            // Full-precision, round-trippable notation.
            s.push_str(&format!("{x:?}"));
        }
        s.push(']');
        s
    }

    /// Approximate nearest-neighbor (ANN) search via HNSW index. Observations (obs_vec/observation/content)
    /// and entities (ent_vec/entity/name) share the same logic. The workspace filter is applied after the
    /// join, so candidates (k) are taken generously above limit so the limit is filled even after filtering.
    fn semantic_hnsw(
        &self,
        query: &[f32],
        workspace: Option<&str>,
        limit: usize,
        target: &SemanticTarget,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let (index, relation, text_field, kind) =
            (target.index, target.relation, target.text_field, target.kind);
        let k = (limit * 4).max(16);
        let ef = (k * 2).max(32);
        let q = Self::list_literal(query);
        let mut p = params(&[]);
        // Bind text_field as text to handle observation content / entity name in one form.
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                format!(
                    "?[id, text, dist] := qv = vec({q}), ~{index}{{id | query: qv, k: {k}, ef: {ef}, bind_distance: dist}}, *{relation}{{id, {text_field}: text, workspace}}, workspace == $ws\n\
                     :order dist, id\n\
                     :limit {limit}"
                )
            }
            None => format!(
                "?[id, text, dist] := qv = vec({q}), ~{index}{{id | query: qv, k: {k}, ef: {ef}, bind_distance: dist}}, *{relation}{{id, {text_field}: text}}\n\
                 :order dist, id\n\
                 :limit {limit}"
            ),
        };
        let rows = self.run(&script, p, false)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| {
                let id = r.first()?.get_str()?;
                let text = r.get(1)?.get_str()?;
                // Cozo Cosine distance = 1 - cosine similarity. Convert back to similarity to use as score.
                let dist = r.get(2)?.get_float()? as f32;
                Some(SearchHit {
                    kind,
                    id: id.to_string(),
                    snippet: text.chars().take(160).collect(),
                    score: 1.0 - dist,
                })
            })
            .collect())
    }

    /// Brute-force semantic search when there is no vector index. Loads the embedding from data JSON
    /// and ranks by cosine similarity in Rust. Observations (observation/content) and entities (entity/name)
    /// share this, and both relations have a workspace column so filtering happens directly in the store.
    fn semantic_brute(
        &self,
        query: &[f32],
        workspace: Option<&str>,
        limit: usize,
        target: &SemanticTarget,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let (relation, text_field, kind) = (target.relation, target.text_field, target.kind);
        let mut p = params(&[]);
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                format!(
                    "?[id, text, data] := *{relation}{{id, {text_field}: text, workspace, data}}, workspace == $ws"
                )
            }
            None => {
                format!("?[id, text, data] := *{relation}{{id, {text_field}: text, data}}")
            }
        };
        let rows = self.run(&script, p, false)?;
        let mut hits: Vec<SearchHit> = rows
            .rows
            .iter()
            .filter_map(|r| {
                let id = r.first()?.get_str()?;
                let text = r.get(1)?.get_str()?;
                let data: serde_json::Value = serde_json::from_str(r.get(2)?.get_str()?).ok()?;
                let emb: Vec<f32> = serde_json::from_value(data.get("embedding")?.clone()).ok()?;
                Some(SearchHit {
                    kind,
                    id: id.to_string(),
                    snippet: text.chars().take(160).collect(),
                    score: cosine_similarity(query, &emb),
                })
            })
            .collect();

        // Break ties by id for a stable sort - keep query row order from leaking into results (Principle 16: reproducibility).
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

/// (key, string-value) pairs into a CozoScript parameter map. (Parameterization prevents injection)
fn params(pairs: &[(&str, String)]) -> BTreeMap<String, DataValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), DataValue::from(v.as_str())))
        .collect()
}

/// Semantic search target descriptor: which HNSW index/stored relation/text column to treat as which
/// hit kind. Observations (obs_vec/observation/content) and entities (ent_vec/entity/name) share the
/// same search logic, differing only in this descriptor.
struct SemanticTarget {
    /// HNSW index name (ignored on the brute-force path).
    index: &'static str,
    /// stored relation name.
    relation: &'static str,
    /// Text column that is the source of the snippet (observation content / entity name).
    text_field: &'static str,
    /// Kind of the resulting hit.
    kind: SearchHitKind,
}

const OBS_TARGET: SemanticTarget = SemanticTarget {
    index: "obs_vec:idx",
    relation: "observation",
    text_field: "content",
    kind: SearchHitKind::Observation,
};

const ENT_TARGET: SemanticTarget = SemanticTarget {
    index: "ent_vec:idx",
    relation: "entity",
    text_field: "name",
    kind: SearchHitKind::Entity,
};

/// Reconstructs an observation row (content, data JSON) into a domain Observation.
/// Returns None if a required field (content/provenance) is broken - the caller promotes it to a backend failure.
fn observation_from_row(id: &str, row: &[DataValue]) -> Option<Observation> {
    let content = row.first()?.get_str()?.to_string();
    let data: serde_json::Value = serde_json::from_str(row.get(1)?.get_str()?).ok()?;
    let provenance: Vec<Provenance> =
        serde_json::from_value(data.get("provenance")?.clone()).ok()?;
    let assertions: Assertions = data
        .get("assertions")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let derived_from: Vec<String> = data
        .get("derived_from")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let embedding: Option<Vec<f32>> = data
        .get("embedding")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    Some(Observation {
        id: id.to_string(),
        content,
        provenance,
        assertions,
        derived_from,
        embedding,
    })
}

/// Reconstructs an entity row (id, etype, name, data JSON) into a domain Entity. Shared by lookup/enumeration.
/// Even if the data JSON is broken, the schema columns (id/type/name) are kept and only the composite fields are emptied (defensive).
fn entity_from_parts(id: String, etype: String, name: String, data_str: &str) -> Entity {
    let data: serde_json::Value =
        serde_json::from_str(data_str).unwrap_or(serde_json::Value::Null);
    Entity {
        id,
        kind: etype,
        canonical_name: name,
        aliases: data
            .get("aliases")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        // Absent (old schema) or JSON null -> None.
        description: data
            .get("description")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        properties: data
            .get("properties")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        provenance: data
            .get("provenance")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        // Reconstruct the embedding so it is not lost on an upsert (get -> put) round-trip. None if absent (old schema).
        embedding: data
            .get("embedding")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
    }
}

/// Reconstructs a relation row (id, src, dst, rtype, data JSON) into a domain Relation.
/// Returns None if provenance parsing fails - a relation without provenance violates Principle 2, so it is not leaked through.
fn relation_from_parts(
    id: &str,
    src: &str,
    dst: &str,
    rtype: &str,
    data_str: &str,
) -> Option<Relation> {
    let data: serde_json::Value = serde_json::from_str(data_str).ok()?;
    Some(Relation {
        id: id.to_string(),
        from: src.to_string(),
        to: dst.to_string(),
        kind: rtype.to_string(),
        description: data
            .get("description")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        provenance: serde_json::from_value(data.get("provenance")?.clone()).ok()?,
        valid_from: data.get("valid_from").and_then(|v| v.as_u64()),
        valid_to: data.get("valid_to").and_then(|v| v.as_u64()),
    })
}

impl KnowledgeStore for CozoStore {
    /// Reconstructs an observation by id from the observation log (baseline read for back-reference + re-arrival merge).
    /// Revives provenance (attestation list)/assertions/derived_from/embedding from the data JSON.
    /// Absence is `Ok(None)`, backend failure/row corruption is `Err` - if the merge baseline read mistakes
    /// a failure for absence, it becomes an overwrite instead of absorb and destroys attestations (Principle 3), so the two must be distinguished.
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        let p = params(&[("id", id.to_string())]);
        let rows = self.run(
            "?[content, data] := *observation{id: $id, content, data}",
            p,
            false,
        )?;
        let Some(row) = rows.rows.into_iter().next() else {
            return Ok(None);
        };
        match observation_from_row(id, &row) {
            Some(obs) => Ok(Some(obs)),
            None => Err(StoreError::Backend(format!(
                "observation row reconstruction failed (data JSON corruption - a backend failure, not absence): {id}"
            ))),
        }
    }

    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        // Make read-merge-write atomic with a mutex - so concurrent re-arrivals cannot erase each
        // other's attestations (concurrency-semantics parity with InMemory's write-guard merge, Principle 3).
        let _guard = self.merge_lock.lock().unwrap();
        // A re-arrival at the same content address is absorbed as a monotonic union, not an overwrite
        // (Principle 3: log immutability). Read the existing row, merge attestations/lineage, then write.
        // Propagate a baseline read failure - mistaking a failure for absence turns absorb into an overwrite.
        let obs = match self.get_observation(&obs.id)? {
            Some(mut existing) => {
                existing.absorb(obs);
                existing
            }
            None => obs,
        };
        let workspace = obs.workspace().to_string();
        let obs_id = obs.id.clone();
        let embedding = obs.embedding.clone();
        // Composite fields go in the data JSON column (provenance + assertions + derived_from lineage + embedding).
        // assertions are the input to reprojection, so they must persist together with the log (Principle 1).
        // The embedding is only a recall aid, not identity (Principle 19) - keep it in data, not a schema column.
        let data = json!({
            "provenance": obs.provenance,
            "assertions": obs.assertions,
            "derived_from": obs.derived_from,
            "embedding": obs.embedding,
        })
        .to_string();
        let p = params(&[
            ("id", obs.id),
            ("content", obs.content),
            ("workspace", workspace),
            ("data", data),
        ]);
        self.run(
            "?[id, content, workspace, data] <- [[$id, $content, $workspace, $data]]\n\
             :put observation {id => content, workspace, data}",
            p,
            true,
        )?;

        // If the vector index is on and the embedding dimension matches, also load into obs_vec (HNSW).
        // The embedding in data JSON is the source, and obs_vec is a materialized index for ANN acceleration.
        if let (Some(dim), Some(emb)) = (self.vector_dim, embedding) {
            if emb.len() == dim {
                let script = format!(
                    "?[id, vec] := id = $id, vec = {}\n:put obs_vec {{id => vec}}",
                    Self::list_literal(&emb)
                );
                self.run(&script, params(&[("id", obs_id)]), true)?;
            }
        }
        Ok(())
    }

    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError> {
        let p = params(&[("id", id.to_string())]);
        let rows = self.run(
            "?[etype, name, data] := *entity{id: $id, etype, name, data}",
            p,
            false,
        )?;
        let Some(row) = rows.rows.into_iter().next() else {
            return Ok(None);
        };
        let (Some(etype), Some(name), Some(data_str)) = (
            row.first().and_then(|v| v.get_str()),
            row.get(1).and_then(|v| v.get_str()),
            row.get(2).and_then(|v| v.get_str()),
        ) else {
            return Err(StoreError::Backend(format!(
                "entity row reconstruction failed (schema column corruption - a backend failure, not absence): {id}"
            )));
        };
        Ok(Some(entity_from_parts(
            id.to_string(),
            etype.to_string(),
            name.to_string(),
            data_str,
        )))
    }

    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        let workspace = entity
            .provenance
            .first()
            .map(|p| p.workspace.clone())
            .unwrap_or_default();
        let embedding = entity.embedding.clone();
        let entity_id = entity.id.clone();
        // The embedding is only a recall aid, not identity (Principle 19) - keep it in data, not a schema column.
        // The data JSON is the source and ent_vec is a materialized index for ANN acceleration (symmetric with obs_vec).
        let data = json!({
            "aliases": entity.aliases,
            "description": entity.description,
            "properties": entity.properties,
            "provenance": entity.provenance,
            "embedding": entity.embedding,
        })
        .to_string();
        let p = params(&[
            ("id", entity.id),
            ("etype", entity.kind),
            ("name", entity.canonical_name),
            ("workspace", workspace),
            ("data", data),
        ]);
        self.run(
            "?[id, etype, name, workspace, data] <- [[$id, $etype, $name, $workspace, $data]]\n\
             :put entity {id => etype, name, workspace, data}",
            p,
            true,
        )?;

        // If the vector index is on and the embedding dimension matches, also load into ent_vec (HNSW).
        if let (Some(dim), Some(emb)) = (self.vector_dim, embedding) {
            if emb.len() == dim {
                let script = format!(
                    "?[id, vec] := id = $id, vec = {}\n:put ent_vec {{id => vec}}",
                    Self::list_literal(&emb)
                );
                self.run(&script, params(&[("id", entity_id)]), true)?;
            }
        }
        Ok(())
    }

    fn add_relation(&self, rel: Relation) -> Result<(), StoreError> {
        // Composite/temporal fields go in the data JSON column (provenance + valid interval).
        let data = json!({
            "provenance": rel.provenance,
            "description": rel.description,
            "valid_from": rel.valid_from,
            "valid_to": rel.valid_to,
        })
        .to_string();
        let p = params(&[
            ("id", rel.id),
            ("src", rel.from),
            ("dst", rel.to),
            ("rtype", rel.kind),
            ("data", data),
        ]);
        self.run(
            "?[id, src, dst, rtype, data] <- [[$id, $src, $dst, $rtype, $data]]\n\
             :put relation {id => src, dst, rtype, data}",
            p,
            true,
        )?;
        Ok(())
    }

    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError> {
        let p = params(&[("id", entity_id.to_string())]);
        let script = "?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}, src == $id\n\
                      ?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}, dst == $id";
        let rows = self.run(script, p, false)?;
        let mut rels: Vec<Relation> = rows
            .rows
            .iter()
            .filter_map(|r| {
                relation_from_parts(
                    r.first()?.get_str()?,
                    r.get(1)?.get_str()?,
                    r.get(2)?.get_str()?,
                    r.get(3)?.get_str()?,
                    r.get(4)?.get_str()?,
                )
            })
            .collect();
        // Sort by id - make explicit that row order does not leak into the response (parity with InMemory, Principle 16).
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

        // Entity: substring match on canonical name.
        let mut ent_params = params(&[("q", q.clone())]);
        let ent_script = match workspace {
            Some(ws) => {
                ent_params.insert("ws".to_string(), DataValue::from(ws));
                "?[id, name] := *entity{id, name, workspace}, str_includes(lowercase(name), $q), workspace == $ws"
            }
            None => "?[id, name] := *entity{id, name}, str_includes(lowercase(name), $q)",
        };
        // A partial failure is an Err, not a partial result - returning observation hits while only the
        // entity query failed would make the caller misread it as "zero entities" (Principle 5: absence != backend failure).
        let rows = self.run(ent_script, ent_params, false)?;
        for r in &rows.rows {
            if let (Some(id), Some(name)) = (
                r.first().and_then(|v| v.get_str()),
                r.get(1).and_then(|v| v.get_str()),
            ) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    id: id.to_string(),
                    snippet: name.to_string(),
                    score: 1.0,
                });
            }
        }

        // Observation: substring match on content.
        let mut obs_params = params(&[("q", q.clone())]);
        let obs_script = match workspace {
            Some(ws) => {
                obs_params.insert("ws".to_string(), DataValue::from(ws));
                "?[id, content] := *observation{id, content, workspace}, str_includes(lowercase(content), $q), workspace == $ws"
            }
            None => {
                "?[id, content] := *observation{id, content}, str_includes(lowercase(content), $q)"
            }
        };
        let rows = self.run(obs_script, obs_params, false)?;
        for r in &rows.rows {
            if let (Some(id), Some(content)) = (
                r.first().and_then(|v| v.get_str()),
                r.get(1).and_then(|v| v.get_str()),
            ) {
                hits.push(SearchHit {
                    kind: SearchHitKind::Observation,
                    id: id.to_string(),
                    snippet: content.chars().take(160).collect(),
                    score: 0.7,
                });
            }
        }

        // Break ties by id for a stable sort - keep query row order from leaking into results (Principle 16: reproducibility).
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
        let md = max_depth.max(1);
        // Recursive Datalog: compute shortest-hop distance with min aggregation and join entity metadata.
        // Without :order the results came out in id (first-column) order and :limit truncation ignored depth
        // - sort explicitly by (depth, id) to prefer nearer neighbors + make truncation reproducible, and
        // match the truncation semantics with the InMemory adapter (Principle 16).
        let script = format!(
            "reach[dst, min(d)] := *relation{{src: $start, dst}}, d = 1\n\
             reach[dst, min(d)] := reach[nid, d0], *relation{{src: nid, dst}}, d = d0 + 1, d <= {md}\n\
             ?[id, depth, name, etype] := reach[id, depth], *entity{{id, name, etype}}\n\
             :order depth, id\n\
             :limit {limit}"
        );
        let p = params(&[("start", start_id.to_string())]);
        let rows = self.run(&script, p, false)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| {
                Some(TraverseHit {
                    id: r.first()?.get_str()?.to_string(),
                    depth: r.get(1)?.get_int()? as usize,
                    name: r.get(2)?.get_str()?.to_string(),
                    kind: r.get(3)?.get_str()?.to_string(),
                })
            })
            .collect())
    }

    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError> {
        // The entity table has a workspace column, so filter directly in the store.
        let mut p = params(&[]);
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                "?[id, etype, name, data] := *entity{id, etype, name, workspace, data}, workspace == $ws"
            }
            None => "?[id, etype, name, data] := *entity{id, etype, name, data}",
        };
        let rows = self.run(script, p, false)?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| {
                Some(entity_from_parts(
                    r.first()?.get_str()?.to_string(),
                    r.get(1)?.get_str()?.to_string(),
                    r.get(2)?.get_str()?.to_string(),
                    r.get(3)?.get_str()?,
                ))
            })
            .collect())
    }

    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError> {
        // The relation table has no workspace column - the workspace is in provenance.workspace within
        // the data JSON, so filter in Rust after reconstruction.
        let rows = self.run(
            "?[id, src, dst, rtype, data] := *relation{id, src, dst, rtype, data}",
            params(&[]),
            false,
        )?;
        Ok(rows
            .rows
            .iter()
            .filter_map(|r| {
                relation_from_parts(
                    r.first()?.get_str()?,
                    r.get(1)?.get_str()?,
                    r.get(2)?.get_str()?,
                    r.get(3)?.get_str()?,
                    r.get(4)?.get_str()?,
                )
            })
            .filter(|rel| workspace.is_none_or(|ws| rel.provenance.workspace == ws))
            .collect())
    }

    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        // The observation table has a workspace column, so filter directly in the store.
        let mut p = params(&[]);
        let script = match workspace {
            Some(ws) => {
                p.insert("ws".to_string(), DataValue::from(ws));
                "?[id, content, data] := *observation{id, content, workspace, data}, workspace == $ws"
            }
            None => "?[id, content, data] := *observation{id, content, data}",
        };
        let rows = self.run(script, p, false)?;
        // A row reconstruction failure is a backend failure, not absence (Principle 5) - do not swallow it in enumeration either.
        // observation_from_row takes a [content, data] slice, so pass r[1..].
        let mut out = Vec::with_capacity(rows.rows.len());
        for r in &rows.rows {
            let Some(id) = r.first().and_then(|v| v.get_str()) else {
                return Err(StoreError::Backend(
                    "observation enumeration: id column corruption (a backend failure, not absence)".into(),
                ));
            };
            match observation_from_row(id, &r[1..]) {
                Some(obs) => out.push(obs),
                // A single row's data JSON reconstruction failure **does not block the whole enumeration**
                // (a degrade that prevents the derived overlay from becoming entirely unusable because of one
                // row, Principle 19). It is not silent - the exclusion is logged as a warning (Principle 5: a
                // backend failure, not absence). The original stays in the log layer, so it is a recovery target
                // in reprojection (M3), not a drop.
                None => tracing::warn!(
                    observation_id = %id,
                    "observation row reconstruction failed - excluded from enumeration (degrade). Original preserved in the log"
                ),
            }
        }
        Ok(out)
    }

    fn search_semantic(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        // If the vector index is on, use native HNSW ANN.
        if self.vector_dim.is_some() {
            return self.semantic_hnsw(query_embedding, workspace, limit, &OBS_TARGET);
        }
        // Otherwise brute-force: load candidates and rank by cosine similarity in Rust.
        self.semantic_brute(query_embedding, workspace, limit, &OBS_TARGET)
    }

    fn search_semantic_entities(
        &self,
        query_embedding: &[f32],
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        // Entities are symmetric with observations: HNSW(ent_vec) acceleration, brute-force cosine otherwise.
        if self.vector_dim.is_some() {
            return self.semantic_hnsw(query_embedding, workspace, limit, &ENT_TARGET);
        }
        self.semantic_brute(query_embedding, workspace, limit, &ENT_TARGET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InMemoryStore;
    use supragnosis_core::Provenance;

    fn tmp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // A process-atomic counter guarantees uniqueness - relying on wall-clock resolution would let two
        // concurrently running Cozo tests grab the same path at the same nanosecond, failing open on a RocksDB lock conflict.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "supragnosis-cozo-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: "ws1".into(),
            source_ref: None,
            observed_at: 1,
            confidence: Some(1.0),
            trust_tier: supragnosis_core::TrustTier::default(),
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

    #[test]
    fn cozo_roundtrip_search_traverse_persist() {
        let dir = tmp_dir();
        {
            let store = CozoStore::open(&dir).unwrap();

            for n in ["a", "b", "c"] {
                store.put_entity(ent(n)).unwrap();
            }
            let (a, b, c) = (
                Entity::make_id("ws1", "a"),
                Entity::make_id("ws1", "b"),
                Entity::make_id("ws1", "c"),
            );
            store
                .add_relation(Relation { description: None,
                    id: Relation::make_id(&a, "rel", &b),
                    from: a.clone(),
                    to: b.clone(),
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
            store
                .add_relation(Relation { description: None,
                    id: Relation::make_id(&b, "rel", &c),
                    from: b.clone(),
                    to: c.clone(),
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
            store
                .add_observation(Observation::new("hello rust world".into(), prov()))
                .unwrap();

            // Lookup
            assert_eq!(
                store.get_entity(&a).unwrap().unwrap().canonical_name,
                "a"
            );
            // b appears in two relations, a->b and b->c
            assert_eq!(store.relations_of(&b).unwrap().len(), 2);
            // Search
            assert!(!store.search("rust", Some("ws1"), 10).unwrap().is_empty());
            assert!(store.search("rust", Some("other"), 10).unwrap().is_empty());
            // Traverse: a -> b (1 hop), c (2 hops)
            let hits = store.traverse(&a, 5, 100).unwrap();
            let ids: HashSet<_> = hits.iter().map(|h| h.id.clone()).collect();
            assert!(
                ids.contains(&b) && ids.contains(&c),
                "traverse got {hits:?}"
            );
        }

        // Persistence: data survives after reopen
        {
            let store2 = CozoStore::open(&dir).unwrap();
            let a = Entity::make_id("ws1", "a");
            assert!(
                store2.get_entity(&a).unwrap().is_some(),
                "data should persist"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// traverse is deterministic in (depth asc, id asc) and limit truncation prefers shallower depth -
    /// the order/truncation semantics of the two adapters match (Principle 16, a remnant of the search-sort tie-break).
    /// The fixture is hash-selected: the min-id candidate is placed as a depth-2 grandchild so that, under a
    /// regression to id-order truncation, the grandchild would have pushed out a depth-1 neighbor.
    #[test]
    fn traverse_order_and_truncation_parity_across_adapters() {
        let mut cands: Vec<String> = (0..30).map(|i| format!("child-{i:02}")).collect();
        cands.sort_by_key(|n| Entity::make_id("ws1", n));
        let grand = cands[0].clone();
        let children: Vec<String> = cands[cands.len() - 6..].to_vec();
        assert!(
            Entity::make_id("ws1", &grand) < Entity::make_id("ws1", &children[0]),
            "fixture premise: the grandchild id is smaller than every child id"
        );

        let fill = |store: &dyn KnowledgeStore| {
            store.put_entity(ent("root")).unwrap();
            for name in &children {
                store.put_entity(ent(name)).unwrap();
                let (f, t) = (Entity::make_id("ws1", "root"), Entity::make_id("ws1", name));
                store
                    .add_relation(Relation { description: None,
                        id: Relation::make_id(&f, "rel", &t),
                        from: f,
                        to: t,
                        kind: "rel".into(),
                        provenance: prov(),
                        valid_from: None,
                        valid_to: None,
                    })
                    .unwrap();
            }
            store.put_entity(ent(&grand)).unwrap();
            let (f, t) = (
                Entity::make_id("ws1", &children[0]),
                Entity::make_id("ws1", &grand),
            );
            store
                .add_relation(Relation { description: None,
                    id: Relation::make_id(&f, "rel", &t),
                    from: f,
                    to: t,
                    kind: "rel".into(),
                    provenance: prov(),
                    valid_from: None,
                    valid_to: None,
                })
                .unwrap();
        };
        let check = |store: &dyn KnowledgeStore, label: &str| -> Vec<(usize, String)> {
            let root = Entity::make_id("ws1", "root");
            let full = store.traverse(&root, 5, 100).unwrap();
            let keys: Vec<(usize, String)> =
                full.iter().map(|h| (h.depth, h.id.clone())).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "{label}: must be in (depth, id) order");
            // Truncation prefers nearer neighbors - the min-id grandchild (depth 2) does not push out depth-1.
            let cut = store.traverse(&root, 5, 4).unwrap();
            assert!(
                cut.iter().all(|h| h.depth == 1),
                "{label}: limit truncation must prefer shallower depth: {cut:?}"
            );
            keys
        };

        // InMemory: same result regardless of instance (HashMap seed).
        let m1 = InMemoryStore::new();
        fill(&m1);
        let k1 = check(&m1, "mem-1");
        let m2 = InMemoryStore::new();
        fill(&m2);
        let k2 = check(&m2, "mem-2");
        assert_eq!(k1, k2, "reproducibility across InMemory instances");

        // Cozo: same order/truncation semantics (adapter parity).
        let dir = tmp_dir();
        {
            let store = CozoStore::open(&dir).unwrap();
            fill(&store);
            let kc = check(&store, "cozo");
            assert_eq!(k1, kc, "InMemory <-> Cozo parity");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Principle 3: the Cozo adapter also absorbs re-arrivals as a union, and the merge result persists.
    #[test]
    fn cozo_reobservation_accumulates_attestations() {
        let dir = tmp_dir();
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
        let id = make("host-a", 0.9, "o1").id;
        {
            let store = CozoStore::open(&dir).unwrap();
            store.add_observation(make("host-a", 0.9, "o1")).unwrap();
            store.add_observation(make("host-b", 0.1, "o2")).unwrap();

            let o = store.get_observation(&id).unwrap().unwrap();
            assert_eq!(o.provenance.len(), 2, "attestation accumulation: {:?}", o.provenance);
            assert_eq!(o.derived_from, vec!["o1".to_string(), "o2".to_string()]);
        }
        // The merge result survives after reopen.
        {
            let store = CozoStore::open(&dir).unwrap();
            let o = store.get_observation(&id).unwrap().unwrap();
            assert_eq!(o.provenance.len(), 2);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn prov_ws(ws: &str) -> Provenance {
        Provenance {
            workspace: ws.into(),
            ..prov()
        }
    }

    fn obs_with_emb(content: &str, ws: &str, emb: [f32; 3]) -> Observation {
        let mut o = Observation::new(content.into(), prov_ws(ws));
        o.embedding = Some(emb.to_vec());
        o
    }

    /// Opening with a specified vector dimension makes semantic search use native HNSW. Verifies
    /// nearest-neighbor ranking + workspace filter, and checks the index survives after reopen.
    #[test]
    fn cozo_semantic_search_uses_hnsw() {
        let dir = tmp_dir();
        let a = Observation::new("x axis".into(), prov_ws("ws1")).id;
        let c = Observation::new("near x".into(), prov_ws("ws1")).id;
        {
            let store = CozoStore::open_with_embedder(&dir, "test-3d", 3).unwrap();
            store
                .add_observation(obs_with_emb("x axis", "ws1", [1.0, 0.0, 0.0]))
                .unwrap();
            store
                .add_observation(obs_with_emb("y axis", "ws1", [0.0, 1.0, 0.0]))
                .unwrap();
            store
                .add_observation(obs_with_emb("near x", "ws1", [0.9, 0.1, 0.0]))
                .unwrap();
            // Exact match in a different workspace - must not appear in the ws1 query.
            store
                .add_observation(obs_with_emb("other x", "ws2", [1.0, 0.0, 0.0]))
                .unwrap();

            let hits = store
                .search_semantic(&[1.0, 0.0, 0.0], Some("ws1"), 2)
                .unwrap();
            let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
            assert_eq!(ids.len(), 2, "top-2 within ws1, got {ids:?}");
            assert!(
                ids.contains(&a) && ids.contains(&c),
                "nearest should be x/near-x, got {ids:?}"
            );
            // The nearest (exact match) ranks first.
            assert_eq!(hits[0].id, a, "exact match should rank first");
        }

        // Reopen: the HNSW index and vectors persist.
        {
            let store = CozoStore::open_with_embedder(&dir, "test-3d", 3).unwrap();
            let hits = store
                .search_semantic(&[1.0, 0.0, 0.0], Some("ws1"), 1)
                .unwrap();
            assert_eq!(hits.first().map(|h| h.id.clone()), Some(a.clone()));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Embedder identifier check: reopening with the same embedder passes, a different embedder is
    /// explicitly rejected. Turns the partial writes of a dimension mismatch, and the vector-space mixing
    /// of a same-dimension different model (both previously silent corruption), into a fail-fast at open time.
    #[test]
    fn cozo_rejects_embedder_switch() {
        let dir = tmp_dir();
        {
            let store = CozoStore::open_with_embedder(&dir, "hashing-3", 3).unwrap();
            store
                .add_observation(obs_with_emb("fact", "ws1", [1.0, 0.0, 0.0]))
                .unwrap();
        }
        // Reopen with the same embedder: passes.
        assert!(CozoStore::open_with_embedder(&dir, "hashing-3", 3).is_ok());
        // Different-dimension embedder: rejected + self-correction hint.
        let err = CozoStore::open_with_embedder(&dir, "other-4", 4)
            .err()
            .expect("a different-dimension embedder must be rejected");
        assert!(err.to_string().contains("embedder mismatch"), "{err}");
        // Even at the same dimension, a different model is rejected (prevents vector-space mixing).
        let err = CozoStore::open_with_embedder(&dir, "other-3", 3)
            .err()
            .expect("a different-model embedder must be rejected");
        assert!(err.to_string().contains("embedder mismatch"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn ent_with_emb(name: &str, ws: &str, emb: [f32; 3]) -> Entity {
        Entity {
            id: Entity::make_id(ws, name),
            kind: "Concept".into(),
            canonical_name: name.into(),
            aliases: vec![],
            description: None,
            properties: serde_json::Value::Null,
            provenance: vec![prov_ws(ws)],
            embedding: Some(emb.to_vec()),
        }
    }

    /// Entity semantic search (ent_vec HNSW): reaches nodes via name embeddings. Verifies nearest-neighbor
    /// ranking + workspace filter + reopen persistence, symmetric with the observation path (obs_vec).
    #[test]
    fn cozo_entity_semantic_search_uses_hnsw() {
        let dir = tmp_dir();
        let x = Entity::make_id("ws1", "x axis");
        let near = Entity::make_id("ws1", "near x");
        {
            let store = CozoStore::open_with_embedder(&dir, "test-3d", 3).unwrap();
            store.put_entity(ent_with_emb("x axis", "ws1", [1.0, 0.0, 0.0])).unwrap();
            store.put_entity(ent_with_emb("y axis", "ws1", [0.0, 1.0, 0.0])).unwrap();
            store.put_entity(ent_with_emb("near x", "ws1", [0.9, 0.1, 0.0])).unwrap();
            // Exact match in a different workspace - must not appear in the ws1 query.
            store.put_entity(ent_with_emb("other x", "ws2", [1.0, 0.0, 0.0])).unwrap();

            let hits = store
                .search_semantic_entities(&[1.0, 0.0, 0.0], Some("ws1"), 2)
                .unwrap();
            let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
            assert_eq!(ids.len(), 2, "top-2 within ws1, got {ids:?}");
            assert!(
                ids.contains(&x) && ids.contains(&near),
                "nearest should be x/near-x entities, got {ids:?}"
            );
            assert_eq!(hits[0].id, x, "exact-match entity should rank first");
            // Must be entity hits (not mixed with observations).
            assert!(hits.iter().all(|h| h.kind == SearchHitKind::Entity));
        }

        // Reopen: the ent_vec index and vectors persist.
        {
            let store = CozoStore::open_with_embedder(&dir, "test-3d", 3).unwrap();
            let hits = store
                .search_semantic_entities(&[1.0, 0.0, 0.0], Some("ws1"), 1)
                .unwrap();
            assert_eq!(hits.first().map(|h| h.id.clone()), Some(x.clone()));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
