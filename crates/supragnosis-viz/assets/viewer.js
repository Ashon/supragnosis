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
    listHead("trust", "Trust")}${listHead("relations", "Relations", "n")}${many ? listHead("workspace", "Workspace") : ""}</tr></thead><tbody>${
    rows.map(n => html`<tr${n === focus ? html` class="on"` : ""}><td><button type="button" class="lrow" data-id="${n.id}">${
      // A contested entity carries the map's badge here too, in words for a screen reader (U3).
      n.contested ? html`<span class="i i-circle-alert lbad" role="img" aria-label="Two sources disagree"></span>` : ""}${
      html`<span class="dot" style="background:${typeColor[n.type] || OTHER}"></span>${n.name}</button></td>`}${
      html`<td>${n.type}</td><td>${tierName(n.trust_tier)}</td><td class="n">${n.degree}</td>`}${
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

// --- the layout (Section 27.5, U35) --------------------------------------------------------------
// What the layout last reported: how hard it still pushes (alpha), the settle's last collision pass
// still owed, and the largest move of its last step in world units - the settle test the cooling
// schedule cannot give. The layout itself is viewer-sim.js's, in a worker or here.
let alpha = 1, overlapOwed = 0, simMotion = 0;
// Bumped whenever the nodes' positions change - a layout report, a new graph, a drag - so what is
// kept from them (the scene grid, the regions' geometry, the minimap) knows when to work them out again.
let positionsGen = 0;
// Reduced motion (same respect the landing pays to prefers-reduced-motion): instead of animating
// the violent early rearrangement behind a loader, burst-step the sim to convergence within one
// frame and reveal the layout already still.
const REDUCED_MOTION = matchMedia("(prefers-reduced-motion: reduce)").matches;
let settling = true;
let refitOnReveal = false;   // re-frame the graph after a sync-driven re-layout settles (follow mode)
// A view fitted at the reveal is fitted again when the layout first rests, unless the person has moved
// it (design-system.md Section 27.6): a fresh layout settles smaller than it starts, and the frame
// taken at the reveal would leave it small in the window.
let refitAtRest = false;
function nodeRadius(n) { return radiusOfDegree(n.degree); }
// Level of detail (design-system.md Section 26.5, U30), without a step. By a node's radius on the
// screen, in pixels: its halo grows in from HALO_FROM to HALO_TO, under it the fill taking the whole
// disc; its trust ring, the arrowheads into it and the bend of its lanes from FULL_FROM to FULL_TO; and
// on the 2D canvas, under SQUARE_PX, it is a square of its color. A relation fades in by its length on
// the screen from EDGE_FROM to EDGE_TO. Every fade is eased and taken in FADE_STEPS steps, so a zoom
// shows detail arrive continuously while a look draws in at most that many opacities.
const HALO_FROM = 1.2, HALO_TO = 3, FULL_FROM = 2, FULL_TO = 4, SQUARE_PX = 1, EDGE_FROM = 1, EDGE_TO = 3;
const FADE_STEPS = 32;
const LANE_MAX = 46;      // the furthest a lane of parallel relations bends, in world units
// An eased fade: 0 at a, 1 at b, smooth at both ends.
function ease(a, b, x) { const t = Math.min(1, Math.max(0, (x - a) / (b - a))); return t * t * (3 - 2 * t); }
// A fade in FADE_STEPS steps, never 0 while it is under way.
function stepped(v) { return Math.ceil(v * FADE_STEPS) / FADE_STEPS; }
// Placed names on a grid of LABEL_CELL-pixel cells (Section 26.5): a name is checked only against
// the names in the cells it covers, and a cell is the anchor of at most LABEL_PER_CELL of the names
// the budget places. It replaces a list every name was checked against, which grew with the square
// of the names on the screen.
const LABEL_CELL = 100, LABEL_PER_CELL = 2;
function labelGrid() {
  const cells = new Map(), anchors = new Map();
  const key = (cx, cy) => (cx + 4096) * 8192 + (cy + 4096);
  const span = (x, y, w, h, fn) => {
    const x0 = Math.floor(x / LABEL_CELL), x1 = Math.floor((x + w) / LABEL_CELL);
    const y0 = Math.floor(y / LABEL_CELL), y1 = Math.floor((y + h) / LABEL_CELL);
    for (let cy = y0; cy <= y1; cy++) for (let cx = x0; cx <= x1; cx++) if (fn(key(cx, cy)) === false) return false;
    return true;
  };
  return {
    fits: (x, y, w, h) => span(x, y, w, h, k => {
      for (const p of cells.get(k) || []) if (x < p.x + p.w && x + w > p.x && y < p.y + p.h && y + h > p.y) return false;
      return true;
    }),
    add(x, y, w, h) {
      const box = { x, y, w, h };
      span(x, y, w, h, k => { const l = cells.get(k); if (l) l.push(box); else cells.set(k, [box]); });
    },
    full: (px, py) => (anchors.get(key(Math.floor(px / LABEL_CELL), Math.floor(py / LABEL_CELL))) || 0) >= LABEL_PER_CELL,
    anchor(px, py) {
      const k = key(Math.floor(px / LABEL_CELL), Math.floor(py / LABEL_CELL));
      anchors.set(k, (anchors.get(k) || 0) + 1);
    },
  };
}
// Node stroke thickness (world units): proportional to radius + a screen-px floor. It reflects
// cam.s (current zoom), so it scales with the marker on zoom yet keeps a minimum thickness on
// zoom-out. Shared by draw and the edge endpoints.
function nodeStrokeW(n) { return Math.min(NODE_STROKE_MAX / cam.s, Math.max(nodeRadius(n) * NODE_STROKE_RATIO, NODE_STROKE_MIN / cam.s)); }
// Wake the simulation (discrete wakeup). Called only from events: new node/deletion (applyGraph),
// drag, focus. Never called from a continuous condition (overlap) - prevents endless reheating after settling.
function wake(a = 0.7) { alpha = Math.max(alpha, a); layout.wake(a); if (a >= SETTLE_ENTER) settling = true; requestFrame(); }
// Whether the layout still moves, and so whether a frame steps it (design-system.md Section 26.3,
// U28): settling, applying force, a node held, the last collision pass owed, or still coasting on
// the screen. Otherwise a frame only redraws - a pan, a zoom or a hover never steps the layout.
function simWarm() {
  return settling || alpha >= ALPHA_MIN || drag !== null || overlapOwed > 0
    || simMotion * cam.s > MOTION_MIN_PX;
}

// --- render scheduling: draw when something moves, not every frame ------------------------------
// The layout already goes dormant (no force is applied below ALPHA_MIN), but draw() re-scheduled itself
// unconditionally, so a settled graph still re-rendered the whole canvas 60 times a second to
// produce an identical image. A viewer left open on a settled graph is the normal case, not an edge
// one - it is what the thing does while you work.
//
// Two halves, and the second is the one that can go wrong. draw() stops scheduling itself when
// nothing is in motion, and anything that changes the picture has to ask for a frame; a request
// that never comes leaves a stale canvas, which is worse than a wasted one. So the requests hang off
// the input events themselves rather than off the individual handlers that read them: pointer,
// wheel, key, input and resize are a superset of every handler that can move the camera or change
// hover, focus or the type filters, and a superset cannot drift out of step with the handlers the
// way an enumerated list would. Capture phase, so the frame is queued before the handler runs - rAF
// fires after the whole event turn, so it sees the state the handler left.
let rafId = null;
function requestFrame() { if (rafId === null) rafId = requestAnimationFrame(draw); }
hearColors();
// The root's text size over the default 16 pixels (Section 10, U21). The map draws its labels on a
// canvas, which rem does not reach, so every label size there is multiplied by it. A browser's text
// size and the app's View menu set the root; the app says "rootsize" when it does.
let TEXT_SCALE = 1;
function readTextScale() {
  TEXT_SCALE = (parseFloat(window.getComputedStyle(document.documentElement).fontSize) || 16) / 16;
}
readTextScale();
addEventListener("rootsize", () => {
  readTextScale();
  fitStats();
  requestFrame();
});
function repaint() {
  hearColors();
  assignColors();
  renderLegend();
  requestFrame();
}
FORCED_COLORS.addEventListener("change", repaint);
LIGHT.addEventListener("change", repaint);
for (const ev of ["mousedown", "mousemove", "mouseup", "wheel", "keydown", "input", "resize"]) {
  addEventListener(ev, requestFrame, { capture: true, passive: true });
}
// Whether anything is still in motion. Every per-frame source is named here; one missing is an
// animation that stops halfway and stays there until the next mouse move.
function animating() {
  return settling                                                 // loader phase, sim mid-flight
    || alpha >= ALPHA_MIN                                         // the sim still applies force
    || overlapOwed > 0                                            // the settle's last collision pass
    // ...and, after force stops, while anything is still visibly coasting. Measured on screen, so
    // the threshold means the same thing at any zoom: below a twentieth of a pixel per frame there
    // is nothing left to show.
    || simMotion * cam.s > MOTION_MIN_PX
    || cam.s !== camT.s || cam.x !== camT.x || cam.y !== camT.y    // easeCam snaps exactly, so == is safe
    || drag !== null                                              // a node is being dragged
    || pulses.size > 0 || peerEdgeFlash.size > 0                  // ring/flash effects still decaying
    // A highlight is a still picture: an active edge is a solid line with an arrowhead (Section 4).
    || (peersOn && peerMarkers.size > 0);                         // markers ease asymptotically, never arrive
}

// --- Camera (canvas is fullscreen, mouse uses client coordinates) ----------------------------
function toWorld(sx, sy) { return [(sx - cam.x) / cam.s, (sy - cam.y) / cam.s]; }
function easeCam() {
  const k = REDUCED_MOTION ? 1 : 0.22;   // reduced motion: the camera arrives, it does not travel
  cam.s += (camT.s - cam.s) * k; cam.x += (camT.x - cam.x) * k; cam.y += (camT.y - cam.y) * k;
  if (Math.abs(camT.s - cam.s) < 0.001) cam.s = camT.s;
  if (Math.abs(camT.x - cam.x) < 0.25) cam.x = camT.x;
  if (Math.abs(camT.y - cam.y) < 0.25) cam.y = camT.y;
}
// The furthest the map zooms out (design-system.md Section 26.5): 0.15, or as far as it takes to see
// the whole map in its window, whichever is further - a map of any size can be seen whole.
const SCALE_LEAST = 0.005;
function scaleFloor() {
  const box = boundsOf(nodes);
  if (!box) return 0.15;
  const w = mapWindow(), gw = Math.max(1, box.c - box.a), gh = Math.max(1, box.d - box.b);
  const fit = Math.min((w.r - w.l) / gw, (w.b - w.t) / gh) * 0.9;
  return Math.max(SCALE_LEAST, Math.min(0.15, fit));
}
// Change the target scale while keeping the world point under the cursor fixed (converges smoothly via easing).
function zoomAt(sx, sy, f) {
  const wx = (sx - camT.x) / camT.s, wy = (sy - camT.y) / camT.s;
  camT.s = Math.max(scaleFloor(), Math.min(4, camT.s * f));
  camT.x = sx - wx * camT.s; camT.y = sy - wy * camT.s; userMoved = true;
}
const TOP_INSET = 49;      // the frame's header (viewer.css) - compensated in centering/fit
let BOTTOM_INSET = 28;     // the frame's status bar, which grows with the root's text size
function readBarHeight() {
  const bar = document.getElementById("statusbar");
  if (bar && bar.offsetHeight) BOTTOM_INSET = bar.offsetHeight;
}
readBarHeight();
addEventListener("rootsize", readBarHeight);
const DOCK_L = 260, DOCK_R = 300;   // the panels' widths (viewer.css) - reserved so content stays in the map's window
function insetL() { return dockLEl.classList.contains("on") && !sheetWindow() ? DOCK_L : 0; }
function insetR() {
  if (sheetWindow()) return 0;
  if (detailEl.classList.contains("on")) return SHEET_R;   // the inspector sheet takes the right side
  return dockREl.classList.contains("on") ? DOCK_R : 0;
}
const SHEET_R = 360;   // the sheet's width (viewer.css #detail), in the right panel's place
// Under 760px a dock is a sheet from the bottom (viewer.css), so it occludes the bottom, not a side.
function sheetWindow() { return innerWidth < 760; }
// Bottom occlusion: the status bar, plus the detail panel when it is open (panel sits at bottom:30,
// measured live so centering keeps the focused node visible above it). Render detail before centerOn.
function insetB() {
  if (!sheetWindow()) return BOTTOM_INSET;   // wide: the inspector is a side sheet, nothing at the bottom
  const sheet = [dockLEl, dockREl].find(d => d.classList.contains("on"));
  const panel = detailEl.classList.contains("on") ? detailEl : sheet;
  if (!panel) return BOTTOM_INSET;
  const h = panel.getBoundingClientRect().height;
  return h ? BOTTOM_INSET + 8 + h + 8 : BOTTOM_INSET;   // the panel sits 8 above the bar (match the CSS)
}
// Smoothly bring ONE node to the screen center (focus-to-zoom). If zoomed too far out, zoom in
// slightly. Used where there is nothing but the node to frame: the activity feed following a single
// hit, and focusView's fallback for a node with no visible neighbours.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = (insetL() + innerWidth - insetR()) / 2 - n.x * camT.s;   // center in the strip between the rails
  camT.y = (innerHeight + TOP_INSET - insetB()) / 2 - n.y * camT.s; userMoved = true;   // above the detail panel
}

// A type's color is keyed by its NAME, not by its place in the view, so Concept keeps its color from
// one workspace to the next. A name hash picks a preferred hue; in name order, each type of the view
// takes the first free hue from its preferred one, in the first tone, then in the second. Up to eight
// types on a screen differ in hue and up to sixteen in hue or tone; past sixteen a color is shared,
// and pointing at a legend entry, which lights its type's nodes, is what tells them apart.
const BAND_HUES = 8;
function nameSlot(name) {
  let h = 2166136261;   // FNV-1a
  for (let i = 0; i < name.length; i++) { h ^= name.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
  return h % BAND_HUES;
}
function stableColors(names) {
  const out = {}, used = new Set();
  for (const t of [...names].sort()) {
    const pref = nameSlot(t);
    let slot = pref;
    for (let k = 0; k < TYPE_BAND.length; k++) {
      const s = (k < BAND_HUES ? 0 : BAND_HUES) + (pref + k) % BAND_HUES;
      if (!used.has(s)) { slot = s; break; }
    }
    used.add(slot);
    out[t] = TYPE_BAND[slot];
  }
  return out;
}
// The store's most used types (design-system.md Section 29.9, D4), asked once per page beside its
// first graph read: they take the band's slots, every other type its preferred hue, and the page
// keeps what it assigned while it lives, so no click on a workspace recolors a node.
let commonTypes = null;
const commonTypesRead = fetch("/api/common_types", { cache: "no-store" })
  .then(r => (r.ok ? r.json() : null))
  .then(j => { commonTypes = (j && Array.isArray(j.types)) ? j.types : []; })
  .catch(() => { commonTypes = []; });
const typeColorKept = {};
function assignColors() {
  const common = stableColors(new Set(commonTypes || []));
  typeColor = {};
  for (const t of new Set(nodes.map(n => n.type))) {
    if (!typeColorKept[t]) typeColorKept[t] = common[t] || TYPE_BAND[nameSlot(t)];
    typeColor[t] = typeColorKept[t];
  }
  if (FORCED_COLORS.matches) for (const t of Object.keys(typeColor)) typeColor[t] = INK;
  edgeKinds = [...new Set(edges.map(e => e.type))].sort();
}

// --- Remembered layout (docs/design-system.md Section 5) ------------------------------------------
// The local console keeps each workspace's node positions in this browser, keyed by the node's host id
// from /api/surface and the workspace, and the next load starts from them: a workspace seen before
// appears at once instead of behind the settling loader. The read tier remembers nothing - the app
// shows every server from one origin, and a principal's positions would outlive a revoked grant - so
// the key exists only when /api/surface says "local". A position is a convenience; losing one costs a
// settling.
let surfaceNode = null;     // this node's host id on the local console, null elsewhere
// The page's own machine, in words (design-system.md Section 9, D2): "this machine", or the word the
// app's shell gives - "this Mac" - since the viewer stays browser-neutral; and only on the local
// console, the one surface whose machine it is. Any other surface names the server.
function here() { return surfaceNode ? (window.supragnosisHere || "this machine") : "this server"; }
function hereTitle() { const w = here(); return w.charAt(0).toUpperCase() + w.slice(1); }
let layoutScope = null;     // the storage key the current positions belong to
let storedLayout = null;    // {id: [x, y]} read for the scope being loaded
let layoutFresh = true;     // the next graph is the first of its scope
let layoutDirty = false;    // positions moved since the last save
async function loadSurface() {
  try {
    const s = await (await fetch("/api/surface", { cache: "no-store" })).json();
    if (s && s.surface) surfaceKind = String(s.surface);
    if (s && s.surface === "local" && s.node) surfaceNode = String(s.node);
    if (s && s.default_workspace) defaultWs = String(s.default_workspace);
  } catch (_) { /* no surface answer - nothing is remembered */ }
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
// The scope addressed as "" by its name: the workspace an omitted one resolves to, as /api/surface
// says (the node's default locally, the first grant on the read tier), and "default" until it has
// said (design-system.md Section 29.10).
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
    if (contestedCount) row("mk-badge", "Two sources disagree" + (surfaceKind === "local" ? " - select to decide" : ""));
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
  renderLegend(); renderTrustLegend(); renderStats(); requestFrame();
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
// in the viewer's own settings dialog outside the app). Absent only where sync is not set up, or on a
// read tier, which has no peers view; unavailable and not-yet-answered are states it draws.
let surfaceKind = null;     // "local" on the owner's console, another word on a read tier, null unknown
async function refreshSyncPill() {
  const pill = document.getElementById("syncPill");
  if (!pill) return;
  if (surfaceKind !== "local") { pill.hidden = true; return; }
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
// --- Controls say what they are (docs/design-system.md Sections 4 and 10, U19) -----------------
// A switch applies at once and says on or off. A chip that filters says whether it is pressed. One
// choice among a few is a radio group: one tab stop, and the arrows move the choice. The class the
// stylesheet draws follows the state these set, never the other way round.
function setSwitch(el, on) {
  el.setAttribute("role", "switch");
  el.setAttribute("aria-checked", on ? "true" : "false");
  el.classList.toggle("on", !!on);
}
function setPressed(el, on) {
  el.setAttribute("aria-pressed", on ? "true" : "false");
}
// radioGroup(host, label): the host's buttons become the radios of one group named `label`; the
// checked one is the button whose `on` class is set, and none is checked when none is. Arrows and
// Home/End pick and move focus, as clicking does; call it again whenever the buttons or the choice
// change, so the state said is the state drawn.
function radioGroup(host, label) {
  host.setAttribute("role", "radiogroup");
  if (label) host.setAttribute("aria-label", label);
  const radios = [...host.querySelectorAll("button")];
  const checked = radios.find(b => b.classList.contains("on"));
  for (const b of radios) {
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", b === checked ? "true" : "false");
    b.tabIndex = b === (checked || radios[0]) ? 0 : -1;
  }
  if (host.dataset.radios) return;
  host.dataset.radios = "1";
  host.addEventListener("keydown", ev => {
    const all = [...host.querySelectorAll('[role="radio"]')];
    const i = all.indexOf(document.activeElement);
    if (i < 0) return;
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[ev.key];
    const j = ev.key === "Home" ? 0 : ev.key === "End" ? all.length - 1
      : step ? (i + step + all.length) % all.length : -1;
    if (j < 0) return;
    ev.preventDefault();
    // A disabled choice is passed by, as b passes it (Section 29.12).
    let k = j;
    if (step) for (let n = 0; n < all.length && all[k].getAttribute("aria-disabled") === "true"; n++) k = (k + step + all.length) % all.length;
    all[k].click();
    (host.querySelector('[aria-checked="true"]') || all[k]).focus();
  });
}

function arrowTabs(list) {
  list.addEventListener("keydown", ev => {
    if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(ev.key)) return;
    const tabs = [...list.querySelectorAll('[role="tab"]')];
    const i = tabs.indexOf(document.activeElement);
    if (i < 0) return;
    const j = ev.key === "Home" ? 0 : ev.key === "End" ? tabs.length - 1
      : (i + (ev.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    ev.preventDefault();
    // An idle tab is reached and not chosen (Section 29.10, 29.11).
    if (tabs[j].getAttribute("aria-disabled") === "true") { tabs[j].focus(); return; }
    tabs[j].click();
    // A strip that re-renders on click replaces its buttons, so focus the selected one afresh.
    (list.querySelector('[aria-selected="true"]') || tabs[j]).focus();
  });
}

async function renderSettings() {
  const strip = document.getElementById("settingsTabs");
  const host = document.getElementById("settingsBody");
  strip.textContent = "";
  let selected = null;
  for (const t of SETTINGS_TABS) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "stab" + (t.id === settingsTab ? " on" : "");
    b.textContent = t.label;
    b.onclick = () => { settingsTab = t.id; renderSettings(); };
    strip.appendChild(b);
    if (t.id === settingsTab) selected = b;
  }
  markTabs(strip, [...strip.children], selected);
  host.setAttribute("role", "tabpanel");
  host.textContent = "";
  const tab = SETTINGS_TABS.find(t => t.id === settingsTab) || SETTINGS_TABS[0];
  // The tab owns its own fetch and its own failure, so an unreachable surface empties one tab rather
  // than the dialog.
  await tab.render(host);
}

// What this build IS. Read from the binary that answered the request rather than restated in the
// page, so the version cannot drift from what is running.
async function renderAboutSection(host) {
  let a;
  try {
    a = await (await fetch("/api/about", { cache: "no-store" })).json();
  } catch (e) {
    host.innerHTML = '<div class="empty">version information unavailable</div>';
    return;
  }
  const repo = String(a.repository || "");
  const row = (k, v) => html`<dt>${k}</dt><dd>${v}</dd>`;
  host.innerHTML = html`<dl class="kv">${[
    row("Package", String(a.name || "supragnosis")),
    row("Version", String(a.version || "")),
    row("License", String(a.license || "")),
    repo ? row("Source", html`<a href="${repo}" target="_blank" rel="noreferrer noopener">${repo}</a>`) : "",
  ]}</dl><div class="note">Dependency licenses are not restated here - a hand-kept list is one that \
rots without saying so. They are in the manifest and lockfile at the source above, which \
cannot disagree with this build.</div>`;
}

async function renderFedSection(host) {
  let f;
  try {
    f = await (await fetch("/api/federation", { cache: "no-store" })).json();
  } catch (e) {
    host.innerHTML = '<div class="empty">federation status unavailable</div>';
    return;
  }
  if (!f || f.configured === false) {
    host.innerHTML = html`<div class="empty">Sync is not set up on ${here()} - create ~/.supragnosis/supragnosis.toml to join or host</div>`;
    return;
  }

  const parts = [html`<div class="fsec">${hereTitle()}</div>`,
    html`<div class="prow"><span class="pid">${String(f.node_id || "")}</span>`,
    html`<span class="hint"> ${f.role === "hub" ? "Hub" : "Spoke"}</span></div>`];

  const hubs = f.servers || [];
  if (hubs.length) {
    parts.push(html`<div class="fsec">Hubs ${here()} syncs to</div>`);
    for (const s of hubs) {
      const dot = s.healthy ? TEAL : BAD;
      parts.push(html`<div class="prow"><span class="dot" style="background:${dot}"></span> `,
        html`<span class="pid">${String(s.url || "").replace(/^https?:\/\//, "")}</span>`,
        s.version ? html`<span class="hint"> v${s.version}</span>` : "",
        s.healthy ? "" : html`<span class="hint"> unreachable</span>`);
      for (const w of (s.workspaces || [])) {
        const insync = !(w.local_ahead | 0) && !(w.hub_ahead | 0);
        parts.push(html`<div class="hint">${w.workspace}: ${insync
          ? "in sync"
          : `${here()} +${w.local_ahead | 0}, hub +${w.hub_ahead | 0}`}</div>`);
      }
      // The two buckets only this section names, moved from the dock's Peers tab. Listed here but
      // not admitted there is a setup error that would otherwise just vanish; admitted there but not
      // shared here is knowledge left on the table. Intersecting the two lists would hide both.
      for (const w of (s.local_only || [])) parts.push(html`<div class="hint">${w}: <span class="bad">not admitted by the hub</span></div>`);
      for (const w of (s.peer_only || [])) parts.push(html`<div class="hint">${w}: admitted, not shared from ${here()}</div>`);
      // Unknown is not an empty grant set: a hub that has not answered must not read as one that
      // revoked everything.
      if (!s.negotiated_at) parts.push(html`<div class="hint">what it shares: unknown - no answer yet</div>`);
      else if (f.updated_ms) {
        const age = Math.max(0, Math.round((f.updated_ms - s.negotiated_at) / 1000));
        parts.push(html`<div class="hint">what it shares: agreed ${age}s ago</div>`);
      }
      parts.push(html`</div>`);
    }
  }
  const known = f.known_peers || [];
  if (known.length || f.role === "hub") {
    parts.push(html`<div class="fsec">Peers that have checked in</div>`);
    if (!known.length) parts.push(html`<div class="prow none">no peer has checked in yet</div>`);
    for (const p of known) {
      const ago = f.updated_ms && p.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - p.last_seen_ms) / 1000)) : null;
      parts.push(html`<div class="prow"><span class="pid">${String(p.node_id || "").slice(0, 16)}</span>`,
        html`<span class="hint"> ${p.last_action}${ago !== null ? " " + ago + "s ago" : ""} (${p.hits | 0}) - `,
        html`${p.release ? "v" + p.release : "release not known"}, sync ${p.protocol | 0}</span></div>`);
    }
  }

  const admitted = f.admitted || [];
  if (f.role === "hub") {
    parts.push(html`<div class="fsec">Peers admitted here, and what each may read</div>`);
    if (!admitted.length) {
      parts.push(html`<div class="prow none">no peer is admitted - add one in supragnosis.toml</div>`);
    }
    for (const a of admitted) {
      const ws = a.shared_workspaces || [];
      parts.push(html`<div class="prow"><span class="pid">${String(a.node_id || "")}</span><div class="wschips">`);
      if (!ws.length) {
        parts.push(html`<span class="none">admitted, may read nothing</span>`);
      } else {
        for (const w of ws) {
          const armed = fedCfgArmed === a.node_id + "\u0000" + w;
          parts.push(html`<button type="button" class="wsc${armed ? " armed" : ""}" data-node="${a.node_id}" `,
            html`data-ws="${w}" title="${tipText(armed ? "Press again to stop" : "Stop sharing")}">`,
            html`${w}<span class="x">${armed ? "confirm" : html`<span class="i i-x" aria-hidden="true"></span>`}</span></button>`);
        }
      }
      parts.push(html`</div></div>`);
    }
    parts.push(html`<div class="note">Removing a grant takes effect at once and is written to `,
      html`supragnosis.toml. It stops FUTURE reads - it does not take back what has already synced. `,
      html`Granting a workspace, and adding or removing a peer, stay in the file.</div>`);
  }

  host.innerHTML = html`${parts}`;

  host.querySelectorAll(".wsc").forEach(el => {
    el.onclick = async () => {
      const node = el.getAttribute("data-node"), ws = el.getAttribute("data-ws");
      const key = node + "\u0000" + ws;
      if (fedCfgArmed !== key) { fedCfgArmed = key; renderSettings(); return; }
      fedCfgArmed = null;
      const row = (admitted.find(a => a.node_id === node) || {}).shared_workspaces || [];
      const keep = row.filter(w => w !== ws);
      const q = `?node_id=${encodeURIComponent(node)}&workspaces=${encodeURIComponent(keep.join(","))}`;
      try {
        const r = await fetch("/api/peer/share" + q, { method: "POST", cache: "no-store" });
        if (!r.ok) {
          const body = await r.json().catch(() => ({}));
          const err = document.createElement("div");
          err.className = "err";
          err.textContent = body.error || `refused (${r.status})`;
          host.appendChild(err);
        }
      } catch (e) { /* the re-render below shows the unchanged state */ }
      renderSettings();
      refreshSyncPill();
    };
  });
}

function openSettings() {
  const d = document.getElementById("settings");
  if (!d) return;
  fedCfgArmed = null;
  renderSettings();
  d.showModal();
  // A chip left armed across a close would commit on the next stray click.
  d.onclose = () => { fedCfgArmed = null; };
}

// The legend's order, sorted by count then name once per scope, so a later read never reorders a
// chip under the pointer; a type that arrives later is added at the end (Section 29.9).
let legendOrder = { scope: null, nodes: [], edges: [] };
function legendCounts() {
  const nodeCount = {}, edgeCount = {};
  for (const n of nodes) nodeCount[n.type] = (nodeCount[n.type] || 0) + 1;
  for (const e of edges) edgeCount[e.type] = (edgeCount[e.type] || 0) + 1;
  const byCount = c => Object.keys(c).sort((a, b) => c[b] - c[a] || (a < b ? -1 : a > b ? 1 : 0));
  const scope = currentWs();
  if (legendOrder.scope !== scope) legendOrder = { scope, nodes: byCount(nodeCount), edges: byCount(edgeCount) };
  else {
    for (const [k, c] of [["nodes", nodeCount], ["edges", edgeCount]]) {
      const kept = legendOrder[k].filter(t => c[t]);
      for (const t of byCount(c)) if (!kept.includes(t)) kept.push(t);
      legendOrder[k] = kept;
    }
  }
  return { nodeCount, edgeCount };
}
// A section's head says how many of its types are hidden, with Show all (Section 29.9).
function sectionHidden(ctEl, keys, offSet) {
  const head = ctEl && ctEl.parentElement;
  if (!head) return;
  let note = head.querySelector(".rhid");
  const hidden = keys.filter(t => offSet.has(t)).length;
  if (!hidden) { if (note) note.remove(); return; }
  if (!note) { note = document.createElement("span"); note.className = "rhid"; head.appendChild(note); }
  const all = document.createElement("button");
  all.type = "button"; all.className = "linkish"; all.textContent = "Show all";
  all.onclick = () => { for (const t of keys) offSet.delete(t); renderLegend(); renderStats(); };
  note.replaceChildren(document.createTextNode(" - " + hidden + " hidden "), all);
}
function renderLegend() {
  // Node-type and edge-kind legends, each in its own dock section. Clicking a chip toggles that kind's
  // visibility (the off set); Only, or Option-click, shows that kind alone.
  // Chips are recreated on every render - clear any hover highlight (and the chip tooltip) so
  // neither can stick to a dead chip that will never fire mouseleave.
  typeHl = null; edgeTypeHl = null; tip.style.display = "none";
  const { nodeCount, edgeCount } = legendCounts();
  const fill = (host, keys, colorOf, offSet, isEdge) => {
    host.innerHTML = "";
    if (!keys.length) { host.innerHTML = '<span class="lbl">none</span>'; return; }
    const counts = isEdge ? edgeCount : nodeCount;
    const only = t => { offSet.clear(); for (const k of keys) if (k !== t) offSet.add(k); renderLegend(); renderStats(); };
    for (const t of keys) {
      const wrap = document.createElement("span");
      wrap.className = "lgw";
      const el = document.createElement("button");
      el.type = "button";
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      setPressed(el, !offSet.has(t));
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      const n = document.createElement("span"); n.className = "n"; n.textContent = String(counts[t] || 0);
      el.append(sw, document.createTextNode(t || "(none)"), n);
      if (offSet.has(t)) el.appendChild(icon("eye-off"));
      el.onclick = (e) => {
        if (e.altKey) { only(t); return; }
        if (offSet.has(t)) offSet.delete(t); else offSet.add(t);
        renderLegend(); renderStats();
      };
      // Hovering a chip highlights its nodes/edges on the graph (render-only - no sim wake) and
      // shows the type's T-Box definition in the styled tooltip (Principle 8: a type has a stated
      // meaning - surfaced right where the type is read, not only in the definitions below).
      el.onmouseenter = () => { if (isEdge) edgeTypeHl = t; else typeHl = t; showTypeTip(el, t, isEdge); };
      el.onmouseleave = () => {
        if (isEdge) { if (edgeTypeHl === t) edgeTypeHl = null; }
        else if (typeHl === t) typeHl = null;
        tip.style.display = "none";
      };
      const o = document.createElement("button");
      o.type = "button"; o.className = "lgonly"; o.textContent = "Only";
      o.setAttribute("aria-label", "Only " + (t || "(none)"));
      o.onclick = () => only(t);
      wrap.append(el, o);
      host.appendChild(wrap);
    }
  };
  const nodeKeys = legendOrder.nodes, edgeKeys = legendOrder.edges;
  fill(legendNodesEl, nodeKeys, t => typeColor[t], typeOff, false);
  fill(legendEdgesEl, edgeKeys, () => EDGE_NEUTRAL, edgeTypeOff, true);
  nodeCtEl.textContent = nodeKeys.length || "";
  edgeCtEl.textContent = edgeKeys.length || "";
  sectionHidden(nodeCtEl, nodeKeys, typeOff);
  sectionHidden(edgeCtEl, edgeKeys, edgeTypeOff);
  renderBoundaryChips();
  renderList();
}

// Legend chip tooltip: the type's glossary definition (T-Box), anchored beside the chip. A type
// with no recorded definition gets a nudge toward define_type instead of silence - curation as a
// micro-decision in the reading flow (Principle 22), not a separate chore.
function showTypeTip(el, t, isEdge) {
  const target = isEdge ? "relation" : "entity";
  const def = glossaryTypes.find(x => x.target === target && x.name === t);
  // Beside the chip and its Only, which the card must not cover (Section 29.9).
  const r = (el.closest(".lgw") || el).getBoundingClientRect();
  tip.style.display = "block";
  tip.style.left = Math.min(r.right + 10, innerWidth - 330) + "px";
  tip.style.top = Math.max(6, r.top - 4) + "px";
  tip.innerHTML = html`<b>${t || "(none)"}</b> <span class="k">${target} type</span>${def
    ? html`<div class="tdef">${def.description}</div><span class="k">${plural(def.sources | 0, "source")}</span>`
    : html`<div class="tdef none">no definition recorded - give this type a meaning with define_type</div>`}`;
}

// What a hover lights without dimming the rest (Section 29.9): the entity, its relations and its
// neighbours, whose names show. Null while a selection holds the map, which lights its own.
function hoverSet() {
  if (!hover || focus) return null;
  const ns = new Set([hover.id]), es = new Set();
  for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (e.a.id === hover.id || e.b.id === hover.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
  }
  return { ns, es };
}
// The faint strength a selection leaves the rest at, which a dimmed trust tier takes too (D5).
const DIM_NODE = 0.12, DIM_EDGE = 0.06;
// The set of nodes/edges to highlight from hover/focus/search. If none, null (everything highlighted equally).
function activeSet() {
  // A selection dims the rest; a hover does not (hoverSet, design-system.md Section 29.9).
  const anchor = focus;
  if (anchor) {
    const ns = new Set([anchor.id]), es = new Set();
    for (let i = 0; i < edges.length; i++) {
      const e = edges[i];
      if (e.a.id === anchor.id || e.b.id === anchor.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
    }
    return { ns, es };
  }
  // The open search lights what the engine answered, not names that contain the typed text, so the
  // list and the map give one answer (docs/design-system.md Section 8).
  if (searchLit) return { ns: searchLit, es: new Set() };
  // A selected proposal's preview dims everything it does not touch, instead of ringing its
  // targets: rings belong to trust (docs/design-system.md Section 5). Its own overlay - the fold
  // arrows, the accented edges - is drawn on top.
  if (proposalSel) {
    const touched = proposalSel.kind === "entity_merge"
      ? [proposalSel.into, ...(proposalSel.targets || [])].map(nodeById).filter(Boolean)
      : affectedNodes(proposalSel);
    if (touched.length) {
      const ns = new Set(touched.map(n => n.id)), es = new Set();
      for (let i = 0; i < edges.length; i++) if (ns.has(edges[i].a.id) || ns.has(edges[i].b.id)) es.add(i);
      return { ns, es };
    }
  }
  return null;
}

// --- Trust and contest on the map (docs/design-system.md Section 5, U2 and U3) -------------------
// The ring is the node's trust_tier: the highest EFFECTIVE tier among the observations supporting it,
// receiver-evaluated (resolution.md Section 3) - the trust of its best support, not of its winning
// value. When the loaded scope has one tier the map draws no ring and the legend says which, as the
// inspector's scope line does; otherwise every node shows its tier and Agent, the default, is no ring.
const TIER_NAME = { unverified: "Unverified", agent_extracted: "Agent", host_signed: "Host signed", human_confirmed: "Human confirmed" };
// What each tier means, said of a source (Section 29.2); under the Trust legend and in the ask.
const TIER_MEANS = {
  human_confirmed: "A person confirmed it in this app, on one of your machines. No agent's claim outranks it; a later review can lower it.",
  host_signed: "Above Agent, but not by a person: a trusted machine's signed claim, or a review an agent accepted.",
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
  renderTierNote();
  const pill = document.getElementById("contestedPill");
  if (pill) {
    // On the local console the count rides Review's entry (Section 6, U20); on a read tier, where
    // nothing is decided, this pill keeps it alone - and is absent only for a known zero (U8).
    pill.hidden = contestedCount === 0 || surfaceKind === "local";
    pill.textContent = contestedCount + " contested";
  }
}
let trustMeansOpen = false;   // the disclosure survives the legend's redraw
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
      const n = document.createElement("div"); n.className = "wsnote"; n.textContent = "Showing all workspaces - " + defaultName() + " is empty";
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
  // A name the node does not list is said as such on the owner's console (Section 29.9); the read
  // tier refuses an unknown workspace before this shows.
  const unknown = surfaceKind === "local" && wsUnlisted(cur);
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
  // In the app, on the owner's console, a store with no entry leads back to Setup, which says what
  // to ask (Section 29.10). The shell gives its address; a browser has none and shows no button.
  if (window.supragnosisSetup && surfaceKind === "local" && !storeEntries) {
    const b = document.createElement("button");
    b.type = "button"; b.className = "chip"; b.textContent = "Open Setup";
    b.onclick = () => { location.href = window.supragnosisSetup + "?from=" + encodeURIComponent(String(location.href).split("#")[0]); };
    const row = document.createElement("div"); row.className = "e-ws"; row.append(b);
    kids.push(row);
  }
  emptyEl.replaceChildren(...kids);
}

// --- Live MCP activity (SSE) --------------------------------------------------------
function nodeById(id) { return nodesById().get(id); }
// The nodes by id, kept until the node list is replaced (applyGraph builds a new one), so a step or
// a frame does not build its own map, and a lookup does not walk the list.
let byIdOf = null, byIdMap = new Map();
function nodesById() {
  if (byIdOf !== nodes) { byIdOf = nodes; byIdMap = new Map(nodes.map(n => [n.id, n])); }
  return byIdMap;
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
// panel, so a refusal - a merge the fold blocks, the hub's read tier answering 403 - looked exactly
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
  close.type = "button"; close.className = "tx"; close.setAttribute("aria-label", "Dismiss");
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
async function consoleWrite(path, query, said, action, detail) {
  let r;
  try { r = await fetch(path + query, { cache: "no-store" }); }
  catch (e) { toast("bad", "Could not reach the server", "Nothing was changed. Is the daemon running?"); return null; }
  let body = null;
  try { body = await r.json(); } catch (_) { /* a refusal without a JSON body */ }
  if (!r.ok) {
    toast("bad", r.status === 403 ? "This view is read-only" : "Refused", (body && body.error) || ("HTTP " + r.status));
    return null;
  }
  // An action may be built once the write has landed - Undo reads its own preview (Section 29.2).
  if (typeof action === "function") action = await action();
  toast("ok", said, detail || null, action);
  return body || {};
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
      html`<span class="src">${plural(x.sources | 0, "source")}</span>`,
      html`<div class="def">${x.description}</div>`];
    // Contested definitions take the same settling rows as an entity's type, so a type conflict and
    // an entity-type conflict read identically - one mediation pattern (IR5).
    if (x.competitors && x.competitors.length) {
      parts.push(html`<div class="contested${x.contested ? " hot" : ""}">${contestedRows({ name: x.name, field: "definition",
        current: x.description, curObs: x.def_source, tier: "", competitors: x.competitors, contested: x.contested })}</div>`);
    }
    return html`${parts}</div>`;
  };
  const section = (title, items) => html`<div class="gsec">${title} (${items.length})</div>${
    items.length ? items.map(item) : html`<div class="empty">none defined - use define_type</div>`}`;
  glossaryBodyEl.innerHTML =
    html`${section("entity types", group("entity"))}${section("relation types", group("relation"))}`;
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
  if (band && band.available === false) {
    return "unavailable - no embedder is configured, so this signal was not computed (which is not a claim that no pairs exist)";
  }
  if (band && band.embedded < band.examined) {
    return `none among the ${band.embedded} of ${band.examined} entities carrying a name vector - the rest were projected before an embedder existed, so reproject to cover them`;
  }
  return "none - no embedding-near pairs";
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
      add("m:" + m.a + "|" + m.b, "Possible duplicate: " + m.a_name + " ~ " + m.b_name,
        { pair: m, meta: "similar " + m.similarity.toFixed(2) + (m.shared_neighbors ? ", " + m.shared_neighbors + " shared neighbors" : "") });
    }
    // Name variants: the deterministic sibling of the band. A pair can be proposed; three or more show
    // as chips, since propose_merge takes a pair. data-src tells the server which surface found it.
    for (const v of curation.name_variants || []) {
      const m = v.members || [];
      const meta = "grouped by " + v.rung + (v.shared_neighbors ? ", " + v.shared_neighbors + " shared neighbors" : "");
      if (m.length === 2) add("v:" + m[0].id + "|" + m[1].id, "Possible duplicate: " + m[0].name + " ~ " + m[1].name,
        { pair: { a: m[0].id, b: m[1].id, a_name: m[0].name, b_name: m[1].name }, src: "variant:" + v.rung, meta });
      else add("v:" + v.key, "Possible duplicates: " + nameList(names(m)), { group: m, meta });
    }
    for (const g of curation.duplicates || []) add("d:" + g.key, "Same name in two workspaces: " + g.key, { group: g.members || [] });
  } else {
    // Contradictory accepted merges (Principle 6): the remedy is a settling merge, never an edit.
    for (const c of curation.merge_cycles || []) add("y:" + names(c.members).join("|"), "Conflicting merges: " + nameList(names(c.members)),
      { group: c.members || [], hint: "These accepted merges fold into each other - settle them with a new merge." });
    for (const b of curation.grab_bags || []) add("g:" + b.id, "Overloaded region: " + b.size + " entities", { bag: b });
    for (const o of curation.orphans || []) add("o:" + o.id, "Unlinked: " + o.name, { node: o });
    // Conflicts trust settled (resolution.md Section 4.1): informational, and still listed (P6).
    for (const c of (curation.contradictions || []).filter(c => !c.contested)) {
      add("k:" + c.id, "Settled conflict: " + c.name + (c.field === "definition" ? "'s definition" : "'s type"), { settled: c });
    }
    // T-Box axis collisions (Principle 9): a name defined on both axes - informative, usually a mistake.
    for (const n of curation.type_axis_collisions || []) add("x:" + n, "A type on both axes: " + n, { axis: n });
    // A verdict filed away from its proposal decides nothing (proposal-workflow.md I2); an earlier
    // build filed one cast under All in the node default. Listed under All on this node only
    // (design-system.md Section 29.15).
    for (const v of curation.stray_verdicts || []) {
      const p = proposals.find(q => q.id === v.proposal);
      if (!p || (p.state !== "open" && p.state !== "blocked")) continue;   // listed while the proposal still waits
      add("sv:" + v.observation, "A verdict filed away from its proposal: " + proposalTitle(p), { stray: v });
    }
  }
  return items;
}
// The line over a signal queue: what it is, and for the merge band, why it may be empty (P5).
function signalHint(seg) {
  if (seg === "suggestions") {
    const band = curation && !(curation.merge_suggestions || []).length ? " By meaning: " + bandEmptyReason(curation.merge_band) + "." : "";
    return html`<div class="hint">Possible duplicates the store found, and names two workspaces share. Proposing a duplicate opens a merge that waits under Needs you.${band}</div>`;
  }
  if (seg === "health") return html`<div class="hint">Signals about the shape of the knowledge. Nothing here needs deciding today.</div>`;
  return "";
}
function nchipHtml(n) {
  return html`<button type="button" class="nchip" data-id="${n.id}" title="${tipText("Focus " + n.name)}">${n.name}${n.type ? html`<span class="ty">${n.type}</span>` : ""}</button>`;
}
// How a stray verdict's act is said (Section 29.15).
const STRAY_ACTS = { merge: "accept", reject: "reject", withdraw: "withdrawal" };
function signalDetail(it) {
  const b = it.body, local = surfaceKind === "local";
  const parts = [html`<h3>${it.title}</h3>`];
  if (b.pair) {
    const m = b.pair;
    parts.push(html`<div class="ms" data-a="${m.a}" data-b="${m.b}"${b.src ? html` data-src="${b.src}"` : ""}>`,
      nchipHtml({ id: m.a, name: m.a_name }), html`<span class="msx">~</span>`, nchipHtml({ id: m.b, name: m.b_name }),
      html`<span class="mssim">${b.meta}</span>`,
      local ? html`<button class="propmerge" title="${tipText("Propose a merge")}">Propose</button>` : "", html`</div>`);
    // What Propose does is said here, once, rather than in the button's tooltip (Section 4, U25).
    if (local) parts.push(html`<p class="hint">Propose opens a merge of the pair, folding the first into the second; it then waits under Needs you.</p>`);
  } else if (b.group) {
    parts.push(html`<div class="chips">${b.group.map(nchipHtml)}</div>`);
    if (b.meta) parts.push(html`<p class="hint">${b.meta}</p>`);
    if (b.hint) parts.push(html`<p class="hint">${b.hint}</p>`);
  } else if (b.bag) {
    const nm = b.bag.member_names.slice(0, 10).join(", ") + (b.bag.member_names.length > 10 ? ", ..." : "");
    parts.push(html`<div class="gb" data-hid="${b.bag.id}"><span class="sz">${b.bag.size}</span>${nm}${local ? html`<button class="reify" title="${tipText("Name this region")}">Name this region</button>` : ""}</div>`);
  } else if (b.node) {
    parts.push(html`<div class="chips">${nchipHtml(b.node)}</div>`, html`<p class="hint">Observed with no relation to anything else.</p>`);
  } else if (b.settled) {
    const c = b.settled, others = (c.competitors || []).map(x => x.value);
    parts.push(html`<p class="why">${c.current} is shown; ${others.join(", ")} ${others.length > 1 ? "were" : "was"} also asserted.</p>`);
  } else if (b.axis) {
    parts.push(html`<p class="why">Defined as both an entity type and a relation type - see Definitions in the filters.</p>`);
  } else if (b.stray) {
    const v = b.stray, act = STRAY_ACTS[v.decision] || "verdict";
    parts.push(html`<p class="why">This ${act} was filed in ${v.filed_in}, where the proposal does not live, so it decides nothing. The proposal still waits in ${v.proposal_in}.</p>`);
    if (proposals.some(q => q.id === v.proposal)) parts.push(html`<div class="rv-acts"><button type="button" class="btn" data-stray="${v.proposal}">Show the proposal</button>${skipButton()}</div>`);
    else parts.push(html`<div class="rv-acts">${skipButton()}</div>`);
    parts.push(html`<p class="hint"><kbd>J</kbd> <kbd>K</kbd> move between items.</p>`);
    return parts;
  }
  if (itemNodes(it).length) parts.push(html`<div class="rv-acts"><button type="button" class="btn" data-map="1">Show on the map</button>${skipButton()}</div>`);
  else parts.push(html`<div class="rv-acts">${skipButton()}</div>`);
  parts.push(html`<p class="hint"><kbd>J</kbd> <kbd>K</kbd> move between items.</p>`);
  return parts;
}
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
      inp.placeholder = "region name (optional)";
      inp.onclick = (e2) => e2.stopPropagation();
      const ok = document.createElement("button");
      ok.type = "button"; ok.title = tipText("Name this region");
      ok.setAttribute("aria-label", "Name this region");
      ok.appendChild(icon("check"));
      ok.onclick = async (e2) => {
        e2.stopPropagation();
        const ws = wsInput.value.trim();
        let q = "?hyperedge=" + encodeURIComponent(row.dataset.hid);
        if (inp.value.trim()) q += "&name=" + encodeURIComponent(inp.value.trim());
        if (ws) q += "&workspace=" + encodeURIComponent(ws);
        await consoleWrite("/api/reify", q, "Region named - it is an entity now, related to its members");
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
      await consoleWrite("/api/propose_merge", q, "Merge proposed - it waits under Needs you for a verdict");
      await poll();
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

function nameOf(id) { const n = nodeById(id); return n ? n.name : HASH_ID.test(id) ? "(an entity not on the map)" : "(" + id + ")"; }

// --- The Review page (docs/design-system.md Section 7, U4) ---------------------------------------
// Everything that waits for a person's decision, in one place: Needs you (suspected secrets, open and
// blocked proposals, contested beliefs), Suggestions and Health. It is a route of this page (#review),
// so it works in the app, on a hub's read tier and in a browser alike. A decision acts on one item,
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
    title.replaceChildren("Review", scope);
  } else title.textContent = statsScope(currentWs());
  document.getElementById("backGraph").hidden = !onReview;
  document.getElementById("panelL").hidden = onReview;
  document.getElementById("panelR").hidden = onReview;
}
// Cmd+[ goes back, as on a Mac; Esc does too (Section 6a, U26).
addEventListener("keydown", e => {
  if (e.key === "[" && (e.metaKey || e.ctrlKey) && reviewOpen()) { e.preventDefault(); location.hash = ""; }
});
const KIND_WORDS = {
  entity_merge: "Merge", entity_split: "Un-merge", claim_promotion: "Confirm a claim",
  claim_demotion: "Demote a claim", tbox_change: "Change a type", recall: "Recall",
};
// A proposal's state said as its outcome (Section 29.3, U17); the ids stay the fold's.
const OUTCOME = { merged: "Accepted", rejected: "Rejected", withdrawn: "Withdrawn", blocked: "Blocked" };
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
// One title per proposal, the same under Needs you and Decided: "Merge PostgreSQL into Postgres",
// Confirm a claim: "The ledger is..." (Ledger).
function proposalTitle(p) {
  const said = (p.targets || []).map((_, i) => targetSaid(p, i));
  if (p.kind === "entity_merge" && p.into) {
    const gone = said.filter((s, i) => s && p.targets[i] !== p.into), into = intoSaid(p);
    if (gone.length && into) return "Merge " + gone.join(", ") + " into " + into;
  }
  if (p.kind === "entity_split" && said[0]) return "Un-merge " + said[0];
  const named = said.filter(Boolean);
  return (KIND_WORDS[p.kind] || p.kind) + (named.length ? ": " + named.join(", ") : "");
}
// A contested row says what it asks: "Ledger's type: Table or Concept?", two values at most.
function contestedTitle(c) {
  if (c.field === "definition") return c.name + "'s definition: which meaning?";
  const values = [c.current, ...(c.competitors || []).map(x => x.value)];
  const shown = values.length > 2 ? values.slice(0, 2).join(", ") + " or " + (values.length - 2) + " more" : values.join(" or ");
  return c.name + "'s type: " + shown + "?";
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
function reviewCount() {
  // Before the first curation read, the graph's own contested count stands in for the contested rows.
  return needsItems().length + (curation ? 0 : contestedCount);
}
function itemTitle(it) {
  if (it.kind === "secret") return "A credential may be in the log";
  if (it.kind === "contested") return contestedTitle(it.c);
  if (it.kind === "signal") return it.title;
  return proposalTitle(it.p);
}
function itemTag(it) {
  if (it.kind === "secret") return html`<span class="rv-k bad">Secret</span>`;
  if (it.kind === "contested") return html`<span class="rv-k bad">Contested</span>`;
  if (it.kind === "signal") return "";
  // Only what the title does not say: under Needs you that it is blocked, under Decided its outcome.
  const st = it.p.state;
  return st === "open" ? "" : html`<span class="rv-st ${st}">${OUTCOME[st] || st}</span>`;
}
function updateReviewPill() {
  const pill = document.getElementById("reviewPill");
  if (!pill) return;
  const n = reviewCount();
  const contested = curation ? needsItems().filter(it => it.kind === "contested").length : contestedCount;
  // Always the way to Review on the local console (Section 6, U20), with one count: what waits under
  // Needs you, and how many of those are contested beliefs. Absent on a read tier (U8).
  pill.hidden = surfaceKind !== "local";
  pill.className = "spill" + (n ? " warn" : "");
  pill.textContent = n ? (n === 1 ? "1 needs you" : n + " need you") + (contested ? " (" + contested + " contested)" : "") : "Review";
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
  document.title = (reviewOpen() ? "Review" : "Graph") + " - Supragnosis";
  if (!reviewOpen()) { reviewSig = ""; return; }
  const local = surfaceKind === "local";
  const items = segItems();
  // Redraw only when what is shown changed: a redraw replaces the buttons under the pointer and the
  // keyboard focus, which every 2.5-second poll would otherwise take away. Each item's state is in
  // it, so a decided item leaves at the next read (Section 29.3).
  const sig = JSON.stringify([reviewSeg, reviewSel, local, recallArmed, currentWs(), items.map(itemSig),
    ["needs", "decided", "suggestions", "health"].map(s => s === reviewSeg ? items.length : segItems(s).length),
    reviewView && [reviewView.id, (reviewView.checks || []).length], undoView && [undoView.id, undoView.state],
    reviewEvidence && [reviewEvidence.id, reviewEvidence.cols.map(c => c.list.length), !!reviewEvidence.opened]]);
  if (sig === reviewSig) return;
  reviewSig = sig;
  // The segments' counts: only Needs you's asks; the others are drawn muted (Section 29.3).
  const segs = [["needs", "Needs you"], ["decided", "Decided"], ["suggestions", "Suggestions"], ["health", "Health"]];
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
  const empty = { needs: all ? "Nothing waits for you in any workspace." : "Nothing waits for you in this workspace.",
    decided: "Nothing decided here yet.", suggestions: "No suggestions here.", health: "Nothing to report here." }[reviewSeg];
  list.innerHTML = html`${signalHint(reviewSeg)}${items.length
    ? items.map(it => html`<button type="button" class="rv-item${it.key === reviewSel ? " on" : ""}" data-key="${it.key}">${itemTag(it)}<span class="rv-t">${itemTitle(it)}</span></button>`)
    : html`<div class="empty">${empty}</div>`}`;
  list.querySelectorAll(".rv-item").forEach(b => { b.onclick = () => { reviewSel = b.dataset.key; renderReview(); }; });
  // Kept until a row takes it: the first draw can come before the proposals have been read.
  if (hadFocus) { const on = list.querySelector(".rv-item.on"); if (on) { on.focus(); reviewFocus = false; } }
  const it = items.find(x => x.key === reviewSel);
  detail.innerHTML = html`${it ? itemDetail(it, local) : html`<div class="empty">${items.length ? "Select an item." : empty}</div>`}`;
  if (!local) detail.querySelectorAll(".propmerge, .reify").forEach(b => b.remove());   // the read tier changes nothing
  if (it && it.kind === "proposal" && (!reviewView || reviewView.id !== it.p.id)) fetchReviewView(it.p.id);
  if (it && it.kind === "proposal" && undoable(it.p) && local && (!undoView || undoView.id !== it.p.id)) fetchUndoView(it.p.id);
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
  return html`<button type="button" class="btn" data-skip="1" aria-keyshortcuts="J">Skip <kbd aria-hidden="true">J</kbd></button>`;
}
function itemDetail(it, local) {
  if (it.kind === "secret") {
    const f = it.f;
    return [html`<h3>A credential may be in the log</h3>`,
      html`<p class="why">A ${f.pattern} pattern matched in an observation's ${f.field}, at byte ${f.at | 0}. It is not shown here on purpose.</p>`,
      html`<p class="why">The log is append-only, so it cannot be removed yet. Stop it spreading by narrowing what this workspace shares, and rotate the credential.</p>`,
      html`<div class="rv-acts"><button type="button" class="btn" data-obs="${f.observation}">Open the observation</button>${skipButton()}</div>`];
  }
  if (it.kind === "contested") {
    const c = it.c, def = c.field === "definition";
    return [html`<h3>${contestedTitle(c)}</h3>`,
      html`<p class="why">Two ${def ? "definitions" : "values"} tie on trust, so Supragnosis does not pick one for you. The other stays in the log and can be chosen again.</p>`,
      html`<div class="contested hot">${contestedRows({ name: c.name, field: def ? "definition" : (c.field === "kind" ? "type" : c.field), entity: def ? "" : c.id,
        current: c.current, curObs: c.kind_source, tier: "", competitors: c.competitors || [], contested: c.contested, ws: c.workspace || (nodeById(c.id) || {}).workspace || "" })}</div>`,
      html`<div class="rv-acts">${def ? "" : html`<button type="button" class="btn" data-map="1">Show on the map</button>`}${skipButton()}</div>`,
      local ? html`<p class="hint"><kbd>1</kbd> <kbd>2</kbd> confirm a value. <kbd>J</kbd> <kbd>K</kbd> move between items.</p>` : html`<p class="why">This view is read-only.</p>`];
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
  parts.push(html`<div class="sec">${decided ? "What it would change now" : "What changes"}</div>`, identityHtml(p, ev), diffHtml(v ? v.diff : null));
  parts.push(html`<div class="sec">Checks</div>`);
  if (!v) parts.push(html`<div class="dnote">reading the checks...</div>`);
  else {
    if (failing.length) parts.push(html`<div class="rv-block"><b>Blocked.</b> ${failing.map(c => html`<div>${checkSaid(p, c)}: ${c.detail}</div>`)}<div>The merge cannot reach canon while this holds. Reject it here, or fix the proposal and open it again.</div></div>`);
    parts.push(checkListHtml(p, v.checks));
  }
  parts.push(html`<div class="sec">Evidence</div>`, evidenceHtml(p, ev));
  // Who asked: the opening event in the who-line grammar, opening its card; "self-attested", which
  // every opened proposal carries, is not drawn (Section 29.7).
  const opened = ev && ev.opened;
  parts.push(html`<div class="sec">Who asked</div>`, opened
    ? html`<div class="hint"><button type="button" class="linkish" data-obs="${opened.id}">${whoLine(opened.effective_tier, authorOf(opened))}</button></div>`
    : html`<div class="hint">${p.opened_at ? new Date(p.opened_at).toLocaleString() : ""}</div>`);
  if (!local) { parts.push(html`<p class="why">This view is read-only.</p>`); return parts; }
  const acts = [];
  const ready = !!v;   // the diff and checks are on screen
  const accept = p.state === "open" || p.state === "rejected";
  if (accept) {
    const armed = recallArmed === p.id;
    const label = p.kind === "recall" && armed ? "Confirm the recall" : p.state === "rejected" ? "Accept after all" : "Accept";
    const off = !ready || failing.length;
    acts.push(html`<button type="button" class="btn p" data-decide="merge" aria-keyshortcuts="A"${off ? html` disabled` : ""}>${label} <kbd aria-hidden="true">A</kbd></button>`);
  }
  if (p.state === "open" || p.state === "blocked") acts.push(html`<button type="button" class="btn" data-decide="reject" aria-keyshortcuts="R"${ready ? "" : html` disabled`}>Reject <kbd aria-hidden="true">R</kbd></button>`);
  if (p.state === "open" || p.state === "blocked") acts.push(skipButton());
  const reversed = proposals.some(q => q.kind === "entity_split" && q.state === "merged" && (q.targets || []).includes(p.id));
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) acts.push(html`<button type="button" class="btn" data-unmerge="1">Un-merge</button>`);
  const u = undoable(p) ? undoParts(p) : null;
  if (u) acts.push(u.button);
  acts.push(html`<button type="button" class="btn" data-map="1">Show on the map</button>`);
  parts.push(html`<div class="rv-acts">${acts}</div>`);
  if (u) parts.push(u.note);
  if (p.state === "rejected" && p.merge_verdict) parts.push(html`<p class="hint">If its check passes later, the accept it carries still merges it.</p>`);
  else if (p.state === "rejected") parts.push(html`<p class="hint">A reject is provisional: accepting it later still merges it.</p>`);
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) parts.push(html`<p class="hint">Un-merge opens a proposal to separate them again. The pair is then never suggested again, and merging them later takes a proposal distinguishable from this one.</p>`);
  if (p.state === "merged" && !undoable(p) && !(p.kind === "entity_merge" && !reversed)) {
    parts.push(html`<p class="hint">${p.kind === "entity_split" ? "Merging them again takes a new proposal." : "Undoing this takes a new proposal."}</p>`);
  }
  parts.push(html`<p class="hint">${accept ? "Accept casts your verdict; nothing is deleted. " : ""}<kbd>J</kbd> <kbd>K</kbd> move between items.</p>`);
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
    rows.push(html`<div class="drow ident">${name} leaves the map; its name becomes an alias of ${keptName}</div>`);
    const col = ev && ev.cols.find(c => c.id === t);
    if (col && col.list.length) rows.push(html`<div class="drow ident">${plural(col.list.length, "observation")} ${col.list.length === 1 ? "counts" : "count"} for ${keptName} from then on</div>`);
  }
  const still = ((curation && curation.contradictions) || []).find(c => c.id === p.into && c.contested);
  if (still) rows.push(html`<div class="drow ident">${keptName}'s ${still.field === "kind" ? "type" : still.field} stays contested - <button type="button" class="linkish" data-review="${"c:" + still.id}">also under Needs you</button></div>`);
  return html`<div class="ddiff">${rows}</div>`;
}
// Every check, in words keyed by the proposal's kind, the check's name and whether it passed
// (Sections 29.7 and 29.15), so a failed check never reads as its passing words; one the page has
// no words for as the engine names it, with the engine's detail when it fails.
const CHECK_SAID = {
  "entity_merge|referential integrity": (p, ok) => ok ? "All " + ((p.targets || []).length + (p.into && !(p.targets || []).includes(p.into) ? 1 : 0)) + " entities are in its workspace" : "Not every entity it names is in its workspace",
  "entity_merge|distinct targets": (p, ok) => ok ? "The targets are at least two entities" : "It names fewer than two entities",
  "entity_merge|canonical target named": (p, ok) => ok ? "It names the entity to keep" : "The entity to keep is not among the ones it names",
  "tbox_change|t-box axis consistency": (p, ok) => ok ? "Types are consistent" : "A name is defined as both an entity type and a relation type",
  "entity_split|reversible target": (p, ok) => ok ? "It names a merge to undo" : "The merge it names is not in its workspace, or was never accepted",
  "claim_promotion|referential integrity": (p, ok) => ok ? "Its observations are in its workspace" : "Not every observation it names is in its workspace",
  "claim_demotion|referential integrity": (p, ok) => ok ? "Its observations are in its workspace" : "Not every observation it names is in its workspace",
  "claim_promotion|tier stated": (p, ok) => ok ? "It says the tier it asks for" : "It does not say the tier it asks for",
  "claim_demotion|tier stated": (p, ok) => ok ? "It says the tier it asks for" : "It does not say the tier it asks for",
};
function checkSaid(p, c) {
  const f = CHECK_SAID[p.kind + "|" + c.name];
  return f ? f(p, c.passed) : c.passed ? c.name : c.name + ": " + c.detail;
}
function checkListHtml(p, cs) {
  const rows = (cs || []).map(c => {
    if (c.name === "trust profile") {
      const said = String(c.detail || "").split(", ").filter(Boolean).map(x => { const [n, t] = x.split(" "); return n + " " + tierName(t); }).join(", ");
      return html`<div class="ckrow"><span class="i i-info" aria-hidden="true"></span><span>Sources: ${said || "none"}</span></div>`;
    }
    return html`<div class="ckrow ${c.passed ? "ok" : "bad"}"><span class="i ${c.passed ? "i-circle-check" : "i-circle-alert"}" role="img" aria-label="${c.passed ? "Passed" : "Failed"}"></span><span>${checkSaid(p, c)}</span></div>`;
  });
  return html`<div class="cklist">${rows}</div>`;
}
// The evidence in the Log's row anatomy, each row opening its card: one column per entity for a merge.
function evidenceHtml(p, ev) {
  if (!ev) return html`<div class="dnote">reading the evidence...</div>`;
  if (!ev.cols.length) return html`<div class="dnote">This proposal rests on no observation of its own.</div>`;
  return html`<div class="evcols">${ev.cols.map(c => html`<div class="evcol">${c.name ? html`<div class="evh">${c.name} <span class="ct">${c.list.length}</span></div>` : ""}${
    c.list.map((o, i, l) => obsRowHtml(o, i, l))}</div>`)}</div>`;
}
// A merged gate act has a reverse on its Decided item (Section 29.3).
function undoable(p) { return p.state === "merged" && (p.kind === "claim_promotion" || p.kind === "claim_demotion"); }
function undoLabel(p) {
  return p.kind === "claim_demotion" ? "Undo demotion" : String(p.tier) === "human_confirmed" ? "Undo confirmation" : "Undo promotion";
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
  if (!u || u.state === "reading") return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">Reading what undoing this moves back...</p>` };
  if (u.state === "failed") return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">${label} is not offered: what it would move back could not be read.</p>` };
  if (!u.acts.length) return { button: html`<button type="button" class="btn" data-undo="1"${off}>${label}</button>`, note: html`<p class="hint">Nothing to undo: another decision holds this source where it is.</p>` };
  const failing = (u.checks || []).filter(c => c.blocking && !c.passed);
  const lines = u.acts.map(a => a.kind === "claim_demotion"
    ? "Undo returns this source to " + tierName(a.tier) + " and keeps it there until someone confirms it again."
    : "Undo returns this source to " + tierName(a.tier) + ".");
  const moves = ringMoves(u.diff, null);
  const by = p.kind === "claim_demotion" && p.proposer ? " This demotion was cast by " + p.proposer + "." : "";
  const note = [...new Set(lines)].join(" ") + (moves.length ? " " + moves.join(". ") + "." : "") + by
    + (failing.length ? " It cannot be cast while this check fails: " + failing.map(c => c.name + ": " + c.detail).join("; ") + "." : "");
  return { button: html`<button type="button" class="btn" data-undo="1"${failing.length ? off : ""}>${label}</button>`, note: html`<p class="hint">${note}</p>` };
}
// Casts the reverse, one act per target, one after the other; the notice says which targets went
// back when one does not (U1).
async function undoDecided(p) {
  const u = undoView && undoView.id === p.id ? undoView : null;
  if (!u || u.state !== "ok" || !u.acts.length || (u.checks || []).some(c => c.blocking && !c.passed)) return;
  const rationale = "Undid " + proposalTitle(p);
  const said = a => targetSaid(p, (p.targets || []).indexOf(a.observation)) || "a source";
  const back = [];
  for (const a of u.acts) {
    const ok = await consoleWrite("/api/resolve", settleQuery(a.observation, a.tier, a.kind, rationale),
      "Undone - " + said(a) + " is " + tierName(a.tier) + " again", { decided: true });
    if (!ok) {
      if (back.length) toast("bad", "Undo stopped partway", "Returned: " + back.join(", ") + ". Not returned: "
        + u.acts.filter(x => !back.includes(said(x))).map(said).join(", ") + ".");
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
  await consoleWrite("/api/propose_split", sq, "Un-merge proposed - it waits under Needs you for a verdict");
  refreshProposals();
}
// One decision on one item (U4). It runs only when the item's view - its diff and checks - is the
// one on screen, never for a blocked merge, and a recall asks twice; there is no decide-all.
async function reviewDecide(item, decision) {
  if (!item || item.kind !== "proposal") return;
  const p = item.p;
  if (!reviewView || reviewView.id !== p.id) return;   // the diff and checks are not on screen yet
  if (decision === "merge") {
    if (reviewView.checks.some(c => c.blocking && !c.passed)) return;
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
  const action = decision === "merge" && p.kind === "entity_merge" ? { label: "Un-merge", decided: true, run: () => proposeSplit(p) } : null;
  const done = decision === "merge"
    ? await consoleWrite("/api/review", q, "Accepted: " + title, action)
    : await consoleWrite("/api/review", q, "Rejected: " + title, null, "Accept after all is under Decided.");
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
  if (!reviewOpen()) return;
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
  else if (e.key === "a" && surfaceKind === "local") reviewDecide(item, "merge");
  else if (e.key === "r" && surfaceKind === "local") reviewDecide(item, "reject");
  else if (/^[1-9]$/.test(e.key) && item && item.kind === "contested" && surfaceKind === "local") {
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
  reviewPreview = { seg: reviewSeg, key: it.key, title: itemTitle(it) };
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
  bar.querySelector(".pv-t").textContent = "Preview: " + reviewPreview.title;
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

// Render the diff as a before -> after comparison. An uncomputable diff says so: for the three
// proposal kinds that still enforce nothing, an empty diff would read as "changes nothing".
// A diff row's field, in the screen's words (design-system.md Section 29.6): the engine's ids
// `kind` and `edge` are a type and a relation.
const FIELD_SAID = { kind: "Type", edge: "Relation", tier: "Trust", definition: "Definition", description: "Description" };
function fieldSaid(f) { return FIELD_SAID[f] || String(f || ""); }
function diffHtml(d) {
  if (!d) return html`<div class="dnote">computing the diff...</div>`;
  if (!d.computable) return html`<div class="dnote">no diff - ${d.note || "not computable for this kind"}</div>`;
  const tiers = (d.tier_changes || []).map(t => [
    html`<div class="drow"><span class="dk">${fieldSaid("tier")}</span><code>${t.observation.slice(0, 10)}</code>`,
    html`<span class="dfrom">${tierName(t.from)}</span><span class="darr">-&gt;</span><span class="dto">${tierName(t.to)}</span></div>`]);
  const beliefs = (d.overturned || []).map(b => {
    const settled = b.contested_before && !b.contested_after;
    const created = !b.contested_before && b.contested_after;
    const flag = settled ? html`<span class="dsettled">settles a contradiction</span>`
               : created ? html`<span class="dcreated">creates a contradiction</span>` : "";
    return [html`<div class="drow"><span class="dk">${fieldSaid(b.field)}</span>`,
      html`<button type="button" class="nchip" data-id="${b.entity}" title="${tipText("Focus " + b.name)}">${b.name}</button>`,
      html`<span class="dfrom">${b.from || "(none)"}</span><span class="darr">-&gt;</span>`,
      html`<span class="dto">${b.to || "(none)"}</span>${flag}</div>`];
  });
  // entity_merge: which references move onto the canonical id, and which edges stop existing.
  // A self-loop is dropped by graph(), so that edge vanishes on accept - not readable from the
  // canvas overlay, which can only accent edges incident to a target.
  const rewires = (d.rewired || []).map(r => [
    html`<div class="drow"><span class="dk">${fieldSaid("edge")}</span><code>${r.kind}</code>`,
    html`<span class="dfrom">${r.from_name}</span><span class="darr">-&gt;</span>`,
    html`<span class="dto">${r.to_name}</span>`,
    html`<span class="dother">(${r.other_name})</span>`,
    r.becomes_self_loop ? html`<span class="dcreated">becomes a loop, so the relation goes</span>` : "",
    html`</div>`]);
  if (!tiers.length && !beliefs.length && !rewires.length) return html`<div class="dnote">computed: this proposal overturns no current belief</div>`;
  return html`<div class="ddiff">${tiers}${beliefs}${rewires}</div>`;
}

// Failing blocking checks, shown above the accept button. A blocking failure means the fold would
// refuse the merge anyway, so surfacing it here turns a silent "nothing happened" into a reason.
function checksHtml(cs) {
  if (!cs || !cs.length) return html``;
  const bad = cs.filter(c => c.blocking && !c.passed);
  if (!bad.length) return html`<div class="dnote">checks pass</div>`;
  return html`<div class="cblock">${bad.map(c => [
    html`<div class="crow"><span class="cbad">blocked</span><span class="ck">${c.name}</span>`,
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
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/proposals" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const p = await r.json(); if (Array.isArray(p)) proposals = p; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderReview();
}

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
// it, then its targets in names - "Merge proposed: PostgreSQL into Postgres". Every kind the engine
// opens has its noun and every event its verb; a row the page cannot phrase says "Something changed"
// and keeps the stored text in its card. A verdict carries its decision, so its verb is the
// decision's.
const PROPOSAL_NOUN = {
  entity_merge: "Merge", entity_split: "Un-merge", claim_promotion: "Claim confirmation",
  claim_demotion: "Claim demotion", tbox_change: "Type change", recall: "Recall",
};
const EVENT_VERB = { opened: "proposed", merge: "accepted", reject: "rejected", withdrawn: "withdrawn", comment: "discussed" };
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
  const noun = PROPOSAL_NOUN[p.kind];
  const verb = EVENT_VERB[p.event === "verdict" ? p.decision : p.event];
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
    const by = writerOf({ source_ref: g.surface, origin_node: g.origin_node });
    const what = g.promotion ? tierName(o.effective_tier) + " by " + by : "Lowered to " + tierName(o.effective_tier) + " by " + by;
    parts.push(html`<div class="oc-gate"><span>${what}, ${obsWhenFull(g.at)}</span><button type="button" class="linkish" data-open-review="${g.proposal}">Open in Review</button></div>`);
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
      html`<div class="ocprop">${PROPOSAL_NOUN[p.kind] || "Proposal"} - ${(OUTCOME[p.state] || (p.state === "open" ? "Open" : p.state || "")).toLowerCase()}</div>`,
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
function hideObsCard() { obscardEl.className = ""; obscardEl.innerHTML = ""; }
function showObsCard(o) {
  obscardEl.innerHTML = html`${obsCardHtml(o)}`;
  obscardEl.className = "on";
  obscardEl.querySelector(".close").onclick = hideObsCard;
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
// and any other by its origin id's first eight characters. A console verdict written here is "you
// at this console" on the local console.
let ownNode = null;   // this node's sync id, from the Sync read; null until known
function writtenHere(a) { return !a.origin_node || (!!ownNode && a.origin_node === ownNode); }
function writerOf(a) {
  if (!a) return "";
  const mine = writtenHere(a);
  const machine = mine ? here() : String(a.origin_node).slice(0, 8);
  if (a.source_ref === "surface:console") return mine && surfaceKind === "local" ? "you at this console" : "a person at " + machine;
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
function ringMoves(diff, skip) {
  const moved = new Map();
  for (const r of (diff && diff.reach) || []) for (const e of r.entities || []) {
    if (e.from !== e.to && e.entity !== skip) moved.set(e.entity, e);
  }
  const byTo = new Map();
  for (const e of moved.values()) { if (!byTo.has(e.to)) byTo.set(e.to, []); byTo.get(e.to).push(e.name); }
  return [...byTo.entries()].map(([to, names]) => {
    const shown = nameList(names);
    const many = names.length > 1;
    const verb = TIER_ORDER.indexOf(to) === 3 ? (many ? " become " : " becomes ") : (many ? " go back to " : " goes back to ");
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
    || document.getElementById("keysDlg").open
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
    node.aliases && node.aliases.length ? html`<div class="meta">aliases: ${node.aliases.join(", ")}</div>` : "",
    origin, workspace,
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
        relatedDrawn(node).length ? html`<button type="button" class="linkish shrel">Show related</button>` : ""}</div>`,
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

// --- Minimap (map-tool convention): whole-graph overview + viewport rectangle -------------------
// Drawn by the rAF loop each frame: the miniature is kept as a bitmap of its own, painted again only
// when the nodes, their positions or the look change, and each frame draws that bitmap and the
// window's frame over it (Section 26.6). The world->minimap transform fits the VISIBLE node bounds
// (typeOff respected, same set fitView uses), so the overview always matches what the main canvas
// can show. Click/drag pans the camera (scale unchanged).
const miniEl = document.getElementById("minimap"), mctx = miniEl.getContext("2d");
const MINI_W = 168, MINI_H = 154, MINI_PAD = 10;   // height matches the HUD column (see viewer.css)
let miniT = { k: 1, ox: 0, oy: 0 };   // world -> minimap: m = w * k + o (kept for the pan handler)
const miniStill = { c: null, x: null, look: "", nodes: null, hyper: null, gen: -1, src: [], dirty: true, at: 0, k: 1, ox: 0, oy: 0 };
// While the layout moves, the miniature is painted at most this often - an overview needs no more -
// and at once when it comes to rest (design-system.md Section 27.5).
const MINI_MOVING_MS = 250;
// The miniature keeps nodes as the far map does (design-system.md Section 27.8, U38): about one whole
// for each MINI_KEEP_PX by MINI_KEEP_PX of its pixels, best-connected first, the next as many fading;
// and only the regions at least MINI_REGION_PX wide, the MINI_REGIONS widest. Every node a dot covered
// the next at 10,000 into one disc, and every region of 50,000 took 36 ms of the 81 its paint took.
const MINI_KEEP_PX = 5, MINI_REGION_PX = 12, MINI_REGIONS = 48;
// A relation's width in the miniature, and its largest dot.
const MINI_EDGE_W = 0.6, MINI_DOT_MAX = 1.8;
// A color as its RGB, read once from a pixel a canvas paints with it.
const rgbOf = new Map();
function cssRgb(color) {
  let v = rgbOf.get(color);
  if (!v) {
    const c = cssRgb.ctx || (cssRgb.ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true }));
    c.clearRect(0, 0, 1, 1); c.fillStyle = color; c.fillRect(0, 0, 1, 1);
    const d = c.getImageData(0, 0, 1, 1).data;
    v = [d[0], d[1], d[2]];
    rgbOf.set(color, v);
  }
  return v;
}
// The miniature's dots, drawn into a pixel buffer here rather than through the canvas: ten thousand
// dots one call each kept the minimap's paint at 39 ms in WebKit, and copying a dot drawn once,
// from a canvas each, kept WebKit's renderer so busy its frames came twice a second (Section 27.5).
// Each dot is what a fill and its halo stroke drew - its color inside, the surface's ring around -
// laid over the dots before it in order, so a dense cluster still shows its dots apart.
const miniDots = { c: null, x: null, img: null, buf: null, w: 0, h: 0 };
function paintMiniDots(src, dpr, mx, my, weight) {
  const L = miniDots, w = MINI_W * dpr, h = MINI_H * dpr;
  if (L.w !== w || L.h !== h) {
    L.c = document.createElement("canvas"); L.c.width = w; L.c.height = h; L.x = L.c.getContext("2d");
    L.img = L.x.createImageData(w, h); L.buf = new Float32Array(w * h * 4); L.w = w; L.h = h;
  }
  const buf = L.buf, halo = cssRgb(SURFACE);
  buf.fill(0);
  for (const n of src) {
    const mr = Math.min(MINI_DOT_MAX, 0.9 + Math.sqrt(n.degree || 0) * 0.25);
    const lw = Math.min(1.4, Math.max(mr * NODE_STROKE_RATIO, 0.6)) * ease(HALO_FROM, HALO_TO, mr), fill = cssRgb(typeColor[n.type] || OTHER);
    const cx = mx(n) * dpr, cy = my(n) * dpr, rOut = (mr + lw / 2) * dpr, rIn = (mr - lw / 2) * dpr, o = weight(n);
    const x0 = Math.max(0, Math.floor(cx - rOut - 1)), x1 = Math.min(w - 1, Math.ceil(cx + rOut + 1));
    const y0 = Math.max(0, Math.floor(cy - rOut - 1)), y1 = Math.min(h - 1, Math.ceil(cy + rOut + 1));
    for (let y = y0; y <= y1; y++) {
      const dy = y + 0.5 - cy;
      for (let x = x0; x <= x1; x++) {
        const dx = x + 0.5 - cx, d = Math.sqrt(dx * dx + dy * dy);
        const aOut = Math.min(1, Math.max(0, rOut - d + 0.5)) * o;
        if (aOut <= 0) continue;
        const aIn = Math.min(1, Math.max(0, rIn - d + 0.5)) * o, ring = aOut - aIn, keep = 1 - aOut, i = (y * w + x) * 4;
        buf[i] = fill[0] * aIn + halo[0] * ring + buf[i] * keep;
        buf[i + 1] = fill[1] * aIn + halo[1] * ring + buf[i + 1] * keep;
        buf[i + 2] = fill[2] * aIn + halo[2] * ring + buf[i + 2] * keep;
        buf[i + 3] = aOut + buf[i + 3] * keep;
      }
    }
  }
  const d = L.img.data;
  for (let i = 0; i < buf.length; i += 4) {
    const a = buf[i + 3];
    if (a <= 0) { d[i + 3] = 0; continue; }
    d[i] = buf[i] / a; d[i + 1] = buf[i + 1] / a; d[i + 2] = buf[i + 2] / a; d[i + 3] = a * 255;
  }
  L.x.putImageData(L.img, 0, 0);
  return L.c;
}
let miniShown = false;
function miniWanted(src) {
  const box = miniStill.box || (miniStill.box = boundsOf(src));
  if (!box) return false;
  // Grown by the largest node's radius, so a single entity or a line of them has an area.
  const grow = miniStill.grow ?? (miniStill.grow = src.reduce((m, n) => Math.max(m, nodeRadius(n)), 0));
  const a = box.a - grow, b = box.b - grow, c = box.c + grow, d = box.d + grow;
  const w = mapWindow();
  const wa = (w.l - cam.x) / cam.s, wb = (w.t - cam.y) / cam.s, wc = (w.r - cam.x) / cam.s, wd = (w.b - cam.y) / cam.s;
  const ix = Math.max(0, Math.min(c, wc) - Math.max(a, wa)), iy = Math.max(0, Math.min(d, wd) - Math.max(b, wb));
  const held = (ix * iy) / Math.max(1e-6, (c - a) * (d - b));
  miniShown = miniShown ? held <= 0.9 : held < 0.8;
  return miniShown;
}
function drawMinimap() {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const look = [dpr, hyperMode, showEdges, LIGHT.matches, FORCED_COLORS.matches, SURFACE, EDGE_OTHER, [...typeOff].join(","),
    edges.length, hyperedges.length, Object.keys(typeColor).map(t => t + ":" + typeColor[t]).join(",")].join("|");
  const m = miniStill;
  // What the miniature shows changed - its look, the graph, its contexts or a position (positionsGen,
  // Section 27.7) - so it is owed a paint; otherwise a frame neither filters the nodes nor bounds them again.
  if (m.look !== look || m.nodes !== nodes || m.hyper !== hyperedges || m.gen !== positionsGen) {
    m.look = look; m.nodes = nodes; m.hyper = hyperedges; m.gen = positionsGen; m.dirty = true;
    m.box = null; m.grow = null;   // the drawn map's box, read again for the minimap's rule
    m.src = nodes.filter(n => !typeOff.has(n.type));
  }
  const src = m.src;
  const on = showMini && src.length > 0;
  miniEl.classList.toggle("on", on);
  // The minimap draws while zoomed in (Section 29.12, R2-14): while the window holds less than 80
  // percent of the drawn map's box by area, until it holds more than 90 - and only once painted.
  miniEl.classList.toggle("want", on && !!miniStill.c && miniWanted(src));
  if (!on) return;
  if (miniEl.width !== MINI_W * dpr) { miniEl.width = MINI_W * dpr; miniEl.height = MINI_H * dpr; }
  // Everything renders inside the panel's rounded outline, read from viewer.css, so the clip and
  // the window's corners stay concentric with it whatever radius the token gives.
  const R = miniRadius();
  const round = mctx.roundRect !== undefined;
  // The first miniature is painted at once: a small map can show within MINI_MOVING_MS of the page's
  // start, and the frame would have no miniature to draw.
  if (m.dirty && (!m.c || !simWarm() || performance.now() - m.at >= MINI_MOVING_MS)) {
    let a = 1e9, b = 1e9, c = -1e9, d = -1e9;
    for (const n of src) { a = Math.min(a,n.x); b = Math.min(b,n.y); c = Math.max(c,n.x); d = Math.max(d,n.y); }
    const gw = Math.max(1, c-a), gh = Math.max(1, d-b);
    const k = Math.min((MINI_W - MINI_PAD*2) / gw, (MINI_H - MINI_PAD*2) / gh);
    const ox = (MINI_W - gw*k)/2 - a*k, oy = (MINI_H - gh*k)/2 - b*k;
    paintMiniature(dpr, k, ox, oy, R, round);
    m.dirty = false; m.at = performance.now(); m.k = k; m.ox = ox; m.oy = oy;
  } else if (m.dirty) requestFrame();   // still owed: painted when the layout rests or the wait ends
  // The window's frame and a click on the miniature go by the transform it was painted with.
  miniT = { k: m.k, ox: m.ox, oy: m.oy };
  mctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  mctx.clearRect(0, 0, MINI_W, MINI_H);
  mctx.drawImage(miniStill.c, 0, 0, MINI_W, MINI_H);
  mctx.save();
  if (round) {
    mctx.beginPath();
    mctx.roundRect(0, 0, MINI_W, MINI_H, R);
    mctx.clip();
  }
  drawMiniWindow(m.k, m.ox, m.oy, R, round);
  mctx.restore();
}
// The panel's corner radius, as viewer.css draws it (the --r-surface token), read once.
let miniR = null;
function miniRadius() {
  if (miniR === null) miniR = parseFloat(window.getComputedStyle(miniEl).borderTopLeftRadius) || 0;
  return miniR;
}
// The miniature's groups (Section 27.8, U41): the nodes in its drawn types taken best-connected first,
// each one not yet taken starting a group that takes every node not yet taken within MINI_REACH of the
// minimap's pixels, found through a grid of cells that wide. With each group its hub, its members'
// mean and the radius they would fill evenly, the type most of them share, and the relations between
// two groups as one pair with their number.
// The miniature into its own bitmap, keeping nodes as the far map does (Section 27.8, U38): the widest
// regions, the relations between nodes it keeps, then their dots sized by degree, inside the panel's
// rounded outline.
function paintMiniature(dpr, k, ox, oy, R, round) {
  const m = miniStill;
  if (!m.c) { m.c = document.createElement("canvas"); m.x = m.c.getContext("2d"); }
  if (m.c.width !== MINI_W * dpr) { m.c.width = MINI_W * dpr; m.c.height = MINI_H * dpr; }
  const mctx = m.x;
  mctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  mctx.clearRect(0, 0, MINI_W, MINI_H);
  mctx.save();
  if (round) {
    mctx.beginPath();
    mctx.roundRect(0, 0, MINI_W, MINI_H, R);
    mctx.clip();
  }
  const mx = n => n.x * k + ox, my = n => n.y * k + oy;
  // A node's weight in the miniature: whole among the first it keeps by rank, fading past them.
  const r = ranksOf(), keep = keptAt(r, k, MINI_KEEP_PX), weight = n => keptWeight(r.of[n._k], keep);
  // Regions (hyperedge overlay) - the graph's own clouds (hullGeom + paintCloud), scaled into the
  // minimap via a world->minimap transform, so a region looks the same in both. Only while the
  // overlay is on, and only the widest: a region narrower than MINI_REGION_PX is a smudge at this
  // size, and its members' box is all a region costs to find that out.
  if (hyperMode && hyperedges.length) {
    const rs = regionSimOf(), wide = [];
    for (const h of rs.drawn) {
      let a = Infinity, b = Infinity, c = -Infinity, d = -Infinity, n = 0;
      for (let j = rs.start[h]; j < rs.start[h + 1]; j++) {
        const v = nodes[rs.mem[j]];
        if (typeOff.has(v.type)) continue;
        n++; a = Math.min(a, v.x); b = Math.min(b, v.y); c = Math.max(c, v.x); d = Math.max(d, v.y);
      }
      const span = Math.max(c - a, d - b) * k;
      if (n >= 3 && span >= MINI_REGION_PX) wide.push([span, h]);
    }
    wide.sort((p, q) => q[0] - p[0] || p[1] - q[1]);
    mctx.save();
    mctx.transform(k, 0, 0, k, ox, oy);   // compose world->minimap on top of the DPR transform
    for (const [, h] of wide.slice(0, MINI_REGIONS)) {
      const ms = [];
      for (let j = rs.start[h]; j < rs.start[h + 1]; j++) { const v = nodes[rs.mem[j]]; if (!typeOff.has(v.type)) ms.push(v); }
      const geo = hullGeom(ms);
      if (geo) paintCloud(mctx, geo, regionRgb(hyperedges[rs.hid[h]].id), 0.16);
    }
    mctx.restore();
  }
  // Relations - thin and neutral (structure, not type), drawn while both their ends are and fading
  // with the fainter: one path for each opacity.
  if (showEdges) {
    const paths = new Map();
    for (const e of edges) {
      const a = Math.min(weight(e.a), weight(e.b));
      if (a === 0) continue;
      let path = paths.get(a);
      if (!path) { path = new Path2D(); paths.set(a, path); }
      path.moveTo(mx(e.a), my(e.a)); path.lineTo(mx(e.b), my(e.b));
    }
    mctx.strokeStyle = EDGE_OTHER; mctx.lineWidth = MINI_EDGE_W;
    for (const [a, path] of paths) { mctx.globalAlpha = 0.5 * a; mctx.stroke(path); }
    mctx.globalAlpha = 1;
  }
  // Nodes - filled circles whose radius grows with degree (a hub reads bigger), clamped for the
  // minimap, the best-connected drawn last. Each carries the same background-color halo the main
  // graph gives its nodes (NODE_STROKE_RATIO), grown in by its size as the map's is (Section 26.5), so
  // two dots that meet stay apart.
  const dots = [];
  for (let j = Math.min(r.count, Math.ceil(2 * keep)) - 1; j >= 0; j--) dots.push(nodes[r.order[j]]);
  mctx.drawImage(paintMiniDots(dots, dpr, mx, my, weight), 0, 0, MINI_W, MINI_H);
  mctx.restore();
}
function drawMiniWindow(k, ox, oy, R, round) {
  // Viewport rectangle: the map window's corners in world coords, mapped in and clamped to the frame.
  // Its outline is the window's rectangle cut by the panel's rounded outline, half a stroke inside
  // it: a corner that falls past the panel's curve follows that curve, concentric with the panel's,
  // so the line keeps its width round the corner; a free corner keeps a small radius.
  const vw = mapWindow();
  const x1 = Math.max(0.5, Math.min(MINI_W - 0.5, ((vw.l - cam.x) / cam.s) * k + ox));
  const y1 = Math.max(0.5, Math.min(MINI_H - 0.5, ((vw.t - cam.y) / cam.s) * k + oy));
  const x2 = Math.max(0.5, Math.min(MINI_W - 0.5, ((vw.r - cam.x) / cam.s) * k + ox));
  const y2 = Math.max(0.5, Math.min(MINI_H - 0.5, ((vw.b - cam.y) / cam.s) * k + oy));
  // Ink, not gold: with the whole graph in view the rect lies on the minimap's edge, and a gold frame
  // is the decoration gold is not for (docs/design-system.md Section 4).
  mctx.strokeStyle = INK; mctx.globalAlpha = 0.55; mctx.lineWidth = 1;
  if (!round) { mctx.strokeRect(x1, y1, Math.max(2, x2 - x1), Math.max(2, y2 - y1)); return; }
  mctx.beginPath();
  miniWindowPath(mctx, x1, y1, Math.max(x1 + 2, x2), Math.max(y1 + 2, y2), R - 0.5);
  mctx.stroke();
}
// The outline of a rectangle cut by the rounded rectangle inset half a pixel in the panel, whose
// corner radius is `r`. Each corner of the rectangle that lies past the rounded rectangle's curve is
// replaced by the arc of that curve between the two edges' crossings; any other corner is rounded by
// at most 3 pixels. Traced clockwise from the top-left corner's end on the top edge.
function miniWindowPath(c, x1, y1, x2, y2, r) {
  const lo = 0.5, hiX = MINI_W - 0.5, hiY = MINI_H - 0.5;
  const small = Math.min(3, (x2 - x1) / 2, (y2 - y1) / 2);
  // For each corner: the point it would be, the curve's centre, and the side of the centre it lies on.
  const corners = [
    { x: x1, y: y1, cx: lo + r, cy: lo + r, sx: -1, sy: -1 },     // top-left
    { x: x2, y: y1, cx: hiX - r, cy: lo + r, sx: 1, sy: -1 },     // top-right
    { x: x2, y: y2, cx: hiX - r, cy: hiY - r, sx: 1, sy: 1 },     // bottom-right
    { x: x1, y: y2, cx: lo + r, cy: hiY - r, sx: -1, sy: 1 },     // bottom-left
  ];
  const pts = corners.map(q => {
    // A corner is cut when it lies past the curve's centre on both axes and outside its circle.
    const dx = (q.x - q.cx) * q.sx, dy = (q.y - q.cy) * q.sy;
    if (r <= 0 || dx <= 0 || dy <= 0 || Math.hypot(dx, dy) <= r || dx > r || dy > r) return null;
    // Where the corner's horizontal edge and its vertical edge cross the circle.
    const hx = q.cx + q.sx * Math.sqrt(r * r - dy * dy);   // on the horizontal edge
    const vy = q.cy + q.sy * Math.sqrt(r * r - dx * dx);   // on the vertical edge
    // The crossings must lie on the rectangle's own edges, or the curve is not this corner's.
    if (hx < x1 || hx > x2 || vy < y1 || vy > y2) return null;
    return { hx, vy, a: Math.atan2(q.y - q.cy, hx - q.cx), b: Math.atan2(vy - q.cy, q.x - q.cx) };
  });
  // Top-left: from its vertical crossing round to its horizontal one, or a small round corner.
  const [tl, tr, brc, bl] = pts;
  if (tl) { c.moveTo(x1, tl.vy); c.arc(corners[0].cx, corners[0].cy, r, tl.b, tl.a); }
  else { c.moveTo(x1, y1 + small); c.arcTo(x1, y1, x1 + small, y1, small); }
  if (tr) { c.lineTo(tr.hx, y1); c.arc(corners[1].cx, corners[1].cy, r, tr.a, tr.b); }
  else c.arcTo(x2, y1, x2, y1 + small, small);
  if (brc) { c.lineTo(x2, brc.vy); c.arc(corners[2].cx, corners[2].cy, r, brc.b, brc.a); }
  else c.arcTo(x2, y2, x2 - small, y2, small);
  if (bl) { c.lineTo(bl.hx, y2); c.arc(corners[3].cx, corners[3].cy, r, bl.a, bl.b); }
  else c.arcTo(x1, y2, x1, y2 - small, small);
  c.closePath();
}
// Click/drag pans: center the camera on the pointed world position, at the current zoom.
function miniPan(ev) {
  const r = miniEl.getBoundingClientRect();
  const wx = (ev.clientX - r.left - miniT.ox) / miniT.k;
  const wy = (ev.clientY - r.top - miniT.oy) / miniT.k;
  const w = mapWindow();   // centred in the map's window, not under a panel
  camT.x = (w.l + w.r)/2 - wx * camT.s;
  camT.y = (w.t + w.b)/2 - wy * camT.s;
  userMoved = true;
}
miniEl.addEventListener("mousedown", ev => {
  personMoved();
  ev.preventDefault();
  ev.stopPropagation();
  miniPan(ev);
  const move = e2 => miniPan(e2);
  const up = () => { removeEventListener("mousemove", move); removeEventListener("mouseup", up); };
  addEventListener("mousemove", move);
  addEventListener("mouseup", up);
});

// Set the target camera so the given node set fits on screen (smoothly, via easing). In CSS pixels.
// Fit is against the FULL window width, regardless of which side panels float open - the panels
// are overlays, and on a narrow window fitting into the strip between them shrank the graph past
// readability (the map-app convention: fit the viewport, let overlays cover the edges). Only the
// top/bottom insets stay (header/status are opaque bars, not floating cards). The focus framings
// (centerOn, focusView) do avoid the panels - what a focus puts on screen is exactly what the open
// inspector is describing, so hiding it under that inspector would be self-defeating. The difference
// is affordable there and not here: a neighbourhood is a handful of nodes, the whole graph is not.
function fitView(list, pad = 90) {
  const box = boundsOf(list || nodes);
  if (!box) return;
  // Under 760px a panel is a sheet over the bottom of the canvas: frame above it, with less margin.
  if (sheetWindow()) { frameBox(box, 0, innerWidth, TOP_INSET, innerHeight - insetB(), Math.min(pad, 36), 2.5); return; }
  // The panels sit beside the map, not over it (docs/design-system.md Section 6), so the frame is the
  // map's window between them. Framed to the whole canvas, as when the docks floated, a small graph
  // zoomed to the cap ran under both panels. A node's label hangs to its right, so the frame keeps
  // room for one there: the window's edge is hard now, and it cut the rightmost names.
  const w = mapWindow();
  frameBox(box, w.l, w.r - LABEL_ROOM, w.t, w.b, pad, 2.5);
}
const LABEL_ROOM = 110;   // screen pixels a right-hand node label needs beyond the frame's padding
// The map's window in screen coordinates: the canvas less the panels, the header and the status bar.
function mapWindow() {
  return { l: insetL(), r: innerWidth - insetR(), t: TOP_INSET, b: innerHeight - insetB() };
}

// World-space bounding box of the visible members of a node list. Types the filter hides are dropped:
// a node nobody can see must not drag the frame. Null when nothing is left to frame.
function boundsOf(list) {
  const src = list.filter(n => !typeOff.has(n.type));
  if (!src.length) return null;
  let a = 1e9, b = 1e9, c = -1e9, d = -1e9;
  for (const n of src) { a = Math.min(a,n.x); b = Math.min(b,n.y); c = Math.max(c,n.x); d = Math.max(d,n.y); }
  return { a, b, c, d };
}

// Aim the camera so a world box fits a screen rect, with `pad` breathing room and the zoom capped at
// `hi`. Shared so the two framings differ only in their rect and ceiling, not in their arithmetic.
function frameBox({ a, b, c, d }, L, R, T, B, pad, hi) {
  // Every framing leaves out the camera toolbar's box (Section 29.12): at the window's foot when it
  // sits there, and at its top, both rows, when it sits there (under 760 pixels).
  const hud = document.getElementById("hud");
  const hb = hud && hud.offsetParent !== null ? hud.getBoundingClientRect() : null;
  if (hb && hb.height) {
    if (hb.top > innerHeight / 2) B = Math.min(B, hb.top - 8);
    else T = Math.max(T, hb.bottom + 8);
  }
  const vw = Math.max(1, R - L - pad*2), vh = Math.max(1, B - T - pad*2);
  const gw = Math.max(1, c-a), gh = Math.max(1, d-b);
  camT.s = Math.max(SCALE_LEAST, Math.min(hi, Math.min(vw / gw, vh / gh)));
  camT.x = (L + R)/2 - (a+c)/2*camT.s;
  camT.y = (T + B)/2 - (b+d)/2*camT.s;
}

// The nodes focusing `n` reveals: the anchor plus everything one edge away. Mirrors activeSet's
// adjacency on purpose - the camera should frame exactly what the highlight lights up.
function neighbourhoodOf(n) {
  const ids = new Set([n.id]);
  for (const e of edges) {
    if (e.a.id === n.id) ids.add(e.b.id);
    else if (e.b.id === n.id) ids.add(e.a.id);
  }
  return nodes.filter(m => ids.has(m.id));
}

// Frame a focused node together with the neighbours its focus reveals. Focusing lights up the
// neighbours and the inspector lists them, so a camera that frames the anchor alone leaves the reader
// hunting off-screen for the very things it just highlighted.
//
// The zoom only ever goes DOWN from what centerOn would have chosen: that ceiling is what keeps a
// tight pair from jump-cutting to maximum zoom, so the felt behaviour is unchanged whenever the
// neighbourhood already fits, and the one thing that changes is that a neighbourhood which does not
// fit now pulls the camera out until it does. Unlike fitView this respects the side panels, because
// focusing is exactly when the inspector opens - fitting past it would hide the neighbours under the
// panel that is describing them.
function focusView(n) {
  const box = boundsOf(neighbourhoodOf(n));
  // An isolated node (or one whose neighbours are all filtered out) has a degenerate box - there is
  // no extent to fit, and zooming to a point is not framing. Centring is the honest answer.
  if (!box || (box.c - box.a < 1 && box.d - box.b < 1)) { centerOn(n); return; }
  const ceiling = Math.min(2.5, Math.max(cam.s, 1.1));   // what centerOn would have picked
  frameBox(box, insetL(), innerWidth - insetR(), TOP_INSET, innerHeight - insetB(), 70, ceiling);
  userMoved = true;
}

// hyperedge id -> palette color (deterministic hash). Overlapping hulls blend semi-transparently (C1: overlap = connective tissue).
// Size-scaled visual weight for a hyperedge (0 at min size 2, 1 at HULL_SIZE_REF+ members) is
// viewer-sim.js's hullSizeNorm, which the layout's cohesion uses too.
// Geometry of one hyperedge hull: the convex hull of the member centers, an outward expansion radius r
// (largest member glyph + gap), and the member centroid (for the label). Returns null when degenerate.
function hullGeom(ms) {
  let cx = 0, cy = 0;
  for (const m of ms) { cx += m.x; cy += m.y; }
  cx /= ms.length; cy /= ms.length;
  const pts = ms.map(m => ({ x: m.x, y: m.y })), hull = convexHull(pts);
  if (hull.length < 3) return null;
  return { hull, r: hullReach(ms), cx, cy, pts };
}
// How far a hull reaches past its members' centres: the widest node with its halo, and the gap. The
// halo keeps a width on the screen, so the reach follows the zoom.
function hullReach(ms) {
  let r = 0;
  for (const m of ms) { const g = nodeRadius(m) + nodeStrokeW(m) / 2; if (g > r) r = g; }
  return r + HULL_NODE_GAP;
}
// The regions as the map draws them - each one's members in the drawn types, its hull and its cloud's
// shape - kept while every node stands where it stood and the regions and the filters are the same
// (design-system.md Section 26.6, U32). Worked out every frame, a large map's regions made thousands
// of small objects a frame, and the collector's pauses were a zoom's slowest frames.
const regionGeo = { nodes: null, hyper: null, filt: "", gen: -1, list: [] };
function regionGeometry() {
  const r = regionGeo, filt = [...typeOff].join(",");
  // The nodes stand where they stood while no position has changed (positionsGen, Section 27.7).
  if (r.nodes === nodes && r.hyper === hyperedges && r.filt === filt && r.gen === positionsGen) return r.list;
  r.nodes = nodes; r.hyper = hyperedges; r.filt = filt; r.gen = positionsGen; r.list = [];
  const nb = nodesById();
  for (const h of hyperedges) {
    const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
    if (ms.length < 3) continue;
    const g = hullGeom(ms);
    if (g) r.list.push({ h, id: h.id, ms, g, shape: cloudShape(g), hue: regionHue(h.id), hot: false, lgHit: false, alpha: 0 });
  }
  return r.list;
}
// Trace the outward-offset rounded hull as a SINGLE closed path: each edge is pushed out by r (past the
// node glyphs) and consecutive edges are joined by a corner arc sampled into short segments. Filling this
// one path gives the same rounded blob a thick round-join stroke would - but with no stroke, so there is
// no fill/stroke overlap (no seam) and no offscreen compositing is needed; the caller just fills it once
// at the hull's opacity, directly on the canvas, and overlapping hulls blend naturally.
// The workspace outlines. Drawn once per workspace present, and only when more than one is - a
// single boundary around the whole graph states nothing the view does not already say.
//
// Groups of one or two nodes are skipped for the same reason `hullGeom` needs three: a hull around
// two points is a line, which reads as an edge rather than a region.
// The groups a Boundaries choice outlines, over the drawn entities - those whose type is shown
// (Section 29.12). One predicate, two or more groups, decides the outline, the choice's state, b's
// cycle and the map key's row.
function boundaryGroups(mode) {
  const groups = new Map();
  const push = (key, n) => {
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(n);
  };
  if (mode === "workspace") {
    // At most one key per node. Empty means the projection found attestations naming more than one
    // workspace (a merge folded across the boundary), and such a node joins no region.
    for (const n of nodes) if (!typeOff.has(n.type) && n.workspace) push(n.workspace, n);
  } else if (mode === "origin") {
    // Several keys per node, on purpose. Attestations union across nodes (F4), so an entity both
    // sides asserted belongs to both regions and the outlines overlap - which is the fact worth
    // seeing, not a defect to design away. Labelled by id prefix: `origins` is the self-declared
    // host and pairing the two would need a stamp-level join the graph does not carry, so a hex
    // prefix is the honest name here (P2 - the verifiable id is the one that may attribute).
    for (const n of nodes) if (!typeOff.has(n.type)) for (const o of (n.origin_nodes || [])) push(o, n);
  }
  return groups;
}
function outlines(mode) { return mode !== "off" && boundaryGroups(mode).size >= 2; }
function drawWsHulls(ctx, hullLabels) {
  if (boundaryMode === "off") return;
  const groups = boundaryGroups(boundaryMode);
  if (groups.size < 2) return;
  ctx.save();
  ctx.lineWidth = 1;
  // Sorted, so the colour a workspace gets does not depend on map iteration order (P16's
  // reproducibility, applied to the render: the same state must draw the same picture).
  for (const ws of [...groups.keys()].sort()) {
    const ms = groups.get(ws);
    const col = hyperColor(boundaryMode + ":" + ws);
    ctx.strokeStyle = col;
    ctx.globalAlpha = 0.55;
    let g;
    if (ms.length >= 3) {
      g = hullGeom(ms);
      if (!g) continue;
      roundedHullPath(ctx, g);
      ctx.stroke();
    } else {
      // One or two nodes cannot have a convex hull - two points are a line, which would read as an
      // edge. They still get a boundary, because drawing some workspaces and silently omitting
      // others says the omitted nodes belong to none. A rounded box around their extent carries the
      // same "same sharing unit" claim without pretending to be a hull.
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity, pad = 0;
      for (const m of ms) {
        x0 = Math.min(x0, m.x); y0 = Math.min(y0, m.y);
        x1 = Math.max(x1, m.x); y1 = Math.max(y1, m.y);
        pad = Math.max(pad, nodeRadius(m) + nodeStrokeW(m) / 2);
      }
      pad += HULL_NODE_GAP;
      const rr = Math.min(14, pad);
      ctx.beginPath();
      ctx.roundRect(x0 - pad, y0 - pad, (x1 - x0) + pad * 2, (y1 - y0) + pad * 2, rr);
      ctx.stroke();
      g = { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, r: pad, hull: [{ x: (x0 + x1) / 2, y: y0 - pad }] };
    }
    // Above the outline's highest point, not above the centroid: `hullGeom.r` is the node-glyph gap,
    // so a centroid offset would land the name inside the region on top of its own nodes. `size` is
    // held at 1 because the label renderer scales the font by it, and a workspace is not a context
    // whose importance grows with membership - it is a boundary, and boundaries are named quietly.
    const top = g.hull.reduce((a, q) => (q.y < a.y ? q : a), g.hull[0]);
    const label = boundaryMode === "origin" ? ws.slice(0, 8) : ws;
    hullLabels.push({ cx: top.x, cy: top.y - g.r - 10, text: label, col, hot: false, lgHit: false, size: 1 });
  }
  ctx.restore();
  ctx.globalAlpha = 1;
}

// The boundary-mode chips, in the same language as the type legends above them: a chip per mode,
// the inactive ones dimmed with `.off`, click to switch.
//
// A keyboard-only toggle was the first version and it was the wrong call. The dock already carries
// every other view switch, so a mode that lives only behind "b" reads as absent - the first question
// it drew was "why is it grouping by workspace", which is what a control with no affordance earns.
// "b" still cycles, for whoever finds it.
const BOUNDARY_MODES = [
  ["workspace", "A workspace outline around each workspace: a sharing unit, and no entity is in two", "Workspace"],
  ["origin", "Who signed for each entity; outlines overlap where two machines both did", "Origin"],
  ["off", "The workspaces keep together, without outlines", "None"],
];
// Why a choice outlines nothing here (Section 29.12, P5: what is counted, not what is assumed).
const BOUNDARY_NOTHING = {
  workspace: "One workspace here - nothing to outline",
  origin: "Fewer than two machines are recorded here - nothing to outline",
};
function renderBoundaryChips() {
  renderMarks();
  if (!legendBoundaryEl) return;
  legendBoundaryEl.innerHTML = "";
  for (const [mode, why, label] of BOUNDARY_MODES) {
    const el = document.createElement("button");
    el.type = "button";
    el.className = boundaryMode === mode ? "on" : "";
    const sw = document.createElement("span");
    sw.className = "sw";
    // A thin outline for the two that draw, hollow for off - the same outline the layer uses.
    if (mode === "off") { sw.style.background = "transparent"; sw.style.border = "1px solid #666"; }
    else { sw.style.background = "transparent"; sw.style.border = "1px solid " + hyperColor(mode + ":swatch"); }
    el.appendChild(sw);
    el.appendChild(document.createTextNode(label));
    el.title = tipText(label, singleKeys ? "b" : null);
    // A choice that outlines nothing here is dimmed and says why; the chosen one stays chosen.
    if (mode !== "off" && !outlines(mode)) el.setAttribute("aria-disabled", "true");
    // Boundaries only draws: the layout does not move (Section 29.12).
    el.onclick = () => { if (el.getAttribute("aria-disabled") === "true" && boundaryMode !== mode) return; boundaryMode = mode; renderBoundaryChips(); saveViewPrefs(); requestFrame(); };
    legendBoundaryEl.appendChild(el);
  }
  radioGroup(legendBoundaryEl, "Boundaries");   // one choice of three (Section 4)
  // What the chosen boundaries draw, said under the choice (Section 4, U25).
  const desc = document.getElementById("boundaryDesc");
  const chosen = BOUNDARY_MODES.find(([mode]) => mode === boundaryMode);
  if (desc && chosen) desc.textContent = boundaryMode !== "off" && !outlines(boundaryMode) ? BOUNDARY_NOTHING[boundaryMode] + "." : chosen[1] + ".";
}
// b moves to the next choice that draws, or None; with nothing to outline it says so and changes
// nothing. Its notice replaces the last one (U42).
let boundaryNote = null;
function cycleBoundaries() {
  const order = ["workspace", "origin", "off"];
  const usable = order.filter(m => m === "off" || outlines(m));
  let said;
  if (usable.length === 1) said = "Boundaries: nothing to outline here";
  else {
    const at = order.indexOf(boundaryMode);
    for (let k = 1; k <= order.length; k++) {
      const m = order[(at + k) % order.length];
      if (usable.includes(m)) { boundaryMode = m; break; }
    }
    said = "Boundaries: " + BOUNDARY_MODES.find(([m]) => m === boundaryMode)[2];
    renderBoundaryChips(); saveViewPrefs(); requestFrame();
  }
  if (boundaryNote) dismiss(boundaryNote);
  boundaryNote = toast("ok", said);
}

// --- Region clouds, mixed in OKLCH (docs/design-system.md Section 5) ------------------------------
// Each region's hue comes from its context's id; every region shares one lightness and one chroma in
// OKLCH, so no region reads brighter or heavier than another because of its hue (the HSL band gave
// some 50% lightness and others 66%). Where regions overlap they are mixed in OKLCH as well, not by
// the canvas: its compositing mixes in sRGB, where two clouds of different hues meet in a grey or a
// brown neither of them is. The field below does the mixing itself, at a quarter of the screen's
// resolution - a cloud has no detail to lose - and is drawn smoothed up to the window.
const CLOUD_L = 0.70, CLOUD_C = 0.12;   // the clouds' OKLCH lightness and chroma
const LABEL_L = 0.82, LABEL_C = 0.10;   // a region's name, the same hue lighter, to read on the ground
// On a light ground a cloud is a paler wash and a region's name the same hue darker (U27).
const CLOUD_L_LIGHT = 0.82, CLOUD_C_LIGHT = 0.09, LABEL_L_LIGHT = 0.45, LABEL_C_LIGHT = 0.12;
function cloudL() { return LIGHT.matches ? CLOUD_L_LIGHT : CLOUD_L; }
function cloudC() { return LIGHT.matches ? CLOUD_C_LIGHT : CLOUD_C; }
const CLOUD_CELL = 4;                   // CSS px per cell of the mixing field
// A cloud wider than this share of the window is no longer a region you see but the ground you read
// on (zoomed in, one region spans the window), so it fades by how much wider it is.
const CLOUD_FADE_AT = 0.45;
// However many clouds overlap, their coverage eases toward this ceiling instead of stacking toward
// opaque: ten regions sharing a cluster's core had covered it at 0.65.
const CLOUD_COVER_MAX = 0.42;
// A region's hue lies on the band's arcs - 109 to 155 degrees and 205 around to 11 - so no cloud is
// gold, coral or teal (Section 5): the golden angle over 512 slots, laid along those 212 degrees.
function regionHue(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  const t = ((h % 512) * 137.508) % 212;
  return t < 46 ? 109 + t : (205 + t - 46) % 360;
}
function regionLabelColor(id) {
  if (FORCED_COLORS.matches) return INK;
  const l = LIGHT.matches ? LABEL_L_LIGHT : LABEL_L, c = LIGHT.matches ? LABEL_C_LIGHT : LABEL_C;
  return "oklch(" + l + " " + c + " " + regionHue(id).toFixed(1) + ")";
}
// OKLab -> sRGB in 0..255 (Ottosson's matrices), clamped into the gamut. The gamma step is a table.
const SRGB_LUT = (() => {
  const t = new Uint8ClampedArray(4097);
  for (let i = 0; i <= 4096; i++) { const v = i / 4096; t[i] = Math.round(255 * (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.pow(v, 1 / 2.4) - 0.055)); }
  return t;
})();
function oklabToSrgb(L, a, b, out) {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3;
  const g = v => SRGB_LUT[Math.round(Math.min(1, Math.max(0, v)) * 4096)];
  out[0] = g(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s);
  out[1] = g(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s);
  out[2] = g(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
  return out;
}
function regionRgb(id) {
  const h = regionHue(id) * Math.PI / 180;
  return oklabToSrgb(cloudL(), cloudC() * Math.cos(h), cloudC() * Math.sin(h), [0, 0, 0]);
}

// A region's ellipse. It lies along the members' principal axis (their covariance), each radius the
// farthest member along it plus CLOUD_REACH times the hull's gap.
function cloudShape(g) {
  const ms = g.pts;
  let sxx = 0, syy = 0, sxy = 0;
  for (const m of ms) { const dx = m.x - g.cx, dy = m.y - g.cy; sxx += dx * dx; syy += dy * dy; sxy += dx * dy; }
  const th = 0.5 * Math.atan2(2 * sxy, sxx - syy), ux = Math.cos(th), uy = Math.sin(th);
  let ru = 0, rv = 0;
  for (const m of ms) {
    const dx = m.x - g.cx, dy = m.y - g.cy;
    ru = Math.max(ru, Math.abs(dx * ux + dy * uy)); rv = Math.max(rv, Math.abs(-dx * uy + dy * ux));
  }
  const pad = g.r * CLOUD_REACH, R = ru + pad;
  return { cx: g.cx, cy: g.cy, th, R, Rv: Math.max(rv + pad, R * 0.35), ru, rv };
}
// The density across a cloud, by distance from its centre as a fraction of its radius: most of it
// stays over the members before it fades out, so a member at the far end of a long region still sits
// in about a third of it.
function cloudFalloff(r) {
  return r < 0.45 ? 1 - 0.2 * r / 0.45 : r < 0.75 ? 0.8 - 0.42 * (r - 0.45) / 0.3 : 0.38 * (1 - r) / 0.25;
}
// One region as a cloud on its own (the minimap), composited by the canvas.
function paintCloud(c, g, rgb, alpha) {
  const sh = cloudShape(g), col = a => "rgba(" + rgb[0] + "," + rgb[1] + "," + rgb[2] + "," + a + ")";
  c.save();
  c.translate(sh.cx, sh.cy); c.rotate(sh.th); c.scale(1, sh.Rv / sh.R);
  const grad = c.createRadialGradient(0, 0, 0, 0, 0, sh.R);
  grad.addColorStop(0, col(1)); grad.addColorStop(0.45, col(0.8)); grad.addColorStop(0.75, col(0.38)); grad.addColorStop(1, col(0));
  c.globalAlpha = alpha; c.fillStyle = grad;
  c.beginPath(); c.arc(0, 0, sh.R, 0, 2 * Math.PI); c.fill();
  c.restore();
}
// Every region of the frame, mixed in OKLCH and drawn in one image. Coverage builds as layered paint
// would (1 - the product of what each cloud leaves uncovered); the colour is the clouds' weighted mix.
// All share one lightness and chroma, so only the hue mixes: by the clouds' weighted circular mean,
// with the chroma eased only where the hues oppose (two opposite hues have no mean to keep).
//
// The field is kept between frames with the camera it was worked out for (Section 26.6). While the
// regions and the look are unchanged and only the camera moves, the kept image is drawn moved and
// scaled by the camera's change; it is worked out again once the camera has rested CLOUD_REST_MS,
// when the moved image no longer covers the window, or when it would be scaled past half or double.
// While the layout is still, the field is worked out CLOUD_MARGIN beyond each edge of the window, so a
// pan has field to move into; while it steps, every frame works it out anyway, so no margin is paid.
const CLOUD_MARGIN = 0.25;              // of the window's longer side, beyond each edge
const CLOUD_REST_MS = 150;
// The field's cell grows with its work. A cloud costs a sum for every cell its box covers, so zoomed
// into a large map, where hundreds of clouds each span the window, 4-pixel cells came to tens of
// millions of sums; the cell is the smallest of 4, 8, 16 and 32 pixels that keeps the work - the
// clouds' cells and the field's own - within CLOUD_WORK. A cloud that wide on the screen has no detail
// a coarser cell loses, and the image is smoothed up to the window either way.
const CLOUD_CELL_MAX = 32, CLOUD_WORK = 3e6;
const cloudField = {
  c: null, x: null, img: null, w: 0, h: 0, W: null, A: null, B: null, P: null,
  key: null, look: "", any: false, s: 1, cx: 0, cy: 0, m: 0, cell: CLOUD_CELL, due: false, timer: 0,
};
// Whether the regions are the ones the kept field was worked out from: each one's centre, axis and
// members' reach along it, hue and strength, compared in place and left as the new key. The padding
// past the members is left out: it holds the nodes' halos, kept to a width on the screen, so it
// changes with the zoom, and the camera's change is the one the kept field is moved by.
function cloudKeySame(f, items) {
  const n = items.length * 7;
  let same = f.key !== null && f.key.length === n;
  if (!same) f.key = new Float64Array(n);
  const k = f.key;
  for (let i = 0, o = 0; i < items.length; i++, o += 7) {
    const it = items[i], sh = it.shape;
    if (same && (k[o] !== sh.cx || k[o + 1] !== sh.cy || k[o + 2] !== sh.ru || k[o + 3] !== sh.rv
      || k[o + 4] !== sh.th || k[o + 5] !== it.alpha || k[o + 6] !== it.hue)) same = false;
    k[o] = sh.cx; k[o + 1] = sh.cy; k[o + 2] = sh.ru; k[o + 3] = sh.rv; k[o + 4] = sh.th; k[o + 5] = it.alpha; k[o + 6] = it.hue;
  }
  return same;
}
// The field's work at this camera, in square pixels: every cloud's box within the field, which reaches
// `margin` pixels past the window, and the field itself.
function cloudWork(items, margin) {
  const W = innerWidth + 2 * margin, H = innerHeight + 2 * margin;
  let work = W * H;
  for (const it of items) {
    if (it.alpha <= 0) continue;
    const sh = it.shape, cs = Math.cos(sh.th), sn = Math.sin(sh.th), Ru = sh.R * cam.s, Rv = sh.Rv * cam.s;
    const ex = Math.hypot(Ru * cs, Rv * sn), ey = Math.hypot(Ru * sn, Rv * cs);
    const px = sh.cx * cam.s + cam.x + margin, py = sh.cy * cam.s + cam.y + margin;
    const bw = Math.min(W, px + ex) - Math.max(0, px - ex), bh = Math.min(H, py + ey) - Math.max(0, py - ey);
    if (bw > 0 && bh > 0) work += bw * bh;
  }
  return work;
}
function cloudCellFor(items, margin) {
  const work = cloudWork(items, margin);
  let cell = CLOUD_CELL;
  while (cell < CLOUD_CELL_MAX && work / (cell * cell) > CLOUD_WORK) cell *= 2;
  return cell;
}
// The kept field on the screen, moved and scaled from the camera it was worked out for to this one.
function blitCloudField(f, k, ox, oy) {
  if (!f.any) return;
  const e = f.cell * k;
  ctx.save();
  ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  ctx.imageSmoothingEnabled = true; ctx.imageSmoothingQuality = "high";
  ctx.drawImage(f.c, -f.m * e + ox, -f.m * e + oy, f.w * e, f.h * e);
  ctx.restore();
}
function drawCloudField(items) {
  if (bulk !== ctx) { bulk.field(items, { L: cloudL(), C: cloudC(), coverMax: CLOUD_COVER_MAX, fadeAt: CLOUD_FADE_AT }); return; }
  const f = cloudField;
  const look = cloudL() + " " + cloudC() + " " + innerWidth + " " + innerHeight + " " + edges.length;
  const same = cloudKeySame(f, items) && f.look === look;
  if (same && !f.due) {
    const k = cam.s / f.s, ox = cam.x - f.cx * k, oy = cam.y - f.cy * k;
    if (k === 1 && ox === 0 && oy === 0) { blitCloudField(f, 1, 0, 0); return; }
    const e = f.cell * k, x0 = -f.m * e + ox, y0 = -f.m * e + oy;
    if (k > 0.5 && k < 2 && x0 <= 0 && y0 <= 0 && x0 + f.w * e >= innerWidth && y0 + f.h * e >= innerHeight) {
      blitCloudField(f, k, ox, oy);
      clearTimeout(f.timer);
      f.timer = setTimeout(() => { f.due = true; requestFrame(); }, CLOUD_REST_MS);
      return;
    }
  }
  clearTimeout(f.timer);
  f.due = false; f.look = look; f.s = cam.s; f.cx = cam.x; f.cy = cam.y;
  const margin = simWarm() ? 0 : CLOUD_MARGIN * Math.max(innerWidth, innerHeight);
  const cell = cloudCellFor(items, margin), m = Math.ceil(margin / cell);
  const w = Math.ceil(innerWidth / cell) + 2 * m, h = Math.ceil(innerHeight / cell) + 2 * m, n = w * h;
  if (f.w !== w || f.h !== h) {
    f.c = document.createElement("canvas"); f.c.width = w; f.c.height = h; f.x = f.c.getContext("2d");
    f.img = f.x.createImageData(w, h); f.w = w; f.h = h;
    f.W = new Float32Array(n); f.A = new Float32Array(n); f.B = new Float32Array(n); f.P = new Float32Array(n);
  }
  f.m = m; f.cell = cell;
  f.W.fill(0); f.A.fill(0); f.B.fill(0); f.P.fill(1);
  let any = false;
  for (const it of items) {
    const sh = it.shape, k = cam.s / cell;
    const px = (sh.cx * cam.s + cam.x) / cell + m, py = (sh.cy * cam.s + cam.y) / cell + m;
    const Ru = sh.R * k, Rv = sh.Rv * k, cs = Math.cos(sh.th), sn = Math.sin(sh.th);
    const across = Math.max(Ru, Rv) * cell, fit = CLOUD_FADE_AT * Math.min(innerWidth, innerHeight);
    const fade = across > fit ? Math.max(0.15, (fit / across) ** 1.5) : 1;
    const ex = Math.hypot(Ru * cs, Rv * sn), ey = Math.hypot(Ru * sn, Rv * cs);
    const x0 = Math.max(0, Math.floor(px - ex)), x1 = Math.min(w - 1, Math.ceil(px + ex));
    const y0 = Math.max(0, Math.floor(py - ey)), y1 = Math.min(h - 1, Math.ceil(py + ey));
    if (x0 > x1 || y0 > y1 || it.alpha <= 0) continue;
    const hue = it.hue * Math.PI / 180, ca = Math.cos(hue), cb = Math.sin(hue), al = it.alpha * fade;
    for (let y = y0; y <= y1; y++) {
      const dy = y + 0.5 - py;
      for (let x = x0; x <= x1; x++) {
        const dx = x + 0.5 - px, u = (dx * cs + dy * sn) / Ru, v = (dy * cs - dx * sn) / Rv, r2 = u * u + v * v;
        if (r2 >= 1) continue;
        const wt = al * cloudFalloff(Math.sqrt(r2)), i = y * w + x;
        f.W[i] += wt; f.A[i] += wt * ca; f.B[i] += wt * cb; f.P[i] *= 1 - wt;
        any = true;
      }
    }
  }
  f.any = any;
  if (!any) return;
  const d = f.img.data, rgb = [0, 0, 0];
  for (let i = 0; i < n; i++) {
    const wt = f.W[i], o = i * 4;
    if (wt <= 0) { d[o + 3] = 0; continue; }
    const vec = Math.hypot(f.A[i], f.B[i]) / wt;   // 1 where every hue agrees, toward 0 where they oppose
    const c = vec > 1e-6 ? cloudC() * Math.pow(Math.min(1, vec), 0.35) / (vec * wt) : 0;
    oklabToSrgb(cloudL(), f.A[i] * c, f.B[i] * c, rgb);
    const cover = CLOUD_COVER_MAX * (1 - Math.exp(-(1 - f.P[i]) / CLOUD_COVER_MAX));
    d[o] = rgb[0]; d[o + 1] = rgb[1]; d[o + 2] = rgb[2]; d[o + 3] = Math.round(255 * cover);
  }
  f.x.putImageData(f.img, 0, 0);
  blitCloudField(f, 1, 0, 0);
}

function roundedHullPath(c, g) {
  const hull = g.hull, n = hull.length, r = g.r, cx = g.cx, cy = g.cy;
  const nrm = [];   // outward unit normal per edge i (edge hull[i]->hull[i+1])
  for (let i = 0; i < n; i++) {
    const a = hull[i], b = hull[(i + 1) % n];
    let nx = -(b.y - a.y), ny = (b.x - a.x); const L = Math.hypot(nx, ny) || 1; nx /= L; ny /= L;
    if (nx * ((a.x + b.x) / 2 - cx) + ny * ((a.y + b.y) / 2 - cy) < 0) { nx = -nx; ny = -ny; }  // point away from centroid
    nrm.push([nx, ny]);
  }
  c.beginPath();
  for (let i = 0; i < n; i++) {
    const a = hull[i], b = hull[(i + 1) % n], [nx, ny] = nrm[i];
    if (i === 0) c.moveTo(a.x + nx * r, a.y + ny * r); else c.lineTo(a.x + nx * r, a.y + ny * r);
    c.lineTo(b.x + nx * r, b.y + ny * r);
    // corner arc at vertex b, from this edge's normal to the next edge's normal (shortest signed sweep)
    const v = hull[(i + 1) % n], [mx, my] = nrm[(i + 1) % n];
    const a1 = Math.atan2(ny, nx); let da = Math.atan2(my, mx) - a1;
    while (da <= -Math.PI) da += 2 * Math.PI; while (da > Math.PI) da -= 2 * Math.PI;
    const steps = Math.max(1, Math.ceil(Math.abs(da) / 0.4));
    for (let k = 1; k <= steps; k++) { const t = a1 + da * k / steps; c.lineTo(v.x + Math.cos(t) * r, v.y + Math.sin(t) * r); }
  }
  c.closePath();
}
// A boundary's color: the band picked by the id's hash, so its outline is never a signal's color.
function hyperColor(id) {
  if (FORCED_COLORS.matches) return INK;
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return TYPE_BAND[h % TYPE_BAND.length];
}
// Convex hull (Andrew monotone chain). Deterministic (sorted input) - Principle 16. Returns as-is if fewer than 3 points.
function convexHull(pts) {
  if (pts.length < 3) return pts.slice();
  const p = pts.slice().sort((a, b) => a.x - b.x || a.y - b.y);
  const cross = (o, a, b) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lo = [];
  for (const q of p) { while (lo.length >= 2 && cross(lo[lo.length-2], lo[lo.length-1], q) <= 0) lo.pop(); lo.push(q); }
  const up = [];
  for (let i = p.length - 1; i >= 0; i--) { const q = p[i]; while (up.length >= 2 && cross(up[up.length-2], up[up.length-1], q) <= 0) up.pop(); up.push(q); }
  lo.pop(); up.pop();
  return lo.concat(up);
}
// The scene grid (design-system.md Section 27.7, U37): every node counting-sorted into square cells of
// about four nodes each, and every relation into the cell of its midpoint - one whose ends lie more
// than REL_SPAN cells apart on either axis is kept in a list every query reads. A relation's box,
// grown by the widest lane, meets a window only if its midpoint lies within half that span and a lane
// of it, so a query reads the midpoint cells of the window grown by as much. It is rebuilt when the
// positions have moved since (positionsGen); a frame asks it what a box holds rather than passing
// over every node and relation. A box that holds every node is answered with every index at once, and
// one that covers more than an eighth of the cells by a pass over the nodes, which answers in order
// without the sort the cells' answer needs.
const REL_SPAN = 4;
const scene = { gen: -1, nodes: null, edges: null, cell: 1, x0: 0, y0: 0, x1: 0, y1: 0, w: 0, h: 0,
  start: new Int32Array(1), order: new Int32Array(0), rStart: new Int32Array(1), rOrder: new Int32Array(0),
  long: new Int32Array(0), allNodes: new Int32Array(0), allRels: new Int32Array(0), buf: new Int32Array(0) };
function sceneOf() {
  const g = scene;
  if (g.gen === positionsGen && g.nodes === nodes && g.edges === edges) return g;
  g.gen = positionsGen; g.nodes = nodes; g.edges = edges;
  const N = nodes.length, E = edges.length;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const v of nodes) {
    if (v.x < minX) minX = v.x; if (v.x > maxX) maxX = v.x;
    if (v.y < minY) minY = v.y; if (v.y > maxY) maxY = v.y;
  }
  if (!N) { minX = minY = maxX = maxY = 0; }
  const cell = Math.max(2 * NODE_R_MAX, Math.sqrt(Math.max(1, (maxX - minX) * (maxY - minY)) / Math.max(1, N / 4)));
  const w = Math.floor((maxX - minX) / cell) + 1, h = Math.floor((maxY - minY) / cell) + 1, cells = w * h;
  Object.assign(g, { cell, x0: minX, y0: minY, x1: maxX, y1: maxY, w, h });
  const at = (vx, vy) => Math.min(w - 1, Math.max(0, Math.floor((vx - minX) / cell))) + Math.min(h - 1, Math.max(0, Math.floor((vy - minY) / cell))) * w;
  const start = new Int32Array(cells + 1), cellOf = new Int32Array(N);
  for (let i = 0; i < N; i++) { const c = at(nodes[i].x, nodes[i].y); cellOf[i] = c; start[c + 1]++; }
  for (let c = 0; c < cells; c++) start[c + 1] += start[c];
  const order = new Int32Array(N), cur = start.slice(0, cells);
  for (let i = 0; i < N; i++) order[cur[cellOf[i]]++] = i;
  g.start = start; g.order = order;
  // Relations by the cell of their midpoint, the long ones in a list of their own.
  const span = REL_SPAN * cell, rCell = new Int32Array(E), rStart = new Int32Array(cells + 1), long = [];
  for (let k = 0; k < E; k++) {
    const e = edges[k];
    if (Math.abs(e.a.x - e.b.x) > span || Math.abs(e.a.y - e.b.y) > span) { rCell[k] = -1; long.push(k); continue; }
    const c = at((e.a.x + e.b.x) / 2, (e.a.y + e.b.y) / 2);
    rCell[k] = c; rStart[c + 1]++;
  }
  for (let c = 0; c < cells; c++) rStart[c + 1] += rStart[c];
  const rOrder = new Int32Array(rStart[cells]), rCur = rStart.slice(0, cells);
  for (let k = 0; k < E; k++) if (rCell[k] >= 0) rOrder[rCur[rCell[k]]++] = k;
  g.rStart = rStart; g.rOrder = rOrder; g.long = Int32Array.from(long);
  if (g.allNodes.length !== N) g.allNodes = Int32Array.from({ length: N }, (_, i) => i);
  if (g.allRels.length !== E) g.allRels = Int32Array.from({ length: E }, (_, i) => i);
  if (g.buf.length < Math.max(N, E)) g.buf = new Int32Array(Math.max(N, E));
  return g;
}
// The cells a world box covers, clamped to the grid, or null when it misses the grid altogether.
function sceneCells(g, x0, y0, x1, y1) {
  const cx0 = Math.floor((x0 - g.x0) / g.cell), cx1 = Math.floor((x1 - g.x0) / g.cell);
  const cy0 = Math.floor((y0 - g.y0) / g.cell), cy1 = Math.floor((y1 - g.y0) / g.cell);
  if (cx1 < 0 || cy1 < 0 || cx0 >= g.w || cy0 >= g.h) return null;
  return [Math.max(0, cx0), Math.min(g.w - 1, cx1), Math.max(0, cy0), Math.min(g.h - 1, cy1)];
}
// Whether a world box holds every node, so every index answers it.
function sceneHeld(g, x0, y0, x1, y1) { return x0 <= g.x0 && y0 <= g.y0 && x1 >= g.x1 && y1 >= g.y1; }
// Whether cells cover more than an eighth of the grid, where a pass costs less than the cells and a sort.
function sceneMost(g, c) { return 8 * (c[1] - c[0] + 1) * (c[3] - c[2] + 1) > g.w * g.h; }
const NONE = new Int32Array(0);
// The nodes a world box holds, by index, in the order they are drawn in. The answer is read, not kept.
function nodesIn(x0, y0, x1, y1) {
  const g = sceneOf();
  if (sceneHeld(g, x0, y0, x1, y1)) return g.allNodes;
  const c = sceneCells(g, x0, y0, x1, y1);
  if (!c) return NONE;
  const buf = g.buf;
  let n = 0;
  if (sceneMost(g, c)) {
    for (let i = 0; i < nodes.length; i++) { const v = nodes[i]; if (v.x >= x0 && v.x <= x1 && v.y >= y0 && v.y <= y1) buf[n++] = i; }
    return buf.slice(0, n);
  }
  for (let y = c[2]; y <= c[3]; y++) for (let x = c[0]; x <= c[1]; x++) {
    const cc = x + y * g.w;
    for (let k = g.start[cc]; k < g.start[cc + 1]; k++) {
      const i = g.order[k], v = nodes[i];
      if (v.x >= x0 && v.x <= x1 && v.y >= y0 && v.y <= y1) buf[n++] = i;
    }
  }
  return buf.slice(0, n).sort();
}
// The relations whose box may meet a world box, each once, in their order; the caller still tests them.
function relationsIn(x0, y0, x1, y1) {
  const g = sceneOf();
  if (sceneHeld(g, x0, y0, x1, y1)) return g.allRels;
  const grow = REL_SPAN * g.cell / 2 + LANE_MAX, c = sceneCells(g, x0 - grow, y0 - grow, x1 + grow, y1 + grow);
  if (c && sceneMost(g, c)) return g.allRels;
  if (!c) return g.long;
  // The cells' relations, sorted, merged with the long ones, which are kept in order.
  const buf = g.buf, long = g.long;
  let n = 0;
  for (let y = c[2]; y <= c[3]; y++) for (let x = c[0]; x <= c[1]; x++) {
    const cc = x + y * g.w;
    for (let k = g.rStart[cc]; k < g.rStart[cc + 1]; k++) buf[n++] = g.rOrder[k];
  }
  const near = buf.subarray(0, n).sort(), out = new Int32Array(n + long.length);
  let i = 0, j = 0, o = 0;
  while (i < n && j < long.length) out[o++] = near[i] < long[j] ? near[i++] : long[j++];
  while (i < n) out[o++] = near[i++];
  while (j < long.length) out[o++] = long[j++];
  return out;
}
// --- Semantic zoom (design-system.md Section 27.8, U38) -------------------------------------------
// Zoomed out, the map keeps its shape by keeping its best-connected nodes. Every node stays where the
// layout put it; the map keeps whole about one node for each KEEP_PX by KEEP_PX of the screen it
// covers, the best-connected first, and the next as many fade out, eased and in FADE_STEPS steps, so
// as the map zooms out the least-connected fade first and as it zooms in they come back. A node faded
// out is clipped: not drawn, named, under the pointer or visited by the frame. A node kept is drawn at
// least KEEP_MIN_PX in radius, so a far map is its best-connected nodes where they are.
const KEEP_PX = 12, KEEP_MIN_PX = 1.6;
// The nodes' indices best-connected first, kept while the page holds the same nodes.
const byDegree = { nodes: null, order: NONE };
function degreeOrder() {
  const d = byDegree;
  if (d.nodes !== nodes) {
    d.nodes = nodes;
    d.order = Int32Array.from({ length: nodes.length }, (_, i) => i).sort((p, q) => (nodes[q].degree || 0) - (nodes[p].degree || 0) || p - q);
  }
  return d.order;
}
// The nodes in the drawn types by rank, best-connected first (`of` is a node's rank, -1 for a hidden
// type); the contested among them, which never fade; and their mean spacing in world units - the
// square root of the area they cover over their number, that area the disc whose mean distance from
// its centre is theirs, which a few far nodes barely move - kept while the nodes, their positions and
// the type filter are the same.
const ranks = { nodes: null, filt: null, gen: -1, of: NONE, order: NONE, contested: NONE, count: 0, spacing: 1 };
function ranksOf() {
  const r = ranks, filt = [...typeOff].join(",");
  if (r.nodes !== nodes || r.filt !== filt) {
    const of = new Int32Array(nodes.length).fill(-1), order = [], contested = [];
    for (const i of degreeOrder()) {
      if (typeOff.has(nodes[i].type)) continue;
      of[i] = order.length; order.push(i);
      if (nodes[i].contested) contested.push(i);
    }
    Object.assign(r, { nodes, filt, gen: -1, of, order: Int32Array.from(order), contested: Int32Array.from(contested), count: order.length });
  }
  if (r.gen !== positionsGen) {
    r.gen = positionsGen;
    let cx = 0, cy = 0, md = 0;
    for (const i of r.order) { cx += nodes[i].x; cy += nodes[i].y; }
    cx /= r.count || 1; cy /= r.count || 1;
    for (const i of r.order) md += Math.hypot(nodes[i].x - cx, nodes[i].y - cy);
    // A disc of radius R has its mean distance from the centre at 2R/3.
    md /= r.count || 1;
    r.spacing = r.count > 1 ? Math.max(1, 1.5 * md * Math.sqrt(Math.PI / r.count)) : 1;
  }
  return r;
}
// How many nodes a map keeps whole at a scale `s`, keeping one for each `px` by `px` of what it covers:
// not bounded by their number, so the fade past it comes to nothing just as every node is whole.
function keptAt(r, s, px) { return r.count * (r.spacing * s / px) ** 2; }
// A node's weight by its rank where `c` are kept whole: 1 whole, fading to 0 by rank 2c.
function keptWeight(rank, c) { return rank < 0 || rank >= 2 * c ? 0 : rank < c ? 1 : stepped(1 - ease(c, 2 * c, rank)); }
// The nodes a frame visits when the map keeps fewer than its window holds: the first `upto` by rank,
// and those never faded - hovered, selected, active, pulsing or contested - by index, in drawing order.
// A frame of a far map passes over these and the relations they hold (Section 27.8), not over every
// node and relation the window holds.
function keptNodes(r, upto, act) {
  const n0 = Math.min(r.count, Math.ceil(upto)), out = Array.from(r.order.subarray(0, n0));
  const add = n => { if (n && n._k !== undefined && (r.of[n._k] < 0 || r.of[n._k] >= n0)) out.push(n._k); };
  add(hover); add(focus);
  if (act) for (const id of act.ns) add(nodeById(id));
  for (const id of pulses.keys()) add(nodeById(id));
  for (const i of r.contested) add(nodes[i]);
  return Int32Array.from(new Set(out)).sort();
}
const incidence = { edges: null, start: NONE, list: NONE, seen: NONE, stamp: 0 };
function incidenceOf() {
  const g = incidence;
  if (g.edges === edges && g.start.length === nodes.length + 1) return g;
  const N = nodes.length, start = new Int32Array(N + 1);
  for (const e of edges) { start[e.a._k + 1]++; start[e.b._k + 1]++; }
  for (let i = 0; i < N; i++) start[i + 1] += start[i];
  const list = new Int32Array(start[N]), cur = start.slice(0, N);
  for (let k = 0; k < edges.length; k++) { const e = edges[k]; list[cur[e.a._k]++] = k; list[cur[e.b._k]++] = k; }
  Object.assign(g, { edges, start, list, seen: new Int32Array(edges.length), stamp: 0 });
  return g;
}
function relationsOfNodes(ks) {
  const g = incidenceOf(), stamp = ++g.stamp, out = [];
  for (const i of ks) for (let j = g.start[i]; j < g.start[i + 1]; j++) {
    const r = g.list[j];
    if (g.seen[r] !== stamp) { g.seen[r] = stamp; out.push(r); }
  }
  return Int32Array.from(out).sort();
}

// Whether a node is drawn this frame: its type is shown and it meets the window (stamped in draw).
let drawFrame = 0;
function onScreen(n) { return n._vf === drawFrame; }
// Whether the last frame visited only the nodes the map keeps (Section 27.8): a node it did not stamp
// has faded out.
let frameKept = false;

// The contexts' membership as index arrays into the nodes (design-system.md Section 27.4, U34), kept
// while the page holds the same nodes and contexts: `start[h]` to `start[h + 1]` of `mem` are context
// h's members, and `drawn` the contexts of three or more - the regions the map draws. The layout
// takes them with the graph.
const regionSim = { nodes: null, hyper: null, count: 0, start: new Int32Array(1), mem: new Int32Array(0), drawn: new Int32Array(0), hid: new Int32Array(0) };
function regionSimOf() {
  const r = regionSim;
  if (r.nodes === nodes && r.hyper === hyperedges) return r;
  const at = new Map();
  for (let i = 0; i < nodes.length; i++) at.set(nodes[i].id, i);
  const start = [0], mem = [], drawn = [], hid = [];
  for (let e = 0; e < hyperedges.length; e++) {
    const from = mem.length;
    for (const id of hyperedges[e].members) { const k = at.get(id); if (k !== undefined) mem.push(k); }
    if (mem.length - from < 2) { mem.length = from; continue; }
    if (mem.length - from >= 3) drawn.push(start.length - 1);
    start.push(mem.length); hid.push(e);
  }
  r.nodes = nodes; r.hyper = hyperedges; r.count = start.length - 1;
  r.start = Int32Array.from(start); r.mem = Int32Array.from(mem); r.drawn = Int32Array.from(drawn); r.hid = Int32Array.from(hid);
  return r;
}

// --- the layout's host (Section 27.5, U35) -------------------------------------------------------
// The layout runs in a worker when one starts, and here, from draw, when none does - the same engine
// either way (viewer-sim.js). The page sends it the graph when the graph changes and its state when
// that changes, and takes back positions, which it writes into the nodes it draws; a report for a
// graph the page has since replaced (an older `gen`) is dropped.
const layout = (() => {
  let worker = null, engine = null, gen = 0, lastGraph = null, shownNow = false;
  const lastSet = {}, stats = { steps: 0, stepMs: 0, graphMs: 0 };
  const apply = st => {
    if (st.gen !== gen) return;
    alpha = st.alpha; overlapOwed = st.overlapOwed; simMotion = st.motion;
    stats.steps = st.steps; stats.stepMs = st.stepMs; stats.graphMs = st.graphMs;
    // A report without positions moved nothing, and what is kept from them stands (Section 27.7).
    if (!st.x) return;
    const n = Math.min(nodes.length, st.x.length);
    // The held node is where the pointer is: a report carries where the layout last heard it was, and
    // writing that back pulled the node behind the pointer - and sent the older place back as where it
    // is held, so a fast drag lost the node.
    for (let i = 0; i < n; i++) { const v = nodes[i]; if (v === drag) continue; v.x = st.x[i]; v.y = st.y[i]; }
    positionsGen++;
  };
  // No worker, or one that failed to start: the engine runs here, from the positions the page holds.
  const here = () => {
    if (engine) return;
    if (worker) worker.terminate();
    worker = null;
    engine = createLayout();
    engine.set(lastSet);
    if (lastGraph) layoutGraph(true);
  };
  try {
    worker = new Worker("/viewer-sim.js");
    worker.onmessage = ev => { if (ev.data.kind === "state") { apply(ev.data); requestFrame(); } };
    worker.onerror = () => { here(); requestFrame(); };
  } catch (e) { here(); }
  return {
    graph(g) {
      g.gen = ++gen; lastGraph = g;
      if (engine) engine.graph(g); else worker.postMessage({ kind: "graph", ...g });
    },
    set(q) {
      Object.assign(lastSet, q);
      if (engine) engine.set(q); else worker.postMessage({ kind: "set", set: q });
    },
    wake(a) { if (engine) engine.wake(a); else worker.postMessage({ kind: "wake", alpha: a }); },
    cool(a) { if (engine) engine.cool(a); else worker.postMessage({ kind: "cool", alpha: a }); },
    shown(on) {
      if (on === shownNow) return;
      shownNow = on;
      if (worker) worker.postMessage({ kind: "shown", shown: on });
    },
    // A frame's step, when the engine runs here; in a worker the positions come as messages.
    tick() {
      if (!engine) return;
      engine.step();
      const { x, y } = engine.positions();
      apply({ ...engine.state(), x, y });
    },
    here: () => engine !== null,
    // How many steps the layout has taken and what the last one cost - what a bench reads.
    stats: () => ({ ...stats, worker: !engine }),
  };
})();

// The graph as the layout takes it: each node's position, velocity, degree, workspace and type as
// arrays, its index in the graph the layout held before (`_k`, so a node that stays keeps what the
// layout has for it), the relations as index pairs and the contexts as kept index arrays. `moved`
// says the page moved every node itself, and the layout takes all their positions.
function layoutGraph(moved = false) {
  const N = nodes.length;
  const x = new Float64Array(N), y = new Float64Array(N), vx = new Float64Array(N), vy = new Float64Array(N);
  const deg = new Float64Array(N), ws = new Int32Array(N), typ = new Int32Array(N), prev = new Int32Array(N);
  const wsAt = new Map(), named = [], typeAt = new Map(Object.keys(typeColor).sort().map((t, i) => [t, i]));
  const at = new Map();
  for (let i = 0; i < N; i++) {
    const v = nodes[i], w = v.workspace || "";
    x[i] = v.x; y[i] = v.y; vx[i] = v.vx || 0; vy[i] = v.vy || 0; deg[i] = v.degree || 0;
    let k = wsAt.get(w);
    if (k === undefined) { k = wsAt.size; wsAt.set(w, k); named.push(w ? 1 : 0); }
    ws[i] = k;
    const t = typeAt.get(v.type);
    typ[i] = t === undefined ? -1 : t;
    prev[i] = v._k === undefined ? -1 : v._k;
    at.set(v, i);
  }
  for (let i = 0; i < N; i++) nodes[i]._k = i;
  const ea = new Int32Array(edges.length), eb = new Int32Array(edges.length);
  for (let k = 0; k < edges.length; k++) { ea[k] = at.get(edges[k].a); eb[k] = at.get(edges[k].b); }
  const rg = regionSimOf();
  layout.graph({ x, y, vx, vy, deg, ws, wsNamed: Uint8Array.from(named), typ, prev, moved, ea, eb,
    regStart: rg.start, regMem: rg.mem, regDrawn: rg.drawn });
}

// The layout's view of the moment, sent when it changes: the modes, the window's centre, the zoom
// (the coast is measured on the screen), the held node and where it is held, the focused node, and
// group mode's targets - one per type, on a circle, in sorted type order.
let layoutSent = "";
function layoutSync() {
  const q = {
    // The workspaces cohere whatever Boundaries draws, so b never moves a node (Section 29.12); the
    // layout still turns it off under Arrange by type and in a scope of one workspace.
    hullForce, clusterMode, wsMode: true, wcx: innerWidth / 2, wcy: innerHeight / 2,
    camS: +cam.s.toPrecision(2), drag: drag ? drag._k : -1, dragX: drag ? drag.x : 0, dragY: drag ? drag.y : 0,
    focus: focus && focus._k !== undefined ? focus._k : -1, targets: null,
  };
  if (clusterMode) {
    const types = Object.keys(typeColor).sort(), k = Math.max(1, types.length);
    const R = Math.min(innerWidth, innerHeight) * 0.34, t = new Array(2 * types.length);
    types.forEach((_, i) => { const a = (i / k) * Math.PI * 2; t[2 * i] = q.wcx + R * Math.cos(a); t[2 * i + 1] = q.wcy + R * Math.sin(a); });
    q.targets = t;
  }
  const key = JSON.stringify(q);
  if (key === layoutSent) return;
  layoutSent = key;
  layout.set(q);
}

// --- Haloed labels: text and its halo rasterized together, then blitted on the device pixel grid ---
// A label used to be strokeText (the halo) then fillText (the name) at the same point, and the two did
// not land together. Measured in this engine (WebKit/CoreText): fillText snaps glyphs to the device
// pixel grid while strokeText draws the outline at the exact fractional coordinate, so at 1x the halo
// sat up to 0.26px off horizontally and 0.15-0.5px vertically - and the vertical part depends on the
// glyph, so no coordinate snapping or fixed offset can cancel it. A translucent label also let the
// halo's inner half show through its own glyphs. So the halo is built FROM the fill: the same glyphs
// drawn in the background color at whole-device-pixel offsets inside a disc (a dilation - every copy
// is the identical raster, shifted), the name on top, baked once into a sprite. Registration then
// holds by construction, globalAlpha fades the label as one unit, and a frame costs one drawImage per
// label instead of two text layouts. Sprites are cached by everything that shapes them, LRU-bounded.
// Names - of nodes and of regions - are set in the reading face and relation kinds in the identifier
// face (docs/design-system.md Section 4), the same stacks viewer.css names: a Korean name and an
// English one then share one design, which the mono stack, holding no Hangul, could not give them.
const NAME_FAMILY = '"Pretendard Variable",Pretendard,system-ui,-apple-system,"Apple SD Gothic Neo",sans-serif';
const KIND_FAMILY = "'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
// The cap bounds memory (a 12px name is ~50KB at 2x) and sits above what label thinning leaves on screen;
// the node pass culls off-screen labels so a large graph cannot cycle the cache every frame.
const LABEL_SPRITES = new Map(), LABEL_SPRITE_CAP = 300;
// A sprite baked before its face had loaded holds the fallback's glyphs, and was placed by the
// fallback's measure. When a face finishes loading the sprites go and the map draws again.
if (document.fonts) document.fonts.addEventListener("loadingdone", () => { LABEL_SPRITES.clear(); requestFrame(); });
const haloDiscs = new Map();   // "radius|dpr" -> [[dx, dy], ...] in device px, centre excluded
function haloDisc(radius, dpr) {
  const key = radius + "|" + dpr;
  let d = haloDiscs.get(key);
  if (!d) {
    const r = radius * dpr, R = Math.ceil(r);
    d = [];
    for (let dy = -R; dy <= R; dy++) for (let dx = -R; dx <= R; dx++)
      if ((dx || dy) && dx * dx + dy * dy <= r * r + 1e-6) d.push([dx, dy]);
    haloDiscs.set(key, d);
  }
  return d;
}
function labelSprite(text, weight, size, color, spacing, halo) {
  const key = [DPR, SURFACE, weight, size, color, spacing, halo, text].join("|");
  let s = LABEL_SPRITES.get(key);
  if (s) { LABEL_SPRITES.delete(key); LABEL_SPRITES.set(key, s); return s; }   // LRU touch
  const font = weight + " " + size + "px " + NAME_FAMILY;
  const scratch = labelSprite.ctx || (labelSprite.ctx = document.createElement("canvas").getContext("2d"));
  scratch.font = font;
  if ("letterSpacing" in scratch) scratch.letterSpacing = spacing;
  const tw = scratch.measureText(text).width;
  // Whole CSS px of padding, so the text origin sits on the device grid inside the sprite too.
  const pad = Math.ceil(halo) + 2, w = Math.ceil(tw) + 2 * pad, h = Math.ceil(size * 1.6) + 2 * pad;
  const c = document.createElement("canvas");
  c.width = Math.ceil(w * DPR); c.height = Math.ceil(h * DPR);
  const o = c.getContext("2d");
  o.setTransform(DPR, 0, 0, DPR, 0, 0);
  o.font = font; o.textBaseline = "middle"; o.textAlign = "left";
  if ("letterSpacing" in o) o.letterSpacing = spacing;
  const ax = pad, ay = Math.round(h / 2 * DPR) / DPR;
  o.fillStyle = SURFACE;
  for (const [dx, dy] of haloDisc(halo, DPR)) o.fillText(text, ax + dx / DPR, ay + dy / DPR);
  o.fillStyle = color; o.fillText(text, ax, ay);
  s = { c, w: c.width / DPR, h: c.height / DPR, ax, ay, tw };
  LABEL_SPRITES.set(key, s);
  if (LABEL_SPRITES.size > LABEL_SPRITE_CAP) LABEL_SPRITES.delete(LABEL_SPRITES.keys().next().value);
  return s;
}
// Draw a haloed label in screen space (the DPR transform must be current). `y` is the label's middle,
// as textBaseline "middle" would place it; `align` is "left" or "center". The sprite lands on whole
// device pixels so the blit stays 1:1 - a fractional destination would resample it soft.
function drawLabel(text, weight, size, color, spacing, halo, x, y, align) {
  const s = labelSprite(text, weight, size, color, spacing, halo);
  const left = align === "center" ? x - s.tw / 2 : x;
  ctx.drawImage(s.c, Math.round((left - s.ax) * DPR) / DPR, Math.round((y - s.ay) * DPR) / DPR, s.w, s.h);
}

function draw() {
  rafId = null;
  layoutSync();
  layout.shown(!settling);
  // The layout steps on a thread of its own (U35); here only when no worker could start.
  if (simWarm()) layout.tick();
  easeCam();
  // Reduced motion: settle synchronously (bounded - alpha decays multiplicatively, so convergence
  // takes ~110 steps; the cap only guards a pathological graph from locking the frame). A worker
  // settles behind the loader as fast as it can anyway.
  if (settling && REDUCED_MOTION && layout.here()) { for (let i = 0; i < 600 && alpha > REVEAL_ALPHA; i++) layout.tick(); }
  // Reveal transition: the layout has calmed enough to show. Frame it first (auto-fit + snap the
  // camera, before any user interaction) so the graph appears already fitted rather than mid-zoom.
  if (settling && alpha <= REVEAL_ALPHA) {
    settling = false;
    if (needFit && !userMoved) { needFit = false; fitView(); cam.s = camT.s; cam.x = camT.x; cam.y = camT.y; refitAtRest = true; }
    else if (refitOnReveal) { fitView(); }   // smooth re-frame after synced knowledge landed
    refitOnReveal = false;
  }
  // While settling, keep stepping the sim (above) but hide the graph behind the loader - the user sees
  // a calm spinner instead of nodes flying around during the violent early rearrangement.
  if (settling && nodes.length) {
    ctx.setTransform(1,0,0,1,0,0); ctx.clearRect(0,0,canvas.width,canvas.height);
    if (gpu) gpu.clear();
    // Section 9's words: what the layout settles, counted (design-system.md Section 29.6).
    if (loaderSayEl) loaderSayEl.textContent = "Arranging " + plural(nodes.length, "entity", "entities");
    loaderEl.classList.add("on");
    requestFrame(); return;
  }
  loaderEl.classList.remove("on");
  // Initial auto-fit: once after the layout settles (only before user interaction).
  if (needFit && alpha < ALPHA_MIN && !userMoved) { needFit = false; fitView(); }
  if (refitAtRest && !simWarm()) { refitAtRest = false; if (!userMoved) fitView(); }

  const act = activeSet(), hov = act ? null : hoverSet();
  // A working set dims the rest while no selection, search or preview takes the map; its members are
  // not made active, so they fade by rank like any entity (Section 29.11, U38).
  const kept = !act && workSet ? workSet.ids : null;
  // Legend-chip hover highlight: for an edge-kind chip, collect the endpoint nodes of matching visible
  // edges once per frame (nodes to keep lit). Node-type chips match on n.type directly.
  let lgEndpoints = null;
  if (edgeTypeHl) {
    lgEndpoints = new Set();
    for (const e of edges) {
      if (e.type !== edgeTypeHl || typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
      if (e.valid_to && !showSuperseded) continue;
      lgEndpoints.add(e.a.id); lgEndpoints.add(e.b.id);
    }
  }
  const hlAnchor = focus || hover;   // the active node (click or hover) - drives hull emphasis + fade
  const anchor = focus || hover;
  ctx.setTransform(1,0,0,1,0,0);
  ctx.clearRect(0,0,canvas.width,canvas.height);
  bulk = gpu && !FORCED_COLORS.matches ? gpu : ctx;
  glCanvas.classList.toggle("on", bulk === gpu);
  if (bulk === gpu) gpu.begin({ s: cam.s, x: cam.x, y: cam.y, w: innerWidth, h: innerHeight, dpr: DPR });

  // Edges + nodes use the world transform (zoom/pan, incl. DPR supersampling); labels use screen coordinates (keeping readability).
  ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);
  ctx.lineCap = "round"; ctx.lineJoin = "round";
  // Hyperedge hull overlay (laid behind edges/nodes). Only size>=3 is drawn - 2 converges to a binary
  // edge. Each hull is a cloud (paintCloud): one gradient-filled ellipse, directly on the canvas - no
  // stroke and no offscreen compositing (cheap: one fill, not a large drawImage copy). Overlapping
  // hulls blend naturally (C1: overlap = connective tissue).
  // While a node is active, its own hulls are emphasized and the rest fade. Labels are collected per hull.
  let hullLabels = [];
  // Every label placed this frame - region names first, then node names - so the second pass avoids
  // the first: screen-space boxes on a grid of cells (labelGrid).
  const labelBoxes = labelGrid();
  if (hyperMode && hyperedges.length) {
    const items = [];
    for (const it of regionGeometry()) {
      const hot = !!hlAnchor && it.h.members.includes(hlAnchor.id);   // is the active (click/hover) node a member of this context
      // Every context is drawn as a cloud by default, the active node's at full strength and the
      // rest faded (design-system.md Section 5); as flat hulls they had overlapped into one slab,
      // which is why only the active node's were drawn. "all regions" off in the view menu draws
      // the active node's alone.
      if (!allRegions && !hot) continue;
      // A kept hull's reach follows the zoom, and its cloud's shape with it.
      const reach = hullReach(it.ms);
      if (reach !== it.g.r) { it.g.r = reach; it.shape = cloudShape(it.g); }
      // Legend-chip hover: does this context contain any highlighted member? Hulls without one fade,
      // so the highlighted type/relation stands out against the hull layer too (same inspection language).
      it.lgHit = typeHl ? it.ms.some(m => m.type === typeHl)
        : edgeTypeHl ? it.ms.some(m => lgEndpoints.has(m.id))
        : false;
      it.hot = hot;
      items.push(it);
    }
    const lgActive = !!(typeHl || edgeTypeHl);
    for (const it of items) {
      it.alpha = lgActive ? (it.lgHit ? HULL_LAYER_ALPHA : DIM_REGION)
        // A hover brings its regions to full strength and dims no other; dimming is a selection's.
        : hlAnchor ? (it.hot ? HULL_ACTIVE_ALPHA : focus ? DIM_REGION : HULL_LAYER_ALPHA) : HULL_LAYER_ALPHA;
      // The name sits above the region's upper edge, outside it, where no member's label is. Only
      // the active node's regions are named: names are placed before the nodes', which avoid them,
      // so twenty region names at rest would push the map's own names out. A region is named by its
      // two best-connected members other than the active node, with the count, so the regions around
      // one node read as what else is in them rather than as that node five times.
      if (!it.hot) continue;
      const named = it.ms.filter(m => !hlAnchor || m.id !== hlAnchor.id)
        .sort((a, b) => (b.degree || 0) - (a.degree || 0) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
        .slice(0, 2).map(m => m.name);
      const top = Math.min(...it.g.hull.map(p => p.y)) - it.g.r;
      hullLabels.push({ cx: it.g.cx, cy: top, text: named.join(", ") + " (" + it.ms.length + ")", col: regionLabelColor(it.id), hot: it.hot, lgHit: it.lgHit, size: it.ms.length });
    }
    drawCloudField(items);   // every cloud at once, mixed in OKLCH
    ctx.globalAlpha = 1;
    drawWsHulls(bulk, hullLabels);
    // Group labels: their own layer, right after the hulls (below edges/nodes). Greedy placement -
    // active first, then larger contexts - skips any label whose box overlaps one already placed, so
    // the map reads as spaced region names instead of a wall of overlapping text. A soft pill keeps
    // each name legible over the busy hull fills. Screen space, so restore the world transform after.
    if (showLabels && hullLabels.length) {
      ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      // Region names are 13px at medium weight whatever the region's size (design-system.md Section 5):
      // scaled up to 30px in bold they ran over one another and the nodes they named. Sorted so the
      // active and the largest contexts win placement; one overlapping a placed name is skipped.
      const cand = hullLabels
        .map(l => {
          const fs = 13 * TEXT_SCALE;
          return { l, px: l.cx*cam.s + cam.x, py: l.cy*cam.s + cam.y - fs * 0.8, fs };
        })
        .filter(o => o.px > -80 && o.px < innerWidth + 80 && o.py > -30 && o.py < innerHeight + 30)
        .sort((a, b) => (b.l.hot - a.l.hot) || (b.l.size - a.l.size));
      for (const o of cand) {
        const l = o.l;
        ctx.font = "500 " + o.fs + "px " + NAME_FAMILY;   // measures the placement box; o.fs is 13 * TEXT_SCALE
        const w = ctx.measureText(l.text).width + o.fs*0.5, h = o.fs*1.3, x = o.px - w/2, y = o.py - h/2;
        if (!labelBoxes.fits(x, y, w, h)) continue;
        labelBoxes.add(x, y, w, h);
        // Blend into the map without a chip: a crisp background-color halo (a cutout that matches the
        // canvas) keeps the group-color name sharp over the hull fills; lower idle opacity lets it recede.
        // The label fades as one unit (drawLabel), so the halo no longer darkens its own glyphs.
        ctx.globalAlpha = lgActive ? (l.lgHit ? 0.75 : HULL_LABEL_HOVER_FADE)
          : hlAnchor ? (l.hot ? 1 : focus ? HULL_LABEL_HOVER_FADE : 0.6) : 0.6;
        drawLabel(l.text, "500", o.fs, l.col, "0px", 2, o.px, o.py, "center");
      }
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";
      ctx.globalAlpha = 1; ctx.textAlign = "left"; ctx.textBaseline = "alphabetic";
      ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);   // back to world for edges/nodes
    }
  }
  // Relation labels for the active node's edges - collected during the edge draw (which has the curve
  // geometry), rendered later in the label pass. Only when a node is active and labels are on.
  const edgeLabels = (act && showLabels) ? [] : null;
  // What the frame draws (design-system.md Section 26.5, U30). The map's window in world units, grown
  // by the most a node's decoration reaches past its disc, so nothing that could touch the window is
  // left out; and each node's size on the screen, which decides how much of it is drawn, continuously
  // (HALO_FROM to HALO_TO for its halo, FULL_FROM to FULL_TO for its ring, arrowheads and lanes).
  // A hovered, selected or active node is drawn full whatever its size, so a highlight reads. The
  // window is grown by the largest node and its glow, and by the contested badge, which keeps its
  // size on the screen whatever the zoom.
  const reachOut = NODE_R_MAX + 16 + 9 / cam.s;
  const vx0 = (insetL() - cam.x) / cam.s - reachOut, vx1 = (innerWidth - insetR() - cam.x) / cam.s + reachOut;
  const vy0 = -cam.y / cam.s - reachOut, vy1 = (innerHeight - insetB() - cam.y) / cam.s + reachOut;
  // Semantic zoom (Section 27.8, U38): how many nodes the map keeps whole at this zoom, best-connected
  // first, the next as many fading out; a hovered, selected, active, pulsing or contested node never fades.
  const R = ranksOf(), keep = keptAt(R, cam.s, KEEP_PX), fading = keep < R.count;
  const never = n => n === hover || n === focus || (act !== null && act.ns.has(n.id)) || n.contested || pulses.has(n.id);
  // A node's weight: 1 drawn whole, 0 faded out, between as it fades.
  const weightOf = n => (!fading || never(n) ? 1 : keptWeight(R.of[n._k], keep));
  // A hovered, selected or active node is drawn in full whatever its size (Section 26.5).
  const lit1 = n => n === hover || n === focus || (act !== null && act.ns.has(n.id));
  // The nodes the window holds, from the scene grid (Section 27.7, U37), stamped for this frame with
  // their weight; a node faded out is not drawn. A map that keeps fewer nodes than its window holds -
  // about as many as its spacing on the screen fits in the window - draws only those, so a frame takes
  // them and their relations rather than the window's all.
  const few = fading && 2 * keep < (vx1 - vx0) * (vy1 - vy0) / R.spacing ** 2 ? keptNodes(R, 2 * keep, act) : null;
  drawFrame++; frameKept = few !== null;
  const vis = [];
  for (const k of few || nodesIn(vx0, vy0, vx1, vy1)) {
    const n = nodes[k];
    if (typeOff.has(n.type) || (few && (n.x < vx0 || n.x > vx1 || n.y < vy0 || n.y > vy1))) continue;
    n._vf = drawFrame; n._o = weightOf(n);
    if (n._o === 0) continue;
    // A node smaller on the screen than KEEP_MIN_PX is drawn that large.
    n._hr = nodeRadius(n) * cam.s < KEEP_MIN_PX ? KEEP_MIN_PX / cam.s : 0;
    // Its detail (Section 26.5): how much of its halo and of its ring, arrowheads and lanes is drawn.
    const rs = (n._hr || nodeRadius(n)) * cam.s, whole1 = lit1(n);
    n._d = whole1 ? 1 : stepped(ease(HALO_FROM, HALO_TO, rs));
    n._f = whole1 ? 1 : stepped(ease(FULL_FROM, FULL_TO, rs));
    n._sq = !whole1 && bulk === ctx && rs < SQUARE_PX;
    vis.push(n);
  }
  // A node's radius as drawn this frame.
  const rOf = n => (onScreen(n) && n._hr) || nodeRadius(n);
  // Where a relation meets a node: outside its halo, as wide as it is drawn.
  const edgeR = n => rOf(n) + nodeStrokeW(n) * haloNow(n) / 2;
  // A relation's end outside the window is still sized by its radius on the screen, and weighed.
  const drawn1 = n => onScreen(n) && n._o > 0;
  const haloNow = n => (drawn1(n) ? n._d : lit1(n) ? 1 : stepped(ease(HALO_FROM, HALO_TO, nodeRadius(n) * cam.s)));
  const fullNow = n => (drawn1(n) ? n._f : lit1(n) ? 1 : stepped(ease(FULL_FROM, FULL_TO, nodeRadius(n) * cam.s)));
  const weightNow = n => (onScreen(n) ? n._o : weightOf(n));
  if (showEdges) {
    // Parallel-edge lanes: relations between the same two nodes - a reciprocal pair (a<->b) or several
    // verbs in one direction (a->b) - fan out into distinct arcs instead of stacking on one line. A
    // relation is drawn when its straight span, grown by the widest lane, meets the window; every
    // relation of one pair shares that span, so the lanes are worked out from the drawn ones alone,
    // keyed by the pair's two indices rather than a string per relation.
    const pairKey = nodes.length + 1;
    const lanes = new Map(), drawn = [];
    for (const i of few ? relationsOfNodes(few) : relationsIn(vx0, vy0, vx1, vy1)) {
      const e = edges[i];
      if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
      if (edgeTypeOff.has(e.type)) continue;                 // edge-kind toggle (legend)
      if (e.valid_to && !showSuperseded) continue;           // hide superseded (past) edges (history toggle)
      if (Math.max(e.a.x, e.b.x) + LANE_MAX < vx0 || Math.min(e.a.x, e.b.x) - LANE_MAX > vx1
        || Math.max(e.a.y, e.b.y) + LANE_MAX < vy0 || Math.min(e.a.y, e.b.y) - LANE_MAX > vy1) continue;
      drawn.push(i);
      const ia = e.a._k, ib = e.b._k, pk = ia < ib ? ia * pairKey + ib : ib * pairKey + ia;
      const lst = lanes.get(pk);
      if (lst) lst.push(i); else lanes.set(pk, [i]);
    }
    // One path per look: relations that share a color, an opacity and a width are one stroke, and
    // their arrowheads one fill. The dimmest go first, so a highlight lies on top.
    const looks = new Map();
    const lookOf = (alpha, color, width) => {
      const key = color + "|" + width + "|" + alpha;
      let l = looks.get(key);
      if (!l) { l = { alpha, color, width, line: bulkPath(), heads: null }; looks.set(key, l); }
      return l;
    };
    for (const i of drawn) {
    const e = edges[i];
    const oa = weightNow(e.a), ob = weightNow(e.b);
    if (oa === 0 || ob === 0) continue;   // an end faded out, and the relation with it
    const dx = e.b.x-e.a.x, dy = e.b.y-e.a.y, d = Math.hypot(dx,dy) || 1, ux = dx/d, uy = dy/d;
    // Hovering an edge-kind chip treats its edges as hot (weight + arrowhead) - same language as
    // node hover, so "what does this relation connect" reads instantly.
    const hot = act ? act.es.has(i) : hov ? hov.es.has(i) : (edgeTypeHl ? e.type === edgeTypeHl : false);
    // A relation fades in by its length on the screen (Section 26.5); one whose ends overlap is not drawn.
    const lengthA = hot ? 1 : stepped(ease(EDGE_FROM, EDGE_TO, d * cam.s));
    if (lengthA === 0) continue;
    // Grow each radius by half of that node's stroke so the edge meets outside the node stroke (the
    // stroke is radius-proportional, so it differs per endpoint) - the arrowhead tip touches the outer
    // stroke boundary, connecting with no gap/penetration, and it holds on zoom.
    const ar = edgeR(e.a), br = edgeR(e.b), room = d - ar - br;
    if (room <= 0.5) continue;   // (temporary) overlap - skip this frame's edge
    // A relation into a node drawn full keeps its lane and its arrowhead; into a smaller one it is a
    // straight line (Section 26.5), since a lane's bend and a head shorter than a pixel read as noise.
    const full = hot ? 1 : fullNow(e.b);
    // The line starts at the source node's edge and ends at the arrowhead base (or the tip if arrows are off).
    // Arrowhead length in WORLD units (proportional to the target node, bounded 7..16) so it scales
    // with zoom like the node markers. A fixed screen size looked tiny on large/zoomed-in nodes and
    // clunky when zoomed out. Still clamped to half the free gap so short edges never overrun it.
    const alen = full * Math.min(Math.max(7, Math.min(16, nodeRadius(e.b) * 0.7)), room * 0.5);
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    // Fan this edge onto its lane within the pair: lane index centered on 0, so a lone edge -> 0 ->
    // straight (quadratic control on the midpoint = the original straight line). sign uses a canonical
    // perpendicular (smaller id -> larger id) so lanes stay distinct regardless of each edge's
    // direction - a reciprocal pair opens into a lens, several one-way verbs into a fan.
    const ia = e.a._k, ib = e.b._k;
    const lst = lanes.get(ia < ib ? ia * pairKey + ib : ib * pairKey + ia) || [i];
    const lane = full > 0 ? (lst.indexOf(i) - (lst.length - 1) / 2) * full : 0;
    const off = lane * Math.min(d * 0.30, LANE_MAX) * (e.a.id < e.b.id ? 1 : -1);
    const cpx = (sx0 + tipx)/2 + (-uy)*off, cpy = (sy0 + tipy)/2 + ux*off;   // quadratic control point
    // Tip tangent = control -> tip; the line end and arrowhead align to it (reduces to ux,uy when straight).
    let tdx = tipx - cpx, tdy = tipy - cpy; const tdl = Math.hypot(tdx, tdy) || 1; tdx /= tdl; tdy /= tdl;
    const basex = tipx - tdx*alen, basey = tipy - tdy*alen;   // arrowhead base, back along the tip tangent
    // Active-edge label: relation type at the curve midpoint (quadratic B(0.5)). len = distance so the
    // shortest edges are labeled first when the count exceeds the cap.
    if (hot && edgeLabels) edgeLabels.push({
      mx: 0.25*sx0 + 0.5*cpx + 0.25*tipx, my: 0.25*sy0 + 0.5*cpy + 0.25*tipy,
      text: e.type, len: d, col: e.valid_to ? EDGE_OLD : EDGE_ACTIVE,
    });
    // Group mode: make cross-group (different-type) edges stand out, in-group edges dim.
    const cross = clusterMode && e.a.type !== e.b.type;
    // The default is semi-transparent (EDGE_ALPHA); on hover/focus a connected edge (hot) activates to
    // 1.0 and the rest dim. Group mode is a separate emphasis that makes cross-group links stand out.
    // Legend hover outranks the rest: node-type chip -> both-endpoint edges bright, one-endpoint dim
    // halo, others faded; edge-kind chip -> matching edges full, others faded.
    const alpha = Math.min(oa, ob) * (typeHl
      ? (e.a.type === typeHl && e.b.type === typeHl ? 0.9 : (e.a.type === typeHl || e.b.type === typeHl) ? 0.45 : 0.05)
      : edgeTypeHl
      ? (e.type === edgeTypeHl ? 1 : 0.05)
      : act ? (hot ? 1 : DIM_EDGE) : hot ? 1 : (clusterMode ? (cross ? 0.9 : 0.1) : EDGE_ALPHA))
      // An end in a dimmed trust tier dims its relation too, unless a selection or hover lights it.
      * (!hot && trustOff.size && (trustOff.has(e.a.trust_tier) || trustOff.has(e.b.trust_tier)) ? DIM_EDGE / EDGE_ALPHA : 1)
      // A relation leaving the working set dims with the rest; one between two members keeps its strength.
      * (!hot && kept && !(kept.has(e.a.id) && kept.has(e.b.id)) ? DIM_EDGE / EDGE_ALPHA : 1);
    // A relation's kind is said by its label, not a color (Section 5); an active edge or one whose kind
    // the legend points at is drawn brighter. A superseded edge is EDGE_OLD, thin (a past signal).
    const color = e.valid_to ? EDGE_OLD
      : (hot || edgeTypeHl === e.type) ? EDGE_ACTIVE : EDGE_NEUTRAL;
    // Constant thickness on screen; a past relation is the thinnest line.
    const look = lookOf(stepped(alpha * lengthA), color, (e.valid_to ? 0.8 : hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s);
    // An active edge says its direction with an arrowhead even when the view's arrows are off: lines
    // are solid (Section 4), so nothing marches along it.
    const arrow = full > 0 && (showArrows || hot);
    // Without an arrowhead, draw the curve to the node edge (tip); with one, to the arrowhead base.
    const endx = arrow ? basex : tipx, endy = arrow ? basey : tipy;
    look.line.moveTo(sx0, sy0);
    if (off) look.line.quadraticCurveTo(cpx, cpy, endx, endy); else look.line.lineTo(endx, endy);
    if (arrow) {
      // Arrowhead: base -> tip along the tip tangent (points the way the curve arrives). The heads are
      // filled, and a fill closes each triangle, so none is closed here: closing every one of a few
      // thousand in one path cost Chrome 80 ms a frame.
      const hw = alen * 0.55, h = look.heads || (look.heads = bulkPath());
      h.moveTo(tipx, tipy);
      h.lineTo(basex - tdy*hw, basey + tdx*hw);
      h.lineTo(basex + tdy*hw, basey - tdx*hw);
    }
    }
    for (const l of [...looks.values()].sort((p, q) => p.alpha - q.alpha)) {
      bulk.globalAlpha = l.alpha; bulk.strokeStyle = l.color; bulk.lineWidth = l.width;
      bulk.stroke(l.line);
      if (l.heads) { bulk.fillStyle = l.color; bulk.fill(l.heads); }
    }
  }
  // Nodes, one path per look (Section 26.5): the decorations under a node first - the selection's
  // halo and this session's footprint glow, soft discs, so the rings stay trust's alone (Section 5) -
  // then, for each opacity, every halo as a disc of the surface's color and every fill of one type's
  // color as one path, then the rings and the group mode's bridge marks over them. A node's halo was
  // a stroke of its own; a disc under a smaller fill draws the same ring, and draws it in one fill.
  // Each node in the layer of its opacity: lit or dimmed, by its weight in a crossfade.
  const layersOf = new Map();
  for (const n of vis) {
    const on = typeHl ? n.type === typeHl
      : edgeTypeHl ? lgEndpoints.has(n.id)
      : act ? act.ns.has(n.id) : kept ? kept.has(n.id) : true;
    const base = stepped((on && !trustOff.has(n.trust_tier) ? 1 : DIM_NODE) * n._o), r = rOf(n);
    const layer = layersOf.get(base);
    if (layer) layer.push(n); else layersOf.set(base, [n]);
    if (n === anchor) {
      bulk.globalAlpha = base * 0.2; bulk.fillStyle = INK;
      bulk.beginPath(); bulk.arc(n.x, n.y, r + 8, 0, 7); bulk.fill();
    }
    // The footprint is a light centred on the node that fades out. It was a disc dropped below the
    // node, which read as a circle out of place; a crisp disc on the centre would read as a ring, and
    // rings belong to trust.
    if (showFootprint && footprint.has(n.id) && !n._sq) {
      const glow = bulk.createRadialGradient(n.x, n.y, r * 0.8, n.x, n.y, r + 13);
      const teal = LIGHT.matches ? "29,122,106" : "86,179,162";   // --ok in each appearance
      glow.addColorStop(0, "rgba(" + teal + "," + (LIGHT.matches ? 0.35 : 0.55) + ")");
      glow.addColorStop(1, "rgba(" + teal + ",0)");
      bulk.globalAlpha = base; bulk.fillStyle = glow;
      bulk.beginPath(); bulk.arc(n.x, n.y, r + 13, 0, 7); bulk.fill();
    }
  }
  for (const [base, group] of [...layersOf].sort((p, q) => p[0] - q[0])) {
    bulk.globalAlpha = base;
    // Halos: the surface's color, one fill, as wide as the node's size allows (Section 26.5): from none
    // on a small node, which is its color, to its full width. A square node has none.
    const halo = bulkPath();
    for (const n of group) {
      if (n._sq || n._d === 0) continue;
      const r = rOf(n) + nodeStrokeW(n) * n._d / 2;
      halo.moveTo(n.x + r, n.y); halo.arc(n.x, n.y, r, 0, 7);
    }
    bulk.fillStyle = SURFACE; bulk.fill(halo);
    // Fills: one path per type's color - a disc inside the halo, or a square of the node's size.
    const fills = new Map();
    for (const n of group) {
      const color = typeColor[n.type] || OTHER;
      let f = fills.get(color);
      if (!f) { f = bulkPath(); fills.set(color, f); }
      const r = rOf(n);
      if (n._sq) { f.rect(n.x - r, n.y - r, 2 * r, 2 * r); continue; }
      const inner = Math.max(0.5, r - nodeStrokeW(n) * n._d / 2);
      f.moveTo(n.x + inner, n.y); f.arc(n.x, n.y, inner, 0, 7);
    }
    for (const [color, f] of fills) { bulk.fillStyle = color; bulk.fill(f); }
    // A trust ring and a bridge mark fade in with the node's size.
    for (const n of group) {
      if (n._f === 0) continue;
      bulk.globalAlpha = stepped(base * n._f);
      trustRing(n, rOf(n));
      // Group mode: a bridge node (linked to another group) carries a small square at its upper left -
      // a cross-group transit point, marked without a ring.
      if (clusterMode && bridgeSet.has(n.id)) {
        const r = rOf(n), q = 5 / cam.s;
        bulk.fillStyle = INK; bulk.fillRect(n.x - r * 0.8 - q, n.y - r * 0.8 - q, q, q);
      }
    }
    bulk.globalAlpha = base;
  }
  // Contested beliefs (resolution.md R6): a badge at the node's upper right, drawn in its own pass at
  // full strength, so neither focus dimming nor label thinning can mute it (U3). The inspector shows
  // the competing values with the mediation act.
  ctx.globalAlpha = 1; bulk.globalAlpha = 1;
  for (const n of vis) {
    if (!n.contested) continue;
    const r = rOf(n), br = 7 / cam.s, bx = n.x + r * 0.75, by = n.y - r * 0.75;
    ctx.beginPath(); ctx.arc(bx, by, br, 0, 7);
    ctx.fillStyle = BAD; ctx.fill(); ctx.lineWidth = 1.5 / cam.s; ctx.strokeStyle = SURFACE; ctx.stroke();
    alertMark(ctx, bx, by, br, SURFACE, cam.s);
  }
  // Event pulses (nodes the agent touched) - an expanding, fading ring. rAF always runs, so it keeps
  // animating even after cooling.
  for (const [id, ttl] of pulses) {
    const n = nodeById(id);
    if (!n || ttl <= 0 || typeOff.has(n.type)) { pulses.delete(id); continue; }
    if (showPulses && !REDUCED_MOTION && onScreen(n)) {
      const t = 1 - ttl/60;
      ctx.globalAlpha = (1 - t) * 0.85;
      ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 3 + t*22, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = EDGE_HI; ctx.stroke();
    }
    pulses.set(id, ttl - 1);   // expiry advances even when hidden (avoids afterimages on toggle)
  }
  ctx.globalAlpha = 1;

  // Proposal preview (belief-diff visualization): when a proposal is selected, show on the graph what it
  // would change. For entity_merge: an arrow from each fold-away target to the canonical `into`,
  // and the target's incident edges accented (they rewire); everything the merge does not touch is
  // dimmed by activeSet, and no node is ringed - rings are trust's (design-system.md Section 5). For an open proposal both nodes are still present (a before-preview); for a merged one the
  // targets are already folded away, so only the canonical is highlighted (the result).
  if (proposalSel && proposalSel.kind === "entity_merge") {
    const into = nodeById(proposalSel.into);
    const tgts = (proposalSel.targets || []).map(nodeById).filter(n => n && (!into || n.id !== into.id));
    const tgtIds = new Set(tgts.map(n => n.id));
    const ACC = GOLD;
    for (const e of edges) {   // edges that will rewire onto the canonical: thin, under the fold arrow
      if (tgtIds.has(e.a.id) || tgtIds.has(e.b.id)) {
        ctx.globalAlpha = 0.75; ctx.strokeStyle = ACC; ctx.lineWidth = 1.2/cam.s;
        ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke();
      }
    }
    ctx.globalAlpha = 1;
    for (const t of tgts) {
      if (into) {   // fold arrow target -> into, with an arrowhead near into
        const dx = into.x - t.x, dy = into.y - t.y, d = Math.hypot(dx, dy) || 1, ux = dx/d, uy = dy/d;
        const tipx = into.x - ux*(nodeRadius(into) + 6), tipy = into.y - uy*(nodeRadius(into) + 6);
        ctx.strokeStyle = ACC; ctx.lineWidth = 2.5/cam.s;
        ctx.beginPath(); ctx.moveTo(t.x + ux*(nodeRadius(t) + 4), t.y + uy*(nodeRadius(t) + 4)); ctx.lineTo(tipx, tipy); ctx.stroke();
        const hl = Math.min(Math.max(8, nodeRadius(into) * 0.7), 16), hw = hl * 0.55;   // world units - scales with zoom like the edge arrowheads
        ctx.fillStyle = ACC; ctx.beginPath();
        ctx.moveTo(tipx, tipy);
        ctx.lineTo(tipx - ux*hl - uy*hw, tipy - uy*hl + ux*hw);
        ctx.lineTo(tipx - ux*hl + uy*hw, tipy - uy*hl - ux*hw);
        ctx.closePath(); ctx.fill();
      }
    }
    ctx.globalAlpha = 1;
  }

  // Proposal preview - T-Box change (belief-diff hint via affected_types): accent every edge whose
  // relation kind is being (re)defined; the nodes whose entity type is stay lit while activeSet dims the
  // rest, and none is ringed (design-system.md Section 5). A tbox_change edits
  // type definitions, which are edge kinds / node types (not first-class nodes), so the change shows as
  // a highlight over the members of those types rather than as a fold arrow. Kinds hidden via the legend
  // stay hidden (respect typeOff/edgeTypeOff) so the preview never contradicts the visible graph.
  if (proposalSel && proposalSel.affected_types && proposalSel.affected_types.length) {
    const { rel } = affectedTypeSets(proposalSel);
    const ACC = GOLD;
    ctx.strokeStyle = ACC; ctx.globalAlpha = 0.95; ctx.lineWidth = 2.5/cam.s;
    if (rel.size) for (const e of edges) {
      if (typeOff.has(e.a.type) || typeOff.has(e.b.type) || edgeTypeOff.has(e.type)) continue;
      if (rel.has(e.type)) { ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke(); }
    }
    ctx.globalAlpha = 1;
  }

  // Labels (nodes + hulls) - turned on/off by the labels toggle. In screen coordinates (DPR), so constant size regardless of zoom.
  if (showLabels) {
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    // Label thinning by what is on screen: every name when 40 or fewer nodes are in view, otherwise
    // only hubs (degree >= cut) + hover/focus/active, the cut set by the nodes in view rather than the
    // graph's - zooming into a large graph's cluster then names it progressively, where a cut from the
    // whole graph left it unnamed until 1.4x. Label avoidance still keeps the names apart.
    const wl = insetL(), wr = innerWidth - insetR(), wb = innerHeight - insetB();
    const inView = nodesIn((wl - cam.x) / cam.s, -cam.y / cam.s, (wr - cam.x) / cam.s, (wb - cam.y) / cam.s).length;
    const cut = inView <= 40 ? 0 : Math.max(4, Math.round(inView / 25));
    const want = [];
    // A name reaches up to 600 pixels right of its node, so the box it is looked for in reaches as far left.
    const named = few || nodesIn((-605 - cam.x) / cam.s - NODE_R_MAX, (-20 - cam.y) / cam.s, (innerWidth - cam.x) / cam.s, (innerHeight + 20 - cam.y) / cam.s);
    for (const k of named) {
      const n = nodes[k];
      if (typeOff.has(n.type)) continue;
      // Legend hover: label exactly the highlighted set (bypasses the degree cut - hover is transient).
      const lg = typeHl ? n.type === typeHl : edgeTypeHl ? lgEndpoints.has(n.id) : null;
      if (lg === false) continue;
      const on = lg === true ? true : (act ? act.ns.has(n.id) : true);
      const show = on && (lg === true || n === hover || n === focus || (act && act.ns.has(n.id)) || (hov && hov.ns.has(n.id)) || (n.degree || 0) >= cut);
      if (!show) continue;
      const o = weightNow(n);
      if (o === 0) continue;   // faded out
      const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = rOf(n)*cam.s;
      if (px + r + 5 > innerWidth || px + r + 5 + 600 < 0 || py < -20 || py > innerHeight + 20) continue;   // off screen
      want.push({ n, text: n.name, on, o, deg: n.degree || 0, px: px + r + 5, py });
    }
    // Labels avoid each other and the region names (design-system.md Section 5): the focused and
    // hovered node first, then by degree, and a label that would overlap one already placed is left
    // out - its node is still there, and hovering it names it.
    const rank = o => (o.n !== null && o.n === focus ? 2 : 0) + (o.n !== null && o.n === hover ? 1 : 0);
    want.sort((a, b) => rank(b) - rank(a) || b.deg - a.deg);
    ctx.font = "500 " + 12 * TEXT_SCALE + "px " + NAME_FAMILY;
    for (const o of want) {
      const w = ctx.measureText(o.text).width + 4, h = 15, x = o.px - 2, y = o.py - h / 2;
      // A name the budget places must fit, and its cell must have room (Section 26.5); the hovered and
      // focused nodes are always named.
      if (rank(o) === 0 && ((inView > 40 && labelBoxes.full(o.px, o.py)) || !labelBoxes.fits(x, y, w, h))) continue;
      labelBoxes.add(x, y, w, h);
      labelBoxes.anchor(o.px, o.py);
      ctx.globalAlpha = (o.on ? 1 : 0.25) * o.o;
      drawLabel(o.text, "500", 12 * TEXT_SCALE, (o.n !== null && (o.n === focus || o.n === hover)) ? INK : INK2, "0px", 1.5, o.px, o.py, "left");
    }
    ctx.globalAlpha = 1;
    // (Group / hull labels are drawn in their own pass right after the hulls - see drawHullLabels below.)
    // Active-edge relation labels: only the hovered/focused node's edges, shortest first, capped so a
    // hub does not flood the canvas; the overflow is summarized as "+K more" under the active node.
    if (edgeLabels && edgeLabels.length) {
      edgeLabels.sort((p, q) => p.len - q.len);
      const shown = Math.min(edgeLabels.length, EDGE_LABEL_MAX);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.font = 11 * TEXT_SCALE + "px " + KIND_FAMILY;   // relation kinds, at the scale's floor (docs/design-system.md Section 4)
      // Pill and text on whole device pixels: fillText snaps to the grid and the pill path does not, so
      // at a fractional point the text sits off-centre in its pill by a different amount every frame.
      const snap = v => Math.round(v * DPR) / DPR;
      const pill = (px, py, w) => {
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(px - w/2 - 5, py - 8, w + 10, 16, 4);
        else ctx.rect(px - w/2 - 5, py - 8, w + 10, 16);
        ctx.fill();
      };
      for (let k = 0; k < shown; k++) {
        const L = edgeLabels[k];
        const px = snap(L.mx*cam.s + cam.x), py = snap(L.my*cam.s + cam.y), w = ctx.measureText(L.text).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = L.col; ctx.fillText(L.text, px, py);
      }
      const omitted = edgeLabels.length - shown, an = focus || hover;
      if (omitted > 0 && an) {
        ctx.font = 11 * TEXT_SCALE + "px " + NAME_FAMILY;
        const t = "+" + omitted + " more", px = snap(an.x*cam.s + cam.x), py = snap(an.y*cam.s + cam.y + nodeRadius(an)*cam.s + 15), w = ctx.measureText(t).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = INK2; ctx.fillText(t, px, py);
      }
      ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.globalAlpha = 1;
    }
  }
  if (bulk === gpu) gpu.end();
  // Peer cursor-dots (server mode) - drawn last so they float above the graph and labels.
  stepPeers(); drawPeers();
  drawMinimap();
  if (animating()) { layoutDirty = true; requestFrame(); }
  else if (layoutDirty) { layoutDirty = false; saveLayout(); }   // the picture came to rest - keep it
}

function nodeAt(sx, sy) {
  const [wx, wy] = toWorld(sx, sy), reach = Math.max(NODE_R_MAX, KEEP_MIN_PX / cam.s) + 6;
  let best = null, bd = 1e9;
  for (const k of nodesIn(wx - reach, wy - reach, wx + reach, wy + reach)) {
    const n = nodes[k];
    if (typeOff.has(n.type) || (onScreen(n) ? n._o === 0 : frameKept)) continue;   // faded out (U38)
    // As large as it is drawn: a far map's node is at least KEEP_MIN_PX on the screen.
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d < Math.max(nodeRadius(n), KEEP_MIN_PX / cam.s) + 6 && d < bd) { bd = d; best = n; }
  }
  return best;
}

function showTip(n, cx, cy) {
  if (!n) { tip.style.display = "none"; return; }
  tip.style.display = "block";
  tip.style.left = Math.min(cx + 14, innerWidth - 330) + "px";
  tip.style.top = (cy + 14) + "px";
  // Node name/type come from untrusted observe calls and land in innerHTML here (Principle 18); the
  // html tag escapes every interpolation, the numeric fields included.
  // The hover card (Section 29.9): a card, not a tooltip - the entity's facts in sentences.
  const many = new Set(nodes.map(x => x.workspace)).size > 1;
  const first = String(n.description || "").split("\n")[0];
  tip.innerHTML = html`${[
    html`<b>${n.name}</b><br>`,
    html`<span class="k">${many && n.workspace ? n.type + " in " + n.workspace : n.type}</span><br>`,
    html`${plural(n.degree || 0, "relation")} - ${plural(n.sources || 0, "source")} - ${tierName(n.trust_tier)}`,
    first ? html`<div class="tdesc">${first.length > 120 ? first.slice(0, 120).trimEnd() + "..." : first}</div>` : "",
    n.contested ? html`<div class="tcontest">Two sources disagree</div>` : ""]}`;
}


// --- Interaction ---------------------------------------------------------------------
canvas.addEventListener("wheel", ev => {
  if (settling) return;   // graph is hidden behind the loader - ignore interaction until it settles
  ev.preventDefault();
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
  personMoved();
}, { passive: false });

canvas.addEventListener("mousedown", ev => {
  if (settling) return;   // graph hidden (loading) - no drag/pan/select
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; }   // press alone does not reheat - wake only once the pointer actually drags (mousemove)
  else {
    // Clicking empty canvas clears any proposal preview, but not one Review opened, which keeps its
    // bar until Back to Review or Done (Section 29.3).
    if (proposalSel && !reviewPreview) { proposalSel = null; requestFrame(); }
    panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing");
  }
});
addEventListener("mousemove", ev => {
  if (drag) {
    const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; positionsGen++;
    canvas.classList.add("grabbing");   // grabbing while dragging (Section 29.9)
    wake(0.3); showTip(null); personMoved(); return;
  }
  if (panning) {
    // Panning is instant (1:1) - move cam and camT together so easing does not drag behind.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); personMoved(); return;
  }
  // Only canvas-targeted moves drive node hover from here on. Over the chrome (docks, header,
  // statusbar) the shared #tip belongs to whatever chrome element is showing it (legend chip
  // definitions) - and node hover through an opaque panel was wrong anyway.
  if (ev.target !== canvas) return;
  if (settling) { showTip(null); return; }   // no hover while the graph is hidden behind the loader
  const ph = peerAt(ev.clientX, ev.clientY);   // peer cursor-dots sit above the graph - test them first
  if (ph) { hover = null; showPeerTip(ph.m, ph.id, ev.clientX, ev.clientY); return; }
  hover = nodeAt(ev.clientX, ev.clientY);
  // A hand over an entity, a grab hand over the ground (Section 29.9).
  canvas.classList.toggle("over", !!hover);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  if (settling) { drag = null; panning = null; canvas.classList.remove("grabbing"); return; }   // drop any gesture that spanned into a reload
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    // A node click selects it (select() puts the inspector first, the camera second: insetB() measures
    // the open panel); a click on the same node or on empty space clears it and leaves the camera,
    // since the person is moving it (Section 29.4).
    if (n && focus !== n) select(n); else clearSelection(false);
  }
  if (drag && moved) {
    wake(0.3);   // settle neighbors only after a real drag, not a plain click
    // Where it was let go reaches the layout before the node is let go, so the next report does not
    // put it back where the last frame left it.
    layoutSync();
  }
  drag = null;
  panning = null; canvas.classList.remove("grabbing");
  downPos = null;
});

// --- Search (docs/design-system.md Section 8, U6) -------------------------------------------------
// The field asks /api/search, which calls the engine function behind MCP's search_knowledge, and the
// list is what it answers, in its order: entities, then observations, under the label of the surface
// that answered (P19). No ranking is made here - a second one would answer a person differently from
// the agent beside them. Entity ids arrive canonical, so a hit is always a node the map can draw.
const searchWrap = searchEl.closest(".searchwrap");
const searchPop = document.getElementById("searchPop");
const SEARCH_MODE = {
  hybrid: ["keyword + meaning", "Searched the words and what they mean."],
  keyword: ["keyword only", "Searched the words only: there is no meaning index here, so a miss may be one the words did not catch."],
};
let searchAns = null;    // { q, ws, mode, hits, note } or { q, ws, error }
const SEARCH_LIMIT = 20;  // the path's default limit: an answer this long may have been cut (P5)
// The last five queries this page ran, newest first, offered by an empty field (Section 29.11). They
// live in the page's memory only, never in the browser's storage.
const recentQueries = [];
function noteRecent(q) {
  const at = recentQueries.indexOf(q);
  if (at >= 0) recentQueries.splice(at, 1);
  recentQueries.unshift(q);
  recentQueries.length = Math.min(recentQueries.length, 5);
}
// An empty field that has focus lists the recent queries instead of an answer.
function recentShown() { return !searchQuery() && recentQueries.length > 0; }
let searchRows = [];     // the hits in the order the list shows them
let searchSel = -1, searchSeq = 0, searchTimer = 0;   // -1 = nothing chosen yet

function searchQuery() { return searchEl.value.trim(); }
// An answer is shown only for the text in the field and the scope on the map; a workspace switch
// asks again rather than listing what another scope held.
function searchFresh() { return !!searchAns && searchAns.q === searchQuery() && searchAns.ws === currentWs(); }
function setSearchOpen(on) {
  if (on && recentShown()) { renderSearch(); }
  on = on && (searchFresh() || recentShown());
  searchPop.hidden = !on;
  searchEl.setAttribute("aria-expanded", on ? "true" : "false");
  searchLit = on && searchFresh() && searchAns.hits
    ? new Set(searchAns.hits.filter(h => h.kind === "entity").map(h => h.id))
    : null;
  requestFrame();
}
async function runSearch() {
  const q = searchQuery();
  const seq = ++searchSeq;
  // An emptied field offers the recent queries again while it has focus.
  if (!q) { searchAns = null; renderSearch(); setSearchOpen(document.activeElement === searchEl); return; }
  // The scope reads as the map's: the same workspace parameter /api/graph is asked with.
  const ws = currentWs();
  let ans;
  try {
    const r = await fetch("/api/search?q=" + encodeURIComponent(q) + (ws ? "&workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    const body = await r.json().catch(() => ({}));
    ans = r.ok ? { q, ws, mode: body.mode, hits: body.hits || [], note: body.note }
      : { q, ws, error: "search failed (" + r.status + ")" + (body.error ? " - " + body.error : "") };
  } catch (e) {
    ans = { q, ws, error: "search failed - the daemon did not answer" };
  }
  if (seq !== searchSeq) return;   // a newer query is on its way; this answer is for text no longer there
  searchAns = ans;
  if (!ans.error) noteRecent(q);
  searchSel = -1;   // the person has not moved yet - no row is chosen
  renderSearch();
  setSearchOpen(document.activeElement === searchEl);
  // Nothing here contains the words: count what the wider scope holds, without holding up the list.
  if (!ans.error && ws !== "*" && ws !== "all" && !ans.hits.some(h => h.reached === "words")) {
    const r = await searchWider(q);
    if (r && seq === searchSeq && searchAns === ans) { ans.wider = r.n; ans.widerCapped = r.capped; renderSearch(); }
  }
}
// How many items contain the words across every workspace the reader may read - the count the
// wider-scope row offers. The path's limit cuts the list, so an answer at the limit says "20+".
async function searchWider(q) {
  try {
    const r = await fetch("/api/search?q=" + encodeURIComponent(q) + "&workspace=*", { cache: "no-store" });
    if (!r.ok) return null;
    const hits = (await r.json()).hits || [];
    const n = hits.filter(h => h.reached === "words").length;
    return { n, capped: n >= 20 };   // the keyword list is cut at the limit before it is fused
  } catch (_) { return null; }
}
function searchNode(h) {
  const n = h.kind === "entity" ? nodeById(h.id) : null;
  return n && !typeOff.has(n.type) ? n : null;
}
function renderSearch() {
  if (recentShown()) {
    // An empty field: the queries this page ran, newest first; choosing one asks it again.
    searchRows = recentQueries.map(q => ({ kind: "recent", q }));
    searchSel = -1;
    searchPop.innerHTML = html`<div class="sp-sec">Recent</div>${recentQueries.map((q, i) =>
      html`<button type="button" class="sp-row" role="option" id="sp${i}" data-i="${i}" aria-selected="false"><span class="sp-name">${q}</span></button>`)}`;
    searchPop.querySelectorAll(".sp-row").forEach(b => {
      b.onmousedown = ev => ev.preventDefault();
      b.onclick = () => chooseSearch(Number(b.dataset.i));
    });
    searchEl.setAttribute("aria-activedescendant", "");
    return;
  }
  if (!searchAns) { searchRows = []; searchPop.innerHTML = html``; return; }
  if (searchAns.error) {
    searchRows = [];
    searchPop.innerHTML = html`<div class="sp-note bad">${searchAns.error}</div>`;
    return;
  }
  // Two groups, each in the engine's order (design-system.md Section 8, U15): the hits that contain
  // the words, then the related ones, each saying how it was reached.
  const words = searchAns.hits.filter(h => h.reached === "words");
  const related = searchAns.hits.filter(h => h.reached !== "words");
  // The answer kept on the map (Section 29.11, D8): the first row, when two or more of its entities
  // are drawn. It says "these", or "the first" when the answer reached the path's limit, and counts
  // apart the entity hits this view does not draw (P5).
  const ents = searchAns.hits.filter(h => h.kind === "entity");
  const drawn = ents.filter(h => searchNode(h)).map(h => h.id);
  const keep = drawn.length >= 2
    ? { kind: "keep", ids: drawn, q: searchAns.q, first: searchAns.hits.length >= SEARCH_LIMIT, apart: ents.length - drawn.length }
    : null;
  const off = keep ? 1 : 0;
  searchRows = [...(keep ? [keep] : []), ...words, ...related];
  const why = h => h.reached === "meaning" ? "near in meaning" : h.reached === "neighbor" ? "related to a match" : "";
  // Where the scope holds more than one workspace, an entity's row names its own in the hover card's
  // words, "Service in infra" (Section 29.11).
  const manyWs = new Set(nodes.map(n => n.workspace)).size > 1;
  const [modeWord, modeWhy] = SEARCH_MODE[searchAns.mode] || [String(searchAns.mode || "?"), ""];
  const row = (h, i) => {
    const on = i === searchSel ? " on" : "";
    const reason = why(h) ? html`<span class="sp-why">${why(h)}</span>` : "";
    if (h.kind === "observation")
      return html`<button type="button" class="sp-row${on}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}"><span class="sp-snip">${h.snippet}</span>${reason}</button>`;
    const n = searchNode(h);
    // An entity the answer holds but this view does not draw (its type is switched off, or the loaded
    // graph predates it) stays listed - it is part of the answer - and says why it cannot be focused.
    return html`<button type="button" class="sp-row${on}${n ? "" : " off"}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}">${
      n ? html`<span class="dot" style="background:${typeColor[n.type] || OTHER}"></span>` : ""}<span class="sp-name">${n ? n.name : h.snippet}</span>${
      n ? html`<span class="sp-k">${manyWs && n.workspace ? n.type + " in " + n.workspace : n.type}</span>` : html`<span class="sp-k">not on the map in this view</span>`}${reason}</button>`;
  };
  const count = searchAns.hits.length;
  const cur = currentWs();
  const all = cur === "*" || cur === "all";
  const scope = all ? "any workspace" : cur || defaultName();
  // Nothing in the scope contains the words: say so, and offer the wider scope with what it found.
  const widen = !all && searchAns.wider > 0
    ? html`<button type="button" class="sp-row sp-widen" data-widen="1">${"Search all workspaces (" + (searchAns.widerCapped ? "20+" : searchAns.wider) + " found)"}</button>`
    : "";
  const keepRow = keep ? html`<button type="button" class="sp-row sp-keep${searchSel === 0 ? " on" : ""}" role="option" id="sp0" data-i="0" aria-selected="${searchSel === 0}" aria-keyshortcuts="Meta+Enter"><span class="sp-name">${
    "Show these " + keep.ids.length + " on the map" + (keep.first ? " - the search stopped at " + SEARCH_LIMIT : "")}</span>${
    keep.apart ? html`<span class="sp-why">${keep.apart + (keep.apart === 1 ? " is" : " are") + " not drawn in this view"}</span>` : ""}<kbd class="sp-key">Cmd+Enter</kbd></button>` : "";
  const first = words.length
    ? [html`<div class="sp-sec">${"Contains \"" + searchAns.q + "\""}</div>`, words.map((h, i) => row(h, off + i))]
    : [html`<div class="sp-note">${"No entity in " + scope + " contains \"" + searchAns.q + "\"."}</div>`, widen];
  searchPop.innerHTML = html`<div class="sp-head"><span>${count} ${count === 1 ? "hit" : "hits"}</span><span class="sp-mode">${modeWord}</span></div><div class="sp-modewhy">${modeWhy}</div>${keepRow}${
    count || widen ? first : ""}${
    related.length ? [html`<div class="sp-sec">Related</div>`, related.map((h, i) => row(h, off + words.length + i))] : ""}${
    count ? "" : html`<div class="sp-note">${searchAns.note || "no hits - absence is unknown, not a negation"}</div>`}`;
  searchPop.querySelectorAll("[data-widen]").forEach(b => {
    b.onmousedown = ev => ev.preventDefault();
    b.onclick = () => { wsInput.value = "*"; beginWorkspaceTransition(); renderChipsActive(); poll(); runSearch(); };
  });
  searchPop.querySelectorAll(".sp-row:not(.sp-widen)").forEach(b => {
    // mousedown keeps the field focused, so the click below is the choice and not a blur first.
    b.onmousedown = ev => ev.preventDefault();
    b.onclick = () => chooseSearch(Number(b.dataset.i));
  });
  searchEl.setAttribute("aria-activedescendant", searchRows.length && searchSel >= 0 ? "sp" + searchSel : "");
}
function moveSearch(d) {
  if (!searchRows.length) return;
  // The first press chooses a row: down starts at the top, up at the bottom. After that it cycles.
  if (searchSel < 0) searchSel = d > 0 ? 0 : searchRows.length - 1;
  else searchSel = (searchSel + d + searchRows.length) % searchRows.length;
  searchPop.querySelectorAll(".sp-row").forEach(b => {
    const on = Number(b.dataset.i) === searchSel;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", on ? "true" : "false");
    if (on) b.scrollIntoView({ block: "nearest" });
  });
  searchEl.setAttribute("aria-activedescendant", "sp" + searchSel);
}
async function chooseSearch(i) {
  const h = searchRows[i];
  if (!h) return;
  if (h.kind === "recent") {
    searchEl.value = h.q; searchWrap?.classList.toggle("filled", true);
    runSearch();
    return;
  }
  if (h.kind === "keep") {
    // The answer stays on the map as the working set; the field clears (D8), and from Review the
    // Graph opens.
    searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
    closeSearch();
    if (reviewOpen()) location.hash = "";
    keepInView(h.ids, { kind: "search", q: h.q, first: h.first });
    return;
  }
  if (h.kind === "entity") {
    const n = searchNode(h);
    if (!n) return;   // listed, not drawn: the row already says so
    // Search goes somewhere (D8): the field clears, and the chosen name is the sheet's title.
    searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
    closeSearch();
    if (reviewOpen()) location.hash = "";
    select(n);
    return;
  }
  // An observation opens its card - from the Log's rows when they hold it, else read once - and the
  // canvas frames the entities it asserts.
  const o = await observationById(h.id);
  if (!o) { toast("bad", "Could not open the observation", "the log of this scope did not hold " + h.id.slice(0, 10)); return; }
  searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
  closeSearch();
  if (reviewOpen()) location.hash = "";
  showObsCard(o);
  const asserted = (o.entities || []).map(e => nodeById(e.id)).filter(Boolean);
  if (asserted.length) { fitView(asserted, 140); userMoved = true; }
}
function closeSearch() { setSearchOpen(false); searchEl.blur(); }

searchEl.addEventListener("input", () => {
  // Keep the "/" badge clear of the text once the field carries a term (CSS .searchwrap.filled).
  // Optional: the badge is decoration, so losing its wrapper must never break search itself.
  searchWrap?.classList.toggle("filled", searchEl.value !== "");
  // The list of a query no longer in the field is not shown, and the map stops lighting it.
  setSearchOpen(false);
  clearTimeout(searchTimer);
  searchTimer = setTimeout(runSearch, 160);
});
searchEl.addEventListener("focus", () => {
  if (searchQuery() && !searchFresh()) runSearch(); else setSearchOpen(true);
});
searchEl.addEventListener("blur", () => setSearchOpen(false));
searchEl.addEventListener("keydown", ev => {
  if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
    ev.preventDefault();
    if (!searchPop.hidden) moveSearch(ev.key === "ArrowDown" ? 1 : -1);
    else if (searchFresh()) setSearchOpen(true);
    else if (searchQuery()) runSearch();
  } else if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
    // Cmd+Enter keeps a fresh answer on the map; before the answer, it asks first (Section 29.11).
    ev.preventDefault();
    clearTimeout(searchTimer);
    if (searchFresh() && searchRows[0] && searchRows[0].kind === "keep") chooseSearch(0);
    else if (searchQuery() && !searchFresh()) runSearch();
  } else if (ev.key === "Enter") {
    ev.preventDefault();
    clearTimeout(searchTimer);
    // Enter on a chosen row picks it; otherwise ask first. No auto-pick of the top row.
    if ((searchFresh() || recentShown()) && searchSel >= 0) chooseSearch(searchSel);
    else if (searchQuery() && !searchFresh()) runSearch();
  } else if (ev.key === "Escape") {
    // Hand the keyboard back to the canvas, so "/" -> type -> Escape is a closed loop.
    // stopPropagation: the window handler reads Escape as "dismiss the obs card / view popover",
    // which is not what leaving the search field should do.
    ev.stopPropagation();
    closeSearch();
  }
});
// Retry where the map is stale (Section 29.12): the workspaces and the map at once; a Retry that fails
// too says so.
let retryAsked = null;   // when Retry was last pressed, until a read succeeds
const retryBtn = document.getElementById("retry");
retryBtn.onclick = () => {
  retryAsked = new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  loadWorkspaces(); poll();
};
// The zoom buttons zoom about the map window's centre, which is not the canvas's when a panel is open.
function zoomWindow(f) { const w = mapWindow(); zoomAt((w.l + w.r)/2, (w.t + w.b)/2, f); personMoved(); }
document.getElementById("zin").onclick = e => { if (idle(e)) return; zoomWindow(1.2); };
document.getElementById("zout").onclick = e => { if (idle(e)) return; zoomWindow(1/1.2); };
document.getElementById("fit").onclick = e => {
  if (idle(e)) return;
  userMoved = true;
  // With a working set pinned, Fit frames the set (Section 29.11).
  const kept = keptDrawn();
  fitView(kept.length ? kept : undefined); personMoved();
};
// Follow on the camera toolbar (Section 29.4): a press turns it on or off, waiting or not.
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; renderFollow(); };
setInterval(renderFollow, 1000);
document.getElementById("resetLayout").onclick = () => { setViewPop(false); resetLayout(); };
const peersBtn = document.getElementById("peersBtn");
peersBtn.onclick = () => { peersOn = !peersOn; setSwitch(peersBtn, peersOn); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; setSwitch(clusterBtn, clusterMode); wake(0.6); poll(); };
// The view menu's stored choices: each switch, the Regions choice and the single keys.
const VIEW_PREFS = ["followBtn", "clusterBtn", "labelsBtn", "edgesBtn", "arrowsBtn", "footBtn",
  "pulseBtn", "histBtn", "miniBtn", "keysBtn"];
let viewPrefsReady = false;   // false while stored choices are replayed, so the replay stores nothing
function saveViewPrefs() {
  if (!viewPrefsReady) return;
  const now = { v: 5, regions: regionsValue(), boundaries: boundaryMode };
  for (const k of VIEW_PREFS) { const x = document.getElementById(k); if (x) now[k] = k === "followBtn" ? follow : x.classList.contains("on"); }
  try { localStorage.setItem("supra.view", JSON.stringify(now)); } catch (_) { /* private mode - the choice lasts the session */ }
}
// Regions: All, Selected or Off (design-system.md Section 5) - one choice over the two switches it
// replaced, "hulls" and "all regions". Render-only (the draw loop shows it next frame), so the sim is
// not reheated; the hyperedges are fetched only if they are not already here.
const regionsSeg = document.getElementById("regionsSeg");
function regionsValue() { return !hyperMode ? "off" : allRegions ? "all" : "selected"; }
function setRegions(v) {
  hyperMode = v !== "off"; allRegions = v === "all";
  if (hyperMode && !hyperedges.length) poll();
  renderRegionsSeg(); saveViewPrefs(); requestFrame();
}
// What each choice of regions draws, said under the choice rather than in a tooltip (Section 4, U25).
const REGION_WHY = {
  all: "Every region faint, and the active entity's at full strength.",
  selected: "Only the regions of the entity you point at or select.",
  off: "No regions.",
};
function renderRegionsSeg() {
  renderMarks();
  for (const b of regionsSeg.querySelectorAll("button")) b.classList.toggle("on", b.dataset.v === regionsValue());
  radioGroup(regionsSeg, "Regions");
  const desc = document.getElementById("regionsDesc");
  if (desc) desc.textContent = REGION_WHY[regionsValue()] || "";
}
regionsSeg.querySelectorAll("button").forEach(b => { b.onclick = () => setRegions(b.dataset.v); });
// The single keys can be turned off (Section 10, WCAG 2.1.4); the switch is in the Keys dialog, beside
// the keys it governs (Section 29.12, the author's decision).
let singleKeys = true;
const keysBtn = document.getElementById("keysBtn");
keysBtn.onclick = () => { singleKeys = !singleKeys; setSwitch(keysBtn, singleKeys); renderKeyNames(); };
const keysDlg = document.getElementById("keysDlg");
function openKeys() { setViewPop(false); if (!keysDlg.open) keysDlg.showModal(); }
document.getElementById("keysOpen").onclick = openKeys;
function dialogOpen() { return !!document.querySelector("dialog[open]"); }
// A control names its key only while the single keys are on (Section 9: one line never states two
// contradictory states).
function renderKeyNames() {
  const k = key => singleKeys ? key : null;
  const name = (id, label, key) => { const b = document.getElementById(id); if (b) b.title = tipText(label, k(key)); };
  name("zin", "Zoom in", "="); name("zout", "Zoom out", "-");
  name("mapTab", "Map", "l"); name("listBtn", "List", "l");
  name("panelL", "Filters panel", "["); name("panelR", "Log panel", "]");
  renderKept();   // Fit's name and key follow the working set
  for (const el of document.querySelectorAll(".slashkey, #viewpop .keyhint")) el.hidden = !singleKeys;
}
// Relations off leaves Arrows and History nothing to draw: they are dimmed and say why (Section 4).
function renderShowDeps() {
  for (const id of ["arrowsBtn", "histBtn"]) {
    const b = document.getElementById(id);
    if (!b) continue;
    if (showEdges) { b.removeAttribute("aria-disabled"); b.setAttribute("aria-describedby", id === "arrowsBtn" ? "dArrows" : "dHist"); }
    else { b.setAttribute("aria-disabled", "true"); b.setAttribute("aria-describedby", "dShownWithRel"); }
  }
  // More's summary names a switch under it that is off.
  const off = [["footBtn", "Footprint"], ["pulseBtn", "Pulses"]].filter(([id]) => !document.getElementById(id).classList.contains("on")).map(([, w]) => w);
  document.getElementById("moreSum").textContent = off.length ? "More - " + off.join(" and ") + " off" : "More";
}
// The panels and their title-bar toggles (docs/design-system.md Section 6): each panel opens and
// closes on its own, with its toggle or [ and ], and the choice persists across reloads. A panel is
// part of the frame beside the map's window - body.dl / body.dr move the window's edges - and
// toggling one never reheats the sim.
const panelLBtn = document.getElementById("panelL"), panelRBtn = document.getElementById("panelR");
function applyDock(isLeft, on, remember = true) {
  (isLeft ? dockLEl : dockREl).classList.toggle("on", on);
  (isLeft ? panelLBtn : panelRBtn).setAttribute("aria-pressed", on ? "true" : "false");
  document.body.classList.toggle(isLeft ? "dl" : "dr", on);
  if (isLeft) renderTierNote();
  else if (on) refreshLog();
  // Closing the Log fetches the strip that takes its place at once, not at the next poll.
  if (!isLeft && !on) refreshStrip();
  else renderStrip();
  if (remember) {
    try { localStorage.setItem(isLeft ? "supra.dockL" : "supra.dockR", on ? "1" : "0"); } catch (_) { /* private mode */ }
  }
  // Under 1200px the canvas holds one panel at a time (design-system.md Section 6): opening a dock
  // closes the other and the inspector. Not remembered - the wide window's choice stands.
  if (on && narrowWindow()) {
    applyDock(!isLeft, false, false);
    clearSelection(false);
  }
}
function narrowWindow() { return innerWidth < 1200; }
function toggleDock(isLeft) { applyDock(isLeft, !(isLeft ? dockLEl : dockREl).classList.contains("on")); }
panelLBtn.onclick = () => toggleDock(true);
panelRBtn.onclick = () => toggleDock(false);
// View options popover (HUD "view" button): preferences stay off the data rails; a click
// elsewhere or Escape dismisses it.
const viewBtn = document.getElementById("viewBtn"), viewPop = document.getElementById("viewpop");
function setViewPop(on) {
  if (!on && viewPop.contains(document.activeElement)) viewBtn.focus();
  viewPop.classList.toggle("on", on); viewBtn.classList.toggle("on", on);
  viewBtn.setAttribute("aria-expanded", on ? "true" : "false");
  if (on) { const first = viewPop.querySelector("button"); if (first) first.focus(); }
}
viewBtn.onclick = ev => { ev.stopPropagation(); setViewPop(!viewPop.classList.contains("on")); };
document.addEventListener("click", ev => { if (viewPop.classList.contains("on") && !viewPop.contains(ev.target)) setViewPop(false); });
// [ / ] toggle the docks, "/" jumps to search - but never while typing in a field.
window.addEventListener("keydown", e => {
  // Cmd+K (Ctrl+K off the Mac) opens search from anywhere, a field included - it types nothing.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "k") {
    e.preventDefault();
    searchEl.focus();
    searchEl.select();
    return;
  }
  if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
  // A dialog holds the keyboard: its own Esc closes it alone (Section 10).
  if (dialogOpen()) return;
  // Cmd+[ and Cmd+] step through the selections on the Graph page (Section 29.4); the Review map
  // preview's own way back is a capturing listener and comes first while its bar shows.
  if ((e.key === "[" || e.key === "]") && (e.metaKey || e.ctrlKey) && !e.altKey && !reviewOpen()) {
    e.preventDefault();
    stepSelection(e.key === "[" ? -1 : 1);
    return;
  }
  // The single keys (Section 29.12): without Cmd, Ctrl or Option; a letter by what it types, and by
  // the key's place only when the input source types a letter that is not Latin (b and l under Hangul).
  const bare = !e.metaKey && !e.ctrlKey && !e.altKey;
  const k = /^[\x20-\x7e]$/.test(e.key) ? e.key : e.code === "KeyB" ? "b" : e.code === "KeyL" ? "l" : e.key;
  const single = bare && ["[", "]", "/", "b", "=", "+", "-", "0", "l", "?"].includes(k);
  if (single && !singleKeys) return;   // turned off in the Keys dialog (WCAG 2.1.4)
  // "/" opens search on the Review page too; the map's keys act only on the Graph page with no card open.
  const onMap = !reviewOpen() && !obscardEl.classList.contains("on");
  const press = id => { const b = document.getElementById(id); if (b && b.getAttribute("aria-disabled") !== "true") b.click(); };
  if (single && k === "/") {
    // preventDefault or the "/" itself lands in the field as the first character (and Firefox
    // opens its quick-find). The INPUT/TEXTAREA guard above already keeps this from hijacking
    // a "/" typed into any other field. select() so the shortcut replaces a stale term.
    e.preventDefault();
    searchEl.focus();
    searchEl.select();
  }
  else if (single && !onMap) return;
  else if (single) {
    e.preventDefault();
    if (k === "[") toggleDock(true);
    else if (k === "]") toggleDock(false);
    else if (k === "=" || k === "+") press("zin");
    else if (k === "-") press("zout");
    else if (k === "0") press("fit");
    else if (k === "l") press(listOn ? "mapTab" : "listBtn");
    else if (k === "?") openKeys();
    else if (k === "b") cycleBoundaries();
  }
  // Esc closes the topmost thing: the observation card, then the view menu, then the inspector.
  else if (e.key === "Escape") {
    if (listOn) setList(false);   // List goes back to Map first, from the table or its tab (Section 29.11)
    else if (obscardEl.classList.contains("on")) hideObsCard();
    else if (viewPop.classList.contains("on")) setViewPop(false);
    else if (focus) clearSelection(true);   // and the camera goes back to before the walk (Section 29.4)
    else if (workSet) clearWorkSet();       // the working set ends last (Section 29.11)
  }
});
// Restore the persisted state (default: both open).
{
  let keptL = true, keptR = true;
  try {
    keptL = localStorage.getItem("supra.dockL") !== "0";
    keptR = localStorage.getItem("supra.dockR") !== "0";
  } catch (_) { /* private mode - the defaults stand */ }
  applyDock(true, keptL, false);
  applyDock(false, keptR, false);
}
// Fetch the glossary lazily when its section is expanded (and keep it fresh via poll while open).
// Dock tabs: clicking a tab shows only that panel (one at a time, fixed-height body - no upward growth),
// and refreshes the newly active data panel.
document.querySelectorAll(".dock .tabs").forEach(tabs => {
  const dock = tabs.closest(".dock");
  for (const t of tabs.querySelectorAll(".tab")) {
    t.id = "tab-" + t.dataset.tab;
    const panel = dock.querySelector(`.tabpanel[data-panel="${t.dataset.tab}"]`);
    if (panel) { panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", t.id); }
  }
  markTabs(tabs, [...tabs.querySelectorAll(".tab")], tabs.querySelector(".tab.on"));
  arrowTabs(tabs);
  tabs.addEventListener("click", ev => {
    const btn = ev.target.closest(".tab");
    if (!btn) return;
    tabs.querySelectorAll(".tab").forEach(t => t.classList.toggle("on", t === btn));
    markTabs(tabs, [...tabs.querySelectorAll(".tab")], btn);
    dock.querySelectorAll(".tabpanel").forEach(p => p.classList.toggle("on", p.dataset.panel === btn.dataset.tab));
    if (btn.dataset.tab === "review") refreshCuration();
    else if (btn.dataset.tab === "proposals") refreshProposals();
    else if (btn.dataset.tab === "log") refreshLog();
  });
});
arrowTabs(document.getElementById("settingsTabs"));
// Pure render toggles (no layout/data change - rAF reflects them every frame, so wake/poll is unnecessary).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; setSwitch(e.currentTarget, showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; setSwitch(e.currentTarget, showEdges); renderShowDeps(); };
// Arrows and History act on relations: while Relations is off they are idle (a stored choice still replays).
document.getElementById("arrowsBtn").onclick = e => { if (viewPrefsReady && idle(e)) return; showArrows = !showArrows; setSwitch(e.currentTarget, showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; setSwitch(e.currentTarget, showFootprint); renderMarks(); renderShowDeps(); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; setSwitch(e.currentTarget, showPulses); renderMarks(); renderShowDeps(); };
document.getElementById("histBtn").onclick = e => { if (viewPrefsReady && idle(e)) return; showSuperseded = !showSuperseded; setSwitch(e.currentTarget, showSuperseded); };
document.getElementById("settingsBtn").onclick = openSettings;
// The sync pill opens the settings at Sync. In the app the shell takes this click to the app's own
// Settings > Sync page, as it takes the gear's.
document.getElementById("syncPill").onclick = () => { settingsTab = "sync"; openSettings(); };
document.getElementById("miniBtn").onclick = e => { showMini = !showMini; setSwitch(e.currentTarget, showMini); };

// The contested pill and the review pill open the Review page, where contested beliefs wait under
// Needs you.
document.getElementById("contestedPill").onclick = () => { reviewSeg = "needs"; location.hash = "#review"; };
document.querySelector("#kept .kx").onclick = clearWorkSet;
document.getElementById("showAll").onclick = showAll;
document.getElementById("reviewPill").onclick = () => { reviewSeg = "needs"; location.hash = "#review"; };
// The view menu's choices persist, as the docks' open state does (docs/design-system.md Section 14
// step 1): a choice made once stays made. A preference, so it lives in this browser and nowhere else.
// peers is left out on purpose: it switches itself on for a hub, so a stored "off" would fight that.
{
  for (const b of document.querySelectorAll("button.tog")) setSwitch(b, b.classList.contains("on"));
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem("supra.view") || "{}") || {}; } catch (_) { saved = {}; }
  // Any click stored every choice, so a stored minimap is rarely one anyone made: 0.6.0 and 0.6.1
  // started it off and stored that on the first click of any toggle. Forget it once more now that it
  // starts on (docs/design-system.md Section 6), and keep it from the next click. "all regions" is
  // the same case from 0.6.4, when it starts on: a stored "off" before then is forgotten once.
  if (!(saved.v >= 3)) delete saved.miniBtn;
  if (!(saved.v >= 4)) delete saved.regionsBtn;
  // Before 0.7.0 Regions was two switches, "hulls" and "all regions"; their stored choices become the
  // one choice now (Section 5).
  if (!saved.regions && (typeof saved.hyperBtn === "boolean" || typeof saved.regionsBtn === "boolean")) {
    saved.regions = saved.hyperBtn === false ? "off" : saved.regionsBtn === false ? "selected" : "all";
  }
  viewPrefsReady = false;
  for (const id of VIEW_PREFS) {
    const b = document.getElementById(id);
    if (!b) continue;
    // Replay a stored choice through the button's own handler, so its state and side effects agree.
    const on = id === "followBtn" ? follow : b.classList.contains("on");   // Follow's look also says "paused"
    if (typeof saved[id] === "boolean" && saved[id] !== on) b.click();
    b.addEventListener("click", saveViewPrefs);
  }
  if (["all", "selected", "off"].includes(saved.regions)) setRegions(saved.regions);
  else renderRegionsSeg();
  // Boundaries is stored since it only draws (Section 29.12).
  if (["workspace", "origin", "off"].includes(saved.boundaries)) { boundaryMode = saved.boundaries; renderBoundaryChips(); }
  viewPrefsReady = true;
  renderKeyNames(); renderShowDeps();
}

// Restore the workspace selection from the URL (deep link / reload) before the first poll.
{
  const qws = new URLSearchParams(location.search).get("workspace");
  if (qws !== null) wsInput.value = qws;
}
resize(); syncSearchScope(); loadWorkspaces(); loadSurface().finally(poll); connectEvents(); detectServerMode();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); setInterval(refreshPeerRoster, 5000); draw();
loadSurface().then(() => { refreshSyncPill(); renderReview(); syncSearchScope(); loadWorkspaces(); renderStats(); }); setInterval(refreshSyncPill, 5000);
