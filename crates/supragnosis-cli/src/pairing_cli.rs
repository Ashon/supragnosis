//! Pairing from the terminal: the `invite`, `join`, `hub` and `peer` subcommands and the admin
//! calls they make to the running daemon.

use super::*;

/// Tells a running daemon the hub's admissions changed, so its listener opens or closes as the bind
/// rule now allows (docs/pairing.md Section 4.1). With no daemon running there is nothing to tell.
/// What the admin calls answer when no daemon runs here.
pub(crate) fn no_daemon() -> anyhow::Error {
    anyhow::anyhow!(
        "No supragnosis daemon answers on this machine. Start it - `supragnosis start`, or open the \
         app - and try again."
    )
}

/// An admin call's answer, or its refusal as an error.
pub(crate) fn admin(
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let answer = match body {
        Some(b) => hub::call_with(method, path, b)?,
        None => hub::call(method, path)?,
    }
    .ok_or_else(no_daemon)?;
    if let Some(e) = answer.get("error").and_then(|e| e.as_str()) {
        anyhow::bail!("{e}");
    }
    Ok(answer)
}

pub(crate) fn ask_yes(question: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// `supragnosis invite` (docs/pairing.md Section 3a): asks the daemon for an invitation, prints the
/// code, and waits for a request - its words and Allow in this terminal. Leaving it leaves the
/// invitation open until it expires; `--show` comes back to it.
pub(crate) fn invite_cmd(a: InviteArgs) -> Result<()> {
    if a.cancel {
        admin("POST", "/hub/cancel", None)?;
        println!("Cancelled the open invitation.");
        return Ok(());
    }
    let made = match a.show {
        true => None,
        false => {
            let keep = a
                .keep
                .as_deref()
                .map(|k| {
                    k.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect()
                })
                .unwrap_or_default();
            let ask = hub::InviteAsk { workspaces: keep, addresses: a.address.clone() };
            Some(admin("POST", "/hub/invite", Some(&serde_json::to_value(ask)?))?)
        }
    };
    if a.json {
        let out = match &made {
            Some(m) => m.clone(),
            None => admin("GET", "/hub/invite", None)?,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    let (code, expires, keep, id) = match &made {
        Some(m) => (
            m["code"].as_str().map(str::to_string),
            m["expires"].as_u64(),
            m["workspaces"].clone(),
            m["id"].as_str().unwrap_or_default().to_string(),
        ),
        None => {
            let st = admin("GET", "/hub/invite", None)?;
            let open = &st["open"];
            if open.is_null() {
                match st["last"]["said"].as_str() {
                    Some(said) => println!("No invitation is open. The last one: {said}"),
                    None => println!("No invitation is open. `supragnosis invite --keep <workspaces>` makes one."),
                }
                return Ok(());
            }
            (
                open["code"].as_str().map(str::to_string),
                open["expires"].as_u64(),
                open["workspaces"].clone(),
                open["id"].as_str().unwrap_or_default().to_string(),
            )
        }
    };
    let left = expires.unwrap_or(0).saturating_sub(invite::now()).div_ceil(60);
    let keep: Vec<&str> =
        keep.as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
    if let Some(m) = &made {
        if m["replaced"] == true {
            println!("The invitation that was open is cancelled.");
        }
        if m["listener"]["listening"] != true {
            if let Some(why) = m["listener"]["why"].as_str() {
                println!("The hub is not listening yet: {why}");
            }
        }
        for note in m["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
            println!("{note}");
        }
    }
    match code {
        Some(code) => {
            println!("The code, good for one join and {left} more minute(s):");
            println!();
            println!("  {code}");
            println!();
        }
        None => println!(
            "The code can no longer be shown - the daemon restarted since it was made. It still \
             works for {left} more minute(s), or make a new one."
        ),
    }
    match keep.is_empty() {
        true => println!("It shares no workspace yet."),
        false => println!("The two machines will keep {} in step.", keep.join(", ")),
    }
    println!(
        "On the other machine: Settings > Sync > Join another Mac, or `supragnosis join` and paste it."
    );
    println!("Waiting for it to ask. Leaving here leaves the invitation open; `supragnosis invite --show` comes back.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let st = admin("GET", "/hub/invite", None)?;
        let open = &st["open"];
        if open.is_null() {
            let said = st["last"]["said"].as_str().unwrap_or("The invitation closed.");
            match (st["last"]["state"].as_str(), st["last"]["name"].as_str()) {
                (Some("used"), Some(name)) => println!("{name} joined."),
                _ => println!("{said}"),
            }
            return Ok(());
        }
        if open["id"].as_str() != Some(id.as_str()) {
            println!(
                "A newer invitation replaced this one; `supragnosis invite --show` follows it."
            );
            return Ok(());
        }
        let request = &open["request"];
        let (Some(name), Some(node), Some(words)) = (
            request["name"].as_str(),
            request["node_id"].as_str(),
            request["words"].as_array(),
        ) else {
            continue;
        };
        let words: Vec<String> =
            words.iter().filter_map(|w| w.as_str().map(str::to_string)).collect();
        // The words on a line of their own, apart from the name the joining machine chose.
        println!();
        println!("A machine named \"{name}\" asks to join. Its words:");
        println!();
        println!("    {}", words.join("   "));
        println!();
        // What the person answers is this request and these words: the hub refuses the answer
        // if the request has changed since (PA11).
        let answering = serde_json::json!({"invitation": id, "node_id": node, "words": words});
        if ask_yes("Are they the words the other machine shows? Allow it?")? {
            let joined = admin("POST", "/hub/allow", Some(&answering))?;
            let keep: Vec<&str> = joined["joined"]["workspaces"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|w| w.as_str())
                .collect();
            println!(
                "{name} joined.{}",
                match keep.is_empty() {
                    true => " It shares no workspace yet.".to_string(),
                    false => format!(" The two keep {} in step.", keep.join(", ")),
                }
            );
        } else {
            admin("POST", "/hub/deny", Some(&answering))?;
            println!("Denied. The invitation is closed; make a new code to try again.");
        }
        return Ok(());
    }
}

/// The terminal's side of a join: the preview's questions, and the words.
pub(crate) struct TerminalPerson {
    send: Option<Vec<String>>,
    json: bool,
}

/// What the hub's unsigned preview says, as a terminal may print it: no control characters, so a
/// name cannot move the cursor or rewrite the line.
pub(crate) fn printable(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

impl join::Person for TerminalPerson {
    fn preview(&mut self, hub: &str, workspaces: &[String]) -> Result<join::Choices> {
        let hub = printable(hub);
        let hub = hub.as_str();
        if self.json {
            println!(
                "{}",
                serde_json::json!({"preview": {"hub_name": hub, "workspaces": workspaces}})
            );
            return Ok(join::Choices { send: self.send.clone().unwrap_or_default() });
        }
        println!();
        match workspaces.is_empty() {
            true => println!("{hub} shares no workspace yet."),
            false => println!(
                "From {hub}, this machine will receive {}.",
                workspaces.iter().map(|w| printable(w)).collect::<Vec<_>>().join(", ")
            ),
        }
        let send = match &self.send {
            Some(send) => send.clone(),
            None => {
                let mut send = Vec::new();
                for w in workspaces {
                    if ask_yes(&format!("Also send {} from this machine to {hub}?", printable(w)))?
                    {
                        send.push(w.clone());
                    }
                }
                send
            }
        };
        Ok(join::Choices { send })
    }

    fn words(&mut self, hub: &str, words: &[&str]) {
        let hub = printable(hub);
        if self.json {
            println!("{}", serde_json::json!({"words": words, "hub_name": hub}));
            return;
        }
        println!();
        println!("This machine's words:");
        println!();
        println!("    {}", words.join("   "));
        println!();
        println!("Allow it on {hub} if it shows the same words. Waiting for {hub}...");
    }
}

/// `supragnosis join` (docs/pairing.md Section 4.3): the code on stdin, never as an argument, so it
/// never sits in a process list.
pub(crate) fn join_cmd(a: JoinArgs) -> Result<()> {
    use std::io::{BufRead, IsTerminal, Write};
    init_tracing();
    let interactive = std::io::stdin().is_terminal() && !a.json;
    if interactive {
        print!("Paste the code from the hub, then press Return: ");
        std::io::stdout().flush()?;
    }
    // A code pasted across lines is read until it is whole; from a pipe, all of it is the code.
    let code = if interactive {
        let mut text = String::new();
        let mut lines = 0;
        loop {
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line)? == 0 {
                break pair::Code::decode(&text)?;
            }
            text.push_str(&line);
            lines += 1;
            match pair::Code::decode(&text) {
                Ok(code) => break code,
                Err(pair::CodeError::Damaged) if lines < 6 => continue,
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
        pair::Code::decode(&text)?
    };
    let send = a
        .send
        .as_deref()
        .map(|s| s.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect());
    let mut person = TerminalPerson {
        send: send.or(if a.json { Some(Vec::new()) } else { None }),
        json: a.json,
    };
    let paths = join::Paths::default_paths();
    let me = fed::load_or_create_identity()?;
    let name = a.name.unwrap_or_else(|| invite::hub_name(&paths.config));
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    let joined = rt.block_on(join::join(
        &paths,
        &me,
        &name,
        &code,
        &mut person,
        std::time::Duration::from_secs(2),
    ));
    let joined = match joined {
        Ok(j) => j,
        Err(e) if a.json => {
            println!("{}", serde_json::json!({"error": format!("{e:#}")}));
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    // `[sync]` is read at start: a running daemon restarts to sync with the hub.
    let restarted = restart_the_answering_daemon();
    if a.json {
        println!("{}", serde_json::json!({"joined": joined, "daemon": restarted}));
        return Ok(());
    }
    println!();
    println!("Joined {}.", joined.hub_name);
    match joined.sent.is_empty() {
        true => println!(
            "This machine receives {} and sends nothing yet.",
            list_or_none(&joined.receive)
        ),
        false => println!(
            "This machine receives {} and sends {}.",
            list_or_none(&joined.receive),
            joined.sent.join(", ")
        ),
    }
    match restarted {
        "restarted" => {
            println!("Supragnosis restarted and syncs with {} on its own.", joined.hub_name)
        }
        "not_restarted" => println!("Restart Supragnosis to start syncing."),
        _ => println!("Start Supragnosis - `supragnosis start`, or the app - to start syncing."),
    }
    Ok(())
}

pub(crate) fn list_or_none(list: &[String]) -> String {
    match list.is_empty() {
        true => "no workspace yet".into(),
        false => list.join(", "),
    }
}

/// Restarts the daemon that answers this user's admin socket - and only when it is the one this
/// CLI's service manager runs, so a join made under another HOME, or a daemon started by hand,
/// never restarts a different one. Says "restarted", "not_restarted" or "not_running".
pub(crate) fn restart_the_answering_daemon() -> &'static str {
    let Ok(Some(who)) = hub::call("GET", "/who") else {
        return "not_running";
    };
    let answering = who["pid"].as_u64();
    let managed = match lifecycle::classify(&observe()) {
        lifecycle::Situation::One(lifecycle::Manager::Pidfile { pid }) => Some(pid),
        lifecycle::Situation::One(lifecycle::Manager::Launchd(job))
        | lifecycle::Situation::One(lifecycle::Manager::Systemd(job)) => job.pid,
        _ => None,
    };
    if answering.is_none() || managed.map(u64::from) != answering {
        return "not_restarted";
    }
    match restart(resolve(RunArgs::default(), true)) {
        Ok(()) => "restarted",
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the daemon did not restart");
            "not_restarted"
        }
    }
}

/// `supragnosis hub` (docs/pairing.md Section 4.6).
pub(crate) fn hub_cmd(cmd: HubCmd) -> Result<()> {
    init_tracing();
    match cmd {
        HubCmd::Leave { yes } => {
            let paths = join::Paths::default_paths();
            let hubs = join::joined(&paths)?;
            let Some((_, car)) = hubs.first() else {
                anyhow::bail!("This machine joined no hub through pairing.");
            };
            println!(
                "This machine stops syncing with {} now. What each already holds stays on it.",
                car.name
            );
            if !yes && !ask_yes(&format!("Stop syncing with {}?", car.name))? {
                println!("Nothing changed.");
                return Ok(());
            }
            let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
            let left = rt.block_on(join::leave(&paths))?;
            match left.hub_answered {
                true => println!("{} removed this machine.", left.hub_name),
                false => println!(
                    "{} did not answer, so it keeps this machine's row until it is removed there \
                     (`supragnosis peer remove`). This machine no longer syncs with it.",
                    left.hub_name
                ),
            }
            if restart_the_answering_daemon() == "not_restarted" {
                println!("Restart Supragnosis to stop the rounds it runs.");
            }
            Ok(())
        }
    }
}

/// `supragnosis peer` (docs/pairing.md Section 4.6).
pub(crate) fn peer_cmd(cmd: PeerCmd) -> Result<()> {
    match cmd {
        PeerCmd::List { json } => {
            let peers = invite::peers_json(&invite::Paths::default_paths())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&peers)?);
                return Ok(());
            }
            let joined = peers["joined"].as_array().cloned().unwrap_or_default();
            if joined.is_empty() {
                println!("No machine has joined this one. `supragnosis invite` lets one join.");
            }
            for j in &joined {
                let keep: Vec<&str> = j["workspaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str())
                    .collect();
                println!(
                    "  {:<20} {}  keeps: {}",
                    j["name"].as_str().unwrap_or("?"),
                    j["node_id"].as_str().unwrap_or("?"),
                    if keep.is_empty() { "nothing yet".to_string() } else { keep.join(", ") }
                );
            }
            Ok(())
        }
        PeerCmd::Remove { name, yes } => {
            println!(
                "{name} stops syncing with this machine now. What it already received stays on it."
            );
            if !yes && !ask_yes(&format!("Remove {name}?"))? {
                println!("Nothing changed.");
                return Ok(());
            }
            let out = admin("POST", "/hub/peer/remove", Some(&serde_json::json!({"name": name})))?;
            println!("Removed {}.", out["removed"].as_str().unwrap_or(&name));
            Ok(())
        }
    }
}
