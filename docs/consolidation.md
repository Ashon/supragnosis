# Consolidation - forgetting as demotion, tidying as re-projection (Principle 7)

> How a workspace stays usable as it grows, without anything being deleted. This document fixes what
> a consolidation pass may compute, what may act on the result, and why the obvious cheaper version -
> a mutable score column that a background job writes - is worse than not having it.
>
> Status: **specification**. Nothing here is built. The ledger owns it as M6
> ([architecture.md](architecture.md) Section 12). The generate half - the read-only curation
> signals - landed early with M3.5 and this document builds on it rather than replacing it.

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

The fix is to stop asking the machine what time it is and ask the log instead. Age is a **rank
against the workspace's own HLC frontier**: an observation's position in the ordering the log already
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

## 7. Ordering

Each step is useful alone and none reports work it did not do.

1. **The weight, fold-only, unused.** Compute it, expose it on the curation report, change no
   ranking. This is the whole of Section 4 and it is observable before it is load-bearing: an
   operator can look at what the system would demote before it demotes anything.
2. **The weight enters the ranked surfaces.** `fuse_rrf` gains a per-item term. `recall_eval.rs`
   already pins mean recall@5 >= 0.9 with an entity-gold subset at >= 0.99, so this step has a
   regression gate the day it lands - which is why the weight is designed before the pass that
   would tune it.
3. **The node-local re-rank.** Usage tracking, layered after and labelled by `mode` (4.1).
4. **The recall commit effect.** Section 6. Independent of 1-3 except for the weight floor.
5. **Idle scheduling.** Only now is there anything worth scheduling. Steps 1-4 are recomputation on
   the read path; this step is the decision to precompute at `reproject` time, and it is a
   performance change with no semantics of its own.

Step 4 could precede 1-3. It should not: a retraction whose only visible effect is on belief
selection teaches reviewers that `recall` is a weaker `claim_demotion`, and that is the wrong mental
model to install first.

## 8. Invariants

| | Invariant |
|---|---|
| **C1** | The recall weight is a fold over the log, never a stored column. No consolidation pass writes to the projection. |
| **C2** | The weight consumes no wall clock, no arrival order and no randomness. Recency is rank against the workspace HLC frontier. |
| **C3** | The committed weight consumes no node-local signal. Usage may only re-rank the already-exempt surfaces, labelled by `mode`. |
| **C4** | The weight has a positive floor. `get_entity`, `get_observation` and `traverse` do not consult it - demoted knowledge stays reachable by explicit query (P7). |
| **C5** | Demotion appends no observation and changes no belief. Anything that appends an observation goes through the gate. |
| **C6** | A merged `recall` retracts and floors; it deletes nothing and is reversible by a new proposal (P3). |
| **C7** | A recall proposal presents its `derived_from` closure in the belief diff and binds to the base it reviewed. The closure is never cascaded silently. |
| **C8** | Retraction is not excision. A demand to destroy is answered by excision.md or by saying it is unbuilt - never by a recall reported as removal (E9). |
| **C9** | A partial implementation is not shipped past step 1. A weight that ranks without the floor, or a recall that retracts without the diff, reports a curation the system did not perform. |

## 9. Closure map

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
