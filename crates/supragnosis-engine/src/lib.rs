//! supragnosis-engine - the service (use-case) layer.
//!
//! Deterministic logic invoked by the MCP tools: observation ingest -> entity resolution -> relation linking -> lookup/search.
//! The store is accessed only through the [`supragnosis_core::KnowledgeStore`] port.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use supragnosis_core::{
    evaluated_tier, hyperedge_id, normalize_relation_kind, ordering_hlc, verdict_grant_ceiling,
    AssertionStore, Assertions, BeliefCandidate, Clock, EmbeddingProvider, Entity, EntityAssertion,
    Hlc, KnowledgeStore, Observation, ProposalEventAssertion, ProposalEventKind, Provenance,
    Relation, RelationAssertion, ResolutionPolicy, SearchHit, SearchHitKind, StoreError,
    SystemClock, TierWeighted, Timestamp, TraverseHit, TrustTier, TypeDefAssertion,
    VERDICT_SURFACE_AGENT, VERDICT_SURFACE_CONSOLE, VERDICT_SURFACE_PREFIX,
};
// Re-export the UI observability port/types - so mcp/viz can use them without depending on core directly.
pub use supragnosis_core::{Event, EventEnvelope, EventSink, TypeTarget};

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

/// One T-Box type definition to record (Principle 8/11).
pub struct TypeDefInput {
    pub target: TypeTarget,
    pub name: String,
    pub description: String,
}

/// define_type ingest input. Records type-vocabulary definitions as an observation (Principle 1/23).
pub struct DefineTypeInput {
    pub workspace: Option<String>,
    pub source_ref: Option<String>,
    pub on_behalf_of: Option<String>,
    pub defs: Vec<TypeDefInput>,
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

/// An entity + its relations (lookup response). `entity.kind` carries the POLICY-selected belief
/// (the view is a projection; the log keeps every assertion), and the belief overlay fields say
/// whether that choice was contested and what else was asserted (resolution.md Section 4.2).
#[derive(Serialize)]
pub struct EntityView {
    #[serde(flatten)]
    pub entity: Entity,
    pub relations: Vec<Relation>,
    /// The representative effective tier over supporting observations (resolution.md Section 3).
    pub effective_tier: TrustTier,
    /// True when the kind winner was decided by recency alone among tier-tied values (R6).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub contested: bool,
    /// Surviving non-winning kind values with their effective tiers + one asserting observation (R7).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub competitors: Vec<Competitor>,
    /// The observation that asserted the winning kind - the mediation handle for confirming it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_source: Option<String>,
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

/// A projected T-Box type definition (the workspace glossary entry - Principle 8/11).
#[derive(Serialize)]
pub struct TypeDefView {
    /// The workspace this definition belongs to. Part of the identity of a type, not a label on it:
    /// P11 fixes the T-Box's scope AT the workspace ("there is no global domain T-Box"), so two
    /// workspaces that both define `Widget` have defined two different things, and only an explicit
    /// alignment assertion connects them. An all-workspaces read is the UNION of per-workspace
    /// glossaries; it is not one glossary.
    pub workspace: String,
    pub target: TypeTarget,
    pub name: String,
    /// The policy-selected definition (M3a policy over description candidates - resolution-identity.md
    /// Section 6). The full history stays in the log.
    pub description: String,
    /// Number of observations that defined this type - a corroboration signal.
    pub sources: usize,
    /// Highest effective trust tier among the defining observations (Principle 18).
    pub trust_tier: TrustTier,
    /// True when distinct definitions survive at a tied top effective tier - the winner stood on
    /// recency alone, so this type invites mediation (IR5, same criterion as an entity kind - R6).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contested: bool,
    /// The non-winning definitions still asserted for this type (IR5, conflicts stay queryable - R7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub competitors: Vec<Competitor>,
    /// The observation asserting the winning definition - the mediation handle (confirm = promote it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def_source: Option<String>,
}

/// Result of a workspace re-materialization ([`Engine::reproject`]).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReprojectReport {
    pub observations: usize,
    pub entities: usize,
    pub relations: usize,
}

/// What a workspace re-key moved, skipped, and why.
#[derive(Serialize, Debug, Default)]
pub struct RekeyReport {
    /// Knowledge observations re-created under the target workspace.
    pub moved: usize,
    /// Already present there from an earlier run (the lineage says so) - a re-key is idempotent.
    pub already: usize,
    /// Proposal events left behind. Their payloads name SOURCE-workspace entity ids, which do not
    /// exist in the target, so carrying them over would import proposals that are permanently
    /// blocked on referential integrity - the disease, not the cure.
    pub skipped_proposal_events: usize,
}

/// Grab-bag detection threshold: a hyperedge with this many members is flagged as a loose co-occurrence
/// context (a split/refine candidate, Principle 11). Tunable.
const CURATION_GRAB_BAG_MIN: usize = 10;

/// The conservative merge band floor (resolution-identity.md Section 3): embedding cosine at or
/// above this makes a distinct-name entity pair a merge candidate. Deliberately high - a candidate
/// is a hypothesis for review, and the gate (not the score) commits (Principle 15/19).
const SIM_CANDIDATE: f32 = 0.85;
/// Nearest entities considered per node when scanning the merge band (cost bound - the recall aid
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

/// Read-only curation signals (Principle 7 consolidation, "generate, do not commit"): candidates a
/// human/agent might act on to tidy the knowledge, computed as a pure deterministic projection
/// (Principle 16). Surfacing them commits nothing and needs no gate - they are NOT proposals or edits,
/// just "here is what looks worth reviewing". See docs/proposal-workflow.md Section 14.
#[derive(Serialize)]
pub struct CurationReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Entities colliding under the SAME normalization the entity id uses (`trim`+`lowercase`).
    /// Within one workspace this is necessarily empty - two such entities would share an id and be
    /// one row - so it reports only **cross-workspace** name collisions, and only when the report is
    /// built unscoped. Orthographic duplicates inside a workspace are `name_variants` below.
    pub duplicates: Vec<DuplicateGroup>,
    /// Oversized co-occurrence contexts - loose grab-bags, split/refine candidates (Principle 11).
    pub grab_bags: Vec<GrabBag>,
    /// Entities with no relation in the graph - observed alone, weakly integrated.
    pub orphans: Vec<CurationNode>,
    /// Live single-valued-field conflicts (resolution.md Section 4.2): every node whose kind has
    /// surviving competitors, tier-tied (contested - mediation invited) first. This is the
    /// Principle 6 introspection query - read-only, commits nothing (I18).
    pub contradictions: Vec<CurationConflict>,
    /// Contradictory accepted entity-merges (Principle 6): sets of entities whose merged proposals
    /// fold into EACH OTHER (a cycle). The projection still resolves them deterministically (P16 -
    /// hop-capped forwarding), but the resolution is parity, not principle - the cycle itself is
    /// the signal, and the remedy is a new proposal, never an edit (P3/P23).
    pub merge_cycles: Vec<MergeCycle>,
    /// The conservative merge band (resolution-identity.md Section 3, Principle 15): distinct-name
    /// entity pairs whose embeddings are near (a node-local recall aid, P16-exempt) - entity-merge
    /// candidates the substrate proposes so resolution is not the operator's manual job. Read-only:
    /// a suggestion commits nothing (IR2) - acting on it opens an entity_merge proposal through the
    /// gate. Excludes same-id, already-merged, and already-open-proposal pairs.
    pub merge_suggestions: Vec<MergeSuggestion>,
    /// Whether `merge_suggestions` above was actually computable, and over how much of the
    /// workspace - so an empty list can be read as "none found" or "not computed", never both.
    pub merge_band: MergeBandCoverage,
    /// Entity sets colliding under a normalization STRONGER than the id key (Principle 15/16). The id
    /// key is `trim`+`lowercase`, so `duplicates` above cannot fire within one workspace - these are
    /// the orthographic variants (`TrustTier` vs `Trust Tier`, `Port` vs `Ports`) that no other signal
    /// catches, and unlike `merge_suggestions` this needs NO embedder, so it works on every node.
    /// Read-only candidates: acting on one opens an entity_merge through the gate (I18/IR2).
    pub name_variants: Vec<NameVariantGroup>,
    /// Observations already in the log whose text is credential-shaped (Principle 17,
    /// [excision.md](../../docs/excision.md) Section 8 step 2).
    ///
    /// The ingest hook keeps new ones out; this finds what predates it, arrived while it was off, or
    /// matches a pattern added since. It is the honest intermediate state between prevention and a
    /// removal path that does not exist yet: it cannot delete anything, and it stops the operator
    /// from being unaware, which is the only thing available while excision is unbuilt.
    ///
    /// Read-only, like every other signal here (P7: a consolidation pass generates, it never commits).
    pub secrets: Vec<SecretFinding>,
    /// Type names defined on BOTH the entity and relation axes (resolution-identity.md Section 6,
    /// Principle 9). Informative, not blocking: an axis collision is legal but usually a mistake -
    /// the one structural T-Box check available before a subtype hierarchy exists (Principle 13).
    pub type_axis_collisions: Vec<String>,
    pub stats: CurationStats,
}

/// One conservative-merge-band candidate (resolution-identity.md Section 3): a pair of distinct
/// entities whose name embeddings are near, with the similarity (node-local recall score) and how
/// many graph neighbors they share (a corroborating structural signal). Not a proposal - the
/// viewer/agent opens an entity_merge through the gate to act on it (IR2).
#[derive(Serialize)]
pub struct MergeSuggestion {
    pub a: String,
    pub a_name: String,
    pub b: String,
    pub b_name: String,
    /// Embedding cosine similarity (recall aid, P16-exempt, node-local) - for ranking only.
    pub similarity: f32,
    /// Graph neighbors the two entities share - structural corroboration of the name similarity.
    pub shared_neighbors: usize,
}

/// One contradictory merge cycle: the member entities (id + name) and the merged proposals whose
/// effects form the cycle (proposal ids are observation ids - dereferenceable, Principle 14).
#[derive(Serialize)]
pub struct MergeCycle {
    pub members: Vec<CurationNode>,
    pub proposals: Vec<String>,
}

/// Which rung of the name-variant ladder grouped a candidate set. Declaration order is the ladder
/// order (most conservative first) and a group is reported at the FIRST rung that forms it, so a
/// pure separator variance never re-appears as a weaker plural/alias signal.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum VariantRung {
    /// Equal once separators/punctuation/case are dropped: `TrustTier` vs `Trust Tier` vs `trust-tier`.
    Separator,
    /// Equal after additionally folding a naive English plural: `Port` vs `Ports`.
    Plural,
    /// One entity's name matches ANOTHER entity's recorded alias - the rung that repays aliases as a
    /// dedup signal rather than only as a recall one.
    Alias,
}

/// A set of entities whose names collide under a stronger normalization than the id key - distinct
/// entities today, plausibly one concept. The counterpart of [`MergeSuggestion`] on the deterministic
/// axis: same read-only contract (a group commits nothing), same structural corroboration field, but
/// computed without an embedder so it is available on every node (Principle 19 - this signal belongs
/// to the deterministic core, not the probabilistic edge).
#[derive(Serialize)]
pub struct NameVariantGroup {
    /// The normalized key that grouped the members (shown so a reviewer can see WHY they collided).
    pub key: String,
    pub rung: VariantRung,
    pub members: Vec<CurationNode>,
    /// The largest number of graph neighbors shared by any member pair - structural corroboration,
    /// mirroring [`MergeSuggestion::shared_neighbors`]. 0 means the names look alike with nothing
    /// structural backing it, which is exactly the case a reviewer should look at hardest.
    pub shared_neighbors: usize,
}

/// A detected cycle before name resolution: (sorted member entity ids, forming proposal ids).
type CycleSet = (Vec<String>, Vec<String>);

/// Whether the embedding-dependent merge band could run, and over how much of the workspace
/// (resolution-identity.md Section 3). Without this, an empty `merge_suggestions` is
/// indistinguishable from "no embedder is configured" - exactly the absence-vs-unavailable
/// conflation `search_knowledge` already refuses to make by labelling the `mode` it actually used
/// (Principle 5; Principle 16 4th revision, the convergence surface vs the node-local recall aid).
/// The other curation signals are deterministic and need no such caveat.
#[derive(Serialize)]
pub struct MergeBandCoverage {
    /// False when no embedder is configured: `merge_suggestions` is empty because the signal could
    /// not be computed at all, NOT because no near-name pairs exist.
    pub available: bool,
    /// Live entities carrying a name vector, out of `examined`. Short of it means the band ran but
    /// under-covered - rows projected before an embedder was configured keep no vector until they
    /// are re-projected - so an empty result is still not a negation over the whole set.
    pub embedded: usize,
    /// Live entities the band considered (merged-away rows excluded).
    pub examined: usize,
}

/// A set of entities sharing one normalized name but distinct ids (a merge candidate).
#[derive(Serialize)]
pub struct DuplicateGroup {
    pub key: String,
    pub members: Vec<CurationNode>,
}

/// An oversized hyperedge flagged as a grab-bag.
#[derive(Serialize)]
pub struct GrabBag {
    pub id: String,
    pub size: usize,
    pub sources: usize,
    pub member_names: Vec<String>,
}

/// A node reference in a curation signal (enough for the UI to display + focus it).
#[derive(Serialize)]
pub struct CurationNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub sources: usize,
    pub degree: usize,
}

/// One live conflict on a single-valued belief field (M3a scope: entity `kind`). `current` is the
/// policy winner; `competitors` are the surviving other values with their effective tiers and one
/// asserting observation each (the dereference path for "who said so"). `contested` follows R6.
#[derive(Serialize)]
pub struct CurationConflict {
    pub id: String,
    pub name: String,
    pub field: String,
    pub current: String,
    pub contested: bool,
    pub competitors: Vec<Competitor>,
    /// The observation asserting `current` - the handle for confirming the current value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_source: Option<String>,
}

#[derive(Serialize)]
pub struct CurationStats {
    pub duplicate_groups: usize,
    pub grab_bags: usize,
    pub orphans: usize,
    pub contradictions: usize,
    pub merge_cycles: usize,
    pub merge_suggestions: usize,
    pub name_variants: usize,
    pub type_axis_collisions: usize,
}

/// The five canon-affecting proposal kinds (Principle 23 / proposal-workflow.md 3.3).
pub const PROPOSAL_KINDS: [&str; 6] = [
    "entity_merge",
    "entity_split",
    "claim_promotion",
    "claim_demotion",
    "tbox_change",
    "recall",
];

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
fn uncomputable_diff(note: &str) -> BeliefDiff {
    BeliefDiff {
        computable: false,
        note: Some(note.into()),
        tier_changes: Vec::new(),
        overturned: Vec::new(),
        rewired: Vec::new(),
    }
}

/// A merge verdict was cast on this proposal, whether or not the blocking checks let it commit.
/// `merged` and `blocked` are the two states that carry one; `open`/`rejected`/`withdrawn` do not.
///
/// This is the condition an `entity_split`'s target must satisfy, and deliberately NOT "is merged".
/// The check runs inside the fold that decides merged-versus-blocked, so depending on that
/// distinction would make one proposal's state depend on another's within a single pass. Reversing
/// a merge that the checks are holding back is harmless anyway - it subtracts an edge that is not
/// being contributed (unmerge.md Section 9).
fn carries_merge_verdict(state: &str) -> bool {
    matches!(state, "merged" | "blocked")
}

/// The two gate kinds with a tier commit effect (resolution.md Section 5).
const GATE_KINDS: [&str; 2] = ["claim_promotion", "claim_demotion"];

/// Which surface cast a verdict (resolution.md Section 6) - decided by the CALLER CRATE per
/// call-site, never by the remote client (the review surfaces accept no source_ref of their own).
/// The engine stamps the marker into the verdict observation's provenance; the gate fold derives the
/// grant ceiling from the log-borne marker, so the cap is deterministic (I2, P16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictSurface {
    /// The human console (viz unix socket - reachable only by the local OS principal). Grants up to
    /// HumanConfirmed (Principle 18: a human's direct act).
    Console,
    /// The agent-facing MCP tool path. Grants cap at HostSigned.
    Agent,
}

impl VerdictSurface {
    fn marker(self) -> &'static str {
        match self {
            VerdictSurface::Console => VERDICT_SURFACE_CONSOLE,
            VerdictSurface::Agent => VERDICT_SURFACE_AGENT,
        }
    }
}

/// Parses a snake_case tier label (the serialized form of [`TrustTier`]). Used by the propose surface
/// and the gate fold - both read the same labels [`tier_label`] writes.
fn parse_tier(s: &str) -> Option<TrustTier> {
    match s {
        "unverified" => Some(TrustTier::Unverified),
        "agent_extracted" => Some(TrustTier::AgentExtracted),
        "host_signed" => Some(TrustTier::HostSigned),
        "human_confirmed" => Some(TrustTier::HumanConfirmed),
        _ => None,
    }
}

/// A competing value for a contested single-valued field (resolution.md Section 4) - carried on
/// graph nodes / entity views so every surface can answer "what else was asserted, at what trust,
/// and where" (Principle 2). One entry per distinct non-winning value, at that value's highest
/// effective tier, with one asserting observation id as the dereference path.
#[derive(Serialize, Clone)]
pub struct Competitor {
    pub value: String,
    pub trust_tier: TrustTier,
    pub observation: String,
}

/// A T-Box type (entity or relation) that a proposal defines or changes. Carried by `tbox_change`
/// proposals so the viewer can highlight the affected graph elements when previewing the change - a
/// structured belief-diff hint, not parsed out of the rationale prose. Relation names are stored
/// normalized (they must match the graph's edge kinds, which the viewer highlights by `kind`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AffectedType {
    /// Which vocabulary axis: an entity (node) type or a relation (edge) type.
    pub target: TypeTarget,
    /// The type name (e.g. `Driver`, `depends_on`).
    pub name: String,
}

/// Input for [`Engine::reify_hyperedge`] (Principle 11's promotion path into the graph).
pub struct ReifyInput {
    pub workspace: Option<String>,
    /// The hyperedge to reify (from `workspace_map` / the hypergraph resource).
    pub hyperedge: String,
    /// Group entity name; defaults to "context: <first member names>".
    pub name: Option<String>,
    /// Group entity type; defaults to "Context".
    pub kind: Option<String>,
    pub source_ref: Option<String>,
    pub on_behalf_of: Option<String>,
}

/// Input to open a proposal.
pub struct ProposeInput {
    pub workspace: Option<String>,
    pub kind: String,
    /// Referenced entity/observation ids the proposal acts on.
    pub targets: Vec<String>,
    /// For entity_merge: the canonical target the others fold into (must be one of `targets`).
    pub into: Option<String>,
    /// For claim_promotion/claim_demotion: the requested tier (snake_case label, e.g.
    /// "human_confirmed"). Required for the gate kinds, rejected on the others. What a merged
    /// verdict actually grants is min(requested, surface ceiling) - resolution.md Sections 5/6.
    pub tier: Option<String>,
    pub rationale: Option<String>,
    /// For tbox_change: the entity/relation types this proposal defines or changes (viewer highlight
    /// hint). Empty for other kinds and for tbox_change proposals that do not declare their scope.
    pub affected_types: Vec<AffectedType>,
    pub source_ref: Option<String>,
    pub on_behalf_of: Option<String>,
}

/// One observation's effective-tier change under a proposal's effects.
#[derive(Serialize, Debug)]
pub struct TierChange {
    pub observation: String,
    pub from: TrustTier,
    pub to: TrustTier,
}

/// A current belief this proposal would overturn - the component of the diff a reviewer is actually
/// deciding on (proposal-workflow.md Section 5, item 2). `contested_*` carries item 3: a contradiction
/// does not block a merge, but settling or creating one must be visible before the verdict.
#[derive(Serialize, Debug)]
pub struct BeliefChange {
    pub entity: String,
    pub name: String,
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub contested_before: bool,
    pub contested_after: bool,
}

/// The computed belief diff (proposal-workflow.md Section 5): what materializing the canon WITH this
/// proposal's effects differs from materializing it without them. Both sides run the same
/// `belief_fold` with only the gate grants changed, so the diff cannot drift from what a merge would
/// actually do - the alternative, re-deriving the outcome separately, is the incremental-vs-replay
/// divergence M3b spent a milestone removing.
///
/// `computable` is load-bearing. Three of the five proposal kinds still enforce nothing, and an empty
/// diff for those would read as "this changes nothing" when the truth is "this cannot be computed
/// yet" - the same absence-vs-unavailable conflation the merge band's coverage report exists to
/// prevent (Principle 5).
#[derive(Serialize)]
pub struct BeliefDiff {
    pub computable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub tier_changes: Vec<TierChange>,
    pub overturned: Vec<BeliefChange>,
    /// For entity_merge: the references that get rewired onto the canonical id
    /// (proposal-workflow.md Section 5, item 5). Empty for every other kind.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rewired: Vec<RelationRewire>,
}

/// One relation whose endpoint moves when a merge commits. `becomes_self_loop` is the consequence a
/// reviewer would otherwise not see: merging two entities that are connected to each other turns
/// their edge into a self-loop, and `graph()` drops self-loops - so accepting silently removes an
/// edge that is on screen right now.
#[derive(Serialize, Debug)]
pub struct RelationRewire {
    pub relation: String,
    pub kind: String,
    pub from_name: String,
    pub to_name: String,
    /// The endpoint that does not move - what the edge still connects to afterwards.
    pub other_name: String,
    pub becomes_self_loop: bool,
}

/// One check result (proposal-workflow.md Section 6). A blocking failure prevents a merge verdict
/// from reaching canon; an informative one is shown and blocks nothing (the Principle 6/9 split - a
/// structural contradiction is a bug and is stopped, a contradiction between assertions is a fact
/// about the world and is only surfaced).
#[derive(Serialize, Debug)]
pub struct CheckResult {
    pub name: String,
    pub blocking: bool,
    pub passed: bool,
    pub detail: String,
}

/// Folded proposal state (a deterministic read view over the proposal's events, I2).
#[derive(Serialize)]
pub struct ProposalView {
    pub id: String,
    pub kind: String,
    pub targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
    /// For claim_promotion/claim_demotion: the requested tier (resolution.md Section 5).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<TrustTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// For tbox_change: the T-Box types this proposal defines/changes - the viewer's highlight hint.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub affected_types: Vec<AffectedType>,
    /// open | merged | rejected | withdrawn (solo fold; merge is the absorbing state, I16).
    pub state: String,
    pub verdicts: usize,
    pub opened_at: Timestamp,
    pub proposer: String,
    /// Solo single-user marker (Principle 23 self-approval exception): proposer == reviewer.
    pub self_attested: bool,
    /// The computed diff, attached by `get_proposal` only. `list_proposals` leaves it None: a diff is
    /// two full belief folds, which is the right cost for the one proposal being reviewed and the
    /// wrong cost per row of a list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub belief_diff: Option<BeliefDiff>,
    /// Check results, attached by `get_proposal` only (like the diff - the fold recomputes what it
    /// needs on its own). Section 6.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckResult>,
}

/// Transient verdict accumulator for the proposal fold.
#[derive(Default)]
struct ProposalTally {
    merge: bool,
    reject: bool,
    withdraw: bool,
    verdicts: usize,
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
    /// Names of entities merged into this canonical node (Principle 15) - empty when nothing folded in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The number of edges included in the graph that connect to this node (only edges whose both endpoints are in the node set).
    pub degree: usize,
    /// The number of sources (attestations) accumulated on this entity - larger when more observations back it.
    pub sources: usize,
    /// Distinct provenance hosts that attested this entity (sorted) - where the knowledge came
    /// from, e.g. ["ashon-mac", "knowledge-vm"] on a hub after a sync (federation observability).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<String>,
    /// The node's representative trust: the highest EFFECTIVE tier over its supporting observations
    /// (receiver-evaluated + gate grants - resolution.md Section 3; never a max over claimed tiers,
    /// which would let a remote self-declaration raise the displayed tier - F13).
    pub trust_tier: TrustTier,
    /// True when distinct kind values survive whose effective tiers tie at the top - the winner was
    /// decided by recency alone, not trust, so this node invites mediation (resolution.md R6).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contested: bool,
    /// The non-winning kind values still asserted in the log (resolution.md R7 - conflicts stay
    /// queryable whether or not they are contested). Empty when the kind was never disputed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub competitors: Vec<Competitor>,
    /// The observation that asserted the winning kind (present when the kind came from the belief
    /// fold) - the mediation handle: confirming the current value = promoting this observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_source: Option<String>,
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

/// Every free-text field of an observation that can carry a secret, with the name the operator sees.
///
/// One list, shared by the ingest refusal and the stored-log scan. Two walks would drift, and the
/// drift would be silent in the worst direction: a field the door checks but the scan does not means
/// a secret that was refused at ingest, arrived some other way, and is then reported as absent.
fn scannable_fields(obs: &Observation) -> Vec<(&'static str, &str)> {
    let mut fields: Vec<(&'static str, &str)> = vec![("content", obs.content.as_str())];
    for p in &obs.provenance {
        if let Some(r) = &p.source_ref {
            fields.push(("source_ref", r.as_str()));
        }
    }
    for e in &obs.assertions.entities {
        fields.push(("entity name", e.name.as_str()));
        if let Some(d) = &e.description {
            fields.push(("entity description", d.as_str()));
        }
    }
    for r in &obs.assertions.relations {
        if let Some(d) = &r.description {
            fields.push(("relation description", d.as_str()));
        }
    }
    for t in &obs.assertions.type_defs {
        fields.push(("type description", t.description.as_str()));
    }
    for ev in &obs.assertions.proposal_events {
        fields.push(("proposal payload", ev.payload.as_str()));
    }
    fields
}

/// One stored observation whose text is credential-shaped.
///
/// Carries the id, the field and the pattern - never the matched text, and never the surrounding
/// content. A report that quoted the secret would copy it into every log, transcript and screenshot
/// the report reaches, which is the same reason the ingest refusal does not quote it either
/// (excision.md E2). Dereference the id to see the row, deliberately as a separate act.
#[derive(Serialize)]
pub struct SecretFinding {
    pub observation: String,
    /// Which field matched: `content`, `source_ref`, `entity name`, and so on.
    pub field: &'static str,
    /// The shape that matched, e.g. `aws-access-key-id`. Safe to display.
    pub pattern: &'static str,
    /// Byte offset within that field, so the operator can find it without the report showing it.
    pub at: usize,
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

/// One read's view of the observation log, loaded at most once.
///
/// Every fold on the read path - proposal state, merge forwarding, gate grants, the belief fold,
/// the asserted-id set - is a pure function of the same rows, and each used to fetch those rows for
/// itself. Measured on a workspace with a merged entity_merge and a merged claim_promotion,
/// `graph` walked the log four times and `curation` sixteen, deserializing every row on each walk -
/// including a 384-float embedding that no fold on this path reads. The count is constant in the
/// size of the workspace, so it is a fixed multiplier on a cost that grows with the log forever.
///
/// Loading once also makes the answer a snapshot. The read path is deliberately not serialized
/// against writes, so separate enumerations could straddle a concurrent `observe` and compose one
/// view out of two log states - convergent per fold, but not a state the log was ever in.
///
/// Scoped to a single call and never stored on the engine: this is not a cache with an invalidation
/// problem, it is one read declining to ask the same question twice.
#[derive(Default)]
struct ReadCtx {
    /// The workspace scope the rows were loaded under, kept beside them. A cache that ignored it
    /// would answer a `Some("a")` read with rows loaded for `None`, which is not slower - it is
    /// wrong. No call path mixes scopes today; this is here so that adding one cannot be silent.
    observations: std::cell::OnceCell<(u64, Option<String>, Vec<Observation>)>,
}

/// The result of [`Engine::belief_fold`]: per canonical entity id, the kind candidates and the
/// representative effective tier over supporting observations (resolution.md Sections 2-4).
struct BeliefFold {
    kinds: HashMap<String, Vec<BeliefCandidate>>,
    tiers: HashMap<String, TrustTier>,
}

/// An observation's EFFECTIVE tier (resolution.md Section 3): the tier set by the HLC-latest merged
/// gate event targeting it, if any (overrides in both directions - a demotion can push below base);
/// otherwise the max receiver-evaluated tier over its attestations ([`evaluated_tier`] - a wire claim
/// never evaluates above HostSigned). Never a max over claimed tiers (F13).
/// Refuses a client-supplied source_ref inside the reserved verdict-marker namespace
/// (resolution.md Section 6, R8). The ceiling fold trusts a log-borne marker to be engine-stamped,
/// so every local ingest door except `review_proposal` (which stamps its own marker and accepts no
/// source_ref) must refuse the prefix. Sync apply is deliberately unguarded: a replicated verdict
/// legitimately carries its marker, and refusing it would break the ceiling's convergence.
fn reject_reserved_source_ref(source_ref: Option<&str>) -> Result<(), ObserveError> {
    if let Some(s) = source_ref {
        if s.starts_with(VERDICT_SURFACE_PREFIX) {
            return Err(ObserveError::Invalid(format!(
                "source_ref '{s}' is inside the reserved '{VERDICT_SURFACE_PREFIX}' namespace - \
                 verdict-surface markers are engine-stamped and cannot be supplied by a client. \
                 Name the actual source instead (a file path, URL, or tool)"
            )));
        }
    }
    Ok(())
}

fn effective_tier(obs: &Observation, gates: &HashMap<String, TrustTier>) -> TrustTier {
    gates
        .get(&obs.id)
        .copied()
        .unwrap_or_else(|| obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default())
}

/// The authoring attestation of an observation: earliest effective HLC, index-tiebroken. After an
/// absorb the provenance vec is union-sorted (by host first), so `first()` names whichever host
/// sorts first, and any max-over-attestations moves as attestations accumulate - neither names the
/// author. This is the single rule for "who authored this act": the verdict surface marker
/// (gate_grants), relation provenance (reproject) and proposal attribution (fold_proposals) must
/// not each pick a different attestation off one row.
fn authoring_attestation(obs: &Observation) -> Option<&Provenance> {
    obs.provenance
        .iter()
        .enumerate()
        .min_by_key(|(i, p)| {
            (
                p.sync
                    .as_ref()
                    .map(|s| s.hlc.clone())
                    .unwrap_or_else(|| Hlc::legacy(p.observed_at)),
                *i,
            )
        })
        .map(|(_, p)| p)
}

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
    let ws = obs.workspace();
    let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
    let mut members: BTreeSet<String> = BTreeSet::new();
    for e in &obs.assertions.entities {
        let id = canon(Entity::make_id(ws, &e.name));
        if node_ids.contains(id.as_str()) {
            members.insert(id);
        }
    }
    for r in &obs.assertions.relations {
        for name in [&r.from, &r.to] {
            let id = canon(Entity::make_id(ws, name));
            if node_ids.contains(id.as_str()) {
                members.insert(id);
            }
        }
    }
    members.into_iter().collect()
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

/// One provenance attestation flattened for the log/explain surface (observability). Provenance is
/// a first-class citizen (Principle 2): the log view exposes who attested a fact, when, at what
/// CLAIMED tier (log data, verbatim - F13), and how the receiver EVALUATES that claim
/// ([`evaluated_tier`] - a synced claim never evaluates above host_signed; the evaluation, not the
/// claim, is what resolution consumes).
#[derive(Serialize)]
pub struct AttestationSummary {
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    pub observed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub trust_tier: TrustTier,
    pub evaluated_tier: TrustTier,
    /// The origin node_id when the attestation is sync-stamped (federation provenance); absent for local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_node: Option<String>,
}

/// A named entity an observation asserts (spelling + resolved canonical id), so a log row links back
/// to the graph node it touched (click-to-focus in the viewer).
#[derive(Serialize)]
pub struct EntityRef {
    pub name: String,
    pub id: String,
}

/// A relation an observation asserts, by endpoint spellings + normalized kind (log-row context).
#[derive(Serialize)]
pub struct RelationRef {
    pub from: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub to: String,
}

/// A proposal event rendered for a reader, so the log panel does not have to show the raw text.
///
/// A proposal event's stored `content` is machine text (`proposal(merge) <64 hex>`) and it is inside
/// the content address, so it can never be rewritten - the log would read as hashes forever. This is
/// the read-time translation: the same event said in names. Resolution is best-effort by design, and
/// an unresolvable target keeps its id rather than being dropped, because a row that silently omits
/// what it could not name would misreport how many entities an act touched.
#[derive(Serialize)]
pub struct ProposalEventSummary {
    /// The proposal this event belongs to (its opening observation's id).
    pub proposal: String,
    /// `opened` | `verdict` | `comment` | `withdrawn`.
    pub event: String,
    /// The verdict carried, on a verdict event: `merge` | `reject` | ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// The proposal's kind (`entity_merge`, `claim_promotion`, ...), empty if the opening event has
    /// not arrived on this node yet.
    pub kind: String,
    /// The proposal's state as the fold sees it NOW - so a verdict row says what it settled into.
    pub state: String,
    /// What the proposal acts on, named where the projection still knows the name.
    pub targets: Vec<EntityRef>,
    /// The canonical target of an entity merge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub into: Option<EntityRef>,
}

/// One observation flattened for the log-browser surface (observability). The observation log is
/// the source of truth (Principle 1); this is a read-only projection of one log row with its
/// provenance (Principle 2) and effective tier (resolution.md Section 3). Embeddings never appear
/// (Principle 21 - they are an internal recall aid, not part of the legible surface).
#[derive(Serialize)]
pub struct ObsSummary {
    pub id: String,
    pub content: String,
    /// The deterministic fold-ordering key ([`ordering_hlc`]) - also the log-feed order.
    pub hlc: Hlc,
    /// The observation's effective tier (gate grants applied - resolution.md Section 3).
    pub effective_tier: TrustTier,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
    pub attestations: Vec<AttestationSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationRef>,
    /// Present when this observation IS a proposal event - the readable form of `content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProposalEventSummary>,
}

/// One competing value for a single-valued belief field, with its role in the RESOLVED projection -
/// the unit of the "why this belief" explanation (resolution.md Sections 2-4). Role is the projected
/// winner, an `alias` (a non-winning name spelling kept as an alias, Principle 3 / IR1), or a
/// `competitor` (a non-winning kind still asserted in the log, R7). One row per distinct value at
/// that value's highest effective tier, with one asserting observation as the dereference path.
#[derive(Serialize)]
pub struct CandidateRow {
    pub value: String,
    /// "winner" | "alias" | "competitor".
    pub role: &'static str,
    pub trust_tier: TrustTier,
    pub hlc: Hlc,
    pub observation: String,
}

/// The resolution of one single-valued belief field (canonical_name or kind): the winning value,
/// whether it is contested (distinct values tie on trust - R6), and the ranked candidates behind it.
#[derive(Serialize)]
pub struct FieldExplain {
    /// "canonical_name" | "kind".
    pub field: &'static str,
    pub winner: String,
    pub contested: bool,
    pub candidates: Vec<CandidateRow>,
}

/// "Why is this node projected this way": the per-field belief resolution (evidence + decision) plus
/// the supporting observation log for one entity. A pure read projection (Principle 1) built ON TOP
/// of [`Engine::get_entity`], so the winners ARE the projected entity's values - an explanation OF
/// the projection, never a second computation that could drift from it.
#[derive(Serialize)]
pub struct EntityExplain {
    pub id: String,
    pub name: String,
    pub effective_tier: TrustTier,
    pub fields: Vec<FieldExplain>,
    pub supporting: Vec<ObsSummary>,
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
    /// The belief-resolution strategy (Principle 1, resolution.md R1) - replaceable; defaults to
    /// [`TierWeighted`]. Consumed by the read-path belief folds and by reprojection.
    policy: Arc<dyn ResolutionPolicy>,
    /// Bumped whenever this engine appends an observation. A [`ReadCtx`] records the value it
    /// loaded at and reloads when it moves, so a context that outlives a write cannot serve rows
    /// from before it. Without this the rule "do not read through a context after writing" would be
    /// a convention, and the only thing enforcing it would be whoever reads the code next.
    log_epoch: std::sync::atomic::AtomicU64,
    /// Whether the ingest doors refuse credential-shaped text (Principle 17). On by default; the
    /// operator can disable it for a node whose corpus trips the patterns, which is a decision worth
    /// making explicitly rather than by silently overriding each refusal.
    scan_secrets: bool,
    /// The transaction-time source (Principle 20) - defaults to the node wall clock. What it returns
    /// becomes `observed_at`, which is the ordering key for a local attestation, so this is the seam
    /// that lets a test state the arrival order it is testing instead of sleeping to produce one.
    clock: Arc<dyn Clock>,
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
            policy: Arc::new(TierWeighted),
            log_epoch: std::sync::atomic::AtomicU64::new(0),
            clock: Arc::new(SystemClock),
            scan_secrets: true,
            host: host.into(),
            default_workspace: default_workspace.into(),
        }
    }

    /// Replaces the belief-resolution policy (builder; Principle 1, resolution.md R1). The default is
    /// [`TierWeighted`]. Changing the policy and re-running [`Engine::reproject`] recomputes the
    /// belief from the unchanged log.
    pub fn with_policy(mut self, policy: Arc<dyn ResolutionPolicy>) -> Self {
        self.policy = policy;
        self
    }

    /// Turns the ingest secret scan off (builder, Principle 17). Defence in depth is opt-out rather
    /// than opt-in because the cost of a miss is unbounded: the log is append-only and replicates.
    pub fn with_secret_scan(mut self, on: bool) -> Self {
        self.scan_secrets = on;
        self
    }

    /// Replaces the transaction-time source (builder, Principle 20). The default is the node wall
    /// clock. A test injects one so that "this assertion arrived after that one" is something it
    /// states rather than something it hopes the scheduler produced.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
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

    /// Refuses to append an observation carrying credential-shaped text (Principle 17: the
    /// secret-redaction hook at ingest).
    ///
    /// **Every local door, and only the local doors.** It deliberately does NOT run on the sync apply
    /// path. `check_well_formed` can run there because structural validity is stable across versions,
    /// but a detector's patterns grow - so a node on a newer build would refuse an event an older peer
    /// accepted, and the two would hold different logs from the same event set. That is precisely the
    /// P16 divergence the apply path exists to avoid. This is a node-local ingest aid, which is what
    /// P17 asks for: "an aid to, not a replacement for, the sharing filter".
    ///
    /// The refusal names the field and the shape and never the value, so declining the write cannot
    /// itself publish the secret (excision.md E2). The message is written for an agent to act on
    /// without a human in the loop (P21).
    fn refuse_secrets(&self, obs: &Observation) -> Result<(), ObserveError> {
        if !self.scan_secrets {
            return Ok(());
        }
        for (field, text) in scannable_fields(obs) {
            if let Some(hit) = supragnosis_core::detect_secret(text) {
                return Err(ObserveError::Invalid(format!(
                    "refusing to store this: the {field} contains something shaped like a credential \
                     ({}, at byte {}). The log is append-only, so anything written here cannot be \
                     taken back - and if this workspace is shared, it replicates. Remove the secret \
                     and observe the knowledge without it: say that a credential exists and where it \
                     is configured, never what it is",
                    hit.pattern, hit.at
                )));
            }
        }
        Ok(())
    }

    /// Emits a UI event. Does nothing if there is no sink (observability is optional).
    /// Carries the session id in the envelope (footprint group key). The caller (MCP tool handler) invokes it per
    /// intent - a side channel unrelated to the storage/resolution logic.
    pub fn emit(&self, event: Event) {
        if let Some(sink) = &self.events {
            sink.emit(&EventEnvelope { session: self.session.clone(), event });
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
            observed_at: self.clock.now_millis(),
            // Preserve no-annotation as no-annotation (Principle 2, 4th) - substituting a default (1.0) is a
            // capture loss that erases the distinction between "no assertion" and "full-confidence assertion". Interpretation is the resolution policy's job (M3).
            confidence,
            // Trust tier promotion only happens in an explicit flow (human confirmation / cross-validation) - observe uses the default.
            trust_tier: TrustTier::default(),
            sync: None,
        }
    }

    /// Ingests a piece of knowledge: stores an immutable observation + links the provided entities/relations into the ontology.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, ObserveError> {
        // The verdict-marker namespace is engine-controlled provenance, like trust_tier and
        // observed_at - a client-supplied "surface:*" is a forged provenance claim, not content.
        reject_reserved_source_ref(input.source_ref.as_deref())?;
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
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let prov =
            self.provenance(&workspace, input.source_ref, input.confidence, input.on_behalf_of);

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
            // observe does not define types or open proposals (those are the define_type/propose intents).
            type_defs: Vec::new(),
            proposal_events: Vec::new(),
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
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        self.store.add_observation(obs)?;
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // Resolution write path (resolution-identity.md Section 4): the observation is in the log,
        // so (re)project exactly the entities it touched - the same fold reproject runs, so the
        // incremental row equals a fresh replay's (IR3). Relations then project directly (their id
        // is deterministic and description/valid-interval are last-write, matching reproject).
        let entity_ids: Vec<String> =
            input.entities.iter().map(|e| Entity::make_id(&workspace, &e.name)).collect();
        let mut touched: HashSet<String> = entity_ids.iter().cloned().collect();
        for r in &input.relations {
            touched.insert(Entity::make_id(&workspace, &r.from));
            touched.insert(Entity::make_id(&workspace, &r.to));
        }
        // Created here, after the append: a read context is a snapshot, so one taken before the
        // write would hand the projection a log that does not contain the observation it is
        // projecting. Scoping it to the read is what keeps that unrepresentable.
        let cx = &ReadCtx::default();
        self.project_entities(&workspace, Some(&touched), cx)?;

        // The edge ids this call asserted. Derived from the input rather than from the projection,
        // because the two answer different questions: this is "what did this observe name", while the
        // projection decides what each of those edges now looks like given the whole log.
        let relations: Vec<String> = input
            .relations
            .iter()
            .map(|r| {
                Relation::make_id(
                    &Entity::make_id(&workspace, &r.from),
                    &normalize_relation_kind(&r.kind),
                    &Entity::make_id(&workspace, &r.to),
                )
            })
            .collect();
        let touched_edges: HashSet<String> = relations.iter().cloned().collect();
        self.project_relations(&workspace, Some(&touched_edges))?;

        Ok(ObserveOutput { observation_id, entities: entity_ids, relations })
    }

    /// Records T-Box type definitions as an observation (Principle 8/11: an explicit define_type act,
    /// scoped to the workspace). It rides the observation log like any other assertion (Principle 1/23:
    /// no parallel provenance/storage), so the glossary is a deterministic projection ([`types`]) and a
    /// future proposal gate (Principle 23) can wrap this without rework. Principle 8 is enforced here:
    /// a definition with no name or an empty description is rejected before it reaches the permanent log.
    pub fn define_type(&self, input: DefineTypeInput) -> Result<String, ObserveError> {
        reject_reserved_source_ref(input.source_ref.as_deref())?;
        if input.defs.is_empty() {
            return Err(ObserveError::Invalid(
                "no type definitions provided. give at least one {target, name, description}"
                    .into(),
            ));
        }
        for d in &input.defs {
            if d.name.trim().is_empty() {
                return Err(ObserveError::Invalid(
                    "a type definition has an empty name - name the type you are defining".into(),
                ));
            }
            // Principle 8 (Clarity): a type cannot be created without a natural-language definition.
            if d.description.trim().is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "type '{}' has an empty description. Principle 8 (clarity): a type needs a \
                     natural-language definition of what it means - describe it, or drop the item",
                    d.name
                )));
            }
        }
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(&workspace, input.source_ref, None, input.on_behalf_of);

        // Synthesize readable content so the definition is also keyword/semantic searchable (Principle 22).
        let mut content = String::from("Type definitions:");
        for d in &input.defs {
            let axis = match d.target {
                TypeTarget::Entity => "entity",
                TypeTarget::Relation => "relation",
            };
            content.push_str(&format!(
                "\n- {axis} type `{}`: {}",
                d.name.trim(),
                d.description.trim()
            ));
        }
        let assertions = Assertions {
            type_defs: input
                .defs
                .into_iter()
                .map(|d| TypeDefAssertion {
                    target: d.target,
                    name: d.name.trim().to_string(),
                    description: d.description.trim().to_string(),
                })
                .collect(),
            ..Default::default()
        };
        let mut obs = Observation::with_assertions(content, prov.clone(), assertions);
        if let Some(embedder) = &self.embedder {
            if let Ok(vec) = embedder.embed_one(&obs.content) {
                obs.embedding = Some(vec);
            }
        }
        let observation_id = obs.id.clone();
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        self.store.add_observation(obs)?;
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(observation_id)
    }

    /// Projects the workspace type glossary from the observation log (a pure read, Principle 1). Folds every
    /// type_def assertion, keeping the latest definition per (target, name) - last-write-wins by
    /// (ordering HLC, observation id), the cross-node fold order of federation.md Section 4 (M4 Phase 1;
    /// pre-federation observations fall back to a deterministic legacy HLC from observed_at), so the
    /// winner is arrival-order independent (Principle 16) and converges across nodes once stamps exist.
    /// Accumulates a corroboration count (sources) and the representative (highest) trust tier.
    pub fn types(&self, workspace: Option<&str>) -> Result<Vec<TypeDefView>, StoreError> {
        self.types_in(workspace, &ReadCtx::default())
    }

    /// [`Engine::types`] over an existing read context, so a caller that already loaded the
    /// log does not load it again (see [`ReadCtx`]).
    fn types_in(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<Vec<TypeDefView>, StoreError> {
        let gates = self.gate_grants(workspace, cx)?;
        // (disc, name) -> (target, description candidates, sources, max effective trust). The
        // description is resolved by the SAME policy as an entity kind (resolution-identity.md
        // Section 6): distinct definitions at a tied top tier are contested, not silently
        // last-write-won (M3a's contested treatment applied to the T-Box - IR5).
        type Acc = (TypeTarget, Vec<BeliefCandidate>, usize, TrustTier);
        // Keyed by (workspace, target, name). Without the workspace this fold merged same-named types
        // from unrelated workspaces into one row - picking one description as the winner and
        // reporting the other as a `contested` competitor, which invents a conflict where there is
        // none and hides one definition behind another. The scoped reads never showed it because they
        // only ever hold one workspace; only the all-workspaces view could, and that is the view the
        // console offers. The merge band already refuses to span workspaces for exactly this reason
        // (P17 clause `p17_candidates_never_span_workspaces_in_the_all_view`); this fold did not.
        let mut descs: BTreeMap<(String, u8, String), Acc> = BTreeMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            let hlc = ordering_hlc(obs);
            let eff = effective_tier(obs, &gates); // receiver-evaluated + gate grants (F13)
            for t in &obs.assertions.type_defs {
                let disc: u8 = match t.target {
                    TypeTarget::Entity => 0,
                    TypeTarget::Relation => 1,
                };
                let e = descs
                    .entry((obs.workspace().to_string(), disc, t.name.clone()))
                    .or_insert_with(|| (t.target, Vec::new(), 0, TrustTier::Unverified));
                e.1.push(BeliefCandidate {
                    value: t.description.clone(),
                    tier: eff,
                    hlc: hlc.clone(),
                    observation: obs.id.clone(),
                });
                e.2 += 1;
                e.3 = e.3.max(eff);
            }
        }
        // Deterministic order by (workspace, target, name) - the BTreeMap key already gives it.
        Ok(descs
            .into_iter()
            .map(|((workspace, _, name), (target, cands, sources, trust))| {
                let (winner, contested, competitors) = self.resolve_kind(Some(&cands));
                let (description, def_source) = match winner {
                    Some((d, obs)) => (d, Some(obs)),
                    None => (String::new(), None),
                };
                TypeDefView {
                    workspace,
                    target,
                    name,
                    description,
                    sources,
                    trust_tier: trust,
                    contested,
                    competitors,
                    def_source,
                }
            })
            .collect())
    }

    /// The conservative merge band (resolution-identity.md Section 3, Principle 15): distinct-name
    /// entity pairs whose name embeddings are near (>= [`SIM_CANDIDATE`]) - candidates the substrate
    /// proposes so identity resolution is not the operator's manual job. A node-local recall aid
    /// (P16-exempt, P19): the similarity is not part of the converging graph, and a candidate commits
    /// NOTHING (IR2/I18) - acting on it opens an entity_merge through the gate. Excludes same-id,
    /// already-merged (either side forwarded away), and pairs already under an open entity_merge.
    /// Without an embedder there are no candidates (degrade, P19). Within-node reproducible order
    /// Every pair a candidate generator must not offer: in flight under an open merge
    /// ([`Self::open_merge_pairs`]) or pulled apart by a merged split ([`Self::split_pairs`]).
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

    /// Entity pairs already carried by an OPEN entity_merge proposal. Every candidate generator
    /// suppresses these: a pair in flight is not a fresh candidate, and re-offering it invites a
    /// second proposal for a merge already awaiting a verdict. Shared by the merge band and the
    /// name-variant ladder so the two cannot drift apart on what "already proposed" means.
    fn open_merge_pairs(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<(String, String)>, StoreError> {
        let mut open_pairs: HashSet<(String, String)> = HashSet::new();
        for p in self.fold_proposals(workspace, cx)?.values() {
            if p.kind == "entity_merge" && p.state == "open" {
                for i in 0..p.targets.len() {
                    for j in (i + 1)..p.targets.len() {
                        open_pairs.insert(unordered_pair(&p.targets[i], &p.targets[j]));
                    }
                }
            }
        }
        Ok(open_pairs)
    }

    /// (similarity desc, then ids); it need not converge across nodes (Section 3).
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
                MergeBandCoverage { available: false, embedded: 0, examined: 0 },
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
                if !seen.insert(pair.clone()) || open_pairs.contains(&pair) {
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
        Ok((out, MergeBandCoverage { available: true, embedded, examined }))
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
        // (1) Merge candidates: entities sharing a normalized name but with distinct ids (Principle 15).
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
            })
            .collect();
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
                        all_entities.iter().find(|e| &e.id == id).map(&node).unwrap_or_else(|| {
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
        let name_variants = name_variant_groups(
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
        Ok(CurationReport {
            workspace: workspace.map(String::from),
            duplicates,
            grab_bags,
            orphans,
            contradictions,
            merge_cycles,
            merge_suggestions,
            merge_band,
            name_variants,
            type_axis_collisions,
            secrets,
            stats,
        })
    }

    // --- Proposal workflow (Principle 23, solo-scoped M3.5a) ---------------------------------------
    // A proposal and its verdicts are observations (I1); the state is a deterministic fold (I2). This is
    // the gate skeleton - it records/folds open+verdict, but the belief diff, blocking checks, and the
    // merge EFFECT (id forwarding) are later steps. Solo scope: no HLC/quorum; self-approval is the
    // single-user exception, marked self_attested (Principle 23).

    /// Open a proposal (Principle 23). Records an `opened` event as an observation; the observation id
    /// becomes the proposal id. Validates only well-formedness (kind known, targets present, entity_merge
    /// has a valid `into`) - it does not yet commit or check the canon.
    pub fn propose(&self, input: ProposeInput) -> Result<String, ObserveError> {
        reject_reserved_source_ref(input.source_ref.as_deref())?;
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
        if input.kind == "entity_merge" {
            if input.targets.len() < 2 {
                return Err(ObserveError::Invalid(
                    "entity_merge needs at least 2 target ids (the entities to merge)".into(),
                ));
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
            for target in &input.targets {
                if self.store.get_observation(target)?.is_none() {
                    return Err(ObserveError::Invalid(format!(
                        "target observation '{target}' is not in the local log - {} targets are \
                         observation ids (as returned by observe / carried on search hits), and an \
                         observation that is not here cannot be promoted or demoted",
                        input.kind
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
        let prov = self.provenance(&workspace, input.source_ref, None, input.on_behalf_of);
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
        let payload = serde_json::json!({
            "kind": input.kind,
            "targets": input.targets,
            "into": input.into,
            "tier": gate_tier.map(|t| tier_label(t).to_string()),
            "rationale": input.rationale,
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
        let obs = Observation::with_assertions(content, prov, assertions);
        let id = obs.id.clone();
        // A proposal IS its content (Principle 14), so re-opening an identical merge yields the same
        // id - including one a split has already reversed, and the split names that id forever. The
        // re-proposal would be permanently dead: accepted, verdicted, and unable to ever forward.
        // Refuse and name the fix instead (P21), which is also the honest one - a re-merge after a
        // split is a different act and should say why (unmerge.md Section 7).
        if input.kind == "entity_merge"
            && self.reversed_merges(Some(&workspace), &ReadCtx::default())?.contains(&id)
        {
            return Err(ObserveError::Invalid(format!(
                "this exact resolution was already reversed by an entity_split. A proposal is its \
                 content (Principle 14), so re-opening it produces {id} again and it stays \
                 reversed. Give the re-merge a `rationale` saying why the split was wrong - that \
                 makes it a different proposal, which is what it is"
            )));
        }
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        self.store.add_observation(obs)?;
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(id)
    }

    /// Cast a verdict / comment / withdrawal on a proposal (Principle 23). Records the event as an
    /// observation (I1); the fold derives the resulting state. `decision` is merge|reject|comment|withdraw.
    /// `surface` is decided by the caller crate per call-site (resolution.md Section 6) - it is
    /// stamped into the verdict's provenance and caps what a merged promotion can grant.
    pub fn review_proposal(
        &self,
        workspace: Option<String>,
        proposal: String,
        decision: String,
        note: Option<String>,
        on_behalf_of: Option<String>,
        surface: VerdictSurface,
    ) -> Result<String, ObserveError> {
        let cx = &ReadCtx::default();
        let event = match decision.as_str() {
            "merge" | "reject" => ProposalEventKind::Verdict,
            "comment" => ProposalEventKind::Comment,
            "withdraw" => ProposalEventKind::Withdrawn,
            other => {
                return Err(ObserveError::Invalid(format!(
                    "unknown decision '{other}'. use merge | reject | comment | withdraw"
                )))
            }
        };
        if proposal.trim().is_empty() {
            return Err(ObserveError::Invalid("proposal id is required".into()));
        }
        let workspace = workspace.unwrap_or_else(|| self.default_workspace.clone());
        // Fail fast on a local merge attempt that the fold would refuse anyway. This is a courtesy,
        // not the gate: the fold is what enforces (a replicated verdict never reaches this path), so
        // this exists to say WHY rather than to let the caller discover a "blocked" state later.
        if decision == "merge" {
            if let Some(view) = self.get_proposal(Some(&workspace), &proposal)? {
                let asserted = self.asserted_entity_ids(Some(&workspace), cx)?;
                let decided = self.decided_merges(Some(&workspace), cx)?;
                let failures =
                    self.blocking_failures(Some(&workspace), &view, &asserted, &decided)?;
                if !failures.is_empty() {
                    return Err(ObserveError::Invalid(format!(
                        "blocking checks fail, so this merge would not reach canon: {}. Fix the proposal or reject it (proposal-workflow.md Section 6)",
                        failures.join("; ")
                    )));
                }
            }
        }
        // The surface marker rides source_ref, engine-stamped - the review surfaces accept no
        // source_ref of their own, so a client cannot mint the console marker (resolution.md R8).
        let prov =
            self.provenance(&workspace, Some(surface.marker().to_string()), None, on_behalf_of);
        let payload = serde_json::json!({ "decision": decision, "note": note }).to_string();
        let content = format!("proposal({decision}) {proposal}");
        let assertions = Assertions {
            proposal_events: vec![ProposalEventAssertion { proposal, event, payload }],
            ..Default::default()
        };
        let obs = Observation::with_assertions(content, prov, assertions);
        let id = obs.id.clone();
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        self.store.add_observation(obs)?;
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(id)
    }

    /// The workspace's observations for this read, fetched on first use and reused after.
    /// Reusing the rows requires the scope to match. On a mismatch this reads through rather than
    /// answering from the wrong scope - correctness first, and the caller merely loses the reuse.
    fn log<'c>(
        &self,
        workspace: Option<&str>,
        cx: &'c ReadCtx,
    ) -> Result<std::borrow::Cow<'c, [Observation]>, StoreError> {
        let epoch = self.log_epoch.load(std::sync::atomic::Ordering::SeqCst);
        if let Some((at, scope, rows)) = cx.observations.get() {
            if *at == epoch && scope.as_deref() == workspace {
                return Ok(std::borrow::Cow::Borrowed(rows));
            }
            // Either the log grew under this context or the scope changed. Reuse is an optimization
            // and this is the case where it does not apply, so read through: at worst that costs
            // what every one of these folds used to cost unconditionally.
            //
            // Deliberately not a debug_assert. Asserting "this must not happen" beside code that
            // handles it correctly would mean the handling is unreachable in the builds where it is
            // tested, and the panic would be the only thing anyone learned about the case. The
            // handling is the answer; the cost is one extra read, and the guard for it is
            // `a_read_context_reuses_rows_only_while_that_changes_nothing`.
            return Ok(std::borrow::Cow::Owned(self.observations(workspace)?));
        }
        let loaded = self.observations(workspace)?;
        let (_, _, rows) =
            cx.observations.get_or_init(|| (epoch, workspace.map(str::to_string), loaded));
        Ok(std::borrow::Cow::Borrowed(rows))
    }

    /// The workspace's log with **re-keyed predecessors folded out** - the enumeration every fold
    /// and every projection reads.
    ///
    /// `migrate` re-creates a pre-formula row under the current content address, and because the log
    /// is append-only (Principle 3) the original stays. The two rows then hold the same content and
    /// the same assertions, so a raw enumeration reports one act as two: a proposal gains a twin no
    /// verdict can ever reference (a proposal's id IS its opening observation's id, so the copy is
    /// permanently open), and an entity's supporting-attestation count doubles - which the
    /// corroboration rules read as extra independent support (Principles 2/18).
    ///
    /// Dedup by content address (Principle 14) is what normally prevents this; a re-keying is
    /// precisely the case where one content wears two ids, so the same rule has to be applied by
    /// hand. A predecessor is recognised structurally rather than by re-hashing every row: the
    /// successor names it in `derived_from` AND carries byte-identical content and assertions AND
    /// lives in the same workspace. Genuine derived knowledge cannot collide with that test -
    /// identical content and assertions under the current formula IS the same content address, so
    /// it would be one row, not two.
    ///
    /// The same-workspace condition is what separates the two re-keyings: a `migrate` successor
    /// shares its predecessor's workspace (the id FORMULA changed), so the pair is one act and the
    /// door folds it to one row; a `rekey_workspace` successor does not (the WORKSPACE changed), so
    /// both rows stay live - each workspace keeps its own support, and the unscoped view is the
    /// union of the scoped ones. Without the condition the unscoped fold dropped the source row,
    /// showing the source workspace's entities without the attestations its own scoped view reports
    /// (a scoped and an unscoped read of one log must not disagree - Principle 16).
    ///
    /// Nothing is deleted (Principle 3). The predecessor stays in the store and stays
    /// dereferenceable by its id; the successor's `derived_from` is the record of the re-keying.
    fn observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        let rows = self.store.all_observations(workspace)?;
        let by_id: HashMap<&str, &Observation> = rows.iter().map(|o| (o.id.as_str(), o)).collect();
        let superseded: HashSet<String> = rows
            .iter()
            .flat_map(|successor| {
                successor.derived_from.iter().filter_map(|parent| {
                    let predecessor = by_id.get(parent.as_str())?;
                    (predecessor.content == successor.content
                        && predecessor.assertions == successor.assertions
                        && predecessor.workspace() == successor.workspace())
                    .then(|| parent.clone())
                })
            })
            .collect();
        if superseded.is_empty() {
            return Ok(rows);
        }
        Ok(rows.into_iter().filter(|o| !superseded.contains(&o.id)).collect())
    }

    /// Folds the proposal events in the workspace into their current states (I2). Solo decision rule:
    /// merge is the absorbing state (I16), then withdrawn, then rejected, else open.
    fn fold_proposals(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<BTreeMap<String, ProposalView>, StoreError> {
        let obss = self.log(workspace, cx)?;
        let obss = obss.as_ref();
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
                    kind,
                    targets,
                    into,
                    tier,
                    rationale,
                    affected_types,
                    state: "open".into(),
                    verdicts: 0,
                    opened_at: observed_at,
                    proposer: proposer.clone(),
                    self_attested: true,
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
                        let t = tally.entry(ev.proposal.clone()).or_default();
                        t.verdicts += 1;
                        match v.get("decision").and_then(|x| x.as_str()) {
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
            let asserted = self.asserted_entity_ids(workspace, cx)?;
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
                    && !self.blocking_failures(workspace, view, &asserted, &decided)?.is_empty()
                {
                    blocked.insert(id.clone());
                }
            }
        }
        for (id, view) in views.iter_mut() {
            if let Some(t) = tally.get(id) {
                view.verdicts = t.verdicts;
                view.state = if t.merge && !blocked.contains(id) {
                    "merged"
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
        let mut v: Vec<ProposalView> = self.fold_proposals(workspace, cx)?.into_values().collect();
        v.sort_by(|a, b| b.opened_at.cmp(&a.opened_at).then_with(|| a.id.cmp(&b.id)));
        Ok(v)
    }

    /// One proposal's folded state by id (None if there is no such proposal).
    pub fn get_proposal(
        &self,
        workspace: Option<&str>,
        id: &str,
    ) -> Result<Option<ProposalView>, StoreError> {
        let cx = &ReadCtx::default();
        let Some(mut view) = self.fold_proposals(workspace, cx)?.remove(id) else {
            return Ok(None);
        };
        view.belief_diff = Some(self.belief_diff(workspace, &view, cx)?);
        view.checks = self.blocking_checks(
            workspace,
            &view,
            &self.asserted_entity_ids(workspace, cx)?,
            &self.decided_merges(workspace, cx)?,
        )?;
        Ok(Some(view))
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
    fn asserted_entity_ids(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<String>, StoreError> {
        let mut ids = HashSet::new();
        for obs in self.log(workspace, cx)?.iter() {
            let ws = obs.workspace().to_string();
            for e in &obs.assertions.entities {
                ids.insert(Entity::make_id(&ws, &e.name));
            }
            // Both endpoints of a relation assert their entities too - the projection creates rows
            // for them, so the check must see them as existing for the same reason.
            for r in &obs.assertions.relations {
                ids.insert(Entity::make_id(&ws, &r.from));
                ids.insert(Entity::make_id(&ws, &r.to));
            }
        }
        Ok(ids)
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
    /// fold that calls this.
    fn blocking_checks(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
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
                             the local log, is another kind, or has no merge verdict yet"
                        ),
                    },
                );
            }
            k if GATE_KINDS.contains(&k) => {
                // Referential integrity: you cannot promote what is not here. Under incomplete sync
                // the targets may simply not have arrived, and merging then would grant a tier to
                // nothing.
                let mut missing = Vec::new();
                for t in &view.targets {
                    if self.store.get_observation(t)?.is_none() {
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
                            "{} target observation(s) are not in the local log: {}",
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
                            "{} target entit(ies) are not asserted in the local log: {}",
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
        let _ = workspace;
        Ok(out)
    }

    /// The blocking checks that FAIL - empty means a merge verdict may take effect.
    fn blocking_failures(
        &self,
        workspace: Option<&str>,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
    ) -> Result<Vec<String>, StoreError> {
        Ok(self
            .blocking_checks(workspace, view, asserted, decided_merges)?
            .into_iter()
            .filter(|c| c.blocking && !c.passed)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect())
    }

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
        if merge.kind != "entity_merge" || !carries_merge_verdict(&merge.state) {
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
    fn belief_diff(
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
        if !GATE_KINDS.contains(&view.kind.as_str()) {
            return Ok(BeliefDiff {
                computable: false,
                note: Some(format!(
                    "{} has no commit effect yet, so there is no difference to compute - an empty diff here would mean 'not computable', never 'changes nothing' (architecture.md Section 14)",
                    view.kind
                )),
                tier_changes: Vec::new(),
                overturned: Vec::new(),
                rewired: Vec::new(),
            });
        }
        let Some(tier) = view.tier else {
            return Ok(BeliefDiff {
                computable: false,
                note: Some("a gate proposal without a requested tier grants nothing".into()),
                tier_changes: Vec::new(),
                overturned: Vec::new(),
                rewired: Vec::new(),
            });
        };

        let fwd = self.merge_forwarding(workspace, cx)?;
        let before_gates = self.gate_grants(workspace, cx)?;
        // The one difference between the two materializations: this proposal's grant applied.
        let mut after_gates = before_gates.clone();
        for t in &view.targets {
            after_gates.insert(t.clone(), tier);
        }

        let mut tier_changes = Vec::new();
        for t in &view.targets {
            if let Some(obs) = self.store.get_observation(t)? {
                let (from, to) =
                    (effective_tier(&obs, &before_gates), effective_tier(&obs, &after_gates));
                if from != to {
                    tier_changes.push(TierChange { observation: t.clone(), from, to });
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
        // Deterministic order (Principle 16): the diff is a read surface like any other.
        overturned.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.entity.cmp(&b.entity)));
        tier_changes.sort_by(|a, b| a.observation.cmp(&b.observation));
        Ok(BeliefDiff {
            computable: true,
            note: None,
            tier_changes,
            overturned,
            rewired: Vec::new(),
        })
    }

    /// The `entity_merge` proposals a verdict has already decided - what an `entity_split` may name
    /// (unmerge.md Section 9). For callers OUTSIDE [`Self::fold_proposals`]; the fold builds the same
    /// set from its own tally, because it cannot ask itself.
    fn decided_merges(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashSet<String>, StoreError> {
        Ok(self
            .fold_proposals(workspace, cx)?
            .values()
            .filter(|p| p.kind == "entity_merge" && carries_merge_verdict(&p.state))
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
    fn reversed_merges(
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
    fn merge_forwarding(
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
    fn forwarding_less(
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
    fn merge_cycle_sets(
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

    /// Gate-tier grants (resolution.md Section 5): target observation id -> the tier set by the
    /// HLC-latest merged claim_promotion/claim_demotion verdict targeting it. Per proposal, the
    /// representative verdict is the earliest merge by (ordering HLC, observation id) - the
    /// proposal-workflow 7.1 canonicalization; what it grants is min(requested tier, the surface
    /// ceiling of the log-borne marker on that verdict) (resolution.md Section 6). A pure
    /// fold-projection of the log - converges continuously (F5), and a gate event overrides the base
    /// evaluation in BOTH directions (a merged demotion can push below base - the fast-path).
    ///
    /// A grant is a merge's COMMIT EFFECT, so it exists only where [`Engine::fold_proposals`] says
    /// the merge committed (state `merged`) - a verdict on a proposal whose blocking checks fail
    /// folds to `blocked` and grants nothing (I13), exactly as `merge_forwarding` already refuses
    /// to forward for one. Without this coupling the two folds can disagree: a claim_promotion
    /// whose targets are only partly synced would read `blocked` on the proposal surface while its
    /// present targets were already promoted. Skipping is the safe direction and stays monotone -
    /// when the missing target arrives the state flips to `merged` and the grant applies (the same
    /// blocked -> merged direction the blocking checks are pinned to).
    fn gate_grants(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<HashMap<String, TrustTier>, StoreError> {
        // proposal id -> (targets, requested tier); collected from opened gate-kind events.
        let mut opened: HashMap<String, (Vec<String>, TrustTier)> = HashMap::new();
        // proposal id -> representative merge verdict (ordering hlc, verdict obs id, source_ref).
        let mut rep_merge: HashMap<String, (Hlc, String, Option<String>)> = HashMap::new();
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
                        opened.insert(obs.id.clone(), (targets, tier));
                    }
                    ProposalEventKind::Verdict => {
                        if v.get("decision").and_then(|x| x.as_str()) != Some("merge") {
                            continue;
                        }
                        // The verdict's surface marker rides the authoring attestation's source_ref
                        // (engine-stamped, resolution.md R8).
                        let source_ref =
                            authoring_attestation(obs).and_then(|p| p.source_ref.clone());
                        let cand = (okey.clone(), obs.id.clone(), source_ref);
                        rep_merge
                            .entry(ev.proposal.clone())
                            .and_modify(|cur| {
                                if (&cand.0, cand.1.as_str()) < (&cur.0, cur.1.as_str()) {
                                    *cur = cand.clone();
                                }
                            })
                            .or_insert(cand);
                    }
                    _ => {}
                }
            }
        }
        // The state fold is the single answer to "did this merge commit" (I13): consult it rather
        // than re-deriving blocking here, so the two can never drift apart again. Shares the
        // ReadCtx-cached log, so this adds no store enumeration.
        let states = if rep_merge.is_empty() {
            BTreeMap::new() // no merge verdicts anywhere - nothing to gate, skip the fold
        } else {
            self.fold_proposals(workspace, cx)?
        };
        // Per target, the HLC-latest gate event governs (tie: proposal id) - resolution.md R5.
        let mut grants: HashMap<String, (Hlc, String, TrustTier)> = HashMap::new();
        for (proposal_id, (targets, requested)) in &opened {
            let Some((verdict_hlc, _, source_ref)) = rep_merge.get(proposal_id) else {
                continue; // not merged - grants nothing (the gate, Principle 23)
            };
            if states.get(proposal_id).is_none_or(|p| p.state != "merged") {
                continue; // the fold says this merge did not commit (blocked) - no effect (I13)
            }
            let granted = (*requested).min(verdict_grant_ceiling(source_ref.as_deref()));
            for target in targets {
                let cand = (verdict_hlc.clone(), proposal_id.clone(), granted);
                grants
                    .entry(target.clone())
                    .and_modify(|cur| {
                        if (&cand.0, cand.1.as_str()) > (&cur.0, cur.1.as_str()) {
                            *cur = cand.clone();
                        }
                    })
                    .or_insert(cand);
            }
        }
        Ok(grants.into_iter().map(|(t, (_, _, tier))| (t, tier)).collect())
    }

    /// Per-canonical-entity belief fold over the observation log (resolution.md Sections 2-4):
    /// kind candidates (each at its observation's EFFECTIVE tier) and the node's representative
    /// effective tier. Shared by the graph projection, the curation report, and the entity view -
    /// a pure fold, so it converges continuously (F5) and needs no re-materialization to be current.
    fn belief_fold(
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
    fn resolve_kind(
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

    /// The resolution write path (resolution-identity.md Section 4): (re)projects entity rows purely
    /// from the observation log. `only = None` projects every entity in the workspace (reproject);
    /// `only = Some(ids)` projects just those (the incremental observe path). Both run the SAME fold,
    /// so the incremental projection of a write equals a fresh replay's row for the same log (IR3) -
    /// there is no field-wise last-write interim to diverge.
    ///
    /// For each in-scope entity, purely from the log:
    /// - **kind**: the policy winner over kind candidates at their effective tiers (M3a).
    /// - **canonical_name**: the policy winner over the asserted spellings (M3a, arrival-order-free).
    /// - **aliases**: the distinct asserted spellings minus the representative, ordered by
    ///   (first-asserting ordering-HLC, spelling) - a deterministic set union that never drops a
    ///   spelling (Principle 3, IR1).
    /// - **description**: HLC-latest non-empty (never erased by a later omission - Principle 8).
    /// - **embedding**: the name-meaning recall aid (Principle 19), recomputed only when the
    ///   embedding text (canonical_name + aliases) changed since the stored row (IR4) - so it is
    ///   never silently stale, and unchanged rows do not re-hit the probabilistic adapter.
    ///
    /// Same log -> same rows on every node/call (P16). Entity `properties` (not modeled by any
    /// assertion yet) are carried forward from the stored row rather than reset.
    fn project_entities(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        cx: &ReadCtx,
    ) -> Result<usize, StoreError> {
        let gates = self.gate_grants(Some(ws), cx)?;
        let mut obss = self.observations(Some(ws))?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });

        let mut name_cands: HashMap<String, Vec<BeliefCandidate>> = HashMap::new();
        let mut kind_cands: HashMap<String, Vec<BeliefCandidate>> = HashMap::new();
        let mut prov: HashMap<String, Vec<Provenance>> = HashMap::new();
        let mut descr: HashMap<String, String> = HashMap::new(); // HLC-latest (ascending replay -> last wins)
        let want = |id: &str| only.is_none_or(|s| s.contains(id));

        for obs in &obss {
            let eff = effective_tier(obs, &gates);
            let hlc = ordering_hlc(obs);
            // Entity ids this observation touches (deduped), so each observation's attestation set is
            // credited to an entity's provenance exactly once - an entity named as both an assertion
            // and a relation endpoint in the same observation is one supporting observation, not two.
            let mut touched_here: BTreeSet<String> = BTreeSet::new();
            let mut touch =
                |id: String, spelling: &str, kind: Option<&str>, description: Option<&str>| {
                    if !want(&id) {
                        return;
                    }
                    name_cands.entry(id.clone()).or_default().push(BeliefCandidate {
                        value: spelling.trim().to_string(),
                        tier: eff,
                        hlc: hlc.clone(),
                        observation: obs.id.clone(),
                    });
                    if let Some(k) = kind {
                        kind_cands.entry(id.clone()).or_default().push(BeliefCandidate {
                            value: k.to_string(),
                            tier: eff,
                            hlc: hlc.clone(),
                            observation: obs.id.clone(),
                        });
                    }
                    if let Some(d) = description {
                        descr.insert(id.clone(), d.to_string()); // ascending replay: highest HLC wins
                    }
                    touched_here.insert(id);
                };
            for ea in &obs.assertions.entities {
                touch(
                    Entity::make_id(ws, &ea.name),
                    &ea.name,
                    ea.kind.as_deref(),
                    ea.description.as_deref(),
                );
            }
            for ra in &obs.assertions.relations {
                touch(Entity::make_id(ws, &ra.from), &ra.from, None, None);
                touch(Entity::make_id(ws, &ra.to), &ra.to, None, None);
            }
            // Credit ALL of this observation's attestations to each entity it supports (Principle 3:
            // the entity provenance reflects every attestation in the log, not just a representative -
            // so re-projecting recovers the same count the incremental write produced, IR3).
            for id in touched_here {
                prov.entry(id).or_default().extend(obs.provenance.iter().cloned());
            }
        }

        let mut count = 0;
        for (id, ncs) in &name_cands {
            let canonical =
                self.policy.choose(ncs).map(|c| ncs[c.index].value.clone()).unwrap_or_default();
            let aliases = alias_set(ncs, &canonical);
            let kind = kind_cands
                .get(id)
                .and_then(|ks| self.policy.choose(ks).map(|c| ks[c.index].value.clone()))
                .unwrap_or_else(|| "Concept".to_string());
            let mut entity = Entity {
                id: id.clone(),
                kind,
                canonical_name: canonical,
                aliases,
                description: descr.get(id).cloned(),
                properties: serde_json::Value::Null,
                provenance: prov.get(id).cloned().unwrap_or_default(),
                embedding: None,
            };
            let text = entity_text(&entity);
            if let Some(existing) = self.store.get_entity(id)? {
                // Keep the stored embedding ONLY if its source text is unchanged (IR4: never stale).
                if existing.embedding.is_some() && entity_text(&existing) == text {
                    entity.embedding = existing.embedding;
                }
                entity.properties = existing.properties;
            }
            if entity.embedding.is_none() {
                if let Some(embedder) = &self.embedder {
                    match embedder.embed_one(&text) {
                        Ok(vec) => entity.embedding = Some(vec),
                        Err(e) => tracing::warn!(
                            entity_id = %entity.id, name = %entity.canonical_name, error = %e,
                            "entity embedding failed - stored without name-meaning recall (degrade)"
                        ),
                    }
                }
            }
            self.store.put_entity(entity)?;
            count += 1;
        }
        Ok(count)
    }

    /// Rebuilds the materialized entity/relation projection of a workspace from the observation log,
    /// replayed in (ordering HLC, id) order - the deterministic re-materialization step after sync
    /// apply (F1/F3; architecture.md: "orders by HLC, then re-materializes"). Same log -> same replay
    /// order -> same materialization on every node (P16/F5), unlike observe-time upserts whose
    /// last-write-wins fields depend on local arrival order. Builds fresh states and upserts them
    /// (idempotent: a second run writes identical rows); rows with no support in the log are left in
    /// place - removal is a curation concern, not reprojection's (Principle 3).
    /// Re-create a workspace's knowledge under another workspace name, provenance intact.
    ///
    /// The workspace is part of the content address, so this is not a move and cannot be: a
    /// re-keyed observation is a NEW observation, and the original stays where it is (Principle 3).
    /// What makes it a re-key rather than a re-ingest is that **every attestation is copied
    /// verbatim** - acting host, `on_behalf_of`, `observed_at`, confidence. Pushing the same text
    /// back through `observe` would restamp all of it with this engine's clock and host, which
    /// fabricates the two things the log exists to preserve (Principles 2 and 4) and collapses the
    /// HLC order that last-write-wins fields are decided by. The one field dropped is the sync
    /// stamp: it is bound to the old content id and signs it, so it cannot follow - and because
    /// `evaluated_tier` trusts a stamp-less claim at face value, the claimed tier is carried as its
    /// **pre-strip evaluation** rather than verbatim (a synced claim stays capped at HostSigned;
    /// P18: dropping the stamp must not raise what the claim evaluates to).
    ///
    /// Shaped exactly like `migrate_legacy_ids`, which re-keys across a change of id FORMULA; this
    /// re-keys across a change of WORKSPACE. Lineage records the origin either way, but the
    /// live-set door treats the two differently on purpose: a migrated pair shares one workspace
    /// and folds to one row, while a re-keyed pair spans two and BOTH rows stay live - each
    /// workspace keeps its own support, and the unscoped view is the union of the scoped ones
    /// (see [`Engine::observations`]).
    ///
    /// Proposal events are left behind deliberately - see [`RekeyReport::skipped_proposal_events`].
    /// `dry_run` reports what would move and writes nothing.
    /// Projects relations from the log - the edge half of what [`Engine::project_entities`] does for
    /// nodes, and shared by `observe` and `reproject` for the same reason (IR3): if the incremental
    /// write and a fresh replay run different code, they are two answers to one question and the
    /// projection stops being a function of the log.
    ///
    /// They WERE different. `observe` stamped each edge with the attestation of the call that wrote
    /// it, while `reproject` used [`authoring_attestation`] over the HLC-latest observation asserting
    /// that edge. The two agree on a fresh single-attestation observation and part company as soon as
    /// one absorbs a second attestation, or as soon as two observations assert the same edge - so a
    /// reproject could move an edge's tier and confidence with no change in the log. Commit 3a04ece
    /// already collected this rule into one helper for the proposal fold, the gate fold and
    /// reprojection; this is the fourth site it missed.
    ///
    /// `only` bounds the work to the edges a single observe touched. It is a filter on which rows are
    /// WRITTEN, never on which are read: the winner for an edge is still chosen across every
    /// observation asserting it, so narrowing the write set cannot change what gets written.
    fn project_relations(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
    ) -> Result<usize, StoreError> {
        let mut obss = self.observations(Some(ws))?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });
        let mut rels: BTreeMap<String, Relation> = BTreeMap::new();
        for obs in &obss {
            let Some(prov) = authoring_attestation(obs).cloned() else {
                continue;
            };
            for ra in &obs.assertions.relations {
                let from = Entity::make_id(ws, &ra.from);
                let to = Entity::make_id(ws, &ra.to);
                let kind = normalize_relation_kind(&ra.kind);
                let id = Relation::make_id(&from, &kind, &to);
                if only.is_some_and(|s| !s.contains(&id)) {
                    continue;
                }
                // Ascending replay, upsert by id: description and valid interval are last-write, which
                // is the store's own semantics given a deterministic order.
                rels.insert(
                    id.clone(),
                    Relation {
                        id,
                        from,
                        to,
                        kind,
                        description: ra.description.clone(),
                        provenance: prov.clone(),
                        // The client's valid interval is projected as-is (Principle 4 capture);
                        // derivation such as auto-closing valid_to on refutation is M3c.
                        valid_from: ra.valid_from,
                        valid_to: ra.valid_to,
                    },
                );
            }
        }
        let written = rels.len();
        for (_, r) in rels {
            self.store.add_relation(r)?;
        }
        Ok(written)
    }

    pub fn rekey_workspace(
        &self,
        from: &str,
        to: &str,
        dry_run: bool,
    ) -> Result<RekeyReport, StoreError> {
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() || to.is_empty() {
            return Err(StoreError::Backend("both workspaces must be named".into()));
        }
        if from == to {
            return Err(StoreError::Backend(format!(
                "source and target are the same workspace ('{from}') - nothing to re-key"
            )));
        }
        let _guard = self.write_guard.lock().unwrap_or_else(|e| e.into_inner());
        let mut report = RekeyReport::default();
        for obs in self.observations(Some(from))? {
            if !obs.assertions.proposal_events.is_empty() {
                report.skipped_proposal_events += 1;
                continue;
            }
            let mut provs = obs.provenance.clone();
            for p in &mut provs {
                p.workspace = to.to_string();
                // Clamp BEFORE the stamp drops: `evaluated_tier` trusts a stamp-less claim at
                // face value, so a synced claim carried verbatim past this line would evaluate
                // above HostSigned in the target workspace - a tier promotion by an operator
                // act (P18: the tier is the receiver's evaluation, and a re-key must not raise it).
                p.trust_tier = evaluated_tier(p);
                p.sync = None; // the stamp signs the OLD content id and cannot follow it
            }
            if provs.is_empty() {
                continue; // unreachable by construction (P2), but never panic on a stored row
            }
            let first = provs.remove(0);
            let mut fresh =
                Observation::with_assertions(obs.content.clone(), first, obs.assertions.clone());
            for p in provs {
                let mut copy =
                    Observation::with_assertions(obs.content.clone(), p, obs.assertions.clone());
                copy.derived_from = Vec::new();
                fresh.absorb(copy); // union semantics - the attestation set is preserved whole
            }
            fresh.derived_from = obs.derived_from.clone();
            fresh.derived_from.push(obs.id.clone());
            fresh.derived_from.sort();
            fresh.derived_from.dedup();
            // Idempotent: a second run finds the target already recording this origin and stops.
            if let Some(existing) = self.store.get_observation(&fresh.id)? {
                if existing.derived_from.contains(&obs.id) {
                    report.already += 1;
                    continue;
                }
            }
            if !dry_run {
                self.store.add_observation(fresh)?;
            }
            report.moved += 1;
        }
        if !dry_run && report.moved > 0 {
            self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(report)
    }

    pub fn reproject(&self, workspace: Option<&str>) -> Result<ReprojectReport, StoreError> {
        let cx = &ReadCtx::default();
        let ws = workspace.unwrap_or(&self.default_workspace).to_string();
        // Entities: the same resolution write path the incremental observe uses, over ALL entities
        // (only = None) - so a reproject and an incremental write agree row-for-row (IR3).
        let entities = self.project_entities(&ws, None, cx)?;

        // The same fold the incremental write runs, over every edge rather than one observe's.
        let relations = self.project_relations(&ws, None)?;
        Ok(ReprojectReport {
            observations: self.observations(Some(&ws))?.len(),
            entities,
            relations,
        })
    }

    /// The shared store port - the federation sync layer operates on the same log the engine
    /// projects from (M4 Phase 4 wiring; one process, one store, one clock).
    ///
    /// Deliberately narrowed to [`AssertionStore`] (Principle 1, third enforcement demand). The engine
    /// holds the full [`KnowledgeStore`] and is the only thing that does; what it hands out can append
    /// to the log and read the graph, but cannot write an entity or a relation row. Knowledge reaches
    /// the projection through `observe`/`apply` and the folds those run, or it does not reach it at all.
    pub fn store(&self) -> Arc<dyn AssertionStore> {
        self.store.clone()
    }

    /// Observation dereference (Principle 2/14): from an observation id returned by a search hit / derivation
    /// lineage, reach the original text, the full provenance, and the derived_from lineage - the terminus of "where did this answer come from".
    pub fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        self.store.get_observation(id)
    }

    /// Entity + relation lookup. `Ok(None)` is absence (unknown, Principle 5), `Err` is a store failure -
    /// failures are not swallowed, so the caller (the MCP surface) can distinguish and relay the two.
    pub fn get_entity(&self, id: &str) -> Result<Option<EntityView>, StoreError> {
        let cx = &ReadCtx::default();
        let Some(row) = self.store.get_entity(id)? else {
            return Ok(None);
        };
        let ws = row.provenance.first().map(|p| p.workspace.clone());
        let fwd = self.merge_forwarding(ws.as_deref(), cx)?;
        // Principle 14: a merged-away id keeps forwarding to its canonical entity - a lookup by any
        // pre-merge id dereferences to the surviving row (the log keeps both; un-merge is a proposal).
        let canon_id = fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut entity = if canon_id == *id {
            row
        } else {
            self.store.get_entity(&canon_id)?.unwrap_or(row)
        };
        let relations = self.store.relations_of(&canon_id)?;
        // Union merged-away names into aliases so get_entity sees the same alias set as the graph
        // fold (the write path already materialized same-id spelling aliases on the row - IR1).
        if !fwd.is_empty() {
            let mut merged: Vec<String> = self
                .store
                .all_entities(ws.as_deref())?
                .into_iter()
                .filter(|e| fwd.get(&e.id).is_some_and(|c| *c == canon_id))
                .map(|e| e.canonical_name)
                .filter(|n| *n != entity.canonical_name && !entity.aliases.contains(n))
                .collect();
            merged.sort();
            merged.dedup();
            entity.aliases.extend(merged);
        }
        // Belief overlay (resolution.md Section 4.2), scoped to this entity's workspace so the agent
        // surface sees the same policy-current kind/contested state as the viewer.
        let gates = self.gate_grants(ws.as_deref(), cx)?;
        let belief = self.belief_fold(ws.as_deref(), &fwd, &gates, cx)?;
        let (winner, contested, competitors) = self.resolve_kind(belief.kinds.get(&canon_id));
        let mut kind_source = None;
        if let Some((k, obs)) = winner {
            entity.kind = k;
            kind_source = Some(obs);
        }
        let effective_tier = belief.tiers.get(&canon_id).copied().unwrap_or_else(|| {
            entity.provenance.iter().map(evaluated_tier).max().unwrap_or_default()
        });
        Ok(Some(EntityView {
            entity,
            relations,
            effective_tier,
            contested,
            competitors,
            kind_source,
        }))
    }

    /// The observation log for a workspace, flattened for the log-browser surface (observability).
    /// The log is the source of truth (Principle 1); this is a read-only projection of it, ordered
    /// newest-first by the deterministic fold key (ordering HLC desc, then id asc - P16), each row
    /// carrying its provenance (Principle 2) and effective tier (resolution.md Section 3). A storage
    /// failure is `Err`, never an empty log (Principle 5). When `entity` is given, only observations
    /// that touch that entity (an assertion or a relation endpoint, forwarded through accepted
    /// merges) are returned - the evidence set behind one node. `limit` keeps the newest N (None = all).
    pub fn observation_log(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        let cx = &ReadCtx::default();
        let fwd = self.merge_forwarding(workspace, cx)?;
        let gates = self.gate_grants(workspace, cx)?;
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
        let want = entity.map(|e| canon(e.to_string()));
        // Proposal-event rows are unreadable as stored (machine text, fixed inside the content
        // address), so they are translated below. Both inputs are loaded ONLY if such a row exists:
        // the fold is over rows already in the ReadCtx, and the name lookup is the one store scan,
        // which a workspace with no proposals never pays. The log panel is opened on demand, not
        // polled like the graph, so that scan is affordable where it would not be on the poll path.
        let has_events = self
            .log(workspace, cx)?
            .iter()
            .any(|o| !o.assertions.proposal_events.is_empty());
        let proposals =
            if has_events { self.fold_proposals(workspace, cx)? } else { BTreeMap::new() };
        let names: BTreeMap<String, String> = if has_events {
            self.store
                .all_entities(workspace)?
                .into_iter()
                .map(|e| (e.id, e.canonical_name))
                .collect()
        } else {
            BTreeMap::new()
        };
        // A target keeps its id when nothing can name it - a merged-away row still resolves here
        // because the entity table keeps it; only a target this node has never projected stays bare.
        let eref = |id: &str| EntityRef {
            name: names.get(id).cloned().unwrap_or_else(|| id.to_string()),
            id: id.to_string(),
        };
        let mut out: Vec<ObsSummary> = Vec::new();
        for obs in self.log(workspace, cx)?.iter() {
            let ws = obs.workspace().to_string();
            // Canonical entity refs this observation asserts (assertions + relation endpoints),
            // keyed by canonical id (deterministic BTreeMap order), first spelling kept for display.
            let mut ent_ids: BTreeMap<String, String> = BTreeMap::new();
            for ea in &obs.assertions.entities {
                ent_ids
                    .entry(canon(Entity::make_id(&ws, &ea.name)))
                    .or_insert_with(|| ea.name.clone());
            }
            for ra in &obs.assertions.relations {
                for name in [&ra.from, &ra.to] {
                    ent_ids
                        .entry(canon(Entity::make_id(&ws, name)))
                        .or_insert_with(|| name.clone());
                }
            }
            if let Some(w) = &want {
                if !ent_ids.contains_key(w) {
                    continue;
                }
            }
            let attestations = obs
                .provenance
                .iter()
                .map(|p| AttestationSummary {
                    host: p.host.clone(),
                    on_behalf_of: p.on_behalf_of.clone(),
                    source_ref: p.source_ref.clone(),
                    observed_at: p.observed_at,
                    confidence: p.confidence,
                    trust_tier: p.trust_tier,
                    evaluated_tier: evaluated_tier(p),
                    origin_node: p.sync.as_ref().map(|s| s.origin_node.clone()),
                })
                .collect();
            let entities: Vec<EntityRef> =
                ent_ids.into_iter().map(|(id, name)| EntityRef { name, id }).collect();
            // The readable form of a proposal event. `opened` names its own proposal (the id IS this
            // observation); every other event points at one, and the view supplies kind/targets that
            // the event itself does not carry.
            let proposal = obs.assertions.proposal_events.first().map(|ev| {
                let pid = if ev.proposal.is_empty() { obs.id.clone() } else { ev.proposal.clone() };
                let payload: serde_json::Value =
                    serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                let view = proposals.get(&pid);
                ProposalEventSummary {
                    event: match ev.event {
                        ProposalEventKind::Opened => "opened",
                        ProposalEventKind::Verdict => "verdict",
                        ProposalEventKind::Withdrawn => "withdrawn",
                        ProposalEventKind::Comment => "comment",
                    }
                    .to_string(),
                    decision: payload.get("decision").and_then(|d| d.as_str()).map(str::to_string),
                    kind: view.map(|v| v.kind.clone()).unwrap_or_default(),
                    state: view.map(|v| v.state.clone()).unwrap_or_default(),
                    targets: view
                        .map(|v| v.targets.iter().map(|t| eref(t)).collect())
                        .unwrap_or_default(),
                    into: view.and_then(|v| v.into.as_deref()).map(eref),
                    proposal: pid,
                }
            });
            let relations = obs
                .assertions
                .relations
                .iter()
                .map(|ra| RelationRef {
                    from: ra.from.clone(),
                    kind: normalize_relation_kind(&ra.kind),
                    to: ra.to.clone(),
                })
                .collect();
            out.push(ObsSummary {
                hlc: ordering_hlc(obs),
                effective_tier: effective_tier(obs, &gates),
                id: obs.id.clone(),
                content: obs.content.clone(),
                proposal,
                derived_from: obs.derived_from.clone(),
                attestations,
                entities,
                relations,
            });
        }
        // Newest first: descending ordering HLC, ascending id as the stable tiebreak (P16).
        out.sort_by(|a, b| b.hlc.cmp(&a.hlc).then_with(|| a.id.cmp(&b.id)));
        if let Some(n) = limit {
            out.truncate(n);
        }
        Ok(out)
    }

    /// "Why is this node projected this way" (observability): the per-field belief resolution
    /// (evidence + decision) and the supporting observation log for one entity. Built ON TOP of
    /// [`Engine::get_entity`] - the winner values ARE the projected entity's values, so the
    /// explanation cannot disagree with what the graph shows (never a second computation that could
    /// drift). The candidate rows and supporting log are folded from the same log the projection
    /// uses. `None` iff the id resolves to no entity (absence, Principle 5). A merged-away id
    /// forwards to its canonical entity (Principle 15), like [`Engine::get_entity`].
    pub fn explain_entity(&self, entity_id: &str) -> Result<Option<EntityExplain>, StoreError> {
        let cx = &ReadCtx::default();
        let Some(view) = self.get_entity(entity_id)? else {
            return Ok(None);
        };
        let canon_id = view.entity.id.clone();
        let ws = view.entity.provenance.first().map(|p| p.workspace.clone());
        let fwd = self.merge_forwarding(ws.as_deref(), cx)?;
        let gates = self.gate_grants(ws.as_deref(), cx)?;
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);

        // Rebuild this entity's name/kind belief candidates from the log, at the same effective tier
        // and fold order as project_entities/belief_fold - so the ranking matches the projection.
        let mut name_cands: Vec<BeliefCandidate> = Vec::new();
        let mut kind_cands: Vec<BeliefCandidate> = Vec::new();
        for obs in self.log(ws.as_deref(), cx)?.iter() {
            let obs_ws = obs.workspace().to_string();
            let eff = effective_tier(obs, &gates);
            let hlc = ordering_hlc(obs);
            for ea in &obs.assertions.entities {
                if canon(Entity::make_id(&obs_ws, &ea.name)) != canon_id {
                    continue;
                }
                name_cands.push(BeliefCandidate {
                    value: ea.name.trim().to_string(),
                    tier: eff,
                    hlc: hlc.clone(),
                    observation: obs.id.clone(),
                });
                if let Some(k) = &ea.kind {
                    kind_cands.push(BeliefCandidate {
                        value: k.clone(),
                        tier: eff,
                        hlc: hlc.clone(),
                        observation: obs.id.clone(),
                    });
                }
            }
            for ra in &obs.assertions.relations {
                for name in [&ra.from, &ra.to] {
                    if canon(Entity::make_id(&obs_ws, name)) == canon_id {
                        name_cands.push(BeliefCandidate {
                            value: name.trim().to_string(),
                            tier: eff,
                            hlc: hlc.clone(),
                            observation: obs.id.clone(),
                        });
                    }
                }
            }
        }

        // canonical_name: the projected name is the winner; every other spelling is an alias (IR1).
        let name_contested = self.policy.choose(&name_cands).map(|c| c.contested).unwrap_or(false);
        let fields = vec![
            FieldExplain {
                field: "canonical_name",
                winner: view.entity.canonical_name.clone(),
                contested: name_contested,
                candidates: candidate_rows(&name_cands, &view.entity.canonical_name, "alias"),
            },
            // kind: the projected kind is the winner; non-winning kinds are competitors (R7).
            // contested is get_entity's own flag (the identical resolve_kind result).
            FieldExplain {
                field: "kind",
                winner: view.entity.kind.clone(),
                contested: view.contested,
                candidates: candidate_rows(&kind_cands, &view.entity.kind, "competitor"),
            },
        ];

        let supporting = self.observation_log(ws.as_deref(), Some(&canon_id), None)?;
        Ok(Some(EntityExplain {
            id: canon_id,
            name: view.entity.canonical_name,
            effective_tier: view.effective_tier,
            fields,
            supporting,
        }))
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
        let mode = if qvec.is_some() { SearchMode::Hybrid } else { SearchMode::Keyword };
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
        self.graph_in(workspace, &ReadCtx::default())
    }

    /// [`Engine::graph`] over an existing read context, so a caller that already loaded the
    /// log does not load it again (see [`ReadCtx`]).
    fn graph_in(&self, workspace: Option<&str>, cx: &ReadCtx) -> Result<GraphView, StoreError> {
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

        // Edges: rewire endpoints through canon, drop merge self-loops and duplicates, count degree.
        let mut degree: HashMap<String, usize> = HashMap::new();
        let mut seen: HashSet<(String, String, String)> = HashSet::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        for r in &relations {
            let (f, t) = (canon(&r.from), canon(&r.to));
            if f == t || !node_ids.contains(f.as_str()) || !node_ids.contains(t.as_str()) {
                continue; // self-loop from a merge, or an endpoint outside the node set
            }
            if !seen.insert((f.clone(), r.kind.clone(), t.clone())) {
                continue; // the merge can produce duplicate edges - keep the lowest relation id
            }
            *degree.entry(f.clone()).or_default() += 1;
            *degree.entry(t.clone()).or_default() += 1;
            edges.push(GraphEdge {
                from: f,
                to: t,
                kind: r.kind.clone(),
                description: r.description.clone(),
                // Receiver-evaluated, never the raw claim (resolution.md Section 3, F13).
                trust_tier: evaluated_tier(&r.provenance),
                confidence: r.provenance.confidence,
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
    fn hypergraph_in(
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
        for obs in self.observations(ws)? {
            if co_asserted_members(&obs, &node_ids, &fwd) == h.members {
                derived_from.push(obs.id.clone());
            }
        }
        derived_from.sort();
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

/// The alias set for an entity (resolution-identity.md Section 2, IR1): the distinct asserted
/// spellings minus the representative (`canonical`), ordered by (first-asserting ordering-HLC,
/// spelling). A deterministic set union - the same candidate set yields the same aliases on any
/// node (P16), and a spelling that lost the representative choice is never dropped (Principle 3).
/// Collapses belief candidates to one row per distinct value for the explain surface: each value at
/// its highest effective tier, tie-broken to the smallest observation id (the same representative
/// choice as [`Engine::resolve_kind`]'s competitors - P16). The row equal to `winner` is tagged
/// "winner"; every other value gets `loser_role` ("alias" for names, "competitor" for kinds).
/// Ordered winner-first, then (tier desc, value asc) - a stable, arrival-order-free ranking.
fn candidate_rows(
    cands: &[BeliefCandidate],
    winner: &str,
    loser_role: &'static str,
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
    let mut rows: Vec<CandidateRow> = best
        .into_iter()
        .map(|(value, (tier, obs, hlc))| CandidateRow {
            value: value.to_string(),
            role: if value == winner { "winner" } else { loser_role },
            trust_tier: tier,
            hlc: hlc.clone(),
            observation: obs.to_string(),
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

fn alias_set(cands: &[BeliefCandidate], canonical: &str) -> Vec<String> {
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

/// The text to embed for an entity: canonical name + aliases (if any). Opens semantic recall by the meaning of the name.
/// Since aliases hold notation variants, embedding them together widens the reach to other notations of the same target.
fn entity_text(entity: &Entity) -> String {
    if entity.aliases.is_empty() {
        entity.canonical_name.clone()
    } else {
        format!("{} {}", entity.canonical_name, entity.aliases.join(" "))
    }
}

/// The workspace an entity belongs to. The id is blake3(workspace + normalized name), so an entity
/// lives in exactly one workspace and every attestation agrees; the first is representative.
/// Candidate generators need this because a report built with `workspace: None` (the viewer's
/// all-workspaces view) otherwise groups across workspaces, and an entity_merge spanning two of them
/// is not a merge anyone can act on - the proposal has to be filed somewhere, and neither is right.
fn entity_workspace(e: &Entity) -> &str {
    e.provenance.first().map(|p| p.workspace.as_str()).unwrap_or("")
}

/// Separator/case-insensitive name key: keep alphanumerics and lowercase. Unicode-aware via
/// `is_alphanumeric`, so a Korean or otherwise non-Latin name survives intact instead of collapsing to
/// the empty string. `TrustTier`, `Trust Tier` and `trust-tier` all fold to `trusttier` - a collision
/// the entity id key (`trim`+`lowercase` only) deliberately keeps apart.
fn variant_key(name: &str) -> String {
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
fn plural_fold(key: &str) -> String {
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
) -> Vec<NameVariantGroup> {
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
            if pairs.iter().all(|p| seen_pairs.contains(p)) {
                continue;
            }
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
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::{SyncMeta, TypeDefAssertion};
    use supragnosis_store::InMemoryStore;

    /// guard: a [`ReadCtx`] reuses rows only while reusing them is indistinguishable from reading
    /// again. Both ways it can stop being indistinguishable are checked here, because both were
    /// introduced by the change that added it and neither was caught by a test - they were caught by
    /// rereading the diff, which is the kind of guarantee this suite exists to replace.
    #[test]
    fn a_read_context_reuses_rows_only_while_that_changes_nothing() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "h", "ws1");
        let observe = |ws: &str, name: &str| {
            engine
                .observe(ObserveInput {
                    content: format!("{name} in {ws}"),
                    workspace: Some(ws.to_string()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: name.into(),
                        kind: None,
                        description: None,
                    }],
                    relations: vec![],
                })
                .expect("observe");
        };
        observe("ws1", "Alpha");
        observe("ws2", "Beta");

        let cx = ReadCtx::default();
        assert_eq!(engine.log(Some("ws1"), &cx).unwrap().len(), 1, "one observation in ws1");

        // Scope: the context holds ws1's rows, and a ws2 read must not be answered from them. A
        // cache that ignored the scope would not be slower here, it would be wrong.
        assert_eq!(
            engine.log(Some("ws2"), &cx).unwrap().len(),
            1,
            "a different scope must be read through, not answered from the loaded one"
        );
        assert_eq!(
            engine.log(None, &cx).unwrap().len(),
            2,
            "the unscoped read sees both workspaces"
        );

        // Freshness: appending through the engine moves the log epoch, so the same context stops
        // reusing. Without this the rule "never read through a context after writing" would hold
        // only as long as nobody arranged the calls in the other order.
        observe("ws1", "Gamma");
        assert_eq!(
            engine.log(Some("ws1"), &cx).unwrap().len(),
            2,
            "a context that outlived a write must not serve the rows from before it"
        );
    }

    /// guard: sharing a context across the folds inside one call does not change what the call
    /// answers. `curation` embeds the graph and hypergraph projections and now computes them over
    /// its own context; if reuse ever altered a result, the report would silently disagree with the
    /// surfaces it is supposed to be reporting on.
    #[test]
    fn a_shared_context_answers_what_separate_reads_answer() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "h", "ws1");
        for i in 0..6 {
            engine
                .observe(ObserveInput {
                    content: format!("fact {i}"),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![
                        EntityInput {
                            name: format!("E{i}"),
                            kind: Some("Concept".into()),
                            description: None,
                        },
                        EntityInput { name: format!("E{}", i + 1), kind: None, description: None },
                    ],
                    relations: vec![RelationInput {
                        from: format!("E{i}"),
                        kind: "relates_to".into(),
                        to: format!("E{}", i + 1),
                        description: None,
                        valid_from: None,
                        valid_to: None,
                    }],
                })
                .expect("observe");
        }
        let ws = Some("ws1");
        // Each surface, computed standalone (its own context) and again inside curation's shared one.
        let standalone_graph = serde_json::to_string(&engine.graph(ws).unwrap()).unwrap();
        let standalone_hyper = serde_json::to_string(&engine.hypergraph(ws).unwrap()).unwrap();
        let cx = ReadCtx::default();
        let shared_graph = serde_json::to_string(&engine.graph_in(ws, &cx).unwrap()).unwrap();
        let shared_hyper = serde_json::to_string(&engine.hypergraph_in(ws, &cx).unwrap()).unwrap();
        assert_eq!(
            standalone_graph, shared_graph,
            "graph must not depend on whose context it ran in"
        );
        assert_eq!(
            standalone_hyper, shared_hyper,
            "hypergraph must not depend on whose context it ran in"
        );
        // And the same context answering both in sequence is still right for the second one.
        let again = serde_json::to_string(&engine.graph_in(ws, &cx).unwrap()).unwrap();
        assert_eq!(standalone_graph, again, "a reused context must stay correct on later reads");
    }

    /// M4 Phase 2 (F5, engine level): two nodes author overlapping knowledge, exchange their logs via
    /// the sync pipeline in BOTH directions, re-project, and must materialize the same entities,
    /// relations, and type glossary - regardless of which direction applied first.
    #[test]
    fn cross_node_reprojection_converges() {
        use std::collections::BTreeMap as Map;
        use supragnosis_core::{NodeIdentity, VersionVector};
        use supragnosis_sync::{export_delta, SyncNode};

        let mk = |seed: u8| {
            let store = Arc::new(InMemoryStore::new());
            let engine = Engine::new(store.clone(), format!("host-{seed}"), "ws");
            let node = SyncNode::new(NodeIdentity::from_secret_bytes([seed; 32]));
            (store, engine, node)
        };
        let observe = |e: &Engine,
                       content: &str,
                       ents: Vec<(&str, Option<&str>, Option<&str>)>,
                       rels: Vec<(&str, &str, &str)>| {
            e.observe(ObserveInput {
                workspace: None,
                content: content.into(),
                entities: ents
                    .into_iter()
                    .map(|(n, k, d)| EntityInput {
                        name: n.into(),
                        kind: k.map(String::from),
                        description: d.map(String::from),
                    })
                    .collect(),
                relations: rels
                    .into_iter()
                    .map(|(f, k, t)| RelationInput {
                        from: f.into(),
                        kind: k.into(),
                        to: t.into(),
                        description: None,
                        valid_from: None,
                        valid_to: None,
                    })
                    .collect(),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: Vec::new(),
            })
            .expect("observe");
        };

        let (store_a, engine_a, node_a) = mk(1);
        let (store_b, engine_b, node_b) = mk(2);
        // Overlapping knowledge: both mention Driver (different descriptions - LWW must converge to
        // ONE winner by HLC on both sides), plus disjoint facts and a cross-entity relation each.
        observe(
            &engine_a,
            "driver a",
            vec![("Driver", Some("Component"), Some("desc from A"))],
            vec![],
        );
        observe(
            &engine_a,
            "kernel",
            vec![("Kernel", None, None)],
            vec![("Driver", "runs_on", "Kernel")],
        );
        observe(&engine_b, "driver b", vec![("Driver", None, Some("desc from B"))], vec![]);
        observe(
            &engine_b,
            "loader",
            vec![("Loader", None, None)],
            vec![("Loader", "loads", "Driver")],
        );
        engine_a
            .define_type(DefineTypeInput {
                workspace: None,
                defs: vec![TypeDefInput {
                    target: TypeTarget::Entity,
                    name: "Component".into(),
                    description: "a deployable part".into(),
                }],
                source_ref: None,
                on_behalf_of: None,
            })
            .unwrap();

        // Stamp and exchange full logs both ways (share = ws), applying in opposite orders per side.
        let share = vec!["ws".to_string()];
        node_a.backfill(store_a.as_ref(), "ws").unwrap();
        node_b.backfill(store_b.as_ref(), "ws").unwrap();
        let keys: Map<String, String> = [
            (node_a.node_id().to_string(), node_a.public_key_hex()),
            (node_b.node_id().to_string(), node_b.public_key_hex()),
        ]
        .into_iter()
        .collect();
        let delta_a =
            export_delta(store_a.as_ref(), "ws", &VersionVector::default(), &share).unwrap();
        let delta_b =
            export_delta(store_b.as_ref(), "ws", &VersionVector::default(), &share).unwrap();
        let mut vv = VersionVector::default();
        let ra = node_a.apply(store_a.as_ref(), "ws", delta_b, &keys, &mut vv).unwrap();
        let mut vv = VersionVector::default();
        let rb = node_b.apply(store_b.as_ref(), "ws", delta_a, &keys, &mut vv).unwrap();
        assert!(ra.rejected.is_empty() && rb.rejected.is_empty());

        // Re-materialize both sides and compare the projections.
        engine_a.reproject(Some("ws")).unwrap();
        engine_b.reproject(Some("ws")).unwrap();
        let ents = |s: &InMemoryStore| -> Vec<(String, String, Option<String>, String)> {
            let mut v: Vec<_> = s
                .all_entities(Some("ws"))
                .unwrap()
                .into_iter()
                .map(|e| (e.id, e.kind, e.description, e.canonical_name))
                .collect();
            v.sort();
            v
        };
        let rels = |s: &InMemoryStore| -> Vec<(String, String, String, String)> {
            let mut v: Vec<_> = s
                .all_relations(Some("ws"))
                .unwrap()
                .into_iter()
                .map(|r| (r.id, r.from, r.kind, r.to))
                .collect();
            v.sort();
            v
        };
        assert_eq!(ents(&store_a), ents(&store_b), "entity materialization must converge (F5)");
        assert_eq!(rels(&store_a), rels(&store_b), "relation materialization must converge (F5)");
        // Exactly one Driver description won on BOTH sides (HLC LWW, not arrival order).
        let driver_id = Entity::make_id("ws", "Driver");
        let da = store_a.get_entity(&driver_id).unwrap().unwrap().description;
        let db = store_b.get_entity(&driver_id).unwrap().unwrap().description;
        assert_eq!(da, db, "description LWW must pick the same winner everywhere");
        assert!(da.is_some());
        // The type glossary converges too (already a pure HLC fold).
        let ta: Vec<_> = engine_a
            .types(Some("ws"))
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.description))
            .collect();
        let tb: Vec<_> = engine_b
            .types(Some("ws"))
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.description))
            .collect();
        assert_eq!(ta, tb, "type glossary must converge (F5)");
        // The belief projection converges wholesale (resolution.md Section 7): kind winners,
        // contested flags, competitors, and effective tiers are part of the serialized view, so
        // graph equality pins the extended P16 obligation, not just the table shapes.
        let ga = serde_json::to_string(&engine_a.graph(Some("ws")).unwrap()).unwrap();
        let gb = serde_json::to_string(&engine_b.graph(Some("ws")).unwrap()).unwrap();
        assert_eq!(ga, gb, "belief projection (kind/contested/effective tier) must converge");
    }

    /// M4 Phase 1: the type-glossary fold orders by HLC, not observed_at (docs/federation.md Section 4).
    /// Two definitions of the same (target, name): the one with the LATER observed_at but EARLIER sync
    /// HLC must lose - the HLC is the cross-node fold order, and observed_at stays human-facing (P4).
    #[test]
    fn types_fold_orders_by_hlc_not_observed_at() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "test-host", "ws1");
        let def = |desc: &str, observed_at: Timestamp, hlc_wall: u64, seq: u64| {
            let p = Provenance {
                host: "h".into(),
                on_behalf_of: None,
                workspace: "ws1".into(),
                source_ref: None,
                observed_at,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: Some(SyncMeta {
                    origin_node: "node-a".into(),
                    origin_seq: seq,
                    hlc: Hlc { wall: hlc_wall, counter: 0, node: "node-a".into() },
                    signature: "sig".into(),
                    lineage: Vec::new(),
                }),
            };
            let assertions = Assertions {
                type_defs: vec![TypeDefAssertion {
                    target: TypeTarget::Entity,
                    name: "Driver".into(),
                    description: desc.into(),
                }],
                ..Assertions::default()
            };
            Observation::with_assertions(format!("define Driver: {desc}"), p, assertions)
        };
        // Authored first (HLC 100) but with a LATER human-facing observed_at (900)...
        store.add_observation(def("older definition", 900, 100, 1)).unwrap();
        // ...vs authored later (HLC 200) with an earlier observed_at (500): the HLC winner.
        store.add_observation(def("newer definition", 500, 200, 2)).unwrap();

        let types = engine.types(Some("ws1")).unwrap();
        assert_eq!(types.len(), 1);
        assert_eq!(
            types[0].description, "newer definition",
            "last-write-wins must follow the HLC order (I11), not observed_at"
        );
        assert_eq!(types[0].sources, 2);
    }

    /// Proposal gate skeleton (Principle 23, solo): open -> Open, verdict(merge) -> Merged (absorbing),
    /// and an independent reject-only proposal -> Rejected. The state is a deterministic fold (I2/I16).
    #[test]
    fn proposal_open_verdict_fold() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws1");
        // Real entities: the referential-integrity check (Section 6) blocks a merge whose targets are
        // not in the local log, and this test is about the state machine, not about that check.
        engine
            .observe(ObserveInput {
                content: "a and b".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { name: "A".into(), kind: None, description: None },
                    EntityInput { name: "B".into(), kind: None, description: None },
                ],
                relations: vec![],
            })
            .expect("observe");
        let (id_a, id_b) = (Entity::make_id("ws1", "A"), Entity::make_id("ws1", "B"));
        let pid = engine
            .propose(ProposeInput {
                workspace: None,
                kind: "entity_merge".into(),
                targets: vec![id_a.clone(), id_b.clone()],
                into: Some(id_b.clone()),
                tier: None,
                rationale: Some("A and B are the same entity".into()),
                affected_types: vec![],
                source_ref: None,
                on_behalf_of: None,
            })
            .expect("open proposal");
        // Freshly opened -> state open, no verdicts.
        let p = engine.get_proposal(Some("ws1"), &pid).unwrap().expect("proposal exists");
        assert_eq!(p.state, "open");
        assert_eq!(p.kind, "entity_merge");
        assert_eq!(p.into.as_deref(), Some(id_b.as_str()));
        assert_eq!(p.verdicts, 0);

        // A merge verdict is the absorbing outcome.
        engine
            .review_proposal(None, pid.clone(), "merge".into(), None, None, VerdictSurface::Console)
            .expect("cast merge verdict");
        let p = engine.get_proposal(Some("ws1"), &pid).unwrap().unwrap();
        assert_eq!(p.state, "merged");
        assert_eq!(p.verdicts, 1);

        // A second, reject-only proposal folds to rejected.
        let pid2 = engine
            .propose(ProposeInput {
                workspace: None,
                kind: "recall".into(),
                targets: vec!["idC".into()],
                into: None,
                tier: None,
                rationale: None,
                affected_types: vec![],
                source_ref: None,
                on_behalf_of: None,
            })
            .unwrap();
        engine
            .review_proposal(
                None,
                pid2.clone(),
                "reject".into(),
                None,
                None,
                VerdictSurface::Console,
            )
            .unwrap();
        assert_eq!(engine.get_proposal(Some("ws1"), &pid2).unwrap().unwrap().state, "rejected");

        // list_proposals returns both.
        assert_eq!(engine.list_proposals(Some("ws1")).unwrap().len(), 2);

        // A malformed entity_merge (no `into`) is rejected at open (well-formedness only).
        assert!(engine
            .propose(ProposeInput {
                workspace: None,
                kind: "entity_merge".into(),
                targets: vec!["x".into(), "y".into()],
                into: None,
                tier: None,
                rationale: None,
                affected_types: vec![],
                source_ref: None,
                on_behalf_of: None,
            })
            .is_err());
    }

    /// A tbox_change carries its affected T-Box types through the opened payload and the fold, so the
    /// viewer has a structured belief-diff hint to highlight (Principle 23 / M3.5b). Relation names are
    /// normalized (matching the graph's edge kinds); entity names are kept verbatim.
    #[test]
    fn tbox_change_affected_types_round_trip() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws1");
        let pid = engine
            .propose(ProposeInput {
                workspace: None,
                kind: "tbox_change".into(),
                targets: vec!["obs-def".into()],
                into: None,
                tier: None,
                rationale: Some("define relation types from the census".into()),
                affected_types: vec![
                    AffectedType { target: TypeTarget::Relation, name: "dependsOn".into() },
                    AffectedType { target: TypeTarget::Entity, name: "Driver".into() },
                ],
                source_ref: None,
                on_behalf_of: None,
            })
            .expect("open tbox_change");
        let p = engine.get_proposal(Some("ws1"), &pid).unwrap().expect("proposal exists");
        assert_eq!(p.kind, "tbox_change");
        assert_eq!(p.into, None);
        assert_eq!(p.affected_types.len(), 2);
        // Relation names are normalized to the graph's edge-kind form; entity labels stay verbatim.
        assert_eq!(p.affected_types[0].name, "depends_on");
        assert_eq!(p.affected_types[0].target, TypeTarget::Relation);
        assert_eq!(p.affected_types[1].name, "Driver");
        assert_eq!(p.affected_types[1].target, TypeTarget::Entity);

        // An entity_merge carries no affected_types (the field folds to empty).
        let mid = engine
            .propose(ProposeInput {
                workspace: None,
                kind: "entity_merge".into(),
                targets: vec!["idA".into(), "idB".into()],
                into: Some("idB".into()),
                tier: None,
                rationale: None,
                affected_types: vec![],
                source_ref: None,
                on_behalf_of: None,
            })
            .unwrap();
        assert!(engine
            .get_proposal(Some("ws1"), &mid)
            .unwrap()
            .unwrap()
            .affected_types
            .is_empty());
    }

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
                    EntityInput {
                        description: None,
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                    EntityInput {
                        description: None,
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                ],
                relations: vec![RelationInput {
                    description: None,
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
        assert!(!out.hits.is_empty(), "keyword search should find the observation");
        // An engine without an embedder has mode keyword (degrade marker, Principle 16 4th).
        assert_eq!(out.mode, SearchMode::Keyword);

        // Not visible from another workspace.
        assert!(engine.search("rust", Some("other"), 10).unwrap().hits.is_empty());
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
                    relations: vec![RelationInput {
                        description: None,
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
                    entities: vec![EntityInput {
                        description: None,
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
        let ent = |name: &str, kind: Option<&str>| EntityInput {
            description: None,
            name: name.into(),
            kind: kind.map(String::from),
        };
        let rel = |from: &str, kind: &str, to: &str| RelationInput {
            description: None,
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
                    relations: vec![RelationInput {
                        description: None,
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
                    entities: vec![EntityInput {
                        description: None,
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
        let entity = store.get_entity(&Entity::make_id("ws1", "thing")).unwrap().unwrap();

        // The log and the projection carry the same attestation count - the log is the source of truth.
        assert_eq!(logged.provenance.len(), 2, "two attestations preserved in the log");
        assert_eq!(entity.provenance.len(), 2);
        let behalfs: Vec<Option<String>> =
            logged.provenance.iter().map(|p| p.on_behalf_of.clone()).collect();
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
        assert!(keyword_out.hits.is_empty(), "substring keyword search should miss this query");
        assert_eq!(keyword_out.mode, SearchMode::Keyword, "degrade is marked keyword");

        // Hybrid recalls the lexically overlapping rust observation at the top.
        let out = hybrid.search(q, Some("ws"), 10).unwrap();
        assert_eq!(
            out.mode,
            SearchMode::Hybrid,
            "marked hybrid when the semantic surface is referenced"
        );
        let hits = out.hits;
        assert!(!hits.is_empty(), "hybrid search should recall via embedding");
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
                entities: vec![EntityInput {
                    description: None,
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
                    EntityInput {
                        description: None,
                        name: "supragnosis".into(),
                        kind: Some("Project".into()),
                    },
                    EntityInput {
                        description: None,
                        name: "rmcp".into(),
                        kind: Some("Tool".into()),
                    },
                ],
                relations: vec![RelationInput {
                    description: None,
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
                entities: vec![EntityInput {
                    description: None,
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
                    entities: vec![EntityInput {
                        description: None,
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
                    RelationInput {
                        description: None,
                        from: "M".into(),
                        kind: "relates_to".into(),
                        to: "N".into(),
                        valid_from: None,
                        valid_to: None,
                    },
                    RelationInput {
                        description: None,
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
        assert_eq!(
            hg.hyperedges.len(),
            1,
            "P is degenerate, only the relation observation is a hyperedge"
        );
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

    /// The name-variant ladder's normalization keys, tested directly because they define what counts
    /// as "the same spelling" - the one place a change silently widens or narrows every candidate set.
    #[test]
    fn variant_keys_fold_orthography_but_not_meaning() {
        // Separator/case variance collapses - the collision the id key (trim+lowercase) keeps apart.
        for name in ["TrustTier", "Trust Tier", "trust-tier", "trust_tier", " Trust  Tier "] {
            assert_eq!(variant_key(name), "trusttier", "{name}");
        }
        // Non-Latin names survive instead of folding to the empty string (is_alphanumeric, not ASCII).
        assert_eq!(variant_key("신뢰 등급"), "신뢰등급");
        // Distinct concepts stay distinct: the ladder must not reach across a real name difference.
        assert_ne!(variant_key("Observation"), variant_key("Observation Log"));
        assert_ne!(variant_key("Hlc"), variant_key("HLC Ordering"));
        // Plural folding, and the `ss` guard that keeps `class` out of `clas`.
        assert_eq!(plural_fold("ports"), plural_fold("port"));
        assert_eq!(plural_fold("types"), plural_fold("type"));
        assert_eq!(plural_fold("entities"), plural_fold("entity"));
        assert_eq!(plural_fold("class"), "class");
    }

    /// The ladder is the dedup signal that survives WITHOUT an embedder (Principle 19): on a
    /// keyword-only node the conservative merge band returns empty and `duplicates` cannot fire
    /// (it is keyed on the same normalization as the entity id), so this is the only thing standing
    /// between an agent's orthographic slip and two permanent entities for one concept.
    #[test]
    fn name_variant_ladder_catches_orthographic_duplicates_without_an_embedder() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let observe = |content: &str, names: &[&str]| {
            engine
                .observe(ObserveInput {
                    content: content.into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: names
                        .iter()
                        .map(|n| EntityInput { name: (*n).into(), kind: None, description: None })
                        .collect(),
                    relations: vec![],
                })
                .expect("observe");
        };
        // A separator variant, a plural variant, and a control pair that must NOT group.
        observe("enum", &["TrustTier"]);
        observe("concept", &["Trust Tier"]);
        observe("one", &["Port"]);
        observe("many", &["Ports"]);
        observe("distinct", &["Observation", "Observation Log"]);

        let rep = engine.curation(Some("ws1")).unwrap();
        // Baseline: the two signals this ladder exists to replace are both silent here.
        assert!(
            rep.merge_suggestions.is_empty(),
            "no embedder is configured, so the merge band contributes nothing"
        );
        assert!(
            rep.duplicates.is_empty(),
            "the id key already folded case/whitespace, so duplicates cannot fire in one workspace"
        );

        let group_of = |name: &str| {
            rep.name_variants
                .iter()
                .find(|g| g.members.iter().any(|m| m.name == name))
                .unwrap_or_else(|| panic!("{name} should be flagged as a variant"))
        };
        let tt = group_of("TrustTier");
        assert_eq!(tt.rung, VariantRung::Separator);
        assert_eq!(tt.members.len(), 2);
        let port = group_of("Ports");
        assert_eq!(port.rung, VariantRung::Plural, "differs by more than separators");

        // The control pair is untouched - a ladder that grouped these would be folding meaning.
        assert!(
            !rep.name_variants
                .iter()
                .any(|g| g.members.iter().any(|m| m.name == "Observation Log")),
            "'Observation' and 'Observation Log' are two concepts, not two spellings"
        );
        assert_eq!(rep.stats.name_variants, rep.name_variants.len());

        // Deterministic (P16): the same store serializes identically on a recomputation.
        let once =
            serde_json::to_string(&engine.curation(Some("ws1")).unwrap().name_variants).unwrap();
        let twice =
            serde_json::to_string(&engine.curation(Some("ws1")).unwrap().name_variants).unwrap();
        assert_eq!(once, twice);
    }

    /// An empty `merge_suggestions` conflates three states, and only one of them is a negation
    /// (Principle 5): the band did not run at all, it ran over part of the workspace, or it ran
    /// exhaustively and found nothing. `search_knowledge` already refuses this conflation by
    /// labelling its `mode`; the curation report must too, or a reviewer reads "no duplicates"
    /// off a signal that never looked.
    #[test]
    fn merge_band_reports_whether_it_could_run_and_over_how_much() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};
        struct Fixed;
        impl EmbeddingProvider for Fixed {
            fn dimensions(&self) -> usize {
                3
            }
            fn id(&self) -> String {
                "fixed-3".into()
            }
            fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0]).collect())
            }
        }
        let observe = |engine: &Engine, name: &str| {
            engine
                .observe(ObserveInput {
                    content: format!("about {name}"),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: name.into(),
                        kind: None,
                        description: None,
                    }],
                    relations: vec![],
                })
                .expect("observe");
        };

        // (1) No embedder: the signal cannot be computed, and says so rather than returning a bare [].
        let store = Arc::new(InMemoryStore::new());
        let plain = Engine::new(store.clone(), "h", "ws1");
        observe(&plain, "Alpha");
        observe(&plain, "Beta");
        let rep = plain.curation(Some("ws1")).unwrap();
        assert!(rep.merge_suggestions.is_empty());
        assert!(!rep.merge_band.available, "no embedder - not computed");
        assert_eq!((rep.merge_band.embedded, rep.merge_band.examined), (0, 0));

        // (2) Same store, now with an embedder: the rows written above still carry no vector, so the
        // band runs but under-covers. This is the state that used to be indistinguishable from an
        // exhaustive empty result - reachable in practice by configuring an embedder after the fact.
        let embedded_engine = Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(Fixed));
        observe(&embedded_engine, "Gamma");
        let rep = embedded_engine.curation(Some("ws1")).unwrap();
        assert!(rep.merge_band.available, "an embedder is configured");
        assert!(
            rep.merge_band.embedded < rep.merge_band.examined,
            "the pre-embedder rows are uncovered: {:?}/{:?}",
            rep.merge_band.embedded,
            rep.merge_band.examined
        );

        // (3) After a reproject every live entity carries a vector, so coverage is exhaustive and an
        // empty result finally IS a negation over the whole set.
        embedded_engine.reproject(Some("ws1")).unwrap();
        let rep = embedded_engine.curation(Some("ws1")).unwrap();
        assert!(rep.merge_band.available);
        assert_eq!(
            rep.merge_band.embedded, rep.merge_band.examined,
            "reproject covers the rows written before the embedder existed"
        );
        assert!(rep.merge_band.examined > 0, "there are live entities to cover");
    }

    /// A candidate already awaiting a verdict is not a candidate (I18): once an entity_merge is open
    /// for the pair, the ladder must stop offering it, exactly as the merge band does. Without this
    /// the console re-offers a pair that is already in flight and a reviewer files duplicate
    /// proposals for one merge.
    #[test]
    fn name_variants_stop_being_offered_once_a_merge_is_open() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        for (content, name) in [("enum", "TrustTier"), ("concept", "Trust Tier")] {
            engine
                .observe(ObserveInput {
                    content: content.into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: name.into(),
                        kind: None,
                        description: None,
                    }],
                    relations: vec![],
                })
                .expect("observe");
        }
        let before = engine.curation(Some("ws1")).unwrap().name_variants;
        assert_eq!(before.len(), 1, "the separator variant is offered first");
        let members: Vec<String> = before[0].members.iter().map(|m| m.id.clone()).collect();

        engine
            .propose(ProposeInput {
                workspace: Some("ws1".into()),
                kind: "entity_merge".into(),
                targets: members.clone(),
                into: Some(members[1].clone()),
                tier: None,
                rationale: Some("name-variant ladder (separator/case normalization)".into()),
                affected_types: Vec::new(),
                source_ref: None,
                on_behalf_of: None,
            })
            .expect("propose");

        assert!(
            engine.curation(Some("ws1")).unwrap().name_variants.is_empty(),
            "the pair is in flight - re-offering it would invite a duplicate proposal"
        );
    }
}
