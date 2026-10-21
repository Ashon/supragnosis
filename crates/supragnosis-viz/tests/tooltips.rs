//! docs/design-system.md Section 4 and U25: a tooltip names a control and its key, and what a control
//! does is said on the screen. Before the third pass the viewer wrote 58 tooltips and the app's pages
//! 21, and many held the only explanation of what a control does, which a keyboard and a finger never
//! see.

const HTML: &str = include_str!("../assets/viewer.html");
const SCRIPT: &str = include_str!("../assets/viewer.js");
const SETTINGS_HTML: &str = include_str!("../../../app/assets/settings.html");
const WELCOME_HTML: &str = include_str!("../../../app/assets/welcome.html");
const SETTINGS_JS: &str = include_str!("../../../app/assets/settings.js");

/// The words of a tooltip before its key, an interpolation counting as one word.
fn words(tip: &str) -> usize {
    let tip = match tip.rfind(" (") {
        Some(i) if tip.ends_with(')') => &tip[..i],
        _ => tip,
    };
    tip.split_whitespace().count()
}

fn attrs<'a>(src: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("{name}=\"");
    src.match_indices(&needle)
        .map(|(i, _)| {
            let rest = &src[i + needle.len()..];
            &rest[..rest.find('"').expect("the attribute closes")]
        })
        .collect()
}

/// U25: every tooltip of the viewer's and the app pages' markup is a name of at most five words before
/// its key, and every tooltip the viewer's script writes goes through `tipText`, whose names are as short.
#[test]
fn a_tooltip_names_and_explains_nothing() {
    let mut long = Vec::new();
    for (file, src) in [
        ("viewer.html", HTML),
        ("settings.html", SETTINGS_HTML),
        ("welcome.html", WELCOME_HTML),
    ] {
        for t in attrs(src, "title") {
            if words(t) > 5 {
                long.push(format!("{file}: {t}"));
            }
        }
    }
    let in_comment =
        |i: usize| SCRIPT[..i].rsplit('\n').next().unwrap_or("").trim_start().starts_with("//");
    for (i, _) in SCRIPT.match_indices("title=\"") {
        if in_comment(i) {
            continue;
        }
        if !SCRIPT[i..].starts_with("title=\"${tipText(") {
            long.push(format!(
                "viewer.js writes a tooltip around tip: {}",
                &SCRIPT[i..i + 60.min(SCRIPT.len() - i)]
            ));
        }
    }
    for (i, _) in SCRIPT.match_indices(".title = ") {
        if !SCRIPT[i..].starts_with(".title = tipText(") {
            long.push(format!("viewer.js sets a tooltip around tip: {}", &SCRIPT[i..i + 50]));
        }
    }
    for (i, _) in SCRIPT.match_indices("tipText(\"") {
        let rest = &SCRIPT[i + 9..];
        let lit = &rest[..rest.find('"').expect("the literal closes")];
        if lit.split_whitespace().count() > 5 {
            long.push(format!("viewer.js: tipText(\"{lit}\")"));
        }
    }
    assert!(
        !SETTINGS_JS.contains("title: \"Stop sharing \" + w + \" with this peer\""),
        "the Settings page explains in a tooltip"
    );
    assert!(long.is_empty(), "a tooltip explains:\n  {}", long.join("\n  "));
}

/// Section 4: what a control does is said on the screen. Each of View's switches and choices carries a
/// line under its name, read with it, and the search says what it searched in words, not in a tooltip.
#[test]
fn view_and_search_say_what_they_do_on_the_screen() {
    let at = HTML.find("<div id=\"viewpop\"").expect("View");
    let pop = &HTML[at..at + HTML[at..].find("<div id=\"hud\"").expect("the HUD follows")];
    // Ten since Follow left View for the camera toolbar (design-system.md Section 29.4).
    let switches = pop.matches("class=\"tog").count();
    assert!(switches >= 10, "View lost its switches ({switches})");
    let described = pop.matches("aria-describedby=\"").count();
    assert!(described > switches, "a View option has no line under its name");
    for id in attrs(pop, "aria-describedby") {
        assert!(pop.contains(&format!("id=\"{id}\"")), "nothing says what {id} names");
    }
    for id in ["regionsDesc", "boundaryDesc"] {
        assert!(
            pop.contains(&format!("id=\"{id}\"")),
            "a View choice does not say what it draws"
        );
    }
    assert!(
        SCRIPT.contains("class=\"sp-modewhy\">${modeWhy}"),
        "the search does not say what it searched"
    );
}
