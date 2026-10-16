# Dreaming - idle consolidation by a model, behind the gate

> How a node tidies its own knowledge while nobody is using it: a scheduled, opt-in session of the
> person's own AI client reads one workspace's curation signals and leaves a few evidence-cited
> **drafts** of proposals on this node, which the person publishes, accepts or discards in the
> morning. The dream writes nothing to the log. Companion to [consolidation.md](consolidation.md)
> (the deterministic half, retrieval demotion), [proposal-workflow.md](proposal-workflow.md)
> Sections 4 and 14 (the Draft state, consolidation as a generator), [resolution.md](resolution.md)
> (the gate grants R5 governs), [prompts.md](prompts.md) (`curate`, whose digest the dream reads)
> and [remote-server.md](remote-server.md) Section 4.3 (the per-tool policy the dream surface
> copies).
>
> Status: **specified**, not built. Section 12 is the order the work lands in; what building it
> changes goes into Section 15. Two adversarial reviews against the principles and the code shaped
> this document before it was committed; what they changed is in Section 16.

## 1. Why this exists

The owner asked for a model that optimizes and prunes the knowledge the way sleep consolidates
memory. Principle 7 anticipated exactly this: it names sleep-time reprocessing as the approach
agent-memory research converged on, and fixes its shape - an idle-time pass that "generates from"
the accumulated observations "while committing nothing itself".

The generating half already exists, but only on demand:

- `CurationReport` computes duplicates, grab-bags, orphans, contradictions, merge cycles, merge
  suggestions, name variants and demotion candidates, deterministically and read-only.
- The `curate` prompt hands that report to the person's model, which drafts proposals for the
  person to review in the console.

Nothing runs while nobody asks. Curation is a session the person has to remember to start, which
is the separate labor Principle 22 says kills knowledge systems. A dream is the same `curate` work,
run by the node on the person's behalf at an hour they chose, so that tidying arrives as a few
decisions waiting where they already look.

## 2. The dream in one paragraph

Once a night, if the person turned it on for a workspace and that workspace's knowledge changed
since the last dream, the node starts the person's AI client headless with one tool source: the
**dream surface** of the local daemon, bound to that one workspace. Through it the model reads the
workspace and writes **drafts** - proposals authored but not published, the Draft state
proposal-workflow.md Section 4 already defines - up to a small cap, kept on this node and never in
the log. Everything else is refused. The client itself has no shell, no files and no other tools,
and the run checks that before the model is given anything to do. In the morning the person finds
the drafts in the Review panel, each with its evidence and belief diff, and publishes, accepts or
discards each one; only the person's act enters the log. The deterministic half of consolidation -
demoting what is stale out of crowded search results - is consolidation.md's, runs inside the
daemon, and needs no model (Section 9).

## 3. What "pruning" means here

The request uses one word for several acts, and the principles answer each differently.

| What the person means | What does it | Who decides |
|---|---|---|
| Stale or low-value knowledge stops crowding results | the retrieval weight (consolidation.md Sections 3-4): a fold, floored, never zero | nobody - it is recomputed, not committed (consolidation.md Section 5) |
| Two entities are the same thing | an `entity_merge` draft | the person (P15, P23) |
| A merge that was accepted was wrong | an `entity_split` draft naming that merge ([unmerge.md](unmerge.md) S2) | the person |
| Two claims contradict and one is wrong | a `claim_demotion` draft citing both sides, to a tier below the target's current one | the person - the mediation point of P6 |
| One entity is really several, or a pattern deserves a type | nothing yet: no kind splits a grab-bag, and `tbox_change` has no commit effect (Section 4) | - |
| A source turned out to be poisoned | a `recall` - not the dream's in this version (Section 4) | a human's direct act (I17) |
| Forty observations should read as one | a summary `brief` generates at read time - never stored (consolidation.md 7.2) | nobody - nothing is kept |
| Something must be gone | excision ([excision.md](excision.md)), the one destructive act, for regulation and privacy demands - never a dream | the operator |

So the dream never removes anything, and nothing it produces changes the knowledge until the person
acts. "Pruning" by the dream is a draft to merge, undo a merge, or demote; pruning by the system is a
retrieval weight that floors.

## 4. What this is NOT

- **Not autonomous editing.** The dream writes no observation, opens no proposal and casts no verdict
  (I18, made mechanical in Sections 5.2 and 5.6). A draft becomes a proposal only when the person
  publishes it.
- **Not deletion.** Nothing is removed, ever (P3). Excision stays the one destructive act and is
  outside every dream.
- **Not a stored summary.** A draft is a candidate in the gate's own Draft state, not knowledge: no
  search, no `brief` section about the knowledge and no hub surface returns it. Its rationale reaches
  the log only if the person publishes it, as the proposal's rationale - a derived, lineage-bearing,
  lowest-trust observation, the one form consolidation.md C11 and proposal-workflow.md 14.1 admit -
  and a draft whose targets changed is dropped as superseded, so its rationale is never read as
  current (5.3).
- **Not a promoter.** `claim_promotion` drafts are refused, and a `claim_demotion` draft must ask for
  a tier below every target's current one (5.2), on top of a fold rule that keeps any demotion from
  raising (5.7). A model that reads untrusted text unattended must not be the one asking for trust
  to rise (P18: promotion happens only explicitly).
- **Not a recaller, yet.** `recall` is refused: its commit effect is unbuilt (consolidation.md
  Section 6, step 5), so a merged recall would report a retraction that did not happen (C12), and its
  verdict is a human's direct act anyway (I17).
- **Not a type inducer, yet.** `tbox_change` is refused for the same reason as recall: its belief
  diff is not computable today, because it has no commit effect, and P23 holds that a merge without a
  diff is a merge without review.
- **Not a description writer.** Filling in a missing description is `observe`, which the dream
  surface refuses; there is no proposal kind for it, and adding one is a P23 change this document
  does not make.
- **Not a model inside the daemon.** The daemon embeds; it does not generate (architecture.md
  Section 1, non-goals). The model is the person's own client, its cost is theirs, and they turn it
  on.
- **Not auto-merge.** The policy executor of proposal-workflow.md Section 9 is unbuilt and is not
  this document. Every draft waits for the person.
- **Not a governance principal.** The dream drafts on the operator's behalf and is marked as the
  dream; it is not a second person whose approval would make the operator's verdict someone else's
  (remote-server.md Section 4.2). A published dream draft is the operator's proposal for I9, exactly
  as a `curate` proposal is today (Section 8).
- **Not replicated.** Drafts live on the node that dreamed them. Two nodes dreaming over one shared
  workspace each keep their own; nothing either drafts reaches the other unless a person publishes it
  (C11).

## 5. The dream surface

The dream's safety cannot rest on what its prompt asks. It reads text that agents and peers wrote,
unattended, at night; an instruction planted in that text is the threat Principle 18 names, and a
prompt that says "do not merge" is a discipline, which I18 says this must not be. So the restriction
lives in the server, in the shape remote-server.md built for principals.

### 5.1 The credential and the configuration

`supragnosis dream enable` creates `~/.supragnosis/dream.token` (0600, like `mcp.token`) and
`~/.supragnosis/dream.toml` (0600). The loopback daemon reads both **on every request**, as
remote-server.md Section 4.2 does for principals: a request bearing the dream token is answered by
the dream surface while the file exists and names the workspace, and refused once `disable` has
removed it - no restart, no stale credential. An unknown key in `dream.toml` disables the dream
surface with the reason in `supragnosis status`; the daemon itself keeps serving (P24).

`supragnosis bridge --dream --workspace W` is the stdio relay a dream client launches: it reads the
dream token on every request and keeps no copy (client-connect.md C2). The dream surface exists only
on loopback (P17); a hub's principals never reach it. It also does not exist on a daemon run with
`SUPRAGNOSIS_MCP_AUTH=off`, checked on every request and by every run, since a surface defined by its
credential cannot exist without one.

### 5.2 What each tool may do

**One workspace, bound.** `dream run` writes the client's MCP configuration itself, naming
`supragnosis bridge --dream --workspace W`, so the client cannot change the bridge's arguments. The
bridge sends `W` with every request, and the daemon binds the MCP session to the workspace of its
first request and refuses, for the rest of that session, any request that names another - in the
binding or in a tool argument. `*` is refused. Nothing read in one workspace can therefore be
drafted into another (P17).

Every tool has a declared dream policy, and a tool without one is refused (remote-server.md R3).

| Tool | Policy | Notes |
|---|---|---|
| `search_knowledge`, `get_entity`, `traverse`, `workspace_map` | read | The bound workspace only. An id outside it answers "not found", the same answer an absent id gets (P5, and no existence oracle). |
| `list_proposals`, `get_proposal` | read | The bound workspace only. |
| resources (`supragnosis://...`) | read | The bound workspace only. |
| `propose` | draft | Writes a **draft** on this node, not a proposal in the log (5.3). Kinds `entity_merge`, `entity_split`, `claim_demotion` only. `claim_demotion` must request a tier below every target's current effective tier. Evidence and confidence required (5.4); the cap and the duplicate rules apply (5.5). |
| `review` | refused | A dream casts no verdict and leaves no comment: a comment from the person's own node on someone else's proposal would read as an endorsement. |
| `observe` | refused | The dream writes no knowledge of its own (C11). |
| `define_type` | refused | A T-Box change goes through `tbox_change` (P11), which the dream may not draft yet. |
| `sync_status`, `sync_pull`, `sync_push` | refused | Node operations. |
| prompts | `dream` only | Section 7. |

`tools/list` on the dream surface lists only the tools its policy admits, so what a session is
offered is what it may use; the run's session check (6.3, step 6) rests on that. A refusal names the
tool and the reason, so the model can carry on with what is allowed (P21), and the run record counts
it (6.4).

### 5.3 Drafts

A draft is everything a `propose` call carries - kind, targets, `into`, tier, rationale, evidence,
confidence - plus the workspace, when it was drafted, the knowledge frontier it was drafted against,
and `drafted_by: dream`. The daemon keeps drafts in `~/.supragnosis/dream/drafts/`: node-local
operational state, like the daemon's log - not knowledge, never replicated, never an observation,
never served by the viewer's `/api/proposals` (which the hub serves) or by any read tier.

- **Before a draft is kept**, its text is scanned for credential-shaped content, as ingest is, and
  refused if it has any; its rationale is capped in length and stripped of control characters.
- **A stale draft is superseded, not refused.** When an observation that is not a proposal event and
  that touches a draft's targets (5.5, "changed") arrives after the draft's frontier, the draft is
  dropped as superseded: a rationale written about a state that no longer holds must not sit beside a
  diff computed for the state that does (P7: generated output describes current state or is not
  shown). Supersession is not the person's no, so the next run may draft the same idea again with a
  rationale about the state as it is.
- **A draft expires** after 14 days unread. Expiry counts as the person's no (5.5), so an ignored
  draft is not drafted again until its targets change.
- **Excision.** A draft and the run record (6.4) can quote observation text. excision.md's closure
  gains both: when a destruction demand is honored, every draft whose evidence or targets include an
  excised id is discarded, and the run record's entries naming one are removed. Until excision is
  built, a destruction demand is answered as excision.md says - unbuilt - and this is part of what
  it will have to reach.

### 5.4 Evidence, confidence and lineage

A draft must name its evidence - the observation ids its rationale rests on, each of which must exist
in the bound workspace - and its confidence, the model's own assessment between 0 and 1. When the
person publishes the draft, the evidence becomes the proposal observation's `derived_from` and the
confidence its `confidence` (P7: a derived observation carries confidence and lineage; P18: traceable
to its sources). This needs one optional argument on `propose` - `evidence`, a list of observation
ids - which every surface accepts and only the dream surface requires, and the dream surface's
requirement that `confidence` be given. It is an argument, not a tool (P21).

Lineage has a consequence worth stating: a proposal citing a source is in that source's `derived_from`
closure, so a future recall of a poisoned source presents the proposals that cited it. Retracting a
proposal event changes nothing in the fold - a merged proposal is absorbing (I16) - so the recall's
diff presents any merge such a proposal produced as a split candidate rather than retracting it.

### 5.5 The cap, and the person's no

- **The cap.** At most `budget` dream items wait at once on this node (default 8), across every dream
  workspace: pending drafts, plus published dream proposals still open - those stamped `surface:dream`
  with this node as the acting host - for seven days after they were published. A published proposal
  in a shared workspace may wait on someone else's review for longer than the person's own attention
  should be held; after seven days it stops counting, and stays open for whoever reviews it. At the
  cap, `propose` is refused with the count, the run skips, and `dream status` says why ("8 waiting: 5
  drafts, 3 published awaiting review"). The cap is on what is waiting, not on what one night adds, so
  an unread queue never grows; it is read from the drafts on disk and the log, so a daemon restart
  changes nothing.
- **Equal.** Two proposals or drafts are equal when they have the same kind, the same set of targets
  after following merge forwarding, the same `into` and the same tier. Order does not matter.
- **No duplicates.** A draft equal to a pending draft, or to a proposal open or merged in the log, is
  refused, naming the other. Publishing re-checks it, since an agent may have opened the same
  proposal since; a publish whose content id equals an existing proposal's is refused and points at
  that proposal, so it never lands silently on one the person already decided - with one exception:
  when the existing proposal is this draft's own earlier publish (stamped `surface:dream`, this node
  the acting host, still open), the act resumes where it stopped (5.6).
- **Changed, per kind.** New knowledge about a draft's targets is an observation that is not a
  proposal event, arrived after the frontier in question, that asserts: for `entity_merge`, one of
  the target entities; for `entity_split`, one of the entities of the merge it reverses; for
  `claim_demotion`, an entity, relation or field that one of the target observations asserts.
- **The person's no stands until the knowledge changes.** A draft is refused while it matches a no of
  the person's - ignoring tier, so a refused demotion does not return the next night at another
  tier - and no change, as defined above, has arrived since that no's frontier, which is the
  knowledge frontier at the moment of the act, not the draft's. The person's no is: their discard of a
  draft in the console; a draft's expiry; their `surface:console` reject of a published dream proposal
  cast on this node; and their withdrawal of one. The first two are kept on this node, content-free -
  kind, targets, tier, frontier and the person's note - and the last two are read from the log. A
  rejection that arrived from a peer, or one an agent cast, is not the person's no.

The cap and these rules read the draft store and the log. They are operational checks on what the
dream may add, not folds that decide knowledge: nothing about the canon depends on them.

### 5.6 What reaches the log, and how it is marked

Nothing, until the person acts in the console (Section 8). Then:

- **Publishing a draft** opens the proposal. That is the person's own act of proposing, the same as
  opening a proposal `curate` drafted today; it serializes the candidate on a human decision, which is
  what consolidation.md 7.3 asks of a generated candidate before it becomes an observation. The engine
  stamps the proposal observation with `source_ref = surface:dream` - so the log records that a model
  drafted it - at the trust tier `Unverified`, the lowest, because its rationale is model output (P7),
  with the configured operator as `on_behalf_of`, the evidence as `derived_from` and the draft's
  confidence. The tier of a proposal-open observation feeds no fold today; it is recorded so the
  provenance is honest.
- **Accepting a draft** publishes it and then casts the person's merge verdict; the verdict is stamped
  `surface:console`, as every console verdict is, and grants what the console may grant. The two are
  separate writes, and the draft is kept until both have landed. After a crash between them the draft
  is still there; accepting it again finds its own published proposal (5.5's exception) and goes
  straight to the verdict. A blocking check that refuses the merge leaves the proposal published and
  open, and the console says so; the draft is then removed, since the decision now lives with the
  proposal.

The `surface:` namespace is the engine's: the local ingest surfaces refuse a client-supplied
`source_ref` inside it (resolution.md Section 6, R8), so no agent can claim that the dream drafted
something, and the dream cannot claim the console. Publishing becomes the second place the engine
stamps a marker on the local surface, beside `review_proposal`, and the core comment that names
`review_proposal` as the only one says so. Sync apply does not guard the namespace, by design (a
replicated verdict legitimately carries its marker), so a peer can send an observation stamped
`surface:dream`; this node counts as its own dream's only what is stamped `surface:dream` **and**
whose acting host is this node.

As a second lock (I18 mechanical, not a discipline): **a verdict or withdrawal event every
attestation of which carries `surface:dream` decides nothing in the fold**, whatever its decision.
No legitimate path produces one; the rule is what keeps a bug or a forgetful future surface from
turning the dream into a judge, in the manner I17 already treats a proxied recall verdict.

### 5.7 A demotion only demotes

Building the dream surface's demotion rule exposed a defect in the gate it relies on. A merged gate
proposal grants `min(requested, ceiling)`, and resolution.md R5 lets the HLC-latest merged gate event
per target govern "in both directions". So today a `claim_demotion` that requests a higher tier than
its target holds **promotes** it when merged - from the console up to `human_confirmed`, from the
agent surface up to `host_signed`. That is a promotion under a demotion's name; with I9's demotion
exception it is even self-approvable. P18 allows a promotion only explicitly. Separately, the belief
diff applies the raw requested tier, without the ceiling, so the Review panel can show a grant the
fold will not make.

The fix is a revision of R5, for every node and every surface:

> **R5 (revised).** The merged gate events on a target apply in the order of their representative
> merge verdicts - by HLC, then by proposal id, the tie-break the fold already uses - each from the
> tier the previous one left, starting from the base evaluation. A
> `claim_promotion` leaves `max(previous, grant)`; a `claim_demotion` leaves `min(previous, grant)`;
> `grant` is `min(requested, ceiling)`. The result is a function of the set of merged events, so it
> converges.

A demotion can still push a target below its base - the fast-path R5 exists for - and a promotion can
still lift it above; what neither can do any longer is move the other way. A late-arriving event with
an earlier HLC now changes the result of every later event on the target, not only of the last, and
the registry clause for R5 and its tests say so. The belief diff calls the same function, ceiling
included, so what the Review panel shows is what the fold will do. The dream surface's "below every
target's current tier" check (5.2) is then a courtesy that refuses a draft the fold would ignore.
This is step 0 of Section 12, and worth landing before anything else here.

## 6. The dream run

### 6.1 Turning it on

```sh
supragnosis dream enable --workspace supragnosis --at 03:30 --budget 8
```

It is off by default and stays off until the person runs that: it spends their model budget, and it
drafts in their name. `enable`:

- writes `dream.toml` (the workspaces, the hour, the cap, the operator name, the client) and the
  dream credential (5.1). Its own file, not `supragnosis.toml`, because a `supragnosis.toml` makes a
  node a configured federation node, and a dream must not turn a standalone node into one;
- installs the schedule: a LaunchAgent `com.supragnosis.dream` on macOS, a systemd user timer on
  Linux (deploy/systemd). It is a second job beside the daemon, not a second daemon: it runs a client
  and never opens the store, and the daemon stays the store's one writer (redb admits one writer, and
  daemon-lifecycle.md L1 keeps one daemon);
- runs the session check of 6.3 once with the configured client, and refuses with what it found if
  the client cannot be confined.

`supragnosis dream disable` removes the schedule, then the credential and the file; pending drafts
stay until they expire or the person goes through them.

### 6.2 Which workspaces

Only those `dream.toml` names, one per session (5.2). A shared workspace needs no special treatment:
drafts never leave the node, and what the person publishes is a proposal like any other, in the
workspace it came from, carrying only what was read there.

### 6.3 One run

`supragnosis dream run`, started by the schedule or by hand, for each workspace in turn:

1. **The daemon must answer.** The run reads `/api/dream` on the local viewer socket - a read
   endpoint that gives, per dream workspace, the knowledge frontier, the waiting count toward the cap,
   and how long ago the person last did something (step 3). If the daemon does not answer, the run
   records that and ends; it never opens the store.
2. **Something must have changed.** The knowledge frontier counts observations that are not proposal
   events (prompts.md Section 4 already separates the two) and their newest HLC. If it equals the one
   the last run recorded, the model is not called and no tokens are spent. Proposal events are left
   out so that the morning's verdicts do not schedule the next night's dream.
3. **Nobody may be working.** What counts as the person working: an MCP tool call from a session that
   is not a dream session, and a write act in the console. What does not: reads of `/api/health`,
   `/api/events` and `/api/dream`, which the desktop app makes on a timer and the run makes itself.
   Activity in the last 15 minutes skips the run (P7: off the critical path). A run the schedule
   started more than three hours after its time also skips, so a Mac that slept through 03:30 does
   not dream at 08:00 as the person sits down; a run started by hand has no such window.
4. **The cap must have room** (5.5).
5. **The client is started confined.** In an empty temporary directory, with one MCP server -
   `supragnosis bridge --dream --workspace W` - and no other tool. For Claude Code: `-p` with
   `--input-format stream-json` and `--output-format stream-json`, `--tools ""` (no built-in tool: no
   shell, no file access, no web, no subagent, no skill), `--strict-mcp-config` with an
   `--mcp-config` naming only the dream bridge, `--setting-sources project` in the empty directory (so
   none of the person's user settings load - not their hooks, plugins or permission rules), and
   `--allowedTools` listing the dream surface's tools. (`--bare` would skip more, but it accepts only
   an API key, which would exclude every person who uses Claude through a subscription.)
6. **The run checks the session before the model sees anything untrusted.** In three steps, over the
   stream-json input:
   - **Before any message**, the run sends the client's `mcp_status` control request. It answers
     without a model turn, with every MCP server, its status and its tools; the run continues only if
     there is exactly one server, the dream bridge, connected, offering exactly the dream surface's
     tools.
   - **Then a fixed first message** that carries nothing from the knowledge ("reply with ok"). The
     session's `system/init` event comes with it and lists the tools and plugins the session actually
     has; the run continues only if the tools are exactly the dream surface's and there is no plugin
     beyond the client's built-in ones. The model has seen nothing but that one line.
   - **Only then the `dream` prompt**, whose digest is the first untrusted text the model reads.

   Any mismatch stops the session and is recorded. The check is on the effect, not on the command
   line: `--allowedTools` only pre-approves and removes nothing from the list, which is why the dream
   surface's own `tools/list` carries only its tools (5.2). The init event also lists built-in agents
   and skills; they are inert, because invoking them needs the agent and skill tools the list shows
   absent. What the check cannot see is stated rather than assumed: hooks and an organization's
   managed settings appear in neither answer. User hooks are excluded by the setting sources; managed
   settings are the organization's, outside what a person's dream can decide.
7. **It has a time limit.** Twenty minutes, then the client is stopped. What it drafted stays.
8. **It is recorded** (6.4).

Checked on the author's machine on 2026-10-16, with the flags of step 5:

- With an empty MCP configuration the init event listed no tools and no servers. With the existing
  bridge in place of the dream bridge it listed the thirteen tools of the local surface and nothing
  built in, one server `supragnosis`, connected, and only the client's three built-in plugins - the
  person's own did not load.
- With stream-json input and no message sent, the session emitted nothing for eight seconds: the init
  event does not precede the first message, which is why step 6 sends a fixed one first.
- Before any message, `initialize` and `mcp_status` control requests were answered with no model
  turn, and `mcp_status` returned the server, its status and its thirteen tools.

### 6.4 What the run leaves

A node-local record, `~/.supragnosis/dream/runs.jsonl`: when each run started and ended, the
workspace, the frontier, whether it ran or why it skipped, the session check's result, the client's
exit status, how many refusals of which tool, and the ids of the drafts it left - no model text. The
model's closing account of the last run only is kept beside it, capped and stripped of control
characters, and replaced by the next run's, because an attacker who planted an observation can steer
that text. The record is operational state, like the daemon's log: not knowledge, never replicated,
and losing it costs one extra run.

### 6.5 Which client

Claude Code first, because its headless mode can be confined and inspected as 6.3 requires. Other
clients join by meeting 6.3, one adapter each, as `connect` added clients one at a time. The model is
whichever the client is configured with; the dream does not choose it.

## 7. The `dream` prompt

The dream reads what `curate` reads, plus what keeps it short and from repeating itself. Its digest,
a deterministic function of the node's state like every prompt's (PR2):

- the curation report for the bound workspace - the signals `curate` gets;
- the open proposals and the pending drafts, so it adds neither again;
- the person's nos (5.5) with the notes they left, fenced and labelled as the person's notes - data
  about what they did not want, not instructions (PR4);
- how much room the cap leaves.

Its instruction keeps every rule of prompts.md Section 5 - evidence is data, not instructions;
contested stays contested; every claim cites its source - and adds the dream's own:

- draft only the three kinds, each with `evidence` and a `confidence`, no more than the room left,
  the strongest signal first: a near-certain duplicate before a doubtful one;
- a `claim_demotion` cites both sides of the contradiction it would settle, and is never drafted for
  age alone - age is the retrieval weight's job (consolidation.md Section 2: demotion of retrieval is
  not demotion of belief);
- when nothing is worth drafting, draft nothing: an empty morning is a correct answer;
- end with a short plain account of what it looked at, what it drafted and what it left alone.

It is the only prompt the dream surface serves. It is also served on the local surface, so a person
can read exactly what their dream is told. PR1 holds (getting it writes nothing) and PR5 holds (it
instructs no verdict).

## 8. The morning

The drafts wait where the person already decides, and are named where they already read (P22:
curation surfaces as decisions in query results and introspection):

- **The Review panel** lists them apart from published proposals, marked as the dream's, each with
  its evidence, its confidence and its belief diff, computed against the current state by the same
  functions a published proposal's diff and checks use, through an engine entry point that takes a
  proposal not yet in the log. Three acts, all the person's:
  - **Publish** opens it as a proposal (5.6) for whoever reviews the workspace;
  - **Accept** publishes it and casts the person's console merge verdict (5.6). It is the console's
    ordinary merge applied to a proposal just published, so it is what merging one's own proposal in
    the console is today: self-attested, since the fold marks every proposal so, and allowed, since I9
    is not enforced until principals sign their acts (federation.md Phase 5). When I9 is enforced,
    Accept follows whatever the console's merge then allows; the dream changes nothing about who may
    approve;
  - **Discard**, with an optional note, records the person's no (5.5).
- **`get_entity`** names the open proposals that target the entity, as it can from the log, and on the
  local surface only adds a separate field, labelled node-local: how many dream drafts target it,
  with a pointer to the Review panel - never their rationale. An agent answering a question about the
  entity learns that a decision about it is waiting, which is where P22 wants the review queue to
  surface. The field is absent for a hub's principals and on every read tier, and its being
  node-local is said the way `mode` says which surface answered: two nodes holding one log answer the
  converging part alike.
- **At the start of a session**, the capture hook's line (client-connect.md Section 12) adds the
  count: "3 dream drafts wait in the Review panel".
- **`brief`** names the pending drafts beside the open proposals.
- **`supragnosis dream status`** prints the last runs and the dream's track record: how many drafts
  the person accepted, published, discarded, or let expire, and of the published ones - stamped
  `surface:dream` with this node as the acting host - how many were merged. A dream whose drafts are
  mostly discarded costs the person attention and tokens for nothing, and this is where they see it.

The console's draft acts are writes on the local viewer socket. Like every console write they exist
only on the local trust surface: each new viewer path declares its read-tier policy as refused (the
registry already fails a viewer route without one), and the proxy guidance for a viewer behind nginx
(deploy/docker) lists them among the writes to refuse.

## 9. The deterministic half

Retrieval demotion is consolidation.md's, not the dream's, and it does not wait for a model. Its
order is consolidation.md Section 8: the materialized span (step 2), which first needs the sync path
to advance `log_epoch` (the defect consolidation.md records), then the weight in the ranked surfaces
(step 3).

This document decides one thing consolidation.md left for last, step 6's scheduling: when step 2's
materialization runs on a timer, it runs **inside the daemon**, on the daemon's own idle (the person
not working, as 6.3 step 3 defines it, for 15 minutes; at most once an hour), because the daemon is
the store's one writer. The dream job never writes the store and never triggers a materialization;
the two halves share an idle definition and nothing else. A node with the dream turned off still gets
its retrieval weight.

## 10. What an injected instruction can do

The dream reads untrusted text unattended, so its design is judged by what an attacker who wrote an
observation could make it do.

- **Inside supragnosis:** leave drafts, up to the room the cap leaves, in the one workspace the
  session is bound to, of three kinds, each with evidence that exists there, none of them in the log,
  none able to raise a tier. It cannot write an observation, open or decide a proposal, comment,
  retract, define a type, sync, or read or write another workspace. A draft that slips past the
  person's attention and is accepted does what the person accepted, through the console, under the
  console's ceiling, and can be undone by a new proposal (P3).
- **On the machine:** nothing the session's own tool list allows. The session has no tool but the
  dream surface, and the run sends the model nothing until the session itself shows that (6.3).
  Hooks and managed settings, which that list cannot show, are kept out by the setting sources or
  are the organization's.
- **What remains:** a person who accepts what they did not read. The cap keeps the queue short, the
  duplicate and no rules keep it from repeating, stale drafts are superseded, the evidence and
  the diff are in front of them, and `dream status` shows when a dream should be off. Nothing in
  software removes the last of it, and this document does not claim to.

## 11. Invariants

| | Invariant |
|---|---|
| **DR1** | A dream writes nothing to the log. The dream surface refuses `observe`, `review`, `define_type` and sync, and its `propose` writes a node-local draft; only the person's publish or accept in the console enters the log. |
| **DR2** | A verdict or withdrawal event every attestation of which carries `surface:dream` decides nothing in the fold, however it reached the log. |
| **DR3** | A dream drafts only `entity_merge`, `entity_split` and `claim_demotion`, each with evidence in its workspace and a confidence, which become the published proposal's `derived_from` and `confidence`. A `claim_demotion` draft requests a tier below every target's current one. |
| **DR4** | Merged gate events on a target apply in HLC order, each from the tier the previous left: a demotion leaves `min(previous, grant)`, a promotion `max(previous, grant)`, `grant = min(requested, ceiling)`; the belief diff uses the same function. |
| **DR5** | A published dream draft is engine-stamped `surface:dream` at the lowest tier on behalf of the configured operator; no client can supply the marker, and this node counts as its own dream's only what carries the marker with this node as the acting host. |
| **DR6** | A dream session is bound to the workspace of its first request; any other workspace, `*`, and ids outside it answer as refused or absent for the rest of the session. |
| **DR7** | At most the cap of dream items wait at once on a node - pending drafts and open published dream proposals - and a draft equal to a pending draft or an open or merged proposal, or matching a no of the person's with no change since, is refused; a publish equal to an existing proposal is refused. |
| **DR8** | A dream run calls no model when the knowledge frontier is unchanged, when the person has worked in the last 15 minutes (MCP tool calls of other sessions and console writes, not the app's reads), when a scheduled run is more than three hours late, or when the cap is full. |
| **DR9** | A dream session has no tool but the dream surface. The run sends the model nothing from the knowledge until the client's `mcp_status`, before any message, shows only the dream bridge with only the dream tools, and the init event that comes with a fixed first message shows exactly the dream tools and no plugin beyond the client's built-in ones. |
| **DR10** | A dream run never opens the store, and the dream surface exists only on loopback, with authentication on, and only while `dream.toml` and its credential exist, both read on every request. |

## 12. Ordering

0. **A demotion only demotes** (5.7, DR4). resolution.md R5 revised, its registry clause and tests,
   and the belief diff on the same function with the ceiling. Independent of everything else here and
   worth landing first: it closes a promotion path that exists today.
1. **Evidence on `propose`.** The optional `evidence` argument recorded as `derived_from`, on every
   surface. Useful alone: `curate`'s proposals gain lineage too.
2. **Drafts and the dream surface.** The draft store with its scan, staleness and expiry; the
   credential and `dream.toml` read per request; `bridge --dream --workspace` and the session binding;
   the per-tool policy and the filtered `tools/list`; the cap and the duplicate and no rules;
   `/api/dream` with the activity definition; the engine entry point that computes a diff and checks
   for a proposal not in the log; the stamp at publish and the fold rule (DR2).
3. **The `dream` prompt.** Its digest and instruction, on both surfaces, in the pinned prompt list.
4. **The run.** `dream enable / disable / run / status`, the LaunchAgent and the systemd timer, the
   Claude Code adapter with its stream-json session check, the run record.
5. **The morning.** The Review panel's drafts with publish, accept and discard; `get_entity`'s and
   `brief`'s mention; the capture hook's count; the track record.
6. **Documents.** README, architecture.md's companion list and Section 14, resolution.md R5,
   excision.md's closure (the draft store and the run record), the core comment on the marker
   namespace, the deploy guides and the viewer proxy's list of writes to refuse.

The deterministic half (Section 9) is consolidation.md's track and does not wait for any of this.

## 13. Closure map

| Principle | Where this closes it |
|---|---|
| P1 - no API writes a fact that did not pass through an assertion | DR1: the dream writes nothing; the person's publish is an assertion of a candidate |
| P2 - provenance first-class | 5.6, DR5: the log records that a model drafted it, on whose behalf, at what trust, with what confidence |
| P3 - nothing is destroyed | Sections 3, 4: no removal; every accepted change is reversible by a new proposal |
| P5 - absence is not falsehood | 5.2 (out-of-scope ids answer as absent), Section 7 (an empty morning is correct) |
| P6 - contradictions surface, not settle | Section 7: a demotion cites both sides and waits for the person |
| P7 - consolidation generates, never commits; off the critical path; model output at lowest trust, with confidence and lineage | Sections 2, 5, 9; DR1, DR2, DR3, DR5, DR8; a stale draft is superseded (5.3) |
| P9 / P23 - no merge without a diff | Section 4: `tbox_change` and `recall` refused until they have commit effects; drafts show the diff the fold will apply (5.7) |
| P11 - induction goes as far as a proposal | 5.2: `define_type` refused; type drafts wait for `tbox_change`'s commit effect |
| P15 - resolve conservatively | Section 3: merges and their reversals are drafts, never automatic |
| P16 - determinism in folds; convergence on the deterministic read surfaces | 5.7, DR4: R5's revision is a function of the set of merged events; DR2 is a fold rule. The cap and the duplicate and no rules (5.5) and the run's clock checks are operational and decide no knowledge; `get_entity`'s draft count is a separate node-local field (8) |
| P17 - sovereignty; local surfaces stay local | 5.1, 5.2, 6.2, DR6, DR10: one bound workspace per session, loopback only, drafts never leave the node |
| P18 - writes are an attack surface; lineage; trust rises only explicitly | 5.4, 5.7, Section 10, DR3, DR4, DR9 |
| P19 - the model drafts, never decides | Sections 4, 5; DR1, DR2 |
| P20 - hexagonal purity | the client is an adapter outside the engine; the engine gains a stamp, an argument, an entry point and a fold rule |
| P21 - narrow surface; errors an LLM can correct from | 5.2, 5.4: no new tool, one optional argument, refusals that say why |
| P22 - curation is not separate labor | Sections 1, 8: decisions waiting where the person reads, counted in `get_entity` on the local surface, named in `brief` and at session start |
| P23 - the gate to canon; I9; I17 | Sections 4, 5, 8; DR1-DR4; I9 unchanged and stated (8), I17 untouched (recall refused) |
| P24 - degrade loudly | 5.1, 6.1, 6.4: a bad `dream.toml` disables the surface and says so; every skip and refusal is recorded |
| proposal-workflow.md I1 - no side store for proposals | a draft is the Draft state that precedes a proposal (Section 4 of that document); once published, everything about it is an observation. A discard decides about a draft, which no proposal yet exists for; a decision about a published proposal - a reject, a withdrawal - is in the log and is read from there (5.5) |
| consolidation.md 7.3 and C11 - a generated candidate becomes an observation only on a human decision; only commitment replicates | 5.6: publish is the person's act; drafts are node-local |
| consolidation.md C12 - no partial curation reported | Section 4: kinds without a commit effect are refused |
| consolidation.md step 6 - idle scheduling | Section 9: in the daemon, not in the dream |

## 14. What the tests assert

The tests are named when they exist; what each asserts is fixed here.

- **A demotion only demotes.** A merged `claim_demotion` requesting `human_confirmed` over an
  `agent_extracted` target leaves it at `agent_extracted`; a merged `claim_promotion` requesting
  `unverified` over a `host_signed` target leaves it at `host_signed`; a demotion still pushes below
  base; two gate events on one target apply in HLC order, whichever arrived first; the belief diff
  shows the same tier the fold grants, ceiling included.
- **The dream surface refuses what it must.** Over a dream credential: `observe`, `review` with any
  decision, `define_type`, `sync_*`, drafts of `claim_promotion`, `recall` or `tbox_change`, a
  `claim_demotion` draft not below its targets - each refused, naming the reason; `tools/list` lists
  only the dream tools; the log is unchanged.
- **A session is bound.** After a first request for workspace A, a request naming workspace B, also in
  `dream.toml`, is refused in that session, in the binding and in a tool argument; `*` is refused; an
  id in B answers as absent.
- **A dream `propose` writes a draft, not an observation.** After it, the log is unchanged and the
  draft store holds it with its evidence and confidence; without evidence, with an id outside the
  workspace, or with credential-shaped text, it is refused.
- **The cap and the person's no.** With the cap filled by drafts and open published dream proposals,
  the next draft is refused and `dream status` names the split; a published proposal older than seven
  days no longer counts; a draft equal to a pending one or to an open or merged proposal is
  refused; one matching a discard, an expiry, or a console reject cast on this node is refused at any
  tier until a change of the kind's definition arrives, then accepted; a peer's or an agent's reject
  does not count; a publish equal to an existing proposal is refused.
- **A stale draft is superseded.** After a change touching its targets, the draft is gone as
  superseded, which is not a no: the next run may draft it again.
- **`get_entity` keeps drafts local.** On the local surface it carries the node-local draft count and
  no rationale; for a hub's principal and on every read tier it has no such field.
- **Publishing stamps it.** A published draft is a proposal observation with `surface:dream`,
  `Unverified`, the operator as `on_behalf_of`, the evidence as `derived_from` and the confidence;
  accept adds a `surface:console` merge verdict; a blocked accept leaves the proposal open and says
  so; an accept interrupted after the publish keeps the draft, and accepting it again casts the verdict
  on the proposal already published.
- **The fold refuses a dream verdict.** A `verdict_cast` or withdrawal stamped `surface:dream`,
  written to the log directly, leaves the proposal as it was.
- **No model is called for nothing.** A run over an unchanged knowledge frontier, with a non-dream MCP
  tool call a minute old, more than three hours late on a schedule, or at the cap, starts no session;
  with the desktop app running and polling, the run is not skipped for the polling; a verdict alone
  does not change the frontier.
- **The session is checked before the model reads anything untrusted.** A session whose `mcp_status`
  names a server beyond the dream bridge or a tool beyond the dream surface's is stopped before any
  message; one whose init event, after the fixed first message, lists a built-in tool or a plugin
  beyond the built-in ones is stopped before the `dream` prompt; `enable` refuses a client that fails
  the check.
- **The credential follows the file.** After `disable`, a request with the old dream token is refused
  by the running daemon; with authentication off, the dream surface refuses every request.
- **The track record is a fold plus the draft store.** Its counts equal the draft outcomes on disk
  and the proposal folds for what this node's dream published.

## 15. What building it changed

Written when it is built.

## 16. What the reviews changed

The document was read twice against the principles and the code by a reviewer who had not seen its
reasoning, and revised after each reading.

**The first reading:**

- **The dream's proposals replicated before anyone accepted them**, against consolidation.md 7.3 and
  C11. The dream now writes node-local drafts - the Draft state proposal-workflow.md Section 4 already
  defined - and only the person's act enters the log.
- **A demotion could promote** (5.7), since a grant has no direction. R5 is revised for every node.
- **Reading several workspaces in one session could carry private text into a shared one.** One
  workspace per session.
- **`tbox_change` had no computable diff**, the defect the draft used to refuse `recall`. Refused too.
- **The confinement was a promise about flags.** It became a check on the session's own tool list.
- **Comments let the dream endorse proposals it could not open.** The dream comments on nothing.
- **The budget limited a night, not the queue**; **"the person's no" counted any rejection and fed its
  text to the model as guidance**; **the dream's own output would have scheduled the next dream**;
  **the credential would have outlived `disable`**. Each is fixed where 5.5, 5.1 and 6.3 say.
- Smaller corrections: the lowest tier for model output, the verdict fold rule's shape, attribution by
  marker and host, the digest's determinism, `entity_split`'s meaning, excision's scope.

**The second reading:**

- **The idle check could never pass while the desktop app ran**, since the app reads the viewer
  socket every minute and holds its event stream open. Activity is now MCP tool calls of other
  sessions and console writes (6.3 step 3).
- **Nothing bound a session to one workspace.** The bridge carries the workspace and the daemon binds
  the session to it (5.2).
- **"The tier before the grant" was undefined when several gate events target one assertion**, and the
  belief diff ignored the ceiling. R5's revision is a sequential application in HLC order, and the diff
  uses the same function (5.7).
- **The session check ran after the prompt was already sent, and plugins were called inert though
  their hooks need no tool.** The prompt now waits for the check over stream-json input, and only
  built-in plugins pass (6.3 steps 5-6); what the check cannot see is stated.
- **A stored draft could show a rationale about a state that no longer holds**, and drafts and the run
  record sat outside excision's reach. Stale drafts are dropped, the run record keeps no model
  text but the last account, and excision.md's closure gains both (5.3, 6.4).
- **"The person's no" leaked**: "changed" was undefined for two kinds, tier let a refused demotion
  return, and the person's own console reject did not count. All three are defined (5.5).
- **The cap left out published dream proposals and expired drafts**, and was per workspace. It is now
  per node, counts both, and expiry is a no (5.5).
- **Accept was two writes with no failure story**, **diffs for drafts had no entry point**, **I9 was
  claimed as policy that does not exist**, **evidence would put proposals into recall closures without
  saying what that means**, **P7's confidence was missing**, and **authentication off was checked only
  at enable**. Each is now stated where 5.6, 8, 5.4 and 5.1 say.

**The third reading, and a measurement:**

- **`get_entity` naming drafts would have served them to a hub's principals and broken convergence on
  an exact-lookup surface.** It now carries only a node-local count, on the local surface only (8).
- **A stale draft forced a discard the person never meant**, and a no's frontier was unspecified.
  Staleness is supersession, not a no; a no records the frontier at the act (5.3, 5.5).
- **Accept's retry was refused by the duplicate rule it relied on.** The draft is kept until both
  writes land, and its own earlier publish resumes instead of being refused (5.5, 5.6).
- **Published proposals waiting on other reviewers could stop the dream everywhere, silently.** They
  count for seven days, and `dream status` names why a run skipped (5.5).
- **The session check was said to run before the prompt, which had not been measured.** Measured: the
  init event does not come before the first message, and `mcp_status` answers before any message with
  no model turn. Step 6 now checks the servers and their tools first, sends a fixed harmless message,
  checks the init event, and only then sends the dream prompt (6.3).
