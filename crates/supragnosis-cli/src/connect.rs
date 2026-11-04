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

mod capture;
mod clients;
mod json_edit;

pub use capture::*;
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
