//! docs/design-system.md U19, the part that needs no screen change: the viewer's controls say what
//! they are. View's toggles are switches whose state is set with their class, the legend's chips say
//! whether they are pressed, the workspace chips are one radio group, the page declares its language
//! and the map's canvas is named. The rest of U19 - dialogs, keys, targets, reflow - is owed until
//! Section 14 steps 13-16 meet it, and its registry row says so.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");

/// The id of every `<button ... class="tog ...">` in the page.
fn toggles() -> Vec<&'static str> {
    PAGE.match_indices("<button id=\"")
        .filter_map(|(at, _)| {
            let tag = &PAGE[at..at + PAGE[at..].find('>').expect("the tag closes")];
            let id = tag[12..].split('"').next()?;
            tag.contains("class=\"tog").then_some(id)
        })
        .collect()
}

#[test]
fn the_viewers_controls_say_what_they_are() {
    let togs = toggles();
    assert!(togs.len() >= 10, "the View toggles were read ({togs:?})");
    // Each switch's state is set where its class is, never the class alone.
    assert!(
        SCRIPT.contains("for (const b of document.querySelectorAll(\"button.tog\")) setSwitch(b, b.classList.contains(\"on\"));"),
        "the switches do not say their state when the page starts"
    );
    for id in &togs {
        for change in ["toggle", "add", "remove"] {
            assert!(
                !SCRIPT.contains(&format!("{id}.classList.{change}(\"on\"")),
                "{id} changes its class without saying its state"
            );
        }
    }
    assert!(
        !SCRIPT.contains("e.currentTarget.classList.toggle(\"on\""),
        "a View toggle changes its class without saying its state"
    );
    let switch = &SCRIPT[SCRIPT.find("function setSwitch(").expect("setSwitch")..];
    let switch = &switch[..switch.find("\n}\n").expect("it closes")];
    for part in ["\"role\", \"switch\"", "\"aria-checked\"", "classList.toggle(\"on\""] {
        assert!(switch.contains(part), "setSwitch does not set {part}");
    }

    // A legend chip filters, so it says whether it is pressed; the workspace chips choose one scope.
    assert!(
        SCRIPT.contains("setPressed(el, !offSet.has(t));"),
        "a legend chip does not say it is pressed"
    );
    assert!(
        SCRIPT.matches("radioGroup(chipBar, \"Workspace\")").count() >= 2,
        "the workspace chips are not one radio group"
    );
    let radios = &SCRIPT[SCRIPT.find("function radioGroup(").expect("radioGroup")..];
    let radios = &radios[..radios.find("\n}\n").expect("it closes")];
    for part in [
        "\"radiogroup\"",
        "\"radio\"",
        "\"aria-checked\"",
        "tabIndex",
        "ArrowRight",
        "Home",
        "End",
    ] {
        assert!(radios.contains(part), "radioGroup does not handle {part}");
    }

    // The page says its language, and the map is named.
    assert!(PAGE.contains("<html lang=\"en\">"), "the viewer does not declare its language");
    assert!(
        PAGE.contains("<canvas id=\"c\" role=\"img\" aria-label=\""),
        "the map's canvas has no name"
    );
}

/// Section 5 and 10: View holds two choices with one name each - Regions and Boundaries, each a radio
/// group of three - a switch that turns the single keys off, and no option drawn dimmed for being off.
#[test]
fn view_holds_two_choices_with_one_name_each() {
    let css = include_str!("../assets/viewer.css");
    assert!(
        !PAGE.contains("id=\"hyperBtn\"") && !PAGE.contains("id=\"regionsBtn\""),
        "hulls and all regions are still two switches"
    );
    for v in ["data-v=\"all\"", "data-v=\"selected\"", "data-v=\"off\""] {
        assert!(PAGE.contains(v), "Regions has no {v}");
    }
    assert!(
        SCRIPT.contains("radioGroup(regionsSeg, \"Regions\")"),
        "Regions is not a radio group"
    );
    assert!(
        SCRIPT.contains("radioGroup(legendBoundaryEl, \"Boundaries\")"),
        "Boundaries is not a radio group"
    );
    for label in ["\"Workspace\"", "\"Origin\"", "\"None\""] {
        assert!(SCRIPT.contains(label), "a boundary mode is not named {label}");
    }
    assert!(
        PAGE.contains("id=\"keysBtn\" class=\"tog on\""),
        "the single keys cannot be turned off"
    );
    // The single keys and their switch, in the Keys dialog since step 50a (Section 29.12).
    assert!(SCRIPT.contains(
        "const single = bare && [\"[\", \"]\", \"/\", \"b\", \"=\", \"+\", \"-\", \"0\", \"l\", \"?\"].includes(k);"
    ));
    assert!(SCRIPT.contains("if (single && !singleKeys) return;"));
    let dialog = &PAGE[PAGE.find("<dialog id=\"keysDlg\"").expect("the Keys dialog")..];
    assert!(
        dialog[..dialog.find("</dialog>").unwrap()].contains("id=\"keysBtn\""),
        "the switch is not in the Keys dialog"
    );
    assert!(
        !css.contains("button.tog { opacity"),
        "an option that is off is drawn as if disabled"
    );
}

/// docs/design-system.md U19 and Section 10: the focus ring is seen whole. A scroll container clips
/// what lies outside its padding box, so every container of the viewer's stylesheet that scrolls keeps
/// a gutter of at least 6 pixels - the widest ring, 3 under prefers-contrast, plus its offset - on
/// every side, its own or the shared gutter rule's; and the status bar, on the window's edge, draws
/// the ring inside.
#[test]
fn a_focus_ring_is_never_cut_by_a_scroll_container() {
    let css = include_str!("../assets/viewer.css");
    let gutter = ".railbody, .dock .panels, #detail .rels, .rv-list, #detail .why .whybody { padding:6px; margin:-6px; }";
    assert!(css.contains(gutter), "the shared ring gutter is gone");
    assert!(
        css.contains("#statusbar :focus-visible, .sp-row:focus-visible { outline-offset:-3px; }")
    );
    let guttered: Vec<&str> =
        gutter[..gutter.find('{').unwrap()].split(',').map(str::trim).collect();
    let px = |v: &str| v.trim_end_matches("px").parse::<f32>().unwrap_or(0.0);
    let mut cut = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].rsplit(['}', '\n']).next().unwrap_or("").trim().to_string();
        let close = rest[open..].find('}').map(|i| open + i).unwrap_or(rest.len());
        let body = &rest[open + 1..close];
        rest = &rest[close.min(rest.len() - 1) + 1..];
        let scrolls = body.split(';').any(|d| {
            let d = d.trim().replace(' ', "");
            (d.starts_with("overflow:")
                || d.starts_with("overflow-y:")
                || d.starts_with("overflow-x:"))
                && (d.ends_with("auto") || d.ends_with("scroll"))
        });
        if !scrolls || selector.starts_with('@') {
            continue;
        }
        let pad = body
            .split(';')
            .find_map(|d| d.trim().strip_prefix("padding:"))
            .map(|v| v.split_whitespace().map(px).collect::<Vec<_>>())
            .unwrap_or_default();
        let sides = match pad.len() {
            1 => vec![pad[0]; 4],
            2 => vec![pad[0], pad[1], pad[0], pad[1]],
            3 => vec![pad[0], pad[1], pad[2], pad[1]],
            4 => pad,
            _ => vec![0.0; 4],
        };
        let padded = sides.iter().all(|s| *s >= 6.0);
        for one in selector.split(',').map(str::trim) {
            if !padded && !guttered.contains(&one) {
                cut.push(one.to_string());
            }
        }
    }
    assert!(cut.is_empty(), "a scroll container cuts the focus ring: {cut:?}");
}
