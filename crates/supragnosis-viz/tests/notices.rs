//! docs/design-system.md Section 28.4, U42: a notice has one anatomy on every surface - its kind's
//! icon, a title, at most a detail and one action, a 28-pixel dismiss showing x - a success leaves
//! 4.5 seconds after it shows, counted only while it is not being read, every other notice stays
//! until it is dismissed or used, four at most, and it moves in the durations of Section 3. The
//! viewer and the app's pages write it twice, under two policies with no shared script; this reads
//! both as one.

const VIEWER_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
const VIEWER_CSS: &str = include_str!("../assets/viewer.css");
const KIT: &str = include_str!("../../../app/assets/kit.js");
const SETTINGS_CSS: &str = include_str!("../../../app/assets/settings.css");
const SETTINGS_HTML: &str = include_str!("../../../app/assets/settings.html");
const WELCOME_HTML: &str = include_str!("../../../app/assets/welcome.html");
const VIEWER_HTML: &str = include_str!("../assets/viewer.html");

/// A top-level function's source, up to the brace that closes it at column zero.
fn function<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &src[at..at + src[at..].find("\n}\n").expect("it closes")]
}

/// The body of the first rule whose selector is exactly `selector`.
fn rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let at = css
        .match_indices(selector)
        .map(|(i, _)| i)
        .find(|&i| css[i + selector.len()..].trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no rule {selector}"));
    let open = at + css[at..].find('{').unwrap();
    &css[open + 1..open + css[open..].find('}').expect("the rule closes")]
}

/// The file each kind's icon class draws, read from the stylesheet that declares the class.
fn icon_file(css: &str, class: &str) -> String {
    let body = rule(css, &format!(".i-{class}"));
    let at = body.find("url(").expect("a url");
    let url = &body[at + 4..at + body[at..].find(')').unwrap()];
    let url = url.trim_matches(['"', '\'']);
    url.rsplit('/').next().unwrap().trim_end_matches(".svg").to_string()
}

/// The `{ ok: "...", bad: "...", info: "..." }` a script maps its kinds through.
fn kinds(src: &str) -> Vec<(String, String)> {
    let at = src.find("const NOTICE_ICON = {").expect("a NOTICE_ICON map");
    let body = &src[at + 21..at + src[at..].find('}').unwrap()];
    body.split(',')
        .map(|kv| {
            let (k, v) = kv.split_once(':').expect("kind: icon");
            (k.trim().to_string(), v.trim().trim_matches('"').to_string())
        })
        .collect()
}

/// The anatomy, on both surfaces: the kinds are ok, bad and info and show circle-check,
/// circle-alert and info in their inks; the icon leads, the words follow; the dismiss is a button
/// named Dismiss showing x, 28 pixels square; the surface has no edge (U9), and an error tints the
/// whole of it toward --bad - 6%, the most that keeps --muted at 4.5:1 on the light ground.
#[test]
fn a_notice_has_one_anatomy() {
    let want = [("ok", "circle-check"), ("bad", "circle-alert"), ("info", "info")];
    for (name, src, css) in [("viewer.js", VIEWER_JS, VIEWER_CSS), ("kit.js", KIT, SETTINGS_CSS)] {
        let map = kinds(src);
        for (kind, file) in want {
            let class = &map
                .iter()
                .find(|(k, _)| k == kind)
                .unwrap_or_else(|| panic!("{name} has no {kind} notice"))
                .1;
            let drawn = if name == "viewer.js" { class.clone() } else { icon_file(css, class) };
            assert_eq!(drawn, file, "\n{name}'s {kind} notice shows {drawn}, not {file}");
        }
        let t = function(src, "toast");
        assert!(
            t.contains("icon(NOTICE_ICON[kind]"),
            "\n{name}'s notice leads with its kind's icon"
        );
        assert!(
            t.contains("\"aria-label\", \"Dismiss\"") || t.contains("\"aria-label\": \"Dismiss\""),
            "\n{name}'s dismiss is named Dismiss"
        );
        assert!(t.contains("icon(\"x\")"), "\n{name}'s dismiss shows the set's x");
        assert!(t.contains("children.length > 4"), "\n{name} keeps four at most");
        assert!(t.contains(".prepend(el)"), "\n{name} puts the newest first");

        let toast = rule(css, ".toast").replace("border-radius", "");
        assert!(
            !toast.contains("border") && !toast.contains("inset"),
            "\n{name}'s notice draws an edge (U9)"
        );
        let bad = rule(css, ".toast.bad");
        assert!(
            bad.contains("color-mix(in srgb, var(--bad) 6%, var(--panel-2))")
                && !bad.contains("border"),
            "\n{name}'s error notice does not tint its whole surface"
        );
        for (kind, ink) in [("ok", "--ok-ink"), ("bad", "--bad-ink"), ("info", "--ink2")] {
            assert!(
                rule(css, &format!(".toast.{kind} > .i")).contains(&format!("var({ink})")),
                "\n{name}'s {kind} icon is not in {ink}"
            );
        }
    }
    let x = rule(VIEWER_CSS, ".toast button.tx");
    assert!(
        x.contains("width:28px") && x.contains("height:28px"),
        "the viewer's dismiss is not 28 pixels"
    );
    let x = rule(SETTINGS_CSS, ".toast .btn");
    assert!(
        x.contains("height: 28px") && x.contains("width: 28px"),
        "the app's dismiss is not 28 pixels"
    );
    // The follow notice is news, not the outcome of the person's act.
    assert!(VIEWER_JS.contains("toast(\"info\", \"New in \" + ws"));
    // Still one polite live region on every page that shows notices (Section 9).
    for (page, html) in [
        ("viewer.html", VIEWER_HTML),
        ("settings.html", SETTINGS_HTML),
        ("welcome.html", WELCOME_HTML),
    ] {
        let at = html.find("id=\"toasts\"").unwrap_or_else(|| panic!("{page} has no notices"));
        let tag = &html[html[..at].rfind('<').unwrap()..at + html[at..].find('>').unwrap()];
        assert!(
            tag.contains("role=\"status\"") && tag.contains("aria-live=\"polite\""),
            "\n{page}'s notices are not a polite status"
        );
    }
}

/// The clock: only a success without an action leaves by itself, 4.5 seconds after it shows - and on
/// the Review page a success whose act is also on its Decided item (design-system.md Section 29.3);
/// the pointer over it or focus inside it stops the clock, which starts whole again once both have
/// left.
#[test]
fn a_success_waits_while_it_is_read() {
    for (name, src, gate) in [
        (
            "viewer.js",
            VIEWER_JS,
            "if (kind === \"ok\" && (!action || (action.decided && reviewOpen()))) leaveAfter(el, 4500);",
        ),
        ("kit.js", KIT, "if (kind === \"ok\") leaveAfter(el, 4500);"),
    ] {
        let t = function(src, "toast");
        assert!(t.contains(gate), "\n{name}: only a success without an action leaves by itself");
        assert_eq!(
            t.matches("setTimeout").count(),
            0,
            "\n{name}'s notice keeps a clock outside leaveAfter"
        );
        let l = function(src, "leaveAfter");
        for ev in ["pointerenter", "pointerleave", "focusin", "focusout"] {
            assert!(l.contains(&format!("\"{ev}\"")), "\n{name}'s clock does not hear {ev}");
        }
        assert!(
            l.contains("clearTimeout(timer); if (!pointer && !focus) timer = setTimeout(() => dismiss(el), ms);"),
            "\n{name}'s clock does not stop while the notice is read and start whole after"
        );
        assert!(
            l.contains("focus = el.contains(e.relatedTarget)"),
            "\n{name}: focus moving inside the notice is still reading it"
        );
    }
}

/// The motion: in over 200 ms on the ease-out curve, from 8 pixels above in the viewer and 8 to the
/// right on the app's pages; out by fading over 120 ms; at once under reduced motion.
#[test]
fn a_notice_moves_in_the_three_durations() {
    for (name, css, from) in [
        ("viewer.css", VIEWER_CSS, "translateY(-8px)"),
        ("settings.css", SETTINGS_CSS, "translateX(8px)"),
    ] {
        let toast = rule(css, ".toast");
        assert!(
            toast.contains("animation:noticein 0.2s ease-out")
                || toast.contains("animation: noticein 0.2s ease-out"),
            "\n{name}: a notice does not enter over 200 ms"
        );
        let frames = &css[css
            .find("@keyframes noticein")
            .unwrap_or_else(|| panic!("{name} has no noticein"))..];
        assert!(
            frames[..frames.find('}').unwrap()].contains(from),
            "\n{name}: a notice does not enter from {from}"
        );
        let out = rule(css, ".toast.out");
        assert!(
            out.contains("opacity") && out.contains("0.12s ease-out"),
            "\n{name}: a notice does not leave by fading over 120 ms"
        );
        assert!(
            css.contains("prefers-reduced-motion: reduce"),
            "\n{name} does not hold still under reduced motion"
        );
    }
    for (name, src) in [("viewer.js", VIEWER_JS), ("kit.js", KIT)] {
        let d = function(src, "dismiss");
        assert!(
            d.contains("if (REDUCED_MOTION) { el.remove(); return; }")
                && d.contains("setTimeout(() => el.remove(), 120)"),
            "\n{name}: a notice does not fade over 120 ms, or does under reduced motion"
        );
    }
    assert!(
        !SETTINGS_CSS.contains("slidein"),
        "the app's notice still moves on its old 180 ms"
    );
}
