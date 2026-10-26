//! The shell's tests: the transport, the status line and the CLI's answers, the Welcome page, the
//! window, the tray, and the source tripwires that read the app's own source through `SRC`.

use super::*;

/// The app on Tauri's mock runtime with a recorder in place of its CLI (cli.rs's `Cli`): every call
/// the app makes to the CLI is written down, as its words joined by spaces, and answered as the
/// test's [`Scene`] says. The recorder starts no process but a shell that prints what a streamed run
/// is to print, and no daemon: its `serve` is written down and refused. Nothing reaches this Mac's
/// daemon, its login job or the person's settings: the app's directories and the viewer socket are
/// in a directory of the test's own.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Arc;

    /// `usage --json` as the CLI's example has it, the shape its contract test holds.
    const USAGE: &str = include_str!("../../crates/supragnosis-cli/tests/fixtures/json/usage.json");

    /// What the recorder answers. `status --json` says the login job's state (`service`) and the
    /// managers it names; `connect --json` lists no app; `usage --json` is the CLI's example;
    /// setup's checklist has no step. Any other
    /// call succeeds, or is refused with `refuse`, on stderr as the CLI writes a refusal.
    #[derive(Clone)]
    pub(crate) struct Scene {
        pub(crate) service: &'static str,
        pub(crate) managers: serde_json::Value,
        pub(crate) running: &'static str,
        pub(crate) refuse: Option<&'static str>,
    }

    /// A login job runs the daemon (`service install`'s, the canonical LaunchAgent).
    pub(crate) fn login_job() -> Scene {
        Scene {
            service: "generated",
            managers: serde_json::json!([{ "type": "launchd", "source": "canonical", "pid": 4242 }]),
            running: "0.10.1",
            refuse: None,
        }
    }

    /// Homebrew's service runs the daemon, and no login job of the app's is installed.
    pub(crate) fn homebrew() -> Scene {
        Scene {
            service: "absent",
            managers: serde_json::json!([{ "type": "launchd", "source": "homebrew", "pid": 4343 }]),
            ..login_job()
        }
    }

    pub(crate) fn output(code: i32, stdout: &str, stderr: &str) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    /// What a test runs at each call, with its words.
    pub(crate) type AtCall = Box<dyn Fn(&str) + Send + Sync>;

    pub(crate) struct Recorder {
        scene: Scene,
        calls: Mutex<Vec<String>>,
        /// Run at each call, with its words, before it is answered.
        pub(crate) at_call: Mutex<Option<AtCall>>,
    }

    impl Recorder {
        /// The calls since the last `take`, in order.
        pub(crate) fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }

        fn record(&self, call: String) {
            if let Some(f) = &*self.at_call.lock().unwrap() {
                f(&call);
            }
            self.calls.lock().unwrap().push(call);
        }

        fn answer(&self, args: &[&str]) -> std::process::Output {
            let s = &self.scene;
            match args {
                ["status", "--json"] => output(
                    0,
                    &serde_json::json!({
                        "schema": 1,
                        "situation": if s.managers.as_array().map_or(0, Vec::len) > 1 { "conflict" } else { "one" },
                        "managers": s.managers,
                        "service": { "state": s.service },
                        "answering": true,
                        "version": { "running": s.running, "here": "0.10.1" },
                    })
                    .to_string(),
                    "",
                ),
                ["connect", "--json"] => output(0, r#"{"schema":1,"clients":[]}"#, ""),
                ["usage", "--json", "--since", _] => output(0, USAGE, ""),
                ["setup", "--check", ..] => output(0, "{\"schema\":1}\n", ""),
                _ => match s.refuse {
                    Some(why) => output(1, "", &format!("\nError: {why}\n")),
                    None => output(0, "done\n", ""),
                },
            }
        }
    }

    impl Cli for Recorder {
        fn run(&self, args: &[&str]) -> Option<std::process::Output> {
            self.record(args.join(" "));
            Some(self.answer(args))
        }

        fn stream(&self, args: &[String]) -> Result<Child, String> {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            self.record(args.join(" "));
            let o = self.answer(&args);
            Command::new("/bin/sh")
                .args(["-c", "printf '%s' \"$1\"; printf '%s' \"$2\" >&2; exit \"$3\"", "sh"])
                .arg(String::from_utf8_lossy(&o.stdout).as_ref())
                .arg(String::from_utf8_lossy(&o.stderr).as_ref())
                .arg(o.status.code().unwrap_or(1).to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())
        }

        fn serve(&self, _sock: &Path) -> anyhow::Result<(Child, PathBuf)> {
            self.record("serve".into());
            anyhow::bail!("the recorder starts no daemon")
        }
    }

    /// The app, held for a test: its main window shows the Settings page.
    pub(crate) struct Held {
        pub(crate) app: tauri::App<Rt>,
        pub(crate) cli: Arc<Recorder>,
        pub(crate) sock: PathBuf,
        dir: PathBuf,
    }

    pub(crate) fn app(daemon: Daemon, scene: Scene) -> Held {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sg-app-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("viz.sock");
        let cli =
            Arc::new(Recorder { scene, calls: Mutex::new(Vec::new()), at_call: Mutex::new(None) });
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().app.app_directories_override =
            Some(tauri::utils::config::AppDirectoriesOverride::Root(dir.clone()));
        let app = tauri::test::mock_builder()
            .manage(DaemonGuard(Mutex::new(daemon)))
            .manage(RunningSteps(Mutex::new(std::collections::HashMap::new())))
            .manage(VizSock(sock.clone()))
            .manage(CliHeld(cli.clone()))
            .build(context)
            .unwrap();
        WebviewWindowBuilder::new(&app, "main", WebviewUrl::External(settings_url()))
            .build()
            .unwrap();
        Held { app, cli, sock, dir }
    }

    impl Held {
        pub(crate) fn handle(&self) -> tauri::AppHandle<Rt> {
            self.app.handle().clone()
        }

        /// The main window on the Settings page, as a click there calls a command.
        pub(crate) fn page(&self) -> tauri::Webview<Rt> {
            let w = self.app.get_webview_window("main").unwrap();
            w.navigate(settings_url()).unwrap();
            w.as_ref().clone()
        }

        /// The viewer socket answers from now on: a daemon is running. Each connection is closed at
        /// once, so a read of the socket fails as fast as the attach succeeds.
        pub(crate) fn answering(&self) {
            serve_socket(&self.sock);
        }

        pub(crate) fn daemon(&self) -> std::sync::MutexGuard<'_, Daemon> {
            self.app.state::<DaemonGuard>().inner().0.lock().unwrap()
        }

        pub(crate) fn run<T>(&self, f: impl std::future::Future<Output = T>) -> T {
            tauri::async_runtime::block_on(f)
        }

        /// Runs an act that returns before its work is done - a press, the exit - and waits until
        /// every task it spawned on the app's runtime has ended, so the recorder is read after all
        /// of it. A task is counted from its spawn; the other tests spawn none on that runtime.
        pub(crate) fn settled(&self, act: impl FnOnce()) {
            let metrics = tauri::async_runtime::handle().inner().metrics();
            let before = metrics.num_alive_tasks();
            act();
            let until = std::time::Instant::now() + Duration::from_secs(60);
            while metrics.num_alive_tasks() > before {
                assert!(std::time::Instant::now() < until, "the act's work did not end");
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    pub(crate) fn serve_socket(sock: &Path) {
        let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || listener.incoming().for_each(drop));
    }

    impl Drop for Held {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

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
        app_row, app_toggle, capture_set, cli_change, cli_outcome, cli_path, daemon_restart,
        diagnostics, login_env_from_output, login_outcome, login_set, menu_pressed, peer_narrow,
        read_login_env, refresh_status, search_rebuild, settings_state, setup_check, setup_run,
        setup_running, setup_stop, splash_beat, start_up, starting_page, status_text, tray_pressed,
        usage_read, welcome_set, welcome_state, write_prefs, Daemon, TrayPress, WelcomePrefs,
        MENU_ITEMS, TRAY_IDS,
    };
    use std::process::Command;
    use std::sync::Mutex;
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

        // The switch's command, against a CLI that refuses: the reason reaches the page as it is,
        // never re-wrapped as a success, turning it on and off.
        for (on, scene) in [(true, super::fake::homebrew()), (false, super::fake::login_job())] {
            let held = super::fake::app(
                Daemon::External,
                super::fake::Scene { refuse: Some(why), ..scene },
            );
            held.answering();
            let got = held.run(super::login_set(held.page(), held.handle(), on));
            assert_eq!(got, Err(why.to_string()), "turning it {}", if on { "on" } else { "off" });
        }
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
    /// asks for it. The app runs here against a recorder in place of the CLI, with a daemon a login
    /// job runs, one Homebrew's service runs, one whose update is ready and two managers at once:
    /// the start, the status beat, the starting page and its watch's beat in each state the shell
    /// can hold, every tray item (Quit's end is the exit, `quitting_leaves_a_daemon_it_did_not_spawn`),
    /// every menu bar item, and every command of the app's pages but the three that act on the
    /// desktop and hold no CLI. Not driven: the window's own events (close, move, resize) and the
    /// viewer's transport (the proxy's other requests, the event bridge).
    /// A call that stops a daemon - `stop`, `restart`, `service uninstall`, a `--take-over`, `embed
    /// rebuild`, setup's server step - comes only from the Start at Login switch, Setup's Set up and
    /// Take over, and Restart and Rebuild index, which bring it back, each the one its click asks
    /// for. Everything else only reads (`status`, `connect`, setup's checklist) or starts the app's
    /// own daemon. A beat that restarts a daemon whose update is ready, a start that takes over
    /// Homebrew's service, a tray or menu item that restarts rather than opening This Mac, or a
    /// command that does more than its click asks fails this, the work a press spawns included.
    #[test]
    fn only_the_persons_act_stops_a_daemon_it_did_not_spawn() {
        use super::fake::{self, Scene};
        let stops = |c: &str| {
            let w: Vec<&str> = c.split(' ').collect();
            match w[0] {
                "stop" | "restart" => true,
                "service" => w.contains(&"uninstall") || w.contains(&"--take-over"),
                "embed" => w.get(1) == Some(&"rebuild"),
                "setup" => {
                    !w.contains(&"--check")
                        && !w.contains(&"model")
                        && !w.iter().any(|a| a.starts_with("app="))
                }
                _ => false,
            }
        };
        // What a way in asked of the CLI: every call but a read and the app's own `serve`.
        let acts = |calls: Vec<String>| -> Vec<String> {
            calls
                .into_iter()
                .filter(|c| {
                    !matches!(c.as_str(), "status --json" | "connect --json" | "serve")
                        && !c.starts_with("setup --check --events")
                        && !c.starts_with("usage --json")
                })
                .collect()
        };
        let none: Vec<String> = Vec::new();
        let failed = || Daemon::Failed("the daemon exited".into());
        let two = serde_json::json!([
            { "type": "launchd", "source": "canonical", "pid": 1 },
            { "type": "launchd", "source": "homebrew", "pid": 2 },
        ]);
        let scenes = [
            ("a login job", fake::login_job()),
            ("Homebrew's service", fake::homebrew()),
            ("an update ready", Scene { running: "0.9.0", ..fake::login_job() }),
            ("two managers", Scene { managers: two, ..fake::login_job() }),
        ];

        // Nobody's act: the start, the beat, the starting page and the tray.
        for (name, scene) in &scenes {
            for answers in [true, false] {
                let held = fake::app(Daemon::Starting, scene.clone());
                if answers {
                    held.answering();
                }
                let h = held.handle();
                // A launch after the first, so the start asks setup whether a step waits.
                write_prefs(&h, WelcomePrefs { seen: true, ..WelcomePrefs::default() }).unwrap();
                held.run(start_up(h.clone(), held.sock.clone(), false));
                // The reads are heard, so no act is not a recorder that heard nothing.
                let calls = held.cli.take();
                assert!(calls.iter().any(|c| c.starts_with("setup --check")), "{calls:?}");
                assert_eq!(acts(calls), none, "the start, with {name}");
                for daemon in [Daemon::External, failed(), Daemon::Starting] {
                    *held.daemon() = daemon;
                    held.run(refresh_status(&h));
                    let (_, drawn) = held.run(starting_page(&h));
                    held.run(splash_beat(&h, &held.sock, &drawn));
                    let opened = Mutex::new(Vec::new());
                    held.settled(|| {
                        for (_, press) in TRAY_IDS.iter().filter(|(_, p)| *p != TrayPress::Quit) {
                            tray_pressed(&h, *press);
                        }
                        for (id, ..) in MENU_ITEMS {
                            menu_pressed(&h, id, |url| {
                                opened.lock().unwrap().push(url.to_string());
                                Ok(())
                            });
                        }
                    });
                    let repo = env!("CARGO_PKG_REPOSITORY");
                    assert_eq!(
                        *opened.lock().unwrap(),
                        [format!("{repo}#readme"), format!("{repo}/issues")],
                        "the menu opens only the project's addresses"
                    );
                    let calls = held.cli.take();
                    assert!(calls.iter().any(|c| c == "status --json"), "{calls:?}");
                    assert_eq!(
                        acts(calls),
                        none,
                        "the beat, the starting page or the tray, {name}"
                    );
                }
            }
        }

        // The pages' commands, each against a daemon it did not spawn: what each asks of the CLI.
        let run = |scene: Scene, daemon: Daemon, command: &str| -> Vec<String> {
            let held = fake::app(daemon, scene);
            held.answering();
            let (h, page) = (held.handle(), held.page());
            held.run(async {
                let _ = match command {
                    "settings_state" => settings_state(page, h).await.map(drop),
                    "setup_check" => setup_check(page, h).await.map(drop),
                    "setup_running" => setup_running(page, h).await.map(drop),
                    "welcome_state" => welcome_state(page, h).await.map(drop),
                    "welcome_set" => welcome_set(page, h, Some(true), Some(true)).await.map(drop),
                    "diagnostics" => diagnostics(page, h).await.map(drop),
                    "usage_read" => usage_read(page, h, "7d".into()).await.map(drop),
                    "peer_narrow" => peer_narrow(page, h, "peer".into(), vec![]).await.map(drop),
                    "setup_stop" => setup_stop(page, h, "model".into()).await.map(drop),
                    "app_toggle" => app_toggle(page, h, "claude-code".into()).await.map(drop),
                    "capture_set" => capture_set(page, h, false).await.map(drop),
                    "login_set off" => login_set(page, h, false).await.map(drop),
                    "login_set on" => login_set(page, h, true).await.map(drop),
                    "daemon_restart" => daemon_restart(page, h).await.map(drop),
                    "search_rebuild" => search_rebuild(page, h).await.map(drop),
                    other => {
                        let step = other.strip_prefix("setup_run ").expect("a page's command");
                        setup_run(page, h, step.into(), None).await.map(drop)
                    }
                };
            });
            acts(held.cli.take())
        };
        // Each command, the scene it runs in, and what it asks of the CLI.
        type Case = (&'static str, fn() -> Scene, &'static [&'static str]);
        let commands: &[Case] = &[
            ("settings_state", fake::login_job, &[]),
            ("setup_check", fake::login_job, &[]),
            ("setup_running", fake::login_job, &[]),
            ("welcome_state", fake::login_job, &[]),
            ("welcome_set", fake::login_job, &[]),
            ("diagnostics", fake::login_job, &[]),
            ("usage_read", fake::login_job, &[]),
            ("peer_narrow", fake::login_job, &[]),
            ("setup_stop", fake::login_job, &[]),
            ("app_toggle", fake::login_job, &["connect claude-code --replace"]),
            ("capture_set", fake::login_job, &["connect claude-code --no-capture"]),
            ("setup_run model", fake::login_job, &["setup --step model --events"]),
            (
                "setup_run app=claude-code",
                fake::login_job,
                &["setup --step app=claude-code --events"],
            ),
            ("login_set off", fake::login_job, &["service uninstall"]),
            ("login_set on", fake::homebrew, &["service install --take-over"]),
            ("daemon_restart", fake::homebrew, &["restart"]),
            ("search_rebuild", fake::login_job, &["embed rebuild"]),
            ("setup_run server", fake::homebrew, &["setup --step server --events"]),
            (
                "setup_run takeover",
                fake::homebrew,
                &["service install --take-over --no-model"],
            ),
        ];
        let mut stoppers: Vec<&str> = Vec::new();
        for (command, scene, want) in commands {
            for daemon in [Daemon::External, failed()] {
                assert_eq!(run(scene(), daemon, command), *want, "{command}");
            }
            if want.iter().any(|c| stops(c)) {
                stoppers.push(command);
            }
        }
        let persons = [
            "login_set off",
            "login_set on",
            "daemon_restart",
            "search_rebuild",
            "setup_run server",
            "setup_run takeover",
        ];
        assert_eq!(stoppers, persons, "the acts that stop a daemon, each with what its click asks");
        // The daemon this app spawned is its own to stop: a restart does that itself.
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        assert_eq!(run(fake::login_job(), Daemon::Spawned(child), "daemon_restart"), none);

        // Every command the pages call ran here, but the clipboard's, Finder's and the other apps'
        // helpers, which act on the desktop and hold no CLI.
        let mut ran: Vec<&str> = commands
            .iter()
            .map(|(c, ..)| c.split(' ').next().unwrap())
            .chain(["clipboard_copy", "reveal", "app_helper"])
            .collect();
        ran.sort();
        ran.dedup();
        let mut all = crate::APP_COMMANDS.to_vec();
        all.sort();
        assert_eq!(ran, all, "a command of the pages is not run here");
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

        // The exit itself, as the tray's Quit and the platform's end the app, with a recorder in
        // place of the CLI: it asks the CLI nothing; and the child it spawned is gone after it.
        for daemon in [Daemon::External, Daemon::Failed("x".into()), Daemon::Starting] {
            let held = super::fake::app(daemon, super::fake::login_job());
            held.settled(|| super::on_exit(&held.handle()));
            assert_eq!(held.cli.take(), Vec::<String>::new(), "the exit runs the CLI");
        }
        let child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id().to_string();
        let held = super::fake::app(Daemon::Spawned(child), super::fake::login_job());
        held.settled(|| super::on_exit(&held.handle()));
        assert_eq!(held.cli.take(), Vec::<String>::new(), "the exit runs the CLI");
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!alive.success(), "the app's own daemon outlives the app");
    }

    /// settings-page.md S3: the page decides nothing the CLI or the daemon decides. Every command
    /// the Settings page calls is in one list here, by what it is. A change runs the CLI, and
    /// against a CLI that refuses it, the refusal is the command's error carrying what the CLI said,
    /// which the page shows - setup's run says it as its `why`. Stopping a peer's sharing is the one
    /// change that goes to the daemon's narrow-only endpoint instead, and runs no CLI. The rest read,
    /// or act on the system rather than a setting. A command the page starts calling fails this
    /// until it is placed.
    #[test]
    fn every_setting_change_is_a_cli_call() {
        let changes = [
            "app_toggle",
            "capture_set",
            "login_set",
            "daemon_restart",
            "search_rebuild",
            "setup_run",
        ];
        let reads = ["settings_state", "setup_running", "diagnostics", "usage_read"];
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
        let mut placed: Vec<&str> = changes.to_vec();
        placed.extend(reads);
        placed.extend(system);
        placed.push("peer_narrow");
        placed.sort();
        assert_eq!(called, placed, "every command the page calls, placed by what it is");

        use super::fake;
        let why = "the CLI refused this on purpose";
        let read = |c: &String| c == "status --json" || c == "connect --json";
        for change in changes {
            // The switch turns Start at Login on here, where no login job is installed.
            let scene = if change == "login_set" { fake::homebrew() } else { fake::login_job() };
            let held = fake::app(Daemon::External, fake::Scene { refuse: Some(why), ..scene });
            held.answering();
            let (h, page) = (held.handle(), held.page());
            let got = held.run(async {
                match change {
                    "app_toggle" => app_toggle(page, h, "claude-code".into()).await,
                    "capture_set" => capture_set(page, h, true).await,
                    "login_set" => login_set(page, h, true).await,
                    "daemon_restart" => daemon_restart(page, h).await,
                    "search_rebuild" => search_rebuild(page, h).await,
                    _ => setup_run(page, h, "model".into(), None).await.and_then(|o| {
                        match (o["ok"].as_bool(), o["why"].as_str()) {
                            (Some(false), Some(w)) => Err(w.to_string()),
                            _ => Ok(o.to_string()),
                        }
                    }),
                }
            });
            let err = got.expect_err(&format!("{change} says it was done"));
            assert!(err.contains(why), "{change} drops what the CLI said: {err}");
            assert!(!held.cli.take().iter().all(read), "{change} does not run the CLI");
        }
        let held = fake::app(Daemon::External, fake::login_job());
        let narrowed = held.run(peer_narrow(held.page(), held.handle(), "peer".into(), vec![]));
        assert!(narrowed.is_err(), "no daemon answers here");
        assert_eq!(held.cli.take(), Vec::<String>::new(), "stopping a peer's sharing runs the CLI");
    }

    /// settings-page.md Section 3.6: the Usage pane reads `usage --json` for 7, 30 or 90 days and
    /// gets the CLI's summary back as it was counted; any other window is refused before the CLI
    /// runs, so the page cannot ask the CLI for anything else through it.
    #[test]
    fn the_usage_pane_reads_only_its_three_windows() {
        use super::fake;
        for since in ["7d", "30d", "90d"] {
            let held = fake::app(Daemon::External, fake::login_job());
            let got = held.run(usage_read(held.page(), held.handle(), since.into()));
            let sum = got.unwrap_or_else(|e| panic!("{since}: {e}"));
            assert_eq!(sum["by_hour"][0]["writes"], 1, "the CLI's counts, as they came");
            assert_eq!(held.cli.take(), [format!("usage --json --since {since}")]);
        }
        for since in ["1y", "7d --json", "", "--since"] {
            let held = fake::app(Daemon::External, fake::login_job());
            let got = held.run(usage_read(held.page(), held.handle(), since.into()));
            assert!(got.is_err(), "{since:?} was read");
            assert_eq!(held.cli.take(), Vec::<String>::new(), "{since:?} ran the CLI");
        }
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
    use super::{
        attention_section, frame_on_a_screen, initial_frame, keep_view_pref_at, text_scale_of,
        WindowFrame, MIN_WINDOW,
    };

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

        // The window opens where it was kept, no smaller than the smallest window and cut to the
        // screen it opens on; with nothing kept, or a place on no screen, centred (None).
        let kept = |x: f64, y: f64, w: f64, h: f64| serde_json::json!({ "window": { "x": x, "y": y, "w": w, "h": h } });
        let frame = |x, y, w, h| Some(WindowFrame { x, y, w, h });
        assert_eq!(
            initial_frame(&kept(100.0, 50.0, 1280.0, 860.0), &screens),
            frame(100.0, 50.0, 1280.0, 860.0)
        );
        assert_eq!(
            initial_frame(&kept(100.0, 50.0, 300.0, 200.0), &screens),
            frame(100.0, 50.0, MIN_WINDOW.0, MIN_WINDOW.1),
            "a frame under the smallest window"
        );
        assert_eq!(
            initial_frame(&kept(1600.0, 0.0, 4000.0, 3000.0), &screens),
            frame(1600.0, 0.0, 2560.0, 1440.0),
            "a frame larger than its screen"
        );
        assert_eq!(initial_frame(&kept(5000.0, 50.0, 1280.0, 860.0), &screens), None);
        assert_eq!(initial_frame(&serde_json::json!({ "text_scale": 1.5 }), &screens), None);
        let cut = serde_json::json!({ "window": { "x": 1.0, "y": 2.0, "w": 900.0 } });
        assert_eq!(initial_frame(&cut, &screens), None, "a frame cut short");

        // The frame and the text size share one file: each is written beside the other, and the
        // file is replaced whole, never written over, so a write cut short loses neither.
        let dir = std::env::temp_dir().join(format!("sg-view-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config").join("view.json");
        let window = serde_json::json!({ "x": 100.0, "y": 50.0, "w": 1280.0, "h": 860.0 });
        keep_view_pref_at(&path, "window", window).expect("written, its directory made");
        let before = std::fs::File::open(&path).expect("the file as it was");
        keep_view_pref_at(&path, "text_scale", serde_json::json!(1.25)).expect("written");
        let read = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("whole")
        };
        assert_eq!(text_scale_of(&read()), 1.25);
        assert_eq!(initial_frame(&read(), &screens), frame(100.0, 50.0, 1280.0, 860.0));
        let mut old = String::new();
        std::io::Read::read_to_string(&mut &before, &mut old).unwrap();
        let old: serde_json::Value = serde_json::from_str(&old).expect("the old file is whole");
        assert!(old["text_scale"].is_null() && old["window"].is_object(), "written over: {old}");
        assert!(!path.with_extension("json.tmp").exists(), "the temporary file is left");
        std::fs::write(&path, "{\"window\": ").unwrap();
        keep_view_pref_at(&path, "text_scale", serde_json::json!(9.0)).unwrap();
        assert_eq!(text_scale_of(&read()), 2.0, "a broken file is replaced; the size held to 200%");
        assert_eq!(text_scale_of(&serde_json::json!({})), 1.0);
        let _ = std::fs::remove_dir_all(&dir);

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
    use super::{
        daemon_trouble, key_js, map_items_enabled, menu_item, press_js, quit_label, quit_stops,
        shell_init_script, tray_act, tray_press, Daemon, MenuPress, TrayPress, MAP_CONTROLS,
        MENU_ITEMS, TRAY_IDS,
    };

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

    /// One id per item per menu: every press reaches both listeners, and each acts only on its own
    /// ids, so the tray's and the menu bar's are apart and a press does only its own act; the menu
    /// bar holds Setup..., Find, the map's commands and Help; Quit says it stops the memory while
    /// the daemon is the app's own child; and the page reports the controls the map commands press.
    #[test]
    fn each_menu_item_has_its_own_id() {
        let menu: Vec<&str> = MENU_ITEMS.iter().map(|m| m.0).collect();
        let tray: Vec<&str> = TRAY_IDS.iter().map(|(id, _)| *id).collect();
        for ids in [&menu, &tray] {
            let mut once = ids.clone();
            once.sort();
            once.dedup();
            assert_eq!(once.len(), ids.len(), "an id names two items: {ids:?}");
        }
        for id in &menu {
            assert_eq!(tray_press(id), None, "the tray hears the menu bar's {id}");
        }
        for id in &tray {
            assert_eq!(menu_item(id), None, "the menu bar hears the tray's {id}");
            assert!(tray_press(id).is_some(), "{id} does nothing");
        }
        assert_eq!(
            tray_press("tray-act"),
            Some(TrayPress::Settings),
            "the act line opens Settings"
        );
        assert_eq!(tray_press("tray-quit"), Some(TrayPress::Quit));
        for (id, words) in [
            ("setup", "Setup..."),
            ("edit-find", "Find"),
            ("view-filters", "Filters Panel"),
            ("view-back", "Back"),
            ("help-docs", "Supragnosis Documentation"),
            ("help-diagnostics", "Diagnostics..."),
            ("help-issue", "Report an Issue..."),
        ] {
            assert_eq!(menu_item(id).map(|m| m.1), Some(words), "the menu bar lacks {id}");
        }
        // What each item does, so two items that trade acts fail here.
        for (id, press) in [
            ("settings", MenuPress::Settings),
            ("setup", MenuPress::Setup),
            ("graph", MenuPress::Graph),
            ("review", MenuPress::Review),
            ("help-docs", MenuPress::Docs),
            ("help-diagnostics", MenuPress::Diagnostics),
            ("help-log", MenuPress::ShowLog),
            ("help-issue", MenuPress::Issue),
        ] {
            assert_eq!(menu_item(id).map(|m| m.3), Some(press), "{id} does another item's act");
        }
        for (id, press) in [
            ("tray-open", TrayPress::OpenGraph),
            ("tray-review", TrayPress::Review),
            ("tray-finish-setup", TrayPress::FinishSetup),
            ("tray-settings", TrayPress::Settings),
        ] {
            assert_eq!(tray_press(id), Some(press), "{id} does another item's act");
        }
        for (id, _) in map_items_enabled("viz://localhost/", &[]) {
            assert_eq!(
                menu_item(id).map(|m| m.3),
                Some(MenuPress::Map),
                "{id} is not a map command"
            );
        }

        let mut spawned = Daemon::Spawned(std::process::Command::new("true").spawn().unwrap());
        assert_eq!(quit_label(quit_stops(&spawned), "Supragnosis"), "Quit and Stop Memory");
        if let Daemon::Spawned(c) = &mut spawned {
            let _ = c.wait();
        }
        for other in [Daemon::Starting, Daemon::External, Daemon::Failed("x".into())] {
            assert_eq!(quit_label(quit_stops(&other), "Supragnosis"), "Quit Supragnosis");
        }

        let list = serde_json::json!(MAP_CONTROLS).to_string();
        assert!(
            shell_init_script().contains(&format!("controls: {list}")),
            "the page is not given the list"
        );
        let all: Vec<String> = MAP_CONTROLS.iter().map(|c| c.to_string()).collect();
        assert!(
            map_items_enabled("viz://localhost/?workspace=w", &all)
                .iter()
                .all(|(_, on)| *on),
            "a map command's control is not in the list"
        );
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
