//! The daemon's lifecycle: attach to a running daemon or spawn one, and restart it, rebuild its
//! index or change Start at Login, through the CLI where something else manages it.

use super::*;

/// The shell's relationship to the daemon. Spawned = ours to reap and restart; External =
/// attached to an externally managed daemon (launchd / `supragnosis start` / another shell) -
/// never killed by us (its MCP clients outlive this app).
pub(crate) enum Daemon {
    Starting,
    External,
    Spawned(Child),
    Failed(String),
}

/// What quitting the app stops (daemon-lifecycle.md L6): only a daemon this app spawned, which lives
/// as long as the app does. One it attached to - a login job's, Homebrew's, a `supragnosis start` -
/// keeps running, and nothing on the way out runs the CLI to stop it.
pub(crate) fn reaped_on_exit(daemon: Daemon) -> Option<Child> {
    match daemon {
        Daemon::Spawned(child) => Some(child),
        Daemon::Starting | Daemon::External | Daemon::Failed(_) => None,
    }
}

/// The app's exit: the window's frame is kept, and the child [`reaped_on_exit`] names is stopped.
pub(crate) fn on_exit(app: &tauri::AppHandle<Rt>) {
    keep_frame(app);
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    if let Some(mut child) = reaped_on_exit(prev) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Daemon {
    /// What the shell knows of the daemon by itself, in the state words every surface uses
    /// (docs/design-system.md Section 9, step 45).
    pub(crate) fn status_line(&self) -> String {
        match self {
            Daemon::Starting => "Starting...".to_string(),
            Daemon::External => "Running - started outside this app".to_string(),
            Daemon::Spawned(_) => "Running while this app is open".to_string(),
            Daemon::Failed(e) => format!("Could not start: {}", cause_of(e)),
        }
    }
}

pub(crate) struct DaemonGuard(pub(crate) Mutex<Daemon>);

pub(crate) fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

/// Viewer socket path - the same resolution the CLI uses (SUPRAGNOSIS_VIZ_SOCK -> default), so the
/// shell and a `supragnosis start` daemon land on the same socket without configuration.
pub(crate) fn viz_sock() -> PathBuf {
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
pub(crate) fn find_server_bin() -> Option<PathBuf> {
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

/// Spawns `supragnosis serve` as this app's own daemon (logs to ~/.supragnosis/log/app-daemon.*.log),
/// for [`FoundCli::serve`].
pub(crate) fn spawn_daemon(sock: &Path) -> anyhow::Result<(Child, PathBuf)> {
    let bin = find_server_bin().context(
        "supragnosis server binary not found - set SUPRAGNOSIS_BIN, or install it to ~/.local/bin \
         (scripts/install.sh)",
    )?;
    let base = home().join(".supragnosis");
    // Created closed to other accounts when this is the first thing to create it - the store and the
    // token will live here. An existing directory is the daemon's to correct, which it does on start.
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
    let child = Command::new(&bin)
        .args(["serve", "--http", &http_addr, "--viz"])
        .arg(sock)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .with_context(|| format!("failed to spawn {}", bin.display()))?;
    Ok((child, bin))
}

/// Attach-or-spawn. A live socket means an externally managed daemon - attach and own nothing.
/// Otherwise spawn `supragnosis serve` and wait briefly for its socket; the viz:// proxy serves a
/// retry page until it answers, so a slow start is not fatal.
async fn ensure_daemon(cli: &dyn Cli, sock: &Path) -> anyhow::Result<Option<Child>> {
    if UnixStream::connect(sock).await.is_ok() {
        tracing::info!(sock = %sock.display(), "attached to a running daemon");
        return Ok(None);
    }
    let (mut child, bin) = cli.serve(sock)?;
    let logs = home().join(".supragnosis/log");
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

/// A child the app spawned that has since exited is a failed start - Section 9's "Could not start" -
/// for whatever reads the state next: the tray, Settings and the starting page say the same.
pub(crate) fn settle_exited(d: &mut Daemon) {
    let exited = match d {
        Daemon::Spawned(c) => match c.try_wait() {
            Ok(Some(status)) => Some(format!("the daemon exited ({status})")),
            Ok(None) => None,
            Err(e) => Some(format!("the daemon could not be watched: {e}")),
        },
        _ => None,
    };
    if let Some(why) = exited {
        *d = Daemon::Failed(why);
    }
}

/// A failure's cause in one sentence: the shell's reason up to " - full log:", its first line,
/// begun with a capital and ended with a stop.
pub(crate) fn cause_of(why: &str) -> String {
    let cut = why.split(" - full log:").next().unwrap_or(why);
    let line = cut.lines().next().unwrap_or("").trim().trim_end_matches('.');
    let mut c = line.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
        None => "The daemon did not start.".to_string(),
    }
}

/// The daemon's error log: the one written last of the app's (`app-daemon.err.log`, when the app
/// spawns the daemon) and a login job's (`supragnosis.err.log`).
pub(crate) fn daemon_log() -> PathBuf {
    let dir = home().join(".supragnosis/log");
    let (app, job) = (dir.join("app-daemon.err.log"), dir.join("supragnosis.err.log"));
    let when = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (when(&app), when(&job)) {
        (Some(a), Some(j)) if j > a => job,
        (None, Some(_)) => job,
        _ => app,
    }
}

/// The path as a person types it: under the home directory, with `~`.
pub(crate) fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// Runs attach-or-spawn and publishes the outcome (state + tray status line).
pub(crate) async fn bring_up(app: tauri::AppHandle<Rt>, sock: PathBuf) {
    mark_start();
    refresh_status(&app).await;
    let cli = app.state::<CliHeld>().0.clone();
    let state = match ensure_daemon(&*cli, &sock).await {
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

/// The app's start: attach-or-spawn, then, on a launch after the first, the Welcome page if
/// setup says a step waits (docs/onboarding.md Section 6).
pub(crate) async fn start_up(app: tauri::AppHandle<Rt>, sock: PathBuf, first: bool) {
    bring_up(app.clone(), sock).await;
    if !first {
        welcome_at_start(app).await;
    }
}

/// How long a restart, and a rebuild, wait for a login job's daemon to answer before deciding
/// (docs/daemon-lifecycle.md Section 6).
pub(crate) const RESTART_WAIT: Duration = Duration::from_secs(30);

/// Restart: bounce whatever we manage, then attach-or-spawn again. A spawned child is killed
/// directly; an external daemon is bounced through the CLI (`supragnosis restart` knows every
/// manager the product installs). A refused restart (a conflict, an unrecognized holder) says why,
/// instead of the shell quietly re-attaching to the process it failed to replace (L4).
pub(crate) async fn restart_daemon(
    app: &tauri::AppHandle<Rt>,
    sock: PathBuf,
) -> Result<String, String> {
    // A failed daemon that something manages is restarted through that manager, as an attached one
    // is: the shell spawns none of its own beside it (docs/daemon-lifecycle.md Section 6,
    // design-system.md Section 29.5).
    let failed = matches!(*app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Failed(_));
    let job = failed && on_cli(app, cli_status).await.ok().flatten().as_ref().is_some_and(managed);
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    mark_start();
    refresh_status(app).await;
    let outcome = match restart_by(&prev, job) {
        RestartBy::Kill => {
            if let Daemon::Spawned(mut child) = prev {
                let _ = child.kill();
                let _ = child.wait();
            }
            Ok("restarted the daemon this app runs".to_string())
        }
        RestartBy::Cli => {
            let before = on_cli(app, cli_status).await.ok().flatten();
            let out = on_cli(app, |c| c.run(&["restart"])).await.ok().flatten();
            // launchd may still be starting the new process when the CLI returns, or retrying one
            // that lost the store lock to the old process while it exited, and the CLI then reports
            // a failure that a few seconds settle. Wait for the socket, as set_login does, so the
            // shell attaches to that daemon rather than spawning a second one into its lock.
            let up = wait_for_socket(&sock, RESTART_WAIT).await;
            let after = if up { on_cli(app, cli_status).await.ok().flatten() } else { None };
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
        RestartBy::BringUp => Ok("starting the daemon".to_string()),
    };
    bring_up(app.clone(), sock).await;
    outcome
}

/// How a restart reaches the daemon (docs/daemon-lifecycle.md Section 6, design-system.md Section
/// 29.5): a child the app spawned is stopped by the app; one it attached to goes through `supragnosis
/// restart`, and so does a failed one a manager owns (`job`), so the app spawns none beside it; a
/// daemon still starting, or a failed one nothing manages, is brought up again.
#[derive(Debug, PartialEq)]
pub(crate) enum RestartBy {
    Kill,
    Cli,
    BringUp,
}

pub(crate) fn restart_by(daemon: &Daemon, job: bool) -> RestartBy {
    match daemon {
        Daemon::Spawned(_) => RestartBy::Kill,
        Daemon::External => RestartBy::Cli,
        Daemon::Failed(_) if job => RestartBy::Cli,
        Daemon::Starting | Daemon::Failed(_) => RestartBy::BringUp,
    }
}

/// What a restart of a daemon the shell did not spawn came to (docs/daemon-lifecycle.md L4): the
/// CLI's outcome, the running process before and after, and whether the viewer socket answered
/// within the wait. A refusal while a new process took over - launchd retrying a start that lost the
/// store lock for a moment - is a restart; a refusal that left the same process is not.
pub(crate) fn restart_outcome(
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

/// Whether `status --json` names any manager of the daemon - a login job, a systemd unit, a
/// `supragnosis start` pidfile.
pub(crate) fn managed(st: &serde_json::Value) -> bool {
    st["managers"].as_array().is_some_and(|ms| !ms.is_empty())
}

/// The pid of the manager `status --json` names first, when it has a process.
pub(crate) fn manager_pid(st: Option<&serde_json::Value>) -> Option<u64> {
    st?["managers"].get(0)?["pid"].as_u64()
}

/// A Failed state outlives its cause: a spawn that lost the store lock to a login job still
/// starting, or a restart reported failed while launchd brought the daemon back. Whenever the state
/// is read, a Failed daemon whose socket answers is attached to, so the tray line and the Settings
/// page say what is true instead of the failure that has passed.
pub(crate) async fn heal_failed(app: &tauri::AppHandle<Rt>) {
    let (Some(guard), Some(sock)) = (app.try_state::<DaemonGuard>(), app.try_state::<VizSock>())
    else {
        return;
    };
    if !matches!(*guard.0.lock().unwrap(), Daemon::Failed(_)) {
        return;
    }
    let sock = sock.0.clone();
    let answering = UnixStream::connect(&sock).await.is_ok();
    let mut guard = guard.0.lock().unwrap();
    if heals(&guard, answering) {
        tracing::info!(sock = %sock.display(), "the daemon answers again - attached");
        *guard = Daemon::External;
    }
}

/// Whether a read of the state attaches to the daemon again: a failed one whose socket answers.
pub(crate) fn heals(daemon: &Daemon, answering: bool) -> bool {
    answering && matches!(daemon, Daemon::Failed(_))
}

/// Waits for the viewer socket to answer, so a daemon launchd is still starting is attached to
/// rather than raced: attach-or-spawn would otherwise spawn a second daemon into the store lock.
pub(crate) async fn wait_for_socket(sock: &Path, limit: Duration) -> bool {
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
pub(crate) async fn set_login(
    app: &tauri::AppHandle<Rt>,
    sock: PathBuf,
    on: bool,
) -> Result<String, String> {
    let installed = on_cli(app, cli_status)
        .await
        .ok()
        .flatten()
        .and_then(|v| v["service"]["state"].as_str().map(|s| s != "absent"))
        .unwrap_or(false);
    if installed == on {
        return Ok(if on { "already starts at login" } else { "already off" }.to_string());
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
    let out = on_cli(app, move |c| c.run(args)).await.ok().flatten();
    let outcome = login_outcome(installed, out.as_ref());
    if !installed {
        wait_for_socket(&sock, Duration::from_secs(10)).await;
    }
    bring_up(app.clone(), sock).await;
    outcome
}

/// What a Start at Login change came to, from the CLI's exit status (daemon-lifecycle.md L4).
pub(crate) fn login_outcome(
    installed: bool,
    out: Option<&std::process::Output>,
) -> Result<String, String> {
    cli_change(out, |_| {
        if installed { "no longer starts at login" } else { "starts at login" }.to_string()
    })
}

/// The socket path, for the commands that bring the daemon back up.
pub(crate) struct VizSock(pub(crate) PathBuf);

/// Rebuild index (docs/settings-page.md Section 3.3, semantic-retrieval.md Section 13): the daemon
/// this app spawned is stopped here, since only the app can stop it; any other is the CLI's, which
/// `supragnosis embed rebuild` stops and starts again. The store is reset either way, and the shell
/// then attaches to the daemon that comes back, or spawns its own where none runs - and that start's
/// backfill embeds every row with the model it runs.
pub(crate) async fn rebuild_index(
    app: &tauri::AppHandle<Rt>,
    sock: PathBuf,
) -> Result<String, String> {
    let prev =
        std::mem::replace(&mut *app.state::<DaemonGuard>().0.lock().unwrap(), Daemon::Starting);
    refresh_status(app).await;
    if let Daemon::Spawned(mut child) = prev {
        let _ = child.kill();
        let _ = child.wait();
    }
    let out = on_cli(app, |c| c.run(&["embed", "rebuild"])).await.ok().flatten();
    let outcome = match out.as_ref().map(cli_outcome) {
        Some((true, line)) => Ok(line),
        Some((false, why)) => Err(why),
        None => Err("the supragnosis CLI was not found".to_string()),
    };
    // A login job the CLI started again: wait for its socket, as a restart does, so the shell
    // attaches to it rather than spawning a second daemon into its lock.
    let job = on_cli(app, cli_status)
        .await
        .ok()
        .flatten()
        .is_some_and(|st| st["service"]["state"].as_str().is_some_and(|s| s != "absent"));
    if outcome.is_ok() && job {
        wait_for_socket(&sock, RESTART_WAIT).await;
    }
    bring_up(app.clone(), sock).await;
    outcome
}
