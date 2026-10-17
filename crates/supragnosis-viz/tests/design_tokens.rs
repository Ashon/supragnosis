//! docs/design-system.md U7: the viewer, the desktop app's settings page and the landing are three
//! stylesheets of one product, and each keeps its own copy of the tokens. The viewer's is compiled
//! into the binary and served under a policy that names no other style source, the settings page
//! carries its own policy, and the landing is a static site - so nothing but this file keeps the
//! copies in step. Before it, the three disagreed on names (`--ink` and `--text`, `--accent` and
//! `--gold`) and on two values (`--panel`, and the red), and `--faint` drew text at 3.3:1.

const VIEWER: &str = include_str!("../assets/viewer.css");
const SETTINGS: &str = include_str!("../../../app/assets/settings.css");
const LANDING: &str = include_str!("../../../site/style.css");

/// Declared by every surface (Section 3).
const CORE: &[&str] =
    &["--surface", "--panel", "--line", "--ink", "--muted", "--faint", "--accent"];
/// What text is drawn in, and what it is drawn on.
const TEXT: &[&str] = &["--ink", "--ink2", "--muted", "--faint"];
const SURFACES: &[&str] = &["--surface", "--panel", "--panel-2", "--panel-3"];

/// The custom properties of a stylesheet's first `:root` block, comments removed, each value with its
/// whitespace dropped and lowercased so `rgba(217, 165, 68, 0.35)` and `rgba(217,165,68,0.35)` agree.
fn tokens(css: &str) -> Vec<(String, String)> {
    let start = css.find(":root {").expect("a :root block");
    let body = &css[start + ":root {".len()..];
    let body = &body[..body.find('}').expect("the :root block closes")];
    let mut clean = String::new();
    let mut rest = body;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    clean.push_str(rest);
    clean
        .split(';')
        .filter_map(|decl| {
            let (name, value) = decl.split_once(':')?;
            let name = name.trim();
            name.starts_with("--").then(|| {
                let value: String = value.split_whitespace().collect::<String>().to_lowercase();
                (name.to_string(), value)
            })
        })
        .collect()
}

fn surfaces() -> [(&'static str, Vec<(String, String)>); 3] {
    [
        ("viewer.css", tokens(VIEWER)),
        ("settings.css", tokens(SETTINGS)),
        ("site/style.css", tokens(LANDING)),
    ]
}

fn value<'a>(set: &'a [(String, String)], name: &str) -> Option<&'a str> {
    set.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Every token two surfaces both declare has one value, and every surface declares the core.
#[test]
fn the_surfaces_share_one_token_set() {
    let all = surfaces();
    let mut problems = Vec::new();
    for (file, set) in &all {
        assert!(set.len() >= 10, "{file}: the :root block was not read ({} tokens)", set.len());
        for core in CORE {
            if value(set, core).is_none() {
                problems.push(format!("{file} does not declare {core}"));
            }
        }
    }
    for (i, (file_a, a)) in all.iter().enumerate() {
        for (file_b, b) in &all[i + 1..] {
            for (name, va) in a {
                if let Some(vb) = value(b, name) {
                    if va != vb {
                        problems.push(format!("{name}: {file_a} {va} but {file_b} {vb}"));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the three stylesheets disagree:\n  {}",
        problems.join("\n  ")
    );
}

fn luminance(hex: &str) -> f64 {
    let hex = hex.strip_prefix('#').expect("a #rrggbb color");
    assert_eq!(hex.len(), 6, "a #rrggbb color: {hex}");
    let channel = |i: usize| {
        let v = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Every text token measures at least 4.5:1 on every surface token each stylesheet declares - the
/// WCAG AA floor for body text, which `--faint` missed at 3.3:1 until the design review measured it.
#[test]
fn every_text_token_meets_4_5_on_every_surface() {
    let mut low = Vec::new();
    let mut pairs = 0;
    for (file, set) in &surfaces() {
        for text in TEXT {
            let Some(fg) = value(set, text) else { continue };
            for ground in SURFACES {
                let Some(bg) = value(set, ground) else {
                    continue;
                };
                pairs += 1;
                let ratio = contrast(fg, bg);
                if ratio < 4.5 {
                    low.push(format!("{file}: {text} on {ground} is {ratio:.2}:1"));
                }
            }
        }
    }
    assert!(pairs >= 20, "the pairs were read ({pairs})");
    assert!(low.is_empty(), "text under 4.5:1:\n  {}", low.join("\n  "));
}
