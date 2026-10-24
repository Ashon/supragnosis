//! Gate previews and undo, and the blocking checks of a proposal.

use super::*;

impl Engine {
    /// What the console's settle would do, without casting it (design-system.md Section 29.2): the
    /// belief diff of `acts` applied in order after every merged gate event, at the console's grant -
    /// the surface `/api/resolve` casts from - and each act's blocking checks. Writes nothing. A
    /// "Confirm instead" is two acts (a demotion and a promotion), previewed together so the ask
    /// reads their one result (P23: no merge without a diff that says what it does).
    pub fn preview_gate(
        &self,
        workspace: Option<&str>,
        acts: &[GateAct],
    ) -> Result<GatePreview, StoreError> {
        let cx = &ReadCtx::default();
        let ceiling = verdict_grant_ceiling(Some(VERDICT_SURFACE_CONSOLE));
        let events: Vec<(&[String], TrustTier, bool)> = acts
            .iter()
            .filter(|a| GATE_KINDS.contains(&a.kind.as_str()))
            .map(|a| {
                let promotion = a.kind == "claim_promotion";
                (
                    a.targets.as_slice(),
                    if promotion { a.tier.min(ceiling) } else { a.tier },
                    promotion,
                )
            })
            .collect();
        let belief_diff = if events.len() == acts.len() && !acts.is_empty() {
            self.gate_diff(workspace, &events, cx)?
        } else {
            uncomputable_diff("the console previews claim_promotion and claim_demotion only")
        };
        let asserted = self.asserted_entity_ids(workspace, cx)?;
        let decided = self.decided_merges(workspace, cx)?;
        let mut checks: Vec<CheckResult> = Vec::new();
        for act in acts {
            let view = ProposalView {
                id: String::new(),
                no_effect_yet: !has_commit_effect(&act.kind),
                kind: act.kind.clone(),
                targets: act.targets.clone(),
                into: None,
                tier: Some(act.tier),
                rationale: None,
                affected_types: Vec::new(),
                granted_tier: None,
                workspace: workspace.unwrap_or_default().to_string(),
                state: "open".into(),
                verdicts: 0,
                opened_at: 0,
                proposer: String::new(),
                self_attested: false,
                labels: Vec::new(),
                into_label: None,
                merge_cast: false,
                belief_diff: None,
                checks: Vec::new(),
            };
            for c in self.blocking_checks(&view, &asserted, &decided, cx)? {
                if !checks
                    .iter()
                    .any(|k| k.name == c.name && k.passed == c.passed && k.detail == c.detail)
                {
                    checks.push(c);
                }
            }
        }
        Ok(GatePreview { belief_diff, checks })
    }

    /// What reverses a merged claim promotion or demotion (design-system.md Section 29.3): for each
    /// target the proposal still moves, the act that returns it to the tier it has with this
    /// proposal's verdict left out of the gate fold, and those acts' diff and checks at the console's
    /// grant, as [`Engine::preview_gate`] answers them. No acts when the proposal moves no target any
    /// more - another verdict holds the same tier. `None` when no merged gate proposal has that id.
    /// Writes nothing.
    pub fn undo_gate(
        &self,
        workspace: Option<&str>,
        proposal: &str,
    ) -> Result<Option<GateUndo>, StoreError> {
        let cx = &ReadCtx::default();
        let views = self.fold_proposals(workspace, cx)?;
        let Some(view) = views.get(proposal) else {
            return Ok(None);
        };
        if view.state != "merged" || !GATE_KINDS.contains(&view.kind.as_str()) {
            return Ok(None);
        }
        let now = self.gate_tiers(workspace, cx, &[], None)?;
        let before = self.gate_tiers(workspace, cx, &[], Some(proposal))?;
        let mut acts: Vec<GateAct> = Vec::new();
        for obs in self.log(workspace, cx)?.iter().filter(|o| view.targets.contains(&o.id)) {
            let (is, was) = (effective_tier(obs, &now), effective_tier(obs, &before));
            if is != was {
                acts.push(GateAct {
                    kind: if was < is { "claim_demotion" } else { "claim_promotion" }.into(),
                    targets: vec![obs.id.clone()],
                    tier: was,
                });
            }
        }
        let (belief_diff, checks) = if acts.is_empty() {
            (None, Vec::new())
        } else {
            let p = self.preview_gate(workspace, &acts)?;
            (Some(p.belief_diff), p.checks)
        };
        Ok(Some(GateUndo {
            proposal: proposal.to_string(),
            acts: acts
                .into_iter()
                .map(|a| UndoAct {
                    observation: a.targets.into_iter().next().unwrap_or_default(),
                    kind: a.kind,
                    tier: a.tier,
                })
                .collect(),
            belief_diff,
            checks,
        }))
    }

    /// Materialize the canon with and without this proposal's effects and report the difference
    /// (proposal-workflow.md Section 5). Only the gate grants differ between the two folds, so the
    /// "after" side is computed by the same code path a merged verdict would take - the diff cannot
    /// promise an outcome the merge would not deliver.
    /// The entity ids the LOG asserts, derived from observations rather than read off the projection.
    ///
    /// Referential integrity has to be a function of the event set (I8). Reading the projection made
    /// it a function of whether re-materialization had happened yet, and a freshly synced node holds
    /// every observation while its projection is still empty - so the same merge counted as valid on
    /// the authoring node and blocked on the receiving one, which is canon becoming node-dependent.
    /// A property test on arrival order is what surfaced that (`i8_blocking_check_conclusion_is_...`).
    pub(crate) fn asserted_entity_ids(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<String>, StoreError> {
        Ok(asserted_in(self.log(workspace, cx)?.iter()))
    }

    /// The blocking checks of proposal-workflow.md Section 6, recomputed from the proposal and what
    /// the local log holds.
    ///
    /// Deliberately NOT read from a `check_reported` event (I13): a check result is a cache for UX,
    /// and trusting it would mean a forged "pass" could promote contamination into canon. It also has
    /// to be the fold that enforces rather than the review entry point, because under federation a
    /// verdict arrives as an observation from another node and never passes through `review_proposal`
    /// here at all.
    ///
    /// Pure in (proposal, base) (I8), and it reads the store directly rather than through any surface
    /// that folds proposals - both to stay a pure function of the log and to avoid re-entering the
    /// fold that calls this. The one row it takes from the read instead is a writer's overlay, the
    /// row about to be committed (crash-recovery.md Section 3a), which the store does not hold yet.
    pub(crate) fn blocking_checks(
        &self,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
        cx: &ReadCtx,
    ) -> Result<Vec<CheckResult>, StoreError> {
        let mut out = Vec::new();
        let mut check = |name: &str, passed: bool, detail: String| {
            out.push(CheckResult { name: name.into(), blocking: true, passed, detail })
        };

        match view.kind.as_str() {
            "entity_split" => {
                // Referential integrity for a reversal: you cannot un-merge what is not here. The
                // target is an entity_merge PROPOSAL id (unmerge.md Section 4), and it has to have
                // been decided - a split of something still open would be a verdict about an act
                // nobody has committed to.
                let t = view.targets.first();
                let known = t.is_some_and(|t| decided_merges.contains(t));
                check(
                    "reversible target",
                    known,
                    match t {
                        None => "an entity_split names no proposal to reverse".into(),
                        Some(t) if known => format!("{t} is a decided entity_merge"),
                        Some(t) => format!(
                            "{t} is not an entity_merge that has been decided - it is absent from \
                             this proposal's workspace, is another kind, or has no merge verdict yet"
                        ),
                    },
                );
            }
            k if GATE_KINDS.contains(&k) => {
                // Referential integrity: you cannot promote what is not here. Under incomplete sync
                // the targets may simply not have arrived, and merging then would grant a tier to
                // nothing.
                // A proposal acts within its own workspace (proposal-workflow.md Section 6), so a
                // target written to another workspace is as absent here as one never written.
                let mut missing = Vec::new();
                for t in &view.targets {
                    let here = match cx.held(self.store.as_ref(), t)? {
                        Some(o) => view.workspace.is_empty() || o.workspace() == view.workspace,
                        None => false,
                    };
                    if !here {
                        missing.push(t.clone());
                    }
                }
                check(
                    "referential integrity",
                    missing.is_empty() && !view.targets.is_empty(),
                    if view.targets.is_empty() {
                        "a gate proposal names no target observation".into()
                    } else if missing.is_empty() {
                        format!(
                            "all {} target observations are in the local log",
                            view.targets.len()
                        )
                    } else {
                        format!(
                            "{} target observation(s) are not in this proposal's workspace: {}",
                            missing.len(),
                            missing.join(", ")
                        )
                    },
                );
                check(
                    "tier stated",
                    view.tier.is_some(),
                    match view.tier {
                        Some(t) => format!("grants {t:?}"),
                        None => "a gate proposal with no requested tier grants nothing".into(),
                    },
                );
            }
            "entity_merge" => {
                let into = view.into.clone().unwrap_or_default();
                check(
                    "canonical target named",
                    !into.is_empty() && view.targets.contains(&into),
                    if into.is_empty() {
                        "entity_merge names no `into`, so there is nothing to fold onto".into()
                    } else if view.targets.contains(&into) {
                        "the canonical id is among the targets".into()
                    } else {
                        "`into` is not among the targets - the fold would drop every named entity"
                            .into()
                    },
                );
                let distinct: BTreeSet<&String> = view.targets.iter().collect();
                check(
                    "distinct targets",
                    distinct.len() >= 2,
                    format!("{} distinct entities named; a merge needs at least 2", distinct.len()),
                );
                let missing: Vec<String> =
                    view.targets.iter().filter(|t| !asserted.contains(*t)).cloned().collect();
                check(
                    "referential integrity",
                    missing.is_empty(),
                    if missing.is_empty() {
                        format!(
                            "all {} target entities are asserted in the local log",
                            view.targets.len()
                        )
                    } else {
                        format!(
                            "{} target entit(ies) are not asserted in this proposal's workspace: {}",
                            missing.len(),
                            missing.join(", ")
                        )
                    },
                );
            }
            "tbox_change" => {
                // The one structural T-Box check available before a subtype hierarchy exists: a name
                // defined on both vocabularies. Principle 9 - a structural contradiction is a bug, so
                // it blocks rather than merely surfacing.
                let mut axis: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
                for a in &view.affected_types {
                    let e = axis.entry(a.name.as_str()).or_insert((false, false));
                    match a.target {
                        TypeTarget::Entity => e.0 = true,
                        TypeTarget::Relation => e.1 = true,
                    }
                }
                let collisions: Vec<&str> =
                    axis.into_iter().filter(|(_, (e, r))| *e && *r).map(|(n, _)| n).collect();
                check(
                    "t-box axis consistency",
                    collisions.is_empty(),
                    if collisions.is_empty() {
                        "no name is defined on both the entity and relation axes".into()
                    } else {
                        format!("defined on both axes: {}", collisions.join(", "))
                    },
                );
            }
            _ => {}
        }
        Ok(out)
    }

    /// The blocking checks that FAIL - empty means a merge verdict may take effect.
    pub(crate) fn blocking_failures(
        &self,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
        cx: &ReadCtx,
    ) -> Result<Vec<String>, StoreError> {
        Ok(self
            .blocking_checks(view, asserted, decided_merges, cx)?
            .into_iter()
            .filter(|c| c.blocking && !c.passed)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect())
    }
}
