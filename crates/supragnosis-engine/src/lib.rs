//! supragnosis-engine - the service (use-case) layer.
//!
//! Deterministic logic invoked by the MCP tools: observation ingest -> entity resolution -> relation linking -> lookup/search.
//! The store is accessed only through the [`supragnosis_core::KnowledgeStore`] port.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use supragnosis_core::{
    hyperedge_id, normalize_relation_kind, now_millis, Assertions, EmbeddingProvider, Entity,
    EntityAssertion, KnowledgeStore, Observation, Provenance, Relation, RelationAssertion,
    SearchHit, SearchHitKind, StoreError, Timestamp, TraverseHit, TrustTier,
};
// Re-export the UI observability port/types - so mcp/viz can use them without depending on core directly.
pub use supragnosis_core::{Event, EventEnvelope, EventSink};

/// Ingest input (the domain input mapped from the transport DTO).
pub struct ObserveInput {
    pub content: String,
    pub workspace: Option<String>,
    pub source_ref: Option<String>,
    pub confidence: Option<f32>,
    /// Delegation chain (Principle 2): the principal that the acting host represents for this observation.
    pub on_behalf_of: Option<String>,
    /// Lineage (Principle 18): the ids of the source observations this observation was derived from.
    pub derived_from: Vec<String>,
    pub entities: Vec<EntityInput>,
    pub relations: Vec<RelationInput>,
}

pub struct EntityInput {
    pub name: String,
    pub kind: Option<String>,
    /// (Optional) Human-readable explanation of this entity.
    pub description: Option<String>,
}

pub struct RelationInput {
    pub from: String,
    pub kind: String,
    pub to: String,
    /// (Optional) Human-readable explanation of this connection.
    pub description: Option<String>,
    /// Valid-time start (Principle 4, optional). Captures retroactive observations at ingest time.
    pub valid_from: Option<Timestamp>,
    /// Valid-time end (Principle 4, optional).
    pub valid_to: Option<Timestamp>,
}

#[derive(Serialize)]
pub struct ObserveOutput {
    pub observation_id: String,
    pub entities: Vec<String>,
    pub relations: Vec<String>,
}

/// Ingest failure. Validation error messages are written so the LLM client can self-correct (Principle 21:
/// why it failed and what to do differently).
#[derive(Debug, thiserror::Error)]
pub enum ObserveError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The surface the search actually used (Principle 16, 4th revision: a response marks which
/// surface it came from, so the client can distinguish the convergence surface from recall assistance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Hybrid of keyword (convergence surface) + semantic vector (node-local recall assistance).
    Hybrid,
    /// Keyword only. Includes the state degraded by a missing embedder or a failed query embedding (Principle 19) -
    /// zero results in this mode are more likely a "recall failure" than zero results in hybrid mode.
    Keyword,
}

/// Search response: the surface used + hits.
#[derive(Serialize)]
pub struct SearchOutput {
    pub mode: SearchMode,
    pub hits: Vec<SearchHit>,
}

/// An entity + its relations (lookup response).
#[derive(Serialize)]
pub struct EntityView {
    #[serde(flatten)]
    pub entity: Entity,
    pub relations: Vec<Relation>,
}

/// Ontology graph projection (the read view for observability/visualization).
///
/// The observation log is the source of truth and this view is a **derived view** computed on top of it (Principle 1) - a
/// pure read that writes nothing. Nodes/edges carry a provenance summary (trust tier / source count) so you can see "where
/// this knowledge is supported and by how much" (Principle 2/18). Ordering is deterministic (Principle 16).
#[derive(Serialize)]
pub struct GraphView {
    /// The scoped workspace. None means all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphStats,
}

/// Graph node = entity. Carries visualization hints (type/degree/trust).
#[derive(Serialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// (Optional) Human-readable explanation of this entity - shown in the viewer inspector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The number of edges included in the graph that connect to this node (only edges whose both endpoints are in the node set).
    pub degree: usize,
    /// The number of sources (attestations) accumulated on this entity - larger when more observations back it.
    pub sources: usize,
    /// The **highest** trust tier among the sources (Principle 18) - the node's representative trust.
    pub trust_tier: TrustTier,
}

/// Graph edge = a typed relation. Carries the provenance summary and valid interval.
#[derive(Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// (Optional) Human-readable explanation of this connection - shown in the viewer inspector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust_tier: TrustTier,
    /// No annotation (None) stays as no annotation - it is not shown as 1.0 (Principle 2, 4th).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Valid interval end (Principle 4). Some means it was superseded/refuted and is no longer true now - the viewer draws it faded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

/// Graph summary metrics (the first measure of observability). BTreeMap for deterministic ordering.
#[derive(Serialize)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    /// Node count by type.
    pub type_counts: BTreeMap<String, usize>,
    /// Node count by trust tier (by representative tier).
    pub trust_counts: BTreeMap<String, usize>,
}

/// A stable string label for TrustTier (matching the serialized snake_case). Used as metric keys.
fn tier_label(t: TrustTier) -> &'static str {
    match t {
        TrustTier::Unverified => "unverified",
        TrustTier::AgentExtracted => "agent_extracted",
        TrustTier::HostSigned => "host_signed",
        TrustTier::HumanConfirmed => "human_confirmed",
    }
}

/// Hypergraph projection (the second-order structure of co-occurrence - Principle 11, "the ground of induction").
///
/// Revives the set of entities co-asserted by one observation as a single **hyperedge** - a derived view that
/// deterministically recovers from the log "what was said together" (context), which the binary-relation
/// projection discarded (Principle 1). It does not touch the storage model (binary Relation). The member set is the
/// hyperedge's identity, so multiple observations that produce the same set are deduped and accumulated as
/// attestation (sources/trust) (Principle 3/14). Node/edge order, member order, and identifiers are all deterministic (Principle 16).
#[derive(Serialize)]
pub struct HyperGraphView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    pub nodes: Vec<GraphNode>,
    pub hyperedges: Vec<HyperEdge>,
    pub stats: HyperGraphStats,
}

/// Hyperedge = the set of entities co-asserted in one observation (or several). Undirected/untyped/n-ary -
/// the dual of a binary relation (directed/typed/pair), not a replacement for it.
#[derive(Serialize)]
pub struct HyperEdge {
    /// The content address of the member set (Principle 14) - the same set has the same id no matter which observation it is derived from.
    pub id: String,
    /// The member entity ids (sorted, deterministic). Only those in the graph node set (closed hull).
    pub members: Vec<String>,
    /// The members' canonical names (canonical_name), in the same order as `members`. An id-only response is
    /// hard for the LLM to read and the viewer labels need names, so the projection carries the names too (readability).
    pub member_names: Vec<String>,
    /// arity = member count. A granularity signal (a large loose cluster is a grab-bag/split candidate - Principle 11 second-order structure).
    pub size: usize,
    /// The number of observations that co-asserted this member set - a corroboration signal (Principle 6/18).
    pub sources: usize,
    /// The highest trust tier among the provenance of the contributing observations (Principle 18).
    pub trust_tier: TrustTier,
}

/// Hypergraph summary metrics (the first measure of observability).
#[derive(Serialize)]
pub struct HyperGraphStats {
    pub node_count: usize,
    pub hyperedge_count: usize,
    /// The maximum hyperedge size (arity) - the first measure for grab-bag detection.
    pub max_size: usize,
}

pub struct Engine {
    store: Arc<dyn KnowledgeStore>,
    /// The embedding provider port (Principle 19: the probabilistic boundary). If absent, search degrades to keyword.
    embedder: Option<Arc<dyn EmbeddingProvider>>,
    /// The UI event sink (observability, optional). If absent, emit is a no-op.
    events: Option<Arc<dyn EventSink>>,
    /// The session id (footprint group key). Carried on every event emitted - defaults to "local".
    session: String,
    /// The projection write serialization lock. observe's entity upsert is read-merge-write (get -> push
    /// provenance -> put), so it is not atomic - if concurrent observations touch the same entity, attestation
    /// can be lost (deferred, architecture section 14). It was harmless with a single stdio client, but the HTTP
    /// daemon allows concurrent calls, so the write section is serialized with this lock to prevent loss.
    /// Reads (get/search/traverse/graph) stay outside the lock - kept concurrent. Full atomicity is the M3 resolution layer.
    write_guard: std::sync::Mutex<()>,
    host: String,
    default_workspace: String,
}

impl Engine {
    pub fn new(
        store: Arc<dyn KnowledgeStore>,
        host: impl Into<String>,
        default_workspace: impl Into<String>,
    ) -> Self {
        Self {
            store,
            embedder: None,
            events: None,
            session: "local".to_string(),
            write_guard: std::sync::Mutex::new(()),
            host: host.into(),
            default_workspace: default_workspace.into(),
        }
    }

    /// Attaches an embedding provider (builder). When attached, observe adds embeddings to observations and
    /// search operates as a vector+keyword hybrid. When not attached, keyword only (degrade).
    pub fn with_embedder(mut self, embedder: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Attaches a UI event sink (builder, observability). When attached, [`Engine::emit`] streams here -
    /// for the viewer's live activity log / node highlighting. When not attached, emit is a no-op.
    pub fn with_events(mut self, sink: Arc<dyn EventSink>) -> Self {
        self.events = Some(sink);
        self
    }

    /// Sets the session id (builder). Carried on every event emitted, it becomes the group key of the conversation
    /// footprint - the viewer groups "which knowledge this session used" together.
    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session = session.into();
        self
    }

    /// Emits a UI event. Does nothing if there is no sink (observability is optional).
    /// Carries the session id in the envelope (footprint group key). The caller (MCP tool handler) invokes it per
    /// intent - a side channel unrelated to the storage/resolution logic.
    pub fn emit(&self, event: Event) {
        if let Some(sink) = &self.events {
            sink.emit(&EventEnvelope {
                session: self.session.clone(),
                event,
            });
        }
    }

    fn provenance(
        &self,
        workspace: &str,
        source_ref: Option<String>,
        confidence: Option<f32>,
        on_behalf_of: Option<String>,
    ) -> Provenance {
        Provenance {
            host: self.host.clone(),
            on_behalf_of,
            workspace: workspace.to_string(),
            source_ref,
            observed_at: now_millis(),
            // Preserve no-annotation as no-annotation (Principle 2, 4th) - substituting a default (1.0) is a
            // capture loss that erases the distinction between "no assertion" and "full-confidence assertion". Interpretation is the resolution policy's job (M3).
            confidence,
            // Trust tier promotion only happens in an explicit flow (human confirmation / cross-validation) - observe uses the default.
            trust_tier: TrustTier::default(),
        }
    }

    /// Ingests a piece of knowledge: stores an immutable observation + links the provided entities/relations into the ontology.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, ObserveError> {
        // Enforce the confidence range (Principle 2: schema-level enforcement). A value once written to the
        // append-only log is permanent, so we block it before ingest. NaN is caught too, since contains is false for it.
        if let Some(c) = input.confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(ObserveError::Invalid(format!(
                    "confidence must be in the range 0.0~1.0 (received: {c}). If confidence is low, \
                     give a low value; if it cannot be evaluated, omit it"
                )));
            }
        }
        // Well-formedness validation (Principle 1: ingest validation goes only as far as well-formedness). An empty
        // directive is not a "differently spelled assertion" but a non-assertion with no referent - block it before it
        // reaches the permanent log. The notation itself is not censored: rejection is not transformation, and normalization is the projection's job.
        for e in &input.entities {
            if e.name.trim().is_empty() {
                return Err(ObserveError::Invalid(
                    "entity name is empty. an entity assertion with no name does not hold - \
                     provide a name to refer to, or drop the item"
                        .into(),
                ));
            }
            if e.kind.as_deref().is_some_and(|k| k.trim().is_empty()) {
                return Err(ObserveError::Invalid(format!(
                    "the type of entity '{}' is an empty string. an empty-type assertion is a \
                     non-holding assertion, different from leaving the type unspecified - if you don't know the type, omit type",
                    e.name
                )));
            }
        }
        for r in &input.relations {
            if r.from.trim().is_empty() || r.to.trim().is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "a relation endpoint is empty (from: {:?}, to: {:?}). a relation assertion that \
                     points to an unnamed entity does not hold - provide both endpoint entity names",
                    r.from, r.to
                )));
            }
            if normalize_relation_kind(&r.kind).is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "the relation type is empty (received: {:?} - normalizes to an empty string). \
                     provide a type whose meaning reads clearly, like depends_on / part_of",
                    r.kind
                )));
            }
        }
        let workspace = input
            .workspace
            .unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(
            &workspace,
            input.source_ref,
            input.confidence,
            input.on_behalf_of,
        );

        // Structured assertions are enclosed in the observation log **verbatim** (Principle 1: the log is the source
        // of truth and the graph is a projection - if an assertion is not in the log, the graph cannot be recovered by
        // re-projection). Normalization (kind canonicalization, etc.) is the job of the projection step below.
        let assertions = Assertions {
            entities: input
                .entities
                .iter()
                .map(|e| EntityAssertion {
                    name: e.name.clone(),
                    kind: e.kind.clone(),
                    description: e.description.clone(),
                })
                .collect(),
            relations: input
                .relations
                .iter()
                .map(|r| RelationAssertion {
                    from: r.from.clone(),
                    kind: r.kind.clone(),
                    to: r.to.clone(),
                    description: r.description.clone(),
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
        };
        let mut obs = Observation::with_assertions(input.content, prov.clone(), assertions);
        obs.derived_from = input.derived_from;
        // Embedding attachment is best-effort: a failure does not block storing the observation (Principle 19: degrade).
        // But degrade is not silent: an embedding failure at ingest time excludes this observation from semantic
        // search with no retry (until the same content is re-observed), so it leaves a trace.
        if let Some(embedder) = &self.embedder {
            match embedder.embed_one(&obs.content) {
                Ok(vec) => obs.embedding = Some(vec),
                Err(e) => tracing::warn!(
                    observation_id = %obs.id,
                    error = %e,
                    "observation embedding failed - recalled by keyword search only (degrade)"
                ),
            }
        }
        let observation_id = obs.id.clone();
        // Serialize the write section (prevents the read-merge-write race of concurrent observations' projections, see the field comment above).
        // Embedding (above, probabilistic/CPU) is left outside the lock; we lock from here. The read path is not locked.
        let _write = self
            .write_guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.store.add_observation(obs)?;

        let mut entities = Vec::new();
        for e in input.entities {
            entities.push(self.upsert_named(&workspace, &e.name, e.kind, e.description, &prov)?);
        }

        let mut relations = Vec::new();
        for r in input.relations {
            // Endpoints are named-only references here (no type/description of their own on a relation input).
            let from = self.upsert_named(&workspace, &r.from, None, None, &prov)?;
            let to = self.upsert_named(&workspace, &r.to, None, None, &prov)?;
            // kind is projected into its canonical form - so the id and the stored notation always match
            // (if only the id is normalized, different notations for the same id are left last-write-wins).
            let kind = normalize_relation_kind(&r.kind);
            let rel = Relation {
                id: Relation::make_id(&from, &kind, &to),
                from,
                to,
                kind,
                // Human-readable explanation of the connection (last-write-wins in M0; the log keeps history).
                description: r.description,
                provenance: prov.clone(),
                // Project the valid interval the client specified as-is (Principle 4 capture).
                // Derivation logic such as auto-closing valid_to on refutation is M3.
                valid_from: r.valid_from,
                valid_to: r.valid_to,
            };
            let rid = rel.id.clone();
            self.store.add_relation(rel)?;
            relations.push(rid);
        }

        Ok(ObserveOutput {
            observation_id,
            entities,
            relations,
        })
    }

    /// M0 resolution: exact match on the canonical name. If it exists, only append the source; otherwise create it.
    fn upsert_named(
        &self,
        workspace: &str,
        name: &str,
        kind: Option<String>,
        description: Option<String>,
        prov: &Provenance,
    ) -> Result<String, StoreError> {
        let id = Entity::make_id(workspace, name);
        let mut entity = self.store.get_entity(&id)?.unwrap_or_else(|| Entity {
            id: id.clone(),
            kind: kind.clone().unwrap_or_else(|| "Concept".to_string()),
            canonical_name: name.trim().to_string(),
            aliases: Vec::new(),
            description: None,
            properties: serde_json::Value::Null,
            provenance: Vec::new(),
            embedding: None,
        });
        if let Some(k) = kind {
            entity.kind = k;
        }
        // Update the explanation only when this observation actually supplies one (do not erase a prior
        // description with an omission) - last-write-wins among observations that specify it (M0).
        if let Some(d) = description {
            entity.description = Some(d);
        }
        entity.provenance.push(prov.clone());
        // Embed the entity name/aliases so semantic search reaches the node by the **meaning of its name**
        // (Principle 19: recall expansion). This fills the recall gap for nodes that observations do not mention lexically.
        // Embedding attachment is best-effort: a failure does not block storing the entity (Principle 19: degrade).
        // The name is stable, so compute it only once when absent (to minimize probabilistic adapter calls).
        // A failure is not silent - it is retried when the next observation touches this entity (since it is still
        // None), but until then it is not recalled by the meaning of its name.
        if entity.embedding.is_none() {
            if let Some(embedder) = &self.embedder {
                match embedder.embed_one(&entity_text(&entity)) {
                    Ok(vec) => entity.embedding = Some(vec),
                    Err(e) => tracing::warn!(
                        entity_id = %entity.id,
                        name = %entity.canonical_name,
                        error = %e,
                        "entity embedding failed - stored without name-meaning recall (degrade)"
                    ),
                }
            }
        }
        self.store.put_entity(entity)?;
        Ok(id)
    }

    /// Observation dereference (Principle 2/14): from an observation id returned by a search hit / derivation
    /// lineage, reach the original text, the full provenance, and the derived_from lineage - the terminus of "where did this answer come from".
    pub fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        self.store.get_observation(id)
    }

    /// Entity + relation lookup. `Ok(None)` is absence (unknown, Principle 5), `Err` is a store failure -
    /// failures are not swallowed, so the caller (the MCP surface) can distinguish and relay the two.
    pub fn get_entity(&self, id: &str) -> Result<Option<EntityView>, StoreError> {
        match self.store.get_entity(id)? {
            Some(entity) => {
                let relations = self.store.relations_of(id)?;
                Ok(Some(EntityView { entity, relations }))
            }
            None => Ok(None),
        }
    }

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
        let keyword = self.store.search(query, workspace, limit)?;

        // The query embedding is computed once and shared by the observation/entity semantic searches.
        // An embedding failure is a degrade (keyword only, Principle 19), but a store failure is an Err -
        // the absence/failure of the probabilistic adapter and the failure of the deterministic store are different events.
        let qvec = self.embedder.as_ref().and_then(|e| e.embed_one(query).ok());
        // mode is "did it reference the semantic surface" - even if semantic recall is zero, it did reference it, so it is
        // hybrid (the epistemic weight of zero results differs by mode, Principle 5/16 4th).
        let mode = if qvec.is_some() {
            SearchMode::Hybrid
        } else {
            SearchMode::Keyword
        };
        let (semantic_obs, semantic_ent) = match &qvec {
            Some(v) => (
                self.store.search_semantic(v, workspace, limit)?,
                self.store.search_semantic_entities(v, workspace, limit)?,
            ),
            None => (Vec::new(), Vec::new()),
        };

        // If there is no semantic recall (no embedder / not embedded), use the keyword ranking as-is; otherwise RRF-fuse.
        let fused = if semantic_obs.is_empty() && semantic_ent.is_empty() {
            keyword
        } else {
            fuse_rrf(&[keyword, semantic_obs, semantic_ent], limit)
        };

        // Graph enrichment: fill the spare slots with the 1-hop neighbors of the top entity hits.
        let hits = self.enrich_with_graph(fused, workspace, limit)?;
        Ok(SearchOutput { mode, hits })
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

    /// Traverses neighbors from an entity following the relation direction (from->to) up to `max_depth` hops.
    pub fn traverse(
        &self,
        id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError> {
        self.store.traverse(id, max_depth.max(1), limit)
    }

    /// This node's default workspace (referenced when an MCP resource builds a concrete URI).
    pub fn default_workspace(&self) -> &str {
        &self.default_workspace
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
    pub fn graph(&self, workspace: Option<&str>) -> Result<GraphView, StoreError> {
        let entities = self.store.all_entities(workspace)?;
        let relations = self.store.all_relations(workspace)?;

        let node_ids: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();

        // degree is counted only against the edges actually included in the graph.
        let mut degree: HashMap<String, usize> = HashMap::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for r in &relations {
            if node_ids.contains(r.from.as_str()) && node_ids.contains(r.to.as_str()) {
                *degree.entry(r.from.clone()).or_default() += 1;
                *degree.entry(r.to.clone()).or_default() += 1;
                edges.push(GraphEdge {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    kind: r.kind.clone(),
                    description: r.description.clone(),
                    trust_tier: r.provenance.trust_tier,
                    confidence: r.provenance.confidence,
                    valid_to: r.valid_to,
                });
            }
        }

        let mut type_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut trust_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut nodes: Vec<GraphNode> = entities
            .iter()
            .map(|e| {
                // Representative trust = the highest tier among the sources (Principle 18). Default if there are no sources.
                let trust = e
                    .provenance
                    .iter()
                    .map(|p| p.trust_tier)
                    .max()
                    .unwrap_or_default();
                *type_counts.entry(e.kind.clone()).or_default() += 1;
                *trust_counts.entry(tier_label(trust).to_string()).or_default() += 1;
                GraphNode {
                    id: e.id.clone(),
                    name: e.canonical_name.clone(),
                    kind: e.kind.clone(),
                    description: e.description.clone(),
                    degree: degree.get(&e.id).copied().unwrap_or(0),
                    sources: e.provenance.len(),
                    trust_tier: trust,
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
        Ok(GraphView {
            workspace: workspace.map(String::from),
            nodes,
            edges,
            stats,
        })
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
        let entities = self.store.all_entities(workspace)?;
        let node_ids: HashSet<&str> = entities.iter().map(|e| e.id.as_str()).collect();
        // id -> canonical name (readability: hyperedge members are carried as names too).
        let name_by_id: HashMap<&str, &str> = entities
            .iter()
            .map(|e| (e.id.as_str(), e.canonical_name.as_str()))
            .collect();

        // Per-observation co-occurrence set -> accumulate hyperedges, deduping by member set.
        // Value: (sorted members, sources count, highest trust among contributing observations).
        let mut acc: HashMap<String, (Vec<String>, usize, TrustTier)> = HashMap::new();
        for obs in self.store.all_observations(workspace)? {
            let ws = obs.workspace();
            // The entities co-asserted by an observation: entity assertions + both endpoints of relations. Resolved to
            // canonical ids and keeping only those in the graph node set (closed hull - the same discipline as graph()'s edge closure).
            // BTreeSet gives dedup + sort at once (independent of arrival order - Principle 16).
            let mut members: BTreeSet<String> = BTreeSet::new();
            for e in &obs.assertions.entities {
                let id = Entity::make_id(ws, &e.name);
                if node_ids.contains(id.as_str()) {
                    members.insert(id);
                }
            }
            for r in &obs.assertions.relations {
                for name in [&r.from, &r.to] {
                    let id = Entity::make_id(ws, name);
                    if node_ids.contains(id.as_str()) {
                        members.insert(id);
                    }
                }
            }
            if members.len() < HYPEREDGE_MIN_SIZE {
                continue; // A degenerate set (single/0 members) is not a hyperedge.
            }
            let members: Vec<String> = members.into_iter().collect();
            let id = hyperedge_id(&members);
            // This observation's representative trust = the highest tier among its provenance (Principle 18).
            let obs_trust = obs
                .provenance
                .iter()
                .map(|p| p.trust_tier)
                .max()
                .unwrap_or_default();
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
                HyperEdge {
                    size: members.len(),
                    member_names,
                    id,
                    members,
                    sources,
                    trust_tier,
                }
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

        let mut nodes: Vec<GraphNode> = entities
            .iter()
            .map(|e| {
                let trust = e
                    .provenance
                    .iter()
                    .map(|p| p.trust_tier)
                    .max()
                    .unwrap_or_default();
                GraphNode {
                    id: e.id.clone(),
                    name: e.canonical_name.clone(),
                    kind: e.kind.clone(),
                    description: e.description.clone(),
                    degree: hyper_degree.get(&e.id).copied().unwrap_or(0),
                    sources: e.provenance.len(),
                    trust_tier: trust,
                }
            })
            .collect();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));

        let stats = HyperGraphStats {
            node_count: nodes.len(),
            hyperedge_count: hyperedges.len(),
            max_size,
        };
        Ok(HyperGraphView {
            workspace: workspace.map(String::from),
            nodes,
            hyperedges,
            stats,
        })
    }
}

/// The decay applied to neighbors in graph enrichment. A neighbor is a weaker signal than a seed (direct match), so it
/// is ranked at half the seed score to keep it below primary hits, while allowing the neighbor of a strong seed to rise
/// above a weak primary hit (reflecting graph proximity in the ranking).
const GRAPH_ENRICH_DECAY: f32 = 0.5;
/// The cap on the number of seeds (top entity hits) whose neighbors are expanded - cost/precision control (bounded so an
/// active node cannot flood the results).
const GRAPH_ENRICH_SEEDS: usize = 5;

/// The minimum hyperedge size (arity). 1 (a single entity) / 0 is not co-occurrence but a degenerate state where a
/// hyperedge does not hold (Principle 11 second-order structure caveat). 2 converges on a binary co-mention but is still
/// a "said together" context, so it is included.
const HYPEREDGE_MIN_SIZE: usize = 2;

/// The text to embed for an entity: canonical name + aliases (if any). Opens semantic recall by the meaning of the name.
/// Since aliases hold notation variants, embedding them together widens the reach to other notations of the same target.
fn entity_text(entity: &Entity) -> String {
    if entity.aliases.is_empty() {
        entity.canonical_name.clone()
    } else {
        format!("{} {}", entity.canonical_name, entity.aliases.join(" "))
    }
}

/// Reciprocal Rank Fusion. Fuses rankings on different scales (keyword score vs cosine similarity) by rank alone,
/// combining them without scale normalization. The same (kind, id) has its contributions summed.
/// A deterministic function (Principle 16) - the same input ranks give the same result on any node.
fn fuse_rrf(lists: &[Vec<SearchHit>], limit: usize) -> Vec<SearchHit> {
    // RRF constant. Larger values flatten the advantage of top ranks (60 is the information-retrieval convention).
    const K: f32 = 60.0;

    let mut acc: HashMap<(SearchHitKind, String), (SearchHit, f32)> = HashMap::new();
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let contrib = 1.0 / (K + rank as f32 + 1.0);
            let entry = acc
                .entry((hit.kind, hit.id.clone()))
                .or_insert_with(|| (hit.clone(), 0.0));
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

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_store::InMemoryStore;

    #[test]
    fn observe_then_get_and_search() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "test-host", "ws1");

        let out = engine
            .observe(ObserveInput {
                content: "rmcp is the official Rust MCP SDK".into(),
                workspace: None,
                source_ref: Some("docs/architecture.md".into()),
                confidence: None,
                on_behalf_of: Some("ashon".into()),
                derived_from: vec![],
                entities: vec![
                    EntityInput { description: None,
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                    EntityInput { description: None,
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                ],
                relations: vec![RelationInput { description: None,
                    from: "supragnosis".into(),
                    kind: "depends_on".into(),
                    to: "rmcp".into(),
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .unwrap();

        assert_eq!(out.entities.len(), 2);
        assert_eq!(out.relations.len(), 1);

        // Re-lookup by deterministic id -> relations come along too.
        let rmcp_id = Entity::make_id("ws1", "rmcp");
        let view = engine.get_entity(&rmcp_id).unwrap().expect("entity exists");
        assert_eq!(view.entity.canonical_name, "rmcp");
        assert_eq!(view.entity.kind, "Tool");
        assert_eq!(view.relations.len(), 1);

        // Re-ingest converges to the same entity because of content addressing (only sources accumulate).
        let out = engine.search("rust", Some("ws1"), 10).unwrap();
        assert!(
            !out.hits.is_empty(),
            "keyword search should find the observation"
        );
        // An engine without an embedder has mode keyword (degrade marker, Principle 16 4th).
        assert_eq!(out.mode, SearchMode::Keyword);

        // Not visible from another workspace.
        assert!(engine
            .search("rust", Some("other"), 10)
            .unwrap()
            .hits
            .is_empty());
    }

    /// Notation variance in the relation kind (depends_on/dependsOn/depends-on) converges to the same single edge,
    /// while the observation log keeps the assertion **in its original notation** (Principle 1).
    #[test]
    fn relation_kind_variants_converge_and_assertions_are_logged() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "test-host", "ws1");

        let mut relation_ids = Vec::new();
        for kind in ["depends_on", "dependsOn", "depends-on"] {
            let out = engine
                .observe(ObserveInput {
                    content: format!("supragnosis {kind} rmcp"),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![],
                    relations: vec![RelationInput { description: None,
                        from: "supragnosis".into(),
                        kind: kind.into(),
                        to: "rmcp".into(),
                        valid_from: None,
                        valid_to: None,
                    }],
                })
                .unwrap();
            relation_ids.push(out.relations[0].clone());
        }
        // All three notations yield the same relation id.
        assert_eq!(relation_ids[0], relation_ids[1]);
        assert_eq!(relation_ids[0], relation_ids[2]);

        // The projection has only the single canonical kind.
        let sup_id = Entity::make_id("ws1", "supragnosis");
        let view = engine.get_entity(&sup_id).unwrap().unwrap();
        assert_eq!(view.relations.len(), 1);
        assert_eq!(view.relations[0].kind, "depends_on");
    }

    /// Structured assertions are enclosed in the observation log and reflected in the id - carrying different assertions
    /// on the same text is not lost to dedup (Principle 1: the graph can be reconstructed from the log alone).
    #[test]
    fn observations_carry_assertions_in_log() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "test-host", "ws1");

        let observe_with_kind = |kind: &str| {
            engine
                .observe(ObserveInput {
                    content: "supragnosis is written in Rust".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput { description: None,
                        name: "supragnosis".into(),
                        kind: Some(kind.into()),
                    }],
                    relations: vec![],
                })
                .unwrap()
        };
        let first = observe_with_kind("Tool");
        let second = observe_with_kind("Project");

        // Even with the same text, different assertions mean a different observation - the trace of type reassignment stays in the log.
        assert_ne!(first.observation_id, second.observation_id);
        let logged = store.get_observation(&second.observation_id).unwrap().unwrap();
        assert_eq!(logged.assertions.entities.len(), 1);
        assert_eq!(logged.assertions.entities[0].kind.as_deref(), Some("Project"));
    }

    /// Principle 2 schema-level enforcement: an out-of-range confidence is rejected before it reaches the append-only
    /// log, and the error message guides self-correction (Principle 21).
    #[test]
    fn confidence_out_of_range_is_rejected() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let observe_with_conf = |conf: f32| {
            engine.observe(ObserveInput {
                content: "fact".into(),
                workspace: None,
                source_ref: None,
                confidence: Some(conf),
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![],
            })
        };
        for bad in [-0.1f32, 1.5, f32::NAN] {
            let err = observe_with_conf(bad).err().expect("out of range is rejected");
            assert!(
                err.to_string().contains("0.0~1.0"),
                "there should be a self-correction hint: {err}"
            );
        }
        // Boundary values are valid.
        assert!(observe_with_conf(0.0).is_ok());
        assert!(observe_with_conf(1.0).is_ok());
    }

    /// Principle 1 well-formedness validation: an empty directive (name/type/endpoint/kind) is a non-assertion, so it is
    /// rejected before it reaches the permanent log. Notation variance (a name surrounded by whitespace, a separator-varied
    /// kind), by contrast, is content and passes - rejection goes only as far as well-formedness; notation is not censored.
    #[test]
    fn formless_assertions_are_rejected_before_logging() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let observe = |entities: Vec<EntityInput>, relations: Vec<RelationInput>| {
            engine.observe(ObserveInput {
                content: "fact".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities,
                relations,
            })
        };
        let ent = |name: &str, kind: Option<&str>| EntityInput { description: None,
            name: name.into(),
            kind: kind.map(String::from),
        };
        let rel = |from: &str, kind: &str, to: &str| RelationInput { description: None,
            from: from.into(),
            kind: kind.into(),
            to: to.into(),
            valid_from: None,
            valid_to: None,
        };

        // Non-assertions: empty/whitespace name, empty type, empty endpoint, kind that normalizes to empty - all rejected.
        for (label, entities, relations) in [
            ("empty name", vec![ent("", None)], vec![]),
            ("whitespace name", vec![ent("   ", None)], vec![]),
            ("empty type", vec![ent("thing", Some(""))], vec![]),
            ("empty from", vec![], vec![rel("", "depends_on", "b")]),
            ("whitespace to", vec![], vec![rel("a", "depends_on", "  ")]),
            ("empty kind", vec![], vec![rel("a", "", "b")]),
            ("separators-only kind", vec![], vec![rel("a", "-- __ ", "b")]),
        ] {
            let err = observe(entities, relations)
                .err()
                .unwrap_or_else(|| panic!("{label} should be rejected"));
            assert!(
                matches!(err, ObserveError::Invalid(_)),
                "{label}: should be a validation error: {err}"
            );
        }

        // Notation variance is content - it passes (normalization/preservation is the job of the log and projection).
        assert!(observe(
            vec![ent("  Padded Name  ", Some("Tool"))],
            vec![rel("a", "Depends-On", "b")],
        )
        .is_ok());
    }

    /// Principle 4 capture: the valid interval of a retroactive observation ("it was true until last month") is carried in
    /// both the observation log (assertion) and the projection (relation). If the surface cannot receive it, it is not in
    /// the log, and if it is not in the log it cannot be recovered by re-projection - the reason capture cannot be deferred.
    #[test]
    fn relation_valid_interval_is_captured_in_log_and_projection() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "h", "ws1");

        let observe_with_interval = |valid_to: Option<Timestamp>| {
            engine
                .observe(ObserveInput {
                    content: "kim led team A until last month".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![],
                    relations: vec![RelationInput { description: None,
                        from: "kim".into(),
                        kind: "leads".into(),
                        to: "team A".into(),
                        valid_from: Some(100),
                        valid_to,
                    }],
                })
                .unwrap()
        };
        let out = observe_with_interval(Some(200));

        // Log: the valid interval is enclosed in the assertion verbatim.
        let logged = store.get_observation(&out.observation_id).unwrap().unwrap();
        assert_eq!(logged.assertions.relations[0].valid_from, Some(100));
        assert_eq!(logged.assertions.relations[0].valid_to, Some(200));

        // Projection: the relation carries the valid interval.
        let kim = Entity::make_id("ws1", "kim");
        let rels = store.relations_of(&kim).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].valid_from, Some(100));
        assert_eq!(rels[0].valid_to, Some(200));

        // A different valid interval is a different assertion - a different observation id (part of content identity).
        let out2 = observe_with_interval(None);
        assert_ne!(out.observation_id, out2.observation_id);
    }

    /// Principle 3: on re-observing the same content, the log preserves all attestations - it prevents the "source-of-truth
    /// inversion" regression where only the entity provenance accumulated while the log kept just the last one
    /// (re-projecting the log must be able to recover the graph's attestation).
    #[test]
    fn log_retains_all_attestations_on_reobservation() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "host", "ws1");

        let observe = |behalf: &str, conf: f32| {
            engine
                .observe(ObserveInput {
                    content: "repeated fact".into(),
                    workspace: None,
                    source_ref: None,
                    confidence: Some(conf),
                    on_behalf_of: Some(behalf.into()),
                    derived_from: vec![],
                    entities: vec![EntityInput { description: None,
                        name: "thing".into(),
                        kind: None,
                    }],
                    relations: vec![],
                })
                .unwrap()
        };
        let first = observe("alice", 0.9);
        let second = observe("bob", 0.1);
        assert_eq!(first.observation_id, second.observation_id, "content-address dedup");

        let logged = store.get_observation(&first.observation_id).unwrap().unwrap();
        let entity = store
            .get_entity(&Entity::make_id("ws1", "thing"))
            .unwrap()
            .unwrap();

        // The log and the projection carry the same attestation count - the log is the source of truth.
        assert_eq!(logged.provenance.len(), 2, "two attestations preserved in the log");
        assert_eq!(entity.provenance.len(), 2);
        let behalfs: Vec<Option<String>> = logged
            .provenance
            .iter()
            .map(|p| p.on_behalf_of.clone())
            .collect();
        assert!(
            behalfs.contains(&Some("alice".into())) && behalfs.contains(&Some("bob".into())),
            "the first observation's provenance must not be destroyed: {behalfs:?}"
        );
    }

    fn observe_text(engine: &Engine, content: &str) {
        engine
            .observe(ObserveInput {
                content: content.into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![],
            })
            .unwrap();
    }

    /// Recall regression (Appendix B): with an embedder attached, hybrid search recalls observations that keyword
    /// substring match misses via the semantic (lexical-overlap) path. Contrasted with degrade (no embedder).
    #[test]
    fn hybrid_search_adds_semantic_recall() {
        use supragnosis_embed::HashingEmbedder;

        let store = Arc::new(InMemoryStore::new());
        let hybrid = Engine::new(store.clone(), "h", "ws")
            .with_embedder(Arc::new(HashingEmbedder::default()));

        observe_text(&hybrid, "rust compiler emits fast native binaries");
        observe_text(&hybrid, "python interpreter runs bytecode");
        observe_text(&hybrid, "banana bread recipe with walnuts");

        // The query is not a substring of any observation (word order/form differ).
        let q = "native binary compiler";

        // Keyword only (same store, no embedder) misses this query.
        let keyword_only = Engine::new(store.clone(), "h", "ws");
        let keyword_out = keyword_only.search(q, Some("ws"), 10).unwrap();
        assert!(
            keyword_out.hits.is_empty(),
            "substring keyword search should miss this query"
        );
        assert_eq!(keyword_out.mode, SearchMode::Keyword, "degrade is marked keyword");

        // Hybrid recalls the lexically overlapping rust observation at the top.
        let out = hybrid.search(q, Some("ws"), 10).unwrap();
        assert_eq!(out.mode, SearchMode::Hybrid, "marked hybrid when the semantic surface is referenced");
        let hits = out.hits;
        assert!(
            !hits.is_empty(),
            "hybrid search should recall via embedding"
        );
        assert!(
            hits[0].snippet.contains("native"),
            "semantic top hit should be the rust observation, got {:?}",
            hits.first()
        );
    }

    /// Principle 19 degrade: even if the embedding adapter fails on every call, ingest is not blocked
    /// (best-effort attachment - a failure is reported only via the log), and search degrades to keyword only
    /// while marking that fact via mode.
    #[test]
    fn embed_failure_degrades_without_blocking_ingest() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};

        struct FailingEmbedder;
        impl EmbeddingProvider for FailingEmbedder {
            fn dimensions(&self) -> usize {
                3
            }
            fn id(&self) -> String {
                "failing-3".into()
            }
            fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                Err(EmbedError::Provider("simulated failure".into()))
            }
        }

        let store = Arc::new(InMemoryStore::new());
        let engine =
            Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(FailingEmbedder));

        let out = engine
            .observe(ObserveInput {
                content: "rust compiles fast".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { description: None,
                    name: "rust".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
            })
            .expect("an embedding failure must not block ingest (Principle 19: degrade)");

        // Both the observation and the entity are stored without embeddings.
        let obs = store.get_observation(&out.observation_id).unwrap().unwrap();
        assert!(obs.embedding.is_none());
        let ent = store.get_entity(&out.entities[0]).unwrap().unwrap();
        assert!(ent.embedding.is_none());

        // The query embedding also fails, so search degrades to keyword but still works.
        let found = engine.search("rust", Some("ws1"), 10).unwrap();
        assert_eq!(found.mode, SearchMode::Keyword);
        assert!(!found.hits.is_empty());
    }

    /// Graph projection: turns the entities/relations created by observations back into a node-link view.
    /// Verifies workspace scoping, the closed graph (orphan edges excluded), degree/stats, and deterministic order.
    #[test]
    fn graph_projection_nodes_edges_stats() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "ws1");

        // ws1: supragnosis --depends_on--> rmcp (2 entities, 1 relation).
        engine
            .observe(ObserveInput {
                content: "supragnosis depends on rmcp".into(),
                workspace: Some("ws1".into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { description: None,
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                    EntityInput { description: None,
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                ],
                relations: vec![RelationInput { description: None,
                    from: "supragnosis".into(),
                    kind: "depends_on".into(),
                    to: "rmcp".into(),
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .unwrap();

        // Knowledge in another workspace - must not leak into the ws1 graph.
        engine
            .observe(ObserveInput {
                content: "unrelated".into(),
                workspace: Some("other".into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { description: None,
                    name: "elsewhere".into(),
                    kind: None,
                }],
                relations: vec![],
            })
            .unwrap();

        let g = engine.graph(Some("ws1")).unwrap();
        assert_eq!(g.stats.node_count, 2, "ws1 has 2 nodes");
        assert_eq!(g.stats.edge_count, 1, "ws1 has 1 edge");

        // Nodes are deterministically sorted by id.
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted, "nodes are in ascending id order (determinism)");

        // The degree of each relation endpoint is 1.
        for n in &g.nodes {
            assert_eq!(n.degree, 1, "each node is connected to 1 relation: {}", n.name);
        }

        // The edge is depends_on, and both endpoints are in the node set.
        let e = &g.edges[0];
        assert_eq!(e.kind, "depends_on");
        assert!(ids.contains(&e.from.as_str()) && ids.contains(&e.to.as_str()));

        // Type distribution.
        assert_eq!(g.stats.type_counts.get("Project"), Some(&1));
        assert_eq!(g.stats.type_counts.get("Tool"), Some(&1));

        // Workspace isolation: no entity from other.
        assert!(
            g.nodes.iter().all(|n| n.name != "elsewhere"),
            "a node from another workspace must not leak"
        );

        // With all (None), other is included too, for 3 nodes.
        assert_eq!(engine.graph(None).unwrap().stats.node_count, 3);
    }

    /// workspaces(): returns the workspaces where knowledge exists, deduped and sorted (Principle 16).
    #[test]
    fn workspaces_lists_distinct_sorted() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "alpha");

        assert!(engine.workspaces().unwrap().is_empty(), "an empty state is an empty list");

        let observe_in = |ws: &str, name: &str| {
            engine
                .observe(ObserveInput {
                    content: format!("{name} in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput { description: None,
                        name: name.into(),
                        kind: None,
                    }],
                    relations: vec![],
                })
                .unwrap();
        };
        // Deliberately shuffle the arrival order and re-ingest the same ws.
        observe_in("gamma", "x");
        observe_in("alpha", "y");
        observe_in("gamma", "z");

        // Dedup + sort (independent of arrival order).
        assert_eq!(engine.workspaces().unwrap(), vec!["alpha", "gamma"]);
    }

    /// Hypergraph: the entities co-asserted by one observation are recovered as a single hyperedge -
    /// even with no binary relation (just co-mention of entities), context becomes structure (Principle 11 second-order structure).
    #[test]
    fn hypergraph_recovers_co_assertion() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "ws1");
        engine
            .observe(ObserveInput {
                content: "A, B, C were discussed together".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { description: None, name: "A".into(), kind: None },
                    EntityInput { description: None, name: "B".into(), kind: None },
                    EntityInput { description: None, name: "C".into(), kind: None },
                ],
                relations: vec![], // no binary relation - co-mention only
            })
            .unwrap();

        let hg = engine.hypergraph(Some("ws1")).unwrap();
        assert_eq!(hg.stats.node_count, 3);
        assert_eq!(hg.stats.hyperedge_count, 1, "three entities into one hyperedge");
        assert_eq!(hg.hyperedges[0].size, 3);
        assert_eq!(hg.stats.max_size, 3);
        // Members are sorted entity ids (deterministic, Principle 16).
        let mut expect: Vec<String> =
            ["A", "B", "C"].iter().map(|n| Entity::make_id("ws1", n)).collect();
        expect.sort();
        assert_eq!(hg.hyperedges[0].members, expect);
        // Member names are carried too (readability) - not id-only.
        let mut names = hg.hyperedges[0].member_names.clone();
        names.sort();
        assert_eq!(names, vec!["A", "B", "C"]);
        // The id is the content address of the member set (matches core).
        assert_eq!(hg.hyperedges[0].id, hyperedge_id(&expect));
    }

    /// Different observations (different content) that produce the same member set are deduped into a single
    /// hyperedge and sources accumulate (Principle 3/14: the member set is identity, observations are attestation).
    #[test]
    fn hypergraph_dedup_by_member_set_accumulates_sources() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "ws1");
        let observe_xy = |content: &str| {
            engine
                .observe(ObserveInput {
                    content: content.into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![
                        EntityInput { description: None, name: "X".into(), kind: None },
                        EntityInput { description: None, name: "Y".into(), kind: None },
                    ],
                    relations: vec![],
                })
                .unwrap();
        };
        observe_xy("first mention of X and Y");
        observe_xy("second, differently worded mention of X with Y");

        let hg = engine.hypergraph(Some("ws1")).unwrap();
        assert_eq!(hg.hyperedges.len(), 1, "same member set -> one hyperedge");
        assert_eq!(hg.hyperedges[0].sources, 2, "two observations accumulate as attestation");
    }

    /// Size < 2 is not a hyperedge (degenerate). Relation endpoints also contribute to members -
    /// a co-occurrence hyperedge stands from relations alone, with no entity assertion. An orphan node has degree 0.
    #[test]
    fn hypergraph_min_size_and_relation_endpoints() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "ws1");
        // Single-entity observation - degenerate (not a hyperedge).
        engine
            .observe(ObserveInput {
                content: "just P".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { description: None, name: "P".into(), kind: None }],
                relations: vec![],
            })
            .unwrap();
        // Two relations - endpoints {M, N, O} are the co-occurrence of one observation.
        engine
            .observe(ObserveInput {
                content: "M relates to N and O".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![
                    RelationInput { description: None,
                        from: "M".into(),
                        kind: "relates_to".into(),
                        to: "N".into(),
                        valid_from: None,
                        valid_to: None,
                    },
                    RelationInput { description: None,
                        from: "M".into(),
                        kind: "relates_to".into(),
                        to: "O".into(),
                        valid_from: None,
                        valid_to: None,
                    },
                ],
            })
            .unwrap();

        let hg = engine.hypergraph(Some("ws1")).unwrap();
        assert_eq!(hg.stats.node_count, 4, "four nodes P,M,N,O");
        assert_eq!(hg.hyperedges.len(), 1, "P is degenerate, only the relation observation is a hyperedge");
        assert_eq!(hg.hyperedges[0].size, 3);
        let members = &hg.hyperedges[0].members;
        for n in ["M", "N", "O"] {
            assert!(members.contains(&Entity::make_id("ws1", n)), "{n} should be a member");
        }
        // An orphan node (in no hyperedge) has hyperedge-degree 0.
        let p = Entity::make_id("ws1", "P");
        assert_eq!(hg.nodes.iter().find(|n| n.id == p).unwrap().degree, 0);
    }

    /// The hypergraph is scoped by workspace and reproduces deterministically for the same state (Principle 16).
    /// The hyperedge-degree of a node spanning multiple hyperedges (contexts) is carried in degree (hub signal).
    #[test]
    fn hypergraph_scoped_deterministic_and_hub_degree() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store, "h", "w");
        let observe_pair = |ws: &str, a: &str, b: &str, content: &str| {
            engine
                .observe(ObserveInput {
                    content: content.into(),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![
                        EntityInput { description: None, name: a.into(), kind: None },
                        EntityInput { description: None, name: b.into(), kind: None },
                    ],
                    relations: vec![],
                })
                .unwrap();
        };
        // H appears in two contexts in common -> a hub.
        observe_pair("w", "H", "A", "H with A");
        observe_pair("w", "H", "B", "H with B");
        // Another workspace - must not leak.
        observe_pair("other", "Z", "Q", "elsewhere Z with Q");

        let hg = engine.hypergraph(Some("w")).unwrap();
        assert_eq!(hg.hyperedges.len(), 2, "{{H,A}}, {{H,B}}");
        assert_eq!(hg.stats.node_count, 3, "only H,A,B (other isolated)");
        assert!(hg.nodes.iter().all(|n| n.name != "Z"), "no leakage of nodes from another ws");
        // H belongs to two hyperedges -> degree 2.
        let h = Entity::make_id("w", "H");
        assert_eq!(hg.nodes.iter().find(|n| n.id == h).unwrap().degree, 2);
        // Determinism: computing twice gives identical serialization.
        let s1 = serde_json::to_string(&engine.hypergraph(Some("w")).unwrap()).unwrap();
        let s2 = serde_json::to_string(&engine.hypergraph(Some("w")).unwrap()).unwrap();
        assert_eq!(s1, s2);
        // Nodes in ascending id order (determinism).
        let ids: Vec<&str> = hg.nodes.iter().map(|n| n.id.as_str()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }
}
