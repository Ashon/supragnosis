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

mod curation;
mod diffs;
mod forwarding;
mod gate;
mod observe;
mod proposal_types;
mod proposals;
mod propose;
mod review;
mod types;

use curation::*;
use diffs::*;
use observe::*;
pub use proposal_types::*;
use proposals::*;
pub use types::*;

thread_local! {
    /// The client name of the local MCP session this thread writes for ([`Engine::as_app`]).
    static WRITING_APP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The (kind, id) of the hits one stage of a search held.
type HitSet = HashSet<(SearchHitKind, String)>;

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
    let members: BTreeSet<String> = asserted_ids(obs)
        .map(|id| fwd.get(&id).cloned().unwrap_or(id))
        .filter(|id| node_ids.contains(id.as_str()))
        .collect();
    members.into_iter().collect()
}

/// The entity ids an observation asserts, as entities and as both ends of its relations, as
/// written - not forwarded through merges, the ids the entity fold keys its rows by.
fn asserted_ids(obs: &Observation) -> impl Iterator<Item = String> + '_ {
    let ws = obs.workspace();
    let names = obs.assertions.entities.iter().map(|e| e.name.as_str());
    let ends = obs.assertions.relations.iter().flat_map(|r| [r.from.as_str(), r.to.as_str()]);
    names.chain(ends).map(move |name| Entity::make_id(ws, name))
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
    ///
    /// The rows are ready to store: folded from the log `cx` reads, each carrying the stored row's
    /// properties and a vector for its text. Nothing is written, so a writer commits them with its
    /// row and a reproject with the rest of its projection, in one transaction either way
    /// (crash-recovery.md Section 3a).
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

    /// The entity rows the log folds to (see [`Engine::materialize_entities`]), before anything is
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

    /// Projects relations from the log - the edge half of what [`Engine::materialize_entities`] does for
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
    ///
    /// The rows are folded from the log as it reads with `overlay` in it. Nothing is written, so a
    /// writer commits them with its row and a reproject with the rest of its projection
    /// (crash-recovery.md Section 3a).
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
        let entities = self.materialize_entities(&ws, None, cx, &prepared)?;
        // The same fold the incremental write runs, over every edge rather than one observe's.
        let relations = self.relation_rows(&ws, None, None)?;
        let report = ReprojectReport {
            observations: self.observations(Some(&ws))?.len(),
            entities: entities.len(),
            relations: relations.len(),
        };
        // One write for the whole projection: one commit instead of one per row, and no reader sees
        // a projection half rewritten (crash-recovery.md Section 3a).
        self.store.put_projection(entities, relations)?;
        self.store.clear_owed(&owed)?;
        Ok(report)
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
        let relations = self.merged_relations(&canon_id, &fwd)?;
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

    /// The relations of an entity as the graph draws them (Principle 15): its own and every
    /// merged-away member's, endpoints forwarded to their canonical ids, a merge's self-loops
    /// dropped, and one row per (from, kind, to), the lowest relation id, as [`Engine::graph`] keeps.
    fn merged_relations(
        &self,
        canon_id: &str,
        fwd: &HashMap<String, String>,
    ) -> Result<Vec<Relation>, StoreError> {
        let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut rows = self.store.relations_of(canon_id)?;
        for (member, c) in fwd {
            if c == canon_id {
                rows.extend(self.store.relations_of(member)?);
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for mut r in rows {
            (r.from, r.to) = (canon(&r.from), canon(&r.to));
            if r.from != r.to && seen.insert((r.from.clone(), r.kind.clone(), r.to.clone())) {
                out.push(r);
            }
        }
        Ok(out)
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
    /// routes call it; MCP reads [`Engine::observation_log`].
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
        // and fold order as materialize_entities/belief_fold - so the ranking matches the projection.
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
