//! The tests of the app's own pages and the starting page: the caller check, the commands they
//! call and the source tripwires that hold them to docs/settings-page.md and docs/onboarding.md.

use super::*;

#[cfg(test)]
mod settings_tests {
    use super::{
        cause_of, managed, manager_pid, own_page_ok, restart_outcome, settings_url, start_page,
        starting_html, welcome_url, without_entry, Daemon, StartPage, APP_COMMANDS,
    };
    use std::path::Path;
    use std::time::Duration;

    const LOG: &str = "/nowhere/.supragnosis/log/app-daemon.err.log";

    /// docs/settings-page.md S1 and docs/onboarding.md Section 6: a setting changes only from the
    /// app's own pages, Settings and Welcome - not from the viewer the same window shows, which a
    /// daemon or a hub serves, and not from any other address, a hub's page or a lookalike path
    /// included.
    #[test]
    fn only_the_apps_own_page_may_change_a_setting() {
        let viewer: tauri::Url = "viz://localhost/".parse().unwrap();
        let hub: tauri::Url = "https://hub.example/viz/".parse().unwrap();
        let other_page: tauri::Url = "tauri://localhost/index.html".parse().unwrap();
        let lookalike: tauri::Url = "viz://localhost/settings.html".parse().unwrap();
        let hub_welcome: tauri::Url = "https://hub.example/welcome.html".parse().unwrap();
        for own in [settings_url(), welcome_url()] {
            assert!(own_page_ok("main", &own), "{own}");
            assert!(!own_page_ok("other", &own), "a window that is not the main one");
        }
        assert!(!own_page_ok("main", &viewer), "the viewer in the same window");
        assert!(!own_page_ok("main", &hub));
        assert!(!own_page_ok("main", &other_page), "another page of the app");
        assert!(!own_page_ok("main", &lookalike), "a viewer path that looks alike");
        assert!(!own_page_ok("main", &hub_welcome), "a hub's page named like the app's");
    }

    /// S1, the other half: build.rs declares the commands so none is open by default, and the one
    /// capability that grants them grants every one, to the main window only. The lists are held to
    /// the one in main.rs.
    /// docs/settings-page.md S1: every command the app registers runs the caller check as its first
    /// statement, so a command added later cannot act before it - and the commands the source
    /// declares, the ones `generate_handler!` registers and the list the capability is held to are
    /// one set.
    #[test]
    fn every_command_checks_its_caller_first() {
        let src = super::SRC;
        let mut declared: Vec<String> = Vec::new();
        let mut rest = src;
        while let Some(at) = rest.find("#[tauri::command]\n") {
            rest = &rest[at + "#[tauri::command]\n".len()..];
            let sig_at = rest.find("fn ").expect("a command is a fn");
            let name: String = rest[sig_at + 3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let body_at = rest.find('{').expect("a command has a body");
            let first = rest[body_at + 1..].trim_start();
            assert!(
                first.starts_with("own_page_caller(&webview)?;"),
                "{name} acts before it checks its caller: {}",
                first.lines().next().unwrap_or("")
            );
            declared.push(name);
        }
        let start = src.find("tauri::generate_handler![").expect("the handler list");
        let end = start + src[start..].find(']').expect("its end");
        let mut registered: Vec<String> = src[start + "tauri::generate_handler![".len()..end]
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let mut listed: Vec<String> = APP_COMMANDS.iter().map(|c| c.to_string()).collect();
        declared.sort();
        registered.sort();
        listed.sort();
        assert_eq!(declared.len(), 17, "the scan read the commands");
        assert_eq!(declared, registered, "every command is registered, and only these");
        assert_eq!(declared, listed, "the capability's list is the same set");
    }

    #[test]
    fn settings_commands_are_closed_until_granted() {
        let build = include_str!("../build.rs");
        let settings: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/settings.json")).unwrap();
        let viewer: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(settings["windows"], serde_json::json!(["main"]));
        let granted = |cap: &serde_json::Value, perm: &str| {
            cap["permissions"].as_array().unwrap().iter().any(|p| p.as_str() == Some(perm))
        };
        for cmd in APP_COMMANDS {
            assert!(build.contains(&format!("\"{cmd}\"")), "build.rs must declare {cmd}");
            let perm = format!("allow-{}", cmd.replace('_', "-"));
            assert!(granted(&settings, &perm), "the settings capability needs {perm}");
            assert!(!granted(&viewer, &perm), "{perm} belongs to the settings capability alone");
        }
    }

    /// The narrowing act reaches the daemon as a query string, so a workspace or node name cannot
    /// break out of its parameter: everything but the unreserved characters is encoded.
    #[test]
    fn a_narrowing_is_encoded_into_its_own_parameters() {
        assert_eq!(super::query_value("team-docs_1.x~"), "team-docs_1.x~");
        assert_eq!(super::query_value("a b,c&workspaces=*"), "a%20b%2Cc%26workspaces%3D%2A");
        assert_eq!(super::query_value(""), "");
    }

    /// S5: the settings page renders the text it is given as text. It shows CLI output, which can
    /// quote a server's answer, so no markup sink may appear in its script at all.
    #[test]
    fn the_settings_page_never_renders_markup() {
        // The init script runs in every page the main window shows - the viewer included - and builds
        // the page navigation there, so it is held to the same rule.
        for (file, js) in [
            ("settings.js", include_str!("../assets/settings.js")),
            ("welcome.js", include_str!("../assets/welcome.js")),
            ("kit.js", include_str!("../assets/kit.js")),
            ("shell-init.js", include_str!("../assets/shell-init.js")),
        ] {
            for sink in ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write", "eval("]
            {
                assert!(!js.contains(sink), "{file} must not use {sink}");
            }
        }
        let html = include_str!("../assets/settings.html");
        assert!(html.contains("Content-Security-Policy"), "the page carries its own policy");
        assert!(!html.contains("<script>"), "no inline script");
    }

    /// docs/design-system.md U19: a dialog on the app's pages holds focus - Tab stays inside it and the
    /// page behind it is inert until it closes - and every page says its language.
    #[test]
    fn the_app_pages_dialogs_hold_focus() {
        let kit = include_str!("../assets/kit.js");
        let hold = &kit[kit.find("function holdFocus(").expect("holdFocus")..];
        let hold = &hold[..hold.find("\n}\n").expect("it closes")];
        for part in [
            "el.inert = true",
            "el.inert = false",
            "\"Tab\"",
            "e.shiftKey",
            "first.focus()",
            "last.focus()",
        ] {
            assert!(hold.contains(part), "holdFocus does not {part}");
        }
        let settings = include_str!("../assets/settings.js");
        let open = &settings[settings.find("function openDialog(").expect("openDialog")..];
        let open = &open[..open.find("\n}\n").expect("it closes")];
        assert!(
            open.contains("releaseFocus = holdFocus(dialog);"),
            "the Settings dialog does not hold focus"
        );
        let close = &settings[settings.find("function closeDialog(").expect("closeDialog")..];
        let close = &close[..close.find("\n}\n").expect("it closes")];
        assert!(
            close.contains("releaseFocus()"),
            "closing the dialog does not give the page back"
        );
        for (file, html) in [
            ("settings.html", include_str!("../assets/settings.html")),
            ("welcome.html", include_str!("../assets/welcome.html")),
        ] {
            assert!(html.contains("<html lang=\"en\">"), "{file} does not declare its language");
        }
    }

    /// docs/settings-page.md S7 and semantic-retrieval.md Section 13: a daemon that cannot start because
    /// its store was indexed by another model is shown with that reason and Rebuild index, which takes
    /// Restart's place; Rebuild index is on the Search row whenever the daemon runs with a model; the
    /// command stops the daemon the app spawned itself, leaves every other to `embed rebuild`, waits
    /// for a login job the CLI started again, and attaches; and the page follows the re-embedding.
    #[test]
    fn a_store_the_daemon_cannot_open_is_rebuilt_from_the_page() {
        let src = super::SRC;
        let body = |src: &'static str, sig: &str| -> &'static str {
            let at = src.find(sig).unwrap_or_else(|| panic!("no {sig}"));
            &src[at..at + src[at..].find("\n}\n").expect("it closes")]
        };
        let rebuild = body(src, "async fn rebuild_index(");
        for part in [
            "if let Daemon::Spawned(mut child) = prev {",
            "run_cli(&[\"embed\", \"rebuild\"])",
            "wait_for_socket(&sock, Duration::from_secs(30))",
            "bring_up(app.clone(), sock).await;",
        ] {
            assert!(rebuild.contains(part), "the rebuild does not run as the page says: {part}");
        }
        let kill = rebuild.find("child.kill()").expect("the app's own daemon is stopped");
        let cli = rebuild.find("run_cli(&[\"embed\", \"rebuild\"])").expect("the CLI rebuilds");
        assert!(kill < cli, "the app's own daemon is stopped before the CLI resets its store");
        assert!(
            body(src, "async fn settings_state_of(").contains("\"embedder_conflict\": status"),
            "the page is not told why the daemon cannot start"
        );

        let js = include_str!("../assets/settings.js");
        let status = body(js, "function daemonStatus(");
        assert!(
            status.contains("d.embedder_conflict")
                && status.contains("The search index needs rebuilding"),
            "the reason is not shown"
        );
        let daemon = body(js, "function renderDaemon(");
        assert!(
            daemon.contains("conflict ? h(\"div\", { class: \"end\" },")
                && daemon.contains("drift || conflict ? null :"),
            "Rebuild index does not take Restart's place on a store the daemon cannot open"
        );
        assert!(
            body(js, "function searchRow(").contains("() => rebuildIndex())"),
            "the Search row does not offer Rebuild index"
        );
        assert!(
            body(js, "function rebuildIndex(").contains("act(\"rebuild\", \"search_rebuild\"")
                && body(js, "function rebuildIndex(").contains(".then(watchDaemon)"),
            "Rebuild index does not call its command, or does not follow the daemon"
        );
        assert!(
            body(js, "function daemonUnsettled(").contains("|| indexing()")
                && body(js, "function watchDaemon(").contains("if (indexing()) watch.until"),
            "the page does not follow the re-embedding"
        );

        // The starting page says why and opens Settings to rebuild, holding no command itself (S1);
        // Settings starts the rebuild only while the store still cannot open, and drops the ask.
        let conflict = ("hashing-256".to_string(), "<model>".to_string());
        let failed_with = |why: &str| StartPage::Failed(why.to_string());
        let page =
            starting_html(&failed_with("X."), Some(&conflict), &settings_url(), Path::new(LOG));
        for part in [
            "The search index needs rebuilding",
            "Indexed by hashing-256, and the daemon runs &lt;model&gt;",
            "settings.html?act=rebuild#daemon\"",
            ">Rebuild index</a>",
            "supragnosis embed rebuild",
        ] {
            assert!(page.contains(part), "the starting page does not offer the rebuild: {part}");
        }
        assert!(!page.contains("<script"), "the starting page holds a script");
        let failed =
            starting_html(&failed_with("Port in use."), None, &settings_url(), Path::new(LOG));
        assert!(
            failed.contains("settings.html#daemon\"") && failed.contains(">Open This Mac</a>"),
            "a failed start does not open This Mac"
        );
        assert!(
            !starting_html(&StartPage::Starting, None, &settings_url(), Path::new(LOG))
                .contains("</a>"),
            "a start in progress offers a button"
        );
        assert_eq!(
            super::conflict_of(
                &serde_json::json!({"embedder_conflict": {"recorded": "a", "daemon": "b"}})
            ),
            Some(("a".to_string(), "b".to_string()))
        );
        assert_eq!(super::conflict_of(&serde_json::json!({"embedder_conflict": null})), None);
        let asked = body(js, "function actAsked(");
        assert!(
            asked.contains("act === \"rebuild\"")
                && asked.contains("history.replaceState(")
                && asked.contains("d.embedder_conflict) rebuildIndex();"),
            "Settings does not start the rebuild the starting page asked for, or not only on a conflict"
        );
        let welcome = include_str!("../assets/welcome.js");
        assert!(
            welcome.contains("state === \"attention\" && /embed rebuild/.test(say)")
                && welcome.contains("await invoke(\"search_rebuild\")"),
            "Setup's server card does not offer the rebuild"
        );
    }

    /// daemon-lifecycle.md Section 6: a restart of a daemon a login job runs is decided after the
    /// shell waited for the socket. The CLI's success is a restart, and so is a refusal after which a
    /// new process answers - launchd retrying a start that lost the store lock for a moment; a
    /// refusal that left the same process says why; a daemon that never answered is a failure that
    /// names `supragnosis status`. A failed state is healed whenever the state is read, and Settings
    /// follows the daemon after a restart until it answers, for a minute at most.
    #[test]
    fn a_restart_says_what_it_came_to() {
        let refused = || Some((false, "2 managers claim the daemon".to_string()));
        assert_eq!(
            restart_outcome(Some((true, "restarted".into())), Some(1), Some(2), true),
            Ok("restarted".into())
        );
        assert_eq!(
            restart_outcome(Some((false, "exited with status 1".into())), Some(1), Some(2), true),
            Ok("restarted".into()),
            "a refusal that launchd's retry settled is reported as the failure that passed"
        );
        assert_eq!(
            restart_outcome(refused(), Some(1), Some(1), true),
            Err("restart refused: 2 managers claim the daemon".into())
        );
        let late = restart_outcome(refused(), Some(1), None, false).unwrap_err();
        assert!(late.contains("30 seconds") && late.contains("`supragnosis status`"), "{late}");
        assert!(late.contains("2 managers"), "the CLI's reason is dropped: {late}");
        assert!(restart_outcome(Some((true, String::new())), Some(1), None, false).is_err());
        assert!(restart_outcome(None, None, None, true).is_err());
        let st = serde_json::json!({ "managers": [{ "type": "launchd", "pid": 4242 }] });
        assert_eq!(manager_pid(Some(&st)), Some(4242));
        assert_eq!(manager_pid(Some(&serde_json::json!({ "managers": [] }))), None);

        let src = super::SRC;
        let body = |src: &'static str, sig: &str| -> &'static str {
            let at = src.find(sig).unwrap_or_else(|| panic!("no {sig}"));
            &src[at..at + src[at..].find("\n}\n").expect("it closes")]
        };
        for reader in ["async fn refresh_status(", "async fn settings_state_of("] {
            assert!(
                body(src, reader).contains("heal_failed(app).await"),
                "{reader} keeps a stale failure"
            );
        }
        assert!(
            body(src, "async fn restart_daemon(")
                .contains("wait_for_socket(&sock, Duration::from_secs(30))"),
            "a restart races the login job it restarted"
        );
        let js = include_str!("../assets/settings.js");
        assert!(body(js, "function restartDaemon(").contains(".then(watchDaemon)"));
        assert!(body(js, "async function load(").contains("if (daemonUnsettled()) watchDaemon();"));
        let watch = body(js, "function watchDaemon(");
        for part in [
            "if (watch) return;",
            "Date.now() + 60000",
            "setTimeout(tick, 2000)",
            "watch = null; return;",
        ] {
            assert!(watch.contains(part), "the page does not follow the daemon: {part}");
        }
    }

    /// docs/design-system.md Section 6, U26: in the app the title bar drags the window from anywhere
    /// but its controls. Tauri's handler counts a bare data-tauri-drag-region only for a press on that
    /// element itself, and since the bar became a Mac toolbar its two groups cover all of it but the
    /// padding; so the header drags deep, the search's results opt out, and the splash, whose text
    /// covers its middle, drags deep too.
    #[test]
    fn the_title_bar_drags_the_window_but_not_its_controls() {
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("header.setAttribute(\"data-tauri-drag-region\", \"deep\");"));
        assert!(shell.contains("results.setAttribute(\"data-tauri-drag-region\", \"false\")"));
        assert!(
            !shell.contains("h1.setAttribute(\"data-tauri-drag-region\""),
            "a bare attribute on the title stops a press on its scope from dragging"
        );
        assert!(starting_html(&StartPage::Starting, None, &settings_url(), Path::new(LOG))
            .contains("<body data-tauri-drag-region=\"deep\""));
        assert!(!super::SRC.contains("<body data-tauri-drag-region style="));
    }

    /// docs/design-system.md Section 29.5, P24: the page the window shows while the daemon does not
    /// answer says which of three things is true, from the shell's state, and has no timer of its
    /// own. A failure names its cause in one sentence and offers Restart, the default, Diagnostics and
    /// Open This Mac - each a link to Settings, no command of its own (S1) - and the log's path in a
    /// field; a start the shell made says nothing to press for 10 seconds, and then that it is still
    /// starting while the child it spawned lives.
    #[test]
    fn a_daemon_that_cannot_start_says_so() {
        let mut starting = Daemon::Starting;
        assert_eq!(start_page(&mut starting, Some(Duration::from_secs(3))), StartPage::Starting);
        assert_eq!(
            start_page(&mut starting, Some(Duration::from_secs(12))),
            StartPage::StillStarting
        );
        assert_eq!(start_page(&mut starting, None), StartPage::StillStarting);
        let mut alive =
            Daemon::Spawned(std::process::Command::new("sleep").arg("5").spawn().unwrap());
        assert_eq!(start_page(&mut alive, Some(Duration::from_secs(30))), StartPage::StillStarting);
        if let Daemon::Spawned(c) = &mut alive {
            let _ = c.kill();
            let _ = c.wait();
        }
        assert_eq!(
            start_page(&mut alive, None),
            StartPage::Failed("The daemon exited.".to_string())
        );
        assert_eq!(start_page(&mut Daemon::External, None), StartPage::Stopped);
        let stopped = starting_html(&StartPage::Stopped, None, &settings_url(), Path::new(LOG));
        assert!(stopped.contains("Supragnosis stopped") && stopped.contains(">Restart</a>"));
        let why = "the spawned daemon exited immediately (exit status: 1): store locked - full log: ~/.supragnosis/log/app-daemon.err.log";
        assert_eq!(
            start_page(&mut Daemon::Failed(why.to_string()), None),
            StartPage::Failed(
                "The spawned daemon exited immediately (exit status: 1): store locked.".to_string()
            )
        );
        assert_eq!(cause_of("port in use.\nmore"), "Port in use.");
        assert_eq!(cause_of(""), "The daemon did not start.");

        let log = Path::new(LOG);
        let failed = starting_html(
            &StartPage::Failed("Port in use.".to_string()),
            None,
            &settings_url(),
            log,
        );
        for part in [
            "Supragnosis could not start",
            "Port in use.",
            "settings.html?act=restart#daemon\" autofocus",
            ">Restart</a>",
            "settings.html?act=diagnostics#daemon\"",
            ">Diagnostics</a>",
            "settings.html#daemon\"",
            ">Open This Mac</a>",
            "<input class=\"lf\" readonly value=\"/nowhere/.supragnosis/log/app-daemon.err.log\"",
            "settings.html?act=reveal-log#daemon\"",
            ">Show in Finder</a>",
            "@media (prefers-reduced-motion: reduce) { .spin { animation:none } }",
        ] {
            assert!(failed.contains(part), "a failed start does not say: {part}");
        }
        let still = starting_html(&StartPage::StillStarting, None, &settings_url(), log);
        assert!(
            still.contains("Still starting.")
                && still.contains(">Open This Mac</a>")
                && still.contains("<input")
        );
        assert!(!still.contains("act=restart"), "a start in progress offers Restart");
        let first = starting_html(&StartPage::Starting, None, &settings_url(), log);
        assert!(first.contains("Starting Supragnosis...") && !first.contains("<input"));
        for page in [&failed, &still, &first] {
            assert!(!page.contains("http-equiv"), "the starting page reloads itself");
            assert!(!page.contains("<script"), "the starting page holds a script");
            assert!(!page.contains("starting the supragnosis daemon"), "the old heading is back");
        }

        // The shell changes the page: it watches while the page is drawn and reloads when the daemon
        // answers or the page should say something else, and forgets it once the graph is served.
        let src = super::SRC;
        let watch = &src[src.find("async fn watch_splash(").expect("watch_splash")..];
        let watch = &watch[..watch.find("\n}\n").expect("it closes")];
        assert!(watch.contains("Duration::from_secs(1)") && watch.contains("reload_viewer"));
        assert!(
            src.contains("if target == \"/\" {\n                                *SPLASH.lock().unwrap() = None;"),
            "the graph's page does not end the watch"
        );

        // Restart of a failed daemon goes through `supragnosis restart` when a manager owns it.
        assert!(src.contains(
            "prev @ (Daemon::External | Daemon::Failed(_))\n            if matches!(prev, Daemon::External) || job =>"
        ));
        assert!(managed(&serde_json::json!({"managers": [{"label": "launchd"}]})));
        assert!(!managed(&serde_json::json!({"managers": []})));
        assert!(!managed(&serde_json::json!({})));

        // Settings acts on what the page asks, once, and only while that act still fits.
        let js = include_str!("../assets/settings.js");
        let asked = &js[js.find("function actAsked(").expect("actAsked")..];
        let asked = &asked[..asked.find("\n}\n").expect("it closes")];
        for part in [
            "keep.delete(\"act\")",
            "history.replaceState(",
            "if (!d.answering && d.shell !== \"starting\") restartDaemon();",
            "\"Not restarted: the daemon answers\"",
            "\"Not restarted: the daemon is still starting\"",
            "document.getElementById(\"copyDiagnostics\")",
            "revealFile(\"log\")",
        ] {
            assert!(asked.contains(part), "Settings does not act on the page's ask: {part}");
        }
        // Show in Finder shows one of two files, for the app's own pages only.
        let reveal = &src[src.find("async fn reveal(").expect("reveal")..];
        let reveal = &reveal[..reveal.find("\n}\n").expect("it closes")];
        assert!(reveal.contains("own_page_caller(&webview)?;"));
        assert!(reveal.contains("\"log\" => daemon_log(),") && reveal.contains("\"config\" =>"));
        assert!(reveal.contains("other => return Err("));
        assert!(APP_COMMANDS.contains(&"reveal"));
        assert!(include_str!("../capabilities/settings.json").contains("\"allow-reveal\""));
        // Diagnostics carries the shell's status and the log's last lines; This Mac shows the log.
        let diag = &src[src.find("async fn diagnostics(").expect("diagnostics")..];
        let diag = &diag[..diag.find("\n}\n").expect("it closes")];
        assert!(diag.contains("status_line()") && diag.contains("tail -n 40"));
        assert!(js.contains("id: \"copyDiagnostics\"") && js.contains("\"data-k\": \"log\""));
    }

    /// design-system.md Section 29.10: Try it quotes the entry in the question's block and opens the
    /// graph on its entity when it names one; the waiting sentence is setup's, with the question set
    /// apart; the page counts how long it has waited (O4), to show the minutes, the checks the
    /// event carries and the foot's Copy Diagnostics; the entry's arrival is announced; and the
    /// shell gives the viewer Setup's address for an empty store.
    #[test]
    fn try_it_quotes_the_entry_and_counts_its_wait() {
        let js = include_str!("../assets/welcome.js");
        let body = |name: &str| -> &str {
            let at = js.find(&format!("\nfunction {name}(")).unwrap_or_else(|| panic!("no {name}"));
            &js[at + 1..at + 1 + js[at + 1..].find("\n}\n").expect("it closes")]
        };
        let first = body("firstBody");
        for part in [
            "h(\"div\", { class: \"ask entry\" }, h(\"q\", { class: \"selectable\", text: e.quote }))",
            "if (e.entity) {",
            "openGraph(e.id, e.entity)",
            "\"Show in the Graph\"",
            "const lead = m ? say.slice(0, m.index).trim() : \"\";",
            "mins >= 2 && Array.isArray(e.checks)",
            "/^No connected app has reached the server/",
        ] {
            assert!(first.contains(part), "firstBody lacks {part}");
        }
        assert!(!js.contains("The entry appears here"), "the page no longer promises it");
        assert!(body("openGraph").contains("\"entity=\" + encodeURIComponent(entity)"));
        let check = &js[js.find("async function check()").unwrap()..];
        let check = &check[..check.find("\n}\n").unwrap()];
        assert!(check.contains("announce(\"First entry recorded in \""));
        assert!(
            check.contains("firstWaitSince = Date.now()"),
            "the wait counts from the first wait seen"
        );
        assert!(
            body("renderActions").contains("|| waitedMinutes() >= 2"),
            "Copy Diagnostics after two minutes"
        );
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("window.supragnosisSetup = \"tauri://localhost/welcome.html\""));
    }

    /// design-system.md Section 29.10 and onboarding.md O6: the page and Copy Diagnostics read the
    /// same `setup --check --events` line, so the report drops the first entry's words, its app and
    /// its entity, and keeps everything else as setup wrote it.
    #[test]
    fn copy_diagnostics_carries_no_entry() {
        let line = r#"{"step":"first","id":"billing","state":"done","say":"An entry is in \"billing\", just now.","quote":"the launch is friday","app":"Claude Code","entity":"e1"}"#;
        let out = without_entry(&format!("{{\"schema\":1}}\n{line}\nnot json\n"));
        assert!(
            !out.contains("friday") && !out.contains("Claude Code") && !out.contains("\"e1\""),
            "{out}"
        );
        // The done sentence is copied without the workspace or the app it named (Section 29.13).
        assert!(
            out.contains("An entry is in the store.")
                && !out.contains("billing")
                && out.contains("not json")
        );
        assert!(out.ends_with('\n'));
        let src = super::SRC;
        let diag = &src[src.find("async fn diagnostics(").expect("diagnostics")..];
        assert!(
            diag[..diag.find("\n}\n").unwrap()].contains("without_entry(&text(run_cli(&args)))")
        );
    }

    /// docs/design-system.md U19: a page that draws itself again keeps the control that had focus.
    /// Setup and Settings draw through `kit.js`'s `keepFocus`, which finds the control again by the
    /// key its row carries and its name, and keeps a field's selection; the reads that come on a
    /// timer or on the window's focus draw only what changed, and a streamed line is added to its
    /// open drawer in place.
    #[test]
    fn the_app_pages_keep_focus_across_a_draw() {
        let kit = include_str!("../assets/kit.js");
        let keep = &kit[kit.find("function keepFocus(").expect("keepFocus")..];
        let keep = &keep[..keep.find("\n}\n").expect("it closes")];
        for part in [
            "focusKey(el, true)",
            "focusKey(el, false)",
            "near.length === 1",
            "setSelectionRange(",
            ".focus(",
        ] {
            assert!(keep.contains(part), "keepFocus does not: {part}");
        }
        assert!(
            kit.contains("el.closest(\"[data-k], [id]\")"),
            "a control is not found by its row"
        );
        let settings = include_str!("../assets/settings.js");
        let welcome = include_str!("../assets/welcome.js");
        for (page, js) in [("Settings", settings), ("Setup", welcome)] {
            assert!(
                js.contains("function render() { keepFocus(document.body, draw); }"),
                "{page} draws without keeping focus"
            );
        }
        for key in [
            "\"capture:\" + a.id",
            "\"data-k\": \"daemon\"",
            "\"data-k\": \"model\"",
            "\"hub:\" + hub.url",
            "\"peer:\" + id",
        ] {
            assert!(settings.contains(key), "a Settings row has no key: {key}");
        }
        assert!(welcome.contains("\"data-k\": \"step:\" + s.step"), "a Setup card has no key");
        // An AI app's row, on either page, is kit.js's, which keys it.
        assert!(kit.contains("\"data-k\": \"app:\" + row.id"), "an app row has no key");
        assert!(settings.contains("if (!quiet || now !== drawnFrom) render();"));
        assert!(settings.contains("load(true);"), "the window's focus draws what did not change");
        assert!(settings.contains("await load(true);"), "the watch draws what did not change");
        assert!(
            welcome.contains("if (now === checkedFrom) return;"),
            "Setup's check draws what did not change"
        );
        assert!(
            welcome.contains(
                "open.append(h(\"div\", { class: \"l-\" + entry.kind, text: entry.text }));"
            ),
            "a streamed line draws the page"
        );
    }

    /// docs/design-system.md Section 29.29 and settings-page.md S8: the main window's pages get the
    /// platform's events, its full-screen getter and the title bar's drag, and nothing else - no
    /// menu, tray, image, path or resource, which any page the window shows could otherwise use to
    /// speak with the app's voice.
    #[test]
    fn the_window_pages_get_only_what_they_use() {
        let caps: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(caps["windows"], serde_json::json!(["main"]));
        let granted: Vec<&str> = caps["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .collect();
        assert_eq!(
            granted,
            [
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "core:event:allow-emit",
                "core:window:allow-is-fullscreen",
                "core:window:allow-start-dragging",
                "core:window:allow-internal-toggle-maximize",
            ],
            "exactly what the pages use"
        );
    }

    /// docs/design-system.md Section 6, U26: the bar keeps room for the traffic lights only while
    /// they are shown. The shell announces each full-screen transition, and a page asks the window on
    /// load, since one opened while the window is already full screen saw no transition and kept the
    /// room; the first answer lands without the padding's animation. Every page links the one
    /// stylesheet that holds the rule.
    #[test]
    fn the_title_bar_clears_the_lights_only_while_they_are_shown() {
        let shell = include_str!("../assets/shell-init.js");
        assert!(shell.contains("event.listen(\"shell-fullscreen\""), "transitions are not heard");
        assert!(
            shell.contains("getCurrentWindow().isFullscreen()"),
            "a page loaded in full screen does not ask"
        );
        let css = include_str!("../assets/shell.css");
        assert!(css.contains("html:not(.shell-fullscreen) header { padding-left: 84px; }"));
        assert!(css.contains("html.shell-asking header { transition: none; }"));
        // The shell's side, read from the app's source: the test's own copy of the line is escaped, so it
        // cannot be what matches.
        assert!(super::SRC.contains("window.emit(\"shell-fullscreen\", fs)"));
        for (file, html) in [
            ("settings.html", include_str!("../assets/settings.html")),
            ("welcome.html", include_str!("../assets/welcome.html")),
        ] {
            assert!(
                html.contains("<link rel=\"stylesheet\" href=\"shell.css\" data-shell>"),
                "{file}"
            );
        }
    }

    /// docs/design-system.md Section 10, U19: a button that shows only an icon is named by its act and
    /// its object.
    #[test]
    fn the_settings_page_names_its_buttons_and_its_errors() {
        let js = include_str!("../assets/settings.js");
        let body = |name: &str| {
            let at = js.find(&format!("function {name}(")).expect(name);
            &js[at..at + js[at..].find("\n}\n").expect("it closes")]
        };
        assert!(
            body("actionButton").contains("\"aria-label\""),
            "an icon button cannot be named"
        );
    }

    /// docs/design-system.md Section 6a: the first-run page is Setup wherever it is reached, every
    /// sentence on it is one the code keeps, and its progress is said - as a progress bar a screen
    /// reader reads, in the foot while the plan runs, in a live region as each step ends, and in the
    /// heading once the essentials are done.
    /// design-system.md Section 14 step 17: every checkbox and radio on the app's pages takes the
    /// accent, Setup's privacy line keeps a reading width, and the starting splash is drawn in the
    /// tokens and the reading face rather than the old mono and colors.
    #[test]
    fn the_third_pass_defects_on_the_apps_pages() {
        let css = include_str!("../assets/settings.css");
        let welcome = include_str!("../assets/welcome.css");
        let main = super::SRC;
        assert!(
            css.contains(":where(input[type=\"checkbox\"], input[type=\"radio\"]) { accent-color: var(--accent); }"),
            "a checkbox can take the system's blue beside a gold one"
        );
        assert!(
            welcome.contains("min-height: 24px; border: 0;"),
            "Setup's Details is under 24 pixels"
        );
        assert!(
            welcome.contains("max-width: 34em"),
            "the privacy line runs past a reading width"
        );
        let splash: Vec<&str> = main
            .split("r#\"<!doctype html>")
            .skip(1)
            .map(|page| &page[..page.find("</body>").expect("a splash ends its body")])
            .collect();
        assert_eq!(splash.len(), 1, "the shell authors one splash page");
        for page in splash {
            assert!(
                !page.contains("monospace") && !page.contains("#7b8494"),
                "a splash keeps the old look"
            );
        }
    }

    /// design-system.md Section 10 and U21: View sets the root's text size from 100% to 200%, every
    /// rem size follows it, the choice is kept, and the script a page is given sets it.
    #[test]
    fn view_sets_the_root_text_size() {
        let main = super::SRC;
        for part in [
            "\"Make Text Bigger\"",
            "Some(\"CmdOrCtrl+=\")",
            "\"Make Text Smaller\"",
            "Some(\"CmdOrCtrl+-\")",
            "\"Actual Size\"",
            "Some(\"CmdOrCtrl+0\")",
            "\"view.json\"",
        ] {
            assert!(main.contains(part), "the root text size is missing {part}");
        }
        assert_eq!(super::step_text_scale(1.0, 1), 1.125);
        assert_eq!(super::step_text_scale(2.0, 1), 2.0);
        assert_eq!(super::step_text_scale(1.0, -1), 1.0);
        assert_eq!(super::step_text_scale(1.5, 0), 1.0);
        assert!(super::text_scale_script(1.25).contains("fontSize = \"125%\""));
    }

    #[test]
    fn the_setup_page_says_what_is_true_and_how_far_it_is() {
        let html = include_str!("../assets/welcome.html");
        let js = include_str!("../assets/welcome.js");
        let settings = include_str!("../assets/settings.js");
        let main = super::SRC;
        assert!(
            html.contains("<h1 id=\"pageTitle\">Setup</h1>"),
            "the title bar does not say Setup"
        );
        assert!(
            html.contains(
                "Supragnosis keeps your memory on this Mac. It is shared only if you set up sync."
            ),
            "the privacy line promises what the code does not keep"
        );
        assert!(js.contains("Open Setup"), "the page does not name the way back");
        assert!(settings.contains("\"Open Setup\""), "About does not say Open Setup");
        assert!(
            main.contains("\"tray-finish-setup\",\n                \"Finish Setup...\","),
            "the tray does not say Finish Setup..."
        );
        for part in [
            "role: \"progressbar\"",
            "\"aria-valuenow\"",
            "Setting up... ",
            "Supragnosis is set up",
            "function announce(",
        ] {
            assert!(js.contains(part), "the page does not say its progress: {part}");
        }
        assert!(
            html.contains("id=\"announce\"") && html.contains("aria-live=\"polite\""),
            "there is no live region for a step's end"
        );
    }

    /// settings-page.md Section 3.3: the model download runs outside the page's one-act lock, so the
    /// rest of the page stays usable for its minutes; its row shows its progress and can stop it.
    #[test]
    fn the_download_does_not_lock_the_settings_page() {
        let js = include_str!("../assets/settings.js");
        let body = |name: &str| {
            let at = js.find(&format!("function {name}(")).expect(name);
            &js[at..at + js[at..].find("\n}\n").expect("it closes")]
        };
        let row = body("searchRow");
        assert!(
            !row.contains("act(\"model\"") && !row.contains("actionButton(\"model\""),
            "the download takes the page's lock"
        );
        assert!(
            row.contains("role: \"progressbar\"") && row.contains("\"Stop\""),
            "the download's row has no progress or Stop"
        );
        assert!(body("downloadModel").contains("invoke(\"setup_run\", { step: \"model\" })"));
        assert!(body("stopDownload").contains("invoke(\"setup_stop\", { step: \"model\" })"));
    }

    /// design-system.md Section 6a, onboarding.md O8: Setup's plan is checkboxes, each row its label;
    /// replacing what the person set up is in the plan and starts unchecked, so nothing changes until
    /// Set up; and Set up is the default button, first in tab order, which Return presses.
    #[test]
    fn the_setup_plan_is_checkboxes_and_set_up_is_the_default() {
        let js = include_str!("../assets/welcome.js");
        let css = include_str!("../assets/welcome.css");
        assert!(!js.contains("function sw("), "the plan still mixes switches with checkboxes");
        assert!(
            js.contains("replace: {}, takeover: false"),
            "replacing items do not start unchecked"
        );
        assert!(
            js.contains("checked: !!plan.replace[a.id]"),
            "an app's replacement is not a plan item"
        );
        assert!(
            js.contains("pick: !!tick")
                && include_str!("../assets/kit.js").contains("h(row.pick ? \"label\" : \"div\""),
            "a plan row is not its checkbox's label"
        );
        assert!(
            js.contains("if (takingOver) await run(\"takeover\""),
            "Set up does not apply the take-over it was given"
        );
        assert!(
            js.contains("document.getElementById(\"setup-go\")")
                && js.contains("e.key === \"Enter\""),
            "Return does not press Set up"
        );
        assert!(
            css.contains("flex-direction: row-reverse"),
            "Set up is not first in tab order and rightmost"
        );
    }

    /// design-system.md Section 14 step 13: a step the person started and left is not lost. The shell
    /// answers what is running, and Setup and Settings show it as running and wait for it; a second
    /// start of a running step says it is already running instead of reporting a failure.
    #[test]
    fn a_running_step_survives_leaving_its_page() {
        let welcome = include_str!("../assets/welcome.js");
        let settings = include_str!("../assets/settings.js");
        assert!(
            welcome.contains("loadInfo().then(check).then(adoptRunning);"),
            "Setup does not pick up a running step"
        );
        assert!(
            welcome.contains("/already running/.test(String(e))"),
            "a second start reads as a failure"
        );
        assert!(
            settings.contains("load().then(adoptDownload).then(actAsked);"),
            "Settings does not pick up a running download"
        );
    }

    /// design-system.md Section 6a: the Settings page says each fact once. Version drift is one card
    /// with the one Restart; a section's dot appears only when it needs attention, in words a screen
    /// reader reads; a peer is one row.
    #[test]
    fn the_settings_page_says_each_fact_once() {
        let js = include_str!("../assets/settings.js");
        assert!(
            js.contains("\"Update ready - restart to load \" + d.installed"),
            "drift is not one card"
        );
        // A store the daemon cannot open takes Restart away too: its card holds Rebuild index (S7).
        assert!(
            js.contains("drift || conflict ? null :"),
            "the startup row's Restart repeats the drift card's"
        );
        assert!(
            js.contains("return kind === \"warn\" || kind === \"bad\" ? kind : null;"),
            "every section carries a dot"
        );
        assert!(js.contains("class: \"sr-only\""), "a dot says nothing to a screen reader");
        assert!(
            js.contains(
                "new Set([...admitted.map((p) => p.node_id), ...known.map((p) => p.node_id)])"
            ),
            "a peer is listed twice"
        );
    }

    /// design-system.md Section 6a: Claude Code's capture hook is a switch on its row while it is
    /// connected through the bridge, and nowhere else; on and off are `connect claude-code` with and
    /// without `--no-capture`.
    #[test]
    fn the_capture_switch_shows_only_on_a_bridged_row() {
        let js = include_str!("../assets/settings.js");
        assert!(
            js.contains(
                "if (a.id !== \"claude-code\" || typeof a.capture !== \"boolean\") return row;"
            ),
            "the switch shows on a row it cannot act on"
        );
        assert!(js.contains("act(\"capture\", \"capture_set\", { on: e.target.checked }"));
        // The switch is a second row after Claude Code's; the rows are flattened into the card, and
        // the element helper flattens any depth, so neither row becomes "[object HTMLDivElement]".
        assert!(
            js.contains("const rows = state.apps.flatMap((a) => {"),
            "Claude Code's two rows are nested in the card's children"
        );
        assert!(
            include_str!("../assets/kit.js").contains("for (const c of children.flat(Infinity)) {"),
            "a nested list of children is turned into text"
        );
        let main = super::SRC;
        let at = main.find("async fn capture_set(").expect("capture_set");
        let body = &main[at..at + main[at..].find("\n}\n").expect("it closes")];
        assert!(body.contains("\"--no-capture\"") && body.contains("\"claude-code\""));
        assert!(
            main.contains(".filter(|r| r[\"entry\"].as_str() == Some(\"bridge\"))"),
            "capture is read from a row not bridged"
        );
    }

    /// Settings > About names where the source lives, and its hint points there for the dependency
    /// licences. The value is the package manifest's `repository`, which this crate has to inherit
    /// from its own workspace by name; when the shell left the server workspace it stopped doing
    /// so, and every build until 0.5.2 showed "-" in that row.
    /// The tray's Review item counts what waits for a person (docs/design-system.md Section 7): open
    /// proposals and blocked ones, which wait to be rejected, never merged, rejected or withdrawn
    /// ones; and from the curation report the suspected secrets and the contested beliefs, all of
    /// Needs you. Without the report it says what it counted (P5).
    #[test]
    fn the_review_item_counts_open_and_blocked_proposals() {
        let list = serde_json::json!([
            {"state": "open"}, {"state": "blocked"}, {"state": "blocked"},
            {"state": "merged"}, {"state": "rejected"}, {"state": "withdrawn"}
        ]);
        assert_eq!(super::review_count(&list), 3);
        assert_eq!(super::review_count(&serde_json::json!({"error": "x"})), 0);
        let report = serde_json::json!({
            "secrets": [{"observation": "s"}],
            "contradictions": [
                {"id": "a", "field": "kind", "contested": true},
                {"id": "type:ws:entity:Service", "field": "definition", "contested": true},
                {"id": "b", "field": "kind", "contested": false}
            ]
        });
        assert_eq!(super::curation_count(&report), Some(3), "a secret and two contested beliefs");
        assert_eq!(super::curation_count(&serde_json::json!({"error": "x"})), None);
        let text = |proposals, needs| super::review_item_text(&super::Waiting { proposals, needs });
        assert_eq!(text(3, Some(6)).as_deref(), Some("Review - 6 need you..."));
        assert_eq!(text(0, Some(1)).as_deref(), Some("Review - 1 needs you..."));
        assert_eq!(text(2, None).as_deref(), Some("Review - 2 proposals wait..."));
        assert_eq!(text(1, None).as_deref(), Some("Review - 1 proposal waits..."));
        assert_eq!(text(0, Some(0)), None);
        assert_eq!(text(0, None), None);
    }

    /// design-system.md Section 29.4: the way back is the viewer's address an app page carries in
    /// `?from=`, its `#review` dropped, or the viewer's own address; nothing else is followed, so a
    /// page cannot make "< Graph" a redirect (settings-page.md Section 4).
    #[test]
    fn the_way_back_goes_only_to_the_viewer() {
        let viewer = super::graph_url();
        let mut left = viewer.clone();
        left.query_pairs_mut()
            .append_pair("workspace", "billing")
            .append_pair("entity", "e1");
        let settings = super::with_from(super::settings_url(), Some(&left));
        assert_eq!(super::graph_left(&settings).as_ref(), Some(&left), "the way back is lost");
        let mut on_review = left.clone();
        on_review.set_fragment(Some("review"));
        assert_eq!(super::graph_left(&on_review).as_ref(), Some(&left), "#review is kept");
        // Passed on from one app page to the other.
        let setup = super::with_from(super::welcome_url(), Some(&settings));
        assert_eq!(super::graph_left(&setup).as_ref(), Some(&left));
        let mut evil = super::settings_url();
        evil.query_pairs_mut().append_pair("from", "https://example.com/");
        assert_eq!(super::graph_left(&evil), None, "an address elsewhere is followed");
        assert_eq!(super::graph_left(&super::settings_url()), None);
    }

    #[test]
    fn the_about_section_names_the_source() {
        assert!(
            env!("CARGO_PKG_REPOSITORY").starts_with("https://github.com/"),
            "app/Cargo.toml [package] must inherit repository from [workspace.package]"
        );
    }
}
