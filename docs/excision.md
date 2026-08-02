# Excision - the destruction-demand exception (Principle 3)

> The one act that removes knowledge instead of adding it. This document fixes what it may do, what
> it may not, and why the obvious cheaper versions are worse than not having it.
>
> Status: **specification**. The excision path itself is unbuilt and the ledger owns it as M4 Phase 5
> ([architecture.md](architecture.md) Section 14). Step 1 of Section 8 - the ingest hook that keeps
> secrets out in the first place - has since been built; nothing else here has.

## 1. Why this exists, and why it is the only one

Principle 3 makes the log immutable and says so in the strongest terms: deletion destroys
information, superseding adds it, and append-only immutability is the precondition for content-address
dedup and topology-independent convergence. Break it and the sync model breaks with it.

It then carves out exactly one exception:

> There is exactly one exception, a **destruction demand due to regulation/privacy**, and even in this
> case a tombstone recording "what was destroyed" is left behind.

Appendix A settles the conflict in advance - *"Preservation (3, 6) vs privacy (17): **Privacy wins.**"*
So this is not an open design question. It is an unimplemented mandate, and the deferral ledger has
already assigned it a due date it reached by argument rather than by anyone scheduling it: the first
multi-principal deployment, because that is when a destruction demand can first arrive from someone
who is not the node's operator.

There is a second demand, closer to hand than regulation: **a secret that an agent observed while
working.** P22 makes knowledge a by-product - agents shed observations as they go - so sooner or later
one of them reads a file with a credential in it and observes it. That is the case this document is
actually written for, and it is why the ordering in Section 8 matters.

## 2. What excision is NOT

Three cheaper things look like this feature and are not it.

**It is not editing.** An observation's id is `blake3(workspace, content, assertions)` - content IS
identity (P14). Redacting a substring changes the id, and every reference to the old one - lineage,
proposal targets, signed attestations - becomes dangling. There is no surgical edit of an observation,
only excision of the whole row. The unit is the observation, not the character range.

**It is not retraction.** P18's sanitizability walks `derived_from` and retracts a contaminated tree in
bulk - but a retraction is an appended verdict, and the log keeps everything. That is the right answer
for contamination (the claim stops being believed) and the wrong one for a secret (the secret is still
readable in the log). Excision and recall are different acts with different mechanisms, and a surface
that offers one while the operator believes they got the other is the failure this document exists to
prevent.

**It is not deleting the row from the store.** In a federated node this silently fails. The sync cursor
is *derived from what is stored* (`version_vector` folds `attestations_since`), so removing a row
retreats the cursor and the next pull re-delivers it. P3 named this - *"destroyed knowledge would come
back from a peer that still holds it and be resurrected"* - and the current implementation confirms it:
there is no separate persisted watermark to keep the deleted row from coming home.

## 3. The tombstone

Excising id `X` replaces its row with a **tombstone** under the same id.

**What it records.** Enough to audit the act, never enough to reconstruct what was destroyed: the id,
the workspace, the excising principal as a delegation chain (P2), the transaction time, a
non-quoting reason, and a structural census of what was removed - attestation count, how many entity
and relation assertions, whether it carried lineage. The census exists so a later reader can tell a
destroyed observation from one that never existed; it must not include content, assertions, or
anything derived from them by a reversible function.

**The reason field must not quote the secret.** A destruction reason saying `contained AWS key
AKIA...` re-enters the thing being destroyed into a permanent record, and the tombstone propagates.
The surface must refuse to accept a reason it cannot bound, and the ingest detector of Section 8 is
the same machinery that can enforce it.

**It is absorbing** (P3, and P16's absorbing-state concept). Once a node holds a tombstone for `X`:

- `add_observation(X)` is refused, locally and from the wire. It is not an error to the caller - a
  peer re-sending `X` is behaving correctly and must not be treated as faulty - but it never lands.
- The tombstone can never be superseded, merged with, or removed. Excision is terminal, which is what
  makes it monotonic and therefore safe to build derivations on (P16: convergence is order-independence,
  monotonicity is stability under growth).
- Re-observing the same content later produces the same id and is therefore also refused. This is
  correct and worth stating plainly: **excision destroys an id, not a text.** If the same text is
  legitimately observed again later, it cannot enter this workspace under that id again.

**It participates in the sync cursor.** A tombstoned id must keep advancing the version vector for its
`(origin, origin_seq)`, exactly as the live row did. Without this the cursor retreats and Section 2's
resurrection returns through the front door. This is the single most easily missed requirement here,
because it is invisible on a node with no peers.

## 4. Propagation, and how far it reaches

The tombstone replicates like any other event, through the same signed, allowlisted, workspace-filtered
path (federation.md 6a/6c). Receiving one destroys the local row if present and installs the absorbing
state if not - a node that never held `X` still learns that `X` must never be accepted.

**Excision reaches exactly as far as the sharing boundary reached.** This has an operational consequence
worth stating as a rule, because it inverts the intuition:

> **Excise first, then narrow sharing.** Removing a peer's access before excising leaves the secret on
> that peer with no path for the tombstone to follow. Narrowing is the right first move to stop a leak
> *spreading*; it is the wrong first move if you intend to destroy.

A workspace that was never shared has no propagation problem. A workspace shared with a peer that has
since been removed from the allowlist has an unreachable copy, and the surface must say so rather than
report a clean excision.

## 5. The derived tree

P18 makes lineage the recall list: a contaminated source can be traced through `derived_from` to
everything summarized or inferred from it. Excision needs the same walk for a different reason - a
summary of a secret may contain the secret.

**The walk selects candidates; a human excises.** It must not cascade automatically. Whether a derived
observation actually carries the secret is a judgment about content, and P19's rule holds: a generator
proposes, a deterministic act commits. So the surface presents the derived closure with each row's
content, and each excision is its own act with its own tombstone.

Cascading would be the more comfortable design and it is wrong twice: it destroys knowledge that
merely *cites* the secret without containing it, and it hands the largest destructive radius in the
system to a heuristic.

## 6. Who may do it

At least as restricted as the recall verdict, which P23 makes **non-delegable** - a human's direct act,
not an agent's proxy verdict under a human principal's authority. Excision has a strictly larger radius
than recall, so:

- **Console only.** The viewer's unix socket is the surface whose "only the local principal" property
  is guarded (P17, F19). The agent MCP path must not carry it - not gated, not with elicitation.
- **Not through the proposal gate.** P23 is explicit that the gate is *"a gate of tier, not a gate of
  existence"*, and a proposal is itself an observation, so proposing an excision would put a
  reason-bearing record of the secret's location in the log and then wait for review. A destruction
  demand is urgent by nature. The tombstone is the audit record; it does not need a second one.
- **Recorded as the sovereign's act.** The excising principal rides in the tombstone as a delegation
  chain, so a later reader can ask who destroyed this and under whose authority (P2).

## 7. What the projection does afterwards

Excision removes attestations from whatever the observation supported, so the entity/relation
projection must be **re-materialized, not patched** - the same replay the store migration relies on.
An entity whose only supporting observation was excised correctly disappears from the graph.

Two consequences to make explicit, because both look like bugs when first seen:

- **A proposal that targeted the excised observation keeps a dangling reference.** The fold already
  tolerates unresolvable targets, keeping the id rather than dropping the row, and that is the right
  behaviour here: a proposal that reviewed something now destroyed should read as exactly that.
- **The graph can shrink in ways no one asked for**, if the excised row was the last support for
  entities that other rows merely mention. This is the same shape as the log/projection divergence a
  fresh replay exposes, and the same answer applies: the log is the truth, and the projection follows.

## 8. Ordering - why prevention comes first

The excision path is the most destructive act in the system and depends on machinery that does not
exist (the lineage walk is M5). A partial version of it is **worse than nothing**: an excision that
skips the derived tree, or that does not propagate, reports the secret removed while it is still
readable somewhere. The operator stops looking. With no feature at all, at least the problem is known
to be open.

So the order is:

1. **A redaction hook at ingest** (P17's fourth enforcement demand). **Built.** It refuses rather than
   rewrites - P1 forbids transforming an assertion before the log, and a rewrite would move the
   content address (P14) - and the refusal names the shape and the field, never the value. It runs at
   every local ingest door and deliberately NOT on the sync apply path: detector patterns grow, so a
   newer node would refuse what an older peer accepted and the two would hold different logs from one
   event set (P16). Every secret it catches is one that never needs any of this.
2. **Detection without destruction.** **Built.** The same detector, over the stored log, reported as a
   curation signal - read-only like every other one (P7), and above the hygiene signals in the console
   because this is not housekeeping. It reports the id, the field and the shape and never the text: a
   report travels into logs and screenshots, so quoting the secret would copy it everywhere the report
   goes. The door and the scan walk one shared field list, because two would drift silently in the
   worst direction - a field the door checks and the scan does not is a secret reported as absent.
3. **Containment, which already works today.** Narrowing a peer's shared workspaces stops a leak
   spreading now. It does not recall what has synced, and Section 4's ordering rule applies.
4. **Excision**, with the lineage walk, the absorbing tombstone, cursor participation and propagation
   - all of it or none of it.

## 9. Invariants

| | Invariant |
|---|---|
| **E1** | Excision replaces an observation with a tombstone under the same id. There is no in-place edit of content, because content is identity (P14). |
| **E2** | A tombstone records the act and a structural census, never the content or anything reversibly derived from it - including the stated reason. |
| **E3** | A tombstone is absorbing: `add_observation` for that id is refused locally and from the wire, and the tombstone is never superseded or removed (P3, P16 monotonicity). |
| **E4** | A tombstoned id keeps advancing the version vector for its `(origin, origin_seq)`. Without this the cursor retreats and the row is re-pulled. |
| **E5** | Tombstones propagate through the ordinary signed, allowlisted, workspace-filtered sync path. Excision reaches exactly as far as sharing reached, and the surface reports where it could not reach. |
| **E6** | The derived closure is presented, never cascaded. Each excision is its own act with its own tombstone (P19: a generator proposes, a deterministic act commits). |
| **E7** | Excision is a console act only, non-delegable, at least as restricted as the recall verdict (P23 I17). It does not pass through the proposal gate, which is a gate of tier and not of existence. |
| **E8** | After excision the projection is re-materialized from the log, never patched. |
| **E9** | A partial implementation is not shipped. Excision without the lineage walk, the absorbing state, cursor participation or propagation reports a removal it did not perform. |

## 10. Closure map

| Demand | Where it is answered |
|---|---|
| P3 - the single destruction exception, tombstone, absorbing, propagating | Sections 3, 4; E1-E5 |
| P17 - privacy wins over preservation; sovereignty over what leaves | Sections 4, 8; E5 |
| P18 - sanitizability through `derived_from` | Section 5; E6 |
| P23 I17 - the most destructive verdict is a human's direct act | Section 6; E7 |
| P16 - absorbing state, monotonicity, no resurrection | Section 3; E3, E4 |
| P14 - content is identity, so excision is not editing | Section 2; E1 |
| P2 - who destroyed this, under whose authority | Sections 3, 6; E2 |
