//! docs/design-system.md U10: the landing's stage (site/stage.js) draws the viewer's marks with the
//! viewer's meanings. It is a picture of the product, so a mark that drifted from the viewer would
//! teach a visitor something the app does not show. The stage is a static file with no build step
//! and no test harness of its own; this file reads its source beside the viewer's script, the
//! landing's stylesheet and the page, the way the http.rs tripwires read the viewer's.

const STAGE: &str = include_str!("../../../site/stage.js");
const PAGE: &str = include_str!("../../../site/index.html");
const LANDING_CSS: &str = include_str!("../../../site/style.css");
const VIEWER: &str = include_str!("../assets/viewer.js");

/// The source of a top-level function or const in `src`, from its first line to the brace or
/// semicolon that closes it, brackets counted, each line with its leading whitespace dropped - the
/// stage keeps its copies one level deeper, inside its wrapper.
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

/// The value of a token in the landing's `:root` block.
fn token(name: &str) -> String {
    let root = &LANDING_CSS[LANDING_CSS.find(":root {").expect("a :root block")..];
    let root = &root[..root.find('}').expect("the :root block closes")];
    let decl = root
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("site/style.css does not declare {name}"));
    decl[name.len() + 1..].trim().trim_end_matches(';').trim().to_lowercase()
}

/// A `const NAME = "#rrggbb"` color in the viewer's script.
fn viewer_color(name: &str) -> String {
    let at = VIEWER
        .find(&format!("{name} = \"#"))
        .unwrap_or_else(|| panic!("viewer.js: no {name}"));
    VIEWER[at + name.len() + 4..at + name.len() + 11].to_lowercase()
}

/// The source of one scene, from its id to the next scene's.
fn scene(id: &str) -> &'static str {
    let at = STAGE
        .find(&format!("{{ id: \"{id}\""))
        .unwrap_or_else(|| panic!("no scene {id}"));
    let rest = &STAGE[at + 1..];
    &STAGE[at..at
        + 1
        + rest
            .find("{ id: \"")
            .unwrap_or_else(|| rest.find("\n  ];").expect("SCENES closes"))]
}

#[test]
fn the_stage_draws_the_viewers_marks() {
    // A type's color is the viewer's function of its name, copied character for character.
    for start in [
        "const TYPE_BAND",
        "const BAND_HUES",
        "function nameSlot(",
        "function stableColors(",
    ] {
        assert_eq!(
            block(STAGE, start),
            block(VIEWER, start),
            "stage.js's `{start}` is not the viewer's"
        );
    }
    assert!(
        STAGE.contains("const TYPE = stableColors(["),
        "the stage's type colors come from stableColors"
    );
    assert!(
        !STAGE.contains("hsl(") && !STAGE.contains("oklch("),
        "the stage computes a color the viewer's band does not hold"
    );

    // Every palette color is a landing token's value, named beside it.
    let pal = &STAGE[STAGE.find("const C = {").expect("the stage's palette")..];
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
        assert_eq!(hex.to_lowercase(), token(tok), "stage.js {line}");
        seen += 1;
    }
    assert!(seen >= 8, "the palette was read ({seen} colors)");
    let hexes =
        STAGE.matches("\"#").count() - block(STAGE, "const TYPE_BAND").matches("\"#").count();
    assert_eq!(
        hexes,
        seen + 1,
        "stage.js writes a color outside its palette, the band and the edge line"
    );

    // A relation is the viewer's neutral line at its alpha.
    assert!(STAGE.contains(&format!("EDGE_NEUTRAL = \"{}\"", viewer_color("EDGE_NEUTRAL"))));
    let alpha = |src: &str| {
        let at = src.find("EDGE_ALPHA = ").expect("EDGE_ALPHA");
        src[at + 13..].split([';', ',', ' ']).next().unwrap().to_string()
    };
    assert_eq!(alpha(STAGE), alpha(VIEWER), "the stage's edge alpha is not the viewer's");

    // Human confirmed is the viewer's ring: gold-bright, 2.4 wide, 3.5 outside the node.
    let ring = &VIEWER[VIEWER.find("function trustRing(").expect("trustRing")..];
    let ring = &ring[..ring.find("\n}\n").expect("trustRing closes")];
    assert!(
        ring.contains("r + 3.5")
            && ring.contains("bulk.lineWidth = 2.4/cam.s; bulk.strokeStyle = EDGE_HI;")
    );
    assert_eq!(viewer_color("EDGE_HI"), token("--gold-bright"));
    assert!(
        STAGE.contains("ctx.arc(x, y, r + 3.5,")
            && STAGE.contains("ctx.lineWidth = 2.4; ctx.strokeStyle = C.goldHi;")
    );

    // Contested is the viewer's badge: --bad, at the node's upper right, with circle-alert's mark
    // (design-system.md Section 28.3, U41) rather than a typed "!".
    for src in [VIEWER, STAGE] {
        assert!(
            src.contains("+ r * 0.75") && src.contains("- r * 0.75"),
            "the badge sits at the upper right"
        );
    }
    assert_eq!(viewer_color("BAD"), token("--bad"));
    assert!(
        STAGE.contains("ctx.fillStyle = C.bad;")
            && STAGE.contains("alertMark(ctx, bx, by, 7, C.bg)")
    );
    for src in [VIEWER, STAGE] {
        assert!(
            src.contains("new Path2D(\"M12 8v4M12 16h.01\")") && !src.contains("\"!\""),
            "the badge's mark is circle-alert's, not a typed \"!\""
        );
    }

    // The second machine replays the same log, so it draws the marks the first one does
    // (resolution.md Section 6: a verdict's ceiling is read from the log, the same on every node).
    // Every ring and badge is set in one function, and both machines' scenes call it.
    let at = STAGE.find("\n  function settle(").expect("stage.js sets its marks in settle()");
    let end = at + STAGE[at..].find("\n  }\n").expect("settle() closes");
    for mark in [".ring = ", ".ringP = ", ".bad = "] {
        for (i, _) in STAGE.match_indices(mark) {
            assert!(
                i > at && i < end,
                "stage.js sets `{mark}` outside settle(), where one machine may not"
            );
        }
    }
    assert!(
        scene("govern").contains("settle(st, "),
        "the first machine's marks do not come from settle()"
    );
    assert!(
        scene("travel").contains("settle(b, "),
        "the second machine does not draw the first one's marks"
    );
    assert!(
        !STAGE.contains("\"thin\""),
        "a ring other than human confirmed: say which tier it draws"
    );

    // Motion: each scene ends within five seconds and then rests, and reduced motion is read and
    // followed when it changes.
    let mut scenes = 0;
    for (i, _) in STAGE.match_indices("\", dur: ") {
        let d: f64 = STAGE[i + 8..].split(',').next().unwrap().trim().parse().expect("a duration");
        assert!(d <= 5.0, "a scene runs {d}s");
        scenes += 1;
    }
    assert_eq!(scenes, 6, "the scenes were read: the overview and five steps");
    assert!(STAGE.contains("matchMedia(\"(prefers-reduced-motion: reduce)\")"));
    assert!(
        STAGE.contains("playing = next && !reduce.matches"),
        "a scene plays under reduced motion"
    );
    assert!(
        STAGE.contains("reduce.addEventListener(\"change\""),
        "a change of the setting is not followed"
    );
    assert!(
        STAGE.contains("if (playing || snap) raf = requestAnimationFrame(frame);"),
        "the stage draws at rest"
    );

    // The page: the canvases are hidden from assistive technology, and each of the five steps says
    // its scene in its own words.
    let canvases = PAGE.matches("<canvas").count();
    assert_eq!(
        canvases,
        PAGE.matches("<canvas class=\"frame\" aria-hidden=\"true\">").count() + 1
    );
    assert!(PAGE.contains("<canvas id=\"stage\" aria-hidden=\"true\">"));
    for n in 1..=5 {
        let at = PAGE
            .find(&format!("data-scene=\"{n}\""))
            .unwrap_or_else(|| panic!("no step {n}"));
        let step = &PAGE[at..at + PAGE[at..].find("</article>").expect("the step closes")];
        assert!(
            step.contains("<h3>") && step.contains("<p>"),
            "step {n} has no words of its own"
        );
    }
}

/// The deploy stamps the release into the hero's facts with one sed over this exact element
/// (.github/workflows/pages.yml), and fails unless it finds exactly one.
#[test]
fn the_landing_has_one_version_badge() {
    assert_eq!(PAGE.matches("<li id=\"version-badge\">").count(), 1);
}

/// docs/design-system.md Section 11: after the install command the next step is said, Install is in
/// the header, the command wraps on a phone with Copy on its own line, a code block that scrolls takes
/// focus, the Review picture's chips are a picture and not buttons, the header's links are a finger
/// apart, and "Copied" is announced.
#[test]
fn the_landing_says_the_next_step_and_reads_on_a_phone() {
    // The one line opens the app, and the line under it names Setup (design-system.md Section
    // 29.10); the script's line names `supragnosis setup`.
    assert!(
        PAGE.contains("brew install --cask ashon/tap/supragnosis &amp;&amp; open -a Supragnosis"),
        "the hero's command does not open the app"
    );
    assert!(
        PAGE.contains("<p class=\"next\">The app's Setup opens:")
            && PAGE.contains("With the script, run <code>supragnosis setup</code>."),
        "the hero does not say what follows the install"
    );
    assert!(PAGE.contains("Up and running in a few minutes."), "the promise is D18's");
    assert!(
        PAGE.contains("<a href=\"#get-started\">Install</a>"),
        "the header has no Install link"
    );
    assert!(
        PAGE.contains("<pre class=\"code\" tabindex=\"0\""),
        "a scrolling code block cannot take focus"
    );
    assert_eq!(
        PAGE.matches("<pre class=\"code\"").count(),
        PAGE.matches("<pre class=\"code\" tabindex=\"0\"").count(),
        "every code block takes focus"
    );
    assert!(
        PAGE.contains("id=\"copied\"") && PAGE.contains("aria-live=\"polite\""),
        "Copied is not announced"
    );
    assert!(
        PAGE.contains("document.getElementById('copied').textContent"),
        "the copy script does not announce"
    );
    let phone =
        &LANDING_CSS[LANDING_CSS.find("@media (max-width: 560px)").expect("the phone rules")..];
    let phone = &phone[..phone.find("\n}\n").expect("they close")];
    assert!(
        phone.contains(".cmd {") && phone.contains("flex-direction: column"),
        "the command does not wrap on a phone"
    );
    assert!(
        phone.contains(".cmd code {") && phone.contains("white-space: normal"),
        "the command still scrolls on a phone"
    );
    let nav = &LANDING_CSS[LANDING_CSS.find(".site-head nav {").expect("the nav")..];
    let nav = &nav[..nav.find('}').expect("it closes")];
    assert!(nav.contains("gap: 8px 24px"), "the header's links are closer than a finger");
    let act = &LANDING_CSS[LANDING_CSS.find(".snip.review .act {").expect("the Review picture")..];
    let act = &act[..act.find('}').expect("it closes")];
    assert!(
        !act.contains("background: var(--accent)"),
        "the picture's chip looks like a button to press"
    );
}
