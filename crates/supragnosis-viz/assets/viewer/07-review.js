// --- The Review page (docs/design-system.md Section 7, U4) ---------------------------------------
// Everything that waits for a person's decision, in one place: Needs you (suspected secrets, open and
// blocked proposals, contested beliefs), Suggestions and Health. It is a route of this page (#review),
// so it works in the app and in a browser alike. A decision acts on one item,
// and only once that item's belief diff and checks are on screen; there is no decide-all (P23).
const reviewEl = document.getElementById("reviewPage");
let reviewSeg = "needs";      // needs | decided | suggestions | health
let reviewSel = null;         // the key of the selected item
let reviewView = null;        // {id, diff, checks} of the selected proposal, once fetched
let recallArmed = null;       // a recall's id, after its first Accept (a recall asks twice)
function reviewOpen() { return location.hash === "#review"; }
// The title bar names the place on screen (Section 6, U26): the scope on the Graph page; Review and
// its scope on the Review page, where "< Graph" takes the filters toggle's place and the panel toggles,
// which have no panel there, go.
function renderPlace() {
  const onReview = reviewOpen();
  const title = document.getElementById("placeTitle");
  if (onReview) {
    const scope = document.createElement("span");
    scope.className = "tscope";
    scope.textContent = statsScope(currentWs());
    title.replaceChildren(word("review"), scope);
  } else title.textContent = statsScope(currentWs());
  document.getElementById("backGraph").hidden = !onReview;
  document.getElementById("panelL").hidden = onReview;
  document.getElementById("panelR").hidden = onReview;
}
// Cmd+[ goes back, as on a Mac; Esc does too (Section 6a, U26).
addEventListener("keydown", e => {
  if (e.key === "[" && (e.metaKey || e.ctrlKey) && reviewOpen()) { e.preventDefault(); location.hash = ""; }
});
// A proposal's kind as its title says it, by the kind's id.
const KINDS = { entity_merge: "nounMerge", entity_split: "nounUnmerge", claim_promotion: "titleConfirm", claim_demotion: "titleDemote", tbox_change: "titleTypeChange", recall: "nounRecall" };
// A kind with no commit effect yet, in the screen's words (Section 29.22): the server marks the
// proposal no_effect_yet, and it cannot be accepted until its kind has an effect. One and many.
const NOEFFECT = { tbox_change: ["noEffectType", "noEffectTypes"], recall: ["noEffectRecall", "noEffectRecalls"], other: ["noEffectKind", "noEffectKinds"] };
function noEffectSaid(p) {
  const k = Object.hasOwn(NOEFFECT, p.kind) ? p.kind : "other";
  return [word(NOEFFECT[k][0]), word(NOEFFECT[k][1])];
}
// A proposal's state said as its outcome (Section 29.3, U17); the ids stay the fold's.
const OUTCOME = { merged: word("stateAccepted"), rejected: word("stateRejected"), withdrawn: word("stateWithdrawn"), blocked: word("stateBlocked") };
// How a proposal's target is said: the server's label (it names a merged-away entity, and a gate
// act's source in its own words), then the graph's name; a hash id nothing can name is left out.
function targetSaid(p, i) {
  const l = p.labels && p.labels[i];
  if (l) return l;
  const id = (p.targets || [])[i], n = id && nodeById(id);
  return n ? n.name : (id && !HASH_ID.test(id) ? id : null);
}
function intoSaid(p) {
  if (p.into_label) return p.into_label;
  const n = p.into && nodeById(p.into);
  return n ? n.name : (p.into && !HASH_ID.test(p.into) ? p.into : null);
}
// A merge's names, what goes and what it goes into; null when either cannot be named.
function mergeNames(p) {
  if (p.kind !== "entity_merge" || !p.into) return null;
  const gone = (p.targets || []).map((_, i) => targetSaid(p, i)).filter((s, i) => s && p.targets[i] !== p.into), b = intoSaid(p);
  return gone.length && b ? { a: gone.join(", "), b } : null;
}
// One title per proposal, the same under Needs you and Decided: "Merge PostgreSQL into Postgres",
// Confirm a claim: "The ledger is..." (Ledger).
function proposalTitle(p) {
  const said = (p.targets || []).map((_, i) => targetSaid(p, i));
  const merge = mergeNames(p);
  if (merge) return word("mergeInto", merge);
  if (p.kind === "entity_split") {
    // The merge it undoes in the page's words; the server's label, which is English, when the page
    // does not hold that merge.
    const m = mergeNames(proposals.find(q => q.id === (p.targets || [])[0]) || {});
    if (m) return word("unmergeInto", m);
    if (said[0]) return word("unmergeName", { name: said[0] });
  }
  const named = said.filter(Boolean);
  const kind = Object.hasOwn(KINDS, p.kind) ? word(KINDS[p.kind]) : p.kind;
  return named.length ? word("saidOn", { said: kind, what: named.join(", ") }) : kind;
}
// A contested row says what it asks: "Ledger's type: Table or Concept?", two values at most.
function contestedTitle(c) {
  if (c.field === "definition") return word("whichMeaning", { name: c.name });
  const values = [c.current, ...(c.competitors || []).map(x => x.value)];
  const two = { a: values[0], b: values[1], n: values.length - 2 };
  const shown = values.length > 2 ? word("valuesOrMore", two) : values.length === 2 ? word("valuesOr", two) : values[0];
  return word("whichType", { name: c.name, values: shown });
}
function needsItems() {
  const items = [];
  for (const f of (curation && curation.secrets) || []) items.push({ key: "s:" + f.observation, kind: "secret", f });
  // Every open proposal is listed, whatever its diff (U4): one whose diff cannot be computed says so,
  // since nothing else would ever close it. A blocked one waits for a person too - to reject it.
  for (const p of proposals.filter(p => p.state === "open" || p.state === "blocked")) items.push({ key: "p:" + p.id, kind: "proposal", p });
  // Only a belief that ties on trust waits for a person (resolution.md Section 4.1), an entity's type
  // or a type's definition (IR5); one that trust settled is listed under Health, so a confirmation
  // takes its item off the count (U20).
  for (const c of ((curation && curation.contradictions) || []).filter(c => c.contested)) items.push({ key: "c:" + c.id, kind: "contested", c });
  return items;
}
function decidedItems() {
  return proposals.filter(p => p.state !== "open" && p.state !== "blocked").map(p => ({ key: "p:" + p.id, kind: "proposal", p }));
}
// Every segment is a queue (Section 7): its items, in the order the list draws them.
function segItems(seg = reviewSeg) {
  return seg === "needs" ? needsItems() : seg === "decided" ? decidedItems() : signalItems(seg);
}
// What waits in other workspaces (Section 29.21): the reads the tray makes, at
// most every 30 seconds while the page is seen and again after a console act; nothing is said about
// elsewhere when the proposal read fails, and the proposals alone count when the curation read does.
let waitAll = null, waitAllAt = 0;
async function refreshElsewhere(force) {
  if (document.hidden) return;
  if (!force && Date.now() - waitAllAt < 30000) return;
  waitAllAt = Date.now();
  const cur = currentWs();
  if (cur === "*" || cur === "all") { waitAll = null; updateReviewPill(); return; }
  try {
    const r = await fetch("/api/proposals?workspace=*", { cache: "no-store" });
    if (!r.ok) { waitAll = null; updateReviewPill(); return; }
    const list = await r.json();
    let n = (Array.isArray(list) ? list : []).filter(p => p.state === "open" || p.state === "blocked").length;
    try {
      const c = await (await fetch("/api/curation?workspace=*", { cache: "no-store" })).json();
      n += (c.secrets || []).length + (c.contradictions || []).filter(x => x.contested).length;
    } catch (_) { /* the proposals alone count */ }
    waitAll = n;
  } catch (_) { waitAll = null; }
  updateReviewPill();
  if (reviewOpen()) renderReview();
}
// How many wait in the other workspaces, or 0 when unknown or none.
function waitElsewhere() { return waitAll === null ? 0 : Math.max(0, waitAll - reviewCount()); }
function reviewCount() {
  // Before the first curation read, the graph's own contested count stands in for the contested rows.
  return needsItems().length + (curation ? 0 : contestedCount);
}
function itemTitle(it) {
  if (it.kind === "secret") return word("credentialInLog");
  if (it.kind === "contested") return contestedTitle(it.c);
  if (it.kind === "signal") return it.title;
  return proposalTitle(it.p);
}
function itemTag(it) {
  if (it.kind === "secret") return html`<span class="rv-k bad">${word("secretTag")}</span>`;
  if (it.kind === "contested") return html`<span class="rv-k bad">${word("contestedItemTag")}</span>`;
  if (it.kind === "signal") return "";
  // Only what the title does not say: under Needs you that it is blocked, under Decided its outcome.
  const st = it.p.state;
  if (st === "open") return it.p.no_effect_yet ? html`<span class="rv-st">${word("noEffectYetTag")}</span>` : "";
  return html`<span class="rv-st ${st}">${OUTCOME[st] || st}</span>`;
}
function updateReviewPill() {
  const pill = document.getElementById("reviewPill");
  if (!pill) return;
  const n = reviewCount();
  const contested = curation ? needsItems().filter(it => it.kind === "contested").length : contestedCount;
  // Always the way to Review (Section 6, U20), with one count: what waits under Needs you, and how
  // many of those are contested beliefs.
  pill.hidden = false;
  const away = n ? 0 : waitElsewhere();
  pill.className = "spill" + (n || away ? " warn" : "");
  // One count, the scope's; with nothing here, what waits elsewhere (Section 29.21).
  const needs = word(n === 1 ? "needsYouOne" : "needsYouN", { n });
  pill.textContent = n ? (contested ? word("needsContested", { needs, n: contested }) : needs)
    : away ? word("waitElsewhere", { n: away }) : word("review");
}
let reviewSig = "";   // what the page last drew - a poll that changes nothing must not redraw it
let reviewIndex = 0;  // where the selected item stood: when it leaves, the next one takes its place
// Put focus on the selected row at the next draw that has one: on opening - a page loaded on #review
// included, which fires no hashchange - and after a decision. Focus the person moves cancels it.
let reviewFocus = location.hash === "#review";
if (reviewEl) reviewEl.addEventListener("focusin", () => { reviewFocus = false; });
let undoView = null;  // {id, state: reading|ok|failed, acts, diff, checks} - the selected Decided gate act's reverse
// What the redraw signature holds of an item: its key and the state that changes what it shows.
function itemSig(it) {
  if (it.kind === "contested") return [it.key, it.c.contested, it.c.current];
  if (it.kind === "proposal") return [it.key, it.p.state, it.p.verdicts, it.p.labels || null];
  return [it.key];
}
// The Review page is a full-screen route; everything else on the page is inert while it is open, so
// Tab and the screen reader stay inside it. Runs on close too (reviewEl.hidden flips first).
function setReviewInert(on) {
  for (const id of ["c", "dockL", "dockR", "detail", "log", "strip", "minimap", "hud"]) {
    const el = document.getElementById(id);
    if (el) el.inert = !!on;
  }
}
function renderReview() {
  updateReviewPill();
  if (!reviewEl) return;
  reviewEl.hidden = !reviewOpen();
  setReviewInert(reviewOpen());
  document.body.classList.toggle("reviewing", reviewOpen());
  // The window's document title names the page as the title bar does (Section 29.6, U26).
  document.title = word(reviewOpen() ? "reviewTitle" : "pageTitle");
  if (!reviewOpen()) { reviewSig = ""; return; }
  const items = segItems();
  // Redraw only when what is shown changed: a redraw replaces the buttons under the pointer and the
  // keyboard focus, which every 2.5-second poll would otherwise take away. Each item's state is in
  // it, so a decided item leaves at the next read (Section 29.3).
  // What waits elsewhere and the Blocked count draw lines of their own (Section 29.21).
  const sig = JSON.stringify([reviewSeg, reviewSel, recallArmed, currentWs(), items.map(itemSig), waitElsewhere(), blockedItems().length,
    ["needs", "decided", "suggestions", "health"].map(s => s === reviewSeg ? items.length : segItems(s).length),
    reviewView && [reviewView.id, (reviewView.checks || []).length], undoView && [undoView.id, undoView.state],
    reviewEvidence && [reviewEvidence.id, reviewEvidence.cols.map(c => c.list.length), !!reviewEvidence.opened]]);
  if (sig === reviewSig) return;
  reviewSig = sig;
  // The segments' counts: only Needs you's asks; the others are drawn muted (Section 29.3).
  const segs = [["needs", word("segNeeds")], ["decided", word("segDecided")], ["suggestions", word("segSuggestions")], ["health", word("segHealth")]];
  const segEl = document.getElementById("rvSeg");
  segEl.textContent = "";
  let segSel = null;
  for (const [id, label] of segs) {
    const n = id === reviewSeg ? items.length : segItems(id).length;
    const b = document.createElement("button");
    b.type = "button"; b.className = "rv-segb" + (reviewSeg === id ? " on" : "");
    b.append(label);
    if (n) { const c = document.createElement("span"); c.className = "rv-ct" + (id === "needs" ? "" : " muted"); c.textContent = String(n); b.append(" ", c); }
    b.onclick = () => { reviewSeg = id; reviewIndex = 0; renderReview(); };
    segEl.appendChild(b);
    if (reviewSeg === id) segSel = b;
  }
  markTabs(segEl, [...segEl.children], segSel);
  // The strip keeps its element while its buttons are redrawn, so its arrow keys are wired once.
  if (!segEl.dataset.arrows) { arrowTabs(segEl); segEl.dataset.arrows = "1"; }
  renderPlace();
  const list = document.getElementById("rvList"), detail = document.getElementById("rvDetail");
  if (!items.some(it => it.key === reviewSel)) reviewSel = items.length ? items[Math.min(reviewIndex, items.length - 1)].key : null;
  reviewIndex = Math.max(0, items.findIndex(it => it.key === reviewSel));
  // A redraw replaces the focused item's button, and a decision replaces the button that cast it: in
  // either case focus goes on to the item now selected - the next one, after a decision.
  const hadFocus = reviewFocus || !!(document.activeElement && document.activeElement.closest
    && document.activeElement.closest("#rvList, #rvDetail [data-decide], #rvDetail [data-skip], #rvDetail [data-settle], #rvDetail [data-undo]"));
  const all = currentWs() === "*" || currentWs() === "all";
  const away = all || reviewSeg !== "needs" ? 0 : waitElsewhere();
  const nothingHere = word("nothingWaitsHere"), awaySaid = away ? word("waitInOthers", { n: away }) : "";
  const empty = { needs: all ? word("nothingWaitsAny") : (away ? nothingHere + " " + awaySaid : nothingHere),
    decided: word("nothingDecided"), suggestions: word("noSuggestions"), health: word("nothingToReport") }[reviewSeg];
  const blocked = reviewSeg === "needs" ? blockedItems() : [];
  const head = html`${away && items.length ? html`<div class="rv-else">${word("moreWaitInOthers", { n: away })} <button type="button" class="linkish" data-allws="1">${word("showAllWorkspaces")}</button></div>` : ""}${
    blocked.length >= 2 ? html`<div class="rv-else">${word("acceptedButHeld", { n: blocked.length })} <button type="button" class="linkish" data-blocked="1">${word("rejectAllBlocked")}</button></div>` : ""}`;
  list.innerHTML = html`${signalHint(reviewSeg)}${head}${items.length
    ? items.map(it => html`<button type="button" class="rv-item${it.key === reviewSel ? " on" : ""}" data-key="${it.key}">${itemTag(it)}<span class="rv-t">${itemTitle(it)}</span></button>`)
    : html`<div class="empty">${empty}${away ? html` <button type="button" class="linkish" data-allws="1">${word("showAllWorkspaces")}</button>` : ""}</div>`}`;
  list.querySelectorAll(".rv-item").forEach(b => { b.onclick = () => { reviewSel = b.dataset.key; renderReview(); }; });
  list.querySelectorAll("[data-allws]").forEach(b => { b.onclick = () => showAllForReview(); });
  list.querySelectorAll("[data-blocked]").forEach(b => { b.onclick = () => openBlocked(); });
  // Kept until a row takes it: the first draw can come before the proposals have been read.
  if (hadFocus) { const on = list.querySelector(".rv-item.on"); if (on) { on.focus(); reviewFocus = false; } }
  const it = items.find(x => x.key === reviewSel);
  detail.innerHTML = html`${it ? itemDetail(it) : html`<div class="empty">${items.length ? word("selectItem") : empty}</div>`}`;
  if (it && it.kind === "proposal" && (!reviewView || reviewView.id !== it.p.id)) fetchReviewView(it.p.id);
  if (it && it.kind === "proposal" && undoable(it.p) && (!undoView || undoView.id !== it.p.id)) fetchUndoView(it.p.id);
  // A contested item's values confirm as 1 to 9, in the order shown (U19: only while focus is here).
  if (it && it.kind === "contested") detail.querySelectorAll('[data-settle="ask"]').forEach((b, i) => { if (i < 9) b.setAttribute("aria-keyshortcuts", String(i + 1)); });
  wireCuration(detail);
  wireDecisions(detail, it);
  // The evidence's rows open their cards; a linked item is selected in place (Section 29.7).
  const ev = it && it.kind === "proposal" && reviewEvidence && reviewEvidence.id === it.p.id ? reviewEvidence : null;
  if (ev) detail.querySelectorAll(".evcol").forEach((col, ci) => {
    const list = (ev.cols[ci] || {}).list || [];
    col.querySelectorAll(".obs").forEach((row, i) => { const o = list[i]; if (o && o.id === row.dataset.id) row.querySelector(".ohead").onclick = () => showObsCard(o); });
  });
  detail.querySelectorAll("[data-review]").forEach(b => { b.onclick = () => { reviewSeg = "needs"; reviewSel = b.dataset.review; reviewFocus = true; renderReview(); }; });
  // Each value's source, as the inspector says it (design-system.md Section 29.7).
  if (it && it.kind === "contested" && it.c.field !== "definition") fillSettleWho(detail, it.c.id);
}
// Skip, as a button and as J, wherever an item is decided one at a time (Section 7).
function skipButton() {
  return html`<button type="button" class="btn" data-skip="1" aria-keyshortcuts="J">${word("skip")} <kbd aria-hidden="true">J</kbd></button>`;
}
function itemDetail(it) {
  if (it.kind === "secret") {
    const f = it.f;
    // The pattern and the field are the scanner's names, said as it names them.
    return [html`<h3>${word("credentialInLog")}</h3>`,
      html`<p class="why">${word("secretWhere", { pattern: f.pattern, field: f.field, at: String(f.at | 0) })}</p>`,
      html`<p class="why">${word("secretWhat")}</p>`,
      html`<div class="rv-acts"><button type="button" class="btn" data-obs="${f.observation}">${word("openObservation")}</button>${skipButton()}</div>`];
  }
  if (it.kind === "contested") {
    const c = it.c, def = c.field === "definition";
    return [html`<h3>${contestedTitle(c)}</h3>`,
      html`<p class="why">${word(def ? "tieDefinitions" : "tieValues")}</p>`,
      html`<div class="contested hot">${contestedRows({ name: c.name, field: def ? "definition" : (c.field === "kind" ? "type" : c.field), entity: def ? "" : c.id,
        current: c.current, curObs: c.kind_source, tier: "", competitors: c.competitors || [], contested: c.contested, ws: c.workspace || (nodeById(c.id) || {}).workspace || "" })}</div>`,
      html`<div class="rv-acts">${def ? "" : html`<button type="button" class="btn" data-map="1">${word("showOnMap")}</button>`}${skipButton()}</div>`,
      html`<p class="hint"><kbd>1</kbd> <kbd>2</kbd> ${word("confirmAValue")} <kbd>J</kbd> <kbd>K</kbd> ${word("moveBetween")}</p>`];
  }
  if (it.kind === "signal") return signalDetail(it);
  const p = it.p, v = reviewView && reviewView.id === p.id ? reviewView : null;
  const failing = v && v.checks ? v.checks.filter(c => c.blocking && !c.passed) : [];
  const decided = p.state === "merged" || p.state === "rejected" || p.state === "withdrawn";
  const parts = [html`<h3>${itemTitle(it)}</h3>`];
  if (p.rationale) parts.push(html`<p class="why">"${p.rationale}"</p>`);
  const ev = reviewEvidence && reviewEvidence.id === p.id ? reviewEvidence : null;
  // A decided item's diff is worked out against today's beliefs, so it says so (Section 29.3); a
  // merge's leads with what happens to identity (Section 29.7).
  const none = p.no_effect_yet ? noEffectSaid(p) : null;   // no Accept until its kind has an effect (Section 29.22)
  parts.push(html`<div class="sec">${word(decided ? "wouldChangeNow" : "whatChanges")}</div>`, none
    ? html`<div class="dnote">${word("noEffectYet", { what: none[0] })}</div>`
    : [identityHtml(p, ev), diffHtml(v ? v.diff : null)]);
  parts.push(html`<div class="sec">${word("checks")}</div>`);
  if (!v) parts.push(html`<div class="dnote">${word("readingChecks")}</div>`);
  else {
    // A failing check's detail is the server's sentence, shown as it wrote it (docs/i18n.md Section 5).
    if (failing.length) parts.push(html`<div class="rv-block"><b>${word("blockedHead")}</b> ${failing.map(c => html`<div>${checkSaid(p, c)}: ${c.detail}</div>`)}<div>${word("blockedWhat")}</div></div>`);
    parts.push(checkListHtml(p, v.checks));
  }
  parts.push(html`<div class="sec">${word("evidence")}</div>`, evidenceHtml(p, ev));
  // Who asked: the opening event in the who-line grammar, opening its card; "self-attested", which
  // every opened proposal carries, is not drawn (Section 29.7).
  const opened = ev && ev.opened;
  parts.push(html`<div class="sec">${word("whoAsked")}</div>`, opened
    ? html`<div class="hint"><button type="button" class="linkish" data-obs="${opened.id}">${whoLine(opened.effective_tier, authorOf(opened))}</button></div>`
    : html`<div class="hint">${p.opened_at ? new Date(p.opened_at).toLocaleString(LANG) : ""}</div>`);
  const acts = [];
  const ready = !!v;   // the diff and checks are on screen
  const pair = keepPair(p), rev = pair ? reverseOf(p, pair.id) : null;
  const revAccepted = !!rev && rev.state === "merged";   // no Accept of any kind: no cycle (Section 29.17)
  const accept = (p.state === "open" || p.state === "rejected") && !revAccepted && !none;
  if (accept) {
    const armed = recallArmed === p.id;
    const label = p.kind === "recall" && armed ? word("confirmRecall") : word(p.state === "rejected" ? "acceptAfterAll" : "accept");
    const off = !ready || failing.length;
    acts.push(html`<button type="button" class="btn p" data-decide="merge" aria-keyshortcuts="A"${off ? html` disabled` : ""}>${label} <kbd aria-hidden="true">A</kbd></button>`);
  }
  if (p.state === "open" || p.state === "blocked") acts.push(html`<button type="button" class="btn" data-decide="reject" aria-keyshortcuts="R"${ready ? "" : html` disabled`}>${word("reject")} <kbd aria-hidden="true">R</kbd></button>`);
  // Keep the other name (Section 29.17): an open merge of two, once its diff and checks show.
  if (pair && p.state === "open" && !revAccepted) acts.push(html`<button type="button" class="btn" data-keep="1"${ready ? "" : html` disabled aria-describedby="keepWait"`}>${word("keepInstead", { name: pair.name })}</button>`,
    ready ? "" : html`<span class="dnote" id="keepWait">${word("waitingChecks")}</span>`);
  if (p.state === "open" || p.state === "blocked") acts.push(skipButton());
  const reversed = proposals.some(q => q.kind === "entity_split" && q.state === "merged" && (q.targets || []).includes(p.id));
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) acts.push(html`<button type="button" class="btn" data-unmerge="1">${word("nounUnmerge")}</button>`);
  const u = undoable(p) ? undoParts(p) : null;
  if (u) acts.push(u.button);
  acts.push(html`<button type="button" class="btn" data-map="1">${word("showOnMap")}</button>`);
  parts.push(html`<div class="rv-acts">${acts}</div>`);
  if (pair && p.state === "open" && !revAccepted) parts.push(html`<p class="hint">${word("keepInsteadWhy", { name: pair.name })}</p>`);
  if (revAccepted && (p.state === "open" || p.state === "rejected")) parts.push(html`<p class="hint">${word("reverseAccepted")}</p>`);
  if (u) parts.push(u.note);
  if (none && p.state === "open") parts.push(html`<p class="hint">${word("rejectSetsAside", { what: none[1] })}</p>`);
  else if (none && p.state === "rejected") parts.push(html`<p class="hint">${word("rejectUntilEffect", { what: none[1] })}</p>`);
  else if (p.state === "rejected" && p.merge_verdict) parts.push(html`<p class="hint">${word("ifCheckPasses")}</p>`);
  else if (p.state === "rejected") parts.push(html`<p class="hint">${word("rejectProvisional")}</p>`);
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) parts.push(html`<p class="hint">${word("unmergeWhy")}</p>`);
  if (p.state === "merged" && !undoable(p) && !(p.kind === "entity_merge" && !reversed)) {
    parts.push(html`<p class="hint">${word(p.kind === "entity_split" ? "mergeAgainNew" : "undoNew")}</p>`);
  }
  parts.push(html`<p class="hint">${accept ? word("acceptCasts") + " " : ""}<kbd>J</kbd> <kbd>K</kbd> ${word("moveBetween")}</p>`);
  return parts;
}
async function fetchReviewView(id) {
  const ws = wsInput.value.trim();
  let q = "?id=" + encodeURIComponent(id);
  if (ws && ws !== "*" && ws !== "all") q += "&workspace=" + encodeURIComponent(ws);
  try {
    const r = await fetch("/api/proposal" + q, { cache: "no-store" });
    if (!r.ok) return;
    const view = await r.json();
    if (!view || view.id !== id) return;
    reviewView = { id, diff: view.belief_diff || null, checks: view.checks || [] };
    fetchReviewEvidence(view);
    if (proposalSel && proposalSel.id === id) { proposalDiff = reviewView.diff; proposalChecks = reviewView.checks; }
    renderReview();
  } catch (e) { /* the page still lists the item - it just cannot be decided yet */ }
}
// What a decision rests on (design-system.md Section 29.7): for a merge each entity's supporting
// observations, for a gate act the observations it moves, and the opening event - read when the item
// is selected, each by the read that answers it alone.
let reviewEvidence = null;   // {id, cols: [{id, name, list}], opened}
async function fetchReviewEvidence(p) {
  const id = p.id, ws = wsInput.value.trim();
  // Under All the evidence is every workspace's, as the scope on screen is (Section 29.15).
  const wsq = ws ? "&workspace=" + encodeURIComponent(ws) : "";
  const cols = [];
  try {
    if (p.kind === "entity_merge") {
      const ids = [...new Set([...(p.targets || []), ...(p.into ? [p.into] : [])])];
      for (const e of ids) {
        const r = await fetch("/api/observations?entity=" + encodeURIComponent(e) + wsq, { cache: "no-store" });
        const list = r.ok ? await r.json() : [];
        const n = nodeById(e);
        cols.push({ id: e, name: n ? n.name : labelOf(p, e), list: Array.isArray(list) ? list : [] });
      }
    } else if (p.kind === "claim_promotion" || p.kind === "claim_demotion") {
      const list = [];
      for (const t of p.targets || []) { const o = await observationById(t); if (o) list.push(o); }
      cols.push({ id: "", name: "", list });
    }
    const opened = await observationById(id);
    if (reviewView && reviewView.id !== id) return;   // the selection moved on
    reviewEvidence = { id, cols, opened };
    renderReview();
  } catch (_) { /* the item stands without it */ }
}
function labelOf(p, e) {
  const i = (p.targets || []).indexOf(e);
  return (i >= 0 && p.labels && p.labels[i]) || (e === p.into && p.into_label) || String(e).slice(0, 8);
}
// A merge's identity, before the belief rows: the entity that goes becomes an alias of the one kept,
// its observations count for it, and a contested field of the kept one stays contested.
function identityHtml(p, ev) {
  if (p.kind !== "entity_merge" || !p.into) return html``;
  const kept = nodeById(p.into), keptName = kept ? kept.name : labelOf(p, p.into);
  const rows = [];
  for (const t of (p.targets || []).filter(t => t !== p.into)) {
    const n = nodeById(t), name = n ? n.name : labelOf(p, t);
    rows.push(html`<div class="drow ident">${word("leavesMap", { name, kept: keptName })}</div>`);
    const col = ev && ev.cols.find(c => c.id === t);
    const count = col ? { n: col.list.length, kept: keptName } : null;
    if (count && count.n) rows.push(html`<div class="drow ident">${word(count.n === 1 ? "countsForOne" : "countsForN", count)}</div>`);
  }
  const still = ((curation && curation.contradictions) || []).find(c => c.id === p.into && c.contested);
  const stays = still ? { kept: keptName, field: fieldSaid(still.field).toLowerCase() } : null;
  if (still) rows.push(html`<div class="drow ident">${word("staysContested", stays)} <button type="button" class="linkish" data-review="${"c:" + still.id}">${word("alsoUnderNeeds")}</button></div>`);
  return html`<div class="ddiff">${rows}</div>`;
}
// Every check, in words keyed by the proposal's kind, the check's name and whether it passed
// (Sections 29.7 and 29.15), so a failed check never reads as its passing words; one the page has
// no words for as the engine names it, with the engine's detail when it fails.
const CHECK_SAID = {
  "entity_merge|referential integrity": (p, ok) => ok ? word("ckAllInWs", { n: (p.targets || []).length + (p.into && !(p.targets || []).includes(p.into) ? 1 : 0) }) : word("ckNotAllInWs"),
  "entity_merge|distinct targets": (p, ok) => word(ok ? "ckTwoTargets" : "ckFewerTargets"),
  "entity_merge|canonical target named": (p, ok) => word(ok ? "ckNamesKept" : "ckKeptMissing"),
  "tbox_change|t-box axis consistency": (p, ok) => word(ok ? "ckTypesConsistent" : "ckBothAxes"),
  "entity_split|reversible target": (p, ok) => word(ok ? "ckNamesMerge" : "ckMergeMissing"),
  "claim_promotion|referential integrity": (p, ok) => word(ok ? "ckObsInWs" : "ckObsNotInWs"),
  "claim_demotion|referential integrity": (p, ok) => word(ok ? "ckObsInWs" : "ckObsNotInWs"),
  "claim_promotion|tier stated": (p, ok) => word(ok ? "ckTierStated" : "ckTierUnstated"),
  "claim_demotion|tier stated": (p, ok) => word(ok ? "ckTierStated" : "ckTierUnstated"),
};
function checkSaid(p, c) {
  const f = CHECK_SAID[p.kind + "|" + c.name];
  return f ? f(p, c.passed) : c.passed ? c.name : c.name + ": " + c.detail;
}
function checkListHtml(p, cs) {
  const rows = (cs || []).map(c => {
    if (c.name === "trust profile") {
      const said = String(c.detail || "").split(", ").filter(Boolean).map(x => { const [n, t] = x.split(" "); return n + " " + tierName(t); }).join(", ");
      return html`<div class="ckrow"><span class="i i-info" aria-hidden="true"></span><span>${word("sourcesAre", { said: said || word("none") })}</span></div>`;
    }
    return html`<div class="ckrow ${c.passed ? "ok" : "bad"}"><span class="i ${c.passed ? "i-circle-check" : "i-circle-alert"}" role="img" aria-label="${word(c.passed ? "passed" : "failed")}"></span><span>${checkSaid(p, c)}</span></div>`;
  });
  return html`<div class="cklist">${rows}</div>`;
}
// The evidence in the Log's row anatomy, each row opening its card: one column per entity for a merge.
function evidenceHtml(p, ev) {
  if (!ev) return html`<div class="dnote">${word("readingEvidence")}</div>`;
  if (!ev.cols.length) return html`<div class="dnote">${word("noOwnEvidence")}</div>`;
  return html`<div class="evcols">${ev.cols.map(c => html`<div class="evcol">${c.name ? html`<div class="evh">${c.name} <span class="ct">${c.list.length}</span></div>` : ""}${
    c.list.map((o, i, l) => obsRowHtml(o, i, l))}</div>`)}</div>`;
}
// A merged gate act has a reverse on its Decided item (Section 29.3).
function undoable(p) { return p.state === "merged" && (p.kind === "claim_promotion" || p.kind === "claim_demotion"); }
function undoLabel(p) {
  if (p.kind === "claim_demotion") return word("undoDemotion");
  return word(String(p.tier) === "human_confirmed" ? "undoConfirmation" : "undoPromotion");
}
// The acts that reverse it, with their diff and checks, read when the item is selected (U4).
async function fetchUndoView(id) {
  undoView = { id, state: "reading" };
  try {
    const r = await fetch("/api/resolve/undo?proposal=" + encodeURIComponent(id), { cache: "no-store" });
    const u = r.ok ? await r.json() : null;
    if (!undoView || undoView.id !== id) return;
    undoView = u ? { id, state: "ok", acts: u.acts || [], diff: u.belief_diff || null, checks: u.checks || [] } : { id, state: "failed" };
  } catch (_) { if (undoView && undoView.id === id) undoView = { id, state: "failed" }; }
  renderReview();
}
// The Undo button and the note beside it: what it moves back, in step 41's words, or why it is not
// offered. It acts only once the read is on screen and no check fails.
function undoParts(p) {
  const u = undoView && undoView.id === p.id ? undoView : null;
  const label = undoLabel(p);
  const off = html` disabled`;
  if (!u || u.state === "reading") return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">${word("readingUndo")}</p>` };
  if (u.state === "failed") return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">${word("undoNotOffered", { label })}</p>` };
  if (!u.acts.length) return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">${word("nothingToUndo")}</p>` };
  const failing = (u.checks || []).filter(c => c.blocking && !c.passed);
  const lines = u.acts.map(a => word(a.kind === "claim_demotion" ? "undoSourceNote" : "undoReturns", { tier: tierName(a.tier) }));
  const moves = ringMoves(u.diff, null);
  const by = p.kind === "claim_demotion" && p.proposer ? word("demotionBy", { who: p.proposer }) : "";
  // A check's name and detail are the server's (docs/i18n.md Section 5).
  const fails = failing.length ? word("cannotCastWhile", { checks: failing.map(c => c.name + ": " + c.detail).join("; ") }) : "";
  const note = [[...new Set(lines)].join(" "), moves.length ? moves.join(". ") + "." : "", by, fails].filter(Boolean).join(" ");
  return { button: html`<button type="button" class="btn" data-undo="1"${failing.length ? off : ""}>${label}</button>`, note: html`<p class="hint">${note}</p>` };
}
// Casts the reverse, one act per target, one after the other; the notice says which targets went
// back when one does not (U1).
async function undoDecided(p) {
  const u = undoView && undoView.id === p.id ? undoView : null;
  if (!u || u.state !== "ok" || !u.acts.length || (u.checks || []).some(c => c.blocking && !c.passed)) return;
  const rationale = word("undid", { said: proposalTitle(p) });
  const said = a => targetSaid(p, (p.targets || []).indexOf(a.observation)) || word("aSource");
  const back = [];
  for (const a of u.acts) {
    const ok = await consoleWrite("/api/resolve", settleQuery(a.observation, a.tier, a.kind, rationale),
      word("undoneAgain", { who: said(a), tier: tierName(a.tier) }), { decided: true });
    if (!ok) {
      const not = u.acts.filter(x => !back.includes(said(x))).map(said).join(", ");
      if (back.length) toast("bad", word("undoPartway"), word("returnedNot", { back: back.join(", "), not }));
      break;
    }
    back.push(said(a));
  }
  undoView = null;
  await poll();
  refreshProposals();
}
// Un-merge opens the split proposal (unmerge.md) and decides nothing; it then waits under Needs you.
async function proposeSplit(p) {
  const ws = wsInput.value.trim();
  const sq = "?merge=" + encodeURIComponent(p.id) + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
  const o = await consoleWrite("/api/propose_split", sq, b => foundSaid(b, "un-merge") || word("unmergeProposed"));
  await refreshProposals();
  // Opened or found, the split opens under Needs you, as Same as... and Keep ... instead open theirs
  // (Section 29.25); accepting it is the person's next act.
  if (o && o.proposal_id) openReviewItem(o.proposal_id);
}
// One decision on one item (U4). It runs only when the item's view - its diff and checks - is the
// one on screen, never for a blocked merge, and a recall asks twice; there is no decide-all.
// Show all workspaces from Review (Section 29.21): the scope turns to All, Review stays open, and focus
// goes to the selected row.
function showAllForReview() {
  wsInput.value = "*"; beginWorkspaceTransition(); renderChipsActive(); syncUrlWorkspace(); poll();
  reviewFocus = true; refreshProposals(); refreshCuration();
}
// The Blocked items Reject all blocked takes: accepted, held by a failing check; a recall is left out,
// its verdict being a person's direct act on that one item (I17, Section 29.21).
function blockedItems() { return proposals.filter(p => p.state === "blocked" && p.kind !== "recall"); }
const blockedDlg = document.getElementById("blockedDlg");
let blockedListed = [];
async function openBlocked() {
  blockedListed = blockedItems().map(p => ({ p, why: null, ok: false }));
  const ul = document.getElementById("blockedList");
  const draw = () => {
    ul.innerHTML = html`${blockedListed.map(b => html`<li>${proposalTitle(b.p)}<span class="why">${b.why === null ? word("readingItsChecks") : b.why}</span></li>`)}`;
    document.getElementById("blockedGo").textContent = word("rejectAllN", { n: blockedListed.filter(b => b.ok).length });
  };
  draw();
  if (!blockedDlg.open) blockedDlg.showModal();
  document.getElementById("blockedCancel").focus();
  for (const b of blockedListed) {
    try {
      const r = await fetch("/api/proposal?id=" + encodeURIComponent(b.p.id) + (b.p.workspace ? "&workspace=" + encodeURIComponent(b.p.workspace) : ""), { cache: "no-store" });
      const v = r.ok ? await r.json() : null;
      const failing = v && v.checks ? v.checks.filter(c => c.blocking && !c.passed) : null;
      if (failing && failing.length) { b.why = failing.map(c => checkSaid(b.p, c)).join("; "); b.ok = true; }
      else b.why = word("checksUnread");
    } catch (_) { b.why = word("checksUnread"); }
    draw();
  }
}
document.getElementById("blockedCancel").onclick = () => blockedDlg.close();
blockedDlg.addEventListener("close", () => { const b = document.querySelector("[data-blocked]"); if (b) b.focus(); });
document.getElementById("blockedGo").onclick = async () => {
  const take = blockedListed.filter(b => b.ok);
  await refreshProposals();
  let done = 0; const failed = [];
  for (const b of take) {
    const now = proposals.find(p => p.id === b.p.id);
    if (!now || now.state !== "blocked") continue;   // its state changed meanwhile - left out
    const w = now.workspace ? "&workspace=" + encodeURIComponent(now.workspace) : "";
    const r = await consoleWrite("/api/review", "?proposal=" + encodeURIComponent(now.id) + "&decision=reject" + w, null, null, null, true);
    if (r.ok) done++; else failed.push(r.why);
  }
  blockedDlg.close();
  const tally = { done, of: done + failed.length, n: failed.length };
  if (failed.length) toast("bad", word(failed.length === 1 ? "rejectedOfOne" : "rejectedOfN", tally), failed[0]);
  else toast("ok", word(done === 1 ? "rejectedBlockedOne" : "rejectedBlockedN", { n: done }));
  reviewFocus = true;
  await refreshProposals();
  refreshElsewhere(true);
};
// Keep ... instead (Section 29.17): an entity merge of two, the one it would fold away, and its name.
function keepPair(p) {
  if (!p || p.kind !== "entity_merge" || !p.into) return null;
  const ts = [...new Set(p.targets || [])];
  if (ts.length !== 2 || !ts.includes(p.into)) return null;
  const id = ts.find(t => t !== p.into), i = (p.targets || []).indexOf(id);
  return { id, name: targetSaid(p, i) || word("theOtherOne") };
}
// The reverse of a merge: any merge of the same workspace, the same entities and `kept` kept, whatever
// its reason; a withdrawn one counts as none.
function reverseOf(p, kept) {
  const want = [...new Set(p.targets || [])].sort().join("|");
  return proposals.find(q => q.id !== p.id && q.kind === "entity_merge" && q.state !== "withdrawn" && q.into === kept
    && (q.workspace || "") === (p.workspace || "") && [...new Set(q.targets || [])].sort().join("|") === want) || null;
}
// Keep ... instead reads the reverse's state before writing anything, and rejects only once the
// reverse is there (P24): nothing is left half done.
async function keepInstead(item) {
  const p = item && item.p, pair = keepPair(p);
  if (!pair || !reviewView || reviewView.id !== p.id) return;   // the diff and checks are not on screen yet
  const ws = p.workspace || wsInput.value.trim();
  const wq = ws ? "&workspace=" + encodeURIComponent(ws) : "";
  const kept = pair.name;
  let rev = reverseOf(p, pair.id);
  // A reverse merge a split has since reversed holds nothing: the server asks again (Section 29.23).
  if (rev && rev.state === "merged" && proposals.some(q => q.kind === "entity_split" && q.state === "merged" && (q.targets || []).includes(rev.id))) rev = null;
  if (!rev) {
    const o = await consoleWrite("/api/propose_merge", "?a=" + encodeURIComponent(p.into) + "&b=" + encodeURIComponent(pair.id) + "&src=reverse" + wq, null, null, null, true);
    if (!o.ok) { toast("bad", word("keepNotOpened", { kept }), o.why); return; }
    rev = { id: o.body.proposal_id, state: o.body.state || "open" };
  }
  const r = await consoleWrite("/api/review", "?proposal=" + encodeURIComponent(p.id) + "&decision=reject" + wq, null, null, null, true);
  await refreshProposals();
  if (!r.ok) { toast("bad", word("keepNotRejected", { kept }), r.why); openReviewItem(rev.id); return; }
  if (rev.state === "merged") { toast("ok", word("keptAlready", { kept })); reviewFocus = true; renderReview(); return; }
  if (rev.state === "rejected") toast("info", word("keepRejectedBefore", { kept }));
  else if (rev.state === "blocked") toast("info", word("keepHeld", { kept }));
  else toast("ok", word("keptWaits", { kept }));
  openReviewItem(rev.id);
}
async function reviewDecide(item, decision) {
  if (!item || item.kind !== "proposal") return;
  const p = item.p;
  if (!reviewView || reviewView.id !== p.id) return;   // the diff and checks are not on screen yet
  if (decision === "merge") {
    if (p.no_effect_yet) return;   // no Accept until its kind has an effect (Section 29.22)
    if (reviewView.checks.some(c => c.blocking && !c.passed)) return;
    const pair = keepPair(p), rev = pair ? reverseOf(p, pair.id) : null;
    if (rev && rev.state === "merged") return;   // its reverse is accepted: no cycle (Section 29.17)
    if (p.kind === "recall" && recallArmed !== p.id) { recallArmed = p.id; renderReview(); return; }
  }
  recallArmed = null;
  const ws = wsInput.value.trim();
  const q = "?proposal=" + encodeURIComponent(p.id) + "&decision=" + decision + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
  // The notice names the item and offers the act that is on its Decided item, where that act decides
  // nothing; on this page it then leaves as a success without an action does (Section 29.3, U42).
  // No CSRF header needed: the socket (0600) admits no third-party origin - this page, proxied into
  // the desktop shell, is the only one on the surface - so there is no attacker origin for a forged
  // request to come from. The transport itself is the write gate.
  const title = proposalTitle(p);
  const action = decision === "merge" && p.kind === "entity_merge" ? { label: word("nounUnmerge"), decided: true, run: () => proposeSplit(p) } : null;
  const done = decision === "merge"
    ? await consoleWrite("/api/review", q, word("acceptedTitle", { title }), action)
    : await consoleWrite("/api/review", q, word("rejectedTitle", { title }), null, p.no_effect_yet ? null : word("acceptLater"));
  if (done) { reviewFocus = true; stepReview(1); }
  refreshProposals();
}
function wireDecisions(host, item) {
  host.querySelectorAll("[data-decide]").forEach(b => { b.onclick = () => reviewDecide(item, b.dataset.decide); });
  host.querySelectorAll("[data-unmerge]").forEach(b => { b.onclick = () => proposeSplit(item.p); });
  host.querySelectorAll("[data-undo]").forEach(b => { b.onclick = () => undoDecided(item.p); });
  host.querySelectorAll("[data-map]").forEach(b => { b.onclick = () => previewOnMap(item); });
  host.querySelectorAll("[data-skip]").forEach(b => { b.onclick = () => stepReview(1); });
  host.querySelectorAll("[data-stray]").forEach(b => { b.onclick = () => openReviewItem(b.dataset.stray); });
  host.querySelectorAll("[data-keep]").forEach(b => { b.onclick = () => keepInstead(item); });
}
function stepReview(by) {
  const items = segItems();
  if (!items.length) return;
  const i = Math.max(0, items.findIndex(x => x.key === reviewSel));
  reviewSel = items[Math.min(items.length - 1, Math.max(0, i + by))].key;
  reviewIndex = items.findIndex(x => x.key === reviewSel);
  recallArmed = null;
  renderReview();
}
// The tray opens Review at All workspaces, the scope its count is of (Section 7); the app calls this.
function openReviewAt(ws) {
  if (currentWs() !== ws) { wsInput.value = ws; beginWorkspaceTransition(); renderChipsActive(); poll(); }
  reviewSeg = "needs";
  if (reviewOpen()) { reviewFocus = true; renderReview(); refreshCuration(); }
  else location.hash = "#review";
}
// Opening the page puts focus on the selected row, so the keys act from the first press (U19).
window.addEventListener("hashchange", () => { recallArmed = null; if (reviewOpen()) reviewFocus = true; renderPlace(); renderReview(); if (reviewOpen()) refreshCuration(); refreshStrip(); requestFrame(); });
window.addEventListener("keydown", e => {
  if (!reviewOpen() || dialogOpen()) return;   // a dialog over Review takes the keys (Section 29.21)
  if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === "Escape") { location.hash = ""; return; }   // Escape closes from anywhere
  // The letters and numbers decide, so they act only while focus is in the list or the item (WCAG
  // 2.1.4); anywhere else on the page they are letters.
  const inReview = !!(e.target && e.target.closest && e.target.closest("#rvList, #rvDetail"));
  if (!inReview) return;
  const item = segItems().find(x => x.key === reviewSel);
  if (e.key === "j" || e.key === "ArrowDown") { e.preventDefault(); stepReview(1); }
  else if (e.key === "k" || e.key === "ArrowUp") { e.preventDefault(); stepReview(-1); }
  else if (e.key === "a") reviewDecide(item, "merge");
  else if (e.key === "r") reviewDecide(item, "reject");
  else if (/^[1-9]$/.test(e.key) && item && item.kind === "contested") {
    const ask = [...document.querySelectorAll('#rvDetail [data-settle="ask"]')][Number(e.key) - 1];
    if (ask) { e.preventDefault(); ask.click(); }
  }
});

// --- "Show on the map" from Review, and the way back (Section 29.3, U26) ---------------------------
// The preview frames every entity the item names, and a bar over the map says what it previews with
// "Back to Review" and "Done". Back to Review, Esc and Cmd+[ return to the item; Done ends the
// preview; a click on the empty map keeps it.
let reviewPreview = null;   // {seg, key, title} - the item the map preview left Review from
function itemNodes(it) {
  const ids = [];
  if (it.kind === "proposal") ids.push(...(it.p.targets || []), ...(it.p.into ? [it.p.into] : []));
  else if (it.kind === "contested" && it.c.field !== "definition") ids.push(it.c.id);
  else if (it.kind === "signal") {
    const b = it.body;
    if (b.pair) ids.push(b.pair.a, b.pair.b);
    if (b.group) ids.push(...b.group.map(m => m.id));
    if (b.node) ids.push(b.node.id);
    if (b.settled && b.settled.field !== "definition") ids.push(b.settled.id);
    if (b.bag) return nodes.filter(n => b.bag.member_names.includes(n.name));
  }
  const out = [...new Set(ids)].map(nodeById).filter(Boolean);
  return out.length || it.kind !== "proposal" ? out : affectedNodes(it.p);
}
function previewOnMap(it, only) {
  if (!it) return;
  const ns = only ? [only] : itemNodes(it);
  // The map shows what a kind with no effect yet names, not a change (Section 29.22).
  const noEffect = it.kind === "proposal" && it.p.no_effect_yet;
  reviewPreview = { seg: reviewSeg, key: it.key, title: noEffect ? word("titleNoEffect", { title: itemTitle(it) }) : itemTitle(it) };
  location.hash = "";
  if (it.kind === "proposal") { proposalSel = null; selectProposal(it.p); }
  if (ns.length > 1) { fitView(ns); userMoved = true; }
  else if (ns.length === 1) select(ns[0]);
  showPreviewBar();
}
function showPreviewBar() {
  const bar = document.getElementById("previewBar");
  if (!bar) return;
  bar.hidden = !reviewPreview;
  document.body.classList.toggle("previewing", !!reviewPreview);
  if (!reviewPreview) return;
  bar.querySelector(".pv-t").textContent = word("previewOf", { title: reviewPreview.title });
  bar.querySelector("[data-pv=back]").focus();
}
function endPreview() {
  reviewPreview = null;
  if (proposalSel) { proposalSel = null; requestFrame(); }
  showPreviewBar();
}
function backToReview() {
  const r = reviewPreview;
  if (!r) return;
  endPreview();
  clearSelection(false);   // the sheet the preview opened closes with it
  reviewSeg = r.seg; reviewSel = r.key;
  location.hash = "#review";
}
{
  const bar = document.getElementById("previewBar");
  if (bar) {
    bar.querySelector("[data-pv=back]").onclick = backToReview;
    bar.querySelector("[data-pv=done]").onclick = endPreview;
  }
  // Ahead of the page's own Esc, so the preview's way back wins while it is shown.
  window.addEventListener("keydown", e => {
    if (!reviewPreview || reviewOpen()) return;
    const back = e.key === "Escape" || (e.key === "[" && (e.metaKey || e.ctrlKey));
    if (!back) return;
    e.preventDefault(); e.stopImmediatePropagation();
    backToReview();
  }, true);
}

// The T-Box types a proposal touches, split by axis. Relation names match the graph's edge kinds
// (normalized at propose time); entity names match node types. Empty sets when the proposal declares none.
function affectedTypeSets(p) {
  const rel = new Set(), ent = new Set();
  for (const a of (p && p.affected_types) || []) {
    if (a.target === "relation") rel.add(a.name);
    else if (a.target === "entity") ent.add(a.name);
  }
  return { rel, ent };
}
// The nodes a tbox_change preview touches: endpoints of edges whose kind is (re)defined, plus nodes
// whose entity type is. Used to frame the preview (a tbox_change has no single `into` to center on).
function affectedNodes(p) {
  const { rel, ent } = affectedTypeSets(p);
  if (!rel.size && !ent.size) return [];
  const out = new Set();
  if (rel.size) for (const e of edges) if (rel.has(e.type)) { out.add(e.a); out.add(e.b); }
  if (ent.size) for (const n of nodes) if (ent.has(n.type)) out.add(n);
  return [...out];
}

// The computed belief diff for the selected proposal (proposal-workflow.md Section 5). Null while
// unfetched or when nothing is selected; the canvas overlay is a hint, this is the artifact.
let proposalDiff = null;
// Blocking check results for the selected proposal (proposal-workflow.md Section 6).
let proposalChecks = null;

async function fetchProposalDiff(id) {
  const ws = wsInput.value.trim();
  let q = "?id=" + encodeURIComponent(id);
  if (ws && ws !== "*" && ws !== "all") q += "&workspace=" + encodeURIComponent(ws);
  try {
    const r = await fetch("/api/proposal" + q, { cache: "no-store" });
    if (!r.ok) return;
    const view = await r.json();
    // Ignore a response that lost the race against another selection.
    if (proposalSel && view && view.id === proposalSel.id) {
      proposalDiff = view.belief_diff || null;
      proposalChecks = view.checks || null;
      requestFrame();
    }
  } catch (e) { /* the overlay still works - the diff is additive */ }
}

// Render the diff as a before -> after comparison. An uncomputable diff says so: for a proposal kind
// that still enforces nothing, an empty diff would read as "changes nothing".
// A diff row's field, in the screen's words (design-system.md Section 29.6): the engine's ids
// `kind` and `edge` are a type and a relation.
const FIELD_SAID = { canonical_name: word("colName"), kind: word("fieldType"), type: word("fieldType"), edge: word("fieldRelation"), tier: word("trust"), definition: word("fieldDefinition"), description: word("fieldDescription") };
function fieldSaid(f) { return Object.hasOwn(FIELD_SAID, f) ? FIELD_SAID[f] : String(f || ""); }
function diffHtml(d) {
  if (!d) return html`<div class="dnote">${word("computingDiff")}</div>`;
  // The note is the server's sentence (docs/i18n.md Section 5).
  if (!d.computable) return html`<div class="dnote">${word("noDiff", { note: d.note || word("notComputable") })}</div>`;
  const tiers = (d.tier_changes || []).map(t => [
    html`<div class="drow"><span class="dk">${fieldSaid("tier")}</span><code>${t.observation.slice(0, 10)}</code>`,
    html`<span class="dfrom">${tierName(t.from)}</span><span class="darr">-&gt;</span><span class="dto">${tierName(t.to)}</span></div>`]);
  const beliefs = (d.overturned || []).map(b => {
    const settled = b.contested_before && !b.contested_after;
    const created = !b.contested_before && b.contested_after;
    const flag = settled ? html`<span class="dsettled">${word("settlesContradiction")}</span>`
               : created ? html`<span class="dcreated">${word("createsContradiction")}</span>` : "";
    return [html`<div class="drow"><span class="dk">${fieldSaid(b.field)}</span>`,
      html`<button type="button" class="nchip" data-id="${b.entity}" title="${tipText(word("focusName", { name: b.name }))}">${b.name}</button>`,
      html`<span class="dfrom">${b.from || word("noType")}</span><span class="darr">-&gt;</span>`,
      html`<span class="dto">${b.to || word("noType")}</span>${flag}</div>`];
  });
  // entity_merge: which references move onto the canonical id, and which edges stop existing.
  // A self-loop is dropped by graph(), so that edge vanishes on accept - not readable from the
  // canvas overlay, which can only accent edges incident to a target.
  const rewires = (d.rewired || []).map(r => [
    html`<div class="drow"><span class="dk">${fieldSaid("edge")}</span><code>${r.kind}</code>`,
    html`<span class="dfrom">${r.from_name}</span><span class="darr">-&gt;</span>`,
    html`<span class="dto">${r.to_name}</span>`,
    html`<span class="dother">(${r.other_name})</span>`,
    r.becomes_self_loop ? html`<span class="dcreated">${word("becomesLoop")}</span>` : "",
    html`</div>`]);
  if (!tiers.length && !beliefs.length && !rewires.length) return html`<div class="dnote">${word("overturnsNothing")}</div>`;
  return html`<div class="ddiff">${tiers}${beliefs}${rewires}</div>`;
}

// Failing blocking checks, shown above the accept button. A blocking failure means the fold would
// refuse the merge anyway, so surfacing it here turns a silent "nothing happened" into a reason.
function checksHtml(cs) {
  if (!cs || !cs.length) return html``;
  const bad = cs.filter(c => c.blocking && !c.passed);
  if (!bad.length) return html`<div class="dnote">${word("checksPassLower")}</div>`;
  return html`<div class="cblock">${bad.map(c => [
    html`<div class="crow"><span class="cbad">${word("blockedLower")}</span><span class="ck">${c.name}</span>`,
    html`<span class="cd">${c.detail}</span></div>`])}</div>`;
}

// Select a proposal to preview on the graph (toggle). Centers on the canonical (`into`) node when
// present (entity_merge); otherwise frames the affected T-Box elements (tbox_change).
function selectProposal(p) {
  proposalSel = (proposalSel && p && proposalSel.id === p.id) ? null : p;
  // The diff is per-proposal (two belief folds), so it is fetched on selection rather than carried
  // on every list row. Cleared first so a stale diff never sits under a newly selected proposal.
  proposalDiff = null;
  proposalChecks = null;
  if (proposalSel) {
    fetchProposalDiff(proposalSel.id);
    const into = nodeById(proposalSel.into);
    if (into) select(into);
    // No single canonical node (tbox_change): frame the affected members and mark the view user-driven
    // so a pending auto-fit does not stomp the preview (same as the search-result fit).
    else { const framed = affectedNodes(proposalSel); if (framed.length) { fitView(framed); userMoved = true; } }
  }
  requestFrame();
}

// Read on every poll, page open or not: the review count in the status bar is made of these.
async function refreshProposals() {
  refreshElsewhere(false);
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/proposals" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const p = await r.json(); if (Array.isArray(p)) proposals = p; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderReview();
}

