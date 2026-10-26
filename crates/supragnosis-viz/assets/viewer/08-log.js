// --- Observation log panel (the source of truth, Principle 1) ------------------------
// The graph is a projection OF this log; this panel reads the raw events newest-first, with their
// provenance (Principle 2) and effective tier. A row expands to show who attested it / lineage /
// which entities it touched (click an entity to focus its node). Shared row rendering feeds both
// this panel and the inspector "why" section's supporting-observations list.
let obsLog = [];   // /api/observations for the current workspace (newest-first): the Log's rows and one more
// The Log lists the scope's newest observations, LOG_ROWS at a time (design-system.md Section 27.7,
// U37). A store filled through observe holds one for every write, and drawing them all - each row
// found again in the whole list to wire it - took the page seconds on 12,000. It asks for one more
// than it shows, to know there are more, asks with the ETag of what it holds (U33), and shows more
// on request.
const LOG_ROWS = 200;
let logLimit = LOG_ROWS;
const heldLog = { url: "", tag: "" };
// An observation by id: from the Log's rows when they hold it, else read from the scope's log once.
async function observationById(id) {
  const kept = obsLog.find(x => x.id === id);
  if (kept) return kept;
  // Read by its id, in whichever workspace holds it (design-system.md Section 29.7).
  try {
    const r = await fetch("/api/observations?id=" + encodeURIComponent(id), { cache: "no-store" });
    if (r.ok) { const l = await r.json(); if (Array.isArray(l)) return l[0] || null; }
  } catch (e) { /* the caller says it could not open it */ }
  return null;
}
let obsLogSig = "";   // set signature of the rendered rows - re-render only when it changes (below)

// Compact clock for the row (full timestamp on hover + in the expanded provenance). The log is a
// record, so it shows the wall time, not "3s ago": HH:MM for today, else MM/DD HH:MM.
function obsWhen(ms) {
  const d = new Date(ms);
  if (!Number.isFinite(d.getTime())) return "";
  const now = new Date();
  const hm = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const sameDay = d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth() && d.getDate() === now.getDate();
  // The year too when it is not this one, so last October does not read as two days ago (Section 29.19).
  if (d.getFullYear() !== now.getFullYear()) return `${d.getMonth() + 1}/${d.getDate()}/${d.getFullYear()} ${hm}`;
  return sameDay ? hm : `${d.getMonth() + 1}/${d.getDate()} ${hm}`;
}
function obsWhenSec(ms) {
  const d = new Date(ms);
  return Number.isFinite(d.getTime()) ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit", second: "2-digit" }) : "";
}
function obsWhenFull(ms) { const d = new Date(ms); return Number.isFinite(d.getTime()) ? d.toLocaleString() : ""; }
// Trust tier as a colored dot (the edge columns' dot idiom) - a quiet ramp from dim (unverified) to
// gold (human-confirmed). The full tier text lives in the expanded provenance.
// A tier's mark wherever a row or card shows one: the ring the map draws for that tier, as the Trust
// legend draws it (design-system.md Section 9), so a mark means one thing on every surface.
function tierDot(t) { return html`<span class="tsw tmark t-${String(t)}" title="${tipText(tierName(t))}"></span>`; }

// Content-first row: a quiet meta line (tier dot + compact time) over the observation content (the
// primary element, clamped to 2 lines). Host and the rest move to the expanded provenance.
// A proposal event said in names. Its stored content is machine text with raw ids in it, and that
// text is inside the content address so it can never be rewritten - the log read as rows of hashes.
// The server resolves the ids (it can still name a merged-away entity, which this side cannot, since
// the graph folds those away); this only phrases the result. Falls back to the raw content whenever
// the server had nothing to add, so an untranslatable row still says something.
// Returns text, not markup: callers interpolate it into an html template, which escapes it once.
// What the Log says for a proposal event (Section 9, U17): the proposal's noun and what happened to
// it, then its targets in names - "Merge proposed: PostgreSQL into Postgres". The engine sends the
// noun and the verb with the row, from its one table; a row without them - an older node's, or a
// kind it has no word for - says "Something changed" and keeps the stored text in its card.
const HASH_ID = /^[0-9a-f]{16,}$/;   // a content address, as observation and entity ids are
function proposalLine(p) {
  const ts = p.targets || [];
  // Two entities can share a display name - that is what a duplicate IS, and a merge of them is
  // precisely the act most worth reading. "T2 -> T2" says nothing, so a name that is not unique
  // within the line carries its short id. Only the ambiguous ones pay it.
  const all = p.into ? ts.concat(ts.some(t => t.id === p.into.id) ? [] : [p.into]) : ts;
  const seen = {};
  for (const t of all) { const n = t.name || t.id; seen[n] = (seen[n] || 0) + 1; }
  // A target the server could not name is left out when its id is a hash - hashes live in the card
  // (Section 9) - and said whole otherwise, so an id that is itself a name is never cut short.
  const named = t => {
    const n = t.name && t.name !== t.id ? t.name : (HASH_ID.test(t.id) ? null : t.id);
    return n && seen[t.name || t.id] > 1 ? `${n} (${t.id.slice(0, 6)})` : n;
  };
  let what = "";
  if (p.into) {
    // A merge reads as a direction: what disappears, and what it folds into.
    const gone = ts.filter(t => t.id !== p.into.id).map(named).filter(Boolean);
    const into = named(p.into);
    what = gone.length && into ? `${gone.join(", ")} into ${into}` : (into || gone.join(", "));
  } else if (ts.length) {
    what = ts.map(named).filter(Boolean).join(", ");
  }
  const noun = p.noun, verb = p.verb;
  if (!noun || !verb) return "Something changed" + (what ? ": " + what : "");
  // State is worth saying on a verdict precisely when it disagrees with the decision - an accepted
  // merge that a check blocks is the case a reader must not miss.
  const state = p.event === "verdict" && p.decision === "merge" && p.state === "blocked" ? ". A check blocks it"
    // A blocked merge a person then rejected (proposal-workflow.md Section 7.1, design-system.md 29.3).
    : p.event === "verdict" && p.decision === "merge" && p.state === "rejected" ? ". A check blocked it, and it was rejected"
    : p.event === "verdict" && p.state && !(p.decision === "merge" && p.state === "merged") && !(p.decision === "reject" && p.state === "rejected")
      ? " (" + p.state + ")" : "";
  return `${noun} ${verb}${what ? ": " + what : ""}${state}`;
}

// A Log row (design-system.md Section 9, U17): what happened, in words, under a who-line - the tier,
// who and when, marked with the ring the map draws for that tier. Rows from one minute say the time
// once, on the first. Called as a map callback, so `list` is the rows it is one of.
function obsRowHtml(o, i, list) {
  const a0 = authorOf(o) || {};
  const text = o.proposal ? proposalLine(o.proposal) : o.content;
  const cls = o.proposal ? "otext ev" : "otext";
  const when = obsWhen(a0.observed_at);
  const prev = i > 0 && Array.isArray(list) ? authorOf(list[i - 1]) || {} : null;
  const who = [tierName(o.effective_tier), writerFor(a0), prev && obsWhen(prev.observed_at) === when ? "" : when]
    .filter(Boolean).join(" - ");
  return html`${[
    html`<div class="obs" data-id="${o.id}">`,
    html`<button type="button" class="ohead" title="${tipText("Open the observation")}">`,
      html`<span class="ometa">${tierDot(o.effective_tier)}`,
        html`<span class="owhen">${who}</span></span>`,
      html`<span class="${cls}">${text}</span>`,
    html`</button></div>`]}`;
}

// The observation detail CARD: a focused, dismissable view of one observation - full content plus
// its provenance (every attestation), lineage (derived_from), and the entities/relations it asserted.
// The single detail surface for both the node log column and the workspace Log tab (a log row opens
// it, so the list stays a scannable list - the earlier cramped in-column expansion is gone).
function obsCardHtml(o) {
  const a0 = authorOf(o) || {};
  // What grounds it, first: the reference its writer cited - a link only for a web address, since
  // the writer wrote it (P18) - and never a `surface:` marker, which the who-line says (Section 29.7).
  const cited = (o.attestations || []).map(a => a.source_ref).find(r => r && !/^surface:/.test(r));
  const citedHtml = !cited ? "" : /^https?:\/\//.test(cited)
    ? html`<div class="oc-cited">Cited: <a href="${cited}" target="_blank" rel="noreferrer noopener">${cited}</a></div>`
    : html`<div class="oc-cited">Cited: <span class="selectable">${cited}</span></div>`;
  const parts = [html`<div class="card">`,
    html`<button class="close" title="${tipText("Close", "Esc")}" aria-label="Close"><span class="i i-x" aria-hidden="true"></span></button>`,
    html`<div class="oc-meta">${tierDot(o.effective_tier)}<span class="octier">${tierName(o.effective_tier)}</span>`,
      html`<span class="ocwhen">${obsWhenFull(a0.observed_at)}</span></div>`,
    citedHtml,
    html`<div class="oc-content">${o.proposal ? proposalLine(o.proposal) : o.content}</div>`];
  // The act that set the tier it reads at, said beside what its writer wrote.
  const g = o.gated_by;
  if (g) {
    const by = (g.surface === "surface:console" ? " from " : " by ") + writerOf({ source_ref: g.surface, origin_node: g.origin_node });
    const what = g.promotion ? tierName(o.effective_tier) + by : "Lowered to " + tierName(o.effective_tier) + by;
    parts.push(html`<div class="oc-gate"><span>${what}, ${obsWhenFull(g.at)}</span><button type="button" class="linkish" data-open-review="${g.proposal}">Open in Review</button></div>`);
  }
  // A person can demote a claim (Section 29.26): knowledge that reads above Unverified. It asks first, with what the act does, and casts only then.
  if (!o.proposal && TIER_ORDER.indexOf(o.effective_tier) > 0) {
    parts.push(demoteAsk && demoteAsk.id === o.id ? demoteHtml(o)
      : html`<div class="oc-act"><button type="button" class="linkish" data-demote="open">Demote this claim</button></div>`);
  }
  parts.push(html`<div class="osec">Provenance <span class="ct">${(o.attestations || []).length}</span></div>`);
  for (const a of o.attestations || []) {
    parts.push(html`<div class="prow"><span class="pwho">${whoLine(a.trust_tier, a)}</span>`,
      a.evaluated_tier !== a.trust_tier ? html`<span class="ptier">counted as ${tierName(a.evaluated_tier)}</span>` : "",
      a.confidence != null ? html`<span class="pconf">the agent's confidence: ${String(a.confidence)}</span>` : "",
      a.origin_node ? html`<span class="poid">machine ${String(a.origin_node).slice(0, 8)}</span>` : "",
      html`</div>`);
  }
  if (o.derived_from && o.derived_from.length)
    parts.push(html`<div class="osec">Derived from</div><div class="oids">${
      o.derived_from.map(id => html`<button type="button" class="oid linkish" data-open-obs="${id}">${String(id).slice(0, 10)}</button>`)}</div>`);
  if (o.proposal) {
    const p = o.proposal;
    const full = proposals.find(q => q.id === p.proposal);
    parts.push(html`<div class="osec">Proposal</div>`,
      html`<div class="ocprop">${p.noun || "Proposal"} - ${(OUTCOME[p.state] || (p.state === "open" ? "Open" : p.state || "")).toLowerCase()}</div>`,
      full && full.rationale ? html`<div class="ocwhy">"${full.rationale}"</div>` : "",
      html`<div><button type="button" class="linkish" data-open-review="${p.proposal}">Open in Review</button></div>`);
    const ts = p.targets || [];
    if (ts.length) {
      // A target that no longer has a node is one this merge folded away - it stays listed (the act
      // touched it) but is not offered as a jump, because there is nothing left to jump to.
      const chip = t => {
        const live = !!nodeById(t.id);
        const into = p.into && t.id === p.into.id;
        const label = html`${t.name || String(t.id).slice(0, 8)}${into ? html` <span class="rk">kept</span>` : ""}`;
        return live
          ? html`<button type="button" class="echip" data-id="${t.id}" title="${tipText("Focus " + (t.name || t.id))}">${label}</button>`
          : html`<span class="echip off" title="${tipText("Folded away by this merge")}">${label}</span>`;
      };
      parts.push(html`<div class="osec">Targets</div><div class="echips">${ts.map(chip)}</div>`);
    }
  }
  if (o.entities && o.entities.length)
    parts.push(html`<div class="osec">Entities</div><div class="echips">${
      o.entities.map(e => html`<button type="button" class="echip" data-id="${e.id}" title="${tipText("Focus " + e.name)}">${e.name}</button>`)}</div>`);
  if (o.relations && o.relations.length)
    parts.push(html`<div class="osec">Relations</div>`,
      o.relations.map(r => html`<div class="ocrel">${r.from} <span class="rk">${r.type}</span> ${r.to}</div>`));
  // A proposal event's stored text stays inspectable, folded at the foot: this card is where an id
  // resolves (P2/P14), and what is literally in the log is part of that.
  if (o.proposal) parts.push(html`<details class="ocraw-fold"><summary><span class="i i-chevron-right" aria-hidden="true"></span>As stored</summary><div class="ocraw">${o.content}</div></details>`);
  parts.push(html`<div class="ocid">obs ${String(o.id).slice(0, 16)}</div></div>`);
  return html`${parts}`;
}
// Opens a proposal's Review item, in its own segment, from a card (Section 29.7).
function openReviewItem(pid, ws) {
  hideObsCard();
  if (ws && currentWs() !== ws && currentWs() !== "*" && currentWs() !== "all") { wsInput.value = ws; beginWorkspaceTransition(); renderChipsActive(); poll(); }
  const p = proposals.find(q => q.id === pid);
  reviewSeg = p && p.state !== "open" && p.state !== "blocked" ? "decided" : "needs";
  reviewSel = "p:" + pid; reviewFocus = true;
  if (reviewOpen()) renderReview(); else location.hash = "#review";
}
function hideObsCard() { obscardEl.className = ""; obscardEl.innerHTML = ""; cardObs = null; demoteAsk = null; }
// The observation on the card, and the card's open "Demote this claim to Unverified?" question:
// { id, state: "reading" | "ok" | "err", preview, why } (Section 29.26).
let cardObs = null, demoteAsk = null;
// A claim's first words, as a notice and a reason say it.
function claimWords(o) {
  const t = String((o && o.content) || "").trim();
  return t.length > 48 ? t.slice(0, 48).trimEnd() + "..." : t;
}
// The question, in step 41's form: what flips, whose ring drops, what else the claim states, what
// Unverified means and the ceiling it leaves, then the checks - and Demote waits for all of it (U18).
function demoteHtml(o) {
  const v = demoteAsk, q = "Demote this claim to Unverified?";
  const parts = [html`<div class="cq">${q}</div>`];
  let can = false, state = "Reading what this demotes...";
  if (v.state === "err") state = v.why || "What this act changes could not be read.";
  if (v.state === "ok") {
    const d = v.preview.belief_diff, mine = (d.reach || []).find(r => r.observation === o.id);
    const flips = (d.overturned || []).map(b => {
      const what = b.name + "'s " + fieldSaid(b.field).toLowerCase();
      if (!b.contested_before && b.contested_after) return what + " becomes contested and waits under Needs you.";
      if (b.contested_before && !b.contested_after) return what + (b.to ? " becomes " + b.to + " and" : "") + " is no longer contested.";
      return what + " becomes " + (b.to || "nothing") + ".";
    });
    for (const t of flips.length ? flips : ["Every value it states stays; it is trusted less."]) parts.push(html`<div class="creach">${t}</div>`);
    const rings = ringMoves(d, null, true);
    for (const t of rings.length ? rings.map(l => "With it, " + l + ".") : ["No entity's ring moves."]) parts.push(html`<div class="creach">${t}</div>`);
    const rel = mine ? (mine.relations || []).length : 0, defs = (mine && mine.type_defs) || [];
    if (rel) parts.push(html`<div class="creach">It also states ${rel} relation${rel === 1 ? "" : "s"}.</div>`);
    if (defs.length) parts.push(html`<div class="creach">It also defines ${defs.length} type${defs.length === 1 ? "" : "s"}: ${nameList(defs)}.</div>`);
    parts.push(html`<div class="cdef">${tierName("unverified")}: ${TIER_MEANS.unverified}</div>`,
      html`<div class="cdef">Later claims do not raise it; a review that raises it does.</div>`);
    const words = c => CHECK_WORDS[c.name] || c.name;
    const fails = (v.preview.checks || []).filter(c => !c.passed);
    if (fails.length) state = "Blocked: " + fails.map(c => words(c) + " - " + c.detail).join("; ");
    else {
      parts.push(html`<div class="cchk"><span class="i i-check" aria-hidden="true"></span>Checks pass: ${[...new Set((v.preview.checks || []).map(words))].join(", ")}</div>`);
      can = true;
    }
  }
  const sid = "dst-" + String(o.id).slice(0, 12);
  return html`<div class="cask" role="group" aria-label="${q}">${parts}${
    can ? "" : html`<div class="cstate" id="${sid}">${state}</div>`}<span class="cacts"><button type="button" class="confirm" data-demote="yes"${can ? "" : html` disabled aria-describedby="${sid}"`}>Demote</button><button type="button" class="quiet" data-demote="no">Cancel</button></span></div>`;
}
async function openDemote(o) {
  demoteAsk = { id: o.id, state: "reading" };
  showObsCard(o);
  const got = await readPreview([{ obs: o.id, tier: "unverified", kind: "claim_demotion" }]);
  if (!demoteAsk || demoteAsk.id !== o.id || !cardObs || cardObs.id !== o.id) return;   // closed or moved on meanwhile
  demoteAsk = got.preview ? { id: o.id, state: "ok", preview: got.preview } : { id: o.id, state: "err", why: got.why };
  showObsCard(o);
  const go = obscardEl.querySelector("[data-demote=yes]:not([disabled])") || obscardEl.querySelector("[data-demote=no]");
  if (go) go.focus();
}
// Cancel and Esc close the question, and focus returns to the act that opened it (U19).
function closeDemote(o) {
  demoteAsk = null;
  showObsCard(o);
  const back = obscardEl.querySelector("[data-demote=open]");
  if (back) back.focus();
}
// Demote casts the demotion through the path Undo uses; its notice offers Undo once the act's own
// undo is read - the read Decided's Undo demotion makes - with that diff as its note (Section 29.26).
async function castDemote(o) {
  if (!demoteAsk || demoteAsk.id !== o.id || demoteAsk.state !== "ok") return;
  const said = "Demoted to Unverified: " + claimWords(o);
  const ws = currentWs(), away = o.workspace && ws && ws !== o.workspace && ws !== "*" && ws !== "all";
  const undo = async body => {
    if (!body.proposal_id) return null;
    let u = null;
    try {
      const r = await fetch("/api/resolve/undo?proposal=" + encodeURIComponent(body.proposal_id), { cache: "no-store" });
      if (r.ok) u = await r.json();
    } catch (_) { /* said below */ }
    if (!u) return { note: "Undo is not offered: what it would move back could not be read." };
    const acts = u.acts || [];
    if (!acts.length) return { note: "Nothing to undo: another decision holds this claim where it is." };
    if ((u.checks || []).some(c => c.blocking && !c.passed)) return { note: "Undo is not offered: a check holds it back." };
    const moves = ringMoves(u.belief_diff, null);
    return {
      label: "Undo",
      note: "Undo returns this claim to " + tierName(acts[0].tier) + " and keeps it there until a review moves it again."
        + (moves.length ? " " + moves.join(". ") + "." : ""),
      run: async () => {
        for (const a of acts) {
          if (!await consoleWrite("/api/resolve", settleQuery(a.observation, a.tier, a.kind, "Undid " + said), "Undone - the claim is " + tierName(a.tier) + " again")) break;
        }
        await poll();
      },
    };
  };
  demoteAsk = null;
  const body = await consoleWrite("/api/resolve", settleQuery(o.id, "unverified", "claim_demotion", said), said, undo, away ? "In " + o.workspace + "." : null);
  if (!body) { showObsCard(o); return; }
  await poll();
  // Read again by its id: the Log's kept row still has the tier it had.
  try {
    const r = await fetch("/api/observations?id=" + encodeURIComponent(o.id), { cache: "no-store" });
    if (r.ok) { const l = await r.json(); if (Array.isArray(l) && l[0]) o = l[0]; }
  } catch (_) { /* the card shows what it had */ }
  if (!cardObs || cardObs.id !== o.id) return;
  showObsCard(o);
  const close = obscardEl.querySelector(".close");
  if (close) close.focus();
}
function showObsCard(o) {
  if (!cardObs || cardObs.id !== o.id) demoteAsk = demoteAsk && demoteAsk.id === o.id ? demoteAsk : null;
  cardObs = o;
  obscardEl.innerHTML = html`${obsCardHtml(o)}`;
  obscardEl.className = "on";
  obscardEl.querySelector(".close").onclick = hideObsCard;
  obscardEl.querySelectorAll("[data-demote]").forEach(b => {
    b.onclick = ev => {
      ev.stopPropagation();
      const d = b.dataset.demote;
      if (d === "open") openDemote(o); else if (d === "yes") castDemote(o); else closeDemote(o);
    };
  });
  // Every id leads somewhere: a parent observation opens its card, a proposal its Review item.
  obscardEl.querySelectorAll("[data-open-obs]").forEach(b => {
    b.onclick = async ev => { ev.stopPropagation(); const p = await observationById(b.dataset.openObs); if (p) showObsCard(p); else toast("bad", "That observation is not here", "It may be in a workspace this view cannot read."); };
  });
  obscardEl.querySelectorAll("[data-open-review]").forEach(b => {
    b.onclick = ev => { ev.stopPropagation(); openReviewItem(b.dataset.openReview, o.workspace); };
  });
  // An asserted entity: close the card and focus its node.
  obscardEl.querySelectorAll(".echip").forEach(c => {
    c.onclick = ev => {
      ev.stopPropagation();
      const n = nodeById(c.dataset.id);
      hideObsCard();
      if (n) select(n);
    };
  });
}

// Renders a list of observation summaries into a container (a scannable list) and wires each row to
// open the observation detail card. Used by the workspace Log tab and the node inspector's log column.
function wireObsList(container, list) {
  container.innerHTML = html`${list.length ? list.map(obsRowHtml) : html`<div class="empty">no observations</div>`}`;
  // The rows are the list's, in its order, so each is wired by its place rather than found again.
  container.querySelectorAll(".obs").forEach((row, i) => {
    const o = list[i];
    if (!o || o.id !== row.dataset.id) return;
    row.querySelector(".ohead").onclick = () => showObsCard(o);
  });
}

function renderLog() {
  const more = obsLog.length > logLimit, shown = more ? obsLog.slice(0, logLimit) : obsLog;
  logCtEl.textContent = obsLog.length ? (more ? logLimit + "+" : obsLog.length) : "";
  if (!obsLog.length) {
    // The canvas's words, and where what an AI app remembers shows (design-system.md Section 29.10).
    const ws = currentWs(), all = ws === "*" || ws === "all";
    obsLogSig = "";
    logBodyEl.innerHTML = html`<div class="empty">${"Nothing here yet. What an AI app remembers" + (all ? "" : " in " + statsScope(ws)) + " shows up here."}</div>`;
    return;
  }
  // Re-render only when the observation set actually changed, so a poll refresh does not collapse a
  // row the user expanded (the log is append-mostly; an unchanged set keeps its DOM + open rows).
  const sig = shown.map(o => o.id + ":" + o.effective_tier + ":" + (o.attestations || []).length).join(",") + (more ? "+" : "");
  if (sig === obsLogSig && logBodyEl.querySelector(".obs")) return;
  obsLogSig = sig;
  wireObsList(logBodyEl, shown);
  if (more) {
    const b = document.createElement("button");
    b.type = "button"; b.className = "obsmore"; b.textContent = "Show " + LOG_ROWS + " more";
    b.onclick = () => { logLimit += LOG_ROWS; refreshLog(); };
    logBodyEl.appendChild(b);
  }
}

// --- The strip: the Log folded (docs/design-system.md Section 6) ----------------------------------
// While the right side is empty - no Log, no sheet - the window's lower left shows the scope's newest
// observation. It reads one row on its own, because the Log loads only while it is open, and says so
// when there is none or the server does not answer rather than keeping a stale line (P5, P24).
const stripEl = document.getElementById("strip");
let stripState = null;   // { o } | { empty: true } | { failed: true }; null until first read
function stripShown() {
  return !dockREl.classList.contains("on") && !detailEl.classList.contains("on") && !reviewOpen();
}
// The who-line (design-system.md Section 29.7): tier - writer, for whom - time. The writer is the
// machine that wrote the authoring attestation: this one when it carries no sync stamp or its stamp's
// origin is this node - a host name decides nothing, since every unconfigured node is "localhost" -
// and any other by its origin id's first eight characters. A console verdict written here is "this
// console": the console is a path any program of the user can take, so it names where the act was
// cast, not who cast it (resolution.md Section 6).
let ownNode = null;   // this node's sync id, from the Sync read; null until known
function writtenHere(a) { return !a.origin_node || (!!ownNode && a.origin_node === ownNode); }
function writerOf(a) {
  if (!a) return "";
  const mine = writtenHere(a);
  const machine = mine ? here() : String(a.origin_node).slice(0, 8);
  if (a.source_ref === "surface:console") return mine ? "this console" : "the console at " + machine;
  // The app this node recorded for it (Section 29.8): "Claude Code on this Mac".
  return a.app && mine ? a.app + " on " + machine : machine;
}
function writerFor(a) { return a ? writerOf(a) + (a.on_behalf_of ? ", for " + a.on_behalf_of : "") : ""; }
// The attestation that wrote it - the engine's `author`, not the first by host.
function authorOf(o) { const as = (o && o.attestations) || []; return as[o && o.author | 0] || as[0] || null; }
// "11:27 AM", or "11:27:41 AM" where a neighbor shares the minute.
function whoLine(tier, a, withSeconds) {
  const when = a ? (withSeconds ? obsWhenSec(a.observed_at) : obsWhen(a.observed_at)) : "";
  return [tier ? tierName(tier) : "", writerFor(a), when].filter(Boolean).join(" - ");
}
async function refreshStrip() {
  if (!stripShown()) { renderStrip(); return; }
  const ws = currentWs();
  try {
    const r = await fetch("/api/observations?limit=1" + (ws ? "&workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (!r.ok) throw new Error("status " + r.status);
    const l = await r.json();
    stripState = Array.isArray(l) && l.length ? { o: l[0] } : { empty: true };
  } catch (e) { stripState = { failed: true }; }
  renderStrip();
}
function renderStrip() {
  if (!stripEl) return;
  const on = stripShown() && !!stripState;
  stripEl.classList.toggle("on", on);
  if (!on) return;
  if (stripState.o) {
    const o = stripState.o, a = authorOf(o) || {};
    const first = String(o.content || "").split("\n")[0];
    stripEl.innerHTML = html`${tierDot(o.effective_tier)}<span class="w">${writerFor(a)} ${obsWhen(a.observed_at)}</span><span class="t">${o.proposal ? proposalLine(o.proposal) : first}</span>`;
  } else if (stripState.empty) {
    stripEl.innerHTML = html`<span class="t">Nothing here yet.</span>`;
  } else {
    stripEl.innerHTML = html`<span class="t bad">The Log is unavailable - the server did not answer</span>`;
  }
}
if (stripEl) stripEl.onclick = () => applyDock(false, true);

async function refreshLog() {
  if (!panelOn("log")) return;
  const ws = wsInput.value.trim();
  const url = "/api/observations?limit=" + (logLimit + 1) + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
  try {
    const r = await fetch(url, askedWith(heldLog, url));
    if (r.status !== 304 && r.ok) { const l = await r.json(); if (Array.isArray(l)) { obsLog = l; hold(heldLog, url, r); } }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderLog();
}

// The node inspector's evidence column: this node's supporting observations (the evidence behind its
// belief), always visible beside the edge columns. Cached per node so it survives the inspector's
// poll-driven re-render, and it stays live (re-fetched, re-rendered only when the set changes).
let nodeLogCache = { id: null, list: [] };
async function fillNodeLog(node, colEl, secEl) {
  const sameNode = nodeLogCache.id === node.id;
  if (!sameNode) nodeLogCache = { id: node.id, list: [] };
  // `sources` counts attestations and this list counts observations; a re-observation by another
  // host adds the first without the second. Equal is the common case, so the second number appears
  // only when they part - which is when it says something (docs/inspector.md Section 5).
  const setCount = n => {
    if (secEl) secEl.innerHTML = html`Evidence <span class="ct">${n}${node.sources !== n ? " / " + node.sources + " attestations" : ""}</span>`;
  };
  if (sameNode && nodeLogCache.list.length) { wireObsList(colEl, nodeLogCache.list); setCount(nodeLogCache.list.length); }
  else colEl.textContent = "loading...";
  const ws = wsInput.value.trim();
  try {
    const q = "?entity=" + encodeURIComponent(node.id) + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
    const r = await fetch("/api/observations" + q, { cache: "no-store" });
    if (!r.ok) return;
    const list = await r.json();
    // Guard against a focus change mid-fetch (a later renderDetail owns the column now).
    if (!Array.isArray(list) || nodeLogCache.id !== node.id) return;
    const changed = list.map(o => o.id).join(",") !== nodeLogCache.list.map(o => o.id).join(",");
    nodeLogCache.list = list;
    if (changed || !colEl.querySelector(".obs")) wireObsList(colEl, list);
    setCount(list.length);
  } catch (e) { /* keep the last render */ }
}

// A pulse lasts a second; the key names its ring from the first one on, so the row does not blink.
function pulseNodes(ids) {
  for (const id of ids || []) if (posById.has(id)) pulses.set(id, 60);
  if (pulses.size && !pulsedAny) { pulsedAny = true; renderMarks(); }
  requestFrame();
}
function logRow(body, tip) {
  const row = document.createElement("div");
  row.className = "row";
  if (tip) row.title = tipText(tip);
  row.innerHTML = html`<span class="t">${new Date().toLocaleTimeString()}</span>${body}`;
  logEl.prepend(row);
  while (logEl.children.length > 8) logEl.lastChild.remove();
  setTimeout(() => row.remove(), 8000);
}
// Whether a workspace is part of the scope on screen. The node default is named by its nodes, so an
// empty default counts every workspace as its own until something arrives to say otherwise.
function inScope(ws) {
  const cur = currentWs();
  if (cur === "*" || cur === "all" || cur === ws) return true;
  return cur === "" && (!nodes.length || nodes.some(n => n.workspace === ws));
}
// Writes in another workspace, one notice per workspace that counts them, with Show - the person's
// act that moves the scope (U20).
const elsewhere = new Map();   // workspace -> { el, n }
function noteElsewhere(ws) {
  const was = elsewhere.get(ws);
  if (was && was.el && was.el.isConnected && !was.el.classList.contains("out")) {
    was.n += 1;
    was.el.querySelector(".tt").textContent = was.n + " new writes in " + ws;
    return;
  }
  const el = toast("info", "New in " + ws, null, { label: "Show", note: "", run: () => {
    elsewhere.delete(ws);
    wsInput.value = ws; beginWorkspaceTransition(); renderChipsActive(); poll();
  } });
  elsewhere.set(ws, { el, n: 1 });
}
// The map's activity, in sentences (design-system.md Section 29.6, U17): no tool name, abbreviation
// or mode id. Who acted is the app the local event names (Section 29.8), else "An agent".
function plural(n, one, many) { return n + " " + (n === 1 ? one : many || one + "s"); }
function actorOf(ev) { return ev.app || "An agent"; }
function wroteSaid(ev) {
  const parts = [];
  const ne = (ev.entities || []).length, nr = ev.relations || 0;
  if (ne) parts.push(plural(ne, "entity", "entities"));
  if (nr) parts.push(plural(nr, "relation"));
  return actorOf(ev) + " wrote " + (parts.length ? parts.join(" and ") : "an observation") + " in " + ev.workspace;
}
// A peer as a row names it: a node id's first eight characters, or a server's host.
function peerSaid(peer) {
  const p = String(peer || "");
  try { if (/^https?:/.test(p)) return new URL(p).host; } catch (_) { /* not an address */ }
  return p.slice(0, 8);
}
function syncSaid(ev) {
  const n = ev.count, who = peerSaid(ev.peer), ws = ev.workspace;
  if (!n) return null;   // a check-in that moved nothing writes no row
  return ev.direction === "push" ? "Sent " + n + " in " + ws + " to " + who
    : ev.direction === "pull" ? "Received " + n + " in " + ws + " from " + who
    : ev.direction === "push-received" ? who + " sent " + n + " to " + ws
    : ev.direction === "pull-served" ? who + " read " + n + " from " + ws
    : null;
}
// One sentence for an event, written once for the passing row and for Follow's notice (Section 29.6
// revises 29.4's words to these).
function activitySaid(ev) {
  const nameOf = id => { const n = nodeById(id); return n ? n.name : null; };
  if (ev.kind === "observe") return wroteSaid(ev);
  if (ev.kind === "search") return actorOf(ev) + " searched \"" + ev.query + "\" - " + (ev.hits | 0) + " found";
  if (ev.kind === "get_entity") {
    const n = ev.name || nameOf(ev.id) || "an entity";
    return ev.found ? actorOf(ev) + " looked up " + n : actorOf(ev) + " looked for " + n + " - not found";
  }
  if (ev.kind === "traverse") {
    const n = nameOf(ev.start);
    return actorOf(ev) + " followed " + (n ? n + "'s" : "an entity's") + " relations - " + (ev.reached || []).length + " reached";
  }
  if (ev.kind === "sync") return syncSaid(ev);
  return null;
}
function primaryNode(ev) {
  const id = ev.kind === "observe" ? (ev.entities || [])[0]
    : ev.kind === "traverse" ? ev.start
    : ev.kind === "get_entity" ? (ev.found ? ev.id : null)
    : ev.kind === "search" ? (ev.nodes || [])[0] : null;
  return id ? nodeById(id) : null;
}
async function handleEvent(ev) {
  // If the session (conversation) changes, reset the footprint - track the new conversation's knowledge use from the start.
  if (ev.session && ev.session !== footprintSession) { footprintSession = ev.session; footprint.clear(); footprintSince = Date.now(); }
  // Follow moves the camera, never the reader (design-system.md Section 6, U20): activity in the scope
  // on screen frames what it touched below; a write in another workspace is a notice with Show, and
  // the scope, the selection and the open sheet stay where the person left them.
  if (ev.workspace && !inScope(ev.workspace)) {
    if (ev.kind === "observe" || (ev.kind === "sync" && ev.count > 0)) noteElsewhere(ev.workspace);
    return;
  }
  let ids = [];
  if (ev.kind === "observe") {
    logRow(html`${activitySaid(ev)}`);
    await poll();                       // wait for the new nodes to enter the graph, then pulse
    ids = ev.entities || [];
  } else if (ev.kind === "search") {
    logRow(html`${activitySaid(ev)}`);
    ids = ev.nodes || [];
  } else if (ev.kind === "get_entity") {
    logRow(html`${activitySaid(ev)}`);
    ids = ev.found ? [ev.id] : [];
  } else if (ev.kind === "sync") {
    // Federation hit: who touched this store, which direction, how much - the live remote feed.
    const said = activitySaid(ev);
    if (said) logRow(html`${said}`, String(ev.peer));
    let addedNodes = [], addedEdges = [];
    if (ev.count > 0) {
      // Knowledge landed: load it now, and re-frame once the re-layout settles (follow mode) so the
      // camera presents the grown graph instead of staring at a stale corner of it. Diff the node AND
      // edge sets across the reload so a peer's cursor can tour exactly what it just contributed.
      const beforeN = new Set(nodes.map(n => n.id)), beforeE = new Set(edges.map(edgeKey));
      refitOnReveal = follow && !mapInUse();   // not while the person uses the map (Section 29.4)
      await poll();
      addedNodes = nodes.filter(n => !beforeN.has(n.id));
      addedEdges = edges.filter(e => !beforeE.has(edgeKey(e)));
    }
    if (peersOn) notePeer(ev, addedNodes, addedEdges);   // server mode: tour the peer's marker, not the camera
    ids = [];
  } else if (ev.kind === "traverse") {
    logRow(html`${activitySaid(ev)}`);
    ids = [ev.start, ...(ev.reached || [])];
  } else return;
  pulseNodes(ids);
  const had = footprint.size;
  for (const id of ids) if (id) footprint.add(id);   // accumulate the conversation footprint (regardless of whether the node exists)
  if (!had && footprint.size) renderMarks();
  // Reheat only when the event touched actual nodes - sync/hc chatter (now periodic via the
  // status loop) must never jiggle a settled layout.
  if (ids.length) wake(0.3);
  // While the person uses the map, follow waits and says what happened instead (D6, Section 29.4).
  if (follow && !peersOn && mapInUse()) noteActivity(ev, ids);
  else if (follow && !peersOn) {
    // Frame the WHOLE hit set: several hits fit into view together (pan + zoom as needed); a
    // single hit centers smoothly. The camera narrates what the agent touched. In server mode the
    // camera holds still and the peer markers move instead (a hub would otherwise jump on every hit).
    const hitNodes = ids.map(nodeById).filter(Boolean);
    if (hitNodes.length > 1) fitView(hitNodes, 130);
    else { const n = hitNodes[0] || primaryNode(ev); if (n) centerOn(n); }
  }
  const sEl = document.getElementById("session");
  // The footprint in words (Section 29.6). The session is the daemon's run, not one conversation:
  // every agent since it started, so the line says agents, and the tooltip since when.
  if (sEl) {
    sEl.textContent = footprintSession ? "Agents used " + plural(footprint.size, "entity", "entities") : "";
    sEl.title = tipText(footprintSession ? "Since " + new Date(footprintSince).toLocaleTimeString() + ", session " + footprintSession : "");
  }
}
// --- Peer markers (server mode) -----------------------------------------------------
// On a hub, many peers sync at once. Instead of the camera chasing every remote hit, each federated
// peer is a minimal cursor-dot that glides to the nodes it touched (like a remote cursor): a push shows
// the peer drifting onto the knowledge it just contributed. Deterministic per-peer color; hover for id.
function peerColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return TYPE_BAND[h % TYPE_BAND.length];   // the band, so a peer's dot is never a signal's color
}
// Centroid of a node set (or the whole visible graph) in world coords; parks at the view center when empty.
function viewCentroid(list) {
  const src = (list && list.length ? list : nodes).filter(n => !typeOff.has(n.type));
  if (!src.length) {
    const wx = (insetL() + innerWidth - insetR()) / 2, wy = (innerHeight + TOP_INSET - insetB()) / 2;
    return [(wx - cam.x) / cam.s, (wy - cam.y) / cam.s];
  }
  let sx = 0, sy = 0; for (const n of src) { sx += n.x; sy += n.y; }
  return [sx / src.length, sy / src.length];
}
function peerMarker(id) {
  let m = peerMarkers.get(id);
  if (!m) {
    const [cx, cy] = viewCentroid();
    const a = ((parseInt(id.slice(0, 4), 16) || 0) / 65535) * 6.283;   // stable per-peer angle
    m = { x: cx + Math.cos(a) * 140, y: cy + Math.sin(a) * 140, tx: cx, ty: cy,
          color: peerColor(id), phase: a, flare: 0, action: "", count: 0, seen: 0, sx: null, sy: null,
          queue: null, qi: 0, dwell: 0, arrivedAt: false };
    peerMarkers.set(id, m);
  }
  return m;
}
function edgeKey(e) { return e.from + "|" + e.type + "|" + e.to; }
// A tour of what a peer just touched: for every uploaded edge, visit its endpoints and the edge itself
// (node -> edge -> node), then any standalone uploaded nodes. Capped so a big push stays a quick hop, not
// a marathon. Targets hold live node/edge refs so the marker tracks them as the layout moves.
function buildTour(addedNodes, addedEdges) {
  const q = [], onEdge = new Set();
  for (const e of addedEdges) {
    if (e.a) { q.push({ node: e.a }); onEdge.add(e.a.id); }
    q.push({ edge: e });
    if (e.b) { q.push({ node: e.b }); onEdge.add(e.b.id); }
  }
  for (const n of addedNodes) if (!onEdge.has(n.id)) q.push({ node: n });
  return q.slice(0, 12);
}
function targetPos(t) {
  if (t.node) return [t.node.x, t.node.y];
  if (t.edge && t.edge.a && t.edge.b) return [(t.edge.a.x + t.edge.b.x) / 2, (t.edge.a.y + t.edge.b.y) / 2];
  return null;
}
// A sync hit from `ev.peer`. With per-node detail (an upload we could diff) the marker tours the exact
// nodes/edges it touched, hopping node -> edge -> node; otherwise (a pull/heartbeat we cannot attribute to
// specific nodes) it just drifts toward the workspace area. A small per-peer offset keeps peers from stacking.
function notePeer(ev, addedNodes, addedEdges) {
  if (!ev.peer) return;
  const m = peerMarker(ev.peer);
  m.flare = 1; m.action = ev.direction || ""; m.count = ev.count | 0; m.seen = Date.now();
  const tour = ((addedNodes && addedNodes.length) || (addedEdges && addedEdges.length))
    ? buildTour(addedNodes || [], addedEdges || []) : [];
  if (tour.length) { m.queue = tour; m.qi = 0; m.dwell = 0; m.arrivedAt = false; }
  else {
    m.queue = null;   // no attributable nodes - a soft glide toward where the peer is active
    const [cx, cy] = viewCentroid(null);
    m.tx = cx + Math.cos(m.phase) * 64; m.ty = cy + Math.sin(m.phase) * 64;
  }
  requestFrame();
}
function stepPeers() {
  if (!peersOn) return;
  for (const m of peerMarkers.values()) {
    if (m.queue && m.qi < m.queue.length) {
      const t = m.queue[m.qi], p = targetPos(t);
      if (p) { m.tx = p[0]; m.ty = p[1]; }
      m.x += (m.tx - m.x) * 0.2; m.y += (m.ty - m.y) * 0.2;   // fast hop between things it touched
      if (Math.hypot(m.tx - m.x, m.ty - m.y) < 6 / cam.s) {
        if (!m.arrivedAt) {   // attach once on arrival: ring the node / flash the edge
          m.arrivedAt = true;
          if (t.node) pulseNodes([t.node.id]);
          else if (t.edge) peerEdgeFlash.set(edgeKey(t.edge), { e: t.edge, color: m.color, life: 40 });
        }
        if (++m.dwell > 7) { m.qi++; m.dwell = 0; m.arrivedAt = false; }   // brief dwell, then next
      }
    } else {
      m.x += (m.tx - m.x) * 0.07; m.y += (m.ty - m.y) * 0.07;   // idle drift toward the resting target
    }
    m.flare = m.flare > 0.002 ? m.flare * 0.95 : 0;
  }
  for (const [k, f] of peerEdgeFlash) if (--f.life <= 0) peerEdgeFlash.delete(k);
}
function drawPeers() {
  if (!peersOn || (!peerMarkers.size && !peerEdgeFlash.size)) return;
  ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  // Edges a peer just visited (fading), so an upload reads as the cursor attaching to the connections
  // it created, not only the nodes.
  for (const f of peerEdgeFlash.values()) {
    const e = f.e; if (!e.a || !e.b) continue;
    const ax = e.a.x * cam.s + cam.x, ay = e.a.y * cam.s + cam.y, bx = e.b.x * cam.s + cam.x, by = e.b.y * cam.s + cam.y;
    ctx.globalAlpha = Math.min(1, f.life / 40) * 0.85;
    ctx.strokeStyle = f.color; ctx.lineWidth = 2.5;
    ctx.beginPath(); ctx.moveTo(ax, ay); ctx.lineTo(bx, by); ctx.stroke();
  }
  const t = performance.now() / 1000, now = Date.now();
  for (const m of peerMarkers.values()) {
    const touring = m.queue && m.qi < m.queue.length;
    const bob = touring ? 0 : Math.sin(t * 1.2 + m.phase) * 2.5;   // idle drift; hold steady while touring
    const px = m.x * cam.s + cam.x, py = m.y * cam.s + cam.y + bob;
    m.sx = px; m.sy = py;
    const live = Math.max(0, 1 - (m.seen ? (now - m.seen) / 1000 : 999) / 90);   // dim as a peer goes quiet
    const r = 3.5 + m.flare * 5;
    const g = ctx.createRadialGradient(px, py, 0, px, py, r * 3.4);
    g.addColorStop(0, m.color); g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.globalAlpha = 0.22 * (0.4 + 0.6 * live) + m.flare * 0.5;
    ctx.fillStyle = g; ctx.beginPath(); ctx.arc(px, py, r * 3.4, 0, 7); ctx.fill();
    if (touring && m.arrivedAt) {   // attached ring at the node/edge it is currently on
      ctx.globalAlpha = 0.9; ctx.strokeStyle = m.color; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.arc(px, py, r + 4, 0, 7); ctx.stroke();
    }
    ctx.globalAlpha = 0.45 + 0.55 * live;
    ctx.fillStyle = m.color; ctx.beginPath(); ctx.arc(px, py, r, 0, 7); ctx.fill();
    ctx.lineWidth = 1; ctx.strokeStyle = "rgba(0,0,0,0.55)"; ctx.stroke();
  }
  ctx.globalAlpha = 1;
}
function peerAt(cx, cy) {
  if (!peersOn) return null;
  for (const [id, m] of peerMarkers) if (m.sx != null && Math.hypot(cx - m.sx, cy - m.sy) <= 11) return { id, m };
  return null;
}
function showPeerTip(m, id, cx, cy) {
  tip.style.display = "block";
  tip.style.left = Math.min(cx + 14, innerWidth - 330) + "px";
  tip.style.top = (cy + 14) + "px";
  const ago = m.seen ? Math.max(0, Math.round((Date.now() - m.seen) / 1000)) : null;
  tip.innerHTML = html`${[
    html`<b>peer ${id.slice(0, 6)}</b><br>`,
    html`<span class="k">machine</span> ${id.slice(0, 16)}<br>`,
    html`<span class="k">last</span> ${m.action || "-"}${ago !== null ? " " + ago + "s ago" : ""} `,
    html`&nbsp; <span class="k">hits</span> ${m.count | 0}`]}`;
}
// Seed/refresh markers from the peer roster so idle peers (that only advertise, which is not a live
// event) still appear and stay lit. Only metadata + `seen` are touched - a marker's glide target/flare
// belong to the live Sync events (notePeer), so a roster refresh never yanks a moving cursor.
function seedPeers(f) {
  for (const p of (f.known_peers || [])) {
    const m = peerMarker(p.node_id);
    m.action = p.last_action || ""; m.count = p.hits | 0;
    const ageMs = f.updated_ms && p.last_seen_ms ? Math.max(0, f.updated_ms - p.last_seen_ms) : 0;
    m.seen = Date.now() - ageMs;   // keep `seen` in client-clock terms (avoid hub/viewer clock skew)
  }
  requestFrame();
}
// A hub gathers peers - auto-enter server mode: draw peer cursors and stop the camera from chasing
// remote hits (the owner can still toggle either). On a client/loopback viewer this is a no-op.
async function detectServerMode() {
  try {
    const f = await (await fetch("/api/federation", { cache: "no-store" })).json();
    if (!f || f.role !== "hub") return;
    serverMode = true;
    peersOn = true; setSwitch(peersBtn, true);
    follow = false; renderFollow();
    seedPeers(f);
  } catch (_) { /* federation status unavailable - stay in client mode */ }
}
// Keep the roster fresh on a hub so a peer that only heartbeats (advertise emits no live event) still
// shows up within a few seconds of checking in, and quiet peers do not fade out while still present.
async function refreshPeerRoster() {
  if (!serverMode) return;
  try { seedPeers(await (await fetch("/api/federation", { cache: "no-store" })).json()); } catch (_) {}
}
function connectEvents() {
  try {
    const es = new EventSource("/api/events");
    es.onmessage = e => { try { handleEvent(JSON.parse(e.data)); } catch (_) {} };
    // On error, EventSource reconnects automatically.
  } catch (_) { /* EventSource unsupported - works with polling alone */ }
}

