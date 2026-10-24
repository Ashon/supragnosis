//! The graph views: workspaces, common types, the node-link graph, the hypergraph and
//! hyperedge reification.

use super::*;

/// The canonical member set an observation co-asserts (the hyperedge membership rule): entity
/// assertions + both relation endpoints, resolved by name to entity ids, forwarded through
/// accepted merges, and kept only when present in the graph node set (closed hull - the same
/// discipline as graph()'s edge closure). BTreeSet gives dedup + sorted order at once
/// (arrival-order independent - Principle 16). Shared by the hypergraph projection and reify.
fn co_asserted_members(
    obs: &Observation,
    node_ids: &HashSet<&str>,
    fwd: &HashMap<String, String>,
) -> Vec<String> {
    let members: BTreeSet<String> = asserted_ids(obs)
        .map(|id| fwd.get(&id).cloned().unwrap_or(id))
        .filter(|id| node_ids.contains(id.as_str()))
        .collect();
    members.into_iter().collect()
}

/// The entity ids an observation asserts, as entities and as both ends of its relations, as
/// written - not forwarded through merges, the ids the entity fold keys its rows by.
pub(crate) fn asserted_ids(obs: &Observation) -> impl Iterator<Item = String> + '_ {
    let ws = obs.workspace();
    let names = obs.assertions.entities.iter().map(|e| e.name.as_str());
    let ends = obs.assertions.relations.iter().flat_map(|r| [r.from.as_str(), r.to.as_str()]);
    names.chain(ends).map(move |name| Entity::make_id(ws, name))
}

impl Engine {
    /// This node's default workspace (referenced when an MCP resource builds a concrete URI).
    pub fn default_workspace(&self) -> &str {
        &self.default_workspace
    }

    /// This node's host id, the one its provenance carries (`localhost` unless configured).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The list of workspaces where knowledge exists (sorted, deterministic - Principle 16). Derived from the
    /// provenance.workspace of the projected graph (entities/relations) - the set of workspaces for which a graph can
    /// be drawn. Computed with the existing read ports alone, without a separate store enumeration.
    /// BTreeSet gives dedup + sort at once, guaranteeing a result independent of arrival order.
    pub fn workspaces(&self) -> Result<Vec<String>, StoreError> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        for e in self.store.all_entities(None)? {
            for p in &e.provenance {
                set.insert(p.workspace.clone());
            }
        }
        for r in self.store.all_relations(None)? {
            set.insert(r.provenance.workspace.clone());
        }
        Ok(set.into_iter().collect())
    }

    /// Projects the ontology graph into a node-link view (the read path for observability/visualization).
    /// A pure read - it does not touch the observation log (Principle 1). An edge is included only when both endpoints
    /// are in the node set, giving a closed (renderable) graph. Node/edge order is deterministic (Principle 16).
    /// The store's most used entity types, at most `COMMON_TYPES`, by count then name - what a page
    /// assigns the type band's colors to, once (design-system.md Section 29.9, D4). Counted over the
    /// materialized projection, merged-away rows left out, in every workspace (`None`) or in the
    /// named ones, each counted on its own and summed. It reads the entity rows only.
    pub fn common_types(&self, workspaces: Option<&[String]>) -> Result<Vec<String>, StoreError> {
        let cx = &ReadCtx::default();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut count_in = |ws: Option<&str>| -> Result<(), StoreError> {
            let fwd = self.merge_forwarding(ws, cx)?;
            for e in self.store.all_entities(ws)? {
                if !fwd.contains_key(&e.id) {
                    *counts.entry(e.kind).or_default() += 1;
                }
            }
            Ok(())
        };
        match workspaces {
            None => count_in(None)?,
            Some(list) => {
                for ws in list {
                    count_in(Some(ws))?;
                }
            }
        }
        let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(ranked.into_iter().take(COMMON_TYPES).map(|(t, _)| t).collect())
    }

    pub fn graph(&self, workspace: Option<&str>) -> Result<GraphView, StoreError> {
        self.graph_in(workspace, &ReadCtx::default())
    }

    /// [`Engine::graph`] over an existing read context, so a caller that already loaded the
    /// log does not load it again (see [`ReadCtx`]).
    pub(crate) fn graph_in(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<GraphView, StoreError> {
        let entities = self.store.all_entities(workspace)?;
        // Ordered by the stable key before anything picks among rows (Principle 16). A merge can
        // fold two relations onto one (from, kind, to), and the survivor carries its own
        // description/tier/confidence - so "which duplicate" is answered by the relation id, not by
        // whichever order the adapter enumerated (InMemory a HashMap, redb a B-tree range).
        // Choosing among duplicates on MERIT would be relation belief resolution, which is deferred
        // with negation semantics (architecture.md Section 14); pinning the tie is not.
        let mut relations = self.store.all_relations(workspace)?;
        relations.sort_by(|a, b| a.id.cmp(&b.id));
        // Apply accepted entity-merges (Principle 15): fold merged-away ids into their canonical, at
        // projection time only - the log keeps both (Principle 3). Deterministic (Principle 16).
        let fwd = self.merge_forwarding(workspace, cx)?;
        // Belief fold (resolution.md): gate grants + per-node kind candidates / effective tiers,
        // computed from the log so the view is policy-current without waiting for reprojection (F5
        // continuous convergence; the materialized rows converge at the next replay).
        let gates = self.gate_grants(workspace, cx)?;
        let belief = self.belief_fold(workspace, &fwd, &gates, cx)?;
        let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());

        let by_id: HashMap<&str, &Entity> = entities.iter().map(|e| (e.id.as_str(), e)).collect();
        // Group entities by canonical id - merged duplicates collapse into one node.
        let mut groups: BTreeMap<String, Vec<&Entity>> = BTreeMap::new();
        for e in &entities {
            groups.entry(canon(&e.id)).or_default().push(e);
        }
        // Same reason as the relations above: the representative falls back to a member by position
        // when the canonical id has no projected row (a partial-sync state), so the members are
        // ordered by the stable key rather than by enumeration (Principle 16).
        for members in groups.values_mut() {
            members.sort_by(|a, b| a.id.cmp(&b.id));
        }
        let node_ids: HashSet<&str> = groups.keys().map(|s| s.as_str()).collect();

        // A relation's trust is the highest EFFECTIVE tier among the observations that state it, gate
        // events applied (resolution.md Section 3, design-system.md Section 29.7) - keyed as the edges
        // are, by endpoints after merges and the normalized type, so merged duplicates share one. A
        // max over a set, so the order of the log does not matter (Principle 16).
        let mut edge_tier: HashMap<(String, String, String), TrustTier> = HashMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            if obs.assertions.relations.is_empty() {
                continue;
            }
            let ws = obs.workspace();
            let tier = effective_tier(obs, &gates);
            for ra in &obs.assertions.relations {
                let key = (
                    canon(&Entity::make_id(ws, &ra.from)),
                    normalize_relation_kind(&ra.kind),
                    canon(&Entity::make_id(ws, &ra.to)),
                );
                edge_tier.entry(key).and_modify(|t| *t = (*t).max(tier)).or_insert(tier);
            }
        }

        // A merge collapses rows onto one edge, and its support is all of theirs, as a merged node's
        // is; a row written before relations accumulated counts its one attestation.
        let mut support: HashMap<(String, String, String), usize> = HashMap::new();
        for r in &relations {
            let (f, t) = (canon(&r.from), canon(&r.to));
            if f != t {
                *support.entry((f, r.kind.clone(), t)).or_default() += r.attestations.len().max(1);
            }
        }

        // Edges: rewire endpoints through canon, drop merge self-loops and duplicates, count degree.
        let mut degree: HashMap<String, usize> = HashMap::new();
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for r in &relations {
            let (f, t) = (canon(&r.from), canon(&r.to));
            if f == t || !node_ids.contains(f.as_str()) || !node_ids.contains(t.as_str()) {
                continue; // self-loop from a merge, or an endpoint outside the node set
            }
            let trust_tier = edge_tier
                .get(&(f.clone(), r.kind.clone(), t.clone()))
                .copied()
                .unwrap_or_else(|| evaluated_tier(&r.provenance));
            let key = (f.clone(), r.kind.clone(), t.clone());
            let sources = support.get(&key).copied().unwrap_or(1);
            if !seen.insert(key) {
                continue; // the merge can produce duplicate edges - keep the lowest relation id
            }
            *degree.entry(f.clone()).or_default() += 1;
            *degree.entry(t.clone()).or_default() += 1;
            edges.push(GraphEdge {
                from: f,
                to: t,
                kind: r.kind.clone(),
                description: r.description.clone(),
                // Effective, never the raw claim (resolution.md Section 3, F13); the row's own
                // evaluation only when no observation in this log states it (a partial sync).
                trust_tier,
                // The latest asserting row's, as the description is (design-system.md Section 29.7).
                confidence: r.provenance.confidence,
                sources,
                valid_to: r.valid_to,
            });
        }

        let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut trust_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut nodes: Vec<GraphNode> = groups
            .iter()
            .map(|(cid, members)| {
                // Canonical entity = the one whose id is the canonical id (fallback: first member).
                let ce = by_id.get(cid.as_str()).copied().unwrap_or(members[0]);
                // Representative trust = the effective tier from the belief fold (receiver-evaluated
                // + gate grants). Fallback for rows with no log support (legacy/pre-log entities):
                // max EVALUATED tier over stored attestations - never the raw claimed max (F13).
                let trust = belief.tiers.get(cid.as_str()).copied().unwrap_or_else(|| {
                    members
                        .iter()
                        .flat_map(|m| m.provenance.iter())
                        .map(evaluated_tier)
                        .max()
                        .unwrap_or_default()
                });
                // Kind belief: the policy winner over the log's kind candidates; the stored row's
                // kind is the fallback when the log never asserted one (resolution.md Section 2.2).
                let (kind_winner, contested, competitors) =
                    self.resolve_kind(belief.kinds.get(cid.as_str()));
                let sources: usize = members.iter().map(|m| m.provenance.len()).sum();
                let mut origins: Vec<String> = members
                    .iter()
                    .flat_map(|m| m.provenance.iter())
                    .map(|p| p.host.clone())
                    .collect();
                origins.sort();
                origins.dedup();
                // Aliases = every member's canonical spelling + its accumulated same-id spelling
                // aliases (the write path materializes those - Section 2/IR1), minus the canonical
                // name. So a merged-away name and a case-variant spelling both surface here.
                let mut aliases: Vec<String> = members
                    .iter()
                    .flat_map(|m| {
                        std::iter::once(m.canonical_name.clone()).chain(m.aliases.iter().cloned())
                    })
                    .filter(|n| n != &ce.canonical_name)
                    .collect();
                aliases.sort();
                aliases.dedup();
                let (kind, kind_source) = match kind_winner {
                    Some((value, obs)) => (value, Some(obs)),
                    None => (ce.kind.clone(), None),
                };
                *type_counts.entry(kind.clone()).or_default() += 1;
                *trust_counts.entry(tier_label(trust).to_string()).or_default() += 1;
                GraphNode {
                    id: cid.clone(),
                    name: ce.canonical_name.clone(),
                    kind,
                    description: ce.description.clone(),
                    aliases,
                    degree: degree.get(cid).copied().unwrap_or(0),
                    sources,
                    origins,
                    origin_nodes: {
                        let mut o: Vec<String> = members
                            .iter()
                            .flat_map(|m| m.provenance.iter())
                            .filter_map(|p| p.sync.as_ref().map(|s| s.origin_node.clone()))
                            .collect();
                        o.sort();
                        o.dedup();
                        o
                    },
                    workspace: {
                        let mut it = members
                            .iter()
                            .flat_map(|m| m.provenance.iter())
                            .map(|p| p.workspace.as_str());
                        match it.next() {
                            Some(w) if it.all(|o| o == w) => w.to_string(),
                            _ => String::new(),
                        }
                    },
                    last_said: members
                        .iter()
                        .flat_map(|m| m.provenance.iter())
                        .map(|p| p.observed_at)
                        .max(),
                    trust_tier: trust,
                    contested,
                    competitors,
                    kind_source,
                }
            })
            .collect();

        // Deterministic order (Principle 16): nodes stable-sorted by id, edges by (from, kind, to).
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.to.cmp(&b.to))
        });

        let stats = GraphStats {
            node_count: nodes.len(),
            edge_count: edges.len(),
            type_counts,
            trust_counts,
        };
        Ok(GraphView { workspace: workspace.map(String::from), nodes, edges, stats })
    }

    /// Projects the hypergraph (the second-order structure of co-occurrence) (Principle 11 "the ground of induction").
    /// Reads the entire observation log (a pure read - Principle 1), resolves the entity names co-asserted by each
    /// observation to canonical ids, and takes the set with only the members in the graph node set as a hyperedge.
    /// Size < 2 is not a hyperedge (degenerate - Principle 11 second-order structure caveat). The same member set is
    /// deduped and its sources/trust accumulate (Principle 3). Order/identifiers are deterministic (Principle 16).
    ///
    /// This view only **generates** candidates/signals - decisions such as merge/promotion/schema definition go through
    /// the existing gates (resolution/proposal/human confirmation). A derived view does not write the canonical record directly (Principle 1/19).
    pub fn hypergraph(&self, workspace: Option<&str>) -> Result<HyperGraphView, StoreError> {
        self.hypergraph_in(workspace, &ReadCtx::default())
    }

    /// [`Engine::hypergraph`] over an existing read context, so a caller that already loaded the
    /// log does not load it again (see [`ReadCtx`]).
    pub(crate) fn hypergraph_in(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HyperGraphView, StoreError> {
        let all_entities = self.store.all_entities(workspace)?;
        // Gate grants feed the per-observation effective tier (resolution.md Section 3).
        let gates = self.gate_grants(workspace, cx)?;
        // Apply accepted entity-merges (Principle 15), exactly like graph(): membership resolves
        // through the forwarding, merged-away rows drop from the node set, and member sets that
        // coincide after canonicalization union into one hyperedge (their sources accumulate -
        // Principle 3, the member set is the identity, Principle 14).
        let fwd = self.merge_forwarding(workspace, cx)?;
        let entities: Vec<&Entity> =
            all_entities.iter().filter(|e| !fwd.contains_key(&e.id)).collect();
        let node_ids: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();
        // id -> canonical name (readability: hyperedge members are carried as names too).
        let name_by_id: HashMap<&str, &str> =
            entities.iter().map(|e| (e.id.as_str(), e.canonical_name.as_str())).collect();

        // Per-observation co-occurrence set -> accumulate hyperedges, deduping by member set.
        // Value: (sorted members, sources count, highest trust among contributing observations).
        let mut acc: HashMap<String, (Vec<String>, usize, TrustTier)> = HashMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            let members = co_asserted_members(obs, &node_ids, &fwd);
            if members.len() < HYPEREDGE_MIN_SIZE {
                continue; // A degenerate set (single/0 members) is not a hyperedge.
            }
            let id = hyperedge_id(&members);
            // This observation's representative trust = its EFFECTIVE tier (receiver-evaluated +
            // gate grants, resolution.md Section 3) - never the raw claimed max (F13).
            let obs_trust = effective_tier(obs, &gates);
            acc.entry(id)
                .and_modify(|(_, sources, trust)| {
                    *sources += 1;
                    *trust = (*trust).max(obs_trust);
                })
                .or_insert((members, 1, obs_trust));
        }

        let mut hyperedges: Vec<HyperEdge> = acc
            .into_iter()
            .map(|(id, (members, sources, trust_tier))| {
                let member_names = members
                    .iter()
                    .map(|m| name_by_id.get(m.as_str()).copied().unwrap_or("").to_string())
                    .collect();
                HyperEdge { size: members.len(), member_names, id, members, sources, trust_tier }
            })
            .collect();
        // Deterministic order (Principle 16): size desc (larger context first), ties broken by id asc.
        hyperedges.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.id.cmp(&b.id)));
        let max_size = hyperedges.iter().map(|h| h.size).max().unwrap_or(0);

        // Carry each node's hyperedge-degree (Principle 11 second-order structure: boundary-concept/hub signal) in degree -
        // unlike graph()'s degree (binary edge degree), here it is "how many contexts it belongs to".
        let mut hyper_degree: HashMap<String, usize> = HashMap::new();
        for h in &hyperedges {
            for m in &h.members {
                *hyper_degree.entry(m.clone()).or_default() += 1;
            }
        }

        // When each node was last said, its merged-away rows' attestations included, so the hypergraph
        // and the graph give one entity one time (design-system.md Section 29.19).
        let mut last_said: HashMap<&str, Timestamp> = HashMap::new();
        for e in &all_entities {
            let canon = fwd.get(&e.id).map(String::as_str).unwrap_or(e.id.as_str());
            if let Some(t) = e.provenance.iter().map(|p| p.observed_at).max() {
                let slot = last_said.entry(canon).or_insert(t);
                *slot = (*slot).max(t);
            }
        }
        let mut nodes: Vec<GraphNode> = entities
            .iter()
            .map(|e| {
                // Receiver-evaluated per attestation (resolution.md Section 3) - never the raw
                // claimed max (F13). Kind belief/contested live on the graph projection; this
                // overlay keeps the stored kind (the two share node ids, so the viewer joins them).
                let trust = e.provenance.iter().map(evaluated_tier).max().unwrap_or_default();
                GraphNode {
                    id: e.id.clone(),
                    name: e.canonical_name.clone(),
                    kind: e.kind.clone(),
                    description: e.description.clone(),
                    // Merged-away rows are dropped and membership forwards to the canonical id;
                    // the merged-name alias display stays graph()'s concern (the overlay joins by id).
                    aliases: Vec::new(),
                    degree: hyper_degree.get(&e.id).copied().unwrap_or(0),
                    sources: e.provenance.len(),
                    origins: {
                        let mut o: Vec<String> =
                            e.provenance.iter().map(|p| p.host.clone()).collect();
                        o.sort();
                        o.dedup();
                        o
                    },
                    origin_nodes: {
                        let mut o: Vec<String> = e
                            .provenance
                            .iter()
                            .filter_map(|p| p.sync.as_ref().map(|s| s.origin_node.clone()))
                            .collect();
                        o.sort();
                        o.dedup();
                        o
                    },
                    // Agreement, not the first: a merge can fold entities from two workspaces
                    // (nothing on that path compares them), and picking either would assert a
                    // membership the log does not. Disagreement reports empty.
                    workspace: {
                        let mut it = e.provenance.iter().map(|p| p.workspace.as_str());
                        match it.next() {
                            Some(w) if it.all(|o| o == w) => w.to_string(),
                            _ => String::new(),
                        }
                    },
                    last_said: last_said.get(e.id.as_str()).copied(),
                    trust_tier: trust,
                    contested: false,
                    competitors: Vec::new(),
                    kind_source: None,
                }
            })
            .collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let stats = HyperGraphStats {
            node_count: nodes.len(),
            hyperedge_count: hyperedges.len(),
            max_size,
        };
        Ok(HyperGraphView { workspace: workspace.map(String::from), nodes, hyperedges, stats })
    }

    /// Reifies a co-occurrence context into first-class ontology structure - the promotion path a
    /// hyperedge takes INTO the graph (Principle 11: the substrate generates, it never becomes an
    /// edge itself). Asserts a group entity + a `member_of` relation from each member, as an
    /// ordinary observation whose `derived_from` names every co-asserting observation (P18:
    /// induction output is lineage-bearing; it enters at the default tier and rises only through
    /// the gate). The hyperedge is untouched - a derived view has no state to edit - but the
    /// grouping is now ASSERTED, so it inherits provenance, tier, supersede, and merge management
    /// exactly like any other edge, with no parallel mechanism. Ingest stays free (P22).
    pub fn reify_hyperedge(&self, input: ReifyInput) -> Result<ObserveOutput, ObserveError> {
        let cx = &ReadCtx::default();
        let workspace = input.workspace.clone();
        let ws = workspace.as_deref();
        let hg = self.hypergraph(ws).map_err(ObserveError::Store)?;
        let Some(h) = hg.hyperedges.iter().find(|h| h.id == input.hyperedge) else {
            return Err(ObserveError::Invalid(format!(
                "unknown hyperedge '{}' - ids come from workspace_map / the hypergraph resource, \
                 and a hyperedge's id changes when its membership changes (the member set is the \
                 identity), so re-list before retrying",
                input.hyperedge
            )));
        };
        // The lineage of the reified assertion = every observation whose canonical co-asserted
        // member set is exactly this hyperedge (the same membership rule the projection uses).
        let all_entities = self.store.all_entities(ws)?;
        let fwd = self.merge_forwarding(ws, cx)?;
        let live: Vec<&Entity> = all_entities.iter().filter(|e| !fwd.contains_key(&e.id)).collect();
        let node_ids: HashSet<&str> = live.iter().map(|e| e.id.as_str()).collect();
        let mut derived_from: Vec<String> = Vec::new();
        let mut homes: BTreeSet<String> = BTreeSet::new();
        for obs in self.observations(ws)? {
            if co_asserted_members(&obs, &node_ids, &fwd) == h.members {
                derived_from.push(obs.id.clone());
                homes.insert(obs.workspace().to_string());
            }
        }
        derived_from.sort();
        // All is a view, not a place (design-system.md Section 29.15): a region read under All is
        // asserted where the observations that asserted it live, so its members' names are minted
        // in their own workspace and nothing is copied into another.
        let workspace = match workspace {
            Some(w) => Some(w),
            None => match homes.len() {
                1 => homes.pop_first(),
                0 => {
                    return Err(ObserveError::Invalid(format!(
                        "no observation asserts hyperedge '{}' any more - re-list before retrying",
                        input.hyperedge
                    )))
                }
                _ => {
                    return Err(ObserveError::Invalid(format!(
                        "hyperedge '{}' is asserted in more than one workspace, so it has no one \
                         place to be named - name it from one workspace",
                        input.hyperedge
                    )))
                }
            },
        };
        let name = match input.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            Some(n) => n,
            None => {
                let head: Vec<&str> = h.member_names.iter().take(3).map(|s| s.as_str()).collect();
                format!(
                    "context: {}{}",
                    head.join(", "),
                    if h.member_names.len() > 3 { ", ..." } else { "" }
                )
            }
        };
        let kind = input
            .kind
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| "Context".to_string());
        let content = format!(
            "Reified co-occurrence context '{name}': {} (corroborated by {} observation{})",
            h.member_names.join(", "),
            h.sources,
            if h.sources == 1 { "" } else { "s" }
        );
        let description = format!(
            "reified from co-occurrence hyperedge {} ({} members, {} co-asserting observations)",
            h.id, h.size, h.sources
        );
        self.observe(ObserveInput {
            content,
            workspace,
            source_ref: input.source_ref,
            confidence: None,
            on_behalf_of: input.on_behalf_of,
            derived_from,
            entities: vec![EntityInput {
                name: name.clone(),
                kind: Some(kind),
                description: Some(description),
            }],
            relations: h
                .member_names
                .iter()
                .filter(|m| !m.trim().is_empty())
                .map(|m| RelationInput {
                    from: m.clone(),
                    kind: "member_of".into(),
                    to: name.clone(),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                })
                .collect(),
        })
    }
}

/// The minimum hyperedge size (arity). 1 (a single entity) / 0 is not co-occurrence but a degenerate state where a
/// hyperedge does not hold (Principle 11 second-order structure caveat). 2 converges on a binary co-mention but is still
/// a "said together" context, so it is included.
const HYPEREDGE_MIN_SIZE: usize = 2;
