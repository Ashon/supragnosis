//! supragnosis-app - the desktop shell (a thin client over the daemon).
//!
//! A Tauri webview around the daemon's unix-socket viewer (Principle 21: a human-facing channel,
//! separate from the MCP tool surface). The shell embeds NO frontend and NO engine - the viewer UI
//! is served by the daemon (supragnosis-viz), so the desktop app and the future hub read tier keep
//! a single UI source, and the store stays single-process (the daemon owns the db; the shell owns
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

// Tauri on macOS/Windows expects a windowed (non-console) binary in release bundles.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
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

/// The shell's relationship to the daemon. Spawned = ours to reap and restart; External =
/// attached to an externally managed daemon (launchd / `supragnosis start` / another shell) -
/// never killed by us (its MCP clients outlive this app).
enum Daemon {
    Starting,
    External,
    Spawned(Child),
    Failed(String),
}

impl Daemon {
    fn status_line(&self) -> String {
        match self {
            Daemon::Starting => "daemon: starting...".to_string(),
            Daemon::External => "daemon: attached (externally managed)".to_string(),
            Daemon::Spawned(c) => format!("daemon: running (spawned, pid {})", c.id()),
            Daemon::Failed(e) => format!("daemon: FAILED - {e}"),
        }
    }
}

struct DaemonGuard(Mutex<Daemon>);

/// The tray's status menu item - kept as managed state so the daemon tasks can rewrite its text.
struct TrayStatus(MenuItem<tauri::Wry>);

/// The active remote server profile, when there is one (docs/remote-server.md Section 5). This
/// machine's daemon is then not what the AI apps use, and the shell says so instead of showing local
/// knowledge.
struct ActiveRemote(Mutex<Option<RemoteServer>>);

/// What the status line says needs doing when nothing else does: no AI app connected yet, the state
/// a new user is in (docs/client-connect.md Section 5).
struct AppsHint(Mutex<Option<String>>);

/// The settings section the tray's Settings... opens: the one whose state the status line is
/// pointing at, if any (docs/settings-page.md Section 3.0).
struct Attention(Mutex<Option<&'static str>>);

#[derive(Clone)]
struct RemoteServer {
    name: String,
    url: String,
    answering: bool,
    credential_refused: bool,
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
    #[cfg(unix)]
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

/// What the daemon answered, minus the headers this proxy does not forward.
struct Fetched {
    status: u16,
    ctype: String,
    /// The daemon's Content-Security-Policy, forwarded verbatim rather than restated here.
    ///
    /// The shell is a separate workspace by design ("It shares no code with the server"), so a
    /// shared constant is not available - and a second copy of the policy would be a second thing to
    /// keep in step, which is the failure this proxy already avoids for content-type by reading it
    /// rather than assuming it. The daemon serves the page; the daemon's policy governs it.
    csp: Option<String>,
    body: Vec<u8>,
}

/// One GET over the socket. The daemon answers Connection: close, so read-to-EOF terminates.
/// /api/events must never come through here (an endless stream) - the protocol handler
/// short-circuits it.
async fn uds_fetch(sock: &Path, target: &str) -> anyhow::Result<Fetched> {
    uds_request(sock, "GET", target).await
}

/// One request over the socket, with no body - the viewer takes its parameters in the query string,
/// POST included (its one POST is the console's narrowing act, `/api/peer/share`).
async fn uds_request(sock: &Path, method: &str, target: &str) -> anyhow::Result<Fetched> {
    let mut s = UnixStream::connect(sock).await?;
    s.write_all(
        format!("{method} {target} HTTP/1.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await?;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("malformed response - no header terminator")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let body = raw[split + 4..].to_vec();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .context("malformed status line")?;
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let ctype = header("content-type").unwrap_or_else(|| "application/octet-stream".to_string());
    Ok(Fetched { status, ctype, csp: header("content-security-policy"), body })
}

/// The policy for responses the SHELL authors rather than proxies: the starting splash and the
/// error JSONs. Strictly tighter than the daemon's - none of them loads a script or an image, and
/// the splash's inline styles are the only reason `style-src` is not `'none'` too.
///
/// It is not a fallback for the viewer page. If the daemon ever answers without a policy, that is a
/// daemon that stopped sending one, and serving its HTML under a shell-authored guess would hide
/// exactly that.
const SHELL_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; \
     form-action 'none'; frame-ancestors 'none'; object-src 'none'";

/// The origins Tauri's IPC rides on, which the daemon has no reason to know about.
///
/// This is the one place the forwarded policy must be edited rather than passed through. Tauri
/// normally solves it by extending the CSP itself - but it does that for a policy declared in
/// `tauri.conf.json`, and this one arrives as a header on a proxied response, so nothing extends it
/// and `connect-src 'self'` would refuse the transport `window.__TAURI__.event.emit/listen` uses.
/// The window would render and then be deaf to the tray.
///
/// Note what does NOT need an entry: `shell-init.js` is injected as a webview user script rather
/// than fetched by the document, so it is not `script-src`'s business - which is why Tauri apps run
/// under policies with no `'unsafe-inline'` at all.
const TAURI_IPC_SOURCES: &str = "ipc: http://ipc.localhost";

/// Adds the IPC origins to a forwarded policy's `connect-src`.
///
/// A duplicate directive would be ignored (a browser honours the first occurrence), so the existing
/// one is widened in place. A policy with no `connect-src` gets one, because `default-src 'none'`
/// would otherwise deny the fetch by inheritance.
fn with_ipc_sources(csp: &str) -> String {
    if csp.trim().is_empty() {
        return String::new(); // nothing to extend; forward the absence honestly
    }
    let mut found = false;
    let mut out: Vec<String> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            if d == "connect-src" || d.starts_with("connect-src ") {
                found = true;
                format!("{d} {TAURI_IPC_SOURCES}")
            } else {
                d.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("connect-src {TAURI_IPC_SOURCES}"));
    }
    out.join("; ")
}

/// The shell's own files on the viewer's origin (docs/settings-page.md Section 3.0): the chrome
/// stylesheet both pages share. The viewer is the daemon's page and its policy allows styles from
/// its own origin only, so the shell answers this path itself - never passing it to a daemon or a
/// hub - and the stylesheet is the same bytes the settings page links from the app's origin.
fn shell_asset(target: &str) -> Option<(&'static str, &'static [u8])> {
    let path = target.split('?').next().unwrap_or(target);
    match path {
        "/__shell/shell.css" => {
            Some(("text/css; charset=utf-8", include_bytes!("../assets/shell.css")))
        }
        _ => None,
    }
}

fn resp(status: u16, ctype: &str, csp: &str, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("content-type", ctype)
        // The daemon marks every response no-store; the proxy must not launder that away -
        // without it WKWebView disk-caches the viewer assets and serves STALE pages across
        // app restarts (live-updating data and a hand-deployed daemon make caching all wrong).
        .header("cache-control", "no-store")
        // Same laundering concern, higher stakes. The webview renders entity names, descriptions and
        // proposal rationale, which under federation are synced, attacker-influenceable input - so
        // the daemon's Content-Security-Policy is the second line of defence behind output escaping,
        // and a proxy that drops it leaves the shell (the surface a human actually looks at) with
        // only the first. It used to drop it, because it forwarded content-type and nothing else.
        .header("content-security-policy", csp)
        .header("x-content-type-options", "nosniff")
        .body(body)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// Served at `/` while the daemon's socket is not answering yet; refreshes itself into the real
/// viewer once it is. Carries the LIVE daemon state so a failure is readable on the splash
/// instead of an eternal "starting..." (the tray shows the same line, but the splash is what
/// the user is looking at). Palette mirrors the viewer's candlelight theme.
// data-tauri-drag-region: with the overlay title bar there is no other chrome to drag the
// window by while the splash is up.
/// What the viewer shows while a remote profile is active: which server the AI apps use, and that
/// browsing it arrives with the network read tier (remote-server.md Section 5). Not an empty graph,
/// which would read as "no knowledge" (P5).
fn remote_html(r: &RemoteServer) -> String {
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let state = match (r.answering, r.credential_refused) {
        (true, true) => "it answers, but refused this machine's credential",
        (true, false) => "it answers",
        (false, _) => "it does not answer right now",
    };
    format!(
        r#"<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="30"><title>supragnosis</title><body data-tauri-drag-region style="background:#0c0e14;color:#f0c469;font:14px ui-monospace,monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center;max-width:80%"><div>AI apps on this Mac use the server "{name}"</div><div style="color:#8e96a5;margin-top:10px;font-size:12px;word-break:break-word">{url} - {state}</div><div style="color:#5c6472;margin-top:14px;font-size:11px">browsing a remote server's knowledge here arrives with a later release / tray: Server &gt; This Mac to switch back</div></div></body>"#,
        name = esc(&r.name),
        url = esc(&r.url),
    )
}

fn starting_html(status: &str) -> String {
    let esc = status.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="1"><title>supragnosis</title><body data-tauri-drag-region style="background:#0c0e14;color:#f0c469;font:14px ui-monospace,monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center;max-width:80%"><div>starting the supragnosis daemon...</div><div style="color:#8e96a5;margin-top:10px;font-size:12px;word-break:break-word">{esc}</div><div style="color:#5c6472;margin-top:6px;font-size:11px">log: ~/.supragnosis/log/app-daemon.err.log / tray: Restart Daemon</div></div></body>"#
    )
}

/// Runs attach-or-spawn and publishes the outcome (state + tray status line).
async fn bring_up(app: tauri::AppHandle, sock: PathBuf) {
    // A remote profile means this machine's daemon is not what the AI apps use - so the shell does
    // not start one for them (remote-server.md Section 5).
    refresh_status(&app).await;
    if active_remote(&app).is_some() {
        return;
    }
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

/// Runs the CLI the shell found, blocking. `None` when there is no CLI to run.
fn run_cli(args: &[&str]) -> Option<std::process::Output> {
    let bin = find_server_bin()?;
    Command::new(bin).args(args).stdin(Stdio::null()).output().ok()
}

/// `supragnosis status --json`. `None` when the CLI predates it - the tray then falls back to what
/// the shell itself knows and disables the switch rather than failing on click.
fn cli_status() -> Option<serde_json::Value> {
    let out = run_cli(&["status", "--json"])?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// The one line the CLI said that matters: its error on failure, its first line on success.
fn cli_outcome(out: &std::process::Output) -> (bool, String) {
    let text = if out.status.success() { &out.stdout } else { &out.stderr };
    let line = String::from_utf8_lossy(text)
        .lines()
        .map(|l| l.trim().trim_start_matches("Error:").trim().to_string())
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    (out.status.success(), line)
}

/// The tray's status line, from what the CLI observed plus the shell's own relationship to the
/// daemon (a child it spawned is invisible to the CLI - no pidfile, no launchd job).
/// Who runs the daemon, in the words the status line and the settings page use - including a child
/// of this app, which the CLI cannot see (no pidfile, no launchd job).
fn who_runs(daemon: &Daemon, st: &serde_json::Value) -> String {
    match (daemon, st["managers"].get(0)) {
        (Daemon::Spawned(_), _) => "run by this app, not at login".to_string(),
        (_, Some(m)) => match (m["type"].as_str(), m["source"].as_str()) {
            (Some("launchd"), Some("canonical")) => "login item".to_string(),
            (Some("launchd"), Some("homebrew")) => "brew services".to_string(),
            (Some("launchd"), _) => format!("launchd {}", m["label"].as_str().unwrap_or("?")),
            (Some("pidfile"), _) => "supragnosis start".to_string(),
            _ => "externally managed".to_string(),
        },
        (_, None) => "externally managed".to_string(),
    }
}

fn status_text(daemon: &Daemon, st: Option<&serde_json::Value>, note: Option<&str>) -> String {
    let base = match (daemon, st) {
        (Daemon::Starting | Daemon::Failed(_), _) | (_, None) => daemon.status_line(),
        (_, Some(st)) => {
            let situation = st["situation"].as_str().unwrap_or("");
            let running = st["version"]["running"].as_str();
            let here = st["version"]["here"].as_str().unwrap_or("?");
            if situation == "conflict" {
                let n = st["managers"].as_array().map_or(0, Vec::len);
                format!("daemon: CONFLICT - {n} managers claim it (see `supragnosis status`)")
            } else {
                let who = who_runs(daemon, st);
                match running {
                    Some(r) if r != here => {
                        format!("daemon {r} running, {here} installed - restart it in Settings")
                    }
                    Some(r) => format!("daemon {r} - {who}"),
                    None => format!("daemon - {who}"),
                }
            }
        }
    };
    match note {
        Some(n) if !n.is_empty() => format!("{base} | {n}"),
        _ => base,
    }
}

/// The active remote profile, when one is active.
fn active_remote(app: &tauri::AppHandle) -> Option<RemoteServer> {
    app.try_state::<ActiveRemote>().and_then(|s| s.0.lock().unwrap().clone())
}

/// `supragnosis server --json`. `None` when the CLI predates profiles.
fn cli_server_list() -> Option<serde_json::Value> {
    let out = run_cli(&["server", "--json"])?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Re-reads the server profiles and records whether a remote one is active. Returns what the CLI
/// said, for the settings window.
async fn refresh_servers(app: &tauri::AppHandle) -> Option<serde_json::Value> {
    let list = tokio::task::spawn_blocking(cli_server_list).await.ok().flatten();
    let remote = list.as_ref().and_then(|list| {
        let active = list["active"].as_str().unwrap_or("local");
        let row = list["servers"].as_array()?.iter().find(|r| {
            r["name"].as_str() == Some(active) && r["remote"].as_bool().unwrap_or(false)
        })?;
        Some(RemoteServer {
            name: active.to_string(),
            url: row["url"].as_str().unwrap_or("").to_string(),
            answering: list["check"]["answering"].as_bool().unwrap_or(false),
            credential_refused: list["check"]["credential"].as_bool() == Some(false),
        })
    });
    if let Some(state) = app.try_state::<ActiveRemote>() {
        *state.0.lock().unwrap() = remote;
    }
    list
}

/// The tray's line while a remote profile is active: which server, and whether it answers.
fn remote_line(r: &RemoteServer) -> String {
    match (r.answering, r.credential_refused) {
        (true, false) => format!("server {} - answering", r.name),
        (true, true) => format!("server {} - credential refused, see Settings", r.name),
        (false, _) => format!("server {} - not answering, see Settings", r.name),
    }
}

/// Re-reads the CLI's view and republishes the tray's one line (docs/settings-page.md Section 5).
async fn refresh_status(app: &tauri::AppHandle) {
    refresh_servers(app).await;
    refresh_apps(app).await;
    let hint = app.try_state::<AppsHint>().and_then(|h| h.0.lock().unwrap().clone());
    let (text, attention) = match active_remote(app) {
        Some(r) => {
            let trouble = !r.answering || r.credential_refused;
            let attention = if trouble { Some("server") } else { hint.as_ref().map(|_| "apps") };
            (remote_line(&r), attention)
        }
        None => {
            let st = tokio::task::spawn_blocking(cli_status).await.ok().flatten();
            let drift = st.as_ref().is_some_and(|v| {
                let running = v["version"]["running"].as_str();
                running.is_some() && running != v["version"]["here"].as_str()
            });
            let text = status_text(
                &app.state::<DaemonGuard>().0.lock().unwrap(),
                st.as_ref(),
                hint.as_deref(),
            );
            let attention = if drift { Some("daemon") } else { hint.as_ref().map(|_| "apps") };
            (text, attention)
        }
    };
    if let Some(status) = app.try_state::<TrayStatus>() {
        let _ = status.0.set_text(text);
    }
    if let Some(a) = app.try_state::<Attention>() {
        *a.0.lock().unwrap() = attention;
    }
}

/// `supragnosis connect --json`. `None` when the CLI predates it.
fn cli_connect_list() -> Option<serde_json::Value> {
    let out = run_cli(&["connect", "--json"])?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
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
        "bridge" => ("connected through the bridge".into(), Some("Disconnect"), "connected"),
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
        "other" => ("another supragnosis entry is configured".into(), Some("Replace"), "attention"),
        "unknown" => ("its settings file could not be read".into(), None, "absent"),
        _ => ("not connected".into(), Some("Connect"), "off"),
    }
}

/// Re-reads which AI apps are connected, for the status line's hint while none is.
async fn refresh_apps(app: &tauri::AppHandle) {
    let list = tokio::task::spawn_blocking(cli_connect_list).await.ok().flatten();
    let any = list
        .as_ref()
        .and_then(|l| l["clients"].as_array())
        .is_some_and(|rs| rs.iter().any(|r| r["entry"].as_str().is_some_and(|e| e != "none")));
    if let Some(hint) = app.try_state::<AppsHint>() {
        *hint.0.lock().unwrap() =
            (list.is_some() && !any).then(|| "no AI app connected - see Settings".to_string());
    }
}

/// Connects an AI app through the bridge, or disconnects it if it already is. The click is the
/// consent to replace another supragnosis entry (client-connect.md Section 5). The CLI decides and
/// does; the answer is what it said.
async fn toggle_app(app: &tauri::AppHandle, id: &str) -> String {
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
        None => format!("{name}: supragnosis CLI not found"),
        Some(o) if o.status.success() && connected => format!("{name} disconnected"),
        Some(o) if o.status.success() => {
            // The CLI's "next" line is the one thing the person still has to do.
            let next = String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.trim().strip_prefix("next").map(|n| n.trim().to_string()));
            match next {
                Some(n) => format!("{name} connected - {n}"),
                None => format!("{name} connected"),
            }
        }
        Some(o) => format!("connecting {name} failed: {}", cli_outcome(&o).1),
    };
    refresh_status(app).await;
    message
}

/// Makes a server profile the one AI apps here use. Switching to a remote server leaves any local
/// daemon running (it is not this app's to stop); switching back to this Mac brings the local daemon
/// up the usual way.
async fn use_server(app: &tauri::AppHandle, sock: PathBuf, name: &str) -> String {
    let n = name.to_string();
    let out = tokio::task::spawn_blocking(move || run_cli(&["server", "use", &n]))
        .await
        .ok()
        .flatten();
    let label = if name == "local" { "this Mac".to_string() } else { name.to_string() };
    let message = match out {
        Some(o) if o.status.success() => {
            format!("AI apps here use {label} from their next session")
        }
        Some(o) => format!("switching to {label} failed: {}", cli_outcome(&o).1),
        None => "supragnosis CLI not found".to_string(),
    };
    bring_up(app.clone(), sock).await;
    reload_viewer(app);
    message
}

/// The arguments of `server add`. The credential is not among them, and cannot be: it reaches the
/// CLI on stdin only (docs/settings-page.md S2).
fn server_add_args(name: &str, url: &str, ca: Option<&str>) -> Vec<String> {
    let mut args = vec!["server".to_string(), "add".into(), name.to_string(), url.to_string()];
    if let Some(ca) = ca.filter(|c| !c.trim().is_empty()) {
        args.push("--ca".into());
        args.push(ca.trim().to_string());
    }
    args
}

/// Adds a remote server profile through the CLI, writing the credential to its stdin.
fn add_server(name: &str, url: &str, credential: &str, ca: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    let bin = find_server_bin().ok_or("supragnosis CLI not found")?;
    let mut child = Command::new(bin)
        .args(server_add_args(name, url, ca))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run the supragnosis CLI: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{}\n", credential.trim()).as_bytes())
            .map_err(|e| format!("could not hand the credential to the CLI: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("the CLI did not finish: {e}"))?;
    match cli_outcome(&out) {
        (true, _) => Ok(format!("added {name}")),
        (false, why) => Err(why),
    }
}

/// Removes a remote profile and its credential. Removing the active one sends AI apps back to this
/// Mac, which the CLI says and the app then follows.
async fn remove_server(app: &tauri::AppHandle, sock: PathBuf, name: &str) -> String {
    let n = name.to_string();
    let out = tokio::task::spawn_blocking(move || run_cli(&["server", "remove", &n]))
        .await
        .ok()
        .flatten();
    let message = match out {
        Some(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            if text.contains("use local again") {
                format!("removed {name} - AI apps here use this Mac again")
            } else {
                format!("removed {name}")
            }
        }
        Some(o) => format!("removing {name} failed: {}", cli_outcome(&o).1),
        None => "supragnosis CLI not found".to_string(),
    };
    bring_up(app.clone(), sock).await;
    reload_viewer(app);
    message
}

/// Restart: bounce whatever we manage, then attach-or-spawn again. A spawned child is killed
/// directly; an external daemon is bounced through the CLI (`supragnosis restart` knows every
/// manager the product installs). A refused restart (a conflict, an unrecognized holder) says why,
/// instead of the shell quietly re-attaching to the process it failed to replace (L4).
async fn restart_daemon(app: &tauri::AppHandle, sock: PathBuf) -> String {
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    refresh_status(app).await;
    let message = match prev {
        Daemon::Spawned(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            "restarted the daemon this app runs".to_string()
        }
        Daemon::External => {
            let out = tokio::task::spawn_blocking(|| run_cli(&["restart"])).await.ok().flatten();
            match out.as_ref().map(cli_outcome) {
                Some((true, _)) => "restarted".to_string(),
                Some((false, why)) => format!("restart refused: {why}"),
                None => "restart: supragnosis CLI not found".to_string(),
            }
        }
        Daemon::Starting | Daemon::Failed(_) => "starting the daemon".to_string(),
    };
    bring_up(app.clone(), sock).await;
    message
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
async fn set_login(app: &tauri::AppHandle, sock: PathBuf, on: bool) -> String {
    let installed = tokio::task::spawn_blocking(cli_status)
        .await
        .ok()
        .flatten()
        .and_then(|v| v["service"]["state"].as_str().map(|s| s != "absent"))
        .unwrap_or(false);
    if installed == on {
        return if on { "already starts at login" } else { "already off" }.to_string();
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
    let message = match out.as_ref().map(cli_outcome) {
        Some((true, _)) if installed => "no longer starts at login".to_string(),
        Some((true, _)) => "starts at login".to_string(),
        Some((false, why)) => format!("Start at Login failed: {why}"),
        None => "Start at Login: supragnosis CLI not found".to_string(),
    };
    if !installed {
        wait_for_socket(&sock, Duration::from_secs(10)).await;
    }
    bring_up(app.clone(), sock).await;
    message
}

// --- The settings page (docs/settings-page.md) ---------------------------------------------

/// The commands the settings page calls - the list `generate_handler!` registers below. build.rs
/// declares the same list in the app manifest, so none of them is open to any page by default;
/// capabilities/settings.json grants them to the main window, and each one refuses unless that
/// window is showing the app's own settings page (S1). A test holds the lists together.
#[cfg(test)]
const SETTINGS_COMMANDS: &[&str] = &[
    "settings_state",
    "server_use",
    "server_add",
    "server_remove",
    "app_toggle",
    "login_set",
    "daemon_restart",
    "peer_narrow",
];

/// The settings page's address: the app's own page, bundled with it - never one a daemon or a hub
/// serves.
fn settings_url() -> tauri::Url {
    let url = if cfg!(windows) {
        "http://tauri.localhost/settings.html"
    } else {
        "tauri://localhost/settings.html"
    };
    url.parse().expect("static url")
}

/// Whether a caller may change a setting (docs/settings-page.md S1): the main window, while its page
/// is the app's own settings page. The capability cannot tell the pages of one window apart - the
/// same window shows the viewer, which a daemon or a hub serves - so this check is the lock. It reads
/// the webview's top-level address, so a viewer page that frames the settings page cannot call
/// through the frame either.
fn settings_caller_ok(label: &str, url: &tauri::Url) -> bool {
    let own = settings_url();
    label == "main"
        && url.scheme() == own.scheme()
        && url.host_str() == own.host_str()
        && url.path() == own.path()
}

fn settings_caller(webview: &tauri::Webview) -> Result<(), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if settings_caller_ok(webview.label(), &url) {
        Ok(())
    } else {
        tracing::warn!(label = webview.label(), page = %url, "a settings command was refused: not the settings page");
        Err(
            "only the app's own Settings page may change a setting (docs/settings-page.md S1)"
                .into(),
        )
    }
}

/// Reloads the viewer if that is what the main window shows - not the settings page, which re-reads
/// its own state after every change.
fn reload_viewer(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.url()
            .is_ok_and(|u| u.scheme() == "viz" || u.host_str() == Some("viz.localhost"))
        {
            let _ = w.eval("location.reload()");
        }
    }
}

/// The socket path, for the commands that bring the daemon back up.
struct VizSock(PathBuf);

/// Everything the settings window shows, gathered from the CLI in one pass.
async fn settings_state_of(app: &tauri::AppHandle) -> serde_json::Value {
    let servers = refresh_servers(app).await;
    // Both CLI calls run at once; each is a process the other need not wait for.
    let status = tokio::task::spawn_blocking(cli_status);
    let connect = tokio::task::spawn_blocking(cli_connect_list);
    let (status, connect) = (status.await.ok().flatten(), connect.await.ok().flatten());
    let remote = active_remote(app);

    let profiles: Vec<serde_json::Value> = servers
        .as_ref()
        .and_then(|l| l["servers"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let remote = r["remote"].as_bool().unwrap_or(false);
            serde_json::json!({
                "name": r["name"],
                "label": if remote { r["name"].clone() } else { serde_json::json!("This Mac") },
                "url": r["url"],
                "remote": remote,
                "active": r["active"],
            })
        })
        .collect();
    let check = servers.as_ref().map(|l| l["check"].clone()).unwrap_or(serde_json::Value::Null);

    let reports = connect.as_ref().and_then(|l| l["clients"].as_array().cloned());
    let apps: Vec<serde_json::Value> = AI_APPS
        .iter()
        .map(|(id, name)| {
            let report = reports
                .as_ref()
                .and_then(|rs| rs.iter().find(|r| r["id"].as_str() == Some(id)).cloned());
            let (state, action, kind) = app_row(report.as_ref());
            serde_json::json!({ "id": id, "name": name, "state": state, "action": action, "kind": kind })
        })
        .collect();

    // Sync is this Mac's daemon's (docs/settings-page.md Section 3.4): read from its viewer socket,
    // and not at all while a remote profile is active.
    let federation = if remote.is_none() {
        let sock = app.state::<VizSock>().0.clone();
        match tokio::time::timeout(Duration::from_secs(5), uds_fetch(&sock, "/api/federation"))
            .await
        {
            Ok(Ok(f)) if f.status == 200 => {
                serde_json::from_slice::<serde_json::Value>(&f.body).ok()
            }
            _ => None,
        }
    } else {
        None
    };
    let guard = app.state::<DaemonGuard>();
    let (line, who) = {
        let daemon = guard.0.lock().unwrap();
        let who = status.as_ref().map(|st| who_runs(&daemon, st));
        (status_text(&daemon, status.as_ref(), None), who)
    };
    let login = status
        .as_ref()
        .and_then(|v| v["service"]["state"].as_str())
        .map(|s| s != "absent");
    serde_json::json!({
        "cli": find_server_bin().is_some(),
        "server": {
            "known": servers.is_some(),
            "active": servers.as_ref().map(|l| l["active"].clone()),
            "check": check,
            "profiles": profiles,
        },
        "apps": apps,
        "daemon": {
            "line": line,
            "who": who,
            "situation": status.as_ref().map(|v| v["situation"].clone()),
            "answering": status.as_ref().map(|v| v["answering"].clone()),
            "running": status.as_ref().map(|v| v["version"]["running"].clone()),
            "installed": status.as_ref().map(|v| v["version"]["here"].clone()),
            "store": status.as_ref().map(|v| v["store"].clone()),
            "login": login,
            "remote": remote.map(|r| r.name),
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
    settings_caller(&webview)?;
    Ok(settings_state_of(&app).await)
}

#[tauri::command]
async fn server_use(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    name: String,
) -> Result<String, String> {
    settings_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    Ok(use_server(&app, sock, &name).await)
}

#[tauri::command]
async fn server_add(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    name: String,
    url: String,
    credential: String,
    ca: Option<String>,
) -> Result<String, String> {
    settings_caller(&webview)?;
    let result =
        tokio::task::spawn_blocking(move || add_server(&name, &url, &credential, ca.as_deref()))
            .await
            .map_err(|e| e.to_string())?;
    refresh_status(&app).await;
    result
}

#[tauri::command]
async fn server_remove(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    name: String,
) -> Result<String, String> {
    settings_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    Ok(remove_server(&app, sock, &name).await)
}

#[tauri::command]
async fn app_toggle(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    id: String,
) -> Result<String, String> {
    settings_caller(&webview)?;
    if !AI_APPS.iter().any(|(i, _)| *i == id) {
        return Err(format!("{id:?} is not an app supragnosis connects"));
    }
    Ok(toggle_app(&app, &id).await)
}

#[tauri::command]
async fn login_set(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    on: bool,
) -> Result<String, String> {
    settings_caller(&webview)?;
    if active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
    let sock = app.state::<VizSock>().0.clone();
    Ok(set_login(&app, sock, on).await)
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
    settings_caller(&webview)?;
    if active_remote(&app).is_some() {
        return Err("a remote server is active - sync settings are this Mac's daemon's".into());
    }
    if node.trim().is_empty() {
        return Err("no peer named".into());
    }
    let target = format!(
        "/api/peer/share?node_id={}&workspaces={}",
        query_value(&node),
        query_value(&keep.join(","))
    );
    let sock = app.state::<VizSock>().0.clone();
    let answer = tokio::time::timeout(Duration::from_secs(15), uds_request(&sock, "POST", &target))
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
    settings_caller(&webview)?;
    if active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
    let sock = app.state::<VizSock>().0.clone();
    Ok(restart_daemon(&app, sock).await)
}

/// The graph's address: the viewer, which the shell's `viz://` proxy serves.
fn graph_url() -> tauri::Url {
    let url = if cfg!(windows) { "http://viz.localhost/" } else { "viz://localhost/" };
    url.parse().expect("static url")
}

/// Turns the main window to the settings page (docs/settings-page.md Section 3.0), creating the
/// window first if the app has retreated to the tray. With a section, opens that section; without
/// one, a window already on the settings page stays where it is.
fn show_settings(app: &tauri::AppHandle, section: Option<&str>) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let there = w.url().is_ok_and(|u| settings_caller_ok("main", &u));
        if !there || section.is_some() {
            let mut url = settings_url();
            url.set_fragment(section);
            w.navigate(url)?;
        }
    }
    Ok(())
}

/// Turns the main window to the graph, creating it first if needed.
fn show_graph(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if w.url().is_ok_and(|u| settings_caller_ok("main", &u)) {
            w.navigate(graph_url())?;
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
    .inner_size(1280.0, 860.0)
    .initialization_script(include_str!("../assets/shell-init.js"));
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
    // workflow pins an SDK-26 runner for the app job (release.yml); build locally with a
    // current Xcode. y=26 re-confirmed on macOS 26 with the SDK-26 build.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 26.0));
    builder.build()?;
    Ok(())
}

/// Holds the daemon's /api/events SSE stream and re-emits each `data:` frame as a "viz-event"
/// Tauri event (the webview side is the EventSource facade injected at init). Reconnects forever -
/// the daemon may not be up yet, or may restart underneath us; the viewer also polls, so a lost
/// frame degrades liveness, never correctness.
async fn sse_bridge(app: tauri::AppHandle, sock: PathBuf) {
    loop {
        if let Ok(mut s) = UnixStream::connect(&sock).await {
            if s.write_all(b"GET /api/events HTTP/1.1\r\n\r\n").await.is_ok() {
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                // One quiet line per connection when the first real frame flows - the log-side
                // proof that daemon events are reaching the webview bridge.
                let mut streamed = false;
                loop {
                    let n = match s.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    // SSE frames end with a blank line ("\n\n"). The response header block ends
                    // with \r\n\r\n (no bare \n\n inside), so it is swept out with the first
                    // frame, and non-"data:" lines (headers, ": ok" keepalive) fall through.
                    while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                        let frame: Vec<u8> = buf.drain(..pos + 2).collect();
                        for line in String::from_utf8_lossy(&frame).lines() {
                            if let Some(json) = line.strip_prefix("data: ") {
                                if !streamed {
                                    streamed = true;
                                    tracing::info!("sse bridge live - first event frame forwarded to the webview");
                                }
                                let _ = app.emit("viz-event", json.to_string());
                            }
                        }
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn main() {
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
    let sock = viz_sock();

    let proxy_sock = sock.clone();
    tauri::Builder::default()
        .manage(DaemonGuard(Mutex::new(Daemon::Starting)))
        .invoke_handler(tauri::generate_handler![
            settings_state,
            server_use,
            server_add,
            server_remove,
            app_toggle,
            login_set,
            daemon_restart,
            peer_narrow
        ])
        .register_asynchronous_uri_scheme_protocol("viz", move |ctx, request, responder| {
            let sock = proxy_sock.clone();
            let app = ctx.app_handle().clone();
            let target = request
                .uri()
                .path_and_query()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "/".to_string());
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
                } else if let Some(r) = active_remote(&app) {
                    // A remote profile: the local socket is not the knowledge the AI apps use.
                    if target == "/" {
                        resp(200, "text/html; charset=utf-8", SHELL_CSP, remote_html(&r).into_bytes())
                    } else {
                        resp(
                            502,
                            "application/json",
                            SHELL_CSP,
                            br#"{"error":"a remote server is active - this window does not browse it yet"}"#.to_vec(),
                        )
                    }
                } else {
                    match tokio::time::timeout(Duration::from_secs(15), uds_fetch(&sock, &target)).await {
                        // The daemon's own policy governs the daemon's own page. An answer with no
                        // policy is forwarded with none, so a daemon that stopped sending one shows
                        // up as that rather than as a shell-authored guess (see SHELL_CSP).
                        Ok(Ok(f)) => {
                            let csp = with_ipc_sources(f.csp.as_deref().unwrap_or(""));
                            resp(f.status, &f.ctype, &csp, f.body)
                        }
                        // Socket not answering: the index gets a self-refreshing splash carrying
                        // the live daemon state (a Failed reason must be readable, not an eternal
                        // "starting..."); API calls get an honest 502 (Principle 5).
                        _ if target == "/" => {
                            let status = app.state::<DaemonGuard>().0.lock().unwrap().status_line();
                            resp(200, "text/html; charset=utf-8", SHELL_CSP, starting_html(&status).into_bytes())
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
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            WindowEvent::Resized(_) => {
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
            // Tray: says what state this Mac is in and opens the windows that change it
            // (docs/settings-page.md Section 5). The settings themselves live on that page.
            let status = MenuItem::with_id(app, "status", "daemon: starting...", false, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Graph", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            // The app's name is the bundle's (productName in tauri.conf.json, "Supragnosis"), as in
            // the macOS app menu; lowercase `supragnosis` is the CLI and the daemon binary.
            let name = app.package_info().name.clone();
            let quit = MenuItem::with_id(app, "quit", format!("Quit {name}"), true, None::<&str>)?;
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
            app.manage(TrayStatus(status));
            app.manage(ActiveRemote(Mutex::new(None)));
            app.manage(AppsHint(Mutex::new(None)));
            app.manage(Attention(Mutex::new(None)));
            app.manage(VizSock(sock.clone()));

            // The app menu keeps the platform's own items - Edit above all, without which a
            // credential cannot be pasted into the settings page - and gains the two pages:
            // Settings... (Cmd+,) where macOS users look for it, and Graph (Cmd+1) in View.
            let app_menu = Menu::default(app.handle())?;
            for sub in app_menu.items()?.iter().filter_map(|i| i.as_submenu().cloned()) {
                let text = sub.text()?;
                if text == "View" {
                    let graph =
                        MenuItem::with_id(app, "graph", "Graph", true, Some("CmdOrCtrl+1"))?;
                    sub.insert(&graph, 0)?;
                    sub.insert(&PredefinedMenuItem::separator(app)?, 1)?;
                } else if text == name {
                    let item = MenuItem::with_id(
                        app,
                        "settings",
                        "Settings...",
                        true,
                        Some("CmdOrCtrl+,"),
                    )?;
                    sub.insert(&item, 1)?;
                }
            }
            app.set_menu(app_menu)?;
            app.on_menu_event(|app, event| {
                let shown = match event.id().as_ref() {
                    "settings" => show_settings(app, None),
                    "graph" => show_graph(app),
                    _ => Ok(()),
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
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => {
                        if let Err(e) = show_graph(app) {
                            tracing::error!(error = %e, "failed to open the graph");
                        }
                    }
                    "settings" => {
                        // Straight to the section the status line is pointing at, if any.
                        let section =
                            app.try_state::<Attention>().and_then(|a| *a.0.lock().unwrap());
                        if let Err(e) = show_settings(app, section) {
                            tracing::error!(error = %e, "failed to open the settings");
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            tauri::async_runtime::spawn(bring_up(app.handle().clone(), sock.clone()));
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
            app.listen("shell-page-loaded", |event| {
                tracing::info!(page = %event.payload(), "webview page loaded");
            });
            show_viewer(app.handle())?;
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
                // Reap only a daemon WE spawned - an attached external daemon keeps running.
                let prev = std::mem::replace(
                    &mut *app.state::<DaemonGuard>().0.lock().unwrap(),
                    Daemon::Starting,
                );
                if let Daemon::Spawned(mut child) = prev {
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
    use super::{app_row, status_text, Daemon};

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
        assert_eq!(app_row(Some(&r(true, "other"))).1, Some("Replace"));
        assert_eq!(app_row(Some(&r(false, "none"))), ("not installed".into(), None, "absent"));
        assert_eq!(app_row(Some(&r(true, "unknown"))).1, None);
        assert_eq!(app_row(None).1, None, "an old CLI cannot connect anything");
        assert_eq!(
            app_row(Some(&r(true, "none"))),
            ("not connected".into(), Some("Connect"), "off")
        );
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

    /// The line names who runs the daemon - including a child of this app, which the CLI cannot see.
    #[test]
    fn the_line_names_the_manager() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let brew = serde_json::json!({ "type": "launchd", "source": "homebrew", "label": "sh.brew.supragnosis-server" });
        let s = st("one", Some("0.4.2"), "0.4.2", canonical);
        assert_eq!(status_text(&Daemon::External, Some(&s), None), "daemon 0.4.2 - login item");
        let s = st("one", Some("0.4.2"), "0.4.2", brew);
        assert_eq!(status_text(&Daemon::External, Some(&s), None), "daemon 0.4.2 - brew services");
        let s = st("unrecognized", Some("0.4.2"), "0.4.2", serde_json::Value::Null);
        assert_eq!(
            status_text(&Daemon::External, Some(&s), Some("no longer starts at login")),
            "daemon 0.4.2 - externally managed | no longer starts at login"
        );
    }

    /// The 2026-10-03 symptom, said where it is read: an upgraded binary under an old process.
    #[test]
    fn drift_and_conflict_are_said_outright() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let s = st("one", Some("0.4.0"), "0.4.2", canonical);
        assert_eq!(
            status_text(&Daemon::External, Some(&s), None),
            "daemon 0.4.0 running, 0.4.2 installed - restart it in Settings"
        );
        let conflict = serde_json::json!({
            "situation": "conflict",
            "managers": [{ "type": "launchd" }, { "type": "launchd" }],
            "version": { "here": "0.4.2", "running": "0.4.0" },
        });
        assert!(status_text(&Daemon::External, Some(&conflict), None)
            .starts_with("daemon: CONFLICT - 2 managers"));
    }

    /// Without a CLI that answers `status --json`, the shell says only what it knows itself.
    #[test]
    fn an_old_cli_falls_back_to_the_shells_own_view() {
        assert_eq!(
            status_text(&Daemon::External, None, None),
            "daemon: attached (externally managed)"
        );
        assert_eq!(status_text(&Daemon::Starting, None, None), "daemon: starting...");
    }
}

#[cfg(test)]
mod settings_tests {
    use super::{server_add_args, settings_caller_ok, settings_url, SETTINGS_COMMANDS};

    /// docs/settings-page.md S1: a setting changes only from the app's own settings page - not from
    /// the viewer the same window shows, which a daemon or a hub serves, and not from any other
    /// address, a hub's page or a lookalike path included.
    #[test]
    fn only_the_apps_own_page_may_change_a_setting() {
        let own = settings_url();
        let viewer: tauri::Url = "viz://localhost/".parse().unwrap();
        let hub: tauri::Url = "https://hub.example/viz/".parse().unwrap();
        let other_page: tauri::Url = "tauri://localhost/index.html".parse().unwrap();
        let windows_viewer: tauri::Url = "http://viz.localhost/settings.html".parse().unwrap();
        assert!(settings_caller_ok("main", &own));
        assert!(!settings_caller_ok("main", &viewer), "the viewer in the same window");
        assert!(!settings_caller_ok("main", &hub));
        assert!(!settings_caller_ok("main", &other_page), "another page of the app");
        assert!(!settings_caller_ok("main", &windows_viewer), "a viewer path that looks alike");
        assert!(!settings_caller_ok("other", &own), "a window that is not the main one");
    }

    /// S1, the other half: build.rs declares the commands so none is open by default, and the one
    /// capability that grants them grants every one, to the main window only. The lists are held to
    /// the one in main.rs.
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
        for cmd in SETTINGS_COMMANDS {
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

    /// S2 by construction: the arguments of `server add` are built without the credential, which
    /// reaches the CLI on stdin only.
    #[test]
    fn a_credential_never_becomes_an_argument() {
        assert_eq!(
            server_add_args("home", "https://hub.example", None),
            ["server", "add", "home", "https://hub.example"]
        );
        assert_eq!(
            server_add_args("home", "https://hub.example", Some(" /etc/ca.pem ")),
            ["server", "add", "home", "https://hub.example", "--ca", "/etc/ca.pem"]
        );
    }

    /// S5: the settings page renders the text it is given as text. It shows CLI output, which can
    /// quote a server's answer, so no markup sink may appear in its script at all.
    #[test]
    fn the_settings_page_never_renders_markup() {
        // The init script runs in every page the main window shows - the viewer included - and builds
        // the page navigation there, so it is held to the same rule.
        for (file, js) in [
            ("settings.js", include_str!("../assets/settings.js")),
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
}
