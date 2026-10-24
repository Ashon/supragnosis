//! The belief diffs of merge, split and gate proposals.

use super::*;

/// Which way an endpoint travels in a merge/split preview: a merge moves it onto the canonical id,
/// a split moves it back off. The only asymmetry between the two previews.
#[derive(Clone, Copy)]
enum Rewire {
    OntoAnchor,
    OffAnchor,
}

/// What a merge or split proposes, in the terms the preview needs: the forwarding map as it would
/// stand, the ids the proposal relocates, the canonical id they travel to or from, and which way.
/// One value rather than four parameters because they are one thing - a description of the change -
/// and passing them separately invites a call site that pairs the wrong anchor with the wrong set.
struct Relocation<'a> {
    after_fwd: &'a HashMap<String, String>,
    moving: &'a HashSet<String>,
    anchor: &'a str,
    rewire: Rewire,
}

/// A preview that cannot be computed, and why. An empty diff would read as "changes nothing", which
/// is the opposite of the truth (Principle 5).
pub(crate) fn uncomputable_diff(note: &str) -> BeliefDiff {
    BeliefDiff {
        computable: false,
        note: Some(note.into()),
        tier_changes: Vec::new(),
        overturned: Vec::new(),
        rewired: Vec::new(),
        reach: Vec::new(),
    }
}

impl Engine {
    /// The diff for an `entity_split`: the mirror of [`Self::merge_diff`]. The "after" map is the
    /// forwarding fold run with this proposal's target treated as reversed, so it is the same
    /// computation the verdict performs rather than a prediction of it (unmerge.md Section 9).
    ///
    /// What a reviewer is owed here (P23's informative checks): which entities separate, which
    /// relation endpoints move back, and any belief the separation overturns on an entity that is
    /// NOT one of the separating ones - that last being the surprise, since the separated entities
    /// regaining their own beliefs is the proposal rather than a consequence of it.
    fn split_diff(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        cx: &ReadCtx,
    ) -> Result<BeliefDiff, StoreError> {
        let Some(target) = view.targets.first().cloned() else {
            return Ok(uncomputable_diff("an entity_split names no proposal to reverse"));
        };
        let props = self.fold_proposals(workspace, cx)?;
        let Some(merge) = props.get(&target) else {
            return Ok(uncomputable_diff(
                "the named proposal is not in the local log, so what it did cannot be read - under \
                 incomplete sync this is arrival order rather than a malformed split",
            ));
        };
        if merge.kind != "entity_merge" || !carries_merge_verdict(merge) {
            return Ok(uncomputable_diff(
                "the named proposal is not an entity_merge that has been decided, so it forwards \
                 nothing for this to take back",
            ));
        }
        let Some(into) = merge.into.clone() else {
            return Ok(uncomputable_diff(
                "the named entity_merge has no `into`, so it folded nothing",
            ));
        };

        let base_fwd = self.merge_forwarding(workspace, cx)?;
        let after_fwd = self.forwarding_less(workspace, &HashSet::from([target]), cx)?;
        // The ids that stop forwarding onto `into` when this merge is taken back. Read from the
        // difference between the two maps rather than from the merge's targets, because a target may
        // still be forwarded by ANOTHER merge and would then not separate at all - saying it would
        // overstate the blast radius in the one direction that matters.
        let separating: HashSet<String> =
            base_fwd.keys().filter(|id| !after_fwd.contains_key(*id)).cloned().collect();

        self.forwarding_diff(
            workspace,
            &base_fwd,
            Relocation {
                after_fwd: &after_fwd,
                moving: &separating,
                anchor: &into,
                rewire: Rewire::OffAnchor,
            },
            cx,
        )
    }
    /// The half of a merge/split preview that is the same act in both directions: fold beliefs under
    /// the forwarding map as it stands and as it would stand, and report what differs.
    ///
    /// `moving` is the ids the proposal directly relocates. They are skipped in `overturned` because
    /// a folded target losing its own belief (or a separated one regaining it) IS the proposal, not a
    /// surprise; and they are what `rewired` matches on. `anchor` is the canonical id those endpoints
    /// travel to or from.
    ///
    /// Extracted because the two previews had drifted apart into 73 identical lines, and identical
    /// lines are how a fix to one becomes a divergence from the other - in exactly the code whose
    /// whole claim is that the preview and the verdict cannot disagree.
    fn forwarding_diff(
        &self,
        workspace: Option<&str>,
        base_fwd: &HashMap<String, String>,
        to: Relocation<'_>,
        cx: &ReadCtx,
    ) -> Result<BeliefDiff, StoreError> {
        let Relocation { after_fwd, moving, anchor, rewire } = to;
        let gates = self.gate_grants(workspace, cx)?;
        let before = self.belief_fold(workspace, base_fwd, &gates, cx)?;
        let after = self.belief_fold(workspace, after_fwd, &gates, cx)?;

        let name_of = |id: &str| -> Result<String, StoreError> {
            Ok(self
                .store
                .get_entity(id)?
                .map(|e| e.canonical_name)
                .unwrap_or_else(|| id.to_string()))
        };

        let mut ids: BTreeSet<&String> = before.kinds.keys().collect();
        ids.extend(after.kinds.keys());
        let mut overturned = Vec::new();
        for id in ids {
            if moving.contains(id) {
                continue;
            }
            let (wb, cb, _) = self.resolve_kind(before.kinds.get(id));
            let (wa, ca, _) = self.resolve_kind(after.kinds.get(id));
            let (vb, va) = (wb.map(|(k, _)| k), wa.map(|(k, _)| k));
            if vb == va && cb == ca {
                continue;
            }
            overturned.push(BeliefChange {
                entity: id.clone(),
                name: name_of(id)?,
                field: "kind".into(),
                from: vb,
                to: va,
                contested_before: cb,
                contested_after: ca,
            });
        }

        // Reference rewiring (proposal-workflow.md Section 5, item 5): every edge with an endpoint
        // the proposal relocates.
        let canon = |id: &str| after_fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let anchor_name = name_of(anchor)?;
        let mut rewired = Vec::new();
        for r in self.store.all_relations(workspace)? {
            let (mf, mt) = (moving.contains(&r.from), moving.contains(&r.to));
            if !mf && !mt {
                continue;
            }
            let endpoint = name_of(if mf { &r.from } else { &r.to })?;
            let other = name_of(if mf { &r.to } else { &r.from })?;
            let (from_name, to_name) = match rewire {
                Rewire::OntoAnchor => (endpoint, anchor_name.clone()),
                Rewire::OffAnchor => (anchor_name.clone(), endpoint),
            };
            rewired.push(RelationRewire {
                relation: r.id.clone(),
                kind: r.kind.clone(),
                from_name,
                to_name,
                other_name: other,
                becomes_self_loop: canon(&r.from) == canon(&r.to),
            });
        }
        rewired.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.other_name.cmp(&b.other_name))
                .then_with(|| a.relation.cmp(&b.relation))
        });
        overturned.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.entity.cmp(&b.entity)));
        Ok(BeliefDiff {
            computable: true,
            note: None,
            tier_changes: Vec::new(),
            overturned,
            rewired,
            reach: Vec::new(),
        })
    }

    /// The diff for an entity_merge: the same two materializations as a gate proposal, except the
    /// thing that differs is the FORWARDING map rather than the gate grants. Merge forwarding is a
    /// read-time overlay applied by `belief_fold`, so adding this proposal's target -> into edges and
    /// folding again is exactly `materialize(canon, base + merge effects)` - no separate prediction of
    /// the outcome, and therefore nothing that can disagree with what the verdict actually does.
    fn merge_diff(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        cx: &ReadCtx,
    ) -> Result<BeliefDiff, StoreError> {
        let Some(into) = view.into.clone() else {
            return Ok(uncomputable_diff(
                "an entity_merge without `into` names no canonical id to fold onto",
            ));
        };
        let base_fwd = self.merge_forwarding(workspace, cx)?;
        let folded: HashSet<String> =
            view.targets.iter().filter(|t| **t != into).cloned().collect();
        let mut after_fwd = base_fwd.clone();
        for t in &folded {
            after_fwd.insert(t.clone(), into.clone());
        }
        self.forwarding_diff(
            workspace,
            &base_fwd,
            Relocation {
                after_fwd: &after_fwd,
                moving: &folded,
                anchor: &into,
                rewire: Rewire::OntoAnchor,
            },
            cx,
        )
    }
    pub(crate) fn belief_diff(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        cx: &ReadCtx,
    ) -> Result<BeliefDiff, StoreError> {
        if view.kind == "entity_merge" {
            return self.merge_diff(workspace, view, cx);
        }
        if view.kind == "entity_split" {
            return self.split_diff(workspace, view, cx);
        }
        // A type change or a recall - or a kind this build does not know, which only a foreign log holds.
        if !GATE_KINDS.contains(&view.kind.as_str()) {
            return Ok(BeliefDiff {
                computable: false,
                note: Some(format!(
                    "{} has no commit effect yet, so it cannot be merged until it has one, and there is no difference to compute - an empty diff here would mean 'not computable', never 'changes nothing' (architecture.md Section 14)",
                    view.kind
                )),
                tier_changes: Vec::new(),
                overturned: Vec::new(),
                rewired: Vec::new(),
                reach: Vec::new(),
            });
        }
        let Some(tier) = view.tier else {
            return Ok(BeliefDiff {
                computable: false,
                note: Some("a gate proposal without a requested tier grants nothing".into()),
                tier_changes: Vec::new(),
                overturned: Vec::new(),
                rewired: Vec::new(),
                reach: Vec::new(),
            });
        };

        // The one difference between the two materializations: this proposal merged now, from the
        // console - the Review panel is where the diff is read - so its grant is what that surface
        // allows, applied after every merged event as a new verdict would be (R5).
        let grant = tier.min(verdict_grant_ceiling(Some(VERDICT_SURFACE_CONSOLE)));
        let promotion = view.kind == "claim_promotion";
        self.gate_diff(workspace, &[(view.targets.as_slice(), grant, promotion)], cx)
    }

    /// The belief diff of gate events applied in order after every merged one (proposal-workflow.md
    /// Section 5): each target's tier change, the beliefs overturned (items 2-3) and `reach`
    /// (item 1) - what each moved source said and the rings it moves. Shared by a proposal under
    /// review and the console's settle preview, so the two cannot describe different acts.
    pub(crate) fn gate_diff(
        &self,
        workspace: Option<&str>,
        events: &[(&[String], TrustTier, bool)],
        cx: &ReadCtx,
    ) -> Result<BeliefDiff, StoreError> {
        let fwd = self.merge_forwarding(workspace, cx)?;
        let before_gates = self.gate_grants(workspace, cx)?;
        let after_gates = self.gate_tiers(workspace, cx, events, None)?;

        let mut targets: Vec<&String> = events.iter().flat_map(|(t, _, _)| t.iter()).collect();
        targets.sort();
        targets.dedup();
        let mut tier_changes = Vec::new();
        let mut moved = Vec::new();
        for t in targets {
            if let Some(obs) = self.store.get_observation(t)? {
                let (from, to) =
                    (effective_tier(&obs, &before_gates), effective_tier(&obs, &after_gates));
                if from != to {
                    tier_changes.push(TierChange { observation: t.clone(), from, to });
                    moved.push((obs, from, to));
                }
            }
        }

        let before = self.belief_fold(workspace, &fwd, &before_gates, cx)?;
        let after = self.belief_fold(workspace, &fwd, &after_gates, cx)?;
        let mut ids: BTreeSet<&String> = before.kinds.keys().collect();
        ids.extend(after.kinds.keys());

        let mut overturned = Vec::new();
        for id in ids {
            let (wb, cb, _) = self.resolve_kind(before.kinds.get(id));
            let (wa, ca, _) = self.resolve_kind(after.kinds.get(id));
            let (vb, va) = (wb.map(|(k, _)| k), wa.map(|(k, _)| k));
            if vb == va && cb == ca {
                continue;
            }
            let name = self
                .store
                .get_entity(id)?
                .map(|e| e.canonical_name)
                .unwrap_or_else(|| id.clone());
            overturned.push(BeliefChange {
                entity: id.clone(),
                name,
                field: "kind".into(),
                from: vb,
                to: va,
                contested_before: cb,
                contested_after: ca,
            });
        }

        // Item 1: every entity a moved source supports, with its ring (the tier of its best support,
        // the same fold the graph reads) before and after.
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
        let mut reach = Vec::new();
        for (obs, from, to) in moved {
            let ws = obs.workspace().to_string();
            let named = obs
                .assertions
                .entities
                .iter()
                .map(|e| &e.name)
                .chain(obs.assertions.relations.iter().flat_map(|r| [&r.from, &r.to]));
            let mut seen = BTreeSet::new();
            let mut entities = Vec::new();
            for name in named {
                let id = canon(Entity::make_id(&ws, name));
                if !seen.insert(id.clone()) {
                    continue;
                }
                let display = self
                    .store
                    .get_entity(&id)?
                    .map(|e| e.canonical_name)
                    .unwrap_or_else(|| name.trim().to_string());
                entities.push(EntityReach {
                    from: before.tiers.get(&id).copied().unwrap_or_default(),
                    to: after.tiers.get(&id).copied().unwrap_or_default(),
                    entity: id,
                    name: display,
                });
            }
            entities.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.entity.cmp(&b.entity)));
            let cut = obs.content.chars().count() > REACH_SAID_CHARS;
            reach.push(ReachRow {
                observation: obs.id.clone(),
                said: obs.content.chars().take(REACH_SAID_CHARS).collect(),
                cut,
                from,
                to,
                entities,
                relations: obs
                    .assertions
                    .relations
                    .iter()
                    .map(|r| StatedRelation {
                        from: r.from.clone(),
                        kind: r.kind.clone(),
                        to: r.to.clone(),
                    })
                    .collect(),
                type_defs: obs.assertions.type_defs.iter().map(|d| d.name.clone()).collect(),
            });
        }
        // Deterministic order (Principle 16): the diff is a read surface like any other.
        overturned.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.entity.cmp(&b.entity)));
        tier_changes.sort_by(|a, b| a.observation.cmp(&b.observation));
        reach.sort_by(|a, b| a.observation.cmp(&b.observation));
        Ok(BeliefDiff {
            computable: true,
            note: None,
            tier_changes,
            overturned,
            rewired: Vec::new(),
            reach,
        })
    }
}
