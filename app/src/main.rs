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

/// The tray's "Review - N need you..." item (docs/design-system.md Section 7): in the menu while
/// something waits on this Mac's daemon, out of it otherwise. The menu and whether the item is in it
/// are kept with it.
struct TrayReview {
    menu: Menu<tauri::Wry>,
    item: MenuItem<tauri::Wry>,
    shown: Mutex<bool>,
}

/// The tray's "Finish setup..." item (docs/onboarding.md Section 6): in the menu while the daemon or the
/// AI apps are not set up, out of it otherwise.
struct TraySetup {
    menu: Menu<tauri::Wry>,
    item: MenuItem<tauri::Wry>,
    shown: Mutex<bool>,
}

/// Puts an optional tray item in the menu, below the status line, or takes it out.
fn tray_item_shown(
    menu: &Menu<tauri::Wry>,
    item: &MenuItem<tauri::Wry>,
    shown: &Mutex<bool>,
    on: bool,
) {
    let mut shown = shown.lock().unwrap();
    if on && !*shown && menu.insert(item, 2).is_ok() {
        *shown = true;
    } else if !on && *shown && menu.remove(item).is_ok() {
        *shown = false;
    }
}

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
    /// The ETag of the graph and the hypergraph (design-system.md U33), so the viewer can ask with it.
    etag: Option<String>,
    body: Vec<u8>,
}

/// One GET over the socket. The daemon answers Connection: close, so read-to-EOF terminates.
/// /api/events must never come through here (an endless stream) - the protocol handler
/// short-circuits it.
async fn uds_fetch(sock: &Path, target: &str) -> anyhow::Result<Fetched> {
    uds_request(sock, "GET", target, None).await
}

/// The webview's `If-None-Match`, when it is one this proxy can pass on: printable ASCII and no line
/// break, since it is written into the request head as it comes.
fn forwarded_if_none_match(value: Option<&http::HeaderValue>) -> Option<String> {
    let v = value?.to_str().ok()?;
    (!v.is_empty() && v.bytes().all(|b| (0x20..0x7f).contains(&b))).then(|| v.to_string())
}

/// One request over the socket, with no body - the viewer takes its parameters in the query string,
/// POST included (its one POST is the console's narrowing act, `/api/peer/share`).
async fn uds_request(
    sock: &Path,
    method: &str,
    target: &str,
    if_none_match: Option<&str>,
) -> anyhow::Result<Fetched> {
    let mut s = UnixStream::connect(sock).await?;
    let conditional = if_none_match.map(|t| format!("If-None-Match: {t}\r\n")).unwrap_or_default();
    s.write_all(
        format!(
            "{method} {target} HTTP/1.1\r\n{conditional}Content-Length: 0\r\nConnection: close\r\n\r\n"
        )
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
    Ok(Fetched {
        status,
        ctype,
        csp: header("content-security-policy"),
        etag: header("etag"),
        body,
    })
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
// data-tauri-drag-region="deep": with the overlay title bar there is no other chrome to drag the
// window by while the splash is up, and its text covers the middle, which the bare attribute would
// leave undraggable.
/// What the viewer shows while a remote profile is active: which server the AI apps use, and that
/// browsing it arrives with the network read tier (remote-server.md Section 5). Not an empty graph,
/// which would read as "no knowledge" (P5).
/// Its way back names the settings page, where the server choice lives (settings-page.md S4); the
/// tray kept a Server submenu once and this line still pointed at it after the move.
fn remote_html(r: &RemoteServer) -> String {
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let state = match (r.answering, r.credential_refused) {
        (true, true) => "it answers, but refused this machine's credential",
        (true, false) => "it answers",
        (false, _) => "it does not answer right now",
    };
    format!(
        r#"<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="30"><title>supragnosis</title><style>@media (prefers-color-scheme: light) {{ body {{ background:#fcfbf8 !important; color:#16181e !important }} .m {{ color:#5c6370 !important }} }}</style><body data-tauri-drag-region="deep" style="background:#08090d;color:#e9e4d6;font:13px/1.5 'Pretendard Variable',Pretendard,system-ui,-apple-system,'Segoe UI',sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center;max-width:80%"><div>AI apps on this Mac use the server "{name}"</div><div class="m" style="color:#b0b8c7;margin-top:10px;font-size:12px;word-break:break-word">{url} - {state}</div><div class="m" style="color:#b0b8c7;margin-top:14px;font-size:12px">browsing a remote server's knowledge here arrives with a later release / Settings (Cmd+,) &gt; Server &gt; This Mac switches back</div></div></body>"#,
        name = esc(&r.name),
        url = esc(&r.url),
    )
}

fn starting_html(status: &str, conflict: Option<&Conflict>, settings: &tauri::Url) -> String {
    let esc = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    // A link that reads as a button: the page has no script (SHELL_CSP), and its one act is opening
    // the app's own page, where the command is (docs/settings-page.md Section 3.3, S1).
    const BUTTON: &str = "display:inline-block;margin-top:18px;padding:7px 16px;border-radius:8px;font-weight:600;text-decoration:none";
    let mut to = settings.clone();
    to.set_fragment(Some("daemon"));
    let (head, detail, action, foot) = match conflict {
        // The daemon cannot open its store for its model (semantic-retrieval.md Section 13): the
        // reason, and Rebuild index, which opens Settings to start it.
        Some((recorded, daemon)) => {
            to.set_query(Some("act=rebuild"));
            (
                "The search index needs rebuilding".to_string(),
                format!(
                    "Indexed by {}, and the daemon runs {} - it cannot start until the index is rebuilt.",
                    esc(recorded),
                    esc(daemon)
                ),
                format!(
                    r#"<a href="{}" style="{BUTTON};background:#d9a544;color:#16181e">Rebuild index</a>"#,
                    esc(to.as_str())
                ),
                "or in a terminal: supragnosis embed rebuild".to_string(),
            )
        }
        None => (
            "starting the supragnosis daemon...".to_string(),
            esc(status),
            // A daemon that failed to start: This Mac, where Restart is.
            if status.contains("FAILED") {
                format!(
                    r#"<a class="b2" href="{}" style="{BUTTON};border:1px solid #3a3f4a;color:#e9e4d6">Open This Mac</a>"#,
                    esc(to.as_str())
                )
            } else {
                String::new()
            },
            "log: ~/.supragnosis/log/app-daemon.err.log / Settings (Cmd+,) &gt; This Mac &gt; Restart".to_string(),
        ),
    };
    format!(
        r#"<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="1"><title>supragnosis</title><style>@media (prefers-color-scheme: light) {{ body {{ background:#fcfbf8 !important; color:#16181e !important }} .m {{ color:#5c6370 !important }} .b2 {{ color:#16181e !important; border-color:#c9ccd3 !important }} }}</style><body data-tauri-drag-region="deep" style="background:#08090d;color:#e9e4d6;font:13px/1.5 'Pretendard Variable',Pretendard,system-ui,-apple-system,'Segoe UI',sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center;max-width:80%"><div>{head}</div><div class="m" style="color:#b0b8c7;margin-top:10px;font-size:12px;word-break:break-word">{detail}</div>{action}<div class="m" style="color:#b0b8c7;margin-top:14px;font-size:12px">{foot}</div></div></body>"#
    )
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
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::null());
    with_login_env(&mut cmd);
    cmd.output().ok()
}

/// What the person's login shell sets that the CLI's calls need, read once per launch
/// (docs/client-connect.md Section 5). An app opened from the Finder gets
/// `/usr/bin:/bin:/usr/sbin:/sbin`, so a client installed through nvm, fnm, volta or mise is on the
/// shell's `PATH` only, and the CLI called it not installed; and a shell that sets
/// `XDG_CONFIG_HOME` moves OpenCode's configuration, which the CLI then looked for in the wrong
/// place.
#[derive(Debug, Default, PartialEq)]
struct LoginEnv {
    path: Option<String>,
    config_home: Option<String>,
}

/// `None` when the shell could not be asked; the app's own environment then stands.
fn login_env() -> Option<&'static LoginEnv> {
    static LOGIN_ENV: OnceLock<Option<LoginEnv>> = OnceLock::new();
    LOGIN_ENV
        .get_or_init(|| {
            let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
            read_login_env(Path::new(&shell), Duration::from_secs(5))
        })
        .as_ref()
}

fn login_path() -> Option<&'static str> {
    login_env()?.path.as_deref()
}

/// A CLI call's environment: the login shell's `PATH` and `XDG_CONFIG_HOME` over the app's own.
fn with_login_env(cmd: &mut Command) {
    if let Some(path) = cli_path(login_path(), std::env::var_os("PATH").as_deref()) {
        cmd.env("PATH", path);
    }
    if let Some(dir) = login_env().and_then(|e| e.config_home.as_deref()) {
        cmd.env("XDG_CONFIG_HOME", dir);
    }
}

/// The line `read_login_env` prints before the environment, so whatever the shell's startup files
/// print first is skipped.
const ENV_MARK: &str = "__supragnosis_env__";

/// Asks `shell`, as a login and interactive shell, for its environment, and returns what it sets.
/// `None` when it does not answer within `within`, or answers without a `PATH`. Interactive, because
/// nvm and its kind are set up in `.zshrc` and `.bashrc`, which a login shell alone does not read.
fn read_login_env(shell: &Path, within: Duration) -> Option<LoginEnv> {
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("echo {ENV_MARK}; /usr/bin/env")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Read on a thread: a startup file that leaves a process holding the pipe must not hang the app.
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        let _ = tx.send(out);
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < within => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = rx.recv_timeout(within.saturating_sub(started.elapsed())).ok()?;
    login_env_from_output(&out)
}

/// The `PATH=` and `XDG_CONFIG_HOME=` lines of `env`'s output after [`ENV_MARK`]; anything printed
/// before the mark is not the environment. `None` without a `PATH`.
fn login_env_from_output(out: &str) -> Option<LoginEnv> {
    let after = out.split_once(&format!("{ENV_MARK}\n"))?.1;
    let var = |key: &str| {
        after
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let path = var("PATH=")?;
    Some(LoginEnv { path: Some(path), config_home: var("XDG_CONFIG_HOME=") })
}

/// The `PATH` the app's CLI calls run with: the login shell's directories first, then those of the
/// app's own that the shell's did not name. `None` leaves the app's `PATH` as it is.
fn cli_path(login: Option<&str>, own: Option<&std::ffi::OsStr>) -> Option<std::ffi::OsString> {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(login?).collect();
    for dir in own.map(std::env::split_paths).into_iter().flatten() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).ok()
}

/// The highest schema of the CLI's machine-read answers this app reads (docs/compatibility.md
/// Section 6). An answer without one is from a CLI before the contract: its fields are the ones
/// this app already reads, and degrades on when they are missing.
const CLI_SCHEMA: u64 = 1;

/// Whether the last machine-read answer carried a schema newer than [`CLI_SCHEMA`]. The tray line
/// and the Settings page then say to update the app, instead of reading a payload whose meaning
/// moved.
static CLI_NEWER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One machine-read CLI answer, by its schema. `Err` carries a schema newer than this app reads,
/// whose payload is not read at all; `Ok(None)` is output that is not JSON.
fn cli_json(stdout: &[u8]) -> Result<Option<serde_json::Value>, u64> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(stdout) else {
        return Ok(None);
    };
    match v["schema"].as_u64() {
        Some(s) if s > CLI_SCHEMA => Err(s),
        _ => Ok(Some(v)),
    }
}

/// Runs a `--json` command. `None` when there is no answer to read: no CLI, a CLI that does not
/// know the command, or one newer than this app (which [`CLI_NEWER`] records).
fn run_cli_json(args: &[&str]) -> Option<serde_json::Value> {
    let out = run_cli(args)?;
    if !out.status.success() {
        return None;
    }
    let read = cli_json(&out.stdout);
    CLI_NEWER.store(read.is_err(), std::sync::atomic::Ordering::Relaxed);
    read.ok().flatten()
}

/// `supragnosis status --json`. `None` when the CLI predates it - the tray then falls back to what
/// the shell itself knows and disables the switch rather than failing on click.
fn cli_status() -> Option<serde_json::Value> {
    run_cli_json(&["status", "--json"])
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
    run_cli_json(&["server", "--json"])
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

/// The tray's line when the CLI answers in a schema this app does not read.
const CLI_NEWER_LINE: &str = "supragnosis-server is newer than this app - update the app";

/// Re-reads the CLI's view and republishes the tray's one line (docs/settings-page.md Section 5).
async fn refresh_status(app: &tauri::AppHandle) {
    heal_failed(app).await;
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
            if CLI_NEWER.load(std::sync::atomic::Ordering::Relaxed) {
                (CLI_NEWER_LINE.to_string(), Some("daemon"))
            } else {
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
        }
    };
    if let Some(status) = app.try_state::<TrayStatus>() {
        let _ = status.0.set_text(text);
    }
    if let Some(a) = app.try_state::<Attention>() {
        *a.0.lock().unwrap() = attention;
    }
    // The review item counts this Mac's daemon only; on a remote profile the tray stays as
    // remote-viewer.md Section 6 has it.
    let text = if active_remote(app).is_none() {
        waiting(&app.state::<VizSock>().0).await.as_ref().and_then(review_item_text)
    } else {
        None
    };
    if let Some(r) = app.try_state::<TrayReview>() {
        if let Some(t) = &text {
            let _ = r.item.set_text(t);
        }
        tray_item_shown(&r.menu, &r.item, &r.shown, text.is_some());
    }
    // "Finish setup..." while the daemon or the AI apps are not set up (docs/onboarding.md
    // Section 6), from setup's own checklist. A CLI that predates setup offers nothing.
    let pid = spawned_pid(app);
    let steps = tokio::task::spawn_blocking(move || setup_check_blocking(pid))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|events| essentials(&events));
    if let Some(t) = app.try_state::<TraySetup>() {
        tray_item_shown(&t.menu, &t.item, &t.shown, finish_setup_offered(read_prefs(app), steps));
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
async fn restart_daemon(app: &tauri::AppHandle, sock: PathBuf) -> Result<String, String> {
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    refresh_status(app).await;
    let outcome = match prev {
        Daemon::Spawned(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok("restarted the daemon this app runs".to_string())
        }
        Daemon::External => {
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
        Daemon::Starting | Daemon::Failed(_) => Ok("starting the daemon".to_string()),
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

/// The commands the app's own pages call - the list `generate_handler!` registers below. build.rs
/// declares the same list in the app manifest, so none of them is open to any page by default;
/// capabilities/settings.json grants them to the main window, and each one refuses unless that
/// window is showing one of the app's own pages (S1). A test holds the lists together.
#[cfg(test)]
const APP_COMMANDS: &[&str] = &[
    "settings_state",
    "server_use",
    "server_add",
    "server_remove",
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
];

/// The app's own pages: bundled with it, never served by a daemon or a hub, and the only pages its
/// commands answer - Settings (docs/settings-page.md) and Welcome (docs/onboarding.md Section 6).
const OWN_PAGES: &[&str] = &["settings.html", "welcome.html"];

/// The address of one of the app's own pages.
fn own_page(name: &str) -> tauri::Url {
    let url = if cfg!(windows) {
        format!("http://tauri.localhost/{name}")
    } else {
        format!("tauri://localhost/{name}")
    };
    url.parse().expect("static url")
}

fn settings_url() -> tauri::Url {
    own_page("settings.html")
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
    heal_failed(app).await;
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
        "cli_newer": CLI_NEWER.load(std::sync::atomic::Ordering::Relaxed),
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
            // The daemon's own report on search by meaning (docs/onboarding.md Section 6): null when
            // it runs without a model or does not answer.
            "semantic": status.as_ref().map(|v| v["semantic"].clone()),
            // A store the daemon cannot open for its model (docs/settings-page.md Section 3.3): the
            // two models, or null.
            "embedder_conflict": status.as_ref().map(|v| v["embedder_conflict"].clone()),
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
    own_page_caller(&webview)?;
    Ok(settings_state_of(&app).await)
}

#[tauri::command]
async fn server_use(
    webview: tauri::Webview,
    app: tauri::AppHandle,
    name: String,
) -> Result<String, String> {
    own_page_caller(&webview)?;
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
    own_page_caller(&webview)?;
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
    own_page_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    Ok(remove_server(&app, sock, &name).await)
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
    Ok(toggle_app(&app, &id).await)
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
    if active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
    // Turning it off is a choice the app remembers, so Welcome and the tray stop asking
    // (docs/onboarding.md Section 4.1); turning it on forgets it.
    let mut p = read_prefs(&app);
    p.login_off = !on;
    let _ = write_prefs(&app, p);
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
    own_page_caller(&webview)?;
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
    if active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
    let sock = app.state::<VizSock>().0.clone();
    restart_daemon(&app, sock).await
}

#[tauri::command]
async fn search_rebuild(webview: tauri::Webview, app: tauri::AppHandle) -> Result<String, String> {
    own_page_caller(&webview)?;
    if active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
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
    if let Some(path) = view_prefs_path(app) {
        let kept = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            std::fs::write(&path, serde_json::json!({ "text_scale": scale }).to_string())
        });
        if let Err(e) = kept {
            tracing::warn!(error = %e, "the text size was not kept");
        }
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
        #[cfg(unix)]
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
    if installs && active_remote(&app).is_some() {
        return Err(
            "a remote server is active - this Mac's daemon is not what the AI apps use".into()
        );
    }
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
        "remote": active_remote(&app).map(|r| r.name),
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
    Ok("Claude Desktop restarted and loaded supragnosis.".into())
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
            "supragnosis app {}\n\n$ supragnosis {}\n{}\n$ supragnosis status --json\n{}",
            env!("CARGO_PKG_VERSION"),
            args.join(" "),
            text(run_cli(&args)),
            text(run_cli(&["status", "--json"])),
        )
    })
    .await
    .map_err(|e| e.to_string())
}

/// Turns the main window to the Welcome page, creating the window first if needed.
fn show_welcome(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if !w.url().is_ok_and(|u| is_page(&u, "welcome.html")) {
            w.navigate(welcome_url())?;
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
    let url = if cfg!(windows) { "http://viz.localhost/" } else { "viz://localhost/" };
    url.parse().expect("static url")
}

/// Turns the main window to the settings page (docs/settings-page.md Section 3.0), creating the
/// window first if the app has retreated to the tray. With a section, opens that section; without
/// one, a window already on the settings page stays where it is.
fn show_settings(app: &tauri::AppHandle, section: Option<&str>) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let there = w.url().is_ok_and(|u| is_page(&u, "settings.html"));
        if !there || section.is_some() {
            let mut url = settings_url();
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
        if w.url().is_ok_and(|u| own_page_ok("main", &u)) {
            let mut url = graph_url();
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
        if w.url().is_ok_and(|u| own_page_ok("main", &u)) {
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
        .manage(RunningSteps(Mutex::new(std::collections::HashMap::new())))
        .invoke_handler(tauri::generate_handler![
            settings_state,
            server_use,
            server_add,
            server_remove,
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
                    let fetch = uds_request(&sock, "GET", &target, if_none_match.as_deref());
                    match tokio::time::timeout(Duration::from_secs(15), fetch).await {
                        // The daemon's own policy governs the daemon's own page. An answer with no
                        // policy is forwarded with none, so a daemon that stopped sending one shows
                        // up as that rather than as a shell-authored guess (see SHELL_CSP).
                        Ok(Ok(f)) => {
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
                            let status = app.state::<DaemonGuard>().0.lock().unwrap().status_line();
                            let conflict = splash_conflict().await;
                            let html = starting_html(&status, conflict.as_ref(), &settings_url());
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
            // The login shell's PATH, asked once in the background, so the first CLI call the
            // Settings page makes does not wait for the shell to start (client-connect.md Section 5).
            std::thread::spawn(|| {
                let _ = login_env();
            });
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
            // Out of the menu until something waits (refresh_status inserts it).
            let review = MenuItem::with_id(app, "review", "Review...", true, None::<&str>)?;
            app.manage(TrayReview { menu: menu.clone(), item: review, shown: Mutex::new(false) });
            let finish = MenuItem::with_id(app, "finish-setup", "Finish Setup...", true, None::<&str>)?;
            app.manage(TraySetup { menu: menu.clone(), item: finish, shown: Mutex::new(false) });
            app.manage(ActiveRemote(Mutex::new(None)));
            app.manage(AppsHint(Mutex::new(None)));
            app.manage(Attention(Mutex::new(None)));
            app.manage(VizSock(sock.clone()));
            app.manage(TextScale(Mutex::new(read_text_scale(app.handle()))));

            // The app menu keeps the platform's own items - Edit above all, without which a
            // credential cannot be pasted into the settings page - and gains the two pages:
            // Settings... (Cmd+,) where macOS users look for it, and Graph (Cmd+1) in View.
            let app_menu = Menu::default(app.handle())?;
            for sub in app_menu.items()?.iter().filter_map(|i| i.as_submenu().cloned()) {
                let text = sub.text()?;
                if text == "View" {
                    let graph =
                        MenuItem::with_id(app, "graph", "Graph", true, Some("CmdOrCtrl+1"))?;
                    let review =
                        MenuItem::with_id(app, "review", "Review", true, Some("CmdOrCtrl+2"))?;
                    // Text that can grow (design-system.md Section 10, U21): the root's size,
                    // not the webview's zoom, so the title bar and its traffic lights stay put.
                    let bigger = MenuItem::with_id(
                        app,
                        "text-bigger",
                        "Make Text Bigger",
                        true,
                        Some("CmdOrCtrl+="),
                    )?;
                    let smaller = MenuItem::with_id(
                        app,
                        "text-smaller",
                        "Make Text Smaller",
                        true,
                        Some("CmdOrCtrl+-"),
                    )?;
                    let actual =
                        MenuItem::with_id(app, "text-actual", "Actual Size", true, Some("CmdOrCtrl+0"))?;
                    sub.insert(&graph, 0)?;
                    sub.insert(&review, 1)?;
                    sub.insert(&PredefinedMenuItem::separator(app)?, 2)?;
                    sub.insert(&bigger, 3)?;
                    sub.insert(&smaller, 4)?;
                    sub.insert(&actual, 5)?;
                    sub.insert(&PredefinedMenuItem::separator(app)?, 6)?;
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
                    "review" => show_review(app),
                    "text-bigger" => set_text_scale(app, 1),
                    "text-smaller" => set_text_scale(app, -1),
                    "text-actual" => set_text_scale(app, 0),
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
                    "review" => {
                        if let Err(e) = show_review_all(app) {
                            tracing::error!(error = %e, "failed to open the review page");
                        }
                    }
                    "finish-setup" => {
                        if let Err(e) = show_welcome(app) {
                            tracing::error!(error = %e, "failed to open the welcome page");
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
            app.listen("shell-page-loaded", |event| {
                tracing::info!(page = %event.payload(), "webview page loaded");
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
    use super::{app_row, cli_path, login_env_from_output, read_login_env, status_text, Daemon};
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
        assert_eq!(app_row(Some(&r(true, "other"))).1, Some("Replace"));
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
    use super::{
        manager_pid, own_page_ok, restart_outcome, server_add_args, settings_url, starting_html,
        welcome_url, APP_COMMANDS,
    };

    /// docs/settings-page.md S1 and docs/onboarding.md Section 6: a setting changes only from the
    /// app's own pages, Settings and Welcome - not from the viewer the same window shows, which a
    /// daemon or a hub serves, and not from any other address, a hub's page or a lookalike path
    /// included.
    #[test]
    fn only_the_apps_own_page_may_change_a_setting() {
        let viewer: tauri::Url = "viz://localhost/".parse().unwrap();
        let hub: tauri::Url = "https://hub.example/viz/".parse().unwrap();
        let other_page: tauri::Url = "tauri://localhost/index.html".parse().unwrap();
        let windows_viewer: tauri::Url = "http://viz.localhost/settings.html".parse().unwrap();
        let hub_welcome: tauri::Url = "https://hub.example/welcome.html".parse().unwrap();
        for own in [settings_url(), welcome_url()] {
            assert!(own_page_ok("main", &own), "{own}");
            assert!(!own_page_ok("other", &own), "a window that is not the main one");
        }
        assert!(!own_page_ok("main", &viewer), "the viewer in the same window");
        assert!(!own_page_ok("main", &hub));
        assert!(!own_page_ok("main", &other_page), "another page of the app");
        assert!(!own_page_ok("main", &windows_viewer), "a viewer path that looks alike");
        assert!(!own_page_ok("main", &hub_welcome), "a hub's page named like the app's");
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
        let src = include_str!("main.rs");
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
                && daemon.contains("remote || drift || conflict ? null :"),
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
        let page = starting_html("daemon: FAILED - x", Some(&conflict), &settings_url());
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
        let failed = starting_html("daemon: FAILED - port in use", None, &settings_url());
        assert!(
            failed.contains("settings.html#daemon\"") && failed.contains(">Open This Mac</a>"),
            "a failed start does not open This Mac"
        );
        assert!(
            !starting_html("daemon: starting...", None, &settings_url()).contains("</a>"),
            "a start in progress offers a button"
        );
        assert_eq!(
            super::conflict_of(
                &serde_json::json!({"embedder_conflict": {"recorded": "a", "daemon": "b"}})
            ),
            Some(("a".to_string(), "b".to_string()))
        );
        assert_eq!(super::conflict_of(&serde_json::json!({"embedder_conflict": null})), None);
        let asked = body(js, "function rebuildAsked(");
        assert!(
            asked.contains("get(\"act\") !== \"rebuild\"")
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

        let src = include_str!("main.rs");
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
    /// padding; so the header drags deep, the search's results opt out, and the splash and the remote
    /// page, whose text covers their middle, drag deep too.
    #[test]
    fn the_title_bar_drags_the_window_but_not_its_controls() {
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("header.setAttribute(\"data-tauri-drag-region\", \"deep\");"));
        assert!(shell.contains("results.setAttribute(\"data-tauri-drag-region\", \"false\")"));
        assert!(
            !shell.contains("h1.setAttribute(\"data-tauri-drag-region\""),
            "a bare attribute on the title stops a press on its scope from dragging"
        );
        assert!(starting_html("x", None, &settings_url())
            .contains("<body data-tauri-drag-region=\"deep\""));
        let src = include_str!("main.rs");
        let pages = &src[..src.find("#[cfg(test)]").expect("the tests")];
        assert_eq!(pages.matches("<body data-tauri-drag-region=\"deep\"").count(), 2);
        assert!(!pages.contains("<body data-tauri-drag-region style="));
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
        assert!(include_str!("main.rs").contains("window.emit(\"shell-fullscreen\", fs)"));
        // isFullscreen is one of the window getters core:default grants.
        let caps = include_str!("../capabilities/default.json");
        assert!(caps.contains("\"core:default\""));
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
    /// its object, and a refused form says which field and why - to a screen reader too - with focus
    /// moved to the first such field. A refusal from the CLI says that the credential was cleared.
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
        assert!(js.contains("\"Remove \" + p.label"), "the remove button has no name");
        let add = body("openAddServer");
        for part in ["aria-invalid", "aria-describedby", ".focus()"] {
            assert!(add.contains(part), "the form's errors do not use {part}");
        }
        assert!(
            add.contains("For safety the credential was cleared. Paste it again."),
            "a refusal does not say the credential was cleared"
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
        let main = include_str!("main.rs");
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
        assert_eq!(splash.len(), 2, "the shell authors two splash pages");
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
        let main = include_str!("main.rs");
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
        let main = include_str!("main.rs");
        assert!(
            html.contains("<h1 id=\"pageTitle\">Setup</h1>"),
            "the title bar does not say Setup"
        );
        assert!(
            html.contains("Supragnosis keeps your memory on this Mac. It is shared only if you add a remote server or set up sync."),
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
            js.contains("h(tick ? \"label\" : \"div\""),
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
            settings.contains("load().then(adoptDownload).then(rebuildAsked);"),
            "Settings does not pick up a running download"
        );
    }

    /// design-system.md Section 6a: the Settings page says each fact once. Version drift is one card
    /// with the one Restart; a section's dot appears only when it needs attention, in words a screen
    /// reader reads; the server list has no radio that cannot be pressed; a peer is one row.
    #[test]
    fn the_settings_page_says_each_fact_once() {
        let js = include_str!("../assets/settings.js");
        let css = include_str!("../assets/settings.css");
        assert!(
            js.contains("\"Update ready - restart to load \" + d.installed"),
            "drift is not one card"
        );
        // A store the daemon cannot open takes Restart away too: its card holds Rebuild index (S7).
        assert!(
            js.contains("remote || drift || conflict ? null :"),
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
            !js.contains("class: \"radio\"") && !css.contains(".radio"),
            "the server list draws a radio"
        );
        assert!(js.contains("text: \"Active\""), "the active server is not said");
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
        let main = include_str!("main.rs");
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
            "Supragnosis keeps your memory on this Mac. It is shared only if you add a remote server or set up sync."
        ));
    }
}

/// docs/compatibility.md Section 6, the app's side. The CLI's own tests hold its `--json` output
/// to one example document per command; these hold this app's readers to the same documents, so a
/// field read here that the CLI never sends fails here, and a field the CLI renames fails there.
#[cfg(test)]
mod cli_contract_tests {
    use super::{app_row, cli_json, status_text, who_runs, Daemon, CLI_SCHEMA};

    const STATUS: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/status.json");
    const CONNECT: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/connect.json");
    const SERVERS: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/servers.json");

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
        let reads: [(&str, &[&str]); 3] = [
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
            (
                SERVERS,
                &[
                    "/active",
                    "/servers/0/name",
                    "/servers/0/url",
                    "/servers/0/remote",
                    "/servers/0/active",
                    "/check/answering",
                    "/check/credential",
                    "/check/detail",
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
        assert!(status_text(&Daemon::External, Some(&st), None).contains("CONFLICT"));
        let mut one = st.clone();
        one["situation"] = "one".into();
        one["managers"] = serde_json::json!([st["managers"][1].clone()]);
        assert_eq!(who_runs(&Daemon::External, &one), "login item");
        let line = status_text(&Daemon::External, Some(&one), None);
        assert!(line.contains("0.4.7 running, 0.4.8 installed"), "{line}");

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
