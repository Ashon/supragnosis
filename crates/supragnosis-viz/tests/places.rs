//! docs/design-system.md U20: the way to Review is always in the status bar on the local console,
//! with one count, and nothing but the person moves what they are reading. Source tripwires over the
//! viewer's script.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));

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
        pill.contains("(n === 1 ? \"1 needs you\" : n + \" need you\") + (contested ? \" (\" + contested + \" contested)\" : \"\")"),
        "the contested beliefs are not counted inside the one count"
    );
    // Needs you lists only the beliefs that tie on trust, so a confirmation takes its item away.
    let needs = function("needsItems");
    assert!(
        needs.contains(".filter(c => c.contested)"),
        "Needs you lists conflicts trust already settled"
    );
    assert!(
        function("signalItems").contains("\"Settled conflict: \""),
        "a settled conflict is listed nowhere"
    );
}

/// Follow moves the camera, never the reader: activity elsewhere is a notice with Show, and nothing in
/// the event handler changes the scope.
#[test]
fn follow_frames_what_is_in_scope_and_notes_the_rest() {
    let handle = function("handleEvent");
    assert!(
        handle.contains("if (ev.workspace && !inScope(ev.workspace))"),
        "activity elsewhere is not told apart"
    );
    assert!(handle.contains("noteElsewhere(ev.workspace)"), "a write elsewhere is not noted");
    assert!(
        !handle.contains("wsInput.value ="),
        "an agent's activity changes the scope on screen"
    );
    assert!(
        !handle.contains("beginWorkspaceTransition"),
        "an agent's activity moves the reader"
    );
    let note = function("noteElsewhere");
    assert!(note.contains("label: \"Show\""), "the notice has no Show");
}

/// The first view is where the knowledge is: a default holding at most half of the node's entities,
/// with the rest elsewhere, opens on All, decided once before the first draw (design-system.md
/// Section 29.21); an empty default with knowledge elsewhere opens on All and says so; and the scope
/// in use is always a selected chip.
#[test]
fn the_first_view_opens_where_the_knowledge_is() {
    let first = SCRIPT
        .find("async function decideFirstView()")
        .map(|at| &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")])
        .expect("decideFirstView");
    assert!(
        first.contains("if (t > d && d * 2 <= t)"),
        "a default holding most of the knowledge is left"
    );
    assert!(
        SCRIPT.contains("loadSurface().then(decideFirstView).finally(poll)"),
        "decided after the first draw"
    );
    let empty = function("refreshEmpty");
    assert!(
        empty.contains("if (firstView && cur === \"\" && others.length)"),
        "an empty default is where the page stays"
    );
    assert!(empty.contains("wsInput.value = \"*\""), "the page does not open on All");
    let load = function("loadWorkspaces");
    // The default by its name (design-system.md Section 29.10).
    assert!(
        load.contains("\"Showing all workspaces - \" + defaultName() + (openedHeld ? \" holds \" + openedHeld.d + \" of \" + openedHeld.t : \" is empty\")"),
        "the page does not say why it opened on All"
    );
    assert!(load.contains("mk(defaultName(), \"\")"), "an empty default has no chip");
    assert!(
        function("renderChipsActive").contains("chipIsCurrent("),
        "the selected chip is not the scope in use"
    );
}
