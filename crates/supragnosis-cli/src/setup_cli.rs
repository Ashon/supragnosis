//! The first-run command, `supragnosis setup`: what it observes and the acts it runs. The
//! decisions and their words are in setup.rs.

use super::*;

// --- First run: `supragnosis setup` (docs/onboarding.md) ---------------------------------------
// The decisions and their words are in setup.rs; what is here looks (the lifecycle's observation,
// `connect --json`, the viewer socket) and acts by running this binary's own subcommands, so what a
// person would have read in a terminal is what reaches the app's log drawer.

/// Where setup writes: one JSON object per line for the app, or rows for a person. A test keeps the
/// lines instead of printing them.
pub(crate) struct SetupOut {
    events: bool,
    kept: Option<std::cell::RefCell<Vec<String>>>,
}

impl SetupOut {
    fn new(events: bool) -> Self {
        SetupOut { events, kept: None }
    }
    fn line(&self, line: String) {
        match &self.kept {
            Some(k) => k.borrow_mut().push(line),
            None => println!("{line}"),
        }
    }
    fn emit(&self, e: &setup::Event) {
        if self.events {
            self.line(serde_json::to_string(e).unwrap_or_default());
        } else if let Some(line) = &e.out {
            self.line(format!("    {line}"));
        } else if e.cmd.is_none() {
            self.line(setup::text_row(e));
        }
    }
    fn list(&self, list: &[setup::Event]) {
        for e in list {
            self.emit(e);
        }
    }
}

/// An act setup can take for a step that is `todo`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SetupAct {
    Server,
    App(String),
    Model,
}

impl SetupAct {
    fn describe(&self, o: &setup::Observed) -> String {
        match self {
            SetupAct::Server => "keep the server running, now and at every login".to_string(),
            SetupAct::App(id) => format!(
                "connect {}",
                o.apps.iter().find(|a| &a.id == id).map_or(id.as_str(), |a| a.name.as_str())
            ),
            SetupAct::Model => match o.model {
                setup::Model::Missing(mb) => format!("download the search model ({mb} MB)"),
                _ => "download the search model".to_string(),
            },
        }
    }
}

/// The plan: every step the checklist says setup can do, in the order a terminal runs them - the
/// model last, since nothing waits for it (Section 4.3).
pub(crate) fn setup_plan(list: &[setup::Event]) -> Vec<SetupAct> {
    use setup::State;
    let mut plan = Vec::new();
    if list.iter().any(|e| e.step == "server" && e.state == State::Todo) {
        plan.push(SetupAct::Server);
    }
    for e in list.iter().filter(|e| e.step == "app" && e.state == State::Todo) {
        plan.extend(e.id.clone().map(SetupAct::App));
    }
    if list.iter().any(|e| e.step == "model" && e.state == State::Todo) {
        plan.push(SetupAct::Model);
    }
    plan
}

pub(crate) fn setup_cmd(a: SetupArgs) -> Result<()> {
    let out = SetupOut::new(a.events);
    if a.events {
        println!("{}", serde_json::json!({ "schema": setup::EVENTS_SCHEMA }));
    }
    if let Some(step) = a.step.clone() {
        let ok = setup_step(&a, &out, &step)?;
        return if ok { Ok(()) } else { std::process::exit(1) };
    }
    let observed = setup_observe(a.app_daemon)?;
    let list = setup::checklist(&observed);
    if a.check {
        out.list(&list);
        out.emit(&setup::summary(&list));
        return Ok(());
    }
    let plan = setup_plan(&list);
    if !out.events {
        println!("supragnosis setup - from install to the first entry\n");
        out.list(&list);
        println!();
    }
    if plan.is_empty() {
        out.emit(&setup::summary(&list));
        setup_next(&out, &list);
        return Ok(());
    }
    let chosen = if a.yes {
        plan
    } else {
        if a.events {
            anyhow::bail!(
                "with --events setup asks nothing: add --yes to apply the plan, or --check to only read"
            );
        }
        setup_ask(&observed, plan)?
    };
    if chosen.is_empty() {
        out.emit(&setup::summary(&list));
        return Ok(());
    }
    let mut ok = true;
    for act in &chosen {
        let step = match act {
            SetupAct::Server => "server".to_string(),
            SetupAct::App(id) => format!("app={id}"),
            SetupAct::Model => "model".to_string(),
        };
        ok &= setup_step(&a, &out, &step)?;
    }
    let list = setup::checklist(&setup_observe(a.app_daemon)?);
    if !out.events {
        println!();
        out.list(&list);
        println!();
    }
    out.emit(&setup::summary(&list));
    setup_next(&out, &list);
    if ok {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

/// Shows the plan and asks, as rustup's installer does: Enter applies it, `n` leaves, `c` asks act by
/// act. Asking needs a terminal; without one, `--yes` or `--check` says which the caller meant.
pub(crate) fn setup_ask(o: &setup::Observed, plan: Vec<SetupAct>) -> Result<Vec<SetupAct>> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "setup asks before it acts, and there is no terminal to ask in: add --yes to apply the plan, or --check to only read"
        );
    }
    println!("Setup will:");
    for act in &plan {
        println!("  - {}", act.describe(o));
    }
    // An answer is read per question. The end of input answers no, never the default: a closed
    // terminal or a short script must not consent to a download (or anything else) for the person.
    let read = |q: &str| -> Result<Option<String>> {
        print!("{q}");
        std::io::stdout().flush()?;
        let mut s = String::new();
        if std::io::stdin().lock().read_line(&mut s)? == 0 {
            println!();
            return Ok(None);
        }
        Ok(Some(s.trim().to_ascii_lowercase()))
    };
    let yes = |a: &Option<String>| matches!(a.as_deref(), Some("" | "y" | "yes"));
    let answer = read("Proceed? [Y/n, c to choose] ")?;
    if answer.as_deref() == Some("c") {
        let mut chosen = Vec::new();
        for act in plan {
            let a = read(&format!("  {}? [Y/n] ", act.describe(o)))?;
            if a.is_none() {
                break;
            }
            if yes(&a) {
                chosen.push(act);
            }
        }
        return Ok(chosen);
    }
    if yes(&answer) {
        return Ok(plan);
    }
    println!("Nothing was changed. `supragnosis setup` shows this plan again.");
    Ok(Vec::new())
}

/// What to do next in a terminal, once the acts are done: the question to ask, and how to wait.
pub(crate) fn setup_next(out: &SetupOut, list: &[setup::Event]) {
    if out.events {
        return;
    }
    if list.iter().any(|e| e.step == "first" && e.state == setup::State::Working) {
        println!(
            "\nNext: in a connected app, ask \"{}\"\n`supragnosis setup --step wait` waits for the entry, and the viewer shows it.",
            setup::TRY_IT
        );
    }
}

/// One step, applied: `server`, `app=<id>`, `model` or `wait`. Re-reads the state first, does
/// nothing to a step that is not `todo`, and ends with the step's state as it is afterwards. `false`
/// when the step failed or could not be applied (O2).
pub(crate) fn setup_step(a: &SetupArgs, out: &SetupOut, step: &str) -> Result<bool> {
    use setup::{Event, State};
    let observed = setup_observe(a.app_daemon)?;
    let list = setup::checklist(&observed);
    let (name, id) = match step.split_once('=') {
        Some(("app", id)) => ("app", Some(id)),
        None if matches!(step, "server" | "model" | "wait") => (step, None),
        _ => anyhow::bail!("unknown step {step:?} - one of: server, app=<id>, model, wait"),
    };
    let row = |l: &[Event], name: &str, id: Option<&str>| {
        l.iter().find(|e| e.step == name && e.id.as_deref() == id).cloned()
    };
    if name == "wait" {
        return setup_wait(a, out);
    }
    let name: &'static str = if name == "app" {
        "app"
    } else if name == "server" {
        "server"
    } else {
        "model"
    };
    let Some(before) = row(&list, name, id) else {
        let ids: Vec<&str> = connect::CLIENTS.iter().map(|c| c.id()).collect();
        anyhow::bail!("unknown app {:?} - one of: {}", id.unwrap_or(""), ids.join(", "));
    };
    if before.state != State::Todo {
        let ok = matches!(before.state, State::Done | State::Skipped | State::Working);
        out.emit(&before);
        return Ok(ok);
    }
    let args: Vec<String> = match name {
        "server" => match setup::server_act(&observed.runner) {
            Ok(Some(args)) => args.into_iter().map(String::from).collect(),
            Ok(None) => {
                out.emit(&before);
                return Ok(true);
            }
            Err(why) => {
                out.emit(&Event::new("server", None, State::Attention, why));
                return Ok(false);
            }
        },
        "app" => {
            let id = id.unwrap_or_default();
            let mut v = vec!["connect".to_string(), id.to_string()];
            if a.no_capture && id == connect::Client::ClaudeCode.id() {
                v.push("--no-capture".to_string());
            }
            v
        }
        _ if out.events => vec!["embed".into(), "fetch".into(), "--events".into()],
        _ => vec!["embed".into(), "fetch".into()],
    };
    let exe = std::env::current_exe().context("the path of this binary")?;
    let ran = setup_run(out, &exe, name, id, &args)?;
    if let Err(why) = ran {
        let hint = match name {
            "server" => "`supragnosis status` says who runs the daemon.",
            "app" => "The app's settings are as they were, or the copy made first is named above.",
            _ => "Search matches words until the model is here; run setup again when online.",
        };
        out.emit(&Event::new(name, id, State::Failed, format!("{why} {hint}")));
        return Ok(false);
    }
    let after = setup::checklist(&setup_observe(a.app_daemon)?);
    let row_after = row(&after, name, id).unwrap_or(before);
    out.emit(&row_after);
    if name == "app" {
        if let Some(apps) = row(&after, "apps", None) {
            out.emit(&apps);
        }
    }
    Ok(row_after.state != State::Failed)
}

/// Runs `supragnosis <args>` - `exe`, this same binary. In a terminal its output is the person's to
/// read as it comes; with `--events` each line of it is an event naming the command, and a child that
/// writes events itself (`embed fetch --events`) has them passed on as they are. `Err` carries why
/// it failed: the child's last error line, or its exit status.
pub(crate) fn setup_run(
    out: &SetupOut,
    exe: &std::path::Path,
    step: &'static str,
    id: Option<&str>,
    args: &[String],
) -> Result<std::result::Result<(), String>> {
    use setup::{Event, State};
    use std::io::BufRead;
    let shown = format!("supragnosis {}", args.join(" "));
    if !out.events {
        println!("$ {shown}");
        let st = std::process::Command::new(exe)
            .args(args)
            .status()
            .with_context(|| format!("running {shown}"))?;
        return Ok(if st.success() { Ok(()) } else { Err(format!("`{shown}` failed ({st}).")) });
    }
    let running = format!("Running {shown}.");
    let mut start = Event::new(step, id, State::Working, running.clone());
    start.cmd = Some(shown.clone());
    out.emit(&start);
    let mut child = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("running {shown}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let pipes: Vec<(bool, Box<dyn std::io::Read + Send>)> = vec![
        (false, Box::new(child.stdout.take().context("the child's stdout")?)),
        (true, Box::new(child.stderr.take().context("the child's stderr")?)),
    ];
    for (is_err, pipe) in pipes {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = tx.send((is_err, line));
            }
        });
    }
    drop(tx);
    let mut last_err = String::new();
    for (is_err, line) in rx {
        if line.trim().is_empty() {
            continue;
        }
        if is_err {
            last_err = line.trim().trim_start_matches("Error: ").to_string();
        }
        match serde_json::from_str::<serde_json::Value>(&line) {
            // A child's own events, passed on; its schema line is this run's already.
            Ok(v) if v.get("step").is_some() => out.line(v.to_string()),
            Ok(v) if v.get("schema").is_some() => {}
            _ => {
                let mut e = Event::new(step, id, State::Working, running.clone());
                e.out = Some(line);
                out.emit(&e);
            }
        }
    }
    let st = child.wait().with_context(|| format!("waiting for {shown}"))?;
    Ok(if st.success() {
        Ok(())
    } else if last_err.is_empty() {
        Err(format!("`{shown}` failed ({st})."))
    } else {
        Err(format!("`{shown}` failed: {last_err}."))
    })
}

/// `--step wait` (Section 4.4): polls until the store holds an entry, for `--timeout` seconds. Exits
/// 0 when one is there and 2 when none came, which is not a failure; it writes nothing (O1).
pub(crate) fn setup_wait(a: &SetupArgs, out: &SetupOut) -> Result<bool> {
    use setup::{Event, FirstEntry, State};
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(a.timeout);
    let mut said = false;
    let mut checked = false;
    loop {
        match newest_entry() {
            entry @ FirstEntry::Some { .. } => {
                // The checklist's sentence, and the entry's own words under it (Section 29.10).
                let e = setup::entry_event(&entry).expect("an entry");
                out.emit(&e);
                if !out.events {
                    out.line(format!("      \"{}\"", e.quote.as_deref().unwrap_or_default()));
                }
                return Ok(true);
            }
            _ if said && !checked && started.elapsed().as_secs() >= 120 => {
                // Two minutes and nothing: what to check, as the page shows it then.
                checked = true;
                let mut e = Event::new(
                    "first",
                    None,
                    State::Working,
                    "Nothing yet after 2 minutes. Check:",
                );
                e.checks = setup::TRY_IT_CHECKS.iter().map(|c| c.to_string()).collect();
                out.emit(&e);
                if !out.events {
                    for c in &e.checks {
                        out.line(format!("      - {c}"));
                    }
                }
            }
            _ if !said => {
                said = true;
                out.emit(&Event::new(
                    "first",
                    None,
                    State::Working,
                    format!(
                        "Waiting for the first entry. Ask a connected app: \"{}\"",
                        setup::TRY_IT
                    ),
                ));
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            out.emit(&Event::new(
                "first",
                None,
                State::Todo,
                format!(
                    "No entry yet after {} seconds. It appears when a connected app records one; `supragnosis setup --step wait` waits again.",
                    a.timeout
                ),
            ));
            std::process::exit(2);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// What setup reads before it decides anything, every time (O3).
pub(crate) fn setup_observe(app_daemon: Option<u32>) -> Result<setup::Observed> {
    let env = connect::Env::from_process();
    let observed = observe();
    let situation = lifecycle::classify(&observed);
    let app_child = app_daemon
        .is_some_and(|p| i32::try_from(p).is_ok_and(|p| pid_alive(p) && pid_is_supragnosis(p)));
    // The login manager setup can install into: the LaunchAgent, the user unit where a user manager
    // answers, and none where it does not (docs/onboarding.md Section 4.1).
    let platform = if cfg!(target_os = "macos") {
        setup::Platform::Launchd
    } else if systemd_user_available().is_ok() {
        setup::Platform::Systemd { linger: linger_state() }
    } else {
        setup::Platform::Without
    };
    let runner = setup::runner(
        &situation,
        observed.answering,
        canonical_service_state(),
        app_child,
        platform,
    );
    // A daemon refused its store for the model it runs says why, with the way on (SR8): starting it
    // again would fail the same way.
    let runner = match (observed.answering, daemon_embedder_conflict()) {
        (false, Some((recorded, daemon))) => {
            setup::Runner::Attention(embedder_conflict_attention(&recorded, &daemon))
        }
        _ => runner,
    };

    let reported = if observed.answering { first_contacts() } else { None };
    let contacts_known = reported.is_some();
    let contacts: Vec<(String, u64)> = reported
        .as_ref()
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["name"].as_str()?.to_string(), c["first"].as_u64()?)))
        .collect();
    let now = supragnosis_core::now_millis();
    let doc = connect_document(&env, &bridge_program()?);
    let apps = doc["clients"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let id = c["id"].as_str().unwrap_or_default().to_string();
            setup::App {
                name: c["name"].as_str().unwrap_or_default().to_string(),
                installed: c["installed"].as_bool().unwrap_or(false),
                entry: c["entry"].as_str().unwrap_or("unknown").to_string(),
                next: connect::Client::parse(&id)
                    .map(|c| c.next_step())
                    .unwrap_or_default()
                    .to_string(),
                reached: contacts
                    .iter()
                    .filter(|(n, _)| setup::client_app(n) == Some(id.as_str()))
                    .map(|(_, first)| *first)
                    .min()
                    .map(|t| setup::ago(t, now)),
                id,
            }
        })
        .collect();
    let other_contacts = contacts
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| setup::client_app(n).is_none())
        .collect();
    let health = if observed.answering { store_health() } else { None };
    Ok(setup::Observed {
        runner,
        apps,
        contacts_known,
        other_contacts,
        model: setup_model(health.as_ref()),
        first: if observed.answering { newest_entry() } else { setup::FirstEntry::Unknown },
    })
}

/// The search model: the running daemon's own report when it answers (semantic-retrieval.md SR3),
/// and otherwise what this binary finds in the cache under its own environment.
pub(crate) fn setup_model(health: Option<&serde_json::Value>) -> setup::Model {
    let Some(h) = health else {
        return local_model();
    };
    match h.get("semantic") {
        Some(r) if r["state"].as_str() == Some("on") => setup::Model::On,
        Some(r) if r.is_object() => match local_model() {
            // Complete on disk and still off: the daemon tried it and it did not load.
            setup::Model::On => setup::Model::Broken(
                r["note"].as_str().unwrap_or("the search model is downloaded but did not load").to_string(),
            ),
            other => other,
        },
        _ => setup::Model::Skipped(
            "This server runs without a search model (SUPRAGNOSIS_EMBED=none, or a build without one), so search matches words."
                .to_string(),
        ),
    }
}

#[cfg(feature = "fastembed")]
pub(crate) fn local_model() -> setup::Model {
    let kind = std::env::var("SUPRAGNOSIS_EMBED").unwrap_or_default();
    if !matches!(kind.trim(), "" | "fastembed") {
        return setup::Model::Skipped(format!(
            "SUPRAGNOSIS_EMBED={} here, so there is no search model to fetch.",
            kind.trim()
        ));
    }
    match embed_model() {
        Ok(m) if m.present(&supragnosis_embed::model_cache_dir()) => setup::Model::On,
        Ok(m) => setup::Model::Missing(m.download_mb()),
        Err(e) => setup::Model::Broken(format!("{e:#}")),
    }
}

#[cfg(not(feature = "fastembed"))]
pub(crate) fn local_model() -> setup::Model {
    setup::Model::Skipped("This build has no search model, so search matches words.".to_string())
}

/// The newest entry in the store, read through the viewer socket across every workspace. A socket
/// that does not answer, or answers with an error, is unknown, never empty (P5).
pub(crate) fn newest_entry() -> setup::FirstEntry {
    let Some(body) = viz_get_within("/api/observations?workspace=*&limit=1", 10_000) else {
        return setup::FirstEntry::Unknown;
    };
    let Ok(serde_json::Value::Array(log)) = serde_json::from_str::<serde_json::Value>(&body) else {
        return setup::FirstEntry::Unknown;
    };
    let Some(o) = log.first() else {
        return setup::FirstEntry::None;
    };
    let mut said: String = o["content"].as_str().unwrap_or_default().chars().take(140).collect();
    if said.len() < o["content"].as_str().unwrap_or_default().len() {
        said.push_str("...");
    }
    // The local read names the app that wrote an attestation (design-system.md Section 29.8); the
    // first one that does is the entry's.
    let app = o["attestations"]
        .as_array()
        .and_then(|a| a.iter().find_map(|t| t["app"].as_str()))
        .map(str::to_string);
    let entity = o["entities"]
        .as_array()
        .and_then(|e| e.first())
        .and_then(|e| e["id"].as_str())
        .map(str::to_string);
    setup::FirstEntry::Some {
        workspace: o["workspace"].as_str().map(str::to_string),
        ago: setup::ago(o["hlc"]["wall"].as_u64().unwrap_or(0), supragnosis_core::now_millis()),
        said,
        app,
        entity,
    }
}

/// docs/onboarding.md: what setup reads, what it runs, and what it says when an act fails.
#[cfg(test)]
mod first_run {
    use super::*;

    /// This file's setup section, from its banner to its tests.
    fn section() -> &'static str {
        let src = include_str!("setup_cli.rs");
        let start = src.find("// --- First run: `supragnosis setup`").expect("the setup section");
        let end = start + src[start..].find("#[cfg(test)]").expect("its end");
        &src[start..end]
    }

    /// O1 and O6: setup reaches the daemon only through reads of the viewer socket, and acts only by
    /// running this binary's `service install`, `connect` and `embed fetch` - nothing that observes,
    /// syncs, adds a server, or replaces an entry `connect` did not write.
    #[test]
    fn setup_reads_the_daemon_and_runs_only_its_own_acts() {
        let s = section();
        let mut reads: Vec<&str> = s
            .match_indices("viz_get(\"")
            .chain(s.match_indices("viz_get_within(\""))
            .map(|(i, m)| {
                let r = &s[i + m.len()..];
                &r[..r.find('"').expect("a closing quote")]
            })
            .collect();
        reads.sort_unstable();
        assert_eq!(reads, ["/api/observations?workspace=*&limit=1"]);
        // The other reads go through the helpers `status` uses: /api/clients and /api/health.
        assert!(s.contains("first_contacts()") && s.contains("store_health()"));
        for helper in ["fn first_contacts(", "fn store_health("] {
            let src = include_str!("managers.rs");
            let body = &src[src.find(helper).expect(helper)..];
            let body = &body[..body.find("\n}\n").expect("its end")];
            assert_eq!(body.matches("viz_get(").count(), 1, "{helper} reads one path");
        }
        assert_eq!(s.matches("Command::new(").count(), 2, "a subprocess other than this binary");
        assert_eq!(s.matches("Command::new(exe)").count(), 2);
        let acts = &s[s.find("let args: Vec<String> = match name {").expect("the acts")..];
        let acts = &acts[..acts.find("let exe =").expect("their end")];
        assert!(acts.contains("setup::server_act(&observed.runner)"));
        assert!(acts.contains("\"connect\".to_string()") && acts.contains("\"embed\".into()"));
        for word in [
            "\"observe\"",
            "\"sync\"",
            "\"principal\"",
            "\"server\".into",
            "--replace",
            "--take-over",
        ] {
            assert!(!acts.contains(word), "setup runs `{word}`");
        }
    }

    /// O3: the plan is what the checklist says is `todo`, and only that - a step done, skipped or
    /// waiting on the person is not acted on - and the model comes last, since nothing waits on it
    /// (O5).
    #[test]
    fn the_plan_is_what_is_todo_and_the_model_goes_last() {
        use setup::{Event, State};
        let list = vec![
            Event::new("server", None, State::Done, ""),
            Event::new("apps", None, State::Todo, ""),
            Event::new("app", Some("cursor"), State::Attention, ""),
            Event::new("model", None, State::Todo, ""),
            Event::new("app", Some("codex"), State::Todo, ""),
            Event::new("app", Some("gemini"), State::Skipped, ""),
            Event::new("first", None, State::Working, ""),
        ];
        assert_eq!(setup_plan(&list), vec![SetupAct::App("codex".into()), SetupAct::Model]);
        let mut fresh = list.clone();
        fresh[0].state = State::Todo;
        assert_eq!(setup_plan(&fresh)[0], SetupAct::Server, "the server first");
        let done: Vec<Event> =
            list.into_iter().map(|e| Event { state: State::Done, ..e }).collect();
        assert!(setup_plan(&done).is_empty(), "running again changes nothing that is done");
    }

    /// O2: an act's command is named before it runs, each line it writes is an event (a file it
    /// changed among them), a child's own events pass through as they are, and a failure carries
    /// the child's reason.
    #[test]
    fn a_failed_act_says_why_and_names_what_it_ran() {
        let out = SetupOut { events: true, kept: Some(Default::default()) };
        let script = "echo '{\"schema\":1}'; \
                      echo '{\"step\":\"model\",\"state\":\"working\",\"say\":\"1 of 2 MB.\"}'; \
                      echo '  wrote   /x/mcp.json'; \
                      echo 'Error: the file could not be read' >&2; exit 3";
        let ran = setup_run(
            &out,
            std::path::Path::new("/bin/sh"),
            "app",
            Some("cursor"),
            &["-c".to_string(), script.to_string()],
        )
        .expect("it ran");
        let why = ran.expect_err("it failed");
        assert!(why.ends_with("failed: the file could not be read."), "{why}");
        let lines: Vec<serde_json::Value> = out
            .kept
            .expect("kept")
            .into_inner()
            .iter()
            .map(|l| serde_json::from_str(l).expect("JSON"))
            .collect();
        assert!(
            lines[0]["cmd"].as_str().is_some_and(|c| c.starts_with("supragnosis -c")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l["out"] == "  wrote   /x/mcp.json" && l["id"] == "cursor"));
        assert!(lines.iter().any(|l| l["step"] == "model" && l["say"] == "1 of 2 MB."));
        assert!(lines.iter().all(|l| l.get("schema").is_none()), "the child's schema line");
    }
}
