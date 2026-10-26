// --- Remembered layout (docs/design-system.md Section 5) ------------------------------------------
// The console keeps each workspace's node positions in this browser, keyed by the node's host id from
// /api/surface and the workspace, and the next load starts from them: a workspace seen before appears
// at once instead of behind the settling loader. Until /api/surface names the node nothing is kept. A
// position is a convenience; losing one costs a settling.
let surfaceNode = null;     // this node's host id, from /api/surface; null until it answers
// The page's own machine, in words (design-system.md Section 9, D2): "this machine", or the word the
// app's shell gives - "this Mac" - since the viewer stays browser-neutral.
function here() { return window.supragnosisHere || "this machine"; }
function hereTitle() { const w = here(); return w.charAt(0).toUpperCase() + w.slice(1); }
let layoutScope = null;     // the storage key the current positions belong to
let storedLayout = null;    // {id: [x, y]} read for the scope being loaded
let layoutFresh = true;     // the next graph is the first of its scope
let layoutDirty = false;    // positions moved since the last save
// One read of the surface for the whole start, however many chains wait on it; only a page whose
// address named no scope asks where the first view opens, which costs the server a pass over the store.
let surfaceRead = null;
function loadSurface() {
  if (!surfaceRead) surfaceRead = (async () => {
    try {
      const url = "/api/surface" + (firstView ? "?first_view=1" : "");
      const s = await (await fetch(url, { cache: "no-store" })).json();
      if (s && s.node) surfaceNode = String(s.node);
      if (s && s.default_workspace) defaultWs = String(s.default_workspace);
      if (s && s.first_view) surfaceFirstView = s.first_view;
    } catch (_) { /* no surface answer - nothing is remembered */ }
    loadVisit();
  })();
  return surfaceRead;
}
function loadLayout() {
  storedLayout = null;
  // A scope of several workspaces is laid out with their cohesion since 0.6.4 (the layout); a layout
  // remembered before it is forgotten once, or the warm start would keep the scattered picture.
  const ws = currentWs() || "", many = ws === "*" || ws.includes(",");
  layoutScope = surfaceNode ? "supra.layout:" + surfaceNode + ":" + ws + (many ? ":ws2" : "") : null;
  if (!layoutScope) return;
  try { storedLayout = JSON.parse(localStorage.getItem(layoutScope) || "null"); } catch (_) { storedLayout = null; }
}
function saveLayout() {
  if (!layoutScope || !nodes.length) return;
  const at = {};
  for (const n of nodes) at[n.id] = [Math.round(n.x * 10) / 10, Math.round(n.y * 10) / 10];
  try { localStorage.setItem(layoutScope, JSON.stringify(at)); } catch (_) { /* full or private - nothing kept */ }
}
addEventListener("pagehide", saveLayout);

// Status bar stats line: one plain sentence, scope first. `regions` is a count or null.
function statsScope(cur) {
  return cur === "*" || cur === "all" ? "All workspaces" : cur || defaultName();
}
// The scope addressed as "" by its name: the workspace an omitted one resolves to, the node's default
// as /api/surface says, and "default" until it has said (design-system.md Section 29.10).
let defaultWs = null;
function defaultName() { return defaultWs || "default"; }
// What the map draws, counted from the filters alone - never from the zoom, the Relations switch or
// Regions' mode (design-system.md Section 29.9): entities whose type is shown, relations whose ends
// and type are shown, regions with three or more shown members.
function drawnCounts() {
  const shown = new Set();
  for (const n of nodes) if (!typeOff.has(n.type)) shown.add(n.id);
  let rel = 0;
  for (const e of edges) if (shown.has(e.a.id) && shown.has(e.b.id) && !edgeTypeOff.has(e.type)) rel++;
  let reg = null;
  if (hyperedges.length) {
    reg = 0;
    for (const h of hyperedges) {
      let k = 0;
      for (const m of h.members) if (shown.has(m) && ++k >= 3) break;
      if (k >= 3) reg++;
    }
  }
  const present = new Set(nodes.map(n => n.type));
  const hiddenTypes = [...typeOff].filter(t => present.has(t)).length;
  const tiers = new Set(nodes.map(n => n.trust_tier));
  const dimmed = [...trustOff].filter(t => tiers.has(t));
  const hiddenKinds = [...edgeTypeOff].filter(t => edges.some(e => e.type === t)).length;
  return { shown: shown.size, all: nodes.length, rel, reg, hiddenTypes, hiddenKinds, dimmed };
}
function statsLine(cur, c) {
  const scope = statsScope(cur);
  let e = c.shown === c.all ? (c.all === 1 ? "1 entity" : c.all + " entities") : c.shown + " of " + c.all + " entities";
  if (c.hiddenTypes) e += " (" + (c.hiddenTypes === 1 ? "1 type" : c.hiddenTypes + " types") + " hidden)";
  const r = c.rel === 1 ? "1 relation" : c.rel + " relations";
  let line = scope + " - " + e + ", " + r;
  if (c.reg !== null) line += ", " + (c.reg === 1 ? "1 region" : c.reg + " regions");
  if (c.dimmed.length) line += ", " + c.dimmed.map(tierName).join(" and ") + " dimmed";
  if (saidWin !== "any" && saidSince() !== null) line += ", said before " + SAID_BEFORE[saidWin] + " dimmed";
  // A pinned working set is said after the counts, as a dimmed tier is (Section 29.11).
  if (workSet) line += ", " + workSetSaid();
  return line;
}
// The line drawn: the counts in their own span, which a phone gives up before the scope's name.
// The counts drawn, said again wherever they show (Section 29.9).
function renderStats() { showStats(currentWs()); }
// The working set's members the map draws, and its words for the status line, List and the canvas.
function keptDrawn() {
  return workSet ? nodes.filter(n => !typeOff.has(n.type) && workSet.ids.has(n.id)) : [];
}
function partSaid(p) { return p.kind === "search" ? p.n + " for \"" + p.q + "\"" : p.n + " related to " + p.name; }
function workSetSaid() {
  if (!workSet) return "";
  const k = keptDrawn().length, [one] = workSet.parts;
  if (workSet.parts.length === 1 && one.kind === "search")
    return k + " kept for \"" + one.q + "\"" + (one.first ? " (the search stopped at " + SEARCH_LIMIT + ")" : "");
  return k + " kept - " + workSet.parts.map(partSaid).join(", ");
}
// The pill a narrow status bar keeps, and Fit's name, follow the set (Section 29.11).
function renderKept() {
  const pill = document.getElementById("kept"), fit = document.getElementById("fit");
  if (pill) {
    pill.hidden = !workSet;
    if (workSet) pill.querySelector(".kn").textContent = keptDrawn().length + " kept";
  }
  if (fit) {
    const name = workSet ? "Fit to kept entities" : "Fit to view";
    fit.title = tipText(name, typeof singleKeys === "undefined" || singleKeys ? "0" : null); fit.setAttribute("aria-label", name);
  }
}
// A member merged away is followed to what it became, as a selection is (Section 29.4).
function followKept() {
  if (!workSet) return;
  for (const id of [...workSet.ids]) {
    if (nodeById(id)) continue;
    const now = mergedInto(id);
    if (now) { workSet.ids.delete(id); workSet.ids.add(now.id); }
  }
}
// The entities a node's drawn relations reach: a relation whose kind is shown, between two drawn ends.
function relatedDrawn(node) {
  const out = new Map();
  for (const e of edges) {
    if (edgeTypeOff.has(e.type) || typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
    if (e.a.id === node.id && e.b.id !== node.id) out.set(e.b.id, e.b);
    else if (e.b.id === node.id && e.a.id !== node.id) out.set(e.a.id, e.a);
  }
  return [...out.values()];
}
// Adds entities to the working set, pinning one when none is; the part says where they came from.
function keepInView(ids, part) {
  if (!workSet) workSet = { ids: new Set(), parts: [] };
  let added = 0;
  for (const id of ids) if (!workSet.ids.has(id)) { workSet.ids.add(id); added++; }
  // A part that added nothing is not said: the set already held all of it.
  if (added) workSet.parts.push(Object.assign(part, { n: added }));
  renderStats(); renderList(); requestFrame();
}
function clearWorkSet() {
  if (!workSet) return;
  workSet = null;
  renderStats(); renderList(); requestFrame();
}
// The map's key (Section 29.9): each mark the current view draws, its mark beside it, and only those.
function renderMarks() {
  const host = document.getElementById("legendMarks"), head = document.getElementById("marksHead");
  if (!host) return;
  const rows = [];
  const row = (cls, text) => {
    const r = document.createElement("div"); r.className = "mrow";
    const m = document.createElement("span"); m.className = "mk " + cls; m.setAttribute("aria-hidden", "true");
    r.append(m, document.createTextNode(text));
    rows.push(r);
  };
  if (nodes.length) {
    if (contestedCount) row("mk-badge", "Two sources disagree - select to decide");
    row("mk-size", "Larger: more relations");
    if (hyperMode && allRegions && drawnCounts().reg) row("mk-cloud", "Soft cloud: a region - entities said together");
    // Only while two or more outlines are drawn (Section 29.12).
    if (boundaryMode === "workspace" && outlines("workspace")) row("mk-outline", "Outline: a workspace");
    else if (boundaryMode === "origin" && outlines("origin")) row("mk-outline", "Outline: the machine that signed");
    if (showFootprint && footprint.size) row("mk-glow", "Glow: what agents used");
    if (pulsedAny && showPulses && !REDUCED_MOTION) row("mk-pulse", "Ring that grows and fades: just touched");
  }
  host.replaceChildren(...rows);
  if (head) head.hidden = !rows.length;
}
function showStats(cur) {
  const c = drawnCounts();
  const scope = statsScope(cur), line = statsLine(cur, c);
  const head = document.createElement("span"), counts = document.createElement("span");
  head.textContent = scope;
  counts.className = "counts";
  counts.textContent = line.slice(scope.length);
  document.getElementById("stats").replaceChildren(head, counts);
  fitStats();
  renderPlace();
  // The canvas says what it draws and where else it is (Section 6, U24).
  // With nothing drawn it says the scope holds nothing, and no List (Section 29.10); with a working
  // set, that List holds the kept ones (Section 29.11).
  renderKept();
  canvas.setAttribute("aria-label", c.all ? "Map of " + line.slice(scope.length + 3) + (workSet ? ". List shows the kept ones as a table." : ". List shows them as a table.")
    : "Map of " + scope + (scopeEntries ? ": nothing on the map yet; the Log holds its entries." : ": nothing here yet."));
  // While anything is hidden or dimmed the status bar says so and offers Show all, which clears both.
  const all = document.getElementById("showAll");
  if (all) all.hidden = !(c.hiddenTypes || c.hiddenKinds || c.dimmed.length || workSet);
  const lh = listWin && listWin.querySelector(".lhead");
  if (lh) lh.textContent = line;
  renderMarks();
}
function showAll() {
  typeOff.clear(); edgeTypeOff.clear(); trustOff.clear(); workSet = null;
  renderLegend(); renderTrustLegend(); renderSaidLegend(); renderStats(); requestFrame();
}
// A line that does not fit gives up its counts before it cuts the scope's name (Section 6).
function fitStats() {
  const el = document.getElementById("stats");
  el.classList.remove("tight");
  el.classList.toggle("tight", el.scrollWidth > el.clientWidth);
}
addEventListener("resize", fitStats);
// Status bar: only say when the graph is stale, and only then say since when.
function sayLate(reason) {
  // The reason is poll's: the HTTP status, the server's own error, or that it did not answer.
  const line = lastOkAt
    ? "Not updated since " + lastOkAt.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) + " - " + reason
    : "Not loaded - " + reason;
  // A Retry that failed keeps the poll's reason and says when it tried (P24, P5).
  statusEl.textContent = retryAsked ? line + " (tried " + retryAsked + ")" : line;
  // Retry follows the line, as its own button, so the line's rewrite never takes its focus.
  const r = document.getElementById("retry");
  if (r) r.hidden = false;
}

// The graph is current: a poll answered, with a graph or with "unchanged" (U33).
function heardBack() {
  lastOkAt = new Date();
  statusEl.textContent = "";
  retryAsked = null;
  { const r = document.getElementById("retry"); if (r) r.hidden = true; }
}

function applyGraph(g) {
  graphGen++;
  const seen = new Set();
  let added = false, restored = 0;
  if (layoutFresh) loadLayout();
  nodes = g.nodes.map(n => {
    let p = posById.get(n.id);
    if (!p) {
      added = true;
      const at = storedLayout && storedLayout[n.id];
      if (Array.isArray(at) && at.length === 2 && at.every(Number.isFinite)) {
        p = { x: at[0], y: at[1], vx: 0, vy: 0 };
        restored++;
      } else {
        const i = spiralN++, a = i * 2.39996, r = 60 + 30 * Math.sqrt(i);
        p = { x: innerWidth/2 + r*Math.cos(a), y: innerHeight/2 + r*Math.sin(a), vx:0, vy:0 };
      }
      posById.set(n.id, p);
    }
    seen.add(n.id);
    // Reset the fields serde OMITS when false/empty (contested, competitors, aliases, origins,
    // description, kind_source) before layering the fresh node on. `p` is reused across polls to keep
    // object identity (focus / edge endpoints), so a value set in a PRIOR poll would otherwise persist
    // even after the belief changed - e.g. a confirmed belief keeping its contested ring/wording.
    // Position/velocity live on `p` (not in these defaults, not in `n`) and are preserved.
    return Object.assign(p, { description: null, aliases: [], origins: [], contested: false, competitors: [], kind_source: null }, n);
  });
  let removed = 0;
  for (const id of [...posById.keys()]) if (!seen.has(id)) { posById.delete(id); removed++; }
  // First data = full settle behind the loader (the initial layout is violent). Later add/remove
  // (an agent observing, a peer pushing) warms the sim BELOW the loader threshold so the graph stays
  // on screen and new nodes ease in - hiding the whole graph on every incremental hit was wrong, and it
  // hid the very peer-marker tour that a push triggers. A scope whose layout was remembered (most of
  // its nodes restored) skips the settle: the known picture shows at once and the rest eases in.
  const warm = layoutFresh && nodes.length > 0 && restored >= 0.8 * nodes.length;
  if (layoutFresh && nodes.length) layoutFresh = false;
  if (warm) {
    // Positions saved under another window size have another centre, and the sim's anchor pulls the
    // centroid to this window's world centre every frame - so move the picture there first, or the
    // frame below would chase it.
    let cx = 0, cy = 0;
    for (const n of nodes) { cx += n.x; cy += n.y; }
    const dx = innerWidth / 2 - cx / nodes.length, dy = innerHeight / 2 - cy / nodes.length;
    for (const n of nodes) { n.x += dx; n.y += dy; }
    alpha = Math.min(alpha, REVEAL_ALPHA);
    layout.cool(REVEAL_ALPHA);
    // Frame the remembered picture now: the reveal that fits a fresh layout may already have passed
    // while the page waited for its first graph.
    if (!userMoved) { fitView(); cam.s = camT.s; cam.x = camT.x; cam.y = camT.y; needFit = false; }
  } else if (added || removed) wake(firstData ? 0.7 : 0.4);

  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  edges = g.edges.map(e => Object.assign({}, e, { a: byId[e.from], b: byId[e.to] }))
                 .filter(e => e.a && e.b);
  // Bridge nodes: nodes connected to another type (group) - linking/navigation points that join groups.
  bridgeSet.clear();
  for (const e of edges) if (e.a.type !== e.b.type) { bridgeSet.add(e.a.id); bridgeSet.add(e.b.id); }
  assignColors();
  layoutGraph(warm);   // after the colors: group mode's targets are by type
  positionsGen++;
  renderLegend();
  noteTrustAndContest();
  followKept();
  const s = g.stats || {};
  lastStats = s;
  heardBack();
  // Current workspace leads the stats line - the selection UI is the left rail's chips, and this
  // keeps the selection readable even with that dock collapsed.
  // The regions the last hypergraph read counted stay in the line until the next one answers, so the
  // count does not blink out at every poll.
  showStats(currentWs());
  emptyEl.style.display = nodes.length ? "none" : "flex";
  idleWhileEmpty();
  if (!nodes.length) refreshEmpty();

  // If focused, refresh the detail inspector (reflect connection changes). A focus the graph no longer
  // holds is followed to what a merge made of it, or said gone (U20, Section 29.4).
  if (focus) { if (nodes.includes(focus)) renderDetail(focus); else keepSelection(); }
  selectLinked();

  // Initial auto-fit: once after the layout settles (cooling done), and only before user interaction (in draw).
  if (firstData && nodes.length) { firstData = false; needFit = !warm; }
  if (nodes.length) firstView = false;   // the scope the page opened on holds knowledge
  // wake() only fires when the node set changed; a tier, kind or contested flag that moved
  // repaints without moving anything, so it has to ask for the frame itself.
  requestFrame();
}

// Federation panel: hubs (health + per-workspace diff vs this node) and, on a hub, the known-peer
// registry (who checked in, what they did, how long ago). Data = /api/federation (wiring-layer blob).
// The status bar's sync pill (docs/design-system.md Section 6, U8): one line for what sync is doing,
// opening Settings > Sync. It replaces the dock's Peers tab, whose rows now live in that section (and
// in the viewer's own settings dialog outside the app). Absent only where sync is not set up;
// unavailable and not-yet-answered are states it draws.
async function refreshSyncPill() {
  const pill = document.getElementById("syncPill");
  if (!pill) return;
  let f = null;
  try {
    const r = await fetch("/api/federation", { cache: "no-store" });
    if (r.ok) f = await r.json();
  } catch (_) { f = null; }
  let text, tone;
  if (f && f.node_id) ownNode = String(f.node_id);
  if (!f) { text = "Sync unavailable"; tone = "bad"; }
  else if (f.configured === false) { pill.hidden = true; return; }
  else {
    const hubs = f.servers || [];
    const down = hubs.filter(s => !s.healthy).length;
    const unknown = hubs.filter(s => !s.negotiated_at).length;
    let mine = 0, theirs = 0;
    for (const s of hubs) for (const w of (s.workspaces || [])) { mine += w.local_ahead | 0; theirs += w.hub_ahead | 0; }
    if (down) { text = down === hubs.length ? "Hub unreachable" : `${down} of ${hubs.length} hubs unreachable`; tone = "bad"; }
    else if (unknown) { text = "Sync: no answer yet"; tone = "warn"; }
    else if (mine || theirs) { text = `Syncing: ${here()} +${mine}, hub +${theirs}`; tone = "warn"; }
    else if (hubs.length) { text = "In sync"; tone = "ok"; }
    else { const k = (f.known_peers || []).length; text = `Hub: ${k} peer${k === 1 ? "" : "s"}`; tone = "ok"; }
  }
  pill.hidden = false;
  pill.className = "spill " + tone;
  pill.textContent = text;
}

// Settings dialog. One composer, one section per topic - federation is the first, and adding the next
// is adding a render function to the list, not another dialog. It hangs off the status bar rather than
// a dock panel because it is not about the graph being viewed.
//
// The federation section is where the sharing boundary is CHANGED. The Peers panel next door is the
// monitor - health, sync drift, who checked in - and stays read-only; this is the acting surface.
//
// It only narrows. A grant is a chip you can drop, and there is no field to type a wider set into, so
// "widen" is not expressible here at all (the server refuses it too - federation.md 6a - but a UI
// that could ask for it would be inviting an error it then has to explain). Adding or removing a peer
// stays in supragnosis.toml: admission needs the peer's key and bearer hash to arrive out of band,
// which a console cannot do for the operator.
//
// Dropping is two-step - click arms the chip, a second click commits - rather than a native
// confirm(), which blocks the webview and looks nothing like the rest of the page.
let fedCfgArmed = null;   // "<node_id>\u0000<workspace>" of the chip awaiting its second click

// The tabs, in order. One list: the strip, the routing and the default all read from it, so adding a
// topic is adding an entry rather than markup here plus styling there plus a branch somewhere else.
const SETTINGS_TABS = [
  { id: "about", label: "About", render: renderAboutSection },
  { id: "sync", label: "Sync", render: renderFedSection },
];
let settingsTab = "about";

// Tab lists (docs/design-system.md Section 10): the roles a screen reader announces, aria-selected on
// the shown tab, and only that tab in the Tab order - the arrow keys, Home and End move between tabs.
function markTabs(list, tabs, selected) {
  list.setAttribute("role", "tablist");
  for (const t of tabs) {
    const on = t === selected;
    t.setAttribute("role", "tab");
    t.setAttribute("aria-selected", on ? "true" : "false");
    t.tabIndex = on ? 0 : -1;
  }
}
