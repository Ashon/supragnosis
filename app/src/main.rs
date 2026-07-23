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

/// Locates the `supragnosis` server binary: explicit override -> installed location -> dev build
/// -> PATH. Shipping it inside the bundle (Tauri externalBin sidecar) is the packaging milestone's
/// job; this runtime search keeps dev and installed layouts working without it.
fn find_server_bin() -> Option<PathBuf> {
    if let Some(p) = env_nonempty("SUPRAGNOSIS_BIN") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let installed = home().join(".local/bin/supragnosis");
    if installed.exists() {
        return Some(installed);
    }
    if cfg!(debug_assertions) {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/supragnosis");
        if dev.exists() {
            return Some(dev);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("supragnosis"))
            .find(|c| c.exists())
    })
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
    let logs = home().join(".supragnosis/log");
    std::fs::create_dir_all(&logs).context("failed to create the daemon log dir")?;
    let out = std::fs::File::create(logs.join("app-daemon.out.log"))?;
    let err = std::fs::File::create(logs.join("app-daemon.err.log"))?;
    let http_addr = mcp_addr();
    tracing::info!(bin = %bin.display(), http = %http_addr, sock = %sock.display(), "spawning the daemon");
    let child = Command::new(&bin)
        .args(["serve", "--http", &http_addr, "--viz"])
        .arg(sock)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .with_context(|| format!("failed to spawn {}", bin.display()))?;
    for _ in 0..40 {
        if UnixStream::connect(sock).await.is_ok() {
            tracing::info!("daemon is up");
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(Some(child))
}

/// One GET over the socket -> (status, content-type, body). The daemon answers Connection: close,
/// so read-to-EOF terminates. /api/events must never come through here (an endless stream) - the
/// protocol handler short-circuits it.
async fn uds_fetch(sock: &Path, target: &str) -> anyhow::Result<(u16, String, Vec<u8>)> {
    let mut s = UnixStream::connect(sock).await?;
    s.write_all(format!("GET {target} HTTP/1.1\r\nConnection: close\r\n\r\n").as_bytes())
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
    let ctype = head
        .lines()
        .skip(1)
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-type")
                .then(|| v.trim().to_string())
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Ok((status, ctype, body))
}

fn resp(status: u16, ctype: &str, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("content-type", ctype)
        // The daemon marks every response no-store; the proxy must not launder that away -
        // without it WKWebView disk-caches the viewer assets and serves STALE pages across
        // app restarts (live-updating data and a hand-deployed daemon make caching all wrong).
        .header("cache-control", "no-store")
        .body(body)
        .unwrap_or_else(|_| http::Response::new(Vec::new()))
}

/// Served at `/` while the daemon's socket is not answering yet; refreshes itself into the real
/// viewer once it is. Palette mirrors the viewer's candlelight theme.
// data-tauri-drag-region: with the overlay title bar there is no other chrome to drag the
// window by while the splash is up.
const STARTING_HTML: &str = r#"<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="1"><title>supragnosis</title><body data-tauri-drag-region style="background:#0c0e14;color:#f0c469;font:14px ui-monospace,monospace;display:flex;align-items:center;justify-content:center;height:100vh;margin:0">starting the supragnosis daemon...</body>"#;

/// Runs attach-or-spawn and publishes the outcome (state + tray status line).
async fn bring_up(app: tauri::AppHandle, sock: PathBuf) {
    let state = match ensure_daemon(&sock).await {
        Ok(Some(child)) => Daemon::Spawned(child),
        Ok(None) => Daemon::External,
        Err(e) => {
            tracing::error!(error = %e, "daemon startup failed - the viewer will keep retrying");
            Daemon::Failed(e.to_string())
        }
    };
    *app.state::<DaemonGuard>().0.lock().unwrap() = state;
    refresh_status(&app);
}

fn refresh_status(app: &tauri::AppHandle) {
    let text = app.state::<DaemonGuard>().0.lock().unwrap().status_line();
    if let Some(status) = app.try_state::<TrayStatus>() {
        let _ = status.0.set_text(text);
    }
}

/// Tray "Restart Daemon": bounce whatever we manage, then attach-or-spawn again. A spawned child
/// is killed directly; an external daemon is bounced through the CLI (`supragnosis restart`
/// knows pidfile and launchd daemons); a foreign daemon the CLI cannot control is left alone and
/// bring_up simply re-attaches to it.
async fn restart_daemon(app: tauri::AppHandle, sock: PathBuf) {
    let prev = std::mem::replace(
        &mut *app.state::<DaemonGuard>().0.lock().unwrap(),
        Daemon::Starting,
    );
    refresh_status(&app);
    match prev {
        Daemon::Spawned(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
        }
        Daemon::External => {
            if let Some(bin) = find_server_bin() {
                let _ = tokio::task::spawn_blocking(move || {
                    Command::new(bin).arg("restart").status()
                })
                .await;
            }
        }
        Daemon::Starting | Daemon::Failed(_) => {}
    }
    bring_up(app, sock).await;
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
    .title("supragnosis")
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
    // survives window events).
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
        .register_asynchronous_uri_scheme_protocol("viz", move |_ctx, request, responder| {
            let sock = proxy_sock.clone();
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
                        br#"{"error":"SSE rides the Tauri event bridge (viz-event), not the proxy"}"#.to_vec(),
                    )
                } else {
                    match tokio::time::timeout(Duration::from_secs(15), uds_fetch(&sock, &target)).await {
                        Ok(Ok((status, ctype, body))) => resp(status, &ctype, body),
                        // Socket not answering: the index gets a self-refreshing splash (the daemon
                        // is still starting); API calls get an honest 502 (Principle 5).
                        _ if target == "/" => {
                            resp(200, "text/html; charset=utf-8", STARTING_HTML.as_bytes().to_vec())
                        }
                        Ok(Err(e)) => resp(
                            502,
                            "application/json",
                            serde_json::json!({ "error": format!("viewer socket unreachable: {e}") })
                                .to_string()
                                .into_bytes(),
                        ),
                        Err(_) => resp(
                            504,
                            "application/json",
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
            // Tray: the management surface for the resident shell.
            let open = MenuItem::with_id(app, "open", "Open Viewer", true, None::<&str>)?;
            let status = MenuItem::with_id(app, "status", "daemon: starting...", false, None::<&str>)?;
            let restart = MenuItem::with_id(app, "restart", "Restart Daemon", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit supragnosis", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &open,
                    &PredefinedMenuItem::separator(app)?,
                    &status,
                    &restart,
                    &PredefinedMenuItem::separator(app)?,
                    &quit,
                ],
            )?;
            app.manage(TrayStatus(status));
            let tray_sock = sock.clone();
            TrayIconBuilder::with_id("supragnosis")
                // Template image (bare mark, alpha-only): macOS recolors it for light/dark menu bars.
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("supragnosis")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => {
                        if let Err(e) = show_viewer(app) {
                            tracing::error!(error = %e, "failed to open the viewer window");
                        }
                    }
                    "restart" => {
                        tauri::async_runtime::spawn(restart_daemon(app.clone(), tray_sock.clone()));
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            tauri::async_runtime::spawn(bring_up(app.handle().clone(), sock.clone()));
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
            _ => {}
        });
}
