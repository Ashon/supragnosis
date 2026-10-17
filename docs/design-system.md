# Design system - one look, one set of parts, and a map that shows who said it (Principles 5, 6, 18, 23, 24)

> The viewer, the desktop shell's own pages and the landing are one product drawn three ways. This
> document fixes what they share - the tokens, the type, the parts - and the places where a rendering
> choice carries a principle stake: how the map draws trust and contradiction, where a decision is
> made, what the viewer's search answers with, what an absent indicator means, and how a write reports
> back. Companion to [inspector.md](inspector.md), whose rules for the detail panel stand unchanged.
>
> Status: **specification, with Section 13 steps 0-1 built** - the fixes that needed no document,
> then the tokens and the foundations.

## 1. Why this exists

Measured on 2026-10-17, on a copy of the author's store (168 entities and 274 relations in its
largest workspace, 7 in its smallest), in Chrome at 1440x813 and at 760 and 390 pixels wide, and in
the desktop shell at its default size:

- **Three token sets.** The viewer declares 20 custom properties, the settings page 23, the landing
  16. Most are the same values under different names (`--ink` and `--text`, `--accent` and
  `--gold`), and some are not the same values: `--panel` is `#10131b` in two files and `#0e1118` in
  the third, and red is `#d96a5f` in the viewer and `#e07a5f` on the settings page.
- **No type scale.** The viewer drew text at eight sizes from 9 to 12.5px, half a pixel apart; 48
  visible elements in the default view were 9px. Nothing reads as a heading.
- **Trust is not on the map.** The canvas encodes type, degree, contests, the session's footprint,
  activity and boundaries, but no tier. A node supported by a human's confirmation and one supported
  only by an unverified source look alike until each is opened.
- **Rings mean six things.** A contested node, a node this session touched, a pulse of activity, a
  bridge in group mode, the selected node and a merge preview's targets are each drawn as a ring,
  told apart by color and dash.
- **The panels cover the map.** The two docks and the minimap cover about 35% of the canvas, and
  about 58% once the inspector opens at the bottom. The viewer has no media query: at 390 pixels the
  docks overlap each other, and at 760 little canvas is left.
- **Decisions are in two tabs of five.** Proposals and Review are separate tabs of a dock whose labels
  were cut off. Types and Peers each hold rows that exist nowhere else (Section 6), under names that
  read like duplicates of the legend and of the app's Settings > Sync.
- **Search filters names.** It matches node names by substring, shows no list and no count, and
  cannot find an observation, or a thing by what it means. The MCP search the agents use can.
- **System words on human surfaces.** "grab-bag contexts", "reify", "BLOCKED 1V", "same across this
  scope: agent_extracted / from localhost", "accepted entity_merge [blocked]: c9a66148 -> T2".

Six defects needed no design to fix and shipped first (Section 13 step 0): the console's writes
reported nothing, type colors followed the view's order, faint text measured 3.3:1, Esc did not close
the inspector, the dock's tab names were cut, and the shell's splash pointed at tray items that had
moved. What remains carries principle stakes, which is why it is written down before it is built.

## 2. What this is NOT

**It is not a change to what the server decides.** No fold, check, tier or verdict changes. One read
path is added (Section 8), and nothing the server answers today is removed or narrowed.

**It is not a new MCP surface.** The viewer is a separate human channel (P21). No tool, prompt or
resource changes; the viewer's search calls the function `search_knowledge` calls, over its own path.

**It does not reopen the inspector's rules.** The silence rule, the scope line and D1-D7 stand,
including the contested block's inline actions (D1). This document moves the panel, renames its
lines in plain words and adds marks to the map, and every mark it adds reads the same effective tier
the inspector's baseline reads (inspector.md D4).

## 3. Tokens

One vocabulary, the one the viewer and the settings page already share, with one value each:

| Token | Value | For |
|---|---|---|
| `--surface` | `#08090d` | the canvas and the page ground |
| `--panel` | `#10131b` | panels, sheets, cards |
| `--panel-2` | `#131722` | raised controls, hover |
| `--panel-3` | `#181d2a` | icon wells and badges on a raised control |
| `--line` / `--line-soft` | `#222836` / `#1a1f2b` | rules and borders; never text |
| `--ink` | `#e9e4d6` | primary text |
| `--ink2` | `#aab1bd` | body text |
| `--muted` | `#8e96a5` | secondary text |
| `--faint` | `#7f8898` | meta text, placeholders |
| `--accent` / `--gold-bright` | `#d9a544` / `#f0c469` | the accent, and human-confirmed |
| `--ok` | `#56b3a2` | success, connected, healthy |
| `--warn` | `#e0a24a` | needs attention |
| `--bad` | `#e07a5f` | error, blocked, contested |

The landing renames to match (`--ink-0` to `--surface`, `--text` to `--ink`, `--gold` to `--accent`),
the viewer's `--teal` becomes `--ok`, and its literal `#d96a5f` becomes `--bad`. A surface declares
the tokens it uses; the core - `--surface`, `--panel`, `--line`, `--ink`, `--muted`, `--faint`,
`--accent` - is declared by all three. Every text token measures at least 4.5:1 on every surface
token. `--faint` moves once more for that, from step 0's `#7b8494` (4.46:1 on `--panel-3`) to
`#7f8898`, whose lowest pair is 4.71:1.

Space runs on a 4px step (4, 8, 12, 16, 24, 32, 48). Corners are 4px for chips, 8 for controls, 12
for panels and fully round for pills. Layers are named, lowest first: canvas, HUD, dock, sheet,
popover, dialog, notice. Motion uses three durations (120, 200 and 320ms) and one ease-out curve.

**Each surface keeps its own copy, and a test holds them together** (U7). The viewer's three files
are compiled into the binary and served under a policy that names no other style source, the
settings page carries its own policy, and the landing is a static site. A generated shared file would
add a build step to all three for what one test gives: it reads the three `:root` blocks, fails when
a token declared in more than one differs, and computes the contrast of every text token on every
surface token.

## 4. Type, focus and the selected state

- **Two families, as viewer.css already decided.** The system sans for what a person reads, with the
  Hangul faces named early in the stack, and IBM Plex Mono for identifiers: relation kinds, ids,
  hashes, counts. The app and the viewer load no web font; the landing keeps its own display faces.
- **Six sizes**: 24 (page title), 18 (sheet title), 15 (section), 13 (body), 12 (secondary) and 11
  (labels). Nothing is drawn below 11px. An uppercase label is 11px mono with 0.08em tracking.
- **Focus is visible on every control**: a 2px `--gold-bright` ring, offset 2px. Today only the
  inputs show one.
- **A selected or highlighted item is tinted and bordered all round.** A rounded box never takes a
  rail on one side: the rail bends around the corners and reads as a bracket. A rail is drawn only on
  a square-cornered row.

## 5. The map

One channel, one meaning:

| Channel | Encodes | Notes |
|---|---|---|
| fill | type | keyed by name (step 0) |
| size | degree | as today |
| ring | trust of the best support | Unverified: dashed, `--muted`. Agent: none. Host signed: thin, `--ink`. Human confirmed: solid, `--gold-bright` |
| badge | contested | a `--bad` disc with "!" at the node's upper right |
| halo | selected | a soft `--ink` disc behind the node, replacing the selection ring |
| glow | this session's footprint | a soft `--ok` light under the node, replacing the teal ring |
| pulse | activity | one expanding ring, none under reduced motion |
| edge | relation | neutral by default; kind color and label only on the active node's edges |
| region | context (hyperedge) | only the active node's contexts by default |

**Rings belong to trust.** The selection ring becomes a halo, the footprint ring a glow, and two
marks that draw rings today change shape: a merge preview shows its fold arrows and dims the rest
instead of ringing its targets, and a bridge in group mode is a small square beside the node. Only
the pulse stays a ring, and it lasts one second.

**What the ring reads, said on the map.** The ring draws `GraphNode.trust_tier`: the highest
effective tier among the observations that support the node, receiver-evaluated, never a claimed one
(U2). It is the trust of the node's best support, not of its winning value, which may stand on a
lower tier or be contested. The legend states exactly that, beside the four ring styles.

**The map is silent the way the inspector is.** When every node of the loaded scope has one tier, no
node carries a ring, and the legend states the tier for the scope, as the inspector's scope line does
(inspector.md Sections 3-4, D3). When tiers differ, each node's ring shows its tier, and Agent - what
`observe` asserts unless something raises or lowers it - is drawn as no ring. A hub whose every node
is unverified therefore shows no rings and says so, rather than a dashed ring on every node.

**A contested node is marked wherever it is drawn** (U3). Label thinning, zoom and focus dimming
leave the badge at full strength. A type the legend hides is not drawn at all, so the status bar's
contested pill counts the contested nodes of the loaded scope whether the legend shows them or not
(P6).

**Colors are keyed by name.** An FNV-1a hash of the name picks one of 48 slots of the generated band,
and a slot already taken in the view probes forward in name order. Two types never share a color on
one screen, and a type keeps its color in every view where no earlier name collides with its slot -
which is not every view, and the legend is what names a color.

**Regions are drawn for the active node** and named by two of their members other than it, with the
count: "proposal gate, claim_demotion defect (4)". Today every context of a hub is named after the
hub, so selecting one titles five regions alike. A view toggle still draws them all. Region names
sit outside the region's upper edge, and node labels avoid them and each other, larger degree first.

**The layout is remembered on the local console.** Node positions are kept per workspace in the
browser's storage, keyed by the node's id from `/api/surface` as well as the workspace, and the next
load starts from them: a known workspace appears at once, instead of behind a loader that, for 168
nodes, held the map for more than four seconds, once more than eight. The read tier remembers
nothing: the app shows every server from one origin, and positions of a principal's knowledge would
outlive a revoked grant. A position is a convenience; losing it costs one settling.

## 6. Places

The window has three pages: the **Graph**, **Review** (Section 7) and **Settings**. Graph and Review
are routes of the viewer's own page (`#review`), so they work in the app, on a hub's read tier and in
a browser alike; Settings is the app's page (settings-page.md Section 6). The app's View menu holds
Graph (Cmd+1) and Review (Cmd+2), and the status bar's gear opens Settings, as it does today. In a
browser, where those keys belong to the browser, the review pill and the address reach the page. The
title bar holds the name and the search and no navigation.

On the Graph page:

- **The left dock is the filters**: workspaces, types, relations, boundaries. The Types tab's rows
  move here before the tab goes: a contested type definition with its confirm act, an axis
  collision, and the types that are defined but have no node in the loaded graph. The legend's hover
  shows only the winning definition; the full entry opens in place.
- **The right side shows one thing at a time**: the inspector as a sheet while a node is focused,
  otherwise the Log. The inspector moves there from the bottom, where it covered the part of the map
  being inspected. Its two regions, relations then evidence, stack in the sheet (inspector.md Section 6).
- **The status bar is the window's state**: the scope and its counts on the left; on the right,
  pills for what waits, what is contested, sync and semantic search, then the gear.

The dock's Peers tab is the only place that lists known peers with "not admitted", "admitted, not
shared" and "surface unknown". Those rows move into Settings > Sync, and into the viewer's own
settings dialog outside the app, before the tab becomes the sync pill. On the read tier there is no
peers view and no sync pill (remote-viewer.md Section 6).

**A pill's absence means one thing** (U8): a known zero, or a feature this node does not have set
up. Unavailable, refused and not yet known are states the pill draws, never reasons to leave it out
(P5, P24). The review pill is absent on the read tier, where nothing can be decided, and the read
tier's banner already says why.

By width: from 1200 pixels the filters, the canvas and the sheet sit side by side; from 760 one panel
shows at a time over the canvas; below 760 the panel is a sheet from the bottom.

## 7. The Review page

Everything that waits for a person's decision, in one place, split in three:

- **Needs you**: suspected secrets first, as an alert; then open proposals; then this node's dream
  drafts, listed apart from published proposals and marked as drafts (dreaming.md Section 8); then
  contested beliefs.
- **Suggestions**: merge suggestions, name variants and cross-workspace collisions. Each offers
  "Propose", which opens a proposal and decides nothing.
- **Health**: overloaded contexts, unlinked entities, merge cycles and type axis collisions, folded
  by default. P22 wants curation where the reader already is, not a second job.

An item shows, in this order: what it would do in one sentence; for a draft, its confidence; the
belief diff, before and after (proposal-workflow.md Section 14.2); each check, and for a failing one
the reason in words and the act that fixes it where there is one; the evidence, each observation with
who said it and when; and the preview on the map that the console draws today.

**The decision controls sit beside that and act on that one item** (U4). For a proposal: Accept,
Reject and Skip, also as A, R and J, with K to step back. For a draft: Publish, Accept and Discard,
as dreaming.md Section 8 names them, also as P, A and D. There is no decide-all. A key decides only
once the item's diff and checks are on screen, so stepping fast through the list cannot cast a verdict
on an item not yet shown. A recall asks for a second, explicit act: its effect is the strongest the
console casts, and one keystroke should not carry it.

**What follows a decision is said plainly.** An accepted merge offers "Un-merge", which opens the
split proposal (unmerge.md) and states its two consequences: the separated pair is never suggested
again (unmerge.md S5), and merging them later needs a proposal distinguishable from this one
(unmerge.md S10). A rejected proposal stays listed among the decided, since a reject is provisional
and a later accept still merges it (proposal-workflow.md Section 8).

**What is listed.** Every open proposal is listed, whatever its diff: one whose diff cannot be computed
says so, since nothing else would ever close it. A generated candidate - a suggestion, a draft - is
not listed when its diff is computable and empty (proposal-workflow.md Section 14.2). A draft leaves
the list by the person's act or by the expiry dreaming.md Section 5.3 sets, which counts as the
person's no.

**The inspector keeps its own act.** A contested belief's "use this" in the inspector stays where it
is, under D1: it is the mediation act at the point of reading, and it is listed on the Review page
too. U4 governs the Review page's controls.

The count of items under Needs you is the status bar's review pill and, on this Mac's daemon when it
is not zero, a "Review (N)..." item in the tray menu. On a remote profile the tray stays as
remote-viewer.md Section 6 has it.

On the read tier the page lists and explains, shows no dream draft (dreaming.md Section 5.3 serves
none there) and offers no control the tier refuses, as remote-viewer.md Section 6 requires of the
whole page.

## 8. Search

Cmd+K, or "/", opens a search bar over the canvas. The query goes to a new read path,
`/api/search?q=...&workspace=...`, which calls the engine function behind `search_knowledge` and
answers what it answers: entities and observations in its deterministic order, with the mode label
saying whether the keyword or the hybrid surface answered (P19). The bar groups the two, Enter focuses
an entity or opens an observation's card, and the canvas frames the result. While the bar is open,
the map lights the entities the search returned, not names that merely contain the typed text, so
the list and the map give one answer.

The path's read-tier policy is `Read`, never `Served`: the workspace is resolved over the reader's
grants as `/api/graph` resolves it (remote-viewer.md V3). A union - `*` on the read tier - is searched
per granted workspace and merged by score, then id, and cut to the limit, the rule MCP's remote search
uses; it is never a node-wide search ranking across workspaces the reader may not read. Entity hits
are forwarded through accepted merges on the server before they are answered, so focusing a hit never
lands on a merged-away id the map does not draw (inspector.md D5). An empty answer says what the
agent surface says: absence in the workspaces read is unknown, not a negation (P5). The path adds
nothing to the MCP surface (U6).

Searching the loaded graph in the page was the cheaper design and was rejected: the page holds names,
not observation text or the semantic index, and a second ranking would answer a person differently
from the agent sitting beside them.

## 9. Notices and words

**Every write answers where it was made** (U1, built in step 0): what was done, the server's own
reason for a refusal, "This view is read-only" for a 403, or that nothing changed when the server
cannot be reached. The notices are the settings page's in shape: a success leaves after 4.5 seconds,
an error stays until it is dismissed, four at most, in a polite live region.

**Words are the reader's.** The interface stays in English. A system term may stay in a tooltip or a
disclosure, where it links the screen to these documents, and never in the line a person reads first.
Hashes and ids live in footers and cards. One line never states two contradictory states. A host is
named by its id; it is called "this node" only when the id is the one `/api/surface` reports for the
node serving the page, since every unconfigured node calls itself `localhost`.

| Today | Shown |
|---|---|
| the Proposals and Review tabs | Review |
| BLOCKED 1V | Blocked - 1 check fails |
| same across this scope: agent_extracted / from localhost | All here: Agent, from localhost (this node) |
| accepted entity_merge [blocked]: c9a66148 -> T2 | You accepted merging into "T2". A check blocks it. |
| grab-bag contexts / reify | Overloaded contexts / Name this group |
| merge cycles / name variants / orphans | Conflicting merges / Possible duplicates / Unlinked |
| Healthy - nothing owed | Up to date |
| Peers, client, local +N | Sync, Spoke, this node +N |
| settling layout... | Arranging 168 items |

## 10. Keyboard, motion and contrast

- Every control is a `button` or an `input`. Chips, legend entries, proposal rows and log rows stop
  being clickable spans. Tab lists carry their roles and move with the arrow keys.
- The map is reached through search, and from a focused node its relations are a Tab away in the
  sheet. Esc closes the topmost card, menu or sheet (step 0).
- Under reduced motion nothing moves: no pulse, no marching dashes, no sliding docks, no spinner, and
  the layout settles in one frame. Today only the settling obeys it.
- Text drawn at full strength on a token surface meets 4.5:1 (U7). Two things are outside the pairs
  on purpose: labels the map dims to put the focus forward, and text over the panels' translucent
  glass, whose ground is whatever the canvas shows through. The glass is dark enough that `--muted`
  stays above 4.5:1 over the canvas's own ground, and that is checked by measurement, not by the test.

## 11. Decisions

- **Review is a page, not a wider dock.** A decision deserves the window, and the dock cut its own
  labels with five tabs. What the dock had - deciding beside the map - is kept by the preview on the
  page and a link that focuses the item's entities on the Graph page.
- **Trust is a ring, not a fill or an opacity.** Fill already carries type. A faded node reads as
  irrelevant, not as unverified. A ring adds a fact without hiding one - which is why the other
  marks give up their rings.
- **Colors come from a generated band keyed by name, not a hand-kept table of types.** Types are
  induced (P11): nobody knows the list in advance, and a table naming them would run out.
- **The layout is remembered in the browser, not the store.** It is a view preference, the same for
  nobody else, and the store holds knowledge.
- **The same English words everywhere.** The landing, the app and the viewer say Sync, Review,
  Spoke; the documents keep their own terms and the tooltips point to them.

## 12. Invariants

| | Invariant |
|---|---|
| **U1** | Every console write - each path the read tier refuses as a verdict or a write - reports its outcome where it was made: what was done, the server's reason for a refusal, or that nothing changed (P24). |
| **U2** | The map draws a node's trust from its effective tier, the value the inspector's baseline reads, never a claimed tier; the legend states what the ring reads, and a scope of one tier draws no rings and states the tier (P18, inspector.md D3, D4). |
| **U3** | A contested node is marked wherever it is drawn, at every zoom, label and focus state, and the status bar counts the contested nodes of the loaded scope whatever the legend hides (P6). |
| **U4** | On the Review page a decision control appears only with its item's belief diff and checks on screen, and decides that item alone; there is no batch decision. Every open proposal is listed; a generated candidate is left out only when its diff is computable and empty (P23, proposal-workflow.md Section 14.2). |
| **U5** | Dream drafts are listed apart from published proposals and marked as this node's drafts; only the person's act publishes or accepts one, and none is shown on a read tier (dreaming.md DR1). |
| **U6** | The viewer's search answers with the engine's ranking and mode label, over a path whose read-tier policy is `Read`: workspaces resolve over the reader's grants, a union merges per-workspace hits by score then id, and entity hits are canonical. It adds nothing to the MCP surface (P17, P19, P21, remote-viewer.md V3). |
| **U7** | The viewer, the settings page and the landing give every token they share one value, and every text token meets 4.5:1 on every surface token. |
| **U8** | A status pill is absent only for a known zero or a feature this node does not have set up; unavailable, refused and unknown are drawn as states (P5, P24). |

U7 is guarded by `the_surfaces_share_one_token_set` and `every_text_token_meets_4_5_on_every_surface`
in `crates/supragnosis-viz/tests/design_tokens.rs`, which read the three stylesheets' `:root` blocks.

U1 is guarded by `every_console_write_reports_its_outcome`, a source tripwire in
`crates/supragnosis-viz/tests/http.rs` beside the escaping and D1 guards. It reads the write paths from
the read tier's own policy table, so a write added later is covered without anyone extending a list,
and it fails when one of them appears in the viewer's source other than as an argument of
`consoleWrite`, which reads the status and shows a notice on every branch.

## 13. Ordering

0. **The fixes that needed no document** [built]: the console's writes report (U1), type colors keyed
   by name, faint text at 4.5:1 on the surfaces then in use, Esc closes the inspector, the b key is
   shown, dock tabs fit their names, the shell's splash points at Settings, Settings > About names
   the source, and the landing's phone heading keeps its space.
1. **Tokens and foundations** [built]: the shared vocabulary and its test (U7), the six sizes, focus
   rings, buttons for every control, tab roles, reduced motion everywhere, and view toggles that
   persist.
2. **The map**: trust rings and their legend (U2), the other marks off rings, the contested badge and
   pill (U3), neutral edges, regions for the active node with their new names, label avoidance,
   remembered layout on the local console, and the three widths.
3. **Places and the Review page**: the Types and Peers rows moved first, then the tabs folded; the
   Review page (U4), with dream drafts when dreaming.md Section 12 step 5 lands (U5); the inspector
   as a sheet; the status bar's pills (U8) and the tray's Review item.
4. **Search** (U6): the read path, its policy and the server-side forwarding, then the bar.
5. **The landing's picture of the viewer**, retaken once the map has changed.

Each step is judged at the shell's default size and at 1440, 760 and 390 pixels wide, on the busiest
hub, an isolated node, a contested node (a seeded store, as inspector.md Section 8 notes), a blocked
proposal and the read tier.

## 14. What this revises

- **settings-page.md Section 5**: the tray menu gains "Review (N)..." while something waits on this
  Mac's daemon. The status line is unchanged, and on a remote profile the tray is as remote-viewer.md
  Section 6 leaves it.
- **inspector.md**: the panel moves from the bottom to a sheet on the right, and the scope line's
  words change. Its rules and D1-D7 do not.
- **The viewer's dock**: Proposals and Review leave for the Review page; the Types tab's rows join the
  filters dock and the Peers tab's join Settings > Sync before those tabs go. The Log stays.
- **remote-viewer.md Section 3.5**: the local `/api/surface` also names the node's host id, which the
  remembered layout is keyed by (Section 5) and which a host is compared with before it is called
  "this node" (Section 9). The read tier's answer is unchanged.

## 15. Closure map

| Demand | Where it is answered |
|---|---|
| P2 - a claim says where it came from | Sections 5, 7 (who said it on every evidence row), 9 (hosts named by id) |
| P5 - absence is not negation | Section 6 (rows moved before tabs go); Section 8 (empty search); U8 |
| P6 - a contradiction is surfaced | Section 5; U3 |
| P17 - the read tier reads what it was granted | Sections 5 (no remembered layout there), 8; U6 |
| P18 - trust is the receiver's evaluation | Section 5; U2 |
| P19 - a deterministic core, the edge labelled | Section 8; U6 |
| P21 - the MCP surface stays narrow | Section 2; U6 |
| P22 - curation where the reader already is | Section 7 (Health folded, the review count where the person looks) |
| P23 - the gate decides, one verdict at a time | Section 7; U4 |
| P24 - fail loudly | Sections 6, 9; U1, U8 |
| inspector.md D3, D4 - silence declared, display reads the effective tier | Section 5; U2 |
| inspector.md D5 - the viewer never re-resolves identity | Section 8; U6 |
| remote-viewer.md Section 6 - the read tier offers nothing it refuses | Sections 6, 7 |
| dreaming.md Section 8 - drafts wait apart, decided by the person | Section 7; U5 |

## 16. What the review changed

The draft was read once against the principles, the sibling documents and the code by a reviewer who
had not written it. Thirteen findings changed it:

- **The search path would have leaked.** It was given `Served`, the policy of the page and its
  assets, which on the read tier passes a request through with no grant check. It is `Read` now, a union merges per workspace as MCP's remote search does, and hits are canonical.
- **Two tabs were not duplicates.** Types holds contested type definitions with their confirm act, axis
  collisions and types with no node; Peers is the only list of peers with their admission state. Each
  tab's rows now move before the tab goes.
- **The ring said less than it seemed to.** `trust_tier` is the best support's tier, not the winning
  value's; six marks were rings; and a fixed "Agent has no ring" would have ringed every node on an
  unverified hub. The ring now follows the inspector's silence rule, the legend says what it reads,
  and the other marks change shape.
- **"From this Mac" was wrong on any synced scope**, since every unconfigured node is `localhost`.
- **U4 collided with D1 and with I4**: the inspector's mediation act stays, and an open proposal whose
  diff cannot be computed stays listed, since nothing else closes it.
- **Drafts, un-merge and reject were described by the wrong rules**: drafts expire, their acts are
  Publish, Accept and Discard, an un-merge suppresses the pair, and a reject is provisional.
- **The remembered layout would have mixed servers** under the app's single origin and kept a
  principal's positions past a revoked grant. It is the local console's alone.
- **U1 named a hand-kept list**, U7 missed the settings page's `--panel-3`, and a pill's absence could
  mean four things. U1 now reads the policy table, `--faint` moves to `#7f8898`, and U8 is new.

## 17. What building it changed

- **Step 1: the floor widened the right dock.** At 11px its five tabs with their counts measure 300
  pixels against a 274-pixel row, so the dock went from 300 to 336 pixels wide and its tabs stopped
  clipping. Step 3 takes four of the five tabs away, and the width is revisited there.
- **Step 1: the base rule for inputs and buttons moved to zero specificity.** Chips, legend entries
  and proposal and log rows became buttons, and the old base rule outranked nothing but element
  selectors, so it would have drawn its own box and hover color over each of them. At zero
  specificity every component class overrides it whole.
