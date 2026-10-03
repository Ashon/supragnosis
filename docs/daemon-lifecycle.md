# Daemon lifecycle - one owner for the daemon, and the app as its switch (Principles 24, 17)

> Who starts the daemon at login, who restarts it after an upgrade, and what happens when more than
> one thing believes it is responsible. This document fixes the answer to the first two so that the
> third cannot arise unnoticed.
>
> Status: **specification, with Section 8 steps 1-4 built.** Sections 4, 6 and 7 carry corrections
> that building and releasing them forced; they are recorded in place rather than edited away.
> Section 11 (the 2026-10 review's corrections) is specified, not yet built.

## 1. Why this exists

On 2026-10-03, after `brew upgrade` had put v0.4.2 on disk, a client kept reporting v0.4.0 through a
restart. What was running:

- `com.supragnosis.daemon`, a LaunchAgent written by hand during the redb migration, owned the store
  and served from a process started that afternoon - the v0.4.0 image, still in memory although the
  path it was launched from now resolved to v0.4.2.
- `sh.brew.supragnosis-server`, the job `brew services restart` had just created, failed on start
  with `Database already open. Cannot acquire lock`, and with `KeepAlive` set it went on failing -
  6,124 lines in its error log before anyone looked.

Nothing reported either half. `supragnosis status` names one launchd label and saw a healthy daemon;
the desktop shell attached to the socket that answered; the failing job wrote to a log nobody reads.

The incident is one instance of a structural fact: the product ships **four ways to run the
daemon**, and the code that controls the daemon knows one of them.

| Manager | Installed by | At login | `status` / `restart` / `stop` know it |
|---|---|---|---|
| `com.supragnosis.daemon` (LaunchAgent) | `deploy/install.sh`, from a template with another user's paths in it | yes | yes |
| `sh.brew.supragnosis-server` (LaunchAgent) | `brew services start` - what README's quick start says | yes | **no** |
| pidfile (`~/.supragnosis/supragnosis.pid`) | `supragnosis start` | no | yes |
| child of the desktop shell | the shell, when no socket answers | while the app runs | no (the shell reaps it) |

So for a user who followed the README, `supragnosis restart` and `supragnosis stop` refuse ("a
daemon is responding ... but is not managed by this CLI or launchd"), and the tray's Restart Daemon
- which runs `supragnosis restart` and ignores its exit status - quietly re-attaches to the old
process. After an upgrade, the one action meant to load the new binary does nothing and says
nothing.

## 2. What this is NOT

**It is not a fifth way to run the daemon.** It removes one (brew services) and makes the CLI and
the app agree on the rest. The number of ways the product can start the daemon goes down.

**It is not a bundled daemon.** Registering an agent through `SMAppService` would put the app in
System Settings > Login Items and keep app and daemon versions locked together, but it requires the
server binary inside the app bundle (the cask deliberately ships none -
`deploy/homebrew/README.md`), widens what is signed and notarized, and leaves CLI-only and
source-build users on a different path. It stays a possible later step; nothing here depends on it.

**It is not an MCP tool.** Starting and stopping the daemon is an operator act on the operator's
machine. It belongs to the tray and the CLI, the human-facing surfaces, and never to the agent
surface (P21) - an agent that could unload the process it is talking through is not a feature.

## 3. One owner

At any moment at most one manager owns the daemon. That is already true of the store - redb admits
one writer, which is exactly why the second job could only fail - so the lifecycle code has to be at
least as strict as the storage it fronts, and has to say what it sees instead of finding out by lock
error.

**Every manager the product has ever installed is recognized**, whether or not the product still
installs it: the canonical label `com.supragnosis.daemon`; Homebrew's `sh.brew.supragnosis-server`
and its older forms (`homebrew.mxcl.supragnosis-server`, and both shapes for the retired formula
token `supragnosis`); the pre-0.1.2 label `com.ashon.supragnosis`; and the pidfile. A manager the
code does not know about is how today's incident stayed invisible.

**The decision is a pure function of observations.** The CLI observes which of those labels launchd
has loaded (and which of them has a live pid), whether the pidfile names a live process, and whether
the MCP port and viewer socket answer. From that it classifies:

- **none** - nothing loaded, nothing answering: stopped. If the canonical plist is installed (a
  `stop` unloaded its job), `restart` loads that job again rather than starting a pidfile daemon
  beside the login item - which would be two owners again at the next login.
- **one** - act on it. `restart` kickstarts a launchd job of any recognized label, or stops and
  starts the pidfile daemon. `stop` boots out or signals the same.
- **more than one** - a conflict. `status` lists every manager it found and which one holds the live
  process; `restart` and `stop` refuse, name them, and print the command that resolves it
  (`supragnosis service install --take-over`, Section 4). Guessing is the wrong move here:
  restarting one of two owners is how a crash loop gets a second wind.
- **answering but unrecognized** - something serves the port that is none of the above. Reported as
  such, never as "stopped" (P5: unknown is not absent).

Only observing and acting shell out (`launchctl`, a socket connect). The classification is a
function from observations to an outcome, tested as a table in
`classification_counts_managers_not_processes`, whose rows include the incident of Section 1.

## 4. The canonical manager and `supragnosis service`

The always-on daemon on macOS is the LaunchAgent `com.supragnosis.daemon`, in the user's `gui/<uid>`
domain. The CLI gains one subcommand that owns it:

- **`supragnosis service install`** generates the plist and bootstraps it.
- **`supragnosis service uninstall`** boots the job out and retires the plist (below).

The plist is **generated, not templated**. The template that lived in `deploy/launchd/` carried
absolute paths for one user, so every other user was told to hand-edit it - which is how
hand-written plists with divergent environments came to exist; it is deleted. Generated, the plist
contains: the program and `serve`, `RunAtLoad`, `KeepAlive`, stdout and stderr in
`~/.supragnosis/log/`, an `EnvironmentVariables` dict whose one variable of its own is
`SUPRAGNOSIS_HTTP_ADDR=127.0.0.1:7373`, and a marker comment saying it was generated and by which
version. Guarded by `the_generated_plist_is_marked_escaped_and_carries_env_verbatim`.

> **Correction, from building it.** This section first said the generated plist carries no
> environment, configuration belonging to `supragnosis.toml`. But `SUPRAGNOSIS_HOST`, `_WORKSPACE`,
> `_EMBED` and `_DATA_DIR` are settings that exist only as environment variables, and the host is
> recorded in the provenance of every new observation. Dropping a hand-written plist's environment
> on take-over would change who the daemon says it is, without a word - P24's "the operator's file
> is theirs" applied to its contents, not only to the file. So the job's environment is carried
> forward verbatim from the plist being replaced (generated or hand-written), `--env KEY=VALUE` adds
> `SUPRAGNOSIS_*` keys and nothing else, and `install` prints what the job carries.

**The program path survives upgrades.** The running binary resolves to a versioned keg
(`<prefix>/Cellar/supragnosis-server/<version>/bin/supragnosis`), and pinning that path would pin
the version. When the executable lives in a keg and
`<prefix>/opt/supragnosis-server/bin/supragnosis` exists, the plist names the `opt` link, which
Homebrew repoints on upgrade. Otherwise (a source build, `~/.local/bin`) it names the executable as
found. Guarded by `a_keg_path_becomes_the_opt_link_that_upgrades_repoint`.

**Install refuses to share.** If another recognized manager is loaded, `install` stops and prints
which one and the command that retires it. With `--take-over` it retires it itself: a Homebrew job
by `brew services stop <token>` when `brew` is on the path (the file is Homebrew's, so Homebrew
removes it), and by a printed instruction when it is not; a running pidfile daemon by stopping it.
A holder it cannot name is refused with or without `--take-over`: take-over retires managers by
name, and something answering with no manager has none to retire it by. Guarded by
`install_refuses_a_holder_it_cannot_name`.

> **Correction, before release.** This section first covered only recognized managers, so a daemon
> the desktop app spawned for its session - no pidfile, no launchd job - left `install` free to start
> the canonical job beside it, which is Section 1's crash loop reached by the command the caveats
> recommend. Section 6's switch already stopped its own child first; the CLI did not. `install` now
> refuses while something unrecognized answers, says it is most likely the app's daemon and that the
> app's switch handles it, and checks again after retiring the others that the address actually
> went quiet before it writes anything.

**The operator's file is theirs** (P24). A canonical-label plist without the marker was written by a
person. `install` will not overwrite it, and `--take-over` moves it aside to
`~/.supragnosis/launchd/com.supragnosis.daemon.plist.<timestamp>` instead of deleting it, so
whatever environment it carried can be recovered. `uninstall` treats it the same way. A generated
plist is removed outright; there is nothing in it the generator cannot write again.

## 5. Version drift is reported where it is read

The daemon answers `/api/about` with its version over the viewer socket, and the CLI knows its own.
When they differ, `status` says so in a line of its own (guarded by
`drift_never_assumes_the_running_version`) - running, installed, and the command that reconciles
them - and the tray status line carries the same fact. If the socket does not answer, the running
version is reported as unknown, not as the installed one.

This is P24's demand applied to upgrades: a binary replaced on disk under a process that keeps the
old image is a degrade, and a degrade nobody can see has become a silent one. Today it surfaced as a
client showing a number nobody expected.

## 6. The app

The shell gains one control and loses one silence. Both go through the CLI, found where it is found
today (`find_server_bin`); the shell shares no code with the server, so it does not write plists or
classify managers itself - one implementation, in the workspace where it is tested.

- **Start at Login**, a check item in the tray menu, reflects whether the canonical job is
  installed. Turning it on runs `supragnosis service install --take-over` (the click is the consent;
  the status line then says what was retired) and re-attaches. Turning it off runs `supragnosis
  service uninstall`. That stops the daemon - the one path by which the app stops a daemon it did
  not spawn - so the shell then falls back to attach-or-spawn and runs one for the session, and the
  status line says the daemon is no longer started at login.
- **The status line** names the manager (launchd, Homebrew, pidfile, spawned by this app) and
  carries version drift and conflicts from `status`.
- **Restart Daemon** reports its outcome. The CLI's exit status and message reach the status line; a
  refused restart (a conflict) says why instead of re-attaching to the process it failed to replace.
- A CLI too old to know `service` leaves the item disabled with "update supragnosis-server" rather
  than failing on click.

> **Correction, from building it.** Turning the switch on cannot just call the CLI. A daemon this
> app spawned holds the store, and the CLI cannot see it - it has no pidfile and no launchd job - so
> install would start the canonical job beside it, the job would fail on the lock, and KeepAlive
> would retry it forever: the crash loop of Section 1, produced by the switch meant to prevent it.
> The shell stops its own child first, and after install waits for the socket before attaching, so a
> job launchd is still starting is attached to rather than raced by a second spawn.

Quitting the app still never stops a launchd-managed daemon. Its MCP clients outlive the window,
which is the reason the shell attaches rather than owns.

## 7. Homebrew and the documents

- The formula drops its `service do` block. Its caveats point to the app's Start at Login and to
  `supragnosis service install`. An existing `brew services` job keeps running after the upgrade and
  is now visible to `status`; `service install --take-over` migrates it.
  **Correction:** this reaches users only because the release renders the tap's formula from
  `deploy/homebrew/` (`update-tap.sh`). The script had edited the tap's own copy, version and
  sha256 only, so the block would have stayed in the tap under the new binary.
- `deploy/launchd/com.supragnosis.daemon.plist` is deleted - the generator replaces it - and
  `deploy/install.sh` calls `supragnosis service install --take-over` after copying the binary.
- README's quick start and the Homebrew upgrade section say `brew upgrade` then `supragnosis
  restart`, which after Section 3 works for whichever single manager is in place.
- Linux is out of scope. The systemd user unit in `deploy/systemd/` stays documented; recognizing it
  in `status` is the natural next step and would follow Section 3's rule unchanged.

## 8. Ordering

1. **Recognition** (Section 3) and **drift** (Section 5) in `status`, `restart` and `stop`. Useful
   on its own: it repairs restart for Homebrew users today and makes a conflict visible.
2. **`supragnosis service install|uninstall`** (Section 4), with take-over and the operator-file
   rule.
3. **The app** (Section 6): the toggle, the status line, the reported restart.
4. **Homebrew and documents** (Section 7). Last, because the formula change is what moves existing
   users, and the steps before it are what they move to. All four land before the next release,
   whose note carries the migration.

## 9. Invariants

| | Invariant |
|---|---|
| **L1** | At most one manager owns the daemon. `install` refuses while another is loaded unless told to take over, and while something it cannot name answers or holds the store even then; `restart` and `stop` refuse on a conflict and name the managers. |
| **L2** | Every manager the product has installed is recognized by `status`, `restart` and `stop`, including retired labels. |
| **L3** | Version drift is visible: `status` and the tray show running and installed versions when they differ, and an unanswering daemon's version is unknown, not assumed. |
| **L4** | Lifecycle failures are loud: every CLI lifecycle command exits non-zero on failure with the reason - including a job it loaded that does not come up - and the app never discards that exit status. |
| **L5** | The operator's file is theirs: `install` never overwrites a plist it did not generate, and nothing deletes one - it is moved aside. |
| **L6** | The daemon's lifetime is not the app's. Quitting the app never stops a launchd-managed daemon; turning Start at Login off is the only way the app stops one it did not spawn. |
| **L7** | The generated job adds no exposure: loopback MCP, the viewer's unix socket, the bearer token unchanged. Nothing in the plist widens a bind or disables auth, and `install` refuses an environment that would. |
| **L8** | No new privilege: a user LaunchAgent in the `gui/<uid>` domain, no administrator rights, no helper tool. |
| **L9** | A signal goes only to a supragnosis process. A pidfile naming any other process is stale, and is cleared rather than acted on. |

## 10. Closure map

| Demand | Where it is answered |
|---|---|
| P24 - degrade loudly, in the place that is read | Sections 1, 5, 6; L3, L4 |
| P24 - the operator's file is theirs | Section 4; L5 |
| P24 - refuse when proceeding would be worse | Section 3 (a conflict is refused, not guessed); L1 |
| P5 - unknown is not absent | Sections 3, 5 (an unrecognized holder, an unanswering daemon); L3 |
| P17 - local surfaces stay local and authenticated | Section 4; L7 |
| P21 - the agent surface stays narrow | Section 2 (lifecycle is not a tool) |

## 11. Corrections from the 2026-10 review

An adversarial review after v0.4.3 tested these sections against running processes rather than
against their text. Five places where the built behavior fell short of an invariant, each with the
rule that now closes it. Recorded here rather than edited into the sections above, because what the
first version missed is the part worth keeping.

- **A stdio server holds the store and answers nothing (L1).** Section 3's "answering but
  unrecognized" looked only at the MCP port. A `supragnosis` stdio server - what `claude mcp add
  supragnosis -- supragnosis` starts in every session - holds the redb lock and binds no port. So
  `install` saw nothing, loaded the canonical job beside it, and the job failed on the lock under
  KeepAlive: Section 1's crash loop again, from a third direction. The observation now includes
  whether the store is held, probed with a read-only open that fails while a writer holds it. Held
  by no manager counts as unrecognized, both before `install` acts and after it retires the others.
- **`install` reported success for a job that never came up (L4).** After loading the job it printed
  "the socket has not answered yet" and exited 0, whatever the job was doing. It now waits for the
  daemon to answer. If launchd shows the job with no process and a non-zero last exit, `install`
  exits non-zero and names the error log. A slow start that is still running is reported as slow,
  not as failed.
- **The plist could disable auth (L7).** `--env` admitted any `SUPRAGNOSIS_*` key, so
  `--env SUPRAGNOSIS_MCP_AUTH=off` produced a generated job without authentication, and the
  environment carried forward from a replaced plist was never checked. `install` now refuses an
  environment - given or carried - that turns off authentication or the ingest secret scan, or names
  an HTTP address the daemon would refuse as non-loopback. The last one would also have been a
  KeepAlive crash loop, since `serve` exits on it. The carried case refuses rather than drops: the
  operator's file is theirs (L5), and silently removing a setting they wrote would change the
  daemon without a word.
- **A stale pidfile could aim SIGTERM at an unrelated process (L9).** The pidfile's process counted as
  a manager when `kill -0` succeeded, which any process holding a reused pid passes. `stop`,
  `restart` and `install --take-over` - which the app's switch runs - would then signal it. Before
  acting, the CLI now confirms the pid belongs to a supragnosis executable. A pidfile that names
  anything else is stale and is cleared.
- **`status` printed the bearer token.** Agents run `status`, so the token landed in their
  transcripts. `status` and `start` now print where the token is and a command line that reads it
  (`$(cat ~/.supragnosis/mcp.token)`), never the value.

