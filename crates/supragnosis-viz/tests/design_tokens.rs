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
const CORE: &[&str] = &["--surface", "--panel", "--line", "--ink", "--muted", "--accent"];
/// What text is drawn in, and what it is drawn on. A signal drawn as text takes its `-ink` token, and
/// gold drawn as text is `--gold-ink` in the tools and `--accent-ink` on the site (Section 3).
const TEXT: &[&str] = &[
    "--ink",
    "--ink2",
    "--muted",
    "--gold-ink",
    "--accent-ink",
    "--ok-ink",
    "--warn-ink",
    "--bad-ink",
];
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

/// APCA 0.0.98G's lightness contrast, as a positive Lc for light text on a dark ground: the method
/// Apple's guidelines name beside WCAG's ratio, and the one that sees what the ratio misses on a dark
/// ground (Section 1's third measurement).
fn apca(text: &str, ground: &str) -> f64 {
    let y = |hex: &str| {
        let hex = hex.strip_prefix('#').expect("a #rrggbb color");
        let c = |i: usize| {
            (f64::from(u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")) / 255.0).powf(2.4)
        };
        0.2126729 * c(0) + 0.7151522 * c(2) + 0.0721750 * c(4)
    };
    let clamp = |v: f64| if v > 0.022 { v } else { v + (0.022 - v).powf(1.414) };
    let (yt, yb) = (clamp(y(text)), clamp(y(ground)));
    let s = if yb > yt {
        (yb.powf(0.56) - yt.powf(0.57)) * 1.14
    } else {
        (yb.powf(0.65) - yt.powf(0.62)) * 1.14
    };
    if s.abs() < 0.1 {
        0.0
    } else {
        (s.abs() - 0.027) * 100.0
    }
}

/// The third pass's floor (U7): every text token measures APCA Lc 60 on every surface token, and
/// `--ink2`, which carries what a person reads to act, Lc 75. WCAG's 4.5:1 held while 446 of the
/// viewer's 496 texts were 11 or 12 pixels at Lc 59 and below.
#[test]
fn every_text_token_reads_on_every_surface_by_apca() {
    assert!((apca("#e9e4d6", "#10131b") - 89.8).abs() < 0.2, "APCA is not 0.0.98G's");
    let mut low = Vec::new();
    let mut pairs = 0;
    for (file, set) in &surfaces() {
        for text in TEXT {
            let Some(fg) = value(set, text) else { continue };
            let floor = if *text == "--ink2" { 75.0 } else { 60.0 };
            for ground in SURFACES {
                let Some(bg) = value(set, ground) else {
                    continue;
                };
                pairs += 1;
                let lc = apca(fg, bg);
                if lc < floor {
                    low.push(format!("{file}: {text} on {ground} is Lc {lc:.1}, under {floor}"));
                }
            }
        }
    }
    assert!(pairs >= 40, "the pairs were read ({pairs})");
    assert!(low.is_empty(), "text under its APCA floor:\n  {}", low.join("\n  "));
}

/// A `#rrggbb` color or an `rgba(r,g,b,a)` tint laid over an opaque `#rrggbb` ground, as `#rrggbb`.
fn over(tint: &str, ground: &str) -> String {
    let g =
        |i: usize| f64::from(u8::from_str_radix(&ground[1 + i * 2..3 + i * 2], 16).expect("hex"));
    let inner = tint
        .strip_prefix("rgba(")
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or_else(|| panic!("a tint is rgba(): {tint}"));
    let parts: Vec<f64> = inner.split(',').map(|v| v.parse().expect("a number")).collect();
    let alpha = parts[3];
    let mix = |i: usize| (parts[i] * alpha + g(i) * (1.0 - alpha)).round() as u8;
    format!("#{:02x}{:02x}{:02x}", mix(0), mix(1), mix(2))
}

/// Text on a pill's tint meets the same floor over it (Section 3, U7): the tint is its signal at 12%,
/// laid on `--panel` and on `--panel-2`, where pills sit. `--ok-ink` read Lc 59.4 on the old 13% tint.
#[test]
fn text_on_a_pill_reads_over_its_tint() {
    let pairs = [
        ("--ok-ink", "--ok-glass"),
        ("--warn-ink", "--warn-glass"),
        ("--bad-ink", "--bad-glass"),
        ("--gold-ink", "--gold-glass"),
    ];
    let mut low = Vec::new();
    let mut seen = 0;
    for (file, set) in &surfaces() {
        for (ink, glass) in pairs {
            let (Some(fg), Some(tint)) = (value(set, ink), value(set, glass)) else {
                continue;
            };
            for ground in ["--panel", "--panel-2"] {
                let bg = over(tint, value(set, ground).expect("a panel"));
                seen += 1;
                let lc = apca(fg, &bg);
                if lc < 60.0 {
                    low.push(format!("{file}: {ink} on {glass} over {ground} is Lc {lc:.1}"));
                }
            }
        }
    }
    assert!(seen >= 12, "the tints were read ({seen})");
    assert!(low.is_empty(), "pill text under Lc 60:\n  {}", low.join("\n  "));
}

/// Section 4 and U22: no line is dashed or dotted, on a page or on a canvas, as the author asked.
/// Trust's rings, which a dash told apart, are told by weight: 1 pixel for Unverified, 1.6 for Host
/// signed and 2.4 for Human confirmed.
#[test]
fn lines_are_thin_and_solid() {
    let strip = |src: &str| {
        let mut clean = String::new();
        let mut rest = src;
        while let Some(i) = rest.find("/*") {
            clean.push_str(&rest[..i]);
            rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
        }
        clean.push_str(rest);
        clean
            .lines()
            .map(|l| l.split(" //").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    };
    for (file, src) in [
        ("viewer.css", VIEWER),
        ("settings.css", SETTINGS),
        ("welcome.css", include_str!("../../../app/assets/welcome.css")),
        ("site/style.css", LANDING),
        ("viewer.js", include_str!("../assets/viewer.js")),
        ("site/stage.js", include_str!("../../../site/stage.js")),
        ("site/principles.js", include_str!("../../../site/principles.js")),
    ] {
        let code = strip(src);
        assert!(
            !code.contains("dashed") && !code.contains("dotted"),
            "{file} draws a dashed line"
        );
        let mut rest = code.as_str();
        while let Some(i) = rest.find("setLineDash([") {
            rest = &rest[i + "setLineDash([".len()..];
            assert!(rest.starts_with(']'), "{file} draws a dash pattern");
        }
    }
    let script = include_str!("../assets/viewer.js");
    let ring = &script[script.find("function trustRing(").expect("trustRing")..];
    let ring = &ring[..ring.find("\n}\n").expect("trustRing closes")];
    for weight in [
        "bulk.lineWidth = 1/cam.s; bulk.strokeStyle = OTHER;",
        "bulk.lineWidth = 1.6/cam.s; bulk.strokeStyle = INK;",
        "bulk.lineWidth = 2.4/cam.s; bulk.strokeStyle = EDGE_HI;",
    ] {
        assert!(ring.contains(weight), "a trust ring is not told by weight: {weight}");
    }
    let flat: String = VIEWER.split_whitespace().collect();
    for swatch in [
        ".tsw.t-unverified{border:1pxsolidvar(--muted);}",
        ".tsw.t-host_signed{border:1.6pxsolidvar(--ink);}",
        ".tsw.t-human_confirmed{border:2.4pxsolidvar(--gold-bright);}",
    ] {
        assert!(flat.contains(swatch), "the Trust legend's swatch is not the ring: {swatch}");
    }
}

/// The custom properties of the `:root` inside a stylesheet's `@media (prefers-color-scheme: light)`
/// block, read as `tokens` reads the first one.
fn light_tokens(css: &str) -> Vec<(String, String)> {
    let at = css.find("@media (prefers-color-scheme: light) {").expect("a light appearance");
    tokens(&css[at..])
}

/// U27: the viewer and the app's pages follow the system's appearance. Every color token their dark
/// `:root` declares has a light value, the two stylesheets agree on them, and the light values meet
/// the floors the dark ones do: 4.5:1 and APCA Lc 60 for every text token on every light surface,
/// Lc 75 for `--ink2`, and the same on a pill's tint. Under Increase Contrast each light text token
/// takes the next stronger one.
#[test]
fn the_light_appearance_meets_the_same_floors() {
    let sets = [("viewer.css", VIEWER), ("settings.css", SETTINGS)];
    let mut low = Vec::new();
    for (file, css) in sets {
        let dark = tokens(css);
        let light = light_tokens(css);
        let flat: String = css.split_whitespace().collect();
        assert!(
            flat.contains("@media(prefers-color-scheme:light){:root{color-scheme:light;"),
            "{file}: the light appearance does not set color-scheme"
        );
        for (name, v) in &dark {
            let is_color = v.starts_with('#') || v.starts_with("rgba(");
            if is_color && value(&light, name).is_none() {
                low.push(format!("{file}: {name} has no light value"));
            }
        }
        for text in TEXT {
            let Some(fg) = value(&light, text) else {
                continue;
            };
            let floor = if *text == "--ink2" { 75.0 } else { 60.0 };
            for ground in SURFACES {
                let Some(bg) = value(&light, ground) else {
                    continue;
                };
                let (lc, ratio) = (apca(fg, bg), contrast(fg, bg));
                if lc < floor || ratio < 4.5 {
                    low.push(format!(
                        "{file}: light {text} on {ground} is Lc {lc:.1}, {ratio:.2}:1"
                    ));
                }
            }
        }
        for (ink, glass) in [
            ("--ok-ink", "--ok-glass"),
            ("--warn-ink", "--warn-glass"),
            ("--bad-ink", "--bad-glass"),
            ("--gold-ink", "--gold-glass"),
        ] {
            let (Some(fg), Some(tint)) = (value(&light, ink), value(&light, glass)) else {
                continue;
            };
            for ground in ["--panel", "--panel-2"] {
                let bg = over(tint, value(&light, ground).expect("a light panel"));
                let lc = apca(fg, &bg);
                if lc < 60.0 {
                    low.push(format!("{file}: light {ink} on {glass} over {ground} is Lc {lc:.1}"));
                }
            }
        }
        let ink = value(&light, "--ink").expect("--ink").to_string();
        let ink2 = value(&light, "--ink2").expect("--ink2").to_string();
        let hc = "@media (prefers-contrast: more) and (prefers-color-scheme: light) {";
        let at = css.find(hc).unwrap_or_else(|| panic!("{file}: no light Increase Contrast"));
        let block: String = css[at..at + css[at..].find('}').expect("it closes")]
            .split_whitespace()
            .collect();
        assert!(
            block.contains(&format!("--ink2:{ink}")) && block.contains(&format!("--muted:{ink2}")),
            "{file}: under Increase Contrast the light text tokens do not strengthen"
        );
    }
    let (v, s) = (light_tokens(VIEWER), light_tokens(SETTINGS));
    for (name, a) in &v {
        if let Some(b) = value(&s, name) {
            if a != b {
                low.push(format!("light {name}: viewer.css {a} but settings.css {b}"));
            }
        }
    }
    assert!(low.is_empty(), "the light appearance falls short:\n  {}", low.join("\n  "));
}

/// The colors the tools' stylesheets draw outside their tokens: text on a solid gold or coral button,
/// which reads on both grounds because the button carries its own fill (U27).
const LITERAL_COLORS: &[&str] = &["#171204", "#17130a", "#1a0d09", "#ea8b72", "rgba(23,18,4,0.14)"];

/// U27: no color the viewer, the Settings page and Setup draw is outside their tokens but the few
/// listed, so nothing stays dark in the light appearance: shadows and scrims are tokens too.
#[test]
fn no_color_is_drawn_outside_the_tokens() {
    let mut stray = Vec::new();
    for (file, css) in [
        ("viewer.css", VIEWER),
        ("settings.css", SETTINGS),
        ("welcome.css", include_str!("../../../app/assets/welcome.css")),
    ] {
        let mut clean = String::new();
        let mut rest = css;
        while let Some(i) = rest.find("/*") {
            clean.push_str(&rest[..i]);
            rest = rest[i..].find("*/").map_or("", |j| &rest[i + j + 2..]);
        }
        clean.push_str(rest);
        // Every :root block - the dark one and those inside media queries - is where colors live.
        let mut outside = String::new();
        let mut rest = clean.as_str();
        while let Some(i) = rest.find(":root {") {
            outside.push_str(&rest[..i]);
            rest = &rest[i + rest[i..].find('}').expect("a :root closes") + 1..];
        }
        outside.push_str(rest);
        let flat: String = outside.split_whitespace().collect();
        let mut at = 0;
        while let Some(i) = flat[at..].find(['#', 'r']) {
            let start = at + i;
            let tail = &flat[start..];
            let lit = if let Some(hex) = tail.strip_prefix('#') {
                let n = hex.chars().take_while(|c| c.is_ascii_hexdigit()).count();
                // An id selector such as #defsBox starts with hex letters; a color ends there.
                let ends =
                    !hex[n..].starts_with(|c: char| c.is_alphanumeric() || c == '-' || c == '_');
                ((n == 3 || n == 4 || n == 6 || n == 8) && ends).then(|| &tail[..=n])
            } else if tail.starts_with("rgba(") || tail.starts_with("rgb(") {
                tail.find(')').map(|j| &tail[..=j])
            } else {
                None
            };
            match lit {
                Some(l) if !LITERAL_COLORS.contains(&l.to_lowercase().as_str()) => {
                    stray.push(format!("{file}: {l}"));
                    at = start + l.len();
                }
                Some(l) => at = start + l.len(),
                None => at = start + 1,
            }
        }
    }
    stray.sort();
    stray.dedup();
    assert!(stray.is_empty(), "a color drawn outside the tokens:\n  {}", stray.join("\n  "));
}

/// `--faint` is retired into `--muted` (Section 3): no stylesheet declares or reads it.
#[test]
fn faint_is_retired() {
    for (file, css) in
        [("viewer.css", VIEWER), ("settings.css", SETTINGS), ("site/style.css", LANDING)]
    {
        assert!(!css.contains("--faint"), "{file} still names --faint");
    }
}

/// A signal's color is a mark's; drawn as text it is its `-ink` token, and gold drawn as text is
/// `--gold-bright` (Section 3). The color property never reads a signal or the accent itself.
#[test]
fn a_signal_drawn_as_text_takes_its_ink() {
    let mut found = Vec::new();
    for (file, css) in [
        ("viewer.css", VIEWER),
        ("settings.css", SETTINGS),
        ("welcome.css", include_str!("../../../app/assets/welcome.css")),
        ("site/style.css", LANDING),
    ] {
        let mut clean = String::new();
        let mut rest = css;
        while let Some(k) = rest.find("/*") {
            clean.push_str(&rest[..k]);
            rest = rest[k..].find("*/").map_or("", |e| &rest[k + e + 2..]);
        }
        clean.push_str(rest);
        let flat: String = clean.split_whitespace().collect();
        for token in ["--bad", "--ok", "--warn", "--accent"] {
            let needle = format!("color:var({token})");
            for (i, _) in flat.match_indices(&needle) {
                let before = flat[..i].chars().last();
                if matches!(before, Some(c) if c == '-' || c.is_ascii_alphanumeric()) {
                    continue;
                }
                // The mark is a logotype, drawn in the brand gold itself (Section 3).
                let open = flat[..i].rfind('{').unwrap_or(0);
                let selector = &flat[flat[..open].rfind(['}', ';']).map_or(0, |j| j + 1)..open];
                if token == "--accent" && LOGOS.contains(&selector) {
                    continue;
                }
                found.push(format!("{file}: {selector} color:var({token})"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a signal drawn as text in the mark's color:\n  {}",
        found.join("\n  ")
    );
}

/// The product's mark in each stylesheet, whitespace dropped: a logotype, outside the text floors.
const LOGOS: &[&str] = &[".mark", ".wordmark::before"];

/// Section 3: the brand gold is the same in both appearances, and the mark is drawn in it. Gold drawn
/// as text is `--gold-ink` - the bright gold on the dark, the ink on the light, where the gold stays in
/// the fill - and gold drawn as a line is `--gold-bright`, on the light the brand's hue deepened only
/// to the 3:1 a line needs.
#[test]
fn the_brand_gold_is_one_gold() {
    for (file, css) in
        [("viewer.css", VIEWER), ("settings.css", SETTINGS), ("site/style.css", LANDING)]
    {
        assert_eq!(
            value(&tokens(css), "--accent"),
            Some("#d9a544"),
            "{file}: the brand gold moved"
        );
    }
    for (file, css) in [("viewer.css", VIEWER), ("settings.css", SETTINGS)] {
        let light = light_tokens(css);
        assert_eq!(
            value(&light, "--accent"),
            Some("#d9a544"),
            "{file}: the light brand gold is darker"
        );
        assert_eq!(
            value(&light, "--gold-ink"),
            value(&light, "--ink"),
            "{file}: light gold text is not the ink"
        );
        let line = value(&light, "--gold-bright").expect("a light line gold");
        for ground in ["--surface", "--panel", "--panel-2"] {
            let ratio = contrast(line, value(&light, ground).expect("a light ground"));
            assert!(ratio >= 3.0, "{file}: the light gold line is {ratio:.2}:1 on {ground}");
        }
    }
    let viewer: String = VIEWER.split_whitespace().collect();
    assert!(
        viewer.contains(".mark{display:flex;color:var(--accent);"),
        "the viewer's mark is not the brand gold"
    );
    let site: String = LANDING.split_whitespace().collect();
    assert!(
        site.contains("content:\"*\";color:var(--accent);"),
        "the landing's mark is not the brand gold"
    );
}

/// The rules of a stylesheet's `@media (prefers-contrast: more)` block, whitespace dropped.
fn more_contrast(css: &str) -> String {
    let at = css
        .find("@media (prefers-contrast: more) {")
        .expect("an Increase Contrast block");
    let body = &css[at..];
    let mut depth = 0usize;
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body[..i].split_whitespace().collect();
                }
            }
            _ => {}
        }
    }
    panic!("the Increase Contrast block does not close")
}

/// U23: under Increase Contrast every text token takes the next stronger one, every control, chip and
/// field draws an edge, and the focus ring is 3 pixels; under forced colors the map redraws in the
/// system's colors and widens the human-confirmed ring.
#[test]
fn increase_contrast_and_forced_colors_are_heard() {
    for (file, css) in
        [("viewer.css", VIEWER), ("settings.css", SETTINGS), ("site/style.css", LANDING)]
    {
        let set = tokens(css);
        let block = more_contrast(css);
        let ink = value(&set, "--ink").expect("--ink");
        let ink2 = value(&set, "--ink2").expect("--ink2");
        assert!(block.contains(&format!("--ink2:{ink}")), "{file}: --ink2 does not take --ink");
        assert!(
            block.contains(&format!("--muted:{ink2}")),
            "{file}: --muted does not take --ink2"
        );
        assert!(block.contains("outline-width:3px"), "{file}: the focus ring stays 2 pixels");
        assert!(block.contains("box-shadow:inset0001px"), "{file}: controls draw no edge");
    }
    let script = include_str!("../assets/viewer.js");
    assert!(
        script.contains("matchMedia(\"(forced-colors: active)\")"),
        "the map does not hear forced colors"
    );
    for system in ["CanvasText", "Canvas", "Highlight"] {
        assert!(script.contains(&format!("\"{system}\"")), "the map does not draw in {system}");
    }
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
    ("#detail .rchip.past", "a superseded relation, outlined"),
    (".tsw.t-unverified", "the Trust legend's swatch of the unverified ring"),
    (".tsw.t-host_signed", "the Trust legend's swatch of the host-signed ring"),
    (
        ".tsw.t-human_confirmed",
        "the Trust legend's swatch of the human-confirmed ring",
    ),
    ("#loader .spin", "a control's own glyph: the loader's spinner"),
    ("#mapwin", "paints the frame around the map's window: a surface, not a line"),
    (
        ":where(button, input, select, textarea, summary)",
        "under Increase Contrast, the edge of a control, a chip and a field (Section 10)",
    ),
];
const SETTINGS_LINES: &[(&str, &str)] = &[(
    ":where(button, input, select, textarea, summary, .switch .track)",
    "under Increase Contrast, the edge of a control, a chip and a field (Section 10)",
)];
const LANDING_LINES: &[(&str, &str)] = &[
    (
        ":where(button, input, select, textarea, summary)",
        "under Increase Contrast, the edge of a control (Section 10)",
    ),
    (
        ".cell.o",
        "an owed clause on the principles ledger, an empty cell outlined in --muted",
    ),
    (
        ".sky .col[aria-current=\"true\"]",
        "the chosen column of the principles ledger, outlined in --gold-bright",
    ),
];

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
/// the viewer's script sets a border only on the boundaries' legend swatches, which show the outlines
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
