//! docs/design-system.md Section 4 and U21: the viewer and the app's Settings and Setup pages draw
//! text at six sizes, each a token in rem, given by role, so a person's own text size scales them.
//! Before the third pass the viewer set 121 font sizes in pixels, the Settings page 24 and Setup 9,
//! and a browser's text size changed none of them.

const VIEWER: &str = include_str!("../assets/viewer.css");
const SETTINGS: &str = include_str!("../../../app/assets/settings.css");
const WELCOME: &str = include_str!("../../../app/assets/welcome.css");
const SCRIPT: &str = include_str!("../assets/viewer.js");

/// The six sizes and their values (Section 4): page 24, sheet 18, section 15, body 13, small 12 and
/// caption 11 pixels at the default root size.
const SIZES: &[(&str, &str)] = &[
    ("--text-page", "1.5rem"),
    ("--text-sheet", "1.125rem"),
    ("--text-section", "0.9375rem"),
    ("--text-body", "0.8125rem"),
    ("--text-small", "0.75rem"),
    ("--text-caption", "0.6875rem"),
];

/// A stylesheet's rules as (selector, declarations), comments dropped and whitespace folded.
fn rules(css: &str) -> Vec<(String, String)> {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    clean.push_str(rest);
    let mut out = Vec::new();
    let mut depth_start = 0usize;
    let bytes: Vec<char> = clean.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '{' {
            let sel: String = bytes[depth_start..i].iter().collect();
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != '{' && bytes[j] != '}' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == '}' {
                let body: String = bytes[i + 1..j].iter().collect();
                let sel = sel
                    .rsplit(['}', ';'])
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push((sel, body.split_whitespace().collect::<Vec<_>>().join(" ")));
                depth_start = j + 1;
                i = j + 1;
                continue;
            }
            depth_start = i + 1;
        } else if bytes[i] == '}' {
            depth_start = i + 1;
        }
        i += 1;
    }
    out
}

fn font_declarations(css: &str) -> Vec<(String, String)> {
    rules(css)
        .into_iter()
        .flat_map(|(sel, body)| {
            body.split(';')
                .map(str::trim)
                .filter(|d| d.starts_with("font:") || d.starts_with("font-size:"))
                .map(|d| (sel.clone(), d.to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// U21: no font size in pixels, and every size the three stylesheets set is one of the six tokens.
#[test]
fn every_font_size_is_a_token_in_rem() {
    for (name, value) in SIZES {
        for (file, css) in [("viewer.css", VIEWER), ("settings.css", SETTINGS)] {
            let decl = format!("{name}:{value}");
            let flat: String = css.split_whitespace().collect();
            assert!(flat.contains(&decl), "{file} does not declare {name} as {value}");
        }
    }
    let mut stray = Vec::new();
    for (file, css) in
        [("viewer.css", VIEWER), ("settings.css", SETTINGS), ("welcome.css", WELCOME)]
    {
        for (sel, decl) in font_declarations(css) {
            let px = decl
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                .any(|w| w.ends_with("px") && w[..w.len() - 2].parse::<f64>().is_ok());
            let sized = decl.starts_with("font-size:") || decl.split(' ').count() > 1;
            let token = SIZES.iter().any(|(n, _)| decl.contains(&format!("var({n})")));
            if px || (sized && !token && !decl.contains("inherit")) {
                stray.push(format!("{file}: {sel} {{ {decl} }}"));
            }
        }
    }
    assert!(stray.is_empty(), "a size off the six tokens:\n  {}", stray.join("\n  "));
}

/// Section 4's roles: running text and every control's label are body, 13; nothing a person reads
/// to act is set at the caption size.
#[test]
fn text_and_control_labels_are_body_size() {
    let all: Vec<(String, String)> = [VIEWER, SETTINGS, WELCOME]
        .iter()
        .flat_map(|css| font_declarations(css))
        .collect();
    for sel in [
        "body",
        ":where(input, button)",
        ".chip,.lg",
        ".tlg",
        ".obs .otext",
        ".rv-acts .btn",
        "#statusbar .spill",
        ".contested .confirm",
        ".contested .crow",
        "#viewpop button.tog",
        ".btn",
        ".field label",
        ".drawer-toggle",
    ] {
        let sizes: Vec<&String> = all.iter().filter(|(s, _)| s == sel).map(|(_, d)| d).collect();
        assert!(!sizes.is_empty(), "no font rule for {sel}");
        assert!(
            sizes.iter().any(|d| d.contains("var(--text-body)")),
            "{sel} is not body size: {sizes:?}"
        );
    }
}

/// U21: the map draws its labels on a canvas, which rem does not reach, so every label size there is
/// scaled by the root's size, read again when it changes.
#[test]
fn the_maps_labels_scale_with_the_root() {
    assert!(SCRIPT.contains("let TEXT_SCALE"), "the map keeps no text scale");
    assert!(
        SCRIPT.contains("getComputedStyle(document.documentElement).fontSize"),
        "the text scale is not read from the root"
    );
    assert!(SCRIPT.contains("addEventListener(\"rootsize\""), "a new root size is not heard");
    for line in SCRIPT.lines().filter(|l| l.contains("ctx.font =")) {
        assert!(line.contains("TEXT_SCALE"), "a label size ignores the root: {}", line.trim());
    }
}

/// U21: no text is cut short except where focus or a press shows it whole (Section 10). The Log's
/// clamp opens on the row's focus, the active search option shows its name and snippet whole, a
/// contested value and a candidate wrap, and the status line gives up its counts.
#[test]
fn cut_text_can_be_read_whole() {
    let flat: String = VIEWER.split_whitespace().collect();
    for sel in [".contested .crow .cv", ".cand .cv"] {
        let rule = rules(VIEWER)
            .into_iter()
            .find(|(s, _)| s == sel)
            .unwrap_or_else(|| panic!("no rule {sel}"));
        assert!(!rule.1.contains("text-overflow"), "{sel} is still cut with an ellipsis");
    }
    assert!(
        flat.contains(".obs.ohead:focus.otext,.obs.ohead:focus-visible.otext{-webkit-line-clamp:unset;line-clamp:unset;display:block;}"),
        "the Log's clamp does not open on focus"
    );
    assert!(
        flat.contains(".sp-row.on.sp-name,.sp-row.on.sp-snip{white-space:normal;-webkit-line-clamp:unset;line-clamp:unset;display:block;}"),
        "the active search option is still cut"
    );
    assert!(
        flat.contains("#statusbar#stats.tight.counts{display:none;}"),
        "the status line cuts its counts"
    );
    assert!(
        SCRIPT.contains("classList.toggle(\"tight\""),
        "nothing tells the status line it is tight"
    );
}
