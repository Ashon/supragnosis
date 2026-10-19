# Design system - one look, one set of parts, and a map that shows who said it (Principles 5, 6, 18, 23, 24)

> The viewer, the desktop shell's own pages and the landing are one product drawn three ways. This
> document fixes what they share - the tokens, the type, the parts - and the places where a rendering
> choice carries a principle stake: how the map draws trust and contradiction, where a decision is
> made, what the viewer's search answers with, what an absent indicator means, how a write reports
> back, and how one area is told from another. Companion to [inspector.md](inspector.md), whose rules
> for the detail panel stand unchanged.
>
> Status: **built - Section 14 steps 0-11**: the fixes that needed no document, the tokens and the
> foundations, the map, the places, the Review page, search, the landing's picture, the three
> surfaces drawn with surfaces instead of lines (Section 4), the landing that shows how knowledge
> moves (Section 11, U10), one voice in type (Section 4, U11), and the principles page (Section 11a,
> U12-U14). Dream drafts join the Review page with dreaming.md Section 12 step 5 (U5). The second
> pass over every screen (Section 1, U15-U20) has its foundation (step 12), the Graph page (step 14)
> and the viewer's and the engine's part of step 13 built; **the app's and the site's parts of step
> 13, and steps 15 and 16, are designed, not built**.

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

Six defects needed no design to fix and shipped first (Section 14 step 0): the console's writes
reported nothing, type colors followed the view's order, faint text measured 3.3:1, Esc did not close
the inspector, the dock's tab names were cut, and the shell's splash pointed at tray items that had
moved. What remains carries principle stakes, which is why it is written down before it is built.

**Measured again after step 6**, on the landing's seeded store at 1600x900: the parts were right and
the drawing was heavy.

- **Lines everywhere.** viewer.css drew a visible line in 53 declarations (a border whose value is
  not none, zero or transparent): every dock, chip, legend entry, row, pill and camera button and the
  minimap had a border of its own, inside a bordered dock. The settings page drew 12 and the landing
  19.
- **Gold everywhere.** Six section headings, the minimap's frame, every hover and the pills' outlines
  were the gold of the human-confirmed ring, and a gold glow lit the map's top, so the one gold that
  says something about knowledge did not stand out. The headings were 11px uppercase mono with wide
  tracking.
- **The map surrounded.** Six objects sat around and over it: the header, two floating docks, the
  status bar, four stacked camera buttons and the minimap. At 1440 pixels the docks left the map 866.
- **Region names shouted.** A focused hub's region names were drawn at up to 30px, the active node's
  in bold, and ran over one another and over the nodes they named.

**Measured again for the landing**, on 2026-10-18 after step 8, at 1440 and 390 pixels wide:

- **The model told four times and shown once.** The agent-first triptych, the three guarantees, the
  pipeline with its two terminals and the eight capability cards each explain observe, the log, the
  gate and trust in prose: about 1,900 words of the page's main text, code excluded, in nine numbered
  sections, eleven screens tall at 1440. The
  one picture of the product is the viewer's screenshot in the fourth section; the hero's
  constellation is decoration.
- **A 76-word lede** under the heading, about four times the subhead of the developer-tool pages it
  was compared with, and two buttons that both leave the page, with the install command below them.
- **Hidden until scrolled.** With script running, every section starts invisible and fades in when
  an observer sees it, and the hero rises in on load, so a reader who follows an anchor, or a capture
  taken as the page loads, meets blank space until the fade has run.
- **Three tokens used and never declared.** The landing reads `--panel-3` and `--gold-glass`, and the
  Settings page `--select`, from no `:root`, so the landing's GitHub and secondary buttons lose their
  hover background, its version badge its ground, and the Settings page's selected section its
  surface. U7's
  test compares the tokens a stylesheet declares and never saw the ones it only reads.
- **Sixteen font sizes and three web fonts**, the body set in a serif that neither the viewer nor the
  app uses.

**Measured again for type**, on 2026-10-18 after step 9, on the author's Mac:

- **The tools load no font.** The viewer's and the Settings page's policies are `default-src 'none'`
  with no `font-src`, so the "IBM Plex Mono" they name is drawn as SF Mono wherever it is not
  installed, which is almost everywhere; only the landing, which loads it from Google Fonts, shows it.
  This section said the tools set identifiers in IBM Plex Mono.
- **Each system draws a different product.** The reading face is the system's: SF Pro with Apple SD
  Gothic Neo's Hangul on a Mac, Segoe UI with Malgun Gothic on Windows, whatever a Linux distribution
  ships. The read tier's viewer opens in any of them, and in each the Hangul and the Latin of one line
  come from two designs.
- **The map draws names in the identifier face.** Node and region labels were set in the mono stack;
  it holds no Hangul, so a Korean name fell back to the system's face beside Latin names in mono.
- **Off the scale.** The Settings page still drew 11.5, 12.5 and 15.5px, and the viewer set 600 in
  25 places.

**Measured again across every screen**, on 2026-10-19 after step 11: the release build on a made-up
store, at 1440, 900 and 390 pixels wide, each screen's accessibility tree and tab order recorded, and
read by four reviewers against WCAG 2.2, Apple's Human Interface Guidelines and Nielsen's heuristics.
They found 53 problems, and these are the ones that ask for a rule:

- **Search was confident and wrong.** With billing selected, "checkout", a word billing does not
  hold, answered "12 hits". The semantic surface ranks every observation by nearness, and the list did
  not say which hits contain the words and which are only near them. The first row was chosen before
  anyone moved, so ArrowDown and Enter opened the second, and the field kept the query.
- **The first view was empty and did not say why.** The node default workspace is empty by design,
  and the viewer opened on it. Its chip was not among the chips, while the Log beside it counted two
  entries.
- **System words were still the first line.** The Log read "accepted claim_promotion: 9f778c20", its
  fourteen rows each said "8:54 AM", and none said who. Tier ids such as `agent_extracted` were
  printed where `TIER_NAME` already had a name for them. The status bar ran "ws (all) / nodes 20 /
  edges 14 / Component 3, Concept 4 ..." for nearly 1000 pixels, over the legend.
- **Type colors were not told apart, and one read as a signal.** Concept, Database and Metric drew
  nearly one green. Service sat beside the contested coral, and Decision measured 2.87:1 on the
  ground. The relation legend showed colors the map draws only on the active node's edges.
- **One idea had four names, and a switch moved the reader.** "hulls", "all regions", "Boundaries"
  and the status bar's "hull" named two displays. An option that was off looked disabled. With
  follow on, a write elsewhere changed the workspace and closed the inspector.
- **Settling a contested value leaned one way.** "keep" was gold and filled and "use this" grey.
  What either did was said in a tooltip, nothing undid it, and nothing said why the shown value was
  shown.
- **The way to Review was a count that vanished.** "2 to review" already held "1 contested", which
  was shown beside it as a third item. With nothing waiting the pill went, and the way to the page
  went with it.
- **One page had four names.** The first-run page was "welcome" in its crumb, "Set up supragnosis"
  in its title, "Finish setup..." in the tray and "Run setup again" in About. It mixed switches with
  unlabelled checkboxes. Three of its sentences promised what the code does not do.
- **The Settings page said one thing three times.** Version drift drew three Restart buttons, every
  section carried a green dot, the server list's radios could not be pressed, and sync peers were
  listed twice.
- **Phones and keyboards.** At 390 pixels the status bar pushed the gear, the only way to Settings,
  off the screen. The landing's install command was cut at `ashon/tap/`, and the principles ledger's
  columns were 8.5 pixels wide. The Review page's tabs did not move with the arrow keys, A and R
  decided from anywhere on the page, toggles did not expose their state, and focus left the Add server
  dialog.

Seventeen of the 53 were defects that ask no design question; Section 14 step 13 fixes them as they
are. The rest change what the sections below say.

## 2. What this is NOT

**It is not a change to what the server decides.** No fold, check, tier or verdict changes. One read
path is added (Section 8), and nothing the server answers today is removed or narrowed.

**It is not a new MCP surface.** The viewer is a separate human channel (P21). No tool, prompt or
resource changes; the viewer's search calls the function `search_knowledge` calls, over its own path.

**It does not reopen the inspector's rules.** The silence rule, the scope line and D1-D7 stand,
including the contested block's inline actions (D1). This document moves the panel, renames its
lines in plain words and adds marks to the map, and every mark it adds reads the same effective tier
the inspector's baseline reads (inspector.md D4).

**It does not put a floor under the semantic surface.** The viewer groups what search answers
(Section 8) and hides none of it. A similarity floor would change what agents get from
`search_knowledge` too, so it belongs to semantic-retrieval.md, measured on real stores before it is
set.

**It adds no new write path.** Undoing a confirmation is a demotion, the gated act resolution.md
Section 5 already defines, and `/api/resolve` gains a `kind` that casts it, on a path the read tier
already refuses (U1). "Neither is right" asks an agent to record the right value. The console still
observes nothing.

## 3. Tokens

One vocabulary, the one the viewer and the settings page already share, with one value each:

| Token | Value | For |
|---|---|---|
| `--surface` | `#08090d` | the canvas and the page ground |
| `--panel` | `#10131b` | the frame: the header, the panels, the sheets and the status bar |
| `--panel-2` | `#161a23` | controls, chips and rows on a panel |
| `--panel-3` | `#1d222d` | hover, and wells on a control |
| `--select` | `#2a2826` | a selected item: the accent at 10% over `--panel-2`, opaque so its text is measured |
| `--line` / `--line-soft` | `#222836` / `#1a1f2b` | the few lines Section 4 keeps; never text, never an area's edge |
| `--ink` | `#e9e4d6` | primary text |
| `--ink2` | `#aab1bd` | body text |
| `--muted` | `#8e96a5` | secondary text |
| `--faint` | `#868f9f` | meta text, placeholders |
| `--accent` / `--gold-bright` | `#d9a544` / `#f0c469` | the accent, and human-confirmed |
| `--ok` | `#56b3a2` | success, connected, healthy |
| `--warn` | `#e0a24a` | needs attention |
| `--bad` | `#e07a5f` | error, blocked, contested |

The landing renames to match (`--ink-0` to `--surface`, `--text` to `--ink`, `--gold` to `--accent`),
the viewer's `--teal` becomes `--ok`, and its literal `#d96a5f` becomes `--bad`. A surface declares
the tokens it uses; the core - `--surface`, `--panel`, `--line`, `--ink`, `--muted`, `--faint`,
`--accent` - is declared by all three. Every text token measures at least 4.5:1 on every surface
token. `--faint` moves once more for that, from step 0's `#7b8494` (4.46:1 on `--panel-3`) to
`#7f8898`, whose lowest pair is 4.71:1. Step 7 raises `--panel-2` and `--panel-3` so that a step of
tone can do what a border did (Section 4), adds `--select`, and moves `--faint` to `#868f9f`, whose
lowest pair, on `--select`, is 4.51:1. `--panel` stays: darker, its edge against the map would
vanish. Because U7's test holds the shared values equal, step 7 changes them in all three
stylesheets at once, and step 8 changes how the other two draw.

Space runs on a 4px step (4, 8, 12, 16, 24, 32, 48). Corners are 8px for chips, controls and rows,
12 for a surface that floats, 16 for the map's window (Section 6) and fully round for pills. Layers
are named, lowest first: canvas, HUD, dock, sheet, popover, dialog, notice. Motion uses three durations (120, 200 and 320ms) and one ease-out curve.

**Each surface keeps its own copy, and a test holds them together** (U7). The viewer's three files
are compiled into the binary and served under a policy that names no other style source, the
settings page carries its own policy, and the landing is a static site. A generated shared file would
add a build step to all three for what one test gives: it reads the three `:root` blocks, fails when
a token declared in more than one differs, and computes the contrast of every text token on every
surface token, `--select` among them.

## 4. Type, focus and the selected state

- **Three faces, two of them in the tools.** Pretendard for what a person reads and the names of
  things: running text, controls, the tools' headings, and the names the map draws. IBM Plex Mono for
  identifiers: relation kinds, ids, hashes, paths and commands. Fraunces for the site's headings
  and its social image, and nowhere in the tools. Pretendard draws Hangul and Latin from one design,
  so a Korean name beside an English one keeps its height and weight, which a system stack that takes
  its Latin from one face and its Hangul from another does not; its Latin is Inter's, open and tall
  enough to read at 11px, with tabular figures. IBM Plex Mono's x-height, 0.516 of the em, is within
  3% of Pretendard's 0.530, so an identifier set among names keeps the line's height.
- **The faces ship with the product** (U11). The daemon serves Pretendard, as its author's dynamic
  subset - 92 files, each with a unicode-range, so a page fetches only the files its text needs - and
  IBM Plex Mono's Latin files at 400, 500 and 600, under `/fonts/`: the same bytes on the local
  console and on the read tier, under a versioned path cached for a year, and the viewer's policy
  gains `font-src 'self'`. The Settings page and the landing are English pages and carry only the
  nine Pretendard files that hold Latin and its punctuation, with the same IBM Plex Mono; a Hangul
  name there falls back to the system's Korean face. Nothing is fetched from another host, and a face
  that does not load falls back to the system stack the token names after it. The files are the ones
  their authors publish, unmodified, with their licenses beside them.
- **Six sizes** in the viewer and on the Settings page: 24 (page title), 18 (sheet title), 15
  (section), 13 (body), 12 (secondary) and 11 (labels). Nothing is drawn below 11px, and nothing
  between the steps. The landing, a page read at length rather than a tool, has six sizes of its own
  (Section 11). A heading on a panel is 12px sans, semibold, in `--muted` and in sentence case, and
  nothing in the viewer or on the Settings page is set in uppercase. The landing keeps its uppercase
  eyebrows in IBM Plex Mono: a page about the product, set in the product's type, not a tool.
- **Three weights.** 400 for reading, 500 for small labels and emphasis, 600 for headings. Nothing in
  the tools is set at 700 or heavier, or lighter than 400: light text on a dark ground spreads and
  reads heavier than it is, and a hairline at 11px breaks up.
- **Lines and spacing.** A line of English in the tools runs at 1.4 to 1.5; where Korean runs - an
  observation's text, the Log - at 1.55 or more, since its syllable blocks stand taller than a
  lowercase line. Tracking is the face's own in the tools - Pretendard's Latin is already spaced for
  small sizes - with -0.02em on the landing's large Fraunces headings and +0.1em on uppercase
  eyebrows, and Korean is never tracked tighter. Numbers that count or line up take tabular figures. Korean
  labels and headings keep their words whole (`word-break: keep-all`); ids and addresses break
  anywhere.
- **On the canvas**, the names of nodes and regions are Pretendard at 500, and relation kinds and the
  contested badge's mark are IBM Plex Mono. The map draws again when a face finishes loading, so no
  label is placed by the fallback's measure and drawn in the face.
- **Focus is visible on every control**: a 2px `--gold-bright` outline, offset 2px - an outline on
  `:focus-visible`, not a border.
- **A selected item sits on `--select`**, its text `--gold-bright` when it is a choice (a chip, a
  segment). It takes no border, and a rounded box never takes a rail on one side: the rail bends
  around the corners and reads as a bracket. Hover lightens a control to `--panel-3` and its text to
  `--ink`; hover is not gold.
- **Surfaces, not lines** (U9). One area is told from another by its surface: the map on
  `--surface`, the frame on `--panel`, a control, chip or row on `--panel-2`, hover on `--panel-3`, a
  selection on `--select`. No border, outline or ring-shaped shadow draws the edge of an area, a
  panel, a control, a chip or a row. A text field is a filled well named by its label or placeholder;
  its old edge measured 1.2:1, short of the 3:1 a boundary needs, so the line was not doing that work
  either. A key cap is filled too. A line is drawn only where it carries a fact:
  - keyboard focus, the outline above;
  - a legend swatch of a line the map draws: the Trust legend's rings and the boundaries' dashes;
  - the dashed chip of a superseded relation, in `--muted`;
  - the dashed box around a contested value, in `--bad`, the contested color;
  - on the principles page's ledger, an owed clause's `--muted` outline and the chosen column's
    `--gold-bright` one (Section 11a);
  - a control's own glyph: a radio's circle, a switch's track, the loader's spinner.

  On the canvas the rings, edges and boundaries are the data and stay.
- **Each control is the kind its act is** (U19). A switch applies at once and says on or off, as
  `role="switch"` with `aria-checked`: Start at Login, View's labels and edges. A checkbox is a choice
  that a button confirms: the Setup page's plan, with the whole row as its label. One choice among a
  few is a radio group or a segmented control (`role="radiogroup"`): View's Regions and Boundaries,
  and the workspace chips, which choose one scope. A chip that filters is a button with
  `aria-pressed`: types and relations. A row of segments that changes what a page shows is a tab list: the Review
  page's. An option that is off is drawn at full strength and says it is off. Dimming means a control
  cannot be used now, and such a control says why.
- **Gold says one of four things** in the viewer and on the Settings page: selected, human confirmed,
  waiting for a person (Review in the status bar while something waits, and the one action a
  decision asks for), and keyboard focus.
  The mark keeps its gold. Headings, frames, hover, counts and the map's ground are not gold. The
  landing presents the product rather than operating it, and also keeps gold for its links and its
  one call to action.

## 5. The map

One channel, one meaning:

| Channel | Encodes | Notes |
|---|---|---|
| fill | type | keyed by name (step 0) |
| size | degree | as today |
| ring | trust of the best support | Unverified: dashed, `--muted`. Agent: none. Host signed: thin, `--ink`. Human confirmed: solid, `--gold-bright` |
| badge | contested | a `--bad` disc with "!" at the node's upper right |
| halo | selected | a soft `--ink` disc behind the node, replacing the selection ring |
| glow | this session's footprint | a soft `--ok` light centred on the node and fading out, drawn beneath it, replacing the teal ring |
| pulse | activity | one expanding ring, none under reduced motion |
| edge | relation | neutral; on the active node's edges, brighter and labelled with the kind |
| region | context (hyperedge) | every region faint at rest, the active node's at full strength (Regions: All, the default) |

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
contested count counts the contested nodes of the loaded scope whether the legend shows them or not
(P6).

**Colors are keyed by name, from a band that cannot be read as a signal** (U16). The band is sixteen
colors: eight hues in two tones.

- **No hue near a signal.** Every hue sits at least 25 degrees of OKLCH hue from `--accent`,
  `--gold-bright`, `--ok` and `--bad`, so no type reads as human confirmed, healthy or contested. That
  leaves the greens from 109 to 155 degrees, and the blues, violets, pinks and reds from 205 around to
  11.
- **Bright enough to see.** Every color has OKLCH lightness 0.62 or more and measures 5.5:1 or more
  on `--surface`, well past the 3:1 a mark needs (WCAG 1.4.11). Lightness alone does not promise the
  contrast - a violet at 0.62 measures 5.1:1 - so both are measured.
- **Measured apart.** In the first tone the eight hues stand at least 0.09 apart in OKLab, at least
  0.11 from every signal and at least 0.10 from every neutral the map draws. In the second tone every
  color stands at least 0.06 from every other color of the band and 0.08 from every signal.
- **How a type gets its color.** An FNV-1a hash of the name picks a preferred hue. In name order,
  each type of the view takes the first free hue from its preferred one, in the first tone, then in
  the second. Up to eight types on a screen are told apart by hue, and up to sixteen by hue and tone.
  Past sixteen a color is shared, and what tells two such types apart is pointing at or focusing a
  legend entry, which lights that type's nodes and dims the rest.

A type keeps its color in every view where no earlier name takes its hue. That is fewer views than
the 48 generated slots kept, and the legend is what names a color. The landing's stage and the
principles page copy the band with the viewer's functions (U10, U14).

**Relation kinds draw no color.** At rest a relation is the neutral line. On the active node's edges
it is drawn brighter, in `--ink2`, and labelled with its kind in the identifier face. The relation
legend draws each kind as that line. Thirty kinds in a band of sixteen colors would say nothing, and
a legend of colors the map draws only on a hover promised what the map does not show.

**Every region is drawn, and the active node's are named.** A region is a cloud in its context's
color: one ellipse laid along its members, densest over them and fading to nothing past the
outermost, with no edge. A region says what was said together, which is a looser fact than a
boundary, and an edge would read as a line no fact asks for (Section 4). At rest every region is
drawn, faint; while a node is active its own regions are drawn at full strength and the rest fade.
View's **Regions** says which are drawn: All, Selected (the active node's alone) or Off. Only the active node's regions are named, by
two of their members other than it, with the count: "proposal gate, claim_demotion defect (4)" -
every context of a hub was once named after the hub, so selecting one titled five regions alike.
Region names are placed before node labels, which avoid them, so names for every region at rest
would push the map's own names out. A region's name sits outside its upper edge, 13px at medium
weight whatever the region's size. The minimap draws the same clouds. Each region's hue comes from its
context's id, mapped onto the band's arcs so that no cloud is gold, coral or teal, and every region
shares one lightness and one chroma in OKLCH, so no region reads
brighter or heavier for its hue; where clouds overlap they are mixed in OKLCH too - lightness and
chroma kept, hues by their weighted circular mean - not by the canvas, whose compositing mixes in
sRGB and meets two hues in a grey or a brown neither of them is.

**Workspaces cohere while the boundaries draw them.** With the boundaries on "workspace" and more
than one workspace in scope, a node outside its workspace's natural radius - what its nodes would
fill at their usual spacing - is pulled toward the workspace's centre, two workspaces whose actual
spreads overlap are pushed apart as wholes, and the repulsion between nodes of different workspaces
is turned down to a quarter, since the workspaces already keep their distance. Inside the radius
nothing pulls, so a large workspace keeps the shape its edges give it.

**Links fit the density around them.** A link's rest length grows with the smaller degree of its two
ends - a leaf sits close to its hub, two hubs keep room for their leaves between them - and with how
crowded its ends are against the graph's average, so a crowded patch spreads and a sparse one
gathers. Its pull falls with that degree, as d3's links do, so a hub is not held by every link at
once, and the end with fewer links does more of the moving.

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
title bar holds the name, the search at its centre and the two panel toggles at its right, and no
navigation.

**The frame and the map's window.** The header, the two panels and the status bar are one surface,
`--panel`, and the map is a window cut into it with 16px corners: the panels sit beside the map, not
over it, and nothing divides them but the change of tone. The left panel is 260 pixels wide and the
right 300; the inspector's sheet, 360, takes the right panel's place. The canvas stays the size of
the window and the frame is drawn over it, so the pointer and camera code keep reading window
coordinates, and the camera's insets keep the content inside the window. With both panels open at
1440 pixels the window is 880 wide, 820 with the sheet: about what the floating docks left. What
changes is that nothing floats over it, and closing a panel gives its width back. The map's ground is
plain `--surface`: the gold glow that lit its top made the window's edge vanish where it was
brightest, about 1.00:1 against the frame.

On the Graph page:

- **The left panel is the filters**: workspaces, types, relations, the type definitions, and the
  Trust legend, which says what a ring reads (Section 5). The boundaries' legend is a view mode: it
  moves into View as **Boundaries** - Workspace, Origin or None - which b still cycles. The Types
  tab's rows move here before the tab goes: a contested type definition with its confirm act, an axis
  collision, and the types that are defined but have no node in the loaded graph. Pointing at or
  focusing a legend entry lights its nodes and dims the rest, and its tooltip shows only the winning
  definition; the full entry opens in place.
- **The right side shows one thing at a time**: the inspector as a sheet while a node is focused,
  otherwise the Log. The inspector moves there from the bottom, where it covered the part of the map
  being inspected. Its two regions, relations then evidence, stack in the sheet (inspector.md Section 6).
- **Either panel closes from its toggle**, or with [ and ], and the choice is remembered; the edge
  handles go. With both closed the map's window takes the frame. With the left panel closed and a
  scope of one tier, the status bar's scope line says the tier, as the Trust legend would (U2).
- **The strip is the Log folded.** While the right side is empty - no Log, no sheet - a one-line
  strip at the window's lower left shows the scope's newest observation: its tier dot, who (the
  on-behalf-of name, else the host by id, called "this node" only as Section 9 allows), when, and its
  first line. It reads `/api/observations` with `limit=1`, a `Read` path on the read tier, refreshed
  with the graph. With nothing observed it says "Nothing observed in this scope yet"; when the server
  does not answer it says so instead of keeping a stale line (P5, P24). Clicking it opens the Log. The
  activity feed's passing rows sit inside the window at its upper left.
- **The camera is one toolbar** at the window's lower right: view, zoom in, zoom out, fit. Under 760
  pixels it stays at the top right, out of the sheets' way (Section 18). The minimap is on, above the
  toolbar, until View turns it off; between 760 and 1200 pixels it gives way to a panel floating over
  the map's right side, and under 760 it is not drawn. View's stored choices forget the minimap's
  once, since any click stored every choice and most were never made, and View remembers it from then
  on.
- **The status bar is the window's state, said once.** On the left, the scope and its counts in
  words: "All workspaces - 20 entities, 14 relations, 10 regions". Counts per type are the legend
  entries' to show, not the bar's. On the right: the way to Review, the sync pill, the search mode,
  then the gear. A pill is tinted, not outlined. How fresh the map is goes unsaid while it is fresh;
  once a poll fails, the bar says "Not updated since 8:54 - the server is not answering", and the
  time does not tick at every poll.
- **Review is always in the status bar** on the local console (U20). It reads "Review" when nothing
  waits and "2 to review (1 contested)" when something does: one count, the items under Needs you,
  and in the parenthesis how many of them are contested beliefs. Needs you lists only beliefs that
  tie on trust (resolution.md Section 4.1), so those are the loaded scope's contested nodes, which U3
  asks the bar to count whatever the legend hides; before the curation read answers, the graph's
  count stands in. A conflict that trust settled is listed under Health, so a confirmation takes its
  item off the count. It is a button, the Graph page's way
  to Review wherever Cmd+2 does not reach, and it is not in the title bar, which holds no navigation.
  On the read tier, where nothing is decided, there is no Review entry, and the bar keeps the
  contested count alone as a pill ("1 contested").

**The first view is where the knowledge is.** The viewer opens on the scope the address names, or
else on the node default. When the default holds nothing while other workspaces hold knowledge, it
opens on All and says so beside the chips: "Showing all workspaces - default is empty". The scope in
use is always a selected chip, All among them, and it is called All, never "(all)" or "*". An empty
default means the wrong scope, not an absence of knowledge (P5).

**Follow moves the camera, not the reader** (U20). With follow on, activity in the scope on screen,
a write or an agent's read, frames the nodes it touched. A write in another workspace is a notice
that names the workspace, with Show; a read elsewhere says nothing. The scope, the selection and the
open sheet stay as the person left them: changing what a person is reading is the person's act
(WCAG 3.2.5). Only the knowledge itself can move the selection: when a merge folds the selected entity
into another, the selection follows it there, and when a sync or a merge leaves it out of the scope,
the sheet says so and closes.

The dock's Peers tab is the only place that lists known peers with "not admitted", "admitted, not
shared" and "surface unknown". Those rows move into Settings > Sync, and into the viewer's own
settings dialog outside the app, before the tab becomes the sync pill. On the read tier there is no
peers view and no sync pill (remote-viewer.md Section 6).

**A pill's absence means one thing** (U8): a known zero, or a feature this node does not have set
up. Unavailable, refused and not yet known are states the pill draws, never reasons to leave it out
(P5, P24). The review pill is absent on the read tier, where nothing can be decided, and the read
tier's banner already says why.

By width: from 1200 pixels the filters, the map's window and the right panel sit side by side; from
760 the frame keeps its header and status bar and one panel at a time lies over the map, a surface
with a shadow; below 760 the panel is a sheet from the bottom. Below 760 the status bar keeps the
scope's name, Review and the gear, and gives up the counts first, then the pills' words. The gear
never leaves the screen, and nothing scrolls sideways at 320 pixels (WCAG 1.4.10).

## 6a. The app's own pages

The Settings page (settings-page.md) and the first-run page (onboarding.md Section 6) are the app's.
Their rules are in those documents, and the second pass changes four things in them:

- **One name: Setup.** The first-run page is Setup everywhere: its crumb is `supragnosis / setup`, its
  title "Set up supragnosis", the tray's item "Finish Setup..." and About's act "Open Setup". "Welcome"
  fits only the first visit, and the page is reached three ways after it. A state has one word on
  both pages: an AI app that waits for the person's act "Needs you", on Setup as in Settings. A
  control that both pages offer, such as an AI app's row or the model's download, is built once in the
  app's shared script and drawn by both.
- **The plan is checkboxes, and Set up is the default.** Every item of Setup's plan is a checkbox
  whose label is its whole row, since the plan is a choice the Set up button confirms (Section 4).
  Set up is the default button, which Return presses, and the first control after the plan in tab
  order. Copy log and Copy diagnostics sit at the left, beside the privacy line. An item that replaces
  what the person set up - an app's "Switch to the bridge", the daemon's "Take over" - starts
  unchecked (onboarding.md O8). While the plan runs
  the foot says "Setting up... 2 of 3"; when it ends the heading says "Supragnosis is set up"; the
  page tells a step that is already running from one that failed.
- **Every sentence is one the code keeps.** The plan says that nothing changes until Set up, so every
  act in the plan waits for it: "Switch to the bridge" and "Take over" are items of the plan, not
  buttons that run at once. Once the plan has run, each card's own buttons act when pressed, and the
  page says so above the cards. The way back is named where it always is: "Open Setup" in Settings >
  About, while "Finish Setup..." stays in the tray only as onboarding.md Section 6 has it. The privacy
  line says what is true: "Supragnosis keeps your memory on this Mac. It is shared only if you add a
  remote server or set up sync."
- **The Settings page says each fact once.** Version drift is one card at the top of This Mac,
  "Update ready - restart to load 0.7.0", with the one Restart. A section's dot appears only when the
  section needs attention, with words a screen reader reads. The server list is a list: the active
  row says Active and every other row has Use, and the radios that could not be pressed go. A sync
  peer is one row. Claude Code's row in AI apps, while it is connected through the bridge, carries a
  switch, "Save the why behind each commit", which turns its capture hook on and off
  (client-connect.md Section 12): on runs `connect claude-code`, and off runs `connect claude-code
  --no-capture`, which then removes the hook and keeps the bridge. A row connected another way, or
  whose capture state the listing does not report, shows no switch, so the switch never connects or
  replaces anything.

## 7. The Review page

Everything that waits for a person's decision, in one place, split in three:

- **Needs you**: suspected secrets first, as an alert; then open proposals; then this node's dream
  drafts, listed apart from published proposals and marked as drafts (dreaming.md Section 8); then
  contested beliefs - those that tie on trust (resolution.md Section 4.1).
- **Suggestions**: merge suggestions, name variants and cross-workspace collisions. Each offers
  "Propose", which opens a proposal and decides nothing.
- **Health**: overloaded contexts, unlinked entities, merge cycles, type axis collisions and the
  conflicts trust settled, folded
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

**Keys act where the person is** (U19). A, R, J and K act only while focus is in the list or the
item; anywhere else they are letters (WCAG 2.1.4). Each control names its key in
`aria-keyshortcuts`, not in its name, so it is read as "Accept" and not "Accept A". Skip is a button
as well as J. The segments - Needs you, Suggestions, Health, Decided - are tabs that move with the
arrow keys. After a decision redraws the list, focus lands on the next item, or on the list when none
is left. While the page is open, the map and the panels behind it are inert.

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

**The inspector keeps its own act.** A contested belief's act in the inspector stays where it is,
under D1: it is the mediation act at the point of reading, and it is listed on the Review page too.
U4 governs the Review page's controls.

**Settling a contested value** (U18) is drawn the same in the inspector and on the Review page:

- **It says first why the shown value is shown**, in the policy's own terms (resolution.md Section
  2.1). For a tie: "Two sources disagree on Postgres's type. Both are Agent claims, so the newer one
  is shown until you decide." For a conflict that trust settled: "The Human confirmed value is
  shown."
- **Each value is a row that says its tier, who said it and when, with the same act at the same
  weight**: Confirm. The tier, the time and the asserting observation's first attestation come from
  `/api/explain`, which the inspector already reads. No value's button is filled or gold while the
  other's is not, and the shown value is marked as shown, not as preferred.
- **Confirm asks once, in place**: "Confirm Database? It becomes Human confirmed, and Service stays
  listed." The two buttons are Confirm and Cancel. The act is the promotion `/api/resolve` casts
  today.
- **When the shown value is already Human confirmed**, confirming the other would tie them again, so
  the other row's act is "Confirm instead", and its question says both halves: "Confirm Service
  instead? Database goes back to Agent, and Service becomes Human confirmed." It casts a demotion of
  the shown value to the tier it has without any person's verdict, which `/api/explain` answers for
  each candidate, and the promotion of the chosen one.
- **The notice that follows offers Undo**, and stays until it is dismissed, as an error does. Undo
  casts the reverse: a demotion of the observation just confirmed to the tier its row showed before,
  and after "Confirm instead" a confirmation of the value it demoted. The log keeps every event (P3).
  A demotion is a ceiling (resolution.md Section 5): until a person confirms that observation again,
  a later attestation that would raise its tier does not, and the notice says so beside Undo, in
  words: "Undo keeps Database at Agent until someone confirms it again".
- **Every act is filed in the workspace of the observation it changes**, whatever scope is on screen.
  From All, `/api/resolve` would otherwise file it in the node default, where the fold that reads the
  observation's own workspace never sees it.
- **"Neither is right"** opens a sentence to give an agent, with Copy: "Record the correct type of
  Postgres in supragnosis." A correction is an assertion the log must hold before anyone can confirm
  it (resolution.md Section 4.2), and the console observes nothing.

The way to these items is Review in the status bar (Section 6), whose count is the items under Needs
you. The tray menu's "Review (N)..."
item counts the proposals that wait on this Mac's daemon, across its workspaces, and is in the menu
only while some do; contested beliefs and suspected secrets need the curation read, which the tray
does not make every minute. On a remote profile the tray stays as remote-viewer.md Section 6 has it.

On the read tier the page lists and explains, shows no dream draft (dreaming.md Section 5.3 serves
none there) and offers no control the tier refuses, as remote-viewer.md Section 6 requires of the
whole page.

## 8. Search

Cmd+K, or "/", opens a search bar over the canvas. The query goes to a new read path,
`/api/search?q=...&workspace=...`, which calls the engine function behind `search_knowledge` and
answers what it answers: entities and observations in its deterministic order, with the mode label
saying whether the keyword or the hybrid surface answered (P19). Enter on a row focuses an entity or
opens an observation's card, and the canvas frames the result. While the bar is open, the map lights
the entities the search returned, not names that merely contain the typed text, so the list and the
map give one answer.

**The answer says how each hit was reached, and over what** (U15).

- **The field names its scope**: "Search billing", "Search all workspaces".
- **Two groups.** "Contains "checkout"" holds the hits the keyword surface matched. "Related" holds
  the rest, and each of its rows says how it was reached: "near in meaning" for a hit only the
  semantic surface found, "linked to a match" for a neighbor the graph's enrichment added. Each group
  keeps the engine's order. The path answers each hit with how it was reached, read from the lists the
  fusion and the enrichment consumed, through a viewer function beside `search_canonical`, so the
  ranking and the MCP tool's answer are unchanged (U6). In keyword mode "Related" holds the neighbors
  alone, and the mode label says that meaning was not searched.
- **An empty scope says so, and offers the wider one.** When no hit in the scope contains the words,
  the first group says "No item in billing contains "checkout"." Its first row is then "Search all
  workspaces (3 found)" when the wider scope holds some. A second request to the same path counts
  them, over the reader's grants on the read tier, and an answer that reached the path's limit says
  "20+", since the limit cuts the keyword list before it is counted.
- **Nothing is chosen until the person moves.** The field is a combobox with a list and no automatic
  selection (WAI-ARIA's combobox pattern): ArrowDown moves to the first row, and Enter acts only on
  the row the person moved to. Once a row is chosen, the field shows its name.

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

**A tier is named, never spelled** (U17). Every place that shows a tier uses the Trust legend's
names, which are Unverified, Agent, Host signed and Human confirmed. The identifiers stay in tooltips
and in cards' footers.

**The Log speaks in sentences** (U17). A proposal event says what happened to which proposal in
words - "Merge proposed: PostgreSQL into Postgres", "Claim confirmation accepted" - and a who-line
under each row says the tier, who and when: "Agent - laptop-3 - 8:55 AM". Who is named as the strip
names a host (Section 6). Every kind of proposal the engine opens and every event it can carry has
its sentence, and one the page cannot phrase reads "Something changed", with the stored text in its
card. An id that is a hash is left out of the line; one that is a name is said whole.
Rows from one minute show the time once, on the first. A row's mark is the ring the map draws for its
tier (Section 5): dashed `--muted` for Unverified, none for Agent, a thin `--ink` ring for Host signed,
`--gold-bright` for Human confirmed.

**One place, one name** (U17). The first-run page is **Setup** wherever it is reached (Section 6a), and
a state keeps one word on every page that shows it.

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
| accepted claim_promotion: 9f778c20 | Claim confirmation accepted |
| opened entity_merge: PostgreS -> Postgres | Merge proposed: PostgreSQL into Postgres |
| ws (all) / nodes 20 / edges 14 / Component 3, ... / hyperedges 10 (hull 4) | All workspaces - 20 entities, 14 relations, 10 regions |
| (all), * | All |
| hulls / all regions / hull | Regions |
| keep / use this | Confirm, for every value |
| agent_extracted, host_signed | Agent, Host signed |
| welcome / Finish setup... / Run setup again | Setup / Finish Setup... / Open Setup |
| Needs switching | Needs you |

## 10. Keyboard, motion and contrast

- Every control is a `button` or an `input`. Chips, legend entries, proposal rows and log rows stop
  being clickable spans. Tab lists carry their roles and move with the arrow keys.
- The map is reached through search, and from a focused node its relations are a Tab away in the
  sheet. Esc closes the topmost card, menu or sheet (step 0).
- Under reduced motion nothing moves: no pulse, no marching dashes, no sliding docks, no spinner, and
  the layout settles in one frame.
- Text drawn at full strength on a token surface meets 4.5:1 (U7). One thing is outside the pairs on
  purpose: labels the map dims to put the focus forward. Until step 7 the panels were translucent
  glass whose ground was whatever the canvas showed through; they are opaque `--panel` from step 7,
  so their text is inside the pairs.

The second pass holds every surface - the viewer, the Settings and Setup pages, the landing and the
principles page - to one floor (U19):

- **Every control says what it is and what state it is in.** It has a name, and its kind and state
  are those Section 4 gives it. A button that shows only an icon is named by its act and its object:
  "Remove team-hub", not an empty name. Each page declares its language, `<html lang="en">`, and the
  map's canvas has a label that says what it draws.
- **A dialog holds focus.** It has a name, it moves focus into itself when it opens, Tab stays inside
  it, the page behind it is inert, and closing it gives focus back to what opened it. A form that is
  refused marks the refused field `aria-invalid`, says why beside the field, and moves focus to the
  first such field (WCAG 3.3.1).
- **A press lands.** A control a pointer presses is at least 24 by 24 pixels with its spacing (WCAG
  2.5.8), and 28 below 760 pixels wide, where a finger presses it.
- **A single key can be turned off.** The view's single keys - /, b, [ and ] - act from anywhere
  outside a text field, and View has a switch that turns them off (WCAG 2.1.4). The
  keys that decide act only within their list or item (Section 7).
- **Progress is announced.** A step that runs shows its progress as `role="progressbar"` with its
  value, and a step's end, success or failure, is said in a polite live region (WCAG 4.1.3). A page
  that has finished its work says so in its heading, not only in a notice that leaves.
- **Redrawing keeps the person's place.** A list redrawn by a poll or a decision keeps focus on the
  same item, or on its successor when that item went.
- **Nothing scrolls sideways** at 320 pixels except a code block, which takes focus (`tabindex="0"`)
  so that a keyboard can scroll it.

## 11. The landing

The landing presents the product to someone who has not installed it, and it is held to two rules
the product keeps: what it says the product does, the binary it offers does today, and a mark it
draws means what the same mark means in the viewer. Everything else on it serves the question a
visitor brings - what happens to what my agents learn - and answers it by showing one fact travel
from a note to an answer instead of describing the model again.

**Seven sections**, in this order, in about half the words (Section 18):

| Section | What it does | What it replaces |
|---|---|---|
| Hero | Says what it is in the heading and a subhead of at most 25 words, and offers the install command as the one act | the 76-word lede, the constellation, two buttons and six badges |
| How it works | Follows one fact through five steps beside the stage | agent-first, the guarantees, the pipeline and its two terminals |
| The viewer | The screenshot, told in the marks the stage taught | the viewer section |
| Connect | The apps, the one command that connects them, the tools | the MCP surface |
| Under the hood | Six one-line properties | the eight capability cards |
| Status and questions | Shipped, next, who it is not for yet, four questions | the status ledger |
| Get started | Three steps, with the install tabs | get started |

The portability section goes with its three topology drawings, two of which drew meshes that have
not shipped; moving between machines is the stage's fifth step.

**The hero.** The heading stays: memory for AI agents that remembers who said it. The subhead names
the clients and the split of the work - agents write as they work, a person decides what becomes
canon. The act is the install command in two tabs, the macOS app (`brew install --cask
ashon/tap/supragnosis`, the one spelling the page uses) and the script, with a copy button; beside it
are the install script's source, GitHub and the docs. One line of facts follows: the version, Rust,
the licenses, the platforms the binaries are built for (macOS on Apple silicon, Linux with glibc 2.39
or later), and no account. The version is stamped on deploy by one substitution over exactly one
`<li id="version-badge">` holding plain text (pages.yml), so the page keeps exactly one such element,
with no other attribute. Nothing in the hero waits for a script or an animation to be seen. The
install script the hero links to says what its binary searches with: the release is built with the
local embedding model, and the script's keyword-only notice predates that.

**After the command, the next step.** Under the install command one line says what follows it: "Then
connect an agent: `supragnosis connect claude-code`, or Connect in the app. All steps ->", which
links to Get started, and the header gains Install beside its other links. Below 560 pixels the
command wraps and its Copy button sits on its own line; a code block that still scrolls takes focus.
The hero's picture of a question and an Accept chip is a picture: no pointer, no hover, nothing that
looks pressable. The header's links stand at least 24 pixels apart, a finger's width (Section 10).

**The stage** is one canvas beside the five steps. Each step is a sentence, a short paragraph and one
snippet of the product's own text, and the scene beside it shows the same thing happen to a made-up
project - a payments service, its webhooks and their idempotency, nothing from a real store:

1. **Write.** Three agents' entries fly into one log; the first shows what an entry keeps - the
   machine that wrote it, the person it acts for, its source and how sure it is. The log records the
   machine and the person, not which agent app wrote an entry, so the agents' names are drawn beside
   the log and never on it. It answers "who said this?".
2. **Structure.** Each entry rises into the entities it names and draws its relations, and what was
   said together gathers into a region.
3. **Govern.** A second type for one entity raises the contested badge, and it stays: nothing settles
   it in this story. Two names for one thing become a merge proposal, and an agent asks a person to
   confirm one of its claims (webhooks need an idempotency key). The person accepts both on the Review
   page; the two verdicts land in the log; the merge folds one name into the other; the confirmed
   entry's tick turns gold, as the viewer's Log draws a human-confirmed row, and the two entities it
   names take the gold ring, since a node's trust is its best support's tier. It answers "has anyone
   checked it?".
4. **Recall.** Another agent searches; what matches stays lit while the rest dims, as the viewer's
   search lights its answer, and a dashed line runs from the answer back to the entries that say it -
   the receipts. A verdict is not a receipt; the entry it confirmed is.
5. **Travel.** The log crosses a hub to a second machine, which checks the signatures (an entry is
   signed when it leaves the machine that wrote it) and replays the same graph: the same contested
   badge, the same merge, and the same gold rings, because a verdict is an entry and the tier it grants
   is read from the log (resolution.md Section 6). Only the workspaces the person allows travel. What
   cannot arrive as human confirmed is a bare claim: an entry that only says a person checked it counts
   for no more than its signature proves (resolution.md R4). The step says that in words; the scene
   has no such entry. It answers "does my other machine know?".

The words carry the whole meaning. The canvas is hidden from assistive technology and the page reads
complete without it; what is worth reading - an entry's provenance, a Review row, a search answer's
receipt, the two machines' tiers - is a snippet in the step, and the canvas draws only names and
short labels, at 11 to 12 pixels.

**The stage's marks are the viewer's** (U10). An entity is filled with its type's color, computed by
the viewer's own functions of the type's name, copied into the stage; the stage's type names are
chosen so their colors sit apart from each other and from gold, coral and teal. A node has a halo of
the ground. A gold ring is human confirmed, at the viewer's width and offset; nothing on the stage is
host signed or unverified, so it draws no other ring. The coral badge at the upper right is
contested. A relation is the viewer's neutral line at its alpha, the one color the stage copies from
the viewer's script rather than from a token. A merge waiting for a person is the viewer's proposal
preview: an arrow from the name folded away into the one kept, in `--accent`, and the relation that
would move with it dashed. Log ticks are colored as the viewer's Log colors a row: an agent's entry in
`--ink2`, a human-confirmed entry in `--gold-bright`, a proposal muted and a verdict in `--ink`. Teal
is an agent at work, as the viewer's footprint glow marks what an agent's session touched; the
person and their verdicts are ink. Every other color the stage draws is a landing token.

**Motion.** A scene plays when its step reaches the middle of the window, once, in at most five
seconds, and then rests: at rest nothing moves and no frame is drawn. Each scene ends within the five
seconds after which WCAG 2.2.2 asks for a pause, so the stage needs none, on the phone's frames as
beside the steps. The page's scroll is never captured or tied to the drawing. Under the stage a rail
of five buttons, each named "Step n of 5" with its scene, marks the current step with `aria-current`
and takes the focus ring every control takes; pressing one scrolls that step's words to the middle of
the window, so the scene beside them is always theirs, and a scene that is not the next one cuts to
its resting frame. Under reduced motion every scene is its resting frame, the page scrolls without
smoothing, and a change of the setting while the page is open is followed. A hidden tab stops the
stage. The stage keeps its own timing and easing, outside Section 3's three durations: they are for
the tools' controls, and the stage's motion is its content. The hero has no entrance, and no section
waits to be scrolled into view to be shown (Section 10).

**Drawing.** The canvas's backing store is its CSS size times the device pixel ratio, capped at 2:
measured from the device-pixel content box where the browser reports it, and redrawn when the ratio
changes, as when a window moves to another screen. Its box has a fixed aspect ratio, 16:10 beside the
steps and 4:5 on a phone, its width capped so the frame fits the window's height, so nothing shifts
when it draws. A glow is a sprite drawn once per color, not a blur on every frame. Beside the steps the
stage's box holds a still picture of the third scene's resting frame; it shows whenever the stage
cannot draw - no script, a script that did not load, or one that failed - and the steps read the same
(P24). Without script the phone's frames are not drawn at all, rather than left as empty boxes.

**By width.** From 900 pixels the steps and the stage sit side by side, and the stage stays in view
while the steps scroll past it. Below 900 each step carries its own 4:5 frame above its words, which
plays when the frame comes into view, and the stage's column is not drawn.

**Type and surface.** Headings in Fraunces, reading in Pretendard (step 10; the system sans until
then), commands, identifiers and labels in IBM Plex Mono: Newsreader goes. The landing's own six sizes, which Section 4's scale for the
tools does not bind, are three for headings (up to 72, 38 and 24 pixels, smaller on a phone), 18 for
reading, 15 for secondary text and 13 for labels; the subhead takes the third heading size. Sixteen
sizes become those six. The grain, the two glows, the constellation, the hero's entrance, the scroll
reveal, the numbered eyebrows and the cards' hover lift go; the unnumbered eyebrows - the name's
etymology and "How it works" - stay. Surfaces follow Section 4 (U9). The stylesheet declares
`--panel-3`, `--select`, `--ink2` and `--bad`, which it now reads, and drops `--gold-glass`, the gold
ground under the version badge that Section 4's gold does not cover; every token a stylesheet reads is
one it declares (U7).

**Checked on every change.** The tests that read `site/` run in the Rust workflow, which a change
under `site/` alone did not start; step 9 adds `site/**` to its paths, and the front-end lint covers
`site/*.js` as it covers the viewer's scripts.

**Trust is earned with what can be checked.** The landing has no adoption to show yet, so it shows
the status - what shipped, what is next and who it is not for yet - and answers four questions:
whether anything leaves the machine, whether it needs a key or a model, what happens if you stop
using it, and how it differs from a notes file. It names no other product: a claim about one would be
ours to keep true, and we could not.

## 11a. The principles page

The principles page presents [principles.md](principles.md) to someone deciding whether to trust the
product, and it is held to the rule those principles hold everything else to: it states what the
code states, and a demand the code does not yet meet is shown as owed, never as working. What sets it
apart from a list of promises is the coverage registry (principles.md Appendix B.1), which splits
each principle into clauses and names the tests, or the construction, that hold each one, or says it
is owed - not assured; the page shows that ledger as it is.

**One page**, `/principles/` (`site/principles/index.html`), with an anchor for every principle
(`#p1` to `#p24`), one for the tensions (`#tensions`) and one for the method (`#how-held`), so the
landing, the README and an issue can each point at one principle. The landing links it three times:
from its navigation, between How it works and Status; from four of the six properties under the hood,
each beside the principle it summarizes (a log that never forgets, P3; no model decides, P19; absence
is not falsehood, P5; private by default, P17), with a line after them that names the page; and from
the status section's note. The footer's Principles link moves from the document to the page.

**Six parts**, in this order:

| Part | What it does |
|---|---|
| Hero and ledger | The heading, a subhead saying that each principle is split into clauses and each clause names its test or says it is owed, and the ledger |
| Chapter navigation | The six chapters of principles.md, held at the top while the page scrolls |
| Six chapters | Each chapter's name, a one-line thesis and its part of the ledger, then its principles |
| Tensions | Appendix A's rulings, as a map and a list |
| How a principle is held | The four evidence states, a rule about change tested as change, and links to the registry and the policy cases |
| Closing | The normative text, what is owed and when (architecture.md Section 14), and the install |

**The ledger** draws a column for each principle and a block for each of its clauses, stacked from
the bottom: tested by name, holding by construction, owed with today's behavior pinned, and owed.
Under it is the count of each state. It shows no ratio and no score: the registry deliberately
asserts none (principles.md Appendix B.1), and a ratio on the page would be read instead of the clauses.
Pressing a column shows that principle's clauses, each with its state, beside the ledger. The
columns, the counts and every principle's clauses are written into the page from the registry (U12).
An owed block is drawn as an empty cell with a 1px `--muted` outline, and the chosen column with a
`--gold-bright` outline: on the ground, a filled owed block measured 1.17:1 and the chosen column
1.26:1, so 28 of the 108 clauses could hardly be seen and the ledger looked less owed than it is
(WCAG 1.4.11). Both outlines are lines that carry a fact (U9).

**Owed is defined where it is first read.** The hero's subhead says what owed means - a clause the
code does not yet guarantee - and a short glossary under the ledger's counts names each word the page
uses once: clause, tested, by construction, pinned, owed, tension. One concept keeps one name on the
whole page.

**On a phone.** Below 900 pixels the ledger is six rows, one per chapter, and pressing a principle
moves to its block (`#pN`) instead of changing a panel below the screen. Each scene has "Play scene",
and none plays only because it scrolled into view. A separator such as "/" is drawn by CSS with empty
alternative text, so a screen reader does not read it, and "Copied" is announced.

**A principle** is a block with its number, a short title, one line, a reason of a sentence or two,
its animation, its clauses - folded, in an element the browser's find still opens - and two links,
to its section of principles.md and to the registry. Its line says only what its held clauses say;
where it names an owed demand, as for P7, P9 and P13, it says what the system should do and carries the
owed tag - a judgment the review checks, not the build.
The words are the landing's: they name what a visitor recognizes, and the normative terms stay in the
clause lists, which are the registry's own lines.

**Every principle animates.** Five are demos with one thing the reader changes: the kinds of answer a
read gives (P5), a conflict trust cannot settle and the verdict that settles it (P6), one log
delivered in different orders, twice or across a cut link (P16), a claimed tier against the
receiver's evaluation (P18), and a proposal, a write while it waits, and its merge or rejection (P23).
The other nineteen are scenes of two to four beats, each beat with a caption, with a button for each
beat and Replay. Most show one claim on a made-up project, the stage's payments service and its
neighbours; P20 and P21 draw the product's own crates and tools. Each resting frame is that claim's conclusion. They draw as the stage draws (Section
11): a pure function of the scene and the time, the viewer's marks with the viewer's meanings, a
backing store at the device pixel ratio capped at 2, and only names and short labels on the canvas,
with the sentences in the caption beneath it. A scene plays once when it first comes into view and
then rests, drawing nothing, and a canvas far from the window gives up its pixels; while it plays, Replay reads Stop and stops it at its resting frame,
since a scene may run past the five seconds after which WCAG 2.2.2 asks for a way to stop. Under
reduced motion every beat and every demo state is its resting frame, a change of the setting while the
page is open is followed, and a hidden tab stops a scene. The canvas is hidden from assistive
technology; every beat's caption is text in the page, the current one marked, and it is announced only
after a reader presses a beat, Replay or a demo's control. Without a script, or with one that fails,
no empty frame is drawn: each beat list reads as text, as the landing's steps do (U14). Every anchor
clears the chapter navigation held at the top (WCAG 2.4.11).

**Owed is drawn as owed.** A beat or a branch that shows a clause the registry does not hold names that
clause in the registry's words, carries the owed tag, and is drawn in the page's owed style: a hatched
ground at lowered opacity, a mark no viewer mark uses, so it is never read as unverified or superseded.
Which demands are owed is read from the registry, not listed here; today they include the headline
demands of P9 and P13, P5's destroyed answer, P21's one tool per intent and P18's evaluated verdict
marker. A beat that presents an outcome as the product's behavior is drawn only when a case replays
it; an owed beat presents none, and names the clause it waits on (U13).

**What a demo shows is the code's answer.** Every value a demo or scene presents as the product's
behavior - a belief, a contested flag, an effective tier, an id, a graph's digest, a count, a refusal -
is a case in a JSON block in the page, and each case names held clauses. A case is replayed by a test
in the crate that owns the behavior, or names the registry clauses whose tests hold it; P20's arrows
are read from the crates' `[dependencies]`. A graph's digest is the first six hex digits of the BLAKE3
hash of the workspace's graph serialized to JSON, the form `cross_node_reprojection_converges`
compares; the replay compares whole serializations, and the six digits are only a label. The page's
script takes every such value from the cases and names no trust tier itself (U12).

**Tensions.** Appendix A's rows are drawn as a map in page-only marks - the principles as neutral dots
grouped on their chapters' grounds, each ruling a small diamond joined to the principles it weighs, and
a side that is not a principle (caution, expressiveness, a reviewer's attention) written as a label -
and listed beside it; choosing one in either place shows its ruling. Each entry names the registry
clauses its mechanism rests on, and the build derives its owed tag from their states, which tags more
rulings than Appendix A's own list does (the tbox_change gate, elicitation, the replicated verdict
marker). The list is the way in for a keyboard and a screen reader; the map is hidden from assistive
technology.

**Type, surface and files.** The page uses the landing's faces and tokens and adds its rules to the
landing's stylesheet, with no style element or attribute of its own; step 11 extends U9's and U11's
tests to the page, and U7, U9 and U11 name the site rather than the landing. A change that moves a
clause's state rewrites the ledger in the same change, so its merge also publishes the site. Its script is
`site/principles.js`, one file with no build step, which the front-end lint covers as it covers the
stage; a demo or scene is set up when it first comes near the window. The page shares the landing's
social image.

## 12. Decisions

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
- **Areas are told apart by surface, not by line.** A border around every part made each one an
  object to read before the map; a step of tone separates an area without asking to be read. Lines
  stay where they carry a fact (U9).
- **The map alone is the panels closed, not a mode.** Two toggles reach it and the strip is the Log
  folded, so nothing new is learned and nothing moves where it lives. A map-first layout with both
  panels closed by default was drawn and set aside: it hid the Log, the view of agents writing, until
  someone asked for it.
- **The Trust legend stays beside the filters.** Section 5 has the legend say what a ring reads, and
  the scope's one tier when it draws no ring; inside a menu that would be read by no one. With the
  left panel closed, the status bar says the one tier instead.
- **The landing's picture of how knowledge moves is drawn live, not filmed.** A video would hold
  the viewer's marks as they were on the day it was recorded, scale by resampling on a dense screen,
  and play whether or not anyone is reading. The stage draws with the viewer's color function and the
  landing's tokens, at the screen's own density, from a pure function of the scene and the time, so
  every load draws the same frames and a test can hold its marks to the viewer's.
- **Scenes play on arrival, not on scroll position.** A drawing tied to the scrollbar moves while the
  reader is still reading, and stops halfway when they stop. A scene that plays once and rests leaves
  the step's words still, and the rail lets a reader see any scene again.
- **The install command is the hero's act, not GitHub.** The page is for people who run tools from a
  terminal; the command, with its script one click away, is the shortest true path to trying it.
- **No comparison with other products.** A table of someone else's features is a claim we cannot keep
  true; the status and the questions are claims about this product, kept in this repository.
- **Pretendard, not the system's face.** The system's face costs nothing to ship and is excellent on
  a Mac, but it is a different face on every system the read tier opens in, and on none of them are a
  Korean name and an English one drawn from one design. Pretendard costs about 3MB in the daemon,
  fetched a few files at a time. IBM Plex Sans with its Korean family was set beside it and set aside:
  its Hangul is a second family, heavy to ship at every weight.
- **IBM Plex Mono kept, and now shipped.** JetBrains Mono reads best at 11px, with the tallest
  x-height, and stands out of a line of Pretendard for the same reason. Plex Mono is already the
  landing's face and its social image's, and its height matches.
- **Fraunces stays on the site only.** It is the product's voice where the product is presented, on
  the landing and the principles page; the tools are operated, and a display face there is one more
  family to read past.
- **The principles get a page, not a landing section.** Twenty-four principles with their clauses are
  a reference, read in parts and linked one at a time; the landing keeps its seven sections and points
  at the page from the four properties that summarize a principle.
- **The ledger shows clauses, not a ratio.** A score invites improving the score; the registry
  asserts none, and a column of owed blocks says more than a percentage would.
- **A demo's outcome is the code's, not the page's.** A script that re-implemented resolution would
  drift from the engine with nobody noticing, and the page would then say something false about the
  product; as cases a test replays, every outcome the page shows is one the build checks.
- **Every principle animates, the owed ones as owed.** Animating only what works would hide the debt
  the ledger shows; drawing an owed demand in its own style keeps the pictures and the ledger saying
  the same thing.
- **Search groups by how a hit was reached, not by kind.** Splitting entities from observations was
  the cheaper change, and each group would still have mixed hits that contain the words with hits
  that are only near them, which is the difference the person could not see.
- **No floor in the page.** Cutting meaning hits below a similarity in the viewer would answer the
  person differently from the agent beside them (Section 8). Whether the engine needs a floor is
  semantic-retrieval.md's question.
- **Sixteen designed colors, not 48 generated ones.** The 48 slots kept a type's color across more
  views, but any five types drew two of them close enough to read as one, and some drew a signal's
  color. The trade is fewer stable colors for colors that differ on the screen in front of the
  person; the legend names a color either way.
- **A relation kind has no color.** Its name, drawn on the active node's edges, says more than a hue
  from a band that types already fill.
- **The way to Review is the status bar, not the title bar.** The reviewers drew a Review button in
  the title bar. The title bar holds no navigation (Section 6), the review count already sat in the
  status bar, and an entry that stays when nothing waits keeps the page one press away.
- **Undo is a demotion, not a deletion.** The log is append-only (P3): undoing a confirmation appends
  the gated event that reverses it, which resolution.md Section 5 defines and `/api/resolve` gains a
  `kind` for. Its cost is the ceiling a demotion leaves, which the notice names. A recall would also
  remove the value from view, and that is a different act.
- **"Neither is right" asks an agent.** A field where a person types the right value would add an
  assertion to a console that reads and decides but has never asserted. An agent records the
  correction, and the person confirms it here (resolution.md Section 4.2).
- **Follow frames, it does not move the reader.** Switching the workspace to follow a write was what
  follow did, and it took the reader away from what they were reading. A notice with Show keeps that
  as one press away.
- **Setup, not Welcome.** "Welcome" fits the first visit, and the page is reached three times more
  after it, from the tray and from About. "Setup" says what the page is for on every visit.
- **Checkboxes in the plan, switches in Settings.** A switch applies at once, which is what Settings
  does. The plan is applied by its button, and a switch there promised an act that had not happened
  yet.

## 13. Invariants

| | Invariant |
|---|---|
| **U1** | Every console write - each path the read tier refuses as a verdict or a write - reports its outcome where it was made: what was done, the server's reason for a refusal, or that nothing changed (P24). |
| **U2** | The map draws a node's trust from its effective tier, the value the inspector's baseline reads, never a claimed tier; the legend states what the ring reads, and a scope of one tier draws no rings and states the tier (P18, inspector.md D3, D4). |
| **U3** | A contested node is marked wherever it is drawn, at every zoom, label and focus state, and the status bar counts the contested nodes of the loaded scope whatever the legend hides (P6). |
| **U4** | On the Review page a decision control appears only with its item's belief diff and checks on screen, and decides that item alone; there is no batch decision. Every open proposal is listed; a generated candidate is left out only when its diff is computable and empty (P23, proposal-workflow.md Section 14.2). |
| **U5** | Dream drafts are listed apart from published proposals and marked as this node's drafts; only the person's act publishes or accepts one, and none is shown on a read tier (dreaming.md DR1). |
| **U6** | The viewer's search answers with the engine's ranking, each group of Section 8 in that order, and its mode label, over a path whose read-tier policy is `Read`: workspaces resolve over the reader's grants, a union merges per-workspace hits by score then id, and entity hits are canonical. It adds nothing to the MCP surface (P17, P19, P21, remote-viewer.md V3). |
| **U7** | The viewer, the settings page and the landing give every token they share one value and each declares every custom property it reads, in any of its rules, and every text token meets 4.5:1 on every surface token. |
| **U8** | A status pill is absent only for a known zero or a feature this node does not have set up; unavailable, refused and unknown are drawn as states (P5, P24). |
| **U9** | Areas are told apart by surface: the viewer, the Settings page and the landing draw no border, outline or ring-shaped shadow except where the line carries a fact - keyboard focus, a legend swatch of a line the map draws, a superseded relation, a contested value, an owed clause or the chosen column on the principles ledger, or a control's own glyph. |
| **U10** | The landing's stage draws the viewer's marks with the viewer's meanings: an entity's fill is its type's color by the viewer's own functions, a ring is human confirmed at the viewer's width, the coral badge is contested, a relation is the viewer's neutral line, and every other color it draws is a landing token. The second machine draws every ring and badge the first draws, since it replays the same log. Each scene ends within five seconds and then draws nothing until it is asked again; under reduced motion each scene is its resting frame; the canvas is hidden from assistive technology and each step says its scene in its own words (P2, P6, P18). |
| **U11** | Every face a surface names first in its reading or identifier stack ships with that surface and is served from its own origin, and no surface fetches a font from another host. The viewer, the Settings page and the landing name the same stacks. |
| **U12** | The principles page states only what the code states: its ledger, counts and clause lists are written from the coverage registry and the build fails when they differ; every principle of principles.md has its block and every Appendix A row its entry; every case names held clauses and is replayed by a test of the code that owns it or held by those clauses' tests; and the page's script takes every value it draws as the product's behavior from the cases and names no trust tier itself (P1, P5, P16). |
| **U13** | The principles page never draws an owed clause as working: every owed beat or branch names, in the registry's words, a clause the registry does not hold and is drawn in the owed style, every tension's owed tag is derived from the clauses it names, and the build fails otherwise (P5). |
| **U14** | The principles page draws the viewer's marks with the viewer's meanings and every other mark as its own, every color a landing token; each demo and scene plays at most once on its own, can be stopped while it plays, and rests drawing nothing; under reduced motion each beat and demo state is its resting frame; every canvas is hidden from assistive technology, a caption is announced only after the reader acts, and without a script the page draws no empty frame (P2, P6, P18). |
| **U15** | The viewer's search says how each hit was reached and over what scope: hits that contain the query's words are grouped apart from hits only the semantic surface or the graph's neighbors reached, each related row saying which, each group in the engine's order; the field names the scope; no row is chosen before the person moves to one; and a scope where nothing contains the words says so and offers the wider scope with its count, or "20+" at the limit (P5, P19). |
| **U16** | A type's color is never a signal's: every color of the band stands at least 25 degrees of OKLCH hue from the accent, human-confirmed, ok and bad tokens and has OKLCH lightness 0.62 or more and 5.5:1 or more on `--surface`; the band's first tone stands 0.09 apart in OKLab, 0.11 from every signal and 0.10 from every neutral the map draws, and its second 0.06 from every color of the band and 0.08 from every signal; a view gives up to eight types eight hues; a relation kind draws no color; and every copy of the band is the viewer's (P6, P18). |
| **U17** | No line a person reads first spells a system identifier: a tier is shown by its legend name; every proposal kind the engine opens and every event it carries has a sentence in the Log, and a hash id is left out of it; each observation row has a who-line naming tier, who and when, and its mark is the map's ring for its tier; and a place keeps one name wherever it is reached (P2). |
| **U18** | Settling a contested value says why the shown value is shown, offers every value the same act at the same weight, says what the act does before it is cast - both halves when it demotes the shown value - and is undone by the reverse gated events, which the log keeps; every act is filed in the workspace of the observation it changes, and the console asserts no value of its own (P3, P6, P23). |
| **U19** | Every control on the viewer, the Settings and Setup pages, the landing and the principles page has a name, is the kind its act is and exposes its state; a dialog holds focus and makes the page behind it inert; a key that decides acts only while focus is in its list or item, and the view's single keys can be turned off; every target a pointer presses is at least 24 pixels with its spacing, 28 below 760 pixels wide; and nothing but a focusable code block scrolls sideways at 320 pixels. |
| **U20** | On the local console the status bar always holds the way to Review, its count the items under Needs you, which lists only beliefs that tie on trust, with the contested ones counted in it; and follow never changes the scope, the selection or the open sheet - it frames activity in the scope on screen and names a write elsewhere in a notice - while a selected entity a merge folds is followed to what it became, and one that leaves the scope closes its sheet with a line that says so (P22, P24). |

U2 is guarded by `the_map_rings_read_the_effective_tier` and U3 by
`a_contested_node_is_marked_and_counted_whatever_hides_it`, source tripwires in
`crates/supragnosis-viz/tests/http.rs`: the ring reads `trust_tier` and the scope's uniformity over
every loaded node, the legend says what a ring reads, the badge is drawn in its own full-strength pass
that skips only an undrawn type, and the count is taken over every loaded node.

U4 is guarded by `a_review_decision_takes_one_item_with_its_diff_on_screen`: the decision returns unless
the view it holds is the item's, refuses a merge a blocking check fails, asks a recall twice, and is
never called over a list of items; every open or blocked proposal is listed.

U8 is guarded by `a_pill_is_absent_only_for_a_known_zero_or_nothing_set_up`: the contested pill hides
only at zero, and the sync pill hides only off the local console or where sync is not configured, and
draws "sync unavailable" when the daemon does not answer.

U7 is guarded by `the_surfaces_share_one_token_set`, `every_text_token_meets_4_5_on_every_surface`
and `every_token_a_stylesheet_reads_is_declared` in `crates/supragnosis-viz/tests/design_tokens.rs`.
The first two read the three stylesheets' `:root` blocks; the third fails unless each `var(--name)`
in the three stylesheets names a custom property the same file declares, outside a list, empty today,
of properties a page's markup or script sets, each listed with its setter.

U9 is guarded by `no_border_draws_an_area` in the same file, which reads the three stylesheets' rules
and fails, outside keyboard focus and a short list of selectors each listed with the fact its line
carries, on a border that draws, a `border-color` that is not transparent, an outline, or a
ring-shaped `box-shadow`; it also fails on a border the viewer's script sets outside the boundaries'
legend. In the stylesheets steps 7 and 8 replaced it finds 85 such declarations in the viewer's and 26
in the landing's.

U10 is guarded by `the_stage_draws_the_viewers_marks`, a source tripwire in
`crates/supragnosis-viz/tests/landing.rs` that reads `site/stage.js` beside the viewer's script, the
landing's stylesheet and the page: the stage's color functions are the viewer's, line for line with
leading spaces dropped; each palette color is the value of the token named beside it, and no other
color is written; the edge line and its alpha, the human-confirmed ring and the contested badge are
the viewer's; every ring and badge is set in one function, which both machines' scenes call; no scene
lasts more than five seconds, a frame is requested only while a scene plays, and the reduced-motion
setting is read and its change followed; and every canvas on the page is hidden from assistive
technology while each of the five steps has a heading and a paragraph of its own.
`the_landing_has_one_version_badge` holds the page to the one element the deploy stamps.

U11 is guarded by `every_face_ships_with_its_surface` in `crates/supragnosis-viz/tests/design_tokens.rs`,
which reads the three stylesheets, their pages and the policies they are served under: the reading
and identifier stacks begin with Pretendard and IBM Plex Mono in all three (U7 holds them equal);
each surface declares IBM Plex Mono at three weights and links a Pretendard stylesheet, and every
file those name is one the surface ships - compiled into the viewer and served under `/fonts/`, or
beside the Settings page and the landing on disk; the viewer's and the Settings page's policies
allow `font-src 'self'`; no page or stylesheet names a font host; and the map's names are set in the
reading face and its relation kinds in the identifier face.

U12 and U13 are guarded in the engine's tests. `the_principles_page_reads_the_registry` writes the
page's ledger, each chapter's part of it and each principle's clauses from the registry, and fails when
the page differs; run with `SUPRAGNOSIS_WRITE_PAGE=1`, it rewrites those regions, so a change that
moves a clause carries the page with it. `the_principles_page_shows_held_clauses_as_held_and_owed_ones_as_owed`
reads every clause a figure names: a shown clause must hold, an owed one must not, each tension's owed
tag must follow the clauses it rests on, Appendix A's rows and the page's tensions must name the same
principles, and every figure must have its case. `every_principles_page_case_marked_replay_is_the_engines_answer`
replays each case marked replay - eleven of them - on the in-memory adapter with the project its
figure draws, and compares every value the script reads from it: the digest, the id, the belief during
and after a conflict, the tier a person's verdict gives, the log's counts around a proposal.

U14 is guarded by `the_principles_page_draws_the_viewers_marks` and
`the_principles_page_plays_once_can_stop_and_rests` in `crates/supragnosis-viz/tests/principles.rs`,
which read the page's script beside the viewer's and the landing's stylesheet: the color functions,
the palette, the edge line and the rings are the viewer's; no tier is named in the script; a frame is
requested only while a figure plays, reduced motion draws the resting frame, a hidden tab stops it,
a scene plays once and can be stopped, a caption is announced only after a press; every canvas is
hidden and every beat is text in the page; the script copies the page's words as nodes and writes no
markup; and the page carries no style of its own. U9 and U11 hold
it through `no_border_draws_an_area` and `every_face_ships_with_its_surface`, which reads the page's
links. `the_landing_links_the_principles_page` holds the landing's four links to it.

U16 is guarded by `the_type_band_is_never_a_signal` and `every_category_color_comes_from_the_band` in
`crates/supragnosis-viz/tests/type_band.rs`. The first reads the sixteen colors from the viewer's
script and the signal and neutral tokens from its stylesheet, and measures every rule Section 5
states: the hue gaps, the lightness, the contrast on `--surface`, and the distances within each tone
and from the signals and neutrals. The second holds the band as the only source of a category color -
types, boundaries and peers - with no generated color left, relation kinds drawn in the neutral line,
and a region's hue checked over all 512 slots against the signals. The landing's and the principles
page's copies are held to the viewer's by `the_stage_draws_the_viewers_marks` and
`the_principles_page_draws_the_viewers_marks`.

U17's first half is guarded by `a_tier_is_named_never_spelled` and `every_proposal_event_has_a_sentence`
in `crates/supragnosis-viz/tests/words.rs`: no interpolation in the viewer's script draws a tier but
through `tierName`, and the Log's two tables cover every kind in the engine's `PROPOSAL_KINDS`, every
`ProposalEventKind` and every decision `review_proposal` accepts. Its second half - the who-line and
one name for Setup - is owed until steps 14 and 15.

U19's first part is guarded by `the_viewers_controls_say_what_they_are` in
`crates/supragnosis-viz/tests/controls.rs` and `the_app_pages_dialogs_hold_focus` in the app's tests:
View's toggles, the legend's chips, the workspace chips, the page's language and the map's name, and
the dialog on the app's pages, and `view_holds_two_choices_with_one_name_each` holds View's two
choices and its switch for the single keys. The rest of U19 - every control on every surface, the
targets and the reflow - is owed until the app's and the site's steps are built and browser scenarios
press them at 1440 and 390 pixels.

U15 is guarded by `every_viewer_search_hit_says_how_it_was_reached` in `crates/supragnosis-viz/tests/http.rs`,
which searches a store in keyword mode and with an embedder and reads each hit's `reached` - the word
match, the meaning-only hit and the neighbor - and by `search_chooses_nothing_until_the_person_moves`
and `search_groups_hits_by_how_they_were_reached` in `crates/supragnosis-viz/tests/screens.rs`, which
read the bar: no row chosen on arrival, Enter only on the row moved to, the field naming its scope,
the two groups and the wider scope's count.

U18 is guarded by `settling_a_contested_value_says_why_asks_once_and_can_be_undone` in
`crates/supragnosis-viz/tests/settle.rs` - the heading, one Confirm for every value at one weight, the
question in place, Neither is right, Undo as the demotion and the notice that names its ceiling - and
by `viz_resolve_files_in_the_observations_workspace_and_can_be_undone` in `http.rs`, which confirms a
value from `workspace=*`, finds the act in the observation's workspace and none in the default, reads
the value's `base_tier` from the explain path, and undoes it into a tie again.

U20 is guarded by `review_is_always_in_the_status_bar_with_one_count`,
`follow_frames_what_is_in_scope_and_notes_the_rest` and `the_first_view_opens_where_the_knowledge_is`
in `crates/supragnosis-viz/tests/places.rs`: Review's entry hides only on the read tier, reads Review at
zero and counts the contested beliefs inside its one count; Needs you lists only ties; the event
handler changes no scope and notes a write elsewhere with Show; and an empty default with knowledge
elsewhere opens on All and says so.

U1 is guarded by `every_console_write_reports_its_outcome`, a source tripwire in
`crates/supragnosis-viz/tests/http.rs` beside the escaping and D1 guards. It reads the write paths from
the read tier's own policy table, so a write added later is covered without anyone extending a list,
and it fails when one of them appears in the viewer's source other than as an argument of
`consoleWrite`, which reads the status and shows a notice on every branch.

## 14. Ordering

0. **The fixes that needed no document** [built]: the console's writes report (U1), type colors keyed
   by name, faint text at 4.5:1 on the surfaces then in use, Esc closes the inspector, the b key is
   shown, dock tabs fit their names, the shell's splash points at Settings, Settings > About names
   the source, and the landing's phone heading keeps its space.
1. **Tokens and foundations** [built]: the shared vocabulary and its test (U7), the six sizes, focus
   rings, buttons for every control, tab roles, reduced motion everywhere, and view toggles that
   persist.
2. **The map** [built]: trust rings and their legend (U2), the other marks off rings, the contested
   badge and pill (U3), neutral edges, regions for the active node with their new names, label
   avoidance, remembered layout on the local console, and the three widths.
3. **Places** [built]: the Types tab's rows moved into the filters dock and the Peers tab's into
   Settings > Sync, then both tabs folded; the right dock back to its width; the sync pill beside the
   contested one (U8).
4. **The Review page** [built]: the page itself (U4); the inspector as a sheet; the review pill, View
   > Review (Cmd+2) and the tray's Review item. Dream drafts join it when dreaming.md Section 12 step 5
   lands (U5).
5. **Search** [built] (U6): the read path, its policy and the server-side forwarding, then the bar.
6. **The landing's picture of the viewer** [built], retaken once the map has changed.
7. **Surfaces, not lines: the viewer** [built] (U9): the token values in all three stylesheets (Section 3,
   held equal by U7), and the Settings page's title bar with the viewer's (settings-page.md Section
   3.0); every line the list does not name removed from the viewer; headings in sentence case, hover
   off gold, the contested box in `--bad` and the glow gone (Section 4); the frame and the map's
   window, the panel toggles, the strip, the camera toolbar, the minimap off, and the boundaries'
   control in View (Section 6); region names at 13px (Section 5). The Review page, the search list,
   the observation card, the activity feed and the settings dialog take the same surfaces.
8. **Surfaces, not lines: the Settings page and the landing** [built] (U9): the rest of the rule on both.
   On the Settings page, cards, rows and fields become surfaces, and the sidebar's 10.5px uppercase
   group labels become 12px sentence-case headings. On the landing, the code, terminal and picture
   frames become surfaces and the steps' gold rules go. Then the landing's picture is retaken.
9. **The landing tells how knowledge moves** [built] (Section 11, U10, and U7's declared tokens): the seven
   sections and their words, the stage with its five scenes, the phone's frames and the still picture
   for a stage that cannot draw, the tokens the stylesheets read declared (and `--gold-glass` dropped),
   the install script's search notice, `site/**` in the Rust workflow's paths and `site/*.js` in the
   front-end lint, and the tests. Merging it publishes the site, so it merges on the author's word,
   not on a green check.
10. **One voice in type** [built] (Section 4, U11): Pretendard and IBM Plex Mono shipped and served under
   `/fonts/` by the daemon, on both the local console and the read tier, and carried by the Settings
   page and the landing; `font-src 'self'` in the two policies; the shared stacks; the map's labels
   and the stage's in the reading face; the Settings page's off-scale sizes back on the scale; and the
   landing's fonts served from the site instead of Google Fonts.
11. **The principles page** [built] (Section 11a, U12, U13, U14): the page and its rules in the landing's
   stylesheet, the ledger written from the registry, the cases and the tests that replay them, the five
   demos and nineteen scenes, the tensions, the landing's three links, and `site/principles.js` in the
   front-end lint. Merging it publishes the site, so it merges on the author's word.

12. **The foundation** [built] (U16, U17, U19, and the guards of U15-U20): the band in the viewer
   and its two copies, relation kinds without color, and region hues on the band's arcs (Section 5);
   the tier names and the event sentences in one table each, and Setup's name (Section 9); the
   control helpers in the viewer and in the app's shared script: switch, pressed, radio group, tabs,
   the dialog that holds focus, and the live region; and the guards, which hold at once the rules
   that need no screen change and stay owed for the rest until steps 13-16 meet them.
13. **The defects** [viewer and engine built; app and site designed]: the seventeen found in Section 1's pass that ask no design question,
   each fixed as it is - in the viewer, search's chosen row and its field, the status bar's overflow
   and its empty counts, the Review page's keys and focus, the controls' states, names and targets;
   in the engine, `propose` turning names into ids, the Review page showing names whole, and a gate act
   from All filed in its observation's workspace instead of the node default; on the
   app's pages, the download that locks the Settings page, a running step lost on leaving Setup, its
   three sentences, its progress, the server list's unnamed button and the dialog's focus; on the
   site, the cut install command, the ledger on phones, the owed contrast, the header's spacing and
   the heading order.
14. **The Graph page** [built] (U15, U18, U20): search's groups and scope, with the viewer's
   search function that says how each hit was reached (Section 8); the first view, the status bar,
   Review's entry with Needs you's tie-only contested items, and follow (Section 6); the Log's
   who-lines (Section 9); View's Regions and Boundaries and the switch for single keys (Sections 5,
   10); and settling a contested value, with `/api/resolve`'s demotion kind and each explain
   candidate's tier before any verdict (Section 7). U8's guard follows the contested pill to the read
   tier.
15. **The app's pages** [designed] (Section 6a): Setup's name, its plan and its sentences; the
   Settings page's one card per fact, its radio group, its peers, and the capture switch with
   `connect claude-code --no-capture` removing the hook. It ships with the Setup page itself
   (onboarding.md Section 13 step 3).
16. **The site** [designed] (Sections 11, 11a): the landing's next step and Install link, the
   principles page's glossary, its ledger by chapter on phones and its scenes played by a press.
   Merging it publishes the site, so it merges on the author's word, right before the release that
   carries step 15.

Each step is judged at the shell's default size and at 1440, 760 and 390 pixels wide, on the busiest
hub, an isolated node, a contested node (a seeded store, as inspector.md Section 8 notes), a blocked
proposal and the read tier.

## 15. What this revises

- **settings-page.md Section 5**: the tray menu gains "Review (N)..." while something waits on this
  Mac's daemon. The status line is unchanged, and on a remote profile the tray is as remote-viewer.md
  Section 6 leaves it.
- **inspector.md**: the panel moves from the bottom to a sheet on the right, and the scope line's
  words change. Its rules and D1-D7 do not.
- **The viewer's dock**: Proposals and Review leave for the Review page; the Types tab's rows join the
  filters dock and the Peers tab's join Settings > Sync before those tabs go. The Log stays.
- **settings-page.md Section 3.0**: the title bar is still the viewer's on both pages, now without
  the glass and the rule; both drop them in step 7.
- **The viewer's chrome** (steps 7-8): the docks stop floating and join the frame, their edge
  handles become the title bar's panel toggles, the minimap moves above the camera toolbar, and the
  boundaries' legend moves into View.
- **remote-viewer.md Section 3.2** (step 10): the read tier serves the files under `/fonts/` as it
  serves the page's own assets - no workspace is involved - with a long cache, since the path is
  versioned.
- **settings-page.md** (step 10): the Settings page's policy gains `font-src 'self'`.
- **inspector.md D1** stands (step 14): the contested block keeps its place, stays expanded and keeps
  its actions, which become those Section 7 draws - one Confirm per value with its confirmation, Undo
  in the notice, and "Neither is right".
- **settings-page.md Sections 3.1-3.5 and 5** (step 15): the server list as a radio group, the capture
  switch, drift as one card, one row per peer, About's "Open Setup", and the tray's "Finish
  Setup..." (Section 6a).
- **onboarding.md Section 6** (step 15): the page is Setup, its plan is checkboxes, its acts wait for
  Set up, the tray item stays while any step is not done, and the privacy line is the one Section 6a
  gives.
- **client-connect.md Section 12** (step 15): `connect claude-code --no-capture` on an app already
  connected through the bridge removes the capture hook, which today it leaves in place.
- **The search path** (step 14): `/api/search` answers each hit with how it was reached. The MCP
  tool's answer is unchanged.
- **The resolve and explain paths** (step 14): `/api/resolve` takes `kind=claim_demotion` beside its
  promotion and files either in the observation's workspace; `/api/explain` answers each candidate's
  tier before any person's verdict. Both stay viewer paths, refused or read as remote-viewer.md
  Section 3 has them.
- **remote-viewer.md Section 3.5**: the local `/api/surface` also names the node's host id, which the
  remembered layout is keyed by (Section 5) and which a host is compared with before it is called
  "this node" (Section 9). The read tier's answer is unchanged.

## 16. Closure map

| Demand | Where it is answered |
|---|---|
| P2 - a claim says where it came from | Sections 5, 7 (who said it on every evidence row), 9 (hosts named by id), 11 (the stage's provenance and receipts); U10 |
| P5 - absence is not negation | Section 6 (rows moved before tabs go, the strip's empty state); Section 8 (empty search); U8 |
| P6 - a contradiction is surfaced | Sections 5, 11; U3, U10 |
| P17 - the read tier reads what it was granted | Sections 5 (no remembered layout there), 8; U6 |
| P18 - trust is the receiver's evaluation | Sections 5, 11 (the travel step: a bare claim counts for what its signature proves); U2, U10 |
| P19 - a deterministic core, the edge labelled | Section 8; U6 |
| P21 - the MCP surface stays narrow | Section 2; U6 |
| P22 - curation where the reader already is | Section 7 (Health folded, the review count where the person looks) |
| P23 - the gate decides, one verdict at a time | Section 7; U4 |
| P24 - fail loudly | Sections 6, 9; U1, U8 |
| P1-P24 - the principles where a visitor reads them, each clause with its state | Section 11a; U12, U13, U14 |
| inspector.md D3, D4 - silence declared, display reads the effective tier | Section 5; U2 |
| inspector.md D5 - the viewer never re-resolves identity | Section 8; U6 |
| remote-viewer.md Section 6 - the read tier offers nothing it refuses | Sections 6, 7 |
| dreaming.md Section 8 - drafts wait apart, decided by the person | Section 7; U5 |
| P5, P19 - search says what it searched and how it matched | Section 8; U15 |
| P6, P18 - no type color reads as a signal | Section 5; U16 |
| P2 - who said it, in the reader's words | Sections 6, 9; U17 |
| P3, P6, P23 - a contested value is settled by a gated act that can be reversed | Section 7; U18 |
| P22, P24 - the way to a decision stays where the reader is, and nothing moves them | Section 6; U20 |

## 17. What the review changed

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

Steps 7 and 8 were read the same way before they were built, and eleven findings changed them:

- **The token change has to land in all three files at once**, or U7's test fails between the steps.
- **`--panel` would have darkened**, and under the gold glow the window's top edge measured about
  1.00:1 against the frame. `--panel` stays and the glow goes.
- **A selection on a translucent tint fell below 4.5:1** (`--faint` at 4.18:1). It is the opaque
  `--select` now, inside the test's pairs.
- **The Settings page shares the title bar** (settings-page.md Section 3.0), so it changes with the
  viewer's.
- **Gold's rule contradicted its own complaint** about the selected chip, the contested box was
  dashed in gold though contested is `--bad`, and hover, the mark and the landing were not accounted
  for.
- **U9 was not testable as written.** Focus is an outline; lines also came from `border-color`, ring
  shadows and the script; text fields, key caps, the spinner and the boundaries' swatches were missing
  from the list; and by the stated rule the counts were 53, 12 and 19, not 58, 13 and 21.
- **The strip had no data while it shows**: the Log loads only while it is open. The strip reads one
  observation on its own, with its empty and failed states.
- **Most stored View choices would have kept the minimap on**, since any click stored them all.
- **A closed left panel hid the scope's one tier**, which U2 has the legend say; the status bar says
  it then. View had no boundaries control to sit beside, so it gains one.
- **The geometry was unstated**: the canvas stays the window's size, and the window is about as wide
  as the docks left.
- Smaller: two notes that said "today" of things already built, a clause that would have changed the
  body font, the U9 deferral's trigger, and the Settings page's 10.5px labels.

Step 9 was read the same way before it was built, and thirteen findings changed it:

- **The travel scene drew the wrong tier.** The draft had the second machine draw the person's
  confirmation as host signed, a thin ring. A verdict is an entry whose ceiling is read from the log,
  the same on every node (resolution.md Section 6), so the receiver draws the same gold ring. R4's cap
  is for a bare claim, and the step says so in words.
- **The log does not record which agent wrote an entry.** Provenance holds the machine, the person,
  the source and the confidence; the draft's "by claude-code" said otherwise, and its "signed" was
  true only once an entry leaves the machine.
- **Gold and teal had new meanings.** Gold drew a person, their verdicts and the search's matches, and
  teal on a Log row means host signed. The person is ink now, ticks take the viewer's Log colors,
  matches are lit as the viewer's search lights them, and teal is the viewer's footprint: an agent at
  work.
- **"The stage's colors are the landing's tokens" was not true of its type fills or its edges**, and
  the landing declared neither `--bad` nor `--ink2`. The exceptions are named and the tokens declared.
- **The tests reading `site/` did not run when only `site/` changed**, and nothing linted the stage.
- **The merge and the confirmation were not the viewer's.** The merge is the viewer's preview arrow,
  the confirmation is an agent's claim accepted on the Review page, and it stays off the contested
  node so the badge that travels is still there.
- **The stage's five type names collided in pairs** under the viewer's own color function; they are
  chosen apart now.
- Smaller: the version badge's contract, the rail and the stage's own timing were unstated; a stage
  that failed to load left a blank box; a phone frame at 760 pixels was taller than the window; Section
  4's six sizes did not say they are the tools'; U7 and U10 named checks no test could make; the
  install script still said its binary searched by keyword only; and three of Section 1's measurements
  were stated too strongly.

## 18. What building it changed

- **Step 1: the floor widened the right dock.** At 11px its five tabs with their counts measure 300
  pixels against a 274-pixel row, so the dock went from 300 to 336 pixels wide and its tabs stopped
  clipping. Step 3 took two of the five away, and the dock went back to 300 pixels; step 4 takes two
  more.
- **Step 1: the base rule for inputs and buttons moved to zero specificity.** Chips, legend entries
  and proposal and log rows became buttons, and the old base rule outranked nothing but element
  selectors, so it would have drawn its own box and hover color over each of them. At zero
  specificity every component class overrides it whole.
- **Step 2: the remembered layout had to move to this window's centre.** The sim's anchor pulls the
  centroid to the window's world centre every frame, so positions saved under another window size
  drifted out of the frame fitted around them. They are moved to the centre first, then framed at
  once: the reveal that frames a fresh layout may already have passed while the page waited for its
  first graph.
- **Step 2: notices moved to the top centre.** At the bottom they covered the inspector, where most
  of the acts they answer are made.
- **Step 2: under 760 pixels the camera buttons moved to the top right**, out of the sheets' way, and
  framing leaves out the sheet's height.
- **Step 3: the moved rows read in plain words.** The Peers tab's "local +N / hub +N" and "surface
  unknown" became "this node +N, hub +N" and "what it shares: unknown - no answer yet", in the viewer's
  settings dialog and on the app's Settings > Sync alike, and the dialog's tab is called Sync.
- **Step 3: the ordering was split.** Places and the Review page were one step; the places shipped
  first, so the Review page is step 4, search step 5 and the landing's picture step 6.
- **Step 4: blocked proposals wait under Needs you.** A merge whose verdict is cast but that a check
  blocks is not decided until someone rejects it, so it is listed with its reason and offered only
  Reject; on the author's store that is 38 of them.
- **Step 4: the Review page redraws only when what it shows changes.** Redrawn on every 2.5-second
  poll, it replaced the button under the pointer and took the keyboard focus away.
- **Step 4: the tray counts proposals only** (Section 7), and the right dock is the Log alone.
- **Step 5: an absent scope reads as the map's.** With no workspace parameter the search path searched
  every workspace while `/api/graph` read the default one, so the list could name what the map did not
  load. It reads as the graph does now, and an answer is shown only for the scope it was asked in.
- **Step 5: an entity the map does not draw stays listed.** A hit whose type is switched off is part
  of the answer; its row says it is not on the map in this view instead of dropping out of the list.
- **Step 5: the header sits above the docks and the Review page.** It overlaps neither, and the list
  it holds drops over both.
- **Step 6: the picture is of a made-up store.** The old one showed a seeded workspace too; the new
  one is seeded the same way, in a fresh home, with nothing from a real store. The landing's comment
  says how to retake it.
- **Step 6: the landing says Review.** It still sent the reader to the curation console, a place the
  viewer no longer has, in three places (Section 12).
- **Step 7: the curation rows had been unstyled since step 4.** Their rules were scoped to the
  curation panel's container, which left the page with the Review page; the rows render inside the
  Review page's detail now, and their rules moved there. The proposals list's rules and the dock's
  tab rules, whose markup had gone too, were removed.
- **Step 7: a panel toggle that is on is not gold.** Both panels are open by default, so a gold
  pressed state would have put two gold marks in the title bar at all times; it is drawn in `--ink`.
- **Step 7: between 760 and 1200 pixels a floating panel ends above the camera toolbar**, which
  otherwise sat on the Log's last rows.
- **Step 7: the workspaces' label became the panel's first heading**, "Workspace", like the headings
  under it.
- **Step 8: a card is a stack of surfaces.** The Settings page's rows, the About page's pairs and the
  landing's tool list and roadmap were ruled apart; each row is its own surface now, with two pixels
  of the ground between rows.
- **Step 8: the status dots lost their halos and the fields their focus glow.** Both were ring-shaped
  shadows; a field takes the focus outline every control takes.
- **Step 8: the landing keeps its uppercase eyebrows** (Section 4). They are its display type, like
  its faces; the rule against uppercase is about the tools.
- **After step 7: the camera framed the canvas, not the map's window.** `fitView` still framed the
  whole canvas, as when the docks floated over it, and in v0.6.0 a seven-node workspace zoomed to the
  cap and ran under both panels. Framing, the zoom buttons and the minimap's pan work in the window
  now, and the frame keeps room for the label that hangs right of a node, since the window's edge is
  hard.
- **After step 7: the minimap starts on again.** Step 7 started it off to clear the corner; at the
  author's request it starts on, above the toolbar, and View still turns it off. Stored choices from
  0.6.0 and 0.6.1 held "off" for anyone who had clicked any View toggle, so they forget the minimap's
  once more.
- **After step 7: the footprint glow is centred on its node.** It was a disc dropped below the node,
  which read as a circle out of place. It is a light that fades out from the node's edge, which no
  one mistakes for a trust ring.
- **Step 9: the page has about 1,050 words of main text, not 850.** Counted the way Section 1 counts
  (code excluded), the snippets, the tool list and the status lists are words too; the prose is near
  the plan. Before it the page had about 1,900.
- **Step 9: the stage's type names were chosen by their colors.** Under the viewer's own function the
  draft's five names fell into two near pairs. App, Database, Feature, Pattern and Event sit at least
  45 degrees of hue apart and away from gold, coral and teal.
- **Step 9: every ring and badge is set in one function**, which the first machine's scene and the
  second machine's both call, so the receiver cannot draw a mark the sender did not; the test reads
  that rather than matching each mark by name.
- **Step 9: the stage is linted from the repository's root**, with a configuration of its own: ESLint
  lints nothing outside the directory it starts from, and the viewer's configuration lives beside the
  viewer.
- **Step 9: the Settings page's selected section had drawn no surface since step 7.** It read
  `--select`, which only the viewer declared; the new test found it, and the page's lowest text pair
  is now `--faint` on `--select`, 4.51:1.
- **Step 9: on a phone a long command widened the page.** A one-column grid of `1fr` lets its widest
  line set the column, so a code block made the layout 442 pixels wide on a 390-pixel screen; the
  columns are `minmax(0, 1fr)`.
- **Step 9: measured, the stage costs little.** Drawn in full at 1000 by 625 at twice the density, a
  frame of the busiest scene took under half a millisecond with the CPU slowed four times; at rest it
  draws no frame at all.
- **After step 9: regions are clouds.** A region was its hull filled flat, at 0.34 for the active
  node's: a slab with a crisp rim, and the active node's five contexts read as a patchwork. The
  landing's stage drew its one region as a soft light instead, and the viewer takes it: an ellipse
  fitted to the members' principal axis, filled with one radial gradient. Layers of the hull shrinking
  toward its centre were tried first; they followed the hull's shape but showed their steps as
  contour rings.
- **Step 10: the viewer's chrome had been set in the identifier face.** Its body defaulted to the
  mono stack, so every chip, legend entry, count and timestamp was mono, not only identifiers. The
  body is Pretendard now, and the identifiers are named: the edge legend's entries and the
  inspector's relation headers, besides the map's relation labels.
- **Step 10: a label baked before its face loaded kept the fallback.** The map bakes each name into
  a sprite once; a sprite made in the first frames held the system face's glyphs and its measure. The
  sprites are dropped and the map drawn again when a face finishes loading.
- **Step 10: an English page needs nine of Pretendard's 92 files.** The author's subset groups
  symbols by their use in Korean text, so Latin and its punctuation are spread over nine files, not
  one; the Settings page and the landing ship those nine with the author's rules for them, copied
  unchanged.
- **Step 10: the faces are answered before anything else.** On the local console a font request is
  answered before the router's blocking work, and on the read tier before the audit, which records
  reads and would otherwise log the twenty files one page load fetches.
- **Step 10: the Settings page's off-scale sizes rounded to the nearest step** - 11.5 to 12 (11 for
  its pill), 12.5 to 13, 15.5 to 15 - and the viewer's observation text, which is often Korean, runs
  at 1.55 and its card at 1.6.
- **After step 10: every region is drawn by default.** As flat hulls, regions drawn all at once
  overlapped into one slab, which is why only the active node's were drawn. As clouds they read as a
  faint field (0.10 at the core at rest), so the author asked for them on by default. Their names are
  still the active node's alone: on the author's store, twenty region names at rest took the places
  of the node labels. A stored "off" from before is forgotten once, as the minimap's was.
- **After step 10: workspaces cohere.** With every workspace in scope, a workspace's loose pieces -
  a few nodes no edge ties to the rest - drifted off under a repulsion whose range on a large graph
  is about 960px, held by a centre pull the graph's size had divided down to almost nothing, and the
  boundaries enclosed mostly empty space. Measured on a copy of the author's store (392 nodes in five
  workspaces), the farthest node from its workspace's centre went from 2292 to 530 in one workspace
  and from 2086 to 327 in another, the large ones kept their shape, and the nodes lying closer to
  another workspace's centre than to their own went from 113 to 0. The push first used the radius a
  workspace's nodes would fill; a small workspace's centre then settled inside a large one's real
  spread, so the push uses the 90th percentile of the actual distances. A layout remembered for a
  scope of several workspaces is forgotten once, since a warm start would keep the scattered picture.
- **After step 10: clouds are mixed in OKLCH.** The canvas composites in sRGB, so overlapping clouds
  met in greys and browns, and the HSL band gave some hues 50% lightness and others 66%. The clouds
  of a frame are now mixed in one field at a quarter of the screen's resolution - a cloud has no
  detail to lose - and drawn smoothed up: 1.7ms a frame at 1600 by 900 on the author's store. The
  minimap, too small to show a mix, keeps the canvas's compositing with the same colours.
- **After step 10: links fit their density.** Every link had one rest length (120) and one strength,
  and the repulsion a large graph needs held linked nodes about twice that far apart, so long links
  crossed a cluster from side to side. On the author's store, one workspace's median link went from
  235 to 158 and its 90th percentile from 380 to 259, with the spacing between neighbours nearly
  unchanged (76 to 69); in the view of every workspace, the two large ones' medians went from 230
  and 251 to 176 and 191. Leaves now gather around their hubs instead of lying anywhere in the disk.
- **After step 10: a cloud is not the ground.** Zoomed into a cluster, one region spans the window and
  ten overlap, so the clouds covered 96% of a 2.2x view at a mean 0.37. A cloud wider than 45% of the
  window now fades by how much wider it is, and overlapping coverage eases toward 0.42 instead of
  stacking: the same view is covered at a mean 0.11. Node labels are thinned by the nodes in view, not
  the graph's count, so zooming into a large graph names its cluster progressively.
- **Step 11: the cases moved two lines of copy.** P21's line said each tool is one intent, but the
  registry owes that clause, so the line says thirteen tools and no query language, and its first
  beat carries the owed tag. P23's note while the proposal waits was about Postgres, the merge's own
  target, which would show a verdict on a base that had moved - and binding a verdict to its base is
  owed - so the note is about the orders service.
- **Step 11: ten rulings are owed, not six.** Appendix A names the quarantine tier, the tombstone,
  retrieval demotion, the auto-merge band and the checks that route review; deriving each tag from
  the clauses its ruling rests on also tags the gate for type changes, asking a person through
  elicitation, the replicated verdict marker, the one-intent surface and excision.
- **Step 11: a mention without a type asserts none.** An entry that names billing-api untyped leaves
  its kind to the entries that type it, so a machine that receives the relation first draws a
  Concept that later takes its type, and no order contests it; the engine agrees on the stage's
  project, whose digest is the same on every order the replay tries.

## 19. What the second pass's review changed

The second pass (steps 12-16) was reviewed once against the code before it was built. Thirteen
findings changed it:

1. **Undo was not a path that existed.** `/api/resolve` only promotes, and the fold takes the higher
   of a promotion and the tier it had, so a "promotion" down does nothing. The path gains a demotion
   kind, the target tier is the one the row showed before, and the ceiling a demotion leaves is said
   (Sections 2, 7, 12).
2. **Confirm could not settle a Human confirmed value.** Confirming the other value tied them again.
   That row's act is "Confirm instead", which demotes the shown value to its tier before any verdict
   (Section 7).
3. **The Review count would never come back to "Review".** Needs you listed every conflict with
   competitors, and a confirmation leaves the competitors. It now lists only ties; a settled conflict
   is under Health (Sections 6, 7).
4. **Search's second group mixed two things, and its count was a page.** Neighbors are not related by
   meaning, and keyword mode adds them too. One "Related" group says per row how a hit was reached,
   and a count at the limit says "20+" (Section 8).
5. **Lightness did not promise contrast.** A violet at 0.62 measures 5.1:1, and a first-tone hue sat
   0.109 from `--muted`; both are measured now, with their own thresholds (Section 5).
6. **The single-key rule contradicted the viewer's own keys.** /, b, [ and ] act from anywhere; they
   can be turned off instead, and only the deciding keys are scoped (Section 10, U19).
7. **A radio group of servers would switch every AI app on each arrow press.** The list keeps Use on
   each row (Section 6a).
8. **The plan's new items had no default, and the new tray rule held a step nobody could set aside.**
   Replacing items start unchecked; the tray keeps onboarding.md's rule, and the way back is named in
   About (Section 6a).
9. **The capture switch could connect an app.** It shows only on a row connected through the bridge
   (Section 6a).
10. **Stale text.** The contested pill, the gold rule's review pill, the regions' default and the
    Setup page's old names elsewhere were brought in line.
11. **The event stream carries no tier, author or time.** The who-line is the observation rows', and
    the follow notice names the workspace (Sections 6, 9, U17).
12. **A confirmation from All landed in the node default**, where the fold that reads the
    observation's workspace never sees it. Every gate act is filed in its observation's workspace
    (Section 7, step 13).
13. **Absolute and overstated words.** U20 now says what happens when a merge or a sync takes the
    selection away, and the privacy line says what supragnosis keeps rather than what never leaves.

## 20. What building the second pass's foundation changed

- **The copies were compared in part.** The stage's and the principles page's tripwires read a
  function up to its first line holding a lone closing brace, which was the end of its first loop, so
  most of `stableColors` was never compared. They count brackets now.
- **The inspector still spelled tiers.** Its scope line read "same across this scope: agent_extracted",
  where Section 9's table shows "All here: Agent", and the observation card, the provenance rows and
  the tooltip printed tier ids too. They read `tierName` now, and the guard reads every interpolation,
  nested ones included: its first version stopped at the outer template and missed a planted
  `${n.trust_tier}`.
- **The second tone was raised.** At the lightness first drawn, two second-tone colors measured
  5.16:1 and 5.47:1; the four darkest moved from 0.62 to 0.64 or 0.65 of OKLCH lightness, and the
  lowest is now 5.57:1.
- **Boundaries and peers drew from the generator too.** A workspace's dashed outline and a peer's dot
  took any hue, gold and teal among them; both draw from the band now.
- **The workspace chips choose one scope**, so they are a radio group, not pressed buttons (Section 4).

## 21. What building the Graph page changed

- **The search path says how a hit was reached; the MCP tool does not.** The viewer's engine function
  reads the keyword list, the fused list and the enrichment, and answers each hit with `reached`
  (words, meaning or neighbor); `search_knowledge` is untouched, and its pinned tool list with it.
- **The wider scope's count is the word matches.** A first version counted every hit the wider search
  answered, so a scope full of related hits read "20+"; it counts the hits that contain the words, and
  says 20+ only when those reach the path's limit.
- **The single keys are turned off in View**, beside what they act on, rather than in the settings
  dialog, which holds About and Sync (Section 10).
- **View's stored choices carry over.** "hulls" and "all regions" were stored as two switches; a
  stored pair becomes the one Regions choice the first time the page reads it.
- **Follow said nothing about reads elsewhere.** It framed an agent's reads as well as its writes;
  a read in the scope on screen still frames what it touched, and a read elsewhere is not a notice.
- **The first view needed a chip for an empty default.** The workspace list names only workspaces
  that hold knowledge, so a default that holds none had no chip to select; it gets a "default" chip
  while it is the scope.
- **A Log row's mark is the legend's ring.** The dot had its own colors - Host signed in teal, the
  color of healthy - and every row, card and strip now draws the Trust legend's swatch instead.
- **The block's hint took the inspector's class.** "Neither is right" first used `.why`, which the
  inspector looks up for its own disclosure, so focusing a contested node threw; the hint has its own
  class and `settle.rs` holds the name apart.

