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
    Assertions, Entity, KnowledgeStore, Observation, Provenance, TrustTier, VersionVector,
};
use supragnosis_engine::{Engine, EntityInput, ObserveInput, ProposeInput, RelationInput};
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
            rationale: None,
            source_ref: None,
            on_behalf_of: Some(principal.into()),
        })
        .expect("propose")
}

fn review(engine: &Engine, proposal: &str, decision: &str, principal: &str) {
    engine
        .review_proposal(None, proposal.into(), decision.into(), None, Some(principal.into()))
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
        let p = propose_merge(&engine, &["ent-x", "ent-y"], "ent-y", "alice");
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

/// characterization (P16 holds, P6 does not yet): two merged proposals folding a<->b into each
/// other are contradictory data. Today the fold resolves the cycle silently and deterministically
/// (hop-capped forwarding) instead of surfacing it as a contradiction signal (P6 "conflict is
/// information"). This test pins that the outcome is at least convergent (same events, either
/// ingest order -> the same graph shape) and that no entity is lost. When cycle detection lands
/// as a curation signal, extend this test to assert the signal instead.
#[test]
fn p6_contradictory_merge_cycle_is_convergent_but_silent() {
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
        vec![(g, b, "uses".to_string())],
        "current interim: the cycle rewires the edge to the parity-chosen side; if this fails, \
         cycle handling changed - if it now surfaces a contradiction signal, move this assert to \
         the curation report; if it became order-dependent, that is a P16 regression"
    );
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
    let p = propose_merge(&engine, &["ent-x", "ent-y"], "ent-y", "alice");
    review(&engine, &p, "merge", "bob"); // distinct principal reviews...
    let props = engine.list_proposals(Some(WS)).expect("list");
    assert_eq!(props[0].state, "merged");
    assert!(
        props[0].self_attested,
        "current interim: the marker is a blanket true; if this fails, I9 has landed - \
         rewrite this test to assert the computed marker semantics"
    );
}

// --- P18 / F13: the receiver does not yet re-evaluate a synced trust tier ----------------------

/// characterization (principles.md P18 "the tier is the receiver's evaluation", architecture.md
/// Section 14 overdue entry condition 2): a peer's self-declared tier crosses the wire verbatim.
/// A malicious peer could self-declare human_confirmed and the receiving store would hold it.
/// Bounded today by single-principal deployment + the origin-key allowlist. When receiver-side
/// re-evaluation lands (Phase 5), this test MUST flip: the stored tier must become the receiver's
/// own evaluation, with the sender's claim kept only as the sending node's record.
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
        "current interim: the sender's self-declared tier is stored verbatim; if this fails, \
         receiver-side re-evaluation has landed - rewrite this test to assert the receiver's \
         own evaluation and the demotion of the sender's claim to a record"
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

