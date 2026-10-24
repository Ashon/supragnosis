//! Store maintenance: re-keying, legacy-id migration, owed-projection repayment, the embedding
//! backfill and reprojection.

use super::*;

/// `obs` re-created under `workspace` for a re-key or a migration: every attestation carried, the
/// sync stamp dropped (it signs the old content id and cannot follow), and the lineage naming
/// `obs`. The tier is clamped BEFORE the stamp drops: `evaluated_tier` trusts a stamp-less claim at
/// face value, so a synced claim carried verbatim would evaluate above HostSigned afterwards - a
/// tier promotion by an operator act (P18: the tier is the receiver's evaluation, and re-keying
/// must not raise it). `granted` lowers it further when a merged demotion applied to `obs`, since
/// the gate events do not follow the row. `None` when `obs` has no attestation (P2 forbids it).
/// One attestation as [`recreated`] re-creates it: in the new workspace, at the tier it evaluates to
/// (clamped by a demotion that applied to it), unstamped.
fn recreated_attestation(
    p: &Provenance,
    workspace: &str,
    granted: Option<TrustTier>,
) -> Provenance {
    let mut p = p.clone();
    p.workspace = workspace.to_string();
    p.trust_tier = evaluated_tier(&p);
    if let Some(g) = granted {
        p.trust_tier = p.trust_tier.min(g);
    }
    p.sync = None;
    p
}

fn recreated(
    obs: &Observation,
    workspace: &str,
    granted: Option<TrustTier>,
) -> Option<Observation> {
    let mut provs: Vec<Provenance> = obs
        .provenance
        .iter()
        .map(|p| recreated_attestation(p, workspace, granted))
        .collect();
    if provs.is_empty() {
        return None;
    }
    let first = provs.remove(0);
    let mut fresh =
        Observation::with_assertions(obs.content.clone(), first, obs.assertions.clone());
    for p in provs {
        let mut copy = Observation::with_assertions(obs.content.clone(), p, obs.assertions.clone());
        copy.derived_from = Vec::new();
        fresh.absorb(copy); // union semantics - the attestation set is preserved whole
    }
    fresh.derived_from = obs.derived_from.clone();
    fresh.derived_from.push(obs.id.clone());
    fresh.derived_from.sort();
    fresh.derived_from.dedup();
    Some(fresh)
}

/// Whether a merged promotion raised `obs` above its own evaluation - a grant [`recreated`] does
/// not carry, so the operator act that re-created the row has to say it was left behind.
fn promotion_left_behind(obs: &Observation, granted: Option<TrustTier>) -> bool {
    let own = obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default();
    granted.is_some_and(|g| g > own)
}

impl Engine {
    /// Carries the apps recorded for `obs`'s attestations to the ones [`recreated`] made of them under
    /// `fresh_id` (design-system.md Section 29.8): re-keying a workspace and migrating legacy ids
    /// keep which app wrote what. A row already there keeps its name, so a re-run is idempotent.
    fn carry_apps(
        &self,
        obs: &Observation,
        fresh_id: &str,
        workspace: &str,
        granted: Option<TrustTier>,
    ) -> Result<(), StoreError> {
        let pairs: Vec<(String, String)> = obs
            .provenance
            .iter()
            .map(|p| {
                (
                    attestation_key(&obs.id, p),
                    attestation_key(fresh_id, &recreated_attestation(p, workspace, granted)),
                )
            })
            .collect();
        let found = self
            .store
            .attestation_apps(&pairs.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>())?;
        for (old, new) in pairs {
            if let Some(app) = found.get(&old) {
                self.store.record_attestation_app(&new, app)?;
            }
        }
        Ok(())
    }

    /// Re-create a workspace's knowledge under another workspace name, provenance intact.
    ///
    /// The workspace is part of the content address, so this is not a move and cannot be: a
    /// re-keyed observation is a NEW observation, and the original stays where it is (Principle 3).
    /// What makes it a re-key rather than a re-ingest is that **every attestation is copied
    /// verbatim** - acting host, `on_behalf_of`, `observed_at`, confidence. Pushing the same text
    /// back through `observe` would restamp all of it with this engine's clock and host, which
    /// fabricates the two things the log exists to preserve (Principles 2 and 4) and collapses the
    /// HLC order that last-write-wins fields are decided by. The one field dropped is the sync
    /// stamp: it is bound to the old content id and signs it, so it cannot follow - and because
    /// `evaluated_tier` trusts a stamp-less claim at face value, the claimed tier is carried as its
    /// **pre-strip evaluation** rather than verbatim (a synced claim stays capped at HostSigned;
    /// P18: dropping the stamp must not raise what the claim evaluates to).
    ///
    /// Shaped exactly like [`Engine::migrate_legacy_ids`], which re-keys across a change of id
    /// FORMULA; this
    /// re-keys across a change of WORKSPACE. Lineage records the origin either way, but the
    /// live-set door treats the two differently on purpose: a migrated pair shares one workspace
    /// and folds to one row, while a re-keyed pair spans two and BOTH rows stay live - each
    /// workspace keeps its own support, and the unscoped view is the union of the scoped ones
    /// (see `Engine::observations`).
    ///
    /// Proposal events are left behind deliberately - see [`RekeyReport::skipped_proposal_events`].
    /// `dry_run` reports what would move and writes nothing.
    pub fn rekey_workspace(
        &self,
        from: &str,
        to: &str,
        dry_run: bool,
    ) -> Result<RekeyReport, StoreError> {
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() || to.is_empty() {
            return Err(StoreError::Backend("both workspaces must be named".into()));
        }
        if from == to {
            return Err(StoreError::Backend(format!(
                "source and target are the same workspace ('{from}') - nothing to re-key"
            )));
        }
        let _guard = self.write_guard.lock().unwrap_or_else(|e| e.into_inner());
        // The gate events stay behind with the rest of the proposal rows, so a merged demotion
        // would stop applying in the target, and a row quarantined at Unverified would evaluate
        // at its claimed tier again. A grant below the claim is carried as the clamp; one above
        // it is not, since a promotion is a person's act on this row and the target never saw it.
        let gates = self.gate_grants(Some(from), &ReadCtx::default())?;
        let mut report = RekeyReport::default();
        for obs in self.observations(Some(from))? {
            if !obs.assertions.proposal_events.is_empty() {
                report.skipped_proposal_events += 1;
                continue;
            }
            let granted = gates.get(&obs.id).copied();
            let Some(fresh) = recreated(&obs, to, granted) else {
                continue; // unreachable by construction (P2), but never panic on a stored row
            };
            // Idempotent: a second run finds the target already recording this origin and stops -
            // having carried the apps, which a run stopped between the two writes may have missed.
            if let Some(existing) = self.store.get_observation(&fresh.id)? {
                if existing.derived_from.contains(&obs.id) {
                    if !dry_run {
                        self.carry_apps(&obs, &fresh.id, to, granted)?;
                    }
                    report.already += 1;
                    continue;
                }
            }
            if promotion_left_behind(&obs, granted) {
                report.promotions_left_behind += 1;
            }
            if !dry_run {
                let fresh_id = fresh.id.clone();
                self.store.add_observation(fresh)?;
                self.carry_apps(&obs, &fresh_id, to, granted)?;
            }
            report.moved += 1;
        }
        if !dry_run && report.moved > 0 {
            self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(report)
    }

    /// One-shot legacy-id migration (0.x format evolution): re-creates every observation in
    /// `workspace` whose stored id predates the current content-address formula under the CURRENT
    /// id, so it can sync again. The legacy row stays as local history (Principle 3) and never
    /// exports (the wire guard in `attestations_since` and `backfill`); the successor names it in
    /// `derived_from`, and the live set folds the pair to one row (see `Engine::observations`).
    ///
    /// The twin of [`Engine::rekey_workspace`], which re-keys across a change of WORKSPACE: every
    /// attestation is carried, the sync stamp drops because it signs the old id, and the tier is
    /// clamped to the pre-strip evaluation and to a merged demotion's grant. Proposal rows are
    /// left as they are - see [`MigrateReport::skipped_proposal_events`]. Idempotent.
    pub fn migrate_legacy_ids(&self, workspace: &str) -> Result<MigrateReport, StoreError> {
        let _guard = self.write_guard.lock().unwrap_or_else(|e| e.into_inner());
        // A gate proposal names its target by the legacy id, which the successor does not carry,
        // so a demotion stops applying unless it travels as the clamp - as in a re-key.
        let gates = self.gate_grants(Some(workspace), &ReadCtx::default())?;
        let mut report = MigrateReport::default();
        for obs in self.store.all_observations(Some(workspace))? {
            if observation_content_id(workspace, &obs.content, &obs.assertions) == obs.id {
                continue;
            }
            if !obs.assertions.proposal_events.is_empty() {
                report.skipped_proposal_events += 1;
                continue;
            }
            let granted = gates.get(&obs.id).copied();
            let Some(fresh) = recreated(&obs, workspace, granted) else {
                continue; // unreachable in practice (P2: at least one attestation), never panic
            };
            if let Some(existing) = self.store.get_observation(&fresh.id)? {
                if existing.derived_from.contains(&obs.id) {
                    self.carry_apps(&obs, &fresh.id, workspace, granted)?;
                    continue; // already migrated
                }
            }
            if promotion_left_behind(&obs, granted) {
                report.promotions_left_behind += 1;
            }
            let fresh_id = fresh.id.clone();
            self.store.add_observation(fresh)?;
            self.carry_apps(&obs, &fresh_id, workspace, granted)?;
            report.migrated += 1;
        }
        if report.migrated > 0 {
            self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(report)
    }

    /// Repays the owed-projection ledger (crash-recovery.md K3): re-projects every workspace it
    /// names. A writer process runs this right after opening the store and before it serves, so no
    /// read is answered from a graph known to be behind its log. Recovery is a reproject, so it
    /// produces what the interrupted writes would have (IR3) and running it twice changes nothing.
    ///
    /// `Ok(None)` when nothing was owed - the normal case, one ledger read.
    pub fn repay_owed(&self) -> Result<Option<Recovery>, StoreError> {
        let owed = self.store.owed_projections()?;
        if owed.is_empty() {
            return Ok(None);
        }
        let workspaces: std::collections::BTreeSet<String> =
            owed.iter().map(|(_, w)| w.clone()).collect();
        for ws in &workspaces {
            self.reproject(Some(ws))?;
        }
        let recovery = Recovery {
            at: self.clock.now_millis(),
            workspaces: workspaces.into_iter().collect(),
            observations: owed.len(),
        };
        *self.last_recovery.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(recovery.clone());
        Ok(Some(recovery))
    }

    /// The last repayment this engine ran (K5), for the operator's surfaces.
    pub fn last_recovery(&self) -> Option<Recovery> {
        self.last_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Semantic retrieval for the operator's surfaces (semantic-retrieval.md SR3): `None` when no
    /// embedder is configured, otherwise the model and whether it can answer now.
    pub fn semantic(&self) -> Option<SemanticReport> {
        let e = self.embedder.as_ref()?;
        let (state, note) = match e.readiness() {
            supragnosis_core::Readiness::Ready => ("on", None),
            supragnosis_core::Readiness::Degraded(note) => ("off", Some(note)),
        };
        let backfill =
            self.backfill.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        Some(SemanticReport { model: e.id(), dims: e.dimensions(), state, note, backfill })
    }

    /// Embeds, once, every observation and entity in the store that carries no vector - what was
    /// written before the model, or while it was missing (semantic-retrieval.md Section 12). Each
    /// batch is embedded without the write guard and written under it (SR7). An observation's
    /// vector is attached and nothing else is written (SR6); an entity's is stored only if the
    /// entity still has none and its text is the text that was embedded (IR4). An embed or a write
    /// that fails stops the run with the reason, recorded like the progress. `None` when no
    /// embedder is configured.
    pub fn backfill_embeddings(&self, batch: usize) -> Option<BackfillProgress> {
        let embedder = self.embedder.clone()?;
        let batch = batch.max(1);
        let set = |p: &BackfillProgress| {
            *self.backfill.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(p.clone());
        };
        let stop = |mut p: BackfillProgress, why: String| {
            tracing::warn!(
                embedded = p.embedded,
                found = p.found,
                "the backfill stopped - the rest stays keyword only until the next start: {why}"
            );
            p.state = "stopped";
            p.note = Some(why);
            set(&p);
            p
        };
        let mut progress = BackfillProgress { state: "running", found: 0, embedded: 0, note: None };
        let (observations, entities) =
            match (self.observations(None), self.store.all_entities(None)) {
                (Ok(o), Ok(e)) => (o, e),
                (Err(e), _) | (_, Err(e)) => {
                    return Some(stop(progress, format!("the store could not be read: {e}")))
                }
            };
        let observations: Vec<(String, String)> = observations
            .into_iter()
            .filter(|o| o.embedding.is_none())
            .map(|o| (o.id, o.content))
            .collect();
        let entities: Vec<(String, String)> = entities
            .into_iter()
            .filter(|e| e.embedding.is_none())
            .map(|e| {
                let text = entity_text(&e);
                (e.id, text)
            })
            .collect();
        progress.found = observations.len() + entities.len();
        if progress.found == 0 {
            progress.state = "done";
            set(&progress);
            return Some(progress);
        }
        tracing::info!(
            observations = observations.len(),
            entities = entities.len(),
            "embedding what was written before the model"
        );
        set(&progress);
        let embed = |chunk: &[(String, String)]| {
            let texts: Vec<&str> = chunk.iter().map(|(_, t)| t.as_str()).collect();
            embedder.embed(&texts).and_then(|v| {
                if v.len() == chunk.len() {
                    Ok(v)
                } else {
                    Err(supragnosis_core::EmbedError::Provider(format!(
                        "{} texts gave {} vectors",
                        chunk.len(),
                        v.len()
                    )))
                }
            })
        };
        for chunk in observations.chunks(batch) {
            let vectors = match embed(chunk) {
                Ok(v) => v,
                Err(e) => return Some(stop(progress, format!("an embed failed: {e}"))),
            };
            let pairs: Vec<(String, Vec<f32>)> =
                chunk.iter().map(|(id, _)| id.clone()).zip(vectors).collect();
            let written = {
                let _write =
                    self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                self.store.attach_observation_embeddings(&pairs)
            };
            match written {
                Ok(n) => progress.embedded += n,
                Err(e) => return Some(stop(progress, format!("a write failed: {e}"))),
            }
            set(&progress);
        }
        for chunk in entities.chunks(batch) {
            let vectors = match embed(chunk) {
                Ok(v) => v,
                Err(e) => return Some(stop(progress, format!("an embed failed: {e}"))),
            };
            let written: Result<usize, StoreError> = {
                let _write =
                    self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut n = 0;
                let mut result = Ok(());
                for ((id, text), vec) in chunk.iter().zip(vectors) {
                    // Re-read under the guard: a projection that rewrote the entity since the scan
                    // wins, and a vector for text it no longer has is never stored (IR4, SR7).
                    match self.store.get_entity(id) {
                        Ok(Some(mut e)) if e.embedding.is_none() && entity_text(&e) == *text => {
                            e.embedding = Some(vec);
                            if let Err(err) = self.store.put_entity(e) {
                                result = Err(err);
                                break;
                            }
                            n += 1;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            result = Err(err);
                            break;
                        }
                    }
                }
                result.map(|()| n)
            };
            match written {
                Ok(n) => progress.embedded += n,
                Err(e) => return Some(stop(progress, format!("a write failed: {e}"))),
            }
            set(&progress);
        }
        progress.state = "done";
        tracing::info!(
            embedded = progress.embedded,
            found = progress.found,
            "the backfill finished - what was written before the model is found by meaning too"
        );
        set(&progress);
        Some(progress)
    }

    /// The serving process's backfill worker (semantic-retrieval.md Section 12): waits until the
    /// embedder can answer - the model's files are in the cache - checking every `poll`, then runs
    /// [`Engine::backfill_embeddings`] once. Returns at once, with `None`, when no embedder is
    /// configured.
    pub fn backfill_when_ready(
        &self,
        poll: std::time::Duration,
        batch: usize,
    ) -> Option<BackfillProgress> {
        let embedder = self.embedder.as_ref()?;
        while embedder.readiness() != supragnosis_core::Readiness::Ready {
            std::thread::sleep(poll);
        }
        self.backfill_embeddings(batch)
    }

    /// How many log rows still owe their projection right now. The engine's own writers never leave
    /// one (crash-recovery.md Section 3a), so non-zero on a running node means an append projected
    /// after the fact - a sync apply, a stamping backfill, a re-key - has not been re-materialized,
    /// or its re-materialization failed: the fact is in the log, and the next open repays it.
    pub fn owed_count(&self) -> Result<usize, StoreError> {
        Ok(self.store.owed_projections()?.len())
    }

    /// Rebuilds the materialized entity/relation projection of a workspace from the observation log,
    /// replayed in (ordering HLC, id) order - the deterministic re-materialization step after sync
    /// apply (F1/F3; architecture.md: "orders by HLC, then re-materializes"). Same log -> same replay
    /// order -> same materialization on every node (P16/F5), unlike observe-time upserts whose
    /// last-write-wins fields depend on local arrival order. Builds fresh states and upserts them
    /// (idempotent: a second run writes identical rows); rows with no support in the log are left in
    /// place - removal is a curation concern, not reprojection's (Principle 3).
    pub fn reproject(&self, workspace: Option<&str>) -> Result<ReprojectReport, StoreError> {
        // A re-materialization is a write section like observe's: it reads the log and then upserts
        // every row of the workspace, so an observe landing between the two would have its
        // projection overwritten by a snapshot taken before it. The engine's write guard serializes
        // the two (architecture.md Section 14); reads stay concurrent. The vectors it needs are
        // computed first, with no lock held, so a write waits for the projection and not the model.
        let ws = workspace.unwrap_or(&self.default_workspace).to_string();
        let prepared = self.vectors_for_reproject(&ws);
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // K2: what this reproject repays is what the ledger held BEFORE it read the log. An append
        // landing while it runs - only a sync apply or a stamp, which append without the write
        // guard - may be missing from the snapshot it projects, so its entry stays for the next
        // reproject to clear. Read before the context below exists.
        let owed: Vec<String> = self
            .store
            .owed_projections()?
            .into_iter()
            .filter(|(_, w)| *w == ws)
            .map(|(id, _)| id)
            .collect();
        let cx = &ReadCtx::default();
        // Entities: the same resolution write path the incremental observe uses, over ALL entities
        // (only = None) - so a reproject and an incremental write agree row-for-row (IR3).
        let entities = self.materialize_entities(&ws, None, cx, &prepared)?;
        // The same fold the incremental write runs, over every edge rather than one observe's.
        let relations = self.relation_rows(&ws, None, None)?;
        let report = ReprojectReport {
            observations: self.observations(Some(&ws))?.len(),
            entities: entities.len(),
            relations: relations.len(),
        };
        // One write for the whole projection: one commit instead of one per row, and no reader sees
        // a projection half rewritten (crash-recovery.md Section 3a).
        self.store.put_projection(entities, relations)?;
        self.store.clear_owed(&owed)?;
        Ok(report)
    }

    /// The shared store port - the federation sync layer operates on the same log the engine
    /// projects from (M4 Phase 4 wiring; one process, one store, one clock).
    ///
    /// Deliberately narrowed to [`AssertionStore`] (Principle 1, third enforcement demand). The engine
    /// holds the full [`KnowledgeStore`] and is the only thing that does; what it hands out can append
    /// to the log and read the graph, but cannot write an entity or a relation row. Knowledge reaches
    /// the projection through `observe`/`apply` and the folds those run, or it does not reach it at all.
    pub fn store(&self) -> Arc<dyn AssertionStore> {
        self.store.clone()
    }
}
