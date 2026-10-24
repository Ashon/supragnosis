//! docs/design-system.md Section 14 steps 13 and 14: the screens the second pass changes, held by
//! source tripwires over the viewer's script and stylesheet. Each test names the rule it holds.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
const CSS: &str = include_str!("../assets/viewer.css");

/// The body of a top-level function of the viewer's script.
fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// The declarations of the first rule in the stylesheet whose selector is exactly `selector`.
fn rule(selector: &str) -> &'static str {
    let at = CSS
        .find(&format!("{selector} {{"))
        .unwrap_or_else(|| panic!("no rule {selector}"));
    let body = &CSS[at + selector.len() + 2..];
    &body[..body.find('}').expect("the rule closes")]
}

/// Section 6: the status bar says the scope and its counts once, in words, and says how fresh the map
/// is only once it is late. It gives up its counts before the gear, which never leaves the screen.
#[test]
fn the_status_bar_says_the_scope_once() {
    let stats = format!("{}{}", function("statsScope"), function("statsLine"));
    for word in ["All workspaces", "entities", "relations", "regions"] {
        assert!(stats.contains(word), "the stats line does not say {word}");
    }
    assert!(!SCRIPT.contains("type_counts"), "the status bar still lists counts per type");
    assert!(!SCRIPT.contains("(hull ${"), "the status bar still says hull");
    assert!(!SCRIPT.contains("\"updated \" +"), "the time still ticks at every poll");
    assert!(
        function("sayLate").contains("Not updated since"),
        "a failed poll does not say since when"
    );
    let poll = function("poll");
    assert!(
        poll.contains("sayLate(\"the server is not answering\")"),
        "a failed poll does not say why"
    );
    assert!(
        !poll.contains("statusEl.textContent ="),
        "a failed poll writes the status bar around sayLate"
    );
    let stats_css = rule("#statusbar #stats");
    assert!(
        stats_css.contains("min-width:0") && stats_css.contains("flex:1 1 auto"),
        "the counts do not give way"
    );
    assert!(
        rule("#statusbar #settingsBtn").contains("flex:none"),
        "the gear can be pushed off"
    );
}

/// Section 7: the Review page is used from the keyboard. Its segments are tabs that move with the
/// arrows; A, R, J and K act only while focus is in the list or the item, and a control names its key
/// in `aria-keyshortcuts` rather than in its name; Skip is a button; a redraw keeps focus on the item;
/// and while the page is open the map and the panels behind it are inert.
#[test]
fn the_review_page_works_from_the_keyboard() {
    let render = function("renderReview");
    assert!(render.contains("arrowTabs(segEl)"), "the segments do not move with the arrows");
    assert!(render.contains("segEl.dataset.arrows"), "the arrows are wired at every redraw");
    assert!(
        render.contains(".rv-item.on") && render.contains(".focus()"),
        "a redraw drops focus"
    );
    assert!(render.contains("setReviewInert("), "the page behind Review stays reachable");
    let inert = function("setReviewInert");
    for id in [
        "\"c\"",
        "\"dockL\"",
        "\"dockR\"",
        "\"detail\"",
        "\"log\"",
        "\"strip\"",
        "\"minimap\"",
        "\"hud\"",
    ] {
        assert!(inert.contains(id), "setReviewInert leaves {id} out");
    }
    let detail = function("itemDetail");
    assert!(
        !detail.contains("<kbd>A</kbd></button>") && !detail.contains("<kbd>R</kbd></button>"),
        "a key is part of a button's name"
    );
    for key in ["A", "R"] {
        assert!(
            detail.contains(&format!("aria-keyshortcuts=\"{key}\"")),
            "no control names the key {key}"
        );
    }
    // Skip is one button wherever an item is decided one at a time (Section 29.3).
    let skip = function("skipButton");
    assert!(skip.contains("aria-keyshortcuts=\"J\""), "no control names the key J");
    assert!(
        skip.contains("data-skip") && detail.contains("skipButton()"),
        "there is no Skip button"
    );
    assert!(function("wireDecisions").contains("[data-skip]"), "Skip does nothing");
    let at = SCRIPT
        .find("window.addEventListener(\"keydown\", e => {\n  if (!reviewOpen() || dialogOpen()) return;")
        .expect("the Review keys");
    let keys = &SCRIPT[at..at + SCRIPT[at..].find("\n});").expect("it closes")];
    assert!(
        keys.contains("closest(\"#rvList, #rvDetail\")"),
        "A and R act from anywhere on the page"
    );
}

/// Section 8: the search field is a combobox that chooses nothing until the person moves. Enter acts
/// only on the row moved to, a choice clears the field (Section 29.11), and the field names its scope.
#[test]
fn search_chooses_nothing_until_the_person_moves() {
    assert!(
        function("runSearch").contains("searchSel = -1;"),
        "an answer arrives with a row chosen"
    );
    let mv = function("moveSearch");
    assert!(mv.contains("searchSel < 0"), "moving from no choice is not handled");
    let render = function("renderSearch");
    assert!(
        render.contains("searchSel >= 0 ? \"sp\" + searchSel : \"\""),
        "the field points at a row nobody chose"
    );
    let at = SCRIPT.find("searchEl.addEventListener(\"keydown\"").expect("the search keys");
    let keys = &SCRIPT[at..at + SCRIPT[at..].find("\n});").expect("it closes")];
    assert!(keys.contains("searchSel >= 0"), "Enter acts with no row chosen");
    // Plain Enter never takes a row on its own; Cmd+Enter keeps the answer, the set row being first
    // (design-system.md Section 29.11).
    let enter = &keys[keys.find("} else if (ev.key === \"Enter\") {").expect("plain Enter")..];
    let enter = &enter[..enter.find("} else if").unwrap_or(enter.len())];
    assert!(!enter.contains("chooseSearch(0)"), "Enter takes the top row on its own");
    assert!(
        keys.contains("searchRows[0].kind === \"keep\") chooseSearch(0)"),
        "Cmd+Enter keeps only the set row"
    );
    let choose = function("chooseSearch");
    let entity = &choose[choose.find("if (h.kind === \"entity\")").expect("the entity branch")..];
    assert!(
        entity.contains("searchEl.value = \"\";"),
        "a choice does not clear the field (Section 29.11)"
    );
    let scope = function("syncSearchScope");
    for word in ["Search all workspaces", "\"Search \" + defaultName()", "\"Search \" + "] {
        assert!(scope.contains(word), "the field does not name its scope: {word}");
    }
    assert!(
        function("renderChipsActive").contains("syncSearchScope()"),
        "the scope's name is not kept current"
    );
}

/// Sections 4 and 10: the View popover opens from its button with the keyboard and says it is open,
/// the settings dialog has a name, the contested block's buttons are a finger's size, and a name the
/// map does not draw is said whole.
#[test]
fn the_small_controls_say_what_they_are() {
    let page = include_str!("../assets/viewer.html");
    let view = &page[page.find("<button id=\"viewBtn\"").expect("viewBtn")..];
    let view = &view[..view.find('>').expect("the tag closes")];
    assert!(
        view.contains("aria-expanded=\"false\"") && view.contains("aria-controls=\"viewpop\""),
        "the View button does not say what it opens"
    );
    let pop = function("setViewPop");
    assert!(pop.contains("aria-expanded"), "the View button does not say it is open");
    assert!(pop.contains(".focus()"), "opening the popover leaves focus behind");
    let dlg = &page[page.find("<dialog id=\"settings\"").expect("the settings dialog")..];
    let dlg = &dlg[..dlg.find('>').expect("the tag closes")];
    assert!(
        dlg.contains("aria-labelledby=\"settingsTitle\""),
        "the settings dialog has no name"
    );
    assert!(
        page.contains("<span class=\"ftitle\" id=\"settingsTitle\">"),
        "the dialog's title has no id"
    );
    let confirm = rule(".contested .confirm");
    assert!(confirm.contains("min-height:24px"), "a contested value's button is under 24px");
    assert!(
        CSS.contains(".contested .confirm { min-height:28px; }"),
        "a contested value's button is under 28px on a phone"
    );
    assert!(!function("nameOf").contains("slice(0, 8)"), "a name is cut to 8 characters");
}

/// Section 8, U15: the search list groups its hits by how they were reached - those that contain the
/// words, then the related ones, each saying why it is there - and a scope where nothing contains the
/// words says so and offers the wider scope with its count.
#[test]
fn search_groups_hits_by_how_they_were_reached() {
    let render = function("renderSearch");
    for part in [
        "h.reached === \"words\"",
        "\"Contains \\\"\"",
        ">Related</div>",
        "\"near in meaning\"",
        "\"related to a match\"",
        "\"No entity in \"",
        "\"Search all workspaces (\"",
        "\"20+\"",
    ] {
        assert!(render.contains(part), "the search list does not say {part}");
    }
    assert!(
        !render.contains("\"Entities\"") && !render.contains("Observations</div>"),
        "the list still groups by kind"
    );
    let wider = function("searchWider");
    assert!(
        wider.contains("workspace=*") && wider.contains("reached === \"words\""),
        "the wider count is not the words found"
    );
}

/// The declarations of every rule inside the stylesheet's `@media (max-width: 759px)` block.
fn phone_block() -> &'static str {
    let at = CSS.find("@media (max-width: 759px) {").expect("the phone block");
    let body = &CSS[at..];
    let mut depth = 0usize;
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &body[..i];
                }
            }
            _ => {}
        }
    }
    panic!("the phone block does not close")
}

/// Section 14 step 17: the third pass's defects that ask no design question. An entry the scope does
/// not hold is drawn at full strength and says so, a key cap reads, a short target reaches U19's
/// floor as it stood before step 19 (24, 28 below 760), the search field is named with its scope,
/// and a phone's status bar gives up its counts instead of cutting them.
#[test]
fn the_third_pass_defects_on_the_viewer() {
    let off = rule(".tlg.off");
    assert!(!off.contains("opacity"), "an entry the scope does not hold is dimmed, not said");
    let legend = function("renderTrustLegend");
    assert!(legend.contains("none here"), "an entry the scope does not hold does not say so");
    let kbd = rule(".rv-detail kbd, .rv-acts kbd");
    assert!(
        !kbd.contains("opacity") && kbd.contains("color:var(--ink2)"),
        "a key cap is dimmed"
    );
    for selector in [
        "#statusbar #settingsBtn",
        "#statusbar .spill",
        "#defsBox summary",
        "#detail .close",
        "#obscard .close",
        "#settings .fclose",
        "#settings dd a",
        ".contested .neither summary",
        "#detail .rchip",
        ".nchip",
        ".echip",
    ] {
        assert!(rule(selector).contains("min-height:24px"), "{selector} is under 24 pixels");
    }
    assert!(rule(".tback").contains("min-height:28px"), "the way back is under 28 pixels");
    let phone = phone_block();
    assert!(
        phone.contains(".rv-segb, .rv-acts .btn { min-height:28px; }"),
        "the Review page's tabs and acts are under 28 pixels on a phone"
    );
    assert!(
        function("syncSearchScope").contains("setAttribute(\"aria-label\", ph)"),
        "the search field is named only by its placeholder"
    );
    assert!(
        function("showStats").contains("counts")
            && phone.contains("#statusbar #stats .counts { display:none; }"),
        "a phone cuts the counts instead of giving them up"
    );
}
