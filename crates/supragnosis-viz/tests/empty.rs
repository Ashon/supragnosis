//! docs/design-system.md Section 29.10 (step 48): an empty scope says so in one voice and keeps no
//! idle controls, a store with no entry leads back to Setup in the app, and the default is called by
//! its name. Source tripwires over the viewer's script, markup and stylesheet.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");
const CSS: &str = include_str!("../assets/viewer.css");

/// The body of a top-level function of the viewer's script, async or not.
fn function(name: &str) -> &'static str {
    let at = [format!("\nfunction {name}("), format!("\nasync function {name}(")]
        .iter()
        .find_map(|head| SCRIPT.find(head.as_str()))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at + 1..at + 1 + SCRIPT[at + 1..].find("\n}\n").expect("it closes")]
}

/// While the scope holds no entity the type and trust sections hide, Definitions shows only with
/// one, and zoom, Fit and List stay focusable but aria-disabled, described by the empty map's
/// sentence, and act on nothing. The canvas's name says the scope holds nothing, without List.
#[test]
fn an_empty_scope_keeps_no_idle_controls() {
    for id in ["legendNodes", "legendEdges", "legendTrust"] {
        assert!(
            PAGE.contains(&format!("id=\"{id}\" data-needs=\"entities\"")),
            "{id} hides with no entity"
        );
    }
    assert_eq!(
        PAGE.matches("class=\"rsec\" data-needs=\"entities\"").count(),
        3,
        "their headings too"
    );
    assert!(CSS.contains(
        "body.scope-empty [data-needs=\"entities\"], body.scope-empty #defsBox.none { display:none !important; }"
    ));
    assert!(
        function("renderGlossary").contains("classList.toggle(\"none\", !glossaryTypes.length)")
    );
    let idle = function("idleWhileEmpty");
    for part in [
        "classList.toggle(\"scope-empty\", empty)",
        "setAttribute(\"aria-disabled\", \"true\")",
        "const why = empty ? \"emptyTitle\"",
        "b.setAttribute(\"aria-describedby\", why)",
    ] {
        assert!(idle.contains(part), "idleWhileEmpty lacks {part}");
    }
    assert!(SCRIPT.contains("const IDLE_WHEN_EMPTY = [\"zin\", \"zout\", \"fit\", \"listBtn\"];"));
    for handler in [
        "getElementById(\"zin\").onclick = e => { if (idle(e)) return;",
        "getElementById(\"zout\").onclick = e => { if (idle(e)) return;",
        "getElementById(\"fit\").onclick = e => {\n  if (idle(e)) return;",
        "listBtn.onclick = e => { if (idle(e)) return;",
    ] {
        assert!(SCRIPT.contains(handler), "an idle button still acts: {handler}");
    }
    assert!(
        PAGE.contains("<div class=\"e-title\" id=\"emptyTitle\">"),
        "the description exists"
    );
    assert!(function("refreshEmpty").contains("kids[0].id = \"emptyTitle\""));
    let stats = function("showStats");
    assert!(
        stats.contains("\": nothing here yet.\"") && stats.contains("the Log holds its entries.")
    );
}

/// The Log, the strip and the canvas say "Nothing here yet."; entries that name no entity are not
/// an empty scope (P5); and no reader's line names a tool.
#[test]
fn the_empty_map_speaks_in_one_voice() {
    let log = function("renderLog");
    assert!(
        log.contains("\"Nothing here yet. What an AI app remembers\""),
        "the Log's empty line"
    );
    assert!(function("renderStrip").contains("<span class=\"t\">Nothing here yet.</span>"));
    let empty = function("refreshEmpty");
    assert!(empty.contains("line(\"Nothing here yet.\""));
    assert!(empty.contains("\"Nothing on the map yet.\"") && empty.contains("the Log holds them."));
    assert!(
        empty.contains("scopeEntries = await anyEntry(scopeNow)"),
        "the scope's entries are read"
    );
    for gone in ["observe knowledge", "Nothing observed"] {
        assert!(!SCRIPT.contains(gone) && !PAGE.contains(gone), "the viewer still says {gone}");
    }
}

/// In the app, on the owner's console, a store with no entry offers Open Setup, carrying the way
/// back; a browser, where the shell gives no address, and the read tier show no button, and a
/// store that could not be read is not empty.
#[test]
fn an_empty_store_leads_back_to_setup_in_the_app() {
    let empty = function("refreshEmpty");
    assert!(empty
        .contains("if (window.supragnosisSetup && surfaceKind === \"local\" && !storeEntries) {"));
    assert!(
        empty.contains("window.supragnosisSetup + \"?from=\""),
        "Open Setup carries the way back"
    );
    assert!(empty.contains("\"Open Setup\""));
    assert!(
        empty.contains("catch (_) { scopeEntries = false; storeEntries = true; }"),
        "unknown is not empty (P5): a failed read offers no Setup"
    );
}

/// The scope addressed as "" is called by its name, the workspace /api/surface says an omitted one
/// resolves to, wherever the page names a scope; "default" only until the surface has answered.
#[test]
fn the_default_is_called_by_its_name() {
    assert!(function("loadSurface").contains("defaultWs = String(s.default_workspace)"));
    assert!(SCRIPT.contains("function defaultName() { return defaultWs || \"default\"; }"));
    let code: Vec<&str> = SCRIPT
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains("\"default\""))
        .collect();
    assert_eq!(
        code,
        ["function defaultName() { return defaultWs || \"default\"; }"],
        "the word default is written only as the name's fallback"
    );
    assert!(function("statsScope").contains("cur || defaultName()"));
    assert!(function("syncSearchScope").contains("\"Search \" + defaultName()"));
    let chips = function("loadWorkspaces");
    assert!(chips.contains("mk(defaultName(), \"\")"));
    assert!(chips.contains("\"Showing all workspaces - \" + defaultName() + \" is empty\""));
}
