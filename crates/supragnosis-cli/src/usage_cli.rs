//! `supragnosis usage` - what the usage log says about how this node is used (docs/usage.md
//! Section 4): calls by MCP function and workspace, by app, and what the Claude Code hook gave and
//! asked. Reads the file beside the viewer's socket; writes nothing.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(clap::Args)]
pub(crate) struct UsageArgs {
    /// How far back to read: a number of days (`7d`) or hours (`12h`)
    #[arg(long, default_value = "7d")]
    since: String,
    /// The same tables as JSON
    #[arg(long)]
    json: bool,
}

/// One row of the table by function and workspace.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct CallRow {
    pub kind: String,
    pub name: String,
    /// The workspace the calls named, or `-` when they named none.
    pub workspace: String,
    pub calls: u64,
    pub empty: u64,
    pub not_found: u64,
    pub errors: u64,
}

/// What the hook gave and asked in the window.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct HookUse {
    pub session_indexes: u64,
    /// Index lines shown to sessions, summed.
    pub session_lines: u64,
    pub subagent_indexes: u64,
    pub commit_questions: u64,
    /// Commit questions whose commit an `observe` in the window names as its source.
    pub answered_with_a_record: u64,
}

#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Summary {
    /// The window's start, in milliseconds since the epoch.
    pub since: u64,
    pub calls: u64,
    /// Lines that were not usage records and were left out.
    pub unreadable: u64,
    pub by_call: Vec<CallRow>,
    pub by_app: Vec<(String, u64)>,
    pub hook: HookUse,
}

/// Reads `--since` as milliseconds: `7d`, `12h`.
pub(crate) fn since_millis(arg: &str) -> Result<u64> {
    let arg = arg.trim();
    let (n, unit) = arg.split_at(arg.len().saturating_sub(1));
    let n: u64 = n
        .parse()
        .with_context(|| format!("--since {arg:?} is not a number of days or hours (7d, 12h)"))?;
    match unit {
        "d" => Ok(n * 86_400_000),
        "h" => Ok(n * 3_600_000),
        _ => anyhow::bail!("--since {arg:?} is not a number of days or hours (7d, 12h)"),
    }
}

/// The tables, from the log's lines at or after `since` (ms). A line that is not a usage record is
/// counted as unreadable rather than stopping the read.
pub(crate) fn summarize<'a>(lines: impl Iterator<Item = &'a str>, since: u64) -> Summary {
    let mut sum = Summary { since, ..Default::default() };
    let mut rows: BTreeMap<(String, String, String), CallRow> = BTreeMap::new();
    let mut apps: BTreeMap<String, u64> = BTreeMap::new();
    let (mut asked, mut recorded): (BTreeSet<String>, BTreeSet<String>) = Default::default();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            sum.unreadable += 1;
            continue;
        };
        let (Some(t), Some(kind), Some(name)) =
            (v["t"].as_u64(), v["kind"].as_str(), v["name"].as_str())
        else {
            sum.unreadable += 1;
            continue;
        };
        if t < since {
            continue;
        }
        let workspace = v["workspace"].as_str().unwrap_or("-").to_string();
        if kind == "hook" {
            let n = v["n"].as_u64().unwrap_or(0);
            match name {
                "session_start" => {
                    sum.hook.session_indexes += u64::from(n > 0);
                    sum.hook.session_lines += n;
                }
                "subagent_start" => sum.hook.subagent_indexes += u64::from(n > 0),
                "commit_question" => {
                    sum.hook.commit_questions += 1;
                    if let Some(c) = v["commit"].as_str() {
                        asked.insert(c.to_string());
                    }
                }
                _ => {}
            }
            continue;
        }
        sum.calls += 1;
        *apps.entry(v["app"].as_str().unwrap_or("-").to_string()).or_default() += 1;
        if name == "observe" {
            if let Some(c) = v["commit"].as_str() {
                recorded.insert(c.to_string());
            }
        }
        let row = rows
            .entry((kind.to_string(), name.to_string(), workspace.clone()))
            .or_insert_with(|| CallRow {
                kind: kind.into(),
                name: name.into(),
                workspace,
                ..Default::default()
            });
        row.calls += 1;
        match v["outcome"].as_str() {
            Some("empty") => row.empty += 1,
            Some("not_found") => row.not_found += 1,
            Some("error") => row.errors += 1,
            _ => {}
        }
    }
    // A commit question is answered when the record names the commit; a short id names the full one.
    sum.hook.answered_with_a_record = asked
        .iter()
        .filter(|q| recorded.iter().any(|r| q.starts_with(r.as_str()) || r.starts_with(q.as_str())))
        .count() as u64;
    let mut by_call: Vec<CallRow> = rows.into_values().collect();
    by_call.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name)));
    sum.by_call = by_call;
    let mut by_app: Vec<(String, u64)> = apps.into_iter().collect();
    by_app.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sum.by_app = by_app;
    sum
}

/// The tables as a person reads them in a terminal.
pub(crate) fn render(sum: &Summary) -> String {
    let mut out = format!(
        "Usage since {} (UTC): {} calls from {} app{}\n",
        hook::utc_day(sum.since / 1000),
        sum.calls,
        sum.by_app.len(),
        if sum.by_app.len() == 1 { "" } else { "s" }
    );
    if !sum.by_call.is_empty() {
        let w = sum
            .by_call
            .iter()
            .map(|r| r.name.len() + r.kind.len() + 1)
            .max()
            .unwrap_or(8)
            .max(8);
        let ws = sum.by_call.iter().map(|r| r.workspace.len()).max().unwrap_or(9).max(9);
        out += &format!(
            "\n{:<w$}  {:<ws$}  {:>6}  {:>6}  {:>9}  {:>6}\n",
            "FUNCTION", "WORKSPACE", "CALLS", "EMPTY", "NOT FOUND", "ERRORS"
        );
        for r in &sum.by_call {
            let name =
                if r.kind == "tool" { r.name.clone() } else { format!("{}:{}", r.kind, r.name) };
            out += &format!(
                "{name:<w$}  {:<ws$}  {:>6}  {:>6}  {:>9}  {:>6}\n",
                r.workspace, r.calls, r.empty, r.not_found, r.errors
            );
        }
    }
    if !sum.by_app.is_empty() {
        out += "\nAPP\n";
        for (app, n) in &sum.by_app {
            out += &format!("  {app}: {n}\n");
        }
    }
    let h = &sum.hook;
    out += &format!(
        "\nHOOK (Claude Code)\n  session indexes: {}{}\n  subagent indexes: {}\n  commit questions: {}, answered with a record: {}\n",
        h.session_indexes,
        h.session_lines
            .checked_div(h.session_indexes)
            .map(|each| format!(" (about {each} lines each)"))
            .unwrap_or_default(),
        h.subagent_indexes,
        h.commit_questions,
        h.answered_with_a_record
    );
    if sum.unreadable > 0 {
        out += &format!("\n{} line(s) were not usage records and were left out\n", sum.unreadable);
    }
    out
}

pub(crate) fn usage_cmd(a: UsageArgs) -> Result<()> {
    let sock = std::env::var("SUPRAGNOSIS_VIZ_SOCK")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(config::default_viz_sock);
    let path = serve::usage_path(&sock);
    let since = supragnosis_core::now_millis().saturating_sub(since_millis(&a.since)?);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "nothing recorded yet at {} - the daemon writes it as apps call it",
                path.display()
            );
            return Ok(());
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let sum = summarize(text.lines(), since);
    if a.json {
        println!("{}", serde_json::to_string_pretty(&sum)?);
    } else {
        print!("{}", render(&sum));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/usage.md Section 4: calls are counted by function and workspace with their outcomes,
    /// by app, inside the window only; the hook's indexes and questions are counted apart, and a
    /// question is answered when an observe names its commit.
    #[test]
    fn usage_counts_calls_by_function_and_workspace() {
        let lines = [
            r#"{"t":500,"kind":"tool","name":"search_knowledge","workspace":"supragnosis","app":"Claude Code","outcome":"ok","n":3}"#,
            r#"{"t":1000,"kind":"tool","name":"search_knowledge","workspace":"supragnosis","app":"Claude Code","outcome":"ok","n":3}"#,
            r#"{"t":1001,"kind":"tool","name":"search_knowledge","workspace":"supragnosis","app":"Codex","outcome":"empty","n":0}"#,
            r#"{"t":1002,"kind":"tool","name":"search_knowledge","app":"Claude Code","outcome":"ok","n":5}"#,
            r#"{"t":1003,"kind":"tool","name":"get_entity","app":"Claude Code","outcome":"not_found"}"#,
            r#"{"t":1004,"kind":"resource","name":"observation","app":"Claude Code","outcome":"ok"}"#,
            r#"{"t":1005,"kind":"tool","name":"observe","workspace":"supragnosis","app":"Claude Code","outcome":"ok","n":2,"commit":"abcdef1234"}"#,
            r#"{"t":1006,"kind":"tool","name":"observe","workspace":"verda","app":"Claude Code","outcome":"error"}"#,
            r#"{"t":1007,"kind":"hook","name":"session_start","workspace":"supragnosis","n":25,"of":90}"#,
            r#"{"t":1008,"kind":"hook","name":"session_start","workspace":"x","n":0,"of":0}"#,
            r#"{"t":1009,"kind":"hook","name":"subagent_start","workspace":"supragnosis","n":10,"of":90}"#,
            r#"{"t":1010,"kind":"hook","name":"commit_question","workspace":"supragnosis","commit":"abcdef1234aaaa"}"#,
            r#"{"t":1011,"kind":"hook","name":"commit_question","workspace":"supragnosis","commit":"9999999"}"#,
            "not json",
            r#"{"kind":"tool"}"#,
        ];
        let sum = summarize(lines.iter().copied(), 1000);
        assert_eq!(sum.calls, 7, "the line before the window is left out");
        assert_eq!(sum.unreadable, 2);
        let row = |name: &str, ws: &str| {
            sum.by_call
                .iter()
                .find(|r| r.name == name && r.workspace == ws)
                .unwrap_or_else(|| panic!("no row for {name} in {ws}: {:?}", sum.by_call))
        };
        let s = row("search_knowledge", "supragnosis");
        assert_eq!((s.calls, s.empty), (2, 1));
        assert_eq!(row("search_knowledge", "-").calls, 1, "a call that named no workspace");
        assert_eq!(row("get_entity", "-").not_found, 1);
        assert_eq!(row("observation", "-").kind, "resource");
        assert_eq!(row("observe", "verda").errors, 1);
        assert_eq!(sum.by_call[0].name, "search_knowledge", "the most called first");
        assert_eq!(sum.by_app, vec![("Claude Code".to_string(), 6), ("Codex".to_string(), 1)]);
        assert_eq!(
            sum.hook,
            HookUse {
                session_indexes: 1,
                session_lines: 25,
                subagent_indexes: 1,
                commit_questions: 2,
                answered_with_a_record: 1,
            }
        );
        let text = render(&sum);
        assert!(text.contains("7 calls from 2 apps"), "{text}");
        assert!(text.contains("resource:observation"), "{text}");
        assert!(text.contains("commit questions: 2, answered with a record: 1"), "{text}");
    }

    #[test]
    fn since_reads_days_and_hours() {
        assert_eq!(since_millis("7d").unwrap(), 7 * 86_400_000);
        assert_eq!(since_millis("12h").unwrap(), 12 * 3_600_000);
        assert!(since_millis("7w").is_err() && since_millis("d").is_err());
    }
}
