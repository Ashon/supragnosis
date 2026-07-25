# Resolution, Part B - Identity Resolution (M3b)

> Status: design spec (M3b Phase 0), agreed before the code - the same discipline as
> [resolution.md](resolution.md) (M3a) and [federation.md](federation.md). Implementation feedback
> will be folded back as [impl] notes.
>
> - Normative basis: [principles.md](principles.md) Principles 3, 9, 11, 14, 15, 16, 18, 19, 23.
> - Scope: the **identity** half of M3 - alias accumulation, the conservative merge band with
>   embedding candidates, the resolution write path (absorbing `write_guard`), keyword-search alias
>   parity, entity-embedding staleness, T-Box conflict surfacing, and induced type candidates.
> - Out of scope: bitemporal **query logic** (`as_of_valid`/`as_of_recorded`, automatic `valid_to`
>   closing) - split off as **M3c** (Section 8): it is read-surface work orthogonal to identity, and
>   bundling it would hold shippable identity work hostage to negation-semantics design. Capture is
>   already complete (M1/M4); only processing is deferred, so no information is lost (Principle 4's
>   capture/processing separation). The auto-merge **executor** (top-band automatic entity_merge)
>   stays with the M4+ policy layer (I15) - M3b generates candidates, it never casts verdicts.

## 1. Problem

Entity identity is still M0: exact canonical-name match. The standing debts (architecture.md
Sections 12-14):

1. **Aliases never accumulate.** Spelling variants of one entity converge to one id (case/trim
   normalization), and since M3a the representative spelling is the policy's choice - but the
   losing spellings vanish from the projection instead of becoming aliases. Distinct names for the
   same subject ("Cozo" / "CozoDB") produce two entities that nothing ever links.
2. **No merge band.** The only resolution across distinct names is the human-adjudicated
   `entity_merge` proposal; embedding similarity - the port exists - generates no candidates.
   Principle 15 says resolution is the substrate's job; today it is the operator's.
3. **The write path is provisional.** The entity upsert is read-merge-write under a coarse
   `write_guard` mutex, and the incremental projection applies arrival-order interim rules that
   only `reproject` corrects (the F5 transient window).
4. **Latent conditions come due with aliases** (architecture.md Section 14): Cozo keyword search
   does not match aliases (only InMemory does - latent while aliases are empty), and the entity
   embedding is computed once when absent, so alias growth would leave it silently stale.
5. **T-Box conflicts resolve silently.** The type glossary is HLC-last-write-wins per (target,
   name) with no conflict signal - the same silence M3a removed for entity kinds.

## 2. Alias accumulation (Principle 3: nothing vanishes from the view)

- **Same-id spellings become aliases.** During projection (write path, Section 4; `reproject`
  identically), every distinct asserted spelling of an entity accumulates into `aliases`; the
  policy-selected representative spelling stays `canonical_name` (M3a rule, unchanged). Aliases
  are a **deterministic set union** ordered by (first-asserting ordering-HLC, spelling) and never
  shrink on re-projection - append-only in effect, converging because the set is a pure fold of
  the log (P16).
- **Merged-entity names join the alias set.** An accepted `entity_merge` already lists the folded
  entities' names as view-level aliases in the graph projection; the write path now materializes
  them on the canonical row too, so search and `get_entity` see them without the graph fold.
- **IR1**: aliases = the log's asserted spellings minus the representative; a spelling is never
  dropped, never duplicated, and the set is identical on nodes with equal logs.

## 3. The conservative merge band (Principle 15/19)

Embedding similarity **generates candidates; only the gate commits.**

- **Candidate generation** (deterministic given the node's index - a recall aid, P16-exempt but
  guarded): for each entity, nearest entities by embedding similarity above a **candidate floor**
  (`SIM_CANDIDATE`, initial 0.85) that are not already the same id, not already merged, and not
  already covered by an open entity_merge proposal. Emitted as a new curation-report section
  (`merge_suggestions`: pair + similarity + shared-neighbor count), read-only (I18).
- **No auto-merge in M3b.** The confident band ("top band automatic") requires the M4+ policy
  executor whose routing premises the fold re-validates (I15). Until then every suggestion is a
  human decision in the console / an agent-opened proposal - both through the existing gate (P23).
- **Recall-aid guard** (Principle 16, 4th revision): similarity scores are node-local; a suggestion
  is labeled as coming from the recall aid, and the committed merge is the verdict, which is
  log-derived and converges. Two nodes may see different *suggestions*; they commit the same
  *merges*.
- **IR2**: no code path turns a similarity score into a merge without a verdict observation.

## 4. The resolution write path (absorbing `write_guard`)

The M0 upsert (`upsert_named`) is replaced by a resolution-layer write that applies the SAME rules
as `reproject`, incrementally:

- On observe, the affected entities' rows are recomputed **from their candidate folds** (kind,
  representative spelling, aliases - the M3a policy + Section 2), not by field-wise last-write
  overwrites. The write stays serialized per entity (the guard narrows from engine-global to
  per-entity keying, or is replaced by a store-level atomic upsert where the adapter provides one);
  the read path stays lock-free.
- This closes the *local* half of the F5 transient window: an incremental write and a reprojection
  now produce the same row for the same log. The *cross-node* window (stamps arriving) still
  converges at re-materialization, as specified (8th revision).
- **IR3**: for any log state, the incremental projection of the last write equals the row
  `reproject` would produce - pinned by a property test that interleaves observes and compares
  against a fresh replay.

## 5. Latent conditions repaid with aliases (architecture.md Section 14)

- **Keyword-search alias parity**: the Cozo adapter matches aliases exactly as InMemory does; a
  cross-adapter parity test moves from latent to guard.
- **Entity-embedding staleness**: the embedding text is `canonical_name + aliases` (existing
  `entity_text`); when the alias set or representative spelling changes, the embedding is
  recomputed best-effort (failure degrades, P19 - never blocks the write). **IR4**: the stored
  embedding always corresponds to the current embedding text or is absent - never silently stale.

## 6. T-Box conflict surfacing (Principle 9 vs 6, reusing M3a machinery)

- The types fold gains the M3a contested treatment: distinct **descriptions** for one (target,
  name) at a tied top effective tier -> `contested` on the glossary entry, with competitors and
  their asserting observations; a `claim_promotion` on the chosen defining observation settles it
  (the same mediation path as kinds - no new mechanism).
- **Structural checks stay minimal** because no subtype hierarchy exists yet (Principle 13 has
  nothing to bite on): the one implementable P9 check - one name defined on BOTH the entity and
  relation axes - surfaces as a curation signal (informative, not blocking: an axis collision is
  legal but usually a mistake). Cyclic-subtype / domain-range checks arrive with subtyping.
- **IR5**: type-definition conflicts are surfaced through the same contested/competitor shape as
  entity kinds - one UI, one mediation act.

## 7. Induced type candidates (Principle 11 - the substrate finally feeds the gate)

- A deterministic pass over hyperedges emits **tbox_change candidate proposals** for repeated,
  cohesive co-occurrence patterns: a member set that recurs across `INDUCE_MIN_SOURCES`
  (initial 3) observations from **independent principals** (delegation-chain principal, P2/P18 -
  self-corroboration does not count) whose members share no defined type. The candidate carries
  `derived_from` = the co-asserting observations, enters at the lowest trust, and is a **proposal**
  - promotion stays the explicit `define_type`-through-gate act (P11: induction goes as far as a
  proposal).
- Candidate *generation* is a pure function of the log (converges, P16); *surfacing order* is
  curation UX (heuristics permitted).
- **IR6**: an induced candidate is always lineage-bearing + lowest-trust + gated; hyperedges
  remain a reference, never a judge.
- [impl, landed early] The **reify** half of the promotion story shipped ahead of this milestone:
  `Engine::reify_hyperedge` / the viewer's `/api/reify` assert a recurring context as a group
  entity + `member_of` relations through the normal observe ingest, `derived_from` naming every
  co-asserting observation. Induced *type* candidates (this section) remain M3b: reify makes a
  context an A-Box entity; induction proposes T-Box vocabulary.

## 8. M3c - bitemporal query logic (split out, not dropped)

`as_of_valid(T)` / `as_of_recorded(T)` and automatic `valid_to` closing on disproof need negation
semantics (what counts as a disproving assertion - P5's explicit negative assertion is not yet
modeled) and a query-surface design of their own. Splitting them keeps this ledger honest: the M3
roadmap entry stays open until M3c ships, and the capture side (already complete) guarantees the
deferral is non-destructive. M3c is scheduled after M3b, before M5.

## 9. Test plan

| Test | Kind | Pins |
|---|---|---|
| `aliases_accumulate_and_converge` | guard | IR1 - set union, order-free, representative excluded |
| `merge_suggestions_never_commit` | guard | IR2 - no verdict, no projection change from suggestions |
| `incremental_write_equals_replay` | property | IR3 - interleaved observes vs fresh reproject |
| `cozo_keyword_matches_aliases` | guard (parity) | Section 5 - latent condition retired |
| `embedding_recomputed_on_alias_change` | guard | IR4 |
| `type_def_conflict_surfaces_contested` | guard | IR5 - glossary contested + mediation settles |
| `induced_candidates_are_gated_lineage_bearing` | guard | IR6 - proposal + derived_from + lowest trust |
| Convergence property tests (extended) | property | alias sets + glossary contested included in equality |

## 10. Closure map

| Mechanism | Grounding |
|---|---|
| Policy port, effective tier, gate effects, contested machinery, surface ceiling | existing (M3a, resolution.md) |
| Hyperedge projection, curation report/console, proposal gate, `entity_merge` effect | existing (M3.5/M4) |
| `EmbeddingProvider` port, `entity_text`, HNSW search | existing (M2) |
| Alias accumulation, merge band, resolution write path, alias parity, embedding staleness, T-Box contested, induced candidates | **this spec (M3b)** |
| Bitemporal query logic + negation semantics | M3c (Section 8) |
| Auto-merge executor (top band, I15 re-validation) | M4+ policy layer (proposal-workflow.md Section 9) |
| Quarantine, lineage cleanup, recall effect, trust-weighted recall ranking | M5 |
