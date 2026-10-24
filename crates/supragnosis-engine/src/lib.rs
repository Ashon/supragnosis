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
