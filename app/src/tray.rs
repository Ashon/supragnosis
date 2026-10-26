//! The tray: the state line, the one act, the review item and Quit's words, from the CLI and the
//! shell's own state, and the menu and icon built from them.

use super::*;

/// What the tray last showed, so it is built again only when that changes (design-system.md
/// Section 29.13).
pub(crate) struct TrayKept(pub(crate) Mutex<Option<TrayState>>);

/// The app menu's Quit, whose words follow the tray's (D15).
pub(crate) struct AppQuit(pub(crate) PredefinedMenuItem<Rt>);

/// What a press of one of the tray's items does (design-system.md Section 29.13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TrayPress {
    OpenGraph,
    Review,
    FinishSetup,
    /// The act line and Settings... open the section that needs attention.
    Settings,
    Quit,
}

/// The tray's items that act, by id. Every menu event reaches the tray's listener, which acts only
/// on these; none is the menu bar's, so a press does only its own act.
pub(crate) const TRAY_IDS: &[(&str, TrayPress)] = &[
    ("tray-act", TrayPress::Settings),
    ("tray-finish-setup", TrayPress::FinishSetup),
    ("tray-review", TrayPress::Review),
    ("tray-open", TrayPress::OpenGraph),
    ("tray-settings", TrayPress::Settings),
    ("tray-quit", TrayPress::Quit),
];

/// The press the tray's item `id` makes; `None` for any other id, the menu bar's among them.
pub(crate) fn tray_press(id: &str) -> Option<TrayPress> {
    TRAY_IDS.iter().find(|(i, _)| *i == id).map(|(_, p)| *p)
}

/// What a press of the tray's item does: each opens a page of the main window, or quits the app.
pub(crate) fn tray_pressed(app: &tauri::AppHandle<Rt>, press: TrayPress) {
    let (shown, what) = match press {
        TrayPress::OpenGraph => (show_graph(app), "the graph"),
        TrayPress::Review => (show_review_all(app), "the review page"),
        TrayPress::FinishSetup => (show_welcome(app), "the welcome page"),
        TrayPress::Settings => {
            let section = app.try_state::<Attention>().and_then(|a| *a.0.lock().unwrap());
            (show_settings(app, section), "the settings")
        }
        TrayPress::Quit => return app.exit(0),
    };
    if let Err(e) = shown {
        tracing::error!(error = %e, "failed to open {what}");
    }
}

/// Whether quitting the app stops the memory: only while the daemon is this app's own child, which
/// lives as long as the app does (daemon-lifecycle.md L6).
pub(crate) fn quit_stops(daemon: &Daemon) -> bool {
    matches!(daemon, Daemon::Spawned(_))
}

/// Quit's words, the tray's and the app menu's (D15): they say it stops the memory when it does.
pub(crate) fn quit_label(stops: bool, name: &str) -> String {
    if stops {
        "Quit and Stop Memory".to_string()
    } else {
        format!("Quit {name}")
    }
}

/// Whether no AI app is connected yet, the state a new user is in (docs/client-connect.md Section 5):
/// the tray's act line asks to connect one. And whether any app's row needs the person - an entry
/// other than the bridge, or settings that could not be read - which marks AI apps (Section 29.14).
pub(crate) struct AppsHint(pub(crate) Mutex<(bool, bool)>);

/// The settings section the tray's Settings... opens: the one whose state the status line is
/// pointing at, if any (docs/settings-page.md Section 3.0).
pub(crate) struct Attention(pub(crate) Mutex<Option<&'static str>>);

/// What waits for a person on this Mac's daemon, across its workspaces (docs/design-system.md
/// Section 7): the open and blocked proposals, and with them, from the curation report, the suspected
/// secrets and the contested beliefs - all of Needs you. `needs` is `None` when the curation read
/// failed and the proposal list answered, and the item then says what it counted (P5).
#[derive(Debug, PartialEq)]
pub(crate) struct Waiting {
    pub(crate) proposals: usize,
    pub(crate) needs: Option<usize>,
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
pub(crate) fn curation_count(report: &serde_json::Value) -> Option<usize> {
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
pub(crate) fn review_item_text(w: &Waiting) -> Option<String> {
    match (w.needs, w.proposals) {
        (Some(0), _) | (None, 0) => None,
        (Some(1), _) => Some("Review - 1 needs you...".into()),
        (Some(n), _) => Some(format!("Review - {n} need you...")),
        (None, 1) => Some("Review - 1 proposal waits...".into()),
        (None, n) => Some(format!("Review - {n} proposals wait...")),
    }
}

/// The count `waiting_proposals` reports, from the daemon's proposal list.
pub(crate) fn review_count(list: &serde_json::Value) -> usize {
    list.as_array().map_or(0, |a| {
        a.iter()
            .filter(|p| matches!(p["state"].as_str(), Some("open" | "blocked")))
            .count()
    })
}

/// The tray's status line, from what the CLI observed plus the shell's own relationship to the
/// daemon (a child it spawned is invisible to the CLI - no pidfile, no launchd job).
/// Who runs the daemon, in the words the status line and the settings page use - including a child
/// of this app, which the CLI cannot see (no pidfile, no launchd job).
pub(crate) fn who_runs(daemon: &Daemon, st: &serde_json::Value) -> String {
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

pub(crate) fn status_text(daemon: &Daemon, st: Option<&serde_json::Value>) -> String {
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
pub(crate) fn daemon_trouble(daemon: &Daemon, st: Option<&serde_json::Value>) -> bool {
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
pub(crate) struct TrayState {
    line: String,
    act: Option<(&'static str, &'static str)>,
    trouble: bool,
    finish: bool,
    review: Option<String>,
    quit_stops: bool,
}

/// The section the tray's Settings... opens: the first whose dot shows, in the sidebar's order - AI
/// apps, then This Mac - or none, which reopens the last pane (design-system.md Section 29.14).
pub(crate) fn attention_section(
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
pub(crate) fn tray_act(
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
pub(crate) async fn refresh_status(app: &tauri::AppHandle<Rt>) {
    heal_failed(app).await;
    refresh_apps(app).await;
    let (no_app, apps_need) =
        app.try_state::<AppsHint>().map_or((false, false), |h| *h.0.lock().unwrap());
    // "Finish Setup..." while the daemon or the AI apps are not set up (docs/onboarding.md
    // Section 6), from setup's own checklist. A CLI that predates setup offers nothing.
    let pid = spawned_pid(app);
    let steps = on_cli(app, move |c| setup_check_blocking(c, pid))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|events| essentials(&events));
    let finish = finish_setup_offered(read_prefs(app), steps);
    let mut tray = TrayState { finish, ..TrayState::default() };
    let mut repairing = false;
    let st = on_cli(app, cli_status).await.ok().flatten();
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
        tray.quit_stops = quit_stops(&daemon);
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
fn apply_tray(app: &tauri::AppHandle<Rt>, t: &TrayState) {
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
    let quit = quit_label(t.quit_stops, &name);
    let built = (|| -> tauri::Result<Menu<Rt>> {
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
