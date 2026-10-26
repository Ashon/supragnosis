//! docs/design-system.md Section 29.11 (step 49, D8): search goes somewhere, and its answer can be
//! kept on the map as the working set. Source tripwires over the viewer's script and markup.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
const PAGE: &str = include_str!("../assets/viewer.html");

/// The body of a top-level function of the viewer's script, async or not.
fn function(name: &str) -> &'static str {
    let at = [format!("\nfunction {name}("), format!("\nasync function {name}(")]
        .iter()
        .find_map(|head| SCRIPT.find(head.as_str()))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at + 1..at + 1 + SCRIPT[at + 1..].find("\n}\n").expect("it closes")]
}

/// An empty field offers the last five queries this page ran, kept in its memory and never in the
/// browser's storage; and an entity hit names its workspace in the hover card's words. That a choice
/// clears the field is held by `search_chooses_nothing_until_the_person_moves` in screens.rs.
#[test]
fn a_choice_clears_the_field_and_recent_searches_wait() {
    let note = function("noteRecent");
    assert!(note.contains("recentQueries.length = Math.min(recentQueries.length, 5)"));
    assert!(
        !note.contains("localStorage") && !note.contains("sessionStorage"),
        "recent queries are stored"
    );
    assert!(function("renderSearch").contains("<div class=\"sp-sec\">Recent</div>"));
    assert!(function("runSearch").contains("if (!ans.error) noteRecent(q);"));
    assert!(
        function("renderSearch").contains("n.type + \" in \" + n.workspace"),
        "a hit does not name its workspace as the hover card does"
    );
}

/// "Show these 13 on the map" pins the answer's drawn entities: the first row, saying where the
/// search stopped at the path's limit, the hits not drawn counted apart (P5), Cmd+Enter on a fresh
/// answer; the set dims the rest without making its members active; the status line and its pill say
/// it, the pill's Clear and Show all end it; Follow waits; a merged member is followed; a scope change
/// and, last of all, Esc end it; and Fit frames it under its own name.
#[test]
fn the_answer_can_be_kept_as_a_working_set() {
    let render = function("renderSearch");
    for part in [
        "drawn.length >= 2",
        "first: searchAns.hits.length >= SEARCH_LIMIT",
        "\"Show these \" + keep.ids.length + \" on the map\"",
        "\" - the search stopped at \" + SEARCH_LIMIT",
        "\" not drawn in this view\"",
        "aria-keyshortcuts=\"Meta+Enter\"",
        "searchRows = [...(keep ? [keep] : []), ...words, ...related];",
    ] {
        assert!(render.contains(part), "the set row lacks {part}");
    }
    assert!(SCRIPT.contains("const SEARCH_LIMIT = 20;"));
    assert!(SCRIPT.contains("ev.key === \"Enter\" && (ev.metaKey || ev.ctrlKey)"));
    let choose = function("chooseSearch");
    assert!(choose.contains("keepInView(h.ids, { kind: \"search\", q: h.q, first: h.first });"));
    assert!(
        SCRIPT.contains("const kept = !act && workSet ? workSet.ids : null;"),
        "a selection takes the map first"
    );
    assert!(
        SCRIPT.contains(": act ? act.ns.has(n.id) : kept ? kept.has(n.id) : true;"),
        "the rest is not dimmed"
    );
    assert!(
        SCRIPT.contains(
            "!hot && kept && !(kept.has(e.a.id) && kept.has(e.b.id)) ? DIM_EDGE / EDGE_ALPHA : 1"
        ),
        "relations leaving the set are not dimmed"
    );
    assert!(
        !function("activeSet").contains("workSet"),
        "set members are made active and never fade"
    );
    assert!(function("statsLine").contains("if (workSet) line += \", \" + workSetSaid();"));
    assert!(function("workSetSaid").contains("\" kept for \\\"\""));
    assert!(function("workSetSaid").contains("\" (the search stopped at \" + SEARCH_LIMIT"));
    let kept = function("renderKept");
    assert!(kept.contains("\" kept\"") && kept.contains("\"Fit to kept entities\""));
    assert!(SCRIPT.contains("document.querySelector(\"#kept .kx\").onclick = clearWorkSet;"));
    assert!(PAGE.contains("<span id=\"kept\" class=\"spill\" hidden>"));
    assert!(function("followKept").contains("mergedInto(id)"), "a merged member is dropped");
    assert!(function("showAll").contains("workSet = null;"), "Show all leaves the set");
    assert!(function("mapInUse").contains("!!workSet"), "Follow does not wait for a set");
    assert!(function("beginWorkspaceTransition").contains("workSet = null;"));
    assert!(SCRIPT.contains("else if (workSet) clearWorkSet();       // the working set ends last"));
    assert!(SCRIPT.contains("if (listOn) setList(false);   // List goes back to Map first"));
    assert!(
        SCRIPT.contains("fitView(kept.length ? kept : undefined)"),
        "Fit does not frame the set"
    );
}

/// Show related keeps the entity and what its drawn relations reach, a step at a time.
#[test]
fn show_related_adds_what_the_relations_reach() {
    let related = function("relatedDrawn");
    assert!(related
        .contains("edgeTypeOff.has(e.type) || typeOff.has(e.a.type) || typeOff.has(e.b.type)"));
    let detail = function("renderDetail");
    assert!(
        detail.contains("<button type=\"button\" class=\"linkish shrel\">Show related</button>")
    );
    assert!(detail.contains("keepInView([node.id, ...relatedDrawn(node).map(n => n.id)], { kind: \"related\", name: node.name })"));
    assert!(function("partSaid").contains("\" related to \""));
}

/// While List shows, zoom and Fit are idle in place, described by List's sentence; the List tab is
/// idle on an empty scope and the arrow keys still reach it.
#[test]
fn list_leaves_the_camera_idle_in_place() {
    assert!(SCRIPT.contains("const IDLE_WHEN_LISTING = [\"zin\", \"zout\", \"fit\"];"));
    assert!(function("idleWhileEmpty")
        .contains("listOn && IDLE_WHEN_LISTING.includes(id) ? \"listIdle\""));
    assert!(PAGE.contains("<span id=\"listIdle\" hidden>List shows the map as a table.</span>"));
    assert!(function("setList").contains("idleWhileEmpty();"));
    assert!(function("arrowTabs").contains(
        "if (tabs[j].getAttribute(\"aria-disabled\") === \"true\") { tabs[j].focus(); return; }"
    ));
    let list = function("renderList");
    assert!(
        list.contains("class=\"linkish lclear\">Clear</button>"),
        "List's set line has no Clear"
    );
    assert!(list.contains("rows.length + \" of \" + c.shown + \" entities kept - \""));
}
