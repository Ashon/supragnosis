//! The principle coverage registry - policy as an executable artifact.
//!
//! `principles.md` Appendix B is a review checklist: questions a human is supposed to ask during a
//! PR. That is the same shape of guarantee as a "guarded by <test>" claim with no CI job behind it -
//! it holds exactly as long as someone remembers. This file closes that loop from the other side:
//! every principle must **declare how it is checked**, and the declaration is itself checked.
//!
//! **The unit is the clause, not the principle.** A principle is a conjunction of demands, and
//! filing one verdict per principle hides the demands that are not met behind the ones that are:
//! P2 read as guarded while its headline clause ("at least one attestation, refused at the schema
//! level") was an overdue debt, because two other clauses of P2 had tests. So [`REGISTRY`] carries
//! [`Clause`] rows - what the principle demands, one line each - and the evidence hangs off those.
//!
//! Four evidence states, and the point is that there is no fifth:
//!
//! - [`Evidence::Scenario`] - the clause holds, and these named tests must exist *and run*.
//!   Renaming, deleting, un-`#[test]`ing or `#[ignore]`ing one fails here, so a clause cannot
//!   quietly lose its guard.
//! - [`Evidence::Structural`] - the clause holds by construction rather than by a test (a crate
//!   graph that cannot express the violation, an exhaustive `match` that will not compile). A
//!   reason is mandatory: "structural" without a stated mechanism is just an unchecked clause with
//!   a nicer label.
//! - [`Evidence::Characterized`] - the clause does **not** hold; these running tests pin the
//!   current behavior so that repaying it must rewrite them. Must also name the milestone.
//! - [`Evidence::Deferred`] - the clause does not hold and nothing pins it. Must name the milestone
//!   that repays it, so this file and architecture.md Section 14 cannot drift into disagreeing
//!   about what is owed.
//!
//! `Characterized` is the state this registry originally lacked, and its absence is what made the
//! per-principle version comfortable to read: with only three states, a clause pinned by a
//! characterization test had nowhere to go except `Scenario`, where it counted as evidence that the
//! clause was met - the exact opposite of what such a test asserts. Splitting "a test exists" from
//! "the clause holds" is the whole reason the summary reports guarded clauses out of all clauses
//! instead of guarded principles.
//!
//! Adding Principle 24 to `docs/principles.md` breaks [`every_principle_declares_its_evidence`]
//! until someone writes down what it demands and which of the four states each demand is in. That
//! is the whole design: the registry cannot be silently incomplete, which is the failure mode a
//! checklist has by nature.
//!
//! Three couplings make that real, and each was once claimed rather than held:
//!
//! - **To the document.** `docs/principles.md` is embedded and parsed here, so the principle set is
//!   read from the normative text rather than restated as a constant. A constant compared against
//!   the registry in the same file is a tautology - it agrees with itself while the document walks
//!   away. The registry's short name must be the document's own, so renumbering or replacing a
//!   principle cannot leave a row silently standing for the old one.
//! - **To the test runner.** A declared scenario must be a test that actually *runs*: a
//!   `#[test]`/`#[tokio::test]` with no `#[ignore]`. Checking only that `fn <name>` appears
//!   somewhere cannot tell a guard from a dead function - drop the attribute and the guard stops
//!   running while the registry keeps reporting it as evidence.
//! - **To the other design documents.** This registry is not the only place that promises a guard:
//!   architecture.md Section 14 is written in "guarded by <test>" sentences, and the resolution
//!   documents carry test-plan tables. [`design_docs_name_tests_that_run`] holds those to the same
//!   standard, because a promise the registry keeps and a promise beside it breaks is still a
//!   broken promise to whoever reads the documents.
//!
//! This file deliberately does not re-run those tests - `cargo test` already does. It guards the
//! *map*, and the three couplings above are what keep the map pinned to the territory.

/// How one clause is actually checked today. The first two mean the clause HOLDS; the last two mean
/// it does not, and differ only in whether anything pins the gap.
#[derive(Debug, Clone, Copy)]
enum Evidence {
    /// Enforced, and these tests prove it. Every name must exist AND run (see [`declares`]).
    Scenario(&'static [&'static str]),
    /// Enforced by construction. The reason must name the mechanism that makes violation
    /// unrepresentable, not merely unlikely.
    Structural(&'static str),
    /// **Not enforced.** These tests pin the current non-compliant behavior, so repaying the clause
    /// has to rewrite them - a passing characterization test is a record, not an endorsement
    /// (principle_scenarios.rs says the same in its header). Names must run; the reason must name
    /// the repayment milestone. This is the state that keeps an unmet clause from being filed as
    /// `Scenario` merely because a test mentions it.
    Characterized(&'static [&'static str], &'static str),
    /// **Not enforced**, and nothing pins it. Must name the repayment milestone
    /// (architecture.md Section 14).
    Deferred(&'static str),
}

impl Evidence {
    /// Whether the clause is actually met today. The coverage report exists to keep this
    /// distinction visible, since it is the one a reader is most likely to assume.
    fn holds(&self) -> bool {
        matches!(self, Evidence::Scenario(_) | Evidence::Structural(_))
    }
}

/// One clause of a principle, and how that specific demand is checked.
///
/// The unit is the clause and not the principle because principles are conjunctions: P2 asks for
/// provenance on every assertion AND for that to be refused at the schema level, and only the first
/// half is true. Filed per principle, the second half disappeared behind the first - the registry
/// reported P2 as guarded while its headline clause was an overdue debt. Per clause there is
/// nowhere for it to hide: it has to be written down and given one of the four states above.
struct Clause {
    /// What the principle demands, in one line. This is the thing the evidence is evidence OF.
    demands: &'static str,
    evidence: Evidence,
}

const fn c(demands: &'static str, evidence: Evidence) -> Clause {
    Clause { demands, evidence }
}

/// The normative document itself, embedded at compile time. The registry is checked against what
/// this text declares, not against a count restated here - so a principle added to the document is
/// a failure in this file until it is given an evidence state.
const PRINCIPLES_DOC: &str = include_str!("../../../docs/principles.md");

/// The design documents, which make "guarded by <test>" claims of their own - test-plan tables in
/// resolution.md / resolution-identity.md, and the compliance ledger in architecture.md. Those
/// claims are the same shape of promise as this registry and were rotting the same way: five names
/// in resolution.md's table pointed at tests that had been renamed years of commits earlier.
const DESIGN_DOCS: &[(&str, &str)] = &[
    ("docs/principles.md", PRINCIPLES_DOC),
    ("docs/architecture.md", include_str!("../../../docs/architecture.md")),
    ("docs/resolution.md", include_str!("../../../docs/resolution.md")),
    (
        "docs/resolution-identity.md",
        include_str!("../../../docs/resolution-identity.md"),
    ),
    ("docs/proposal-workflow.md", include_str!("../../../docs/proposal-workflow.md")),
    ("docs/federation.md", include_str!("../../../docs/federation.md")),
    // Names no tests yet - nothing in it is built. Listed now so that the first "guarded by <test>"
    // sentence it grows is checked from the day it is written, rather than on the day someone
    // remembers this list exists.
    ("docs/excision.md", include_str!("../../../docs/excision.md")),
    // Same reason as excision.md: specified, unbuilt, and listed before it names a guard rather
    // than after. Unlike excision it reverses something that already ships, so the first test it
    // names will be one that an existing behaviour has to keep passing.
    ("docs/unmerge.md", include_str!("../../../docs/unmerge.md")),
];

/// Sources scanned for the declared test names. Embedded at compile time, so this test performs no
/// IO and cannot go stale against a moved file without failing to build.
const SOURCES: &[&str] = &[
    include_str!("principle_scenarios.rs"),
    include_str!("policy_cases.rs"),
    include_str!("recall_eval.rs"),
    include_str!("read_path_cost.rs"),
    include_str!("../src/lib.rs"),
    include_str!("../../supragnosis-core/src/lib.rs"),
    include_str!("../../supragnosis-store/src/lib.rs"),
    include_str!("../../supragnosis-store/tests/port_conformance.rs"),
    include_str!("../../supragnosis-sync/src/lib.rs"),
    include_str!("../../supragnosis-sync/src/http.rs"),
    include_str!("../../supragnosis-embed/src/lib.rs"),
    include_str!("../../supragnosis-mcp/tests/mcp_surface.rs"),
    include_str!("../../supragnosis-viz/tests/http.rs"),
    include_str!("../../supragnosis-viz/src/lib.rs"),
    include_str!("../../supragnosis-cli/src/main.rs"),
];

/// One row per principle in `docs/principles.md`, in order, each carrying the clauses that
/// principle actually demands. The name is the document's own (see
/// [`every_principle_declares_its_evidence`]); the clauses come from architecture.md Section 14,
/// which already partitions each principle into what is satisfied and what is owed - in prose.
/// This is that partition as data, so the owed half cannot be read past.
const REGISTRY: &[(u8, &str, &[Clause])] = &[
    (1, "Assertion-Belief Separation", &[
        c("the graph is a projection: re-deriving it from the log reproduces it exactly",
          Evidence::Scenario(&[
            "observations_carry_assertions_in_log",
            "p1_reprojection_rederives_without_touching_the_log",
            "incremental_write_equals_replay",
            // The clause says "the graph"; the guard above reads entities. Relations were the half
            // nobody checked, and they diverged - observe stamped an edge with the attestation of the
            // call that wrote it while reprojection used the authoring attestation, so a replay could
            // move an edge's tier with no change in the log. Both run one fold now, and this is the
            // half of the claim that had no evidence.
            "incremental_write_equals_replay_for_relations",
        ])),
        // The clause above says re-deriving the graph from the log reproduces it. That is only true
        // while nothing writes a row the log never knew about, which was a convention and is now a
        // type. The convention did hold - the whole workspace has exactly two calls to the projection
        // writes, both inside the folds - but the author's own store still carries 35 entity rows no
        // observation asserts, from an era when something did reach for the handle. They survive every
        // re-projection, never cross the sync wire, and a replay cannot reproduce them.
        //
        // No test can guard this: it would have to enumerate callers that do not exist yet.
        c("no API may write a fact that did not pass through an assertion directly into the graph",
          Evidence::Structural(
            "The store port is split: `AssertionStore` appends to the log and reads the graph, and \
             `KnowledgeStore: AssertionStore` adds `put_entity`/`add_relation`. The engine holds the \
             full trait and is the only thing that does - `Engine::store()` hands out the narrow one, \
             so the sync crate applying replicated events, the MCP tools and the CLI cannot reach the \
             projection writes. Knowledge enters through a fold or not at all.",
        )),
        c("a generator proposes; nothing but a verdict commits",
          Evidence::Scenario(&["merge_suggestions_never_commit"])),
        // A read may reuse the rows it already loaded, but only where reusing them is
        // indistinguishable from reading again - otherwise the projection stops being a function
        // of the log and starts being a function of when it was looked at.
        c("a read is answered from the log as it stands, not from a stale view of it",
          Evidence::Scenario(&[
            "a_read_context_reuses_rows_only_while_that_changes_nothing",
            "a_shared_context_answers_what_separate_reads_answer",
            "a_read_walks_the_log_once",
            "a_read_does_not_query_the_store_per_item",
        ])),
        // Half a refusal and half a permission, so the guard asserts both: a non-assertion is
        // refused before the log, and notation variance is NOT (normalizing is the projection's job).
        c("ingest validates well-formedness and nothing beyond it",
          Evidence::Scenario(&["formless_assertions_are_rejected_before_logging"])),
    ]),
    (2, "Provenance First, Identity as Delegation Chain", &[
        c("an attestation carries its acting host, principal, workspace and time, and an unstated \
           confidence stays unstated",
          Evidence::Scenario(&[
            "confidence_out_of_range_is_rejected",
            "unstated_confidence_is_distinct_from_full_confidence",
            // A workspace re-key carries acting host / principal / observed_at / confidence
            // verbatim, where a re-ingest through observe would restamp them all.
            "p2_a_workspace_rekey_carries_provenance_that_a_reingest_would_restamp",
            // Attribution follows the authoring attestation (earliest effective HLC), not the
            // sort-first host or the latest observed_at of an absorbed union.
            "p2_proposal_attribution_names_the_authoring_attestation",
        ])),
        c("a peer's claimed tier is stored verbatim, because the log is audit",
          Evidence::Scenario(&["f13_sync_apply_stores_senders_self_declared_tier_verbatim"])),
        c("at least one attestation, refused at ingest at the schema level",
          Evidence::Characterized(
            &["p2_at_least_one_attestation_is_a_constructor_guarantee_not_a_checked_one"],
            "OVERDUE - declared an M4 entry condition, and M4 Phases 0-4 shipped without it. The \
             clause holds only because no constructor produces the empty case (architecture.md \
             Section 14, overdue entry condition 1)",
        )),
    ]),
    (3, "Supersede, Don't Delete", &[
        c("a re-arrival merges monotonically and drops nothing, in any order",
          Evidence::Scenario(&[
            "absorb_union_is_order_independent_and_idempotent",
            "p3_a_new_spelling_accumulates_and_never_displaces",
            "log_retains_all_attestations_on_reobservation",
            // The same demand asked of the port rather than of one backend: absorb is what
            // `add_observation` promises, so an adapter that replaced the row would satisfy every
            // engine-level test above and still destroy provenance.
            "reobservation_absorbs_attestations_and_lineage",
            "reobservation_converges_regardless_of_arrival_order",
            // The live-set door supersedes only within one workspace: a cross-workspace re-key
            // keeps both rows live, so the unscoped view drops nothing a scoped view still shows.
            "p3_a_rekey_keeps_the_source_row_live_in_the_unscoped_view",
        ])),
        c("a destruction demand leaves an absorbing tombstone that propagates and refuses re-ingest",
          Evidence::Deferred(
            "M4 Phase 5 - the first multi-principal deployment is the first time such a demand can \
             arrive from someone who is not the operator (architecture.md Section 14)",
        )),
        c("every encoding the log has ever used stays readable",
          Evidence::Scenario(&[
            // Append-only cuts both ways: a row that stops parsing is destroyed in effect. While one
            // store held every era, this was a permanent read shim inside that adapter. The store
            // changed, so the demand moved rather than lapsed: the encodings this build cannot read
            // are still readable by the release that wrote them, and the guard below makes skipping
            // that release fail loudly instead of starting empty beside a full store - which is the
            // only way the old rows could actually be lost.
            "a_legacy_store_is_recognised_by_its_rocksdb_marker",
            "an_unmigrated_store_is_refused_with_the_way_out",
        ])),
        c("a relation accumulates attestations the way an entity and an observation do",
          Evidence::Deferred(
            "M3c/M5 - relation provenance is still a single attestation, so a second assertion of \
             the same edge replaces rather than accumulates. Recorded in architecture.md Section 14 \
             under the Principle 1/6 deferral but never given a clause here; the conflict-surfacing \
             half is Principle 6's own deferred row",
        )),
        // Named by three documents and tracked by none until now: P3 demands it here, P15 demands
        // both directions of it, and proposal-workflow.md counts "entity-merge / split" as one of
        // the five gated intents. The implementation shipped five kinds with the split half missing,
        // so a merge is the only canon change with no way back.
        c("entity merge preserves history, so un-merge is possible",
          Evidence::Deferred(
            "M4 Phase 5, with P23's \"every proposal kind has a commit effect\" - the same hole from \
             the other side. Specified in docs/unmerge.md: the mechanism is one edge leaving \
             merge_forwarding, and the cost is the four decisions that document fixes",
        )),
        c("a re-materialization concurrent with an observe cannot interleave",
          Evidence::Deferred(
            "Revisit with a store-level atomic upsert - `reproject` does not take `write_guard`, so \
             a replay concurrent with an observe can interleave (architecture.md Section 14). What \
             keeps it harmless today is a deployment fact (replay runs with the daemon stopped, or \
             from the post-apply sync hook), which is the class of argument that ledger exists to \
             retire. M3's write path was supposed to repay it and shipped without it",
        )),
    ]),
    (4, "Bi-Temporality", &[
        // Capture is the half that cannot be added retroactively, so it is the half that must be
        // guarded now even though the query logic is not built.
        c("both time axes are captured at ingest, into the log and the projection",
          Evidence::Scenario(&["relation_valid_interval_is_captured_in_log_and_projection"])),
        c("as_of_valid / as_of_recorded time travel, and automatic valid_to closing",
          Evidence::Deferred(
            "M3c - blocked on the explicit negative assertion Principle 5 does not model yet; \
             non-destructive because capture is complete",
        )),
    ]),
    (5, "Open World Assumption", &[
        c("absence is a well-formed answer, never an error",
          Evidence::Scenario(&[
            "p5_absent_entity_is_none_not_error",
            // Asked of every read on the port, on every adapter: the clause is about the storage
            // layer's whole surface, and one entity lookup is one of eight places it could break.
            "absence_reads_as_absence_never_as_error",
        ])),
        c("absent is distinguished from unavailable, rather than collapsing into an empty result",
          Evidence::Scenario(&[
            "merge_band_reports_whether_it_could_run_and_over_how_much",
            "p5_a_diff_for_an_unenforced_kind_reports_uncomputable_not_empty",
        ])),
    ]),
    (6, "Contradiction Is Signal", &[
        c("a conflict that trust does not settle surfaces as contested instead of resolving silently",
          Evidence::Scenario(&[
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "p6_contradictory_merge_cycle_is_convergent_and_surfaced",
            "contested_iff_top_tier_ties",
        ])),
        c("a contradiction between relations is surfaced too",
          Evidence::Deferred(
            "M3c/M5 - relations coexist rather than conflict until an explicit negative assertion \
             exists to contradict them with",
        )),
    ]),
    (7, "Forgetting as Demotion, Consolidation as Re-Projection", &[
        c("consolidation generates candidates and commits none of them",
          Evidence::Scenario(&[
            "merge_suggestions_never_commit",
            "name_variants_stop_being_offered_once_a_merge_is_open",
            "p7_curation_generates_candidates_and_commits_nothing",
        ])),
        c("forgetting happens as recall demotion at idle, never as deletion",
          Evidence::Deferred("M6 - the generate side landed early with M3.5; the demotion side does not exist")),
    ]),
    (8, "Clarity", &[
        // The clause with teeth is a refusal, checked on both entry points: a passing-path test
        // cannot tell an enforced validator from a deleted one.
        c("a type cannot enter the vocabulary without a natural-language definition",
          Evidence::Scenario(&["p8_a_type_definition_without_a_description_is_refused_on_both_paths"])),
        c("a description already captured is never erased by a later omission",
          Evidence::Scenario(&["p8_description_survives_reobservation_without_one"])),
    ]),
    (9, "Coherence", &[
        c("conflicting definitions of one type surface as contested",
          Evidence::Scenario(&["type_def_conflict_surfaces_contested"])),
        // A structural contradiction is a bug, unlike a contradiction between assertions - so it
        // blocks the merge rather than merely surfacing.
        c("a name defined on both T-Box axes is surfaced, and blocks a tbox_change merge",
          Evidence::Scenario(&[
            "type_axis_collision_is_a_signal",
            "p23_a_blocked_merge_verdict_does_not_reach_canon",
        ])),
        c("subtype cycles and domain/range coherence are checked",
          Evidence::Deferred(
            "Revisit when subtyping is introduced - no subtype hierarchy exists in the T-Box, so \
             this clause has nothing to bite on yet",
        )),
    ]),
    (10, "Extendibility / Open-Closed", &[
        c("the domain vocabulary extends through the log without touching the core model",
          Evidence::Scenario(&["types_fold_orders_by_hlc_not_observed_at"])),
        // Three 0.x eras changed the assertion encoding; `migrate` is that path honored rather
        // than promised.
        c("a change to the core model comes with a migration path",
          Evidence::Scenario(&["legacy_id_rows_stay_local_and_migrate"])),
    ]),
    (11, "Minimal Commitment, Induced Schema", &[
        c("second-order structure is a derived view identified by its member set, coexisting with \
           binary relations rather than replacing them",
          Evidence::Scenario(&[
            "hypergraph_recovers_co_assertion",
            "hypergraph_dedup_by_member_set_accumulates_sources",
            "p15_hypergraph_membership_forwards_accepted_merges",
        ])),
        c("promoting a recurring context is an ordinary gated assertion that carries its lineage",
          Evidence::Scenario(&["p11_reify_asserts_group_with_lineage"])),
        // P11 fixes the T-Box's scope at the workspace, and this registry had no row for it - so the
        // demand had no guard, and the all-workspaces glossary merged same-named types out of
        // unrelated workspaces without anything noticing. The clause omission is the more useful half
        // of that finding: the completeness test couples to the PRINCIPLE set, so a principle can be
        // present while one of its demands is missing entirely.
        c("the T-Box is scoped to the workspace - an all-workspaces read is a union of glossaries, \
           not one glossary",
          Evidence::Scenario(&["p11_the_all_workspaces_glossary_does_not_merge_across_workspaces"])),
        c("type candidates are induced from repeated co-occurrence",
          Evidence::Deferred(
            "M5 with the Extractor port - the substrate exists, but naming an induced type is \
             probabilistic and belongs with the extractor (IR6)",
        )),
    ]),
    (12, "Minimal Encoding Bias", &[
        c("a storage concept cannot reach the domain model",
          Evidence::Structural(
            "supragnosis-core declares no store/embedder dependency in its Cargo.toml, so the \
             violation is unrepresentable rather than merely discouraged. Shares its enforcement \
             with Principle 20.",
        )),
    ]),
    (13, "Rigidity - OntoClean", &[
        c("essence is distinguished from role, and a role cannot subsume an essence",
          Evidence::Deferred(
            "Revisit when subtyping is introduced - there is no subtype hierarchy for the \
             distinction to constrain, so define_type treats it as a written guideline \
             (architecture.md Section 14)",
        )),
    ]),
    (14, "Stable Identifiers", &[
        c("identifiers are content-derived, collision-resistant and independent of notation",
          Evidence::Scenario(&[
            "length_prefix_blocks_boundary_collision",
            "observation_id_includes_assertions",
            "relation_id_is_notation_independent",
            "node_id_derives_from_public_key_and_is_stable",
        ])),
        c("an identifier stays resolvable after the thing it names is merged away",
          Evidence::Scenario(&["get_entity_forwards_a_merged_id"])),
        c("one content counts once even when a re-keying leaves it under two ids",
          Evidence::Scenario(&[
            // Content-address dedup normally makes this automatic. `migrate` is the case where one
            // content wears two ids on purpose (the old row stays, P3), so the same rule has to be
            // applied by hand or the folds report one act as two.
            "p14_migration_rekeys_an_act_without_duplicating_it",
            "legacy_id_rows_stay_local_and_migrate",
        ])),
        c("every identifier the system hands out is dereferenceable",
          Evidence::Deferred(
            "Revisit with the MCP resource surface - supragnosis://entity/{id} does not resolve \
             (architecture.md Section 7). The ledger records this as a standing gap and assigns it \
             to no milestone, so this is the registry's own unscheduled entry",
        )),
    ]),
    (15, "Resolution Is Substrate's Job", &[
        c("the substrate proposes identity candidates rather than leaving them to the operator",
          Evidence::Scenario(&[
            "merge_suggestions_never_commit",
            "name_variant_ladder_catches_orthographic_duplicates_without_an_embedder",
        ])),
        c("a proposed identity is committed by the gate, never by the generator",
          Evidence::Scenario(&["p15_hypergraph_membership_forwards_accepted_merges"])),
        c("top-band candidates merge automatically",
          Evidence::Deferred("M4+ - deliberately not done; the auto-merge executor needs I15 re-validation")),
    ]),
    (16, "Topology-Independent Convergence", &[
        c("one observation set converges to one state regardless of order, partitioning or duplication",
          Evidence::Scenario(&[
            "p16_canonical_name_selection_is_arrival_order_free",
            "p16_partitioned_and_duplicated_delivery_converges",
            "absorb_converges_under_random_arrival_orders",
            "two_nodes_converge_under_any_exchange_order",
            "cross_node_reprojection_converges",
            "i8_blocking_check_conclusion_is_arrival_order_independent",
            // architecture.md Section 14 already called this the P16 determinism guard; it was
            // never declared here. It also covers the tied-HLC branch, where convergence rests on
            // the id tiebreak rather than on recency.
            "aliases_accumulate_and_converge",
        ])),
        c("a query response is reproducible, and ties and truncation break on a stable key",
          Evidence::Scenario(&[
            "p16_search_ties_break_by_id_and_repeat_identically",
            // P16 names a hash map's iteration order leaking into a response as a violation on its
            // own. The adapters did not agree on one order - InMemory enumerated a HashMap, Cozo a
            // Datalog result - and the defence was to prove no fold depended on the order. The port
            // now promises the order instead (ascending id, stated on the trait), so the divergence
            // is closed where it arose and every adapter is held to it by one suite.
            "enumerations_are_ordered_by_id",
            // Kept, and not made redundant by that promise: "no answer depends on enumeration order"
            // is the stronger property, and it is what would make a later re-ordering safe. The
            // promise removes a hazard; this guard is why removing it is allowed to be cheap.
            "read_surfaces_do_not_depend_on_enumeration_order",
            "traverse_bounds_depth_and_truncates_nearest_first",
            "traverse_passes_through_an_unprojected_endpoint",
            "search_truncation_is_reproducible",
        ])),
    ]),
    (17, "Knowledge Sovereignty", &[
        c("sharing is opt-in per workspace and enforced at the sync boundary, federated recall included",
          Evidence::Scenario(&["export_respects_share_list_and_vv"])),
        // The clause used to read "the local read surface is reachable only by the local
        // principal" and cite these same tests - an over-claim: none of them touches the MCP
        // streamable-http daemon, which is a local surface these guards do not confine. What the
        // tests actually evidence is the viewer socket and the sync bind; the daemon has its own
        // row below, in the state it is actually in.
        c("the viewer socket and the sync bind admit only the local principal or an \
           authenticated peer",
          Evidence::Scenario(&[
            "p17_socket_directory_denies_foreign_users_before_the_socket_mode",
            "bind_guard_enforces_f10",
            "loopback_hosts_and_origins_pass_foreign_ones_refused",
        ])),
        c("the MCP daemon admits only the local principal - loopback TCP is host-local, not \
           single-user",
          Evidence::Deferred(
            "M4 remainder - the streamable-http daemon binds 127.0.0.1 with only a Host/Origin \
             rebinding guard, so on a multi-user host any local OS account reaches the full tool \
             surface (writes and sync_push included), while P17 scopes the workspace-scope-less \
             surface to 'stdio, single user'. Repay the way the viewer was repaid: a unix-socket \
             transport, or an auth layer (architecture.md Sections 10/14)",
          )),
        c("a workspace boundary is not crossed by a derived suggestion either",
          Evidence::Scenario(&["p17_candidates_never_span_workspaces_in_the_all_view"])),
        // The fourth enforcement demand, which had no row here at all until the excision spec asked
        // what happens when a secret is already in the log and found the answer was "nothing"
        // (excision.md Section 8). The hook refuses rather than rewrites: P1 forbids transforming an
        // assertion before the log, and rewriting would move the content address (P14).
        c("credential-shaped text is refused at every local ingest door, and the refusal does not \
           repeat it",
          Evidence::Scenario(&[
            "p17_a_credential_is_refused_at_ingest_without_being_echoed",
            "detect_secret_finds_credentials_without_firing_on_prose",
            "a_finding_never_carries_the_secret",
            // The door only governs what arrives after it. Rows that predate it, or landed while it
            // was off, are found by the same detector over the stored log and reported without being
            // quoted - the honest state while there is no way to remove them (excision.md 8.2).
            "p17_the_log_is_scanned_for_secrets_that_predate_the_door",
        ])),
        c("an authenticated network read tier filters workspace enumeration by the reader's grants",
          Evidence::Deferred(
            "M4 Phase 3.5 - retired for now by removing the reachable state (the viewer left TCP \
             for a unix socket), but owed the moment that tier opens (federation.md 6d)",
        )),
    ]),
    (18, "Writes Are an Attack Surface", &[
        c("a claimed tier is the receiver's to evaluate, and no wire claim or agent verdict can \
           mint a human's direct act",
          Evidence::Scenario(&[
            "evaluated_tier_caps_remote_claimed",
            "verdict_ceiling_by_surface_marker",
            "p18_agent_surface_promotion_caps_at_host_signed",
            "p18_an_agent_surface_verdict_cannot_grant_human_confirmed",
            // The stamp-dropping operator paths (re-key, migration) clamp a carried claim to its
            // pre-strip evaluation - `evaluated_tier` trusts a stamp-less claim at face value, so
            // without the clamp one CLI act promotes a synced claim past HostSigned.
            "p18_rekey_and_migration_clamp_a_synced_claim_to_its_evaluation",
        ])),
        c("the reserved surface-marker namespace is refused at every local ingest door",
          Evidence::Scenario(&[
            "p18_reserved_surface_namespace_is_refused_at_every_ingest_door",
            "surface_markers_live_under_the_reserved_prefix",
        ])),
        c("origin is provable and tampering detectable, and a signature is not mistaken for \
           well-formedness",
          Evidence::Scenario(&[
            "signature_roundtrip_verifies_and_tamper_fails",
            "apply_rejects_signed_but_malformed_event",
        ])),
        c("untrusted text never becomes markup in the console that reviews it",
          Evidence::Scenario(&["viz_source_escapes_untrusted_names"])),
        c("derived assertions without lineage are quarantined, recall is trust-weighted, and \
           contamination can be traced back and cleaned",
          Evidence::Deferred("M5 with the extraction port - the tier weights belief today, not the ranked recall surfaces")),
        c("a replicated verdict marker is evaluated against the canon policy, not honored as sent",
          Evidence::Deferred(
            "M4 Phase 5 - the surface ceiling reads the marker off the log, so a console marker that \
             arrives over sync is honored on the receiver. That is deliberate (making it depend on \
             how the verdict arrived would make the effective tier differ per node over one log, a \
             P16 violation) and sound only under the single-principal premise; the principal-to-key \
             binding in the canon policy is what replaces marker trust (resolution.md Section 6, \
             federation.md F13/Phase 5)",
        )),
    ]),
    (19, "Deterministic Core, Probabilistic Edge", &[
        c("a failing probabilistic edge degrades and never blocks a write, and says so rather than \
           degrading silently",
          Evidence::Scenario(&[
            "embed_failure_degrades_without_blocking_ingest",
            "merge_band_reports_whether_it_could_run_and_over_how_much",
        ])),
        // The degrade above is only honest if "no vectors" and "vectors lost" are distinguishable.
        // They were not: both embedding fields carry `#[serde(skip)]` - deliberately, so a vector
        // never rides out through the MCP surface - which means an adapter that persists a row by
        // serializing it accepts every vector and stores none. A third adapter did exactly that, and
        // every semantic read answered the same empty result a vector-less backend gives. The clause
        // exists because the failure is invisible from the outside unless something asks.
        c("a vector the store accepted is a vector the store returns - a dropped recall aid must not \
           be reported as a backend that has none",
          Evidence::Scenario(&[
            "a_stored_vector_survives_the_round_trip",
            "semantic_recall_ranks_by_similarity_and_skips_unembedded",
            "semantic_entity_recall_ranks_by_similarity",
        ])),
    ]),
    (20, "Hexagonal Purity", &[
        c("dependencies point inward only",
          Evidence::Structural(
            "The dependency rule is the crate graph: core names no adapter, so an inward-pointing \
             violation is a Cargo.toml diff rather than a behavior a test could miss. Workspace \
             lints additionally forbid unsafe_code and deny clippy::all.",
        )),
    ]),
    (21, "Narrow, LLM-Legible Surface", &[
        c("a failure tells the caller how to correct itself, since the caller is a model with no \
           human beside it",
          Evidence::Scenario(&["p23_the_gate_surface_refuses_a_malformed_proposal"])),
        c("the surface stays at one tool per recurring intent",
          Evidence::Deferred(
            "incremental - narrowness is a judgment (13 tools) with no executable predicate; the \
             registry records it as unguarded rather than pretending the count is the property",
        )),
        c("long-running work is non-blocking, and mediation asks through elicitation",
          Evidence::Deferred("M4 remainder - MCP Tasks and elicitation are not exposed (architecture.md Section 7)")),
    ]),
    (22, "Knowledge as a By-Product", &[
        c("ordinary work induces capture and recall without a separate curation chore",
          Evidence::Deferred(
            "incremental - the curation console surfaces micro-decisions, but the MCP prompts that \
             would induce observe/search during work do not exist, so there is no behavior to assert",
        )),
    ]),
    (23, "Gate to Canon", &[
        c("a proposal is itself an observation, and its state is a deterministic fold with merge \
           absorbing",
          Evidence::Scenario(&[
            "i16_merge_absorbs_over_conflicting_reject_in_any_order",
            "p23_a_proposal_alone_changes_nothing_only_the_verdict_commits",
        ])),
        c("no merge without a diff: what a verdict would overturn is computed before it is cast",
          Evidence::Scenario(&[
            "p23_an_open_gate_proposal_carries_a_diff_without_moving_the_canon",
            "p23_a_merge_proposal_names_the_references_it_would_rewire",
        ])),
        // The fold is the enforcement point on purpose: a replicated verdict arrives as an
        // observation and never passes through review_proposal, so a gate living there is no gate.
        c("blocking checks are enforced by the fold, and reach the same conclusion on every node",
          Evidence::Scenario(&[
            "p23_a_blocked_merge_verdict_does_not_reach_canon",
            "p23_a_well_formed_merge_passes_its_checks_and_commits",
            "i8_blocking_check_conclusion_is_arrival_order_independent",
        ])),
        // The state fold and the effect folds must give ONE answer to "did this merge commit":
        // a blocked gate merge that still granted tiers to its present targets was exactly the
        // two-fold disagreement this clause exists to forbid.
        c("a merge the fold calls blocked has no commit effect - the grant fold and the state fold agree",
          Evidence::Scenario(&["p23_a_blocked_gate_merge_grants_nothing"])),
        c("the write surface refuses a proposal the fold could never resolve",
          Evidence::Scenario(&[
            "p23_the_gate_surface_refuses_a_malformed_proposal",
            // Same demand on the re-key surface: a proposal event carried into a workspace whose
            // targets cannot exist there would be permanently blocked - so it is not carried.
            "p23_a_rekey_does_not_carry_proposal_events_into_the_new_workspace",
        ])),
        c("a reviewer is shown the informative checks - blast radius and the payload's trust profile",
          Evidence::Deferred(
            "M4+ with the review-economics layer - only the blocking checks are computed, so the \
             routing rules of proposal-workflow.md Section 9 (impact radius decides what needs a \
             human) have no input to read. Recorded as still open in architecture.md Section 14 \
             without a clause here. Impact radius is also the standing example of a check that is \
             NOT monotone in the growing log, so it cannot ship before the fixed base of I7",
        )),
        c("a merged verdict has the commit effect its kind promises",
          Evidence::Scenario(&["p23_demotion_overrides_below_base"])),
        c("every proposal kind the surface accepts has a commit effect",
          Evidence::Deferred(
            "M4 Phase 5 / M5 - tbox_change and recall fold correctly and change nothing, which is \
             assigned rather than accidental (proposal-workflow.md Section 13)",
        )),
        c("a verdict binds to the base it reviewed, and a stale or withdrawn proposal cannot merge",
          Evidence::Deferred(
            "M4 Phase 5 - the fold checks only the blocking gate of 7.1's validity conditions: a \
             proposal never pins its base (I7), Stale is never computed, a verdict is not bound to \
             a base (I12), and a merge verdict cast after a withdrawal still folds to merged (no \
             Open-state check). Recorded in architecture.md Section 14 and the [impl] note in \
             proposal-workflow.md Section 4",
        )),
        c("self-attestation is computed from the proposer and reviewer, and a recall verdict is \
           not delegable",
          Evidence::Characterized(
            &["i9_self_attested_is_blanket_true_until_principal_check_lands"],
            "M4 Phase 5 - the fold hardcodes self_attested: true, which is honest as a solo-mode \
             blanket label but will mislabel reviewed merges the moment there are two principals",
        )),
    ]),
];

/// The principles `docs/principles.md` actually declares, as (number, short name) in document
/// order. A heading reads `### Principle 4. Bitemporal - Two Time Axes (Bi-Temporality)`, and the
/// short name is the trailing parenthetical - the same string the registry carries, so the two
/// cannot drift without saying so.
fn documented_principles() -> Vec<(u8, &'static str)> {
    let mut out = Vec::new();
    for line in PRINCIPLES_DOC.lines() {
        let Some(rest) = line.trim_end().strip_prefix("### Principle ") else {
            continue;
        };
        let Some((num, title)) = rest.split_once('.') else {
            continue;
        };
        let Ok(n) = num.trim().parse::<u8>() else {
            continue;
        };
        let short = title
            .trim()
            .strip_suffix(')')
            .and_then(|t| t.rfind('(').map(|i| &t[i + 1..]))
            .unwrap_or_else(|| {
                panic!("P{n}: the heading must end with the short name in parentheses: {line:?}")
            });
        out.push((n, short));
    }
    out
}

/// Whether a declared scenario name names a test that actually runs. Declared worst-first: one name
/// can match in several sources, and [`declares`] keeps the best match, so a helper sharing a name
/// with a real test does not mask the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Declared {
    /// No `fn <name>(` anywhere in [`SOURCES`] - renamed, deleted, or a typo here.
    NotFound,
    /// The function exists but carries no test attribute, so `cargo test` never calls it. A guard
    /// that does not run is not evidence, and this is the state that used to pass unnoticed.
    NotATest,
    /// A test, but `#[ignore]`d - it compiles and is skipped, which is the same nothing.
    Ignored,
    /// A test that runs under a plain `cargo test`.
    Running,
}

/// Classifies `name` by reading the attribute block directly above its definition. Comment lines
/// are walked through (a doc comment may sit between the attribute and the `fn`); anything else
/// ends the block, so this never reaches up into a neighbouring item's attributes.
fn declares(name: &str) -> Declared {
    let needle = format!("fn {name}(");
    let mut best = Declared::NotFound;
    for src in SOURCES {
        if !src.contains(&needle) {
            continue;
        }
        let lines: Vec<&str> = src.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(&needle) {
                continue;
            }
            let mut attrs: Vec<&str> = Vec::new();
            for prev in lines[..i].iter().rev() {
                let prev = prev.trim();
                if prev.starts_with("#[") {
                    attrs.push(prev);
                } else if !prev.starts_with("//") {
                    break;
                }
            }
            let found = if attrs.iter().any(|a| a.starts_with("#[ignore")) {
                Declared::Ignored
            } else if attrs.iter().any(|a| *a == "#[test]" || a.starts_with("#[tokio::test")) {
                Declared::Running
            } else {
                Declared::NotATest
            };
            best = best.max(found);
        }
    }
    best
}

/// Every principle in the document carries an evidence state, exactly once, under the document's
/// own number and name. A principle cannot be added to `principles.md` and left unchecked here,
/// an existing one cannot be dropped, and a row cannot go on standing for a principle that was
/// renumbered or replaced underneath it.
#[test]
fn every_principle_declares_its_evidence() {
    let documented = documented_principles();
    assert!(
        documented.len() > 1,
        "parsed {} principles out of docs/principles.md - the heading format changed and this \
         registry is no longer reading the document it claims to mirror",
        documented.len()
    );
    for (i, (n, name)) in documented.iter().enumerate() {
        assert_eq!(
            *n,
            i as u8 + 1,
            "docs/principles.md must number its principles contiguously from 1: heading {i} is \
             P{n} ({name})"
        );
    }
    assert_eq!(
        REGISTRY.len(),
        documented.len(),
        "docs/principles.md declares {} principles, the registry has {} rows - a principle was \
         added or removed and has no evidence state",
        documented.len(),
        REGISTRY.len()
    );
    for ((n, name, clauses), (dn, dname)) in REGISTRY.iter().zip(&documented) {
        assert_eq!(
            (n, name),
            (dn, dname),
            "the registry must mirror docs/principles.md: registry says P{n} ({name}), the \
             document says P{dn} ({dname})"
        );
        assert!(
            !clauses.is_empty(),
            "P{n} ({name}) declares no clause - say what it demands before saying how it is checked"
        );
        for cl in *clauses {
            assert!(
                cl.demands.len() > 20,
                "P{n} ({name}): a clause must state the demand it is evidence for, not a label: \
                 {:?}",
                cl.demands
            );
        }
    }
}

/// Every declared scenario test exists AND runs. This is what makes the registry a guard rather
/// than a comment: rename, delete, un-`#[test]` or `#[ignore]` one and the principle it was
/// standing for reports as unguarded here, instead of silently losing its evidence.
#[test]
fn declared_scenarios_exist() {
    let mut broken: Vec<String> = Vec::new();
    for (n, name, clauses) in REGISTRY {
        for cl in *clauses {
            // Characterization tests are held to the same standard as guards: a record of the
            // interim that does not run records nothing.
            let (tests, kind) = match &cl.evidence {
                Evidence::Scenario(t) => (*t, "Scenario"),
                Evidence::Characterized(t, _) => (*t, "Characterized"),
                Evidence::Structural(_) | Evidence::Deferred(_) => continue,
            };
            assert!(
                !tests.is_empty(),
                "P{n} ({name}) files \"{}\" as {kind} but names no test - use Deferred instead",
                cl.demands
            );
            for t in tests {
                let why = match declares(t) {
                    Declared::Running => continue,
                    Declared::NotFound => "no such function (renamed, deleted, or a typo here)",
                    Declared::NotATest => "exists but has no #[test] attribute, so it never runs",
                    Declared::Ignored => "is #[ignore]d, so a plain `cargo test` skips it",
                };
                broken.push(format!("P{n} ({name}) \"{}\" -> {t}: {why}", cl.demands));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "declared scenario tests that are not running evidence:\n  {}",
        broken.join("\n  ")
    );
}

/// A backtick-quoted identifier in a design document, and whether the document excused it from
/// existing. The excuse has to live in the document (a table row whose Status names a milestone),
/// not in an allowlist here - an exemption a reader of the document cannot see is how the claims
/// drifted in the first place.
struct DocClaim {
    doc: &'static str,
    line_no: usize,
    name: &'static str,
    planned: bool,
}

/// Test names claimed by the design documents. A claim is any backtick-quoted lower-snake-case
/// identifier with at least three underscores - measured against every document in [`DESIGN_DOCS`],
/// that shape has no false positives (field names like `origin_host_id` and type names like
/// `HumanConfirmed` fall outside it) and catches all 27 real claims, in prose and tables alike.
fn doc_claims() -> Vec<DocClaim> {
    let mut out = Vec::new();
    for (doc, text) in DESIGN_DOCS {
        let lines: Vec<&str> = text.lines().collect();
        // Prose wraps, so a sentence's milestone often sits on a different line than the name it
        // qualifies. The unit that matches how the document reads is the paragraph: the contiguous
        // run of non-blank lines around this one.
        let paragraph = |i: usize| -> String {
            let start = lines[..i].iter().rposition(|l| l.trim().is_empty()).map_or(0, |p| p + 1);
            let end = lines[i..]
                .iter()
                .position(|l| l.trim().is_empty())
                .map_or(lines.len(), |p| i + p);
            lines[start..end].join(" ")
        };
        for (i, line) in lines.iter().enumerate() {
            // A document may claim a test that does not exist yet, but only by saying where it
            // comes from. In a table that means the Status column specifically - the last non-empty
            // cell - so a milestone mentioned in the Pins prose cannot excuse a `landed` row. In
            // running prose there is no such column, so the paragraph is read instead; that is
            // looser, and it is the price of letting the document write "the M5 test `x` lands
            // there" without contorting the sentence.
            let milestoned = |s: &str| ["M3", "M4", "M5", "M6"].iter().any(|m| s.contains(m));
            let planned = if line.trim_start().starts_with('|') {
                line.rsplit('|').map(str::trim).find(|c| !c.is_empty()).is_some_and(milestoned)
            } else {
                milestoned(&paragraph(i))
            };
            for piece in line.split('`').skip(1).step_by(2) {
                let underscores = piece.bytes().filter(|b| *b == b'_').count();
                let shaped = underscores >= 3
                    && piece.starts_with(|c: char| c.is_ascii_lowercase())
                    && piece
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                if shaped {
                    out.push(DocClaim { doc, line_no: i + 1, name: piece, planned });
                }
            }
        }
    }
    out
}

/// Every test a design document names must be a test that runs. `architecture.md` Section 14 is
/// built out of "guarded by <test>" sentences and the resolution documents carry test-plan tables;
/// until this existed, both were prose that agreed with the tree only while someone remembered to
/// re-read them. The CI job was added for exactly this reason on the Rust side - this is the same
/// guarantee for the claims the documents make.
#[test]
fn design_docs_name_tests_that_run() {
    let claims = doc_claims();
    assert!(
        claims.len() > 20,
        "found only {} test-name claims across the design documents - the scan shape broke and \
         this test is now vacuous",
        claims.len()
    );
    let mut broken: Vec<String> = Vec::new();
    for c in &claims {
        match declares(c.name) {
            Declared::Running => {}
            // A named-but-absent test is legitimate only where the document says it is future work.
            Declared::NotFound if c.planned => {}
            state => broken.push(format!(
                "{}:{} names `{}` - {}",
                c.doc,
                c.line_no,
                c.name,
                match state {
                    Declared::NotFound =>
                        "no such test (renamed or deleted). Fix the name, or put the milestone \
                         that will deliver it in the row's Status column",
                    Declared::NotATest => "exists but has no #[test] attribute, so it never runs",
                    Declared::Ignored => "is #[ignore]d, so a plain `cargo test` skips it",
                    Declared::Running => unreachable!(),
                }
            )),
        }
    }
    assert!(
        broken.is_empty(),
        "design documents claiming guards that do not run:\n  {}",
        broken.join("\n  ")
    );
}

/// A non-scenario state must carry its justification. "Structural" with no stated mechanism, or
/// "Deferred" with no repayment milestone, is an unguarded principle wearing a label - the exact
/// move this registry exists to make impossible.
#[test]
fn structural_and_deferred_states_are_justified() {
    let repayment_named = |why: &str| {
        ["M3", "M4", "M5", "M6", "Revisit", "incremental"]
            .iter()
            .any(|m| why.contains(m))
    };
    for (n, name, clauses) in REGISTRY {
        for cl in *clauses {
            let d = cl.demands;
            match &cl.evidence {
                Evidence::Scenario(_) => {}
                Evidence::Structural(why) => assert!(
                    why.len() > 60,
                    "P{n} ({name}) \"{d}\": Structural needs the mechanism that makes violation \
                     unrepresentable"
                ),
                // An unmet clause owes the same two things whether or not a test pins it: a reason,
                // and the milestone that ends it.
                Evidence::Characterized(_, why) | Evidence::Deferred(why) => {
                    assert!(
                        why.len() > 60,
                        "P{n} ({name}) \"{d}\": an unmet clause needs a reason and a repayment point"
                    );
                    assert!(
                        repayment_named(why),
                        "P{n} ({name}) \"{d}\": an unmet clause must name where it is repaid, so \
                         this file and architecture.md Section 14 cannot disagree about what is owed"
                    );
                }
            }
        }
    }
}

/// The coverage summary, printed with `--nocapture`. Not an assertion: the ratio is a fact about
/// where the project is, and pinning it would only invite someone to edit the number.
///
/// It counts CLAUSES, not principles. Counting principles is what let a principle with one guarded
/// clause and one overdue debt read the same as a principle that is fully met - and "18 of 23" is a
/// far more comfortable sentence than the truth underneath it.
#[test]
fn report_principle_coverage() {
    let (mut guarded, mut structural, mut pinned, mut unguarded) = (0, 0, 0, 0);
    let mut owed: Vec<String> = Vec::new();
    for (n, name, clauses) in REGISTRY {
        println!("P{n:02} {name}");
        for cl in *clauses {
            let mark = match &cl.evidence {
                Evidence::Scenario(t) => {
                    guarded += 1;
                    format!("guard    ({} tests)", t.len())
                }
                Evidence::Structural(_) => {
                    structural += 1;
                    "structural".to_string()
                }
                Evidence::Characterized(t, _) => {
                    pinned += 1;
                    format!("OWED     (pinned by {} test)", t.len())
                }
                Evidence::Deferred(_) => {
                    unguarded += 1;
                    "OWED     (nothing pins it)".to_string()
                }
            };
            println!("       {mark:<28} {}", cl.demands);
            if !cl.evidence.holds() {
                owed.push(format!("P{n:02} {}", cl.demands));
            }
        }
    }
    let total = guarded + structural + pinned + unguarded;
    println!(
        "\n{total} clauses over {} principles: {guarded} guarded / {structural} structural / \
         {pinned} pinned-but-unmet / {unguarded} unmet-and-unpinned",
        REGISTRY.len()
    );
    println!(
        "\nWhat the principles ask for and this system does not do yet ({}):",
        owed.len()
    );
    for o in &owed {
        println!("  {o}");
    }
}
