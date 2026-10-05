# Settings page - the desktop app's settings in one place

> The desktop app's own settings page, shown in the main window in place of the viewer: which server
> this Mac's AI apps use, how each AI app is connected, and this Mac's daemon. It replaces the tray's
> submenus and leaves the tray with what a menu is good at. Companion to
> [client-connect.md](client-connect.md) Section 5, [remote-server.md](remote-server.md) Section 5
> and [daemon-lifecycle.md](daemon-lifecycle.md) Section 6, whose tray controls moved here.
>
> Status: **built**. Sections 11 and 12 record what guards it and what building it changed.

## 1. Why this exists

Every setting the app had lived in the tray menu: Start at Login, Restart Daemon, a Server submenu
and an AI Apps submenu. A menu can switch and toggle. It cannot take input, and it cannot explain.

- **Adding a remote server needs input.** A profile is a name, a URL, a credential and sometimes a
  CA bundle. The tray could only switch between profiles that already existed, so adding one meant
  `supragnosis server add` in a terminal, with the credential piped in. The path client-connect.md
  built for people who do not use a terminal ended there.
- **Outcomes had nowhere to go.** The result of the last action, the next step it needs ("quit
  Claude and open it again"), version drift and a remote server's state were all appended to one
  status line. It was the only text a menu has.
- **Every feature had grown a submenu.** Two so far, and the remote viewer would have added a third.

## 2. What this is NOT

- **Not a second implementation of the settings.** The page calls the same CLI the tray called, so
  the rules for the daemon's lifecycle, connecting an app and server profiles stay in one place,
  the CLI, where they are tested (daemon-lifecycle.md Section 6).
- **Not the viewer's settings.** The viewer has its own dialog - what the graph shows, the build,
  and on a hub its peers. That dialog belongs to the page a daemon serves. This page belongs to the
  app and is about this Mac.
- **Not callable from any other page.** The settings commands answer only this page. The viewer
  cannot call them, whatever it shows, even though it shares the window (Section 4).

## 3. The page

The settings page is the app's own page, bundled with the app (`assets/settings.html`). It opens in
the main window, in place of the graph.

### 3.0 Moving between the graph and the settings

The main window has two pages, and both carry the same title bar.

- **The title bar is the viewer's.** Same row height, glass, rule and mark, with the window's
  traffic lights over it in the same place.
- **A segmented control beside the name** - Graph | Settings - says which page is showing and moves
  between them. It sits in the same place on both pages, so moving changes the content, not the
  chrome.
- **The keyboard does the same.** Cmd+1 shows the graph, from the View menu. Cmd+, shows the
  settings, from the app menu where macOS users look for it. The tray's Open Viewer shows the graph,
  and Settings... shows the settings.
- **The tray opens Settings where the attention is.** A refused credential or an unanswering server
  opens the Server section; no AI app connected opens AI apps; version drift opens This Mac.
- **One stylesheet draws the chrome on both pages** (`assets/shell.css`), so the two cannot drift
  apart.
  - The settings page links it from the app's own origin.
  - The viewer is the daemon's page, so the shell serves the same file on the viewer's origin, at
    `/__shell/`, and its init script links it there. The shell answers that path itself and never
    passes it to a daemon or a hub.

### 3.0.1 The layout

The page follows the shape of the platform's own settings:

- **A sidebar of sections** - Server, AI apps, This Mac, About - each with a status dot, so the
  state of everything is visible before anything is opened.
- **A content pane** with the selected section: a title, one sentence on what it is for, and
  grouped cards of rows. Each row has a name, a line of state, and its control on the right - a
  switch, a button, a status pill.
- **Dialogs for what needs input or consent.**
  - Adding a server is a sheet with labelled fields and help text. It checks the shape of what was
    typed before the CLI does.
  - Removing a server asks first.
- **Feedback where the action was.** A button shows progress while its CLI call runs, and only that
  button waits. The outcome arrives as a notice that a success dismisses on its own and an error
  keeps until it is read. The first load shows the shape of the page rather than a blank.

The sections follow.

### 3.1 Server

Which server this Mac's AI apps use (remote-server.md Section 3).

- **The profiles.** "This Mac" first, then each remote profile with its URL, the active one
  marked. The active one also shows its state: answers, refused the credential, or does not
  answer.
- **Use** makes a profile the active one. **Remove** deletes a remote profile and its credential.
  Removing the active profile is allowed: AI apps go back to this Mac, and the page says so.
- **Add a server** takes a name, a URL and the credential the server's operator issued, plus an
  optional CA bundle for a server whose certificate a private CA signed. The shell runs
  `supragnosis server add <name> <url>`, with `--ca <file>` when one is given.
  - The credential is written to the CLI's stdin. It never goes into an argument, a log line or a
    response, and the field is cleared the moment the value is handed over (S2).
  - The CLI checks the URL (HTTPS, or loopback HTTP) and refuses what it refuses. The page shows
    that refusal instead of deciding first.
  - "Use it now", checked by default, makes the new profile active once it is added.

### 3.2 AI apps

How each AI app reaches supragnosis (client-connect.md Section 4).

- **Each app is listed with its state:** connected through the bridge, connected the old way
  (HTTP, or a stdio entry), another supragnosis entry, not connected, or not installed.
- **One button per app says what it will do:** Connect, Switch to the bridge, Replace, or
  Disconnect. A click is the consent to replace the entry that is there, as the tray's was. An app
  a click could do nothing for - not installed, its settings unreadable - has no button.
- **The CLI's next step stays beside the app it concerns** ("quit Claude and open it again").

### 3.3 This Mac's daemon

- **The daemon's state:** the version and who runs it (login item, this app, Homebrew,
  `supragnosis start`), version drift when there is any, and the store's health - owed projections
  and the last recovery (crash-recovery.md K5).
- **Start at Login** is a switch, with the behaviour daemon-lifecycle.md Section 6 gave the tray's
  check item: on is `service install --take-over`, off is `service uninstall`.
- **Restart Daemon** restarts it, and the CLI's answer shows at the foot of the page.
- **While a remote profile is active** these controls are disabled, with the reason: this Mac's
  daemon is not what the AI apps use (remote-server.md Section 5).

## 4. Who may change a setting

The commands behind the page change where this Mac's knowledge goes: which server receives what the
AI apps record, and which apps are connected. The window that shows the page also shows the viewer,
which a daemon serves. Once remote-viewer.md's steps land, it will also show a hub's viewer, which a
machine elsewhere serves. A page from another machine must not be able to point this Mac's AI apps
at a different server.

- **No command is open by default.** Tauri lets every page call every command an app registers,
  unless the app declares its commands in its build manifest. The app declares them, and one
  capability grants them.
- **The capability cannot be the lock.** It is scoped to windows, and the settings page and the
  viewer are two pages of one window. So it grants the commands to the main window.
- **Each command checks its caller, and that is the lock.** It refuses unless the calling window
  is showing the app's own settings page - its scheme, host and path - at the top level (S1).
  - A viewer page that frames the settings page cannot call through the frame, because the top
    level is still the viewer.
  - Navigating away from the settings page, to the viewer or anywhere else, takes the commands with
    it.
- **The page is the app's, with a strict policy.** It loads nothing remote and runs no inline
  script or style. It shows what the CLI reports - profile names, URLs, messages - as text, never as
  markup (S5). A profile's name and URL are the user's own input, but a message can quote a
  server's answer.

## 5. The tray, after

The tray keeps what a menu does well - glancing and opening:

- **The status line.** One line: the server the AI apps use and its state, or this Mac's daemon and
  its version. If something needs doing - version drift, a refused credential, an unanswering
  server, no AI app connected - the line says so and points to Settings.
- **Open Viewer.**
- **Settings...**
- **Quit Supragnosis.**

Start at Login, Restart Daemon, Server and AI Apps moved to the page (S4).

## 6. Decisions

- **A page in the main window, not a window of its own.** One window, with the graph a click away.
  This was asked for after the first version, which used a separate window (Section 12). What it
  costs is that the capability can no longer separate the settings page from the viewer, so the
  caller check carries S1 alone.
- **Not the viewer's dialog.** The viewer's page is the daemon's, served the same way to the app
  and, later, by a hub to another machine. The settings that change this Mac belong to the app's
  own page, the one no server supplies.
- **The CLI stays the implementation.** The page is a form over the commands the tray already
  called, and the tray's handlers became the page's commands. Nothing about connecting an app or
  installing a login item is decided twice.
- **Navigation is chrome, not a link on the page.** A control in the title bar that is the same on
  both pages, plus the keyboard, rather than a button the settings page alone carries. The viewer
  does not know the shell exists, so the shell adds the control to the viewer's title bar, as it
  already adds the drag region.
- **The tray loses its submenus rather than mirroring the page.** Two places to switch a server
  would be two places to keep consistent. The tray's job is to say what state this Mac is in and to
  open the page that changes it.

## 7. Invariants

| | Invariant |
|---|---|
| **S1** | A settings command runs only for the app's own settings page, at the top level of the main window. The viewer cannot invoke one, whatever page it shows, framed or not. |
| **S2** | A credential typed into the page reaches the CLI on stdin and nowhere else: not an argument, a log line, a response, or the page once handed over. |
| **S3** | The page decides nothing the CLI decides. Every change is a CLI call, and the CLI's answer is what the page shows. |
| **S4** | Every setting the tray offered is on the page. Moving it lost nothing. |
| **S5** | The page renders the text it is given as text, never as markup. |
| **S6** | While a remote profile is active, the daemon controls are disabled with the reason, as the tray's were. |

## 8. What this revises

- **client-connect.md Section 5**: the AI Apps submenu became the page's AI apps section. Its rules
  carry over: the click is the consent, and the next step is shown.
- **remote-server.md Section 5**: the tray's Server submenu became the page's Server section, which
  can also add and remove profiles.
- **daemon-lifecycle.md Section 6**: Start at Login and Restart Daemon moved from the tray to the
  page, with the same behaviour.
- **The README** describes the page instead of the tray menu.

## 9. Ordering

1. **The commands and who may call them.** The app manifest, the capability, and the caller check.
   The tray's handlers became the commands' implementations. [built]
2. **The page.** [built]
3. **The slimmed tray.** [built]
4. **Docs**: the revisions in Section 8. [built]

## 10. Closure map

| Principle | Where this closes it |
|---|---|
| P5 - unknown is not absent | Section 3: a server's state, an app's state and the store's health are shown with their reasons, never as a blank |
| P17 - knowledge sovereignty | Section 4, S1: what decides where this Mac's knowledge goes answers only the app's own page |
| P18 - writes are an attack surface | S5: CLI-supplied text, which can quote a server, is never markup |
| P24 - degrade loudly, refuse when proceeding is worse | S3, S6: a refusal from the CLI is shown, and controls that do not apply say why |

## 11. Guarded by

- **S1**: `only_the_apps_own_page_may_change_a_setting` holds the caller check to the settings
  page's address and refuses the viewer, a hub, another page of the app and a lookalike path.
  `settings_commands_are_closed_until_granted` holds build.rs's manifest and the capability to the
  command list, and keeps the viewer's capability free of them.
- **S2**: `a_credential_never_becomes_an_argument` - the arguments of `server add` are built without
  it.
- **S5**: `the_settings_page_never_renders_markup` - no markup sink in the page's script or in the
  init script that builds the navigation on every page, a policy in the page's head, no inline
  script.
- **The page's rows**: `an_app_item_says_what_a_click_will_do`, which once tested the tray's AI Apps
  items, now tests the page's app rows: the state, and the button's label or its absence.

Checked in a development build attached to this Mac's running daemon:
- **Navigation.** The Graph | Settings control sat beside the name on both pages and moved between
  them. Cmd+, and Cmd+1 did the same from the keyboard.
- **The page matched the CLI.**
  - Server: This Mac in use and answering.
  - AI apps: Claude Desktop and Claude Code connected, Cursor not installed, the rest with Connect.
  - This Mac: running 0.4.6 as a login item, Start at Login on, nothing owed.
  - About: the versions and paths.
- **The Add server sheet** opened, refused an empty submission field by field without calling the
  CLI, and closed on Escape.

No setting was changed while checking.

## 12. What building it changed

- **The page was first a window of its own.** The first version of this document opened Settings in
  a separate window and confined the commands to it by capability. Moving the page into the main
  window, as asked, moved S1's lock from the window's label to the page's address (Section 4).
- **The page carries the title bar's traffic-light padding itself.** The shell's init script gives
  the viewer that padding by injecting a style element, and the settings page's policy refuses
  inline style. The page's own stylesheet holds the rule instead; the script still toggles the
  fullscreen class it depends on.
- **The app menu is the platform's default plus Settings...** A custom menu would have to restate
  Edit, without which a credential cannot be pasted into the page. Inserting one item into the
  default menu keeps Edit as the platform defines it.
- **Placeholders are dimmed.** At full brightness the example values in the Add a server form read
  as values already entered.
- **Navigation became chrome.** The second version had a "back to the viewer" button on the settings
  page alone. It is replaced by the Graph | Settings control, the same on both pages, and the
  keyboard (Section 3.0).
- **The icons are a set, not drawings.** They are Lucide's (ISC), copied unmodified into
  `assets/icons/` with the license, because the pages load nothing remote (icons/README.md). CSS
  draws them as masks over `currentColor`, so an icon takes its text's color.
- **The chrome moved into one stylesheet.**
  - The init script used to inject the viewer's chrome rules as an inline style element, which the
    settings page's policy refused. Those rules now live in `shell.css` with the page navigation.
  - The shell serves `shell.css`, and the two icons it draws, on the viewer's origin at `/__shell/`.
  - The init script builds the navigation on both pages, so its markup has one source too, and it
    is held to the no-markup rule with the page's script.
- **Hidden means hidden.** A class that sets `display` overrides the `hidden` attribute, and the Add
  server sheet showed an empty error box until the page said `[hidden]` wins.
