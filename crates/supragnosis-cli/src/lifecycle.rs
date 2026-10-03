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
