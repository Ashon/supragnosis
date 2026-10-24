//! Merge forwarding: decided and reversed merges, the forwarding map and merge cycles.

use super::*;

/// A detected cycle before name resolution: (sorted member entity ids, forming proposal ids).
type CycleSet = (Vec<String>, Vec<String>);

impl Engine {
    /// The `entity_merge` proposals a verdict has already decided - what an `entity_split` may name
    /// (unmerge.md Section 9). For callers OUTSIDE [`Self::fold_proposals`]; the fold builds the same
    /// set from its own tally, because it cannot ask itself.
    pub(crate) fn decided_merges(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<String>, StoreError> {
        Ok(self
            .fold_proposals(workspace, cx)?
            .values()
            .filter(|p| p.kind == "entity_merge" && carries_merge_verdict(p))
            .map(|p| p.id.clone())
            .collect())
    }

    /// The `entity_merge` proposal ids that a merged `entity_split` has reversed (unmerge.md
    /// Section 3). Shared by [`Self::merge_forwarding`] and [`Self::merge_cycle_sets`]: a merge that
    /// no longer forwards must also stop contributing a cycle edge, and two separate notions of
    /// "reversed" would disagree in exactly the case that matters.
    ///
    /// A pure fold of the log like every other proposal state, so nodes with equal logs reverse the
    /// same merges (P16). Note this is where forwarding stops being monotonic in the naive sense -
    /// appending an event removes an edge - which unmerge.md Section 7 argues is supersede rather
    /// than an exception: every verdict stays, and the map is a function of all of them.
    pub(crate) fn reversed_merges(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<String>, StoreError> {
        let mut out = HashSet::new();
        for p in self.fold_proposals(workspace, cx)?.values() {
            if p.kind == "entity_split" && p.state == "merged" {
                out.extend(p.targets.iter().cloned());
            }
        }
        Ok(out)
    }

    /// Resolved id-forwarding from ACCEPTED entity-merge proposals (Principle 14/15): each merged-away
    /// target id -> its canonical (`into`) id, transitively resolved to the root. Projections apply this to
    /// collapse merged duplicates while the log keeps both (Principle 3 - un-merge is a new proposal). Pure
    /// deterministic function of the log (Principle 16).
    pub(crate) fn merge_forwarding(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashMap<String, String>, StoreError> {
        self.forwarding_less(workspace, &HashSet::new(), cx)
    }

    /// [`Self::merge_forwarding`] with additional merges treated as reversed - what the map WOULD be
    /// if the named splits had merged. The preview of an `entity_split` is computed by running this
    /// rather than by predicting an outcome, so the diff a reviewer reads and the effect the verdict
    /// has cannot disagree (the same argument `merge_diff` makes for the other direction).
    pub(crate) fn forwarding_less(
        &self,
        workspace: Option<&str>,
        also_reversed: &HashSet<String>,
        cx: &ReadCtx,
    ) -> Result<HashMap<String, String>, StoreError> {
        let props = self.fold_proposals(workspace, cx)?;
        let reversed = self.reversed_merges(workspace, cx)?;
        let mut fwd: HashMap<String, String> = HashMap::new();
        for p in props.values() {
            if p.kind == "entity_merge"
                && p.state == "merged"
                && !reversed.contains(&p.id)
                && !also_reversed.contains(&p.id)
            {
                if let Some(into) = &p.into {
                    for t in &p.targets {
                        if t != into {
                            fwd.insert(t.clone(), into.clone());
                        }
                    }
                }
            }
        }
        // Resolve transitively (a->b, b->c => a->c) with a hop cap as a cycle guard.
        let mut resolved: HashMap<String, String> = HashMap::new();
        for k in fwd.keys() {
            let mut cur = k.clone();
            let mut hops = 0usize;
            while let Some(n) = fwd.get(&cur) {
                if n == &cur || hops > fwd.len() {
                    break;
                }
                cur = n.clone();
                hops += 1;
            }
            resolved.insert(k.clone(), cur);
        }
        Ok(resolved)
    }

    /// Detects contradictory accepted-merge cycles (Principle 6, resolution.md Section 4.2): raw
    /// (pre-transitive) forwarding edges - target -> into per merged entity_merge - that lead back
    /// into themselves. Returns deduped (member ids, proposal ids) pairs, deterministically ordered
    /// (BTreeMap keying, P16). The projection still resolves such cycles by hop-capped parity; this
    /// signal is what makes the contradiction visible instead of silent.
    pub(crate) fn merge_cycle_sets(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<Vec<CycleSet>, StoreError> {
        let props = self.fold_proposals(workspace, cx)?;
        let reversed = self.reversed_merges(workspace, cx)?;
        // target -> (into, proposal id): the raw merge edges before transitive resolution.
        let mut edge: BTreeMap<String, (String, String)> = BTreeMap::new();
        for p in props.values() {
            if p.kind == "entity_merge" && p.state == "merged" && !reversed.contains(&p.id) {
                if let Some(into) = &p.into {
                    for t in &p.targets {
                        if t != into {
                            edge.insert(t.clone(), (into.clone(), p.id.clone()));
                        }
                    }
                }
            }
        }
        let mut cycles: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
        for start in edge.keys() {
            let mut path: Vec<&str> = vec![start.as_str()];
            let mut cur = start.as_str();
            while let Some((next, _)) = edge.get(cur) {
                if let Some(pos) = path.iter().position(|x| *x == next.as_str()) {
                    // The cycle is the path suffix from the first revisit point.
                    let mut members: Vec<String> =
                        path[pos..].iter().map(|s| s.to_string()).collect();
                    let proposals: BTreeSet<String> = members
                        .iter()
                        .filter_map(|m| edge.get(m).map(|(_, pid)| pid.clone()))
                        .collect();
                    members.sort();
                    cycles.entry(members).or_default().extend(proposals);
                    break;
                }
                if path.len() > edge.len() {
                    break;
                }
                path.push(next.as_str());
                cur = next.as_str();
            }
        }
        Ok(cycles.into_iter().map(|(m, p)| (m, p.into_iter().collect())).collect())
    }
}
