//! Ingest limits, read contexts, cross-node reprojection, the type fold and the proposal
//! fold.

use super::*;

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
    let Err(ObserveError::Invalid(msg)) = engine.observe(input("x".repeat(MAX_CONTENT_BYTES + 1)))
    else {
        panic!("an observation over the cap must be refused");
    };
    assert!(msg.contains("1 MiB"), "{msg}");
    let fits = input("x".repeat(MAX_CONTENT_BYTES));
    assert!(engine.observe(fits).is_ok(), "the cap itself is allowed");
}

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
                entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
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
    let delta_a = export_delta(store_a.as_ref(), "ws", &VersionVector::default(), &share).unwrap();
    let delta_b = export_delta(store_b.as_ref(), "ws", &VersionVector::default(), &share).unwrap();
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
        .review_proposal(None, pid2.clone(), "reject".into(), None, None, VerdictSurface::Console)
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

/// design-system.md Section 9, U17: every proposal event the engine can write has a sentence in the
/// Log - a noun for every kind it opens and a verb for every event, a verdict's for each decision
/// `review_proposal` casts - so a new kind or event cannot reach a page as "opened new_kind:
/// 9f778c20". A kind or a decision with no word is left without one, which the Log says in words.
#[test]
fn every_proposal_event_has_a_sentence() {
    // ALL is a hand-written list: a new event kind stops this match compiling until it is named
    // here, and then the assertion holds it to ALL.
    for event in [
        ProposalEventKind::Opened,
        ProposalEventKind::Verdict,
        ProposalEventKind::Withdrawn,
        ProposalEventKind::Comment,
    ] {
        match event {
            ProposalEventKind::Opened
            | ProposalEventKind::Verdict
            | ProposalEventKind::Withdrawn
            | ProposalEventKind::Comment => {
                assert!(ProposalEventKind::ALL.contains(&event), "ALL leaves out {event:?}")
            }
        }
    }
    for kind in PROPOSAL_KINDS {
        for event in ProposalEventKind::ALL {
            let decisions: Vec<Option<&str>> = if event == ProposalEventKind::Verdict {
                VERDICT_DECISIONS.iter().copied().map(Some).collect()
            } else {
                vec![None]
            };
            for decision in decisions {
                let (noun, verb) = proposal_words(kind, event, decision);
                assert!(
                    noun.is_some() && verb.is_some(),
                    "the Log has no sentence for {kind} {event:?} {decision:?}"
                );
            }
        }
    }
    let said = proposal_words("entity_merge", ProposalEventKind::Opened, None);
    assert_eq!(said, (Some("Merge"), Some("proposed")));
    assert_eq!(proposal_words("new_kind", ProposalEventKind::Opened, None).0, None);
    assert_eq!(proposal_words("recall", ProposalEventKind::Verdict, Some("comment")).1, None);
}
