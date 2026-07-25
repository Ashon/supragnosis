//! Principle/invariant scenario suite - each test pins one norm from docs/principles.md,
//! docs/proposal-workflow.md, or docs/federation.md against the running system.
//!
//! Two kinds of test live here (the header of each test says which):
//! - **guard**: the norm is implemented; the test locks it against regression.
//! - **characterization**: the norm is deferred (architecture.md Section 14); the test pins the
//!   CURRENT interim behavior so the deferral stays visible, and the eventual fix is forced to
//!   touch (and rewrite) the test. A passing characterization test is a record, not an endorsement.
//!
//! Naming: `<principle-or-invariant>_<claim>` so a failure names the norm it violates.

use std::collections::BTreeMap;
use std::sync::Arc;

use supragnosis_core::{
    Assertions, Entity, EntityAssertion, KnowledgeStore, Observation, Provenance, TrustTier,
    VersionVector,
};
use supragnosis_engine::{Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, VerdictSurface};
use supragnosis_store::InMemoryStore;
use supragnosis_sync::{export_delta, version_vector, SyncNode};

const WS: &str = "ws";

fn engine() -> (Arc<InMemoryStore>, Engine) {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS);
    (store, engine)
}

fn observe(engine: &Engine, content: &str, entities: &[&str], relations: Vec<RelationInput>) {
    engine
        .observe(ObserveInput {
            content: content.into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: entities
                .iter()
                .map(|n| EntityInput { name: (*n).into(), kind: None, description: None })
                .collect(),
            relations,
        })
        .expect("observe");
}

fn propose_merge(engine: &Engine, targets: &[&str], into: &str, principal: &str) -> String {
    engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: targets.iter().map(|s| s.to_string()).collect(),
            into: Some(into.into()),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some(principal.into()),
        })
        .expect("propose")
}

/// Two real entities to merge. The referential-integrity check (proposal-workflow.md Section 6)
/// blocks a merge whose targets are not in the local log, so a fixture built on invented ids would
/// exercise that check instead of whatever the test is actually about.
fn mergeable_pair(engine: &Engine) -> (String, String) {
    observe(engine, "x and y", &["Ent X", "Ent Y"], vec![]);
    (Entity::make_id(WS, "Ent X"), Entity::make_id(WS, "Ent Y"))
}

fn review(engine: &Engine, proposal: &str, decision: &str, principal: &str) {
    engine
        .review_proposal(
            None,
            proposal.into(),
            decision.into(),
            None,
            Some(principal.into()),
            VerdictSurface::Console,
        )
        .expect("review");
}

/// The (node-id, edge-triple) shape of a graph - timestamps/provenance stripped, so two engines
/// built at different wall-clock moments compare equal iff they projected the same structure.
fn graph_shape(engine: &Engine) -> (Vec<String>, Vec<(String, String, String)>) {
    let g = engine.graph(Some(WS)).expect("graph");
    let mut nodes: Vec<String> = g.nodes.iter().map(|n| n.id.clone()).collect();
    nodes.sort();
    let mut edges: Vec<(String, String, String)> = g
        .edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone(), e.kind.clone()))
        .collect();
    edges.sort();
    (nodes, edges)
}

// --- P23 / I16: merge is the absorbing verdict -------------------------------------------------

/// guard (proposal-workflow.md I16, principles.md P23): once a merge verdict is in the log, no
/// later or concurrent event can retroactively cancel the promotion - a coexisting reject must
/// lose to the absorbing merge, in every arrival order.
#[test]
fn i16_merge_absorbs_over_conflicting_reject_in_any_order() {
    for reviews in [["reject", "merge"], ["merge", "reject"]] {
        let (_store, engine) = engine();
        let (x, y) = mergeable_pair(&engine);
        let p = propose_merge(&engine, &[&x, &y], &y, "alice");
        for decision in reviews {
            review(&engine, &p, decision, "bob");
        }
        let props = engine.list_proposals(Some(WS)).expect("list");
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0].state, "merged",
            "merge must absorb a coexisting reject (order {reviews:?})"
        );
        assert_eq!(props[0].verdicts, 2, "both verdicts must stay counted (P3: nothing erased)");
    }
}

// --- P16 / P6: a contradictory merge cycle -----------------------------------------------------

/// guard (P16 + P6, M3a): two merged proposals folding a<->b into each other are contradictory
/// data. The fold still resolves the cycle deterministically (hop-capped forwarding - P16), but
/// since M3a the cycle is SURFACED as a curation signal (P6 "conflict is information",
/// resolution.md Section 4.2) instead of passing silently - the remedy is a settling proposal.
/// (Formerly a characterization test pinning the silent interim.)
#[test]
fn p6_contradictory_merge_cycle_is_convergent_and_surfaced() {
    let build = |first_into: &str, second_into: &str| {
        let (_store, engine) = engine();
        observe(&engine, "alpha exists", &["alpha"], vec![]);
        observe(&engine, "beta exists", &["beta"], vec![]);
        observe(
            &engine,
            "gamma uses alpha",
            &["gamma"],
            vec![RelationInput {
                from: "gamma".into(),
                kind: "uses".into(),
                to: "alpha".into(),
                description: None,
                valid_from: None,
                valid_to: None,
            }],
        );
        let a = Entity::make_id(WS, "alpha");
        let b = Entity::make_id(WS, "beta");
        let p1 = propose_merge(&engine, &[a.as_str(), b.as_str()], first_into, "alice");
        review(&engine, &p1, "merge", "alice");
        let p2 = propose_merge(&engine, &[a.as_str(), b.as_str()], second_into, "alice");
        review(&engine, &p2, "merge", "alice");
        engine
    };
    let a = Entity::make_id(WS, "alpha");
    let b = Entity::make_id(WS, "beta");

    // Same two contradictory proposals, opened in either order.
    let e1 = build(&b, &a);
    let e2 = build(&a, &b);

    // Reproducible: repeated reads of one engine are identical (P16 reproducibility).
    assert_eq!(graph_shape(&e1), graph_shape(&e1));

    // Convergent across ingest orders (P16): NOTE - if this ever fails, the cycle resolution has
    // become arrival-order dependent, which is a real P16 violation, not a test artifact.
    assert_eq!(graph_shape(&e1), graph_shape(&e2), "cycle resolution must not depend on order");

    // Pin the current resolution shape exactly, so any change to cycle handling surfaces here.
    // Observed today: NO node collapses (all three ids survive - nothing is lost, P3), but the
    // gamma->alpha edge is rewired to beta: the hop-capped forwarding resolves the 2-cycle by
    // iteration parity, not by any principled rule. The graph thus shows an edge into an entity
    // no observation ever asserted an edge to, with no contradiction signal raised (the P6 gap).
    let (nodes, edges) = graph_shape(&e1);
    let g = Entity::make_id(WS, "gamma");
    assert_eq!(nodes, {
        let mut v = vec![a.clone(), b.clone(), g.clone()];
        v.sort();
        v
    });
    assert_eq!(
        edges,
        vec![(g, b.clone(), "uses".to_string())],
        "the cycle rewires the edge to the parity-chosen side (deterministic, P16); if this \
         became order-dependent, that is a P16 regression"
    );

    // P6 (M3a): the contradiction is surfaced - the cycle members and the proposals that formed it
    // appear in the curation report on both engines, identically (a fold-projection, F5).
    for e in [&e1, &e2] {
        let cur = e.curation(Some(WS)).expect("curation");
        assert_eq!(cur.stats.merge_cycles, 1, "the merge cycle must be surfaced (P6)");
        let cycle = &cur.merge_cycles[0];
        let mut ids: Vec<&str> = cycle.members.iter().map(|m| m.id.as_str()).collect();
        ids.sort();
        let mut expect = vec![a.as_str(), b.as_str()];
        expect.sort();
        assert_eq!(ids, expect);
        assert_eq!(cycle.proposals.len(), 2, "both contradictory proposals are named");
    }
}

// --- P6 / M3a: kind conflicts surface as contested and mediation settles them ------------------

/// An observation asserting `name` is of `kind`, at a fixed transaction time (deterministic HLC
/// via the legacy fallback - no wall clock in the test).
fn kind_obs(name: &str, kind: &str, observed_at: u64) -> Observation {
    Observation::with_assertions(
        format!("{name} kind assertion at {observed_at}"),
        Provenance {
            host: "host-a".into(),
            on_behalf_of: None,
            workspace: WS.into(),
            source_ref: None,
            observed_at,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        },
        Assertions {
            entities: vec![EntityAssertion { name: name.into(), kind: Some(kind.into()), description: None }],
            ..Default::default()
        },
    )
}

/// guard (resolution.md Sections 4-6, R5-R8; principles.md P6/P18): a tier-tied kind conflict
/// surfaces as contested (recency alone picked the winner), and a console confirmation - a gated
/// claim_promotion verdict, never an edit - settles it by trust. The losing value stays queryable
/// (R7), and the node's representative tier is the effective tier including the grant.
#[test]
fn p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it() {
    let (store, engine) = engine();
    let o1 = kind_obs("cozo", "Tool", 100);
    let id1 = o1.id.clone();
    let o2 = kind_obs("cozo", "Library", 200);
    let id2 = o2.id.clone();
    store.add_observation(o1).unwrap();
    store.add_observation(o2).unwrap();
    engine.reproject(Some(WS)).expect("reproject");

    let g = engine.graph(Some(WS)).expect("graph");
    let n = g.nodes.iter().find(|n| n.name == "cozo").expect("node");
    assert_eq!(n.kind, "Library", "within a tied tier band, the later HLC wins (R2)");
    assert!(n.contested, "tier-tied distinct values must flag contested (R6)");
    assert_eq!(n.competitors.len(), 1);
    assert_eq!(n.competitors[0].value, "Tool");
    assert_eq!(n.kind_source.as_deref(), Some(id2.as_str()), "the winner's asserting observation is the mediation handle");
    let cur = engine.curation(Some(WS)).expect("curation");
    assert_eq!(cur.stats.contradictions, 1, "the conflict must appear in the P6 introspection list");
    assert!(cur.contradictions[0].contested);

    // Mediation: confirm "Tool" from the console. Promotion is a gated verdict (P23), and the
    // console surface is what permits human_confirmed (a human's direct act, P18).
    let pid = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![id1.clone()],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: Some("the human confirms Tool".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
    engine
        .review_proposal(None, pid, "merge".into(), None, None, VerdictSurface::Console)
        .expect("console verdict");

    let g = engine.graph(Some(WS)).expect("graph after mediation");
    let n = g.nodes.iter().find(|n| n.name == "cozo").expect("node");
    assert_eq!(n.kind, "Tool", "the confirmed side must win by tier (R5)");
    assert!(!n.contested, "trust decided - no longer contested (R6)");
    assert_eq!(n.trust_tier, TrustTier::HumanConfirmed, "the node tier is the effective tier incl. the grant");
    assert_eq!(n.competitors.len(), 1, "the losing value stays queryable (R7)");
    assert_eq!(n.competitors[0].value, "Library");
    let cur = engine.curation(Some(WS)).expect("curation after mediation");
    assert_eq!(cur.stats.contradictions, 1, "resolved-by-trust conflicts stay listed (R7)");
    assert!(!cur.contradictions[0].contested, "but no longer invite mediation");
}

/// guard (resolution.md Section 6, R8): the same promotion merged through the AGENT surface grants
/// at most host_signed - an agent cannot mint a human's direct act (P18). The grant still settles
/// the tie (host_signed beats agent_extracted), but the tier stops below human_confirmed.
#[test]
fn p18_agent_surface_promotion_caps_at_host_signed() {
    let (store, engine) = engine();
    let o1 = kind_obs("cozo", "Tool", 100);
    let id1 = o1.id.clone();
    store.add_observation(o1).unwrap();
    store.add_observation(kind_obs("cozo", "Library", 200)).unwrap();
    engine.reproject(Some(WS)).expect("reproject");

    let pid = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![id1],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("some-agent".into()),
        })
        .expect("propose");
    engine
        .review_proposal(None, pid, "merge".into(), None, None, VerdictSurface::Agent)
        .expect("agent verdict");

    let g = engine.graph(Some(WS)).expect("graph");
    let n = g.nodes.iter().find(|n| n.name == "cozo").expect("node");
    assert_eq!(n.kind, "Tool", "the promoted side still wins the tie");
    assert!(!n.contested);
    assert_eq!(
        n.trust_tier,
        TrustTier::HostSigned,
        "an agent-surface grant must cap at host_signed - never human_confirmed (R8)"
    );
}

/// guard (resolution.md R5, proposal-workflow.md Section 9 fast-path): a merged demotion pushes
/// the target BELOW its base evaluation - the gate overrides in both directions, so demoting the
/// recency winner flips the belief to the surviving side.
#[test]
fn p23_demotion_overrides_below_base() {
    let (store, engine) = engine();
    store.add_observation(kind_obs("cozo", "Tool", 100)).unwrap();
    let o2 = kind_obs("cozo", "Library", 200);
    let id2 = o2.id.clone();
    store.add_observation(o2).unwrap();
    engine.reproject(Some(WS)).expect("reproject");

    let pid = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_demotion".into(),
            targets: vec![id2],
            into: None,
            tier: Some("unverified".into()),
            rationale: Some("wrong extraction".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
    engine
        .review_proposal(None, pid, "merge".into(), None, None, VerdictSurface::Console)
        .expect("verdict");

    let g = engine.graph(Some(WS)).expect("graph");
    let n = g.nodes.iter().find(|n| n.name == "cozo").expect("node");
    assert_eq!(n.kind, "Tool", "demoting the recency winner must flip the belief (R5)");
    assert!(!n.contested, "tiers differ now - not contested");
}

/// guard (resolution.md Section 2.2; the architecture.md Section 14 latent condition): the
/// representative spelling is the policy's choice over the log, not first-write-wins by arrival -
/// two stores fed the same observations in opposite orders re-materialize the same canonical_name.
#[test]
fn p16_canonical_name_selection_is_arrival_order_free() {
    let name_obs = |spelling: &str, observed_at: u64| {
        Observation::with_assertions(
            format!("mentions {spelling} at {observed_at}"),
            Provenance {
                host: "host-a".into(),
                on_behalf_of: None,
                workspace: WS.into(),
                source_ref: None,
                observed_at,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions {
                entities: vec![EntityAssertion { name: spelling.into(), kind: None, description: None }],
                ..Default::default()
            },
        )
    };
    let build = |order: [&Observation; 2]| {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "host-a", WS);
        for o in order {
            store.add_observation(o.clone()).unwrap();
        }
        engine.reproject(Some(WS)).expect("reproject");
        store.get_entity(&Entity::make_id(WS, "driver")).unwrap().expect("entity").canonical_name
    };
    let a = name_obs("driver", 100);
    let b = name_obs("Driver", 200);
    let n1 = build([&a, &b]);
    let n2 = build([&b, &a]);
    assert_eq!(n1, n2, "spelling selection must be arrival-order independent (P16)");
    assert_eq!(n1, "Driver", "the policy picks the tier/HLC winner, not the first arrival");
}

// --- P23 / I9: self-approval and the self-attested marker --------------------------------------

/// characterization (proposal-workflow.md I9, architecture.md Section 14 deferral): the fold
/// hardcodes `self_attested: true` on every proposal view, even when the reviewing principal
/// differs from the proposer, and self-approval is not prohibited. This is the documented solo-
/// mode interim. When I9 lands, this test MUST be rewritten: the marker must be computed from the
/// proposer/reviewer delegation chains (alice-proposed + bob-merged => self_attested false), and
/// alice-proposed + alice-merged must be blocked for non-demotion kinds in shared workspaces.
#[test]
fn i9_self_attested_is_blanket_true_until_principal_check_lands() {
    let (_store, engine) = engine();
    let (x, y) = mergeable_pair(&engine);
    let p = propose_merge(&engine, &[&x, &y], &y, "alice");
    review(&engine, &p, "merge", "bob"); // distinct principal reviews...
    let props = engine.list_proposals(Some(WS)).expect("list");
    assert_eq!(props[0].state, "merged");
    assert!(
        props[0].self_attested,
        "current interim: the marker is a blanket true; if this fails, I9 has landed - \
         rewrite this test to assert the computed marker semantics"
    );
}

// --- P18 / F13: claimed tier - verbatim in the log, evaluated on read --------------------------

/// guard of the LOG layer (F13, resolution.md Section 3): a peer's self-declared tier crosses the
/// wire and is stored VERBATIM - since M3a this is by design (the claim is log data, kept for
/// audit; rewriting it would violate P3). What changed in M3a is the READ path: every surface
/// consumes the receiver's EVALUATION, which caps a wire claim at host_signed - see the sibling
/// guard below. Phase 5 adds canon-policy-based evaluation on top; the stored claim never changes.
#[test]
fn f13_sync_apply_stores_senders_self_declared_tier_verbatim() {
    let store_a = InMemoryStore::new();
    let node_a = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([7u8; 32]));

    // The "malicious peer" self-declares the highest tier on its own observation.
    let obs = Observation::with_assertions(
        "peer-asserted claim".into(),
        Provenance {
            host: "peer-host".into(),
            on_behalf_of: Some("mallory".into()),
            workspace: WS.into(),
            source_ref: None,
            observed_at: 100,
            confidence: None,
            trust_tier: TrustTier::HumanConfirmed, // self-declared, unverified by the receiver
            sync: None,
        },
        Assertions::default(),
    );
    let id = obs.id.clone();
    store_a.add_observation(obs).expect("add");
    node_a.backfill(&store_a, WS).expect("backfill");
    let events =
        export_delta(&store_a, WS, &VersionVector::default(), &[WS.to_string()]).expect("export");
    assert_eq!(events.len(), 1);

    let store_b = InMemoryStore::new();
    let node_b = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([8u8; 32]));
    let keys: BTreeMap<String, String> =
        [(node_a.node_id().to_string(), node_a.public_key_hex())].into();
    let mut vv = VersionVector::default();
    let report = node_b.apply(&store_b, WS, events, &keys, &mut vv).expect("apply");
    assert_eq!(report.accepted, 1, "the signed event itself is valid and lands");

    let got = store_b.get_observation(&id).expect("get").expect("present");
    assert_eq!(
        got.provenance[0].trust_tier,
        TrustTier::HumanConfirmed,
        "the sender's claim is log data and stays verbatim (audit, F13) - evaluation is read-time"
    );
}

/// guard (resolution.md Section 3, R4; the read-path repayment of architecture.md Section 14
/// overdue entry condition 2): the same self-declared human_confirmed claim, read through the
/// engine's projections, evaluates to at most host_signed - a signature proves origin, never a
/// human act, so a wire claim cannot raise the displayed/believed tier on the receiver.
#[test]
fn f13_read_path_evaluates_remote_claim_at_host_signed() {
    let store_a = InMemoryStore::new();
    let node_a = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([7u8; 32]));
    let obs = Observation::with_assertions(
        "peer-asserted claim".into(),
        Provenance {
            host: "peer-host".into(),
            on_behalf_of: Some("mallory".into()),
            workspace: WS.into(),
            source_ref: None,
            observed_at: 100,
            confidence: None,
            trust_tier: TrustTier::HumanConfirmed, // self-declared
            sync: None,
        },
        Assertions {
            entities: vec![EntityAssertion {
                name: "claimed-node".into(),
                kind: None,
                description: None,
            }],
            ..Default::default()
        },
    );
    store_a.add_observation(obs).expect("add");
    node_a.backfill(&store_a, WS).expect("backfill");
    let events =
        export_delta(&store_a, WS, &VersionVector::default(), &[WS.to_string()]).expect("export");

    let store_b = Arc::new(InMemoryStore::new());
    let node_b = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([8u8; 32]));
    let keys: BTreeMap<String, String> =
        [(node_a.node_id().to_string(), node_a.public_key_hex())].into();
    let mut vv = VersionVector::default();
    node_b.apply(store_b.as_ref(), WS, events, &keys, &mut vv).expect("apply");

    let engine_b = Engine::new(store_b, "host-b", WS);
    engine_b.reproject(Some(WS)).expect("reproject");
    let g = engine_b.graph(Some(WS)).expect("graph");
    let n = g.nodes.iter().find(|n| n.name == "claimed-node").expect("node");
    assert_eq!(
        n.trust_tier,
        TrustTier::HostSigned,
        "a remote human_confirmed claim must evaluate to host_signed on the receiver (R4)"
    );
}

// --- P16 / F5 / F7: partitioned + duplicated delivery converges --------------------------------

/// guard (principles.md P16, federation.md F5/F7 and Prop A/C): the same authored event set,
/// delivered whole to one node and in reversed partitions WITH a duplicated batch to another,
/// must yield the identical version vector, the identical log, and the identical re-materialized
/// graph. This extends the existing exchange-order test with partition/duplication injection
/// (the partition half of the P16 property-test obligation).
#[test]
fn p16_partitioned_and_duplicated_delivery_converges() {
    // Author four observations on node A through the real ingest path.
    let store_a = Arc::new(InMemoryStore::new());
    let engine_a = Engine::new(store_a.clone(), "host-a", WS);
    observe(&engine_a, "fact one", &["kernel", "driver"], vec![]);
    observe(
        &engine_a,
        "fact two",
        &["driver"],
        vec![RelationInput {
            from: "driver".into(),
            kind: "depends_on".into(),
            to: "kernel".into(),
            description: None,
            valid_from: None,
            valid_to: None,
        }],
    );
    observe(&engine_a, "fact three", &["scheduler"], vec![]);
    observe(&engine_a, "fact four", &["kernel", "scheduler"], vec![]);

    let node_a = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([1u8; 32]));
    node_a.backfill(&*store_a, WS).expect("backfill");
    let all =
        export_delta(&*store_a, WS, &VersionVector::default(), &[WS.to_string()]).expect("export");
    assert_eq!(all.len(), 4);
    let keys: BTreeMap<String, String> =
        [(node_a.node_id().to_string(), node_a.public_key_hex())].into();

    // Node B: everything in one batch.
    let store_b = Arc::new(InMemoryStore::new());
    let node_b = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([2u8; 32]));
    let mut vv_b = VersionVector::default();
    node_b.apply(&*store_b, WS, all.clone(), &keys, &mut vv_b).expect("apply b");

    // Node C: second half first, then the second half AGAIN (relay duplicate), then the first.
    let store_c = Arc::new(InMemoryStore::new());
    let node_c = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([3u8; 32]));
    let mut vv_c = VersionVector::default();
    let (first, second) = all.split_at(2);
    node_c.apply(&*store_c, WS, second.to_vec(), &keys, &mut vv_c).expect("apply c1");
    node_c.apply(&*store_c, WS, second.to_vec(), &keys, &mut vv_c).expect("apply c2 dup");
    node_c.apply(&*store_c, WS, first.to_vec(), &keys, &mut vv_c).expect("apply c3");

    // Identical version vectors (F7) - both as advanced and as re-derived from the store.
    assert_eq!(version_vector(&*store_b, WS).unwrap(), version_vector(&*store_c, WS).unwrap());

    // Identical logs: same ids, same attestation counts (P3: the duplicate deduped, nothing lost).
    let shape = |s: &InMemoryStore| -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = s
            .all_observations(Some(WS))
            .unwrap()
            .into_iter()
            .map(|o| (o.id, o.provenance.len()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(shape(&store_b), shape(&store_c), "same log regardless of partitioning");

    // Identical re-materialized graphs (F5 / Prop C).
    let engine_b = Engine::new(store_b.clone(), "host-b", WS);
    let engine_c = Engine::new(store_c.clone(), "host-c", WS);
    engine_b.reproject(Some(WS)).expect("reproject b");
    engine_c.reproject(Some(WS)).expect("reproject c");
    assert_eq!(graph_shape(&engine_b), graph_shape(&engine_c), "same graph regardless of path");
}

// --- P16 (4th revision): read-path reproducibility with ties -----------------------------------

/// guard (principles.md P16 "query responses must be deterministic too"): keyword hits that tie
/// on score must be ordered by the stable key (id), and the whole response must be identical on
/// repeat - the iteration order of an internal map must never leak into the response.
#[test]
fn p16_search_ties_break_by_id_and_repeat_identically() {
    let (_store, engine) = engine();
    observe(&engine, "note one", &["tie alpha"], vec![]);
    observe(&engine, "note two", &["tie beta"], vec![]);
    observe(&engine, "note three", &["tie gamma"], vec![]);

    let run = || {
        engine
            .search("tie", Some(WS), 10)
            .expect("search")
            .hits
            .into_iter()
            .map(|h| (h.id, h.score))
            .collect::<Vec<_>>()
    };
    let first = run();
    assert!(first.len() >= 3, "all three tied entities recalled: {first:?}");
    assert_eq!(first, run(), "identical response on repeat (reproducibility)");

    // Among equal scores, ids must be ascending (the pinned tie-break).
    for w in first.windows(2) {
        if (w[0].1 - w[1].1).abs() < f32::EPSILON {
            assert!(w[0].0 < w[1].0, "tied hits must be id-ordered: {first:?}");
        }
    }
}

// --- P8 / P3: a description is never erased by omission ----------------------------------------

/// guard (principles.md P8 capture, P3 no destructive overwrite): a re-observation that omits the
/// description must not erase the one already captured; a supplied kind updates (LWW among
/// suppliers), an omitted kind leaves the previous one.
#[test]
fn p8_description_survives_reobservation_without_one() {
    let (_store, engine) = engine();
    engine
        .observe(ObserveInput {
            content: "gizmo is the daemon".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "gizmo".into(),
                kind: None,
                description: Some("the background daemon".into()),
            }],
            relations: vec![],
        })
        .expect("observe 1");
    // Re-observation: no description, but a kind this time.
    engine
        .observe(ObserveInput {
            content: "gizmo restarted".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "gizmo".into(),
                kind: Some("Tool".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe 2");

    let view = engine
        .get_entity(&Entity::make_id(WS, "gizmo"))
        .expect("lookup")
        .expect("present");
    assert_eq!(
        view.entity.description.as_deref(),
        Some("the background daemon"),
        "omission must not erase a captured description"
    );
    assert_eq!(view.entity.kind, "Tool", "a supplied kind updates");
    assert_eq!(view.entity.provenance.len(), 2, "both observations attested (P2/P3)");
}

// --- P5: absence is unknown, not an error ------------------------------------------------------

/// guard (principles.md P5 open world): looking up an id nothing was ever asserted about is a
/// well-formed answer (None), not a store failure.
#[test]
fn p5_absent_entity_is_none_not_error() {
    let (_store, engine) = engine();
    let got = engine.get_entity("no-such-id").expect("absence must not be an Err");
    assert!(got.is_none(), "absence is None (unknown), never fabricated");
}


// --- P15 / P11: hyperedge management - forwarding hygiene and the reify promotion path ---------

/// guard (Principle 15/14; the engine:hypergraph forwarding follow-up, now landed): hyperedge
/// membership resolves through accepted entity-merges - two co-occurrence sets that differ only
/// by a merged-away spelling collapse into ONE hyperedge (member set = identity), their sources
/// accumulate (P3), and the merged-away row leaves the node set.
#[test]
fn p15_hypergraph_membership_forwards_accepted_merges() {
    let (_store, engine) = engine();
    observe(&engine, "cozo with rust", &["cozo", "rust"], vec![]);
    observe(&engine, "cozodb with rust", &["cozodb", "rust"], vec![]);
    let a = Entity::make_id(WS, "cozodb");
    let b = Entity::make_id(WS, "cozo");
    let before = engine.hypergraph(Some(WS)).expect("hypergraph");
    assert_eq!(before.hyperedges.len(), 2, "distinct spellings start as distinct contexts");

    let p = propose_merge(&engine, &[a.as_str(), b.as_str()], &b, "alice");
    review(&engine, &p, "merge", "alice");

    let after = engine.hypergraph(Some(WS)).expect("hypergraph after merge");
    assert_eq!(after.hyperedges.len(), 1, "canonicalized member sets must union into one hyperedge");
    assert_eq!(after.hyperedges[0].sources, 2, "both co-assertions accumulate (P3)");
    assert!(after.hyperedges[0].members.contains(&b), "membership forwards to the canonical id");
    assert!(!after.nodes.iter().any(|n| n.id == a), "the merged-away row leaves the node set");
    // The curation grab-bag path shares the projection, so it sees the same canon (no re-check
    // needed here) - and the graph and hypergraph node sets now agree.
    let g = engine.graph(Some(WS)).expect("graph");
    let mut gn: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut hn: Vec<&str> = after.nodes.iter().map(|n| n.id.as_str()).collect();
    gn.sort();
    hn.sort();
    assert_eq!(gn, hn, "graph and hypergraph node sets must agree after forwarding");
}

/// guard (Principle 11 promotion path, P18 lineage): reifying a hyperedge asserts a group entity
/// plus member_of relations as an ordinary observation whose derived_from names every
/// co-asserting observation - the grouping becomes first-class (managed like any edge) while the
/// hyperedge stays a derived view. The reified claim enters at the default tier (promotion is a
/// separate, gated act).
#[test]
fn p11_reify_asserts_group_with_lineage() {
    let (_store, engine) = engine();
    let o1 = engine
        .observe(ObserveInput {
            content: "kernel loads driver".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "kernel".into(), kind: None, description: None },
                EntityInput { name: "driver".into(), kind: None, description: None },
            ],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let o2 = engine
        .observe(ObserveInput {
            content: "driver runs in kernel space".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "kernel".into(), kind: None, description: None },
                EntityInput { name: "driver".into(), kind: None, description: None },
            ],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let hg = engine.hypergraph(Some(WS)).expect("hypergraph");
    assert_eq!(hg.hyperedges.len(), 1);
    assert_eq!(hg.hyperedges[0].sources, 2);
    let hid = hg.hyperedges[0].id.clone();

    let out = engine
        .reify_hyperedge(supragnosis_engine::ReifyInput {
            workspace: None,
            hyperedge: hid,
            name: Some("boot stack".into()),
            kind: None,
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("reify");
    // Lineage: the reified assertion derives from BOTH co-asserting observations (P18).
    let reified = engine.get_observation(&out.observation_id).expect("get").expect("present");
    let mut lineage = reified.derived_from.clone();
    lineage.sort();
    let mut expect = vec![o1, o2];
    expect.sort();
    assert_eq!(lineage, expect, "derived_from must name every co-asserting observation");
    // The grouping is now first-class: a Context node + member_of edges in the graph.
    let g = engine.graph(Some(WS)).expect("graph");
    let group = g.nodes.iter().find(|n| n.name == "boot stack").expect("group node");
    assert_eq!(group.kind, "Context");
    let member_edges: Vec<_> =
        g.edges.iter().filter(|e| e.kind == "member_of" && e.to == group.id).collect();
    assert_eq!(member_edges.len(), 2, "each member gains a member_of edge into the group");
    // Default tier: reified knowledge starts unprivileged; promotion is a separate gated act.
    assert_eq!(group.trust_tier, TrustTier::AgentExtracted);
    // An unknown hyperedge id is a self-correctable error, not a silent no-op (P21).
    assert!(engine
        .reify_hyperedge(supragnosis_engine::ReifyInput {
            workspace: None,
            hyperedge: "nope".into(),
            name: None,
            kind: None,
            source_ref: None,
            on_behalf_of: None,
        })
        .is_err());
}

// --- M3b / Principle 3/16: alias accumulation, IR3 (incremental == replay), IR4 -----------------

/// An observe of a single named entity with a given spelling, kind, and no relations.
fn observe_named(engine: &Engine, content: &str, name: &str) {
    engine
        .observe(ObserveInput {
            content: content.into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
            relations: vec![],
        })
        .expect("observe");
}

/// guard (resolution-identity.md Section 2, IR1): distinct asserted spellings of one entity (they
/// share an id under case/trim normalization) accumulate as aliases minus the representative, the
/// set never drops a spelling, and it is arrival-order independent.
#[test]
fn aliases_accumulate_and_converge() {
    let build = |order: [&str; 3]| {
        let (store, engine) = engine();
        for (i, sp) in order.iter().enumerate() {
            observe_named(&engine, &format!("mention {i}"), sp);
            // Force distinct ordering HLCs. Without a gap all three can share a millisecond, tie on
            // HLC, and fall through to the id tiebreak - which is a different code path than the one
            // under test here, and the source of this test's former ~1-in-8 flake.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let e = store.get_entity(&Entity::make_id(WS, "driver")).unwrap().expect("entity");
        let mut spellings = e.aliases.clone();
        spellings.push(e.canonical_name.clone());
        spellings.sort();
        (e.canonical_name, spellings)
    };
    let a = build(["Driver", "driver", "DRIVER"]);
    let b = build(["DRIVER", "Driver", "driver"]);

    // Order-independent, and the actual IR1/P3 guarantee: the union of spellings. Nothing is dropped
    // and which spellings survive never depends on the order they arrived in.
    assert_eq!(a.1, b.1, "the set of spellings kept must not depend on arrival order");
    assert_eq!(a.1, vec!["DRIVER", "Driver", "driver"], "every spelling is kept");

    // Order-DEPENDENT by policy, and deliberately asserted as such: `TierWeighted` selects the latest
    // ordering HLC within the top tier band, so the representative tracks the last spelling seen.
    // These two builds are two DIFFERENT logs (the content differs per position), and P16 promises
    // convergence over the same observation set - not invariance to the order one node happened to
    // see things in. Two nodes that each saw one of these orders converge once they exchange logs,
    // because they then hold the same six observations and the HLCs travel with them.
    assert_eq!(a.0, "DRIVER", "order a ends on DRIVER, so recency selects it");
    assert_eq!(b.0, "driver", "order b ends on driver, so recency selects it");
}

/// guard (resolution-identity.md Section 4, IR3): the incremental projection of the last write
/// equals what a fresh reproject would produce - the two paths run the same fold, so interleaved
/// observes and a full replay agree on kind, canonical_name, aliases, and provenance count.
#[test]
fn incremental_write_equals_replay() {
    let (store, engine) = engine();
    // A mix: kind conflict, spelling variants, a relation endpoint, a re-observation.
    observe(&engine, "a", &["Kernel"], vec![]);
    engine
        .observe(ObserveInput {
            content: "kernel is a component".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { name: "kernel".into(), kind: Some("Component".into()), description: None }],
            relations: vec![],
        })
        .unwrap();
    observe(
        &engine,
        "driver runs on kernel",
        &["Driver"],
        vec![RelationInput {
            from: "Driver".into(),
            kind: "runs_on".into(),
            to: "Kernel".into(),
            description: None,
            valid_from: None,
            valid_to: None,
        }],
    );
    let snapshot = |store: &InMemoryStore| {
        let mut rows: Vec<(String, String, String, Vec<String>, usize)> = store
            .all_entities(Some(WS))
            .unwrap()
            .into_iter()
            .map(|e| {
                let mut al = e.aliases.clone();
                al.sort();
                (e.id, e.canonical_name, e.kind, al, e.provenance.len())
            })
            .collect();
        rows.sort();
        rows
    };
    let incremental = snapshot(&store);
    engine.reproject(Some(WS)).expect("reproject");
    let replayed = snapshot(&store);
    assert_eq!(incremental, replayed, "incremental write must equal a fresh replay (IR3)");
}

/// guard (resolution-identity.md Section 5, IR4): the stored entity embedding always corresponds to
/// the current embedding text (canonical_name + aliases) - never silently stale. Checked directly:
/// the stored vector equals a fresh embed of the row's current name+aliases text, after each write.
#[test]
fn embedding_recomputed_on_alias_change() {
    use supragnosis_core::EmbeddingProvider;
    use supragnosis_embed::HashingEmbedder;
    let embedder = HashingEmbedder::default();
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS)
        .with_embedder(Arc::new(HashingEmbedder::default()));
    // The embedding text is canonical_name + aliases (the engine's entity_text), joined by spaces.
    let text_of = |e: &supragnosis_core::Entity| {
        if e.aliases.is_empty() {
            e.canonical_name.clone()
        } else {
            format!("{} {}", e.canonical_name, e.aliases.join(" "))
        }
    };
    let corresponds = |store: &InMemoryStore| {
        let e = store.get_entity(&Entity::make_id(WS, "driver")).unwrap().unwrap();
        let stored = e.embedding.clone().expect("embedded");
        let fresh = embedder.embed_one(&text_of(&e)).unwrap();
        assert_eq!(stored, fresh, "the stored embedding must match the current name+aliases text (IR4)");
    };
    observe_named(&engine, "one", "driver");
    corresponds(&store);
    // A new spelling accumulates as an alias, changing the row's text; the embedding must still
    // correspond (recomputed, not stale).
    observe_named(&engine, "two", "DRIVER-X"); // distinct id, keep "driver" separate
    observe_named(&engine, "three", "Driver");
    corresponds(&store);
}

/// guard (Principle 14): a merged-away entity id keeps forwarding - get_entity by the old id
/// dereferences to the surviving canonical entity, whose alias set now includes the merged name.
#[test]
fn get_entity_forwards_a_merged_id() {
    let (_store, engine) = engine();
    observe_named(&engine, "one", "cozo");
    observe_named(&engine, "two", "cozodb");
    let a = Entity::make_id(WS, "cozodb");
    let b = Entity::make_id(WS, "cozo");
    let p = propose_merge(&engine, &[a.as_str(), b.as_str()], &b, "alice");
    review(&engine, &p, "merge", "alice");
    // Look up the merged-away id -> the canonical entity, with the merged name among its aliases.
    let view = engine.get_entity(&a).unwrap().expect("forwards to canonical");
    assert_eq!(view.entity.id, b, "the merged-away id dereferences to the canonical id (P14)");
    assert!(
        view.entity.aliases.iter().any(|al| al == "cozodb"),
        "the merged-away name surfaces as an alias: {:?}",
        view.entity.aliases
    );
}

// --- M3b / Principle 15/19: the conservative merge band generates, never commits (IR2) ---------

/// guard (resolution-identity.md Section 3, IR2): embedding-near distinct-name entities surface as
/// merge SUGGESTIONS (a node-local recall aid) - but a suggestion commits nothing: no proposal, no
/// verdict, the rows are untouched. An open entity_merge for the pair suppresses it (in flight).
#[test]
fn merge_suggestions_never_commit() {
    use supragnosis_embed::HashingEmbedder;
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS)
        .with_embedder(Arc::new(HashingEmbedder::default()));
    let prov = || Provenance {
        host: "host-a".into(),
        on_behalf_of: None,
        workspace: WS.into(),
        source_ref: None,
        observed_at: 1,
        confidence: None,
        trust_tier: TrustTier::default(),
        sync: None,
    };
    let ent = |name: &str, emb: Vec<f32>| supragnosis_core::Entity {
        id: Entity::make_id(WS, name),
        kind: "Concept".into(),
        canonical_name: name.into(),
        aliases: vec![],
        description: None,
        properties: serde_json::Value::Null,
        provenance: vec![prov()],
        embedding: Some(emb),
    };
    // Two distinct entities with identical embeddings -> cosine 1.0 -> a candidate; a third,
    // orthogonal -> below the band.
    store.put_entity(ent("cozo", vec![1.0, 0.0, 0.0])).unwrap();
    store.put_entity(ent("cozodb", vec![1.0, 0.0, 0.0])).unwrap();
    store.put_entity(ent("kernel", vec![0.0, 1.0, 0.0])).unwrap();

    let rep = engine.curation(Some(WS)).unwrap();
    assert_eq!(rep.stats.merge_suggestions, 1, "the near pair is suggested, the orthogonal one is not");
    let s = &rep.merge_suggestions[0];
    assert!((s.similarity - 1.0).abs() < 1e-6, "similarity carried for ranking");
    let mut names = vec![s.a_name.clone(), s.b_name.clone()];
    names.sort();
    assert_eq!(names, vec!["cozo".to_string(), "cozodb".to_string()]);

    // IR2: a suggestion is not a commit - no proposal exists and the entities are untouched.
    assert!(engine.list_proposals(Some(WS)).unwrap().is_empty(), "a suggestion is not a proposal");
    assert!(store.get_entity(&Entity::make_id(WS, "cozo")).unwrap().is_some());
    assert!(store.get_entity(&Entity::make_id(WS, "cozodb")).unwrap().is_some());

    // Opening an entity_merge for the pair takes it out of the band (it is now under review).
    let a = Entity::make_id(WS, "cozo");
    let b = Entity::make_id(WS, "cozodb");
    engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![a, b.clone()],
            into: Some(b),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("alice".into()),
        })
        .unwrap();
    let rep2 = engine.curation(Some(WS)).unwrap();
    assert_eq!(rep2.stats.merge_suggestions, 0, "an open entity_merge suppresses the suggestion");
}

// --- M3b / Principle 9 vs 6: T-Box conflict surfacing (IR5) + axis collision --------------------

/// A type-definition observation (target/name = definition) at a fixed transaction time.
fn typedef_obs(
    target: supragnosis_core::TypeTarget,
    name: &str,
    desc: &str,
    observed_at: u64,
) -> Observation {
    Observation::with_assertions(
        format!("type {name} = {desc} @ {observed_at}"),
        Provenance {
            host: "host-a".into(),
            on_behalf_of: None,
            workspace: WS.into(),
            source_ref: None,
            observed_at,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        },
        Assertions {
            type_defs: vec![supragnosis_core::TypeDefAssertion {
                target,
                name: name.into(),
                description: desc.into(),
            }],
            ..Default::default()
        },
    )
}

/// guard (resolution-identity.md Section 6, IR5): distinct definitions of one type at a tied top
/// effective tier surface as contested on the glossary - the SAME contested/competitor shape as an
/// entity kind - and a console promotion of the chosen definition settles it by trust.
#[test]
fn type_def_conflict_surfaces_contested() {
    use supragnosis_core::TypeTarget;
    let (store, engine) = engine();
    store.add_observation(typedef_obs(TypeTarget::Entity, "Driver", "a kernel module", 100)).unwrap();
    store.add_observation(typedef_obs(TypeTarget::Entity, "Driver", "a person who drives", 200)).unwrap();

    let types = engine.types(Some(WS)).unwrap();
    let d = types.iter().find(|t| t.name == "Driver").expect("type present");
    assert!(d.contested, "distinct definitions at a tied tier must be contested (IR5)");
    assert_eq!(d.description, "a person who drives", "recency wins within the tied tier (R2)");
    assert_eq!(d.competitors.len(), 1);
    assert_eq!(d.competitors[0].value, "a kernel module");
    assert!(d.def_source.is_some(), "the winning definition's observation is the mediation handle");

    // Mediation: confirm the kernel-module definition (promote its observation, console surface).
    let target = d.competitors[0].observation.clone();
    let pid = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![target],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .unwrap();
    engine
        .review_proposal(None, pid, "merge".into(), None, None, VerdictSurface::Console)
        .unwrap();

    let d2 = engine.types(Some(WS)).unwrap().into_iter().find(|t| t.name == "Driver").unwrap();
    assert_eq!(d2.description, "a kernel module", "the confirmed definition wins by tier (R5)");
    assert!(!d2.contested, "trust decided - no longer contested");
    assert_eq!(d2.trust_tier, TrustTier::HumanConfirmed, "the glossary tier is the effective tier");
}

/// guard (resolution-identity.md Section 6, Principle 9 minimal): a name defined on both the entity
/// and the relation axis surfaces as a curation signal (informative, not blocking).
#[test]
fn type_axis_collision_is_a_signal() {
    use supragnosis_core::TypeTarget;
    let (store, engine) = engine();
    store.add_observation(typedef_obs(TypeTarget::Entity, "member_of", "a membership entity", 100)).unwrap();
    store.add_observation(typedef_obs(TypeTarget::Relation, "member_of", "belongs to a group", 100)).unwrap();
    store.add_observation(typedef_obs(TypeTarget::Entity, "Driver", "a kernel module", 100)).unwrap();

    let rep = engine.curation(Some(WS)).unwrap();
    assert_eq!(
        rep.type_axis_collisions,
        vec!["member_of".to_string()],
        "a name on both axes is flagged; a single-axis name is not"
    );
    assert_eq!(rep.stats.type_axis_collisions, 1);
}

/// Guard (Principle 1 / resolution.md): explain_entity is an explanation OF the projection, not a
/// second computation. Its winning name/kind equal get_entity's and the graph node's; the
/// non-winning kind surfaces as a competitor; a contested kind (two distinct kinds tie at the top
/// tier) is reported contested. observation_log's entity filter is the same evidence set.
#[test]
fn explain_matches_projection_and_surfaces_competitors() {
    let (_store, engine) = engine();
    let obs = |content: &str, name: &str, kind: &str| {
        engine
            .observe(ObserveInput {
                content: content.into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { name: name.into(), kind: Some(kind.into()), description: None }],
                relations: vec![],
            })
            .expect("observe");
    };
    // Same entity id ("Firefox"/"firefox" normalize together): two conflicting kinds + a case-variant
    // spelling - all AgentExtracted, so the kinds tie at the top tier (contested).
    obs("firefox is a browser", "Firefox", "Browser");
    obs("firefox the application", "firefox", "Application");

    let id = Entity::make_id(WS, "Firefox");
    let view = engine.get_entity(&id).expect("get_entity").expect("entity exists");
    let ex = engine.explain_entity(&id).expect("explain_entity").expect("entity exists");

    assert_eq!(ex.id, view.entity.id);
    // The graph node agrees with get_entity (both are the projection).
    let node_kind = engine
        .graph(Some(WS))
        .expect("graph")
        .nodes
        .into_iter()
        .find(|n| n.id == id)
        .expect("node present")
        .kind;
    assert_eq!(node_kind, view.entity.kind, "graph node kind == get_entity kind");

    let kind_field = ex.fields.iter().find(|f| f.field == "kind").expect("kind field");
    assert_eq!(kind_field.winner, view.entity.kind, "explain winner == projected kind");
    assert!(kind_field.contested, "two distinct kinds tie at the top tier -> contested");
    let winners: Vec<&str> =
        kind_field.candidates.iter().filter(|c| c.role == "winner").map(|c| c.value.as_str()).collect();
    let competitors: Vec<&str> =
        kind_field.candidates.iter().filter(|c| c.role == "competitor").map(|c| c.value.as_str()).collect();
    assert_eq!(winners, vec![view.entity.kind.as_str()], "exactly the projected kind is the winner row");
    assert_eq!(competitors.len(), 1, "the other kind is a competitor");
    assert_ne!(competitors[0], view.entity.kind, "the competitor is not the winner");

    // canonical_name winner is the projected name; the case-variant spelling is an alias.
    let name_field = ex.fields.iter().find(|f| f.field == "canonical_name").expect("name field");
    assert_eq!(name_field.winner, view.entity.canonical_name);
    assert!(
        name_field.candidates.iter().any(|c| c.role == "alias"),
        "the case-variant spelling is an alias"
    );

    // Supporting log = exactly the observations touching this entity (both), newest-first.
    assert_eq!(ex.supporting.len(), 2, "both observations back this entity");
    assert!(ex.supporting[0].hlc >= ex.supporting[1].hlc, "supporting log is newest-first");

    // observation_log entity filter narrows to the same set; unfiltered returns everything.
    assert_eq!(engine.observation_log(Some(WS), Some(&id), None).expect("log filtered").len(), 2);
    assert_eq!(engine.observation_log(Some(WS), None, None).expect("log all").len(), 2);
}

// --- M3.5b: blocking-check monotonicity -------------------------------------------------------

/// guard (proposal-workflow.md Section 6 monotonicity note, Section 13 open decisions; P16/I8):
/// the blocking gate's conclusion must be a function of the event SET, not of the order or
/// partitioning in which the events arrived. The spec asks for this to be verified continuously by
/// a property test rather than argued, because the failure it guards against is silent: a merge
/// that counted as valid on one node and not on another would make canon node-dependent.
///
/// It also pins the direction of change. A proposal whose targets have not arrived yet is `blocked`,
/// and becomes `merged` once they do - fail to pass. The reverse, a merge that later stops counting,
/// is what would break I16, and nothing here can produce it: entities are never removed.
#[test]
fn i8_blocking_check_conclusion_is_arrival_order_independent() {
    // Node A authors the whole story through the real ingest path: the entities, the proposal, the
    // verdict.
    let store_a = Arc::new(InMemoryStore::new());
    let engine_a = Engine::new(store_a.clone(), "host-a", WS);
    let (x, y) = mergeable_pair(&engine_a);
    let p = propose_merge(&engine_a, &[&x, &y], &y, "alice");
    review(&engine_a, &p, "merge", "bob");
    assert_eq!(
        engine_a.get_proposal(Some(WS), &p).unwrap().unwrap().state,
        "merged",
        "the authoring node must see a valid merge"
    );

    let node_a = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([1u8; 32]));
    node_a.backfill(&*store_a, WS).expect("backfill");
    let all =
        export_delta(&*store_a, WS, &VersionVector::default(), &[WS.to_string()]).expect("export");
    let keys: BTreeMap<String, String> =
        [(node_a.node_id().to_string(), node_a.public_key_hex())].into();

    // Deliver the same set to fresh nodes under different orders and partitions.
    let deliver = |batches: Vec<Vec<_>>, label: &str| -> String {
        let store = Arc::new(InMemoryStore::new());
        let node = SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([9u8; 32]));
        let mut vv = VersionVector::default();
        for b in batches {
            node.apply(&*store, WS, b, &keys, &mut vv).unwrap_or_else(|e| panic!("{label}: {e}"));
        }
        Engine::new(store, "host-x", WS)
            .get_proposal(Some(WS), &p)
            .expect("get")
            .expect("proposal replicated")
            .state
    };

    let mut reversed = all.clone();
    reversed.reverse();
    let one_at_a_time: Vec<Vec<_>> = reversed.iter().map(|e| vec![e.clone()]).collect();

    let whole = deliver(vec![all.clone()], "one batch");
    let backwards = deliver(vec![reversed.clone()], "reversed batch");
    let dribbled = deliver(one_at_a_time, "reversed, one event per batch");

    assert_eq!(whole, "merged", "the full event set is a valid merge");
    assert_eq!(
        (whole.as_str(), backwards.as_str(), dribbled.as_str()),
        ("merged", "merged", "merged"),
        "the conclusion must not depend on arrival order or partitioning"
    );

    // Direction: with the entity observation withheld the targets do not exist, so the same verdict
    // does NOT commit - and the fold says blocked rather than merged. This is the state a partially
    // synced node is in, and it is the safe direction (it can only later become merged).
    let without_entities: Vec<_> =
        all.iter().filter(|e| e.assertions.entities.is_empty()).cloned().collect();
    assert_eq!(without_entities.len(), all.len() - 1, "exactly the entity observation is withheld");
    assert_eq!(
        deliver(vec![without_entities], "targets withheld"),
        "blocked",
        "a merge whose targets are not in the local log must not count as merged"
    );
}
