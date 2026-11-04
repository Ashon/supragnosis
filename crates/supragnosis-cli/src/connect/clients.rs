//! The AI apps connect knows: where each keeps its servers, and the CLI calls that register it.

use super::*;

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
    OpenCode,
}

pub const CLIENTS: &[Client] = &[
    Client::ClaudeDesktop,
    Client::ClaudeCode,
    Client::Cursor,
    Client::VsCode,
    Client::Codex,
    Client::Gemini,
    Client::OpenCode,
];

/// The entry's name in every client - one name, so `connect` can always find what it wrote.
pub const NAME: &str = "supragnosis";

/// Where `connect` looks: the home directory, the search path for client CLIs, and the XDG config
/// directory (OpenCode's). The desktop app launches the CLI with a GUI's minimal PATH, so the usual
/// install locations are searched too, and it passes the login shell's `XDG_CONFIG_HOME`.
pub struct Env {
    pub home: PathBuf,
    pub path: Vec<PathBuf>,
    /// `$XDG_CONFIG_HOME`, or `~/.config` where it is unset (an empty or relative value is unset,
    /// as the XDG spec says).
    pub config_home: PathBuf,
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
        for extra in [".local/bin", ".claude/local", ".npm-global/bin", "bin", ".opencode/bin"] {
            path.push(home.join(extra));
        }
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Env { home, path, config_home }
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
            Client::OpenCode => "opencode",
        }
    }

    /// The name a person reads: the core's one table (design-system.md Section 29.8).
    pub fn name(self) -> &'static str {
        supragnosis_core::app_name(self.id()).expect("every client is in the core's table")
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
            // The file OpenCode's own CLI writes (Section 13.1): `opencode.json` where one exists,
            // `opencode.jsonc` where only that does, a new `opencode.json` otherwise.
            Client::OpenCode => {
                let [json, jsonc] = opencode_files(env);
                (if !json.exists() && jsonc.exists() { jsonc } else { json }, "mcp.servers")
            }
        }
    }

    /// The client's CLI, for the clients registered through one.
    pub fn cli(self, env: &Env) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode => env.find("claude"),
            Client::Codex => env.find("codex"),
            Client::Gemini => env.find("gemini"),
            Client::OpenCode => env.find("opencode"),
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

    /// The files the entry is read from: OpenCode's two, every other client's one.
    pub fn files(self, env: &Env) -> Vec<PathBuf> {
        match self {
            Client::OpenCode => opencode_files(env).to_vec(),
            _ => vec![self.config(env).0],
        }
    }

    /// The entry the client holds now, read from its configuration and nothing else (C5).
    pub fn entry(self, env: &Env) -> Entry {
        if self == Client::OpenCode {
            // OpenCode reads both files; where both name supragnosis, the one that asks more of
            // the person (Section 13.3).
            return opencode_files(env)
                .iter()
                .map(|f| read_jsonc_entry(f, &OPENCODE_SECTION, NAME))
                .max_by_key(|e| e.attention())
                .unwrap_or(Entry::None);
        }
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
            Client::OpenCode => {
                "start a new OpenCode session; if it does not list supragnosis, restart OpenCode's background service (`opencode service restart`)"
            }
        }
    }
}

/// Where OpenCode keeps its servers, below the document's root.
pub const OPENCODE_SECTION: [&str; 2] = ["mcp", "servers"];

/// OpenCode's two user configuration files, in the order it reads them (Section 13.1).
pub fn opencode_files(env: &Env) -> [PathBuf; 2] {
    let dir = env.config_home.join("opencode");
    [dir.join("opencode.json"), dir.join("opencode.jsonc")]
}

/// The client CLI invocation that registers the bridge (`None` for the file-edit clients).
pub fn add_argv(client: Client, program: &str) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match client {
        Client::ClaudeCode => v(&["mcp", "add", "--scope", "user", NAME, "--", program, "bridge"]),
        Client::Codex => v(&["mcp", "add", NAME, "--", program, "bridge"]),
        Client::Gemini => v(&["mcp", "add", "--scope", "user", NAME, program, "bridge"]),
        Client::OpenCode => v(&["mcp", "add", "--global", NAME, "--", program, "bridge"]),
        Client::VsCode => {
            let def = serde_json::json!({"name": NAME, "command": program, "args": ["bridge"]});
            v(&["--add-mcp", &def.to_string()])
        }
        Client::ClaudeDesktop | Client::Cursor => None,
    }
}

/// The client CLI invocation that removes the entry (`None` where removal is a file edit - the
/// file-edit clients, and VS Code and OpenCode, whose CLIs add but do not remove).
pub fn remove_argv(client: Client) -> Option<Vec<String>> {
    let v = |a: &[&str]| Some(a.iter().map(|s| s.to_string()).collect());
    match client {
        Client::ClaudeCode => v(&["mcp", "remove", "--scope", "user", NAME]),
        Client::Codex => v(&["mcp", "remove", NAME]),
        Client::Gemini => v(&["mcp", "remove", "--scope", "user", NAME]),
        Client::VsCode | Client::OpenCode | Client::ClaudeDesktop | Client::Cursor => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::tests::bridge;

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

    /// Section 13: OpenCode's entry goes in through its CLI, into the file that CLI writes, and comes
    /// out by a JSONC edit that cuts one span and keeps every comment outside it.
    #[test]
    fn opencode_is_registered_through_its_cli_and_removed_by_a_jsonc_edit() {
        let p = "/opt/homebrew/opt/supragnosis-server/bin/supragnosis";
        assert_eq!(
            add_argv(Client::OpenCode, p).expect("a CLI"),
            ["mcp", "add", "--global", "supragnosis", "--", p, "bridge"]
        );
        assert_eq!(remove_argv(Client::OpenCode), None, "OpenCode's CLI does not remove");

        // The files: under XDG_CONFIG_HOME, `.json` where it exists, `.jsonc` where only that does.
        let root = std::env::temp_dir()
            .join(format!("supragnosis-connect-opencode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let env = Env { home: root.join("home"), path: Vec::new(), config_home: root.join("xdg") };
        let [json, jsonc] = opencode_files(&env);
        assert_eq!(json, root.join("xdg/opencode/opencode.json"));
        assert_eq!(Client::OpenCode.config(&env), (json.clone(), "mcp.servers"));
        std::fs::create_dir_all(json.parent().unwrap()).unwrap();
        std::fs::write(&jsonc, "{\n  // mine\n  \"theme\": \"x\",\n}\n").unwrap();
        assert_eq!(Client::OpenCode.config(&env).0, jsonc, "only the .jsonc exists");
        assert_eq!(Client::OpenCode.entry(&env), Entry::None);

        // What OpenCode's CLI wrote into a commented file with a trailing comma, read back.
        let written = "{\n  // mine\n  \"theme\": \"x\",\n  \"mcp\": {\n    \"servers\": {\n      \"supragnosis\": {\n        \"type\": \"local\",\n        \"command\": [\n          \"/opt/homebrew/opt/supragnosis-server/bin/supragnosis\",\n          \"bridge\"\n        ]\n      }\n    }\n  },\n}\n";
        std::fs::write(&jsonc, written).unwrap();
        assert_eq!(Client::OpenCode.entry(&env), Entry::Bridge);
        // Both files: the one that asks more of the person is what the listing says.
        std::fs::write(&json, r#"{"mcp": {"servers": {"supragnosis": {"type": "remote", "url": "http://127.0.0.1:7373/mcp"}}}}"#).unwrap();
        assert_eq!(Client::OpenCode.config(&env).0, json, "the .json, once it exists");
        assert_eq!(Client::OpenCode.entry(&env), Entry::Http);
        std::fs::write(&json, "{\"mcp\": ").unwrap();
        assert_eq!(Client::OpenCode.entry(&env), Entry::Unknown, "a file that does not parse");
        assert_eq!(Client::OpenCode.files(&env), vec![json.clone(), jsonc.clone()]);
        let _ = std::fs::remove_dir_all(&root);

        let cut = |text: &str| {
            remove_jsonc(text, &OPENCODE_SECTION, NAME).expect("removed").expect("present")
        };
        // The lone member: its line goes, the comment inside the object and the trailing comma
        // after `mcp` stay.
        assert_eq!(
            cut(written),
            "{\n  // mine\n  \"theme\": \"x\",\n  \"mcp\": {\n    \"servers\": {\n    }\n  },\n}\n"
        );
        // A member with one after it: it and its comma go, as one line.
        let first = "{\"mcp\": {\"servers\": {\n  \"supragnosis\": {\"type\": \"local\", \"command\": [\"supragnosis\", \"bridge\"]}, // ours\n  /* theirs */ \"other\": {\"command\": [\"x\"]}\n}}}";
        let out = cut(first);
        assert_eq!(out, "{\"mcp\": {\"servers\": {\n   // ours\n  /* theirs */ \"other\": {\"command\": [\"x\"]}\n}}}");
        // The last member: the comma before it goes with it...
        let last = "{\"mcp\": {\"servers\": {\n  \"other\": {\"command\": [\"x\"]},\n  \"supragnosis\": {\"command\": [\"supragnosis\", \"bridge\"]}\n}}}";
        assert_eq!(
            cut(last),
            "{\"mcp\": {\"servers\": {\n  \"other\": {\"command\": [\"x\"]}\n}}}"
        );
        // ...unless a comment sits between, which is most often the previous member's: the comma
        // stays, as a trailing comma.
        let noted = "{\"mcp\": {\"servers\": {\n  \"other\": {\"command\": [\"x\"]}, // the x server\n  \"supragnosis\": {\"command\": [\"supragnosis\", \"bridge\"]}\n}}}";
        assert_eq!(
            cut(noted),
            "{\"mcp\": {\"servers\": {\n  \"other\": {\"command\": [\"x\"]}, // the x server\n}}}"
        );
        // A comment holding brackets, quotes and a comma does not move the cut.
        let tricky = "{\n  /* a } \", [ */\n  \"mcp\": {\"servers\": {\"supragnosis\": {\"command\": [\"a//b\", \"/*\"]}}}\n}";
        assert_eq!(cut(tricky), "{\n  /* a } \", [ */\n  \"mcp\": {\"servers\": {}}\n}");
        // Every byte outside the span is the original's: what was cut is one run of bytes.
        for (before, after) in [(written, cut(written)), (first, cut(first)), (noted, cut(noted))] {
            let lead = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
            let tail = before
                .bytes()
                .rev()
                .zip(after.bytes().rev())
                .take_while(|(a, b)| a == b)
                .count();
            assert!(lead + tail >= after.len(), "one span: {after:?}");
        }

        // Nothing there, a file that does not parse, and a second key of the same name: no edit.
        assert_eq!(remove_jsonc("{\"theme\": 1}", &OPENCODE_SECTION, NAME), Ok(None));
        assert_eq!(remove_jsonc("", &OPENCODE_SECTION, NAME), Ok(None));
        assert!(remove_jsonc("{\"mcp\": {/* open", &OPENCODE_SECTION, NAME).is_err());
        assert!(remove_jsonc("{'mcp': {}}", &OPENCODE_SECTION, NAME).is_err(), "not JSON5");
        let twice = "{\"mcp\": {\"servers\": {\"supragnosis\": {}, \"supragnosis\": {}}}}";
        assert!(remove_jsonc(twice, &OPENCODE_SECTION, NAME).is_err());

        // Strict JSON stays strict for every other app: a comment or a trailing comma is refused.
        assert!(remove("{\"mcpServers\": {\"supragnosis\": {},}}", "mcpServers", NAME).is_err());
        assert!(
            upsert("{\n  // x\n  \"mcpServers\": {}\n}", "mcpServers", NAME, &bridge()).is_err()
        );
    }
}
