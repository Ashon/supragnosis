//! The supragnosis CLI the shell runs: with the login shell's environment, and its machine-read
//! answers read by their schema.

use super::*;

/// Runs the CLI the shell found, blocking. `None` when there is no CLI to run.
pub(crate) fn run_cli(args: &[&str]) -> Option<std::process::Output> {
    let bin = find_server_bin()?;
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::null());
    with_login_env(&mut cmd);
    cmd.output().ok()
}

/// What the person's login shell sets that the CLI's calls need, read once per launch
/// (docs/client-connect.md Section 5). An app opened from the Finder gets
/// `/usr/bin:/bin:/usr/sbin:/sbin`, so a client installed through nvm, fnm, volta or mise is on the
/// shell's `PATH` only, and the CLI called it not installed; and a shell that sets
/// `XDG_CONFIG_HOME` moves OpenCode's configuration, which the CLI then looked for in the wrong
/// place.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct LoginEnv {
    pub(crate) path: Option<String>,
    pub(crate) config_home: Option<String>,
}

/// `None` when the shell could not be asked; the app's own environment then stands.
pub(crate) fn login_env() -> Option<&'static LoginEnv> {
    static LOGIN_ENV: OnceLock<Option<LoginEnv>> = OnceLock::new();
    LOGIN_ENV
        .get_or_init(|| {
            let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
            read_login_env(Path::new(&shell), Duration::from_secs(5))
        })
        .as_ref()
}

fn login_path() -> Option<&'static str> {
    login_env()?.path.as_deref()
}

/// A CLI call's environment: the login shell's `PATH` and `XDG_CONFIG_HOME` over the app's own.
pub(crate) fn with_login_env(cmd: &mut Command) {
    if let Some(path) = cli_path(login_path(), std::env::var_os("PATH").as_deref()) {
        cmd.env("PATH", path);
    }
    if let Some(dir) = login_env().and_then(|e| e.config_home.as_deref()) {
        cmd.env("XDG_CONFIG_HOME", dir);
    }
}

/// The line `read_login_env` prints before the environment, so whatever the shell's startup files
/// print first is skipped.
const ENV_MARK: &str = "__supragnosis_env__";

/// Asks `shell`, as a login and interactive shell, for its environment, and returns what it sets.
/// `None` when it does not answer within `within`, or answers without a `PATH`. Interactive, because
/// nvm and its kind are set up in `.zshrc` and `.bashrc`, which a login shell alone does not read.
pub(crate) fn read_login_env(shell: &Path, within: Duration) -> Option<LoginEnv> {
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &format!("echo {ENV_MARK}; /usr/bin/env")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Read on a thread: a startup file that leaves a process holding the pipe must not hang the app.
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut out);
        let _ = tx.send(out);
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < within => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = rx.recv_timeout(within.saturating_sub(started.elapsed())).ok()?;
    login_env_from_output(&out)
}

/// The `PATH=` and `XDG_CONFIG_HOME=` lines of `env`'s output after [`ENV_MARK`]; anything printed
/// before the mark is not the environment. `None` without a `PATH`.
pub(crate) fn login_env_from_output(out: &str) -> Option<LoginEnv> {
    let after = out.split_once(&format!("{ENV_MARK}\n"))?.1;
    let var = |key: &str| {
        after
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let path = var("PATH=")?;
    Some(LoginEnv { path: Some(path), config_home: var("XDG_CONFIG_HOME=") })
}

/// The `PATH` the app's CLI calls run with: the login shell's directories first, then those of the
/// app's own that the shell's did not name. `None` leaves the app's `PATH` as it is.
pub(crate) fn cli_path(
    login: Option<&str>,
    own: Option<&std::ffi::OsStr>,
) -> Option<std::ffi::OsString> {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(login?).collect();
    for dir in own.map(std::env::split_paths).into_iter().flatten() {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).ok()
}

/// The highest schema of the CLI's machine-read answers this app reads (docs/compatibility.md
/// Section 6). An answer without one is from a CLI before the contract: its fields are the ones
/// this app already reads, and degrades on when they are missing.
pub(crate) const CLI_SCHEMA: u64 = 1;

/// Whether the last machine-read answer carried a schema newer than [`CLI_SCHEMA`]. The tray line
/// and the Settings page then say to update the app, instead of reading a payload whose meaning
/// moved.
pub(crate) static CLI_NEWER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// One machine-read CLI answer, by its schema. `Err` carries a schema newer than this app reads,
/// whose payload is not read at all; `Ok(None)` is output that is not JSON.
pub(crate) fn cli_json(stdout: &[u8]) -> Result<Option<serde_json::Value>, u64> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(stdout) else {
        return Ok(None);
    };
    match v["schema"].as_u64() {
        Some(s) if s > CLI_SCHEMA => Err(s),
        _ => Ok(Some(v)),
    }
}

/// Runs a `--json` command. `None` when there is no answer to read: no CLI, a CLI that does not
/// know the command, or one newer than this app (which [`CLI_NEWER`] records).
pub(crate) fn run_cli_json(args: &[&str]) -> Option<serde_json::Value> {
    let out = run_cli(args)?;
    if !out.status.success() {
        return None;
    }
    let read = cli_json(&out.stdout);
    CLI_NEWER.store(read.is_err(), std::sync::atomic::Ordering::Relaxed);
    read.ok().flatten()
}

/// `supragnosis status --json`. `None` when the CLI predates it - the tray then falls back to what
/// the shell itself knows and disables the switch rather than failing on click.
pub(crate) fn cli_status() -> Option<serde_json::Value> {
    run_cli_json(&["status", "--json"])
}

/// The one line the CLI said that matters: its error on failure, its first line on success.
pub(crate) fn cli_outcome(out: &std::process::Output) -> (bool, String) {
    let text = if out.status.success() { &out.stdout } else { &out.stderr };
    let line = String::from_utf8_lossy(text)
        .lines()
        .map(|l| l.trim().trim_start_matches("Error:").trim().to_string())
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    (out.status.success(), line)
}

/// What a CLI change the page asked for came to: `done` says a success in the page's words, and a
/// refusal is an error carrying the CLI's reason, which the page keeps until it is read
/// (settings-page.md Section 3) - never a success with the refusal tucked beneath it.
pub(crate) fn cli_change(
    out: Option<&std::process::Output>,
    done: impl FnOnce(&std::process::Output) -> String,
) -> Result<String, String> {
    match out.map(|o| (o, cli_outcome(o))) {
        Some((o, (true, _))) => Ok(done(o)),
        Some((_, (false, why))) if why.is_empty() => {
            Err("the supragnosis command failed".to_string())
        }
        Some((_, (false, why))) => Err(why),
        None => Err("the supragnosis command was not found".to_string()),
    }
}
