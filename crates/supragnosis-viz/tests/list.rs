//! docs/design-system.md Section 6 and U24: the map's window offers List, a table of the entities the
//! map draws, so a screen reader and anyone who cannot tell sixteen colors apart can read the map.

const HTML: &str = include_str!("../assets/viewer.html");
const SCRIPT: &str = include_str!("../assets/viewer.js");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// U24: List is the camera toolbar's first button; its table holds the entities the map draws in the
/// scope and its filters, sorted by type and name, each row naming the entity, its type, its trust by
/// the legend's name, its relations and, across workspaces, its workspace, and selecting its node as
/// the map does; Esc gives the map back; and the canvas's name says what it draws and that List holds it.
#[test]
fn the_map_has_a_list() {
    let hud = &HTML[HTML.find("<div id=\"hud\">").expect("the camera toolbar")..];
    let first = &hud[hud.find("<button").expect("a button")..];
    assert!(
        first.starts_with("<button id=\"listBtn\"")
            && first[..first.find('>').unwrap()].contains("aria-pressed=\"false\"")
            && first[..first.find('>').unwrap()].contains("aria-controls=\"listwin\""),
        "List is not the camera toolbar's first button"
    );
    assert!(HTML.contains("id=\"listwin\""), "there is no table to show");
    let rows = function("listRows");
    assert!(rows.contains("!typeOff.has(n.type)"), "List shows types the filters hide");
    assert!(
        rows.contains("a.type.localeCompare(b.type) || a.name.localeCompare(b.name)"),
        "List is not sorted by type and name"
    );
    let render = function("renderList");
    for part in [
        "Name",
        "Type",
        "Trust",
        "Relations",
        "Workspace",
        "tierName(n.trust_tier)",
        "n.degree",
        "scope=\"col\"",
    ] {
        assert!(render.contains(part), "List does not show {part}");
    }
    assert!(
        render.contains("focus = n; renderDetail(n); focusView(n);"),
        "a row does not select its node as the map does"
    );
    assert!(
        function("setList").contains("aria-pressed"),
        "List does not say whether it is on"
    );
    assert!(
        SCRIPT.contains("e.key === \"Escape\") { e.stopPropagation(); setList(false);"),
        "Esc does not give the map back"
    );
    assert!(
        function("renderLegend").contains("renderList();"),
        "List does not follow the filters"
    );
    assert!(
        function("showStats").contains("List shows them as a table."),
        "the canvas does not say what it draws and where else it is"
    );
}
