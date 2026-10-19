//! docs/design-system.md Section 14 steps 13 and 14: the screens the second pass changes, held by
//! source tripwires over the viewer's script and stylesheet. Each test names the rule it holds.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const CSS: &str = include_str!("../assets/viewer.css");

/// The body of a top-level function of the viewer's script.
fn function(name: &str) -> &'static str {
    let at = SCRIPT.find(&format!("function {name}(")).unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// The declarations of the first rule in the stylesheet whose selector is exactly `selector`.
fn rule(selector: &str) -> &'static str {
    let at = CSS.find(&format!("{selector} {{")).unwrap_or_else(|| panic!("no rule {selector}"));
    let body = &CSS[at + selector.len() + 2..];
    &body[..body.find('}').expect("the rule closes")]
}

/// Section 6: the status bar says the scope and its counts once, in words, and says how fresh the map
/// is only once it is late. It gives up its counts before the gear, which never leaves the screen.
#[test]
fn the_status_bar_says_the_scope_once() {
    let stats = function("statsLine");
    for word in ["All workspaces", "entities", "relations", "regions"] {
        assert!(stats.contains(word), "the stats line does not say {word}");
    }
    assert!(!SCRIPT.contains("type_counts"), "the status bar still lists counts per type");
    assert!(!SCRIPT.contains("(hull ${"), "the status bar still says hull");
    assert!(!SCRIPT.contains("\"updated \" +"), "the time still ticks at every poll");
    assert!(function("sayLate").contains("Not updated since"), "a failed poll does not say since when");
    let poll = function("poll");
    assert!(poll.contains("sayLate(\"the server is not answering\")"), "a failed poll does not say why");
    assert!(!poll.contains("statusEl.textContent ="), "a failed poll writes the status bar around sayLate");
    let stats_css = rule("#statusbar #stats");
    assert!(stats_css.contains("min-width:0") && stats_css.contains("flex:1 1 auto"), "the counts do not give way");
    assert!(rule("#statusbar #settingsBtn").contains("flex:none"), "the gear can be pushed off");
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
    assert!(render.contains(".rv-item.on") && render.contains(".focus()"), "a redraw drops focus");
    assert!(render.contains("setReviewInert("), "the page behind Review stays reachable");
    let inert = function("setReviewInert");
    for id in ["\"c\"", "\"dockL\"", "\"dockR\"", "\"detail\"", "\"log\"", "\"strip\"", "\"minimap\"", "\"hud\""] {
        assert!(inert.contains(id), "setReviewInert leaves {id} out");
    }
    let detail = function("itemDetail");
    assert!(!detail.contains("<kbd>A</kbd></button>") && !detail.contains("<kbd>R</kbd></button>"), "a key is part of a button's name");
    for key in ["A", "R", "J"] {
        assert!(detail.contains(&format!("aria-keyshortcuts=\"{key}\"")), "no control names the key {key}");
    }
    assert!(detail.contains("data-skip"), "there is no Skip button");
    assert!(function("wireDecisions").contains("[data-skip]"), "Skip does nothing");
    let at = SCRIPT.find("window.addEventListener(\"keydown\", e => {\n  if (!reviewOpen()) return;").expect("the Review keys");
    let keys = &SCRIPT[at..at + SCRIPT[at..].find("\n});").expect("it closes")];
    assert!(keys.contains("closest(\"#rvList, #rvDetail\")"), "A and R act from anywhere on the page");
}

/// Section 8: the search field is a combobox that chooses nothing until the person moves. Enter acts
/// only on the row moved to, a chosen entity's name replaces the query, and the field names its scope.
#[test]
fn search_chooses_nothing_until_the_person_moves() {
    assert!(function("runSearch").contains("searchSel = -1;"), "an answer arrives with a row chosen");
    let mv = function("moveSearch");
    assert!(mv.contains("searchSel < 0"), "moving from no choice is not handled");
    let render = function("renderSearch");
    assert!(render.contains("searchSel >= 0 ? \"sp\" + searchSel : \"\""), "the field points at a row nobody chose");
    let at = SCRIPT.find("searchEl.addEventListener(\"keydown\"").expect("the search keys");
    let keys = &SCRIPT[at..at + SCRIPT[at..].find("\n});").expect("it closes")];
    assert!(keys.contains("searchSel >= 0"), "Enter acts with no row chosen");
    assert!(!keys.contains("chooseSearch(0)"), "Enter takes the top row on its own");
    let choose = function("chooseSearch");
    assert!(choose.contains("searchEl.value = n.name;"), "the chosen name does not replace the query");
    let scope = function("syncSearchScope");
    for word in ["Search all workspaces", "Search default", "\"Search \" + "] {
        assert!(scope.contains(word), "the field does not name its scope: {word}");
    }
    assert!(function("renderChipsActive").contains("syncSearchScope()"), "the scope's name is not kept current");
}

/// Sections 4 and 10: the View popover opens from its button with the keyboard and says it is open,
/// the settings dialog has a name, the contested block's buttons are a finger's size, and a name the
/// map does not draw is said whole.
#[test]
fn the_small_controls_say_what_they_are() {
    let page = include_str!("../assets/viewer.html");
    let view = &page[page.find("<button id=\"viewBtn\"").expect("viewBtn")..];
    let view = &view[..view.find('>').expect("the tag closes")];
    assert!(view.contains("aria-expanded=\"false\"") && view.contains("aria-controls=\"viewpop\""), "the View button does not say what it opens");
    let pop = function("setViewPop");
    assert!(pop.contains("aria-expanded"), "the View button does not say it is open");
    assert!(pop.contains(".focus()"), "opening the popover leaves focus behind");
    let dlg = &page[page.find("<dialog id=\"settings\"").expect("the settings dialog")..];
    let dlg = &dlg[..dlg.find('>').expect("the tag closes")];
    assert!(dlg.contains("aria-labelledby=\"settingsTitle\""), "the settings dialog has no name");
    assert!(page.contains("<span class=\"ftitle\" id=\"settingsTitle\">"), "the dialog's title has no id");
    let confirm = rule(".contested .confirm");
    assert!(confirm.contains("min-height:24px"), "a contested value's button is under 24px");
    assert!(CSS.contains(".contested .confirm { min-height:28px; }"), "a contested value's button is under 28px on a phone");
    assert!(!function("nameOf").contains("slice(0, 8)"), "a name is cut to 8 characters");
}
