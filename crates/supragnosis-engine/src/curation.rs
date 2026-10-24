//! The curation report: the merge band, the name-variant ladder and the pairs every candidate
//! generator holds back.

use super::*;

/// Grab-bag detection threshold: a hyperedge with this many members is flagged as a loose co-occurrence
/// context (a split/refine candidate, Principle 11). Tunable.
const CURATION_GRAB_BAG_MIN: usize = 10;

/// The conservative merge band floor (resolution-identity.md Section 3): embedding cosine at or
/// above this makes a distinct-name entity pair a merge candidate. Deliberately high - a candidate
/// is a hypothesis for review, and the gate (not the score) commits (Principle 15/19).
const SIM_CANDIDATE: f32 = 0.85;
/// Nearest entities considered per node when scanning the merge band (cost bound - the retrieval aid
/// returns a ranked list; only the closest few are plausible merge candidates).
const MERGE_BAND_K: usize = 8;

/// An unordered id pair as a canonical (min, max) tuple - so a suggestion for (a, b) and (b, a) is
/// one entry, and an open-proposal exclusion matches regardless of the order the targets were given.
/// The `limit` nearest entities to `subject` by name-embedding cosine, computed over the pool the
/// caller already holds.
///
/// This is what `KnowledgeStore::search_semantic_entities` returns, in the same order (score
/// descending, id ascending - Principle 16: a tie may not resolve by iteration order), for the same
/// reason it is not called: the merge band is handed every entity of the workspace, vectors
/// included, and was asking the store for them again once per entity. That is N queries to rank
/// data already in hand.
///
/// Computing it here also makes the band say the same thing on every adapter. Both stores in the
/// tree rank exhaustively, but an adapter is free to answer with an approximate index instead, and
/// then the same log would produce different merge candidates depending on which store was
/// underneath - the shape of divergence the traverse parity conditions exist to catch.
fn nearest_by_embedding<'e>(
    subject: &Entity,
    pool: &'e [Entity],
    limit: usize,
) -> Vec<(&'e Entity, f32)> {
    let Some(q) = subject.embedding.as_deref() else {
        return Vec::new();
    };
    let mut hits: Vec<(&Entity, f32)> = pool
        .iter()
        .filter_map(|e| Some((e, supragnosis_core::cosine_similarity(q, e.embedding.as_deref()?))))
        .collect();
    hits.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    hits.truncate(limit);
    hits
}

fn unordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

impl Engine {
    /// Every pair a candidate generator must not offer: in flight under an open or Blocked merge
    /// ([`Self::open_merge_pairs`]), pulled apart by a merged split ([`Self::split_pairs`]), or held
    /// back by the person's no ([`Self::person_no_pairs`], design-system.md Section 29.23).
    ///
    /// One set rather than two parameters, because both generators have to suppress both and a
    /// generator that learned about only one would re-offer what the other withheld. Neither of them
    /// knows what a split is; they know what they may not suggest.
    fn suppressed_pairs(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let mut out = self.open_merge_pairs(workspace, cx)?;
        out.extend(self.split_pairs(workspace, cx)?);
        out.extend(self.person_no_pairs(workspace, cx)?);
        Ok(out)
    }

    /// Entity pairs the person's no still holds back (design-system.md Section 29.23, dreaming.md
    /// Section 5.5): every pair a merge names whose reject carries the console's mark, until an
    /// observation that is not a proposal event, authored in a later millisecond than that reject's
    /// console attestation, asserts one of the merge's targets as named. A console reject counts
    /// whichever node cast it, so equal logs hold back equal pairs (P16); an agent's reject and a
    /// withdrawal hold nothing.
    ///
    /// Only the walls are compared. A stamp is made at export (sync-correctness.md Section 3), so the
    /// counter and node that break a tie say which content id or node id sorts first, not which act
    /// came first: read whole, they would let the observation that named the pair lift the no, and
    /// only once the node that cast it stamps.
    fn person_no_pairs(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let props = self.fold_proposals(workspace, cx)?;
        let log = self.log(workspace, cx)?;
        let attested = |p: &Provenance| {
            p.sync
                .as_ref()
                .map(|s| s.hlc.clone())
                .unwrap_or_else(|| Hlc::legacy(p.observed_at))
        };
        // Each merge's earliest console reject.
        let mut no: HashMap<&str, Hlc> = HashMap::new();
        for obs in log.iter() {
            for ev in &obs.assertions.proposal_events {
                if ev.event != ProposalEventKind::Verdict
                    || !props.get(&ev.proposal).is_some_and(|p| p.kind == "entity_merge")
                {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                if v.get("decision").and_then(|d| d.as_str()) != Some("reject") {
                    continue;
                }
                let at = obs
                    .provenance
                    .iter()
                    .filter(|p| p.source_ref.as_deref() == Some(VERDICT_SURFACE_CONSOLE))
                    .map(attested)
                    .min();
                if let Some(at) = at {
                    let e = no.entry(ev.proposal.as_str()).or_insert_with(|| at.clone());
                    if at < *e {
                        *e = at;
                    }
                }
            }
        }
        let mut out = HashSet::new();
        if no.is_empty() {
            return Ok(out);
        }
        // When each entity was last said by knowledge rather than by a proposal event.
        let mut said: HashMap<String, Hlc> = HashMap::new();
        for obs in log.iter().filter(|o| o.assertions.proposal_events.is_empty()) {
            let (ws, at) = (obs.workspace(), ordering_hlc(obs));
            let names = obs.assertions.entities.iter().map(|e| e.name.as_str()).chain(
                obs.assertions.relations.iter().flat_map(|r| [r.from.as_str(), r.to.as_str()]),
            );
            for name in names {
                let e = said.entry(Entity::make_id(ws, name)).or_insert_with(|| at.clone());
                if at > *e {
                    *e = at.clone();
                }
            }
        }
        for (proposal, at) in no {
            let p = &props[proposal];
            if p.targets.iter().any(|t| said.get(t).is_some_and(|h| h.wall > at.wall)) {
                continue; // the knowledge has changed since the person's no
            }
            for i in 0..p.targets.len() {
                for j in (i + 1)..p.targets.len() {
                    out.insert(unordered_pair(&p.targets[i], &p.targets[j]));
                }
            }
        }
        Ok(out)
    }

    /// Entity pairs a merged `entity_split` has pulled apart. Every candidate generator suppresses
    /// these, for the reason unmerge.md Section 5 exists: "already merged" is derived from the
    /// forwarding map, so the moment a split removes the edge the pair becomes a fresh
    /// high-similarity candidate and the console proposes re-merging what a human just separated -
    /// every time they look at it, forever.
    ///
    /// **The suppression is of the suggestion, never of the possibility.** Opening an `entity_merge`
    /// on split entities by hand stays allowed and needs no special case; this only stops the machine
    /// from asking. That is the line the band already draws - a generator proposes, a verdict commits
    /// (P19, IR2).
    ///
    /// Derived from the log like [`Self::open_merge_pairs`] beside it, so nodes with equal logs
    /// suppress equally (P16), and living next to it so the two cannot drift on what "do not offer
    /// this again" means.
    fn split_pairs(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let props = self.fold_proposals(workspace, cx)?;
        let reversed: HashSet<&String> = props
            .values()
            .filter(|p| p.kind == "entity_split" && p.state == "merged")
            .flat_map(|p| p.targets.iter())
            .collect();
        let mut out: HashSet<(String, String)> = HashSet::new();
        for p in props.values() {
            if p.kind == "entity_merge" && reversed.contains(&p.id) {
                for i in 0..p.targets.len() {
                    for j in (i + 1)..p.targets.len() {
                        out.insert(unordered_pair(&p.targets[i], &p.targets[j]));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Entity pairs already carried by an OPEN or Blocked entity_merge proposal. Every candidate
    /// generator suppresses these: a pair in flight is not a fresh candidate, and re-offering it
    /// invites a second proposal for a merge already awaiting a verdict. Shared by the merge band and the
    /// name-variant ladder so the two cannot drift apart on what "already proposed" means.
    fn open_merge_pairs(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let mut open_pairs: HashSet<(String, String)> = HashSet::new();
        for p in self.fold_proposals(workspace, cx)?.values() {
            if p.kind == "entity_merge" && matches!(p.state.as_str(), "open" | "blocked") {
                for i in 0..p.targets.len() {
                    for j in (i + 1)..p.targets.len() {
                        open_pairs.insert(unordered_pair(&p.targets[i], &p.targets[j]));
                    }
                }
            }
        }
        Ok(open_pairs)
    }

    /// The conservative merge band (resolution-identity.md Section 3, Principle 15): distinct-name
    /// entity pairs whose name embeddings are near (>= [`SIM_CANDIDATE`]) - candidates the substrate
    /// proposes so identity resolution is not the operator's manual job. A node-local retrieval aid
    /// (P16-exempt, P19): the similarity is not part of the converging graph, and a candidate commits
    /// NOTHING (IR2/I18) - acting on it opens an entity_merge through the gate. Excludes same-id,
    /// already-merged (either side forwarded away), and the pairs [`Self::suppressed_pairs`] holds
    /// back, which it counts (`held_back`, design-system.md Section 29.27). Without an embedder there
    /// are no candidates (degrade, P19). Within-node reproducible order (similarity desc, then ids); it
    /// need not converge across nodes (Section 3).
    fn merge_band(
        &self,
        workspace: Option<&str>,
        all_entities: &[Entity],
        fwd: &HashMap<String, String>,
        relations: &[Relation],
        cx: &ReadCtx,
    ) -> Result<(Vec<MergeSuggestion>, MergeBandCoverage), StoreError> {
        if self.embedder.is_none() {
            // Report the unavailability instead of returning a bare empty list: the caller cannot
            // otherwise tell "no near-name pairs" from "this signal does not run here" (Principle 5).
            return Ok((
                Vec::new(),
                MergeBandCoverage { available: false, embedded: 0, examined: 0, held_back: 0 },
            ));
        }
        // Pairs already under an open entity_merge - not re-surfaced (they are in flight).
        let open_pairs = self.suppressed_pairs(workspace, cx)?;
        // Canonicalized undirected adjacency for the shared-neighbor count (structural corroboration).
        let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        for r in relations {
            let (f, t) = (canon(&r.from), canon(&r.to));
            if f == t {
                continue;
            }
            adj.entry(f.clone()).or_default().insert(t.clone());
            adj.entry(t).or_default().insert(f);
        }
        let name_by_id: HashMap<&str, &str> = all_entities
            .iter()
            .map(|e| (e.id.as_str(), e.canonical_name.as_str()))
            .collect();

        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut out: Vec<MergeSuggestion> = Vec::new();
        let mut held_back = 0usize;
        // Coverage, counted on the same pass: an entity projected before an embedder was configured
        // carries no vector and is silently skipped, which would otherwise make an under-covered run
        // look identical to an exhaustive one that found nothing.
        let (mut embedded, mut examined) = (0usize, 0usize);
        for e in all_entities {
            if fwd.contains_key(&e.id) {
                continue; // e was merged away - not a live candidate
            }
            examined += 1;
            if e.embedding.is_none() {
                continue;
            }
            embedded += 1;
            for (other, score) in nearest_by_embedding(e, all_entities, MERGE_BAND_K) {
                if other.id == e.id || score < SIM_CANDIDATE || fwd.contains_key(&other.id) {
                    continue;
                }
                // Same reason as the ladder: with workspace: None the pool spans workspaces, and a
                // pair straddling two of them has nowhere coherent to file its proposal.
                if entity_workspace(other) != entity_workspace(e) {
                    continue;
                }
                let pair = unordered_pair(&e.id, &other.id);
                if !seen.insert(pair.clone()) {
                    continue; // reached from its other end already
                }
                if open_pairs.contains(&pair) {
                    held_back += 1; // answered already: a waiting merge, a split or the person's no
                    continue;
                }
                let shared = match (adj.get(&pair.0), adj.get(&pair.1)) {
                    (Some(x), Some(y)) => x.intersection(y).count(),
                    _ => 0,
                };
                out.push(MergeSuggestion {
                    a_name: name_by_id.get(pair.0.as_str()).copied().unwrap_or("").to_string(),
                    b_name: name_by_id.get(pair.1.as_str()).copied().unwrap_or("").to_string(),
                    a: pair.0,
                    b: pair.1,
                    similarity: score,
                    shared_neighbors: shared,
                });
            }
        }
        out.sort_by(|x, y| {
            y.similarity
                .partial_cmp(&x.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| x.a.cmp(&y.a))
                .then_with(|| x.b.cmp(&y.b))
        });
        Ok((out, MergeBandCoverage { available: true, embedded, examined, held_back }))
    }

    /// Read-only curation signals over the workspace (Principle 7 "generate, do not commit"): merge
    /// candidates (name-collision), grab-bag hyperedges, and orphan entities. A pure deterministic read
    /// (Principle 1/16) - it computes nothing into the canon, so no gate is involved. The commit side
    /// (proposals/verdicts) is a separate, gated flow (docs/proposal-workflow.md).
    pub fn curation(&self, workspace: Option<&str>) -> Result<CurationReport, StoreError> {
        let cx = &ReadCtx::default();
        let all_entities = self.store.all_entities(workspace)?;
        let relations = self.store.all_relations(workspace)?;
        // Apply accepted merges: a merged-away entity is resolved, so drop it from the candidate set and
        // rewire relations through its canonical id (so an accepted dedup stops showing as a candidate).
        let fwd = self.merge_forwarding(workspace, cx)?;
        let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let entities: Vec<&Entity> =
            all_entities.iter().filter(|e| !fwd.contains_key(&e.id)).collect();
        let node_ids: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();
        // Graph degree = relations whose both (canonical) endpoints are in the node set; dedup + no self-loops.
        let mut degree: HashMap<String, usize> = HashMap::new();
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        for r in &relations {
            let (f, t) = (canon(&r.from), canon(&r.to));
            if f == t || !node_ids.contains(f.as_str()) || !node_ids.contains(t.as_str()) {
                continue;
            }
            if !seen.insert((f.clone(), r.kind.clone(), t.clone())) {
                continue;
            }
            *degree.entry(f).or_default() += 1;
            *degree.entry(t).or_default() += 1;
        }
        let node = |e: &Entity| CurationNode {
            id: e.id.clone(),
            name: e.canonical_name.clone(),
            kind: e.kind.clone(),
            sources: e.provenance.len(),
            degree: degree.get(&e.id).copied().unwrap_or(0),
        };
        // (1) Namesakes: entities sharing a normalized name with distinct ids, which within a workspace
        // cannot happen, so these are the same name in two workspaces - two entities, never a merge
        // to propose (Principle 15).
        // BTreeMap keeps the groups ordered by key (Principle 16 deterministic read).
        let mut by_name: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
        for &e in &entities {
            by_name.entry(e.canonical_name.trim().to_lowercase()).or_default().push(e);
        }
        let duplicates: Vec<DuplicateGroup> = by_name
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(key, mut v)| {
                v.sort_by(|a, b| a.id.cmp(&b.id));
                DuplicateGroup { key, members: v.iter().map(|e| node(e)).collect() }
            })
            .collect();
        // (2) Orphans: no relation in the graph (degree 0). Sorted by (name, id).
        let mut orphans: Vec<CurationNode> = entities
            .iter()
            .copied()
            .filter(|e| degree.get(&e.id).copied().unwrap_or(0) == 0)
            .map(&node)
            .collect();
        orphans.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        // (3) Grab-bags: oversized hyperedges (Principle 11). Reuse the hypergraph projection.
        let mut grab_bags: Vec<GrabBag> = self
            .hypergraph_in(workspace, cx)?
            .hyperedges
            .into_iter()
            .filter(|h| h.size >= CURATION_GRAB_BAG_MIN)
            .map(|h| GrabBag {
                id: h.id,
                size: h.size,
                sources: h.sources,
                member_names: h.member_names,
            })
            .collect();
        grab_bags.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.id.cmp(&b.id)));
        // (4) Contradictions (resolution.md Section 4.2): nodes whose kind has surviving competitors,
        // reusing the graph projection's belief fold. Tier-tied (contested - mediation invited)
        // first, then (name, id). ALL live conflicts stay listed, tier-resolved ones included (R7).
        let mut contradictions: Vec<CurationConflict> = self
            .graph_in(workspace, cx)?
            .nodes
            .into_iter()
            .filter(|n| !n.competitors.is_empty())
            .map(|n| CurationConflict {
                id: n.id,
                name: n.name,
                field: "kind".into(),
                current: n.kind,
                contested: n.contested,
                competitors: n.competitors,
                kind_source: n.kind_source,
                target: None,
                workspace: None,
            })
            .collect();
        // A type's definition ties as a kind does (resolution-identity.md IR5): the same row, keyed
        // by the type's workspace, axis and name, since a type has no entity id.
        contradictions.extend(
            self.types_in(workspace, cx)?
                .into_iter()
                .filter(|t| !t.competitors.is_empty())
                .map(|t| {
                    let axis = match t.target {
                        TypeTarget::Entity => "entity",
                        TypeTarget::Relation => "relation",
                    };
                    CurationConflict {
                        id: format!("type:{}:{axis}:{}", t.workspace, t.name),
                        name: t.name,
                        field: "definition".into(),
                        current: t.description,
                        contested: t.contested,
                        competitors: t.competitors,
                        kind_source: t.def_source,
                        target: Some(t.target),
                        workspace: Some(t.workspace),
                    }
                }),
        );
        contradictions.sort_by(|a, b| {
            b.contested
                .cmp(&a.contested)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        // (5) Contradictory merge cycles (Principle 6): surfaced, not silent - the parity-resolved
        // projection stands (P16), but the cycle invites a settling proposal.
        let merge_cycles: Vec<MergeCycle> = self
            .merge_cycle_sets(workspace, cx)?
            .into_iter()
            .map(|(members, proposals)| MergeCycle {
                members: members
                    .iter()
                    .map(|id| {
                        all_entities.iter().find(|e| &e.id == id).map(node).unwrap_or_else(|| {
                            CurationNode {
                                id: id.clone(),
                                name: format!("({}...)", &id[..id.len().min(8)]),
                                kind: String::new(),
                                sources: 0,
                                degree: 0,
                            }
                        })
                    })
                    .collect(),
                proposals,
            })
            .collect();
        // The conservative merge band (Principle 15): embedding-near distinct-name candidates.
        let (merge_suggestions, merge_band) =
            self.merge_band(workspace, &all_entities, &fwd, &relations, cx)?;
        // The deterministic name-variant ladder: the same intent as the band, on the axis that needs
        // no embedder - so a keyword-only node (the prebuilt binary) still has a dedup signal.
        let (name_variants, name_ladder) = name_variant_groups(
            &entities,
            &fwd,
            &relations,
            &self.suppressed_pairs(workspace, cx)?,
            &node,
        );
        // T-Box axis collisions (Principle 9 minimal): a name defined on both the entity and the
        // relation axis. A pure fold over the type glossary (deterministic, P16).
        let mut axis: BTreeMap<String, (bool, bool)> = BTreeMap::new();
        for t in self.types_in(workspace, cx)? {
            let e = axis.entry(t.name).or_insert((false, false));
            match t.target {
                TypeTarget::Entity => e.0 = true,
                TypeTarget::Relation => e.1 = true,
            }
        }
        let type_axis_collisions: Vec<String> = axis
            .into_iter()
            .filter(|(_, (ent, rel))| *ent && *rel)
            .map(|(n, _)| n)
            .collect();
        let stats = CurationStats {
            duplicate_groups: duplicates.len(),
            grab_bags: grab_bags.len(),
            orphans: orphans.len(),
            contradictions: contradictions.len(),
            merge_cycles: merge_cycles.len(),
            merge_suggestions: merge_suggestions.len(),
            name_variants: name_variants.len(),
            type_axis_collisions: type_axis_collisions.len(),
        };
        // Over the rows this read already loaded, so the scan costs a pass and not a second walk of
        // the log. Deterministic in (observation id, field order), like every other signal here (P16).
        let mut secrets = Vec::new();
        for obs in self.log(workspace, cx)?.iter() {
            for (field, text) in scannable_fields(obs) {
                if let Some(hit) = supragnosis_core::detect_secret(text) {
                    secrets.push(SecretFinding {
                        observation: obs.id.clone(),
                        field,
                        pattern: hit.pattern,
                        at: hit.at,
                    });
                }
            }
        }
        // The retrieval weight (consolidation.md Section 4), over the same log pass the secret scan
        // walks. Reported, consumed by nothing - Section 8 step 1.
        let gates = self.gate_grants(workspace, cx)?;
        let demotion_candidates = recall_weights(&self.log(workspace, cx)?, &gates);
        let stray_verdicts = match workspace {
            Some(_) => Vec::new(),
            None => self.stray_verdicts(cx)?,
        };
        Ok(CurationReport {
            workspace: workspace.map(String::from),
            stray_verdicts,
            duplicates,
            grab_bags,
            orphans,
            contradictions,
            merge_cycles,
            merge_suggestions,
            merge_band,
            name_variants,
            name_ladder,
            type_axis_collisions,
            secrets,
            demotion_candidates,
            stats,
        })
    }
}

/// The workspace an entity belongs to. The id is blake3(workspace + normalized name), so an entity
/// lives in exactly one workspace and every attestation agrees; the first is representative.
/// Candidate generators need this because a report built with `workspace: None` (the viewer's
/// all-workspaces view) otherwise groups across workspaces, and an entity_merge spanning two of them
/// is refused (Principle 15) - the same name in two workspaces is two entities.
pub(crate) fn entity_workspace(e: &Entity) -> &str {
    e.provenance.first().map(|p| p.workspace.as_str()).unwrap_or("")
}

/// Separator/case-insensitive name key: keep alphanumerics and lowercase. Unicode-aware via
/// `is_alphanumeric`, so a Korean or otherwise non-Latin name survives intact instead of collapsing to
/// the empty string. `TrustTier`, `Trust Tier` and `trust-tier` all fold to `trusttier` - a collision
/// the entity id key (`trim`+`lowercase` only) deliberately keeps apart.
pub(crate) fn variant_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Naive English plural fold applied ON TOP of [`variant_key`]: `ports` -> `port`, `entities` ->
/// `entity`. Deliberately crude, because this is a candidate-generation key and the two error
/// directions are not symmetric: a miss (`classes` vs `class`) is a false negative nobody ever sees,
/// while a false positive costs one reviewer glance at the gate. `ss` endings are left alone so
/// `class` does not fold into `clas`.
pub(crate) fn plural_fold(key: &str) -> String {
    if let Some(stem) = key.strip_suffix("ies") {
        if stem.len() >= 2 {
            return format!("{stem}y");
        }
    }
    if let Some(stem) = key.strip_suffix('s') {
        if !key.ends_with("ss") && stem.len() >= 2 {
            return stem.to_string();
        }
    }
    key.to_string()
}

/// The deterministic name-variant ladder (Principle 15/16) - the dedup signal that needs no embedder.
///
/// Why it exists: an entity id is `blake3(workspace + trim+lowercase(name))`, so case/whitespace
/// variants already collapse at WRITE time, which in turn means [`CurationReport::duplicates`] (keyed
/// on that same normalization) can never fire within a single workspace. Anything orthographically
/// close but not identical under that key therefore had no detector at all whenever no embedder is
/// configured, because [`Engine::merge_band`] returns empty without one. That is the gap this closes,
/// and it is the common case for agent-written knowledge (`TrustTier` vs `Trust Tier`).
///
/// Detection only, never identity (I18/P23): this must NOT feed `Entity::make_id`. Folding variants
/// into the id would change every existing id - breaking content addressing and federation
/// convergence - and would silently merge pairs that must stay distinct, since `TrustTier` (a Rust
/// enum) and `Trust Tier` (the design concept) differ by exactly one separator yet are two concepts.
/// Naming the variance is the substrate's job; deciding identity is the gate's (Principle 15).
///
/// Deterministic (P16): BTreeMap/BTreeSet grouping, members sorted by id, and rungs walked in
/// declaration order, so the output ordering is fixed without an explicit final sort.
fn name_variant_groups(
    entities: &[&Entity],
    fwd: &HashMap<String, String>,
    relations: &[Relation],
    open_pairs: &HashSet<(String, String)>,
    node: &dyn Fn(&Entity) -> CurationNode,
) -> (Vec<NameVariantGroup>, NameLadderCoverage) {
    // Canonicalized undirected adjacency - the same structural corroboration the merge band uses.
    let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
    for r in relations {
        let (f, t) = (canon(&r.from), canon(&r.to));
        if f == t {
            continue;
        }
        adj.entry(f.clone()).or_default().insert(t.clone());
        adj.entry(t).or_default().insert(f);
    }
    let by_id: HashMap<&str, &Entity> = entities.iter().map(|e| (e.id.as_str(), *e)).collect();

    let mut out: Vec<NameVariantGroup> = Vec::new();
    // Pairs already named at a stronger rung. A group is emitted only when it contributes a pair no
    // earlier rung produced, so widening a group (a third member joining at `plural`) still reports
    // while a plain restatement of the same pair does not. Seeded with the pairs already under an
    // open proposal, which gets in-flight suppression for free through the same rule: a group whose
    // every pair is already proposed contributes nothing new and drops out.
    let mut seen_pairs: HashSet<(String, String)> = open_pairs.clone();
    // Every pair reached at any rung, and every pair a listed group shows: what was held back is
    // the answered ones among the first, less the second (design-system.md Section 29.28).
    let mut reached: HashSet<(String, String)> = HashSet::new();
    let mut shown: HashSet<(String, String)> = HashSet::new();

    for rung in [VariantRung::Separator, VariantRung::Plural, VariantRung::Alias] {
        // Keyed by (workspace, normalization) so a group never spans workspaces.
        let mut by_key: BTreeMap<(&str, String), BTreeSet<String>> = BTreeMap::new();
        for e in entities {
            let ws = entity_workspace(e);
            let name_key = variant_key(&e.canonical_name);
            match rung {
                VariantRung::Separator => {
                    by_key.entry((ws, name_key)).or_default().insert(e.id.clone());
                }
                VariantRung::Plural => {
                    by_key.entry((ws, plural_fold(&name_key))).or_default().insert(e.id.clone());
                }
                VariantRung::Alias => {
                    // Both the canonical name and every alias are entry points, so an entity is
                    // reachable by any spelling it has ever carried (the dedup counterpart of the
                    // alias parity the keyword search already has).
                    by_key.entry((ws, name_key)).or_default().insert(e.id.clone());
                    for a in &e.aliases {
                        by_key.entry((ws, variant_key(a))).or_default().insert(e.id.clone());
                    }
                }
            }
        }

        for ((_ws, key), ids) in by_key {
            if ids.len() < 2 || key.is_empty() {
                continue;
            }
            let members: Vec<String> = ids.into_iter().collect();
            let mut pairs: Vec<(String, String)> = Vec::new();
            let mut shared = 0usize;
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    pairs.push(unordered_pair(&members[i], &members[j]));
                    if let (Some(x), Some(y)) = (adj.get(&members[i]), adj.get(&members[j])) {
                        shared = shared.max(x.intersection(y).count());
                    }
                }
            }
            reached.extend(pairs.iter().cloned());
            if pairs.iter().all(|p| seen_pairs.contains(p)) {
                continue;
            }
            shown.extend(pairs.iter().cloned());
            seen_pairs.extend(pairs);
            out.push(NameVariantGroup {
                key,
                rung,
                members: members
                    .iter()
                    .filter_map(|id| by_id.get(id.as_str()).map(|e| node(e)))
                    .collect(),
                shared_neighbors: shared,
            });
        }
    }
    let held_back = reached
        .iter()
        .filter(|p| open_pairs.contains(*p) && !shown.contains(*p))
        .count();
    (out, NameLadderCoverage { held_back })
}
