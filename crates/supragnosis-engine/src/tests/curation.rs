//! The name-variant ladder, the embedding backfill and the merge band.

use super::*;

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
    let once = serde_json::to_string(&engine.curation(Some("ws1")).unwrap().name_variants).unwrap();
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
        .map(|id| serde_json::to_value(store.get_observation(id).unwrap().expect("row")).unwrap())
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
    assert!(ids
        .iter()
        .all(|id| store.get_observation(id).unwrap().unwrap().embedding.is_some()));

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
                entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
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
                entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
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
                entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
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
