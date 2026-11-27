//! What the viewer's tests share (docs/i18n.md Section 4, D8): the words tables as the words test
//! reads them, so a test holds a sentence by its English while the script names its key.
#![allow(dead_code)]

const VIEWER_WORDS: &str = include_str!("../../assets/viewer/00-words.js");

/// A script's words table - each entry's key, English and Korean - and the script after it. An entry
/// is one line, `key: { en: "...", ko: "..." },`.
pub fn words<'a>(file: &str, src: &'a str) -> (Vec<(String, String, String)>, &'a str) {
    let at = src.find("WORDS = {").unwrap_or_else(|| panic!("{file} has no words table"));
    let mut pos = at + src[at..].find('\n').expect("the table goes on") + 1;
    let mut out = Vec::new();
    for line in src[pos..].split_inclusive('\n') {
        pos += line.len();
        let line = line.trim();
        if line == "};" {
            return (out, &src[pos..]);
        }
        if line.starts_with("//") {
            continue;
        }
        let parsed = (|| {
            let (key, rest) = line.split_once(": { en: ")?;
            let (en, rest) = js_string(rest)?;
            let (ko, rest) = js_string(rest.strip_prefix(", ko: ")?)?;
            (rest == " }," || rest == " }").then(|| (key.into(), en, ko))
        })();
        out.push(parsed.unwrap_or_else(|| panic!("{file}: an entry that does not read: {line}")));
    }
    panic!("{file}: the words table does not close")
}

/// The double-quoted JavaScript string `s` starts with, its escaped characters unescaped, and what
/// follows it.
pub fn js_string(s: &str) -> Option<(String, &str)> {
    let s = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &s[i + 1..])),
            c => out.push(c),
        }
    }
    None
}

/// The keys of the viewer's entries whose English is `en`; none fails, so a test cannot hold a
/// sentence the words do not.
pub fn keys_of(en: &str) -> Vec<String> {
    let (table, _) = words("viewer/00-words.js", VIEWER_WORDS);
    let keys: Vec<String> =
        table.into_iter().filter(|(_, e, _)| e == en).map(|(k, ..)| k).collect();
    assert!(!keys.is_empty(), "the viewer's words hold no entry {en:?}");
    keys
}

/// Whether `src` says the sentence `en`: names, as a string, the key of a viewer entry whose English
/// is `en`.
pub fn says(src: &str, en: &str) -> bool {
    keys_of(en).iter().any(|k| src.contains(&format!("\"{k}\"")))
}
