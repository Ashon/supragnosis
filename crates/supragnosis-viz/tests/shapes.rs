//! docs/design-system.md Sections 3, 4 and 10, U19 and U22: corners from six tokens, the act a page
//! or a decision is for drawn large as a capsule, and targets a pointer presses at 28 pixels, 44 under
//! a finger. Before the third pass the viewer drew ten radii, the app's pages six and the site eleven,
//! each a number written where it was used.

const VIEWER: &str = include_str!("../assets/viewer.css");
const SETTINGS: &str = include_str!("../../../app/assets/settings.css");
const WELCOME: &str = include_str!("../../../app/assets/welcome.css");
const SHELL: &str = include_str!("../../../app/assets/shell.css");
const LANDING: &str = include_str!("../../../site/style.css");

const RADII: &[(&str, &str)] = &[
    ("--r-window", "16px"),
    ("--r-surface", "12px"),
    ("--r-control", "8px"),
    ("--r-inset", "4px"),
    ("--r-mark", "2px"),
    ("--r-capsule", "999px"),
];

fn uncommented(css: &str) -> String {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    clean.push_str(rest);
    clean
}

/// The declarations of the first rule whose selector is exactly `selector`, whitespace dropped.
fn rule(css: &str, selector: &str) -> String {
    let flat = uncommented(css);
    let at = flat
        .find(&format!("\n{selector} {{"))
        .or_else(|| flat.find(&format!(" {selector} {{")))
        .unwrap_or_else(|| panic!("no rule {selector}"));
    let body = &flat[at..];
    let body = &body[body.find('{').unwrap() + 1..];
    body[..body.find('}').unwrap()].split_whitespace().collect()
}

/// U22: every corner the tools' and the landing's stylesheets draw is one of the six radius tokens,
/// none, or a dot's circle, and each stylesheet declares the tokens with Section 3's values.
#[test]
fn every_corner_is_a_radius_token() {
    let mut stray = Vec::new();
    for (file, css) in [
        ("viewer.css", VIEWER),
        ("settings.css", SETTINGS),
        ("welcome.css", WELCOME),
        ("shell.css", SHELL),
        ("site/style.css", LANDING),
    ] {
        let flat: String = uncommented(css).split_whitespace().collect();
        if file != "welcome.css" && file != "shell.css" {
            for (name, value) in RADII {
                assert!(
                    flat.contains(&format!("{name}:{value}")),
                    "{file} does not declare {name}"
                );
            }
        }
        let spaced = uncommented(css).split_whitespace().collect::<Vec<_>>().join(" ");
        for decl in spaced.split([';', '{', '}']) {
            let Some((prop, value)) = decl.split_once(':') else {
                continue;
            };
            let prop = prop.trim();
            if !prop.ends_with("radius") || prop.starts_with("--") {
                continue;
            }
            let ok = value.split_whitespace().all(|p| {
                p == "0" || p == "50%" || RADII.iter().any(|(n, _)| p == format!("var({n})"))
            });
            if !ok {
                stray.push(format!("{file}: {prop}:{value}"));
            }
        }
    }
    assert!(stray.is_empty(), "a corner off the radius tokens:\n  {}", stray.join("\n  "));
}

/// Section 4: the act a page or a decision is for draws large, 28 pixels, as a capsule, and the
/// controls of its row share that size; a pill is a capsule at any size.
#[test]
fn large_acts_are_capsules_and_their_rows_share_a_size() {
    for (css, selector) in [
        (VIEWER, ".rv-acts .btn"),
        (WELCOME, ".wactions .btn"),
        (VIEWER, "#statusbar .spill"),
        (SETTINGS, ".pill"),
    ] {
        let r = rule(css, selector);
        assert!(r.contains("border-radius:var(--r-capsule)"), "{selector} is not a capsule: {r}");
    }
    for (css, selector) in [(VIEWER, ".rv-acts .btn"), (WELCOME, ".wactions .btn")] {
        assert!(rule(css, selector).contains("min-height:28px"), "{selector} is not drawn large");
    }
}

/// U19 and Section 10: a target a pointer presses is 28 pixels on the tools - stacked rows and
/// segments reach it by their own height, wrapped chips by their spacing, which browser scenarios
/// measure - and 44 under a coarse pointer, where the status bar grows to hold them.
#[test]
fn targets_reach_28_and_44_under_a_finger() {
    for selector in ["#viewpop button.tog", "#viewpop .seg button", ".chip,.lg", ".rv-segb"] {
        assert!(
            rule(VIEWER, selector).contains("min-height:28px"),
            "{selector} is under 28 pixels"
        );
    }
    for (file, css) in [("viewer.css", VIEWER), ("settings.css", SETTINGS)] {
        let flat: String = uncommented(css).split_whitespace().collect();
        let at = flat
            .find("@media(pointer:coarse){")
            .unwrap_or_else(|| panic!("{file} does not answer a coarse pointer"));
        let block = &flat[at..];
        assert!(
            block.contains("min-height:44px") && block.contains("min-width:44px"),
            "{file}: a finger's targets are under 44 pixels"
        );
        if file == "viewer.css" {
            assert!(
                block.contains("--bar:2.75rem"),
                "the status bar cannot hold a finger's targets"
            );
        }
    }
}
