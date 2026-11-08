//! The AI apps pane: the apps `supragnosis connect` knows, each one's row on the settings page,
//! connecting and disconnecting through the CLI, and the helpers that restart or open an app.

use super::*;

/// The apps `supragnosis connect` knows, by id and display name. Listed here only to build the menu
/// before the CLI has answered; what each one's state is comes from `connect --json`, and an id the
/// CLI does not report is shown as needing a newer CLI.
pub(crate) const AI_APPS: &[(&str, &str)] = &[
    ("claude-desktop", "Claude Desktop"),
    ("claude-code", "Claude Code"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("codex", "Codex"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
];

/// `supragnosis connect --json`. `None` when the CLI predates it.
pub(crate) fn cli_connect_list(cli: &dyn Cli) -> Option<serde_json::Value> {
    run_cli_json(cli, &["connect", "--json"])
}

/// One AI app's row on the settings page, from the CLI's report on it: what state it is in, its one
/// button - `None` where a click could do nothing (not installed, settings unreadable, a CLI too old
/// to know `connect`) - and the kind of state, which picks the row's pill: `connected`, `attention`
/// (connected some other way, which a click replaces), `off` or `absent`. The state and the button
/// are ids from closed sets, which the page says in its words (docs/i18n.md Section 5); any entry
/// other than the bridge has its own, because the person clicking is about to replace it.
pub(crate) fn app_row(
    report: Option<&serde_json::Value>,
) -> (&'static str, Option<&'static str>, &'static str) {
    let Some(r) = report else {
        return ("newer", None, "absent");
    };
    if !r["installed"].as_bool().unwrap_or(false) {
        return ("absent", None, "absent");
    }
    match r["entry"].as_str().unwrap_or("none") {
        "bridge" => ("bridge", Some("disconnect"), "connected"),
        "http" => ("http", Some("switch"), "attention"),
        "stdio" => ("stdio", Some("switch"), "attention"),
        "other" => ("other", Some("switch"), "attention"),
        "unknown" => ("unread", None, "absent"),
        _ => ("none", Some("connect"), "off"),
    }
}

/// Claude Code's capture hook on its row: on or off while it is connected through the bridge and
/// the listing says (client-connect.md 12.4); `None` otherwise, and the page then draws no switch.
pub(crate) fn capture_of(report: Option<&serde_json::Value>) -> Option<bool> {
    report
        .filter(|r| r["entry"].as_str() == Some("bridge"))
        .and_then(|r| r["capture"].as_str().filter(|c| *c == "on" || *c == "off"))
        .map(|c| c == "on")
}

/// The CLI call the capture switch makes: `connect claude-code`, which adds the hook beside the
/// bridge, and with `--no-capture` to remove the hook and keep the bridge (client-connect.md 12.4).
pub(crate) fn capture_args(on: bool) -> Vec<&'static str> {
    let mut args = vec!["connect", "claude-code"];
    if !on {
        args.push("--no-capture");
    }
    args
}

/// Re-reads which AI apps are connected, for the status line's hint while none is.
pub(crate) async fn refresh_apps(app: &tauri::AppHandle<Rt>) {
    let list = on_cli(app, cli_connect_list).await.ok().flatten();
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
async fn toggle_app(app: &tauri::AppHandle<Rt>, id: &str) -> Result<String, String> {
    let name = AI_APPS
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .unwrap_or(id)
        .to_string();
    let wanted = id.to_string();
    let connected = on_cli(app, cli_connect_list)
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
    let out = on_cli(app, move |c| {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        c.run(&args)
    })
    .await
    .ok()
    .flatten();
    let message = match out {
        None => Err(word_with("app.noCommand", &[("app", &name)])),
        Some(o) if o.status.success() && connected => {
            Ok(word_with("app.didDisconnect", &[("app", &name)]))
        }
        Some(o) if o.status.success() => {
            // The CLI's "next" line is the one thing the person still has to do, on a line of its
            // own, which Settings keeps under the app's name.
            let next = String::from_utf8_lossy(&o.stdout)
                .lines()
                .find_map(|l| l.trim().strip_prefix("next").map(|n| n.trim().to_string()));
            let done = word_with("app.didConnect", &[("app", &name)]);
            Ok(match next {
                Some(n) => format!("{done}\n{n}"),
                None => done,
            })
        }
        Some(o) => {
            Err(word_with("app.connectFailed", &[("app", &name), ("why", &cli_outcome(&o).1)]))
        }
    };
    refresh_status(app).await;
    message
}

#[tauri::command]
pub(crate) async fn app_toggle(
    webview: tauri::Webview<Rt>,
    app: tauri::AppHandle<Rt>,
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
pub(crate) async fn capture_set(
    webview: tauri::Webview<Rt>,
    app: tauri::AppHandle<Rt>,
    on: bool,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    let args = capture_args(on);
    let out = on_cli(&app, move |c| c.run(&args)).await.ok().flatten();
    refresh_status(&app).await;
    match out {
        None => Err(word("app.cliMissing")),
        Some(o) if o.status.success() => {
            Ok(word(if on { "app.captureOn" } else { "app.captureOff" }))
        }
        Some(o) => Err(cli_outcome(&o).1),
    }
}

/// The two acts an app page takes on another app (Section 4.2, D9): Claude Desktop is asked to quit
/// as the Dock's Quit asks, waited for, and opened again; Cursor is opened. A refusal - macOS asks the
/// person once whether this app may control Claude - leaves the instruction to do it by hand.
#[tauri::command]
pub(crate) async fn app_helper(webview: tauri::Webview<Rt>, id: String) -> Result<String, String> {
    own_page_caller(&webview)?;
    tokio::task::spawn_blocking(move || match id.as_str() {
        "claude-desktop" => restart_claude_desktop(),
        "cursor" => open_app(&["-a", "Cursor"]).map(|_| word("app.cursorOpen")),
        other => Err(format!("{other:?} has no helper")),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Claude Desktop's bundle identifier, which names it to the quit request and to `open`.
const CLAUDE_DESKTOP: &str = "com.anthropic.claudefordesktop";

fn restart_claude_desktop() -> Result<String, String> {
    let by_hand = word("app.claudeByHand");
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
            let why = String::from_utf8_lossy(&quit.stderr).trim().to_string();
            return Err(word_with("app.claudeNoQuit", &[("byHand", &by_hand), ("why", &why)]));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while running() {
            if std::time::Instant::now() > deadline {
                return Err(word_with("app.claudeStill", &[("byHand", &by_hand)]));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    open_app(&["-b", CLAUDE_DESKTOP]).map_err(|e| format!("{by_hand} ({e})"))?;
    // It is open; whether it loaded supragnosis is its row's to say, once it reaches the server
    // (design-system.md Section 29.10).
    Ok(word("app.claudeOpen"))
}

fn open_app(args: &[&str]) -> Result<(), String> {
    let out = Command::new("open").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
