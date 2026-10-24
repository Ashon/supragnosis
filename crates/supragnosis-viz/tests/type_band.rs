//! docs/design-system.md U16: a type's color is never a signal's. The viewer draws a type from a
//! band of sixteen measured colors, and this file measures them: hue against the signal tokens,
//! lightness, contrast on the ground, and distance from each other and from what else the map draws.
//! The landing's stage and the principles page copy the band, and their own tripwires hold each copy
//! to the viewer's (landing.rs, principles.rs).

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
const CSS: &str = include_str!("../assets/viewer.css");

/// The tokens that mean something on the map: human confirmed, healthy, contested.
const SIGNALS: &[&str] = &["--accent", "--gold-bright", "--ok", "--bad"];
/// The neutrals the map draws beside a type: text, labels and the relation line.
const NEUTRALS: &[&str] = &["--ink", "--ink2", "--muted"];

/// A token's value in the viewer's `:root` block, which declares several on a line.
fn token(name: &str) -> String {
    let root = &CSS[CSS.find(":root {").expect("a :root block")..];
    let mut root = root[..root.find('}').expect("the :root block closes")].to_string();
    while let Some(at) = root.find("/*") {
        let end = at + root[at..].find("*/").expect("the comment closes") + 2;
        root.replace_range(at..end, "");
    }
    let decl = root
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("viewer.css does not declare {name}"));
    decl[name.len() + 1..].trim().to_lowercase()
}

/// The `"#rrggbb"` values of a `const NAME = [ ... ];` block of the viewer's script.
fn hex_list(name: &str) -> Vec<String> {
    let at = SCRIPT.find(&format!("const {name} = [")).unwrap_or_else(|| panic!("no {name}"));
    let body = &SCRIPT[at..at + SCRIPT[at..].find("];").expect("it closes")];
    body.split('"')
        .filter(|p| p.len() == 7 && p.starts_with('#'))
        .map(str::to_lowercase)
        .collect()
}

fn viewer_const(name: &str) -> String {
    let at = SCRIPT.find(&format!("{name} = \"#")).unwrap_or_else(|| panic!("no {name}"));
    SCRIPT[at + name.len() + 4..at + name.len() + 11].to_lowercase()
}

fn rgb(hex: &str) -> [f64; 3] {
    let hex = hex.strip_prefix('#').expect("a #rrggbb color");
    let c = |i: usize| {
        let v = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).expect("hex")) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [c(0), c(2), c(4)]
}

/// sRGB to OKLab, Ottosson's matrices.
fn oklab(hex: &str) -> [f64; 3] {
    let [r, g, b] = rgb(hex);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// OKLCH lightness and hue in degrees.
fn lightness_hue(hex: &str) -> (f64, f64) {
    let [l, a, b] = oklab(hex);
    (l, b.atan2(a).to_degrees().rem_euclid(360.0))
}

fn distance(x: &str, y: &str) -> f64 {
    let (p, q) = (oklab(x), oklab(y));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

fn contrast(a: &str, b: &str) -> f64 {
    let lum = |h: &str| {
        let [r, g, b] = rgb(h);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn hue_gap(a: f64, b: f64) -> f64 {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs()
}

#[test]
fn the_type_band_is_never_a_signal() {
    let band = hex_list("TYPE_BAND");
    assert_eq!(band.len(), 16, "the band is eight hues in two tones");
    let (first, second) = band.split_at(8);
    let signals: Vec<(String, String)> =
        SIGNALS.iter().map(|t| (t.to_string(), token(t))).collect();
    let mut neutrals: Vec<(String, String)> =
        NEUTRALS.iter().map(|t| (t.to_string(), token(t))).collect();
    neutrals.push(("EDGE_NEUTRAL".into(), viewer_const("EDGE_NEUTRAL")));
    let ground = token("--surface");

    let mut bad = Vec::new();
    for c in &band {
        let (l, h) = lightness_hue(c);
        for (name, s) in &signals {
            let gap = hue_gap(h, lightness_hue(s).1);
            if gap < 25.0 {
                bad.push(format!("{c} is {gap:.1} degrees of hue from {name}"));
            }
        }
        if l < 0.62 {
            bad.push(format!("{c} has OKLCH lightness {l:.3}, under 0.62"));
        }
        let ratio = contrast(c, &ground);
        if ratio < 5.5 {
            bad.push(format!("{c} is {ratio:.2}:1 on --surface, under 5.5:1"));
        }
    }
    for (i, a) in first.iter().enumerate() {
        for b in &first[i + 1..] {
            let d = distance(a, b);
            if d < 0.09 {
                bad.push(format!("first tone {a} and {b} are {d:.3} apart, under 0.09"));
            }
        }
        for (name, s) in &signals {
            let d = distance(a, s);
            if d < 0.11 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.11"));
            }
        }
        for (name, n) in &neutrals {
            let d = distance(a, n);
            if d < 0.10 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.10"));
            }
        }
    }
    for a in second {
        for b in band.iter().filter(|b| *b != a) {
            let d = distance(a, b);
            if d < 0.06 {
                bad.push(format!("second tone {a} and {b} are {d:.3} apart, under 0.06"));
            }
        }
        for (name, s) in &signals {
            let d = distance(a, s);
            if d < 0.08 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.08"));
            }
        }
    }
    assert!(bad.is_empty(), "the type band:\n  {}", bad.join("\n  "));
}

/// The band is the only source of a type's color, the boundaries' and the peers' come from it too,
/// relation kinds draw none, and a region's hue stays on the band's arcs.
#[test]
fn every_category_color_comes_from_the_band() {
    assert!(!SCRIPT.contains("hsl("), "the viewer computes a color outside the band");
    assert!(!SCRIPT.contains("catColor"), "the generated palette is still in the viewer");
    assert!(!SCRIPT.contains("edgeTypeColor"), "a relation kind is given a color");
    let stable = &SCRIPT[SCRIPT.find("function stableColors(").expect("stableColors")..];
    let stable = &stable[..stable.find("\n}\n").expect("it closes")];
    assert!(stable.contains("out[t] = TYPE_BAND[slot];"), "a type's color is not the band's");
    for f in ["function hyperColor(", "function peerColor("] {
        let body = &SCRIPT[SCRIPT.find(f).unwrap_or_else(|| panic!("no {f}"))..];
        let body = &body[..body.find("\n}\n").expect("it closes")];
        assert!(
            body.contains("TYPE_BAND[h % TYPE_BAND.length]"),
            "{f} does not draw from the band"
        );
    }
    // The relation legend draws the neutral line, and an active edge the brighter neutral.
    assert!(
        SCRIPT.contains("fill(legendEdgesEl, edgeKeys, () => EDGE_NEUTRAL, edgeTypeOff, true);")
    );
    assert!(SCRIPT.contains("(hot || edgeTypeHl === e.type) ? EDGE_ACTIVE : EDGE_NEUTRAL;"));
    assert_eq!(viewer_const("EDGE_ACTIVE"), token("--ink2"), "an active edge is --ink2");

    // A region's hue: the golden angle laid along the band's 212 degrees, 46 of green from 109 and
    // the rest from 205. Every value it can take is checked against the signals here.
    let region = &SCRIPT[SCRIPT.find("function regionHue(").expect("regionHue")..];
    let region = &region[..region.find("\n}\n").expect("it closes")];
    assert!(
        region.contains("% 212;")
            && region.contains("return t < 46 ? 109 + t : (205 + t - 46) % 360;")
    );
    let signal_hues: Vec<f64> = SIGNALS.iter().map(|t| lightness_hue(&token(t)).1).collect();
    for slot in 0..512u32 {
        let t = (f64::from(slot) * 137.508) % 212.0;
        let h = if t < 46.0 { 109.0 + t } else { (205.0 + t - 46.0) % 360.0 };
        for s in &signal_hues {
            assert!(
                hue_gap(h, *s) >= 24.0,
                "region hue {h:.1} sits {:.1} from a signal",
                hue_gap(h, *s)
            );
        }
    }
}

/// A token's value in the viewer's light appearance: the `:root` inside its
/// `@media (prefers-color-scheme: light)` block.
fn light_token(name: &str) -> String {
    let at = CSS.find("@media (prefers-color-scheme: light) {").expect("a light appearance");
    let root = &CSS[at..];
    let root = &root[root.find(":root {").expect("its :root")..];
    let root = &root[..root.find('}').expect("its :root closes")];
    let decl = root
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with(&format!("{name}:")))
        .unwrap_or_else(|| panic!("the light appearance does not declare {name}"));
    decl[name.len() + 1..].trim().to_lowercase()
}

/// A value of the map's light palette in the viewer's script: `NAME: "#rrggbb"` in `PALETTE_LIGHT`.
fn light_const(name: &str) -> String {
    let at = SCRIPT.find("const PALETTE_LIGHT = {").expect("a light palette");
    let block = &SCRIPT[at..at + SCRIPT[at..].find('}').expect("it closes")];
    let key = format!("{name}: \"#");
    let i = block.find(&key).unwrap_or_else(|| panic!("PALETTE_LIGHT has no {name}"));
    block[i + key.len() - 1..i + key.len() + 6].to_lowercase()
}

/// U27 and Section 5's light band: each type keeps its slot and hue, and the light band is measured
/// by U16's rules on the light ground - a mark there is told by being darker, so every color has OKLCH
/// lightness from 0.42 to 0.67 and measures 3:1 or more on the light `--panel`.
#[test]
fn the_light_band_is_never_a_signal() {
    let dark = hex_list("TYPE_BAND");
    let band = hex_list("TYPE_BAND_LIGHT");
    assert_eq!(band.len(), 16, "the light band is eight hues in two tones");
    let (first, second) = band.split_at(8);
    let signals: Vec<(String, String)> =
        SIGNALS.iter().map(|t| (t.to_string(), light_token(t))).collect();
    let mut neutrals: Vec<(String, String)> =
        NEUTRALS.iter().map(|t| (t.to_string(), light_token(t))).collect();
    neutrals.push(("EDGE_NEUTRAL".into(), light_const("EDGE_NEUTRAL")));
    let panel = light_token("--panel");
    let mut bad = Vec::new();
    for (c, d) in band.iter().zip(&dark) {
        let (l, h) = lightness_hue(c);
        if hue_gap(h, lightness_hue(d).1) > 3.0 {
            bad.push(format!("{c} does not keep the hue of {d}"));
        }
        for (name, s) in &signals {
            let gap = hue_gap(h, lightness_hue(s).1);
            if gap < 25.0 {
                bad.push(format!("{c} is {gap:.1} degrees of hue from {name}"));
            }
        }
        if !(0.42..=0.67).contains(&l) {
            bad.push(format!("{c} has OKLCH lightness {l:.3}, outside 0.42-0.67"));
        }
        let ratio = contrast(c, &panel);
        if ratio < 3.0 {
            bad.push(format!("{c} is {ratio:.2}:1 on the light --panel, under 3:1"));
        }
    }
    for (i, a) in first.iter().enumerate() {
        for b in &first[i + 1..] {
            let d = distance(a, b);
            if d < 0.09 {
                bad.push(format!("first tone {a} and {b} are {d:.3} apart, under 0.09"));
            }
        }
        for (name, s) in &signals {
            let d = distance(a, s);
            if d < 0.11 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.11"));
            }
        }
        for (name, n) in &neutrals {
            let d = distance(a, n);
            if d < 0.10 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.10"));
            }
        }
    }
    for a in second {
        for b in band.iter().filter(|b| *b != a) {
            let d = distance(a, b);
            if d < 0.06 {
                bad.push(format!("second tone {a} and {b} are {d:.3} apart, under 0.06"));
            }
        }
        for (name, s) in &signals {
            let d = distance(a, s);
            if d < 0.08 {
                bad.push(format!("{a} is {d:.3} from {name}, under 0.08"));
            }
        }
    }
    assert!(bad.is_empty(), "the light band reads as a signal:\n  {}", bad.join("\n  "));
    assert_eq!(
        light_const("EDGE_ACTIVE"),
        light_token("--ink2"),
        "a light active edge is --ink2"
    );
    assert_eq!(
        light_const("EDGE_HI"),
        light_token("--gold-bright"),
        "a light ring is --gold-bright"
    );
    assert!(
        SCRIPT.contains("LIGHT.addEventListener(\"change\", repaint)")
            && SCRIPT.contains("[DPR, SURFACE, weight"),
        "the map does not redraw, halos and all, when the appearance changes"
    );
}
