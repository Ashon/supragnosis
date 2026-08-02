# Un-merge - reversing an identity resolution (Principles 3, 15)

> The other half of `entity_merge`, specified in three documents and built in none. This fixes what
> the reversal does, what it must not become, and which of the obvious cheap versions is a trap.
>
> Status: **built.** `entity_split` is a proposal kind with blocking and informative checks, a merged
> one stops its merge forwarding, the separated pair is suppressed as a suggestion, and the act is
> reachable from the MCP `propose` tool and from an un-merge button on a committed merge in the
> console. Guarded by scenarios named in the coverage registry.
>
> Unlike [excision](excision.md), which needed a mechanism that does not exist, this needed almost
> none - the cost was in the decisions, and Section 5 is the one that bites. Sections 7 and 9 carry
> what the implementation found that this document had got wrong.

## 1. Why this exists, and why it is already owed

This is not a proposed feature. Three normative documents already require it, in these words:

- Principle 3's enforcement: *"Entity merge also preserves history - un-merge must be possible."*
- Principle 15's enforcement, on holding off rather than merging without conviction: *"A wrong merge
  is more expensive than a wrong split (though, by Principle 3, both must be reversible)."*
- Principle 23 counts the gated intents as five, and names the third **"entity merge/split"**.
  [proposal-workflow.md](proposal-workflow.md) Section 3 spells the same row out as *"entity-merge /
  split - finalizing/canceling an identity resolution"*.

The implementation shipped five kinds and dropped half of the third: `PROPOSAL_KINDS` is
`["entity_merge", "claim_promotion", "claim_demotion", "tbox_change", "recall"]`. Five names, five
intents on paper, one intent missing. `merge_forwarding`'s own doc comment already states the answer -
*"un-merge is a new proposal"* - so the shape was decided when merge was written, and only the code
is absent.

That makes this different from a feature request. The gap is between the documents and the code, and
the P23 clause "every proposal kind the surface accepts has a commit effect" is the same hole seen
from the other side.

## 2. What un-merge is NOT

Three cheaper things look like this and are not.

**It is not withdrawing or rejecting the merge proposal.** The fold derives a proposal's state from
the *presence* of a merge verdict, not the latest one: `merged` if any merge verdict exists and no
blocking check fails, and only then does it consider `withdrawn` or `rejected`. Adding a withdraw
verdict after a merge changes nothing, and that is correct rather than a bug - a proposal that
already committed cannot become one that never did. Making later verdicts win would make the state
depend on arrival order, which is the opposite of what the gate is for (P16).

**It is not deleting or editing the merge proposal.** A proposal is itself an observation (P23), and
an observation's id is its content (P14). Removing it would dangle every verdict that referenced it
and change what a replay produces. The reversal has to be a new record, which is the general form of
P3: supersede, never destroy.

**It is not a re-projection fix.** Nothing in the store is wrong. Section 3 is why.

## 3. The mechanism: an edge simply stops being drawn

`merge_forwarding` folds the log into `target -> into` edges, one per merged `entity_merge`, then
resolves them transitively with a hop cap. Nine read paths consume it and none of them consult the
merge state directly. What they do with it:

- the graph read hides any entity whose id is a key in the map,
- relation endpoints are canonicalized through it,
- alias sets union in the merged-away names,
- the merge band excludes pairs whose either side is a key in the map.

And the store port has **no delete**. `AssertionStore`/`KnowledgeStore` expose `put_entity` and
`add_relation` and nothing that removes a row. A merged-away entity is still in the store; it is
filtered at read time, every time.

So the whole of un-merge is: **stop contributing that edge**. The separated entities come back
because they never left, their relations point where they always pointed, and the aliases un-union.
This is P3's promise - *"the log keeps both"* - paying out exactly as designed, and it is why this
document is short where [excision](excision.md) is long.

One consequence to state plainly, because it is the reason the estimate is small: there is a single
chokepoint. A split that `merge_forwarding` respects is a split that all nine consumers respect,
without any of them being touched.

## 4. The unit is a resolution, not a pair

**A split names the merge proposal it reverses.** It does not name a pair of entity ids.

A merge with targets `[A, B, C]` and `into: C` contributes two edges. Naming a pair would have to
mean "one of those edges", leaving a resolution half-standing with no record of which half was
undone and no way for a later reader to ask what the resolution now claims. Naming the proposal makes
the unit of reversal identical to the unit of decision, which is what makes the audit trail read as
a sequence of whole acts.

The cost is real and worth accepting: to pull one member out of a three-way merge, split the whole
resolution and open a new merge for the two that do belong together. Two acts instead of one, each
with its own verdict and its own reviewable diff. The alternative buys one click and loses the
property that a proposal's targets describe what it did.

## 5. The candidate loop - the failure this document exists to prevent

The merge band excludes a pair when either side is already forwarded:

```rust
if other.id == e.id || score < SIM_CANDIDATE || fwd.contains_key(&other.id) { continue }
```

"Already merged" is *derived from the forwarding map*. Remove the edge and the exclusion goes with
it, so the moment a human splits `A|B` the band recognizes them as a high-similarity pair again and
suggests merging them. The console would ask the operator to undo the decision they just made, every
time they look at it, forever.

This is the whole of the risk in this feature, and the rule that answers it is:

> **A split suppresses the suggestion, permanently. It never suppresses the possibility.**

- Suppression is derived, not stored: a merged split verdict is in the log forever, so the pair it
  separated is computable on any node with the same log (P16). It is the same shape as
  `open_merge_pairs`, which already suppresses pairs awaiting a verdict, and it should live beside
  it so the two cannot drift on what "do not offer this again" means.
- The pair stays mergeable by hand. Opening an `entity_merge` on split entities is allowed and needs
  no special case - this is the distinction the band already draws, where a generator proposes and
  only a verdict commits (P19, IR2). What the split removes is the nagging, not the option.

Silence about a suppressed pair is a reporting question, not a correctness one, but Principle 5
applies: an empty `merge_suggestions` already distinguishes "no near pairs" from "this node cannot
run the band". A pair withheld because a human separated it is a third state and belongs in the
band's coverage report rather than vanishing.

## 6. Aliases come apart, and IR1 is untouched

resolution-identity.md IR1 says aliases are the log's asserted spellings minus the representative,
never dropped, never shrinking on re-projection. Merged-entity names are then unioned on top of that
set (Section 2 of that document).

On a split the other entity's names stop being aliases of the canonical one, and **this is not a
shrink of IR1's set**. They were never same-id spellings; they were the merge's contribution, and it
is the merge that went away. IR1's own union - every spelling ever asserted *of this entity* - is
exactly as large as it was.

Stating it matters because the two kinds of alias are indistinguishable in the stored row. Only the
derivation tells them apart, so an implementation that removes the wrong ones destroys asserted
spellings while looking correct.

## 7. Reversible in both directions, and what that costs P16

Principle 15 does not say a merge must be reversible. It says **both** must be. So `merge -> split ->
merge` has to work, and it does: the re-merge is a new `entity_merge` proposal, the old one stays
split, and the new one contributes its own edge.

**"New" is load-bearing, and it cost a refusal to get right.** A proposal is its content (P14), and
an opened proposal's content is its kind, its targets and its payload - so re-opening a merge with
the same targets and the same rationale produces *the same id*, which is the id the split named. The
re-proposal would be permanently dead: openable, verdictable, and unable to ever forward, with no
signal that anything was wrong. `propose` therefore refuses that case and names the fix. This is
also the honest shape - a merge made after someone split it is a different act, and it should say
why - but it is a consequence of content addressing rather than something the design chose, and it
was found by a test rather than by writing this document.

What this costs is worth being honest about. The forwarding map is **not monotonic**: appending a
split verdict removes an edge that was there before. Neither is supersede, which P3 mandates
everywhere else, and the property P16 actually requires is that the map be a deterministic function
of the log - which it is, since every verdict stays and the fold reads all of them.

This is the sharpest contrast with [excision](excision.md), and the two features sit close enough
that confusing them would be easy. A tombstone is **absorbing**: destruction is terminal, and E3 says
so. A resolution is not. Merging and splitting the same entities repeatedly is a legitimate history
of a contested identity, and the log records the argument rather than a winner.

## 8. Who may do it, and why it goes through the gate

Through the proposal gate, like every other change to canon (P23). Both the console and an agent may
open one; authority is checked at the verdict by the existing authority check, unchanged.

This is deliberately *unlike* excision, which E7 keeps off the gate entirely, and the difference is
worth writing down so neither is copied onto the other:

| | excision | un-merge |
|---|---|---|
| urgency | a destruction demand is urgent by nature | a wrong merge is wrong at leisure |
| does the reason leak? | a reason naming a secret's location re-enters it into a permanent record | a reason names two entities that are already public in the graph |
| terminal? | yes, absorbing | no, and re-merging is legitimate |
| surface | console only, non-delegable | the ordinary gate |

## 9. Checks

proposal-workflow.md Section 6 says each kind shares the state machine and differs only in its check
suite. A split's suite:

**Blocking** (referential integrity, the existing class - you cannot reverse what is not there):

- the named proposal exists in the local log,
- its kind is `entity_merge`,
- it carries a merge verdict - `merged` or `blocked`, not `open` or `rejected`. Reversing something
  that was never decided is a verdict about an act nobody committed to.

  Deliberately **not** "its state is `merged`", which is what this document said before the check
  was written. The check runs inside the fold that decides merged-versus-blocked, so depending on
  that distinction would make one proposal's state depend on another's within a single pass.
  Reversing a merge the checks are holding back is harmless anyway: it subtracts an edge that is not
  being contributed.
- Nothing refuses a second split of the same resolution. Reversal is set membership, so a second one
  is idempotent, and blocking it would need a total order over splits - more machinery, in exchange
  for making a no-op into an error.

The local console refuses the *verdict* rather than letting it land as `blocked`, which is the
existing behaviour for merges and exists to say why immediately. `blocked` is where a replicated
verdict lands, since it never passes through that path.

**Informative** (what the reviewer is owed - the P23 clause about showing blast radius):

- how many entities separate, and their names,
- how many relation endpoints move back,
- which aliases leave the canonical row - the Section 6 distinction made visible, so a reviewer can
  see that no asserted spelling is among them.

## 9a. The preview

The diff a reviewer reads is produced by running the forwarding fold with the target treated as
reversed - the same computation the verdict performs, not a prediction of it. That is the argument
`merge_diff` already makes for the other direction, and it is why the two cannot disagree: there is
one transitive resolution, parameterized by which merges count, and one comparison, parameterized by
which ids move and which way. The split preview shipped as a copy of the merge preview first, which
is worth recording: 73 identical lines in the code whose entire claim is that a preview and a verdict
cannot diverge is a fix to one becoming a divergence from the other, waiting.

It reports the relation endpoints that move back off the canonical id, and any belief the separation
overturns on an entity that is *not* one of the separating ones. The separating entities regaining
their own beliefs is the proposal rather than a consequence of it, so flagging that would bury the
surprise in the expected.

The set of separating ids is read from the difference between the two forwarding maps rather than
from the merge's target list, because a target may still be forwarded by another merge - in which
case it does not separate at all, and saying it would be a lie about the blast radius.

## 10. What the projection does afterwards

Nothing has to be patched. Reads apply forwarding on every call, so a split takes effect at the
moment its verdict lands, and the stored rows converge at the next re-materialization.

That last clause describes an asymmetry that **already exists and is not this feature's to fix**:
`review_proposal` appends the verdict observation and returns. It does not re-project, and the
incremental projection would not touch the affected entities anyway, since a verdict observation
asserts no entities. A merge is in exactly the same position today. Un-merge inherits the behaviour
rather than introducing it, and a split that quietly re-projected would leave merge as the odd one
out.

## 11. Invariants

| | Invariant |
|---|---|
| **S1** | A split is a proposal kind, subject to the same gate and state machine as every other (P23). It is not a state transition on the merge it reverses. |
| **S2** | A split names the `entity_merge` proposal it reverses, not a pair of entities. The unit of reversal equals the unit of decision. |
| **S3** | A merged split removes that merge's contribution to `merge_forwarding` and nothing else. Every consumer follows from the one map; none of them learns what a split is. |
| **S4** | Nothing is deleted or edited. The merge proposal, its verdicts, and both entity rows all stay exactly where they are - the rows were never removed, only filtered (P3, P14). |
| **S5** | A split permanently suppresses the separated pair as a *suggestion*, and never as a *possibility*. Suppression is derived from the log, so nodes with equal logs suppress equally (P16). |
| **S6** | Aliases contributed by the merge leave the canonical row; asserted spellings never do. IR1's set is unchanged by a split. |
| **S7** | Merge and split are both reversible (P15). Re-merging separated entities is an ordinary new `entity_merge` and needs no special case. |
| **S8** | A split is not absorbing. Unlike a tombstone (excision E3), it can be followed by a new merge, and the log records the whole argument. |
| **S9** | Blocking checks refuse a split whose target is absent, is not an `entity_merge`, or carries no merge verdict - a verdict must correspond to an act. A repeat split is idempotent rather than an error. |
| **S10** | A re-merge must be a distinguishable proposal. Re-opening a reversed resolution verbatim is refused, because content addressing (P14) would hand back the id the split already named. |

## 12. Closure map

| Demand | Where it is answered |
|---|---|
| P3 - merge preserves history, un-merge must be possible | Sections 2, 3; S1, S4 |
| P15 - a wrong merge and a wrong split are both reversible | Section 7; S7 |
| P23 - entity merge/split is one of the five gated intents | Sections 1, 8; S1 |
| P23 - every proposal kind the surface accepts has a commit effect | Sections 1, 3; S3 |
| P23 - the reviewer is shown the blast radius | Section 9 |
| P16 - deterministic function of the log, not of arrival order | Sections 2, 7; S5 |
| P19 / IR2 - a generator proposes, a verdict commits | Section 5; S5 |
| IR1 - a spelling is never dropped | Section 6; S6 |
| P14 - content is identity, so the proposal is not edited | Section 2; S4 |
