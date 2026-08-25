# Consolidation - forgetting as demotion, tidying as re-projection (Principle 7)

> How a workspace stays usable as it grows, without anything being deleted. This document fixes what
> a consolidation pass may compute, what may act on the result, and why the obvious cheaper version -
> a mutable score column that a background job writes - is worse than not having it.
>
> Status: **specification, with Section 8 step 1 landed.** The weight is computed and reported as
> `demotion_candidates` on the curation report; nothing consumes it, so nothing is demoted and no
> clause here is met. The ledger owns the rest as M6 ([architecture.md](architecture.md) Section 12).
> The generate half - the read-only curation signals - landed early with M3.5 and this document
> builds on it rather than replacing it.
>
> Sections 4.2 and 8 carry corrections that building step 1 forced. They are recorded in place
> rather than edited away, because the reasoning that was wrong is the part worth reading.

## 1. The one mechanism three clauses are waiting on

The coverage registry files three separate clauses as unmet, filed under three different principles:

| Principle | Unmet clause |
|---|---|
| P7 | forgetting happens as recall demotion at idle, never as deletion |
| P18 | derived assertions without lineage are quarantined, **recall is trust-weighted**, and contamination can be traced back and cleaned |
| P23 | every proposal kind the surface accepts has a commit effect (`recall` folds correctly and changes nothing) |

They read as three debts. They are one, and the reason is visible in eleven lines of `fuse_rrf`:

```rust
let contrib = 1.0 / (K + rank as f32 + 1.0);
```

Reciprocal Rank Fusion combines the keyword and semantic lists by **rank position alone**. There is
no per-item term in that sum, so there is nowhere for a weight to enter. Trust cannot tilt recall
because recall takes no tilt. Demotion has no handle to turn. A `recall` verdict has nothing to
change even when the gate merges it.

So the missing thing is not three features. It is one: **a per-item recall weight**, and the three
clauses are what it would repay.

## 2. What this is NOT

**Not excision.** [excision.md](excision.md) covers the one act that destroys. Demotion removes
nothing and reaches nothing on disk; a demoted observation answers an explicit query exactly as it
did before. The two documents share a lineage walk and nothing else.

**Not `claim_demotion`.** That kind exists and has a commit effect, and it moves the trust tier -
which decides *which assertion becomes the current belief*. Recall weight decides *what surfaces
when nobody named it*. A claim can be the winning belief and still be worth demoting out of a
crowded search result; a claim can be demoted below its rivals and still be the thing a direct
lookup must return. P7 draws this line itself: forgetting "only adjusts weights in the
projection/index layer", never the belief.

**Not a garbage collector.** The log is eternal (P3). Nothing in this document shrinks it, and any
future version that does has left this document for excision.md.

## 3. The recall weight

A **recall weight** is a number attached to a candidate at ranking time, multiplying its RRF
contribution. It is not stored, not authoritative, and not a belief.

Where it lives is already decided by precedent. [resolution.md](resolution.md) established that the
current belief is **policy-current on the read path and materialized at `reproject`** - a fold over
the log, recomputed rather than written. The recall weight is the same shape and must reuse that
split rather than invent a second one: a mutable score column that a background job writes is a
projection write that no observation asserts, which is the P1 violation the store port was split to
make unrepresentable (`AssertionStore` / `KnowledgeStore`).

So: **the weight is a fold, never a column.** A node that has never run a consolidation pass and a
node that has run a hundred compute the same weight from the same log.

## 4. What feeds it, and the convergence decision

This is the sharp part, and getting it wrong breaks P16 rather than merely underperforming.

P7 names three inputs: "recency, usage frequency, trust tier". Two of the three are traps.

### 4.1 Usage frequency is not in the log

How often a node's operator searched for something is not an observation. It is node-local telemetry,
and no other node can derive it. A weight that consumes usage cannot converge, and P16's duty of
convergence binds the deterministic read surfaces - exact lookup, graph traversal, **keyword search**.

The embedding exemption does not extend here. P16 grants it to an ANN index because such an index
"is not part of the materialized graph but a node-local recall aid". A weight that demotes knowledge
is a statement *about the knowledge*, and if two nodes holding the same log return keyword results in
different orders, the convergence surface has stopped converging.

**Decision: the committed weight excludes usage.** Usage may tune a node-local re-rank layered
*after* the converged ordering, and only on the surfaces already exempt - the same place embeddings
live, reported by the same `mode` field that already tells a client which surface answered. A client
that needs the convergent answer asks for the convergent surface and gets an ordering every node
agrees on.

### 4.2 Recency cannot mean "now"

"Older knowledge ranks lower" needs an age, and age needs a present moment. Wall clock in a fold is
exactly what P16 forbids: "No use of nondeterminism (wall clock, arrival order, random numbers) in
projection/resolution logic."

The fix is to stop asking the machine what time it is and ask the log instead. Age is a **position
in the time span the workspace's own log covers** - 0.0 at its oldest recorded HLC, 1.0 at its
newest

> **Corrected twice while building it, and both corrections matter.** This section first said a
> *rank* in the ordering, and rejected arithmetic on the HLC's `wall` value as "smuggling physical
> time into the fold". The reason was wrong: reading the OS clock is nondeterministic and forbidden,
> while comparing two *recorded* HLC values is comparing two data points and is perfectly
> deterministic.
>
> With that objection gone the two readings could be judged on their merits, and the rank loses. A
> rank **invents age differences that did not happen**: a thousand rows written inside one hour are
> spread by rank across the whole range, so the first and the five-hundredth are treated as far
> apart when they are seconds apart. For a signal whose entire job is to say how stale something is,
> manufacturing staleness is the one error that matters.
>
> The span also costs two numbers per workspace where a rank costs a position per observation, and
> that is what makes step 2 possible at all: **the log is immutable, so a per-observation value has
> nowhere to be written** (P3 - it cannot ride on the observation rows), and carrying it elsewhere
> means a parallel table for a derived number, which is a port change to store something no
> observation asserts. Two scalars need neither.
>
> Guarded by `p7_observations_written_at_one_instant_share_one_frontier_position`, which fails under
> the rank reading.

: an observation's position in the ordering the log already
carries, normalized by the newest HLC in that workspace. Two nodes holding the same observations
compute the same frontier and therefore the same recency, and the number moves when knowledge
arrives rather than when time passes - which is also the more honest reading of "stale", since a
workspace nobody has written to in a year has not become less current about its subject.

### 4.3 The inputs, then

Fold-derived, converging, part of the weight:

| Input | Source | Direction |
|---|---|---|
| effective tier | already computed (resolution.md) | higher tier, higher weight |
| HLC frontier rank | the log's own ordering (4.2) | nearer the frontier, higher weight |
| retraction status | the `recall` fold (Section 6) | retracted floors the weight |
| lineage depth without corroboration | `derived_from` + attestation count | derived-and-alone, lower weight |
| structural integration | the orphan signal already in `CurationReport` | unconnected, lower weight |
| supersession | later observations asserting the same subject | superseded, lower weight |

Node-local, exempt, layered after and labelled:

| Input | Why it cannot be in the fold |
|---|---|
| usage frequency, last access | not an observation; no peer can derive it (4.1) |
| embedding similarity | already exempt and already labelled (P16 4th revision) |

The weight floors rather than zeroes. **A weight of zero would be deletion by arithmetic** - an item
that can never surface is an item that is gone for every purpose except an audit nobody runs. P7
requires demoted knowledge to stay "reachable by an explicit query", so the floor is a positive
constant and `get_entity` / `get_observation` / `traverse` do not consult the weight at all. Only the
ranked surfaces do, because only they have to choose what to leave out.

## 5. The consolidation pass

The generate half already exists and is richer than the milestone assumed: `CurationReport` computes
duplicates, grab-bags, orphans, contradictions, merge cycles, merge suggestions, name variants,
credential findings and T-Box axis collisions, all read-only, committing nothing. P7's 7th revision -
"consolidation generates, it does not commit" - is already honored there, and this document does not
loosen it.

What M6 adds is not another signal. It is the answer to *what happens next*, and there are exactly
two answers:

**Automatic, because it is not a canon change.** Recomputing the recall weight commits nothing that a
reprojection would not reproduce. It writes no observation, changes no belief, and removes no
reachability. Putting a gate in front of it would gate arithmetic. It runs off the critical path
(P7), and "runs" means "is recomputed", not "is written".

**Gated, because it is.** A `recall` (retraction) is a belief change and already has a proposal kind,
a non-delegable verdict (P23 I17) and a blast radius that lineage defines. Section 6.

The boundary is therefore sharp and needs no new machinery to police: **if the act is recomputable
from the log alone, it is automatic; if it appends an observation, it goes through the gate.** Every
existing signal is on the first side today, which is why the console can surface them without a
verdict, and `recall` is the only one that crosses.

## 6. The recall verdict's commit effect

`recall` is an accepted proposal kind whose fold is correct and whose effect is absent. A reviewer
can merge one today and nothing happens - which is worse than refusing the kind, because the gate
reports a decision it did not carry out.

A merged `recall` verdict, over its target and the `derived_from` closure the proposal named:

1. **marks them retracted** - a fold-derived status, like every other verdict effect;
2. **removes them from belief selection** - a retracted assertion is not a candidate, so the belief
   recomputes to whatever else asserts the subject, or to nothing (P5: absence, not falsehood);
3. **floors their recall weight** (Section 4);
4. **deletes nothing** (P3) - the observations stay, `get_observation` still returns them, and the
   retraction is itself an observation, so it converges (P16) and is reversible by a new proposal.

Point 4 is what separates this from excision, and the separation must survive contact with the
obvious request. When someone asks for a recall because a credential leaked, retraction is the wrong
tool and the honest answer is that excision is unbuilt - reporting a removal that did not happen is
E9 in the other document, and it is the same failure here.

The closure is **presented, not cascaded**: the proposal names what it would retract and the belief
diff shows it, which is the existing "no merge without a diff" rule (P23) applied to a kind that has
never exercised it. A recall whose diff was computed over a closure that has since grown must not
silently widen - it binds to the base it reviewed, which is the same Stale-base debt P23 already
carries, and this kind is where it starts to bite.

## 7. Condensation, and the layer it belongs to

Demotion answers "what surfaces". The question next to it is "how many things surface as one" -
folding a body of observations into a single condensed statement. It is the same layer and a
different act, and it splits in two.

### 7.1 Structural condensation is already here

A set of entities co-asserted by one observation is a hyperedge, and `workspace_map` recovers from
the log the context that the binary-relation projection discarded (P11). `reify_hyperedge` promotes
a recurring one into a group entity plus `member_of` relations through the ordinary gated ingest.
That is condensation - N observations become one named thing - and it is deterministic, convergent,
recomputable and reversible, because the hyperedge stays a derived view and only the asserted
grouping becomes first-class.

What is missing is not the mechanism but the **selection**: which recurring contexts have earned a
name. That is a deterministic fold over stability, corroboration and cohesion, and P11 already fixes
its counting rule - corroboration counts **independent sources by delegation-chain principal**,
never raw repetition, because a signal that cannot be verified against self-declaration must not be
bought with volume.

Nothing in this half needs a model, and it is the first condensation worth building.

### 7.2 Semantic condensation is a display act, not a storage act

A summary sentence over forty observations needs an extractor, and the moment it becomes what recall
returns, three things go wrong at once:

- **It resolves the conflict it should be reporting.** If three of the forty disagree, a summary
  picks a reading and the disagreement leaves the only surface an agent actually reads. P6 exists to
  keep contradictions visible, and summarization is precisely the operation that hides them.
- **It launders contamination.** One poisoned observation among forty becomes one confident sentence
  with no visible dissent and no provenance spread. P11 already names this failure for co-occurrence;
  a summary is its stronger form.
- **It replicates, and therefore multiplies.** This is the one that has no answer anywhere else in
  these documents. P7 says a probabilistic consolidation's output "is ingested as a derived
  observation" - and observations replicate. Two nodes holding the same log each run a pass, each
  produces a *different* summary, both are signed and both sync. Next round summarizes the summaries.
  Consolidation would grow the log with divergent content, which is the opposite of what the word
  means.

So a generated summary lives where an embedding lives: **node-local, regenerated from current state,
never stored, and labelled by the `mode` field that already tells a client which surface answered.**
Every one of the three problems dissolves there. A summary rebuilt at read time can *carry* the
contradiction ("three competing claims") instead of settling it; nothing enters the log to be
laundered; and nothing replicates, so nothing multiplies.

### 7.3 The rule that follows, and the two clauses it reconciles

> **Generation is node-local. Only commitment replicates.**

P7 can be read two ways, and the readings appear to conflict. The 3rd-revision clause says a
probabilistic consolidation's output "is ingested as a derived observation carrying confidence and
`derived_from` lineage". The 7th-revision clause says "consolidation generates, it does not commit".
Read the first alone and every summary becomes a log entry; read the second alone and no summary
ever does.

The later and more specific clause governs, and the first is a constraint on **form, not on
frequency**: *if* a consolidation output enters the log, it enters as a derived, lineage-bearing,
confidence-carrying observation - never as a fact, never superseding its sources. It does not say
every generated artifact must enter.

What decides whether one enters is the gate, which already exists.
[proposal-workflow.md](proposal-workflow.md) Section 14.1 blesses exactly this shape: an LLM may
enrich a **candidate proposal** with a summary or a merge rationale, and that enrichment enters as a
derived, lowest-trust, lineage-bearing observation while the model never casts the verdict. So the
serialization point is the human verdict: many nodes may generate many candidates, and only an
accepted one becomes an observation - once, converging, attributable.

This needs no new machinery. It is I18 - "consolidation generates, never commits" - applied to the
**artifact** rather than only to the effect, and it answers the authorship question the ledger never
asked: any node may consolidate, precisely because consolidating changes nothing that leaves it.

## 8. Ordering

Two tracks, independent of each other. Each step is useful alone and none reports work it did not do.

**Condensation (Section 7.1)** is the shorter track and has no dependency on the weight: the
hyperedge projection and `reify_hyperedge` both ship, so what it adds is the selection fold - a
stability/corroboration/cohesion score over recurring contexts, surfaced as candidates like every
other curation signal. It is the first condensation worth building because it needs no extractor and
carries none of Section 7.2's three hazards. Section 7.2's display-layer summary comes after the
weight, since it summarizes what the weight decided to surface.

**The weight** is the longer track, and its steps are not in the order this document first gave
them. Step 1 landed and then step 2 would not fit:

> **The dependency this ordering missed.** Two of the weight's three factors - tier and lineage - are
> properties of the row itself, readable from a hit. The third, position against the workspace's HLC
> frontier, needs the whole ordering. `search` answers from the store's indexes and walks the log
> zero times (pinned by `a_search_does_not_walk_the_log`), so supplying the frontier inside a query
> would turn a bounded lookup into a full deserialization of the log - seconds per query at 10k
> observations, for a ranking nudge. **Materialization is therefore a precondition for ranking, not
> an optimization after it**, and what was step 5 moves ahead of step 2's frontier half. The
> alternative - shipping ranking with a frontier-neutral weight - was rejected because it would put
> two different numbers in the system under one name.

1. **The weight, fold-only, unused.** *(landed)* Compute it, expose it on the curation report, change no
   ranking. This is the whole of Section 4 and it is observable before it is load-bearing: an
   operator can look at what the system would demote before it demotes anything.
2. **The materialized span.** The workspace's oldest and newest ordering HLC, computed where the
   log is already being walked and carried the way resolution.md materializes the belief at
   `reproject`. Formerly step 5; it comes here because step 3 cannot afford to compute it per query.
   *(4.2's correction shrank this from a per-observation table to two scalars.)*

   > **Not a small step, and the survey is worth inheriting.** Two scalars still need somewhere to
   > live, and neither route is cheap.
   >
   > **A store port change.** `KnowledgeStore` adds exactly two methods to `AssertionStore` -
   > `put_entity` and `add_relation` - and both write projection rows that observations assert. A
   > third for a workspace-scoped derived pair would be the first port method for a number no
   > observation asserts, and it lands in core, both adapters and the conformance suite. That is a
   > deliberate widening of the narrowest interface in the system, and it should be argued on its
   > merits rather than arrived at by needing a cache.
   >
   > **An engine-level cache.** `ReadCtx` already memoizes the log per `(log_epoch, scope)`, but it
   > is per-request: a fresh one per call means the first search after any write walks the log,
   > which `a_search_does_not_walk_the_log` refuses. Promoting it to long-lived state runs into a
   > defect that already exists - `sync_pull` reaches the store through `engine.store()` and calls
   > the sync functions directly, so `log_epoch` never advances for an applied pull. A cache keyed
   > on that counter would serve a stale span after every sync. **Fix the invalidation before
   > building anything on it**, or the first symptom is a ranking that quietly disagrees with the
   > log on a federated node - the hardest possible place to notice it.
   >
   > Which is why this document stops here rather than reaching for whichever route is quicker.
3. **The weight enters the ranked surfaces.** `fuse_rrf` gains a per-item term. `recall_eval.rs`
   already pins mean recall@5 >= 0.9 with an entity-gold subset at >= 0.99, so this step has a
   regression gate the day it lands - which is why the weight is designed before the pass that
   would tune it.
4. **The node-local re-rank.** Usage tracking, layered after and labelled by `mode` (4.1).
5. **The recall commit effect.** Section 6. Independent of the rest except for the weight floor.
6. **Idle scheduling.** Whether the materialization of step 2 runs on a timer rather than only at an
   explicit `reproject`. A scheduling decision with no semantics of its own, which is why it is last
   and why it was wrong to file the materialization itself here.

The recall commit effect could precede everything else. It should not: a retraction whose only visible effect is on belief
selection teaches reviewers that `recall` is a weaker `claim_demotion`, and that is the wrong mental
model to install first.

## 9. Invariants

| | Invariant |
|---|---|
| **C1** | The recall weight is a fold over the log. It may be **materialized at `reproject`**, the way resolution.md materializes the belief - what is forbidden is a consolidation pass writing a score of its own, which is a projection write no observation asserts. An earlier wording said "never a stored column" and so forbade the precedent it meant to follow. |
| **C2** | The weight consumes no wall clock, no arrival order and no randomness. Recency is rank against the workspace HLC frontier. |
| **C3** | The committed weight consumes no node-local signal. Usage may only re-rank the already-exempt surfaces, labelled by `mode`. |
| **C4** | The weight has a positive floor. `get_entity`, `get_observation` and `traverse` do not consult it - demoted knowledge stays reachable by explicit query (P7). |
| **C5** | Demotion appends no observation and changes no belief. Anything that appends an observation goes through the gate. |
| **C6** | A merged `recall` retracts and floors; it deletes nothing and is reversible by a new proposal (P3). |
| **C7** | A recall proposal presents its `derived_from` closure in the belief diff and binds to the base it reviewed. The closure is never cascaded silently. |
| **C8** | Retraction is not excision. A demand to destroy is answered by excision.md or by saying it is unbuilt - never by a recall reported as removal (E9). |
| **C9** | Structural condensation (hyperedge -> reify) selects by a deterministic fold, and corroboration counts independent principals rather than repetitions (P11). |
| **C10** | A generated summary is node-local, regenerated, never stored, and labelled by `mode`. It reports contradictions rather than resolving them. |
| **C11** | Generation is node-local; only commitment replicates. A consolidation artifact reaches the log only as the enrichment of a gated candidate (proposal-workflow.md 14.1), so no node's pass can multiply another's. |
| **C12** | A partial implementation is not shipped past step 1. A weight that ranks without the floor, or a recall that retracts without the diff, reports a curation the system did not perform. |

## 10. Closure map

| Demand | Where it is answered |
|---|---|
| P7 - forgetting is demotion of recall, never deletion | Sections 3, 4; C1, C4 |
| P7 - consolidation generates, it does not commit | Section 5; C5 |
| P7 - consolidation runs off the critical path | Section 7 step 5 |
| P16 - no nondeterminism in a fold; convergence on the deterministic surfaces | Section 4.1, 4.2; C2, C3 |
| P16 - a response labels which surface answered | Section 4.1; C3 |
| P18 - recall is trust-weighted | Section 4.3 (effective tier as an input) |
| P18 - contamination can be traced back and cleaned | Section 6; C6, C7 |
| P23 - every proposal kind the surface accepts has a commit effect | Section 6 |
| P23 - no merge without a diff; a verdict binds to the base it reviewed | Section 6; C7 |
| P3 - nothing is destroyed | Sections 2, 6; C6, C8 |
| P1 - no API writes a fact that did not pass through an assertion | Section 3; C1 |
| P5 - a retracted subject with no other assertion is absent, not false | Section 6 point 2 |
| P11 - the induction substrate is second-order structure; a reference, not a judge | Section 7.1; C9 |
| P6 - a condensation must not settle a contradiction it should surface | Section 7.2; C10 |
| P19 - the probabilistic edge widens recall and never commits | Sections 7.2, 7.3; C10, C11 |
| P7 - the two readings of "ingested as a derived observation" reconciled | Section 7.3 |
