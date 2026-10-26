//! docs/design-system.md Section 29.3, step 42: the Review page closes its loop. Source tripwires
//! over the viewer's script and page; the server's half - labels, the undo read and the
//! console's rationale - is `a_decided_confirmation_names_its_source_and_says_its_undo` in http.rs,
//! and the fold, the undo and the curation rows are in the engine's policy cases.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
const PAGE: &str = include_str!("../assets/viewer.html");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// A decided item leaves Needs you with no other input: the redraw signature holds each item's
/// state, a confirmation on the page reads curation again, and the next item takes the place and
/// the focus of the one decided.
#[test]
fn a_decision_leaves_needs_you_at_once() {
    let render = function("renderReview");
    assert!(
        render.contains("items.map(itemSig)"),
        "the signature does not hold each item's state"
    );
    let sig = function("itemSig");
    assert!(
        sig.contains("[it.key, it.c.contested, it.c.current]"),
        "a contested item's state is not in the signature, so a confirmed one stays"
    );
    assert!(
        render.contains("items[Math.min(reviewIndex, items.length - 1)]"),
        "the next item does not take the decided one's place"
    );
    assert!(render.contains("reviewFocus ||"), "focus does not go on after a decision");
    let settle = function("settleValue");
    assert!(
        settle.contains("if (reviewOpen()) { reviewFocus = true; await refreshCuration();"),
        "a confirmation on the Review page waits for the next poll to leave"
    );
    let decide = function("reviewDecide");
    assert!(decide.contains("if (done) { reviewFocus = true; stepReview(1); }"));
}

/// Every reverse is on its Decided item, the notice right after a decision names the item and
/// offers the same act where it decides nothing, and on the Review page that notice leaves.
#[test]
fn every_reverse_is_on_its_decided_item() {
    let detail = function("itemDetail");
    assert!(
        detail.contains("\"Accept after all\""),
        "a rejected item offers no way to accept it"
    );
    assert!(detail.contains("data-unmerge=\"1\""), "an accepted merge offers no Un-merge");
    assert!(
        detail.contains("undoable(p) ? undoParts(p) : null"),
        "a gate act offers no Undo"
    );
    assert!(
        function("undoable")
            .contains("p.kind === \"claim_promotion\" || p.kind === \"claim_demotion\""),
        "the wrong acts are undoable"
    );
    let label = function("undoLabel");
    for word in ["\"Undo demotion\"", "\"Undo confirmation\"", "\"Undo promotion\""] {
        assert!(label.contains(word), "{word} is missing");
    }
    // The reverse is read with its diff before it acts (U4), and a missing read is said.
    assert!(function("fetchUndoView").contains("/api/resolve/undo?proposal="));
    let parts = function("undoParts");
    assert!(parts.contains("is not offered: what it would move back could not be read."));
    assert!(parts.contains("Nothing to undo: another decision holds this source where it is."));
    // One act per target, each a console write that says what it did (U1).
    let undo = function("undoDecided");
    assert!(undo.contains(
        "consoleWrite(\"/api/resolve\", settleQuery(a.observation, a.tier, a.kind, rationale)"
    ));
    // The notices name the item; a reject's offers nothing, since Accept after all decides.
    let decide = function("reviewDecide");
    assert!(decide.contains("\"Accepted: \" + title"));
    assert!(decide.contains("{ label: \"Un-merge\", decided: true, run: () => proposeSplit(p) }"));
    assert!(decide.contains(
        "\"Rejected: \" + title, null, p.no_effect_yet ? null : \"Accept after all is under Decided.\""
    ));
    // A confirmation's Undo is on its Decided item, except after Confirm instead.
    assert!(function("settleValue").contains("decided: !a.instead,"));
    // The console's acts keep the sentence their notice says as their rationale.
    assert!(function("settleQuery").contains("&rationale="));
}

/// Decided says what was decided: one title from the server's labels, the outcome in words, the
/// diff said to be today's, and the Accept hint only where there is an Accept.
#[test]
fn decided_says_what_was_decided() {
    assert!(SCRIPT.contains(
        "const OUTCOME = { merged: \"Accepted\", rejected: \"Rejected\", withdrawn: \"Withdrawn\", blocked: \"Blocked\" };"
    ));
    assert!(
        function("targetSaid").contains("p.labels && p.labels[i]"),
        "titles ignore the server's labels"
    );
    assert!(function("itemTag").contains("OUTCOME[st] || st"), "a tag shows a state id");
    let detail = function("itemDetail");
    assert!(detail.contains("\"What it would change now\""));
    assert!(
        detail.contains("${accept ? \"Accept casts your verdict; nothing is deleted. \" : \"\"}")
    );
    assert!(
        detail.contains("If its check passes later, the accept it carries still merges it."),
        "a rejected blocked merge does not say its reject is provisional"
    );
    let line = function("proposalLine");
    assert!(line.contains(". A check blocked it, and it was rejected"));
}

/// One count, Needs you's, with contested type definitions in it; the queue says what it asks.
#[test]
fn one_count_covers_all_of_needs_you() {
    let title = function("contestedTitle");
    assert!(
        title.contains("\"'s definition: which meaning?\""),
        "a contested definition has no row"
    );
    assert!(title.contains("\"'s type: \""), "a contested type does not say its values");
    assert!(
        function("needsItems")
            .contains("(curation && curation.contradictions) || []).filter(c => c.contested)"),
        "Needs you does not list every contested belief the report holds"
    );
}

/// The keys act from the first press: opening the page focuses the selected row, a contested item
/// has Skip and its values as 1 to 9, and they act only while focus is in the list or the item.
#[test]
fn the_keys_act_from_the_first_press() {
    assert!(SCRIPT.contains(
        "window.addEventListener(\"hashchange\", () => { recallArmed = null; if (reviewOpen()) reviewFocus = true;"
    ));
    assert!(
        SCRIPT.contains("else if (/^[1-9]$/.test(e.key) && item && item.kind === \"contested\") {")
    );
    let detail = function("itemDetail");
    assert!(
        detail.matches("skipButton()").count() >= 3,
        "a secret or a contested item has no Skip"
    );
    assert!(
        function("renderReview").contains("b.setAttribute(\"aria-keyshortcuts\", String(i + 1))")
    );
}

/// Suggestions and Health are queues: one item per signal, its act in the detail.
#[test]
fn suggestions_and_health_are_queues() {
    assert!(function("segItems").contains("signalItems(seg)"));
    let items = function("signalItems");
    for title in [
        "\"Possible duplicate: \"",
        "\"Overloaded region: \"",
        "\"Unlinked: \"",
        "\"Conflicting merges: \"",
    ] {
        assert!(items.contains(title), "{title} is not an item");
    }
    let detail = function("signalDetail");
    assert!(detail.contains("class=\"propmerge\"") && detail.contains("class=\"reify\""));
    assert!(detail.contains("data-map=\"1\""), "a signal cannot be shown on the map");
    assert!(
        function("signalHint").contains("bandEmptyReason(curation.merge_band)"),
        "why the band is empty is not said"
    );
}

/// "Show on the map" frames what the item names, and a bar gives the way back.
#[test]
fn the_map_preview_has_a_way_back() {
    let preview = function("previewOnMap");
    assert!(preview.contains("itemNodes(it)") && preview.contains("fitView(ns)"));
    assert!(PAGE.contains("id=\"previewBar\""), "the page has no preview bar");
    assert!(PAGE.contains(">Back to Review</button>") && PAGE.contains(">Done</button>"));
    assert!(SCRIPT.contains(
        "const back = e.key === \"Escape\" || (e.key === \"[\" && (e.metaKey || e.ctrlKey));"
    ));
    assert!(
        SCRIPT
            .contains("if (proposalSel && !reviewPreview) { proposalSel = null; requestFrame(); }"),
        "a click on the empty map ends a preview Review opened"
    );
    assert!(function("backToReview").contains("reviewSeg = r.seg; reviewSel = r.key;"));
}

/// design-system.md Section 29.7, U4: a proposal's detail reads what changes - identity first for a
/// merge - then every check in words, then the evidence it rests on, then who asked; and a
/// contested item says each value's source as the inspector does.
#[test]
fn the_review_page_shows_its_evidence() {
    let detail = function("itemDetail");
    for part in [
        "identityHtml(p, ev)",
        "checkListHtml(p, v.checks)",
        "html`<div class=\"sec\">Evidence</div>`, evidenceHtml(p, ev)",
        "whoLine(opened.effective_tier, authorOf(opened))",
        "data-map=\"1\">Show on the map</button>`}${skipButton()}",
    ] {
        assert!(detail.contains(part), "the detail does not hold {part}");
    }
    let identity = function("identityHtml");
    assert!(identity.contains(" leaves the map; its name becomes an alias of "));
    assert!(identity.contains(" for ${keptName} from then on"));
    assert!(identity.contains("stays contested - "));
    let checks = function("checkListHtml");
    for part in ["i-circle-check", "i-circle-alert", "c.name === \"trust profile\"", "Sources: "] {
        assert!(checks.contains(part), "the checks do not hold {part}");
    }
    // A check reads its own words whether it passed or failed (Section 29.15).
    assert!(SCRIPT.contains("\"entity_merge|referential integrity\": (p, ok) => ok ? \"All \""));
    assert!(SCRIPT.contains("\"Not every entity it names is in its workspace\""));
    assert!(SCRIPT.contains(
        "\"claim_promotion|referential integrity\": (p, ok) => ok ? \"Its observations are in its workspace\""
    ));
    assert!(
        !checks.contains("checkSaid(p, c) + \": \" + c.detail"),
        "a failed check reads its passing words and then the engine's detail"
    );
    let fetch = function("fetchReviewEvidence");
    assert!(fetch.contains("/api/observations?entity=") && fetch.contains("observationById("));
    let render = function("renderReview");
    assert!(
        render.contains("fillSettleWho(detail, it.c.id)"),
        "a contested item leaves its sources blank"
    );
    let _ = PAGE;
}

/// Section 29.15: under All the console sends the view, `*`, and the server files each act where its
/// item lives - so the page no longer drops `*` before a write, the evidence reads every workspace,
/// and Health lists a verdict filed away from its proposal with the way to it.
#[test]
fn under_all_the_console_files_where_items_live() {
    let wire = function("wireCuration");
    assert!(
        !wire.contains("ws !== \"*\" && ws !== \"all\") q += \"&workspace=\""),
        "Propose drops `*`, so the server files the merge in the node default"
    );
    assert!(
        function("fetchReviewEvidence").contains("const wsq = ws ? \"&workspace=\""),
        "the evidence under All reads the node default"
    );
    let items = function("signalItems");
    assert!(
        items.contains("curation.stray_verdicts")
            && items.contains("A verdict filed away from its proposal: "),
        "Health does not list a verdict filed away from its proposal"
    );
    let detail = function("signalDetail");
    for part in [
        "where the proposal does not live, so it decides nothing",
        "data-stray=",
        "Show the proposal",
    ] {
        assert!(detail.contains(part), "a stray verdict's item does not say {part}");
    }
    assert!(
        function("wireDecisions").contains("[data-stray]"),
        "Show the proposal does nothing"
    );
}

/// Section 29.16: the same name in other workspaces is said on the local console's sheet, read once
/// per selected entity and kept while the graph holds; All lights the drawn namesakes, a hover without
/// dimming; and the Suggestions row says each entity's workspace and what the shared name means.
#[test]
fn the_same_name_is_said_on_the_local_console() {
    let also = function("alsoNamedHtml");
    for part in [
        "namesakes.tag !== heldGraph.tag",
        "Not compared with other workspaces: ",
        "Also named in ",
        "nsshow",
    ] {
        assert!(also.contains(part), "the sheet's line does not hold {part}");
    }
    let read = function("readNamesakes");
    assert!(read.contains("the workspace list did not load"), "a missing list is not said");
    assert!(
        read.contains("\"&in=\" + encodeURIComponent(w)"),
        "the workspaces are not sent one by one"
    );
    assert!(
        read.contains("focus.id !== id) return;"),
        "an answer for an earlier selection is kept"
    );
    let index = function("namesakesOf");
    assert!(
        index.contains("cur !== \"*\""),
        "namesakes light outside All on the local console"
    );
    assert!(
        function("hoverSet").contains("namesakesOf(hover)"),
        "a hover does not light them"
    );
    assert!(
        function("activeSet").contains("namesakesOf(anchor)"),
        "a selection does not light them"
    );
    let items = function("signalItems");
    assert!(
        items.contains("\"Same name in \" + spaces + \" workspaces: \"")
            && items.contains("a merge cannot join entities of two workspaces"),
        "the shared-name row does not say what it is"
    );
    assert!(
        function("nchipHtml").contains("n.ws ? n.name + \" - \" + n.ws : n.name"),
        "a chip does not say its workspace"
    );
    assert!(
        SCRIPT.contains("a sharing unit. The same name in two workspaces is two entities"),
        "the Workspace line still says no entity is in two"
    );
}

/// Section 29.17: Same as... searches the entity's own workspace, chooses nothing unmoved and offers
/// the merge either way; Keep ... instead reads the reverse before writing, opens it before rejecting,
/// and the console offers no Accept on a merge whose reverse is accepted.
#[test]
fn same_as_and_keep_instead_hold_their_order() {
    assert!(
        PAGE.contains("<dialog id=\"sameDlg\" aria-labelledby=\"sameTitle\">"),
        "no Same as... dialog"
    );
    let open = function("openSameAs");
    assert!(open.contains("\"Search \" + node.workspace") && open.contains("showModal()"));
    let search = SCRIPT
        .find("async function searchSame()")
        .map(|at| &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")])
        .expect("searchSame");
    assert!(
        search.contains("&workspace=\" + encodeURIComponent(sameFor.workspace)")
            && search.contains("h.id !== sameFor.id"),
        "the picker searches beyond the entity's workspace, or offers the entity itself"
    );
    assert!(search.contains("sameSel = -1"), "an answer arrives with a row chosen");
    let render = function("renderSame");
    for part in [
        "Nothing else is in ",
        "No entity in ",
        "\"Merge \"",
        "Merge ${sameFor.name} into ${c.name}",
    ] {
        let part = part.trim_matches('"');
        assert!(render.contains(part), "the picker does not say {part}");
    }
    let keep = SCRIPT
        .find("async function keepInstead(item)")
        .map(|at| &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")])
        .expect("keepInstead");
    let read = keep.find("reverseOf(p, pair.id)").expect("Keep reads the reverse");
    let opened = keep.find("src=reverse").expect("Keep opens the reverse");
    let rejected = keep.find("decision=reject").expect("Keep rejects this one");
    assert!(read < opened && opened < rejected, "Keep rejects before the reverse is there");
    assert!(
        keep.contains("could not be opened\", o.why); return;"),
        "a failed open still rejects"
    );
    let detail = function("itemDetail");
    assert!(
        detail.contains("&& !revAccepted") && detail.contains("Its reverse is accepted."),
        "an Accept is offered while the reverse is accepted"
    );
    assert!(
        function("reviewDecide").contains("rev.state === \"merged\") return;"),
        "A accepts it anyway"
    );
}

/// Section 29.21: what waits elsewhere is read as the tray reads it and said beside the scope's one
/// count; Reject all blocked lists each Blocked item but a recall with its check, rejects only what it
/// listed and is still blocked, says a partial failure, and the page's keys wait while a dialog is open.
#[test]
fn the_person_finds_what_waits() {
    let away = SCRIPT
        .find("async function refreshElsewhere(force)")
        .map(|at| &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")])
        .expect("refreshElsewhere");
    for part in [
        "/api/proposals?workspace=*",
        "/api/curation?workspace=*",
        "document.hidden",
        "Date.now() - waitAllAt < 30000",
    ] {
        assert!(
            away.contains(part),
            "what waits elsewhere is not read as the tray reads it: {part}"
        );
    }
    let pill = function("updateReviewPill");
    assert!(
        pill.contains("away + \" wait elsewhere\""),
        "the pill does not say what waits elsewhere"
    );
    let render = function("renderReview");
    assert!(
        render.contains(" more wait in other workspaces. ")
            && render.contains("Show all workspaces")
    );
    assert!(function("blockedItems").contains("p.state === \"blocked\" && p.kind !== \"recall\""));
    let go = &SCRIPT[SCRIPT
        .find("document.getElementById(\"blockedGo\").onclick")
        .expect("Reject all")..];
    let go = &go[..go.find("\n};\n").expect("it closes")];
    assert!(
        go.contains("if (!now || now.state !== \"blocked\") continue;"),
        "an item no longer blocked is rejected"
    );
    assert!(
        go.contains("\" was\" : \" were\") + \" not:\""),
        "a partial failure is not said"
    );
    assert!(PAGE.contains("<dialog id=\"blockedDlg\" aria-labelledby=\"blockedTitle\">"));
    assert!(PAGE.contains("If a check passes later, the accept it carries still merges it."));
}

/// design-system.md Section 29.22: a kind with no commit effect yet offers no Accept, says why where
/// the diff would be, and its row, hints, Reject notice and map preview say so too; the decision
/// itself checks the mark, so A does nothing.
#[test]
fn the_console_says_what_has_no_effect() {
    assert!(SCRIPT.contains(
        "const NO_EFFECT_SAID = { tbox_change: [\"a type change\", \"type changes\"], recall: [\"a recall\", \"recalls\"] };"
    ));
    let render = function("renderReview");
    let detail = function("itemDetail");
    let body = format!("{render}{detail}");
    for part in [
        "Nothing yet: ${none[0]} has no effect in this version, so it cannot be accepted until it has one.",
        "&& !revAccepted && !none;",
        "Reject sets it aside, and the log keeps it. It can still be accepted once ${none[1]} have an effect.",
        "A reject is provisional: it can still be accepted once ${none[1]} have an effect.",
    ] {
        assert!(body.contains(part), "the item does not say it: {part}");
    }
    assert!(function("itemTag").contains("<span class=\"rv-st\">No effect yet</span>"));
    let decide = function("reviewDecide");
    assert!(
        decide.contains("if (p.no_effect_yet) return;"),
        "A would cast a merge the fold reads as a comment"
    );
    assert!(function("previewOnMap").contains("\" (no effect yet)\""));
}

/// design-system.md Section 29.23: an act that opens a proposal says what it found - a merge or an
/// un-merge that waits or is in force, a gate act already so - and a found item opens; Keep ...
/// instead reads the state it gets back; the empty band no longer says that none are near.
#[test]
fn the_console_says_what_a_repeat_found() {
    let found = function("foundSaid");
    for part in [
        "A merge of these two already waits under Needs you.",
        "An un-merge of this merge already waits under Needs you.",
        "These two are already merged.",
        "This merge is already un-merged.",
    ] {
        assert!(found.contains(part), "a found act is not said: {part}");
    }
    let write = function("consoleWrite");
    assert!(
        write.contains("body.found && body.state === \"merged\" && typeof said !== \"function\"")
    );
    assert!(write.contains("\"Nothing changed: this is already so.\""));
    // Section 29.25: Un-merge opens the split it proposed, found or new.
    assert!(
        function("proposeSplit").contains("if (o && o.proposal_id) openReviewItem(o.proposal_id);")
    );
    assert!(SCRIPT.contains(
        "b => foundSaid(b, \"merge\") || \"Merge proposed - it waits under Needs you for a verdict\""
    ));
    let keep = function("keepInstead");
    assert!(keep.contains("state: o.body.state || \"open\""), "Keep assumes what it opened");
    assert!(keep.contains("waits under Needs you, but a check holds it back."));
    assert!(function("openSameMerge")
        .contains("\"A merge of these two already waits under Needs you.\""));
    // Section 29.27: the band says what it held back, apart from absence.
    let band = function("bandEmptyReason");
    for part in [
        "band.held_back != null ? band.held_back : null",
        "already proposed, split apart or held back by your reject",
        "held ? \"none waiting - \" + answered : \"none - no embedding-near pairs\"",
        "none waiting - no near pairs that are not already proposed, split apart or held back by your reject",
    ] {
        assert!(band.contains(part), "the empty band does not say: {part}");
    }
    // Section 29.28: the ladder says what it held back, by name, after what the band says by meaning.
    let ladder = function("ladderEmptyReason");
    for part in [
        "if (!ladder || ladder.held_back == null) return \"\";",
        "alike-named pair${held === 1 ? \" is\" : \"s are\"} already proposed, split apart or held back by your reject",
        "\"none - no two names match once case, spaces and symbols are dropped, a trailing -s or -ies is folded, or an alias is counted\"",
    ] {
        assert!(ladder.contains(part), "the empty ladder does not say: {part}");
    }
    let hint = function("signalHint");
    let (meaning, name) = (
        hint.find("${band}").expect("by meaning"),
        hint.find("${name}").expect("by name"),
    );
    assert!(meaning < name, "by name follows by meaning");
    assert!(hint.contains(
        "!(curation.name_variants || []).length ? ladderEmptyReason(curation.name_ladder)"
    ));
}
