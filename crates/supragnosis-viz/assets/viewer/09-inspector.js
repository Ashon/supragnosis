// --- Settling a contested value (docs/design-system.md Sections 7 and 29.2, U18) -----------------
// Confirming a value is the promotion /api/resolve casts (resolution.md Section 4.2): a claim_promotion
// of ONE source - an observation that asserts the value - with the Console merge verdict, the only
// surface that grants human_confirmed (Section 6). A claim promotion moves its source's whole bundle,
// so the ask names the source in its own words and every entity whose ring the act moves with it,
// read from the engine's diff and checks before anything is cast (/api/resolve/preview - P23: no
// merge without a diff that says what it does). Every value offers the same act at the same weight,
// and when several sources assert a value the person chooses which one. When the shown value is
// already Human confirmed the act is "Confirm instead": the shown value's source is demoted to its
// tier before any verdict and the chosen one confirmed, previewed as one result. The notice that
// follows offers Undo - the reverse acts, with what they move back read from their own preview -
// and the log keeps every event (P3). The console asserts no value of its own: "Neither is right"
// hands an agent a sentence.
let settleAsk = null;       // the row whose question is open (its representative source); survives the poll's redraw
let settleCtx = null;       // that row's belief and value
let settleFrom = null;      // the source chosen for that question
let settleView = null;      // what was read for it: { key, from, gen, state, preview, sources, supporting, curBase, why }
let settleRedraw = null;    // draws the open question's block again
let settleNeither = null;   // the block whose "Neither is right" is open; survives the redraw too
let graphGen = 0;           // bumped when a graph arrives: an open question reads its diff again when the log moves
const TIER_ORDER = ["unverified", "agent_extracted", "host_signed", "human_confirmed"];
// The checks a settle runs (proposal-workflow.md Section 6), in words (U17).
const CHECK_WORDS = { "referential integrity": "the source is in this log", "tier stated": "the tier is stated" };
// `said` is the sentence the act's notice says, kept as the proposal's rationale, so the act's
// Decided item says it too (Section 29.3).
function settleQuery(obs, tier, kind, said) {
  const ws = wsInput.value.trim();
  return "?observation=" + encodeURIComponent(obs) + "&tier=" + encodeURIComponent(tier)
    + (kind ? "&kind=" + kind : "") + (said ? "&rationale=" + encodeURIComponent(said) : "")
    + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
}
// One or two acts, as /api/resolve/preview takes them: the second with the suffix 2.
function previewQuery(acts) {
  const ws = wsInput.value.trim();
  return "?" + acts.map((a, i) => {
    const k = i ? "2" : "";
    return "observation" + k + "=" + encodeURIComponent(a.obs) + "&tier" + k + "=" + encodeURIComponent(a.tier)
      + "&kind" + k + "=" + (a.kind || "claim_promotion");
  }).join("&") + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
}
async function readPreview(acts) {
  try {
    const r = await fetch("/api/resolve/preview" + previewQuery(acts), { cache: "no-store" });
    const body = await r.json().catch(() => null);
    if (!r.ok || !body) return { why: (body && body.error) || "What this act changes could not be read." };
    if (!body.belief_diff || !body.belief_diff.computable) return { why: (body.belief_diff && body.belief_diff.note) || "What this act changes could not be computed." };
    return { preview: body };
  } catch (_) { return { why: "What this act changes could not be read - is the daemon running?" }; }
}
// The entities whose ring an act moves, the belief's own entity aside, each once - grouped by where
// they land: "With it, Nightly export and Warehouse become Human confirmed."
function ringMoves(diff, skip, down) {
  const moved = new Map();
  for (const r of (diff && diff.reach) || []) for (const e of r.entities || []) {
    if (e.from !== e.to && e.entity !== skip) moved.set(e.entity, e);
  }
  const byTo = new Map();
  for (const e of moved.values()) { if (!byTo.has(e.to)) byTo.set(e.to, []); byTo.get(e.to).push(e.name); }
  return [...byTo.entries()].map(([to, names]) => {
    const shown = nameList(names);
    const many = names.length > 1;
    const verb = down ? (many ? " drop to " : " drops to ")
      : TIER_ORDER.indexOf(to) === 3 ? (many ? " become " : " becomes ") : (many ? " go back to " : " goes back to ");
    return shown + verb + tierName(to);
  });
}
// "Nightly export and Warehouse", "A, B and C", "A, B, C, D and 3 more".
function nameList(names) {
  const shown = names.slice(0, 4), more = names.length - shown.length;
  if (more > 0) return shown.join(", ") + " and " + more + " more";
  return shown.length > 1 ? shown.slice(0, -1).join(", ") + " and " + shown[shown.length - 1] : shown.join("");
}
// The question's sources and its diff. Reads /api/explain for the value's sources (and the shown
// value's tier before any verdict, which "Confirm instead" demotes its source to), then the preview.
async function readSettle() {
  const ctx = settleCtx, key = settleAsk, from = settleFrom, gen = graphGen;
  if (!ctx) return;
  settleView = { key, from, gen, state: "reading" };
  if (settleRedraw) settleRedraw();
  let sources = [{ observation: from }], supporting = new Map(), curBase = null, why = null;
  if (ctx.entity) {
    try {
      const r = await fetch("/api/explain?entity=" + encodeURIComponent(ctx.entity), { cache: "no-store" });
      if (r.ok) {
        const ex = await r.json();
        for (const o of ex.supporting || []) supporting.set(o.id, o);
        const kind = (ex.fields || []).find(f => f.field === "kind");
        const cands = (kind && kind.candidates) || [];
        const mine = cands.find(c => c.value === ctx.value);
        if (mine && (mine.observations || []).length) sources = mine.observations;
        const cur = cands.find(c => c.value === ctx.curValue);
        const cs = cur && (cur.observations || []).find(x => x.observation === ctx.curObs);
        if (cs) curBase = cs.base_tier;
      }
    } catch (_) { /* the preview below still answers, and says when it cannot */ }
  }
  let preview = null;
  if (ctx.instead && !curBase) why = "Could not read the tier " + ctx.curValue + " had before any verdict, so nothing would be changed.";
  else {
    const acts = ctx.instead
      ? [{ obs: ctx.curObs, tier: curBase, kind: "claim_demotion" }, { obs: from, tier: "human_confirmed" }]
      : [{ obs: from, tier: "human_confirmed" }];
    const got = await readPreview(acts);
    preview = got.preview || null; why = got.why || null;
  }
  if (settleAsk !== key || settleFrom !== from) return;   // the question moved on while this was read
  settleView = { key, from, gen, state: preview ? "ok" : "err", preview, sources, supporting, curBase, why };
  if (settleRedraw) settleRedraw();
}
// Who said a source and when, and its first words, from the explain read.
function sourceWho(v, obs) {
  const o = v && v.supporting && v.supporting.get(obs);
  const a = authorOf(o);
  return a ? [writerFor(a), obsWhen(a.observed_at)].filter(Boolean).join(" - ") : "";
}
function firstWords(v, obs) {
  const o = v && v.supporting && v.supporting.get(obs);
  const t = o ? String(o.content || "").trim() : "";
  return t.length > 48 ? t.slice(0, 48).trimEnd() + "..." : t;
}
// The open question: the act, the source it confirms in its own words, the rings it moves, what else
// the source states, what Human confirmed means, the checks, and Confirm - which acts only once all
// of that has been read (U18, U19: a waiting or refused Confirm says why beside it).
function askHtml(ctx) {
  const v = settleView && settleView.key === ctx.key ? settleView : null;
  const q = ctx.instead ? "Confirm " + ctx.value + " instead?" : "Confirm " + ctx.value + " as " + ctx.name + "'s " + ctx.field + "?";
  const sid = "cst-" + String(ctx.key).slice(0, 12);
  const parts = [html`<div class="cq">${q}</div>`];
  if (v && v.sources && v.sources.length > 1) {
    parts.push(html`<div class="cfrom" role="radiogroup" aria-label="Confirm from"><span class="cfl">Confirm from</span>${v.sources.map(src => html`<label class="cfr"><input type="radio" name="${"cfrom-" + ctx.key}" value="${src.observation}" data-settle="from"${src.observation === settleFrom ? html` checked` : ""}><span>${[sourceWho(v, src.observation) || tierName(src.trust_tier), firstWords(v, src.observation) ? "\"" + firstWords(v, src.observation) + "\"" : ""].filter(Boolean).join(" - ")}</span></label>`)}</div>`);
  }
  let can = false, state = "Reading what this confirms...";
  if (v && v.state === "err") state = v.why || "What this act changes could not be read.";
  if (v && v.state === "ok") {
    const d = v.preview.belief_diff, rows = d.reach || [];
    const mine = rows.find(r => r.observation === v.from);
    const theirs = ctx.instead ? rows.find(r => r.observation === ctx.curObs) : null;
    const fails = (v.preview.checks || []).filter(c => !c.passed);
    if (!mine) state = "This source is already " + tierName("human_confirmed") + ", so confirming it changes nothing.";
    else {
      parts.push(html`<div class="csrc">This confirms one source: <q>${mine.said.trimEnd()}${mine.cut ? "..." : ""}</q>${mine.cut ? html`<span class="sr-only"> (shortened)</span>` : ""} <span class="cby">${sourceWho(v, v.from)}</span></div>`);
      if (theirs) parts.push(html`<div class="creach">${ctx.curValue}'s source goes back to ${tierName(theirs.to)}.</div>`);
      for (const line of ringMoves(d, ctx.entity)) parts.push(html`<div class="creach">With it, ${line}.</div>`);
      const rel = (mine.relations || []).length, defs = mine.type_defs || [];
      if (rel) parts.push(html`<div class="creach">It also states ${rel} relation${rel === 1 ? "" : "s"}.</div>`);
      if (defs.length) parts.push(html`<div class="creach">It also defines ${defs.length} type${defs.length === 1 ? "" : "s"}: ${nameList(defs)}.</div>`);
      parts.push(html`<div class="cdef">${tierName("human_confirmed")}: ${TIER_MEANS.human_confirmed}</div>`);
      const words = c => CHECK_WORDS[c.name] || c.name;
      if (fails.length) state = "Blocked: " + fails.map(c => words(c) + " - " + c.detail).join("; ");
      else {
        parts.push(html`<div class="cchk"><span class="i i-check" aria-hidden="true"></span>Checks pass: ${[...new Set((v.preview.checks || []).map(words))].join(", ")}</div>`);
        can = true;
      }
    }
  }
  return html`<div class="cask" role="group" aria-label="${q}">${parts}${
    can ? "" : html`<div class="cstate" id="${sid}">${state}</div>`}<span class="cacts"><button type="button" class="confirm" data-settle="yes"${can ? "" : html` disabled aria-describedby="${sid}"`}>Confirm</button><button type="button" class="quiet" data-settle="no">Cancel</button></span></div>`;
}
// Casts what the question previewed: for "Confirm instead" the shown value's source goes back to its
// tier before any verdict first, then the chosen source is confirmed. Undo is built from the reverse
// acts' own preview, each source returning to the tier the diff says it had before (Section 29.2).
async function settleValue(a) {
  const v = a.view, d = v.preview.belief_diff;
  const before = obs => ((d.tier_changes || []).find(t => t.observation === obs) || {}).from;
  const fromWas = before(a.from) || "agent_extracted";
  const curWas = a.instead ? before(a.curObs) : null;
  if (a.instead) {
    const back = a.curValue + "'s source goes back to " + tierName(v.curBase);
    if (!await consoleWrite("/api/resolve", settleQuery(a.curObs, v.curBase, "claim_demotion", back + " (Confirm instead)"), back)) return;
  }
  const reverse = a.instead && curWas
    ? [{ obs: a.from, tier: fromWas, kind: "claim_demotion" }, { obs: a.curObs, tier: curWas, kind: "claim_promotion" }]
    : [{ obs: a.from, tier: fromWas, kind: "claim_demotion" }];
  const undo = async () => {
    const got = await readPreview(reverse);
    if (!got.preview) return { note: "Undo is not offered: what it would move back could not be read." };
    const moves = ringMoves(got.preview.belief_diff, a.entity);
    return {
      label: "Undo",
      // After "Confirm instead" the Undo reverses both acts, which no one Decided item holds, so that
      // notice stays; otherwise the same Undo is on the confirmation's Decided item (Section 29.3).
      decided: !a.instead,
      note: "Undo returns this source to " + tierName(fromWas) + " and keeps it there until someone confirms it again."
        + (moves.length ? " " + moves.join(". ") + "." : ""),
      run: async () => {
        if (!await consoleWrite("/api/resolve", settleQuery(a.from, fromWas, "claim_demotion", "Undid " + confirmed),
          "Undone - the source is " + tierName(fromWas) + " again")) return;
        if (a.instead && curWas) await consoleWrite("/api/resolve", settleQuery(a.curObs, curWas, null, "Undid " + confirmed),
          a.curValue + " is " + tierName(curWas) + " again");
        await poll();
      },
    };
  };
  const confirmed = "Confirmed " + a.value + " as " + a.name + "'s " + a.field;
  await consoleWrite("/api/resolve", settleQuery(a.from, "human_confirmed", null, confirmed), confirmed, undo);
  await poll();   // re-fold: the ring drops, the panel updates, the proposal shows as merged
  // On the Review page the item leaves Needs you at once and the next one takes focus (Section 29.3).
  if (reviewOpen()) { reviewFocus = true; await refreshCuration(); refreshProposals(); }
}
// Who said each value and when, from the explain read: its asserting observation's first attestation.
const settleWho = new Map();   // entity id -> { at, by: Map(observation -> "who - when") }
// Each value's source under it (design-system.md Section 29.7, U18): its first sentence in quotes,
// a button that opens its card, and its who-line - with seconds where two values share a minute.
async function fillSettleWho(host, entity) {
  if (!entity || !host.querySelector(".csrc")) return;
  let got = settleWho.get(entity);
  if (!got || Date.now() - got.at > 10000) {
    try {
      const r = await fetch("/api/explain?entity=" + encodeURIComponent(entity), { cache: "no-store" });
      if (!r.ok) return;
      const ex = await r.json();
      got = { at: Date.now(), by: new Map((ex.supporting || []).map(o => [o.id, o])) };
      settleWho.set(entity, got);
    } catch (_) { return; }   // the rows stand without it
  }
  host.querySelectorAll(".contested").forEach(block => {
    const lines = [...block.querySelectorAll(".csrc[data-src]")];
    const minutes = lines.map(el => { const a = authorOf(got.by.get(el.dataset.src)); return a ? obsWhen(a.observed_at) : ""; });
    lines.forEach((el, i) => {
      const o = got.by.get(el.dataset.src);
      if (!o) { el.replaceChildren(); return; }
      const shared = minutes[i] && minutes.filter(m => m === minutes[i]).length > 1;
      const q = document.createElement("button");
      q.type = "button"; q.className = "cq"; q.textContent = `"${firstSentence(o.content)}"`;
      q.title = tipText("Open the observation");
      q.onclick = (ev) => { ev.stopPropagation(); showObsCard(o); };
      const who = document.createElement("span");
      who.className = "cwho"; who.textContent = whoLine("", authorOf(o), shared);
      el.replaceChildren(q, who);
    });
  });
}
// An observation's first sentence, short enough for one line.
function firstSentence(t) {
  const s = String(t || "").trim().split(/(?<=[.!?])\s|\n/)[0] || "";
  return s.length > 90 ? s.slice(0, 90).trimEnd() + "..." : s;
}

// The rows of a contested belief: why the shown value is shown, then each value with its tier, who
// said it and when, and the act. `o`: { name, field, entity, current, curObs, tier, competitors,
// contested } - `tier` is the shown value's own tier where it is known ("" where it is not), and
// `entity` the node a "Confirm instead" reads its tier from (absent for a type definition).
function contestedRows(o) {
  const comps = o.competitors || [];
  const committed = String(o.tier) === "human_confirmed";
  const tied = o.contested && comps.length ? comps[0].trust_tier : "";
  const curTier = committed ? "human_confirmed" : (tied || o.tier || "");
  const local = surfaceKind === "local";
  const head = o.contested && committed
    ? "These values are all Human confirmed, so the newer one is shown until you decide."
    : o.contested
      ? "Two sources disagree on " + o.name + "'s " + o.field + ". Both are " + tierName(tied) + " claims, so the newer one is shown until you decide."
      : committed ? "The Human confirmed value is shown." : "The value with the higher trust is shown.";
  const row = (value, tier, obs, shown) => {
    const instead = committed && !shown;
    const ctx = { key: obs, obs, value, name: o.name, field: o.field, entity: o.entity || "",
      instead, curObs: o.curObs, curValue: o.current };
    const act = local && obs && !(committed && shown) && !(instead && !o.entity)
      ? html`<button type="button" class="confirm" data-settle="ask" data-obs="${obs}" data-ctx="${JSON.stringify(ctx)}">${instead ? "Confirm instead" : "Confirm"}</button>`
      : "";
    const asking = local && settleAsk === obs && act;
    // An open question whose diff was read before the log last moved reads it again (I7/I12 are owed,
    // so this narrows the gap between preview and cast; Section 29.2).
    if (asking && settleView && settleView.key === obs && settleView.state !== "reading" && settleView.gen !== graphGen) {
      settleView.gen = graphGen;
      setTimeout(readSettle, 0);
    }
    return html`${[
      html`<div class="crow${shown ? " cur" : ""}"><span class="cv">${value}</span>`,
      shown ? html`<span class="curtag">shown</span>` : "",
      tier ? html`<span class="ctier">${tierName(tier)}</span>` : "",
      act, html`</div>`,
      obs ? html`<div class="csrc" data-src="${obs}"></div>` : "",
      asking ? askHtml(ctx) : ""]}`;
  };
  // The sentence an agent is handed names the workspace and every value it calls wrong (Section 29.7).
  const values = [o.current, ...comps.map(c => c.value)].filter(v => v != null && v !== "");
  const where = o.ws ? " in " + o.ws : "";
  const said = values.map(v => "as " + v);
  const listed = said.length > 1 ? said.slice(0, -1).join(", ") + " and " + said[said.length - 1] : said.join("");
  const say = "In Supragnosis" + (o.ws ? " workspace " + o.ws : "") + ", " + o.name + "'s " + o.field + " is recorded " + listed
    + ", and " + (values.length > 2 ? "all are" : "both are") + " wrong. Record the correct " + o.field + " of " + o.name + where + ".";
  const key = o.name + "|" + o.field;
  return html`<div class="chead">${head}</div>${row(o.current, curTier, o.curObs, true)}${
    comps.map(c => row(c.value, c.trust_tier, c.observation, false))}${
    local ? html`<details class="neither" data-key="${key}"${settleNeither === key ? html` open` : ""}><summary><span class="i i-chevron-right" aria-hidden="true"></span>Neither is right</summary><div class="nhelp">Ask an agent to record the right value, then confirm it here.</div><div class="copyline"><code>${say}</code><button type="button" class="quiet" data-settle="copy" data-text="${say}">Copy</button></div></details>` : ""}`;
}
// The block's acts, wired after each draw; `redraw` draws the block's surface again.
function wireSettle(host, redraw) {
  host.querySelectorAll("[data-settle]").forEach(b => {
    const k = b.dataset.settle;
    if (k === "from") {
      b.onchange = () => { if (!b.checked) return; settleFrom = b.value; readSettle(); };
      return;
    }
    b.onclick = async ev => {
      ev.stopPropagation();
      if (k === "ask") {
        settleCtx = JSON.parse(b.dataset.ctx); settleAsk = settleCtx.key; settleFrom = settleCtx.obs;
        settleView = null; settleRedraw = redraw;
        redraw();
        const no = host.querySelector('[data-settle="no"]'); if (no) no.focus();
        await readSettle();
        const yes = host.querySelector('[data-settle="yes"]:not([disabled])'); if (yes) yes.focus();
      }
      else if (k === "no") { const obs = settleAsk; settleAsk = null; settleCtx = null; settleView = null; redraw(); const back = [...host.querySelectorAll('[data-settle="ask"]')].find(x => x.dataset.obs === obs); if (back) back.focus(); }
      else if (k === "yes") {
        if (!settleView || settleView.state !== "ok") return;
        const a = { ...settleCtx, from: settleFrom, view: settleView };
        settleAsk = null; settleCtx = null; settleView = null; redraw();
        await settleValue(a);
      }
      else if (k === "copy") {
        try { await window.navigator.clipboard.writeText(b.dataset.text); toast("ok", "Copied - give it to an agent"); }
        catch (_) { toast("bad", "Could not copy", b.dataset.text); }
      }
    };
  });
  host.querySelectorAll("details.neither").forEach(d => {
    d.addEventListener("toggle", () => { settleNeither = d.open ? d.dataset.key : (settleNeither === d.dataset.key ? null : settleNeither); });
  });
}

// The inspector's silence rule (docs/inspector.md Sections 3-4): a field is silent when it has ONE
// value across the loaded scope, and shown otherwise. The scope is the whole /api/graph response -
// `nodes`, never the legend-filtered subset, so toggling a type chip cannot make a row appear on an
// unrelated node (D4). Uniform rather than majority: silence then states a fact about everything
// here, which the panel can say in one line (D3), instead of a proportion that flips as nodes arrive.
// The tier compared is `trust_tier`, the effective tier - never a claimed one (D4).
function scopeBaseline() {
  const tiers = new Set(), origins = new Set(), workspaces = new Set();
  for (const n of nodes) {
    tiers.add(String(n.trust_tier));
    origins.add((n.origins || []).join(", "));
    workspaces.add(n.workspace || "");   // "" = sits in more than one workspace (a cross-workspace merge)
  }
  const only = s => (s.size === 1 ? [...s][0] : null);
  return { tier: only(tiers), origin: only(origins), workspace: only(workspaces) || null };
}

// The contested-belief block of the inspector. D1 lives here, first and unconditional: a contested
// belief is never silenced or folded, whatever the silence rule decides about the fields around it.
// Only a conflict that trust already resolved (resolution.md 4.1 - informational) folds, and it folds
// to a line that still says it exists, so it stays one act away (D2, P6 keeps it queryable).
// `inspector_never_folds_a_contested_belief` pins this shape.
let foldOpen = null;   // node id whose resolved-by-trust block is expanded - survives the poll re-render
// A node's type, as the settling rows read a belief.
function nodeBelief(node) {
  return { name: node.name, field: "type", entity: node.id, current: node.type, curObs: node.kind_source,
    tier: node.trust_tier, competitors: node.competitors || [], contested: !!node.contested, ws: node.workspace || "" };
}
function contestedBlock(node) {
  if (node.contested) return html`<div class="contested hot">${contestedRows(nodeBelief(node))}</div>`;
  const cmp = node.competitors || [];
  if (!cmp.length) return html``;
  const settled = String(node.trust_tier) === "human_confirmed" ? "Human confirmed" : "Settled by trust";
  return html`${[
    html`<div class="contested folded${foldOpen === node.id ? " open" : ""}">`,
    html`<button class="cfold" type="button">${settled} - ${cmp.length} other type${cmp.length > 1 ? "s" : ""} asserted</button>`,
    html`<div class="cbody">${contestedRows(nodeBelief(node))}</div></div>`]}`;
}

// The inspector "why (evidence & decision)" body from /api/explain: per-field belief resolution
// (each single-valued field's ranked candidates - winner / alias / competitor) plus the supporting
// observation log. An explanation OF the projection - the winner IS what the graph shows.
function candRowHtml(c) {
  return html`${[
    html`<div class="cand ${c.role}"><span class="cv">${c.value}</span>`,
    html`<span class="crole">${c.role}</span>`,
    html`<span class="ctier">${tierName(c.trust_tier)}</span>`,
    html`<span class="cobs" title="${tipText("Observation " + c.observation)}">${String(c.observation).slice(0, 8)}</span></div>`]}`;
}
function fieldHtml(f) {
  return html`${[
    html`<div class="wfield${f.contested ? " hot" : ""}">`,
    html`<div class="wfhead">${f.field}${f.contested ? html` <span class="wtag">contested</span>` : ""}</div>`,
    (f.candidates || []).map(candRowHtml),
    html`</div>`]}`;
}
function renderExplain(ex) {
  // The supporting observations (the evidence) live in the inspector's "log" column now, so this
  // disclosure is just the per-field decision: which value won each single-valued field, and why.
  return html`${(ex.fields || []).map(fieldHtml)}`;
}
// Cache the fetched explanation + open state per node, so the disclosure survives the inspector's
// poll-driven re-render (renderDetail rebuilds innerHTML while a node stays focused).
let whyCache = null;   // { id, open, ex }
function fillWhy(whyBody, ex) {
  if (ex && ex.fields) {
    whyBody.innerHTML = html`${renderExplain(ex)}`;
  } else {
    whyBody.textContent = "no explanation available";
  }
}

// Relations grouped by what they say (docs/inspector.md Section 6). A hub's seventeen incoming
// edges were fifteen rows of the same relation name; grouped by (direction, kind), the name is said
// once and its members follow as chips, folded past RELATION_CHIPS behind a +N that expands in place.
const RELATION_CHIPS = 6;
let relOpen = { id: null, keys: new Set() };   // groups expanded on the focused node - survive the poll re-render
let detailTall = false;   // the panel's taller height: a viewer preference, so it carries across nodes
function relationGroups(node) {
  const byKey = new Map();
  for (const e of edges) {
    const out = e.a === node, other = out ? e.b : (e.b === node ? e.a : null);
    if (!other || typeOff.has(other.type)) continue;
    const key = (out ? "out" : "in") + "|" + e.type;
    let g = byKey.get(key);
    if (!g) byKey.set(key, g = { key, out, kind: e.type, items: [] });
    g.items.push({ other, e });
  }
  const groups = [...byKey.values()];
  // The inspector is rebuilt on every graph poll, so the order must not depend on arrival: groups by
  // size, then kind, outgoing first; members live before superseded (valid_to set - muted, never
  // dropped, P4), then by name.
  for (const g of groups)
    g.items.sort((p, q) => (!!p.e.valid_to - !!q.e.valid_to) || p.other.name.localeCompare(q.other.name));
  groups.sort((p, q) => (q.items.length - p.items.length) || p.kind.localeCompare(q.kind) || (q.out - p.out));
  return groups;
}
// A relation chip's mark: the ring the map draws for its tier, and none for Agent, which draws none
// (Section 9) - an empty swatch would leave a gap in the chip.
function chipTier(t) { return String(t) === "agent_extracted" ? "" : tierDot(t); }
function relationGroupHtml(g, open) {
  const shown = open ? g.items : g.items.slice(0, RELATION_CHIPS);
  const rest = g.items.length - shown.length;
  // A superseded edge says so in its name; an edge's description is a line under the chips, on the
  // screen rather than one hover away (Section 4, U25).
  const chip = ({ other, e }) => html`${[
    html`<button type="button" class="rchip${e.valid_to ? " past" : ""}" data-id="${other.id}" `,
    html`title="${tipText((e.valid_to ? "Superseded, focus " : "Focus ") + other.name)}">`,
    // The relation's own trust, the map's ring for its tier (Section 29.7).
    html`<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>${other.name}${chipTier(e.trust_tier)}</button>`]}`;
  return html`${[
    html`<div class="rgroup"><div class="rghead"><span class="i ${g.out ? "i-arrow-right" : "i-arrow-left"}" role="img" aria-label="${g.out ? "Outgoing" : "Incoming"}"></span>`,
    html`<span class="rkind">${g.kind}</span><span class="rct">${g.items.length}</span></div>`,
    html`<div class="rchips">${shown.map(chip)}`,
    rest > 0 ? html`<button class="rmore" type="button" data-key="${g.key}" title="${tipText("Show " + rest + " more")}">+${rest}</button>` : "",
    html`</div>`,
    shown.filter(x => x.e.description).map(x => html`<p class="rdesc"><b>${x.other.name}</b> ${x.e.description}</p>`),
    html`</div>`]}`;
}

// --- Detail inspector: shows the clicked node's connections (neighbors + relations), and click a neighbor to explore ---
// --- The person's place (docs/design-system.md Section 29.4, U20, U26) -----------------------------
// A selection is the person's: it has a history while the page is open, its id is in the address,
// and Esc gives back the camera it moved. Follow waits while the person uses the map.
let selHist = { scope: null, back: [], fwd: [], last: null };   // entity ids, this scope's
let walkCam = null;     // the camera before the walk's first selection, and when that selection came
let handAt = -Infinity; // when the person last panned, zoomed or dragged (performance.now()); never yet
function personMoved() { handAt = performance.now(); }
// What a merged-away entity became: an accepted merge naming it, else the node carrying its name as
// an alias. Null when nothing here holds it.
function mergedInto(id, name) {
  for (let hop = 0, at = id; hop < 4; hop++) {
    const p = proposals.find(q => q.kind === "entity_merge" && q.state === "merged" && q.into && q.into !== at && (q.targets || []).includes(at));
    if (!p) break;
    const n = nodeById(p.into);
    if (n) return n;
    at = p.into;
  }
  return name ? nodes.find(n => (n.aliases || []).includes(name)) || null : null;
}
// Selects a node, as every selection does: the history, the address, the sheet, then the camera.
function select(n, stepping) {
  if (!n) return;
  if (selHist.scope !== currentWs()) selHist = { scope: currentWs(), back: [], fwd: [], last: null };
  if (!focus) walkCam = { s: camT.s, x: camT.x, y: camT.y, at: performance.now() };
  if (!stepping && focus && focus !== n) { selHist.back.push(focus.id); selHist.fwd = []; }
  else if (!stepping && !focus) selHist.fwd = [];
  focus = n;
  renderDetail(n);   // the sheet first: focusView measures it (see the click handler)
  focusView(n);
  syncUrlWorkspace();
  requestFrame();
}
// Clears the selection. `giveBack`: Esc or the sheet's close, which return the camera to where the
// walk began, unless the person moved it since.
function clearSelection(giveBack) {
  if (!focus) return;
  selHist.last = focus.id;
  focus = null;
  renderDetail(null);
  if (giveBack && walkCam && handAt < walkCam.at) { camT.s = walkCam.s; camT.x = walkCam.x; camT.y = walkCam.y; }
  walkCam = null;
  syncUrlWorkspace();
  requestFrame();
}
// Back (-1) or Forward (+1) through the selections. An entry merged away is followed to what it
// became, one no longer in the scope is skipped; with nothing selected, Back reopens the last.
function stepSelection(dir) {
  if (selHist.scope !== currentWs()) return;
  const from = dir < 0 ? selHist.back : selHist.fwd, to = dir < 0 ? selHist.fwd : selHist.back;
  if (!focus && dir < 0 && selHist.last) {
    const n = nodeById(selHist.last) || mergedInto(selHist.last);
    if (n) { select(n, true); return; }
  }
  while (from.length) {
    const id = from.pop(), n = nodeById(id) || mergedInto(id);
    if (!n) continue;
    if (focus) to.push(focus.id);
    select(n, true);
    return;
  }
}
// Also named in (Section 29.16): the same name's entities in other workspaces, read once per selected
// entity on the local console and kept while the graph does not change, so a poll re-draws the line
// without reading again. Nothing shows while the read is in flight, and an answer for an entity no
// longer selected is dropped.
let namesakes = null;   // { id, tag, list } or { id, tag, why }
function alsoNamedHtml(node) {
  if (surfaceKind !== "local" || !node.workspace) return "";
  if (!namesakes || namesakes.id !== node.id || namesakes.tag !== heldGraph.tag) { readNamesakes(node); return ""; }
  if (namesakes.why) return html`<div class="meta">Not compared with other workspaces: ${namesakes.why}</div>`;
  return namesakes.list.map(m => html`<div class="meta also">Also named in ${m.workspace}: ${m.name} (${m.type}) <button type="button" class="linkish nsshow" data-id="${m.id}" aria-label="${"Show " + m.name + " in " + m.workspace}">Show</button></div>`);
}
let namesakesFor = null;
async function readNamesakes(node) {
  if (namesakesFor === node.id) return;
  namesakesFor = node.id;
  const id = node.id, tag = heldGraph.tag;
  let answer;
  if (!wsListed) answer = { id, tag, why: "the workspace list did not load" };
  else {
    try {
      const q = "?entity=" + encodeURIComponent(id) + wsListed.map(w => "&in=" + encodeURIComponent(w)).join("");
      const r = await fetch("/api/namesakes" + q, { cache: "no-store" });
      const j = await r.json();
      answer = r.ok ? { id, tag, list: j.namesakes || [] } : { id, tag, why: j.error || "the server answered " + r.status };
    } catch (e) { answer = { id, tag, why: "the server is not answering" }; }
  }
  namesakesFor = null;
  if (!focus || focus.id !== id) return;   // the selection moved on
  namesakes = answer;
  if (answer.why || answer.list.length) renderDetail(focus);
}
// Show selects the namesake: in place under All, otherwise by changing the scope to All, which starts
// the selection history afresh, as any change of scope does (Section 29.4).
function showNamesake(id) {
  const cur = currentWs();
  if (cur === "*" || cur === "all") { const n = nodeById(id); if (n) select(n); return; }
  clearSelection();
  linkedEntity = id;
  wsInput.value = "*"; beginWorkspaceTransition(); renderChipsActive(); syncUrlWorkspace(); poll();
}
// The selected entity that the address named when the page opened, selected once the graph holds it.
let linkedEntity = new URLSearchParams(location.search).get("entity");
function selectLinked() {
  if (!linkedEntity || !nodes.length) return;
  const n = nodeById(linkedEntity) || mergedInto(linkedEntity);
  linkedEntity = null;
  if (n) select(n);
  else toast("info", "The linked entity is not in " + statsScope(currentWs()));
}
// A selected entity the graph no longer holds: followed to what a merge made of it, or said gone.
function keepSelection() {
  const was = focus, again = nodeById(was.id);
  if (again) { focus = again; renderDetail(again); return; }
  const now = mergedInto(was.id, was.name);
  if (now) {
    focus = now; renderDetail(now); syncUrlWorkspace();
    toast("info", was.name + " is now " + now.name);
  } else {
    selHist.last = null;
    focus = null; renderDetail(null); syncUrlWorkspace();
    toast("info", was.name + " is no longer in " + statsScope(currentWs()));
  }
}
// Whether the person is using the map, which follow waits out (D6): a selection, the List, a popover,
// the search's results, a card or the Review page open, or their own move in the last 20 seconds.
function mapInUse() {
  return !!focus || listOn || !!workSet || viewPop.classList.contains("on") || !searchPop.hidden
    || document.getElementById("keysDlg").open || document.getElementById("sameDlg").open
    || obscardEl.classList.contains("on") || reviewOpen() || performance.now() - handAt < 20000;
}
// Activity follow did not frame because the map was in use: one notice, the latest said in words,
// with Show framing what it touched.
let activityNote = null;   // { el, n, ids }
function noteActivity(ev, ids) {
  // The row's sentence (Section 29.6); a sync lands without a notice, as before.
  const said = ev.kind === "sync" ? null : activitySaid(ev);
  if (!said) return;
  const was = activityNote;
  if (was && was.el && was.el.isConnected && !was.el.classList.contains("out")) {
    was.n += 1; was.ids = ids;
    was.el.querySelector(".tt").textContent = said + " (and " + (was.n - 1) + " more)";
    return;
  }
  const note = { n: 1, ids, el: null };
  note.el = toast("info", said, null, { label: "Show", note: "", run: () => {
    activityNote = null;
    const hit = note.ids.map(nodeById).filter(Boolean);
    if (hit.length > 1) fitView(hit, 130); else if (hit[0]) centerOn(hit[0]);
  } });
  activityNote = note;
}
// The Follow button says its state: on, waiting while the map is in use, or off (Section 29.4).
function renderFollow() {
  if (!followBtn) return;
  const paused = follow && mapInUse();
  followBtn.setAttribute("aria-pressed", follow ? "true" : "false");
  followBtn.classList.toggle("on", follow && !paused);
  followBtn.classList.toggle("paused", paused);
  const label = !follow ? "Follow" : paused ? "Follow paused" : "Follow";
  followBtn.setAttribute("aria-label", label);
  followBtn.title = tipText(label);
  followBtn.querySelector(".i").className = "i " + (follow ? "i-locate" : "i-locate-off");
}
// "Reset layout" (View): forget this scope's remembered positions and lay it out afresh.
function resetLayout() {
  if (layoutScope) { try { localStorage.removeItem(layoutScope); } catch (_) { /* nothing kept */ } }
  storedLayout = null; posById.clear(); spiralN = 0;
  heldGraph.url = heldGraph.tag = "";
  layoutFresh = true; firstData = true; settling = true; needFit = true; userMoved = false;
  wake(1);
  poll();
}

// What waits for the person about this entity, under the inspector's header (Section 29.7): one line
// per Needs-you item that names it, with Open in Review.
function waitsHtml(node) {
  if (surfaceKind !== "local") return html``;
  const items = needsItems().filter(it =>
    (it.kind === "proposal" && ((it.p.targets || []).includes(node.id) || it.p.into === node.id))
    || (it.kind === "contested" && it.c.id === node.id));
  return html`${items.map(it => html`<div class="waits"><span class="wt">Waits for you: ${itemTitle(it)}</span><button type="button" class="linkish" data-waits="${it.key}">Open in Review</button></div>`)}`;
}
function renderDetail(node) {
  // The inspector is a sheet on the right; the Log and the corner controls step aside (viewer.css).
  document.body.classList.toggle("sheet", !!node);
  renderStrip();
  if (!node) { detailEl.className = ""; detailEl.innerHTML = ""; renderStrip(); return; }
  // One panel at a time under 1200px: the inspector closes the docks (not remembered).
  if (narrowWindow()) { applyDock(true, false, false); applyDock(false, false, false); }
  const groups = relationGroups(node);
  if (relOpen.id !== node.id) relOpen = { id: node.id, keys: new Set() };
  const nrel = groups.reduce((n, g) => n + g.items.length, 0);
  // The silence rule (docs/inspector.md Section 5): a field the whole scope agrees on is not repeated
  // on every node - it is stated once, in `scopeSaid`, on this same panel, so a missing row never
  // reads as a missing value (D3, P5). The tier keeps its dot either way.
  const base = scopeBaseline();
  const tier = String(node.trust_tier);
  const scopeSaid = [];
  if (base.tier !== null) scopeSaid.push(tierName(base.tier));
  if (base.origin) scopeSaid.push("from " + (surfaceNode && base.origin === surfaceNode ? here() : base.origin));
  const origin = base.origin === null && node.origins && node.origins.length
    ? html`<div class="meta">from: ${node.origins.join(", ")}</div>` : "";
  // An empty workspace is an entity a merge carried across a workspace boundary - said, not left blank.
  const workspace = base.workspace === null
    ? html`<div class="meta">workspace: ${node.workspace || "more than one (merged across workspaces)"}</div>` : "";
  const also = alsoNamedHtml(node);
  // With a single candidate per field the disclosure can only answer "nobody said otherwise", and the
  // asserting observation is already a row of the evidence column - so it appears only when some
  // field has a second candidate: an alias spelling or a competing kind (Section 5).
  const disputed = (node.aliases || []).length > 0 || (node.competitors || []).length > 0;
  detailEl.innerHTML = html`${[
    html`<button class="close" title="${tipText("Close", "Esc")}" aria-label="Close"><span class="i i-x" aria-hidden="true"></span></button>`,
    // Taller / shorter: the height cap keeps the graph visible by default; this trades it for room.
    html`<button class="grow" title="${tipText(detailTall ? "Shorter panel" : "Taller panel")}" aria-label="${detailTall ? "Shorter panel" : "Taller panel"}">`,
      html`<span class="i ${detailTall ? "i-chevron-down" : "i-chevron-up"}" aria-hidden="true"></span></button>`,
    // Back and Forward through the selections, once there is somewhere to go (Section 29.4).
    selHist.back.length || selHist.fwd.length ? html`<div class="hnav">${[
      html`<button type="button" class="hback" title="${tipText("Back", "Cmd+[")}" aria-label="Back" aria-keyshortcuts="Meta+["${selHist.back.length ? "" : html` disabled`}><span class="i i-chevron-left" aria-hidden="true"></span></button>`,
      html`<button type="button" class="hfwd" title="${tipText("Forward", "Cmd+]")}" aria-label="Forward" aria-keyshortcuts="Meta+]"${selHist.fwd.length ? "" : html` disabled`}><span class="i i-chevron-right" aria-hidden="true"></span></button>`]}</div>` : "",
    html`<h2>${node.name}</h2>`,
    html`<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span>`,
      html`<span>${node.type}</span>${tierDot(tier)}${base.tier === null ? html`<span>${tierName(tier)}</span>` : ""}`,
      scopeSaid.length ? html`<span class="scopeline">All here: ${scopeSaid.join(", ")}</span>` : "",
    html`</div>`,
    node.last_said ? html`<div class="meta">Last said ${obsWhen(node.last_said)}</div>` : "",
    node.aliases && node.aliases.length ? html`<div class="meta">aliases: ${node.aliases.join(", ")}</div>` : "",
    origin, workspace, also,
    node.description ? html`<div class="desc">${node.description}</div>` : "",
    waitsHtml(node),
    contestedBlock(node),
    // Why this value won: a lazy-loaded disclosure (the per-field decision comes from /api/explain
    // only when opened - the graph poll stays light). The evidence itself is the column below.
    disputed ? html`<div class="why"><button class="whytoggle" type="button">Why this value</button><div class="whybody"></div></div>` : "",
    // Two regions: the node's relations, grouped by kind, and its observation log (the evidence
    // behind its belief), side by side.
    html`<div class="rels">`,
      html`<div class="relcol"><div class="sec">Relations <span class="ct">${nrel}</span>${
        // Keep this entity and what its drawn relations reach in view (Section 29.11).
        relatedDrawn(node).length ? html`<button type="button" class="linkish shrel">Show related</button>` : ""}${
        // Say two entities of one workspace are one (Section 29.17), on the local console.
        surfaceKind === "local" && node.workspace ? html`<button type="button" class="linkish sameas">Same as...</button>` : ""}</div>`,
        groups.length ? groups.map(g => relationGroupHtml(g, relOpen.keys.has(g.key))) : html`<div class="empty">none</div>`,
      html`</div>`,
      html`<div class="relcol"><div class="sec">Evidence</div><div class="logcol"></div></div>`,
    html`</div>`]}`;
  detailEl.className = "on" + (detailTall ? " tall" : "");
  detailEl.querySelector(".close").onclick = () => clearSelection(true);
  detailEl.querySelectorAll(".hback").forEach(b => { b.onclick = () => stepSelection(-1); });
  detailEl.querySelectorAll(".hfwd").forEach(b => { b.onclick = () => stepSelection(1); });
  detailEl.querySelectorAll("[data-waits]").forEach(b => {
    b.onclick = () => { reviewSeg = "needs"; reviewSel = b.dataset.waits; reviewFocus = true; if (reviewOpen()) renderReview(); else location.hash = "#review"; };
  });
  detailEl.querySelector(".grow").onclick = () => { detailTall = !detailTall; renderDetail(node); };
  detailEl.querySelectorAll(".nsshow").forEach(b => { b.onclick = () => showNamesake(b.dataset.id); });
  detailEl.querySelectorAll(".sameas").forEach(b => { b.onclick = () => openSameAs(node); });
  detailEl.querySelectorAll(".shrel").forEach(b => {
    b.onclick = () => keepInView([node.id, ...relatedDrawn(node).map(n => n.id)], { kind: "related", name: node.name });
  });
  detailEl.querySelectorAll(".rchip").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) select(n);
    };
  });
  detailEl.querySelectorAll(".rmore").forEach(b => {
    b.onclick = () => { relOpen.keys.add(b.dataset.key); renderDetail(node); };
  });
  wireSettle(detailEl, () => renderDetail(node));
  fillSettleWho(detailEl, node.id);
  const fold = detailEl.querySelector(".contested.folded");
  if (fold) fold.querySelector(".cfold").onclick = () => {
    foldOpen = fold.classList.toggle("open") ? node.id : null;
  };
  // The "log" column: this node's supporting observations (the evidence behind its belief), always
  // visible beside the edge columns. Cached + an expanded-row set, so it survives the poll re-render.
  const logcol = detailEl.querySelector(".logcol");
  fillNodeLog(node, logcol, logcol.previousElementSibling);
  // "Belief decision" disclosure: fetch /api/explain on first open, render the per-field decision.
  // The result and open state are cached per node so it survives the inspector's poll-driven
  // re-render (and reopening a fetched node is instant, no re-fetch).
  const why = detailEl.querySelector(".why");
  if (!why) return;
  const whyBody = why.querySelector(".whybody");
  if (whyCache && whyCache.id === node.id && whyCache.open) {
    why.classList.add("open");
    if (whyCache.ex) fillWhy(whyBody, whyCache.ex);
  }
  why.querySelector(".whytoggle").onclick = async () => {
    const open = why.classList.toggle("open");
    if (!whyCache || whyCache.id !== node.id) whyCache = { id: node.id, open, ex: null };
    else whyCache.open = open;
    if (!open) return;
    if (whyCache.ex) { fillWhy(whyBody, whyCache.ex); return; }
    whyBody.textContent = "loading...";
    try {
      const r = await fetch("/api/explain?entity=" + encodeURIComponent(node.id), { cache: "no-store" });
      const ex = await r.json();
      whyCache = { id: node.id, open: true, ex };
      fillWhy(whyBody, ex);
    } catch (e) {
      whyBody.textContent = "explain failed - is the server up?";
    }
  };
}

// Supersampling (HiDPI): scale the backing store by DPR and fix the CSS size to the viewport -> sharp.
function resize() {
  DPR = Math.min(window.devicePixelRatio || 1, 2);   // cap at 2x for performance
  canvas.width = Math.round(innerWidth * DPR);
  canvas.height = Math.round(innerHeight * DPR);
  canvas.style.width = innerWidth + "px";
  canvas.style.height = innerHeight + "px";
  glCanvas.style.width = innerWidth + "px";
  glCanvas.style.height = innerHeight + "px";
}
addEventListener("resize", resize);

