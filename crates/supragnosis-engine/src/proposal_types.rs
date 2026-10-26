//! The public types of the curation report, proposals and gate acts, re-exported at the crate
//! root.

use super::*;

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

/// The decisions a verdict carries: what `review_proposal` casts as a verdict rather than a comment
/// or a withdrawal.
pub const VERDICT_DECISIONS: [&str; 2] = ["merge", "reject"];

/// What a person reads for a proposal event (design-system.md Section 9, U17): the proposal's noun
/// and what happened to it - "Merge proposed". One table, so the Log never spells a kind or an
/// event; None for a kind or an event it has no word for, which the Log says as "Something changed".
pub fn proposal_words(
    kind: &str,
    event: ProposalEventKind,
    decision: Option<&str>,
) -> (Option<&'static str>, Option<&'static str>) {
    let noun = match kind {
        "entity_merge" => Some("Merge"),
        "entity_split" => Some("Un-merge"),
        "claim_promotion" => Some("Claim confirmation"),
        "claim_demotion" => Some("Claim demotion"),
        "tbox_change" => Some("Type change"),
        "recall" => Some("Recall"),
        _ => None,
    };
    // A verdict carries its decision, so its verb is the decision's.
    let verb = match (event, decision) {
        (ProposalEventKind::Opened, _) => Some("proposed"),
        (ProposalEventKind::Verdict, Some("merge")) => Some("accepted"),
        (ProposalEventKind::Verdict, Some("reject")) => Some("rejected"),
        (ProposalEventKind::Verdict, _) => None,
        (ProposalEventKind::Withdrawn, _) => Some("withdrawn"),
        (ProposalEventKind::Comment, _) => Some("discussed"),
    };
    (noun, verb)
}

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

/// The two gate kinds with a tier commit effect (resolution.md Section 5). Their targets are
/// observation ids, which the remote surface checks against the proposal's own workspace.
pub const GATE_KINDS: [&str; 2] = ["claim_promotion", "claim_demotion"];

/// Which surface cast a verdict (resolution.md Section 6) - decided by the CALLER CRATE per
/// call-site, never by the remote client (the review surfaces accept no source_ref of their own).
/// The engine stamps the marker into the verdict observation's provenance; the gate fold derives the
/// grant ceiling from the log-borne marker, so the cap is deterministic (I2, P16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictSurface {
    /// The local console (the viz unix socket, 0600: every process of the owning OS user, the app
    /// among them). Grants up to HumanConfirmed, which records the path and not a person
    /// (resolution.md Section 6).
    Console,
    /// The agent-facing MCP tool path. Grants cap at HostSigned.
    Agent,
}

impl VerdictSurface {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            VerdictSurface::Console => VERDICT_SURFACE_CONSOLE,
            VerdictSurface::Agent => VERDICT_SURFACE_AGENT,
        }
    }
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
