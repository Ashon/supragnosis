//! The public input and output types of ingest, search, reads and the graph views, re-exported
//! at the crate root.

use super::*;

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
    /// projection reads it, and MCP never asks for it.
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
