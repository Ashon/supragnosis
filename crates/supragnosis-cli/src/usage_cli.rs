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
    /// Commit questions, by commit, whose commit an `observe` in the window names as its source.
    pub answered_with_a_record: u64,
}

/// The calls that change what this node holds (docs/usage.md Section 4.1); every other call reads.
const WRITES: &[&str] = &["observe", "define_type", "propose", "review", "sync_pull"];

/// One UTC hour that holds a line, for the trends Settings > Usage draws (docs/usage.md Section 4.1).
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Hour {
    /// The hour's start, in milliseconds since the epoch.
    pub t: u64,
    pub calls: u64,
    pub writes: u64,
    pub empty: u64,
    pub not_found: u64,
    pub errors: u64,
    pub by_name: Vec<(String, u64)>,
    pub by_workspace: Vec<(String, u64)>,
    pub by_app: Vec<(String, u64)>,
    pub session_indexes: u64,
    pub subagent_indexes: u64,
    /// Commit questions first asked in this hour, and how many of those a record answers.
    pub commit_questions: u64,
    pub answered: u64,
}

/// An hour as it is counted, before its tallies are sorted into pairs.
#[derive(Default)]
struct Tally {
    hour: Hour,
    names: BTreeMap<String, u64>,
    workspaces: BTreeMap<String, u64>,
    apps: BTreeMap<String, u64>,
}

/// Counts as `[name, count]` pairs, the largest first.
fn pairs(counts: BTreeMap<String, u64>) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// A function as the tables name it: a tool by its name, a resource or a prompt with its kind.
fn label(kind: &str, name: &str) -> String {
    if kind == "tool" {
        name.to_string()
    } else {
        format!("{kind}:{name}")
    }
}

#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub(crate) struct Summary {
    pub schema: u64,
    /// The window's start, in milliseconds since the epoch.
    pub since: u64,
    pub calls: u64,
    /// Lines that were not usage records and were left out.
    pub unreadable: u64,
    pub by_call: Vec<CallRow>,
    pub by_app: Vec<(String, u64)>,
    pub hook: HookUse,
    /// The window by UTC hour, oldest first.
    pub by_hour: Vec<Hour>,
}

/// Reads `--since` as milliseconds: `7d`, `12h`.
pub(crate) fn since_millis(arg: &str) -> Result<u64> {
    let arg = arg.trim();
    let cut = arg.char_indices().last().map_or(0, |(i, _)| i);
    let (n, unit) = arg.split_at(cut);
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
    let mut sum = Summary { schema: connect_cli::JSON_SCHEMA, since, ..Default::default() };
    let mut rows: BTreeMap<(String, String, String), CallRow> = BTreeMap::new();
    let mut apps: BTreeMap<String, u64> = BTreeMap::new();
    let mut hours: BTreeMap<u64, Tally> = BTreeMap::new();
    // Each commit asked about, with when it was first asked.
    let mut asked: BTreeMap<String, u64> = BTreeMap::new();
    let mut recorded: BTreeSet<String> = BTreeSet::new();
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
        let tally = hours.entry(t - t % 3_600_000).or_default();
        if kind == "hook" {
            let n = v["n"].as_u64().unwrap_or(0);
            match name {
                "session_start" => {
                    sum.hook.session_indexes += u64::from(n > 0);
                    sum.hook.session_lines += n;
                    tally.hour.session_indexes += u64::from(n > 0);
                }
                "subagent_start" => {
                    sum.hook.subagent_indexes += u64::from(n > 0);
                    tally.hour.subagent_indexes += u64::from(n > 0);
                }
                "commit_question" => {
                    if let Some(c) = v["commit"].as_str() {
                        let first = asked.entry(c.to_string()).or_insert(t);
                        *first = (*first).min(t);
                    }
                }
                _ => {}
            }
            continue;
        }
        sum.calls += 1;
        let app = v["app"].as_str().unwrap_or("-").to_string();
        *apps.entry(app.clone()).or_default() += 1;
        tally.hour.calls += 1;
        tally.hour.writes += u64::from(kind == "tool" && WRITES.contains(&name));
        *tally.names.entry(label(kind, name)).or_default() += 1;
        *tally.workspaces.entry(workspace.clone()).or_default() += 1;
        *tally.apps.entry(app).or_default() += 1;
        // A failed observe recorded nothing, so it answers no question.
        if name == "observe" && v["outcome"].as_str() != Some("error") {
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
        let hour = &mut tally.hour;
        match v["outcome"].as_str() {
            Some("empty") => {
                row.empty += 1;
                hour.empty += 1;
            }
            Some("not_found") => {
                row.not_found += 1;
                hour.not_found += 1;
            }
            Some("error") => {
                row.errors += 1;
                hour.errors += 1;
            }
            _ => {}
        }
    }
    // Questions are counted by commit, since a second commit command within the minute asks about
    // the same commit again. A question is answered when a record names the commit; a short id
    // names the full one.
    let answered = |q: &str| recorded.iter().any(|r| q.starts_with(r.as_str()) || r.starts_with(q));
    for (q, t) in &asked {
        let hour = &mut hours.entry(t - t % 3_600_000).or_default().hour;
        hour.commit_questions += 1;
        hour.answered += u64::from(answered(q));
    }
    sum.hook.commit_questions = asked.len() as u64;
    sum.hook.answered_with_a_record = asked.keys().filter(|q| answered(q)).count() as u64;
    sum.by_hour = hours
        .into_iter()
        .map(|(t, tally)| Hour {
            t,
            by_name: pairs(tally.names),
            by_workspace: pairs(tally.workspaces),
            by_app: pairs(tally.apps),
            ..tally.hour
        })
        .collect();
    let mut by_call: Vec<CallRow> = rows.into_values().collect();
    by_call.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.name.cmp(&b.name)));
    sum.by_call = by_call;
    sum.by_app = pairs(apps);
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
            out += &format!(
                "{:<w$}  {:<ws$}  {:>6}  {:>6}  {:>9}  {:>6}\n",
                label(&r.kind, &r.name),
                r.workspace,
                r.calls,
                r.empty,
                r.not_found,
                r.errors
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
    // Read lossily: one torn line must not cost the rest of the file.
    let text = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        // The app reads an empty window rather than a sentence (docs/usage.md Section 4).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && a.json => String::new(),
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
            r#"{"t":1012,"kind":"hook","name":"commit_question","workspace":"supragnosis","commit":"9999999"}"#,
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

    /// docs/usage.md Section 4.1: each UTC hour holds its calls by function, workspace and app,
    /// the writes among them and their outcomes, and the hook's indexes; a commit question counts in
    /// the hour it was first asked, answered when an observe names the commit.
    #[test]
    fn the_window_is_counted_by_hour() {
        const H: u64 = 3_600_000;
        let lines = [
            r#"{"t":7200000,"kind":"tool","name":"search_knowledge","workspace":"a","app":"X","outcome":"ok"}"#,
            r#"{"t":7200100,"kind":"tool","name":"observe","workspace":"a","app":"X","outcome":"ok","commit":"abc1234"}"#,
            r#"{"t":7200200,"kind":"hook","name":"session_start","workspace":"a","n":10,"of":20}"#,
            r#"{"t":7200300,"kind":"hook","name":"commit_question","workspace":"a","commit":"1234567"}"#,
            r#"{"t":10800005,"kind":"tool","name":"get_entity","app":"Y","outcome":"not_found"}"#,
            r#"{"t":10800006,"kind":"resource","name":"observation","app":"Y","outcome":"ok"}"#,
            r#"{"t":10800007,"kind":"hook","name":"commit_question","workspace":"a","commit":"abc1234ffff"}"#,
            r#"{"t":10800008,"kind":"hook","name":"commit_question","workspace":"a","commit":"1234567"}"#,
            r#"{"t":10800009,"kind":"tool","name":"observe","workspace":"a","app":"Y","outcome":"error","commit":"1234567"}"#,
        ];
        let sum = summarize(lines.iter().copied(), 0);
        let pair = |n: &str, c: u64| (n.to_string(), c);
        let [two, three] = &sum.by_hour[..] else {
            panic!("two hours: {:?}", sum.by_hour)
        };
        assert_eq!((two.t, three.t), (2 * H, 3 * H), "an hour is named by its start");
        assert_eq!((two.calls, two.writes, two.session_indexes), (2, 1, 1));
        assert_eq!(two.by_name, vec![pair("observe", 1), pair("search_knowledge", 1)]);
        assert_eq!(two.by_workspace, vec![pair("a", 2)]);
        assert_eq!(
            (two.commit_questions, two.answered),
            (1, 0),
            "asked first in this hour, and the observe that names it failed"
        );
        assert_eq!((three.calls, three.writes, three.not_found, three.errors), (3, 1, 1, 1));
        assert_eq!(
            three.by_name,
            vec![pair("get_entity", 1), pair("observe", 1), pair("resource:observation", 1)]
        );
        assert_eq!(
            (three.by_workspace.clone(), three.by_app.clone()),
            (vec![pair("-", 2), pair("a", 1)], vec![pair("Y", 3)])
        );
        assert_eq!((three.commit_questions, three.answered), (1, 1), "the observe names it");
    }

    #[test]
    fn since_reads_days_and_hours() {
        assert_eq!(since_millis("7d").unwrap(), 7 * 86_400_000);
        assert_eq!(since_millis("12h").unwrap(), 12 * 3_600_000);
        assert!(since_millis("7w").is_err() && since_millis("d").is_err());
        assert!(since_millis("7일").is_err(), "a multibyte unit is refused, not a panic");
    }
}
