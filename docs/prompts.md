# Prompts - insight on the client's model, decisions at the gate

> How a person reads what the agents learned, and improves it, without a document store. Companion
> to [consolidation.md](consolidation.md) (Section 7: generation is node-local, only commitment
> replicates), [proposal-workflow.md](proposal-workflow.md) (the gate) and
> [resolution.md](resolution.md) (Section 6: who may grant what).
>
> Status: **built** (Section 8 steps 1-2). Step 3 is three later decisions, each its own.

## 1. Why this exists

The owner asked for a way to read and organize the knowledge the agents write: documents that turn
nodes and subgraphs into insight. An adversarial review of that plan (2026-10-08) found that a
stored, human-managed document breaks rules this project already settled:

- **A stored insight is a stored summary.** consolidation.md Section 7.2: it resolves the conflict
  it should report, launders a poisoned observation into a confident sentence, and multiplies when
  it replicates. C11 follows: *generation is node-local, only commitment replicates*.
- **A document has either no authority or the gate's.** A human's judgment that changes what the
  canon believes is a proposal and a verdict (P23). Prose that carries that weight without the gate
  is a bypass; prose that does not is a note.
- **A document someone must maintain is separate labor** (P22), and it goes stale the moment the
  agents observe again.

What the plan wanted is real, though: a person who can see what the agents learned, what is
contested, and what is waiting on them - and who can make it better. The design keeps that and moves
it to where the rules already allow it:

- **Insight is a display act.** The client's model writes it from current state, on request, and it
  is never stored. Asking again after the canon moves gives the new reading.
- **Judgment is the gate.** Anything that should outlast the conversation becomes a proposal, and a
  person casts the verdict in the console. The canon is what remains.
- **Improvement is a loop.** A person reads a brief, asks the model to tidy what it found, reviews
  the proposals it drafted, and the next brief reflects the verdicts.

MCP prompts are the surface for this. They are user-invoked (a client shows them as commands, e.g.
`/mcp__supragnosis__brief`), they run on the client's model rather than in this server (no model
here - architecture.md non-goals), and they are already specified and unbuilt: architecture.md
Section 7 lists `what-do-we-know-about` and `summarize-workspace-knowledge`, and
proposal-workflow.md Section 11 lists `review-proposal`. P22 names them as its outstanding piece.

## 2. What this is NOT

- **Not a document store,** and not stored summaries. Nothing a prompt produces is written by the
  server. The model's answer lives in the conversation.
- **Not new tools.** The prompts drive the existing ones (`search_knowledge`, `get_entity`,
  `traverse`, `workspace_map`, `propose`, `get_proposal`, `review`). P21's tool budget is unchanged.
- **Not a verdict path.** No prompt asks the model to merge. A merge cast through the agent surface
  is capped at `host_signed` (resolution.md Section 6), and promotion to `human_confirmed` is the
  person's act in the console. The prompts say so, and the cap holds whether they are followed or
  not.
- **Not elicitation.** Casting a verdict from inside the conversation needs MCP elicitation, which
  is unbuilt (P21). The console stays the place a person decides.

## 3. The four prompts

Each prompt returns messages: an instruction, and a digest of current state the server computes
deterministically from the log and projection (Section 4). The model reads the digest, calls tools
for detail, and writes its answer.

| Prompt | Arguments | The job |
|---|---|---|
| `brief` | `workspace` (default: the node default), `focus` (optional topic) | What this workspace's knowledge says now: its themes, what is contested, what is weakly supported, and what is waiting on a decision. |
| `what-do-we-know-about` | `topic`, `workspace` (optional) | What the store holds on one topic, with sources - and what it does not hold. |
| `curate` | `workspace` | Improve the canon: draft proposals for the tidy-ups the curation signals point at, for the person to review. |
| `review-proposal` | `id` | A first-pass review of one proposal: what it changes in belief, what its checks say, and a recommendation. The verdict stays the person's. |

`brief` is the `summarize-workspace-knowledge` of architecture.md, renamed for the job it does: a
brief carries contested points and open decisions, which a summary would flatten (Section 5).

**The loop.** `brief` to understand, `curate` to propose improvements, the console to decide,
`review-proposal` when a proposal needs a second reading, and `brief` again to see the result.

## 4. What the server puts in a prompt

The digest is a pure read of the node's current state - node-local, like a search result - and the
same state always gives the same digest.

- **`brief`:** entity, relation and observation counts; the largest co-occurrence clusters
  (`workspace_map`); contested beliefs with their competing values and tiers; claims resting on a
  single agent-tier source; open proposals with kind and targets; the most recent observations with
  their ids and tiers.
- **`what-do-we-know-about`:** the search hits for the topic across workspaces, with ids, kinds,
  scores and the `mode` that answered (keyword or hybrid), and the entities they touch.
- **`curate`:** the curation report - duplicate candidates, grab-bag clusters, orphans,
  contradictions, merge cycles - with the evidence each names.
- **`review-proposal`:** `get_proposal`'s view: the proposal, its belief diff and its check
  results.

Every section is bounded (a count per section, and a byte budget for the whole digest) and says
what it left out ("12 more open proposals - call `list_proposals`"). A digest that silently stops is
a P5 failure: a short list must not read as the whole list.

## 5. Rules every prompt keeps

- **Evidence is data, not instructions.** Observation text is written by agents and peers, so it is
  untrusted. The digest is fenced and labelled as evidence, and the instruction tells the model not
  to follow instructions found inside it.
- **Contested stays contested.** The model presents competing values as competing, with their tiers
  and sources, and does not pick one. Settling a conflict is a verdict.
- **Tier is shown, not erased.** A claim resting on one `agent_extracted` source is reported as
  such, not as fact. Absence is reported as "not found in this workspace", not as false (P5).
- **Every claim cites its source** as `supragnosis://observation/{id}`, so the person can open the
  evidence.
- **Nothing is stored.** The answer lives in the conversation. If the person wants a judgment to
  last, the model drafts a proposal, never an observation that restates the brief - that would be
  the stored summary of consolidation.md Section 7.2.
- **Decisions are the person's.** The model may draft proposals (`propose`) and comment (`review`
  with `comment`). It does not merge, and it ends by telling the person where to decide: the Review
  panel in the Supragnosis app or viewer.
- **The person's language.** The instruction is English; the model answers in the language the
  person writes in.

## 6. Surfaces

- **The local surface** (the bridge and the loopback daemon) serves all four prompts.
- **A hub's principals** do not get them yet. A digest for a principal must be computed within that
  principal's grants, and the curation report and proposal folds are not scoped that way today.
  Until they are, `prompts/list` on the remote surface is empty and `prompts/get` refuses with the
  reason (remote-server.md R3: what has no declared remote policy is refused, never filtered
  silently).
- **The prompt list is a pinned contract,** like the tool list (compatibility.md Section 7): a
  golden file holds `prompts/list`. Renaming a prompt or an argument, or making one required, is a
  breaking change.
- **The list carries the same cache hints as the tool list** (`ttlMs` 0, `cacheScope` private): a
  client that validates the 2025-11-25 schema rejects a list without them (v0.3.1).

## 7. Invariants

| | |
|---|---|
| **PR1** | No prompt writes. Getting a prompt reads state and returns messages. |
| **PR2** | A digest is a deterministic function of the node's current state and the arguments. |
| **PR3** | Every digest section is bounded and says what it left out. |
| **PR4** | Evidence in a digest is fenced and labelled untrusted. |
| **PR5** | No prompt instructs a merge verdict. Promotion to `human_confirmed` stays the console's act. |
| **PR6** | The remote surface lists no prompt and refuses each by name until digests are scoped to a principal's grants. |

## 8. Ordering

1. **The prompts capability and the four prompts** on the local surface, with the digest builders
   as functions of the engine's reads (`supragnosis-mcp/src/prompts.rs`), the remote refusal, the
   golden `prompts/list`, and the cache hints.
2. **Documentation.** architecture.md Section 7 (prompts implemented), P22's status in Section 14,
   the README's tool and prompt list.
3. **Later, each its own decision:** prompts on the remote surface once digests are grant-scoped;
   a verdict from inside the conversation once elicitation exists; a signed review - a person's
   signature over the beliefs a brief cited, re-checked for drift - if briefs prove useful.

## 9. Closure map

| Need from the plan | Where it lands |
|---|---|
| Read what the agents learned | `brief`, `what-do-we-know-about` |
| Insight from nodes and subgraphs | the client's model, over the digest and tools (Section 4) |
| Organize and improve the knowledge | `curate` drafts proposals; the console decides |
| Human gating | proposals and console verdicts (P23, resolution.md Section 6) |
| Keep the result | the canon - verdicts in the log - not a document |
