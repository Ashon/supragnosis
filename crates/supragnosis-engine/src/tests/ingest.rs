//! Observe and the log, search, the graph projection and the hypergraph.

use super::*;

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
                EntityInput { description: None, name: "rmcp".into(), kind: Some("Tool".into()) },
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
    assert!(
        observe(vec![ent("  Padded Name  ", Some("Tool"))], vec![rel("a", "Depends-On", "b")],)
            .is_ok()
    );
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
                entities: vec![EntityInput { description: None, name: "thing".into(), kind: None }],
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
    let hybrid =
        Engine::new(store.clone(), "h", "ws").with_embedder(Arc::new(HashingEmbedder::default()));

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
    let engine = Engine::new(store.clone(), "h", "ws1").with_embedder(Arc::new(FailingEmbedder));

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
                EntityInput { description: None, name: "rmcp".into(), kind: Some("Tool".into()) },
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
            entities: vec![EntityInput { description: None, name: "elsewhere".into(), kind: None }],
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
                entities: vec![EntityInput { description: None, name: name.into(), kind: None }],
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

/// A workspace that holds only free text - an observation asserting no entity, which is legal
/// (Principle 22) - is listed, though nothing in it is projected.
#[test]
fn a_workspace_of_free_text_is_listed() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "h", "alpha");
    engine
        .observe(ObserveInput {
            content: "a note with no entities".into(),
            workspace: Some("notes".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .unwrap();
    assert!(store.all_entities(None).unwrap().is_empty(), "nothing is projected");
    assert!(store.all_relations(None).unwrap().is_empty(), "nothing is projected");
    assert_eq!(engine.workspaces().unwrap(), vec!["notes"]);
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
