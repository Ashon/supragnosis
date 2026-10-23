//! `supragnosis hook claude-code` - the capture hook (docs/client-connect.md Section 12).
//!
//! Claude Code runs this after a `Bash` tool call and at a session's start, with the event's JSON on
//! stdin. It answers with one line of context for the agent, or with nothing. It never blocks (C6):
//! the answer is always `hookSpecificOutput.additionalContext`, never `decision`. It never writes to
//! the store: the agent does, through the bridge, if it decides there is knowledge to record.
//!
//! Everything that touches the machine - git, the daemon's viewer socket, the clock, the
//! environment - comes in through [`World`], so the decision is a function of its inputs and the
//! tests run it against a scratch repository and canned daemon answers.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// How long after a commit HEAD still counts as the commit the agent just made. A `git commit` that
/// failed leaves HEAD where it was, and a HEAD older than this was made by someone else.
pub const FRESH_SECS: u64 = 60;

/// Runs `git` with these arguments in a directory and returns stdout when it succeeded.
pub type Git<'a> = &'a dyn Fn(&Path, &[&str]) -> Option<String>;
/// One GET on the daemon's viewer socket, returning the body when the daemon answered in time.
pub type Daemon<'a> = &'a dyn Fn(&str) -> Option<String>;

/// What the hook may consult. Nothing else reaches it.
pub struct World<'a> {
    /// Seconds since the epoch, now.
    pub now: u64,
    pub git: Git<'a>,
    pub daemon: Daemon<'a>,
    /// `SUPRAGNOSIS_WORKSPACE` when it is set: names the workspace instead of the repository.
    pub workspace_override: Option<String>,
}

/// The hook's answer to one event. `Ok(None)` is silence: exit 0 and nothing on stdout.
pub fn answer(input: &Value, w: &World) -> Result<Option<Value>, String> {
    let event = input
        .get("hook_event_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "the hook input has no hook_event_name".to_string())?;
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "the hook input has no cwd".to_string())?;
    let context = match event {
        "PostToolUse" => after_commit(input, &cwd, w),
        "SessionStart" => session_start(&cwd, w),
        _ => None,
    };
    Ok(context.map(|c| {
        serde_json::json!({
            "hookSpecificOutput": { "hookEventName": event, "additionalContext": c }
        })
    }))
}

/// After a `Bash` call that named `git commit` and left a fresh HEAD: the facts of the commit and
/// the question, once.
fn after_commit(input: &Value, cwd: &Path, w: &World) -> Option<String> {
    if input.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return None;
    }
    let command = input.pointer("/tool_input/command").and_then(Value::as_str)?;
    if !names_git_commit(command) {
        return None;
    }
    let head = (w.git)(cwd, &["log", "-1", "--format=%H%x1f%ct%x1f%an%x1f%s"])?;
    let mut parts = head.trim_end_matches('\n').splitn(4, '\x1f');
    let id = parts.next()?.trim().to_string();
    let committed: u64 = parts.next()?.trim().parse().ok()?;
    let author = parts.next()?.trim().to_string();
    let subject = parts.next().unwrap_or("").trim().to_string();
    if id.is_empty() || committed + FRESH_SECS < w.now {
        return None;
    }
    let (workspace, repo) = workspace_of(cwd, w);
    let short = &id[..id.len().min(7)];
    Some(format!(
        "Commit {short} {subject:?} landed in {repo} (author {author}). If it settled a decision, a \
         constraint or a fact about the project that the diff does not show - why it was done this \
         way, what was ruled out, what it depends on - record it now with supragnosis observe: \
         workspace {workspace:?}, on_behalf_of {author:?}, source_ref \"git:{id}\". If it settled \
         nothing, record nothing."
    ))
}

/// Whether a shell command runs `git commit`: in some segment of the line, `git` (or a path to it)
/// is the program and `commit` is among its arguments. `git log --grep commit` passes this and is
/// caught by HEAD's age instead; `git commit-tree` does not pass it.
pub fn names_git_commit(command: &str) -> bool {
    command.split(['\n', ';', '|', '&']).any(|segment| {
        let mut tokens = segment
            .split_whitespace()
            .skip_while(|t| t.contains('=') && !t.starts_with('-') && !t.contains('/'));
        let Some(program) = tokens.next() else {
            return false;
        };
        (program == "git" || program.ends_with("/git")) && tokens.any(|t| t == "commit")
    })
}

/// The workspace the hook names, and the repository's name for the text: the directory of the
/// repository `cwd` is in, or `cwd` itself outside one; the override wins for the workspace.
///
/// A linked worktree is named by the repository it shares a `.git` with, not by its own directory,
/// so work in a worktree lands in its project's workspace. A submodule's common directory sits
/// under its parent's `.git/modules`, so its own top level names it.
fn workspace_of(cwd: &Path, w: &World) -> (String, String) {
    let git_path = |args: &[&str]| {
        (w.git)(cwd, args)
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty())
    };
    let top = git_path(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .filter(|d| d.file_name().is_some_and(|n| n == ".git"))
        .and_then(|d| d.parent().map(Path::to_path_buf))
        .or_else(|| git_path(&["rev-parse", "--show-toplevel"]))
        .unwrap_or_else(|| cwd.to_path_buf());
    let repo = top
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("this directory")
        .to_string();
    let workspace = w
        .workspace_override
        .clone()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| repo.clone());
    (workspace, repo)
}

/// At a session's start: what the workspace holds on this node, in one line, when a daemon answers.
fn session_start(cwd: &Path, w: &World) -> Option<String> {
    let (workspace, _) = workspace_of(cwd, w);
    let known: Vec<String> = serde_json::from_str(&(w.daemon)("/api/workspaces")?).ok()?;
    if !known.iter().any(|k| k == &workspace) {
        let others = if known.is_empty() {
            "this node holds no workspace yet".to_string()
        } else {
            let shown: Vec<String> = known.iter().take(5).map(|k| format!("{k:?}")).collect();
            let more = known.len().saturating_sub(shown.len());
            format!(
                "this node holds {}{}",
                shown.join(", "),
                if more > 0 { format!(" and {more} more") } else { String::new() }
            )
        };
        return Some(format!(
            "supragnosis: workspace {workspace:?} is not known on this node yet ({others}); the \
             first observe creates it."
        ));
    }
    let graph: Value =
        serde_json::from_str(&(w.daemon)(&format!("/api/graph?workspace={}", encode(&workspace)))?)
            .ok()?;
    let nodes = graph.pointer("/stats/node_count").and_then(Value::as_u64)?;
    let edges = graph.pointer("/stats/edge_count").and_then(Value::as_u64)?;
    Some(format!(
        "supragnosis: workspace {workspace:?} holds {nodes} entities and {edges} relations on this \
         node. Search it (search_knowledge, workspace_map) before assuming what this project has \
         decided; the brief prompt reads it as a whole."
    ))
}

/// Percent-encodes a query value: unreserved bytes pass, every other byte of its UTF-8 is `%XX`.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A scratch repository with one commit, made with the real git so the hook's parsing is
    /// exercised against what git prints.
    fn repo_with_a_commit(subject: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "supragnosis-hook-{}-{}",
            std::process::id(),
            subject.len() + subject.bytes().map(usize::from).sum::<usize>()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "Ada")
                .env("GIT_AUTHOR_EMAIL", "ada@example.test")
                .env("GIT_COMMITTER_NAME", "Ada")
                .env("GIT_COMMITTER_EMAIL", "ada@example.test")
                .output()
                .expect("git runs");
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", subject]);
        dir
    }

    fn real_git(dir: &Path, args: &[&str]) -> Option<String> {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn no_daemon(_: &str) -> Option<String> {
        None
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn post_tool_use(cwd: &Path, command: &str) -> Value {
        serde_json::json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "cwd": cwd, "tool_input": {"command": command}, "tool_response": {"stdout": ""}
        })
    }

    fn context_of(v: &Value) -> &str {
        v.pointer("/hookSpecificOutput/additionalContext")
            .and_then(Value::as_str)
            .unwrap()
    }

    /// 12.6: a repository with a fresh commit and a Bash call naming `git commit` - the answer
    /// carries the id, the subject, the author and the workspace, and no `decision`.
    #[test]
    fn a_commit_that_landed_asks_for_the_decision_behind_it() {
        let repo = repo_with_a_commit("the bridge opens a new connection per request");
        let id = real_git(&repo, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        let w = World { now: now(), git: &real_git, daemon: &no_daemon, workspace_override: None };
        let out = answer(&post_tool_use(&repo, "git commit -q -m x"), &w)
            .unwrap()
            .expect("answered");
        let text = context_of(&out);
        assert!(text.contains(&id[..7]) && text.contains(&format!("git:{id}")), "{text}");
        assert!(text.contains("\"the bridge opens a new connection per request\""), "{text}");
        assert!(text.contains("on_behalf_of \"Ada\""), "{text}");
        let repo_name = repo.file_name().unwrap().to_str().unwrap();
        assert!(text.contains(&format!("workspace {repo_name:?}")), "{text}");
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert!(out.get("decision").is_none(), "C6: the hook never blocks");

        // The override names the workspace; the repository is still named in the text.
        let w2 = World {
            now: now(),
            git: &real_git,
            daemon: &no_daemon,
            workspace_override: Some("folio".into()),
        };
        let out2 = answer(&post_tool_use(&repo, "git commit -m y"), &w2).unwrap().unwrap();
        assert!(context_of(&out2).contains("workspace \"folio\""), "{out2}");
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// 12.6: a command that is not `git commit`; a `git commit` with nothing staged (HEAD did not
    /// move, and is older than the budget by the clock); a Bash call from another tool: silence.
    #[test]
    fn a_command_that_landed_no_commit_gets_no_answer() {
        let repo = repo_with_a_commit("first");
        let fresh =
            World { now: now(), git: &real_git, daemon: &no_daemon, workspace_override: None };
        for cmd in ["cargo test", "git status", "git commit-tree HEAD^{tree}", "git log --oneline"]
        {
            assert!(answer(&post_tool_use(&repo, cmd), &fresh).unwrap().is_none(), "{cmd}");
        }
        // The same `git commit` an hour later, when HEAD is what it was: nothing landed.
        let later = World {
            now: now() + 3600,
            git: &real_git,
            daemon: &no_daemon,
            workspace_override: None,
        };
        assert!(answer(&post_tool_use(&repo, "git commit -m nothing"), &later)
            .unwrap()
            .is_none());
        // Not a Bash call at all.
        let mut other = post_tool_use(&repo, "git commit -m x");
        other["tool_name"] = Value::String("Write".into());
        assert!(answer(&other, &fresh).unwrap().is_none());
        // Outside any repository: git answers nothing, and so does the hook.
        let empty =
            std::env::temp_dir().join(format!("supragnosis-hook-empty-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        assert!(answer(&post_tool_use(&empty, "git commit -m x"), &fresh).unwrap().is_none());
        // Input that is not an event is an error the command reports, not silence.
        assert!(answer(&serde_json::json!({"cwd": "/"}), &fresh).is_err());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn git_commit_is_recognised_in_the_shapes_agents_write_it() {
        for yes in [
            "git commit -m x",
            "git add -A && git commit -q -F - <<'EOF'\nmsg\nEOF",
            "cd /x && GIT_EDITOR=true git commit --amend --no-edit",
            "/usr/bin/git -C /repo commit -m x; echo done",
            "git -c user.name=a commit -m x | cat",
        ] {
            assert!(names_git_commit(yes), "{yes}");
        }
        for no in ["git commit-tree x", "echo git commit", "cargo test", "git status && ls"] {
            assert!(!names_git_commit(no), "{no}");
        }
    }

    /// 12.6: against a daemon's answers for a seeded store: the counts; for a workspace it has
    /// never seen: the first observe creates it; with no daemon: no output.
    #[test]
    fn a_session_start_says_what_the_workspace_holds() {
        let repo = repo_with_a_commit("seed");
        let name = repo.file_name().unwrap().to_str().unwrap().to_string();
        let input = serde_json::json!({
            "session_id": "s", "hook_event_name": "SessionStart", "source": "startup", "cwd": repo
        });
        let seeded_name = name.clone();
        let seeded = move |path: &str| -> Option<String> {
            if path == "/api/workspaces" {
                return Some(serde_json::json!([seeded_name, "default"]).to_string());
            }
            assert_eq!(path, format!("/api/graph?workspace={}", encode(&seeded_name)), "{path}");
            Some(
                serde_json::json!({"nodes": [], "edges": [], "stats": {"node_count": 168, "edge_count": 274}})
                    .to_string(),
            )
        };
        let w = World { now: now(), git: &real_git, daemon: &seeded, workspace_override: None };
        let out = answer(&input, &w).unwrap().expect("answered");
        let text = context_of(&out);
        assert!(
            text.contains(&format!("workspace {name:?} holds 168 entities and 274 relations")),
            "{text}"
        );
        assert!(text.contains("search_knowledge"), "{text}");
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "SessionStart");

        let unknown = |path: &str| -> Option<String> {
            assert_eq!(path, "/api/workspaces");
            Some(serde_json::json!(["default", "folio"]).to_string())
        };
        let w = World { now: now(), git: &real_git, daemon: &unknown, workspace_override: None };
        let text = answer(&input, &w).unwrap().expect("answered");
        let text = context_of(&text);
        assert!(text.contains("is not known on this node yet"), "{text}");
        assert!(
            text.contains("\"default\", \"folio\"") && text.contains("first observe creates it"),
            "{text}"
        );

        let w = World { now: now(), git: &real_git, daemon: &no_daemon, workspace_override: None };
        assert!(answer(&input, &w).unwrap().is_none(), "no daemon, no line");

        // A workspace name with a space reaches the daemon encoded.
        let spaced = |path: &str| -> Option<String> {
            if path == "/api/workspaces" {
                return Some(serde_json::json!(["my project"]).to_string());
            }
            assert_eq!(path, "/api/graph?workspace=my%20project");
            Some(serde_json::json!({"stats": {"node_count": 1, "edge_count": 0}}).to_string())
        };
        let w = World {
            now: now(),
            git: &real_git,
            daemon: &spaced,
            workspace_override: Some("my project".into()),
        };
        assert!(context_of(&answer(&input, &w).unwrap().unwrap()).contains("holds 1 entities"));
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// Work in a linked worktree is named by its repository, never by the worktree's directory.
    #[test]
    fn a_worktree_is_named_by_its_repository() {
        let repo = repo_with_a_commit("worktree naming");
        let tree = repo.with_file_name(format!(
            "{}-agent-ab67762782df741b3",
            repo.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&tree);
        let added = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&tree)
            .output()
            .expect("git runs");
        assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stderr));
        let w = World { now: now(), git: &real_git, daemon: &no_daemon, workspace_override: None };
        let name = repo.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(workspace_of(&tree, &w), (name.clone(), name.clone()), "from the worktree");
        assert_eq!(workspace_of(&repo.join("."), &w), (name.clone(), name), "from the checkout");
        let _ = std::fs::remove_dir_all(&tree);
        let _ = std::fs::remove_dir_all(&repo);
    }
}
