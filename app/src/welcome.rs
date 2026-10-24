//! The Welcome page (docs/onboarding.md Section 6): what the app remembers about it, the
//! `setup --events` runs it streams, and the commands it calls.

use super::*;

// --- The Welcome page (docs/onboarding.md Section 6) ----------------------------------------
// The page draws only what `supragnosis setup --events` says (O4). The shell runs setup for it, passes
// each line on as it arrives, and keeps three things of its own: stopping the daemon it spawned
// before the server step, the two helpers that restart or open an AI app, and what the person chose.

/// What the CLI says when it does not know `setup`: it predates this app's Welcome page.
pub(crate) const SETUP_TOO_OLD: &str =
    "This supragnosis-server predates setup. Update it: brew upgrade supragnosis-server";

/// What the app remembers about the Welcome page, and nothing else (Section 6): that it was shown,
/// that the person chose "Finish later", and that they turned Start at Login off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WelcomePrefs {
    pub(crate) seen: bool,
    pub(crate) later: bool,
    pub(crate) login_off: bool,
}

impl WelcomePrefs {
    pub(crate) fn from_json(v: &serde_json::Value) -> Self {
        let b = |k: &str| v[k].as_bool().unwrap_or(false);
        WelcomePrefs { seen: b("seen"), later: b("later"), login_off: b("login_off") }
    }
    pub(crate) fn to_json(self) -> serde_json::Value {
        serde_json::json!({ "seen": self.seen, "later": self.later, "login_off": self.login_off })
    }
}

fn prefs_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("welcome.json"))
}

/// The remembered choices. A file that is missing or unreadable is a first launch.
pub(crate) fn read_prefs(app: &tauri::AppHandle) -> WelcomePrefs {
    prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| WelcomePrefs::from_json(&v))
        .unwrap_or_default()
}

pub(crate) fn write_prefs(app: &tauri::AppHandle, p: WelcomePrefs) -> Result<(), String> {
    let path = prefs_path(app).ok_or("the app's settings directory is not known")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, p.to_json().to_string())
        .map_err(|e| format!("writing {}: {e}", path.display()))
}

/// The two steps the launch and the tray wait on (Section 6), from a checklist: whether the daemon is
/// kept running, and whether an AI app is connected. A step that does not apply here counts as done.
pub(crate) fn essentials(events: &[serde_json::Value]) -> (bool, bool) {
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
pub(crate) fn welcome_at_launch(p: WelcomePrefs, steps: Option<(bool, bool)>) -> bool {
    if !p.seen {
        return true;
    }
    !p.later && finish_setup_offered(p, steps)
}

/// Whether the tray offers "Finish setup..." (Section 6): while either step is not done, the daemon
/// step only until the person turned Start at Login off by choice.
pub(crate) fn finish_setup_offered(p: WelcomePrefs, steps: Option<(bool, bool)>) -> bool {
    steps.is_some_and(|(server, apps)| !apps || (!server && !p.login_off))
}

/// The events of one `setup --events` run: the lines after its schema line. `Err` says why there are
/// none - a CLI before `setup`, one newer than this app, or a run that failed before writing any.
pub(crate) fn setup_events(out: &std::process::Output) -> Result<Vec<serde_json::Value>, String> {
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
pub(crate) fn spawned_pid(app: &tauri::AppHandle) -> Option<u32> {
    match &*app.state::<DaemonGuard>().0.lock().unwrap() {
        Daemon::Spawned(c) => Some(c.id()),
        _ => None,
    }
}

/// `setup <args> --events`, with the app's child named when there is one.
pub(crate) fn setup_args(base: &[&str], app_daemon: Option<u32>) -> Vec<String> {
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
pub(crate) fn setup_check_blocking(
    app_daemon: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    let args = setup_args(&["--check"], app_daemon);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_cli(&args).ok_or_else(cli_missing)?;
    setup_events(&out)
}

/// What to say when there is no CLI: the formula for an app the cask installed, which depends on it,
/// and the install script otherwise (docs/onboarding.md Section 7).
pub(crate) fn cli_missing() -> String {
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
pub(crate) struct RunningSteps(pub(crate) Mutex<std::collections::HashMap<String, u32>>);

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
pub(crate) async fn setup_check(
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
pub(crate) async fn setup_run(
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
pub(crate) async fn setup_running(
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
pub(crate) async fn setup_stop(
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
pub(crate) async fn welcome_state(
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
pub(crate) async fn welcome_set(
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

/// At launch, after the daemon is up: the Welcome page opens if Section 6 says it should, and is
/// then remembered as seen.
pub(crate) async fn welcome_at_start(app: tauri::AppHandle) {
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
