//! First contact and the usage log: which apps reached the daemon, and a line per call.

/// First contact (docs/onboarding.md Section 5): which AI apps have talked to this daemon since it
/// started, by the name each client gives itself. Kept in memory only - never in the store, never
/// sent to a peer (O7) - so that setup can say an app reached the daemon without
/// writing anything to learn it.
#[derive(Default)]
pub struct ClientLog {
    seen: std::sync::Mutex<std::collections::BTreeMap<String, ClientSeen>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClientSeen {
    pub version: String,
    /// Milliseconds since the epoch, of the first request and of the last.
    pub first: u64,
    pub last: u64,
}

impl ClientLog {
    /// The name a client that gave none is listed under.
    pub const UNNAMED: &'static str = "an unnamed client";

    pub fn note(&self, name: &str, version: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let name = if name.trim().is_empty() { Self::UNNAMED } else { name.trim() };
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        seen.entry(name.to_string())
            .and_modify(|s| {
                s.last = now;
                if !version.is_empty() {
                    s.version = version.to_string();
                }
            })
            .or_insert(ClientSeen { version: version.to_string(), first: now, last: now });
    }

    /// `{"clients": [{"name", "version", "first", "last"}]}`, in name order.
    pub fn snapshot(&self) -> serde_json::Value {
        let seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let list: Vec<serde_json::Value> = seen
            .iter()
            .map(|(name, s)| {
                serde_json::json!({ "name": name, "version": s.version, "first": s.first, "last": s.last })
            })
            .collect();
        serde_json::json!({ "clients": list })
    }
}

/// The usage log (docs/usage.md): one JSON line per call the daemon's MCP surface answers, appended
/// to a file beside the viewer's socket. It names functions, workspaces, apps and counts, never what
/// was asked (UL2), and it never leaves the machine (UL1). A line that cannot be written is said once
/// in the daemon's log and costs the call nothing (UL3).
pub struct UsageLog {
    path: std::path::PathBuf,
    warned: std::sync::atomic::AtomicBool,
}

impl UsageLog {
    pub fn at(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into(), warned: Default::default() }
    }

    pub(crate) fn record(&self, line: serde_json::Value) {
        if let Err(e) = append_usage_line(&self.path, &line) {
            if !self.warned.swap(true, std::sync::atomic::Ordering::Relaxed) {
                tracing::warn!(path = %self.path.display(), "the usage log cannot be written: {e}");
            }
        }
    }
}

/// Appends one usage line to `path`, creating the file 0600 (UL1). One write per line, so the daemon
/// and the Claude Code hook, two processes appending to the same file, never interleave within one.
pub fn append_usage_line(path: &std::path::Path, line: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut text = line.to_string();
    text.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?
        .write_all(text.as_bytes())
}

/// What a call's answer says for the usage log: its outcome, how many it answered, and a search's
/// mode, read from the answer's JSON without keeping any of its text (UL2). `found: false` is "not
/// found" only on the reads by id; on `propose` it means a new proposal, which is a success.
pub(crate) fn answered(
    tool: &str,
    text: Option<&str>,
    failed: bool,
) -> (&'static str, Option<usize>, Option<String>) {
    if failed {
        return ("error", None, None);
    }
    let Some(v) = text.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()) else {
        return ("ok", None, None);
    };
    let mode = v.get("mode").and_then(serde_json::Value::as_str).map(str::to_string);
    let by_id = matches!(tool, "get_entity" | "get_proposal");
    if by_id && v.get("found") == Some(&serde_json::Value::Bool(false)) {
        return ("not_found", None, mode);
    }
    let list = |k: &str| v.get(k).and_then(serde_json::Value::as_array).map(Vec::len);
    let n = v.as_array().map(Vec::len).or_else(|| list("hits")).or_else(|| list("clusters"));
    let written = list("entities");
    let empty = n == Some(0);
    (if empty { "empty" } else { "ok" }, n.or(written), mode)
}

/// A commit id as the usage log keeps it: 7 to 64 hex digits from an `observe`'s `git:` source,
/// and nothing else an agent wrote there (UL2).
pub(crate) fn commit_of(source: Option<&str>) -> Option<&str> {
    let id = source?.strip_prefix("git:")?;
    ((7..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

#[cfg(test)]
mod usage_tests {
    use super::{answered, commit_of};
    use serde_json::json;

    /// docs/usage.md Section 2: what an answer comes to. `found: false` is "not found" only on the
    /// reads by id - on `propose` it is a new proposal; a map with no clusters and a search with no
    /// hits are empty; a write keeps the count of what it wrote.
    #[test]
    fn an_answer_is_read_for_its_outcome_and_count() {
        let a = |tool: &str, v: serde_json::Value| answered(tool, Some(&v.to_string()), false);
        assert_eq!(a("get_entity", json!({"found": false})).0, "not_found");
        assert_eq!(a("get_proposal", json!({"found": false})).0, "not_found");
        assert_eq!(a("propose", json!({"found": false, "proposal": "x"})).0, "ok");
        assert_eq!(a("workspace_map", json!({"clusters": []})), ("empty", Some(0), None));
        assert_eq!(a("workspace_map", json!({"clusters": [1, 2]})).1, Some(2));
        let hits = json!({"hits": [], "mode": "keyword"});
        assert_eq!(a("search_knowledge", hits), ("empty", Some(0), Some("keyword".into())));
        assert_eq!(a("traverse", json!([1, 2, 3])), ("ok", Some(3), None));
        assert_eq!(a("observe", json!({"entities": []})), ("ok", Some(0), None));
        assert_eq!(answered("observe", Some("not json"), false).0, "ok");
        assert_eq!(answered("observe", None, true).0, "error");
    }

    /// UL2: only a commit id leaves an `observe`'s source for the log.
    #[test]
    fn only_a_commit_id_is_kept_from_a_source() {
        assert_eq!(commit_of(Some("git:abc1234")), Some("abc1234"));
        let long = format!("git:{}", "f".repeat(40));
        assert_eq!(commit_of(Some(&long)).map(str::len), Some(40));
        for not in ["git:", "git:abc", "git:not a commit", "https://x", "git:abcdefg"] {
            assert_eq!(commit_of(Some(not)), None, "{not}");
        }
        assert_eq!(commit_of(None), None);
    }
}
