"use strict";
// --- The type band (docs/design-system.md Section 5, U16) -----------------------------------------
// A type's color is one of sixteen measured colors - eight hues in two tones - not a generated one.
// Every hue stands at least 25 degrees of OKLCH hue from gold, coral and teal, which mean human
// confirmed, contested and healthy, and every color has OKLCH lightness 0.62 or more. The first
// tone's hues stand 0.09 apart in OKLab; design_tokens.rs measures all of it from these values.
const TYPE_BAND = [
  "#889d37", "#82d48c", "#3cd1f5", "#67acf7", "#8986de", "#c38fe3", "#c670ab", "#ff9abd",
  "#adc35e", "#53a45e", "#009cba", "#4c8fd8", "#aeadff", "#aa76c8", "#ffb7e6", "#e17da1",
];
// The light band (Section 5, U27): each slot keeps its hue and is darker, so it is told on a light
// ground - OKLCH lightness 0.42 to 0.67, 3:1 or more on the light --panel - found by a search under
// U16's rules. TYPE_BAND holds whichever band the appearance draws; its dark values stay here too.
const TYPE_BAND_LIGHT = [
  "#647700", "#4f995a", "#0097b6", "#006fcb", "#4b29bb", "#6e2093", "#a6468a", "#c0748e",
  "#758a00", "#007322", "#006176", "#1a5998", "#736fbf", "#9b74b2", "#753963", "#d7307e",
];
const TYPE_BAND_DARK = TYPE_BAND.slice();
let OTHER = "#b0b8c7", EDGE_OTHER = "#5c6472";   // --muted: the unverified ring, as its legend swatch, and a type not in the map
let EDGE = "#2b3345", EDGE_HI = "#f0c469", EDGE_OLD = "#4d4340";
// A relation kind draws no color (Section 5): at rest a relation is one neutral line, and on the
// active node's edges it is drawn brighter and labelled with its kind. Thirty kinds in a band of
// sixteen would say nothing, and the band is the types'.
let EDGE_NEUTRAL = "#6b7383", EDGE_ACTIVE = "#cad2de";   // --ink2
const EDGE_ALPHA = 0.42;         // edge base opacity (low - recedes in a dense graph; raised for the darker ink). On hover/focus, connected edges activate to 1.0
// The node stroke is proportional to the marker radius (scales with the marker on zoom - a
// consistent ratio) + a screen-px floor (so it does not vanish on zoom-out). It is a background-color
// halo, separating node/edge/neighbor (visibility).
const NODE_STROKE_RATIO = 0.35;  // stroke thickness ratio relative to radius (raised)
const NODE_STROKE_MIN = 2;       // minimum stroke thickness (screen px)
const NODE_STROKE_MAX = 5;       // maximum stroke thickness (screen px) - so a large hub does not thicken into a donut
// Canvas palette - mirrors the CSS tokens (the landing's candlelight theme). SURFACE doubles as the
// halo/cutout color, so it must match the body background exactly.
let INK = "#e9e4d6", INK2 = "#cad2de", SURFACE = "#08090d";
let GOLD = "#d9a544", TEAL = "#56b3a2", BAD = "#e07a5f";   // --accent, --ok, --bad
// Forced colors (docs/design-system.md Section 10, U23): the page takes the system's colors, and the
// map redraws in them - "CanvasText" for names, edges and rings, "Canvas" for the ground and
// "Highlight" for what is lit - keeping every mark it draws. Types lose their colors there, and the
// human-confirmed ring, told from the host-signed one by its gold alone, is drawn twice as wide.
const FORCED_COLORS = matchMedia("(forced-colors: active)");
const PALETTE = { OTHER, EDGE_OTHER, EDGE, EDGE_HI, EDGE_OLD, EDGE_NEUTRAL, EDGE_ACTIVE, INK, INK2, SURFACE, GOLD, TEAL, BAD };
// The light appearance (Section 3, U27): the map's palette mirrors the light tokens, as PALETTE does
// the dark ones, and the map redraws when the system's appearance changes.
const LIGHT = matchMedia("(prefers-color-scheme: light)");
const PALETTE_LIGHT = {
  OTHER: "#5c6370", EDGE_OTHER: "#a0a6b0", EDGE: "#c9c5bc", EDGE_HI: "#af7c04", EDGE_OLD: "#b3a39b",
  EDGE_NEUTRAL: "#6b7383", EDGE_ACTIVE: "#3b404b", INK: "#16181e", INK2: "#3b404b", SURFACE: "#fcfbf8",
  GOLD: "#af7c04", TEAL: "#1d7a6a", BAD: "#c4512f",
};
function systemColor(name) {
  const probe = document.createElement("span");
  probe.style.color = name;
  document.body.append(probe);
  const color = window.getComputedStyle(probe).color;
  probe.remove();
  return color;
}
function hearColors() {
  ({ OTHER, EDGE_OTHER, EDGE, EDGE_HI, EDGE_OLD, EDGE_NEUTRAL, EDGE_ACTIVE, INK, INK2, SURFACE, GOLD, TEAL, BAD } =
    LIGHT.matches ? PALETTE_LIGHT : PALETTE);
  TYPE_BAND.splice(0, TYPE_BAND.length, ...(LIGHT.matches ? TYPE_BAND_LIGHT : TYPE_BAND_DARK));
  if (FORCED_COLORS.matches) {
    const text = systemColor("CanvasText");
    OTHER = EDGE_OTHER = EDGE = EDGE_OLD = EDGE_NEUTRAL = EDGE_ACTIVE = INK = INK2 = GOLD = TEAL = BAD = text;
    EDGE_HI = systemColor("Highlight");
    SURFACE = systemColor("Canvas");
  }
}

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
// The GPU renderer under the 2D canvas (design-system.md Section 27.9, U39), where WebGL2 renders to
// half-float targets; `?renderer=2d` holds the 2D canvas alone, for a measure. Once its context is
// lost the 2D canvas draws everything, and under forced colors it does as well.
const glCanvas = document.getElementById("gl");
let gpu = new URLSearchParams(location.search).get("renderer") === "2d" ? null
  : createGpu(glCanvas, () => { gpu = null; glCanvas.classList.remove("on"); requestFrame(); });
// What draws the map's bulk this frame - the region field, the workspace outlines, the relations,
// the bodies and the nodes: the 2D context, or the GPU renderer, which takes the same calls. What is
// text or few - names, badges, pulses, peers - is the 2D context's either way.
let bulk = ctx;
function bulkPath() { return bulk === ctx ? new Path2D() : bulk.path(); }
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
// --- List (docs/design-system.md Section 6, U24) -------------------------------------------------
// The entities the map draws as a table - the same scope, filters and search - sorted by type and
// name. A row selects its node as a click on the map does; List again, or Esc in the table, gives the
// map back with that node selected.
const listBtn = document.getElementById("listBtn"), listWin = document.getElementById("listwin");
const mapTab = document.getElementById("mapTab");
let listOn = false;
// The column List is sorted by, and which way (Section 29.11): it starts by type, then name; a head
// sorts by its column, again to reverse.
let listSort = { key: "type", dir: 1 };
const TIER_RANK = { unverified: 0, agent_extracted: 1, host_signed: 2, human_confirmed: 3 };
const LIST_COLS = {
  name: (a, b) => a.name.localeCompare(b.name),
  type: (a, b) => a.type.localeCompare(b.type) || a.name.localeCompare(b.name),
  trust: (a, b) => (TIER_RANK[a.trust_tier] ?? -1) - (TIER_RANK[b.trust_tier] ?? -1) || a.name.localeCompare(b.name),
  relations: (a, b) => (a.degree || 0) - (b.degree || 0) || a.name.localeCompare(b.name),
  workspace: (a, b) => String(a.workspace).localeCompare(String(b.workspace)) || a.name.localeCompare(b.name),
  said: (a, b) => (a.last_said || 0) - (b.last_said || 0) || a.name.localeCompare(b.name),
};
function listRows() {
  const by = LIST_COLS[listSort.key] || LIST_COLS.type;
  // List follows the working set, not the field (Section 29.11, D8).
  return nodes.filter(n => !typeOff.has(n.type) && (!workSet || workSet.ids.has(n.id)))
    .sort((a, b) => listSort.dir * by(a, b));
}
function listHead(key, label, cls) {
  const sort = listSort.key === key ? (listSort.dir > 0 ? "ascending" : "descending") : "none";
  return html`<th scope="col"${cls ? html` class="${cls}"` : ""} aria-sort="${sort}"><button type="button" class="lsort" data-k="${key}">${label}</button></th>`;
}
function renderList() {
  if (!listOn || !listWin) return;
  const rows = listRows();
  const many = new Set(rows.map(n => n.workspace)).size > 1;
  // Redrawing keeps the person's place (Section 10): the focused row keeps focus.
  const had = listWin.contains(document.activeElement) ? document.activeElement.dataset.id : null;
  listWin.innerHTML = html`<p class="lhead"></p>${workSet ? html`<p class="lset"><span></span> <button type="button" class="linkish lclear">Clear</button></p>` : ""}<table class="ltable"><thead><tr>${listHead("name", "Name")}${listHead("type", "Type")}${
    listHead("trust", "Trust")}${listHead("relations", "Relations", "n")}${listHead("said", "Last said")}${many ? listHead("workspace", "Workspace") : ""}</tr></thead><tbody>${
    rows.map(n => html`<tr${n === focus ? html` class="on"` : ""}><td><button type="button" class="lrow" data-id="${n.id}">${
      // A contested entity carries the map's badge here too, in words for a screen reader (U3).
      n.contested ? html`<span class="i i-circle-alert lbad" role="img" aria-label="Two sources disagree"></span>` : ""}${
      html`<span class="dot" style="background:${typeColor[n.type] || OTHER}"></span>${n.name}</button></td>`}${
      html`<td>${n.type}</td><td>${tierName(n.trust_tier)}</td><td class="n">${n.degree}</td><td>${n.last_said ? obsWhen(n.last_said) : ""}</td>`}${
      many ? html`<td>${n.workspace}</td>` : ""}</tr>`)}</tbody></table>`;
  // List's head says what the status bar says (Section 29.9, U24).
  listWin.querySelector(".lhead").textContent = statsLine(currentWs(), drawnCounts());
  // Under it, what the working set holds, with Clear (Section 29.11).
  const lset = listWin.querySelector(".lset");
  if (lset) {
    const c = drawnCounts();
    lset.firstChild.textContent = rows.length + " of " + c.shown + " entities kept - " + workSet.parts.map(partSaid).join(", ");
    lset.querySelector(".lclear").onclick = clearWorkSet;
  }
  listWin.querySelectorAll(".lsort").forEach(b => {
    b.onclick = () => {
      const k = b.dataset.k;
      listSort = listSort.key === k ? { key: k, dir: -listSort.dir } : { key: k, dir: 1 };
      renderList();
      const again = listWin.querySelector(`.lsort[data-k="${k}"]`);
      if (again) again.focus();
    };
  });
  listWin.querySelectorAll(".lrow").forEach(b => {
    b.onclick = () => {
      const n = nodeById(b.dataset.id);
      if (!n) return;
      select(n);
      for (const r of listWin.querySelectorAll("tr.on")) r.classList.remove("on");
      b.closest("tr").classList.add("on");
    };
  });
  if (had) {
    const again = [...listWin.querySelectorAll(".lrow")].find(b => b.dataset.id === had);
    if (again) again.focus();
  }
}
function setList(on) {
  listOn = on;
  listWin.hidden = !on;
  // Map | List is one tab list (Section 29.11): the selected tab is the one in the tab order.
  for (const [tab, sel] of [[mapTab, !on], [listBtn, on]]) {
    tab.setAttribute("aria-selected", sel ? "true" : "false");
    tab.tabIndex = sel ? 0 : -1;
    tab.classList.toggle("on", sel);
  }
  document.body.classList.toggle("listing", on);   // the minimap is the map's, not the table's
  idleWhileEmpty();
  if (on) {
    renderList();
    const row = listWin.querySelector("tr.on .lrow") || listWin.querySelector(".lrow");
    if (row) row.focus();
  } else mapTab.focus();
}
listBtn.onclick = e => { if (idle(e)) return; if (!listOn) setList(true); };
mapTab.onclick = () => { if (listOn) setList(false); };
arrowTabs(document.getElementById("mapList"));
// With the scope empty, the camera's zoom, Fit and List act on nothing: they stay focusable,
// aria-disabled and described by the empty map's sentence (design-system.md Section 29.10, Section 4).
// While List shows, zoom and Fit act on a map no one sees, and are idle the same way (Section 29.11).
const IDLE_WHEN_EMPTY = ["zin", "zout", "fit", "listBtn"];
const IDLE_WHEN_LISTING = ["zin", "zout", "fit"];
function idle(e) { return e.currentTarget.getAttribute("aria-disabled") === "true"; }
function idleWhileEmpty() {
  const empty = !nodes.length;
  document.body.classList.toggle("scope-empty", empty);
  if (empty && listOn) { setList(false); return; }
  for (const id of IDLE_WHEN_EMPTY) {
    const b = document.getElementById(id);
    if (!b) continue;
    const why = empty ? "emptyTitle" : listOn && IDLE_WHEN_LISTING.includes(id) ? "listIdle" : null;
    if (why) { b.setAttribute("aria-disabled", "true"); b.setAttribute("aria-describedby", why); }
    else { b.removeAttribute("aria-disabled"); b.removeAttribute("aria-describedby"); }
  }
}
listWin.addEventListener("keydown", e => {
  if (e.key === "Escape") { e.stopPropagation(); setList(false); }
});
// The single workspace-selection state ("" = the node default, "*" = all). Shaped like the old
// header <input> ({value}) so every reader keeps working; the WRITERS are the left-rail chips,
// the URL restore, and follow-mode - selection has exactly one UI (the chips) and the status bar
// shows the current value (visible even with the left dock collapsed).
const wsInput = { value: "" };
const searchEl = document.getElementById("search");
const chipBar = document.getElementById("wschips");
const legendNodesEl = document.getElementById("legendNodes"), legendEdgesEl = document.getElementById("legendEdges");
const legendBoundaryEl = document.getElementById("legendBoundary");
const nodeCtEl = document.getElementById("nodeCt"), edgeCtEl = document.getElementById("edgeCt");
const emptyEl = document.getElementById("empty"), logEl = document.getElementById("log");
const loaderEl = document.getElementById("loader"), loaderSayEl = document.getElementById("loaderSay");
const detailEl = document.getElementById("detail");
const obscardEl = document.getElementById("obscard");
// Close the observation detail card on a backdrop (scrim) click - a click on the card itself does not
// bubble to here as `target === obscardEl` only when the scrim is hit directly.
obscardEl.addEventListener("click", ev => { if (ev.target === obscardEl) hideObsCard(); });
const dockLEl = document.getElementById("dockL"), dockREl = document.getElementById("dockR");
const glossaryBodyEl = document.getElementById("glossaryBody"), glossCtEl = document.getElementById("glossCt");
const logBodyEl = document.getElementById("logBody"), logCtEl = document.getElementById("logCt");
// A dock tab-panel is "live" when its tab is active and its dock is shown (replaces the old details.open).
function panelOn(name) {
  const p = document.querySelector('.tabpanel[data-panel="' + name + '"]');
  return !!(p && p.classList.contains("on") && p.closest(".dock").classList.contains("on"));
}

let glossaryTypes = [];          // [{target, name, description, sources, trust_tier}] - /api/types
let curation = null;             // read-only curation signals - /api/curation (Principle 7, generate-not-commit)
let proposals = [];              // proposals with folded state - /api/proposals (Principle 23 gate)
let proposalSel = null;          // the proposal currently previewed on the graph (click to select/toggle)
let follow = true;               // whether the camera follows the most recent agent-activity node
let peersOn = false;             // server mode: draw federated peers as roaming cursor-dots (hub)
let serverMode = false;          // this node is a hub (auto-detected from /api/federation role)
const peerMarkers = new Map();   // peer node_id -> {x,y,tx,ty,color,phase,flare,action,count,seen,sx,sy,queue,qi,dwell,arrivedAt}
const peerEdgeFlash = new Map();  // edge key -> {e,color,life} - an edge a peer marker just visited
let clusterMode = false;         // group by type: type-circle layout + cross-group link emphasis (an alternative organizer)
let hullForce = true;            // hyperedge cohesion+separation physics (Principle 11) - the DEFAULT organizer; suppressed while group mode is on
let hyperMode = true;            // hyperedge hull OVERLAY (fills + labels) - on by default, visual only, independent of hullForce
let hyperedges = [];             // [{id, members:[nodeId], size, sources, trust_tier}] - /api/hypergraph
let lastStats = {};              // the stats of the last successful graph - reused by poll's hyperedge line
let lastOkAt = null;             // when the graph last loaded (status bar says "Not updated since ..." only when stale)
// Graphic-element visibility toggles (all default on). Pure render switches with no effect on layout.
let showLabels = true, showEdges = true, showArrows = true;
let typeHl = null, edgeTypeHl = null;   // legend-chip hover highlight (node type / edge kind) - render-only
const EDGE_LABEL_MAX = 14;   // cap on relation labels shown for an active node's edges (overflow summarized as "+K more")
// The minimap is on until View turns it off (docs/design-system.md Section 6).
let showFootprint = true, showPulses = true, showSuperseded = true, showMini = true;
let allRegions = true;    // draw every context region as a cloud, not only the active node's (design-system.md Section 5)
// Workspace boundaries: a DASHED outline per workspace, drawn only in the all-workspaces view.
//
// A separate layer from the hyperedge hulls above, and deliberately a different shape language -
// those are filled regions saying "these were asserted together" (Principle 11's second-order
// structure), this is an outline saying "these are the same sharing unit". Sharing one visual would
// merge two meanings.
//
// It can be an outline rather than an overlapping region because the workspace partitions the
// OBSERVED graph: `Entity::make_id` hashes the workspace with the name, so an observed entity never
// straddles two. `origins` cannot be drawn this way - a host set unions across nodes (F4), so those
// regions would overlap and an outline would imply a boundary that is not there.
//
// The one exception is handled upstream rather than here. An `entity_merge` can fold ids from two
// workspaces (nothing on that path compares them, and the curation report suggests such merges), so
// the projection reports an empty workspace for an entity whose attestations disagree, and a node
// with no workspace is left out of every outline. An entity that spans two sharing units gets no
// boundary, which is the honest picture.
//
// Render-only, with no force of its own. The hyperedge hulls are the layout organizer (hullForce);
// a second set of attractors would put two meanings in competition over one set of positions.
// One layer, three states, cycled with "b": workspace boundaries, origin boundaries, off.
//
// A mode rather than two layers. They answer different questions - "same sharing unit" and "who
// signed for this" - and stacking two boundary systems over the hyperedge fills leaves a picture
// nobody can read. Switching also makes the difference between them legible: the same graph under
// the two groupings is the comparison worth seeing.
let boundaryMode = "workspace";   // "workspace" | "origin" | "off"
const bridgeSet = new Set();     // ids of nodes connected to another type (linking nodes that join groups)
const pulses = new Map();        // id -> remaining frames (event-node highlight ring animation)
// The working set (Section 29.11, D8): the entities the person asked to keep in view, and where each
// part came from - a search's answer or an entity's related ones. Page memory only; null while none.
let workSet = null;   // { ids: Set, parts: [{ kind: "search", q, n, first } | { kind: "related", name, n }] }
let contestedCount = 0;          // contested nodes in the loaded scope, whatever the legend hides (U3)
// Whether the scope on screen holds entries though it draws no entity, and whether the store holds
// any entry at all - read with the counts, while the empty map shows (design-system.md Section 29.10).
let scopeEntries = false, storeEntries = true, entriesOf = null;
let pulsedAny = false;          // a pulse has been drawn on this page: the key names its ring (Section 29.9)
// The layout's forces and their constants are viewer-sim.js's (Section 27.5), loaded before this.
// Hull rendering: a region is a cloud (docs/design-system.md Section 5) - one ellipse fitted to its
// members, densest at their centre and fading to nothing past the outermost, so it has no edge to
// read as a line. A flat fill of the hull drew a slab whose rim looked like a border. The clouds of a
// frame are mixed in OKLCH in one low-resolution field (drawCloudField) and drawn as one image.
const HULL_LAYER_ALPHA = 0.10;   // a region's density at its core (no node active) - every region is drawn then, so they stay faint
const DIM_REGION = 0.04;         // non-active regions fade to this while a node is active (inspection view)
const HULL_ACTIVE_ALPHA = 0.26;  // the active node's own regions, painted on top
const CLOUD_REACH = 1.8;         // how far the cloud reaches past its outermost member, x the hull's node gap
const HULL_LABEL_BASE = 0.7;     // hull label opacity with no active node
const HULL_LABEL_HOVER_FADE = 0.15; // non-member hull labels fade to this while a node is active
const HULL_NODE_GAP = 14;        // world px: extra gap past the largest member glyph when expanding a hull
let footprintSession = null;     // the session the current footprint belongs to: the daemon's run
let footprintSince = 0;          // when this page first saw that session
const footprint = new Set();     // ids of nodes this session touched - the conversation's knowledge footprint
let nodes = [], edges = [], typeColor = {}, edgeKinds = [];
const posById = new Map();       // id -> {x,y,vx,vy} - layout stability across polls
const typeOff = new Set();       // node types hidden from the legend
const trustOff = new Set();      // trust tiers dimmed from the Trust legend (D5: dimmed, never hidden)
const edgeTypeOff = new Set();   // edge kinds (relation kind) hidden from the legend
let spiralN = 0;
let drag = null, hover = null, focus = null;
let searchLit = null;   // the entity ids the open search answered with (Section 8); null while closed
// Camera: cam = current (drawn), camT = target. Each frame, ease cam toward camT to make
// zoom/pan/focus/fit smooth (removing instant jumps). Coordinates are CSS pixels (same system as mouse events).
let DPR = 1;
const cam = { s: 1, x: 0, y: 0 }, camT = { s: 1, x: 0, y: 0 };
let panning = null, downPos = null, userMoved = false, firstData = true, needFit = false;

