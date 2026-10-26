//! The shell's tests: the transport, the status line and the CLI's answers, the Welcome page, the
//! window, the tray, and the source tripwires that read the app's own source through `SRC`.

use super::*;

#[cfg(test)]
mod src_tests {
    /// `SRC` is every module main.rs declares, in its order, then main.rs, and no test module: a
    /// needle a test looks for in it cannot match the test's own copy of that needle.
    #[test]
    fn src_is_the_apps_modules_and_no_test() {
        let main = include_str!("main.rs");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut expected = String::new();
        let mut modules = 0;
        let mut prev = "";
        for line in main.lines() {
            if let Some(name) = line.strip_prefix("mod ").and_then(|l| l.strip_suffix(';')) {
                if prev != "#[cfg(test)]" {
                    let path = dir.join(format!("{name}.rs"));
                    expected += &std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                    modules += 1;
                }
            }
            prev = line;
        }
        expected += main;
        assert!(modules > 0, "main.rs's modules were read");
        // A module declared some other way (`pub mod`, a trailing comment, inside another module)
        // would leave both lists without it: every source file is main.rs, a module read above, or
        // one of the two test files.
        let files = std::fs::read_dir(&dir)
            .expect("src reads")
            .filter(|e| e.as_ref().is_ok_and(|e| e.path().extension().is_some_and(|x| x == "rs")))
            .count();
        assert_eq!(
            files,
            modules + 3,
            "a source file is neither main.rs, a module nor a test file"
        );
        // An inline test module in a source file would put test code back into SRC.
        assert!(!super::SRC.contains("#[test]"), "a test sits in a module SRC reads");
        assert!(
            super::SRC == expected,
            "SRC is not main.rs's modules, in their order, then main.rs, and nothing else"
        );
    }
}

#[cfg(test)]
mod conditional_tests {
    use super::{forwarded_if_none_match, http};

    /// The poll's tag reaches the daemon as it was sent (design-system.md U33), and only a tag that
    /// cannot break the request head it is written into: a line break would start a header of its own.
    #[test]
    fn the_poll_tag_is_passed_on_only_when_it_is_one_line() {
        let ok = http::HeaderValue::from_static("\"5f3a9c0d1e2b4a67-1a2b\"");
        assert_eq!(
            forwarded_if_none_match(Some(&ok)).as_deref(),
            Some("\"5f3a9c0d1e2b4a67-1a2b\"")
        );
        assert_eq!(forwarded_if_none_match(None), None);
        let empty = http::HeaderValue::from_static("");
        assert_eq!(forwarded_if_none_match(Some(&empty)), None);
        let tab = http::HeaderValue::from_bytes(b"\"a\"\t\"b\"").unwrap();
        assert_eq!(forwarded_if_none_match(Some(&tab)), None, "only printable ASCII is passed on");
    }
}

#[cfg(test)]
mod csp_tests {
    use super::{with_ipc_sources, TAURI_IPC_SOURCES};

    /// The forwarded policy reaches the webview widened for IPC and unchanged everywhere else.
    ///
    /// The failure this guards is silent in both directions: too narrow and the tray goes deaf
    /// (`connect-src 'self'` refuses Tauri's transport), too wide and the proxy has quietly relaxed
    /// a policy the daemon meant strictly. So `script-src` is asserted to survive byte for byte.
    #[test]
    fn forwarding_widens_connect_src_and_nothing_else() {
        let daemon =
            "default-src 'none'; script-src 'self' viz:; style-src 'self' 'unsafe-inline'; \
                      img-src 'self' data:; connect-src 'self'; base-uri 'none'";
        let out = with_ipc_sources(daemon);

        let connect = out
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("connect-src"))
            .expect("connect-src survives");
        assert_eq!(connect, format!("connect-src 'self' {TAURI_IPC_SOURCES}"));

        // Exactly one connect-src: a second would be ignored by the browser, which is the quiet way
        // this fix could look applied and do nothing.
        assert_eq!(out.matches("connect-src").count(), 1, "one directive, widened in place: {out}");

        // Everything else is byte-identical - the script half above all.
        assert!(out.contains("script-src 'self' viz:"), "script-src must be untouched: {out}");
        assert!(!out.contains("unsafe-eval"));
        for d in ["default-src 'none'", "img-src 'self' data:", "base-uri 'none'"] {
            assert!(out.contains(d), "{d} must survive: {out}");
        }
    }

    /// A policy with no `connect-src` still needs one: `default-src 'none'` denies the fetch by
    /// inheritance, so "absent" is not "permitted".
    #[test]
    fn a_policy_without_connect_src_gains_one() {
        let out = with_ipc_sources("default-src 'none'; script-src 'self'");
        assert!(out.contains(&format!("connect-src {TAURI_IPC_SOURCES}")), "got: {out}");
        assert!(out.contains("default-src 'none'"), "got: {out}");
    }

    /// No policy stays no policy. A daemon that stopped sending one must look like that, not like a
    /// shell-authored guess (the reason the proxy forwards rather than restates).
    #[test]
    fn an_absent_policy_is_not_invented() {
        assert_eq!(with_ipc_sources(""), "");
        assert_eq!(with_ipc_sources("   "), "");
    }
}

#[cfg(test)]
mod status_text_tests {
    use super::{
        app_row, cli_change, cli_outcome, cli_path, login_env_from_output, login_outcome,
        read_login_env, status_text, Daemon,
    };
    use std::time::Duration;

    /// An AI app's row in the settings window says its state and labels its one button with what a
    /// click will do - naming any entry other than the bridge, since the click replaces it - and has
    /// no button where a click could do nothing: the app is not installed, its settings could not be
    /// read, or the CLI is too old to know `connect`.
    #[test]
    fn an_app_item_says_what_a_click_will_do() {
        let r = |installed: bool, entry: &str| serde_json::json!({"installed": installed, "entry": entry});
        let (state, action, kind) = app_row(Some(&r(true, "bridge")));
        assert!(state.contains("bridge") && action == Some("Disconnect") && kind == "connected");
        let (state, action, kind) = app_row(Some(&r(true, "http")));
        assert!(state.contains("HTTP") && action == Some("Switch to the bridge"), "{state}");
        assert_eq!(kind, "attention", "a copy of the token is something to fix");
        assert_eq!(app_row(Some(&r(true, "other"))).1, Some("Switch to the bridge"));
        assert_eq!(app_row(Some(&r(false, "none"))), ("not installed".into(), None, "absent"));
        assert_eq!(app_row(Some(&r(true, "unknown"))).1, None);
        assert_eq!(app_row(None).1, None, "an old CLI cannot connect anything");
        assert_eq!(
            app_row(Some(&r(true, "none"))),
            ("not connected".into(), Some("Connect"), "off")
        );
    }

    /// client-connect.md Section 5: the CLI runs with the login shell's `PATH`. From the Finder the app
    /// has `/usr/bin:/bin:/usr/sbin:/sbin`, and `codex` under `~/.nvm/versions/node/<v>/bin` read
    /// "Not installed" on the Settings page while a terminal's `supragnosis connect` found it. The
    /// shell's `XDG_CONFIG_HOME` goes along, for OpenCode's configuration (Section 13.3).
    #[test]
    fn the_cli_runs_with_the_login_shells_path() {
        // What the startup files print is skipped: only the environment after the mark counts.
        let out = "Last login: today\nPATH=/printed/by/zshrc\n__supragnosis_env__\nHOME=/Users/x\n\
                   PATH=/Users/x/.nvm/versions/node/v24/bin:/usr/bin\n";
        let read = login_env_from_output(out).expect("a PATH");
        assert_eq!(read.path.as_deref(), Some("/Users/x/.nvm/versions/node/v24/bin:/usr/bin"));
        assert_eq!(read.config_home, None, "unset stays unset");
        assert_eq!(login_env_from_output("PATH=/usr/bin\n"), None, "no mark, no environment");
        let xdg = "__supragnosis_env__\nXDG_CONFIG_HOME=/Users/x/dots\nPATH=/usr/bin\n";
        assert_eq!(
            login_env_from_output(xdg).and_then(|e| e.config_home).as_deref(),
            Some("/Users/x/dots")
        );

        // A shell that prints first, sets PATH in its startup, then runs the command it is given.
        let dir = std::env::temp_dir().join(format!("sg-login-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let shell = write(
            "shell",
            "#!/bin/sh\necho 'welcome PATH=/not/this'\nPATH=/opt/nvm/bin:/usr/bin\nexport PATH\neval \"$4\"\n",
        );
        assert_eq!(
            read_login_env(&shell, Duration::from_secs(5)).and_then(|e| e.path).as_deref(),
            Some("/opt/nvm/bin:/usr/bin")
        );
        // A shell that never answers leaves the app's PATH alone, and does not hold the app up.
        let hangs = write("hangs", "#!/bin/sh\nsleep 30\n");
        let started = std::time::Instant::now();
        assert_eq!(read_login_env(&hangs, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        let _ = std::fs::remove_dir_all(&dir);

        // The shell's directories lead; the app's own follow, without repeats.
        assert_eq!(
            cli_path(Some("/opt/nvm/bin:/usr/bin"), Some(std::ffi::OsStr::new("/usr/bin:/bin"))),
            Some(std::ffi::OsString::from("/opt/nvm/bin:/usr/bin:/bin"))
        );
        assert_eq!(cli_path(None, Some(std::ffi::OsStr::new("/usr/bin"))), None);
    }

    fn st(
        situation: &str,
        running: Option<&str>,
        here: &str,
        manager: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "situation": situation,
            "managers": if manager.is_null() { serde_json::json!([]) } else { serde_json::json!([manager]) },
            "version": { "here": here, "running": running },
            "service": { "state": "generated" },
        })
    }

    /// docs/design-system.md Sections 6a and 29.6: the AI app's row keeps its words once, in kit.js,
    /// which both pages draw it with - one pill map, one capture label, the helper on both.
    #[test]
    fn the_ai_app_row_is_built_once() {
        let kit = include_str!("../assets/kit.js");
        for part in [
            "attention: [\"warn\", \"Needs you\"]",
            "label: \"Save the why behind each commit\"",
            "const HELPER = { \"claude-desktop\": \"Restart Claude Desktop\", cursor: \"Open Cursor\" };",
        ] {
            assert!(kit.contains(part), "kit.js does not hold: {part}");
        }
    }

    /// A failing `Output` with the CLI's reason on stderr after a blank line and `Error:`, as the CLI
    /// writes one.
    fn refused(why: &str) -> std::process::Output {
        output(1, "", &format!("\nError: {why}\nhint\n"))
    }

    fn output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// The body of `fn name(` in the app's source (`SRC`), to its closing brace at the start of a line.
    fn source_of(name: &str) -> &'static str {
        let src = super::SRC;
        let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
        let rest = &src[start..];
        &rest[..rest.find("\n}\n").expect("it closes")]
    }

    /// daemon-lifecycle.md L4: the app never discards the CLI's exit status. `cli_outcome` reads the
    /// status and the line that matters - the error on failure, past blank lines and the `Error:`
    /// prefix - and a refused Start at Login change is an error carrying that reason all the way to
    /// the page, whose switch shows it under a failure title of its own rather than under "Starts at
    /// login".
    #[test]
    fn a_refused_login_change_says_why() {
        let why = "another manager runs the daemon: sh.brew.supragnosis-server";
        assert_eq!(cli_outcome(&refused(why)), (false, why.to_string()));
        assert_eq!(cli_outcome(&output(0, "installed\n", "")), (true, "installed".to_string()));

        assert_eq!(login_outcome(false, Some(&refused(why))), Err(why.to_string()));
        assert_eq!(login_outcome(true, Some(&refused(why))), Err(why.to_string()));
        assert_eq!(
            login_outcome(false, Some(&output(1, "", ""))),
            Err("the supragnosis command failed".to_string())
        );
        assert_eq!(
            login_outcome(false, None),
            Err("the supragnosis command was not found".to_string())
        );
        let ok = output(0, "", "");
        assert_eq!(login_outcome(false, Some(&ok)), Ok("starts at login".to_string()));
        assert_eq!(login_outcome(true, Some(&ok)), Ok("no longer starts at login".to_string()));

        // The outcome reaches the page as it is: returned last, never re-wrapped as a success.
        assert!(source_of("set_login").trim_end().ends_with("outcome"));
        assert!(source_of("login_set").trim_end().ends_with("set_login(&app, sock, on).await"));
        let js = include_str!("../assets/settings.js");
        let switch = js.lines().find(|l| l.contains("\"login_set\"")).expect("the switch");
        assert!(
            switch.contains("failTitle: e.target.checked ? \"Start at Login was not turned on\" : \"Start at Login was not turned off\""),
            "the switch names its own failure: {switch}"
        );
    }

    /// settings-page.md Section 3: an error keeps until it is read. A refused CLI change is an error
    /// carrying the CLI's reason, returned as it is, never a success with the refusal beneath it.
    #[test]
    fn a_refused_change_says_why() {
        let why = "the login job could not be loaded";
        assert_eq!(cli_change(Some(&refused(why)), |_| "done".into()), Err(why.to_string()));
        assert_eq!(
            cli_change(Some(&output(0, "x", "")), |o| o.stdout.len().to_string()),
            Ok("1".into())
        );
    }

    /// daemon-lifecycle.md L6: the app stops a daemon it did not spawn only on a person's act that
    /// asks for it. Every CLI call that can stop a daemon is listed with the function it sits in -
    /// the switch's `set_login`, Setup's `setup_run`, and `restart_daemon` and `rebuild_index`,
    /// which bring it back - and each of those is reached from one command of the app's own pages,
    /// which a click calls. A new call that stops a daemon fails this until it is added on purpose.
    #[test]
    fn only_the_persons_act_stops_a_daemon_it_did_not_spawn() {
        let app = super::SRC;
        let within = |at: usize| -> &str {
            let head = app[..at]
                .rfind("\nfn ")
                .max(app[..at].rfind("\nasync fn "))
                .max(app[..at].rfind("\npub(crate) fn "))
                .max(app[..at].rfind("\npub(crate) async fn "))
                .unwrap();
            let name = &app[head..];
            let name = &name[name.find("fn ").unwrap() + 3..];
            &name[..name.find('(').unwrap()]
        };
        let mut stops: Vec<(&str, &str)> = Vec::new();
        for call in [
            "\"service\", \"uninstall\"",
            "\"--take-over\"",
            "run_cli(&[\"restart\"",
            "\"embed\", \"rebuild\"",
            "\"--step\", \"server\"",
            "&[\"stop\"",
        ] {
            for (at, _) in app.match_indices(call) {
                stops.push((call, within(at)));
            }
        }
        stops.sort();
        let mut want = vec![
            ("\"--step\", \"server\"", "setup_run"),
            ("\"--take-over\"", "set_login"),
            ("\"--take-over\"", "setup_run"),
            ("\"embed\", \"rebuild\"", "rebuild_index"),
            ("\"service\", \"uninstall\"", "set_login"),
            ("run_cli(&[\"restart\"", "restart_daemon"),
        ];
        want.sort();
        assert_eq!(stops, want, "every CLI call that can stop a daemon, where it sits");

        // Each is reached only from the command a page's control calls.
        for (helper, command) in [
            ("set_login(&", "login_set"),
            ("restart_daemon(&", "daemon_restart"),
            ("rebuild_index(&", "search_rebuild"),
        ] {
            let callers: Vec<&str> = app.match_indices(helper).map(|(at, _)| within(at)).collect();
            assert_eq!(callers, [command], "{helper} is called by {command} alone");
        }
        for command in ["login_set", "daemon_restart", "search_rebuild", "setup_run"] {
            assert!(crate::APP_COMMANDS.contains(&command), "{command} is a page's command");
        }
    }

    /// daemon-lifecycle.md L6: quitting the app never stops a launchd-managed daemon. On exit the app
    /// reaps only the child it spawned; an attached daemon, one still starting and one that failed
    /// are left alone, and the exit path runs no CLI command - so nothing on the way out can stop a
    /// login job, a Homebrew service or a `supragnosis start` daemon.
    #[test]
    fn quitting_leaves_a_daemon_it_did_not_spawn() {
        assert!(super::reaped_on_exit(Daemon::External).is_none());
        assert!(super::reaped_on_exit(Daemon::Starting).is_none());
        assert!(super::reaped_on_exit(Daemon::Failed("x".into())).is_none());
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut reaped = super::reaped_on_exit(Daemon::Spawned(child)).expect("its own child");
        let _ = reaped.wait();

        let src = super::SRC;
        let exit = &src[src.find("RunEvent::Exit => {").expect("the exit arm")..];
        let exit = &exit[..exit.find("\n            }\n").expect("it closes")];
        assert!(exit.contains("reaped_on_exit(prev)"), "{exit}");
        for acts in ["run_cli", "Command::new", "stop", "service"] {
            assert!(!exit.contains(acts), "the exit path does not reach {acts}: {exit}");
        }
    }

    /// settings-page.md S3: the page decides nothing the CLI or the daemon decides. Every command
    /// the Settings page calls is in one list here, by what it is. A change runs the CLI - in the
    /// command or the helper it calls - and a refusal is an `Err` carrying what the CLI said, which
    /// the page shows. Stopping a peer's sharing is the one change that goes to the daemon's
    /// narrow-only endpoint instead, and runs no CLI. The rest read, or act on the system rather
    /// than a setting. A command the page starts calling fails this until it is placed.
    #[test]
    fn every_setting_change_is_a_cli_call() {
        let app = super::SRC;
        let body = |name: &str| -> &str {
            let at = ["async fn ", "fn ", "pub(crate) async fn ", "pub(crate) fn "]
                .iter()
                .find_map(|k| app.find(&format!("\n{k}{name}(")))
                .unwrap_or_else(|| panic!("no fn {name}"));
            let rest = &app[at + 1..];
            &rest[..rest.find("\n}\n").expect("it closes")]
        };
        // Each change, with the helpers its body reaches the CLI through.
        let changes: &[(&str, &[&str])] = &[
            ("app_toggle", &["toggle_app"]),
            ("capture_set", &[]),
            ("login_set", &["set_login", "login_outcome", "cli_change"]),
            ("daemon_restart", &["restart_daemon"]),
            ("search_rebuild", &["rebuild_index"]),
            ("setup_run", &["run_streamed"]),
        ];
        let reads = ["settings_state", "setup_running", "diagnostics"];
        let system = ["clipboard_copy", "reveal", "app_helper", "setup_stop"];

        // A command is a quoted name of the app's own on a line that calls `invoke(` or `act(`.
        let js = include_str!("../assets/settings.js");
        let mut called: Vec<&str> = Vec::new();
        for line in js.lines().filter(|l| l.contains("invoke(") || l.contains("act(")) {
            for command in crate::APP_COMMANDS {
                if line.contains(&format!("\"{command}\"")) {
                    called.push(command);
                }
            }
        }
        called.sort();
        called.dedup();
        let mut placed: Vec<&str> = changes.iter().map(|(c, _)| *c).collect();
        placed.extend(reads);
        placed.extend(system);
        placed.push("peer_narrow");
        placed.sort();
        assert_eq!(called, placed, "every command the page calls, placed by what it is");

        for (command, helpers) in changes {
            let reach: String =
                std::iter::once(*command).chain(helpers.iter().copied()).map(body).collect();
            assert!(
                reach.contains("run_cli(")
                    || reach.contains("Command::new(bin)")
                    || reach.contains("run_streamed("),
                "{command} runs the CLI"
            );
            assert!(
                reach.contains("cli_outcome")
                    || reach.contains("cli_change")
                    || reach.contains("restart_outcome(")
                    || reach.contains("\"why\": (!status.success())"),
                "{command}: a refusal carries what the CLI said"
            );
        }
        let narrow = body("peer_narrow");
        assert!(narrow.contains("/api/peer/share") && !narrow.contains("run_cli("));
    }

    /// docs/design-system.md Section 9 and U17: the daemon has one word per state, the same on the
    /// tray's line, Settings' This Mac pane, Setup's first card and the starting page.
    #[test]
    fn the_daemon_has_one_word_per_state() {
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut spawned = Daemon::Spawned(child);
        assert!(spawned.status_line().starts_with("Running while this app is open"));
        if let Daemon::Spawned(c) = &mut spawned {
            let _ = c.wait();
        }
        assert_eq!(Daemon::Starting.status_line(), "Starting...");
        assert_eq!(Daemon::External.status_line(), "Running - started outside this app");
        assert_eq!(Daemon::Failed("x".into()).status_line(), "Could not start: X.");
        let stopped = serde_json::json!({ "situation": "none", "answering": false, "managers": [], "version": { "here": "0.4.2" } });
        assert_eq!(status_text(&Daemon::External, Some(&stopped)), "Stopped");

        let settings = include_str!("../assets/settings.js");
        let words = &settings[settings.find("function daemonStatus(").expect("daemonStatus")..];
        let words = &words[..words.find("\n}\n").expect("it closes")];
        for word in [
            "\"Two managers run Supragnosis\"",
            "\"Update ready - restart\"",
            "\"Starting...\"",
            "\"Could not start\"",
            "\"Stopped\"",
            "\"Repairing - \"",
            "\"Running\"",
        ] {
            assert!(words.contains(word), "This Mac's card does not say {word}");
        }
        assert!(settings.contains("owed === 0 ? \"Up to date\" : \"Repairing - \""));
        let welcome = include_str!("../assets/welcome.js");
        assert!(welcome.contains("title: \"Keep Supragnosis running\""));
        let setup = include_str!("../../crates/supragnosis-cli/src/setup.rs");
        assert!(setup.contains(
            "\"Running while this app is open. Setup keeps it running at login instead.\""
        ));
    }

    /// The line names who runs the daemon - including a child of this app, which the CLI cannot see.
    #[test]
    fn the_line_names_the_manager() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let brew = serde_json::json!({ "type": "launchd", "source": "homebrew", "label": "sh.brew.supragnosis-server" });
        let s = st("one", Some("0.4.2"), "0.4.2", canonical);
        assert_eq!(status_text(&Daemon::External, Some(&s)), "Running 0.4.2 - starts at login");
        let s = st("one", Some("0.4.2"), "0.4.2", brew);
        assert_eq!(status_text(&Daemon::External, Some(&s)), "Running 0.4.2 - Homebrew's service");
        let s = st("unrecognized", Some("0.4.2"), "0.4.2", serde_json::Value::Null);
        assert_eq!(
            status_text(&Daemon::External, Some(&s)),
            "Running 0.4.2 - started outside this app"
        );
    }

    /// The 2026-10-03 symptom, said where it is read: an upgraded binary under an old process.
    #[test]
    fn drift_and_conflict_are_said_outright() {
        let canonical = serde_json::json!({ "type": "launchd", "source": "canonical", "label": "com.supragnosis.daemon" });
        let s = st("one", Some("0.4.0"), "0.4.2", canonical);
        assert_eq!(
            status_text(&Daemon::External, Some(&s)),
            "Update ready - restart: 0.4.0 running, 0.4.2 installed"
        );
        let conflict = serde_json::json!({
            "situation": "conflict",
            "managers": [{ "type": "launchd" }, { "type": "launchd" }],
            "version": { "here": "0.4.2", "running": "0.4.0" },
        });
        assert_eq!(status_text(&Daemon::External, Some(&conflict)), "Two managers run Supragnosis");
    }

    /// Without a CLI that answers `status --json`, the shell says only what it knows itself.
    #[test]
    fn an_old_cli_falls_back_to_the_shells_own_view() {
        assert_eq!(status_text(&Daemon::External, None), "Running - started outside this app");
        assert_eq!(status_text(&Daemon::Starting, None), "Starting...");
        assert_eq!(
            status_text(&Daemon::Failed("port in use - full log: x".into()), None),
            "Could not start: Port in use."
        );
    }
}

/// docs/onboarding.md Section 6: when the Welcome page opens, when the tray offers to finish, what
/// the page reads from setup, and which acts are the page's own (O4, O6).
#[cfg(test)]
mod welcome_tests {
    use super::{
        essentials, finish_setup_offered, setup_events, welcome_at_launch, WelcomePrefs,
        APP_COMMANDS, SETUP_TOO_OLD,
    };

    fn output(stdout: &str, stderr: &str, code: i32) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// The first launch opens the page; a later one opens it while the daemon or the apps step waits,
    /// unless the person chose "Finish later". Start at Login turned off settles the daemon step their
    /// way, for the launch and the tray alike. Steps setup could not report open nothing.
    #[test]
    fn the_welcome_page_opens_on_the_first_launch_and_while_a_step_waits() {
        let first = WelcomePrefs::default();
        let seen = WelcomePrefs { seen: true, ..first };
        assert!(welcome_at_launch(first, None), "the first launch");
        assert!(!welcome_at_launch(seen, None), "setup did not answer");
        assert!(!welcome_at_launch(seen, Some((true, true))), "both steps done");
        assert!(welcome_at_launch(seen, Some((false, true))), "the daemon is not kept running");
        assert!(welcome_at_launch(seen, Some((true, false))), "no app is connected");
        let later = WelcomePrefs { later: true, ..seen };
        assert!(!welcome_at_launch(later, Some((false, false))), "Finish later");
        assert!(finish_setup_offered(later, Some((false, false))), "the tray still offers it");
        let off = WelcomePrefs { login_off: true, ..seen };
        assert!(!welcome_at_launch(off, Some((false, true))), "Start at Login off by choice");
        assert!(!finish_setup_offered(off, Some((false, true))));
        assert!(finish_setup_offered(off, Some((false, false))), "the apps step still waits");
        assert!(!finish_setup_offered(seen, None));
        assert_eq!(WelcomePrefs::from_json(&off.to_json()), off, "what is remembered reads back");
    }

    /// The page reads setup's events as the CLI's example has them, and the two steps the launch and
    /// the tray wait on come from the step rows, never from an app's row or the summary.
    #[test]
    fn setup_events_read_as_the_clis_example_has_them() {
        let example =
            include_str!("../../crates/supragnosis-cli/tests/fixtures/json/setup-events.jsonl");
        let events = setup_events(&output(example, "", 0)).expect("the example reads");
        assert_eq!(events.len(), example.lines().count() - 1, "every line after the schema");
        assert!(events.iter().any(|e| e["step"] == "first" && e["id"] == "billing"));
        // The example is a stream, a step's lines in the order they came; a checklist has each step
        // once, as it stands at the end.
        let mut checklist: Vec<serde_json::Value> = Vec::new();
        for e in events.iter().rev() {
            if !checklist.iter().any(|c| c["step"] == e["step"] && c.get("id") == e.get("id")) {
                checklist.push(e.clone());
            }
        }
        assert_eq!(essentials(&checklist), (true, false), "the daemon kept running, no app done");
        let first_seen: Vec<serde_json::Value> = events
            .iter()
            .filter(|e| e["step"] == "server" && e["state"] == "todo")
            .cloned()
            .collect();
        assert!(!essentials(&first_seen).0, "a server step that is not done");
    }

    /// A CLI before setup, one newer than the app, and one that failed each say so, and none is read.
    #[test]
    fn setup_events_say_why_there_are_none() {
        assert_eq!(
            setup_events(&output("", "error: unrecognized subcommand 'setup'", 2)),
            Err(SETUP_TOO_OLD.to_string())
        );
        let newer = setup_events(&output("{\"schema\": 2}\n{\"step\":\"server\"}\n", "", 0));
        assert!(newer.is_err_and(|e| e.contains("newer than the app")));
        assert_eq!(
            setup_events(&output("", "Error: the server profile could not be read: x", 1)),
            Err("the server profile could not be read: x".to_string())
        );
    }

    /// O4: the Welcome page's acts are setup's steps, the two helpers, asking setup again, and the
    /// choices the app remembers - every command it calls is one of those, and every command either
    /// page calls is one the app declares.
    #[test]
    fn the_welcome_page_calls_only_its_own_acts() {
        let called = |js: &str| -> Vec<String> {
            js.split("invoke(\"")
                .skip(1)
                .map(|rest| rest[..rest.find('"').expect("a closing quote")].to_string())
                .collect()
        };
        let welcome = called(include_str!("../assets/welcome.js"));
        let own = [
            "setup_check",
            "setup_run",
            "setup_stop",
            "setup_running",
            "welcome_state",
            "welcome_set",
            "app_helper",
            "app_toggle",
            "clipboard_copy",
            "diagnostics",
            // O4: the server step's Rebuild index, for a store indexed by another model.
            "search_rebuild",
        ];
        assert!(welcome.len() >= 8, "the page's calls were read: {welcome:?}");
        for cmd in &welcome {
            assert!(
                own.contains(&cmd.as_str()),
                "welcome.js calls {cmd}, which is not one of its acts"
            );
        }
        for cmd in welcome.iter().chain(&called(include_str!("../assets/settings.js"))) {
            assert!(APP_COMMANDS.contains(&cmd.as_str()), "{cmd} is called but not declared");
        }
    }
}

/// docs/compatibility.md Section 6, the app's side. The CLI's own tests hold its `--json` output
/// to one example document per command; these hold this app's readers to the same documents, so a
/// field read here that the CLI never sends fails here, and a field the CLI renames fails there.
#[cfg(test)]
mod window_tests {
    use super::{attention_section, frame_on_a_screen, WindowFrame, MIN_WINDOW};

    /// design-system.md Section 29.14: a kept frame opens where it was while its title bar is on a
    /// screen now, and is forgotten otherwise; the window has a smallest size that keeps the Graph's
    /// wide layout; the frame shares its file with the text size, written whole; the tray's Settings...
    /// opens the first section whose dot shows; and the sidebar has no heading for its one group,
    /// counts as the tray does, says no count it could not read, and reopens the last pane.
    #[test]
    fn the_window_and_the_sidebar_keep_their_place() {
        let screens = [(0.0, 0.0, 1512.0, 982.0), (1512.0, -200.0, 2560.0, 1440.0)];
        let at = |x, y| WindowFrame { x, y, w: 1280.0, h: 860.0 };
        assert!(frame_on_a_screen(&at(100.0, 50.0), &screens));
        assert!(frame_on_a_screen(&at(1700.0, -100.0), &screens), "the second screen");
        assert!(!frame_on_a_screen(&at(5000.0, 50.0), &screens), "a place on no screen");
        assert_eq!(MIN_WINDOW, (760.0, 480.0), "the Graph keeps its wide layout in the app");
        assert_eq!(attention_section(Some("daemon"), true, false), Some("apps"));
        assert_eq!(attention_section(None, false, true), Some("daemon"), "repairing");
        assert_eq!(attention_section(None, false, false), None, "the last pane");
        let src = super::SRC;
        assert!(src.contains(".min_inner_size(MIN_WINDOW.0, MIN_WINDOW.1)"));
        assert!(src.contains("Some(f) => builder.inner_size(f.w, f.h).position(f.x, f.y),"));
        assert!(src.contains("None => builder.inner_size(1280.0, 860.0).center(),"));
        assert!(
            src.contains("keep_view_pref(app, \"text_scale\""),
            "the text size overwrites the frame"
        );
        assert!(
            src.contains("let tmp = path.with_extension(\"json.tmp\");")
                && src.contains("std::fs::rename(&tmp, &path)"),
            "a write cut short can lose both preferences"
        );
        let js = include_str!("../assets/settings.js");
        assert!(!js.contains("text: \"General\""), "the sidebar still heads its one group");
        assert!(js.contains("text: appsWithEntry() + \" connected\""), "the count says no unit");
        assert!(
            js.contains("a.kind === \"connected\" || a.kind === \"attention\""),
            "the count is not the tray's"
        );
        assert!(
            js.contains("if (!appsRead()) return \"warn\";") && js.contains("state && appsRead()"),
            "an unread listing counts as zero"
        );
        assert!(
            js.contains("!state.apps.some((a) => a.kind === \"attention\")"),
            "a row that needs the person leaves no dot"
        );
        assert!(js.contains("localStorage.getItem(LAST_PANE)") && js.contains("keepPane();"));
        let css = include_str!("../assets/settings.css");
        assert!(
            css.contains("width: clamp(220px, 13.75rem, 40vw);"),
            "the sidebar does not grow with the text"
        );
        assert!(
            css.contains("padding: 12vh 0 24px;\n  overflow-y: auto;"),
            "a tall sheet cannot be scrolled"
        );
    }
}

#[cfg(test)]
mod tray_tests {
    use super::{daemon_trouble, key_js, map_items_enabled, press_js, tray_act, Daemon};

    /// design-system.md Section 29.13 (D14, the author's list): the trouble shape shows while the
    /// memory cannot be used - could not start, stopped, two managers, the index - and not while it
    /// starts, an update waits or the state is unknown.
    #[test]
    fn trouble_is_the_memory_that_cannot_be_used() {
        let st = |answering: bool, running: Option<&str>, situation: &str| {
            serde_json::json!({ "answering": answering, "situation": situation,
                "version": { "here": "0.8.2", "running": running } })
        };
        assert!(daemon_trouble(&Daemon::Failed("x".into()), None));
        assert!(daemon_trouble(&Daemon::External, Some(&st(false, None, "one"))), "stopped");
        assert!(daemon_trouble(&Daemon::External, Some(&st(true, Some("0.8.2"), "conflict"))));
        let mut index = st(false, None, "one");
        index["embedder_conflict"] = serde_json::json!({ "recorded": "a", "daemon": "b" });
        assert!(daemon_trouble(&Daemon::External, Some(&index)), "the index needs rebuilding");
        assert!(!daemon_trouble(&Daemon::Starting, None));
        assert!(!daemon_trouble(&Daemon::External, None), "unknown is not trouble");
        assert!(
            !daemon_trouble(&Daemon::External, Some(&st(true, Some("0.8.1"), "one"))),
            "an update is not trouble"
        );
    }

    /// One act line, the first that holds: the memory cannot be used, an update, then no AI app
    /// connected unless Finish Setup... shows.
    #[test]
    fn the_tray_has_one_act_in_order() {
        assert_eq!(tray_act(true, true, true, false), Some(("Open This Mac...", "daemon")));
        assert_eq!(tray_act(false, true, true, false), Some(("Open This Mac...", "daemon")));
        assert_eq!(tray_act(false, false, true, false), Some(("Connect an AI App...", "apps")));
        assert_eq!(tray_act(false, false, true, true), None, "Finish Setup... covers it");
    }

    /// The map commands act where their control is, open the Graph from another page, and Back and
    /// Forward follow the pages that read them; the shell presses only when the press changes
    /// something and no dialog or card is open.
    #[test]
    fn the_map_commands_act_where_their_control_is() {
        let all: Vec<String> =
            ["panelL", "panelR", "mapTab", "listBtn", "fit", "keysOpen", "keysDlg"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let on = |url: &str, c: &[String]| -> Vec<&'static str> {
            map_items_enabled(url, c)
                .into_iter()
                .filter(|(_, e)| *e)
                .map(|(id, _)| id)
                .collect()
        };
        assert_eq!(on("viz://localhost/?workspace=w", &all).len(), 9, "all on the Graph");
        let old = on(
            "viz://localhost/",
            &["panelL".into(), "panelR".into(), "listBtn".into(), "fit".into()],
        );
        assert!(
            !old.contains(&"view-map")
                && !old.contains(&"view-back")
                && !old.contains(&"view-keys"),
            "{old:?}"
        );
        let review = on("viz://localhost/#review", &all);
        assert!(review.contains(&"view-fit") && !review.contains(&"view-forward"));
        let settings = on("tauri://localhost/settings.html", &[]);
        assert!(
            settings.contains(&"view-back")
                && !settings.contains(&"view-forward")
                && settings.contains(&"view-list")
        );
        assert!(on("tauri://localhost/starting", &[]).is_empty(), "nothing on the starting page");
        let tab = press_js("mapTab", true);
        assert!(
            tab.contains("dialog[open]")
                && tab.contains("#obscard.on")
                && tab.contains("aria-selected")
        );
        assert!(!press_js("fit", false).contains("aria-selected"));
        assert!(key_js("[", true).contains("keysDlg") && !key_js("k", false).contains("keysDlg"));
    }

    /// One id per item per menu: the tray's ids are its own, so a press does only its own act; and
    /// the menu bar holds Setup..., Find, the map's commands and Help.
    #[test]
    fn each_menu_item_has_its_own_id() {
        let src = super::SRC;
        let tray = &src[src.find("TrayIconBuilder::with_id(\"supragnosis\")").expect("the tray")..];
        let tray = &tray[..tray.find(".build(app)?;").unwrap()];
        for arm in [
            "\"tray-open\"",
            "\"tray-review\"",
            "\"tray-finish-setup\"",
            "\"tray-act\" | \"tray-settings\"",
            "\"tray-quit\"",
        ] {
            assert!(tray.contains(arm), "the tray lacks {arm}");
        }
        assert!(
            !tray.contains("\"review\" =>") && !tray.contains("\"settings\" =>"),
            "the tray hears the menu bar's ids"
        );
        for id in [
            "\"setup\"",
            "\"edit-find\"",
            "\"view-filters\"",
            "\"view-back\"",
            "\"help-docs\"",
            "\"help-diagnostics\", \"Diagnostics...\"",
            "\"help-issue\"",
        ] {
            assert!(src.contains(id), "the menu bar lacks {id}");
        }
        assert!(
            src.contains("\"Quit and Stop Memory\".to_string()"),
            "Quit does not say it stops the memory"
        );
        assert!(
            src.contains("tray.quit_stops = matches!(*daemon, Daemon::Spawned(_));"),
            "Quit's words follow the wrong state"
        );
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("controls: [\"panelL\", \"panelR\", \"mapTab\", \"listBtn\", \"fit\", \"keysOpen\", \"keysDlg\"]"));
    }
}

#[cfg(test)]
mod cli_contract_tests {
    use super::{app_row, cli_json, status_text, who_runs, Daemon, CLI_SCHEMA};

    const STATUS: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/status.json");
    const CONNECT: &str =
        include_str!("../../crates/supragnosis-cli/tests/fixtures/json/connect.json");

    fn read(doc: &str) -> serde_json::Value {
        cli_json(doc.as_bytes()).expect("a schema this app reads").expect("JSON")
    }

    /// client-connect.md Section 13.4: every app the CLI's example lists - the CLI holds the
    /// example to every app it knows - has a name in the menu and a name and an icon on both pages,
    /// from kit.js (design-system.md Section 6a).
    #[test]
    fn every_app_page_names_the_apps_connect_lists() {
        let kit = include_str!("../assets/kit.js");
        let table = |name: &str| -> &str {
            let start = kit.find(&format!("const {name} = {{")).unwrap_or_else(|| panic!("{name}"));
            &kit[start..start + kit[start..].find("};").expect("closed")]
        };
        let ids: Vec<String> = read(CONNECT)["clients"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect();
        assert!(ids.iter().any(|i| i == "opencode"), "{ids:?}");
        for id in &ids {
            let key = |t: &str| t.contains(&format!("\"{id}\":")) || t.contains(&format!(" {id}:"));
            assert!(key(table("APPS")), "kit.js names no {id}");
            assert!(key(table("APP_KIND")), "kit.js gives {id} no icon");
            assert!(super::AI_APPS.iter().any(|(i, _)| i == id), "the menu lacks {id}");
        }
    }

    /// Every field this app reads from a CLI answer, by JSON pointer. A reader that starts using
    /// another field adds it here, and the example has to carry it.
    #[test]
    fn every_field_the_app_reads_is_in_the_clis_examples() {
        let reads: [(&str, &[&str]); 2] = [
            (
                STATUS,
                &[
                    "/situation",
                    "/managers/1/type",
                    "/managers/1/source",
                    "/managers/1/label",
                    "/answering",
                    "/version/here",
                    "/version/running",
                    "/store/owed_projections",
                    "/store/last_recovery",
                    "/service/state",
                ],
            ),
            (
                CONNECT,
                &[
                    "/clients/0/id",
                    "/clients/0/installed",
                    "/clients/0/entry",
                    "/clients/1/capture",
                ],
            ),
        ];
        for (doc, pointers) in reads {
            let v = read(doc);
            for p in pointers {
                assert!(v.pointer(p).is_some(), "the app reads {p}, which the example lacks");
            }
        }
    }

    /// The examples go through the same readers a live answer does, and read as they should.
    #[test]
    fn the_examples_read_through_the_apps_own_readers() {
        let st = read(STATUS);
        assert!(status_text(&Daemon::External, Some(&st)).starts_with("Two managers"));
        let mut one = st.clone();
        one["situation"] = "one".into();
        one["managers"] = serde_json::json!([st["managers"][1].clone()]);
        assert_eq!(who_runs(&Daemon::External, &one), "starts at login");
        let line = status_text(&Daemon::External, Some(&one));
        assert_eq!(line, "Update ready - restart: 0.4.7 running, 0.4.8 installed");

        let apps = read(CONNECT);
        let kinds: Vec<_> = apps["clients"]
            .as_array()
            .expect("clients")
            .iter()
            .map(|r| app_row(Some(r)).2)
            .collect();
        for kind in ["connected", "attention", "off", "absent"] {
            assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
        }
    }

    /// A schema newer than this app reads is not read at all; an answer without one is a CLI from
    /// before the contract, and is read as before.
    #[test]
    fn a_newer_schema_is_not_read_and_an_older_cli_still_is() {
        let newer = format!(r#"{{"schema": {}, "situation": "one"}}"#, CLI_SCHEMA + 1);
        assert_eq!(cli_json(newer.as_bytes()), Err(CLI_SCHEMA + 1));
        assert!(cli_json(br#"{"situation": "one"}"#).expect("read").is_some());
        assert_eq!(cli_json(b"not json"), Ok(None));
    }
}
