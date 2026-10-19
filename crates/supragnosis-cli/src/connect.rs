//! `supragnosis connect` - registers the bridge with an AI app (docs/client-connect.md Section 4).
//!
//! Through the app's own CLI where it has one (Claude Code, Codex, Gemini, VS Code's
//! `--add-mcp`), and by editing its config file only where it has none (Claude Desktop, Cursor).
//! A file edit changes one member and leaves every other byte as written - an app's config is its
//! user's file (P24) - and is checked after the fact: the result must parse, carry the intended
//! entry, and agree with the original everywhere else, or nothing is written.

use std::path::{Path, PathBuf};

use serde_json::Value;

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

/// Classifies one server entry, in any client's shape (`command`/`args`, or `url`/`type`). TOML
/// entries (Codex) are converted to JSON values before they get here.
pub fn classify(entry: &Value) -> Entry {
    let is_us = |c: &str| c.rsplit('/').next() == Some("supragnosis");
    if let Some(cmd) = entry.get("command").and_then(Value::as_str) {
        let args: Vec<&str> = entry
            .get("args")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
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

// --- A minimal JSON editor: change one member of one object, keep every other byte ----------------

#[derive(Debug)]
struct Member {
    key: String,
    /// Start of the key's opening quote.
    start: usize,
    /// The value's byte range.
    value: (usize, usize),
}

#[derive(Debug)]
struct Object {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// `i` at an opening quote; returns the index just past the closing one.
fn string_end(b: &[u8], i: usize) -> Result<usize, String> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Ok(j + 1),
            _ => j += 1,
        }
    }
    Err("unterminated string".into())
}

fn value_end(b: &[u8], i: usize) -> Result<usize, String> {
    match b.get(i) {
        Some(b'"') => string_end(b, i),
        Some(b'{') | Some(b'[') => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_end(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            Err("unterminated object or array".into())
        }
        Some(_) => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']') && !b[j].is_ascii_whitespace()
            {
                j += 1;
            }
            Ok(j)
        }
        None => Err("unexpected end of input".into()),
    }
}

fn object_at(text: &str, i: usize) -> Result<Object, String> {
    let b = text.as_bytes();
    if b.get(i) != Some(&b'{') {
        return Err("not an object".into());
    }
    let mut members = Vec::new();
    let mut j = skip_ws(b, i + 1);
    if b.get(j) == Some(&b'}') {
        return Ok(Object { open: i, close: j, members });
    }
    loop {
        if b.get(j) != Some(&b'"') {
            return Err("expected a member name".into());
        }
        let key_end = string_end(b, j)?;
        let key: String = serde_json::from_str(&text[j..key_end]).map_err(|e| e.to_string())?;
        let colon = skip_ws(b, key_end);
        if b.get(colon) != Some(&b':') {
            return Err("expected ':'".into());
        }
        let v0 = skip_ws(b, colon + 1);
        let v1 = value_end(b, v0)?;
        members.push(Member { key, start: j, value: (v0, v1) });
        j = skip_ws(b, v1);
        match b.get(j) {
            Some(b',') => j = skip_ws(b, j + 1),
            Some(b'}') => return Ok(Object { open: i, close: j, members }),
            _ => return Err("expected ',' or '}'".into()),
        }
    }
}

/// The indentation unit the file already uses (the first indented line), two spaces otherwise.
fn indent_unit(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let n = l.len() - l.trim_start().len();
            (n > 0 && !l.trim().is_empty()).then(|| l[..n].to_string())
        })
        .unwrap_or_else(|| "  ".to_string())
}

/// `value` pretty-printed in the file's own indentation step, with every line after the first
/// indented by `pad`. Only leading whitespace is rewritten - a string value is never touched.
fn pretty_at(value: &Value, unit: &str, pad: &str) -> String {
    let s = serde_json::to_string_pretty(value).unwrap_or_default();
    s.lines()
        .enumerate()
        .map(|(n, l)| {
            let depth = (l.len() - l.trim_start_matches(' ').len()) / 2;
            let line = format!("{}{}", unit.repeat(depth), l.trim_start_matches(' '));
            if n == 0 {
                line
            } else {
                format!("{pad}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sets `section.name = value` in a JSON object document, creating `section` if absent, and leaves
/// every other byte of `text` as it was. An empty `text` becomes a new document.
pub fn upsert(text: &str, section: &str, name: &str, value: &Value) -> Result<String, String> {
    if text.trim().is_empty() {
        let doc = serde_json::json!({ section: { name: value } });
        return Ok(serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n");
    }
    let original: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let b = text.as_bytes();
    let root = object_at(text, skip_ws(b, 0))?;
    let unit = indent_unit(text);
    let out = match root.members.iter().find(|m| m.key == section) {
        None => {
            let member = format!(
                "\n{unit}{}: {{\n{unit}{unit}{}: {}\n{unit}}}",
                serde_json::to_string(section).unwrap_or_default(),
                serde_json::to_string(name).unwrap_or_default(),
                pretty_at(value, &unit, &format!("{unit}{unit}"))
            );
            let sep = if root.members.is_empty() { "\n" } else { "," };
            format!("{}{}{}{}", &text[..root.open + 1], member, sep, &text[root.open + 1..])
        }
        Some(sec) => {
            let obj = object_at(text, sec.value.0)
                .map_err(|_| format!("\"{section}\" is not an object"))?;
            match obj.members.iter().find(|m| m.key == name) {
                Some(m) => format!(
                    "{}{}{}",
                    &text[..m.value.0],
                    pretty_at(value, &unit, &format!("{unit}{unit}")),
                    &text[m.value.1..]
                ),
                None => {
                    let member = format!(
                        "{}: {}",
                        serde_json::to_string(name).unwrap_or_default(),
                        pretty_at(value, &unit, &format!("{unit}{unit}"))
                    );
                    match obj.members.last() {
                        Some(last) => format!(
                            "{},\n{unit}{unit}{}{}",
                            &text[..last.value.1],
                            member,
                            &text[last.value.1..]
                        ),
                        None => format!(
                            "{}\n{unit}{unit}{}\n{unit}{}",
                            &text[..obj.open + 1],
                            member,
                            &text[obj.close..]
                        ),
                    }
                }
            }
        }
    };
    verify(&original, &out, section, name, Some(value))?;
    Ok(out)
}

/// Removes `section.name`, leaving every other byte as it was. `Ok(None)` when it is not there.
pub fn remove(text: &str, section: &str, name: &str) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let original: Value =
        serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e})"))?;
    let b = text.as_bytes();
    let root = object_at(text, skip_ws(b, 0))?;
    let Some(sec) = root.members.iter().find(|m| m.key == section) else {
        return Ok(None);
    };
    let obj =
        object_at(text, sec.value.0).map_err(|_| format!("\"{section}\" is not an object"))?;
    let Some(pos) = obj.members.iter().position(|m| m.key == name) else {
        return Ok(None);
    };
    let m = &obj.members[pos];
    // Take the member with the separator on one side: the preceding comma when there is a member
    // before it, the following one otherwise.
    let (cut0, cut1) = if pos > 0 {
        (obj.members[pos - 1].value.1, m.value.1)
    } else if let Some(next) = obj.members.get(1) {
        (m.start, next.start)
    } else {
        (obj.open + 1, obj.close)
    };
    let out = format!("{}{}", &text[..cut0], &text[cut1..]);
    verify(&original, &out, section, name, None)?;
    Ok(Some(out))
}

/// The edit is accepted only if it did exactly what it says: the entry is as intended, and every
/// other top-level member and every other member of `section` equals the original.
fn verify(
    original: &Value,
    out: &str,
    section: &str,
    name: &str,
    want: Option<&Value>,
) -> Result<(), String> {
    let edited: Value = serde_json::from_str(out)
        .map_err(|e| format!("edit produced invalid JSON ({e}) - not written"))?;
    let (Some(o), Some(e)) = (original.as_object(), edited.as_object()) else {
        return Err("the document is not a JSON object".into());
    };
    for (k, v) in o {
        if k != section && e.get(k) != Some(v) {
            return Err(format!("edit would change \"{k}\" - not written"));
        }
    }
    let empty = serde_json::Map::new();
    let os = o.get(section).and_then(Value::as_object).unwrap_or(&empty);
    let es = e.get(section).and_then(Value::as_object).unwrap_or(&empty);
    for (k, v) in os {
        if k != name && es.get(k) != Some(v) {
            return Err(format!("edit would change \"{section}.{k}\" - not written"));
        }
    }
    if es.get(name) != want {
        return Err("edit did not produce the intended entry - not written".into());
    }
    Ok(())
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
        Ok(v) => v.get(section).and_then(|s| s.get(name)).map(classify).unwrap_or(Entry::None),
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

// --- The clients ---------------------------------------------------------------------------------

/// One AI app `connect` knows (client-connect.md Section 4's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    VsCode,
    Codex,
    Gemini,
}

pub const CLIENTS: &[Client] = &[
    Client::ClaudeDesktop,
    Client::ClaudeCode,
    Client::Cursor,
    Client::VsCode,
    Client::Codex,
    Client::Gemini,
];

/// The entry's name in every client - one name, so `connect` can always find what it wrote.
pub const NAME: &str = "supragnosis";

/// Where `connect` looks: the home directory, and the search path for client CLIs. The desktop app
/// launches the CLI with a GUI's minimal PATH, so the usual install locations are searched too.
pub struct Env {
    pub home: PathBuf,
    pub path: Vec<PathBuf>,
}

impl Env {
    pub fn from_process() -> Env {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
        let mut path: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            path.push(PathBuf::from(extra));
        }
        for extra in [".local/bin", ".claude/local", ".npm-global/bin", "bin"] {
            path.push(home.join(extra));
        }
        Env { home, path }
    }

    pub fn find(&self, program: &str) -> Option<PathBuf> {
        self.path.iter().map(|d| d.join(program)).find(|p| p.is_file())
    }

    /// The PATH a client CLI runs with - the same directories it was found in, so a CLI that is a
    /// node script can find its node.
    pub fn path_var(&self) -> std::ffi::OsString {
        std::env::join_paths(&self.path).unwrap_or_default()
    }
}

/// How a client's configuration is changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Cli,
    File,
}

impl Client {
    pub fn id(self) -> &'static str {
        match self {
            Client::ClaudeDesktop => "claude-desktop",
            Client::ClaudeCode => "claude-code",
            Client::Cursor => "cursor",
            Client::VsCode => "vscode",
            Client::Codex => "codex",
            Client::Gemini => "gemini",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeDesktop => "Claude Desktop",
            Client::ClaudeCode => "Claude Code",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
            Client::Codex => "Codex",
            Client::Gemini => "Gemini CLI",
        }
    }

    pub fn parse(id: &str) -> Option<Client> {
        CLIENTS.iter().copied().find(|c| c.id() == id)
    }

    /// The file the client keeps its servers in, and the key they live under.
    pub fn config(self, env: &Env) -> (PathBuf, &'static str) {
        let h = &env.home;
        match self {
            Client::ClaudeDesktop => (
                h.join("Library/Application Support/Claude/claude_desktop_config.json"),
                "mcpServers",
            ),
            Client::ClaudeCode => (h.join(".claude.json"), "mcpServers"),
            Client::Cursor => (h.join(".cursor/mcp.json"), "mcpServers"),
            Client::VsCode if cfg!(target_os = "macos") => {
                (h.join("Library/Application Support/Code/User/mcp.json"), "servers")
            }
            Client::VsCode => (h.join(".config/Code/User/mcp.json"), "servers"),
            Client::Codex => (h.join(".codex/config.toml"), "mcp_servers"),
            Client::Gemini => (h.join(".gemini/settings.json"), "mcpServers"),
        }
    }

    /// The client's CLI, for the clients registered through one.
    pub fn cli(self, env: &Env) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode => env.find("claude"),
            Client::Codex => env.find("codex"),
            Client::Gemini => env.find("gemini"),
            // The app bundle's CLI only on macOS: a Linux machine can see a Mac's /Applications
            // (OrbStack mounts it), and running that binary from Linux reaches the Mac's VS Code.
            Client::VsCode => {
                let bundled = PathBuf::from(
                    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
                );
                (cfg!(target_os = "macos") && bundled.is_file())
                    .then_some(bundled)
                    .or_else(|| env.find("code"))
            }
            Client::ClaudeDesktop | Client::Cursor => None,
        }
    }

    pub fn installed(self, env: &Env) -> bool {
        // An app bundle counts only on macOS, for the reason `cli` gives: elsewhere a /Applications
        // can be a Mac's, seen through a VM, and its apps are not this machine's.
        let app = |name: &str| {
            cfg!(target_os = "macos")
                && (Path::new("/Applications").join(name).exists()
                    || env.home.join("Applications").join(name).exists())
        };
        match self {
            Client::ClaudeDesktop => app("Claude.app"),
            Client::Cursor => app("Cursor.app") || env.home.join(".cursor").is_dir(),
            _ => self.cli(env).is_some(),
        }
    }

    pub fn via(self) -> Via {
        match self {
            Client::ClaudeDesktop | Client::Cursor => Via::File,
            _ => Via::Cli,
        }
    }

    /// The entry the client holds now, read from its configuration and nothing else (C5).
    pub fn entry(self, env: &Env) -> Entry {
        let (path, section) = self.config(env);
        if self == Client::Codex {
            let Ok(text) = read_config(&path) else {
                return Entry::Unknown;
            };
            return match text.parse::<toml::Table>() {
                Ok(t) => t
                    .get(section)
                    .and_then(|s| s.get(NAME))
                    .and_then(|e| serde_json::to_value(e).ok())
                    .map(|e| classify(&e))
                    .unwrap_or(Entry::None),
                Err(_) => Entry::Unknown,
            };
        }
        read_json_entry(&path, section, NAME)
    }

    /// What the person has to do after `connect` for the client to load the entry.
    pub fn next_step(self) -> &'static str {
        match self {
            Client::ClaudeDesktop => {
                "quit Claude (Cmd+Q) and open it again - it reads its servers at launch"
            }
            Client::ClaudeCode => "start a new Claude Code session",
            Client::Cursor => "Cursor lists it under Settings > MCP; turn it on there if it is off",
            Client::VsCode => {
                "VS Code starts it the first time a chat needs it (it may ask you to trust it)"
            }
            Client::Codex => "start a new Codex session",
            Client::Gemini => "start a new Gemini CLI session",
        }
    }
}

/// The client CLI invocation that registers the bridge (`None` for the file-edit clients).
pub fn add_argv(client: Client, program: &str) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match client {
        Client::ClaudeCode => v(&["mcp", "add", "--scope", "user", NAME, "--", program, "bridge"]),
        Client::Codex => v(&["mcp", "add", NAME, "--", program, "bridge"]),
        Client::Gemini => v(&["mcp", "add", "--scope", "user", NAME, program, "bridge"]),
        Client::VsCode => {
            let def = serde_json::json!({"name": NAME, "command": program, "args": ["bridge"]});
            v(&["--add-mcp", &def.to_string()])
        }
        Client::ClaudeDesktop | Client::Cursor => None,
    }
}

/// The client CLI invocation that removes the entry (`None` where removal is a file edit - the
/// file-edit clients, and VS Code, whose CLI adds but does not remove).
pub fn remove_argv(client: Client) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match client {
        Client::ClaudeCode => v(&["mcp", "remove", "--scope", "user", NAME]),
        Client::Codex => v(&["mcp", "remove", NAME]),
        Client::Gemini => v(&["mcp", "remove", "--scope", "user", NAME]),
        Client::VsCode | Client::ClaudeDesktop | Client::Cursor => None,
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

/// The two events the capture hook answers, in the order `connect` registers them.
pub const HOOK_EVENTS: [&str; 2] = ["PostToolUse", "SessionStart"];

/// The hook's shell command: the program quoted, so a path with a space survives the shell.
pub fn hook_command(program: &str) -> String {
    format!("\"{}\" hook claude-code", program.replace('"', "\\\""))
}

/// The hook group `connect` adds under `hooks.<event>`: one command, a short timeout. After a
/// Bash call only; a session start has no matcher, so every source (startup, resume, clear,
/// compact) gets the line.
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

/// What Claude Code's settings say about capture: on when both events carry the hook.
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

/// A JSON array in the text: its brackets and each item's byte range.
struct Array {
    open: usize,
    close: usize,
    items: Vec<(usize, usize)>,
}

fn array_at(text: &str, i: usize) -> Result<Array, String> {
    let b = text.as_bytes();
    if b.get(i) != Some(&b'[') {
        return Err("not an array".into());
    }
    let mut items = Vec::new();
    let mut j = skip_ws(b, i + 1);
    if b.get(j) == Some(&b']') {
        return Ok(Array { open: i, close: j, items });
    }
    loop {
        let v1 = value_end(b, j)?;
        items.push((j, v1));
        j = skip_ws(b, v1);
        match b.get(j) {
            Some(b',') => j = skip_ws(b, j + 1),
            Some(b']') => return Ok(Array { open: i, close: j, items }),
            _ => return Err("expected ',' or ']'".into()),
        }
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

/// The byte range that takes member `pos` out of `obj` with the separator on one side: the
/// preceding comma when a member precedes it, the following one otherwise.
fn cut_member(obj: &Object, pos: usize) -> (usize, usize) {
    let m = &obj.members[pos];
    if pos > 0 {
        (obj.members[pos - 1].value.1, m.value.1)
    } else if let Some(next) = obj.members.get(1) {
        (m.start, next.start)
    } else {
        (obj.open + 1, obj.close)
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

    fn bridge() -> Value {
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

    /// C3's "everything else as written": a member is added to an existing section and the rest of
    /// the file - order, spacing, other servers, other keys - is byte-for-byte what it was.
    #[test]
    fn an_edit_changes_one_member_and_nothing_else() {
        let before = "{\n    \"preferences\": {\"z\": 1, \"a\": 2},\n    \"mcpServers\": {\n        \"other\": {\"command\": \"x\"}\n    }\n}\n";
        let after = upsert(before, "mcpServers", "supragnosis", &bridge()).expect("upsert");
        assert!(after.starts_with("{\n    \"preferences\": {\"z\": 1, \"a\": 2},\n    \"mcpServers\": {\n        \"other\": {\"command\": \"x\"},\n        \"supragnosis\": {"), "{after}");
        assert_eq!(read_entry_of(&after), Entry::Bridge);

        // And removing it restores the original exactly.
        let restored =
            remove(&after, "mcpServers", "supragnosis").expect("remove").expect("present");
        assert_eq!(restored, before);
    }

    fn read_entry_of(text: &str) -> Entry {
        serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| v["mcpServers"].get("supragnosis").map(classify))
            .unwrap_or(Entry::None)
    }

    #[test]
    fn a_missing_section_or_file_is_created_and_an_existing_entry_replaced() {
        let fresh = upsert("", "mcpServers", "supragnosis", &bridge()).expect("new file");
        assert_eq!(read_entry_of(&fresh), Entry::Bridge);

        let no_section = "{\"globalShortcut\": \"\"}";
        let added =
            upsert(no_section, "mcpServers", "supragnosis", &bridge()).expect("section added");
        let v: Value = serde_json::from_str(&added).expect("json");
        assert_eq!(v["globalShortcut"], "");
        assert_eq!(read_entry_of(&added), Entry::Bridge);

        let empty_section = "{\"mcpServers\": {}}";
        assert_eq!(
            read_entry_of(
                &upsert(empty_section, "mcpServers", "supragnosis", &bridge()).expect("into {}")
            ),
            Entry::Bridge
        );

        let http = r#"{"mcpServers": {"supragnosis": {"type": "http", "url": "http://127.0.0.1:7373/mcp", "headers": {"Authorization": "Bearer t"}}, "b": {"command": "y"}}}"#;
        let replaced = upsert(http, "mcpServers", "supragnosis", &bridge()).expect("replace");
        assert_eq!(read_entry_of(&replaced), Entry::Bridge);
        assert!(!replaced.contains("Bearer"), "the token copy is gone with the entry");
        let removed = remove(&replaced, "mcpServers", "supragnosis")
            .expect("remove")
            .expect("present");
        let v: Value = serde_json::from_str(&removed).expect("json");
        assert_eq!(v["mcpServers"], serde_json::json!({"b": {"command": "y"}}));
    }

    /// The registrations name the bridge and the user scope where a client has scopes, and carry
    /// no token anywhere in the argument list (C2).
    #[test]
    fn registrations_run_the_bridge_and_carry_no_secret() {
        let p = "/opt/homebrew/opt/supragnosis-server/bin/supragnosis";
        let cc = add_argv(Client::ClaudeCode, p).expect("claude code");
        assert_eq!(cc, ["mcp", "add", "--scope", "user", "supragnosis", "--", p, "bridge"]);
        let gm = add_argv(Client::Gemini, p).expect("gemini");
        assert_eq!(&gm[..4], ["mcp", "add", "--scope", "user"]);
        let vs = add_argv(Client::VsCode, p).expect("vscode");
        let def: Value = serde_json::from_str(&vs[1]).expect("json");
        assert_eq!(classify(&def), Entry::Bridge);
        for c in CLIENTS {
            if let Some(argv) = add_argv(*c, p) {
                assert!(
                    !argv.iter().any(|a| a.contains("Bearer") || a.contains("token")),
                    "{argv:?}"
                );
            }
        }
        assert_eq!(add_argv(Client::ClaudeDesktop, p), None, "a file edit, not a CLI");
        assert_eq!(remove_argv(Client::VsCode), None, "VS Code's CLI does not remove");
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

    /// Section 12.4 / C3: the two hook groups go in beside whatever hooks the person has, which
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

    /// A file that is not JSON - a comment someone added, a half-written save - is refused, never
    /// rewritten into something that parses (C3).
    #[test]
    fn a_file_that_does_not_parse_is_refused() {
        let jsonc = "{\n  // my servers\n  \"mcpServers\": {}\n}";
        assert!(upsert(jsonc, "mcpServers", "supragnosis", &bridge()).is_err());
        assert!(remove(jsonc, "mcpServers", "supragnosis").is_err());
        assert!(
            upsert("{\"mcpServers\": []}", "mcpServers", "supragnosis", &bridge()).is_err(),
            "a section that is not an object"
        );
    }
}
