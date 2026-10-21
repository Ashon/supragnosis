//! docs/design-system.md Section 28, U41: every icon the viewer, the app's pages and the landing
//! draw is a Lucide 1.52.0 file, vendored unmodified and drawn as a mask over the text's color; no
//! page draws one from a character, a path written by hand or a pasted copy, and a file both
//! surfaces ship is the same bytes on both. The author asked for one set on 2026-10-21.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const VIEWER_HTML: &str = include_str!("../assets/viewer.html");
const VIEWER_JS: &str = include_str!("../assets/viewer.js");
const VIEWER_CSS: &str = include_str!("../assets/viewer.css");
const SETTINGS_HTML: &str = include_str!("../../../app/assets/settings.html");
const WELCOME_HTML: &str = include_str!("../../../app/assets/welcome.html");
const KIT: &str = include_str!("../../../app/assets/kit.js");
const SETTINGS_JS: &str = include_str!("../../../app/assets/settings.js");
const WELCOME_JS: &str = include_str!("../../../app/assets/welcome.js");
const SHELL_CSS: &str = include_str!("../../../app/assets/shell.css");
const SETTINGS_CSS: &str = include_str!("../../../app/assets/settings.css");
const WELCOME_CSS: &str = include_str!("../../../app/assets/welcome.css");
const STAGE: &str = include_str!("../../../site/stage.js");
const PRINCIPLES_JS: &str = include_str!("../../../site/principles.js");
const SITE_CSS: &str = include_str!("../../../site/style.css");

/// The release both surfaces ship, as Lucide writes it in the first line of every file.
const RELEASE: &str = "1.52.0";
/// Where the viewer serves them; the release is in the path because the path is cached for a year.
const VIEWER_PREFIX: &str = "/icons/lucide-1.52.0/";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `.svg` files of one surface's icon directory, by name.
fn svgs(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("svg"))
        .map(|p| {
            let name = p.file_stem().expect("a name").to_string_lossy().to_string();
            (name, fs::read(&p).expect("read an icon"))
        })
        .collect();
    out.sort();
    out
}

fn viewer_icons() -> PathBuf {
    root().join("crates/supragnosis-viz/assets/icons")
}

fn app_icons() -> PathBuf {
    root().join("app/assets/icons")
}

fn site_icons() -> PathBuf {
    root().join("site/icons")
}

/// Every `.i-<name> { --i: url(<url>) }` a stylesheet declares, as (name, url).
fn declared(css: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (at, _) in css.match_indices(".i-") {
        let rest = &css[at + 3..];
        let name: String =
            rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        let rule = &rest[name.len()..];
        let Some(body) = rule.trim_start().strip_prefix('{') else {
            continue;
        };
        let body = &body[..body.find('}').expect("the rule closes")];
        let Some(u) = body.find("url(") else { continue };
        let url = body[u + 4..body[u..].find(')').map(|e| u + e).expect("url closes")]
            .trim_matches(['"', '\'', ' ']);
        out.push((name, url.to_string()));
    }
    out
}

/// The icon names a page or script names literally: `i i-<name>`, `"i-<name>"` and `icon("<name>")`.
fn named(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let word = |s: &str| -> String {
        s.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
    };
    for pat in ["\"i i-", "\"i-", " i-", "icon(\""] {
        for (at, _) in src.match_indices(pat) {
            let w = word(&src[at + pat.len()..]);
            if w.starts_with(|c: char| c.is_ascii_lowercase()) {
                out.insert(w);
            }
        }
    }
    out
}

/// The files are Lucide's own: each starts with the license line of the release both surfaces ship
/// and is a whole SVG; a name both surfaces ship is the same bytes on both; and both carry Lucide's
/// license, the same text.
#[test]
fn the_icons_are_lucides_files_unmodified() {
    let header = format!("<!-- @license lucide-static v{RELEASE} - ISC -->");
    let viewer = svgs(&viewer_icons());
    let app = svgs(&app_icons());
    let site = svgs(&site_icons());
    assert!(
        !viewer.is_empty() && !app.is_empty() && !site.is_empty(),
        "every surface ships icons"
    );
    for (dir, files) in [("viewer", &viewer), ("app", &app), ("site", &site)] {
        for (name, bytes) in files {
            let text = std::str::from_utf8(bytes).expect("an SVG is text");
            assert_eq!(
                text.lines().next(),
                Some(header.as_str()),
                "\nthe {dir}'s {name}.svg does not start with Lucide {RELEASE}'s license line - copy it \
                 from lucide-static/icons/ at that release, unmodified"
            );
            assert!(
                text.contains(&format!("class=\"lucide lucide-{name}\""))
                    && text.trim_end().ends_with("</svg>"),
                "\nthe {dir}'s {name}.svg is not Lucide's {name}, whole"
            );
        }
    }
    for (one, other, files, others) in [
        ("viewer", "app", &viewer, &app),
        ("viewer", "site", &viewer, &site),
        ("app", "site", &app, &site),
    ] {
        for (name, bytes) in files {
            if let Some((_, theirs)) = others.iter().find(|(n, _)| n == name) {
                assert_eq!(bytes, theirs, "\n{name}.svg differs between the {one} and the {other}");
            }
        }
    }
    let license = |dir: PathBuf| fs::read(dir.join("LICENSE")).expect("a LICENSE beside the icons");
    let a = license(viewer_icons());
    for (name, dir) in [("app", app_icons()), ("site", site_icons())] {
        assert_eq!(
            a,
            license(dir),
            "the {name}'s copy of Lucide's license differs from the viewer's"
        );
    }
    assert!(String::from_utf8_lossy(&a).starts_with("ISC License"));
}

/// Every icon class a page declares names a file it ships - on the viewer, a path the binary serves
/// with the file's bytes, on both tiers through the one lookup - and every icon a page or script
/// names literally is declared.
#[test]
fn every_icon_a_page_names_is_a_file_it_ships() {
    let viewer_files = svgs(&viewer_icons());
    let decl = declared(VIEWER_CSS);
    assert!(!decl.is_empty(), "viewer.css declares its icons");
    for (name, url) in &decl {
        let file = url
            .strip_prefix(VIEWER_PREFIX)
            .and_then(|f| f.strip_suffix(".svg"))
            .unwrap_or_else(|| panic!(".i-{name} points at {url}, outside {VIEWER_PREFIX}"));
        assert_eq!(
            file, name,
            "\n.i-{name} draws {file}.svg: on the viewer a class is its file's name"
        );
        let (kind, bytes) = supragnosis_viz::asset(url)
            .unwrap_or_else(|| panic!("\nthe viewer does not serve {url} (.i-{name})"));
        assert_eq!(kind, "image/svg+xml");
        let (_, want) = viewer_files.iter().find(|(n, _)| n == name).expect("the file exists");
        assert_eq!(bytes, want.as_slice(), "\n{url} is not the bytes of {name}.svg");
    }
    let viewer_names: BTreeSet<_> = decl.iter().map(|(n, _)| n.clone()).collect();
    for src in [VIEWER_HTML, VIEWER_JS] {
        for name in named(src) {
            assert!(
                viewer_names.contains(&name),
                "\nthe viewer names icon {name:?}, which viewer.css does not declare"
            );
        }
    }
    // A mask image is an image: the viewer's policy admits its own.
    assert!(supragnosis_viz::CSP.contains("img-src 'self'"));

    let app_files = svgs(&app_icons());
    let mut app_names = BTreeSet::new();
    for css in [SHELL_CSS, SETTINGS_CSS, WELCOME_CSS] {
        for (name, url) in declared(css) {
            let file = url
                .strip_prefix("icons/")
                .and_then(|f| f.strip_suffix(".svg"))
                .unwrap_or_else(|| panic!(".i-{name} points at {url}, outside icons/"));
            assert!(
                app_files.iter().any(|(n, _)| n == file),
                "\nthe app's .i-{name} draws icons/{file}.svg, which app/assets/icons does not hold"
            );
            app_names.insert(name);
        }
    }
    for src in [SETTINGS_HTML, WELCOME_HTML, KIT, SETTINGS_JS, WELCOME_JS] {
        for name in named(src) {
            assert!(
                app_names.contains(&name),
                "\nthe app names icon {name:?}, which no stylesheet of its declares"
            );
        }
    }

    // The landing's stylesheet paints its one mark through a file beside it, resolved from the
    // stylesheet as CSS resolves a url().
    let site_files = svgs(&site_icons());
    let mut masks = 0;
    for (at, _) in SITE_CSS.match_indices("url(\"icons/") {
        let rest = &SITE_CSS[at + 11..];
        let file = &rest[..rest.find(".svg").expect("an .svg")];
        assert!(
            site_files.iter().any(|(n, _)| n == file),
            "\nstyle.css paints icons/{file}.svg, which site/icons does not hold"
        );
        masks += 1;
    }
    assert!(masks > 0, "style.css paints its marks from site/icons");
}

/// No page draws an icon from text. The only `<svg>` a page holds is the brand mark; no script
/// writes one; no stylesheet generates a character; a script's one- or two-character text is not a
/// mark; and the former marks - the letter x, "ok", a relation's "->" and "<-", a typed "!" - are gone.
#[test]
fn no_page_draws_an_icon_from_text() {
    for (page, html) in [
        ("viewer.html", VIEWER_HTML),
        ("settings.html", SETTINGS_HTML),
        ("welcome.html", WELCOME_HTML),
    ] {
        let svg = html.matches("<svg").count();
        assert_eq!(svg, 1, "\n{page} holds {svg} <svg> elements; the brand mark is the only one");
        let at = html.find("<svg").unwrap();
        assert!(
            html[..at].ends_with("<span class=\"mark\">"),
            "\n{page}'s <svg> is not the brand mark"
        );
    }
    for (name, src) in [
        ("viewer.js", VIEWER_JS),
        ("kit.js", KIT),
        ("settings.js", SETTINGS_JS),
        ("welcome.js", WELCOME_JS),
    ] {
        assert!(
            !src.contains("<svg"),
            "\n{name} writes an <svg>: draw an icon from its file instead"
        );
        for (at, _) in src.match_indices("textContent = \"") {
            let lit = &src[at + 15..];
            let lit = &lit[..lit.find('"').expect("the literal closes")];
            assert!(
                lit.is_empty()
                    || lit.chars().count() > 2
                    || lit.chars().all(|c| c.is_ascii_digit()),
                "\n{name} writes {lit:?} as a control's text: a mark comes from the icon set"
            );
        }
    }
    for (name, css) in [
        ("viewer.css", VIEWER_CSS),
        ("shell.css", SHELL_CSS),
        ("settings.css", SETTINGS_CSS),
        ("welcome.css", WELCOME_CSS),
        ("style.css", SITE_CSS),
    ] {
        for (at, _) in css.match_indices("content:") {
            // `justify-content:` and `align-content:` lay out; only the property itself writes text.
            if css[..at].ends_with(|c: char| c == '-' || c.is_ascii_alphanumeric()) {
                continue;
            }
            let v = css[at + 8..].trim_start();
            // Text that is text: the shell's prompt before a command Setup ran, the landing's
            // wordmark, which is the brand's, the slash between the landing's facts, and a step's
            // number, which is a count.
            if [
                ("welcome.css", "\"$ \""),
                ("style.css", "\"* \""),
                ("style.css", "\"/\""),
                ("style.css", "counter("),
            ]
            .iter()
            .any(|(n, lit)| *n == name && v.starts_with(lit))
            {
                continue;
            }
            assert!(
                v.starts_with("\"\"") || v.starts_with("''") || v.starts_with("none"),
                "\n{name} generates text with `content:{}`: a mark comes from the icon set",
                &v[..v.find(';').unwrap_or(v.len()).min(12)]
            );
        }
    }
    for gone in [
        "\"->\" : \"<-\"",
        "\" -> \"",
        "textContent = \"x\"",
        "textContent = \"ok\"",
        "? \"confirm\" : \"x\"",
        "fillText(\"!\"",
    ] {
        assert!(!VIEWER_JS.contains(gone), "\nviewer.js still draws a mark from text: {gone}");
    }
    for (name, src) in [("stage.js", STAGE), ("principles.js", PRINCIPLES_JS)] {
        assert!(!src.contains("\"!\""), "\n{name} still types the contested badge's \"!\"");
    }
}

/// The contested badge's mark is circle-alert's - its bar and dot, read from the file - on the map
/// and on the landing's two pictures, drawn as a path at the set's stroke, never under 1.5 pixels.
#[test]
fn the_contested_badge_draws_the_sets_mark() {
    let file =
        fs::read_to_string(viewer_icons().join("circle-alert.svg")).expect("circle-alert.svg");
    let num = |line: &str, attr: &str| -> f64 {
        let at = line.find(&format!("{attr}=\"")).unwrap_or_else(|| panic!("{attr} in {line}"));
        let v = &line[at + attr.len() + 2..];
        v[..v.find('"').unwrap()].parse().unwrap()
    };
    let fmt = |v: f64| -> String {
        let s = format!("{}", (v * 1000.0).round() / 1000.0);
        s.strip_prefix("0.").map_or(s.clone(), |r| format!(".{r}"))
    };
    let mut path = String::new();
    for line in file.lines().filter(|l| l.trim_start().starts_with("<line")) {
        let (x1, x2, y1, y2) = (num(line, "x1"), num(line, "x2"), num(line, "y1"), num(line, "y2"));
        path.push_str(&format!("M{} {}", fmt(x1), fmt(y1)));
        if x1 == x2 {
            path.push_str(&format!("v{}", fmt(y2 - y1)));
        } else {
            assert_eq!(y1, y2, "circle-alert's marks are straight");
            path.push_str(&format!("h{}", fmt(x2 - x1)));
        }
    }
    assert_eq!(path, "M12 8v4M12 16h.01", "circle-alert.svg changed its marks");
    let want = format!("new Path2D(\"{path}\")");
    for (name, src) in
        [("viewer.js", VIEWER_JS), ("stage.js", STAGE), ("principles.js", PRINCIPLES_JS)]
    {
        assert!(src.contains(&want), "\n{name} does not draw circle-alert's mark ({want})");
        let at = src
            .find("function alertMark(")
            .unwrap_or_else(|| panic!("{name} has no alertMark"));
        let body = &src[at..at + src[at..].find("\n}").expect("alertMark closes")];
        assert!(
            body.contains("const k = r / 10;")
                && body.contains("Math.max(2, 1.5 /")
                && body.contains("lineCap = \"round\""),
            "\n{name}'s alertMark is not the set's stroke on the badge's disc"
        );
    }
    assert!(VIEWER_JS.contains("alertMark(ctx, bx, by, br, SURFACE, cam.s);"));
}
