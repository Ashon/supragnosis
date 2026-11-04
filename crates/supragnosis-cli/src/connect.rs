//! `supragnosis connect` - registers the bridge with an AI app (docs/client-connect.md Section 4).
//!
//! Through the app's own CLI where it has one (Claude Code, Codex, Gemini, OpenCode, VS Code's
//! `--add-mcp`), and by editing its config file only where it has none (Claude Desktop, Cursor, and
//! removal from VS Code and OpenCode). A file edit changes one member and leaves every other byte as
//! written - an app's config is its user's file (P24) - and is checked after the fact: the result
//! must parse, carry the intended entry, and agree with the original everywhere else, or nothing is
//! written. OpenCode's files are JSONC (Section 13.3); every other JSON file stays strict.

use std::path::{Path, PathBuf};

use serde_json::Value;

mod clients;
mod json_edit;

pub use clients::*;
pub use json_edit::*;

/// What a client's `supragnosis` entry is, read from its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// No `supragnosis` entry.
    None,
    /// `<supragnosis> bridge` - what `connect` writes.
    Bridge,
    /// An HTTP entry: the token has been copied into the client.
    Http,
    /// The store-opening stdio server, which cannot run beside the daemon.
    Stdio,
    /// Something named `supragnosis` that is none of the above.
    Other,
    /// The configuration exists but could not be read (P5: not the same as no entry).
    Unknown,
}

impl Entry {
    /// How much an entry asks of the person, for an app that reads two files (OpenCode, Section
    /// 13.3): where both hold one, the listing reports the higher, since either may be the one the
    /// app loads.
    fn attention(self) -> u8 {
        match self {
            Entry::None => 0,
            Entry::Bridge => 1,
            Entry::Other => 2,
            Entry::Stdio => 3,
            Entry::Http => 4,
            Entry::Unknown => 5,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Entry::None => "none",
            Entry::Bridge => "bridge",
            Entry::Http => "http",
            Entry::Stdio => "stdio",
            Entry::Other => "other",
            Entry::Unknown => "unknown",
        }
    }
}

/// Classifies one server entry, in any client's shape (`command`/`args`, one `command` array, or
/// `url`/`type`). TOML entries (Codex) are converted to JSON values before they get here.
pub fn classify(entry: &Value) -> Entry {
    // An entry turned off does not load, whatever it runs (OpenCode's `"enabled": false`).
    if entry.get("enabled") == Some(&Value::Bool(false)) {
        return Entry::Other;
    }
    let is_us = |c: &str| c.rsplit('/').next() == Some("supragnosis");
    fn strings(a: &[Value]) -> Vec<&str> {
        a.iter().filter_map(Value::as_str).collect()
    }
    let command = match entry.get("command") {
        Some(Value::String(cmd)) => Some((
            cmd.as_str(),
            entry
                .get("args")
                .and_then(Value::as_array)
                .map(|a| strings(a))
                .unwrap_or_default(),
        )),
        // OpenCode's shape: the program and its arguments in one array.
        Some(Value::Array(a)) => {
            a.first().and_then(Value::as_str).map(|cmd| (cmd, strings(&a[1..])))
        }
        _ => None,
    };
    if let Some((cmd, args)) = command {
        return match (is_us(cmd), args.first()) {
            (true, Some(&"bridge")) => Entry::Bridge,
            (true, None) | (true, Some(&"serve")) => Entry::Stdio,
            _ => Entry::Other,
        };
    }
    let url = entry.get("url").or_else(|| entry.get("httpUrl")).and_then(Value::as_str);
    match url {
        Some(_) => Entry::Http,
        None => Entry::Other,
    }
}

/// The server entry `connect` writes: the stable program path and `bridge`, nothing else - no
/// token, no environment (client-connect.md C2).
pub fn bridge_entry(program: &str) -> Value {
    serde_json::json!({ "command": program, "args": ["bridge"] })
}

/// A client's config file as text. A missing file is the empty one `connect` may create; any other
/// failure to read it - text that is not UTF-8, a file this user may not read - is an error, since
/// a file read as empty would be rewritten with one entry and lose everything else in it (C3).
pub fn read_config(path: &Path) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

/// Reads one named entry from a JSON config file. A missing file or section is `None`; a file that
/// cannot be read or does not parse is `Unknown`, never `None` (P5).
pub fn read_json_entry(path: &Path, section: &str, name: &str) -> Entry {
    let Ok(text) = read_config(path) else {
        return Entry::Unknown;
    };
    if text.trim().is_empty() {
        return Entry::None;
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => entry_at(&v, &[section], name),
        Err(_) => Entry::Unknown,
    }
}

/// Copies a client's config aside before it is edited (C3): `~/.supragnosis/connect/<client>.<ts>.json`.
pub fn back_up(
    path: &Path,
    client: &str,
    home: &Path,
    now: u64,
) -> std::io::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let dir = home.join(".supragnosis/connect");
    std::fs::create_dir_all(&dir)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("json");
    // Never over an earlier backup: two edits in one second (connect, then --remove) would otherwise
    // replace the copy of the original with a copy of the first edit.
    let to = (0..)
        .map(|n| match n {
            0 => dir.join(format!("{client}.{now}.{ext}")),
            n => dir.join(format!("{client}.{now}-{n}.{ext}")),
        })
        .find(|p| !p.exists())
        .expect("an unused name");
    std::fs::copy(path, &to)?;
    Ok(Some(to))
}

/// Writes a client's config, copying the file it replaces aside first (C3): the copy exists before
/// the file changes, and a copy that cannot be taken stops the write with the file untouched. The
/// copy's path, or `None` when there was no file to copy.
pub fn write_backed_up(
    path: &Path,
    client: &str,
    home: &Path,
    now: u64,
    text: &str,
) -> std::io::Result<Option<PathBuf>> {
    let copy = back_up(path, client, home, now)?;
    write_replacing(path, text)?;
    Ok(copy)
}

/// What `connect <app>` does with the entry it finds there (Section 4, C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// No entry: write the bridge.
    Add,
    /// The bridge is already there; only the capture hook may change.
    AlreadyConnected,
    /// Take out an entry this did not write, as `--replace` asked, then write the bridge.
    Replace(Entry),
}

/// The decision `connect <app>` makes before it writes anything. An entry it did not write is left
/// alone without `--replace` - refused, naming it - and a config that cannot be read is never
/// touched, `--replace` or not. A refusal is the entry that stopped it.
pub fn plan_connect(before: Entry, replace: bool) -> Result<Plan, Entry> {
    match before {
        Entry::None => Ok(Plan::Add),
        Entry::Bridge => Ok(Plan::AlreadyConnected),
        Entry::Unknown => Err(Entry::Unknown),
        other if replace => Ok(Plan::Replace(other)),
        other => Err(other),
    }
}

/// Writes `text` over `path` through a sibling temp file and a rename, so the client never reads a
/// half-written config.
pub fn write_replacing(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.supragnosis-new",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("config")
    ));
    std::fs::write(&tmp, text)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path)
}

// ---- the capture hook's registration (docs/client-connect.md Section 12) ----

/// Claude Code's user settings, where its hooks live.
pub fn claude_code_settings(env: &Env) -> PathBuf {
    env.home.join(".claude/settings.json")
}

/// The events the capture hook answers, in the order `connect` registers them: after a Bash call,
/// at a session's start, and at a subagent's start (docs/recall.md Section 4).
pub const HOOK_EVENTS: [&str; 3] = ["PostToolUse", "SessionStart", "SubagentStart"];

/// The hook's shell command: the program quoted, so a path with a space survives the shell.
pub fn hook_command(program: &str) -> String {
    format!("\"{}\" hook claude-code", program.replace('"', "\\\""))
}

/// The hook group `connect` adds under `hooks.<event>`: one command, a short timeout. After a
/// Bash call only; a session start has no matcher, so every source (startup, resume, clear,
/// compact) gets the index, and neither has a subagent start, so every agent type does.
pub fn hook_group(event: &str, program: &str) -> Value {
    let hooks = serde_json::json!([
        { "type": "command", "command": hook_command(program), "timeout": 10 }
    ]);
    match event {
        "PostToolUse" => serde_json::json!({ "matcher": "Bash", "hooks": hooks }),
        _ => serde_json::json!({ "hooks": hooks }),
    }
}

/// Whether a hook group is the capture hook's: one of its commands runs `supragnosis ... hook
/// claude-code`, whichever program path it names.
pub fn is_capture(group: &Value) -> bool {
    group.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
        hs.iter().any(|h| {
            h.get("command").and_then(Value::as_str).is_some_and(|c| {
                c.contains("supragnosis") && c.trim_end().ends_with("hook claude-code")
            })
        })
    })
}

/// What Claude Code's settings say about capture: on when every event of [`HOOK_EVENTS`] carries
/// the hook. An install connected before the subagent event reads as off until `connect` runs again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    On,
    Off,
    /// The settings file could not be read or parsed, so nothing is known.
    Unknown,
}

impl Capture {
    pub fn as_str(self) -> &'static str {
        match self {
            Capture::On => "on",
            Capture::Off => "off",
            Capture::Unknown => "unknown",
        }
    }
}

pub fn capture_state(text: &str) -> Capture {
    if text.trim().is_empty() {
        return Capture::Off;
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Capture::Unknown;
    };
    let has = |event: &str| {
        v.pointer(&format!("/hooks/{event}"))
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(is_capture))
    };
    if HOOK_EVENTS.iter().all(|e| has(e)) {
        Capture::On
    } else {
        Capture::Off
    }
}

/// Appends the capture group to `hooks.<event>`, creating `hooks` and the array when absent, and
/// leaves every other byte as it was. Returns the text unchanged when the group is already there.
pub fn add_hook(text: &str, event: &str, group: &Value) -> Result<String, String> {
    if text.trim().is_empty() {
        let doc = serde_json::json!({ "hooks": { event: [group] } });
        return Ok(serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n");
    }
    let original: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let b = text.as_bytes();
    let root = object_at(text, skip_ws(b, 0))?;
    let unit = indent_unit(text);
    let (u1, u2, u3) = (unit.clone(), unit.repeat(2), unit.repeat(3));
    let key = serde_json::to_string(event).unwrap_or_default();
    let item = pretty_at(group, &unit, &u3);
    let out = match root.members.iter().find(|m| m.key == "hooks") {
        None => {
            let member = format!("\n{u1}\"hooks\": {{\n{u2}{key}: [\n{u3}{item}\n{u2}]\n{u1}}}");
            let sep = if root.members.is_empty() { "\n" } else { "," };
            format!("{}{}{}{}", &text[..root.open + 1], member, sep, &text[root.open + 1..])
        }
        Some(sec) => {
            let obj = object_at(text, sec.value.0)
                .map_err(|_| "\"hooks\" is not an object".to_string())?;
            match obj.members.iter().find(|m| m.key == event) {
                Some(m) => {
                    let arr = array_at(text, m.value.0)
                        .map_err(|_| format!("\"hooks.{event}\" is not an array"))?;
                    let present = arr.items.iter().any(|(s, e)| {
                        serde_json::from_str::<Value>(&text[*s..*e]).is_ok_and(|v| is_capture(&v))
                    });
                    if present {
                        return Ok(text.to_string());
                    }
                    match arr.items.last() {
                        Some((_, end)) => {
                            format!("{},\n{u3}{item}{}", &text[..*end], &text[*end..])
                        }
                        None => format!(
                            "{}\n{u3}{item}\n{u2}{}",
                            &text[..arr.open + 1],
                            &text[arr.close..]
                        ),
                    }
                }
                None => {
                    let member = format!("{key}: [\n{u3}{item}\n{u2}]");
                    match obj.members.last() {
                        Some(last) => format!(
                            "{},\n{u2}{member}{}",
                            &text[..last.value.1],
                            &text[last.value.1..]
                        ),
                        None => format!(
                            "{}\n{u2}{member}\n{u1}{}",
                            &text[..obj.open + 1],
                            &text[obj.close..]
                        ),
                    }
                }
            }
        }
    };
    verify_hooks(&original, &out, event, true)?;
    Ok(out)
}

/// Removes the capture group from `hooks.<event>`, and the array and `hooks` when that leaves them
/// empty, so a file `connect` wrote into comes back as it was. `Ok(None)` when it is not there.
pub fn remove_hook(text: &str, event: &str) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let original: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let b = text.as_bytes();
    let root = object_at(text, skip_ws(b, 0))?;
    let Some(hooks_pos) = root.members.iter().position(|m| m.key == "hooks") else {
        return Ok(None);
    };
    let hooks = object_at(text, root.members[hooks_pos].value.0)
        .map_err(|_| "\"hooks\" is not an object".to_string())?;
    let Some(event_pos) = hooks.members.iter().position(|m| m.key == event) else {
        return Ok(None);
    };
    let arr = array_at(text, hooks.members[event_pos].value.0)
        .map_err(|_| format!("\"hooks.{event}\" is not an array"))?;
    let Some(pos) = arr.items.iter().position(|(s, e)| {
        serde_json::from_str::<Value>(&text[*s..*e]).is_ok_and(|v| is_capture(&v))
    }) else {
        return Ok(None);
    };
    let out = if arr.items.len() > 1 {
        let (cut0, cut1) = if pos > 0 {
            (arr.items[pos - 1].1, arr.items[pos].1)
        } else {
            (arr.items[0].0, arr.items[1].0)
        };
        format!("{}{}", &text[..cut0], &text[cut1..])
    } else if hooks.members.len() > 1 {
        let (cut0, cut1) = cut_member(&hooks, event_pos);
        format!("{}{}", &text[..cut0], &text[cut1..])
    } else {
        let (cut0, cut1) = cut_member(&root, hooks_pos);
        format!("{}{}", &text[..cut0], &text[cut1..])
    };
    verify_hooks(&original, &out, event, false)?;
    // A second copy, if an earlier run left one, goes the same way.
    match remove_hook(&out, event)? {
        Some(again) => Ok(Some(again)),
        None => Ok(Some(out)),
    }
}

/// The edit did exactly what it says: every top-level member but `hooks` is as it was, every event
/// but `event` is as it was, the other groups under `event` are as they were and in their order, and
/// the capture group is there once (`want`) or not at all.
fn verify_hooks(original: &Value, out: &str, event: &str, want: bool) -> Result<(), String> {
    let edited: Value = serde_json::from_str(out)
        .map_err(|e| format!("edit produced invalid JSON ({e}) - not written"))?;
    let (Some(o), Some(e)) = (original.as_object(), edited.as_object()) else {
        return Err("the document is not a JSON object".into());
    };
    for (k, v) in o {
        if k != "hooks" && e.get(k) != Some(v) {
            return Err(format!("edit would change \"{k}\" - not written"));
        }
    }
    let empty = serde_json::Map::new();
    let oh = o.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
    let eh = e.get("hooks").and_then(Value::as_object).unwrap_or(&empty);
    for (k, v) in oh {
        if k != event && eh.get(k) != Some(v) {
            return Err(format!("edit would change \"hooks.{k}\" - not written"));
        }
    }
    let others = |h: &serde_json::Map<String, Value>| -> Vec<Value> {
        h.get(event)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter(|g| !is_capture(g)).cloned().collect())
            .unwrap_or_default()
    };
    if others(oh) != others(eh) {
        return Err(format!("edit would change another \"hooks.{event}\" group - not written"));
    }
    let ours = eh
        .get(event)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|g| is_capture(g)).count())
        .unwrap_or(0);
    if ours != usize::from(want) {
        return Err("edit did not produce the intended hook entry - not written".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn bridge() -> Value {
        bridge_entry("/opt/homebrew/opt/supragnosis-server/bin/supragnosis")
    }

    #[test]
    fn entries_are_classified_by_what_they_run() {
        assert_eq!(classify(&bridge()), Entry::Bridge);
        assert_eq!(classify(&serde_json::json!({"command": "supragnosis"})), Entry::Stdio);
        assert_eq!(
            classify(&serde_json::json!({"command": "/x/supragnosis", "args": ["serve"]})),
            Entry::Stdio
        );
        assert_eq!(
            classify(
                &serde_json::json!({"type": "http", "url": "http://127.0.0.1:7373/mcp", "headers": {}})
            ),
            Entry::Http
        );
        assert_eq!(
            classify(&serde_json::json!({"command": "npx", "args": ["mcp-remote"]})),
            Entry::Other
        );
    }

    /// Two edits in one second keep two backups: the copy of the original is never overwritten by a
    /// copy of the first edit.
    #[test]
    fn a_backup_never_replaces_an_earlier_one() {
        let home = std::env::temp_dir().join(format!("supragnosis-connect-{}", std::process::id()));
        let file = home.join("app.json");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(&file, "original").unwrap();
        let first = back_up(&file, "app", &home, 7).unwrap().expect("first");
        std::fs::write(&file, "edited").unwrap();
        let second = back_up(&file, "app", &home, 7).unwrap().expect("second");
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "original");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// C3 and Section 4: an entry `connect` did not write is left alone without `--replace`, and the
    /// copy is taken before the file is written. The plan is a table over every entry; the write
    /// leaves the copy holding the old bytes and the file the new, makes no copy of a file that was
    /// not there, and when the copy cannot be taken it stops with the file untouched. The CLI writes
    /// a client's config only through it.
    #[test]
    fn an_entry_connect_did_not_write_is_left_alone() {
        for replace in [false, true] {
            assert_eq!(plan_connect(Entry::None, replace), Ok(Plan::Add));
            assert_eq!(plan_connect(Entry::Bridge, replace), Ok(Plan::AlreadyConnected));
            assert_eq!(plan_connect(Entry::Unknown, replace), Err(Entry::Unknown), "never touched");
            for theirs in [Entry::Http, Entry::Stdio, Entry::Other] {
                let want = if replace { Ok(Plan::Replace(theirs)) } else { Err(theirs) };
                assert_eq!(plan_connect(theirs, replace), want);
            }
        }

        let home = std::env::temp_dir().join(format!("supragnosis-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let file = home.join("app.json");
        std::fs::write(&file, "old").unwrap();
        let copy = write_backed_up(&file, "app", &home, 7, "new").unwrap().expect("a copy");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        let fresh = home.join("fresh.json");
        assert_eq!(write_backed_up(&fresh, "app", &home, 7, "new").unwrap(), None);
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "new");
        // A copy that cannot be taken - the backup directory's place holds a file - stops the write.
        let blocked = home.join("blocked");
        std::fs::create_dir_all(&blocked).unwrap();
        std::fs::write(blocked.join(".supragnosis"), "a file, not a directory").unwrap();
        assert!(write_backed_up(&file, "app", &blocked, 8, "newer").is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new", "the file is untouched");
        let _ = std::fs::remove_dir_all(&home);

        let main = crate::MAIN_SOURCES.concat();
        assert!(!main.contains("connect::back_up("), "no copy is taken apart from its write");
        let writes: Vec<&str> = main
            .match_indices("connect::write_replacing(")
            .map(|(i, _)| main[i..].lines().next().unwrap())
            .collect();
        assert!(
            writes.is_empty(),
            "the CLI writes no file plainly, outside connect's plan: {writes:?}"
        );
        let cmd = &main[main.find("fn connect_cmd(").expect("connect_cmd")..];
        let cmd = &cmd[..cmd.find("\n}\n").expect("it closes")];
        assert!(
            cmd.contains("connect::plan_connect(before, a.replace)"),
            "connect decides by the plan"
        );
    }

    /// A file that exists and cannot be read is not an absent one: read as empty, it would be
    /// rewritten with only the bridge in it. Not UTF-8 here; a file this user may not read is the
    /// same case.
    #[test]
    fn a_file_that_cannot_be_read_is_not_taken_for_an_empty_one() {
        let dir = std::env::temp_dir()
            .join(format!("supragnosis-connect-unreadable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let unreadable = dir.join("mcp.json");
        std::fs::write(&unreadable, b"{\"mcpServers\": {\"other\": \"\xff\"}}").unwrap();
        assert!(read_config(&unreadable).is_err());
        assert_eq!(read_json_entry(&unreadable, "mcpServers", NAME), Entry::Unknown);
        let missing = dir.join("absent.json");
        assert_eq!(read_config(&missing).unwrap(), "", "a missing file is the empty one");
        assert_eq!(read_json_entry(&missing, "mcpServers", NAME), Entry::None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Section 12.4 / C3: the hook's groups go in beside whatever hooks the person has, which
    /// stay byte for byte; `--remove` takes them out and the file is what it was; a file that is
    /// not JSON is refused; registering twice adds nothing.
    #[test]
    fn connect_registers_the_hook_beside_the_bridge_and_removes_both() {
        let p = "/opt/homebrew/opt/supragnosis-server/bin/supragnosis";
        let before = "{\n  \"model\": \"opus\",\n  \"hooks\": {\n    \"PostToolUse\": [\n      {\n        \"matcher\": \"Write\",\n        \"hooks\": [{\"type\": \"command\", \"command\": \"prettier\"}]\n      }\n    ],\n    \"Stop\": [\n      {\"hooks\": [{\"type\": \"command\", \"command\": \"say done\"}]}\n    ]\n  }\n}\n";
        assert_eq!(capture_state(before), Capture::Off);

        let mut text = before.to_string();
        for event in HOOK_EVENTS {
            text = add_hook(&text, event, &hook_group(event, p)).expect(event);
        }
        assert_eq!(capture_state(&text), Capture::On);
        // The person's groups are where they were, untouched, and ours follow them.
        assert!(text.starts_with("{\n  \"model\": \"opus\",\n  \"hooks\": {\n    \"PostToolUse\": [\n      {\n        \"matcher\": \"Write\",\n        \"hooks\": [{\"type\": \"command\", \"command\": \"prettier\"}]\n      },\n      {\n        \"hooks\": [\n"), "{text}");
        assert!(text.contains("    \"Stop\": [\n      {\"hooks\": [{\"type\": \"command\", \"command\": \"say done\"}]}\n    ],\n    \"SessionStart\": [\n"), "{text}");
        let v: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            v["hooks"]["PostToolUse"][1]["hooks"][0]["command"],
            format!("\"{p}\" hook claude-code")
        );
        assert_eq!(v["hooks"]["PostToolUse"][1]["matcher"], "Bash");
        assert!(v["hooks"]["SessionStart"][0].get("matcher").is_none(), "every session source");
        assert!(v["hooks"]["SubagentStart"][0].get("matcher").is_none(), "every agent type");

        // An install connected before the subagent event reads as off, and connecting again adds
        // only the missing group (docs/recall.md Section 4).
        let older = remove_hook(&text, "SubagentStart").expect("remove").expect("removed");
        assert_eq!(capture_state(&older), Capture::Off);
        let mut upgraded = older.clone();
        for event in HOOK_EVENTS {
            upgraded = add_hook(&upgraded, event, &hook_group(event, p)).expect(event);
        }
        assert_eq!(upgraded, text, "the two groups it had are untouched");
        assert!(!text.contains("Bearer") && !text.contains("token"), "C2 holds for the hook too");

        // Twice is once.
        let again = add_hook(&text, "PostToolUse", &hook_group("PostToolUse", p)).expect("again");
        assert_eq!(again, text);

        // Removal restores the original exactly, including the array that only held ours.
        let mut restored = text.clone();
        for event in HOOK_EVENTS {
            if let Some(next) = remove_hook(&restored, event).expect(event) {
                restored = next;
            }
        }
        assert_eq!(restored, before);
        assert_eq!(remove_hook(&restored, "PostToolUse").expect("nothing"), None);

        // From nothing: a file holding only the hooks, and removing them leaves an empty document
        // rather than a stray `"hooks": {}`.
        let mut fresh = String::new();
        for event in HOOK_EVENTS {
            fresh = add_hook(&fresh, event, &hook_group(event, p)).expect(event);
        }
        assert_eq!(capture_state(&fresh), Capture::On);
        let v: Value = serde_json::from_str(&fresh).expect("json");
        assert_eq!(v["hooks"]["PostToolUse"].as_array().map(Vec::len), Some(1));
        let mut emptied = fresh.clone();
        for event in HOOK_EVENTS {
            if let Some(next) = remove_hook(&emptied, event).expect(event) {
                emptied = next;
            }
        }
        let v: Value = serde_json::from_str(&emptied).expect("json");
        assert_eq!(v, serde_json::json!({}));

        // A settings file with other keys and no hooks at all gains only `hooks`.
        let plain = "{\n  \"theme\": \"dark\"\n}\n";
        let with = add_hook(plain, "SessionStart", &hook_group("SessionStart", p)).expect("add");
        let v: Value = serde_json::from_str(&with).expect("json");
        assert_eq!(v["theme"], "dark");
        assert_eq!(remove_hook(&with, "SessionStart").expect("remove").as_deref(), Some(plain));

        // Not JSON, or hooks that are not the documented shape: refused, never rewritten (C3).
        let jsonc = "{\n  // mine\n  \"hooks\": {}\n}";
        assert!(add_hook(jsonc, "PostToolUse", &hook_group("PostToolUse", p)).is_err());
        assert!(remove_hook(jsonc, "PostToolUse").is_err());
        assert_eq!(capture_state(jsonc), Capture::Unknown);
        assert!(add_hook("{\"hooks\": []}", "PostToolUse", &hook_group("PostToolUse", p)).is_err());
        assert!(add_hook(
            "{\"hooks\": {\"PostToolUse\": {}}}",
            "PostToolUse",
            &hook_group("PostToolUse", p)
        )
        .is_err());

        // A program path with a space is quoted for the shell.
        assert_eq!(
            hook_command("/Users/a b/supragnosis"),
            "\"/Users/a b/supragnosis\" hook claude-code"
        );
    }

    /// Section 13.3: OpenCode's shape is one `command` array; a remote entry is the HTTP one; an
    /// entry turned off does not load, in OpenCode or in any app with the same key.
    #[test]
    fn classify_reads_a_command_array_a_remote_url_and_a_disabled_entry() {
        let j = |v: Value| classify(&v);
        let p = "/opt/homebrew/opt/supragnosis-server/bin/supragnosis";
        assert_eq!(
            j(serde_json::json!({"type": "local", "command": [p, "bridge"]})),
            Entry::Bridge
        );
        assert_eq!(j(serde_json::json!({"type": "local", "command": [p]})), Entry::Stdio);
        assert_eq!(j(serde_json::json!({"type": "local", "command": [p, "serve"]})), Entry::Stdio);
        assert_eq!(j(serde_json::json!({"type": "local", "command": ["npx", "x"]})), Entry::Other);
        assert_eq!(j(serde_json::json!({"type": "local", "command": []})), Entry::Other);
        assert_eq!(
            j(serde_json::json!({"type": "remote", "url": "http://127.0.0.1:7373/mcp"})),
            Entry::Http
        );
        assert_eq!(
            j(serde_json::json!({"type": "local", "command": [p, "bridge"], "enabled": false})),
            Entry::Other,
            "off, so it does not load"
        );
        assert_eq!(
            j(serde_json::json!({"type": "local", "command": [p, "bridge"], "enabled": true})),
            Entry::Bridge
        );
        assert_eq!(
            j(serde_json::json!({"command": p, "args": ["bridge"], "enabled": false})),
            Entry::Other
        );
    }
}
