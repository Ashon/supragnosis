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
> (Section 28, U41-U42), is built - steps 39-40: Lucide's files on every surface, and a notice with
> one anatomy and a clock that waits while it is read. The seventh pass, curation, wayfinding and words
> (Section 29), goes one step at a time from step 41, each checked against the principles first;
> steps 41 to 45 are built: a confirmation that says what it confirms, a review that closes its loop,
> a map that keeps the person's place, app pages that hold focus and a failure that says so, and one
> concept with one name; and step 46, evidence where a decision is made and provenance to its end,
> is built, as is step 46b, the app that wrote it, step 47, a map that explains itself, and step 48,
> the first entry on screen, step 49, search that finds and holds, and step 50a, the map's keys and
> its View, step 50b, the menu bar and the tray, and step 50c, Settings' sidebar and the window.
> Step 51a, All as a view, is built, and so are step 51b, the same name, and step 51c, Same as...
> and Keep ... instead, step 52, the map on a phone, step 53, time on the map, and step 54, the agent
> surface says where, and step 55, the person finds what waits. Step 56, no merge without an effect,
> is built. Step 57, a repeat says what it found, is built. Step 58, a panel on a tablet ends above
> the camera, is built, and so is step 59, Un-merge opens what it proposed. Step 60, a person can
> demote a claim, is built. Step 61, the band says what it held back, is built. Step 62, the
> ladder says what it held back, is built. Step 63, the window's pages get only what they use, is
> built. Step 64, a pinch zooms as far as the fingers spread, is built.

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
already refuses (U1). *(Revised by step 60: a claim's card casts the same demotion, Section 29.26.)* "Neither is right" asks an agent to record the right value. The console still
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

A type keeps one color in every view a page shows: the graph read names the store's sixteen most
used types, which take the band's slots by the rule above, and any other type takes its preferred
hue, which it may share; a page keeps the assignment it first made (D4, step 47). *(Revised by step
47: a type kept its color only where no earlier name in the view took its hue, so a click on a
workspace recolored a node.)* The legend is what names a color. The landing's stage and the
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

**Workspaces cohere while the boundaries draw them.** *(Revised by step 50a: they cohere whenever more
than one is in scope, whatever Boundaries draws, except under Arrange by type, Section 29.12.)* With
the boundaries on "workspace" and more
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
Graph (Cmd+1) and Review (Cmd+2), and the status bar's gear opens Settings, as it does today.
*(Revised by step 50b: View also holds the map's commands, Edit holds Find, Help the app's help, and
the app menu Setup..., Section 29.13.)* In a
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
  moves into View as **Boundaries** - Workspace, Origin or None - which b still cycles *(revised by
  step 50a: b cycles the choices that outline two or more groups here, and None, and only draws,
  Section 29.12)*. The Types
  tab's rows move here before the tab goes: a contested type definition with its confirm act, an axis
  collision, and the types that are defined but have no node in the loaded graph. Pointing at or
  focusing a legend entry lights its nodes and dims the rest, and its tooltip shows only the winning
  definition; the full entry opens in place.
- **The right side shows one thing at a time**: the inspector as a sheet while a node is focused,
  otherwise the Log. The inspector moves there from the bottom, where it covered the part of the map
  being inspected. Its two regions, relations then evidence, stack in the sheet (inspector.md Section 6).
- **Either panel closes from its toggle**, or with [ and ], and the choice is remembered, except on
  a phone (Section 29.18) *(revised by step 52)*; the edge handles go. With both closed the map's window takes the frame. With the left panel closed and a
  scope of one tier, the status bar's scope line says the tier, as the Trust legend would (U2). A
  Said window dims what was not said in it, and the status line says so (Section 29.19). *(Revised
  by step 53.)*
- **The strip is the Log folded.** While the right side is empty - no Log, no sheet - a one-line
  strip at the window's lower left shows the scope's newest observation: its tier dot, who (the
  on-behalf-of name, else the host by id, called "this node" only as Section 9 allows), when, and its
  first line. It reads `/api/observations` with `limit=1`, a `Read` path on the read tier, refreshed
  with the graph. With nothing observed it says "Nothing observed in this scope yet"; when the server
  does not answer it says so instead of keeping a stale line (P5, P24). Clicking it opens the Log. The
  activity feed's passing rows sit inside the window at its upper left. *(Revised by step 48: with
  nothing observed the strip says "Nothing here yet.", the canvas's words, Section 29.10.)*
- **The map has a List** (U24). *(Revised by step 49: Map and List are one tab list after Follow,
  List follows the working set, its rows are marked when contested and sort by any column, Section
  29.11.)* List, the camera toolbar's first button after Follow (Section 29.4),
  turns the map's window
  into a table of the entities the map draws - the same scope, the same filters, the same search -
  sorted by type and then by name, each row naming the entity, its type, its trust by the legend's
  name and its count of relations, and its workspace when the scope holds more than one. Regions are
  not its groups: they overlap, and an entity can sit in several or in none. A row is a button that
  selects its node as a click on the map does, and opens the sheet; List again, or Esc in the table,
  gives the map back with that node selected. The canvas's name says what it draws and where else it
  is: "Map of 20 entities and 14 relations in 10 regions. List shows them as a table." The table is
  the map for a screen reader and for anyone who cannot tell sixteen colors apart: it names each
  type in words.
- **The camera is one toolbar** at the window's lower right: Follow, List, view, zoom in, zoom out,
  fit (Follow since step 43, Section 29.4; since step 49 Map and List are one tab list in List's
  place, and zoom and Fit are idle while List shows, Section 29.11). Under 760
  pixels it stays at the top right, out of the sheets' way (Section 18). *(Revised by step 50a: the
  minimap draws only while zoomed in, and every framing leaves out the toolbar's box, Section
  29.12.)* The minimap is on, above the
  toolbar, until View turns it off; between 760 and 1200 pixels it gives way to a panel floating over
  the map's right side, and under 760 it is not drawn. View's stored choices forget the minimap's
  once, since any click stored every choice and most were never made, and View remembers it from then
  on.
- **The status bar is the window's state, said once.** On the left, the scope and its counts in
  words: "All workspaces - 20 entities, 14 relations, 10 regions". Counts per type are the legend
  entries' to show, not the bar's. On the right: the way to Review, the sync pill, the search mode,
  then the gear, each a target of U19's size. A pill is tinted, not outlined. How fresh the map is goes unsaid while it is fresh;
  once a poll fails, the bar says "Not updated since 8:54 - the server is not answering", and the
  time does not tick at every poll. *(Revised by step 50a: a Retry button follows that line, Section
  29.12.)*
- **Review is always in the status bar** on the local console (U20). It reads "Review" when nothing
  waits, and "1 needs you" or "2 need you (1 contested)" when something does: one count, the items
  under Needs you, and in the parenthesis how many of them are contested beliefs. Needs you lists
  only beliefs that tie on trust (resolution.md Section 4.1), so those are the loaded scope's
  contested nodes, which U3 asks the bar to count whatever the legend hides, and its contested type
  definitions; before the curation read answers, the graph's count stands in. A conflict that trust
  settled is listed under Health, so a confirmation takes its item off the count. With nothing waiting
  here and something in other workspaces it reads "3 wait elsewhere" (Section 29.21). *(Revised by
  step 55.)* It is a button, the
  Graph page's way to Review wherever Cmd+2 does not reach, and it is not in the title bar, which
  holds no navigation but the way back. *(Revised by step 42: it read "2 to review (1 contested)"
  while the segment read "Needs you 2", and type definitions were not counted; Section 29.3.)*
  On the read tier, where nothing is decided, there is no Review entry, and the bar keeps the
  contested count alone as a pill ("1 contested").

**The first view is where the knowledge is.** The viewer opens on the scope the address names, or
else on the node default. When the default holds at most half of the node's entities while other
workspaces hold the rest, it opens on All and says so beside the chips: "Showing all workspaces -
default holds 2 of 40", or "default is empty" *(revised by step 48: the default by its own name,
Section 29.10; revised by step 55: it opened on All only when the default held nothing, Section
29.21)*. The scope in
use is always a selected chip, All among them, and it is called All, never "(all)" or "*". An empty
default means the wrong scope, not an absence of knowledge (P5).

**Follow moves the camera, not the reader** (U20). With follow on, activity in the scope on screen,
a write or an agent's read, frames the nodes it touched - while the person is not using the map. While
a node is selected, the List, a popover, a card or the Review page is open, or within 20 seconds of
their own pan, zoom or drag, it is a notice with Show instead (Section 29.4). *(Revised by step 43: follow framed every
activity, and moved the camera off the node the person was reading.)* A write in another workspace is a notice
that names the workspace, with Show; a read elsewhere says nothing. The scope, the selection and the
open sheet stay as the person left them: changing what a person is reading is the person's act
(WCAG 3.2.5). Only the knowledge itself can move the selection: when a merge folds the selected entity
into another, the selection follows it there, and when a sync or a merge leaves it out of the scope,
the sheet closes and a notice says so.

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
with a shadow; below 760 the panel is a sheet from the bottom, and a phone opens on the map with no
panel open (Section 29.18). *(Revised by step 52: a phone opened both panels, as a wide window does
by default.)* Below 760 the status bar keeps the
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
  "Set up Supragnosis" (capitalized since step 45, D17), the tray's item "Finish Setup..." and About's act "Open Setup". "Welcome"
  fits only the first visit, and the page is reached three ways after it (four since step 48: the
  empty map's Open Setup, in the app, Section 29.10; five since step 50b: the app menu's Setup...,
  Section 29.13). A state has one word on
  both pages: an AI app that waits for the person's act "Needs you", on Setup as in Settings. A
  control that both pages offer, such as an AI app's row or the model's download, is built once in the
  app's shared script and drawn by both.
- **The plan is checkboxes, and Set up is the default.** Every item of Setup's plan is a checkbox
  whose label is its whole row, since the plan is a choice the Set up button confirms (Section 4).
  Set up is the default button, which Return presses, and the first control after the plan in tab
  order, drawn at the trailing edge. One Copy Diagnostics sits at the left, beside the privacy line,
  once a step has run or failed in this visit, or the check could not be read (Section 29.5), or,
  since step 48, Try it has waited two minutes (Section 29.10). An item that replaces
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
  contested beliefs - those that tie on trust (resolution.md Section 4.1), an entity's type or a
  type's definition (IR5). Each row says what it asks: "Ledger's type: Table or Concept?".
- **Suggestions**: merge suggestions, name variants and names two workspaces share. A suggestion
  offers "Propose", which opens a proposal and decides nothing; a name two workspaces share is two
  entities, so it offers only Show on the map. *(Revised by step 51a: the shared name offered Propose
  in words, which a merge across workspaces cannot be.)* The shared-name row is titled by the name as
  written, says each entity's workspace on its chip and says what the shared name means (Section
  29.16). *(Revised by step 51b: it was titled by its lower-case key, and its chips named the same
  entity twice.)*
- **Health**: overloaded contexts, unlinked entities, merge cycles, type axis collisions, the
  conflicts trust settled and, under All, verdicts filed away from their proposal, folded
  by default. P22 wants curation where the reader already is, not a second job. *(Revised by step
  51a: a verdict filed away from its proposal decides nothing, and was listed nowhere.)*
- **Decided**: every proposal decided, by its outcome in words - Accepted, Rejected, Withdrawn -
  with each act's reverse on its item. A blocked one is still under Needs you.

Every segment is a queue: a list of items on the left, the selected item's detail and acts on the
right, and J and K between them, so a suggestion or a signal is an item as a proposal is. *(Revised
by step 42: Suggestions and Health were one sentence beside one long list, and contested type
definitions waited only in the rail.)*

**Every act is filed where its item lives**, whatever scope is on screen: a verdict in its
proposal's workspace, a proposed merge in its two entities' workspace, a split in its merge's, a
named region in the workspace of the observations that asserted it, and a settle in its
observation's (below). All is a view and no place to file anything (Section 29.15). *(Revised by
step 51a: the rule was written for Settle alone, and every other act taken under All was filed in
the node default.)*

An item shows, in this order: what it would do in one sentence; for a draft, its confidence; the
belief diff, before and after (proposal-workflow.md Section 14.2); each check, and for a failing one
the reason in words and the act that fixes it where there is one; the evidence, each observation with
who said it and when; and the preview on the map that the console draws today.

**The decision controls sit beside that and act on that one item** (U4). For a proposal: Accept,
Reject and Skip, also as A, R and J, with K to step back; an open merge of two entities also offers
Keep ... instead, which has no key (Section 29.17). *(Revised by step 51c.)* For a draft: Publish,
Accept and Discard, as dreaming.md Section 8 names them, also as P, A and D. There is no decide-all,
except Reject all blocked (Section 29.21). *(Revised by step 55.)* A key decides only
once the item's diff and checks are on screen, so stepping fast through the list cannot cast a verdict
on an item not yet shown. A recall asks for a second, explicit act: its effect is the strongest the
console casts, and one keystroke should not carry it. A kind with no commit effect yet - a type
change or a recall - offers no Accept, and its item says why (Section 29.22); a recall asks twice
once it has its effect. *(Revised by step 56: both kinds offered Accept while a merge of them changed
nothing.)*

**Keys act where the person is** (U19). A, R, J and K act only while focus is in the list or the
item; anywhere else they are letters (WCAG 2.1.4). Opening the page puts focus on the selected row,
so they act from the first press. A contested item has Skip, also as J, and its values' Confirm
buttons are 1, 2 and on, each opening its ask as a click does. Each control names its key in
`aria-keyshortcuts`, not in its name, so it is read as "Accept" and not "Accept A". Skip is a button
as well as J. The segments - Needs you, Suggestions, Health, Decided - are tabs that move with the
arrow keys. After a decision redraws the list, focus lands on the next item, or on the list when none
is left. While the page is open, the map and the panels behind it are inert.

**What follows a decision is said plainly.** A decided item leaves Needs you at once, and the next
one is selected. Its reverse is on its Decided item (Section 29.3):
- An accepted merge offers "Un-merge", which opens the split proposal (unmerge.md) and states its
  two consequences: the separated pair is never suggested again (unmerge.md S5), and merging them
  later needs a proposal distinguishable from this one (unmerge.md S10).
- A rejected proposal stays listed among the decided and offers "Accept after all", also as A,
  since a reject is provisional and a later accept still merges it (proposal-workflow.md Section 8),
  except while its reverse merge is accepted, when no Accept is offered (Section 29.17). *(Revised by
  step 51c.)*
- A merged claim promotion or demotion offers its Undo, with the reverse acts' diff.

The notice right after a decision names the item and offers the same act where the act decides
nothing or its note says the diff. On this page it leaves after 4.5 seconds like a success without an
action, since the act stays on the Decided item (U42).

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
- **Confirm asks once, in place**, and names what it confirms: the source it promotes, in that
  source's words, and every entity whose ring the promotion raises with it (Section 29.2). The two
  buttons are Confirm and Cancel, and Confirm acts only once the engine's diff and checks for that act
  have been read. The act is the promotion `/api/resolve` casts. *(Revised by step 41: the ask first
  read "Confirm Database? It becomes Human confirmed, and Service stays listed.", which named the
  value and not the source it promotes with everything else that source said.)*
- **When the shown value is already Human confirmed**, confirming the other would tie them again, so
  the other row's act is "Confirm instead", and its question says both halves: "Confirm Service
  instead? Database goes back to Agent, and Service becomes Human confirmed." It casts a demotion of
  the shown value's source to the tier it has without any person's verdict, which `/api/explain`
  answers for each source, and the promotion of the chosen one; the ask reads both acts' diff as one
  (Section 29.2).
- **The notice that follows offers Undo**, and stays until it is dismissed, as an error does; on the
  Review page it leaves after 4.5 seconds, since the same Undo is on the confirmation's Decided item
  (Section 29.3), except after "Confirm instead", whose Undo reverses two acts. Undo
  casts the reverse: a demotion of the source just confirmed to the tier that source had before the
  Confirm, read from the diff's `tier_changes`, and after "Confirm instead" a confirmation of the
  source it demoted. The log keeps every event (P3). A demotion is a ceiling (resolution.md Section
  5): until a person confirms that source again, a later attestation that would raise its tier does
  not. The notice's note says so and names the entities whose ring Undo moves back, from the
  reverse act's diff: "Undo returns this source to Agent and keeps it there until someone confirms
  it again. Nightly export goes back to Agent." *(Revised by step 41: Undo first demoted to "the tier
  its row showed before", the value's highest, which after a choice of a lower source would have
  raised it.)*
- **Every act is filed in the workspace of the observation it changes**, whatever scope is on screen.
  From All, `/api/resolve` would otherwise file it in the node default, where the fold that reads the
  observation's own workspace never sees it.
- **"Neither is right"** opens a sentence to give an agent, with Copy: "Record the correct type of
  Postgres in supragnosis." A correction is an assertion the log must hold before anyone can confirm
  it (resolution.md Section 4.2), and the console observes nothing.

The way to these items is Review in the status bar (Section 6), whose count is the items under Needs
you. The tray menu's "Review - 3 need you..." item counts the same items on this Mac's daemon,
across its workspaces, from the proposal list and the curation report, read once a minute, and is
in the menu only while some wait. It opens Review at All workspaces, the scope its count is of. When
the curation read fails and the proposal list answers, it says what it counted: "Review - 2
proposals wait...". On a remote profile the tray stays as remote-viewer.md Section 6 has it.
*(Revised by step 42: the tray counted only the proposals, since it did not make the curation read
every minute; the author chose that it does, D12.)*

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
  the row the person moved to. Once a row is chosen, the field shows its name. *(Revised by step 49:
  a choice clears the field, the set row "Show these 13 on the map" comes first, and an empty field
  offers recent searches, Section 29.11.)*

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
an error stays until it is dismissed, four at most, in a polite live region. Section 28.4 gives both
surfaces one anatomy for them, and a clock that stops while a notice is read (U42).

**Words are the reader's.** The interface stays in English. A system term may stay in a tooltip or a
disclosure, where it links the screen to these documents, and never in the line a person reads first.
Hashes and ids live in footers and cards. One line never states two contradictory states. A host is
named by its id; it is called "this Mac" in the app on a Mac and "this machine" elsewhere only when
the id is the one `/api/surface` reports for the node serving the page, since every unconfigured node
calls itself `localhost`. *(Revised by step 45, D2: it was "this node", the word the rail used for an
entity.)* The product is "Supragnosis" in a sentence and "supragnosis" in a command, a path, the
CLI's own output and code (D17).

**A tier is named, never spelled** (U17). Every place that shows a tier uses the Trust legend's
names, which are Unverified, Agent, Host signed and Human confirmed. The identifiers stay in tooltips
and in cards' footers.

**The Log speaks in sentences** (U17). A proposal event says what happened to which proposal in
words - "Merge proposed: PostgreSQL into Postgres", and for a gate act the source it moves: Claim
confirmation accepted: "The ledger is a Postgres table..." (Ledger, Nightly export). Decision rows
are drawn in the Log's full ink. *(Revised by step 42: a confirmation read "Claim confirmation
accepted" with no object, and decision rows were faint and italic.)* A who-line
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
| same across this scope: agent_extracted / from localhost | All here: Agent, from this Mac (or this machine) - since step 45 |
| accepted entity_merge [blocked]: c9a66148 -> T2 | You accepted merging into "T2". A check blocks it. |
| grab-bag contexts / reify | Overloaded contexts / Name this group |
| merge cycles / name variants / orphans | Conflicting merges / Possible duplicates / Unlinked |
| Healthy - nothing owed | Up to date |
| Peers, client, local +N | Sync, Spoke, this Mac (or this machine) +N - since step 45 |
| settling layout... | Arranging 168 entities - since step 45 |
| accepted claim_promotion: 9f778c20 | Claim confirmation accepted: "The ledger is a Postgres table..." (Ledger) - since step 42 |
| opened entity_merge: PostgreS -> Postgres | Merge proposed: PostgreSQL into Postgres |
| ws (all) / nodes 20 / edges 14 / Component 3, ... / hyperedges 10 (hull 4) | All workspaces - 20 entities, 14 relations, 10 regions |
| (all), * | All |
| hulls / all regions / hull | Regions |
| keep / use this | Confirm, for every value |
| agent_extracted, host_signed | Agent, Host signed |
| welcome / Finish setup... / Run setup again | Setup / Finish Setup... / Open Setup |
| Needs switching | Needs you |

**A state keeps one word** (U17). The daemon's state is said with these words on the tray's line,
Settings' Server and This Mac panes, Setup's first card and the starting page (step 45):

| The daemon | Word | Said with |
|---|---|---|
| started by the shell, not answering yet | Starting... | nothing to press for 10 seconds (Section 29.5) |
| answering at the installed version | Running | who keeps it: starts at login, while this app is open, Homebrew's service, `supragnosis start`, started outside this app |
| answering at an older version than the installed one | Update ready - restart | both versions (daemon-lifecycle.md L3) and where to restart |
| answering while its store repairs | Repairing - N items left | |
| not answering, and nothing starting it - a login job's, and one the app attached to, included | Stopped | a login job's last exit, where Setup reads it |
| its start failed, or the app's child exited | Could not start | the cause, in one sentence |
| claimed by two managers | Two managers run Supragnosis | where to settle it |
| unable to open its store for its model | The search index needs rebuilding | the two models, and Rebuild index |
| not used, a remote server is | Not in use | the server's name |
| not read: the command line did not answer | Unknown | |

"Answering" and "Not answering" are said of a remote server only. A step's own pills on Setup - Done,
Not done, Needs you - are the step's, not the daemon's. The app's own state - a `supragnosis-server`
newer than the app - is the tray's line "supragnosis-server is newer than this app - update the
app". No page reads a state out of these words: Settings is given the shell's state as a field.

**Headings are in sentence case** (U17): only the first word and names take a capital, and a count
stands beside its heading in the muted count style - "Relations 3", "Provenance 2" - not in brackets.

**Retired words** (U17). Each phrase of the left column, as it was written and matched case for case,
fails the build when it appears in the viewer's markup or script, the app's pages and scripts, the
shell's lines or Setup's sentences, comments and test code left out:

| Retired | Said instead |
|---|---|
| this node | this Mac, or this machine |
| This node | This Mac, or This machine |
| Node types | Entity types |
| Edge types | Relation types |
| between nodes | between entities |
| links between groups | relations between groups |
| nodes they touch | entities they touch |
| linked to a match | related to a match |
| No item in | No entity in |
| no node is ringed | no entity is ringed |
| belief decision | Why this value |
| Needs switching | Needs you |
| nothing owed | Up to date |
| projections owed | Repairing - N items left |
| settling layout | Arranging N entities |
| After a commit, ask for the decision | Save the why behind each commit |
| Keep it running | Keep Supragnosis running |
| Set up supragnosis | Set up Supragnosis |
| Welcome - supragnosis | Setup - Supragnosis |
| Running only while the app is open | Running while this app is open |
| run by this app, not at login | while this app is open |
| Restart to update | Update ready - restart |
| Not running | Stopped |
| daemon: FAILED | Could not start |
| daemon: CONFLICT | Two managers run Supragnosis |
| licence | License |
| daemon: starting | Starting... |
| externally managed | started outside this app |
| login item | starts at login |
| observe knowledge | What an AI app remembers shows up here |
| Nothing observed | Nothing here yet. |
| The store holds an entry | An entry is in "billing", from Claude Code, just now. |
| restarted and loaded supragnosis | Claude Desktop is open. Its row says when it reaches the server. |

## 10. Keyboard, motion and contrast

- Every control is a `button` or an `input`. Chips, legend entries, proposal rows and log rows stop
  being clickable spans. Tab lists carry their roles and move with the arrow keys.
- The map is reached through search, and from a focused node its relations are a Tab away in the
  sheet. Esc closes the topmost card, menu or sheet (step 0), and since step 49 ends a working set
  last, when nothing else is open (Section 29.11).
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
  outside a text field, and View has a switch that turns them off (WCAG 2.1.4). *(Revised by step
  50a: =, +, -, 0, l and ? join them, they act on the Graph page while no dialog or card is open,
  and the switch is in the Keys dialog, Section 29.12.)* The
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
| Check it yourself | One paste that shows a disagreement kept and a credential refused (step 66) | - |
| Connect | The apps, the one command that connects them, the tools | the MCP surface |
| Under the hood | Six one-line properties | the eight capability cards |
| Status and questions | Shipped, next, who it is not for yet, four questions | the status ledger |
| Get started | Three steps, with the install tabs | get started |

*(Revised by step 66: eight sections, with Check it yourself after the viewer.)*

The portability section goes with its three topology drawings, two of which drew meshes that have
not shipped; moving between machines is the stage's fifth step.

**The hero.** The heading stays: memory for AI agents that remembers who said it. The subhead names
the clients and the split of the work - agents write as they work, a person decides what becomes
canon. *(Revised by step 66: in the same 25 words it also says what only this product does, that each
machine checks what it receives; "remembers who said it" is a claim other products now make, and the
receiver's own evaluation of trust is not.)* The act is the install command in two tabs, the macOS app (`brew install --cask
ashon/tap/supragnosis`, the one spelling the page uses; *revised by step 48*: followed by `&& open -a
Supragnosis`, Section 29.10) and the script, with a copy button; beside it
are the install script's source, GitHub and the docs. One line of facts follows: the version, Rust,
the licenses, the platforms the binaries are built for (macOS on Apple silicon, Linux with glibc 2.39
or later), and no account. The version is stamped on deploy by one substitution over exactly one
`<li id="version-badge">` holding plain text (pages.yml), so the page keeps exactly one such element,
with no other attribute. Nothing in the hero waits for a script or an animation to be seen. The
install script the hero links to says what its binary searches with: the release is built with the
local embedding model, and the script's keyword-only notice predates that.

**After the command, the next step.** Under the install command one line says what follows it: "Then
connect an agent: `supragnosis connect claude-code`, or Connect in the app. All steps ->"
*(revised by step 48: "The app's Setup opens: it keeps supragnosis running, offers to connect each
AI app it finds and downloads the search model. With the script, run `supragnosis setup`. All steps
->", Section 29.10)*, which links to Get started, and the header gains Install beside its other links. Below 560 pixels the
command wraps and its Copy button sits on its own line; a code block that still scrolls takes focus.
The hero's picture of a question and an Accept chip is a picture: no pointer, no hover, nothing that
looks pressable. *(Revised by step 66: the Review snippet in the govern step is no longer a picture -
its Accept chips are buttons, and pressing one casts the verdict on the stage.)* The header's links stand at least 24 pixels apart, a finger's width (Section 10).

**The stage** is one canvas beside the five steps. Each step is a sentence, a short paragraph and one
snippet of the product's own text, and the scene beside it shows the same thing happen to a made-up
project - a payments service, its webhooks and their idempotency, nothing from a real store:

1. **Write.** Three agents' entries fly into one log; the first shows what an entry keeps - its
   workspace, the machine that wrote it, the person it acts for, its source and how sure it is. The log
   records the machine and the person, not which agent app wrote an entry, so the agents' names are
   drawn beside the log and never on it. The words add that after a commit Claude Code is asked for
   the decision behind it (client-connect.md Section 12), and the snippet's source is that commit. It
   answers "who said this?". *(Revised by step 66.)*
2. **Structure.** Each entry rises into the entities it names and draws its relations, and what was
   said together gathers into a region. A range in the snippet replays the log: at k the stage draws
   the graph of the first k entries, the entry it reaches rising as it does in the scene, and the
   snippet's counts are recomputed from the same entries, so replaying the log is something the visitor
   does rather than reads. *(Added by step 66.)*
3. **Govern.** A second type for one entity raises the contested badge, and it stays: nothing settles
   it in this story. Two names for one thing become a merge proposal, and an agent asks a person to
   confirm one of its claims (webhooks need an idempotency key). *(Revised by step 66:)* The scene
   stops there, with both waiting, and the step's snippet holds them as Review rows whose Accept is a
   button. Pressing one casts that verdict: its tick flies from the person into the log, and the merge
   folds one name into the other, or the confirmed entry's tick turns gold, as the viewer's Log draws a
   human-confirmed row, and the two entities it names take the gold ring, since a node's trust is its
   best support's tier. The row then says what was done. Scrolling on is accepting both: the later
   steps start from the accepted graph, and the snippet says so. The words say what is flagged - a
   second type, where trust ties; a description's latest wins (resolution.md Section 2.2) - that once
   merged an agent's read of either name follows the merge, and that a verdict can be undone in
   Review. It answers "has anyone checked it?".
4. **Recall.** Another agent searches; what matches stays lit while the rest dims, as the viewer's
   search lights its answer, and a thin line runs from the answer back to the entries that say it -
   the receipts. A verdict is not a receipt; the entry it confirmed is. *(Revised by step 66:)* The
   snippet is the two calls an agent makes: `search_knowledge`, whose three hits are buttons, and
   `get_entity` of the chosen one - who said it, its entries and its trust, since a hit carries none of
   them (the MCP answer names an entity's name, type and workspace). Choosing a hit lights it and
   traces its own receipts.
5. **Travel.** The log crosses a hub to a second machine, which checks the signatures (an entry is
   signed when it leaves the machine that wrote it) and replays the same graph: the same contested
   badge, the same merge, and the same gold rings, because a verdict is an entry and the tier it grants
   is read from the log (resolution.md Section 6). Only the workspaces the person allows travel. What
   cannot arrive as human confirmed is a bare claim: an entry that only says a person checked it counts
   for no more than its signature proves (resolution.md R4). *(Revised by step 66:)* Two choices in the
   snippet say what stands behind the gold ring: the verdict, which both machines draw gold, or an agent
   saying a person checked it - an entry whose words claim it, which both machines draw as Agent, with
   no ring, since words in an entry are not a verdict and a local writer's tier is the default. A
   claimed tier arriving from a peer is the snippet's own row: host signed at most. The words say how a machine joins: a one-time code
   from `supragnosis invite`, `supragnosis join`, and three words compared (pairing.md), after which
   it syncs on its own. It answers "does my other machine know?".

The words carry the whole meaning. The canvas is hidden from assistive technology and the page reads
complete without it; what is worth reading - an entry's provenance, a Review row, a search answer's
receipt, the two machines' tiers - is a snippet in the step, and the canvas draws only names and
short labels, at 11 to 12 pixels. *(Added by step 66:)* A control a step adds is a button or a range
in its snippet, reached by the keyboard as any other; the canvas draws its result, and the snippet's
words change with it, so a reader without the canvas reads the same outcome. Hover means nothing.

**The stage's marks are the viewer's** (U10). An entity is filled with its type's color, computed by
the viewer's own functions of the type's name, copied into the stage; the stage's type names are
chosen so their colors sit apart from each other and from gold, coral and teal. A node has a halo of
the ground. A gold ring is human confirmed, at the viewer's width and offset; nothing on the stage is
host signed or unverified, so it draws no other ring. *(Step 66 keeps this: an agent saying a person
checked it draws no ring on either machine.)* The coral badge at the upper right is
contested. A relation is the viewer's neutral line at its alpha, the one color the stage copies from
the viewer's script rather than from a token. A merge waiting for a person is the viewer's proposal
preview: an arrow from the name folded away into the one kept, in `--accent`, and the relation that
would move with it drawn thin in `--accent`. Log ticks are colored as the viewer's Log colors a row: an agent's entry in
`--ink2`, a human-confirmed entry in `--gold-bright`, a proposal muted and a verdict in `--ink`. Teal
is an agent at work, as the viewer's footprint glow marks what an agent's session touched; the
person and their verdicts are ink. Every other color the stage draws is a landing token.

**Motion.** A scene plays when its step reaches the middle of the window, once, in at most five
seconds, and then rests: at rest nothing moves and no frame is drawn. *(Added by step 66:)* A press
plays only what it changes, in under two seconds, and rests again; under reduced motion it cuts to its
result. Each scene ends within the five
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
plays when the frame comes into view, and the stage's column is not drawn. *(Revised by step 66: the
frame stands between the step's paragraph and its snippet, so it plays after the words that say it
and beside the controls that drive it.)*

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
the status - what shipped, what is owed and who it is not for yet - and answers four questions:
whether anything leaves the machine, whether it needs a key or a model, what happens if you stop
using it, and how it differs from a notes file. It names no other product: a claim about one would be
ours to keep true, and we could not. *(Added by step 66:)* What is next is not scheduled - a person
picks each bundle - so the list is what is owed. And the page offers one check a visitor can run:
Check it yourself is a paste that runs `supragnosis serve --store mem --embed none`, a server on a store
in memory that stores none of what it is sent, sends it two entries giving one entity two types and one
carrying AWS's published example key, and prints what comes back - the entity contested by the other
type, and the refusal naming the credential's shape. It runs on the released binary; the page says
what it should print.

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

**Six parts**, in this order *(revised by step 67)*:

| Part | What it does |
|---|---|
| Hero | The heading, a subhead saying that each principle is split into clauses and each clause names its test or says it is owed, and the ledger's count with a link to it |
| Chapter navigation | The six chapters of principles.md, held at the top while the page scrolls |
| Six chapters | Each chapter's name, a one-line thesis and its part of the ledger, then its principles - an index, one row a principle, until one is opened |
| Tensions | Appendix A's rulings, as a map and a list |
| How a principle is held | The ledger, its legend and glossary, the four evidence states, a rule about change tested as change, and links to the registry and the policy cases |
| Closing | The normative text, what is owed and when (architecture.md Section 14), and the install |

**One principle at a time** *(added by step 67)*. With a script, the chapters are an index: each
principle is a row of its number, its title and its line, and its title opens it. An opened principle
(`#pN`) is the only one on the screen, under its chapter's name and thesis, with a bar that says where
it is ("P3 of 24"), steps to the previous and the next in the document's order, and leads back to its
chapter in the index; the arrow keys step and Escape goes back, unless a control has the focus. It
reads in one order whatever its kind: the line, the reason, the animation or demo, the clauses, the
links. Opening one, from the index or from a link to `#pN`, puts it at the top of the page, moves the
focus to it (it is named by its title) and names it in the window's title. The other parts
stand below the index and leave the screen while a principle is open. Without a script the page is
the whole of it, as before, and every anchor still lands where it points. The ledger leaves the hero
for How a principle is held, where its words - clause, tested, by construction, pinned, owed - are
defined beside it; the hero keeps its count. Before step 67 a reader passed the ledger, its legend, a
glossary and the chapters' navigation, about 300 words and 40 stops for the keyboard, before the
first principle, and the ledger chose P3, whose clauses use words the page never defines.

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
and none plays only because it scrolled into view. *(Revised by step 67: one the reader opened plays
when it first comes into view, since opening it is asking to watch.)* A separator such as "/" is drawn by CSS with empty
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
| **U4** | On the Review page a decision control appears only with its item's belief diff and checks on screen, and decides that item alone; there is no batch decision, except one act that rejects every Blocked item at once, each listed with the check that blocks it before the act is cast (Section 29.21; the author's decision of 2026-10-22, that what the system cannot process may be dropped). Every open proposal is listed; a generated candidate is left out only when its diff is computable and empty (P23, proposal-workflow.md Section 14.2). |
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
| **U16** | A type's color is never a signal's: every color of the band stands at least 25 degrees of OKLCH hue from the accent, human-confirmed, ok and bad tokens and has OKLCH lightness 0.62 or more and 5.5:1 or more on `--surface`; the band's first tone stands 0.09 apart in OKLab, 0.11 from every signal and 0.10 from every neutral the map draws, and its second 0.06 from every color of the band and 0.08 from every signal; a type keeps one color in every view a page shows, and the store's sixteen most used types differ in hue or tone (D4); a relation kind draws no color; and every copy of the band is the viewer's (P6, P18). |
| **U17** | No line a person reads first spells a system identifier: a tier is shown by its legend name; every proposal kind the engine opens and every event it carries has a sentence in the Log, and a hash id is left out of it; each observation row has a who-line naming tier, who and when, and its mark is the map's ring for its tier; a place keeps one name wherever it is reached; a state of the daemon keeps one word on every surface that shows it, from Section 9's state words; and no retired word of Section 9 reaches a page, the tray or Setup's sentences (P2). |
| **U18** | Settling a contested value says why the shown value is shown, offers every value the same act at the same weight, says what the act does before it is cast - both halves when it demotes the shown value, each naming the source it moves, every entity whose ring it moves and what else that source states, read from the engine's diff and checks, with no act and no Undo before that diff is read - lets the person choose the source when several assert the value, and is undone by the reverse gated events, which the log keeps; every act is filed in the workspace of the observation it changes, and the console asserts no value of its own (P3, P6, P23). A demotion from a claim's card holds to the same: what it overturns, the rings it moves and what else the claim states, read from the engine's diff and checks before the act, and no Undo before its own diff. *(Revised by step 60.)* |
| **U19** | Every control on the viewer, the Settings and Setup pages, the landing and the principles page has a name, is the kind its act is and exposes its state; a dialog holds focus and makes the page behind it inert; a page that draws itself again keeps the control that had focus; a key that decides acts only while focus is in its list or item, and the view's single keys can be turned off; every target a pointer presses is at least 28 pixels with its spacing on the viewer and the app's pages, 44 under a coarse pointer, and 24, 28 below 760 pixels wide, on the landing and the principles page; and nothing but a focusable code block scrolls sideways at 320 pixels. |
| **U20** | On the local console the status bar always holds the way to Review, its count the items under Needs you, which lists only beliefs that tie on trust, with the contested ones counted in it; and follow never changes the scope, the selection or the open sheet - it frames activity in the scope on screen while the person is not using the map, names it in a notice with Show while they are, and names a write elsewhere in a notice - while a selected entity a merge folds is followed to what it became, and one that leaves the scope closes its sheet with a line that says so (P22, P24). |
| **U21** | Every font size the viewer and the Settings and Setup pages draw is one of Section 4's six, through a size token in rem; running text and every control's label are 13 pixels or more, and nothing is under 11; the map's labels scale with the root's size; the app's View menu sets the root's size from 100% to 200% and keeps it, while the title bar keeps its height and its traffic lights' place; and no text is cut short except where focus or a press shows it whole. |
| **U22** | Every corner the viewer's, the app pages' (`settings.css`, `welcome.css`, `shell.css`) and the landing's stylesheets draw is one of Section 3's radius tokens, none, or a dot's circle; a part inside a rounded part takes the outer radius less the space between them; a pill and a large control are capsules, and the controls of one row share one size; a checkbox, a radio and a switch take the accent; and no line is dashed or dotted, on a page or on a canvas. |
| **U23** | Under `prefers-contrast: more` every text token takes the next stronger one, every control, chip and field draws an edge of 3:1 on its surface, and the focus ring is 3 pixels; under `forced-colors: active` the map redraws in the system's colors, keeps every mark it draws, and widens the human-confirmed ring. |
| **U24** | The map's window offers List: a table of the entities the map draws, in its scope, filters and working set, starting by type and name and sorted by any column, each row naming the entity, its type, its trust by the legend's name, its count of relations and, when the scope holds more than one, its workspace, marked when contested, and selecting its node as the map does; and the canvas's name says how many entities, relations and regions it draws and that List holds them.  List also shows when each entity was last said (Section 29.19). *(Revised by step 53.)* |
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
| **U42** | A notice has one anatomy on every surface - its kind's icon, a title, at most a detail and one action, and a 28-pixel dismiss showing x; a success without an action leaves 4.5 seconds after it shows, counted only while the pointer is off it and focus outside it, and so does a success on the Review page whose act is also on its Decided item, and every other notice stays until dismissed or used; four at most; it enters over 200 ms and leaves over 120 ms, and under reduced motion does neither (Section 28, WCAG 2.2.1). |
| **U25** | A tooltip holds a name and its key and nothing a person needs to act: every tooltip the viewer and the app's pages write goes through one helper and is at most five words before its key, an interpolated name counting as one, and what a control does is said on the screen or behind a control that a keyboard and a finger reach. The map's hover card is a card, not a tooltip: it says the entity's facts and nothing to act on. |

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
`the_minimap_window_is_concentric_with_its_frame` holds the one rounded part a canvas draws inside
another: the minimap reads its frame's radius from the stylesheet, clips to that outline, and runs
its window's line on the radius less the half pixel it sits in, wherever the line meets the curve.

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
never called over a list of items; every open or blocked proposal is listed. The detail's evidence
is guarded by `the_review_page_shows_its_evidence` in `crates/supragnosis-viz/tests/review.rs` -
identity first for a merge, every check in words, the evidence, who asked, a contested item's
sources - and the engine's half, a relation at its source's effective tier, the act that moved an
observation and the trust profile, by
`a_confirmed_source_says_who_confirmed_it_and_its_relations_take_its_tier` in the engine's policy
cases.

U8 is guarded by `a_pill_is_absent_only_for_a_known_zero_or_nothing_set_up`: the contested pill hides
only at zero, and the sync pill hides only off the local console or where sync is not configured, and
draws "Sync unavailable" when the daemon does not answer.

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
the app's tests, which holds Setup's one name in its crumb, the tray and About. Its words are guarded
by `no_retired_word_reaches_a_page` in the same file, which reads Section 9's retired words and fails
when one is in a page's, the shell's or Setup's source, and `the_maps_activity_speaks_in_sentences`,
which holds the activity's rows and the footprint in words, and
`a_who_line_names_the_writer_apart_from_whom_it_wrote_for`, which holds the who-line's grammar and
the card's Cited, confirmation and links; and by `the_daemon_has_one_word_per_state`
and `the_ai_app_row_is_built_once` in the app's tests - the state words on the tray, Settings, Setup
and the starting page, and the AI app's row drawn from `kit.js` by both pages.

U19's first part is guarded by `the_viewers_controls_say_what_they_are` in
`crates/supragnosis-viz/tests/controls.rs` and `the_app_pages_dialogs_hold_focus` in the app's tests:
View's toggles, the legend's chips, the workspace chips, the page's language and the map's name, and
the dialog on the app's pages; `the_app_pages_keep_focus_across_a_draw` in the app's tests holds the
part a draw owes - Setup and Settings draw through `kit.js`'s `keepFocus`, their controls carry the
keys it finds them by, and the reads on a timer or on focus draw only what changed;
`a_focus_ring_is_never_cut_by_a_scroll_container` in `controls.rs` holds the ring whole - every
container of the viewer's stylesheet that scrolls keeps a 6-pixel gutter, and the status bar's
controls draw the ring inside; and `view_holds_two_choices_with_one_name_each` holds View's two
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
handler changes no scope and notes a write elsewhere with Show; and a default holding at most half
of the knowledge, or nothing, with knowledge elsewhere opens on All and says so (Section 29.21).

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
39. **Icons** [built] (Section 28.1-28.3; U41): the viewer's files under `assets/icons/` and their path, the
   `.i` classes, every mark of Section 28.3's table, the contested badge on the map and the landing,
   and the test that reads both surfaces.
40. **Notices** [built] (Section 28.4; U42): the three kinds, the surface, the clock that stops while it is
   read, the motion, in `viewer.js` and `kit.js` both.
41. **A confirmation says what it confirms** [built] (Section 29.2; U18): each value's sources in `/api/explain`,
   `/api/resolve/preview` with the diff's `reach` and the checks, the ask with the source and its reach,
   the choice of source, both halves of Confirm instead, Undo's reach, no act without its diff, and the
   tiers defined under the legend.
42. **Review closes the loop** [built] (Section 29.3; U4, U20, U42): a decision leaves Needs you at once, every
   reverse on its Decided item and in the notice after it, Decided in names and outcomes, the queue
   that says what it asks, one count with the tray's read of curation, contested type definitions in
   Needs you, keys from the first press, Suggestions and Health as queues, the map preview's way back,
   and the Log's decision rows.
43. **The map keeps the person's place** [built] (Section 29.4; U20, U26): follow that waits while the map is in
   use and its toggle on the camera toolbar, the selection in the address, the way back to the
   address left, a selection history, Esc giving the camera back, a layout reset, and a selection
   followed when it is merged or named when it leaves.
44. **The app's pages hold focus, and a failure says so** [built] (Section 29.5; U19): focus kept across a
   draw and no draw for an unchanged read, the starting page's three states with Restart, Diagnostics
   and the log, a restart that leaves a login job its daemon, Settings acting on what that page asks,
   selectable commands and paths with Copy, and Setup's foot.
45. **One concept, one name** [built] (Section 29.6; U17, D2, D17): this Mac or this machine for the page's
   own machine, entity, relation and type for the graph's parts, sentence-case headings with their
   counts, the map's activity in sentences, one word per daemon state, the AI app's row built once in
   `kit.js`, This Mac's own sentences, Supragnosis in sentences, and the retired words as a tripwire.
46. **Evidence where a decision is made, and provenance to its end** [built] (Section 29.7; U4, U17, U18,
   P14): one who-line grammar, the card's Cited, confirmation and links, an observation read by its
   id, each contested value's source, Review's identity diff, every check and the evidence, what
   waits for the person on the inspector, a relation's trust, and Neither is right's sentence. The
   app that wrote an attestation (D13) is step 46b.
46b. **The app that wrote it** [built] (Section 29.8; D13, onboarding.md O7): the MCP client's name recorded in
   a local table beside each attestation its session writes, store format 3, one name table in the
   core crate, said in the who-line and the map's activity, and kept off the wire and the read tier.
47. **The map explains itself** [built] (Section 29.9; U16 and Section 5 revised by D4, U25, D3, D5): the map's
   key, counted chips with Only and Show all, counts of what is drawn, one color per type for the
   store, trust toggles that dim, the hover card, and one word for the region.
48. **The first entry on screen** [built] (Section 29.10; D18; onboarding.md Sections 4.4, 7 and 8): Try it
   quotes the entry, names the app that wrote it and opens the graph on it; the wait follows first
   contact, with the minutes and what to check; the landing's one line opens the app; an empty scope
   says so in one voice and keeps no camera controls that act on nothing; and the default is called
   by its name.
49. **Search finds, and holds** [built] (Section 29.11; D8; Sections 6, 8, 10 and 29.4 and U24 revised): a
   choice clears the field, an empty field offers this page's recent searches, hits name their
   workspace, "Show these 13 on the map" pins the answer as a working set that dims the rest and
   grows by Show related, List follows the set, and Map | List takes List's place, with List's rows
   marked when contested and sorted by any column.
50a. **The map's keys and its View** [built] (Section 29.12; D7; Sections 5, 6, 10, 18 and 29.4 revised): single keys
   for zoom, Fit, List and the Keys dialog, which holds the single-key switch; View holds what
   changes the picture; b only draws; Retry where the map is stale; the minimap while zoomed in; and
   framing that keeps the toolbar's box. The menu bar and the tray are step 50b; Settings' sidebar
   and the window's frame are step 50c.
50b. **The menu bar and the tray** [built] (Section 29.13; D14, D15; Sections 6 and 6a, settings-page.md
   Section 5, remote-viewer.md Section 6 and compatibility.md Section 8 revised): the app menu's
   Setup..., Edit's Find, View's map commands, Help, the tray's state line with its act in a fixed
   order, an icon that changes shape only while the memory cannot be used, Quit that says it stops
   the memory, and one id per item per menu. Settings' sidebar and the window's frame are step 50c.
50c. **Settings' sidebar and the window** [built] (Section 29.14; settings-page.md Sections 3.0 and 3.0.1
   revised):
   a sidebar with no heading for its one group, a count that says what it counts, a dot on AI apps
   whenever a row needs the person, the last pane reopened, and a window that keeps its size and
   place and has a smallest size.

51a. **All as a view** [built] (Section 29.15; principles.md Principle 15, proposal-workflow.md I2,
   proposal-workflow.md Section 3.1, proposal-workflow.md Section 6, proposal-workflow.md I16,
   resolution.md Section 5,
   unmerge.md Section 9, prompts.md Sections 4 and 5, compatibility.md Section 7, and this
   document's Section 7 revised): `*`
   reads every workspace and files into none, on every surface; a verdict counts only in its
   proposal's workspace, and one filed elsewhere is listed in Health; every console act under All is
   filed where its item lives; and a merge joins entities of one workspace. The same name is step
   51b, and Same as... and Keep ... instead are step 51c.
51b. **The same name** [built] (Section 29.16; D9; Section 7, inspector.md Section 5 and remote-viewer.md
   Section 3.2 revised): on the local console the sheet says when another workspace
   uses the entity's name, All lights the drawn namesakes with what is pointed at or selected, and
   the Suggestions row for a shared name says what it is and which workspace each entity is in.
51c. **Same as..., and Keep ... instead** [built] (Section 29.17; D10, D19; Sections 7 and 29.7,
   proposal-workflow.md Section 14.3 and resolution.md Section 6 revised): a person may say two entities of one workspace are
   the same, a merge on Review offers to keep the other name, and what the console opens says it
   was opened at the console.
52. **The map on a phone** [built] (Section 29.18; D16; Sections 6 and 29.12, Section 18's step 2 and
   inspector.md Section 6 revised): one finger moves the map and two zoom it, the page opens on the
   map, the inspector's sheet has two heights, and the camera buttons sit at the map's foot above the
   sheet.
53. **Time on the map** [built] (Section 29.19; Section 6, U24, inspector.md Section 5 and compatibility.md
   Section 7 revised): each entity carries when it was last said, the hover card, the sheet and List
   say it, and a Said filter dims what was not said today, in the last 7 days or since the person's
   last visit.
54. **The agent surface says where** [built] (Section 29.20; compatibility.md Section 7 and architecture.md
   Section 7 revised): every write's answer names its workspace and says when it made a new one,
   search hits name their entity, a name passed as an id is said to be one, a failed call is flagged
   as failed, a merged promotion says what it granted, and a merged demotion is no longer capped.
55. **The person finds what waits** [built] (Section 29.21; Sections 6 and 7 and U4 revised): the page opens
   on All unless the default holds most of the knowledge, Review and its pill say what waits in other
   workspaces, and the Blocked items can be rejected together.
56. **No merge without an effect** [built] (Section 29.22; Sections 7 and 29.3 revised): a type change and a
   recall, which have no commit effect yet, cannot be merged until they have one - the fold counts
   no merge of either and every surface refuses one - and Review, the agent's answers and tool
   descriptions, and the prompts say so.
57. **A repeat says what it found** [built] (Section 29.23; Section 29.17, unmerge.md Section 5 and
   resolution-identity.md Section 3 revised): a person's act at the console asks again when the
   answer it would land on no longer holds, an act that finds its answer waiting or in force says so,
   the agent's `propose` says what it found, and suggestions follow the person's no.
58. **A panel on a tablet ends above the camera** [built] (Section 29.24; Section 29.18's owed overlap repaid):
   from 760 to 1200 pixels a floating panel ends above the camera toolbar under a finger as with a
   mouse.
59. **Un-merge opens what it proposed** [built] (Section 29.25): the split Un-merge proposes opens under Needs
   you, as Same as... and Keep ... instead open theirs.
60. **A person can demote a claim** [built] (Section 29.26; Section 2, U18 and proposal-workflow.md Section 14.3
   revised): the observation card offers a demotion to Unverified, with what it overturns, its
   reach, its ceiling and its checks before the act, and Undo after it.
61. **The band says what it held back** [built] (Section 29.27; resolution-identity.md Section 3 and
   unmerge.md Section 5 repaid): `merge_band` counts the near pairs it left out, and an empty band
   says whether none are near or the near pairs it found are all answered.
62. **The ladder says what it held back** [built] (Section 29.28; Section 29.27's owed row repaid): the
   name-variant ladder counts the pairs it left out because they are answered, and Suggestions says
   by name, as it does by meaning, whether none are alike or the ones it found are all answered.
63. **The window's pages get only what they use** [built] (Section 29.29; Section 29.13's owed menu
   interface repaid, settings-page.md S8): a page in the main window may listen for and emit the
   shell's events, ask whether the window is full screen and drag it, and nothing else of the
   platform's: not the menus, the tray, images, paths or resources.
64. **A pinch zooms as far as the fingers spread** [built] (Section 29.30; step 52's rule carried to
   the trackpad): a trackpad pinch scales the map with the fingers, 1:1 around the pointer, as a
   touch screen's pinch already does, and the mouse wheel keeps its step of 12% a notch.

Steps 18 and 19 change the landing's stylesheet and step 22 is the site's own, so like step 16 they
merge on the author's word, right before the release that carries steps 17-21 and 23.

Each step is judged at the shell's default size and at 1440, 760 and 390 pixels wide, on the busiest
hub, an isolated node, a contested node (a seeded store, as inspector.md Section 8 notes), a blocked
proposal and the read tier.

## 15. What this revises

- **settings-page.md Section 5**: the tray menu gains "Review (N)..." while something waits on this
  Mac's daemon (since step 42, "Review - N need you...", Section 29.3). The status line is unchanged
  (until step 50b, which splits it into a state line and an act line, Section 29.13),
  and on a remote profile the tray is as remote-viewer.md Section 6 leaves it.
- **settings-page.md Section 3.0** (step 43): the way back goes to the graph as it was left, its scope
  and selection, which the way in carries in `?from=` (Section 29.4). Since step 50c the gear and
  Cmd+, reopen the last pane, and the tray's Settings... the first section whose dot shows (Section
  29.14).
- **settings-page.md Section 3.0.1** (step 50c): the sidebar has no heading for its one group, AI
  apps' count says "2 connected", and its dot shows whenever a row needs the person (Section 29.14).
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
  "this node" (Section 9). The read tier's answer is unchanged. Since step 48 it also gives
  `default_workspace`, the node's default, so the page calls the default by its name (Section 29.10).

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
  framing leaves out the sheet's height. *(Revised by step 52: at the top they covered the labels,
  so they sit at the map's foot, above whatever sheet is open; framing leaves out their box there.)*
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
- **After step 7: the minimap starts on again.** *(Revised by step 50a: it stays on, and draws while
  zoomed in, by Section 29.12's rule.)* Step 7 started it off to clear the corner; at the
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
  Definitions disclosure, and the principles page's clause lists, open with "+" and close with "-";
  the inspector heads a relation group
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
star is the brand mark, as the landing's wordmark "* supragnosis" is. The shell's "$ " before a
command Setup ran, the slash between the landing's facts and a step's number are typography. None of
those is an icon here.

### 28.3 How a page draws one

**The files are Lucide's, unmodified.** Each starts with Lucide's own license line,
`<!-- @license lucide-static v1.52.0 - ISC -->`, and is never edited: a test reads the release from
that line, and a name the two surfaces share must be the same bytes on both. The app keeps its files in `app/assets/icons/`, beside its
pages. The viewer keeps its own in `crates/supragnosis-viz/assets/icons/`, compiled into the binary as
the faces are (U11) and served at `/icons/lucide-1.52.0/<name>.svg` by the same lookup, on the local
console and on a hub's read tier alike, with the same year-long cache: a new release of the set is a
new path. The viewer's policy needs nothing new, since `img-src 'self'` admits a mask image. Two
copies, as with the tokens (U7): a file in both is the same bytes, and a test reads both. The landing
keeps the one file it draws, chevron-right, in `site/icons/`, the third copy under the same rule.

**A page draws an icon as a mask over the text's color.** One class, `.i`, sets the size and paints
`currentColor` through the file's shape, and `.i-<name>` names the file. An icon takes the color of
the text around it, so a state's color reaches its icon with no second rule, and Increase Contrast and
forced colors reach it the way they reach text (Section 10). An icon that stands alone in a button is
`aria-hidden` and the button is named by its act and object (U19); one that carries meaning beside
text, such as a relation group's direction, is `role="img"` with its word. Where a page does not write
the markup - the principles page's clause lists, which its generator writes - the mark is the
element's pseudo-element with the same mask, and generates no text. Sizes are two: 16 pixels in the
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
| the HUD's list, eye, plus, minus and fit | list, eye, plus, minus and scan, and since step 49 the Map tab's waypoints (Section 29.11) |
| the title bar's and status bar's pasted paths | panel-left, panel-right, chevron-left and settings, as files |
| the principles page's "+" and "-" on a clause list | chevron-right, turned down while open |

### 28.4 Notices

**One anatomy, on every surface.** A notice is a row: its kind's icon, then its title, at most one
detail under the title and the note of its action, then its action, if any, then its dismiss. The
kinds are three. `ok`, an act done, shows circle-check in `--ok-ink`; `bad`, an act refused or not
done, circle-alert in `--bad-ink`; `info`, news that is not the outcome of the person's act - the
viewer's "New in <workspace>" with Show (Section 6, U20) - info in `--ink2`. The title is
`--text-body` at weight 500, the detail `--text-small` in `--muted` and selectable. The action is a
quiet button with its word. The dismiss is a button showing x and named Dismiss, whose target is 28
pixels square (U19).

**Its surface.** `--panel-2`, `--r-surface` and the notice layer's shadow, with no edge: areas are
told apart by surface (U9). A `bad` notice's surface is `--bad` mixed 6% into `--panel-2`, a tint of
the whole notice as a selection takes one (Section 4), never a rule on one side of a rounded box. Six
is the most that keeps `--muted` at 4.5:1 on the light ground (U7): 4.60 there, against 4.49 at 8.

**How long it stays.** A notice of kind `ok` without an action leaves 4.5 seconds after it shows. The
clock stops while the pointer is over the notice or focus is inside it, and the 4.5 seconds start
again when both have left, so a notice is not taken away while it is being read (WCAG 2.2.1). A `bad`
notice, an `info` notice and a notice with an action stay until they are dismissed or their action is
used - except on the Review page, where a success whose act is also on its Decided item leaves as one
without an action does (Section 29.3). Four at most are shown, the newest first; a fifth takes the
place of the oldest.

**How it moves.** It enters over 200 ms on the ease-out curve - from 8 pixels above in the viewer,
where the stack hangs under the header at the top centre, and from 8 pixels to the right on the app's
pages, where it sits at the top right under the title bar - and it leaves by fading over 120 ms. Both
are durations of Section 3. Under reduced motion it appears and goes at once. Where the notices sit is
unchanged (Section 18, step 2), but on the Review page at 760 pixels and wider, where the stack sits
over the list column so it does not cover the item's heading (Section 29.3); the stack is still one
polite live region (Section 9).

**Written twice, held as one.** The viewer is a page the daemon serves and the app's pages are the
shell's, under two policies with no shared script, so `toast` stays in `viewer.js` and in `kit.js`,
and a test reads both for the anatomy, the kinds' icons, the target, the clock and the motion.

### 28.5 What this is not

It adds no icon where a word is shown: buttons that say what they do keep their words. It is not an
icon font, which would put marks back into text, nor a sprite, which is a build step for what a
class and a file already do. It does not touch the tray's menu, which is the system's, or the app's
own icon.

### 28.6 What building it changed

- **A notice has no edge.** The first draft gave it a 1-pixel edge in `--line` that an error turned
  toward `--bad`. U9 holds that areas are told apart by surface, with a line only where it carries a
  fact, and its test refused the edge. An error tints the notice's whole surface instead, at the
  strength the contrast pairs allow (Section 28.4).
- **The landing has a third copy.** The principles page's clause lists opened with "+" and closed with
  "-", generated by its stylesheet; they take chevron-right from `site/icons/`, as a pseudo-element,
  since the page's generator writes the markup.
- **Some generated text is typography.** Reading every stylesheet for generated text found the shell's
  "$ " before a command Setup ran, the landing's wordmark "* ", the slash between its facts and a
  step's number. They stay, and Section 28.2 names them.
- **A mask needs `forced-color-adjust: none`.** Under forced colors a page's background colors are the
  system's, and a mask's fill is a background color, so an icon would take the ground's color and
  vanish. The app's `.i` lacked it too.

## 29. Curation, wayfinding and words

The author asked, on 2026-10-21, for a review of every surface through the lenses of interface,
experience, information architecture, interaction design and Apple's guidelines. Four reviewers,
working from a fifth's survey of those lenses, found 70 points: one blocker, 33 major, 31 minor and
5 polish. A storyboard grouped them into
fourteen scenes along four journeys: first use, exploring, deciding and running the app. The author
took the recommended answer to each of the nineteen open questions. The author also asked that the
steps go one at a time, and that each be checked against the principles before it is made. This
section is the seventh pass: the steps in order, each with the check that came before it.

### 29.1 How a step is checked

Before a step changes a rule, it lists every principle, invariant and specified sentence the change
touches, and says how the change stands with each:
- **agrees**: the change keeps the rule as written;
- **repays**: the rule already asked for this and the build fell short;
- **contradicts**: the change and the rule cannot both hold;
- **owed**: the rule asks for more than the build gives, before and after the step, and the step
  names the gap rather than closing it.

A contradiction stops the step until the author decides. Until then the principle stands and the
change waits. The table stays in this section once the step is built, as the record of why the
change was allowed.

### 29.2 Step 41: a confirmation says what it confirms

**What was found.** Confirm on a contested value casts a `claim_promotion` of one observation that
asserts the value: the representative that `/api/explain` reports. A claim promotion promotes an
assertion bundle (proposal-workflow.md Section 3, kind 1), so every entity and relation in that
observation is confirmed together with the value.

The ask said none of this. It named only the value: "Confirm Concept? It becomes Human confirmed,
and Table stays listed."

Measured on the review's demo store, confirming Concept as Ledger's type confirmed the sentence
"The nightly export sends the ledger to the finance team's warehouse". Nightly export and Warehouse
took the gold ring. Nobody had looked at either of them.

When two observations asserted Concept, the page picked which one to promote, and did not say so.

**The check.** A first draft of this step was checked by a second reader against the principles and
the specs. That reader found three contradictions in the draft: tier definitions that misdescribed a
ring, an Undo sentence that was false, and a source choice whose Undo could raise a tier. The rows
below are the corrected step. No row needs a principle changed.

| Rule | What it asks | Standing |
|---|---|---|
| P18, promotion (principles.md, Principle 18) | "Promotion of the trust tier happens only explicitly" | repays - the old ask raised entities nobody chose; now every entity whose ring a Confirm raises is named before it is cast |
| P23, the gate | "a belief diff (promotions/reversals/new contradictions/blast radius) and check results must already be computed" | repays items 1-3 of the diff and the checks for the console's settle; item 4, the knowledge derived along `derived_from`, stays owed here as for every kind |
| U4 | A decision control appears only with its item's diff and checks on screen | repays - the ask shows both |
| proposal-workflow.md Section 3, kind 1 | A claim promotion promotes an assertion bundle | agrees - the bundle stays the unit; the ask says what is in it |
| proposal-workflow.md Section 5, item 1 | "Assertions being promoted - what newly enters canon" | repays - the diff gains `reach`, which for a demotion lists what leaves |
| BeliefDiff, the fold's own effect | What a reviewer reads is what the fold will do | agrees - `gate_tiers` gains an ordered list of hypothetical events in place of its one, so "Confirm instead" and its Undo, two events each, are previewed as the pair they are |
| U2 and Section 5 | A ring is the tier of the node's best support | agrees - the reach names an entity only where its ring changes, with the tier it lands on; the tier definitions describe a source, not a value |
| U18 | Says what the act does before it is cast; every value the same act at the same weight | repays the first clause; the source choice agrees because Undo lowers the chosen source to its own tier before the act (the diff's `tier_changes`), never to the value's highest, so undoing a choice cannot raise anything |
| U17 and inspector.md D7 | No identifier in a line read first; text from the log is untrusted | agrees - the ask quotes the source through the escaping `html` builder and names entities, never an id |
| P5, truncated | A cut answer says it is cut | agrees - a quote cut at 280 characters ends with "..." and its accessible text says it was shortened |
| U42 | A notice holds at most one detail and one action's note | agrees - Undo's reach is part of its note |
| U19 | A control exposes its state | agrees - a Confirm waiting for its diff, or refused one, is disabled and says why beside it |
| P21 | No new tool without a repeated intent | agrees - no tool is added; MCP `get_proposal`'s answer gains the diff field `reach`, which the pinned `tools/list` (compatibility.md Section 7) does not cover |
| remote-viewer.md, "Every path on the read tier has a declared policy" | The read tier names each path | agrees - the preview path is declared refused with its own reason (the read tier offers no settle), not as a verdict, and remote-viewer.md Section 3.2's table gains the row |
| proposal-workflow.md I7 and I12 | A verdict is bound to the base it was reviewed at | owed, as before - nothing binds the cast to the previewed base; the page reads the preview again whenever the poll sees the log move, which narrows the gap without closing it |
| P3 | Nothing is destroyed | agrees - the preview writes nothing |

Found while checking, and not changed here: a relation's trust in the graph read is its attested
tier with no gate event applied (`graph`, the edge rows). resolution.md Section 3 says that every
display reads the effective tier, so a confirmed source's relations still read Agent. Edges draw no
ring, so nothing on the map is wrong today. A later step gives relations their trust marks and
repays this first.

**Behaviour.**
- **Each candidate value lists its sources.** `/api/explain`'s candidate rows gain `observations`:
  every observation that asserts the value, each with its effective tier and its tier before any
  person's verdict (`base_tier`), which a "Confirm instead" demotes the shown value's source to.
  They are ordered by the rule that already picks the row's representative, highest effective tier
  and then smallest id, so the representative comes first. The ask preselects the policy's own choice for the shown value (`kind_source`) and the
  representative for every other value.
- **The preview.** `GET /api/resolve/preview` takes the query `/api/resolve` takes: the
  observations, the tier and the kind, and once more for a second act, so "Confirm instead" is
  previewed as one. It applies the acts in order after every merged event, as the fold would, and
  answers their belief diff and checks without writing anything. It refuses observations from two
  workspaces, as `/api/resolve` does.
- **The diff's `reach`.** Diff item 1 becomes `reach`. For each target whose tier changes, it holds
  what the observation said (its first 280 characters, and whether it was cut) and the tier it
  moves from and to. It also holds the entities the observation asserts, each with its ring before
  and after, and the relations and type definitions it states.
- **The ask names the source and its reach**: "Confirm Concept as Ledger's type?" Then "This
  confirms one source:" with the source's words in a quote and its who-line. Next, the entities
  whose ring rises: "With it, Nightly export and Warehouse become Human confirmed." At most four are
  named, then "and N more". Then what else the source states: "It also states 2 relations" or "It
  also defines 3 types". The ask closes with the line Human confirmed has under the legend. The
  buttons stay Confirm and Cancel.
- **The person chooses the source.** When more than one observation asserts the value, "Confirm
  from" is a group of radio buttons, one per source, each named by its who-line and first words.
  The reach follows the choice.
- **"Confirm instead" says both halves with their reach** (U18). The preview applies the demotion
  of the shown value's source and then the confirmation of the chosen one, so the ask reads the pair
  as one result.
- **Undo says what it moves back.** When the notice is drawn, the page reads the preview of the
  reverse act. The note reads: "Undo returns this source to Agent and keeps it there until
  someone confirms it again. Nightly export goes back to Agent." That reverse act lowers the chosen
  source to the tier the diff's `tier_changes` gives as its own before the Confirm. After "Confirm
  instead", the reverse is both acts in reverse order, previewed together. If that preview cannot
  be read, the notice offers no Undo and says so.
- **No diff, no act.** A Confirm, a Confirm instead or an Undo acts only after its preview has been
  read. While the preview is being read, the button waits and says "Reading what this confirms...".
  If it cannot be read, the button stays disabled and the ask says so (P23).
- **The tiers are defined where trust is decided.** Under the Trust legend, beside its note on
  rings, a disclosure "What these mean" gives one line per tier, describing a source:
  - Human confirmed: "Confirmed through this app on one of your machines, by you or another program
    on your account; agents are told to leave it to you. No agent's claim outranks it; a later review
    can lower it." *(Revised 2026-10-23: it read "A person confirmed it", which the console cannot
    tell from a program of the same user - resolution.md Section 6.)*
  - Host signed: "Above Agent, but not confirmed through this app: a trusted machine's signed claim, or
    a review an agent accepted." *(Revised 2026-10-23: "but not by a person".)*
  - Agent: "What an AI app writes, and what this app writes for you, until a review raises or
    lowers it."
  - Unverified: "Trusted less than an agent's claim: a review lowered it, or its sender marked it
    so."

  A tooltip cannot carry these (U25).

**What this step does not do.** It does not let a person confirm one field without the rest of its
source. A field-scoped promotion would change what a gate event targets: an assertion inside an
observation, instead of an observation. That touches P23's list of proposal kinds, proposal-workflow
I6 ("merge is additive") and Section 3, resolution.md Section 3's gate tier per observation, and how
gate events replicate (federation.md). It is a later step with its own check. Until then, the ask
says the bundle's reach honestly.

### 29.3 Step 42: review closes the loop

**What was found.** Measured on the review's demo store, and read in the code:
- **A decided item stays, still asking.** After Confirm on a contested item in Review, the notice
  says it is done and the pill drops to 2, but the list still shows 3 and the item is still selected
  with its Confirm live. A second Confirm casts a second promotion. A conflict that trust settled
  stays in the curation report (resolution.md R7), so the report's length and stats do not change,
  and the page's redraw signature holds only those.
- **A blocked merge never leaves.** A reject cast on it is recorded, but the fold keeps the state
  "blocked" whenever a merge verdict exists. proposal-workflow.md Section 7.1 concludes Rejected when
  no valid merge exists and a valid reject does, and a blocked merge is not valid.
- **Three acts reverse three ways.** A value confirmation has Undo, in a notice that never leaves. A
  merge has Un-merge, on its Decided item. A claim confirmation has nothing once its notice is
  dismissed.
- **Decided forgets what was decided.** Once a merge lands, its row reads "(an entity not on the
  map)", since the page names targets only from the graph. Its tag is a state id, "merged", which a
  confirmation shows too. Its diff is worked out against today's beliefs and reads "overturns no
  current belief". The Accept hint shows on items with no Accept.
- **The queue does not say what it asks.** A row reads "Merge / Merge PostgreSQL into Postgres", and
  a contested one "Ledger: which value?".
- **One queue, three names and two counts.** The tray counts open and blocked proposals across the
  workspaces. The pill and the segment count Needs you in the scope on screen, as "3 to review" and
  "Needs you 3". A contested type definition waits only in the rail's Definitions and enters no count.
- **The keys on the buttons do nothing at first.** Opening Review from the pill leaves focus outside
  the list, and on a contested item A and R return early and there is no Skip.
- **Suggestions and Health are not queues.** The list column holds one sentence and the detail holds
  everything; J and K do nothing there.
- **"Show on the map" does not come back to its item.** The map holds no way back. The pill reopens
  Review on Needs you, so an item left from Decided is lost, and Cmd+2 keeps the segment and the item
  but is a key to know. It frames only the entity merged into, and a click on the empty map ends
  it.
- **The Log's decision rows say the least.** They are drawn at 72% opacity in italic, and a claim
  confirmation reads "Claim confirmation proposed", then "Claim confirmation accepted", with no
  object.

**Decisions taken.** The author took the recommendation on both, on 2026-10-21:
- **D11:** every act's reverse is on its Decided item, and the notice right after a decision offers
  the same act. On the Review page that notice leaves after 4.5 seconds, as an exception to U42.
- **D12:** the tray counts all of Needs you, reading the curation report once a minute, and a
  contested type definition joins Needs you.

The review also suggested folding one console act's two Log rows into one (R3-15). That changes
U17's "every event it carries has a sentence in the Log", the storyboard the author approved did not
carry it, and this step leaves the rows apart.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U4 | A decision control appears only with its item's diff and checks on screen, and decides that item alone | agrees - "Undo confirmation" on a Decided item shows the reverse acts' diff and checks; "Accept after all" is Accept on a rejected item, with its diff and checks as before; Un-merge opens a proposal and decides nothing. A notice offers an act only where the act decides nothing or its note says the act's diff (an Undo), so a reject's notice offers none |
| U42 and Section 28.4 | "every other notice stays until dismissed or used"; the stack "hangs under the header at the top centre" | contradicts, on the Review page; decided by the author (D11). U42 and Section 28.4 gain: on the Review page, a success whose act is also on its Decided item leaves as a success without an action does. The stack, still one live region, sits over the list column there at 760 pixels and wider, so it does not cover the item's heading |
| Section 7, Undo's notice "stays until it is dismissed, as an error does" | | contradicts, on the Review page; the same decision. After "Confirm instead" the notice's Undo reverses two acts, which no one Decided item holds, so that notice stays |
| WCAG 2.2.1, timing adjustable | Content that times out can be kept | agrees - a notice that leaves takes no act away, since the same act is on the Decided item; its clock still waits while the pointer or focus is on it |
| Section 7, the tray's count | "contested beliefs and suspected secrets need the curation read, which the tray does not make every minute" | contradicts; decided by the author (D12): the tray makes that read once a minute. Measured: `/api/curation?workspace=*` answers in 112 ms on the author's store of 437 entities |
| Section 6, the pill | "those are the loaded scope's contested nodes, which U3 asks the bar to count" | revised: Needs you's contested beliefs are the scope's contested nodes and contested type definitions, and the parenthesis counts both |
| U20 | The pill's count is the items under Needs you, which lists only beliefs that tie on trust | agrees - a type definition ties as a kind does (resolution-identity.md IR5), so a contested one joins Needs you and its count, and a settled one moves to Health |
| resolution.md Section 4.1, and resolution-identity.md IR5 | Only a tie waits for a person; a definition ties as a kind does | repays - Needs you listed entity kinds only |
| proposal-workflow.md Section 7.1 | No valid merge and a valid reject conclude Rejected | repays - the fold concludes Rejected for a blocked merge a person rejects, so it leaves Needs you; once its check passes, the merge verdict is valid and it is Merged, as a late merge is (I16). The reject is provisional, and the item says so |
| unmerge.md Section 9 | A split's target carries a merge verdict, "`merged` or `blocked`, not `open` or `rejected`" | revised: the check reads whether a merge verdict was cast, so a blocked merge that a reject made Rejected still counts, and the fold, the shown checks and the split's diff agree (I13). proposal-workflow.md's monotonicity notes gain the same case |
| proposal-workflow.md Section 7.1(d) | A verdict is valid only on an Open proposal | owed, as before - "Accept after all" casts a merge on a Rejected proposal, which Section 8 allows ("a later accept still merges it") and (d) does not; Section 4's [impl] note says (d) is not enforced |
| proposal-workflow.md I18 | Curation commits nothing | agrees - the report gains type-definition conflicts and still writes nothing |
| compatibility.md Section 7 | MCP pins its `tools/list` | agrees - no tool's input changes; `list_proposals` and `get_proposal` rows gain `labels`, and the `curate` prompt's digest gains type-definition rows |
| compatibility.md Section 8 | The couplings between the app and the daemon are listed | owed, and repaid here - the tray already read `/api/proposals` over the socket, which Section 8 did not list; it now reads `/api/curation` too, and Section 8 names both and what the tray reads of them |
| U17 | No identifier in a line read first; a place keeps one name | repays - Decided says Accepted, Rejected and Withdrawn; titles name merged-away entities and a gate act's source from the server; the queue is "Needs you" wherever it is counted; Section 9's words stay ("Overloaded context", "Name this group") |
| U18, as step 41 sharpened it | A confirmation confirms a source, with everything else that source says | agrees - a confirmation's title names the source and the entities it names. The notice right after names the value too, as step 41's does, and the console records that sentence as the proposal's rationale, so the Decided item says both. The review's "Confirmed: Ledger's type is Concept" (R3-15) would name the value alone, which the log does not hold |
| U19 | A key that decides acts only while focus is in its list or item | agrees - opening Review puts focus on the selected row, so the keys act from the first press and still only there; a contested item gains Skip (J), and its first nine Confirm buttons as 1 to 9 |
| U26 | Esc and Cmd+[ take the way back | agrees - the map preview from Review has a way back: its banner's "Back to Review", Esc and Cmd+[ |
| Section 9, the Log's sentences | "Claim confirmation accepted" | revised: a gate act's rows name its source, and decision rows are in the Log's full ink |
| P18 and P23 | Promotion only explicitly; the diff before the act | agrees - Undo from Decided casts demotions through `/api/resolve`, as step 41's Undo does, only after their diff is read |
| proposal-workflow.md I16 | Merge is absorbing | agrees - Undo casts new demotions, and the confirmation stays merged |
| resolution.md Section 5 | A demotion is a ceiling | agrees - the Undo's note says so, as step 41's does |
| remote-viewer.md Sections 3.2 and 3.4 | Every path on the read tier has a declared policy; a read names nothing outside the reader's grants | agrees - `/api/resolve/undo` is refused there with its own reason, as the preview is. A label names only what is in the proposal's own workspace, so a reader granted that workspace is told nothing from another; a cross-workspace merge's far side has no label |
| P21 | No new tool without a repeated intent | agrees - no tool is added (compatibility.md Section 7's row above) |

No row needs a principle changed. Two rows contradict a rule of this document, and the author's
D11 and D12 decided both.

**What the server adds.**
- **Labels.** A proposal row's `labels` name each target: an entity by its name, merged-away ones
  included, as the Log's event rows already do; a gate act's observation by its first 80 characters
  and the entities it names: "The ledger is a Postgres table..." (Ledger, Nightly export); an
  un-merge's merge by what it merged. A label names only what is in the proposal's own workspace.
  The Log's event rows name a target that is not an entity the same way.
- **The undo read.** `GET /api/resolve/undo?proposal=<id>` answers, for a merged claim promotion or
  demotion, the acts that reverse it: for each target, a demotion (or promotion) to the tier it has
  with this proposal's merge verdicts - an agent's and the console's alike - left out of the gate
  fold. It answers those acts' diff and checks as the preview does, and no acts when the proposal no
  longer moves any target: another proposal holds the same tier.
- **The fold.** A blocked merge with a valid reject concludes Rejected (proposal-workflow.md Section
  7.1). A proposal row says `merge_verdict` when a merge verdict was cast on it, so the page can say
  that such a reject is provisional.
- **The report.** Contested and settled type definitions join `contradictions`, with `field`
  "definition", the type's `target` and `workspace`, and the defining observation as `kind_source`.
- **The rationale.** Every act the console casts records the sentence its notice says as its
  rationale: "Confirmed Concept as Ledger's type"; "Table's source goes back to Agent (Confirm
  instead)"; "Undid Confirm a claim: "The ledger is..." (Ledger)".

**Behaviour.**
- **A decision leaves Needs you at once.** After Accept, Reject or a Confirm on the Review page, the
  item leaves Needs you with no other input, and the selection and focus go on to the next item. The
  redraw signature holds every item's key and state: a contradiction's id, whether it is contested
  and its current value; a proposal's id, state and verdicts.
- **Every reverse is on its Decided item.**
  - An accepted merge offers Un-merge, as now.
  - A rejected proposal offers "Accept after all", also as A, with its diff and checks; it merges
    it, since a reject is provisional (proposal-workflow.md Section 8).
  - A merged claim promotion offers "Undo confirmation" when it raised its source to Human
    confirmed, and "Undo promotion" otherwise. It is read from `/api/resolve/undo` when the item is
    selected and shown with the reverse acts' diff and checks, in the words step 41's Undo note
    uses. One act is cast per target, each to its own tier, one after the other; if one fails, the
    notice says which targets were returned and which were not (U1). When the read answers no acts,
    the item says "Nothing to undo: another decision holds this source where it is."
  - A merged claim demotion offers "Undo demotion", the same way in the other direction. Its note
    names who cast the demotion, since undoing it can raise a source someone lowered on purpose.
  - The two acts of a "Confirm instead" are two Decided items, each with its own reverse; undoing
    one alone moves only its own source, and its note says what that moves back, from the diff.
  - A split offers none, and says what undoing it takes: "Merging them again takes a new proposal."
    A type change or a recall merged before step 56, which changed nothing, says "Nothing changed,
    so there is nothing to undo." (Section 29.22). *(Revised by step 56: this line gave all three
    the split's sentence, and the console said "Undoing this takes a new proposal" for the other
    two.)*
- **The notice right after names the item.**
  - "Accepted: Merge PostgreSQL into Postgres" offers Un-merge.
  - A confirmation's notice offers Undo, with step 41's note. After "Confirm instead" it stays, since
    its Undo reverses both acts.
  - "Rejected: Merge PostgreSQL into Postgres" offers nothing, and says "Accept after all is under
    Decided".
  - On the Review page the others leave after 4.5 seconds, counted as U42 counts.
- **Decided says what was decided.**
  - An item keeps the title it had under Needs you, from the server's labels: "Merge PostgreSQL
    into Postgres"; Confirm a claim: "The ledger is a Postgres table..." (Ledger, Nightly export).
  - Tags say the outcome: Accepted, Rejected, Withdrawn. A blocked item is still under Needs you.
  - A blocked merge a person rejected says that the reject is provisional: "If its check passes
    later, the accept it carries still merges it." Its accept's Log row reads "Merge accepted:
    PostgreSQL into Postgres. A check blocked it, and it was rejected".
  - A decided item's diff is headed "What it would change now", since it is worked out against
    today's beliefs.
  - The Accept hint shows only where there is an Accept.
- **The queue says what it asks.**
  - A contested row reads "Ledger's type: Table or Concept?", with at most two values and then "or
    N more".
  - A contested type definition reads "Service's definition: which meaning?". Its key is its
    workspace, axis and name, so two workspaces' Service are two items.
  - A proposal's tag says only what its title does not, Blocked, so "Merge" is said once.
- **One count, the items under Needs you.**
  - The pill reads "Review" when nothing waits, and "1 needs you" or "3 need you (2 contested)"
    when something does. The parenthesis counts contested beliefs, types and definitions both.
  - The segment reads "Needs you 3".
  - The tray reads "Review - 3 need you..." across this Mac's workspaces, from the proposal list and
    the curation report, read once a minute. Its item opens Review at All workspaces, the scope its
    count is of. When the curation read fails and the proposal list answers, it says what it
    counted: "Review - 2 proposals wait..." (P5).
  - Suggestions and Health keep their counts, drawn muted, so only Needs you's count asks.
  - A contested type definition is a Needs you item, settled with the block the rail draws. The
    rail keeps its own act, as the inspector does (inspector.md D1).
- **The keys act from the first press.**
  - Opening Review from the pill, Cmd+2 or the tray puts focus on the selected row.
  - A contested item has Skip, also as J, and K steps back. Its Confirm buttons, in the order shown,
    are 1 to 9; a value with no Confirm has no number, and a tenth has none. A number opens the ask
    as a click does, and the ask's Confirm, which takes focus, is pressed with Enter or Space as any
    button is.
  - A suspected secret has Skip too.
- **Suggestions and Health are queues.** Each suggestion or signal is an item in the list, its detail
  on the right with its act:
  - "Possible duplicate: PostgreSQL ~ Postgres" with Propose;
  - "Overloaded context: 14 entities" with Name this group;
  - "Unlinked: Session cookie" with Show on the map.
  J and K step through them. At All, an empty Needs you reads "Nothing waits for you in any
  workspace."
- **The map preview has a way back.**
  - "Show on the map" frames every entity the item names.
  - A banner over the map says "Preview: Merge PostgreSQL into Postgres", with "Back to Review" and
    "Done".
  - "Back to Review", Esc and Cmd+[ return to the item, closing the sheet the preview opened. "Done"
    ends the preview, and a click on the empty map keeps it.
- **The Log's decision rows say what was decided**, in the Log's full ink: "Merge accepted:
  PostgreSQL into Postgres", as now, and Claim confirmation accepted: "The ledger is a Postgres
  table..." (Ledger, Nightly export).

**What this step does not do.**
- It does not keep a decided item's diff as it was at the decision: that needs the fold at the
  decision's base, and the heading says the diff is today's instead.
- It does not fold a console act's two Log rows into one (above).
- It does not let a person keep the other name of a merge (D19) or open a merge the detectors missed
  (step 51).
- It does not change the who-line's grammar, which step 46 does.

### 29.4 Step 43: the map keeps the person's place

**What was found.** Measured on the review's demo store, and read in the code:
- **Follow takes the camera from the person.** With Ledger selected and its sheet open, one agent
  search in infra moved the camera from zoom 1.10 to 1.44 and put Ledger behind the filters panel.
  The sheet still described Ledger. Esc afterwards left the camera where follow had put it. Follow
  frames every read and write in the scope on screen, with no regard for a selection or the person's
  own last move, and its switch is inside View, out of sight.
- **The way back loses the place.** "< Graph", Esc and Cmd+[ on Settings and Setup, and Cmd+1 and
  Cmd+2 from them, go to the graph's bare address. The scope, the selection and the camera are gone,
  and with an empty default the page opens on All.
- **A selection has no history.** Walking Ledger, Postgres, Backups through the sheet's relation
  chips leaves no trail back. Esc clears the selection and keeps the zoom it brought. A node dragged by
  accident stays where it was dropped, in this browser, with no reset.
- **Only the scope has an address.** A selected entity cannot be kept across a reload or returned to.
- **A selection that leaves vanishes.** When a merge folds the selected entity, or a sync moves it out
  of the scope, the sheet closes with no word. U20 asks that the first be followed to what it became
  and the second be said; neither is built.
- **Cmd+[ and Cmd+] already act on the Graph page,** by accident: the single keys `[` and `]` toggle
  the panels without looking at the modifiers.

**Decisions taken.** The author took the recommendation on both, on 2026-10-21:
- **D6:** follow pauses while the person is using the map - a node selected, the List or a popover
  open, or within 20 seconds of their own pan, zoom or drag. Activity meanwhile is a quiet notice with
  Show.
- **D7:** a selection history, on Cmd+[ and Cmd+] on the Graph page, and the selected entity's id in
  the address. Registering the `supragnosis://` scheme waits (R1-07).

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U20 | "follow never changes the scope, the selection or the open sheet - it frames activity in the scope on screen" | contradicts, decided by the author (D6): it frames activity in the scope on screen while the person is not using the map, and names it in a notice with Show while they are. The scope, the selection and the sheet stay the person's either way |
| U20, its last clause | A selection a merge folds is followed to what it became; one that leaves the scope closes its sheet with a line that says so | repays - neither was built (R1-15). The review suggested keeping the sheet open over the line; U20 says the sheet closes, and it does |
| WCAG 3.2.5, change on request | A change of context comes from the person | repays - follow took the viewport while the person read it; the camera now waits for them |
| Section 6, the camera toolbar and List | "List, view, zoom in, zoom out, fit"; "List, the first button of the camera toolbar" | contradicts, decided by the author (D6, which puts the toggle where it acts): Follow leads the toolbar and leaves View; both sentences say so |
| Section 6, the first view | The viewer opens on the scope the address names, or else on the node default | agrees - the way back passes the address it left, so no other memory of the scope is needed; the review's tab memory (R1-05) is left out |
| U26 | On every page but the Graph the way back is "< Graph", which Esc and Cmd+[ also take | agrees, and sharpened: the way back goes to the address the graph was left at. On the Graph page Cmd+[ and Cmd+] step through the selection; the Review map preview's way back (Section 29.3) comes first while its bar shows, and the single keys `[` and `]` act only without a modifier, which they did not |
| U19 | A single key that decides acts only in its region, and the view's single keys can be turned off | agrees - Cmd+[ and Cmd+] are not single keys; `[` and `]` stay single keys, now only without a modifier |
| U17 and Section 9 | No identifier in a line read first | agrees - an address naming an entity the scope does not hold says "The linked entity is not in billing", with no id |
| settings-page.md Section 3.0 | "The page goes back to the graph" | agrees - it goes back to the graph the person left; the sentence says so |
| settings-page.md Section 4 | "A page from another machine must not be able to point this Mac's AI apps at a different server", nor, by the same reasoning, the app's pages anywhere | agrees, on a condition the step keeps: `?from=` is followed only when it is the viewer's own address (`viz://localhost/`), on the app's pages and in the shell alike; anything else goes to the bare graph, so the way back cannot be made a redirect |
| compatibility.md Section 8 | The shell's couplings with the page the daemon serves are listed | agrees - Section 8 gains the address read: the shell's script in the viewer page reads the page's address when it leaves for Settings, and the app reads the window's address when its menu or tray does |
| Section 5, the remembered layout | Positions are a convenience kept per workspace on the local console; the read tier remembers nothing | agrees - "Reset layout" forgets this scope's positions and lays it out afresh; on the read tier, which keeps none, it only lays out afresh |
| P17 and remote-viewer.md Section 3.4 | A reader is told nothing outside their grants | agrees - an entity id in the address selects only a node the page's graph holds |
| P3 | Nothing is destroyed | agrees - every act here moves the view; nothing reaches the log |
| P21 | No new tool without a repeated intent | agrees - nothing changes on the MCP surface or in the event stream |

No row needs a principle changed. Two rows contradict sentences of this document, and the author's D6
decided both.

**Behaviour.**
- **The person's move** is any pan, zoom or drag they make: a wheel or pinch on the map, a drag of the
  map or a node, the minimap, and the zoom and Fit buttons. The camera moves follow, a selection or a
  search make are not the person's move. *(Revised by step 50a: the keys =, +, - and 0 are the
  person's move, and Follow waits while the Keys dialog is open, Section 29.12.)*
- **Follow waits while the map is in use**: while a node is selected; while the List, the View
  popover, the search's results or an observation card is open; while the Review page is open; and
  for 20 seconds after the person's move. *(Revised by step 49: and while a working set is pinned,
  Section 29.11.)*
  - Activity in the scope on screen meanwhile - a write, a search, a read of an entity, a traversal -
    is one notice, `info`, saying the latest in words, always as what an agent did, so it is never
    taken for the notice of a write elsewhere ("New in infra", whose Show changes the scope): the
    sentence the map's passing row says for it (Section 29.6) - "An agent wrote 3 entities in
    billing", "An agent searched "ledger" - 8 found", "An agent looked up Ledger", "An agent followed
    Ledger's relations - 5 reached". Show frames what that latest activity touched. Later activity
    rewrites the notice in place, with a count: "An agent looked up Ledger (and 2 more)". It stays
    until it is used or dismissed, as U42 has it. *(Revised by step 45: the notice had words of its
    own - "read", "traversed from" - beside the row's.)*
  - A sync that lands knowledge in the scope meanwhile does not refit the map.
- **Follow leads the camera toolbar**, with Lucide's `locate`. While on it is pressed
  (`aria-pressed`); while on and waiting it is named "Follow paused"; while off it shows
  `locate-off`. A press turns it on or off, waiting or not. It is remembered as View's choices are,
  and a hub starts with it off, as now.
- **The address holds the place.** The viewer keeps `?workspace=` and `?entity=<id>` with
  `replaceState` as the scope and the selection change. A page opened on an entity selects it once the
  graph holds it, and says "The linked entity is not in billing" when the graph does not.
- **The way back goes back to it.**
  - Leaving the viewer for Settings - the gear, the sync pill, the menu bar's Settings or the tray's -
    or for Setup carries the viewer's address in `?from=`, its `#review` dropped.
  - Settings keeps `?from=` through its own sections and hands it to Setup ("Open Setup"), and the
    menu bar and the tray pass the current page's `?from=` on when they turn one app page into the
    other.
  - "< Graph", Esc and Cmd+[ on those pages, the menu bar's Graph (Cmd+1) and the tray's Open Graph
    go to it; the menu bar's Review (Cmd+2) goes to it with `#review`. Setup's "Finish later" goes to
    it too; Setup's own "Open the graph" on a workspace goes to that workspace.
  - When the person switches the active server in Settings, the way back becomes the bare graph,
    since the address named the other server's knowledge.
  - Only the viewer's own address is followed (the S1 row above).
- **A selection has a history** for as long as the page is open, in this scope. Every selection - from
  the map, the sheet's relations, the List, search, a card or Review - joins it.
  - The sheet's header has Back and Forward (`chevron-left` and `chevron-right`, "Back (Cmd+[)" and
    "Forward (Cmd+])"), and Cmd+[ and Cmd+] on the Graph page take them. With nothing selected,
    Cmd+[ reopens the last selection.
  - An entry merged away is followed to what it became; one no longer in the scope is skipped.
    Changing the scope starts a new history.
  - In a browser that keeps Cmd+[ for its own Back, the sheet's buttons do it while a node is
    selected.
- **Esc gives the camera back.** Esc, or the sheet's close, returns the camera to where it was before
  the first selection of the walk - the walk being every selection since nothing was selected - unless
  the person made a move since that selection. A click on the empty map clears the selection and
  leaves the camera, since the person is moving it. Esc on a map preview opened from Review goes back
  to Review (Section 29.3) and leaves the camera, which that page hides.
- **"Reset layout"** in View forgets this scope's remembered positions and lays it out afresh, in the
  arrangement on screen (by type, if Arrange by type is on).
- **A selection that leaves is followed or named.** When the selected entity is merged away, the
  selection moves to what it became and a notice says "PostgreSQL is now Postgres". When it leaves the
  scope otherwise, the sheet closes and a notice says "PostgreSQL is no longer in billing".

**What this step does not do.**
- It does not undo a single drag. In the app the Edit menu's Undo owns Cmd+Z, and a drag's place lasts
  only until its neighbours settle; "Reset layout" is the way back.
- It does not register `supragnosis://` with the system, offer "Copy link" (an app address opens
  nowhere yet), or give the Review item, the observation card or the List an address (R1-07).
- It does not reopen Settings on its last pane, or say "< Settings" on Setup opened from Settings.
  *(Step 50c reopens the last pane, Section 29.14.)*

### 29.5 Step 44: the app's pages hold focus, and a failure says so

**What was found.** Measured in the app's pages and on the shell's starting page:
- **Setup and Settings throw focus away.** Setup draws itself again on its 15-second check, 1.5
  seconds after any event and on every streamed line; Settings on every window focus, every 2 seconds
  while the daemon is unsettled, and on every download line. Each draw replaces the page, and
  keyboard focus lands on the page's body. A probe focused "Details" and then Restart; after the next
  draw, focus was on the body both times. VoiceOver starts again from the top.
- **The page shown when the daemon cannot start keeps saying "starting".** Its heading is "starting
  the supragnosis daemon..." when it starts and when it failed; the line under it is the shell's raw
  status ("daemon: FAILED - the spawned daemon exited immediately (...): ... - full log: ..."); and
  the page reloads every second, so a Tab to "Open This Mac" is lost a second later. The whole page
  is a drag region, so the error cannot be selected either. There is no Restart, no copy, and the
  log path must be retyped - and it names the app's own log even when a login job runs the daemon
  and writes another.
- **Commands and paths cannot be selected.** The shell's stylesheet makes the app's pages
  unselectable but for inputs and `.selectable`, so the command Setup gives for an AI app and Sync's
  `~/.supragnosis/supragnosis.toml` must be retyped.
- **Setup's foot puts two copy buttons beside the page's acts** before anything has run, when the
  log is "(nothing ran)" four times.
- **Restarting a failed daemon can spawn one beside a login job.** This Mac's Restart, from the
  shell's Failed state, starts the daemon the way the app's launch does, and spawns a child whenever
  the socket is silent - also when a login job owns the daemon and is still bringing it back.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| WCAG 2.4.3, 3.2.1 and 3.2.5 | Focus moves where the person moves it; nothing changes context unasked | repays - a draw keeps focus on the control that had it, a read that changed nothing draws nothing, and the starting page no longer reloads under the person. The shell turns that page into the graph when the daemon answers: the page stands in for the graph at the graph's own address, and the graph arriving is the content the person opened, not a change of context |
| WCAG 2.2.1, timing adjustable | Content that changes on a timer can be stopped | repays - the starting page has no timer of its own |
| U19 | Every control exposes its state; a dialog holds focus | revised: it gains "a page that draws itself again keeps the control that had focus", the rule R4-01 asked for, guarded by the app's `the_app_pages_keep_focus_across_a_draw` |
| design-system.md Section 6a | A control both pages offer is built once in the app's shared script; Set up is "the first control after the plan in tab order"; "Copy log and Copy diagnostics sit at the left" | agrees with the first two - the focus keeping is `kit.js`'s, and Set up stays first in tab order while it is drawn at the trailing edge; revised in the third: one Copy Diagnostics, once a step has run or failed in this visit, or the check could not be read |
| onboarding.md Section 6 | "Copy log" copies every drawer; "Copy diagnostics" copies the two reads | revised: one Copy Diagnostics copies the drawers, the two reads, the shell's status and the daemon log's last lines |
| onboarding.md O4 | Setup's own acts are few and listed | agrees - copying changes nothing, and nothing is added to them |
| settings-page.md Section 3.3, the starting page | "says why and offers one button": Rebuild index, or Open This Mac, "where Restart is"; it holds no command of its own (S1) | revised: for a failure other than a store the daemon cannot open, it offers Restart, Diagnostics and Open This Mac, each a link to Settings as Rebuild index is, and still no command of its own |
| settings-page.md Section 4 (S1) | A command acts only for the app's own pages | agrees - Settings starts a restart from `act=restart` only while the daemon does not answer and the shell is not starting it, which a page a daemon serves cannot cause (a viewer page exists only while its daemon answers); with a remote profile active the command refuses (S6). The new command that shows a file in Finder checks its caller and shows only one of a fixed pair, the daemon's log and `supragnosis.toml` |
| daemon-lifecycle.md Section 6 | "the shell spawns none of its own beside a login job" | repays - a Restart of a failed or silent daemon goes through `supragnosis restart` when `status --json` names any manager, as a Restart of an attached daemon already does, and spawns only when none does |
| P24 | Degrade, and be loud in the place that is read | repays - the page the window shows says "could not start" and why, not "starting" |
| P17 and onboarding.md O6 | Nothing leaves the machine; no secret is copied into a report | agrees - Copy Diagnostics puts the CLI's two reads, the shell's status and the log's last 40 lines on this Mac's clipboard, where they stay until the person pastes them. The log's lines can name workspaces, principals and peers; none carries the token, which the daemon logs only by its path |
| U7 | Text meets its contrast floor | agrees - the starting page keeps its colors, which are the tokens' |

No row needs a principle changed, and no decision of the author's is needed: the review asked none for
these findings.

**Behaviour.**
- **A draw keeps focus.**
  - Before Setup or Settings draws itself again, `kit.js` notes the focused control's key - the
    nearest `data-k` or `id` at or above it, its kind and its name - and an input's selection. After
    the draw it focuses the control with that key; one whose name changed with the draw ("Restart" to
    "Restarting...") is found by the rest of its key when only one control matches.
  - The keys: a row of an AI app is `app:<id>` and its capture row `capture:<id>`, a server
    `server:<name>`, a hub `hub:<url>`, a peer `peer:<id>`, a step's card on Setup `step:<step>`,
    This Mac's daemon card `daemon` and its model card `model`.
  - A line streamed into a step's log is added to its open drawer in place, with no draw, so a screen
    reader is not sent back to the control; a line that changes what a card says - its state or its
    sentence - draws, and the foot's progress words with it.
  - Three reads draw only when what they read changed: Setup's 15-second check, and Settings' read
    on window focus and its 2-second watch. Every other draw - after an act, on a hash change - happens
    as now.
- **The starting page says which of three things is true,** from the shell's state, and has no timer:
  - **Starting**, for the first 10 seconds of a start the shell made: "Starting Supragnosis..." (the name capitalized since step 45) with a
    small spinner, still under reduced motion, and nothing to press.
  - **Still starting**, after that, and while a child the app spawned is alive and not yet
    answering (the app stops waiting on it at 10 seconds, and a cold start can take longer): the same
    heading, "Still starting.", the daemon's log path in a read-only field, and Open This Mac.
  - **Could not start**, when the shell's state is Failed, when the child the app spawned has
    exited, or when a daemon the app attached to has gone silent: the heading "Supragnosis could not
    start"; the cause in one sentence - the shell's reason up to " - full log:", "The daemon exited."
    or "The daemon stopped answering."; Restart, the
    default (it has focus when the page opens), Diagnostics and Open This Mac, each a link to Settings
    > This Mac (`?act=restart`, `?act=diagnostics`); the log's path in a read-only field, selectable
    and copyable; and Show in Finder (`?act=reveal-log`). *(Revised by step 45: a daemon the app
    attached to that went silent is Section 9's "Stopped" - the heading "Supragnosis stopped", "It
    stopped answering, and nothing is starting it.", with the same acts.)*
  - **The log** is the one written last of `app-daemon.err.log` (the app's child) and
    `supragnosis.err.log` (a login job, and `supragnosis start`).
  - A store the daemon cannot open keeps its page, with Rebuild index.
  - **The shell changes the page**, not the page itself. It sets a flag when it serves the starting
    page and clears it when it serves the graph's own page, both at the viewer's address. While the
    flag is set it checks the socket and its state every second, and reloads the window when the
    daemon answers - the graph then - or when what the page should say changed. A graph whose daemon
    goes silent for a moment is not swapped for this page.
- **Settings acts on what the starting page asks,** and drops the parameter once read, as it drops
  `act=rebuild`:
  - `act=restart` restarts the daemon once, only while it does not answer and the shell is not
    starting it; otherwise This Mac says the daemon's state as usual, with a notice that names it:
    "Not restarted: the daemon answers" or "Not restarted: the daemon is still starting".
  - `act=diagnostics` puts focus on This Mac's Copy Diagnostics, so the copy is the person's press.
  - `act=reveal-log` shows the daemon's log in Finder.
- **This Mac gains Copy Diagnostics and the daemon log's path,** selectable, with Show in Finder.
- **Commands and paths are selectable,** in `code` and `.mono` on both pages. A command in Setup's
  sentences - a `code` span that begins with `supragnosis ` - has a Copy beside it, and Sync's
  sentence naming `supragnosis.toml` gets Show in Finder.
- **Setup's foot** draws its acts at the trailing edge, the default rightmost and first in tab order,
  and one Copy Diagnostics at the leading edge, once a step has run or failed in this visit or the
  check could not be read.

**What this step does not do.**
- It does not give the page shown while a remote server is active (`remote_html`) the same anatomy.
- It does not read a systemd unit's journal or a legacy `brew services` job's log for the log field
  and Diagnostics; under those, they name the two files above.
- It does not change the tray's lines or icon, or the Quit item's words (D14, D15: step 50; the line's
  words are step 45's).
- It does not give the daemon's states one word on every surface (R4-11, step 45), build the AI app's
  row once for both pages (R4-08, step 45), or rename Settings' sidebar, reopen its last pane and keep
  the window's size (R4-14, R4-16, step 50). *(Step 50c removes the sidebar's heading rather than
  renaming it, Section 29.14.)*

### 29.6 Step 45: one concept, one name

**What was found.** Measured in the viewer, the app's pages, the tray and the code that writes them:
- **"Node" names two things, and the computer has many names.** The Log's who-line says "this node"
  beside a rail headed "Node types"; the trust note says "no node is ringed"; the Peers switch moves
  dots "to the nodes they touch". The same Mac is "this node" in the Log and the sync pill, "localhost
  (this node)" in the inspector, "localhost" on Review, "This Mac" in Settings, a heading "This node"
  over "Hubs this Mac syncs to" on Sync, and "daemon" in the tray.
- **Entities and relations go by node, edge, link, kind and item.** The rail says "Node types" and
  "Edge types" and, two rows lower in Definitions, "entity types" and "relation types"; View's switch
  is "Edges - Relations between nodes"; search says "linked to a match" and "No item in billing
  contains"; Review's diff prints the field ids "kind" and "edge"; the map's tooltip says "degree".
- **Section 9's renames were not all applied.** "Needs switching" (Settings' AI apps), "Healthy -
  nothing owed" (This Mac's store) and "settling layout..." (the map's loader) are still drawn.
- **Headings change case from panel to panel:** sentence case in the rail, lowercase in the
  inspector ("relations (3)", "evidence (2)") and the observation card ("provenance (2)", "derived
  from"), and "belief decision (why this value)" as a toggle.
- **The map's passing rows speak in tool calls:** "observe +2 ent, +1 rel ws infra", "search "q" ->
  8 hits hybrid", "get_entity Ledger found", "sync push billing <-> 4a1b2c3d4e5f6071 (3)"; the status
  bar names a session by its id, "session localhost-1792... / 8 used". Closing the Log leaves no strip
  in its place until the next poll, up to 2.5 seconds later.
- **The daemon's state has a word per surface.** The same healthy state is "daemon 0.8.2 - login
  item" in the tray, "Answering" on Settings' Server pane, "Running" on This Mac and "Done" under
  "Keep it running" on Setup; drift is "daemon 0.8.1 running, 0.8.2 installed - restart it in
  Settings" in the tray and "Update ready - restart to load 0.8.2" in Settings; a stopped daemon is
  "Not running"; the tray says "daemon: FAILED - ..." and "daemon: CONFLICT - 2 managers claim it".
- **The AI app's row is built twice and the two disagree.** Settings and Setup each draw their own
  row: "Needs switching" against "Needs you", the capture option "Save the why behind each commit"
  against "After a commit, ask for the decision behind it", and Restart Claude Desktop and Open
  Cursor only on Setup. Settings' Connect says the next step twice, in its notice and in a callout in
  the row.
- **This Mac speaks the CLI's words beside its own controls.** The model row shows the CLI's note
  ("semantic retrieval is off: the embedding model ... - run `supragnosis embed fetch` once") beside
  a Download button; Diagnostics is described as "setup --check, status --json, ..."; with drift, a
  conflict or a remote server the "Restart the daemon" row stays with nothing to press.
- **The product is "supragnosis" and "Supragnosis" on one page** ("Set up supragnosis", "Supragnosis
  is set up"), and Setup's window is titled "Welcome - supragnosis" where every other place calls it
  Setup.
- **The browser's Settings dialog reads like a debug panel:** its title "settings", its rows
  "package", "version", "licence" and "source", and the role "spoke", where the app's page says
  License and Spoke. Definitions and Review count sources as "3 src".

**Decisions.** The author approved the review's recommendations: the computer serving the page is
"this Mac" in the app on a Mac and "this machine" elsewhere (D2), which replaces Section 9's "this
node"; and the product is "Supragnosis" in the interface's sentences and "supragnosis" in a command,
a path, the CLI's own output and code (D17).

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U17 | No line read first spells a system identifier; a place keeps one name wherever it is reached | repays - the activity rows, the status bar's session, Review's diff fields and the tray's lines say what happened in words; revised: it gains "a state keeps one word on every surface that shows it, from Section 9's state words, and no retired word of Section 9 reaches a page", guarded by `no_retired_word_reaches_a_page`, `the_maps_activity_speaks_in_sentences`, `the_daemon_has_one_word_per_state` and `the_ai_app_row_is_built_once` |
| Section 9, "Words are the reader's" | A host is named by its id, and "this node" only when the id is the page's own | revised by D2: the page's own machine is "this Mac" in the app on a Mac and "this machine" elsewhere, still only for the id `/api/surface` reports; another machine stays named by its id |
| Section 9, "One place, one name" | The table's left column is replaced by its right | repays - "Needs switching", "Healthy - nothing owed" and "settling layout..." go, and a test now fails when a retired word returns |
| P8 | One meaning per concept | agrees - the screen uses the ontology's own three words, entity, relation and type; "node" no longer names both an entity and a machine |
| P2 | Provenance is first-class: who wrote it is said | agrees - the who-line keeps tier, who and when; only the page's own machine changes its word |
| design-system.md Section 6a | A control both pages offer "is built once in the app's shared script and drawn by both"; Setup's heading "Set up supragnosis" | repays the first - the AI app's row, its pill words, the capture option's label and the helper are `kit.js`'s; each page passes what it knows; revised in the second by D17: "Set up Supragnosis" |
| design-system.md Section 29.4 | Follow's notice says the latest activity: "An agent read Ledger", "traversed from Ledger" | revised: it says the passing row's sentence, built once |
| settings-page.md Section 3.2 | "The CLI's next step stays beside the app it concerns" | agrees - it stays in the row, said once, with the helper that does it where there is one; the notice names only what was done |
| settings-page.md Section 3.3 | The daemon's card says its version and who runs it (login item, this app, Homebrew, `supragnosis start`); "Update ready - restart to load 0.7.0" | revised in who's words - starts at login, while this app is open, Homebrew's service, `supragnosis start`; agrees in drift's |
| settings-page.md Section 3.4 | Sync shows "This node: its id and its role" | revised by D2: "This Mac" |
| settings-page.md Section 5 | The tray's line says what needs doing and points to Settings | agrees - "Update ready - 0.8.1 running, 0.8.2 installed - restart in Settings", "Two managers run Supragnosis - open Settings" |
| daemon-lifecycle.md L3 | The tray shows the running and the installed versions when they differ | agrees - the drift line keeps both |
| remote-viewer.md Sections 3.5 and 6 | The page reads `/api/surface`; on a remote profile the app's window shows the server's read tier | agrees - the machine's word is said only where `/api/surface` reports the local console; a read tier's page says "this server" |
| shell-init.js, its duties | The viewer stays browser-neutral; the shell's concerns stay in the shell's script | agrees - the word "this Mac" is the shell's to give, a fifth duty, and the viewer falls back to "this machine" |
| onboarding.md Section 4.1 | The first step is "Keep it running" | revised: "Keep Supragnosis running" in the app and "Keep supragnosis running" in the CLI, by D17; the step's state and act are unchanged |
| settings-page.md Section 3.2 and design-system.md Section 6a | Claude Code's capture switch is "Save the why behind each commit" | agrees - Setup takes the same label |
| WCAG 3.2.4 | One function, one name across pages | repays |
| U26 | The title bar names the place, never the product | agrees - the window's document titles become "Graph - Supragnosis", "Settings - Supragnosis" and "Setup - Supragnosis"; the bar itself is unchanged |

No row needs a principle changed; D2 and D17 are the author's decisions.

**Behaviour.**
- **The page's own machine** is "this Mac" in the app on a Mac and "this machine" in a browser and
  on other systems. The word is the app's shell's to give - its script sets it before the page loads,
  as a fifth duty, since the viewer stays browser-neutral - and the viewer says it only where
  `/api/surface` reports the local console; a read tier's page says "this server". It is said in the
  Log's who-line, the inspector's scope line ("All here: Agent, from this Mac"), the sync pill
  ("Syncing: this Mac +3, hub +2") and the Sync dialog's headings ("This Mac", "Hubs this Mac syncs
  to"). Settings' Sync heading is "This Mac". Another machine is named by its id, as now, with
  "machine" where a label names it. The keyword-only search note says "there is no meaning index
  here".
- **Entity, relation and type** are the only words for the graph's parts in a line read first:
  - the rail's headings "Entity types" and "Relation types", each with its count beside it;
  - View's switch "Relations", described "Lines between entities."; Group by type "A circle of
    entities for each type, with the relations between groups kept."; Peers "Other machines, as dots
    that move to the entities they touch."; *(Revised by step 47: the switch is "Arrange by type",
    "with the relations between types kept" - region is the cloud's one name, Section 29.9.)*
  - search's reason "related to a match", and "No entity in billing contains "ledger".";
  - the trust note "so no entity is ringed";
  - Review's diff rows "Type" and "Relation" for the fields `kind` and `edge`;
  - the map's tooltip "type", "relations" (the entity's count of relations) and "sources".
- **Headings are in sentence case, a count beside them in the muted count style** the rail already
  uses: the inspector's "Relations 3" and "Evidence 2"; the observation card's "Provenance 2",
  "Derived from", "Proposal", "Targets", "Entities" and "Relations"; and the toggle "Why this value".
- **The map's activity** is named "Activity" for a screen reader, and its rows are sentences with no
  tool name, abbreviation or mode id; who acted is "An agent" until the event names the app (D13,
  step 46b):
  - "An agent wrote 2 entities and 1 relation in infra" (a part with none is left out; "wrote an
    observation" when it linked nothing);
  - "An agent searched "kubernetes ingress" - 8 found";
  - "An agent looked up Ledger", or "... looked for Ledger - not found";
  - "An agent followed Ledger's relations - 4 reached";
  - a sync: "Sent 3 in billing to 4a1b2c3d", "Received 3 in billing from 4a1b2c3d", and on a hub
    "4a1b2c3d sent 3 to billing", "4a1b2c3d read 3 from billing"; a peer is its id's first eight
    characters or its server's host, whole in the row's tooltip, and a check-in with nothing moved
    writes no row.
  - Follow's notice (Section 29.4) says the same sentence, from the same function.
- **The status bar's footprint** reads "Agents used 8 entities": its session is the daemon's run, not
  one conversation, so it counts every agent since then. The tooltip says since when and the
  session's id. Closing the Log fetches the strip at once.
- **The map's loader** says "Arranging 168 entities", the count of what the layout settles.
- **The daemon has one word per state** (Section 9's state words), on the tray's line, Settings'
  Server and This Mac panes, Setup's first card and the starting page:
  - Starting; Running, with who keeps it - "starts at login", "while this app is open", Homebrew's
    service, `supragnosis start` or "started outside this app"; Stopped; Could not start, with the
    cause; Update ready - restart; Repairing - N items left; Two managers run it; and Not in use while
    a remote server is.
  - "Answering" and "Not answering" are said of a remote server only. Settings' Server pane says the
    daemon's word for This Mac.
  - The tray's line: "Running 0.8.2 - starts at login", "Starting...", "Could not start: <cause>",
    "Update ready - 0.8.1 running, 0.8.2 installed - restart in Settings", "Two managers run
    Supragnosis - open Settings". Its structure and icon are step 50's.
  - This Mac's store row: "Up to date", or "Repairing - 12 items left". About's daemon row says the
    version, "Not in use" or "Stopped".
  - No page reads a state out of these words: Settings is given the shell's state - starting, failed
    with its cause, spawned, attached - as a field of its own, and the tray's line only to show. A
    child the app spawned that has exited is settled to failed before any of them reads it, so all
    say "Could not start"; a daemon the app attached to that went silent is "Stopped" on all,
    the starting page's heading "Supragnosis stopped" included.
  - "Answering" stays on Settings' Server pane for a remote server only, and so do its two warnings,
    a refused credential and a server that does not answer.
- **The AI app's row is `kit.js`'s.** Both pages draw it from one function, which takes the app, its
  state in one vocabulary and what the page puts at its end - a checkbox on Setup's plan, a button on
  Settings:
  - one pill map: Connected, Not connected, Connecting, Needs you, Failed, Not installed, and
    Unavailable when its settings cannot be read or the CLI is too old;
  - one capture option, "Save the why behind each commit", with "After a commit, Claude Code is
    asked to record the decision behind it, if there was one." under it - a switch on Settings, a
    checkbox in Setup's plan;
  - the helper - Restart Claude Desktop, Open Cursor - on both pages after this visit connected the
    app, and Settings' Connect says its next step once, in the row; its notice says "Claude Desktop
    connected".
- **This Mac writes its own sentences.** The model row: "Search matches words until the model (224
  MB) is downloaded." - the CLI's note goes into Copy Diagnostics. Diagnostics: "Setup's check, the
  daemon's status, this app's state and the log's last lines, for a report. Nothing leaves this
  Mac." The "Restart the daemon" row is not drawn while the card above holds the section's act or a
  remote server is in use.
- **The product's name** is "Supragnosis" in every sentence of the viewer, the app's pages, the tray
  and the starting page - Setup's heading "Set up Supragnosis", Settings' blurbs ("The Supragnosis
  server this Mac's AI apps use"), the document titles "Graph - Supragnosis", "Review - Supragnosis",
  "Settings - Supragnosis", "Setup - Supragnosis" - and "supragnosis" in a command, a path, the
  CLI's own output and code.
- **Setup's sentences are the CLI's,** written once for the terminal and the app, so they say neither
  the machine's word nor the product's name: "AI apps here use the server "home", so the daemon here
  is not what they use"; "Cursor already has an entry setup did not write". A daemon that does not
  run is "Stopped" there too - "Stopped, and ...", and with a login job loaded "Stopped (last exit
  1): the login job is loaded, but its daemon does not run". The step's title is "Keep Supragnosis
  running" on the page and "Keep supragnosis running" in the terminal. Claude Code's capture option
  carries its line under the checkbox as under Settings' switch.
- **One act, one name on both pages:** replacing another app's entry is "Switch to the bridge" on
  Settings as on Setup, and a helper that fails says "Do it by hand" on both. A Connect that fails is
  an error, never a notice that it connected (U1).
- **The browser's Settings dialog** says "Settings", "Package", "Version", "License", "Source", and
  the role "Hub" or "Spoke", as the app's page does. A count of sources is "3 sources".
- **The Graph is the page; the map is its drawing.** Setup's "Open the Graph" names the page, as the
  tray's Open Graph and the title bar do; "on the map" keeps naming the drawing inside it.
- **Section 9's words become a tripwire.** Its table of retired words lists phrases as they were
  written; a test reads the viewer's markup and script, the app's pages and scripts, the shell's
  lines and Setup's sentences, comments and test code left out, and fails when one appears, case for
  case. Section 9 gains the rule that headings are in sentence case.

**What this step does not do.**
- It does not rename regions or their count ("10 regions", Health's "contexts"), which step 47 does
  with the map's legend (D3).
- It does not change the landing's words (R1-17's "viewer", "entry", "machine"), which step 48 does
  with its install line.
- It does not add the client's name to an observation's attestation (D13, step 46b), so an activity
  row says "An agent".
- It does not split the tray into two lines, change its icon or the Quit item (D14, D15, step 50), or
  rename Settings' sidebar group and reopen its last pane (R4-14, R4-16, step 50). *(Step 50c removes
  the heading rather than renaming it, Section 29.14.)*
- It does not change the first entry's words - Setup's "Waiting for an entry", the map's "new
  writes" - which step 48 settles with the landing's (R1-17).
- It does not give the browser's dialog and the app's Sync pane one source for their words (R1-11),
  or a case rule for buttons; a button keeps its words.

### 29.7 Step 46: evidence where a decision is made, and provenance to its end

**What was found.** Measured on the Review page, the inspector and the observation card:
- **Review decides without evidence.** Section 7 lists "the evidence, each observation with who said
  it and when" on a proposal's detail; the page shows the diff, a "checks pass" line in the muted
  italic of an empty state, and "Opened by localhost ... self-attested" - a mark every opened
  proposal carries, so it tells the reader nothing. A contested item's rows leave who and when blank:
  only the inspector fills them.
- **The merge diff does not say what happens to the entity that goes**: that PostgreSQL leaves the
  map and becomes an alias of Postgres, or that its observations count for Postgres from then on.
- **Checks show only what fails,** though the engine answers every check it ran; and none reads in
  words ("referential integrity", "t-box axis consistency").
- **A contested value cannot be traced to what said it.** "Two sources disagree" leads to rows that
  read alike, with nothing that quotes or opens the source, and two times in one minute read the same.
- **The who-line names the wrong actor.** It puts the principal in the writer's place - "Agent -
  alex" reads as an agent named alex, "Human confirmed - sam" as sam's confirmation - and the
  person's own console verdicts read "Agent - this machine". A row names whichever attestation sorts
  first by host, not the one that wrote it.
- **The card leaves out the reference and the confirmation.** `source_ref` - the commit or runbook
  the agent cited - is drawn nowhere. A card whose observation a person confirmed says "Human
  confirmed" over a provenance row that says Agent, with no who or when. Confidence reads "conf 0.9".
- **The card's ids lead nowhere.** Derived from lists hashes that cannot be opened; a proposal
  event's card prints "entity_merge", "merged" and an id, with no way to its Review item. Opening an
  observation the page has not loaded reads the scope's whole log, and fails outside the scope.
- **Entity and Review item do not link** (R1-06): the inspector does not say that a merge of this
  entity waits for a verdict, and a contested Review item cannot show its entity.
- **A relation's trust is never drawn,** and in the graph read it ignores gate events (step 41's
  finding, owed since): a relation stated by a confirmed source still reads Agent.
- **"Neither is right"** is the browser's own disclosure triangle, and the sentence it copies leaves
  out the workspace and the values it calls wrong.

**Decisions.** D13 - recording which AI app wrote an attestation - changes the store's format and
needs the sync wire and the read tier to keep it out; it is specified and built on its own as step
46b. Until then a writer is its machine, as now.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P2 | Provenance is first-class; "who" is a delegation chain, the acting host and the principal | repays - the who-line says the writer and the principal apart ("Agent - this Mac, for alex"), from the authoring attestation, and the card says who confirmed and when |
| P14 | An identifier stays dereferenceable | repays - Derived from opens its card, a proposal event's card opens its Review item, a cited reference that is a web address is a link, and an observation is read by its id |
| P18 | `source_ref` is the writer's own claim | agrees - it is drawn as written, a link only for `http` and `https`; a `surface:` marker is the console's, said as "this console", never as Cited |
| P23 and proposal-workflow.md Section 6 | Checks before canon; an informative trust profile beside the blocking checks | repays - the engine adds the trust profile as a non-blocking check, which the fold already ignores, and Review lists every check |
| resolution.md Section 3 | Every display reads the effective tier | repays - a relation's trust in the graph read takes the gate events of the observations that state it, and its mark is drawn; guarded by `a_confirmed_source_says_who_confirmed_it_and_its_relations_take_its_tier` |
| design-system.md Section 7 and U4 | Review shows the item's evidence; a decision control appears only with the diff and checks on screen | repays - Evidence under Checks, every check listed, the diff led by what happens to identity; guarded by `the_review_page_shows_its_evidence` |
| U17 | A who-line says tier, who and when | revised: one grammar - tier, writer, for whom, when - for every row and card, the writer from the authoring attestation, a console verdict written here "you at this console"; guarded by `a_who_line_names_the_writer_apart_from_whom_it_wrote_for` |
| U18 | Settling names each source | repays - each value carries its source's first sentence, opening its card, and its who-line, on the inspector and on Review alike |
| proposal-workflow.md Section 14.4 | A merged entity's old id forwards to the new | agrees - the diff says so in words before the verdict |
| remote-viewer.md Section 3.2 | `/api/observations` and `/api/explain` are read on the read tier under the workspace's grants | revised: `/api/observations?id=` answers one observation, its workspace granted and servable, an id outside the grants answered as an unknown one (remote-viewer.md Section 3.4) |
| inspector.md, the card | The card is where an id resolves | repays - Cited first, the confirmation's who and when, Open in Review |
| P16 | A read answers alike on every node | agrees - the edge's tier is a max over a set, and the confirmation is the last event in the gate fold's verdict order (resolution.md R5) |

No row needs a principle changed.

**Behaviour.**
- **One who-line grammar** on every row and card: tier - writer, for whom - time.
  - The writer comes from the authoring attestation (the engine's rule for "who authored this
    act"), which the observation's answer names. It was written here when it carries no sync stamp,
    or its stamp's origin is this node - the id the Sync read names - and is then this machine as
    Section 9 says it, on the local console only. Any other origin is named by its id's first eight
    characters. A host name is not a machine's identity - every unconfigured node is `localhost` - so
    it never decides "here".
  - "for alex" when that attestation names a principal: "Agent - this Mac, for alex - 11:27 AM".
  - A verdict cast at this console and written here (`surface:console`) is "Agent - this console -
    11:38 AM": the row's tier is the verdict's own, and its sentence says what it granted. A merge or
    split opened at this console reads the same way (Section 29.17). *(Revised by step 51c.)*
    One cast at another machine's console is "Agent - the console at 4a1b2c3d - ...". The console
    names where the act was cast, not who cast it: any program of the user can reach it (resolution.md
    Section 6). *(Revised 2026-10-23: they read "you at this console" and "a person at 4a1b2c3d".)*
  - Where two values of one contested field were written in the same minute, both their who-lines
    show seconds; the Log keeps its time once per minute (Section 9).
- **The card** leads with what grounds it and resolves every id:
  - "Cited: git:4f2a9c1" first, a link when it is an `http` or `https` address; a `surface:` marker
    is not a citation and is said in the who-line;
  - each attestation in the who-line grammar, with "the agent's confidence: 0.9";
  - when a merged gate event set the effective tier, the act that did, from the engine: "Human
    confirmed from this console, 11:38 AM - Open in Review", or "Lowered to Agent by ..." for a
    demotion - each observation's answer names the gate event that last moved it in the gate fold's
    verdict order (resolution.md R5): its proposal, and its verdict's time, surface and sync origin,
    read by the rule above;
  - Derived from, each id a button that opens that observation's card;
  - for a proposal event, its kind and state in words ("Merge - accepted"), the rationale in quotes,
    and Open in Review; the stored text in a collapsed footer;
  - the date once, at the top.
- **An observation is read by its id**: `/api/observations?id=<id>` answers that one, in any workspace
  the surface may read, so a card opens without reading the scope's log.
- **Each contested value names its source,** on the inspector and on Review alike: under the value,
  its source's first sentence in quotes - a button that opens the card - and its who-line. A Review
  contested item gains Show on the map.
- **Review's proposal detail** reads top to bottom: what changes, checks, evidence, who asked.
  - **What changes** leads with identity for a merge, worked out by the page from the proposal and
    each entity's support: "PostgreSQL leaves the map; its name becomes an alias of Postgres", "1
    observation counts for Postgres from then on". When the kept entity has a contested field in the
    Needs-you list, "Postgres's type stays contested - also under Needs you", a link to that item.
    Then the belief rows, in Section 29.6's words.
  - **Checks** lists every check, passed and failed, each with a Lucide check or alert. A check
    reads in words keyed by the proposal's kind, the check's name and whether it passed - for a merge
    "All 3 entities are in its workspace" or "Not every entity it names is in its workspace", "The
    targets are at least two entities", "It names the entity to keep"; for a type change "Types are
    consistent"; for an un-merge "It names a merge to undo"; for a gate act "Its observations are in
    its workspace", "It says the tier it asks for" - and one the page has no words for as the engine
    names it, with the engine's detail when it fails. A failing blocking one keeps its Blocked block.
    *(Revised by step 51a: a failed check read its passing words and then the engine's detail, so a
    blocked merge read "All 2 entities are in this log: 1 target entit(ies) are not asserted".)* The trust profile, a check that blocks
    nothing, reads "Sources: 3 Agent, 1 Human confirmed".
  - **Evidence** lists the observations the decision rests on in the Log's row anatomy, each opening
    its card: for a merge, one column per entity - "PostgreSQL 1", "Postgres 2"; for a gate act, the
    source observations it moves.
  - **Who asked** is the opening event's who-line, opening its card. "self-attested" is not drawn.
- **The inspector says what waits for the person**: under its header, one line per Needs-you item
  that names the entity - "Waits for you: Merge PostgreSQL into Postgres" - with Open in Review.
- **A relation carries its trust.** The graph read gives each relation - by its endpoints after
  merges and its normalized type - the highest effective tier among the observations that state it,
  gate events applied; its confidence stays the latest row's. The inspector's relation rows draw the
  map's ring for that tier beside each relation, as a log row's mark.
- **"Neither is right"** opens with Lucide's chevron, and copies "In Supragnosis workspace billing,
  Ledger's type is recorded as Table and as Concept, and both are wrong. Record the correct type of
  Ledger in billing." (the product's name is in the sentence an agent reads, by D17).

**What this step does not do.**
- It does not record which AI app wrote an attestation, name it in a who-line or an activity row, or
  keep it off the wire and the read tier (D13: step 46b).
- It does not let a person open a merge from the console ("Same as...", D10) or offer "Keep
  PostgreSQL instead" (D19); step 51 does both.
- It does not settle a contested name the way a type is settled (R3-17's question, not decided).
- It does not mark a Review item "Waiting for a correction" after Neither is right, or notice the
  correction's arrival.
- It does not change the inspector's scope line ("All here: Agent, from this Mac"), which still names
  an origin by its host as step 45 left it; graph nodes carry hosts, not sync origins.

### 29.8 Step 46b: the app that wrote it

**What was found.** Two AI apps on one Mac read alike everywhere a writer is named: "Agent - this
Mac - 11:27 AM" for Claude Code and for Cursor, "An agent wrote 2 entities" on the map. The daemon
knows which app each MCP session is - `initialize` names it, and first contact keeps it in memory
(onboarding.md Section 5) - but nothing it writes records it, so an attestation cannot say which app
made it (R3-02).

**Decision.** The author approved D13: the MCP client's name is recorded with what the session
writes.

**How it is kept, and why there.** Step 46's review found that a field on the attestation would ride
the sync wire unsigned (the wire carries the attestation as serialized, and a receiver keeps what
arrives), would raise the store's `min_reader` (compatibility.md Section 3.7: an older writer's
absorb strips it), and would need a merge rule for an echoed attestation. The name is therefore not
part of the attestation. It is a row of a table of its own in this node's store, `attestation_app`:
the attestation's key -> the app's name. The key is the observation's id and a hash of the
attestation with its sync stamp left out, computed by one core function that names every field, so
a field added later must be decided for it (P14), and a stamp added at first export does not move
it. Nothing that is signed, synced, ordered, folded or compared reads the table.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P2 | Provenance is first-class; "who" is a delegation chain - its own example is `ashon -> claude-code@macbook`; a relayed attestation's authenticity is the origin's signature | repays on this node - the acting app is recorded beside the host and the principal; owed toward peers, which would need the name signed and a wire version (federation.md, a new row); agrees in the signature - the record never travels, so no relay can forge it |
| P2, on self-declared identity | An agent's self-declared identity cannot be verified | agrees - the name is the app's own word in `initialize`, shown as that, read by no trust rule, check or gate |
| P14 | Every attestation-distinguishing field is decided where it is enumerated | agrees - the key hashes every field but the stamp through one destructuring function, as `provenance_order` and the signing bytes do |
| P17 | Nothing leaves the machine unasked | agrees - sync never sends the table; the read tier and MCP never ask for it |
| P3 | Nothing is destroyed | agrees - a row is written once; re-keying a workspace or migrating legacy ids copies the rows to the re-created attestations' keys; excision (excision.md, not built) removes an excised observation's rows with it, the one exception |
| P16 | A read answers alike on every node | agrees - the name is a separately labelled, node-local field, as the tier a node evaluates itself is (F13): no fold, check, projection or convergence test reads it, and only the local viewer's reads ask for it |
| P18 | Writes are an attack surface | agrees - the name grants nothing; it is cut to 64 characters; only this node's own MCP sessions write it |
| P24 and P5 | Degrade loudly; a failure is not an absence | agrees - recording the name never fails the write it belongs to (a failure is logged and the write stands); a backend failure reading the table is an error, never "no app"; one row that does not decode is dropped and logged, as a stored vector that does not decode is |
| onboarding.md Section 5 and O7 | First contact is the client's own name, in memory only, "not a record of what an app did" | revised by D13: the app that wrote an attestation is now recorded, in the table above; first contact itself is unchanged - still in memory, still not served on the read tier, recorded by no hub listener, sent to no peer |
| compatibility.md Sections 3.2, 3.7 and 5 | A change raises `min_reader` when a binary of the previous format writing to the store would lose or contradict something | agrees - format 3 adds the table and `min_reader` stays 2: a format 2 writer ignores the table, records no app for what it writes - which reads as no app, not as another app, since its attestations have keys of their own - and never rewrites a row. Its upgrade step creates the table. Revised: 3.7's last sentence reads as Section 5 does - a golden store of a format is written once a release moves past it; format 2's stays the oldest |
| federation.md Section 3 and F2 | The wire carries the signed attestation; the content address excludes provenance | agrees - nothing on the wire or in the address changes |
| remote-viewer.md Sections 3.2, 3.6 and V7 | The read tier answers a principal's workspaces; its event stream drops the session | revised: the read tier never asks for the app's name, and its event frame is built from an allowed list of fields, without the session or the app |
| remote-server.md, the hub's listener | A principal's MCP session runs the same tool code as a local one | agrees with O7 - a server that serves principals records no name and its events carry none: the check is whether the server serves principals, nothing else, so the stdio server records |
| compatibility.md Section 7 | The MCP surface is a contract | agrees - no tool's answer or resource carries the name; a test holds that |
| architecture.md Section 2.4 | An attestation is acting host, `on_behalf_of`, workspace, `source_ref`, time, confidence, tier | agrees - the attestation is unchanged; the app is a local record beside it, said in a note there |
| design-system.md Sections 29.6 and 29.7 | An activity row says "An agent" until an event names its app; a who-line's writer is the machine | revised: "Claude Code wrote 2 entities ...", and "Agent - Claude Code on this Mac, for alex - 11:27 AM" |

No row needs a principle changed. O7 is revised by the author's D13.

**Behaviour.**
- **What is recorded.** When an MCP session on this node writes - observe, define_type, propose,
  review - the daemon records the session's client name (from `initialize`, or the request's
  metadata where a session skipped it), cut to 64 characters, for the attestation that write added.
  The stdio server, which runs on this node's store for one local app, records it too. A session
  with no name records none. A principal's session on a hub's listener records none, and its events
  carry none. A write by the console, the CLI or a peer records none.
- **Two writes in one millisecond** with every field alike are one attestation, and keep the first
  app's name; any field that differs makes two attestations and two rows.
- **One table of names.** The names the apps send are mapped to the apps `connect` lists - Claude
  Code, Claude Desktop, Cursor, VS Code, Codex, Gemini CLI, OpenCode - by one table in the core crate,
  which setup's first contact, `connect`'s names and the reads use; the app pages' copy of the names
  is held to it by a test. A name the table does not know is shown as sent.
- **Where it is read.** The local viewer's reads of observations (`/api/observations`, `/api/explain`'s
  supporting rows) name each attestation's app, only for one written here (Section 29.7's rule). The
  local event stream names the app of the session an activity came from. The viewer says it:
  - the who-line's writer: "Agent - Claude Code on this Mac, for alex - 11:27 AM"; the machine alone
    where no app is named;
  - the map's activity: "Claude Code wrote 2 entities and 1 relation in infra", "Cursor searched
    "ledger" - 8 found"; "An agent" where no app is named;
  - Follow's notice, which says the row's sentence.
- **Where it is not.** The read tier's reads never ask for it, and its events are built from an
  allowed list; MCP's tools and its raw observation resource never carry it; sync never sends it.
- **Ids that change.** Re-keying a workspace and migrating legacy ids copy each row to the re-created
  attestation's key, inside their dry run and their idempotent re-run.
- **The store's format** is 3: the upgrade from 2 creates the table, and `min_reader` stays 2.

**What this step does not do.**
- It does not send the app's name to a peer, which would need it signed and a wire version.
- It does not record a name for what was written before it: those attestations read as no app.
- It does not build excision; excision.md gains the line that removes an excised observation's rows.

**Guarded by** `an_attestation_keeps_the_app_that_wrote_it` in the store's port conformance (a row
written once, the key moved by every field but the stamp), `an_attestation_names_its_app_on_local_reads_only`
in the engine's policy cases (local reads only, the name table, the 64-character cut, the event, a
re-key that keeps it), `a_session_records_its_app_and_no_tool_answer_carries_it` in the MCP surface
tests (a local session records, no tool or resource answers with it), the read tier's tests until
0.11.0 removed the tier (never answered, never streamed), and `the_app_names_are_one_table` in
`crates/supragnosis-viz/tests/words.rs` (the app pages' and the shell's copies of the names).

### 29.9 Step 47: the map explains itself

**What was found.** Measured on the Graph page:
- **Marks no key explains.** The filters panel keys the type fill and the trust ring only. The coral
  "!" badge, a disc's size, the soft region clouds, the named outlines, the footprint's glow and the
  pulse are explained nowhere on the page; a first reader takes the three labelled outlines for the
  "10 regions" the status bar counts, which are the unlabelled clouds - two-member ones and ones
  drawn under Regions: Off included.
- **Counts that do not match the map.** The status bar and the canvas's name count the server's
  rows, so with Concept hidden 17 of 20 entities are drawn while both still say 20. With the panel
  closed nothing says a filter is on.
- **A legend that cannot answer "how many" or "only this".** The type chips carry no count, though
  Section 6 promises one; seeing one type alone takes a click on every other; bringing all back takes
  one per hidden chip; an off chip is drawn at 38% opacity, which Section 4 says reads as disabled.
- **A type's color changes with the scope.** Colors are assigned from the types of the loaded view,
  so Technology is olive under All, green in billing and pink in atlas-web: a click on a workspace
  chip recolors a node.
- **Trust can be read but not filtered,** and the four entries look like the type chips above them,
  which are buttons. "Agent has none" reads as "agents have no trust".
- **Hover dims the whole map** at every entity the pointer crosses, the card leaves out the
  workspace under All, and the pointer is a grab hand over an entity too.
- **One object, several names:** region (the status bar), "Overloaded context" (Health), "Make it a
  group", "group name (optional)" and "Name this group" beside them, and View's "Group by type",
  whose line says "between groups".

**Decisions.** The author approved D3 - keep "region", define it in the map's key, count what is
drawn; D4 - assign each type its color once for the store; D5 - a trust filter dims, it does not
hide.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U16 and Section 5 | "a view gives up to eight types eight hues"; "A type keeps its color in every view where no earlier name takes its hue"; past sixteen a color is shared | revised by D4: a type keeps one color in every view a page shows - the store's sixteen most used types take the band's slots by the same rule, in hue then tone, and any other type its preferred hue, which it may share; a view of rarer types can repeat a hue. The band, its measurements and `stableColors` are unchanged - the landing's and the principles page's copies of it stay byte-identical |
| design-system.md Section 6 | "Counts per type are the legend entries' to show" | repays - each chip carries its count |
| design-system.md Section 4 and U9 | An option that is off is drawn at full strength; surfaces, not lines | repays - an off chip keeps its text and its filled swatch at full strength, with Lucide's eye-off; no hollow swatch, which would read as the Boundaries outline |
| U2 and U3 | Trust is the ring; the badge marks a contested entity and its count is never hidden | agrees - the key names both; a dimmed tier keeps its rings, and a contested entity's badge stays at full strength, counted whatever is dimmed |
| U19 | Every control is reachable from the keyboard and big enough to press | agrees - Only is a button of its own beside each chip, shown with the chip's focus or hover and always under a coarse pointer, 28 pixels with its spacing |
| U17 | One place, one name | repays - region for the cloud, "workspace outline" for the boundary, "Arrange by type" for View's layout |
| U24 | List shows the entities the map draws, in its scope, filters and search | repays - List's head says the status bar's count |
| U25 | A tooltip names; it explains nothing | revised: the hover card is a card, not a tooltip - Section 29.6 called it the map's tooltip - and says only the entity's facts |
| U28 and U38 | Drawing never moves the layout; what is drawn does not depend on the zoom's cut | agrees - dimming, a hover and a filter redraw without a simulation step, and the counts are taken from the filters, never from the zoom |
| U40 | The first picture comes within its budget | agrees - the store's most used types are a read of their own, made once per page beside the first graph read; it reads the entity rows only, never the log, so it does not outlast the graph read it runs beside |
| P17, remote-viewer.md Sections 3.2 and 3.3, V3, V4 | The read tier answers only within the grants; `*` is refused whole while a grant is unservable | agrees - the most used types are their own read, `/api/common_types`, which the viewer's HTTP layer answers and MCP never carries; on the read tier it counts one granted workspace at a time, over those the hub may serve, and skips an unservable grant, since naming its types would answer what V4 refuses to; remote-viewer.md Section 3.2 gains its row |
| P16 | A read answers alike | agrees - counted over the entity rows, the materialized projection, merged-away rows left out, ordered by count then name, names only; a row's kind is the projected one, which the map's converges with at the next replay; a page keeps the list it first received, so a later count does not move a color |
| onboarding.md Section 7 | The empty workspace says "Nothing here yet. Ask a connected AI app to remember something, or pick a workspace that holds knowledge, each named with its count." | agrees - kept; only an unlisted name is said differently, on the local console |
| design-system.md Section 29.6 | The map's activity and its hover words | agrees - unchanged; the card gains the workspace and the contested line |

No row needs a principle changed.

**Behaviour.**
- **The map's key.** Under Trust, a short "On the map" list names each mark the current view draws,
  its mark drawn beside it, and only those: "Two sources disagree" (while one is contested; "- select
  to decide" on the local console), "Larger: more relations", "Soft cloud: a region - entities said
  together" (while a region is drawn), "Outline: a workspace" (Boundaries: Workspace) or "Outline:
  the machine that signed" (Boundaries: Origin), "Glow: what agents used" (while the footprint shows)
  and "Ring that grows and fades: just touched" (the pulse).
- **Type and relation chips** carry their count in tabular figures, sorted by count then name once
  per scope - a later read does not reorder them under the pointer. Beside each chip, an Only button
  shows that type alone - shown while the chip or the button itself has the pointer or focus, and
  always under a coarse pointer; Option-click on the chip does the same. A section with anything hidden heads
  "Entity types 13 - 2 hidden" with Show all. An off chip keeps its text and swatch at full strength,
  marks itself with eye-off, `aria-pressed` false. The legend lists the loaded scope's types, not the
  store's.
- **Counts say what is drawn.** The status bar, the canvas's name and List's head read "billing - 6 of
  7 entities (1 type hidden), 5 relations, 2 regions": entities whose type is shown; relations whose
  ends are shown and whose type is shown, whatever the Relations switch says; regions with three or
  more shown members, whatever Regions draws. A dimmed tier is said too ("Agent dimmed"). While
  anything is hidden or dimmed the status bar offers Show all, which clears both.
- **One color per type.** A read of its own, `/api/common_types`, names the store's sixteen most used
  entity types in rank order, names only - every workspace on the local console; on the read tier the
  granted, servable ones, counted one at a time. The page asks once, beside its first graph read,
  assigns those types the band's slots by the existing rule and every other type its preferred hue,
  and keeps that assignment while it lives.
- **Trust filters.** The four Trust entries become toggles with counts ("Human confirmed 2"); one with
  nothing in the scope is disabled and says "none here". Switching one off dims its entities to the
  strength a selection leaves the rest at - named `DIM_NODE`, `DIM_EDGE` and `DIM_REGION` - and keeps
  them in the layout and in the counts. Where the scope has one tier, or none, there is nothing to
  filter: no toggles, and the note "All here: Agent, so no entity is ringed" stays. Otherwise the note reads "A
  ring is the strongest source behind an entity. Agent claims, the usual case, draw no ring.", and
  the Agent swatch is a plain disc.
- **The hover card**: the name; "Service in atlas-web" where the scope spans more than one workspace;
  "2 relations - 1 source - Agent"; the description's first line; "Two sources disagree" when
  contested. The pointer is a hand over an entity, a grab hand over the ground and grabbing while
  dragging. Hover lights the entity's relations, names its neighbours and brings its regions to full
  strength, without dimming the rest; dimming stays with a selection.
- **One word for the cloud.** Health's "Overloaded context" is "Overloaded region"; "Make it a group",
  "group name (optional)" and "Name this group" are "Name this region" and "region name (optional)";
  the notice is "Region named - it is an entity now, related to its members". View's "Group by type"
  is "Arrange by type", "with the relations between types kept"; Boundaries' line names "a workspace
  outline".
- **An unlisted workspace** in the address shows its chip, selected and muted, and on the local
  console the empty map says "No workspace is called nothing-here." above Section 7's sentence; the
  read tier keeps its refusal.

**What this step does not do.**
- It does not record what Name this region creates as a region: `/api/reify` still types it
  "Context" (owed, with the type vocabulary).
- It does not filter or sort List by trust (R2-06), move labels off rings, outlines and badges or
  shrink the badge on a small node (R2-18); both are owed beyond this pass.
- It does not move the minimap or make Fit leave room for it (R2-14), which step 50 takes with the
  zoom keys (D7).
- It does not hide the type and trust sections or disable zoom, Fit and List on an empty scope, or
  reword the Log's empty line and the "default" scope's chip (R2-17, R1-09), which step 48 takes with
  the first entry.

**Guarded by** `the_maps_key_names_only_what_is_drawn`, `the_type_chips_count_and_offer_only`,
`the_counts_are_of_what_is_drawn`, `a_type_keeps_its_color_in_every_scope`,
`a_trust_filter_dims_and_keeps_the_count`, `the_hover_card_says_what_an_entity_is`,
`an_unlisted_workspace_is_named_as_such` and `the_cloud_is_a_region_everywhere` in
`crates/supragnosis-viz/tests/key.rs`; `the_store_names_its_most_used_types_whatever_the_scope` in
the viewer's HTTP tests (names only, counted in every workspace, at most sixteen, ties by name); and
the read tier's tests until 0.11.0 removed the tier (a type only outside the grants was not named,
and an unservable grant was skipped rather than refusing the page its colors).

### 29.10 Step 48: the first entry on screen

**What was found.** Measured on Setup, the landing and the empty map:
- **The first entry arrives as a sentence about the store.** When it comes, the Try it card turns
  Done with "The store holds an entry: workspace "billing", just now." The person's own words do not
  show, though setup reads them for `setup --step wait`, which prints them. The card still says "The
  entry appears here as soon as the app records it.", the change is not announced, and Open the Graph
  goes to the workspace, not to what was written. onboarding.md Section 4.4 already says the step
  shows what the entry said.
- **Try it waits without saying for what.** "Ask a connected app" shows as soon as one app is
  connected, whether or not any has reached the server. The card has no clock and nothing to check
  when nothing comes. The helper's notice says "Claude Desktop restarted and loaded supragnosis.",
  though nothing checked the load.
- **The landing hands off to the CLI.** The hero's command installs the app without opening it. The
  line under it teaches `supragnosis connect claude-code`, the step Setup takes for every installed
  app at once, and Get started says the same under "Up and running in a minute.", above a 224 MB
  download. onboarding.md Section 8 decided the command ends with `open -a Supragnosis`. The Linux
  tab still says to keep the server running with "the systemd user unit in deploy/systemd", which
  `supragnosis setup` now writes.
- **The empty map speaks in three voices and keeps its controls.** The Log says "no observations in
  this workspace yet - observe knowledge to see the log", lowercase and naming a tool. The strip says
  "Nothing observed in this scope yet" and the canvas "Nothing here yet.". The type, relation and
  trust sections, zoom, Fit and List stay, and act on nothing, and the canvas's name still says List
  shows them as a table. A store whose entries name no entity reads as empty. In the app, a store with
  nothing in it does not lead back to Setup.
- **The default has two names.** The status bar and the search field call the scope addressed as ""
  "default", whatever the node's default is named, and so does its chip while it holds nothing; once
  it holds something its chip takes its name.

**Decisions.** The author approved D18: the landing promises "Up and running in a few minutes.", since
the model is a 224 MB download and an AI app may need a restart.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| onboarding.md Sections 4.4 and 9 | When the entry arrives the step shows what it said and in which workspace; the first useful moment is an entry, not a green check | repays - the card quotes the entry and opens the graph on it |
| onboarding.md O1 | Setup appends nothing to the knowledge log | agrees - setup reads the newest entry, the app that wrote it and its first entity, and writes nothing |
| onboarding.md O3 and O4 | The checklist is computed from the live state; every state the page draws comes from setup's events | revised - the entry, its app and entity, which app has reached the server and the checks are setup's; O4 gains one exception, how long the page has waited in this visit, which it counts itself and uses only to show the minutes, the checks the event carries and the foot's Copy Diagnostics |
| onboarding.md O6 and P17 | Nothing leaves the machine during setup; a report copies no secret | agrees - the shell's Copy Diagnostics drops `quote`, `app` and `entity` from each event line it copies, so a report carries no knowledge |
| Section 29.8 and onboarding.md O7 | The app's name is read on local reads only | agrees - setup reads the entry over the local socket, whose reads name the app; on a remote profile the step is skipped as before |
| onboarding.md Sections 4.2 and 5 | First contact is not a condition any step waits on, and a restart forgets it | agrees - it chooses the waiting sentence only, the question shows in every case, and the sentence for no contact says "since it started"; Try it is done when an entry is in, as before |
| P5 | Unknown is not empty | agrees - a daemon that does not report first contact keeps "Ask a connected app"; one that does not answer keeps "Known once the server answers."; a store whose entries name no entity says its Log holds them |
| onboarding.md Section 9 | The app restarts Claude Desktop for the person (its ninth decision) | agrees - the helper's notice says only what it knows |
| onboarding.md Section 8 and design-system.md Section 11 | The landing's install command ends with the app open; under it one line says what follows; the page says only what the binary it offers does | revised - Section 11's hero command and next-step line change, the Linux tab gains `supragnosis setup`, and every sentence is true of the release the page offers today; `the_landing_says_the_next_step_and_reads_on_a_phone` follows the new line |
| U17 and Section 9 | One concept, one name; a reader's words name no tool | repays - "observe knowledge" and "Nothing observed" are retired, the Log, the strip and the canvas say "Nothing here yet.", and the node's default is called by its name everywhere |
| onboarding.md Section 7 | Words where people land | revised - the Log's empty line joins the table, and in the app an empty store's map offers Open Setup |
| Section 4 and U19 | A control that cannot be used now is dimmed and says why; a control exposes its state | agrees - with the scope empty, zoom, Fit and List stay focusable, are `aria-disabled`, and are described by the empty map's sentence |
| U24 | The canvas's name says what it draws and that List holds it | revised - with nothing drawn the name says the scope holds nothing, without List |
| Section 29.9 | Where the scope has one tier, or none, the Trust note stays | revised - with no entity the Trust section hides with the type sections |
| Section 6 | The strip says "Nothing observed in this scope yet" | revised - it says "Nothing here yet." |
| Section 6a and Section 29.5 | One Copy Diagnostics, shown once a step has run or failed in this visit; the page is reached three ways after the first visit | revised - the foot's Copy Diagnostics also shows once Try it has waited two minutes, and the empty map's Open Setup is a fourth way in |
| settings-page.md Section 6 and compatibility.md Section 8 | The viewer does not know the shell exists; every coupling between them is listed | revised - the shell gives the page one more optional global, Setup's address, as it gives `window.supragnosisHere` (Section 9, D2); without it the page has no button, and compatibility.md Section 8 lists it |
| Section 10 | A step's end is announced in the page's live region | repays - the first entry is announced |
| Section 6 | The scope in use is a selected chip; the first view says when the default is empty | revised - the default's chip keeps its place and takes the default's name, and so does the first view's note |
| remote-viewer.md Section 3.5 | Locally `/api/surface` answers the surface and the node; the read tier gives the workspace an omitted one resolves to | revised - the local answer gains `default_workspace`, the same key with the same meaning, the workspace an omitted one resolves to: the node's default locally, the reader's first grant on the read tier |

No row needs a principle changed.

**Behaviour.**
- **The entry, quoted.** When Try it is done, setup's event carries the newest entry's first 140
  characters (`quote`), the app that wrote it when the local read names one (`app`), and the id of its
  first entity when it names one (`entity`), beside the workspace its id already carries. Its sentence
  reads "An entry is in "billing", from Claude Code, just now." ("An entry is in "billing", just
  now." with no app named, and "An entry is in the store, ..." from a daemon that does not say the
  workspace). The card shows the sentence and the quote in the block Try it used for the question.
  With an entity, a quiet "Show in the Graph" opens the graph at that workspace with that entity
  selected (`?entity=`, Section 29.4); the foot's Open the Graph stays the page's main act and opens
  the workspace. When the card turns done while the page shows, the page announces "First entry
  recorded in billing". `setup --step wait` says the same sentence and quote, and `setup --check`'s
  text the sentence alone. The page reads `setup --check --events`, the line Copy Diagnostics copies
  too, so the shell's diagnostics drop `quote`, `app` and `entity` from each event line before they
  are copied. An app older than this step shows the sentence alone, and this app with an older CLI
  shows what that CLI says. The `--events` example, `setup-events.jsonl`, gains the three fields and
  `checks` (compatibility.md Section 6).
- **The wait follows first contact.** While an app is connected and no entry is in, setup's sentence
  says which app has reached the server, and quotes the question in every case:
  - no client has reached the server since it started: "No connected app has reached the server
    since it started - each app's row says how. Then ask, in the project you are working on:", and
    the card repeats an app's restart when its row offers one;
  - one has: "Claude Code has reached the server. Ask it, in the project you are working on:", or
    "Claude Code and Cursor have reached the server. Ask one, in the project you are working on:", a
    client the table does not know by the name it sent; an unnamed client is not named, and when
    only such clients have reached it, "A client has reached the server. Ask your app, in the project
    you are working on:";
  - first contact is not known: "Ask a connected app, in the project you are working on:", as the
    page words it now.
- **Minutes and what to check.** From 60 seconds after the card first waited in this visit it says
  "Waiting 1 minute", in whole minutes. From two minutes it shows the checks setup's event carries:
  "Does the app list supragnosis's tools? Its tools or MCP list shows them." and "Some agents answer
  without recording. Ask it to use supragnosis to remember the decision.", and the foot's Copy
  Diagnostics shows. `setup --step wait` prints the same checks once two minutes pass.
- **The helper's notice** says what it knows: "Claude Desktop is open. Its row says when it reaches
  the server."
- **The landing.** The hero's macOS command is `brew install --cask ashon/tap/supragnosis && open -a
  Supragnosis`. The line under it reads "The app's Setup opens: it keeps supragnosis running, offers
  to connect each AI app it finds and downloads the search model. With the script, run `supragnosis
  setup`. All steps ->". Get started is "Up and running in a few minutes." with three steps: Install
  (the macOS tab's command opens the app; the server alone and the source build stay; the Linux tab's
  comment becomes `supragnosis setup`), "Set up" ("The app's Setup keeps the server running at
  login, offers to connect each AI app it finds and downloads the search model in the background. On
  Linux or a server, `supragnosis setup` does the same."), and "Try it" ("Ask your agent to remember
  one decision and why. Setup says when it is in, and the Graph shows it."). The note names Setup
  among what fetches the model. Each sentence is true of the release the page offers today.
- **An empty scope keeps no idle controls.** While the scope holds no entity, the Entity types,
  Relation types and Trust sections hide, Definitions shows only while the scope has a definition,
  and zoom in, zoom out, Fit and List are `aria-disabled` and described by the empty map's sentence.
  The canvas's name says "Map of billing: nothing here yet." ("Map of billing: nothing on the map yet;
  the Log holds its entries." when its entries name no entity). The Log's empty line is "Nothing here
  yet. What an AI app remembers in billing shows up here." ("What an AI app remembers shows up here."
  under All), and the strip says "Nothing here yet.".
- **Entries with no entity.** When the scope's Log holds entries and none names an entity, the canvas
  says "Nothing on the map yet. The entries here name no entity - the Log holds them." in place of
  the first-entry sentence.
- **Open Setup on an empty store.** In the app, on the local console (`/api/surface` says `local`; a
  remote profile's window shows a read tier, where the store is not this Mac's), while the store
  holds no entry, the empty map adds
  "Open Setup" under its sentence, carrying `?from=` as the gear does (Section 29.4). The shell gives
  the page Setup's address in `window.supragnosisSetup`; without it there is no button.
- **The default by its name.** The local console's `/api/surface` gains `default_workspace`, the key
  the read tier already answers with the same meaning: the workspace an omitted one resolves to, the
  node's default here. The chip, the status bar, the title, List's head, the search field and
  search's sentences call the scope addressed as "" by that name, whether or not it holds anything,
  and "Showing all workspaces - default is empty" names it the same way.
- **Retired words** gain "observe knowledge", "Nothing observed", "The store holds an entry" and
  "restarted and loaded supragnosis".

**What this step does not do.**
- It does not give the tray its two lines and state icon, or add the Help and Setup menu commands
  (R4-05, R4-13), which step 50 takes.
- It does not change Settings' sidebar or remember the window's frame (R4-14, R4-16), which step 50
  takes with the menus.
- It does not add keys for zoom or Fit (D7), which step 50 takes.
- It does not carry the server inside the app (onboarding.md Section 10).

**Guarded by** `try_it_quotes_the_entry_and_follows_first_contact` in setup's tests (the done
sentence, quote, app and entity, the waiting sentence for each case of first contact with the
question quoted in each, an unnamed client counted and never named, and a checklist text without
the entry's words); `setup_events_match_their_example` (the four new fields); in the app's tests
`try_it_quotes_the_entry_and_counts_its_wait` (the card, Show in the Graph, the announcement, the
page's one clock and Setup's address in the shell) and `copy_diagnostics_carries_no_entry`;
`an_empty_scope_keeps_no_idle_controls`, `the_empty_map_speaks_in_one_voice`,
`an_empty_store_leads_back_to_setup_in_the_app` and `the_default_is_called_by_its_name` in
`crates/supragnosis-viz/tests/empty.rs`; `the_local_surface_names_its_node` (the default's name);
`the_landing_says_the_next_step_and_reads_on_a_phone` (the command that opens the app, the line
under it and the promise); and `no_retired_word_reaches_a_page` (the four retired phrases).

### 29.11 Step 49: search finds, and holds

**What was found.** Measured on the Graph page:
- **The field keeps the term like a filter.** Choosing a hit writes its name into the field, and the
  name stays on the Graph, in List and on Review. A filled field reads as a filter that is on, yet
  List shows every row under it: its rows apply the type filters only, though U24 says List follows
  the search.
- **A search's answer cannot be kept.** Typing "Postgres" lights 13 hits while the list is open; a
  click on the map or a choice closes the list and the lit set is gone. Comparing three hits takes
  three searches, and from a selected entity only its direct neighbours light, with no way to go a
  step further without losing the first.
- **Hits do not say where they are.** Under All, two "Payment service - Service" rows differ in
  nothing the row shows.
- **List keeps the map's camera.** In List, zoom in, zoom out and Fit stay lit and move a map no one
  can see; List is one camera button among six. A contested entity is a plain row, and the table is
  fixed to type then name.

**Decisions.** The author approved D8: search is a way to go somewhere, and its answer can be kept
on the map. A choice clears the field, and "Show these 13 on the map" pins the answer as the
**working set** - the entities the person asked to keep in view - while the rest of the map dims as
a selection dims it. A set laid out on its own, as Neo4j Bloom does, was ruled out because it loses
the remembered layout; a term kept in the field that every view filters by was ruled out because a
filled field would then mean two things. For the same reason List follows the working set and not
the field.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 8 | Enter on a row focuses an entity or opens an observation's card; once a row is chosen, the field shows its name; the empty scope's first row is "Search all workspaces" | revised - after a choice the field clears and the chosen name is the sheet's title, so a filled field always means a query in progress; the set row comes first in the list, before the groups |
| U15 | Each hit says how it was reached and over what scope; nothing is chosen before the person moves | agrees - an entity hit names its workspace when the scope holds more than one; the set row is a row like the others, not chosen until the person moves to it, and ArrowDown then Enter pins the set where it took the top hit before |
| P5 | A bounded answer says what it left out | agrees - the set row says "these 12", and "- the search stopped at 20" when the answer holds the path's limit, as Section 8's "20+" reads it; hits the view does not draw are counted apart, "2 are not drawn in this view"; the status line says the same |
| P19 | The search answers what the engine answers | agrees - the set is the engine's answer, kept; what Show related adds is the graph's own relations, said apart in the status bar |
| U24 | List shows the entities the map draws, in its scope, filters and search, sorted by type and name | revised - List follows the working set, not the field, starts by type and name and sorts by any column; a line under its head says what the set holds, with Clear |
| U3 | A contested entity is marked wherever it is drawn | repays - List's row carries the badge, with "Two sources disagree" as its words |
| Section 29.9 | The status bar says what is hidden or dimmed and offers Show all, which clears both; the canvas's name says what it draws | agrees - the status line says ", 13 kept for "Postgres"" while a set is pinned, Show all ends the set with the filters, and the canvas's name says it too |
| Section 29.4 and U20 | Follow waits while the map is in use; it never takes the camera while the person is using the map | revised - Section 29.4's list gains "while a working set is pinned", an extension of D6 that U20's own words already cover; pinning changes no scope or selection, and moves the camera only when the person presses Fit |
| U38 and U40 | At a far zoom a node fades by rank unless hovered, selected, active, pulsing or contested; a frame keeps its budget | agrees - the set dims the rest without making its members active, so at a far zoom they fade by rank like any entity and Fit brings them back; a frame looks each drawn node up once |
| U28 | Drawing never moves the layout | agrees - a set dims and lights and moves nothing |
| Section 4 | A row of segments that changes what a page shows is a tab list; a control that cannot act now is dimmed and says why | agrees - Map and List are two tabs; while List shows, zoom and Fit stay in place, `aria-disabled` and described, as Section 29.10's idle buttons are |
| Section 6 and Section 29.4 | Follow leads the camera toolbar, and List is the first button after it | revised - Map and List, as one tab list, take List's place after Follow |
| Section 10 | Esc closes the topmost card, menu or sheet | revised - while List shows, Esc goes back to Map first, from the table or the tab; it ends a working set last, when nothing else is open to close |
| U19 | Every control is reachable from the keyboard, exposes its state and is big enough to press | agrees - Map and List are tabs with `aria-selected` and the arrow keys; the column heads are buttons with `aria-sort`; the set row carries `aria-keyshortcuts`; the kept pill and Clear are buttons; each is 28 pixels or the camera's 32, and 44 under a finger |
| U25 | A tooltip names a control and its key, five words at most | agrees - "Map", "List", and Fit's "Fit to kept entities" while a set is pinned |
| Section 29.10 | With the scope empty, zoom, Fit and List are idle | agrees - the List tab is idle with them and stays reachable by the arrow keys |
| inspector.md Section 6 | The sheet's Relations heading counts the entity's relations | agrees - it gains Show related beside the count, an act on the map and not a change to the sheet |
| U41 and Section 28.3 | Icons are Lucide's files, one per role | agrees - Map is Lucide's `waypoints`, copied unmodified; the toolbar's row of Section 28.3 gains it |
| Section 5 and remote-viewer.md V7 | The page keeps nothing in the browser that a grant would outlive; one principal's reads are not shown to another | agrees - recent searches and the set live in the page's memory only and are gone when it reloads, as it does on a change of server |

No row needs a principle changed.

**Behaviour.**
- **A choice clears the field.** Choosing an entity selects it and clears the field; choosing an
  observation opens its card and clears the field, as it does now. From Review a choice opens the
  Graph, as now.
- **Recent searches.** An empty field that takes focus lists the last five queries this page ran,
  newest first, under "Recent"; choosing one asks it again.
- **Hits name their workspace.** When the scope holds more than one workspace, an entity hit's row
  reads "Service in infra", the words the hover card uses.
- **Show these on the map.** When the answer holds two or more entities the map draws, the list's
  first row is "Show these 13 on the map", with "- the search stopped at 20" when the answer holds the
  path's limit and "2 are not drawn in this view" when some entity hits are not drawn, and the key cap
  Cmd+Enter, as the viewer's other keys are written (Ctrl+Enter works too), which the field takes on a
  fresh answer. It adds the drawn entity hits to the working set, clears the field and, from Review,
  opens the Graph. A second one adds to the set, as Show related does.
- **What a working set does.**
  - While no selection, open search list or Review preview takes the map, its members and the
    relations between them stay at full strength, and the rest of the map dims to `DIM_NODE` and
    `DIM_EDGE`; a dimmed trust tier stays dimmed inside it, and regions are drawn as at rest. At a far
    zoom its members fade by rank like any entity (Section 26).
  - The status line says it after its counts: ", 13 kept for "Postgres"", ", 12 kept for "Postgres"
    (the search stopped at 20)", or ", 25 kept - 13 for "Postgres", 12 related to Ledger" once more
    has been added. Since a narrow status bar gives up its counts first, a pill beside them says "13
    kept" with an x named "Clear", which ends the set; it stays on a phone. The status bar's Show all
    ends the set with the filters.
  - The canvas's name says the same, and "List shows the kept ones as a table."
  - Fit frames the set, named "Fit to kept entities"; the camera moves only when the person presses
    it.
  - Follow waits (Section 29.4).
  - A member merged away is followed to what it became, as a selection is (Section 29.4); one the
    filters hide or the scope no longer holds is not counted; a change of scope ends the set.
  - Esc ends it when nothing else is open to close.
- **Show related.** The sheet's Relations heading offers "Show related" while the entity has drawn
  relations. It adds the entity and the entities its drawn relations reach to the working set,
  pinning one when none is, and pressing it on a related entity's sheet goes a step further.
- **List follows the working set.** While a set is pinned List lists its members, and a line under
  List's head says "13 of 20 entities kept - 13 for "Postgres"", with Clear. Otherwise it lists
  everything drawn, as now.
- **Map | List.** The camera toolbar keeps Follow first; after it, Map (Lucide `waypoints`) and List
  (Lucide `list`) are one tab list, the arrow keys moving between them. While List shows, zoom in,
  zoom out and Fit stay in place, `aria-disabled` and described by "List shows the map as a table.",
  and Esc goes back to Map, from the table or the tab. Under 760 pixels the toolbar wraps onto a
  second row when its controls do not fit, so nothing scrolls sideways at 320 under a finger.
- **List's rows.** A contested entity's row carries the badge before its name, with "Two sources
  disagree" as its words. Each column head is a button that sorts by that column, again to reverse,
  and says which way with `aria-sort`; ties sort by name, and the order holds across polls. The table
  starts by type, then name. Sorting by Trust takes a part of R2-06; filtering by trust stays owed.

**What this step does not do.**
- It does not lay a set out on its own or keep it across visits (D8).
- It does not filter List by trust (R2-06).
- It does not move the minimap or make Fit leave room for it (R2-14), which step 50 takes.

**Guarded by** `a_choice_clears_the_field_and_recent_searches_wait`,
`the_answer_can_be_kept_as_a_working_set`, `show_related_adds_what_the_relations_reach` and
`list_leaves_the_camera_idle_in_place` in `crates/supragnosis-viz/tests/holds.rs`;
`the_map_has_a_list` (the tab list after Follow, the rows that follow the set, the sorting heads and
the contested badge); `search_chooses_nothing_until_the_person_moves` (plain Enter takes no row,
Cmd+Enter keeps only the set row, a choice clears the field); and
`an_empty_scope_keeps_no_idle_controls` (the idle buttons, which List shares).

### 29.12 Step 50a: the map's keys and its View

**What was found.** Measured on the Graph page:
- **The keys cannot be found.** Cmd+K is said nowhere. Zoom, Fit and List have no key, and there is
  no list of the keys; View's "Keys" line names four of them beside the switch that turns them off.
- **View mixes kinds of things.** Its popover holds how the map looks, how the layout behaves, a
  keyboard preference and an act, Reload. Twelve controls with a line each make it taller than a
  900-pixel window, so it scrolls. Reload is wanted exactly when the status bar says "Not updated
  since ...", which is far from the popover.
- **b moves the map and says nothing.** Under All, three presses of b moved nodes by up to 101
  pixels, since the workspaces cohere only while their outlines are drawn. A choice that outlines
  nothing here - Origin where fewer than two machines are recorded, Workspace in a scope of one -
  looks the same as None.
- **Fit and the minimap cover the same corner.** Fit frames down to the window's foot, under the
  camera toolbar; at rest, when the map is fitted, the minimap repeats the
  overview at a fifth of its size over the window's lower right (R2-14).

**Decisions.** D7, approved, gives the map's keys the Mac's conventions, with the single keys =, -
and 0 for zoom and Fit, since Cmd+= is the text's size; its other half, the selection history and
the address, is Section 29.4's. Its recommended option moved the single-key switch to Settings; the
author decided to put it in the Keys dialog instead, beside the keys it governs, since it is the
page's own choice, kept in the page as View's choices are, and the app's Settings page holds only
what changes this Mac (settings-page.md Section 2). R2-14, which Sections 29.9 and 29.11 handed to
this step, hides the minimap while it repeats the map; the minimap stays on, as the author asked
after step 7 (Section 18), and draws while zoomed in, by the rule below. R2-10's
recommended option holds: the workspaces cohere whatever Boundaries draws. R2-15's holds: the
switches that change least sit under More.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 10, and U19 | The view's single keys act outside a text field and can be turned off (WCAG 2.1.4); a dialog holds focus and makes the page behind it inert | Section 10 revised, U19 agrees - the single keys are /, b, [, ], =, +, -, 0, l and ?, each without Cmd, Ctrl or Option; the switch moves from View to the Keys dialog, which holds focus, makes the page behind it inert and closes with Esc |
| Section 7 | On the Review page the map behind it is inert | repays - b, [ and ] acted behind the Review page; the map's keys now act only on the Graph page while no dialog or card is open, and / also on the Review page, where a search opens the Graph |
| U25 and Section 9 | A tooltip names a control and its key; one line never states two contradictory states | agrees - "Zoom in (=)", "Zoom out (-)", "Fit to view (0)", "Map (l)" and "List (l)", and each control names its key only while the single keys are on |
| U28 | Drawing never moves the layout | repays - b only draws: the workspaces cohere whenever the scope holds more than one, whatever Boundaries draws |
| Section 5 | The workspaces cohere while the boundaries draw them; b cycles Boundaries | revised - they cohere whenever more than one is in scope, except under Arrange by type, as now; b cycles the choices that outline two or more groups here, and None |
| Section 27 and the remembered layout | A scope of several workspaces is laid out with their cohesion | agrees - Boundaries is not among View's stored choices and starts on Workspace, which already coheres, so a remembered layout is unchanged |
| P5 | Unknown is not empty | agrees - Origin counts the machines recorded on the drawn entities, those whose type is shown; an entity no peer stamped records none, so its line says "Fewer than two machines are recorded here", not that one machine signed everything; Retry keeps the poll's own reason |
| Section 29.9 | The key names each mark the view draws, and only those | repays - an outline's row shows only while two or more outlines are drawn |
| U42 and Section 28.4 | A notice that is the outcome of the person's act is `ok` and leaves on its own | agrees - b's notice is `ok`, and a later press replaces it with a new one, in the newest place with its own clock |
| Section 6 | The status bar says "Not updated since 8:54 - the server is not answering"; the minimap is on until View turns it off; b still cycles Boundaries | revised - a Retry button follows the line; the minimap draws only while zoomed in; b cycles the choices that draw |
| Section 18 | The minimap starts on, at the author's request | revised - it stays on, and draws while it shows something the window does not (R2-14) |
| Section 29.4 | The person's move is a pan, zoom or drag, the buttons' included; Follow waits while the map is in use | revised - =, +, - and 0 are the person's move, and Follow waits while the Keys dialog is open |
| Section 6, View's stored choices | View's choices are remembered | repays - Boundaries is stored with them, now that it no longer moves the layout |
| Section 4 | A switch that is off is drawn at full strength; a control that cannot act now is dimmed and says why | agrees - Arrows and History are dimmed and say why while Relations is off; More is a disclosure whose summary names a switch under it that is off |
| U32 and U35 | The minimap's miniature is a bitmap painted when it changes, under the window's frame drawn each frame | agrees - the minimap shows only once its miniature is painted, and fades over 120 ms, at once under reduced motion |
| Section 29.11 | Map and List are one tab list; zoom and Fit are idle while List shows | agrees - l moves between them, and the keys are idle when their buttons are |
| P24 | A failure says so | agrees - a Retry that fails keeps the poll's reason and says when it tried: "(tried 9:02)" |

No row needs a principle changed.

**Behaviour.**
- **Keys.** On the Graph page, outside a text field, while no dialog or card is open, and without
  Cmd, Ctrl or Option: = (or +) zooms in, - zooms out, 0 fits, l moves between Map and List, b cycles
  Boundaries, ? opens the Keys dialog, and [ and ] toggle the panels; / opens search on the Review
  page too. A letter is matched by what it types, and by the key's place only when the input source
  types a letter that is not Latin, so b and l act under Hangul as under Dvorak. A key is idle when
  its button is (Sections 29.10 and 29.11). The buttons' tooltips name their keys while the single
  keys are on.
- **The Keys dialog.** ? or View's "Keys..." opens a dialog titled "Keys", listing the Graph page's
  keys with what each does, in groups: Find (/ and Cmd+K; Cmd+Enter keeps an answer), Panels ([ and
  ]), Map (=, -, 0, l, b), Selection (Cmd+[ and Cmd+], Esc) and Lists (the arrow keys move, Enter
  chooses). At its foot is the switch "Single-key shortcuts", with the line "Letters and symbols act
  on the Graph page, outside a text field, while no dialog or card is open." The switch governs ?
  too; View's "Keys..." stays the way back.
  The dialog holds focus, makes the page behind it inert, and Esc closes it alone.
- **View holds what changes the picture.** Regions; Boundaries; Show with Labels, Relations, Arrows,
  History, Minimap and Arrange by type; and a disclosure "More" with Footprint, Pulses, Peers and
  Reset layout, its summary saying "More - Pulses off" when a switch under it that starts on is off
  (Peers starts off on a node and on on a hub, so it is not named). Arrows and
  History are dimmed while Relations is off, with the line "Shown with relations." At View's foot,
  "Keys...". The Layout group, the Keys group and Reload leave it, and View fits a 900-pixel window
  without scrolling. *(Building it settled that Peers and Reset layout go under More: with them in a
  Layout group View still scrolled at 900 pixels.)*
- **Retry where the map is stale.** While the status bar says "Not updated since 8:54 - the server is
  not answering", or "Not loaded - ...", a Retry button follows the line; it reads the workspaces and
  the map at once, and when that fails too the line keeps the poll's reason and says when it tried:
  "Not updated since 8:54 - the server is not answering (tried 9:02)".
- **b only draws.** The workspaces cohere whenever the scope holds more than one, whatever Boundaries
  draws, except under Arrange by type, as now, so b never moves a node; None's line says "The
  workspaces keep together, without outlines." A choice that would outline fewer than two groups
  here is disabled with its reason - "One workspace here - nothing to outline", "Fewer than two
  machines are recorded here - nothing to outline" - and b and the arrow keys pass it by; a current
  choice that becomes so stays chosen and says so; while no choice outlines anything, b says
  "Boundaries: nothing to outline here" and changes nothing. The groups are counted over the drawn
  entities, those whose type is shown, as the outlines are drawn. Each press of b says its state in
  an `ok` notice, which the next press replaces: "Boundaries: Workspace", "Boundaries: Origin",
  "Boundaries: None". Boundaries is stored with View's choices.
- **The minimap while zoomed in.** The minimap draws while the window holds less than 80 percent of
  the drawn map's box by area, the box grown by the largest node's radius so a single entity or a line
  of them has an area, and leaves when it holds more than 90 percent, so it does not flicker
  at the line; it fades in and out over 120 ms, at once under reduced motion, and shows only once its
  miniature is painted. The Minimap switch's line says "The whole graph, while you are zoomed in;
  click or drag it to move." A stored choice keeps.
- **Framing keeps the toolbar's box.** Every framing - Fit, the first view, the refit at rest, a
  search, a preview, Follow, a selection's neighbourhood and the working set - leaves out the camera
  toolbar's box: at the
  window's foot from 760 pixels wide, and at its top, both rows, below it. *(Revised by step 52: on a
  phone the toolbar sits at the map's foot, above the open sheet, and framing leaves it out there.)*

**What this step does not do.**
- It does not add the View, Edit and Help menus, Setup..., the tray's lines and icon or Quit's words
  (D14, D15, R4-05, R4-06, R4-13, R1-16), which step 50b takes, or change Settings' sidebar or the
  window's frame (R4-14, R4-16), which step 50c takes.
- It does not name Origin's outlines by host, R2-10's other part, which waits for a roster the read
  tier may read.

**Guarded by** `the_map_has_keys_that_name_themselves`, `the_keys_dialog_holds_the_switch`,
`view_holds_what_changes_the_picture`, `boundaries_only_draw`, `retry_follows_a_stale_line` and
`the_minimap_waits_and_framing_keeps_the_toolbar_box` in `crates/supragnosis-viz/tests/keys.rs`;
`view_holds_two_choices_with_one_name_each` (the single keys and the switch's place) and
`a_selection_has_a_history_and_esc_gives_the_camera_back` (no single key takes Cmd).

### 29.13 Step 50b: the menu bar and the tray

**What was found.** Measured in the app:
- **The menu bar teaches none of the map.** View holds Graph, Review, the text's size and the
  platform's Enter Full Screen. The panels, List, Fit, Back and Forward, Find and Setup are in no
  menu, so a Mac user's usual way of learning a shortcut - reading the menus - teaches none of the
  viewer's. Help holds macOS's search field and nothing to find with it; Settings... sits directly
  under About with no separator.
- **The tray says its state in one line it cannot act on.** "Running 0.8.2 - starts at login | no
  AI app connected - see Settings" is one disabled line holding two facts and an instruction. Finish
  Setup... and Review take the third place in the order they appeared. The icon never changes, so a
  daemon that could not start looks like one that runs.
- **The tray and the menu bar share item ids, and each hears the other's.** Cmd+2 opens Review at
  All workspaces, since the tray's Review handler runs too, and Cmd+, also takes the tray's jump to
  the section that needs attention.
- **Quit stops the memory without a word.** While the app runs the daemon itself, Quit stops it, and
  every connected AI app's next tool call fails.

**Decisions.** The author approved D14: the tray's icon changes shape only while the memory cannot be
used - and decided that this means a daemon that could not start, stopped or is claimed by two
managers, a search index that needs rebuilding, and a remote server that does not answer or refuses
the credential; not an update ready, an unknown state, a review waiting or no AI app connected. And
D15: while the app runs the daemon itself, Quit says that it stops the memory, with no alert; the
author decided the words replace both the tray's Quit and the app menu's, though the platform names
that one "Quit Supragnosis". The author also decided that closing the window, which leaves the app in
the menu bar, says nothing: the tray says the state, as a menu bar app's does (R4-06's other half).

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| settings-page.md Section 5 | The tray: one status line that says when something needs doing and points to Settings; Open Viewer, Finish Setup..., Settings..., Quit Supragnosis | revised - the state line, and when something needs doing a second line that does it; a fixed order; Quit's words while the app runs the daemon |
| settings-page.md Sections 3.0 and 6 | The tray opens Settings where the attention is; it says what state this Mac is in and opens the page that changes it; no submenus | agrees - the act line opens that section, the tray's Settings... still does, and the tray gains no submenu |
| remote-viewer.md Section 6 and remote-server.md Section 5 | On a remote profile the tray is unchanged: its line names the server and whether it answers | revised - the line reads "Server home - not answering" or "Server home - refused the credential", its act "Check the Server...", and the trouble shape shows while it does not answer or refuses; this Mac's states do not count while a remote server is used |
| Section 9's state words and its Retired words | The daemon has one word per state; no retired word reaches the tray | agrees - the state line keeps the words, "Update ready - restart: 0.8.1 running, 0.8.2 installed" included, and no act line says "Restart to update" |
| Section 7 and Section 29.3 | The tray's review item reads "Review - 3 need you...", or "Review - 2 proposals wait..." when the curation read fails | agrees - unchanged, in its fixed place |
| onboarding.md Section 6 | Finish Setup... while step 4.1 or 4.2 is not done, Start at Login off by choice settling 4.1 | agrees - the same rule; while it shows, the act line does not repeat "Connect an AI App..." |
| Section 6, Section 6a | The app's View menu holds Graph (Cmd+1) and Review (Cmd+2); Setup is reached four ways after the first visit | revised - View also holds the map's commands, Help the app's help, and the app menu's Setup... is a fifth way in |
| U26 and Section 29.4 | Esc and Cmd+[ take the way back on every page but the Graph; the menu bar's Review keeps the scope | agrees, and repays - View's Back and Forward send the page on screen its own Cmd+[ and Cmd+]; one id per item per menu ends Cmd+2's jump to All workspaces |
| U17 | One concept, one name | agrees - View's "Fit to View" is the button's "Fit to view" in a menu's title case |
| Section 29.12 and U19 | The single keys can be turned off; the page behind a dialog is inert | agrees - the menu commands are not single keys; they act only while no dialog or card is open on the page |
| settings-page.md Section 6 | The viewer does not know the shell | agrees - the viewer still learns nothing of the shell; the shell knows more of the page |
| compatibility.md Section 8 | The controls the shell finds on the page are listed | revised - the shell presses the page's own controls by id, and only when the press changes something - the panel toggles by `aria-pressed`, the Map and List tabs by `aria-selected`, Fit, Keys... - only while no `dialog[open]` and no `#obscard.on` is on the page; it sends Cmd+K, and to a viewer that has `#keysDlg` Cmd+[ and Cmd+], to the focused element; a viewer without a control leaves its item disabled |
| U41 | Icons are Lucide's files, and the app's own mark is the app's | agrees - the trouble shape is the mark drawn by the same generator, a second template |
| onboarding.md O6 and P17 | A report copies no knowledge | repays - the shell's diagnostics read, which This Mac's Copy Diagnostics copies, gives the done sentence of Try it as "An entry is in the store.", without the workspace or the app it named |
| Section 29.5 and P24 | `act=diagnostics` focuses This Mac's Copy Diagnostics, so the copy is the person's press; Show in Finder reports its outcome where it ran | agrees - Help's Diagnostics... opens This Mac with Copy Diagnostics focused, the starting page's word for the same act, and Show Log in Finder opens it with `act=reveal-log`, where the page says whether it worked |
| Section 10 | A control has a name and says its state | agrees - the tray's tooltip names the state while the trouble shape shows, "Supragnosis - Could not start"; whether VoiceOver reads it is checked by hand before the release, with the menus |

No row needs a principle changed.

**Behaviour.**
- **Each item has its own id in its own menu,** the tray's apart from the menu bar's, so an item does
  only its own act.
- **The app menu.** About Supragnosis, a separator, Settings... (Cmd+,) and Setup..., a separator,
  then the platform's own items. Its Quit (Cmd+Q) takes the tray's words.
- **Edit.** After the platform's items, Find (Cmd+F) focuses the Graph's search; from another page it
  opens the Graph first.
- **View.** Graph (Cmd+1) and Review (Cmd+2); then Filters Panel (Ctrl+Cmd+S), Log Panel
  (Ctrl+Cmd+L), as Map, as List, Fit to View and Keys...; then Back (Cmd+[) and Forward (Cmd+]); then
  the text's size and the platform's Enter Full Screen. The map's commands act on the Graph page
  while no dialog or card is open; on Review, Settings or Setup they open the Graph and do nothing
  else; on the starting page, or a viewer that lacks the control, they are disabled. Back and Forward
  send the page on screen its own Cmd+[ and Cmd+], which each page reads as it does now; Forward is
  disabled on Settings, Setup and Review, where it has no meaning. Which items act is known from the
  report each page already sends when it loads, which says the controls it has; a page that reported
  wrongly enables an item at most, since each press looks for its control again.
- **Help.** Supragnosis Documentation opens the project's documentation, and Report an Issue... its
  issue tracker, in the browser, both from the app's repository address, sending nothing of this
  Mac's; Diagnostics... opens Settings' This Mac with its Copy Diagnostics focused, and Show Log in
  Finder opens it and shows the log there. macOS's search field stays.
- **The tray's menu**, in this order:
  - the state line, disabled, in Section 9's words: "Running 0.8.2 - starts at login", "Could not
    start: <cause>", "Update ready - restart: 0.8.1 running, 0.8.2 installed", "Two managers run
    Supragnosis", "The search index needs rebuilding", "Stopped"; on a remote profile "Server home -
    answering", "Server home - not answering" or "Server home - refused the credential";
  - when something needs doing, one line that does it, opening the section of Settings where it is
    done. The first that holds wins: the memory cannot be used - "Open This Mac..." (could not start,
    stopped, two managers, the index) or "Check the Server..." (a remote server not answering or
    refusing); an update ready - "Open This Mac..."; no AI app connected, while Finish Setup... does
    not show - "Connect an AI App...". A command line newer than the app says so on the state line and
    adds no act;
  - a separator; Finish Setup... by onboarding.md Section 6's rule; "Review - 3 need you..." while
    something waits; Open Graph; Settings..., which still opens the section that needs attention;
  - a separator, and Quit.
- **The icon.** The mark stays the template it is. While the memory cannot be used, by the author's
  list above, it shows the mark with a dot cut beside it, a second template, so the state reads as
  shape and not as color, and its tooltip says the state.
- **Quit.** While the app's own child runs the daemon and no remote profile is active, the tray's
  Quit and the app menu's read "Quit and Stop Memory"; otherwise "Quit Supragnosis".

**What this step does not do.**
- It does not change Settings' sidebar or remember the window's frame (R4-14, R4-16), which step 50c
  takes.
- It does not take the menu interface from the pages of the main window, which any of them may still
  call; it is owed with the window's permissions, unscheduled. *(Repaid by step 63, Section 29.29.)*

**Guarded by** `trouble_is_the_memory_that_cannot_be_used`, `the_tray_has_one_act_in_order`,
`the_map_commands_act_where_their_control_is` and `each_menu_item_has_its_own_id` in the app's tray
tests; `copy_diagnostics_carries_no_entry` (the done sentence without its workspace or app); and
`drift_and_conflict_are_said_outright` and `the_examples_read_through_the_apps_own_readers` (the
state line's words). The menus and the tray are native, so the build checks them by hand before the
release: each item's act, the order, the trouble shape and what VoiceOver reads.

### 29.14 Step 50c: Settings' sidebar and the window

**What was found.** Measured in the app:
- **The sidebar heads its one group "General".** The heading names the whole list and reads like a
  pane that does not exist.
- **AI apps' count is a bare number,** and counts a different thing from the tray. "2" could be two
  problems or two apps, VoiceOver reads "AI apps 2", and it counts only the apps on the bridge, while
  the tray counts every app with an entry.
- **AI apps shows no dot while a row needs the person.** One app on the bridge makes the section
  fine, so VS Code's "Needs you" leaves no mark in the sidebar, and the tray's Settings... then opens
  Server.
- **Every Cmd+, and every press of the gear opens Server,** the pane a person who keeps everything on
  this Mac never changes.
- **The window forgets its size and place.** It opens at 1280 by 860 at every launch, wherever the
  person left it, and can be dragged narrower than anything it holds; and Settings' Add a server sheet
  runs below a short window with no way to scroll to its buttons.

**Decisions.** None was needed: R4-14 and R4-16 asked for no decision, and the storyboard's
recommended options hold - R4-14's removes the heading rather than renaming it, and counts the apps
as the tray does.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| settings-page.md Section 3.0.1 | A sidebar of sections, each with a status dot, so the state of everything shows before anything opens | revised - no heading for one group; AI apps' count says what it counts; its dot shows whenever a row needs the person |
| settings-page.md Section 3.0 and Section 29.13 | The gear and Cmd+, open Settings; the tray's Settings... opens the section that needs attention | revised - the gear and Cmd+, reopen the last pane; the tray's Settings... opens the first section whose dot shows - Server, AI apps, This Mac - and the last pane while none does; the act line stays as Section 29.13 has it |
| Section 6a, U17 | A section's dot appears only when it needs attention; a state keeps one word on every surface | agrees - "connected" counts every app with an entry, the tray's count; the dot reads "needs attention" and shows while no app has an entry or a row says "Needs you" |
| P5, Section 6 | Unknown is not empty; a count is a known count | agrees - while the apps could not be read (no command line, or one newer than the app) the sidebar shows no count, and the dot says the section needs attention, as its pane does |
| U19 | A control has a name and says its state | repays - VoiceOver reads "AI apps, 2 connected" |
| Section 4 and Section 9 | Mono sets identifiers; a count is said in the reading face, "Relations 3" | repays - the count is set in the caption size of the reading face |
| U21 and Section 10 | Text grows to 200% and nothing is cut short | agrees - the sidebar grows with the text, up to 40 percent of the window, so "2 connected" fits at 200% and the pane keeps most of a small window; a sheet taller than the window scrolls inside it |
| Section 5 | Positions are a convenience; the page keeps nothing a grant could outlive | agrees - the last pane is the Settings page's own, kept in its storage on every load however it opened, and holds no knowledge; lost, the page opens on Server |
| U21's text size | The text's size is kept in the app's view preferences | agrees - the window's frame is kept in the same file; each write keeps the other's key and replaces the file whole, through a temporary file |
| Section 6, by width | Under 760 pixels the viewer takes its narrow layout | agrees - the app's smallest window is 760 by 480, so the Graph keeps its title and wide layout there; the narrow layout stays the browser's |
| Section 29.4, 29.5 and 29.6 | Settings does not reopen its last pane; the sidebar group is renamed by step 50 | revised - it reopens its last pane, and the group's heading is removed |

No row needs a principle changed.

**Behaviour.**
- **One group, no heading.** The sidebar lists its sections with no group heading while it has one
  group.
- **A count that says what it counts.** AI apps' count reads "2 connected", counting every app with
  an entry as the tray does, in the caption size of the reading face. While the apps could not be
  read, there is no count.
- **AI apps' dot.** The section shows its dot, read as "needs attention", whenever no app has an
  entry, a row needs the person, or the apps could not be read.
- **The last pane.** The gear and Cmd+, reopen the pane last shown, however it was opened, which the
  page keeps in its own storage. A link that names a section opens it. The tray's Settings... opens
  the first section whose dot shows - Server, AI apps, This Mac, in that order - and the last pane
  while none does; the tray reads no sync health, so Sync's dot is the sidebar's to show.
- **The window keeps its frame.** The main window keeps its size and place across launches, in the
  app's view preferences beside the text's size, written when it closes and when the app quits. It
  reopens there while the top centre of its title bar is on a screen now, its size cut to that
  screen's; otherwise, and at the first launch, it opens at 1280 by 860 centred on the main screen. A
  full-screen or minimized window keeps the frame it had before, and reopens windowed. It cannot be
  made smaller than 760 by 480.
- **A tall sheet scrolls.** Settings' sheets hold within the window and scroll inside it.

**What this step does not do.**
- It does not keep the frame through a crash: the frame is written when the window closes or the app
  quits.
- It does not read sync health in the tray.

**Guarded by** `the_window_and_the_sidebar_keep_their_place` in the app's window tests (a frame kept
only while on a screen, the smallest window, one preferences file written whole, the tray's
Settings... and its first section with a dot, and the sidebar's heading, count, dot and last pane);
browser checks of Settings at 760 by 480 and 200% text measure that the count fits and a tall sheet
scrolls to its buttons.


### 29.15 Step 51a: All as a view

**What was found.** By the consistency, closure and usability audits of 2026-10-22, each verified by
running a scratch store:
- **`*` means three things.** The viewer files an act taken under All in the node default
  workspace; the remote surface refuses `*` for a write; the local MCP surface takes it for a
  workspace literally named `*`, which `observe` then creates.
- **One proposal reads differently in two scopes.** A verdict cast under All lands in the node
  default, where its proposal does not live: the proposal stays Open in its own workspace and reads
  Merged under All, and the two maps differ. The tray opens Review under All, so this is the common
  path. A merge proposed from All is filed where neither entity is, and its Accept is refused. This
  breaks proposal-workflow.md I2 and proposal-workflow.md Section 3.1, and store-migration.md
  Section 6: the union of the scoped views is the unscoped view.
- **Name this region under All crosses a sharing boundary.** It asserts the region and its members'
  names in the node default, so a name from a workspace a principal may not read becomes readable
  in one it may (P17).
- **A merge across workspaces is invited, and belongs to no canon.** The `curate` prompt asks for a
  merge of each duplicate its digest lists, those across workspaces included; such a merge is
  Blocked in its own workspace and Merged only under All.
- **A verdict on an unknown proposal id is taken,** on every surface, and the log keeps its row.
- **A gate act can name another workspace's observation.** A claim promotion filed in one workspace
  is checked against the whole log, so its grant reaches another workspace's observation under All
  alone.
- **Review's evidence under All reads the node default,** so a merge's sources there count 0.

**Decisions.** The author decided (2026-10-22): an entity merge joins entities of one workspace, on
every surface, and identity across workspaces is an explicit alignment assertion, as a type's is; a
verdict counts only in its proposal's workspace, and one filed elsewhere is listed in Health; and
this step comes before the same name (51b) and Same as... (51c).

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P15 | Resolution is the substrate's job, and conservative | revised, tightening - resolution stays inside a workspace: a merge joins entities of one workspace, and the same name in two is two entities until an alignment assertion says otherwise. principles.md records the revision, and the registry gains its clause |
| P11 | Type connections across workspaces only through an explicit alignment assertion | agrees - entities follow the same rule |
| P17 | A workspace is the sharing unit; a node-wide read exists only on the local trust surface | repays - no act taken under All writes into a workspace other than its item's, so Name this region copies no name across |
| proposal-workflow.md I2 and proposal-workflow.md Section 3.1 | A proposal's state folds the events that belong to it; a canon is a workspace's | revised - the events that belong to a proposal are those filed in its own workspace, and All is the union of the workspaces' folds, not a canon |
| proposal-workflow.md Section 6 and resolution.md Section 5 | Referential integrity: what the payload names, an entity or a gate act's observation, exists in the local log, also at capture for a gate act | revised - in the proposal's own workspace, whatever scope reads it, at capture as at the fold |
| unmerge.md Section 9 | A split's target is a decided merge in the local log | revised - in the split's own workspace |
| proposal-workflow.md I16 | Merged is absorbing | revised - a proposal an earlier build filed away from what it names stops acting: a merge across workspaces, a gate act on another workspace's observation, and a split filed away from its merge read Blocked, where under All they had read Merged. In its own workspace each already read Blocked or acted on nothing, and All was never a canon. The author's store holds none: its four gate acts name their own workspace, no entity spans two, and each of its 100 verdicts is filed with its proposal |
| store-migration.md Section 6 | The union of the scoped views equals the unscoped view | repays - for proposals, for forwarding, and for every projection built on them |
| P16 | Equal logs read equally | agrees - the fold is still a function of the log, now one workspace at a time |
| P5 and P21 | Unknown is said and never reads as success; the surface is narrow and legible | repays - a verdict that names no proposal of its workspace is refused with the reason, and `*` means one thing on every surface |
| P24 and P16, and the capture policy of propose | Refuse only when proceeding is worse; an act is not refused because something has not synced yet | agrees - a verdict here is cast on what this node lists. One naming no proposal of its workspace here was reviewed nowhere, and kept, it would decide nothing until a proposal arrived unseen, so proceeding is worse. A comment and a withdrawal are refused alike: each would be a row about nothing. A replicated verdict still folds whenever its proposal arrives (I8, I11), and on the local surfaces a merge's target, or a split's merge, that has not arrived is still taken; on the remote surface an absent one answers as one elsewhere, as a gate act's target already did (remote-viewer.md Section 3.4) |
| P2 and P18 | A proposal's reason is true and not shaped by the client | agrees - `/api/propose_merge` refuses a suggestion source it does not know, rather than recording the merge band's reason for it |
| U18 | Settle is filed in the workspace of the observation it changes | agrees - every other console act now follows it |
| remote-viewer.md Section 3.4 | On the remote surface an id outside the grants answers as an unknown one | agrees - the remote surface checks a merge's targets as it checks a gate act's, with one sentence for absent and elsewhere, so the engine's sentence that names the other workspace reaches only the local surfaces; and the stray verdicts are computed only for the node-wide report, which the read tier never builds |
| Section 7 | Every act is filed in the workspace of the observation it changes, written for Settle | revised - every console act is filed where its item lives |
| Section 7, Health | Health's signals | revised - under All it lists verdicts filed away from their proposal, while the proposal still waits |
| Section 29.7 | A check reads its passing words; a failed one adds the engine's detail | revised - a failed check reads its own words, so it never reads as passed |
| prompts.md Section 4 and compatibility.md Section 7 | The digest and the MCP surface's changes are listed | revised - no tool's input changes; the answers that do are listed there |

No other row needs a principle changed.

**Behaviour.**
- **`*` is a view on every surface.** A read with `*` reads every workspace the surface may read,
  and a write is filed in one. On the local MCP surface `observe`, `propose` and `define_type`
  refuse `*`, "observe writes to one workspace - name it, or leave it out for default", as the
  remote surface does. `search_knowledge` and the graph, hypergraph and types resources, which took
  `*` for a name, take it for every workspace; `workspace_map`, `list_proposals` and
  `get_proposal` already did, and `traverse` reads by id.
- **A verdict is filed where its proposal lives.** The engine refuses a verdict, a comment or a
  withdrawal that names no proposal of the workspace it would be filed in: "no proposal 3f2a... in
  workspace infra - a verdict is filed in its proposal's own workspace, which list_proposals
  names". To the engine an omitted workspace is still the node default. On the local surfaces, the
  local `review` tool an omitted workspace or `*` means the proposal's own, and on the console `*`
  does; there an id this node holds nowhere is refused, "no proposal 3f2a... in any workspace". On the
  remote surface the principal's workspace is named, as now.
- **A verdict counts only in its proposal's workspace.** Under All the fold runs in each workspace
  and the results are joined. A proposal then reads the same in its workspace and under All, its
  checks read its own workspace, and forwarding under All is the union of each workspace's. A verdict
  filed in another workspace decides nothing.
- **Health lists it.** Under All on the local console, Health lists each verdict or withdrawal
  whose proposal this node holds in another workspace, while that proposal is still Open or
  Blocked: "A verdict filed away from its proposal: Merge Postgres into PostgreSQL", saying "This
  accept was filed in default, where the proposal does not live, so it decides nothing. The
  proposal still waits in infra." with Show the proposal, which selects it on Review. A verdict whose proposal this node does not hold is not
  listed: it may be waiting for its proposal to arrive. The list is part of the node-wide report
  only, so no single workspace's report, and no remote reader, carries it.
- **A proposal filed away from what it names** reads Blocked, its check saying what is missing
  from its workspace; it is rejected, and opened again where its targets live.
- **Every console act is filed where its item lives,** under All as in one workspace:
  - Accept and Reject in the proposal's workspace;
  - Propose on a suggestion in its pair's workspace. A pair in two workspaces is refused, "The
    console opens no merge across workspaces.", and so is a suggestion source the server does not
    know; an absent source is the merge band, as now;
  - Propose a split in its merge's workspace;
  - Name this region in the workspace of the observations that asserted the region;
  - Settle in its observation's, as now.
- **A proposal acts within its own workspace.** `propose` refuses an `entity_merge` naming an
  entity of another workspace than the one it is filed in, and on the local surfaces says which:
  "entity_merge joins entities of one workspace: Payment service is in atlas-web, and this proposal
  is filed in infra. The same name in two workspaces is two entities (Principle 15)." A merge's
  target the log does not hold yet is still taken, and blocks until it arrives, as now. A gate act
  naming another workspace's observation is refused the same way, and so is a split filed away
  from its merge. On the remote surface every target must be in the principal's workspace, said in
  one sentence for absent and elsewhere.
- **A proposal names its workspace.** `list_proposals` and `get_proposal` carry `workspace`, and
  their descriptions list `blocked` among the states.
- **The `curate` prompt** lists the same name in two workspaces as two entities for the person, and
  drafts no merge across workspaces; a proposal is filed in its targets' workspace, never `*`.
- **Review's evidence under All** reads every workspace, so a merge's sources are counted.
- **A failed check reads its own words** on Review: "Not every entity it names is in its
  workspace", never the passing "All 2 entities are in its workspace" followed by the engine's
  detail. The engine's detail says "not asserted in this proposal's workspace" where it said "not
  asserted in the local log", which named no other workspace and still does not.
- **Suggestions' line** reads "Possible duplicates the store found, and names two workspaces share.
  Proposing a duplicate opens a merge that waits under Needs you."
- **The engine's notes** on the curation report and on an entity's workspace say the same name in
  two workspaces is two entities, never a merge to propose.

**What this step does not do.**
- It does not show namesakes on the sheet or light them together (step 51b), nor offer Same as...
  and Keep ... instead (step 51c).
- It does not build the alignment assertion that says two entities of two workspaces are one; it is
  owed, and its registry clause names the trigger.
- It does not echo the workspace in every write's answer, warn of a new workspace name, or name the
  workspace on search hits; those are the agent surface's step after step 53.

**Guarded by** `p15_a_merge_never_crosses_a_workspace`,
`p15_a_gate_act_names_only_its_own_workspaces_observations`,
`p15_a_split_names_only_its_own_workspaces_merge`,
`p15_the_all_workspaces_view_is_the_union_of_each_workspaces_fold` (a proposal reads under All as in
its workspace, nothing merges across, a stray console accept raises no tier under All, and the stray
accept is listed in the node-wide report only),
`p24_a_verdict_naming_no_proposal_of_its_workspace_is_refused` and
`p17_a_region_named_under_all_is_filed_where_it_was_asserted` in the engine's policy cases;
`every_act_under_all_is_filed_where_its_item_lives` over the viewer's socket;
`under_all_the_console_files_where_items_live` in the viewer's review tests; and
`star_is_a_view_on_the_local_surface` in the agent surface's tests (and, until 0.11.0 removed the
remote surface, a remote merge naming only entities of its own workspace).

### 29.16 Step 51b: the same name

**What was found.** Measured on the Graph and Review pages:
- **The same name in two workspaces reads as two unrelated things.** "Payment service" in infra and
  in atlas-web sit apart on the map, and neither the sheet nor the map says the store noticed the
  shared name.
- **The Suggestions row for it says little.** "Same name in two workspaces: payment service" shows
  the name as the key it is grouped by, in lower case; its chips name the same entity twice with
  nothing to tell them apart; and nothing says what the shared name means now that a merge cannot
  join them (Section 29.15).

**Decisions.** The author approved D9 - the same name across workspaces is said - and decided
(2026-10-22) that it is said on the local console only.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P15 and Section 29.15 | Resolution stays inside a workspace; the same name in two is two entities | agrees - the shared name is said, and nothing offers to merge it |
| P17 and remote-viewer.md Sections 3.2 and 12 | A node-wide read exists only on the local trust surface; the read tier's union is never computed across workspaces | agrees - the sheet's line and the lighting are the local console's alone, and the read tier refuses `/api/namesakes`, saying where the same name is shown |
| P5 and inspector.md D3 | A read that could not answer is said, never shown as nothing | agrees - on the local console a failed read, or a workspace list that did not load, is said on the sheet. The read tier has no comparison at all, so no line is ever missing there: it says what it reads, each workspace alone, in remote-viewer.md Section 12 |
| Section 29.9 | A hover lights without dimming the rest; dimming stays with a selection | agrees - a hover lights the drawn namesakes in the hover's set, and a selection lights them with its neighbours |
| U37 and U38 | The map does no work per frame that a load can do; what a selection lights keeps its strength | agrees - the namesakes are indexed when the graph loads. A selection's namesakes are lit as its neighbours are, so they do not fade; a hover's fade by rank, as a hover's neighbours do |
| Focus framing, the viewer's rule | Focus frames exactly what the selection lights | agrees, with one exception the code says - focus frames the entity and its neighbours, not its namesakes, which can sit across the map |
| Section 29.4 and Section 29.11 | A change of scope starts the selection history afresh and ends the working set | agrees - Show changes the scope to All only when it is not already All, as any change of scope does, so Back does not return to the entity left; under All it selects in place, and Back returns |
| Section 7, Suggestions | A row says what it is and offers its act | revised - the shared-name row is titled by the name as written, its chips say their workspace, and a line says what the shared name means; it keeps Show on the map |
| Section 9 | System terms stay out of a line read first | agrees - the Suggestions line names no principle |
| Boundaries' Workspace line, the viewer's words | A line says what is drawn, and is true | agrees - it said "no entity is in two", which a merge across workspaces could make untrue; since step 51a none can, and the line says what the shared name means |
| inspector.md Section 5 | The sheet's rows | revised - it gains Also named in, after the workspace row; its workspace row's "sits in more than one through a merge" can no longer arise |
| U17 | No identifier in a line read first | agrees - each entity is said by its name and its workspace, and no answer spells an id |
| U19 | Every act is a control with a name | agrees - Show is a button named with the entity it shows |

No row needs a principle changed.

**Behaviour.**
- **Also named in.** On the local console, the sheet of an entity that another workspace has an
  entity by the same name for says, after its workspace line, "Also named in atlas-web: Payment
  service (Service)", with Show; with several, one line each, in workspace order.
  - The namesakes come from `/api/namesakes?entity=<id>&in=<ws>&in=<ws>...`, one `in` for each
    workspace the page lists. Each costs one lookup of the id the entity's name has there, the same
    trimmed, lower-case name the id is made from, and each namesake found costs the fold of its own
    workspace's merges: a namesake its workspace merged into another entity is left out, as the
    Suggestions row leaves it out. The sheet reads once per selected entity and keeps the answer
    while the graph does not change, so a poll re-draws it without reading again. Nothing shows
    while the read is in flight, and an answer for an entity no longer selected is dropped.
  - If the read fails, the sheet says "Not compared with other workspaces:" and why; if the page's
    workspace list did not load, "Not compared with other workspaces: the workspace list did not
    load". The page makes no read until it knows it is on the local console, and none for an entity
    whose workspace is empty, which since step 51a cannot arise. An entity the server does not hold
    answers 404, "no entity by that id".
  - Show selects that entity. Under All it selects in place; otherwise it goes to the address
    `?workspace=*&entity=<id>`, so the selection waits for All's graph.
- **Lit together.** Under All on the local console, the drawn entities of other workspaces with the
  same name light with what is pointed at - in the hover's set, without dimming the rest - and with
  what is selected, as its neighbours do. They are indexed by the page's lower case of the trimmed
  name when the graph loads, which can differ from the server's rule on rare characters; the sheet's
  line is the server's.
- **Suggestions.** The shared-name row reads "Same name in 2 workspaces: Payment service", with the
  count of workspaces and the name as its first member writes it. Each chip keeps its type and says
  its workspace, "Payment service - infra", read from the graph the page holds; a member the page
  does not hold shows its name alone. Its line says "Each workspace keeps its own entity by this
  name, and a merge cannot join entities of two workspaces." It keeps Show on the map, which frames
  them with its way back to Review.
- **Boundaries.** The Workspace line reads "A workspace outline around each workspace: a sharing
  unit. The same name in two workspaces is two entities".
- **The read tier** refuses `/api/namesakes`: "the read tier compares no workspaces - each is read
  alone; the same name is shown at a node's own console (docs/remote-viewer.md Section 12)".

**What this step does not do.**
- It does not offer Same as... or Keep ... instead, which are step 51c.
- It does not compare names on the read tier.
- It does not look up aliases: a name is matched by the id's rule, as the Suggestions row matches it.

**Guarded by** `p15_namesakes_are_the_same_name_standing_in_other_workspaces` in the engine's policy
cases (workspace order, a merged-away namesake left out, an unknown id unknown);
`the_console_reads_the_same_name_in_other_workspaces` over the viewer's socket (one `in` per
workspace, a comma inside a name, the unknown entity unspelled, the read tier's refusal); and
`the_same_name_is_said_on_the_local_console` in the viewer's review tests.

### 29.17 Step 51c: Same as..., and Keep ... instead

**What was found.** Measured on the Graph and Review pages:
- **A person cannot say two entities are one.** The console opens a merge only from a Suggestions
  row, so "these are the same thing" is possible only where a detector already thought so;
  otherwise the person has to ask an agent.
- **A merge cannot keep the other name.** "Merge PostgreSQL into Postgres" can be accepted or
  rejected; keeping PostgreSQL takes a reject and a new proposal from an agent.
- **What the console opens reads as an agent's.** Who asked on a merge proposed from Suggestions or
  an un-merge reads "Agent - this machine", because only a verdict carries the console's mark.

**Decisions.** The author approved D10 - a person may open a merge from the console, its reason
fixed - and D19 - Review's merge offers "Keep PostgreSQL instead", which ends the merge and opens
the reverse one. Since step 51a both are inside one workspace.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P23 and proposal-workflow.md Section 14.3 | The console writes only through the gate - a verdict, a proposal opened, an observation ingested - and a decision acts on one item with its diff on screen | agrees - Same as... opens a proposal, which decides nothing, and Review shows its diff and checks before anything is decided. Keep ... instead is a reject of the item on screen, once its diff and checks are on screen, and a proposal opened |
| P23's solo exception and proposal-workflow.md Section 10 | Self-approval is permitted alone, and marked | agrees - accepting a merge the person opened at the console is the solo exception, and the verdict is self-attested as every console verdict is |
| P2 and P18 | Provenance is true and not shaped by the client | repays - a merge or split the console opens carries the console's mark, stamped by the engine as a verdict's is, so it reads as the person's; its reason, which says why and not who, comes from the console's fixed list. A confirmation's reason stays its notice sentence (Section 29.3) |
| resolution.md R8 and Section 6 | The surface marker on a verdict is engine-stamped, never client-supplied | revised - the console's own server also asks the engine to stamp it on the merges and splits it opens; a client-supplied source naming it is still refused, and the mark on an opening event grants nothing, since the ceiling and the recall check read verdicts only |
| P24 | Refuse only when proceeding is worse; a half-done act is worse | agrees - Keep ... instead opens nothing and rejects nothing until it knows the reverse's state, and rejects only once the reverse is there |
| Principle 14 and unmerge.md S10 | A proposal is its content, so a re-merge after an un-merge must differ from the merge reversed | agrees - the console's re-merge reason says how many un-merges came before, which the server counts from the log, so a merge, un-merge and merge again work from the console |
| P6 | Contradictory merges are surfaced, never silent | agrees - the console offers no Accept of any kind on a merge whose reverse is accepted, so it opens no cycle itself; one an agent makes still shows in Health as a merge cycle |
| Section 29.15 | A merge joins entities of one workspace | agrees - Same as... searches only the entity's own workspace |
| U15 | A search names its scope, chooses nothing before the person moves, and offers the wider scope when nothing matches | revised - the picker names its scope and chooses nothing unmoved, but offers no wider scope, since a merge stays in one workspace |
| U19 | A dialog holds focus; every act is a control; a disabled control says why | agrees - the picker holds focus, its two merges are buttons, and Keep ... instead is disabled with why beside it until the diff and checks are on screen |
| U4 and U25 | A decision acts on one item once its diff is on screen; what an act does is said once on screen | agrees - a line under the acts says what Keep ... instead does |
| Section 7 | Proposal controls and their keys; the next item is selected; a rejected proposal offers Accept after all | revised - Keep ... instead joins the controls with no key; after it the reverse is selected; Accept after all is not offered while the reverse is accepted |
| Section 29.7 | Who asked reads a console verdict as "you at this console" | revised - a merge or split opened at the console reads so too; Who asked is the first opener's, so an identical merge an agent opened first stays the agent's, and proposals opened before this step keep reading as the machine (P3) |

No row needs a principle changed.

**Behaviour.**
- **Same as...** On the local console, the sheet of an entity offers "Same as...", after Show
  related.
  - It opens a dialog with a search of the entity's own workspace, "Search infra", listing its
    entity hits as the search list groups them, leaving out the entity itself. The arrow keys move
    between rows and Enter chooses the row moved to; Enter with no row moved to does nothing. With
    nothing else in the workspace it says "Nothing else is in infra yet"; with no match, "No entity
    in infra matches", and offers no wider scope.
  - Choosing a row offers two buttons, "Merge Postgres into PostgreSQL" and "Merge PostgreSQL into
    Postgres". Either opens the merge through `/api/propose_merge` with `src=picked`, closes the
    dialog and selects the merge on Review in the scope on screen, where its diff and checks show
    before any decision. If a merge of the two is already open, in either direction, it opens
    nothing and selects that one: "A merge of these two is already open." If the server refuses,
    the notice says "Not opened:" and why.
  - Esc closes the dialog from anywhere in it and changes nothing; focus returns to Same as....
- **Keep ... instead.** On the local console, an open merge of two entities on Review offers "Keep
  PostgreSQL instead", naming the one the merge would fold away, after Reject. It has no key. Until
  the diff and checks are on screen it is disabled, with "Waiting for the checks" beside it. A line
  says "Keep PostgreSQL instead rejects this merge and makes PostgreSQL the one kept; the merge into
  it then waits for your verdict."
  - The reverse is any merge of the same workspace with the same two entities and PostgreSQL kept,
    whatever its reason, read from the proposals the page holds before anything is written.
  - With no reverse, it opens one through `/api/propose_merge` with `src=reverse`, `a` the entity
    this merge would keep and `b` PostgreSQL, and only once it is open rejects this one, selecting
    the reverse: "Kept PostgreSQL: the merge into it waits for your verdict."
  - With an open reverse, it rejects this one and selects the reverse, with the same notice.
  - With an accepted reverse, it rejects this one: "Kept PostgreSQL: it was already merged into
    it."
  - With a rejected reverse, it rejects this one and selects the reverse on Decided, where Accept
    after all is: "Rejected. The merge into PostgreSQL was rejected before; accept it after all
    there." A withdrawn reverse counts as none.
  - If the reverse cannot be opened, nothing is rejected: "Not changed: the merge into PostgreSQL
    could not be opened:" and why. If it opened and the reject failed: "Opened the merge into
    PostgreSQL; this one was not rejected:" and why.
- **No cycle from the console.** A merge whose reverse is accepted offers no Accept and no Accept
  after all; its item says "Its reverse is accepted", and focus stays where it was.
- **Reasons.** `/api/propose_merge` takes `src=picked`, "two entities named as one from a search",
  and `src=reverse`, "the other name kept, instead of a merge the other way". When the log holds
  un-merges of the same two entities in the same direction, the server adds ", again after 1
  un-merge" or ", after 2 un-merges", so the re-merge is a different proposal (unmerge.md S10). An
  unknown `src` is refused, an absent one is the merge band, as before.
- **The console's mark.** A merge or split the console opens - from Same as..., Keep ... instead,
  Suggestions or Un-merge - carries the console's mark, so Who asked reads "Agent - you at this
  console", the tier first as every who-line has it, and "a person at" its machine elsewhere.

**What this step does not do.**
- It does not merge across workspaces (Section 29.15).
- It does not preview a merge's diff before it is opened; Review shows it once it is.
- It does not offer Keep ... instead on a blocked merge, or on a merge of three or more entities.
- It does not mark a confirmation's proposal: its reason is its notice sentence, and Who asked on it
  reads as before.

**Guarded by** `p2_a_merge_opened_at_the_console_carries_its_mark` and
`s10_the_console_counts_the_unmerges_a_re_merge_follows` in the engine's policy cases;
`the_console_opens_marked_merges_that_can_follow_an_unmerge` over the viewer's socket (the two
reasons, the mark on a merge and an un-merge, and a re-merge after an un-merge); and
`same_as_and_keep_instead_hold_their_order` in the viewer's review tests (the picker's scope, Keep's
read, open and reject in that order, and no Accept while the reverse is accepted).

### 29.18 Step 52: the map on a phone

**What was found.** Measured at 390 by 844 with touch:
- **A finger does not move the map.** One finger dragged 100 pixels leaves the camera where it was,
  and two fingers spread zoom the whole page 3.85 times, not the map.
- **The map gets a third of the screen.** On a first visit the Log opens as a sheet over 60 percent
  of the map, because both panels open by default and a phone takes the default as a wide window
  does.
- **The camera buttons cover the labels.** At the top right since Section 18's step 2, wrapping onto
  two rows since step 49, they sit over the names near the top of the map.

**Decisions.** The author approved D16: a phone opens on the map, not on the panels a desktop
would. A phone here is a window under 760 pixels wide, or a touch screen whose shorter side is under
600 pixels, so a phone held sideways is one too.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| WCAG 2.5.1 | A multipoint gesture has a single-pointer alternative | agrees - pinch and two-finger pan have the zoom buttons, Fit and one-finger pan |
| WCAG 2.5.7 | A drag has a single-pointer alternative | agrees - panning has the camera buttons and Fit; moving an entity arranges the drawing and is not one of the page's functions, and Reset layout undoes it |
| WCAG 1.3.4 | Nothing locks the orientation | agrees - a phone held sideways is a phone too |
| WCAG 1.4.4 and 1.4.10 | Text resizes and the page reflows | agrees - only the map takes the touches, and while the page is zoomed in the map leaves pinches to the browser, so the page can always be zoomed back out |
| Section 6, by width and the right side | Below 760 the panel is a sheet from the bottom; a panel's choice is remembered | revised - a phone opens with no panel open, and a panel opened or closed on a phone is not remembered, so the same browser in a wider window keeps its choice |
| Section 18's step 2 and Section 29.12 | Under 760 the camera buttons sit at the top right, framing leaving out their box at the top | revised - on a phone they sit at the map's foot above the open sheet, and framing leaves out their box there |
| inspector.md Section 6 | A 36vh cap for the inspector, taller on request | revised - on a phone the sheet is a third of the visible screen, and taller it is four fifths, never more than the screen less the header, the status bar and the camera buttons; the panels' sheets stay as they are |
| Section 10, reduced motion | Under reduced motion nothing slides | agrees - the camera buttons jump to their place, and a double tap's zoom jumps, as the buttons' does |
| Section 29.9 | A hover lights what the pointer is over and shows its card | agrees - a finger has no hover, so a touch shows no card |
| Section 29.4 | A click on an entity selects it; on the ground, clears the selection | agrees - a tap is a click |

No row needs a principle changed.

**Behaviour.**
- **Touch on the map.** A touch is read from pointer events of the touch kind, which the map takes
  for itself so the browser sends no mouse events after them; the mouse keeps its own handling.
  - One finger pans, on the ground or on an entity. Held still on an entity for half a second, it
    picks the entity up and moves it. A finger reaches an entity within 22 pixels of its centre.
  - A tap - a touch that moved less than 10 pixels - selects or clears as a click does.
  - A double tap on the ground zooms in twice as close around it, easing as the buttons do; on an
    entity it selects it once and does not clear it.
  - Two fingers pinch to zoom around their midpoint and move together to pan, 1:1 under the
    fingers; a second finger landing while an entity is being moved lets it go where it is. A third
    finger is ignored, and a cancelled touch ends the gesture with nothing selected.
  - While the page is zoomed in, pinches go to the browser.
- **The map first.** A phone opens with both panels closed, whatever the default or a wider window
  stored, and a panel opened or closed on a phone is not remembered. An address that names an entity
  still opens its inspector, and one that names Review still opens Review.
- **The sheet's two heights.** On a phone the inspector's sheet is a third of the visible screen, and
  taller it is four fifths, but never more than the screen less the header, the status bar and the
  camera buttons. Its Taller and Shorter control is drawn at the sheet's top centre as a handle, with
  the same name and state; Close and the way back stay where they are.
- **The camera buttons** sit at the map's bottom right on a phone, above the open sheet, the panel
  or the strip, and wrap upward. They follow the sheet as it opens, grows or slides, and jump there
  under reduced motion. The View popover opens above them. Framing leaves out their box there.

**What this step does not do.**
- It does not resize the sheet by dragging its handle: a tap toggles the two heights.
- It does not carry a pan on after the finger lifts, or rotate the map.
- It does not change the 760 to 1200 pixel band, where under a coarse pointer the camera buttons and
  the inspector already overlap by a few pixels; that is owed to a later pass.

**Guarded by** `a_finger_moves_the_map_on_a_phone` in the viewer's key tests (the touch handling and
its thresholds, the cancelled down, a phone's panels neither opened nor remembered, framing's foot,
the sheet's heights and the toolbar above it); browser checks in Chromium with touch at 390 by 844
and 844 by 390 measure one finger panning 80 pixels, a pinch zooming the map and not the page, a tap
selecting, a double tap on the ground zooming twice as close, the sheet a third of the screen with
the toolbar 8 pixels above it, and both panels closed on opening.

### 29.19 Step 53: time on the map

**What was found.** Measured on the Graph page (R2-08):
- **The map has no time.** Nothing on it says what is new or what has not been said in months; an
  entity said a minute ago and one said last spring are drawn alike.
- **Coming back answers nothing.** A person who opens the map after a week cannot see what agents
  recorded meanwhile except by reading the Log row by row.
- **The read has no time to give.** The graph's nodes carry their sources, origins and tier, and no
  time, so the page could not show one.

**Decisions.** R2-08's recommendation holds - a time per entity and a filter of today, the last 7
days and since the last visit - with these, by the recommended options the author approved for the
UI: one word for the time, "said", since a merge or a confirmation says nothing about an entity; a
visit that ends only after half an hour away, so the app's reloads do not end it; and no last visit
on the read tier, which remembers nothing (Section 5).

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P16 | Equal logs read equally | agrees - an entity's time is the latest `observed_at` among the attestations its rows hold, its merged-away rows' included. `observed_at` is signed with the attestation and kept by every stamp upgrade, backfill and rekey, so it converges at replay as the rows do; the clock is never read for it. An attestation from a peer whose clock runs ahead counts as said when it says, so it can read as said today until that time passes |
| P4 | Two time axes: when it was true and when it was recorded | agrees - this is the recording axis, and no valid time is read or written |
| P17 and remote-viewer.md Sections 3.3 and 12 | The read tier answers within the grants; its union is merged | agrees - the field rides each workspace's graph, which the tier serves only for a granted workspace, and the union concatenates nodes, each in one workspace since step 51a |
| compatibility.md Section 7 | The MCP surface's changes are listed | revised - the graph and hypergraph resources gain an optional `last_said` on each node; resources have no golden file, so the row says so |
| Section 5 | The read tier remembers nothing; the local console keeps a convenience keyed by node and workspace | agrees - the last visit is kept on the local console only, keyed by the node's id and the scope as the layout is, and the read tier offers no "since your last visit" |
| D5, Section 29.9 and Section 6 | A trust filter dims rather than hides, and the status bar says what is dimmed | revised - the Said filter dims as a tier does, and the status line says "said before today dimmed" |
| Section 29.9's D3 | A count is what is drawn | agrees - each Said choice counts the entities drawn, hidden types left out, and dimmed ones stay counted in the status line |
| U17 and Section 9 | One state keeps one word; a time is said in words a person reads | agrees - "said" everywhere, and a time as the Log says its times, which gain the year when it is not this one |
| U24 | List is the map for a screen reader | revised - List gains a Last said column, sortable, since dimming does not reach a table |
| U19 | Every control is the kind its act is and says its state | agrees - the choices are a radio group with their counts; one with nothing to offer is disabled and says why |
| inspector.md Section 5 | The sheet's rows | revised - it gains Last said |

No row needs a principle changed.

**Behaviour.**
- **An entity's time.** Each node of the graph and of the hypergraph carries `last_said`, the latest
  time any attestation its rows hold was observed, its merged-away rows' included, in epoch
  milliseconds; a node no attestation supports has none.
- **Said where the entity is.** The hover card adds "Last said 9:41 AM", the sheet says it under its
  type, and List has a Last said column, in the Log's form: the time today, the month, day and time
  this year, and the year too before. A re-observation counts as said again, though Evidence lists
  the observation once, at its first time. A node with no time shows no line.
- **Said.** The filter panel gains a Said section after Trust, a radio group: "Any time", the
  default, "Today", "Last 7 days" and, on the local console, "Since your last visit", each with the
  count of drawn entities it keeps. A choice dims every entity not said in that window, as a dimmed
  tier does, and the relations it touches unless they are lit; the status line adds "said before
  today dimmed" (or "said before the last 7 days", or "said before your last visit"). Rank fading
  applies as everywhere (U38).
  - Today starts at local midnight; the last 7 days are the 7 times 24 hours before now. The windows
    move every minute while the page is open, and a chosen window that keeps nothing stays chosen
    with its 0.
  - A visit ends after half an hour with the page unseen. The last visit for a scope is when this
    browser last showed that scope before this visit, kept per node and scope, so a reload in the
    middle of a visit keeps it. With no earlier visit the choice is disabled with "No earlier visit
    here".
  - The choice is not stored: every visit opens on Any time.

**What this step does not do.**
- It does not show valid time, when a fact was true in the world (P4's other axis).
- It does not time relations, or replay the map as it was at a past moment.
- It does not know when knowledge arrived: a time is when it was said, so something synced after the
  last visit but said before it is not "since your last visit".

**Guarded by** `p16_an_entity_is_last_said_at_its_latest_attestation` in the engine's policy cases
(the latest of two observations, a merged-away row's time counted, the graph and the hypergraph
agreeing) and `the_map_says_when_each_entity_was_said` in the viewer's key tests (the Said section
and its radio group, the read tier's missing last visit, the visit's half hour, the status line, the
relations dimmed, and the hover card, the sheet and List saying the time).

### 29.20 Step 54: the agent surface says where

**What was found.** By the usability audit of 2026-10-22, run as an agent over stdio, and its review:
- **A write in the wrong place reads as success.** `observe` with no workspace, or with a typo -
  `acme_api` beside `acme-api` - answers only ids, and the typo becomes a workspace of its own.
- **Hits cannot be told apart.** `search_knowledge` spans every workspace, but two entity hits named
  "PostgreSQL" carry only an id, a snippet and a score.
- **A name passed as an id reads as absent.** `get_entity` with "PostgreSQL" answers "unknown - not
  found is not a negation", though the entity exists, and says nothing about ids.
- **A failure reads as a result.** A refused or failed local call comes back as an ordinary result
  holding an `error` object; only the remote surface's own refusals are flagged.
- **A promotion over-reports.** A claim promotion to human_confirmed that an agent merges is granted
  host_signed, the agent surface's ceiling, yet `get_proposal` reports only the tier it asked for.
- **A demotion is capped, against resolution.md Section 6.** The fold caps a merged demotion as it
  caps a promotion, so an agent's demotion to human_confirmed lowers the source to host_signed,
  though Section 6 says a demotion needs no ceiling. The author's store holds no merged demotion.

**Decisions.** None was needed: the audit's recommendations hold, and resolution.md decided the
demotion. Flagging a failure as an error is MCP's own convention for a tool error, so it is listed as
a changed answer, and the release note says so.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P5 | Not found, withheld and unavailable are distinct answers; a bounded answer says what it left out | repays - a refused or unavailable call is flagged as an error, a not-found id stays an ordinary `found: false` answer, and the new workspace's list says how many it left out |
| P21 and architecture.md Section 7 | A narrow surface whose answers say what to do next | repays - the answers name their workspace and their entities, and a new workspace's answer names the ones that hold knowledge |
| P17 and remote-viewer.md Section 3.4 | A remote answer names nothing outside the grants | agrees - the new-workspace note is the local surface's alone, since the remote one already refuses a workspace outside the write grants, and search hits come only from the grants; the name hint depends only on the id's shape |
| P18 and resolution.md Section 6 | A merged promotion grants at most its surface's ceiling; a demotion needs no ceiling | repays - a merged promotion says the tier it granted, and a merged demotion now lowers to the tier it asked for, as Section 6 says |
| P16 | Equal logs read equally | agrees - the nearest names are ranked by a stated rule with the name as the tie-break |
| compatibility.md Section 7 | Changes to the MCP surface are listed | revised - the added fields, the note, the error flag and the descriptions that change are listed there |
| architecture.md Section 7 | LLM-friendly response conventions | revised - it gains: a failure is flagged as an error, and a not-found answer stays a result |

No row needs a principle changed.

**Behaviour.**
- **Writes say where.** The answers of `observe`, `define_type`, `propose` and `review` carry
  `workspace`, the one written to - for `review`, the proposal's own, after step 51a resolved it. The
  MCP layer adds it, so the CLI's own output is unchanged.
- **A new workspace is said.** When an `observe` on the local surface writes to a workspace that held
  no observation before, its answer adds a note: "workspace acme_api is new; the workspaces that hold
  knowledge are acme-api, home-lab - observe there if this was meant for one of them". The names are
  ranked by the distance between their letters and digits, lower-cased, nearest first and then by
  name; ten at most, then "and 3 more". With no other workspace the note says "this node held no
  knowledge before". The check is one lookup of whether the workspace holds any observation, made
  before the write; the list is read only when the workspace is new.
- **Hits name their entity.** An entity hit of `search_knowledge` from this node carries `name`,
  `type` and `workspace`, read from the entity's projected row. A federated hit, from another
  server, is left as it is.
- **A name is said to be one.** When `get_entity` is given something that is not a 64-character hex
  id and finds nothing, its note adds "this looks like a name; an entity's id is the 64-character hex
  id search_knowledge returns - search for the name".
- **Failures are flagged.** A tool answer that is a top-level `error` object comes back with
  `isError` set, on the local surface as on the remote. An error inside a larger answer - one
  server's failure in a federated search or a sync round - stays inside it, unflagged.
- **A promotion says what it granted.** A merged claim promotion in `list_proposals` and
  `get_proposal` carries `granted_tier`: the tier asked for, capped by the highest surface among its
  merge verdicts, read by the same fold that applies it.
- **A demotion is not capped.** A merged demotion lowers its sources to the tier it asked for,
  whichever surface merged it.
- **Descriptions.** The tool descriptions of `observe`, `review`, `list_proposals` and
  `get_proposal` say the new fields. The server's instructions ask an agent to name the workspace
  after the project it works in - the repository's top-level directory name - and to keep using it.

**What this step does not do.**
- It does not change `list_proposals`' answer for an empty workspace, which stays an empty list:
  wrapping it would change its shape.
- It does not add the name hint to `traverse`, or the new-workspace note to `define_type`, whose
  vocabulary is defined on purpose rather than written by the way.
- It does not decide whether an agent may settle a person's conflict; the granted tier says what it
  did.

**Guarded by** `p18_a_demotion_lowers_to_the_tier_it_asks_for` and
`p18_a_merged_promotion_says_what_it_granted` in the engine's policy cases;
`a_write_says_where_and_a_new_workspace_is_said` and
`hits_name_their_entity_and_a_name_is_said_to_be_one` in the agent surface's tests (the workspace on
each write, the note and its nearest-first order, a known workspace saying nothing, named hits, and
the name hint by shape); and `a_failure_is_flagged_and_a_not_found_is_a_result` over the protocol
(the error flag, a not-found answer left a result, and the instructions' naming).

### 29.21 Step 55: the person finds what waits

**What was found.** By the usability audit of 2026-10-22, on a seeded store:
- **One stray entry pins the person to the default.** With one entry in the node default and the rest
  in acme-api, the page opens on the default's two entities, since it opens on All only when the
  default is empty.
- **Review says nothing waits while things wait.** In that scope Review says "Nothing waits for you in
  this workspace" while two proposals and a contested belief wait in acme-api, and the status bar's
  pill says only "Review", though the tray counts them.
- **What cannot pass waits for ever, one by one.** A Blocked item - accepted, but held by a check that
  fails - stays under Needs you until someone rejects it, one at a time. On the author's store 38 such
  merges, made before the store's ids changed, had to be rejected one by one.

**Decisions.** The author decided (2026-10-22) that what the system cannot process may be dropped.
Read here as a person's act: one reject for every Blocked item, which the log keeps. A Blocked item
carries its accept already, so if its check passes later that accept still merges it
(proposal-workflow.md Sections 7.1 and 8). The rest are the audit's recommendations, with the pill
keeping its one count of the scope (U3, U20) and saying what waits elsewhere only beside it.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U4 and Section 7 | A decision decides one item with its diff and checks on screen; there is no decide-all | revised, by the author's decision - one act rejects the Blocked items under Needs you in the scope on screen, each listed with the check that blocks it before the act is cast; a recall is left out, as its verdict is a person's direct act on that item (I17) |
| P23 and proposal-workflow.md Sections 7.1 and 8 | Canon changes only through a verdict; a reject is provisional | agrees - a reject changes no canon, and a Blocked item reaches none until its check passes, when the accept it carries still merges it |
| dreaming.md Section 5.5 | A person's reject of a published dream proposal is their no | agrees - each item rejected together is that person's no, as one rejected alone is |
| P5 | An empty answer in one scope is not absence | repays - a scoped Needs you says what waits in the other workspaces |
| U3 and U20 | The status bar counts the loaded scope's contested nodes; the pill's one count is the items under Needs you | agrees - the pill keeps its one count of the scope; it says what waits elsewhere only when the scope has none |
| Section 6 | The page opens where the knowledge is; the pill reads Review when nothing waits | revised - the page opens on All unless the default holds most of the knowledge, and a pill with nothing here says what waits elsewhere |
| Section 29.15 | All is a view | agrees - the counts read All, and every act is still filed where its item lives |
| U19 and U42 | A dialog holds focus; a notice that reports a failure stays | agrees - the dialog holds focus with Cancel first, and a partly failed act's notice stays |

No row needs a principle changed.

**Behaviour.**
- **The first view.** When the address names no scope, the page decides once, before its first
  draw, from the node's counts of entities by workspace: it opens on All when other workspaces hold
  entities and the default holds at most half of them, saying beside the chips "Showing all
  workspaces - default holds 2 of 40" (or, as before, "default is empty"). When the counts cannot be
  read, or no workspace holds an entity, it stays on the default.
- **What waits elsewhere.** On the local console the page reads what waits in every workspace - open
  and Blocked proposals, suspected secrets and contested beliefs - from the reads the tray makes, at
  most every 30 seconds while the page is seen, again after each console act, and from the scope's
  own reads when the scope is All. When the curation read fails, it counts the proposals alone; when
  the proposal read fails, it says nothing about elsewhere.
  - **The pill** keeps its one count of the scope. When the scope has nothing waiting and other
    workspaces do, it reads "3 wait elsewhere" and opens Review.
  - **Needs you**, in one workspace, opens with "3 more wait in other workspaces." and Show all
    workspaces whenever others hold any; with nothing here it reads "Nothing waits for you in this
    workspace. 3 wait in other workspaces." Show all workspaces turns the scope to All, keeps Review
    open and puts focus on the selected row.
- **Reject all blocked.** When Needs you holds two or more Blocked items other than recalls, its list
  opens with "4 were accepted, but a check holds each back." and Reject all blocked. That opens a
  dialog listing each with the check that blocks it, read for each, and the line "If a check passes
  later, the accept it carries still merges it." Cancel, first, and Esc close the dialog and change
  nothing, with focus back on Reject all blocked; while it is open the page's keys do nothing. Reject
  all rejects exactly the items listed, each where it lives, leaving out any whose state changed
  meanwhile or whose checks could not be read, and says "Rejected 4 blocked items."; when some fail it
  says "Rejected 3 of 4. 1 was not:" and why, and stays. Focus then goes to the selected row, or to
  the list when none is left. One Blocked item is rejected as before, on its own.

**What this step does not do.**
- It does not reject an open item whose check fails before any accept; its Accept is idle, and
  Reject decides it.
- It does not drop anything without a person's act: the system rejects nothing on its own.
- It does not count dream drafts, which the tray does not count either.

**Guarded by** `the_first_view_opens_where_the_knowledge_is` in the viewer's place tests (decided
once, before the first draw, at half the knowledge or less) and `the_person_finds_what_waits` in its
review tests (the tray's reads, the pill's words, the line in Needs you, Reject all blocked leaving
out recalls and items no longer blocked, and a partial failure said); browser checks seed a default
with one entity beside a workspace of five and two Blocked merges, and measure the page opening on
All, the pill and the line in a scoped Review, the dialog's list and its focus, and both items
Rejected.

### 29.22 Step 56: no merge without an effect

**What was found.** By the audit of 2026-10-22, and by reading the code:
- **An accept that changes nothing is reported like one that does.** Two of the six proposal kinds,
  a type change (`tbox_change`) and a recall, have no commit effect yet (architecture.md Section 14):
  the fold merges them and nothing in canon moves. Nothing reads a merged one - not the types, the
  gate grants, id forwarding or the belief. In Review the item offers Accept as for any kind, and
  where the diff would be it prints the engine's note, a sentence written for an engineer: "no diff
  - tbox_change has no commit effect yet, so there is no difference to compute - an empty diff here
  would mean 'not computable', never 'changes nothing' (architecture.md Section 14)". The hint says
  "Accept casts your verdict; nothing is deleted", the notice says "Accepted: Change a type ...", and
  Decided says "Undoing this takes a new proposal", as if something had been done.
- **A recall asks twice for an effect it does not have.** Section 7 gives the reason: "its effect is
  the strongest the console casts".
- **The agent surface says what the kinds will do, not what they do.** `propose` describes a type
  change as adding or revising "a type in the workspace's vocabulary" and a recall as "bulk
  retraction of everything derived from a contamination source". `review` accepts a merge of a type
  change from any agent, and `list_proposals` then reads `merged`. The `review-proposal` prompt asks
  the model to "explain in plain terms what merging would change", and the `brief` prompt asks, of
  each open proposal, "what merging it would change".

consolidation.md Section 6 already names the failure: a reviewer can merge one today and nothing
happens, "which is worse than refusing the kind, because the gate reports a decision it did not
carry out". There is a worse failure later. The v0.4.10 release note named it for recalls cast by an
agent, which the fold has demoted to comments since: on the day a kind gets its effect, every merge
of it already in a log would take effect at once. Nothing closes that for a recall merged at the
console, or for a type change merged anywhere.

**Decisions.** The author decided (2026-10-22) that a kind with no commit effect cannot be merged
until it has one, which the audit's recommendation, "say that it has no effect yet", stopped short
of. The fold is where a gate lives, since a replicated verdict never passes a write path
(proposal-workflow.md Section 13, M3.5b), so:
- **The fold counts no merge of either kind.** A merge verdict on a type change or a recall is a
  comment: it stays in the log and decides nothing, as an agent's recall merge has been since
  v0.4.10. This is a new validity condition, (f) in proposal-workflow.md Section 7.1. A proposal an
  earlier build merged that way had changed nothing, and now reads open, or rejected if a reject was
  cast. That is I16's second exception, made once. The author's store held none.
- **Every write surface of this node refuses the merge and says why.** That is the courtesy the
  write path already pays for a blocking check. Opening, comments, rejects and withdrawals are as
  before, so a person can still drop one (Section 29.21). The dream already refuses to draft either
  kind for the same reason (dreaming.md Section 4).
- **When a kind gets its effect, a merge cast before then never fires.** The step that builds the
  effect counts only merges the engine stamped as cast once the kind had it, as it stamps I17's
  console marker. Every other merge stays a comment. The stamp is built with the effect; this step
  fixes its direction.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| P23, "no merge without a diff" | A merge's diff and checks are computed before it is cast | repays, for a kind with no commit effect - the fold counts no merge of one and no surface casts one; other merges whose diff cannot be computed, such as a gate act with no tier, are unchanged |
| P23, "every kind the surface accepts has a commit effect" | Each kind the gate takes does what the kind promises | owed - unchanged (M4 Phase 5 for a type change, M6 step 5 for a recall); the surfaces still open both kinds, and now neither merges |
| P23, no bidirectional gating | No proposal state blocks or delays an observe | agrees - observe and opening a proposal are untouched |
| proposal-workflow.md Section 7.1 | What makes a verdict valid | revised - (f): the proposal's kind has its commit effect, and once it has, the merge carries the engine's stamp that it was cast with it |
| proposal-workflow.md I16 | Merged is absorbing | revised - a second exception, once: a type change or a recall an earlier build merged reads open or rejected; it had changed nothing |
| proposal-workflow.md I17 | A recall verdict is a person's direct act | agrees - the agent surface still refuses a recall merge with I17's reason, and the fold's console marker rule is unchanged; it decides again once recall has its effect, and its reading of the marker off every attestation is pinned on its own |
| P23, merges cast before an effect | When a kind gets its effect, no merge cast before it fires | owed, with its direction fixed - the step that builds an effect counts only merges stamped as cast once the kind had it |
| consolidation.md Section 6 | The gate does not report a decision it did not carry out | repays - the gate does not take the decision |
| consolidation.md C6 and C12 | A merged recall retracts; a partial effect is not shipped | agrees - no recall reads merged, and nothing of the effect ships |
| P5 | "Not computable" is not "nothing" | agrees - the diff stays not computable, and the item says in words that the kind has no effect yet |
| Section 7 | A proposal offers Accept, Reject and Skip; a recall asks twice | revised - a kind with no effect yet offers no Accept; a recall asks twice once it has one |
| Section 29.3 | "A split, a type change and a recall offer none, and say what undoing them takes" | revised - a type change and a recall are never merged while they have no effect, so only a split says what undoing it takes |
| Section 29.21 | Reject all blocked takes the Blocked items, a recall left out | agrees - a type change can no longer be Blocked, since Blocked needs a merge |
| U4 | A decision decides one item with its diff and checks on screen | agrees - Accept never appears without a diff, and Reject waits for the checks as before |
| U1 and U17 | An act reports its outcome, or that nothing changed; a state is said as its outcome | agrees - nothing reads Accepted while it changed nothing |
| U20 | The pill counts what waits under Needs you | agrees - an open type change or recall still waits for a person, who can reject it, so it counts |
| P11 and federation.md F18 | `define_type` is the solo exception; in a shared workspace a T-Box change passes the gate | agrees - unchanged: `define_type` stays the only T-Box write, so deployment stays single-principal, and nothing new points an agent at it |
| P22, P21 and prompts.md Section 5 | The tool descriptions and prompts carry what an agent must know | repays - `propose`, `review`, `list_proposals` and `get_proposal` say which kinds have no effect yet, and every prompt's rules say what to tell the person about them |
| Section 29.20 and compatibility.md Section 7 | Answers name where they wrote; a refusal is a listed change | agrees - `propose` keeps its id and `workspace` and gains a `note` for these two kinds; a proposal view gains `no_effect_yet`; `review` refuses one more merge, listed with the release |
| dreaming.md Section 4 | The dream drafts neither kind | agrees - unchanged, and now the rule of every surface |

No row needs a principle changed.

**Behaviour.**
- **The engine says which kinds have an effect.** One function answers it for every caller.
  - The fold reads a merge verdict on a kind without one as a comment, before I17's rule.
  - A proposal view of a type change or a recall carries `no_effect_yet: true` in `list_proposals`,
    `get_proposal` and the console's reads.
  - Its diff stays not computable. The note says so in words an agent can repeat: "tbox_change has
    no commit effect yet, so it cannot be merged until it has one, and there is no difference to
    compute".
  - `review_proposal` refuses a merge of either kind on every surface: "tbox_change has no commit
    effect yet, so it cannot be merged until it has one - a merge without a diff is a merge without
    review (Principle 23). A comment or a withdrawal is accepted, and the person can reject it in the
    Review panel". On the agent surface a recall merge keeps I17's refusal, which adds that no
    surface merges a recall until it has its effect.
- **Review offers no Accept.** Where the diff would be, under "What changes", the item reads "Nothing
  yet: a type change has no effect in this version, so it cannot be accepted until it has one." For a
  recall it reads "Nothing yet: a recall has no effect in this version, so it cannot be accepted until
  it has one."
  - Its controls are Reject, Skip and Show on the map, and A does nothing: the decision itself checks
    the mark, as it checks the blocking checks.
  - The hint reads "Reject sets it aside, and the log keeps it. It can still be accepted once type
    changes have an effect." For a recall, "once recalls have an effect".
  - Under Needs you its row carries "No effect yet".
  - Its map preview reads "Preview: Change a type ... (no effect yet)", since the map shows what it
    names, not a change.
  - Its Reject notice says "Rejected: Change a type ..." and nothing more, since there is no Accept
    after all to point at.
  - A rejected one offers no Accept after all. Its hint reads "A reject is provisional: it can still
    be accepted once type changes have an effect." For a recall, "once recalls have an effect".
- **The agent hears it.**
  - The descriptions of `propose`'s kinds and of `propose`, `review`, `list_proposals` and
    `get_proposal` say that the two kinds have no effect yet and cannot be merged until they do.
    The `recall` kind no longer sends the agent to the Review panel to decide; there the person can
    only reject it.
  - `propose` adds `note` when it opens one: "tbox_change has no commit effect yet: it can be
    commented on, rejected or withdrawn, and cannot be merged until it has one".
  - The rules every prompt keeps gain one line: a proposal marked `no_effect_yet` changes nothing and
    cannot be accepted yet; say so, and never describe the effect it would have.
  - The `brief` digest's open proposals carry the mark.

**What this step does not do.**
- It does not build the stamp that lets a kind's merges count once it has its effect.
- It does not refuse to open either kind. Ingest is free (P23), and an opened one waits for its
  effect or a person's reject.

**Guarded by** `p23_no_merge_without_an_effect` in the engine's policy cases (the view's mark, the
diff note, the refusal on both surfaces, a replicated merge console-marked or not read as a comment,
and the reject that is left), `i17_a_recall_merge_without_the_console_marker_never_folds_to_merged`
(no recall merge counts, a console-marked one included) and the engine's
`the_console_marker_is_read_off_every_attestation` (I17's marker reading and the one function);
`what_has_no_effect_says_so` in the MCP crate's unit tests (the note on `propose`, the refused merge,
the listed mark), the golden tool list and the prompts test; and `the_console_says_what_has_no_effect`
in the viewer's review tests (the sentence, no Accept, the row, the hints, the Reject notice, the
preview, and A checked in the decision itself). Browser checks seed a type change and a recall, and
read both items, the rows, A pressed with no request sent, the preview bar, the Reject notice and the
Decided item; the agent's merge is refused with why.

### 29.23 Step 57: a repeat says what it found

**What was found.** By the audit of 2026-10-22, by its two reviews, and by a case run against the
engine:
- **A proposal is its content, and the console's acts have fixed words.** The id hashes the
  workspace, the kind, the targets in order, `into`, the tier, `affected_types` and the reason. The
  console writes its own reasons: "merge-band suggestion (embedding-near names)" for Propose, "two
  entities named as one from a search" for Same as..., the notice's own sentence for Confirm and Undo.
  So the same act on the same items lands on the proposal it opened before, and the answer names
  that id alone.
- **Confirm, Undo, Confirm changes nothing the third time.** The second Confirm is the first one,
  already accepted, so its verdict is the first verdict and no grant comes after the Undo. A case
  confirms it: the source stays at the Undo's tier, while the notice says "Confirmed ..." and the
  preview showed the tier moving. A second Undo after that lands on the first Undo the same way.
- **Propose and Un-merge say a proposal waits when it is decided.** Suggestions leave out a pair an
  open merge names, and one a split pulled apart (unmerge.md S5). They offer a pair whose merge is
  Blocked, or that the person rejected, every time the person looks. Propose then lands on that
  merge, and the notice says "Merge proposed - it waits under Needs you for a verdict" while it sits
  under Decided. Un-merge does the same when a split of that merge was rejected or withdrawn. Keep
  ... instead does it when the reverse merge was withdrawn: it lands on that one, rejects the
  original, and leaves both closed.
- **The agent cannot tell either.** `propose` answers the id and the workspace whether it opened a
  proposal or found one.

**Decisions.** By rules already written:
- **A person's act at the console asks again when the answer no longer holds.** When the proposal
  it would open is already in the log, the console reads what that proposal still does:
  - It no longer holds when it was rejected or withdrawn, when it is a gate act a later accepted act
    on one of its sources has superseded, or when it is a merge a split reversed. The console then opens it again with ", again"
    added to its reason, ", again (3)" the third time, and so on, which makes it a new proposal, as
    unmerge.md S10 does for a merge after its un-merges. S10's count comes first in the reason.
  - It still holds when it is open, Blocked, or accepted and still in force. The console then writes
    nothing, so an agent's proposal never gains the console's mark (Section 29.17), and says what it
    found.
  - `/api/resolve` opens and accepts in one act. When it finds its gate act open it casts the
    verdict, since that is the act. When it finds it Blocked it refuses and names the check, so a
    Confirm instead stops at its first act. When it finds it in force it changes nothing and says
    so. It keeps opening without the console's mark, as today.
- **An agent's act stays its content.** `propose` lands on the proposal its content names, as P14
  says, and an identical one is absorbed as before (P3). Its answer says what it found, so the agent
  can give a different reason if it means a different act.
- **Suggestions follow the person's no.** dreaming.md Section 5.5 already says what a person's no is
  and how long it stands: a reject cast at the console, until new knowledge about one of the
  targets arrives. Suggestions leave out:
  - a pair a merge names while that merge waits, open or Blocked;
  - a pair a split pulled apart (S5);
  - a pair a merge names whose reject carries the console's mark, until an observation that is not a
    proposal event, authored in a later millisecond than that reject's console attestation (the
    wall of each HLC), asserts one of the merge's targets as named. A merge of three or more holds
    back every pair it names.

  A tie keeps the no. A stamp is made at export and carries the authoring time (sync-correctness.md
  Section 3), so the counter and node that break a tie of walls say which content id or node id sorts
  first, not which act came first. A no lifted by them could be lifted by the very observation that
  named the pair, and on the node that cast it only once that node stamps its log.

  Three departures from 5.5 keep the rule a function of the log (P16): a console reject counts
  whichever node it was cast on; a withdrawal holds nothing back, since the console has no
  withdrawal and every withdrawal today is an agent's; and knowledge counts as new by when it was
  authored, not when it arrived, so a peer's older knowledge that arrives after the no does not lift
  it. A reject only an agent cast holds nothing back either. The suppression stays of the
  suggestion, never of the possibility.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| U1 | Every console write reports its outcome, or that nothing changed | repays - a Confirm after an Undo confirms, and an Undo after that undoes; an act that finds its proposal waiting or in force says so, rather than that it opened one |
| P14, P3 and proposal-workflow.md I1 | A proposal is its content; the same content is absorbed | agrees - a console act that asks again carries a different reason, so it is a different proposal; an agent's identical content is the same proposal, absorbed and said |
| unmerge.md S10 | A re-merge must be a distinguishable proposal | agrees - ", again" is S10's rule for every console act whose answer no longer holds; S10's own count stays first |
| unmerge.md S5 and S3 | A split suppresses the suggestion permanently, never the possibility | agrees - unchanged; the suggestions also wait on a merge in flight and on the person's no |
| unmerge.md Section 5 and resolution-identity.md Section 3 | The band leaves out pairs "already covered by an open entity_merge proposal" | revised - and a Blocked one, a split's pair, and the person's rejected one until the knowledge changes |
| dreaming.md Section 5.5 | The person's no stands until the knowledge changes; a reject from a peer or an agent is not it | agrees, with three departures named above - any node's console reject counts, so equal logs suppress equally (P16), a withdrawal holds nothing back, and knowledge is new by its authoring time rather than its arrival |
| sync-correctness.md Section 3 | A stamp carries the authoring time, and the order it gives is total, not causal | agrees - the hold reads the walls alone, so a tie, which nothing here can order, keeps the no, and stamping changes nothing the hold shows |
| P5 and IR2 | A generator proposes, a verdict commits; a withheld pair is not "no pair" | owed - the band's report of the pairs it withholds stays owed (unmerge.md Section 5), and now withholds more; the empty band's line no longer says that none are near |
| proposal-workflow.md I5 and I16 | A reject is provisional; merged is absorbing | agrees - no state changes; a rejected merge can still be accepted after all, and a new act comes after the ones before it |
| resolution.md Section 5 | Gate acts apply in order, each from where the last left | agrees - the new Confirm applies after the Undo, as the person meant |
| Section 29.22 | No merge without an effect | agrees - no act is opened again to merge a kind without one; a type change or a recall is never accepted, so it is opened again only when rejected or withdrawn |
| Section 29.17 | Keep ... instead says what it found when the reverse merge exists | revised - Keep reads the state it gets back: a reverse merge withdrawn or reversed by a split is opened again, and a Blocked one is said as Blocked |
| P22, P21 and Section 29.20 | The agent's answers say what was done and where | repays - `propose` answers `state` and `found`, and a `note` when it found one |
| compatibility.md Section 7 | Answer changes are listed | agrees - `propose` gains `state` and `found`, added fields |

No row needs a principle changed.

**Behaviour.**
- **The engine.**
  - One function opens a console act. Under the write lock it computes the act's id. While that id
    names a proposal that no longer holds, it adds ", again" or ", again (n)" to the reason and
    computes again.
  - Every open reports `{id, state, found}`. `found` is true when the id was in the log before the
    call; `state` says the rest.
  - The suggestions read one set of held-back pairs, as above. A name-variant group of three or more
    stays listed while any of its pairs is neither held back nor already listed at an earlier rung,
    and its chips still name every member.
- **The console.**
  - `/api/propose_merge`, `/api/propose_split` and `/api/resolve` go through that function. The first
    two answer `{"proposal_id", "state", "found"}`.
  - A found merge that waits says "A merge of these two already waits under Needs you.", the sentence
    Same as... now uses too; a found un-merge says "An un-merge of this merge already waits under
    Needs you."; the item opens. A found merge in force says "These two are already merged." and a
    found un-merge in force "This merge is already un-merged.".
  - A found Confirm or Undo in force says "Nothing changed: this is already so." A found Blocked one
    is refused with its check.
  - Keep ... instead reads the state it gets back. A Blocked reverse merge reads "Rejected. The merge
    into X waits under Needs you, but a check holds it back."
  - The empty band says "none waiting - no near pairs that are not already proposed, split apart or
    held back by your reject" in place of "none - no embedding-near pairs".
- **The agent.** `propose` answers `state` and `found` beside its id and workspace. When it found a
  proposal it adds a `note` naming the state:
  - open: "an identical proposal is already open - a proposal is its content, so this is that one";
  - Blocked: "an identical proposal was accepted but a check holds it back - get_proposal names the
    check";
  - rejected: "an identical proposal was rejected - a reject is provisional, and a different
    rationale opens a new one";
  - accepted: "an identical proposal was already accepted - if a later act has superseded it, a
    different rationale opens a new one";
  - withdrawn: "an identical proposal was withdrawn - a different rationale opens a new one".
  - Section 29.22's note, when it applies, comes first. An identical merge a split reversed is
    refused before any note, as today (S10).

**What this step does not do.**
- It does not report the withheld pairs in the band's coverage report, which unmerge.md Section 5
  owes.
- It does not change what an agent's identical content means: the same proposal.
- It does not give `/api/resolve`'s opening the console's mark.

**Guarded by** `p14_a_repeat_says_what_it_found` in the engine's policy cases (Confirm, Undo, Confirm
confirms with ", again" in its reason, and the Undo after it undoes; a Confirm in force and a merge
that waits are found with nothing written; a rejected merge is asked again; an agent's identical
content is the rejected proposal, found) and `p19_suggestions_follow_the_persons_no` (a merge in
flight, an agent's reject, the person's reject, and new knowledge that lifts it);
`p16_equal_logs_suppress_suggestions_equally` in the engine's scenarios (a split pair, the person's
no cast on either of two nodes and a no tied with knowledge held back, and a no that later knowledge
lifted offered again, alike on the author before and after it stamps and on nodes given one log in
four orders);
`a_repeat_says_what_it_found` in the MCP crate's unit tests (state, found and each note, Section
29.22's note first) and the golden tool list; `a_console_repeat_says_what_it_found` in the viewer's
HTTP tests (the console's merge found and asked again, and Confirm, Undo, Confirm through
`/api/resolve`, then found in force with nothing cast); and `the_console_says_what_a_repeat_found` in
its review tests (the sentences, a found item opened, Keep reading its state, the empty band's line).
Browser checks propose a name variant from Suggestions, reject it and see Suggestions leave it out
with the new line, and press Un-merge twice to see the second find the first; an agent's identical
`propose` answers found with its note.

### 29.24 Step 58: a panel on a tablet ends above the camera

**What was found.** Step 52 left it owed (Section 29.18): from 760 to 1200 pixels, under a coarse
pointer, the camera buttons and a floating panel overlap. A measure in Chromium with touch at 1024 by
768 and 1180 by 820 shows the inspector's bottom 20 pixels over the buttons' top, across their full
width, and the same for a floating dock; with a mouse the two are 8 pixels apart. Its review found
more of the same cause:
- **The panels' bottom is a sum written for a mouse.** They end 90 pixels above the window's bottom:
  the status bar's 28, a gap of 16, the toolbar's 38 and a gap of 8. A finger makes the status bar
  44 and the toolbar 50 (U19), and 200% text makes the status bar 56 even with a mouse (U21).
- **The minimap and the View popover** sit 62 pixels above the status bar, the same mouse-only sum,
  at every width; under a finger they reach 4 pixels over the toolbar.
- **The strip** at the window's lower left keeps 200 pixels clear for the toolbar, which is 230 wide
  with a mouse and 314 under a finger, so below about 900 pixels the toolbar covers the strip's end.

**Decisions.** No choice is open: everything that clears the toolbar does it by the toolbar's own
size. `--hud-h`, which a phone's page already measures, becomes the toolbar's height at every
width, and `--hud-w` its width.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 18, step 7 | Between 760 and 1200 pixels a floating panel ends above the camera toolbar | repays - under a finger and at 200% text too |
| Section 18's note on the status bar | `--bar` is a rem token, and every panel and the camera above the bar read it | repays - the panels, the minimap and the View popover read the toolbar's size as well |
| Section 29.18 | The tablet band's overlap is owed | repays |
| U19 | Under a coarse pointer every target is 44 pixels | agrees - unchanged; the things around the toolbar make room for it |
| U21 | Text scales to 200% without loss | repays - the panels' bottom grows with the status bar |
| U7 | Every custom property a rule reads is declared, with one value per token | agrees - `--hud-h` and `--hud-w` are declared at the root and redefined for a finger |

No row needs a principle changed.

**Behaviour.**
- **Two tokens.** The camera toolbar is `--hud-h` tall and `--hud-w` wide: 38 by 230 pixels with a
  mouse, 50 by 314 under a coarse pointer. On a phone the page still measures the height, since
  there the toolbar can wrap.
- **From 760 to 1200 pixels** a floating panel, dock or inspector, ends the status bar's height, 16,
  the toolbar's height and 8 above the window's bottom. With a mouse at the default text size
  nothing moves; under a finger the panel ends 8 pixels above the toolbar. A phone held sideways
  falls in this band, and its panel is short and scrolls.
- **At every width** the minimap and the View popover sit at that same height, and the popover's
  tallest is the window less the header, the status bar, the toolbar and the gaps.
- **The strip** keeps the toolbar's width and three gaps clear.
- The panels' tallest reads the visible height where the browser knows it (`dvh`), as the phone's
  sheet does.

**What this step does not do.** It does not change the phone's sheet, which already reads the
toolbar's measured height, or the wide window's docked panels, which the toolbar does not meet.

**Guarded by** `a_panel_on_a_tablet_ends_above_the_camera` in the viewer's key tests (the two tokens
equal to the toolbar's buttons and padding, with a mouse and under a finger; the band's panels, the
minimap, the View popover and the strip reading them; no mouse-only sum left). Browser checks in
Chromium measure the toolbar against the inspector, both docks, the View popover and the strip at
1024 by 768, 1180 by 820, 820 by 1180 and 844 by 390 under touch, and at 1024 by 768 with a mouse at
100% and 200% text: nothing overlaps, and the band's inspector ends 8 pixels above the toolbar.

### 29.25 Step 59: Un-merge opens what it proposed

**What was found.** By the usability audit of 2026-10-22: Un-merge opens a proposal to separate two
entities and says "Un-merge proposed - it waits under Needs you for a verdict", and the person then
has to find that proposal under Needs you and accept it. Same as... and Keep ... instead open the
merge they propose (Section 29.17), and since step 57 an Un-merge that finds its split waiting opens
it; an Un-merge that opens a new one does not.

**Decisions.** None is open: Un-merge does what its siblings do.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| unmerge.md Section 8 | An un-merge goes through the proposal gate, like every change to canon | agrees - opening the item decides nothing; the split still waits for its verdict |
| U4 | A decision decides one item with its diff and checks on screen | agrees - the person sees the split's diff and checks before Accept, as for any item |
| Section 29.17 and Section 29.23 | Same as..., Keep ... instead and a found act open the item | agrees - Un-merge joins them |
| Section 29.3 | Every reverse is on its Decided item | agrees - unchanged; Un-merge is still on the accepted merge |

No row needs a principle changed.

**Behaviour.** Un-merge, from the accepted merge's Decided item or from the notice after its Accept,
opens the split it proposed under Needs you, selected and focused, with its notice as before.

**What this step does not do.** It does not accept the split: that stays the person's next act.

**Guarded by** `the_console_says_what_a_repeat_found` in the viewer's review tests, which holds
Un-merge to opening the split it proposed, found or new. A browser check accepts a merge, presses
Un-merge on its Decided item, and finds the split selected and focused under Needs you.

### 29.26 Step 60: a person can demote a claim

**What was found.** By the audit of 2026-10-22: some acts on the canon only an agent can take. Two
were closed by steps 51c and 57: naming any two entities as one (Same as...), and merging them again
after an un-merge. Two remain:
- **Demoting a claim.** A person who reads a claim and knows it is wrong can confirm another value
  of a contested field (Confirm instead) or undo a confirmation, but cannot lower the trust of a
  claim that is simply wrong. resolution.md Section 4.2 names that demotion as "the stronger act",
  and it "remains fast-path". Today it takes an agent.
- **Withdrawing a proposal.** Only the agent surface withdraws.

**Decisions.** By the rules already written:
- **The observation card offers the demotion.** It is the gate act the console already casts for
  Undo and Confirm instead, through the same path (`/api/resolve`, `kind=claim_demotion`), so it
  adds no write path. It asserts nothing, so the console still observes nothing and "Neither is
  right" still asks an agent to record the right value (Section 12). It lowers to Unverified, the one
  tier that says a person doubts the claim; Undo of a confirmation already returns a source to the
  tier it had. It keeps Review's name for the act, Demote a claim, and its Undo is the Undo demotion
  Decided offers (Section 29.6: one concept, one name).
- **No Withdraw at the console.** A withdrawal closes a proposal for good, where a person's reject
  stays provisional and can be taken back (Accept after all); and step 57's suggestions read a
  withdrawal as no one's no because the console has none.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 2 | The viewer adds no new write path; the console observes nothing | revised - the sentence names Undo as the demotion the console casts; a demotion from the card is the same act on the same path |
| proposal-workflow.md Section 14.3 | The viewer's write paths are the gated verdict, "the confirm act of resolve", the gate open and reify | revised - resolve's acts are confirm, undo and demote, each a gated proposal and verdict |
| Section 12 | "Neither is right" asks an agent | agrees - a demotion states no value; the right one is still an agent's to record |
| resolution.md Section 4.2 | Demoting the losing side is the stronger act, fast-path | repays - a person can now take it |
| resolution.md Section 3 and R5 | A demotion is a ceiling: claims do not raise it, a raising review does | agrees - the question says so in those words |
| P6 and resolution.md Section 4.1 | A contradiction is surfaced, never silently resolved | agrees - the question says which values flip and which contradictions start or end |
| P23 | A merge comes with what it overturns and the contradictions it creates | agrees - the question reads the act's diff, overturned values included, before Demote acts |
| P18 | Promotion only explicitly | agrees - a demotion promotes nothing, and its Undo is an explicit act with its own diff |
| proposal-workflow.md I9 | A demotion may be approved by its proposer | agrees - the person's own act; `/api/resolve` opens it without the console's mark, as for Undo, so Decided names the machine that cast it |
| U4 | A decision shows its diff and checks | agrees - Demote waits until both are read |
| U18 | Settling a contested value says what the act does before it is cast, and offers no Undo before its diff | revised - the same holds for a demotion from the card |
| U1 and U20 | A console write reports its outcome where it was made; an act names a workspace not on screen | agrees - the notice names the claim, and its workspace when it is not the scope |
| U19 | Keys act where the person is | agrees - Esc closes the question before the card, and focus returns to the act that opened it |
| Section 29.6 | One concept, one name | agrees - Demote a claim, as Review says it |
| Section 29.23 | A repeat says what it found | agrees - a second demotion already in force says "Nothing changed: this is already so." |
| remote-viewer.md Section 3.2 | The read tier refuses `/api/resolve` and its preview and undo | agrees - the card offers nothing there |
| proposal-workflow.md Section 8 | A withdrawal is the proposer's | agrees - the console offers none |

No row needs a principle changed.

**Behaviour.**
- **The offer.** On the local console, the card of an observation that is not a proposal event and
  reads above Unverified offers "Demote this claim".
- **The question.** It opens in the card, "Demote this claim to Unverified?", and reads the act's
  preview. Then it says:
  - what flips, a line per value the diff overturns: "Ledger's type becomes Concept.", "...becomes
    contested and waits under Needs you." or "...becomes View and is no longer contested."; when
    nothing flips, "Every value it states stays; it is trusted less.";
  - whose trust ring drops: "With it, Ledger drops to Unverified.", or "No entity's ring moves.";
  - what else it states: "It also states 2 relations." and "It also defines 1 type: Driver.";
  - "Unverified: Trusted less than an agent's claim: a review lowered it, or its sender marked it
    so." and "Later claims do not raise it; a review that raises it does.";
  - the checks.
  Demote waits until all of that is read. Cancel and Esc close the question, and focus returns to
  "Demote this claim".
- **The act.** Demote casts the demotion with "Demoted to Unverified: " and the claim's first words
  as its reason and its notice; the notice names the claim's workspace when it is not the scope on
  screen. The notice offers Undo once it has read the act's own undo, the read Decided's Undo
  demotion makes, with that diff as its note: "Undo returns this claim to Agent and keeps it there
  until a review moves it again." Undo casts with "Undid Demoted to Unverified: " and the first
  words. The card then reads the claim again by its id, says "Lowered to Unverified by you at this
  console", and focus goes to its Close.

**What this step does not do.**
- It does not lower to a tier between: a person who wants Agent back undoes the confirmation.
- It does not offer Withdraw, for the reasons under Decisions.

**Guarded by** `a_person_can_demote_a_claim` in the viewer's settle tests (the offer and its
condition, the question's lines and Demote waiting for them, the demotion kind, the reason and
notice, Undo from the act's own undo read, the card read again by id, the downward ring verb, and Esc
closing the question first). A browser check seeds two claims of one entity's type, demotes one from
its card, and reads the question ("Ledger's type becomes View and is no longer contested.", "With it,
Billing drops to Unverified.", "It also states 1 relation."), Esc closing only the question with focus
back on the offer, the notice with its Undo note, the card's "Lowered to Unverified by you at this
console", focus on Close, and the contradiction settled on the map.

### 29.27 Step 61: the band says what it held back

**What was found.** Step 57 left it owed (Section 29.23), as unmerge.md Section 5 already had: the
merge band leaves out a pair that a waiting merge names, that a split pulled apart, or that the
person rejected, and its report says nothing of them. An empty band therefore reads two ways - no
near pairs, or the near pairs it found all already answered - and step 57's line, "none waiting - no
near pairs that are not already proposed, split apart or held back by your reject", covers both
without saying which. unmerge.md Section 5: "A pair withheld because a human separated it is a third
state and belongs in the band's coverage report rather than vanishing."

**Decisions.** None is open: the report counts what it held back, and the line says which state it
is. The count is named `held_back`, since P5 already gives "withheld" its own meaning, a scope the
caller is not granted.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| unmerge.md Section 5 | A withheld pair is a third state, reported in the band's coverage | repays |
| P5 and resolution-identity.md Section 3 | An empty list does not stand for absence; `merge_band` says why it is empty | repays - `merge_band` gains `held_back` beside `available`, `embedded` and `examined` |
| P5's answer alphabet | Unavailable, uncomputable, withheld and truncated stay distinct | agrees - the count says what the list left out, as truncated does, and adds no new value; it is not named "withheld" |
| Section 29.23 | The band's report of what it held back is owed | repays |
| P16 and resolution-identity.md Section 3 | The band is a node-local retrieval aid; the suppressed set is read off the log | agrees - the count depends on this node's vectors, as the band's pairs do |
| compatibility.md Section 7 | Answer changes are listed | agrees - Section 7 gains `merge_band.held_back`, an added field of the curation report the `curate` prompt's digest carries, and the prompt says what it means |
| The name-variant ladder | An empty list does not stand for absence | owed - the ladder reports no count of what it leaves out; its groups still list every member, and its empty list is said by the queue, not by a coverage line *(repaid by step 62, Section 29.28)* |

No row needs a principle changed.

**Behaviour.**
- **The count.** `merge_band.held_back` counts the pairs the band reached - among each entity's
  `MERGE_BAND_K` nearest, above the floor, inside one workspace, neither merged away - that it left out
  because a waiting merge names them, a split pulled them apart, or the person's reject holds them
  back. A pair is counted the first time it is reached, as it is listed, so each counts once.
- **The line.** When the band is empty:
  - over every entity, with nothing held back: "none - no embedding-near pairs", as before step 57;
  - over every entity, with some held back: "none waiting - 3 near pairs are already proposed, split
    apart or held back by your reject" (one pair: "1 near pair is ...");
  - over part of the entities, the coverage line as before, and with some held back it adds "- 3
    near pairs are already proposed, split apart or held back by your reject";
  - a report without the count reads as step 57's line.
- **The agent.** The `curate` prompt says that a nonzero `held_back` beside an empty list means the
  near pairs are already answered, not that none exist.

**What this step does not do.** It does not count what the name-variant ladder leaves out, the owed
row above. *(Step 62 does, Section 29.28.)*

**Guarded by** `the_band_counts_what_it_held_back` in the engine's unit tests (three near pairs, one
put in flight: two listed and one held back, counted once though reached from both ends), the review
tests' `the_console_says_what_a_repeat_found` (each line of the empty band, the count read apart from
its absence), and the MCP prompts test (the `curate` sentence). A browser check on a scenario daemon,
whose embedder finds no near pair among four names, reads `held_back: 0` and the line "none - no
embedding-near pairs": a count of zero is said as a known none.

### 29.28 Step 62: the ladder says what it held back

**What was found.** Step 61 left it owed (Section 29.27): the name-variant ladder, the
deterministic sibling of the merge band, leaves out a pair that a waiting merge names, that a split
pulled apart, or that the person rejected, and nothing reports it. An empty ladder therefore reads
two ways - no names alike, or names alike that are all answered - and Suggestions says neither: its
line speaks for the band alone ("By meaning: ..."), and the empty queue says only "No suggestions
here."

**Decisions.** None is open: the ladder counts what it held back, as the band does, and the line
says by name what it says by meaning. The report gains `name_ladder`, a `{held_back}` object right
after `name_variants`, always present, since the ladder needs no embedder and always runs; it has no
`available` field, and the report's `stats` do not change. The count keeps the name `held_back`, as
the band's does.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 29.27 | The ladder reports no count of what it leaves out | repays |
| P5 | An empty list does not stand for absence | repays - the report gains `name_ladder.held_back`, and Suggestions says by name whether none are alike or all are answered |
| unmerge.md Section 5 and resolution-identity.md Section 3 | A pair withheld because a human separated it is a third state, reported in the band's coverage | agrees - both are written of the band; the ladder is its sibling on the other axis and reads the same held-back set |
| P5's answer alphabet | Unavailable, uncomputable, withheld and truncated stay distinct | agrees - the ladder needs no embedder, so it is always computed, and the count says what the list left out without a new value; it is not named "withheld" |
| P16 and P19 | The ladder is deterministic and belongs to the core | agrees - the count is a pure fold over the entities and the held-back set read off the log |
| compatibility.md Section 7 | Answer changes are listed | agrees - it gains `name_ladder.held_back`, an added field of the curation report the `curate` prompt's digest carries, and the prompt says what it means |
| architecture.md, Principle 5's section | "The other curation signals are deterministic and need no such caveat" | revised - deterministic, and still owing a count of what they leave out; the same sentence on `MergeBandCoverage` is revised with it |

No row needs a principle changed.

**Behaviour.**
- **The count.** Taken after all three rungs, as a set: of the pairs the ladder reached - two
  entities of one workspace, neither merged away, whose names or aliases meet at a rung under a
  non-empty key - those held back (a waiting merge names them, a split pulled them apart, or the
  person's reject holds them back), less the pairs of every group it lists. A pair reached at
  several rungs counts once; a pair a listed group shows - a group of three that stays listed for
  another of its pairs - is not counted, since its members are in front of the person. When the
  ladder lists nothing, every pair it reached is held back, and the count is all of them. A waiting
  merge of three entities holds back three pairs, as it does in the band's count.
- **The line.** Over Suggestions, a sentence by name whenever the ladder lists nothing, after the
  "By meaning" sentence when there is one:
  - with nothing held back: "By name: none - no two names match once case, spaces and symbols are
    dropped, a trailing -s or -ies is folded, or an alias is counted.";
  - with some held back: "By name: none waiting - 3 alike-named pairs are already proposed, split
    apart or held back by your reject." (one pair: "1 alike-named pair is ...");
  - a report without the count says nothing by name, as before this step.
  The two counts are separate, never a total: a pair both near in meaning and alike in name is in
  each. The empty queue still says "No suggestions here." below the line.
- **Everywhere.** The read tier answers the same report, and a union over workspaces (`*`) adds the
  counts, which is exact, since a ladder pair never spans workspaces.
- **The agent.** The `curate` prompt says that a nonzero `name_ladder.held_back` beside an empty
  `name_variants` means those pairs are already answered, not that none exist, and that it is
  counted apart from `merge_band.held_back`.

**What this step does not do.** It changes no grouping: the ladder lists what it listed, and a
group's chips still name every member.

**Guarded by** an engine unit test (one pair reached at two rungs counted once; a listed group of
three keeping its held-back pair uncounted; an empty ladder counting every pair it reached), the
review tests (each line by name, the count read apart from its absence, the order after "By
meaning"), and the MCP prompts test (the `curate` sentence). A browser check on a scenario daemon
reads Suggestions in a workspace holding "TrustTier" and "Trust Tier": with the pair listed, no
sentence by name; once a merge of them waits, "By name: none waiting - 1 alike-named pair is already
proposed, split apart or held back by your reject." after "By meaning"; and in a workspace with
nothing alike, "By name: none - no two names match ..." - a count of zero said as a known none.

### 29.29 Step 63: the window's pages get only what they use

**What was found.** Step 50b left it owed (Section 29.13): "It does not take the menu interface from
the pages of the main window, which any of them may still call; it is owed with the window's
permissions, unscheduled." The main window's baseline capability grants Tauri's `core:default`,
which carries `core:menu:default` and `core:tray:default`: any page the window shows - the viewer a
daemon serves, and once remote-viewer.md's app step lands, a page a hub serves - may replace the app
menu (`set-as-app-menu`), add items, or change, hide or remove the tray and replace its menu - its
id is fixed - so a page can speak with the app's own voice where the person reads the app's state. The pages use none of it:
the shell listens for and emits events, asks whether the window is full screen, and drags the
window by its title bar; the app's own pages call the app's commands, which settings-page.md S1
already locks.

**Decisions.** None is open: the baseline names what the pages use and nothing else.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 29.13 | The menu interface stays callable by every page of the main window | repays |
| settings-page.md Section 4 and S1 | No command is open by default; a capability is scoped to windows, so the caller check is the lock | agrees - S1 locks the app's commands by caller; this narrows the platform's to what the pages use, and the new S8 says so |
| remote-viewer.md | A hub's page shown in the app is not trusted with the app's surfaces | agrees - repaid before the page that needs it arrives |
| P18 | What can write is an attack surface | agrees - the menus and the tray are where the person reads the app's state, and no page writes them now, except the map items' enabled state a page's load report sets, harmless since each press finds its control again (Section 29.13) |
| Section 29.13's tray and menu | The tray says the memory's state and the menu holds the map's commands | agrees - both are built and changed in the app's Rust side, which no permission governs, and the pages' load report reaches it as an event |

No row needs a principle changed.

**Behaviour.**
- **The baseline.** The main window's `default` capability grants `core:event:allow-listen`,
  `allow-unlisten` and `allow-emit`, `core:window:allow-is-fullscreen`,
  `core:window:allow-start-dragging` and `core:window:allow-internal-toggle-maximize`, and no other
  platform permission.
- **What stays.** The title bar drags and double-click zooms; a page loaded in full screen asks
  and keeps no room for the traffic lights; the shell's `shell-page-loaded` report enables the
  map's menu commands; Settings and Welcome hear `setup-line`; the app's own commands still answer
  only the app's own pages (S1).
- **What goes.** A page can no longer make or change a menu, the tray, an image, a path or a
  resource, nor read the window's size, place or monitor. In a debug build the devtools shortcut
  (Cmd+Opt+I) goes with them, since Tauri sends it through the webview permission; Inspect Element
  on the page's menu still opens them.

**What this step does not do.** It does not change S1's caller check, the commands' list, or the
menu and tray themselves.

**Guarded by** `the_window_pages_get_only_what_they_use` in the app's tests, which holds
`capabilities/default.json` to exactly those grants, reads every platform call in the app's page
scripts and fails on one no grant names, and holds the viewer's scripts to no platform call at
all.

### 29.30 Step 64: a pinch zooms as far as the fingers spread

**What was found.** The author, 2026-10-23: zooming by touch is much faster than by the mouse
wheel, so a pinch has to be made with great care. The map's wheel handler zooms 12% in or out for
every wheel event, whatever its size. A mouse sends one event a notch, so the step is right for it.
A trackpad pinch, which Chrome and Firefox send as wheel events with `ctrlKey` set, sends dozens a
second, each a small delta - and each one moved the map the whole 12%. WebKit, the app's window
among it, sends a trackpad pinch as gesture events instead, which the map did not take. A touch
screen's pinch already follows the fingers, 1:1 (step 52).

**Decisions.** None is open: step 52's rule - a pinch zooms as far as the fingers spread - is
carried to the trackpad.

**The check.**

| Rule | What it asks | Standing |
|---|---|---|
| Section 29.18 (step 52) | Two fingers pinch to zoom 1:1 under the fingers | agrees - the trackpad's pinch now follows the same rule, through the same camera move |
| Section 29.4 | A wheel or pinch on the map is the person's move | agrees - every pinch event is one, as before |
| U28 | Drawing never moves the layout | agrees - only the camera moves |
| U31 and U40 | The map pans and zooms within the frame budget | agrees - a pinch moves the camera as a wheel step does, and the bench's wheel steps are unchanged |
| WCAG 2.5.1 | A multipoint gesture has a single-pointer alternative | agrees - unchanged: the wheel, the zoom buttons and Fit |

No row needs a principle changed.

**Behaviour.**
- **The mouse wheel** is unchanged: 12% in or out a notch, eased, around the pointer.
- **A trackpad pinch as wheel events** - `ctrlKey` set, as Chrome and Firefox send it - scales the map
  by `exp(-deltaY / 100)` an event, the delta in pixels, at most 25% either way, 1:1 around the
  pointer with no easing, as a touch pinch does.
- **A trackpad pinch as gesture events** - WebKit's `gesturestart`, `gesturechange` and
  `gestureend` - scales the map by the ratio of the gesture's scale to the last it reported, around
  the pointer, and the page is not magnified. While a gesture runs, `ctrlKey` wheel events are not
  taken, so no pinch counts twice.
- **A touch screen's pinch** is unchanged.

**What this step does not do.** A two-finger scroll on a trackpad is a wheel event without `ctrlKey`,
which an engine does not tell apart from a mouse's; it keeps the wheel's step.

**Guarded by** `a_pinch_zooms_as_far_as_the_fingers_spread` in the viewer's keys tests: the wheel
handler keeps the notch step, a `ctrlKey` wheel scales by the delta through the touch pinch's camera
move and is not taken during a gesture, and the gesture events scale by the ratio and prevent the
page's own zoom. A browser check sends ten small `ctrlKey` wheel events and a gesture through the
viewer and reads the camera's scale.

### 29.31 Step 66: the landing says what ships, and lets the visitor do it

**What was found.** A read of the landing against v0.10.2 and three reviews (copy, the stage's
interaction, and how comparable products present themselves):
- It claimed more than ships. "A person checked it" and "your verdict" for a tier the console's socket
  records (resolution.md Section 6); "the conflict is flagged" where only a second type is; remote
  servers under Shipped, which are frozen; sync that "starts when you add a server", where it starts
  when a machine joins.
- It left out what a visitor would most want: the commit hook that keeps the why without being asked,
  a second machine joining with a code, and that a verdict can be undone.
- The stage only played back. The govern step's Accept was a picture, the scene accepted on the
  visitor's behalf before anyone pressed it, and on a phone the frame played above words not yet
  read.

**Decisions** (the person's, 2026-10-24, from the recommendations):
- D1. The heading stays; the subhead says what only this product does, in 25 words.
- D2. The five interactions: the phone frame between a step's words and its snippet; Accept in the
  govern step; the travel step's verdict or an agent saying so; the structure step's replay range; the
  recall step's hits. A choice of one among a few is a radio group (U19).
- D3. The copy follows the product as released: every claim above corrected, and the hook, joining and
  undo said where they belong.
- D4. The viewer's screenshot is taken again, from a scratch daemon holding the stage's made-up
  project, never from a real store.
- D5. Check it yourself joins after the viewer.

**Order.** This section and Section 11's revisions; then the copy and Check it yourself; then the
interactions; then the screenshot. Each is its own change. Nothing the page says is main-only: what
the next minor adds (a write names its workspace) waits for its release before the page says it.

### 29.32 Step 67: the principles, one at a time

**What was found.** The principles page put 111 clauses in front of its first principle, then two
principles a screen with about eleven parts each, and on a desktop two scenes played at once on half
its screens. It was 17 screens tall on a desktop and 28 on a phone, and a reader had to parse the
ledger's vocabulary before meeting one principle in plain words.

**Decisions** (the person's, 2026-10-24, from the recommendations): an index, then one principle at a
time, rather than a short "start here" path, which would rank principles that principles.md does
not, or an accordion with no previous and next; and on a phone a principle the reader opened plays.

**Order.** Section 11a's revisions and this section; then the reader in `site/principles.js`, the
page's markup moved (the ledger into How a principle is held, its markers with it, so the generator
writes the same regions), and the rules in `site/style.css`.

**What this step does not do.** Its review left four things owed, none of them a wrong reading:
- The arrow keys still step while a link inside the principle has the focus; only the form controls
  and a disclosure keep them. Revisit when a reader reports a step they did not ask for.
- A reader that fails after it has opened a principle leaves the one principle on the screen without
  its bar. Revisit when the page's markup changes shape.
- An open principle's title is still a link to itself. Revisit with the next change to the reader.
- The phone's test of when a scene plays reads the script's text, as it did before. Revisit when the
  page gets a test that runs it.
