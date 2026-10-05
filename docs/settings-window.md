# Settings window - the desktop app's settings in one place

> The desktop app's own settings window: which server this Mac's AI apps use, how each AI app is
> connected, and this Mac's daemon. It replaces the tray's submenus and leaves the tray with what a
> menu is good at. Companion to [client-connect.md](client-connect.md) Section 5,
> [remote-server.md](remote-server.md) Section 5 and [daemon-lifecycle.md](daemon-lifecycle.md)
> Section 6, whose tray controls move here.
>
> Status: **specified**, not built.

## 1. Why this exists

Every setting the app has today lives in the tray menu: Start at Login, Restart Daemon, a Server
submenu and an AI Apps submenu. A menu can switch and toggle. It cannot take input, and it cannot
explain.

- **Adding a remote server needs input.** A profile is a name, a URL, a credential and sometimes a
  CA bundle. The tray can only switch between profiles that already exist, so adding one means
  `supragnosis server add` in a terminal, with the credential piped in. The path client-connect.md
  built for people who do not use a terminal ends there.
- **Outcomes have nowhere to go.** The result of the last action, the next step it needs ("quit
  Claude and open it again"), version drift and a remote server's state are all appended to one
  status line. It is the only text a menu has.
- **Every feature has grown a submenu.** Two so far, and the remote viewer would add a third.

## 2. What this is NOT

- **Not a second implementation of the settings.** The window calls the same CLI the tray calls, so
  the rules for the daemon's lifecycle, connecting an app and server profiles stay in one place,
  the CLI, where they are tested (daemon-lifecycle.md Section 6).
- **Not the viewer's settings.** The viewer has its own dialog - what the graph shows, the build,
  and on a hub its peers. That dialog belongs to the page a daemon serves. This window belongs to
  the app and is about this Mac.
- **Not reachable from a page the app shows.** Only the settings window can ask for a setting to
  change. The viewer window cannot, whatever page it shows (Section 4).

## 3. The window

The settings window is the app's own page, bundled with the app (`assets/settings.html`). It opens
from the tray's **Settings...** item and from the app menu (Cmd+,), and it has three sections.

### 3.1 Server

Which server this Mac's AI apps use (remote-server.md Section 3).

- **The profiles.** "This Mac" first, then each remote profile with its URL, the active one
  marked. The active one also shows its state: answers, refused the credential, or does not
  answer.
- **Use** makes a profile the active one. **Remove** deletes a remote profile and its credential.
  Removing the active profile is allowed: AI apps go back to this Mac, and the window says so.
- **Add a server** takes a name, a URL and the credential the server's operator issued, plus an
  optional CA bundle for a server whose certificate a private CA signed. The window runs
  `supragnosis server add <name> <url>`, with `--ca <file>` when one is given.
  - The credential is written to the CLI's stdin. It never goes into an argument, a log line or a
    response, and the field is cleared once the CLI has answered (S2).
  - The CLI checks the URL (HTTPS, or loopback HTTP) and refuses what it refuses. The window shows
    that refusal instead of deciding first.

### 3.2 AI apps

How each AI app reaches supragnosis (client-connect.md Section 4).

- **Each app is listed with its state:** connected through the bridge, connected the old way
  (HTTP or a stdio entry), another supragnosis entry, not connected, or not installed.
- **One button per app says what it will do:** Connect, Switch to the bridge, Replace, or
  Disconnect. A click is the consent to replace the entry that is there, as the tray's was.
- **The CLI's next step stays beside the app it concerns** ("quit Claude and open it again"),
  until the next refresh shows the app connected.

### 3.3 This Mac's daemon

- **The daemon's state:** the version running and the version installed, who runs it (login item,
  this app, Homebrew, `supragnosis start`), and the store's health - owed projections and the last
  recovery (crash-recovery.md K5).
- **Start at Login** is a switch, with the behaviour daemon-lifecycle.md Section 6 gives the tray's
  check item: on is `service install --take-over`, off is `service uninstall`.
- **Restart Daemon** restarts it, and the CLI's answer shows beside the button.
- **While a remote profile is active** these controls are disabled, with the reason: this Mac's
  daemon is not what the AI apps use (remote-server.md Section 5).

## 4. Who may change a setting

The commands behind the window change where this Mac's knowledge goes: which server receives what
the AI apps record, and which apps are connected. The viewer window shows pages a daemon serves.
Once remote-viewer.md's steps land, it will also show pages a hub on another machine serves. A page
from another machine must not be able to point this Mac's AI apps at a different server.

- **No command is open by default.** Tauri lets every window call every command an app registers,
  unless the app declares its commands in its build manifest. The app declares them, and a
  capability grants them to the window labelled `settings` and to no other (S1).
- **Each command also checks its caller.** It refuses unless the calling webview is the settings
  window, showing the app's own page. A capability file edited by mistake then fails closed.
- **The page is the app's, with a strict policy.** It loads nothing remote and runs no inline
  script. It shows what the CLI reports - profile names, URLs, messages - as text, never as markup
  (S5). A profile's name and URL are the user's own input, but a message can quote a server's
  answer.

## 5. The tray, after

The tray keeps what a menu does well - glancing and opening:

- **The status line.** One line: the server the AI apps use and its state, or this Mac's daemon
  and its version. If something needs doing - version drift, a refused credential, no AI app
  connected - the line says so and points to Settings.
- **Open Viewer.**
- **Settings...**
- **Quit Supragnosis.**

Start at Login, Restart Daemon, Server and AI Apps move to the window (S4).

## 6. Decisions

- **A window, not the viewer's dialog.** The viewer's page is the daemon's, served the same way to
  the app and, later, by a hub to another machine. The settings that change this Mac belong to the
  app's own page, the one no server supplies.
- **The CLI stays the implementation.** The window is a form over the commands the tray already
  calls, and the tray's handlers become the window's commands. Nothing about connecting an app or
  installing a login item is decided twice.
- **The tray loses its submenus rather than mirroring the window.** Two places to switch a server
  would be two places to keep consistent. The tray's job becomes saying what state this Mac is in
  and opening the window that changes it.

## 7. Invariants

| | Invariant |
|---|---|
| **S1** | Only the settings window can invoke a settings command. The viewer window cannot, whatever page it shows. The capability grants the commands to that window alone, and each command checks its caller. |
| **S2** | A credential typed into the window reaches the CLI on stdin and nowhere else: not an argument, a log line, a response, or the page once the CLI has answered. |
| **S3** | The window decides nothing the CLI decides. Every change is a CLI call, and the CLI's answer is what the window shows. |
| **S4** | Every setting the tray offered is in the window. Moving it loses nothing. |
| **S5** | The page renders the text it is given as text, never as markup. |
| **S6** | While a remote profile is active, the daemon controls are disabled with the reason, as the tray's were. |

## 8. What this revises

- **client-connect.md Section 5**: the AI Apps submenu becomes the window's AI apps section. Its
  rules carry over: the click is the consent, and the next step is shown.
- **remote-server.md Section 5**: the tray's Server submenu becomes the window's Server section,
  which can also add and remove profiles.
- **daemon-lifecycle.md Section 6**: Start at Login and Restart Daemon move from the tray to the
  window, with the same behaviour.
- **The README** describes the window instead of the tray menu.

## 9. Ordering

1. **The commands and who may call them.** The app manifest, the `settings` capability, and the
   caller check. The tray's handlers become the commands' implementations.
2. **The page.**
3. **The slimmed tray.**
4. **Docs**: the revisions in Section 8.

## 10. Closure map

| Principle | Where this closes it |
|---|---|
| P5 - unknown is not absent | Section 3: a server's state, an app's state and the store's health are shown with their reasons, never as a blank |
| P17 - knowledge sovereignty | Section 4, S1: what decides where this Mac's knowledge goes is reachable only from the app's own window |
| P18 - writes are an attack surface | S5: CLI-supplied text, which can quote a server, is never markup |
| P24 - degrade loudly, refuse when proceeding is worse | S3, S6: a refusal from the CLI is shown, and controls that do not apply say why |
