//! Connecting AI apps: the `connect`, `hook` and `bridge` subcommands, the capture hook, and
//! the listing `connect` prints.

use super::*;

/// Initializes the stderr log subscriber (idempotent). stdout is the MCP stdio channel, so logs must go to stderr.
/// The program an AI app launches as `<program> bridge`: the stable path, so a Homebrew upgrade does
/// not leave the app pointing at a removed keg (client-connect.md Section 4).
pub(crate) fn bridge_program() -> Result<String> {
    let exe = std::env::current_exe()?.canonicalize()?;
    Ok(lifecycle::stable_program(&exe, |p| p.exists()).to_string_lossy().to_string())
}

/// `supragnosis connect [app]` (docs/client-connect.md Section 4).
pub(crate) fn connect_cmd(a: ConnectArgs) -> Result<()> {
    use connect::{Client, Entry, CLIENTS};
    let env = connect::Env::from_process();
    let program = bridge_program()?;
    let Some(id) = a.client else {
        return connect_list(&env, &program, a.json);
    };
    let client = Client::parse(&id).with_context(|| {
        let ids: Vec<&str> = CLIENTS.iter().map(|c| c.id()).collect();
        format!("unknown app {id:?} - one of: {}", ids.join(", "))
    })?;
    if !client.installed(&env) {
        anyhow::bail!("{} is not installed on this machine", client.name());
    }
    let before = client.entry(&env);
    if a.remove {
        let hook_gone = client == Client::ClaudeCode && capture_remove(&env)?;
        if before == Entry::None {
            if !hook_gone {
                println!("{} has no supragnosis entry - nothing to remove", client.name());
            }
            return Ok(());
        }
        connect_remove(client, &env)?;
        println!("removed the supragnosis entry from {}", client.name());
        return Ok(());
    }
    match connect::plan_connect(before, a.replace) {
        Ok(connect::Plan::AlreadyConnected) => {
            println!("{} is already connected through the bridge", client.name());
            match capture_act(client, a.no_capture) {
                Some(true) => capture_add(&env, &program)?,
                Some(false) if !capture_remove(&env)? => {
                    println!("Claude Code has no capture hook - nothing to remove");
                }
                Some(false) | None => {}
            }
            return Ok(());
        }
        Err(Entry::Unknown) => anyhow::bail!(
            "{} could not be read - not touching a file this cannot parse (client-connect.md C3)",
            files_named(client, &env)
        ),
        Err(other) => anyhow::bail!(
            "{} already has a supragnosis entry ({}){} - re-run with --replace to switch it to the bridge",
            client.name(),
            other.as_str(),
            if other == Entry::Http { ", which holds a copy of the token" } else { "" }
        ),
        Ok(connect::Plan::Add) => {}
        Ok(connect::Plan::Replace(other)) => {
            connect_remove(client, &env)?;
            println!("removed the previous supragnosis entry ({})", other.as_str());
            if other == Entry::Http {
                println!("  the copy of the token it held is gone with it");
            }
        }
    }
    if program.contains("/target/debug/") || program.contains("/target/release/") {
        println!(
            "note: the app will launch a build-tree binary ({program}); `cargo clean` removes it"
        );
    }
    match connect::add_argv(client, &program) {
        Some(argv) => run_client_cli(client, &env, &argv)?,
        None => {
            let (path, section) = client.config(&env);
            let text = connect::read_config(&path).with_context(|| {
                format!("{} could not be read - not touching it", path.display())
            })?;
            let edited =
                connect::upsert(&text, section, connect::NAME, &connect::bridge_entry(&program))
                    .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            let copy = connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                .with_context(|| format!("writing {}", path.display()))?;
            if let Some(b) = copy {
                println!("  backup  {}", b.display());
            }
            // Every file setup changes is named where it ran (docs/onboarding.md O2).
            println!("  wrote   {}", path.display());
        }
    }
    let after = client.entry(&env);
    if after != Entry::Bridge {
        anyhow::bail!(
            "{} did not take the entry - it reads as {} in {}",
            client.name(),
            after.as_str(),
            files_named(client, &env)
        );
    }
    println!("connected {} -> {program} bridge", client.name());
    if client == Client::ClaudeCode && !a.no_capture {
        capture_add(&env, &program)?;
    }
    println!("  next    {}", client.next_step());
    // The entry is written either way, and the app fails until a server answers - said here rather
    // than at the app's first tool call (docs/onboarding.md Section 4.2).
    if !port_open(&status_http_addr()) {
        println!(
            "  warning no daemon answers at http://{}/mcp, so {} cannot reach it yet - `supragnosis setup` starts one and keeps it running",
            status_http_addr(),
            client.name()
        );
    }
    Ok(())
}

/// Claude Code's settings as text, refusing a file that exists and cannot be read (C3).
pub(crate) fn claude_code_settings_text(
    env: &connect::Env,
) -> Result<(std::path::PathBuf, String)> {
    let path = connect::claude_code_settings(env);
    let text = connect::read_config(&path)
        .with_context(|| format!("{} could not be read - not touching it", path.display()))?;
    Ok((path, text))
}

/// Registers the capture hook in Claude Code's settings (client-connect.md Section 12.4), unless
/// it is there already. The file is copied aside first and only the hook's own groups are added.
pub(crate) fn capture_add(env: &connect::Env, program: &str) -> Result<()> {
    let (path, mut text) = claude_code_settings_text(env)?;
    match connect::capture_state(&text) {
        connect::Capture::On => {
            println!("  capture is on ({})", path.display());
            return Ok(());
        }
        connect::Capture::Unknown => anyhow::bail!(
            "{} could not be parsed - not touching a file this cannot parse (client-connect.md C3); \
             re-run with --no-capture to connect without the hook",
            path.display()
        ),
        connect::Capture::Off => {}
    }
    for event in connect::HOOK_EVENTS {
        text = connect::add_hook(&text, event, &connect::hook_group(event, program))
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    }
    let copy =
        connect::write_backed_up(&path, "claude-code-settings", &env.home, unix_now(), &text)
            .with_context(|| format!("writing {}", path.display()))?;
    if let Some(b) = copy {
        println!("  backup  {}", b.display());
    }
    println!(
        "  capture on: after a commit the agent is asked for the decision behind it, and a session \
         and each subagent start with the workspace's newest records ({})",
        path.display()
    );
    Ok(())
}

/// Takes the capture hook out of Claude Code's settings. `Ok(true)` when something was removed.
/// What `connect` does to Claude Code's capture hook on an app already connected through the bridge:
/// adds it, or with `--no-capture` removes it and keeps the bridge (client-connect.md Section 12.4),
/// so the Settings page's capture switch is one command spelled two ways. `None` for any other app.
pub(crate) fn capture_act(client: connect::Client, no_capture: bool) -> Option<bool> {
    (client == connect::Client::ClaudeCode).then_some(!no_capture)
}

pub(crate) fn capture_remove(env: &connect::Env) -> Result<bool> {
    let (path, text) = claude_code_settings_text(env)?;
    if connect::capture_state(&text) == connect::Capture::Unknown {
        anyhow::bail!(
            "{} could not be parsed - not touching a file this cannot parse (client-connect.md C3)",
            path.display()
        );
    }
    let mut edited: Option<String> = None;
    for event in connect::HOOK_EVENTS {
        let current = edited.as_deref().unwrap_or(&text);
        if let Some(next) = connect::remove_hook(current, event)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        {
            edited = Some(next);
        }
    }
    let Some(edited) = edited else {
        return Ok(false);
    };
    let copy =
        connect::write_backed_up(&path, "claude-code-settings", &env.home, unix_now(), &edited)
            .with_context(|| format!("writing {}", path.display()))?;
    if let Some(b) = copy {
        println!("  backup  {}", b.display());
    }
    println!("removed the capture hook from Claude Code's settings");
    Ok(true)
}

/// `supragnosis hook <client>`: the hook event on stdin, the answer on stdout, nothing else
/// (docs/client-connect.md Section 12). Silence is exit 0 with no output; an input this cannot
/// read is an error on stderr, which Claude Code shows and carries on from.
pub(crate) fn hook_cmd(a: HookArgs) -> Result<()> {
    if a.client != "claude-code" {
        anyhow::bail!(
            "unknown hook client {:?} - only claude-code has one (docs/client-connect.md Section 12)",
            a.client
        );
    }
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)
        .context("reading the hook event from stdin")?;
    let input: serde_json::Value =
        serde_json::from_str(&raw).context("the hook event on stdin is not JSON")?;
    let git = |dir: &std::path::Path, args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let world = hook::World {
        now: unix_now(),
        git: &git,
        daemon: &viz_get,
        workspace_override: std::env::var("SUPRAGNOSIS_WORKSPACE").ok(),
    };
    if let Some(v) = hook::answer(&input, &world).map_err(|e| anyhow::anyhow!(e))? {
        println!("{v}");
    }
    Ok(())
}

/// The files a client's entry is read from, for a message: one, or OpenCode's two.
pub(crate) fn files_named(client: connect::Client, env: &connect::Env) -> String {
    let files: Vec<String> = client.files(env).iter().map(|f| f.display().to_string()).collect();
    files.join(" or ")
}

pub(crate) fn connect_remove(client: connect::Client, env: &connect::Env) -> Result<()> {
    match connect::remove_argv(client) {
        Some(argv) => run_client_cli(client, env, &argv),
        // OpenCode reads two files, so the entry goes from each that holds it. Both are read and
        // edited before either is written: a file that does not parse stops the removal with
        // neither touched (client-connect.md Section 13.3).
        None if client == connect::Client::OpenCode => {
            let mut edits = Vec::new();
            for path in connect::opencode_files(env) {
                let text = connect::read_config(&path).with_context(|| {
                    format!("{} could not be read - not touching it", path.display())
                })?;
                if let Some(edited) =
                    connect::remove_jsonc(&text, &connect::OPENCODE_SECTION, connect::NAME)
                        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
                {
                    edits.push((path, edited));
                }
            }
            for (path, edited) in edits {
                let copy =
                    connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                        .with_context(|| format!("writing {}", path.display()))?;
                if let Some(b) = copy {
                    println!("  backup  {}", b.display());
                }
                println!("  wrote   {}", path.display());
            }
            Ok(())
        }
        None => {
            let (path, section) = client.config(env);
            let text = connect::read_config(&path).with_context(|| {
                format!("{} could not be read - not touching it", path.display())
            })?;
            let Some(edited) = connect::remove(&text, section, connect::NAME)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
            else {
                return Ok(());
            };
            let copy = connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                .with_context(|| format!("writing {}", path.display()))?;
            if let Some(b) = copy {
                println!("  backup  {}", b.display());
            }
            Ok(())
        }
    }
}

pub(crate) fn run_client_cli(
    client: connect::Client,
    env: &connect::Env,
    argv: &[String],
) -> Result<()> {
    let cli = client
        .cli(env)
        .with_context(|| format!("{}'s command-line tool was not found", client.name()))?;
    let out = std::process::Command::new(&cli)
        .args(argv)
        .env("PATH", env.path_var())
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running {}", cli.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!(
            "{} {} failed: {}",
            cli.display(),
            argv.first().map(String::as_str).unwrap_or(""),
            if err.trim().is_empty() {
                msg.trim().to_string()
            } else {
                err.trim().to_string()
            }
        );
    }
    // The command that changed the app's settings, since no file of ours names the change
    // (docs/onboarding.md Section 4.2, O2).
    println!("  ran     {} {}", cli.display(), argv.join(" "));
    Ok(())
}

/// The schema of every machine-read CLI answer: `status --json` and `connect --json`
/// (docs/compatibility.md Section 6). Adding a field keeps it. Removing or renaming
/// one, changing its type or what a value means raises it, and the desktop app - which reads these -
/// then says to update itself instead of misreading the answer. Each has an example document in
/// tests/fixtures/json that this crate's tests and the app's both read.
pub(crate) const JSON_SCHEMA: u64 = 1;

/// `connect --json`: every AI app this build knows, and how each is connected.
pub(crate) fn connect_document(env: &connect::Env, program: &str) -> serde_json::Value {
    use connect::{Entry, CLIENTS};
    let clients: Vec<_> = CLIENTS
        .iter()
        .map(|c| {
            let installed = c.installed(env);
            let entry = if installed { c.entry(env) } else { Entry::None };
            let capture = match c {
                connect::Client::ClaudeCode => {
                    serde_json::Value::String(capture_state_of(env).as_str().to_string())
                }
                _ => serde_json::Value::Null,
            };
            serde_json::json!({
                "id": c.id(), "name": c.name(), "installed": installed,
                "entry": entry.as_str(), "config": c.config(env).0,
                "via": match c.via() { connect::Via::Cli => "cli", connect::Via::File => "file" },
                "capture": capture,
            })
        })
        .collect();
    serde_json::json!({ "schema": JSON_SCHEMA, "program": program, "clients": clients })
}

pub(crate) fn connect_list(env: &connect::Env, program: &str, json: bool) -> Result<()> {
    print!("{}", connect_listing(env, program, json));
    Ok(())
}

/// What `connect` without an app prints, built by reading and nothing else (C5).
pub(crate) fn connect_listing(env: &connect::Env, program: &str, json: bool) -> String {
    use connect::{Entry, CLIENTS};
    use std::fmt::Write as _;
    if json {
        return format!("{}\n", connect_document(env, program));
    }
    let rows: Vec<_> = CLIENTS
        .iter()
        .map(|c| (*c, c.installed(env), if c.installed(env) { c.entry(env) } else { Entry::None }))
        .collect();
    let mut out = String::from(
        "AI apps - connect one with `supragnosis connect <app>`, or from the Supragnosis menu\n",
    );
    for (c, installed, entry) in rows {
        let state = match (installed, entry) {
            (false, _) => "not installed".to_string(),
            (true, Entry::None) => "not connected".to_string(),
            (true, Entry::Bridge) if c == connect::Client::ClaudeCode => {
                format!("connected (bridge), capture {}", capture_state_of(env).as_str())
            }
            (true, Entry::Bridge) => "connected (bridge)".to_string(),
            (true, Entry::Http) => format!(
                "connected over HTTP, holding a copy of the token - `supragnosis connect {} --replace` moves it to the bridge",
                c.id()
            ),
            (true, Entry::Stdio) => format!(
                "connected to the store-opening stdio server, which cannot run beside the daemon - `supragnosis connect {} --replace`",
                c.id()
            ),
            (true, Entry::Other) => "has another entry named supragnosis".to_string(),
            (true, Entry::Unknown) => format!("its config could not be read ({})", files_named(c, env)),
        };
        let _ = writeln!(out, "  {:<15} {:<15} {state}", c.id(), c.name());
    }
    out
}

/// What Claude Code's settings say about the capture hook, read and nothing else (C5).
pub(crate) fn capture_state_of(env: &connect::Env) -> connect::Capture {
    match connect::read_config(&connect::claude_code_settings(env)) {
        Ok(text) => connect::capture_state(&text),
        Err(_) => connect::Capture::Unknown,
    }
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `supragnosis bridge`: stdout carries JSON-RPC and nothing else, so nothing here logs to it. It
/// reaches this machine's loopback daemon; remote servers were removed in 0.11.0 (remote-server.md
/// Section 13).
pub(crate) fn bridge_cmd() -> Result<()> {
    let addr = std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    let addr = parse_loopback_addr(&addr)?; // loopback only, as `serve` binds (P17)
    let cfg = bridge::Config {
        url: format!("http://{addr}/mcp"),
        // Read on every request, never copied anywhere (client-connect.md C2).
        token: Arc::new(|| read_secret(&mcp_token_path())),
        wait: std::time::Duration::from_secs(10),
        unreachable_hint: bridge::NOT_RUNNING.to_string(),
        token_source: mcp_token_path().display().to_string(),
    };
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(bridge::run(cfg, tokio::io::stdin(), tokio::io::stdout()))
}

#[cfg(test)]
mod capture_switch_tests {
    use super::{capture_act, connect::Client};

    /// client-connect.md Section 12.4: on an app already connected through the bridge, `connect
    /// claude-code` adds the capture hook and `--no-capture` removes it, which is the Settings page's
    /// switch (design-system.md Section 6a); any other app has no hook to change.
    #[test]
    fn the_capture_switch_is_connect_spelled_two_ways() {
        assert_eq!(capture_act(Client::ClaudeCode, false), Some(true));
        assert_eq!(capture_act(Client::ClaudeCode, true), Some(false));
        assert_eq!(capture_act(Client::Cursor, false), None);
        assert_eq!(capture_act(Client::Cursor, true), None);
    }
}
