//! Search and traversal: hybrid search, graph enrichment, rank fusion and recall weights.

use super::*;

/// The (kind, id) of the hits one stage of a search held.
type HitSet = HashSet<(SearchHitKind, String)>;

impl Engine {
    /// Hybrid search: fuses keyword (substring match) + vector (semantic) results with RRF, then enriches with the
    /// graph neighbors of the top entity hits. The vector path semantically recalls **both** observation bodies and
    /// entity names (so even entity nodes not mentioned lexically by an observation are reached by the meaning of their name),
    /// and the enrichment step fills in the 1-hop neighbors of matched entities to recall nodes that are not caught by
    /// lexical/semantic means but are graph-adjacent (architecture 4.2 "graph enrichment"). If there is no embedder or the query
    /// embedding fails, only keyword results are fused (Principle 19: degrade). The final ranking is deterministic.
    pub fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<SearchOutput, StoreError> {
        Ok(self.search_traced(query, workspace, limit)?.0)
    }

    /// [`Engine::search`], with the two sets the viewer reads to say how each hit was reached: the
    /// hits the keyword surface matched, and every hit the fusion held before the graph's enrichment
    /// added neighbors. The ranking is the search's own; nothing here reorders it.
    fn search_traced(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<(SearchOutput, HitSet, HitSet), StoreError> {
        let keyword = self.store.search(query, workspace, limit)?;
        let matched: HitSet = keyword.iter().map(|h| (h.kind, h.id.clone())).collect();

        // The query embedding is computed once and shared by the observation/entity semantic searches.
        // An embedding failure is a degrade (keyword only, Principle 19), but a store failure is an Err -
        // the absence/failure of the probabilistic adapter and the failure of the deterministic store are different events.
        let qvec = self.embedder.as_ref().and_then(|e| e.embed_one(query).ok());
        // mode is "did it reference the semantic surface" - even if semantic retrieval is zero, it did reference it, so it is
        // hybrid (the epistemic weight of zero results differs by mode, Principle 5/16 4th).
        let mode = if qvec.is_some() { SearchMode::Hybrid } else { SearchMode::Keyword };
        let (semantic_obs, semantic_ent) = match &qvec {
            Some(v) => (
                self.store.search_semantic(v, workspace, limit)?,
                self.store.search_semantic_entities(v, workspace, limit)?,
            ),
            None => (Vec::new(), Vec::new()),
        };

        // If there is no semantic retrieval (no embedder / not embedded), use the keyword ranking as-is; otherwise RRF-fuse.
        let fused = if semantic_obs.is_empty() && semantic_ent.is_empty() {
            keyword
        } else {
            fuse_rrf(&[keyword, semantic_obs, semantic_ent], limit)
        };

        // Graph enrichment: fill the spare slots with the 1-hop neighbors of the top entity hits.
        let fused_set: HitSet = fused.iter().map(|h| (h.kind, h.id.clone())).collect();
        let hits = self.enrich_with_graph(fused, workspace, limit)?;
        Ok((SearchOutput { mode, hits }, matched, fused_set))
    }

    /// Search as the viewer asks it (design-system.md Section 8): the function `search_knowledge`
    /// answers with, its ranking and mode unchanged, with entity hits forwarded through accepted
    /// merges, so a hit names the entity the graph draws and never a merged-away id the viewer would
    /// otherwise have to resolve itself (inspector.md D5). Two hits that forward to one entity keep
    /// the first, the higher ranked. Each hit says how it was reached - its words matched, it is near
    /// in meaning, or it is a neighbor the enrichment added - read from the lists the search itself
    /// consumed, so the viewer can group them (U15) without ranking anything. The MCP tool's own
    /// answer is untouched: forwarding folds the log, which `search` never walks (read_path_cost.rs),
    /// and a merged-away id it answers forwards at `get_entity` and `traverse`.
    pub fn search_canonical(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<ViewerSearch, StoreError> {
        let (out, matched, fused) = self.search_traced(query, workspace, limit)?;
        let fwd = self.merge_forwarding(workspace, &ReadCtx::default())?;
        let mut seen = HashSet::new();
        let mut hits = Vec::with_capacity(out.hits.len());
        for mut hit in out.hits {
            let key = (hit.kind, hit.id.clone());
            let reached = if matched.contains(&key) {
                Reached::Words
            } else if fused.contains(&key) {
                Reached::Meaning
            } else {
                Reached::Neighbor
            };
            if hit.kind == SearchHitKind::Entity {
                if let Some(c) = fwd.get(&hit.id) {
                    hit.id = c.clone();
                }
                if !seen.insert(hit.id.clone()) {
                    continue;
                }
            }
            hits.push(ViewerHit { hit, reached });
        }
        Ok(ViewerSearch { mode: out.mode, hits })
    }

    /// Graph enrichment: adds the 1-hop neighbors of the top entity hits (seeds) to the results. A neighbor is a weaker
    /// signal than a seed's direct match, so it is ranked with the seed score decayed - a primary hit stronger than a
    /// neighbor stays above it, and the neighbor of a strong seed can rise above a weak primary hit (reflecting graph proximity).
    /// It is bounded: the seed count / resolved neighbor count are capped so an active node cannot flood the results.
    /// It is deterministic (Principle 16): a neighbor's score is taken as the max over the seeds that reached it, so it is
    /// independent of traversal order, and the final sort is pinned to (score desc, id asc).
    fn enrich_with_graph(
        &self,
        mut results: Vec<SearchHit>,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        // The (kind, id) already in the results - prevents duplicate neighbors / re-adding primary hits.
        let present: HashSet<(SearchHitKind, String)> =
            results.iter().map(|h| (h.kind, h.id.clone())).collect();

        // Using the top entity hits as seeds, gather 1-hop neighbor scores. If reached from multiple seeds, take the max
        // (independent of arrival order - determinism). The relation's opposite endpoint is the neighbor.
        let mut neighbor_score: HashMap<String, f32> = HashMap::new();
        for seed in results
            .iter()
            .filter(|h| h.kind == SearchHitKind::Entity)
            .take(GRAPH_ENRICH_SEEDS)
        {
            let contrib = seed.score * GRAPH_ENRICH_DECAY;
            for rel in self.store.relations_of(&seed.id)? {
                let neighbor = if rel.from == seed.id {
                    rel.to
                } else if rel.to == seed.id {
                    rel.from
                } else {
                    continue;
                };
                if present.contains(&(SearchHitKind::Entity, neighbor.clone())) {
                    continue;
                }
                let e = neighbor_score.entry(neighbor).or_insert(0.0);
                *e = e.max(contrib);
            }
        }

        // Bound the resolution cost: resolve only the top limit neighbors to entities (check name/workspace).
        let mut candidates: Vec<(String, f32)> = neighbor_score.into_iter().collect();
        candidates.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        candidates.truncate(limit);

        for (id, score) in candidates {
            if let Some(entity) = self.store.get_entity(&id)? {
                // If a workspace is specified, only nodes within it (prevents cross-workspace neighbor leakage).
                let in_ws =
                    workspace.is_none_or(|ws| entity.provenance.iter().any(|p| p.workspace == ws));
                if !in_ws {
                    continue;
                }
                results.push(SearchHit {
                    kind: SearchHitKind::Entity,
                    id,
                    snippet: entity.canonical_name,
                    score,
                });
            }
        }

        // Global re-sort (score desc, id asc) then limit - unifies primary hits and neighbors into one ranking.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        results.truncate(limit);
        Ok(results)
    }

    /// Traverses neighbors from an entity following the relation direction (from->to) up to
    /// `max_depth` hops, through accepted merges as the graph draws them (Principle 15): the walk
    /// starts at the entity a merged-away id forwards to, leaves a node by any member's edges and
    /// names each neighbor by its canonical id. With no merge in the start's workspace it is the
    /// store's walk; either way it is in (depth, id) order, and a node with no row is walked
    /// through but not named.
    pub fn traverse(
        &self,
        id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError> {
        let max_depth = max_depth.max(1);
        let ws = self
            .store
            .get_entity(id)?
            .and_then(|e| e.provenance.first().map(|p| p.workspace.clone()));
        let fwd = match &ws {
            Some(ws) => self.merge_forwarding(Some(ws), &ReadCtx::default())?,
            None => HashMap::new(),
        };
        if fwd.is_empty() {
            return self.store.traverse(id, max_depth, limit);
        }
        let canon = |x: &str| fwd.get(x).cloned().unwrap_or_else(|| x.to_string());
        let mut members: HashMap<&str, Vec<&str>> = HashMap::new();
        for (m, c) in &fwd {
            members.entry(c.as_str()).or_default().push(m.as_str());
        }
        let start = canon(id);
        let mut visited = HashSet::from([start.clone()]);
        let mut frontier = vec![start];
        let mut out = Vec::new();
        for depth in 1..=max_depth {
            let mut next: BTreeSet<String> = BTreeSet::new();
            for node in &frontier {
                let class = members.get(node.as_str()).into_iter().flatten().copied();
                for m in std::iter::once(node.as_str()).chain(class) {
                    for r in self.store.relations_of(m)? {
                        let to = canon(&r.to);
                        if r.from == m && !visited.contains(&to) {
                            next.insert(to);
                        }
                    }
                }
            }
            for to in &next {
                visited.insert(to.clone());
                if let Some(e) = self.store.get_entity(to)? {
                    out.push(TraverseHit {
                        id: to.clone(),
                        depth,
                        name: e.canonical_name,
                        kind: e.kind,
                    });
                    if out.len() >= limit {
                        return Ok(out);
                    }
                }
            }
            if next.is_empty() {
                break; // nothing left to walk, whatever depth was asked for
            }
            frontier = next.into_iter().collect();
        }
        Ok(out)
    }
}

/// The decay applied to neighbors in graph enrichment. A neighbor is a weaker signal than a seed (direct match), so it
/// is ranked at half the seed score to keep it below primary hits, while allowing the neighbor of a strong seed to rise
/// above a weak primary hit (reflecting graph proximity in the ranking).
const GRAPH_ENRICH_DECAY: f32 = 0.5;
/// The cap on the number of seeds (top entity hits) whose neighbors are expanded - cost/precision control (bounded so an
/// active node cannot flood the results).
const GRAPH_ENRICH_SEEDS: usize = 5;

/// Reciprocal Rank Fusion. Fuses rankings on different scales (keyword score vs cosine similarity) by rank alone,
/// combining them without scale normalization. The same (kind, id) has its contributions summed.
/// A deterministic function (Principle 16) - the same input ranks give the same result on any node.
/// How many lowest-weighted rows the curation report carries. A cap, not a threshold: the report is
/// a thing a human reads, and "everything, sorted" is not a curation signal.
const DEMOTION_CANDIDATES: usize = 20;

/// The floor of the retrieval weight. Positive on purpose (C4): zero would make an item unrankable,
/// which is deletion performed by arithmetic, and P7 requires demoted knowledge to stay reachable.
const RECALL_FLOOR: f32 = 0.05;

/// The retrieval weight of every observation, lowest first ([consolidation.md](../../../docs/consolidation.md)
/// Section 4.3).
///
/// Three factors compose multiplicatively rather than additively, because the signal being looked
/// for is the conjunction: old knowledge is not weak, untrusted knowledge is not weak, uncorroborated
/// derivation is not weak - old AND untrusted AND uncorroborated is. A sum would let any one factor
/// carry a row on its own, which is how a recency heuristic ends up demoting a human-confirmed fact
/// for the crime of having been true for a while.
///
/// Deterministic in the observation set alone: no wall clock (Section 4.2), no arrival order, no
/// node-local signal (Section 4.1). Ties break on id, like every other ordered response (P16).
pub(crate) fn recall_weights(
    log: &[Observation],
    gates: &HashMap<String, TrustTier>,
) -> Vec<RecallWeight> {
    // Frontier position is where this observation's ordering HLC falls in the SPAN the workspace's
    // own log covers - not its rank in that ordering. Two reasons, and the second is why the first
    // is affordable.
    //
    // A rank manufactures age differences that did not happen. A thousand observations written
    // inside one hour get spread by rank across the whole range, so the first and the five-hundredth
    // are treated as far apart in age when they are seconds apart; the span says what actually
    // occurred. For a signal whose whole job is "how stale is this", inventing staleness is the one
    // error that matters.
    //
    // And a span needs two numbers per workspace where a rank needs a position per observation.
    // That is what makes this materializable at all (consolidation.md Section 8 step 2): the log is
    // immutable, so a per-observation value could not be written onto the observation rows without
    // mutating the log (P3), and carrying it anywhere else means a parallel table holding a derived
    // number - a port change to store something no observation asserts. Two scalars need neither.
    //
    // `wall` is a recorded field, not a reading of this machine's clock, so comparing two of them
    // stays deterministic (P16 forbids consulting the clock, not arithmetic on data).
    let walls: Vec<u64> = log.iter().map(|o| supragnosis_core::ordering_hlc(o).wall).collect();
    let (oldest, newest) = (
        walls.iter().copied().min().unwrap_or(0),
        walls.iter().copied().max().unwrap_or(0),
    );
    // A workspace written in one instant is a workspace where nothing is old.
    let span = newest.saturating_sub(oldest);

    let mut out: Vec<RecallWeight> = log
        .iter()
        .map(|obs| {
            let tier = effective_tier(obs, gates);
            let tier_f = match tier {
                TrustTier::Unverified => 0.25,
                TrustTier::AgentExtracted => 0.5,
                TrustTier::HostSigned => 0.75,
                TrustTier::HumanConfirmed => 1.0,
            };
            let frontier = if span == 0 {
                1.0
            } else {
                let w = supragnosis_core::ordering_hlc(obs).wall.saturating_sub(oldest);
                (w as f64 / span as f64) as f32
            };
            // Age is a weak signal and the oldest row keeps most of its weight: a workspace nobody
            // has written to has not become less current about its subject (Section 4.2).
            let frontier_f = 0.4 + 0.6 * frontier;
            // Lineage WITHOUT corroboration. Derived knowledge that several principals also assert
            // is not the P18 shape - one attestation and a parent is.
            let derived_alone = !obs.derived_from.is_empty() && obs.provenance.len() == 1;
            let lineage_f = if derived_alone { 0.6 } else { 1.0 };
            RecallWeight {
                observation: obs.id.clone(),
                weight: RECALL_FLOOR + (1.0 - RECALL_FLOOR) * tier_f * frontier_f * lineage_f,
                tier: tier_label(tier),
                frontier,
                derived_alone,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.weight
            .partial_cmp(&b.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.observation.cmp(&b.observation))
    });
    out.truncate(DEMOTION_CANDIDATES);
    out
}

fn fuse_rrf(lists: &[Vec<SearchHit>], limit: usize) -> Vec<SearchHit> {
    // RRF constant. Larger values flatten the advantage of top ranks (60 is the information-retrieval convention).
    const K: f32 = 60.0;

    let mut acc: HashMap<(SearchHitKind, String), (SearchHit, f32)> = HashMap::new();
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let contrib = 1.0 / (K + rank as f32 + 1.0);
            let entry = acc.entry((hit.kind, hit.id.clone())).or_insert_with(|| (hit.clone(), 0.0));
            entry.1 += contrib;
        }
    }

    let mut fused: Vec<SearchHit> = acc
        .into_values()
        .map(|(mut hit, score)| {
            hit.score = score;
            hit
        })
        .collect();
    // Ties are stable-sorted by id to guarantee determinism.
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    fused.truncate(limit);
    fused
}
