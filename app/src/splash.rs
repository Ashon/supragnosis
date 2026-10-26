//! The starting page: what the window shows while the daemon's socket is silent, and the watch
//! that reloads it when the daemon answers or the page should say something else.

use super::*;

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
pub(crate) enum StartPage {
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
pub(crate) static START_AT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

pub(crate) fn mark_start() {
    *START_AT.lock().unwrap() = Some(std::time::Instant::now());
}

/// The page for the shell's state, `since` the start it made; it is drawn only while the socket is
/// silent. A child the app spawned that is alive is still starting - the app stops waiting on it at
/// 10 seconds, and a cold start can take longer - and one that exited is not; a daemon the app
/// attached to that went silent stopped answering.
pub(crate) fn start_page(d: &mut Daemon, since: Option<Duration>) -> StartPage {
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

pub(crate) fn starting_html(
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
pub(crate) static SPLASH: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn splash_drawn(app: &tauri::AppHandle<Rt>, sock: PathBuf, key: String) {
    let watching = SPLASH.lock().unwrap().replace(key).is_some();
    if !watching {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { watch_splash(app, sock).await });
    }
}

/// How often the watch looks again while a starting page is drawn.
pub(crate) const SPLASH_BEAT: Duration = Duration::from_secs(1);

/// Whether the watch reloads the window on its beat: when the daemon answers - the graph then - or
/// when the page would now say something else than the one `drawn`.
pub(crate) fn splash_reloads(drawn: &str, answering: bool, now: &str) -> bool {
    answering || now != drawn
}

async fn watch_splash(app: tauri::AppHandle<Rt>, sock: PathBuf) {
    loop {
        tokio::time::sleep(SPLASH_BEAT).await;
        let Some(drawn) = SPLASH.lock().unwrap().clone() else {
            return;
        };
        if splash_beat(&app, &sock, &drawn).await {
            *SPLASH.lock().unwrap() = None;
            reload_viewer(&app);
            return;
        }
    }
}

/// One beat of the watch over the page `drawn`: whether the window reloads now.
pub(crate) async fn splash_beat(app: &tauri::AppHandle<Rt>, sock: &Path, drawn: &str) -> bool {
    let answering = UnixStream::connect(sock).await.is_ok();
    heal_failed(app).await;
    let now = splash_key(app, splash_conflict(app).await.as_ref());
    splash_reloads(drawn, answering, &now)
}

/// The starting page the window is served at `/` while the socket is silent, and its key.
pub(crate) async fn starting_page(app: &tauri::AppHandle<Rt>) -> (String, String) {
    let since = START_AT.lock().unwrap().map(|t| t.elapsed());
    let page = start_page(&mut app.state::<DaemonGuard>().0.lock().unwrap(), since);
    let conflict = splash_conflict(app).await;
    let html = starting_html(&page, conflict.as_ref(), &settings_url(), &daemon_log());
    (html, splash_key(app, conflict.as_ref()))
}

/// What the starting page would say now, as the key `SPLASH` keeps.
pub(crate) fn splash_key(app: &tauri::AppHandle<Rt>, conflict: Option<&Conflict>) -> String {
    let since = START_AT.lock().unwrap().map(|t| t.elapsed());
    let page = start_page(&mut app.state::<DaemonGuard>().0.lock().unwrap(), since);
    format!("{page:?}|{conflict:?}")
}

/// The store the daemon cannot open, for the starting page (docs/settings-page.md Section 3.3): the
/// model it was indexed by and the one the daemon runs, from `status --json`. Read at most every five
/// seconds, since the page asks again every second.
pub(crate) async fn splash_conflict(app: &tauri::AppHandle<Rt>) -> Option<Conflict> {
    static LAST: Mutex<Option<(std::time::Instant, Option<Conflict>)>> = Mutex::new(None);
    if let Some((at, c)) = LAST.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(5) {
            return c;
        }
    }
    let st = on_cli(app, cli_status).await.ok().flatten();
    let c = st.as_ref().and_then(conflict_of);
    *LAST.lock().unwrap() = Some((std::time::Instant::now(), c.clone()));
    c
}

/// The model a store was indexed by and the one the daemon runs.
type Conflict = (String, String);

/// `status --json`'s `embedder_conflict` as its two models, or `None`.
pub(crate) fn conflict_of(st: &serde_json::Value) -> Option<Conflict> {
    let e = &st["embedder_conflict"];
    Some((e["recorded"].as_str()?.to_string(), e["daemon"].as_str()?.to_string()))
}
