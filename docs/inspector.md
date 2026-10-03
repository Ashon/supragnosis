# Inspector - what a detail surface says, and what it may leave unsaid (Principles 2, 5, 6)

> The viewer's detail surfaces - the node inspector, the observation card, the hover tip - render
> every field they receive, every time. This document fixes which fields may go quiet, under what
> rule, and what no rule may touch.
>
> Status: **specification, with Section 8 step 1 built** - the silence rule, the scope line and
> the D1 guard. Steps 2-4 are not.

## 1. Why this exists

Measured on a copy of the author's store (2026-10-03, 336 entities across two workspaces), in the
desktop shell at its default window size (1280x860):

- All 336 entities have the same effective tier, and all 336 have the same single origin host. The
  inspector spends its second line on the first and its third on the second, for every node.
- No entity is contested and none carries a competitor, so the "belief decision" disclosure, opened,
  shows one box per field holding a single candidate marked winner. Opening it shrinks the relation
  and evidence columns to one visible row each.
- The panel is capped at `36vh`. On the busiest entity (degree 18) that shows 6 of 17 incoming
  relations and 2 of 10 evidence rows. 15 of the 17 incoming relations are the same kind, so most of
  the visible rows repeat one word.
- The evidence column shows the first two lines of each observation. The observations behind that
  entity run 565 to 1235 characters and name 12 to 19 entities each. In the one opened during the
  measurement, 2 of 11 asserted relations involve the entity being inspected, and nothing marks which
  two.

So the panel is not short of space. It spends its space on what does not vary and cuts off what does.

The obvious repair - leave out what does not vary - is the one with principle stakes. Leaving out a
field is how a surface stops answering "where did this come from" (P2), how silence comes to read as
negation (P5), and how a contradiction stops being visible (P6). This document exists so that the
repair is made against those three, not around them.

## 2. What this is NOT

**It is not a change to what the server answers.** `/api/graph`, `/api/explain` and
`/api/observations` keep every field they return today. The rule below is a rendering decision over
data the viewer already holds; one field is added (Section 7) and none is removed or narrowed. The MCP
surface is untouched - the viewer is a separate human channel (P21).

**It is not a way to quiet a conflict.** A contested field is outside the rule entirely (D1). The space
this document reclaims comes from fields that agree with everything around them, never from fields
that disagree.

**It is not a canvas change.** Focusing a node on the canvas stacks one edge label per relation and
repeats hull labels named after the same hub. That is real clutter, but it carries no principle
stake: label placement is rendering discretion the way hull shape already is (architecture.md
Section 10), and it can be fixed without a document.

## 3. The rule: a field speaks when it varies

A field is silent in the inspector when it has **one value across the scope**, and shown otherwise.

**The scope is what the viewer loaded**: the `/api/graph` response for the selected workspace, or for
all of them under `*`. It is not the legend-filtered subset. Toggling a type chip off hides nodes; it
must not make a tier line appear on a node that has nothing to do with that type.

**Uniform, not majority.** The alternative - silent when the node matches the scope's most common
value - hides more, and was rejected. Under it, silence states a statistic rather than a fact: a
node's tier line would appear or vanish as unrelated nodes arrive and the proportions move, and "not
shown" would mean "same as most" with no way to tell how many. Under the uniform rule, silence means
exactly "the same as everything here", which the scope line (Section 4) can state in full.

**Effective values only.** The tier the rule compares is `GraphNode.trust_tier`, the receiver-evaluated
effective tier (resolution.md Section 3). A baseline built from claimed tiers would reintroduce, as a
display rule, the exposure F13 closed in the projection.

**Federation needs no special case.** On a node that has never synced, origin is uniform and goes
quiet. On a hub, or in any scope holding knowledge from more than one host, it varies and comes back on
its own. The rule is written so that the change Phase 3.5 brings - the same viewer served to principals
who are not the operator - reaches the display without anyone revisiting it.

## 4. Silence is declared

A row that is usually present and is not reads as "this node has no tier" or "origin unknown". P5
forbids exactly that reading: absence is not negation. So every field the rule silences is **stated
once, on the inspector itself**: one muted line naming the scope's uniform values (`same across this
scope: agent_extracted, localhost`), present only when at least one field is silenced.

Two cheaper placements were rejected. The status bar already carries scope-wide counts, but it is read
for the graph, not for the node in hand - the question "what tier is this?" is asked in the panel and
must be answered there. A tooltip is not read at all until someone already suspects the answer.

The tier additionally keeps its colored dot (the log rows' `.tdot`) on every node, silenced or not. It
costs eight pixels and means the field is never wholly absent from the panel.

## 5. Field by field

| Element | Shown | When silent or folded |
|---|---|---|
| name, type | always | - |
| description | when asserted | nothing: an unasserted description is the normal case (P8 makes it optional at capture) |
| effective tier, as text | when the scope is not uniform | stated in the scope line; the dot stays |
| origin | when the scope is not uniform | stated in the scope line; per-attestation hosts stay on the card |
| workspace | when the scope spans more than one | the workspace chips name it. An entity whose `workspace` is empty sits in more than one through a merge, and says so rather than showing nothing |
| aliases | when non-empty | nothing to state |
| contested block | whenever `contested` is true, expanded, with its actions | never silenced (D1) |
| competitors resolved by trust | one visible line with the count, expanding to the rows and their actions | never hidden, only folded: resolution.md 4.1 makes these informational, P6 keeps them queryable |
| "belief decision" disclosure | when some field has more than one candidate | absent; the observation that asserted the value stays one row in the evidence list |
| degree | not in the header; the relation group headers carry the counts | - |
| sources | in the evidence header. The attestation count appears beside the observation count only when they differ | - |
| ids and hashes | the card footer | - |

Two of these deserve their reason in prose.

**The disclosure goes, the evidence stays.** With one candidate per field, the disclosure answers "why
this value" with "nobody said otherwise", and the observation it would name is already a row in the
evidence list one region over - a row that, after Section 7, leads with the kind it asserted. P2's
demand is that the answer be reachable from the claim, not that it be printed twice.

**`sources` and the evidence count are different numbers.** `sources` counts attestations; the
evidence list counts observations, and a re-observation by another host adds an attestation without
adding a row. Equal is the common case, so the panel shows one number and adds the second only when
they part - which is precisely when it says something.

## 6. Relations are grouped by what they say

The relation region groups by **(direction, kind)**. A group header carries the kind, the direction and
the count; its members are name chips colored by type, folded beyond a small number with a `+N` that
expands in place. Superseded relations (`valid_to` set) sort after live ones and are drawn muted, as on
the canvas - captured history is shown faithfully, never dropped (P4).

The inspector is rebuilt on every graph poll, so the order must be stable across rebuilds: groups by
count, then kind; members by name. An expanded group and an expanded panel survive the rebuild the way
the "why" disclosure already does.

The three equal columns become two regions, relations and evidence. The `36vh` cap stays the default
and the panel gains an expand control; the expanded height is tuning, not a decision this document
makes.

## 7. Evidence rows say what touched this node

An evidence row leads with **what the observation asserted about the focused node** - the relations
with an endpoint that resolves to it, and its own entity assertion where that carries a kind or a
description. The content follows, clamped, and stays one act away on the card, which remains the
dereference surface for the observation (P2, P14). Opened from a node's evidence row, the card leads
with the same assertions and folds the rest under counts; opened from the workspace Log tab, where
there is no focused node, it renders as it does today.

**Which assertions involve the node is decided by the server, by id.** `ObsSummary.entities` already
carries canonical ids - `canon(Entity::make_id(ws, name))`, forwarded through accepted merges - but
`RelationRef` carries only endpoint spellings. Matching spellings in the viewer would be a second
identity resolution, and it would be wrong in reachable cases: `make_id` trims and lowercases, so two
spellings name one entity, and an accepted merge forwards an id that no spelling reveals. The
`/api/explain` contract already rules this shape out for the belief ("an explanation OF the
projection, never a second computation that could drift"), and the same reasoning holds for identity
(P15).

So `RelationRef` gains its two canonical endpoint ids, computed by the same `canon` the entity filter in
`observation_log` uses one loop earlier. This is the only server change in the document. It is
additive: no row is selected differently and no existing field changes.

## 8. Ordering

1. **The silence rule and the scope line** (Sections 3-5). Client-only, smallest, and the one that
   carries D1-D4, so it lands first and alone.
2. **Grouped relations and the two-region layout** (Section 6). Client-only.
3. **Endpoint ids on `RelationRef`, then node-focused evidence rows and card** (Section 7). The server
   field lands first, because the client half has nothing correct to match on without it.
4. **The hover tip** follows the same rule as the inspector header. It is last because it is the
   smallest surface and inherits every decision above.

Each step is judged on fixed reference entities at the shell's default window size: the busiest hub,
an evidence-heavy entity, an isolated one, and a contested one. The author's store holds no contested
entity, so the last needs a seeded store (`task server:mem`). It is the case D1 exists for, and judging
the change without it would test everything except the invariant that matters most.

D1 is guarded by `inspector_never_folds_a_contested_belief`, a source-level tripwire in
`crates/supragnosis-viz/tests/http.rs` written in the style of the escaping guard beside it: the
contested branch is the first decision the block makes and returns before any fold is built, the
silence rule never reaches it, and the panel renders it outside any condition of its own.

## 9. Invariants

| | Invariant |
|---|---|
| **D1** | A contested field is never silenced or folded. A node with `contested` true shows its contested block expanded, with its actions, in every scope (P6). |
| **D2** | Everything the rule silences or folds is reachable from the inspector in at most one act - an expand, or opening the card - and none of it requires another surface (P2, P14). |
| **D3** | Silence is declared. A field the rule silences is stated once for the scope on the inspector itself; a silenced value never reads as an absent one (P5). |
| **D4** | The baseline is computed over the loaded scope from effective values - the tier from `GraphNode.trust_tier`, never a claimed tier - and never over the legend-filtered subset (resolution.md Section 3, F13). |
| **D5** | The viewer never re-resolves identity. Which entity an assertion is about comes from server-resolved ids, not from matching spellings (P15). |
| **D6** | The rule changes rendering only. No response field is removed or narrowed, and no MCP tool or resource changes (P21). |
| **D7** | Every new HTML sink routes untrusted strings through `esc()`. The `no-unsanitized` lint enforces it in CI, and `viz_source_escapes_untrusted_names` pins the escaper (P18, federation.md 6d). |

## 10. Closure map

| Demand | Where it is answered |
|---|---|
| P2 - a claim on a read surface can say where it came from | Sections 5, 7; D2 |
| P5 - absence is not negation | Section 4; D3 |
| P6 - a contradiction is surfaced, and a resolved one stays queryable | Section 5; D1 |
| P14 - identifiers stay dereferenceable | Sections 5, 7; D2 |
| P15 - identity is resolved by the substrate, once | Section 7; D5 |
| resolution.md Section 3 - display consumes the effective tier | Section 3; D4 |
| P21 - the MCP surface stays narrow; the viewer is a separate channel | Section 2; D6 |
| P22 - curation surfaces as a micro-decision where the reader already is | Section 5 (contested actions stay inline) |
| P18 / federation.md 6d - synced names are untrusted input | D7 |
