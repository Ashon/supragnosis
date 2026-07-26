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
}

/// One read surface's cost, in enumerations of each table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    observations: usize,
    observation_rows: usize,
    entities: usize,
    relations: usize,
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
        }
    }

    fn reset(&self) {
        for c in [&self.obs_scans, &self.obs_rows, &self.entity_scans, &self.relation_scans] {
            c.store(0, Ordering::SeqCst);
        }
    }

    fn cost(&self) -> Cost {
        Cost {
            observations: self.obs_scans.load(Ordering::SeqCst),
            observation_rows: self.obs_rows.load(Ordering::SeqCst),
            entities: self.entity_scans.load(Ordering::SeqCst),
            relations: self.relation_scans.load(Ordering::SeqCst),
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
