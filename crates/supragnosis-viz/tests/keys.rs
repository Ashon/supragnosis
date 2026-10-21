//! docs/design-system.md Section 29.12 (step 50a): the map's keys and its View. Source tripwires over
//! the viewer's script, markup and stylesheet.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");
const CSS: &str = include_str!("../assets/viewer.css");

/// The body of a top-level function of the viewer's script.
fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("\nfunction {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at + 1..at + 1 + SCRIPT[at + 1..].find("\n}\n").expect("it closes")]
}

/// The single keys act on the Graph page without Cmd, Ctrl or Option, outside a text field and a
/// dialog, through their buttons so an idle button keeps its key idle; a letter is matched by its
/// key's place only under a non-Latin input source; and a control names its key only while the keys
/// are on.
#[test]
fn the_map_has_keys_that_name_themselves() {
    for part in [
        "if (dialogOpen()) return;",
        "const k = /^[\\x20-\\x7e]$/.test(e.key) ? e.key : e.code === \"KeyB\" ? \"b\" : e.code === \"KeyL\" ? \"l\" : e.key;",
        "const onMap = !reviewOpen() && !obscardEl.classList.contains(\"on\");",
        "b.getAttribute(\"aria-disabled\") !== \"true\") b.click();",
        "else if (k === \"=\" || k === \"+\") press(\"zin\");",
        "else if (k === \"-\") press(\"zout\");",
        "else if (k === \"0\") press(\"fit\");",
        "else if (k === \"l\") press(listOn ? \"mapTab\" : \"listBtn\");",
        "else if (k === \"?\") openKeys();",
        "else if (k === \"b\") cycleBoundaries();",
    ] {
        assert!(SCRIPT.contains(part), "the keys lack {part}");
    }
    let names = function("renderKeyNames");
    assert!(names.contains("const k = key => singleKeys ? key : null;"));
    assert!(
        names.contains("name(\"zin\", \"Zoom in\", \"=\")")
            && names.contains("name(\"listBtn\", \"List\", \"l\")")
    );
    assert!(
        names.contains(".slashkey, #viewpop .keyhint"),
        "the key badges stay while the keys are off"
    );
    assert!(
        function("mapInUse").contains("keysDlg\").open"),
        "Follow does not wait for the Keys dialog"
    );
}

/// The Keys dialog lists the Graph page's keys and holds the switch that turns the single keys off
/// (the author's decision); View opens it.
#[test]
fn the_keys_dialog_holds_the_switch() {
    let dialog = &PAGE[PAGE
        .find("<dialog id=\"keysDlg\" aria-labelledby=\"keysTitle\">")
        .expect("the dialog")..];
    let dialog = &dialog[..dialog.find("</dialog>").unwrap()];
    for part in [
        "<h3>Find</h3>",
        "<h3>Panels</h3>",
        "<h3>Map</h3>",
        "<h3>Selection</h3>",
        "<h3>Lists</h3>",
        "Cmd+Enter",
        "id=\"keysBtn\"",
    ] {
        assert!(dialog.contains(part), "the Keys dialog lacks {part}");
    }
    assert!(dialog
        .contains("on the Graph page, outside a text field, while no dialog or card is open."));
    assert!(PAGE.contains("<button id=\"keysOpen\" type=\"button\">Keys...</button>"));
    assert!(
        function("openKeys").contains("keysDlg.showModal()"),
        "the dialog does not hold focus"
    );
}

/// View holds what changes the picture: no Keys group and no Reload; Footprint and Pulses under More,
/// whose summary names one that is off; Arrows and History idle while Relations is off.
#[test]
fn view_holds_what_changes_the_picture() {
    let view = &PAGE[PAGE.find("<div id=\"viewpop\"").expect("View")..];
    let view = &view[..view.find("<dialog").unwrap()];
    assert!(
        !view.contains("id=\"reload\"") && !view.contains(">Keys</span>"),
        "View still holds the keyboard and Reload"
    );
    let more = &view[view.find("<details id=\"moreView\"").expect("More")..];
    assert!(more.contains("id=\"footBtn\"") && more.contains("id=\"pulseBtn\""));
    assert!(
        more.contains("id=\"peersBtn\"") && more.contains("id=\"resetLayout\""),
        "Peers and Reset layout are not under More"
    );
    assert!(!view.contains(">Layout</span>"), "View still has a Layout group");
    let deps = function("renderShowDeps");
    assert!(deps.contains("[\"arrowsBtn\", \"histBtn\"]") && deps.contains("\"dShownWithRel\""));
    assert!(deps.contains("\"More - \" + off.join(\" and \") + \" off\""));
    assert!(PAGE.contains(">Shown with relations.</p>"));
}

/// b only draws: the workspaces cohere whatever Boundaries draws; one predicate over the drawn
/// entities decides the outline, the choice's state, b's cycle and the key's row; b's notice is `ok`
/// and replaces the last; and Boundaries is stored with View's choices.
#[test]
fn boundaries_only_draw() {
    assert!(
        SCRIPT.contains("hullForce, clusterMode, wsMode: true,"),
        "the layout still follows Boundaries"
    );
    assert!(
        function("boundaryGroups").contains("!typeOff.has(n.type)"),
        "hidden entities are outlined"
    );
    assert!(function("outlines").contains("boundaryGroups(mode).size >= 2"));
    let cycle = function("cycleBoundaries");
    assert!(cycle.contains("\"Boundaries: nothing to outline here\""));
    assert!(
        cycle.contains("if (boundaryNote) dismiss(boundaryNote);")
            && cycle.contains("toast(\"ok\", said)")
    );
    assert!(!cycle.contains("wake("), "b still moves the layout");
    assert!(SCRIPT.contains("\"Fewer than two machines are recorded here - nothing to outline\""));
    assert!(SCRIPT
        .contains("boundaryMode === \"workspace\" && outlines(\"workspace\")) row(\"mk-outline\""));
    assert!(
        function("radioGroup").contains("getAttribute(\"aria-disabled\") === \"true\""),
        "the arrows choose a disabled choice"
    );
    assert!(
        SCRIPT.contains("boundaries: boundaryMode };")
            || SCRIPT.contains("boundaries: boundaryMode }")
    );
}

/// Retry follows a stale line as its own button, keeps the poll's reason and says when it tried; a
/// read that succeeds hides it.
#[test]
fn retry_follows_a_stale_line() {
    assert!(
        PAGE.contains("<button type=\"button\" id=\"retry\" class=\"spill\" hidden>Retry</button>")
    );
    let late = function("sayLate");
    assert!(
        late.contains("line + \" (tried \" + retryAsked + \")\"")
            && late.contains("r.hidden = false")
    );
    assert!(SCRIPT.contains("retryAsked = null;\n  { const r = document.getElementById(\"retry\"); if (r) r.hidden = true; }"));
}

/// The minimap draws while zoomed in, with room between showing and leaving, once painted, fading
/// unless motion is reduced; and every framing leaves out the camera toolbar's box.
#[test]
fn the_minimap_waits_and_framing_keeps_the_toolbar_box() {
    let want = function("miniWanted");
    assert!(want.contains("miniShown = miniShown ? held <= 0.9 : held < 0.8;"));
    assert!(want.contains("nodeRadius(n)"), "a single entity has no area");
    assert!(SCRIPT
        .contains("miniEl.classList.toggle(\"want\", on && !!miniStill.c && miniWanted(src));"));
    assert!(CSS.contains(
        "@media (prefers-reduced-motion: reduce) { #minimap, #minimap.want { transition:none; } }"
    ));
    let frame = function("frameBox");
    assert!(
        frame.contains("getElementById(\"hud\")")
            && frame.contains("B = Math.min(B, hb.top - 8)")
            && frame.contains("T = Math.max(T, hb.bottom + 8)")
    );
}
