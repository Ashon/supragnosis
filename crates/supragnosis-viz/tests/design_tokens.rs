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
const SURFACES: &[&str] = &["--surface", "--panel", "--panel-2", "--panel-3", "--select"];

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

/// Custom properties a stylesheet reads that its page's markup or script sets, each with its setter.
/// None today: the three pages set no custom property of their own.
const SET_ELSEWHERE: &[(&str, &str, &str)] = &[];

/// Every `var(--name)` a stylesheet reads names a property the same file declares (U7). U7's first
/// test compares what the files declare and never saw what they only read: the landing read
/// `--panel-3` and `--gold-glass` and the settings page `--select` from no `:root`, so a hover, a
/// badge's ground and the selected section's surface were drawn with nothing.
#[test]
fn every_token_a_stylesheet_reads_is_declared() {
    let mut missing = Vec::new();
    for (file, css) in
        [("viewer.css", VIEWER), ("settings.css", SETTINGS), ("site/style.css", LANDING)]
    {
        let name = |rest: &str| -> String {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect()
        };
        let declared: Vec<String> = css
            .match_indices("--")
            .filter(|(i, _)| !css[..*i].ends_with("var("))
            .map(|(i, _)| name(&css[i..]))
            .filter(|n| css.contains(&format!("{n}:")))
            .collect();
        let mut read = 0;
        for (i, _) in css.match_indices("var(--") {
            read += 1;
            let n = name(&css[i + 4..]);
            let elsewhere = SET_ELSEWHERE.iter().any(|(f, p, _)| *f == file && *p == n);
            if !declared.contains(&n) && !elsewhere {
                missing.push(format!("{file} reads {n}, which it does not declare"));
            }
        }
        assert!(read >= 10, "{file}: the var() reads were found ({read})");
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "tokens read but never declared:\n  {}",
        missing.join("\n  ")
    );
}

/// The leaf rules of a stylesheet as (selector, declarations), comments removed and whitespace in the
/// selector collapsed. A rule inside `@media` is returned with its own selector.
fn rules(css: &str) -> Vec<(String, String)> {
    let mut clean = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
    }
    clean.push_str(rest);
    let mut out = Vec::new();
    let mut start = 0;
    let mut open: Option<(String, usize)> = None;
    for (i, ch) in clean.char_indices() {
        match ch {
            '{' => {
                let prelude = clean[start..i].split_whitespace().collect::<Vec<_>>().join(" ");
                open = Some((prelude, i + 1));
                start = i + 1;
            }
            '}' => {
                if let Some((sel, body)) = open.take() {
                    out.push((sel, clean[body..i].to_string()));
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// What a declaration draws as a line, if anything: a border that draws, a border color that is
/// not transparent, an outline, or a box-shadow shaped as a ring (no offset, no blur, a spread).
fn draws_a_line(name: &str, value: &str) -> bool {
    let v = value.trim().to_lowercase();
    let invisible = v == "none" || v == "0" || v == "0px" || v.contains("transparent");
    match name {
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => !invisible,
        n if n.starts_with("border") && n.ends_with("-color") => !v.contains("transparent"),
        "outline" => !invisible,
        "box-shadow" => v.split(',').any(|shadow| {
            let parts: Vec<&str> = shadow.split_whitespace().filter(|p| *p != "inset").collect();
            let zero = |p: &&str| matches!(*p, "0" | "0px");
            parts.len() >= 4 && parts[..3].iter().all(zero) && !zero(&parts[3])
        }),
        _ => false,
    }
}

/// The lines each stylesheet keeps, and the fact each one carries (design-system.md Section 4). An
/// outline under `:focus-visible` is keyboard focus wherever it is, so the lists leave it out.
const VIEWER_LINES: &[(&str, &str)] = &[
    (".contested", "a contested value, in --bad"),
    ("#detail .rchip.past", "a superseded relation, dashed"),
    (".tsw.t-unverified", "the Trust legend's swatch of the unverified ring"),
    (".tsw.t-host_signed", "the Trust legend's swatch of the host-signed ring"),
    (
        ".tsw.t-human_confirmed",
        "the Trust legend's swatch of the human-confirmed ring",
    ),
    ("#loader .spin", "a control's own glyph: the loader's spinner"),
    ("#mapwin", "paints the frame around the map's window: a surface, not a line"),
];
const SETTINGS_LINES: &[(&str, &str)] = &[
    (".radio", "a control's own glyph: the radio's circle"),
    (".radio.on", "a control's own glyph: the chosen radio's circle"),
];
const LANDING_LINES: &[(&str, &str)] = &[];

/// The declarations of a stylesheet that draw a line outside its list, and the listed selectors
/// that drew one.
fn stray_lines<'a>(css: &str, listed: &'a [(&'a str, &'a str)]) -> (Vec<String>, Vec<&'a str>) {
    let mut stray = Vec::new();
    let mut used = Vec::new();
    for (selector, body) in rules(css) {
        for decl in body.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if !draws_a_line(name, value)
                || (name == "outline" && selector.contains(":focus-visible"))
            {
                continue;
            }
            match listed.iter().find(|(s, _)| *s == selector) {
                Some((s, _)) => used.push(*s),
                None => stray.push(format!("{selector} {{ {name}:{} }}", value.trim())),
            }
        }
    }
    (stray, used)
}

/// U9 (design-system.md Section 4): areas are told apart by surface. Each of the three stylesheets
/// draws a line only for keyboard focus or a selector listed with the fact its line carries, and
/// the viewer's script sets a border only on the boundaries' legend swatches, which show the dashes
/// the map draws. Before step 7 viewer.css drew 85 such lines; every dock, chip, row, pill and camera
/// button had a border of its own.
#[test]
fn no_border_draws_an_area() {
    for (file, css, listed) in [
        ("viewer.css", VIEWER, VIEWER_LINES),
        ("settings.css", SETTINGS, SETTINGS_LINES),
        ("site/style.css", LANDING, LANDING_LINES),
    ] {
        let (stray, used) = stray_lines(css, listed);
        assert!(
            stray.is_empty(),
            "{file} draws lines no fact asks for (U9):\n  {}",
            stray.join("\n  ")
        );
        let unused: Vec<_> = listed.iter().filter(|(s, _)| !used.contains(s)).collect();
        assert!(unused.is_empty(), "{file}: listed lines it no longer draws: {unused:?}");
    }

    let js = include_str!("../assets/viewer.js");
    let start = js
        .find("\nfunction renderBoundaryChips(")
        .expect("viewer.js no longer defines renderBoundaryChips");
    let swatches = &js[start..];
    let swatches = &swatches[..swatches.find("\n}\n").expect("renderBoundaryChips closes")];
    // A border the script sets, not a corner it rounds.
    let borders = |src: &str| {
        src.match_indices("style.border")
            .filter(|(i, m)| !src[i + m.len()..].starts_with("Radius"))
            .count()
    };
    assert_eq!(
        borders(js),
        borders(swatches),
        "the script sets a border outside the boundaries' legend swatches (U9)"
    );
    assert!(!js.contains("style=\"border"), "a template sets a border inline (U9)");
}

const VIEWER_HTML: &str = include_str!("../assets/viewer.html");
const VIEWER_JS: &str = include_str!("../assets/viewer.js");
const SETTINGS_HTML: &str = include_str!("../../../app/assets/settings.html");
const LANDING_HTML: &str = include_str!("../../../site/index.html");
const PRINCIPLES_HTML: &str = include_str!("../../../site/principles/index.html");

/// Whether a surface ships the file at a path it names.
type Ships<'a> = &'a dyn Fn(&str) -> bool;

/// The `url(...)` targets of a stylesheet, quotes dropped.
fn urls(css: &str) -> Vec<String> {
    css.match_indices("url(")
        .filter_map(|(i, _)| {
            let rest = &css[i + 4..];
            let end = rest.find(')')?;
            Some(rest[..end].trim().trim_matches(['"', '\'']).to_string())
        })
        .filter(|u| !u.starts_with("data:"))
        .collect()
}

/// The `href` of each stylesheet link in a page.
fn stylesheet_links(html: &str) -> Vec<String> {
    html.match_indices("<link")
        .filter_map(|(i, _)| {
            let tag = &html[i..i + html[i..].find('>')?];
            if !tag.contains("rel=\"stylesheet\"") {
                return None;
            }
            let h = tag.find("href=\"")? + 6;
            Some(tag[h..h + tag[h..].find('"')?].to_string())
        })
        .collect()
}

/// U11 (design-system.md Section 4): every face a surface names first in its reading or identifier
/// stack ships with that surface and is served from its own origin, and no surface fetches a font
/// from another host. Until step 10 the tools shipped none: the "IBM Plex Mono" they named was drawn
/// as SF Mono, because their policies had no font-src and the face was nowhere on disk.
#[test]
fn every_face_ships_with_its_surface() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // The reading and identifier stacks begin with the shipped faces; U7 holds them equal.
    for (file, set) in &surfaces() {
        let sans = value(set, "--sans").unwrap_or_else(|| panic!("{file} names no --sans"));
        let mono = value(set, "--mono").unwrap_or_else(|| panic!("{file} names no --mono"));
        assert!(
            sans.starts_with("\"pretendardvariable\",pretendard,"),
            "{file}: --sans is {sans}"
        );
        assert!(mono.starts_with("\"ibmplexmono\","), "{file}: --mono is {mono}");
    }

    // Each surface declares both faces from files it ships. The viewer's are compiled into the
    // binary and served under /fonts/; the Settings page's and the landing's sit beside the page.
    let viewer_ships = |url: &str| supragnosis_viz::asset(url).is_some();
    let settings_ships = |url: &str| root.join("app/assets").join(url).is_file();
    let landing_ships = |url: &str| root.join("site").join(url).is_file();
    let surfaces_: [(&str, &str, &str, Ships); 3] = [
        ("viewer", VIEWER_HTML, VIEWER, &viewer_ships),
        ("settings page", SETTINGS_HTML, SETTINGS, &settings_ships),
        ("landing", LANDING_HTML, LANDING, &landing_ships),
    ];
    for (name, html, css, ships) in surfaces_ {
        let plex: Vec<String> =
            urls(css).into_iter().filter(|u| u.contains("ibm-plex-mono")).collect();
        assert!(
            plex.len() >= 3,
            "{name}: IBM Plex Mono is not declared at 400, 500 and 600 ({plex:?})"
        );
        let links = stylesheet_links(html);
        let pretendard = links
            .iter()
            .find(|h| h.contains("fonts/pretendard"))
            .unwrap_or_else(|| panic!("{name} links no Pretendard stylesheet: {links:?}"));
        assert!(ships(pretendard), "{name}: {pretendard} is not shipped");
        let sheet = if name == "viewer" {
            String::from_utf8(supragnosis_viz::asset(pretendard).unwrap().1.to_vec()).unwrap()
        } else {
            let base = if name == "landing" { "site" } else { "app/assets" };
            std::fs::read_to_string(root.join(base).join(pretendard)).unwrap()
        };
        let dir = &pretendard[..pretendard.rfind('/').unwrap() + 1];
        let files: Vec<String> = urls(&sheet)
            .iter()
            .map(|u| format!("{dir}{}", u.trim_start_matches("./")))
            .collect();
        assert!(files.len() >= 9, "{name}: the Pretendard sheet declares {} files", files.len());
        for url in plex.iter().chain(files.iter()) {
            assert!(ships(url), "{name}: {url} is named but not shipped");
        }
        for text in [html, css] {
            assert!(
                !text.contains("fonts.googleapis") && !text.contains("fonts.gstatic"),
                "{name} fetches a font from another host"
            );
        }
    }

    // The principles page reads the landing's stylesheet and links its Pretendard sheet, from one
    // directory down, and fetches nothing from a font host (design-system.md Section 11a).
    let page_links = stylesheet_links(PRINCIPLES_HTML);
    for want in ["../fonts/pretendard-1.3.9/pretendard-latin.css", "../style.css"] {
        assert!(
            page_links.iter().any(|h| h == want),
            "the principles page does not link {want}: {page_links:?}"
        );
        assert!(
            root.join("site/principles").join(want).is_file(),
            "the principles page links {want}, which is not shipped"
        );
    }
    assert!(
        !PRINCIPLES_HTML.contains("fonts.googleapis") && !PRINCIPLES_HTML.contains("fonts.gstatic"),
        "the principles page fetches a font from another host"
    );

    // The two policies that would refuse a font allow this origin's.
    assert!(
        supragnosis_viz::CSP.contains("font-src 'self'"),
        "the viewer's policy refuses its own fonts"
    );
    assert!(
        SETTINGS_HTML.contains("font-src 'self'"),
        "the Settings page's policy refuses its own fonts"
    );

    // The map's names are drawn in the reading face and its relation kinds in the identifier face.
    assert!(VIEWER_JS.contains("const NAME_FAMILY = '\"Pretendard Variable\",Pretendard,"));
    assert!(VIEWER_JS.contains("const KIND_FAMILY = \"'IBM Plex Mono',"));
    assert!(
        VIEWER_JS.contains("px \" + NAME_FAMILY;"),
        "the name sprites are not set in the reading face"
    );
    assert!(
        !VIEWER_JS.contains("LABEL_FAMILY"),
        "a label is still set in the old mono stack"
    );
}
