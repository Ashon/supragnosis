# Connecting AI apps - the bridge and `supragnosis connect`

> How an MCP client reaches the daemon, and how a person who has never opened a terminal connects
> one. Companion to [daemon-lifecycle.md](daemon-lifecycle.md) (who runs the daemon) and
> [architecture.md](architecture.md) Section 10 (the local surfaces).
>
> Status: **built** (Section 9 steps 1-4). What building it changed is recorded in Section 11.
> Section 12, capture, is **specified** and not built (Section 9 step 5).

## 1. Why this exists

Connecting a client today means typing this into a terminal:

```sh
claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp \
  --header "Authorization: Bearer $(cat ~/.supragnosis/mcp.token)"
```

The README, the curl installer and the landing page all print a command like that. A person who
does not use a terminal cannot connect anything, and the desktop app - the part built for that
person - has no way to help: it runs the daemon and shows the viewer, but connecting a client is not
something it can do.

The command is also not portable. What each client can reach, checked in October 2026:

| Client | Local stdio server | HTTP with a bearer header |
|---|---|---|
| Claude Code | yes | yes (`claude mcp add --header`) |
| Claude Desktop | yes (config file, or an `.mcpb` bundle) | **no** - the config file takes stdio only, and the Connectors UI is OAuth-only |
| Cursor | yes | yes (`mcp.json` `url` + `headers`) |
| VS Code | yes | yes (`mcp.json` `type: http` + `headers`) |
| Codex CLI | yes | only from an environment variable (`--bearer-token-env-var`) |
| Gemini CLI | yes | yes (`--header`) |

The client a non-developer is most likely to use, Claude Desktop, cannot reach the daemon at all
without a third-party bridge such as `mcp-remote`, which needs Node. Every client speaks stdio, but
the stdio server supragnosis ships cannot be offered while the daemon runs. Run with no arguments,
`supragnosis` opens the store itself. The store admits one writer, so beside the daemon - which
the app and Start at Login keep running - that server fails on the lock. If it starts first, the
daemon is the one that fails (daemon-lifecycle.md Section 11).

The token also leaves the 0700 directory. The shell expands `$(cat ...)` before the client sees
it, so the client stores the token itself: on the author's machine `~/.claude.json` holds it in
plain text. Every connected client becomes another copy of the secret, with that client's file
permissions, sync and backup habits, and nothing tells the operator where the copies are.

## 2. What this is NOT

- **Not a second MCP surface.** The bridge relays messages. It has no tools of its own, and what a
  client sees through it is the daemon's surface unchanged (Principle 21: one narrow surface, not
  two that drift).
- **Not a second owner.** The bridge never opens the store and never starts a daemon. Starting the
  daemon belongs to the lifecycle (daemon-lifecycle.md L1): the app, Start at Login, or
  `supragnosis service install`. When nothing answers, the bridge says what to do.
- **Not remote access.** The bridge reaches only the local daemon, on loopback (Principle 17).
  **Revised by [remote-server.md](remote-server.md):** the bridge reaches the server its active
  profile names, and the local daemon is the default profile. C1 is unchanged.
- **Not packaging.** Installing without Homebrew is Section 7, a separate decision.

## 3. The bridge: `supragnosis bridge`

A client launches `supragnosis bridge` as its stdio server. The bridge relays every message the
client sends to the local daemon's streamable-HTTP endpoint, and every reply back. It does not
read or rewrite what passes through.

- **The token is read from its file, by the bridge, on every connect** - the local daemon's, or the
  active profile's credential (remote-server.md Section 3). No client configuration holds a secret,
  and regenerating the token needs no change in any client.
- **The address** of the local profile is `SUPRAGNOSIS_HTTP_ADDR` or `127.0.0.1:7373`, refused unless
  it is loopback - the same check `serve` applies. A remote profile is reached over verified TLS, or
  plain HTTP to a loopback host only (remote-server.md R8).
- **A daemon restart is invisible to the client.** The daemon answers 404 for a session it no longer
  knows, which is already how it tells a client to re-initialize. The bridge keeps the client's own
  `initialize` request, replays it to open a new session, and retries the request once. An upgrade
  followed by `supragnosis restart` then does not require restarting Claude Desktop.
- **When nothing answers.** The bridge waits a few seconds, since a login job may still be starting.
  If the daemon is still down, it answers the client's request with an error naming the fix: open
  Supragnosis, or turn on Start at Login. It does not exit, so the next request tries again.
- **It ends when stdin closes**, which is how a client stops a stdio server.

## 4. `supragnosis connect`

One command registers the bridge with a client, using whatever mechanism that client provides.

- **`supragnosis connect`** (with `--json` for the app) lists the known clients. For each it says
  whether the client is installed and whether a `supragnosis` entry exists, and of which kind:
  `bridge`, `http` (a token copied into the client), `stdio` (the store-opening server), or `other`.
  The listing only reads.
- **`supragnosis connect <client>`** registers `<supragnosis> bridge` with that client. The program
  path is the stable one: the Homebrew `opt` link rather than the versioned keg, chosen the same way
  as for the login job (daemon-lifecycle.md Section 4), so an upgrade does not break it.
  - **Through the client's own CLI** when it has one - Claude Code, Codex, Gemini, and VS Code's
    `code --add-mcp`. The client owns its file format, and a CLI is the interface it promises to
    keep.
  - **By editing the client's config file** only where no CLI exists: Claude Desktop and Cursor. One
    key is added under `mcpServers` and everything else stays as written, in its original order. The
    file is first copied to `~/.supragnosis/connect/<client>.<timestamp>.json`. A file that does not
    parse as JSON is refused rather than rewritten.
- **An entry that is already there.** One that already names the bridge needs nothing. Any other
  `supragnosis` entry is reported and left alone, unless `--replace` is given. `--replace` removes
  the old entry the client's own way first. For an `http` entry, the output says that the token
  copy in that client is now gone. This is the take-over rule of `service install` applied to
  another program's configuration.
- **`supragnosis connect --remove <client>`** removes the `supragnosis` entry, by the same route and
  with the same backup.
- **After connecting**, the command says what the person has to do next: restart Claude Desktop, or
  start a new Claude Code session.

| Client | Detected by | Registered through | Lands in |
|---|---|---|---|
| `claude-desktop` | `/Applications/Claude.app` | file edit | `~/Library/Application Support/Claude/claude_desktop_config.json`, `mcpServers` |
| `claude-code` | `claude` on `PATH` or in `~/.local/bin` | `claude mcp add --scope user` | `~/.claude.json`, `mcpServers` |
| `cursor` | `/Applications/Cursor.app` or `~/.cursor` | file edit | `~/.cursor/mcp.json`, `mcpServers` |
| `vscode` | the `code` CLI inside the app bundle | `code --add-mcp` (file edit to remove) | the user profile's `mcp.json`, `servers` |
| `codex` | `codex` on `PATH` | `codex mcp add` | `~/.codex/config.toml`, `mcp_servers` |
| `gemini` | `gemini` on `PATH` | `gemini mcp add --scope user` | `~/.gemini/settings.json`, `mcpServers` |

## 5. The app: AI Apps in the tray

> **Revised ([settings-page.md](settings-page.md)).** The AI Apps submenu became the AI apps section
> of the app's settings page: one row per app with its state, and one button that says what a click
> will do. The rules below carry over - the click is the consent, the next step is shown beside the
> app, and the status line still says when no app is connected.

The tray gains an **AI Apps** submenu built from `connect --json`:

- A connected client shows a check, a client that is not installed is disabled, and the rest are
  plain items.
- Clicking an unchecked client runs `connect <client> --replace`. As with Start at Login, the click
  is the consent, and the status line then says what was replaced.
- Clicking a checked client runs `connect --remove <client>`.
- The note line reports the outcome and the next step ("restart Claude to load it").
- While no client is connected, the status line says so, because that is the state a new user is
  in.

The app decides nothing itself. It calls the CLI it already finds and shows the answer, the same
rule as daemon-lifecycle.md Section 6, so the logic lives once, in the code that is tested.

## 6. Decisions

- **A bridge, not a token handed out.** The bridge is the only option that reaches every client in
  Section 1's table, keeps the secret in one file, and survives daemon restarts. Writing the token
  into each client's config would have been less code, at the cost of the copies Section 1
  describes.
- **A relay, not a reimplementation.** Forwarding messages means a tool added to the daemon reaches
  every client with no change here. A bridge that re-declared the tools would be a second surface to
  keep in step.
- **Client CLIs before file edits.** A client's CLI is what it commits to keep working across its
  own upgrades. Its file format is not. Edits are kept to the two clients that offer nothing else.
- **The store-opening stdio server stays, demoted.** It is still the right shape where no daemon
  runs - a CI job, a one-off container. But documents, installers and `connect` stop recommending
  it, because beside a daemon it is the second writer.
- **The user scope where a client has scopes.** A person connecting "Claude" means everywhere, not
  the one project directory a terminal happened to be in. Claude Code's default scope is the local
  project; `connect` passes `--scope user`.

## 7. Installing without Homebrew - recorded, not decided here

The app finds the CLI on the Homebrew path or on `PATH`, and ships without one of its own
(deploy/homebrew/README.md: "no sidecar"). A person who downloads only the app zip therefore has no
daemon, no bridge and no `connect`. The non-developer path starts with `brew install` until that
changes.

Bundling the CLI inside the app (a Tauri sidecar) would make the app self-sufficient, but it
reverses a recorded decision and brings its own problems:
- A Homebrew install would then have two copies of the binary, and it must be clear which one runs
  the daemon.
- The bundled CLI and the formula could be different versions.
- The app grows by the size of a second copy of the binary.

That is a release-pipeline decision with its own trade-offs, so it is left out of this document's
steps.

## 8. Invariants

| | Invariant |
|---|---|
| **C1** | The bridge never opens the store and never starts a daemon. |
| **C2** | `connect` never writes a secret into a client's configuration. The bridge reads the active profile's credential from its 0600 file. |
| **C3** | `connect` never replaces an entry it did not write unless `--replace` is given, and never edits a client file without first copying it aside. A file it cannot parse is refused, not rewritten. |
| **C4** | Through the bridge a client sees the daemon's surface unchanged: the same tool list, the same results, the same errors. |
| **C5** | `connect` without a client argument only reads. |
| **C6** | The capture hook never blocks the agent, never writes to the store itself, and speaks only after a commit that landed or at the start of a session. |

## 9. Ordering

1. **The bridge.**
2. **`connect`**: listing, registration and removal for the six clients in Section 4's table.
3. **The app's AI Apps submenu.**
4. **Documents.** README, the curl installer and the landing page show `supragnosis connect` and the
   app, rather than the `--header` command or the store-opening stdio line.
5. **Capture** (Section 12): `supragnosis hook claude-code`, its registration by `connect`, the
   listing's `capture` field, the P22 registry row, and the README's first minute.

## 10. Closure map

| Principle | Where this closes it |
|---|---|
| P17 - local surfaces stay local, the secret stays in one place | Section 3 (the local profile on loopback only, a remote one over verified TLS, the credential read from its file); C2 |
| P21 - one narrow surface | Section 2 and the relay decision; C4 |
| Outside principles.md's scope (client configuration) - P24's "the operator's file is theirs", followed by analogy | Section 4 (backup, merge, refuse what does not parse, `--replace` to take over); C3 |
| Outside principles.md's scope - P5's refusal to collapse states, followed by analogy | Section 4's listing distinguishes not installed, not connected, and connected some other way |
| daemon-lifecycle.md L1 - one owner | C1: the bridge is never a second writer |
| P22 - knowledge as a by-product of work | Section 12: a commit asks the agent for the decision behind it, and a session starts knowing what the project's workspace holds; C6 |

Guarded by:

- `the_bridge_relays_the_daemons_surface_unchanged` - C4. The tool list, and a tool call that lands
  in the daemon's store.
- `a_daemon_restart_is_invisible_through_the_bridge` - the replayed handshake.
- `the_bridge_says_what_to_do_when_no_daemon_answers` and
  `a_refused_token_is_reported_with_its_file` - errors a person can act on.
- `registrations_run_the_bridge_and_carry_no_secret` - C2.
- `an_edit_changes_one_member_and_nothing_else`, `a_file_that_does_not_parse_is_refused` and
  `a_backup_never_replaces_an_earlier_one` - C3.
- `an_app_item_says_what_a_click_will_do` - the settings page's app rows (the tray's AI Apps items
  until settings-page.md).

Checked end to end:
- **Bridge, against the live daemon**, with reads only: initialize, tools/list (13 tools),
  search_knowledge, then a clean exit when stdin closed.
- **`connect` under a temporary HOME:**
  - Claude Desktop by file edit: add, then "already connected", then remove back to the original
    bytes.
  - Codex, Gemini and Claude Code through their real CLIs, each read back as the bridge.
  - An HTTP entry with a fake token: refused without `--replace`, and with it replaced and the
    token gone.
- **The tray**, in a development build on this machine: the AI Apps submenu showed Claude Code as
  "connected over HTTP - click to switch" and Cursor as not installed. No item was clicked.
- **Not run live**: VS Code's `code --add-mcp`, which would have changed this machine's real
  profile. Its arguments are covered by the unit test.

## 11. What building it changed

- **The bridge opens a new connection for every request.** A pooled connection outlives a daemon
  restart. A request sent on it then fails in a way that cannot tell "never delivered" apart from
  "delivered, then the daemon died", and retrying the second would run a write twice. A refused
  connection is unambiguous, so that is the only failure the bridge retries. On loopback a fresh
  connection costs nothing worth saving.
- **Backups never overwrite each other.** The first version named a backup by the second it was
  taken. `connect` followed by `--remove` within one second then replaced the copy of the original
  with a copy of the first edit. Names now count up instead.
- **VS Code keeps its settings in a different place on Linux** (`~/.config/Code/User`). The table in
  Section 4 gives the macOS path.

## 12. Capture - the decision behind a commit

> Status: **specified**, not built. Principle 22's first clause is deferred in the registry with the
> reason that nothing makes an agent capture or retrieve unasked. This section is the mechanism that
> repays it. What building it changes goes into Section 11.

### 12.1 Why

Principle 22 says knowledge is a by-product of work. Today that holds only as far as the prompt
does: the tool descriptions and the server instructions tell an agent what `observe` is for, and
whether it calls `observe` during a task depends on the agent and on what the person wrote in their
instructions. The README's first minute begins with the person saying "remember that ...". A memory
that has to be asked for is a memory that stays mostly empty, and the registry says so: P22's first
clause, "ordinary work induces capture and retrieval without a separate curation chore", is
`Deferred`.

Work has one moment where a decision has just been made and the reasoning is still in the context
window: the commit. The diff records what changed. The message records some of why. What is in
neither - what was ruled out, what the change depends on, what the person said in the conversation
that led to it - is the knowledge this project exists to keep, and it is gone when the session ends.
The same session, at its start, does not know whether the project it is in has any memory here at
all: the server instructions are generic, and nothing says "this workspace holds 168 entities".

### 12.2 What it is

One CLI command, `supragnosis hook claude-code`, that Claude Code runs as a hook on two events. It
reads the event's JSON from stdin and answers on stdout with `hookSpecificOutput.additionalContext`,
the channel that adds a line to the agent's context without refusing anything. It never answers
with `decision`.

**After a commit** (`PostToolUse`, tool `Bash`). The hook answers only when all three hold:
`tool_name` is `Bash`; the command names `git commit`; and the repository at `cwd` has a HEAD whose
committer time is not more than a minute before the hook started. A commit that failed (nothing
staged, a rejected message), `git commit --help`, and a HEAD another session made minutes ago all
leave that false, and the hook says nothing. The answer names the commit and asks the question once:

> Commit 9f41c2e "the bridge opens a new connection for every request" landed in supragnosis
> (author ashon). If it settled a decision, a constraint or a fact about the project that the diff
> does not show - why it was done this way, what was ruled out, what it depends on - record it now
> with `observe`: workspace "supragnosis", on_behalf_of "ashon", source_ref "git:9f41c2e...". If it
> settled nothing, record nothing.

**At the start of a session** (`SessionStart`, every source: startup, resume, clear, compact). The
hook asks the local daemon over the viewer socket, with the 800 ms budget `status` already uses,
whether the repository's workspace holds anything, and says so in one line:

> supragnosis: workspace "supragnosis" holds 168 entities and 274 relations on this node. Search it
> (`search_knowledge`, `workspace_map`) before assuming what this project has decided; `brief` reads
> it as a whole.

When the workspace is not known here the line says that the first `observe` will create it. When no
daemon answers, the hook says nothing: the bridge already says what to do when a tool is called,
and a session start is not the place to repeat it.

The hook never writes to the store. The agent does, through the bridge, with its own judgment and
under the tool's own rules (C4): the hook names the moment and the facts of the commit, and the
agent decides whether there is knowledge in it. It exits 0 whenever it has nothing to say. Input it
cannot parse, or a `git` it cannot run, goes to stderr with exit 1, which Claude Code shows and
carries on from: Principle 24's degrade loudly, without refusing the agent's work.

### 12.3 The workspace

The hook names the workspace after the repository's top-level directory
(`git rev-parse --show-toplevel`), because the project is the unit knowledge is organized by (the
server instructions say so) and a repository is how a project appears on disk.
`SUPRAGNOSIS_WORKSPACE` in the hook's environment overrides it. The name is a suggestion in both
lines: the agent may know better (a monorepo, a fork, a workspace the person named differently), and
the text says "workspace", not "only this workspace".

### 12.4 Registration

- **`supragnosis connect claude-code` registers the hook beside the bridge**, in
  `~/.claude/settings.json`: one `PostToolUse` entry with matcher `Bash` and one `SessionStart`
  entry, each with the single command `<program> hook claude-code`, `<program>` chosen as the bridge's
  is. `--no-capture` registers the bridge alone. `connect --remove claude-code` removes both. The
  listing (`connect`, `connect --json`) reports `capture: on | off` beside the entry kind - an added
  field, so the JSON schema stays at its number - and the app's row says it.
- **The edit follows C3.** The file is copied aside first; only an entry whose command names
  `supragnosis` and `hook` is added or removed; every other hook and setting stays as written, in its
  order. A file that does not parse is refused. A file that does not exist is created holding only
  the two entries.
- **On by default** because the person asked for Claude Code to be connected to their memory: the
  bridge gives the agent the tools, the hook gives it the moment. A flag nobody reads is a default
  for nobody. The user scope, for the reason `connect` passes `--scope user`: "Claude" means
  everywhere.
- **It takes effect in the next session.** Claude Code takes its hooks at startup and asks before
  running one a settings file gained since, so the next-step line stays "start a new Claude Code
  session".
- **Claude Code only, for now.** Codex, Gemini CLI, Cursor and VS Code each have hooks of their own
  shape or none; the listing shows nothing about capture for them. Adding one is a row in the table
  of Section 4 and a reader for that client's event, not a second mechanism.

### 12.5 What this is NOT

- **Not an extractor.** A hook that read the diff and wrote observations itself would be a second
  extractor with no judgment and no provenance beyond "a script" (Principle 19: the probabilistic
  edge drafts, and here the agent is the edge). The hook puts a question in front of the agent; the
  agent writes an observation, or nothing.
- **Not a `Stop` hook.** A nudge at the end of every turn is noise, and a blocked `Stop` interrupts
  the person. A commit happens once and is the moment; a session starts once.
- **Not a change to the surface.** No tool, resource or prompt is added (Principle 21). The daemon
  does not know the hook exists.
- **Not a transcript reader.** The hook decides from the repository and the daemon, never from the
  session's transcript file, which is written asynchronously and is the person's.

### 12.6 What the tests assert

The tests are named when they exist (the registry refuses a design document that names a test that
does not run); what each asserts is fixed here.

- **A commit that landed is answered.** A repository with a fresh commit and a `PostToolUse` input
  naming `git commit`: the answer carries the id, the subject, the author and the workspace, and no
  `decision`.
- **A command that landed no commit is not.** A command that is not `git commit`; a `git commit`
  with nothing staged; a HEAD older than the budget: no output, exit 0.
- **A session start says what the workspace holds.** Against a daemon serving a seeded store: the
  counts; against a workspace it has never seen: the line says the first `observe` creates it; with
  no socket: no output, exit 0.
- **`connect` registers the hook beside the bridge and removes both.** A `settings.json` with other
  hooks keeps them byte for byte around the two entries; `--remove` restores it; `--no-capture`
  leaves it untouched; a file that does not parse is refused (C3).
- C6 is registered `Deferred` until then, and the registry's P22 first clause moves from
  `Deferred` to `Scenario` naming the first and third tests, reworded to what is then true: "a
  commit the agent made asks it for the decision behind the change, and a session starts knowing
  what the workspace holds, with the person asking for neither".
