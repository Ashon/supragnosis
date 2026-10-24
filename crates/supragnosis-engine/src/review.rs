//! Casting verdicts with `review_proposal`, and the log enumeration every fold reads.

use super::*;

impl Engine {
    /// Cast a verdict / comment / withdrawal on a proposal (Principle 23). Records the event as an
    /// observation (I1); the fold derives the resulting state. `decision` is merge|reject|comment|withdraw.
    /// `surface` is decided by the caller crate per call-site (resolution.md Section 6) - it is
    /// stamped into the verdict's provenance and caps what a merged promotion can grant.
    pub fn review_proposal(
        &self,
        workspace: Option<String>,
        proposal: String,
        decision: String,
        note: Option<String>,
        on_behalf_of: Option<String>,
        surface: VerdictSurface,
    ) -> Result<String, ObserveError> {
        let cx = &ReadCtx::default();
        let event = match decision.as_str() {
            "merge" | "reject" => ProposalEventKind::Verdict,
            "comment" => ProposalEventKind::Comment,
            "withdraw" => ProposalEventKind::Withdrawn,
            other => {
                return Err(ObserveError::Invalid(format!(
                    "unknown decision '{other}'. use merge | reject | comment | withdraw"
                )))
            }
        };
        if proposal.trim().is_empty() {
            return Err(ObserveError::Invalid("proposal id is required".into()));
        }
        let workspace = workspace.unwrap_or_else(|| self.default_workspace.clone());
        // A verdict is filed in its proposal's workspace, the only place it counts (I2): one filed
        // anywhere else would decide nothing and stay in the log for good. So a verdict that names no
        // proposal of this workspace is refused here, with nothing said about any other workspace -
        // on the remote surface this one is the principal's (remote-viewer.md Section 3.4). A
        // replicated verdict never passes through here, so arrival order is still the fold's to
        // tolerate.
        let views = self.fold_proposals(Some(&workspace), cx)?;
        let Some(view) = views.get(&proposal) else {
            return Err(ObserveError::Invalid(format!(
                "no proposal {proposal} in workspace '{workspace}' - a verdict is filed in its \
                 proposal's own workspace, which list_proposals names"
            )));
        };
        // Fail fast on a local merge attempt that the fold would refuse anyway. This is a courtesy,
        // not the gate: the fold is what enforces (a replicated verdict never reaches this path), so
        // this exists to say WHY rather than to let the caller discover a "blocked" state later.
        if decision == "merge" {
            // I17: a recall verdict is a human's direct act. The fold would not count a recall
            // merge cast from the agent surface (it reads the marker off the log), so refuse here
            // and say where to decide - the same courtesy as the blocking checks below.
            if view.kind == "recall" && surface == VerdictSurface::Agent {
                return Err(ObserveError::Invalid(
                    "a recall verdict is a human's direct act (Principle 23, I17): this surface \
                     cannot merge one, and the fold would not count it. Until recall has its \
                     commit effect no surface merges one; a `comment` is accepted here, and the \
                     person can reject it in the Review panel of the Supragnosis app or viewer"
                        .into(),
                ));
            }
            // 7.1 (f): the fold would read this merge as a comment, so say why instead of taking it.
            if !has_commit_effect(&view.kind) {
                return Err(ObserveError::Invalid(format!(
                    "{} has no commit effect yet, so it cannot be merged until it has one - a merge \
                     without a diff is a merge without review (Principle 23). A comment or a \
                     withdrawal is accepted, and the person can reject it in the Review panel",
                    view.kind
                )));
            }
            let asserted = self.asserted_entity_ids(Some(&workspace), cx)?;
            let decided: HashSet<String> = views
                .values()
                .filter(|p| p.kind == "entity_merge" && carries_merge_verdict(p))
                .map(|p| p.id.clone())
                .collect();
            let failures = self.blocking_failures(view, &asserted, &decided, cx)?;
            if !failures.is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "blocking checks fail, so this merge would not reach canon: {}. Fix the proposal or reject it (proposal-workflow.md Section 6)",
                    failures.join("; ")
                )));
            }
        }
        // The surface marker rides source_ref, engine-stamped - the review surfaces accept no
        // source_ref of their own, so a client cannot mint the console marker (resolution.md R8).
        let prov =
            self.provenance(&workspace, Some(surface.marker().to_string()), None, on_behalf_of);
        let payload = serde_json::json!({ "decision": decision, "note": note }).to_string();
        let content = format!("proposal({decision}) {proposal}");
        let assertions = Assertions {
            proposal_events: vec![ProposalEventAssertion { proposal, event, payload }],
            ..Default::default()
        };
        let obs = Observation::with_assertions(content, prov, assertions);
        let id = obs.id.clone();
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));
        // Proposal state is a read-time fold over the log, but a merged gate verdict changes the
        // tier the entity fold weighs its targets' assertions by, and with it the spelling and kind
        // their rows hold. Those rows commit with the verdict, folded from the log as it will read
        // with the verdict in it (crash-recovery.md Section 3a); a sync apply reprojects instead.
        let entities = if decision == "merge" && GATE_KINDS.contains(&view.kind.as_str()) {
            let mut touched = HashSet::new();
            for target in &view.targets {
                if let Some(o) = self.store.get_observation(target)? {
                    touched.extend(asserted_ids(&o));
                }
            }
            let row = match self.store.get_observation(&obs.id)? {
                Some(mut stored) => {
                    stored.absorb(obs.clone());
                    stored
                }
                None => obs.clone(),
            };
            let cx = &ReadCtx::over(row);
            self.materialize_entities(&workspace, Some(&touched), cx, &HashMap::new())?
        } else {
            Vec::new()
        };
        self.store.commit_projected(obs, entities, Vec::new())?;
        self.note_app(app_key.as_deref());
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(id)
    }

    /// The workspace's observations for this read, fetched on first use and reused after.
    /// Reusing the rows requires the scope to match. On a mismatch this reads through rather than
    /// answering from the wrong scope - correctness first, and the caller merely loses the reuse.
    pub(crate) fn log<'c>(
        &self,
        workspace: Option<&str>,
        cx: &'c ReadCtx,
    ) -> Result<std::borrow::Cow<'c, [Observation]>, StoreError> {
        let epoch = self.log_epoch.load(std::sync::atomic::Ordering::SeqCst);
        if let Some((at, scope, rows)) = cx.observations.get() {
            if *at == epoch && scope.as_deref() == workspace {
                return Ok(std::borrow::Cow::Borrowed(rows));
            }
            // Either the log grew under this context or the scope changed. Reuse is an optimization
            // and this is the case where it does not apply, so read through: at worst that costs
            // what every one of these folds used to cost unconditionally.
            //
            // Deliberately not a debug_assert. Asserting "this must not happen" beside code that
            // handles it correctly would mean the handling is unreachable in the builds where it is
            // tested, and the panic would be the only thing anyone learned about the case. The
            // handling is the answer; the cost is one extra read, and the guard for it is
            // `a_read_context_reuses_rows_only_while_that_changes_nothing`.
            return Ok(std::borrow::Cow::Owned(
                self.observations_over(workspace, cx.overlay.as_ref())?,
            ));
        }
        let loaded = self.observations_over(workspace, cx.overlay.as_ref())?;
        let (_, _, rows) =
            cx.observations.get_or_init(|| (epoch, workspace.map(str::to_string), loaded));
        Ok(std::borrow::Cow::Borrowed(rows))
    }

    /// The workspace's log with **re-keyed predecessors folded out** - the enumeration every fold
    /// and every projection reads.
    ///
    /// `migrate` re-creates a pre-formula row under the current content address, and because the log
    /// is append-only (Principle 3) the original stays. The two rows then hold the same content and
    /// the same assertions, so a raw enumeration reports one act as two: a proposal gains a twin no
    /// verdict can ever reference (a proposal's id IS its opening observation's id, so the copy is
    /// permanently open), and an entity's supporting-attestation count doubles - which the
    /// corroboration rules read as extra independent support (Principles 2/18).
    ///
    /// Dedup by content address (Principle 14) is what normally prevents this; a re-keying is
    /// precisely the case where one content wears two ids, so the same rule has to be applied by
    /// hand. A predecessor is recognised structurally rather than by re-hashing every row: the
    /// successor names it in `derived_from` AND carries byte-identical content and assertions AND
    /// lives in the same workspace. Genuine derived knowledge cannot collide with that test -
    /// identical content and assertions under the current formula IS the same content address, so
    /// it would be one row, not two.
    ///
    /// The same-workspace condition is what separates the two re-keyings: a `migrate` successor
    /// shares its predecessor's workspace (the id FORMULA changed), so the pair is one act and the
    /// door folds it to one row; a `rekey_workspace` successor does not (the WORKSPACE changed), so
    /// both rows stay live - each workspace keeps its own support, and the unscoped view is the
    /// union of the scoped ones. Without the condition the unscoped fold dropped the source row,
    /// showing the source workspace's entities without the attestations its own scoped view reports
    /// (a scoped and an unscoped read of one log must not disagree - Principle 16).
    ///
    /// Nothing is deleted (Principle 3). The predecessor stays in the store and stays
    /// dereferenceable by its id; the successor's `derived_from` is the record of the re-keying.
    pub(crate) fn observations(
        &self,
        workspace: Option<&str>,
    ) -> Result<Vec<Observation>, StoreError> {
        self.observations_over(workspace, None)
    }

    /// [`Engine::observations`] as it will read once `overlay` is in the log: the row a writer is
    /// about to commit stands in for the stored row of its id, or joins the log when there is none
    /// (crash-recovery.md Section 3a). The overlay is the row as the store will hold it - already
    /// absorbed onto the stored one - so the fold reads exactly the log the commit leaves.
    pub(crate) fn observations_over(
        &self,
        workspace: Option<&str>,
        overlay: Option<&Observation>,
    ) -> Result<Vec<Observation>, StoreError> {
        let mut rows = self.store.all_observations(workspace)?;
        if let Some(row) = overlay.filter(|o| workspace.is_none_or(|ws| o.workspace() == ws)) {
            match rows.iter_mut().find(|o| o.id == row.id) {
                Some(stored) => *stored = row.clone(),
                None => rows.push(row.clone()),
            }
        }
        let by_id: HashMap<&str, &Observation> = rows.iter().map(|o| (o.id.as_str(), o)).collect();
        let superseded: HashSet<String> = rows
            .iter()
            .flat_map(|successor| {
                successor.derived_from.iter().filter_map(|parent| {
                    let predecessor = by_id.get(parent.as_str())?;
                    (predecessor.content == successor.content
                        && predecessor.assertions == successor.assertions
                        && predecessor.workspace() == successor.workspace())
                    .then(|| parent.clone())
                })
            })
            .collect();
        if superseded.is_empty() {
            return Ok(rows);
        }
        Ok(rows.into_iter().filter(|o| !superseded.contains(&o.id)).collect())
    }
}
