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
> pass over every screen (Section 1, U15-U20) is built - steps 12-16 - and U19's every-surface half
> waits on browser scenarios. The third pass, for reading and pressing on a Mac (Section 1, U21-U27),
> is built - steps 17-24: its defects, reading, lines, shapes and targets, words on the screen, the
> map's List, the site's type, the title bar and the light appearance. The fourth pass, for a map of
> many nodes (Section 26, U28-U32), is built - steps 25-30: the bench, the simulation at rest, the
> grids, drawing less, still layers, and the measure that finds the budget met in Chrome and WebKit.
> The fifth pass, for the map at any size (Section 27, U33-U40), is built - steps 31-38: the poll that
> asks first, region forces that scale, the layout on its own thread, the multilevel start, clipping,
> semantic zoom, the GPU renderer, and the measure. The sixth pass, one icon set and one notice
> (Section 28, U41-U42), is specified - steps 39-40.

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

**Measured again for reading and pressing**, on 2026-10-19 after step 16: 31 states of the viewer,
the Settings and Setup pages, the landing and the principles page, at 1440, 390 and 320 pixels (the
app's pages at 1280 and 720), with a script in each page that recorded every visible text's size,
face and contrast, every control's size, every corner, and the preferences the stylesheets answer.
Contrast was measured by WCAG 2's ratio and by APCA 0.0.98G, the method Apple's guidelines name
beside it, and the tools were held to Apple's Human Interface Guidelines for macOS 27 and to the
control metrics AppKit reports on macOS 27.0.1.

- **WCAG held; reading did not.** Five texts missed 4.5:1: the Trust legend's three entries when off
  (2.26:1) and the Review page's J and K key caps (3.68:1). But 446 of the viewer's 496 texts were
  11 or 12 pixels, drawn mostly in `--ink2` (APCA Lc 59 on `--panel`), `--muted` (45) and `--faint`
  (41). APCA's guidance asks Lc 90 of body text at 14 pixels and Lc 75 at 18, and leaves Lc 30 to 45
  to text no one has to read. Apple's dark appearance asks for 7:1, especially for small text, and a
  Mac sets its body text at 13 points in its primary label color, Lc 84 on its window. `--muted` and
  `--faint` measured 1.1:1 against each other: two tokens, one tier on the screen.
- **Text could not grow.** The viewer's stylesheet set 121 font sizes, the Settings page 24 and Setup
  33, all in pixels, so a browser's text size changed nothing, and the app's window had no zoom; in
  the app the only way to larger text was the system's magnifier (WCAG 1.4.4). With WCAG 1.4.12's
  spacing applied, the Log's two-line clamp hid the end of eight observations, whose full text was only
  in the inspector.
- **Targets were small.** Eight of the viewer's controls missed even U19's 24 pixels: the gear (18
  by 18), the review pill (19 tall), Definitions (16), the inspector's close (22), "Neither is
  right" (18), Ledger (21), the Review page's Graph link (18) and an entity chip (22). At 390 pixels
  the Review page's tabs were 26 tall and Accept, Reject and Skip 25, under U19's 28, and no control
  but the Log's and the Review queue's rows reached the 44 that iOS asks of a finger. A Mac's
  default target is 28 by 28.
- **The system's settings were not heard.** The stylesheets answered reduced motion and nothing else:
  not Increase Contrast, which WebKit reports as `prefers-contrast: more`, and not forced colors.
- **No shape system.** Section 3 named four corner radii; the viewer drew ten - 2, 3, 4, 5, 6, 8, 9,
  12 and 16 pixels and full round - the app's pages six and the site eleven, each a number written
  where it was used. Setup drew its apps' checkboxes in the system's blue and its other checkboxes in
  gold.
- **Explained only to a mouse.** The viewer wrote 58 tooltips and the app's pages 21, and many held
  the only explanation of what a control does - "every context region, faint, and the active node's
  at full strength" - which a keyboard and a finger never see. The map is a canvas a screen reader
  cannot walk, and on it a node's type is told by color alone, one of sixteen.
- **One appearance.** Every screen was dark, and nothing followed the system's Light or Auto setting.
  Apple's guidelines keep a dark-only app for rare cases, immersive media among them, and ask that an
  app offer no appearance setting of its own; small text, which the tools are made of, reads better
  dark on light (Piepenbrock and others, Human Factors, 2014).
- **The title bar named the product.** It set "supragnosis" in mono at its leading end, the search at
  its centre and both panel toggles at its trailing end, and the app's pages closed with an x in the
  corner where a Mac closes its window. Apple's guidelines put a toolbar's sidebar toggle and title at
  the leading end and search at the trailing end, and title a window by what it shows, not by the app.
- **The site set its labels in mono.** 61% of the site's texts were IBM Plex Mono, which Section 4
  keeps for what is typed or copied, and the principles ledger's labels were 10 pixels.

The two missed contrasts, the short targets and five more ask no design question - Setup's two
kinds of checkbox, the search field named only by its placeholder, the status line cut short at 390
pixels, a privacy line 103 characters long, and the app's starting splash in the old tokens and the
mono face - and Section 14 step 17 fixes them. The rest change Sections 3, 4, 5, 6, 6a, 10, 11 and 11a.

Measured again on 2026-10-19 for a map of many nodes, in Chrome at 1440x900 at twice the pixels on
the author's Mac, on synthetic graphs: communities of 20 to 60 entities, about 1.5 relations per
entity and a region for every twelve, standing in for `/api/graph` and `/api/hypergraph` while the
viewer's own functions were timed from outside it (Section 26.1):

| Entities | Relations | First picture | One settling step | A frame while panning | Its simulation | Its drawing |
|---:|---:|---:|---:|---:|---:|---:|
| 500 | 738 | 2.1 s | 2.7 ms | 16.7 ms | 2.4 ms | 3.9 ms |
| 1,000 | 1,461 | 2.0 s | 10.9 ms | 16.7 ms | 6.2 ms | 6.1 ms |
| 2,000 | 2,935 | 4.8 s | 41.8 ms | 33.3 ms | 36.1 ms | 10.3 ms |
| 5,000 | 7,335 | 27 s | 244 ms | 183 ms | 214 ms | 26.5 ms |
| 10,000 | 14,690 | 98 s | 875 ms | 833 ms | 763 ms | 55 ms |

- **The map kept 60 frames a second to 1,000 entities.** From 2,000 the simulation took 85 to 90% of
  every frame: it visits every pair of nodes, about 50 million at 10,000, and every frame runs it -
  a pan, a zoom and a hover included - because its collision pass went on after the layout had come
  to rest, and the frame that draws is the frame that steps.
- **Drawing alone passed the budget between 2,000 and 5,000.** Every relation was its own path with
  its own opacity and color, every node up to three discs and a stroke, nothing outside the window
  was skipped, and a node a pixel wide was drawn as fully as one forty wide. The region field was
  recomputed on the processor for every frame of a pan.
- **At 10,000 the map did not read.** The nodes' halos covered their fills, and the communities the
  graph was made of showed as one dark disc.

Section 26 is what the fourth pass changes; the techniques it takes are the ones large-graph renderers
publish (sigma.js, Cytoscape.js, d3-force, ForceAtlas2) and its sources are listed there. Section 26.2
holds the same measure taken after it.

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
| `--ink` | `#e9e4d6` | primary text: names, values, headings |
| `--ink2` | `#cad2de` | body text: what a person reads to act, a row's text, a panel's heading |
| `--muted` | `#b0b8c7` | secondary text: counts, times, hints, placeholders |
| `--accent` / `--gold-bright` | `#d9a544` / `#f0c469` | the brand gold - the mark, fills and tints - and gold as a line: the human-confirmed ring, focus |
| `--gold-ink` | `#f0c469` | gold drawn as text: a selected choice, a pill that waits for a person |
| `--ok` / `--ok-ink` | `#56b3a2` / `#78c8b8` | success, connected, healthy; the second when it is text |
| `--warn` / `--warn-ink` | `#e0a24a` / `#e9af60` | needs attention; the second when it is text |
| `--bad` / `--bad-ink` | `#e07a5f` / `#fca18a` | error, blocked, contested; the second when it is text |

The landing renames to match (`--ink-0` to `--surface`, `--text` to `--ink`, `--gold` to `--accent`),
the viewer's `--teal` becomes `--ok`, and its literal `#d96a5f` becomes `--bad`. A surface declares
the tokens it uses; the core - `--surface`, `--panel`, `--line`, `--ink`, `--muted` and
`--accent` - is declared by all three. Every text token measures at least 4.5:1 on every surface
token. `--faint` moves once more for that, from step 0's `#7b8494` (4.46:1 on `--panel-3`) to
`#7f8898`, whose lowest pair is 4.71:1. Step 7 raises `--panel-2` and `--panel-3` so that a step of
tone can do what a border did (Section 4), adds `--select`, and moves `--faint` to `#868f9f`, whose
lowest pair, on `--select`, is 4.51:1. `--panel` stays: darker, its edge against the map would
vanish. Because U7's test holds the shared values equal, step 7 changes them in all three
stylesheets at once, and step 8 changes how the other two draw.

The third pass retires `--faint` into `--muted` and raises `--ink2` and `--muted` (Section 1):
`--faint` and `--muted` were one tier on the screen, and APCA put both below text anyone has to read.
Every text token now also measures APCA Lc 60 or more on every surface token, and `--ink2`, which
carries what a person reads to act, Lc 75. These are this document's floors, not APCA's: APCA's own
table asks Lc 90 of body text at 14 pixels and Lc 75 at 18, which no tier but `--ink` could give
11-pixel text. The floors lift every tier off the bottom of APCA's scale, where Lc 30 to 45 is left to
text no one has to read, and size does the rest (Section 4). The lowest pair of `--muted` measures
Lc 60.2 and 7.36:1, of `--ink2` Lc 75.3 and 9.64:1. A signal's color is a mark's; drawn as text it
takes its `-ink` token, and gold drawn as text is `--gold-ink`. The brand gold, `--accent`, is the
same `#d9a544` in both appearances and is never darkened: the mark is drawn in it, a logotype outside
the text floors (WCAG 1.4.3's exception), and fills and tints carry it, with dark text on a gold fill.
On the site, which draws its small gold labels in the brand's own hue, that text is `--accent-ink`,
`#e1af54`, the brand gold raised only to the floor. A pill's tint is a token too -
`--ok-glass`, `--warn-glass`, `--bad-glass` and `--gold-glass`, each its signal at 12% - and the
text on a tint is measured over it on `--panel` and on `--panel-2`. A tier is not made by dimming:
what matters less is set smaller, lighter in weight or further down, in a color that still reads
(Section 4). A mark drawn in a text token moves with it: the Trust legend's unverified swatch and the
map's unverified ring, which take `--muted` together; the map's active edges and relation labels,
`--ink2` (U16); the ledger's owed outline; and the landing's stage and the principles page's scenes,
whose palettes copy the tokens.

**The tools follow the system's appearance** (U27). The viewer, the Settings page and Setup take
their light values under `prefers-color-scheme: light` and their dark ones otherwise; the landing and
the principles page stay dark, a page that presents the product in its own look. There is no
appearance setting in the product: a Mac's own setting, Light, Dark or Auto, decides. Every light
value is held to the floors the dark ones meet (U7) on the light surfaces:

| Token | Light | For |
|---|---|---|
| `--surface` / `--panel` | `#fcfbf8` / `#f3f1ec` | the map's ground, lightest, and the frame around it |
| `--panel-2` / `--panel-3` | `#ebe8e1` / `#e3dfd7` | controls and rows, and hover |
| `--select` | `#efe5d1` | the brand gold at 16% over `--panel`, opaque |
| `--line` / `--line-soft` | `#dcd8cf` / `#e7e4dd` | the lines Section 4 keeps |
| `--ink` / `--ink2` / `--muted` | `#16181e` / `#3b404b` / `#5c6370` | Lc 85.9, 75.5 and 61.6 at their lowest |
| `--accent` / `--gold-bright` | `#d9a544` / `#af7c04` | the brand gold, unchanged; and gold as a line, its hue deepened only to the 3:1 a line needs |
| `--gold-ink` | `#16181e` | gold drawn as text is the ink here: text on the light ground is dark, and the gold stays in the fill or tint behind it |
| `--ok` / `--ok-ink` | `#1d7a6a` / `#02665a` | success; the second as text |
| `--warn` / `--warn-ink` | `#b7791f` / `#875400` | needs attention; the second as text |
| `--bad` / `--bad-ink` | `#c4512f` / `#9f3816` | error and contested; the second as text |

The tints are their signals at 12%, as in the dark, and a shadow and the scrim behind a dialog are
tokens too, `--shadow` and `--scrim`, light enough on the light ground to read as depth rather than
dirt. Under Increase Contrast each light text token takes the next stronger one, as the dark ones do.

Space runs on a 4px step (4, 8, 12, 16, 24, 32, 48). Corners are six tokens (U22): `--r-window` 16
for the map's window (Section 6) and the app pages' content pane, `--r-surface` 12 for a surface
that floats or groups - a popover, a sheet, a card, a list's group - `--r-control` 8 for a control, a
chip and a row, `--r-inset` 4 for a key cap or a tag inside a row, `--r-mark` 2 for a mark of data -
a legend swatch, a ledger block - and `--r-capsule`, half the height, for a pill and a large control
(Section 4). A part inside a rounded part takes the outer radius less the space between their edges,
on the token nearest: a row inset 4 pixels in a 12-pixel card takes 8, and a row flush with its
group's edge takes the group's 12 on its outer corners and none inside. A dot is a circle. Layers
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
- **Six sizes** in the viewer and on the Settings and Setup pages, each a token in rem so that a
  person's own text size scales them (U21): `--text-page` 24 (a page's title), `--text-sheet` 18 (a
  sheet's title), `--text-section` 15 (a section), `--text-body` 13, `--text-small` 12 and
  `--text-caption` 11. They follow a Mac's text styles where a Mac has one - Body and Headline 13, Callout 12,
  Subheadline 11, Title 3 15 - and each is given by role, not by the room left:
  - 13 for what a person reads to act or to choose: running text, an observation's text, a row's
    first line, the inspector's rows, a notice, and the label of every control - a button, a field, a
    segment, a chip, a menu item, a legend entry - as a Mac sets its regular controls' labels at 13;
  - 12 for a row's second line, a description under a heading or an option, a hint, and a panel's
    heading, which is semibold, in `--ink2` and in sentence case;
  - 11 only for a count, a time, a key cap and the caption beside a value.

  Nothing is drawn below 11, nothing between the steps, and no size is written in pixels outside the
  tokens. The map's labels are drawn on a canvas, which rem does not reach, so they scale with the
  root's size as the page's text does. The landing, a page read at length rather than a tool, has six
  sizes of its own (Section 11). Nothing in the viewer or on the Settings and Setup pages is set in
  uppercase. The landing keeps its uppercase eyebrows in IBM Plex Mono: a page about the product, set
  in the product's type, not a tool.
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
- **A selected item sits on `--select`**, its text `--gold-ink` when it is a choice (a chip, a
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
  - a legend swatch of a line the map draws: the Trust legend's rings and the boundaries' outlines;
  - the outlined chip of a superseded relation, a thin `--muted` line;
  - the thin box around a contested value, in `--bad`, the contested color;
  - on the principles page's ledger, an owed clause's `--muted` outline and the chosen column's
    `--gold-bright` one (Section 11a);
  - a control's own glyph: a radio's circle, a switch's track, the loader's spinner;
  - under Increase Contrast, the edge of a control, a chip and a field (Section 10).

  On the canvas the rings, edges and boundaries are the data and stay.
- **Controls are a Mac's sizes** (U22). A control draws 24 pixels tall, a Mac's regular size, and
  the act a page or a decision is for draws 28, the large size, as a capsule; the controls of one row
  share one size, so a row whose act is large draws its other buttons large too. A pill is a capsule
  at any size, and every other control takes `--r-control`. A native checkbox or radio takes the
  accent through `accent-color`, and a drawn one - the viewer's switches and radios - through its
  track or Section 4's selected state, so the system's blue never sits beside a gold one. Every
  target reaches U19's size with its spacing (Section 10).
- **Lines are thin and solid** (U22). No line is dashed or dotted, on a page or on a canvas: a dash
  breaks a line into marks that read as noise at these sizes on a dark ground. A line that carries a
  fact is solid and 1 pixel, unless its weight is the fact, as trust's rings are (Section 5). A past
  relation is told by its color and its lowered strength, a boundary by its hue, and a direction by an
  arrowhead.
- **A tooltip names; it does not explain** (U25). It shows only to a pointer that rests, so it holds
  a control's name and its key - "Zoom in", "Filters panel ([)" - and nothing a person needs to act.
  What a control does is said on the screen: each of View's options carries a line under its name,
  the search mode says what it searched, and what a mark means is in the legend. Every tooltip is
  written through one helper that takes a name and a key, so the guard can count its words; an
  interpolated name counts as one.
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
| ring | trust of the best support | Unverified: a 1-pixel `--muted` ring. Agent: none. Host signed: 1.6 pixels, `--ink`. Human confirmed: 2.4 pixels, `--gold-bright`. The more support, the heavier the ring |
| badge | contested | a `--bad` disc with "!" at the node's upper right |
| halo | selected | a soft `--ink` disc behind the node, replacing the selection ring |
| glow | this session's footprint | a soft `--ok` light centred on the node and fading out, drawn beneath it, replacing the teal ring |
| pulse | activity | one expanding ring, none under reduced motion |
| edge | relation | neutral; on the active node's edges, brighter, labelled with the kind and ending in an arrowhead at its target |
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
is unverified therefore shows no rings and says so, rather than a ring on every node.

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

**The light band** (U27) keeps each hue and its slot and is measured by the same rules on the light
ground, but a mark on a light ground is told by being darker: every color has OKLCH lightness from
0.42 to 0.67 and measures 3:1 or more on `--panel`, the 3:1 a mark needs. Its sixteen colors are
`#647700`, `#4f995a`, `#0097b6`, `#006fcb`, `#4b29bb`, `#6e2093`, `#a6468a`, `#c0748e` in the first
tone and `#758a00`, `#007322`, `#006176`, `#1a5998`, `#736fbf`, `#9b74b2`, `#753963`, `#d7307e` in the
second; they were found by a search over each color's lightness and chroma inside what the screen can
show. One signal moved for it: the light `--ok` is a deeper teal, since the cyan hue could otherwise
sit near neither it nor the neutrals. The band is measured against the brand gold and the light line
gold too. A type keeps its slot, and so its hue, in
both appearances. The map redraws when the appearance changes: its palette, its band, the regions'
clouds and names, and the labels' halos.

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
browser, where those keys belong to the browser, the review pill and the address reach the page.

**The title bar is a Mac toolbar** (U26). At its leading end, after the traffic lights and the star,
sit the filters panel's toggle and the title of the place on screen - on the Graph page the scope,
"All workspaces" or "billing" - and at its trailing end the search field, then the right panel's
toggle. The product's name is not the title: a window's title names what it shows, and the Dock and
the menu bar already name the app. The bar holds no navigation but the way back: on every page but the
Graph, "< Graph" takes the filters toggle's place, Esc and Cmd+[ take it too, and Cmd+1 goes there
from anywhere. On the Review page the title is "Review" with its scope, which the page's own heading
gave before. Below 760 pixels the title gives its room to the search field. Every page has this one
bar: the star leads on the Review, Settings and Setup pages as on the Graph, and the way back and the
title stand where the Review page has them, so moving between pages changes the words in the bar and
never where they sit. Only the trailing end differs, since the app's pages have no search. In the app
the bar drags the window from anywhere but its controls, and a double-click zooms it, as a Mac's title
bar does; the search's results, which open inside it, are not part of it. The bar's leading end
clears the traffic lights, and in full screen, where macOS hides them, it moves to the window's edge -
on every page, including one opened while the window is already full screen.

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
- **The map has a List** (U24). List, the first button of the camera toolbar, turns the map's window
  into a table of the entities the map draws - the same scope, the same filters, the same search -
  sorted by type and then by name, each row naming the entity, its type, its trust by the legend's
  name and its count of relations, and its workspace when the scope holds more than one. Regions are
  not its groups: they overlap, and an entity can sit in several or in none. A row is a button that
  selects its node as a click on the map does, and opens the sheet; List again, or Esc in the table,
  gives the map back with that node selected. The canvas's name says what it draws and where else it
  is: "Map of 20 entities and 14 relations in 10 regions. List shows them as a table." The table is
  the map for a screen reader and for anyone who cannot tell sixteen colors apart: it names each
  type in words.
- **The camera is one toolbar** at the window's lower right: List, view, zoom in, zoom out, fit. Under 760
  pixels it stays at the top right, out of the sheets' way (Section 18). The minimap is on, above the
  toolbar, until View turns it off; between 760 and 1200 pixels it gives way to a panel floating over
  the map's right side, and under 760 it is not drawn. View's stored choices forget the minimap's
  once, since any click stored every choice and most were never made, and View remembers it from then
  on.
- **The status bar is the window's state, said once.** On the left, the scope and its counts in
  words: "All workspaces - 20 entities, 14 relations, 10 regions". Counts per type are the legend
  entries' to show, not the bar's. On the right: the way to Review, the sync pill, the search mode,
  then the gear, each a target of U19's size. A pill is tinted, not outlined. How fresh the map is goes unsaid while it is fresh;
  once a poll fails, the bar says "Not updated since 8:54 - the server is not answering", and the
  time does not tick at every poll.
- **Review is always in the status bar** on the local console (U20). It reads "Review" when nothing
  waits and "2 to review (1 contested)" when something does: one count, the items under Needs you,
  and in the parenthesis how many of them are contested beliefs. Needs you lists only beliefs that
  tie on trust (resolution.md Section 4.1), so those are the loaded scope's contested nodes, which U3
  asks the bar to count whatever the legend hides; before the curation read answers, the graph's
  count stands in. A conflict that trust settled is listed under Health, so a confirmation takes its
  item off the count. It is a button, the Graph page's way
  to Review wherever Cmd+2 does not reach, and it is not in the title bar, which holds no navigation
  but the way back.
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
Their rules are in those documents, and the second pass changes four things in them, and the third
pass a fifth:

- **The app's pages go back; they do not close** (U26). Settings and Setup are pages of the main
  window, so the leading end of their title bar holds, after the star, "< Graph", where the crumb
  was, and the x at the trailing end goes: an x in a window's corner reads as closing the window, which a Mac's traffic
  lights already do. The title is the page's - "Setup", or the Settings pane on screen, "Server" - and
  Esc and Cmd+[ go back too (settings-page.md Section 3.0).

- **One name: Setup.** The first-run page is Setup everywhere: its title bar says Setup, its heading
  "Set up supragnosis", the tray's item "Finish Setup..." and About's act "Open Setup". "Welcome"
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
tier (Section 5): a 1-pixel `--muted` ring for Unverified, none for Agent, a 1.6-pixel `--ink` ring for
Host signed and a 2.4-pixel `--gold-bright` ring for Human confirmed.

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
- Under reduced motion nothing moves: no pulse, no sliding docks, no spinner, and
  the layout settles in one frame.
- Text drawn at full strength on a token surface meets 4.5:1 (U7). One thing is outside the pairs on
  purpose: labels the map dims to put the focus forward. Until step 7 the panels were translucent
  glass whose ground was whatever the canvas showed through; they are opaque `--panel` from step 7,
  so their text is inside the pairs. It also meets APCA's Lc 60, and body text Lc 75 (Section 3).
- **Increase Contrast and forced colors** (U23). Under `prefers-contrast: more`, which a Mac's
  Increase Contrast sets, each text token takes the next stronger one - `--muted` reads as `--ink2`,
  `--ink2` as `--ink` - every control, chip and field draws a 1-pixel edge that measures 3:1 on its
  surface, and the focus ring is 3 pixels. Under `forced-colors: active` the page takes the system's
  colors and the map redraws in them - `CanvasText` for names, edges and rings, `Canvas` for the
  ground, `Highlight` for the selection - and keeps every mark it draws. The human-confirmed ring,
  told from the host-signed one by its gold alone, is drawn twice as wide there.
- **Text can grow** (U21). Every size is a rem token (Section 4), so a browser's text size scales the
  page. The app's View menu has Make Text Bigger, Make Text Smaller and Actual Size (Cmd+=, Cmd+-
  and Cmd+0), which set the root's size from 100% to 200% in steps of 12.5%, and the choice is kept
  for the next launch. Scaling the root rather than the whole webview keeps the title bar's height
  and its traffic lights' place, which settings-page.md Section 3.0 fixes; what the bar holds scales
  inside it. At 200% nothing is lost: panels wrap their rows, the status bar grows with its text and
  the map's window narrows (WCAG 1.4.4). With WCAG 1.4.12's spacing applied nothing is clipped: text
  cut short - the Log's two lines, a contested value, a candidate, a relation chip, a search snippet
  and the strip - shows whole on the row's focus or on a press, never only in a tooltip, and the
  status line gives up its counts rather than cutting them (Section 6).

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
- **A press lands.** On the viewer and the Settings and Setup pages a target a pointer presses is at
  least 28 by 28 pixels, a Mac's default, or its 28-pixel circle overlaps no other target's; where
  the pointer is a finger (`pointer: coarse`), 44, as iOS asks. A narrow window on a Mac is still
  pressed with a pointer, so the width alone does not decide it. The status bar takes its height from
  a token in rem, 28 pixels at the default size and 44 under a coarse pointer, and the rules that sit
  above it read the same token. The landing and the principles page, read in any browser, keep WCAG
  2.5.8's 24, and 28 below 760.
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
   search lights its answer, and a thin line runs from the answer back to the entries that say it -
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
would move with it drawn thin in `--accent`. Log ticks are colored as the viewer's Log colors a row: an agent's entry in
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

**Type and surface.** Headings in Fraunces, reading and labels in Pretendard (step 10; the system
sans until then), commands, identifiers and the uppercase eyebrows in IBM Plex Mono: Newsreader goes.
The navigation, captions, counts, chips and a row's who-line are labels, not identifiers; set in mono
they made 61% of the site's text a face kept for what is typed or copied (Section 1, step 22).
No text of the site's pages is set below its label size - a scene's canvas, which draws to its own
scale, is outside the rule - and running text runs at a line height of 1.5 or more. The landing's own six sizes, which Section 4's scale for the
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
A column's label is the principle's number at the label size, with tabular figures; where the
columns are too narrow for it, the labels give way to the chapters' names, as they do on a phone.
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
  the title bar. The title bar holds no navigation but the way back (Section 6), the review count
  already sat in the status bar, and an entry that stays when nothing waits keeps the page one press
  away.
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
- **Three text tiers, not four.** `--muted` and `--faint` measured 1.1:1 against each other, so the
  screen had three tiers whatever the tokens said, and both sat below what APCA allows for text anyone
  reads. Less important text is set smaller, lighter or later, in a color that still reads; dimming
  it asked the person to work harder at text that was put there to be read.
- **A Mac's body size.** A Mac sets body text and its regular controls' labels at 13 points. At 11
  and 12 pixels in dimmed colors the viewer read as a dense dashboard rather than a Mac app; 13 costs
  the left panel some width, and its rows wrap.
- **Corners from tokens, concentric.** Radii written where they were used drifted from the four this
  document named to ten. Six tokens and one rule - the inner radius is the outer less the space
  between - is the shape a Mac's own parts take, and a test can hold a stylesheet to it.
- **No glass.** Liquid Glass is the system's material for its own controls, and it changes with
  Reduce Transparency, Increase Contrast and Reduce Motion. A glass drawn in CSS changes with none of
  them, and WebKit does not report Reduce Transparency to a page. The frame stays opaque, as step 7
  made it.
- **No SF Symbols and no SF Pro.** Apple licenses the symbols for apps on its platforms and the font
  for mock-ups; neither may ship in a page a browser opens. Lucide and Pretendard stay.
- **Gold stays gold.** A Mac lets the person's accent color replace an app's. Here gold says
  selected, human confirmed, waiting for a person and focus (Section 4), so following the person's
  accent would change what the map says.
- **The brand gold does not darken for the light ground.** The light appearance first darkened the
  accent to read as text, which turned the mark brown. Text on the light ground is dark anyway, so gold
  need not be text there: it stays in the mark, the fills and the tints, the ink carries the words,
  and only a thin line - a ring, a focus outline - deepens, to the 3:1 a line needs. The author asked
  for this (2026-10-19).
- **The status bar keeps Review and the gear.** Apple's guidelines keep controls and important
  information away from a window's bottom. The title bar holds no navigation but the way back
  (Section 6), Review's count is where the person already looks, and the menu bar reaches both -
  Cmd+2 and Cmd+, - so they stay, each a full target.
- **A table beside the map, not a description of it.** A long description says what the map showed
  when it was written; a table of the same entities stays true as the map changes, and names each type
  in words, which sixteen colors cannot.
- **A tooltip names.** It shows only to a pointer that rests, so what it alone says is said to no one
  on a keyboard or a phone.
- **The system's appearance, not ours.** A dark-only tool is Apple's exception, not its rule, and an
  appearance switch inside the app is one the guidelines ask an app not to offer. The tools follow the
  Mac; the site stays dark because it presents the product rather than operating it, and its pictures
  are of the dark tools.
- **Thin solid lines, not dashes.** The author asked for it: a dash breaks a line into marks that read
  as noise at 13 pixels on a dark ground, and dashes marching along an edge said less than an
  arrowhead does. Trust's rings, which a dash told apart, are told by weight: the more support, the
  heavier the ring, and the human-confirmed ring keeps its gold.
- **The title bar is a Mac toolbar.** The first title bar named the product in mono, centred the
  search and put both toggles at its right, and the app's pages showed a crumb and closed with an x;
  that was the author's choice before Apple's guidelines were read beside the screens. The guidelines
  put the sidebar's toggle and the title at the leading end and search at the trailing end, and title
  a window by what it shows; the author chose them on 2026-10-19. The way back is the one navigation
  the bar holds, since a page that is not the Graph needs a way out where the x was.
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
| **U7** | The viewer, the settings page and the landing give every token they share one value and each declares every custom property it reads, in any of its rules, and every text token meets 4.5:1 and APCA Lc 60 on every surface token, and `--ink2` Lc 75; a signal drawn as text takes its `-ink` token and gold `--gold-ink`, the brand gold `--accent` in both appearances with the mark drawn in it, and text on a pill's tint meets the same floor over it. |
| **U8** | A status pill is absent only for a known zero or a feature this node does not have set up; unavailable, refused and unknown are drawn as states (P5, P24). |
| **U9** | Areas are told apart by surface: the viewer, the Settings page and the landing draw no border, outline or ring-shaped shadow except where the line carries a fact - keyboard focus, a legend swatch of a line the map draws, a superseded relation, a contested value, an owed clause or the chosen column on the principles ledger, a control's own glyph, or under Increase Contrast a control's edge. |
| **U10** | The landing's stage draws the viewer's marks with the viewer's meanings: an entity's fill is its type's color by the viewer's own functions, a ring is human confirmed at the viewer's width, the coral badge is contested, a relation is the viewer's neutral line, and every other color it draws is a landing token. The second machine draws every ring and badge the first draws, since it replays the same log. Each scene ends within five seconds and then draws nothing until it is asked again; under reduced motion each scene is its resting frame; the canvas is hidden from assistive technology and each step says its scene in its own words (P2, P6, P18). |
| **U11** | Every face a surface names first in its reading or identifier stack ships with that surface and is served from its own origin, and no surface fetches a font from another host. The viewer, the Settings page and the landing name the same stacks. |
| **U12** | The principles page states only what the code states: its ledger, counts and clause lists are written from the coverage registry and the build fails when they differ; every principle of principles.md has its block and every Appendix A row its entry; every case names held clauses and is replayed by a test of the code that owns it or held by those clauses' tests; and the page's script takes every value it draws as the product's behavior from the cases and names no trust tier itself (P1, P5, P16). |
| **U13** | The principles page never draws an owed clause as working: every owed beat or branch names, in the registry's words, a clause the registry does not hold and is drawn in the owed style, every tension's owed tag is derived from the clauses it names, and the build fails otherwise (P5). |
| **U14** | The principles page draws the viewer's marks with the viewer's meanings and every other mark as its own, every color a landing token; each demo and scene plays at most once on its own, can be stopped while it plays, and rests drawing nothing; under reduced motion each beat and demo state is its resting frame; every canvas is hidden from assistive technology, a caption is announced only after the reader acts, and without a script the page draws no empty frame (P2, P6, P18). |
| **U15** | The viewer's search says how each hit was reached and over what scope: hits that contain the query's words are grouped apart from hits only the semantic surface or the graph's neighbors reached, each related row saying which, each group in the engine's order; the field names the scope; no row is chosen before the person moves to one; and a scope where nothing contains the words says so and offers the wider scope with its count, or "20+" at the limit (P5, P19). |
| **U16** | A type's color is never a signal's: every color of the band stands at least 25 degrees of OKLCH hue from the accent, human-confirmed, ok and bad tokens and has OKLCH lightness 0.62 or more and 5.5:1 or more on `--surface`; the band's first tone stands 0.09 apart in OKLab, 0.11 from every signal and 0.10 from every neutral the map draws, and its second 0.06 from every color of the band and 0.08 from every signal; a view gives up to eight types eight hues; a relation kind draws no color; and every copy of the band is the viewer's (P6, P18). |
| **U17** | No line a person reads first spells a system identifier: a tier is shown by its legend name; every proposal kind the engine opens and every event it carries has a sentence in the Log, and a hash id is left out of it; each observation row has a who-line naming tier, who and when, and its mark is the map's ring for its tier; and a place keeps one name wherever it is reached (P2). |
| **U18** | Settling a contested value says why the shown value is shown, offers every value the same act at the same weight, says what the act does before it is cast - both halves when it demotes the shown value - and is undone by the reverse gated events, which the log keeps; every act is filed in the workspace of the observation it changes, and the console asserts no value of its own (P3, P6, P23). |
| **U19** | Every control on the viewer, the Settings and Setup pages, the landing and the principles page has a name, is the kind its act is and exposes its state; a dialog holds focus and makes the page behind it inert; a key that decides acts only while focus is in its list or item, and the view's single keys can be turned off; every target a pointer presses is at least 28 pixels with its spacing on the viewer and the app's pages, 44 under a coarse pointer, and 24, 28 below 760 pixels wide, on the landing and the principles page; and nothing but a focusable code block scrolls sideways at 320 pixels. |
| **U20** | On the local console the status bar always holds the way to Review, its count the items under Needs you, which lists only beliefs that tie on trust, with the contested ones counted in it; and follow never changes the scope, the selection or the open sheet - it frames activity in the scope on screen and names a write elsewhere in a notice - while a selected entity a merge folds is followed to what it became, and one that leaves the scope closes its sheet with a line that says so (P22, P24). |
| **U21** | Every font size the viewer and the Settings and Setup pages draw is one of Section 4's six, through a size token in rem; running text and every control's label are 13 pixels or more, and nothing is under 11; the map's labels scale with the root's size; the app's View menu sets the root's size from 100% to 200% and keeps it, while the title bar keeps its height and its traffic lights' place; and no text is cut short except where focus or a press shows it whole. |
| **U22** | Every corner the viewer's, the app pages' (`settings.css`, `welcome.css`, `shell.css`) and the landing's stylesheets draw is one of Section 3's radius tokens, none, or a dot's circle; a part inside a rounded part takes the outer radius less the space between them; a pill and a large control are capsules, and the controls of one row share one size; a checkbox, a radio and a switch take the accent; and no line is dashed or dotted, on a page or on a canvas. |
| **U23** | Under `prefers-contrast: more` every text token takes the next stronger one, every control, chip and field draws an edge of 3:1 on its surface, and the focus ring is 3 pixels; under `forced-colors: active` the map redraws in the system's colors, keeps every mark it draws, and widens the human-confirmed ring. |
| **U24** | The map's window offers List: a table of the entities the map draws, in its scope, filters and search, sorted by type and name, each row naming the entity, its type, its trust by the legend's name, its count of relations and, when the scope holds more than one, its workspace, and selecting its node as the map does; and the canvas's name says how many entities, relations and regions it draws and that List holds them. |
| **U26** | The title bar names the place on screen, never the product: on the Graph page its leading end holds the filters toggle and the scope's name and its trailing end the search and the right panel's toggle; on every other page its leading end holds the way back to the Graph, which Esc and Cmd+[ also take; it holds no other navigation; every page's bar is one bar, the star first and the way back and the title where the Review page has them; and in the app it drags the window from anywhere but its controls, and clears the traffic lights only while they are shown. |
| **U27** | The viewer, the Settings page and Setup follow the system's appearance and offer none of their own: every color token they declare has a light value that meets U7's floors on the light surfaces; the light band keeps every type's slot and meets U16's rules on the light ground at OKLCH lightness 0.42 to 0.67 and 3:1 on `--panel`; no color those pages draw is outside their tokens but a listed few that read on both grounds; and the map redraws its palette, band, regions and halos when the appearance changes. |
| **U28** | Drawing never moves the layout: a pan, a zoom, a hover, a highlight or a selection redraws the map without a simulation step, and a layout that has come to rest visits no pair of nodes until something warms it. |
| **U29** | The layout's forces come from grids, never from every pair: collision, crowding and repulsion pair by pair within a node's own and the adjacent cells, the repulsion of the cells beyond, out to its range, from each cell as one body, and the push between regions only between regions in reach of each other; so a step costs in proportion to the nodes, the cells around them and the regions near each other. |
| **U30** | What the map draws follows what is on the screen: a node or relation outside the window is not drawn; a node's detail follows its radius on the screen continuously - its halo growing in from 1.2 pixels to 3, its trust ring, the arrowheads into it and the bend of its lanes from 2 to 4, a relation by its length from 1 to 3, every fade eased in 32 steps - unless it is hovered, selected or active; relations and nodes of one look are drawn as one path; and names are placed by a per-cell budget. |
| **U31** | At 10,000 entities and 15,000 relations the map pans and zooms within a 16.7 ms frame at the 95th percentile, and shows its first picture within 10 seconds, on the reference Mac in Chrome and in WebKit, as the scale bench measures it. |
| **U32** | Layers that do not change when only the camera moves are not worked out again for it: the region field is kept with the camera it was worked out for and drawn moved and scaled by the camera's change, and worked out again once the camera rests, when the moved field would leave the window uncovered or be scaled past half or double, and when a region, its strength or the appearance changes; the minimap's miniature is a bitmap of its own, painted when the nodes, their positions, the filters or the look change, under the window's frame drawn each frame; and the regions' members and hulls are kept while the nodes stand still. |
| **U33** | A poll that finds nothing new costs the page nothing: the graph, the hypergraph and the Log's observations answer with an ETag of their body, the viewer asks with it, an unchanged one answers 304 with no body, and the viewer keeps what it has without parsing or applying anything. |
| **U34** | The layout's region forces grow with the regions' members, not with their pairs or their number: membership is kept as index arrays while the graph and its contexts are unchanged, a step allocates nothing per region, the shared-member test marks one region's members and reads the other's, and only regions of three or more members - the ones the map draws - push apart, while every context of two or more keeps its pull. |
| **U35** | The layout steps on a thread of its own when a worker can start, and the page draws its frames without waiting on a step: the worker takes the graph and its changes, steps as fast as it can while the map is hidden behind the loader and at most once every 16.7 ms after, and hands back positions; when no worker can start, the same file steps on the page's thread as before; and while the layout moves, the minimap's miniature is painted at most four times a second, its dots drawn into a pixel buffer. |
| **U36** | A fresh layout starts from its communities: label propagation over the relations finds them, the graph of communities is laid out first, its positions are carried to the members, and the full layout continues from there; a saved layout skips it. |
| **U37** | A frame touches what the window holds, not every node: a grid of the node positions, rebuilt when they move, answers which nodes are drawn, which can be named or badged, which pulse and which is under the pointer, and the relations come from a grid of their midpoints; and the Log lists the scope's newest 200 observations, asking for one more to know there are more, wires its rows by their place, and shows more on request. |
| **U38** | Zoomed out, the map keeps its shape by keeping its best-connected nodes where they are: it keeps whole about one node for each 12 by 12 pixels of the screen it covers, ranked by their relations, and the next as many fade out, the least-connected first; a node faded out is not drawn, named or under the pointer, and a frame that keeps fewer nodes than its window holds visits only those and their relations; a node kept is drawn at least 1.6 pixels in radius; no node is drawn for others; a hovered, selected, active, pulsing or contested node never fades; and the minimap keeps nodes by the same rule at its own scale, one for each 5 by 5 of its pixels, with only its 48 widest regions of at least 12 pixels. |
| **U39** | Where WebGL2 renders to half-float targets, the GPU draws the region field, the workspace outlines, the relations and their arrowheads and the nodes in their three sizes, and a 2D canvas above draws the names, badges, pulses, glows and peers; otherwise, under forced colors, or when the context is lost, the 2D canvas draws all of it as it does now. |
| **U40** | At 10,000 entities shaped like a store, the first picture comes within 3 seconds and a frame within 16.7 ms at the 95th percentile at rest, at any zoom and while the layout moves; at 50,000, within 10 seconds and 16.7 ms on WebGL; on the reference Mac in Chrome and WebKit, as the scale bench measures it. |
| **U41** | Every icon the viewer, the app's pages and the landing draw is a Lucide 1.52.0 file, vendored unmodified and drawn as a mask over the text's color, the contested badge's mark included; no page draws an icon from a character, a path written by hand or a pasted copy of one, and a file both surfaces ship is the same bytes on both (Section 28). |
| **U42** | A notice has one anatomy on every surface - its kind's icon, a title, at most a detail and one action, and a 28-pixel dismiss showing x; a success without an action leaves 4.5 seconds after it shows, counted only while the pointer is off it and focus outside it, and every other notice stays until dismissed or used; four at most; it enters over 200 ms and leaves over 120 ms, and under reduced motion does neither (Section 28, WCAG 2.2.1). |
| **U25** | A tooltip holds a name and its key and nothing a person needs to act: every tooltip the viewer and the app's pages write goes through one helper and is at most five words before its key, an interpolated name counting as one, and what a control does is said on the screen or behind a control that a keyboard and a finger reach. |

U7's third-pass half is guarded in `crates/supragnosis-viz/tests/design_tokens.rs`:
`every_text_token_reads_on_every_surface_by_apca` measures APCA 0.0.98G over the three `:root` blocks,
`text_on_a_pill_reads_over_its_tint` lays each tint on the two panels, `a_signal_drawn_as_text_takes_its_ink`
reads every color property of the four stylesheets, and `faint_is_retired` keeps `--faint` out. U23 is
guarded by `increase_contrast_and_forced_colors_are_heard` in the same file: each stylesheet's Increase
Contrast block, and the map's palette under forced colors.

U21 is guarded by `crates/supragnosis-viz/tests/type_scale.rs`: `every_font_size_is_a_token_in_rem`
reads every font declaration of the viewer's, the Settings page's and Setup's stylesheets,
`text_and_control_labels_are_body_size` holds the roles Section 4 gives at 13, and
`the_maps_labels_scale_with_the_root` and `cut_text_can_be_read_whole` read the map's label sizes and
every clamp; `view_sets_the_root_text_size` in the app's tests holds View's three items, the kept size
and the page load that gives it to each page. U22's line half is guarded by `lines_are_thin_and_solid` in
`design_tokens.rs`, which reads the four stylesheets and the three scripts that draw for a dash, and
the viewer's rings and their swatches for their weights. U24 is guarded by `the_map_has_a_list` in
`crates/supragnosis-viz/tests/list.rs`: List's place in the camera toolbar, its rows' filter and
order, its columns, a row selecting its node, Esc, the filters it follows and the canvas's name. U26
is guarded by `crates/supragnosis-viz/tests/titlebar.rs`: the viewer's bar in order - filters toggle,
way back, title, search, right panel's toggle - with no product name, the Review page's heading moved
into it and Cmd+[, and the app pages' "< Graph", their own titles and Cmd+[.

The rest of U22 is guarded by `crates/supragnosis-viz/tests/shapes.rs`: `every_corner_is_a_radius_token`
reads every radius of the viewer's, the app pages' and the landing's stylesheets, and
`large_acts_are_capsules_and_their_rows_share_a_size` the Review page's and Setup's acts and the pills;
`targets_reach_28_and_44_under_a_finger` holds the stacked rows and segments at 28 and the coarse
pointer's 44, the part of U19's targets a stylesheet can hold, and browser scenarios measure the rest.

U25 is guarded by `crates/supragnosis-viz/tests/tooltips.rs`: `a_tooltip_names_and_explains_nothing`
reads every tooltip of the viewer's and the app pages' markup and every one the viewer's script writes,
through `tipText`, and `view_and_search_say_what_they_do_on_the_screen` holds a line under each of
View's options, read with it, and the search's words for what it searched.

U27 is guarded by `the_light_appearance_meets_the_same_floors` and `no_color_is_drawn_outside_the_tokens`
in `design_tokens.rs`, which read each light `:root` - every dark color token redefined, the floors on
the light surfaces and tints, Increase Contrast in the light - and every color the tools' stylesheets
draw outside their tokens, and by `the_light_band_is_never_a_signal` in `type_band.rs`, which measures
the light band by U16's rules on the light ground and reads the map's light palette and its redraw.

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
`ProposalEventKind` and every decision `review_proposal` accepts. Its second half is guarded by
`every_log_row_says_who_and_when` in the same file - the tier by name, the writer and the time once a
minute, marked with the legend's ring - and by `the_setup_page_says_what_is_true_and_how_far_it_is` in
the app's tests, which holds Setup's one name in its crumb, the tray and About.

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
13. **The defects** [built]: the seventeen found in Section 1's pass that ask no design question,
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
15. **The app's pages** [built] (Section 6a): Setup's name, its plan and its sentences; the
   Settings page's one card per fact, its radio group, its peers, and the capture switch with
   `connect claude-code --no-capture` removing the hook. It ships with the Setup page itself
   (onboarding.md Section 13 step 3).
16. **The site** [built] (Sections 11, 11a): the landing's next step and Install link, the
   principles page's glossary, its ledger by chapter on phones and its scenes played by a press.
   Merging it publishes the site, so it merges on the author's word, right before the release that
   carries step 15.
17. **The third pass's defects** [built]: the Trust legend's off entries drawn at full strength
   and saying they are off, the Review page's key caps in `--ink2`, the eight short targets and the
   phone's Review tabs and acts brought to the floor before step 19 (24, 28 below 760), Setup's
   checkboxes in the accent, the search field named with its scope, the status line's short form on
   a phone, the privacy line held to 34em, and the app's starting splash on the tokens and the
   reading stack, which falls to the system's face there, since nothing serves the fonts yet.
18. **Reading** [built] (Sections 3, 4, 10; U7, U21, U23): `--ink2` and `--muted` raised and
   `--faint` retired into `--muted` in all three stylesheets and the scripts, the marks that move
   with them - the unverified ring with its swatch, the active edges, the owed outline, and the
   stage's and the scenes' palettes - the signals' text tokens
   and the pills' tints as tokens, measured; Increase Contrast and forced colors, with U9's guard
   admitting the edges Increase Contrast draws; the six sizes as rem tokens given by role, the map's
   labels scaled with the root, the app's root-size menu items, and every clamp and ellipsis Section
   10 names opened on focus or a press; and U7's test measuring APCA.
19. **Shapes, lines and targets** [built] (Sections 3, 4, 5, 10; U19, U22): the radius tokens and
   the concentric rule in the viewer's, the app pages' and the landing's stylesheets, capsules for
   pills and large controls, one size per row, the accent on every checkbox, radio and switch; every
   dashed line drawn thin and solid - trust's rings by weight with their swatches and the Log's marks,
   the boundaries and their swatches, superseded relations, the contested box, active edges with an
   arrowhead and the merge preview - on the viewer, the stage and the principles page; and every
   target on the viewer and the app's pages at 28, 44 under a coarse pointer.
20. **Said on the screen** [built] (Section 4; U25): View's options and the search mode say what they do in a
   line under their names, and every tooltip is written through one helper with a name and its key.
21. **The map's List** [built] (Section 6; U24): the table, its selection kept with the map's, and the
   canvas's name.
22. **The site's type** [built] (Sections 11, 11a): labels in Pretendard, nothing under the label
   size, the ledger's labels as numbers, running text at 1.5, and the landing's picture of the viewer
   retaken once steps 18-21 have changed how it looks.
23. **The title bar** [built] (Sections 6, 6a; U26): on the Graph page the filters toggle and the
   scope's title at the leading end and the search and the right panel's toggle at the trailing end;
   "< Graph" on the Review, Settings and Setup pages with Esc and Cmd+[; the product's name, the
   crumbs and the x gone; the star leading on every page, with the way back and the title where the
   Review page has them; and settings-page.md's and onboarding.md's title bars with it.
24. **Light and dark** [built] (Sections 3, 5, 10; U27): the light values in the viewer's and the
   Settings page's stylesheets, shadows and scrims as tokens, Increase Contrast in both, the light
   band and palette in the viewer's script with the map's redraw on a change, and the starting splash
   in both; every step from here is judged in both appearances, with screenshots of each.

25. **The scale bench** [built] (Section 26.1): `scenarios/web/bench/canvas.mjs`, the synthetic graph
   and the timings of Section 1's table, run before and after every step below and in both engines.
26. **The simulation at rest** [built] (Section 26.3; U28): the simulation on its own clock, no step
   in a frame that only redraws, the collision pass ended with the settle, and the recenter only while
   it steps.
27. **The grid** [built] (Section 26.4; U29): near forces pair by pair from a uniform grid, the far
   field per cell, region pushes from a grid of their reach, and scratch arrays and the node lookup
   kept between steps.
28. **Drawing less** [built] (Section 26.5; U30): culling, one path per look, the three sizes of a
   node, the label grid, and the zoom floor that follows the graph.
29. **Still layers** [built] (Section 26.6; U32): the region field kept with its camera and moved as a
   bitmap while only the camera moves, and the minimap's miniature painted only when it changes.
30. **Measured** [built] (Section 26.2; U31): the bench from 500 to 20,000 in Chrome and WebKit, at
   rest, at a reading zoom and while the layout moves. The budget holds in both; what it leaves -
   the moving layout and a store's many contexts - is the next pass's.
31. **The poll asks first** [built] (Section 27.3; U33): an ETag on the graph and the hypergraph,
   and a 304 for an unchanged one, through the desktop app's proxy as well.
32. **Region forces that scale** [built] (Section 27.4; U34): kept membership, marking, and
   separation among drawn regions; the bench's graph gains a store's contexts.
33. **The layout on its own thread** [built] (Section 27.5; U35): `viewer-sim.js` on typed arrays,
   in a worker, and on the page's thread when none starts.
34. **The multilevel start** [built] (Section 27.6; U36): communities, their layout first.
35. **Clipping** [built] (Section 27.7; U37): the grid index of positions and of relations' midpoints.
36. **Semantic zoom** [built] (Section 27.8; U38): the far map keeps its best-connected nodes where
   they are, the rest fading by rank and clipped, and the minimap keeps its nodes the same way.
37. **WebGL** [built] (Section 27.9; U39): the GPU renderer beside the 2D canvas.
38. **Measured** [built] (Section 27.2; U40): the bench to 50,000 in both browsers and both
   renderers, and the dev store through its daemon; with what it found - the multilevel start at the
   layout's density, a far frame over what it draws, and the minimap's paint out of the moving frames.
39. **Icons** (Section 28.1-28.3; U41): the viewer's files under `assets/icons/` and their path, the
   `.i` classes, every mark of Section 28.3's table, the contested badge on the map and the landing,
   and the test that reads both surfaces.
40. **Notices** (Section 28.4; U42): the three kinds, the surface, the clock that stops while it is
   read, the motion, in `viewer.js` and `kit.js` both.

Steps 18 and 19 change the landing's stylesheet and step 22 is the site's own, so like step 16 they
merge on the author's word, right before the release that carries steps 17-21 and 23.

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
- **settings-page.md Section 3.0** (step 18): the title bar keeps its height and its traffic lights'
  place while the root's size scales, and what it holds scales inside it. Section 6's View menu gains
  Make Text Bigger, Make Text Smaller and Actual Size.
- **settings-page.md Section 3.0 and onboarding.md Section 6** (step 23): the crumb and the x give way
  to "< Graph" and the page's title, and Esc and Cmd+[ go back.
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

## 22. What building the app's pages changed

- **The shell says what runs.** A step's progress lived only in the page, so leaving Setup lost it and
  pressing Set up again reported a download that was still running as failed. `setup_running` answers
  the steps the shell runs; Setup and Settings show them as running and wait for them.
- **The download runs outside the page's lock.** Settings ran it under the one-act-at-a-time lock that
  keeps two CLI writes from racing, which disabled the page for minutes; nothing the download writes
  races with the rest, so it runs on its own, with its progress and a Stop on its row.
- **`connect claude-code --no-capture` removes the hook on a bridged app.** It did nothing there before,
  so the hook could be added from Setup and never taken away; the switch needed the CLI to say both.
- **The tray keeps its rule.** The spec's first draft extended "Finish Setup..." to every step; the way
  back is named where it always is instead, in About.
- **Return presses Set up** unless focus is on a control Return already acts on, and the foot is drawn
  right to left so that Set up is both first in tab order and rightmost.
- **One row per peer** on a hub reads the checked-in list and the admitted list together; a peer that
  checked in but is not admitted says so.

## 23. What building the site changed

- **The ledger's two outlines join U9's list.** An owed clause's `--muted` outline and the chosen
  column's `--gold-bright` one are lines that carry a fact, listed in `no_border_draws_an_area` beside
  the Trust legend's swatches; every other column keeps a transparent edge of the same width, so
  choosing one does not move the rest.
- **A phone's ledger needed its old height gone.** The 560-pixel rule fixed the ledger at 136 pixels,
  which would have clipped the six chapter rows; the rows set their own height now.
- **Only the scenes wait for a press on a phone.** Section 11a's rule names scenes, and below 560
  pixels they say Play scene; the five demos still play their opening once as they come into view, each
  with its own control to play it again or change it.
- **A separator is drawn, not read.** The "/" between two shown clauses is its own span hidden from a
  screen reader, outside the clause spans the registry's tests read.

## 24. What the third pass's review changed

The third pass (steps 17-22) was reviewed once against the code before it was built. These findings
changed it:

1. **`--bad-ink` missed its own floor.** It measured Lc 59.9 on `--select`; it is `#fca18a` now. A
   pill's text sat on a tint no test measured, and `--ok-ink` read Lc 59.4 there, so the tints are
   tokens, measured, and `--ok-ink` and `--warn-ink` moved to `#78c8b8` and `#e9af60` (Section 3).
2. **APCA was quoted for what it does not say.** Lc 75 is APCA's body floor at 18 pixels, not at 13;
   Lc 60 and 75 are this document's floors, said as such (Sections 1, 3).
3. **List grouped by something undefined.** Regions overlap and can be off; the table is sorted by
   type and name, with the workspace when the scope holds more than one (Section 6, U24).
4. **Zooming the webview would move the traffic lights' bar.** The menu scales the root's size
   instead, which every rem size follows and the title bar does not (Section 10, U21, Section 15).
5. **A width stood for a finger.** 44 pixels apply under a coarse pointer, and the status bar's
   height is a token that can hold them (Section 10, U19).
6. **`accent-color` reaches native controls only.** The viewer's drawn switches and radios take the
   accent through their tracks (Section 4).
7. **The text tokens also draw marks.** The unverified ring and its swatch, the active edges, the
   owed outline and the two site palettes move with them, and step 18 names them (Section 3).
8. **U9's guard reads no media query**, so the edges Increase Contrast draws are listed in it by
   name, and U9 says so (step 18).
9. **Setup's stylesheet was left out of U22**, with the shell's; both are named.
10. **The `-ink` rule had no invariant.** U7 holds it now.
11. **Smaller corrections.** The Review queue's rows reached 44; every clamp and ellipsis is named,
    not only the Log's; a scene's canvas is outside the site's label floor; the ledger's labels give
    way where the columns are narrow; gold keeps its four meanings; the steps carry their state; and
    the owed rows name what will be seen when they are built.

## 25. What building the third pass changed

- **The tools' sizes are `--text-*`.** Section 4 first named them `--fs-*`, but the landing already
  names `--fs-body` and `--fs-small` for its own scale, at 18 and 15 pixels, and U7 holds a name two
  stylesheets share to one value.
- **The status bar's height moved into step 18.** At 200% its 28 fixed pixels cut the review pill in
  half, so `--bar` is a rem token now, every panel and the camera above the bar read it, and the map's
  framing reads the bar's height again when the root's size changes.
- **The tints were never a token in the viewer.** Its pills drew their signals at 13% from literals;
  they read `--ok-glass` and `--bad-glass` at 12%, the Settings page's values.
- **A mark could not keep the old `--muted`.** The unverified ring drew `#8e96a5` while its legend
  swatch read the token, so the ring is `--muted` too.
- **The forced-colors map covers its regions.** A region's name and a boundary drew from the type band;
  under forced colors both are `CanvasText`.
- **The Review page's tabs did not fit one row.** At 13 pixels the four tabs needed 333 pixels of a
  queue 288 wide, so the queue is 360 and a tab's padding 8; they wrap only when text grows.
- **A finger's 44 pixels override the controls' own heights.** A zero-specificity rule lost to every
  class that set a smaller height, so the coarse pointer's rule is marked important.
- **Measured in a browser.** Every target on the Graph and Review pages, View, Settings and Setup
  reaches 28 pixels with its spacing at 1440 and 1280, and 44 under a coarse pointer at 390, with the
  status bar grown to hold them; the gear is 28 square.
- **The helper is `tipText`.** `tip` already named the map's hover card in the viewer's script.
- **What a tooltip held moved onto the screen.** View's options and its two choices carry a line under
  their names, and View scrolls when they do not fit; the search says what it searched; the merge
  suggestions say once what Propose does; a similarity and a grouping rung say their numbers in words;
  and an edge's description is a line under its chips.
- **List keeps the person's place.** A poll redrew the table and dropped the focused row, so the
  table gives focus back to the same row when it redraws; while List is on, the minimap, which is the
  map's, is not drawn over it. Its rows are told apart by a step of tone, as U9 asks.
- **The search trails, so its list opens under it toward the left.** Its list was centred under a
  centred field; at the trailing end it would have run off the window, so it is anchored to the
  field's right edge. On the Review page the panel toggles go, since no panel is there.
- **The wordmark stays mono.** It is a logotype, the product's name drawn as its mark, not a label;
  every other label on the site is Pretendard now, and the counters and step numbers take tabular
  figures.
- **The ledger's numbers give way below 1000 pixels**, where a column grows narrower than "24" at the
  label size, and the chapters' names stay.
- **The landing's pictures were retaken.** The viewer's picture shows the third pass's title bar, type
  and rings, and the stage's poster its rings by weight and its thin lines.
- **The mark was swept into a rule for text.** Step 18 moved every gold drawn as text to the bright
  gold, and the mark, which is not text, went with it, as did the site's eyebrows and principle
  numbers; the light appearance then drew the mark brown. The mark is the brand gold again everywhere,
  and the site's small gold labels take `--accent-ink`.
- **The app's pages lost the star with their crumb.** Step 23 took the crumb out of the Settings and
  Setup title bars and the star went with it, so their "< Graph" stood 24 pixels and their title 25
  pixels left of where the Review page has them, though settings-page.md Section 3.0 asks for the
  viewer's mark on both pages. The app's pages take the viewer's leading group - the same star, its
  spacing and its title's margin - and a browser measures the star, the way back and the title at
  the same places on the Review, Settings and Setup pages in both appearances.
- **A highlight is a still picture.** Dashes marched along an active edge every frame, so the map
  redrew 60 times a second while a node was held; a solid edge with an arrowhead does not move, and
  the map stops drawing once nothing else does.
- **The rewiring relations of a merge preview are thin and fainter** than its fold arrow, which they
  were told from by their dashes.
- **Panel headings and pills follow their roles.** The Review page's heading wraps rather than cuts
  its link at 200%, and the Log's who-line wraps instead of being clipped.

## 26. The map at scale

The fourth pass (Section 1's last table). The map has to stay a map - movable, readable, and the same
picture of the same knowledge - as a workspace grows past the size its drawing and its layout were
written for.

### 26.1 The bench

`scenarios/web/bench/canvas.mjs` starts a daemon with the scenario harness, serves the viewer through
its proxy, and answers `/api/graph` and `/api/hypergraph` itself with a seeded synthetic graph:
communities of 20 to 60 entities in twelve types, about 1.3 relations per entity inside a community
and 0.2 across, a region for every twelve entities, and a context of two for every relation between
two of those groups, as a store filled through `observe` holds (Section 27.1). It answers as the
daemon does, with an ETag and a `304` to a poll that asks with it (Section 27.3); without, every poll
applied the same graph again. The viewer's script is not changed for it; its functions `draw` and
`nodeAt` are timed from outside, since the script is a classic one and they are its globals, and the
layout's steps are read from what the layout reports (Section 27.5). For each size it reports the time to the first picture, a settling step, the
frames of a two-second pan and of ten wheel steps - each split into simulation and drawing - and a hover's
hit test, as the 50th and 95th percentiles; with `BREAKDOWN=1` it also times, within a pan's and a
zoom's frames, the region field, the workspace outlines and the minimap. It takes the pan again at a
reading zoom (`NEAR_SCALE`, 0.6), where nodes are drawn full, and times the frames while the layout
moves: from the reveal to rest, and from a node held and dragged for two seconds to rest again. `SIZES`, `BROWSER` (chromium or webkit) and
`OUT` choose what it runs and where it writes; it is a measurement, not a test, and CI does not run it.

### 26.2 The budget

On the reference Mac - Apple silicon, at twice the pixels, 1440x900, in Chrome and in WebKit, which the
desktop app runs on - at 10,000 entities and about 15,000 relations:

- a frame of a pan or a zoom within 16.7 ms at the 95th percentile, with drawing about 10 ms of it;
- a hover's hit test within 1 ms;
- the first picture within 10 seconds of the data arriving.

At 20,000 a pan holds 30 frames a second. Past that, step 30's numbers decide whether the map moves to
WebGL - one instanced draw for the nodes, signed-distance discs and glyphs, picking by color - which
would be its own document. Nothing in this section assumes it.

**Measured** on 2026-10-19 after steps 26-29, by the bench on the reference Mac. Times are the 50th and
95th percentiles in milliseconds, a pan's and a zoom's taken at rest; "pan near" is the same pan at
the reading zoom 0.6, and the two moving columns are the frame rates while the layout steps, from the
reveal to rest and from a node dragged for two seconds to rest again. WebKit's clock reads whole
milliseconds.

In Chrome:

| Entities | Relations | First picture | At rest | Settling step | Pan | Pan near | Zoom | Moving, cooling | Moving, dragged |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 500 | 727 | 2.1 s | 3.9 s | 2 | 1.5 / 2.4 | 1.1 / 2.3 | 1.5 / 2.2 | 60 fps | 60.1 fps |
| 1,000 | 1,468 | 1.9 s | 3.8 s | 4 | 0.9 / 3.4 | 1.3 / 2.7 | 1 / 1.3 | 60 fps | 56.7 fps |
| 2,000 | 2,950 | 2 s | 5.1 s | 5.5 | 3.6 / 5.4 | 1.9 / 2.8 | 1.7 / 4.8 | 60 fps | 50.4 fps |
| 5,000 | 7,326 | 3 s | 7.9 s | 21.5 | 7.3 / 8.7 | 4.1 / 5.7 | 3.3 / 7.2 | 40.1 fps | 36.3 fps |
| 10,000 | 14,666 | 7.9 s | 17.4 s | 65.7 | 7.3 / 11.5 | 6.8 / 8.4 | 6.2 / 7.2 | 20.3 fps | 19.5 fps |
| 20,000 | 29,347 | 24.1 s | 41.9 s | 228.2 | 10 / 14.4 | 8 / 11.3 | 11.2 / 14.8 | 10.5 fps | 10.3 fps |

In WebKit:

| Entities | Relations | First picture | At rest | Settling step | Pan | Pan near | Zoom | Moving, cooling | Moving, dragged |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 500 | 727 | 2.1 s | 4 s | 2 | 3 / 5 | 3 / 5 | 2 / 3 | 60 fps | 56.7 fps |
| 1,000 | 1,468 | 2.1 s | 3.9 s | 4 | 4 / 6 | 3 / 6 | 3 / 4 | 59.9 fps | 53.2 fps |
| 2,000 | 2,950 | 2.1 s | 5.4 s | 6 | 7 / 8 | 4 / 6 | 4 / 5 | 58.1 fps | 41.9 fps |
| 5,000 | 7,326 | 3 s | 10.1 s | 20 | 4 / 5 | 3 / 4 | 6 / 7 | 26.3 fps | 21.8 fps |
| 10,000 | 14,666 | 7.1 s | 20.2 s | 56 | 7 / 12 | 5 / 6 | 13 / 15 | 14.8 fps | 14.3 fps |
| 20,000 | 29,347 | 19.9 s | 49.8 s | 162 | 15 / 18 | 7 / 9 | 27 / 30 | 6.5 fps | 6.9 fps |

- **The budget holds in both, at every zoom.** At 10,000 a pan's frame is 11.5 ms at the 95th
  percentile in Chrome and 12 in WebKit, at the reading zoom 8.4 and 6, a zoom's 7.2 and 15, a
  hover's hit test 0.1 ms, and the first picture comes in 7.9 and 7.1 seconds. Before the pass the
  same pan took 651 and 893 ms and the first picture 100 and 96 seconds (Section 1). At 20,000 a pan
  is within the 60-frame budget in both, at 14.4 and 18 ms.
- **While the layout moves, the step is the frame.** At 10,000 a frame that steps the layout takes 50
  to 70 ms, and the map moves at 20 frames a second in Chrome and 15 in WebKit; at 1,000 it keeps 60.
  The step, about 30 ms of it, runs on the page's own thread.
- **The bench's regions are not a store's.** It draws a region for every twelve entities. A store
  filled the way the bench's graph is, through `observe`, holds a co-occurrence context for every
  observation - 11,862 for the same 10,000 entities, 10,958 of them of two - and the layout's region
  forces, which take every context of two or more, did not reveal it in six minutes.
- **WebGL is not needed for this budget.** The 2D canvas meets it at 10,000 and 20,000 entities.

### 26.3 The simulation runs only while it moves

- **A frame that redraws does not step.** The simulation steps on its own clock: a frame steps it while
  it is warm (alpha at or above its floor), while a node is held, and while the settle's last collision
  pass is owed. A pan, a zoom, a hover, a legend highlight or a selection redraws the picture from the
  positions it has (U28).
- **At rest no pair is visited.** The collision correction that ran after cooling - to clear the last
  overlaps - ends with the settle: it runs until no pair overlaps or for a bounded number of steps, and
  stops. A drag, a new node or a changed filter warms the layout again, as `wake` does now.
- **A zoom does not find it moving.** The coast after the forces stop is measured on the screen, and
  it ends once it moves nothing by a twentieth of a pixel a frame; what is left of it is dropped then,
  so zooming into a map at rest magnifies no motion that would step the layout again.
- **The recenter runs while the layout steps.** The drift it corrects comes from the steps' clamps; a
  layout at rest does not drift.

Guarded by `drawing_never_moves_the_layout`.

### 26.4 The layout's forces come from grids

Repulsion acts within its range, collision within two radii and the padding, crowding within
`DENSITY_R`. Each step bins the nodes into a uniform grid by counting sort, and no loop visits every
pair (U29):

- **Near, pair by pair.** The cells are a sixth of the repulsion's range, never narrower than
  crowding's or collision's reach. A node meets the nodes of its own and the eight adjacent cells
  one by one - every collision and every crowd it has, and the repulsion of everything that near.
  While the layout only clears its last overlaps the cells are a collision's reach, so that pass
  costs less still.
- **Far, per cell.** Beyond those cells, out to the range, each cell pushes as one body at its nodes'
  centre, its count weighted as every pair would weight it: by the node's radius and the cell's mean
  radius, and by how many of its nodes are in another workspace. This is Barnes-Hut's approximation,
  the one the layout makes: from the same positions on 2,000 nodes, a step's velocities stood within
  0.21 of every pair's at the 95th percentile, of velocities near 60.
- **Regions, near each other.** Two regions push apart only within their radii and the padding, so
  their centres are binned into cells that wide, a region meets only those of its own and the
  adjacent cells, and the shared-member test runs only for a pair close enough to push.

The step's scratch arrays and the nodes' lookup by id are kept between steps. The lanes of parallel
relations are worked out from the drawn relations alone (Section 26.5).

Guarded by `near_forces_come_from_a_grid`.

### 26.5 Drawing does less

- **Only what is in the window.** A node is drawn when its disc, grown by its halo and its ring, meets
  the window; a relation when its bounding box does. A name, a badge or a pulse goes with its node.
- **One path per look.** Relations that share a color, an opacity and a width are one path and one
  stroke, and their arrowheads one path and one fill. Nodes that share a fill and an opacity are one
  path; a node's halo is a disc of the surface color under it, drawn the same way, rather than a
  stroke for each node. An arrowhead is a triangle left open, since the fill closes it: closed one by
  one in a path of thousands, the heads took Chrome 80 ms a frame at a reading zoom on 10,000
  entities.
- **A node's detail follows its size, without a step** (U30). By its radius on the screen:
  - its halo grows from nothing at 1.2 pixels to its full width at 3, and under it the fill takes the
    node's whole disc, so a small node is its color - a halo at full width on a node of 2 pixels was
    most of it, and a map zoomed out read as dark specks;
  - its trust ring fades in from 2 pixels to 4, and over the same sizes the arrowheads into it grow to
    their length and the lanes of parallel relations into it bend open from straight;
  - on the 2D canvas, under 1 pixel, it is a square of its type's color, which is cheaper to fill; the
    GPU draws it as a disc at any size.

  A relation fades in by its length on the screen, from 1 pixel to 3. Every fade is eased and taken
  in 32 steps, so a zoom shows detail arrive and leave continuously while a frame still draws at most
  32 opacities of a look. A hovered, selected or active node and its relations are drawn in full
  whatever their size, so a highlight reads. A visible contested node keeps its badge at any size, in
  its own pass (U3). Drawn in three sizes, a zoom had shown each switch: halos appearing at once,
  rings and arrowheads popping in, lanes snapping from straight to bent.
- **Names by a budget.** The hubs among the nodes in view are named, as now, the cut set by how many
  are in view. A name is checked against the names in the 100-pixel cells it covers only, and each
  cell anchors two names at most; the hovered and selected nodes are always named.
- **The zoom floor follows the graph.** Zooming out stops where the whole map fits the window, so a
  map of any size can be seen whole.

Guarded by `what_the_map_draws_follows_the_screen`.

### 26.6 Layers that do not move are not redrawn

- **The region field moves with the camera.** The field is kept with the camera it was worked out
  for. While the regions, their strengths and the appearance are unchanged and only the camera moves,
  the kept image is drawn moved and scaled by the camera's change. It is worked out again 150 ms
  after the camera rests; at once when the moved image would leave part of the window uncovered or
  be scaled past half or double; and on any frame where a region, its strength or the appearance
  changed. While the layout is still, the field is worked out a quarter of the window's longer side
  beyond each edge, so a pan has field to move into; while the layout steps, every frame works it
  out anyway, and no margin is paid. What the zoom alone changes - a region's reach past its members,
  which holds the nodes' halos at a width on the screen, and the fade of a region wider than the
  window's share - is brought up to date when the field is worked out at rest.
- **The field's cell grows with its work.** A cloud costs a sum for every cell its box covers, so
  zoomed into a large map, where hundreds of clouds each span the window, 4-pixel cells came to tens
  of millions of sums. The cell is the smallest of 4, 8, 16 and 32 pixels that keeps the clouds'
  cells and the field's own within 3 million. At the fitted view of 10,000 entities it stays at 4
  pixels; a cloud wide enough to need a coarser cell has no detail it loses.
- **The minimap's miniature is a bitmap.** Its regions, relations and dots are painted into a bitmap
  of their own when the nodes - where they are, their degree and their type - the filters or the look
  change; each frame draws that bitmap and the window's frame over it. The key is what the miniature
  shows, not which arrays hold it, since a poll that brings the same graph brings new arrays.
- **The regions' geometry is kept.** Each region's members in the drawn types and its hull are kept
  while every node stands where it stood and the regions and the filters are the same. Only the
  hull's reach past its members, which holds the nodes' halos and so follows the zoom, is taken
  again each frame, without making anything new.

Guarded by `still_layers_move_with_the_camera`.

### 26.7 What this is not

It is not a new layout: the forces are the ones every pair gives, exactly near a node and within a
fraction of them beyond its neighbouring cells (Section 26.4). It does not hide relations while
the map moves, as some renderers do, since the picture should not change under the hand. It is not a
WebGL renderer (Section 26.2).

### 26.8 Sources

MDN, Optimizing canvas; web.dev, Improving HTML5 Canvas performance and Towards an animation
smoothness metric; Cytoscape.js's performance notes and its canvas renderer's texture caches; sigma.js's
label grid and settings; ECharts' large symbol drawing; d3-force's many-body and d3-quadtree; graphology's
ForceAtlas2 and Jacomy and others, PLOS ONE 2014; Barnes and Hut, Nature 1986; Zhao and others, 2025,
on web graph libraries; Horak and others, 2018; Game Programming Patterns, Spatial Partition and Dirty
Flag; Gaffer on Games, Fix your timestep.

### 26.9 What building it changed

- **The sizes of a node are 3 and 1.2 pixels, not 6 and 2.** A node with no relations is 4 world
  pixels in radius, so at 6 a map of ordinary size, framed at its usual zoom, lost its rings and its
  arrowheads. At 3 the maps people read today look as they did, and only a map zoomed out to hold
  thousands of nodes draws less.
- **The contested badge is not thinned.** U3 keeps it at full strength through every other dimming;
  a size rule would have been one more way to mute it.
- **Names keep the hub cut.** The per-cell budget alone named more nodes than before on a map of
  ordinary size; the cut by the nodes in view stays, and the grid and its budget bound what it lets
  through.
- **ESLint knows `Path2D`.** One path per look is a `Path2D` per look, and the viewer's lint names
  every browser API it may use.
- **The far field was taken.** With the grid alone a step on 5,000 nodes stayed at 151 ms: on a large
  map the repulsion reaches 960 world pixels and a node has hundreds of neighbours inside it. With
  the far field per cell it is 36 ms. Profiled at 10,000, the push between 838 regions then took 116
  of a step's 163 ms - 350,000 pairs, each tested for a shared member before its distance - and the
  regions' own grid, testing the distance first, brought the step to 64 ms, and cells of a sixth of
  the range to 48. In the bench the first picture of 10,000 came in 7.6 seconds, where the grid
  alone took 56, and of 20,000 in 22.
- **The field is keyed on what the regions are.** A region's reach past its members holds the nodes'
  halos, which keep a width on the screen, so every frame of a zoom changed every region's ellipse,
  and a key of the ellipses worked the field out on most of them: 22 ms of a zoom's 32 at 10,000.
  Keyed on each region's centre, axis and members' reach instead, a zoom moves the kept field like a
  pan does.
- **The rest of a frame was found by profiling.** The label pass read the window's insets three
  times per node, and every region was named and coloured each frame though only the active node's
  are shown; the insets are read once a frame and a region is named only when it is shown, which
  took a pan's frame at 20,000 from 24.2 ms at the median to 15.3. With the still layers, a pan's
  frame at 10,000 went from 28.5 ms at the median to 8.8 and a zoom's from 38.4 to 6.2. The map at
  rest is drawn as before, pixel for pixel, but for the minimap's dot edges, a few levels apart
  where its bitmap is composited.
- **The regions' geometry is kept, and the minimap is keyed on what it shows.** Measured in WebKit,
  a zoom's slowest frames at 10,000 were 15 to 20 ms with the regions shown and 10 to 14 without them:
  every frame worked out every region's members and hull again, thousands of small objects a frame,
  and the collector's pauses landed in the zoom. The same measure found the minimap painted again
  mid-zoom, 6 to 8 ms, when the poll brought the same graph in new arrays.
- **The field's cell grows with its work.** The bench measured the pan and the zoom at the fitted
  view, where the field had been measured too. Zoomed in on 10,000 entities, from 0.3 to 2.4, working
  the field out took 130 to 450 ms in Chrome and 120 to 570 in WebKit: once when the camera rested,
  and on every frame while the layout moved, up to a second in WebKit after a node was let go. With
  the cell growing it is 7 to 16 ms in Chrome and 6 to 35 in WebKit at every zoom, but for one reading
  of 107 ms in WebKit, which compiles the same loop differently by the order it is called in.

## 27. The map at any size

The author asked, on 2026-10-20, for every change the map needs to perform at its best - WebGL, a
level of detail that folds the map into its hubs by zoom or fades what it hides, and clipping - after
a store of 10,000 entities, opened in the desktop app, took minutes to show. This section is the
fifth pass. Section 26 stands: what follows builds on its grids, its sizes and its still layers.

### 27.1 What the dev store found

The bench's graph (Section 26.1) is drawn in the browser. A dev daemon was filled with the same
10,000 entities through `observe`, as an AI app would fill it - a group of about twelve per
observation and each relation between groups its own - and opened in the desktop app:

- **A store holds a context for every observation.** 12,166 observations gave 11,862 contexts, 10,958
  of them of two entities; the bench's graph had 838 regions, all of three or more.
- **The map did not appear.** In WebKit, through the daemon, no first picture came in six minutes.
  Profiled in Chrome for 45 seconds, the layout's step took 25 seconds and the regions' shared-member
  test 11.8 more; drawing took 0.6. The region forces take every context of two or more, and each
  step built every context's members and a set of them again.
- **Every poll fetched everything.** Every 2.5 seconds the viewer fetched the graph (5.8 MB) and the
  hypergraph (6.5 MB), parsed both and applied them, whether or not anything had changed; with the Log
  open it fetched every observation too (8.8 MB) and drew all 12,166, finding each row again in the
  whole list to wire it - 3.6 of the five seconds the data took to arrive.

### 27.2 The budget

On the reference Mac, in Chrome and in WebKit, on a graph shaped like a store - a context for every
observation, as Section 27.1 found:

- at 10,000 entities, the first picture within 3 seconds of the page asking, and a frame within
  16.7 ms at the 95th percentile at rest, at any zoom, and while the layout moves;
- at 50,000, the first picture within 10 seconds and a frame within 16.7 ms on WebGL;
- a poll that finds nothing new, under a millisecond of the page's time.

**Measured** on 2026-10-20 at the end of the pass, by the bench on the reference Mac, on its graph
shaped like a store (Section 26.1). Times are the 50th and 95th percentiles in milliseconds; the two
moving columns are the frame rate and the 95th percentile of a frame while the layout steps, from the
reveal to rest and from a node held for two seconds, let go 30 pixels from where it was pressed, to
rest again or for 30 seconds; "GPU" is the renderer of Section 27.9 and
"2D" the canvas alone (`?renderer=2d`). The node is held at the fitted view, and at 50,000 the
dragged layout was followed for the 30 seconds. WebKit's clock reads whole milliseconds.

In Chrome:

| Entities | Renderer | First picture | At rest | Pan | Pan near | Zoom | Moving, cooling | Moving, dragged |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | GPU | 2.2 s | 8.1 s | 2.9 / 4.7 | 2.5 / 3.1 | 5.5 / 9.7 | 60 fps, 6.1 | 60 fps, 8.7 |
| 10,000 | 2D | 2.1 s | 7.8 s | 3.4 / 5.7 | 2.4 / 3 | 6.5 / 10.1 | 60 fps, 13.4 | 44.9 fps, 31.2 |
| 20,000 | GPU | 3.8 s | 13.5 s | 2.9 / 5.4 | 4.1 / 5.1 | 6.2 / 10.5 | 60 fps, 8.5 | 60 fps, 11.9 |
| 20,000 | 2D | 3.8 s | 13.6 s | 3.7 / 4.8 | 4.1 / 5.2 | 7.2 / 13.9 | 59.1 fps, 20 | 57.6 fps, 28.4 |
| 50,000 | GPU | 8.8 s | 32.3 s | 2.7 / 3.3 | 5.2 / 8.1 | 4.3 / 5.6 | 60 fps, 17.8 | 60 fps, 19.4 |
| 50,000 | 2D | 8.9 s | 31.7 s | 4.5 / 11.3 | 7 / 12.8 | 4.9 / 15.1 | 56.2 fps, 30.1 | 56.6 fps, 32.7 |

In WebKit:

| Entities | Renderer | First picture | At rest | Pan | Pan near | Zoom | Moving, cooling | Moving, dragged |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 10,000 | GPU | 2.1 s | 6.9 s | 5 / 7 | 6 / 8 | 8 / 10 | 60 fps, 7 | 60 fps, 10 |
| 10,000 | 2D | 2 s | 6.9 s | 5 / 8 | 6 / 7 | 7 / 11 | 59.5 fps, 13 | 52.1 fps, 24 |
| 20,000 | GPU | 4 s | 14.2 s | 5 / 7 | 8 / 9 | 9 / 13 | 60 fps, 11 | 59.8 fps, 17 |
| 20,000 | 2D | 3.4 s | 12.3 s | 5 / 6 | 8 / 9 | 10 / 14 | 59.7 fps, 20 | 39 fps, 34 |
| 50,000 | GPU | 7.2 s | 31.1 s | 6 / 7 | 10 / 11 | 6 / 7 | 59.9 fps, 23 | 57 fps, 28 |
| 50,000 | 2D | 7.4 s | 36.7 s | 6 / 7 | 10 / 11 | 6 / 13 | 54.7 fps, 34 | 51.6 fps, 44 |

And the dev store of Section 27.1 - 10,000 entities, 14,672 relations and 11,862 contexts - through
its own daemon, on the GPU:

| Browser | Data | First picture | At rest | Pan | Zoom |
|---|---:|---:|---:|---:|---:|
| Chrome | 0.3 s | 2.2 s | 12.2 s | 3.6 / 5 | 6.1 / 12.3 |
| WebKit | 0.3 s | 2.0 s | 7.0 s | 4 / 6 | 8 / 11 |

- **At rest the budget holds at every size, in both browsers and both renderers.** At 10,000 the
  first picture comes in 2.2 seconds in Chrome and 2.1 in WebKit, and a frame at rest on the GPU is
  within 9.7 ms at the 95th percentile in Chrome and 10 in WebKit, at the fitted view, at a reading
  zoom and in a zoom; at 50,000 the first picture comes in 8.8 and 7.2 seconds and a frame at rest
  within 8.1 and 11 ms on the GPU. The dev store, which at the start of this pass did not show in six
  minutes, shows in about two seconds.
- **While the layout moves, the GPU keeps it at 10,000, and at 20,000 but for WebKit's drag.** A
  moving frame at 10,000 is 6.1 to 8.7 ms at the 95th percentile in Chrome and 7 to 10 in WebKit on
  the GPU; at 20,000, 8.5 to 11.9 and 11 to 17. The 2D canvas keeps a cooling layout at 10,000 - 13.4
  and 13 ms - but not a dragged one, 31.2 and 24, and in WebKit at 20,000 a dragged layout falls to 39
  frames a second.
- **At 50,000 a moving frame is not within it yet.** In Chrome a cooling frame is 17.8 ms at the 95th
  percentile and a dragged one 19.4, at 60 frames a second; in WebKit 23 and 28, at 57 to 60. At
  50,000 the budget holds at rest, and not yet while the layout settles.
- **Keeping by rank costs the fitted view more than folding did.** A far map now draws the nodes it
  keeps and their relations where it drew a few hubs (Section 27.8). At 10,000 a zoom at the fitted
  view went from 3.7 ms at the 95th percentile to 9.7 in Chrome and from 8 to 10 in WebKit, and the
  dev store's from 2.3 to 12.3 and from 3 to 11, within the budget; at 50,000, where the fitted view
  keeps 318 nodes whole, a pan's went from 7.7 to 3.3 in Chrome and from 14 to 7 in WebKit. One run
  each, against the measure taken before the map kept by rank.
- **The bench's runs fall into two modes.** On this Mac a run's frame times can sit in a fast or a slow
  mode - the same build's pan at a reading zoom measured 2.4 and 5.2 ms at the median - so a table is
  one run, and the steps were compared in turn, three runs each, by their medians.
- **What the measure changed.** It found three costs that were not drawing. The multilevel start
  packed members about twice as dense as the layout settles them, and at 50,000 the first picture
  took 16 seconds; it now starts them a little sparser than that (Section 27.6). A far map
  still passed over every node and relation to find the ones it had faded, 16 ms a frame at 50,000;
  it now passes over the nodes it keeps and their relations (Section 27.8). The minimap's miniature
  of 50,000, 81 ms to paint, cost the moving map a frame four times a second; a quick miniature while
  a large map moved took the moving frames' 95th percentile at 50,000 from 65 ms to 15 in Chrome, and
  the miniature that keeps nodes as the map does (Section 27.8) has since replaced it, painted whole
  in under 3 ms.

### 27.3 The poll asks first

The graph and the hypergraph answer with an `ETag`, a hash of their body. The viewer sends it back as
`If-None-Match`, and an unchanged answer is a `304` with no body, which the viewer takes as "keep what
you have" (U33). The daemon still builds the body to hash it: the engine's write counter counts
appended observations, not a review, a definition or a validity that ends with time, so a revision
taken from it could call a changed graph unchanged. Building is the daemon's cost, 0.1 seconds at
10,000; parsing and applying was the page's, and that is what the poll stops paying. The desktop
app's proxy passes the tag to the daemon and the ETag back, and passes a tag on only when it is one
line of printable characters, since it writes it into the request as it comes. The page keeps only
the tag of the graph it holds, with its address, and forgets it when the workspace changes.

The Log's observations carry the tag the same way, and the Log asks with it.

Guarded by `a_poll_that_finds_nothing_new_costs_nothing` and `a_poll_asks_with_what_it_holds`.

### 27.4 Region forces that scale

- **Membership is kept.** Each context's members are index arrays, kept while the nodes and the
  contexts are the same; a step works out the centres and spreads in typed arrays and allocates
  nothing per region (U34).
- **The shared-member test marks.** A region marks its members in an array as long as the nodes, and
  a neighbour shares one if any of its members carries the mark - no set is built or searched.
- **Only drawn regions push apart.** Separation is between regions of three or more members, the ones
  the map draws as clouds. A context of two keeps its pull, which is a spring between two entities
  seen together; pushed apart, ten thousand of them added a second repulsion to the first.
- **The bench's graph is a store's.** Its regions are each community's groups of twelve, and it gains
  a context of two for every relation between groups, as `observe` gives one: the dev store's graph,
  drawn in the same random order - 12,140 contexts for 10,000 entities, 904 of them of three or more.

On that graph the first picture of 10,000 entities came in 6.2 seconds in Chrome and 5.4 in WebKit,
and through the dev daemon in 10, where it had not come in six minutes.

Guarded by `region_forces_grow_with_the_members`.

### 27.5 The layout on its own thread

- **`viewer-sim.js`.** The simulation leaves `viewer.js` for a file of its own, served beside it and
  minified the same way. The policy's `script-src 'self'` admits a worker from a served file; a
  worker from a `blob:` address it refuses, so the file is the worker.
- **Typed arrays.** The worker holds positions, velocities, radii, degrees, workspaces, types,
  relations and contexts as typed arrays. The page sends the graph when it changes and each change of
  state - a wake, a held or dragged node, a focus, the mode, the window's centre - and the worker
  sends positions back (U35).
- **Its own pace.** Behind the loader the worker steps as fast as it can. Once the map shows, it takes
  at most one step every 16.7 ms, so the cooling looks as it did at sixty frames a second whatever the
  page's frames cost, and the page draws every frame with the positions it last received - a pan or a
  zoom never waits on a step.
- **The page's thread when there is no worker.** The same file loads as a classic script, and when a
  worker cannot start, the page steps it from `draw` as it does now.
- **The same layout.** From the same start, 300 steps of the engine on 2,000 nodes left every position
  where the step it was moved from left it, to the last bit, with three workspaces and without.
- **The minimap waits while the layout moves.** With the step gone from the page's thread, a moving
  frame in WebKit was the minimap: its ten thousand dots, one canvas call each, took 39 of a frame's
  63 ms. Copying a dot drawn once, from a canvas each, was worse - WebKit's renderer was kept so busy
  the page's frames came twice a second. The dots are now drawn into one pixel buffer, each a fill and
  its halo laid over the dots before it as before, and while the layout moves the miniature is painted
  at most every 250 ms, and at once when it rests.

At 10,000 entities the layout's motion went from 20 frames a second to 58 in Chrome and from 16 to 58
in WebKit.

Guarded by `the_layout_steps_on_its_own_thread`.

### 27.6 The multilevel start

- **Communities.** Label propagation over the relations, in a fixed order with ties broken by id, so
  a graph gives the same communities every time (Raghavan and others, 2007).
- **Coarse first.** The graph of communities - one node for each, its radius by its size, the
  relations between two summed - is laid out first; each member starts on a spiral within its
  community's disc, and the full layout continues from a cooler start, so it reveals in fewer steps
  (U36). This is the multilevel scheme of Walshaw's and of sfdp, with one level.
- **About the density it settles to.** A community's disc is 40 world units for every square root of
  its members, a little sparser than the layout leaves them, and the full layout continues from 0.25.
  Measured at 50,000 entities, discs of 22 had started the members about twice as dense as the
  layout settles them: the first steps met every crowded pair, at 260 ms each, and the first picture
  took 16 seconds; at 40 a step takes about 160 ms and the first picture 8.9 seconds, and at 10,000
  2.1 where it took 2.8. Sparser still is faster - 135 ms at 50 - but the map then settles much smaller
  than it starts: its loose pieces were left outside, and the view fitted to it held the map at half
  its size. At 34, the density it settles to, the first picture of 50,000 took 9.9 seconds.
- **The nodes with no relation start among the others**, on a spiral through the communities'
  extent; on a ring outside it they were left there as the map settled.
- **Framed again at rest.** The map still settles a little smaller than it is at the reveal, where the
  view is fitted to it, so the view is fitted again when the layout first rests, unless the person has
  moved it.
- **A saved layout skips it.**
- **Small communities join their neighbours.** On the bench's graph, sparse as a store's is,
  propagation left 1,285 communities at 10,000 entities, half of them of three or fewer, and the
  communities' layout, every pair of them for 160 steps, took 1.6 seconds, and at 20,000 6.3. A
  community smaller than six now joins, in label order, the one its members share the most relations
  with, the lower label on a tie; a node with no relation starts on a spiral through the others'
  extent; and the communities' layout meets at most 20 million pairs in all, taking fewer steps when
  they are many.

The first picture of 10,000 entities came in 2.8 seconds in Chrome and 2.4 in WebKit, where it took
6.2 and 5.5, and of 20,000 in 5.5 and 5.0, where it took 11.3; the layout reveals in about 60 steps
where it took about 120.

Guarded by `a_fresh_layout_starts_from_its_communities`.

### 27.7 Clipping

A uniform grid of the node positions, rebuilt when they move and kept while they rest, answers the
window's questions: which nodes are drawn, which can be named, badged or pulse, and which is under the
pointer. A frame then costs what the window holds rather than what the map holds (U37).

- **The grid.** The nodes are counting-sorted into square cells of about four nodes each, never
  narrower than the largest node. A query reads the cells a box covers and answers the indices in the
  order the nodes are drawn in, so what a frame draws, and in what order, is what it drew before. A
  box that holds every node is answered with every index at once, and one that covers more than an
  eighth of the cells by a pass over the nodes, which answers in order without a sort: over that much
  of the map, the cells and the sort cost more than the pass they save.
- **Relations by their midpoints.** A relation is kept in the cell of its midpoint, and one whose
  ends lie more than four cells apart on either axis in a list, kept in order, that every query
  merges into its answer. A relation's box,
  grown by the widest lane, meets a window only if its midpoint lies within two cells and a lane of
  it, so a query reads the midpoint cells of the window grown by as much. Kept in every cell its box
  covered, as first built, the relations made the grid cost more to build than a frame saved while
  the layout moved.
- **Built when the positions move.** A counter, raised when the layout reports positions, a graph
  arrives or a node is dragged, says when the positions have moved; the grid, the regions' geometry
  and the minimap are worked out again when it has changed, rather than compared node by node. The
  layout's worker reports the positions only when a step or a graph has moved them: the camera's place
  at rest goes to it with every pan, and a report for each had the grid built again every frame.
- **The same picture.** Checked against a pass over every node and relation in 40 windows of every
  size, the grid answered the same nodes and missed no relation; and at a camera held still, a
  10,000-entity map was drawn as it was before, pixel for pixel.

Measured against step 34 at 10,000 entities, in three runs of each taken in turn: in WebKit the
moving layout went from 53 frames a second to 58 and a pan's 95th percentile from 11 ms to 8; in
Chrome the frames stayed within the runs' own spread, at 10,000 and at 20,000, where a frame's cost
is its drawing and the passes the grid replaced were a fraction of a millisecond; and a hover's hit
test fell under 0.05 ms. The grid's worth grows with the map: what a frame asks of it costs what the
window holds.

The Log is clipped the same way: it lists the scope's newest 200 observations, asks for one more to
know there are more, and shows 200 more on request; its count reads "200+" until they are all shown.
Its rows are wired by their place in the list rather than found again in it, and an observation it
does not hold - a curation finding's, a search hit's - is read from the scope's log once.

Guarded by `a_frame_touches_what_the_window_holds` and `the_log_draws_its_newest_rows`.

### 27.8 Semantic zoom

Specified first as the multilevel start's communities folded into bodies, it was measured before it
was built: the communities do not stay together. On the bench's 10,000, the 405 communities of six or
more had a median radius of 826 world units about their centre, their members a median 549 from it -
three times a compact disc of their size - since past the start the layout's forces are the
relations' and the regions', not the communities'.

Built then as groups gathered around hubs on the screen - first as discs at their members' mean, then
as a hub over a soft haze of what it gathered, with one line for the relations between two groups -
it drew what was not there. A hub stood for its neighbours, a haze lay where nodes had faded, and a
line stood for relations no two nodes have, so a far map took the groups' shape rather than the
graph's, which the author found broke the graph's form. A node fading by its own size alone would
empty a large map instead: at the fitted view of 50,000 the largest node is half a pixel in radius,
and at 10,000 one node is whole.

The map now keeps its shape by keeping its best-connected nodes where they are (U38):

- **Kept by rank.** The nodes in the drawn types are ranked by their relations, most first. At a zoom
  the map keeps whole about one node for each 12 by 12 pixels of the screen it covers: their number
  times the square of their mean spacing on the screen over 12 pixels, the mean spacing being the
  square root of the area they cover over their number, and that area the disc whose mean distance
  from its centre is theirs, which a few far nodes barely move. The next as many by rank fade out,
  eased and in 32 steps as every fade of the map is (Section 26.5). The count is not bounded by the
  nodes' number, so the fade comes to nothing just as every node is whole, and a map whose nodes stand
  12 pixels apart never fades. As the map zooms out the least-connected fade first, a few at a time,
  and zooming in brings them back where they were.
- **Clipped.** A node faded out is not drawn, not named and not under the pointer, and a relation is
  drawn while both its ends are, fading with the fainter. When the map keeps fewer nodes than its
  window holds - as many as its spacing fits in the window - a frame passes over the kept nodes, the
  first by rank, and their relations, not over every node and relation the window holds.
- **A node kept is seen.** A node smaller on the screen than 1.6 pixels in radius is drawn that large,
  and is under the pointer as far as it is drawn, so a far map is its best-connected nodes, where
  they are.
- **Nothing stands for another.** No node is drawn for its neighbours, nothing is drawn where a node
  has faded, and no relation is drawn that two nodes do not have.
- **Never faded.** A hovered, selected, active or pulsing node, and a contested one with its badge
  (U3), are drawn whatever the zoom.
- **The minimap keeps the same way.** Its miniature drew every node as a dot sized by its relations,
  every relation and every region: at 10,000 the dots covered one another into one disc, and at
  50,000 it took 81 ms to paint, 36 of them the regions. It now keeps nodes by the same rule at its
  own scale, one whole for each 5 by 5 of its pixels and the next as many fading, their dots at most
  1.8 pixels in radius and their halos grown in by their size as the map's are; the relations between
  the nodes it keeps; and the regions at least 12 of its pixels wide, the 48 widest. A miniature of
  50,000 paints in under 3 ms, so it is painted whole while the layout moves too, at most four times
  a second, and the quick miniature of Section 27.2 is gone.
- **The clouds are unchanged.**

### 27.9 WebGL

- **The GPU draws the map's bulk.** Where WebGL2 can render to half-float targets, it draws the region
  field - each cloud an instanced quad adding its weight, its hue's vector and its log-coverage into
  the targets, then one pass mixing them in OKLCH as the 2D field does - the workspace outlines, the
  relations and their arrowheads, the nodes in their three sizes as signed-distance discs with halo,
  fill and trust ring (U39).
- **A 2D canvas above it** draws what is text or few: names, badges, pulses, glows, peers' cursors and
  the relations' labels. The hit test stays on the processor, through the grid.
- **The same tokens.** Colors are the tokens the 2D canvas uses, resolved to RGBA when the appearance
  changes.
- **The 2D canvas stays.** Without WebGL2 or half-float rendering, under forced colors, or when the
  context is lost, the 2D canvas draws all of it as it does now.
- **One drawing, two renderers.** The viewer draws the bulk with the calls it gives the 2D canvas -
  paths of moves, lines, quadratic curves, full circles and rectangles, filled or stroked in a style.
  `viewer-gl.js` takes the same calls and keeps each fill and stroke as instances, in the order they
  came: a circle filled is a disc, stroked a ring; a square, a square; a radial gradient's fill, a glow;
  a line, a capsule, and a curve its own shape by its distance to the curve, so a lane has no joints;
  the arrowheads, triangles. At the frame's end they are drawn in that order, one draw call for each
  run of one kind.
- **Where they differ.** The 2D canvas strokes a look's relations as one path, so two that cross are no
  darker where they cross; on the GPU each is its own shape and a crossing adds, as on every WebGL map.
  The region names are text, so they lie on the 2D canvas, above the relations and nodes rather than
  under them. The field is worked out every frame, at a cell of 2 pixels, where the 2D field keeps its
  image while only the camera moves (Section 26.6).
- **A measure's switch.** `?renderer=2d` keeps the 2D canvas alone, so the bench can take both.

### 27.10 What this is not

It is not a layout in the daemon: the layout stays the page's, and keeping it in the store would be
its own document. It is not a new layout: the forces are Section 26's, and the multilevel start only
changes where they begin. It does not hide knowledge: a faded node comes back as the map zooms in,
List shows every entity the map draws, and a node that is hovered, selected, active, pulsing or
contested never fades.

### 27.11 Sources

Raghavan, Albert and Kumara, Physical Review E 2007, on label propagation; Walshaw, Journal of Graph
Algorithms and Applications 2003, and Hu, Mathematica Journal 2005, on multilevel force-directed
layout; Elmqvist and Fekete, IEEE TVCG 2010, on hierarchical aggregation; Mapbox's supercluster, on
gathering points by zoom; sigma.js's WebGL programs;
MDN, WebGL best practices and the EXT_color_buffer_float and EXT_float_blend extensions; Inigo Quilez
on signed distance functions; the HTTP conditional requests of RFC 9110.


## 28. Icons and notices

The author asked, on 2026-10-21, for the notices to be made finer and for every place that still
draws an icon as text to move to one icon set. This section is the sixth pass. It changes how marks
are drawn and how a notice looks and leaves; it changes no place, no word a person reads first and
nothing the map says.

### 28.1 What is there

The app's own pages draw every icon from Lucide's files (Section 12, "Lucide and Pretendard stay";
`app/assets/icons/README.md`), and the 35 files there are byte for byte those of `lucide-static`
1.52.0. The viewer and the landing draw theirs four other ways:

- **A character for a mark.** A notice's dismiss is the letter x and its kind a 7-pixel dot; a
  workspace a peer is sent shows x to stop sending it; naming a group confirms with "ok"; the
  Definitions disclosure opens with "+ " and closes with "- "; the inspector heads a relation group
  with "->" or "<-"; the Log writes "->" between a search and its hits and between a traverse and
  what it reached; the contested badge is a typed "!" on the map, on the landing's stage and on the
  principles page's pictures.
- **A path written by hand.** The x that closes the inspector, the observation card and the
  settings dialog, the chevron that makes the inspector taller or shorter, and the HUD's list, eye,
  plus, minus and fit are paths drawn on 12- and 18-unit grids, close to Lucide's and not the same.
- **A Lucide path copied into the page.** The title bar's panel toggles, the way back and the
  status bar's gear are Lucide's paths pasted into `viewer.html`, so they are right today and nothing
  holds them to the files the app ships.
- **A notice the settings page's in shape but not in parts** (Section 9). The two are written twice -
  `toast` in `viewer.js` and in the app's `kit.js` - and they differ: the viewer's marks its kind with
  a dot and dismisses with the letter, the app's draws the set's icons; the app's enters over 180 ms
  on a plain ease, outside the three durations of Section 3, and the viewer's does not move; neither
  leaves except by vanishing, and a success leaves 4.5 seconds after it shows even while it is being
  read.

### 28.2 One set

Lucide stays the one set, at 1.52.0, the release the app ships. A second set beside it - Phosphor,
Tabler or Heroicons, each well made - would give one product two hands. Lucide is ISC, so it ships in a
page a browser opens, which Apple's symbols may not (Section 12). It is drawn with a 2-unit stroke on
a 24-unit grid, round at the ends and joins, which sits with the thin solid lines the map draws
(Section 12), and it holds every mark this product uses.

An icon is a mark that stands for a thing or an act in place of a word. A word stays a word: Copy,
Replay, Confirm and Show are not replaced by marks. A count stays a count, "+3" included. A key cap
names a key ("/"). The legend's dots and the map's rings are data, drawn from the data, and the gold
star is the brand mark. None of those is an icon here.

### 28.3 How a page draws one

**The files are Lucide's, unmodified.** Each starts with Lucide's own license line,
`<!-- @license lucide-static v1.52.0 - ISC -->`, and is never edited: a test reads the release from
that line, and a name the two surfaces share must be the same bytes on both. The app keeps its files in `app/assets/icons/`, beside its
pages. The viewer keeps its own in `crates/supragnosis-viz/assets/icons/`, compiled into the binary as
the faces are (U11) and served at `/icons/lucide-1.52.0/<name>.svg` by the same lookup, on the local
console and on a hub's read tier alike, with the same year-long cache: a new release of the set is a
new path. The viewer's policy needs nothing new, since `img-src 'self'` admits a mask image. Two
copies, as with the tokens (U7): a file in both is the same bytes, and a test reads both.

**A page draws an icon as a mask over the text's color.** One class, `.i`, sets the size and paints
`currentColor` through the file's shape, and `.i-<name>` names the file. An icon takes the color of
the text around it, so a state's color reaches its icon with no second rule, and Increase Contrast and
forced colors reach it the way they reach text (Section 10). An icon that stands alone in a button is
`aria-hidden` and the button is named by its act and object (U19). Sizes are two: 16 pixels in the
title bar, the HUD, the inspector and a notice, and 14 in the status bar and inline in a row - a
relation group's head, a Log row. The stroke stays Lucide's, which at 16 pixels is 1.33.

**The map draws the contested badge's mark from the set.** The badge stays a filled `--bad` disc of
7 pixels' radius at the node's upper right (U3); its mark is circle-alert's two strokes - the bar
`M12 8v4` and the dot `M12 16h.01` - drawn as a `Path2D` on the badge's disc taken as the icon's
circle, in the surface color, at Lucide's 2-unit stroke and never under 1.5 pixels. The landing's
stage and the principles page draw the same badge, with the same mark. A typed "!" took the shape and
the weight of whichever face was loaded, and moved with it.

| Today | Shown |
|---|---|
| a notice's dot, its "x" | circle-check, circle-alert or info; x |
| a sent workspace's "x" | x |
| naming a group's "ok" | check, named "Name this group" |
| Definitions' "+ " and "- " | chevron-right, turned down while open |
| a relation group's "->" and "<-" | arrow-right and arrow-left, named Outgoing and Incoming |
| the Log's "->" | arrow-right |
| the contested badge's "!" | circle-alert's mark |
| the hand-drawn closing x, three places | x |
| the inspector's hand-drawn chevrons | chevron-up and chevron-down |
| the HUD's list, eye, plus, minus and fit | list, eye, plus, minus and scan |
| the title bar's and status bar's pasted paths | panel-left, panel-right, chevron-left and settings, as files |

### 28.4 Notices

**One anatomy, on every surface.** A notice is a row: its kind's icon, then its title, at most one
detail under the title and the note of its action, then its action, if any, then its dismiss. The
kinds are three. `ok`, an act done, shows circle-check in `--ok-ink`; `bad`, an act refused or not
done, circle-alert in `--bad-ink`; `info`, news that is not the outcome of the person's act - the
viewer's "New in <workspace>" with Show (Section 6, U20) - info in `--ink2`. The title is
`--text-body` at weight 500, the detail `--text-small` in `--muted` and selectable. The action is a
quiet button with its word. The dismiss is a button showing x and named Dismiss, whose target is 28
pixels square (U19).

**Its surface.** `--panel-2`, a 1-pixel edge in `--line` all the way round, `--r-surface` and the
notice layer's shadow. A `bad` notice's edge is `--bad` mixed 45% into `--line`: a change of the whole
edge, as a selection takes one (Section 4), never a rule on one side of a rounded box.

**How long it stays.** A notice of kind `ok` without an action leaves 4.5 seconds after it shows. The
clock stops while the pointer is over the notice or focus is inside it, and the 4.5 seconds start
again when both have left, so a notice is not taken away while it is being read (WCAG 2.2.1). A `bad`
notice, an `info` notice and a notice with an action stay until they are dismissed or their action is
used. Four at most are shown, the newest first; a fifth takes the place of the oldest.

**How it moves.** It enters over 200 ms on the ease-out curve - from 8 pixels above in the viewer,
where the stack hangs under the header at the top centre, and from 8 pixels to the right on the app's
pages, where it sits at the top right under the title bar - and it leaves by fading over 120 ms. Both
are durations of Section 3. Under reduced motion it appears and goes at once. Where the notices sit is
unchanged (Section 18, step 2), and the stack is still one polite live region (Section 9).

**Written twice, held as one.** The viewer is a page the daemon serves and the app's pages are the
shell's, under two policies with no shared script, so `toast` stays in `viewer.js` and in `kit.js`,
and a test reads both for the anatomy, the kinds' icons, the target, the clock and the motion.

### 28.5 What this is not

It adds no icon where a word is shown: buttons that say what they do keep their words. It is not an
icon font, which would put marks back into text, nor a sprite, which is a build step for what a
class and a file already do. It does not touch the tray's menu, which is the system's, or the app's
own icon.
