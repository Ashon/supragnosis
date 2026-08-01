//! Port conformance - one suite, every adapter.
//!
//! [`supragnosis_core::KnowledgeStore`] states a contract in prose: absence is not failure
//! (Principle 5), the same query on the same state gives the same response, and the iteration order
//! of an internal data structure must not leak into a result (Principle 16).
//!
//! That contract was checked per adapter, in each adapter's own `mod tests`, with different cases
//! and different counts. Cross-adapter parity existed only where a divergence had already bitten:
//! `traverse`, guarded by two parity tests that live in the Cozo adapter's test module. The
//! enumeration order was a known divergence rather than an unknown one. The P16 clause in
//! `principle_coverage.rs` names it outright ("InMemory enumerates a HashMap, Cozo a Datalog
//! result"), and the defence chosen was to prove no engine fold depends on the order
//! (`read_surfaces_do_not_depend_on_enumeration_order`) instead of making the port promise one.
//!
//! This suite takes the other decision. A hazard every consumer must route around is worse than a
//! promise the port keeps, especially with a third adapter arriving: "do not depend on the order"
//! has to be re-proved by each new reader, while "rows come back by id" is proved once, here, by
//! everyone who implements the trait.
//!
//! Every case takes `&dyn KnowledgeStore` and is run against every adapter by [`for_each_adapter`].
//! A new adapter earns its coverage by being added to that one list; it does not get to bring its
//! own idea of the contract with it.
//!
//! Cases assert the PORT, never an adapter's internals. Anything only one backend can do (Cozo's
//! HNSW index, its Datalog encoding) stays in that adapter's own tests - this file is the part that
//! has to stay true when the backend is replaced.

use std::path::PathBuf;

use supragnosis_core::{
    Entity, Hlc, KnowledgeStore, Observation, Provenance, Relation, SearchHit, SearchHitKind,
    SyncMeta, TrustTier, VersionVector,
};
use supragnosis_store::{CozoStore, InMemoryStore};

// ---------------------------------------------------------------------------
// harness
// ---------------------------------------------------------------------------

/// A store under test, plus whatever the adapter needs kept alive (a temp directory for the
/// file-backed ones). The store is dropped before the directory is removed, so a backend holding a
/// lock file releases it first.
struct Fixture {
    store: Option<Box<dyn KnowledgeStore>>,
    dir: Option<PathBuf>,
}

impl Fixture {
    fn store(&self) -> &dyn KnowledgeStore {
        self.store.as_deref().expect("store taken before use")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// A unique temp path per call. A process-atomic counter rather than wall-clock alone: two
/// concurrently running cases would otherwise be able to grab the same nanosecond and collide on a
/// backend lock file.
fn tmp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before the unix epoch")
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "supragnosis-conformance-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ))
}

/// One adapter's entry in the roster: its name, and how to stand a fresh one up for a case.
type Adapter = (&'static str, fn(&str) -> Fixture);

/// Every adapter that claims to implement the port, by name. Adding one here is what subscribes it
/// to the whole suite.
///
/// Each case is a plain `#[test] fn` rather than a macro invocation on purpose: the coverage
/// registry finds a declared guard by scanning its sources for the literal `fn <name>(`
/// (`principle_coverage.rs`), which is a deliberately blunt check so that renaming a test cannot
/// silently un-guard a clause. A macro that generated these names would defeat it, and the few
/// lines saved are not worth teaching that scan a special case.
fn for_each_adapter(f: impl Fn(&dyn KnowledgeStore)) {
    let adapters: Vec<Adapter> = vec![
        ("in_memory", |_tag| Fixture {
            store: Some(Box::new(InMemoryStore::new())),
            dir: None,
        }),
        ("cozo", |tag| {
            let dir = tmp_dir(tag);
            let store = CozoStore::open(&dir).expect("cozo open");
            Fixture {
                store: Some(Box::new(store)),
                dir: Some(dir),
            }
        }),
    ];
    for (name, build) in adapters {
        let fx = build(name);
        // Printed before the body runs, so a panic is attributable: a case that fails on one backend
        // and passes on the other is the single most useful thing this suite can report.
        println!("--- adapter `{name}`");
        f(fx.store());
    }
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

const WS: &str = "ws1";
const WS_OTHER: &str = "ws2";

fn prov_in(workspace: &str) -> Provenance {
    Provenance {
        host: "host-a".into(),
        on_behalf_of: Some("ashon".into()),
        workspace: workspace.into(),
        source_ref: None,
        observed_at: 1,
        confidence: Some(1.0),
        trust_tier: TrustTier::default(),
        sync: None,
    }
}

fn prov() -> Provenance {
    prov_in(WS)
}

fn ent_in(workspace: &str, name: &str) -> Entity {
    Entity {
        id: Entity::make_id(workspace, name),
        kind: "Concept".into(),
        canonical_name: name.into(),
        aliases: vec![],
        description: None,
        properties: serde_json::Value::Null,
        provenance: vec![prov_in(workspace)],
        embedding: None,
    }
}

fn ent(name: &str) -> Entity {
    ent_in(WS, name)
}

fn rel_in(workspace: &str, from: &str, kind: &str, to: &str) -> Relation {
    let (from, to) = (
        Entity::make_id(workspace, from),
        Entity::make_id(workspace, to),
    );
    Relation {
        id: Relation::make_id(&from, kind, &to),
        from,
        to,
        kind: kind.into(),
        description: None,
        provenance: prov_in(workspace),
        valid_from: None,
        valid_to: None,
    }
}

fn rel(from: &str, kind: &str, to: &str) -> Relation {
    rel_in(WS, from, kind, to)
}

fn obs_in(workspace: &str, content: &str) -> Observation {
    Observation::new(content.into(), prov_in(workspace))
}

fn obs(content: &str) -> Observation {
    obs_in(WS, content)
}

fn eid(name: &str) -> String {
    Entity::make_id(WS, name)
}

/// A search result reduced to what the port promises: which thing, in what order. Score scale is
/// explicitly per-surface (`SearchHit::score` doc), so it is not part of the contract.
fn hit_keys(hits: &[SearchHit]) -> Vec<(SearchHitKind, String)> {
    hits.iter().map(|h| (h.kind, h.id.clone())).collect()
}

fn is_sorted_by_id<T>(rows: &[T], id: impl Fn(&T) -> &str) -> bool {
    rows.windows(2).all(|w| id(&w[0]) <= id(&w[1]))
}

// ---------------------------------------------------------------------------
// Principle 5 - absence is not failure
// ---------------------------------------------------------------------------

/// Principle 5: a missing id is `Ok(None)` and an empty scope is `Ok(vec![])`. `Err` is reserved
/// for a backend failure, so that a caller can tell "unknown" from "cannot answer". An adapter
/// that reported absence as an error would make every open-world read a false negative.
#[test]
fn absence_reads_as_absence_never_as_error() {
    for_each_adapter(|store| {
        assert!(store.get_entity("no-such-entity").expect("get_entity").is_none());
        assert!(store.get_observation("no-such-observation").expect("get_observation").is_none());
        assert!(store.relations_of("no-such-entity").expect("relations_of").is_empty());
        assert!(store.all_entities(Some("no-such-workspace")).expect("all_entities").is_empty());
        assert!(store.all_relations(Some("no-such-workspace")).expect("all_relations").is_empty());
        assert!(store.all_observations(Some("no-such-workspace")).expect("all_observations").is_empty());
        assert!(store.search("nothing matches this", None, 10).expect("search").is_empty());
        assert!(store.traverse("no-such-entity", 3, 10).expect("traverse").is_empty());
    });
}

// ---------------------------------------------------------------------------
// Principle 3 - re-arrival absorbs, it does not overwrite
// ---------------------------------------------------------------------------

/// Principle 3 merge norm: two observations at the same content address are one row whose
/// attestations and lineage are a monotonic union - never an overwrite. Principle 16: the union
/// is commutative, so arrival order cannot change the result. Checked here rather than per
/// adapter because "add_observation absorbs" is the port's promise; an adapter that silently
/// replaced the row would destroy provenance and only fail much later, in a fold.
#[test]
fn reobservation_absorbs_attestations_and_lineage() {
    for_each_adapter(|store| {
        let make = |host: &str, derived: &str| {
            let mut o = Observation::new("the same fact".into(), Provenance { host: host.into(), ..prov() });
            o.derived_from = vec![derived.into()];
            o
        };
        let id = make("host-a", "o1").id.clone();

        store.add_observation(make("host-a", "o1")).expect("first arrival");
        store.add_observation(make("host-b", "o2")).expect("second arrival");

        let got = store.get_observation(&id).expect("get").expect("absorbed row");
        let mut hosts: Vec<&str> = got.provenance.iter().map(|p| p.host.as_str()).collect();
        hosts.sort_unstable();
        assert_eq!(hosts, ["host-a", "host-b"], "both attestations survive the re-arrival");
        assert_eq!(got.derived_from, vec!["o1".to_string(), "o2".to_string()], "lineage unions");
        assert_eq!(
            store.all_observations(Some(WS)).expect("enumerate").len(),
            1,
            "one content address is one log row"
        );
    });
}

/// The same union, reached from the opposite arrival order. Convergence is the property that
/// makes replication topology-independent (Principle 16), so it is checked as a difference
/// between two stores rather than as a state of one.
#[test]
fn reobservation_converges_regardless_of_arrival_order() {
    for_each_adapter(|store| {
        let make = |host: &str, conf: f32| {
            Observation::new(
                "order independent".into(),
                Provenance { host: host.into(), confidence: Some(conf), ..prov() },
            )
        };
        let id = make("host-a", 0.9).id.clone();
        store.add_observation(make("host-b", 0.1)).expect("b first");
        store.add_observation(make("host-a", 0.9)).expect("a second");
        let reversed = store.get_observation(&id).expect("get").expect("row");

        let forward = InMemoryStore::new();
        forward.add_observation(make("host-a", 0.9)).expect("a first");
        forward.add_observation(make("host-b", 0.1)).expect("b second");
        let forward = forward.get_observation(&id).expect("get").expect("row");

        let key = |o: &Observation| -> Vec<(String, Option<u32>)> {
            o.provenance.iter().map(|p| (p.host.clone(), p.confidence.map(f32::to_bits))).collect()
        };
        assert_eq!(key(&reversed), key(&forward), "arrival order must not survive into the row");
    });
}

// ---------------------------------------------------------------------------
// upsert semantics
// ---------------------------------------------------------------------------

/// `put_entity` is an upsert keyed on `entity.id` - the port says so in one line, and both the
/// projection and reprojection depend on it (a second projection pass must not double a node).
#[test]
fn put_entity_upserts_on_id() {
    for_each_adapter(|store| {
        store.put_entity(ent("alpha")).expect("first put");
        let mut second = ent("alpha");
        second.kind = "Project".into();
        second.description = Some("the second write wins".into());
        store.put_entity(second).expect("second put");

        let got = store.get_entity(&eid("alpha")).expect("get").expect("row");
        assert_eq!(got.kind, "Project", "the later write is what the row reads back as");
        assert_eq!(got.description.as_deref(), Some("the second write wins"));
        assert_eq!(store.all_entities(Some(WS)).expect("enumerate").len(), 1, "upsert, not insert");
    });
}

/// `add_relation` is likewise keyed on the relation id, which is derived from
/// (from, normalized kind, to). So the same edge asserted twice - including through a different
/// spelling of the kind - is one edge, not two.
#[test]
fn add_relation_upserts_on_id_across_kind_spellings() {
    for_each_adapter(|store| {
        store.add_relation(rel("alpha", "depends_on", "beta")).expect("first");
        let mut restated = rel("alpha", "dependsOn", "beta");
        restated.description = Some("restated with a different spelling".into());
        store.add_relation(restated).expect("second");

        let all = store.all_relations(Some(WS)).expect("enumerate");
        assert_eq!(all.len(), 1, "spelling jitter converges to one edge id");
        assert_eq!(all[0].description.as_deref(), Some("restated with a different spelling"));
    });
}

/// `relations_of` is both directions - the port says "whose from or to is entity_id" - and the
/// result is ordered by id, because a caller that renders an inspector panel must not see the
/// rows move between two identical reads (Principle 16).
#[test]
fn relations_of_returns_both_directions_ordered_by_id() {
    for_each_adapter(|store| {
        for n in ["alpha", "beta", "gamma"] {
            store.put_entity(ent(n)).expect("put");
        }
        store.add_relation(rel("alpha", "depends_on", "beta")).expect("in");
        store.add_relation(rel("beta", "part_of", "gamma")).expect("out");

        let rels = store.relations_of(&eid("beta")).expect("relations_of");
        assert_eq!(rels.len(), 2, "beta is an endpoint of both edges, incoming and outgoing");
        assert!(is_sorted_by_id(&rels, |r| &r.id), "row order must not leak: {rels:?}");
        assert_eq!(store.relations_of(&eid("alpha")).expect("relations_of").len(), 1);
    });
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

/// Keyword search is scoped by workspace, and the scope is a filter on the result, not a hint:
/// a hit in another workspace must not leak. `None` is every workspace (the local trust
/// surface's global read).
#[test]
fn search_scopes_by_workspace() {
    for_each_adapter(|store| {
        store.add_observation(obs_in(WS, "rust ownership notes")).expect("ws1");
        store.add_observation(obs_in(WS_OTHER, "rust ownership notes elsewhere")).expect("ws2");

        assert_eq!(store.search("ownership", Some(WS), 10).expect("scoped").len(), 1);
        assert_eq!(store.search("ownership", Some(WS_OTHER), 10).expect("scoped").len(), 1);
        assert_eq!(store.search("ownership", None, 10).expect("global").len(), 2);
        assert!(store.search("ownership", Some("ws-absent"), 10).expect("empty scope").is_empty());
    });
}

/// An entity is reachable by an alias, not only by its canonical name - spelling variants
/// accumulate as aliases (resolution-identity.md IR1), so a recall surface that only matched the
/// canonical spelling would hide the node behind whichever name won resolution.
#[test]
fn search_matches_canonical_name_and_alias() {
    for_each_adapter(|store| {
        let mut e = ent("Postgres");
        e.aliases = vec!["PostgreSQL".into(), "pg".into()];
        store.put_entity(e).expect("put");

        for q in ["postgres", "postgresql", "pg"] {
            let hits = store.search(q, Some(WS), 10).expect("search");
            assert_eq!(
                hit_keys(&hits),
                vec![(SearchHitKind::Entity, eid("Postgres"))],
                "query `{q}` must reach the entity"
            );
        }
    });
}

/// Truncation is part of the answer, so it has to be reproducible: the same query against the
/// same state returns the same rows in the same order, twice (Principle 16, reproducibility).
/// A hash-ordered result would pass a length assertion and fail this one.
#[test]
fn search_truncation_is_reproducible() {
    for_each_adapter(|store| {
        for i in 0..12 {
            store.add_observation(obs(&format!("shared token, row {i}"))).expect("obs");
            store.put_entity(ent(&format!("shared-token-{i}"))).expect("ent");
        }
        let first = store.search("shared", Some(WS), 5).expect("first read");
        let second = store.search("shared", Some(WS), 5).expect("second read");
        assert_eq!(first.len(), 5, "limit is honored");
        assert_eq!(hit_keys(&first), hit_keys(&second), "two reads of one state must agree");
        // Entities rank above observations, and ties inside a rank break by id.
        let entity_ids: Vec<String> = first
            .iter()
            .filter(|h| h.kind == SearchHitKind::Entity)
            .map(|h| h.id.clone())
            .collect();
        let mut sorted = entity_ids.clone();
        sorted.sort();
        assert_eq!(entity_ids, sorted, "ties break by id, not by row order");
    });
}

// ---------------------------------------------------------------------------
// traverse
// ---------------------------------------------------------------------------

/// Traversal follows the edge direction (from -> to) and reports hop distance. Walking the graph
/// backwards would silently change what a `traverse` tool call means.
#[test]
fn traverse_follows_direction_and_reports_depth() {
    for_each_adapter(|store| {
        for n in ["a", "b", "c"] {
            store.put_entity(ent(n)).expect("put");
        }
        store.add_relation(rel("a", "depends_on", "b")).expect("a->b");
        store.add_relation(rel("b", "depends_on", "c")).expect("b->c");

        let hits = store.traverse(&eid("a"), 5, 100).expect("forward");
        let depths: Vec<(String, usize)> = hits.iter().map(|h| (h.id.clone(), h.depth)).collect();
        assert_eq!(
            depths,
            vec![(eid("b"), 1), (eid("c"), 2)],
            "b is one hop, c is two, and the start node is not its own hit"
        );
        assert!(
            store.traverse(&eid("c"), 5, 100).expect("backward").is_empty(),
            "direction is not symmetric"
        );
    });
}

/// `max_depth` bounds the walk, and `limit` truncates it - in (depth, id) order, so truncation
/// keeps the NEARER neighbours. Truncating in row order would drop near nodes in favour of far
/// ones and make the answer depend on storage layout.
#[test]
fn traverse_bounds_depth_and_truncates_nearest_first() {
    for_each_adapter(|store| {
        // Hash-selected fixture, the same trick the Cozo adapter's own parity test uses: the
        // candidate whose derived id sorts FIRST is planted as the depth-2 grandchild. Under a
        // regression to id-order truncation it would necessarily surface and push a depth-1
        // neighbour out, so this case fails every time rather than only when the hashes happen to
        // fall that way. A fixture of plain `near-*`/`far-*` names would leave that to chance, and
        // an id hash is deterministic - it would either always catch the regression or always miss
        // it, with no way to tell which from a green run.
        let mut names: Vec<String> = (0..24).map(|i| format!("node-{i:02}")).collect();
        names.sort_by_key(|n| Entity::make_id(WS, n));
        let grandchild = names[0].clone();
        let children: Vec<String> = names[1..5].to_vec();

        store.put_entity(ent("root")).expect("put");
        for c in &children {
            store.put_entity(ent(c)).expect("put");
            store.add_relation(rel("root", "depends_on", c)).expect("hop 1");
        }
        store.put_entity(ent(&grandchild)).expect("put");
        store.add_relation(rel(&children[0], "depends_on", &grandchild)).expect("hop 2");

        let shallow = store.traverse(&eid("root"), 1, 100).expect("depth 1");
        assert_eq!(shallow.len(), 4, "max_depth stops the walk before the grandchild");
        assert!(shallow.iter().all(|h| h.depth == 1));

        let truncated = store.traverse(&eid("root"), 3, 4).expect("limited");
        assert_eq!(truncated.len(), 4);
        assert!(
            !truncated.iter().any(|h| h.id == eid(&grandchild)),
            "the min-id node sits at depth 2, so id-order truncation would surface it: {truncated:?}"
        );
        assert!(
            truncated.iter().all(|h| h.depth == 1),
            "limit keeps the nearest hops, not whichever rows came back first: {truncated:?}"
        );
        let ids: Vec<String> = truncated.iter().map(|h| h.id.clone()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "within a depth, order is by id");
    });
}

/// An edge endpoint with no projected entity row is traversed THROUGH but never emitted. It is
/// reachable state - sync applies a log before the projection catches up - and the alternative
/// (emitting a hit whose name is the empty string) invents a node that the graph does not have.
#[test]
fn traverse_passes_through_an_unprojected_endpoint() {
    for_each_adapter(|store| {
        store.put_entity(ent("start")).expect("put");
        store.put_entity(ent("end")).expect("put");
        // `middle` is asserted only as a relation endpoint; no entity row is projected for it.
        store.add_relation(rel("start", "depends_on", "middle")).expect("hop 1");
        store.add_relation(rel("middle", "depends_on", "end")).expect("hop 2");

        let hits = store.traverse(&eid("start"), 3, 100).expect("traverse");
        let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        assert!(
            !ids.contains(&eid("middle").as_str()),
            "an unprojected endpoint is not described as a node"
        );
        assert_eq!(
            hits.iter().find(|h| h.id == eid("end")).map(|h| h.depth),
            Some(2),
            "but reachability still runs through it"
        );
    });
}

// ---------------------------------------------------------------------------
// enumeration
// ---------------------------------------------------------------------------

/// Enumerations are workspace-filtered, and `None` means every workspace. These are the read
/// path of the graph projection and of log replay, so a scope leak here becomes a cross-
/// workspace fold (Principle 11: the workspace is the scope of the schema).
#[test]
fn enumerations_filter_by_workspace() {
    for_each_adapter(|store| {
        for (ws, name) in [(WS, "alpha"), (WS, "beta"), (WS_OTHER, "gamma")] {
            store.put_entity(ent_in(ws, name)).expect("put");
        }
        store.add_relation(rel_in(WS, "alpha", "depends_on", "beta")).expect("rel ws1");
        store.add_relation(rel_in(WS_OTHER, "gamma", "depends_on", "delta")).expect("rel ws2");
        store.add_observation(obs_in(WS, "one")).expect("obs ws1");
        store.add_observation(obs_in(WS_OTHER, "two")).expect("obs ws2");

        assert_eq!(store.all_entities(Some(WS)).expect("e").len(), 2);
        assert_eq!(store.all_entities(Some(WS_OTHER)).expect("e").len(), 1);
        assert_eq!(store.all_entities(None).expect("e").len(), 3);
        assert_eq!(store.all_relations(Some(WS)).expect("r").len(), 1);
        assert_eq!(store.all_relations(None).expect("r").len(), 2);
        assert_eq!(store.all_observations(Some(WS)).expect("o").len(), 1);
        assert_eq!(store.all_observations(None).expect("o").len(), 2);
    });
}

/// The port forbids an internal iteration order from leaking into a response, and an enumeration
/// is a response. Ordering by id is the form that costs nothing to promise and makes the promise
/// checkable: every adapter either keeps rows in a sorted structure already or sorts on the way
/// out.
///
/// This is not pedantry about an unordered list. `all_observations` is the input to every fold on
/// the read path, and a fold that resolves a tie by "first row seen" inherits whatever order the
/// store handed it. Pinning the order here is what lets a fold be audited on its own terms.
#[test]
fn enumerations_are_ordered_by_id() {
    for_each_adapter(|store| {
        // Names chosen only so the derived ids are well spread; the assertion is on id order, which
        // is a hash and therefore unrelated to insertion order.
        for i in 0..16 {
            store.put_entity(ent(&format!("node-{i}"))).expect("put");
            store.add_observation(obs(&format!("observation {i}"))).expect("obs");
            store.add_relation(rel("node-0", "relates_to", &format!("node-{i}"))).expect("rel");
        }

        let entities = store.all_entities(Some(WS)).expect("entities");
        assert!(
            is_sorted_by_id(&entities, |e| &e.id),
            "all_entities leaked its storage order: {:?}",
            entities.iter().map(|e| &e.id).collect::<Vec<_>>()
        );

        let relations = store.all_relations(Some(WS)).expect("relations");
        assert!(
            is_sorted_by_id(&relations, |r| &r.id),
            "all_relations leaked its storage order: {:?}",
            relations.iter().map(|r| &r.id).collect::<Vec<_>>()
        );

        let observations = store.all_observations(Some(WS)).expect("observations");
        assert!(
            is_sorted_by_id(&observations, |o| &o.id),
            "all_observations leaked its storage order: {:?}",
            observations.iter().map(|o| &o.id).collect::<Vec<_>>()
        );
    });
}

// ---------------------------------------------------------------------------
// federation delta read
// ---------------------------------------------------------------------------

/// The sync delta read (federation.md Section 5): only sync-stamped attestations leave, filtered
/// by the peer's version vector, in deterministic (origin, seq) order. An unstamped attestation
/// is local history and never crosses the wire until backfill stamps it.
///
/// The port ships a default implementation over `all_observations`, which is exactly why this
/// belongs to the conformance suite rather than to one adapter: an adapter that overrides it with
/// an indexed scan has to land on the same answer.
#[test]
fn attestations_since_filters_by_version_vector() {
    for_each_adapter(|store| {
        let stamp = |seq: u64| SyncMeta {
            origin_node: "node-a".into(),
            origin_seq: seq,
            hlc: Hlc { wall: seq, counter: 0, node: "node-a".into() },
            signature: "sig".into(),
            lineage: Vec::new(),
        };
        for (content, seq) in [("fact one", 1u64), ("fact two", 2u64)] {
            let p = Provenance { sync: Some(stamp(seq)), ..prov() };
            store.add_observation(Observation::new(content.into(), p)).expect("stamped");
        }
        store.add_observation(obs("local only")).expect("unstamped");

        let all = store.attestations_since(WS, &VersionVector::default()).expect("delta");
        assert_eq!(all.len(), 2, "an unstamped attestation is local-only");
        let seqs: Vec<u64> = all
            .iter()
            .filter_map(|e| e.attestation.sync.as_ref().map(|s| s.origin_seq))
            .collect();
        assert_eq!(seqs, vec![1, 2], "ordered by (origin, seq)");

        let mut vv = VersionVector::default();
        vv.advance("node-a", WS, 1);
        let rest = store.attestations_since(WS, &vv).expect("delta");
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].content, "fact two");

        vv.advance("node-a", WS, 2);
        assert!(store.attestations_since(WS, &vv).expect("delta").is_empty());
        assert!(
            store.attestations_since(WS_OTHER, &VersionVector::default()).expect("delta").is_empty(),
            "the delta read is scoped by workspace (the sharing boundary's input)"
        );
    });
}

// ---------------------------------------------------------------------------
// semantic recall
// ---------------------------------------------------------------------------

/// Semantic recall ranks by cosine similarity and excludes rows with no embedding - it widens
/// recall (Principle 19) and is never a filter that invents membership. The port's default
/// implementation returns nothing, so an adapter that stores vectors is opting in to this
/// contract.
#[test]
fn semantic_recall_ranks_by_similarity_and_skips_unembedded() {
    for_each_adapter(|store| {
        let mut near = obs("near the query");
        near.embedding = Some(vec![1.0, 0.0, 0.0]);
        let mut far = obs("far from the query");
        far.embedding = Some(vec![0.0, 1.0, 0.0]);
        let plain = obs("no embedding at all");
        let (near_id, far_id, plain_id) = (near.id.clone(), far.id.clone(), plain.id.clone());
        store.add_observation(near).expect("near");
        store.add_observation(far).expect("far");
        store.add_observation(plain).expect("plain");

        let hits = store.search_semantic(&[1.0, 0.0, 0.0], Some(WS), 10).expect("semantic");
        // An adapter that stores no vectors legitimately returns nothing (the port's default).
        if hits.is_empty() {
            return;
        }
        let ids: Vec<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids.first(), Some(&near_id.as_str()), "nearest first: {hits:?}");
        assert!(ids.contains(&far_id.as_str()));
        assert!(!ids.contains(&plain_id.as_str()), "an unembedded row is not a candidate");
        assert!(
            store.search_semantic(&[1.0, 0.0, 0.0], Some(WS_OTHER), 10).expect("scoped").is_empty(),
            "semantic recall is workspace-scoped like every other read"
        );
    });
}

/// The same contract for entity-name recall, which is what reaches a node no observation mentions
/// lexically.
#[test]
fn semantic_entity_recall_ranks_by_similarity() {
    for_each_adapter(|store| {
        let mut near = ent("near");
        near.embedding = Some(vec![1.0, 0.0, 0.0]);
        let mut far = ent("far");
        far.embedding = Some(vec![0.0, 1.0, 0.0]);
        store.put_entity(near).expect("near");
        store.put_entity(far).expect("far");
        store.put_entity(ent("unembedded")).expect("plain");

        let hits = store.search_semantic_entities(&[1.0, 0.0, 0.0], Some(WS), 10).expect("semantic");
        if hits.is_empty() {
            return;
        }
        assert_eq!(hits.first().map(|h| h.id.as_str()), Some(eid("near").as_str()));
        assert!(
            hits.iter().all(|h| h.kind == SearchHitKind::Entity),
            "entity recall returns entity hits"
        );
        assert!(!hits.iter().any(|h| h.id == eid("unembedded")));
    });
}
