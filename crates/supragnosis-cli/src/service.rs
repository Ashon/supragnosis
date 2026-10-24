//! The `service install` and `service uninstall` subcommands: the canonical LaunchAgent and
//! systemd user unit.

use super::*;

// --- The canonical LaunchAgent and user unit (docs/daemon-lifecycle.md Section 4) ------------

/// The canonical manager's name: the LaunchAgent's label, or the user unit's name on Linux.
pub(crate) fn canonical_service_label() -> &'static str {
    if cfg!(target_os = "linux") {
        lifecycle::CANONICAL_UNIT
    } else {
        lifecycle::CANONICAL_LABEL
    }
}

/// Where the canonical plist lives - or on Linux the canonical user unit, under `$XDG_CONFIG_HOME`
/// when it is set, as systemd reads it.
pub(crate) fn canonical_service_path() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    if cfg!(target_os = "linux") {
        let config = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|v| v.starts_with('/'))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        return config.join("systemd/user").join(lifecycle::CANONICAL_UNIT);
    }
    home.join("Library/LaunchAgents")
        .join(format!("{}.plist", lifecycle::CANONICAL_LABEL))
}

/// absent | generated | hand_written - the state the app's Start at Login item reflects.
pub(crate) fn canonical_service_state() -> &'static str {
    match std::fs::read_to_string(canonical_service_path()) {
        Err(_) => "absent",
        Ok(text) if lifecycle::is_generated(&text) => "generated",
        Ok(_) => "hand_written",
    }
}

pub(crate) fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The EnvironmentVariables of an existing plist, read through plutil (launchd's own reader) rather
/// than a hand-rolled XML parse - a hand-written plist can be in any of the forms launchd accepts.
#[cfg(target_os = "macos")]
pub(crate) fn plist_env(
    path: &std::path::Path,
) -> Result<std::collections::BTreeMap<String, String>> {
    let out = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()
        .context("failed to run plutil")?;
    if !out.status.success() {
        anyhow::bail!(
            "plutil could not read {} - fix or remove it by hand: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(v.get("EnvironmentVariables")
        .and_then(|e| e.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

/// Move a plist the product did not write out of LaunchAgents, keeping it (L5).
#[cfg(target_os = "macos")]
pub(crate) fn move_aside(path: &std::path::Path, label: &str) -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    move_file_aside(path, lifecycle::moved_aside_path(&home, label, unix_secs()))
}

/// Move a file the product did not write to `dst`, keeping it (L5).
pub(crate) fn move_file_aside(
    path: &std::path::Path,
    dst: std::path::PathBuf,
) -> Result<std::path::PathBuf> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::rename(path, &dst)
        .with_context(|| format!("failed to move {} aside to {}", path.display(), dst.display()))?;
    Ok(dst)
}

#[cfg(target_os = "macos")]
pub(crate) fn launchctl_quiet(args: &[&str]) -> bool {
    std::process::Command::new("launchctl")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Wait for the MCP port to close and the store to be released after a job is booted out, so the
/// next owner does not race the last one for the store lock. False when either is still held after
/// ten seconds.
pub(crate) fn wait_until_released(store: Option<&std::path::Path>) -> bool {
    let mut first = true;
    lifecycle::released(std::iter::from_fn(|| {
        if !first {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        first = false;
        // The store is probed only once the address went quiet, as before.
        let answering = port_open(&status_http_addr());
        Some((answering, !answering && store.is_some_and(supragnosis_store::redb_in_use)))
    }))
}

/// A Homebrew job is retired by Homebrew: the plist or the unit is its file, and only `brew services
/// stop` removes it (a bootout or a `systemctl stop` alone would let it start again at the next login).
pub(crate) fn brew_services_stop(token: &str) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let brew = [
        "/opt/homebrew/bin/brew".to_string(),
        "/usr/local/bin/brew".to_string(),
        "/home/linuxbrew/.linuxbrew/bin/brew".to_string(),
        format!("{home}/.linuxbrew/bin/brew"),
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .find(|p| p.exists());
    let Some(brew) = brew else {
        anyhow::bail!(
            "a brew services job ({token}) owns the daemon but brew was not found - run `brew services stop {token}` and install again"
        );
    };
    let st = std::process::Command::new(&brew)
        .args(["services", "stop", token])
        .status()
        .context("failed to run brew services stop")?;
    if !st.success() {
        anyhow::bail!("`brew services stop {token}` failed - run it by hand, then install again");
    }
    Ok(())
}

/// `service install`: the LaunchAgent on macOS, the user unit on Linux.
pub(crate) fn service_install(args: ServiceInstallArgs) -> Result<()> {
    if cfg!(target_os = "linux") {
        systemd_install(args)
    } else {
        launchd_install(args)
    }
}

/// `service uninstall`: the LaunchAgent on macOS, the user unit on Linux.
pub(crate) fn service_uninstall() -> Result<()> {
    if cfg!(target_os = "linux") {
        systemd_uninstall()
    } else {
        launchd_uninstall()
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn launchd_install(args: ServiceInstallArgs) -> Result<()> {
    use lifecycle::{LabelKind, Manager};
    let home = std::env::var("HOME").context("HOME is not set")?;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let state = canonical_service_state();

    // The job's environment: whatever the plist being replaced set, verbatim, then --env on top.
    let mut env = if state == "absent" { Default::default() } else { plist_env(&plist)? };
    for a in &args.env {
        let (k, v) = lifecycle::parse_env_arg(a).map_err(|e| anyhow::anyhow!(e))?;
        env.insert(k, v);
    }
    env.entry("SUPRAGNOSIS_HTTP_ADDR".to_string())
        .or_insert_with(|| "127.0.0.1:7373".to_string());
    // L7, before anything is touched: the job adds no exposure, whichever source a setting came from.
    lifecycle::refuse_exposure(&env, |a| parse_loopback_addr(a).is_ok())
        .map_err(|e| anyhow::anyhow!(e))?;

    // The store the JOB will open - from its own environment, since launchd does not hand it this
    // shell's - is what a second writer would contend for.
    let store = daemon_store_path(|k| env.get(k).cloned());
    let observed = observe_with(store.as_deref());
    let canonical_loaded = observed.jobs.iter().any(|j| j.kind == LabelKind::Canonical);

    // L1 and L5: refuse to share, refuse a holder nothing names, and refuse to overwrite a person's
    // file - the first and last unless told to take over.
    let hand = (state == "hand_written").then_some(lifecycle::HAND_WRITTEN_PLIST);
    let others =
        lifecycle::plan_install(&observed, args.take_over, hand).map_err(|e| anyhow::anyhow!(e))?;

    let exe = std::env::current_exe()?.canonicalize()?;
    let program = lifecycle::stable_program(&exe, |p| p.exists());
    let program_str = program.to_string_lossy().to_string();
    if program_str.contains("/target/debug/") || program_str.contains("/target/release/") {
        println!(
            "note: the job will run a build-tree binary ({program_str}); `cargo clean` removes it"
        );
    }

    // Retire the others (take-over).
    for m in &others {
        match m {
            Manager::Pidfile { pid } => stop_pidfile(*pid as i32)?,
            // Units exist only on Linux, which installs through systemd_install.
            Manager::Systemd(_) => {}
            Manager::Launchd(job) => match job.kind {
                LabelKind::Homebrew(token) => {
                    brew_services_stop(token)?;
                    println!(
                        "retired brew services job {} (brew services stop {token})",
                        job.label
                    );
                }
                LabelKind::Retired => {
                    launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", job.label)]);
                    let old = std::path::PathBuf::from(&home)
                        .join("Library/LaunchAgents")
                        .join(format!("{}.plist", job.label));
                    if old.exists() {
                        let to = move_aside(&old, job.label)?;
                        println!("retired {} - its plist is now {}", job.label, to.display());
                    } else {
                        println!("retired {}", job.label);
                    }
                }
                LabelKind::Canonical => {}
            },
        }
    }
    if canonical_loaded {
        launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", lifecycle::CANONICAL_LABEL)]);
    }
    // The plan saw a manager for whatever answered; this checks that retiring them freed the store.
    // A holder that outlives them (an app-spawned daemon beside a failing job) is the one the plan
    // could not see, and installing now would start the crash loop all the same.
    if !wait_until_released(store.as_deref()) {
        anyhow::bail!(
            "{} still answers, or the store is still held, after retiring the managers above - something else holds it, so not installing beside it.\n{}",
            status_http_addr(),
            lifecycle::UNRECOGNIZED_HOLDER
        );
    }
    if state == "hand_written" {
        let to = move_aside(&plist, lifecycle::CANONICAL_LABEL)?;
        println!("moved the hand-written plist aside: {}", to.display());
    }

    std::fs::create_dir_all(plist.parent().context("LaunchAgents path has no parent")?)?;
    std::fs::create_dir_all(log_dir())?;
    let text = lifecycle::render_plist(&program_str, &home, &env, env!("CARGO_PKG_VERSION"));
    std::fs::write(&plist, text).with_context(|| format!("failed to write {}", plist.display()))?;
    let st = std::process::Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{uid}"))
        .arg(&plist)
        .status()
        .context("failed to run launchctl bootstrap")?;
    if !st.success() {
        anyhow::bail!("launchctl bootstrap gui/{uid} {} failed", plist.display());
    }
    println!("installed {} -> {}", lifecycle::CANONICAL_LABEL, plist.display());
    println!("  program {program_str} serve");
    for (k, v) in &env {
        println!("  env     {k}={v}");
    }
    println!("  the daemon starts now and at every login (supragnosis service uninstall to undo)");
    // The model, so the first tool call does not find it missing (semantic-retrieval.md Section
    // 6). A failure is reported and does not fail the install: the daemon is useful without it.
    // `setup` asks for none and fetches it as its own step, which nothing waits on (O5).
    if args.no_model {
        println!("  model   not fetched here (--no-model) - `supragnosis embed fetch` fetches it");
    } else if let Err(e) = embed_fetch(true, false) {
        println!("  model   not fetched: {e:#} - run `supragnosis embed fetch` when online");
    }
    await_daemon(lifecycle::CANONICAL_LABEL)
}

#[cfg(target_os = "macos")]
pub(crate) fn launchd_uninstall() -> Result<()> {
    use lifecycle::LabelKind;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let state = canonical_service_state();
    let observed = observe();
    if observed.jobs.iter().any(|j| j.kind == LabelKind::Canonical) {
        launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", lifecycle::CANONICAL_LABEL)]);
        wait_until_released(daemon_store_path(|k| std::env::var(k).ok()).as_deref());
        println!("stopped {}", lifecycle::CANONICAL_LABEL);
    }
    match lifecycle::uninstall_file_step(state) {
        lifecycle::FileStep::Remove => {
            std::fs::remove_file(&plist)
                .with_context(|| format!("failed to remove {}", plist.display()))?;
            println!("removed {} - the daemon no longer starts at login", plist.display());
        }
        lifecycle::FileStep::MoveAside => {
            let to = move_aside(&plist, lifecycle::CANONICAL_LABEL)?;
            println!(
                "moved the hand-written plist aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        lifecycle::FileStep::Nothing => println!("no canonical job is installed"),
    }
    let rest: Vec<String> = observed
        .jobs
        .iter()
        .filter(|j| j.kind != LabelKind::Canonical)
        .map(|j| format!("  launchd {}", j.label))
        .collect();
    if !rest.is_empty() {
        println!("other managers are still loaded (not touched):\n{}", rest.join("\n"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_install(_args: ServiceInstallArgs) -> Result<()> {
    anyhow::bail!("service install manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither - `supragnosis serve` runs the daemon in the foreground")
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn launchd_uninstall() -> Result<()> {
    anyhow::bail!("service uninstall manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither")
}

/// Why there is no user manager to install into, and what runs the daemon instead.
pub(crate) fn no_user_manager(why: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "systemctl --user does not work here ({why}) - as in many containers, or a session without a user bus - so there is no user manager to keep the daemon running. `supragnosis serve` runs it in the foreground, and `supragnosis start` runs it until you log out"
    )
}

/// The directories a unit's daemon writes outside `~/.supragnosis`, which `ProtectSystem=strict` and
/// `ProtectHome=read-only` would otherwise refuse it: a data directory or viewer socket its
/// environment moved elsewhere.
pub(crate) fn unit_writable_paths(
    env: &std::collections::BTreeMap<String, String>,
    home: &str,
) -> Vec<String> {
    let state = std::path::PathBuf::from(home).join(".supragnosis");
    let mut out: Vec<String> = Vec::new();
    let data = env.get("SUPRAGNOSIS_DATA_DIR").map(std::path::PathBuf::from);
    let sock = env
        .get("SUPRAGNOSIS_VIZ_SOCK")
        .and_then(|v| std::path::Path::new(v).parent().map(std::path::Path::to_path_buf));
    for dir in [data, sock].into_iter().flatten() {
        let d = dir.to_string_lossy().to_string();
        if dir.is_absolute() && !dir.starts_with(&state) && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// `service install` on Linux (daemon-lifecycle.md Section 4, "On Linux"): the canonical user unit,
/// generated, enabled and started, with lingering asked for. The rules are the LaunchAgent's - refuse
/// to share, refuse a holder nothing names, never overwrite a file it did not write, carry the
/// environment, add no exposure - read for a unit.
pub(crate) fn systemd_install(args: ServiceInstallArgs) -> Result<()> {
    use lifecycle::{LabelKind, Manager};
    systemd_user_available().map_err(|why| no_user_manager(&why))?;
    let home = std::env::var("HOME").context("HOME is not set")?;
    let user = current_user().context("could not determine the user name (id -un)")?;
    let unit = canonical_service_path();
    let state = canonical_service_state();

    // The unit's environment: whatever the unit being replaced set, read as systemd reads it, then
    // --env on top (L5's carrying).
    let mut env = if state == "absent" {
        Default::default()
    } else {
        let text = std::fs::read_to_string(&unit)
            .with_context(|| format!("failed to read {}", unit.display()))?;
        lifecycle::parse_unit_env(&text, &home, &user)
            .map_err(|e| anyhow::anyhow!("{}: {e}", unit.display()))?
    };
    for a in &args.env {
        let (k, v) = lifecycle::parse_env_arg(a).map_err(|e| anyhow::anyhow!(e))?;
        env.insert(k, v);
    }
    env.entry("SUPRAGNOSIS_HTTP_ADDR".to_string())
        .or_insert_with(|| "127.0.0.1:7373".to_string());
    lifecycle::refuse_exposure(&env, |a| parse_loopback_addr(a).is_ok())
        .map_err(|e| anyhow::anyhow!(e))?;

    let store = daemon_store_path(|k| env.get(k).cloned());
    let observed = observe_with(store.as_deref());
    let hand = (state == "hand_written").then_some(lifecycle::HAND_WRITTEN_UNIT);
    let others =
        lifecycle::plan_install(&observed, args.take_over, hand).map_err(|e| anyhow::anyhow!(e))?;

    let exe = std::env::current_exe()?.canonicalize()?;
    let program = lifecycle::stable_program(&exe, |p| p.exists());
    let program_str = program.to_string_lossy().to_string();
    if program_str.contains("/target/debug/") || program_str.contains("/target/release/") {
        println!(
            "note: the unit will run a build-tree binary ({program_str}); `cargo clean` removes it"
        );
    }
    let writable = unit_writable_paths(&env, &home);
    let text = lifecycle::render_unit(&program_str, &env, &writable, env!("CARGO_PKG_VERSION"))
        .map_err(|e| anyhow::anyhow!("the unit cannot be written: {e}"))?;

    for m in &others {
        match m {
            Manager::Pidfile { pid } => stop_pidfile(*pid as i32)?,
            Manager::Systemd(job) => {
                if let LabelKind::Homebrew(token) = job.kind {
                    brew_services_stop(token)?;
                    println!(
                        "retired brew services unit {} (brew services stop {token})",
                        job.label
                    );
                }
            }
            Manager::Launchd(_) => {}
        }
    }
    // Whatever is at the canonical path stops before its file changes: a generated unit is
    // rewritten, a copied one moved aside.
    if state != "absent" || observed.units.iter().any(|j| j.kind == LabelKind::Canonical) {
        let _ = systemctl_user(&["disable", "--now", lifecycle::CANONICAL_UNIT]);
    }
    if !wait_until_released(store.as_deref()) {
        anyhow::bail!(
            "{} still answers, or the store is still held, after retiring the managers above - something else holds it, so not installing beside it.\n{}",
            status_http_addr(),
            lifecycle::UNRECOGNIZED_HOLDER
        );
    }
    let moved = if state == "hand_written" {
        let to = move_file_aside(&unit, lifecycle::moved_aside_unit_path(&home, unix_secs()))?;
        println!("moved the unit that was not generated aside: {}", to.display());
        Some(to)
    } else {
        None
    };

    std::fs::create_dir_all(unit.parent().context("the unit path has no parent")?)?;
    std::fs::create_dir_all(base_dir())?;
    for dir in &writable {
        std::fs::create_dir_all(dir).with_context(|| format!("failed to create {dir}"))?;
    }
    std::fs::write(&unit, text).with_context(|| format!("failed to write {}", unit.display()))?;
    systemctl_user(&["daemon-reload"])?;
    systemctl_user(&["enable", "--now", lifecycle::CANONICAL_UNIT])?;
    println!("installed {} -> {}", lifecycle::CANONICAL_UNIT, unit.display());
    println!("  program {program_str} serve");
    for (k, v) in &env {
        println!("  env     {k}={v}");
    }
    if let Some(to) = moved {
        println!(
            "  carried only the Environment= lines of the unit moved aside - compare the rest (an ExecStart with its own flags, ordering) with {}",
            to.display()
        );
    }
    // A user manager stops with the person's last session unless it lingers. polkit lets a person
    // turn it on for themselves on most distributions; where it does not, an administrator has to
    // (L8: asked, never escalated).
    match linger_state() {
        Some(true) => println!("  linger  on - it keeps running after you log out"),
        _ => {
            let out = std::process::Command::new("loginctl").arg("enable-linger").output();
            match out {
                Ok(o) if o.status.success() => println!(
                    "  linger  turned on (loginctl enable-linger) - it keeps running after you log out"
                ),
                Ok(o) => println!(
                    "  linger  off, and turning it on was refused ({}) - it stops when you log out until an administrator runs `sudo loginctl enable-linger {user}`",
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => println!(
                    "  linger  off, and loginctl could not be run ({e}) - it stops when you log out until an administrator runs `sudo loginctl enable-linger {user}`"
                ),
            }
        }
    }
    println!(
        "  the daemon starts now and at every login (supragnosis service uninstall to undo); its log is `journalctl --user -u {}`",
        lifecycle::CANONICAL_UNIT
    );
    if args.no_model {
        println!("  model   not fetched here (--no-model) - `supragnosis embed fetch` fetches it");
    } else if let Err(e) = embed_fetch(true, false) {
        println!("  model   not fetched: {e:#} - run `supragnosis embed fetch` when online");
    }
    await_daemon(lifecycle::CANONICAL_UNIT)
}

/// `service uninstall` on Linux: the canonical unit is disabled and stopped, and its file removed -
/// or moved aside when this command did not write it (L5). Lingering is left as it is, since other
/// units may need it.
pub(crate) fn systemd_uninstall() -> Result<()> {
    use lifecycle::LabelKind;
    systemd_user_available().map_err(|why| no_user_manager(&why))?;
    let home = std::env::var("HOME").context("HOME is not set")?;
    let unit = canonical_service_path();
    let state = canonical_service_state();
    let observed = observe();
    if state != "absent" || observed.units.iter().any(|j| j.kind == LabelKind::Canonical) {
        let _ = systemctl_user(&["disable", "--now", lifecycle::CANONICAL_UNIT]);
        wait_until_released(daemon_store_path(|k| std::env::var(k).ok()).as_deref());
        println!("stopped and disabled {}", lifecycle::CANONICAL_UNIT);
    }
    match lifecycle::uninstall_file_step(state) {
        lifecycle::FileStep::Remove => {
            std::fs::remove_file(&unit)
                .with_context(|| format!("failed to remove {}", unit.display()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!("removed {} - the daemon no longer starts at login", unit.display());
        }
        lifecycle::FileStep::MoveAside => {
            let to = move_file_aside(&unit, lifecycle::moved_aside_unit_path(&home, unix_secs()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!(
                "moved the unit that was not generated aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        lifecycle::FileStep::Nothing => println!("no canonical unit is installed"),
    }
    let rest: Vec<String> = observed
        .units
        .iter()
        .filter(|j| j.kind != LabelKind::Canonical)
        .map(|j| format!("  systemd {}", j.label))
        .collect();
    if !rest.is_empty() {
        println!("other managers are still enabled (not touched):\n{}", rest.join("\n"));
    }
    if linger_state() == Some(true) {
        println!("lingering is left on - other units may need it (`loginctl disable-linger` turns it off)");
    }
    Ok(())
}

/// docs/compatibility.md Section 6: each machine-read answer has one example document in
/// tests/fixtures/json, read by these tests and by the desktop app's. The producer and its example
/// must have the same keys at every level and the same kind of value wherever both hold one. A
/// field this side renames or drops fails here; a field the app reads that the example lacks fails
/// on the app's side.
#[cfg(test)]
mod service_files {
    use super::*;

    /// L5: the operator's file is theirs - nothing deletes a plist or a unit the product did not
    /// write. Uninstall removes only the file it generated and moves one a person wrote aside; the
    /// move is a rename that keeps every byte at the timestamped path, under a directory it makes,
    /// and one that cannot be made leaves the file where it was. Every service command's file step
    /// goes through these: a removal appears only in uninstall's arm for the generated file.
    #[test]
    fn an_operators_file_is_moved_aside_never_deleted() {
        use lifecycle::{uninstall_file_step, FileStep};
        assert_eq!(uninstall_file_step("generated"), FileStep::Remove);
        assert_eq!(uninstall_file_step("hand_written"), FileStep::MoveAside);
        assert_eq!(uninstall_file_step("absent"), FileStep::Nothing);

        let home = std::env::temp_dir().join(format!("sg-move-aside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let agents = home.join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let plist = agents.join("com.supragnosis.daemon.plist");
        std::fs::write(&plist, "<plist>the operator's own</plist>").unwrap();
        let dst =
            lifecycle::moved_aside_path(home.to_str().unwrap(), lifecycle::CANONICAL_LABEL, 7);
        assert_eq!(move_file_aside(&plist, dst.clone()).unwrap(), dst);
        assert!(!plist.exists(), "out of LaunchAgents, so launchd stops loading it");
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "<plist>the operator's own</plist>");

        let unit = home.join(".config/systemd/user/supragnosis.service");
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "[Service]\n").unwrap();
        // The place for the copy is taken by a file, so its directory cannot be made.
        std::fs::write(home.join(".supragnosis/systemd"), "").unwrap();
        let blocked = lifecycle::moved_aside_unit_path(home.to_str().unwrap(), 7);
        assert!(move_file_aside(&unit, blocked).is_err());
        assert_eq!(std::fs::read_to_string(&unit).unwrap(), "[Service]\n", "left where it was");
        let _ = std::fs::remove_dir_all(&home);

        // Each function's working body, not a stub for another system: the real ones open with a
        // `use` or take their arguments.
        let main = include_str!("service.rs");
        let body = |head: &str| {
            let at = main.find(head).unwrap_or_else(|| panic!("no {head}"));
            let rest = &main[at..];
            &rest[..rest.find("\n}\n").expect("it closes")]
        };
        for uninstall in [
            "fn launchd_uninstall() -> Result<()> {\n    use",
            "fn systemd_uninstall() -> Result<()> {\n    use",
        ] {
            let b = body(uninstall);
            let remove = b.find("FileStep::Remove =>").expect("the generated arm");
            let aside = b.find("FileStep::MoveAside =>").expect("the hand-written arm");
            assert_eq!(b.matches("remove_file(").count(), 1, "{uninstall}: one removal");
            let removal = b.find("remove_file(").unwrap();
            assert!(remove < removal && removal < aside, "{uninstall}: only of the generated file");
            assert!(
                b[aside..].contains("move_"),
                "{uninstall}: the hand-written one is moved aside"
            );
        }
        for install in ["fn launchd_install(args", "fn systemd_install(args"] {
            let b = body(install);
            assert!(b.contains("move_"), "{install} moves a hand-written file aside");
            assert!(!b.contains("remove_file("), "{install} removes no file");
        }
    }
}
