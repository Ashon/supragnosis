# Onboarding - from install to the first entry, inside the app (Principles 3, 17, 19, 24)

> What a person does between installing supragnosis and seeing the first entry their own work made,
> and how the desktop app walks them through it without a terminal. One procedure, `supragnosis
> setup`, decides what is done and does what is not; the app's Welcome page draws it and shows, step
> by step, what it ran. Companion to [client-connect.md](client-connect.md) (connecting AI apps),
> [daemon-lifecycle.md](daemon-lifecycle.md) (who runs the daemon),
> [settings-page.md](settings-page.md) (the app's own pages) and
> [semantic-retrieval.md](semantic-retrieval.md) (the search model).
>
> Status: **specified, not built** (Section 13). The choices it records were made by the author on
> 2026-10-18 from a storyboard.

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
- **Linux has no always-on daemon**: the install script prints a `serve` command and a link.

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
  3.1), the steps about this Mac's daemon and model say so and stand down; connecting apps still
  applies, since the bridge follows the profile.

## 3. One procedure, two faces

**`supragnosis setup`** knows the four steps of Section 4. For each it reads the live state, decides
whether the step is done, and if not, does it. It keeps no record of its own progress: the checklist
is computed from `status` and `connect` every time (O3), so running it again shows only what is left
and does only that.

- `supragnosis setup` - prints the plan, asks "Proceed? [Y/n, c to choose]", applies it and says what
  each step did. The text face, for a terminal and for Linux, where there is no app.
- `supragnosis setup --yes` - applies the plan with its defaults, asking nothing.
- `supragnosis setup --check` - only reads, and prints the checklist (like `connect` with no app,
  client-connect.md C5).
- `supragnosis setup --step <step>` - applies one step: `server`, `app=<id>`, `model`, or `wait` (wait
  for a first contact or a first entry, with a timeout).
- `--events` - with any of the above, writes one JSON object per line instead of text: the step, its
  state (`todo`, `working`, `done`, `attention`, `failed`, `skipped`), one sentence saying what
  happened, and where a command ran, the command as text, each line of its output, and every file it
  changed with the copy it made first. A last object summarizes.

**The app decides nothing itself** (O4). The Welcome page draws its cards from `setup --check
--events`, and each button runs `setup --step ... --events`; the log drawer under a card is that
step's events. The app and the CLI therefore cannot disagree about what is set up.

Failures follow the lifecycle's rule (daemon-lifecycle.md L4): a step that fails exits non-zero and
says why and what to do, in its events and in its text (O2).

## 4. The steps

### 4.1 Keep it running

**State:** who manages the daemon, whether it answers, and whether it starts at login (`status
--json`). **Done** when the canonical login job owns a daemon that answers.

**Act:** `service install` with the take-over the Start at Login switch already uses
(daemon-lifecycle.md Section 4, L1, L6): an app-spawned child is taken over; Homebrew's service or a
manager the lifecycle cannot name is reported as `attention`, with what owns it and the command that
would take over, and setup does not take it over without being asked (O8). Setup asks
`service install` not to fetch the model (`--no-model`): the model is step 4.3, and the daemon does
not wait for it.

**Default:** on (D2). The Welcome page shows the Start at Login switch already on, and turning it off
is the Settings page's act (settings-page.md Section 3.3).

**On Linux** `service install` writes a systemd user unit (`~/.config/systemd/user/supragnosis.service`)
and enables it with `systemctl --user`. It keeps the lifecycle's rules: the unit is generated and an
operator's own file is never overwritten (L5), it binds and authenticates exactly as the daemon does
by default (L7), and it needs no administrator rights (L8). Where `systemctl --user` is unavailable,
as in many containers, the step is `attention` and names the `serve` command.

### 4.2 Connect your AI apps

**State:** each app's row from `connect --json` (installed, connected, capture), and the first
contacts of Section 5. **Done** when at least one connected app has reached the daemon.

**Act:** `connect <id>` for each app the person chooses (client-connect.md Section 4), which backs up
the app's file first (C3) and, for Claude Code, registers the capture hook unless the person turns
it off (client-connect.md Section 12.4). The default choice is every installed app.

After connecting, an app still has to load the change, and setup says how for each one, in the words
`connect` prints today. Two of them the app can do for the person (D9):

- **Claude Desktop:** "Restart Claude Desktop" asks Claude Desktop to quit through Apple Events, waits
  for it to exit and opens it again. macOS asks the person once to allow the app to control Claude
  Desktop; a refusal leaves the instruction in place.
- **Cursor:** "Open Cursor" opens it, with where to turn the server on.
- **Claude Code, Codex, Gemini and VS Code** load it in their next session or chat, which only the
  person can start.

A connected app becomes **reached** when its first contact arrives (Section 5). Until then its row
says it is waiting, and why that is normal. `connect` itself warns, in its own output, when no daemon
answers: the entry is written, and the app will fail until a daemon runs.

### 4.3 Search by meaning

**State:** the search mode `status` reports (semantic on, or keyword only and why). **Done** when the
model is present.

**Act:** `embed fetch --events`, which reports progress as the bytes the model's directory holds
against the model's size, about twice a second, and ends by checking the files. The daemon loads the
model on its next search, without a restart (semantic-retrieval.md Section 6).

**Default:** the app starts the download when the Welcome page opens, in the background (D3); "Not
now" stops it, and the model step stays `todo`. Nothing waits for it (O5): the other steps and the
first entry go on, and every search answer says it is keyword only until the model is there. In the
terminal, `setup` runs it last, with a progress line; interrupting it leaves keyword search, and the
next run fetches what is missing.

### 4.4 Try it

**State:** whether an entry has arrived since setup began. **Done** when one has.

**Act:** none on the store (O1). The step shows a sentence to ask in a connected app ("Remember that
billing deploys on Fridays, and why."), with a copy button, and waits: the app listens to the viewer's
event stream for the first observation (remote-viewer.md Section 3.6 describes the stream), and
`setup --step wait` polls for one. When it arrives the step shows what it said and in which
workspace, and the app offers "Open the graph" at that workspace.

## 5. First contact

An AI app announces itself when it starts an MCP session: the `initialize` request carries the
client's name and version. The daemon records, for each name, the version and the first and last
time it arrived, in memory only.

- **Where it is seen:** on the viewer's local socket at `/api/clients`, and in `status --json` as
  `clients`. Not on the read tier: `/api/clients` is refused there (remote-viewer.md Section 3.2), and
  nothing about it reaches a peer (O7).
- **Through the bridge** the app's own `initialize` reaches the daemon unchanged (client-connect.md
  C4), so the name is the app's, not the bridge's.
- **Which app it is:** setup maps the names it knows to the apps `connect` lists, in a table kept
  beside the client table of client-connect.md Section 4, and shows a name it does not know as sent.
- **What it is not:** a record of what an app did. It says an app reached the daemon since the daemon
  started. A daemon restart forgets it, and the app keeps what it saw for its own checklist.

## 6. The app: Welcome

**A page in the main window**, `assets/welcome.html`, under the same title bar with the crumb
`supragnosis / welcome` (settings-page.md Section 3.0): no new window, no navigation in the title bar.

**When it shows.** On the first launch, and on any launch while step 4.1 or 4.2 is not done, until the
person closes it with "Finish later". While either is not done, the tray offers "Finish setup...",
and the Settings page's About section offers "Run setup again" at any time
(settings-page.md Section 3.5).

**What it shows.** Four cards, one per step, each with its state, its sentence, and its act: the
Start at Login switch, a row per AI app with Connect and its helper, the model's progress with "Not
now", and the sentence to try with the first entry when it comes. Under each card a **log drawer**
holds that step's events as they arrive: the command, its output, every file changed and the copy
made first, and the exit status (D8). "Copy log" copies every drawer; "Copy diagnostics" copies `setup
--check --json` and `status --json` together, for a report.

**Why a log drawer and not a terminal.** A terminal shows the same output, but as text the page
cannot read, so the cards could not follow it; it puts a shell inside the app and invites typing into
it; and it adds a pseudo-terminal and a terminal widget to the app. The drawer shows what was run and
what it changed, and the cards stay the truth about state.

**The privacy line** at the foot: nothing leaves this Mac, and sync stays off until a server is added
in Settings (O6).

**Settings** gains the search state in This Mac's daemon section (settings-page.md Section 3.3):
"Search by meaning" or "Keyword only - the model is not downloaded", with the download.

## 7. Words where people land

The places a person meets without the Welcome page say what to do next:

| Where | Today | After |
|---|---|---|
| The viewer, an empty workspace | no nodes in this workspace - observe knowledge, or pick another workspace | Nothing here yet. Ask a connected AI app to remember something, or pick a workspace that holds knowledge, each named with its count. |
| `status`, daemon stopped | stopped | stopped, then how to start it now, how to keep it running, and `supragnosis setup` for a first run |
| `connect`, no daemon | (no check) | the connection is made, and the app will fail until a daemon runs: `supragnosis setup` |
| The `brief` prompt, an empty workspace | counts of zero | says the workspace holds nothing yet, and that recording decisions as work happens is what fills it (prompts.md Section 3) |
| The app, CLI not found | points at the install script | names the Homebrew formula for a cask install, and the install script otherwise |

## 8. The installers and the landing

- **The landing's install command** becomes `brew install --cask ashon/tap/supragnosis && open -a
  Supragnosis` (design-system.md Section 11), so one line ends with the Welcome page open.
- **The cask's caveats** say to open the app to finish setting up.
- **The install script** ends with "Next: supragnosis setup" instead of its own list, and when it runs
  in a terminal it asks whether to run it now.

## 9. Decisions

- **D1. First contact, not a test entry.** A test entry would prove the pipe and stay in the log
  forever (P3). The `initialize` an app sends anyway proves the same pipe and costs nothing.
- **D2. Start at Login on by default in Welcome.** An app that is connected but finds no daemon the
  next morning is the failure the product most needs to avoid; the switch is visible and one click.
- **D3. The model downloads by itself, in the background.** It is what makes search work across
  languages and phrasing, and it blocks nothing; "Not now" is one click.
- **D4. The app leads; the CLI is the same procedure.** The app is where a non-developer is; the CLI
  is where a developer and Linux are. One decision procedure keeps them the same.
- **D5. Linux gets a user unit.** The only always-on path today is a hand-run command.
- **D6. A "Try it" step.** The first useful moment is an entry, not a green check.
- **D7. Homebrew stays the installer for now.** One line installs and opens the app; carrying the
  server inside the app is Section 10.
- **D8. A log drawer, not a terminal** (Section 6).
- **D9. The app restarts Claude Desktop for the person,** with macOS's permission; other apps are told
  how, or opened.

## 10. Later: an app with the server inside - recorded, not decided

Removing the last terminal line means downloading one app from the website. That needs the server
inside the app (a sidecar binary, about 52 MB), an update the app can install itself (signed, as
the release already is), and a rule for which binary runs when Homebrew's is also present.
client-connect.md Section 7 records the same trade-offs. It changes the release pipeline and the
lifecycle's idea of who owns the daemon, so it gets its own document when it is taken up.

## 11. Invariants

| | Invariant |
|---|---|
| **O1** | Setup writes nothing to the knowledge store: its checks read, its acts change the daemon's manager, the apps' configuration and the model cache only, and its first-entry step waits for an entry the person's own work makes. |
| **O2** | Every step reports its outcome where it ran - what it did, every file it changed with the copy made first, or why it failed and what to do - and a failed step exits non-zero (P24). |
| **O3** | The checklist is computed from the live state each time it is shown; `setup --check` only reads, and running setup again changes only what is not done. |
| **O4** | The app's Welcome page decides nothing itself: every state it draws and every change it makes comes from `setup`'s events, so the app and the CLI cannot disagree. |
| **O5** | The model never blocks: the server, the apps and the first entry proceed while it downloads, search says keyword only meanwhile, and stopping the download leaves nothing broken. |
| **O6** | Nothing leaves the machine during setup: it never configures sync or a remote server, and the page says so. |
| **O7** | First contact is the app's own `initialize`, kept in the daemon's memory: never written to the store, never served on the read tier, never sent to a peer. |
| **O8** | Setup takes over a running manager only as the Start at Login switch does, and reports any other manager instead (daemon-lifecycle.md L1, L6); a Linux user unit adds no exposure and needs no privilege (L7, L8). |

## 12. What this revises

- **client-connect.md Section 4**: `connect` warns when no daemon answers.
- **daemon-lifecycle.md Section 4**: `service install` takes `--no-model`, and on Linux writes and
  enables a systemd user unit.
- **semantic-retrieval.md Section 6**: `embed fetch` takes `--events` and reports progress.
- **settings-page.md Section 3.3**: This Mac's daemon section shows the search state and offers the
  download.
- **settings-page.md Section 3.5**: About offers "Run setup again".
- **settings-page.md Section 5**: the tray offers "Finish setup..." while the daemon or the apps are not
  set up.
- **remote-viewer.md Section 3.2**: `/api/clients` is refused on the read tier.
- **prompts.md Section 3**: `brief` says what an empty workspace means.
- **design-system.md Section 11**: the landing's install command opens the app.

## 13. Ordering

1. **This document** and its registry rows.
2. **The daemon and the CLI**: first contact (`/api/clients`, `status --json`), `setup` with
   `--check`, `--step`, `--yes` and `--events`, `embed fetch --events`, `service install --no-model`,
   the Linux user unit, `connect`'s warning, `status`'s stopped wording, `brief`'s empty wording and
   the install script's ending. It ships in a release, since the app reads it.
3. **The app**: the Welcome page and its log drawers, the two helpers, "Finish setup..." in the tray,
   the search row and "Run setup again" in Settings, the viewer's empty state, the CLI-not-found
   message, the cask's caveats and the landing's install command.
4. **A clean run**: both install paths from an empty home directory to the first entry. The app's run
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
| client-connect.md C3, C4, C5 - backups, an unchanged surface, a read-only listing | Sections 3, 4.2, 5 |
