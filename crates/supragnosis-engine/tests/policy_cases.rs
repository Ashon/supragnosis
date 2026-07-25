//! Policy cases - "this is what was known, this is what happened, this is what the policy required
//! of the difference."
//!
//! `principle_scenarios.rs` asserts outcomes: do X, expect Y. That answers "does the feature work",
//! which is necessary and not sufficient for a policy. A policy is a constraint on **change**:
//! nothing may be forgotten, the log may only grow, a generator may not commit, a gate may not be
//! bypassed. Those are statements about a *delta*, and a test that only inspects the final state
//! cannot distinguish "the rule held" from "the rule happened not to be exercised".
//!
//! So a case here takes a snapshot of the knowledge before the act, applies the act, snapshots
//! after, and asserts a named clause about the difference - and prints both states when it fails,
//! because a policy violation is only legible as a before/after.
//!
//! The four predicates below are the recurring shapes. They are deliberately reusable: a principle
//! that cannot be phrased as one of them is usually a scenario test, not a policy case.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use supragnosis_core::{Entity, KnowledgeStore, TrustTier};
use supragnosis_engine::{Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, VerdictSurface};
use supragnosis_store::InMemoryStore;

const WS: &str = "ws";

// --- the harness ----------------------------------------------------------------------------

/// What one entity looks like, reduced to the parts a policy talks about.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EntityFacts {
    name: String,
    kind: String,
    aliases: BTreeSet<String>,
    /// Supporting attestations - a policy about "nothing is forgotten" is about this not shrinking.
    sources: usize,
}

/// The knowledge a node holds, in a form two moments in time can be compared by. Keyed by id, not
/// by name: the representative name is itself a policy outcome and must be free to change without
/// making the snapshot think a different entity appeared.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    observations: BTreeSet<String>,
    entities: BTreeMap<String, EntityFacts>,
    relations: BTreeSet<(String, String, String)>,
}

fn snapshot(store: &dyn KnowledgeStore) -> Snapshot {
    let entities = store
        .all_entities(Some(WS))
        .expect("entities")
        .into_iter()
        .map(|e| {
            (
                e.id.clone(),
                EntityFacts {
                    name: e.canonical_name,
                    kind: e.kind,
                    aliases: e.aliases.into_iter().collect(),
                    sources: e.provenance.len(),
                },
            )
        })
        .collect();
    Snapshot {
        observations: store
            .all_observations(Some(WS))
            .expect("observations")
            .into_iter()
            .map(|o| o.id)
            .collect(),
        entities,
        relations: store
            .all_relations(Some(WS))
            .expect("relations")
            .into_iter()
            .map(|r| (r.from, r.kind, r.to))
            .collect(),
    }
}

/// One policy case. Carries the principle and the exact clause under test so a failure names the
/// norm that broke, not just the assertion that fired.
struct Case {
    principle: &'static str,
    clause: &'static str,
}

impl Case {
    fn new(principle: &'static str, clause: &'static str) -> Self {
        Self { principle, clause }
    }

    fn fail(&self, what: &str, before: &Snapshot, after: &Snapshot) -> ! {
        panic!(
            "{} violated - {}\n  required: {what}\n\n  before: {} observations, {} entities, {} relations\n  after : {} observations, {} entities, {} relations\n\n  before entities: {:#?}\n  after entities : {:#?}",
            self.principle,
            self.clause,
            before.observations.len(),
            before.entities.len(),
            before.relations.len(),
            after.observations.len(),
            after.entities.len(),
            after.relations.len(),
            before.entities.values().collect::<Vec<_>>(),
            after.entities.values().collect::<Vec<_>>(),
        )
    }

    /// The act touched nothing at all. The shape of every generate-not-commit rule (I18): a
    /// candidate producer that mutates anything has already stopped being a candidate producer.
    fn changed_nothing(&self, before: &Snapshot, after: &Snapshot) {
        if before != after {
            self.fail("the act must leave the knowledge untouched", before, after);
        }
    }

    /// The log is identical; only the derived projection may differ. The separation of assertion
    /// from belief (P1) is exactly this asymmetry.
    fn log_unchanged(&self, before: &Snapshot, after: &Snapshot) {
        if before.observations != after.observations {
            self.fail(
                "the observation log must not change - only the projection derived from it may",
                before,
                after,
            );
        }
    }

    /// The log grew by exactly `n` and kept everything it had. Append-only in the form that matters:
    /// not "it got bigger" but "nothing that was there stopped being there" (P3).
    fn log_appended(&self, before: &Snapshot, after: &Snapshot, n: usize) {
        if !before.observations.is_subset(&after.observations) {
            self.fail("no observation may leave the log", before, after);
        }
        if after.observations.len() != before.observations.len() + n {
            self.fail(
                &format!("the log must grow by exactly {n} observation(s)"),
                before,
                after,
            );
        }
    }

    /// No entity vanished and no spelling was dropped. "Supersede, don't delete" (P3) is a claim
    /// about the delta, so this is where it can actually be checked: a later assertion may change
    /// which spelling represents an entity, never which spellings are still reachable.
    fn forgot_nothing(&self, before: &Snapshot, after: &Snapshot) {
        for (id, was) in &before.entities {
            let Some(now) = after.entities.get(id) else {
                self.fail(
                    &format!("entity {id} ({}) disappeared", was.name),
                    before,
                    after,
                );
            };
            let known_before: BTreeSet<&String> =
                was.aliases.iter().chain(std::iter::once(&was.name)).collect();
            let known_now: BTreeSet<&String> =
                now.aliases.iter().chain(std::iter::once(&now.name)).collect();
            if !known_before.is_subset(&known_now) {
                self.fail(
                    &format!("spellings of {id} were dropped: {known_before:?} -> {known_now:?}"),
                    before,
                    after,
                );
            }
            if now.sources < was.sources {
                self.fail(
                    &format!("attestations of {id} shrank: {} -> {}", was.sources, now.sources),
                    before,
                    after,
                );
            }
        }
    }
}

fn engine() -> (Arc<InMemoryStore>, Engine) {
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS);
    (store, engine)
}

fn observe(engine: &Engine, content: &str, names: &[&str]) {
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
                .map(|n| EntityInput {
                    name: (*n).into(),
                    kind: None,
                    description: None,
                })
                .collect(),
            relations: vec![],
        })
        .expect("observe");
}

// --- the cases ------------------------------------------------------------------------------

/// GIVEN knowledge already projected, WHEN the projection is rebuilt from the log, THEN the log is
/// untouched and the projection is identical. This is what makes the log the source of truth rather
/// than a second copy: re-deriving must be a no-op you can run at any time (P1/P16).
#[test]
fn p1_reprojection_rederives_without_touching_the_log() {
    let (store, engine) = engine();
    observe(&engine, "a fact", &["Alpha", "Beta"]);
    observe(&engine, "another", &["Beta", "Gamma"]);
    let before = snapshot(store.as_ref());

    engine.reproject(Some(WS)).expect("reproject");
    let after = snapshot(store.as_ref());

    let case = Case::new("Principle 1", "the graph is a projection of the log, never a second source");
    case.log_unchanged(&before, &after);
    case.forgot_nothing(&before, &after);
    assert_eq!(before, after, "a replay of the same log must reproduce the same projection exactly");
}

/// GIVEN entities that a generator will flag, WHEN the curation pass runs, THEN nothing whatsoever
/// changed. Consolidation generates candidates and commits none of them (I18/P7) - and the only way
/// to test "commits none" is to compare the whole knowledge state across the act.
#[test]
fn p7_curation_generates_candidates_and_commits_nothing() {
    let (store, engine) = engine();
    observe(&engine, "the enum", &["TrustTier"]);
    observe(&engine, "the concept", &["Trust Tier"]);
    observe(&engine, "an orphan", &["Unlinked"]);
    let before = snapshot(store.as_ref());

    let report = engine.curation(Some(WS)).expect("curation");
    let after = snapshot(store.as_ref());

    assert!(
        !report.name_variants.is_empty() || !report.orphans.is_empty(),
        "fixture must actually produce candidates, or this proves nothing"
    );
    Case::new("Principle 7", "consolidation generates candidates but never commits them")
        .changed_nothing(&before, &after);
}

/// GIVEN an entity known by one spelling, WHEN a second spelling is asserted, THEN both remain
/// reachable and the log grew by exactly the one new observation. The representative may change;
/// what may never happen is a spelling ceasing to exist (P3/IR1).
#[test]
fn p3_a_new_spelling_accumulates_and_never_displaces() {
    let (store, engine) = engine();
    observe(&engine, "first mention", &["Driver"]);
    let before = snapshot(store.as_ref());
    let id = Entity::make_id(WS, "driver");
    assert_eq!(before.entities[&id].name, "Driver");

    observe(&engine, "second mention", &["DRIVER"]);
    let after = snapshot(store.as_ref());

    let case = Case::new("Principle 3", "knowledge is superseded, never erased");
    case.log_appended(&before, &after, 1);
    case.forgot_nothing(&before, &after);
    let now = &after.entities[&id];
    let reachable: BTreeSet<&str> = now
        .aliases
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(now.name.as_str()))
        .collect();
    assert_eq!(
        reachable,
        BTreeSet::from(["Driver", "DRIVER"]),
        "both spellings must stay reachable, whichever one represents the entity"
    );
}

/// GIVEN two entities, WHEN a merge is PROPOSED, THEN nothing changed; only WHEN the verdict merges
/// does the fold take effect. The gate is not the proposal, it is the verdict (P23) - so the policy
/// is a two-step delta, and the interesting assertion is the one about the first step.
#[test]
fn p23_a_proposal_alone_changes_nothing_only_the_verdict_commits() {
    let (store, engine) = engine();
    observe(&engine, "one", &["Postgres"]);
    observe(&engine, "two", &["PostgreSQL"]);
    let (a, b) = (Entity::make_id(WS, "postgres"), Entity::make_id(WS, "postgresql"));
    let before = snapshot(store.as_ref());

    // Step 1: opening a proposal is an observation about the canon, not a change to it.
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![a.clone(), b.clone()],
            into: Some(b.clone()),
            tier: None,
            rationale: Some("policy case".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");
    let proposed = snapshot(store.as_ref());

    let case = Case::new("Principle 23", "canon changes only through a merged verdict");
    case.log_appended(&before, &proposed, 1); // the proposal event itself
    assert_eq!(
        before.entities, proposed.entities,
        "an open proposal must not move the projection - the gate has not decided yet"
    );

    // Step 2: the verdict is what commits.
    engine
        .review_proposal(
            None,
            proposal,
            "merge".into(),
            None,
            Some("ashon".into()),
            VerdictSurface::Console,
        )
        .expect("review");
    let merged = snapshot(store.as_ref());

    case.forgot_nothing(&before, &merged);
    assert!(
        engine.get_entity(&a).expect("get").expect("forwarded").entity.id == b,
        "after the verdict the merged-away id must forward to the canonical one"
    );
}

/// GIVEN an entity at the default tier, WHEN an AGENT-surface promotion verdict merges, THEN the
/// effective tier rises no further than HostSigned. A signature proves origin, never a human act,
/// so the surface a verdict arrives on is part of what it is allowed to grant (P18).
#[test]
fn p18_an_agent_surface_verdict_cannot_grant_human_confirmed() {
    let (store, engine) = engine();
    observe(&engine, "a claim", &["Subject"]);
    let before = snapshot(store.as_ref());
    let obs = store
        .all_observations(Some(WS))
        .expect("obs")
        .into_iter()
        .next()
        .expect("one observation");

    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![obs.id.clone()],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: Some("policy case".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("agent".into()),
        })
        .expect("propose");
    engine
        .review_proposal(
            None,
            proposal,
            "merge".into(),
            None,
            Some("agent".into()),
            VerdictSurface::Agent,
        )
        .expect("review");
    let after = snapshot(store.as_ref());

    let case = Case::new("Principle 18", "a claimed tier is the receiver's to evaluate, not the writer's to declare");
    case.forgot_nothing(&before, &after);
    let view = engine.get_entity(&Entity::make_id(WS, "subject")).expect("get").expect("entity");
    assert!(
        view.effective_tier <= TrustTier::HostSigned,
        "an agent-surface verdict granted {:?} - the ceiling is HostSigned",
        view.effective_tier
    );
}

/// GIVEN the same orthographic variant present in two different workspaces, WHEN the all-workspaces
/// curation view is built, THEN no candidate group spans them. `*` is a view, not a place: a merge
/// across workspaces has nowhere coherent to be filed, since the proposal itself must belong to one
/// (P17 - a workspace is the sovereignty boundary, and a candidate that crosses it is not actionable).
#[test]
fn p17_candidates_never_span_workspaces_in_the_all_view() {
    let store = Arc::new(InMemoryStore::new());
    let a = Engine::new(store.clone(), "h", "alpha");
    let b = Engine::new(store.clone(), "h", "beta");
    observe(&a, "in alpha", &["TrustTier"]);
    observe(&b, "in beta", &["Trust Tier"]);

    let scoped = a.curation(Some("alpha")).expect("curation");
    assert!(
        scoped.name_variants.is_empty(),
        "each workspace holds one spelling, so neither alone has a variant pair"
    );

    let all = a.curation(None).expect("curation across workspaces");
    for g in &all.name_variants {
        assert!(
            g.members.len() < 2 || g.members.iter().all(|m| m.id == g.members[0].id),
            "a candidate group spanned workspaces: {:?}",
            g.members.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
    }
}

/// GIVEN a belief contested between two equally-trusted assertions, WHEN a claim_promotion is OPENED
/// over one of them, THEN the projection is untouched and the proposal carries a computed diff naming
/// the belief the verdict would overturn.
///
/// This is the pair that makes review possible at all (proposal-workflow.md Section 5): the canon has
/// not moved, and yet the reviewer can see what moving it would do. Before this, the diff existed only
/// as a viewer canvas overlay, so "no merge without a diff" held by UI convention - an agent opening
/// proposals through MCP got no diff at all.
#[test]
fn p23_an_open_gate_proposal_carries_a_diff_without_moving_the_canon() {
    let (store, engine) = engine();
    let kinded = |content: &str, kind: &str| {
        engine
            .observe(ObserveInput {
                content: content.into(),
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
                relations: vec![],
            })
            .expect("observe")
            .observation_id
    };
    let as_db = kinded("cozo is a database", "Database");
    kinded("cozo is a library", "Library");

    let entity = Entity::make_id(WS, "cozo");
    let contested_before = engine.get_entity(&entity).expect("get").expect("entity").contested;
    assert!(contested_before, "fixture must actually contest, or the diff proves nothing");
    let before = snapshot(store.as_ref());

    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![as_db.clone()],
            into: None,
            tier: Some("host_signed".into()),
            rationale: Some("settle the kind".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");
    let after = snapshot(store.as_ref());

    let case = Case::new("Principle 23", "a diff is available before the verdict, and the verdict is what commits");
    case.log_appended(&before, &after, 1); // the proposal event, nothing else
    assert_eq!(
        before.entities, after.entities,
        "opening a gate proposal must not move the projection"
    );

    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    let diff = view.belief_diff.expect("get_proposal must attach a diff");
    assert!(diff.computable, "claim_promotion has a commit effect, so its diff is computable");
    assert!(
        diff.tier_changes.iter().any(|t| t.observation == as_db),
        "the promoted observation's effective tier must appear as changing: {:?}",
        diff.tier_changes
    );
    let overturn = diff
        .overturned
        .iter()
        .find(|b| b.entity == entity)
        .expect("the contested belief must appear as overturned");
    assert!(
        overturn.contested_before && !overturn.contested_after,
        "the diff must show the contradiction being settled: {overturn:?}"
    );
    assert_eq!(overturn.to.as_deref(), Some("Database"), "promoting the Database claim must win it");
}

/// A kind with no commit effect must say so rather than return an empty diff. An empty list would
/// read as "this proposal changes nothing", when the truth is "this cannot be computed yet" - the
/// same absence-vs-unavailable distinction the merge band's coverage report exists to preserve (P5).
#[test]
fn p5_a_diff_for_an_unenforced_kind_reports_uncomputable_not_empty() {
    let (_store, engine) = engine();
    observe(&engine, "a subject", &["Thing"]);
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "tbox_change".into(),
            targets: vec!["Thing".into()],
            into: None,
            tier: None,
            rationale: Some("rename a type".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");

    let diff = engine
        .get_proposal(None, &proposal)
        .expect("get")
        .expect("proposal")
        .belief_diff
        .expect("a diff must be attached even when it cannot be computed");
    assert!(!diff.computable, "tbox_change enforces nothing yet");
    assert!(
        diff.note.is_some_and(|n| n.contains("no commit effect")),
        "the reason must be stated, not left to the reader to infer from emptiness"
    );
    assert!(diff.overturned.is_empty() && diff.tier_changes.is_empty());
}

/// GIVEN two entities that are duplicates AND connected to each other, WHEN an entity_merge is
/// OPENED, THEN the canon has not moved and the proposal names every reference that would rewire -
/// including the edge between them, which becomes a self-loop and therefore disappears from the graph.
///
/// That last part is the reason the diff has to be computed rather than drawn: the viewer's overlay
/// accents edges incident to a target, but "this edge stops existing" is not something you can read
/// off target ids (proposal-workflow.md Section 5, item 5).
#[test]
fn p23_a_merge_proposal_names_the_references_it_would_rewire() {
    let (store, engine) = engine();
    engine
        .observe(ObserveInput {
            content: "postgres notes".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![
                // The duplicate pair, connected to each other AND each to a third party.
                RelationInput { from: "Postgres".into(), kind: "relates_to".into(), to: "PostgreSQL".into(), description: None, valid_from: None, valid_to: None },
                RelationInput { from: "Postgres".into(), kind: "used_by".into(), to: "App".into(), description: None, valid_from: None, valid_to: None },
                RelationInput { from: "PostgreSQL".into(), kind: "runs_on".into(), to: "Linux".into(), description: None, valid_from: None, valid_to: None },
            ],
        })
        .expect("observe");

    let (a, b) = (Entity::make_id(WS, "postgres"), Entity::make_id(WS, "postgresql"));
    let before = snapshot(store.as_ref());

    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![a.clone(), b.clone()],
            into: Some(b.clone()),
            tier: None,
            rationale: Some("same database".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");
    let after = snapshot(store.as_ref());

    let case = Case::new("Principle 23", "a merge is reviewable before it commits, and only the verdict commits");
    case.log_appended(&before, &after, 1);
    assert_eq!(before.entities, after.entities, "opening the proposal must not fold anything");

    let diff = engine
        .get_proposal(None, &proposal)
        .expect("get")
        .expect("proposal")
        .belief_diff
        .expect("entity_merge must carry a computed diff");
    assert!(diff.computable, "entity_merge has a commit effect (id forwarding)");

    // Every edge touching the folded-away side is named, and none touching only the survivor.
    let kinds: Vec<&str> = diff.rewired.iter().map(|r| r.kind.as_str()).collect();
    assert!(kinds.contains(&"used_by"), "the folded entity's own edge must rewire: {kinds:?}");
    assert!(kinds.contains(&"relates_to"), "the edge between the pair must be named: {kinds:?}");
    assert!(
        !kinds.contains(&"runs_on"),
        "an edge that only touches the survivor does not move: {kinds:?}"
    );

    let between = diff.rewired.iter().find(|r| r.kind == "relates_to").expect("the pair's own edge");
    assert!(
        between.becomes_self_loop,
        "merging two connected entities makes their edge a self-loop, which graph() drops - the \
         reviewer must see the edge disappearing: {between:?}"
    );
    let moved = diff.rewired.iter().find(|r| r.kind == "used_by").expect("used_by");
    assert!(!moved.becomes_self_loop);
    assert_eq!(moved.other_name, "App", "the endpoint that stays is what the edge still connects to");
    assert_eq!(moved.to_name, "PostgreSQL", "everything rewires onto the canonical id");
}
