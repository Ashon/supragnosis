//! docs/design-system.md Section 29.12 (step 50a): the map's keys and its View. Source tripwires over
//! the viewer's script, markup and stylesheet.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
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

/// Section 29.18: on a phone a finger moves the map - one pans, a long press moves an entity, two
/// pinch, a double tap on the ground zooms - without the browser's mouse events after it; the page
/// opens on the map and remembers no panel there; the inspector's sheet has two heights; and the
/// camera buttons sit at the map's foot above the open sheet.
#[test]
fn a_finger_moves_the_map_on_a_phone() {
    let touch = &SCRIPT[SCRIPT
        .find("// --- Touch (docs/design-system.md Section 29.18)")
        .expect("the touch section")..];
    let touch = &touch[..touch.find("// --- Search").expect("it ends")];
    for part in [
        "if (ev.pointerType !== \"touch\") return;\n  ev.preventDefault();",
        "TOUCH_SLOP = 10, TOUCH_REACH = 22, HOLD_MS = 500, DOUBLE_MS = 300",
        "if (settling || touches.size >= 2) return;   // a third finger is ignored",
        "if (dbl && !n) { zoomAt(tap.x, tap.y, 2);",
        "if (dbl && n && focus === n) return;",
        "if (cancelled || !tap || tap.moved || settling) return;",
        "cam.s = camT.s = s;",
        "canvas.addEventListener(\"pointercancel\"",
        "canvas.classList.toggle(\"pagezoom\"",
    ] {
        assert!(touch.contains(part), "the touch handling does not hold {part}");
    }
    assert!(
        CSS.contains("#c { touch-action:none; }")
            && CSS.contains("#c.pagezoom { touch-action:pinch-zoom; }")
    );
    let phone = function("isPhone");
    assert!(
        phone.contains("innerWidth < 760")
            && phone.contains("Math.min(innerWidth, innerHeight) < 600")
    );
    assert!(
        function("applyDock").contains("if (remember && !isPhone())"),
        "a phone's panel is remembered"
    );
    assert!(
        SCRIPT.contains("if (isPhone()) keptL = keptR = false;"),
        "a phone opens on the panels"
    );
    assert!(
        function("frameBox").contains(
            "if (innerWidth < 760 || hb.top > innerHeight / 2) B = Math.min(B, hb.top - 8);"
        ),
        "framing reads a phone's toolbar as at the top"
    );
    for rule in [
        "max-height:33dvh",
        "max-height:min(80dvh, calc(100dvh - 57px - var(--bar) - var(--hud-h, 44px) - 32px))",
        "bottom:calc(var(--foot, calc(var(--bar) + 8px)) + 8px)",
        "#hud { flex-wrap:wrap-reverse;",
    ] {
        assert!(CSS.contains(rule), "the phone layout does not hold {rule}");
    }
    let foot = function("placeFoot");
    assert!(
        foot.contains("el.offsetTop") && foot.contains("--hud-h"),
        "the toolbar does not follow the sheet"
    );
}

/// design-system.md Section 29.24: what clears the camera toolbar reads its size - the floating
/// panels from 760 to 1200 pixels, the minimap, the View popover and the strip - so a finger's
/// larger toolbar and a larger status bar are cleared as a mouse's are.
#[test]
fn a_panel_on_a_tablet_ends_above_the_camera() {
    let hud = &PAGE[PAGE.find("id=\"hud\"").expect("the toolbar")..];
    let hud = &hud[..hud.find("\n</div>").expect("it closes")];
    let buttons = hud.matches("<button").count();
    assert!(
        CSS.contains(&format!("--hud-h:38px; --hud-w:{}px;", buttons * 32 + 6)),
        "the toolbar's tokens are not its {buttons} 32-pixel buttons and padding"
    );
    assert!(
        CSS.contains(&format!("--bar:2.75rem; --hud-h:50px; --hud-w:{}px;", buttons * 44 + 6)),
        "a finger's toolbar is not its {buttons} 44-pixel buttons"
    );
    let band = &CSS[CSS.find("@media (max-width: 1199px)").expect("the band")..];
    let band = &band[..band.find("\n  }\n").expect("it closes")];
    for part in [
        "max-height:calc(100dvh - 61px - var(--bar) - var(--hud-h) - 24px)",
        "bottom:calc(var(--bar) + var(--hud-h) + 24px)",
    ] {
        assert!(band.contains(part), "a floating panel does not clear the toolbar: {part}");
    }
    for stale in ["bottom:90px", "151px", "var(--bar) + 62px", "- 200px));"] {
        assert!(!CSS.contains(stale), "a mouse-only sum is left: {stale}");
    }
    assert!(
        CSS.contains("calc(100vw - var(--win-l) - var(--win-r) - var(--hud-w) - 48px)"),
        "the strip"
    );
}

/// Section 29.30 (step 64): a pinch zooms as far as the fingers spread. The wheel keeps its notch
/// step; a `ctrlKey` wheel - a trackpad pinch as Chrome and Firefox send it - scales by its delta
/// through the touch pinch's camera move, at most 25% either way, and is not taken while a WebKit
/// gesture runs; the gesture events scale by the ratio of the scale to the last, and keep the
/// page's own zoom out.
#[test]
fn a_pinch_zooms_as_far_as_the_fingers_spread() {
    let at = SCRIPT.find("canvas.addEventListener(\"wheel\"").expect("the wheel handler");
    let wheel = &SCRIPT[at..at + SCRIPT[at..].find("}, { passive: false });").expect("it closes")];
    assert!(
        wheel.contains("zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);"),
        "a mouse notch keeps its step"
    );
    let pinch = &wheel[wheel.find("if (ev.ctrlKey) {").expect("the pinch branch")..];
    assert!(pinch.contains("if (gestureScale !== null) return;"), "never counted twice");
    assert!(
        pinch.contains("Math.min(1.25, Math.max(0.8, Math.exp(-dy / 100)))")
            && pinch.contains("pinchTo(ev.clientX, ev.clientY,"),
        "scaled by the delta through the touch pinch's camera move"
    );
    for (event, does) in [
        ("gesturestart", "gestureScale = ev.scale || 1"),
        ("gesturechange", "pinchTo(ev.clientX, ev.clientY, ev.scale / gestureScale)"),
        ("gestureend", "gestureScale = null"),
    ] {
        let at = SCRIPT.find(&format!("canvas.addEventListener(\"{event}\"")).expect(event);
        let body = &SCRIPT[at..at + SCRIPT[at..].find("});").expect("it closes")];
        assert!(body.contains("ev.preventDefault()"), "{event} keeps the page's own zoom out");
        assert!(body.contains(does), "{event}: {does}");
    }
}
