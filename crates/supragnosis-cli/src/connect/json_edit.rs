//! connect's file edits: a minimal JSON editor that keeps every other byte, and JSONC for OpenCode.

use super::*;

// --- A minimal JSON editor: change one member of one object, keep every other byte ----------------

#[derive(Debug)]
pub(super) struct Member {
    pub(super) key: String,
    /// Start of the key's opening quote.
    start: usize,
    /// The value's byte range.
    pub(super) value: (usize, usize),
}

#[derive(Debug)]
pub(super) struct Object {
    pub(super) open: usize,
    pub(super) close: usize,
    pub(super) members: Vec<Member>,
}

pub(super) fn skip_ws(b: &[u8], mut i: usize) -> usize {
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

pub(super) fn object_at(text: &str, i: usize) -> Result<Object, String> {
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
            Some(b',') => {
                j = skip_ws(b, j + 1);
                // A trailing comma: JSONC's. Strict JSON never reaches here with one - its parse
                // refused it first.
                if b.get(j) == Some(&b'}') {
                    return Ok(Object { open: i, close: j, members });
                }
            }
            Some(b'}') => return Ok(Object { open: i, close: j, members }),
            _ => return Err("expected ',' or '}'".into()),
        }
    }
}

/// A JSON array in the text: its brackets and each item's byte range.
pub(super) struct Array {
    pub(super) open: usize,
    pub(super) close: usize,
    pub(super) items: Vec<(usize, usize)>,
}

pub(super) fn array_at(text: &str, i: usize) -> Result<Array, String> {
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

/// The indentation unit the file already uses (the first indented line), two spaces otherwise.
pub(super) fn indent_unit(text: &str) -> String {
    text.lines()
        .find_map(|l| {
            let n = l.len() - l.trim_start().len();
            (n > 0 && !l.trim().is_empty()).then(|| l[..n].to_string())
        })
        .unwrap_or_else(|| "  ".to_string())
}

/// `value` pretty-printed in the file's own indentation step, with every line after the first
/// indented by `pad`. Only leading whitespace is rewritten - a string value is never touched.
pub(super) fn pretty_at(value: &Value, unit: &str, pad: &str) -> String {
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

/// The byte range that takes member `pos` out of `obj` with the separator on one side: the
/// preceding comma when a member precedes it, the following one otherwise.
pub(super) fn cut_member(obj: &Object, pos: usize) -> (usize, usize) {
    let m = &obj.members[pos];
    if pos > 0 {
        (obj.members[pos - 1].value.1, m.value.1)
    } else if let Some(next) = obj.members.get(1) {
        (m.start, next.start)
    } else {
        (obj.open + 1, obj.close)
    }
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

// --- JSONC, for OpenCode: `//` and `/* */` comments and trailing commas, nothing more -----------

/// `text` with every comment blanked to spaces (line ends kept), so an offset in the result is the
/// same offset in `text` and the scanner above can walk it.
fn blank_comments(text: &str) -> Result<String, String> {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1)) {
            (b'"', _) => i = string_end(b, i)?,
            (b'/', Some(b'/')) => {
                while i < b.len() && b[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            (b'/', Some(b'*')) => {
                let end = text[i + 2..]
                    .find("*/")
                    .map(|n| i + 2 + n + 2)
                    .ok_or_else(|| "unterminated comment".to_string())?;
                for (j, c) in out.iter_mut().enumerate().take(end).skip(i) {
                    if b[j] != b'\n' {
                        *c = b' ';
                    }
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    // Only whole comments became spaces, so what is left is still UTF-8.
    String::from_utf8(out).map_err(|e| e.to_string())
}

/// Blanks the commas JSONC allows before a closing bracket, for a strict parser.
fn blank_trailing_commas(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i = string_end(b, i).unwrap_or(b.len());
                continue;
            }
            b',' if matches!(b.get(skip_ws(b, i + 1)), Some(b'}') | Some(b']')) => out[i] = b' ',
            _ => {}
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// A JSONC document as a value: comments and trailing commas, nothing more (not JSON5).
pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    let bare = blank_trailing_commas(&blank_comments(text)?);
    serde_json::from_str(&bare).map_err(|e| format!("not valid JSONC ({e})"))
}

/// Removes `path.name` from a JSONC document, cutting one span and leaving every other byte - every
/// comment outside the span - as it was (client-connect.md Section 13.3). `Ok(None)` when it is not
/// there.
pub fn remove_jsonc(text: &str, path: &[&str], name: &str) -> Result<Option<String>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let original = parse_jsonc(text)?;
    let bare = blank_comments(text)?;
    let b = bare.as_bytes();
    let mut obj = object_at(&bare, skip_ws(b, 0))?;
    for (n, key) in path.iter().enumerate() {
        let Some(m) = obj.members.iter().find(|m| m.key == *key) else {
            return Ok(None);
        };
        obj = object_at(&bare, m.value.0)
            .map_err(|_| format!("\"{}\" is not an object", path[..=n].join(".")))?;
    }
    let Some(pos) = obj.members.iter().position(|m| m.key == name) else {
        return Ok(None);
    };
    let m = &obj.members[pos];
    // The member and its separating comma: the one after it, or for a last member with none after
    // it, the one before it - with whatever lies directly between them. A comment between that
    // earlier comma and the member is most often the previous member's, so there the comma stays,
    // as a trailing comma - legal, and the file holds comments already, so it is JSONC already.
    let after = skip_ws(b, m.value.1);
    let before = (pos > 0).then(|| skip_ws(b, obj.members[pos - 1].value.1));
    let commented = |from: usize| !text[from..m.start].trim().is_empty();
    let (mut c0, mut c1) = match before {
        _ if b.get(after) == Some(&b',') => (m.start, after + 1),
        Some(comma) if !commented(comma + 1) => (comma, m.value.1),
        _ => (m.start, m.value.1),
    };
    // A member on a line of its own takes the line's indentation and end with it. Read in `text`,
    // so a comment sharing the line keeps the line.
    let flat = |s: &str| s.bytes().all(|c| matches!(c, b' ' | b'\t' | b'\r'));
    let line0 = text[..c0].rfind('\n').map_or(0, |n| n + 1);
    if let Some(end) = text[c1..].find('\n').map(|n| c1 + n) {
        if flat(&text[line0..c0]) && flat(&text[c1..end]) {
            (c0, c1) = (line0, end + 1);
        }
    }
    // The span holds the member, at most one comma and whitespace once comments are blanked.
    let around = format!("{}{}", &bare[c0..m.start], &bare[m.value.1..c1]);
    if around.bytes().filter(|&c| c == b',').count() > 1
        || !around.bytes().all(|c| c == b',' || c.is_ascii_whitespace())
    {
        return Err("the cut would take more than the entry - not written".into());
    }
    let out = format!("{}{}", &text[..c0], &text[c1..]);
    let mut want = original;
    let section = path.iter().try_fold(&mut want, |v, k| v.get_mut(*k));
    section.and_then(Value::as_object_mut).map(|s| s.remove(name));
    let edited = parse_jsonc(&out)
        .map_err(|e| format!("edit produced invalid JSONC ({e}) - not written"))?;
    if edited != want {
        return Err("edit would change more than the entry - not written".into());
    }
    Ok(Some(out))
}

/// The entry at `path.name` in a parsed document, `None` where any step is missing.
pub(super) fn entry_at(doc: &Value, path: &[&str], name: &str) -> Entry {
    path.iter()
        .try_fold(doc, |v, k| v.get(*k))
        .and_then(|s| s.get(name))
        .map(classify)
        .unwrap_or(Entry::None)
}

/// Reads one entry from a JSONC file, as `read_json_entry` does from a JSON one.
pub fn read_jsonc_entry(path: &Path, section: &[&str], name: &str) -> Entry {
    let Ok(text) = read_config(path) else {
        return Entry::Unknown;
    };
    if text.trim().is_empty() {
        return Entry::None;
    }
    match parse_jsonc(&text) {
        Ok(v) => entry_at(&v, section, name),
        Err(_) => Entry::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::tests::bridge;

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
