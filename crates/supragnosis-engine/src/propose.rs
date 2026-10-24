//! Opening proposals: `propose` and its console variants, and the reads beside them.

use super::*;

/// A reason that asks again: ", again" the second time, ", again (3)" the third, and so on - a
/// different reason, so a different proposal (P14, unmerge.md S10).
fn again(rationale: &Option<String>, asked: usize) -> Option<String> {
    let suffix = match asked {
        0 => return rationale.clone(),
        1 => "again".to_string(),
        n => format!("again ({})", n + 1),
    };
    Some(match rationale {
        Some(r) if !r.is_empty() => format!("{r}, {suffix}"),
        _ => suffix,
    })
}

impl Engine {
    // --- Proposal workflow (Principle 23, solo-scoped M3.5a) ---------------------------------------
    // A proposal and its verdicts are observations (I1); the state is a deterministic fold (I2). This is
    // the gate skeleton - it records/folds open+verdict, but the belief diff, blocking checks, and the
    // merge EFFECT (id forwarding) are later steps. Solo scope: no HLC/quorum; self-approval is the
    // single-user exception, marked self_attested (Principle 23).

    /// Open a proposal (Principle 23). Records an `opened` event as an observation; the observation id
    /// becomes the proposal id. Validates only well-formedness (kind known, targets present, entity_merge
    /// has a valid `into`) - it does not yet commit or check the canon.
    /// The id an entity_merge target names (design-system.md Section 14 step 13). An entity id is
    /// kept. Anything else is read as a name and turned into `Entity::make_id(workspace, name)` - the
    /// id `observe` gives that name - when that entity exists. An id-shaped target that is not here is
    /// kept too, since on a spoke it may not have synced yet, and the referential-integrity check says
    /// so; a name that names nothing is refused here, with the reason, rather than stored and blocked.
    fn merge_target_id(&self, workspace: &str, target: &str) -> Result<String, ObserveError> {
        if self.store.get_entity(target)?.is_some() {
            return Ok(target.to_string());
        }
        let id = Entity::make_id(workspace, target);
        if self.store.get_entity(&id)?.is_some() {
            return Ok(id);
        }
        if target.len() == 64 && target.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(target.to_string());
        }
        Err(ObserveError::Invalid(format!(
            "entity_merge target '{target}' is neither an entity id nor the name of an entity in \
             workspace '{workspace}' - name it as observe did, or use the id a search hit carries"
        )))
    }

    pub fn propose(&self, input: ProposeInput) -> Result<String, ObserveError> {
        self.propose_reporting(input).map(|p| p.id)
    }

    /// [`Engine::propose`], saying what it found: an identical proposal already in the log is that
    /// proposal (P14), absorbed (P3), and its state is the answer's (design-system.md Section 29.23).
    pub fn propose_reporting(&self, input: ProposeInput) -> Result<Proposed, ObserveError> {
        reject_reserved_source_ref(input.source_ref.as_deref())?;
        self.propose_marked(input, None, false)
    }

    /// A person's act at the local console's own server (design-system.md Section 29.23): when the
    /// proposal it would open is in the log and its answer no longer holds, it is opened again with
    /// ", again" in its reason; when it still holds, nothing is written and the answer says what was
    /// found. `marked` stamps the console's mark on the opening, as [`Engine::propose_at_console`].
    pub fn propose_anew(
        &self,
        input: ProposeInput,
        marked: bool,
    ) -> Result<Proposed, ObserveError> {
        if input.source_ref.is_some() {
            return Err(ObserveError::Invalid(
                "a proposal opened at the console carries the console's mark, not a source of its own"
                    .into(),
            ));
        }
        self.propose_marked(input, marked.then_some(VERDICT_SURFACE_CONSOLE), true)
    }

    /// [`Engine::propose`] for the local console's own server: the opening observation carries the
    /// console's mark, engine-stamped as a verdict's is (resolution.md Section 6), so who asked reads
    /// as the person at that console (design-system.md Section 29.17). The input names no source of
    /// its own; the mark grants nothing, since the ceiling and the I17 check read verdicts only.
    pub fn propose_at_console(&self, input: ProposeInput) -> Result<String, ObserveError> {
        if input.source_ref.is_some() {
            return Err(ObserveError::Invalid(
                "a proposal opened at the console carries the console's mark, not a source of its own"
                    .into(),
            ));
        }
        self.propose_marked(input, Some(VERDICT_SURFACE_CONSOLE), false).map(|p| p.id)
    }

    /// How many merged un-merges of the same two-or-more entities, merged into the same one, this
    /// workspace's log holds. The console counts them into a re-merge's reason so it is a different
    /// proposal from the one each split reversed (unmerge.md S10, design-system.md Section 29.17).
    pub fn unmerges_of(
        &self,
        workspace: &str,
        targets: &[String],
        into: &str,
    ) -> Result<usize, StoreError> {
        let views = self.fold_proposals(Some(workspace), &ReadCtx::default())?;
        let want: BTreeSet<&str> = targets.iter().map(String::as_str).collect();
        Ok(views
            .values()
            .filter(|p| p.kind == "entity_split" && p.state == "merged")
            .filter_map(|p| p.targets.first().and_then(|t| views.get(t)))
            .filter(|m| {
                m.kind == "entity_merge"
                    && m.into.as_deref() == Some(into)
                    && m.targets.iter().map(String::as_str).collect::<BTreeSet<_>>() == want
            })
            .count())
    }

    fn propose_marked(
        &self,
        input: ProposeInput,
        marker: Option<&str>,
        anew: bool,
    ) -> Result<Proposed, ObserveError> {
        if !PROPOSAL_KINDS.contains(&input.kind.as_str()) {
            return Err(ObserveError::Invalid(format!(
                "unknown proposal kind '{}'. use one of {PROPOSAL_KINDS:?}",
                input.kind
            )));
        }
        if input.targets.iter().any(|t| t.trim().is_empty()) || input.targets.is_empty() {
            return Err(ObserveError::Invalid(
                "a proposal needs at least one non-empty target id".into(),
            ));
        }
        let mut input = input;
        if input.kind == "entity_merge" {
            // An agent names entities by name as readily as by id. A target that is a name is turned
            // into the id `observe` gave that name in this workspace, before anything is checked or
            // stored, so the proposal holds ids and is never blocked for naming what exists.
            let ws = input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
            for t in input.targets.iter_mut() {
                *t = self.merge_target_id(&ws, t)?;
            }
            // `into` must be one of the targets, and the check below says so in those words when it
            // names nothing the targets hold.
            if let Some(into) = input.into.as_mut() {
                if let Ok(id) = self.merge_target_id(&ws, into) {
                    *into = id;
                }
            }
            if input.targets.len() < 2 {
                return Err(ObserveError::Invalid(
                    "entity_merge needs at least 2 target ids (the entities to merge)".into(),
                ));
            }
            // Resolution stays inside a workspace (Principle 15): the workspace is the population an
            // id is resolved in and the sharing unit, so a merge across two belongs to no canon -
            // Blocked in the workspace it is filed in, and never Merged anywhere else
            // (design-system.md Section 29.15). A target this node does not hold yet is still taken,
            // and blocks until it arrives. The remote surface checks targets before they get here,
            // so this sentence, which names the other workspace, reaches only the local surfaces.
            for t in &input.targets {
                if let Some(e) = self.store.get_entity(t)? {
                    let home = entity_workspace(&e);
                    if home != ws {
                        return Err(ObserveError::Invalid(format!(
                            "entity_merge joins entities of one workspace: {} is in {home}, and \
                             this proposal is filed in {ws}. The same name in two workspaces is \
                             two entities (Principle 15)",
                            e.canonical_name
                        )));
                    }
                }
            }
            match &input.into {
                None => {
                    return Err(ObserveError::Invalid(
                        "entity_merge needs `into` - the canonical target id the others fold into"
                            .into(),
                    ))
                }
                Some(into) if !input.targets.contains(into) => {
                    return Err(ObserveError::Invalid("`into` must be one of the targets".into()))
                }
                _ => {}
            }
        }
        // A split names the resolution it reverses, so its single target is an entity_merge PROPOSAL
        // id, not an entity id (unmerge.md Section 4 - the unit of reversal is the unit of decision).
        // Whether that proposal exists and actually merged is a blocking check, not well-formedness:
        // on a spoke the proposal may simply not have synced yet, and refusing at capture would make
        // an arrival-order accident look like a malformed request.
        if input.kind == "entity_split" {
            if input.targets.len() != 1 {
                return Err(ObserveError::Invalid(
                    "entity_split needs exactly one target - the entity_merge proposal id it \
                     reverses"
                        .into(),
                ));
            }
            if input.into.is_some() {
                return Err(ObserveError::Invalid(
                    "entity_split takes no `into` - it reverses a resolution rather than choosing a \
                     canonical id"
                        .into(),
                ));
            }
            // A proposal acts within its own workspace (design-system.md Section 29.15): a split
            // filed away from its merge would reverse nothing anywhere. A merge this node does not
            // hold yet is still taken, as above; the remote surface checks it before it gets here.
            let filed_in =
                input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
            if let Some(home) = self.proposal_workspace(&input.targets[0])? {
                if home != filed_in {
                    return Err(ObserveError::Invalid(format!(
                        "the merge {} lives in workspace {home}, and this split is filed in \
                         {filed_in} - a proposal acts within its own workspace, so file it there",
                        input.targets[0]
                    )));
                }
            }
        }
        // Gate kinds (resolution.md Section 5): a requested tier is mandatory, and the targets are
        // OBSERVATION ids that must exist in the local log - the referential-integrity blocking check
        // of proposal-workflow.md Section 6, applied at capture ("you cannot promote what is not
        // there"). Other kinds reject a tier so the surface stays honest (Principle 21).
        let gate_tier = if GATE_KINDS.contains(&input.kind.as_str()) {
            let Some(t) = input.tier.as_deref() else {
                return Err(ObserveError::Invalid(format!(
                    "{} needs `tier` - the requested trust tier: unverified | agent_extracted | \
                     host_signed | human_confirmed",
                    input.kind
                )));
            };
            let Some(tier) = parse_tier(t) else {
                return Err(ObserveError::Invalid(format!(
                    "unknown tier '{t}'. use unverified | agent_extracted | host_signed | human_confirmed"
                )));
            };
            let filed_in =
                input.workspace.clone().unwrap_or_else(|| self.default_workspace.clone());
            for target in &input.targets {
                let Some(obs) = self.store.get_observation(target)? else {
                    return Err(ObserveError::Invalid(format!(
                        "target observation '{target}' is not in the local log - {} targets are \
                         observation ids (as returned by observe / carried on search hits), and an \
                         observation that is not here cannot be promoted or demoted",
                        input.kind
                    )));
                };
                // A proposal acts within its own workspace (design-system.md Section 29.15). The
                // remote surface checks targets before they get here, so naming the other
                // workspace reaches only the local surfaces.
                if obs.workspace() != filed_in {
                    return Err(ObserveError::Invalid(format!(
                        "target observation '{target}' is in workspace {}, and this proposal is \
                         filed in {filed_in} - a proposal acts within its own workspace, so file it \
                         there",
                        obs.workspace()
                    )));
                }
            }
            Some(tier)
        } else {
            if input.tier.is_some() {
                return Err(ObserveError::Invalid(format!(
                    "`tier` only applies to claim_promotion / claim_demotion (got kind '{}')",
                    input.kind
                )));
            }
            None
        };
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let source_ref = marker.map(String::from).or(input.source_ref);
        let prov = self.provenance(&workspace, source_ref, None, input.on_behalf_of);
        // Relation type names are normalized so they match the graph's edge kinds exactly (the viewer
        // highlights edges by `kind`); entity type names are labels, kept verbatim.
        let affected_types: Vec<AffectedType> = input
            .affected_types
            .into_iter()
            .map(|a| AffectedType {
                name: match a.target {
                    TypeTarget::Relation => normalize_relation_kind(&a.name),
                    TypeTarget::Entity => a.name,
                },
                target: a.target,
            })
            .collect();
        let open = |rationale: &Option<String>| {
            let payload = serde_json::json!({
                "kind": input.kind,
                "targets": input.targets,
                "into": input.into,
                "tier": gate_tier.map(|t| tier_label(t).to_string()),
                "rationale": rationale,
                "affected_types": affected_types,
            })
            .to_string();
            let content = format!("proposal(open) {}: {:?}", input.kind, input.targets);
            let assertions = Assertions {
                proposal_events: vec![ProposalEventAssertion {
                    proposal: String::new(),
                    event: ProposalEventKind::Opened,
                    payload,
                }],
                ..Default::default()
            };
            Observation::with_assertions(content, prov.clone(), assertions)
        };
        // A proposal is its content (P14), so the same act on the same items is the proposal it
        // opened before. A person's act at the console asks again when that answer no longer holds,
        // with ", again" in its reason, and otherwise writes nothing and says what it found; an
        // agent's identical content is that proposal, absorbed (P3) and said (design-system.md
        // Section 29.23). Read and written under the write lock, so two acts cannot both ask again.
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let cx = &ReadCtx::default();
        let views = self.fold_proposals(Some(&workspace), cx)?;
        let mut asked = 0usize;
        let (obs, found) = loop {
            let obs = open(&again(&input.rationale, asked));
            let found = match self.store.get_observation(&obs.id)? {
                Some(_) => views.get(&obs.id),
                None => None,
            };
            match found {
                Some(view) if anew && !self.still_holds(&workspace, view, cx)? => asked += 1,
                _ => break (obs, found),
            }
        };
        let id = obs.id.clone();
        if anew {
            if let Some(view) = found {
                return Ok(Proposed { id, state: view.state.clone(), found: true });
            }
        }
        // A proposal IS its content (Principle 14), so re-opening an identical merge yields the same
        // id - including one a split has already reversed, and the split names that id forever. The
        // re-proposal would be permanently dead: accepted, verdicted, and unable to ever forward.
        // Refuse and name the fix instead (P21), which is also the honest one - a re-merge after a
        // split is a different act and should say why (unmerge.md Section 7).
        if input.kind == "entity_merge" && self.reversed_merges(Some(&workspace), cx)?.contains(&id)
        {
            return Err(ObserveError::Invalid(format!(
                "this exact resolution was already reversed by an entity_split. A proposal is its \
                 content (Principle 14), so re-opening it produces {id} again and it stays \
                 reversed. Give the re-merge a `rationale` saying why the split was wrong - that \
                 makes it a different proposal, which is what it is"
            )));
        }
        let state = found.map_or_else(|| "open".to_string(), |v| v.state.clone());
        let found = found.is_some();
        self.refuse_secrets(&obs)?;
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));
        // Proposal state is a read-time fold over the log; no materialized row waits on it, so the
        // row commits whole with no projection (crash-recovery.md Section 3a).
        self.store.commit_projected(obs, Vec::new(), Vec::new())?;
        self.note_app(app_key.as_deref());
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Proposed { id, state, found })
    }

    /// Whether a decided proposal's answer still holds, so a person's act asking the same at the
    /// console finds it rather than asking again (design-system.md Section 29.23): one that waits
    /// holds; a rejected or withdrawn one does not; an accepted merge holds until a split reverses
    /// it, and an accepted gate act until a later accepted act on one of its sources supersedes it.
    fn still_holds(
        &self,
        workspace: &str,
        view: &ProposalView,
        cx: &ReadCtx,
    ) -> Result<bool, StoreError> {
        Ok(match view.state.as_str() {
            "rejected" | "withdrawn" => false,
            "merged" if view.kind == "entity_merge" => {
                !self.reversed_merges(Some(workspace), cx)?.contains(&view.id)
            }
            "merged" if GATE_KINDS.contains(&view.kind.as_str()) => {
                let (opened, rep_merge) = self.gate_verdicts(Some(workspace), cx)?;
                let states = self.fold_proposals(Some(workspace), cx)?;
                let (Some((targets, _, _)), Some((at, _, _))) =
                    (opened.get(&view.id), rep_merge.get(&view.id))
                else {
                    return Ok(true);
                };
                // "Later" in the order the gate fold applies acts: by verdict HLC, then proposal id.
                !opened.iter().any(|(other, (theirs, _, _))| {
                    other != &view.id
                        && theirs.iter().any(|t| targets.contains(t))
                        && states.get(other).is_some_and(|p| p.state == "merged")
                        && rep_merge
                            .get(other)
                            .is_some_and(|(hlc, _, _)| (hlc, other) > (at, &view.id))
                })
            }
            _ => true,
        })
    }

    /// Every verdict or withdrawal filed in a workspace its proposal does not live in, oldest first
    /// by the log's order. A verdict whose proposal this node does not hold is not listed: it may be
    /// waiting for its proposal to arrive, and the fold tolerates that (I8).
    pub(crate) fn stray_verdicts(&self, cx: &ReadCtx) -> Result<Vec<StrayVerdict>, StoreError> {
        let log = self.log(None, cx)?;
        let homes: HashMap<&str, &str> = log
            .iter()
            .filter(|o| {
                o.assertions
                    .proposal_events
                    .iter()
                    .any(|e| e.event == ProposalEventKind::Opened)
            })
            .map(|o| (o.id.as_str(), o.workspace()))
            .collect();
        let mut out = Vec::new();
        for obs in log.iter() {
            for ev in &obs.assertions.proposal_events {
                if !matches!(ev.event, ProposalEventKind::Verdict | ProposalEventKind::Withdrawn) {
                    continue;
                }
                let Some(home) = homes.get(ev.proposal.as_str()) else {
                    continue;
                };
                if *home == obs.workspace() {
                    continue;
                }
                let decision = if ev.event == ProposalEventKind::Withdrawn {
                    "withdraw".to_string()
                } else {
                    serde_json::from_str::<serde_json::Value>(&ev.payload)
                        .ok()
                        .and_then(|v| v.get("decision").and_then(|d| d.as_str()).map(String::from))
                        .unwrap_or_default()
                };
                out.push(StrayVerdict {
                    observation: obs.id.clone(),
                    proposal: ev.proposal.clone(),
                    decision,
                    filed_in: obs.workspace().to_string(),
                    proposal_in: home.to_string(),
                });
            }
        }
        Ok(out)
    }

    /// The entities of `workspaces` that carry this entity's name (design-system.md Section 29.16),
    /// in workspace order; `None` when this node holds no entity by that id. The name is matched by
    /// the id's own rule, so each workspace costs one lookup. A namesake its own workspace merged into
    /// another entity is left out, as the curation report's `duplicates` leaves it out; the entity's
    /// own workspace is never listed.
    pub fn namesakes(
        &self,
        id: &str,
        workspaces: &[&str],
    ) -> Result<Option<Vec<Namesake>>, StoreError> {
        let Some(entity) = self.store.get_entity(id)? else {
            return Ok(None);
        };
        let own = entity_workspace(&entity).to_string();
        let mut list: Vec<&str> = workspaces
            .iter()
            .map(|w| w.trim())
            .filter(|w| !w.is_empty() && *w != own)
            .collect();
        list.sort_unstable();
        list.dedup();
        let mut out = Vec::new();
        for ws in list {
            let nid = Entity::make_id(ws, &entity.canonical_name);
            // `get_entity` forwards within the namesake's own workspace: one merged away there
            // comes back as the entity it became.
            if let Some(v) = self.get_entity(&nid)? {
                if v.entity.id == nid {
                    out.push(Namesake {
                        id: nid,
                        name: v.entity.canonical_name,
                        kind: v.entity.kind,
                        workspace: ws.to_string(),
                    });
                }
            }
        }
        Ok(Some(out))
    }

    /// Whether `workspace` holds any observation yet (design-system.md Section 29.20).
    pub fn holds_observations(&self, workspace: &str) -> Result<bool, StoreError> {
        self.store.holds_observations(workspace)
    }

    /// The workspace a proposal lives in: the one its opening observation was written to. `None` when
    /// this node holds no proposal by that id. The local surfaces use it to file a verdict cast under
    /// All where its proposal lives (design-system.md Section 29.15).
    pub fn proposal_workspace(&self, id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .store
            .get_observation(id)?
            .filter(|o| {
                o.assertions
                    .proposal_events
                    .iter()
                    .any(|e| e.event == ProposalEventKind::Opened)
            })
            .map(|o| o.workspace().to_string()))
    }
}
