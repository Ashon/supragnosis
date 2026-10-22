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

use supragnosis_core::TypeTarget;
use supragnosis_core::{
    observation_content_id, AssertionStore, Assertions, Entity, KnowledgeStore, Observation,
    ProposalEventAssertion, ProposalEventKind, Provenance, TrustTier, VERDICT_SURFACE_CONSOLE,
};
use supragnosis_engine::{
    DefineTypeInput, Engine, EntityInput, ObserveInput, ProposeInput, RelationInput, TypeDefInput,
    VerdictSurface,
};
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
            self.fail(&format!("the log must grow by exactly {n} observation(s)"), before, after);
        }
    }

    /// No entity vanished and no spelling was dropped. "Supersede, don't delete" (P3) is a claim
    /// about the delta, so this is where it can actually be checked: a later assertion may change
    /// which spelling represents an entity, never which spellings are still reachable.
    fn forgot_nothing(&self, before: &Snapshot, after: &Snapshot) {
        for (id, was) in &before.entities {
            let Some(now) = after.entities.get(id) else {
                self.fail(&format!("entity {id} ({}) disappeared", was.name), before, after);
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
                .map(|n| EntityInput { name: (*n).into(), kind: None, description: None })
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

    let case =
        Case::new("Principle 1", "the graph is a projection of the log, never a second source");
    case.log_unchanged(&before, &after);
    case.forgot_nothing(&before, &after);
    assert_eq!(
        before, after,
        "a replay of the same log must reproduce the same projection exactly"
    );
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
    // The retrieval weight is computed on this same pass (consolidation.md Section 8 step 1). It is the
    // newest thing in the report and the one most likely to reach for a store write, so the
    // commits-nothing assertion below has to be exercising it.
    assert!(
        !report.demotion_candidates.is_empty(),
        "the weight must be part of what this case proves changes nothing"
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

    let case = Case::new(
        "Principle 18",
        "a claimed tier is the receiver's to evaluate, not the writer's to declare",
    );
    case.forgot_nothing(&before, &after);
    let view = engine
        .get_entity(&Entity::make_id(WS, "subject"))
        .expect("get")
        .expect("entity");
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

    let case = Case::new(
        "Principle 23",
        "a diff is available before the verdict, and the verdict is what commits",
    );
    case.log_appended(&before, &after, 1); // the proposal event, nothing else
    assert_eq!(
        before.entities, after.entities,
        "opening a gate proposal must not move the projection"
    );

    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    let diff = view.belief_diff.expect("get_proposal must attach a diff");
    assert!(
        diff.computable,
        "claim_promotion has a commit effect, so its diff is computable"
    );
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
    assert_eq!(
        overturn.to.as_deref(),
        Some("Database"),
        "promoting the Database claim must win it"
    );
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
                RelationInput {
                    from: "Postgres".into(),
                    kind: "relates_to".into(),
                    to: "PostgreSQL".into(),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                },
                RelationInput {
                    from: "Postgres".into(),
                    kind: "used_by".into(),
                    to: "App".into(),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                },
                RelationInput {
                    from: "PostgreSQL".into(),
                    kind: "runs_on".into(),
                    to: "Linux".into(),
                    description: None,
                    valid_from: None,
                    valid_to: None,
                },
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

    let case = Case::new(
        "Principle 23",
        "a merge is reviewable before it commits, and only the verdict commits",
    );
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
    assert!(
        kinds.contains(&"used_by"),
        "the folded entity's own edge must rewire: {kinds:?}"
    );
    assert!(
        kinds.contains(&"relates_to"),
        "the edge between the pair must be named: {kinds:?}"
    );
    assert!(
        !kinds.contains(&"runs_on"),
        "an edge that only touches the survivor does not move: {kinds:?}"
    );

    let between = diff
        .rewired
        .iter()
        .find(|r| r.kind == "relates_to")
        .expect("the pair's own edge");
    assert!(
        between.becomes_self_loop,
        "merging two connected entities makes their edge a self-loop, which graph() drops - the \
         reviewer must see the edge disappearing: {between:?}"
    );
    let moved = diff.rewired.iter().find(|r| r.kind == "used_by").expect("used_by");
    assert!(!moved.becomes_self_loop);
    assert_eq!(
        moved.other_name, "App",
        "the endpoint that stays is what the edge still connects to"
    );
    assert_eq!(moved.to_name, "PostgreSQL", "everything rewires onto the canonical id");
}

/// GIVEN a merge proposal whose targets are not in the local log, WHEN a merge verdict is recorded
/// anyway, THEN the canon does not move and the proposal reads `blocked` rather than `merged`.
///
/// The verdict is recorded on purpose - the log keeps it (P3) - and the enforcement is that the FOLD
/// refuses to let it commit (I13). That distinction is the whole point: under federation a verdict
/// arrives as a replicated observation and never passes through `review_proposal`, so a gate that
/// lived only at the entry point would be no gate at all. Here the verdict is injected directly to
/// simulate exactly that arrival.
#[test]
fn p23_a_blocked_merge_verdict_does_not_reach_canon() {
    let (store, engine) = engine();
    observe(&engine, "a real one", &["Real"]);
    // A relation gives the merge effect something visible to (wrongly) rewire - the probe below.
    engine
        .observe(ObserveInput {
            content: "real depends on other".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![RelationInput {
                from: "Real".into(),
                kind: "depends_on".into(),
                to: "Other".into(),
                description: None,
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe relation");
    let ghost = Entity::make_id(WS, "never observed");
    let real = Entity::make_id(WS, "Real");

    // Fold Real INTO the absent entity, so a wrongly applied effect would be visible: Real's row
    // would drop from the projection views and its references would rewire onto nothing.
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "entity_merge".into(),
            targets: vec![real.clone(), ghost.clone()],
            into: Some(ghost.clone()),
            tier: None,
            rationale: Some("merge into something that is not here".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");

    // The local path refuses and says why, rather than letting the caller find out later.
    let refused = engine
        .review_proposal(
            None,
            proposal.clone(),
            "merge".into(),
            None,
            Some("ashon".into()),
            VerdictSurface::Console,
        )
        .expect_err("a merge that cannot commit must not be accepted silently");
    assert!(
        refused.to_string().contains("referential integrity"),
        "the refusal must name the failing check: {refused}"
    );

    let before = snapshot(store.as_ref());
    // Now bypass that path entirely. A replicated verdict never passes through review_proposal: it
    // arrives as an observation CARRYING the verdict event in its assertions (I1 - the assertions
    // are what sync replicates). Plant exactly that shape store-side and let the fold meet it.
    store
        .add_observation(Observation::with_assertions(
            format!("proposal(merge) {proposal}"),
            Provenance {
                host: "elsewhere".into(),
                on_behalf_of: Some("elsewhere".into()),
                workspace: WS.into(),
                source_ref: None,
                observed_at: 9_000,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions {
                proposal_events: vec![ProposalEventAssertion {
                    proposal: proposal.clone(),
                    event: ProposalEventKind::Verdict,
                    payload: r#"{"decision":"merge"}"#.into(),
                }],
                ..Default::default()
            },
        ))
        .expect("plant replicated verdict");
    let after = snapshot(store.as_ref());

    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "blocked", "the fold must call this merge blocked, not merged");
    let case =
        Case::new("Principle 23", "a merge reaches canon only when the blocking checks pass");
    case.log_appended(&before, &after, 1);
    case.forgot_nothing(&before, &after);
    // The commit effect must not have applied either: Real still answers with its own references
    // rather than having been folded away into the absent entity.
    let real_view = engine.get_entity(&real).expect("get").expect("Real");
    assert_eq!(
        real_view.relations.len(),
        1,
        "a blocked merge must not rewire Real's references: {:?}",
        real_view.relations
    );
    assert!(
        view.checks
            .iter()
            .any(|c| c.blocking && !c.passed && c.name == "referential integrity"),
        "the failing check must be visible on the proposal: {:?}",
        view.checks
    );
}

/// GIVEN a gate proposal whose blocking checks fail (one target observation has not arrived), WHEN
/// its replicated merge verdict lands, THEN the grant fold agrees with the state fold: `blocked`
/// grants nothing, not even to the targets that ARE present. Without this the two folds diverge -
/// the proposal surface reads blocked while the present target's belief is already promoted - and
/// "blocked" stops meaning "did not commit" (I13). The events are planted store-side because that
/// is the shape sync apply gives them; the local propose() refuses a missing target outright.
#[test]
fn p23_a_blocked_gate_merge_grants_nothing() {
    let (store, engine) = engine();
    let present = engine
        .observe(ObserveInput {
            content: "thing exists".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Thing".into(),
                kind: Some("Tool".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe");
    let entity_id = present.entities[0].clone();
    let tier = |engine: &Engine| {
        engine.get_entity(&entity_id).expect("get").expect("entity").effective_tier
    };
    assert_eq!(tier(&engine), TrustTier::AgentExtracted, "pre-gate baseline");

    // The other target does not exist yet, but its content-addressed id is knowable in advance -
    // the state a partially synced node is in while the second observation is still in flight.
    let late_id = observation_content_id(WS, "late fact", &Assertions::default());
    let plant = |content: String, observed_at, source_ref: Option<&str>, events| {
        let obs = Observation::with_assertions(
            content,
            Provenance {
                host: "host-b".into(),
                on_behalf_of: None,
                workspace: WS.into(),
                source_ref: source_ref.map(String::from),
                observed_at,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions { proposal_events: events, ..Default::default() },
        );
        let id = obs.id.clone();
        store.add_observation(obs).expect("plant");
        id
    };
    let payload = serde_json::json!({
        "kind": "claim_promotion",
        "targets": [present.observation_id, late_id],
        "into": null,
        "tier": "human_confirmed",
        "rationale": "partially synced promotion",
        "affected_types": [],
    })
    .to_string();
    let proposal = plant(
        "proposal(open) claim_promotion".into(),
        1_000,
        None,
        vec![ProposalEventAssertion {
            proposal: String::new(),
            event: ProposalEventKind::Opened,
            payload,
        }],
    );
    let before = snapshot(store.as_ref());
    plant(
        format!("proposal(merge) {proposal}"),
        2_000,
        Some(VERDICT_SURFACE_CONSOLE),
        vec![ProposalEventAssertion {
            proposal: proposal.clone(),
            event: ProposalEventKind::Verdict,
            payload: r#"{"decision":"merge"}"#.into(),
        }],
    );
    let after = snapshot(store.as_ref());

    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "blocked", "a merge with an absent target must fold to blocked");
    Case::new(
        "Principle 23",
        "a blocked merge is a merge that did not commit - it grants nothing",
    )
    .log_appended(&before, &after, 1);
    assert_eq!(
        tier(&engine),
        TrustTier::AgentExtracted,
        "the state fold says blocked, so the grant fold must not have promoted the present target"
    );

    // The pinned direction (blocked -> merged, never the reverse): when the missing observation
    // arrives, the SAME log commits and the grant applies - capped by the console ceiling.
    let late = engine
        .observe(ObserveInput {
            content: "late fact".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .expect("late observe");
    assert_eq!(
        late.observation_id, late_id,
        "the in-flight id resolves to the arrived observation"
    );
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "merged", "the arrival of the target unblocks the same verdict");
    assert_eq!(
        tier(&engine),
        TrustTier::HumanConfirmed,
        "once merged, the grant applies to the promoted targets"
    );
}

/// A well-formed proposal passes its checks, so the gate blocks nothing it should not - a gate that
/// only ever says no is indistinguishable from a broken one.
#[test]
fn p23_a_well_formed_merge_passes_its_checks_and_commits() {
    let (_store, engine) = engine();
    observe(&engine, "duplicates", &["Postgres", "PostgreSQL"]);
    let (a, b) = (Entity::make_id(WS, "postgres"), Entity::make_id(WS, "postgresql"));
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

    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert!(
        view.checks.iter().all(|c| c.passed),
        "a well-formed merge must pass every check: {:?}",
        view.checks
    );
    engine
        .review_proposal(
            None,
            proposal.clone(),
            "merge".into(),
            None,
            Some("ashon".into()),
            VerdictSurface::Console,
        )
        .expect("a passing proposal must be reviewable");
    assert_eq!(engine.get_proposal(None, &proposal).expect("get").expect("p").state, "merged");
}

/// GIVEN a recall proposal, WHEN an agent-surface merge is cast on it, THEN the surface refuses and
/// the log is unchanged - and a comment through the same surface is accepted and appends exactly
/// one observation that decides nothing. A recall verdict is a human's direct act (P23, I17): the
/// agent path may speak, never decide.
#[test]
fn i17_the_agent_surface_refuses_a_recall_merge_and_the_log_is_unchanged() {
    let (store, engine) = engine();
    observe(&engine, "a contamination source", &["Source"]);
    let source = store
        .all_observations(Some(WS))
        .expect("obs")
        .into_iter()
        .next()
        .expect("one observation")
        .id;
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "recall".into(),
            targets: vec![source],
            into: None,
            tier: None,
            rationale: Some("retract the derived tree".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("agent".into()),
        })
        .expect("propose");
    let case = Case::new("Principle 23 (I17)", "a recall verdict is a human's direct act");

    let before = snapshot(store.as_ref());
    let refused = engine
        .review_proposal(
            None,
            proposal.clone(),
            "merge".into(),
            None,
            Some("agent".into()),
            VerdictSurface::Agent,
        )
        .expect_err("the agent surface must refuse a recall merge");
    assert!(
        refused.to_string().contains("human's direct act"),
        "the refusal must say why and where to decide: {refused}"
    );
    let after = snapshot(store.as_ref());
    case.changed_nothing(&before, &after);

    // Speaking is still allowed: a comment records, and the proposal stays open.
    engine
        .review_proposal(
            None,
            proposal.clone(),
            "comment".into(),
            Some("looks contaminated to me".into()),
            Some("agent".into()),
            VerdictSurface::Agent,
        )
        .expect("a comment is accepted on the agent surface");
    let commented = snapshot(store.as_ref());
    case.log_appended(&after, &commented, 1);
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "open", "a comment decides nothing");
    assert_eq!(view.verdicts, 0, "a comment is not a verdict");
}

/// GIVEN a recall proposal and a merge verdict that reached the log WITHOUT the console marker -
/// planted store-side, the shape of a verdict replicated from a peer or one that landed through MCP
/// before the agent surface refused them - WHEN the state is folded, THEN it is not merged: the
/// fold demotes the verdict to a comment (I17). A console merge then folds to merged, so the marker
/// is the discriminator and not the kind. Read off the log, the rule gives one answer on every node.
#[test]
fn i17_a_recall_merge_without_the_console_marker_never_folds_to_merged() {
    let (store, engine) = engine();
    observe(&engine, "a contamination source", &["Source"]);
    let source = store
        .all_observations(Some(WS))
        .expect("obs")
        .into_iter()
        .next()
        .expect("one observation")
        .id;
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "recall".into(),
            targets: vec![source],
            into: None,
            tier: None,
            rationale: Some("retract the derived tree".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("agent".into()),
        })
        .expect("propose");
    // Distinct content per plant: two verdicts with one content address would absorb into one
    // row (P3), which is a different case from two writers.
    let plant_merge = |source_ref: Option<&str>, observed_at, writer: &str| {
        store
            .add_observation(Observation::with_assertions(
                format!("proposal(merge) {proposal} by {writer}"),
                Provenance {
                    host: "host-b".into(),
                    on_behalf_of: Some("agent".into()),
                    workspace: WS.into(),
                    source_ref: source_ref.map(String::from),
                    observed_at,
                    confidence: None,
                    trust_tier: TrustTier::default(),
                    sync: None,
                },
                Assertions {
                    proposal_events: vec![ProposalEventAssertion {
                        proposal: proposal.clone(),
                        event: ProposalEventKind::Verdict,
                        payload: r#"{"decision":"merge"}"#.into(),
                    }],
                    ..Default::default()
                },
            ))
            .expect("plant verdict");
    };
    let case = Case::new(
        "Principle 23 (I17)",
        "a recall merge without the console marker decides nothing",
    );

    // An agent-marked merge, and one with no marker at all (an older or foreign writer).
    let before = snapshot(store.as_ref());
    plant_merge(Some("surface:agent"), 9_000, "an agent");
    plant_merge(None, 9_001, "an unmarked writer");
    let after = snapshot(store.as_ref());
    case.log_appended(&before, &after, 2);
    case.forgot_nothing(&before, &after);
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(
        view.state, "open",
        "a recall merge without the console marker must not fold to merged"
    );
    assert_eq!(view.verdicts, 0, "a demoted verdict is a comment, not a verdict");

    // The console's own act is what the fold counts.
    engine
        .review_proposal(
            None,
            proposal.clone(),
            "merge".into(),
            None,
            None,
            VerdictSurface::Console,
        )
        .expect("the console may merge a recall");
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "merged", "the console marker is the discriminator");
    assert_eq!(view.verdicts, 1, "only the console merge counts");
    // Planting the console marker store-side is the replicated-console case, honored under the
    // single-principal premise (resolution.md Section 6) - the same marker, the same answer.
    let _ = VERDICT_SURFACE_CONSOLE;
}

/// GIVEN a recall proposal whose agent-marked merge reached the log - replicated, or from before the
/// agent surface refused one - WHEN a person casts the same decision at the console, THEN the
/// recall folds to merged. The two acts share one content address, so the console's is a second
/// attestation on the agent's row (P3); the fold reads the marker off every attestation, as the gate
/// ceiling does, so the person's act is not hidden behind the agent's authorship (I17).
#[test]
fn i17_a_console_act_on_an_agent_marked_recall_merge_counts() {
    let (store, engine) = engine();
    observe(&engine, "a contamination source", &["Source"]);
    let source = store
        .all_observations(Some(WS))
        .expect("obs")
        .into_iter()
        .next()
        .expect("one observation")
        .id;
    let proposal = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "recall".into(),
            targets: vec![source],
            into: None,
            tier: None,
            rationale: Some("retract the derived tree".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("agent".into()),
        })
        .expect("propose");
    // The exact row review_proposal writes for a merge with no note, under the agent's marker.
    store
        .add_observation(Observation::with_assertions(
            format!("proposal(merge) {proposal}"),
            Provenance {
                host: "host-b".into(),
                on_behalf_of: Some("agent".into()),
                workspace: WS.into(),
                source_ref: Some("surface:agent".into()),
                observed_at: 9_000,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions {
                proposal_events: vec![ProposalEventAssertion {
                    proposal: proposal.clone(),
                    event: ProposalEventKind::Verdict,
                    payload: serde_json::json!({ "decision": "merge", "note": null }).to_string(),
                }],
                ..Default::default()
            },
        ))
        .expect("plant the agent's verdict");
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(view.state, "open", "the agent's recall merge alone decides nothing");

    let case = Case::new(
        "Principle 23 (I17)",
        "a person's console act on an agent-marked recall merge is counted",
    );
    let before = snapshot(store.as_ref());
    engine
        .review_proposal(
            None,
            proposal.clone(),
            "merge".into(),
            None,
            None,
            VerdictSurface::Console,
        )
        .expect("the console may merge a recall");
    let after = snapshot(store.as_ref());
    case.log_appended(&before, &after, 0);
    case.forgot_nothing(&before, &after);
    let view = engine.get_proposal(None, &proposal).expect("get").expect("proposal");
    assert_eq!(
        view.state, "merged",
        "the console's attestation on the shared row is the human's direct act"
    );
}

/// A merge verdict as sync delivers it: an observation carrying the event, which never passes
/// through `review_proposal` and its refusal.
fn plant_merge_verdict(store: &InMemoryStore, proposal: &str) {
    store
        .add_observation(Observation::with_assertions(
            format!("proposal(merge) {proposal}"),
            Provenance {
                host: "elsewhere".into(),
                on_behalf_of: Some("elsewhere".into()),
                workspace: WS.into(),
                source_ref: None,
                observed_at: 9_000,
                confidence: None,
                trust_tier: TrustTier::default(),
                sync: None,
            },
            Assertions {
                proposal_events: vec![ProposalEventAssertion {
                    proposal: proposal.into(),
                    event: ProposalEventKind::Verdict,
                    payload: r#"{"decision":"merge"}"#.into(),
                }],
                ..Default::default()
            },
        ))
        .expect("plant replicated verdict");
}

fn propose(
    engine: &Engine,
    kind: &str,
    targets: Vec<String>,
    into: Option<String>,
    tier: Option<&str>,
) -> String {
    engine
        .propose(ProposeInput {
            workspace: None,
            kind: kind.into(),
            targets,
            into,
            tier: tier.map(String::from),
            rationale: Some("test".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose")
}

fn decide(engine: &Engine, proposal: &str, decision: &str) {
    engine
        .review_proposal(
            None,
            proposal.into(),
            decision.into(),
            None,
            Some("ashon".into()),
            VerdictSurface::Console,
        )
        .expect("verdict");
}

/// GIVEN a merge whose check holds its accept back, WHEN a person rejects it, THEN it concludes
/// Rejected - no valid merge and a valid reject (proposal-workflow.md Section 7.1) - and still
/// carries its merge verdict, so an un-merge of it is not refused for want of one (unmerge.md
/// Section 9). The fold kept it blocked before, and it never left the Review page's Needs you
/// (design-system.md Section 29.3).
#[test]
fn p23_a_rejected_blocked_merge_concludes_rejected_and_keeps_its_merge_verdict() {
    let (store, engine) = engine();
    observe(&engine, "a real one", &["Real"]);
    let ghost = Entity::make_id(WS, "never observed");
    let real = Entity::make_id(WS, "Real");
    let merge = propose(&engine, "entity_merge", vec![real, ghost.clone()], Some(ghost), None);
    plant_merge_verdict(&store, &merge);
    let state = |id: &str| engine.get_proposal(None, id).expect("get").expect("proposal").state;
    assert_eq!(state(&merge), "blocked");

    decide(&engine, &merge, "reject");
    assert_eq!(state(&merge), "rejected", "a reject concludes a blocked merge (Section 7.1)");
    let listed = engine.list_proposals(None).expect("list");
    assert!(listed.iter().any(|p| p.id == merge && p.state == "rejected"), "the list agrees");

    let split = propose(&engine, "entity_split", vec![merge.clone()], None, None);
    let checks = engine.get_proposal(None, &split).expect("get").expect("split").checks;
    assert!(
        checks.iter().any(|c| c.name == "reversible target" && c.passed),
        "the merge verdict still makes it reversible: {checks:?}"
    );
}

/// GIVEN a source the console confirmed, WHEN the undo read asks what reverses that confirmation,
/// THEN it is one demotion to the tier the source has without it, with the diff of that demotion;
/// and once a second confirmation holds the same tier, there is nothing to undo
/// (design-system.md Section 29.3).
#[test]
fn p23_a_confirmation_is_undone_to_the_tier_it_had_without_it() {
    let (_store, engine) = engine();
    let obs = engine
        .observe(ObserveInput {
            content: "the ledger is a table".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Ledger".into(),
                kind: Some("Table".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let first =
        propose(&engine, "claim_promotion", vec![obs.clone()], None, Some("human_confirmed"));
    decide(&engine, &first, "merge");

    let undo = engine.undo_gate(None, &first).expect("read").expect("a merged gate proposal");
    assert_eq!(undo.acts.len(), 1, "one act per target it moves");
    assert_eq!(undo.acts[0].observation, obs);
    assert_eq!(undo.acts[0].kind, "claim_demotion");
    assert_eq!(undo.acts[0].tier, TrustTier::AgentExtracted, "back to its tier without the act");
    assert!(undo.belief_diff.is_some(), "the act comes with its diff (U4)");

    // A second, distinguishable confirmation of the same source (the same text would be the same
    // proposal, P3).
    let second = engine
        .propose(ProposeInput {
            workspace: None,
            kind: "claim_promotion".into(),
            targets: vec![obs],
            into: None,
            tier: Some("human_confirmed".into()),
            rationale: Some("confirmed again".into()),
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: Some("ashon".into()),
        })
        .expect("propose");
    assert_ne!(second, first);
    decide(&engine, &second, "merge");
    let undo = engine.undo_gate(None, &first).expect("read").expect("still merged");
    assert!(
        undo.acts.is_empty(),
        "another confirmation holds the tier: {:?}",
        undo.acts.len()
    );
    assert!(engine.undo_gate(None, "no such proposal").expect("read").is_none());
}

/// GIVEN a merge and a confirmation, WHEN the proposals are listed, THEN each target is said in
/// words - an entity by its name, a source by its first words and the entities it names - and only
/// what is in the proposal's own workspace is named (design-system.md Section 29.3).
#[test]
fn proposal_rows_name_their_targets_in_their_own_workspace() {
    let (store, engine) = engine();
    observe(&engine, "postgres", &["Postgres"]);
    observe(&engine, "postgresql", &["PostgreSQL"]);
    let a = Entity::make_id(WS, "PostgreSQL");
    let b = Entity::make_id(WS, "Postgres");
    let merge = propose(&engine, "entity_merge", vec![a, b.clone()], Some(b), None);
    let obs = engine
        .observe(ObserveInput {
            content: "The nightly export sends the ledger to the finance team's warehouse every night at two.".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "Nightly export".into(), kind: None, description: None },
                EntityInput { name: "Warehouse".into(), kind: None, description: None },
            ],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let confirm = propose(&engine, "claim_promotion", vec![obs], None, Some("human_confirmed"));
    let list = engine.list_proposals(None).expect("list");
    let row = |id: &str| list.iter().find(|p| p.id == id).expect("listed");
    assert_eq!(
        row(&merge).labels,
        vec![Some("PostgreSQL".to_string()), Some("Postgres".to_string())]
    );
    assert_eq!(row(&merge).into_label.as_deref(), Some("Postgres"));
    let said = row(&confirm).labels[0].clone().expect("the source is named");
    assert!(said.starts_with("\"The nightly export sends the ledger"), "{said}");
    assert!(
        said.contains("...\" (Nightly export, Warehouse)"),
        "cut, then its entities: {said}"
    );

    // The same row read alone carries the same labels.
    let one = engine.get_proposal(None, &confirm).expect("get").expect("proposal");
    assert_eq!(one.labels, row(&confirm).labels);

    // A target in another workspace is not named in this one's rows. `propose` refuses such a
    // target, so the proposal arrives as sync would deliver it.
    let other = engine
        .observe(ObserveInput {
            content: "elsewhere".into(),
            workspace: Some("other".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput { name: "Far".into(), kind: None, description: None }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let payload = serde_json::json!({
        "kind": "claim_promotion", "targets": [other], "into": null, "tier": "human_confirmed",
        "rationale": "across", "affected_types": [],
    })
    .to_string();
    let opened = Observation::with_assertions(
        "proposal(open) claim_promotion".into(),
        Provenance {
            host: "host-b".into(),
            on_behalf_of: None,
            workspace: WS.into(),
            source_ref: None,
            observed_at: 1_000,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        },
        Assertions {
            proposal_events: vec![ProposalEventAssertion {
                proposal: String::new(),
                event: ProposalEventKind::Opened,
                payload,
            }],
            ..Default::default()
        },
    );
    let far = opened.id.clone();
    store.add_observation(opened).expect("plant");
    let list = engine.list_proposals(None).expect("list");
    let row = list.iter().find(|p| p.id == far).expect("listed");
    assert_eq!(row.labels, vec![None], "another workspace's source is not named here");
}

/// GIVEN two definitions of one type at one tier, WHEN the curation report is read, THEN the tie is
/// one of its contradictions, contested, keyed by the type's workspace, axis and name - so it waits
/// under Needs you as an entity's type does (resolution-identity.md IR5, design-system.md Section
/// 29.3) - and reading it commits nothing (I18).
#[test]
fn a_contested_type_definition_is_a_curation_contradiction() {
    let (store, engine) = engine();
    for description in ["a deployable unit", "a long-running process"] {
        engine
            .define_type(DefineTypeInput {
                workspace: None,
                source_ref: None,
                on_behalf_of: None,
                defs: vec![TypeDefInput {
                    target: TypeTarget::Entity,
                    name: "Service".into(),
                    description: description.into(),
                }],
            })
            .expect("define");
    }
    let before = snapshot(store.as_ref());
    let report = engine.curation(Some(WS)).expect("curation");
    let after = snapshot(store.as_ref());
    let def = report
        .contradictions
        .iter()
        .find(|c| c.field == "definition")
        .expect("the tied definition is listed");
    assert_eq!(def.id, format!("type:{WS}:entity:Service"));
    assert_eq!(def.name, "Service");
    assert!(def.contested, "two definitions at one tier tie");
    assert_eq!(def.target, Some(TypeTarget::Entity));
    assert_eq!(def.workspace.as_deref(), Some(WS));
    assert!(def.kind_source.is_some() && def.competitors.len() == 1);
    Case::new("Principle 7", "the curation read commits nothing").changed_nothing(&before, &after);
}

/// GIVEN a source that states a relation, WHEN the console confirms it, THEN the relation reads at
/// the source's effective tier in the graph (resolution.md Section 3, the gap step 41 found), the
/// source's own read names the act that moved it - its proposal, a promotion, cast at the console,
/// when - and which attestation wrote it, an observation is read by its id, and the proposal's
/// checks carry a trust profile that blocks nothing (design-system.md Section 29.7).
#[test]
fn a_confirmed_source_says_who_confirmed_it_and_its_relations_take_its_tier() {
    let (_store, engine) = engine();
    let obs = engine
        .observe(ObserveInput {
            content: "the ledger is stored in postgres".into(),
            workspace: None,
            source_ref: Some("git:4f2a9c1".into()),
            confidence: None,
            on_behalf_of: Some("alex".into()),
            derived_from: vec![],
            entities: vec![],
            relations: vec![RelationInput {
                from: "Ledger".into(),
                kind: "stored_in".into(),
                to: "Postgres".into(),
                description: None,
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe")
        .observation_id;
    let edge_tier = |engine: &Engine| {
        let g = engine.graph(Some(WS)).expect("graph");
        g.edges.iter().find(|e| e.kind == "stored_in").expect("the edge").trust_tier
    };
    assert_eq!(edge_tier(&engine), TrustTier::AgentExtracted);
    let read = engine.observation(&obs, None).expect("read").expect("the observation");
    assert!(read.gated_by.is_none(), "nothing moved it yet");
    assert_eq!(read.author, 0);

    let confirm =
        propose(&engine, "claim_promotion", vec![obs.clone()], None, Some("human_confirmed"));
    decide(&engine, &confirm, "merge");

    assert_eq!(
        edge_tier(&engine),
        TrustTier::HumanConfirmed,
        "the relation reads at its source's tier"
    );
    let read = engine.observation(&obs, None).expect("read").expect("the observation");
    assert_eq!(read.effective_tier, TrustTier::HumanConfirmed);
    let moved = read.gated_by.expect("the act that moved it");
    assert_eq!(moved.proposal, confirm);
    assert!(moved.promotion);
    assert_eq!(moved.surface.as_deref(), Some(VERDICT_SURFACE_CONSOLE));
    assert!(moved.origin_node.is_none(), "written here, unstamped");
    assert!(moved.at > 0);
    assert_eq!(
        engine.observation(&obs, Some(WS)).expect("read").map(|o| o.id),
        Some(obs.clone())
    );
    assert!(engine.observation("no such observation", None).expect("read").is_none());
    assert_eq!(engine.observation_workspace(&obs).expect("read").as_deref(), Some(WS));

    let checks = engine.get_proposal(None, &confirm).expect("get").expect("proposal").checks;
    let profile = checks.iter().find(|c| c.name == "trust profile").expect("a trust profile");
    assert!(!profile.blocking && profile.passed);
    assert_eq!(profile.detail, "1 human_confirmed");
}

/// design-system.md Section 29.8: what a local MCP session writes names its app on the local
/// viewer's reads only - never on the reads the read tier and MCP use - by the name a person reads,
/// a name the table does not know as sent and cut to 64 characters; an activity event names it too;
/// re-keying a workspace keeps it, and a dry run writes nothing.
#[test]
fn an_attestation_names_its_app_on_local_reads_only() {
    #[derive(Default)]
    struct Seen(std::sync::Mutex<Vec<Option<String>>>);
    impl supragnosis_core::EventSink for Seen {
        fn emit(&self, env: &supragnosis_core::EventEnvelope) {
            self.0.lock().unwrap().push(env.app.clone());
        }
    }
    let seen = Arc::new(Seen::default());
    let store = Arc::new(InMemoryStore::new());
    let engine = Engine::new(store.clone(), "host-a", WS).with_events(seen.clone());
    let write = |app: Option<&str>, content: &str| {
        engine
            .as_app(app, || {
                engine.observe(ObserveInput {
                    content: content.into(),
                    workspace: None,
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EntityInput {
                        name: "Ledger".into(),
                        kind: None,
                        description: None,
                    }],
                    relations: vec![],
                })
            })
            .expect("observe")
            .observation_id
    };
    let long = "x".repeat(100);
    let ids = [
        write(Some("claude-code"), "written by claude code"),
        write(Some("cursor-vscode"), "written by cursor"),
        write(Some("some-agent"), "written by an app the table does not know"),
        write(Some(&long), "written by a long name"),
        write(None, "written by no app"),
    ];
    let named = |id: &str| {
        let o = engine.observation_local(id, None).expect("read").expect("row");
        o.attestations[o.author].app.clone()
    };
    assert_eq!(named(&ids[0]).as_deref(), Some("Claude Code"));
    assert_eq!(named(&ids[1]).as_deref(), Some("Cursor"));
    assert_eq!(named(&ids[2]).as_deref(), Some("some-agent"));
    assert_eq!(named(&ids[3]).map(|n| n.chars().count()), Some(64));
    assert_eq!(named(&ids[4]), None);
    // The reads the read tier and MCP use never ask.
    for o in engine.observation_log(None, None, None).expect("read") {
        assert!(o.attestations.iter().all(|a| a.app.is_none()), "{} names its app", o.id);
    }
    assert!(engine.observation(&ids[0], None).expect("read").expect("row").attestations[0]
        .app
        .is_none());
    let ledger = Entity::make_id(WS, "Ledger");
    let local = engine.explain_entity_local(&ledger).expect("read").expect("entity");
    assert!(local
        .supporting
        .iter()
        .any(|o| o.attestations.iter().any(|a| a.app.as_deref() == Some("Claude Code"))));
    let plain = engine.explain_entity(&ledger).expect("read").expect("entity");
    assert!(plain.supporting.iter().all(|o| o.attestations.iter().all(|a| a.app.is_none())));
    // The event names it, by the name a person reads.
    engine.emit_from(
        Some("claude-code"),
        supragnosis_core::Event::Traverse { start: ledger.clone(), reached: vec![] },
    );
    assert_eq!(seen.0.lock().unwrap().last().cloned().flatten().as_deref(), Some("Claude Code"));

    // Re-keying keeps it; a dry run writes nothing.
    engine.rekey_workspace(WS, "moved", true).expect("dry run");
    assert!(engine
        .observation_log_local(Some("moved"), None, None)
        .expect("read")
        .is_empty());
    engine.rekey_workspace(WS, "moved", false).expect("rekey");
    let moved = engine.observation_log_local(Some("moved"), None, None).expect("read");
    let claude = moved.iter().find(|o| o.content == "written by claude code").expect("moved row");
    assert_eq!(claude.attestations[claude.author].app.as_deref(), Some("Claude Code"));
}

// --- All is a view (design-system.md Section 29.15) -----------------------------------------

fn propose_in(
    engine: &Engine,
    ws: &str,
    kind: &str,
    targets: Vec<String>,
    into: Option<String>,
) -> Result<String, String> {
    engine
        .propose(ProposeInput {
            workspace: Some(ws.into()),
            kind: kind.into(),
            targets,
            into,
            tier: if kind.starts_with("claim_") { Some("human_confirmed".into()) } else { None },
            rationale: None,
            affected_types: vec![],
            source_ref: None,
            on_behalf_of: None,
        })
        .map_err(|e| e.to_string())
}

/// One proposal event written straight to the log, as an earlier build or another node may have.
fn plant_event(
    store: &InMemoryStore,
    ws: &str,
    at: u64,
    event: ProposalEventKind,
    proposal: &str,
    payload: serde_json::Value,
) -> String {
    let obs = Observation::with_assertions(
        format!("planted {event:?} {proposal}"),
        Provenance {
            host: "host-a".into(),
            on_behalf_of: None,
            workspace: ws.into(),
            source_ref: Some("surface:console".into()),
            observed_at: at,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        },
        Assertions {
            proposal_events: vec![ProposalEventAssertion {
                proposal: proposal.into(),
                event,
                payload: payload.to_string(),
            }],
            ..Default::default()
        },
    );
    let id = obs.id.clone();
    store.add_observation(obs).expect("plant");
    id
}

/// GIVEN the same name in two workspaces, WHEN a merge of the two is proposed, THEN it is refused,
/// naming the other workspace on this local surface; a merge inside one workspace is taken, and so
/// is a target this node does not hold yet (Principle 15: resolution stays inside a workspace).
#[test]
fn p15_a_merge_never_crosses_a_workspace() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let web = Engine::new(store.clone(), "h", "atlas-web");
    observe(&infra, "in infra", &["Payment service", "Payments"]);
    observe(&web, "in web", &["Payment service"]);
    let here = Entity::make_id("infra", "Payment service");
    let there = Entity::make_id("atlas-web", "Payment service");
    let err =
        propose_in(&infra, "infra", "entity_merge", vec![there, here.clone()], Some(here.clone()))
            .expect_err("a merge across workspaces is refused");
    assert!(
        err.contains("joins entities of one workspace") && err.contains("atlas-web"),
        "the refusal does not say why: {err}"
    );
    propose_in(
        &infra,
        "infra",
        "entity_merge",
        vec![Entity::make_id("infra", "Payments"), here.clone()],
        Some(here.clone()),
    )
    .expect("a merge inside one workspace is taken");
    propose_in(&infra, "infra", "entity_merge", vec!["ab".repeat(32), here.clone()], Some(here))
        .expect("a target that has not arrived yet is still taken");
}

/// GIVEN acts an earlier build filed away from what they name - a merge across workspaces with its
/// merge verdict, and an accept filed in the node default for a proposal of another workspace - WHEN
/// the all-workspaces view is read, THEN each proposal reads there as in its own workspace, nothing
/// merges across, and the stray accept is listed in the node-wide report only (proposal-workflow.md
/// I2, store-migration.md Section 6).
#[test]
fn p15_the_all_workspaces_view_is_the_union_of_each_workspaces_fold() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let web = Engine::new(store.clone(), "h", "atlas-web");
    observe(&infra, "in infra", &["Payment service", "Payments"]);
    observe(&web, "in web", &["Payment service"]);
    let here = Entity::make_id("infra", "Payment service");
    let there = Entity::make_id("atlas-web", "Payment service");

    // The merge across workspaces, as a build before step 51a took it.
    let across = plant_event(
        &store,
        "infra",
        10,
        ProposalEventKind::Opened,
        "",
        serde_json::json!({
            "kind": "entity_merge", "targets": [there, here], "into": here,
            "tier": null, "rationale": "an earlier build", "affected_types": [],
        }),
    );
    plant_event(
        &store,
        "infra",
        11,
        ProposalEventKind::Verdict,
        &across,
        serde_json::json!({ "decision": "merge", "note": null }),
    );
    // A merge inside infra, accepted from All by an earlier build: the accept went to the default.
    let inside = propose_in(
        &infra,
        "infra",
        "entity_merge",
        vec![Entity::make_id("infra", "Payments"), here.clone()],
        Some(here.clone()),
    )
    .expect("propose");
    let stray = plant_event(
        &store,
        "default",
        12,
        ProposalEventKind::Verdict,
        &inside,
        serde_json::json!({ "decision": "merge", "note": null }),
    );

    let state = |ws: Option<&str>, id: &str| {
        infra
            .list_proposals(ws)
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| (p.state, p.workspace))
    };
    for (id, want) in [(&across, "blocked"), (&inside, "open")] {
        assert_eq!(state(Some("infra"), id), Some((want.to_string(), "infra".to_string())));
        assert_eq!(
            state(None, id),
            state(Some("infra"), id),
            "All reads a proposal as its workspace does"
        );
    }
    let all = infra.graph(None).expect("graph");
    assert!(
        all.nodes.iter().any(|n| n.id == there)
            && all.nodes.iter().all(|n| !n.workspace.is_empty()),
        "a merge across workspaces still joined the two under All"
    );
    let report = infra.curation(None).expect("curation");
    assert_eq!(report.stray_verdicts.len(), 1, "the stray accept is not listed");
    let s = &report.stray_verdicts[0];
    assert_eq!(
        (
            s.observation.as_str(),
            s.proposal.as_str(),
            s.decision.as_str(),
            s.filed_in.as_str(),
            s.proposal_in.as_str()
        ),
        (stray.as_str(), inside.as_str(), "merge", "default", "infra")
    );
    assert!(
        infra.curation(Some("infra")).expect("curation").stray_verdicts.is_empty(),
        "a workspace's report names another workspace's proposal"
    );
    assert!(
        infra.curation(Some("default")).expect("curation").stray_verdicts.is_empty(),
        "a workspace's report names another workspace's proposal"
    );
    // A confirmation an agent merged in infra, capped at host_signed, beside a console accept an
    // earlier build filed in the default: the tier under All is the tier in infra.
    let billing = infra
        .observe(ObserveInput {
            content: "Billing is a Service".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Billing".into(),
                kind: Some("Service".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let gate =
        propose_in(&infra, "infra", "claim_promotion", vec![billing], None).expect("propose");
    infra
        .review_proposal(
            Some("infra".into()),
            gate.clone(),
            "merge".into(),
            None,
            None,
            VerdictSurface::Agent,
        )
        .expect("an agent's merge");
    plant_event(
        &store,
        "default",
        13,
        ProposalEventKind::Verdict,
        &gate,
        serde_json::json!({ "decision": "merge", "note": null }),
    );
    let tier = |ws: Option<&str>| {
        infra
            .graph(ws)
            .expect("graph")
            .nodes
            .into_iter()
            .find(|n| n.name == "Billing")
            .map(|n| n.trust_tier)
    };
    assert_eq!(tier(Some("infra")), Some(TrustTier::HostSigned), "the agent's merge is capped");
    assert_eq!(
        tier(None),
        tier(Some("infra")),
        "a stray console accept raised the tier under All"
    );
}

/// GIVEN a merge of one workspace, WHEN a split of it is filed in another, THEN it is refused at
/// capture on this local surface, naming where the merge lives; filed with its merge it is taken.
#[test]
fn p15_a_split_names_only_its_own_workspaces_merge() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    observe(&infra, "in infra", &["Payment service", "Payments"]);
    let here = Entity::make_id("infra", "Payment service");
    let merge = propose_in(
        &infra,
        "infra",
        "entity_merge",
        vec![Entity::make_id("infra", "Payments"), here.clone()],
        Some(here),
    )
    .expect("propose");
    let err = propose_in(&infra, "default", "entity_split", vec![merge.clone()], None)
        .expect_err("a split filed away from its merge is refused");
    assert!(err.contains("lives in workspace infra"), "the refusal does not say why: {err}");
    propose_in(&infra, "infra", "entity_split", vec![merge], None).expect("filed with its merge");
}

/// GIVEN a proposal of one workspace, WHEN a verdict, a comment or a withdrawal names it in another
/// workspace, or names an id no proposal has, THEN it is refused and nothing is written; in its own
/// workspace the verdict is taken (P24: kept, the row would decide nothing).
#[test]
fn p24_a_verdict_naming_no_proposal_of_its_workspace_is_refused() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    observe(&infra, "in infra", &["Payment service", "Payments"]);
    let here = Entity::make_id("infra", "Payment service");
    let pid = propose_in(
        &infra,
        "infra",
        "entity_merge",
        vec![Entity::make_id("infra", "Payments"), here.clone()],
        Some(here),
    )
    .expect("propose");
    let before = snapshot(store.as_ref()).observations;
    for (ws, id, decision) in [
        (Some("atlas-web"), pid.as_str(), "reject"),
        (Some("default"), pid.as_str(), "merge"),
        (Some("infra"), "nope", "comment"),
        (Some("infra"), "nope", "withdraw"),
    ] {
        let err = infra
            .review_proposal(
                ws.map(String::from),
                id.into(),
                decision.into(),
                None,
                None,
                VerdictSurface::Agent,
            )
            .expect_err("a verdict that names no proposal of its workspace is taken");
        assert!(err.to_string().contains("no proposal"), "the refusal does not say why: {err}");
    }
    assert_eq!(snapshot(store.as_ref()).observations, before, "a refused verdict wrote a row");
    infra
        .review_proposal(
            Some("infra".into()),
            pid.clone(),
            "reject".into(),
            None,
            None,
            VerdictSurface::Agent,
        )
        .expect("in its own workspace the verdict is taken");
}

/// GIVEN an observation of one workspace, WHEN a gate act filed in another names it, THEN it is
/// refused at capture, naming the other workspace on this local surface; filed in its own workspace
/// it is taken (proposal-workflow.md Section 6, resolution.md Section 5).
#[test]
fn p15_a_gate_act_names_only_its_own_workspaces_observations() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let obs = infra
        .observe(ObserveInput {
            content: "Payments is a Service".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Payments".into(),
                kind: Some("Service".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let err = propose_in(&infra, "default", "claim_promotion", vec![obs.clone()], None)
        .expect_err("a gate act on another workspace's observation is refused");
    assert!(err.contains("is in workspace infra"), "the refusal does not say why: {err}");
    propose_in(&infra, "infra", "claim_promotion", vec![obs], None)
        .expect("in its own workspace it is taken");
}

/// GIVEN a region read under All, WHEN it is named with no workspace, THEN it is asserted in the
/// workspace of the observations that asserted it, and no name is minted anywhere else
/// (design-system.md Section 29.15, P17).
#[test]
fn p17_a_region_named_under_all_is_filed_where_it_was_asserted() {
    let store = Arc::new(InMemoryStore::new());
    // The node default is not the region's workspace, so filing in the default would show.
    let secret = Engine::new(store.clone(), "h", "default");
    secret
        .observe(ObserveInput {
            content: "together".into(),
            workspace: Some("secret".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: ["Atlas", "Layers", "Tiles"]
                .iter()
                .map(|n| EntityInput { name: (*n).into(), kind: None, description: None })
                .collect(),
            relations: vec![],
        })
        .expect("observe");
    let hg = secret.hypergraph(None).expect("hypergraph");
    let h = hg.hyperedges.iter().find(|h| h.size == 3).expect("the region");
    let out = secret
        .reify_hyperedge(supragnosis_engine::ReifyInput {
            workspace: None,
            hyperedge: h.id.clone(),
            name: Some("Mapping".into()),
            kind: None,
            source_ref: None,
            on_behalf_of: None,
        })
        .expect("reify");
    let o = store
        .get_observation(&out.observation_id)
        .expect("get")
        .expect("the region's observation");
    assert_eq!(o.workspace(), "secret", "the region was filed away from its members");
    assert!(store.get_entity(&Entity::make_id("secret", "Mapping")).expect("get").is_some());
    for name in ["Mapping", "Atlas"] {
        assert!(
            store.get_entity(&Entity::make_id("default", name)).expect("get").is_none(),
            "{name} was minted in the node default"
        );
    }
}

/// GIVEN the same name in three workspaces, one of them merged into another entity there, WHEN the
/// namesakes of one are read, THEN the others that still stand on their own are listed in workspace
/// order, the merged-away one is left out, and an unknown id is unknown (design-system.md Section
/// 29.16).
#[test]
fn p15_namesakes_are_the_same_name_standing_in_other_workspaces() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let web = Engine::new(store.clone(), "h", "web");
    let ops = Engine::new(store.clone(), "h", "ops");
    observe(&infra, "in infra", &["Payment service"]);
    observe(&web, "in web", &["Payment Service"]);
    observe(&ops, "in ops", &["Payment service", "Payments"]);
    let gone = Entity::make_id("ops", "Payment service");
    let kept = Entity::make_id("ops", "Payments");
    let merge = propose_in(&ops, "ops", "entity_merge", vec![gone, kept.clone()], Some(kept))
        .expect("propose");
    ops.review_proposal(
        Some("ops".into()),
        merge,
        "merge".into(),
        None,
        None,
        VerdictSurface::Console,
    )
    .expect("merge");
    let here = Entity::make_id("infra", "Payment service");
    let found = infra
        .namesakes(&here, &["ops", "web", "infra", "nowhere"])
        .expect("read")
        .expect("the entity is known");
    let said: Vec<(String, String)> = found.into_iter().map(|n| (n.workspace, n.name)).collect();
    assert_eq!(said, vec![("web".to_string(), "Payment Service".to_string())]);
    assert!(infra.namesakes(&"ab".repeat(32), &["web"]).expect("read").is_none());
}

/// GIVEN a merge the console opens, WHEN its opening observation is read, THEN it carries the
/// console's mark, engine-stamped, so who asked reads as the person; a source of its own is refused,
/// and a client still cannot name the mark itself (design-system.md Section 29.17, resolution.md R8).
#[test]
fn p2_a_merge_opened_at_the_console_carries_its_mark() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    observe(&infra, "in infra", &["Postgres", "PostgreSQL"]);
    let (a, b) = (Entity::make_id("infra", "Postgres"), Entity::make_id("infra", "PostgreSQL"));
    let input = |source_ref: Option<&str>| ProposeInput {
        workspace: Some("infra".into()),
        kind: "entity_merge".into(),
        targets: vec![a.clone(), b.clone()],
        into: Some(b.clone()),
        tier: None,
        rationale: Some("two entities named as one from a search".into()),
        affected_types: vec![],
        source_ref: source_ref.map(String::from),
        on_behalf_of: None,
    };
    let id = infra.propose_at_console(input(None)).expect("open at the console");
    let opened = store.get_observation(&id).expect("get").expect("the opening");
    assert!(
        opened
            .provenance
            .iter()
            .all(|p| p.source_ref.as_deref() == Some(VERDICT_SURFACE_CONSOLE)),
        "the opening does not carry the console's mark"
    );
    assert!(
        infra.propose_at_console(input(Some("git:abc"))).is_err(),
        "a source of its own was taken"
    );
    assert!(
        infra.propose(input(Some(VERDICT_SURFACE_CONSOLE))).is_err(),
        "a client minted the mark"
    );
}

/// GIVEN a merge, its un-merge, and the same merge opened again, WHEN the console counts the
/// un-merges of that pair in that direction, THEN it counts each one, so its re-merge reason makes a
/// different proposal from every merge a split reversed (unmerge.md S10).
#[test]
fn s10_the_console_counts_the_unmerges_a_re_merge_follows() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    observe(&infra, "in infra", &["Postgres", "PostgreSQL"]);
    let (a, b) = (Entity::make_id("infra", "Postgres"), Entity::make_id("infra", "PostgreSQL"));
    let decide = |id: &str| {
        infra
            .review_proposal(
                Some("infra".into()),
                id.into(),
                "merge".into(),
                None,
                None,
                VerdictSurface::Console,
            )
            .expect("merge");
    };
    assert_eq!(infra.unmerges_of("infra", &[a.clone(), b.clone()], &b).expect("count"), 0);
    let merge =
        propose_in(&infra, "infra", "entity_merge", vec![a.clone(), b.clone()], Some(b.clone()))
            .expect("merge");
    decide(&merge);
    let split = propose_in(&infra, "infra", "entity_split", vec![merge], None).expect("split");
    decide(&split);
    assert_eq!(infra.unmerges_of("infra", &[b.clone(), a.clone()], &b).expect("count"), 1);
    assert_eq!(
        infra.unmerges_of("infra", &[a.clone(), b.clone()], &a).expect("count"),
        0,
        "the other direction counted"
    );
}

/// GIVEN an entity said twice and a second entity merged into it, WHEN the graph and the hypergraph
/// are read, THEN the entity's time is the latest observation among its rows, the merged-away row's
/// included, and both reads give it the same time (design-system.md Section 29.19, P16).
#[test]
fn p16_an_entity_is_last_said_at_its_latest_attestation() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let at = |n: &str, t: u64| {
        store
            .add_observation(Observation::with_assertions(
                format!("{n} at {t}"),
                Provenance {
                    host: "h".into(),
                    on_behalf_of: None,
                    workspace: "infra".into(),
                    source_ref: None,
                    observed_at: t,
                    confidence: None,
                    trust_tier: TrustTier::default(),
                    sync: None,
                },
                Assertions {
                    entities: vec![supragnosis_core::EntityAssertion {
                        name: n.into(),
                        kind: None,
                        description: None,
                    }],
                    ..Default::default()
                },
            ))
            .expect("add");
    };
    at("Postgres", 1_000);
    at("Postgres", 3_000);
    at("PostgreSQL", 5_000);
    infra.reproject(Some("infra")).expect("reproject");
    let (a, b) = (Entity::make_id("infra", "PostgreSQL"), Entity::make_id("infra", "Postgres"));
    let time = |id: &str| {
        let g = infra.graph(Some("infra")).expect("graph");
        let h = infra.hypergraph(Some("infra")).expect("hypergraph");
        (
            g.nodes.iter().find(|n| n.id == id).and_then(|n| n.last_said),
            h.nodes.iter().find(|n| n.id == id).and_then(|n| n.last_said),
        )
    };
    assert_eq!(time(&b), (Some(3_000), Some(3_000)), "the latest of two observations");
    let merge = propose_in(&infra, "infra", "entity_merge", vec![a, b.clone()], Some(b.clone()))
        .expect("propose");
    infra
        .review_proposal(
            Some("infra".into()),
            merge,
            "merge".into(),
            None,
            None,
            VerdictSurface::Console,
        )
        .expect("merge");
    assert_eq!(time(&b), (Some(5_000), Some(5_000)), "the merged-away row's time counts");
}

/// GIVEN a source a person confirmed, WHEN an agent merges a claim demotion of it to human_confirmed,
/// THEN the source keeps human_confirmed - a demotion needs no ceiling, so it lowers to the tier it
/// asks for and no further (resolution.md Section 6, design-system.md Section 29.20).
#[test]
fn p18_a_demotion_lowers_to_the_tier_it_asks_for() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let obs = infra
        .observe(ObserveInput {
            content: "Ledger is a Table".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Ledger".into(),
                kind: Some("Table".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let decide = |id: String, surface| {
        infra
            .review_proposal(Some("infra".into()), id, "merge".into(), None, None, surface)
            .expect("merge");
    };
    let up =
        propose_in(&infra, "infra", "claim_promotion", vec![obs.clone()], None).expect("promote");
    decide(up, VerdictSurface::Console);
    let tier = || {
        infra
            .graph(Some("infra"))
            .expect("graph")
            .nodes
            .into_iter()
            .find(|n| n.name == "Ledger")
            .map(|n| n.trust_tier)
    };
    assert_eq!(tier(), Some(TrustTier::HumanConfirmed));
    let down = propose_in(&infra, "infra", "claim_demotion", vec![obs], None).expect("demote");
    decide(down, VerdictSurface::Agent);
    assert_eq!(tier(), Some(TrustTier::HumanConfirmed), "the agent's surface capped a demotion");
}

/// GIVEN a claim promotion to human_confirmed, WHEN an agent merges it and then a person does,
/// THEN its granted tier is host_signed, the agent surface's ceiling, and then human_confirmed, the
/// highest surface among its merge verdicts (design-system.md Section 29.20).
#[test]
fn p18_a_merged_promotion_says_what_it_granted() {
    let store = Arc::new(InMemoryStore::new());
    let infra = Engine::new(store.clone(), "h", "infra");
    let obs = infra
        .observe(ObserveInput {
            content: "Ledger is a Table".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![EntityInput {
                name: "Ledger".into(),
                kind: Some("Table".into()),
                description: None,
            }],
            relations: vec![],
        })
        .expect("observe")
        .observation_id;
    let up = propose_in(&infra, "infra", "claim_promotion", vec![obs], None).expect("promote");
    let granted = |id: &str| {
        let listed = infra
            .list_proposals(Some("infra"))
            .expect("list")
            .into_iter()
            .find(|p| p.id == id)
            .and_then(|p| p.granted_tier);
        let got = infra.get_proposal(Some("infra"), id).expect("get").and_then(|p| p.granted_tier);
        assert_eq!(listed, got, "list and get disagree");
        got
    };
    assert_eq!(granted(&up), None, "an open promotion grants nothing");
    infra
        .review_proposal(
            Some("infra".into()),
            up.clone(),
            "merge".into(),
            None,
            None,
            VerdictSurface::Agent,
        )
        .expect("agent merge");
    assert_eq!(granted(&up), Some(TrustTier::HostSigned));
    infra
        .review_proposal(
            Some("infra".into()),
            up.clone(),
            "merge".into(),
            None,
            None,
            VerdictSurface::Console,
        )
        .expect("console merge");
    assert_eq!(granted(&up), Some(TrustTier::HumanConfirmed));
}
