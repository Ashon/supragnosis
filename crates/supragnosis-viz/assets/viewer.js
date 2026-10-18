"use strict";
// --- Categorical color generator (shared by node/edge/hull) -------------------------
// Spread hue as far apart as possible by the golden angle (137.508deg), and rotate
// saturation/lightness in steps so that even with many kinds (beyond a fixed palette's limit) they
// stay distinguishable. For dark-background readability, lightness 53-81% / saturation 62-92%.
// Edges are lines, so lightness is raised a little to distinguish them from nodes. Deterministic
// function (same index -> same color). Note: with very many categories, perceptual
// distinguishability has limits no matter the method.
function catColor(i, edge) {
  const h = (i * 137.508) % 360;
  // Lower saturation (roughly 50-74%) so it does not clash on either light/dark, and keep lightness
  // in the mid range (securing contrast on both backgrounds). Edges are lines, so only lightness is
  // raised a little to distinguish them from nodes.
  const l = edge ? [70, 62, 73][i % 3] : [58, 66, 50][i % 3];
  const s = [62, 50, 74][(i / 3 | 0) % 3];
  return `hsl(${h | 0}, ${s}%, ${l}%)`;
}
const OTHER = "#8e96a5", EDGE_OTHER = "#5c6472";   // defensive neutral color for types not in the map
const EDGE = "#2b3345", EDGE_HI = "#f0c469", EDGE_OLD = "#4d4340";
// A relation at rest is drawn in one neutral; its kind's color is for the active node's edges and a
// relation kind the legend points at (docs/design-system.md Section 5). Thirty kinds in thirty hues
// say nothing at a glance; one hue says "connected".
const EDGE_NEUTRAL = "#6b7383";
const EDGE_ALPHA = 0.42;         // edge base opacity (low - recedes in a dense graph; raised for the darker ink). On hover/focus, connected edges activate to 1.0
// The node stroke is proportional to the marker radius (scales with the marker on zoom - a
// consistent ratio) + a screen-px floor (so it does not vanish on zoom-out). It is a background-color
// halo, separating node/edge/neighbor (visibility).
const NODE_STROKE_RATIO = 0.35;  // stroke thickness ratio relative to radius (raised)
const NODE_STROKE_MIN = 2;       // minimum stroke thickness (screen px)
const NODE_STROKE_MAX = 5;       // maximum stroke thickness (screen px) - so a large hub does not thicken into a donut
// Canvas palette - mirrors the CSS tokens (the landing's candlelight theme). SURFACE doubles as the
// halo/cutout color, so it must match the body background exactly.
const INK = "#e9e4d6", INK2 = "#aab1bd", SURFACE = "#08090d";
const GOLD = "#d9a544", TEAL = "#56b3a2", BAD = "#e07a5f";   // --accent, --ok, --bad

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
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
const loaderEl = document.getElementById("loader");
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
// Graphic-element visibility toggles (all default on). Pure render switches with no effect on layout.
let showLabels = true, showEdges = true, showArrows = true;
let flowPhase = 0;   // per-frame counter driving the marching-dash flow animation on active edges
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
const CLUSTER_PULL = 0.03;       // pull toward the group target point (stronger than the center attraction)
const HYPER_PULL = 0.03;         // base hyperedge centroid cohesion (scaled by hull size - see hullSizeNorm)
// Cohesion scales with member count: larger hulls pull their nodes tighter (small ones stay loose).
const HULL_SIZE_REF = 8;         // members at/above which a hull gets full cohesion weight
const HULL_COH_MIN = 0.4, HULL_COH_MAX = 1.15;    // cohesion factor range (x HYPER_PULL)
// Hull rendering: a region is a cloud (docs/design-system.md Section 5) - one ellipse fitted to its
// members and filled with a radial gradient that is densest at their centre and fades to nothing past
// the outermost, so it has no edge to read as a line. A flat fill of the hull drew a slab whose rim
// looked like a border. One gradient fill per region, directly on the canvas: no stroke, no blur
// filter, nothing composited offscreen.
const HULL_LAYER_ALPHA = 0.10;   // a region's density at its core (no node active) - every region is drawn then, so they stay faint
const HULL_LAYER_DIM = 0.04;     // non-active regions fade to this while a node is active (inspection view)
const HULL_ACTIVE_ALPHA = 0.26;  // the active node's own regions, painted on top
const CLOUD_REACH = 1.8;         // how far the cloud reaches past its outermost member, x the hull's node gap
const HULL_LABEL_BASE = 0.7;     // hull label opacity with no active node
const HULL_LABEL_HOVER_FADE = 0.15; // non-member hull labels fade to this while a node is active
const HULL_NODE_GAP = 14;        // world px: extra gap past the largest member glyph when expanding a hull
const HULL_PAD = 24;             // target gap between hulls (world px). Kept small to avoid over-separation at high density
const HULL_SEP = 0.008;          // separation force between hulls (gentle - scaled by cooling alpha)
const HULL_R_CAP = 160;          // upper bound on the hull radius used for separation - so a huge grab-bag cannot push the whole layout
const HULL_MAX_PUSH = 4;         // per-frame separation displacement cap per hull - prevents divergence accumulating across many pairs
let footprintSession = null;     // the session (conversation) the current footprint belongs to
const footprint = new Set();     // ids of nodes this session touched - the conversation's knowledge footprint
let nodes = [], edges = [], typeColor = {}, edgeTypeColor = {};
const posById = new Map();       // id -> {x,y,vx,vy} - layout stability across polls
const typeOff = new Set();       // node types hidden from the legend
const edgeTypeOff = new Set();   // edge kinds (relation kind) hidden from the legend
let spiralN = 0;
let drag = null, hover = null, focus = null;
let searchLit = null;   // the entity ids the open search answered with (Section 8); null while closed
// Camera: cam = current (drawn), camT = target. Each frame, ease cam toward camT to make
// zoom/pan/focus/fit smooth (removing instant jumps). Coordinates are CSS pixels (same system as mouse events).
let DPR = 1;
const cam = { s: 1, x: 0, y: 0 }, camT = { s: 1, x: 0, y: 0 };
let panning = null, downPos = null, userMoved = false, firstData = true, needFit = false;

// --- force simulation (alpha cooling + collision separation) ------------------------------------
let alpha = 1;
// alpha decays multiplicatively (x0.9772 a frame), so ALPHA_MIN buys time logarithmically: 0.02
// applies force for ~170 frames from a cold load, 0.008 for ~209. Lowered because the tail of the
// layout is where it is still worth nudging - the forces down there are tiny, and stopping while
// they are still meaningful is what made the settle read as a cut rather than a stop. The coasting
// afterwards is not governed by this at all; that is DAMPING, and it is measured (see simMotion).
const ALPHA_DECAY = 0.0228, ALPHA_MIN = 0.008;
// Layout-loader gating: while the sim is reheated at/above SETTLE_ENTER (initial load, data change,
// group toggle - the big rearrangements), the graph is hidden behind a loader; it is revealed once
// alpha cools to REVEAL_ALPHA. Small wakes (drag/focus at 0.3) stay below SETTLE_ENTER, so those never
// trigger the loader. `settling` starts true so the first layout comes up settled, not mid-flight.
const SETTLE_ENTER = 0.5, REVEAL_ALPHA = 0.08;
// Largest distance any node moved on the last step, in world units - the settle test the cooling
// schedule cannot give, since alpha says how hard the sim is pushing and not whether anything moved.
let simMotion = 0;
const MOTION_MIN_PX = 0.05;   // screen px per frame below which the picture is not changing
// Reduced motion (same respect the landing pays to prefers-reduced-motion): instead of animating
// the violent early rearrangement behind a loader, burst-step the sim to convergence within one
// frame and reveal the layout already still.
const REDUCED_MOTION = matchMedia("(prefers-reduced-motion: reduce)").matches;
let settling = true;
let refitOnReveal = false;   // re-frame the graph after a sync-driven re-layout settles (follow mode)
// Base force parameters. The larger the graph, the wider it should spread, so stepSim scales by node count (spread).
const REPULSE = 7000, SPRING_LEN = 120, SPRING_K = 0.02;
const CENTER_BASE = 0.0015; // center-attraction base - weakened for large graphs (prevents central clumping)
const ANCHOR_K = 0.5;       // central-axis anchor: fraction of the centroid-offset corrected each frame (rigid recenter, positions only) - pins the whole cluster to the world center so it cannot drift off, even when dormant
const RANGE_BASE = 240;     // repulsion range base - widened for large graphs (pushes out farther)
const COLLIDE_PAD = 16, DAMPING = 0.85;
const MIN_SEP = 12;        // repulsion denominator floor - prevents force blowup (flinging) when very close
const MAX_V = 30;          // per-frame max speed base - raised for large graphs
const MAX_PUSH = 6;        // per-frame per-node collision displacement cap - prevents hub blowup
// Node size is proportional to neighbor count (degree) (sqrt, to flatten a wide range). The enlarged
// radius feeds directly into collision separation (minD), so spacing widens with neighbor count too,
// and nodes with few neighbors stay small and dense.
const NODE_R_BASE = 4;         // radius at degree 0
const NODE_R_SCALE = 3.4;      // sqrt(degree) coefficient
const NODE_R_MAX = 28;         // radius upper bound (prevents hub runaway)
const REPULSE_HUB_MAX = 2.5;   // hub-hub repulsion weight cap (prevents divergence)
function nodeRadius(n) { return Math.min(NODE_R_MAX, NODE_R_BASE + Math.sqrt(n.degree || 0) * NODE_R_SCALE); }
// Node stroke thickness (world units): proportional to radius + a screen-px floor. It reflects
// cam.s (current zoom), so it scales with the marker on zoom yet keeps a minimum thickness on
// zoom-out. Shared by draw and the edge endpoints.
function nodeStrokeW(n) { return Math.min(NODE_STROKE_MAX / cam.s, Math.max(nodeRadius(n) * NODE_STROKE_RATIO, NODE_STROKE_MIN / cam.s)); }
// Wake the simulation (discrete wakeup). Called only from events: new node/deletion (applyGraph),
// drag, focus. Never called from a continuous condition (overlap) - prevents endless reheating after settling.
function wake(a = 0.7) { alpha = Math.max(alpha, a); if (a >= SETTLE_ENTER) settling = true; requestFrame(); }

// --- render scheduling: draw when something moves, not every frame ------------------------------
// stepSim already goes dormant (no force is applied below ALPHA_MIN), but draw() re-scheduled itself
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
for (const ev of ["mousedown", "mousemove", "mouseup", "wheel", "keydown", "input", "resize"]) {
  addEventListener(ev, requestFrame, { capture: true, passive: true });
}
// Whether anything is still in motion. Every per-frame source is named here; one missing is an
// animation that stops halfway and stays there until the next mouse move.
function animating(act) {
  return settling                                                 // loader phase, sim mid-flight
    || alpha >= ALPHA_MIN                                         // the sim still applies force
    // ...and, after force stops, while anything is still visibly coasting. Measured on screen, so
    // the threshold means the same thing at any zoom: below a twentieth of a pixel per frame there
    // is nothing left to show.
    || simMotion * cam.s > MOTION_MIN_PX
    || cam.s !== camT.s || cam.x !== camT.x || cam.y !== camT.y    // easeCam snaps exactly, so == is safe
    || drag !== null                                              // a node is being dragged
    || pulses.size > 0 || peerEdgeFlash.size > 0                  // ring/flash effects still decaying
    || (peersOn && peerMarkers.size > 0)                          // markers ease asymptotically, never arrive
    // The flow animation is the only per-frame part of a highlight, and it only runs on edges that
    // are hot - so a search term matching nodes but no edges is a still picture, not motion. Under
    // reduced motion the dashes stand still, so a highlight is a still picture too.
    || (!REDUCED_MOTION && act !== null && act.es.size > 0);
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
// Change the target scale while keeping the world point under the cursor fixed (converges smoothly via easing).
function zoomAt(sx, sy, f) {
  const wx = (sx - camT.x) / camT.s, wy = (sy - camT.y) / camT.s;
  camT.s = Math.max(0.15, Math.min(4, camT.s * f));
  camT.x = sx - wx * camT.s; camT.y = sy - wy * camT.s; userMoved = true;
}
const TOP_INSET = 49;      // the frame's header (viewer.css) - compensated in centering/fit
const BOTTOM_INSET = 28;   // the frame's status bar
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
  return h ? 36 + h + 8 : BOTTOM_INSET;   // 36 = the panel's bottom offset (match the CSS)
}
// Smoothly bring ONE node to the screen center (focus-to-zoom). If zoomed too far out, zoom in
// slightly. Used where there is nothing but the node to frame: the activity feed following a single
// hit, and focusView's fallback for a node with no visible neighbours.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = (insetL() + innerWidth - insetR()) / 2 - n.x * camT.s;   // center in the strip between the rails
  camT.y = (innerHeight + TOP_INSET - insetB()) / 2 - n.y * camT.s; userMoved = true;   // above the detail panel
}

// A type's color is keyed by its NAME, not by its place in the view. It used to be the index into
// the view's sorted type list, so a workspace holding fewer types shifted every later hue: Concept
// was purple in one workspace and red in the next. A name hash now picks a slot in a fixed band of
// generated colors; a slot already taken in this view probes forward, in name order, so two types
// never share a color on one screen, and a type keeps its slot in every view where no earlier name
// collides with it. Relation kinds take the same rule in the edge band.
const COLOR_SLOTS = 48;
function nameSlot(name) {
  let h = 2166136261;   // FNV-1a
  for (let i = 0; i < name.length; i++) { h ^= name.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
  return h % COLOR_SLOTS;
}
function stableColors(names, edge) {
  const out = {}, used = new Set();
  for (const t of [...names].sort()) {
    let slot = nameSlot(t);
    if (used.size < COLOR_SLOTS) while (used.has(slot)) slot = (slot + 1) % COLOR_SLOTS;
    used.add(slot);
    out[t] = catColor(slot, edge);
  }
  return out;
}
function assignColors() {
  typeColor = stableColors(new Set(nodes.map(n => n.type)), false);
  edgeTypeColor = stableColors(new Set(edges.map(e => e.type)), true);
}

// --- Remembered layout (docs/design-system.md Section 5) ------------------------------------------
// The local console keeps each workspace's node positions in this browser, keyed by the node's host id
// from /api/surface and the workspace, and the next load starts from them: a workspace seen before
// appears at once instead of behind the settling loader. The read tier remembers nothing - the app
// shows every server from one origin, and a principal's positions would outlive a revoked grant - so
// the key exists only when /api/surface says "local". A position is a convenience; losing one costs a
// settling.
let surfaceNode = null;     // this node's host id on the local console, null elsewhere
let layoutScope = null;     // the storage key the current positions belong to
let storedLayout = null;    // {id: [x, y]} read for the scope being loaded
let layoutFresh = true;     // the next graph is the first of its scope
let layoutDirty = false;    // positions moved since the last save
async function loadSurface() {
  try {
    const s = await (await fetch("/api/surface", { cache: "no-store" })).json();
    if (s && s.surface) surfaceKind = String(s.surface);
    if (s && s.surface === "local" && s.node) surfaceNode = String(s.node);
  } catch (_) { /* no surface answer - nothing is remembered */ }
}
function loadLayout() {
  storedLayout = null;
  layoutScope = surfaceNode ? "supra.layout:" + surfaceNode + ":" + (currentWs() || "") : null;
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

function applyGraph(g) {
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
  renderLegend();
  noteTrustAndContest();
  const s = g.stats || {};
  statusEl.textContent = "updated " + new Date().toLocaleTimeString();
  // Current workspace leads the stats line - the selection UI is the left rail's chips, and this
  // keeps the selection readable even with that dock collapsed.
  const cur = currentWs();
  const wsLabel = cur === "*" || cur === "all" ? "(all)" : cur || "(default)";
  document.getElementById("stats").textContent =
    `ws ${wsLabel} / nodes ${s.node_count ?? nodes.length} / edges ${s.edge_count ?? edges.length}`
    + (clusterMode ? ` / groups ${Object.keys(typeColor).length}, bridges ${bridgeSet.size}` : "")
    + (s.type_counts ? " / " + Object.entries(s.type_counts).map(([t,c]) => `${t} ${c}`).join(", ") : "");
  emptyEl.style.display = nodes.length ? "none" : "flex";

  // If focused, refresh the detail inspector (reflect connection changes). If the focus node is gone, clear it.
  if (focus) { if (nodes.includes(focus)) renderDetail(focus); else { focus = null; renderDetail(null); } }

  // Initial auto-fit: once after the layout settles (cooling done), and only before user interaction (in draw).
  if (firstData && nodes.length) { firstData = false; needFit = !warm; }
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
  if (!f) { text = "sync unavailable"; tone = "bad"; }
  else if (f.configured === false) { pill.hidden = true; return; }
  else {
    const hubs = f.servers || [];
    const down = hubs.filter(s => !s.healthy).length;
    const unknown = hubs.filter(s => !s.negotiated_at).length;
    let mine = 0, theirs = 0;
    for (const s of hubs) for (const w of (s.workspaces || [])) { mine += w.local_ahead | 0; theirs += w.hub_ahead | 0; }
    if (down) { text = down === hubs.length ? "hub unreachable" : `${down} of ${hubs.length} hubs unreachable`; tone = "bad"; }
    else if (unknown) { text = "sync: no answer yet"; tone = "warn"; }
    else if (mine || theirs) { text = `syncing: this node +${mine}, hub +${theirs}`; tone = "warn"; }
    else if (hubs.length) { text = "in sync"; tone = "ok"; }
    else { const k = (f.known_peers || []).length; text = `hub: ${k} peer${k === 1 ? "" : "s"}`; tone = "ok"; }
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
function arrowTabs(list) {
  list.addEventListener("keydown", ev => {
    if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(ev.key)) return;
    const tabs = [...list.querySelectorAll('[role="tab"]')];
    const i = tabs.indexOf(document.activeElement);
    if (i < 0) return;
    const j = ev.key === "Home" ? 0 : ev.key === "End" ? tabs.length - 1
      : (i + (ev.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    ev.preventDefault();
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
    row("package", String(a.name || "supragnosis")),
    row("version", String(a.version || "")),
    row("licence", String(a.license || "")),
    repo ? row("source", html`<a href="${repo}" target="_blank" rel="noreferrer noopener">${repo}</a>`) : "",
  ]}</dl><div class="note">Dependency licences are not restated here - a hand-kept list is one that \
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
    host.innerHTML = '<div class="empty">federation is not configured on this node - create '
      + '~/.supragnosis/supragnosis.toml to join or host</div>';
    return;
  }

  const parts = [html`<div class="fsec">This node</div>`,
    html`<div class="prow"><span class="pid">${String(f.node_id || "")}</span>`,
    html`<span class="hint"> ${f.role === "hub" ? "hub" : "spoke"}</span></div>`];

  const hubs = f.servers || [];
  if (hubs.length) {
    parts.push(html`<div class="fsec">Hubs this node syncs to</div>`);
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
          : `this node +${w.local_ahead | 0}, hub +${w.hub_ahead | 0}`}</div>`);
      }
      // The two buckets only this section names, moved from the dock's Peers tab. Listed here but
      // not admitted there is a setup error that would otherwise just vanish; admitted there but not
      // shared here is knowledge left on the table. Intersecting the two lists would hide both.
      for (const w of (s.local_only || [])) parts.push(html`<div class="hint">${w}: <span class="bad">not admitted by the hub</span></div>`);
      for (const w of (s.peer_only || [])) parts.push(html`<div class="hint">${w}: admitted, not shared from this node</div>`);
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
            html`data-ws="${w}" title="${armed ? "click again to stop sharing" : "stop sharing this workspace with this peer"}">`,
            html`${w}<span class="x">${armed ? "confirm" : "x"}</span></button>`);
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

function renderLegend() {
  // Node-type and edge-kind legends, each in its own dock section. Clicking a chip toggles that kind's
  // visibility (the off set). The section summary shows the count.
  // Chips are recreated on every render - clear any hover highlight (and the chip tooltip) so
  // neither can stick to a dead chip that will never fire mouseleave.
  typeHl = null; edgeTypeHl = null; tip.style.display = "none";
  const fill = (host, keys, colorOf, offSet, isEdge) => {
    host.innerHTML = "";
    if (!keys.length) { host.innerHTML = '<span class="lbl">none</span>'; return; }
    for (const t of keys) {
      const el = document.createElement("button");
      el.type = "button";
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      el.appendChild(sw); el.appendChild(document.createTextNode(t || "(none)"));
      el.onclick = () => { if (offSet.has(t)) offSet.delete(t); else offSet.add(t); renderLegend(); };
      // Hovering a chip highlights its nodes/edges on the graph (render-only - no sim wake) and
      // shows the type's T-Box definition in the styled tooltip (Principle 8: a type has a stated
      // meaning - surfaced right where the type is read, not only in the definitions below).
      el.onmouseenter = () => { if (isEdge) edgeTypeHl = t; else typeHl = t; showTypeTip(el, t, isEdge); };
      el.onmouseleave = () => {
        if (isEdge) { if (edgeTypeHl === t) edgeTypeHl = null; }
        else if (typeHl === t) typeHl = null;
        tip.style.display = "none";
      };
      host.appendChild(el);
    }
  };
  const nodeKeys = Object.keys(typeColor).sort(), edgeKeys = Object.keys(edgeTypeColor).sort();
  fill(legendNodesEl, nodeKeys, t => typeColor[t], typeOff, false);
  fill(legendEdgesEl, edgeKeys, t => edgeTypeColor[t], edgeTypeOff, true);
  nodeCtEl.textContent = nodeKeys.length || "";
  edgeCtEl.textContent = edgeKeys.length || "";
  renderBoundaryChips();
}

// Legend chip tooltip: the type's glossary definition (T-Box), anchored beside the chip. A type
// with no recorded definition gets a nudge toward define_type instead of silence - curation as a
// micro-decision in the reading flow (Principle 22), not a separate chore.
function showTypeTip(el, t, isEdge) {
  const target = isEdge ? "relation" : "entity";
  const def = glossaryTypes.find(x => x.target === target && x.name === t);
  const r = el.getBoundingClientRect();
  tip.style.display = "block";
  tip.style.left = Math.min(r.right + 10, innerWidth - 330) + "px";
  tip.style.top = Math.max(6, r.top - 4) + "px";
  tip.innerHTML = html`<b>${t || "(none)"}</b> <span class="k">${target} type</span>${def
    ? html`<div class="tdef">${def.description}</div><span class="k">${def.sources} src</span>`
    : html`<div class="tdef none">no definition recorded - give this type a meaning with define_type</div>`}`;
}

// The set of nodes/edges to highlight from hover/focus/search. If none, null (everything highlighted equally).
function activeSet() {
  const anchor = focus || hover;
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
let trustUniform = null;     // the one tier of the loaded scope, or null when tiers differ
let contestedCount = 0;      // contested nodes in the loaded scope, whatever the legend hides (U3)
function noteTrustAndContest() {
  const tiers = new Set(nodes.map(n => n.trust_tier));
  trustUniform = nodes.length && tiers.size === 1 ? [...tiers][0] : null;
  contestedCount = nodes.reduce((k, n) => k + (n.contested ? 1 : 0), 0);
  renderTrustLegend();
  renderTierNote();
  const pill = document.getElementById("contestedPill");
  if (pill) {
    pill.hidden = contestedCount === 0;   // absent only for a known zero (U8)
    pill.textContent = contestedCount + " contested";
  }
}
function renderTrustLegend() {
  const host = document.getElementById("legendTrust");
  if (!host) return;
  host.textContent = "";
  for (const t of ["unverified", "agent_extracted", "host_signed", "human_confirmed"]) {
    const el = document.createElement("span");
    el.className = "tlg" + (trustUniform && trustUniform !== t ? " off" : "");
    const sw = document.createElement("span"); sw.className = "tsw t-" + t;
    el.append(sw, document.createTextNode(TIER_NAME[t]));
    host.appendChild(el);
  }
  const note = document.createElement("div");
  note.className = "tnote";
  note.textContent = trustUniform
    ? "All here: " + TIER_NAME[trustUniform] + ", so no node is ringed."
    : "A ring is the highest trust among the sources that support a node. Agent has none.";
  host.appendChild(note);
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
  ctx.beginPath(); ctx.arc(n.x, n.y, r + 3.5, 0, 7);
  if (t === "unverified") { ctx.setLineDash([3/cam.s, 3/cam.s]); ctx.lineWidth = 1.4/cam.s; ctx.strokeStyle = OTHER; }
  else if (t === "host_signed") { ctx.lineWidth = 1.2/cam.s; ctx.strokeStyle = INK; }
  else { ctx.lineWidth = 2.2/cam.s; ctx.strokeStyle = EDGE_HI; }
  ctx.stroke(); ctx.setLineDash([]);
}

async function poll() {
  const ws = wsInput.value.trim();
  const url = "/api/graph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
  try {
    const r = await fetch(url, { cache: "no-store" });
    if (!r.ok) { statusEl.textContent = "HTTP " + r.status; return; }
    const g = await r.json();
    if (g.error) { statusEl.textContent = g.error; return; }
    applyGraph(g);
    // Hyperedges (second-order structure) are fetched whenever the hull force or overlay needs them
    // (the force is on by default, suppressed only in group mode); otherwise cleared. As an auxiliary
    // channel, a failure still keeps the graph rendering (Principle 21: observability is optional).
    if (hyperMode || (hullForce && !clusterMode)) {
      try {
        const hurl = "/api/hypergraph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
        const hr = await fetch(hurl, { cache: "no-store" });
        if (hr.ok) {
          const hg = await hr.json();
          if (!hg.error) {
            hyperedges = hg.hyperedges || [];
            const drawn = hyperedges.filter(h => h.size >= 3).length;
            document.getElementById("stats").textContent += ` / hyperedges ${hyperedges.length} (hull ${drawn})`;

          }
        }
      } catch (e) { /* hull is auxiliary - the graph stays as-is */ }
    } else { hyperedges = []; }
    // Keep the type glossary + curation + proposals + log panels current (no-op while closed). The
    // log refreshes here too, so an observe event (which routes through poll) also refreshes it.
    refreshGlossary();
    refreshCuration();
    refreshProposals();
    refreshLog();
    refreshStrip();
  } catch (e) { statusEl.textContent = "connection failed - check the server is running"; }
}

function currentWs() { return wsInput.value.trim(); }
// Clean workspace transition: reset per-workspace view state, raise the loader immediately (no
// flash of the old layout under a stale camera), and treat the new graph like a fresh load - the
// reveal ends with an auto-fit, so switching workspaces always lands framed and zoomed sensibly.
// Reflect the selected workspace in the URL (?workspace=...) - shareable, bookmarkable, and it
// survives a reload. Empty (the node default) keeps the URL clean; "*" (all) is kept as-is.
function syncUrlWorkspace() {
  const ws = wsInput.value.trim();
  const url = new URL(location.href);
  if (ws) url.searchParams.set("workspace", ws);
  else url.searchParams.delete("workspace");
  history.replaceState(null, "", url);
}

function beginWorkspaceTransition() {
  saveLayout();          // the scope being left, under its own key
  layoutFresh = true;    // the next graph reads the new scope's
  syncUrlWorkspace();
  focus = null; hover = null; renderDetail(null);
  proposalSel = null;
  pulses.clear();
  settling = true;
  needFit = true;
  userMoved = false;
  wake(1);
}

function renderChipsActive() {
  const cur = currentWs();
  chipBar.querySelectorAll(".chip").forEach(c => c.classList.toggle("on", c.dataset.ws === cur));
}
async function loadWorkspaces() {
  try {
    const r = await fetch("/api/workspaces", { cache: "no-store" });
    if (!r.ok) return;
    const list = await r.json();
    const cur = currentWs();
    const mk = (label, val) => {
      const c = document.createElement("button");
      c.type = "button";
      c.className = "chip" + (val === cur ? " on" : "");
      c.dataset.ws = val; c.textContent = label;
      c.onclick = () => {
        if (wsInput.value.trim() === val) return;
        wsInput.value = val; beginWorkspaceTransition(); renderChipsActive(); poll();
      };
      return c;
    };
    // A panel heading like the ones below it (docs/design-system.md Section 4).
    const lbl = document.createElement("div"); lbl.className = "rsec wshead"; lbl.textContent = "Workspace";
    chipBar.replaceChildren(lbl, mk("(all)", "*"), ...list.map(w => mk(w, w)));
  } catch (e) { /* server not up - retry next cycle */ }
}

// --- Live MCP activity (SSE) --------------------------------------------------------
function nodeById(id) { return nodes.find(n => n.id === id); }
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

// --- Notices: every write answers (Principle 24) -------------------------------------------------
// The console's writes used to drop the server's answer and wait for the next poll to change the
// panel, so a refusal - a merge the fold blocks, the hub's read tier answering 403 - looked exactly
// like a success that had not arrived yet. Each one now reports here. The settings page's toasts in
// shape: a success leaves by itself, an error stays until it is dismissed, at most four at a time.
// Built from text nodes, never markup: an error string can carry an entity name.
const toastsEl = document.getElementById("toasts");
function toast(kind, title, detail) {
  if (!toastsEl) return;
  const el = document.createElement("div");
  el.className = "toast " + kind;
  const tick = document.createElement("span"); tick.className = "tk";
  const body = document.createElement("div"); body.className = "tb";
  const t = document.createElement("div"); t.className = "tt"; t.textContent = title;
  body.appendChild(t);
  if (detail) { const d = document.createElement("div"); d.className = "td"; d.textContent = detail; body.appendChild(d); }
  const close = document.createElement("button"); close.textContent = "x"; close.setAttribute("aria-label", "dismiss");
  close.onclick = () => el.remove();
  el.append(tick, body, close);
  toastsEl.prepend(el);
  if (kind === "ok") setTimeout(() => el.remove(), 4500);
  while (toastsEl.children.length > 4) toastsEl.lastChild.remove();
}
// One console write: GET path+query (the viewer's writes are GETs on the 0600 socket - see the review
// handler below), then a notice saying what happened. Returns the parsed answer, or null when the
// write did not land.
async function consoleWrite(path, query, said) {
  let r;
  try { r = await fetch(path + query, { cache: "no-store" }); }
  catch (e) { toast("bad", "Could not reach the server", "Nothing was changed. Is the daemon running?"); return null; }
  let body = null;
  try { body = await r.json(); } catch (_) { /* a refusal without a JSON body */ }
  if (!r.ok) {
    toast("bad", r.status === 403 ? "This view is read-only" : "Refused", (body && body.error) || ("HTTP " + r.status));
    return null;
  }
  toast("ok", said);
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
      html`<span class="src">${x.sources} src</span>`,
      html`<div class="def">${x.description}</div>`];
    // Contested definitions reuse the shared contested UI (keep / use this), so a type conflict and
    // an entity-kind conflict read identically - one mediation pattern (IR5).
    if (x.competitors && x.competitors.length) {
      parts.push(html`<div class="contested${x.contested ? " hot" : ""}">${
        contestedRows(x.description, "", x.def_source, x.competitors, x.contested)}</div>`);
    }
    return html`${parts}</div>`;
  };
  const section = (title, items) => html`<div class="gsec">${title} (${items.length})</div>${
    items.length ? items.map(item) : html`<div class="empty">none defined - use define_type</div>`}`;
  glossaryBodyEl.innerHTML =
    html`${section("entity types", group("entity"))}${section("relation types", group("relation"))}`;
  glossCtEl.textContent = glossaryTypes.length || "";
  // A contested definition opens the list, so its mediation act is in view (P6); otherwise the list
  // stays as the person left it.
  if (glossaryTypes.some(x => x.contested)) document.getElementById("defsBox").open = true;
  glossaryBodyEl.querySelectorAll(".confirm").forEach(b => {
    b.onclick = (ev) => { ev.stopPropagation(); resolveBelief(b.dataset.obs); };
  });
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
// The curation signals (Principle 7: generated, never committed), rendered by group for the Review
// page's Suggestions and Health (docs/design-system.md Section 7). Suspected secrets and contested
// beliefs are items of Needs you instead, decided one at a time.
function nchipHtml(n) {
  return html`<button type="button" class="nchip" data-id="${n.id}" title="focus ${n.name} (deg ${n.degree}, ${n.sources} src)">${n.name}<span class="ty">${n.type}</span></button>`;
}
function curationParts(group) {
  if (!curation) return [html`<div class="empty">no signals yet</div>`];
  const nchip = nchipHtml;
  const parts = [];
  if (group === "suggestions") {
    // Merge band (resolution-identity.md Section 3, Principle 15): embedding-near distinct-name pairs
    // the substrate proposes as merge candidates. A suggestion commits nothing (IR2); "Propose"
    // opens an entity_merge through the gate, which then waits under Needs you.
    const ms = curation.merge_suggestions || [];
    parts.push(html`<div class="csec">possible duplicates, by meaning (${ms.length})</div>`);
    parts.push(ms.length
      ? ms.map(m => [
          html`<div class="ms" data-a="${m.a}" data-b="${m.b}">`,
          html`<button type="button" class="nchip" data-id="${m.a}" title="focus ${m.a_name}">${m.a_name}</button>`,
          html`<span class="msx">~</span>`,
          html`<button type="button" class="nchip" data-id="${m.b}" title="focus ${m.b_name}">${m.b_name}</button>`,
          html`<span class="mssim" title="embedding similarity (retrieval aid) / shared neighbors">${m.similarity.toFixed(2)}${m.shared_neighbors ? " / " + m.shared_neighbors + "nb" : ""}</span>`,
          html`<button class="propmerge" title="open a merge proposal for this pair (folds the first into the second); it then waits under Needs you">Propose</button>`,
          html`</div>`])
      : html`<div class="empty">${bandEmptyReason(curation.merge_band)}</div>`);
    // Name variants: the deterministic sibling of the merge band (Principle 15/16). Entity ids already
    // fold case/whitespace, so these are the orthographic collisions nothing else catches - and unlike
    // the band above they need no embedder. A pair reuses the .ms row so the propose wiring applies;
    // 3+ members show as chips only (propose_merge takes a pair).
    const nv = curation.name_variants || [];
    parts.push(html`<div class="csec">possible duplicates, by spelling (${nv.length})</div>`);
    parts.push(nv.length
      ? nv.map(v => {
          const meta = html`<span class="mssim" title="which normalization rung grouped them / shared neighbors (structural corroboration)">${v.rung}${v.shared_neighbors ? " / " + v.shared_neighbors + "nb" : ""}</span>`;
          const m = v.members || [];
          if (m.length === 2) {
            // data-src tells the server which surface produced this, so the proposal records the real
            // evidence instead of inheriting the merge band's "embedding-near" rationale.
            return [html`<div class="ms" data-a="${m[0].id}" data-b="${m[1].id}" data-src="variant:${v.rung}">`,
              html`<button type="button" class="nchip" data-id="${m[0].id}" title="focus ${m[0].name}">${m[0].name}</button>`,
              html`<span class="msx">~</span>`,
              html`<button type="button" class="nchip" data-id="${m[1].id}" title="focus ${m[1].name}">${m[1].name}</button>`,
              meta,
              html`<button class="propmerge" title="open a merge proposal for this pair (folds the first into the second); it then waits under Needs you">Propose</button>`,
              html`</div>`];
          }
          return html`<div class="grp"><span class="gk">${v.key}</span>${meta}<div class="chips">${m.map(nchip)}</div></div>`;
        })
      : html`<div class="empty">none - no spelling variants</div>`);
    const dup = curation.duplicates || [];
    parts.push(html`<div class="csec">same name in two workspaces (${dup.length})</div>`);
    parts.push(dup.length
      ? dup.map(g => html`<div class="grp"><span class="gk">${g.key}</span><div class="chips">${g.members.map(nchip)}</div></div>`)
      : html`<div class="empty">none - this signal only fires across workspaces (same-workspace variants are above)</div>`);
  } else {
    // Contradictory accepted merges (Principle 6): the projection resolves the cycle by parity, but
    // the cycle itself is surfaced - the remedy is a settling merge proposal, never an edit.
    const mc = curation.merge_cycles || [];
    parts.push(html`<div class="csec">conflicting merges (${mc.length})</div>`);
    parts.push(mc.length
      ? mc.map(c => [
          html`<div class="grp"><div class="chips">${(c.members || []).map(nchip)}</div>`,
          html`<div class="hint">these accepted merges fold into each other - settle them with a new merge</div></div>`])
      : html`<div class="empty">none - no contradictory merges</div>`);
    const gb = curation.grab_bags || [];
    parts.push(html`<div class="csec">overloaded contexts (${gb.length})</div>`);
    parts.push(gb.length
      ? gb.map(b => { const nm = b.member_names.slice(0, 10).join(", ") + (b.member_names.length > 10 ? ", ..." : ""); return html`<div class="gb" data-hid="${b.id}"><span class="sz">${b.size}</span>${nm}<button class="reify" title="assert this context as a group entity + member_of relations (a lineage-bearing observation - the hyperedge itself stays a derived view)">Name this group</button></div>`; })
      : html`<div class="empty">none - no oversized clusters</div>`);
    const orph = curation.orphans || [];
    parts.push(html`<div class="csec">unlinked (${orph.length})</div>`);
    parts.push(orph.length ? html`<div class="chips">${orph.map(nchip)}</div>` : html`<div class="empty">none - all nodes linked</div>`);
    // T-Box axis collisions (Principle 9): a name defined on both the entity and relation axes -
    // informative, usually a mistake. Mediation lives in the filters dock's definitions.
    const ax = curation.type_axis_collisions || [];
    if (ax.length) {
      parts.push(html`<div class="csec">a type on both axes (${ax.length})</div>`);
      // The ", " separators go in as array items: joining the chips into one string would escape them.
      parts.push(html`<div class="gb">${ax.map((n, i) => [i ? ", " : "", html`<span class="gk">${n}</span>`])}<div class="hint">defined as both an entity type and a relation type - see Definitions in the filters</div></div>`);
    }
  }
  return parts;
}
function curationCount(group) {
  const s = (curation && curation.stats) || {};
  return group === "suggestions"
    ? (s.merge_suggestions || 0) + (s.name_variants || 0) + (s.duplicate_groups || 0)
    : (s.merge_cycles || 0) + (s.grab_bags || 0) + (s.orphans || 0) + (s.type_axis_collisions || 0);
}
// The handlers a rendered curation or review fragment needs, bound on its container.
function wireCuration(host) {
  // A finding names an observation without showing its text; clicking is the separate, deliberate act
  // of looking at it. The Log holds the loaded rows, so fall back to opening it when this scope has
  // not loaded the one being pointed at rather than silently doing nothing.
  host.querySelectorAll("[data-obs]:not(.confirm)").forEach(el => {
    el.onclick = () => {
      const id = el.getAttribute("data-obs");
      const o = (obsLog || []).find(x => x.id === id);
      if (o) showObsCard(o); else if (!dockREl.classList.contains("on")) toggleDock(false);
    };
  });
  host.querySelectorAll(".nchip[data-id]").forEach(c => {
    c.onclick = (ev) => {
      ev.stopPropagation();
      const n = nodeById(c.dataset.id);
      if (!n) return;
      if (reviewOpen()) location.hash = "";
      focus = n; renderDetail(n); focusView(n);
    };
  });
  host.querySelectorAll(".confirm").forEach(b => {
    b.onclick = (ev) => { ev.stopPropagation(); resolveBelief(b.dataset.obs); };
  });
  // Name this group (Principle 11 promotion path): name the context, assert it as a group entity
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
      inp.placeholder = "group name (optional)";
      inp.onclick = (e2) => e2.stopPropagation();
      const ok = document.createElement("button");
      ok.textContent = "ok";
      ok.onclick = async (e2) => {
        e2.stopPropagation();
        const ws = wsInput.value.trim();
        let q = "?hyperedge=" + encodeURIComponent(row.dataset.hid);
        if (inp.value.trim()) q += "&name=" + encodeURIComponent(inp.value.trim());
        if (ws) q += "&workspace=" + encodeURIComponent(ws);
        await consoleWrite("/api/reify", q, "Group named - it is a node now, linked to its members");
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
      // "*"/"all" is the all-workspaces VIEW sentinel, not a workspace name - forwarding it would
      // file the proposal into a workspace literally called "*" (the read endpoints normalize it,
      // the write endpoints do not).
      if (ws && ws !== "*" && ws !== "all") q += "&workspace=" + encodeURIComponent(ws);
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

function nameOf(id) { const n = nodeById(id); return n ? n.name : "(" + id.slice(0, 8) + ")"; }

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
const KIND_WORDS = {
  entity_merge: "Merge", entity_split: "Un-merge", claim_promotion: "Confirm a claim",
  claim_demotion: "Demote a claim", tbox_change: "Change a type", recall: "Recall",
};
function needsItems() {
  const items = [];
  for (const f of (curation && curation.secrets) || []) items.push({ key: "s:" + f.observation, kind: "secret", f });
  // Every open proposal is listed, whatever its diff (U4): one whose diff cannot be computed says so,
  // since nothing else would ever close it. A blocked one waits for a person too - to reject it.
  for (const p of proposals.filter(p => p.state === "open" || p.state === "blocked")) items.push({ key: "p:" + p.id, kind: "proposal", p });
  for (const c of (curation && curation.contradictions) || []) items.push({ key: "c:" + c.id, kind: "contested", c });
  return items;
}
function decidedItems() {
  return proposals.filter(p => p.state !== "open" && p.state !== "blocked").map(p => ({ key: "p:" + p.id, kind: "proposal", p }));
}
function reviewCount() {
  // Before the first curation read, the graph's own contested count stands in for the contested rows.
  return needsItems().length + (curation ? 0 : contestedCount);
}
function itemTitle(it) {
  if (it.kind === "secret") return "A credential may be in the log";
  if (it.kind === "contested") return it.c.name + ": which value?";
  const p = it.p;
  const names = (p.targets || []).map(nameOf);
  if (p.kind === "entity_merge" && p.into) return "Merge " + names.filter((_, i) => p.targets[i] !== p.into).join(", ") + " into " + nameOf(p.into);
  return (KIND_WORDS[p.kind] || p.kind) + (names.length ? ": " + names.join(", ") : "");
}
function itemTag(it) {
  if (it.kind === "secret") return html`<span class="rv-k bad">Secret</span>`;
  if (it.kind === "contested") return html`<span class="rv-k bad">Contested</span>`;
  const st = it.p.state;
  return html`<span class="rv-k">${KIND_WORDS[it.p.kind] || it.p.kind}</span>${st !== "open" ? html`<span class="rv-st ${st}">${st}</span>` : ""}`;
}
function updateReviewPill() {
  const pill = document.getElementById("reviewPill");
  if (!pill) return;
  const n = reviewCount();
  // Absent at a known zero, and on a read tier, where nothing can be decided (U8).
  pill.hidden = n === 0 || surfaceKind !== "local";
  pill.textContent = n + " to review";
}
let reviewSig = "";   // what the page last drew - a poll that changes nothing must not redraw it
function renderReview() {
  updateReviewPill();
  if (!reviewEl) return;
  reviewEl.hidden = !reviewOpen();
  if (!reviewOpen()) { reviewSig = ""; return; }
  const local = surfaceKind === "local";
  const needs = needsItems(), decided = decidedItems();
  // Redraw only when what is shown changed: a redraw replaces the buttons under the pointer and the
  // keyboard focus, which every 2.5-second poll would otherwise take away.
  const sig = JSON.stringify([reviewSeg, reviewSel, local, recallArmed, currentWs(),
    proposals.map(p => [p.id, p.state, p.verdicts]), curation && curation.stats, (curation && curation.contradictions || []).length,
    reviewView && [reviewView.id, (reviewView.checks || []).length]]);
  if (sig === reviewSig) return;
  reviewSig = sig;
  const segs = [["needs", "Needs you", needs.length], ["decided", "Decided", decided.length],
    ["suggestions", "Suggestions", curationCount("suggestions")], ["health", "Health", curationCount("health")]];
  const segEl = document.getElementById("rvSeg");
  segEl.textContent = "";
  let segSel = null;
  for (const [id, label, n] of segs) {
    const b = document.createElement("button");
    b.type = "button"; b.className = "rv-segb" + (reviewSeg === id ? " on" : "");
    b.textContent = label + (n ? " " + n : "");
    b.onclick = () => { reviewSeg = id; renderReview(); };
    segEl.appendChild(b);
    if (reviewSeg === id) segSel = b;
  }
  markTabs(segEl, [...segEl.children], segSel);
  const cur = currentWs();
  document.getElementById("rvScope").textContent = cur === "*" || cur === "all" ? "all workspaces" : (cur || "default");
  const list = document.getElementById("rvList"), detail = document.getElementById("rvDetail");
  if (reviewSeg === "suggestions" || reviewSeg === "health") {
    list.innerHTML = html`<div class="hint">${reviewSeg === "suggestions"
      ? "Possible duplicates the store found. Proposing one opens a merge that waits under Needs you."
      : "Signals about the shape of the knowledge. Nothing here needs deciding today."}</div>`;
    detail.innerHTML = html`${curationParts(reviewSeg)}`;
    if (!local) detail.querySelectorAll(".propmerge, .reify").forEach(b => b.remove());   // the read tier changes nothing
    wireCuration(detail);
    return;
  }
  const items = reviewSeg === "needs" ? needs : decided;
  if (!items.some(it => it.key === reviewSel)) reviewSel = items.length ? items[0].key : null;
  list.innerHTML = html`${items.length
    ? items.map(it => html`<button type="button" class="rv-item${it.key === reviewSel ? " on" : ""}" data-key="${it.key}">${itemTag(it)}<span class="rv-t">${itemTitle(it)}</span></button>`)
    : html`<div class="empty">${reviewSeg === "needs" ? "Nothing waits for you in this workspace." : "Nothing decided here yet."}</div>`}`;
  list.querySelectorAll(".rv-item").forEach(b => { b.onclick = () => { reviewSel = b.dataset.key; renderReview(); }; });
  const it = items.find(x => x.key === reviewSel);
  detail.innerHTML = html`${it ? itemDetail(it, local) : html`<div class="empty">Select an item.</div>`}`;
  if (it && it.kind === "proposal" && (!reviewView || reviewView.id !== it.p.id)) fetchReviewView(it.p.id);
  wireCuration(detail);
  wireDecisions(detail, it);
}
function itemDetail(it, local) {
  if (it.kind === "secret") {
    const f = it.f;
    return [html`<h3>A credential may be in the log</h3>`,
      html`<p class="why">A ${f.pattern} pattern matched in an observation's ${f.field}, at byte ${f.at | 0}. It is not shown here on purpose.</p>`,
      html`<p class="why">The log is append-only, so it cannot be removed yet. Stop it spreading by narrowing what this workspace shares, and rotate the credential.</p>`,
      html`<div class="rv-acts"><button type="button" class="btn" data-obs="${f.observation}">Open the observation</button></div>`];
  }
  if (it.kind === "contested") {
    const c = it.c;
    return [html`<h3>${c.name}: which value?</h3>`,
      html`<p class="why">Two values tie on trust, so supragnosis does not pick one for you. The other value stays in the log and can be chosen again.</p>`,
      html`<div class="contested hot">${contestedRows(c.current, "", c.kind_source, c.competitors || [], c.contested)}</div>`,
      local ? "" : html`<p class="why">This view is read-only.</p>`];
  }
  const p = it.p, v = reviewView && reviewView.id === p.id ? reviewView : null;
  const failing = v && v.checks ? v.checks.filter(c => c.blocking && !c.passed) : [];
  const parts = [html`<h3>${itemTitle(it)}</h3>`];
  if (p.rationale) parts.push(html`<p class="why">${p.rationale}</p>`);
  parts.push(html`<div class="sec">What changes</div>`, diffHtml(v ? v.diff : null));
  parts.push(html`<div class="sec">Checks</div>`);
  if (!v) parts.push(html`<div class="dnote">reading the checks...</div>`);
  else if (failing.length) parts.push(html`<div class="rv-block"><b>Blocked.</b> ${failing.map(c => html`<div>${c.name}: ${c.detail}</div>`)}<div>The merge cannot reach canon while this holds. Reject it here, or fix the proposal and open it again.</div></div>`);
  else parts.push(html`<div class="dnote">checks pass</div>`);
  const who = p.proposer ? "Opened by " + p.proposer : "Opened";
  parts.push(html`<div class="sec">Who asked</div><div class="hint">${who}${p.opened_at ? ", " + new Date(p.opened_at).toLocaleString() : ""}${p.self_attested ? " - self-attested" : ""}</div>`);
  if (!local) { parts.push(html`<p class="why">This view is read-only.</p>`); return parts; }
  const acts = [];
  const ready = !!v;   // the diff and checks are on screen
  if (p.state === "open" || p.state === "rejected") {
    const armed = recallArmed === p.id;
    const label = p.kind === "recall" && armed ? "Confirm the recall" : "Accept";
    const off = !ready || failing.length;
    acts.push(html`<button type="button" class="btn p" data-decide="merge"${off ? html` disabled` : ""}>${label} <kbd>A</kbd></button>`);
  }
  if (p.state === "open" || p.state === "blocked") acts.push(html`<button type="button" class="btn" data-decide="reject"${ready ? "" : html` disabled`}>Reject <kbd>R</kbd></button>`);
  const reversed = proposals.some(q => q.kind === "entity_split" && q.state === "merged" && (q.targets || []).includes(p.id));
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) acts.push(html`<button type="button" class="btn" data-unmerge="1">Un-merge</button>`);
  acts.push(html`<button type="button" class="btn" data-map="1">Show on the map</button>`);
  parts.push(html`<div class="rv-acts">${acts}</div>`);
  if (p.state === "rejected") parts.push(html`<p class="hint">A reject is provisional: accepting it later still merges it.</p>`);
  if (p.kind === "entity_merge" && p.state === "merged" && !reversed) parts.push(html`<p class="hint">Un-merge opens a proposal to separate them again. The pair is then never suggested again, and merging them later takes a proposal distinguishable from this one.</p>`);
  parts.push(html`<p class="hint">Accept casts your verdict; nothing is deleted. <kbd>J</kbd> <kbd>K</kbd> move between items.</p>`);
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
    if (proposalSel && proposalSel.id === id) { proposalDiff = reviewView.diff; proposalChecks = reviewView.checks; }
    renderReview();
  } catch (e) { /* the page still lists the item - it just cannot be decided yet */ }
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
  // No CSRF header needed: the socket (0600) admits no third-party origin - this page, proxied into
  // the desktop shell, is the only one on the surface - so there is no attacker origin for a forged
  // request to come from. The transport itself is the write gate.
  const done = await consoleWrite("/api/review", q, decision === "merge"
    ? "Accepted - your verdict is recorded" : "Rejected - your verdict is recorded");
  if (done) stepReview(1);
  refreshProposals();
}
function wireDecisions(host, item) {
  host.querySelectorAll("[data-decide]").forEach(b => { b.onclick = () => reviewDecide(item, b.dataset.decide); });
  host.querySelectorAll("[data-unmerge]").forEach(b => {
    b.onclick = async () => {
      const ws = wsInput.value.trim();
      const sq = "?merge=" + encodeURIComponent(item.p.id) + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
      await consoleWrite("/api/propose_split", sq, "Un-merge proposed - it waits under Needs you for a verdict");
      refreshProposals();
    };
  });
  host.querySelectorAll("[data-map]").forEach(b => {
    b.onclick = () => { location.hash = ""; proposalSel = null; selectProposal(item.p); };
  });
}
function stepReview(by) {
  const items = reviewSeg === "decided" ? decidedItems() : needsItems();
  if (!items.length) return;
  const i = Math.max(0, items.findIndex(x => x.key === reviewSel));
  reviewSel = items[Math.min(items.length - 1, Math.max(0, i + by))].key;
  recallArmed = null;
  renderReview();
}
window.addEventListener("hashchange", () => { recallArmed = null; renderReview(); if (reviewOpen()) refreshCuration(); refreshStrip(); requestFrame(); });
window.addEventListener("keydown", e => {
  if (!reviewOpen()) return;
  if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const items = reviewSeg === "decided" ? decidedItems() : needsItems();
  const item = items.find(x => x.key === reviewSel);
  if (e.key === "j" || e.key === "ArrowDown") { e.preventDefault(); stepReview(1); }
  else if (e.key === "k" || e.key === "ArrowUp") { e.preventDefault(); stepReview(-1); }
  else if (e.key === "a" && surfaceKind === "local") reviewDecide(item, "merge");
  else if (e.key === "r" && surfaceKind === "local") reviewDecide(item, "reject");
  else if (e.key === "Escape") location.hash = "";
});

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
function diffHtml(d) {
  if (!d) return html`<div class="dnote">computing the diff...</div>`;
  if (!d.computable) return html`<div class="dnote">no diff - ${d.note || "not computable for this kind"}</div>`;
  const tiers = (d.tier_changes || []).map(t => [
    html`<div class="drow"><span class="dk">tier</span><code>${t.observation.slice(0, 10)}</code>`,
    html`<span class="dfrom">${t.from}</span><span class="darr">-&gt;</span><span class="dto">${t.to}</span></div>`]);
  const beliefs = (d.overturned || []).map(b => {
    const settled = b.contested_before && !b.contested_after;
    const created = !b.contested_before && b.contested_after;
    const flag = settled ? html`<span class="dsettled">settles a contradiction</span>`
               : created ? html`<span class="dcreated">creates a contradiction</span>` : "";
    return [html`<div class="drow"><span class="dk">${b.field}</span>`,
      html`<button type="button" class="nchip" data-id="${b.entity}" title="focus ${b.name}">${b.name}</button>`,
      html`<span class="dfrom">${b.from || "(none)"}</span><span class="darr">-&gt;</span>`,
      html`<span class="dto">${b.to || "(none)"}</span>${flag}</div>`];
  });
  // entity_merge: which references move onto the canonical id, and which edges stop existing.
  // A self-loop is dropped by graph(), so that edge vanishes on accept - not readable from the
  // canvas overlay, which can only accent edges incident to a target.
  const rewires = (d.rewired || []).map(r => [
    html`<div class="drow"><span class="dk">edge</span><code>${r.kind}</code>`,
    html`<span class="dfrom">${r.from_name}</span><span class="darr">-&gt;</span>`,
    html`<span class="dto">${r.to_name}</span>`,
    html`<span class="dother">(${r.other_name})</span>`,
    r.becomes_self_loop ? html`<span class="dcreated">becomes a self-loop, edge disappears</span>` : "",
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
    if (into) { focus = into; renderDetail(into); focusView(into); }
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
let obsLog = [];   // /api/observations for the current workspace (newest-first)
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
function obsWhenFull(ms) { const d = new Date(ms); return Number.isFinite(d.getTime()) ? d.toLocaleString() : ""; }
// Trust tier as a colored dot (the edge columns' dot idiom) - a quiet ramp from dim (unverified) to
// gold (human-confirmed). The full tier text lives in the expanded provenance.
function tierDot(t) { return html`<span class="tdot t-${String(t)}" title="${String(t)}"></span>`; }

// Content-first row: a quiet meta line (tier dot + compact time) over the observation content (the
// primary element, clamped to 2 lines). Host and the rest move to the expanded provenance.
// A proposal event said in names. Its stored content is machine text with raw ids in it, and that
// text is inside the content address so it can never be rewritten - the log read as rows of hashes.
// The server resolves the ids (it can still name a merged-away entity, which this side cannot, since
// the graph folds those away); this only phrases the result. Falls back to the raw content whenever
// the server had nothing to add, so an untranslatable row still says something.
// Returns text, not markup: callers interpolate it into an html template, which escapes it once.
function proposalLine(p) {
  const kind = p.kind || "proposal";
  const ts = p.targets || [];
  // Two entities can share a display name - that is what a duplicate IS, and a merge of them is
  // precisely the act most worth reading. "T2 -> T2" says nothing, so a name that is not unique
  // within the line carries its short id. Only the ambiguous ones pay it.
  const all = p.into ? ts.concat(ts.some(t => t.id === p.into.id) ? [] : [p.into]) : ts;
  const seen = {};
  for (const t of all) { const n = t.name || t.id; seen[n] = (seen[n] || 0) + 1; }
  const named = t => {
    const n = t.name && t.name !== t.id ? t.name : t.id.slice(0, 8);
    return seen[t.name || t.id] > 1 ? `${n} (${t.id.slice(0, 6)})` : n;
  };
  let what = "";
  if (p.into) {
    // A merge reads as a direction: what disappears, and what it folds into.
    const gone = ts.filter(t => t.id !== p.into.id).map(named);
    what = gone.length ? `${gone.join(", ")} -> ${named(p.into)}` : named(p.into);
  } else if (ts.length) {
    what = ts.map(named).join(", ");
  }
  const head = p.event === "opened" ? `opened ${kind}`
    : p.event === "verdict" ? `${p.decision === "merge" ? "accepted" : p.decision || "verdict"} ${kind}`
    : `${p.event} ${kind}`;
  // State is worth showing on a verdict precisely when it disagrees with the decision - a merge that
  // folded to blocked is the case a reader must not miss.
  const state = p.event === "verdict" && p.state && !(p.decision === "merge" && p.state === "merged")
    ? ` [${p.state}]` : "";
  return `${head}${state}${what ? ": " + what : ""}`;
}

function obsRowHtml(o) {
  const a0 = (o.attestations || [])[0] || {};
  const text = o.proposal ? proposalLine(o.proposal) : o.content;
  const cls = o.proposal ? "otext ev" : "otext";
  return html`${[
    html`<div class="obs" data-id="${o.id}">`,
    html`<button type="button" class="ohead" title="open observation detail">`,
      html`<span class="ometa">${tierDot(o.effective_tier)}`,
        html`<span class="owhen">${obsWhen(a0.observed_at)}</span></span>`,
      html`<span class="${cls}">${text}</span>`,
    html`</button></div>`]}`;
}

// The observation detail CARD: a focused, dismissable view of one observation - full content plus
// its provenance (every attestation), lineage (derived_from), and the entities/relations it asserted.
// The single detail surface for both the node log column and the workspace Log tab (a log row opens
// it, so the list stays a scannable list - the earlier cramped in-column expansion is gone).
function obsCardHtml(o) {
  const a0 = (o.attestations || [])[0] || {};
  const parts = [html`<div class="card">`,
    html`<button class="close" title="close" aria-label="close">`,
      html`<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`,
      html`<path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg></button>`,
    html`<div class="oc-meta">${tierDot(o.effective_tier)}<span class="octier">${String(o.effective_tier)}</span>`,
      html`<span class="ocwhen">${obsWhenFull(a0.observed_at)}</span></div>`,
    // A proposal event leads with the readable form, but keeps the stored text underneath: this
    // card is the dereference surface an id resolves to (P2/P14), so what is literally in the log
    // has to stay inspectable here even once it is no longer what the reader is shown.
    html`<div class="oc-content">${o.proposal ? proposalLine(o.proposal) : o.content}</div>`,
    o.proposal ? html`<div class="ocraw" title="the text stored in the log - fixed inside the content address">${o.content}</div>` : "",
    html`<div class="osec">provenance (${(o.attestations || []).length})</div>`];
  for (const a of o.attestations || []) {
    parts.push(html`<div class="prow"><span class="phost">${a.host}</span>`,
      a.on_behalf_of ? html`<span class="pobo">for ${a.on_behalf_of}</span>` : "",
      html`<span class="ptier">${String(a.trust_tier)}${a.evaluated_tier !== a.trust_tier ? " -> " + String(a.evaluated_tier) : ""}</span>`,
      a.confidence != null ? html`<span class="pconf">conf ${String(a.confidence)}</span>` : "",
      a.origin_node ? html`<span class="poid">node ${String(a.origin_node).slice(0, 8)}</span>` : "",
      html`<span class="pwhen">${obsWhenFull(a.observed_at)}</span></div>`);
  }
  if (o.derived_from && o.derived_from.length)
    parts.push(html`<div class="osec">derived from</div><div class="oids">${
      o.derived_from.map(id => html`<span class="oid">${String(id).slice(0, 10)}</span>`)}</div>`);
  if (o.proposal) {
    const p = o.proposal;
    parts.push(html`<div class="osec">proposal</div>`,
      html`<div class="ocprop"><span class="pkind">${p.kind || "proposal"}</span>`,
        html`<span class="pstate ${p.state || ""}">${p.state || ""}</span>`,
        html`<span class="oid">${String(p.proposal).slice(0, 10)}</span></div>`);
    const ts = p.targets || [];
    if (ts.length) {
      // A target that no longer has a node is one this merge folded away - it stays listed (the act
      // touched it) but is not offered as a jump, because there is nothing left to jump to.
      const chip = t => {
        const live = !!nodeById(t.id);
        const into = p.into && t.id === p.into.id;
        const label = html`${t.name || String(t.id).slice(0, 8)}${into ? html` <span class="rk">kept</span>` : ""}`;
        return live
          ? html`<button type="button" class="echip" data-id="${t.id}" title="focus ${t.name || t.id}">${label}</button>`
          : html`<span class="echip off" title="folded away by this merge - no node to focus">${label}</span>`;
      };
      parts.push(html`<div class="osec">targets</div><div class="echips">${ts.map(chip)}</div>`);
    }
  }
  if (o.entities && o.entities.length)
    parts.push(html`<div class="osec">entities</div><div class="echips">${
      o.entities.map(e => html`<button type="button" class="echip" data-id="${e.id}" title="focus ${e.name}">${e.name}</button>`)}</div>`);
  if (o.relations && o.relations.length)
    parts.push(html`<div class="osec">relations</div>`,
      o.relations.map(r => html`<div class="ocrel">${r.from} <span class="rk">${r.type}</span> ${r.to}</div>`));
  parts.push(html`<div class="ocid">obs ${String(o.id).slice(0, 16)}</div></div>`);
  return html`${parts}`;
}
function hideObsCard() { obscardEl.className = ""; obscardEl.innerHTML = ""; }
function showObsCard(o) {
  obscardEl.innerHTML = html`${obsCardHtml(o)}`;
  obscardEl.className = "on";
  obscardEl.querySelector(".close").onclick = hideObsCard;
  // An asserted entity: close the card and focus its node.
  obscardEl.querySelectorAll(".echip").forEach(c => {
    c.onclick = ev => {
      ev.stopPropagation();
      const n = nodeById(c.dataset.id);
      hideObsCard();
      if (n) { focus = n; renderDetail(n); focusView(n); }
    };
  });
}

// Renders a list of observation summaries into a container (a scannable list) and wires each row to
// open the observation detail card. Used by the workspace Log tab and the node inspector's log column.
function wireObsList(container, list) {
  container.innerHTML = html`${list.length ? list.map(obsRowHtml) : html`<div class="empty">no observations</div>`}`;
  container.querySelectorAll(".obs").forEach(row => {
    const o = list.find(x => x.id === row.dataset.id);
    if (!o) return;
    row.querySelector(".ohead").onclick = () => showObsCard(o);
  });
}

function renderLog() {
  logCtEl.textContent = obsLog.length || "";
  if (!obsLog.length) { obsLogSig = ""; logBodyEl.innerHTML = '<div class="empty">no observations in this workspace yet - observe knowledge to see the log</div>'; return; }
  // Re-render only when the observation set actually changed, so a poll refresh does not collapse a
  // row the user expanded (the log is append-mostly; an unchanged set keeps its DOM + open rows).
  const sig = obsLog.map(o => o.id + ":" + o.effective_tier + ":" + (o.attestations || []).length).join(",");
  if (sig === obsLogSig && logBodyEl.querySelector(".obs")) return;
  obsLogSig = sig;
  wireObsList(logBodyEl, obsLog);
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
// Who wrote it: the person it was written for, else the host by id - "this node" only for the id
// /api/surface reports (Section 9).
function writerOf(a) {
  if (a.on_behalf_of) return a.on_behalf_of;
  if (a.host && surfaceNode && a.host === surfaceNode) return "this node";
  return a.host || "";
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
    const o = stripState.o, a = (o.attestations || [])[0] || {};
    const first = String(o.content || "").split("\n")[0];
    stripEl.innerHTML = html`${tierDot(o.effective_tier)}<span class="w">${writerOf(a)} ${obsWhen(a.observed_at)}</span><span class="t">${o.proposal ? proposalLine(o.proposal) : first}</span>`;
  } else if (stripState.empty) {
    stripEl.innerHTML = html`<span class="t">Nothing observed in this scope yet</span>`;
  } else {
    stripEl.innerHTML = html`<span class="t bad">The Log is unavailable - the server did not answer</span>`;
  }
}
if (stripEl) stripEl.onclick = () => applyDock(false, true);

async function refreshLog() {
  if (!panelOn("log")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/observations" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const l = await r.json(); if (Array.isArray(l)) obsLog = l; }
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
    if (secEl) secEl.textContent = "evidence (" + n + ")"
      + (node.sources !== n ? " / " + node.sources + " attestations" : "");
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

function pulseNodes(ids) { for (const id of ids || []) if (posById.has(id)) pulses.set(id, 60); requestFrame(); }
function logRow(body) {
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = html`<span class="t">${new Date().toLocaleTimeString()}</span>${body}`;
  logEl.prepend(row);
  while (logEl.children.length > 8) logEl.lastChild.remove();
  setTimeout(() => row.remove(), 8000);
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
  if (ev.session && ev.session !== footprintSession) { footprintSession = ev.session; footprint.clear(); }
  // While following, if activity happens in a different workspace, switch to it - otherwise added
  // nodes/hits are outside the current scope and do not appear (the SSE event arrives, but the polling ws mismatches).
  const switched = follow && ev.workspace && currentWs() !== "*" && currentWs() !== ev.workspace;
  if (switched) { wsInput.value = ev.workspace; beginWorkspaceTransition(); renderChipsActive(); }
  let ids = [];
  if (ev.kind === "observe") {
    logRow(html`<b>observe</b> +${(ev.entities||[]).length} ent, +${ev.relations||0} rel <span class="t">ws ${ev.workspace}</span>`);
    await poll();                       // wait for the new nodes to enter the graph, then pulse
    ids = ev.entities || [];
  } else if (ev.kind === "search") {
    logRow(html`<b>search</b> "${ev.query}" -> ${ev.hits} hits <span class="t">${ev.mode}</span>`);
    if (switched) await poll();          // if the workspace switched, load that graph (so hits are visible)
    ids = ev.nodes || [];
  } else if (ev.kind === "get_entity") {
    logRow(html`<b>get_entity</b> ${ev.name || ev.id.slice(0,8)} <span class="t">${ev.found ? "found" : "unknown"}</span>`);
    ids = ev.found ? [ev.id] : [];
  } else if (ev.kind === "sync") {
    // Federation hit: who touched this store, which direction, how much - the live remote feed.
    logRow(html`<b>sync</b> ${ev.direction} ${ev.workspace} &lt;-&gt; ${String(ev.peer).slice(0, 18)} (${ev.count})`);
    let addedNodes = [], addedEdges = [];
    if (ev.count > 0) {
      // Knowledge landed: load it now, and re-frame once the re-layout settles (follow mode) so the
      // camera presents the grown graph instead of staring at a stale corner of it. Diff the node AND
      // edge sets across the reload so a peer's cursor can tour exactly what it just contributed.
      const beforeN = new Set(nodes.map(n => n.id)), beforeE = new Set(edges.map(edgeKey));
      refitOnReveal = follow;
      await poll();
      addedNodes = nodes.filter(n => !beforeN.has(n.id));
      addedEdges = edges.filter(e => !beforeE.has(edgeKey(e)));
    }
    if (peersOn) notePeer(ev, addedNodes, addedEdges);   // server mode: tour the peer's marker, not the camera
    ids = [];
  } else if (ev.kind === "traverse") {
    const sn = nodeById(ev.start);
    logRow(html`<b>traverse</b> ${sn ? sn.name : ev.start.slice(0,8)} -> ${(ev.reached||[]).length}`);
    ids = [ev.start, ...(ev.reached || [])];
  } else return;
  pulseNodes(ids);
  for (const id of ids) if (id) footprint.add(id);   // accumulate the conversation footprint (regardless of whether the node exists)
  // Reheat only when the event touched actual nodes - sync/hc chatter (now periodic via the
  // status loop) must never jiggle a settled layout.
  if (ids.length) wake(0.3);
  if (follow && !peersOn) {
    // Frame the WHOLE hit set: several hits fit into view together (pan + zoom as needed); a
    // single hit centers smoothly. The camera narrates what the agent touched. In server mode the
    // camera holds still and the peer markers move instead (a hub would otherwise jump on every hit).
    const hitNodes = ids.map(nodeById).filter(Boolean);
    if (hitNodes.length > 1) fitView(hitNodes, 130);
    else { const n = hitNodes[0] || primaryNode(ev); if (n) centerOn(n); }
  }
  const sEl = document.getElementById("session");
  if (sEl) sEl.textContent = footprintSession ? `session ${footprintSession.slice(0,22)} / ${footprint.size} used` : "";
}
// --- Peer markers (server mode) -----------------------------------------------------
// On a hub, many peers sync at once. Instead of the camera chasing every remote hit, each federated
// peer is a minimal cursor-dot that glides to the nodes it touched (like a remote cursor): a push shows
// the peer drifting onto the knowledge it just contributed. Deterministic per-peer color; hover for id.
function peerColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return `hsl(${h % 360} 68% 63%)`;
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
    html`<span class="k">node</span> ${id.slice(0, 16)}<br>`,
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
    peersOn = true; peersBtn.classList.add("on");
    follow = false; followBtn.classList.toggle("on", false);
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

// Confirm one side of a contested belief (resolution.md Section 4.2): /api/resolve opens a
// claim_promotion for the asserting observation and casts the Console merge verdict - the human
// console is the only surface that can grant human_confirmed (Section 6). Both are gated appended
// events; the belief changes because the fold consumes the verdict, never by a direct write.
async function resolveBelief(obs) {
  if (!obs) return;
  const ws = wsInput.value.trim();
  const q = "?observation=" + encodeURIComponent(obs) + "&tier=human_confirmed"
    + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
  await consoleWrite("/api/resolve", q, "Confirmed - this value is now human confirmed");
  await poll();   // re-fold: the ring drops, the panel updates, the proposal shows as merged
}

// The contested-belief block of the inspector / curation panel: the current value and each surviving
// competitor, with a confirm button per value (the mediation act). `cur` marks the policy winner.
function contestedRows(current, tier, curObs, competitors, contested) {
  // Once the belief is human_confirmed there is nothing higher to promote to, so the confirm buttons
  // are hidden and the box reads as COMMITTED (not a lingering confirm dialog - the losing value
  // stays listed for reference, Principle 3/6, but without an action). Below that tier the actions
  // stay: "keep" locks in the value shown now (so recency cannot flip it), "use this" switches to a
  // competing value - both promote that value's observation to human_confirmed (a gated console verdict).
  const committed = String(tier) === "human_confirmed";
  const row = (v, t, obs, cur) => html`${[
    html`<div class="crow${cur ? " cur" : ""}"><span class="cv">${v}</span>`,
    cur
      ? html`<span class="curtag${committed ? " confirmed" : ""}">${committed ? "confirmed" : "current"}</span>`
      : html`<span class="ctier">${String(t)}</span>`,
    obs && !committed
      ? html`<button class="confirm" data-obs="${obs}" title="${cur ? "lock in the current value (mark it human-confirmed so recency cannot flip it)" : "make this the confirmed value instead (mark it human-confirmed)"}">${cur ? "keep" : "use this"}</button>`
      : "",
    html`</div>`]}`;
  const head = committed
    ? (contested
        ? "human-confirmed - competing values still tie (both confirmed)"
        : "confirmed by a human - this belief is settled")
    : (contested
        ? "these values tie on trust - confirm which is correct"
        : "resolved by trust - other asserted values shown for reference");
  return html`<div class="chead">${head}</div>${row(current, tier, curObs, true)}${
    competitors.map(c => row(c.value, c.trust_tier, c.observation, false))}`;
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
function contestedBlock(node) {
  if (node.contested) return html`<div class="contested hot">${
    contestedRows(node.type, node.trust_tier, node.kind_source, node.competitors || [], true)}</div>`;
  const cmp = node.competitors || [];
  if (!cmp.length) return html``;
  const settled = String(node.trust_tier) === "human_confirmed" ? "confirmed by a human" : "resolved by trust";
  return html`${[
    html`<div class="contested folded${foldOpen === node.id ? " open" : ""}">`,
    html`<button class="cfold" type="button">${settled} - ${cmp.length} other kind${cmp.length > 1 ? "s" : ""} asserted</button>`,
    html`<div class="cbody">${contestedRows(node.type, node.trust_tier, node.kind_source, cmp, false)}</div></div>`]}`;
}

// The inspector "why (evidence & decision)" body from /api/explain: per-field belief resolution
// (each single-valued field's ranked candidates - winner / alias / competitor) plus the supporting
// observation log. An explanation OF the projection - the winner IS what the graph shows.
function candRowHtml(c) {
  return html`${[
    html`<div class="cand ${c.role}"><span class="cv">${c.value}</span>`,
    html`<span class="crole">${c.role}</span>`,
    html`<span class="ctier">${String(c.trust_tier)}</span>`,
    html`<span class="cobs" title="asserting observation ${c.observation}">${String(c.observation).slice(0, 8)}</span></div>`]}`;
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
function relationGroupHtml(g, open) {
  const shown = open ? g.items : g.items.slice(0, RELATION_CHIPS);
  const rest = g.items.length - shown.length;
  // The edge's description stays one hover away, as it was on the row; a superseded edge says so.
  const chip = ({ other, e }) => html`${[
    html`<button type="button" class="rchip${e.valid_to ? " past" : ""}" data-id="${other.id}" `,
    html`title="${e.description || ((e.valid_to ? "superseded - " : "") + "focus " + other.name)}">`,
    html`<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>${other.name}</button>`]}`;
  return html`${[
    html`<div class="rgroup"><div class="rghead"><span class="rdir">${g.out ? "->" : "<-"}</span>`,
    html`<span class="rkind">${g.kind}</span><span class="rct">${g.items.length}</span></div>`,
    html`<div class="rchips">${shown.map(chip)}`,
    rest > 0 ? html`<button class="rmore" type="button" data-key="${g.key}" title="show the other ${rest}">+${rest}</button>` : "",
    html`</div></div>`]}`;
}

// --- Detail inspector: shows the clicked node's connections (neighbors + relations), and click a neighbor to explore ---
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
  if (base.tier !== null) scopeSaid.push(base.tier);
  if (base.origin) scopeSaid.push("from " + base.origin);
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
    html`<button class="close" title="close" aria-label="close">`,
      html`<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`,
      html`<path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>`,
      html`</svg></button>`,
    // Taller / shorter: the height cap keeps the graph visible by default; this trades it for room.
    html`<button class="grow" title="${detailTall ? "shorter panel" : "taller panel"}" aria-label="${detailTall ? "shorter panel" : "taller panel"}">`,
      html`<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`,
      html`<path d="${detailTall ? "M3 4.5 L6 7.5 L9 4.5" : "M3 7.5 L6 4.5 L9 7.5"}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>`,
      html`</svg></button>`,
    html`<h2>${node.name}</h2>`,
    html`<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span>`,
      html`<span>${node.type}</span>${tierDot(tier)}${base.tier === null ? html`<span>${tier}</span>` : ""}`,
      scopeSaid.length ? html`<span class="scopeline" title="these values are the same for every node in this view, so they are not repeated per node">same across this scope: ${scopeSaid.join(" / ")}</span>` : "",
    html`</div>`,
    node.aliases && node.aliases.length ? html`<div class="meta">aliases: ${node.aliases.join(", ")}</div>` : "",
    origin, workspace,
    node.description ? html`<div class="desc">${node.description}</div>` : "",
    contestedBlock(node),
    // Why this value won: a lazy-loaded disclosure (the per-field decision comes from /api/explain
    // only when opened - the graph poll stays light). The evidence itself is the column below.
    disputed ? html`<div class="why"><button class="whytoggle" type="button">belief decision (why this value)</button><div class="whybody"></div></div>` : "",
    // Two regions: the node's relations, grouped by kind, and its observation log (the evidence
    // behind its belief), side by side.
    html`<div class="rels">`,
      html`<div class="relcol"><div class="sec">relations (${nrel})</div>`,
        groups.length ? groups.map(g => relationGroupHtml(g, relOpen.keys.has(g.key))) : html`<div class="empty">none</div>`,
      html`</div>`,
      html`<div class="relcol"><div class="sec">evidence</div><div class="logcol"></div></div>`,
    html`</div>`]}`;
  detailEl.className = "on" + (detailTall ? " tall" : "");
  detailEl.querySelector(".close").onclick = () => { focus = null; renderDetail(null); };
  detailEl.querySelector(".grow").onclick = () => { detailTall = !detailTall; renderDetail(node); };
  detailEl.querySelectorAll(".rchip").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) { focus = n; renderDetail(n); focusView(n); }
    };
  });
  detailEl.querySelectorAll(".rmore").forEach(b => {
    b.onclick = () => { relOpen.keys.add(b.dataset.key); renderDetail(node); };
  });
  detailEl.querySelectorAll(".confirm").forEach(b => {
    b.onclick = (ev) => { ev.stopPropagation(); resolveBelief(b.dataset.obs); };
  });
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
}
addEventListener("resize", resize);

// --- Minimap (map-tool convention): whole-graph overview + viewport rectangle -------------------
// Redrawn by the rAF loop each frame (dots + one rect - cheap). The world->minimap transform fits
// the VISIBLE node bounds (typeOff respected, same set fitView uses), so the overview always
// matches what the main canvas can show. Click/drag pans the camera (scale unchanged).
const miniEl = document.getElementById("minimap"), mctx = miniEl.getContext("2d");
const MINI_W = 168, MINI_H = 154, MINI_PAD = 10;   // height matches the HUD column (see viewer.css)
let miniT = { k: 1, ox: 0, oy: 0 };   // world -> minimap: m = w * k + o (kept for the pan handler)
function drawMinimap() {
  const src = nodes.filter(n => !typeOff.has(n.type));
  const on = showMini && src.length > 0;
  miniEl.classList.toggle("on", on);
  if (!on) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (miniEl.width !== MINI_W * dpr) { miniEl.width = MINI_W * dpr; miniEl.height = MINI_H * dpr; }
  mctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  mctx.clearRect(0, 0, MINI_W, MINI_H);
  let a = 1e9, b = 1e9, c = -1e9, d = -1e9;
  for (const n of src) { a = Math.min(a,n.x); b = Math.min(b,n.y); c = Math.max(c,n.x); d = Math.max(d,n.y); }
  const gw = Math.max(1, c-a), gh = Math.max(1, d-b);
  const k = Math.min((MINI_W - MINI_PAD*2) / gw, (MINI_H - MINI_PAD*2) / gh);
  const ox = (MINI_W - gw*k)/2 - a*k, oy = (MINI_H - gh*k)/2 - b*k;
  miniT = { k, ox, oy };
  // Everything renders inside the panel's rounded outline (radius matches the 10px
  // border-radius in viewer.css, minus the half-pixel stroke inset) - without the clip, dots
  // and the viewport stroke land in the corner cut zones and get sliced by the CSS rounding.
  const R = 9.5;
  const round = mctx.roundRect !== undefined;
  mctx.save();
  if (round) {
    mctx.beginPath();
    mctx.roundRect(0.5, 0.5, MINI_W - 1, MINI_H - 1, R);
    mctx.clip();
  }
  // A faithful miniature: hyperedge hulls (under), edge connections, then nodes sized by degree.
  const mx = n => n.x * k + ox, my = n => n.y * k + oy;
  // Hulls (hyperedge overlay) - the graph's own clouds (hullGeom + paintCloud), scaled into the
  // minimap via a world->minimap transform, so a region looks the same in both. Only while the
  // overlay is on.
  if (hyperMode && hyperedges.length) {
    const nodeMap = new Map(src.map(n => [n.id, n]));
    mctx.save();
    mctx.transform(k, 0, 0, k, ox, oy);   // compose world->minimap on top of the DPR transform
    for (const he of hyperedges) {
      if ((he.size || 0) < 3) continue;
      const ms = (he.members || []).map(id => nodeMap.get(id)).filter(Boolean);
      if (ms.length < 3) continue;
      const g = hullGeom(ms);
      if (!g) continue;
      paintCloud(mctx, g, hyperColor(he.id), 0.16);
    }
    mctx.restore();
  }
  // Edge connections - one batched path, thin and neutral (structure, not type).
  if (showEdges) {
    mctx.beginPath();
    for (const e of edges) {
      if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
      mctx.moveTo(mx(e.a), my(e.a));
      mctx.lineTo(mx(e.b), my(e.b));
    }
    mctx.strokeStyle = EDGE_OTHER; mctx.globalAlpha = 0.5; mctx.lineWidth = 0.6;
    mctx.stroke();
    mctx.globalAlpha = 1;
  }
  // Nodes - filled circles whose radius grows with degree (a hub reads bigger), clamped for the minimap.
  // Each carries the same background-color halo the main graph gives its nodes (NODE_STROKE_RATIO):
  // without it, dots that overlap in a dense cluster merge into one blob and the minimap stops
  // reporting density, which is most of what it is for. Same ratio, minimap-scale bounds - the floor
  // keeps a 1.2px dot separated at all, the cap keeps a hub from closing into a donut.
  for (const n of src) {
    const mr = Math.max(1.2, Math.min(5, 1.2 + Math.sqrt(n.degree || 0) * 0.75));
    mctx.beginPath();
    mctx.arc(mx(n), my(n), mr, 0, 6.2832);
    mctx.fillStyle = typeColor[n.type] || OTHER;
    mctx.fill();
    mctx.lineWidth = Math.min(1.4, Math.max(mr * NODE_STROKE_RATIO, 0.6));
    mctx.strokeStyle = SURFACE;
    mctx.stroke();
  }
  // Viewport rectangle: the map window's corners in world coords, mapped in and clamped to the frame.
  // A corner that is clamped onto the frame corner takes the PANEL's radius (the rect then traces
  // the rounded outline instead of slicing through it); free corners keep a small radius.
  const vw = mapWindow();
  const x1 = Math.max(0.5, Math.min(MINI_W - 0.5, ((vw.l - cam.x) / cam.s) * k + ox));
  const y1 = Math.max(0.5, Math.min(MINI_H - 0.5, ((vw.t - cam.y) / cam.s) * k + oy));
  const x2 = Math.max(0.5, Math.min(MINI_W - 0.5, ((vw.r - cam.x) / cam.s) * k + ox));
  const y2 = Math.max(0.5, Math.min(MINI_H - 0.5, ((vw.b - cam.y) / cam.s) * k + oy));
  const w = Math.max(2, x2 - x1), h = Math.max(2, y2 - y1);
  // Ink, not gold: with the whole graph in view the rect lies on the minimap's edge, and a gold frame
  // is the decoration gold is not for (docs/design-system.md Section 4).
  mctx.strokeStyle = INK; mctx.globalAlpha = 0.55; mctx.lineWidth = 1;
  if (round) {
    const lc = x1 <= 1, tc = y1 <= 1, rc = x2 >= MINI_W - 1, bc = y2 >= MINI_H - 1;
    const radii = [
      lc && tc ? R : 3,   // top-left
      rc && tc ? R : 3,   // top-right
      rc && bc ? R : 3,   // bottom-right
      lc && bc ? R : 3,   // bottom-left
    ];
    mctx.beginPath();
    mctx.roundRect(x1, y1, w, h, radii);
    mctx.stroke();
  } else {
    mctx.strokeRect(x1, y1, w, h);
  }
  mctx.restore();
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
  const vw = Math.max(1, R - L - pad*2), vh = Math.max(1, B - T - pad*2);
  const gw = Math.max(1, c-a), gh = Math.max(1, d-b);
  camT.s = Math.max(0.15, Math.min(hi, Math.min(vw / gw, vh / gh)));
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
// Size-scaled visual weight for a hyperedge (0 at min size 2, 1 at HULL_SIZE_REF+ members).
function hullSizeNorm(size) { return Math.max(0, Math.min(1, (size - 2) / (HULL_SIZE_REF - 2))); }
// Geometry of one hyperedge hull: the convex hull of the member centers, an outward expansion radius r
// (largest member glyph + gap), and the member centroid (for the label). Returns null when degenerate.
function hullGeom(ms) {
  let cx = 0, cy = 0, r = 0;
  for (const m of ms) { cx += m.x; cy += m.y; const g = nodeRadius(m) + nodeStrokeW(m) / 2; if (g > r) r = g; }
  cx /= ms.length; cy /= ms.length; r += HULL_NODE_GAP;
  const pts = ms.map(m => ({ x: m.x, y: m.y })), hull = convexHull(pts);
  if (hull.length < 3) return null;
  return { hull, r, cx, cy, pts };
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
function drawWsHulls(ctx, hullLabels) {
  if (boundaryMode === "off") return;
  const groups = new Map();
  const push = (key, n) => {
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(n);
  };
  if (boundaryMode === "workspace") {
    // At most one key per node. Empty means the projection found attestations naming more than one
    // workspace (a merge folded across the boundary), and such a node joins no region.
    for (const n of nodes) if (n.workspace) push(n.workspace, n);
  } else {
    // Several keys per node, on purpose. Attestations union across nodes (F4), so an entity both
    // sides asserted belongs to both regions and the outlines overlap - which is the fact worth
    // seeing, not a defect to design away. Labelled by id prefix: `origins` is the self-declared
    // host and pairing the two would need a stamp-level join the graph does not carry, so a hex
    // prefix is the honest name here (P2 - the verifiable id is the one that may attribute).
    for (const n of nodes) for (const o of (n.origin_nodes || [])) push(o, n);
  }
  if (groups.size < 2) return;
  ctx.save();
  ctx.setLineDash([6, 5]);
  ctx.lineWidth = 1.2;
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
  ["workspace", "each workspace is a sharing unit, and no entity is in two"],
  ["origin", "who signed for each entity - regions OVERLAP where two peers both attested it"],
  ["off", "no boundaries"],
];
function renderBoundaryChips() {
  if (!legendBoundaryEl) return;
  legendBoundaryEl.innerHTML = "";
  for (const [mode, why] of BOUNDARY_MODES) {
    const el = document.createElement("button");
    el.type = "button";
    el.className = "lg" + (boundaryMode === mode ? "" : " off");
    const sw = document.createElement("span");
    sw.className = "sw";
    // Dashed swatch for the two that draw, hollow for off - the same outline the layer uses.
    if (mode === "off") { sw.style.background = "transparent"; sw.style.border = "1px dashed #666"; }
    else { sw.style.background = "transparent"; sw.style.border = "1px dashed " + hyperColor(mode + ":swatch"); }
    el.appendChild(sw);
    el.appendChild(document.createTextNode(mode));
    el.title = why + " (b cycles the three)";
    el.onclick = () => { boundaryMode = mode; renderBoundaryChips(); requestFrame(); };
    legendBoundaryEl.appendChild(el);
  }
}

// One region as a cloud. The ellipse lies along the members' principal axis (their covariance), each
// radius the farthest member along it plus CLOUD_REACH times the hull's gap, and the gradient keeps
// most of its density over the members before it fades out: a member at the far end of a long region
// still sits in about a third of it.
function paintCloud(c, g, col, alpha) {
  const ms = g.pts;
  let sxx = 0, syy = 0, sxy = 0;
  for (const m of ms) { const dx = m.x - g.cx, dy = m.y - g.cy; sxx += dx * dx; syy += dy * dy; sxy += dx * dy; }
  const th = 0.5 * Math.atan2(2 * sxy, sxx - syy), ux = Math.cos(th), uy = Math.sin(th);
  let ru = 0, rv = 0;
  for (const m of ms) {
    const dx = m.x - g.cx, dy = m.y - g.cy;
    ru = Math.max(ru, Math.abs(dx * ux + dy * uy)); rv = Math.max(rv, Math.abs(-dx * uy + dy * ux));
  }
  const pad = g.r * CLOUD_REACH, R = ru + pad, Rv = Math.max(rv + pad, R * 0.35);
  const hsla = a => col.replace("hsl(", "hsla(").replace(")", ", " + a + ")");
  c.save();
  c.translate(g.cx, g.cy); c.rotate(th); c.scale(1, Rv / R);
  const grad = c.createRadialGradient(0, 0, 0, 0, 0, R);
  grad.addColorStop(0, hsla(1)); grad.addColorStop(0.45, hsla(0.8)); grad.addColorStop(0.75, hsla(0.38)); grad.addColorStop(1, hsla(0));
  c.globalAlpha = alpha; c.fillStyle = grad;
  c.beginPath(); c.arc(0, 0, R, 0, 2 * Math.PI); c.fill();
  c.restore();
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
function hyperColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return catColor(h % 512, false);   // id hash -> generated palette index (removes the fixed 8-color limit)
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
// Do the two hyperedges share a node (iterate the smaller set). Shared = a connected context, so they are not separated (C1).
function hullsShareMember(a, b) {
  const [s, l] = a.size <= b.size ? [a, b] : [b, a];
  for (const x of s) if (l.has(x)) return true;
  return false;
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
  const key = [DPR, weight, size, color, spacing, halo, text].join("|");
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

function stepSim() {
  simMotion = 0;
  const N = nodes.length;
  if (N === 0) return;
  const cooling = alpha >= ALPHA_MIN;
  if (cooling) alpha += (0 - alpha) * ALPHA_DECAY;
  const active = alpha >= ALPHA_MIN;   // dormant once cooling is done - no force is applied
  const pinned = v => v === drag || v === focus;

  // Scale by node count: more nodes spread wider (repulsion range/strength up, center attraction down).
  // Prevents a hairball (central clumping) - base is tuned for small graphs, large ones expand via spread.
  const spread = Math.min(4, Math.max(1, Math.sqrt(N / 20)));
  const range = RANGE_BASE * spread, centerG = CENTER_BASE / spread;
  const repulse = REPULSE * spread, maxV = MAX_V * Math.min(spread, 2);

  // Collision displacement is accumulated per node, then clamped to a cap - stops a hub overlapping
  // many neighbors from flinging far in a single frame (instead of moving directly).
  const cdx = new Array(N).fill(0), cdy = new Array(N).fill(0);
  // The radius (degree-proportional) is computed once per frame and shared by repulsion weighting/collision separation.
  const rad = new Array(N);
  for (let i = 0; i < N; i++) rad[i] = nodeRadius(nodes[i]);

  for (let i = 0; i < N; i++) for (let j = i + 1; j < N; j++) {
    const a = nodes[i], b = nodes[j];
    let dx = b.x - a.x, dy = b.y - a.y, d = Math.hypot(dx, dy);
    if (d < 0.5) {
      // (Nearly) coincident coordinates have a zero direction and cannot be pushed apart - separate in a deterministic direction (prevents degeneracy).
      const ang = ((i * 7 + j * 13) % 628) / 100;
      dx = Math.cos(ang); dy = Math.sin(ang); d = 0.5;
    } else { dx /= d; dy /= d; }
    const d2 = d * d;
    // Repulsion only when active (cooling), and near-range only (0 outside the range). Prevents the
    // problem where, when dormant, only repulsion remained and spread endlessly without the balance of cohesion (gravity/springs).
    if (active && d < range) {
      // The larger a node's neighbor count (radius), the more gently repulsion is weighted up - spacing around a hub widens (clamped to a cap).
      const w = Math.min(REPULSE_HUB_MAX, (rad[i] + rad[j]) / (2 * NODE_R_BASE));
      const rf = repulse * alpha * w * (1 - d / range) / Math.max(d2, MIN_SEP * MIN_SEP);
      a.vx -= rf*dx; a.vy -= rf*dy; b.vx += rf*dx; b.vy += rf*dy;
    }
    // Collision minimum gap = sum of the two radii + padding -> a node with more neighbors (larger) gets wider spacing around it.
    const minD = rad[i] + rad[j] + COLLIDE_PAD;
    if (d < minD) {
      const push = (minD - d) / 2;
      cdx[i] -= dx*push; cdy[i] -= dy*push; cdx[j] += dx*push; cdy[j] += dy*push;
    }
  }

  // Residual overlap is cleaned up by the collision displacement below, pushing each frame (position correction applies even while dormant).
  // Do not reheat here - wake only from 'events' like a new node/drag/resize
  // (removes the problem of reheating every frame after settling).
  if (active) {
    for (const e of edges) {
      let dx = e.b.x - e.a.x, dy = e.b.y - e.a.y, d = Math.hypot(dx,dy) || 1;
      const f = (d - SPRING_LEN) * SPRING_K * alpha; dx /= d; dy /= d;
      e.a.vx += f*dx; e.a.vy += f*dy; e.b.vx -= f*dx; e.b.vy -= f*dy;
    }
  }

  // Hyperedge layout (Principle 11 second-order structure "well cohered"): (1) pull members toward
  // each hyperedge's centroid to cohere the hull tightly, and (2) push apart the centroids of
  // non-overlapping hulls to widen the gap. Hulls that share a node (overlap) stay naturally close
  // because the shared node is pulled to both centroids at once, and the separation force cancels at
  // the shared node, preserving the overlap relationship (C1: overlap = connective tissue).
  // Default organizer; group mode takes precedence (suppressed while clusterMode is on) so the two never fight.
  if (hullForce && !clusterMode && active && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    // Geometry: members + centroid + mean radius (clamped to a cap - so a huge grab-bag cannot push the whole layout)
    // + member id set (for share detection).
    const hgs = [];
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(Boolean);
      if (ms.length < 2) continue;
      let cx = 0, cy = 0; for (const m of ms) { cx += m.x; cy += m.y; }
      cx /= ms.length; cy /= ms.length;
      let r = 0; for (const m of ms) r += Math.hypot(m.x - cx, m.y - cy);
      r = Math.min(r / ms.length, HULL_R_CAP);
      hgs.push({ ms, ids: new Set(ms.map(m => m.id)), cx, cy, r });
    }
    // (1) Cohesion: member -> its own centroid. Scaled by hull size - larger hulls pull tighter,
    // small ones stay loose.
    for (const g of hgs) {
      const cf = HYPER_PULL * (HULL_COH_MIN + hullSizeNorm(g.ms.length) * (HULL_COH_MAX - HULL_COH_MIN));
      for (const m of g.ms) {
        if (pinned(m)) continue;
        m.vx += (g.cx - m.x) * cf * alpha; m.vy += (g.cy - m.y) * cf * alpha;
      }
    }
    // (2) Separation: push apart only **disjoint** hull pairs (not sharing a node) - a shared hull is
    // a connected context and must stay attached (C1), and pushing every pair at high density blows up the whole layout.
    // Accumulate each hull's net displacement, clamp it to the cap (HULL_MAX_PUSH), then apply it to members to prevent divergence.
    const sepx = new Array(hgs.length).fill(0), sepy = new Array(hgs.length).fill(0);
    for (let i = 0; i < hgs.length; i++) for (let j = i + 1; j < hgs.length; j++) {
      const a = hgs[i], b = hgs[j];
      if (hullsShareMember(a.ids, b.ids)) continue;   // do not push a connected context (C1)
      const dx = b.cx - a.cx, dy = b.cy - a.cy, d = Math.hypot(dx, dy) || 0.01;
      const want = a.r + b.r + HULL_PAD;
      if (d < want) {
        const mag = (want - d) * HULL_SEP * alpha, ux = dx / d * mag, uy = dy / d * mag;
        sepx[i] -= ux; sepy[i] -= uy; sepx[j] += ux; sepy[j] += uy;
      }
    }
    for (let i = 0; i < hgs.length; i++) {
      let mx = sepx[i], my = sepy[i]; const mm = Math.hypot(mx, my);
      if (mm === 0) continue;
      if (mm > HULL_MAX_PUSH) { mx *= HULL_MAX_PUSH / mm; my *= HULL_MAX_PUSH / mm; }
      for (const m of hgs[i].ms) if (!pinned(m)) { m.vx += mx; m.vy += my; }
    }
  }

  const wcx = innerWidth/2, wcy = innerHeight/2;   // world coordinates (CSS pixel system) - independent of the camera
  // Group mode: place per-type target points on a circle to spatially separate groups (deterministic:
  // angle assigned in sorted type order). The group-target attraction replaces the center attraction, and
  // bridge edges (springs) pull linking nodes between groups so a "navigable" connection remains.
  let tgt = null;
  if (clusterMode && active) {
    const types = Object.keys(typeColor).sort(), k = Math.max(1, types.length);
    const R = Math.min(innerWidth, innerHeight) * 0.34;
    tgt = {};
    types.forEach((t, i) => { const a = (i / k) * Math.PI * 2; tgt[t] = [wcx + R * Math.cos(a), wcy + R * Math.sin(a)]; });
  }
  for (let k = 0; k < N; k++) {
    const v = nodes[k];
    if (pinned(v)) { v.vx = 0; v.vy = 0; continue; }
    if (active) {
      if (tgt) {
        const g = tgt[v.type] || [wcx, wcy];
        v.vx += (g[0] - v.x) * CLUSTER_PULL * alpha; v.vy += (g[1] - v.y) * CLUSTER_PULL * alpha;
      } else {
        v.vx += (wcx - v.x) * centerG * alpha; v.vy += (wcy - v.y) * centerG * alpha;
      }
    }
    v.vx *= DAMPING; v.vy *= DAMPING;
    // speed cap - even under a large force, nothing flies off-screen.
    const sp = Math.hypot(v.vx, v.vy);
    if (sp > maxV) { v.vx *= maxV/sp; v.vy *= maxV/sp; }
    // the collision displacement is also clamped to a per-node cap before adding.
    let mx = cdx[k], my = cdy[k]; const m = Math.hypot(mx, my);
    if (m > MAX_PUSH) { mx *= MAX_PUSH/m; my *= MAX_PUSH/m; }
    v.x += v.vx + mx; v.y += v.vy + my;
    // What this node actually moved. Cooling says whether force is still being applied; it does not
    // say whether anything is still moving, and those part company exactly where the stop was
    // abrupt - damping and integration run whether or not the sim is active, so nodes coast for
    // roughly half a second after the last force.
    const stepped = Math.hypot(v.vx + mx, v.vy + my);
    if (stepped > simMotion) simMotion = stepped;
  }

  // Central-axis anchor: the pairwise forces are action-reaction symmetric (zero net force on the
  // whole-graph centroid), but the per-node / per-hull clamps (maxV, MAX_PUSH, HULL_MAX_PUSH) and the
  // collision push that keeps correcting while dormant break that symmetry, so the constellation
  // slowly drifts to one side. Rigidly translate every node so the centroid returns to the world
  // center - positions only (no velocity -> no momentum/oscillation), applied every frame including
  // when dormant, which fixes the cluster to the center after cooling and across simulation restarts.
  // Skipped while dragging (do not fight the pointer); pinned nodes (drag/focus) are held in place.
  if (!drag) {
    let sx = 0, sy = 0;
    for (const v of nodes) { sx += v.x; sy += v.y; }
    const offx = (wcx - sx / N) * ANCHOR_K, offy = (wcy - sy / N) * ANCHOR_K;
    if (offx || offy) for (const v of nodes) if (!pinned(v)) { v.x += offx; v.y += offy; }
    // The recenter shifts every node, so it is motion too - and it runs while dormant.
    const shifted = Math.hypot(offx, offy);
    if (shifted > simMotion) simMotion = shifted;
  }
}

function draw() {
  rafId = null;
  stepSim();
  easeCam();
  // Reduced motion: settle synchronously (bounded - alpha decays multiplicatively, so convergence
  // takes ~110 steps; the cap only guards a pathological graph from locking the frame).
  if (settling && REDUCED_MOTION) { for (let i = 0; i < 600 && alpha > REVEAL_ALPHA; i++) stepSim(); }
  // Reveal transition: the layout has calmed enough to show. Frame it first (auto-fit + snap the
  // camera, before any user interaction) so the graph appears already fitted rather than mid-zoom.
  if (settling && alpha <= REVEAL_ALPHA) {
    settling = false;
    if (needFit && !userMoved) { needFit = false; fitView(); cam.s = camT.s; cam.x = camT.x; cam.y = camT.y; }
    else if (refitOnReveal) { fitView(); }   // smooth re-frame after synced knowledge landed
    refitOnReveal = false;
  }
  // While settling, keep stepping the sim (above) but hide the graph behind the loader - the user sees
  // a calm spinner instead of nodes flying around during the violent early rearrangement.
  if (settling && nodes.length) {
    ctx.setTransform(1,0,0,1,0,0); ctx.clearRect(0,0,canvas.width,canvas.height);
    loaderEl.classList.add("on");
    requestFrame(); return;
  }
  loaderEl.classList.remove("on");
  // Initial auto-fit: once after the layout settles (only before user interaction).
  if (needFit && alpha < ALPHA_MIN && !userMoved) { needFit = false; fitView(); }

  const act = activeSet();
  flowPhase++;   // advance the active-edge flow animation (only while a frame is being drawn)
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
  // the first. Screen-space boxes {x, y, w, h}.
  const labelBoxes = [];
  if (hyperMode && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    const items = [];
    for (const h of hyperedges) {
      const hot = hlAnchor && h.members.includes(hlAnchor.id);   // is the active (click/hover) node a member of this context
      // Every context is drawn as a cloud by default, the active node's at full strength and the
      // rest faded (design-system.md Section 5); as flat hulls they had overlapped into one slab,
      // which is why only the active node's were drawn. "all regions" off in the view menu draws
      // the active node's alone.
      if (!allRegions && !hot) continue;
      const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
      if (ms.length < 3) continue;
      const g = hullGeom(ms);
      if (!g) continue;
      // Legend-chip hover: does this context contain any highlighted member? Hulls without one fade,
      // so the highlighted type/relation stands out against the hull layer too (same inspection language).
      const lgHit = typeHl ? ms.some(m => m.type === typeHl)
        : edgeTypeHl ? ms.some(m => lgEndpoints.has(m.id))
        : false;
      // Named by its two best-connected members other than the active node, with the count, so the
      // regions around one node read as what else is in them rather than as that node five times.
      const named = ms.filter(m => !hlAnchor || m.id !== hlAnchor.id)
        .sort((a, b) => (b.degree || 0) - (a.degree || 0) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
        .slice(0, 2).map(m => m.name);
      const top = Math.min(...g.hull.map(p => p.y)) - g.r;
      items.push({ g, col: hyperColor(h.id), hot, lgHit, text: named.join(", ") + " (" + ms.length + ")", top, size: ms.length });
    }
    const lgActive = !!(typeHl || edgeTypeHl);
    const paint = (it) => {
      paintCloud(ctx, it.g, it.col, lgActive ? (it.lgHit ? HULL_LAYER_ALPHA : HULL_LAYER_DIM)
        : hlAnchor ? (it.hot ? HULL_ACTIVE_ALPHA : HULL_LAYER_DIM) : HULL_LAYER_ALPHA);
      // The name sits above the region's upper edge, outside it, where no member's label is. Only
      // the active node's regions are named: names are placed before the nodes', which avoid them,
      // so twenty region names at rest would push the map's own names out.
      if (it.hot) hullLabels.push({ cx: it.g.cx, cy: it.top, text: it.text, col: it.col, hot: it.hot, lgHit: it.lgHit, size: it.size });
    };
    for (const it of items) if (!it.hot) paint(it);   // active hulls painted last so they sit on top
    for (const it of items) if (it.hot) paint(it);
    ctx.globalAlpha = 1;
    drawWsHulls(ctx, hullLabels);
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
          const fs = 13;
          return { l, px: l.cx*cam.s + cam.x, py: l.cy*cam.s + cam.y - fs * 0.8, fs };
        })
        .filter(o => o.px > -80 && o.px < innerWidth + 80 && o.py > -30 && o.py < innerHeight + 30)
        .sort((a, b) => (b.l.hot - a.l.hot) || (b.l.size - a.l.size));
      for (const o of cand) {
        const l = o.l;
        ctx.font = "500 " + o.fs + "px " + NAME_FAMILY;   // measures the placement box
        const w = ctx.measureText(l.text).width + o.fs*0.5, h = o.fs*1.3, x = o.px - w/2, y = o.py - h/2;
        if (labelBoxes.some(p => x < p.x + p.w && x + w > p.x && y < p.y + p.h && y + h > p.y)) continue;
        labelBoxes.push({ x, y, w, h });
        // Blend into the map without a chip: a crisp background-color halo (a cutout that matches the
        // canvas) keeps the group-color name sharp over the hull fills; lower idle opacity lets it recede.
        // The label fades as one unit (drawLabel), so the halo no longer darkens its own glyphs.
        ctx.globalAlpha = lgActive ? (l.lgHit ? 0.75 : HULL_LABEL_HOVER_FADE)
          : hlAnchor ? (l.hot ? 1 : HULL_LABEL_HOVER_FADE) : 0.6;
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
  if (showEdges) {
    // Parallel-edge lanes: group visible edges by unordered node pair, so multiple links between the
    // same two nodes - a reciprocal pair (a<->b) OR several verbs in one direction (a->b) - fan out
    // into distinct arcs instead of stacking on one line. Same visibility filters as the loop.
    const pairList = new Map();
    for (let di = 0; di < edges.length; di++) {
      const de = edges[di];
      if (typeOff.has(de.a.type) || typeOff.has(de.b.type) || edgeTypeOff.has(de.type) || (de.valid_to && !showSuperseded)) continue;
      const pk = de.a.id < de.b.id ? de.a.id + "|" + de.b.id : de.b.id + "|" + de.a.id;
      if (!pairList.has(pk)) pairList.set(pk, []);
      pairList.get(pk).push(di);
    }
    for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
    if (edgeTypeOff.has(e.type)) continue;                 // edge-kind toggle (legend)
    if (e.valid_to && !showSuperseded) continue;           // hide superseded (past) edges (history toggle)
    const dx = e.b.x-e.a.x, dy = e.b.y-e.a.y, d = Math.hypot(dx,dy) || 1, ux = dx/d, uy = dy/d;
    // Grow each radius by half of that node's stroke so the edge meets outside the node stroke (the
    // stroke is radius-proportional, so it differs per endpoint) - the arrowhead tip touches the outer
    // stroke boundary, connecting with no gap/penetration, and it holds on zoom.
    const ar = nodeRadius(e.a) + nodeStrokeW(e.a)/2, br = nodeRadius(e.b) + nodeStrokeW(e.b)/2, room = d - ar - br;
    if (room <= 0.5) continue;   // (temporary) overlap - skip this frame's edge
    // Hovering an edge-kind chip treats its edges as hot (flow animation + weight) - same language as
    // node hover, so "what does this relation connect" reads instantly.
    const hot = act ? act.es.has(i) : (edgeTypeHl ? e.type === edgeTypeHl : false);
    // The line starts at the source node's edge and ends at the arrowhead base (or the tip if arrows are off).
    // Arrowhead length in WORLD units (proportional to the target node, bounded 7..16) so it scales
    // with zoom like the node markers. A fixed screen size looked tiny on large/zoomed-in nodes and
    // clunky when zoomed out. Still clamped to half the free gap so short edges never overrun it.
    const alen = Math.min(Math.max(7, Math.min(16, nodeRadius(e.b) * 0.7)), room * 0.5);
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    // Fan this edge onto its lane within the pair: lane index centered on 0, so a lone edge -> 0 ->
    // straight (quadratic control on the midpoint = the original straight line). sign uses a canonical
    // perpendicular (smaller id -> larger id) so lanes stay distinct regardless of each edge's
    // direction - a reciprocal pair opens into a lens, several one-way verbs into a fan.
    const pk = e.a.id < e.b.id ? e.a.id + "|" + e.b.id : e.b.id + "|" + e.a.id;
    const lst = pairList.get(pk) || [i], lane = lst.indexOf(i) - (lst.length - 1) / 2;
    const off = lane * Math.min(d * 0.30, 46) * (e.a.id < e.b.id ? 1 : -1);
    const cpx = (sx0 + tipx)/2 + (-uy)*off, cpy = (sy0 + tipy)/2 + ux*off;   // quadratic control point
    // Tip tangent = control -> tip; the line end and arrowhead align to it (reduces to ux,uy when straight).
    let tdx = tipx - cpx, tdy = tipy - cpy; const tdl = Math.hypot(tdx, tdy) || 1; tdx /= tdl; tdy /= tdl;
    const basex = tipx - tdx*alen, basey = tipy - tdy*alen;   // arrowhead base, back along the tip tangent
    // Active-edge label: relation type at the curve midpoint (quadratic B(0.5)). len = distance so the
    // shortest edges are labeled first when the count exceeds the cap.
    if (hot && edgeLabels) edgeLabels.push({
      mx: 0.25*sx0 + 0.5*cpx + 0.25*tipx, my: 0.25*sy0 + 0.5*cpy + 0.25*tipy,
      text: e.type, len: d, col: e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER),
    });
    // Group mode: make cross-group (different-type) edges stand out, in-group edges dim.
    const cross = clusterMode && e.a.type !== e.b.type;
    // The default is semi-transparent (EDGE_ALPHA); on hover/focus a connected edge (hot) activates to
    // 1.0 and the rest dim. Group mode is a separate emphasis that makes cross-group links stand out.
    // Legend hover outranks the rest: node-type chip -> both-endpoint edges bright, one-endpoint dim
    // halo, others faded; edge-kind chip -> matching edges full, others faded.
    ctx.globalAlpha = typeHl
      ? (e.a.type === typeHl && e.b.type === typeHl ? 0.9 : (e.a.type === typeHl || e.b.type === typeHl) ? 0.45 : 0.05)
      : edgeTypeHl
      ? (e.type === edgeTypeHl ? 1 : 0.05)
      : act ? (hot ? 1 : 0.06) : (clusterMode ? (cross ? 0.9 : 0.1) : EDGE_ALPHA);
    // Color is by relation kind - it reveals what kind of connection this is. A superseded edge is EDGE_OLD (a past signal, dashed).
    ctx.strokeStyle = e.valid_to ? EDGE_OLD
      : (hot || edgeTypeHl === e.type) ? (edgeTypeColor[e.type] || EDGE_OTHER) : EDGE_NEUTRAL;
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // constant thickness on screen
    if (hot) {
      // Active edges show flow direction: marching dashes travel source -> target. Screen-constant
      // dash/speed (divide by cam.s) so it reads the same at any zoom. Negative offset -> forward flow.
      ctx.setLineDash([7/cam.s, 6/cam.s]);
      ctx.lineDashOffset = REDUCED_MOTION ? 0 : -(flowPhase * 0.6) / cam.s;
    } else {
      ctx.setLineDash(e.valid_to ? [5/cam.s, 5/cam.s] : []);
      ctx.lineDashOffset = 0;
    }
    // With arrows off, draw the curve to the node edge (tip); with arrows on, to the arrowhead base.
    const endx = showArrows ? basex : tipx, endy = showArrows ? basey : tipy;
    ctx.beginPath(); ctx.moveTo(sx0, sy0); ctx.quadraticCurveTo(cpx, cpy, endx, endy); ctx.stroke();
    ctx.setLineDash([]); ctx.lineDashOffset = 0;
    if (showArrows) {
      // Arrowhead: base -> tip along the tip tangent (points the way the curve arrives).
      const hw = alen * 0.55;
      ctx.beginPath(); ctx.moveTo(tipx, tipy);
      ctx.lineTo(basex - tdy*hw, basey + tdx*hw);
      ctx.lineTo(basex + tdy*hw, basey - tdx*hw);
      ctx.closePath(); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
    }
    }
  }
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = typeHl ? n.type === typeHl
      : edgeTypeHl ? lgEndpoints.has(n.id)
      : act ? act.ns.has(n.id) : true;
    const base = on ? 1 : 0.12;
    const r = nodeRadius(n);
    // Under the node: the selection's halo and this session's footprint glow - soft discs, so the
    // rings stay trust's alone (docs/design-system.md Section 5).
    if (n === anchor) {
      ctx.globalAlpha = base * 0.2; ctx.fillStyle = INK;
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 8, 0, 7); ctx.fill();
    }
    // The footprint is a light centred on the node that fades out. It was a disc dropped below the
    // node, which read as a circle out of place; a crisp disc on the centre would read as a ring, and
    // rings belong to trust.
    if (showFootprint && footprint.has(n.id)) {
      const glow = ctx.createRadialGradient(n.x, n.y, r * 0.8, n.x, n.y, r + 13);
      glow.addColorStop(0, "rgba(86,179,162,0.55)");   // --ok
      glow.addColorStop(1, "rgba(86,179,162,0)");
      ctx.globalAlpha = base; ctx.fillStyle = glow;
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 13, 0, 7); ctx.fill();
    }
    ctx.globalAlpha = base;
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    // Default stroke (background-color halo): sharpens the node boundary and separates it from edges/neighbors (visibility).
    // Being the background color, it stays a cutout that matches the background even when the theme changes.
    ctx.lineWidth = nodeStrokeW(n); ctx.strokeStyle = SURFACE; ctx.stroke();
    trustRing(n, r);
    // Group mode: a bridge node (linked to another group) carries a small square at its upper left -
    // a cross-group transit point, marked without a ring.
    if (clusterMode && bridgeSet.has(n.id)) {
      const q = 5 / cam.s;
      ctx.fillStyle = INK; ctx.fillRect(n.x - r * 0.8 - q, n.y - r * 0.8 - q, q, q);
    }
  }
  // Contested beliefs (resolution.md R6): a badge at the node's upper right, drawn in its own pass at
  // full strength, so neither focus dimming nor label thinning can mute it (U3). The inspector shows
  // the competing values with the mediation act.
  ctx.globalAlpha = 1;
  for (const n of nodes) {
    if (!n.contested || typeOff.has(n.type)) continue;
    const r = nodeRadius(n), br = 7 / cam.s, bx = n.x + r * 0.75, by = n.y - r * 0.75;
    ctx.beginPath(); ctx.arc(bx, by, br, 0, 7);
    ctx.fillStyle = BAD; ctx.fill(); ctx.lineWidth = 1.5 / cam.s; ctx.strokeStyle = SURFACE; ctx.stroke();
    ctx.fillStyle = SURFACE; ctx.textAlign = "center"; ctx.textBaseline = "middle";
    ctx.font = "600 " + (11 / cam.s) + "px " + KIND_FAMILY;
    ctx.fillText("!", bx, by + 0.5 / cam.s);
    ctx.textAlign = "left"; ctx.textBaseline = "alphabetic";
  }
  // Event pulses (nodes the agent touched) - an expanding, fading ring. rAF always runs, so it keeps
  // animating even after cooling.
  for (const [id, ttl] of pulses) {
    const n = nodeById(id);
    if (!n || ttl <= 0 || typeOff.has(n.type)) { pulses.delete(id); continue; }
    if (showPulses && !REDUCED_MOTION) {
      const t = 1 - ttl/60;
      ctx.globalAlpha = (1 - t) * 0.85;
      ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 3 + t*22, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = EDGE_HI; ctx.stroke();
    }
    pulses.set(id, ttl - 1);   // expiry advances even when hidden (avoids afterimages on toggle)
  }
  ctx.globalAlpha = 1;

  // Proposal preview (belief-diff visualization): when a proposal is selected, show on the graph what it
  // would change. For entity_merge: a dashed arrow from each fold-away target to the canonical `into`,
  // and the target's incident edges accented (they rewire); everything the merge does not touch is
  // dimmed by activeSet, and no node is ringed - rings are trust's (design-system.md Section 5). For an open proposal both nodes are still present (a before-preview); for a merged one the
  // targets are already folded away, so only the canonical is highlighted (the result).
  if (proposalSel && proposalSel.kind === "entity_merge") {
    const into = nodeById(proposalSel.into);
    const tgts = (proposalSel.targets || []).map(nodeById).filter(n => n && (!into || n.id !== into.id));
    const tgtIds = new Set(tgts.map(n => n.id));
    const ACC = GOLD;
    ctx.setLineDash([5/cam.s, 4/cam.s]);
    for (const e of edges) {   // edges that will rewire onto the canonical
      if (tgtIds.has(e.a.id) || tgtIds.has(e.b.id)) {
        ctx.globalAlpha = 0.95; ctx.strokeStyle = ACC; ctx.lineWidth = 2/cam.s;
        ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke();
      }
    }
    ctx.setLineDash([]);
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
    // Label thinning: everything when small (<=40) or zoomed in enough (cam.s>1.4); on a large graph,
    // only hubs (high degree >= cut) + hover/focus/active. Removes the hairball's wall of labels.
    const cut = (nodes.length <= 40 || cam.s > 1.4) ? 0 : Math.max(4, Math.round(nodes.length / 25));
    const want = [];
    for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      // Legend hover: label exactly the highlighted set (bypasses the degree cut - hover is transient).
      const lg = typeHl ? n.type === typeHl : edgeTypeHl ? lgEndpoints.has(n.id) : null;
      if (lg === false) continue;
      const on = lg === true ? true : (act ? act.ns.has(n.id) : true);
      const show = on && (lg === true || n === hover || n === focus || (act && act.ns.has(n.id)) || (n.degree || 0) >= cut);
      if (!show) continue;
      const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = nodeRadius(n)*cam.s;
      if (px + r + 5 > innerWidth || px + r + 5 + 600 < 0 || py < -20 || py > innerHeight + 20) continue;   // off screen
      want.push({ n, on, px: px + r + 5, py });
    }
    // Labels avoid each other and the region names (design-system.md Section 5): the focused and
    // hovered node first, then by degree, and a label that would overlap one already placed is left
    // out - its node is still there, and hovering it names it.
    const rank = o => (o.n === focus ? 2 : 0) + (o.n === hover ? 1 : 0);
    want.sort((a, b) => rank(b) - rank(a) || (b.n.degree || 0) - (a.n.degree || 0));
    ctx.font = "500 12px " + NAME_FAMILY;
    for (const o of want) {
      const w = ctx.measureText(o.n.name).width + 4, h = 15, x = o.px - 2, y = o.py - h / 2;
      if (rank(o) === 0 && labelBoxes.some(p => x < p.x + p.w && x + w > p.x && y < p.y + p.h && y + h > p.y)) continue;
      labelBoxes.push({ x, y, w, h });
      ctx.globalAlpha = o.on ? 1 : 0.25;
      drawLabel(o.n.name, "500", 12, (o.n === focus || o.n === hover) ? INK : INK2, "0px", 1.5, o.px, o.py, "left");
    }
    ctx.globalAlpha = 1;
    // (Group / hull labels are drawn in their own pass right after the hulls - see drawHullLabels below.)
    // Active-edge relation labels: only the hovered/focused node's edges, shortest first, capped so a
    // hub does not flood the canvas; the overflow is summarized as "+K more" under the active node.
    if (edgeLabels && edgeLabels.length) {
      edgeLabels.sort((p, q) => p.len - q.len);
      const shown = Math.min(edgeLabels.length, EDGE_LABEL_MAX);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.font = "11px " + KIND_FAMILY;   // relation kinds, at the scale's floor (docs/design-system.md Section 4)
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
        ctx.font = "11px " + NAME_FAMILY;
        const t = "+" + omitted + " more", px = snap(an.x*cam.s + cam.x), py = snap(an.y*cam.s + cam.y + nodeRadius(an)*cam.s + 15), w = ctx.measureText(t).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = INK2; ctx.fillText(t, px, py);
      }
      ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.globalAlpha = 1;
    }
  }
  // Peer cursor-dots (server mode) - drawn last so they float above the graph and labels.
  stepPeers(); drawPeers();
  drawMinimap();
  if (animating(act)) { layoutDirty = true; requestFrame(); }
  else if (layoutDirty) { layoutDirty = false; saveLayout(); }   // the picture came to rest - keep it
}

function nodeAt(sx, sy) {
  const [wx, wy] = toWorld(sx, sy);
  let best = null, bd = 1e9;
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d < nodeRadius(n) + 6 && d < bd) { bd = d; best = n; }
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
  tip.innerHTML = html`${[
    html`<b>${n.name}</b><br>`,
    html`<span class="k">type</span> ${n.type} &nbsp; <span class="k">degree</span> ${n.degree || 0}<br>`,
    html`<span class="k">sources</span> ${n.sources || 0} &nbsp; <span class="k">trust</span> ${n.trust_tier}`]}`;
}

// --- Interaction ---------------------------------------------------------------------
canvas.addEventListener("wheel", ev => {
  if (settling) return;   // graph is hidden behind the loader - ignore interaction until it settles
  ev.preventDefault();
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
}, { passive: false });

canvas.addEventListener("mousedown", ev => {
  if (settling) return;   // graph hidden (loading) - no drag/pan/select
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; }   // press alone does not reheat - wake only once the pointer actually drags (mousemove)
  else {
    // Clicking empty canvas clears any proposal preview.
    if (proposalSel) { proposalSel = null; requestFrame(); }
    panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing");
  }
});
addEventListener("mousemove", ev => {
  if (drag) { const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; wake(0.3); showTip(null); return; }
  if (panning) {
    // Panning is instant (1:1) - move cam and camT together so easing does not drag behind.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); return;
  }
  // Only canvas-targeted moves drive node hover from here on. Over the chrome (docks, header,
  // statusbar) the shared #tip belongs to whatever chrome element is showing it (legend chip
  // definitions) - and node hover through an opaque panel was wrong anyway.
  if (ev.target !== canvas) return;
  if (settling) { showTip(null); return; }   // no hover while the graph is hidden behind the loader
  const ph = peerAt(ev.clientX, ev.clientY);   // peer cursor-dots sit above the graph - test them first
  if (ph) { hover = null; showPeerTip(ph.m, ph.id, ev.clientX, ev.clientY); return; }
  hover = nodeAt(ev.clientX, ev.clientY);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  if (settling) { drag = null; panning = null; canvas.classList.remove("grabbing"); return; }   // drop any gesture that spanned into a reload
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    focus = n ? (focus === n ? null : n) : null;   // node click = toggle focus (pin), empty space = clear
    // Inspector first, camera second: insetB() MEASURES the open panel, so aiming before the panel
    // exists frames against the previous layout and drops the bottom of the neighbourhood behind it.
    // Every other focus path already ordered it this way; this one did not, and framing a box rather
    // than centring a point is what made it visible.
    renderDetail(focus);                           // show/clear the detail inspector
    if (focus) focusView(focus);   // frame the node with the neighbours the focus reveals - no reheat, the layout stays put
  }
  if (drag && moved) wake(0.3);   // settle neighbors only after a real drag, not a plain click
  drag = null;
  if (panning) { panning = null; canvas.classList.remove("grabbing"); }
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
  hybrid: ["keyword + meaning", "answered by the keyword and the semantic surface together"],
  keyword: ["keyword only", "answered by the keyword surface alone - no semantic index here, so a miss is more likely a recall miss"],
};
let searchAns = null;    // { q, ws, mode, hits, note } or { q, ws, error }
let searchRows = [];     // the hits in the order the list shows them
let searchSel = 0, searchSeq = 0, searchTimer = 0;

function searchQuery() { return searchEl.value.trim(); }
// An answer is shown only for the text in the field and the scope on the map; a workspace switch
// asks again rather than listing what another scope held.
function searchFresh() { return !!searchAns && searchAns.q === searchQuery() && searchAns.ws === currentWs(); }
function setSearchOpen(on) {
  on = on && searchFresh();
  searchPop.hidden = !on;
  searchEl.setAttribute("aria-expanded", on ? "true" : "false");
  searchLit = on && searchAns.hits
    ? new Set(searchAns.hits.filter(h => h.kind === "entity").map(h => h.id))
    : null;
  requestFrame();
}
async function runSearch() {
  const q = searchQuery();
  const seq = ++searchSeq;
  if (!q) { searchAns = null; renderSearch(); setSearchOpen(false); return; }
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
  searchSel = 0;
  renderSearch();
  setSearchOpen(document.activeElement === searchEl);
}
function searchNode(h) {
  const n = h.kind === "entity" ? nodeById(h.id) : null;
  return n && !typeOff.has(n.type) ? n : null;
}
function renderSearch() {
  if (!searchAns) { searchRows = []; searchPop.innerHTML = html``; return; }
  if (searchAns.error) {
    searchRows = [];
    searchPop.innerHTML = html`<div class="sp-note bad">${searchAns.error}</div>`;
    return;
  }
  const ents = searchAns.hits.filter(h => h.kind === "entity");
  const obs = searchAns.hits.filter(h => h.kind === "observation");
  searchRows = [...ents, ...obs];
  const [modeWord, modeWhy] = SEARCH_MODE[searchAns.mode] || [String(searchAns.mode || "?"), ""];
  const row = (h, i) => {
    const on = i === searchSel ? " on" : "";
    if (h.kind === "observation")
      return html`<button type="button" class="sp-row${on}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}"><span class="sp-snip">${h.snippet}</span></button>`;
    const n = searchNode(h);
    // An entity the answer holds but this view does not draw (its type is switched off, or the loaded
    // graph predates it) stays listed - it is part of the answer - and says why it cannot be focused.
    return html`<button type="button" class="sp-row${on}${n ? "" : " off"}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}">${
      n ? html`<span class="dot" style="background:${typeColor[n.type] || OTHER}"></span>` : ""}<span class="sp-name">${n ? n.name : h.snippet}</span>${
      n ? html`<span class="sp-k">${n.type}</span>` : html`<span class="sp-k">not on the map in this view</span>`}</button>`;
  };
  const count = searchAns.hits.length;
  searchPop.innerHTML = html`<div class="sp-head"><span>${count} ${count === 1 ? "hit" : "hits"}</span><span class="sp-mode" title="${modeWhy}">${modeWord}</span></div>${
    ents.length ? [html`<div class="sp-sec">Entities</div>`, ents.map((h, i) => row(h, i))] : ""}${
    obs.length ? [html`<div class="sp-sec">Observations</div>`, obs.map((h, i) => row(h, ents.length + i))] : ""}${
    count ? "" : html`<div class="sp-note">${searchAns.note || "no hits - absence is unknown, not a negation"}</div>`}`;
  searchPop.querySelectorAll(".sp-row").forEach(b => {
    // mousedown keeps the field focused, so the click below is the choice and not a blur first.
    b.onmousedown = ev => ev.preventDefault();
    b.onclick = () => chooseSearch(Number(b.dataset.i));
  });
  searchEl.setAttribute("aria-activedescendant", searchRows.length ? "sp" + searchSel : "");
}
function moveSearch(d) {
  if (!searchRows.length) return;
  searchSel = (searchSel + d + searchRows.length) % searchRows.length;
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
  if (h.kind === "entity") {
    const n = searchNode(h);
    if (!n) return;   // listed, not drawn: the row already says so
    closeSearch();
    if (reviewOpen()) location.hash = "";
    focus = n; renderDetail(n); focusView(n);
    return;
  }
  // An observation opens its card - from the Log's rows when they hold it, else read once - and the
  // canvas frames the entities it asserts.
  let o = (obsLog || []).find(x => x.id === h.id);
  if (!o) {
    const ws = currentWs();
    try {
      const r = await fetch("/api/observations" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
      if (r.ok) { const l = await r.json(); if (Array.isArray(l)) o = l.find(x => x.id === h.id); }
    } catch (e) { /* reported below */ }
  }
  if (!o) { toast("bad", "Could not open the observation", "the log of this scope did not hold " + h.id.slice(0, 10)); return; }
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
  } else if (ev.key === "Enter") {
    ev.preventDefault();
    clearTimeout(searchTimer);
    // Enter on text the list has not answered yet asks first, then takes the top row.
    if (searchFresh()) chooseSearch(searchSel);
    else if (searchQuery()) runSearch().then(() => { if (searchFresh()) chooseSearch(0); });
  } else if (ev.key === "Escape") {
    // Hand the keyboard back to the canvas, so "/" -> type -> Escape is a closed loop.
    // stopPropagation: the window handler reads Escape as "dismiss the obs card / view popover",
    // which is not what leaving the search field should do.
    ev.stopPropagation();
    closeSearch();
  }
});
document.getElementById("reload").onclick = () => { loadWorkspaces(); poll(); };
// The zoom buttons zoom about the map window's centre, which is not the canvas's when a panel is open.
function zoomWindow(f) { const w = mapWindow(); zoomAt((w.l + w.r)/2, (w.t + w.b)/2, f); }
document.getElementById("zin").onclick = () => zoomWindow(1.2);
document.getElementById("zout").onclick = () => zoomWindow(1/1.2);
document.getElementById("fit").onclick = () => { userMoved = true; fitView(); };
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; followBtn.classList.toggle("on", follow); };
const peersBtn = document.getElementById("peersBtn");
peersBtn.onclick = () => { peersOn = !peersOn; peersBtn.classList.toggle("on", peersOn); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; clusterBtn.classList.toggle("on", clusterMode); wake(0.6); poll(); };
const hyperBtn = document.getElementById("hyperBtn");
// Overlay is render-only (the draw loop shows it next frame) - do not reheat the sim. Fetch hyperedge
// data only if we do not already have it (e.g. when leaving group mode, where it was cleared).
hyperBtn.onclick = () => { hyperMode = !hyperMode; hyperBtn.classList.toggle("on", hyperMode); if (hyperMode && !hyperedges.length) poll(); };
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
  renderStrip();
  if (remember) {
    try { localStorage.setItem(isLeft ? "supra.dockL" : "supra.dockR", on ? "1" : "0"); } catch (_) { /* private mode */ }
  }
  // Under 1200px the canvas holds one panel at a time (design-system.md Section 6): opening a dock
  // closes the other and the inspector. Not remembered - the wide window's choice stands.
  if (on && narrowWindow()) {
    applyDock(!isLeft, false, false);
    if (focus) { focus = null; renderDetail(null); }
  }
}
function narrowWindow() { return innerWidth < 1200; }
function toggleDock(isLeft) { applyDock(isLeft, !(isLeft ? dockLEl : dockREl).classList.contains("on")); }
panelLBtn.onclick = () => toggleDock(true);
panelRBtn.onclick = () => toggleDock(false);
// View options popover (HUD "view" button): preferences stay off the data rails; a click
// elsewhere or Escape dismisses it.
const viewBtn = document.getElementById("viewBtn"), viewPop = document.getElementById("viewpop");
function setViewPop(on) { viewPop.classList.toggle("on", on); viewBtn.classList.toggle("on", on); }
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
  if (e.key === "[") toggleDock(true);
  else if (e.key === "]") toggleDock(false);
  else if (e.key === "/" && !e.metaKey && !e.ctrlKey && !e.altKey) {
    // preventDefault or the "/" itself lands in the field as the first character (and Firefox
    // opens its quick-find). The INPUT/TEXTAREA guard above already keeps this from hijacking
    // a "/" typed into any other field. select() so the shortcut replaces a stale term.
    e.preventDefault();
    searchEl.focus();
    searchEl.select();
  }
  else if (e.key === "b") {
    boundaryMode = boundaryMode === "workspace" ? "origin" : boundaryMode === "origin" ? "off" : "workspace";
    // The mode belongs in the stats line, which is durable, and not in `statusEl`, which the render
    // loop rewrites every frame - the first version put it there and the feedback was gone before it
    // could be read, so the toggle looked like it had done nothing.
    renderBoundaryChips();
    requestFrame();
  }
  // Esc closes the topmost thing: the observation card, then the view menu, then the inspector.
  else if (e.key === "Escape") {
    if (obscardEl.classList.contains("on")) hideObsCard();
    else if (viewPop.classList.contains("on")) setViewPop(false);
    else if (focus) { focus = null; renderDetail(null); requestFrame(); }
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
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; e.currentTarget.classList.toggle("on", showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; e.currentTarget.classList.toggle("on", showEdges); };
document.getElementById("arrowsBtn").onclick = e => { showArrows = !showArrows; e.currentTarget.classList.toggle("on", showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; e.currentTarget.classList.toggle("on", showFootprint); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; e.currentTarget.classList.toggle("on", showPulses); };
document.getElementById("histBtn").onclick = e => { showSuperseded = !showSuperseded; e.currentTarget.classList.toggle("on", showSuperseded); };
document.getElementById("settingsBtn").onclick = openSettings;
// The sync pill opens the settings at Sync. In the app the shell takes this click to the app's own
// Settings > Sync page, as it takes the gear's.
document.getElementById("syncPill").onclick = () => { settingsTab = "sync"; openSettings(); };
document.getElementById("miniBtn").onclick = e => { showMini = !showMini; e.currentTarget.classList.toggle("on", showMini); };
document.getElementById("regionsBtn").onclick = e => { allRegions = !allRegions; e.currentTarget.classList.toggle("on", allRegions); };
// The contested pill and the review pill open the Review page, where contested beliefs wait under
// Needs you.
document.getElementById("contestedPill").onclick = () => { reviewSeg = "needs"; location.hash = "#review"; };
document.getElementById("reviewPill").onclick = () => { reviewSeg = "needs"; location.hash = "#review"; };
// The view menu's choices persist, as the docks' open state does (docs/design-system.md Section 14
// step 1): a choice made once stays made. A preference, so it lives in this browser and nowhere else.
// peers is left out on purpose: it switches itself on for a hub, so a stored "off" would fight that.
{
  const VIEW_PREFS = ["followBtn", "clusterBtn", "hyperBtn", "regionsBtn", "labelsBtn", "edgesBtn",
    "arrowsBtn", "footBtn", "pulseBtn", "histBtn", "miniBtn"];
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem("supra.view") || "{}") || {}; } catch (_) { saved = {}; }
  // Any click stored every choice, so a stored minimap is rarely one anyone made: 0.6.0 and 0.6.1
  // started it off and stored that on the first click of any toggle. Forget it once more now that it
  // starts on (docs/design-system.md Section 6), and keep it from the next click. "all regions" is
  // the same case from 0.6.4, when it starts on: a stored "off" before then is forgotten once.
  if (!(saved.v >= 3)) delete saved.miniBtn;
  if (!(saved.v >= 4)) delete saved.regionsBtn;
  for (const id of VIEW_PREFS) {
    const b = document.getElementById(id);
    if (!b) continue;
    // Replay a stored choice through the button's own handler, so its state and side effects agree.
    if (typeof saved[id] === "boolean" && saved[id] !== b.classList.contains("on")) b.click();
    b.addEventListener("click", () => {
      const now = { v: 4 };
      for (const k of VIEW_PREFS) { const x = document.getElementById(k); if (x) now[k] = x.classList.contains("on"); }
      try { localStorage.setItem("supra.view", JSON.stringify(now)); } catch (_) { /* private mode - the choice lasts the session */ }
    });
  }
}

// Restore the workspace selection from the URL (deep link / reload) before the first poll.
{
  const qws = new URLSearchParams(location.search).get("workspace");
  if (qws !== null) wsInput.value = qws;
}
resize(); loadWorkspaces(); loadSurface().finally(poll); connectEvents(); detectServerMode();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); setInterval(refreshPeerRoster, 5000); draw();
loadSurface().then(() => { refreshSyncPill(); renderReview(); }); setInterval(refreshSyncPill, 5000);
