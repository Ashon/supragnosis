//! docs/design-system.md U14: the principles page (site/principles/index.html, site/principles.js)
//! draws the viewer's marks with the viewer's meanings and its own marks as its own, plays each
//! figure at most once on its own, can stop it, rests, and reads complete without a script. Like the
//! stage's tripwire (landing.rs), it reads the page's sources beside the viewer's script and the
//! landing's stylesheet; what the page states is held to the registry and the engine elsewhere (U12,
//! U13: principle_coverage.rs and principles_page.rs in the engine's tests).

const SCRIPT: &str = include_str!("../../../site/principles.js");
const PAGE: &str = include_str!("../../../site/principles/index.html");
const LANDING: &str = include_str!("../../../site/index.html");
const CSS: &str = include_str!("../../../site/style.css");
const VIEWER: &str = include_str!("../assets/viewer.js");

/// A top-level function or const of `src`, from its first line to the brace or semicolon that closes
/// it, brackets counted, each line with its leading whitespace dropped (landing.rs reads the stage
/// the same way).
fn block(src: &str, start: &str) -> String {
    let at = src.find(start).unwrap_or_else(|| panic!("no `{start}`"));
    let mut out = Vec::new();
    let mut depth = 0i32;
    for line in src[at..].lines() {
        let l = line.trim_start();
        out.push(l.to_string());
        for c in l.chars() {
            match c {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                _ => {}
            }
        }
        // A function ends at the brace that closes it, a const at its semicolon outside any bracket.
        if depth == 0 && (l.ends_with('}') || l.ends_with(';')) {
            break;
        }
    }
    out.join("\n")
}

/// A top-level function's whole source, up to the brace that closes it at the start of a line.
fn body(src: &str, start: &str) -> String {
    let at = src.find(start).unwrap_or_else(|| panic!("no `{start}`"));
    src[at..at + src[at..].find("\n}\n").expect("it closes")].to_string()
}

/// How many `"#rrggbb"` colors a source writes.
fn hex_colors(src: &str) -> usize {
    src.match_indices("\"#")
        .filter(|(i, _)| {
            let h = &src.as_bytes()[i + 2..(i + 9).min(src.len())];
            h.len() == 7 && h[..6].iter().all(u8::is_ascii_hexdigit) && h[6] == b'"'
        })
        .count()
}

fn token(name: &str) -> String {
    let root = &CSS[CSS.find(":root {").expect("a :root block")..];
    let root = &root[..root.find('}').expect("the :root block closes")];
    let decl = root
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("site/style.css does not declare {name}"));
    decl[name.len() + 1..].trim().trim_end_matches(';').trim().to_lowercase()
}

#[test]
fn the_principles_page_draws_the_viewers_marks() {
    // A type's color is the viewer's function of its name, copied character for character.
    for start in [
        "const TYPE_BAND",
        "const BAND_HUES",
        "function nameSlot(",
        "function stableColors(",
    ] {
        assert_eq!(
            block(SCRIPT, start),
            block(VIEWER, start),
            "principles.js's `{start}` is not the viewer's"
        );
    }
    assert!(
        !SCRIPT.contains("hsl(") && !SCRIPT.contains("oklch("),
        "the page computes a color the viewer's band does not hold"
    );

    // Every palette color is a landing token's value, named beside it, and no other color is written.
    let pal = &SCRIPT[SCRIPT.find("const C = {").expect("the page's palette")..];
    let pal = &pal[..pal.find("};").expect("the palette closes")];
    let mut seen = 0;
    for line in pal.lines().skip(1) {
        let line = line.trim();
        let Some(hex) = line.split('"').nth(1) else {
            continue;
        };
        let tok = line
            .split("// ")
            .nth(1)
            .and_then(|c| c.split([',', ' ']).next())
            .unwrap_or_else(|| panic!("palette entry names no token: {line}"));
        assert_eq!(hex.to_lowercase(), token(tok), "principles.js {line}");
        seen += 1;
    }
    assert!(seen >= 10, "the palette was read ({seen} colors)");
    assert_eq!(
        hex_colors(SCRIPT) - hex_colors(&block(SCRIPT, "const TYPE_BAND")),
        seen + 1,
        "principles.js writes a color outside its palette, the band and the edge line"
    );
    assert!(
        !SCRIPT.contains("rgba(2") && !SCRIPT.contains("rgb(2"),
        "principles.js writes a color by its numbers"
    );

    // A relation is the viewer's neutral line at its alpha.
    let at = VIEWER.find("EDGE_NEUTRAL = \"#").expect("the viewer's edge color");
    assert!(
        SCRIPT.contains(&format!("EDGE_NEUTRAL = \"{}\"", &VIEWER[at + 16..at + 23])),
        "the edge line is not the viewer's"
    );
    let alpha = |src: &str| {
        let at = src.find("EDGE_ALPHA = ").expect("EDGE_ALPHA");
        src[at + 13..].split([';', ',', ' ']).next().unwrap().to_string()
    };
    assert_eq!(alpha(SCRIPT), alpha(VIEWER), "the page's edge alpha is not the viewer's");

    // The trust rings are the viewer's: gold-bright 2.2 wide, a host's signature thin ink, the lowest
    // tier dashed muted, each 3.5 outside the node; the contested badge is the viewer's coral disc.
    let ring = body(SCRIPT, "function ring(");
    assert!(
        ring.contains("r + 3.5")
            && ring.contains("ctx.lineWidth = 2.2; ctx.strokeStyle = C.goldHi;"),
        "the gold ring is not the viewer's"
    );
    assert!(
        ring.contains("ctx.lineWidth = 1.2; ctx.strokeStyle = C.ink;"),
        "the signed ring is not the viewer's"
    );
    assert!(
        body(SCRIPT, "function dot(")
            .contains("ctx.arc(bx, by, 7 * o.bad, 0, 7); ctx.fillStyle = C.bad;"),
        "the contested badge is not the viewer's"
    );

    // The page names no trust tier itself: every tier it draws comes from the cases (U12).
    for tier in [
        "human confirmed",
        "host signed",
        "agent extracted",
        "unverified",
        "human_confirmed",
        "host_signed",
        "agent_extracted",
    ] {
        assert!(
            !SCRIPT.to_lowercase().contains(tier),
            "principles.js names the tier `{tier}` instead of reading it from the cases"
        );
    }
    // An owed demand has its own mark, never a dashed viewer mark.
    assert!(SCRIPT.contains("function owedBox("), "the page's owed style is gone");
}

#[test]
fn the_principles_page_plays_once_can_stop_and_rests() {
    // Reduced motion is read when a figure starts, so a change of the setting is followed, and a frame
    // is requested only while a figure plays.
    assert!(SCRIPT.contains("const RM = matchMedia(\"(prefers-reduced-motion: reduce)\");"));
    let anim = body(SCRIPT, "function animate(");
    assert!(
        anim.contains("if (RM.matches) { frame(Infinity);"),
        "reduced motion does not draw the resting frame"
    );
    assert_eq!(
        SCRIPT.matches("requestAnimationFrame(").count(),
        anim.matches("requestAnimationFrame(").count(),
        "a frame is requested outside animate"
    );
    assert!(
        SCRIPT.contains("document.addEventListener(\"visibilitychange\""),
        "a hidden tab does not stop the figures"
    );
    // Each scene plays once when it first comes into view, and while it plays its control reads Stop.
    let mount = body(SCRIPT, "function mountScene(");
    assert!(
        mount.contains("onceVisible(fig, () => run(0, END));")
            && mount.contains("play.textContent = \"Stop\""),
        "a scene does not play once or cannot be stopped"
    );
    // A caption is announced only after the reader presses a control.
    assert!(
        mount.contains("\"aria-live\": \"off\"")
            && SCRIPT.contains("c.setAttribute(\"aria-live\", \"polite\")")
    );
    // A canvas far from the window gives up its pixels.
    assert!(body(SCRIPT, "function surface(").contains("cv.width = cv.height = 1;"));

    // The page: every canvas is hidden from assistive technology, and every beat is text in the page.
    assert!(PAGE.matches("<canvas").count() >= 24, "the page has a canvas per figure");
    assert_eq!(
        PAGE.matches("<canvas").count(),
        PAGE.matches("aria-hidden=\"true\"></canvas>").count(),
        "a canvas is exposed to assistive technology"
    );
    let scenes = PAGE.matches("<figure class=\"scene\"").count();
    assert_eq!(scenes, 19, "nineteen principles are told in beats");
    assert_eq!(PAGE.matches("<ol class=\"beats\">").count(), scenes, "a scene has no beat list");
    for list in PAGE.split("<ol class=\"beats\">").skip(1) {
        let list = &list[..list.find("</ol>").unwrap()];
        let ts: Vec<f64> = list
            .split("data-t=\"")
            .skip(1)
            .map(|t| t[..t.find('"').unwrap()].parse().unwrap())
            .collect();
        assert!(
            (2..=4).contains(&ts.len()) && ts.windows(2).all(|w| w[0] < w[1]),
            "beats {ts:?} are not two to four in order"
        );
    }
    assert_eq!(PAGE.matches("<figure class=\"demo\"").count(), 5, "five principles are demos");
    // Without a script nothing is drawn: the canvases and controls show only under `.js`.
    for rule in [
        ".scene canvas, .demo canvas { display: none;",
        ".js .scene canvas, .js .demo canvas { display: block; }",
        ".js .scene .beats { display: none; }",
        ".tmap { display: none;",
    ] {
        assert!(CSS.contains(rule), "site/style.css lost `{rule}`");
    }
    // The page carries no style of its own, so the landing's stylesheet rules (U7, U9) hold it.
    assert!(
        !PAGE.contains("<style") && !PAGE.contains(" style="),
        "the page styles itself outside the stylesheet"
    );
    assert!(
        !SCRIPT.contains(".style.border") && !SCRIPT.contains(".style.outline"),
        "the script draws a line on an area"
    );
    // The page's words are copied as nodes, never reparsed as markup.
    assert!(
        !SCRIPT.contains("innerHTML")
            && !SCRIPT.contains("outerHTML")
            && !SCRIPT.contains("insertAdjacentHTML"),
        "the script writes markup"
    );
}

#[test]
fn the_landing_links_the_principles_page() {
    let nav = &LANDING[LANDING.find("<nav aria-label=\"Sections\">").unwrap()..];
    let nav = &nav[..nav.find("</nav>").unwrap()];
    assert!(nav.contains("<a href=\"#how\">How it works</a>\n      <a href=\"principles/\">Principles</a>\n      <a href=\"#status\">Status</a>"), "the navigation does not name the page between How it works and Status");
    for p in [3, 5, 17, 19] {
        assert!(
            LANDING.contains(&format!("<a class=\"pref\" href=\"principles/#p{p}\">P{p}</a>")),
            "the property that summarizes P{p} does not link it"
        );
    }
    assert!(
        LANDING.contains("<a href=\"principles/#how-held\">"),
        "the status note does not link the method"
    );
    let foot = &LANDING[LANDING.find("<nav aria-label=\"Footer\">").unwrap()..];
    assert!(
        foot[..foot.find("</nav>").unwrap()].contains("<a href=\"principles/\">Principles</a>"),
        "the footer links the document, not the page"
    );
}

/// docs/design-system.md Section 11a: an owed clause is an empty cell with a --muted outline and the
/// chosen column has a --gold-bright one, so both measure as marks (WCAG 1.4.11); the detail panel puts
/// no heading under the page's own; and a "/" between two shown clauses is not read aloud.
#[test]
fn the_principles_ledger_marks_owed_and_chosen_clearly() {
    let rule = |sel: &str| {
        let at = CSS.find(&format!("{sel} {{")).unwrap_or_else(|| panic!("no rule {sel}"));
        let body = &CSS[at + sel.len() + 2..];
        body[..body.find('}').expect("it closes")].to_string()
    };
    let owed = rule(".cell.o");
    assert!(owed.contains("background: none") && owed.contains("border: 1px solid var(--muted)"), "an owed cell is a filled block: {owed}");
    let chosen = rule(".sky .col[aria-current=\"true\"]");
    assert!(chosen.contains("border: 1px solid var(--gold-bright)"), "the chosen column is not outlined: {chosen}");
    assert!(!SCRIPT.contains("el(\"h3\", null, $(\"h3\", art).textContent)"), "the detail panel inserts a heading");
    assert!(!PAGE.contains("</span> / <span data-clause"), "a separator is read aloud");
    assert!(PAGE.contains("<span class=\"sep\" aria-hidden=\"true\"> / </span>"), "the separators are not hidden from a screen reader");
}

/// Section 11a: owed is defined where it is first read, a glossary names each word the page uses
/// once, and below 900 pixels the ledger is a row per chapter whose columns go to their principle.
#[test]
fn the_principles_page_says_what_its_words_mean_and_reads_on_a_phone() {
    let sub = &PAGE[PAGE.find("<p class=\"sub\">").expect("the hero's subhead")..];
    let sub = &sub[..sub.find("</p>").expect("it closes")];
    assert!(sub.contains("does not yet guarantee"), "the hero does not say what owed means");
    let end = PAGE.find("<!-- /registry:ledger -->").expect("the ledger's end");
    let gloss = &PAGE[end..];
    let gloss = &gloss[..gloss.find("</dl>").expect("a glossary after the ledger")];
    for term in ["Clause", "Tested", "By construction", "Pinned", "Owed", "Tension"] {
        assert!(gloss.contains(&format!("<dt>{term}</dt>")), "the glossary does not name {term}");
    }
    let phone = &CSS[CSS.rfind("@media (max-width: 899px)").expect("the narrow rules")..];
    assert!(phone.contains(".sky .cols { flex-direction: column"), "the ledger is not a row per chapter on a phone");
    assert!(SCRIPT.contains("matchMedia(\"(max-width: 899px)\").matches"), "a column on a phone does not go to its principle");
}

