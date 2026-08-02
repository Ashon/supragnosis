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
    evaluated_tier, Assertions, Entity, EntityAssertion, Hlc, AssertionStore, KnowledgeStore, Observation,
    Provenance, SyncMeta, TrustTier, VersionVector,
};
use supragnosis_engine::{
    DefineTypeInput, Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, TypeDefInput,
    TypeTarget, VerdictSurface,
};
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
    // iteration parity, not by any principled rule. So the graph shows an edge into an entity no
    // observation ever asserted an edge to - which is why the curation signal asserted at the end
    // of this test is the thing that makes the shape reviewable rather than merely deterministic.
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

/// guard (resolution.md Section 6, R8): the verdict-surface markers ride source_ref, and the
/// ceiling fold trusts a log-borne marker to be engine-stamped. That trust holds only if every
/// LOCAL ingest door refuses the namespace - review_proposal stamps its own marker and accepts no
/// source_ref, so it is the only local author. Without this, an ordinary observation could park
/// "surface:console" in the log and hand any future marker-reading fold a forged human act.
#[test]
fn p18_reserved_surface_namespace_is_refused_at_every_ingest_door() {
    let (_store, engine) = engine();
    let observe_with = |source_ref: Option<&str>, content: &str| {
        engine.observe(ObserveInput {
            content: content.into(),
            workspace: None,
            source_ref: source_ref.map(String::from),
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
    };

    let err = match observe_with(Some("surface:console"), "innocent text") {
        Ok(_) => panic!("observe must refuse the reserved namespace"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("reserved"), "the refusal must say why: {err}");

    let err = engine
        .define_type(DefineTypeInput {
            workspace: None,
            source_ref: Some("surface:agent".into()),
            on_behalf_of: None,
            defs: vec![TypeDefInput {
                target: TypeTarget::Entity,
                name: "Widget".into(),
                description: "a thing".into(),
            }],
        })
        .expect_err("define_type must refuse the reserved namespace");
    assert!(err.to_string().contains("reserved"), "{err}");

    // propose, with an otherwise fully valid gate proposal - the refusal must not depend on the
    // rest of the input being broken.
    let target = observe_with(None, "a fact worth promoting").expect("observe").observation_id;
    let err = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![target],
            into: None,
            tier: Some("host_signed".into()),
            rationale: None,
            affected_types: vec![],
            source_ref: Some("surface:web".into()),
            on_behalf_of: None,
        })
        .expect_err("propose must refuse the reserved namespace");
    assert!(err.to_string().contains("reserved"), "{err}");

    // The namespace is reserved, not source_ref itself - an ordinary reference stays welcome.
    observe_with(Some("file:///notes.md"), "sourced text").expect("a normal source_ref must pass");
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

    // Identical re-materialized graphs (F5 / Prop C). Compared WHOLE, not by shape: node ids and
    // edge triples agreeing says nothing about the belief values, contested flags, effective tiers,
    // aliases or edge metadata carried on them, and those are the parts a fold actually decides.
    // Shape-level comparison is what let an order-dependent duplicate-edge pick sit in `graph` while
    // the P16 suite was green. Serializing closes the "batch-partitioned delivery only checks graph
    // shape" sliver that architecture.md Section 14 recorded.
    let engine_b = Engine::new(store_b.clone(), "host-b", WS);
    let engine_c = Engine::new(store_c.clone(), "host-c", WS);
    engine_b.reproject(Some(WS)).expect("reproject b");
    engine_c.reproject(Some(WS)).expect("reproject c");
    assert_eq!(graph_shape(&engine_b), graph_shape(&engine_c), "same graph regardless of path");
    assert_eq!(
        serde_json::to_string(&engine_b.graph(Some(WS)).unwrap()).unwrap(),
        serde_json::to_string(&engine_c.graph(Some(WS)).unwrap()).unwrap(),
        "partitioned delivery must converge on the whole graph, not merely its shape"
    );
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

/// A clock that hands out a stated sequence of transaction times, so a test says what order the
/// assertions arrived in instead of sleeping until the machine agrees. `step` of 0 makes every
/// ingest land in the same millisecond - the tie that sends resolution to its final tiebreak.
struct ScriptedClock {
    start: supragnosis_core::Timestamp,
    step: supragnosis_core::Timestamp,
    calls: std::sync::atomic::AtomicU64,
}

impl ScriptedClock {
    fn new(start: u64, step: u64) -> Self {
        Self { start, step, calls: std::sync::atomic::AtomicU64::new(0) }
    }
}

impl supragnosis_core::Clock for ScriptedClock {
    fn now_millis(&self) -> supragnosis_core::Timestamp {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.start + n * self.step
    }
}

/// guard (resolution-identity.md Section 2, IR1): distinct asserted spellings of one entity (they
/// share an id under case/trim normalization) accumulate as aliases minus the representative, the
/// set never drops a spelling, and it is arrival-order independent.
///
/// The transaction time is injected rather than slept for. This test used to sleep 2ms between
/// ingests so the three observations would not share a millisecond, which is the wall-clock
/// dependence the P16 guards are otherwise free of - and before the sleep it failed roughly 1 run in
/// 8. Worse, the sleep bought determinism by steering around the tied-HLC branch, so the id tiebreak
/// this policy falls back to went untested. Both orderings are now stated outright, tie included.
#[test]
fn aliases_accumulate_and_converge() {
    // Content is derived from the SPELLING, not from the position, so the three observations are one
    // set that two nodes can see in different orders. With `mention {i}` they were three different
    // observations per order - two different logs - and P16 promises nothing about those.
    let build = |order: [&str; 3], step: u64| {
        let store = Arc::new(InMemoryStore::new());
        let engine = Engine::new(store.clone(), "host-a", WS)
            .with_clock(Arc::new(ScriptedClock::new(1_000, step)));
        for sp in order {
            observe_named(&engine, &format!("a mention of {sp}"), sp);
        }
        let e = store.get_entity(&Entity::make_id(WS, "driver")).unwrap().expect("entity");
        let mut spellings = e.aliases.clone();
        spellings.push(e.canonical_name.clone());
        spellings.sort();
        (e.canonical_name, spellings)
    };
    let a = build(["Driver", "driver", "DRIVER"], 2);
    let b = build(["DRIVER", "Driver", "driver"], 2);

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

    // The branch the sleep used to hide. With every ingest inside one millisecond the ordering HLCs
    // tie, recency cannot choose, and selection falls to the stable id tiebreak (R2's final step).
    // Here the two orders really are one log - same content, same assertions, same timestamps, so
    // the same three content addresses - which makes this a P16 convergence claim rather than a
    // comparison of two different histories.
    let tied_a = build(["Driver", "driver", "DRIVER"], 0);
    let tied_b = build(["DRIVER", "Driver", "driver"], 0);
    assert_eq!(tied_a.1, tied_b.1, "the spelling union is the same under a tie");
    assert_eq!(tied_a.1, vec!["DRIVER", "Driver", "driver"], "a tie drops no spelling either");
    assert_eq!(
        tied_a.0, tied_b.0,
        "with the HLCs tied the representative cannot depend on arrival order (P16): {} vs {}",
        tied_a.0, tied_b.0
    );

    // Which rule decided it, stated rather than assumed. Order-independence alone is satisfied by
    // any stable iteration order, so comparing the two orders to each other cannot tell the declared
    // tiebreak from an accident of how the fold happens to walk its candidates - removing
    // `Reverse(observation)` from TierWeighted leaves both assertions above passing. R2's last step
    // is the LOWEST observation id, so the test computes the three content addresses and names the
    // spelling that must win.
    let lowest_id_spelling = ["Driver", "driver", "DRIVER"]
        .into_iter()
        .min_by_key(|sp| {
            supragnosis_core::observation_content_id(
                WS,
                &format!("a mention of {sp}"),
                &Assertions {
                    entities: vec![EntityAssertion {
                        name: (*sp).into(),
                        kind: None,
                        description: None,
                    }],
                    ..Default::default()
                },
            )
        })
        .expect("three spellings");
    assert_eq!(
        tied_a.0, lowest_id_spelling,
        "a tie must resolve to the lowest observation id (resolution.md R2, final step)"
    );

    // And the tie is genuinely a different branch from recency: recency answered differently for
    // these two orders above, so one shared answer here cannot be "the last spelling seen".
    assert_ne!(a.0, b.0, "the recency cases must disagree, or the tie case proves nothing");
}

/// guard (P17 / excision.md Section 8 step 2): the curation report finds credential-shaped text
/// ALREADY in the log, reports where without repeating it, and commits nothing.
///
/// The ingest door keeps new ones out. This is for what predates the door, arrived while it was off,
/// or matches a pattern added since - the honest intermediate state while the removal path does not
/// exist. Not being able to delete it is not a reason to leave the operator unaware of it.
///
/// The door and the scan walk ONE field list. Two would drift, and the drift is silent in the worst
/// direction: a field the door checks and the scan does not is a secret reported as absent.
#[test]
fn p17_the_log_is_scanned_for_secrets_that_predate_the_door() {
    let store = Arc::new(InMemoryStore::new());
    // Written with the scan off - exactly the shape of a row that predates the hook.
    let unguarded = Engine::new(store.clone(), "host-a", WS).with_secret_scan(false);
    unguarded
        .observe(ObserveInput {
            content: "deploy notes: AKIAIOSFODNN7EXAMPLE".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "prod".into(),
                kind: None,
                description: Some("reached at postgres://admin:hunter2@db/app".into()),
            }],
            relations: vec![],
        })
        .expect("the scan is off, so it lands");

    let engine = Engine::new(store.clone(), "host-a", WS);
    let before = store.all_observations(Some(WS)).expect("log").len();
    let report = engine.curation(Some(WS)).expect("curation");

    assert_eq!(report.secrets.len(), 2, "both fields are found: {:?}", 
        report.secrets.iter().map(|f| (f.field, f.pattern)).collect::<Vec<_>>());
    let shapes: Vec<(&str, &str)> =
        report.secrets.iter().map(|f| (f.field, f.pattern)).collect();
    assert!(shapes.contains(&("content", "aws-access-key-id")), "{shapes:?}");
    assert!(shapes.contains(&("entity description", "url-inline-credentials")), "{shapes:?}");

    // The report names the shape and the place, never the value - it travels into logs and
    // screenshots, so quoting the secret would copy it everywhere the report goes.
    let rendered = serde_json::to_string(&report.secrets).expect("serialize");
    assert!(!rendered.contains("AKIAIOSFODNN7EXAMPLE"), "the report quoted a secret: {rendered}");
    assert!(!rendered.contains("hunter2"), "the report quoted a secret: {rendered}");

    // A signal generates; it commits nothing (P7/I18).
    assert_eq!(store.all_observations(Some(WS)).expect("log").len(), before);

    // And a workspace with nothing credential-shaped reports nothing, rather than reporting noise.
    let clean = Engine::new(store.clone(), "host-a", "ws-clean");
    clean
        .observe(ObserveInput {
            content: "the deploy reads its key from the environment".into(),
            workspace: Some("ws-clean".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .expect("observe");
    assert!(clean.curation(Some("ws-clean")).expect("curation").secrets.is_empty());
}

/// guard (P17, "provide a secret-redaction hook at ingest"): a credential-shaped observation is
/// refused before the log, and the refusal does not repeat the secret.
///
/// Refused, not rewritten. P1 forbids transforming an assertion before it reaches the log, and
/// rewriting would change the content and therefore the content address (P14) - so the hook declines
/// the write and tells the caller how to observe the knowledge without the secret (P21).
///
/// This is the moment that matters: the log is append-only, its only removal path is the destruction
/// exception (unbuilt - excision.md), and a shared workspace replicates. Nothing later is cheap.
#[test]
fn p17_a_credential_is_refused_at_ingest_without_being_echoed() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS);
    let secret = "AKIAIOSFODNN7EXAMPLE";

    let err = engine
        .observe(ObserveInput {
            content: format!("the deploy uses {secret} for S3"),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .err()
        .expect("a credential must not reach the log");
    let msg = err.to_string();
    assert!(msg.contains("aws-access-key-id"), "names the shape: {msg}");
    assert!(msg.contains("content"), "names the field: {msg}");
    assert!(!msg.contains(secret), "the refusal must not repeat the secret: {msg}");
    assert!(
        store.all_observations(Some(WS)).expect("log").is_empty(),
        "nothing was written - the check runs before the append, which is the only moment it can"
    );

    // It reaches the assertion fields too, not just the free text.
    assert!(engine
        .observe(ObserveInput {
            content: "a deploy note".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "prod-db".into(),
                kind: None,
                description: Some("postgres://admin:hunter2@db.internal/app".into()),
            }],
            relations: vec![],
        })
        .is_err());

    // The same knowledge, said without the secret, goes in.
    engine
        .observe(ObserveInput {
            content: "the deploy reads its S3 key from the environment".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { name: "deploy".into(), kind: None, description: None }],
            relations: vec![],
        })
        .expect("knowledge about a credential is not a credential");
    assert_eq!(store.all_observations(Some(WS)).expect("log").len(), 1);

    // Opt-out is explicit, so a corpus that trips the patterns is a decision rather than a workaround.
    let unguarded = Engine::new(store.clone(), "host-a", WS).with_secret_scan(false);
    unguarded
        .observe(ObserveInput {
            content: format!("the deploy uses {secret} for S3"),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .expect("the operator turned the scan off");
    assert_eq!(store.all_observations(Some(WS)).expect("log").len(), 2);
}

/// guard (P11, "the scope of the T-Box is the workspace"): an all-workspaces read is the UNION of
/// per-workspace glossaries, never one merged glossary.
///
/// The fold keyed definitions by (target, name) and not by workspace, so two workspaces that both
/// defined `Widget` collapsed into one row: one description won, the other was reported as a
/// `contested` competitor, and the sources count summed across them. Every part of that is wrong
/// under P11 - there is no global domain T-Box, so those are two different types, and the only thing
/// that connects types across a workspace boundary is an explicit alignment assertion.
///
/// It was invisible from a scoped read, which only ever holds one workspace. Only the all-view could
/// show it, and that is the view the console offers.
#[test]
fn p11_the_all_workspaces_glossary_does_not_merge_across_workspaces() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", "ws-a");
    for (ws, desc) in [("ws-a", "a thing in A"), ("ws-b", "a DIFFERENT thing in B")] {
        engine
            .define_type(DefineTypeInput {
                workspace: Some(ws.into()),
                source_ref: None,
                on_behalf_of: None,
                defs: vec![TypeDefInput {
                    target: TypeTarget::Entity,
                    name: "Widget".into(),
                    description: desc.into(),
                }],
            })
            .expect("define_type");
    }

    let all = engine.types(None).expect("all-workspaces glossary");
    let widgets: Vec<_> = all.iter().filter(|t| t.name == "Widget").collect();
    assert_eq!(
        widgets.len(),
        2,
        "two workspaces defining the same name are two types, not one: {:?}",
        all.iter().map(|t| (&t.workspace, &t.name, &t.description)).collect::<Vec<_>>()
    );
    for t in &widgets {
        assert_eq!(t.sources, 1, "a definition is corroborated within its workspace, not across");
        assert!(!t.contested, "unrelated workspaces do not put a type in conflict with itself");
    }
    let mut seen: Vec<(&str, &str)> =
        widgets.iter().map(|t| (t.workspace.as_str(), t.description.as_str())).collect();
    seen.sort_unstable();
    assert_eq!(
        seen,
        vec![("ws-a", "a thing in A"), ("ws-b", "a DIFFERENT thing in B")],
        "each workspace keeps its own definition"
    );

    // And the scoped reads, which never showed the bug, must still answer as they did.
    for (ws, desc) in [("ws-a", "a thing in A"), ("ws-b", "a DIFFERENT thing in B")] {
        let scoped = engine.types(Some(ws)).expect("scoped glossary");
        assert_eq!(scoped.len(), 1);
        assert_eq!((scoped[0].workspace.as_str(), scoped[0].description.as_str()), (ws, desc));
    }
}

/// guard (P1, "the graph is a projection: re-deriving it from the log reproduces it exactly"): the
/// EDGE half of that claim, which its sibling above does not reach.
///
/// The sibling checks entities - kind, canonical_name, aliases, provenance count - and the clause it
/// stands for says "the graph". Relations were the half nobody looked at, and they diverged: observe
/// stamped an edge with the attestation of the call that wrote it, reproject with the authoring
/// attestation of the HLC-latest observation asserting it. Identical for a fresh single-attestation
/// row, different the moment one absorbs a second attestation - so a reproject could move an edge's
/// tier and confidence with no change in the log at all.
///
/// The absorb is what makes the two rules disagree, so the fixture has to contain one: the same
/// content observed twice under different attestations is one row (P3) whose authoring attestation is
/// the earlier of them, while the second observe's own attestation is the later.
#[test]
fn incremental_write_equals_replay_for_relations() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS);

    let observe = |host: &str| {
        Engine::new(store.clone(), host, WS)
            .observe(ObserveInput {
                content: "alpha depends on beta".into(),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![RelationInput {
                    from: "Alpha".into(),
                    kind: "depends_on".into(),
                    to: "Beta".into(),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .expect("observe")
    };

    // Same content, two hosts: one content address, two attestations (P3 absorb).
    observe("host-a");
    let out = observe("host-b");
    let edge_id = out.relations.first().cloned().expect("the observe named an edge");

    let after_incremental = store
        .all_relations(Some(WS))
        .expect("relations")
        .into_iter()
        .find(|r| r.id == edge_id)
        .expect("the edge is projected");

    engine.reproject(Some(WS)).expect("reproject");
    let after_replay = store
        .all_relations(Some(WS))
        .expect("relations")
        .into_iter()
        .find(|r| r.id == edge_id)
        .expect("the edge survives the replay");

    assert_eq!(
        (after_incremental.provenance.host.as_str(), after_incremental.provenance.observed_at),
        (after_replay.provenance.host.as_str(), after_replay.provenance.observed_at),
        "a replay must not restamp an edge the log did not change - incremental {:?} vs replay {:?}",
        after_incremental.provenance,
        after_replay.provenance,
    );
    assert_eq!(after_incremental.provenance.trust_tier, after_replay.provenance.trust_tier);
    assert_eq!(after_incremental.provenance.confidence, after_replay.provenance.confidence);
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

// --- P8 / P2: the refusals themselves, on both the local and the wire path ---------------------

/// guard (principles.md P8, architecture.md Section 14 "define_type **rejects** a type with no
/// description"): the clause that gives P8 teeth is a refusal, and a refusal is only guaranteed by
/// a test that tries to get past it. Passing-path tests cannot see this one - delete the validation
/// block and every other test in this suite still goes green.
///
/// Both entry points are checked, because `check_event` claims they agree: the local `define_type`
/// refuses before the log, and `Observation::check_well_formed` refuses the same shape arriving
/// signed over the wire. A signature proves origin, not well-formedness (P18), so a gap between the
/// two would be a way to put a meaningless type into canon from a peer.
#[test]
fn p8_a_type_definition_without_a_description_is_refused_on_both_paths() {
    use supragnosis_core::{TypeDefAssertion, TypeTarget};
    use supragnosis_engine::{DefineTypeInput, TypeDefInput};

    let (_store, engine) = engine();
    let define = |defs: Vec<TypeDefInput>| {
        engine.define_type(DefineTypeInput {
            workspace: None,
            defs,
            source_ref: None,
            on_behalf_of: None,
        })
    };
    let def = |name: &str, description: &str| TypeDefInput {
        target: TypeTarget::Entity,
        name: name.into(),
        description: description.into(),
    };

    // Local path. Each refusal must also say what to do instead (P21: written for self-correction).
    for (label, defs, hint) in [
        ("no definitions at all", vec![], "at least one"),
        ("empty name", vec![def("", "a deployable part")], "name the type"),
        ("empty description", vec![def("Component", "")], "Principle 8"),
        ("whitespace description", vec![def("Component", "   ")], "Principle 8"),
    ] {
        let err = define(defs).err().unwrap_or_else(|| panic!("{label} must be refused"));
        assert!(
            err.to_string().contains(hint),
            "{label}: the refusal must tell the caller how to correct it, got: {err}"
        );
    }

    // Wire path: the same shape, signed and arriving as an observation, is refused by the core
    // well-formedness check rather than landing in the log.
    let typedef_event = |name: &str, description: &str| {
        Observation::with_assertions(
            "a type definition from a peer".into(),
            Provenance {
                host: "peer-host".into(),
                on_behalf_of: None,
                workspace: WS.into(),
                source_ref: None,
                observed_at: 100,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions {
                type_defs: vec![TypeDefAssertion {
                    target: TypeTarget::Entity,
                    name: name.into(),
                    description: description.into(),
                }],
                ..Default::default()
            },
        )
    };
    assert!(
        typedef_event("Component", "").check_well_formed().is_err(),
        "an empty description must be refused on the wire path too, or a peer can put a \
         meaningless type into canon that define_type would have rejected locally"
    );
    assert!(
        typedef_event("", "a deployable part").check_well_formed().is_err(),
        "an empty name must be refused on the wire path too"
    );

    // Positive control: a well-formed definition is accepted and reaches the glossary. A validator
    // that refuses everything would satisfy the assertions above and be entirely broken.
    define(vec![def("Component", "a deployable part")]).expect("a well-formed definition is fine");
    assert!(
        typedef_event("Component", "a deployable part").check_well_formed().is_ok(),
        "the wire path must accept what the local path accepts"
    );
    let types = engine.types(Some(WS)).expect("glossary");
    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name, "Component");
    assert_eq!(types[0].description, "a deployable part");
}

/// characterization (principles.md P2, architecture.md Section 14 **overdue entry condition 1**):
/// "every observation carries at least one attestation" is a doc comment on the field and a
/// guarantee of the constructors - it is NOT checked anywhere. `check_well_formed` loops over
/// `provenance` to range-check confidence, so an EMPTY list passes it vacuously.
///
/// This pins both halves of the ledger's claim: the type permits a zero-provenance observation, and
/// the reason one cannot currently land is that nothing constructs it that way. That is a
/// reachability argument, not a guard, which is exactly what the entry condition was written to
/// end. When it is repaid, this test MUST be rewritten to assert that the empty case is REFUSED.
#[test]
fn p2_at_least_one_attestation_is_a_constructor_guarantee_not_a_checked_one() {
    // The gap: a zero-provenance observation is representable and passes well-formedness.
    let unattested = Observation {
        id: "synthetic".into(),
        content: "a claim nobody attested".into(),
        provenance: vec![],
        assertions: Assertions::default(),
        derived_from: vec![],
        embedding: None,
    };
    assert!(
        unattested.check_well_formed().is_ok(),
        "INTERIM: check_well_formed does not require an attestation. If this now fails, overdue \
         entry condition 1 has been repaid - rewrite this test to assert the refusal (P2)"
    );

    // Why it is nonetheless unreachable today: every constructor takes one attestation by value, so
    // the ingest and sync paths cannot produce the empty case. This is the reachability argument the
    // deferral rests on - if it ever stops holding, the gap above becomes live.
    let built = Observation::new(
        "a claim someone attested".into(),
        Provenance {
            host: "host-a".into(),
            on_behalf_of: None,
            workspace: WS.into(),
            source_ref: None,
            observed_at: 100,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        },
    );
    assert_eq!(built.provenance.len(), 1, "the constructor is what supplies the guarantee");
}

// --- P23 / P21: the gate surface refuses what it cannot honestly record ------------------------

/// guard (proposal-workflow.md Section 3, principles.md P23/P21): `propose` and `review_proposal`
/// are the write surface of the canon gate, and every refusal below keeps a proposal that could
/// never be folded coherently out of the permanent log. Only one of these branches had a test, so
/// the rest could have been deleted without a single failure.
///
/// P21 is asserted alongside P23 on purpose: these errors are read by an LLM that must correct
/// itself without a human, so each one has to name the fix, not merely say no.
#[test]
fn p23_the_gate_surface_refuses_a_malformed_proposal() {
    let (_store, engine) = engine();
    let (x, y) = mergeable_pair(&engine);
    // A real observation id, so gate-kind referential integrity passes and the cases below fail on
    // the branch each one is aiming at. Asserted rather than assumed: if the fixture grows another
    // observation, this must say so instead of silently pointing at a different one.
    let log = engine.observation_log(Some(WS), None, None).expect("log");
    assert_eq!(log.len(), 1, "the fixture asserts exactly one observation");
    let obs_id = log[0].id.clone();

    let propose = |kind: &str, targets: Vec<String>, into: Option<String>, tier: Option<String>| {
        engine.propose(ProposeInput {
            workspace: None,
            kind: kind.into(),
            targets,
            into,
            tier,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("alice".into()),
        })
    };
    let pair = || vec![x.clone(), y.clone()];

    for (label, kind, targets, into, tier, hint) in [
        ("unknown kind", "entity_split", pair(), None, None, "unknown proposal kind"),
        ("no targets", "entity_merge", vec![], None, None, "at least one non-empty target"),
        ("blank target", "entity_merge", vec![x.clone(), "  ".into()], None, None, "non-empty target"),
        ("merge of one", "entity_merge", vec![x.clone()], Some(x.clone()), None, "at least 2 target"),
        ("merge without into", "entity_merge", pair(), None, None, "needs `into`"),
        ("into outside targets", "entity_merge", pair(), Some("other".into()), None, "must be one of the targets"),
        ("gate kind without tier", "claim_promotion", vec![obs_id.clone()], None, None, "needs `tier`"),
        ("unknown tier", "claim_promotion", vec![obs_id.clone()], None, Some("archangel".into()), "unknown tier"),
        ("promote what is not here", "claim_promotion", vec!["no-such-observation".into()], None, Some("host_signed".into()), "not in the local log"),
        ("tier on a non-gate kind", "entity_merge", pair(), Some(y.clone()), Some("host_signed".into()), "only applies to claim_promotion"),
    ] {
        let err = propose(kind, targets, into, tier)
            .err()
            .unwrap_or_else(|| panic!("{label} must be refused"));
        assert!(
            err.to_string().contains(hint),
            "{label}: the refusal must name the fix (P21), got: {err}"
        );
    }
    assert!(
        engine.list_proposals(Some(WS)).expect("list").is_empty(),
        "not one refused proposal may reach the log - the refusal is before the write (P3: what \
         lands is permanent)"
    );

    // The verdict surface refuses on the same terms.
    let good = propose("entity_merge", pair(), Some(y.clone()), None).expect("well-formed");
    for (label, id, decision, hint) in [
        ("unknown decision", good.clone(), "approve", "unknown decision"),
        ("empty proposal id", "  ".to_string(), "merge", "proposal id is required"),
    ] {
        let err = engine
            .review_proposal(None, id, decision.into(), None, Some("bob".into()), VerdictSurface::Console)
            .err()
            .unwrap_or_else(|| panic!("{label} must be refused"));
        assert!(
            err.to_string().contains(hint),
            "{label}: the refusal must name the fix (P21), got: {err}"
        );
    }
    assert_eq!(
        engine.get_proposal(Some(WS), &good).expect("get").expect("proposal").state,
        "open",
        "a refused verdict must leave the proposal exactly where it was"
    );
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

// --- P14 / P3: a re-keyed row is one act, not two ----------------------------------------------

/// guard (principles.md P14 content-is-identity, P3 nothing is deleted): `migrate` re-creates a
/// pre-formula row under the current content address and, the log being append-only, leaves the
/// original in place. Both rows then carry the same content and the same assertions, so a raw
/// enumeration would report one act as two.
///
/// What that costs is not cosmetic. A proposal's id IS its opening observation's id, so the copy
/// becomes a second proposal that no verdict can ever reference - permanently open, un-closable. And
/// an entity's supporting-attestation count doubles, which the corroboration rules read as extra
/// independent support (P2/P18). Both are measured here before and after the migration, because a
/// duplicate is only legible as a difference.
///
/// The other half of the claim is that this is dedup, not deletion (P3): the predecessor is still in
/// the store and still dereferenceable by its id afterwards.
#[test]
fn p14_migration_rekeys_an_act_without_duplicating_it() {
    let (store, engine) = engine();
    let prov = || Provenance {
        host: "host-a".into(),
        on_behalf_of: Some("ashon".into()),
        workspace: WS.into(),
        source_ref: None,
        observed_at: 10,
        confidence: None,
        trust_tier: TrustTier::default(),
        sync: None,
    };
    // Two pre-formula rows: one opening a proposal, one asserting an entity. Simulating the id era
    // is what `migrate` itself keys on - a stored id that no longer matches the current formula.
    let mut legacy_proposal = Observation::with_assertions(
        "propose: promote the store note".into(),
        prov(),
        Assertions {
            proposal_events: vec![supragnosis_core::ProposalEventAssertion {
                proposal: String::new(),
                event: supragnosis_core::ProposalEventKind::Opened,
                payload: r#"{"kind":"claim_promotion","targets":["obs-x"],"tier":"host_signed"}"#
                    .into(),
            }],
            ..Default::default()
        },
    );
    legacy_proposal.id = "legacy-era-proposal-id".into();
    let mut legacy_entity = Observation::with_assertions(
        "cozo is the store".into(),
        prov(),
        Assertions {
            entities: vec![EntityAssertion {
                name: "Cozo".into(),
                kind: Some("Tool".into()),
                description: None,
            }],
            ..Default::default()
        },
    );
    legacy_entity.id = "legacy-era-entity-id".into();
    store.add_observation(legacy_proposal).unwrap();
    store.add_observation(legacy_entity).unwrap();
    engine.reproject(Some(WS)).unwrap();

    let sources = |e: &Engine| {
        e.graph(Some(WS))
            .unwrap()
            .nodes
            .iter()
            .find(|n| n.name == "Cozo")
            .expect("the entity projects")
            .sources
    };
    let before = (engine.list_proposals(Some(WS)).unwrap().len(), sources(&engine));
    assert_eq!(before, (1, 1), "one proposal and one supporting attestation to start with");

    let migrated = supragnosis_sync::migrate_legacy_ids(store.as_ref(), WS).unwrap();
    assert_eq!(migrated, 2, "both pre-formula rows are re-keyed");
    engine.reproject(Some(WS)).unwrap();

    assert_eq!(
        (engine.list_proposals(Some(WS)).unwrap().len(), sources(&engine)),
        before,
        "re-keying an act must not turn it into two - a duplicate proposal cannot be closed, \
         and a doubled attestation count reads as independent corroboration (P14, P2/P18)"
    );
    // Dedup, not deletion (P3): the predecessor is still there to be dereferenced.
    for id in ["legacy-era-proposal-id", "legacy-era-entity-id"] {
        assert!(
            store.get_observation(id).unwrap().is_some(),
            "{id} must stay in the store - the successor supersedes it, nothing erases it"
        );
    }
    assert_eq!(
        store.all_observations(Some(WS)).unwrap().len(),
        4,
        "the store holds both the predecessors and their re-keyed successors"
    );
}

// --- P2 / P4: a workspace re-key preserves provenance, a re-ingest would not -------------------

/// guard (principles.md P2 provenance first-class, P4 transaction time, P3 nothing is deleted):
/// `rekey_workspace` re-creates a workspace's knowledge under another name with every attestation
/// copied verbatim - acting host, `on_behalf_of`, `observed_at`, confidence. (The claimed tier is
/// the one exception: it is carried as its pre-strip evaluation, not verbatim - see the P18
/// laundering guard below.)
///
/// The distinction this pins is the whole reason the operation exists. Pushing the same text back
/// through `observe` would look equivalent and is not: the engine stamps its own clock and host, so
/// a "move" done that way rewrites who observed the knowledge and when, and flattens the HLC order
/// that last-write-wins fields are decided by. The re-key is modelled on `migrate_legacy_ids`
/// instead, which re-keys across a change of id formula rather than of workspace.
#[test]
fn p2_a_workspace_rekey_carries_provenance_that_a_reingest_would_restamp() {
    let (store, engine) = engine();
    // Ingest through the real door, then rewrite the stored attestation to an older, other-host act -
    // the state a re-key has to carry rather than overwrite.
    observe(&engine, "the driver depends on the kernel", &["Driver"], vec![]);
    let original = store.all_observations(Some(WS)).unwrap().pop().expect("one row");
    let mut aged = original.clone();
    aged.provenance = vec![Provenance {
        host: "host-b".into(),
        on_behalf_of: Some("ashon".into()),
        workspace: WS.into(),
        source_ref: Some("docs/design.md".into()),
        observed_at: 1_700_000_000_000,
        confidence: Some(0.9),
        trust_tier: TrustTier::default(),
        sync: None,
    }];
    store.add_observation(aged.clone()).unwrap();

    let dry = engine.rekey_workspace(WS, "archive", true).expect("dry run");
    assert_eq!(dry.moved, 1, "the dry run counts the knowledge row");
    assert!(
        store.all_observations(Some("archive")).unwrap().is_empty(),
        "a dry run must write nothing"
    );

    let rep = engine.rekey_workspace(WS, "archive", false).expect("rekey");
    assert_eq!(rep.moved, 1);

    let moved = store.all_observations(Some("archive")).unwrap().pop().expect("re-keyed row");
    assert_eq!(moved.content, original.content, "content is unchanged - this is a re-key, not an edit");
    assert_ne!(moved.id, original.id, "the workspace is inside the content address, so the id must differ");
    // The whole attestation SET is carried, not a representative: re-observing the same content
    // absorbed a second attestation onto the row, and a re-key that kept only one would be losing
    // provenance just as surely as a re-ingest does (P3's union, P2's first-class provenance).
    // Found by identity rather than by index - indexing passed only while the fixture host happened
    // to sort first, which is a property of the alphabet, not of the code under test.
    assert_eq!(moved.provenance.len(), 2, "both attestations of the source row are carried over");
    assert!(
        moved.provenance.iter().all(|p| p.workspace == "archive"),
        "every carried attestation names the new workspace"
    );
    let p = moved
        .provenance
        .iter()
        .find(|p| p.host == "host-b")
        .expect("the aged attestation is among them");
    assert_eq!(
        (p.host.as_str(), p.on_behalf_of.as_deref(), p.observed_at, p.confidence),
        ("host-b", Some("ashon"), 1_700_000_000_000, Some(0.9)),
        "acting host, principal, transaction time and confidence survive verbatim (P2/P4) - \
         a re-ingest through observe would have stamped this engine's host and clock instead"
    );
    assert!(moved.derived_from.contains(&aged.id), "lineage records where it came from");

    // P3: the original is still there. A re-key adds, it never moves anything away.
    assert!(
        store.get_observation(&aged.id).unwrap().is_some(),
        "the source row must survive - the log is append-only"
    );
    // Idempotent, and the same act is not counted twice in the live set.
    assert_eq!(engine.rekey_workspace(WS, "archive", false).unwrap().moved, 0, "a second run moves nothing");
    assert_eq!(
        engine.rekey_workspace(WS, "archive", false).unwrap().already, 1,
        "it recognises what it already re-keyed"
    );
}

// --- P18: dropping the sync stamp must not raise what a claim evaluates to ----------------------

/// guard (P18 the tier is the receiver's evaluation; resolution.md Section 3): `evaluated_tier`
/// trusts a stamp-less claim at face value on the premise that every stamp-less producer holds the
/// line - the local observe door forces the default tier. Re-key and migration both strip the sync
/// stamp (it signs the old content id and cannot follow), so each must clamp the carried claim to
/// its pre-strip evaluation. Without the clamp, a peer-asserted human_confirmed - correctly
/// evaluating HostSigned while stamped (the sibling guard above) - evaluates HumanConfirmed after
/// one operator CLI act, and from there wins belief selection outright.
#[test]
fn p18_rekey_and_migration_clamp_a_synced_claim_to_its_evaluation() {
    let (store, engine) = engine();
    let stamped_claim = || Provenance {
        host: "peer-host".into(),
        on_behalf_of: Some("mallory".into()),
        workspace: WS.into(),
        source_ref: None,
        observed_at: 100,
        confidence: None,
        trust_tier: TrustTier::HumanConfirmed, // self-declared; stored verbatim per F13
        sync: Some(SyncMeta {
            origin_node: "peer-node".into(),
            origin_seq: 1,
            hlc: Hlc { wall: 100, counter: 0, node: "peer-node".into() },
            signature: "not-verified-here".into(),
            lineage: vec![],
        }),
    };

    // Re-key path.
    let synced =
        Observation::with_assertions("peer-asserted claim".into(), stamped_claim(), Assertions::default());
    store.add_observation(synced).unwrap();
    let rep = engine.rekey_workspace(WS, "archive", false).expect("rekey");
    assert_eq!(rep.moved, 1);
    let moved = store.all_observations(Some("archive")).unwrap().pop().expect("re-keyed row");
    let p = &moved.provenance[0];
    assert!(p.sync.is_none(), "the stamp signs the old content id and cannot follow");
    assert_eq!(
        evaluated_tier(p),
        TrustTier::HostSigned,
        "the claim must evaluate after the re-key exactly as it evaluated before it - \
         dropping the stamp is not a tier promotion"
    );

    // Migration path: the same stamped claim under a fabricated pre-formula id.
    let mut legacy = Observation::with_assertions(
        "legacy stamped claim".into(),
        stamped_claim(),
        Assertions::default(),
    );
    legacy.id = "legacy-era-stamped-id".into();
    store.add_observation(legacy).unwrap();
    assert_eq!(supragnosis_sync::migrate_legacy_ids(store.as_ref(), WS).unwrap(), 1);
    let migrated = store
        .all_observations(Some(WS))
        .unwrap()
        .into_iter()
        .find(|o| o.derived_from.contains(&"legacy-era-stamped-id".to_string()))
        .expect("migrated row");
    let p = &migrated.provenance[0];
    assert!(p.sync.is_none());
    assert_eq!(
        evaluated_tier(p),
        TrustTier::HostSigned,
        "the migration path holds the same line as the re-key path"
    );
}

/// guard (P3 nothing is hidden, P16 scoped and unscoped reads agree): the live-set door supersedes
/// a re-keyed predecessor only within one workspace (a migration). A re-keyed pair spans two
/// workspaces, and dropping the source row from the workspace=None fold would show the source
/// workspace's entities without the support its own scoped view reports - the unscoped view must
/// be the union of the scoped ones.
#[test]
fn p3_a_rekey_keeps_the_source_row_live_in_the_unscoped_view() {
    let (_store, engine) = engine();
    observe(&engine, "the driver depends on the kernel", &["Driver"], vec![]);
    engine.rekey_workspace(WS, "archive", false).expect("rekey");
    assert_eq!(engine.observation_log(Some(WS), None, None).unwrap().len(), 1);
    assert_eq!(engine.observation_log(Some("archive"), None, None).unwrap().len(), 1);
    assert_eq!(
        engine.observation_log(None, None, None).unwrap().len(),
        2,
        "the unscoped log is the union of the scoped ones - a re-key must not hide the source \
         row from the all-workspaces view while its own workspace still shows it"
    );
}

/// guard (P2 provenance first-class - attribution follows the author): after an absorb the
/// provenance vec is union-sorted by host, so `first()` names whichever host sorts first, and a
/// max over `observed_at` moves forward as attestations accumulate. The proposal view must name
/// the authoring attestation (earliest effective HLC) - the same rule the verdict surface marker
/// and relation provenance already follow.
#[test]
fn p2_proposal_attribution_names_the_authoring_attestation() {
    let (store, engine) = engine();
    let (x, y) = mergeable_pair(&engine);
    let p = propose_merge(&engine, &[&x, &y], &y, "alice");
    let before = engine.list_proposals(Some(WS)).unwrap().pop().expect("one proposal");
    // A second attestation of the same proposal row arrives by absorb: a host that sorts BEFORE
    // the author's, observed LATER. Neither first() nor max(observed_at) may follow it.
    let stored = store.get_observation(&p).unwrap().expect("proposal row");
    let mut copy = stored.clone();
    copy.provenance = vec![Provenance {
        host: "aaa-relay".into(),
        on_behalf_of: Some("mallory".into()),
        workspace: WS.into(),
        source_ref: None,
        observed_at: before.opened_at + 60_000,
        confidence: None,
        trust_tier: TrustTier::default(),
        sync: None,
    }];
    store.add_observation(copy).unwrap();
    let after = engine.list_proposals(Some(WS)).unwrap().pop().expect("still one proposal");
    assert_eq!(
        after.proposer, before.proposer,
        "the proposer is the authoring attestation's principal, not the sort-first host's"
    );
    assert_eq!(
        after.opened_at, before.opened_at,
        "opened_at is the authoring attestation's time and does not move as attestations accumulate"
    );
}

/// guard: a re-key leaves proposal events behind on purpose. Their payloads name SOURCE-workspace
/// entity ids, which cannot exist in the target, so carrying them over would import proposals that
/// are permanently blocked on referential integrity - importing the disease, not the cure.
#[test]
fn p23_a_rekey_does_not_carry_proposal_events_into_the_new_workspace() {
    let (_store, engine) = engine();
    observe(&engine, "one", &["Postgres"], vec![]);
    observe(&engine, "two", &["PostgreSQL"], vec![]);
    let (a, b) = (Entity::make_id(WS, "postgres"), Entity::make_id(WS, "postgresql"));
    engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![a, b.clone()],
            into: Some(b),
            tier: None,
            rationale: Some("duplicate".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");

    let rep = engine.rekey_workspace(WS, "archive", false).expect("rekey");
    assert_eq!(rep.moved, 2, "both knowledge rows move");
    assert_eq!(rep.skipped_proposal_events, 1, "the proposal event stays behind");
    assert!(
        engine.list_proposals(Some("archive")).unwrap().is_empty(),
        "no proposal may arrive in the target - one that did would be blocked forever"
    );
    assert_eq!(
        engine.list_proposals(Some(WS)).unwrap().len(),
        1,
        "and the source keeps its gate history"
    );
}
