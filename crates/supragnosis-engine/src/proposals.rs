//! The proposal fold and the proposal listing: states, labels and the trust profile.

use super::*;

/// Whether a verdict carries the engine-stamped console marker on any of its attestations - the
/// human's direct act of I17. Read off every attestation, as the gate ceiling is: one decision cast
/// twice is one row with two attestations (P3), and a person's console act on it is still theirs.
pub(crate) fn console_cast(obs: &Observation) -> bool {
    obs.provenance
        .iter()
        .any(|p| p.source_ref.as_deref() == Some(VERDICT_SURFACE_CONSOLE))
}

/// A source said in one line: its first [`LABEL_SAID_CHARS`] characters in quotes, cut with "...",
/// and the entities it names, four at most: "The ledger is a Postgres table..." (Ledger, Nightly
/// export). The proposal rows' labels and the Log's event rows say a gate act's observation so.
fn source_label(o: &Observation) -> String {
    let flat: String = o.content.split_whitespace().collect::<Vec<_>>().join(" ");
    let said: String = flat.chars().take(LABEL_SAID_CHARS).collect();
    let cut = flat.chars().count() > LABEL_SAID_CHARS;
    let mut names: Vec<&str> = Vec::new();
    let named = o
        .assertions
        .entities
        .iter()
        .map(|e| e.name.as_str())
        .chain(o.assertions.relations.iter().flat_map(|r| [r.from.as_str(), r.to.as_str()]));
    for n in named {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let shown = names.iter().take(4).copied().collect::<Vec<_>>().join(", ");
    let more = names.len().saturating_sub(4);
    let entities = match (names.is_empty(), more) {
        (true, _) => String::new(),
        (false, 0) => format!(" ({shown})"),
        (false, n) => format!(" ({shown} and {n} more)"),
    };
    format!("\"{}{}\"{entities}", said.trim_end(), if cut { "..." } else { "" })
}

/// Transient verdict accumulator for the proposal fold.
#[derive(Default)]
struct ProposalTally {
    merge: bool,
    reject: bool,
    withdraw: bool,
    verdicts: usize,
}

impl Engine {
    /// Folds the proposal events in the workspace into their current states (I2). Solo decision rule:
    /// merge is the absorbing state (I16), then withdrawn, then rejected, else open.
    pub(crate) fn fold_proposals(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<BTreeMap<String, ProposalView>, StoreError> {
        let obss = self.log(workspace, cx)?;
        if workspace.is_some() {
            return self.fold_workspace(&obss.iter().collect::<Vec<_>>(), cx);
        }
        // All is a view, not a place (design-system.md Section 29.15): it is the union of each
        // workspace's fold, never a fold across them. A verdict therefore counts only where its
        // proposal lives, a proposal's checks read its own workspace, and the forwarding every
        // all-workspaces projection applies is the union of each workspace's (proposal-workflow.md
        // I2, store-migration.md Section 6).
        let mut by_ws: BTreeMap<&str, Vec<&Observation>> = BTreeMap::new();
        for obs in obss.iter() {
            by_ws.entry(obs.workspace()).or_default().push(obs);
        }
        let mut views = BTreeMap::new();
        for group in by_ws.values() {
            views.extend(self.fold_workspace(group, cx)?);
        }
        Ok(views)
    }

    /// The proposal fold over one workspace's log.
    fn fold_workspace(
        &self,
        obss: &[&Observation],
        cx: &ReadCtx,
    ) -> Result<BTreeMap<String, ProposalView>, StoreError> {
        let mut views: BTreeMap<String, ProposalView> = BTreeMap::new();
        let mut tally: HashMap<String, ProposalTally> = HashMap::new();
        // Pass 1: opened events define the proposals (id = the opening observation id).
        for obs in obss {
            // Attribution follows the authoring attestation (P2) - a max over observed_at moves
            // forward as attestations absorb, and first() names the sort-first host, not the author.
            let author = authoring_attestation(obs);
            let observed_at = author.map(|p| p.observed_at).unwrap_or(0);
            let proposer = author
                .map(|p| match &p.on_behalf_of {
                    Some(who) => format!("{}@{}", who, p.host),
                    None => p.host.clone(),
                })
                .unwrap_or_default();
            for ev in &obs.assertions.proposal_events {
                if ev.event != ProposalEventKind::Opened {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let targets = v
                    .get("targets")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let into = v.get("into").and_then(|x| x.as_str()).map(String::from);
                let tier = v.get("tier").and_then(|x| x.as_str()).and_then(parse_tier);
                let rationale = v.get("rationale").and_then(|x| x.as_str()).map(String::from);
                // affected_types is absent on pre-M3.5 proposals and on kinds that do not declare it.
                let affected_types: Vec<AffectedType> = v
                    .get("affected_types")
                    .and_then(|x| serde_json::from_value(x.clone()).ok())
                    .unwrap_or_default();
                views.entry(obs.id.clone()).or_insert(ProposalView {
                    id: obs.id.clone(),
                    no_effect_yet: !has_commit_effect(&kind),
                    kind,
                    targets,
                    into,
                    tier,
                    rationale,
                    affected_types,
                    granted_tier: None,
                    workspace: obs.workspace().to_string(),
                    state: "open".into(),
                    verdicts: 0,
                    opened_at: observed_at,
                    proposer: proposer.clone(),
                    self_attested: true,
                    labels: Vec::new(),
                    into_label: None,
                    merge_cast: false,
                    belief_diff: None,
                    checks: Vec::new(),
                });
            }
        }
        // Pass 2: verdicts/withdrawals accumulate (order-independent - set semantics, I3).
        for obs in obss {
            for ev in &obs.assertions.proposal_events {
                match ev.event {
                    ProposalEventKind::Verdict => {
                        let v: serde_json::Value =
                            serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                        let decision = v.get("decision").and_then(|x| x.as_str());
                        let kind = views.get(&ev.proposal).map(|p| p.kind.as_str());
                        // 7.1 (f): a merge of a kind with no commit effect yet has no diff, and a
                        // merge without a diff is a merge without review (Principle 23). It is
                        // demoted to a comment - it stays in the log and decides nothing - whichever
                        // path it arrived by, so a merge an earlier build took, or one replicated
                        // from it, never fires once the kind has its effect (design-system.md Section
                        // 29.22). The stamp that lets a merge count then is built with the effect.
                        if decision == Some("merge") && kind.is_some_and(|k| !has_commit_effect(k))
                        {
                            continue;
                        }
                        // I17: a recall merge counts only when the verdict carries the engine-stamped
                        // console marker - the human's direct act (resolution.md Section 6). Any other
                        // recall merge is demoted to a comment. The marker is read off the log, never
                        // off the write path, so a verdict that arrived replicated, or landed through
                        // MCP before the agent surface refused them, is judged the same way (P16).
                        // Until recall has its effect, (f) above has demoted every recall merge first.
                        if decision == Some("merge") && kind == Some("recall") && !console_cast(obs)
                        {
                            continue;
                        }
                        let t = tally.entry(ev.proposal.clone()).or_default();
                        t.verdicts += 1;
                        match decision {
                            Some("merge") => t.merge = true,
                            Some("reject") => t.reject = true,
                            _ => {}
                        }
                    }
                    ProposalEventKind::Withdrawn => {
                        tally.entry(ev.proposal.clone()).or_default().withdraw = true;
                    }
                    _ => {}
                }
            }
        }
        // A merge verdict only reaches canon if the blocking checks pass, and the check is recomputed
        // HERE rather than read from a check_reported event (I13). This is also why enforcement cannot
        // live in review_proposal: a verdict can arrive from another node as a replicated observation
        // and never pass through it. Computed only for proposals that actually carry a merge verdict,
        // so the common path pays nothing. Separate pass to keep the view borrow immutable.
        let mut blocked: HashSet<String> = HashSet::new();
        if views.iter().any(|(id, _)| tally.get(id).is_some_and(|t| t.merge)) {
            // One log pass, shared by every proposal being checked.
            let asserted = asserted_in(obss.iter().copied());
            // The entity_split check needs to know which merges were decided. This pass IS the fold
            // that assigns states, so it cannot ask for them - but `t.merge` is precisely the
            // condition [`carries_merge_verdict`] recognizes once a state exists.
            let decided: HashSet<String> = views
                .iter()
                .filter(|(id, v)| {
                    v.kind == "entity_merge" && tally.get(*id).is_some_and(|t| t.merge)
                })
                .map(|(id, _)| id.clone())
                .collect();
            for (id, view) in views.iter() {
                if tally.get(id).is_some_and(|t| t.merge)
                    && !self.blocking_failures(view, &asserted, &decided, cx)?.is_empty()
                {
                    blocked.insert(id.clone());
                }
            }
        }
        for (id, view) in views.iter_mut() {
            if let Some(t) = tally.get(id) {
                view.verdicts = t.verdicts;
                view.merge_cast = t.merge;
                view.state = if t.merge && !blocked.contains(id) {
                    "merged"
                } else if t.merge && t.reject {
                    // A blocked merge is not a valid merge, so a valid reject concludes Rejected
                    // (proposal-workflow.md Section 7.1). Once the check passes the merge is valid,
                    // and it is Merged, as a late merge is (I16).
                    "rejected"
                } else if t.merge {
                    // The verdict is recorded and stays in the log; what it does not do is commit.
                    "blocked"
                } else if t.withdraw {
                    "withdrawn"
                } else if t.reject {
                    "rejected"
                } else {
                    "open"
                }
                .into();
            }
        }
        Ok(views)
    }

    /// All proposals in the workspace, newest first (opened_at desc, id asc for ties - deterministic).
    pub fn list_proposals(&self, workspace: Option<&str>) -> Result<Vec<ProposalView>, StoreError> {
        let cx = &ReadCtx::default();
        let mut all = self.fold_proposals(workspace, cx)?;
        self.label_proposals(&mut all)?;
        self.fill_granted(workspace, &mut all, cx)?;
        let mut v: Vec<ProposalView> = all.into_values().collect();
        v.sort_by(|a, b| b.opened_at.cmp(&a.opened_at).then_with(|| a.id.cmp(&b.id)));
        Ok(v)
    }

    /// Fills each merged promotion's `granted_tier` from the same reading of its verdicts the fold
    /// applies (design-system.md Section 29.20).
    fn fill_granted(
        &self,
        workspace: Option<&str>,
        views: &mut BTreeMap<String, ProposalView>,
        cx: &ReadCtx,
    ) -> Result<(), StoreError> {
        if !views.values().any(|v| v.kind == "claim_promotion" && v.state == "merged") {
            return Ok(());
        }
        let (_, rep_merge) = self.gate_verdicts(workspace, cx)?;
        for v in views.values_mut() {
            if v.kind == "claim_promotion" && v.state == "merged" {
                if let (Some(asked), Some((_, _, ceiling))) = (v.tier, rep_merge.get(&v.id)) {
                    v.granted_tier = Some(asked.min(*ceiling));
                }
            }
        }
        Ok(())
    }

    /// Fills each view's `labels` and `into_label` (design-system.md Section 29.3). Point reads
    /// only, so the proposal list, which the viewer reads on every poll, does not scan the entity
    /// table.
    fn label_proposals(
        &self,
        views: &mut BTreeMap<String, ProposalView>,
    ) -> Result<(), StoreError> {
        let mut labels = Vec::new();
        for (id, v) in views.iter() {
            labels.push((id.clone(), self.labels_for(id, v, views)?));
        }
        for (id, (ls, into)) in labels {
            if let Some(v) = views.get_mut(&id) {
                v.labels = ls;
                v.into_label = into;
            }
        }
        Ok(())
    }

    /// One proposal's labels: each target's, and its merge target's. A label names only what is in
    /// the proposal's own workspace - the workspace its opening observation was written to; a target
    /// elsewhere keeps `None`.
    pub(crate) fn labels_for(
        &self,
        id: &str,
        view: &ProposalView,
        views: &BTreeMap<String, ProposalView>,
    ) -> Result<(Vec<Option<String>>, Option<String>), StoreError> {
        let Some(ws) = self.store.get_observation(id)?.map(|o| o.workspace().to_string()) else {
            return Ok((vec![None; view.targets.len()], None));
        };
        let mut ls = Vec::with_capacity(view.targets.len());
        for t in &view.targets {
            ls.push(self.target_label(t, &ws, views)?);
        }
        let into = match &view.into {
            Some(i) => self.target_label(i, &ws, views)?,
            None => None,
        };
        Ok((ls, into))
    }

    /// How a proposal's target in workspace `ws` is said to a reader: an entity by its name (the
    /// entity table keeps a merged-away one); a proposal, which an un-merge targets, by what it
    /// merged; an observation by its first 80 characters and the entities it names: "The ledger is
    /// a Postgres table..." (Ledger, Nightly export). `None` for what is not in `ws`, and for what
    /// nothing on this node can say.
    fn target_label(
        &self,
        id: &str,
        ws: &str,
        views: &BTreeMap<String, ProposalView>,
    ) -> Result<Option<String>, StoreError> {
        let entity_name = |id: &str| -> Result<Option<String>, StoreError> {
            Ok(self
                .store
                .get_entity(id)?
                .filter(|e| e.provenance.iter().any(|p| p.workspace == ws))
                .map(|e| e.canonical_name))
        };
        if self.store.get_entity(id)?.is_some() {
            return entity_name(id);
        }
        if let Some(p) = views.get(id) {
            if self.store.get_observation(id)?.is_none_or(|o| o.workspace() != ws) {
                return Ok(None);
            }
            let mut names = Vec::new();
            for t in p.targets.iter().filter(|t| p.into.as_deref() != Some(t.as_str())) {
                names.extend(entity_name(t)?);
            }
            let into = match &p.into {
                Some(i) => entity_name(i)?,
                None => None,
            };
            return Ok(match (names.is_empty(), into) {
                (false, Some(into)) => Some(format!("{} into {into}", names.join(", "))),
                (false, None) => Some(names.join(", ")),
                (true, into) => into,
            });
        }
        Ok(self
            .store
            .get_observation(id)?
            .filter(|o| o.workspace() == ws)
            .map(|o| source_label(&o)))
    }

    /// One proposal's folded state by id (None if there is no such proposal).
    pub fn get_proposal(
        &self,
        workspace: Option<&str>,
        id: &str,
    ) -> Result<Option<ProposalView>, StoreError> {
        // Under All a proposal is read where it lives, so its diff and checks are its workspace's
        // (design-system.md Section 29.15).
        if workspace.is_none() {
            return match self.proposal_workspace(id)? {
                Some(home) => self.get_proposal(Some(&home), id),
                None => Ok(None),
            };
        }
        let cx = &ReadCtx::default();
        let mut all = self.fold_proposals(workspace, cx)?;
        self.fill_granted(workspace, &mut all, cx)?;
        let Some(mut view) = all.remove(id) else {
            return Ok(None);
        };
        (view.labels, view.into_label) = self.labels_for(id, &view, &all)?;
        view.belief_diff = Some(self.belief_diff(workspace, &view, cx)?);
        view.checks = self.blocking_checks(
            &view,
            &self.asserted_entity_ids(workspace, cx)?,
            &self.decided_merges(workspace, cx)?,
            cx,
        )?;
        if let Some(profile) = self.trust_profile(workspace, &view, cx)? {
            view.checks.push(profile);
        }
        Ok(Some(view))
    }

    /// The informative trust profile beside the blocking checks (proposal-workflow.md Section 6,
    /// design-system.md Section 29.7): how many observations the proposal rests on, at each
    /// EFFECTIVE tier - for a merge those that state its entities, for a gate act its targets. It
    /// blocks nothing (the fold reads blocking checks only), and is `None` for a kind that rests on
    /// no observation of its own. Highest tier first, as "3 agent_extracted, 1 human_confirmed".
    fn trust_profile(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        cx: &ReadCtx,
    ) -> Result<Option<CheckResult>, StoreError> {
        let gate = GATE_KINDS.contains(&view.kind.as_str());
        if view.kind != "entity_merge" && !gate {
            return Ok(None);
        }
        let fwd = self.merge_forwarding(workspace, cx)?;
        let gates = self.gate_grants(workspace, cx)?;
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
        let mut wanted: HashSet<String> = view.targets.iter().cloned().collect();
        wanted.extend(view.into.iter().cloned());
        // A merge's entities are counted after forwarding, so an entity merged elsewhere still
        // finds the observations that named it.
        let wanted_canon: HashSet<String> = wanted.iter().map(|t| canon(t.clone())).collect();
        let mut counts: BTreeMap<TrustTier, usize> = BTreeMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            let rests = if gate {
                wanted.contains(&obs.id)
            } else {
                let ws = obs.workspace();
                obs.assertions
                    .entities
                    .iter()
                    .map(|e| e.name.as_str())
                    .chain(
                        obs.assertions
                            .relations
                            .iter()
                            .flat_map(|r| [r.from.as_str(), r.to.as_str()]),
                    )
                    .any(|n| wanted_canon.contains(&canon(Entity::make_id(ws, n))))
            };
            if rests {
                *counts.entry(effective_tier(obs, &gates)).or_default() += 1;
            }
        }
        let detail = counts
            .iter()
            .rev()
            .map(|(tier, n)| format!("{n} {}", tier_label(*tier)))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Some(CheckResult {
            name: "trust profile".into(),
            blocking: false,
            passed: true,
            detail,
        }))
    }
}

/// The entity ids a run of observations asserts, each minted in its observation's workspace: the
/// referential-integrity check's input (proposal-workflow.md Section 6), read off the log rather than
/// the projection (I8).
pub(crate) fn asserted_in<'a>(obss: impl Iterator<Item = &'a Observation>) -> HashSet<String> {
    let mut ids = HashSet::new();
    for obs in obss {
        let ws = obs.workspace();
        for e in &obs.assertions.entities {
            ids.insert(Entity::make_id(ws, &e.name));
        }
        // Both endpoints of a relation assert their entities too - the projection creates rows for
        // them, so the check must see them as existing for the same reason.
        for r in &obs.assertions.relations {
            ids.insert(Entity::make_id(ws, &r.from));
            ids.insert(Entity::make_id(ws, &r.to));
        }
    }
    ids
}
