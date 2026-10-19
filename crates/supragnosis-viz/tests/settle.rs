//! docs/design-system.md U18: settling a contested value. A source tripwire over the viewer's script
//! and stylesheet; the server's half - the act filed in the observation's workspace, and the
//! demotion that undoes it - is `viz_resolve_files_in_the_observations_workspace_and_can_be_undone`
//! in http.rs.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const CSS: &str = include_str!("../assets/viewer.css");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

#[test]
fn settling_a_contested_value_says_why_asks_once_and_can_be_undone() {
    let rows = function("contestedRows");
    // Why the shown value is shown, in the policy's terms.
    assert!(
        rows.contains("so the newer one is shown until you decide"),
        "the block does not say why"
    );
    assert!(rows.contains("The Human confirmed value is shown."));
    // One act per value, at one weight; the shown value is marked as shown, not as preferred.
    assert!(
        rows.contains("\"Confirm instead\" : \"Confirm\""),
        "the values do not share one act"
    );
    assert!(
        !rows.contains("\"keep\"") && !rows.contains("use this"),
        "the old lopsided acts remain"
    );
    assert!(
        !CSS.contains(".contested .crow.cur .confirm"),
        "the shown value's act is drawn heavier"
    );
    assert!(rows.contains("<span class=\"curtag\">shown</span>"));
    // The act asks once, in place, and says what it will do.
    assert!(
        rows.contains("data-settle=\"yes\"") && rows.contains("data-settle=\"no\""),
        "the act does not ask"
    );
    assert!(
        rows.contains("It becomes Human confirmed"),
        "the question does not say what happens"
    );
    // The console asserts nothing of its own: "Neither is right" hands an agent a sentence.
    assert!(rows.contains("Neither is right") && rows.contains("\"Record the correct \""));
    // The inspector finds its own "why" disclosure by class, so the block must not reuse the name.
    assert!(!rows.contains("class=\"why\""), "the block reuses the inspector's .why");
    // Undo is the reverse gated event, and the notice says what it costs, in words.
    let settle = function("settleValue");
    assert!(
        settle.contains("consoleWrite(\"/api/resolve\", settleQuery(a.obs, \"human_confirmed\")")
    );
    assert!(
        settle.contains("settleQuery(a.obs, a.was, \"claim_demotion\")"),
        "Undo is not the demotion"
    );
    assert!(
        settle.contains("\"claim_demotion\""),
        "Confirm instead does not demote the shown value"
    );
    assert!(
        settle.contains("until someone confirms it again"),
        "the notice does not name the ceiling"
    );
    assert!(
        function("toast").contains("kind === \"ok\" && !action"),
        "a notice with Undo leaves on its own"
    );
}
