//! The app's words (docs/i18n.md Sections 3 and 4): app/assets/words.json, one file for this Rust
//! and the app's pages, each entry a key and its English and Korean, so a word the app says in two
//! places is one entry. The language is the one macOS gives the app, read once; the shell hands a
//! page that language and its words (shell-init.js).

use super::*;

/// The languages the words hold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Lang {
    En,
    Ko,
}

impl Lang {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ko => "ko",
        }
    }
}

pub(crate) const WORDS_JSON: &str = include_str!("../assets/words.json");

fn table() -> &'static serde_json::Map<String, serde_json::Value> {
    static TABLE: OnceLock<serde_json::Map<String, serde_json::Value>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(WORDS_JSON).expect("app/assets/words.json is a map"))
}

/// The language macOS gives this app: of the two the words hold, the one the bundle prefers for the
/// person's languages, or the one picked for this app in System Settings > General > Language &
/// Region > Applications, which lists the app because Info.plist declares both
/// (`CFBundleLocalizations`). Read once. The tests read English, whatever the Mac running them
/// prefers.
pub(crate) fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        if cfg!(test) {
            return Lang::En;
        }
        use objc2_foundation::{NSArray, NSBundle, NSString};
        let held =
            NSArray::from_retained_slice(&[NSString::from_str("en"), NSString::from_str("ko")]);
        let first = NSBundle::preferredLocalizationsFromArray(&held).firstObject();
        if first.is_some_and(|l| l.to_string() == "ko") {
            Lang::Ko
        } else {
            Lang::En
        }
    })
}

/// An entry's words in a language: a string, or for English a `one` and an `other` form.
fn entry(key: &str, lang: Lang) -> &'static serde_json::Value {
    table()
        .get(key)
        .and_then(|e| e.get(lang.code()))
        .unwrap_or_else(|| panic!("app/assets/words.json has no {key:?} in {}", lang.code()))
}

/// A sentence in the app's language, its `{name}` placeholders filled (docs/i18n.md Section 4: one
/// entry per sentence, never one built by joining entries).
pub(crate) fn word_with(key: &str, values: &[(&str, &str)]) -> String {
    let e = entry(key, lang());
    let mut s = e.as_str().or_else(|| e["other"].as_str()).unwrap_or_default().to_string();
    for (name, value) in values {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

pub(crate) fn word(key: &str) -> String {
    word_with(key, &[])
}

/// What the shell hands a page (shell-init.js): the app's language and every entry's words in it.
pub(crate) fn page_words() -> String {
    let lang = lang();
    let words: serde_json::Map<String, serde_json::Value> =
        table().iter().map(|(k, _)| (k.clone(), entry(k, lang).clone())).collect();
    serde_json::json!({ "lang": lang.code(), "words": words }).to_string()
}

/// Whether `script` says the sentence `en`: whether it names the key of an entry whose English is
/// `en`, or one form of it. The tests that held a page's words by their English in the script hold
/// them so now, read from the words (docs/i18n.md D8).
#[cfg(test)]
pub(crate) fn says(script: &str, en: &str) -> bool {
    table().iter().any(|(key, e)| {
        let w = &e["en"];
        let is = w.as_str() == Some(en)
            || w.as_object().is_some_and(|f| f.values().any(|v| v.as_str() == Some(en)));
        is && script.contains(&format!("\"{key}\""))
    })
}
