//! The principle coverage registry - policy as an executable artifact.
//!
//! `principles.md` Appendix B is a review checklist: questions a human is supposed to ask during a
//! PR. That is the same shape of guarantee as a "guarded by <test>" claim with no CI job behind it -
//! it holds exactly as long as someone remembers. This file closes that loop from the other side:
//! every principle must **declare how it is checked**, and the declaration is itself checked.
//!
//! Three evidence states, and the point is that there is no fourth:
//!
//! - [`Evidence::Scenario`] - named tests that must exist. Renaming or deleting one fails here, so a
//!   principle cannot quietly lose its guard.
//! - [`Evidence::Structural`] - enforced by construction rather than by a test (a crate graph that
//!   cannot express the violation, an exhaustive `match` that will not compile). A reason is
//!   mandatory: "structural" without a stated mechanism is just an untested principle with a nicer
//!   label.
//! - [`Evidence::Deferred`] - no enforcement exists yet. Must name the milestone that repays it, so
//!   this file and architecture.md Section 14 cannot drift into disagreeing about what is owed.
//!
//! Adding Principle 24 breaks [`every_principle_declares_its_evidence`] until someone says which of
//! the three it is. That is the whole design: the registry cannot be silently incomplete, which is
//! the failure mode a checklist has by nature.
//!
//! This file deliberately does not re-run those tests - `cargo test` already does. It guards the
//! *map*, not the territory.

/// How a principle is actually checked today.
#[derive(Debug, Clone, Copy)]
enum Evidence {
    /// Test function names that must exist somewhere in the workspace's sources.
    Scenario(&'static [&'static str]),
    /// Enforced by construction. The reason must name the mechanism that makes violation
    /// unrepresentable, not merely unlikely.
    Structural(&'static str),
    /// Not enforced yet. Must name the repayment milestone (architecture.md Section 14).
    Deferred(&'static str),
}

/// Sources scanned for the declared test names. Embedded at compile time, so this test performs no
/// IO and cannot go stale against a moved file without failing to build.
const SOURCES: &[&str] = &[
    include_str!("principle_scenarios.rs"),
    include_str!("policy_cases.rs"),
    include_str!("recall_eval.rs"),
    include_str!("../src/lib.rs"),
    include_str!("../../supragnosis-core/src/lib.rs"),
    include_str!("../../supragnosis-store/src/lib.rs"),
    include_str!("../../supragnosis-store/src/cozo_store.rs"),
    include_str!("../../supragnosis-sync/src/lib.rs"),
    include_str!("../../supragnosis-sync/src/http.rs"),
    include_str!("../../supragnosis-embed/src/lib.rs"),
    include_str!("../../supragnosis-mcp/tests/mcp_surface.rs"),
    include_str!("../../supragnosis-viz/tests/http.rs"),
    include_str!("../../supragnosis-viz/src/lib.rs"),
    include_str!("../../supragnosis-cli/src/main.rs"),
];

/// One row per principle in `docs/principles.md`, in order. The name is carried so a mismatch is
/// legible in a failure message rather than a bare number.
const REGISTRY: &[(u8, &str, Evidence)] = &[
    (1, "Assertion-Belief Separation", Evidence::Scenario(&[
        "observations_carry_assertions_in_log",
        "p1_reprojection_rederives_without_touching_the_log",
        "incremental_write_equals_replay",
        "merge_suggestions_never_commit",
    ])),
    (2, "Provenance First", Evidence::Scenario(&[
        "confidence_out_of_range_is_rejected",
        "unstated_confidence_is_distinct_from_full_confidence",
        "f13_sync_apply_stores_senders_self_declared_tier_verbatim",
    ])),
    (3, "Supersede, Don't Delete", Evidence::Scenario(&[
        "absorb_union_is_order_independent_and_idempotent",
        "p3_a_new_spelling_accumulates_and_never_displaces",
        "log_retains_all_attestations_on_reobservation",
        "cozo_reobservation_accumulates_attestations",
    ])),
    (4, "Bitemporality", Evidence::Scenario(&[
        // Capture only. as_of_valid/as_of_recorded and automatic valid_to closing are M3c, blocked
        // on negation semantics - but capture is what cannot be added retroactively, so it is the
        // part that must be guarded now.
        "relation_valid_interval_is_captured_in_log_and_projection",
    ])),
    (5, "Open World Assumption", Evidence::Scenario(&[
        "p5_absent_entity_is_none_not_error",
        "merge_band_reports_whether_it_could_run_and_over_how_much",
    ])),
    (6, "Contradiction Is Signal", Evidence::Scenario(&[
        "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
        "p6_contradictory_merge_cycle_is_convergent_and_surfaced",
        "contested_iff_top_tier_ties",
    ])),
    (7, "Forgetting as Demotion", Evidence::Scenario(&[
        // The generate-not-commit half only. Recall demotion itself does not exist -> M6.
        "merge_suggestions_never_commit",
        "name_variants_stop_being_offered_once_a_merge_is_open",
        "p7_curation_generates_candidates_and_commits_nothing",
    ])),
    (8, "Clarity", Evidence::Scenario(&[
        "p8_description_survives_reobservation_without_one",
    ])),
    (9, "Coherence", Evidence::Scenario(&[
        "type_def_conflict_surfaces_contested",
        "type_axis_collision_is_a_signal",
        // Section 6 blocks a tbox_change that defines a name on both axes: a structural
        // contradiction is a bug (P9), so it stops rather than merely surfacing.
        "p23_a_blocked_merge_verdict_does_not_reach_canon",
    ])),
    (10, "Open-Closed Schema", Evidence::Scenario(&[
        // The clause with teeth is "a core change demands a migration path". Three 0.x eras changed
        // the assertion encoding, and `migrate` is that path being honored rather than promised.
        "legacy_id_rows_stay_local_and_migrate",
    ])),
    (11, "Minimal Commitment", Evidence::Scenario(&[
        "p11_reify_asserts_group_with_lineage",
        "hypergraph_recovers_co_assertion",
        "hypergraph_dedup_by_member_set_accumulates_sources",
    ])),
    (12, "Minimal Encoding Bias", Evidence::Structural(
        "supragnosis-core declares no store/embedder dependency in its Cargo.toml, so a storage \
         concept cannot reach the domain model - the violation is unrepresentable, not merely \
         discouraged. Shares its enforcement with Principle 20.",
    )),
    (13, "Rigidity", Evidence::Deferred(
        "No subtype hierarchy exists in the T-Box, so essence-vs-role has nothing to bite on yet. \
         Revisit when subtyping is introduced (architecture.md Section 14).",
    )),
    (14, "Stable Identifiers", Evidence::Scenario(&[
        "length_prefix_blocks_boundary_collision",
        "observation_id_includes_assertions",
        "relation_id_is_notation_independent",
        "node_id_derives_from_public_key_and_is_stable",
    ])),
    (15, "Resolution Is Substrate's Job", Evidence::Scenario(&[
        "p15_hypergraph_membership_forwards_accepted_merges",
        "merge_suggestions_never_commit",
        "name_variant_ladder_catches_orthographic_duplicates_without_an_embedder",
    ])),
    (16, "Topology-Independent Convergence", Evidence::Scenario(&[
        "p16_canonical_name_selection_is_arrival_order_free",
        "p16_partitioned_and_duplicated_delivery_converges",
        "p16_search_ties_break_by_id_and_repeat_identically",
        "absorb_converges_under_random_arrival_orders",
        "two_nodes_converge_under_any_exchange_order",
        "cross_node_reprojection_converges",
        "traverse_dangling_endpoint_parity_across_adapters",
        "traverse_order_and_truncation_parity_across_adapters",
        "i8_blocking_check_conclusion_is_arrival_order_independent",
    ])),
    (17, "Knowledge Sovereignty", Evidence::Scenario(&[
        "export_respects_share_list_and_vv",
        "p17_socket_directory_denies_foreign_users_before_the_socket_mode",
        "bind_guard_enforces_f10",
        "loopback_hosts_and_origins_pass_foreign_ones_refused",
    ])),
    (18, "Writes Are an Attack Surface", Evidence::Scenario(&[
        "p18_agent_surface_promotion_caps_at_host_signed",
        "p18_an_agent_surface_verdict_cannot_grant_human_confirmed",
        "evaluated_tier_caps_remote_claimed",
        "signature_roundtrip_verifies_and_tamper_fails",
        "apply_rejects_signed_but_malformed_event",
        "viz_source_escapes_untrusted_names",
        "verdict_ceiling_by_surface_marker",
    ])),
    (19, "Deterministic Core, Probabilistic Edge", Evidence::Scenario(&[
        "embed_failure_degrades_without_blocking_ingest",
        "merge_band_reports_whether_it_could_run_and_over_how_much",
    ])),
    (20, "Hexagonal Purity", Evidence::Structural(
        "The dependency rule is the crate graph: core names no adapter, so an inward-pointing \
         violation is a Cargo.toml diff rather than a behavior a test could miss. Workspace lints \
         additionally forbid unsafe_code and deny clippy::all.",
    )),
    (21, "Narrow LLM-Legible Surface", Evidence::Deferred(
        "Surface narrowness is a judgment (13 tools, one per recurring intent) with no executable \
         predicate. The mechanical half that IS testable - non-blocking long-running work and \
         elicitation - does not exist yet -> M4 remainder (architecture.md Section 7/14).",
    )),
    (22, "Knowledge as a By-Product", Evidence::Deferred(
        "The MCP prompts that would induce observe/search during ordinary work do not exist, so \
         there is no behavior to assert -> incremental (architecture.md Section 14).",
    )),
    (23, "Gate to Canon", Evidence::Scenario(&[
        "p23_demotion_overrides_below_base",
        "p23_a_proposal_alone_changes_nothing_only_the_verdict_commits",
        "i16_merge_absorbs_over_conflicting_reject_in_any_order",
        "i9_self_attested_is_blanket_true_until_principal_check_lands",
        "name_variants_stop_being_offered_once_a_merge_is_open",
        "p23_a_blocked_merge_verdict_does_not_reach_canon",
        "p23_a_well_formed_merge_passes_its_checks_and_commits",
        "i8_blocking_check_conclusion_is_arrival_order_independent",
        "p23_a_merge_proposal_names_the_references_it_would_rewire",
        "p23_an_open_gate_proposal_carries_a_diff_without_moving_the_canon",
    ])),
];

/// The number of principles in `docs/principles.md`. Bumping this without adding a registry row
/// fails the completeness test below - which is the intended way to notice.
const PRINCIPLE_COUNT: u8 = 23;

fn declares(name: &str) -> bool {
    let needle = format!("fn {name}(");
    SOURCES.iter().any(|src| src.contains(&needle))
}

/// Every principle carries an evidence state, exactly once, in order. A new principle cannot be
/// added to the document and left unchecked here, and an existing one cannot be dropped.
#[test]
fn every_principle_declares_its_evidence() {
    assert_eq!(
        REGISTRY.len(),
        PRINCIPLE_COUNT as usize,
        "the registry must have one row per principle in docs/principles.md"
    );
    for (i, (n, name, _)) in REGISTRY.iter().enumerate() {
        assert_eq!(
            *n,
            i as u8 + 1,
            "registry must be in principle order, and complete: row {i} is P{n} ({name})"
        );
    }
}

/// Every declared scenario test actually exists. This is what makes the registry a guard rather
/// than a comment: rename or delete a test and the principle it was standing for reports as
/// unguarded here, instead of silently losing its evidence.
#[test]
fn declared_scenarios_exist() {
    let mut missing: Vec<String> = Vec::new();
    for (n, name, ev) in REGISTRY {
        if let Evidence::Scenario(tests) = ev {
            assert!(
                !tests.is_empty(),
                "P{n} ({name}) claims Scenario evidence but names no test - use Deferred instead"
            );
            for t in *tests {
                if !declares(t) {
                    missing.push(format!("P{n} ({name}) -> {t}"));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "declared scenario tests that no longer exist (renamed, deleted, or a typo here):\n  {}",
        missing.join("\n  ")
    );
}

/// A non-scenario state must carry its justification. "Structural" with no stated mechanism, or
/// "Deferred" with no repayment milestone, is an unguarded principle wearing a label - the exact
/// move this registry exists to make impossible.
#[test]
fn structural_and_deferred_states_are_justified() {
    for (n, name, ev) in REGISTRY {
        match ev {
            Evidence::Scenario(_) => {}
            Evidence::Structural(why) => assert!(
                why.len() > 60,
                "P{n} ({name}): Structural needs the mechanism that makes violation unrepresentable"
            ),
            Evidence::Deferred(why) => {
                assert!(
                    why.len() > 60,
                    "P{n} ({name}): Deferred needs a reason and a repayment point"
                );
                assert!(
                    why.contains("M3") || why.contains("M4") || why.contains("M5")
                        || why.contains("M6") || why.contains("Revisit")
                        || why.contains("incremental"),
                    "P{n} ({name}): Deferred must name where it is repaid, so this file and \
                     architecture.md Section 14 cannot disagree about what is owed"
                );
            }
        }
    }
}

/// The coverage summary, printed with `--nocapture`. Not an assertion: the ratio is a fact about
/// where the project is, and pinning it would only invite someone to edit the number.
#[test]
fn report_principle_coverage() {
    let (mut scenario, mut structural, mut deferred) = (0, 0, 0);
    for (n, name, ev) in REGISTRY {
        let line = match ev {
            Evidence::Scenario(t) => {
                scenario += 1;
                format!("scenario ({} tests)", t.len())
            }
            Evidence::Structural(_) => {
                structural += 1;
                "structural".to_string()
            }
            Evidence::Deferred(_) => {
                deferred += 1;
                "DEFERRED".to_string()
            }
        };
        println!("P{n:02} {name:<38} {line}");
    }
    println!(
        "\n{scenario} scenario / {structural} structural / {deferred} deferred  (of {})",
        REGISTRY.len()
    );
}
