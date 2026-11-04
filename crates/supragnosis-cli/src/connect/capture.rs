//! The capture hook's registration in Claude Code's settings; the hook itself is src/hook.rs.

use super::*;

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
}
