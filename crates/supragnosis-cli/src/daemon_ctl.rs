//! The `start`, `stop`, `restart` and `status` subcommands.

use super::*;

/// Prints how to connect an AI app, on `start` and `status` - where a person looking for it looks.
///
/// `supragnosis connect` comes first: it registers the bridge, which reads the token from its file,
/// so no client holds a copy (docs/client-connect.md). The HTTP form follows for a client that takes
/// only a URL, with the token read by the shell rather than printed - agents run `status`, and a
/// printed token lands in their transcripts (daemon-lifecycle.md Section 11).
pub(crate) fn print_client_command(http: &str) {
    let path = mcp_token_path();
    println!("  token   {} (0600 - read from the file, never printed)", path.display());
    println!("  connect supragnosis connect claude-code   (`supragnosis connect` lists every app)");
    println!(
        "  http    http://{http}/mcp with --header \"Authorization: Bearer $(cat {})\"",
        path.display()
    );
}

/// Resolved MCP http address for status/lifecycle checks (env var or default).
pub(crate) fn status_http_addr() -> String {
    std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string())
}

pub(crate) fn start(cfg: Config) -> Result<()> {
    let http = cfg.http.clone().unwrap_or_else(|| "127.0.0.1:7373".to_string());
    // A pidfile left by a daemon that is gone would read, after the fork, as this one gone.
    clear_stale_pidfile();
    if let Some(pid) = live_pidfile() {
        anyhow::bail!("already running (pid {pid}). Run 'supragnosis stop' and try again.");
    }
    if port_open(&http) {
        anyhow::bail!(
            "{http} is already in use (another instance or a launchd daemon?). Stop it or use a different address with --http."
        );
    }
    std::fs::create_dir_all(log_dir()).with_context(|| "failed to create log directory")?;
    let out = std::fs::File::create(log_dir().join("supragnosis.out.log"))?;
    let err = std::fs::File::create(log_dir().join("supragnosis.err.log"))?;
    let viz_msg = cfg
        .viz
        .as_deref()
        .map(|v| format!("unix:{v}"))
        .unwrap_or_else(|| "(off)".to_string());
    // Generated here rather than in the child, so the connect line can be printed to the terminal
    // the operator is actually looking at. It is the same file the daemon then loads.
    if cfg.mcp_auth {
        load_or_create_mcp_token()?;
    }
    // fork/setsid/pidfile/stdio redirect. The daemonized child serves; this process waits for it
    // to answer, so `start` says started only once it is, and fails with the reason when the
    // daemon exits first (daemon-lifecycle.md L4).
    let parent = match daemonize::Daemonize::new()
        .pid_file(pid_path())
        .stdout(out)
        .stderr(err)
        .execute()
    {
        daemonize::Outcome::Child(Ok(_)) => return run_blocking(cfg),
        daemonize::Outcome::Child(Err(e)) | daemonize::Outcome::Parent(Err(e)) => {
            anyhow::bail!("daemonization failed: {e}")
        }
        daemonize::Outcome::Parent(Ok(parent)) => parent,
    };
    if parent.first_child_exit_code != 0 {
        anyhow::bail!(
            "daemonization failed: its first child exited with status {}",
            parent.first_child_exit_code
        );
    }
    match await_forked(&http) {
        lifecycle::Forked::Serving => {
            println!("supragnosis daemon started - MCP http://{http}/mcp  viewer {viz_msg}")
        }
        lifecycle::Forked::Slow => println!(
            "supragnosis daemon still starting - `supragnosis status` shows it once http://{http}/mcp answers"
        ),
        lifecycle::Forked::Gone(pid) => {
            let log = log_dir().join("supragnosis.err.log");
            let reason = std::fs::read_to_string(&log)
                .ok()
                .and_then(|t| lifecycle::logged_error(&t))
                .unwrap_or_else(|| "it logged no error".to_string());
            anyhow::bail!(
                "the daemon (pid {pid}) exited before it answered on {http}: {reason}\n  the log is {}",
                log.display()
            );
        }
    }
    println!("  pidfile {}  logs {}", pid_path().display(), log_dir().display());
    if cfg.mcp_auth {
        print_client_command(&http);
    }
    Ok(())
}

/// Watches the daemon `start` just forked until its address answers, its process is gone, or thirty
/// seconds pass (`lifecycle::forked`).
pub(crate) fn await_forked(http: &str) -> lifecycle::Forked {
    let mut looks = 0;
    loop {
        std::thread::sleep(lifecycle::START_TICK);
        looks += 1;
        if let Some(seen) = lifecycle::forked(port_open(http), &pidfile_now(), looks) {
            return seen;
        }
    }
}

/// Sends SIGTERM to the self-managed (pidfile) daemon and waits for graceful exit.
pub(crate) fn stop_pidfile(pid: i32) -> Result<()> {
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| "failed to run kill")?;
    // Wait for shutdown (up to ~10s) - a graceful exit after SIGTERM.
    for _ in 0..50 {
        if !pid_alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if pid_alive(pid) {
        anyhow::bail!("timed out waiting for stop (pid {pid}). Check manually: kill {pid}");
    }
    let _ = std::fs::remove_file(pid_path());
    println!("daemon stopped (pid {pid}).");
    Ok(())
}

pub(crate) fn stop() -> Result<()> {
    use lifecycle::Manager;
    clear_stale_pidfile();
    let situation = lifecycle::classify(&observe());
    match lifecycle::stop_or_refuse(&situation, &status_http_addr()).map_err(anyhow::Error::msg)? {
        None => {
            println!("not running.");
            Ok(())
        }
        Some(Manager::Pidfile { pid }) => stop_pidfile(pid as i32),
        Some(Manager::Launchd(job)) => launchd_bootout(&job),
        Some(Manager::Systemd(job)) => systemd_stop(&job),
    }
}

pub(crate) fn restart(cfg: Config) -> Result<()> {
    use lifecycle::{Manager, Restart};
    clear_stale_pidfile();
    let situation = lifecycle::classify(&observe());
    let installed = || canonical_service_state() != "absent";
    match lifecycle::restart_or_refuse(&situation, installed, &status_http_addr())
        .map_err(anyhow::Error::msg)?
    {
        Restart::LoadCanonical => {
            if cfg!(target_os = "linux") {
                systemd_start_canonical()
            } else {
                launchd_bootstrap_canonical()
            }
        }
        Restart::StartFresh => start(cfg),
        Restart::Bounce(Manager::Pidfile { pid }) => {
            stop_pidfile(pid as i32)?;
            std::thread::sleep(std::time::Duration::from_millis(400)); // wait for the port to release
            start(cfg)
        }
        Restart::Bounce(Manager::Launchd(job)) => {
            launchd_kickstart(&job)?;
            await_daemon(job.label)
        }
        Restart::Bounce(Manager::Systemd(job)) => systemd_restart(&job),
    }
}

/// L4: after loading or restarting a launchd job or a systemd unit, it counts as up only when the
/// daemon answers, and then says which version it serves - a restart's whole point after an upgrade
/// is the version, and "restarted" alone does not show it. It counts as failed when its manager shows
/// it failing - no process and a non-zero last exit - for three seconds running: a job crash-looping
/// on the store lock sits like that between respawns, as does a unit waiting out `RestartSec`, while
/// one being restarted shows its previous exit only for a moment. A slow start that is still running
/// is reported as slow, not failed: a daemon repaying owed projections before it binds
/// (crash-recovery.md K3) can take a while.
pub(crate) fn await_daemon(label: &str) -> Result<()> {
    let here = env!("CARGO_PKG_VERSION");
    let mut watch = lifecycle::StartWatch::default();
    loop {
        std::thread::sleep(lifecycle::START_TICK);
        let serving = running_version();
        let job = if serving.is_some() {
            None
        } else {
            launchd_jobs().into_iter().chain(systemd_units()).find(|j| j.label == label)
        };
        let look = lifecycle::StartLook {
            serving,
            pid: job.as_ref().and_then(|j| j.pid),
            last_exit: job.as_ref().and_then(|j| j.last_exit),
        };
        match watch.see(look) {
            None => {}
            Some(lifecycle::Start::Serving(v)) => {
                match lifecycle::drift(Some(&v), here) {
                    lifecycle::Drift::Differs { running, here } => println!(
                        "  now serving {running}, but this binary is {here} - the job runs a different binary than this one"
                    ),
                    _ => println!("  now serving {v}"),
                }
                return Ok(());
            }
            Some(lifecycle::Start::Failed(code)) => {
                let (manager, log) = if cfg!(target_os = "linux") {
                    ("systemd", format!("`journalctl --user -u {label}`"))
                } else {
                    ("launchd", log_dir().join("supragnosis.err.log").display().to_string())
                };
                anyhow::bail!(
                    "{label} is loaded but not running - it exited with status {code} and {manager} is retrying it. The reason is in {log}"
                );
            }
            Some(lifecycle::Start::Slow) => {
                println!("  still starting - `supragnosis status` shows the version once the daemon answers");
                return Ok(());
            }
        }
    }
}

/// Everything `status --json` reports, already gathered. [`status_document`] only shapes it, so a
/// test can build the document without a daemon (docs/compatibility.md Section 6).
pub(crate) struct StatusFacts<'a> {
    pub(crate) situation: &'a lifecycle::Situation,
    pub(crate) answering: bool,
    pub(crate) store_held: bool,
    pub(crate) http: String,
    pub(crate) here: &'a str,
    pub(crate) running: Option<String>,
    pub(crate) health: Option<serde_json::Value>,
    pub(crate) server: serde_json::Value,
    /// First contact (docs/onboarding.md Section 5): the AI apps that reached the daemon since it
    /// started; null when it does not answer, or does not record them.
    pub(crate) clients: Option<serde_json::Value>,
    /// The model the store recorded and the one the daemon would open it with, when the daemon does
    /// not answer because they differ (semantic-retrieval.md Section 13, SR8).
    pub(crate) embedder_conflict: Option<(String, String)>,
    /// The canonical LaunchAgent's plist, or the canonical user unit on Linux, and its state.
    pub(crate) plist: std::path::PathBuf,
    pub(crate) plist_state: &'static str,
    /// Whether the person's systemd manager outlives their sessions; null off Linux, or unknown.
    pub(crate) linger: Option<bool>,
}

/// One line on semantic retrieval, from the daemon's health report (semantic-retrieval.md SR3):
/// on with the model, or off with the reason and what to run. No report at all is a node with no
/// embedder - a build without the model, or `SUPRAGNOSIS_EMBED=none`.
pub(crate) fn semantic_line(report: Option<&serde_json::Value>) -> String {
    match report {
        Some(r) if r["state"].as_str() == Some("on") => {
            let model = r["model"].as_str().unwrap_or("?");
            let b = &r["backfill"];
            match b["state"].as_str() {
                Some("running") => format!(
                    "semantic on ({model}) - embedding what was written before the model: {} of {}",
                    b["embedded"].as_u64().unwrap_or(0),
                    b["found"].as_u64().unwrap_or(0)
                ),
                Some("stopped") => format!(
                    "semantic on ({model}) - embedding what was written before the model stopped at {} of {}, the rest is found by keyword until the next start: {}",
                    b["embedded"].as_u64().unwrap_or(0),
                    b["found"].as_u64().unwrap_or(0),
                    b["note"].as_str().unwrap_or("no reason given")
                ),
                _ => format!("semantic on ({model})"),
            }
        }
        Some(r) if r["state"].as_str() == Some("off") => format!(
            "semantic off - {}",
            r["note"].as_str().unwrap_or("the daemon gave no reason")
        ),
        _ => "semantic off - keyword only (no embedding model in this build, or SUPRAGNOSIS_EMBED=none)"
            .to_string(),
    }
}

/// `status --json`, which the desktop app reads for its tray line and its Settings page.
pub(crate) fn status_document(f: &StatusFacts) -> serde_json::Value {
    use lifecycle::{Manager, Situation};
    let managers: Vec<&Manager> = match f.situation {
        Situation::One(m) => vec![m],
        Situation::Conflict(ms) => ms.iter().collect(),
        Situation::Stopped | Situation::Unrecognized => vec![],
    };
    let manager_json = |m: &Manager| match m {
        Manager::Pidfile { pid } => serde_json::json!({ "type": "pidfile", "pid": pid }),
        Manager::Launchd(j) => {
            let (source, formula) = match j.kind {
                lifecycle::LabelKind::Canonical => ("canonical", None),
                lifecycle::LabelKind::Homebrew(t) => ("homebrew", Some(t)),
                lifecycle::LabelKind::Retired => ("retired", None),
            };
            serde_json::json!({
                "type": "launchd", "label": j.label, "source": source, "formula": formula,
                "pid": j.pid, "last_exit": j.last_exit,
            })
        }
        Manager::Systemd(j) => {
            let (source, formula) = match j.kind {
                lifecycle::LabelKind::Canonical => ("canonical", None),
                lifecycle::LabelKind::Homebrew(t) => ("homebrew", Some(t)),
                lifecycle::LabelKind::Retired => ("retired", None),
            };
            serde_json::json!({
                "type": "systemd", "unit": j.label, "label": j.label, "source": source,
                "formula": formula, "pid": j.pid, "last_exit": j.last_exit,
            })
        }
    };
    serde_json::json!({
        "schema": JSON_SCHEMA,
        "situation": match f.situation {
            Situation::Stopped => "stopped",
            Situation::One(_) => "one",
            Situation::Conflict(_) => "conflict",
            Situation::Unrecognized => "unrecognized",
        },
        "managers": managers.iter().map(|m| manager_json(m)).collect::<Vec<_>>(),
        "answering": f.answering,
        "store_held": f.store_held,
        "mcp": format!("http://{}/mcp", f.http),
        "version": { "here": f.here, "running": f.running },
        // The health report minus the semantic part, which has its own field below.
        "store": f.health.as_ref().map(|h| {
            let mut s = h.clone();
            if let Some(o) = s.as_object_mut() {
                o.remove("semantic");
            }
            s
        }),
        // The daemon's own report on semantic retrieval (SR3); null when it is not answering.
        "semantic": f.health.as_ref().and_then(|h| h.get("semantic")).cloned(),
        // A store the daemon cannot open for its model (SR8); null otherwise.
        "embedder_conflict": f.embedder_conflict.as_ref().map(|(recorded, daemon)| {
            serde_json::json!({ "recorded": recorded, "daemon": daemon })
        }),
        "server": f.server,
        "clients": f.clients,
        // `plist` keeps its name in schema 1 and holds the unit's path on Linux; `path` says the same
        // without the name (daemon-lifecycle.md Section 4, "On Linux").
        "service": {
            "label": canonical_service_label(),
            "plist": f.plist,
            "path": f.plist,
            "state": f.plist_state,
            "linger": f.linger,
        },
    })
}

pub(crate) fn status(json: bool) -> Result<()> {
    use lifecycle::{Drift, Situation};
    let http = status_http_addr();
    let observed = observe();
    let situation = lifecycle::classify(&observed);
    let here = env!("CARGO_PKG_VERSION");
    let running = if observed.answering { running_version() } else { None };
    let drift = lifecycle::drift(running.as_deref(), here);
    let health = if observed.answering { store_health() } else { None };
    // A daemon that does not answer may be refused its store for the model (SR8): what the store
    // recorded, read without holding it, against the model the daemon's environment names.
    let embedder_conflict = if observed.answering { None } else { daemon_embedder_conflict() };
    // Always this daemon since remote servers were removed in 0.11.0; kept so `status --json`'s
    // schema stays 1 (remote-server.md Section 13).
    let server = serde_json::json!({"name": "local", "remote": false});

    if json {
        let facts = StatusFacts {
            situation: &situation,
            answering: observed.answering,
            store_held: observed.store_held,
            http: http.clone(),
            here,
            running: running.clone(),
            health,
            server,
            clients: if observed.answering { first_contacts() } else { None },
            embedder_conflict: embedder_conflict.clone(),
            plist: canonical_service_path(),
            plist_state: canonical_service_state(),
            linger: if cfg!(target_os = "linux") { linger_state() } else { None },
        };
        println!("{}", status_document(&facts));
        return Ok(());
    }

    match &situation {
        Situation::Stopped => {
            match read_pid() {
                Some(pid) => println!("stopped (stale pidfile, pid {pid})"),
                None => println!("stopped"),
            }
            if let Some((recorded, daemon)) = &embedder_conflict {
                println!("{}", embedder_conflict_line(recorded, daemon));
            }
            // Where a person lands on a first run, or after a restart that did not bring it back
            // (docs/onboarding.md Section 7): how to run it now, how to keep it, and setup.
            println!("  start   supragnosis start            runs it until you log out");
            println!("  keep    supragnosis service install  runs it now and at every login");
            println!("  setup   supragnosis setup            the whole first run, step by step");
            return Ok(());
        }
        Situation::One(m) => {
            println!(
                "{} ({})",
                if observed.answering { "running" } else { "not responding" },
                m.describe()
            );
        }
        Situation::Conflict(ms) => {
            println!("CONFLICT: {}", lifecycle::conflict_message(ms));
        }
        Situation::Unrecognized if observed.answering => {
            println!(
                "running (unrecognized manager - no pidfile, no known launchd label or systemd unit)"
            );
        }
        Situation::Unrecognized => {
            println!(
                "store held by a process no manager runs - an MCP client's stdio server, or a `supragnosis serve` in a terminal"
            );
        }
    }
    println!(
        "  MCP     http://{http}/mcp  ({})",
        if observed.answering { "responding" } else { "not responding" }
    );
    match &drift {
        Drift::Same(v) => println!("  version {v}"),
        Drift::Differs { running, here } => println!(
            "  version RUNNING {running}, this binary {here} - the daemon still runs the old image; `supragnosis restart` loads this one"
        ),
        Drift::Unknown if observed.answering => {
            println!("  version unknown (the viewer socket did not answer)")
        }
        Drift::Unknown => {}
    }
    if let Some((recorded, daemon)) = &embedder_conflict {
        println!("{}", embedder_conflict_line(recorded, daemon));
    }
    if let Some(h) = &health {
        let owed = h["owed_projections"].as_u64().unwrap_or(0);
        if owed > 0 {
            println!(
                "  store   {owed} observation(s) not yet projected - a sync, a backfill or a re-key has not re-materialized them; the log has them, and `supragnosis restart` re-projects them"
            );
        }
        if let Some(r) = h["last_recovery"].as_object() {
            let wss: Vec<&str> = r
                .get("workspaces")
                .and_then(|w| w.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            println!(
                "  store   recovered at start: {1} observation(s) were owed a projection (an interrupted write, or this store's first open by a build that keeps the ledger), so {0} was re-projected",
                wss.join(", "),
                r.get("observations").and_then(|v| v.as_u64()).unwrap_or(0)
            );
        }
        println!("  {}", semantic_line(h.get("semantic")));
    }
    if observed.answering {
        // The connect line is reported for a RUNNING daemon whose token file exists. Read, never
        // generated: `status` must not create state, and a missing file here is the honest report
        // that this daemon is running without auth rather than an invitation to mint a token.
        let token = std::fs::read_to_string(mcp_token_path())
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        match token {
            Some(_) => print_client_command(&http),
            None => println!(
                "  auth    none - every local OS account on this host can reach the tool surface"
            ),
        }
    }
    let label = canonical_service_label();
    match canonical_service_state() {
        "generated" => println!("  login   starts at login ({label})"),
        "hand_written" if cfg!(target_os = "linux") => {
            println!("  login   starts at login ({label}, a unit not generated by service install)")
        }
        "hand_written" => println!("  login   starts at login ({label}, hand-written plist)"),
        _ => println!("  login   not installed - supragnosis service install"),
    }
    // A user manager stops with the person's last session unless it lingers (Section 4, "On Linux").
    if cfg!(target_os = "linux")
        && canonical_service_state() != "absent"
        && linger_state() == Some(false)
    {
        println!(
            "  linger  off - it stops when you log out; `loginctl enable-linger` keeps it running"
        );
    }
    if matches!(situation, Situation::One(_)) {
        println!("  control supragnosis restart | supragnosis stop");
    }
    Ok(())
}
