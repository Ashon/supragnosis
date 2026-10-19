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
