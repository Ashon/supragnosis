// --- Trust and contest on the map (docs/design-system.md Section 5, U2 and U3) -------------------
// The ring is the node's trust_tier: the highest EFFECTIVE tier among the observations supporting it,
// receiver-evaluated (resolution.md Section 3) - the trust of its best support, not of its winning
// value. When the loaded scope has one tier the map draws no ring and the legend says which, as the
// inspector's scope line does; otherwise every node shows its tier and Agent, the default, is no ring.
const TIER_NAME = { unverified: "Unverified", agent_extracted: "Agent", host_signed: "Host signed", human_confirmed: "Human confirmed" };
// What each tier means, said of a source (Section 29.2); under the Trust legend and in the ask.
const TIER_MEANS = {
  human_confirmed: "Confirmed through this app on one of your machines, by you or another program on your account; agents are told to leave it to you. No agent's claim outranks it; a later review can lower it.",
  host_signed: "Above Agent, but not confirmed through this app: a trusted machine's signed claim, or a review an agent accepted.",
  agent_extracted: "What an AI app writes, and what this app writes for you, until a review raises or lowers it.",
  unverified: "Trusted less than an agent's claim: a review lowered it, or its sender marked it so.",
};
// A tier as a person reads it: the Trust legend's name, never the identifier (Section 9, U17). A tier
// this page does not know keeps its identifier, so a new one still says something.
function tierName(t) { return TIER_NAME[String(t)] || String(t); }
let trustUniform = null;     // the one tier of the loaded scope, or null when tiers differ

function noteTrustAndContest() {
  const tiers = new Set(nodes.map(n => n.trust_tier));
  trustUniform = nodes.length && tiers.size === 1 ? [...tiers][0] : null;
  contestedCount = nodes.reduce((k, n) => k + (n.contested ? 1 : 0), 0);
  renderTrustLegend();
  renderSaidLegend();
  renderTierNote();
}
let trustMeansOpen = false;   // the disclosure survives the legend's redraw
// Said (Section 29.19): a window of time dims every entity not said in it, as a dimmed tier does.
const SAID_WINDOWS = [["any", "Any time"], ["today", "Today"], ["week", "Last 7 days"], ["visit", "Since your last visit"]];
const SAID_BEFORE = { today: "today", week: "the last 7 days", visit: "your last visit" };
let saidWin = "any";
// Where the chosen window starts, or null when it has none here (no earlier visit).
function saidSince(win = saidWin) {
  if (win === "today") { const d = new Date(); d.setHours(0, 0, 0, 0); return d.getTime(); }
  if (win === "week") return Date.now() - 7 * 24 * 3600 * 1000;
  if (win === "visit") return visitSince(currentWs() || "");
  return null;
}
function saidOut(n) {
  if (saidWin === "any") return false;
  const since = saidSince();
  return since !== null && !((n.last_said || 0) >= since);
}
function renderSaidLegend() {
  const host = document.getElementById("legendSaid");
  if (!host) return;
  host.innerHTML = "";
  const drawn = nodes.filter(n => !typeOff.has(n.type));
  for (const [win, label] of SAID_WINDOWS) {
    const since = win === "any" ? 0 : saidSince(win);
    const el = document.createElement("button");
    el.type = "button";
    el.className = "tlg" + (saidWin === win ? " on" : "");
    el.append(document.createTextNode(label));
    const n = document.createElement("span"); n.className = "n";
    if (since === null) {
      n.textContent = "No earlier visit here";
      el.setAttribute("aria-disabled", "true");
    } else n.textContent = String(drawn.filter(x => win === "any" || (x.last_said || 0) >= since).length);
    el.append(n);
    el.onclick = () => {
      if (el.getAttribute("aria-disabled") === "true") return;
      saidWin = win; renderSaidLegend(); renderStats(); requestFrame();
    };
    host.appendChild(el);
  }
  radioGroup(host, "Said");
}
// The last visit (Section 29.19): a visit ends after half an hour with the
// page unseen, and the last visit to a scope is when this browser last showed it before this visit,
// kept per node and scope so a reload in the middle of a visit keeps it.
const VISIT_GAP = 30 * 60 * 1000;
let visit = null;   // { seen, last: {scope: ms}, since: {scope: ms | null} }
function visitKey() { return surfaceNode ? "supra.visit." + surfaceNode : null; }
function loadVisit() {
  const key = visitKey();
  if (!key) return;
  let v = null;
  try { v = JSON.parse(localStorage.getItem(key) || "null"); } catch (_) { v = null; }
  const now = Date.now();
  if (!v || typeof v !== "object" || !(now - (v.seen || 0) < VISIT_GAP)) v = { seen: now, last: (v && v.last) || {}, since: {} };
  visit = v;
  visitSince(currentWs() || "");
  markSeen();
}
function visitSince(scope) {
  if (!visit) return null;
  if (!(scope in visit.since)) visit.since[scope] = typeof visit.last[scope] === "number" ? visit.last[scope] : null;
  return visit.since[scope];
}
function markSeen() {
  const key = visitKey();
  if (!visit || !key || document.hidden) return;
  const now = Date.now();
  if (now - visit.seen >= VISIT_GAP) visit.since = {};   // back after a while: a new visit
  visitSince(currentWs() || "");
  visit.seen = now;
  visit.last[currentWs() || ""] = now;
  try { localStorage.setItem(key, JSON.stringify(visit)); } catch (_) { /* private mode */ }
}
document.addEventListener("visibilitychange", () => { if (document.hidden) { if (visit) { visit.seen = Date.now(); try { localStorage.setItem(visitKey(), JSON.stringify(visit)); } catch (_) { /* private mode */ } } } else markSeen(); });
// The windows move every minute while the page is open.
setInterval(() => { markSeen(); if (saidWin !== "any") { renderSaidLegend(); renderStats(); requestFrame(); } }, 60 * 1000);
function renderTrustLegend() {
  const host = document.getElementById("legendTrust");
  if (!host) return;
  host.textContent = "";
  // Where the scope has one tier there is nothing to filter: the entries stay a key. Otherwise each
  // is a toggle with its count, and switching one off dims its entities (D5, Section 29.9).
  // An empty scope has nothing to filter either.
  const count = {};
  for (const n of nodes) count[n.trust_tier] = (count[n.trust_tier] || 0) + 1;
  const filters = !trustUniform && nodes.length > 0;
  for (const t of ["unverified", "agent_extracted", "host_signed", "human_confirmed"]) {
    const off = !!trustUniform && trustUniform !== t;
    const el = document.createElement(filters ? "button" : "span");
    const sw = document.createElement("span"); sw.className = "tsw t-" + t;
    el.append(sw, document.createTextNode(TIER_NAME[t]));
    if (!filters) {
      el.className = "tlg" + (off ? " off" : "");
      if (off) {
        const none = document.createElement("span"); none.className = "sr-only"; none.textContent = ", none here";
        el.append(none);
      }
    } else {
      el.type = "button";
      el.className = "tlg toggle" + (trustOff.has(t) ? " dim" : "");
      // A tier the scope does not hold is disabled and says so where its count would be.
      const n = document.createElement("span"); n.className = "n"; n.textContent = count[t] ? String(count[t]) : "none here";
      el.append(n);
      if (!count[t]) el.disabled = true;
      else {
        setPressed(el, !trustOff.has(t));
        if (trustOff.has(t)) el.appendChild(icon("eye-off"));
        el.onclick = () => {
          if (trustOff.has(t)) trustOff.delete(t); else trustOff.add(t);
          renderTrustLegend(); renderStats(); requestFrame();
        };
      }
    }
    host.appendChild(el);
  }
  const note = document.createElement("div");
  note.className = "tnote";
  note.textContent = trustUniform
    ? "All here: " + TIER_NAME[trustUniform] + ", so no entity is ringed."
    : "A ring is the strongest source behind an entity. Agent claims, the usual case, draw no ring.";
  host.appendChild(note);
  // What each tier means, where trust is decided (Section 29.2) - a tooltip cannot carry it (U25).
  const means = document.createElement("details");
  means.className = "tmeans";
  means.open = trustMeansOpen;
  const sum = document.createElement("summary");
  sum.append(icon("chevron-right"), document.createTextNode("What these mean"));
  means.appendChild(sum);
  for (const t of ["human_confirmed", "host_signed", "agent_extracted", "unverified"]) {
    const line = document.createElement("div");
    line.className = "tmean";
    const b = document.createElement("b");
    b.textContent = TIER_NAME[t];
    line.append(b, document.createTextNode(" " + TIER_MEANS[t]));
    means.appendChild(line);
  }
  means.addEventListener("toggle", () => { trustMeansOpen = means.open; });
  host.appendChild(means);
}
// With the left panel closed the Trust legend is out of sight, so the status bar says what it would
// when the map draws no ring: the scope's one tier (U2).
function renderTierNote() {
  const el = document.getElementById("tierNote");
  if (!el) return;
  const show = !!trustUniform && !dockLEl.classList.contains("on");
  el.hidden = !show;
  el.textContent = show ? "All here: " + TIER_NAME[trustUniform] : "";
}
function trustRing(n, r) {
  const t = trustUniform ? null : n.trust_tier;
  if (!t || t === "agent_extracted") return;
  bulk.beginPath(); bulk.arc(n.x, n.y, r + 3.5, 0, 7);
  // Told by weight, not by dash (Section 5): the more support, the heavier the ring.
  if (t === "unverified") { bulk.lineWidth = 1/cam.s; bulk.strokeStyle = OTHER; }
  else if (t === "host_signed") { bulk.lineWidth = 1.6/cam.s; bulk.strokeStyle = INK; }
  else if (FORCED_COLORS.matches) { bulk.lineWidth = 4.8/cam.s; bulk.strokeStyle = INK; }
  else { bulk.lineWidth = 2.4/cam.s; bulk.strokeStyle = EDGE_HI; }
  bulk.stroke();
}

// The graph and the hypergraph the page holds, by their address and ETag (design-system.md Section
// 27.3, U33). A poll asks with the tag, and an unchanged answer, a 304, leaves the map as it is:
// nothing is parsed or applied. Only the tag of what the page holds is kept, so another workspace's
// address is never asked with it.
const heldGraph = { url: "", tag: "" }, heldHyper = { url: "", tag: "" };
function askedWith(held, url) {
  return { cache: "no-store", headers: held.url === url && held.tag ? { "If-None-Match": held.tag } : {} };
}
function hold(held, url, r) { held.url = url; held.tag = r.headers.get("ETag") || ""; }
async function poll() {
  const ws = wsInput.value.trim();
  const url = "/api/graph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
  try {
    const r = await fetch(url, askedWith(heldGraph, url));
    if (r.status === 304) heardBack();
    else {
      if (!r.ok) { sayLate("the server answered HTTP " + r.status); return; }
      const g = await r.json();
      if (g.error) { sayLate(g.error); return; }
      await commonTypesRead;   // answered once, beside the first graph read (Section 29.9)
      applyGraph(g);
      hold(heldGraph, url, r);
    }
    // Hyperedges (second-order structure) are fetched whenever the hull force or overlay needs them
    // (the force is on by default, suppressed only in group mode); otherwise cleared. As an auxiliary
    // channel, a failure still keeps the graph rendering (Principle 21: observability is optional).
    if (hyperMode || (hullForce && !clusterMode)) {
      try {
        const hurl = "/api/hypergraph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
        const hr = await fetch(hurl, askedWith(heldHyper, hurl));
        if (hr.status !== 304 && hr.ok) {
          const hg = await hr.json();
          if (!hg.error) {
            hyperedges = hg.hyperedges || [];
            showStats(currentWs());
            hold(heldHyper, hurl, hr);
            layoutGraph();
          }
        }
      } catch (e) { /* hull is auxiliary - the graph stays as-is */ }
    } else { hyperedges = []; heldHyper.url = heldHyper.tag = ""; }
    // Keep the type glossary + curation + proposals + log panels current (no-op while closed). The
    // log refreshes here too, so an observe event (which routes through poll) also refreshes it.
    refreshGlossary();
    refreshCuration();
    refreshProposals();
    refreshLog();
    refreshStrip();
  } catch (e) { sayLate("the server is not answering"); }
}

function currentWs() { return wsInput.value.trim(); }
// Clean workspace transition: reset per-workspace view state, raise the loader immediately (no
// flash of the old layout under a stale camera), and treat the new graph like a fresh load - the
// reveal ends with an auto-fit, so switching workspaces always lands framed and zoomed sensibly.
// Reflect the selected workspace in the URL (?workspace=...) - shareable, bookmarkable, and it
// survives a reload. Empty (the node default) keeps the URL clean; "*" (all) is kept as-is.
// The address holds the place: the scope and the selected entity (Section 29.4). A page opened on an
// entity keeps that id until the graph that holds it arrives.
function syncUrlWorkspace() {
  const ws = wsInput.value.trim();
  const url = new URL(location.href);
  if (ws) url.searchParams.set("workspace", ws);
  else url.searchParams.delete("workspace");
  const entity = focus ? focus.id : linkedEntity;
  if (entity) url.searchParams.set("entity", entity);
  else url.searchParams.delete("entity");
  history.replaceState(null, "", url);
}

function beginWorkspaceTransition() {
  workSet = null;        // a working set belongs to the scope it was made in (Section 29.11)
  saveLayout();          // the scope being left, under its own key
  layoutFresh = true;    // the next graph reads the new scope's
  heldGraph.url = heldGraph.tag = heldHyper.url = heldHyper.tag = "";   // the next poll fetches in full
  heldLog.url = heldLog.tag = ""; logLimit = LOG_ROWS;
  syncUrlWorkspace();
  focus = null; hover = null; renderDetail(null);
  proposalSel = null;
  if (reviewPreview) endPreview();
  pulses.clear();
  settling = true;
  needFit = true;
  userMoved = false;
  wake(1);
}

// The scope in use is always a selected chip (design-system.md Section 6). The node default is
// addressed as "" and named by its nodes, so its chip is the one its nodes' workspace names, or a
// "default" chip while it holds nothing.
function chipIsCurrent(ws) {
  const cur = currentWs();
  if (ws === cur) return true;
  return cur === "" && ((!!defaultWs && ws === defaultWs) || (nodes.length > 0 && nodes[0].workspace === ws));
}
function renderChipsActive() {
  chipBar.querySelectorAll(".chip").forEach(c => c.classList.toggle("on", chipIsCurrent(c.dataset.ws)));
  radioGroup(chipBar, "Workspace");
  syncSearchScope();
}
// The search field's placeholder tells the person what scope the query will run in.
function syncSearchScope() {
  const cur = currentWs();
  let ph;
  if (cur === "*" || cur === "all") ph = "Search all workspaces";
  else if (cur === "") ph = "Search " + defaultName();
  else ph = "Search " + cur;
  searchEl.placeholder = ph;
  // The placeholder goes when the person types, so the field's name says the scope too.
  searchEl.setAttribute("aria-label", ph);
}
async function loadWorkspaces() {
  try {
    const r = await fetch("/api/workspaces", { cache: "no-store" });
    if (!r.ok) return;
    const list = await r.json();
    wsListed = list;
    const cur = currentWs();
    const mk = (label, val, cls) => {
      const c = document.createElement("button");
      c.type = "button";
      c.className = "chip" + (chipIsCurrent(val) ? " on" : "") + (cls ? " " + cls : "");
      c.dataset.ws = val; c.textContent = label;
      c.onclick = () => {
        openedAll = false;
        if (wsInput.value.trim() === val) return;
        wsInput.value = val; beginWorkspaceTransition(); renderChipsActive(); poll();
      };
      return c;
    };
    // A panel heading like the ones below it (docs/design-system.md Section 4).
    const lbl = document.createElement("div"); lbl.className = "rsec wshead"; lbl.textContent = "Workspace";
    const named = list.some(w => chipIsCurrent(w));
    // A workspace the address names and the list does not still shows its chip, selected and muted
    // (Section 29.9): the page is in that scope, and nothing is filed under it.
    const unlisted = wsUnlisted(cur) ? [mk(cur, cur, "unlisted")] : [];
    const chips = [mk("All", "*"), ...(cur === "" && !named ? [mk(defaultName(), "")] : []), ...list.map(w => mk(w, w)), ...unlisted];
    const note = [];
    if (openedAll && (cur === "*" || cur === "all")) {
      const n = document.createElement("div"); n.className = "wsnote";
      n.textContent = "Showing all workspaces - " + defaultName() + (openedHeld ? " holds " + openedHeld.d + " of " + openedHeld.t : " is empty");
      note.push(n);
    }
    chipBar.replaceChildren(lbl, ...chips, ...note);
    radioGroup(chipBar, "Workspace");   // one scope at a time (Section 4)
  } catch (e) { /* server not up - retry next cycle */ }
}

// An empty workspace says it is empty, not that knowledge is missing (docs/onboarding.md Section 7):
// what fills it, and the workspaces that hold knowledge, each with its count, one click away. The
// counts are read across every workspace at most every 30 seconds, and only while this shows.
let wsCounts = null, wsCountsAt = 0;
let wsListed = null;     // the workspaces the node lists, as /api/workspaces last said
function wsUnlisted(cur) { return !!(wsListed && cur && cur !== "*" && cur !== "all" && !wsListed.includes(cur)); }
let firstView = new URLSearchParams(location.search).get("workspace") === null;   // the address named no scope
let openedAll = false;   // the page opened on All because the default was empty - said beside the chips
let openedHeld = null;   // { d, t } when it opened on All because the default held at most half
// The first view is where the knowledge is (Section 29.21): when the address names no scope, the page
// decides once, before its first draw, from the node's entities by workspace - All when other
// workspaces hold entities and the default holds at most half of them. Unread counts keep the default.
async function decideFirstView() {
  if (!firstView || currentWs() !== "") return;
  try {
    const r = await fetch("/api/graph?workspace=*", { cache: "no-store" });
    if (!r.ok) return;
    const nodesAll = (await r.json()).nodes || [];
    const t = nodesAll.length, d = nodesAll.filter(n => n.workspace === defaultName()).length;
    if (t > d && d * 2 <= t) {
      firstView = false; openedAll = true; openedHeld = d ? { d, t } : null;
      wsInput.value = "*"; renderChipsActive();
    }
  } catch (_) { /* unread counts keep the default */ }
}
async function anyEntry(ws) {
  const r = await fetch("/api/observations?limit=1" + (ws ? "&workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
  if (!r.ok) throw new Error("status " + r.status);
  const l = await r.json();
  return Array.isArray(l) && l.length > 0;
}
async function refreshEmpty() {
  const scopeNow = currentWs();
  if (entriesOf !== scopeNow) wsCountsAt = 0;   // a new scope asks again at once
  if (Date.now() - wsCountsAt > 30000) {
    wsCountsAt = Date.now();
    try {
      const r = await fetch("/api/graph?workspace=*", { cache: "no-store" });
      if (r.ok) {
        const counts = {};
        for (const n of (await r.json()).nodes || []) counts[n.workspace] = (counts[n.workspace] || 0) + 1;
        wsCounts = counts;
      }
    } catch (_) { /* the daemon is not answering - the sentence stands without the list */ }
    try {
      entriesOf = scopeNow;
      scopeEntries = await anyEntry(scopeNow);
      storeEntries = scopeEntries || await anyEntry("*");
    } catch (_) { scopeEntries = false; storeEntries = true; }   // unknown is not empty (P5): no Open Setup
    renderStats();
  }
  const cur = currentWs();
  const others = Object.entries(wsCounts || {}).filter(([w]) => w !== cur).sort((a, b) => b[1] - a[1]);
  // The first view is where the knowledge is (design-system.md Section 6): when the address named no
  // scope and the node default holds nothing while other workspaces do, the page opens on All and
  // says so beside the chips. An empty default is the wrong scope, not an absence of knowledge (P5).
  if (firstView && cur === "" && others.length) {
    firstView = false; openedAll = true;
    wsInput.value = "*"; beginWorkspaceTransition(); renderChipsActive(); loadWorkspaces(); poll();
    return;
  }
  const line = (text, cls) => { const d = document.createElement("div"); d.className = cls; d.textContent = text; return d; };
  // A name the node does not list is said as such (Section 29.9).
  const unknown = wsUnlisted(cur);
  const kids = scopeEntries
    // Entries that name no entity are not an empty scope (P5): the Log holds them.
    ? [line("Nothing on the map yet.", "e-title"), line("The entries here name no entity - the Log holds them.", "e-sub")]
    : [...(unknown ? [line("No workspace is called " + cur + ".", "e-title")] : []),
      line("Nothing here yet.", unknown ? "e-sub" : "e-title"),
      line("Ask a connected AI app to remember something" + (others.length ? ", or pick a workspace that holds knowledge:" : "."), "e-sub")];
  // The camera's idle buttons are described by the map's first sentence.
  kids[0].id = "emptyTitle";
  if (others.length) {
    const row = document.createElement("div");
    row.className = "e-ws";
    for (const [w, c] of others.slice(0, 8)) {
      const b = document.createElement("button");
      b.type = "button"; b.className = "chip"; b.textContent = w + " (" + c + ")";
      b.onclick = () => { wsInput.value = w; beginWorkspaceTransition(); renderChipsActive(); poll(); };
      row.append(b);
    }
    kids.push(row);
  }
  // In the app, a store with no entry leads back to Setup, which says what
  // to ask (Section 29.10). The shell gives its address; a browser has none and shows no button.
  if (window.supragnosisSetup && !storeEntries) {
    const b = document.createElement("button");
    b.type = "button"; b.className = "chip"; b.textContent = "Open Setup";
    b.onclick = () => { location.href = window.supragnosisSetup + "?from=" + encodeURIComponent(String(location.href).split("#")[0]); };
    const row = document.createElement("div"); row.className = "e-ws"; row.append(b);
    kids.push(row);
  }
  emptyEl.replaceChildren(...kids);
}

