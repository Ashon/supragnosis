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

mod belief;
mod curation;
mod diffs;
mod forwarding;
mod gate;
mod graph;
mod maintenance;
mod observe;
mod projection;
mod proposal_types;
mod proposals;
mod propose;
mod read;
mod review;
mod search;
mod types;

use belief::*;
use curation::*;
use diffs::*;
use graph::*;
use observe::*;
use projection::*;
pub use proposal_types::*;
use proposals::*;
use search::*;
pub use types::*;

thread_local! {
    /// The client name of the local MCP session this thread writes for ([`Engine::as_app`]).
    static WRITING_APP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
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

/// A stable string label for TrustTier (matching the serialized snake_case). Used as metric keys.
fn tier_label(t: TrustTier) -> &'static str {
    match t {
        TrustTier::Unverified => "unverified",
        TrustTier::AgentExtracted => "agent_extracted",
        TrustTier::HostSigned => "host_signed",
        TrustTier::HumanConfirmed => "human_confirmed",
    }
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
}

#[cfg(test)]
mod tests;
