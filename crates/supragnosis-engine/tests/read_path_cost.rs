//! What a read costs, counted rather than timed.
//!
//! Every read surface here is a fold over the observation log, and the log only grows. So the cost
//! that matters is not milliseconds on this machine - it is **how many times one call walks the
//! whole log**, which is a property of the code and identical everywhere. A wall-clock benchmark
//! would measure the CI runner; this measures the algorithm, and can therefore be a guard.
//!
//! [`CountingStore`] wraps a real store and tallies each enumeration plus the rows it handed back.
//! `scans` is the number the assertions are about: at 10k observations the difference between one
//! pass and four is four seconds of deserialization per viewer poll, and the viewer polls
//! `/api/graph` every 2.5s.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use supragnosis_core::{
    Entity, KnowledgeStore, Observation, Relation, SearchHit, StoreError, TraverseHit,
};
use supragnosis_engine::{Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, VerdictSurface};
use supragnosis_store::InMemoryStore;

const WS: &str = "ws";

/// A store that counts what the layer above asks of it.
struct CountingStore {
    inner: Box<dyn KnowledgeStore>,
    obs_scans: AtomicUsize,
    obs_rows: AtomicUsize,
    entity_scans: AtomicUsize,
    relation_scans: AtomicUsize,
    /// Per-item store queries. An enumeration count cannot see these, which is how a read surface
    /// that asks the store once per entity looked cheap.
    semantic_queries: AtomicUsize,
}

/// One read surface's cost, in enumerations of each table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    observations: usize,
    observation_rows: usize,
    entities: usize,
    relations: usize,
    semantic_queries: usize,
}

impl CountingStore {
    fn new() -> Self {
        Self::wrapping(Box::new(InMemoryStore::new()))
    }

    fn wrapping(inner: Box<dyn KnowledgeStore>) -> Self {
        Self {
            inner,
            obs_scans: AtomicUsize::new(0),
            obs_rows: AtomicUsize::new(0),
            entity_scans: AtomicUsize::new(0),
            relation_scans: AtomicUsize::new(0),
            semantic_queries: AtomicUsize::new(0),
        }
    }

    fn reset(&self) {
        for c in [&self.obs_scans, &self.obs_rows, &self.entity_scans, &self.relation_scans, &self.semantic_queries] {
            c.store(0, Ordering::SeqCst);
        }
    }

    fn cost(&self) -> Cost {
        Cost {
            observations: self.obs_scans.load(Ordering::SeqCst),
            observation_rows: self.obs_rows.load(Ordering::SeqCst),
            entities: self.entity_scans.load(Ordering::SeqCst),
            relations: self.relation_scans.load(Ordering::SeqCst),
            semantic_queries: self.semantic_queries.load(Ordering::SeqCst),
        }
    }

    /// Runs `f` with the counters zeroed and reports what it cost.
    fn measure<T>(&self, f: impl FnOnce() -> T) -> (T, Cost) {
        self.reset();
        let out = f();
        (out, self.cost())
    }
}

impl KnowledgeStore for CountingStore {
    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        self.obs_scans.fetch_add(1, Ordering::SeqCst);
        let rows = self.inner.all_observations(workspace)?;
        self.obs_rows.fetch_add(rows.len(), Ordering::SeqCst);
        Ok(rows)
    }
    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError> {
        self.entity_scans.fetch_add(1, Ordering::SeqCst);
        self.inner.all_entities(workspace)
    }
    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError> {
        self.relation_scans.fetch_add(1, Ordering::SeqCst);
        self.inner.all_relations(workspace)
    }

    // Everything else is plain delegation - these are point lookups, not enumerations.
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError> {
        self.inner.add_observation(obs)
    }
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        self.inner.get_observation(id)
    }
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError> {
        self.inner.get_entity(id)
    }
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError> {
        self.inner.put_entity(entity)
    }
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError> {
        self.inner.add_relation(rel)
    }
    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError> {
        self.inner.relations_of(entity_id)
    }
    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        self.inner.search(query, workspace, limit)
    }
    fn traverse(
        &self,
        start_id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError> {
        self.inner.traverse(start_id, max_depth, limit)
    }
    fn search_semantic(
        &self,
        q: &[f32],
        ws: Option<&str>,
        n: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        self.semantic_queries.fetch_add(1, Ordering::SeqCst);
        self.inner.search_semantic(q, ws, n)
    }
    fn search_semantic_entities(
        &self,
        q: &[f32],
        ws: Option<&str>,
        n: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        self.semantic_queries.fetch_add(1, Ordering::SeqCst);
        self.inner.search_semantic_entities(q, ws, n)
    }
}

/// A workspace with entities, relations, a merged entity_merge and a merged claim_promotion - so
/// every branch a read surface can take is actually reachable. A fixture with no proposals would
/// measure the cheap path and call it the cost.
fn workspace_n(store: &Arc<CountingStore>, n: usize) -> Engine {
    let engine = Engine::new(store.clone(), "host-a", WS);
    for i in 0..n {
        engine
            .observe(ObserveInput {
                content: format!("fact {i}"),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { name: format!("Ent {i}"), kind: Some("Concept".into()), description: None },
                    EntityInput { name: format!("Ent {}", i + 1), kind: None, description: None },
                ],
                relations: vec![RelationInput {
                    from: format!("Ent {i}"),
                    kind: "relates_to".into(),
                    to: format!("Ent {}", i + 1),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .expect("observe");
    }
    // A merged entity_merge, so merge forwarding is non-empty.
    let (a, b) = (Entity::make_id(WS, "Ent 0"), Entity::make_id(WS, "Ent 1"));
    let p = engine
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
        .expect("propose");
    engine
        .review_proposal(None, p, "merge".into(), None, Some("bob".into()), VerdictSurface::Console)
        .expect("review");
    // A merged claim_promotion, so the gate-grant fold has something to find.
    let obs = store.all_observations(Some(WS)).expect("obs")[0].id.clone();
    let g = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![obs],
            into: None,
            tier: Some("host_signed".into()),
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("alice".into()),
        })
        .expect("propose gate");
    engine
        .review_proposal(None, g, "merge".into(), None, Some("bob".into()), VerdictSurface::Console)
        .expect("review gate");
    engine
}

/// One named read surface to measure.
type Surface<'a> = (&'a str, Box<dyn Fn() + 'a>);

/// The cost of every read surface, printed with `--nocapture` and bounded by assertion.
///
/// The bound is one log pass per call. That is not an aspiration about speed - it is what these
/// folds need: proposal state, gate grants and the belief fold all read the same observations, and
/// each is a pure function of them. Reading the log more than once per call means the same rows were
/// deserialized again to answer a second question about them, which is work with no result to show.
///
/// `observation_rows` is carried alongside because it is where the cost actually lands: each row
/// deserializes a data JSON that includes a 384-float embedding these folds never look at.
#[test]
fn a_read_walks_the_log_once() {
    let store = Arc::new(CountingStore::new());
    let engine = workspace_n(&store, 12);
    let total = store.all_observations(Some(WS)).expect("obs").len();

    let surfaces: Vec<Surface> = vec![
        ("graph", Box::new({ let e = &engine; move || { e.graph(Some(WS)).expect("graph"); } })),
        ("curation", Box::new({ let e = &engine; move || { e.curation(Some(WS)).expect("curation"); } })),
        ("hypergraph", Box::new({ let e = &engine; move || { e.hypergraph(Some(WS)).expect("hypergraph"); } })),
        ("list_proposals", Box::new({ let e = &engine; move || { e.list_proposals(Some(WS)).expect("list"); } })),
        ("types", Box::new({ let e = &engine; move || { e.types(Some(WS)).expect("types"); } })),
    ];

    println!("\n{total} observations in the workspace\n");
    println!("{:<16} {:>6} {:>10} {:>9} {:>10}", "surface", "scans", "rows", "entities", "relations");
    let mut costs: BTreeMap<&str, Cost> = BTreeMap::new();
    for (name, run) in &surfaces {
        let (_, c) = store.measure(run);
        println!(
            "{name:<16} {:>6} {:>10} {:>9} {:>10}",
            c.observations, c.observation_rows, c.entities, c.relations
        );
        costs.insert(name, c);
    }
    println!();

    let over: Vec<String> = costs
        .iter()
        .filter(|(_, c)| c.observations > 1)
        .map(|(n, c)| {
            format!("{n} walks the log {} times ({} rows deserialized)", c.observations, c.observation_rows)
        })
        .collect();
    assert!(
        over.is_empty(),
        "a read surface may walk the observation log at most once - these fold the same rows \
         repeatedly to answer questions that one pass could answer together:\n  {}",
        over.join("\n  ")
    );
}

/// Whether the scan count is a constant or grows with the log. A constant that is merely large is a
/// fixed tax; a count that tracks the workspace size is an N+1 and gets worse forever.
#[test]
fn scan_counts_do_not_grow_with_the_log() {
    let mut rows: Vec<(usize, usize, usize, usize)> = Vec::new();
    for n in [4usize, 8, 16, 32] {
        let store = Arc::new(CountingStore::new());
        let engine = workspace_n(&store, n);
        let total = store.all_observations(Some(WS)).expect("obs").len();
        let (_, g) = store.measure(|| { engine.graph(Some(WS)).expect("graph"); });
        let (_, c) = store.measure(|| { engine.curation(Some(WS)).expect("curation"); });
        rows.push((total, g.observations, c.observations, c.observation_rows));
    }
    println!("\n{:>6} {:>8} {:>11} {:>14}", "obs", "graph", "curation", "curation rows");
    for (o, g, c, cr) in &rows {
        println!("{o:>6} {g:>8} {c:>11} {cr:>14}");
    }
    println!();
    let first = rows.first().expect("rows");
    let last = rows.last().expect("rows");
    assert_eq!(
        (first.1, first.2), (last.1, last.2),
        "scan counts must not depend on how much knowledge the workspace holds - \
         {} observations cost {:?} scans, {} observations cost {:?}",
        first.0, (first.1, first.2), last.0, (last.1, last.2)
    );
}

/// A wall-clock reading, for the record rather than as a gate - it measures this machine, so it is
/// `#[ignore]`d and the assertions live in the scan counts above.
#[test]
#[ignore = "wall-clock measurement, not a guard - run manually with --nocapture"]
fn read_path_wall_clock() {
    // Measured over the Cozo adapter, not the in-memory one. A scan of InMemoryStore is a clone out
    // of a map; a scan of Cozo parses a data JSON per row, embedding included. The scan count is the
    // same either way - which store you measure decides whether that count costs anything.
    let dir = std::env::temp_dir().join(format!("supragnosis-readcost-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for n in [50usize, 200, 800] {
        let path = dir.join(format!("n{n}"));
        let cozo = supragnosis_store::CozoStore::open(&path).expect("cozo");
        let store = Arc::new(CountingStore::wrapping(Box::new(cozo)));
        let engine = workspace_n(&store, n);
        let total = store.all_observations(Some(WS)).expect("obs").len();
        let t = std::time::Instant::now();
        for _ in 0..20 { engine.curation(Some(WS)).expect("curation"); }
        let curation = t.elapsed() / 20;
        let t = std::time::Instant::now();
        for _ in 0..20 { engine.graph(Some(WS)).expect("graph"); }
        let graph = t.elapsed() / 20;
        println!("{total:>5} observations: graph {graph:>10.2?}   curation {curation:>10.2?}");
    }
}

/// A read-only view of another store that hands the log back in the opposite order.
struct ReversedStore(Arc<InMemoryStore>);

impl KnowledgeStore for ReversedStore {
    fn all_observations(&self, ws: Option<&str>) -> Result<Vec<Observation>, StoreError> {
        let mut v = self.0.all_observations(ws)?;
        v.reverse();
        Ok(v)
    }
    fn all_entities(&self, ws: Option<&str>) -> Result<Vec<Entity>, StoreError> { self.0.all_entities(ws) }
    fn all_relations(&self, ws: Option<&str>) -> Result<Vec<Relation>, StoreError> { self.0.all_relations(ws) }
    fn add_observation(&self, o: Observation) -> Result<(), StoreError> { self.0.add_observation(o) }
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> { self.0.get_observation(id) }
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError> { self.0.get_entity(id) }
    fn put_entity(&self, e: Entity) -> Result<(), StoreError> { self.0.put_entity(e) }
    fn add_relation(&self, r: Relation) -> Result<(), StoreError> { self.0.add_relation(r) }
    fn relations_of(&self, id: &str) -> Result<Vec<Relation>, StoreError> { self.0.relations_of(id) }
    fn search(&self, q: &str, ws: Option<&str>, n: usize) -> Result<Vec<SearchHit>, StoreError> { self.0.search(q, ws, n) }
    fn traverse(&self, id: &str, d: usize, n: usize) -> Result<Vec<TraverseHit>, StoreError> { self.0.traverse(id, d, n) }
}

/// guard (P16): the read surfaces must not depend on the order the store enumerates the log in.
///
/// P16 names the iteration order of an internal structure leaking into a response as a violation by
/// itself, and the stores do not agree on one order: `InMemoryStore` enumerates a HashMap, Cozo a
/// Datalog result. Sharing one loaded copy across the folds pins whichever order the store gave for
/// the whole call rather than per fold, which is only harmless if no answer depends on it.
///
/// ONE log, read two ways - the reversed store is a view over the same rows, so the enumeration
/// order is the only thing that differs. Building two workspaces through two stores would not test
/// this: it would test two different logs, since anything that picks a target by position picks a
/// different one.
#[test]
fn read_surfaces_do_not_depend_on_enumeration_order() {
    // Every observation lands in the same millisecond, so the ordering HLCs tie and resolution has
    // to fall through to its stable-key tiebreak. Without the tie the tiebreak never runs and this
    // test passes while asserting nothing: verified by removing `Reverse(observation)` from
    // TierWeighted, which leaves a clock-stepped fixture green and this one failing.
    struct Frozen;
    impl supragnosis_core::Clock for Frozen {
        fn now_millis(&self) -> supragnosis_core::Timestamp { 1_000 }
    }
    let inner = Arc::new(InMemoryStore::new());
    let writer = Engine::new(inner.clone(), "host-a", WS).with_clock(Arc::new(Frozen));
    // Conflicting kinds for one entity at a tied tier - the case resolution actually has to decide.
    for kind in ["Concept", "Tool", "Library"] {
        writer
            .observe(ObserveInput {
                content: format!("cozo is a {kind}"),
                workspace: None,
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput {
                    name: "Cozo".into(),
                    kind: Some(kind.into()),
                    description: None,
                }],
                relations: vec![RelationInput {
                    from: "Cozo".into(),
                    kind: "relates_to".into(),
                    to: format!("Peer {kind}"),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                }],
            })
            .expect("observe");
    }

    let forward = Engine::new(inner.clone(), "host-a", WS);
    let backward = Engine::new(Arc::new(ReversedStore(inner.clone())), "host-a", WS);
    let ws = Some(WS);
    let render = |e: &Engine| {
        (
            serde_json::to_string(&e.graph(ws).unwrap()).unwrap(),
            serde_json::to_string(&e.curation(ws).unwrap()).unwrap(),
            serde_json::to_string(&e.hypergraph(ws).unwrap()).unwrap(),
            serde_json::to_string(&e.types(ws).unwrap()).unwrap(),
            serde_json::to_string(&e.observation_log(ws, None, None).unwrap()).unwrap(),
        )
    };
    let f = render(&forward);
    let b = render(&backward);
    for (name, x, y) in [
        ("graph", &f.0, &b.0),
        ("curation", &f.1, &b.1),
        ("hypergraph", &f.2, &b.2),
        ("types", &f.3, &b.3),
        ("observation_log", &f.4, &b.4),
    ] {
        assert_eq!(x, y, "{name} changed when the store enumerated the log backwards (P16)");
    }
}

#[test]
#[ignore = "measurement: what an embedding costs a fold that never reads it"]
fn embedding_cost_on_the_read_path() {
    let dir = std::env::temp_dir().join(format!("supragnosis-embcost-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for embedded in [false, true] {
        let path = dir.join(if embedded { "with" } else { "without" });
        let cozo = supragnosis_store::CozoStore::open(&path).expect("cozo");
        let store = Arc::new(cozo);
        let mut engine = Engine::new(store.clone(), "host-a", WS);
        if embedded {
            engine = engine.with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::new(384)));
        }
        for i in 0..400 {
            engine.observe(ObserveInput {
                content: format!("fact number {i} with some prose so the row is not trivial"),
                workspace: None, source_ref: None, confidence: None, on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { name: format!("E{i}"), kind: Some("Concept".into()), description: None }],
                relations: vec![],
            }).expect("observe");
        }
        let t = std::time::Instant::now();
        for _ in 0..20 { store.all_observations(Some(WS)).expect("scan"); }
        let scan = t.elapsed() / 20;
        let t = std::time::Instant::now();
        for _ in 0..20 { engine.graph(Some(WS)).expect("graph"); }
        let graph = t.elapsed() / 20;
        println!("embedding={embedded:<5} scan {scan:>10.2?}   graph {graph:>10.2?}");
    }
}

#[test]
#[ignore = "measurement: what one viewer poll costs the server"]
fn viewer_poll_cost() {
    let dir = std::env::temp_dir().join(format!("supragnosis-poll-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for n in [100usize, 300] {
        let cozo = supragnosis_store::CozoStore::open(dir.join(format!("n{n}"))).expect("cozo");
        let store = Arc::new(cozo);
        let engine = Engine::new(store.clone(), "host-a", WS)
            .with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::new(384)));
        for i in 0..n {
            engine.observe(ObserveInput {
                content: format!("fact number {i} with prose so the row is not trivial"),
                workspace: None, source_ref: None, confidence: None, on_behalf_of: None,
                derived_from: vec![],
                entities: vec![
                    EntityInput { name: format!("E{i}"), kind: Some("Concept".into()), description: None },
                    EntityInput { name: format!("E{}", i + 1), kind: None, description: None },
                ],
                relations: vec![RelationInput {
                    from: format!("E{i}"), kind: "relates_to".into(), to: format!("E{}", i + 1),
                    description: None, valid_from: None, valid_to: None }],
            }).expect("observe");
        }
        let ws = Some(WS);
        let each = |label: &str, f: &dyn Fn()| {
            let t = std::time::Instant::now();
            for _ in 0..5 { f(); }
            (label.to_string(), t.elapsed() / 5)
        };
        let parts = vec![
            each("graph", &|| { engine.graph(ws).unwrap(); }),
            each("hypergraph", &|| { engine.hypergraph(ws).unwrap(); }),
            each("types", &|| { engine.types(ws).unwrap(); }),
            each("curation (review tab)", &|| { engine.curation(ws).unwrap(); }),
            each("observations (log tab)", &|| { engine.observation_log(ws, None, None).unwrap(); }),
        ];
        println!("\n--- {n} observations, 384-dim embeddings ---");
        let mut closed = std::time::Duration::ZERO;
        for (label, d) in &parts {
            println!("  {label:<26} {d:>10.2?}");
            if matches!(label.as_str(), "graph" | "hypergraph" | "types") { closed += *d; }
        }
        let all: std::time::Duration = parts.iter().map(|(_, d)| *d).sum();
        println!("  {:<26} {closed:>10.2?}  (every 2.5s)", "= poll, panels closed");
        println!("  {:<26} {all:>10.2?}  (every 2.5s)", "= poll, panels open");
    }
}

#[test]
#[ignore = "measurement: what makes curation superlinear"]
fn curation_cost_breakdown() {
    use supragnosis_core::EmbeddingProvider;
    let dir = std::env::temp_dir().join(format!("supragnosis-cur-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for embedded in [false, true] {
        for n in [100usize, 200, 300] {
            // Match how the CLI assembles these: an embedder means the store is opened WITH it, so
            // the HNSW indexes exist and semantic lookup is ANN rather than brute force. Opening
            // plain and attaching the embedder only to the engine measures a configuration the
            // product never ships.
            let path = dir.join(format!("e{embedded}n{n}"));
            let emb = supragnosis_embed::HashingEmbedder::new(384);
            let store: Arc<supragnosis_store::CozoStore> = Arc::new(if embedded {
                supragnosis_store::CozoStore::open_with_embedder(&path, &emb.id(), emb.dimensions())
                    .expect("cozo+hnsw")
            } else {
                supragnosis_store::CozoStore::open(&path).expect("cozo")
            });
            let mut engine = Engine::new(store.clone(), "host-a", WS);
            if embedded {
                engine = engine.with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::new(384)));
            }
            for i in 0..n {
                engine.observe(ObserveInput {
                    content: format!("fact {i}"),
                    workspace: None, source_ref: None, confidence: None, on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput { name: format!("E{i}"), kind: Some("Concept".into()), description: None }],
                    relations: vec![],
                }).expect("observe");
            }
            let t = std::time::Instant::now();
            for _ in 0..3 { engine.curation(Some(WS)).unwrap(); }
            println!("embedder={embedded:<5} n={n:<4} curation {:>10.2?}", t.elapsed() / 3);
        }
    }
}

#[test]
#[ignore = "measurement: per-item store queries inside one read"]
fn per_item_queries_in_one_read() {
    for n in [50usize, 100, 200] {
        let inner = InMemoryStore::new();
        let store = Arc::new(CountingStore::wrapping(Box::new(inner)));
        let engine = Engine::new(store.clone(), "host-a", WS)
            .with_embedder(Arc::new(supragnosis_embed::HashingEmbedder::new(384)));
        for i in 0..n {
            engine.observe(ObserveInput {
                content: format!("fact {i}"),
                workspace: None, source_ref: None, confidence: None, on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { name: format!("E{i}"), kind: Some("Concept".into()), description: None }],
                relations: vec![],
            }).expect("observe");
        }
        let (_, c) = store.measure(|| { engine.curation(Some(WS)).unwrap(); });
        println!("n={n:<4} curation: log scans {} / semantic queries {}", c.observations, c.semantic_queries);
    }
}

#[test]
#[ignore = "measurement: how the merge band's candidate set differs from an exact scan"]
fn merge_band_candidates_vs_exact() {
    use supragnosis_core::EmbeddingProvider;
    let emb = supragnosis_embed::HashingEmbedder::new(384);
    // A cluster of 12 near-identical names, so more than MERGE_BAND_K (8) neighbours sit above
    // SIM_CANDIDATE for each member - plus unrelated entities that must NOT be suggested.
    // HashingEmbedder is a bag of tokens, so names sharing n-1 of n tokens sit at exactly (n-1)/n.
    // Eight tokens puts the cluster at 0.875, comfortably over SIM_CANDIDATE (0.85); four would sit
    // at 0.75 and nothing would be a candidate at all.
    let cluster: Vec<String> =
        (0..12).map(|i| format!("Alpha Ingest Service Node Cluster Primary Region {i}")).collect();
    let others: Vec<String> =
        (0..8).map(|i| format!("Zeta Archive Vault Shard Cold Storage Zone {i}")).collect();

    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS).with_embedder(Arc::new(
        supragnosis_embed::HashingEmbedder::new(384),
    ));
    for name in cluster.iter().chain(others.iter()) {
        engine.observe(ObserveInput {
            content: format!("mentions {name}"),
            workspace: None, source_ref: None, confidence: None, on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { name: name.clone(), kind: None, description: None }],
            relations: vec![],
        }).expect("observe");
    }

    // What the band reports today. Note this is InMemoryStore, whose semantic search is exhaustive -
    // so the only thing separating it from the exact scan below is the top-K cut, with no
    // approximation error mixed in. Over Cozo the HNSW index adds a second, separate source of
    // misses that this fixture deliberately does not measure.
    let rep = engine.curation(Some(WS)).expect("curation");
    let mut band: Vec<(String, String)> = rep.merge_suggestions.iter()
        .map(|s| { let (a, b) = (s.a_name.clone(), s.b_name.clone());
                   if a <= b { (a, b) } else { (b, a) } })
        .collect();
    band.sort();

    // What an exact all-pairs scan above the same threshold would report.
    let ents = store.all_entities(Some(WS)).expect("entities");
    let cos = |a: &[f32], b: &[f32]| {
        let (mut d, mut na, mut nb) = (0f32, 0f32, 0f32);
        for i in 0..a.len() { d += a[i]*b[i]; na += a[i]*a[i]; nb += b[i]*b[i]; }
        if na == 0.0 || nb == 0.0 { 0.0 } else { d / (na.sqrt() * nb.sqrt()) }
    };
    let mut exact: Vec<(String, String)> = Vec::new();
    for i in 0..ents.len() {
        for j in (i + 1)..ents.len() {
            let (Some(x), Some(y)) = (&ents[i].embedding, &ents[j].embedding) else { continue };
            if cos(x, y) >= 0.85 {
                let (a, b) = (ents[i].canonical_name.clone(), ents[j].canonical_name.clone());
                exact.push(if a <= b { (a, b) } else { (b, a) });
            }
        }
    }
    exact.sort();

    let band_set: std::collections::BTreeSet<_> = band.iter().cloned().collect();
    let exact_set: std::collections::BTreeSet<_> = exact.iter().cloned().collect();
    let missed: Vec<_> = exact_set.difference(&band_set).cloned().collect();
    let extra: Vec<_> = band_set.difference(&exact_set).cloned().collect();

    println!("\n{} entities ({} in one near-identical cluster), embedder dim {}",
             ents.len(), cluster.len(), emb.dimensions());
    println!("  band (top-{MERGE_BAND_K_ECHO} neighbours per entity): {} pairs", band_set.len());
    println!("  exact (all pairs >= 0.85):            {} pairs", exact_set.len());
    println!("  in exact but not reported by the band: {}", missed.len());
    for p in missed.iter().take(6) { println!("      {} <-> {}", p.0, p.1); }
    if missed.len() > 6 { println!("      ... and {} more", missed.len() - 6); }
    println!("  reported by the band but not exact:    {}", extra.len());
    for p in extra.iter().take(6) { println!("      {} <-> {}", p.0, p.1); }
}
const MERGE_BAND_K_ECHO: usize = 8;
