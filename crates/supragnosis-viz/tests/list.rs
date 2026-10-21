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

/// U24: Map and List are one tab list, the first control after Follow (design-system.md Sections
/// 29.4 and 29.11); List's table holds the entities the map draws in the scope, its filters and the
/// working set, starting by type and name and sorted by any column, each row naming the entity, its
/// type, its trust by the legend's name, its relations and, across workspaces, its workspace, marked
/// when contested, and selecting its node as the map does; Esc gives the map back; and the canvas's
/// name says what it draws and that List holds it.
#[test]
fn the_map_has_a_list() {
    let hud = &HTML[HTML.find("<div id=\"hud\">").expect("the camera toolbar")..];
    let follow = &hud[hud.find("<button").expect("a button")..];
    assert!(
        follow.starts_with("<button id=\"followBtn\""),
        "Follow does not lead the toolbar"
    );
    let rest = &follow[follow.find("</button>").expect("Follow closes")..];
    let tabs = rest.trim_start_matches("</button>").trim_start();
    let tabs = &tabs[tabs.find('<').unwrap()..];
    assert!(
        tabs.starts_with("<!--") || tabs.starts_with("<div id=\"mapList\" role=\"tablist\""),
        "Map | List is not the first control after Follow"
    );
    let list = &tabs[tabs.find("<div id=\"mapList\" role=\"tablist\"").expect("the tab list")..];
    let list = &list[..list.find("</div>").unwrap()];
    for tab in [
        "id=\"mapTab\" type=\"button\" role=\"tab\"",
        "id=\"listBtn\" type=\"button\" role=\"tab\"",
        "aria-controls=\"c\"",
        "aria-controls=\"listwin\"",
        "i-waypoints",
    ] {
        assert!(list.contains(tab), "the tab list lacks {tab}");
    }
    assert!(HTML.contains("id=\"listwin\""), "there is no table to show");
    let rows = function("listRows");
    assert!(rows.contains("!typeOff.has(n.type)"), "List shows types the filters hide");
    assert!(
        rows.contains("!workSet || workSet.ids.has(n.id)"),
        "List does not follow the working set"
    );
    assert!(
        SCRIPT.contains("let listSort = { key: \"type\", dir: 1 };")
            && SCRIPT.contains(
                "type: (a, b) => a.type.localeCompare(b.type) || a.name.localeCompare(b.name),"
            ),
        "List does not start by type and name"
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
        "listHead(",
        "aria-label=\"Two sources disagree\"",
    ] {
        assert!(render.contains(part), "List does not show {part}");
    }
    assert!(
        function("listHead").contains("scope=\"col\"")
            && function("listHead").contains("aria-sort="),
        "a column head is not a sorting button that says which way"
    );
    assert!(render.contains("select(n);"), "a row does not select its node as the map does");
    assert!(
        function("setList")
            .contains("tab.setAttribute(\"aria-selected\", sel ? \"true\" : \"false\")"),
        "the tabs do not say which view shows"
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
