//! docs/design-system.md Section 29.4, step 43: the map keeps the person's place. Source tripwires
//! over the viewer's script and page, and the app pages' way back; the app's own half - the address
//! carried in `?from=` and followed only to the viewer - is `the_way_back_goes_only_to_the_viewer` in
//! the app's tests.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");
const KIT: &str = include_str!("../../../app/assets/kit.js");
const SETTINGS: &str = include_str!("../../../app/assets/settings.js");
const WELCOME: &str = include_str!("../../../app/assets/welcome.js");
const SHELL: &str = include_str!("../../../app/assets/shell-init.js");

fn function(src: &'static str, name: &str) -> &'static str {
    let at = src
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &src[at..at + src[at..].find("\n}\n").expect("it closes")]
}

/// D6: follow waits while the person uses the map, says what happened instead, and is on the camera
/// toolbar, saying when it waits.
#[test]
fn follow_waits_while_the_map_is_in_use() {
    let busy = function(SCRIPT, "mapInUse");
    for part in [
        "!!focus",
        "listOn",
        "viewPop.classList.contains(\"on\")",
        "!searchPop.hidden",
        "obscardEl.classList.contains(\"on\")",
        "reviewOpen()",
        "performance.now() - handAt < 20000",
    ] {
        assert!(busy.contains(part), "follow does not wait for {part}");
    }
    let handle = function(SCRIPT, "handleEvent");
    assert!(handle.contains("if (follow && !peersOn && mapInUse()) noteActivity(ev, ids);"));
    assert!(
        handle.contains("refitOnReveal = follow && !mapInUse();"),
        "a sync refits a map in use"
    );
    let note = function(SCRIPT, "noteActivity");
    for said in [
        "\"An agent wrote \"",
        "\"An agent searched\"",
        "\"An agent read \"",
        "\"An agent traversed from \"",
    ] {
        assert!(note.contains(said), "{said} is not said");
    }
    assert!(note.contains("label: \"Show\""), "the notice has no Show");
    assert!(!note.contains("\"New in \""), "the notice reads like a write elsewhere");
    // The person's own moves, and only those, start the 20 seconds.
    for hook in [
        "zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);\n  personMoved();",
        "userMoved = true; showTip(null); personMoved(); return;",
        "function zoomWindow(f) { const w = mapWindow(); zoomAt((w.l + w.r)/2, (w.t + w.b)/2, f); personMoved(); }",
        "miniEl.addEventListener(\"mousedown\", ev => {\n  personMoved();",
    ] {
        assert!(SCRIPT.contains(hook), "a move of the person's does not count: {hook}");
    }
    assert!(
        !function(SCRIPT, "centerOn").contains("personMoved"),
        "follow's own move counts as the person's"
    );
    let hud = &PAGE[PAGE.find("<div id=\"hud\">").expect("the toolbar")..];
    assert!(hud.contains("<button id=\"followBtn\""), "Follow is not on the camera toolbar");
    assert!(hud.contains("i-locate"));
    let look = function(SCRIPT, "renderFollow");
    assert!(
        look.contains("\"Follow paused\"")
            && look.contains("\"i-locate-off\"")
            && look.contains("aria-pressed")
    );
}

/// D7: the address holds the scope and the selection, and an entity the scope does not hold is said
/// without its id.
#[test]
fn the_address_holds_the_place() {
    let sync = function(SCRIPT, "syncUrlWorkspace");
    assert!(
        sync.contains("url.searchParams.set(\"entity\", entity)"),
        "the selection is not in the address"
    );
    let linked = function(SCRIPT, "selectLinked");
    assert!(linked.contains("\"The linked entity is not in \" + statsScope(currentWs())"));
    assert!(
        SCRIPT.contains("let linkedEntity = new URLSearchParams(location.search).get(\"entity\");")
    );
}

/// The way back goes to the address the graph was left at, and only when it is the viewer's own.
#[test]
fn the_way_back_goes_back_to_it() {
    let back = function(KIT, "graphBack");
    assert!(
        back.contains("from.protocol === home.protocol && from.host === home.host"),
        "the way back follows any address"
    );
    for (name, src) in [("settings.js", SETTINGS), ("welcome.js", WELCOME)] {
        assert!(
            src.contains("document.getElementById(\"close\").href = graphBack();"),
            "{name}'s < Graph"
        );
        assert!(src.contains("location.href = graphBack();"), "{name}'s Esc");
    }
    assert!(
        SETTINGS.contains("href: withBack(\"welcome.html\")"),
        "Settings does not hand the way back to Setup"
    );
    assert!(
        SETTINGS.contains("after: forgetGraphLeft"),
        "a server switch keeps the other server's address"
    );
    assert!(
        SHELL.contains("to = SETTINGS + \"?from=\" + encodeURIComponent(here) + to;"),
        "the gear does not carry the viewer's address"
    );
}

/// D7: a selection history on Cmd+[ and Cmd+], merged entries followed and gone ones skipped; Esc
/// gives the camera back; a click on the empty map does not.
#[test]
fn a_selection_has_a_history_and_esc_gives_the_camera_back() {
    let select = function(SCRIPT, "select");
    assert!(select.contains("selHist.back.push(focus.id); selHist.fwd = [];"));
    assert!(select.contains("if (!focus) walkCam = "), "the walk's camera is not kept");
    let step = function(SCRIPT, "stepSelection");
    assert!(
        step.contains("nodeById(id) || mergedInto(id)"),
        "a merged entry is not followed"
    );
    assert!(step.contains("if (!n) continue;"), "a gone entry is not skipped");
    let clear = function(SCRIPT, "clearSelection");
    assert!(
        clear.contains("if (giveBack && walkCam && handAt < walkCam.at)"),
        "the camera is not given back"
    );
    assert!(SCRIPT.contains("else if (focus) clearSelection(true);"), "Esc keeps the zoom");
    assert!(SCRIPT.contains("if (n && focus !== n) select(n); else clearSelection(false);"));
    assert!(
        SCRIPT.contains("stepSelection(e.key === \"[\" ? -1 : 1);"),
        "Cmd+[ does not step back"
    );
    assert!(
        SCRIPT.contains(
            "if (e.key === \"[\" && !e.metaKey && !e.ctrlKey && !e.altKey) toggleDock(true);"
        ),
        "the single key [ still takes Cmd+["
    );
    let detail = function(SCRIPT, "renderDetail");
    assert!(
        detail.contains("class=\"hback\"") && detail.contains("class=\"hfwd\""),
        "the sheet has no Back"
    );
    assert!(
        SCRIPT.matches("focus = n; renderDetail(n); focusView(n);").count() == 0,
        "a selection skips the history"
    );
}

/// A selection a merge folds is followed; one that leaves is said (U20); and the layout can be reset.
#[test]
fn a_selection_that_leaves_is_followed_or_named() {
    let keep = function(SCRIPT, "keepSelection");
    assert!(keep.contains("was.name + \" is now \" + now.name"));
    assert!(keep.contains("was.name + \" is no longer in \" + statsScope(currentWs())"));
    assert!(function(SCRIPT, "mergedInto").contains("(n.aliases || []).includes(name)"));
    assert!(SCRIPT.contains(
        "if (focus) { if (nodes.includes(focus)) renderDetail(focus); else keepSelection(); }"
    ));
    assert!(PAGE.contains("<button id=\"resetLayout\""), "View has no Reset layout");
    assert!(function(SCRIPT, "resetLayout").contains("localStorage.removeItem(layoutScope)"));
}
