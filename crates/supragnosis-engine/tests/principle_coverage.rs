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

/// `docs/federation.md`, embedded for the same reason as the principles document: the invariant set
/// is read from the normative text, so an invariant added there is a failure here until it is given
/// an evidence state. This is the coupling the F axis lacked - `every_principle_declares_its_evidence`
/// parses the principles document only, so F21 could be written into the spec with no accounting at
/// all, and F1..F20 had never been mapped to their guards either. Several of them turned out to be
/// guarded by tests nobody had connected to them (F10 by `bind_guard_enforces_f10`), which is the
/// shape of the debt: not unenforced, unmapped.
const FEDERATION_DOC: &str = include_str!("../../../docs/federation.md");

/// One row per `F` invariant of federation.md Section 8, in document order.
///
/// The clause split follows the invariant's own text: where it enumerates demands (F5, F9, F12, F13,
/// F14) they are separate rows, for the reason [`Clause`] gives - a conjunction filed as one verdict
/// reports the met half and hides the other. Where it states a single demand there is one clause.
const FEDERATION_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[c(
        "sync replicates the observation log, never a projection",
        Evidence::Structural(
            "the wire has no representation for a projection: `PullResp` and `PushReq` carry              `Vec<AttestationEvent>` and nothing else, and there is no serializable entity or              relation row in the sync crate, so sending one is not a thing the codec can express",
        ),
    )]),
    (2, &[c(
        "the content-address id excludes sync metadata, so identical content dedups across nodes",
        Evidence::Scenario(&["cross_node_identical_id_dedups_and_unions", "observation_id_includes_assertions"]),
    )]),
    (3, &[c(
        "apply is verify -> CAS dedup/absorb -> advance VV -> re-project, and trust never gates it",
        Evidence::Scenario(&["apply_verifies_rejects_and_stays_idempotent"]),
    )]),
    (4, &[c(
        "provenance is a monotonic enrichment-ordered union: nothing is removed or overwritten",
        Evidence::Scenario(&[
            "absorb_union_is_order_independent_and_idempotent",
            "absorb_stamp_upgrade_supersedes_unstamped_base",
            "reobservation_absorbs_attestations_and_lineage",
        ]),
    )]),
    (5, &[
        c(
            "fold-projections converge continuously with the log, ordered by HLC and not by arrival",
            Evidence::Scenario(&["types_fold_orders_by_hlc_not_observed_at"]),
        ),
        c(
            "materialized projections converge at re-materialization, over any exchange order",
            Evidence::Scenario(&["cross_node_reprojection_converges", "two_nodes_converge_under_any_exchange_order"]),
        ),
    ]),
    (6, &[c(
        "an event with a bad signature, unknown origin key or bad bearer token is never applied",
        Evidence::Scenario(&[
            "apply_verifies_rejects_and_stays_idempotent",
            "signature_roundtrip_verifies_and_tamper_fails",
            "wire_auth_rejects_bad_token_and_unshared_workspace",
        ]),
    )]),
    (7, &[c(
        "origin_seq is monotonic per (origin, workspace) and apply is hole-tolerant and idempotent",
        Evidence::Scenario(&[
            "seq_continues_after_restart",
            "two_nodes_converge_under_any_exchange_order",
            "attestations_since_filters_by_version_vector",
        ]),
    )]),
    (8, &[c(
        "HLC is monotonic and totally ordered, and an observation orders by its earliest attestation",
        Evidence::Scenario(&["hlc_is_monotonic_and_merge_lands_after_both", "ordering_hlc_takes_earliest_and_falls_back_to_legacy"]),
    )]),
    (9, &[
        c(
            "only whitelisted workspaces leave the node, filtered before the boundary",
            Evidence::Scenario(&["export_respects_share_list_and_vv"]),
        ),
        c(
            "the server enforces per-node access, and the remote read surface obeys the same list",
            Evidence::Scenario(&["wire_auth_rejects_bad_token_and_unshared_workspace"]),
        ),
    ]),
    (10, &[c(
        "the sync surface binds non-loopback only with TLS and a non-empty allowlist",
        Evidence::Scenario(&["bind_guard_enforces_f10", "parse_loopback_addr_accepts_loopback_rejects_public"]),
    )]),
    (11, &[c(
        "sync is a non-blocking pollable task that never blocks a tool handler",
        Evidence::Deferred(
            "the sync_* tools ship as ordinary blocking calls - federation.md Section 9 records this              as the P21 remainder, and store work does offload via spawn_blocking, but no test pins              either half. Revisit when a round grows past one small delta exchange",
        ),
    )]),
    (12, &[
        c(
            "a transport failure is reported as a failure, never as an empty result",
            Evidence::Scenario(&["wire_auth_rejects_bad_token_and_unshared_workspace"]),
        ),
        c(
            "a store failure is reported as a failure, never as empty or converged",
            Evidence::Deferred(
                "the port returns Result at every call site and `internal()` maps a store error to                  500, but nothing forbids a future caller substituting a default, and no test pins                  it because there is no fault-injecting adapter. Revisit when one exists",
            ),
        ),
    ]),
    (13, &[
        c(
            "a valid signature proves origin, never that the content is well-formed or true",
            Evidence::Scenario(&["apply_rejects_signed_but_malformed_event"]),
        ),
        c(
            "the effective tier is the receiver's evaluation and never maxes in a remote claim",
            Evidence::Scenario(&["evaluated_tier_caps_remote_claimed"]),
        ),
    ]),
    (14, &[
        c(
            "node_id is derived from the public key and is stable across restarts",
            Evidence::Scenario(&["node_id_derives_from_public_key_and_is_stable"]),
        ),
        c(
            "an empty or default node_id cannot occur: the identity is generated, never configured",
            Evidence::Structural(
                "there is no configuration key for `node_id` - it is derived from a keypair the node generates once, so `localhost` and the empty string are not values the field can take rather than values something checks for",
            ),
        ),
        c(
            "the sync role refuses to start when its own id sits in its own allowlist",
            Evidence::Scenario(&["a_node_that_admits_itself_is_refused"]),
        ),
    ]),
    (15, &[c(
        "a replicated verdict_cast applies only after the I9 and I17 checks, HLC-ordered",
        Evidence::Deferred(
            "the verdict fold is pinned by `proposal_open_verdict_fold`, but the cross-node I9/I17              checks are not written - the fold hardcodes the solo self-attested path. Lands with M4              Phase 5 (governance enforcement), which federation.md Phasing already names",
        ),
    )]),
    (16, &[c(
        "the accept gate is the sole commit path to canon, and a verdict is final once causally stable",
        Evidence::Deferred(
            "the gate exists, the log-borne canon policy and the causal-stability watermark do not,              so policy-in-force at a verdict's HLC cannot be computed yet. Lands with M4 Phase 5 -              federation.md 8a says the same about Prop D's premise set",
        ),
    )]),
    (17, &[c(
        "a governance stakeholder is a principal rather than a host, bound by the canon policy",
        Evidence::Deferred(
            "principal identity across nodes rests on the canon policy's principal-to-key binding,              which is M4 Phase 5 work; until then a deployment stays single-principal under the P23              solo exception and the comparison never has two principals to make",
        ),
    )]),
    (18, &[c(
        "in a multi-principal shared workspace a T-Box change passes the accept gate",
        Evidence::Deferred(
            "define_type is ungated working-set last-write-wins today, which the invariant itself              says is tolerable only under the solo exception - and so federated deployment stays              single-principal until the M4 Phase 5 gate exists",
        ),
    )]),
    (19, &[c(
        "the hub human surface authenticates by enrolled user keys and never accepts an unattributable write",
        Evidence::Deferred(
            "there is no human surface yet - the viewer is a local unix socket with no network bind,              so the clause has nothing to govern. Owed the moment that tier opens, which is M4              Phase 3.5 in federation.md Phasing",
        ),
    )]),
    (20, &[c(
        "a recall verdict and a HumanConfirmed promotion require a principal-signed act",
        Evidence::Deferred(
            "surface markers cap a grant today (`verdict_ceiling_by_surface_marker`), which is the              weaker strength (i) story; client-side user-key signatures over the act bytes are M4              Phase 5 work and nothing pins strength (ii) yet",
        ),
    )]),
    (21, &[c(
        "the negotiated surface is entitlement-scoped, narrowing-only, log-external, three-valued, response-labelled and non-monotonic",
        Evidence::Deferred(
            "6e specifies the surface and `ping` already answers with the caller's grants, but no              caller consumes that answer, so none of the six clauses has anything to hold. Lands              with M4 Phase 7, which federation.md Phasing names",
        ),
    )]),
];

/// The invariant numbers `docs/federation.md` Section 8 declares, in document order.
fn documented_invariants() -> Vec<u8> {
    FEDERATION_DOC
        .lines()
        .filter_map(|l| l.trim_end().strip_prefix("- **F"))
        .filter_map(|rest| rest.split_once("**"))
        .filter_map(|(num, _)| num.parse::<u8>().ok())
        .collect()
}

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
    // Same reason again, with one difference worth naming: this one specifies the surface that
    // three unmet clauses across three principles are all waiting on (P7 demotion, P18
    // trust-weighted recall, P23's effect-less `recall` kind), so the tests it eventually names
    // will be cited from three registry rows rather than one.
    ("docs/consolidation.md", include_str!("../../../docs/consolidation.md")),
    // Listed on the day it was written rather than the day it grows a guard, like the three above.
    // It carries one difference: the invariant it implements, F21, is already a registry row, so the
    // first test it names has somewhere to be cited FROM as well as checked against.
    (
        "docs/negotiated-surface.md",
        include_str!("../../../docs/negotiated-surface.md"),
    ),
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
          Evidence::Scenario(&[
            "p3_a_merged_split_reverses_the_merge_it_names",
            // The reversal is worth nothing if the band immediately asks to undo it, so the
            // suppression guard is part of this clause holding rather than a separate nicety.
            "p19_a_split_pair_is_never_suggested_again",
        ])),
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
          Evidence::Deferred(
            "M6 ([consolidation.md](../../../docs/consolidation.md)) - the generate side landed early \
             with M3.5, and Section 8 step 1 has now landed too: the weight is computed and \
             reported as `demotion_candidates`. The clause stays unmet because nothing \
             consumes it - `fuse_rrf` still fuses by rank position alone (step 2), and a \
             weight that ranks nothing forgets nothing")),
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
        // P15 says a wrong merge is more expensive than a wrong split "though, by Principle 3, both
        // must be reversible". The merge half shipped long ago; this is the other one, and the
        // re-merge case is where content addressing (P14) turned out to bite - unmerge.md Section 7.
        c("a merge and a split are both reversible",
          Evidence::Scenario(&["p15_separated_entities_can_be_merged_again"])),
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
            // The newest fold on the read path, declared here on the day it landed rather than the
            // day someone audits it. Its convergence is what decides which side of P16's two-layer
            // split it may live on: a weight that inherited arrival order could only ever be a
            // node-local recall aid (consolidation.md Section 4).
            "p16_the_recall_weight_is_the_same_on_any_arrival_order",
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
        // Repaid by the auth layer, not by the socket: MCP clients reach the daemon over HTTP, so
        // the viewer's repair (move to a unix socket, let its 0600 mode be the access control) does
        // not transfer. What transfers is the FILE MODE - the token lives at ~/.supragnosis/mcp.token
        // under the same 0600, so the surface is confined to one OS user either way.
        c("the MCP daemon admits only the local principal - loopback TCP is host-local, not \
           single-user",
          Evidence::Scenario(&[
            "only_the_exact_token_is_admitted",
            "digest_equality_separates_digests_that_differ_anywhere",
        ])),
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
          Evidence::Deferred(
            "M5 with the extraction port - the tier weights belief today, not the ranked recall \
             surfaces, because `fuse_rrf` fuses by rank position and takes no per-item term. The \
             recall half is specified with M6's weight ([consolidation.md](../../../docs/consolidation.md) \
             Section 4.3); the quarantine and lineage-cleanup halves stay here")),
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
             assigned rather than accidental (proposal-workflow.md Section 13). What a merged \
             recall must then do is specified in \
             [consolidation.md](../../../docs/consolidation.md) Section 6 - retract and floor, \
             never delete, which is what separates it from excision",
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
    for (n, clauses) in FEDERATION_REGISTRY {
        for cl in *clauses {
            let (tests, kind) = match &cl.evidence {
                Evidence::Scenario(t) => (*t, "Scenario"),
                Evidence::Characterized(t, _) => (*t, "Characterized"),
                Evidence::Structural(_) | Evidence::Deferred(_) => continue,
            };
            assert!(
                !tests.is_empty(),
                "F{n} files \"{}\" as {kind} but names no test - use Deferred instead",
                cl.demands
            );
            for t in tests {
                let why = match declares(t) {
                    Declared::Running => continue,
                    Declared::NotFound => "no such function (renamed, deleted, or a typo here)",
                    Declared::NotATest => "exists but has no #[test] attribute, so it never runs",
                    Declared::Ignored => "is #[ignore]d, so a plain `cargo test` skips it",
                };
                broken.push(format!("F{n} \"{}\" -> {t}: {why}", cl.demands));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "declared scenario tests that are not running evidence:\n  {}",
        broken.join("\n  ")
    );
}

/// The F axis of the same coupling: an invariant written into `docs/federation.md` Section 8 has no
/// evidence state until someone gives it one, and this fails until they do.
///
/// The hole this closes is not hypothetical. F21 was added to the spec in this same branch and
/// nothing anywhere objected, because the only completeness check read the principles document.
#[test]
fn every_invariant_declares_its_evidence() {
    let documented = documented_invariants();
    assert!(
        documented.len() > 10,
        "parsed {} invariants out of docs/federation.md - the list format changed and this registry \
         is no longer reading the document it claims to mirror",
        documented.len()
    );
    for (i, n) in documented.iter().enumerate() {
        assert_eq!(
            *n,
            i as u8 + 1,
            "docs/federation.md must number its invariants contiguously from 1: entry {i} is F{n}"
        );
    }
    assert_eq!(
        FEDERATION_REGISTRY.len(),
        documented.len(),
        "docs/federation.md declares {} invariants, the registry has {} rows - an invariant was \
         added or removed and has no evidence state",
        documented.len(),
        FEDERATION_REGISTRY.len()
    );
    for ((n, clauses), dn) in FEDERATION_REGISTRY.iter().zip(&documented) {
        assert_eq!(n, dn, "the registry must mirror the document: registry F{n}, document F{dn}");
        assert!(!clauses.is_empty(), "F{n} declares no clause");
        for cl in *clauses {
            assert!(
                cl.demands.len() > 20,
                "F{n}: a clause must state the demand it is evidence for, not a label: {:?}",
                cl.demands
            );
        }
    }
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
/// The sentence a name sits in, inside a paragraph already joined from its wrapped lines.
///
/// A boundary is a period followed by whitespace and then a capital, a backtick or a bold marker.
/// That leaves the two shapes this corpus is full of intact: a version (`v0.2.0`) has no whitespace
/// after its internal periods, and `e.g.` is followed by a lowercase word. A sentence ending in a
/// version still splits, because the period before the space is followed by the next capital.
/// Where the rule guesses wrong it splits too eagerly, which narrows the window - the safe
/// direction for an escape hatch.
fn sentence_of<'a>(paragraph: &'a str, name: &str) -> &'a str {
    let bytes = paragraph.as_bytes();
    let mut bounds = vec![0usize];
    for k in 0..bytes.len() {
        if bytes[k] != b'.' {
            continue;
        }
        let mut j = k + 1;
        while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
            j += 1;
        }
        let starts_sentence =
            bytes.get(j).is_some_and(|c| c.is_ascii_uppercase() || *c == b'`' || *c == b'*');
        if j > k + 1 && starts_sentence && paragraph.is_char_boundary(j) {
            bounds.push(j);
        }
    }
    bounds.push(paragraph.len());
    bounds
        .windows(2)
        .map(|w| &paragraph[w[0]..w[1]])
        .find(|s| s.contains(name))
        .unwrap_or(paragraph)
}

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
            // comes from, and the scope of that excuse is the unit that can actually qualify the
            // name. In a table that is the Status column - the last non-empty cell - so a milestone
            // mentioned in the Pins prose cannot excuse a `landed` row. In running prose it is the
            // **sentence** the name sits in, which is what "the M5 test `x` lands there" was always
            // meant to allow. Paragraph scope allowed more than that: any milestone anywhere in the
            // block excused every name in it, and three dead guards deleted by one breaking change
            // sat behind that for 24 days, each in a `Guarded by <test>` sentence of its own under a
            // heading that happened to say M3b.
            let milestoned = |s: &str| ["M3", "M4", "M5", "M6"].iter().any(|m| s.contains(m));
            let is_row = line.trim_start().starts_with('|');
            let row_planned = is_row
                && line.rsplit('|').map(str::trim).find(|c| !c.is_empty()).is_some_and(milestoned);
            let para = if is_row { String::new() } else { paragraph(i) };
            for piece in line.split('`').skip(1).step_by(2) {
                let underscores = piece.bytes().filter(|b| *b == b'_').count();
                let shaped = underscores >= 3
                    && piece.starts_with(|c: char| c.is_ascii_lowercase())
                    && piece
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                if shaped {
                    let planned =
                        if is_row { row_planned } else { milestoned(sentence_of(&para, piece)) };
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
                        "no such test (renamed or deleted). Fix the name, or say where it comes \
                         from: the milestone belongs in the row's Status column, or in the same \
                         sentence as the name when the claim is prose",
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
    for (n, clauses) in FEDERATION_REGISTRY {
        for cl in *clauses {
            let d = cl.demands;
            match &cl.evidence {
                Evidence::Scenario(_) => {}
                Evidence::Structural(why) => assert!(
                    why.len() > 60,
                    "F{n} \"{d}\": Structural needs the mechanism that makes violation unrepresentable"
                ),
                Evidence::Characterized(_, why) | Evidence::Deferred(why) => {
                    assert!(
                        why.len() > 60,
                        "F{n} \"{d}\": an unmet clause needs a reason and a repayment point"
                    );
                    assert!(
                        repayment_named(why),
                        "F{n} \"{d}\": an unmet clause must name where it is repaid"
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

    // The same accounting for the federation invariants. Reported separately rather than summed in,
    // because the two documents answer different questions - principles say what the system must be,
    // invariants say what federation must preserve - and one blended ratio would hide that most of
    // the F debt is a single milestone rather than a scatter.
    let (mut f_guarded, mut f_structural, mut f_pinned, mut f_unguarded) = (0, 0, 0, 0);
    let mut f_owed: Vec<String> = Vec::new();
    println!("\ndocs/federation.md Section 8 - invariants");
    for (n, clauses) in FEDERATION_REGISTRY {
        println!("  F{n}");
        for cl in *clauses {
            let mark = match &cl.evidence {
                Evidence::Scenario(t) => {
                    f_guarded += 1;
                    format!("guard    ({} tests)", t.len())
                }
                Evidence::Structural(_) => {
                    f_structural += 1;
                    "structural".to_string()
                }
                Evidence::Characterized(t, _) => {
                    f_pinned += 1;
                    format!("OWED     (pinned by {} test)", t.len())
                }
                Evidence::Deferred(_) => {
                    f_unguarded += 1;
                    "OWED     (nothing pins it)".to_string()
                }
            };
            println!("       {mark:<28} {}", cl.demands);
            if !cl.evidence.holds() {
                f_owed.push(format!("F{n:02} {}", cl.demands));
            }
        }
    }
    let f_total = f_guarded + f_structural + f_pinned + f_unguarded;
    println!(
        "\n{f_total} clauses over {} invariants: {f_guarded} guarded / {f_structural} structural / \
         {f_pinned} pinned-but-unmet / {f_unguarded} unmet-and-unpinned",
        FEDERATION_REGISTRY.len()
    );
    println!(
        "\nWhat federation promises and this build does not enforce yet ({}):",
        f_owed.len()
    );
    for o in &f_owed {
        println!("  {o}");
    }
    println!(
        "\nWhat the principles ask for and this system does not do yet ({}):",
        owed.len()
    );
    for o in &owed {
        println!("  {o}");
    }
}
