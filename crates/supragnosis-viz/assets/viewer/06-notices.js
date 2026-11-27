// --- Live MCP activity (SSE) --------------------------------------------------------
function nodeById(id) { return nodesById().get(id); }
// The nodes by id, kept until the node list is replaced (applyGraph builds a new one), so a step or
// a frame does not build its own map, and a lookup does not walk the list.
let byIdOf = null, byIdMap = new Map();
function nodesById() {
  if (byIdOf !== nodes) { byIdOf = nodes; byIdMap = new Map(nodes.map(n => [n.id, n])); }
  return byIdMap;
}
// The drawn entities of other workspaces with the same name (Section 29.16), under All only. Indexed
// once per node list, by the page's lower
// case of the trimmed name - the id's rule, which can differ from the server's on rare characters.
let namesOf = null, namesMap = new Map();
const nameKey = n => String(n.name).trim().toLowerCase();
function namesakesOf(n) {
  const cur = currentWs();
  if (!n || (cur !== "*" && cur !== "all") || !n.workspace) return [];
  if (namesOf !== nodes) {
    namesOf = nodes; namesMap = new Map();
    for (const m of nodes) { const k = nameKey(m); if (!namesMap.has(k)) namesMap.set(k, []); namesMap.get(k).push(m); }
  }
  return (namesMap.get(nameKey(n)) || []).filter(m => m.workspace && m.workspace !== n.workspace);
}
// Escape for both element-text and quoted-attribute contexts. Quotes must be escaped too: entity/type
// names come from untrusted observe calls (including federation sync - Principle 18, writes are an attack
// surface), and they are interpolated into `title="..."` / `data-id="..."` attributes below; without quote
// escaping a name like `x" onmouseover=alert(1) z="` breaks out of the attribute into an event handler.
function esc(s) { return String(s).replace(/[<&>"']/g, c => ({ "<": "&lt;", "&": "&amp;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])); }

// Markup, the one way this file writes HTML. Every interpolated value is escaped unless it is itself
// markup built by `html`; an array is its items in order; null, undefined and false are nothing.
// no-unsanitized accepts an innerHTML sink only when its value is an `html` template
// (eslint.config.js), so text that never passed through the tag cannot reach the DOM unescaped - and
// a fragment someone forgot to build with it shows up on screen as visible tags, not as markup.
function Markup(s) { this.s = s; }
Markup.prototype.toString = function () { return this.s; };
// A tooltip names a control and its key, and explains nothing a person needs to act (Section 4, U25):
// it shows only to a pointer that rests. Every tooltip the script writes goes through here.
function tipText(name, key) { return key ? name + " (" + key + ")" : name; }
function html(strings, ...values) {
  let out = strings[0];
  for (let i = 0; i < values.length; i++) out += markupOf(values[i]) + strings[i + 1];
  return new Markup(out);
}
function markupOf(v) {
  if (v instanceof Markup) return v.s;
  if (Array.isArray(v)) return v.map(markupOf).join("");
  if (v === null || v === undefined || v === false) return "";
  return esc(String(v));
}
// An icon (docs/design-system.md Section 28, U41): Lucide's file, drawn by the `.i` classes in
// viewer.css as a mask over the text's color. Hidden from a screen reader: the control it sits in is
// named by its act.
function icon(name) {
  const el = document.createElement("span");
  el.className = "i i-" + name;
  el.setAttribute("aria-hidden", "true");
  return el;
}
// The contested badge's mark (Section 28.3, U41): circle-alert's bar and dot, from
// icons/circle-alert.svg in its own 24-unit grid, with the badge's disc of radius r taken as the
// icon's 10-unit circle - Lucide's 2-unit stroke, never under 1.5 pixels at the camera's scale s.
const ALERT_MARK = new Path2D("M12 8v4M12 16h.01");
function alertMark(c, x, y, r, color, s) {
  const k = r / 10;
  c.save();
  c.translate(x - 12 * k, y - 12 * k); c.scale(k, k);
  c.lineCap = "round"; c.strokeStyle = color; c.lineWidth = Math.max(2, 1.5 / (k * s));
  c.stroke(ALERT_MARK);
  c.restore();
}

// --- Notices: every write answers (Principle 24) -------------------------------------------------
// The console's writes used to drop the server's answer and wait for the next poll to change the
// panel, so a refusal - a merge the fold blocks - looked exactly
// like a success that had not arrived yet. Each one now reports here, in the anatomy the app's pages
// share (design-system.md Section 28.4, U42): its kind's icon, a title, at most a detail and one
// action, and a dismiss. At most four at a time, the newest first.
// Built from text nodes, never markup: an error string can carry an entity name.
const toastsEl = document.getElementById("toasts");
// ok - an act done; bad - an act refused or not done; info - news that is not the outcome of the
// person's act, such as a write in another workspace.
const NOTICE_ICON = { ok: "circle-check", bad: "circle-alert", info: "info" };
// A notice may carry one action - { label, note, run } - such as Undo: the note says what the
// action does, in words beside it, and the notice stays until it is used or dismissed, as an error
// does, so the action is not taken away while it is being read. On the Review page an action marked
// `decided` is also on its Decided item, so there the notice leaves as one without an action does
// (Section 29.3, U42).
function toast(kind, title, detail, action) {
  if (!toastsEl) return;
  const el = document.createElement("div");
  el.className = "toast " + kind;
  const body = document.createElement("div"); body.className = "tb";
  const t = document.createElement("div"); t.className = "tt"; t.textContent = title;
  body.appendChild(t);
  if (detail) { const d = document.createElement("div"); d.className = "td"; d.textContent = detail; body.appendChild(d); }
  if (action && action.note) { const n = document.createElement("div"); n.className = "td"; n.textContent = action.note; body.appendChild(n); }
  el.append(icon(NOTICE_ICON[kind] || "info"), body);
  if (action && action.run) {
    const act = document.createElement("button");
    act.type = "button"; act.className = "tact"; act.textContent = action.label;
    act.onclick = () => { dismiss(el); action.run(); };
    el.appendChild(act);
  }
  const close = document.createElement("button");
  close.type = "button"; close.className = "tx"; close.setAttribute("aria-label", word("dismiss"));
  close.appendChild(icon("x"));
  close.onclick = () => dismiss(el);
  el.appendChild(close);
  toastsEl.prepend(el);
  if (kind === "ok" && (!action || (action.decided && reviewOpen()))) leaveAfter(el, 4500);
  while (toastsEl.children.length > 4) toastsEl.lastChild.remove();
  return el;
}
// A success leaves by itself 4.5 seconds after it shows, counted only while it is not being read: the
// pointer over it or focus inside it stops the clock, and the whole wait starts again once both have
// left (WCAG 2.2.1).
function leaveAfter(el, ms) {
  let timer = 0, pointer = false, focus = false;
  const sync = () => { clearTimeout(timer); if (!pointer && !focus) timer = setTimeout(() => dismiss(el), ms); };
  el.addEventListener("pointerenter", () => { pointer = true; sync(); });
  el.addEventListener("pointerleave", () => { pointer = false; sync(); });
  el.addEventListener("focusin", () => { focus = true; sync(); });
  el.addEventListener("focusout", (e) => { focus = el.contains(e.relatedTarget); sync(); });
  sync();
}
// A notice leaves by fading over 120 ms (Section 3), and at once under reduced motion.
function dismiss(el) {
  if (!el.isConnected || el.classList.contains("out")) return;
  if (REDUCED_MOTION) { el.remove(); return; }
  el.classList.add("out");
  setTimeout(() => el.remove(), 120);
}
// One console write: GET path+query (the viewer's writes are GETs on the 0600 socket - see the review
// handler below), then a notice saying what happened. Returns the parsed answer, or null when the
// write did not land.
// `quiet`: the caller takes several acts and says what they came to once (Section 29.17), so this
// answers { ok, body, why } and shows nothing; the outcome is still reported, by the caller.
async function consoleWrite(path, query, said, action, detail, quiet) {
  let r;
  try { r = await fetch(path + query, { cache: "no-store" }); }
  catch (e) {
    if (quiet) return { ok: false, why: word("serverNotAnswering") };
    toast("bad", word("couldNotReach"), word("nothingWasChanged")); return null;
  }
  let body = null;
  try { body = await r.json(); } catch (_) { /* a refusal without a JSON body */ }
  // A refusal's reason is the server's sentence, shown as it wrote it (docs/i18n.md Section 5).
  if (quiet) return r.ok ? { ok: true, body: body || {} } : { ok: false, why: (body && body.error) || word("serverAnsweredStatus", { status: r.status }) };
  if (!r.ok) {
    toast("bad", word("refusedTitle"), (body && body.error) || word("serverAnswered", { status: r.status }));
    return null;
  }
  waitAllAt = 0;   // what waits elsewhere is read again after an act (Section 29.21)
  // An act that found itself already in force changed nothing, and says so (Section 29.23); a
  // caller whose notice reads what was found says it in its own words.
  if (body && body.found && body.state === "merged" && typeof said !== "function") {
    toast("ok", word("alreadySo"));
    return body;
  }
  // The notice may read what the act found (Section 29.23).
  if (typeof said === "function") said = said(body || {});
  // An action may be built once the write has landed - Undo reads its own preview (Section 29.2).
  if (typeof action === "function") action = await action(body || {});
  toast("ok", said, detail || null, action);
  return body || {};
}

// What an act that opens a merge or an un-merge found already there (Section 29.23), or null when it
// opened one: the notice says it, and the caller opens the item.
function foundSaid(body, noun) {
  if (!body || !body.found) return null;
  if (body.state === "open" || body.state === "blocked") {
    return word(noun === "merge" ? "mergeWaits" : "unmergeWaits");
  }
  if (body.state === "merged") return word(noun === "merge" ? "alreadyMerged" : "alreadyUnmerged");
  return null;
}

// Type glossary (T-Box) section body: entity types and relation types with their define_type
// definitions. A type whose definition is contested (distinct definitions tie at the top tier -
// IR5) shows the competing definitions with a confirm action, the same mediation as an entity kind.
function renderGlossary() {
  const group = t => glossaryTypes.filter(x => x.target === t);
  // The T-Box is scoped to the workspace (P11), so an all-workspaces read can legitimately contain
  // the same NAME twice - two workspaces defining `Widget` have defined two different things. Label
  // the workspace only when the view actually spans more than one, so the scoped glossary stays
  // uncluttered and the label appears exactly where it carries information.
  const spansWorkspaces = new Set(glossaryTypes.map(x => x.workspace || "")).size > 1;
  const item = x => {
    const parts = [html`<div class="item"><span class="nm">${x.name}</span>`,
      spansWorkspaces ? html`<span class="gws">${String(x.workspace || "")}</span>` : "",
      html`<span class="src">${word((x.sources | 0) === 1 ? "oneSource" : "sources", { n: x.sources | 0 })}</span>`,
      html`<div class="def">${x.description}</div>`];
    // Contested definitions take the same settling rows as an entity's type, so a type conflict and
    // an entity-type conflict read identically - one mediation pattern (IR5).
    if (x.competitors && x.competitors.length) {
      parts.push(html`<div class="contested${x.contested ? " hot" : ""}">${contestedRows({ name: x.name, field: "definition",
        current: x.description, curObs: x.def_source, tier: "", competitors: x.competitors, contested: x.contested })}</div>`);
    }
    return html`${parts}</div>`;
  };
  const section = (title, items) => html`<div class="gsec">${title}</div>${
    items.length ? items.map(item) : html`<div class="empty">${word("noneDefined")}</div>`}`;
  const entities = group("entity"), relations = group("relation");
  glossaryBodyEl.innerHTML = html`${section(word("entityTypesN", { n: entities.length }), entities)}${
    section(word("relationTypesN", { n: relations.length }), relations)}`;
  glossCtEl.textContent = glossaryTypes.length || "";
  // An empty scope shows Definitions only while it has one (Section 29.10).
  document.getElementById("defsBox").classList.toggle("none", !glossaryTypes.length);
  // A contested definition opens the list, so its mediation act is in view (P6); otherwise the list
  // stays as the person left it.
  if (glossaryTypes.some(x => x.contested)) document.getElementById("defsBox").open = true;
  wireSettle(glossaryBodyEl, renderGlossary);
}

// Fetch the glossary for the current workspace, then render (only meaningful while the section is open).
async function refreshGlossary() {
  // Always fetched (not gated on the Types tab): the legend chip tooltips read glossaryTypes too,
  // so the vocabulary must be warm even when the glossary panel is closed. Tiny loopback GET.
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/types" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const t = await r.json(); if (Array.isArray(t)) glossaryTypes = t; }
  } catch (e) { /* glossary is auxiliary - keep the last render */ }
  renderGlossary();
}

// Why the merge band came back empty. An empty list alone conflates three different states, and
// only one of them is "there are no near-name pairs" - the other two are "this signal cannot run
// here" and "it ran over part of the workspace" (Principle 5: absence is not a negation). The
// server reports which via curation.merge_band; older payloads without the field read as computed.
function bandEmptyReason(band) {
  if (band && band.available === false) return word("bandUnavailable");
  // What it held back, said apart from absence (Section 29.27): a count of 0 is a known none.
  const held = band && band.held_back != null ? band.held_back : null;
  const answered = held ? word(held === 1 ? "heldNearOne" : "heldNearN", { n: held }) : "";
  if (band && band.embedded < band.examined) {
    const part = { embedded: band.embedded, examined: band.examined, held: answered };
    return word(held ? "bandPartialHeld" : "bandPartial", part);
  }
  if (held === null) return word("bandNoneWaiting");
  return held ? word("noneWaiting", { held: answered }) : word("bandNone");
}

// Why the name-variant ladder came back empty (Section 29.28): it always runs, so its line says only
// whether no names match or the ones it found are all answered. An older payload without the count
// says nothing by name, as before.
function ladderEmptyReason(ladder) {
  if (!ladder || ladder.held_back == null) return "";
  const held = ladder.held_back;
  return held
    ? word("noneWaiting", { held: word(held === 1 ? "heldAlikeOne" : "heldAlikeN", { n: held }) })
    : word("ladderNone");
}

// Read-only curation signals (Principle 7, generate-not-commit): merge candidates / grab-bags / orphans.
// Clicking a node chip only focuses it - the panel commits nothing (no gate).
// Suggestions and Health as queues (Section 7, Section 29.3): each signal one item in the list, its
// detail and act on the right, J and K between them. A signal is generated and commits nothing
// (Principle 7): its act opens a proposal or names a group, and the item leaves once that lands.
function signalItems(seg) {
  if (!curation) return [];
  const items = [];
  const add = (key, title, body) => items.push({ key, kind: "signal", title, body });
  const names = ms => (ms || []).map(m => m.name);
  if (seg === "suggestions") {
    // Merge band (resolution-identity.md Section 3, Principle 15): embedding-near distinct-name pairs.
    for (const m of curation.merge_suggestions || []) {
      const sim = { sim: m.similarity.toFixed(2), n: m.shared_neighbors };
      add("m:" + m.a + "|" + m.b, word("possibleDuplicate", { a: m.a_name, b: m.b_name }),
        { pair: m, meta: word(m.shared_neighbors ? "similarShared" : "similar", sim) });
    }
    // Name variants: the deterministic sibling of the band. A pair can be proposed; three or more show
    // as chips, since propose_merge takes a pair. data-src tells the server which surface found it.
    for (const v of curation.name_variants || []) {
      const m = v.members || [];
      const by = { rung: Object.hasOwn(RUNGS, v.rung) ? word(RUNGS[v.rung]) : v.rung, n: v.shared_neighbors };
      const meta = word(v.shared_neighbors ? "groupedByShared" : "groupedBy", by);
      if (m.length === 2) add("v:" + m[0].id + "|" + m[1].id, word("possibleDuplicate", { a: m[0].name, b: m[1].name }),
        { pair: { a: m[0].id, b: m[1].id, a_name: m[0].name, b_name: m[1].name }, src: "variant:" + v.rung, meta });
      else add("v:" + v.key, word("possibleDuplicates", { names: nameList(names(m)) }), { group: m, meta });
    }
    // A name two workspaces share is two entities (Section 29.16): titled as written, each chip saying
    // its workspace, read from the graph the page holds.
    for (const g of curation.duplicates || []) {
      const ms = (g.members || []).map(m => { const n = nodeById(m.id); return n && n.workspace ? { ...m, ws: n.workspace } : m; });
      const spaces = new Set(ms.map(m => m.ws).filter(Boolean)).size || ms.length;
      add("d:" + g.key, word("sameNameIn", { n: spaces, name: ms[0] ? ms[0].name : g.key }), { group: ms, hint: word("sameNameWhy") });
    }
  } else {
    // Contradictory accepted merges (Principle 6): the remedy is a settling merge, never an edit.
    for (const c of curation.merge_cycles || []) add("y:" + names(c.members).join("|"), word("conflictingMerges", { names: nameList(names(c.members)) }),
      { group: c.members || [], hint: word("conflictingWhy") });
    for (const b of curation.grab_bags || []) add("g:" + b.id, word("overloadedRegion", { n: b.size }), { bag: b });
    for (const o of curation.orphans || []) add("o:" + o.id, word("unlinked", { name: o.name }), { node: o });
    // Conflicts trust settled (resolution.md Section 4.1): informational, and still listed (P6).
    for (const c of (curation.contradictions || []).filter(c => !c.contested)) {
      add("k:" + c.id, word(c.field === "definition" ? "settledDefinition" : "settledType", { name: c.name }), { settled: c });
    }
    // T-Box axis collisions (Principle 9): a name defined on both axes - informative, usually a mistake.
    for (const n of curation.type_axis_collisions || []) add("x:" + n, word("bothAxes", { name: n }), { axis: n });
    // A verdict filed away from its proposal decides nothing (proposal-workflow.md I2); an earlier
    // build filed one cast under All in the node default. Listed under All on this node only
    // (design-system.md Section 29.15).
    for (const v of curation.stray_verdicts || []) {
      const p = proposals.find(q => q.id === v.proposal);
      if (!p || (p.state !== "open" && p.state !== "blocked")) continue;   // listed while the proposal still waits
      add("sv:" + v.observation, word("strayVerdict", { title: proposalTitle(p) }), { stray: v });
    }
  }
  return items;
}
// The line over a signal queue: what it is, and for the merge band, why it may be empty (P5).
function signalHint(seg) {
  if (seg === "suggestions") {
    const band = curation && !(curation.merge_suggestions || []).length ? word("byMeaning", { why: bandEmptyReason(curation.merge_band) }) : "";
    // By name after by meaning: two counts, never a total - a pair both near and alike is in each.
    const ladder = curation && !(curation.name_variants || []).length ? ladderEmptyReason(curation.name_ladder) : "";
    const name = ladder ? word("byName", { why: ladder }) : "";
    return html`<div class="hint">${[word("suggestionsHint"), band, name].filter(Boolean).join(" ")}</div>`;
  }
  if (seg === "health") return html`<div class="hint">${word("healthHint")}</div>`;
  return "";
}
function nchipHtml(n) {
  return html`<button type="button" class="nchip" data-id="${n.id}" title="${tipText(word("focusName", { name: n.name }))}">${n.ws ? n.name + " - " + n.ws : n.name}${n.type ? html`<span class="ty">${n.type}</span>` : ""}</button>`;
}
// What a stray verdict's act is, by its decision (Section 29.15).
const STRAY = { merge: "strayAccept", reject: "strayReject", withdraw: "strayWithdraw", other: "strayOther" };
// A name variant's rung, by its id (proposal_types.rs VariantRung).
const RUNGS = { separator: "rungSeparator", plural: "rungPlural", alias: "rungAlias" };
function signalDetail(it) {
  const b = it.body;
  const parts = [html`<h3>${it.title}</h3>`];
  if (b.pair) {
    const m = b.pair;
    parts.push(html`<div class="ms" data-a="${m.a}" data-b="${m.b}"${b.src ? html` data-src="${b.src}"` : ""}>`,
      nchipHtml({ id: m.a, name: m.a_name }), html`<span class="msx">~</span>`, nchipHtml({ id: m.b, name: m.b_name }),
      html`<span class="mssim">${b.meta}</span>`,
      html`<button class="propmerge" title="${tipText(word("proposeMerge"))}">${word("propose")}</button>`, html`</div>`);
    // What Propose does is said here, once, rather than in the button's tooltip (Section 4, U25).
    parts.push(html`<p class="hint">${word("proposeWhy")}</p>`);
  } else if (b.group) {
    parts.push(html`<div class="chips">${b.group.map(nchipHtml)}</div>`);
    if (b.meta) parts.push(html`<p class="hint">${b.meta}</p>`);
    if (b.hint) parts.push(html`<p class="hint">${b.hint}</p>`);
  } else if (b.bag) {
    const nm = b.bag.member_names.slice(0, 10).join(", ") + (b.bag.member_names.length > 10 ? ", ..." : "");
    parts.push(html`<div class="gb" data-hid="${b.bag.id}"><span class="sz">${b.bag.size}</span>${nm}<button class="reify" title="${tipText(word("nameRegion"))}">${word("nameRegion")}</button></div>`);
  } else if (b.node) {
    parts.push(html`<div class="chips">${nchipHtml(b.node)}</div>`, html`<p class="hint">${word("unlinkedWhy")}</p>`);
  } else if (b.settled) {
    const c = b.settled, others = (c.competitors || []).map(x => x.value);
    const said = { current: c.current, others: others.join(", ") };
    parts.push(html`<p class="why">${word(others.length > 1 ? "settledShownN" : "settledShownOne", said)}</p>`);
  } else if (b.axis) {
    parts.push(html`<p class="why">${word("bothAxesWhy")}</p>`);
  } else if (b.stray) {
    const v = b.stray, act = Object.hasOwn(STRAY, v.decision) ? v.decision : "other";
    parts.push(html`<p class="why">${word(STRAY[act], { filed: v.filed_in, waits: v.proposal_in })}</p>`);
    if (proposals.some(q => q.id === v.proposal)) parts.push(html`<div class="rv-acts"><button type="button" class="btn" data-stray="${v.proposal}">${word("showProposal")}</button>${skipButton()}</div>`);
    else parts.push(html`<div class="rv-acts">${skipButton()}</div>`);
    parts.push(moveHint());
    return parts;
  }
  if (itemNodes(it).length) parts.push(html`<div class="rv-acts"><button type="button" class="btn" data-map="1">${word("showOnMap")}</button>${skipButton()}</div>`);
  else parts.push(html`<div class="rv-acts">${skipButton()}</div>`);
  parts.push(moveHint());
  return parts;
}
// The line under a queue's item that names its keys.
function moveHint() { return html`<p class="hint"><kbd>J</kbd> <kbd>K</kbd> ${word("moveBetween")}</p>`; }
// The handlers a rendered curation or review fragment needs, bound on its container.
function wireCuration(host) {
  // A finding names an observation without showing its text; clicking is the separate, deliberate act
  // of looking at it. The Log holds the loaded rows, so fall back to opening it when this scope has
  // not loaded the one being pointed at rather than silently doing nothing.
  host.querySelectorAll("[data-obs]:not(.confirm)").forEach(el => {
    el.onclick = async () => {
      const o = await observationById(el.getAttribute("data-obs"));
      if (o) showObsCard(o); else if (!dockREl.classList.contains("on")) toggleDock(false);
    };
  });
  host.querySelectorAll(".nchip[data-id]").forEach(c => {
    c.onclick = (ev) => {
      ev.stopPropagation();
      const n = nodeById(c.dataset.id);
      if (!n) return;
      // From Review, the map is a preview with its way back (Section 29.3).
      if (reviewOpen()) { previewOnMap(segItems().find(x => x.key === reviewSel), n); return; }
      select(n);
    };
  });
  wireSettle(host, () => { reviewSig = ""; renderReview(); });
  // Name this region (Principle 11 promotion path): name the region, assert it as an entity
  // through /api/reify (an ordinary lineage-bearing observation - free ingest, no gate on the
  // assertion). DOM-built inline form (no innerHTML sink for the user-typed name).
  host.querySelectorAll(".gb .reify").forEach(b => {
    b.onclick = (ev) => {
      ev.stopPropagation();
      const row = b.closest(".gb");
      if (!row || row.querySelector(".reifyform")) return;
      const form = document.createElement("span");
      form.className = "reifyform";
      const inp = document.createElement("input");
      inp.placeholder = word("regionNameOptional");
      inp.onclick = (e2) => e2.stopPropagation();
      const ok = document.createElement("button");
      ok.type = "button"; ok.title = tipText(word("nameRegion"));
      ok.setAttribute("aria-label", word("nameRegion"));
      ok.appendChild(icon("check"));
      ok.onclick = async (e2) => {
        e2.stopPropagation();
        const ws = wsInput.value.trim();
        let q = "?hyperedge=" + encodeURIComponent(row.dataset.hid);
        if (inp.value.trim()) q += "&name=" + encodeURIComponent(inp.value.trim());
        if (ws) q += "&workspace=" + encodeURIComponent(ws);
        await consoleWrite("/api/reify", q, word("regionNamed"));
        await poll();   // the group node + member_of edges appear; the page re-renders
      };
      form.append(inp, ok);
      row.appendChild(form);
      inp.focus();
    };
  });
  // Propose (Principle 15): opens an entity_merge through the gate (/api/propose_merge). The pair
  // leaves the suggestions (now in flight) and waits under Needs you for a verdict.
  host.querySelectorAll(".ms .propmerge").forEach(b => {
    b.onclick = async (ev) => {
      ev.stopPropagation();
      const row = b.closest(".ms");
      if (!row) return;
      const ws = wsInput.value.trim();
      let q = "?a=" + encodeURIComponent(row.dataset.a) + "&b=" + encodeURIComponent(row.dataset.b);
      if (row.dataset.src) q += "&src=" + encodeURIComponent(row.dataset.src);
      // Under All the server files the merge where its pair lives (design-system.md Section 29.15).
      if (ws) q += "&workspace=" + encodeURIComponent(ws);
      const o = await consoleWrite("/api/propose_merge", q, b => foundSaid(b, "merge") || word("mergeProposed"));
      await poll();
      if (o && o.found) openReviewItem(o.proposal_id);
    };
  });
}

// Curation is heavier than the graph, so it is read when the Review page is open and on every sixth
// poll otherwise - often enough for the review count to carry suspected secrets and contested beliefs.
let curationBeat = 0;
async function refreshCuration() {
  if (!reviewOpen() && (curationBeat++ % 6) !== 0) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/curation" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const c = await r.json(); if (!c.error) curation = c; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderReview();
}

function nameOf(id) { const n = nodeById(id); return n ? n.name : HASH_ID.test(id) ? word("entityNotOnMap") : "(" + id + ")"; }

