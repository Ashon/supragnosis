# Onboarding - from install to the first entry, inside the app (Principles 3, 17, 19, 24)

> What a person does between installing supragnosis and seeing the first entry their own work made,
> and how the desktop app walks them through it without a terminal. One procedure, `supragnosis
> setup`, decides what is done and does what is not; the app's Welcome page draws it and shows, step
> by step, what it ran. Companion to [client-connect.md](client-connect.md) (connecting AI apps),
> [daemon-lifecycle.md](daemon-lifecycle.md) (who runs the daemon),
> [settings-page.md](settings-page.md) (the app's own pages) and
> [semantic-retrieval.md](semantic-retrieval.md) (the search model).
>
> Status: **the daemon, the CLI, the app and Linux are built** (Section 13 steps 2-4); the clean
> run is not. The choices it records were made by the author on 2026-10-18 from a storyboard.

## 1. Why this exists

The product's first experience is its bottleneck: no one outside the author's machines has run it for
long, and the differences it offers are only felt once an AI app is writing to it. Traced through the
code at 0.6.4, a new person meets this:

- **No AI app is connected.** Neither the Homebrew cask nor the install script runs `connect`. The
  app says "no AI app connected - see Settings" in the tray, and Cmd+, opens the Server section.
- **The daemon the app starts dies with the app.** With Start at Login off, which is the default, the
  app runs the daemon as its own child; quitting the app stops it, and a connected app's next tool
  call fails.
- **The search model is missing and nothing says so.** A daemon the app spawned never downloads the
  224 MB model (semantic-retrieval.md Section 6), so search is keyword only, and the Settings page does
  not show the search state at all.
- **Nothing confirms that an app reached the daemon.** `connect` does not check that a daemon
  answers, and the first sign of trouble is a tool error inside the AI app. `status` on a stopped
  machine prints "stopped" and nothing else.
- **The first screen reads as empty.** The viewer opens on the default workspace, which is empty by
  design, while the session hook names a workspace after the repository.
- **Linux has no always-on daemon by default**: the install script prints a `serve` command and a
  link to a systemd user unit to copy by hand (deploy/systemd/README.md), which also needs lingering
  turned on to survive a logout.

The install script lists the steps under "Onboarding:", and the Homebrew formula's caveats name two
of them, but both scroll past in the installer's output and are gone by the time the app opens.

**The first useful moment** is defined here as: *an AI app the person uses reaches the daemon, and
the first entry their own work makes appears in the graph.* Everything in this document shortens
the path to it.

## 2. What this is NOT

- **Not a tour.** The Welcome page teaches nothing about the product; it gets the four things done
  that the product needs, and points at where work happens.
- **Not a demo workspace.** Setup writes nothing to the store (O1). The log is append-only (P3), so a
  sample entry written to check the pipe would stay in it forever.
- **Not a terminal inside the app.** The app shows what it ran in a log drawer (Section 6); it opens
  no shell.
- **Not an app that carries its own server.** The app still finds the CLI the Homebrew formula
  installs; making it self-sufficient is recorded in Section 10, as client-connect.md Section 7
  recorded it, and left out of these steps.
- **Not a second place for settings.** The Welcome page uses the Settings page's parts and its acts.
  Changing a choice later happens in Settings, which already has each of them.
- **Not for a remote profile.** When this Mac's AI apps use a remote server (settings-page.md Section
  3.1), the daemon, model and first-entry steps are `skipped`, since contacts and entries go to the
  hub; connecting apps still applies, since the bridge follows the profile.

## 3. One procedure, two faces

**`supragnosis setup`** knows the four steps of Section 4. For each it reads the live state, decides
whether the step is done, and if not, does it. It keeps no record of its own progress: the checklist
is computed from `status` and `connect` every time (O3), so running it again shows only what is left
and does only that.

- `supragnosis setup` - prints the plan, asks "Proceed? [Y/n, c to choose]", applies it and says what
  each step did. The text face, for a terminal and for Linux, where there is no app.
- `supragnosis setup --yes` - applies the plan with its defaults, asking nothing. It never replaces an
  entry `connect` did not write (client-connect.md C3), and never takes over a manager setup would
  only report (Section 4.1).
- `supragnosis setup --check` - only reads, and prints the checklist (like `connect` with no app,
  client-connect.md C5).
- `supragnosis setup --step <step>` - applies one step: `server`, `app=<id>`, `model`, or `wait`.
  `wait` waits for the first entry (Section 4.4) for up to ten minutes, or `--timeout <seconds>`, and
  exits 0 when it arrives and 2 when it does not, which is not a failure.
- `--events` - with any of the above, writes one JSON object per line instead of text. The first line
  names the schema (`{"schema": 1}`); each after it carries the step, its state, one sentence saying
  what happened, and while an act runs, the command it ran as text, a line of that command's output,
  or a file it changed. A last line summarizes.

**States.** `todo` - not done, and setup can do it. `working` - under way: an app connected and not
yet heard from, a download running. `done`. `attention` - not done, and setup will not do it without
being asked; the sentence names the command that would. `failed` - an act failed; the sentence says
why and what to do. `skipped` - does not apply here: a remote profile (Section 2), an app that is not
installed, or a node with no search model to fetch (`SUPRAGNOSIS_EMBED=none`, or a build without one).

**The Welcome page draws only what setup says** (O4). Its cards come from `setup --check --events`,
each act it offers runs `setup --step ... --events`, and the log drawer under a card is that step's
events. Its own acts are few and named: it stops the daemon it spawned before the server step runs
(the CLI cannot see that child, which is why the Start at Login switch stops it first today), it
restarts or opens an AI app (Section 4.2), and it runs `--check` again when the viewer's event stream
reports an entry. Since the CLI cannot tell the app's child from any other process it does not
manage, the app passes that child's pid (`--app-daemon <pid>`), and setup reads it as the app's
daemon only while that pid is alive and is supragnosis. An app whose CLI is too old to know `setup`
says so, and names `brew upgrade supragnosis-server`.

Failures follow the lifecycle's rule (daemon-lifecycle.md L4): a step that fails exits non-zero and
says why and what to do, in its events and in its text (O2).

## 4. The steps

### 4.1 Keep Supragnosis running

*(Titled "Keep it running" until design-system.md step 45, which names the product: "Keep supragnosis
running" in the CLI's checklist.)*

**State:** who manages the daemon, whether it answers, and whether it starts at login (`status
--json`). **Done** when a daemon answers and a manager that outlives the app keeps it: the canonical
login job, or another manager the lifecycle recognizes (daemon-lifecycle.md L2). A daemon only the app
or `supragnosis start` runs is `todo`.

**Act:** `service install` (daemon-lifecycle.md Section 4), asked not to fetch the model
(`--no-model`): the model is step 4.3, and the daemon does not wait for it. What setup takes over, and
what it only reports (O8):

- **Taken over:** a daemon the app spawned (the app stops it first, Section 3), a pidfile daemon from
  `supragnosis start`, and a retired label - the cases where nobody chose a manager.
- **Reported as `attention`:** a Homebrew service that does not answer, a hand-written plist that
  nothing has loaded or that a take-over would move aside, two managers at once, anything
  answering that the lifecycle cannot name, and a daemon that cannot open its store because the
  store was indexed by another model - whose sentence names both models, Settings > This Mac >
  Rebuild index and `supragnosis embed rebuild` (semantic-retrieval.md Section 13); in the app the
  card offers Rebuild index, which runs the same command This Mac's button does. The sentence names the command that would act, most
  often `supragnosis service install --take-over`; in the app the card offers "Take over", and the
  click is the consent the Start at Login switch asks for today (L1, L6). A Homebrew service or a
  hand-written login job that keeps the daemon answering is a manager that outlives the app, so the
  step is done and setup leaves it as it is.

**Default:** on (D2). Nothing is applied until the person acts: the Welcome page shows the plan with
Start at Login checked, and one "Set up" click applies it, as rustup's "Proceed" does. Turning it off
there, or later in Settings (settings-page.md Section 3.3), is a choice the app remembers, so Welcome
and the tray do not ask again; `setup` itself still reports the step as `todo`.

**On Linux** `service install` generates a systemd user unit at
`~/.config/systemd/user/supragnosis.service`, enables it with `systemctl --user`, and asks
`loginctl enable-linger` so it survives a logout; a refused linger is `attention`, since it may need
an administrator. A unit already at that path that setup did not generate - the one
deploy/systemd/README.md tells people to copy - is never overwritten (L5) and counts as done when it
is enabled and answering. `status`, `restart` and `stop` recognize the unit as they recognize the
LaunchAgent (L2). The unit binds and authenticates as the daemon does by default (L7) and needs no
administrator rights (L8). Where `systemctl --user` is unavailable, as in many containers, the step
is `attention` and names the `serve` command. daemon-lifecycle.md Section 4, "On Linux", has the
unit's rules.

### 4.2 Connect your AI apps

**State:** each app's row from `connect --json` (installed, connected, capture), and the first
contacts of Section 5. **Done** when at least one installed app is connected through the bridge.
Whether a connected app has reached the daemon is shown on its row, as "reached" with the time or as
waiting, but does not hold the step open: first contact lives in the daemon's memory and is gone
after a restart, which the server step itself may cause.

**Act:** `connect <id>` for each app the person chooses (client-connect.md Section 4). For an app
configured by file it backs the file up first (C3); for one registered through the app's own CLI the
event says which command it ran. For Claude Code it registers the capture hook unless the person
turns it off (client-connect.md Section 12.4). The default choice is every installed app without an
entry; an entry `connect` did not write is `attention`, and setup leaves it (C3).

After connecting, an app still has to load the change, and setup says how for each one, in the words
`connect` prints today. Two of them the app can do for the person (D9):

- **Claude Desktop:** "Restart Claude Desktop" asks it to quit as the Dock's Quit does, waits for it to
  exit and opens it again. The app forbids `unsafe` code, so it does not call the running-application
  interface itself: it sends the same quit Apple event through `osascript` and opens the app with
  `open -b`. Sending an Apple event needs the Automation permission, so the app declares it - an
  `NSAppleEventsUsageDescription` in `app/Info.plist` and the
  `com.apple.security.automation.apple-events` entitlement in `app/Entitlements.plist`, both of which
  the bundler signs into the release - and macOS asks the person once; a refusal leaves the
  instruction in place. The page offers it on a row it connected itself.
- **Cursor:** "Open Cursor" opens it, with where to turn the server on.
- **Claude Code, Codex, Gemini, OpenCode and VS Code** load it in their next session or chat, which
  only the person can start (OpenCode may need its background service restarted; client-connect.md
  Section 13.3).

`connect` itself warns, in its own output, when the server the active profile names does not answer:
the entry is written, and the app will fail until a server runs.

### 4.3 Search by meaning

**State:** whether the model the daemon is configured with is complete in the cache the daemon reads
(the daemon's own report when it answers, otherwise the model and directory this command's
environment names). A model that is complete on disk and that the daemon still reports off did not
load: that is `attention`, with the daemon's reason, not `todo`. **Complete** means every file the model needs is linked under the cache's `snapshots`
directory; a file still downloading (`.part`) never counts. Today's check counts any file over a
megabyte, so a half-finished download reads as present, the daemon tries to load it, the load fails,
and a failed load is not retried until a restart; this document revises that (Section 12). **Done**
when complete; `skipped` on a node with nothing to fetch (Section 3).

**Act:** `embed fetch --events`, which turns off the terminal progress bar and instead reports the
bytes the model's directory holds against the model's size, about twice a second, and ends by
checking the files. The daemon loads the model on its next search, without a restart
(semantic-retrieval.md Section 6).

**Default:** the app starts the download in the background as part of "Set up" (D3); "Not now" stops
it, and the model step stays `todo`. Nothing waits for it (O5): the other steps and the first entry go
on, and every search answer says it is keyword only until the model is complete. In the terminal,
`setup` runs it last, with a progress line; interrupting it leaves keyword search, and the next run
fetches what is missing.

### 4.4 Try it

**State:** whether the store holds an observation. **Done** when it does; `skipped` on a remote
profile.

**Act:** none on the store (O1). The step suggests what to ask in a connected app, about the person's
real work: "Remember one decision we made in this project today, and why." It is not a sample sentence
to paste, since a made-up fact would stay in the log for good (P3). Then it waits: the app, on the
viewer's event stream (remote-viewer.md Section 3.6), for the next observation, which it reads by
the id the event carries; `setup --step wait`, by reading the newest entry across every workspace
every two seconds until there is one (a store that already holds one is done at once). When the entry arrives the step shows what it said and in which workspace,
and the app offers "Open the graph" at that workspace.

*(Revised by design-system.md Section 29.10, step 48.)* The done step carries the entry's first 140
characters, the app that wrote it when the local read names one, and its first entity when it names
one, and says "An entry is in "billing", from Claude Code, just now."; the page quotes the entry,
offers "Show in the
Graph" with that entity selected, and announces it. While it waits, the sentence follows first
contact - no client has reached the server since it started, a client that has, or, unknown, any
connected app - and quotes the question in every case, without making contact a condition of the
step (Section 5). The page counts the minutes from one, and from two shows the checks the event
carries (O4); `setup --step wait` prints them then too. Copy Diagnostics drops the entry's words, its
app and its entity from what it copies (O6).

## 5. First contact

When an AI app talks to the local daemon, the daemon notes who it was. The name is the client's own:
the `clientInfo` of the session's `initialize`, or, in a session that skips `initialize` as newer
protocol versions allow, the client information a request carries in its metadata. A session that
gives neither counts as "an unnamed client". For each name the daemon keeps the version, the time of
the first request and of the last, in memory only.

- **Where it is seen:** on the viewer's local socket at `/api/clients`, and in `status --json` as
  `clients`. Not on the read tier: `/api/clients` is refused there (remote-viewer.md Section 3.2). A
  hub's listener for remote principals records nothing, and nothing about it reaches a peer (O7).
- **Through the bridge** the app's requests reach the daemon as the app sent them (client-connect.md
  Section 3: the bridge does not read or rewrite them), so the name is the app's, not the bridge's.
- **Which app it is:** setup maps the names it knows to the apps `connect` lists, by the one table
  in the core crate that `connect` and the reads naming an app also use (design-system.md Section
  29.8), and shows a name it does not know as sent.
- **What it is not:** a condition any step waits on. It says an app reached the daemon since the
  daemon started; a restart forgets it. What an app wrote is recorded apart from it, with each
  attestation its session writes (design-system.md Section 29.8).

## 6. The app: Setup

**A page in the main window**, `assets/welcome.html`, under the same title bar, titled "Setup" after
its way back, "< Graph" (settings-page.md Section 3.0): no new window, and no navigation in the title
bar but that way back.
It is called Setup wherever it is reached (design-system.md Section 6a); "Welcome" names it in this
document's history and in its file.

**When it shows.** On the first launch, and on a later launch while step 4.1 or 4.2 is not done,
unless the person closed it with "Finish later" or turned Start at Login off by choice. The app
remembers only those two things. While either step is not done, the tray offers "Finish Setup...",
and the Settings page's About section offers "Open Setup" at any time (settings-page.md Section 3.5).

**Before anything is applied** the page shows the plan with its defaults - Start at Login on, every
installed app without an entry checked, the model to download - and one "Set up" button applies them.
Every item of the plan is a checkbox whose label is its row, including an app's "Switch to the
bridge" and the daemon's "Take over", which wait for Set up like the rest and start unchecked, since
each replaces what the person set up (O8); Set up is the default button. Once the plan has run, each
card keeps its own act for later, which acts when pressed: the switch, a row's Connect or Disconnect,
"Not now".

**What it shows.** Four cards, one per step, each with its state, its sentence, and its act: the
Start at Login switch, a row per AI app with Connect and its helper, the model's progress with "Not
now", and the sentence to try with the first entry when it comes. Under each card a **log drawer**
holds that step's events as they arrive: the command, its output, every file changed and the copy
where one is made, and the exit status (D8). "Copy Diagnostics" copies every drawer with
`setup --check --events`, `status --json`, the shell's status and the daemon log's last lines, for a
report, and shows once a step has run or failed in this visit, or the check could not be read
(design-system.md Section 29.5).

**Why a log drawer and not a terminal.** A terminal shows the same output, but as text the page
cannot read, so the cards could not follow it; it puts a shell inside the app and invites typing into
it; and it adds a pseudo-terminal and a terminal widget to the app. The drawer shows what was run and
what it changed, and the cards stay the truth about state.

**The privacy line** at the foot: "Supragnosis keeps your memory on this Mac. It is shared only if you
add a remote server or set up sync." (O6). Adding a server changes which server the AI apps use, and sync
is set up in supragnosis.toml, so the line names both.

**Who may act.** The page is the app's own, and it changes settings the way the Settings page does:
it joins that page as a caller the app's commands admit (settings-page.md Section 4), and its commands
are declared and granted the same way.

**Settings** gains the search state in This Mac's daemon section (settings-page.md Section 3.3):
"Search by meaning" or "Keyword only - the model is not downloaded", with the download.

**What building it settled.**
- The two remembered choices, and whether the page was ever shown, are one small file in the app's
  settings directory (`~/Library/Application Support/dev.supragnosis.desktop/welcome.json`).
  Closing the page with its x is navigation, not "Finish later": the page comes back at the next
  launch while a step is not done.
- The page reads `setup --check` again every 15 seconds while it is shown, after each act, and once
  shortly after the viewer's event stream reports anything until the first entry is in.
- Each act runs as its own process group, so "Not now" stops the download `setup --step model`
  started as well as setup itself, and the next run fetches what is missing.
- A command a sentence names is set in code type on the page; the sentence is still setup's.

## 7. Words where people land

The places a person meets without the Welcome page say what to do next:

| Where | Today | After |
|---|---|---|
| The viewer, an empty workspace | no nodes in this workspace - observe knowledge, or pick another workspace | Nothing here yet. Ask a connected AI app to remember something, or pick a workspace that holds knowledge, each named with its count. |
| The viewer's Log, an empty workspace | no observations in this workspace yet - observe knowledge to see the log | Nothing here yet. What an AI app remembers in `<workspace>` shows up here. (design-system.md Section 29.10) |
| The app's map, an empty store | the viewer's sentence alone | the same sentence, and Open Setup (design-system.md Section 29.10) |
| `status`, daemon stopped | stopped | stopped, then how to start it now, how to keep it running, and `supragnosis setup` for a first run |
| `connect`, no daemon | (no check) | the connection is made, and the app will fail until a daemon runs: `supragnosis setup` |
| The `brief` prompt, an empty workspace | counts of zero | says the workspace holds nothing yet, and that recording decisions as work happens is what fills it (prompts.md Section 4) |
| The app, CLI not found | points at the install script | names the Homebrew formula for a cask install, and the install script otherwise |

## 8. The installers and the landing

- **The landing's install command** becomes `brew install --cask ashon/tap/supragnosis && open -a
  Supragnosis` (design-system.md Section 11), so one line ends with the Welcome page open. The line
  under it and Get started name Setup rather than `connect`, and keep `supragnosis setup` for Linux
  and a server (design-system.md Section 29.10).
- **The cask's caveats** say to open the app to finish setting up.
- **The install script** ends with "Next: supragnosis setup" instead of its own list, and when a
  terminal is attached it asks whether to run it now, reading the answer from `/dev/tty`, since under
  `curl | sh` its standard input is the script.

## 9. Decisions

- **D1. First contact, not a test entry.** A test entry would prove the pipe and stay in the log
  forever (P3). The `initialize` an app sends anyway proves the same pipe and costs nothing.
- **D2. Start at Login on by default in Welcome.** An app that is connected but finds no daemon the
  next morning is the failure the product most needs to avoid. It is a default in a plan the person
  applies with one click, not something done before they click.
- **D3. The model downloads by itself, in the background.** It is what makes search work across
  languages and phrasing, and it blocks nothing; "Not now" is one click.
- **D4. The app leads; the CLI is the same procedure.** The app is where a non-developer is; the CLI
  is where a developer and Linux are. One decision procedure keeps them the same.
- **D5. Linux gets a user unit.** The only always-on path today is a hand-run command.
- **D6. A "Try it" step.** The first useful moment is an entry, not a green check, and the entry is
  the person's own: the step suggests what to ask, never what to write.
- **D7. Homebrew stays the installer for now.** One line installs and opens the app; carrying the
  server inside the app is Section 10.
- **D8. A log drawer, not a terminal** (Section 6).
- **D9. The app restarts Claude Desktop for the person,** as the Dock would; other apps are told how,
  or opened.

## 10. Later: an app with the server inside - recorded, not decided

Removing the last terminal line means downloading one app from the website. That needs the server
inside the app (a sidecar binary, about 52 MB), an update the app can install itself (signed, as
the release already is), and a rule for which binary runs when Homebrew's is also present.
client-connect.md Section 7 records the same trade-offs. It changes the release pipeline and the
lifecycle's idea of who owns the daemon, so it gets its own document when it is taken up.

## 11. Invariants

| | Invariant |
|---|---|
| **O1** | Setup appends nothing to the knowledge log: its checks read, its first-entry step waits for an entry the person's own work makes, and every path it changes is named in its events. |
| **O2** | Every step reports its outcome where it ran - what it did, each file it changed and the copy where client-connect.md C3 or daemon-lifecycle.md L5 makes one, or why it failed and what to do - and a failed step exits non-zero (P24). |
| **O3** | The checklist is computed from the live state each time it is shown; `setup --check` only reads, and running setup again changes only what is not done. |
| **O4** | Every state the Welcome page draws comes from `setup`'s events, except how long Try it has waited in this visit, which the page counts itself and uses only to show the minutes, the checks the event carries and the foot's Copy Diagnostics (design-system.md Section 29.10). Its own acts are only stopping the daemon it spawned before the server step, restarting or opening an AI app, rebuilding the search index when the server step says the store was indexed by another model, and asking `setup` again when an entry arrives. |
| **O5** | The model never blocks: the server, the apps and the first entry proceed while it downloads, search says keyword only until the model is complete, a download in progress never reads as complete, and stopping it leaves nothing broken. |
| **O6** | Nothing leaves the machine during setup: it never configures sync or a remote server, and the page says so. |
| **O7** | First contact is the client's own name, kept in the local daemon's memory; the name of the app that wrote an attestation is written to this node's store, in a table of its own (design-system.md Section 29.8, by the author's decision); neither is served on the read tier, recorded by a hub's remote listener or sent to a peer. |
| **O8** | Setup takes over only a daemon nobody chose a manager for - the app's child, a `start` daemon, a retired label - and reports a Homebrew service, a hand-written plist or an unnamed holder until the person asks (daemon-lifecycle.md L1, L6); a Linux user unit overwrites no unit it did not generate (L5), adds no exposure and needs no privilege (L7, L8). |

## 12. What this revises

- **client-connect.md Section 4**: `connect` warns when the server the active profile names does not
  answer.
- **daemon-lifecycle.md Section 4**: `service install` takes `--no-model`; on Linux it generates and
  enables a systemd user unit and asks for lingering. **L2** recognizes that unit on Linux, and **L7**
  and **L8** read for a user unit as they do for a LaunchAgent.
- **semantic-retrieval.md Section 6**: the model is present when every file it needs is linked under
  `snapshots`, never because a partial file is large; a load that finds a download in progress is
  missing, not failed, and is tried again (**SR2**); `embed fetch` takes `--events`.
- **settings-page.md Section 3.3**: This Mac's daemon section shows the search state and offers the
  download.
- **settings-page.md Section 3.5**: About offers "Open Setup".
- **settings-page.md Section 4**: the Setup page is a caller the app's commands admit, alongside the
  Settings page, with its commands declared and granted the same way.
- **settings-page.md Section 5**: the tray offers "Finish Setup..." while the daemon or the apps are not
  set up.
- **remote-viewer.md Section 3.2**: `/api/clients` is refused on the read tier.
- **prompts.md Section 4**: `brief` says what an empty workspace means.
- **design-system.md Section 11**: the landing's install command opens the app.

## 13. Ordering

1. **This document** and its registry rows.
2. **The daemon and the CLI**: first contact (`/api/clients`, `status --json`), `setup` with
   `--check`, `--step`, `--yes` and `--events`, the complete-model check and the loader's retry,
   `embed fetch --events`, `service install --no-model`, `connect`'s warning, `status`'s stopped
   wording, `brief`'s empty wording and the install script's ending. It ships in a release, since the
   app reads it.
3. **The app**: the Welcome page and its log drawers, the two helpers (and, if restarting Claude
   Desktop needs it, the usage description and the entitlement, which the signing step outside this
   repository must carry), "Finish setup..." in the tray, the search row and "Run setup again" in
   Settings, the caller rule, the viewer's empty state, the CLI-not-found message, the cask's caveats
   and the landing's install command.
4. **Linux**: the generated user unit, lingering, and the lifecycle's recognition of the unit.
5. **A clean run**: both install paths from an empty home directory to the first entry. The app's run
   opens windows on the person's screen, so it is announced before it starts.

## 14. Closure map

| Demand | Where it is answered |
|---|---|
| P3 - nothing is erased, so nothing is written to try | Sections 2, 4.4, 9 (D1); O1 |
| P5 - absence is not falsehood: an empty workspace says it is empty | Section 7 |
| P17 - nothing leaves the machine | Sections 6, 4.2; O6, O7 |
| P19 - the model widens search and never blocks | Section 4.3; O5 |
| P24 - fail loudly, where it is read | Sections 3, 6, 7; O2 |
| daemon-lifecycle.md L1, L5-L8 - one owner, the operator's file, no exposure, no privilege | Section 4.1; O8 |
| client-connect.md C3, C5 - backups, a read-only listing | Sections 3, 4.2 |

## 15. What the review changed

The draft was read once against the principles, the sibling documents and the code by a reviewer who
had not written it. Twelve findings changed it:

1. **The app is not a pure renderer.** The CLI cannot see the daemon the app spawned, so the app has to
   stop it before the server step, as the Start at Login switch already does; O4 now names the app's
   own acts instead of claiming it has none.
2. **The Welcome page must be an admitted caller.** The app's commands admit only the Settings page
   today; Section 12 adds the Welcome page under settings-page.md Section 4.
3. **A default is not consent.** Showing Start at Login already on and applying it would take over
   without the click L6 relies on. The plan is shown, and one "Set up" click applies it; Homebrew's
   service, a hand-written plist and an unnamed holder are reported, not taken over (O8).
4. **A half-downloaded model read as present.** The presence check counts any file over a megabyte,
   `.part` files included, and a failed load is not retried, so a background download could leave
   search keyword only until a restart. Section 4.3 defines a complete model, and Section 12 revises
   the check and the loader.
5. **Not every session sends `initialize`.** First contact also reads the client information a request
   carries, and an unnamed client still counts.
6. **"Reached" cannot gate a step.** It lives in memory and the server step may restart the daemon, so
   step 4.2 is done when an app is connected, and reached is shown, not waited on.
7. **A remote profile skips more.** Contacts and entries go to the hub, so the first-entry step is
   skipped too.
8. **"Try it" must not write a made-up fact.** The suggested question is about the person's real work,
   and the step waits for a rise in the store's counts instead of a baseline nobody recorded.
9. **O1 and O2 overclaimed.** Setup changes more paths than the three named, and apps registered
   through their own CLI make no backup. O1 is now about the log, and O2 names the copies where they
   exist.
10. **Linux already has a unit to copy, and lingering.** Setup must not overwrite a hand-copied unit,
    has to ask for lingering, and the lifecycle has to recognize the unit; Linux is its own step.
11. **Restarting Claude Desktop** is specified through the running-application interface first, with the
    Automation permission and its entitlement named if that is what it takes.
12. **Smaller gaps:** `--events` names its schema, `--yes` never implies `--replace`, `wait` has a
    timeout and an exit status, `skipped` is defined, the install script reads its answer from
    `/dev/tty`, and an app whose CLI predates `setup` says to upgrade it.
