//! supragnosis-engine - the service (use-case) layer.
//!
//! Deterministic logic invoked by the MCP tools: observation ingest -> entity resolution -> relation linking -> lookup/search.
//! The store is accessed only through the [`supragnosis_core::KnowledgeStore`] port.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use supragnosis_core::{
    attestation_key, evaluated_tier, hyperedge_id, normalize_relation_kind, observation_content_id,
    ordering_hlc, verdict_grant_ceiling, AssertionStore, Assertions, BeliefCandidate, Clock,
    EmbeddingProvider, Entity, EntityAssertion, Hlc, KnowledgeStore, Observation,
    ProposalEventAssertion, ProposalEventKind, Provenance, Relation, RelationAssertion,
    ResolutionPolicy, SearchHit, SearchHitKind, StoreError, SystemClock, TierWeighted, Timestamp,
    TraverseHit, TypeDefAssertion, VERDICT_SURFACE_AGENT, VERDICT_SURFACE_CONSOLE,
    VERDICT_SURFACE_PREFIX,
};
// Re-export the UI observability port/types - so mcp/viz can use them without depending on core directly.
pub use supragnosis_core::{Event, EventEnvelope, EventSink, TrustTier, TypeTarget};

thread_local! {
    /// The client name of the local MCP session this thread writes for ([`Engine::as_app`]).
    static WRITING_APP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

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

/// The largest observation content `observe` accepts: 1 MiB.
pub const MAX_CONTENT_BYTES: usize = 1 << 20;

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
    /// Hybrid of keyword (convergence surface) + semantic vector (node-local retrieval assistance).
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

/// The (kind, id) of the hits one stage of a search held.
type HitSet = HashSet<(SearchHitKind, String)>;

/// How a viewer search hit was reached (design-system.md Section 8, U15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reached {
    /// The keyword surface matched it: it contains the query's words.
    Words,
    /// Only the semantic surface found it: it is near the query in meaning.
    Meaning,
    /// The graph's enrichment added it as a neighbor of a hit.
    Neighbor,
}

/// A viewer search hit: the search's own hit and how it was reached.
#[derive(Debug, Clone, Serialize)]
pub struct ViewerHit {
    #[serde(flatten)]
    pub hit: SearchHit,
    pub reached: Reached,
}

/// The viewer's search answer: the surface used and the hits, in the search's order.
#[derive(Serialize)]
pub struct ViewerSearch {
    pub mode: SearchMode,
    pub hits: Vec<ViewerHit>,
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

/// Semantic retrieval as `status` and the viewer's health report it (semantic-retrieval.md SR3):
/// which model, and whether it can answer in this process - with the reason and the remedy when
/// it cannot.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct SemanticReport {
    pub model: String,
    pub dims: usize,
    /// `on` or `off`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The backfill of what was written before the model, once it has started in this process
    /// (semantic-retrieval.md Section 12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfill: Option<BackfillProgress>,
}

/// How far the backfill got (semantic-retrieval.md Section 12): the rows it found without a
/// vector, how many it embedded, and whether it is running, finished or stopped - with the reason
/// when it stopped.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct BackfillProgress {
    /// `running`, `done` or `stopped`.
    pub state: &'static str,
    /// Observations and entities that had no vector when the backfill began.
    pub found: usize,
    /// Of those, how many now have one.
    pub embedded: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What [`Engine::repay_owed`] found owed and repaid (crash-recovery.md K3/K5) - kept so the
/// operator's surfaces can say a recovery happened, instead of leaving it in a startup log.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// When the repayment finished, in the node's transaction time (ms).
    pub at: supragnosis_core::Timestamp,
    /// The workspaces re-projected, in name order.
    pub workspaces: Vec<String>,
    /// How many log rows the ledger named as owed.
    pub observations: usize,
}

/// The core ontology's meta-level types (principles.md P10). The core is fixed and the domain
/// vocabulary extends it through `define_type`, so these names are refused there: a domain type
/// called `Observation` would put a second meaning on the word the whole model is built on (P8).
/// Refused at the local door only. A peer's definition is not refused on arrival, since a node
/// that did would diverge from one that had already accepted it (P16).
const CORE_META_TYPES: [&str; 6] =
    ["Observation", "Entity", "Relation", "Provenance", "Host", "Workspace"];

/// What a workspace re-key moved, skipped, and why.
#[derive(Serialize, Debug, Default)]
pub struct RekeyReport {
    /// Knowledge observations re-created under the target workspace.
    pub moved: usize,
    /// Already present there from an earlier run (the lineage says so) - a re-key is idempotent.
    pub already: usize,
    /// Proposal events left behind. Their payloads name SOURCE-workspace entity ids, which do not
    /// exist in the target, so carrying them over would import proposals that are permanently
    /// blocked on referential integrity - the disease, not the cure. A merged demotion among them
    /// still reaches the target, as the tier its row is clamped to; a promotion does not.
    pub skipped_proposal_events: usize,
    /// Moved rows a merged promotion had raised. The promotion stays with its record in the source
    /// workspace, so in the target the row evaluates at its claim until someone promotes it there.
    pub promotions_left_behind: usize,
}

/// What [`Engine::migrate_legacy_ids`] re-created, skipped, and why.
#[derive(Serialize, Debug, Default)]
pub struct MigrateReport {
    /// Rows whose stored id predates the current content-address formula, re-created under the
    /// current id. Already-migrated rows (the lineage says so) are not counted again.
    pub migrated: usize,
    /// Legacy-id rows carrying proposal events, left as they are. An opening row's id is the
    /// proposal's id, and its verdicts and comments name that id in their payloads, so re-keying it
    /// would orphan them and reopen a decided proposal. No release wrote one: proposal events and
    /// the current formula arrived in the same change, so only a hand-built row can be here.
    pub skipped_proposal_events: usize,
    /// Migrated rows a merged promotion had raised. The proposal names the legacy id, which the
    /// live set no longer shows, so the successor evaluates at its claim until promoted again.
    pub promotions_left_behind: usize,
}

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
    /// built unscoped. They are two entities that share a name, never a merge to propose (Principle
    /// 15). Orthographic duplicates inside a workspace are `name_variants` below.
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
    /// entity pairs whose embeddings are near (a node-local retrieval aid, P16-exempt) - entity-merge
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
    /// What the ladder left out because it is already answered (design-system.md Section 29.28), so
    /// an empty `name_variants` says whether no names are alike or all the alike ones are answered.
    pub name_ladder: NameLadderCoverage,
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
    /// The lowest-weighted observations - what a demotion pass would push down, were anything
    /// consuming the weight yet ([consolidation.md](../../../docs/consolidation.md) Section 8 step 1).
    /// Read-only like every other signal here, and in this case doubly so: nothing ranks by it.
    pub demotion_candidates: Vec<RecallWeight>,
    /// Type names defined on BOTH the entity and relation axes (resolution-identity.md Section 6,
    /// Principle 9). Informative, not blocking: an axis collision is legal but usually a mistake -
    /// the one structural T-Box check available before a subtype hierarchy exists (Principle 13).
    pub type_axis_collisions: Vec<String>,
    /// Verdicts filed in a workspace their proposal does not live in, which decide nothing
    /// (proposal-workflow.md I2). An earlier build filed a verdict cast under All in the node
    /// default; this lists each so a person can decide the proposal where it lives
    /// (design-system.md Section 29.15). Computed only for the all-workspaces report on this node:
    /// naming the proposal's workspace answers across workspaces, which no single workspace's
    /// report, and so no remote reader, may do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stray_verdicts: Vec<StrayVerdict>,
    pub stats: CurationStats,
}

/// An entity of another workspace that carries the same name ([`Engine::namesakes`]).
#[derive(Serialize)]
pub struct Namesake {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub workspace: String,
}

/// A verdict filed away from its proposal ([`CurationReport::stray_verdicts`]).
#[derive(Serialize)]
pub struct StrayVerdict {
    /// The verdict's own observation id.
    pub observation: String,
    pub proposal: String,
    /// merge | reject | withdraw.
    pub decision: String,
    /// Where the verdict was filed.
    pub filed_in: String,
    /// Where its proposal lives.
    pub proposal_in: String,
}

/// One observation's retrieval weight ([consolidation.md](../../../docs/consolidation.md) Section 4).
///
/// Computed and reported, consumed by nothing: Section 8 step 1 is deliberately observable before it
/// is load-bearing, so an operator can read what the system would demote before it demotes anything.
/// `fuse_rrf` still fuses by rank position alone and takes no per-item term.
///
/// Every input is a fold over the log (C1/C2/C3). None of them is a wall clock, an arrival order or a
/// node-local signal, which is what lets this converge on the surfaces P16 binds - two nodes holding
/// the same observations compute the same weight.
#[derive(Serialize)]
pub struct RecallWeight {
    pub observation: String,
    /// In `[FLOOR, 1.0]`. Never zero: a weight of zero is deletion by arithmetic, and P7 requires
    /// demoted knowledge to stay reachable by an explicit query (C4).
    pub weight: f32,
    /// The receiver's evaluation, never the claimed tier (P18) - the "retrieval is trust-weighted"
    /// half of P18's unmet clause.
    pub tier: &'static str,
    /// Where this observation falls in the time SPAN the workspace's own log covers: 0.0 at the
    /// oldest, 1.0 at the newest. A span rather than a rank, because a rank invents age differences
    /// that did not happen - a thousand rows written inside one hour would be spread across the
    /// whole range as if the first and the five-hundredth were far apart in age.
    ///
    /// Both ends are recorded HLC fields, so this is arithmetic on data and not a reading of this
    /// machine's clock: P16 forbids consulting the clock, not comparing two timestamps the log
    /// already carries (consolidation.md Section 4.2).
    pub frontier: f32,
    /// Derived from other knowledge and attested by nobody else. The P18 shape that should not
    /// crowd a ranked surface: lineage without corroboration.
    pub derived_alone: bool,
}

/// One conservative-merge-band candidate (resolution-identity.md Section 3): a pair of distinct
/// entities whose name embeddings are near, with the similarity (node-local retrieval score) and how
/// many graph neighbors they share (a corroborating structural signal). Not a proposal - the
/// viewer/agent opens an entity_merge through the gate to act on it (IR2).
#[derive(Serialize)]
pub struct MergeSuggestion {
    pub a: String,
    pub a_name: String,
    pub b: String,
    pub b_name: String,
    /// Embedding cosine similarity (retrieval aid, P16-exempt, node-local) - for ranking only.
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
/// (Principle 5; Principle 16 4th revision, the convergence surface vs the node-local retrieval aid).
/// The other curation signals are deterministic and always run; the name-variant ladder, which also
/// leaves out what is answered, says how much in [`NameLadderCoverage`].
#[derive(Serialize)]
pub struct MergeBandCoverage {
    /// False when no embedder is configured: `merge_suggestions` is empty because the signal could
    /// not be computed at all, NOT because no near-name pairs exist.
    pub available: bool,
    /// Live entities carrying a name vector, out of `examined`. Short of it means the band ran but
    /// under-covered - rows projected before an embedder was configured keep no vector until the
    /// serving process's backfill or a re-projection gives them one (semantic-retrieval.md Section
    /// 12) - so an empty result is still not a negation over the whole set.
    pub embedded: usize,
    /// Live entities the band considered (merged-away rows excluded).
    pub examined: usize,
    /// Near pairs the band reached and left out because they are already answered: a waiting merge
    /// names them, a split pulled them apart, or the person's reject holds them back. Each counts
    /// once (design-system.md Section 29.27). Not "withheld", which P5 keeps for an ungranted scope.
    pub held_back: usize,
}

/// What the name-variant ladder held back (design-system.md Section 29.28). The ladder needs no
/// embedder, so it always runs and there is no `available` to report - only what it left out.
#[derive(Serialize, Default)]
pub struct NameLadderCoverage {
    /// Pairs the ladder reached and lists in no group because they are already answered: a waiting
    /// merge names them, a split pulled them apart, or the person's reject holds them back. A set -
    /// a pair reached at several rungs counts once, and a pair a listed group shows does not count.
    /// Separate from [`MergeBandCoverage::held_back`]: a pair both near and alike is in each.
    pub held_back: usize,
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
    /// For a type definition (`field` "definition"): the axis it is defined on. A definition ties
    /// as a kind does (IR5), so it waits under Needs you too (design-system.md Section 29.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<TypeTarget>,
    /// For a type definition: the workspace it belongs to, part of the type's identity (P11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
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

/// The six canon-affecting proposal kinds (Principle 23 / proposal-workflow.md 3.3): five intents,
/// with entity identity's two directions as two kinds.
pub const PROPOSAL_KINDS: [&str; 6] = [
    "entity_merge",
    "entity_split",
    "claim_promotion",
    "claim_demotion",
    "tbox_change",
    "recall",
];

/// Whether a merge of this kind changes anything. A type change and a recall have no commit effect
/// yet (architecture.md Section 14), so no merge of either is valid until each has one: the fold
/// reads it as a comment and the write surfaces refuse it (proposal-workflow.md 7.1 (f),
/// design-system.md Section 29.22). The one place every caller asks.
pub fn has_commit_effect(kind: &str) -> bool {
    !matches!(kind, "tbox_change" | "recall")
}

/// What opening a proposal did (design-system.md Section 29.23): its id, its state after the act,
/// and whether that id was already in the log before it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    pub id: String,
    pub state: String,
    pub found: bool,
}

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

/// Whether a verdict carries the engine-stamped console marker on any of its attestations - the
/// human's direct act of I17. Read off every attestation, as the gate ceiling is: one decision cast
/// twice is one row with two attestations (P3), and a person's console act on it is still theirs.
fn console_cast(obs: &Observation) -> bool {
    obs.provenance
        .iter()
        .any(|p| p.source_ref.as_deref() == Some(VERDICT_SURFACE_CONSOLE))
}

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
        reach: Vec::new(),
    }
}

/// A merge verdict was cast on this proposal, whether or not the blocking checks let it commit.
/// `merged` and `blocked` carry one, and so does a blocked merge a person rejected, which reads
/// `rejected` (proposal-workflow.md Section 7.1); an `open`, `withdrawn` or plainly rejected one does
/// not.
///
/// This is the condition an `entity_split`'s target must satisfy, and deliberately NOT "is merged".
/// The check runs inside the fold that decides merged-versus-blocked, so depending on that
/// distinction would make one proposal's state depend on another's within a single pass. Reversing
/// a merge that the checks are holding back is harmless anyway - it subtracts an edge that is not
/// being contributed (unmerge.md Section 9).
fn carries_merge_verdict(view: &ProposalView) -> bool {
    view.merge_cast
}

/// The two gate kinds with a tier commit effect (resolution.md Section 5). Their targets are
/// observation ids, which the remote surface checks against the proposal's own workspace.
pub const GATE_KINDS: [&str; 2] = ["claim_promotion", "claim_demotion"];

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
    /// Group entity name; defaults to `"context: <first member names>"`.
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
    /// For the gate kinds: item 1, the assertions a claim promotion moves together (an observation is
    /// one bundle - Section 3, kind 1), or for a demotion what leaves. One row per target whose tier
    /// changes. The console reads it before a settle so a person sees a source's whole reach before
    /// confirming one value from it (design-system.md Section 29.2). Empty for every other kind.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reach: Vec<ReachRow>,
}

/// One source a gate event moves, and what moves with it (design-system.md Section 29.2).
#[derive(Serialize, Debug)]
pub struct ReachRow {
    pub observation: String,
    /// What the observation said: its first [`REACH_SAID_CHARS`] characters.
    pub said: String,
    /// True when `said` is cut short - a cut answer says so (Principle 5).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// The observation's own effective tier, before and after.
    pub from: TrustTier,
    pub to: TrustTier,
    /// Every entity the observation supports - asserted, or named at a relation's end - with its ring
    /// before and after: the tier of its best support (U2), which another source may hold where it
    /// is, so `from == to` is a fact the console reports rather than an omission.
    pub entities: Vec<EntityReach>,
    /// The relations the observation states, as written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<StatedRelation>,
    /// The type definitions it carries - a define_type observation is one bundle too.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub type_defs: Vec<String>,
}

/// An entity a moved source supports, with its ring before and after.
#[derive(Serialize, Debug)]
pub struct EntityReach {
    pub entity: String,
    pub name: String,
    pub from: TrustTier,
    pub to: TrustTier,
}

/// A relation as a moved source states it.
#[derive(Serialize, Debug)]
pub struct StatedRelation {
    pub from: String,
    pub kind: String,
    pub to: String,
}

/// How much of a moved source a reach row quotes.
pub const REACH_SAID_CHARS: usize = 280;

/// How much of a source a label quotes (design-system.md Section 29.3).
pub const LABEL_SAID_CHARS: usize = 80;

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

/// One act the console would cast through `/api/resolve`: a gate kind, its targets and the tier it
/// asks for (design-system.md Section 29.2).
#[derive(Debug, Clone)]
pub struct GateAct {
    pub kind: String,
    pub targets: Vec<String>,
    pub tier: TrustTier,
}

/// What the console's settle would do, computed and not cast: the acts' belief diff, applied in
/// order after every merged event, and each act's blocking checks (P23, U4).
#[derive(Serialize)]
pub struct GatePreview {
    pub belief_diff: BeliefDiff,
    pub checks: Vec<CheckResult>,
}

/// What reverses one merged gate proposal ([`Engine::undo_gate`]): one act per target it still
/// moves, and their diff and checks. No acts, and no diff, when it moves nothing any more.
#[derive(Serialize)]
pub struct GateUndo {
    pub proposal: String,
    pub acts: Vec<UndoAct>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub belief_diff: Option<BeliefDiff>,
    pub checks: Vec<CheckResult>,
}

/// One reversing act: the observation, the gate kind `/api/resolve` casts and the tier it returns to.
#[derive(Serialize)]
pub struct UndoAct {
    pub observation: String,
    pub kind: String,
    pub tier: TrustTier,
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
    /// For a merged claim_promotion: the tier it granted - the one asked for, capped by the highest
    /// surface among its merge verdicts (resolution.md Section 6, design-system.md Section 29.20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granted_tier: Option<TrustTier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// For tbox_change: the T-Box types this proposal defines/changes - the viewer's highlight hint.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub affected_types: Vec<AffectedType>,
    /// The workspace the proposal lives in: the one its opening observation was written to, where its
    /// verdicts count (proposal-workflow.md I2). An all-workspaces list names it on every row.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub workspace: String,
    /// Its kind has no commit effect yet, so it cannot be merged until it has one ([`has_commit_effect`],
    /// design-system.md Section 29.22). Said only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_effect_yet: bool,
    /// open | blocked | merged | rejected | withdrawn (merge is the absorbing state, I16).
    pub state: String,
    pub verdicts: usize,
    pub opened_at: Timestamp,
    pub proposer: String,
    /// Solo single-user marker (Principle 23 self-approval exception): proposer == reviewer.
    pub self_attested: bool,
    /// How each target is said to a reader, in `targets`' order (design-system.md Section 29.3): an
    /// entity by its name, merged-away ones included; a gate act's observation by its first words
    /// and the entities it names; an un-merge's merge by what it merged. `None` where nothing on this
    /// node can say it. Attached by `list_proposals` and `get_proposal`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<Option<String>>,
    /// The canonical target of an entity merge, said as `labels` says a target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub into_label: Option<String>,
    /// A merge verdict was cast on it, whether or not it committed: what an un-merge's target must
    /// carry ([`carries_merge_verdict`]). Not the state, since a blocked merge a person rejected
    /// reads Rejected and still carries its merge verdict, which the Review page says.
    #[serde(rename = "merge_verdict", skip_serializing_if = "std::ops::Not::not")]
    pub merge_cast: bool,
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
    /// The key-derived node ids that signed for this entity (sorted), from the attestations' sync
    /// stamps - who attested it, as opposed to `origins`, which is what they called themselves.
    ///
    /// Two fields rather than one because the two answer different questions and only one of them
    /// can be checked. `host` is a string the writer chose, and P2 is explicit that a self-declared
    /// identity cannot be verified; `origin_node` is bound to a keypair (F14) and is what a
    /// signature verifies against (F6). Anything that attributes knowledge to a peer has to rest on
    /// the second, with the first as a label - collapsing them would promote an unverifiable claim
    /// into structure, which is the mistake F13 exists to prevent on the trust axis.
    ///
    /// Unions across nodes like `origins` does (F4), so this is a set and not an owner.
    ///
    /// Empty has **two** readings and neither is "origin unknown" (P5). Either no attestation is
    /// stamped - stamping happens at the export boundary, so a workspace never offered to a peer has
    /// none - or the log is stamped and this projection has not caught up. The second is real and
    /// was measured: a sync round stamps via `backfill` before it advertises, so a round that fails
    /// at the door leaves the log stamped and the entity rows holding their pre-stamp provenance
    /// until the next re-materialization (F5's transient window, Prop C). `reproject` closes it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origin_nodes: Vec<String>,
    /// The workspace this entity belongs to, or empty when it does not sit in exactly one.
    ///
    /// `Entity::make_id` hashes the workspace with the canonical name, so the same name in two
    /// workspaces is two entities and an *observed* entity never straddles them. That is what lets
    /// the workspace be drawn as a boundary where `origins` cannot: a host set unions across nodes
    /// (F4), and a workspace does not.
    ///
    /// **One case broke it until design-system.md step 51a.** An `entity_merge` folds several ids into
    /// one canonical id, and nothing compared workspaces, so a merge across two left one entity whose
    /// attestations named both, merged in the all-workspaces view alone. A merge now joins entities of
    /// one workspace (Principle 15) and All is the union of each workspace's fold, so the case no
    /// longer arises. The field still reports **empty** rather than picking one if a row ever names two
    /// workspaces: an arbitrary pick would be a claim the log does not make, and the boundary layer
    /// draws nothing for a node with no workspace.
    ///
    /// Read from the attestations rather than stored on the entity: it is already determined by the
    /// id they projected into, and a model field would be a second copy of what the identity fixes
    /// (P14).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace: String,
    /// The node's representative trust: the highest EFFECTIVE tier over its supporting observations
    /// (receiver-evaluated + gate grants - resolution.md Section 3; never a max over claimed tiers,
    /// which would let a remote self-declaration raise the displayed tier - F13).
    /// When the entity was last said: the latest `observed_at` among the attestations its rows hold,
    /// its merged-away rows' included, in epoch milliseconds (design-system.md Section 29.19). Signed
    /// with each attestation and kept by every stamp upgrade, so equal logs give the same time (P16);
    /// the recording axis of P4, never the valid one. `None` when no attestation supports the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_said: Option<Timestamp>,
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
    /// How many attestations assert this edge, as a node's `sources` counts its own. A row written
    /// before relations accumulated counts 1 until it is re-materialized.
    pub sources: usize,
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
    /// A row a writer is about to commit, read as if the log held it (crash-recovery.md Section
    /// 3a): it stands in for the stored row of its id, or joins the log. Every fold of the read sees
    /// it - the entity fold, the gate grants that fold applies, the blocking checks those consult -
    /// so none of them reads a log the commit will not leave. None for every other read.
    overlay: Option<Observation>,
}

impl ReadCtx {
    /// A read of the log as it will be once `row` - already absorbed onto any stored row of its id -
    /// is committed.
    fn over(row: Observation) -> Self {
        Self { overlay: Some(row), ..Self::default() }
    }

    /// The row of `id` this read holds: the overlay when it is that row, else the store's.
    fn held(
        &self,
        store: &dyn KnowledgeStore,
        id: &str,
    ) -> Result<Option<Observation>, StoreError> {
        match self.overlay.as_ref().filter(|o| o.id == id) {
            Some(row) => Ok(Some(row.clone())),
            None => store.get_observation(id),
        }
    }
}

/// The result of [`Engine::belief_fold`]: per canonical entity id, the kind candidates and the
/// representative effective tier over supporting observations (resolution.md Sections 2-4).
struct BeliefFold {
    kinds: HashMap<String, Vec<BeliefCandidate>>,
    tiers: HashMap<String, TrustTier>,
}

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

/// `obs` re-created under `workspace` for a re-key or a migration: every attestation carried, the
/// sync stamp dropped (it signs the old content id and cannot follow), and the lineage naming
/// `obs`. The tier is clamped BEFORE the stamp drops: `evaluated_tier` trusts a stamp-less claim at
/// face value, so a synced claim carried verbatim would evaluate above HostSigned afterwards - a
/// tier promotion by an operator act (P18: the tier is the receiver's evaluation, and re-keying
/// must not raise it). `granted` lowers it further when a merged demotion applied to `obs`, since
/// the gate events do not follow the row. `None` when `obs` has no attestation (P2 forbids it).
/// One attestation as [`recreated`] re-creates it: in the new workspace, at the tier it evaluates to
/// (clamped by a demotion that applied to it), unstamped.
fn recreated_attestation(
    p: &Provenance,
    workspace: &str,
    granted: Option<TrustTier>,
) -> Provenance {
    let mut p = p.clone();
    p.workspace = workspace.to_string();
    p.trust_tier = evaluated_tier(&p);
    if let Some(g) = granted {
        p.trust_tier = p.trust_tier.min(g);
    }
    p.sync = None;
    p
}

fn recreated(
    obs: &Observation,
    workspace: &str,
    granted: Option<TrustTier>,
) -> Option<Observation> {
    let mut provs: Vec<Provenance> = obs
        .provenance
        .iter()
        .map(|p| recreated_attestation(p, workspace, granted))
        .collect();
    if provs.is_empty() {
        return None;
    }
    let first = provs.remove(0);
    let mut fresh =
        Observation::with_assertions(obs.content.clone(), first, obs.assertions.clone());
    for p in provs {
        let mut copy = Observation::with_assertions(obs.content.clone(), p, obs.assertions.clone());
        copy.derived_from = Vec::new();
        fresh.absorb(copy); // union semantics - the attestation set is preserved whole
    }
    fresh.derived_from = obs.derived_from.clone();
    fresh.derived_from.push(obs.id.clone());
    fresh.derived_from.sort();
    fresh.derived_from.dedup();
    Some(fresh)
}

/// Whether a merged promotion raised `obs` above its own evaluation - a grant [`recreated`] does
/// not carry, so the operator act that re-created the row has to say it was left behind.
fn promotion_left_behind(obs: &Observation, granted: Option<TrustTier>) -> bool {
    let own = obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default();
    granted.is_some_and(|g| g > own)
}

/// An observation's EFFECTIVE tier (resolution.md Section 3): the tier its merged gate events leave it
/// at, if any target it - applied in verdict order from its base, a demotion only lowering and a
/// promotion only raising (R5, [`Engine::gate_grants`]); otherwise the max receiver-evaluated tier
/// over its attestations ([`evaluated_tier`] - a wire claim never evaluates above HostSigned). Never
/// a max over claimed tiers (F13).
fn effective_tier(obs: &Observation, gates: &HashMap<String, TrustTier>) -> TrustTier {
    gates
        .get(&obs.id)
        .copied()
        .unwrap_or_else(|| obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default())
}

/// The authoring attestation of an observation: earliest effective HLC, index-tiebroken. After an
/// absorb the provenance vec is union-sorted (by host first), so `first()` names whichever host
/// sorts first, and any max-over-attestations moves as attestations accumulate - neither names the
/// author. This is the single rule for "who authored this act": relation provenance (reproject)
/// and proposal attribution (fold_proposals) must not each pick a different attestation off one
/// row. The surface marker is not an authorship question - a gate grant's ceiling and the I17
/// recall check read every attestation of a merge verdict (resolution.md Section 6).
fn authoring_attestation(obs: &Observation) -> Option<&Provenance> {
    authoring_index(obs).map(|i| &obs.provenance[i])
}

/// [`authoring_attestation`]'s position in the observation's provenance list - what a read that
/// lists every attestation names as the one that wrote it (design-system.md Section 29.7).
fn authoring_index(obs: &Observation) -> Option<usize> {
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
        .map(|(i, _)| i)
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
    /// The AI app that wrote it, as this node recorded it, by the name a person reads - only on the
    /// local viewer's reads (design-system.md Section 29.8). A node-local field: no fold, check or
    /// projection reads it, and the read tier and MCP never ask for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
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
/// (Principle 21 - they are an internal retrieval aid, not part of the legible surface).
#[derive(Serialize)]
pub struct ObsSummary {
    pub id: String,
    /// The workspace the observation was written to - what a log read across every workspace needs
    /// to say where an entry landed (docs/onboarding.md Section 4.4).
    pub workspace: String,
    pub content: String,
    /// The deterministic fold-ordering key ([`ordering_hlc`]) - also the log-feed order.
    pub hlc: Hlc,
    /// The observation's effective tier (gate grants applied - resolution.md Section 3).
    pub effective_tier: TrustTier,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
    pub attestations: Vec<AttestationSummary>,
    /// Which of `attestations` wrote it - [`authoring_attestation`], not the first by host, which is
    /// how the union sorts them (design-system.md Section 29.7).
    pub author: usize,
    /// The gate event that last moved its effective tier, when one did (design-system.md Section
    /// 29.7): what the card says confirmed or lowered it, and when.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gated_by: Option<GateMove>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationRef>,
    /// Present when this observation IS a proposal event - the readable form of `content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proposal: Option<ProposalEventSummary>,
}

/// How many of the store's most used types take the type band's own slots (design-system.md
/// Section 29.9): the band holds sixteen colors.
pub const COMMON_TYPES: usize = 16;

/// The gate event that last moved an observation's effective tier, in the gate fold's verdict order
/// (resolution.md R5): its proposal, whether it raised or lowered the tier, and its representative
/// merge verdict - the verdict's id, time, surface marker and sync origin, so a reader can say who
/// cast it and where (design-system.md Section 29.7).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct GateMove {
    pub proposal: String,
    pub promotion: bool,
    pub verdict: String,
    pub at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_node: Option<String>,
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
    /// The tier the representative observation has without any person's verdict: the max
    /// receiver-evaluated tier over its attestations, before gate events. What "Confirm instead"
    /// demotes a confirmed value back to (design-system.md Section 7).
    pub base_tier: TrustTier,
    pub hlc: Hlc,
    pub observation: String,
    /// Every observation that asserts this value - the sources a person may choose to confirm
    /// (design-system.md Section 29.2) - ordered by the rule that picks `observation`: highest
    /// effective tier, then smallest id, so the representative comes first.
    pub observations: Vec<CandidateSource>,
}

/// One observation behind a candidate value: its effective tier, and its tier before any person's
/// verdict, which a "Confirm instead" demotes it to.
#[derive(Serialize, Debug, Clone)]
pub struct CandidateSource {
    pub observation: String,
    pub trust_tier: TrustTier,
    pub base_tier: TrustTier,
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
    /// The projection write serialization lock. Every section that reads the log and then writes projected
    /// rows from what it read - observe, define_type, the gate acts, rekey, migrate, the embedding
    /// backfill's write, and reproject - holds it, so none can overwrite another's rows with an older
    /// snapshot (architecture.md Section 14). Reads (get/search/traverse/graph) stay outside the lock and
    /// concurrent. A sync apply appends to the log without it; its rows are projected by the reproject that
    /// follows, which takes it.
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
    /// The last repayment of owed projections this engine ran, if any (crash-recovery.md K5).
    last_recovery: std::sync::Mutex<Option<Recovery>>,
    /// The backfill's progress, once one has started in this process (semantic-retrieval.md
    /// Section 12).
    backfill: std::sync::Mutex<Option<BackfillProgress>>,
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
            last_recovery: std::sync::Mutex::new(None),
            backfill: std::sync::Mutex::new(None),
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
        self.emit_from(None, event);
    }

    /// [`Engine::emit`], naming the app of the session the activity came from - a local MCP
    /// session's, never a remote principal's (design-system.md Section 29.8).
    pub fn emit_from(&self, app: Option<&str>, event: Event) {
        if let Some(sink) = &self.events {
            sink.emit(&EventEnvelope {
                session: self.session.clone(),
                // By the name a person reads (the core's table), as an attestation's app is said.
                app: app.map(supragnosis_core::client_said),
                event,
            });
        }
    }

    /// Runs `f` with what it writes attributed to `app`, the client name of the local MCP session
    /// that asked (design-system.md Section 29.8): each attestation `f` adds records it, in this
    /// node's own table. `None` attributes nothing. The attribution is the calling thread's, as the
    /// write is.
    pub fn as_app<T>(&self, app: Option<&str>, f: impl FnOnce() -> T) -> T {
        let was = WRITING_APP.with(|a| a.replace(app.and_then(supragnosis_core::client_recorded)));
        let out = f();
        WRITING_APP.with(|a| a.replace(was));
        out
    }

    /// Carries the apps recorded for `obs`'s attestations to the ones [`recreated`] made of them under
    /// `fresh_id` (design-system.md Section 29.8): re-keying a workspace and migrating legacy ids
    /// keep which app wrote what. A row already there keeps its name, so a re-run is idempotent.
    fn carry_apps(
        &self,
        obs: &Observation,
        fresh_id: &str,
        workspace: &str,
        granted: Option<TrustTier>,
    ) -> Result<(), StoreError> {
        let pairs: Vec<(String, String)> = obs
            .provenance
            .iter()
            .map(|p| {
                (
                    attestation_key(&obs.id, p),
                    attestation_key(fresh_id, &recreated_attestation(p, workspace, granted)),
                )
            })
            .collect();
        let found = self
            .store
            .attestation_apps(&pairs.iter().map(|(o, _)| o.clone()).collect::<Vec<_>>())?;
        for (old, new) in pairs {
            if let Some(app) = found.get(&old) {
                self.store.record_attestation_app(&new, app)?;
            }
        }
        Ok(())
    }

    /// Records the app this thread writes for, under the attestation's key. Recording never fails
    /// the write it belongs to (P24): a failure is logged and the write stands.
    fn note_app(&self, key: Option<&str>) {
        let (Some(key), Some(app)) = (key, WRITING_APP.with(|a| a.borrow().clone())) else {
            return;
        };
        if let Err(e) = self.store.record_attestation_app(key, &app) {
            tracing::warn!(error = %e, "the app that wrote an attestation was not recorded");
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
        // A bound every event fits inside (sync-correctness.md Section 11): without it one
        // observation could exceed any sync body limit on its own, and a peer's stream would be held
        // forever behind an event that can never be delivered. Applied at local ingest only - an
        // event already signed elsewhere is not refused for its size.
        if input.content.len() > MAX_CONTENT_BYTES {
            return Err(ObserveError::Invalid(format!(
                "content is {} bytes; an observation holds at most {MAX_CONTENT_BYTES} (1 MiB). \
                 Split it into observations that each say one thing, or keep the large text \
                 elsewhere and observe what it says with a source_ref pointing at it",
                input.content.len()
            )));
        }
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
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));

        // Resolution write path (resolution-identity.md Section 4): (re)project exactly the entities
        // the observation touches - the same fold reproject runs, so the incremental row equals a
        // fresh replay's (IR3). Relations project the same way (their id is deterministic and
        // description/valid-interval are last-write, matching reproject).
        let entity_ids: Vec<String> =
            input.entities.iter().map(|e| Entity::make_id(&workspace, &e.name)).collect();
        let mut touched: HashSet<String> = entity_ids.iter().cloned().collect();
        for r in &input.relations {
            touched.insert(Entity::make_id(&workspace, &r.from));
            touched.insert(Entity::make_id(&workspace, &r.to));
        }
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

        // The row and its projection commit in one transaction (crash-recovery.md Section 3a), so
        // the projection is folded from the log as it will read with the row in it: the stored row
        // of this id, absorbed with the new attestation, or the new row when there is none. The read
        // context carries it to every fold, the gate grants included - a merged promotion of this
        // very row, synced in before it, applies once the row is here, as a replay applies it.
        let row = match self.store.get_observation(&obs.id)? {
            Some(mut stored) => {
                stored.absorb(obs.clone());
                stored
            }
            None => obs.clone(),
        };
        let cx = &ReadCtx::over(row);
        let entities =
            self.materialize_entities(&workspace, Some(&touched), cx, &HashMap::new())?;
        let edges = self.relation_rows(&workspace, Some(&touched_edges), cx.overlay.as_ref())?;
        // Nothing is owed: the row lands with its projection or not at all (K1), and no entry the
        // ledger already holds for this id - a sync apply of the same content - is cleared (K2).
        self.store.commit_projected(obs, entities, edges)?;
        self.note_app(app_key.as_deref());
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Ok(ObserveOutput { observation_id, entities: entity_ids, relations })
    }

    /// Records T-Box type definitions as an observation (Principle 8/11: an explicit define_type act,
    /// scoped to the workspace). It rides the observation log like any other assertion (Principle 1/23:
    /// no parallel provenance/storage), so the glossary is a deterministic projection
    /// ([`Engine::types`]) and a future proposal gate (Principle 23) can wrap this without rework.
    /// Principles 8 and 10 are enforced here: a definition with no name, an empty description, or a
    /// core meta-type's name is rejected before it reaches the permanent log.
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
            // Principle 10: the core ontology is fixed, so its meta-level names are not domain types.
            if let Some(core) =
                CORE_META_TYPES.iter().find(|c| c.eq_ignore_ascii_case(d.name.trim()))
            {
                return Err(ObserveError::Invalid(format!(
                    "type '{}' is the core ontology's {core}, and the core is fixed (Principle 10) - \
                     name the domain concept you mean instead, such as 'MeetingNote' or 'Service'",
                    d.name.trim()
                )));
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
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));
        // The glossary is a read-time fold; nothing materialized depends on this row, so it commits
        // whole with no projection (crash-recovery.md Section 3a).
        self.store.commit_projected(obs, Vec::new(), Vec::new())?;
        self.note_app(app_key.as_deref());
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
    fn stray_verdicts(&self, cx: &ReadCtx) -> Result<Vec<StrayVerdict>, StoreError> {
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
        // A verdict is filed in its proposal's workspace, the only place it counts (I2): one filed
        // anywhere else would decide nothing and stay in the log for good. So a verdict that names no
        // proposal of this workspace is refused here, with nothing said about any other workspace -
        // on the remote surface this one is the principal's (remote-viewer.md Section 3.4). A
        // replicated verdict never passes through here, so arrival order is still the fold's to
        // tolerate.
        let views = self.fold_proposals(Some(&workspace), cx)?;
        let Some(view) = views.get(&proposal) else {
            return Err(ObserveError::Invalid(format!(
                "no proposal {proposal} in workspace '{workspace}' - a verdict is filed in its \
                 proposal's own workspace, which list_proposals names"
            )));
        };
        // Fail fast on a local merge attempt that the fold would refuse anyway. This is a courtesy,
        // not the gate: the fold is what enforces (a replicated verdict never reaches this path), so
        // this exists to say WHY rather than to let the caller discover a "blocked" state later.
        if decision == "merge" {
            // I17: a recall verdict is a human's direct act. The fold would not count a recall
            // merge cast from the agent surface (it reads the marker off the log), so refuse here
            // and say where to decide - the same courtesy as the blocking checks below.
            if view.kind == "recall" && surface == VerdictSurface::Agent {
                return Err(ObserveError::Invalid(
                    "a recall verdict is a human's direct act (Principle 23, I17): this surface \
                     cannot merge one, and the fold would not count it. Until recall has its \
                     commit effect no surface merges one; a `comment` is accepted here, and the \
                     person can reject it in the Review panel of the Supragnosis app or viewer"
                        .into(),
                ));
            }
            // 7.1 (f): the fold would read this merge as a comment, so say why instead of taking it.
            if !has_commit_effect(&view.kind) {
                return Err(ObserveError::Invalid(format!(
                    "{} has no commit effect yet, so it cannot be merged until it has one - a merge \
                     without a diff is a merge without review (Principle 23). A comment or a \
                     withdrawal is accepted, and the person can reject it in the Review panel",
                    view.kind
                )));
            }
            let asserted = self.asserted_entity_ids(Some(&workspace), cx)?;
            let decided: HashSet<String> = views
                .values()
                .filter(|p| p.kind == "entity_merge" && carries_merge_verdict(p))
                .map(|p| p.id.clone())
                .collect();
            let failures = self.blocking_failures(view, &asserted, &decided, cx)?;
            if !failures.is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "blocking checks fail, so this merge would not reach canon: {}. Fix the proposal or reject it (proposal-workflow.md Section 6)",
                    failures.join("; ")
                )));
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
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));
        // Proposal state is a read-time fold over the log; no materialized row waits on it, so the
        // row commits whole with no projection (crash-recovery.md Section 3a).
        self.store.commit_projected(obs, Vec::new(), Vec::new())?;
        self.note_app(app_key.as_deref());
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
            return Ok(std::borrow::Cow::Owned(
                self.observations_over(workspace, cx.overlay.as_ref())?,
            ));
        }
        let loaded = self.observations_over(workspace, cx.overlay.as_ref())?;
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
        self.observations_over(workspace, None)
    }

    /// [`Engine::observations`] as it will read once `overlay` is in the log: the row a writer is
    /// about to commit stands in for the stored row of its id, or joins the log when there is none
    /// (crash-recovery.md Section 3a). The overlay is the row as the store will hold it - already
    /// absorbed onto the stored one - so the fold reads exactly the log the commit leaves.
    fn observations_over(
        &self,
        workspace: Option<&str>,
        overlay: Option<&Observation>,
    ) -> Result<Vec<Observation>, StoreError> {
        let mut rows = self.store.all_observations(workspace)?;
        if let Some(row) = overlay.filter(|o| workspace.is_none_or(|ws| o.workspace() == ws)) {
            match rows.iter_mut().find(|o| o.id == row.id) {
                Some(stored) => *stored = row.clone(),
                None => rows.push(row.clone()),
            }
        }
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
    /// the proposal's own workspace - the workspace its opening observation was written to - so a
    /// reader granted that workspace on the read tier is told nothing from another (remote-viewer.md
    /// Section 3.4); a target elsewhere keeps `None`.
    fn labels_for(
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

    /// What the console's settle would do, without casting it (design-system.md Section 29.2): the
    /// belief diff of `acts` applied in order after every merged gate event, at the console's grant -
    /// the surface `/api/resolve` casts from - and each act's blocking checks. Writes nothing. A
    /// "Confirm instead" is two acts (a demotion and a promotion), previewed together so the ask
    /// reads their one result (P23: no merge without a diff that says what it does).
    pub fn preview_gate(
        &self,
        workspace: Option<&str>,
        acts: &[GateAct],
    ) -> Result<GatePreview, StoreError> {
        let cx = &ReadCtx::default();
        let ceiling = verdict_grant_ceiling(Some(VERDICT_SURFACE_CONSOLE));
        let events: Vec<(&[String], TrustTier, bool)> = acts
            .iter()
            .filter(|a| GATE_KINDS.contains(&a.kind.as_str()))
            .map(|a| {
                let promotion = a.kind == "claim_promotion";
                (
                    a.targets.as_slice(),
                    if promotion { a.tier.min(ceiling) } else { a.tier },
                    promotion,
                )
            })
            .collect();
        let belief_diff = if events.len() == acts.len() && !acts.is_empty() {
            self.gate_diff(workspace, &events, cx)?
        } else {
            uncomputable_diff("the console previews claim_promotion and claim_demotion only")
        };
        let asserted = self.asserted_entity_ids(workspace, cx)?;
        let decided = self.decided_merges(workspace, cx)?;
        let mut checks: Vec<CheckResult> = Vec::new();
        for act in acts {
            let view = ProposalView {
                id: String::new(),
                no_effect_yet: !has_commit_effect(&act.kind),
                kind: act.kind.clone(),
                targets: act.targets.clone(),
                into: None,
                tier: Some(act.tier),
                rationale: None,
                affected_types: Vec::new(),
                granted_tier: None,
                workspace: workspace.unwrap_or_default().to_string(),
                state: "open".into(),
                verdicts: 0,
                opened_at: 0,
                proposer: String::new(),
                self_attested: false,
                labels: Vec::new(),
                into_label: None,
                merge_cast: false,
                belief_diff: None,
                checks: Vec::new(),
            };
            for c in self.blocking_checks(&view, &asserted, &decided, cx)? {
                if !checks
                    .iter()
                    .any(|k| k.name == c.name && k.passed == c.passed && k.detail == c.detail)
                {
                    checks.push(c);
                }
            }
        }
        Ok(GatePreview { belief_diff, checks })
    }

    /// What reverses a merged claim promotion or demotion (design-system.md Section 29.3): for each
    /// target the proposal still moves, the act that returns it to the tier it has with this
    /// proposal's verdict left out of the gate fold, and those acts' diff and checks at the console's
    /// grant, as [`Engine::preview_gate`] answers them. No acts when the proposal moves no target any
    /// more - another verdict holds the same tier. `None` when no merged gate proposal has that id.
    /// Writes nothing.
    pub fn undo_gate(
        &self,
        workspace: Option<&str>,
        proposal: &str,
    ) -> Result<Option<GateUndo>, StoreError> {
        let cx = &ReadCtx::default();
        let views = self.fold_proposals(workspace, cx)?;
        let Some(view) = views.get(proposal) else {
            return Ok(None);
        };
        if view.state != "merged" || !GATE_KINDS.contains(&view.kind.as_str()) {
            return Ok(None);
        }
        let now = self.gate_tiers(workspace, cx, &[], None)?;
        let before = self.gate_tiers(workspace, cx, &[], Some(proposal))?;
        let mut acts: Vec<GateAct> = Vec::new();
        for obs in self.log(workspace, cx)?.iter().filter(|o| view.targets.contains(&o.id)) {
            let (is, was) = (effective_tier(obs, &now), effective_tier(obs, &before));
            if is != was {
                acts.push(GateAct {
                    kind: if was < is { "claim_demotion" } else { "claim_promotion" }.into(),
                    targets: vec![obs.id.clone()],
                    tier: was,
                });
            }
        }
        let (belief_diff, checks) = if acts.is_empty() {
            (None, Vec::new())
        } else {
            let p = self.preview_gate(workspace, &acts)?;
            (Some(p.belief_diff), p.checks)
        };
        Ok(Some(GateUndo {
            proposal: proposal.to_string(),
            acts: acts
                .into_iter()
                .map(|a| UndoAct {
                    observation: a.targets.into_iter().next().unwrap_or_default(),
                    kind: a.kind,
                    tier: a.tier,
                })
                .collect(),
            belief_diff,
            checks,
        }))
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
        Ok(asserted_in(self.log(workspace, cx)?.iter()))
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
    /// fold that calls this. The one row it takes from the read instead is a writer's overlay, the
    /// row about to be committed (crash-recovery.md Section 3a), which the store does not hold yet.
    fn blocking_checks(
        &self,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
        cx: &ReadCtx,
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
                             this proposal's workspace, is another kind, or has no merge verdict yet"
                        ),
                    },
                );
            }
            k if GATE_KINDS.contains(&k) => {
                // Referential integrity: you cannot promote what is not here. Under incomplete sync
                // the targets may simply not have arrived, and merging then would grant a tier to
                // nothing.
                // A proposal acts within its own workspace (proposal-workflow.md Section 6), so a
                // target written to another workspace is as absent here as one never written.
                let mut missing = Vec::new();
                for t in &view.targets {
                    let here = match cx.held(self.store.as_ref(), t)? {
                        Some(o) => view.workspace.is_empty() || o.workspace() == view.workspace,
                        None => false,
                    };
                    if !here {
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
                            "{} target observation(s) are not in this proposal's workspace: {}",
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
                            "{} target entit(ies) are not asserted in this proposal's workspace: {}",
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
        Ok(out)
    }

    /// The blocking checks that FAIL - empty means a merge verdict may take effect.
    fn blocking_failures(
        &self,
        view: &ProposalView,
        asserted: &HashSet<String>,
        decided_merges: &HashSet<String>,
        cx: &ReadCtx,
    ) -> Result<Vec<String>, StoreError> {
        Ok(self
            .blocking_checks(view, asserted, decided_merges, cx)?
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
    fn gate_diff(
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
    fn gate_grants(
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
    fn gate_tiers(
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
    fn gate_verdicts(
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
    fn gate_fold(
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
    /// - **embedding**: the name-meaning retrieval aid (Principle 19), recomputed only when the
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
        prepared: &HashMap<String, Vec<f32>>,
    ) -> Result<usize, StoreError> {
        let rows = self.materialize_entities(ws, only, cx, prepared)?;
        let count = rows.len();
        for entity in rows {
            self.store.put_entity(entity)?;
        }
        Ok(count)
    }

    /// The entity rows [`Engine::project_entities`] writes, ready to store: folded from the log `cx`
    /// reads, each carrying the stored row's properties and a vector for its text. Writes nothing,
    /// so a writer can commit them with its row (crash-recovery.md Section 3a).
    fn materialize_entities(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        cx: &ReadCtx,
        prepared: &HashMap<String, Vec<f32>>,
    ) -> Result<Vec<Entity>, StoreError> {
        let mut out = Vec::new();
        for mut entity in self.fold_entities(ws, only, cx)? {
            let text = entity_text(&entity);
            if let Some(existing) = self.store.get_entity(&entity.id)? {
                // Keep the stored embedding ONLY if its source text is unchanged (IR4: never stale).
                if existing.embedding.is_some() && entity_text(&existing) == text {
                    entity.embedding = existing.embedding;
                }
                entity.properties = existing.properties;
            }
            if entity.embedding.is_none() {
                // A vector computed before the write lock for this exact text (see `reproject`);
                // otherwise embedded here.
                if let Some(vec) = prepared.get(&text) {
                    entity.embedding = Some(vec.clone());
                } else if let Some(embedder) = &self.embedder {
                    match embedder.embed_one(&text) {
                        Ok(vec) => entity.embedding = Some(vec),
                        Err(e) => tracing::warn!(
                            entity_id = %entity.id, name = %entity.canonical_name, error = %e,
                            "entity embedding failed - stored without name-meaning recall (degrade)"
                        ),
                    }
                }
            }
            out.push(entity);
        }
        Ok(out)
    }

    /// The entity rows the log folds to (see [`Engine::project_entities`]), before anything is
    /// read from the stored rows: no embedding, no carried properties.
    fn fold_entities(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        cx: &ReadCtx,
    ) -> Result<Vec<Entity>, StoreError> {
        let gates = self.gate_grants(Some(ws), cx)?;
        let mut obss = self.observations_over(Some(ws), cx.overlay.as_ref())?;
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
                    // Ascending replay: the highest HLC wins among the observations that supplied
                    // one. A blank description is an omission, and an omission never erases (P8).
                    if let Some(d) = description.filter(|d| !d.trim().is_empty()) {
                        descr.insert(id.clone(), d.to_string());
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

        let mut out = Vec::with_capacity(name_cands.len());
        for (id, ncs) in &name_cands {
            let canonical =
                self.policy.choose(ncs).map(|c| ncs[c.index].value.clone()).unwrap_or_default();
            let aliases = alias_set(ncs, &canonical);
            let kind = kind_cands
                .get(id)
                .and_then(|ks| self.policy.choose(ks).map(|c| ks[c.index].value.clone()))
                .unwrap_or_else(|| "Concept".to_string());
            out.push(Entity {
                id: id.clone(),
                kind,
                canonical_name: canonical,
                aliases,
                description: descr.get(id).cloned(),
                properties: serde_json::Value::Null,
                provenance: prov.get(id).cloned().unwrap_or_default(),
                embedding: None,
            });
        }
        Ok(out)
    }

    /// Vectors for the entity texts a re-materialization of `ws` will need - every row whose stored
    /// vector is missing or was made for other text - computed with no lock held, as the backfill
    /// does (semantic-retrieval.md SR7), so a write does not wait out the embedder. The projection
    /// then takes a vector from here only for the exact text it was made for (IR4). Empty without an
    /// embedder; a batch that fails is left to be embedded under the lock, as before.
    fn vectors_for_reproject(&self, ws: &str) -> HashMap<String, Vec<f32>> {
        let mut prepared = HashMap::new();
        let Some(embedder) = &self.embedder else {
            return prepared;
        };
        let Ok(rows) = self.fold_entities(ws, None, &ReadCtx::default()) else {
            return prepared;
        };
        let mut texts: Vec<String> = Vec::new();
        for row in &rows {
            let text = entity_text(row);
            let stored = self.store.get_entity(&row.id).ok().flatten();
            let fresh = stored.is_some_and(|e| e.embedding.is_some() && entity_text(&e) == text);
            if !fresh && !prepared.contains_key(&text) && !texts.contains(&text) {
                texts.push(text);
            }
        }
        for chunk in texts.chunks(32) {
            let refs: Vec<&str> = chunk.iter().map(String::as_str).collect();
            if let Ok(vectors) = embedder.embed(&refs) {
                prepared.extend(chunk.iter().cloned().zip(vectors));
            }
        }
        prepared
    }

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
        let rows = self.relation_rows(ws, only, None)?;
        let written = rows.len();
        for r in rows {
            self.store.add_relation(r)?;
        }
        Ok(written)
    }

    /// The relation rows [`Engine::project_relations`] writes, folded from the log as it reads with
    /// `overlay` in it. Writes nothing, so a writer can commit them with its row (crash-recovery.md
    /// Section 3a).
    fn relation_rows(
        &self,
        ws: &str,
        only: Option<&HashSet<String>>,
        overlay: Option<&Observation>,
    ) -> Result<Vec<Relation>, StoreError> {
        let mut obss = self.observations_over(Some(ws), overlay)?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });
        let mut rels: BTreeMap<String, Relation> = BTreeMap::new();
        // Every attestation of every observation asserting the edge, credited once per observation
        // however many times it names the edge - an entity's provenance is counted the same way.
        let mut support: BTreeMap<String, Vec<Provenance>> = BTreeMap::new();
        for obs in &obss {
            let Some(prov) = authoring_attestation(obs).cloned() else {
                continue;
            };
            let mut credited: BTreeSet<String> = BTreeSet::new();
            for ra in &obs.assertions.relations {
                let from = Entity::make_id(ws, &ra.from);
                let to = Entity::make_id(ws, &ra.to);
                let kind = normalize_relation_kind(&ra.kind);
                let id = Relation::make_id(&from, &kind, &to);
                if only.is_some_and(|s| !s.contains(&id)) {
                    continue;
                }
                if credited.insert(id.clone()) {
                    support.entry(id.clone()).or_default().extend(obs.provenance.iter().cloned());
                }
                // Ascending replay, upsert by id: the valid interval is last-write, which is the
                // store's own semantics given a deterministic order. The description is the
                // highest-HLC one supplied, as for an entity: an omission, or a blank, never erases.
                let description = match ra.description.as_deref() {
                    Some(d) if !d.trim().is_empty() => Some(d.to_string()),
                    _ => rels.get(&id).and_then(|r| r.description.clone()),
                };
                rels.insert(
                    id.clone(),
                    Relation {
                        id,
                        from,
                        to,
                        kind,
                        description,
                        provenance: prov.clone(),
                        attestations: Vec::new(),
                        // The client's valid interval is projected as-is (Principle 4 capture);
                        // derivation such as auto-closing valid_to on refutation is M3c.
                        valid_from: ra.valid_from,
                        valid_to: ra.valid_to,
                    },
                );
            }
        }
        Ok(rels
            .into_iter()
            .map(|(id, mut r)| {
                r.attestations = support.remove(&id).unwrap_or_default();
                r
            })
            .collect())
    }

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
    /// Shaped exactly like [`Engine::migrate_legacy_ids`], which re-keys across a change of id
    /// FORMULA; this
    /// re-keys across a change of WORKSPACE. Lineage records the origin either way, but the
    /// live-set door treats the two differently on purpose: a migrated pair shares one workspace
    /// and folds to one row, while a re-keyed pair spans two and BOTH rows stay live - each
    /// workspace keeps its own support, and the unscoped view is the union of the scoped ones
    /// (see `Engine::observations`).
    ///
    /// Proposal events are left behind deliberately - see [`RekeyReport::skipped_proposal_events`].
    /// `dry_run` reports what would move and writes nothing.
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
        // The gate events stay behind with the rest of the proposal rows, so a merged demotion
        // would stop applying in the target, and a row quarantined at Unverified would evaluate
        // at its claimed tier again. A grant below the claim is carried as the clamp; one above
        // it is not, since a promotion is a person's act on this row and the target never saw it.
        let gates = self.gate_grants(Some(from), &ReadCtx::default())?;
        let mut report = RekeyReport::default();
        for obs in self.observations(Some(from))? {
            if !obs.assertions.proposal_events.is_empty() {
                report.skipped_proposal_events += 1;
                continue;
            }
            let granted = gates.get(&obs.id).copied();
            let Some(fresh) = recreated(&obs, to, granted) else {
                continue; // unreachable by construction (P2), but never panic on a stored row
            };
            // Idempotent: a second run finds the target already recording this origin and stops -
            // having carried the apps, which a run stopped between the two writes may have missed.
            if let Some(existing) = self.store.get_observation(&fresh.id)? {
                if existing.derived_from.contains(&obs.id) {
                    if !dry_run {
                        self.carry_apps(&obs, &fresh.id, to, granted)?;
                    }
                    report.already += 1;
                    continue;
                }
            }
            if promotion_left_behind(&obs, granted) {
                report.promotions_left_behind += 1;
            }
            if !dry_run {
                let fresh_id = fresh.id.clone();
                self.store.add_observation(fresh)?;
                self.carry_apps(&obs, &fresh_id, to, granted)?;
            }
            report.moved += 1;
        }
        if !dry_run && report.moved > 0 {
            self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(report)
    }

    /// One-shot legacy-id migration (0.x format evolution): re-creates every observation in
    /// `workspace` whose stored id predates the current content-address formula under the CURRENT
    /// id, so it can sync again. The legacy row stays as local history (Principle 3) and never
    /// exports (the wire guard in `attestations_since` and `backfill`); the successor names it in
    /// `derived_from`, and the live set folds the pair to one row (see `Engine::observations`).
    ///
    /// The twin of [`Engine::rekey_workspace`], which re-keys across a change of WORKSPACE: every
    /// attestation is carried, the sync stamp drops because it signs the old id, and the tier is
    /// clamped to the pre-strip evaluation and to a merged demotion's grant. Proposal rows are
    /// left as they are - see [`MigrateReport::skipped_proposal_events`]. Idempotent.
    pub fn migrate_legacy_ids(&self, workspace: &str) -> Result<MigrateReport, StoreError> {
        let _guard = self.write_guard.lock().unwrap_or_else(|e| e.into_inner());
        // A gate proposal names its target by the legacy id, which the successor does not carry,
        // so a demotion stops applying unless it travels as the clamp - as in a re-key.
        let gates = self.gate_grants(Some(workspace), &ReadCtx::default())?;
        let mut report = MigrateReport::default();
        for obs in self.store.all_observations(Some(workspace))? {
            if observation_content_id(workspace, &obs.content, &obs.assertions) == obs.id {
                continue;
            }
            if !obs.assertions.proposal_events.is_empty() {
                report.skipped_proposal_events += 1;
                continue;
            }
            let granted = gates.get(&obs.id).copied();
            let Some(fresh) = recreated(&obs, workspace, granted) else {
                continue; // unreachable in practice (P2: at least one attestation), never panic
            };
            if let Some(existing) = self.store.get_observation(&fresh.id)? {
                if existing.derived_from.contains(&obs.id) {
                    self.carry_apps(&obs, &fresh.id, workspace, granted)?;
                    continue; // already migrated
                }
            }
            if promotion_left_behind(&obs, granted) {
                report.promotions_left_behind += 1;
            }
            let fresh_id = fresh.id.clone();
            self.store.add_observation(fresh)?;
            self.carry_apps(&obs, &fresh_id, workspace, granted)?;
            report.migrated += 1;
        }
        if report.migrated > 0 {
            self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(report)
    }

    /// Repays the owed-projection ledger (crash-recovery.md K3): re-projects every workspace it
    /// names. A writer process runs this right after opening the store and before it serves, so no
    /// read is answered from a graph known to be behind its log. Recovery is a reproject, so it
    /// produces what the interrupted writes would have (IR3) and running it twice changes nothing.
    ///
    /// `Ok(None)` when nothing was owed - the normal case, one ledger read.
    pub fn repay_owed(&self) -> Result<Option<Recovery>, StoreError> {
        let owed = self.store.owed_projections()?;
        if owed.is_empty() {
            return Ok(None);
        }
        let workspaces: std::collections::BTreeSet<String> =
            owed.iter().map(|(_, w)| w.clone()).collect();
        for ws in &workspaces {
            self.reproject(Some(ws))?;
        }
        let recovery = Recovery {
            at: self.clock.now_millis(),
            workspaces: workspaces.into_iter().collect(),
            observations: owed.len(),
        };
        *self.last_recovery.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(recovery.clone());
        Ok(Some(recovery))
    }

    /// The last repayment this engine ran (K5), for the operator's surfaces.
    pub fn last_recovery(&self) -> Option<Recovery> {
        self.last_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Semantic retrieval for the operator's surfaces (semantic-retrieval.md SR3): `None` when no
    /// embedder is configured, otherwise the model and whether it can answer now.
    pub fn semantic(&self) -> Option<SemanticReport> {
        let e = self.embedder.as_ref()?;
        let (state, note) = match e.readiness() {
            supragnosis_core::Readiness::Ready => ("on", None),
            supragnosis_core::Readiness::Degraded(note) => ("off", Some(note)),
        };
        let backfill =
            self.backfill.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        Some(SemanticReport { model: e.id(), dims: e.dimensions(), state, note, backfill })
    }

    /// Embeds, once, every observation and entity in the store that carries no vector - what was
    /// written before the model, or while it was missing (semantic-retrieval.md Section 12). Each
    /// batch is embedded without the write guard and written under it (SR7). An observation's
    /// vector is attached and nothing else is written (SR6); an entity's is stored only if the
    /// entity still has none and its text is the text that was embedded (IR4). An embed or a write
    /// that fails stops the run with the reason, recorded like the progress. `None` when no
    /// embedder is configured.
    pub fn backfill_embeddings(&self, batch: usize) -> Option<BackfillProgress> {
        let embedder = self.embedder.clone()?;
        let batch = batch.max(1);
        let set = |p: &BackfillProgress| {
            *self.backfill.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(p.clone());
        };
        let stop = |mut p: BackfillProgress, why: String| {
            tracing::warn!(
                embedded = p.embedded,
                found = p.found,
                "the backfill stopped - the rest stays keyword only until the next start: {why}"
            );
            p.state = "stopped";
            p.note = Some(why);
            set(&p);
            p
        };
        let mut progress = BackfillProgress { state: "running", found: 0, embedded: 0, note: None };
        let (observations, entities) =
            match (self.observations(None), self.store.all_entities(None)) {
                (Ok(o), Ok(e)) => (o, e),
                (Err(e), _) | (_, Err(e)) => {
                    return Some(stop(progress, format!("the store could not be read: {e}")))
                }
            };
        let observations: Vec<(String, String)> = observations
            .into_iter()
            .filter(|o| o.embedding.is_none())
            .map(|o| (o.id, o.content))
            .collect();
        let entities: Vec<(String, String)> = entities
            .into_iter()
            .filter(|e| e.embedding.is_none())
            .map(|e| {
                let text = entity_text(&e);
                (e.id, text)
            })
            .collect();
        progress.found = observations.len() + entities.len();
        if progress.found == 0 {
            progress.state = "done";
            set(&progress);
            return Some(progress);
        }
        tracing::info!(
            observations = observations.len(),
            entities = entities.len(),
            "embedding what was written before the model"
        );
        set(&progress);
        let embed = |chunk: &[(String, String)]| {
            let texts: Vec<&str> = chunk.iter().map(|(_, t)| t.as_str()).collect();
            embedder.embed(&texts).and_then(|v| {
                if v.len() == chunk.len() {
                    Ok(v)
                } else {
                    Err(supragnosis_core::EmbedError::Provider(format!(
                        "{} texts gave {} vectors",
                        chunk.len(),
                        v.len()
                    )))
                }
            })
        };
        for chunk in observations.chunks(batch) {
            let vectors = match embed(chunk) {
                Ok(v) => v,
                Err(e) => return Some(stop(progress, format!("an embed failed: {e}"))),
            };
            let pairs: Vec<(String, Vec<f32>)> =
                chunk.iter().map(|(id, _)| id.clone()).zip(vectors).collect();
            let written = {
                let _write =
                    self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                self.store.attach_observation_embeddings(&pairs)
            };
            match written {
                Ok(n) => progress.embedded += n,
                Err(e) => return Some(stop(progress, format!("a write failed: {e}"))),
            }
            set(&progress);
        }
        for chunk in entities.chunks(batch) {
            let vectors = match embed(chunk) {
                Ok(v) => v,
                Err(e) => return Some(stop(progress, format!("an embed failed: {e}"))),
            };
            let written: Result<usize, StoreError> = {
                let _write =
                    self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut n = 0;
                let mut result = Ok(());
                for ((id, text), vec) in chunk.iter().zip(vectors) {
                    // Re-read under the guard: a projection that rewrote the entity since the scan
                    // wins, and a vector for text it no longer has is never stored (IR4, SR7).
                    match self.store.get_entity(id) {
                        Ok(Some(mut e)) if e.embedding.is_none() && entity_text(&e) == *text => {
                            e.embedding = Some(vec);
                            if let Err(err) = self.store.put_entity(e) {
                                result = Err(err);
                                break;
                            }
                            n += 1;
                        }
                        Ok(_) => {}
                        Err(err) => {
                            result = Err(err);
                            break;
                        }
                    }
                }
                result.map(|()| n)
            };
            match written {
                Ok(n) => progress.embedded += n,
                Err(e) => return Some(stop(progress, format!("a write failed: {e}"))),
            }
            set(&progress);
        }
        progress.state = "done";
        tracing::info!(
            embedded = progress.embedded,
            found = progress.found,
            "the backfill finished - what was written before the model is found by meaning too"
        );
        set(&progress);
        Some(progress)
    }

    /// The serving process's backfill worker (semantic-retrieval.md Section 12): waits until the
    /// embedder can answer - the model's files are in the cache - checking every `poll`, then runs
    /// [`Engine::backfill_embeddings`] once. Returns at once, with `None`, when no embedder is
    /// configured.
    pub fn backfill_when_ready(
        &self,
        poll: std::time::Duration,
        batch: usize,
    ) -> Option<BackfillProgress> {
        let embedder = self.embedder.as_ref()?;
        while embedder.readiness() != supragnosis_core::Readiness::Ready {
            std::thread::sleep(poll);
        }
        self.backfill_embeddings(batch)
    }

    /// How many log rows still owe their projection right now. The engine's own writers never leave
    /// one (crash-recovery.md Section 3a), so non-zero on a running node means an append projected
    /// after the fact - a sync apply, a stamping backfill, a re-key - has not been re-materialized,
    /// or its re-materialization failed: the fact is in the log, and the next open repays it.
    pub fn owed_count(&self) -> Result<usize, StoreError> {
        Ok(self.store.owed_projections()?.len())
    }

    /// The nodes whose attestations a workspace's log holds, by origin id (docs/remote-server.md
    /// R5). An unstamped attestation is this node's own and names no origin.
    pub fn origins(
        &self,
        workspace: &str,
    ) -> Result<std::collections::BTreeSet<String>, StoreError> {
        let mut out = std::collections::BTreeSet::new();
        for obs in self.store.all_observations(Some(workspace))? {
            for p in &obs.provenance {
                if let Some(s) = &p.sync {
                    out.insert(s.origin_node.clone());
                }
            }
        }
        Ok(out)
    }

    /// Rebuilds the materialized entity/relation projection of a workspace from the observation log,
    /// replayed in (ordering HLC, id) order - the deterministic re-materialization step after sync
    /// apply (F1/F3; architecture.md: "orders by HLC, then re-materializes"). Same log -> same replay
    /// order -> same materialization on every node (P16/F5), unlike observe-time upserts whose
    /// last-write-wins fields depend on local arrival order. Builds fresh states and upserts them
    /// (idempotent: a second run writes identical rows); rows with no support in the log are left in
    /// place - removal is a curation concern, not reprojection's (Principle 3).
    pub fn reproject(&self, workspace: Option<&str>) -> Result<ReprojectReport, StoreError> {
        // A re-materialization is a write section like observe's: it reads the log and then upserts
        // every row of the workspace, so an observe landing between the two would have its
        // projection overwritten by a snapshot taken before it. The engine's write guard serializes
        // the two (architecture.md Section 14); reads stay concurrent. The vectors it needs are
        // computed first, with no lock held, so a write waits for the projection and not the model.
        let ws = workspace.unwrap_or(&self.default_workspace).to_string();
        let prepared = self.vectors_for_reproject(&ws);
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // K2: what this reproject repays is what the ledger held BEFORE it read the log. An append
        // landing while it runs - only a sync apply or a stamp, which append without the write
        // guard - may be missing from the snapshot it projects, so its entry stays for the next
        // reproject to clear. Read before the context below exists.
        let owed: Vec<String> = self
            .store
            .owed_projections()?
            .into_iter()
            .filter(|(_, w)| *w == ws)
            .map(|(id, _)| id)
            .collect();
        let cx = &ReadCtx::default();
        // Entities: the same resolution write path the incremental observe uses, over ALL entities
        // (only = None) - so a reproject and an incremental write agree row-for-row (IR3).
        let entities = self.project_entities(&ws, None, cx, &prepared)?;

        // The same fold the incremental write runs, over every edge rather than one observe's.
        let relations = self.project_relations(&ws, None)?;
        self.store.clear_owed(&owed)?;
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
        // An edge's attestations are already this entity's or its neighbor's provenance; listing them
        // again on every relation would grow the answer with edges times sayings for nothing new.
        let mut relations = relations;
        for r in &mut relations {
            r.attestations.clear();
        }
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
        self.observation_rows(workspace, entity, limit, None, false)
    }

    /// [`Engine::observation_log`] for the local viewer: each attestation names the app that wrote
    /// it where this node recorded one (design-system.md Section 29.8). Only the local console's
    /// routes call it; the read tier and MCP read [`Engine::observation_log`].
    pub fn observation_log_local(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        self.observation_rows(workspace, entity, limit, None, true)
    }

    /// One observation by its id, as [`Engine::observation_log`] answers it in `workspace` (every
    /// workspace when `None`), so a card opens without reading a scope's log (design-system.md
    /// Section 29.7). `None` for an id the scope does not hold (absence, Principle 5).
    pub fn observation(
        &self,
        id: &str,
        workspace: Option<&str>,
    ) -> Result<Option<ObsSummary>, StoreError> {
        Ok(self
            .observation_rows(workspace, None, None, Some(id), false)?
            .into_iter()
            .next())
    }

    /// [`Engine::observation`] for the local viewer, naming each attestation's app.
    pub fn observation_local(
        &self,
        id: &str,
        workspace: Option<&str>,
    ) -> Result<Option<ObsSummary>, StoreError> {
        Ok(self.observation_rows(workspace, None, None, Some(id), true)?.into_iter().next())
    }

    /// The workspace an observation was written to, or `None` for an id no workspace holds - what
    /// the read tier checks against a principal's grants before it answers by id.
    pub fn observation_workspace(&self, id: &str) -> Result<Option<String>, StoreError> {
        let cx = &ReadCtx::default();
        Ok(self
            .log(None, cx)?
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.workspace().to_string()))
    }

    fn observation_rows(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
        only: Option<&str>,
        apps: bool,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        let cx = &ReadCtx::default();
        let fwd = self.merge_forwarding(workspace, cx)?;
        let (gates, moved) = self.gate_fold(workspace, cx, &[], None)?;
        // Each moving event's verdict, said by its authoring attestation: when, at which surface,
        // from which sync origin (design-system.md Section 29.7).
        let verdicts: HashSet<&str> = moved.values().map(|m| m.1.as_str()).collect();
        let mut verdict_said: HashMap<String, (Timestamp, Option<String>, Option<String>)> =
            HashMap::new();
        if !verdicts.is_empty() {
            for obs in self.log(workspace, cx)?.iter() {
                if !verdicts.contains(obs.id.as_str()) {
                    continue;
                }
                if let Some(a) = authoring_attestation(obs) {
                    verdict_said.insert(
                        obs.id.clone(),
                        (
                            a.observed_at,
                            a.source_ref.clone(),
                            a.sync.as_ref().map(|s| s.origin_node.clone()),
                        ),
                    );
                }
            }
        }
        let gated_by = |id: &str| {
            moved.get(id).map(|(proposal, verdict, promotion)| {
                let (at, surface, origin_node) =
                    verdict_said.get(verdict).cloned().unwrap_or((0, None, None));
                GateMove {
                    proposal: proposal.clone(),
                    promotion: *promotion,
                    verdict: verdict.clone(),
                    at,
                    surface,
                    origin_node,
                }
            })
        };
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
        // A target that is not an entity - a gate act's observation, an un-merge's merge - is said as
        // the proposal rows say it (design-system.md Section 29.3).
        let mut labels: BTreeMap<String, String> = BTreeMap::new();
        for (pid, v) in &proposals {
            if v.targets.iter().chain(v.into.iter()).all(|t| names.contains_key(t)) {
                continue;
            }
            let (ls, into) = self.labels_for(pid, v, &proposals)?;
            for (t, l) in v.targets.iter().zip(ls).chain(v.into.iter().zip([into])) {
                if let (false, Some(l)) = (names.contains_key(t), l) {
                    labels.entry(t.clone()).or_insert(l);
                }
            }
        }
        // A target keeps its id when nothing can name it - a merged-away row still resolves here
        // because the entity table keeps it; only a target this node has never projected stays bare.
        let eref = |id: &str| EntityRef {
            name: names
                .get(id)
                .or_else(|| labels.get(id))
                .cloned()
                .unwrap_or_else(|| id.to_string()),
            id: id.to_string(),
        };
        let mut out: Vec<ObsSummary> = Vec::new();
        // Each row's attestation keys, in its attestations' order, when the read names apps.
        let mut app_keys: HashMap<String, Vec<String>> = HashMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            if only.is_some_and(|o| o != obs.id) {
                continue;
            }
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
            let attestations: Vec<AttestationSummary> = obs
                .provenance
                .iter()
                .map(|p| AttestationSummary {
                    app: None,
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
            if apps {
                app_keys.insert(
                    obs.id.clone(),
                    obs.provenance.iter().map(|p| attestation_key(&obs.id, p)).collect(),
                );
            }
            out.push(ObsSummary {
                hlc: ordering_hlc(obs),
                effective_tier: effective_tier(obs, &gates),
                id: obs.id.clone(),
                workspace: ws,
                content: obs.content.clone(),
                proposal,
                derived_from: obs.derived_from.clone(),
                attestations,
                author: authoring_index(obs).unwrap_or(0),
                gated_by: gated_by(&obs.id),
                entities,
                relations,
            });
        }
        // Newest first: descending ordering HLC, ascending id as the stable tiebreak (P16).
        out.sort_by(|a, b| b.hlc.cmp(&a.hlc).then_with(|| a.id.cmp(&b.id)));
        if let Some(n) = limit {
            out.truncate(n);
        }
        if apps {
            self.name_apps(&mut out, &app_keys)?;
        }
        Ok(out)
    }

    /// Names each attestation's app where this node recorded one (design-system.md Section 29.8).
    /// A row exists only for what this node's own sessions wrote, so a peer's attestation is never
    /// named. A backend failure is an error, not "no app" (Principle 5).
    fn name_apps(
        &self,
        rows: &mut [ObsSummary],
        app_keys: &HashMap<String, Vec<String>>,
    ) -> Result<(), StoreError> {
        let mut keys: Vec<String> = Vec::new();
        let mut at: Vec<(usize, usize, String)> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            for (j, key) in app_keys.get(&row.id).into_iter().flatten().enumerate() {
                keys.push(key.clone());
                at.push((i, j, key.clone()));
            }
        }
        if keys.is_empty() {
            return Ok(());
        }
        let found = self.store.attestation_apps(&keys)?;
        for (i, j, key) in at {
            if let (Some(name), Some(a)) = (found.get(&key), rows[i].attestations.get_mut(j)) {
                a.app = Some(supragnosis_core::client_said(name));
            }
        }
        Ok(())
    }

    /// "Why is this node projected this way" (observability): the per-field belief resolution
    /// (evidence + decision) and the supporting observation log for one entity. Built ON TOP of
    /// [`Engine::get_entity`] - the winner values ARE the projected entity's values, so the
    /// explanation cannot disagree with what the graph shows (never a second computation that could
    /// drift). The candidate rows and supporting log are folded from the same log the projection
    /// uses. `None` iff the id resolves to no entity (absence, Principle 5). A merged-away id
    /// forwards to its canonical entity (Principle 15), like [`Engine::get_entity`].
    pub fn explain_entity(&self, entity_id: &str) -> Result<Option<EntityExplain>, StoreError> {
        self.explain_entity_as(entity_id, false)
    }

    /// [`Engine::explain_entity`] for the local viewer: its supporting rows name each attestation's
    /// app (design-system.md Section 29.8).
    pub fn explain_entity_local(
        &self,
        entity_id: &str,
    ) -> Result<Option<EntityExplain>, StoreError> {
        self.explain_entity_as(entity_id, true)
    }

    fn explain_entity_as(
        &self,
        entity_id: &str,
        apps: bool,
    ) -> Result<Option<EntityExplain>, StoreError> {
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
        let mut bases: HashMap<String, TrustTier> = HashMap::new();
        for obs in self.log(ws.as_deref(), cx)?.iter() {
            let obs_ws = obs.workspace().to_string();
            let eff = effective_tier(obs, &gates);
            bases.insert(
                obs.id.clone(),
                obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default(),
            );
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
                candidates: candidate_rows(
                    &name_cands,
                    &view.entity.canonical_name,
                    "alias",
                    &bases,
                ),
            },
            // kind: the projected kind is the winner; non-winning kinds are competitors (R7).
            // contested is get_entity's own flag (the identical resolve_kind result).
            FieldExplain {
                field: "kind",
                winner: view.entity.kind.clone(),
                contested: view.contested,
                candidates: candidate_rows(&kind_cands, &view.entity.kind, "competitor", &bases),
            },
        ];

        let supporting = self.observation_rows(ws.as_deref(), Some(&canon_id), None, None, apps)?;
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
    /// answer is untouched.
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

/// The text to embed for an entity: canonical name + aliases (if any). Opens semantic retrieval by the meaning of the name.
/// Since aliases hold notation variants, embedding them together widens the reach to other notations of the same target.
fn entity_text(entity: &Entity) -> String {
    if entity.aliases.is_empty() {
        entity.canonical_name.clone()
    } else {
        format!("{} {}", entity.canonical_name, entity.aliases.join(" "))
    }
}

/// The entity ids a run of observations asserts, each minted in its observation's workspace: the
/// referential-integrity check's input (proposal-workflow.md Section 6), read off the log rather than
/// the projection (I8).
fn asserted_in<'a>(obss: impl Iterator<Item = &'a Observation>) -> HashSet<String> {
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

/// The workspace an entity belongs to. The id is blake3(workspace + normalized name), so an entity
/// lives in exactly one workspace and every attestation agrees; the first is representative.
/// Candidate generators need this because a report built with `workspace: None` (the viewer's
/// all-workspaces view) otherwise groups across workspaces, and an entity_merge spanning two of them
/// is refused (Principle 15) - the same name in two workspaces is two entities.
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
fn recall_weights(log: &[Observation], gates: &HashMap<String, TrustTier>) -> Vec<RecallWeight> {
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

#[cfg(test)]
mod tests {

    /// sync-correctness.md Section 11: content past 1 MiB is refused at ingest, with the limit
    /// named, so every event fits inside the sync bounds.
    #[test]
    fn an_observation_over_the_content_cap_is_refused_at_ingest() {
        let engine = Engine::new(Arc::new(supragnosis_store::InMemoryStore::new()), "h", "ws");
        let input = |content: String| ObserveInput {
            content,
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: Vec::new(),
            entities: Vec::new(),
            relations: Vec::new(),
        };
        let Err(ObserveError::Invalid(msg)) =
            engine.observe(input("x".repeat(MAX_CONTENT_BYTES + 1)))
        else {
            panic!("an observation over the cap must be refused");
        };
        assert!(msg.contains("1 MiB"), "{msg}");
        let fits = input("x".repeat(MAX_CONTENT_BYTES));
        assert!(engine.observe(fits).is_ok(), "the cap itself is allowed");
    }
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

    /// I17's discriminator, pinned on its own: until recall has its commit effect the fold demotes
    /// every recall merge before this is read (design-system.md Section 29.22), so no merged recall
    /// can show it end to end. A verdict counts as the console's when any attestation carries the
    /// marker - a person's console act on an agent-marked row is a second attestation on that row (P3).
    #[test]
    fn the_console_marker_is_read_off_every_attestation() {
        let at = |source_ref: Option<&str>| Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: source_ref.map(String::from),
            observed_at: 1,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        };
        let mut obs = Observation::with_assertions(
            "proposal(merge) p".into(),
            at(Some(VERDICT_SURFACE_AGENT)),
            Assertions::default(),
        );
        assert!(!console_cast(&obs), "an agent's verdict is not the console's");
        obs.provenance.push(at(None));
        assert!(!console_cast(&obs), "an unmarked writer is not the console");
        obs.provenance.push(at(Some(VERDICT_SURFACE_CONSOLE)));
        assert!(console_cast(&obs), "the person's act on the same row is theirs");
        assert!(!has_commit_effect("recall") && !has_commit_effect("tbox_change"));
        for kind in ["entity_merge", "entity_split", "claim_promotion", "claim_demotion"] {
            assert!(has_commit_effect(kind), "{kind}");
        }
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

    /// An entity_merge may name its entities by name: each is stored as the id `observe` gave that name
    /// in the workspace, so the proposal is not blocked for naming what exists. A name that names
    /// nothing is refused at open, with the target named; an id-shaped target that is not here is kept,
    /// for the referential-integrity check to say so (design-system.md Section 14 step 13).
    #[test]
    fn a_merge_named_by_name_holds_ids() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "test-host", "ws1");
        engine
            .observe(ObserveInput {
                content: "PostgreSQL and Postgres".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { name: "PostgreSQL".into(), kind: None, description: None },
                    EntityInput { name: "Postgres".into(), kind: None, description: None },
                ],
                relations: vec![],
            })
            .expect("observe");
        let merge = |targets: Vec<&str>, into: &str| ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: targets.into_iter().map(String::from).collect(),
            into: Some(into.into()),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        };
        let (sql, pg) = (Entity::make_id("ws1", "PostgreSQL"), Entity::make_id("ws1", "Postgres"));
        // Names, an id beside a name, and a name in another case all land on the same ids.
        for (targets, into) in [
            (vec!["PostgreSQL", "Postgres"], "Postgres"),
            (vec![sql.as_str(), "Postgres"], pg.as_str()),
            (vec!["postgresql", "POSTGRES"], "postgres"),
        ] {
            let pid = engine.propose(merge(targets, into)).expect("open the merge");
            let p = engine.get_proposal(Some("ws1"), &pid).unwrap().expect("stored");
            assert_eq!(p.targets, vec![sql.clone(), pg.clone()]);
            assert_eq!(p.into.as_deref(), Some(pg.as_str()));
            assert_eq!(p.state, "open", "a merge of named entities is not blocked");
        }
        // A name that names nothing is refused, and the refusal names it.
        let err = engine.propose(merge(vec!["PostgreSQL", "MySQL"], "PostgreSQL")).unwrap_err();
        assert!(err.to_string().contains("'MySQL'"), "{err}");
        // An id that is not here yet is kept: the integrity check reports it.
        let elsewhere = "ab".repeat(32);
        let pid = engine
            .propose(merge(vec![sql.as_str(), elsewhere.as_str()], &sql))
            .expect("kept");
        let p = engine.get_proposal(Some("ws1"), &pid).unwrap().unwrap();
        assert_eq!(p.targets, vec![sql.clone(), elsewhere.clone()]);
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

        // An entity_merge carries no affected_types (the field folds to empty). Its targets are ids of
        // entities not in this log, which propose keeps for the integrity check.
        let (id_a, id_b) = ("a".repeat(64), "b".repeat(64));
        let mid = engine
            .propose(ProposeInput {
                workspace: None,
                kind: "entity_merge".into(),
                targets: vec![id_a, id_b.clone()],
                into: Some(id_b),
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

    /// Retrieval regression (Appendix B): with an embedder attached, hybrid search recalls observations that keyword
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
    /// Observations and entities written through an engine with no embedder - what a node held
    /// before the model - for the backfill tests (semantic-retrieval.md Section 12).
    fn written_before_the_model(store: &Arc<InMemoryStore>) -> Vec<String> {
        let plain = Engine::new(store.clone(), "h", "ws1");
        let mut ids = Vec::new();
        for (content, names) in [
            (
                "the worker crashed at boot because the connection pool was exhausted",
                vec!["Alpha"],
            ),
            (
                "deploy rollbacks re-point the image tag and never touch migrations",
                vec!["Beta"],
            ),
            ("the cache is cleared only when a price changes", vec!["Alpha", "Gamma"]),
        ] {
            let out = plain
                .observe(ObserveInput {
                    content: content.into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: names
                        .into_iter()
                        .map(|n| EntityInput { name: n.into(), kind: None, description: None })
                        .collect(),
                    relations: vec![],
                })
                .expect("observe");
            ids.push(out.observation_id);
        }
        ids
    }

    fn rows_as_json(store: &InMemoryStore, ids: &[String]) -> Vec<serde_json::Value> {
        ids.iter()
            .map(|id| {
                serde_json::to_value(store.get_observation(id).unwrap().expect("row")).unwrap()
            })
            .collect()
    }

    /// SR6: what was written before the model is embedded - every observation and entity - and
    /// nothing else is written: the rows and their attestations are what they were, nothing is
    /// owed, the merge band covers every live entity, and an old observation is found by meaning.
    #[test]
    fn what_was_written_before_the_model_is_embedded_and_nothing_else() {
        let store = Arc::new(InMemoryStore::new());
        let ids = written_before_the_model(&store);
        assert!(store.all_entities(None).unwrap().iter().all(|e| e.embedding.is_none()));
        assert!(store.owed_projections().unwrap().is_empty());
        let before = rows_as_json(&store, &ids);
        let embedder = Arc::new(supragnosis_embed::HashingEmbedder::default());
        let engine = Engine::new(store.clone(), "h", "ws1").with_embedder(embedder.clone());
        let q = embedder.embed_one("pool exhausted worker").unwrap();
        assert!(store.search_semantic(&q, None, 3).unwrap().is_empty(), "no vectors yet");

        let p = engine.backfill_embeddings(2).expect("an embedder is configured");
        let entities = store.all_entities(None).unwrap();
        assert_eq!(p.state, "done");
        assert_eq!(p.found, ids.len() + entities.len());
        assert_eq!(p.embedded, p.found);
        for id in &ids {
            assert!(store.get_observation(id).unwrap().unwrap().embedding.is_some(), "{id}");
        }
        assert!(entities.iter().all(|e| e.embedding.is_some()));
        assert_eq!(rows_as_json(&store, &ids), before, "the rows are untouched");
        assert!(store.owed_projections().unwrap().is_empty(), "nothing is owed");
        let band = engine.curation(Some("ws1")).unwrap().merge_band;
        assert_eq!(band.embedded, band.examined, "every live entity is covered");
        let hits = store.search_semantic(&q, None, 1).unwrap();
        assert_eq!(hits.first().map(|h| h.id.as_str()), Some(ids[0].as_str()));
        assert_eq!(engine.semantic().unwrap().backfill, Some(p), "the report carries it");
    }

    /// SR7: a second run finds nothing to do, and a stored vector is kept as it was.
    #[test]
    fn a_second_backfill_embeds_nothing_and_keeps_a_stored_vector() {
        let store = Arc::new(InMemoryStore::new());
        written_before_the_model(&store);
        let engine = Engine::new(store.clone(), "h", "ws1")
            .with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::default()));
        engine.backfill_embeddings(8).unwrap();
        let alpha = Entity::make_id("ws1", "Alpha");
        let vector = store.get_entity(&alpha).unwrap().unwrap().embedding;
        assert!(vector.is_some());
        let again = engine.backfill_embeddings(8).unwrap();
        assert_eq!((again.state, again.found, again.embedded), ("done", 0, 0));
        assert_eq!(store.get_entity(&alpha).unwrap().unwrap().embedding, vector);
    }

    /// SR7: an entity whose text changes while its batch is embedding is not given the vector of
    /// the text it no longer has; the next run embeds it with the text it has.
    #[test]
    fn an_entity_whose_text_changed_mid_batch_is_left_for_the_next_run() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};
        struct Meddler {
            store: Arc<InMemoryStore>,
            inner: supragnosis_embed::HashingEmbedder,
            done: std::sync::atomic::AtomicBool,
        }
        impl EmbeddingProvider for Meddler {
            fn dimensions(&self) -> usize {
                self.inner.dimensions()
            }
            fn id(&self) -> String {
                self.inner.id()
            }
            fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                // A projection lands while Alpha's batch embeds: the entity gains an alias.
                if texts.contains(&"Alpha")
                    && !self.done.swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    let id = Entity::make_id("ws1", "Alpha");
                    let mut e = self.store.get_entity(&id).unwrap().unwrap();
                    e.aliases.push("Alfa".into());
                    self.store.put_entity(e).unwrap();
                }
                self.inner.embed(texts)
            }
        }
        let store = Arc::new(InMemoryStore::new());
        written_before_the_model(&store);
        let meddler = Arc::new(Meddler {
            store: store.clone(),
            inner: supragnosis_embed::HashingEmbedder::default(),
            done: std::sync::atomic::AtomicBool::new(false),
        });
        let engine = Engine::new(store.clone(), "h", "ws1").with_embedder(meddler);
        let alpha = Entity::make_id("ws1", "Alpha");
        let first = engine.backfill_embeddings(8).unwrap();
        assert_eq!(first.state, "done");
        assert_eq!(first.embedded, first.found - 1, "Alpha's vector was for the old text");
        assert!(store.get_entity(&alpha).unwrap().unwrap().embedding.is_none());
        let second = engine.backfill_embeddings(8).unwrap();
        assert_eq!((second.found, second.embedded), (1, 1));
        let e = store.get_entity(&alpha).unwrap().unwrap();
        let fresh = supragnosis_embed::HashingEmbedder::default()
            .embed_one(&entity_text(&e))
            .unwrap();
        assert_eq!(e.embedding, Some(fresh), "embedded with the text it has now");
    }

    /// Principle 19 and 24: an embed that fails stops the backfill with the reason, in the report;
    /// what it did not reach keeps no vector, and a later run embeds it.
    #[test]
    fn an_embed_failure_stops_the_backfill_and_says_why() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};
        struct FailsSecond {
            inner: supragnosis_embed::HashingEmbedder,
            calls: std::sync::atomic::AtomicUsize,
        }
        impl EmbeddingProvider for FailsSecond {
            fn dimensions(&self) -> usize {
                self.inner.dimensions()
            }
            fn id(&self) -> String {
                self.inner.id()
            }
            fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                    return Err(EmbedError::Provider("the session went away".into()));
                }
                self.inner.embed(texts)
            }
        }
        let store = Arc::new(InMemoryStore::new());
        let ids = written_before_the_model(&store);
        let engine = Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(FailsSecond {
            inner: supragnosis_embed::HashingEmbedder::default(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }));
        let p = engine.backfill_embeddings(1).unwrap();
        assert_eq!((p.state, p.embedded), ("stopped", 1));
        assert!(p.note.as_deref().unwrap_or("").contains("the session went away"), "{p:?}");
        assert_eq!(engine.semantic().unwrap().backfill.unwrap().state, "stopped");
        // The store enumerates in id order, so which observation came first is not the order they
        // were written in: one has a vector, the rest have none.
        let with_vector = ids
            .iter()
            .filter(|id| store.get_observation(id).unwrap().unwrap().embedding.is_some())
            .count();
        assert_eq!(with_vector, 1);

        let later = Engine::new(store.clone(), "h", "ws1")
            .with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::default()));
        let rest = later.backfill_embeddings(8).unwrap();
        assert_eq!(rest.state, "done");
        assert_eq!(rest.found, p.found - 1);
        assert_eq!(rest.embedded, rest.found);
    }

    /// Section 12.2: the worker waits for the model's files, then runs once; with no embedder it
    /// does not start.
    #[test]
    fn the_worker_waits_for_the_models_files_then_runs_once() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use supragnosis_embed::{LazyEmbedder, LoadError, Loader, Present};
        let store = Arc::new(InMemoryStore::new());
        let ids = written_before_the_model(&store);
        let present = Arc::new(AtomicBool::new(false));
        let loads = Arc::new(AtomicUsize::new(0));
        let (p1, p2, l) = (present.clone(), present.clone(), loads.clone());
        let loader: Loader = Box::new(move || {
            l.fetch_add(1, Ordering::SeqCst);
            if p1.load(Ordering::SeqCst) {
                Ok(Box::new(supragnosis_embed::HashingEmbedder::default())
                    as Box<dyn supragnosis_core::EmbeddingProvider>)
            } else {
                Err(LoadError::Missing("no files - run supragnosis embed fetch".into()))
            }
        });
        let present_fn: Present = Box::new(move || p2.load(Ordering::SeqCst));
        let lazy = LazyEmbedder::new("hashing-256", 256, present_fn, loader);
        let engine = Arc::new(Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(lazy)));
        let worker = {
            let engine = engine.clone();
            std::thread::spawn(move || {
                engine.backfill_when_ready(std::time::Duration::from_millis(5), 8)
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(60));
        assert!(!worker.is_finished(), "it waits while the files are missing");
        assert!(store.get_observation(&ids[0]).unwrap().unwrap().embedding.is_none());
        assert_eq!(engine.semantic().unwrap().backfill, None, "nothing has started");
        present.store(true, Ordering::SeqCst);
        let p = worker.join().unwrap().expect("ran");
        assert_eq!(p.state, "done");
        assert!(p.found > 0 && p.embedded == p.found);
        assert!(ids.iter().all(|id| store
            .get_observation(id)
            .unwrap()
            .unwrap()
            .embedding
            .is_some()));

        let none = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        assert_eq!(none.backfill_when_ready(std::time::Duration::from_secs(3600), 8), None);
    }

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

    /// design-system.md Section 29.27: the band counts the near pairs it held back, each once though it
    /// reaches it from both ends, so an empty band says whether none are near or all are answered.
    #[test]
    fn the_band_counts_what_it_held_back() {
        use supragnosis_core::{EmbedError, EmbeddingProvider};
        struct Same;
        impl EmbeddingProvider for Same {
            fn dimensions(&self) -> usize {
                3
            }
            fn id(&self) -> String {
                "same-3".into()
            }
            fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0]).collect())
            }
        }
        let engine =
            Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1").with_embedder(Arc::new(Same));
        for name in ["Alpha", "Beta", "Gamma"] {
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
        }
        let rep = engine.curation(Some("ws1")).unwrap();
        assert_eq!(
            (rep.merge_suggestions.len(), rep.merge_band.held_back),
            (3, 0),
            "three near pairs, none answered"
        );
        let id = |n: &str| Entity::make_id("ws1", n);
        engine
            .propose(ProposeInput {
                workspace: Some("ws1".into()),
                kind: "entity_merge".into(),
                targets: vec![id("Alpha"), id("Beta")],
                into: Some(id("Beta")),
                tier: None,
                rationale: Some("one".into()),
                affected_types: Vec::new(),
                source_ref: None,
                on_behalf_of: None,
            })
            .expect("propose");
        let rep = engine.curation(Some("ws1")).unwrap();
        assert_eq!(rep.merge_suggestions.len(), 2);
        assert_eq!(
            rep.merge_band.held_back, 1,
            "the pair in flight is counted once, from either end"
        );
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

    /// design-system.md Section 29.28: the ladder counts, as a set, the pairs it left out because
    /// they are answered - a pair it reaches at every rung once, every pair of an empty ladder, and
    /// never a pair a listed group still shows.
    #[test]
    fn the_ladder_counts_what_it_held_back() {
        let engine = Engine::new(Arc::new(InMemoryStore::new()), "h", "ws1");
        let say = |names: &[&str]| {
            for name in names {
                engine
                    .observe(ObserveInput {
                        content: format!("about {name}"),
                        workspace: None,
                        source_ref: None,
                        confidence: None,
                        on_behalf_of: None,
                        derived_from: vec![],
                        entities: vec![EntityInput {
                            name: (*name).into(),
                            kind: None,
                            description: None,
                        }],
                        relations: vec![],
                    })
                    .expect("observe");
            }
        };
        let merge = |a: &str, b: &str| {
            let (a, b) = (Entity::make_id("ws1", a), Entity::make_id("ws1", b));
            engine
                .propose(ProposeInput {
                    workspace: Some("ws1".into()),
                    kind: "entity_merge".into(),
                    targets: vec![a.clone(), b],
                    into: Some(a),
                    tier: None,
                    rationale: Some("alike".into()),
                    affected_types: Vec::new(),
                    source_ref: None,
                    on_behalf_of: None,
                })
                .expect("propose");
        };
        let report = || engine.curation(Some("ws1")).unwrap();
        say(&["TrustTier", "Trust Tier"]);
        assert_eq!(report().name_ladder.held_back, 0, "a listed pair is not held back");
        // In flight: reached at the separator, plural and alias rungs alike, counted once.
        merge("TrustTier", "Trust Tier");
        let r = report();
        assert!(r.name_variants.is_empty());
        assert_eq!(r.name_ladder.held_back, 1, "an empty ladder counts each pair it reached once");
        // A third spelling joins: the group of three is listed for its two new pairs, and the pair in
        // flight, shown among its members, is not counted.
        say(&["trust-tier"]);
        let r = report();
        assert_eq!(r.name_variants.len(), 1);
        assert_eq!(r.name_variants[0].members.len(), 3);
        assert_eq!(r.name_ladder.held_back, 0, "a pair a listed group shows is not held back");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["name_ladder"], serde_json::json!({"held_back": 0}), "always present");
    }
}
