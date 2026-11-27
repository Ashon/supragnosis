//! docs/design-system.md U18: settling a contested value. A source tripwire over the viewer's
//! script; the server's half - the act filed in the observation's workspace, and the
//! demotion that undoes it - is `viz_resolve_files_in_the_observations_workspace_and_can_be_undone`
//! in http.rs.

mod support;
use support::says;

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));

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
    for en in [
        "These values are all {tier}, so the newer one is shown until you decide.",
        "Two sources disagree on {name}'s {field}. Both are {tier} claims, so the newer one is shown until you decide.",
        "The {tier} value is shown.",
    ] {
        assert!(says(rows, en), "the block does not say why: {en}");
    }
    // One act per value, at one weight; the shown value is marked as shown, not as preferred.
    assert!(
        rows.contains("word(instead ? \"confirmInstead\" : \"confirmAct\")"),
        "the values do not share one act"
    );
    assert!(says(rows, "shown") && rows.contains("<span class=\"curtag\">"));
    // The act asks once, in place, and says what it will do (Section 29.2: the source and its reach).
    let ask = function("askHtml");
    assert!(
        rows.contains("data-settle=\"ask\"") && rows.contains("askHtml(ctx)"),
        "the act does not ask"
    );
    assert!(
        ask.contains("data-settle=\"yes\"") && ask.contains("data-settle=\"no\""),
        "the question has no Confirm and Cancel"
    );
    assert!(
        says(ask, "This confirms one source:") && ask.contains("ringMoves(d, ctx.entity)"),
        "the question does not say what happens"
    );
    // The console asserts nothing of its own: "Neither is right" hands an agent a sentence, in the
    // agents' English whatever the page's language (docs/i18n.md Section 2).
    assert!(says(rows, "Neither is right") && rows.contains("\" wrong. Record the correct \""));
    assert!(rows.contains("<code lang=\"en\">${say}</code>"));
    // Undo is the reverse gated event, and the notice says what it costs, in words.
    let settle = function("settleValue");
    // Each act keeps the sentence its notice says as its rationale (Section 29.3).
    assert!(settle.contains(
        "consoleWrite(\"/api/resolve\", settleQuery(a.from, \"human_confirmed\", null, confirmed)"
    ));
    assert!(
        settle.contains("settleQuery(a.from, fromWas, \"claim_demotion\", word(\"undid\", { said: confirmed }))"),
        "Undo is not the demotion"
    );
    assert!(
        settle.contains("\"claim_demotion\""),
        "Confirm instead does not demote the shown value"
    );
    assert!(
        says(settle, "Undo returns this source to {tier} and keeps it there until someone confirms it again."),
        "the notice does not name the ceiling"
    );
}

/// design-system.md Section 29.2 and U18's sharpened clause: the ask is drawn from the engine's diff
/// and checks (/api/resolve/preview) and Confirm acts only once they are read; it names the source,
/// the rings it moves and what else it states; the person chooses the source when several assert the
/// value; "Confirm instead" is previewed as its two acts; Undo lowers the source to the tier the diff
/// says it had and is offered only after its own preview; and the tiers are defined under the legend.
#[test]
fn a_confirmation_says_what_it_confirms() {
    let read = function("readSettle");
    assert!(
        read.contains("/api/explain?entity=") && read.contains("mine.observations"),
        "the sources are not read"
    );
    assert!(
        read.contains("cs.base_tier"),
        "Confirm instead has no tier to demote the shown source to"
    );
    assert!(
        read.contains("{ obs: ctx.curObs, tier: curBase, kind: \"claim_demotion\" }, { obs: from, tier: \"human_confirmed\" }"),
        "Confirm instead is not previewed as its two acts"
    );
    assert!(
        function("readPreview").contains("/api/resolve/preview"),
        "the ask is not read from the engine"
    );
    assert!(
        function("previewQuery").contains("i ? \"2\" : \"\""),
        "a second act is not sent"
    );

    let ask = function("askHtml");
    assert!(
        ask.contains("role=\"radiogroup\" aria-label=\"${word(\"confirmFrom\")}\"")
            && says(ask, "Confirm from"),
        "the source cannot be chosen"
    );
    assert!(ask.contains("v.sources.length > 1"), "a single source asks to be chosen");
    assert!(
        ask.contains("can ? \"\" : html` disabled aria-describedby="),
        "Confirm acts before its diff is read, or does not say why it waits"
    );
    assert!(says(ask, "Reading what this confirms..."));
    assert!(ask.contains("mine.cut ? \"...\""), "a cut quote does not say so");
    assert!(
        says(ask, "It also states {n} relations.")
            && says(ask, "It also defines {n} types: {names}."),
        "the rest of the bundle is unsaid"
    );
    assert!(
        ask.contains("TIER_MEANS.human_confirmed"),
        "the ask does not say what Human confirmed means"
    );
    assert!(
        says(ask, "Checks pass: {checks}") && says(ask, "Blocked: {checks}"),
        "the checks are not shown"
    );
    assert!(
        function("ringMoves").contains("e.from !== e.to && e.entity !== skip"),
        "the reach names entities whose ring does not move"
    );

    let settle = function("settleValue");
    assert!(
        settle.contains("before(a.from)") && settle.contains("d.tier_changes"),
        "Undo does not lower the source to its own tier before the act"
    );
    assert!(
        settle.contains("const got = await readPreview(reverse);"),
        "Undo is offered before its diff is read"
    );
    assert!(
        says(settle, "Undo is not offered: what it would move back could not be read."),
        "an unreadable Undo is not said"
    );

    let legend = function("renderTrustLegend");
    assert!(
        says(legend, "What these mean") && legend.contains("TIER_MEANS[t]"),
        "the tiers are not defined under the legend"
    );
    for line in [
        "Confirmed through this app on one of your machines, by you or another program on your account; agents are told to leave it to you. No agent's claim outranks it; a later review can lower it.",
        "Above Agent, but not confirmed through this app: a trusted machine's signed claim, or a review an agent accepted.",
        "What an AI app writes, and what this app writes for you, until a review raises or lowers it.",
        "Trusted less than an agent's claim: a review lowered it, or its sender marked it so.",
    ] {
        assert!(SCRIPT.contains(line), "the definition is not the spec's: {line}");
    }
}

/// design-system.md Section 29.26: a claim's card offers its demotion, asks
/// first - what flips, whose ring drops, what else it states, what Unverified means and the ceiling,
/// then the checks - and casts only then; its notice offers Undo from the act's own undo read, and
/// Esc closes the question before the card.
#[test]
fn a_person_can_demote_a_claim() {
    let card = function("obsCardHtml");
    assert!(card.contains("if (!o.proposal && TIER_ORDER.indexOf(o.effective_tier) > 0) {"));
    assert!(says(card, "Demote this claim"));
    let ask = function("demoteHtml");
    for part in ["ringMoves(d, null, true)", "can ? \"\" : html` disabled aria-describedby="] {
        assert!(ask.contains(part), "the question does not hold {part}");
    }
    for en in [
        "Demote this claim to {tier}?",
        "{what} becomes contested and waits under Needs you.",
        "{what} becomes {to} and is no longer contested.",
        "Every value it states stays; it is trusted less.",
        "No entity's ring moves.",
        "Later claims do not raise it; a review that raises it does.",
    ] {
        assert!(says(ask, en), "the question does not say it: {en}");
    }
    assert!(function("openDemote").contains("kind: \"claim_demotion\""));
    let cast = function("castDemote");
    for part in [
        "/api/resolve/undo?proposal=",
        "word(\"undid\", { said })",
        "/api/observations?id=",
    ] {
        assert!(cast.contains(part), "the act does not: {part}");
    }
    assert!(says(cast, "Demoted to {tier}: {claim}"));
    assert!(says(function("ringMoves"), "{names} drops to {tier}"));
    assert!(SCRIPT.contains("if (demoteAsk && cardObs) closeDemote(cardObs); else hideObsCard();"));
}
