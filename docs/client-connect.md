# Connecting AI apps - the bridge and `supragnosis connect`

> How an MCP client reaches the daemon, and how a person who has never opened a terminal connects
> one. Companion to [daemon-lifecycle.md](daemon-lifecycle.md) (who runs the daemon) and
> [architecture.md](architecture.md) Section 10 (the local surfaces).
>
> Status: **built** (Section 9 steps 1-5, Section 12 included). What building it changed is recorded in Section 11.
> Section 13, OpenCode, is built.

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
| OpenCode | yes | yes (`opencode mcp add --url --header`; added October 2026, Section 13) |

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
  **Revised by [remote-server.md](remote-server.md),** which let a profile name a server elsewhere,
  and back in 0.11.0, which removed profiles (its Section 13): the bridge reaches only the local
  daemon. C1 is unchanged.
- **Not packaging.** Installing without Homebrew is Section 7, a separate decision.

## 3. The bridge: `supragnosis bridge`

A client launches `supragnosis bridge` as its stdio server. The bridge relays every message the
client sends to the local daemon's streamable-HTTP endpoint, and every reply back. It does not
read or rewrite what passes through.

- **The token is read from its file, by the bridge, on every connect** - the local daemon's. No
  client configuration holds a secret,
  and regenerating the token needs no change in any client.
- **The address** is `SUPRAGNOSIS_HTTP_ADDR` or `127.0.0.1:7373`, refused unless it is loopback -
  the same check `serve` applies.
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
  - **Through the client's own CLI** when it has one - Claude Code, Codex, Gemini, OpenCode's
    `opencode mcp add`, and VS Code's `code --add-mcp`. The client owns its file format, and a CLI
    is the interface it promises to keep.
  - **By editing the client's config file** only where no CLI does the act: Claude Desktop and
    Cursor, which have no CLI for it; the removal for VS Code and OpenCode, whose CLIs add but do
    not remove; and Claude Code's capture hook, which its CLI has no command for (Section 12.4). One
    key is added or removed and everything else stays as written, in its original order. The file is
    first copied into `~/.supragnosis/connect/`, named for the client and the time and keeping its
    extension (Section 11 has the exact names). A file that does not parse - as JSON, or as JSONC
    for OpenCode (Section 13) - is refused rather than rewritten. VS Code reads its file as JSONC
    too, but `connect` holds it to strict JSON: a commented one is refused, never edited.
- **An entry that is already there.** One that already names the bridge needs nothing. Any other
  `supragnosis` entry is reported and left alone, unless `--replace` is given. `--replace` removes
  the old entry the client's own way first. For an `http` entry, the output says that the token
  copy in that client is now gone. This is the take-over rule of `service install` applied to
  another program's configuration.
- **`supragnosis connect --remove <client>`** removes the `supragnosis` entry, by the same route and
  with the same backup.
- **After connecting**, the command says what the person has to do next: restart Claude Desktop, or
  start a new Claude Code session. It names what it changed: the file it wrote, or the client
  command it ran. When the local daemon does not answer, it says so: the entry is written, and the
  app fails until a daemon runs (onboarding.md Section 4.2).
- **Claude Code also gets the capture hook** (Section 12) unless `--no-capture` is given, and the
  listing says `capture on` or `off` beside its entry.

| Client | Detected by | Registered through | Lands in |
|---|---|---|---|
| `claude-desktop` | `/Applications/Claude.app` | file edit | `~/Library/Application Support/Claude/claude_desktop_config.json`, `mcpServers` |
| `claude-code` | `claude` on `PATH` or in `~/.local/bin` | `claude mcp add --scope user` | `~/.claude.json`, `mcpServers` |
| `cursor` | `/Applications/Cursor.app` or `~/.cursor` | file edit | `~/.cursor/mcp.json`, `mcpServers` |
| `vscode` | the `code` CLI inside the app bundle | `code --add-mcp` (file edit to remove) | the user profile's `mcp.json`, `servers` |
| `codex` | `codex` on `PATH` | `codex mcp add` | `~/.codex/config.toml`, `mcp_servers` |
| `gemini` | `gemini` on `PATH` | `gemini mcp add --scope user` | `~/.gemini/settings.json`, `mcpServers` |
| `opencode` | `opencode` on `PATH` or in `~/.opencode/bin` | `opencode mcp add --global` (file edit to remove) | `opencode.json` and `opencode.jsonc` in `$XDG_CONFIG_HOME/opencode/` (`~/.config/opencode/`), `mcp.servers` (Section 13) |

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

**The CLI runs with the person's `PATH`, not the app's.** An app opened from the Finder gets
`/usr/bin:/bin:/usr/sbin:/sbin`, and a client installed through a Node version manager (nvm, fnm,
volta, mise) or a package manager's own directory is on the login shell's `PATH` and nowhere else.
"On `PATH`" in Section 4's table means the shell's `PATH`, as in a terminal. The app reads it once
per launch, by asking the login shell (`$SHELL -l -i -c`) to print its environment, and gives it to
every CLI call it makes, ahead of its own. When the shell cannot be asked - no `SHELL`, no answer
within five seconds, nothing to read - the app's own `PATH` stands, and the CLI's fixed list of
directories (Homebrew, `~/.local/bin` and the like) still applies. The same read takes
`XDG_CONFIG_HOME` when the shell sets it, so the CLI, and the OpenCode it runs, find the
configuration a terminal's OpenCode reads (Section 13).

## 6. Decisions

- **A bridge, not a token handed out.** The bridge is the only option that reaches every client in
  Section 1's table, keeps the secret in one file, and survives daemon restarts. Writing the token
  into each client's config would have been less code, at the cost of the copies Section 1
  describes.
- **A relay, not a reimplementation.** Forwarding messages means a tool added to the daemon reaches
  every client with no change here. A bridge that re-declared the tools would be a second surface to
  keep in step.
- **Client CLIs before file edits.** A client's CLI is what it commits to keep working across its
  own upgrades. Its file format is not. Edits are kept to the acts a client's CLI does not do:
  everything for Claude Desktop and Cursor, removal for VS Code and OpenCode, and Claude Code's
  capture hook. *(Revised when OpenCode was added: this read "the two clients that offer nothing
  else", which VS Code's removal and the capture hook had already outgrown.)*
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
| **C2** | `connect` never writes a secret into a client's configuration. The bridge reads the daemon's token from its 0600 file. |
| **C3** | `connect` never replaces an entry it did not write unless `--replace` is given, and never edits a client file without first copying it aside. A file it cannot parse is refused, not rewritten. |
| **C4** | Through the bridge a client sees the daemon's surface unchanged: the same tool list, the same results, the same errors. |
| **C5** | `connect` without a client argument only reads. |
| **C6** | The capture hook never blocks the agent, never writes to the store itself, and speaks only after a commit that landed or at the start of a session. |

## 9. Ordering

1. **The bridge.**
2. **`connect`**: listing, registration and removal for the six clients in Section 4's table, and
   the seventh, OpenCode, in Section 13.
3. **The app's AI Apps submenu.**
4. **Documents.** README, the curl installer and the landing page show `supragnosis connect` and the
   app, rather than the `--header` command or the store-opening stdio line.
5. **Capture** (Section 12): `supragnosis hook claude-code`, its registration by `connect`, the
   listing's `capture` field, the P22 registry row, and the README's first minute.

## 10. Closure map

| Principle | Where this closes it |
|---|---|
| P17 - local surfaces stay local, the secret stays in one place | Section 3 (the daemon on loopback only, the token read from its file); C2 |
| P21 - one narrow surface | Section 2 and the relay decision; C4 |
| Outside principles.md's scope (client configuration) - P24's "the operator's file is theirs", followed by analogy | Section 4 (backup, merge, refuse what does not parse, `--replace` to take over); C3 |
| Outside principles.md's scope - P5's refusal to collapse states, followed by analogy | Section 4's listing distinguishes not installed, not connected, and connected some other way |
| daemon-lifecycle.md L1 - one owner | C1: the bridge is never a second writer |
| P22 - knowledge as a by-product of work | Section 12: a commit asks the agent for the decision behind it, and a session starts knowing what the project's workspace holds; C6 |

Guarded by:

- `the_bridge_relays_the_daemons_surface_unchanged` - C4. The tool list, and a tool call that lands
  in the daemon's store.
- `a_daemon_restart_is_invisible_through_the_bridge` - the replayed handshake.
- `the_daemons_own_errors_reach_the_client_as_written` - C4 for errors. A call to a tool the daemon
  lacks gets the same JSON-RPC error through the bridge as without it, and an HTTP-level refusal
  arrives as an error for that id carrying the daemon's words unchanged.
- `the_bridge_says_what_to_do_when_no_daemon_answers` and
  `a_refused_token_is_reported_with_its_file` - errors a person can act on.
- `registrations_run_the_bridge_and_carry_no_secret` - C2.
- `an_edit_changes_one_member_and_nothing_else`, `a_file_that_does_not_parse_is_refused` and
  `a_backup_never_replaces_an_earlier_one` - C3.
- `an_entry_connect_did_not_write_is_left_alone` - C3: `connect::plan_connect` over every entry
  (another's is refused without `--replace`, an unreadable config always), and
  `connect::write_backed_up`, the CLI's only write of a client's config, copies the file before it
  changes and stops with the file untouched when the copy cannot be taken.
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
- **The hook decides from HEAD's age, not from the Bash tool's result** (Section 12). A failed
  commit leaves no mark in the command's text, and the tool response's shape is Claude Code's to
  change; `git log -1` is the same on every version. `git log --grep commit` names `commit` without
  committing, and HEAD's age is what tells the two apart.
- **The groups `connect` writes read `hooks` before `matcher`.** serde_json writes object members
  in key order; the person's own groups are never re-serialized, which is what C3 asks, so only
  ours have that order.
- **Removal takes the array and `hooks` with it when only ours were in them.** Otherwise a settings
  file `connect` created the key in would keep a stray `"hooks": {}` after `--remove`, and "the file
  as it was" is the promise.
- **A client installed through nvm read "Not installed" in the app.** The CLI searched the app's
  `PATH` plus a fixed list of directories, so `codex` and `gemini` under
  `~/.nvm/versions/node/<version>/bin` were found in a terminal and missed from the Settings page.
  The app now hands the CLI the login shell's `PATH` (Section 5); listing more directories would
  have chased every version manager in turn.
- **The repository's workspace is usually not the node's `default`.** On the author's node the
  first session start said that the workspace `supragnosis` was not known there yet, while
  `default` held 168 entities. That line, not an error, is the point: the agent learns where the
  project's knowledge is kept, or that it is not kept yet.

## 12. Capture - the decision behind a commit

> Status: **built**. Principle 22's first clause was deferred in the registry with the reason that
> nothing makes an agent capture or retrieve unasked; this section is the mechanism that repaid it.
> What building it changed is in Section 11.

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

The hook names the workspace after the repository's directory, because the project is the unit
knowledge is organized by (the server instructions say so) and a repository is how a project appears
on disk. The directory is the one holding the `.git` that `git rev-parse --git-common-dir` names, so
a linked worktree is named by the repository it belongs to and not by its own directory, which for an
agent's worktree is a generated name that would open a workspace of its own. A submodule, whose
common directory sits under its parent's `.git/modules`, and a git too old to answer fall back to
`git rev-parse --show-toplevel`.
`SUPRAGNOSIS_WORKSPACE` in the hook's environment overrides it. The name is a suggestion in both
lines: the agent may know better (a monorepo, a fork, a workspace the person named differently), and
the text says "workspace", not "only this workspace".

### 12.4 Registration

- **`supragnosis connect claude-code` registers the hook beside the bridge**, in
  `~/.claude/settings.json`: one `PostToolUse` entry with matcher `Bash` and one `SessionStart`
  entry, each with the single command `<program> hook claude-code`, `<program>` chosen as the bridge's
  is. `--no-capture` registers the bridge alone, and on an app already connected through the bridge
  it removes the hook and keeps the bridge, which is the Settings page's capture switch turned off
  (design-system.md Section 6a). `connect --remove claude-code` removes both. The
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
- **Claude Code only, for now.** Codex, Gemini CLI, OpenCode, Cursor and VS Code each have hooks of their own
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

### 12.6 Guarded by

- `a_commit_that_landed_asks_for_the_decision_behind_it` - a repository with a fresh commit, made
  by the real git, and a `PostToolUse` input naming `git commit`: the answer carries the id, the
  subject, the author and the workspace, and no `decision` (C6).
- `a_command_that_landed_no_commit_gets_no_answer` - a command that is not `git commit`; the same
  `git commit` once HEAD is older than the budget; a call from another tool; a directory outside
  any repository: no output. An input with no event is an error, not silence.
- `git_commit_is_recognised_in_the_shapes_agents_write_it` - the heredoc, the `&&` chain, the
  `-C` and `-c` forms, a path to git; and not `commit-tree`, not an echo.
- `a_session_start_says_what_the_workspace_holds` - against a daemon's answers for a seeded store:
  the counts; for a workspace it has never seen: the first `observe` creates it; with no daemon: no
  output; a workspace name with a space reaches the daemon encoded.
- `connect_registers_the_hook_beside_the_bridge_and_removes_both` - a `settings.json` with other
  hooks keeps them byte for byte around the two groups; removal gives the file back as it was,
  the arrays and `hooks` included when only ours were in them; twice is once; a file that is not
  JSON, or whose `hooks` is not the documented shape, is refused (C3).
- The registry: C6 is `Scenario` over those, and P22's first clause moved from `Deferred` to
  `Scenario`, reworded to what is now true.

Checked end to end on the author's machine: the built binary answered a `SessionStart` event from
this repository against the live daemon ("workspace \"supragnosis\" is not known on this node yet
(this node holds \"default\", \"folio\")"), and with `SUPRAGNOSIS_WORKSPACE=default` reported 168
entities and 274 relations; a `PostToolUse` event six minutes after the last commit was silent; a
non-JSON input and `hook cursor` each exited 1 with the reason on stderr. `connect claude-code`
under a temporary HOME registered the bridge through the real `claude` CLI and the two groups
beside an existing `Stop` hook, the listing read `connected (bridge), capture on`, and `--remove`
gave the settings file back byte for byte with two backups kept.

## 13. OpenCode

The author asked, on 2026-10-21, for OpenCode to join the AI apps `connect` knows, and that every
change be checked against the principles before it is made (design-system.md Section 29.1). A
second reader checked the first draft of this section and then its revision; what that changed is
in Section 13.5.

### 13.1 What OpenCode offers

Read from OpenCode 2.0.32 on the author's machine, in throwaway homes so the author's own
configuration was not touched. The section says "2.x"; `connect` reads its entry back after adding
it, so a release that writes elsewhere fails there rather than silently.
- **Adding a server.** `opencode mcp add [--global] <name> -- <command...>` adds a local server, and
  its help lists `--url` and `--header` for a remote one. `--global` writes the user's
  configuration in `$XDG_CONFIG_HOME/opencode/`, or `~/.config/opencode/` when that is unset: into
  `opencode.json` where one exists, into `opencode.jsonc` where only that exists, and into a new
  `opencode.json` otherwise, keeping the file's comments. It returns within a second and starts no
  process.
- **Two files.** OpenCode reads both `opencode.json` and `opencode.jsonc` from that directory, in
  that order (read from its source). Which one wins when both name the same server was not settled
  by measurement.
- **The entry's shape.** `mcp.servers.<name>` = `{"type": "local", "command": [program, args...]}`:
  the program and its arguments are one array, one level below `mcp`.
- **No removal command.** It has `mcp list`, `add`, `auth` and `logout`, and no `mcp remove`.
- **The handshake.** It names itself `opencode` in the MCP handshake (`clientInfo.name`).
- **Its own installer** puts the binary in `~/.opencode/bin`, which is not always on a GUI app's
  `PATH`.
- **Its background service.** OpenCode 2.x runs a background server on a loopback port, restarted
  by `opencode service restart`. Its API answers 401 without the password OpenCode generates for it
  by default (measured on a throwaway service, and as a status code from the author's own).

### 13.2 The check

| Rule | What it asks | Standing |
|---|---|---|
| C1 | The bridge never opens the store and never starts a daemon | agrees - the entry is the same bridge |
| C2 | No secret in a client's configuration | agrees - the entry is `<supragnosis> bridge`, with no token |
| C3 | Never replace an entry `connect` did not write without `--replace`; copy a file aside before editing it; refuse a file it cannot parse | agrees - Section 4's wording now names JSONC for OpenCode; see 13.3 |
| C4 | The client sees the daemon's surface unchanged | agrees |
| C5 | `connect` without a client only reads | agrees |
| Section 6, "Client CLIs before file edits" | Edit a file only where the CLI does not do the act | agrees, as reworded - removal is a file edit because OpenCode has no `mcp remove`, as with VS Code |
| Section 6, "The user scope" | Everywhere, not one project | agrees - `--global` |
| Section 5, the CLI runs with the person's environment | The app's CLI calls see what a terminal sees | agrees, extended - the login shell's `XDG_CONFIG_HOME` is passed along with its `PATH` |
| Section 12, capture | Claude Code only, for now | agrees - OpenCode is outside capture, as Codex and Gemini are |
| onboarding.md Section 5, "Which app it is" | An app is known by the name it sends | agrees - `opencode` maps to the row |
| design-system.md Section 6a | A control both pages offer is built once in the app's shared script | repays - the apps' names and icons move into `kit.js`, read by both pages |
| P5 and P24 | Unknown is not none; a precedence is allowed when it is stated | agrees - see 13.3 for the file it reads and what an unparseable file reads as |
| P17 | The node-wide surface belongs to the one OS user who holds the token file | agrees - OpenCode runs as that user, as every app `connect` knows does, and the bridge reads the token file as that user; what OpenCode exposes of itself is OpenCode's (13.5) |
| P21 | No new tool without a repeated intent | agrees - the MCP surface does not change |
| compatibility.md Section 6 | A new value of an open field keeps the schema | agrees - a new client id in `connect --json`; the fixture gains the row |

No contradiction.

### 13.3 Behaviour

- **Detected** by `opencode` on `PATH`, or in `~/.opencode/bin`, which joins the directories
  `connect` searches.
- **Registered** by `opencode mcp add --global supragnosis -- <supragnosis> bridge`, with the
  person's `PATH` and `XDG_CONFIG_HOME`.
- **The files it reads** are the two OpenCode reads, in `$XDG_CONFIG_HOME/opencode/` or
  `~/.config/opencode/`. The listing's `config` is the one OpenCode's CLI writes (13.1).
  - The listing reads `mcp.servers.supragnosis` from each.
  - Where both hold one, it reports the one that needs the person's attention, in this order:
    `unknown`, `http`, `stdio`, `other`, `bridge`. Which one OpenCode loads is not settled (13.1),
    so the listing never calls a pair connected while either half is not.
  - A file that does not parse reads `unknown`, never `none` (P5).
  - An entry marked `"enabled": false` reads `other`, since it does not load - in every app's file,
    since Codex has the same key.
  - A `command` may be an array (program and arguments).
  - `type: "remote"` with a `url` reads `http`, as for every other client.
- **Removed** by a file edit, from both `opencode.json` and `opencode.jsonc` where each holds the
  entry, each backed up first (`~/.supragnosis/connect/opencode.<timestamp>.json` or `.jsonc`).
  This way `--replace` leaves no copy behind, and its "the copy of the token it held is gone" stays
  true. Both files are read and edited before either is written, so a file that does not parse
  stops the removal with neither touched.
  - **The dialect** is JSONC: `//` and `/* */` comments and trailing commas, nothing more (not
    JSON5).
  - **The edit** cuts one contiguous span, case by case:
    - a member with a comma after it (a separator, or a trailing comma): the member, that comma and
      what lies between them;
    - a last member with no comma after it: the comma before it, the member and what lies between
      them - unless a comment lies there, which is most often the previous member's; then only the
      member goes, and the comma before it stays as a trailing comma (legal, and a file holding a
      comment is JSONC already);
    - a member alone in its object: the member, and nothing else inside the braces.
    When the member had its line to itself, the span takes that line's indentation and line end too,
    so no blank line is left.
  - **The checks.** The result must parse. It must equal the original as a value everywhere but that
    member. As text it must equal the original with exactly that span taken out, and the span,
    comments aside, must hold nothing but the member, at most one comma and whitespace. Every
    comment outside the span stays.
- **The next step:** "start a new OpenCode session; if it does not list supragnosis, restart
  OpenCode's background service (`opencode service restart`)". A running service picked up some
  changes to its file and missed others (13.5), so the line names both.
- **First contact** maps the handshake name `opencode` to the row.
- **Listed** wherever the apps are named:
  - the CLI's help, and Setup's "no app is installed" sentence;
  - the app's menu, and both pages' rows (through `kit.js`, with the terminal icon Codex and Gemini
    have);
  - the README;
  - the curl installer's closing lines and the landing's connect section, with the release that
    ships it, since both are published the moment they reach main;
  - onboarding.md Section 4.2.

### 13.4 Guarded by

- `opencode_is_registered_through_its_cli_and_removed_by_a_jsonc_edit` - the add command; the files
  (`XDG_CONFIG_HOME` honoured, the CLI's choice between them); the listing across both, and
  `unknown` for one that does not parse; each case of the cut, a comment holding brackets and
  quotes, and every byte outside the span kept; strict JSON staying strict for the other apps.
- `opencode_removal_clears_both_files_or_neither` - removal from both files, each backed up, and
  none at all while one does not parse.
- `classify_reads_a_command_array_a_remote_url_and_a_disabled_entry`.
- `client_names_map_to_apps_and_unknown_names_to_none` - the first-contact name `opencode`.
- `every_app_page_names_the_apps_connect_lists` (the app) and `connect_json_matches_its_example`
  (the CLI): the example lists every app the CLI knows, and the menu and both pages name each one
  from the one table in `kit.js`.

### 13.5 What the second reading changed

The first draft got these wrong:
- **Two rules it would have contradicted:**
  - Section 4's "does not parse as JSON is refused";
  - Section 6's "the two clients that offer nothing else".
  Both are reworded above.
- **"Keeps every comment"** was promised without a check that could see comments; 13.3 now names
  the dialect, the span and a text check.
- **It read two shapes and two files with no precedence.** The 1.x shape is dropped, the file is
  OpenCode's own choice, and removal clears both files.
- **Smaller corrections:**
  - the first-contact citation (Section 5, not O7);
  - the backup's extension;
  - the onboarding pointer (onboarding.md Section 4.2);
  - design-system.md Section 6a, which the draft called met: it was owed, and this section repays
    it.
- **Three questions it left unmeasured,** now answered:
  - `mcp add` returns at once, so nothing can hang;
  - the service's API needs a password by default;
  - whether a running service reloads its configuration: a throwaway service picked up some
    changes to its file and missed others, so the next step names both acts.
- **A second check of the revised section, and measuring again before building,** corrected the
  file OpenCode writes when both exist (`opencode.json`, not `opencode.jsonc`). They found that
  OpenCode reads both files, so the listing and the removal cover both. They also settled the cut
  case by case.
- **Limits it found outside this section,** stated rather than fixed here:
  - Codex honours `CODEX_HOME`, and `connect` reads `~/.codex/config.toml`: a person who moves
    Codex's home sees `connect codex` fail its read-back.
  - VS Code reads JSONC, and `connect` holds its file to strict JSON (Section 4).
- **No password check.** The first measurement read the web interface's 200 as the API's and
  reported OpenCode's service as unprotected, and the author first chose that `connect` require a
  password on it. Once the API was found to take one by default, the author dropped the check as
  more than the act needs: `connect` registers a local server through OpenCode's own CLI, as for
  every other app, and an OpenCode left open to others already reaches the whole account - it runs
  commands and reads files, the token file among them - so a refusal here would close nothing.
