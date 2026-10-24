//! The projection write path: entity and relation rows folded from the log.

use super::*;

impl Engine {
    /// The resolution write path (resolution-identity.md Section 4): (re)projects entity rows purely
    /// from the observation log. `only = None` projects every entity in the workspace (reproject);
    /// `only = Some(ids)` projects just those (the incremental observe path). Both run the SAME fold,
    /// so the incremental projection of a write equals a fresh replay's row for the same log (IR3) -
    /// there is no field-wise last-write interim to diverge.
    ///
    /// For each in-scope entity, purely from the log:
    /// - **kind**: the policy winner over kind candidates at their effective tiers (M3a).
    /// - **canonical_name**: the policy winner over the asserted spellings (M3a, arrival-order-free).
    /// - **aliases**: the distinct asserted spellings minus the representative, ordered by
    ///   (first-asserting ordering-HLC, spelling) - a deterministic set union that never drops a
    ///   spelling (Principle 3, IR1).
    /// - **description**: HLC-latest non-empty (never erased by a later omission - Principle 8).
    /// - **embedding**: the name-meaning retrieval aid (Principle 19), recomputed only when the
    ///   embedding text (canonical_name + aliases) changed since the stored row (IR4) - so it is
    ///   never silently stale, and unchanged rows do not re-hit the probabilistic adapter.
    ///
    /// Same log -> same rows on every node/call (P16). Entity `properties` (not modeled by any
    /// assertion yet) are carried forward from the stored row rather than reset.
    ///
    /// The rows are ready to store: folded from the log `cx` reads, each carrying the stored row's
    /// properties and a vector for its text. Nothing is written, so a writer commits them with its
    /// row and a reproject with the rest of its projection, in one transaction either way
    /// (crash-recovery.md Section 3a).
    pub(crate) fn materialize_entities(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        cx: &ReadCtx,
        prepared: &HashMap<String, Vec<f32>>,
    ) -> Result<Vec<Entity>, StoreError> {
        let mut out = Vec::new();
        for mut entity in self.fold_entities(ws, only, cx)? {
            let text = entity_text(&entity);
            if let Some(existing) = self.store.get_entity(&entity.id)? {
                // Keep the stored embedding ONLY if its source text is unchanged (IR4: never stale).
                if existing.embedding.is_some() && entity_text(&existing) == text {
                    entity.embedding = existing.embedding;
                }
                entity.properties = existing.properties;
            }
            if entity.embedding.is_none() {
                // A vector computed before the write lock for this exact text (see `reproject`);
                // otherwise embedded here.
                if let Some(vec) = prepared.get(&text) {
                    entity.embedding = Some(vec.clone());
                } else if let Some(embedder) = &self.embedder {
                    match embedder.embed_one(&text) {
                        Ok(vec) => entity.embedding = Some(vec),
                        Err(e) => tracing::warn!(
                            entity_id = %entity.id, name = %entity.canonical_name, error = %e,
                            "entity embedding failed - stored without name-meaning recall (degrade)"
                        ),
                    }
                }
            }
            out.push(entity);
        }
        Ok(out)
    }

    /// The entity rows the log folds to (see [`Engine::materialize_entities`]), before anything is
    /// read from the stored rows: no embedding, no carried properties.
    fn fold_entities(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        cx: &ReadCtx,
    ) -> Result<Vec<Entity>, StoreError> {
        let gates = self.gate_grants(Some(ws), cx)?;
        let mut obss = self.observations_over(Some(ws), cx.overlay.as_ref())?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });

        let mut name_cands: HashMap<String, Vec<BeliefCandidate>> = HashMap::new();
        let mut kind_cands: HashMap<String, Vec<BeliefCandidate>> = HashMap::new();
        let mut prov: HashMap<String, Vec<Provenance>> = HashMap::new();
        let mut descr: HashMap<String, String> = HashMap::new(); // HLC-latest (ascending replay -> last wins)
        let want = |id: &str| only.is_none_or(|s| s.contains(id));

        for obs in &obss {
            let eff = effective_tier(obs, &gates);
            let hlc = ordering_hlc(obs);
            // Entity ids this observation touches (deduped), so each observation's attestation set is
            // credited to an entity's provenance exactly once - an entity named as both an assertion
            // and a relation endpoint in the same observation is one supporting observation, not two.
            let mut touched_here: BTreeSet<String> = BTreeSet::new();
            let mut touch =
                |id: String, spelling: &str, kind: Option<&str>, description: Option<&str>| {
                    if !want(&id) {
                        return;
                    }
                    name_cands.entry(id.clone()).or_default().push(BeliefCandidate {
                        value: spelling.trim().to_string(),
                        tier: eff,
                        hlc: hlc.clone(),
                        observation: obs.id.clone(),
                    });
                    if let Some(k) = kind {
                        kind_cands.entry(id.clone()).or_default().push(BeliefCandidate {
                            value: k.to_string(),
                            tier: eff,
                            hlc: hlc.clone(),
                            observation: obs.id.clone(),
                        });
                    }
                    // Ascending replay: the highest HLC wins among the observations that supplied
                    // one. A blank description is an omission, and an omission never erases (P8).
                    if let Some(d) = description.filter(|d| !d.trim().is_empty()) {
                        descr.insert(id.clone(), d.to_string());
                    }
                    touched_here.insert(id);
                };
            for ea in &obs.assertions.entities {
                touch(
                    Entity::make_id(ws, &ea.name),
                    &ea.name,
                    ea.kind.as_deref(),
                    ea.description.as_deref(),
                );
            }
            for ra in &obs.assertions.relations {
                touch(Entity::make_id(ws, &ra.from), &ra.from, None, None);
                touch(Entity::make_id(ws, &ra.to), &ra.to, None, None);
            }
            // Credit ALL of this observation's attestations to each entity it supports (Principle 3:
            // the entity provenance reflects every attestation in the log, not just a representative -
            // so re-projecting recovers the same count the incremental write produced, IR3).
            for id in touched_here {
                prov.entry(id).or_default().extend(obs.provenance.iter().cloned());
            }
        }

        let mut out = Vec::with_capacity(name_cands.len());
        for (id, ncs) in &name_cands {
            let canonical =
                self.policy.choose(ncs).map(|c| ncs[c.index].value.clone()).unwrap_or_default();
            let aliases = alias_set(ncs, &canonical);
            let kind = kind_cands
                .get(id)
                .and_then(|ks| self.policy.choose(ks).map(|c| ks[c.index].value.clone()))
                .unwrap_or_else(|| "Concept".to_string());
            out.push(Entity {
                id: id.clone(),
                kind,
                canonical_name: canonical,
                aliases,
                description: descr.get(id).cloned(),
                properties: serde_json::Value::Null,
                provenance: prov.get(id).cloned().unwrap_or_default(),
                embedding: None,
            });
        }
        Ok(out)
    }

    /// Vectors for the entity texts a re-materialization of `ws` will need - every row whose stored
    /// vector is missing or was made for other text - computed with no lock held, as the backfill
    /// does (semantic-retrieval.md SR7), so a write does not wait out the embedder. The projection
    /// then takes a vector from here only for the exact text it was made for (IR4). Empty without an
    /// embedder; a batch that fails is left to be embedded under the lock, as before.
    pub(crate) fn vectors_for_reproject(&self, ws: &str) -> HashMap<String, Vec<f32>> {
        let mut prepared = HashMap::new();
        let Some(embedder) = &self.embedder else {
            return prepared;
        };
        let Ok(rows) = self.fold_entities(ws, None, &ReadCtx::default()) else {
            return prepared;
        };
        let mut texts: Vec<String> = Vec::new();
        for row in &rows {
            let text = entity_text(row);
            let stored = self.store.get_entity(&row.id).ok().flatten();
            let fresh = stored.is_some_and(|e| e.embedding.is_some() && entity_text(&e) == text);
            if !fresh && !prepared.contains_key(&text) && !texts.contains(&text) {
                texts.push(text);
            }
        }
        for chunk in texts.chunks(32) {
            let refs: Vec<&str> = chunk.iter().map(String::as_str).collect();
            if let Ok(vectors) = embedder.embed(&refs) {
                prepared.extend(chunk.iter().cloned().zip(vectors));
            }
        }
        prepared
    }

    /// Projects relations from the log - the edge half of what [`Engine::materialize_entities`] does for
    /// nodes, and shared by `observe` and `reproject` for the same reason (IR3): if the incremental
    /// write and a fresh replay run different code, they are two answers to one question and the
    /// projection stops being a function of the log.
    ///
    /// They WERE different. `observe` stamped each edge with the attestation of the call that wrote
    /// it, while `reproject` used [`authoring_attestation`] over the HLC-latest observation asserting
    /// that edge. The two agree on a fresh single-attestation observation and part company as soon as
    /// one absorbs a second attestation, or as soon as two observations assert the same edge - so a
    /// reproject could move an edge's tier and confidence with no change in the log. Commit 3a04ece
    /// already collected this rule into one helper for the proposal fold, the gate fold and
    /// reprojection; this is the fourth site it missed.
    ///
    /// `only` bounds the work to the edges a single observe touched. It is a filter on which rows are
    /// WRITTEN, never on which are read: the winner for an edge is still chosen across every
    /// observation asserting it, so narrowing the write set cannot change what gets written.
    ///
    /// The rows are folded from the log as it reads with `overlay` in it. Nothing is written, so a
    /// writer commits them with its row and a reproject with the rest of its projection
    /// (crash-recovery.md Section 3a).
    pub(crate) fn relation_rows(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        overlay: Option<&Observation>,
    ) -> Result<Vec<Relation>, StoreError> {
        let mut obss = self.observations_over(Some(ws), overlay)?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });
        let mut rels: BTreeMap<String, Relation> = BTreeMap::new();
        // Every attestation of every observation asserting the edge, credited once per observation
        // however many times it names the edge - an entity's provenance is counted the same way.
        let mut support: BTreeMap<String, Vec<Provenance>> = BTreeMap::new();
        for obs in &obss {
            let Some(prov) = authoring_attestation(obs).cloned() else {
                continue;
            };
            let mut credited: BTreeSet<String> = BTreeSet::new();
            for ra in &obs.assertions.relations {
                let from = Entity::make_id(ws, &ra.from);
                let to = Entity::make_id(ws, &ra.to);
                let kind = normalize_relation_kind(&ra.kind);
                let id = Relation::make_id(&from, &kind, &to);
                if only.is_some_and(|s| !s.contains(&id)) {
                    continue;
                }
                if credited.insert(id.clone()) {
                    support.entry(id.clone()).or_default().extend(obs.provenance.iter().cloned());
                }
                // Ascending replay, upsert by id: the valid interval is last-write, which is the
                // store's own semantics given a deterministic order. The description is the
                // highest-HLC one supplied, as for an entity: an omission, or a blank, never erases.
                let description = match ra.description.as_deref() {
                    Some(d) if !d.trim().is_empty() => Some(d.to_string()),
                    _ => rels.get(&id).and_then(|r| r.description.clone()),
                };
                rels.insert(
                    id.clone(),
                    Relation {
                        id,
                        from,
                        to,
                        kind,
                        description,
                        provenance: prov.clone(),
                        attestations: Vec::new(),
                        // The client's valid interval is projected as-is (Principle 4 capture);
                        // derivation such as auto-closing valid_to on refutation is M3c.
                        valid_from: ra.valid_from,
                        valid_to: ra.valid_to,
                    },
                );
            }
        }
        Ok(rels
            .into_iter()
            .map(|(id, mut r)| {
                r.attestations = support.remove(&id).unwrap_or_default();
                r
            })
            .collect())
    }
}

/// The text to embed for an entity: canonical name + aliases (if any). Opens semantic retrieval by the meaning of the name.
/// Since aliases hold notation variants, embedding them together widens the reach to other notations of the same target.
pub(crate) fn entity_text(entity: &Entity) -> String {
    if entity.aliases.is_empty() {
        entity.canonical_name.clone()
    } else {
        format!("{} {}", entity.canonical_name, entity.aliases.join(" "))
    }
}
