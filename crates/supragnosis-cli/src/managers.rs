//! The daemon's pidfile and the managers that may run it: looking at them (launchctl,
//! systemctl, the pidfile, the sockets) and acting on them.

use super::*;

// --- Background daemon lifecycle (start/stop/restart/status) -------------------------
// Self-managed via a pidfile + logs. Uses only kill (-0/SIGTERM)/TcpStream, so no unsafe/libc is needed.

pub(crate) fn base_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".supragnosis")
}
pub(crate) fn pid_path() -> std::path::PathBuf {
    base_dir().join("supragnosis.pid")
}
pub(crate) fn log_dir() -> std::path::PathBuf {
    base_dir().join("log")
}
pub(crate) fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_path()).ok().and_then(|s| s.trim().parse().ok())
}
/// Checks whether the process is alive via `kill -0` (portable, without unsafe/libc).
pub(crate) fn pid_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
/// Whether a listener is at the address (a successful connection attempt = in use).
pub(crate) fn port_open(addr: &str) -> bool {
    use std::net::ToSocketAddrs;
    addr.to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|sa| {
            std::net::TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(300)).is_ok()
        })
        .unwrap_or(false)
}

// --- Observing and acting on the daemon's managers (docs/daemon-lifecycle.md) -----------------
// The decisions are pure and live in `lifecycle.rs`; what is here only looks (launchctl, the pidfile,
// the sockets) and acts (kickstart, bootout, a signal).

/// User id for the `gui/<uid>` launchd domain target (via `id -u` - no libc/unsafe).
#[cfg(target_os = "macos")]
pub(crate) fn launchd_uid() -> Option<String> {
    let out = std::process::Command::new("id").arg("-u").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let uid = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!uid.is_empty()).then_some(uid)
}

/// Every loaded launchd job under a label the product has ever installed (L2). A job that is
/// loaded but has no process is included - a KeepAlive job failing on start is still a manager.
#[cfg(target_os = "macos")]
pub(crate) fn launchd_jobs() -> Vec<lifecycle::Job> {
    lifecycle::KNOWN_LABELS
        .iter()
        .filter_map(|&(label, kind)| {
            let out =
                std::process::Command::new("launchctl").arg("list").arg(label).output().ok()?;
            if !out.status.success() {
                return None;
            }
            let (pid, last_exit) =
                lifecycle::parse_launchctl_list(&String::from_utf8_lossy(&out.stdout));
            Some(lifecycle::Job { label, kind, pid, last_exit })
        })
        .collect()
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_jobs() -> Vec<lifecycle::Job> {
    Vec::new()
}

/// Every systemd user unit under a name the product runs the daemon as (L2) that manages it now:
/// enabled, or active (daemon-lifecycle.md Section 4, "On Linux"). Empty where there is no user
/// manager to ask, and everywhere but Linux.
pub(crate) fn systemd_units() -> Vec<lifecycle::Job> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    lifecycle::KNOWN_UNITS
        .iter()
        .filter_map(|&(label, kind)| {
            let st = unit_show(label)?;
            st.manages().then_some(lifecycle::Job {
                label,
                kind,
                pid: st.pid,
                last_exit: st.last_exit,
            })
        })
        .collect()
}

/// What the person's systemd manager says about one unit; `None` when it cannot be asked.
pub(crate) fn unit_show(unit: &str) -> Option<lifecycle::UnitShow> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", unit])
        .args(["-p", "LoadState", "-p", "ActiveState", "-p", "UnitFileState", "-p", "MainPID"])
        .args(["-p", "ExecMainStatus", "-p", "ExecMainExitTimestampMonotonic"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| lifecycle::parse_systemctl_show(&String::from_utf8_lossy(&out.stdout)))
}

/// Whether this session can reach a systemd user manager, and why not when it cannot - no systemctl,
/// or no user bus, as in many containers.
pub(crate) fn systemd_user_available() -> std::result::Result<(), String> {
    match std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    {
        Err(e) => Err(format!("systemctl was not found ({e})")),
        Ok(o) if !o.status.success() => {
            let why = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Err(if why.is_empty() { "systemctl --user did not answer".to_string() } else { why })
        }
        Ok(_) => Ok(()),
    }
}

/// The user name, for lingering and a unit's `%u`.
pub(crate) fn current_user() -> Option<String> {
    let out = std::process::Command::new("id").arg("-un").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// Whether the person's systemd manager outlives their sessions: logind keeps one file per lingering
/// user. `None` when the user name is not known.
pub(crate) fn linger_state() -> Option<bool> {
    let user = current_user()?;
    Some(std::path::Path::new("/var/lib/systemd/linger").join(user).exists())
}

/// Runs `systemctl --user` with these arguments; a failure carries systemctl's own words (L4).
pub(crate) fn systemctl_user(args: &[&str]) -> Result<()> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("failed to run systemctl")?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// What the lifecycle commands decide from. Reads only - a stale pidfile is cleared by the commands
/// that act, never by `status`.
pub(crate) fn observe() -> lifecycle::Observed {
    observe_with(daemon_store_path(|k| std::env::var(k).ok()).as_deref())
}

/// What the lifecycle commands observe, with the store probed at `store` - the path the daemon in
/// question would open, which for `service install` comes from the job's environment rather than
/// this process's.
pub(crate) fn observe_with(store: Option<&std::path::Path>) -> lifecycle::Observed {
    lifecycle::Observed {
        pidfile: live_pidfile().and_then(|p| u32::try_from(p).ok()),
        jobs: launchd_jobs(),
        units: systemd_units(),
        answering: port_open(&status_http_addr()),
        store_held: store.is_some_and(supragnosis_store::redb_in_use),
    }
}

/// The redb file a daemon running with this environment opens, resolved the way `serve` resolves
/// it. `None` for the in-memory store, which nothing else can hold.
pub(crate) fn daemon_store_path(
    env: impl Fn(&str) -> Option<String>,
) -> Option<std::path::PathBuf> {
    let get = |k: &str| env(k).filter(|v| !v.trim().is_empty());
    let kind = get("SUPRAGNOSIS_STORE").unwrap_or_else(|| "redb".to_string());
    if matches!(kind.as_str(), "mem" | "memory") {
        return None;
    }
    let dir = get("SUPRAGNOSIS_DATA_DIR").unwrap_or_else(|| default_data_dir_for(&kind));
    Some(redb_path(&dir))
}

/// The pidfile's process, only when it is alive AND is supragnosis (L9). A pid outlives the process
/// it named, and acting on a reused one would signal an unrelated program.
pub(crate) fn pidfile_now() -> lifecycle::Pidfile {
    let text = std::fs::read_to_string(pid_path()).ok();
    lifecycle::pidfile_state(text.as_deref(), pid_alive, pid_is_supragnosis)
}

pub(crate) fn live_pidfile() -> Option<i32> {
    match pidfile_now() {
        lifecycle::Pidfile::Live(pid) => Some(pid),
        _ => None,
    }
}

pub(crate) fn pid_is_supragnosis(pid: i32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|o| {
            o.status.success()
                && lifecycle::is_supragnosis_command(&String::from_utf8_lossy(&o.stdout))
        })
        .unwrap_or(false)
}

pub(crate) fn clear_stale_pidfile() {
    if let lifecycle::Pidfile::Stale(_) = pidfile_now() {
        let _ = std::fs::remove_file(pid_path());
    }
}

/// The running daemon's version, from the viewer's `/api/about` over its unix socket. `None` when
/// the socket does not answer - and then the version is unknown, never assumed (Section 5).
pub(crate) fn running_version() -> Option<String> {
    lifecycle::parse_about_version(&viz_get("/api/about")?)
}

/// First contact as the running daemon reports it, the `clients` list of `/api/clients`. `None`
/// when the socket does not answer or the daemon does not record contacts (one older than this).
pub(crate) fn first_contacts() -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&viz_get("/api/clients")?)
        .ok()?
        .get("clients")
        .filter(|c| c.is_array())
        .cloned()
}

/// The running daemon's store health - the owed-projection ledger and the last recovery
/// (crash-recovery.md K5). `None` when the viewer socket does not answer, or answers without the
/// route (a daemon older than this binary): both are unknown, not healthy (Principle 5).
pub(crate) fn store_health() -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&viz_get("/api/health")?)
        .ok()
        .filter(|v| v.get("owed_projections").is_some())
}

/// One GET over the viewer's unix socket, returning the body. Short timeouts: `status` must answer
/// even when the daemon is wedged.
pub(crate) fn viz_get(path: &str) -> Option<String> {
    viz_get_within(path, 800)
}

/// [`viz_get`] with its own timeout, for a read that walks the log.
pub(crate) fn viz_get_within(path: &str, millis: u64) -> Option<String> {
    use std::io::{Read, Write};
    let sock = std::env::var("SUPRAGNOSIS_VIZ_SOCK")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_viz_sock);
    let mut s = std::os::unix::net::UnixStream::connect(sock).ok()?;
    let limit = Some(std::time::Duration::from_millis(millis));
    s.set_read_timeout(limit).ok()?;
    s.set_write_timeout(limit).ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nConnection: close\r\n\r\n").as_bytes())
        .ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let (_, body) = raw.split_once("\r\n\r\n")?;
    Some(body.to_string())
}

/// Restart a launchd job in place (`kickstart -k`), whichever known label it runs under. The plist's
/// environment is re-applied, so the viewer comes back with the server.
#[cfg(target_os = "macos")]
pub(crate) fn launchd_kickstart(job: &lifecycle::Job) -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{}", job.label);
    let st = std::process::Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl kickstart")?;
    if !st.success() {
        anyhow::bail!("launchctl kickstart {target} failed");
    }
    println!("restarted launchd job {} (MCP server + viewer).", job.label);
    Ok(())
}

/// Stop a launchd job (`bootout`), so KeepAlive does not respawn it. A Homebrew job's plist stays
/// where Homebrew put it and loads again at the next login - said here, because "stopped" alone
/// would be true only until then.
#[cfg(target_os = "macos")]
pub(crate) fn launchd_bootout(job: &lifecycle::Job) -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{}", job.label);
    let st = std::process::Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl bootout")?;
    if !st.success() {
        anyhow::bail!("launchctl bootout {target} failed (already stopped?).");
    }
    match job.kind {
        lifecycle::LabelKind::Homebrew(token) => println!(
            "stopped launchd job {}. It is Homebrew's and loads again at the next login - retire it with `brew services stop {token}`.",
            job.label
        ),
        _ => println!(
            "stopped launchd job {}. It stays down until `supragnosis restart` (or the next login) loads it again.",
            job.label
        ),
    }
    Ok(())
}

/// Load the installed canonical job (`bootstrap`) - `restart` after a `stop`.
#[cfg(target_os = "macos")]
pub(crate) fn launchd_bootstrap_canonical() -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let st = std::process::Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{uid}"))
        .arg(&plist)
        .status()
        .context("failed to run launchctl bootstrap")?;
    if !st.success() {
        anyhow::bail!("launchctl bootstrap gui/{uid} {} failed", plist.display());
    }
    println!("loaded launchd job {} (MCP server + viewer).", lifecycle::CANONICAL_LABEL);
    await_daemon(lifecycle::CANONICAL_LABEL)
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_bootstrap_canonical() -> Result<()> {
    anyhow::bail!("a LaunchAgent plist was found on a system without launchd")
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_kickstart(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_bootout(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}

/// Restart a systemd user unit in place. Its program is the path an upgrade repoints, so the new
/// binary is what starts (daemon-lifecycle.md Section 4, "On Linux").
pub(crate) fn systemd_restart(job: &lifecycle::Job) -> Result<()> {
    systemctl_user(&["restart", job.label])?;
    println!("restarted systemd unit {} (MCP server + viewer).", job.label);
    await_daemon(job.label)
}

/// Stop a systemd user unit. It stays enabled, so it starts again at the next login - said here,
/// because "stopped" alone would be true only until then.
pub(crate) fn systemd_stop(job: &lifecycle::Job) -> Result<()> {
    systemctl_user(&["stop", job.label])?;
    match job.kind {
        lifecycle::LabelKind::Homebrew(token) => println!(
            "stopped systemd unit {}. It is Homebrew's and starts again at the next login - retire it with `brew services stop {token}`.",
            job.label
        ),
        _ => println!(
            "stopped systemd unit {}. While it is enabled it starts again at the next login - `supragnosis restart` starts it now, `supragnosis service uninstall` keeps it off.",
            job.label
        ),
    }
    Ok(())
}

/// Start the installed canonical unit - `restart` when it is installed but neither enabled nor active.
pub(crate) fn systemd_start_canonical() -> Result<()> {
    systemctl_user(&["start", lifecycle::CANONICAL_UNIT])?;
    println!("started systemd unit {} (MCP server + viewer).", lifecycle::CANONICAL_UNIT);
    await_daemon(lifecycle::CANONICAL_UNIT)
}
