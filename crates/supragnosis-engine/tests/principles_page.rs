//! docs/design-system.md U12: what the principles page (site/principles/index.html) shows as the
//! product's behavior is the engine's answer. The page carries a case for each figure; a case marked
//! `replay` is replayed here against the engine on the in-memory adapter, with the made-up project the
//! figure draws, and every value the page's script draws from it is compared with what the engine
//! computes. A case not marked `replay` is held by the registry clauses its figure names, which
//! principle_coverage.rs checks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use supragnosis_core::{AssertionStore, Clock, Entity, Observation, Timestamp, TrustTier};
use supragnosis_engine::{
    DefineTypeInput, Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, TypeDefInput,
    TypeTarget, VerdictSurface,
};
use supragnosis_store::InMemoryStore;

const PAGE: &str = include_str!("../../../site/principles/index.html");
const WS: &str = "billing";
const MAR_3: Timestamp = 1_772_496_000_000; // 2026-03-03, when the figures' system learns things
const JAN_10: Timestamp = 1_768_003_200_000; // 2026-01-10

fn cases() -> Value {
    let at = PAGE
        .find("<script type=\"application/json\" id=\"cases\">")
        .expect("the cases block");
    let body = &PAGE[at..][PAGE[at..].find('>').unwrap() + 1..];
    serde_json::from_str(&body[..body.find("</script>").unwrap()]).expect("the cases are JSON")
}

fn values(p: u8) -> Value {
    cases()[format!("p{p}")]["values"].clone()
}

/// Time steps a second per reading, from a fixed start, so a figure's order of events is its order
/// of time and no test depends on the wall clock.
struct Steps(Timestamp, AtomicU64);
impl Clock for Steps {
    fn now_millis(&self) -> Timestamp {
        self.0 + self.1.fetch_add(1, Ordering::SeqCst) * 1000
    }
}

fn engine_on(store: &Arc<InMemoryStore>, host: &str, start: Timestamp) -> Engine {
    Engine::new(store.clone(), host, WS).with_clock(Arc::new(Steps(start, AtomicU64::new(0))))
}

fn entity(name: &str, kind: Option<&str>, description: Option<&str>) -> EntityInput {
    EntityInput {
        name: name.into(),
        kind: kind.map(Into::into),
        description: description.map(Into::into),
    }
}

fn relation(from: &str, kind: &str, to: &str) -> RelationInput {
    RelationInput {
        from: from.into(),
        kind: kind.into(),
        to: to.into(),
        description: None,
        valid_from: None,
        valid_to: None,
    }
}

fn input(content: &str, entities: Vec<EntityInput>, relations: Vec<RelationInput>) -> ObserveInput {
    ObserveInput {
        content: content.into(),
        workspace: Some(WS.into()),
        source_ref: None,
        confidence: None,
        on_behalf_of: None,
        derived_from: vec![],
        entities,
        relations,
    }
}

fn observe(engine: &Engine, i: ObserveInput) -> String {
    engine.observe(i).expect("observe").observation_id
}

fn graph_json(engine: &Engine) -> String {
    serde_json::to_string(&engine.graph(Some(WS)).expect("graph")).expect("serialize")
}

/// The page's digest of a graph: the first six hex digits of the BLAKE3 hash of its JSON, the form
/// the convergence tests compare whole (design-system.md Section 11a). The six digits are a label;
/// what is compared below is always the whole serialization.
fn digest(json: &str) -> String {
    blake3::hash(json.as_bytes()).to_hex()[..6].to_string()
}

fn node<'a>(g: &'a supragnosis_engine::GraphView, name: &str) -> &'a supragnosis_engine::GraphNode {
    g.nodes
        .iter()
        .find(|n| n.name == name)
        .unwrap_or_else(|| panic!("no node {name}"))
}

fn log_len(store: &InMemoryStore) -> usize {
    store.all_observations(Some(WS)).expect("log").len()
}

fn tier_words(t: TrustTier) -> &'static str {
    match t {
        TrustTier::Unverified => "unverified",
        TrustTier::AgentExtracted => "agent extracted",
        TrustTier::HostSigned => "host signed",
        TrustTier::HumanConfirmed => "human confirmed",
    }
}

/// The stage's project, the six entries P1, P12 and P16 draw, observed on one machine.
fn billing_entries() -> Vec<Observation> {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    let ids = [
        observe(
            &e,
            input("billing-api is an App", vec![entity("billing-api", Some("App"), None)], vec![]),
        ),
        observe(
            &e,
            input(
                "Postgres is a Database",
                vec![entity("Postgres", Some("Database"), None)],
                vec![],
            ),
        ),
        observe(
            &e,
            input(
                "billing-api uses Postgres",
                vec![entity("billing-api", None, None), entity("Postgres", None, None)],
                vec![relation("billing-api", "uses", "Postgres")],
            ),
        ),
        observe(
            &e,
            input("webhooks is a Feature", vec![entity("webhooks", Some("Feature"), None)], vec![]),
        ),
        observe(
            &e,
            input(
                "billing-api sends webhooks",
                vec![entity("billing-api", None, None), entity("webhooks", None, None)],
                vec![relation("billing-api", "sends", "webhooks")],
            ),
        ),
        observe(
            &e,
            input(
                "webhooks use idempotency keys, a Pattern",
                vec![entity("webhooks", None, None), entity("idempotency", Some("Pattern"), None)],
                vec![relation("webhooks", "uses", "idempotency")],
            ),
        ),
    ];
    ids.iter()
        .map(|id| e.get_observation(id).expect("read").expect("present"))
        .collect()
}

/// A machine that received `order` of the entries (indices may repeat), projected from its log.
fn machine(entries: &[Observation], order: &[usize]) -> String {
    let store = Arc::new(InMemoryStore::new());
    for &i in order {
        store.add_observation(entries[i].clone()).expect("append");
    }
    let e = engine_on(&store, "desktop", MAR_3);
    e.reproject(Some(WS)).expect("reproject");
    graph_json(&e)
}

fn p1() {
    let entries = billing_entries();
    let store = Arc::new(InMemoryStore::new());
    for o in &entries {
        store.add_observation(o.clone()).expect("append");
    }
    let e = engine_on(&store, "laptop", MAR_3);
    e.reproject(Some(WS)).expect("project");
    let before = graph_json(&e);
    e.reproject(Some(WS)).expect("throw the graph away and replay the log");
    assert_eq!(graph_json(&e), before, "P1: replaying the log builds the same graph");
    assert_eq!(values(1)["digest"], digest(&before), "P1: the page's digest is the engine's");
    let g = e.graph(Some(WS)).unwrap();
    for (name, kind) in [
        ("billing-api", "App"),
        ("Postgres", "Database"),
        ("webhooks", "Feature"),
        ("idempotency", "Pattern"),
    ] {
        assert_eq!(node(&g, name).kind, kind, "P1: {name} is drawn as a {kind}");
        assert!(!node(&g, name).contested, "P1: nothing in the project is contested");
    }
}

fn p16() {
    let entries = billing_entries();
    let base = machine(&entries, &[0, 1, 2, 3, 4, 5]);
    let orders: [&[usize]; 6] = [
        &[5, 4, 3, 2, 1, 0],
        &[2, 0, 4, 1, 5, 3],
        &[3, 5, 1, 2, 0, 4],
        &[2, 4, 5, 0, 3, 1],
        &[0, 1, 2, 2, 3, 4, 5],
        &[2, 4, 0, 1, 3, 5],
    ];
    for order in orders {
        assert_eq!(
            machine(&entries, order),
            base,
            "P16: the order {order:?} builds the same graph"
        );
    }
    // a cut link: two entries now, the rest after it heals, on one machine
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "hub", MAR_3);
    for i in [1, 4] {
        store.add_observation(entries[i].clone()).unwrap();
    }
    e.reproject(Some(WS)).unwrap();
    for i in [5, 0, 3, 2] {
        store.add_observation(entries[i].clone()).unwrap();
    }
    e.reproject(Some(WS)).unwrap();
    assert_eq!(graph_json(&e), base, "P16: a machine cut off and healed builds the same graph");
    assert_eq!(values(16)["digest"], digest(&base), "P16: the page's digest is the engine's");
    assert_eq!(values(16)["digest"], values(1)["digest"], "P1 and P16 draw one project");
}

fn p2() {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    let mut i = input(
        "billing-api uses Postgres",
        vec![
            entity("billing-api", Some("App"), None),
            entity("Postgres", Some("Database"), None),
        ],
        vec![relation("billing-api", "uses", "Postgres")],
    );
    i.on_behalf_of = Some("you".into());
    i.source_ref = Some("git:3fa91c2".into());
    let id = observe(&e, i);
    let o = e.get_observation(&id).unwrap().expect("the entry");
    let a = &o.provenance[0];
    assert_eq!(
        (a.host.as_str(), a.on_behalf_of.as_deref(), a.workspace.as_str()),
        ("laptop", Some("you"), WS),
        "P2: the machine, the person and the workspace"
    );
    assert_eq!(a.source_ref.as_deref(), Some("git:3fa91c2"), "P2: the source");
    assert_eq!(a.observed_at, MAR_3, "P2: the time");
    assert_eq!(a.confidence, None, "P2: a confidence nobody stated stays unstated");
    assert_eq!(values(2)["confidence"], "unstated", "P2: and the page says so");
}

fn p3() {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    observe(
        &e,
        input(
            "billing-api runs on Postgres 14",
            vec![entity("billing-api", Some("App"), Some("Postgres 14"))],
            vec![],
        ),
    );
    let second = input(
        "billing-api now runs on Postgres 16",
        vec![entity("billing-api", Some("App"), Some("Postgres 16"))],
        vec![],
    );
    let id = observe(&e, second);
    let shown = values(3)["shown"].as_str().unwrap().to_string();
    let desc = |e: &Engine| node(&e.graph(Some(WS)).unwrap(), "billing-api").description.clone();
    assert_eq!(desc(&e).as_deref(), Some(shown.as_str()), "P3: the correction is what is shown");
    assert_eq!(log_len(&store), 2, "P3: the older entry stays in the log");
    let again = e.get_observation(&id).unwrap().unwrap();
    store.add_observation(again).expect("the same entry arrives again");
    e.reproject(Some(WS)).unwrap();
    assert_eq!(
        log_len(&store) as u64,
        values(3)["entries"].as_u64().unwrap(),
        "P3: the same entry twice is one entry"
    );
    observe(
        &e,
        input(
            "billing-api, described with nothing",
            vec![entity("billing-api", Some("App"), Some(""))],
            vec![],
        ),
    );
    assert_eq!(
        desc(&e).as_deref(),
        Some(shown.as_str()),
        "P8: a blank description erases nothing"
    );
}

fn p4() {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    let mut r = relation("billing-api", "uses", "Postgres");
    r.valid_from = Some(JAN_10);
    let id = observe(
        &e,
        input(
            "billing-api has used Postgres since January 10",
            vec![
                entity("billing-api", Some("App"), None),
                entity("Postgres", Some("Database"), None),
            ],
            vec![r],
        ),
    );
    let o = e.get_observation(&id).unwrap().unwrap();
    assert_eq!(o.provenance[0].observed_at, MAR_3, "P4: when the system learned it");
    let view = e.get_entity(&Entity::make_id(WS, "billing-api")).unwrap().expect("billing-api");
    let rel = view.relations.iter().find(|x| x.kind == "uses").expect("the relation");
    assert_eq!(rel.valid_from, Some(JAN_10), "P4: when it became true, kept apart");
}

fn p6() {
    let store = Arc::new(InMemoryStore::new());
    let laptop = engine_on(&store, "laptop", MAR_3);
    let ci = engine_on(&store, "ci-runner", MAR_3 + 86_400_000);
    let v = values(6);
    let kind = |e: &Engine| {
        let g = e.graph(Some(WS)).unwrap();
        let n = node(&g, "Redis");
        (n.kind.clone(), n.contested, n.trust_tier)
    };
    let mut i = input("Redis is a Database", vec![entity("Redis", Some("Database"), None)], vec![]);
    i.on_behalf_of = Some("you".into());
    let first = observe(&laptop, i);
    assert_eq!(
        kind(&laptop),
        (v["first"].as_str().unwrap().to_string(), false, TrustTier::AgentExtracted),
        "P6: one claim is the belief"
    );
    observe(
        &ci,
        input("Redis is a Cache", vec![entity("Redis", Some("Cache"), None)], vec![]),
    );
    let (k, contested, _) = kind(&laptop);
    assert_eq!(
        (k.as_str(), contested),
        (v["during"].as_str().unwrap(), true),
        "P6: trust cannot decide, so the latest is shown and contested"
    );
    let pid = laptop
        .propose(ProposeInput {
            workspace: Some(WS.into()),
            kind: "claim_promotion".into(),
            targets: vec![first],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: Some("you confirm Database".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
    laptop
        .review_proposal(Some(WS.into()), pid, "merge".into(), None, None, VerdictSurface::Console)
        .expect("the console verdict");
    let (k, contested, tier) = kind(&laptop);
    assert_eq!(
        (k.as_str(), contested),
        (v["after"].as_str().unwrap(), false),
        "P6: trust settles it"
    );
    assert_eq!(
        tier_words(tier),
        v["tier"].as_str().unwrap(),
        "P6: the confirmed side carries the person's tier"
    );
    let g = laptop.graph(Some(WS)).unwrap();
    assert!(
        node(&g, "Redis").competitors.iter().any(|c| c.value == "Cache"),
        "P6: the defeated claim stays"
    );
}

fn p8() {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    let def = |d: &str| DefineTypeInput {
        workspace: Some(WS.into()),
        source_ref: None,
        on_behalf_of: None,
        defs: vec![TypeDefInput {
            target: TypeTarget::Entity,
            name: "Cache".into(),
            description: d.into(),
        }],
    };
    assert!(e.define_type(def("")).is_err(), "P8: a type with no definition is refused");
    e.define_type(def("a store of copies")).expect("P8: a defined type enters");
    assert!(
        e.types(Some(WS)).unwrap().iter().any(|t| t.name == "Cache"),
        "P8: the vocabulary has it"
    );
    observe(
        &e,
        input(
            "Redis caches sessions",
            vec![entity("Redis", Some("Cache"), Some("a cache for sessions"))],
            vec![],
        ),
    );
    observe(
        &e,
        input(
            "Redis, again, with no description",
            vec![entity("Redis", Some("Cache"), Some(""))],
            vec![],
        ),
    );
    assert_eq!(
        node(&e.graph(Some(WS)).unwrap(), "Redis").description.as_deref(),
        Some("a cache for sessions"),
        "P8: kept"
    );
}

fn p9() {
    let store = Arc::new(InMemoryStore::new());
    let laptop = engine_on(&store, "laptop", MAR_3);
    let desk = engine_on(&store, "desk-2", MAR_3 + 1000);
    let def = |target, name: &str, d: &str| DefineTypeInput {
        workspace: Some(WS.into()),
        source_ref: None,
        on_behalf_of: None,
        defs: vec![TypeDefInput { target, name: name.into(), description: d.into() }],
    };
    laptop
        .define_type(def(TypeTarget::Entity, "Service", "a running process"))
        .unwrap();
    desk.define_type(def(TypeTarget::Entity, "Service", "a support plan")).unwrap();
    let types = laptop.types(Some(WS)).unwrap();
    assert!(
        types.iter().any(|t| t.name == "Service" && t.contested),
        "P9: two definitions are contested"
    );
    laptop
        .define_type(def(TypeTarget::Relation, "member_of", "belongs to a group"))
        .unwrap();
    laptop
        .define_type(def(TypeTarget::Entity, "member_of", "a membership"))
        .unwrap();
    assert!(
        laptop
            .curation(Some(WS))
            .unwrap()
            .type_axis_collisions
            .contains(&"member_of".to_string()),
        "P9: a name on both axes is surfaced"
    );
}

fn p14() {
    let (a, b) = (Arc::new(InMemoryStore::new()), Arc::new(InMemoryStore::new()));
    let laptop = engine_on(&a, "laptop", MAR_3);
    let desktop = engine_on(&b, "desktop", MAR_3 + 5000);
    let note = || input("retries reuse the key", vec![], vec![]);
    let (ia, ib) = (observe(&laptop, note()), observe(&desktop, note()));
    assert_eq!(ia, ib, "P14: the same content is the same id on two machines");
    assert_eq!(values(14)["id"], &ia[..6], "P14: the page shows the engine's id");
    b.add_observation(laptop.get_observation(&ia).unwrap().unwrap()).expect("sync");
    assert_eq!(log_len(&b), 1, "P14: one entry, counted once");
    observe(
        &laptop,
        input(
            "pg and Postgres",
            vec![entity("pg", Some("Database"), None), entity("Postgres", Some("Database"), None)],
            vec![],
        ),
    );
    let (pg, postgres) = (Entity::make_id(WS, "pg"), Entity::make_id(WS, "Postgres"));
    let pid = laptop
        .propose(ProposeInput {
            workspace: Some(WS.into()),
            kind: "entity_merge".into(),
            targets: vec![pg.clone(), postgres.clone()],
            into: Some(postgres.clone()),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
    laptop
        .review_proposal(Some(WS.into()), pid, "merge".into(), None, None, VerdictSurface::Console)
        .expect("merge");
    let got = laptop.get_entity(&pg).unwrap().expect("P14: the merged name still resolves");
    assert_eq!(got.entity.id, postgres, "P14: forwarded to Postgres");
}

fn p17() {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    let key = "AKIAIOSFODNN7EXAMPLE";
    let Err(err) = e.observe(input(&format!("the deploy token={key}"), vec![], vec![])) else {
        panic!("P17: a credential is refused");
    };
    assert!(!err.to_string().contains(key), "P17: the refusal does not repeat it");
    assert_eq!(log_len(&store), 0, "P17: nothing was stored");
}

/// The project P23 draws: six entries, two names for one database.
fn p23_project() -> (Arc<InMemoryStore>, Engine, String, String) {
    let store = Arc::new(InMemoryStore::new());
    let e = engine_on(&store, "laptop", MAR_3);
    observe(
        &e,
        input("billing-api is an App", vec![entity("billing-api", Some("App"), None)], vec![]),
    );
    observe(
        &e,
        input("pg is a Database", vec![entity("pg", Some("Database"), None)], vec![]),
    );
    observe(
        &e,
        input(
            "billing-api uses pg",
            vec![entity("billing-api", None, None), entity("pg", None, None)],
            vec![relation("billing-api", "uses", "pg")],
        ),
    );
    observe(
        &e,
        input(
            "Postgres is a Database",
            vec![entity("Postgres", Some("Database"), None)],
            vec![],
        ),
    );
    observe(&e, input("orders is an App", vec![entity("orders", Some("App"), None)], vec![]));
    observe(
        &e,
        input(
            "orders uses Postgres",
            vec![entity("orders", None, None), entity("Postgres", None, None)],
            vec![relation("orders", "uses", "Postgres")],
        ),
    );
    let before = graph_json(&e);
    let pid = e
        .propose(ProposeInput {
            workspace: Some(WS.into()),
            kind: "entity_merge".into(),
            targets: vec![Entity::make_id(WS, "pg"), Entity::make_id(WS, "Postgres")],
            into: Some(Entity::make_id(WS, "Postgres")),
            tier: None,
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("propose");
    (store, e, pid, before)
}

fn p23() {
    let v = values(23);
    let (log, canon) = (&v["log"], &v["canon"]);
    let n = |k: &str, of: &Value| of[k].as_u64().unwrap() as usize;
    let (store, e, pid, before) = p23_project();
    assert_eq!(
        log_len(&store),
        n("base", log) + n("propose", log),
        "P23: the proposal is an entry in the log"
    );
    assert_eq!(
        graph_json(&e),
        before,
        "P23: proposing changed nothing (canon changes: {})",
        n("base", canon)
    );
    observe(
        &e,
        input(
            "orders retries a failed charge twice",
            vec![entity("orders", None, None)],
            vec![],
        ),
    );
    assert_eq!(
        log_len(&store),
        n("base", log) + n("propose", log) + n("note", log),
        "P23: a write goes through while the proposal waits"
    );
    e.review_proposal(Some(WS.into()), pid, "merge".into(), None, None, VerdictSurface::Console)
        .expect("merge");
    assert_eq!(
        log_len(&store),
        n("base", log) + n("propose", log) + n("note", log) + n("verdict", log),
        "P23: the verdict is an entry too"
    );
    let g = e.graph(Some(WS)).unwrap();
    assert!(
        !g.nodes.iter().any(|x| x.name == "pg") && n("merge", canon) == 1,
        "P23: the merge is the one change to the canon"
    );
    assert_eq!(
        e.get_entity(&Entity::make_id(WS, "pg")).unwrap().expect("resolves").entity.id,
        Entity::make_id(WS, "Postgres"),
        "P23: the old id still resolves"
    );
    // the other branch: a rejection deletes nothing
    let (store, e, pid, _) = p23_project();
    let before = log_len(&store);
    e.review_proposal(Some(WS.into()), pid, "reject".into(), None, None, VerdictSurface::Console)
        .expect("reject");
    assert_eq!(
        log_len(&store),
        before + n("verdict", log),
        "P23: a rejection is an entry, and removes none"
    );
    let g = e.graph(Some(WS)).unwrap();
    assert!(
        g.nodes.iter().any(|x| x.name == "pg")
            && g.nodes.iter().any(|x| x.name == "Postgres")
            && n("reject", canon) == 0,
        "P23: both names stay"
    );
}

#[test]
fn every_principles_page_case_marked_replay_is_the_engines_answer() {
    let replays: &[(&str, fn())] = &[
        ("p1", p1),
        ("p2", p2),
        ("p3", p3),
        ("p4", p4),
        ("p6", p6),
        ("p8", p8),
        ("p9", p9),
        ("p14", p14),
        ("p16", p16),
        ("p17", p17),
        ("p23", p23),
    ];
    let cases = cases();
    let marked: Vec<String> = cases
        .as_object()
        .expect("an object of cases")
        .iter()
        .filter(|(_, c)| c["replay"] == true)
        .map(|(k, _)| k.clone())
        .collect();
    let mut have: Vec<String> = replays.iter().map(|(k, _)| k.to_string()).collect();
    let mut want = marked.clone();
    have.sort();
    want.sort();
    assert_eq!(
        have, want,
        "every case the page marks replay has its replay here, and only those"
    );
    for (_, f) in replays {
        f();
    }
}
