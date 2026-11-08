//! docs/design-system.md U10: the landing's stage (site/stage.js) draws the viewer's marks with the
//! viewer's meanings. It is a picture of the product, so a mark that drifted from the viewer would
//! teach a visitor something the app does not show. The stage is a static file with no build step
//! and no test harness of its own; this file reads its source beside the viewer's script, the
//! landing's stylesheet and the page, the way the http.rs tripwires read the viewer's.

const STAGE: &str = include_str!("../../../site/stage.js");
const LANDING_JS: &str = include_str!("../../../site/landing.js");
const PRINCIPLES_JS: &str = include_str!("../../../site/principles.js");
const PAGE: &str = include_str!("../../../site/index.html");
const KO_PAGE: &str = include_str!("../../../site/ko/index.html");
const PRIVACY: &str = include_str!("../../../site/privacy/index.html");
const KO_PRIVACY: &str = include_str!("../../../site/ko/privacy/index.html");
const PRINCIPLES: &str = include_str!("../../../site/principles/index.html");
const KO_PRINCIPLES: &str = include_str!("../../../site/ko/principles/index.html");
const LANDING_CSS: &str = include_str!("../../../site/style.css");
const VIEWER: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));

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

    // Human confirmed is the viewer's ring, gold-bright, and contested the viewer's badge, --bad.
    assert_eq!(viewer_color("EDGE_HI"), token("--gold-bright"));
    assert_eq!(viewer_color("BAD"), token("--bad"));

    // The second machine replays the same log, so it draws the marks the first one does
    // (resolution.md Section 6: a verdict's ceiling is read from the log, the same on every node):
    // both machines' scenes set their marks with settle().
    assert!(
        scene("govern").contains("settle(st, "),
        "the first machine's marks do not come from settle()"
    );
    assert!(
        scene("travel").contains("settle(b, "),
        "the second machine does not draw the first one's marks"
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

/// The deploy stamps the release into the hero's facts of both landings with one sed over this exact
/// element (.github/workflows/pages.yml), and fails unless it finds exactly one in each.
#[test]
fn the_landing_has_one_version_badge() {
    for page in [PAGE, KO_PAGE] {
        assert_eq!(page.matches("<li id=\"version-badge\">").count(), 1);
    }
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
        LANDING_JS.contains("document.getElementById(\"copied\").textContent"),
        "the copy script does not announce"
    );
    assert!(
        PAGE.contains("<script src=\"landing.js\" defer></script>")
            && KO_PAGE.contains("<script src=\"../landing.js\" defer></script>"),
        "a landing does not load its controls"
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
    // The govern step's Accept is a button since step 66, but not the page's call to action: that
    // stays the install command's gold.
    let act =
        &LANDING_CSS[LANDING_CSS.find(".snip.review .act {").expect("the Review row's Accept")..];
    let act = &act[..act.find('}').expect("it closes")];
    assert!(
        !act.contains("background: var(--accent)"),
        "a Review row's Accept takes the install command's gold"
    );
}

/// The `data-` attributes whose value a figure draws as words, so a twin says them in its own
/// language: a tension's side that is not a principle, and the place a question is routed to.
const SAID: &[&str] = &["data-side", "data-route"];

/// The elements of a page that carry an id, a class or a `data-` attribute, in order, each as its
/// tag and those attributes: what the pages' scripts and stylesheet hold on to, and so what a
/// page's twin in the other language keeps (docs/i18n.md LG4). An attribute in `SAID` is kept by its
/// name alone. Comments are skipped.
fn skeleton(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        if rest.starts_with("<!--") {
            rest = &rest[rest.find("-->").expect("a comment closes") + 3..];
            continue;
        }
        let end = rest.find('>').expect("a tag closes");
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        if tag.starts_with(['/', '!']) {
            continue;
        }
        let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
        let mut kept = vec![tag[..name_end].to_string()];
        // each `name="value"` pair, a value read whole up to its closing quote
        let mut attrs = &tag[name_end..];
        while let Some(eq) = attrs.find("=\"") {
            let name = attrs[..eq].split_whitespace().last().unwrap_or_default();
            let value_end = attrs[eq + 2..].find('"').expect("a value closes");
            let value = &attrs[eq + 2..eq + 2 + value_end];
            if SAID.contains(&name) {
                kept.push(name.to_string());
            } else if name == "id" || name == "class" || name.starts_with("data-") {
                kept.push(format!("{name}={value}"));
            }
            attrs = &attrs[eq + 2 + value_end + 1..];
        }
        if kept.len() > 1 {
            out.push(kept.join(" "));
        }
    }
    out
}

/// docs/i18n.md LG2 and LG4: the landing, the Privacy page and the principles page each have a
/// Korean twin under site/ko/. Each page declares its language, links its twin by the twin's own
/// name, names both as alternates with its own canonical address, and the twin keeps the English
/// page's skeleton and, on the principles page, its cases.
#[test]
fn each_site_page_has_its_korean_twin() {
    let site = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site");
    let host = "https://supragnosis.dev/";
    for (dir, en, ko) in [
        ("", PAGE, KO_PAGE),
        ("privacy/", PRIVACY, KO_PRIVACY),
        ("principles/", PRINCIPLES, KO_PRINCIPLES),
    ] {
        assert!(en.contains("<html lang=\"en\">"), "/{dir} does not say it is English");
        assert!(ko.contains("<html lang=\"ko\">"), "/ko/{dir} does not say it is Korean");
        // the twin links, each resolving to the other page's file
        let up = "../".repeat(dir.matches('/').count());
        let to_ko = format!("<a href=\"{up}ko/{dir}\" hreflang=\"ko\" lang=\"ko\">한국어</a>");
        let to_en = format!("<a href=\"../{up}{dir}\" hreflang=\"en\" lang=\"en\">English</a>");
        assert!(en.contains(&to_ko), "/{dir} does not link its twin: {to_ko}");
        assert!(ko.contains(&to_en), "/ko/{dir} does not link its twin: {to_en}");
        assert!(site.join("ko").join(dir).join("index.html").is_file());
        for (page, own) in [(en, format!("{host}{dir}")), (ko, format!("{host}ko/{dir}"))] {
            assert!(page.contains(&format!("<link rel=\"canonical\" href=\"{own}\">")));
            for (lang, url) in [("en", format!("{host}{dir}")), ("ko", format!("{host}ko/{dir}"))]
                .into_iter()
                .chain([("x-default", format!("{host}{dir}"))])
            {
                let alt = format!("<link rel=\"alternate\" hreflang=\"{lang}\" href=\"{url}\">");
                assert!(page.contains(&alt), "{own} does not name its alternate: {alt}");
            }
        }
        let (a, b) = (skeleton(en), skeleton(ko));
        assert!(a.len() >= 10, "/{dir}: the skeleton was not read ({} elements)", a.len());
        if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
            panic!(
                "/ko/{dir} leaves the English skeleton at element {i}: {:?} in English, {:?} in Korean",
                a.get(i),
                b.get(i)
            );
        }
        // the cases are data, not words: the same block on both pages (principles_page.rs replays
        // the English one)
        let cases = |html: &'static str| {
            html.split_once("<script type=\"application/json\" id=\"cases\">")
                .map(|(_, rest)| rest.split_once("</script>").expect("the cases close").0)
        };
        assert_eq!(cases(en), cases(ko), "/ko/{dir} does not hold the English page's cases");
    }
}

/// The entries of a script's words table (`WORDS = {` to its closing `};`), one per line as
/// `key: { en: "...", ko: "..." },` with comment lines between them, and the script after the table.
fn words<'a>(file: &str, src: &'a str) -> (Vec<(String, String, String)>, &'a str) {
    let at = src.find("WORDS = {").unwrap_or_else(|| panic!("{file} has no words table"));
    let mut pos = at + src[at..].find('\n').expect("the table goes on") + 1;
    let mut out = Vec::new();
    for line in src[pos..].split_inclusive('\n') {
        pos += line.len();
        let line = line.trim();
        if line == "};" {
            return (out, &src[pos..]);
        }
        if line.starts_with("//") {
            continue;
        }
        let parsed = (|| {
            let (key, rest) = line.split_once(": { en: ")?;
            let (en, rest) = js_string(rest)?;
            let (ko, rest) = js_string(rest.strip_prefix(", ko: ")?)?;
            (rest == " }," || rest == " }").then(|| (key.into(), en, ko))
        })();
        out.push(parsed.unwrap_or_else(|| panic!("{file}: an entry that does not read: {line}")));
    }
    panic!("{file}: the words table does not close")
}

/// The double-quoted JavaScript string `s` starts with, its escaped characters unescaped, and what
/// follows it.
fn js_string(s: &str) -> Option<(String, &str)> {
    let s = s.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &s[i + 1..])),
            c => out.push(c),
        }
    }
    None
}

/// The named placeholders of a string, sorted.
fn placeholders(s: &str) -> Vec<String> {
    let mut out: Vec<String> = s
        .split('{')
        .skip(1)
        .filter_map(|p| p.split_once('}').map(|(name, _)| name.to_string()))
        .collect();
    out.sort();
    out
}

/// A call's arguments: the text after a call's opening parenthesis, split at the commas at its own
/// depth up to the parenthesis that closes it, strings skipped.
fn args(rest: &str) -> Vec<&str> {
    let (mut out, mut from) = (Vec::new(), 0);
    let (mut depth, mut quote, mut escaped) = (0usize, None::<char>, false);
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (Some(_), _) if escaped => escaped = false,
            (Some(_), '\\') => escaped = true,
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') if depth > 0 => depth -= 1,
            (None, ',') if depth == 0 => {
                out.push(rest[from..i].trim());
                from = i + 1;
            }
            (None, ')') => {
                out.push(rest[from..i].trim());
                return out;
            }
            _ => {}
        }
    }
    out
}

/// The keys a script looks up in its words table through `calls`, the functions that take a key:
/// an argument that is a key, both keys of one that picks with `? :`, and the keys a map names when
/// an argument looks its key up in one (`t(DONE[act])`, the map written on one line as
/// `DONE = ...`); and whether it looks a key up by a number, as `"caption" + i`.
fn lookups(after: &str, calls: &[&str]) -> (Vec<String>, bool) {
    let (mut keys, mut numbered) = (Vec::new(), false);
    let strings = |mut rest: &str, keys: &mut Vec<String>| {
        while let Some(q) = rest.find('"') {
            let (key, tail) = js_string(&rest[q..]).expect("a closed string");
            keys.push(key);
            rest = tail;
        }
    };
    for call in calls {
        for (i, _) in after.match_indices(call) {
            // the lookup itself, not the end of a longer name like createElement(
            let before = after[..i].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            for arg in args(&after[i + call.len()..]) {
                let map = arg.find('[').map(|b| &arg[..b]);
                match js_string(arg) {
                    Some((key, "")) => keys.push(key),
                    Some((key, tail)) if key == "caption" && tail.trim() == "+ i" => {
                        numbered = true
                    }
                    _ if map.is_some_and(|m| {
                        !m.is_empty() && m.chars().all(|c| c.is_ascii_uppercase())
                    }) =>
                    {
                        let def = format!(" {} = ", map.unwrap());
                        let at = after
                            .find(&def)
                            .unwrap_or_else(|| panic!("no map {def:?} for {arg:?}"));
                        strings(after[at..].lines().next().unwrap_or_default(), &mut keys);
                    }
                    // a pick, `cond ? "a" : "b"`
                    _ if arg.contains(" ? ") => {
                        strings(&arg[arg.find(" ? ").unwrap()..], &mut keys)
                    }
                    _ => {}
                }
            }
        }
    }
    (keys, numbered)
}

/// docs/i18n.md Section 4, its D8, for the site's scripts: every entry of a words table holds both
/// languages with the same named placeholders, no Korean placeholder is followed directly by a
/// particle that would change with the name's last sound, every key a script looks up is in its
/// table, and every entry is looked up.
#[test]
fn the_sites_words_hold_both_languages() {
    // the particles whose form follows the last sound of the word before them
    const PARTICLES: &[char] = &['을', '를', '이', '가', '은', '는', '과', '와', '로', '으'];
    for (file, src, calls) in [
        ("site/stage.js", STAGE, &["word("][..]),
        // the landing also hands a key to tier(), which splits its entry around the tier's word
        ("site/landing.js", LANDING_JS, &["t(", "tier("]),
        ("site/principles.js", PRINCIPLES_JS, &["word("]),
    ] {
        let (table, after) = words(file, src);
        assert!(table.len() >= 10, "{file}: the table was not read ({} entries)", table.len());
        for (key, en, ko) in &table {
            assert!(!en.is_empty() && !ko.is_empty(), "{file} {key}: a language is empty");
            assert_eq!(placeholders(en), placeholders(ko), "{file} {key}: the placeholders differ");
            for (i, _) in ko.match_indices('}') {
                let next = ko[i + 1..].chars().next();
                assert!(
                    !next.is_some_and(|c| PARTICLES.contains(&c)),
                    "{file} {key}: a particle follows a placeholder in {ko:?}"
                );
            }
        }
        let (keys, numbered) = lookups(after, calls);
        assert!(keys.len() >= 10, "{file}: the lookups were not read ({} keys)", keys.len());
        for key in &keys {
            assert!(
                table.iter().any(|(k, ..)| k == key),
                "{file} looks up {key:?}, which it lacks"
            );
        }
        for (key, ..) in &table {
            // the stage's captions are looked up by their number, "caption" + i
            let by_number =
                numbered && key.strip_prefix("caption").is_some_and(|n| n.parse::<u8>().is_ok());
            assert!(keys.contains(key) || by_number, "{file}: {key} is never looked up");
        }
    }
}
