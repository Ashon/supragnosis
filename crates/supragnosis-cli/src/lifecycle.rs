//! Daemon lifecycle decisions (docs/daemon-lifecycle.md).
//!
//! Everything here is a pure function of what was observed - which launchd labels are loaded, whether
//! the pidfile names a live process, whether the daemon answers - so the rules are tested as tables
//! and run on every platform. Only observing and acting shell out, and that lives in main.rs.

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

/// A loaded launchd job under one of [`KNOWN_LABELS`].
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
}

impl Manager {
    /// One line naming the manager, its process and its origin - what `status` lists.
    pub fn describe(&self) -> String {
        match self {
            Manager::Pidfile { pid } => format!("pidfile (supragnosis start), pid {pid}"),
            Manager::Launchd(j) => {
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
                format!("launchd {} ({origin}), {proc}", j.label)
            }
        }
    }
}

/// What the lifecycle commands observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// The pidfile's process, only when it is alive (a stale pidfile manages nothing).
    pub pidfile: Option<u32>,
    pub jobs: Vec<Job>,
    /// Whether the MCP address accepts a connection.
    pub answering: bool,
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
    /// Something answers that is none of the known managers. Reported as such, never as stopped
    /// (P5: unknown is not absent).
    Unrecognized,
}

pub fn classify(o: &Observed) -> Situation {
    let mut managers: Vec<Manager> = Vec::new();
    if let Some(pid) = o.pidfile {
        managers.push(Manager::Pidfile { pid });
    }
    managers.extend(o.jobs.iter().cloned().map(Manager::Launchd));
    match managers.len() {
        0 if o.answering => Situation::Unrecognized,
        0 => Situation::Stopped,
        1 => Situation::One(managers.remove(0)),
        _ => Situation::Conflict(managers),
    }
}

/// Why `service install` will not start a job beside a holder it cannot name, take-over or not.
pub const UNRECOGNIZED_HOLDER: &str = "something this CLI does not manage is serving the daemon's address - most likely a daemon the desktop app started for its session, or a `supragnosis serve` in a terminal. Nothing names it, so nothing can retire it, and a login job started beside it would fail on the store lock and be retried forever. Turn on Start at Login in the app instead (it stops its own daemon first), or quit the app or stop that process, then run this again";

/// What `service install` retires before installing the canonical job, or why it refuses (Section 4,
/// L1, L5). `hand_written` is whether the canonical plist exists without the generator's marker.
///
/// An unrecognized holder is refused even under --take-over. Take-over retires managers by name, and
/// this one has none: the likeliest is a daemon the desktop app spawned, which has no pidfile and no
/// launchd job. Installing beside it reproduces Section 1's crash loop from the command meant to end
/// it. The canonical job itself is never in the list - it is replaced, not retired.
pub fn plan_install(
    o: &Observed,
    take_over: bool,
    hand_written: bool,
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
    if !take_over {
        if !others.is_empty() {
            let list: Vec<String> = others.iter().map(|m| format!("  {}", m.describe())).collect();
            return Err(format!(
                "another manager already runs the daemon, and the store admits one writer:\n{}\nre-run with --take-over to retire it and install the canonical job",
                list.join("\n")
            ));
        }
        if hand_written {
            return Err(format!(
                "~/Library/LaunchAgents/{CANONICAL_LABEL}.plist was written by hand. --take-over moves it aside (to ~/.supragnosis/launchd/) and carries its EnvironmentVariables into the generated job"
            ));
        }
    }
    Ok(others)
}

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
                Observed { pidfile: Some(7), jobs: vec![canonical.clone()], answering: true },
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
            let refused = plan_install(&app_child, take_over, false).unwrap_err();
            assert_eq!(refused, UNRECOGNIZED_HOLDER, "take_over={take_over}");
        }

        // Nothing there: install, retiring nothing.
        assert_eq!(plan_install(&Observed::default(), false, false), Ok(vec![]));
        // The canonical job is replaced, never listed as something to retire.
        let ours =
            Observed { jobs: vec![canonical.clone()], answering: true, ..Default::default() };
        assert_eq!(plan_install(&ours, false, false), Ok(vec![]));

        // Another manager: refused by name without --take-over, retired with it (L1).
        let theirs =
            Observed { jobs: vec![canonical, brew.clone()], answering: true, ..Default::default() };
        let refused = plan_install(&theirs, false, false).unwrap_err();
        assert!(refused.contains("sh.brew.supragnosis-server") && refused.contains("--take-over"));
        assert_eq!(plan_install(&theirs, true, false), Ok(vec![Manager::Launchd(brew)]));
        let pidfile = Observed { pidfile: Some(7), answering: true, ..Default::default() };
        assert_eq!(plan_install(&pidfile, true, false), Ok(vec![Manager::Pidfile { pid: 7 }]));

        // A person's plist is not overwritten without --take-over, which moves it aside (L5).
        assert!(plan_install(&Observed::default(), false, true).unwrap_err().contains("by hand"));
        assert_eq!(plan_install(&Observed::default(), true, true), Ok(vec![]));
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
}
