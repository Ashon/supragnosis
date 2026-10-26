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

mod apps;
mod cli;
mod daemon;
mod menu;
mod proxy;
mod settings;
mod splash;
mod tray;
mod welcome;
mod window;

use apps::*;
use cli::*;
use daemon::*;
use menu::*;
use proxy::*;
use settings::*;
use splash::*;
use tray::*;
use welcome::*;
use window::*;

/// The runtime the app runs on: the platform's webview, and in the tests Tauri's mock of it, so a
/// test can hold the app and run its commands, its tray and its exit.
mod rt {
    #[cfg(not(test))]
    pub(crate) type Rt = tauri::Wry;
    #[cfg(test)]
    pub(crate) type Rt = tauri::test::MockRuntime;
}
use rt::Rt;

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

fn main() {
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
    let sock = viz_sock();

    let proxy_sock = sock.clone();
    tauri::Builder::<Rt>::new()
        .manage(DaemonGuard(Mutex::new(Daemon::Starting)))
        .manage(CliHeld(std::sync::Arc::new(FoundCli)))
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
                            let (html, key) = starting_page(&app).await;
                            splash_drawn(&app, sock.clone(), key);
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
            let item = |id: &str| {
                let (id, words, key, _) = menu_item(id).expect("an item of MENU_ITEMS");
                MenuItem::with_id(app, id, words, true, key)
            };
            let mut map_items: Vec<(&'static str, MenuItem<Rt>)> = Vec::new();
            for sub in app_menu.items()?.iter().filter_map(|i| i.as_submenu().cloned()) {
                let text = sub.text()?;
                if text == "View" {
                    // The map's commands (design-system.md Section 29.13), kept so each page's load
                    // report can say which of them act there.
                    let mapped = [
                        "view-filters",
                        "view-log",
                        "view-map",
                        "view-list",
                        "view-fit",
                        "view-keys",
                        "view-back",
                        "view-forward",
                    ];
                    let mut kept = Vec::new();
                    for id in mapped {
                        kept.push((id, item(id)?));
                    }
                    let at = |id: &str| kept.iter().find(|(k, _)| *k == id).map(|(_, m)| m.clone()).unwrap();
                    // Text that can grow (design-system.md Section 10, U21): the root's size, not the
                    // webview's zoom, so the title bar and its traffic lights stay put.
                    let items: Vec<Box<dyn tauri::menu::IsMenuItem<Rt>>> = vec![
                        Box::new(item("graph")?),
                        Box::new(item("review")?),
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
                        Box::new(item("text-bigger")?),
                        Box::new(item("text-smaller")?),
                        Box::new(item("text-actual")?),
                        Box::new(PredefinedMenuItem::separator(app)?),
                    ];
                    for (i, it) in items.iter().enumerate() {
                        sub.insert(it.as_ref(), i)?;
                    }
                    map_items.extend(kept);
                } else if text == "Edit" {
                    let find = item("edit-find")?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&find)?;
                    map_items.push(("edit-find", find));
                } else if text == "Help" {
                    sub.append(&item("help-docs")?)?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&item("help-diagnostics")?)?;
                    sub.append(&item("help-log")?)?;
                    sub.append(&PredefinedMenuItem::separator(app)?)?;
                    sub.append(&item("help-issue")?)?;
                } else if text == name {
                    // About, a separator, Settings... and Setup..., a separator, then the
                    // platform's own items; its Quit takes the tray's words (D15).
                    sub.insert(&PredefinedMenuItem::separator(app)?, 1)?;
                    sub.insert(&item("settings")?, 2)?;
                    sub.insert(&item("setup")?, 3)?;
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
            app.on_menu_event(|app, event| menu_pressed(app, event.id().as_ref(), open_url));

            TrayIconBuilder::with_id("supragnosis")
                // Template image (bare mark, alpha-only): macOS recolors it for light/dark menu bars.
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip(&name)
                .menu(&menu)
                .show_menu_on_left_click(true)
                // Every menu event reaches this listener; it acts only on the tray's own ids.
                .on_menu_event(move |app, event| {
                    if let Some(press) = tray_press(event.id().as_ref()) {
                        tray_pressed(app, press);
                    }
                })
                .build(app)?;

            // The Welcome page opens at once on the first launch; on a later one, once the daemon is
            // up and setup has said whether the daemon and the apps are set up (Section 6).
            let first = !read_prefs(app.handle()).seen;
            tauri::async_runtime::spawn(start_up(app.handle().clone(), sock.clone(), first));
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
            RunEvent::Exit => on_exit(app),
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

/// The app's own source, for the tests that read it by its text: every module this file declares, in
/// its order, then this file. No test file is in it, so a needle cannot match the test's own copy of
/// it; `src_is_the_apps_modules_and_no_test` holds the list to the `mod` lines above.
#[cfg(test)]
const SRC: &str = concat!(
    include_str!("apps.rs"),
    include_str!("cli.rs"),
    include_str!("daemon.rs"),
    include_str!("menu.rs"),
    include_str!("proxy.rs"),
    include_str!("settings.rs"),
    include_str!("splash.rs"),
    include_str!("tray.rs"),
    include_str!("welcome.rs"),
    include_str!("window.rs"),
    include_str!("main.rs"),
);

#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod tests;
