//! docs/design-system.md Section 29.9 (step 47): the map explains itself. Source tripwires over the
//! viewer's script for the key, the counted chips, the counts of what is drawn, the store's colors,
//! the trust filters and the hover card. Each test names the rule it holds.

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

/// The key names each mark the view draws, and only those: every row but size is conditional on
/// the state that draws its mark.
#[test]
fn the_maps_key_names_only_what_is_drawn() {
    let marks = function("renderMarks");
    for (word, gate) in [
        ("\"Two sources disagree\"", "contestedCount"),
        ("\"Soft cloud: a region - entities said together\"", "drawnCounts().reg"),
        ("\"Outline: a workspace\"", "boundaryMode === \"workspace\""),
        ("\"Outline: the machine that signed\"", "boundaryMode === \"origin\""),
        ("\"Glow: what agents used\"", "footprint.size"),
        ("\"Ring that grows and fades: just touched\"", "showPulses"),
    ] {
        let line = marks
            .lines()
            .find(|l| l.contains(word))
            .unwrap_or_else(|| panic!("the key does not say {word}"));
        assert!(line.contains(gate), "{word} shows only while {gate} draws it: {line}");
    }
    assert!(marks.contains("\"Larger: more relations\""), "the key says what size means");
    assert!(marks.contains("head.hidden = !rows.length"), "an empty key has no heading");
    assert!(function("showStats").contains("renderMarks()"), "the key follows every count");
    for toggle in ["renderBoundaryChips", "renderRegionsSeg"] {
        assert!(function(toggle).contains("renderMarks()"), "{toggle} redraws the key");
    }
}

/// Each type chip carries its count and an Only; a hidden one shows eye-off; a section says how
/// many are hidden, with Show all; and the status bar's Show all clears every filter.
#[test]
fn the_type_chips_count_and_offer_only() {
    let legend = function("renderLegend");
    assert!(legend.contains("className = \"n\""), "a chip carries its count");
    assert!(
        legend.contains("\"Only\"") && legend.contains("e.altKey"),
        "Only, or Option-click"
    );
    assert!(
        legend.contains("icon(\"eye-off\")"),
        "a hidden type shows eye-off, not a hollow disc"
    );
    let hidden = function("sectionHidden");
    assert!(hidden.contains("\" hidden \"") && hidden.contains("\"Show all\""));
    let all = function("showAll");
    for set in ["typeOff.clear()", "edgeTypeOff.clear()", "trustOff.clear()"] {
        assert!(all.contains(set), "Show all clears {set}");
    }
    assert!(PAGE.contains("id=\"showAll\""), "the status bar offers Show all");
    // The chips keep their order while the scope stays, so a toggle does not move the one pressed.
    assert!(function("legendCounts").contains("legendOrder.scope !== scope"));
}

/// The counts are of what is drawn, from the filters alone: never the zoom, the Relations switch or
/// Regions' mode; and every place that says them says the same line.
#[test]
fn the_counts_are_of_what_is_drawn() {
    let counts = function("drawnCounts");
    for set in ["typeOff", "edgeTypeOff", "trustOff"] {
        assert!(counts.contains(set), "the counts read {set}");
    }
    for view in ["cam.", "showEdges", "hyperMode", "allRegions", "clusterMode"] {
        assert!(!counts.contains(view), "the counts do not follow {view}");
    }
    let line = function("statsLine");
    for word in [" of ", " hidden)", " dimmed"] {
        assert!(line.contains(word), "the stats line says {word:?}");
    }
    let show = function("showStats");
    assert!(
        show.contains("aria-label") && show.contains(".lhead"),
        "the canvas and List say it too"
    );
    assert!(function("renderList").contains("statsLine("), "List's head is the same line");
}

/// A type keeps one color in every view a page shows (U16, D4): the store's most used types take
/// the band's slots, asked once per page; the rest take their preferred hue; and the page keeps
/// what it assigned, so changing scope recolors nothing.
#[test]
fn a_type_keeps_its_color_in_every_scope() {
    assert_eq!(
        SCRIPT.matches("fetch(\"/api/common_types\"").count(),
        1,
        "the store's common types are asked once per page"
    );
    let assign = function("assignColors");
    assert!(
        assign.contains("stableColors(new Set(commonTypes"),
        "the band goes to the store's types"
    );
    assert!(
        !assign.contains("stableColors(new Set(nodes"),
        "the scope's own types never decide the band"
    );
    assert!(assign.contains("typeColorKept[t]") && assign.contains("TYPE_BAND[nameSlot(t)]"));
    let first = SCRIPT
        .find("await commonTypesRead;")
        .expect("the first graph waits for the census");
    let apply = SCRIPT[first..].find("applyGraph(").expect("then applies the graph");
    assert!(apply < 400, "the census is read before the first graph is colored");
}

/// A trust filter dims (D5): its entities keep their place and their count, and draw at the
/// strength a selection leaves the rest at; a tier the scope does not hold is disabled.
#[test]
fn a_trust_filter_dims_and_keeps_the_count() {
    assert!(SCRIPT.contains("const DIM_REGION = "), "the region's dim strength has its name");
    assert!(
        SCRIPT.contains("!trustOff.has(n.trust_tier) && !saidOut(n) ? 1 : DIM_NODE"),
        "a dimmed tier, or a window of Said, draws at DIM_NODE (Section 29.19)"
    );
    let legend = function("renderTrustLegend");
    assert!(legend.contains("trustOff.has(t)") && legend.contains("el.disabled = true"));
    assert!(legend.contains("\"none here\""), "an empty tier says none here");
    assert!(legend.contains("A ring is the strongest source behind an entity."));
    // Dimming is drawing, not filtering: the layout and the counts keep a dimmed entity.
    let counts = function("drawnCounts");
    let shown = counts.lines().find(|l| l.contains("shown.add(")).expect("the shown set");
    assert!(!shown.contains("trustOff"), "a dimmed entity is still counted as drawn");
}

/// The hover card is a card (U25 revised): the name, the type and workspace where the scope spans
/// several, the relations, sources and tier in words, the description's first line, and the
/// contest.
#[test]
fn the_hover_card_says_what_an_entity_is() {
    let tip = function("showTip");
    for part in [
        "n.type + \" in \" + n.workspace",
        "plural(n.degree || 0, \"relation\")",
        "plural(n.sources || 0, \"source\")",
        "tierName(n.trust_tier)",
        "split(\"\\n\")[0]",
        "Two sources disagree",
    ] {
        assert!(tip.contains(part), "the hover card lacks {part}");
    }
    let hov = function("hoverSet");
    assert!(
        hov.contains("if (!hover || focus) return null;"),
        "a selection keeps the dimming"
    );
}

/// A workspace the address names and the node does not list shows its chip, selected and muted,
/// and the page says no workspace is called that.
#[test]
fn an_unlisted_workspace_is_named_as_such() {
    let chips = function("loadWorkspaces");
    assert!(chips.contains("mk(cur, cur, \"unlisted\")"), "its chip shows, muted");
    let empty = function("refreshEmpty");
    assert!(empty.contains("\"No workspace is called \""));
}

/// One word for the cloud: "region" wherever the map's soft clouds are named.
#[test]
fn the_cloud_is_a_region_everywhere() {
    for retired in ["Overloaded context", "Make it a group", "Name this group", "Group by type"] {
        assert!(
            !SCRIPT.contains(retired) && !PAGE.contains(retired),
            "the viewer still says {retired}"
        );
    }
    assert!(SCRIPT.contains("Overloaded region: ") && PAGE.contains("Arrange by type"));
}

/// Section 29.19: each entity says when it was last said - on the hover card, the sheet and List - in
/// the Log's form with the year when it is not this one; the Said filter is a radio group whose
/// window dims, as a tier does, and whose last visit ends after half an hour away.
#[test]
fn the_map_says_when_each_entity_was_said() {
    assert!(
        PAGE.contains("<div class=\"rsec\" data-needs=\"entities\">Said</div>"),
        "no Said section"
    );
    let when = function("obsWhen");
    assert!(
        when.contains("d.getFullYear() !== now.getFullYear()"),
        "an old time reads as this year's"
    );
    let legend = function("renderSaidLegend");
    for part in [
        "\"No earlier visit here\"",
        "radioGroup(host, \"Said\")",
        "!typeOff.has(n.type)",
    ] {
        assert!(legend.contains(part), "the Said legend does not hold {part}");
    }
    assert!(SCRIPT.contains("[\"visit\", \"Since your last visit\"]"));
    assert!(SCRIPT.contains("const VISIT_GAP = 30 * 60 * 1000;"));
    assert!(
        function("statsLine").contains("\", said before \" + SAID_BEFORE[saidWin] + \" dimmed\"")
    );
    assert!(
        SCRIPT.contains("saidOut(e.a) || saidOut(e.b)"),
        "a dimmed entity's relations stay lit"
    );
    assert!(
        SCRIPT.contains("Last said ${obsWhen(node.last_said)}"),
        "the sheet does not say it"
    );
    assert!(
        SCRIPT.contains("Last said ${obsWhen(n.last_said)}"),
        "the hover card does not say it"
    );
    assert!(
        SCRIPT.contains("listHead(\"said\", \"Last said\")"),
        "List has no Last said column"
    );
}
