//! The Settings page (docs/settings-page.md): the state it shows, gathered in one pass, and the
//! commands it calls.

use super::*;

// --- The settings page (docs/settings-page.md) ---------------------------------------------

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
pub(crate) async fn settings_state(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    own_page_caller(&webview)?;
    Ok(settings_state_of(&app).await)
}

#[tauri::command]
pub(crate) async fn login_set(
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
pub(crate) fn query_value(v: &str) -> String {
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
pub(crate) async fn peer_narrow(
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
pub(crate) async fn daemon_restart(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    restart_daemon(&app, sock).await
}

#[tauri::command]
pub(crate) async fn search_rebuild(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<String, String> {
    own_page_caller(&webview)?;
    let sock = app.state::<VizSock>().0.clone();
    rebuild_index(&app, sock).await
}

/// "Copy log" and "Copy diagnostics": the page hands the text to the system clipboard.
#[tauri::command]
pub(crate) async fn clipboard_copy(webview: tauri::Webview, text: String) -> Result<(), String> {
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
pub(crate) async fn diagnostics(
    webview: tauri::Webview,
    app: tauri::AppHandle,
) -> Result<String, String> {
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
pub(crate) fn without_entry(text: &str) -> String {
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
pub(crate) async fn reveal(webview: tauri::Webview, what: String) -> Result<(), String> {
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
