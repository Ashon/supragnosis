//! supragnosis-app - the desktop shell (a thin client over the daemon).
//!
//! A Tauri webview around the daemon's unix-socket viewer (Principle 21: a human-facing channel,
//! separate from the MCP tool surface). The shell embeds NO frontend and NO engine - the viewer UI
//! is served by the daemon (supragnosis-viz), so the app keeps a single UI source, and the store
//! stays single-process (the daemon owns the db; the shell owns
//! nothing). It adds exactly three things:
//!
//! 1. **Daemon lifecycle** - attach to a live viz socket, or spawn `supragnosis serve` as a child
//!    and reap it on exit. An externally managed daemon (launchd / `supragnosis start`) is never
//!    stopped by the shell - its MCP clients outlive this window.
//! 2. **Transport** - a `viz://` custom protocol proxies every webview request onto HTTP over the
//!    unix socket. The socket file's 0600 mode remains the only access control; the shell
//!    reintroduces no TCP (the point of retiring the localhost viewer port).
//! 3. **SSE bridge** - the webview custom protocol cannot stream, so the shell holds the
//!    /api/events connection in Rust and re-emits frames as "viz-event" Tauri events; an init
//!    script swaps EventSource for a listener facade (assets/eventsource-shim.js).
//! 4. **Settings** - a page of the app's own (docs/settings-page.md), in the main window: the server this Mac's AI
//!    apps use, how each AI app is connected, and this Mac's daemon (Start at Login, Restart). The
//!    shell decides nothing here: it asks the CLI it found (`status --json`, `server`, `connect`,
//!    `service install|uninstall`, `restart`) and shows the answer, so those rules live once, in the
//!    workspace where they are tested. The tray only says what state this Mac is in and opens it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    http, Emitter, Listener, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

mod cli;
mod proxy;

use cli::*;
use proxy::*;

/// The shell's relationship to the daemon. Spawned = ours to reap and restart; External =
/// attached to an externally managed daemon (launchd / `supragnosis start` / another shell) -
/// never killed by us (its MCP clients outlive this app).
enum Daemon {
    Starting,
    External,
    Spawned(Child),
    Failed(String),
}

/// What quitting the app stops (daemon-lifecycle.md L6): only a daemon this app spawned, which lives
/// as long as the app does. One it attached to - a login job's, Homebrew's, a `supragnosis start` -
/// keeps running, and nothing on the way out runs the CLI to stop it.
fn reaped_on_exit(daemon: Daemon) -> Option<Child> {
    match daemon {
        Daemon::Spawned(child) => Some(child),
        Daemon::Starting | Daemon::External | Daemon::Failed(_) => None,
    }
}

impl Daemon {
    /// What the shell knows of the daemon by itself, in the state words every surface uses
    /// (docs/design-system.md Section 9, step 45).
    fn status_line(&self) -> String {
        match self {
            Daemon::Starting => "Starting...".to_string(),
            Daemon::External => "Running - started outside this app".to_string(),
            Daemon::Spawned(_) => "Running while this app is open".to_string(),
            Daemon::Failed(e) => format!("Could not start: {}", cause_of(e)),
        }
    }
}

struct DaemonGuard(Mutex<Daemon>);

/// What the tray last showed, so it is built again only when that changes (design-system.md
/// Section 29.13).
struct TrayKept(Mutex<Option<TrayState>>);

/// The app menu's Quit, whose words follow the tray's (D15).
struct AppQuit(PredefinedMenuItem<tauri::Wry>);

/// The menu bar's map commands, by id, so each page's load report can enable the ones that act there.
struct MapItems(Vec<(&'static str, MenuItem<tauri::Wry>)>);

/// Which map commands act on a page, from its address and the controls it reported (design-system.md
/// Section 29.13): on the Graph the ones whose control it has; on Review, Settings and Setup the ones
/// that open the Graph, Back, and Forward on the Graph only; nothing on the starting page or a viewer
/// without the selection history.
fn map_items_enabled(url: &str, controls: &[String]) -> Vec<(&'static str, bool)> {
    let has = |c: &str| controls.iter().any(|x| x == c);
    let viewer = url.starts_with("viz://");
    let app_page = url.contains("settings.html") || url.contains("welcome.html");
    let review = viewer && url.contains("#review");
    let graph = viewer && !review;
    let current = has("keysDlg");
    let opens = |control: &str| app_page || review || (graph && has(control));
    vec![
        ("view-filters", opens("panelL")),
        ("view-log", opens("panelR")),
        ("view-map", opens("mapTab")),
        ("view-list", opens("listBtn")),
        ("view-fit", opens("fit")),
        ("view-keys", opens("keysOpen")),
        ("edit-find", app_page || viewer),
        ("view-back", app_page || (viewer && current)),
        ("view-forward", graph && current),
    ]
}

fn enable_map_items(app: &tauri::AppHandle, report: &serde_json::Value) {
    let url = report["url"].as_str().unwrap_or("");
    let controls: Vec<String> = report["controls"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if let Some(items) = app.try_state::<MapItems>() {
        for (id, on) in map_items_enabled(url, &controls) {
            if let Some((_, item)) = items.0.iter().find(|(k, _)| *k == id) {
                let _ = item.set_enabled(on);
            }
        }
    }
}

/// The script a map command runs in the viewer (design-system.md Section 29.13): it presses the page's
/// own control, only when the press changes something and no dialog or card is open, so a command and
/// its button do one thing.
fn press_js(id: &str, only_if_unselected: bool) -> String {
    let id = serde_json::to_string(id).unwrap_or_default();
    format!(
        r##"(function(){{if(document.querySelector("dialog[open]")||document.querySelector("#obscard.on"))return;var b=document.getElementById({id});if(!b||b.getAttribute("aria-disabled")==="true")return;{guard}b.click();}})();"##,
        guard = if only_if_unselected {
            r#"if(b.getAttribute("aria-selected")==="true")return;"#
        } else {
            ""
        }
    )
}

/// The script that sends the page on screen a Cmd key (Cmd+K, Cmd+[ or Cmd+]) at its focused element,
/// which each page reads as it reads the key; a viewer without the selection history gets no Cmd+[.
fn key_js(key: &str, needs_history: bool) -> String {
    let k = serde_json::to_string(key).unwrap_or_default();
    format!(
        r#"(function(){{{gate}var t=document.activeElement||document.body;t.dispatchEvent(new KeyboardEvent("keydown",{{key:{k},metaKey:true,bubbles:true,cancelable:true}}));}})();"#,
        gate = if needs_history {
            r#"if(/^viz:/.test(location.href)&&!document.getElementById("keysDlg"))return;"#
        } else {
            ""
        }
    )
}

/// The menu bar's map commands (design-system.md Section 29.13): on the Graph page they press the
/// page's own control; on Review, Settings or Setup they open the Graph and do nothing else; Back and
/// Forward send the page on screen its own Cmd+[ and Cmd+].
fn map_command(app: &tauri::AppHandle, id: &str) -> tauri::Result<()> {
    let control = match id {
        "view-filters" => Some(("panelL", false)),
        "view-log" => Some(("panelR", false)),
        "view-map" => Some(("mapTab", true)),
        "view-list" => Some(("listBtn", true)),
        "view-fit" => Some(("fit", false)),
        "view-keys" => Some(("keysOpen", false)),
        "edit-find" | "view-back" | "view-forward" => None,
        _ => return Ok(()),
    };
    let Some(w) = app.get_webview_window("main") else {
        return Ok(());
    };
    if id == "view-back" || id == "view-forward" {
        return w.eval(key_js(if id == "view-back" { "[" } else { "]" }, true));
    }
    let url = w.url().ok();
    let viewer = url.as_ref().is_some_and(is_viewer);
    if !viewer {
        return show_graph(app);
    }
    if url.as_ref().is_some_and(|u| u.fragment() == Some("review")) {
        return w.eval("location.hash = ''");
    }
    match control {
        Some((c, tab)) => w.eval(press_js(c, tab)),
        None => w.eval(key_js("k", false)),
    }
}

/// Opens Settings' This Mac with an act the page reports itself (Section 29.5): `diagnostics` focuses
/// Copy Diagnostics, `reveal-log` shows the log in Finder.
fn show_settings_act(app: &tauri::AppHandle, act: &str) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        let mut url = with_from(settings_url(), current.as_ref());
        url.query_pairs_mut().append_pair("act", act);
        url.set_fragment(Some("daemon"));
        w.navigate(url)?;
    }
    Ok(())
}

/// Opens an address of the project's in the browser; nothing of this Mac's goes with it.
fn open_url(url: &str) -> tauri::Result<()> {
    if let Err(e) = Command::new("open").arg(url).status() {
        tracing::error!(error = %e, url, "could not open the address");
    }
    Ok(())
}

/// Whether no AI app is connected yet, the state a new user is in (docs/client-connect.md Section 5):
/// the tray's act line asks to connect one. And whether any app's row needs the person - an entry
/// other than the bridge, or settings that could not be read - which marks AI apps (Section 29.14).
struct AppsHint(Mutex<(bool, bool)>);

/// The settings section the tray's Settings... opens: the one whose state the status line is
/// pointing at, if any (docs/settings-page.md Section 3.0).
struct Attention(Mutex<Option<&'static str>>);

/// What waits for a person on this Mac's daemon, across its workspaces (docs/design-system.md
/// Section 7): the open and blocked proposals, and with them, from the curation report, the suspected
/// secrets and the contested beliefs - all of Needs you. `needs` is `None` when the curation read
/// failed and the proposal list answered, and the item then says what it counted (P5).
#[derive(Debug, PartialEq)]
struct Waiting {
    proposals: usize,
    needs: Option<usize>,
}

/// One JSON read over the daemon's socket, or `None` when it does not answer in time or well.
async fn read_json(sock: &Path, path: &str) -> Option<serde_json::Value> {
    let f = tokio::time::timeout(Duration::from_secs(5), uds_fetch(sock, path))
        .await
        .ok()?
        .ok()?;
    if f.status != 200 {
        return None;
    }
    serde_json::from_slice(&f.body).ok()
}

/// The tray's count, read once a beat. `None` when the daemon does not answer the proposal list.
async fn waiting(sock: &Path) -> Option<Waiting> {
    let proposals = review_count(&read_json(sock, "/api/proposals?workspace=*").await?);
    let needs = read_json(sock, "/api/curation?workspace=*")
        .await
        .and_then(|c| curation_count(&c))
        .map(|n| n + proposals);
    Some(Waiting { proposals, needs })
}

/// What the curation report adds to Needs you: the suspected secrets and the contested beliefs (an
/// entity's type or a type's definition). `None` for an answer that is not the report.
fn curation_count(report: &serde_json::Value) -> Option<usize> {
    let secrets = report.get("secrets")?.as_array()?.len();
    let contested = report
        .get("contradictions")?
        .as_array()?
        .iter()
        .filter(|c| c["contested"].as_bool() == Some(true))
        .count();
    Some(secrets + contested)
}

/// The item's words, or `None` when nothing waits: "Review - 3 need you...", and when only the
/// proposals could be read, what was counted: "Review - 2 proposals wait...".
fn review_item_text(w: &Waiting) -> Option<String> {
    match (w.needs, w.proposals) {
        (Some(0), _) | (None, 0) => None,
        (Some(1), _) => Some("Review - 1 needs you...".into()),
        (Some(n), _) => Some(format!("Review - {n} need you...")),
        (None, 1) => Some("Review - 1 proposal waits...".into()),
        (None, n) => Some(format!("Review - {n} proposals wait...")),
    }
}

/// The count `waiting_proposals` reports, from the daemon's proposal list.
fn review_count(list: &serde_json::Value) -> usize {
    list.as_array().map_or(0, |a| {
        a.iter()
            .filter(|p| matches!(p["state"].as_str(), Some("open" | "blocked")))
            .count()
    })
}

/// The apps `supragnosis connect` knows, by id and display name. Listed here only to build the menu
/// before the CLI has answered; what each one's state is comes from `connect --json`, and an id the
/// CLI does not report is shown as needing a newer CLI.
const AI_APPS: &[(&str, &str)] = &[
    ("claude-desktop", "Claude Desktop"),
    ("claude-code", "Claude Code"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("codex", "Codex"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
];

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// Viewer socket path - the same resolution the CLI uses (SUPRAGNOSIS_VIZ_SOCK -> default), so the
/// shell and a `supragnosis start` daemon land on the same socket without configuration.
fn viz_sock() -> PathBuf {
    env_nonempty("SUPRAGNOSIS_VIZ_SOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".supragnosis/viz.sock"))
}

/// Locates the `supragnosis` server binary: explicit override -> the Homebrew install locations
/// -> PATH -> the legacy install.sh location -> dev build. Two lessons are baked into this order:
/// a Finder-launched .app does NOT inherit the shell's PATH (no /opt/homebrew/bin), so the brew
/// prefixes are probed as literal paths; and ~/.local/bin may hold a STALE pre-brew binary from
/// scripts/install.sh - preferring it over brew once shipped an ancient binary that did not know
/// the `serve` subcommand, fell back to stdio, and died instantly - so it is the LAST real
/// candidate. Shipping a bundled sidecar remains out (deploy/homebrew/README.md: no sidecar).
fn find_server_bin() -> Option<PathBuf> {
    if let Some(p) = env_nonempty("SUPRAGNOSIS_BIN") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    for brew in ["/opt/homebrew/bin/supragnosis", "/usr/local/bin/supragnosis"] {
        let p = PathBuf::from(brew);
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(p) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("supragnosis"))
            .find(|c| c.exists())
    }) {
        return Some(p);
    }
    let legacy = home().join(".local/bin/supragnosis");
    if legacy.exists() {
        return Some(legacy);
    }
    if cfg!(debug_assertions) {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/supragnosis");
        if dev.exists() {
            return Some(dev);
        }
    }
    None
}

/// MCP bind address for a daemon the shell spawns: explicit env wins; otherwise the canonical
/// :7373 when free, else an ephemeral port (a foreign holder of :7373 must not kill the daemon -
/// the viewer works regardless, and agents can still be pointed at the logged port).
fn mcp_addr() -> String {
    if let Some(v) = env_nonempty("SUPRAGNOSIS_HTTP_ADDR") {
        return v;
    }
    match std::net::TcpListener::bind("127.0.0.1:7373") {
        Ok(probe) => {
            drop(probe);
            "127.0.0.1:7373".to_string()
        }
        Err(_) => "127.0.0.1:0".to_string(),
    }
}

/// Attach-or-spawn. A live socket means an externally managed daemon - attach and own nothing.
/// Otherwise spawn `supragnosis serve` (logs to ~/.supragnosis/log/app-daemon.*.log) and wait
/// briefly for its socket; the viz:// proxy serves a retry page until it answers, so a slow start
/// is not fatal.
async fn ensure_daemon(sock: &Path) -> anyhow::Result<Option<Child>> {
    if UnixStream::connect(sock).await.is_ok() {
        tracing::info!(sock = %sock.display(), "attached to a running daemon");
        return Ok(None);
    }
    let bin = find_server_bin().context(
        "supragnosis server binary not found - set SUPRAGNOSIS_BIN, or install it to ~/.local/bin \
         (scripts/install.sh)",
    )?;
    let base = home().join(".supragnosis");
    // Created closed to other accounts when this is the first thing to create it - the store and the
    // token will live here. An existing directory is the daemon's to correct, which it does on start.
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&base)
            .context("failed to create ~/.supragnosis")?;
    }
    let logs = base.join("log");
    std::fs::create_dir_all(&logs).context("failed to create the daemon log dir")?;
    let out = std::fs::File::create(logs.join("app-daemon.out.log"))?;
    let err = std::fs::File::create(logs.join("app-daemon.err.log"))?;
    let http_addr = mcp_addr();
    tracing::info!(bin = %bin.display(), http = %http_addr, sock = %sock.display(), "spawning the daemon");
    let mut child = Command::new(&bin)
        .args(["serve", "--http", &http_addr, "--viz"])
        .arg(sock)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .with_context(|| format!("failed to spawn {}", bin.display()))?;
    let mut socket_up = false;
    for _ in 0..40 {
        // A dead child will never bind the socket - detect it NOW and surface the reason,
        // instead of reporting "running (spawned)" against a corpse and letting the splash
        // refresh forever (the exact failure mode of spawning a stale pre-`serve` binary).
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(logs.join("app-daemon.err.log"))
                .ok()
                .map(|s| {
                    s.lines()
                        .rev()
                        .take(3)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "no stderr output".to_string());
            anyhow::bail!(
                "the spawned daemon exited immediately ({status}, binary {}): {tail} - full log: \
                 ~/.supragnosis/log/app-daemon.err.log",
                bin.display()
            );
        }
        if UnixStream::connect(sock).await.is_ok() {
            tracing::info!("daemon is up");
            socket_up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if !socket_up {
        // Still starting (a cold fastembed init can outlast the wait) - the splash keeps
        // retrying, but leave a trace so a hang has a log line to find.
        tracing::warn!(sock = %sock.display(), "daemon spawned but the viewer socket is not answering yet");
    }
    Ok(Some(child))
}

/// Served at `/` while the daemon's socket is not answering yet; refreshes itself into the real
/// viewer once it is. Carries the LIVE daemon state so a failure is readable on the splash
/// instead of an eternal "starting..." (the tray shows the same line, but the splash is what
/// the user is looking at). Palette mirrors the viewer's candlelight theme.
// data-tauri-drag-region="deep": with the overlay title bar there is no other chrome to drag the
// window by while the splash is up, and its text covers the middle, which the bare attribute would
// leave undraggable.
/// What the starting page says (docs/design-system.md Section 29.5): one of three things, from the
/// shell's state and how long the start it made has run. The page holds no timer; the shell reloads
/// it when this changes, or when the daemon answers.
#[derive(Debug, Clone, PartialEq)]
enum StartPage {
    /// The first 10 seconds of a start the shell made.
    Starting,
    /// A start past 10 seconds, or a daemon the shell holds that has not answered yet.
    StillStarting,
    /// The start failed, or the child the app spawned exited: the cause, in one sentence.
    Failed(String),
    /// A daemon the app attached to stopped answering, and nothing is starting it (Section 9's
    /// "Stopped", since step 45).
    Stopped,
}

/// When the shell last began a start - at launch or for a restart - for "Still starting".
static START_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

fn mark_start() {
    *START_AT.lock().unwrap() = Some(std::time::Instant::now());
}

/// The page for the shell's state, `since` the start it made; it is drawn only while the socket is
/// silent. A child the app spawned that is alive is still starting - the app stops waiting on it at
/// 10 seconds, and a cold start can take longer - and one that exited is not; a daemon the app
/// attached to that went silent stopped answering.
fn start_page(d: &mut Daemon, since: Option<Duration>) -> StartPage {
    match d {
        Daemon::Failed(why) => StartPage::Failed(cause_of(why)),
        Daemon::Starting if since.is_none_or(|t| t >= Duration::from_secs(10)) => {
            StartPage::StillStarting
        }
        Daemon::Starting => StartPage::Starting,
        Daemon::Spawned(child) => match child.try_wait() {
            Ok(None) => StartPage::StillStarting,
            _ => StartPage::Failed("The daemon exited.".to_string()),
        },
        Daemon::External => StartPage::Stopped,
    }
}

/// A child the app spawned that has since exited is a failed start - Section 9's "Could not start" -
/// for whatever reads the state next: the tray, Settings and the starting page say the same.
fn settle_exited(d: &mut Daemon) {
    let exited = match d {
        Daemon::Spawned(c) => match c.try_wait() {
            Ok(Some(status)) => Some(format!("the daemon exited ({status})")),
            Ok(None) => None,
            Err(e) => Some(format!("the daemon could not be watched: {e}")),
        },
        _ => None,
    };
    if let Some(why) = exited {
        *d = Daemon::Failed(why);
    }
}

/// A failure's cause in one sentence: the shell's reason up to " - full log:", its first line,
/// begun with a capital and ended with a stop.
fn cause_of(why: &str) -> String {
    let cut = why.split(" - full log:").next().unwrap_or(why);
    let line = cut.lines().next().unwrap_or("").trim().trim_end_matches('.');
    let mut c = line.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
        None => "The daemon did not start.".to_string(),
    }
}

/// The daemon's error log: the one written last of the app's (`app-daemon.err.log`, when the app
/// spawns the daemon) and a login job's (`supragnosis.err.log`).
fn daemon_log() -> PathBuf {
    let dir = home().join(".supragnosis/log");
    let (app, job) = (dir.join("app-daemon.err.log"), dir.join("supragnosis.err.log"));
    let when = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (when(&app), when(&job)) {
        (Some(a), Some(j)) if j > a => job,
        (None, Some(_)) => job,
        _ => app,
    }
}

/// The path as a person types it: under the home directory, with `~`.
fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn starting_html(
    page: &StartPage,
    conflict: Option<&Conflict>,
    settings: &tauri::Url,
    log: &Path,
) -> String {
    let esc = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    // A link that reads as a button: the page has no script (SHELL_CSP), and its acts open the app's
    // own page, where the commands are (docs/settings-page.md Section 3.3, S1).
    const BUTTON: &str = "display:inline-block;margin:18px 4px 0;padding:7px 16px;border-radius:8px;font-weight:600;text-decoration:none";
    let link = |act: Option<&str>| {
        let mut to = settings.clone();
        to.set_query(act.map(|a| format!("act={a}")).as_deref());
        to.set_fragment(Some("daemon"));
        esc(to.as_str())
    };
    let primary = |href: String, label: &str| {
        format!(
            r#"<a href="{href}" autofocus style="{BUTTON};background:#d9a544;color:#16181e">{label}</a>"#
        )
    };
    let plain = |href: String, label: &str| {
        format!(
            r#"<a class="b2" href="{href}" style="{BUTTON};border:1px solid #3a3f4a;color:#e9e4d6">{label}</a>"#
        )
    };
    // The log's path in a read-only field: selectable and copyable inside a page that is a drag
    // region, since a field is not dragged.
    let log_field = format!(
        r#"<div style="margin-top:16px"><input class="lf" readonly value="{}" aria-label="The daemon's log" style="width:min(36em,90vw);font:12px/1.4 'IBM Plex Mono',ui-monospace,monospace;background:transparent;color:inherit;border:1px solid #3a3f4a;border-radius:6px;padding:5px 8px"></div>"#,
        esc(&tilde(log))
    );
    let (head, detail, action, foot) = match (conflict, page) {
        // The daemon cannot open its store for its model (semantic-retrieval.md Section 13): the
        // reason, and Rebuild index, which opens Settings to start it.
        (Some((recorded, daemon)), _) => (
            "The search index needs rebuilding".to_string(),
            format!(
                "Indexed by {}, and the daemon runs {} - it cannot start until the index is rebuilt.",
                esc(recorded),
                esc(daemon)
            ),
            primary(link(Some("rebuild")), "Rebuild index"),
            "or in a terminal: supragnosis embed rebuild".to_string(),
        ),
        (None, StartPage::Starting) => (
            r#"<span class="spin" aria-hidden="true"></span>Starting Supragnosis..."#.to_string(),
            String::new(),
            String::new(),
            String::new(),
        ),
        (None, StartPage::StillStarting) => (
            r#"<span class="spin" aria-hidden="true"></span>Starting Supragnosis..."#.to_string(),
            "Still starting.".to_string(),
            log_field.clone() + &plain(link(None), "Open This Mac"),
            String::new(),
        ),
        (None, page @ (StartPage::Failed(_) | StartPage::Stopped)) => (
            match page {
                StartPage::Stopped => "Supragnosis stopped".to_string(),
                _ => "Supragnosis could not start".to_string(),
            },
            match page {
                StartPage::Failed(cause) => esc(cause),
                _ => "It stopped answering, and nothing is starting it.".to_string(),
            },
            [
                primary(link(Some("restart")), "Restart"),
                plain(link(Some("diagnostics")), "Diagnostics"),
                plain(link(None), "Open This Mac"),
            ]
            .concat(),
            log_field.clone() + &format!(
                r#"<a class="m" href="{}" style="display:inline-block;margin-top:6px;color:#b0b8c7">Show in Finder</a>"#,
                link(Some("reveal-log"))
            ),
        ),
    };
    format!(
        r#"<!doctype html><meta charset="utf-8"><title>Supragnosis</title><style>@media (prefers-color-scheme: light) {{ body {{ background:#fcfbf8 !important; color:#16181e !important }} .m {{ color:#5c6370 !important }} .b2, .lf {{ color:#16181e !important; border-color:#c9ccd3 !important }} }} .spin {{ display:inline-block;width:12px;height:12px;margin-right:8px;vertical-align:-1px;border:2px solid #b0b8c7;border-top-color:transparent;border-radius:50%;animation:s 1s linear infinite }} @keyframes s {{ to {{ transform:rotate(360deg) }} }} @media (prefers-reduced-motion: reduce) {{ .spin {{ animation:none }} }}</style><body data-tauri-drag-region="deep" style="background:#08090d;color:#e9e4d6;font:13px/1.5 'Pretendard Variable',Pretendard,system-ui,-apple-system,'Segoe UI',sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center;max-width:80%"><div>{head}</div><div class="m" style="color:#b0b8c7;margin-top:10px;font-size:12px;word-break:break-word">{detail}</div>{action}<div class="m" style="color:#b0b8c7;margin-top:14px;font-size:12px">{foot}</div></div></body>"#
    )
}

/// The starting page the window shows, as drawn. While one is drawn the shell checks every second,
/// and reloads the window when the daemon answers - the graph then - or when the page should say
/// something else (docs/design-system.md Section 29.5): the page itself has no timer.
static SPLASH: Mutex<Option<String>> = Mutex::new(None);

fn splash_drawn(app: &tauri::AppHandle, sock: PathBuf, key: String) {
    let watching = SPLASH.lock().unwrap().replace(key).is_some();
    if !watching {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { watch_splash(app, sock).await });
    }
}

async fn watch_splash(app: tauri::AppHandle, sock: PathBuf) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(drawn) = SPLASH.lock().unwrap().clone() else {
            return;
        };
        let answering = UnixStream::connect(&sock).await.is_ok();
        heal_failed(&app).await;
        let now = splash_key(&app, splash_conflict().await.as_ref());
        if answering || now != drawn {
            *SPLASH.lock().unwrap() = None;
            reload_viewer(&app);
            return;
        }
    }
}

/// What the starting page would say now, as the key `SPLASH` keeps.
fn splash_key(app: &tauri::AppHandle, conflict: Option<&Conflict>) -> String {
    let since = START_AT.lock().unwrap().map(|t| t.elapsed());
    let page = start_page(&mut app.state::<DaemonGuard>().0.lock().unwrap(), since);
    format!("{page:?}|{conflict:?}")
}

/// The store the daemon cannot open, for the starting page (docs/settings-page.md Section 3.3): the
/// model it was indexed by and the one the daemon runs, from `status --json`. Read at most every five
/// seconds, since the page asks again every second.
async fn splash_conflict() -> Option<Conflict> {
    static LAST: Mutex<Option<(std::time::Instant, Option<Conflict>)>> = Mutex::new(None);
    if let Some((at, c)) = LAST.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(5) {
            return c;
        }
    }
    let st = tokio::task::spawn_blocking(cli_status).await.ok().flatten();
    let c = st.as_ref().and_then(conflict_of);
    *LAST.lock().unwrap() = Some((std::time::Instant::now(), c.clone()));
    c
}

/// The model a store was indexed by and the one the daemon runs.
type Conflict = (String, String);

/// `status --json`'s `embedder_conflict` as its two models, or `None`.
fn conflict_of(st: &serde_json::Value) -> Option<Conflict> {
    let e = &st["embedder_conflict"];
    Some((e["recorded"].as_str()?.to_string(), e["daemon"].as_str()?.to_string()))
}

/// Runs attach-or-spawn and publishes the outcome (state + tray status line).
async fn bring_up(app: tauri::AppHandle, sock: PathBuf) {
    mark_start();
    refresh_status(&app).await;
    let state = match ensure_daemon(&sock).await {
        Ok(Some(child)) => Daemon::Spawned(child),
        Ok(None) => Daemon::External,
        Err(e) => {
            tracing::error!(error = %e, "daemon startup failed - the viewer will keep retrying");
            Daemon::Failed(e.to_string())
        }
    };
    *app.state::<DaemonGuard>().0.lock().unwrap() = state;
    refresh_status(&app).await;
}

/// The tray's status line, from what the CLI observed plus the shell's own relationship to the
/// daemon (a child it spawned is invisible to the CLI - no pidfile, no launchd job).
/// Who runs the daemon, in the words the status line and the settings page use - including a child
/// of this app, which the CLI cannot see (no pidfile, no launchd job).
fn who_runs(daemon: &Daemon, st: &serde_json::Value) -> String {
    match (daemon, st["managers"].get(0)) {
        (Daemon::Spawned(_), _) => "while this app is open".to_string(),
        (_, Some(m)) => match (m["type"].as_str(), m["source"].as_str()) {
            (_, Some("canonical")) => "starts at login".to_string(),
            (Some("launchd"), Some("homebrew")) => "Homebrew's service".to_string(),
            (Some("pidfile"), _) => "supragnosis start".to_string(),
            _ => "started outside this app".to_string(),
        },
        (_, None) => "started outside this app".to_string(),
    }
}

fn status_text(daemon: &Daemon, st: Option<&serde_json::Value>) -> String {
    match (daemon, st) {
        (Daemon::Starting | Daemon::Failed(_), _) | (_, None) => daemon.status_line(),
        // The app's child, alive and not answering yet (an exited one is settled to Failed first).
        (Daemon::Spawned(_), Some(st)) if st["answering"].as_bool() == Some(false) => {
            "Starting...".to_string()
        }
        (_, Some(st)) => {
            let situation = st["situation"].as_str().unwrap_or("");
            let running = st["version"]["running"].as_str();
            let here = st["version"]["here"].as_str().unwrap_or("?");
            if situation == "conflict" {
                "Two managers run Supragnosis".to_string()
            } else if conflict_of(st).is_some() {
                "The search index needs rebuilding".to_string()
            } else {
                let who = who_runs(daemon, st);
                match running {
                    // Both versions, as daemon-lifecycle.md L3 has the tray say, and where to act
                    // (settings-page.md Section 5).
                    // The state word, then both versions (design-system.md Section 29.13); the act
                    // is the tray's next line.
                    Some(r) if r != here => {
                        format!("Update ready - restart: {r} running, {here} installed")
                    }
                    Some(r) => format!("Running {r} - {who}"),
                    None if st["answering"].as_bool() == Some(false) => "Stopped".to_string(),
                    None => format!("Running - {who}"),
                }
            }
        }
    }
}

/// While the memory the AI apps use cannot be used (design-system.md Section 29.13, the author's
/// list): a daemon that could not start, stopped or is claimed by two managers, or a search index
/// that needs rebuilding. A daemon starting, an update ready or an unknown state is not trouble.
fn daemon_trouble(daemon: &Daemon, st: Option<&serde_json::Value>) -> bool {
    match (daemon, st) {
        (Daemon::Failed(_), _) => true,
        (Daemon::Starting, _) | (_, None) => false,
        (Daemon::Spawned(_), Some(st)) if st["answering"].as_bool() == Some(false) => false,
        (_, Some(st)) => {
            st["situation"].as_str() == Some("conflict")
                || conflict_of(st).is_some()
                || (st["version"]["running"].is_null() && st["answering"].as_bool() == Some(false))
        }
    }
}

/// What the tray says (design-system.md Section 29.13): the state line; the one act that does what
/// needs doing, with the Settings section it opens; whether the memory cannot be used; Finish
/// Setup... and the review item; and whether Quit stops the memory.
#[derive(Debug, Clone, PartialEq, Default)]
struct TrayState {
    line: String,
    act: Option<(&'static str, &'static str)>,
    trouble: bool,
    finish: bool,
    review: Option<String>,
    quit_stops: bool,
}

/// The section the tray's Settings... opens: the first whose dot shows, in the sidebar's order - AI
/// apps, then This Mac - or none, which reopens the last pane (design-system.md Section 29.14).
fn attention_section(
    act: Option<&'static str>,
    apps: bool,
    repairing: bool,
) -> Option<&'static str> {
    if apps {
        Some("apps")
    } else if act == Some("daemon") || repairing {
        Some("daemon")
    } else {
        act
    }
}

/// The act line for what needs doing, the first that holds: the memory cannot be used, an update is
/// ready, then no AI app connected - which Finish Setup... covers while it shows.
fn tray_act(
    trouble: bool,
    drift: bool,
    no_app: bool,
    finish: bool,
) -> Option<(&'static str, &'static str)> {
    if trouble || drift {
        Some(("Open This Mac...", "daemon"))
    } else if no_app && !finish {
        Some(("Connect an AI App...", "apps"))
    } else {
        None
    }
}

/// The tray's line when the CLI answers in a schema this app does not read.
const CLI_NEWER_LINE: &str = "supragnosis-server is newer than this app - update the app";

/// Re-reads the CLI's view and republishes the tray's one line (docs/settings-page.md Section 5).
async fn refresh_status(app: &tauri::AppHandle) {
    heal_failed(app).await;
    refresh_apps(app).await;
    let (no_app, apps_need) =
        app.try_state::<AppsHint>().map_or((false, false), |h| *h.0.lock().unwrap());
    // "Finish Setup..." while the daemon or the AI apps are not set up (docs/onboarding.md
    // Section 6), from setup's own checklist. A CLI that predates setup offers nothing.
    let pid = spawned_pid(app);
    let steps = tokio::task::spawn_blocking(move || setup_check_blocking(pid))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|events| essentials(&events));
    let finish = finish_setup_offered(read_prefs(app), steps);
    let mut tray = TrayState { finish, ..TrayState::default() };
    let mut repairing = false;
    let st = tokio::task::spawn_blocking(cli_status).await.ok().flatten();
    if CLI_NEWER.load(std::sync::atomic::Ordering::Relaxed) {
        // Said on the state line, with no act (design-system.md Section 29.13).
        tray.line = CLI_NEWER_LINE.to_string();
    } else {
        let drift = st.as_ref().is_some_and(|v| {
            let running = v["version"]["running"].as_str();
            running.is_some() && running != v["version"]["here"].as_str()
        });
        let guard = app.state::<DaemonGuard>();
        let mut daemon = guard.0.lock().unwrap();
        settle_exited(&mut daemon);
        tray.line = status_text(&daemon, st.as_ref());
        repairing = st
            .as_ref()
            .is_some_and(|v| v["store"]["owed_projections"].as_u64().unwrap_or(0) > 0);
        tray.trouble = daemon_trouble(&daemon, st.as_ref());
        tray.quit_stops = matches!(*daemon, Daemon::Spawned(_));
        tray.act = tray_act(tray.trouble, drift, no_app, finish);
    }
    tray.review = waiting(&app.state::<VizSock>().0).await.as_ref().and_then(review_item_text);
    // Settings... opens the first section whose dot shows - AI apps, This Mac - and the last
    // pane while none does (design-system.md Section 29.14).
    let attention = attention_section(tray.act.map(|(_, s)| s), no_app || apps_need, repairing);
    if let Some(a) = app.try_state::<Attention>() {
        *a.0.lock().unwrap() = attention;
    }
    apply_tray(app, &tray);
}

/// The tray's menu and icon, and the app menu's Quit, from what the tray says (design-system.md
/// Section 29.13). Built again only when it changed, in one fixed order: the state line, the act,
/// Finish Setup..., the review item, Open Graph, Settings..., Quit.
fn apply_tray(app: &tauri::AppHandle, t: &TrayState) {
    let Some(kept) = app.try_state::<TrayKept>() else {
        return;
    };
    {
        let mut last = kept.0.lock().unwrap();
        if last.as_ref() == Some(t) {
            return;
        }
        *last = Some(t.clone());
    }
    let name = app.package_info().name.clone();
    let quit = if t.quit_stops {
        "Quit and Stop Memory".to_string()
    } else {
        format!("Quit {name}")
    };
    let built = (|| -> tauri::Result<Menu<tauri::Wry>> {
        let menu = Menu::new(app)?;
        menu.append(&MenuItem::with_id(app, "tray-status", &t.line, false, None::<&str>)?)?;
        if let Some((words, _)) = t.act {
            menu.append(&MenuItem::with_id(app, "tray-act", words, true, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        if t.finish {
            menu.append(&MenuItem::with_id(
                app,
                "tray-finish-setup",
                "Finish Setup...",
                true,
                None::<&str>,
            )?)?;
        }
        if let Some(r) = &t.review {
            menu.append(&MenuItem::with_id(app, "tray-review", r, true, None::<&str>)?)?;
        }
        menu.append(&MenuItem::with_id(app, "tray-open", "Open Graph", true, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "tray-settings", "Settings...", true, None::<&str>)?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(app, "tray-quit", &quit, true, None::<&str>)?)?;
        Ok(menu)
    })();
    if let (Ok(menu), Some(tray)) = (built, app.tray_by_id("supragnosis")) {
        let _ = tray.set_menu(Some(menu));
        // The state as shape, not color (D14): the mark, or the mark with a dot cut beside it.
        let icon: &[u8] = if t.trouble {
            include_bytes!("../icons/tray-trouble.png")
        } else {
            include_bytes!("../icons/tray.png")
        };
        if let Ok(img) = Image::from_bytes(icon) {
            let _ = tray.set_icon(Some(img));
            let _ = tray.set_icon_as_template(true);
        }
        let tip = if t.trouble { format!("{name} - {}", t.line) } else { name.clone() };
        let _ = tray.set_tooltip(Some(tip));
    }
    // The app menu's Quit takes the tray's words (D15, the author's decision).
    if let Some(q) = app.try_state::<AppQuit>() {
        let _ = q.0.set_text(&quit);
    }
}

/// `supragnosis connect --json`. `None` when the CLI predates it.
fn cli_connect_list() -> Option<serde_json::Value> {
    run_cli_json(&["connect", "--json"])
}

/// One AI app's row on the settings page, from the CLI's report on it: what state it is in, the
/// label of its one button - `None` where a click could do nothing (not installed, settings
/// unreadable, a CLI too old to know `connect`) - and the kind of state, which picks the row's pill:
/// `connected`, `attention` (connected some other way, which a click replaces), `off` or `absent`.
/// Any entry other than the bridge is named, because the person clicking is about to replace it.
fn app_row(report: Option<&serde_json::Value>) -> (String, Option<&'static str>, &'static str) {
    let Some(r) = report else {
        return ("needs a newer supragnosis-server".into(), None, "absent");
    };
    if !r["installed"].as_bool().unwrap_or(false) {
        return ("not installed".into(), None, "absent");
    }
    match r["entry"].as_str().unwrap_or("none") {
        "bridge" => {
            // Claude Code's row also says whether the capture hook is in (client-connect.md 12.4).
            let capture = match r["capture"].as_str() {
                Some("on") => ", capture on",
                Some("off") => ", capture off",
                _ => "",
            };
            (
                format!("connected through the bridge{capture}"),
                Some("Disconnect"),
                "connected",
            )
        }
        "http" => (
            "connected over HTTP, with a copy of the token in its settings".into(),
            Some("Switch to the bridge"),
            "attention",
        ),
        "stdio" => (
            "connected by an old stdio entry".into(),
            Some("Switch to the bridge"),
            "attention",
        ),
        "other" => (
            "another Supragnosis entry is configured".into(),
            Some("Switch to the bridge"),
            "attention",
        ),
        "unknown" => ("its settings file could not be read".into(), None, "absent"),
        _ => ("not connected".into(), Some("Connect"), "off"),
    }
}

/// Re-reads which AI apps are connected, for the status line's hint while none is.
async fn refresh_apps(app: &tauri::AppHandle) {
    let list = tokio::task::spawn_blocking(cli_connect_list).await.ok().flatten();
    let rows = list.as_ref().and_then(|l| l["clients"].as_array());
    let any =
        rows.is_some_and(|rs| rs.iter().any(|r| r["entry"].as_str().is_some_and(|e| e != "none")));
    let needs = rows.is_some_and(|rs| {
        rs.iter().any(|r| {
            r["installed"].as_bool() == Some(true)
                && r["entry"].as_str().is_some_and(|e| e != "none" && e != "bridge")
        })
    });
    if let Some(hint) = app.try_state::<AppsHint>() {
        // A listing that could not be read needs the person too (P5).
        *hint.0.lock().unwrap() = (list.is_some() && !any, list.is_none() || needs);
    }
}

/// Connects an AI app through the bridge, or disconnects it if it already is. The click is the
/// consent to replace another supragnosis entry (client-connect.md Section 5). The CLI decides and
/// does; the answer is what it said.
/// A failure is an error, so a page never draws it as done (U1).
async fn toggle_app(app: &tauri::AppHandle, id: &str) -> Result<String, String> {
    let name = AI_APPS
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .unwrap_or(id)
        .to_string();
    let wanted = id.to_string();
    let connected = tokio::task::spawn_blocking(cli_connect_list)
        .await
        .ok()
        .flatten()
        .and_then(|l| {
            l["clients"]
                .as_array()?
                .iter()
                .find(|r| r["id"].as_str() == Some(&wanted))
                .cloned()
        })
        .is_some_and(|r| r["entry"].as_str() == Some("bridge"));
    let flag = if connected { "--remove" } else { "--replace" };
    let args = ["connect".to_string(), id.to_string(), flag.to_string()];
    let out = tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&args)
    })
    .await
    .ok()
    .flatten();
    let message = match out {
        None => Err(format!("{name}: the supragnosis command was not found")),
        Some(o) if o.status.success() && connected => Ok(format!("{name} disconnected")),
        Some(o) if o.status.success() => {
            // The CLI's "next" line is the one thing the person still has to do.
            let next = String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.trim().strip_prefix("next").map(|n| n.trim().to_string()));
            Ok(match next {
                Some(n) => format!("{name} connected - {n}"),
                None => format!("{name} connected"),
            })
        }
        Some(o) => Err(format!("connecting {name} failed: {}", cli_outcome(&o).1)),
    };
    refresh_status(app).await;
    message
}

/// Restart: bounce whatever we manage, then attach-or-spawn again. A spawned child is killed
/// directly; an external daemon is bounced through the CLI (`supragnosis restart` knows every
/// manager the product installs). A refused restart (a conflict, an unrecognized holder) says why,
/// instead of the shell quietly re-attaching to the process it failed to replace (L4).
async fn restart_daemon(app: &tauri::AppHandle, sock: PathBuf) -> Result<String, String> {
    // A failed daemon that something manages is restarted through that manager, as an attached one
    // is: the shell spawns none of its own beside it (docs/daemon-lifecycle.md Section 6,
    // design-system.md Section 29.5).
    let failed = matches!(*app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Failed(_));
    let job = failed
        && tokio::task::spawn_blocking(cli_status)
            .await
            .ok()
            .flatten()
            .as_ref()
            .is_some_and(managed);
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    mark_start();
    refresh_status(app).await;
    let outcome = match prev {
        Daemon::Spawned(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok("restarted the daemon this app runs".to_string())
        }
        prev @ (Daemon::External | Daemon::Failed(_))
            if matches!(prev, Daemon::External) || job =>
        {
            let before = tokio::task::spawn_blocking(cli_status).await.ok().flatten();
            let out = tokio::task::spawn_blocking(|| run_cli(&["restart"])).await.ok().flatten();
            // launchd may still be starting the new process when the CLI returns, or retrying one
            // that lost the store lock to the old process while it exited, and the CLI then reports
            // a failure that a few seconds settle. Wait for the socket, as set_login does, so the
            // shell attaches to that daemon rather than spawning a second one into its lock.
            let up = wait_for_socket(&sock, Duration::from_secs(30)).await;
            let after = if up {
                tokio::task::spawn_blocking(cli_status).await.ok().flatten()
            } else {
                None
            };
            let outcome = restart_outcome(
                out.as_ref().map(cli_outcome),
                manager_pid(before.as_ref()),
                manager_pid(after.as_ref()),
                up,
            );
            if !up {
                // A login job owns this daemon: spawning one here would race it for the store. The
                // state says so until the socket answers, and heal_failed attaches then.
                *app.state::<DaemonGuard>().0.lock().unwrap() = Daemon::Failed(
                    "the daemon did not answer within 30 seconds of the restart - `supragnosis status` says why"
                        .to_string(),
                );
                refresh_status(app).await;
                return outcome;
            }
            outcome
        }
        Daemon::Starting | Daemon::Failed(_) | Daemon::External => {
            Ok("starting the daemon".to_string())
        }
    };
    bring_up(app.clone(), sock).await;
    outcome
}

/// What a restart of a daemon the shell did not spawn came to (docs/daemon-lifecycle.md L4): the
/// CLI's outcome, the running process before and after, and whether the viewer socket answered
/// within the wait. A refusal while a new process took over - launchd retrying a start that lost the
/// store lock for a moment - is a restart; a refusal that left the same process is not.
fn restart_outcome(
    cli: Option<(bool, String)>,
    before: Option<u64>,
    after: Option<u64>,
    up: bool,
) -> Result<String, String> {
    let replaced = after.is_some() && after != before;
    match cli {
        None => Err("the supragnosis CLI was not found".to_string()),
        Some((true, _)) if up => Ok("restarted".to_string()),
        Some((false, _)) if up && replaced => Ok("restarted".to_string()),
        Some((false, why)) if up => Err(format!("restart refused: {why}")),
        Some((ok, why)) => Err(format!(
            "the daemon did not answer within 30 seconds of the restart{} - `supragnosis status` says why",
            if ok || why.is_empty() { String::new() } else { format!(" ({why})") }
        )),
    }
}

/// Whether `status --json` names any manager of the daemon - a login job, a systemd unit, a
/// `supragnosis start` pidfile.
fn managed(st: &serde_json::Value) -> bool {
    st["managers"].as_array().is_some_and(|ms| !ms.is_empty())
}

/// The pid of the manager `status --json` names first, when it has a process.
fn manager_pid(st: Option<&serde_json::Value>) -> Option<u64> {
    st?["managers"].get(0)?["pid"].as_u64()
}

/// A Failed state outlives its cause: a spawn that lost the store lock to a login job still
/// starting, or a restart reported failed while launchd brought the daemon back. Whenever the state
/// is read, a Failed daemon whose socket answers is attached to, so the tray line and the Settings
/// page say what is true instead of the failure that has passed.
async fn heal_failed(app: &tauri::AppHandle) {
    let (Some(guard), Some(sock)) = (app.try_state::<DaemonGuard>(), app.try_state::<VizSock>())
    else {
        return;
    };
    if !matches!(*guard.0.lock().unwrap(), Daemon::Failed(_)) {
        return;
    }
    let sock = sock.0.clone();
    if UnixStream::connect(&sock).await.is_ok() {
        let mut guard = guard.0.lock().unwrap();
        if matches!(*guard, Daemon::Failed(_)) {
            tracing::info!(sock = %sock.display(), "the daemon answers again - attached");
            *guard = Daemon::External;
        }
    }
}

/// Waits for the viewer socket to answer, so a daemon launchd is still starting is attached to
/// rather than raced: attach-or-spawn would otherwise spawn a second daemon into the store lock.
async fn wait_for_socket(sock: &Path, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if UnixStream::connect(sock).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

/// Start at Login: install or uninstall the canonical LaunchAgent through the CLI
/// (docs/daemon-lifecycle.md Section 6). Turning it on is the consent to take over from another
/// manager; the answer says what happened.
async fn set_login(app: &tauri::AppHandle, sock: PathBuf, on: bool) -> Result<String, String> {
    let installed = tokio::task::spawn_blocking(cli_status)
        .await
        .ok()
        .flatten()
        .and_then(|v| v["service"]["state"].as_str().map(|s| s != "absent"))
        .unwrap_or(false);
    if installed == on {
        return Ok(if on { "already starts at login" } else { "already off" }.to_string());
    }
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    refresh_status(app).await;
    let args: &'static [&'static str] = if installed {
        &["service", "uninstall"]
    } else {
        // A daemon this app spawned holds the store, and the CLI cannot see it (no pidfile, no
        // launchd job) - so it goes first, or the new job fails on the lock and KeepAlive retries
        // it forever. The same crash loop docs/daemon-lifecycle.md Section 1 records.
        if let Daemon::Spawned(mut child) = prev {
            let _ = child.kill();
            let _ = child.wait();
        }
        &["service", "install", "--take-over"]
    };
    let out = tokio::task::spawn_blocking(move || run_cli(args)).await.ok().flatten();
    let outcome = login_outcome(installed, out.as_ref());
    if !installed {
        wait_for_socket(&sock, Duration::from_secs(10)).await;
    }
    bring_up(app.clone(), sock).await;
    outcome
}

/// What a Start at Login change came to, from the CLI's exit status (daemon-lifecycle.md L4).
fn login_outcome(installed: bool, out: Option<&std::process::Output>) -> Result<String, String> {
    cli_change(out, |_| {
        if installed { "no longer starts at login" } else { "starts at login" }.to_string()
    })
}

// --- The settings page (docs/settings-page.md) ---------------------------------------------

/// The commands the app's own pages call - the list `generate_handler!` registers below. build.rs
/// declares the same list in the app manifest, so none of them is open to any page by default;
/// capabilities/settings.json grants them to the main window, and each one refuses unless that
/// window is showing one of the app's own pages (S1). A test holds the lists together.
#[cfg(test)]
const APP_COMMANDS: &[&str] = &[
    "settings_state",
    "app_toggle",
    "capture_set",
    "login_set",
    "daemon_restart",
    "search_rebuild",
    "peer_narrow",
    "setup_check",
    "setup_run",
    "setup_stop",
    "setup_running",
    "welcome_state",
    "welcome_set",
    "app_helper",
    "clipboard_copy",
    "diagnostics",
    "reveal",
];

/// The app's own pages: bundled with it, never served by a daemon or a hub, and the only pages its
/// commands answer - Settings (docs/settings-page.md) and Welcome (docs/onboarding.md Section 6).
const OWN_PAGES: &[&str] = &["settings.html", "welcome.html"];

/// The address of one of the app's own pages.
fn own_page(name: &str) -> tauri::Url {
    format!("tauri://localhost/{name}").parse().expect("static url")
}

fn settings_url() -> tauri::Url {
    own_page("settings.html")
}

/// Whether `url` is the viewer's own address - its scheme and host, `viz://localhost`. Only such an
/// address is a way back:
/// anything else a page could put in `?from=` would make "< Graph" a redirect (settings-page.md
/// Section 4, S1; design-system.md Section 29.4).
fn is_viewer(url: &tauri::Url) -> bool {
    let own = graph_url();
    url.scheme() == own.scheme() && url.host_str() == own.host_str()
}

/// The graph as the person left it (design-system.md Section 29.4): the address the window's page
/// carries in `?from=` when it is an app page and that address is the viewer's own; the viewer's
/// address itself, `#review` dropped, when the window shows the viewer; else `None`.
fn graph_left(current: &tauri::Url) -> Option<tauri::Url> {
    let mut back = if is_viewer(current) {
        current.clone()
    } else {
        let from = current.query_pairs().find(|(k, _)| k == "from")?.1.into_owned();
        tauri::Url::parse(&from).ok().filter(is_viewer)?
    };
    back.set_fragment(None);
    Some(back)
}

/// An app page's address carrying the way back to the graph, when there is one.
fn with_from(mut page: tauri::Url, current: Option<&tauri::Url>) -> tauri::Url {
    if let Some(back) = current.and_then(graph_left) {
        page.query_pairs_mut().append_pair("from", back.as_str());
    }
    page
}

fn welcome_url() -> tauri::Url {
    own_page("welcome.html")
}

/// Whether `url` is the app's own page `name`: its scheme, host and path.
fn is_page(url: &tauri::Url, name: &str) -> bool {
    let own = own_page(name);
    url.scheme() == own.scheme() && url.host_str() == own.host_str() && url.path() == own.path()
}

/// Whether a caller may run the app's commands (docs/settings-page.md S1, docs/onboarding.md
/// Section 6): the main window, while its page is one of the app's own. The capability cannot tell
/// the pages of one window apart, since the same window shows the viewer a daemon or a hub serves,
/// so this check is the lock. It reads the webview's top-level address, so a viewer page that frames
/// an app page cannot call through the frame either.
fn own_page_ok(label: &str, url: &tauri::Url) -> bool {
    label == "main" && OWN_PAGES.iter().any(|p| is_page(url, p))
}

fn own_page_caller(webview: &tauri::Webview) -> Result<(), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if own_page_ok(webview.label(), &url) {
        Ok(())
    } else {
        tracing::warn!(label = webview.label(), page = %url, "an app command was refused: not one of the app's own pages");
        Err("only the app's own pages - Settings and Welcome - may change a setting (docs/settings-page.md S1)".into())
    }
}

/// Reloads the viewer if that is what the main window shows - not the settings page, which re-reads
/// its own state after every change.
fn reload_viewer(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.url().is_ok_and(|u| u.scheme() == "viz") {
            let _ = w.eval("location.reload()");
        }
    }
}

/// The socket path, for the commands that bring the daemon back up.
struct VizSock(PathBuf);

/// Everything the settings window shows, gathered from the CLI in one pass.
async fn settings_state_of(app: &tauri::AppHandle) -> serde_json::Value {
    heal_failed(app).await;
    // Both CLI calls run at once; each is a process the other need not wait for.
    let status = tokio::task::spawn_blocking(cli_status);
    let connect = tokio::task::spawn_blocking(cli_connect_list);
    let (status, connect) = (status.await.ok().flatten(), connect.await.ok().flatten());

    let reports = connect.as_ref().and_then(|l| l["clients"].as_array().cloned());
    let apps: Vec<serde_json::Value> = AI_APPS
        .iter()
        .map(|(id, name)| {
            let report = reports
                .as_ref()
                .and_then(|rs| rs.iter().find(|r| r["id"].as_str() == Some(id)).cloned());
            let (state, action, kind) = app_row(report.as_ref());
            // Claude Code's capture hook, while it is connected through the bridge and the listing
            // says (client-connect.md 12.4); null otherwise, and the page then draws no switch.
            let capture = report
                .as_ref()
                .filter(|r| r["entry"].as_str() == Some("bridge"))
                .and_then(|r| r["capture"].as_str().filter(|c| *c == "on" || *c == "off"))
                .map(|c| c == "on");
            serde_json::json!({ "id": id, "name": name, "state": state, "action": action, "kind": kind, "capture": capture })
        })
        .collect();

    // Sync is this Mac's daemon's (docs/settings-page.md Section 3.4): read from its viewer socket.
    let sock = app.state::<VizSock>().0.clone();
    let federation =
        match tokio::time::timeout(Duration::from_secs(5), uds_fetch(&sock, "/api/federation"))
            .await
        {
            Ok(Ok(f)) if f.status == 200 => {
                serde_json::from_slice::<serde_json::Value>(&f.body).ok()
            }
            _ => None,
        };
    let guard = app.state::<DaemonGuard>();
    let (line, who, shell, cause) = {
        let mut daemon = guard.0.lock().unwrap();
        settle_exited(&mut daemon);
        let who = status.as_ref().map(|st| who_runs(&daemon, st));
        // The shell's own state as a field, so no page parses the tray's words (Section 29.6).
        let (shell, cause) = match &*daemon {
            Daemon::Starting => ("starting", None),
            Daemon::Failed(e) => ("failed", Some(cause_of(e))),
            Daemon::Spawned(_) => ("spawned", None),
            Daemon::External => ("attached", None),
        };
        (status_text(&daemon, status.as_ref()), who, shell, cause)
    };
    let login = status
        .as_ref()
        .and_then(|v| v["service"]["state"].as_str())
        .map(|s| s != "absent");
    serde_json::json!({
        "cli": find_server_bin().is_some(),
        "cli_newer": CLI_NEWER.load(std::sync::atomic::Ordering::Relaxed),
        "apps": apps,
        "daemon": {
            "line": line,
            "who": who,
            "shell": shell,
            "cause": cause,
            "situation": status.as_ref().map(|v| v["situation"].clone()),
            "answering": status.as_ref().map(|v| v["answering"].clone()),
            "running": status.as_ref().map(|v| v["version"]["running"].clone()),
            "installed": status.as_ref().map(|v| v["version"]["here"].clone()),
            "store": status.as_ref().map(|v| v["store"].clone()),
            // The daemon's own report on search by meaning (docs/onboarding.md Section 6): null when
            // it runs without a model or does not answer.
            "semantic": status.as_ref().map(|v| v["semantic"].clone()),
            // A store the daemon cannot open for its model (docs/settings-page.md Section 3.3): the
            // two models, or null.
            "embedder_conflict": status.as_ref().map(|v| v["embedder_conflict"].clone()),
            "login": login,
            // The daemon's log, as a person types it (design-system.md Section 29.5).
            "log": tilde(&daemon_log()),
        },
        "federation": federation,
        "about": {
            "app": env!("CARGO_PKG_VERSION"),
            "license": env!("CARGO_PKG_LICENSE"),
            "source": env!("CARGO_PKG_REPOSITORY"),
            "cli": find_server_bin().map(|p| p.display().to_string()),
            "data": home().join(".supragnosis").display().to_string(),
        },
    })
}

#[tauri::command]
async fn settings_state(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    Ok(settings_state_of(&app).await)
}

#[tauri::command]
async fn app_toggle(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    if !AI_APPS.iter().any(|(i, _)| *i == id) {
        return Err(format!("{id:?} is not an app supragnosis connects"));
    }
    toggle_app(&app, &id).await
}

/// The Settings page's capture switch (design-system.md Section 6a): on runs `connect claude-code`,
/// which adds the hook beside the bridge; off runs it with `--no-capture`, which removes the hook and
/// keeps the bridge (client-connect.md Section 12.4). Only for an app already connected through the
/// bridge, which is the only row the page draws the switch on.
#[tauri::command]
async fn capture_set(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    on: bool,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    let mut args = vec!["connect".to_string(), "claude-code".to_string()];
    if !on {
        args.push("--no-capture".into());
    }
    let out = tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_cli(&args)
    })
    .await
    .ok()
    .flatten();
    refresh_status(&app).await;
    match out {
        None => Err("supragnosis CLI not found".into()),
        Some(o) if o.status.success() => Ok(if on {
            "Claude Code asks for the why behind each commit".into()
        } else {
            "Claude Code no longer asks after a commit".into()
        }),
        Some(o) => Err(cli_outcome(&o).1),
    }
}

#[tauri::command]
async fn login_set(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    on: bool,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    // Turning it off is a choice the app remembers, so Welcome and the tray stop asking
    // (docs/onboarding.md Section 4.1); turning it on forgets it.
    let mut p = read_prefs(&app);
    p.login_off = !on;
    let _ = write_prefs(&app, p);
    let sock = app.state::<VizSock>().0.clone();
    set_login(&app, sock, on).await
}

/// Percent-encodes a query value: everything but the unreserved characters.
fn query_value(v: &str) -> String {
    v.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Narrows what one admitted peer may read - the viewer's console act (`POST /api/peer/share`),
/// moved onto the settings page (docs/settings-page.md Section 3.4). Narrow-only, as the daemon
/// enforces: `keep` must be a subset of what the peer holds, and an empty `keep` means "admitted, may
/// read nothing". The daemon writes supragnosis.toml and answers what the peer now holds.
#[tauri::command]
async fn peer_narrow(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    node: String,
    keep: Vec<String>,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    if node.trim().is_empty() {
        return Err("no peer named".into());
    }
    let target = format!(
        "/api/peer/share?node_id={}&workspaces={}",
        query_value(&node),
        query_value(&keep.join(","))
    );
    let sock = app.state::<VizSock>().0.clone();
    let answer =
        tokio::time::timeout(Duration::from_secs(15), uds_request(&sock, "POST", &target, None))
            .await
            .map_err(|_| "the daemon did not answer within 15 seconds".to_string())?
            .map_err(|e| format!("the daemon could not be reached: {e}"))?;
    let body: serde_json::Value = serde_json::from_slice(&answer.body).unwrap_or_default();
    if answer.status != 200 {
        return Err(body["error"].as_str().unwrap_or("the daemon refused the change").to_string());
    }
    let now: Vec<&str> = body["shared_workspaces"]
        .as_array()
        .map(|a| a.iter().filter_map(|w| w.as_str()).collect())
        .unwrap_or_default();
    Ok(if now.is_empty() {
        "it may read nothing now".to_string()
    } else {
        format!("it may read {}", now.join(", "))
    })
}

#[tauri::command]
async fn daemon_restart(webview: tauri::Webview, app: tauri::AppHandle) -> Result<String, String> {
    own_page_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    restart_daemon(&app, sock).await
}

#[tauri::command]
async fn search_rebuild(webview: tauri::Webview, app: tauri::AppHandle) -> Result<String, String> {
    own_page_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    rebuild_index(&app, sock).await
}

/// Rebuild index (docs/settings-page.md Section 3.3, semantic-retrieval.md Section 13): the daemon
/// this app spawned is stopped here, since only the app can stop it; any other is the CLI's, which
/// `supragnosis embed rebuild` stops and starts again. The store is reset either way, and the shell
/// then attaches to the daemon that comes back, or spawns its own where none runs - and that start's
/// backfill embeds every row with the model it runs.
async fn rebuild_index(app: &tauri::AppHandle, sock: PathBuf) -> Result<String, String> {
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    refresh_status(app).await;
    if let Daemon::Spawned(mut child) = prev {
        let _ = child.kill();
        let _ = child.wait();
    }
    let out = tokio::task::spawn_blocking(|| run_cli(&["embed", "rebuild"]))
        .await
        .ok()
        .flatten();
    let outcome = match out.as_ref().map(cli_outcome) {
        Some((true, line)) => Ok(line),
        Some((false, why)) => Err(why),
        None => Err("the supragnosis CLI was not found".to_string()),
    };
    // A login job the CLI started again: wait for its socket, as a restart does, so the shell
    // attaches to it rather than spawning a second daemon into its lock.
    let job = tokio::task::spawn_blocking(cli_status)
        .await
        .ok()
        .flatten()
        .is_some_and(|st| st["service"]["state"].as_str().is_some_and(|s| s != "absent"));
    if outcome.is_ok() && job {
        wait_for_socket(&sock, Duration::from_secs(30)).await;
    }
    bring_up(app.clone(), sock).await;
    outcome
}

// --- The Welcome page (docs/onboarding.md Section 6) ----------------------------------------
// The page draws only what `supragnosis setup --events` says (O4). The shell runs setup for it, passes
// each line on as it arrives, and keeps three things of its own: stopping the daemon it spawned
// before the server step, the two helpers that restart or open an AI app, and what the person chose.

/// What the CLI says when it does not know `setup`: it predates this app's Welcome page.
const SETUP_TOO_OLD: &str =
    "This supragnosis-server predates setup. Update it: brew upgrade supragnosis-server";

/// What the app remembers about the Welcome page, and nothing else (Section 6): that it was shown,
/// that the person chose "Finish later", and that they turned Start at Login off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WelcomePrefs {
    seen: bool,
    later: bool,
    login_off: bool,
}

impl WelcomePrefs {
    fn from_json(v: &serde_json::Value) -> Self {
        let b = |k: &str| v[k].as_bool().unwrap_or(false);
        WelcomePrefs { seen: b("seen"), later: b("later"), login_off: b("login_off") }
    }
    fn to_json(self) -> serde_json::Value {
        serde_json::json!({ "seen": self.seen, "later": self.later, "login_off": self.login_off })
    }
}

fn prefs_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("welcome.json"))
}

/// The remembered choices. A file that is missing or unreadable is a first launch.
fn read_prefs(app: &tauri::AppHandle) -> WelcomePrefs {
    prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| WelcomePrefs::from_json(&v))
        .unwrap_or_default()
}

/// The root's text size the View menu sets (design-system.md Section 10, U21), kept in view.json.
/// Every rem size of the viewer and the app's pages follows the root; the webview itself is not
/// zoomed, so the title bar keeps its height and the traffic lights their place.
struct TextScale(Mutex<f64>);

/// A text size held to 100%-200% in steps of 12.5%.
fn clamp_text_scale(scale: f64) -> f64 {
    (scale.clamp(1.0, 2.0) * 8.0).round() / 8.0
}

/// The next text size: a step up (+1), a step down (-1), or Actual Size (0).
fn step_text_scale(scale: f64, step: i32) -> f64 {
    if step == 0 {
        return 1.0;
    }
    clamp_text_scale(scale + 0.125 * f64::from(step))
}

/// The script that gives a page the root's text size and tells the viewer's map to read it again.
fn text_scale_script(scale: f64) -> String {
    format!(
        "document.documentElement.style.fontSize = \"{}%\"; window.dispatchEvent(new Event(\"rootsize\"));",
        scale * 100.0
    )
}

fn view_prefs_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("view.json"))
}

/// Keeps one of the view preferences - the text's size, the window's frame - beside the others, so
/// writing one never drops another (design-system.md Section 29.14).
fn keep_view_pref(
    app: &tauri::AppHandle,
    key: &str,
    value: serde_json::Value,
) -> std::io::Result<()> {
    let Some(path) = view_prefs_path(app) else {
        return Ok(());
    };
    let mut all = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    all[key] = value;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Replaced whole, through a temporary file, so a write cut short loses neither preference.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, all.to_string())?;
    std::fs::rename(&tmp, &path)
}

/// The window's kept frame - its place and size in logical points - when it is on a screen now.
fn kept_frame(app: &tauri::AppHandle) -> Option<WindowFrame> {
    let v = view_prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())?;
    let f = &v["window"];
    let mut frame = WindowFrame {
        x: f["x"].as_f64()?,
        y: f["y"].as_f64()?,
        w: f["w"].as_f64()?.max(MIN_WINDOW.0),
        h: f["h"].as_f64()?.max(MIN_WINDOW.1),
    };
    let screens: Vec<(f64, f64, f64, f64)> = app
        .available_monitors()
        .ok()?
        .iter()
        .map(|m| {
            let s = m.scale_factor();
            let (p, z) = (m.position(), m.size());
            (
                f64::from(p.x) / s,
                f64::from(p.y) / s,
                f64::from(z.width) / s,
                f64::from(z.height) / s,
            )
        })
        .collect();
    // Its size is cut to the screen it opens on (design-system.md Section 29.14).
    let (_, _, sw, sh) = screens
        .iter()
        .copied()
        .find(|s| frame_on_a_screen(&frame, std::slice::from_ref(s)))?;
    frame.w = frame.w.min(sw).max(MIN_WINDOW.0);
    frame.h = frame.h.min(sh).max(MIN_WINDOW.1);
    Some(frame)
}

/// The window's frame in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowFrame {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// The smallest window: 760 pixels keeps the Graph in its wide layout, so the narrow one stays the
/// browser's (design-system.md Section 29.14).
const MIN_WINDOW: (f64, f64) = (760.0, 480.0);

/// Whether a kept frame's title bar is on one of the screens now, so the window can be found and
/// moved; a place on no screen is forgotten (design-system.md Section 29.14).
fn frame_on_a_screen(f: &WindowFrame, screens: &[(f64, f64, f64, f64)]) -> bool {
    let (cx, cy) = (f.x + f.w / 2.0, f.y + 12.0);
    screens
        .iter()
        .any(|&(x, y, w, h)| cx >= x && cx <= x + w && cy >= y && cy <= y + h)
}

/// The frame the window has now, kept in memory as it moves and resizes, written when it closes or
/// the app quits.
struct FrameNow(Mutex<Option<WindowFrame>>);

fn note_frame(w: &tauri::Window) {
    let (Ok(p), Ok(z), Ok(s)) = (w.outer_position(), w.inner_size(), w.scale_factor()) else {
        return;
    };
    if w.is_fullscreen().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
        return; // a full-screen or minimized window keeps the frame it had before
    }
    if let Some(now) = w.app_handle().try_state::<FrameNow>() {
        *now.0.lock().unwrap() = Some(WindowFrame {
            x: f64::from(p.x) / s,
            y: f64::from(p.y) / s,
            w: f64::from(z.width) / s,
            h: f64::from(z.height) / s,
        });
    }
}

fn keep_frame(app: &tauri::AppHandle) {
    let Some(f) = app.try_state::<FrameNow>().and_then(|n| *n.0.lock().unwrap()) else {
        return;
    };
    let v = serde_json::json!({ "x": f.x, "y": f.y, "w": f.w, "h": f.h });
    if let Err(e) = keep_view_pref(app, "window", v) {
        tracing::warn!(error = %e, "the window's frame was not kept");
    }
}

/// The kept text size; 100% when the file is missing or unreadable.
fn read_text_scale(app: &tauri::AppHandle) -> f64 {
    view_prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["text_scale"].as_f64())
        .map_or(1.0, clamp_text_scale)
}

/// Steps the text size, keeps it, and gives it to the page on screen.
fn set_text_scale(app: &tauri::AppHandle, step: i32) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TextScale>() else {
        return Ok(());
    };
    let scale = {
        let mut s = state.0.lock().unwrap();
        *s = step_text_scale(*s, step);
        *s
    };
    if let Err(e) = keep_view_pref(app, "text_scale", serde_json::json!(scale)) {
        tracing::warn!(error = %e, "the text size was not kept");
    }
    if let Some(w) = app.get_webview_window("main") {
        w.eval(text_scale_script(scale))?;
    }
    Ok(())
}

fn write_prefs(app: &tauri::AppHandle, p: WelcomePrefs) -> Result<(), String> {
    let path = prefs_path(app).ok_or("the app's settings directory is not known")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, p.to_json().to_string())
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

/// The two steps the launch and the tray wait on (Section 6), from a checklist: whether the daemon is
/// kept running, and whether an AI app is connected. A step that does not apply here counts as done.
fn essentials(events: &[serde_json::Value]) -> (bool, bool) {
    let state = |step: &str| {
        events
            .iter()
            .find(|e| e["step"].as_str() == Some(step) && e.get("id").is_none())
            .and_then(|e| e["state"].as_str())
    };
    let done = |s: Option<&str>| matches!(s, Some("done" | "skipped"));
    (done(state("server")), done(state("apps")))
}

/// Whether the Welcome page opens at launch (Section 6): on the first launch, and on a later one
/// while the daemon or the apps step is not done - unless the person chose "Finish later", or turned
/// Start at Login off, which settles the daemon step their way. Unknown steps open nothing.
fn welcome_at_launch(p: WelcomePrefs, steps: Option<(bool, bool)>) -> bool {
    if !p.seen {
        return true;
    }
    !p.later && finish_setup_offered(p, steps)
}

/// Whether the tray offers "Finish setup..." (Section 6): while either step is not done, the daemon
/// step only until the person turned Start at Login off by choice.
fn finish_setup_offered(p: WelcomePrefs, steps: Option<(bool, bool)>) -> bool {
    steps.is_some_and(|(server, apps)| !apps || (!server && !p.login_off))
}

/// The events of one `setup --events` run: the lines after its schema line. `Err` says why there are
/// none - a CLI before `setup`, one newer than this app, or a run that failed before writing any.
fn setup_events(out: &std::process::Output) -> Result<Vec<serde_json::Value>, String> {
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let head = lines.next().and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok());
    match head.as_ref().and_then(|h| h["schema"].as_u64()) {
        Some(s) if s > CLI_SCHEMA => {
            Err("This supragnosis-server is newer than the app. Update the app: brew upgrade --cask supragnosis".into())
        }
        Some(_) => Ok(lines.filter_map(|l| serde_json::from_str(l).ok()).collect()),
        None if String::from_utf8_lossy(&out.stderr).contains("unrecognized subcommand") => {
            Err(SETUP_TOO_OLD.into())
        }
        None => Err(cli_outcome(out).1),
    }
}

/// The pid of the daemon this app spawned, which setup cannot see on its own (Section 3).
fn spawned_pid(app: &tauri::AppHandle) -> Option<u32> {
    match &*app.state::<DaemonGuard>().0.lock().unwrap() {
        Daemon::Spawned(c) => Some(c.id()),
        _ => None,
    }
}

/// `setup <args> --events`, with the app's child named when there is one.
fn setup_args(base: &[&str], app_daemon: Option<u32>) -> Vec<String> {
    let mut args: Vec<String> = std::iter::once("setup")
        .chain(base.iter().copied())
        .chain(std::iter::once("--events"))
        .map(str::to_string)
        .collect();
    if let Some(pid) = app_daemon {
        args.push("--app-daemon".into());
        args.push(pid.to_string());
    }
    args
}

/// `setup --check --events`: the checklist, read only (O3).
fn setup_check_blocking(app_daemon: Option<u32>) -> Result<Vec<serde_json::Value>, String> {
    let args = setup_args(&["--check"], app_daemon);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_cli(&args).ok_or_else(cli_missing)?;
    setup_events(&out)
}

/// What to say when there is no CLI: the formula for an app the cask installed, which depends on it,
/// and the install script otherwise (docs/onboarding.md Section 7).
fn cli_missing() -> String {
    let cask = ["/opt/homebrew/Caskroom/supragnosis", "/usr/local/Caskroom/supragnosis"]
        .iter()
        .any(|p| Path::new(p).exists());
    if cask {
        "The supragnosis CLI was not found. Install it: brew install ashon/tap/supragnosis-server"
            .into()
    } else {
        "The supragnosis CLI was not found. Install it: curl -fsSL https://supragnosis.dev/install.sh | sh".into()
    }
}

/// The CLI runs the Welcome page started that have not ended, by the page's key, with each one's pid -
/// which is also its process group, so stopping it stops what it started.
struct RunningSteps(Mutex<std::collections::HashMap<String, u32>>);

/// Runs `supragnosis <args>` for an app page and passes every line it writes on as a `setup-line`
/// event under `key`: a JSON event from `--events`, or a line of plain output (`err` marks stderr).
/// Answers how it ended. The child leads its own process group, so `setup_stop` can stop the
/// download `setup --step model` started as well as setup itself.
async fn run_streamed(
    app: &tauri::AppHandle,
    key: &str,
    args: Vec<String>,
) -> Result<serde_json::Value, String> {
    let bin = find_server_bin().ok_or_else(cli_missing)?;
    let app = app.clone();
    let key = key.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let mut cmd = Command::new(&bin);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        with_login_env(&mut cmd);
        let mut child =
            cmd.spawn().map_err(|e| format!("could not run the supragnosis CLI: {e}"))?;
        app.state::<RunningSteps>().0.lock().unwrap().insert(key.clone(), child.id());
        let emit = {
            let app = app.clone();
            let key = key.clone();
            move |line: String, err: bool| {
                let _ = app.emit(
                    "setup-line",
                    serde_json::json!({ "key": key, "line": line, "err": err }),
                );
            }
        };
        let last_err = std::sync::Arc::new(Mutex::new(String::new()));
        let err_reader = child.stderr.take().map(|pipe| {
            let emit = emit.clone();
            let last_err = last_err.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        *last_err.lock().unwrap() =
                            line.trim().trim_start_matches("Error: ").to_string();
                        emit(line, true);
                    }
                }
            })
        });
        if let Some(pipe) = child.stdout.take() {
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    emit(line, false);
                }
            }
        }
        if let Some(t) = err_reader {
            let _ = t.join();
        }
        let status = child.wait().map_err(|e| format!("the CLI did not finish: {e}"))?;
        app.state::<RunningSteps>().0.lock().unwrap().remove(&key);
        let why = last_err.lock().unwrap().clone();
        Ok(serde_json::json!({
            "key": key,
            "ok": status.success(),
            "code": status.code(),
            "why": (!status.success()).then_some(why),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The checklist, for an app page. The app's own child is named, so setup reads it as the app's
/// daemon rather than as a process it does not know.
#[tauri::command]
async fn setup_check(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    let pid = spawned_pid(&app);
    let events = tokio::task::spawn_blocking(move || setup_check_blocking(pid))
        .await
        .map_err(|e| e.to_string())??;
    Ok(serde_json::json!({ "events": events }))
}

/// One act of an app page (Sections 4.1-4.3): `server`, `app=<id>`, `model`, or `takeover` - the
/// consent Section 4.1 asks for before a manager setup only reports is replaced. The daemon this app
/// spawned is stopped first for the two that install the login job, since setup cannot stop it.
#[tauri::command]
async fn setup_run(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    step: String,
    no_capture: Option<bool>,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    let installs = matches!(step.as_str(), "server" | "takeover");
    let args: Vec<String> = match step.as_str() {
        "server" => setup_args(&["--step", "server"], None),
        "takeover" => {
            ["service", "install", "--take-over", "--no-model"].map(String::from).to_vec()
        }
        "model" => setup_args(&["--step", "model"], None),
        s => {
            let id = s.strip_prefix("app=").unwrap_or_default();
            if !AI_APPS.iter().any(|(i, _)| *i == id) {
                return Err(format!("{step:?} is not a setup step"));
            }
            let mut a = setup_args(&["--step", s], None);
            if no_capture == Some(true) && id == "claude-code" {
                a.push("--no-capture".into());
            }
            a
        }
    };
    if app.state::<RunningSteps>().0.lock().unwrap().contains_key(&step) {
        return Err(format!("{step} is already running"));
    }
    if installs {
        let prev =
            std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
        if let Daemon::Spawned(mut child) = prev {
            let _ = child.kill();
            let _ = child.wait();
        } else {
            *app.state::<DaemonGuard>().0.lock().unwrap() = prev;
        }
    }
    let outcome = run_streamed(&app, &step, args).await;
    if installs {
        let sock = app.state::<VizSock>().0.clone();
        if outcome.as_ref().is_ok_and(|o| o["ok"].as_bool() == Some(true)) {
            let mut p = read_prefs(&app);
            p.login_off = false;
            let _ = write_prefs(&app, p);
            wait_for_socket(&sock, Duration::from_secs(10)).await;
        }
        bring_up(app.clone(), sock).await;
    } else {
        refresh_status(&app).await;
    }
    outcome
}

/// The acts of the app's pages that are running now. A page opened again - the person left Setup,
/// or Settings, while a step ran - shows them as running and waits for them, instead of losing them
/// or offering to start them again (design-system.md Section 14 step 13).
#[tauri::command]
async fn setup_running(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<Vec<String>, String> {
    own_page_caller(&webview)?;
    let mut keys: Vec<String> =
        app.state::<RunningSteps>().0.lock().unwrap().keys().cloned().collect();
    keys.sort();
    Ok(keys)
}

/// Stops a running act of an app page: "Not now" on the download (Section 4.3). The whole process
/// group goes, so the download setup started stops with it, and the next run fetches what is missing.
#[tauri::command]
async fn setup_stop(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    step: String,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    let Some(pid) = app.state::<RunningSteps>().0.lock().unwrap().get(&step).copied() else {
        return Ok(format!("{step} is not running"));
    };
    let ok = Command::new("kill")
        .args(["-s", "TERM", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(format!("stopped {step}"))
    } else {
        Err(format!("{step} could not be stopped"))
    }
}

/// What the Welcome page needs besides the checklist: what the app remembers, and its own facts.
#[tauri::command]
async fn welcome_state(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    let p = read_prefs(&app);
    Ok(serde_json::json!({
        "prefs": p.to_json(),
        "app": env!("CARGO_PKG_VERSION"),
        "cli": find_server_bin().map(|b| b.display().to_string()),
        "spawned": spawned_pid(&app).is_some(),
    }))
}

/// Records a choice the Welcome page asks the app to remember: "Finish later", or Start at Login left
/// off in its plan.
#[tauri::command]
async fn welcome_set(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    later: Option<bool>,
    login_off: Option<bool>,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    let mut p = read_prefs(&app);
    if let Some(v) = later {
        p.later = v;
    }
    if let Some(v) = login_off {
        p.login_off = v;
    }
    write_prefs(&app, p)?;
    refresh_status(&app).await;
    Ok(p.to_json())
}

/// The two acts an app page takes on another app (Section 4.2, D9): Claude Desktop is asked to quit
/// as the Dock's Quit asks, waited for, and opened again; Cursor is opened. A refusal - macOS asks the
/// person once whether this app may control Claude - leaves the instruction to do it by hand.
#[tauri::command]
async fn app_helper(webview: tauri::Webview, id: String) -> Result<String, String> {
    own_page_caller(&webview)?;
    tokio::task::spawn_blocking(move || match id.as_str() {
        "claude-desktop" => restart_claude_desktop(),
        "cursor" => open_app(&["-a", "Cursor"]).map(|_| {
            "Cursor is open. Turn supragnosis on under Settings > MCP if it is off.".to_string()
        }),
        other => Err(format!("{other:?} has no helper")),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Claude Desktop's bundle identifier, which names it to the quit request and to `open`.
const CLAUDE_DESKTOP: &str = "com.anthropic.claudefordesktop";

fn restart_claude_desktop() -> Result<String, String> {
    let by_hand = "Quit Claude Desktop (Cmd+Q) and open it again - it reads its servers at launch.";
    let running = || {
        Command::new("osascript")
            .args(["-e", &format!("application id \"{CLAUDE_DESKTOP}\" is running")])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
    };
    if running() {
        let quit = Command::new("osascript")
            .args(["-e", &format!("tell application id \"{CLAUDE_DESKTOP}\" to quit")])
            .output()
            .map_err(|e| format!("{by_hand} ({e})"))?;
        if !quit.status.success() {
            return Err(format!(
                "{by_hand} macOS did not let Supragnosis ask it to quit: {}",
                String::from_utf8_lossy(&quit.stderr).trim()
            ));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while running() {
            if std::time::Instant::now() > deadline {
                return Err(format!("{by_hand} It is still running - it may be asking something."));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    open_app(&["-b", CLAUDE_DESKTOP]).map_err(|e| format!("{by_hand} ({e})"))?;
    // It is open; whether it loaded supragnosis is its row's to say, once it reaches the server
    // (design-system.md Section 29.10).
    Ok("Claude Desktop is open. Its row says when it reaches the server.".into())
}

fn open_app(args: &[&str]) -> Result<(), String> {
    let out = Command::new("open").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// "Copy log" and "Copy diagnostics": the page hands the text to the system clipboard.
#[tauri::command]
async fn clipboard_copy(webview: tauri::Webview, text: String) -> Result<(), String> {
    own_page_caller(&webview)?;
    use std::io::Write;
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the clipboard is not reachable: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

/// "Copy diagnostics" (Section 6): `setup --check --events` and `status --json` together, for a
/// report. Both are read only, and neither carries the token.
#[tauri::command]
async fn diagnostics(webview: tauri::Webview, app: tauri::AppHandle) -> Result<String, String> {
    own_page_caller(&webview)?;
    // The shell's own state and the daemon log's last lines: what a failed start leaves, which the
    // CLI's reads cannot say (design-system.md Section 29.5). The log names the token by its path only.
    let guard = app.state::<DaemonGuard>();
    let shell = {
        let d = guard.0.lock().unwrap();
        // The tray's words, and a failure's reason whole - the log path included.
        match &*d {
            Daemon::Failed(e) => format!("{} ({e})", d.status_line()),
            other => other.status_line(),
        }
    };
    let log = daemon_log();
    let tail = std::fs::read_to_string(&log)
        .map(|t| {
            t.lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|e| format!("(not read: {e})"));
    let pid = spawned_pid(&app);
    tokio::task::spawn_blocking(move || {
        let args = setup_args(&["--check"], pid);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let text = |o: Option<std::process::Output>| {
            o.map(|o| {
                let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
                t.push_str(&String::from_utf8_lossy(&o.stderr));
                t
            })
            .unwrap_or_else(cli_missing)
        };
        format!(
            "supragnosis app {}\n{}\n\n$ supragnosis {}\n{}\n$ supragnosis status --json\n{}\n\n$ tail -n 40 {}\n{}",
            env!("CARGO_PKG_VERSION"),
            shell,
            args.join(" "),
            without_entry(&text(run_cli(&args))),
            text(run_cli(&["status", "--json"])),
            tilde(&log),
            tail,
        )
    })
    .await
    .map_err(|e| e.to_string())
}

/// The checklist's lines as a report copies them: each event without the first entry's words, the
/// app that wrote it and its entity, so a report carries no knowledge (onboarding.md O6, design-system.md
/// Section 29.10). A line that is not an event is copied as it is.
fn without_entry(text: &str) -> String {
    text.lines()
        .map(|line| match serde_json::from_str::<serde_json::Value>(line) {
            Ok(serde_json::Value::Object(mut e)) => {
                for k in ["quote", "app", "entity"] {
                    e.remove(k);
                }
                // The done sentence named the workspace and the app; the report says only that an
                // entry is in (design-system.md Section 29.13).
                if e.get("step").and_then(|v| v.as_str()) == Some("first")
                    && e.get("state").and_then(|v| v.as_str()) == Some("done")
                {
                    e.insert("say".into(), "An entry is in the store.".into());
                    e.remove("id");
                }
                serde_json::Value::Object(e).to_string()
            }
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" }
}

/// "Show in Finder" (design-system.md Section 29.5): one of a fixed pair - the daemon's log and the
/// node's `supragnosis.toml` - and nothing a caller names. The caller is one of the app's own pages
/// (S1).
#[tauri::command]
async fn reveal(webview: tauri::Webview, what: String) -> Result<(), String> {
    own_page_caller(&webview)?;
    let path = match what.as_str() {
        "log" => daemon_log(),
        "config" => home().join(".supragnosis/supragnosis.toml"),
        other => return Err(format!("nothing to show called {other:?}")),
    };
    if !path.exists() {
        return Err(format!("{} does not exist yet", tilde(&path)));
    }
    #[cfg(target_os = "macos")]
    let shown = Command::new("open").arg("-R").arg(&path).status();
    #[cfg(not(target_os = "macos"))]
    let shown = Command::new("xdg-open").arg(path.parent().unwrap_or(&path)).status();
    match shown {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("the file manager did not open ({s})")),
        Err(e) => Err(e.to_string()),
    }
}

/// Turns the main window to the Welcome page, creating the window first if needed.
fn show_welcome(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        if !current.as_ref().is_some_and(|u| is_page(u, "welcome.html")) {
            w.navigate(with_from(welcome_url(), current.as_ref()))?;
        }
    }
    Ok(())
}

/// At launch, after the daemon is up: the Welcome page opens if Section 6 says it should, and is
/// then remembered as seen.
async fn welcome_at_start(app: tauri::AppHandle) {
    let p = read_prefs(&app);
    let steps = if p.seen {
        let pid = spawned_pid(&app);
        tokio::task::spawn_blocking(move || setup_check_blocking(pid))
            .await
            .ok()
            .and_then(Result::ok)
            .map(|events| essentials(&events))
    } else {
        None
    };
    if welcome_at_launch(p, steps) {
        if let Err(e) = show_welcome(&app) {
            tracing::error!(error = %e, "failed to open the welcome page");
        }
        if !p.seen {
            let _ = write_prefs(&app, WelcomePrefs { seen: true, ..p });
        }
    }
}

/// The graph's address: the viewer, which the shell's `viz://` proxy serves.
fn graph_url() -> tauri::Url {
    "viz://localhost/".parse().expect("static url")
}

/// Turns the main window to the settings page (docs/settings-page.md Section 3.0), creating the
/// window first if the app has retreated to the tray. With a section, opens that section; without
/// one, a window already on the settings page stays where it is.
fn show_settings(app: &tauri::AppHandle, section: Option<&str>) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        let there = current.as_ref().is_some_and(|u| is_page(u, "settings.html"));
        if !there || section.is_some() {
            let mut url = with_from(settings_url(), current.as_ref());
            url.set_fragment(section);
            w.navigate(url)?;
        }
    }
    Ok(())
}

/// Turns the main window to the Review page (docs/design-system.md Section 7): a route of the viewer's
/// own page, `#review`. From the settings page that is a navigation; on the viewer it is the hash, so
/// the graph's state is kept.
fn show_review(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if let Some(u) = w.url().ok().filter(|u| own_page_ok("main", u)) {
            let mut url = graph_left(&u).unwrap_or_else(graph_url);
            url.set_fragment(Some("review"));
            w.navigate(url)?;
        } else {
            w.eval("location.hash = '#review'")?;
        }
    }
    Ok(())
}

/// The tray's Review item opens the page at All workspaces, the scope its count is of
/// (docs/design-system.md Section 7). The menu bar's Review keeps the scope on screen.
fn show_review_all(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if w.url().is_ok_and(|u| own_page_ok("main", &u)) {
            let mut url = graph_url();
            url.query_pairs_mut().append_pair("workspace", "*");
            url.set_fragment(Some("review"));
            w.navigate(url)?;
        } else {
            w.eval("typeof openReviewAt === 'function' ? openReviewAt('*') : (location.hash = '#review')")?;
        }
    }
    Ok(())
}

/// Turns the main window to the graph, creating it first if needed.
fn show_graph(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if let Some(u) = w.url().ok().filter(|u| own_page_ok("main", u)) {
            // Back to the graph as the person left it (design-system.md Section 29.4).
            w.navigate(graph_left(&u).unwrap_or_else(graph_url))?;
        }
    }
    Ok(())
}

/// Shows the viewer window (creating it on first use / after the app retreated to the tray) and
/// returns the app to the dock. Closing the window hides it and drops back to Accessory (menu
/// bar only) - see on_window_event.
fn show_viewer(app: &tauri::AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External("viz://localhost/".parse().expect("static url")),
    )
    .title(&app.package_info().name)
    .min_inner_size(MIN_WINDOW.0, MIN_WINDOW.1)
    .initialization_script(include_str!("../assets/shell-init.js"))
    // Each page the window loads - the viewer, Settings, Setup - takes the kept text size (U21).
    .on_page_load(|w, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            if let Some(state) = w.app_handle().try_state::<TextScale>() {
                let scale = *state.0.lock().unwrap();
                if scale != 1.0 {
                    let _ = w.eval(text_scale_script(scale));
                }
            }
        }
    });
    // Merge the title bar into the viewer header: the macOS title bar becomes a transparent
    // overlay (traffic lights float over the content, no title text), and shell-init.js turns
    // the header into the drag region with its left edge padded clear of the lights.
    // The lights are pinned to the header row's geometry (headerHeight in the shell-page-loaded
    // log line; 49px today, center 24.5). NOTE tao's semantics (macos/view.rs
    // inset_traffic_lights): y is NOT the button's top - tao grows the titlebar container to
    // (button height + y) and the button keeps its default in-container offset, so the visual
    // button center lands slightly BELOW y (center = y - 1.5 as calibrated on macOS 15).
    // y=26 centers on 24.5, confirmed visually; recalibrate if headerHeight changes (x=14
    // mirrors the header's own padding, and tao re-applies the inset on every redraw, so it
    // survives window events). Because the container is sized off the BUTTON's height, the
    // mapping also depends on the traffic-light metrics, and macOS 26 gates those on the
    // linked SDK - an app built with an older SDK gets the compatibility metrics and lands
    // the lights off-center (seen on the macos-14-built release under macOS 26). The release
    // build pins an SDK-26 runner for the app; build locally with a current Xcode. y=26
    // re-confirmed on macOS 26 with the SDK-26 build.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 26.0));
    // The window opens where the person left it, at its size, when that place is on a screen now;
    // otherwise at the default size, centred (design-system.md Section 29.14).
    let builder = match kept_frame(app) {
        Some(f) => builder.inner_size(f.w, f.h).position(f.x, f.y),
        None => builder.inner_size(1280.0, 860.0).center(),
    };
    builder.build()?;
    Ok(())
}

fn main() {
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
    let sock = viz_sock();

    let proxy_sock = sock.clone();
    tauri::Builder::default()
        .manage(DaemonGuard(Mutex::new(Daemon::Starting)))
        .manage(RunningSteps(Mutex::new(std::collections::HashMap::new())))
        .invoke_handler(tauri::generate_handler![
            settings_state,
            reveal,
            app_toggle,
            capture_set,
            login_set,
            daemon_restart,
            search_rebuild,
            peer_narrow,
            setup_check,
            setup_run,
            setup_stop,
            setup_running,
            welcome_state,
            welcome_set,
            app_helper,
            clipboard_copy,
            diagnostics
        ])
        .register_asynchronous_uri_scheme_protocol("viz", move |ctx, request, responder| {
            let sock = proxy_sock.clone();
            let app = ctx.app_handle().clone();
            let target = request
                .uri()
                .path_and_query()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "/".to_string());
            let if_none_match = forwarded_if_none_match(request.headers().get("if-none-match"));
            tauri::async_runtime::spawn(async move {
                let r = if target.starts_with("/api/events") {
                    // SSE cannot ride a request/response protocol - the viz-event bridge covers it.
                    resp(
                        404,
                        "application/json",
                        SHELL_CSP,
                        br#"{"error":"SSE rides the Tauri event bridge (viz-event), not the proxy"}"#.to_vec(),
                    )
                } else if let Some((ctype, bytes)) = shell_asset(&target) {
                    resp(200, ctype, SHELL_CSP, bytes.to_vec())
                } else if target.starts_with("/__shell/") {
                    resp(404, "text/plain", SHELL_CSP, b"not a shell asset".to_vec())
                } else {
                    let fetch = uds_request(&sock, "GET", &target, if_none_match.as_deref());
                    match tokio::time::timeout(Duration::from_secs(15), fetch).await {
                        // The daemon's own policy governs the daemon's own page. An answer with no
                        // policy is forwarded with none, so a daemon that stopped sending one shows
                        // up as that rather than as a shell-authored guess (see SHELL_CSP).
                        Ok(Ok(f)) => {
                            // The graph's own page: the starting page is no longer on screen.
                            if target == "/" {
                                *SPLASH.lock().unwrap() = None;
                            }
                            let csp = with_ipc_sources(f.csp.as_deref().unwrap_or(""));
                            let mut r = resp(f.status, &f.ctype, &csp, f.body);
                            // The graph's ETag goes back with it, so the poll can ask (U33).
                            if let Some(v) = f.etag.and_then(|t| http::HeaderValue::from_str(&t).ok()) {
                                r.headers_mut().insert("etag", v);
                            }
                            r
                        }
                        // Socket not answering: the index gets a self-refreshing splash carrying
                        // the live daemon state (a Failed reason must be readable, not an eternal
                        // "starting..."); API calls get an honest 502 (Principle 5).
                        _ if target == "/" => {
                            let since = START_AT.lock().unwrap().map(|t| t.elapsed());
                            let page = start_page(&mut app.state::<DaemonGuard>().0.lock().unwrap(), since);
                            let conflict = splash_conflict().await;
                            let html = starting_html(&page, conflict.as_ref(), &settings_url(), &daemon_log());
                            splash_drawn(&app, sock.clone(), splash_key(&app, conflict.as_ref()));
                            resp(200, "text/html; charset=utf-8", SHELL_CSP, html.into_bytes())
                        }
                        Ok(Err(e)) => resp(
                            502,
                            "application/json",
                            SHELL_CSP,
                            serde_json::json!({ "error": format!("viewer socket unreachable: {e}") })
                                .to_string()
                                .into_bytes(),
                        ),
                        Err(_) => resp(
                            504,
                            "application/json",
                            SHELL_CSP,
                            br#"{"error":"viewer socket timed out"}"#.to_vec(),
                        ),
                    }
                };
                responder.respond(r);
            });
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Closing the window is not quitting: the shell (and the daemon) stay resident,
                // reachable from the tray. On macOS also leave the dock (Accessory) - the menu
                // bar mark is the app's background presence.
                api.prevent_close();
                // The window's frame is kept when it closes (design-system.md Section 29.14).
                keep_frame(window.app_handle());
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            WindowEvent::Moved(_) => note_frame(window),
            WindowEvent::Resized(_) => {
                note_frame(window);
                // macOS hides the traffic lights in fullscreen - tell the page, so the header
                // can drop the left padding that cleared them (shell-init.js toggles a class).
                // Resized fires on the fullscreen transition; re-emitting the same state is a
                // no-op on the page side.
                if let Ok(fs) = window.is_fullscreen() {
                    let _ = window.emit("shell-fullscreen", fs);
                }
            }
            _ => {}
        })
        .setup(move |app| {
            // The login shell's PATH, asked once in the background, so the first CLI call the
            // Settings page makes does not wait for the shell to start (client-connect.md Section 5).
            std::thread::spawn(|| {
                let _ = login_env();
            });
            // Tray: says what state this Mac is in and opens the windows that change it
            // (docs/settings-page.md Section 5, design-system.md Section 29.13). The first menu
            // says Starting...; refresh_status builds it again in its fixed order.
            let name = app.package_info().name.clone();
            let status = MenuItem::with_id(app, "tray-status", "Starting...", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "tray-open", "Open Graph", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "tray-settings", "Settings...", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "tray-quit", format!("Quit {name}"), true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &status,
                    &PredefinedMenuItem::separator(app)?,
                    &open,
                    &settings,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?;
            app.manage(TrayKept(Mutex::new(None)));
            app.manage(FrameNow(Mutex::new(None)));
            app.manage(AppsHint(Mutex::new((false, false))));
            app.manage(Attention(Mutex::new(None)));
            app.manage(VizSock(sock.clone()));
            app.manage(TextScale(Mutex::new(read_text_scale(app.handle()))));

            // The menu bar (design-system.md Section 29.13): the platform's menus, and in them the
            // app's pages, the map's commands and its help. Each item has its own id, apart from the
            // tray's, so an item does only its own act.
            let app_menu = Menu::default(app.handle())?;
            let item = |id: &str, text: &str, key: Option<&str>| MenuItem::with_id(app, id, text, true, key);
            let mut map_items: Vec<(&'static str, MenuItem<tauri::Wry>)> = Vec::new();
            for sub in app_menu.items()?.iter().filter_map(|i| i.as_submenu().cloned()) {
                let text = sub.text()?;
                if text == "View" {
                    // The map's commands (design-system.md Section 29.13), kept so each page's load
                    // report can say which of them act there.
                    let mapped = [
                        ("view-filters", "Filters Panel", Some("Ctrl+CmdOrCtrl+S")),
                        ("view-log", "Log Panel", Some("Ctrl+CmdOrCtrl+L")),
                        ("view-map", "as Map", None),
                        ("view-list", "as List", None),
                        ("view-fit", "Fit to View", None),
                        ("view-keys", "Keys...", None),
                        ("view-back", "Back", Some("CmdOrCtrl+[")),
                        ("view-forward", "Forward", Some("CmdOrCtrl+]")),
                    ];
                    let mut kept = Vec::new();
                    for (id, words, key) in mapped {
                        kept.push((id, item(id, words, key)?));
                    }
                    let at = |id: &str| kept.iter().find(|(k, _)| *k == id).map(|(_, m)| m.clone()).unwrap();
                    // Text that can grow (design-system.md Section 10, U21): the root's size, not the
                    // webview's zoom, so the title bar and its traffic lights stay put.
                    let items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = vec![
                        Box::new(item("graph", "Graph", Some("CmdOrCtrl+1"))?),
                        Box::new(item("review", "Review", Some("CmdOrCtrl+2"))?),
                        Box::new(PredefinedMenuItem::separator(app)?),
                        Box::new(at("view-filters")),
                        Box::new(at("view-log")),
                        Box::new(at("view-map")),
                        Box::new(at("view-list")),
                        Box::new(at("view-fit")),
                        Box::new(at("view-keys")),
                        Box::new(PredefinedMenuItem::separator(app)?),
                        Box::new(at("view-back")),
                        Box::new(at("view-forward")),
                        Box::new(PredefinedMenuItem::separator(app)?),
                        Box::new(item("text-bigger", "Make Text Bigger", Some("CmdOrCtrl+="))?),
                        Box::new(item("text-smaller", "Make Text Smaller", Some("CmdOrCtrl+-"))?),
                        Box::new(item("text-actual", "Actual Size", Some("CmdOrCtrl+0"))?),
                        Box::new(PredefinedMenuItem::separator(app)?),
                    ];
                    for (i, it) in items.iter().enumerate() {
                        sub.insert(it.as_ref(), i)?;
                    }
                    map_items.extend(kept);
                } else if text == "Edit" {
                    let find = item("edit-find", "Find", Some("CmdOrCtrl+F"))?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&find)?;
                    map_items.push(("edit-find", find));
                } else if text == "Help" {
                    sub.append(&item("help-docs", "Supragnosis Documentation", None)?)?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&item("help-diagnostics", "Diagnostics...", None)?)?;
                    sub.append(&item("help-log", "Show Log in Finder", None)?)?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&item("help-issue", "Report an Issue...", None)?)?;
                } else if text == name {
                    // About, a separator, Settings... and Setup..., a separator, then the
                    // platform's own items; its Quit takes the tray's words (D15).
                    sub.insert(&PredefinedMenuItem::separator(app)?, 1)?;
                    sub.insert(&item("settings", "Settings...", Some("CmdOrCtrl+,"))?, 2)?;
                    sub.insert(&item("setup", "Setup...", None)?, 3)?;
                    for it in sub.items()? {
                        if let Some(p) = it.as_predefined_menuitem() {
                            if p.text().is_ok_and(|t| t.starts_with("Quit")) {
                                app.manage(AppQuit(p.clone()));
                            }
                        }
                    }
                }
            }
            app.set_menu(app_menu)?;
            app.manage(MapItems(map_items));
            app.on_menu_event(|app, event| {
                let id = event.id().as_ref();
                let shown = match id {
                    "settings" => show_settings(app, None),
                    "setup" => show_welcome(app),
                    "graph" => show_graph(app),
                    "review" => show_review(app),
                    "text-bigger" => set_text_scale(app, 1),
                    "text-smaller" => set_text_scale(app, -1),
                    "text-actual" => set_text_scale(app, 0),
                    "help-docs" => open_url(&format!("{}#readme", env!("CARGO_PKG_REPOSITORY"))),
                    "help-issue" => open_url(&format!("{}/issues", env!("CARGO_PKG_REPOSITORY"))),
                    // Done where they report their outcome, on This Mac (design-system.md 29.13).
                    "help-diagnostics" => show_settings_act(app, "diagnostics"),
                    "help-log" => show_settings_act(app, "reveal-log"),
                    other => map_command(app, other),
                };
                if let Err(e) = shown {
                    tracing::error!(error = %e, "failed to turn the main window");
                }
            });

            TrayIconBuilder::with_id("supragnosis")
                // Template image (bare mark, alpha-only): macOS recolors it for light/dark menu bars.
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip(&name)
                .menu(&menu)
                .show_menu_on_left_click(true)
                // Every menu event reaches this listener; it acts only on the tray's own ids.
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "tray-open" => {
                        if let Err(e) = show_graph(app) {
                            tracing::error!(error = %e, "failed to open the graph");
                        }
                    }
                    "tray-review" => {
                        if let Err(e) = show_review_all(app) {
                            tracing::error!(error = %e, "failed to open the review page");
                        }
                    }
                    "tray-finish-setup" => {
                        if let Err(e) = show_welcome(app) {
                            tracing::error!(error = %e, "failed to open the welcome page");
                        }
                    }
                    // The act line and Settings... open the section that needs attention.
                    "tray-act" | "tray-settings" => {
                        let section =
                            app.try_state::<Attention>().and_then(|a| *a.0.lock().unwrap());
                        if let Err(e) = show_settings(app, section) {
                            tracing::error!(error = %e, "failed to open the settings");
                        }
                    }
                    "tray-quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // The Welcome page opens at once on the first launch; on a later one, once the daemon is
            // up and setup has said whether the daemon and the apps are set up (Section 6).
            let first = !read_prefs(app.handle()).seen;
            let up = app.handle().clone();
            let up_sock = sock.clone();
            tauri::async_runtime::spawn(async move {
                bring_up(up.clone(), up_sock).await;
                if !first {
                    welcome_at_start(up).await;
                }
            });
            // An upgrade replaces the binary under a running daemon without telling anyone; the
            // status line is where that becomes visible (docs/daemon-lifecycle.md Section 5), so it
            // is re-read on a slow beat rather than only when the shell itself acts.
            let beat = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    refresh_status(&beat).await;
                }
            });
            tauri::async_runtime::spawn(sse_bridge(app.handle().clone(), sock.clone()));
            // Startup health signal from the init script: which page the webview actually loaded
            // (the daemon-served viewer vs the starting splash) - the shell's only observable for
            // "the proxy + webview path works", and the log line to look for when it does not.
            let loaded = app.handle().clone();
            app.listen("shell-page-loaded", move |event| {
                tracing::info!(page = %event.payload(), "webview page loaded");
                // Which of the map's commands act on this page, from the controls it reported
                // (design-system.md Section 29.13).
                if let Ok(report) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                    enable_map_items(&loaded, &report);
                }
            });
            show_viewer(app.handle())?;
            if first {
                show_welcome(app.handle())?;
                let p = read_prefs(app.handle());
                let _ = write_prefs(app.handle(), WelcomePrefs { seen: true, ..p });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the tauri app")
        .run(|app, event| match event {
            RunEvent::ExitRequested { code, api, .. } => {
                // A tray-resident app does not die with its windows: only an explicit exit
                // (tray Quit -> app.exit(0) carries a code) may end the process.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                keep_frame(app);
                let prev = std::mem::replace(
                    &mut *app.state::<DaemonGuard>().0.lock().unwrap(),
                    Daemon::Starting,
                );
                if let Some(mut child) = reaped_on_exit(prev) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            // Relaunching the running instance (Finder/Spotlight/dock) does not start a new
            // process - macOS delivers reopen instead. With the window closed the shell is
            // tray-only (Accessory), so without this arm "opening the app again" does nothing;
            // convention says it brings the main window back.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows,
                ..
            } => {
                tracing::info!(has_visible_windows, "reopen - showing the viewer");
                if let Err(e) = show_viewer(app) {
                    tracing::error!(error = %e, "failed to reopen the viewer window");
                }
            }
            _ => {}
        });
}

/// Every source file of the app, for the tests that read the source by its text: the modules, then
/// this file, then the tests, so the text before the first test module is the app's own.
#[cfg(test)]
const SRC: &str =
    concat!(include_str!("cli.rs"), include_str!("proxy.rs"), include_str!("main.rs"),);

#[cfg(test)]
mod conditional_tests {
    use super::{forwarded_if_none_match, http};

    /// The poll's tag reaches the daemon as it was sent (design-system.md U33), and only a tag that
    /// cannot break the request head it is written into: a line break would start a header of its own.
    #[test]
    fn the_poll_tag_is_passed_on_only_when_it_is_one_line() {
        let ok = http::HeaderValue::from_static("\"5f3a9c0d1e2b4a67-1a2b\"");
        assert_eq!(
            forwarded_if_none_match(Some(&ok)).as_deref(),
            Some("\"5f3a9c0d1e2b4a67-1a2b\"")
        );
        assert_eq!(forwarded_if_none_match(None), None);
        let empty = http::HeaderValue::from_static("");
        assert_eq!(forwarded_if_none_match(Some(&empty)), None);
        let tab = http::HeaderValue::from_bytes(b"\"a\"\t\"b\"").unwrap();
        assert_eq!(forwarded_if_none_match(Some(&tab)), None, "only printable ASCII is passed on");
    }
}

#[cfg(test)]
mod csp_tests {
    use super::{with_ipc_sources, TAURI_IPC_SOURCES};

    /// The forwarded policy reaches the webview widened for IPC and unchanged everywhere else.
    ///
    /// The failure this guards is silent in both directions: too narrow and the tray goes deaf
    /// (`connect-src 'self'` refuses Tauri's transport), too wide and the proxy has quietly relaxed
    /// a policy the daemon meant strictly. So `script-src` is asserted to survive byte for byte.
    #[test]
    fn forwarding_widens_connect_src_and_nothing_else() {
        let daemon =
            "default-src 'none'; script-src 'self' viz:; style-src 'self' 'unsafe-inline'; \
                      img-src 'self' data:; connect-src 'self'; base-uri 'none'";
        let out = with_ipc_sources(daemon);

        let connect = out
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("connect-src"))
            .expect("connect-src survives");
        assert_eq!(connect, format!("connect-src 'self' {TAURI_IPC_SOURCES}"));

        // Exactly one connect-src: a second would be ignored by the browser, which is the quiet way
        // this fix could look applied and do nothing.
        assert_eq!(out.matches("connect-src").count(), 1, "one directive, widened in place: {out}");

        // Everything else is byte-identical - the script half above all.
        assert!(out.contains("script-src 'self' viz:"), "script-src must be untouched: {out}");
        assert!(!out.contains("unsafe-eval"));
        for d in ["default-src 'none'", "img-src 'self' data:", "base-uri 'none'"] {
            assert!(out.contains(d), "{d} must survive: {out}");
        }
    }

    /// A policy with no `connect-src` still needs one: `default-src 'none'` denies the fetch by
    /// inheritance, so "absent" is not "permitted".
    #[test]
    fn a_policy_without_connect_src_gains_one() {
        let out = with_ipc_sources("default-src 'none'; script-src 'self'");
        assert!(out.contains(&format!("connect-src {TAURI_IPC_SOURCES}")), "got: {out}");
        assert!(out.contains("default-src 'none'"), "got: {out}");
    }

    /// No policy stays no policy. A daemon that stopped sending one must look like that, not like a
    /// shell-authored guess (the reason the proxy forwards rather than restates).
    #[test]
    fn an_absent_policy_is_not_invented() {
        assert_eq!(with_ipc_sources(""), "");
        assert_eq!(with_ipc_sources("   "), "");
    }
}

#[cfg(test)]
mod status_text_tests {
    use super::{
        app_row, cli_change, cli_outcome, cli_path, login_env_from_output, login_outcome,
        read_login_env, status_text, Daemon,
    };
    use std::time::Duration;

    /// An AI app's row in the settings window says its state and labels its one button with what a
    /// click will do - naming any entry other than the bridge, since the click replaces it - and has
    /// no button where a click could do nothing: the app is not installed, its settings could not be
    /// read, or the CLI is too old to know `connect`.
    #[test]
    fn an_app_item_says_what_a_click_will_do() {
        let r = |installed: bool, entry: &str| serde_json::json!({"installed": installed, "entry": entry});
        let (state, action, kind) = app_row(Some(&r(true, "bridge")));
        assert!(state.contains("bridge") && action == Some("Disconnect") && kind == "connected");
        let (state, action, kind) = app_row(Some(&r(true, "http")));
        assert!(state.contains("HTTP") && action == Some("Switch to the bridge"), "{state}");
        assert_eq!(kind, "attention", "a copy of the token is something to fix");
        assert_eq!(app_row(Some(&r(true, "other"))).1, Some("Switch to the bridge"));
        assert_eq!(app_row(Some(&r(false, "none"))), ("not installed".into(), None, "absent"));
        assert_eq!(app_row(Some(&r(true, "unknown"))).1, None);
        assert_eq!(app_row(None).1, None, "an old CLI cannot connect anything");
        assert_eq!(
            app_row(Some(&r(true, "none"))),
            ("not connected".into(), Some("Connect"), "off")
        );
    }

    /// client-connect.md Section 5: the CLI runs with the login shell's `PATH`. From the Finder the app
    /// has `/usr/bin:/bin:/usr/sbin:/sbin`, and `codex` under `~/.nvm/versions/node/<v>/bin` read
    /// "Not installed" on the Settings page while a terminal's `supragnosis connect` found it. The
    /// shell's `XDG_CONFIG_HOME` goes along, for OpenCode's configuration (Section 13.3).
    #[test]
    fn the_cli_runs_with_the_login_shells_path() {
        // What the startup files print is skipped: only the environment after the mark counts.
        let out = "Last login: today\nPATH=/printed/by/zshrc\n__supragnosis_env__\nHOME=/Users/x\n\
                   PATH=/Users/x/.nvm/versions/node/v24/bin:/usr/bin\n";
        let read = login_env_from_output(out).expect("a PATH");
        assert_eq!(read.path.as_deref(), Some("/Users/x/.nvm/versions/node/v24/bin:/usr/bin"));
        assert_eq!(read.config_home, None, "unset stays unset");
        assert_eq!(login_env_from_output("PATH=/usr/bin\n"), None, "no mark, no environment");
        let xdg = "__supragnosis_env__\nXDG_CONFIG_HOME=/Users/x/dots\nPATH=/usr/bin\n";
        assert_eq!(
            login_env_from_output(xdg).and_then(|e| e.config_home).as_deref(),
            Some("/Users/x/dots")
        );

        // A shell that prints first, sets PATH in its startup, then runs the command it is given.
        let dir = std::env::temp_dir().join(format!("sg-login-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let shell = write(
            "shell",
            "#!/bin/sh\necho 'welcome PATH=/not/this'\nPATH=/opt/nvm/bin:/usr/bin\nexport PATH\neval \"$4\"\n",
        );
        assert_eq!(
            read_login_env(&shell, Duration::from_secs(5)).and_then(|e| e.path).as_deref(),
            Some("/opt/nvm/bin:/usr/bin")
        );
        // A shell that never answers leaves the app's PATH alone, and does not hold the app up.
        let hangs = write("hangs", "#!/bin/sh\nsleep 30\n");
        let started = std::time::Instant::now();
        assert_eq!(read_login_env(&hangs, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        let _ = std::fs::remove_dir_all(&dir);

        // The shell's directories lead; the app's own follow, without repeats.
        assert_eq!(
            cli_path(Some("/opt/nvm/bin:/usr/bin"), Some(std::ffi::OsStr::new("/usr/bin:/bin"))),
            Some(std::ffi::OsString::from("/opt/nvm/bin:/usr/bin:/bin"))
        );
        assert_eq!(cli_path(None, Some(std::ffi::OsStr::new("/usr/bin"))), None);
    }

    fn st(
        situation: &str,
        running: Option<&str>,
        here: &str,
        manager: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "situation": situation,
            "managers": if manager.is_null() { serde_json::json!([]) } else { serde_json::json!([manager]) },
            "version": { "here": here, "running": running },
            "service": { "state": "generated" },
        })
    }

    /// docs/design-system.md Sections 6a and 29.6: the AI app's row is built once, in kit.js, and
    /// drawn by both pages - one pill map, one capture label, the helper on both.
    #[test]
    fn the_ai_app_row_is_built_once() {
        let kit = include_str!("../assets/kit.js");
        for part in [
            "function appRow(row)",
            "attention: [\"warn\", \"Needs you\"]",
            "label: \"Save the why behind each commit\"",
            "const HELPER = { \"claude-desktop\": \"Restart Claude Desktop\", cursor: \"Open Cursor\" };",
        ] {
            assert!(kit.contains(part), "kit.js does not hold: {part}");
        }
        let settings = include_str!("../assets/settings.js");
        let welcome = include_str!("../assets/welcome.js");
        for (page, js) in [("Settings", settings), ("Setup", welcome)] {
            assert!(js.contains("appRow({ id: a.id"), "{page} builds its own app row");
            assert!(
                !js.contains("const HELPER") && !js.contains("const APP_PILL"),
                "{page} keeps its own words"
            );
        }
        assert!(settings.contains("CAPTURE.label") && welcome.contains("CAPTURE.label"));
        assert!(settings.contains("HELPER[a.id] && appNotes[a.id]"), "Settings offers no helper");
        assert!(settings
            .contains("title: disconnect ? null : APPS[a.id] + \" connected\", detail: null"));
    }

    /// settings-page.md S4: every setting the tray offered - Start at Login, Restart Daemon and AI
    /// Apps - is on the page. A source tripwire, one per command: settings.js calls each of the
    /// three controls' commands by name, through `act` (which says how it went) or `invoke`, and
    /// each is one the app registers for its own pages.
    #[test]
    fn every_setting_the_tray_offered_is_on_the_page() {
        let js = include_str!("../assets/settings.js");
        for (setting, cmd) in [
            ("Start at Login", "login_set"),
            ("Restart Daemon", "daemon_restart"),
            ("AI Apps", "app_toggle"),
        ] {
            let quoted = format!("\"{cmd}\"");
            let called = js.lines().any(|line| {
                line.find(&quoted).is_some_and(|at| {
                    let before = &line[..at];
                    before.contains("act(") || before.contains("invoke(")
                })
            });
            assert!(called, "{setting}: settings.js no longer calls {cmd}");
            assert!(crate::APP_COMMANDS.contains(&cmd), "{cmd} is not one of the app's commands");
        }
    }

    /// A failing `Output` with the CLI's reason on stderr after a blank line and `Error:`, as the CLI
    /// writes one.
    fn refused(why: &str) -> std::process::Output {
        output(1, "", &format!("\nError: {why}\nhint\n"))
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// The body of `fn name(` in this file, to its closing brace at the start of a line.
    fn source_of(name: &str) -> &'static str {
        let src = super::SRC;
        let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
        let rest = &src[start..];
        &rest[..rest.find("\n}\n").expect("it closes")]
    }

    /// daemon-lifecycle.md L4: the app never discards the CLI's exit status. `cli_outcome` reads the
    /// status and the line that matters - the error on failure, past blank lines and the `Error:`
    /// prefix - and a refused Start at Login change is an error carrying that reason all the way to
    /// the page, whose switch shows it under a failure title of its own rather than under "Starts at
    /// login".
    #[test]
    fn a_refused_login_change_says_why() {
        let why = "another manager runs the daemon: sh.brew.supragnosis-server";
        assert_eq!(cli_outcome(&refused(why)), (false, why.to_string()));
        assert_eq!(cli_outcome(&output(0, "installed\n", "")), (true, "installed".to_string()));

        assert_eq!(login_outcome(false, Some(&refused(why))), Err(why.to_string()));
        assert_eq!(login_outcome(true, Some(&refused(why))), Err(why.to_string()));
        assert_eq!(
            login_outcome(false, Some(&output(1, "", ""))),
            Err("the supragnosis command failed".to_string())
        );
        assert_eq!(
            login_outcome(false, None),
            Err("the supragnosis command was not found".to_string())
        );
        let ok = output(0, "", "");
        assert_eq!(login_outcome(false, Some(&ok)), Ok("starts at login".to_string()));
        assert_eq!(login_outcome(true, Some(&ok)), Ok("no longer starts at login".to_string()));

        // The outcome reaches the page as it is: returned last, never re-wrapped as a success.
        assert!(source_of("set_login").trim_end().ends_with("outcome"));
        assert!(source_of("login_set").trim_end().ends_with("set_login(&app, sock, on).await"));
        let js = include_str!("../assets/settings.js");
        let switch = js.lines().find(|l| l.contains("\"login_set\"")).expect("the switch");
        assert!(
            switch.contains("failTitle: e.target.checked ? \"Start at Login was not turned on\" : \"Start at Login was not turned off\""),
            "the switch names its own failure: {switch}"
        );
    }

    /// settings-page.md Section 3: an error keeps until it is read. A refused CLI change is an error
    /// carrying the CLI's reason, returned as it is, never a success with the refusal beneath it.
    #[test]
    fn a_refused_change_says_why() {
        let why = "the login job could not be loaded";
        assert_eq!(cli_change(Some(&refused(why)), |_| "done".into()), Err(why.to_string()));
        assert_eq!(
            cli_change(Some(&output(0, "x", "")), |o| o.stdout.len().to_string()),
            Ok("1".into())
        );
    }

    /// daemon-lifecycle.md L6: the app stops a daemon it did not spawn only on a person's act that
    /// asks for it. Every CLI call that can stop a daemon is listed with the function it sits in -
    /// the switch's `set_login`, Setup's `setup_run`, and `restart_daemon` and `rebuild_index`,
    /// which bring it back - and each of those is reached from one command of the app's own pages,
    /// which a click calls. A new call that stops a daemon fails this until it is added on purpose.
    #[test]
    fn only_the_persons_act_stops_a_daemon_it_did_not_spawn() {
        let src = super::SRC;
        let app = &src[..src.find("#[cfg(test)]\nmod ").expect("the tests")];
        let within = |at: usize| -> &str {
            let head = app[..at]
                .rfind("\nfn ")
                .max(app[..at].rfind("\nasync fn "))
                .max(app[..at].rfind("\npub(crate) fn "))
                .max(app[..at].rfind("\npub(crate) async fn "))
                .unwrap();
            let name = &app[head..];
            let name = &name[name.find("fn ").unwrap() + 3..];
            &name[..name.find('(').unwrap()]
        };
        let mut stops: Vec<(&str, &str)> = Vec::new();
        for call in [
            "\"service\", \"uninstall\"",
            "\"--take-over\"",
            "run_cli(&[\"restart\"",
            "\"embed\", \"rebuild\"",
            "\"--step\", \"server\"",
            "&[\"stop\"",
        ] {
            for (at, _) in app.match_indices(call) {
                stops.push((call, within(at)));
            }
        }
        stops.sort();
        let mut want = vec![
            ("\"--step\", \"server\"", "setup_run"),
            ("\"--take-over\"", "set_login"),
            ("\"--take-over\"", "setup_run"),
            ("\"embed\", \"rebuild\"", "rebuild_index"),
            ("\"service\", \"uninstall\"", "set_login"),
            ("run_cli(&[\"restart\"", "restart_daemon"),
        ];
        want.sort();
        assert_eq!(stops, want, "every CLI call that can stop a daemon, where it sits");

        // Each is reached only from the command a page's control calls.
        for (helper, command) in [
            ("set_login(&", "login_set"),
            ("restart_daemon(&", "daemon_restart"),
            ("rebuild_index(&", "search_rebuild"),
        ] {
            let callers: Vec<&str> = app.match_indices(helper).map(|(at, _)| within(at)).collect();
            assert_eq!(callers, [command], "{helper} is called by {command} alone");
        }
        for command in ["login_set", "daemon_restart", "search_rebuild", "setup_run"] {
            assert!(crate::APP_COMMANDS.contains(&command), "{command} is a page's command");
        }
    }

    /// daemon-lifecycle.md L6: quitting the app never stops a launchd-managed daemon. On exit the app
    /// reaps only the child it spawned; an attached daemon, one still starting and one that failed
    /// are left alone, and the exit path runs no CLI command - so nothing on the way out can stop a
    /// login job, a Homebrew service or a `supragnosis start` daemon.
    #[test]
    fn quitting_leaves_a_daemon_it_did_not_spawn() {
        assert!(super::reaped_on_exit(Daemon::External).is_none());
        assert!(super::reaped_on_exit(Daemon::Starting).is_none());
        assert!(super::reaped_on_exit(Daemon::Failed("x".into())).is_none());
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut reaped = super::reaped_on_exit(Daemon::Spawned(child)).expect("its own child");
        let _ = reaped.wait();

        let src = super::SRC;
        let exit = &src[src.find("RunEvent::Exit => {").expect("the exit arm")..];
        let exit = &exit[..exit.find("\n            }\n").expect("it closes")];
        assert!(exit.contains("reaped_on_exit(prev)"), "{exit}");
        for acts in ["run_cli", "Command::new", "stop", "service"] {
            assert!(!exit.contains(acts), "the exit path does not reach {acts}: {exit}");
        }
    }

    /// settings-page.md S3: the page decides nothing the CLI or the daemon decides. Every command
    /// the Settings page calls is in one list here, by what it is. A change runs the CLI - in the
    /// command or the helper it calls - and a refusal is an `Err` carrying what the CLI said, which
    /// the page shows. Stopping a peer's sharing is the one change that goes to the daemon's
    /// narrow-only endpoint instead, and runs no CLI. The rest read, or act on the system rather
    /// than a setting. A command the page starts calling fails this until it is placed.
    #[test]
    fn every_setting_change_is_a_cli_call() {
        let src = super::SRC;
        let app = &src[..src.find("#[cfg(test)]\nmod ").expect("the tests")];
        let body = |name: &str| -> &str {
            let at = ["async fn ", "fn ", "pub(crate) async fn ", "pub(crate) fn "]
                .iter()
                .find_map(|k| app.find(&format!("\n{k}{name}(")))
                .unwrap_or_else(|| panic!("no fn {name}"));
            let rest = &app[at + 1..];
            &rest[..rest.find("\n}\n").expect("it closes")]
        };
        // Each change, with the helpers its body reaches the CLI through.
        let changes: &[(&str, &[&str])] = &[
            ("app_toggle", &["toggle_app"]),
            ("capture_set", &[]),
            ("login_set", &["set_login", "login_outcome", "cli_change"]),
            ("daemon_restart", &["restart_daemon"]),
            ("search_rebuild", &["rebuild_index"]),
            ("setup_run", &["run_streamed"]),
        ];
        let reads = ["settings_state", "setup_running", "diagnostics"];
        let system = ["clipboard_copy", "reveal", "app_helper", "setup_stop"];

        // A command is a quoted name of the app's own on a line that calls `invoke(` or `act(`.
        let js = include_str!("../assets/settings.js");
        let mut called: Vec<&str> = Vec::new();
        for line in js.lines().filter(|l| l.contains("invoke(") || l.contains("act(")) {
            for command in crate::APP_COMMANDS {
                if line.contains(&format!("\"{command}\"")) {
                    called.push(command);
                }
            }
        }
        called.sort();
        called.dedup();
        let mut placed: Vec<&str> = changes.iter().map(|(c, _)| *c).collect();
        placed.extend(reads);
        placed.extend(system);
        placed.push("peer_narrow");
        placed.sort();
        assert_eq!(called, placed, "every command the page calls, placed by what it is");

        for (command, helpers) in changes {
            let reach: String =
                std::iter::once(*command).chain(helpers.iter().copied()).map(body).collect();
            assert!(
                reach.contains("run_cli(")
                    || reach.contains("Command::new(bin)")
                    || reach.contains("run_streamed("),
                "{command} runs the CLI"
            );
            assert!(
                reach.contains("cli_outcome")
                    || reach.contains("cli_change")
                    || reach.contains("restart_outcome(")
                    || reach.contains("\"why\": (!status.success())"),
                "{command}: a refusal carries what the CLI said"
            );
        }
        let narrow = body("peer_narrow");
        assert!(narrow.contains("/api/peer/share") && !narrow.contains("run_cli("));
    }

    /// docs/design-system.md Section 9 and U17: the daemon has one word per state, the same on the
    /// tray's line, Settings' This Mac pane, Setup's first card and the starting page.
    #[test]
    fn the_daemon_has_one_word_per_state() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut spawned = Daemon::Spawned(child);
        assert!(spawned.status_line().starts_with("Running while this app is open"));
        if let Daemon::Spawned(c) = &mut spawned {
            let _ = c.wait();
        }
        assert_eq!(Daemon::Starting.status_line(), "Starting...");
        assert_eq!(Daemon::External.status_line(), "Running - started outside this app");
        assert_eq!(Daemon::Failed("x".into()).status_line(), "Could not start: X.");
        let stopped = serde_json::json!({ "situation": "none", "answering": false, "managers": [], "version": { "here": "0.4.2" } });
        assert_eq!(status_text(&Daemon::External, Some(&stopped)), "Stopped");

        let settings = include_str!("../assets/settings.js");
        let words = &settings[settings.find("function daemonStatus(").expect("daemonStatus")..];
        let words = &words[..words.find("\n}\n").expect("it closes")];
        for word in [
            "\"Two managers run Supragnosis\"",
            "\"Update ready - restart\"",
            "\"Starting...\"",
            "\"Could not start\"",
            "\"Stopped\"",
            "\"Repairing - \"",
            "\"Running\"",
        ] {
            assert!(words.contains(word), "This Mac's card does not say {word}");
        }
        assert!(settings.contains("owed === 0 ? \"Up to date\" : \"Repairing - \""));
        let welcome = include_str!("../assets/welcome.js");
        assert!(welcome.contains("title: \"Keep Supragnosis running\""));
        let setup = include_str!("../../crates/supragnosis-cli/src/setup.rs");
        assert!(setup.contains(
            "\"Running while this app is open. Setup keeps it running at login instead.\""
        ));
    }

    /// The line names who runs the daemon - including a child of this app, which the CLI cannot see.
    #[test]
    fn the_line_names_the_manager() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let brew = serde_json::json!({ "type": "launchd", "source": "homebrew", "label": "sh.brew.supragnosis-server" });
        let s = st("one", Some("0.4.2"), "0.4.2", canonical);
        assert_eq!(status_text(&Daemon::External, Some(&s)), "Running 0.4.2 - starts at login");
        let s = st("one", Some("0.4.2"), "0.4.2", brew);
        assert_eq!(status_text(&Daemon::External, Some(&s)), "Running 0.4.2 - Homebrew's service");
        let s = st("unrecognized", Some("0.4.2"), "0.4.2", serde_json::Value::Null);
        assert_eq!(
            status_text(&Daemon::External, Some(&s)),
            "Running 0.4.2 - started outside this app"
        );
    }

    /// The 2026-10-03 symptom, said where it is read: an upgraded binary under an old process.
    #[test]
    fn drift_and_conflict_are_said_outright() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let s = st("one", Some("0.4.0"), "0.4.2", canonical);
        assert_eq!(
            status_text(&Daemon::External, Some(&s)),
            "Update ready - restart: 0.4.0 running, 0.4.2 installed"
        );
        let conflict = serde_json::json!({
            "situation": "conflict",
            "managers": [{ "type": "launchd" }, { "type": "launchd" }],
            "version": { "here": "0.4.2", "running": "0.4.0" },
        });
        assert_eq!(status_text(&Daemon::External, Some(&conflict)), "Two managers run Supragnosis");
    }

    /// Without a CLI that answers `status --json`, the shell says only what it knows itself.
    #[test]
    fn an_old_cli_falls_back_to_the_shells_own_view() {
        assert_eq!(status_text(&Daemon::External, None), "Running - started outside this app");
        assert_eq!(status_text(&Daemon::Starting, None), "Starting...");
        assert_eq!(
            status_text(&Daemon::Failed("port in use - full log: x".into()), None),
            "Could not start: Port in use."
        );
    }
}

#[cfg(test)]
mod settings_tests {
    use super::{
        cause_of, managed, manager_pid, own_page_ok, restart_outcome, settings_url, start_page,
        starting_html, welcome_url, without_entry, Daemon, StartPage, APP_COMMANDS,
    };
    use std::path::Path;
    use std::time::Duration;

    const LOG: &str = "/nowhere/.supragnosis/log/app-daemon.err.log";

    /// docs/settings-page.md S1 and docs/onboarding.md Section 6: a setting changes only from the
    /// app's own pages, Settings and Welcome - not from the viewer the same window shows, which a
    /// daemon or a hub serves, and not from any other address, a hub's page or a lookalike path
    /// included.
    #[test]
    fn only_the_apps_own_page_may_change_a_setting() {
        let viewer: tauri::Url = "viz://localhost/".parse().unwrap();
        let hub: tauri::Url = "https://hub.example/viz/".parse().unwrap();
        let other_page: tauri::Url = "tauri://localhost/index.html".parse().unwrap();
        let lookalike: tauri::Url = "viz://localhost/settings.html".parse().unwrap();
        let hub_welcome: tauri::Url = "https://hub.example/welcome.html".parse().unwrap();
        for own in [settings_url(), welcome_url()] {
            assert!(own_page_ok("main", &own), "{own}");
            assert!(!own_page_ok("other", &own), "a window that is not the main one");
        }
        assert!(!own_page_ok("main", &viewer), "the viewer in the same window");
        assert!(!own_page_ok("main", &hub));
        assert!(!own_page_ok("main", &other_page), "another page of the app");
        assert!(!own_page_ok("main", &lookalike), "a viewer path that looks alike");
        assert!(!own_page_ok("main", &hub_welcome), "a hub's page named like the app's");
    }

    /// S1, the other half: build.rs declares the commands so none is open by default, and the one
    /// capability that grants them grants every one, to the main window only. The lists are held to
    /// the one in main.rs.
    /// docs/settings-page.md S1: every command the app registers runs the caller check as its first
    /// statement, so a command added later cannot act before it - and the commands the source
    /// declares, the ones `generate_handler!` registers and the list the capability is held to are
    /// one set.
    #[test]
    fn every_command_checks_its_caller_first() {
        let src = super::SRC;
        let mut declared: Vec<String> = Vec::new();
        let mut rest = src;
        while let Some(at) = rest.find("#[tauri::command]\n") {
            rest = &rest[at + "#[tauri::command]\n".len()..];
            let sig_at = rest.find("fn ").expect("a command is a fn");
            let name: String = rest[sig_at + 3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let body_at = rest.find('{').expect("a command has a body");
            let first = rest[body_at + 1..].trim_start();
            assert!(
                first.starts_with("own_page_caller(&webview)?;"),
                "{name} acts before it checks its caller: {}",
                first.lines().next().unwrap_or("")
            );
            declared.push(name);
        }
        let start = src.find("tauri::generate_handler![").expect("the handler list");
        let end = start + src[start..].find(']').expect("its end");
        let mut registered: Vec<String> = src[start + "tauri::generate_handler![".len()..end]
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let mut listed: Vec<String> = APP_COMMANDS.iter().map(|c| c.to_string()).collect();
        declared.sort();
        registered.sort();
        listed.sort();
        assert_eq!(declared.len(), 17, "the scan read the commands");
        assert_eq!(declared, registered, "every command is registered, and only these");
        assert_eq!(declared, listed, "the capability's list is the same set");
    }

    #[test]
    fn settings_commands_are_closed_until_granted() {
        let build = include_str!("../build.rs");
        let settings: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/settings.json")).unwrap();
        let viewer: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(settings["windows"], serde_json::json!(["main"]));
        let granted = |cap: &serde_json::Value, perm: &str| {
            cap["permissions"].as_array().unwrap().iter().any(|p| p.as_str() == Some(perm))
        };
        for cmd in APP_COMMANDS {
            assert!(build.contains(&format!("\"{cmd}\"")), "build.rs must declare {cmd}");
            let perm = format!("allow-{}", cmd.replace('_', "-"));
            assert!(granted(&settings, &perm), "the settings capability needs {perm}");
            assert!(!granted(&viewer, &perm), "{perm} belongs to the settings capability alone");
        }
    }

    /// The narrowing act reaches the daemon as a query string, so a workspace or node name cannot
    /// break out of its parameter: everything but the unreserved characters is encoded.
    #[test]
    fn a_narrowing_is_encoded_into_its_own_parameters() {
        assert_eq!(super::query_value("team-docs_1.x~"), "team-docs_1.x~");
        assert_eq!(super::query_value("a b,c&workspaces=*"), "a%20b%2Cc%26workspaces%3D%2A");
        assert_eq!(super::query_value(""), "");
    }

    /// S5: the settings page renders the text it is given as text. It shows CLI output, which can
    /// quote a server's answer, so no markup sink may appear in its script at all.
    #[test]
    fn the_settings_page_never_renders_markup() {
        // The init script runs in every page the main window shows - the viewer included - and builds
        // the page navigation there, so it is held to the same rule.
        for (file, js) in [
            ("settings.js", include_str!("../assets/settings.js")),
            ("welcome.js", include_str!("../assets/welcome.js")),
            ("kit.js", include_str!("../assets/kit.js")),
            ("shell-init.js", include_str!("../assets/shell-init.js")),
        ] {
            for sink in ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval("]
            {
                assert!(!js.contains(sink), "{file} must not use {sink}");
            }
        }
        let html = include_str!("../assets/settings.html");
        assert!(html.contains("Content-Security-Policy"), "the page carries its own policy");
        assert!(!html.contains("<script>"), "no inline script");
    }

    /// docs/design-system.md U19: a dialog on the app's pages holds focus - Tab stays inside it and the
    /// page behind it is inert until it closes - and every page says its language.
    #[test]
    fn the_app_pages_dialogs_hold_focus() {
        let kit = include_str!("../assets/kit.js");
        let hold = &kit[kit.find("function holdFocus(").expect("holdFocus")..];
        let hold = &hold[..hold.find("\n}\n").expect("it closes")];
        for part in [
            "el.inert = true",
            "el.inert = false",
            "\"Tab\"",
            "e.shiftKey",
            "first.focus()",
            "last.focus()",
        ] {
            assert!(hold.contains(part), "holdFocus does not {part}");
        }
        let settings = include_str!("../assets/settings.js");
        let open = &settings[settings.find("function openDialog(").expect("openDialog")..];
        let open = &open[..open.find("\n}\n").expect("it closes")];
        assert!(
            open.contains("releaseFocus = holdFocus(dialog);"),
            "the Settings dialog does not hold focus"
        );
        let close = &settings[settings.find("function closeDialog(").expect("closeDialog")..];
        let close = &close[..close.find("\n}\n").expect("it closes")];
        assert!(
            close.contains("releaseFocus()"),
            "closing the dialog does not give the page back"
        );
        for (file, html) in [
            ("settings.html", include_str!("../assets/settings.html")),
            ("welcome.html", include_str!("../assets/welcome.html")),
        ] {
            assert!(html.contains("<html lang=\"en\">"), "{file} does not declare its language");
        }
    }

    /// docs/settings-page.md S7 and semantic-retrieval.md Section 13: a daemon that cannot start because
    /// its store was indexed by another model is shown with that reason and Rebuild index, which takes
    /// Restart's place; Rebuild index is on the Search row whenever the daemon runs with a model; the
    /// command stops the daemon the app spawned itself, leaves every other to `embed rebuild`, waits
    /// for a login job the CLI started again, and attaches; and the page follows the re-embedding.
    #[test]
    fn a_store_the_daemon_cannot_open_is_rebuilt_from_the_page() {
        let src = super::SRC;
        let body = |src: &'static str, sig: &str| -> &'static str {
            let at = src.find(sig).unwrap_or_else(|| panic!("no {sig}"));
            &src[at..at + src[at..].find("\n}\n").expect("it closes")]
        };
        let rebuild = body(src, "async fn rebuild_index(");
        for part in [
            "if let Daemon::Spawned(mut child) = prev {",
            "run_cli(&[\"embed\", \"rebuild\"])",
            "wait_for_socket(&sock, Duration::from_secs(30))",
            "bring_up(app.clone(), sock).await;",
        ] {
            assert!(rebuild.contains(part), "the rebuild does not run as the page says: {part}");
        }
        let kill = rebuild.find("child.kill()").expect("the app's own daemon is stopped");
        let cli = rebuild.find("run_cli(&[\"embed\", \"rebuild\"])").expect("the CLI rebuilds");
        assert!(kill < cli, "the app's own daemon is stopped before the CLI resets its store");
        assert!(
            body(src, "async fn settings_state_of(").contains("\"embedder_conflict\": status"),
            "the page is not told why the daemon cannot start"
        );

        let js = include_str!("../assets/settings.js");
        let status = body(js, "function daemonStatus(");
        assert!(
            status.contains("d.embedder_conflict")
                && status.contains("The search index needs rebuilding"),
            "the reason is not shown"
        );
        let daemon = body(js, "function renderDaemon(");
        assert!(
            daemon.contains("conflict ? h(\"div\", { class: \"end\" },")
                && daemon.contains("drift || conflict ? null :"),
            "Rebuild index does not take Restart's place on a store the daemon cannot open"
        );
        assert!(
            body(js, "function searchRow(").contains("() => rebuildIndex())"),
            "the Search row does not offer Rebuild index"
        );
        assert!(
            body(js, "function rebuildIndex(").contains("act(\"rebuild\", \"search_rebuild\"")
                && body(js, "function rebuildIndex(").contains(".then(watchDaemon)"),
            "Rebuild index does not call its command, or does not follow the daemon"
        );
        assert!(
            body(js, "function daemonUnsettled(").contains("|| indexing()")
                && body(js, "function watchDaemon(").contains("if (indexing()) watch.until"),
            "the page does not follow the re-embedding"
        );

        // The starting page says why and opens Settings to rebuild, holding no command itself (S1);
        // Settings starts the rebuild only while the store still cannot open, and drops the ask.
        let conflict = ("hashing-256".to_string(), "<model>".to_string());
        let failed_with = |why: &str| StartPage::Failed(why.to_string());
        let page =
            starting_html(&failed_with("X."), Some(&conflict), &settings_url(), Path::new(LOG));
        for part in [
            "The search index needs rebuilding",
            "Indexed by hashing-256, and the daemon runs &lt;model&gt;",
            "settings.html?act=rebuild#daemon\"",
            ">Rebuild index</a>",
            "supragnosis embed rebuild",
        ] {
            assert!(page.contains(part), "the starting page does not offer the rebuild: {part}");
        }
        assert!(!page.contains("<script"), "the starting page holds a script");
        let failed =
            starting_html(&failed_with("Port in use."), None, &settings_url(), Path::new(LOG));
        assert!(
            failed.contains("settings.html#daemon\"") && failed.contains(">Open This Mac</a>"),
            "a failed start does not open This Mac"
        );
        assert!(
            !starting_html(&StartPage::Starting, None, &settings_url(), Path::new(LOG))
                .contains("</a>"),
            "a start in progress offers a button"
        );
        assert_eq!(
            super::conflict_of(
                &serde_json::json!({"embedder_conflict": {"recorded": "a", "daemon": "b"}})
            ),
            Some(("a".to_string(), "b".to_string()))
        );
        assert_eq!(super::conflict_of(&serde_json::json!({"embedder_conflict": null})), None);
        let asked = body(js, "function actAsked(");
        assert!(
            asked.contains("act === \"rebuild\"")
                && asked.contains("history.replaceState(")
                && asked.contains("d.embedder_conflict) rebuildIndex();"),
            "Settings does not start the rebuild the starting page asked for, or not only on a conflict"
        );
        let welcome = include_str!("../assets/welcome.js");
        assert!(
            welcome.contains("state === \"attention\" && /embed rebuild/.test(say)")
                && welcome.contains("await invoke(\"search_rebuild\")"),
            "Setup's server card does not offer the rebuild"
        );
    }

    /// daemon-lifecycle.md Section 6: a restart of a daemon a login job runs is decided after the
    /// shell waited for the socket. The CLI's success is a restart, and so is a refusal after which a
    /// new process answers - launchd retrying a start that lost the store lock for a moment; a
    /// refusal that left the same process says why; a daemon that never answered is a failure that
    /// names `supragnosis status`. A failed state is healed whenever the state is read, and Settings
    /// follows the daemon after a restart until it answers, for a minute at most.
    #[test]
    fn a_restart_says_what_it_came_to() {
        let refused = || Some((false, "2 managers claim the daemon".to_string()));
        assert_eq!(
            restart_outcome(Some((true, "restarted".into())), Some(1), Some(2), true),
            Ok("restarted".into())
        );
        assert_eq!(
            restart_outcome(Some((false, "exited with status 1".into())), Some(1), Some(2), true),
            Ok("restarted".into()),
            "a refusal that launchd's retry settled is reported as the failure that passed"
        );
        assert_eq!(
            restart_outcome(refused(), Some(1), Some(1), true),
            Err("restart refused: 2 managers claim the daemon".into())
        );
        let late = restart_outcome(refused(), Some(1), None, false).unwrap_err();
        assert!(late.contains("30 seconds") && late.contains("`supragnosis status`"), "{late}");
        assert!(late.contains("2 managers"), "the CLI's reason is dropped: {late}");
        assert!(restart_outcome(Some((true, String::new())), Some(1), None, false).is_err());
        assert!(restart_outcome(None, None, None, true).is_err());
        let st = serde_json::json!({ "managers": [{ "type": "launchd", "pid": 4242 }] });
        assert_eq!(manager_pid(Some(&st)), Some(4242));
        assert_eq!(manager_pid(Some(&serde_json::json!({ "managers": [] }))), None);

        let src = super::SRC;
        let body = |src: &'static str, sig: &str| -> &'static str {
            let at = src.find(sig).unwrap_or_else(|| panic!("no {sig}"));
            &src[at..at + src[at..].find("\n}\n").expect("it closes")]
        };
        for reader in ["async fn refresh_status(", "async fn settings_state_of("] {
            assert!(
                body(src, reader).contains("heal_failed(app).await"),
                "{reader} keeps a stale failure"
            );
        }
        assert!(
            body(src, "async fn restart_daemon(")
                .contains("wait_for_socket(&sock, Duration::from_secs(30))"),
            "a restart races the login job it restarted"
        );
        let js = include_str!("../assets/settings.js");
        assert!(body(js, "function restartDaemon(").contains(".then(watchDaemon)"));
        assert!(body(js, "async function load(").contains("if (daemonUnsettled()) watchDaemon();"));
        let watch = body(js, "function watchDaemon(");
        for part in [
            "if (watch) return;",
            "Date.now() + 60000",
            "setTimeout(tick, 2000)",
            "watch = null; return;",
        ] {
            assert!(watch.contains(part), "the page does not follow the daemon: {part}");
        }
        assert_eq!(
            js.matches("restartDaemon())").count(),
            2,
            "a Restart button does not follow the daemon"
        );
    }

    /// docs/design-system.md Section 6, U26: in the app the title bar drags the window from anywhere
    /// but its controls. Tauri's handler counts a bare data-tauri-drag-region only for a press on that
    /// element itself, and since the bar became a Mac toolbar its two groups cover all of it but the
    /// padding; so the header drags deep, the search's results opt out, and the splash, whose text
    /// covers its middle, drags deep too.
    #[test]
    fn the_title_bar_drags_the_window_but_not_its_controls() {
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("header.setAttribute(\"data-tauri-drag-region\", \"deep\");"));
        assert!(shell.contains("results.setAttribute(\"data-tauri-drag-region\", \"false\")"));
        assert!(
            !shell.contains("h1.setAttribute(\"data-tauri-drag-region\""),
            "a bare attribute on the title stops a press on its scope from dragging"
        );
        assert!(starting_html(&StartPage::Starting, None, &settings_url(), Path::new(LOG))
            .contains("<body data-tauri-drag-region=\"deep\""));
        let src = super::SRC;
        let pages = &src[..src.find("#[cfg(test)]").expect("the tests")];
        assert_eq!(pages.matches("<body data-tauri-drag-region=\"deep\"").count(), 1);
        assert!(!pages.contains("<body data-tauri-drag-region style="));
    }

    /// docs/design-system.md Section 29.5, P24: the page the window shows while the daemon does not
    /// answer says which of three things is true, from the shell's state, and has no timer of its
    /// own. A failure names its cause in one sentence and offers Restart, the default, Diagnostics and
    /// Open This Mac - each a link to Settings, no command of its own (S1) - and the log's path in a
    /// field; a start the shell made says nothing to press for 10 seconds, and then that it is still
    /// starting while the child it spawned lives.
    #[test]
    fn a_daemon_that_cannot_start_says_so() {
        let mut starting = Daemon::Starting;
        assert_eq!(start_page(&mut starting, Some(Duration::from_secs(3))), StartPage::Starting);
        assert_eq!(
            start_page(&mut starting, Some(Duration::from_secs(12))),
            StartPage::StillStarting
        );
        assert_eq!(start_page(&mut starting, None), StartPage::StillStarting);
        let mut alive =
            Daemon::Spawned(std::process::Command::new("sleep").arg("5").spawn().unwrap());
        assert_eq!(start_page(&mut alive, Some(Duration::from_secs(30))), StartPage::StillStarting);
        if let Daemon::Spawned(c) = &mut alive {
            let _ = c.kill();
            let _ = c.wait();
        }
        assert_eq!(
            start_page(&mut alive, None),
            StartPage::Failed("The daemon exited.".to_string())
        );
        assert_eq!(start_page(&mut Daemon::External, None), StartPage::Stopped);
        let stopped = starting_html(&StartPage::Stopped, None, &settings_url(), Path::new(LOG));
        assert!(stopped.contains("Supragnosis stopped") && stopped.contains(">Restart</a>"));
        let why = "the spawned daemon exited immediately (exit status: 1): store locked - full log: ~/.supragnosis/log/app-daemon.err.log";
        assert_eq!(
            start_page(&mut Daemon::Failed(why.to_string()), None),
            StartPage::Failed(
                "The spawned daemon exited immediately (exit status: 1): store locked.".to_string()
            )
        );
        assert_eq!(cause_of("port in use.\nmore"), "Port in use.");
        assert_eq!(cause_of(""), "The daemon did not start.");

        let log = Path::new(LOG);
        let failed = starting_html(
            &StartPage::Failed("Port in use.".to_string()),
            None,
            &settings_url(),
            log,
        );
        for part in [
            "Supragnosis could not start",
            "Port in use.",
            "settings.html?act=restart#daemon\" autofocus",
            ">Restart</a>",
            "settings.html?act=diagnostics#daemon\"",
            ">Diagnostics</a>",
            "settings.html#daemon\"",
            ">Open This Mac</a>",
            "<input class=\"lf\" readonly value=\"/nowhere/.supragnosis/log/app-daemon.err.log\"",
            "settings.html?act=reveal-log#daemon\"",
            ">Show in Finder</a>",
            "@media (prefers-reduced-motion: reduce) { .spin { animation:none } }",
        ] {
            assert!(failed.contains(part), "a failed start does not say: {part}");
        }
        let still = starting_html(&StartPage::StillStarting, None, &settings_url(), log);
        assert!(
            still.contains("Still starting.")
                && still.contains(">Open This Mac</a>")
                && still.contains("<input")
        );
        assert!(!still.contains("act=restart"), "a start in progress offers Restart");
        let first = starting_html(&StartPage::Starting, None, &settings_url(), log);
        assert!(first.contains("Starting Supragnosis...") && !first.contains("<input"));
        for page in [&failed, &still, &first] {
            assert!(!page.contains("http-equiv"), "the starting page reloads itself");
            assert!(!page.contains("<script"), "the starting page holds a script");
            assert!(!page.contains("starting the supragnosis daemon"), "the old heading is back");
        }

        // The shell changes the page: it watches while the page is drawn and reloads when the daemon
        // answers or the page should say something else, and forgets it once the graph is served.
        let src = super::SRC;
        let watch = &src[src.find("async fn watch_splash(").expect("watch_splash")..];
        let watch = &watch[..watch.find("\n}\n").expect("it closes")];
        assert!(watch.contains("Duration::from_secs(1)") && watch.contains("reload_viewer"));
        assert!(
            src.contains("*SPLASH.lock().unwrap() = None;"),
            "the graph's page does not end the watch"
        );

        // Restart of a failed daemon goes through `supragnosis restart` when a manager owns it.
        assert!(src.contains("prev @ (Daemon::External | Daemon::Failed(_)) if matches!(prev, Daemon::External) || job =>"));
        assert!(managed(&serde_json::json!({"managers": [{"label": "launchd"}]})));
        assert!(!managed(&serde_json::json!({"managers": []})));
        assert!(!managed(&serde_json::json!({})));

        // Settings acts on what the page asks, once, and only while that act still fits.
        let js = include_str!("../assets/settings.js");
        let asked = &js[js.find("function actAsked(").expect("actAsked")..];
        let asked = &asked[..asked.find("\n}\n").expect("it closes")];
        for part in [
            "keep.delete(\"act\")",
            "history.replaceState(",
            "if (!d.answering && d.shell !== \"starting\") restartDaemon();",
            "\"Not restarted: the daemon answers\"",
            "\"Not restarted: the daemon is still starting\"",
            "document.getElementById(\"copyDiagnostics\")",
            "revealFile(\"log\")",
        ] {
            assert!(asked.contains(part), "Settings does not act on the page's ask: {part}");
        }
        // Show in Finder shows one of two files, for the app's own pages only.
        let reveal = &src[src.find("async fn reveal(").expect("reveal")..];
        let reveal = &reveal[..reveal.find("\n}\n").expect("it closes")];
        assert!(reveal.contains("own_page_caller(&webview)?;"));
        assert!(reveal.contains("\"log\" => daemon_log(),") && reveal.contains("\"config\" =>"));
        assert!(reveal.contains("other => return Err("));
        assert!(APP_COMMANDS.contains(&"reveal"));
        assert!(include_str!("../capabilities/settings.json").contains("\"allow-reveal\""));
        // Diagnostics carries the shell's status and the log's last lines; This Mac shows the log.
        let diag = &src[src.find("async fn diagnostics(").expect("diagnostics")..];
        let diag = &diag[..diag.find("\n}\n").expect("it closes")];
        assert!(diag.contains("status_line()") && diag.contains("tail -n 40"));
        assert!(js.contains("id: \"copyDiagnostics\"") && js.contains("\"data-k\": \"log\""));
    }

    /// design-system.md Section 29.10: Try it quotes the entry in the question's block and opens the
    /// graph on its entity when it names one; the waiting sentence is setup's, with the question set
    /// apart; the page counts only how long it has waited (O4), to show the minutes, the checks the
    /// event carries and the foot's Copy Diagnostics; the entry's arrival is announced; and the
    /// shell gives the viewer Setup's address for an empty store.
    #[test]
    fn try_it_quotes_the_entry_and_counts_its_wait() {
        let js = include_str!("../assets/welcome.js");
        let body = |name: &str| -> &str {
            let at = js.find(&format!("\nfunction {name}(")).unwrap_or_else(|| panic!("no {name}"));
            &js[at + 1..at + 1 + js[at + 1..].find("\n}\n").expect("it closes")]
        };
        let first = body("firstBody");
        for part in [
            "h(\"div\", { class: \"ask entry\" }, h(\"q\", { class: \"selectable\", text: e.quote }))",
            "if (e.entity) {",
            "openGraph(e.id, e.entity)",
            "\"Show in the Graph\"",
            "const lead = m ? say.slice(0, m.index).trim() : \"\";",
            "mins >= 2 && Array.isArray(e.checks)",
            "/^No connected app has reached the server/",
        ] {
            assert!(first.contains(part), "firstBody lacks {part}");
        }
        assert!(!js.contains("The entry appears here"), "the page no longer promises it");
        assert!(body("openGraph").contains("\"entity=\" + encodeURIComponent(entity)"));
        let check = &js[js.find("async function check()").unwrap()..];
        let check = &check[..check.find("\n}\n").unwrap()];
        assert!(check.contains("announce(\"First entry recorded in \""));
        assert!(
            check.contains("firstWaitSince = Date.now()"),
            "the wait counts from the first wait seen"
        );
        assert!(
            body("renderActions").contains("|| waitedMinutes() >= 2"),
            "Copy Diagnostics after two minutes"
        );
        // The clock is the one thing the page counts; it never decides a state.
        assert_eq!(
            js.matches("Date.now()").count(),
            2,
            "the wait's start and its minutes, nothing else"
        );
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("window.supragnosisSetup = \"tauri://localhost/welcome.html\""));
    }

    /// design-system.md Section 29.10 and onboarding.md O6: the page and Copy Diagnostics read the
    /// same `setup --check --events` line, so the report drops the first entry's words, its app and
    /// its entity, and keeps everything else as setup wrote it.
    #[test]
    fn copy_diagnostics_carries_no_entry() {
        let line = r#"{"step":"first","id":"billing","state":"done","say":"An entry is in \"billing\", just now.","quote":"the launch is friday","app":"Claude Code","entity":"e1"}"#;
        let out = without_entry(&format!("{{\"schema\":1}}\n{line}\nnot json\n"));
        assert!(
            !out.contains("friday") && !out.contains("Claude Code") && !out.contains("\"e1\""),
            "{out}"
        );
        // The done sentence is copied without the workspace or the app it named (Section 29.13).
        assert!(
            out.contains("An entry is in the store.")
                && !out.contains("billing")
                && out.contains("not json")
        );
        assert!(out.ends_with('\n'));
        let src = super::SRC;
        let diag = &src[src.find("async fn diagnostics(").expect("diagnostics")..];
        assert!(
            diag[..diag.find("\n}\n").unwrap()].contains("without_entry(&text(run_cli(&args)))")
        );
    }

    /// docs/design-system.md U19: a page that draws itself again keeps the control that had focus.
    /// Setup and Settings draw through `kit.js`'s `keepFocus`, which finds the control again by the
    /// key its row carries and its name, and keeps a field's selection; the reads that come on a
    /// timer or on the window's focus draw only what changed, and a streamed line is added to its
    /// open drawer in place.
    #[test]
    fn the_app_pages_keep_focus_across_a_draw() {
        let kit = include_str!("../assets/kit.js");
        let keep = &kit[kit.find("function keepFocus(").expect("keepFocus")..];
        let keep = &keep[..keep.find("\n}\n").expect("it closes")];
        for part in [
            "focusKey(el, true)",
            "focusKey(el, false)",
            "near.length === 1",
            "setSelectionRange(",
            ".focus(",
        ] {
            assert!(keep.contains(part), "keepFocus does not: {part}");
        }
        assert!(
            kit.contains("el.closest(\"[data-k], [id]\")"),
            "a control is not found by its row"
        );
        let settings = include_str!("../assets/settings.js");
        let welcome = include_str!("../assets/welcome.js");
        for (page, js) in [("Settings", settings), ("Setup", welcome)] {
            assert!(
                js.contains("function render() { keepFocus(document.body, draw); }"),
                "{page} draws without keeping focus"
            );
        }
        for key in [
            "\"capture:\" + a.id",
            "\"data-k\": \"daemon\"",
            "\"data-k\": \"model\"",
            "\"hub:\" + hub.url",
            "\"peer:\" + id",
        ] {
            assert!(settings.contains(key), "a Settings row has no key: {key}");
        }
        assert!(welcome.contains("\"data-k\": \"step:\" + s.step"), "a Setup card has no key");
        // An AI app's row, on either page, is kit.js's, which keys it.
        assert!(kit.contains("\"data-k\": \"app:\" + row.id"), "an app row has no key");
        assert!(settings.contains("if (!quiet || now !== drawnFrom) render();"));
        assert!(settings.contains("load(true);"), "the window's focus draws what did not change");
        assert!(settings.contains("await load(true);"), "the watch draws what did not change");
        assert!(
            welcome.contains("if (now === checkedFrom) return;"),
            "Setup's check draws what did not change"
        );
        assert!(
            welcome.contains(
                "open.append(h(\"div\", { class: \"l-\" + entry.kind, text: entry.text }));"
            ),
            "a streamed line draws the page"
        );
    }

    /// docs/design-system.md Section 29.29 and settings-page.md S8: the main window's pages get the
    /// platform's events, its full-screen getter and the title bar's drag, and nothing else - no
    /// menu, tray, image, path or resource, which any page the window shows could otherwise use to
    /// speak with the app's voice. Each page API the shell calls is held to a grant that covers it.
    #[test]
    fn the_window_pages_get_only_what_they_use() {
        let caps: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(caps["windows"], serde_json::json!(["main"]));
        let granted: Vec<&str> = caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .collect();
        assert_eq!(
            granted,
            [
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:event:allow-emit",
                "core:window:allow-is-fullscreen",
                "core:window:allow-start-dragging",
                "core:window:allow-internal-toggle-maximize",
            ],
            "exactly what the pages use"
        );
        // Every platform call a page makes, mapped to the grant that covers it - a new one fails
        // here until it is named, rather than being refused silently at run time.
        let allowed: [(&str, &str); 4] = [
            ("__TAURI__.event.listen", "core:event:allow-listen"),
            ("__TAURI__.event.emit", "core:event:allow-emit"),
            ("__TAURI__.core.invoke", "the settings capability's commands (S1)"),
            (
                "__TAURI__.window.getCurrentWindow().isFullscreen",
                "core:window:allow-is-fullscreen",
            ),
        ];
        let pages = [
            ("shell-init.js", include_str!("../assets/shell-init.js")),
            ("settings.js", include_str!("../assets/settings.js")),
            ("welcome.js", include_str!("../assets/welcome.js")),
            ("kit.js", include_str!("../assets/kit.js")),
        ];
        let mut calls = 0;
        for (file, text) in pages {
            // Calls read across line breaks: `window.__TAURI__.event\n  .listen(` is one call.
            let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            let mut rest = flat.as_str();
            while let Some(at) = rest.find("__TAURI__.") {
                let call: String = rest[at..]
                    .chars()
                    .take_while(|c| {
                        c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '(' || *c == ')'
                    })
                    .collect();
                let call = call.trim_end_matches('(').to_string();
                assert!(
                    call == "__TAURI__." || allowed.iter().any(|(api, _)| call.starts_with(api)),
                    "{file} calls {call}, which no grant names"
                );
                calls += 1;
                rest = &rest[at + "__TAURI__.".len()..];
            }
            assert!(!text.contains("__TAURI_INTERNALS__"), "{file} calls the platform's internals");
        }
        assert!(calls >= 7, "the scan reads the pages' calls, {calls} of them");
        for (_, grant) in &allowed[..2] {
            assert!(granted.contains(grant), "{grant} is named but not granted");
        }
        assert!(granted.contains(&allowed[3].1));
        // The viewer runs in the same window and calls the platform not at all. Its script is the
        // parts the viz crate's build joins (crates/supragnosis-viz/build.rs); this workspace does
        // not reach that build, so every part is read here.
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/supragnosis-viz/assets/viewer");
        let parts: Vec<String> = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {dir}: {e}"))
            .map(|e| e.expect("a directory entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "js"))
            .map(|p| std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .collect();
        assert!(!parts.is_empty(), "no part of the viewer script was read");
        for viewer in parts.iter().map(String::as_str).chain([
            include_str!("../../crates/supragnosis-viz/assets/viewer-sim.js"),
            include_str!("../../crates/supragnosis-viz/assets/viewer-gl.js"),
        ]) {
            assert!(!viewer.contains("__TAURI"), "the viewer calls the platform");
        }
        // The title bar's drag and double-click zoom are the drag region's two window calls.
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("header.setAttribute(\"data-tauri-drag-region\", \"deep\")"));
        assert!(shell.replace(['\n', ' '], "").contains("event.listen(\"viz-event\""));
    }

    /// docs/design-system.md Section 6, U26: the bar keeps room for the traffic lights only while
    /// they are shown. The shell announces each full-screen transition, and a page asks the window on
    /// load, since one opened while the window is already full screen saw no transition and kept the
    /// room; the first answer lands without the padding's animation. Every page links the one
    /// stylesheet that holds the rule.
    #[test]
    fn the_title_bar_clears_the_lights_only_while_they_are_shown() {
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("event.listen(\"shell-fullscreen\""), "transitions are not heard");
        assert!(
            shell.contains("getCurrentWindow().isFullscreen()"),
            "a page loaded in full screen does not ask"
        );
        let css = include_str!("../assets/shell.css");
        assert!(css.contains("html:not(.shell-fullscreen) header { padding-left: 84px; }"));
        assert!(css.contains("html.shell-asking header { transition: none; }"));
        // The shell's side, read from this file: the test's own copy of the line is escaped, so it
        // cannot be what matches.
        assert!(super::SRC.contains("window.emit(\"shell-fullscreen\", fs)"));
        // The window grants the full-screen getter by name (design-system.md Section 29.29).
        let caps = include_str!("../capabilities/default.json");
        assert!(caps.contains("\"core:window:allow-is-fullscreen\""));
        for (file, html) in [
            ("settings.html", include_str!("../assets/settings.html")),
            ("welcome.html", include_str!("../assets/welcome.html")),
        ] {
            assert!(
                html.contains("<link rel=\"stylesheet\" href=\"shell.css\" data-shell>"),
                "{file}"
            );
        }
    }

    /// docs/design-system.md Section 10, U19: a button that shows only an icon is named by its act and
    /// its object.
    #[test]
    fn the_settings_page_names_its_buttons_and_its_errors() {
        let js = include_str!("../assets/settings.js");
        let body = |name: &str| {
            let at = js.find(&format!("function {name}(")).expect(name);
            &js[at..at + js[at..].find("\n}\n").expect("it closes")]
        };
        assert!(
            body("actionButton").contains("\"aria-label\""),
            "an icon button cannot be named"
        );
    }

    /// docs/design-system.md Section 6a: the first-run page is Setup wherever it is reached, every
    /// sentence on it is one the code keeps, and its progress is said - as a progress bar a screen
    /// reader reads, in the foot while the plan runs, in a live region as each step ends, and in the
    /// heading once the essentials are done.
    /// design-system.md Section 14 step 17: every checkbox and radio on the app's pages takes the
    /// accent, Setup's privacy line keeps a reading width, and the starting splash is drawn in the
    /// tokens and the reading face rather than the old mono and colors.
    #[test]
    fn the_third_pass_defects_on_the_apps_pages() {
        let css = include_str!("../assets/settings.css");
        let welcome = include_str!("../assets/welcome.css");
        let main = super::SRC;
        assert!(
            css.contains(":where(input[type=\"checkbox\"], input[type=\"radio\"]) { accent-color: var(--accent); }"),
            "a checkbox can take the system's blue beside a gold one"
        );
        assert!(
            welcome.contains("min-height: 24px; border: 0;"),
            "Setup's Details is under 24 pixels"
        );
        assert!(
            welcome.contains("max-width: 34em"),
            "the privacy line runs past a reading width"
        );
        let splash: Vec<&str> = main
            .split("r#\"<!doctype html>")
            .skip(1)
            .map(|page| &page[..page.find("</body>").expect("a splash ends its body")])
            .collect();
        assert_eq!(splash.len(), 1, "the shell authors one splash page");
        for page in splash {
            assert!(
                !page.contains("monospace") && !page.contains("#7b8494"),
                "a splash keeps the old look"
            );
        }
    }

    /// design-system.md Section 10 and U21: View sets the root's text size from 100% to 200%, every
    /// rem size follows it, the choice is kept, and each page the window loads is given it. The
    /// webview itself is not zoomed, so the title bar keeps its height and its traffic lights.
    #[test]
    fn view_sets_the_root_text_size() {
        let main = super::SRC;
        for part in [
            "\"Make Text Bigger\"",
            "Some(\"CmdOrCtrl+=\")",
            "\"Make Text Smaller\"",
            "Some(\"CmdOrCtrl+-\")",
            "\"Actual Size\"",
            "Some(\"CmdOrCtrl+0\")",
            "fn text_scale_script(",
            ".on_page_load(",
            "\"view.json\"",
        ] {
            assert!(main.contains(part), "the root text size is missing {part}");
        }
        assert!(
            !main.contains(concat!(".zoom_hotkeys", "_enabled(true)")),
            "the webview zooms, title bar and all"
        );
        assert_eq!(super::step_text_scale(1.0, 1), 1.125);
        assert_eq!(super::step_text_scale(2.0, 1), 2.0);
        assert_eq!(super::step_text_scale(1.0, -1), 1.0);
        assert_eq!(super::step_text_scale(1.5, 0), 1.0);
        assert!(super::text_scale_script(1.25).contains("fontSize = \"125%\""));
    }

    #[test]
    fn the_setup_page_says_what_is_true_and_how_far_it_is() {
        let html = include_str!("../assets/welcome.html");
        let js = include_str!("../assets/welcome.js");
        let settings = include_str!("../assets/settings.js");
        let main = super::SRC;
        assert!(
            html.contains("<h1 id=\"pageTitle\">Setup</h1>"),
            "the title bar does not say Setup"
        );
        assert!(
            html.contains(
                "Supragnosis keeps your memory on this Mac. It is shared only if you set up sync."
            ),
            "the privacy line promises what the code does not keep"
        );
        assert!(
            !js.contains("Finish setup in the menu bar"),
            "the page points at a tray item that goes away"
        );
        assert!(js.contains("Open Setup"), "the page does not name the way back");
        assert!(
            settings.contains("\"Open Setup\"") && !settings.contains("Run setup again"),
            "About does not say Open Setup"
        );
        assert!(main.contains("\"Finish Setup...\""), "the tray does not say Finish Setup...");
        for part in [
            "role: \"progressbar\"",
            "\"aria-valuenow\"",
            "Setting up... ",
            "Supragnosis is set up",
            "function announce(",
        ] {
            assert!(js.contains(part), "the page does not say its progress: {part}");
        }
        assert!(
            html.contains("id=\"announce\"") && html.contains("aria-live=\"polite\""),
            "there is no live region for a step's end"
        );
    }

    /// settings-page.md Section 3.3: the model download runs outside the page's one-act lock, so the
    /// rest of the page stays usable for its minutes; its row shows its progress and can stop it.
    #[test]
    fn the_download_does_not_lock_the_settings_page() {
        let js = include_str!("../assets/settings.js");
        let body = |name: &str| {
            let at = js.find(&format!("function {name}(")).expect(name);
            &js[at..at + js[at..].find("\n}\n").expect("it closes")]
        };
        let row = body("searchRow");
        assert!(
            !row.contains("act(\"model\"") && !row.contains("actionButton(\"model\""),
            "the download takes the page's lock"
        );
        assert!(
            row.contains("role: \"progressbar\"") && row.contains("\"Stop\""),
            "the download's row has no progress or Stop"
        );
        assert!(body("downloadModel").contains("invoke(\"setup_run\", { step: \"model\" })"));
        assert!(body("stopDownload").contains("invoke(\"setup_stop\", { step: \"model\" })"));
    }

    /// design-system.md Section 6a, onboarding.md O8: Setup's plan is checkboxes, each row its label;
    /// replacing what the person set up is in the plan and starts unchecked, so nothing changes until
    /// Set up; and Set up is the default button, first in tab order, which Return presses.
    #[test]
    fn the_setup_plan_is_checkboxes_and_set_up_is_the_default() {
        let js = include_str!("../assets/welcome.js");
        let css = include_str!("../assets/welcome.css");
        assert!(!js.contains("function sw("), "the plan still mixes switches with checkboxes");
        assert!(
            js.contains("replace: {}, takeover: false"),
            "replacing items do not start unchecked"
        );
        assert!(
            js.contains("checked: !!plan.replace[a.id]"),
            "an app's replacement is not a plan item"
        );
        assert!(
            js.contains("pick: !!tick")
                && include_str!("../assets/kit.js").contains("h(row.pick ? \"label\" : \"div\""),
            "a plan row is not its checkbox's label"
        );
        assert!(
            js.contains("if (takingOver) await run(\"takeover\""),
            "Set up does not apply the take-over it was given"
        );
        assert!(
            js.contains("document.getElementById(\"setup-go\")")
                && js.contains("e.key === \"Enter\""),
            "Return does not press Set up"
        );
        assert!(
            css.contains("flex-direction: row-reverse"),
            "Set up is not first in tab order and rightmost"
        );
    }

    /// design-system.md Section 14 step 13: a step the person started and left is not lost. The shell
    /// answers what is running, and Setup and Settings show it as running and wait for it; a second
    /// start of a running step says it is already running instead of reporting a failure.
    #[test]
    fn a_running_step_survives_leaving_its_page() {
        let welcome = include_str!("../assets/welcome.js");
        let settings = include_str!("../assets/settings.js");
        assert!(
            super::APP_COMMANDS.contains(&"setup_running"),
            "the shell cannot be asked what runs"
        );
        assert!(include_str!("../capabilities/settings.json").contains("\"allow-setup-running\""));
        assert!(
            welcome.contains("loadInfo().then(check).then(adoptRunning);"),
            "Setup does not pick up a running step"
        );
        assert!(
            welcome.contains("/already running/.test(String(e))"),
            "a second start reads as a failure"
        );
        assert!(
            settings.contains("load().then(adoptDownload).then(actAsked);"),
            "Settings does not pick up a running download"
        );
    }

    /// design-system.md Section 6a: the Settings page says each fact once. Version drift is one card
    /// with the one Restart; a section's dot appears only when it needs attention, in words a screen
    /// reader reads; a peer is one row.
    #[test]
    fn the_settings_page_says_each_fact_once() {
        let js = include_str!("../assets/settings.js");
        assert!(
            js.contains("\"Update ready - restart to load \" + d.installed"),
            "drift is not one card"
        );
        // A store the daemon cannot open takes Restart away too: its card holds Rebuild index (S7).
        assert!(
            js.contains("drift || conflict ? null :"),
            "the startup row's Restart repeats the drift card's"
        );
        assert!(
            !js.contains("Restart the daemon to load the new version"),
            "the drift callout is still there"
        );
        assert!(
            js.contains("return kind === \"warn\" || kind === \"bad\" ? kind : null;"),
            "every section carries a dot"
        );
        assert!(js.contains("class: \"sr-only\""), "a dot says nothing to a screen reader");
        assert!(
            js.contains(
                "new Set([...admitted.map((p) => p.node_id), ...known.map((p) => p.node_id)])"
            ),
            "a peer is listed twice"
        );
    }

    /// design-system.md Section 6a: Claude Code's capture hook is a switch on its row while it is
    /// connected through the bridge, and nowhere else; on and off are `connect claude-code` with and
    /// without `--no-capture`.
    #[test]
    fn the_capture_switch_shows_only_on_a_bridged_row() {
        let js = include_str!("../assets/settings.js");
        assert!(
            js.contains(
                "if (a.id !== \"claude-code\" || typeof a.capture !== \"boolean\") return row;"
            ),
            "the switch shows on a row it cannot act on"
        );
        assert!(js.contains("act(\"capture\", \"capture_set\", { on: e.target.checked }"));
        // The switch is a second row after Claude Code's; the rows are flattened into the card, and
        // the element helper flattens any depth, so neither row becomes "[object HTMLDivElement]".
        assert!(
            js.contains("const rows = state.apps.flatMap((a) => {"),
            "Claude Code's two rows are nested in the card's children"
        );
        assert!(
            include_str!("../assets/kit.js").contains("for (const c of children.flat(Infinity)) {"),
            "a nested list of children is turned into text"
        );
        assert!(super::APP_COMMANDS.contains(&"capture_set"));
        assert!(include_str!("../capabilities/settings.json").contains("\"allow-capture-set\""));
        let main = super::SRC;
        let at = main.find("async fn capture_set(").expect("capture_set");
        let body = &main[at..at + main[at..].find("\n}\n").expect("it closes")];
        assert!(body.contains("\"--no-capture\"") && body.contains("\"claude-code\""));
        assert!(
            main.contains(".filter(|r| r[\"entry\"].as_str() == Some(\"bridge\"))"),
            "capture is read from a row not bridged"
        );
    }

    /// Settings > About names where the source lives, and its hint points there for the dependency
    /// licences. The value is the package manifest's `repository`, which this crate has to inherit
    /// from its own workspace by name; when the shell left the server workspace it stopped doing
    /// so, and every build until 0.5.2 showed "-" in that row.
    /// The tray's Review item counts what waits for a person (docs/design-system.md Section 7): open
    /// proposals and blocked ones, which wait to be rejected, never merged, rejected or withdrawn
    /// ones; and from the curation report the suspected secrets and the contested beliefs, all of
    /// Needs you. Without the report it says what it counted (P5).
    #[test]
    fn the_review_item_counts_open_and_blocked_proposals() {
        let list = serde_json::json!([
            {"state": "open"}, {"state": "blocked"}, {"state": "blocked"},
            {"state": "merged"}, {"state": "rejected"}, {"state": "withdrawn"}
        ]);
        assert_eq!(super::review_count(&list), 3);
        assert_eq!(super::review_count(&serde_json::json!({"error": "x"})), 0);
        let report = serde_json::json!({
            "secrets": [{"observation": "s"}],
            "contradictions": [
                {"id": "a", "field": "kind", "contested": true},
                {"id": "type:ws:entity:Service", "field": "definition", "contested": true},
                {"id": "b", "field": "kind", "contested": false}
            ]
        });
        assert_eq!(super::curation_count(&report), Some(3), "a secret and two contested beliefs");
        assert_eq!(super::curation_count(&serde_json::json!({"error": "x"})), None);
        let text = |proposals, needs| super::review_item_text(&super::Waiting { proposals, needs });
        assert_eq!(text(3, Some(6)).as_deref(), Some("Review - 6 need you..."));
        assert_eq!(text(0, Some(1)).as_deref(), Some("Review - 1 needs you..."));
        assert_eq!(text(2, None).as_deref(), Some("Review - 2 proposals wait..."));
        assert_eq!(text(1, None).as_deref(), Some("Review - 1 proposal waits..."));
        assert_eq!(text(0, Some(0)), None);
        assert_eq!(text(0, None), None);
    }

    /// design-system.md Section 29.4: the way back is the viewer's address an app page carries in
    /// `?from=`, its `#review` dropped, or the viewer's own address; nothing else is followed, so a
    /// page cannot make "< Graph" a redirect (settings-page.md Section 4).
    #[test]
    fn the_way_back_goes_only_to_the_viewer() {
        let viewer = super::graph_url();
        let mut left = viewer.clone();
        left.query_pairs_mut()
            .append_pair("workspace", "billing")
            .append_pair("entity", "e1");
        let settings = super::with_from(super::settings_url(), Some(&left));
        assert_eq!(super::graph_left(&settings).as_ref(), Some(&left), "the way back is lost");
        let mut on_review = left.clone();
        on_review.set_fragment(Some("review"));
        assert_eq!(super::graph_left(&on_review).as_ref(), Some(&left), "#review is kept");
        // Passed on from one app page to the other.
        let setup = super::with_from(super::welcome_url(), Some(&settings));
        assert_eq!(super::graph_left(&setup).as_ref(), Some(&left));
        let mut evil = super::settings_url();
        evil.query_pairs_mut().append_pair("from", "https://example.com/");
        assert_eq!(super::graph_left(&evil), None, "an address elsewhere is followed");
        assert_eq!(super::graph_left(&super::settings_url()), None);
    }

    #[test]
    fn the_about_section_names_the_source() {
        assert!(
            env!("CARGO_PKG_REPOSITORY").starts_with("https://github.com/"),
            "app/Cargo.toml [package] must inherit repository from [workspace.package]"
        );
    }
}

/// docs/onboarding.md Section 6: when the Welcome page opens, when the tray offers to finish, what
/// the page reads from setup, and which acts are the page's own (O4, O6).
#[cfg(test)]
mod welcome_tests {
    use super::{
        essentials, finish_setup_offered, setup_events, welcome_at_launch, WelcomePrefs,
        APP_COMMANDS, SETUP_TOO_OLD,
    };

    fn output(stdout: &str, stderr: &str, code: i32) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// The first launch opens the page; a later one opens it while the daemon or the apps step waits,
    /// unless the person chose "Finish later". Start at Login turned off settles the daemon step their
    /// way, for the launch and the tray alike. Steps setup could not report open nothing.
    #[test]
    fn the_welcome_page_opens_on_the_first_launch_and_while_a_step_waits() {
        let first = WelcomePrefs::default();
        let seen = WelcomePrefs { seen: true, ..first };
        assert!(welcome_at_launch(first, None), "the first launch");
        assert!(!welcome_at_launch(seen, None), "setup did not answer");
        assert!(!welcome_at_launch(seen, Some((true, true))), "both steps done");
        assert!(welcome_at_launch(seen, Some((false, true))), "the daemon is not kept running");
        assert!(welcome_at_launch(seen, Some((true, false))), "no app is connected");
        let later = WelcomePrefs { later: true, ..seen };
        assert!(!welcome_at_launch(later, Some((false, false))), "Finish later");
        assert!(finish_setup_offered(later, Some((false, false))), "the tray still offers it");
        let off = WelcomePrefs { login_off: true, ..seen };
        assert!(!welcome_at_launch(off, Some((false, true))), "Start at Login off by choice");
        assert!(!finish_setup_offered(off, Some((false, true))));
        assert!(finish_setup_offered(off, Some((false, false))), "the apps step still waits");
        assert!(!finish_setup_offered(seen, None));
        assert_eq!(WelcomePrefs::from_json(&off.to_json()), off, "what is remembered reads back");
    }

    /// The page reads setup's events as the CLI's example has them, and the two steps the launch and
    /// the tray wait on come from the step rows, never from an app's row or the summary.
    #[test]
    fn setup_events_read_as_the_clis_example_has_them() {
        let example =
            include_str!("../../crates/supragnosis-cli/tests/fixtures/json/setup-events.jsonl");
        let events = setup_events(&output(example, "", 0)).expect("the example reads");
        assert_eq!(events.len(), example.lines().count() - 1, "every line after the schema");
        assert!(events.iter().any(|e| e["step"] == "first" && e["id"] == "billing"));
        // The example is a stream, a step's lines in the order they came; a checklist has each step
        // once, as it stands at the end.
        let mut checklist: Vec<serde_json::Value> = Vec::new();
        for e in events.iter().rev() {
            if !checklist.iter().any(|c| c["step"] == e["step"] && c.get("id") == e.get("id")) {
                checklist.push(e.clone());
            }
        }
        assert_eq!(essentials(&checklist), (true, false), "the daemon kept running, no app done");
        let first_seen: Vec<serde_json::Value> = events
            .iter()
            .filter(|e| e["step"] == "server" && e["state"] == "todo")
            .cloned()
            .collect();
        assert!(!essentials(&first_seen).0, "a server step that is not done");
    }

    /// A CLI before setup, one newer than the app, and one that failed each say so, and none is read.
    #[test]
    fn setup_events_say_why_there_are_none() {
        assert_eq!(
            setup_events(&output("", "error: unrecognized subcommand 'setup'", 2)),
            Err(SETUP_TOO_OLD.to_string())
        );
        let newer = setup_events(&output("{\"schema\": 2}\n{\"step\":\"server\"}\n", "", 0));
        assert!(newer.is_err_and(|e| e.contains("newer than the app")));
        assert_eq!(
            setup_events(&output("", "Error: the server profile could not be read: x", 1)),
            Err("the server profile could not be read: x".to_string())
        );
    }

    /// O4: the Welcome page's acts are setup's steps, the two helpers, asking setup again, and the
    /// choices the app remembers - every command it calls is one of those, and every command either
    /// page calls is one the app declares. O6: the page says that nothing leaves this Mac.
    #[test]
    fn the_welcome_page_calls_only_its_own_acts() {
        let called = |js: &str| -> Vec<String> {
            js.split("invoke(\"")
                .skip(1)
                .map(|rest| rest[..rest.find('"').expect("a closing quote")].to_string())
                .collect()
        };
        let welcome = called(include_str!("../assets/welcome.js"));
        let own = [
            "setup_check",
            "setup_run",
            "setup_stop",
            "setup_running",
            "welcome_state",
            "welcome_set",
            "app_helper",
            "app_toggle",
            "clipboard_copy",
            "diagnostics",
            // O4: the server step's Rebuild index, for a store indexed by another model.
            "search_rebuild",
        ];
        assert!(welcome.len() >= 8, "the page's calls were read: {welcome:?}");
        for cmd in &welcome {
            assert!(
                own.contains(&cmd.as_str()),
                "welcome.js calls {cmd}, which is not one of its acts"
            );
        }
        for cmd in welcome.iter().chain(&called(include_str!("../assets/settings.js"))) {
            assert!(APP_COMMANDS.contains(&cmd.as_str()), "{cmd} is called but not declared");
        }
        // O6: the page says what stays on this Mac, in words the code keeps (design-system.md 6a).
        assert!(include_str!("../assets/welcome.html").contains(
            "Supragnosis keeps your memory on this Mac. It is shared only if you set up sync."
        ));
    }
}

/// docs/compatibility.md Section 6, the app's side. The CLI's own tests hold its `--json` output
/// to one example document per command; these hold this app's readers to the same documents, so a
/// field read here that the CLI never sends fails here, and a field the CLI renames fails there.
#[cfg(test)]
mod window_tests {
    use super::{attention_section, frame_on_a_screen, WindowFrame, MIN_WINDOW};

    /// design-system.md Section 29.14: a kept frame opens where it was while its title bar is on a
    /// screen now, and is forgotten otherwise; the window has a smallest size that keeps the Graph's
    /// wide layout; the frame shares its file with the text size, written whole; the tray's Settings...
    /// opens the first section whose dot shows; and the sidebar has no heading for its one group,
    /// counts as the tray does, says no count it could not read, and reopens the last pane.
    #[test]
    fn the_window_and_the_sidebar_keep_their_place() {
        let screens = [(0.0, 0.0, 1512.0, 982.0), (1512.0, -200.0, 2560.0, 1440.0)];
        let at = |x, y| WindowFrame { x, y, w: 1280.0, h: 860.0 };
        assert!(frame_on_a_screen(&at(100.0, 50.0), &screens));
        assert!(frame_on_a_screen(&at(1700.0, -100.0), &screens), "the second screen");
        assert!(!frame_on_a_screen(&at(5000.0, 50.0), &screens), "a place on no screen");
        assert_eq!(MIN_WINDOW, (760.0, 480.0), "the Graph keeps its wide layout in the app");
        assert_eq!(attention_section(Some("daemon"), true, false), Some("apps"));
        assert_eq!(attention_section(None, false, true), Some("daemon"), "repairing");
        assert_eq!(attention_section(None, false, false), None, "the last pane");
        let src = super::SRC;
        assert!(src.contains(".min_inner_size(MIN_WINDOW.0, MIN_WINDOW.1)"));
        assert!(src.contains("Some(f) => builder.inner_size(f.w, f.h).position(f.x, f.y),"));
        assert!(src.contains("None => builder.inner_size(1280.0, 860.0).center(),"));
        assert!(
            src.contains("keep_view_pref(app, \"text_scale\""),
            "the text size overwrites the frame"
        );
        assert!(
            src.contains("let tmp = path.with_extension(\"json.tmp\");")
                && src.contains("std::fs::rename(&tmp, &path)"),
            "a write cut short can lose both preferences"
        );
        let js = include_str!("../assets/settings.js");
        assert!(!js.contains("text: \"General\""), "the sidebar still heads its one group");
        assert!(js.contains("text: appsWithEntry() + \" connected\""), "the count says no unit");
        assert!(
            js.contains("a.kind === \"connected\" || a.kind === \"attention\""),
            "the count is not the tray's"
        );
        assert!(
            js.contains("if (!appsRead()) return \"warn\";") && js.contains("state && appsRead()"),
            "an unread listing counts as zero"
        );
        assert!(
            js.contains("!state.apps.some((a) => a.kind === \"attention\")"),
            "a row that needs the person leaves no dot"
        );
        assert!(js.contains("localStorage.getItem(LAST_PANE)") && js.contains("keepPane();"));
        let css = include_str!("../assets/settings.css");
        assert!(
            css.contains("width: clamp(220px, 13.75rem, 40vw);"),
            "the sidebar does not grow with the text"
        );
        assert!(
            css.contains("padding: 12vh 0 24px;\n  overflow-y: auto;"),
            "a tall sheet cannot be scrolled"
        );
    }
}

#[cfg(test)]
mod tray_tests {
    use super::{daemon_trouble, key_js, map_items_enabled, press_js, tray_act, Daemon};

    /// design-system.md Section 29.13 (D14, the author's list): the trouble shape shows while the
    /// memory cannot be used - could not start, stopped, two managers, the index - and not while it
    /// starts, an update waits or the state is unknown.
    #[test]
    fn trouble_is_the_memory_that_cannot_be_used() {
        let st = |answering: bool, running: Option<&str>, situation: &str| {
            serde_json::json!({ "answering": answering, "situation": situation,
                "version": { "here": "0.8.2", "running": running } })
        };
        assert!(daemon_trouble(&Daemon::Failed("x".into()), None));
        assert!(daemon_trouble(&Daemon::External, Some(&st(false, None, "one"))), "stopped");
        assert!(daemon_trouble(&Daemon::External, Some(&st(true, Some("0.8.2"), "conflict"))));
        let mut index = st(false, None, "one");
        index["embedder_conflict"] = serde_json::json!({ "recorded": "a", "daemon": "b" });
        assert!(daemon_trouble(&Daemon::External, Some(&index)), "the index needs rebuilding");
        assert!(!daemon_trouble(&Daemon::Starting, None));
        assert!(!daemon_trouble(&Daemon::External, None), "unknown is not trouble");
        assert!(
            !daemon_trouble(&Daemon::External, Some(&st(true, Some("0.8.1"), "one"))),
            "an update is not trouble"
        );
    }

    /// One act line, the first that holds: the memory cannot be used, an update, then no AI app
    /// connected unless Finish Setup... shows.
    #[test]
    fn the_tray_has_one_act_in_order() {
        assert_eq!(tray_act(true, true, true, false), Some(("Open This Mac...", "daemon")));
        assert_eq!(tray_act(false, true, true, false), Some(("Open This Mac...", "daemon")));
        assert_eq!(tray_act(false, false, true, false), Some(("Connect an AI App...", "apps")));
        assert_eq!(tray_act(false, false, true, true), None, "Finish Setup... covers it");
    }

    /// The map commands act where their control is, open the Graph from another page, and Back and
    /// Forward follow the pages that read them; the shell presses only when the press changes
    /// something and no dialog or card is open.
    #[test]
    fn the_map_commands_act_where_their_control_is() {
        let all: Vec<String> =
            ["panelL", "panelR", "mapTab", "listBtn", "fit", "keysOpen", "keysDlg"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let on = |url: &str, c: &[String]| -> Vec<&'static str> {
            map_items_enabled(url, c)
                .into_iter()
                .filter(|(_, e)| *e)
                .map(|(id, _)| id)
                .collect()
        };
        assert_eq!(on("viz://localhost/?workspace=w", &all).len(), 9, "all on the Graph");
        let old = on(
            "viz://localhost/",
            &["panelL".into(), "panelR".into(), "listBtn".into(), "fit".into()],
        );
        assert!(
            !old.contains(&"view-map")
                && !old.contains(&"view-back")
                && !old.contains(&"view-keys"),
            "{old:?}"
        );
        let review = on("viz://localhost/#review", &all);
        assert!(review.contains(&"view-fit") && !review.contains(&"view-forward"));
        let settings = on("tauri://localhost/settings.html", &[]);
        assert!(
            settings.contains(&"view-back")
                && !settings.contains(&"view-forward")
                && settings.contains(&"view-list")
        );
        assert!(on("tauri://localhost/starting", &[]).is_empty(), "nothing on the starting page");
        let tab = press_js("mapTab", true);
        assert!(
            tab.contains("dialog[open]")
                && tab.contains("#obscard.on")
                && tab.contains("aria-selected")
        );
        assert!(!press_js("fit", false).contains("aria-selected"));
        assert!(key_js("[", true).contains("keysDlg") && !key_js("k", false).contains("keysDlg"));
    }

    /// One id per item per menu: the tray's ids are its own, so a press does only its own act; and
    /// the menu bar holds Setup..., Find, the map's commands and Help.
    #[test]
    fn each_menu_item_has_its_own_id() {
        let src = super::SRC;
        let tray = &src[src.find("TrayIconBuilder::with_id(\"supragnosis\")").expect("the tray")..];
        let tray = &tray[..tray.find(".build(app)?;").unwrap()];
        for arm in [
            "\"tray-open\"",
            "\"tray-review\"",
            "\"tray-finish-setup\"",
            "\"tray-act\" | \"tray-settings\"",
            "\"tray-quit\"",
        ] {
            assert!(tray.contains(arm), "the tray lacks {arm}");
        }
        assert!(
            !tray.contains("\"review\" =>") && !tray.contains("\"settings\" =>"),
            "the tray hears the menu bar's ids"
        );
        for id in [
            "\"setup\"",
            "\"edit-find\"",
            "\"view-filters\"",
            "\"view-back\"",
            "\"help-docs\"",
            "\"help-diagnostics\", \"Diagnostics...\"",
            "\"help-issue\"",
        ] {
            assert!(src.contains(id), "the menu bar lacks {id}");
        }
        assert!(
            src.contains("\"Quit and Stop Memory\".to_string()"),
            "Quit does not say it stops the memory"
        );
        assert!(
            src.contains("tray.quit_stops = matches!(*daemon, Daemon::Spawned(_));"),
            "Quit's words follow the wrong state"
        );
        assert!(include_bytes!("../icons/tray-trouble.png").len() > 100, "no trouble shape");
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("controls: [\"panelL\", \"panelR\", \"mapTab\", \"listBtn\", \"fit\", \"keysOpen\", \"keysDlg\"]"));
    }
}

#[cfg(test)]
mod cli_contract_tests {
    use super::{app_row, cli_json, status_text, who_runs, Daemon, CLI_SCHEMA};

    const STATUS: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/status.json");
    const CONNECT: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/connect.json");

    fn read(doc: &str) -> serde_json::Value {
        cli_json(doc.as_bytes()).expect("a schema this app reads").expect("JSON")
    }

    /// client-connect.md Section 13.4: every app the CLI's example lists - the CLI holds the
    /// example to every app it knows - has a name in the menu and a name and an icon on both pages,
    /// from the one table in kit.js (design-system.md Section 6a).
    #[test]
    fn every_app_page_names_the_apps_connect_lists() {
        let kit = include_str!("../assets/kit.js");
        let table = |name: &str| -> &str {
            let start = kit.find(&format!("const {name} = {{")).unwrap_or_else(|| panic!("{name}"));
            &kit[start..start + kit[start..].find("};").expect("closed")]
        };
        let ids: Vec<String> = read(CONNECT)["clients"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect();
        assert!(ids.iter().any(|i| i == "opencode"), "{ids:?}");
        for id in &ids {
            let key = |t: &str| t.contains(&format!("\"{id}\":")) || t.contains(&format!(" {id}:"));
            assert!(key(table("APPS")), "kit.js names no {id}");
            assert!(key(table("APP_KIND")), "kit.js gives {id} no icon");
            assert!(super::AI_APPS.iter().any(|(i, _)| i == id), "the menu lacks {id}");
        }
        for page in [include_str!("../assets/settings.js"), include_str!("../assets/welcome.js")] {
            assert!(!page.contains("const APP_KIND") && !page.contains("const APPS"), "one table");
        }
    }

    /// Every field this app reads from a CLI answer, by JSON pointer. A reader that starts using
    /// another field adds it here, and the example has to carry it.
    #[test]
    fn every_field_the_app_reads_is_in_the_clis_examples() {
        let reads: [(&str, &[&str]); 2] = [
            (
                STATUS,
                &[
                    "/situation",
                    "/managers/1/type",
                    "/managers/1/source",
                    "/managers/1/label",
                    "/answering",
                    "/version/here",
                    "/version/running",
                    "/store/owed_projections",
                    "/store/last_recovery",
                    "/service/state",
                ],
            ),
            (
                CONNECT,
                &[
                    "/clients/0/id",
                    "/clients/0/installed",
                    "/clients/0/entry",
                    "/clients/1/capture",
                ],
            ),
        ];
        for (doc, pointers) in reads {
            let v = read(doc);
            for p in pointers {
                assert!(v.pointer(p).is_some(), "the app reads {p}, which the example lacks");
            }
        }
    }

    /// The examples go through the same readers a live answer does, and read as they should.
    #[test]
    fn the_examples_read_through_the_apps_own_readers() {
        let st = read(STATUS);
        assert!(status_text(&Daemon::External, Some(&st)).starts_with("Two managers"));
        let mut one = st.clone();
        one["situation"] = "one".into();
        one["managers"] = serde_json::json!([st["managers"][1].clone()]);
        assert_eq!(who_runs(&Daemon::External, &one), "starts at login");
        let line = status_text(&Daemon::External, Some(&one));
        assert_eq!(line, "Update ready - restart: 0.4.7 running, 0.4.8 installed");

        let apps = read(CONNECT);
        let kinds: Vec<_> = apps["clients"]
            .as_array()
            .expect("clients")
            .iter()
            .map(|r| app_row(Some(r)).2)
            .collect();
        for kind in ["connected", "attention", "off", "absent"] {
            assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
        }
    }

    /// A schema newer than this app reads is not read at all; an answer without one is a CLI from
    /// before the contract, and is read as before.
    #[test]
    fn a_newer_schema_is_not_read_and_an_older_cli_still_is() {
        let newer = format!(r#"{{"schema": {}, "situation": "one"}}"#, CLI_SCHEMA + 1);
        assert_eq!(cli_json(newer.as_bytes()), Err(CLI_SCHEMA + 1));
        assert!(cli_json(br#"{"situation": "one"}"#).expect("read").is_some());
        assert_eq!(cli_json(b"not json"), Ok(None));
    }
}
