//! The AI apps `supragnosis connect` knows, and the client names a node records.

/// The AI apps `supragnosis connect` knows (client-connect.md Section 4): the id a command takes and
/// the name a person reads - the one table setup, `connect` and the reads that name an app use
/// (design-system.md Section 29.8).
pub const AI_APPS: [(&str, &str); 7] = [
    ("claude-desktop", "Claude Desktop"),
    ("claude-code", "Claude Code"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("codex", "Codex"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
];

/// The name a person reads for an app id of [`AI_APPS`].
pub fn app_name(id: &str) -> Option<&'static str> {
    AI_APPS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

/// The app a client name from MCP `initialize` belongs to (onboarding.md Section 5), as the apps
/// send it. A name this table does not know is shown as sent, never guessed.
pub fn client_app(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    if n.starts_with("claude-code") {
        Some("claude-code")
    } else if n == "claude-ai" || n.starts_with("claude-desktop") || n == "claude" {
        Some("claude-desktop")
    } else if n.contains("cursor") {
        Some("cursor")
    } else if n.contains("visual studio code") || n == "vscode" || n.starts_with("code-") {
        Some("vscode")
    } else if n.starts_with("codex") {
        Some("codex")
    } else if n.starts_with("gemini") {
        Some("gemini")
    } else if n.starts_with("opencode") {
        Some("opencode")
    } else {
        None
    }
}

/// The longest client name a node records for an attestation (design-system.md Section 29.8).
pub const CLIENT_NAME_MAX: usize = 64;

/// A client name as a node records it: trimmed and cut to [`CLIENT_NAME_MAX`] characters, or
/// `None` for an empty one.
pub fn client_recorded(name: &str) -> Option<String> {
    let n: String = name.trim().chars().take(CLIENT_NAME_MAX).collect();
    (!n.is_empty()).then_some(n)
}

/// What a reader is shown for a recorded client name: its app's name, or the name as sent.
pub fn client_said(name: &str) -> String {
    client_app(name)
        .and_then(app_name)
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}
