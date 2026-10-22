//! Daemon lifecycle decisions (docs/daemon-lifecycle.md).
//!
//! Everything here is a pure function of what was observed - which launchd labels are loaded or which
//! systemd user units are enabled or active, whether the pidfile names a live process, whether the
//! daemon answers - so the rules are tested as tables and run on every platform. Only observing and
//! acting shell out, and that lives in main.rs.

/// Who installed a launchd job, which decides how it is described and retired (Section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// The canonical LaunchAgent (`supragnosis service install`, deploy/install.sh).
    Canonical,
    /// A `brew services` job; the field is the formula token that owns it.
    Homebrew(&'static str),
    /// A label an earlier release installed and nothing installs any more.
    Retired,
}

/// The label of the one always-on manager on macOS (Section 4).
pub const CANONICAL_LABEL: &str = "com.supragnosis.daemon";

/// Every launchd label the product has ever installed a job under (Section 3, L2).
///
/// Retired labels stay listed: a job nobody installs any more is still a job that can be loaded, and
/// a manager the code does not know about is how two of them came to hold one store unnoticed.
pub const KNOWN_LABELS: &[(&str, LabelKind)] = &[
    (CANONICAL_LABEL, LabelKind::Canonical),
    ("sh.brew.supragnosis-server", LabelKind::Homebrew("supragnosis-server")),
    ("homebrew.mxcl.supragnosis-server", LabelKind::Homebrew("supragnosis-server")),
    // The formula's token before it became `supragnosis-server` (the cask took the plain name).
    ("sh.brew.supragnosis", LabelKind::Homebrew("supragnosis")),
    ("homebrew.mxcl.supragnosis", LabelKind::Homebrew("supragnosis")),
    // The pre-0.1.2 deploy label; deploy/install.sh migrated it away.
    ("com.ashon.supragnosis", LabelKind::Retired),
];

/// The one always-on manager on Linux: a systemd user unit (Section 4, "On Linux"). It has the name
/// of the unit deploy/systemd/README.md has people copy, so one name is one manager whoever wrote
/// the file.
pub const CANONICAL_UNIT: &str = "supragnosis.service";

/// Every systemd user unit that runs the daemon (L2): the canonical one, and the units `brew
/// services` creates on Linux - the formula has no service block now, but an older one did.
pub const KNOWN_UNITS: &[(&str, LabelKind)] = &[
    (CANONICAL_UNIT, LabelKind::Canonical),
    ("homebrew.supragnosis-server.service", LabelKind::Homebrew("supragnosis-server")),
    ("homebrew.supragnosis.service", LabelKind::Homebrew("supragnosis")),
];

/// A loaded launchd job under one of [`KNOWN_LABELS`], or a systemd user unit under one of
/// [`KNOWN_UNITS`] that is enabled or active; `label` is then the unit's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub label: &'static str,
    pub kind: LabelKind,
    /// The live process, when the job has one. A loaded job without one is still a manager - a
    /// KeepAlive job failing on start holds no pid and keeps trying, which is the case that matters.
    pub pid: Option<u32>,
    pub last_exit: Option<i64>,
}

/// Something that starts the daemon and believes it is responsible for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manager {
    /// `supragnosis start` - daemonized, recorded in the pidfile.
    Pidfile {
        pid: u32,
    },
    Launchd(Job),
    /// A systemd user unit (Linux).
    Systemd(Job),
}

impl Manager {
    /// One line naming the manager, its process and its origin - what `status` lists.
    pub fn describe(&self) -> String {
        match self {
            Manager::Pidfile { pid } => format!("pidfile (supragnosis start), pid {pid}"),
            Manager::Launchd(j) => format!("launchd {}", describe_job(j)),
            Manager::Systemd(j) => format!("systemd {}", describe_job(j)),
        }
    }
}

/// A job or unit's name, origin and process, the part of [`Manager::describe`] both share.
fn describe_job(j: &Job) -> String {
    let proc = match (j.pid, j.last_exit) {
        (Some(pid), _) => format!("pid {pid}"),
        (None, Some(code)) => format!("not running, last exit {code}"),
        (None, None) => "not running".to_string(),
    };
    let origin = match j.kind {
        LabelKind::Canonical => "canonical".to_string(),
        LabelKind::Homebrew(token) => format!("brew services {token}"),
        LabelKind::Retired => "retired label".to_string(),
    };
    format!("{} ({origin}), {proc}", j.label)
}

/// What the lifecycle commands observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// The pidfile's process, only when it is alive (a stale pidfile manages nothing).
    pub pidfile: Option<u32>,
    pub jobs: Vec<Job>,
    /// The systemd user units that manage the daemon: enabled, or active (Linux).
    pub units: Vec<Job>,
    /// Whether the MCP address accepts a connection.
    pub answering: bool,
    /// Whether a process holds the store open for writing (Section 11). A stdio server binds no
    /// port, so this is the only trace it leaves.
    pub store_held: bool,
}

/// The one question every lifecycle command asks first (Section 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Situation {
    /// Nothing manages the daemon and nothing answers.
    Stopped,
    /// Exactly one manager - the only case in which acting is safe.
    One(Manager),
    /// More than one manager claims the daemon. The store admits one writer, so at most one of
    /// them can be serving; acting on either is a guess, and guessing is how a crash loop gets a
    /// second wind (L1).
    Conflict(Vec<Manager>),
    /// Something answers, or holds the store, that is none of the known managers. Reported as such,
    /// never as stopped (P5: unknown is not absent).
    Unrecognized,
}

pub fn classify(o: &Observed) -> Situation {
    let mut managers: Vec<Manager> = Vec::new();
    if let Some(pid) = o.pidfile {
        managers.push(Manager::Pidfile { pid });
    }
    managers.extend(o.jobs.iter().cloned().map(Manager::Launchd));
    managers.extend(o.units.iter().cloned().map(Manager::Systemd));
    match managers.len() {
        0 if o.answering || o.store_held => Situation::Unrecognized,
        0 => Situation::Stopped,
        1 => Situation::One(managers.remove(0)),
        _ => Situation::Conflict(managers),
    }
}

/// Why `service install` will not start a job beside a holder it cannot name, take-over or not.
pub const UNRECOGNIZED_HOLDER: &str = "something this CLI does not manage is serving the daemon's address or holding its store - most likely a daemon the desktop app started for its session, a stdio server an MCP client launched (`claude mcp add supragnosis -- supragnosis` starts one per session), or a `supragnosis serve` in a terminal. Nothing names it, so nothing can retire it, and a login job started beside it would fail on the store lock and be retried forever. Turn on Start at Login in the app instead (it stops its own daemon first), or quit the app, register MCP clients over HTTP rather than stdio, or stop that process - then run this again";

/// Whether `ps -o comm=` output names this program (L9). macOS prints the executable's path, Linux
/// its name; either way the last component is `supragnosis` - not `supragnosis-app`, the desktop
/// shell, and not whatever process inherited a pid the pidfile still names.
pub fn is_supragnosis_command(comm: &str) -> bool {
    let comm = comm.trim();
    !comm.is_empty() && comm.rsplit('/').next() == Some("supragnosis")
}

/// What the pidfile says (L9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pidfile {
    /// No pidfile, or one that names no pid.
    Absent,
    /// It names a live supragnosis process - the only pid a signal may go to.
    Live(i32),
    /// It names a process that is gone, or one that is not supragnosis: cleared, never signalled.
    Stale(i32),
}

/// Reads the pidfile's `text` (when it exists), asking `alive` and `is_supragnosis` about the pid it
/// names - the process reader kept outside, so a table can stand in for `kill -0` and `ps`.
pub fn pidfile_state(
    text: Option<&str>,
    alive: impl Fn(i32) -> bool,
    is_supragnosis: impl Fn(i32) -> bool,
) -> Pidfile {
    match text.and_then(|t| t.trim().parse::<i32>().ok()) {
        None => Pidfile::Absent,
        Some(pid) if alive(pid) && is_supragnosis(pid) => Pidfile::Live(pid),
        Some(pid) => Pidfile::Stale(pid),
    }
}

/// L7: the environment a generated job would run with must not widen what the daemon exposes or
/// switch off a defence. Each refusal names the key, because it came either from `--env` or from the
/// plist being replaced, and the operator has to know which line to change (L5: their file is not
/// edited for them). `loopback` is the daemon's own address check, so the two cannot disagree.
pub fn refuse_exposure(
    env: &std::collections::BTreeMap<String, String>,
    loopback: impl Fn(&str) -> bool,
) -> Result<(), String> {
    let get = |k: &str| env.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let mut refused = Vec::new();
    // The daemon's own reading: only an explicit "off" disables auth, case-insensitively.
    if get("SUPRAGNOSIS_MCP_AUTH").is_some_and(|v| v.eq_ignore_ascii_case("off")) {
        refused.push("SUPRAGNOSIS_MCP_AUTH=off turns off the bearer token".to_string());
    }
    if get("SUPRAGNOSIS_SCAN_SECRETS").is_some_and(|v| matches!(v.as_str(), "0" | "off" | "false"))
    {
        refused.push("SUPRAGNOSIS_SCAN_SECRETS turns off the ingest secret scan".to_string());
    }
    if let Some(addr) = get("SUPRAGNOSIS_HTTP_ADDR") {
        if !loopback(&addr) {
            refused.push(format!(
                "SUPRAGNOSIS_HTTP_ADDR={addr} is not a loopback address - the daemon refuses it, so the job would exit on every start"
            ));
        }
    }
    if refused.is_empty() {
        return Ok(());
    }
    Err(format!(
        "the generated job would run with:\n  {}\nA login job adds no exposure (daemon-lifecycle.md L7). Remove the setting from --env, or from the plist or unit being replaced, and install again; a daemon that needs it can be run by hand",
        refused.join("\n  ")
    ))
}

/// What `service install` retires before installing the canonical job, or why it refuses (Section 4,
/// L1, L5). `hand_written` is the refusal for the canonical file when it exists without the
/// generator's marker - [`HAND_WRITTEN_PLIST`] or [`HAND_WRITTEN_UNIT`] - and `None` otherwise.
///
/// An unrecognized holder is refused even under --take-over. Take-over retires managers by name, and
/// this one has none: the likeliest is a daemon the desktop app spawned, which has no pidfile and no
/// launchd job. Installing beside it reproduces Section 1's crash loop from the command meant to end
/// it. The canonical job itself is never in the list - it is replaced, not retired.
pub fn plan_install(
    o: &Observed,
    take_over: bool,
    hand_written: Option<&str>,
) -> Result<Vec<Manager>, String> {
    if classify(o) == Situation::Unrecognized {
        return Err(UNRECOGNIZED_HOLDER.to_string());
    }
    let mut others: Vec<Manager> =
        o.pidfile.map(|pid| Manager::Pidfile { pid }).into_iter().collect();
    others.extend(
        o.jobs
            .iter()
            .filter(|j| j.kind != LabelKind::Canonical)
            .cloned()
            .map(Manager::Launchd),
    );
    others.extend(
        o.units
            .iter()
            .filter(|j| j.kind != LabelKind::Canonical)
            .cloned()
            .map(Manager::Systemd),
    );
    if !take_over {
        if !others.is_empty() {
            let list: Vec<String> = others.iter().map(|m| format!("  {}", m.describe())).collect();
            return Err(format!(
                "another manager already runs the daemon, and the store admits one writer:\n{}\nre-run with --take-over to retire it and install the canonical job",
                list.join("\n")
            ));
        }
        if let Some(why) = hand_written {
            return Err(why.to_string());
        }
    }
    Ok(others)
}

/// Why install leaves a hand-written plist at the canonical path (L5).
pub const HAND_WRITTEN_PLIST: &str = "~/Library/LaunchAgents/com.supragnosis.daemon.plist was written by hand. --take-over moves it aside (to ~/.supragnosis/launchd/) and carries its EnvironmentVariables into the generated job";

/// Why install leaves a unit it did not generate at the canonical path - the one deploy/systemd has
/// people copy (L5).
pub const HAND_WRITTEN_UNIT: &str = "~/.config/systemd/user/supragnosis.service was not generated by this command (deploy/systemd/README.md has people copy it). --take-over moves it aside (to ~/.supragnosis/systemd/) and carries its Environment= lines into the generated unit";

/// The refusal a lifecycle command gives on a conflict: every manager, which one is serving, and the
/// command that resolves it.
pub fn conflict_message(managers: &[Manager]) -> String {
    let mut s = format!(
        "{} managers claim the daemon, and the store admits one writer - refusing to guess which to act on:",
        managers.len()
    );
    for m in managers {
        s.push_str("\n  ");
        s.push_str(&m.describe());
    }
    s.push_str(
        "\nresolve: supragnosis service install --take-over   (keeps the canonical job, retires the rest)",
    );
    s
}

/// Reads the pid and last exit status out of `launchctl list <label>` for a loaded job.
///
/// The output is launchd's old-style plist: `"PID" = 55439;` appears only while the job has a
/// process, and `"LastExitStatus" = 256;` once it has exited at least once.
pub fn parse_launchctl_list(out: &str) -> (Option<u32>, Option<i64>) {
    let field = |key: &str| {
        out.lines().find_map(|l| {
            let (k, v) = l.trim().split_once('=')?;
            (k.trim().trim_matches('"') == key)
                .then(|| v.trim().trim_end_matches(';').trim().to_string())
        })
    };
    (
        field("PID").and_then(|v| v.parse().ok()),
        field("LastExitStatus").and_then(|v| v.parse().ok()),
    )
}

/// What `systemctl --user show <unit>` says about one unit, read from the properties main.rs asks
/// for: LoadState, ActiveState, UnitFileState, MainPID, ExecMainStatus and
/// ExecMainExitTimestampMonotonic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitShow {
    pub loaded: bool,
    pub active: String,
    pub unit_file: String,
    pub pid: Option<u32>,
    /// The main process's last exit status, once it has exited at least once - as launchd's
    /// LastExitStatus appears only then.
    pub last_exit: Option<i64>,
}

impl UnitShow {
    /// A unit manages the daemon while it is enabled or active (Section 4, "On Linux"): an enabled
    /// unit starts it at the next login even after a `stop`, and one restarting after a failure is
    /// still trying. Installed but neither is a file, not a manager - as a plist that is not loaded.
    pub fn manages(&self) -> bool {
        let enabled = matches!(
            self.unit_file.as_str(),
            "enabled" | "enabled-runtime" | "linked" | "linked-runtime" | "alias"
        );
        let active = matches!(
            self.active.as_str(),
            "active" | "activating" | "deactivating" | "reloading" | "refreshing"
        );
        self.loaded && (enabled || active)
    }
}

/// Reads `systemctl --user show` output: `Key=Value` lines.
pub fn parse_systemctl_show(out: &str) -> UnitShow {
    let field = |key: &str| {
        out.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let pid = field("MainPID").and_then(|v| v.parse::<u32>().ok()).filter(|p| *p != 0);
    let exited = field("ExecMainExitTimestampMonotonic")
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|t| t != 0);
    UnitShow {
        loaded: field("LoadState").as_deref() == Some("loaded"),
        active: field("ActiveState").unwrap_or_default(),
        unit_file: field("UnitFileState").unwrap_or_default(),
        pid,
        last_exit: if exited { field("ExecMainStatus").and_then(|v| v.parse().ok()) } else { None },
    }
}

/// One value in a unit file's own quoting: double quotes around it, a backslash before a backslash
/// or a quote, and a percent sign doubled, since systemd expands `%` specifiers in these lines.
fn unit_quote(v: &str) -> Result<String, String> {
    if v.contains(['\n', '\r']) {
        return Err(format!("{v:?}: a unit file cannot hold a line break in a value"));
    }
    Ok(format!(
        "\"{}\"",
        v.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")
    ))
}

/// The canonical systemd user unit, generated (Section 4, "On Linux"). It carries every directive of
/// `deploy/systemd/supragnosis.service` - what a user manager can enforce, where systemd reads it -
/// with `ExecStart` naming `program serve`, one `Environment=` line per variable the job carries, and
/// a `ReadWritePaths=` line for each directory in `writable` the daemon must write outside
/// `~/.supragnosis` (a data directory or viewer socket the environment moved).
pub fn render_unit(
    program: &str,
    env: &std::collections::BTreeMap<String, String>,
    writable: &[String],
    version: &str,
) -> Result<String, String> {
    let mut env_lines = String::new();
    if !env.contains_key("RUST_LOG") {
        env_lines.push_str("Environment=RUST_LOG=info\n");
    }
    for (k, v) in env {
        if k.is_empty() || k.contains(|c: char| c.is_whitespace() || c == '=' || c == '"') {
            return Err(format!("{k:?} is not a name a unit's Environment= can hold"));
        }
        env_lines.push_str(&format!("Environment={}\n", unit_quote(&format!("{k}={v}"))?));
    }
    let mut paths = String::new();
    for w in writable {
        paths.push_str(&format!("ReadWritePaths={}\n", unit_quote(w)?));
    }
    let program = unit_quote(program)?;
    Ok(format!(
        r#"; {GENERATED_MARKER} (v{version}). Regenerate rather than edit: install rewrites
; everything here except the Environment= lines, which it carries forward. Its directives are those
; of deploy/systemd/supragnosis.service (docs/daemon-lifecycle.md Section 4, "On Linux").

[Unit]
Description=supragnosis - the agent-first ontology server
Documentation=https://supragnosis.dev/
Documentation=https://github.com/Ashon/supragnosis/blob/main/docs/daemon-lifecycle.md
After=network-online.target
Wants=network-online.target
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=simple
ExecStart={program} serve
Restart=on-failure
RestartSec=5s
{env_lines}KillMode=mixed
KillSignal=SIGTERM
TimeoutStopSec=30
UMask=0077
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=%h/.supragnosis
{paths}RestrictSUIDSGID=true
RestrictNamespaces=true
LockPersonality=true

[Install]
WantedBy=default.target
"#
    ))
}

/// The `Environment=` assignments of a unit file, as systemd reads them: each line holds one or more
/// assignments split at whitespace, a quoted one kept whole with its quotes removed and its escapes
/// read. `%h`, `%u` and `%%` are expanded with `home` and `user`; any other specifier is refused,
/// since carrying it into the generated unit would change what it means. An `EnvironmentFile=` is
/// refused too: its contents are not in the file to carry.
pub fn parse_unit_env(
    text: &str,
    home: &str,
    user: &str,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut env = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("EnvironmentFile=") {
            return Err(format!(
                "the unit reads its environment from a file ({line}), which install cannot carry - move those settings into Environment= lines, or pass them with --env"
            ));
        }
        let Some(rest) = line.strip_prefix("Environment=") else {
            continue;
        };
        for word in unit_words(rest)? {
            let word = expand_specifiers(&word, home, user)?;
            let Some((k, v)) = word.split_once('=') else {
                return Err(format!("Environment={rest}: {word:?} is not KEY=VALUE"));
            };
            env.insert(k.to_string(), v.to_string());
        }
    }
    Ok(env)
}

/// Splits a unit line's value into words the way systemd does for Environment=: whitespace
/// separates, double or single quotes group, and inside double quotes a backslash escapes.
fn unit_words(s: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        None => return Err(format!("{s}: an unclosed quote")),
                        Some(q) if q == c => break,
                        Some('\\') if c == '"' => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some(e) => cur.push(e),
                            None => return Err(format!("{s}: a trailing backslash")),
                        },
                        Some(o) => cur.push(o),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(e) => cur.push(e),
                    None => return Err(format!("{s}: a trailing backslash")),
                }
            }
            o => {
                in_word = true;
                cur.push(o);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

fn expand_specifiers(word: &str, home: &str, user: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('h') => out.push_str(home),
            Some('u') => out.push_str(user),
            Some(o) => {
                return Err(format!(
                    "{word:?} uses the specifier %{o}, which install does not expand - write the value out, or pass it with --env"
                ))
            }
            None => return Err(format!("{word:?} ends in a lone %")),
        }
    }
    Ok(out)
}

/// Where a unit the product did not write goes instead of being deleted (L5).
pub fn moved_aside_unit_path(home: &str, unix_secs: u64) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{home}/.supragnosis/systemd/{CANONICAL_UNIT}.{unix_secs}"))
}

/// The running daemon's version against this binary's (Section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Same(String),
    /// The daemon still runs an older (or newer) image than the binary now on disk - the state an
    /// upgrade leaves until the daemon is restarted.
    Differs {
        running: String,
        here: String,
    },
    /// The daemon did not answer, so its version is not known - and not assumed to be this one.
    Unknown,
}

pub fn drift(running: Option<&str>, here: &str) -> Drift {
    match running {
        None => Drift::Unknown,
        Some(r) if r == here => Drift::Same(r.to_string()),
        Some(r) => Drift::Differs { running: r.to_string(), here: here.to_string() },
    }
}

/// The `version` field of the viewer's `/api/about` body.
pub fn parse_about_version(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("version")?.as_str().map(str::to_string)
}

/// The text that marks a plist as written by `supragnosis service install` (Section 4). Anything at
/// the canonical path without it was written by a person, and is theirs (L5).
pub const GENERATED_MARKER: &str = "generated by supragnosis service install";

pub fn is_generated(plist: &str) -> bool {
    plist.contains(GENERATED_MARKER)
}

/// The program path a LaunchAgent should name so that upgrades reach it (Section 4).
///
/// A Homebrew install runs from a versioned keg - `<prefix>/Cellar/supragnosis-server/<v>/bin/...` -
/// and naming that path would pin the version the job starts. Homebrew repoints
/// `<prefix>/opt/supragnosis-server` on upgrade, so when the keg's `opt` link exists the job names it.
/// Anything else (a source build, `~/.local/bin`) is named as found.
pub fn stable_program(
    exe: &std::path::Path,
    exists: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let s = exe.to_string_lossy();
    if let Some(i) = s.find("/Cellar/supragnosis-server/") {
        let opt =
            std::path::PathBuf::from(format!("{}/opt/supragnosis-server/bin/supragnosis", &s[..i]));
        if exists(&opt) {
            return opt;
        }
    }
    exe.to_path_buf()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The canonical LaunchAgent, generated (Section 4). `env` is what the job carries: the MCP address,
/// plus whatever a replaced plist set or `--env` gave - verbatim, because several settings
/// (SUPRAGNOSIS_HOST among them, which every new observation's provenance records) exist only as
/// environment variables, and dropping them would change the daemon's identity silently.
pub fn render_plist(
    program: &str,
    home: &str,
    env: &std::collections::BTreeMap<String, String>,
    version: &str,
) -> String {
    let mut env_xml = String::new();
    for (k, v) in env {
        env_xml.push_str(&format!(
            "        <key>{}</key>\n        <string>{}</string>\n",
            xml_escape(k),
            xml_escape(v)
        ));
    }
    let home = xml_escape(home);
    // No "--" anywhere in the comment: XML forbids it inside one.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- {GENERATED_MARKER} (v{version}). Regenerate rather than edit: install rewrites
     everything here except EnvironmentVariables, which it carries forward. -->
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{CANONICAL_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{program}</string>
        <string>serve</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
{env_xml}    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{home}/.supragnosis/log/supragnosis.out.log</string>
    <key>StandardErrorPath</key>
    <string>{home}/.supragnosis/log/supragnosis.err.log</string>
</dict>
</plist>
"#,
        program = xml_escape(program),
    )
}

/// Where a plist the product did not write goes instead of being deleted (L5): out of
/// LaunchAgents, so launchd stops loading it, and under a timestamp, so nothing is overwritten.
pub fn moved_aside_path(home: &str, label: &str, unix_secs: u64) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{home}/.supragnosis/launchd/{label}.plist.{unix_secs}"))
}

/// Parses one `--env KEY=VALUE`. Only SUPRAGNOSIS_* keys: the job's environment is this daemon's
/// configuration, not a place to smuggle unrelated variables into a login-time process.
pub fn parse_env_arg(arg: &str) -> Result<(String, String), String> {
    let (k, v) = arg.split_once('=').ok_or_else(|| format!("--env {arg}: expected KEY=VALUE"))?;
    let k = k.trim();
    if !k.starts_with("SUPRAGNOSIS_") || k.len() == "SUPRAGNOSIS_".len() {
        return Err(format!("--env {arg}: only SUPRAGNOSIS_* variables configure the daemon"));
    }
    Ok((k.to_string(), v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(label: &'static str, kind: LabelKind, pid: Option<u32>, last_exit: Option<i64>) -> Job {
        Job { label, kind, pid, last_exit }
    }

    /// Section 3 as a table. The incident row is the one this exists for: the canonical job serving
    /// and Homebrew's job loaded but failing on the lock - two managers, a conflict, not "running".
    #[test]
    fn classification_counts_managers_not_processes() {
        let canonical = job(CANONICAL_LABEL, LabelKind::Canonical, Some(2292), None);
        let brew_failing = job(
            "sh.brew.supragnosis-server",
            LabelKind::Homebrew("supragnosis-server"),
            None,
            Some(1),
        );
        let cases: Vec<(&str, Observed, Situation)> = vec![
            ("nothing", Observed::default(), Situation::Stopped),
            (
                "answering, nothing known",
                Observed { answering: true, ..Default::default() },
                Situation::Unrecognized,
            ),
            (
                "pidfile only",
                Observed { pidfile: Some(7), answering: true, ..Default::default() },
                Situation::One(Manager::Pidfile { pid: 7 }),
            ),
            (
                "brew only - the README path the CLI used to refuse",
                Observed { jobs: vec![brew_failing.clone()], ..Default::default() },
                Situation::One(Manager::Launchd(brew_failing.clone())),
            ),
            (
                "the 2026-10-03 incident",
                Observed {
                    jobs: vec![canonical.clone(), brew_failing.clone()],
                    answering: true,
                    ..Default::default()
                },
                Situation::Conflict(vec![
                    Manager::Launchd(canonical.clone()),
                    Manager::Launchd(brew_failing.clone()),
                ]),
            ),
            (
                "pidfile beside a launchd job",
                Observed {
                    pidfile: Some(7),
                    jobs: vec![canonical.clone()],
                    answering: true,
                    store_held: true,
                    ..Default::default()
                },
                Situation::Conflict(vec![Manager::Pidfile { pid: 7 }, Manager::Launchd(canonical)]),
            ),
        ];
        for (name, observed, want) in cases {
            assert_eq!(classify(&observed), want, "{name}");
        }
    }

    /// Section 4's refusals as a table. The first row is the one found before v0.4.3 shipped: the
    /// desktop app's own daemon answering, invisible to the CLI, with `--take-over` given.
    #[test]
    fn install_refuses_a_holder_it_cannot_name() {
        let canonical = job(CANONICAL_LABEL, LabelKind::Canonical, Some(2292), None);
        let brew = job(
            "sh.brew.supragnosis-server",
            LabelKind::Homebrew("supragnosis-server"),
            Some(9),
            None,
        );
        let app_child = Observed { answering: true, ..Default::default() };
        for take_over in [false, true] {
            let refused = plan_install(&app_child, take_over, None).unwrap_err();
            assert_eq!(refused, UNRECOGNIZED_HOLDER, "take_over={take_over}");
        }

        // Nothing there: install, retiring nothing.
        assert_eq!(plan_install(&Observed::default(), false, None), Ok(vec![]));
        // The canonical job is replaced, never listed as something to retire.
        let ours =
            Observed { jobs: vec![canonical.clone()], answering: true, ..Default::default() };
        assert_eq!(plan_install(&ours, false, None), Ok(vec![]));

        // Another manager: refused by name without --take-over, retired with it (L1).
        let theirs =
            Observed { jobs: vec![canonical, brew.clone()], answering: true, ..Default::default() };
        let refused = plan_install(&theirs, false, None).unwrap_err();
        assert!(refused.contains("sh.brew.supragnosis-server") && refused.contains("--take-over"));
        assert_eq!(plan_install(&theirs, true, None), Ok(vec![Manager::Launchd(brew)]));
        let pidfile = Observed { pidfile: Some(7), answering: true, ..Default::default() };
        assert_eq!(plan_install(&pidfile, true, None), Ok(vec![Manager::Pidfile { pid: 7 }]));

        // A person's plist is not overwritten without --take-over, which moves it aside (L5).
        let hand = Some(HAND_WRITTEN_PLIST);
        assert!(plan_install(&Observed::default(), false, hand).unwrap_err().contains("by hand"));
        assert_eq!(plan_install(&Observed::default(), true, hand), Ok(vec![]));
    }

    /// Section 11: a stdio server holds the store and answers nothing. Held with no manager is
    /// unrecognized - and install refuses it, take-over or not - while held with a manager is just
    /// that manager's daemon running.
    #[test]
    fn a_store_held_by_no_manager_is_unrecognized() {
        let stdio = Observed { store_held: true, ..Default::default() };
        assert_eq!(classify(&stdio), Situation::Unrecognized);
        assert_eq!(plan_install(&stdio, true, None), Err(UNRECOGNIZED_HOLDER.to_string()));
        let ours = job(CANONICAL_LABEL, LabelKind::Canonical, Some(1), None);
        let running = Observed {
            jobs: vec![ours.clone()],
            answering: true,
            store_held: true,
            ..Default::default()
        };
        assert_eq!(classify(&running), Situation::One(Manager::Launchd(ours)));
    }

    /// L9: only this program's executable counts - by its last path component, so neither the
    /// desktop shell nor whatever inherited a reused pid passes.
    #[test]
    fn a_pid_counts_only_when_it_is_supragnosis() {
        assert!(is_supragnosis_command(
            "/opt/homebrew/Cellar/supragnosis-server/0.4.3/bin/supragnosis"
        ));
        assert!(is_supragnosis_command("supragnosis\n"));
        assert!(!is_supragnosis_command("/bin/sleep"));
        assert!(!is_supragnosis_command(
            "/Applications/Supragnosis.app/Contents/MacOS/supragnosis-app"
        ));
        assert!(!is_supragnosis_command(""));
    }

    /// L9: a signal goes only to a supragnosis process. The pidfile counts as live only when its pid
    /// is alive and is supragnosis; a pid that is gone, or one another program now holds, is stale -
    /// cleared, never signalled - and a file that names no pid is no pidfile.
    #[test]
    fn a_pidfile_naming_anything_else_is_stale() {
        let alive = |pid: i32| pid != 3;
        let ours = |pid: i32| pid == 7;
        let state = |text: Option<&str>| pidfile_state(text, alive, ours);
        assert_eq!(state(Some("7\n")), Pidfile::Live(7));
        assert_eq!(state(Some("5")), Pidfile::Stale(5), "alive, but another program's");
        assert_eq!(state(Some("3")), Pidfile::Stale(3), "gone");
        assert_eq!(state(None), Pidfile::Absent);
        assert_eq!(state(Some("")), Pidfile::Absent);
        assert_eq!(state(Some("seven")), Pidfile::Absent);
        let asked = std::cell::Cell::new(false);
        let gone = pidfile_state(
            Some("3"),
            |_| false,
            |_| {
                asked.set(true);
                true
            },
        );
        assert_eq!(gone, Pidfile::Stale(3));
        assert!(!asked.get(), "a pid that is gone is not looked up further");

        // The CLI's pidfile reads go through this one decision, and the only pid the lifecycle
        // commands see - the one a stop or a restart signals - is a live one.
        let main = include_str!("main.rs");
        assert!(
            main.contains("lifecycle::pidfile_state("),
            "the CLI reads the pidfile through it"
        );
        let observed = &main[main.find("    lifecycle::Observed {\n").expect("the observation")..];
        let observed = &observed[..observed.find("    }\n").expect("it closes")];
        assert!(observed.contains("pidfile: live_pidfile()"), "{observed}");
        let live = &main[main.find("fn live_pidfile()").expect("live_pidfile")..];
        let live = &live[..live.find("\n}\n").expect("it closes")];
        assert!(
            live.contains("Pidfile::Live(pid) => Some(pid)") && !live.contains("Stale(pid) =>")
        );
    }

    /// L7: the generated job refuses an environment that disables auth or the secret scan, or names
    /// an address the daemon would refuse - whether it came from --env or from a replaced plist.
    #[test]
    fn the_generated_job_refuses_an_environment_that_adds_exposure() {
        let loopback = |a: &str| a.starts_with("127.") || a.starts_with("[::1]");
        let env = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };
        assert_eq!(
            refuse_exposure(
                &env(&[("SUPRAGNOSIS_HTTP_ADDR", "127.0.0.1:7373"), ("SUPRAGNOSIS_STORE", "redb")]),
                loopback
            ),
            Ok(())
        );
        assert_eq!(
            refuse_exposure(&env(&[("SUPRAGNOSIS_MCP_AUTH", "on")]), loopback),
            Ok(()),
            "only an explicit off disables auth"
        );
        for bad in [
            ("SUPRAGNOSIS_MCP_AUTH", "OFF"),
            ("SUPRAGNOSIS_SCAN_SECRETS", "false"),
            ("SUPRAGNOSIS_HTTP_ADDR", "0.0.0.0:7373"),
        ] {
            let err = refuse_exposure(&env(&[bad]), loopback).unwrap_err();
            assert!(err.contains(bad.0), "{bad:?} must be named: {err}");
        }
    }

    #[test]
    fn every_known_label_is_distinct_and_one_is_canonical() {
        let mut labels: Vec<&str> = KNOWN_LABELS.iter().map(|(l, _)| *l).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), KNOWN_LABELS.len(), "a label listed twice");
        let canonical: Vec<_> =
            KNOWN_LABELS.iter().filter(|(_, k)| *k == LabelKind::Canonical).collect();
        assert_eq!(canonical, vec![&(CANONICAL_LABEL, LabelKind::Canonical)]);
    }

    /// daemon-lifecycle.md L2: every manager the product has installed stays recognized, retired
    /// ones included. Distinctness alone would let a retired label be deleted unnoticed, so each is
    /// pinned by name - and dropping one has to change this list on purpose.
    #[test]
    fn every_label_ever_installed_stays_known() {
        let labels: Vec<(&str, LabelKind)> = KNOWN_LABELS.to_vec();
        for expected in [
            ("com.supragnosis.daemon", LabelKind::Canonical),
            ("sh.brew.supragnosis-server", LabelKind::Homebrew("supragnosis-server")),
            ("homebrew.mxcl.supragnosis-server", LabelKind::Homebrew("supragnosis-server")),
            ("sh.brew.supragnosis", LabelKind::Homebrew("supragnosis")),
            ("homebrew.mxcl.supragnosis", LabelKind::Homebrew("supragnosis")),
            ("com.ashon.supragnosis", LabelKind::Retired),
        ] {
            assert!(labels.contains(&expected), "{expected:?} left the recognized labels");
        }
        let units: Vec<(&str, LabelKind)> = KNOWN_UNITS.to_vec();
        for expected in [
            ("supragnosis.service", LabelKind::Canonical),
            ("homebrew.supragnosis-server.service", LabelKind::Homebrew("supragnosis-server")),
            ("homebrew.supragnosis.service", LabelKind::Homebrew("supragnosis")),
        ] {
            assert!(units.contains(&expected), "{expected:?} left the recognized units");
        }
    }

    #[test]
    fn launchctl_list_output_yields_pid_and_last_exit() {
        let running = "{\n\t\"Label\" = \"com.supragnosis.daemon\";\n\t\"LastExitStatus\" = 9;\n\t\"PID\" = 55439;\n\t\"Program\" = \"/opt/homebrew/opt/supragnosis-server/bin/supragnosis\";\n};";
        assert_eq!(parse_launchctl_list(running), (Some(55439), Some(9)));
        let failing =
            "{\n\t\"Label\" = \"sh.brew.supragnosis-server\";\n\t\"LastExitStatus\" = 256;\n};";
        assert_eq!(parse_launchctl_list(failing), (None, Some(256)));
        assert_eq!(parse_launchctl_list(""), (None, None));
    }

    #[test]
    fn drift_never_assumes_the_running_version() {
        assert_eq!(drift(Some("0.4.2"), "0.4.2"), Drift::Same("0.4.2".into()));
        assert_eq!(
            drift(Some("0.4.0"), "0.4.2"),
            Drift::Differs { running: "0.4.0".into(), here: "0.4.2".into() }
        );
        assert_eq!(drift(None, "0.4.2"), Drift::Unknown);
        assert_eq!(
            parse_about_version(r#"{"name":"supragnosis","version":"0.4.0"}"#).as_deref(),
            Some("0.4.0")
        );
        assert_eq!(parse_about_version("not json"), None);
    }

    #[test]
    fn a_keg_path_becomes_the_opt_link_that_upgrades_repoint() {
        use std::path::Path;
        let keg = Path::new("/opt/homebrew/Cellar/supragnosis-server/0.4.2/bin/supragnosis");
        let opt = Path::new("/opt/homebrew/opt/supragnosis-server/bin/supragnosis");
        assert_eq!(stable_program(keg, |p| p == opt), opt);
        // No opt link (a broken install): name what exists rather than invent a path.
        assert_eq!(stable_program(keg, |_| false), keg);
        let src = Path::new("/Users/me/.local/bin/supragnosis");
        assert_eq!(stable_program(src, |_| true), src);
    }

    #[test]
    fn the_generated_plist_is_marked_escaped_and_carries_env_verbatim() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("SUPRAGNOSIS_HTTP_ADDR".to_string(), "127.0.0.1:7373".to_string());
        env.insert("SUPRAGNOSIS_HOST".to_string(), "a&b<c>".to_string());
        let p = render_plist(
            "/opt/homebrew/opt/supragnosis-server/bin/supragnosis",
            "/Users/me",
            &env,
            "0.4.2",
        );
        assert!(is_generated(&p));
        assert!(p.contains("<string>com.supragnosis.daemon</string>"));
        assert!(p.contains("<string>serve</string>"));
        assert!(
            p.contains("<key>SUPRAGNOSIS_HOST</key>\n        <string>a&amp;b&lt;c&gt;</string>")
        );
        assert!(p.contains("/Users/me/.supragnosis/log/supragnosis.err.log"));
        // An XML comment may not contain "--"; launchd would refuse the file.
        let comment = &p[p.find("<!--").unwrap() + 4..p.find("-->").unwrap()];
        assert!(!comment.contains("--"));
        assert!(!is_generated("<plist><dict><key>Label</key></dict></plist>"));
    }

    #[test]
    fn env_args_configure_the_daemon_and_nothing_else() {
        assert_eq!(
            parse_env_arg("SUPRAGNOSIS_HOST=ashon-mac"),
            Ok(("SUPRAGNOSIS_HOST".into(), "ashon-mac".into()))
        );
        assert!(parse_env_arg("PATH=/tmp").is_err());
        assert!(parse_env_arg("SUPRAGNOSIS_=x").is_err());
        assert!(parse_env_arg("SUPRAGNOSIS_HOST").is_err());
        assert_eq!(
            moved_aside_path("/Users/me", CANONICAL_LABEL, 42),
            std::path::PathBuf::from(
                "/Users/me/.supragnosis/launchd/com.supragnosis.daemon.plist.42"
            )
        );
    }

    #[test]
    fn a_conflict_names_every_manager_and_the_fix() {
        let m = vec![
            Manager::Launchd(job(CANONICAL_LABEL, LabelKind::Canonical, Some(2292), None)),
            Manager::Launchd(job(
                "sh.brew.supragnosis-server",
                LabelKind::Homebrew("supragnosis-server"),
                None,
                Some(1),
            )),
        ];
        let msg = conflict_message(&m);
        assert!(msg.contains("com.supragnosis.daemon (canonical), pid 2292"));
        assert!(msg.contains("sh.brew.supragnosis-server (brew services supragnosis-server), not running, last exit 1"));
        assert!(msg.contains("service install --take-over"));
    }

    /// Section 4, "On Linux": what `systemctl --user show` says decides whether a unit manages the
    /// daemon - enabled or active - and a unit that does is a manager like a loaded launchd job:
    /// one is acted on, two are a conflict, and Homebrew's is retired by take-over.
    #[test]
    fn a_unit_manages_while_enabled_or_active() {
        let show = |active: &str, file: &str, pid: &str, status: &str, exited: &str| {
            parse_systemctl_show(&format!(
                "LoadState=loaded\nActiveState={active}\nUnitFileState={file}\nMainPID={pid}\nExecMainStatus={status}\nExecMainExitTimestampMonotonic={exited}\n"
            ))
        };
        let running = show("active", "enabled", "4242", "0", "0");
        assert_eq!((running.pid, running.last_exit), (Some(4242), None));
        assert!(running.manages());
        let stopped_enabled = show("inactive", "enabled", "0", "0", "123");
        assert_eq!((stopped_enabled.pid, stopped_enabled.last_exit), (None, Some(0)));
        assert!(stopped_enabled.manages(), "an enabled unit starts it at the next login");
        let restarting = show("activating", "disabled", "0", "1", "456");
        assert_eq!(restarting.last_exit, Some(1));
        assert!(restarting.manages(), "a unit restarting after a failure is still trying");
        assert!(show("failed", "enabled", "0", "1", "789").manages());
        assert!(!show("inactive", "disabled", "0", "0", "0").manages(), "a file, not a manager");
        assert!(!show("failed", "disabled", "0", "1", "9").manages());
        let missing = parse_systemctl_show(
            "LoadState=not-found\nActiveState=inactive\nUnitFileState=\nMainPID=0\n",
        );
        assert!(!missing.manages());

        let unit = job(CANONICAL_UNIT, LabelKind::Canonical, Some(4242), None);
        let brew = job(
            "homebrew.supragnosis-server.service",
            LabelKind::Homebrew("supragnosis-server"),
            None,
            Some(1),
        );
        let one = Observed { units: vec![unit.clone()], answering: true, ..Default::default() };
        assert_eq!(classify(&one), Situation::One(Manager::Systemd(unit.clone())));
        assert_eq!(
            Manager::Systemd(unit.clone()).describe(),
            "systemd supragnosis.service (canonical), pid 4242"
        );
        let two = Observed {
            pidfile: Some(7),
            units: vec![unit.clone()],
            answering: true,
            ..Default::default()
        };
        assert_eq!(
            classify(&two),
            Situation::Conflict(vec![Manager::Pidfile { pid: 7 }, Manager::Systemd(unit.clone())])
        );
        // The canonical unit is replaced, Homebrew's is retired by name (L1).
        let theirs =
            Observed { units: vec![unit, brew.clone()], answering: true, ..Default::default() };
        assert!(plan_install(&theirs, false, None).unwrap_err().contains("homebrew.supragnosis"));
        assert_eq!(plan_install(&theirs, true, None), Ok(vec![Manager::Systemd(brew)]));
        // A copied unit is not overwritten without --take-over (L5).
        let refused = plan_install(&Observed::default(), false, Some(HAND_WRITTEN_UNIT));
        assert!(refused.unwrap_err().contains("~/.config/systemd/user/supragnosis.service"));

        let mut names: Vec<&str> = KNOWN_UNITS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KNOWN_UNITS.len(), "a unit listed twice");
        let canonical: Vec<_> =
            KNOWN_UNITS.iter().filter(|(_, k)| *k == LabelKind::Canonical).collect();
        assert_eq!(canonical, vec![&(CANONICAL_UNIT, LabelKind::Canonical)]);
    }

    /// Section 4, "On Linux": the generated unit is the shipped one's directives with the generated
    /// program, environment and marker, so a hub that copied the shipped unit loses nothing it set by
    /// being taken over, and the README's two gotchas stay fixed in both.
    #[test]
    fn the_generated_unit_carries_the_shipped_directives_and_the_marker() {
        const SHIPPED: &str = include_str!("../../../deploy/systemd/supragnosis.service");
        let mut env = std::collections::BTreeMap::new();
        env.insert("SUPRAGNOSIS_HTTP_ADDR".to_string(), "127.0.0.1:7373".to_string());
        env.insert("SUPRAGNOSIS_HOST".to_string(), "my \"host\" 100%".to_string());
        let unit = render_unit(
            "/home/me/.local/bin/supragnosis",
            &env,
            &["/data/sg".to_string()],
            "0.7.0",
        )
        .unwrap();
        assert!(is_generated(&unit) && !is_generated(SHIPPED));
        for line in SHIPPED.lines().map(str::trim) {
            // ExecStart is generated, and a Documentation= line is a pointer, not behaviour: the
            // shipped unit points at the hub's document, the generated one at the lifecycle's.
            let generated = ["ExecStart=", "Documentation="];
            if line.is_empty()
                || line.starts_with(';')
                || generated.iter().any(|g| line.starts_with(g))
            {
                continue;
            }
            assert!(unit.lines().any(|l| l == line), "the generated unit lacks {line:?}");
        }
        assert!(unit.contains("ExecStart=\"/home/me/.local/bin/supragnosis\" serve\n"));
        assert!(unit.contains("Environment=\"SUPRAGNOSIS_HTTP_ADDR=127.0.0.1:7373\"\n"));
        assert!(unit.contains("Environment=\"SUPRAGNOSIS_HOST=my \\\"host\\\" 100%%\"\n"));
        assert!(unit.contains("ReadWritePaths=\"/data/sg\"\n"));
        // The start limit lives in [Unit], where systemd reads it (deploy/systemd/README.md).
        let service = unit.find("[Service]").unwrap();
        assert!(unit.find("StartLimitBurst=5").unwrap() < service);
        assert!(render_unit("/x", &env, &["a\nb".to_string()], "0").is_err());
    }

    /// L5's carrying, on Linux: a copied unit's Environment= lines are read the way systemd reads
    /// them, so what take-over carries is what the daemon ran with - and what the generator writes
    /// reads back the same.
    #[test]
    fn a_copied_units_environment_is_read_as_systemd_quotes_it() {
        let copied = "[Service]\nExecStart=%h/.local/bin/supragnosis serve\nEnvironment=RUST_LOG=info\nEnvironment=\"SUPRAGNOSIS_HOST=hub one\" SUPRAGNOSIS_WORKSPACE=w\nEnvironment=SUPRAGNOSIS_DATA_DIR=%h/data 'SUPRAGNOSIS_EMBED=none'\n";
        let env = parse_unit_env(copied, "/home/me", "me").unwrap();
        let want: Vec<(&str, &str)> = vec![
            ("RUST_LOG", "info"),
            ("SUPRAGNOSIS_DATA_DIR", "/home/me/data"),
            ("SUPRAGNOSIS_EMBED", "none"),
            ("SUPRAGNOSIS_HOST", "hub one"),
            ("SUPRAGNOSIS_WORKSPACE", "w"),
        ];
        assert_eq!(env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), want);
        assert!(parse_unit_env("EnvironmentFile=/etc/sg.env\n", "/h", "u").is_err());
        assert!(parse_unit_env("Environment=SUPRAGNOSIS_HOST=%n\n", "/h", "u").is_err());
        assert!(parse_unit_env("Environment=\"SUPRAGNOSIS_HOST=x\n", "/h", "u").is_err());

        let mut odd = std::collections::BTreeMap::new();
        odd.insert("SUPRAGNOSIS_HOST".to_string(), "a \"b\" \\c 50% 'd'".to_string());
        let unit = render_unit("/bin/supragnosis", &odd, &[], "0").unwrap();
        let back = parse_unit_env(&unit, "/h", "u").unwrap();
        assert_eq!(back.get("SUPRAGNOSIS_HOST"), odd.get("SUPRAGNOSIS_HOST"));
        assert_eq!(
            moved_aside_unit_path("/home/me", 42),
            std::path::PathBuf::from("/home/me/.supragnosis/systemd/supragnosis.service.42")
        );
    }
}
