# End-to-end tests - the product as its people and agents meet it (Principles 5, 24)

> Tests that run the product's binary the way its users run it: a daemon started as
> `supragnosis serve`, AI apps talking MCP over HTTP or through the stdio bridge, a person at the
> viewer's console, the viewer in a browser, and the app's own pages. Each scenario is a short story
> with an outcome a person or an agent can see. The suite reports, as a badge, how many of the
> scenarios this document names are built and passing. Companion to [architecture.md](architecture.md)
> Section 14 (what guards what), [compatibility.md](compatibility.md) Section 6 (the CLI's
> machine-read answers) and [onboarding.md](onboarding.md) (the first run).
>
> Status: **partly built** (Section 11): the daemon's half, the Rust harness with A1-A4 and A8, and
> the scenario check with its badge. The browser, the app's pages and the other scenarios are not.

## 1. Why this exists

Almost every test today builds the engine inside the test process. An in-memory store, the hashing
embedder and an in-process transport stand in for the daemon, its socket and its HTTP listener:

- `crates/supragnosis-mcp/tests/mcp_surface.rs` drives a real MCP client, over an in-process pipe.
- `crates/supragnosis-viz/tests/http.rs` speaks HTTP over a real unix socket, to a server task in
  the test. It also proves the socket's 0600 mode and that verdicts are answered only there.
- `crates/supragnosis-engine/tests/principle_scenarios.rs` holds 61 scenarios against the engine
  directly.
- The bridge's own tests restart a daemon under it, and the sync crate's tests serve over real TCP.
- Many "UI" tests read the viewer's or the app's source text (`include_str!`) and check it for a
  string, not a behaviour.

That catches a wrong rule, and a few seams. What no test runs is the product assembled:

- **The binary's own wiring.** That covers the flags and environment `resolve` reads, the token file
  the daemon writes and the bridge reads, and the order the daemon starts its listeners in. It also
  covers a restart on the same store, and a second daemon syncing with the first.
- **The browser.** No test opens the viewer or the app's pages in a browser. A script error that
  leaves a page blank passes every test. One did, while the Welcome page was being built: a function
  named `top` collided with `window.top` and stopped the whole script. Only a headless render done
  by hand found it (PR #168).

Each of these was checked by hand at a release, on the author's machine. The suite is the place
those stories are written down once and run on every change.

## 2. What this is NOT

- **Not a replacement for the unit and scenario tests.** A rule stays tested where it lives, and
  most rules are cheaper and sharper there. A scenario here proves the path from a person or an
  agent to that rule still connects.
- **Not the model evaluations.** The `e2e/` crate's `#[ignore]`d suites measure how well a real model
  uses the tools and produce a scorecard (`e2e/src/lib.rs`). They stay as they are, under
  `task test:e2e`. This suite needs no model and no network, and lives elsewhere (Section 4).
- **Not pixel comparisons.** Fonts, antialiasing and a canvas layout that settles differently on
  each machine make a golden screenshot a test of the runner. Screenshots are kept as evidence when a
  scenario fails, and are never compared.
- **Not the desktop window itself.** The macOS webview has no WebDriver, so the Tauri shell's window
  is not driven here. Its pages are, with the app's commands answered from examples the app's own
  tests produce (Section 4.3). The window is covered by the shell's own tests and by the clean run
  before a release (onboarding.md Section 13 step 5).
- **Not a way to reach the viewer over TCP.** Section 3.2's proxy exists only inside a scenario, for a
  throwaway store, and nothing starts it outside the suite.

## 3. The harness

### 3.1 One daemon per scenario, in its own home

A scenario starts the binary it tests as `supragnosis serve --http 127.0.0.1:0 --viz <socket>`.

**The environment** is built from nothing rather than inherited (J2):

- Every variable the binary reads is removed first: each `SUPRAGNOSIS_*`, and
  `CLAUDE_CODE_SESSION_ID`.
- `HOME` is a fresh temporary directory, so the token and any AI-app config the
  scenario writes are that directory's.
- `SUPRAGNOSIS_VIZ_SOCK` is a short path under the system's temporary directory (macOS caps a socket
  path at about 104 bytes).
- `SUPRAGNOSIS_HTTP_ADDR` is the address the daemon reports it bound. It is set on every later
  process of the scenario - the bridge, `setup`, `status` - because without it they fall back to
  `127.0.0.1:7373` and `~/.supragnosis/viz.sock`, which on a developer's machine is the person's
  real daemon.
- `SUPRAGNOSIS_EMBED=hashing`. It is deterministic and downloads nothing; hybrid search runs, and
  matches words, not meaning.
- The store is redb in that home, unless the scenario is about memory only.

**The port is the daemon's to choose.** The harness binds nothing itself and passes port 0. A
daemon told to bind port 0 writes the addresses it actually bound, one line each, to
`$HOME/.supragnosis/listening`, and the harness reads them from there (Section 10). Picking a free
port, releasing it and handing it over races: the bridge's own tests measured 7 rebind failures in
40 runs. A scenario that restarts a daemon (A4, A5) asks for the address it had, and retries while
the port is still in use, with a deadline.

**The token** is read from that home's `mcp.token`, as the bridge reads it.

**When a scenario ends**, passed, failed or panicked, the harness stops every process it started
with SIGTERM and waits for it to exit; one that does not exit in time is killed. Then it removes the
home (J5). The daemon exits normally on SIGTERM (Section 10), which is also what lets an instrumented
daemon write its coverage profile (Section 7).

### 3.2 Three ways in, the three users have

- **An AI app**: MCP over streamable HTTP with the bearer token, or the stdio bridge
  (`supragnosis bridge`) the apps launch, which reads the same token.
- **A person at the console**: the viewer's API over the 0600 socket, where console verdicts are
  cast (`/api/resolve`, `/api/review`).
- **A person looking**: a real browser on the page the daemon serves.

**The browser's proxy, and why it is not the TCP viewer the product refuses.** The viewer answers
only on its socket by design: a viewer on TCP brings back the browser attacks the socket retired,
and a page on any origin could cast a verdict that the console stamps human-confirmed (Taskfile.yml's
header, architecture.md Section 10). A browser cannot speak to a unix socket, so a scenario that needs
one starts a proxy for its own lifetime. What keeps it from being that viewer:

- the store behind it is the scenario's throwaway one, so a verdict cast through it confirms nothing
  anyone keeps;
- it binds a loopback port the system chooses, and answers only requests whose `Host` is that
  address and whose `Origin`, when present, is the page's own. A page on another origin is refused
  before anything reaches the socket, and a name rebound to loopback fails the `Host` check;
- no task, script or document outside the suite starts it.

A browser scenario that casts a verdict through the proxy therefore proves the page's controls work.
It does not prove verdicts are socket-only, which `crates/supragnosis-viz/tests/http.rs` holds
(J6).

### 3.3 Waiting is on state, not time

A scenario waits for something it can observe - a tool's answer, an API field, a line of the page's
text - polling until a deadline. When the deadline passes, it fails with what it last saw (J3). It
never sleeps to synchronise.

## 4. Where the scenarios run

### 4.1 Ontology scenarios: Rust

These are integration tests of the `supragnosis-cli` package, `crates/supragnosis-cli/tests/scenario_*.rs`.

- **The binary.** Cargo builds it first and names it in `CARGO_BIN_EXE_supragnosis`, so the test
  runs what this tree builds.
- **The feature gate.** The tests are compiled only with the package's `scenarios` feature
  (`required-features`). The plain `cargo test` that rust.yml runs on three platforms, and the unit
  coverage in coverage.yml, therefore do not run them twice or fold them into the wrong number. The
  suite's own workflow turns the feature on (Section 8).
- **MCP** is spoken as JSON-RPC over HTTP with the reqwest already in the tree. rmcp's HTTP client
  would bring a second reqwest (0.13 beside the tree's 0.12). Scenario A5 speaks it through the
  stdio bridge instead, as the apps do.
- **The viewer's API** is a plain HTTP request over the unix socket.

### 4.2 Viewer scenarios: a browser

A Playwright project in `scenarios/web/`, at the top of the repository: beside the crates, not inside
the model-evaluation crate. It drives the system's Chrome (`channel: "chrome"`), so no browser is
downloaded. The workflow records Chrome's version in its summary, since the runner image moves it.

- **The binary is a release build.** Only a release build serves the viewer's assets minified, as
  shipped (`crates/supragnosis-viz/build.rs`). The workflow builds it once.
- **Each test** starts its own daemon and proxy through a small Node harness,
  `scenarios/web/harness.mjs`, with the environment of Section 3.1. It seeds the store over MCP with
  a minimal JSON-RPC client in the same file.
- **Assertions read what a person reads.**
  - The status line: its text begins with `ws work / nodes 4 / edges 3`.
  - The inspector's text, the Review page's rows, the empty-workspace message.
  - The activity log the event stream fills. The graph also polls every 2.5 seconds, so a node
    appearing proves nothing about the stream; a line in the log does.
- **The graph is a canvas.** A scenario reaches a node the way a person does, through search or the
  inspector, never by pixel coordinates.

### 4.3 The app's own pages: a browser, with the app's commands answered from the app's examples

Settings and Welcome talk only to the app's commands (`window.__TAURI__.core.invoke`).

- **What the pages receive is the app's merge, not the CLI's documents.** For example,
  `settings_state` is the app's merge of three CLI answers and the daemon's `/api/federation`. An
  example document for each command the pages call is therefore written by the app's own Rust tests:
  they run the app's mapping on the CLI's example documents and check the result into
  `app/tests/fixtures/commands/`. A change to the mapping that changes a page's input then shows in
  review as a diff of that file (J7).
- **The stand-in.** A test loads the page with a stand-in for `window.__TAURI__`. It answers each
  command from those files, and records every call the page makes with its arguments, so a scenario
  can say that a credential went to `server_add` and nowhere else (settings-page.md S2). It emits the
  `setup-line` and `viz-event` events the pages listen for.

## 5. The scenarios

Each scenario has an id, a story, and the test that tells it. The badge counts these tables
(Section 7).

### 5.1 Ontology

| Id | Scenario | Way in |
|---|---|---|
| `A1` | **Record and recall.** An agent observes a decision with two entities and a relation. A search finds it; `get_entity` shows the provenance (host, on-behalf-of, source); `traverse` reaches the related entity; the observation's resource returns the raw text. | MCP/HTTP |
| `A2` | **Contested, then settled by a person.** Two agents give one entity different types. The graph marks it contested, and the `brief` prompt lists both sides. A console verdict (`/api/resolve?observation=...&tier=human_confirmed`) confirms one: it wins at the human-confirmed tier, and the other stays as a competitor (P6). | MCP, socket |
| `A3` | **Merge, and take it back.** An agent proposes merging two spellings; the console accepts (`/api/review?decision=merge`); the graph has one node with both names. A split of that merge (`/api/propose_split?merge=...`), accepted, restores two. The observation log keeps every step (P3). | MCP, socket |
| `A4` | **A restart forgets nothing.** The daemon stops and starts on the same store: the same nodes, edges and ids. `reproject`, run with the daemon stopped, rebuilds the same projection (P16). | process |
| `A5` | **An AI app's session, through the bridge.** The session runs through `supragnosis bridge`, as an app launches it. The daemon restarts mid-session, and the next tool call succeeds once the bridge re-initializes. The daemon records the app's own name as first contact (onboarding.md Section 5). | bridge |
| `A6` | **Two nodes.** A hub and a spoke each run as their own daemon, with their own home, on loopback.<br>The spoke's `identity` gives its node id and key. The hub's `supragnosis.toml` admits it with a bearer hash and the workspaces it shares; the spoke names the hub at `http://127.0.0.1:<port>`, since plain HTTP is allowed on loopback.<br>The spoke pushes a workspace and pulls the hub's, and each sees the other's entries with their origin. The hub narrows what the spoke may read (`POST /api/peer/share`). The next pull of that workspace is refused, and the refusal names it: the pull does not return as if nothing were new (P5). | sync |
| `A7` | **First run in a fresh home.** The home holds `.cursor/`, which is how `connect` finds Cursor without the app. `setup --check` reports the Cursor row to do. `setup --step app=cursor` writes the bridge entry and names the file. A session through that entry, launched with the scenario's environment, is recorded as first contact. Setup appends nothing to the log (onboarding.md O1). The server rows are asserted on Linux only, since on macOS setup also reads the machine's launchd jobs. | CLI |
| `A8` | **Refused, loudly.** A request without the token, or with another one, is refused with a 401 and no tool runs. An observation carrying something shaped like an API key is refused at ingest, without echoing it. The viewer's `/api/explain` for an entity that does not exist answers 404, not an empty entity (P5, P24). | MCP/HTTP, socket |
| `A9` | **A crash loses no write.** A burst of writes, then SIGKILL. The restarted daemon re-projects what the log owed before it serves (crash-recovery.md K3): `status` shows nothing owed, and every write is in the graph. | process |

### 5.2 Viewer

| Id | Scenario |
|---|---|
| `B1` | **The page shows what the store holds.** After A1's entries, the status line's counts match the API's. The workspace chips name the seeded workspaces. The page logs no error. |
| `B2` | **An entry arrives while the page is open.** An agent observes over MCP. The activity log shows the write as it lands (the event stream), and the counts follow. |
| `B3` | **Search, then inspect.** The search finds an entity by a word of its description; choosing the result opens the inspector with its provenance and trust tier. |
| `B4` | **A proposal waits, and a person decides.** A merge proposal from an agent appears on the Review page; accepting it there changes its state and the graph's counts; the notice says so. |
| `B5` | **An empty workspace says it is empty.** With knowledge in other workspaces, an empty one says "Nothing here yet" and names the others with their counts; choosing one shows it (onboarding.md Section 7). |
| `B6` | **A contested value, settled in the viewer.** The inspector shows both values of A2's entity with their tiers; confirming one there makes it the projected value, and the badge leaves the node. |

### 5.3 The app's pages

| Id | Scenario |
|---|---|
| `C1` | **Settings reads the app's answers.** Each section renders from the examples, and the dots match their states. Adding a server refuses an empty form field by field without calling a command. A valid one hands the credential to `server_add` only (S2). |
| `C2` | **Welcome, a fresh machine.** The plan shows Start at Login on and every installed app ticked. "Set up" runs the server step, then each ticked app, in that order. Each step's streamed lines appear in its drawer, and the cards follow the checklist read afterwards. |
| `C3` | **Welcome, a CLI that predates setup.** The page says to upgrade the server, and offers nothing it cannot do. |

## 6. What is left out, and where it is held

- **The hub's read tier** (`/viz/`) was left out until 0.11.0 removed it (remote-server.md Section
  13).
- **The console's merge band and reification**, and the dreaming surface, are held by their unit
  and scenario tests.
- **The tray**, **Start at Login with launchd**, and **Restart Claude Desktop** touch the person's
  own session. They stay with the clean run.

## 7. Coverage, as badges

**Scenario coverage** is the suite's own measure: how many of the scenarios in Section 5 are built,
and whether they pass.

- **Mapping scenarios to tests.** Each test names its scenario. A Rust test's name begins with the
  scenario's id in lower case (a1 for A1). A Playwright test's title begins with the id itself.
- **The check.** It runs after the tests, even when they fail (J8):
  - it reads Section 5's tables and collects the ids;
  - it reads the results: the libtest lines (`test <path>::a1_... ... ok`) from the scenario tests'
    output, and Playwright's JSON report;
  - it matches the two. A scenario with no test is unbuilt. A test whose id is in no table fails the
    check, because a story that is not written down is not counted.
- **The badge** reads `e2e | 14/18`: scenarios passing over scenarios specified.
  - It is green when every specified scenario passes.
  - It is gold while some are unbuilt and every built one passes.
  - It is red when a built scenario fails.
- **Where it is published.** As `e2e.json` on the `badges` branch, beside line coverage. The run's
  summary lists each scenario with its result.

**Line coverage under the scenarios** is the second badge. It shows how much of the daemon's code a
real session reaches, beside the unit tests' figure the README already shows.

- **The recipe.**
  1. `source <(cargo llvm-cov show-env --sh)`, then `cargo llvm-cov clean --workspace`.
  2. Build the binary in that environment. Every process it starts then writes a profile on a
     normal exit, into the directory `LLVM_PROFILE_FILE` names.
  3. Run the scenarios. The harness keeps `LLVM_PROFILE_FILE` when it builds a daemon's
     environment: it is the one variable it does not strip.
  4. `cargo llvm-cov report`.
- **The daemon must exit normally.** A daemon that dies on SIGTERM writes no profile. That is one
  reason Section 10 makes SIGTERM a normal exit.
- **What it reads.** The badge shows `e2e lines | 52%`, published as `e2e-lines.json`. It is a
  reading, not a gate, for the reason coverage.yml gives.
- **When it runs.** An instrumented binary is slower, so this runs on main pushes only.

## 8. CI

A workflow of its own, `scenarios.yml`, on ubuntu-24.04.

- **When it runs.** On pull requests and main pushes that touch the crates, the app's pages,
  `scenarios/` or the workflow.
- **The steps.**
  1. Build the debug binary with the `scenarios` feature, and a release binary for the browser.
  2. Run the Rust scenarios.
  3. Install `scenarios/web/`'s locked dependencies, and run Playwright against the system Chrome.
  4. Run the scenario check (Section 7) with `if: always()`, and write the summary.
- **What a failure keeps.** Playwright's traces and screenshots, and each daemon's log, as artifacts
  for a week.
- **Publishing.** Only on main, as coverage.yml does: a separate job with `contents: write` that runs
  no code from the tree.

**The budget is ten minutes.** A scenario that needs more is split, or moved into a unit test.

**Locally**, `task scenarios` runs the same steps, and `task scenarios -- --headed` opens the
browser. `task test:e2e` stays the model evaluations.

## 9. Invariants

| | Invariant |
|---|---|
| **J1** | A scenario runs the product's binary, built from the tree, through the entry points its users have: `serve`, MCP over HTTP with the bearer, the stdio bridge, the viewer's socket, and the page the daemon serves. The browser scenarios run a release build, as the viewer's assets ship. Never an engine built inside the test. |
| **J2** | A scenario is hermetic. Its processes start from an environment built for them - every variable the binary reads removed, then its own home, socket, address and embedder set. It reads and writes nothing of the person's (no `~/.supragnosis`, LaunchAgent or AI-app config), and reaches no network beyond loopback. |
| **J3** | A scenario waits on observable state with a deadline. When the deadline passes, it fails with what it last observed. It never sleeps to synchronise. |
| **J4** | A scenario asserts what a person or an agent sees: a tool's answer, an API field, the page's text. Screenshots are evidence on failure, never compared. |
| **J5** | Every process a scenario starts is stopped when it ends, whether it passed, failed or panicked. A run leaves no daemon, proxy or socket behind. |
| **J6** | The browser reaches the viewer only through the proxy of Section 3.2: started and stopped by the harness, for a throwaway store, on a port the system chose, refusing a foreign Host or Origin. The product opens no viewer port, and nothing outside the suite starts the proxy. |
| **J7** | The app's pages are tested with commands answered from examples the app's own tests produce from the CLI's example documents. A page cannot pass on input the app does not build. |
| **J8** | Every scenario in Section 5 is either told by a test that names its id or counted as unbuilt. Every scenario test names a scenario in Section 5. The badge is that count, made by the check, never a figure typed by hand. |

## 10. What this revises

- **daemon-lifecycle.md:**
  - `serve` writes the addresses it bound to `$HOME/.supragnosis/listening`, one line per listener,
    when it was asked for port 0. Today it logs the address it was asked for.
  - `serve` exits normally on SIGTERM: it stops accepting, lets requests in flight finish for a
    moment, and exits 0. `stop` and launchd already send SIGTERM. The daemon now answers it instead
    of dying on it. A daemon the app spawned and the app's own exit both benefit.
- **architecture.md Section 14** names the suite and its badge.
- **The README** gains the scenario badge beside the line-coverage one.

## 11. Ordering

1. **This document**, and its registry rows (J1-J8, deferred).
2. **The daemon's half:** the bound-address file and a normal exit on SIGTERM, each with a test.
   [built; the hub's sync listener writes its line with A6]
3. **The Rust harness, A1-A4 and A8**, behind the `scenarios` feature, and the scenario check with
   its badge (J8). [built]
4. **The browser harness and B1-B6:** the proxy, the Node MCP seed, and Playwright on the system
   Chrome. B5 needs the empty-workspace message from PR #168.
5. **The app's pages, C1-C3:** the app's command examples and the stand-in. C2 and C3 need the
   Welcome page from PR #168.
6. **A5-A7 and A9:** the bridge, two nodes, the first run, the crash.
7. **Line coverage under the scenarios**, and its badge.

## 12. Decisions

- **D1. Playwright, on the system's Chrome.** Its waiting, tracing and screenshots are what a hand
  CDP script would have to rebuild, and using the installed Chrome downloads nothing. It is a
  test-only npm dependency in `scenarios/web/`, locked and kept current by Dependabot, like the
  viewer's ESLint.
- **D2. The ontology scenarios are Rust integration tests of the CLI package, behind a feature.**
  Cargo hands them the binary it built. MCP is plain JSON-RPC over the tree's reqwest, and the bridge
  where an app's path is the point.
- **D3. Every pull request that touches code runs the suite.** A seam broken by a change should fail
  that change. The instrumented line-coverage run is main-only.
- **D4. No pixel comparisons.** Screenshots are evidence (J4).
- **D5. The desktop window is out of scope.** There is no WebDriver for the macOS webview. The pages
  are covered with the app's own examples (J7), and the window by the clean run.
- **D6. Two badges.** Scenario coverage (`14/18`) says how much of the written story is guarded.
  Line coverage under the scenarios says how much of the code a real session reaches.

## 13. Closure map

| Demand | Where it is answered |
|---|---|
| P5 - a refused or unanswered read is not an empty one | A6, A8; J3's failures say what was last seen |
| P24 - degrade loudly, refuse when proceeding is worse | A8, J3, J5, J8 |
| P3 and P6 - nothing is erased, and conflict is kept | A2, A3 |
| P16 - the same log gives the same projection | A4, A9 |
| architecture.md Section 14 - every guard runs | Sections 7 and 8: the suite runs on every change, and its count is published |
| compatibility.md Section 6 - one example per machine-read answer | Section 4.3, J7 |
| onboarding.md Sections 5 and 7, O1 | A5, A7, B5 |

## 14. What the review changed

The draft was read once against the code and the sibling documents by a reviewer who had not written
it. Sixteen findings changed it:

1. **The port.** `serve` logs the address it was asked for, so port 0 was useless to the harness, and
   picking a free port races. The daemon now writes what it bound (Section 10).
2. **Line coverage needs a normal exit.** A daemon killed by SIGTERM writes no profile, and the draft's
   recipe mixed two modes of cargo-llvm-cov. Section 7 gives the recipe, and Section 10 the exit.
3. **The pages' input is the app's, not the CLI's.** `settings_state` merges three CLI answers and a
   daemon route, so J7 now uses examples the app's tests build.
4. **A6 asserted silence where the hub refuses.** A narrowed workspace answers 403. The scenario
   asserts the refusal (P5), and spells out the hub and spoke configuration.
5. **The proxy against the Taskfile's warning.** Section 3.2 now says why a scenario's proxy is not
   the TCP viewer the product refuses, how it is hardened, and that it does not prove verdicts are
   socket-only.
6. **The environment leaked.** The binary reads many variables, and later processes fall back to the
   person's real daemon. J2 now builds the environment from nothing.
7. **A7 depended on the machine.** Cursor is found through `.cursor/`, and the server rows are
   asserted on Linux only.
8. **The scenarios would also run in rust.yml and coverage.yml.** They are behind a feature now.
9. **Section 1 overclaimed.** The bridge restart, the token refusal and the socket's mode are already
   tested, and the last releases' fixes were not of this kind.
10. **J1 and the debug binary.** The browser scenarios run a release build, which minifies the viewer.
11. **rmcp's HTTP client** would add a second reqwest. MCP is plain JSON-RPC over the existing one.
12. **Routes and principles.** A2 settles with `/api/resolve`, which is P6. A3 accepts and splits with
    their own routes. A8 asks `/api/explain`. A1's search matches words under the hashing embedder.
13. **What the browser can prove.** The graph polls, so B2 reads the activity log. B5, C2 and C3 wait
    on PR #168. The status line is matched by its beginning.
14. **The check's detail.** It parses libtest's text and Playwright's JSON report, and runs even when
    tests fail.
15. **Smaller points.** The web project moved out of the evaluation crate, and the task names no
    longer overlap. P5's wording was turned the right way round, and the socket's location was made
    consistent.
16. **Missing scenarios.** A9 (a crash), A8's secret refusal and B6 (a contested value settled in
    the viewer) were added. The hub's read tier is named as left out, with where it is held
    (Section 6).

## 15. What building it changed

- **No random path prefix on the proxy.** The draft put every proxied path under a prefix chosen per
  run. The viewer's page fetches its API by root-relative paths (`/api/graph`), so a prefix breaks
  the page. The `Host` and `Origin` checks carry what the prefix was for.
- **The badges branch holds every badge.** coverage.yml replaced the branch with its one file, which
  would have erased the scenario badge on every main push. Both workflows now replace their own file
  and keep the others, with a lease on the commit they read, and retry when they race.
- **A tool's refusal is an answer with an `error` field**, not always an MCP tool error: a secret
  refused at ingest comes back that way. The harness reads both as a refusal.
