//! docs/design-system.md Section 29.3, step 42: the Review page closes its loop. Source tripwires
//! over the viewer's script, page and stylesheet; the server's half - labels, the undo read and the
//! console's rationale - is `a_decided_confirmation_names_its_source_and_says_its_undo` in http.rs,
//! and the fold, the undo and the curation rows are in the engine's policy cases.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");
const CSS: &str = include_str!("../assets/viewer.css");

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
    assert!(parts.contains("Reading what undoing this moves back..."));
    assert!(parts.contains("is not offered: what it would move back could not be read."));
    assert!(parts.contains("Nothing to undo: another decision holds this source where it is."));
    assert!(
        parts.contains("This demotion was cast by "),
        "a demotion's undo does not say who cast it"
    );
    // One act per target, each a console write that says what it did; a partial stop says so (U1).
    let undo = function("undoDecided");
    assert!(undo.contains(
        "consoleWrite(\"/api/resolve\", settleQuery(a.observation, a.tier, a.kind, rationale)"
    ));
    assert!(undo.contains("\"Undo stopped partway\""), "a partial undo is not said");
    // The notices name the item; a reject's offers nothing, since Accept after all decides.
    let decide = function("reviewDecide");
    assert!(decide.contains("\"Accepted: \" + title"));
    assert!(decide.contains("{ label: \"Un-merge\", decided: true, run: () => proposeSplit(p) }"));
    assert!(decide.contains("\"Rejected: \" + title, null, \"Accept after all is under Decided.\""));
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
    let render = function("renderReview");
    assert!(
        render.contains("c.className = \"rv-ct\" + (id === \"needs\" ? \"\" : \" muted\")"),
        "another segment's count asks like Needs you's"
    );
    assert!(CSS.contains(".rv-segb .rv-ct.muted { color:var(--muted); }"));
    assert!(render.contains("\"Nothing waits for you in any workspace.\""));
    // The tray opens Review at All workspaces, the scope its count is of.
    assert!(function("openReviewAt").contains("location.hash = \"#review\""));
}

/// The keys act from the first press: opening the page focuses the selected row, a contested item
/// has Skip and its values as 1 to 9, and they act only while focus is in the list or the item.
#[test]
fn the_keys_act_from_the_first_press() {
    assert!(SCRIPT.contains(
        "window.addEventListener(\"hashchange\", () => { recallArmed = null; if (reviewOpen()) reviewFocus = true;"
    ));
    assert!(SCRIPT.contains(
        "else if (/^[1-9]$/.test(e.key) && item && item.kind === \"contested\" && surfaceKind === \"local\")"
    ));
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

/// The notices sit over the list column on the Review page, and the Log's decision rows are in its
/// full ink.
#[test]
fn review_notices_and_log_rows() {
    assert!(CSS.contains("body.reviewing #toasts { left:180px; top:auto;"));
    assert!(!CSS.contains(".otext.ev { opacity"), "decision rows are still faint");
    assert!(function("renderReview")
        .contains("document.body.classList.toggle(\"reviewing\", reviewOpen());"));
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
    assert!(!detail.contains("p.self_attested"), "a mark every proposal carries is drawn");
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
    assert!(CSS.contains(".evcols {") && CSS.contains(".ckrow.ok .i { color:var(--ok-ink); }"));
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
