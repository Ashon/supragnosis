//! The gate fold and the belief fold: effective tiers, kind resolution and candidate rows.

use super::*;

impl Engine {
    /// Gate tiers (resolution.md Section 5, R5 as revised in 2026-10): target observation id -> the
    /// tier its merged claim_promotion/claim_demotion events leave it at. Per proposal, the
    /// representative verdict is the earliest merge by (ordering HLC, observation id) - the
    /// proposal-workflow 7.1 canonicalization, which orders the proposal's effect among others on
    /// the same target; what it grants is min(requested tier, the highest surface ceiling among the
    /// proposal's merge verdicts) (resolution.md Section 6), so a further merge can raise a grant
    /// and never lower it, whichever verdict represents. The events on one target then apply in
    /// that order, by (representative HLC, proposal id), each from the tier the previous one left,
    /// starting from the target's base evaluation: a promotion leaves max(previous, grant), a
    /// demotion min(previous, grant). So a demotion can still push below base (the fast-path), and
    /// neither kind can move a target the other way - a demotion that asked for a higher tier used to
    /// promote (docs/dreaming.md 5.7). A pure fold-projection of the log, a function of the set of
    /// merged events, so it converges continuously (F5).
    ///
    /// A grant is a merge's COMMIT EFFECT, so it exists only where [`Engine::fold_proposals`] says
    /// the merge committed (state `merged`) - a verdict on a proposal whose blocking checks fail
    /// folds to `blocked` and grants nothing (I13), exactly as `merge_forwarding` already refuses
    /// to forward for one. Without this coupling the two folds can disagree: a claim_promotion
    /// whose targets are only partly synced would read `blocked` on the proposal surface while its
    /// present targets were already promoted. Skipping is the safe direction and stays monotone -
    /// when the missing target arrives the state flips to `merged` and the grant applies (the same
    /// blocked -> merged direction the blocking checks are pinned to).
    pub(crate) fn gate_grants(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashMap<String, TrustTier>, StoreError> {
        self.gate_tiers(workspace, cx, &[], None)
    }

    /// [`Engine::gate_grants`], with hypothetical events applied in order after every merged one:
    /// the belief diff's "what merging this would do" (`extra` = each event's targets, its grant, and
    /// whether it is a promotion). One event for a proposal under review; two for the console's
    /// "Confirm instead" and its Undo, which cast a demotion and a promotion as one act
    /// (design-system.md Section 29.2). The diff and the fold share this, so what a reviewer is shown
    /// is what the fold will do (P23: no merge without a diff that says so).
    ///
    /// `without` leaves one merged proposal out, as if its verdict had never been cast: the tiers an
    /// undo returns its targets to (design-system.md Section 29.3).
    pub(crate) fn gate_tiers(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
        extra: &[(&[String], TrustTier, bool)],
        without: Option<&str>,
    ) -> Result<HashMap<String, TrustTier>, StoreError> {
        Ok(self.gate_fold(workspace, cx, extra, without)?.0)
    }

    /// The gate proposals of this read and their merge verdicts: proposal id -> (targets, requested
    /// tier, is a promotion), and proposal id -> (its representative merge verdict's ordering hlc and
    /// observation id, the highest surface ceiling among its merge verdicts filed where it lives). The
    /// one reading of the verdicts' markers, shared by the fold that applies them and the answer that
    /// says what a promotion granted (design-system.md Section 29.20).
    #[allow(clippy::type_complexity)]
    pub(crate) fn gate_verdicts(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<
        (
            HashMap<String, (Vec<String>, TrustTier, bool)>,
            HashMap<String, (Hlc, String, TrustTier)>,
        ),
        StoreError,
    > {
        // proposal id -> (targets, requested tier, is a promotion); from opened gate-kind events.
        let mut opened: HashMap<String, (Vec<String>, TrustTier, bool)> = HashMap::new();
        // proposal id -> (representative merge verdict's ordering hlc and obs id, the highest
        // surface ceiling among the proposal's merge verdicts).
        let mut rep_merge: HashMap<String, (Hlc, String, TrustTier)> = HashMap::new();
        // A verdict counts only in its proposal's workspace (proposal-workflow.md I2), so under All
        // each merge verdict is held with the workspace it was filed in until the proposals are known.
        let mut homes: HashMap<String, String> = HashMap::new();
        let mut merges: Vec<(String, Hlc, String, TrustTier, String)> = Vec::new();
        for obs in self.log(workspace, cx)?.iter() {
            let okey = ordering_hlc(obs);
            for ev in &obs.assertions.proposal_events {
                let v: serde_json::Value =
                    serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                match ev.event {
                    ProposalEventKind::Opened => {
                        let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                        if !GATE_KINDS.contains(&kind) {
                            continue;
                        }
                        let Some(tier) =
                            v.get("tier").and_then(|x| x.as_str()).and_then(parse_tier)
                        else {
                            continue; // a gate proposal without a tier grants nothing
                        };
                        let targets: Vec<String> = v
                            .get("targets")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter().filter_map(|s| s.as_str().map(String::from)).collect()
                            })
                            .unwrap_or_default();
                        opened.insert(obs.id.clone(), (targets, tier, kind == "claim_promotion"));
                        homes.insert(obs.id.clone(), obs.workspace().to_string());
                    }
                    ProposalEventKind::Verdict => {
                        if v.get("decision").and_then(|x| x.as_str()) != Some("merge") {
                            continue;
                        }
                        // The surface marker rides each attestation's source_ref (engine-stamped,
                        // resolution.md R8). One decision cast twice - by an agent, then at the
                        // console - is one content address with two attestations (P3), and each
                        // is an act, so the ceiling is the highest over all of them.
                        let ceiling = obs
                            .provenance
                            .iter()
                            .map(|p| verdict_grant_ceiling(p.source_ref.as_deref()))
                            .max()
                            .unwrap_or_else(|| verdict_grant_ceiling(None));
                        merges.push((
                            ev.proposal.clone(),
                            okey.clone(),
                            obs.id.clone(),
                            ceiling,
                            obs.workspace().to_string(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        for (proposal, okey, id, ceiling, filed_in) in merges {
            if homes.get(&proposal) != Some(&filed_in) {
                continue;
            }
            rep_merge
                .entry(proposal)
                .and_modify(|cur| {
                    if (&okey, id.as_str()) < (&cur.0, cur.1.as_str()) {
                        cur.0 = okey.clone();
                        cur.1 = id.clone();
                    }
                    cur.2 = cur.2.max(ceiling);
                })
                .or_insert((okey, id, ceiling));
        }
        Ok((opened, rep_merge))
    }

    /// [`Engine::gate_tiers`], with each target's last moving event beside its tier: observation id
    /// -> (proposal id, its representative merge verdict's observation id, promotion). An event that
    /// left the tier where it was moves nothing.
    #[allow(clippy::type_complexity)]
    pub(crate) fn gate_fold(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
        extra: &[(&[String], TrustTier, bool)],
        without: Option<&str>,
    ) -> Result<(HashMap<String, TrustTier>, HashMap<String, (String, String, bool)>), StoreError>
    {
        let (opened, rep_merge) = self.gate_verdicts(workspace, cx)?;
        // The state fold is the single answer to "did this merge commit" (I13): consult it rather
        // than re-deriving blocking here, so the two can never drift apart again. Shares the
        // ReadCtx-cached log, so this adds no store enumeration.
        let states = if rep_merge.is_empty() {
            BTreeMap::new() // no merge verdicts anywhere - nothing to gate, skip the fold
        } else {
            self.fold_proposals(workspace, cx)?
        };
        // Per target, every committed gate event with its order key - resolution.md R5.
        let mut events: HashMap<String, Vec<(Hlc, String, TrustTier, bool)>> = HashMap::new();
        for (proposal_id, (targets, requested, promotion)) in &opened {
            if without == Some(proposal_id.as_str()) {
                continue;
            }
            let Some((verdict_hlc, _, ceiling)) = rep_merge.get(proposal_id) else {
                continue; // not merged - grants nothing (the gate, Principle 23)
            };
            if states.get(proposal_id).is_none_or(|p| p.state != "merged") {
                continue; // the fold says this merge did not commit (blocked) - no effect (I13)
            }
            // A promotion is capped by its surface; a demotion needs no ceiling (resolution.md
            // Section 6), so it lowers to the tier it asked for, whoever merged it.
            let granted = if *promotion { (*requested).min(*ceiling) } else { *requested };
            for target in targets {
                events.entry(target.clone()).or_default().push((
                    verdict_hlc.clone(),
                    proposal_id.clone(),
                    granted,
                    *promotion,
                ));
            }
        }
        for (targets, _, _) in extra {
            for target in targets.iter() {
                events.entry(target.clone()).or_default();
            }
        }
        if events.is_empty() {
            return Ok((HashMap::new(), HashMap::new()));
        }
        // Each target's base evaluation is where its events start. A target this log does not hold
        // has nothing to apply to; when it arrives, the next read folds it (the same monotone
        // direction as a blocked merge turning merged).
        let mut tiers: HashMap<String, TrustTier> = HashMap::new();
        let mut moved: HashMap<String, (String, String, bool)> = HashMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            let Some(list) = events.get_mut(&obs.id) else {
                continue;
            };
            list.sort_by(|a, b| (&a.0, a.1.as_str()).cmp(&(&b.0, b.1.as_str())));
            let mut tier = obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default();
            let apply = |tier: TrustTier, grant: TrustTier, promotion: bool| {
                if promotion {
                    tier.max(grant)
                } else {
                    tier.min(grant)
                }
            };
            for (_, proposal, grant, promotion) in list.iter() {
                let next = apply(tier, *grant, *promotion);
                if next != tier {
                    let verdict = rep_merge.get(proposal).map(|r| r.1.clone()).unwrap_or_default();
                    moved.insert(obs.id.clone(), (proposal.clone(), verdict, *promotion));
                }
                tier = next;
            }
            let mut touched = !list.is_empty();
            for (targets, grant, promotion) in extra {
                if targets.contains(&obs.id) {
                    tier = apply(tier, *grant, *promotion);
                    touched = true;
                }
            }
            if touched {
                tiers.insert(obs.id.clone(), tier);
            }
        }
        Ok((tiers, moved))
    }

    /// Per-canonical-entity belief fold over the observation log (resolution.md Sections 2-4):
    /// kind candidates (each at its observation's EFFECTIVE tier) and the node's representative
    /// effective tier. Shared by the graph projection, the curation report, and the entity view -
    /// a pure fold, so it converges continuously (F5) and needs no re-materialization to be current.
    pub(crate) fn belief_fold(
        &self,
        workspace: Option<&str>,
        fwd: &HashMap<String, String>,
        gates: &HashMap<String, TrustTier>,
        cx: &ReadCtx,
    ) -> Result<BeliefFold, StoreError> {
        let mut kinds: HashMap<String, Vec<BeliefCandidate>> = HashMap::new();
        let mut tiers: HashMap<String, TrustTier> = HashMap::new();
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
        for obs in self.log(workspace, cx)?.iter() {
            let ws = obs.workspace().to_string();
            let eff = effective_tier(obs, gates);
            let hlc = ordering_hlc(obs);
            let mut supports = |id: &String| {
                let t = tiers.entry(id.clone()).or_insert(TrustTier::Unverified);
                *t = (*t).max(eff);
            };
            for ea in &obs.assertions.entities {
                let id = canon(Entity::make_id(&ws, &ea.name));
                supports(&id);
                if let Some(k) = &ea.kind {
                    kinds.entry(id).or_default().push(BeliefCandidate {
                        value: k.clone(),
                        tier: eff,
                        hlc: hlc.clone(),
                        observation: obs.id.clone(),
                    });
                }
            }
            for ra in &obs.assertions.relations {
                for name in [&ra.from, &ra.to] {
                    let id = canon(Entity::make_id(&ws, name));
                    supports(&id);
                }
            }
        }
        Ok(BeliefFold { kinds, tiers })
    }

    /// Applies the policy to one entity's kind candidates: (winning kind + its asserting observation
    /// if any candidates exist, contested flag, non-winning competitor values). Competitors are one
    /// entry per distinct value at that value's highest effective tier, ordered (tier desc, value
    /// asc) - deterministic (P16). The winner's observation id is the mediation handle: confirming
    /// the current value means promoting that observation (resolution.md Section 4.2).
    pub(crate) fn resolve_kind(
        &self,
        candidates: Option<&Vec<BeliefCandidate>>,
    ) -> (Option<(String, String)>, bool, Vec<Competitor>) {
        let Some(cands) = candidates else {
            return (None, false, Vec::new());
        };
        let Some(choice) = self.policy.choose(cands) else {
            return (None, false, Vec::new());
        };
        let winner = &cands[choice.index];
        let mut best: BTreeMap<&str, (TrustTier, &str)> = BTreeMap::new();
        for c in cands {
            if c.value == winner.value {
                continue;
            }
            let e = best.entry(c.value.as_str()).or_insert((c.tier, c.observation.as_str()));
            // Highest tier per value; tie-break by smallest observation id (stable, P16).
            if (c.tier, std::cmp::Reverse(c.observation.as_str())) > (e.0, std::cmp::Reverse(e.1)) {
                *e = (c.tier, c.observation.as_str());
            }
        }
        let mut competitors: Vec<Competitor> = best
            .into_iter()
            .map(|(value, (tier, observation))| Competitor {
                value: value.to_string(),
                trust_tier: tier,
                observation: observation.to_string(),
            })
            .collect();
        competitors
            .sort_by(|a, b| b.trust_tier.cmp(&a.trust_tier).then_with(|| a.value.cmp(&b.value)));
        (
            Some((winner.value.clone(), winner.observation.clone())),
            choice.contested,
            competitors,
        )
    }
}

/// The alias set for an entity (resolution-identity.md Section 2, IR1): the distinct asserted
/// spellings minus the representative (`canonical`), ordered by (first-asserting ordering-HLC,
/// spelling). A deterministic set union - the same candidate set yields the same aliases on any
/// node (P16), and a spelling that lost the representative choice is never dropped (Principle 3).
/// Collapses belief candidates to one row per distinct value for the explain surface: each value at
/// its highest effective tier, tie-broken to the smallest observation id (the same representative
/// choice as [`Engine::resolve_kind`]'s competitors - P16). The row equal to `winner` is tagged
/// "winner"; every other value gets `loser_role` ("alias" for names, "competitor" for kinds).
/// Ordered winner-first, then (tier desc, value asc) - a stable, arrival-order-free ranking.
pub(crate) fn candidate_rows(
    cands: &[BeliefCandidate],
    winner: &str,
    loser_role: &'static str,
    bases: &HashMap<String, TrustTier>,
) -> Vec<CandidateRow> {
    // value -> (highest tier, representative observation id, that candidate's hlc).
    let mut best: BTreeMap<&str, (TrustTier, &str, &Hlc)> = BTreeMap::new();
    for c in cands {
        let e = best.entry(c.value.as_str()).or_insert((c.tier, c.observation.as_str(), &c.hlc));
        // Highest tier per value; tie-break by smallest observation id (stable, P16).
        if (c.tier, std::cmp::Reverse(c.observation.as_str())) > (e.0, std::cmp::Reverse(e.1)) {
            *e = (c.tier, c.observation.as_str(), &c.hlc);
        }
    }
    // value -> every asserting observation at its effective tier (one observation can name a value
    // twice, as an entity and at a relation's end - it is one source).
    let mut sources: BTreeMap<&str, BTreeMap<&str, TrustTier>> = BTreeMap::new();
    for c in cands {
        let e = sources
            .entry(c.value.as_str())
            .or_default()
            .entry(c.observation.as_str())
            .or_insert(c.tier);
        *e = (*e).max(c.tier);
    }
    let mut rows: Vec<CandidateRow> = best
        .into_iter()
        .map(|(value, (tier, obs, hlc))| {
            let mut observations: Vec<CandidateSource> = sources
                .get(value)
                .map(|m| {
                    m.iter()
                        .map(|(o, t)| CandidateSource {
                            observation: o.to_string(),
                            trust_tier: *t,
                            base_tier: bases.get(*o).copied().unwrap_or(*t),
                        })
                        .collect()
                })
                .unwrap_or_default();
            observations.sort_by(|a, b| {
                b.trust_tier.cmp(&a.trust_tier).then_with(|| a.observation.cmp(&b.observation))
            });
            CandidateRow {
                value: value.to_string(),
                role: if value == winner { "winner" } else { loser_role },
                trust_tier: tier,
                base_tier: bases.get(obs).copied().unwrap_or(tier),
                hlc: hlc.clone(),
                observation: obs.to_string(),
                observations,
            }
        })
        .collect();
    // Winner first, then highest tier, then value - deterministic.
    rows.sort_by(|a, b| {
        (a.role != "winner")
            .cmp(&(b.role != "winner"))
            .then_with(|| b.trust_tier.cmp(&a.trust_tier))
            .then_with(|| a.value.cmp(&b.value))
    });
    rows
}

pub(crate) fn alias_set(cands: &[BeliefCandidate], canonical: &str) -> Vec<String> {
    // value -> the earliest ordering-HLC that asserted it (first-asserting).
    let mut first: BTreeMap<&str, &Hlc> = BTreeMap::new();
    for c in cands {
        if c.value == canonical {
            continue;
        }
        first
            .entry(c.value.as_str())
            .and_modify(|h| {
                if &c.hlc < *h {
                    *h = &c.hlc;
                }
            })
            .or_insert(&c.hlc);
    }
    let mut ordered: Vec<(&str, &Hlc)> = first.into_iter().collect();
    ordered.sort_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)));
    ordered.into_iter().map(|(v, _)| v.to_string()).collect()
}
