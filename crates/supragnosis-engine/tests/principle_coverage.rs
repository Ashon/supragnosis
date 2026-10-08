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
//! - [`Evidence::Deferred`] - the clause is not GUARANTEED: either it does not hold, or it holds
//!   only incidentally and nothing checks it. Must name the milestone
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
//!   broken promise to whoever reads the documents. And every invariant table those documents
//!   declare is a [`Family`] here, each row with an evidence state, found by shape rather than by
//!   a list someone keeps: [`design_docs_declare_every_invariant`] reads the prefixes off the rows,
//!   and [`every_invariant_declares_its_evidence`] holds each family to the document's numbering.
//!   The coupling once existed for federation alone, after F21 was written with no accounting;
//!   ten other documents had kept invariant tables that nothing read.
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
    /// **Not guaranteed.** Either the clause does not hold, or it holds only incidentally - no code
    /// violates it today - and nothing checks either way. Both are the same debt here, which is why
    /// there is still no fifth state: an unchecked guarantee is a guarantee on paper, and
    /// [`Evidence::holds`] answers what is assured rather than what happens to be true this week.
    /// Must name the repayment milestone (architecture.md Section 14).
    Deferred(&'static str),
}

impl Evidence {
    /// Whether the clause is **assured** - held by a running test or by construction.
    ///
    /// Not the same as "true today", and the difference is deliberate. A `Deferred` clause may
    /// happen to hold because nothing violates it yet; it is still reported as owed, because what
    /// the registry answers is what the system guarantees rather than what it currently gets away
    /// with. The coverage report exists to keep that distinction visible, since it is the one a
    /// reader is most likely to collapse.
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
            // The stamp upgrade only removes an unstamped base. Two stamps of one act would both
            // survive it, so one backfill at a time per node is what keeps an act counted once
            // (sync-correctness.md Section 4).
            "two_backfills_at_once_stamp_each_attestation_once",
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
            "an_event_this_release_cannot_decode_is_rejected_alone",
            // A signature in another spelling of the same bytes is a bad signature, not a second
            // attestation (sync-correctness.md Section 7).
            "a_signature_verifies_in_one_spelling_only",
        ]),
    )]),
    (7, &[c(
        "origin_seq is monotonic per (origin, workspace) and apply is hole-tolerant and idempotent",
        Evidence::Scenario(&[
            "seq_continues_after_restart",
            "two_nodes_converge_under_any_exchange_order",
            "attestations_since_filters_by_version_vector",
            // sync-correctness.md Sections 5-6: a seq is issued once per identity, and a rejection
            // holds its stream rather than leaving a hole a later seq skips past for good.
            "a_restored_store_does_not_reissue_a_seq",
            "what_a_peer_holds_of_this_nodes_stream_floors_its_counter",
            "a_rejected_event_holds_its_stream_until_it_is_accepted",
            // Across batches and pages too, and a held stream does not starve the others (Section 11).
            "a_large_surplus_is_pushed_in_batches_and_pulled_in_pages",
            "a_held_stream_does_not_starve_the_others_across_pages",
        ]),
    )]),
    (8, &[c(
        "HLC is monotonic and totally ordered, and an observation orders by its earliest attestation",
        Evidence::Scenario(&[
            "hlc_is_monotonic_and_merge_lands_after_both",
            "ordering_hlc_takes_earliest_and_falls_back_to_legacy",
            // An observation's HLC is its authoring time whenever it is stamped (Section 3).
            "a_stamp_carries_the_authoring_time_not_the_export_time",
        ]),
    )]),
    (9, &[
        c(
            "only whitelisted workspaces leave the node, filtered before the boundary",
            Evidence::Scenario(&[
                "export_respects_share_list_and_vv",
                // A query is knowledge leaving too (sync-correctness.md Section 9).
                "a_remote_search_does_not_leave_for_an_unshared_workspace",
            ]),
        ),
        c(
            "the server enforces per-node access, and the remote read surface obeys the same list",
            Evidence::Scenario(&[
                "wire_auth_rejects_bad_token_and_unshared_workspace",
                // Per event as well as per request: a peer cannot push for an origin the hub does
                // not grant the workspace (sync-correctness.md Section 8).
                "a_peer_cannot_push_for_an_origin_not_granted_the_workspace",
            ]),
        ),
    ]),
    (10, &[c(
        "the sync surface binds non-loopback only with TLS and a non-empty allowlist",
        Evidence::Scenario(&[
            "bind_guard_enforces_f10",
            "parse_loopback_addr_accepts_loopback_rejects_public",
            // The "TLS enabled" half: the refusal above says nothing about whether the listener
            // that does start actually speaks TLS.
            "tls_listener_serves_https_and_refuses_plaintext",
        ]),
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
            "an allowlist entry naming this node is dropped, and the workaround is reported",
            Evidence::Scenario(&[
                "a_node_is_never_its_own_peer_through_either_path",
                "a_node_that_admits_itself_is_reported_and_ignored",
            ]),
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
    (21, &[
        c(
            "ping reports the authenticated caller's own grants, never the host's inventory",
            Evidence::Scenario(&["ping_answers_with_the_callers_own_grants_and_not_the_hosts_inventory"]),
        ),
        c(
            "a peer's answer may reduce what this node asks of it and never extend the share list",
            Evidence::Scenario(&["routing_narrows_on_a_refusal_and_never_on_ignorance"]),
        ),
        c(
            "the negotiated answer is never written to the observation log",
            Evidence::Scenario(&["routing_on_the_negotiated_map_records_nothing"]),
        ),
        c(
            "an unreachable host yields unknown, never an empty grant set",
            Evidence::Scenario(&["a_failed_check_records_unknown_and_not_an_empty_grant"]),
        ),
        c(
            "a response the map narrowed names the hosts it skipped",
            Evidence::Scenario(&["a_narrowed_round_names_the_hosts_it_skipped"]),
        ),
        c(
            "an answer carries the time it arrived, and only an answer does",
            Evidence::Scenario(&["a_failed_check_records_unknown_and_not_an_empty_grant"]),
        ),
        c(
            "nothing durable is stacked on the map",
            Evidence::Deferred(
                "the half that can be checked - a reader can always date what it acts on - now is. This half is a claim about every future consumer, and the only consumer today re-derives per round rather than caching, so there is nothing yet for a case to catch doing otherwise. Revisit when something wants to keep a routing decision",
            ),
        ),
    ]),
];

// docs/inspector.md Section 9 - the inspector panel (D rows).
const INSPECTOR_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[c(
        "a contested field is never silenced or folded: the contested block decides first, never consults the baseline, and renders outside any condition",
        // crates/supragnosis-viz/tests/http.rs: a source tripwire on contestedBlock and renderDetail.
        Evidence::Scenario(&["inspector_never_folds_a_contested_belief"]),
    )]),
    (2, &[c(
        "everything the rule silences or folds is one act away on the inspector itself, never on another surface",
        Evidence::Deferred(
            "client-only (viewer.js renderDetail: the scope line, the +N fold, the card link) and the viewer's only \
             guards are source tripwires on escaping and on D1. Revisit when the viewer grows a DOM-level harness or a \
             D1-style tripwire for the fold and the card link",
        ),
    )]),
    (3, &[c(
        "a silenced field is stated once for the scope on the inspector, so silence never reads as absence",
        Evidence::Deferred(
            "built in viewer.js (`scopeSaid` and the .scopeline span of renderDetail) but nothing reads it. Revisit \
             when the D1 tripwire is extended to pin that the line is emitted whenever a baseline value is non-null",
        ),
    )]),
    (4, &[
        c(
            "the tier the viewer compares is the receiver-evaluated effective tier, never a peer's claimed tier",
            // principle_scenarios.rs: a remote human_confirmed claim reaches graph() as host_signed.
            Evidence::Scenario(&["f13_read_path_evaluates_remote_claim_at_host_signed"]),
        ),
        c(
            "the baseline is computed over the whole loaded /api/graph response, never the legend-filtered subset",
            Evidence::Deferred(
                "viewer.js scopeBaseline iterates `nodes` and not the typeOff-filtered set - a comment and a loop, \
                 not a guard. Revisit when a source tripwire pins that scopeBaseline reads `nodes` and never `typeOff`",
            ),
        ),
    ]),
    (5, &[
        c(
            "which observations touch a node is decided by the server by canonical id, the same set the projection uses",
            // viz http.rs: the entity filter narrows by graph node id; principle_scenarios.rs: explain's
            // supporting set equals the filtered log.
            Evidence::Scenario(&[
                "viz_serves_observation_log_and_explain",
                "explain_matches_projection_and_surfaces_competitors",
            ]),
        ),
        c(
            "which asserted relations involve the node comes from canonical endpoint ids on RelationRef, never from matching spellings",
            Evidence::Deferred(
                "Section 8 step 3 is unbuilt: `RelationRef` still carries only from/type/to spellings, so the viewer \
                 has nothing to match on but names. Revisit when step 3 adds the two endpoint ids, with a test that a \
                 merged-away spelling resolves",
            ),
        ),
    ]),
    (6, &[
        c(
            "the MCP tool surface is a pinned contract, so a rendering change cannot touch a tool unnoticed",
            // mcp_surface.rs: tools/list must equal tests/fixtures/tools.json.
            Evidence::Scenario(&["the_tool_list_is_the_pinned_contract"]),
        ),
        c(
            "no field of /api/graph, /api/explain or /api/observations is removed or narrowed",
            Evidence::Deferred(
                "the viewer API has no pinned example document the way the CLI's --json answers do (compatibility.md \
                 Section 6); viz_serves_observation_log_and_explain asserts a handful of fields are present, not that \
                 none left. Revisit when the viewer's responses get a fixture pin",
            ),
        ),
    ]),
    (7, &[
        c(
            "esc() escapes <, &, > and both quotes, and the html tag sends every non-markup interpolation through it",
            Evidence::Scenario(&["viz_source_escapes_untrusted_names"]),
        ),
        c(
            "every HTML sink takes an html tagged template, with no no-unsanitized disable comment",
            Evidence::Deferred(
                "held by ESLint no-unsanitized (crates/supragnosis-viz/assets/eslint.config.js, run by \
                 frontend-lint.yml), which this registry cannot name - it scans Rust sources for test fns. Revisit \
                 when a Rust scan of viewer.js for sinks outside an html`...` and for eslint-disable can stand beside \
                 the lint",
            ),
        ),
    ]),
];

// docs/client-connect.md Section 8 - `connect` and the bridge (C rows).
const CLIENT_CONNECT_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[c(
        "the bridge never opens the store and never starts a daemon",
        Evidence::Structural(
            "bridge.rs's non-test code depends on reqwest, tokio, serde_json and the sync crate's TLS helper only - \
             no supragnosis_store or supragnosis_engine item is in scope and nothing spawns a process - and \
             `bridge::Config` carries a URL, a token reader, a wait, a CA bundle and two message strings, so \
             `bridge_cmd` has no store path or binary to hand it. The store crate appears in bridge.rs only under \
             #[cfg(test)], as the daemon the relay test stands up",
        ),
    )]),
    (2, &[
        c(
            "connect writes no secret into a client's configuration: a registration names the bridge and carries no token",
            // connect.rs: no argv element contains Bearer/token; a replaced http entry's token copy is gone.
            Evidence::Scenario(&[
                "registrations_run_the_bridge_and_carry_no_secret",
                "a_missing_section_or_file_is_created_and_an_existing_entry_replaced",
            ]),
        ),
        c(
            "the token file is created 0600 and never exists at another mode",
            Evidence::Scenario(&["the_state_directory_and_its_secrets_are_closed_to_other_accounts"]),
        ),
        c(
            "the bridge reads the token from its file on every request and keeps no copy",
            Evidence::Structural(
                "`Config.token` is `Arc<dyn Fn() -> Option<String>>`, invoked inside `Bridge::post` for each request, \
                 and neither `Bridge` nor `Session` has a field a token could be kept in; `bridge_cmd` passes \
                 `|| read_secret(&mcp_token_path())`, so a regenerated file is read on the next request",
            ),
        ),
    ]),
    (3, &[
        c(
            "a file edit adds or removes one member and leaves every other byte of the client's file as written",
            Evidence::Scenario(&["an_edit_changes_one_member_and_nothing_else"]),
        ),
        c(
            "a client file that does not parse as JSON is refused, never rewritten",
            Evidence::Scenario(&["a_file_that_does_not_parse_is_refused"]),
        ),
        c(
            "the copy taken aside before an edit never overwrites an earlier copy",
            Evidence::Scenario(&["a_backup_never_replaces_an_earlier_one"]),
        ),
        c(
            "an entry connect did not write is left alone without --replace, and the copy is taken before the file is written",
            Evidence::Deferred(
                "both decisions live inline in `connect_cmd` beside the IO, and the only check of them was the manual \
                 run Section 10 records. Revisit when connect_cmd's decision is lifted into a pure function like \
                 lifecycle::plan_install, which a table test can then drive",
            ),
        ),
    ]),
    (4, &[
        c(
            "through the bridge a client sees the daemon's tool list, and a tool call lands in the daemon's store",
            Evidence::Scenario(&["the_bridge_relays_the_daemons_surface_unchanged"]),
        ),
        c(
            "a daemon restart is invisible: the client's own handshake is replayed and its request answered",
            Evidence::Scenario(&["a_daemon_restart_is_invisible_through_the_bridge"]),
        ),
        c(
            "the daemon's own errors reach the client as the daemon wrote them",
            Evidence::Deferred(
                "only success paths are relayed under test; a JSON-RPC error inside a 200 passes through `one_line` \
                 untouched, but an HTTP-level refusal is re-authored by `describe` as a -32000 error, and no case \
                 sends a request the daemon refuses. Revisit when a case calls an unknown tool through the bridge and \
                 compares the two error bodies",
            ),
        ),
    ]),
    (5, &[c(
        "connect without a client argument only reads",
        Evidence::Deferred(
            "`connect_list` reaches `Client::installed`/`entry`, which stat and read files, and the writers \
             (`run_client_cli`, `write_replacing`) are reached only after a client id parses - but nothing asserts \
             the listing leaves HOME untouched; connect_json_matches_its_example runs it under a nonexistent HOME \
             without checking. Revisit when that test asserts the temp HOME is still absent afterwards",
        ),
    )]),
];

// docs/daemon-lifecycle.md Section 9 - the daemon's managers (L rows).
const DAEMON_LIFECYCLE_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c(
            "the situation counts managers, not processes: two are a conflict, one is acted on, none is stopped",
            Evidence::Scenario(&["classification_counts_managers_not_processes"]),
        ),
        c(
            "install refuses while another manager is loaded unless told to take over, which retires it by name",
            Evidence::Scenario(&["install_refuses_a_holder_it_cannot_name"]),
        ),
        c(
            "a holder nothing names - answering, or holding the store with no port - is refused with or without take-over",
            Evidence::Scenario(&[
                "install_refuses_a_holder_it_cannot_name",
                "a_store_held_by_no_manager_is_unrecognized",
                "redb_in_use_sees_a_writer_and_only_a_writer",
            ]),
        ),
        c(
            "a conflict refusal names every manager, which one serves, and the command that resolves it",
            Evidence::Scenario(&["a_conflict_names_every_manager_and_the_fix"]),
        ),
        c(
            "restart and stop act on nothing in a conflict, and install re-checks that the address and store went quiet after retiring the others",
            Evidence::Deferred(
                "the refusals are bail! arms of stop()/restart() and the re-check is `wait_until_released`, all \
                 beside launchctl and socket IO no test drives. Revisit when the act-or-refuse step is lifted into \
                 lifecycle.rs beside classify",
            ),
        ),
    ]),
    (2, &[
        c(
            "a loaded job under any known label is a manager, the Homebrew job included",
            Evidence::Scenario(&["classification_counts_managers_not_processes"]),
        ),
        c(
            "the retired and Homebrew labels stay in the recognized set",
            Evidence::Deferred(
                "KNOWN_LABELS carries com.ashon.supragnosis and four Homebrew spellings, and \
                 every_known_label_is_distinct_and_one_is_canonical checks only distinctness and the single canonical \
                 entry; the retired entries could be deleted without a test noticing. Revisit when that test pins \
                 them by name",
            ),
        ),
    ]),
    (3, &[
        c(
            "status reports running and installed versions when they differ, and an unanswering daemon's version is unknown, not assumed",
            Evidence::Scenario(&["drift_never_assumes_the_running_version"]),
        ),
        c(
            "the tray line and the settings page say the drift outright and point at the repair",
            Evidence::Scenario(&[
                "drift_and_conflict_are_said_outright",
                "the_examples_read_through_the_apps_own_readers",
            ]),
        ),
    ]),
    (4, &[
        c(
            "every CLI lifecycle command exits non-zero on failure with the reason",
            Evidence::Deferred(
                "every lifecycle fn returns anyhow::Result and refuses with bail!, which main() propagates as exit 1, \
                 but no test runs the binary: a_conflict_names_every_manager_and_the_fix pins the reason's text, not \
                 the exit. Revisit when a CLI harness runs stop/restart against a scripted launchctl",
            ),
        ),
        c(
            "a job install loaded that does not come up is a non-zero exit naming the error log, and a slow start is reported as slow",
            Evidence::Deferred(
                "the rule - three seconds with no process and a non-zero last exit - sits in `await_daemon` between \
                 sleeps and launchctl calls, and Section 11 records it was not exercised against a failing job. \
                 Revisit when the decision is lifted into lifecycle.rs as a function over a sequence of launchctl \
                 observations",
            ),
        ),
        c(
            "the app never discards the CLI's exit status: a refused restart or install says why",
            Evidence::Deferred(
                "restart_daemon and set_login route the status through cli_outcome and no test calls either. Revisit \
                 when cli_outcome is pinned on a failing Output and the two commands' messages on a refused CLI are \
                 asserted",
            ),
        ),
    ]),
    (5, &[
        c(
            "install never overwrites a plist it did not generate: a hand-written one is refused without --take-over",
            Evidence::Scenario(&[
                "install_refuses_a_holder_it_cannot_name",
                "the_generated_plist_is_marked_escaped_and_carries_env_verbatim",
            ]),
        ),
        c(
            "nothing deletes an operator's plist: take-over and uninstall move it aside under ~/.supragnosis/launchd",
            Evidence::Deferred(
                "the destination is pinned (env_args_configure_the_daemon_and_nothing_else) but the act is \
                 `move_aside`, a rename reached from service_install and service_uninstall, and nothing asserts a \
                 rename rather than a remove. Revisit when the service commands' file steps run against a temp HOME \
                 in a test",
            ),
        ),
    ]),
    (6, &[
        c(
            "quitting the app never stops a launchd-managed daemon",
            Evidence::Deferred(
                "RunEvent::Exit reaps only the Child inside Daemon::Spawned and Daemon::External holds no handle, but \
                 the shell could still run `supragnosis stop` and nothing checks that it does not. Revisit when the \
                 exit path is a pure function from Daemon to the action taken, testable without a Tauri runtime",
            ),
        ),
        c(
            "turning Start at Login off is the only way the app stops a daemon it did not spawn",
            Evidence::Deferred(
                "set_login runs `service uninstall`, which boots the canonical job out; that this is the single such \
                 path is a reading of main.rs, not a test. Revisit together with the clause above",
            ),
        ),
    ]),
    (7, &[
        c(
            "install refuses an environment, given or carried, that turns off auth or the secret scan or names a non-loopback address",
            Evidence::Scenario(&["the_generated_job_refuses_an_environment_that_adds_exposure"]),
        ),
        c(
            "--env adds SUPRAGNOSIS_* keys and nothing else, and the generated job runs `serve` with its environment carried verbatim",
            Evidence::Scenario(&[
                "env_args_configure_the_daemon_and_nothing_else",
                "the_generated_plist_is_marked_escaped_and_carries_env_verbatim",
            ]),
        ),
    ]),
    (8, &[c(
        "the job is a user LaunchAgent in the gui/<uid> domain: no administrator rights, no helper tool",
        Evidence::Deferred(
            "the plist path is ~/Library/LaunchAgents and every launchctl target is gui/{uid}, with no sudo, \
             SMJobBless or helper anywhere - a reading of the source, not a guard. Revisit when a source scan pins \
             the launchctl targets, or when a Linux manager joins and the domain stops being one constant",
        ),
    )]),
    (9, &[
        c(
            "a pid counts as a manager only when its executable is supragnosis - not the desktop shell, not a reused pid",
            Evidence::Scenario(&["a_pid_counts_only_when_it_is_supragnosis"]),
        ),
        c(
            "a pidfile naming anything else is stale and is cleared, never signalled",
            Evidence::Deferred(
                "`live_pidfile` filters by pid_is_supragnosis and `clear_stale_pidfile` removes what it rejects, \
                 called by stop and restart before they classify; both shell out to kill and ps and no test drives \
                 them. Revisit when the pidfile step is given an injectable process reader",
            ),
        ),
    ]),
];

// docs/settings-page.md Section 7 - the app's settings page (S rows).
const SETTINGS_PAGE_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c(
            "the caller check admits only the main window showing the app's own settings page at the top level: the viewer, a hub's page, another app page and a lookalike path are refused",
            Evidence::Scenario(&["only_the_apps_own_page_may_change_a_setting"]),
        ),
        c(
            "no settings command is open by default: build.rs declares every one, the settings capability grants them to the main window only, and the viewer's capability holds none",
            Evidence::Scenario(&["settings_commands_are_closed_until_granted"]),
        ),
        c(
            "every registered command runs the caller check before it acts",
            Evidence::Deferred(
                "each of the eight #[tauri::command] fns opens with `settings_caller(&webview)?` and the lists are \
                 held together, but nothing scans the commands for the call - a ninth command could skip it and both \
                 tests above would still pass. Revisit when settings_commands_are_closed_until_granted also reads \
                 main.rs for the check as each command's first statement",
            ),
        ),
    ]),
    (2, &[
        c(
            "the credential is never an argument of `server add`: the argument list is built without it",
            Evidence::Scenario(&["a_credential_never_becomes_an_argument"]),
        ),
        c(
            "the credential reaches the CLI on stdin only, appears in no log line or response, and leaves the page once handed over",
            Evidence::Deferred(
                "add_server pipes it to stdin and answers `added <name>` or the CLI's first stderr line, and \
                 settings.js clears the field on submit - neither is under test, and the CLI's stderr is not pinned \
                 to exclude the value. Revisit when add_server is driven with a stub CLI that echoes its stdin",
            ),
        ),
    ]),
    (3, &[
        c(
            "what the page shows of an app, the daemon and the servers is read from the CLI's --json answers, held to the CLI's own example documents",
            Evidence::Scenario(&[
                "an_app_item_says_what_a_click_will_do",
                "every_field_the_app_reads_is_in_the_clis_examples",
                "the_examples_read_through_the_apps_own_readers",
            ]),
        ),
        c(
            "every change is a CLI call, and a refusal from the CLI is what the page shows",
            Evidence::Deferred(
                "each command body is a run_cli call whose outcome is cli_outcome's line and none is under test; the \
                 Add-server sheet also checks the URL's shape before the CLI does, which the document admits. Revisit \
                 when the commands run against a stub CLI and the shown message is asserted for a refusal",
            ),
        ),
    ]),
    (4, &[c(
        "every setting the tray offered - Start at Login, Restart Daemon, Server, AI Apps - is on the page",
        Evidence::Deferred(
            "the four controls became login_set, daemon_restart, server_use/add/remove and app_toggle, all in \
             SETTINGS_COMMANDS, and the tray now builds only status, Open Graph, Settings... and Quit - but no test \
             reads settings.js for the rows that call them. Revisit when the page's script gets a source tripwire \
             like the viewer's, one per command name",
        ),
    )]),
    (5, &[c(
        "the page renders the text it is given as text: no markup sink in settings.js or shell-init.js, a policy in the page's head, no inline script",
        Evidence::Scenario(&["the_settings_page_never_renders_markup"]),
    )]),
    (6, &[c(
        "while a remote profile is active the daemon controls are disabled with the reason, and the commands behind them refuse",
        Evidence::Deferred(
            "login_set, daemon_restart and peer_narrow refuse with the reason when active_remote is set and \
             settings.js disables the switch and hides Restart on d.remote; neither half is under test. Revisit when \
             active_remote is injectable and the three refusals are asserted",
        ),
    )]),
];

// docs/consolidation.md Section 9 - the recall weight and the recall effect (C rows).
const CONSOLIDATION_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("the recall weight is recomputed from the log on every read, and a consolidation pass \
           writes no score of its own",
          // policy_cases.rs: the pass that computes the weight changes nothing in the store;
          // principle_scenarios.rs: two stores fed one log compute one weight.
          Evidence::Scenario(&[
            "p7_curation_generates_candidates_and_commits_nothing",
            "p16_the_recall_weight_is_the_same_on_any_arrival_order",
          ])),
        c("if the weight is materialized it is carried at `reproject` the way the belief is, never \
           as a per-observation column",
          Evidence::Deferred(
            "M6 step 2 (consolidation.md Section 8) - the span is two scalars with nowhere to live \
             yet: a port method for a number no observation asserts, or an engine cache that the \
             un-advanced `log_epoch` after `sync_pull` would serve stale. Nothing is materialized, so \
             nothing can be checked for the shape it takes")),
    ]),
    (2, &[
        c("the weight consumes no arrival order and no randomness",
          Evidence::Scenario(&["p16_the_recall_weight_is_the_same_on_any_arrival_order"])),
        c("recency is a position in the span of the workspace's own recorded HLCs, never a reading \
           of the OS clock",
          // principle_scenarios.rs: the oldest recorded instant is 0.0 and the newest is the
          // frontier at exactly 1.0; against the OS clock every fixture row would sit near 0.0.
          Evidence::Scenario(&["p7_observations_written_at_one_instant_share_one_frontier_position"])),
    ]),
    (3, &[
        c("the committed weight consumes no node-local signal",
          Evidence::Structural(
            "`recall_weights(log, gates)` takes the observation log and the gate-grant fold and \
             nothing else, and no port method records an access or a query - there is no usage \
             telemetry anywhere in the system for a weight to consume")),
        c("usage may re-rank only the already-exempt surfaces, layered after the converged ordering \
           and labelled by `mode`",
          Evidence::Deferred(
            "M6 step 4 (consolidation.md Section 8) - no usage tracking and no re-rank layer exist; \
             `search` labels its `mode` today (hybrid_search_adds_semantic_recall), which is the \
             label the re-rank will reuse, but there is no re-rank for a case to catch leaking into \
             the keyword ordering")),
    ]),
    (4, &[
        c("the weight has a positive floor, so demotion can never reach zero",
          Evidence::Scenario(&["p7_the_recall_weight_never_reaches_zero"])),
        c("`get_entity`, `get_observation` and `traverse` do not consult the weight - demoted \
           knowledge stays reachable by explicit query",
          Evidence::Deferred(
            "M6 step 3 - nothing consumes the weight yet (`recall_weights` has one call site, inside \
             `curation`, and the report is its only output), so the explicit-query surfaces ignore \
             it by default rather than by a guard. The owed case - a floored row still answers the \
             three lookups unchanged - has nothing to assert until ranking consumes the weight")),
    ]),
    (5, &[
        c("demotion appends no observation and changes no belief",
          Evidence::Scenario(&[
            "p7_curation_generates_candidates_and_commits_nothing",
            "merge_suggestions_never_commit",
          ])),
        c("the one consolidation act that appends - a `recall` - is a proposal kind and goes \
           through the gate",
          // engine lib.rs unit test: `recall` opens as a proposal and folds like every other kind.
          Evidence::Scenario(&["proposal_open_verdict_fold"])),
    ]),
    (6, &[
        c("a merged `recall` marks its target and closure retracted, drops them from belief \
           selection and floors their weight",
          Evidence::Deferred(
            "M6 step 5 (consolidation.md Section 6), which the P23 row files under M4 Phase 5 / M5: \
             `recall` folds correctly and changes nothing, and no case merges one, so the gap is \
             neither enforced nor pinned - the gate reports a decision it did not carry out")),
        c("a merged `recall` deletes nothing - the observations stay readable",
          Evidence::Structural(
            "the store port has no delete: `AssertionStore` appends through `add_observation` (an \
             absorb) and reads, `KnowledgeStore` adds `put_entity`/`add_relation` and the \
             owed-projection ledger, so a verdict effect has no method through which to remove a row")),
        c("a retraction is itself an observation, so it converges and is reversible by a new proposal",
          Evidence::Deferred(
            "M6 step 5 - the retraction status is specified as a fold-derived mark over verdict \
             observations; until the effect exists there is no reversal to exercise. It is the P3 \
             shape unmerge.md S7/S8 already pin for a split")),
    ]),
    (7, &[
        c("a recall proposal presents its `derived_from` closure in the belief diff and never \
           cascades it silently",
          Evidence::Deferred(
            "M6 step 5 - a recall's diff reports `no commit effect yet`, the branch \
             p5_a_diff_for_an_unenforced_kind_reports_uncomputable_not_empty pins for tbox_change \
             only; the closure walk is M5's lineage machinery (excision.md Section 8), so nothing \
             presents it and nothing can cascade it")),
        c("a recall verdict binds to the base its diff was computed over",
          Evidence::Deferred(
            "M4 Phase 5 - the Stale-base debt the P23 row carries: a proposal never pins its base \
             (I7) and a verdict is not bound to one (I12), and consolidation.md Section 6 names \
             `recall` as the kind where that starts to bite")),
    ]),
    (8, &[
        c("retraction is not excision: a demand to destroy is answered by excision.md or by saying \
           it is unbuilt, never by a recall reported as a removal",
          Evidence::Deferred(
            "M6 step 5 for the recall report and M4 Phase 5 for excision (E9) - no surface offers \
             destruction and a recall merge has no effect, so nothing today can report a removal; \
             the first guard is the recall effect's own report saying retracted-and-still-readable")),
    ]),
    (9, &[
        c("the condensation substrate is deterministic and identified by member set, and promotion \
           is a gated assertion carrying every co-asserting observation as lineage",
          Evidence::Scenario(&[
            "hypergraph_dedup_by_member_set_accumulates_sources",
            "hypergraph_scoped_deterministic_and_hub_degree",
            "p11_reify_asserts_group_with_lineage",
          ])),
        c("selection is a deterministic fold over stability, corroboration and cohesion, and \
           corroboration counts independent principals rather than repetitions",
          Evidence::Deferred(
            "M6 condensation track (consolidation.md Section 8) - no selection fold exists, and the \
             `sources` a hyperedge carries counts observations (one per co-assertion), not \
             delegation-chain principals, so a fold built on it as-is would count repetition - the \
             P11 rule the clause exists to forbid")),
    ]),
    (10, &[
        c("a generated summary is written from current state and never stored by the server",
          // mcp_surface.rs: getting the brief leaves the observation count unchanged. The brief is
          // the first display-layer summary (prompts.md Section 1 cites 7.2).
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
        c("the digest a summary is written from carries both sides of a contested belief rather \
           than settling it",
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
        c("a summary surface is labelled by `mode` like the recall aid it lives beside",
          Evidence::Deferred(
            "M6, after step 3 (consolidation.md Section 8: 7.2's display-layer summary follows the \
             weight) - the server has no summary surface of its own to label; the prompt digest \
             carries `mode` only on its search hits, and no case reads it")),
    ]),
    (11, &[
        c("the server's generated artifacts - report, weights, digest - are never written, so they \
           never replicate",
          // policy_cases.rs and mcp_surface.rs; plus F1's mechanism - the wire carries attestation
          // events only, so an unwritten artifact has no way across it.
          Evidence::Scenario(&[
            "p7_curation_generates_candidates_and_commits_nothing",
            "a_brief_is_fenced_bounded_and_writes_nothing",
          ])),
        c("a consolidation artifact enters the log only as the enrichment of a gated candidate, \
           derived, lowest-trust and lineage-bearing",
          Evidence::Deferred(
            "M5 with the extractor port - the proposal-workflow.md 14.1 enrichment path is unbuilt, \
             a rationale is the only enrichment slot and carries no tier or lineage, and the prompt \
             instruction telling the model never to observe a brief back is a rule the server \
             cannot check")),
    ]),
    (12, &[
        c("the weight ranks nothing until the materialized span exists, and never without the floor",
          // read_path_cost.rs: search walks the log zero times, so a frontier term cannot enter it
          // unmaterialized; principle_scenarios.rs pins the floor first.
          Evidence::Scenario(&[
            "a_search_does_not_walk_the_log",
            "p7_the_recall_weight_never_reaches_zero",
          ])),
        c("a recall that retracts without its diff is not shipped",
          Evidence::Deferred(
            "M6 step 5 - the effect and the closure diff are both unbuilt, so no case can pin an \
             ordering between them; the honest interim state is pinned on the tbox_change branch by \
             p5_a_diff_for_an_unenforced_kind_reports_uncomputable_not_empty, and the recall branch \
             shares it without a case of its own")),
    ]),
];

// docs/excision.md Section 9 - the destruction-demand exception (E rows). Unbuilt past Section 8
// steps 1-2, which are P17's registry row; every row here is owed to M4 Phase 5 except the one
// that holds by the port's shape.
const EXCISION_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("excision replaces an observation with a tombstone under the same id",
          Evidence::Deferred(
            "M4 Phase 5 (architecture.md Section 14: the first multi-principal deployment is the \
             first time a destruction demand can arrive from someone who is not the operator) - no \
             tombstone type, store method or surface exists; only Section 8 steps 1-2 are built, \
             and they are P17's registry row")),
        c("there is no in-place edit of an observation's content, because content is identity",
          Evidence::Structural(
            "the store port has no edit and no delete: `AssertionStore::add_observation` absorbs \
             into the row the content address names, and no method rewrites or removes one; the id \
             is `blake3(workspace, content, assertions)` (observation_id_includes_assertions), so a \
             changed text is a different row, never the same row changed")),
    ]),
    (2, &[
        c("a tombstone records the act and a structural census, never the content, anything \
           reversibly derived from it, or a reason that quotes it",
          Evidence::Deferred(
            "M4 Phase 5 - there is no tombstone to hold a census or a reason. The detector that \
             would bound the reason exists and never echoes what it finds \
             (a_finding_never_carries_the_secret, \
             p17_a_credential_is_refused_at_ingest_without_being_echoed), but it guards the ingest \
             door and the scan, not a field that does not exist")),
    ]),
    (3, &[
        c("a tombstone is absorbing: `add_observation` for that id is refused locally and from the \
           wire, and the tombstone is never superseded or removed",
          Evidence::Deferred(
            "M4 Phase 5 - `add_observation` has no absorbing state to consult, and the sync apply \
             path (apply_verifies_rejects_and_stays_idempotent) knows only dedup and absorb; the P3 \
             row files the same clause as its tombstone debt")),
    ]),
    (4, &[
        c("a tombstoned id keeps advancing the version vector for its (origin, origin_seq)",
          Evidence::Deferred(
            "M4 Phase 5 - the cursor is derived from stored rows (`version_vector` folds \
             `attestations_since`), which is exactly why a deleted row would be re-pulled; the \
             watermark a tombstone must keep does not exist, and since the failure is invisible on \
             a node with no peers it needs a two-node case the day it lands")),
    ]),
    (5, &[
        c("tombstones propagate through the ordinary signed, allowlisted, workspace-filtered sync \
           path, and the surface reports where excision could not reach",
          Evidence::Deferred(
            "M4 Phase 5 - the path exists (export_respects_share_list_and_vv) but carries no \
             tombstone event, and no surface reports an unreachable copy; excision.md Section 4's \
             excise-first-then-narrow rule has nothing to apply to")),
    ]),
    (6, &[
        c("the derived closure is presented, never cascaded; each excision is its own act with its \
           own tombstone",
          Evidence::Deferred(
            "M5 for the lineage walk (excision.md Section 8: the walk is M5's machinery) and M4 \
             Phase 5 for the act - nothing walks `derived_from` for a destruction candidate today")),
    ]),
    (7, &[
        c("excision is a console-only, non-delegable act, at least as restricted as the recall \
           verdict, and does not pass through the proposal gate",
          Evidence::Deferred(
            "M4 Phase 5 - no excision surface exists on the console or the MCP path, and \
             `PROPOSAL_KINDS` has no such kind, so the gate cannot carry it by accident; the \
             recall-verdict floor this must exceed is the P23 row's own clause")),
    ]),
    (8, &[
        c("after excision the projection is re-materialized from the log, never patched",
          Evidence::Deferred(
            "M4 Phase 5 - `reproject` is the replay (p1_reprojection_rederives_without_touching_the_log) \
             and would be the mechanism, but no excision triggers it; the dangling-target and \
             shrinking-graph consequences of Section 7 have no case")),
    ]),
    (9, &[
        c("a partial implementation is not shipped: excision without the lineage walk, the \
           absorbing state, cursor participation and propagation reports a removal it did not perform",
          Evidence::Deferred(
            "M4 Phase 5 - holds only because nothing is shipped: there is no tool, route, kind or \
             store method for the act, so there is nothing partial to catch. The first case this \
             document names must assert the four parts together, or it will be the partial version")),
    ]),
];

// docs/unmerge.md Section 11 - entity_split, the reversal of a merge (S rows). Built.
const UNMERGE_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("a split is a proposal kind opened through `propose` and decided by a verdict, reachable \
           from the MCP tool and the console",
          // principle_scenarios.rs (propose + review, then the effect); viz http.rs (the console
          // route opens an `entity_split`); mcp_surface.rs (the `propose` description names every
          // kind in PROPOSAL_KINDS, `entity_split` included).
          Evidence::Scenario(&[
            "p3_a_merged_split_reverses_the_merge_it_names",
            "viz_propose_split_opens_a_reversal_without_committing_it",
            "mcp_protocol_surface_end_to_end",
          ])),
        c("an opened split commits nothing - it carries its own state and the merge stands until \
           the verdict",
          Evidence::Scenario(&[
            "viz_propose_split_opens_a_reversal_without_committing_it",
            "p23_a_split_of_an_undecided_merge_has_no_commit_effect",
          ])),
    ]),
    (2, &[
        c("a split names the `entity_merge` proposal it reverses, not a pair of entities",
          Evidence::Scenario(&[
            "p3_a_merged_split_reverses_the_merge_it_names",
            "p5_a_split_of_an_unreadable_target_says_so_instead_of_showing_nothing",
          ])),
    ]),
    (3, &[
        c("a merged split removes that merge's contribution to `merge_forwarding` and nothing else",
          Evidence::Scenario(&[
            "p3_a_merged_split_reverses_the_merge_it_names",
            "p3_a_split_removes_the_merged_name_and_keeps_the_asserted_spelling",
            "p23_a_split_preview_shows_the_endpoints_that_move_back",
          ])),
        c("every consumer follows from the one map, and none of them learns what a split is",
          Evidence::Structural(
            "`reversed_merges` is read by two folds only, `forwarding_less` (which `merge_forwarding` \
             is) and `merge_cycle_sets`; every read path takes the map `merge_forwarding` returns and \
             the candidate generators take the pair set `suppressed_pairs` returns, and the string \
             `entity_split` occurs in no graph, search, alias or traverse code - so a consumer cannot \
             tell a merge that was never cast from one that was split")),
    ]),
    (4, &[
        c("nothing is deleted or edited: the merge proposal, its verdicts and both entity rows stay \
           in the store",
          Evidence::Structural(
            "the store port has no delete: `AssertionStore` appends through `add_observation` (an \
             absorb) and reads, `KnowledgeStore` adds `put_entity`/`add_relation` and the \
             owed-projection ledger, so a split verdict has no method through which to remove a \
             proposal, a verdict or an entity row; the log's only write is absorb")),
        c("the merged-away rows were filtered at read, never rewritten, so reversing the filter \
           restores them exactly",
          Evidence::Scenario(&[
            "p3_a_split_removes_the_merged_name_and_keeps_the_asserted_spelling",
            "p3_a_merged_split_reverses_the_merge_it_names",
          ])),
    ]),
    (5, &[
        c("a split permanently suppresses the separated pair as a suggestion",
          Evidence::Scenario(&["p19_a_split_pair_is_never_suggested_again"])),
        c("a split never suppresses the possibility - the pair stays mergeable by hand",
          Evidence::Scenario(&["p15_separated_entities_can_be_merged_again"])),
        c("suppression is derived from the log, so nodes with equal logs suppress equally",
          Evidence::Deferred(
            "Revisit with a curation-report convergence case - `split_pairs` is computed from the \
             proposal fold on every read and no flag is stored (the port has nowhere to put one), \
             but no case delivers one log to two nodes in different orders and compares their \
             suggestions; the P16 suite compares graphs, not curation reports")),
    ]),
    (6, &[
        c("aliases the merge contributed leave the canonical row on a split; asserted spellings \
           never do, so IR1's set is unchanged",
          Evidence::Scenario(&["p3_a_split_removes_the_merged_name_and_keeps_the_asserted_spelling"])),
    ]),
    (7, &[
        c("merge and split are both reversible: re-merging separated entities is an ordinary new \
           `entity_merge` with no special case",
          Evidence::Scenario(&["p15_separated_entities_can_be_merged_again"])),
    ]),
    (8, &[
        c("a split is not absorbing: it can be followed by a new merge",
          Evidence::Scenario(&["p15_separated_entities_can_be_merged_again"])),
        c("the log records the whole argument - every merge and split verdict stays, and the map \
           is a function of all of them",
          Evidence::Structural(
            "`forwarding_less` folds every proposal in the log and subtracts the ones a merged split \
             names; nothing is removed to make that true because the port has no delete, so the map \
             is a deterministic function of all the verdicts, never of the latest one")),
    ]),
    (9, &[
        c("the verdict on a split whose target is absent, not an `entity_merge`, or undecided is \
           refused and reaches nothing",
          // One predicate - membership in `decided_merges` - covers all three cases, so the
          // undecided case exercises the branch the other two take.
          Evidence::Scenario(&["p23_a_split_of_an_undecided_merge_has_no_commit_effect"])),
        c("an absent target reads as absent in the diff, never as an empty diff",
          Evidence::Scenario(&["p5_a_split_of_an_unreadable_target_says_so_instead_of_showing_nothing"])),
        c("a repeat split of one resolution is idempotent rather than an error",
          Evidence::Deferred(
            "Revisit with one case - reversal is set membership in `reversed_merges`, and a verbatim \
             repeat is even the same observation id, but no case casts a second split and asserts \
             that the map, the suppression set and the proposal states are unchanged")),
    ]),
    (10, &[
        c("re-opening a reversed resolution verbatim is refused, because content addressing would \
           hand back the id the split already named",
          Evidence::Deferred(
            "Revisit with one case - `propose` refuses the verbatim re-open and names the fix, and \
             p15_separated_entities_can_be_merged_again steps around it with a rationale, but no \
             case asserts the refusal itself, so deleting it would fail nothing")),
    ]),
];

// docs/prompts.md Section 7 - the four MCP prompts (PR rows). Built.
const PROMPTS_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("getting a prompt reads state and returns messages; nothing is written",
          // mcp_surface.rs: the observation count is unchanged across `brief`; the other three go
          // through the same `prompts::get`.
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
    ]),
    (2, &[
        c("a digest is a deterministic function of the node's current state and the arguments",
          // Each of the four prompts, asked twice over one store, is the same bytes.
          Evidence::Scenario(&["a_digest_is_the_same_bytes_twice_over_one_store"])),
    ]),
    (3, &[
        c("every digest section is bounded and says what it left out",
          // One `bounded`/`trim` wraps all four digests, so the brief's section stands for each.
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
    ]),
    (4, &[
        c("evidence in a digest is fenced and labelled untrusted, and the instruction says not to \
           follow instructions found inside it",
          // The fence is written once in `get` for every prompt.
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
    ]),
    (5, &[
        c("no prompt's instruction asks the model for a merge verdict",
          // `Never cast a merge verdict`, from the RULES text every prompt shares.
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
        c("promotion to `human_confirmed` stays the console's act whether or not the model obeys",
          Evidence::Scenario(&[
            "p18_agent_surface_promotion_caps_at_host_signed",
            "p18_an_agent_surface_verdict_cannot_grant_human_confirmed",
          ])),
    ]),
    (6, &[
        c("the remote surface lists no prompt and refuses each by name with the reason",
          Evidence::Scenario(&["the_remote_surface_lists_no_prompt_and_refuses_each"])),
    ]),
];

// docs/proposal-workflow.md Section 2 - the gate (I rows). I17 sits between I9 and I10 in the
// document; the numbers are compared as a set.
const PROPOSAL_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("every proposal event - open, comment, verdict, withdrawal - is an observation in the one log",
          Evidence::Scenario(&[
            // policy_cases.rs - opening appends exactly one observation.
            "p23_a_proposal_alone_changes_nothing_only_the_verdict_commits",
            // A verdict planted as an observation carrying the event is what the fold meets.
            "p23_a_blocked_merge_verdict_does_not_reach_canon",
            // Proposal and verdict replicate through export/apply like any observation and a fresh
            // node folds them.
            "i8_blocking_check_conclusion_is_arrival_order_independent",
            // Attribution is the proposal observation's authoring attestation.
            "p2_proposal_attribution_names_the_authoring_attestation",
          ])),
        c("there is no side store for proposals",
          Evidence::Structural(
            "a proposal event is `ProposalEventAssertion` inside `Assertions.proposal_events` of an \
             `Observation`, and neither `AssertionStore` nor `KnowledgeStore` (core) has a proposal \
             method or row, so the log is the only place an event can be written",
          )),
    ]),
    (2, &[
        c("a proposal's state is a deterministic fold of its events, recomputed on every read",
          Evidence::Scenario(&[
            // engine lib.rs unit test - open, merged, rejected from the fold alone.
            "proposal_open_verdict_fold",
            // A node holding only the replicated log folds the same state.
            "i8_blocking_check_conclusion_is_arrival_order_independent",
          ])),
        c("the state is never stored separately",
          Evidence::Structural(
            "`ProposalView` has exactly one construction site, inside `Engine::fold_proposals`, which \
             `get_proposal`/`list_proposals` run over the log on every call; the store port has no \
             proposal row, so a materialized state has no field to live in",
          )),
    ]),
    (3, &[
        c("the conclusion is a function of the verdict set: one valid merge wins, reject only when none exists",
          Evidence::Scenario(&[
            "i16_merge_absorbs_over_conflicting_reject_in_any_order",
            "i8_blocking_check_conclusion_is_arrival_order_independent",
          ])),
        // Both verdicts stay counted. The loser is kept AS a verdict rather than relabeled a
        // comment as 7.1 words it; the demand (decides nothing, not erased) is what is pinned.
        c("a conflicting verdict decides nothing and is kept, never erased",
          Evidence::Scenario(&["i16_merge_absorbs_over_conflicting_reject_in_any_order"])),
    ]),
    (4, &[c(
        "the passage of time alone moves no proposal; expiry or auto-merge is an explicit event",
        Evidence::Deferred(
            "the fold reads only log rows - the engine's clock port is consulted by observe and the \
             recovery ledger, never by `fold_proposals` - but nothing pins a state as clock-independent, \
             and the policy executor that would load an expiry event is M4+ work (Section 13); the \
             case belongs beside it",
        ),
    )]),
    (5, &[
        c("a reject folds to rejected and leaves the proposal open to a later valid merge",
          Evidence::Scenario(&[
            "proposal_open_verdict_fold",
            "i16_merge_absorbs_over_conflicting_reject_in_any_order",
          ])),
        c("the rejected assertion keeps its original tier",
          Evidence::Deferred(
            "`gate_grants` skips every non-merge verdict, so a reject grants and demotes nothing, but \
             no case reads a target's effective tier across a reject. Revisit when the reject path \
             grows its first effect (the Section 13 resubmission cooldown), which is when a case can \
             catch it doing more than recording",
          )),
    ]),
    (6, &[
        c("a merge appends a verdict; the assertion is neither copied nor modified",
          Evidence::Scenario(&[
            // The merge verdict is exactly one appended observation and no id leaves the log.
            "p23_a_blocked_gate_merge_grants_nothing",
            "p23_a_proposal_alone_changes_nothing_only_the_verdict_commits",
          ])),
        c("cancellation is a new demotion or split event, never a rewind",
          Evidence::Scenario(&[
            // A merged demotion is a later gate event, not an edit; a merged split restores the
            // pre-merge graph by filtering, not deleting.
            "p23_demotion_overrides_below_base",
            "p3_a_merged_split_reverses_the_merge_it_names",
          ])),
    ]),
    (7, &[c(
        "a proposal pins the canon frontier at open; a moved touched set makes it stale until re-checked",
        Evidence::Deferred(
            "M4 Phase 5 - no base is pinned at open, Stale is never computed, and diff and checks are \
             functions of the proposal and the growing log (Section 4 and Section 6 [impl]; \
             architecture.md Section 14), so only checks monotone in the growing log ship until the \
             fixed base exists",
        ),
    )]),
    (8, &[c(
        "a check is a pure function of its input: the same conclusion on every node under every delivery order",
        // The input is (proposal, log) today, not (proposal, base frontier) - that half is I7's
        // debt, not a second check.
        Evidence::Scenario(&["i8_blocking_check_conclusion_is_arrival_order_independent"]),
    )]),
    (9, &[
        c("the verdict's authority principal differs from the proposer's, compared by canonical identity",
          Evidence::Characterized(
            &["i9_self_attested_is_blanket_true_until_principal_check_lands"],
            "M4 Phase 5 - no principal comparison exists; the fold accepts any merge and labels every \
             view self_attested (the case pins alice-proposed, bob-merged as merged AND self_attested)",
          )),
        c("a verdict between unresolved principals is suspected self-approval and forces human review",
          Evidence::Deferred(
            "M4 Phase 5 - principal resolution rests on the canon policy's principal-to-key binding \
             (federation.md F17); until then there are no two principals to compare and the \
             deployment stays solo under the P23 exception",
          )),
        c("claim-demotion and recall keep self-approval as the exception when the check lands",
          Evidence::Deferred(
            "M4 Phase 5 - costs nothing to honor today because no principal check exists \
             (resolution.md Section 5); only a case written against the check can pin that demotion \
             keeps the exception while promotion loses it",
          )),
    ]),
    (17, &[c(
        "a recall merge is valid only as a human's direct act: a proxied or agent-cast one decides nothing",
        Evidence::Deferred(
            "M4 Phase 5 for the principal-signed act (federation.md F20 strength ii). The \
             console-marker half - the fold counts a recall merge only with the engine-stamped \
             console marker, and the agent surface refuses to cast one - lands with the \
             recall-verdict change (PR #98) and its two i17 policy cases, which this clause cites \
             once both are on one branch",
        ),
    )]),
    (10, &[c(
        "no proposal state refuses or holds an observation",
        // An observation that a blocked merge verdict already names lands through the ordinary
        // ingest, and its arrival is what unblocks the verdict. The fold is read on the observe
        // path only as projection input, never as a precondition.
        Evidence::Scenario(&["p23_a_blocked_gate_merge_grants_nothing"]),
    )]),
    (11, &[
        c("the clock is monotonic and a received stamp lands it after both sides",
          Evidence::Scenario(&[
            "hlc_is_monotonic_and_merge_lands_after_both",
            // A stamp carries authoring time, so export reorders nothing.
            "a_stamp_carries_the_authoring_time_not_the_export_time",
          ])),
        c("a fold orders by HLC, not arrival, and reaches the same conclusion on every node",
          Evidence::Scenario(&[
            "types_fold_orders_by_hlc_not_observed_at",
            "i8_blocking_check_conclusion_is_arrival_order_independent",
          ])),
    ]),
    (12, &[c(
        "a verdict is bound to the base it reviewed; revise resets unfinalized verdicts and never a finalized merge",
        Evidence::Deferred(
            "M4 Phase 5 - a verdict carries no base reference, there is no revise event \
             (`ProposalEventKind` is Opened/Verdict/Withdrawn/Comment) and the fold checks only \
             7.1(b), so a stale-diff approval can still merge (Section 4 [impl]; architecture.md \
             Section 14), owed with the quorum/revise rules of Section 13",
        ),
    )]),
    (13, &[
        c("the blocking gate is recomputed by the fold: a merge verdict on a failing proposal folds to blocked and commits nothing",
          Evidence::Scenario(&[
            "p23_a_blocked_merge_verdict_does_not_reach_canon",
            "p23_a_blocked_gate_merge_grants_nothing",
            "i8_blocking_check_conclusion_is_arrival_order_independent",
            "p23_a_well_formed_merge_passes_its_checks_and_commits",
          ])),
        c("a reported check result is advisory; a forged pass cannot promote",
          Evidence::Structural(
            "`ProposalEventKind` has no check-report variant (Opened, Verdict, Withdrawn, Comment), so \
             no event can carry a result for the fold to trust; `fold_proposals` runs \
             `blocking_failures` itself over the log for every proposal carrying a merge verdict",
          )),
    ]),
    (14, &[
        c("loading an effect twice is harmless",
          Evidence::Structural(
            "there are no effect events to load twice: `ProposalEventKind` carries no \
             tier_promoted/entities_merged variant, every effect is folded from the verdict \
             (`gate_grants`, merge forwarding) and the tally is a boolean, so a second copy of a \
             verdict is a second observation with the same content address, which the log dedups",
          )),
        c("duplicate entity-merge proposals do not diverge, canonicalized by canonical id order",
          Evidence::Characterized(
            &["p6_contradictory_merge_cycle_is_convergent_and_surfaced"],
            "M4 Phase 5 - opposite-direction merges converge, but by hop-capped iteration parity, \
             which the case pins as not a principled rule; the canonical-id order of the text is \
             the Section 13 open decision on canonicalization, owed with the quorum/revise rules",
          )),
    ]),
    (15, &[c(
        "an automatic verdict counts only if the fold re-validates its routing premises at merge time",
        Evidence::Deferred(
            "M4+ - the auto-merge policy executor does not exist (Section 13; resolution-identity.md \
             Section 3 keeps the top band human), the informative checks it would re-validate are not \
             computed, impact radius waits on I7's fixed base, and the fold cannot tell an automatic \
             verdict from a human one",
        ),
    )]),
    (16, &[
        c("merge is absorbing: no later or concurrent event cancels a valid merge",
          Evidence::Scenario(&["i16_merge_absorbs_over_conflicting_reject_in_any_order"])),
        c("reject is provisional: a late valid merge raises it to merged",
          Evidence::Scenario(&["i16_merge_absorbs_over_conflicting_reject_in_any_order"])),
        c("the blocking conclusion moves blocked -> merged only, never the reverse",
          Evidence::Scenario(&[
            // The missing target's arrival unblocks the same verdict.
            "i8_blocking_check_conclusion_is_arrival_order_independent",
            "p23_a_blocked_gate_merge_grants_nothing",
          ])),
        c("a reversal of a promotion is a new proposal",
          Evidence::Scenario(&[
            "p23_demotion_overrides_below_base",
            "p3_a_merged_split_reverses_the_merge_it_names",
          ])),
    ]),
    (18, &[
        c("a consolidation pass emits read-only signals and candidates and commits nothing",
          Evidence::Scenario(&[
            "p7_curation_generates_candidates_and_commits_nothing",
            "merge_suggestions_never_commit",
          ])),
        c("the console's accept is a verdict_cast observation, never a projection or log write",
          Evidence::Scenario(&[
            // One /api/resolve act is a proposal plus a Console verdict in the trail.
            "viz_resolve_settles_a_contested_belief",
            "p23_a_proposal_alone_changes_nothing_only_the_verdict_commits",
          ])),
        c("a recall acceptance stays a human's direct act even from the console",
          Evidence::Deferred(
            "M4 Phase 5 for the principal-signed act - the same debt as I17: the console-marker \
             half lands with the recall-verdict change (PR #98), whose i17 policy cases this clause \
             cites once both are on one branch",
          )),
    ]),
];

// docs/resolution.md Section 8 - belief resolution (R rows).
const RESOLUTION_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("the belief is computed by a replaceable pure policy behind a port",
          Evidence::Structural(
            "`ResolutionPolicy` is a core trait with one method over a `BeliefCandidate` slice, held \
             by the engine as `Arc<dyn ResolutionPolicy>` and swapped with `Engine::with_policy`; the \
             belief folds and `reproject` reach the rule only through it, so a second policy is a \
             type implementing the trait, not an edit to the fold",
          )),
        c("no projection write encodes a decision the policy did not make: reproject recomputes the belief from the unchanged log",
          Evidence::Scenario(&[
            "p1_reprojection_rederives_without_touching_the_log",
            "incremental_write_equals_replay",
            // The representative is the policy's choice, not first arrival.
            "p16_canonical_name_selection_is_arrival_order_free",
          ])),
    ]),
    (2, &[c(
        "selection is effective tier, then ordering HLC, then observation id - no wall clock, no arrival order",
        Evidence::Scenario(&[
            // core lib.rs - each step, and slice-order independence.
            "tier_weighted_selection_order",
            // The id tiebreak named through the engine under tied HLCs.
            "aliases_accumulate_and_converge",
            // Recency within a tied band, for kinds and for type definitions.
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "type_def_conflict_surfaces_contested",
        ]),
    )]),
    (3, &[
        c("confidence never selects",
          Evidence::Structural(
            "`BeliefCandidate` - the policy's whole input - carries value, effective tier, ordering \
             HLC and observation id and no confidence field, so `choose` has nothing to read; a \
             policy that weights confidence must first change the input type",
          )),
        c("confidence is carried verbatim and an unstated confidence stays unstated",
          Evidence::Scenario(&[
            "unstated_confidence_is_distinct_from_full_confidence",
            "p2_a_workspace_rekey_carries_provenance_that_a_reingest_would_restamp",
          ])),
    ]),
    (4, &[
        c("a wire claim never evaluates above HostSigned, at the policy input and at every read surface",
          Evidence::Scenario(&[
            "evaluated_tier_caps_remote_claimed",
            "f13_read_path_evaluates_remote_claim_at_host_signed",
            "p18_rekey_and_migration_clamp_a_synced_claim_to_its_evaluation",
          ])),
        c("only a merged gate event under the Console ceiling reaches HumanConfirmed",
          Evidence::Scenario(&[
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "p18_agent_surface_promotion_caps_at_host_signed",
            "p18_an_agent_surface_verdict_cannot_grant_human_confirmed",
          ])),
        c("a local writer cannot self-declare a tier",
          Evidence::Structural(
            "`ObserveInput` has no tier field (content, workspace, source_ref, confidence, \
             on_behalf_of, derived_from, entities, relations) and the engine stamps the default, so \
             a client-declared tier is not a value the ingest door can receive",
          )),
    ]),
    (5, &[
        c("a merged gate event overrides the base evaluation in both directions",
          Evidence::Scenario(&[
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "p23_demotion_overrides_below_base",
            // The recall weight consumes the same evaluation.
            "p18_a_merged_demotion_lowers_the_recall_weight_of_its_target",
          ])),
        c("with several merged gate events on one target, the HLC-latest governs",
          Evidence::Deferred(
            "`gate_grants` keeps the max over (verdict HLC, proposal id) per target, but no case \
             stacks two merged gate events on one observation, so the rule holds unchecked. Revisit \
             with the quorum/revise rules of proposal-workflow.md Section 13 (M4 Phase 5), whose \
             cases must stack verdicts",
          )),
    ]),
    (6, &[
        c("contested iff distinct values survive at a tied top tier; a higher tier resolves silently in the projection",
          Evidence::Scenario(&[
            "contested_iff_top_tier_ties",
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "type_def_conflict_surfaces_contested",
          ])),
        c("contested status is part of the projection and converges with it",
          Evidence::Scenario(&[
            "cross_node_reprojection_converges",
            "p16_partitioned_and_duplicated_delivery_converges",
          ])),
    ]),
    (7, &[
        c("a conflict trust resolved stays listed in the curation report and the defeated assertion stays queryable",
          Evidence::Scenario(&[
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
            "viz_resolve_settles_a_contested_belief",
            "p6_contradictory_merge_cycle_is_convergent_and_surfaced",
          ])),
        c("a defeated assertion is reinstated by re-resolution when the tier landscape changes",
          // Demoting the winner makes the surviving side the belief again, with no edit.
          Evidence::Scenario(&["p23_demotion_overrides_below_base"])),
    ]),
    (8, &[
        c("the surface marker is engine-stamped; the reserved namespace is refused at every local ingest door",
          Evidence::Scenario(&[
            "p18_reserved_surface_namespace_is_refused_at_every_ingest_door",
            "surface_markers_live_under_the_reserved_prefix",
          ])),
        c("the ceiling is applied by the fold reading the marker off the log, never by the write path",
          Evidence::Scenario(&[
            "verdict_ceiling_by_surface_marker",
            // A console-marked verdict planted store-side grants through the fold.
            "p23_a_blocked_gate_merge_grants_nothing",
            "p18_agent_surface_promotion_caps_at_host_signed",
          ])),
    ]),
    (9, &[
        c("mediation enters as events: a counter-assertion, a proposal, a verdict",
          Evidence::Scenario(&[
            "viz_resolve_settles_a_contested_belief",
            "p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it",
          ])),
        c("there is no API by which a human edits the belief directly",
          Evidence::Structural(
            "the store port is split: `Engine::store()` hands out `AssertionStore` (append to the log, \
             read the graph) and `put_entity`/`add_relation` exist only on `KnowledgeStore`, which \
             the engine holds and calls from the folds alone - the viewer, the MCP tools and the CLI \
             reach no projection write (the P1 mechanism)",
          )),
    ]),
];

// docs/resolution-identity.md - entity identity (IR rows, list items scattered through the sections).
const IDENTITY_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("aliases are the log's asserted spellings minus the representative - none dropped, none duplicated",
          Evidence::Scenario(&[
            "aliases_accumulate_and_converge",
            "p3_a_new_spelling_accumulates_and_never_displaces",
            // A merged-away name joins the canonical row's aliases.
            "get_entity_forwards_a_merged_id",
          ])),
        c("the alias set is identical on nodes with equal logs",
          Evidence::Scenario(&[
            "aliases_accumulate_and_converge",
            "p16_partitioned_and_duplicated_delivery_converges",
          ])),
    ]),
    (2, &[c(
        "no code path turns a similarity score into a merge without a verdict observation",
        Evidence::Scenario(&[
            "merge_suggestions_never_commit",
            "p7_curation_generates_candidates_and_commits_nothing",
        ]),
    )]),
    (3, &[c(
        "for any log state the incremental projection of the last write equals the row reproject would produce",
        Evidence::Scenario(&[
            "incremental_write_equals_replay",
            "incremental_write_equals_replay_for_relations",
        ]),
    )]),
    (4, &[c(
        "the stored embedding corresponds to the current name+aliases text or is absent, never silently stale",
        Evidence::Scenario(&["embedding_recomputed_on_alias_change"]),
    )]),
    (5, &[c(
        "type-definition conflicts surface in the same contested/competitor shape as entity kinds, settled by one mediation act",
        Evidence::Scenario(&["type_def_conflict_surfaces_contested"]),
    )]),
    (6, &[c(
        "an induced type candidate is lineage-bearing, lowest-trust and gated; hyperedges remain a reference, never a judge",
        Evidence::Deferred(
            "M5 - naming the induced type is the extractor's probabilistic work (Section 7 [impl]), so \
             no tbox_change candidate is generated and the test the document names for it is named \
             for M5 rather than written; the reify half that shipped asserts an A-Box group entity, \
             not a type",
        ),
    )]),
];

// docs/negotiated-surface.md Section 8 - the negotiated surface (N rows). The table lists
// N1-N5, N11, N12, N6-N10; the numbers are compared as a set.
const NEGOTIATED_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("negotiation happens in the background health loop only",
          Evidence::Structural(
            "the map has two writers and both sit in the daemon's health loop: the success arm \
             inserts under `surfaces.write()` and the failure arm calls `record_ping(.., None, 0)`. \
             The MCP crate never writes it - `route` (sync lib.rs) and `sync_status` take \
             `surfaces.read()` only",
          )),
        c("a tool handler reads the cached map and does no network I/O to obtain it",
          Evidence::Scenario(&[
            // Both hosts are closed ports, and the round still skips exactly the host the cached
            // map says refused. A handler that negotiated would have found both unreachable, hence
            // unknown, hence consulted - and skipped neither.
            "a_narrowed_round_names_the_hosts_it_skipped",
            "routing_on_the_negotiated_map_records_nothing",
          ])),
    ]),
    (2, &[c(
        "the map is link-local runtime state, never written to the observation log",
        // The log is byte-identical before and after a narrowed round.
        Evidence::Scenario(&["routing_on_the_negotiated_map_records_nothing"]),
    )]),
    (3, &[
        c("the map may only remove a host from a round",
          Evidence::Scenario(&["routing_narrows_on_a_refusal_and_never_on_ignorance"])),
        c("it never extends share_workspaces: a read-authorization answer decides no write",
          Evidence::Structural(
            "`route(links, surfaces, workspace)` returns a partition of its `links` argument and \
             reads nothing else (sync lib.rs); the share list is checked at each fan-out site before \
             `route` is consulted and is never passed to it, so a host's answer has no path into \
             what leaves - a_remote_search_does_not_leave_for_an_unshared_workspace pins the \
             share-list check running first",
          )),
    ]),
    (4, &[c(
        "an unreachable host yields unknown, never an empty grant set",
        Evidence::Scenario(&["a_failed_check_records_unknown_and_not_an_empty_grant"]),
    )]),
    (5, &[
        c("a narrowed sync_push or sync_pull names the hosts it skipped as not-admitted",
          Evidence::Scenario(&["a_narrowed_round_names_the_hosts_it_skipped"])),
        c("a narrowed federated search names the hosts it skipped",
          Evidence::Deferred(
            "the search fan-out consults the same `route` and attaches `skipped`, but the in-process \
             case drives only sync_push and sync_pull. incremental - add search_knowledge \
             scope=remote to a_narrowed_round_names_the_hosts_it_skipped",
          )),
    ]),
    (11, &[c(
        "a host is dropped only on an explicit refusal; unknown is consulted",
        Evidence::Scenario(&[
            "routing_narrows_on_a_refusal_and_never_on_ignorance",
            "a_failed_check_records_unknown_and_not_an_empty_grant",
            "a_narrowed_round_names_the_hosts_it_skipped",
        ]),
    )]),
    (12, &[c(
        "narrowing applies to fan-out, not to a one-shot command naming one target",
        Evidence::Structural(
            "the CLI's one-shot round calls `SyncClient::sync_workspace(store, node, ws, share, keys)`, \
             which takes no `NegotiatedSurfaces`; `supragnosis_sync::route` has exactly three callers, \
             the daemon's fan-out handlers, and a one-shot process builds no map because it runs no \
             health loop",
        ),
    )]),
    (6, &[
        c("the difference is three buckets, never the intersection",
          Evidence::Scenario(&["the_surface_difference_reports_both_directions_of_disagreement"])),
        c("sync_status and the viewer's federation blob carry the buckets with their time",
          Evidence::Deferred(
            "both surfaces call `surface_diff` but no case reads `negotiated` back from either; the \
             one sync_status case checks config_notes only. incremental - seed a map in \
             a_configuration_workaround_reaches_the_operator_surface and assert the three buckets \
             and negotiated_at",
          )),
    ]),
    (7, &[
        c("an answer carries the time it was negotiated, and only an answer does",
          Evidence::Scenario(&["a_failed_check_records_unknown_and_not_an_empty_grant"])),
        c("the map is never a premise for a durable conclusion",
          Evidence::Deferred(
            "a claim about every future consumer: the only consumer today calls `route` inside \
             each handler and caches nothing, so there is nothing yet for a case to catch doing \
             otherwise - the F21 row says the same. Revisit when something wants to keep a \
             routing decision",
          )),
    ]),
    (8, &[
        c("a sync credential or link mistake narrows or disables federation and the node starts",
          Evidence::Scenario(&[
            // Both shapes -> per-server wins, flat keys reported IGNORED; no token -> no links and a
            // "federation is OFF" note, not an Err.
            "config_parses_and_rejects_typos",
            // Plain HTTP off loopback and an unreadable CA drop the link with a note.
            "the_bearer_only_crosses_the_network_encrypted_to_a_verified_host",
            "a_node_that_admits_itself_is_reported_and_ignored",
          ])),
        c("every workaround is reported in sync_status, not only at boot",
          Evidence::Scenario(&["a_configuration_workaround_reaches_the_operator_surface"])),
    ]),
    (9, &[
        c("an entry naming this node is dropped through every construction path, and said so",
          Evidence::Scenario(&[
            "a_node_is_never_its_own_peer_through_either_path",
            "a_node_that_admits_itself_is_reported_and_ignored",
          ])),
        c("the file is left alone",
          Evidence::Structural(
            "the drop happens in two pure functions over the parsed section - `drop_self_admission` \
             returns notes (cli main.rs) and `PeerDirectory` filters the id in `derive` (sync \
             http.rs) - and neither is handed the config path, so the file is not something they \
             can reach; the one writer of supragnosis.toml is the narrowing act",
          )),
    ]),
    (10, &[c(
        "a host advertises the caller's own grants, never its inventory",
        Evidence::Scenario(&["ping_answers_with_the_callers_own_grants_and_not_the_hosts_inventory"]),
    )]),
];

// docs/crash-recovery.md Section 5 - the owed-projection ledger (K rows). Built.
const CRASH_RECOVERY_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("every append writes its owed entry, and only a clear removes it, on every adapter",
          Evidence::Scenario(&["every_append_owes_its_projection_until_cleared"])),
        c("the entry is written in the same store transaction as the row",
          Evidence::Structural(
            "in the redb adapter the OWED insert sits inside the write transaction that inserted \
             the row, between the row insert and the single `txn.commit()`, so a crash cannot leave \
             one without the other; the in-memory adapter inserts both under one write lock",
          )),
        c("no append path can skip it",
          Evidence::Structural(
            "the ledger write is inside `add_observation`, the only log-append method on the \
             `AssertionStore` port, and the adapters write it themselves; a caller holding only the \
             narrow port - the sync crate's `apply` - has no other way into the log, which is the \
             path every_append_owes_its_projection_until_cleared exercises",
          )),
        c("appends from before the ledger existed are owed once, at the first ledger-aware open",
          Evidence::Scenario(&["a_store_from_before_the_ledger_is_owed_in_full_once"])),
    ]),
    (2, &[
        c("a writer clears only its own entry, and only after its last projection write",
          Evidence::Scenario(&[
            "a_completed_write_owes_nothing",
            // An append with no projection stays owed; the clear runs after `project_relations`.
            "health_reports_owed_projections_and_the_last_recovery",
          ])),
        c("a reproject clears the workspace it projected and no other",
          Evidence::Scenario(&["a_reproject_repays_only_its_own_workspace"])),
        c("a reproject clears only the entries it read before it began",
          Evidence::Deferred(
            "`reproject` reads the owed ids before it opens its read context and clears exactly \
             those, but no case appends during a reproject to see the late entry survive. Revisit \
             with the atomic-write follow-up crash-recovery.md Section 2 names, which removes the \
             window this ordering exists for",
          )),
        c("the hub re-materializes after a pull or push that stamped rows",
          Evidence::Scenario(&["a_pull_that_stamps_rows_re_materializes_them"])),
    ]),
    (3, &[
        c("a writer that opens a store with owed entries re-projects exactly those workspaces",
          Evidence::Scenario(&[
            "an_append_whose_projection_never_ran_is_projected_at_the_next_open",
            "redb_owed_projections_survive_a_reopen",
          ])),
        c("the repayment runs before the process serves a read or accepts a write",
          Evidence::Deferred(
            "`build_engine` repays before handing the engine to every daemon and CLI entry that \
             binds, but that is call order inside one function; no case starts a daemon over a \
             seeded ledger and probes its socket, and the SIGKILL check in crash-recovery.md \
             Section 7 was run by hand. Revisit when the lifecycle suite can start a daemon over a \
             scratch store",
          )),
    ]),
    (4, &[
        c("a process ending between append and projection leaves an entry the next open repays",
          Evidence::Scenario(&[
            // The crash is the engine dropped after an append through the narrow port; the ledger
            // outlives the process.
            "an_append_whose_projection_never_ran_is_projected_at_the_next_open",
            "redb_owed_projections_survive_a_reopen",
            "a_store_from_before_the_ledger_is_owed_in_full_once",
          ])),
        c("a projection write that fails leaves the entry for the next open",
          Evidence::Deferred(
            "the clear sits after the projection writes, so an error returns with the entry in \
             place, but nothing pins it because there is no fault-injecting adapter - the gap F12's \
             store leg records. Revisit when one exists",
          )),
    ]),
    (5, &[
        c("/api/health carries the ledger's size and the last recovery",
          Evidence::Scenario(&["health_reports_owed_projections_and_the_last_recovery"])),
        c("supragnosis status carries the ledger's size and the last recovery",
          // The status document's `store` object is held to the fixture's keys owed_projections
          // and last_recovery; the human-readable "recovered at start" line is not pinned.
          Evidence::Scenario(&["status_json_matches_its_example"])),
    ]),
];

// docs/remote-server.md Section 8 - the hub's agent surface (R rows).
const REMOTE_SERVER_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("the local MCP daemon binds loopback only",
          Evidence::Scenario(&["parse_loopback_addr_accepts_loopback_rejects_public"])),
        c("MCP leaves loopback only on the hub's listener, and only with TLS",
          Evidence::Scenario(&[
            "bind_guard_enforces_f10",
            "tls_listener_serves_https_and_refuses_plaintext",
          ])),
        c("a non-loopback listener needs at least one admitted principal or node",
          Evidence::Deferred(
            "`serve` counts `allowlist + principals`, and the daemon's pre-validation now passes the \
             same sum (it used to pass the allowlist alone, so a principals-only hub off loopback \
             died at startup); bind_guard_enforces_f10 drives `validate_bind` with one count and no \
             case starts a principals-only hub. incremental - add that case to the guard",
          )),
    ]),
    (2, &[c(
        "every remote request is authenticated as exactly one admitted principal; none is anonymous",
        Evidence::Scenario(&[
            // No credential -> 401; the `admit_request` layer wraps the merged /mcp + /viz router.
            "the_read_tier_answers_only_within_the_grants",
            // Wrong credential, revoked, unparsable file: nobody admitted.
            "principals_are_admitted_and_revoked_through_the_file",
            // The call ran as the authenticated principal, whatever the client claimed.
            "a_principal_sees_and_writes_only_what_it_was_granted",
        ]),
    )]),
    (3, &[
        c("every tool has a declared remote policy, and a tool without one is refused",
          Evidence::Scenario(&[
            "every_tool_has_a_remote_policy",
            "governance_and_node_operations_are_refused_remotely",
          ])),
        c("a principal reads and writes only granted workspaces, enumeration and ids included",
          Evidence::Scenario(&[
            "a_principal_reads_only_its_grants",
            "a_remote_write_is_the_principals_own",
            "a_principal_sees_and_writes_only_what_it_was_granted",
            "an_id_outside_the_grants_reads_as_an_unknown_one",
          ])),
    ]),
    (4, &[
        c("a remote write is on_behalf_of the principal; a client's claim cannot replace it",
          Evidence::Scenario(&[
            "a_remote_write_is_the_principals_own",
            "a_principal_sees_and_writes_only_what_it_was_granted",
          ])),
        c("a remote write is recorded at no higher than AgentExtracted",
          Evidence::Scenario(&["a_principal_sees_and_writes_only_what_it_was_granted"])),
        c("a remote write is recorded with host = the hub",
          Evidence::Structural(
            "`ObserveRequest` carries no host field (content, workspace, source_ref, confidence, \
             on_behalf_of, derived_from, entities, relations), so the engine stamps `host` from the \
             label it was built with and a client cannot name one",
          )),
    ]),
    (5, &[
        c("another node's content is served only with that node's consent for the workspace",
          Evidence::Scenario(&[
            "another_nodes_knowledge_needs_its_consent",
            "consent_rides_a_header_and_an_older_node_sends_none",
            "consent_is_kept_and_can_be_withdrawn",
            "serving_is_narrowed_to_what_is_shared",
          ])),
        c("without consent the answer is a refusal naming the origin, never a partial view",
          Evidence::Scenario(&[
            "another_nodes_knowledge_needs_its_consent",
            "a_refusal_names_the_release_each_unconsented_node_runs",
            "a_union_with_an_unconsented_workspace_is_refused_whole",
          ])),
    ]),
    (6, &[
        c("no credential appears in an AI app's configuration",
          Evidence::Scenario(&["registrations_run_the_bridge_and_carry_no_secret"])),
        c("no credential appears on a command line",
          // `server add` argv is name, url, --ca; the credential goes on stdin. The CLI side is by
          // construction: `ServerCmd::Add { name, url, ca }` has no token argument and reads stdin.
          Evidence::Scenario(&["a_credential_never_becomes_an_argument"])),
        c("the bridge and the CLI read the credential from a 0600 file",
          Evidence::Scenario(&[
            // A URL without a token file is refused; the target names a file.
            "the_environment_names_a_server_only_with_a_credential_file",
            // `write_secret`, which `server add` uses, is 0600 and never at another mode.
            "the_state_directory_and_its_secrets_are_closed_to_other_accounts",
          ])),
    ]),
    (7, &[c(
        "review, define_type and the sync_* tools are refused on the remote surface",
        Evidence::Scenario(&[
            "governance_and_node_operations_are_refused_remotely",
            "a_principal_sees_and_writes_only_what_it_was_granted",
        ]),
    )]),
    (8, &[
        c("a remote profile URL is HTTPS, or plain HTTP to loopback only",
          Evidence::Scenario(&["a_remote_server_is_reached_over_verified_tls_or_loopback"])),
        c("there is no option to skip verification",
          Evidence::Structural(
            "`ServerEntry` is `deny_unknown_fields` with `url`, `ca` and `token_file` only, and the \
             bridge's client builder installs nothing but a trusting root via `tls_trusting`, whose \
             verifier pins the named certificates or delegates to webpki and otherwise answers \
             UnknownIssuer (sync http.rs); the node side's `insecure_tls` is reported as no longer read",
          )),
    ]),
];

// docs/remote-viewer.md Section 8 - the hub's read tier (V rows). Steps 0-1 of Section 10 are
// built; steps 2 (the relay) and 3 (the app) are not.
const REMOTE_VIEWER_REGISTRY: &[(u8, &[Clause])] = &[
    (1, &[
        c("a read-tier request without an admitted credential is answered 401 before any route",
          Evidence::Scenario(&[
            "the_read_tier_answers_only_within_the_grants",
            "principals_are_admitted_and_revoked_through_the_file",
          ])),
        c("the page and its assets are behind the same admission as the API",
          Evidence::Structural(
            "the `admit_request` layer wraps the merged router after `/viz` is merged into it \
             (principal.rs), so `/`, `/viewer.css` and `/viewer.js` cannot be routed without \
             passing it; the read tier has no route outside that router",
          )),
    ]),
    (2, &[
        c("every path on the read tier has a declared policy, and an undeclared one is refused",
          // Router paths == policy table in both directions; an unknown path answers 404 at runtime.
          Evidence::Scenario(&["every_viewer_path_has_a_remote_policy"])),
        c("no path changes state, and the console's verdict path is unreachable",
          // /api/review, /api/reify, /api/federation, /api/health -> 403; POST -> 405.
          Evidence::Scenario(&["the_read_tier_answers_only_within_the_grants"])),
    ]),
    (3, &[
        c("enumeration, an omitted workspace and a named one resolve against the grants",
          Evidence::Scenario(&["the_read_tier_answers_only_within_the_grants"])),
        c("`*` is the union of grants, computed per workspace and merged",
          Evidence::Scenario(&[
            "the_read_tier_answers_only_within_the_grants",
            "a_union_sums_counts_and_keeps_the_larger_max",
          ])),
        c("an id outside the grants is answered as an unknown id, on both surfaces",
          Evidence::Scenario(&[
            "the_read_tier_answers_only_within_the_grants",
            "an_id_outside_the_grants_reads_as_an_unknown_one",
          ])),
        c("the event stream is filtered to the reader's grants",
          Evidence::Scenario(&["the_read_tier_streams_knowledge_not_activity"])),
    ]),
    (4, &[c(
        "a union including a workspace the hub may not serve is refused whole, with the reason",
        Evidence::Scenario(&["a_union_with_an_unconsented_workspace_is_refused_whole"]),
    )]),
    (5, &[c(
        "the credential is read per request from its 0600 file and sent only as a header",
        Evidence::Deferred(
            "the relay (`supragnosis bridge --viewer`, remote-viewer.md Section 10 step 2) is not \
             built - no code reads a credential for the viewer, so nothing can be pinned; the hub \
             side already takes it from `Authorization` only (principal.rs). Revisit when step 2 \
             lands, with the profile's `token_file` guard (R6) as the model",
        ),
    )]),
    (6, &[c(
        "the relay verifies TLS, relays only GET, and cannot outlive the app that started it",
        Evidence::Deferred(
            "step 2 of remote-viewer.md Section 10, not built: there is no `--viewer` relay in the \
             CLI, so none of the three demands has code to hold to. The TLS half will reuse the \
             profile rule R8 pins today. Revisit when step 2 lands",
        ),
    )]),
    (7, &[c(
        "the remote stream carries knowledge changes in readable, servable workspaces only",
        Evidence::Scenario(&["the_read_tier_streams_knowledge_not_activity"]),
    )]),
    (8, &[c(
        "on a remote profile each failure state has its own page, never an empty graph",
        Evidence::Deferred(
            "step 3 of remote-viewer.md Section 10, not built: the shell still serves the one \
             placeholder page `remote_html` for every remote state, and no app case pins even that. \
             Revisit when step 3 lands with the relay's `state` JSON",
        ),
    )]),
    (9, &[c(
        "the local viewer is unchanged: a 0600 unix socket, the full console, no credential",
        Evidence::Scenario(&[
            "viz_socket_is_owner_only_and_review_needs_no_browser_headers",
            "p17_socket_directory_denies_foreign_users_before_the_socket_mode",
        ]),
    )]),
];

/// One design document's invariant family: the file, the id prefix its rows use, and the registry
/// rows that give each invariant an evidence state. The coupling that used to exist for the F axis
/// alone - an invariant written into the document is a failure here until it is given a state -
/// now holds for every document that declares invariants, because the hole it closes was not
/// specific to federation: an invariant anywhere could be written with no accounting at all.
struct Family {
    doc: &'static str,
    text: &'static str,
    /// The letters before the number: `F`, `I`, `IR`, `PR`, ... One document, one prefix.
    prefix: &'static str,
    rows: &'static [(u8, &'static [Clause])],
}

/// Every invariant family the design documents declare. The federation rows keep their own const
/// above; the rest follow it in the same shape. A document that grows an invariant table is added
/// here, or `design_docs_declare_every_invariant` names it.
const FAMILIES: &[Family] = &[
    Family {
        doc: "docs/federation.md",
        text: FEDERATION_DOC,
        prefix: "F",
        rows: FEDERATION_REGISTRY,
    },
    Family {
        doc: "docs/inspector.md",
        text: include_str!("../../../docs/inspector.md"),
        prefix: "D",
        rows: INSPECTOR_REGISTRY,
    },
    Family {
        doc: "docs/client-connect.md",
        text: include_str!("../../../docs/client-connect.md"),
        prefix: "C",
        rows: CLIENT_CONNECT_REGISTRY,
    },
    Family {
        doc: "docs/daemon-lifecycle.md",
        text: include_str!("../../../docs/daemon-lifecycle.md"),
        prefix: "L",
        rows: DAEMON_LIFECYCLE_REGISTRY,
    },
    Family {
        doc: "docs/settings-page.md",
        text: include_str!("../../../docs/settings-page.md"),
        prefix: "S",
        rows: SETTINGS_PAGE_REGISTRY,
    },
    Family {
        doc: "docs/consolidation.md",
        text: include_str!("../../../docs/consolidation.md"),
        prefix: "C",
        rows: CONSOLIDATION_REGISTRY,
    },
    Family {
        doc: "docs/excision.md",
        text: include_str!("../../../docs/excision.md"),
        prefix: "E",
        rows: EXCISION_REGISTRY,
    },
    Family {
        doc: "docs/unmerge.md",
        text: include_str!("../../../docs/unmerge.md"),
        prefix: "S",
        rows: UNMERGE_REGISTRY,
    },
    Family {
        doc: "docs/prompts.md",
        text: include_str!("../../../docs/prompts.md"),
        prefix: "PR",
        rows: PROMPTS_REGISTRY,
    },
    Family {
        doc: "docs/proposal-workflow.md",
        text: include_str!("../../../docs/proposal-workflow.md"),
        prefix: "I",
        rows: PROPOSAL_REGISTRY,
    },
    Family {
        doc: "docs/resolution.md",
        text: include_str!("../../../docs/resolution.md"),
        prefix: "R",
        rows: RESOLUTION_REGISTRY,
    },
    Family {
        doc: "docs/resolution-identity.md",
        text: include_str!("../../../docs/resolution-identity.md"),
        prefix: "IR",
        rows: IDENTITY_REGISTRY,
    },
    Family {
        doc: "docs/negotiated-surface.md",
        text: include_str!("../../../docs/negotiated-surface.md"),
        prefix: "N",
        rows: NEGOTIATED_REGISTRY,
    },
    Family {
        doc: "docs/crash-recovery.md",
        text: include_str!("../../../docs/crash-recovery.md"),
        prefix: "K",
        rows: CRASH_RECOVERY_REGISTRY,
    },
    Family {
        doc: "docs/remote-server.md",
        text: include_str!("../../../docs/remote-server.md"),
        prefix: "R",
        rows: REMOTE_SERVER_REGISTRY,
    },
    Family {
        doc: "docs/remote-viewer.md",
        text: include_str!("../../../docs/remote-viewer.md"),
        prefix: "V",
        rows: REMOTE_VIEWER_REGISTRY,
    },
];

/// The invariant numbers a document declares under one prefix, first occurrence of each, in
/// document order. Three row shapes are in use and all are read: a bold table row `| **E1** |`, a
/// plain table row `| I1 |`, and a list item `- **F1**` or `- **IR1**:`. The id must end right
/// after its digits (`**`, `|` or ` |`), so prefix `R` does not swallow an `IR` row and a heading
/// cell such as `| Invariant |` is not a row. A later table that cites the same ids (a closure
/// map, a test plan) repeats numbers already seen and adds nothing.
fn documented_rows(text: &str, prefix: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        let rest = if let Some(r) = l.strip_prefix("| **") {
            r
        } else if let Some(r) = l.strip_prefix("- **") {
            r
        } else if let Some(r) = l.strip_prefix("| ") {
            r
        } else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(prefix) else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let after = &rest[digits.len()..];
        if !(after.starts_with("**") || after.starts_with(" |") || after.starts_with('|')) {
            continue;
        }
        if let Ok(n) = digits.parse::<u8>() {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Every document with an invariant table is a family here. Scanned by shape rather than listed by
/// hand, so a new document's table cannot sit unaccounted the way F21 once did: the prefixes a
/// document uses are read off its rows, and each (document, prefix) pair must be a [`Family`].
#[test]
fn design_docs_declare_every_invariant() {
    let mut missing: Vec<String> = Vec::new();
    for (doc, text) in DESIGN_DOCS {
        // The prefixes this document's rows use: the letters before the digits of every row-shaped
        // line. Principles.md has no invariant rows; its coupling is `every_principle_declares_its_evidence`.
        let mut prefixes: Vec<String> = Vec::new();
        for line in text.lines() {
            let l = line.trim_start();
            let rest = if let Some(r) = l.strip_prefix("| **") {
                r
            } else if let Some(r) = l.strip_prefix("- **") {
                r
            } else if let Some(r) = l.strip_prefix("| ") {
                r
            } else {
                continue;
            };
            let letters: String = rest.chars().take_while(char::is_ascii_uppercase).collect();
            if letters.is_empty() || letters.len() > 2 {
                continue;
            }
            let tail = &rest[letters.len()..];
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            let after = &tail[digits.len()..];
            if digits.is_empty()
                || !(after.starts_with("**") || after.starts_with(" |") || after.starts_with('|'))
            {
                continue;
            }
            // A principle row (`| P7 |` in a document's own alignment table) cites principles.md,
            // which has its own registry; it is not a family of that document.
            if letters == "P" {
                continue;
            }
            if !prefixes.contains(&letters) {
                prefixes.push(letters);
            }
        }
        for p in prefixes {
            if !FAMILIES.iter().any(|f| f.doc == *doc && f.prefix == p) {
                missing.push(format!("{doc}: rows under prefix {p} have no Family here"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "design documents declare invariants the registry does not account for:\n  {}",
        missing.join("\n  ")
    );
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
    // Listed on the day it was written, like the documents above. The guard it promises is of a
    // different kind: it checks the viewer's source, the way the escaping guard in the viz crate
    // does, so the first test it names will live there rather than beside the engine.
    ("docs/inspector.md", include_str!("../../../docs/inspector.md")),
    // Listed the day it was written. Its guards will live in the CLI crate, where the lifecycle
    // decision is a pure function over what launchctl and the socket report.
    ("docs/daemon-lifecycle.md", include_str!("../../../docs/daemon-lifecycle.md")),
    // Listed the day it was written. Its guards span two crates - the ledger is the store's, the
    // repayment the engine's - so the store and engine tests join SOURCES as they are named.
    ("docs/crash-recovery.md", include_str!("../../../docs/crash-recovery.md")),
    // Listed the day it was written. Its guards will live in the CLI crate, beside the lifecycle's,
    // since the bridge and `connect` are CLI subcommands the desktop app calls.
    ("docs/client-connect.md", include_str!("../../../docs/client-connect.md")),
    // Listed the day it was written. Its guards will split between the CLI (profiles, principals)
    // and the MCP crate, where the remote policy table and its every-tool-is-classified test live.
    ("docs/remote-server.md", include_str!("../../../docs/remote-server.md")),
    // Listed the day it was written. Its guards will split between the viz crate, where the read
    // tier's path policy table and its every-path-is-classified test live, and the CLI (the relay).
    ("docs/remote-viewer.md", include_str!("../../../docs/remote-viewer.md")),
    // Listed the day it was written. Its guards live in the app workspace, which this test cannot
    // reach; the shell's source joins SOURCES so the test names it cites are still checked.
    ("docs/settings-page.md", include_str!("../../../docs/settings-page.md")),
    // Four documents that were never listed: the prompts (built, with a PR table), the
    // compatibility contracts, the sync-correctness defects and the store migration. None named a
    // test when listed, so the first guard any of them cites is checked from that day.
    ("docs/prompts.md", include_str!("../../../docs/prompts.md")),
    ("docs/compatibility.md", include_str!("../../../docs/compatibility.md")),
    ("docs/sync-correctness.md", include_str!("../../../docs/sync-correctness.md")),
    ("docs/store-migration.md", include_str!("../../../docs/store-migration.md")),
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
    include_str!("../../supragnosis-store/src/redb_store.rs"),
    include_str!("../../supragnosis-store/tests/port_conformance.rs"),
    include_str!("../../supragnosis-sync/src/lib.rs"),
    include_str!("../../supragnosis-sync/src/http.rs"),
    include_str!("../../supragnosis-embed/src/lib.rs"),
    include_str!("../../supragnosis-mcp/tests/mcp_surface.rs"),
    include_str!("../../supragnosis-viz/tests/http.rs"),
    include_str!("../../supragnosis-viz/src/lib.rs"),
    include_str!("../../supragnosis-viz/src/remote.rs"),
    include_str!("../../supragnosis-cli/src/main.rs"),
    include_str!("../../supragnosis-cli/src/lifecycle.rs"),
    include_str!("../../supragnosis-cli/src/bridge.rs"),
    include_str!("../../supragnosis-cli/src/connect.rs"),
    include_str!("../../supragnosis-cli/src/profile.rs"),
    include_str!("../../supragnosis-cli/src/principal.rs"),
    include_str!("../../supragnosis-cli/tests/golden_stores.rs"),
    include_str!("../../supragnosis-mcp/src/remote.rs"),
    // The desktop shell is its own workspace, but its tests are named by client-connect.md and
    // daemon-lifecycle.md like any other guard, so its source is scanned too.
    include_str!("../../../app/src/main.rs"),
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
            // The redb era: a store a real release wrote, read back whole by this build
            // (compatibility.md Section 5). Before it, every format test built its rows with the
            // structs under test, so an encoding change passed them all.
            "the_format_2_store_v0_4_7_wrote_reads_whole",
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
        c("an identity or a signature, once made, verifies under every later build",
          Evidence::Scenario(&[
            // compatibility.md Section 4: pinned for inputs no release wrote (every optional field
            // absent, and every one present), and re-verified on rows a release did write.
            "encodings_that_cross_versions_keep_their_known_answers",
            "the_format_2_store_v0_4_7_wrote_reads_whole",
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
        // Opened by remote-viewer.md: the hub serves the viewer to its principals at /viz/, and
        // enumeration, omitted and `*` workspaces, id-based reads and the event stream are all
        // resolved against the reader's grants - a union is computed per granted workspace, never
        // the node-wide projection filtered afterwards (V3, V4).
        c("an authenticated network read tier filters workspace enumeration by the reader's grants",
          Evidence::Scenario(&[
            "the_read_tier_answers_only_within_the_grants",
            "a_union_with_an_unconsented_workspace_is_refused_whole",
            "the_read_tier_streams_knowledge_not_activity",
            "every_viewer_path_has_a_remote_policy",
        ])),
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
            "incremental - the person's side has prompts now (brief, curate, review-proposal), but \
             nothing yet makes an agent observe and search during its own work unasked, so there is \
             no agent behavior to assert",
        )),
        // docs/prompts.md: curation reaches the person where they already work, as decisions
        // (open proposals, contested points) beside the reading - and the reading is never stored.
        c("curation surfaces as decisions where the person reads, never as a document to maintain",
          Evidence::Scenario(&["a_brief_is_fenced_bounded_and_writes_nothing"])),
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
        c("self-attestation is computed from the proposer and reviewer",
          Evidence::Characterized(
            &["i9_self_attested_is_blanket_true_until_principal_check_lands"],
            "M4 Phase 5 - the fold hardcodes self_attested: true, which is honest as a solo-mode \
             blanket label but will mislabel reviewed merges the moment there are two principals",
        )),
        // Split out of the clause above, where it used to hide behind the self-attestation debt.
        // The local mechanism is the surface marker read off the log (resolution.md Section 6):
        // the fold demotes a recall merge without the console marker to a comment, whichever path
        // it arrived by, and the agent surface refuses to cast one. The principal-signed act that
        // replaces marker trust under multi-principal federation stays F20's owed clause.
        c("a recall verdict is not delegable: a recall merge cast anywhere but the human console \
           decides nothing, and the agent surface refuses to cast one",
          Evidence::Scenario(&[
            "i17_the_agent_surface_refuses_a_recall_merge_and_the_log_is_unchanged",
            "i17_a_recall_merge_without_the_console_marker_never_folds_to_merged",
          ])),
    ]),
    (24, "Operational Posture", &[
        c("a subsystem that cannot come up leaves the node serving, and says what it disabled",
          Evidence::Scenario(&["config_parses_and_rejects_typos"])),
        c("the workaround reaches the operator's own surface, not only a startup log",
          Evidence::Scenario(&["a_configuration_workaround_reaches_the_operator_surface"])),
        c("a workaround narrows without asking only toward sharing less, never toward more",
          Evidence::Scenario(&[
              "a_node_is_never_its_own_peer_through_either_path",
              "a_key_in_another_spelling_is_named_and_ignored",
              // insecure_tls (retired) and plain HTTP to another machine are ignored, which can only
              // make a link fail, never send the bearer unverified (sync-correctness.md 10).
              "the_bearer_only_crosses_the_network_encrypted_to_a_verified_host",
          ])),
        c("refusal is reserved for proceeding being worse: a wrong answer, or an unauthorized surface",
          Evidence::Scenario(&[
              "bind_guard_enforces_f10",
              "an_unmigrated_store_is_refused_with_the_way_out",
              // A store a later release changed is "a store this build cannot read" in the newest
              // direction: refused unchanged rather than served with rows dropped and fields
              // stripped (compatibility.md Section 3.3).
              "a_store_a_later_release_raised_is_refused_unchanged",
          ])),
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
    for fam in FAMILIES {
        for (n, clauses) in fam.rows {
            let id = format!("{}{n}", fam.prefix);
            for cl in *clauses {
                let (tests, kind) = match &cl.evidence {
                    Evidence::Scenario(t) => (*t, "Scenario"),
                    Evidence::Characterized(t, _) => (*t, "Characterized"),
                    Evidence::Structural(_) | Evidence::Deferred(_) => continue,
                };
                assert!(
                    !tests.is_empty(),
                    "{id} files \"{}\" as {kind} but names no test - use Deferred instead",
                    cl.demands
                );
                for t in tests {
                    let why = match declares(t) {
                        Declared::Running => continue,
                        Declared::NotFound => "no such function (renamed, deleted, or a typo here)",
                        Declared::NotATest => {
                            "exists but has no #[test] attribute, so it never runs"
                        }
                        Declared::Ignored => "is #[ignore]d, so a plain `cargo test` skips it",
                    };
                    broken.push(format!("{id} \"{}\" -> {t}: {why}", cl.demands));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "declared scenario tests that are not running evidence:\n  {}",
        broken.join("\n  ")
    );
}

/// The invariant axis of the same coupling, for every family: an invariant written into a design
/// document has no evidence state until someone gives it one, and this fails until they do.
///
/// The hole this closes is not hypothetical. F21 was added to federation.md and nothing anywhere
/// objected, because the only completeness check read the principles document - and the fix was
/// then made for federation alone, while ten other documents kept invariant tables nothing read.
/// Numbers are compared as sets rather than in document order, because two tables (I17 among the
/// I rows, N11/N12 among the N rows) list an invariant out of sequence on purpose.
#[test]
fn every_invariant_declares_its_evidence() {
    for fam in FAMILIES {
        let documented = documented_rows(fam.text, fam.prefix);
        assert!(
            !documented.is_empty(),
            "parsed no {} rows out of {} - the row format changed and this registry is no longer \
             reading the document it claims to mirror",
            fam.prefix,
            fam.doc
        );
        let mut sorted = documented.clone();
        sorted.sort_unstable();
        let expected: Vec<u8> = (1..=documented.len() as u8).collect();
        assert_eq!(
            sorted, expected,
            "{} must number its {} invariants contiguously from 1 (found {:?})",
            fam.doc, fam.prefix, documented
        );
        let mut registered: Vec<u8> = fam.rows.iter().map(|(n, _)| *n).collect();
        let before = registered.len();
        registered.sort_unstable();
        registered.dedup();
        assert_eq!(
            before,
            registered.len(),
            "{}: a {} row is registered twice",
            fam.doc,
            fam.prefix
        );
        assert_eq!(
            registered, sorted,
            "{} declares {} invariants {:?}, the registry has {:?} - an invariant was added or \
             removed and has no evidence state",
            fam.doc, fam.prefix, sorted, registered
        );
        for (n, clauses) in fam.rows {
            assert!(!clauses.is_empty(), "{}{n} declares no clause", fam.prefix);
            for cl in *clauses {
                assert!(
                    cl.demands.len() > 20,
                    "{}{n}: a clause must state the demand it is evidence for, not a label: {:?}",
                    fam.prefix,
                    cl.demands
                );
            }
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
    for fam in FAMILIES {
        for (n, clauses) in fam.rows {
            let id = format!("{}{n}", fam.prefix);
            for cl in *clauses {
                let d = cl.demands;
                match &cl.evidence {
                    Evidence::Scenario(_) => {}
                    Evidence::Structural(why) => assert!(
                        why.len() > 60,
                        "{id} \"{d}\": Structural needs the mechanism that makes violation unrepresentable"
                    ),
                    Evidence::Characterized(_, why) | Evidence::Deferred(why) => {
                        assert!(
                            why.len() > 60,
                            "{id} \"{d}\": an unmet clause needs a reason and a repayment point"
                        );
                        assert!(
                            repayment_named(why),
                            "{id} \"{d}\": an unmet clause must name where it is repaid"
                        );
                    }
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

    // The same accounting for every invariant family. Reported per document rather than summed into
    // the principles, because the documents answer different questions - principles say what the
    // system must be, invariants say what one design must preserve - and one blended ratio would
    // hide that most of a family's debt is a single milestone rather than a scatter.
    let mut f_owed: Vec<String> = Vec::new();
    let (mut all_total, mut all_assured) = (0, 0);
    for fam in FAMILIES {
        let (mut f_guarded, mut f_structural, mut f_pinned, mut f_unguarded) = (0, 0, 0, 0);
        println!("\n{} - invariants ({})", fam.doc, fam.prefix);
        for (n, clauses) in fam.rows {
            println!("  {}{n}", fam.prefix);
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
                    f_owed.push(format!("{}{n:02} {}", fam.prefix, cl.demands));
                }
            }
        }
        let f_total = f_guarded + f_structural + f_pinned + f_unguarded;
        all_total += f_total;
        all_assured += f_guarded + f_structural;
        println!(
            "  {f_total} clauses over {} invariants: {f_guarded} guarded / {f_structural} structural / \
             {f_pinned} pinned-but-unmet / {f_unguarded} unmet-and-unpinned",
            fam.rows.len()
        );
    }
    println!(
        "\n{all_total} invariant clauses over {} families: {all_assured} assured / {} owed",
        FAMILIES.len(),
        all_total - all_assured
    );
    println!(
        "\nWhat the design documents promise and this build does not enforce yet ({}):",
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
