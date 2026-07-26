# Resolution, Part A - Belief Resolution (M3a)

> Status: **implemented** (spec agreed before the code, then implementation feedback folded back -
> the same discipline as [federation.md](federation.md)). Revision notes are marked [impl].
>
> - Normative basis: [principles.md](principles.md) Principles 1, 2, 6, 15, 16, 18, 23
>   (and the 9th revision note pointing here).
> - Scope: the **belief resolution** slice of M3 - replacing the never-chosen last-write-wins
>   projection with a deliberate, replaceable, trust-weighted deterministic policy; surfacing
>   contested beliefs (Principle 6); giving `claim_promotion` / `claim_demotion` their commit
>   effects (Principle 23); and the human-direct surface ceiling for tier promotion (Principle 18).
> - Out of scope (M3b - identity resolution): alias accumulation, the conservative merge band,
>   embedding candidate generation, entity-embedding staleness, and full write-path atomicity.
>   M3a re-routes the projection merge through the policy but keeps the `write_guard`
>   serialization; the atomic read-merge-write redesign stays with M3b, as recorded in
>   architecture.md Section 14.
> - Out of scope (M5): trust-weighted recall ranking and quarantine. M3a makes the tier decide
>   the **belief**; making it also weight **search ranking** rides with the extraction port and
>   lineage cleanup (Principle 18's remaining logic).

## 1. Problem

Three standing debts, all recorded in architecture.md Sections 12-14:

1. **The current belief policy was never chosen.** The projection is last-write-wins by replay
   order - "a default that was never chosen deliberately" (architecture.md Section 13, Open
   Decision). Entity kind is last-write-wins, `canonical_name` is first-write-wins by arrival
   (masked by HLC-ordered `reproject` after sync).
2. **Conflicts resolve silently.** Contradictory assertions converge to a winner with no signal
   (Principle 6 violation, pinned at the time by a characterization test - since rewritten into the
   guard `p6_contradictory_merge_cycle_is_convergent_and_surfaced`, which now asserts the surfacing
   this section was asking for).
3. **A human cannot commit the right direction.** `claim_promotion` / `claim_demotion` fold but
   enforce nothing, and `trust_tier` is inert - so there is no gated act by which a human review
   settles a contested belief.

The three are one design: a **trust-weighted deterministic policy** computes the belief, a
**contested signal** invites mediation where trust does not decide, and a **gated tier promotion**
is how the mediation verdict enters the log - the human's choice becomes data, not a code path
(Principle 19: the decision rule stays deterministic; the human supplies an event, not an
override).

## 2. The resolution policy port (Principle 1)

Resolution is a **replaceable strategy** behind a port in `supragnosis-core`:

- `ResolutionPolicy` - a pure function (no IO, no wall clock, no map-iteration order - Principle
  16): given the full competitor set for one single-valued projection field (each competitor =
  value + its observations' provenance + effective tier + ordering HLC + observation id), it
  returns the chosen value plus a `contested` verdict. Changing the policy and re-running
  `reproject` computes a different belief from the same log (Principle 1's replaceability, made
  real by the already-implemented reprojection).
- The engine's incremental observe path and `reproject` call the **same policy** - the two may
  still meet at re-materialization points (the 8th-revision convergence split), but they may not
  disagree about the rule.

### 2.1 Default policy: `TierWeighted`

Selection for a single-valued field, in order:

1. **Highest effective tier wins** (Section 3). Trust decides first (Principle 18: the tier is
   reflected in resolution weighting).
2. **Within the top tier band, latest ordering-HLC wins** (recency - the successor of the old
   last-write-wins, now scoped to equals-in-trust only).
3. **Final tie by observation id** (stable key - Principle 16, no arrival-order dependence).

**Confidence combining rule** (a mandatory element of the policy spec - Principle 2): confidence
does **not** participate in selection. It is self-reported and uncalibrated, so it is only a
sub-signal of the tier, and this policy chooses not to consume it; it is carried on the result
verbatim for display, and an unspecified confidence is carried as unspecified (never substituted -
4th revision). A future policy that weights confidence within a tier band replaces this section
explicitly.

### 2.2 Field scope

| Field | M3a rule |
|---|---|
| Entity `kind` | `TierWeighted` selection + contested signal (Section 4) - the headline case |
| Entity `canonical_name` (representative spelling) | `TierWeighted` selection - retires the "first-write-wins by arrival" latent condition (architecture.md Section 14). Not contested-flagged: spelling choice is display, not belief |
| Entity / relation `description` | Unchanged: HLC-latest non-empty wins (display field, allowed to converge late - federation.md Section 4). A later omission still never erases (Principle 8) |
| Relation `valid_from` / `valid_to` | Captured as today. Automatic `valid_to` closing on disproof needs negation semantics that do not exist yet - stays M3b/M5 (see Section 4.2) |

Relations are naturally multi-valued (one entity `depends_on` many things), so a mere coexistence
of relations is not a conflict; genuine relation contradiction requires explicit negative
assertions (Principle 5), which are not yet modeled. M3a deliberately does not invent them.

## 3. Effective tier - the receiver's evaluation (Principle 18, F13)

Tier-weighted selection makes the tier load-bearing, which changes what the F13 debt (claimed
tier stored verbatim) can damage: a self-declared tier would no longer just mislabel a node's
display - it would flip the belief. So the policy never reads the claimed tier directly; it reads
an **effective tier** the receiving node computes:

```
effective_tier(observation) =
  if any merged gate event targets it: tier set by the HLC-latest merged
      claim_promotion / claim_demotion verdict (Section 5), capped by the
      verdict's surface ceiling (Section 6)
  else: max over attestations of per-attestation evaluated tier, where
      - a local attestation (no sync stamp, authored by this engine):
        the stored tier (the engine forces the default at observe -
        a local writer cannot self-declare above AgentExtracted)
      - a synced attestation (sync stamp present, signature verified):
        min(claimed tier, HostSigned) - a signature proves origin,
        never a human act, so a remote claim can never evaluate to
        HumanConfirmed by itself (federation.md 6b)
      - a synced attestation that fails verification never lands (F6),
        so it has no tier to evaluate
```

Notes:

- The claimed tier stays **stored verbatim** in the log (audit, F13) - evaluation is a read-time
  computation, never a rewrite (Principle 3).
- A gate event **overrides** the base evaluation in both directions: a merged demotion can push
  an observation below its base tier (contamination response - the fast-path of
  proposal-workflow.md Section 9), which a max-only formula could not express.
- The gate-event fold is a fold-projection (converges continuously - F5); the per-attestation
  evaluation is a pure function of the stored event. Both are deterministic, so effective tier
  converges across nodes on the same log and canon policy.
- The projection's **representative (display) tier switches to effective tier** at the same
  moment - retiring the max-over-claimed-tiers computation that architecture.md Section 14 and
  federation.md 6b flag as the F13 exposure. This repays the overdue "receiver does not
  re-evaluate trust_tier" entry condition for the read path; the apply path continues to store
  verbatim by design.

## 4. Contested beliefs (Principle 6)

### 4.1 Definition

A single-valued field is **contested** when distinct values survive from distinct observations
whose effective tiers tie at the top - i.e. the winner was decided by recency/id alone, not by
trust. When a strictly higher tier decided the winner, the conflict is *resolved by trust*: it is
not flagged on the projection, but it remains listed in the curation report (history is
queryable - Principle 6 preserves the existence of the contradiction either way).

This split is the review-economics line (proposal-workflow.md Section 9, Appendix A of
principles.md): tier-tied conflicts are exactly the points where the system has no ground to
choose, so they invite mediation; tier-decided conflicts are informational.

### 4.2 Surfacing

- `GraphNode` / `EntityView` gain `contested: bool` plus the competitor list (value, effective
  tier, provenance, observation id) for contested fields. Query responses can therefore always
  answer "what else was asserted and by whom" (Principle 2).
- The curation report (`/api/curation`, `workspace_map` neighborhood) gains a `contradictions`
  section: all live conflicts, tier-tied first. This is the Principle 6 introspection query -
  read-only, commits nothing (I18).
- The P6 characterization test flips to a guard: the same cycle now converges **and** is
  surfaced. Convergence properties (random order / partition / duplication) must hold for the
  contested flag too - contested-ness is part of the projection, so it is part of the P16
  obligation.
- [impl] **Contradictory merge cycles are surfaced too**: accepted entity-merge proposals whose
  effects fold into each other (the P6 characterization scenario) are reported as a separate
  curation signal (`merge_cycles`: member entities + the forming proposal ids). The projection
  still resolves the cycle deterministically (hop-capped parity, P16) - the signal makes the
  contradiction visible, and the remedy is a settling proposal, never an edit (P3/P23).

Mediation itself is not a new mechanism: a human (or an agent proposing to a human) opens a
`claim_promotion` for the side they judge right - or loads a counter-assertion first if the
right side is not yet in the log (Principle 5: negation/correction is an explicit assertion) -
and the verdict resolves the tie through Section 3. Rejecting the losing side is optional
(rejection is not negation - I5); demoting it (`claim_demotion`) is the stronger act and remains
fast-path.

## 5. Gate effects: claim_promotion / claim_demotion (Principle 23)

The two kinds gain their commit effect, mirroring how `entity_merge` already works (a
fold-derived projection effect, no new storage):

- **Payload**: target observation ids + requested tier (promotion) or demoted tier (demotion).
  Well-formedness at `propose`: targets non-empty and present in the local log (the referential
  integrity blocking check of proposal-workflow.md Section 6, applied at open time as
  capture-side validation; the fold re-checks at verdict time), requested tier a valid
  `TrustTier`.
- **Effect**: a merged verdict sets the targets' gate tier (Section 3). The effect is derived by
  the fold from the verdict (I2/I6 - no separate `tier_promoted` event is required for
  correctness; one may be emitted later as a UX record, per proposal-workflow.md 7.2).
- **Ordering**: multiple merged gate events on one target order by verdict HLC; the latest
  governs (a fold-projection, continuous convergence). Merged stays absorbing per proposal
  (I16); a reversal is a new proposal (demotion), which is exactly the HLC-later gate event.
- **Self-approval**: unchanged from the current solo mode - I9 enforcement (principal
  comparison) remains M4 Phase 5, and `self_attested` remains the blanket solo marker until
  then. The I9 exception (demotion permits self-approval) costs nothing to honor now since
  no principal check exists yet; the spec point is that when Phase 5 lands, demotion keeps the
  exception while promotion does not.
- **Recall stays inert** in M3a: its effect (bulk lineage retraction) belongs to M5 (sanitize),
  and its I17 human-direct enforcement lands with it. The Section 6 surface ceiling is designed
  to be the same mechanism I17 will use.

## 6. The human-direct surface ceiling (Principle 18, I17-analog)

Promotion to `HumanConfirmed` is "a human's direct act" and can never be delegated to a machine
(principles.md P18, federation.md F17/F20). Today the log cannot distinguish one: the MCP
`review` tool is agent-callable with a free-string `on_behalf_of`, and the viewer casts verdicts
with no principal. Without MCP elicitation (unimplemented, Principle 21), the local deployment
enforces the distinction by **surface**:

- `Engine::review_proposal` gains a `surface` parameter set by the **caller crate**, never by
  the remote client: `Console` (the viz unix-socket console - reachable only by the local OS
  principal, 0600) or `Agent` (the MCP tool path). The engine stamps the verdict observation's
  provenance with the surface marker (an engine-controlled `source_ref`); neither MCP clients
  nor HTTP bodies can supply it.
  [impl] The whole `surface:` source_ref namespace is additionally **reserved at every local
  ingest door**: observe, define_type and propose (and reify, which routes through observe)
  refuse a client-supplied source_ref under the prefix. The ceiling fold only reads the marker
  off verdict events today, but the reservation means a log-borne marker on a locally-authored
  observation is engine-stamped by construction - the fold's trust does not rest on no future
  surface ever reading a marker off a non-verdict observation. Sync apply is deliberately not
  guarded: a replicated verdict legitimately carries its marker (that is how the ceiling
  converges), and honoring it stays the single-principal premise below.
- **Ceiling rule (fold-side, deterministic)**: the tier a merged promotion may grant is capped
  by the ceiling of its verdict's surface marker - `Console` grants up to `HumanConfirmed`;
  `Agent`, an absent marker, and any unknown marker cap at `HostSigned`. The cap is applied by
  the fold reading the marker from the log - not by the write path - so it is a deterministic
  function of the log and converges (I2, P16, F15-consistent).
  [impl] The ceiling is a function of the **marker alone**, deliberately: making it depend on
  whether the verdict attestation arrived locally or via sync would make the effective tier
  differ per node over the same log - a P16 violation in a fold that now feeds the projection.
  The consequence is that a replicated console marker is honored on the receiver, which is
  sound exactly under the single-principal federation premise (the only supported deployment,
  F18/Phase 6) - a "malicious peer minting console markers" is outside that premise, and
  Phase 5's principal-signed acts replace marker trust for multi-principal federation.
- Demotion needs no ceiling (lowering trust is the low-risk direction - the fast-path
  rationale).
- Phase 5 composes, not replaces: a principal-signed act (F20 strength ii) becomes a second,
  cryptographic way to satisfy the `HumanConfirmed` ceiling, and the canon policy decides which
  acts demand it. The surface marker remains the local-trust fallback exactly as the solo
  self-attested marker does.

Honesty note: within the local trust surface, "Console = human" rests on the OS access control
of the unix socket (the same argument the viewer already stands on - architecture.md Section
10). A local agent with shell access as the owning user could curl the socket; that party
already owns the store file outright, so this draws no new boundary - the ceiling's job is to
stop the *protocol-level* path (a well-behaved MCP client cannot mint `HumanConfirmed`), not to
defend against the local user's own processes.

## 7. Determinism and convergence (Principle 16)

- The policy is a pure function; effective tier and gate folds are log-derived; the surface
  marker is log-borne. No new nondeterminism enters the projection.
- Convergence points are unchanged (8th revision): fold-projections (gate tiers, contested
  lists, curation contradictions) converge continuously; the materialized entity/relation
  tables converge at re-materialization. The incremental path and `reproject` share the policy,
  so the transient divergence window is the same one that exists today.
- Property obligations extended: the random-order / partition / duplication convergence tests
  must assert equality of chosen values, contested flags, and effective tiers - not just graph
  shape. The P6 characterization test flips to guard (Section 4.2); the F13 characterization
  test (`f13_sync_apply_stores_senders_self_declared_tier_verbatim`) stays true for the log
  layer and gains a guard sibling asserting the **evaluation** caps a remote claimed
  `HumanConfirmed` at `HostSigned`.

## 8. Invariants (R1..)

- **R1** The belief is computed by a replaceable pure policy; no projection write encodes a
  policy decision the policy did not make. Changing the policy + `reproject` recomputes the
  belief from the unchanged log (P1).
- **R2** Selection order of the default policy: effective tier, then ordering HLC, then
  observation id. No wall clock, no arrival order (P16).
- **R3** Confidence never selects; it is carried verbatim, unspecified stays unspecified (P2).
- **R4** The policy consumes only **effective** tiers. A claimed tier from the wire can never
  evaluate above `HostSigned`; only a gate event under a `Console` ceiling (or, from Phase 5, a
  principal-signed act) reaches `HumanConfirmed` (P18, F13).
- **R5** A gate event overrides the base evaluation in both directions; the HLC-latest merged
  gate event per target governs (P23, fast-path demotion).
- **R6** Contested = distinct surviving values whose effective tiers tie at the top. Contested
  status is part of the projection and subject to the P16 convergence obligation (P6).
- **R7** Conflicts are never suppressed: tier-resolved conflicts remain queryable in the
  curation report; the defeated assertion remains in the log and is reinstated by re-resolution
  if the tier landscape changes (P3, P6).
- **R8** The surface marker on a verdict is engine-stamped, never client-supplied; the ceiling
  is applied by the fold from the log, never by the write path (I2, P16).
- **R9** Mediation enters as events (counter-assertion, proposal, verdict) - there is no API by
  which a human edits the belief directly (P1, P19, P23 no-bypass).

## 9. Test plan

Every row names the test as it is actually spelled in the tree, and the Status column is what makes
that claim checkable: `principle_coverage.rs` sweeps this table (and the rest of these documents)
and fails if a `landed` row names something that is not a running test. A row may name a test that
does not exist yet only by putting its milestone in Status. This table used to name five tests that
had been renamed out from under it, which is the failure mode the sweep exists to end - a
"guarded by \<test\>" claim with nothing behind it is what CI was added to stop.

| Test | Kind | Pins | Status |
|---|---|---|---|
| `tier_weighted_selection_order` | guard | R2 - tier beats recency, recency beats id, id is final | landed |
| `contested_iff_top_tier_ties` | guard | R6 - higher tier resolves silently-in-projection, tie flags | landed |
| `p6_contradictory_merge_cycle_is_convergent_and_surfaced` | guard | converges AND reports contested (was a characterization test, rewritten in M3a) | landed |
| `evaluated_tier_caps_remote_claimed` / `f13_read_path_evaluates_remote_claim_at_host_signed` | guard | R4 - a remote `HumanConfirmed` claim evaluates to `HostSigned`, at the policy and again at the read surface. Sibling of `f13_sync_apply_stores_senders_self_declared_tier_verbatim`, which keeps pinning that the log stores the claim verbatim | landed |
| `p6_kind_conflict_surfaces_contested_and_console_confirm_settles_it` / `p23_demotion_overrides_below_base` | guard | R5 - a merged promotion sets the gate tier the policy consumes; a merged demotion overrides below base | landed |
| `verdict_ceiling_by_surface_marker` / `p18_agent_surface_promotion_caps_at_host_signed` | guard | R8 - Agent-surface merge of a `HumanConfirmed` request grants `HostSigned`, checked at the marker and end to end | landed |
| `cross_node_reprojection_converges` | property | R6/R7 + Section 7 - the compared graph is serialized whole, so belief values, contested flags and effective tiers are inside the equality | landed |
| `p16_canonical_name_selection_is_arrival_order_free` | guard | retires the latent condition (Section 2.2) | landed |

## 10. Ledger effects (what this slice repays)

[impl] Implemented - the corresponding architecture.md Section 14 updates are in effect:

- Principle 1/6 deferral: the "swappable resolution policy" and conflict surfacing land;
  multi-attestation accumulation for relations and alias rules stay with M3b.
- Open Decision "current belief policy": **decided** - `TierWeighted` as specified here,
  replaceable per R1.
- Overdue entry condition 2 (receiver tier re-evaluation): repaid **for the read path**
  (effective tier + representative-tier switch); apply-path verbatim storage is by-design (F13)
  and Phase 5 owns the remaining canon-policy evaluation.
- Latent condition "canonical_name deterministic": repaid.
- P18 logic: partially repaid (tier now decides resolution and display); recall ranking,
  quarantine, and lineage cleanup remain M5.
- Principle 23 enforcement: `claim_promotion` / `claim_demotion` join `entity_merge`;
  `tbox_change` (Phase 5) and `recall` (M5) remain.

## 11. Closure map

| Mechanism | Grounding |
|---|---|
| Observation log, `absorb`, HLC, `reproject`, proposal fold, `entity_merge` effect | existing code |
| Curation report / console, gated `/api/review` | existing code (M3.5) |
| `ResolutionPolicy` port + `TierWeighted`, effective tier, contested projection, gate effects, surface ceiling | **this spec (M3a)** |
| Alias accumulation, merge band, embedding candidates, atomic write path | M3b (architecture.md Section 12) |
| Negative assertions / automatic `valid_to` closing / time-travel queries | M3b/M5 per architecture.md (bitemporal logic) |
| Recall effect + I17 enforcement, quarantine, lineage cleanup, recall ranking | M5 |
| I9 principal comparison, canon policy, principal-signed acts, `tbox_change` gate | M4 Phase 5 (federation.md) |
| MCP elicitation (protocol-level human confirmation) | Principle 21 remainder (architecture.md Section 7) |
