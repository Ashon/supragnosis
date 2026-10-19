//! docs/design-system.md U20: the way to Review is always in the status bar on the local console,
//! with one count, and nothing but the person moves what they are reading. Source tripwires over the
//! viewer's script.

const SCRIPT: &str = include_str!("../assets/viewer.js");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

#[test]
fn review_is_always_in_the_status_bar_with_one_count() {
    let pill = function("updateReviewPill");
    assert!(
        pill.contains("pill.hidden = surfaceKind !== \"local\";"),
        "Review's entry hides for a reason other than the read tier"
    );
    assert!(
        pill.contains(": \"Review\";"),
        "with nothing waiting the entry does not read Review"
    );
    assert!(
        pill.contains("\" to review\" + (contested ? \" (\" + contested + \" contested)\" : \"\")"),
        "the contested beliefs are not counted inside the one count"
    );
    // Needs you lists only the beliefs that tie on trust, so a confirmation takes its item away.
    let needs = function("needsItems");
    assert!(
        needs.contains(".filter(c => c.contested)"),
        "Needs you lists conflicts trust already settled"
    );
    assert!(
        function("curationParts").contains("settled conflicts"),
        "a settled conflict is listed nowhere"
    );
}
