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
const GOLD = "#d9a544", TEAL = "#56b3a2";

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
const curationBodyEl = document.getElementById("curationBody"), reviewCtEl = document.getElementById("reviewCt");
const proposalsBodyEl = document.getElementById("proposalsBody"), propCtEl = document.getElementById("propCt");
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
let showFootprint = true, showPulses = true, showSuperseded = true, showMini = true;
const bridgeSet = new Set();     // ids of nodes connected to another type (linking nodes that join groups)
const pulses = new Map();        // id -> remaining frames (event-node highlight ring animation)
const CLUSTER_PULL = 0.03;       // pull toward the group target point (stronger than the center attraction)
const HYPER_PULL = 0.03;         // base hyperedge centroid cohesion (scaled by hull size - see hullSizeNorm)
// Cohesion scales with member count: larger hulls pull their nodes tighter (small ones stay loose).
const HULL_SIZE_REF = 8;         // members at/above which a hull gets full cohesion weight
const HULL_COH_MIN = 0.4, HULL_COH_MAX = 1.15;    // cohesion factor range (x HYPER_PULL)
// Hull rendering: each hull is a single outward-offset rounded path (see roundedHullPath) filled once,
// directly on the canvas, at the opacity below. No stroke -> no fill/stroke seam; no offscreen -> cheap.
// A hull's whole area gets uniform transparency, while different hulls blend where they overlap.
const HULL_LAYER_ALPHA = 0.13;   // per-hull fill opacity (no node active) - tuned down for the darker candlelight ink, where the old value read as heavy plum slabs
const HULL_LAYER_DIM = 0.04;     // non-active hulls fade to this while a node is active (inspection view)
const HULL_ACTIVE_ALPHA = 0.34;  // the active node's own hulls, painted on top
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
let searchTerm = "";
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
    // are hot - so a search term matching nodes but no edges is a still picture, not motion.
    || (act !== null && act.es.size > 0);
}

// --- Camera (canvas is fullscreen, mouse uses client coordinates) ----------------------------
function toWorld(sx, sy) { return [(sx - cam.x) / cam.s, (sy - cam.y) / cam.s]; }
function easeCam() {
  const k = 0.22;
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
const TOP_INSET = 52;      // height occluded by the top header - compensated in centering/fit
const BOTTOM_INSET = 24;   // height occluded by the bottom status bar
const DOCK_L = 262, DOCK_R = 312;   // island inset (12) + card width (match the CSS) - reserved so content is not hidden under them
function insetL() { return dockLEl.classList.contains("on") ? DOCK_L : 0; }
function insetR() { return dockREl.classList.contains("on") ? DOCK_R : 0; }
// Bottom occlusion: the status bar, plus the detail panel when it is open (panel sits at bottom:30,
// measured live so centering keeps the focused node visible above it). Render detail before centerOn.
function insetB() {
  if (!detailEl.classList.contains("on")) return BOTTOM_INSET;
  const h = detailEl.getBoundingClientRect().height;
  return h ? 36 + h + 8 : BOTTOM_INSET;   // 36 = the detail panel's bottom offset (match the CSS)
}
// Smoothly bring ONE node to the screen center (focus-to-zoom). If zoomed too far out, zoom in
// slightly. Used where there is nothing but the node to frame: the activity feed following a single
// hit, and focusView's fallback for a node with no visible neighbours.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = (insetL() + innerWidth - insetR()) / 2 - n.x * camT.s;   // center in the strip between the rails
  camT.y = (innerHeight + TOP_INSET - insetB()) / 2 - n.y * camT.s; userMoved = true;   // above the detail panel
}

function assignColors() {
  const types = [...new Set(nodes.map(n => n.type))].sort();
  typeColor = {};
  types.forEach((t, i) => { typeColor[t] = catColor(i, false); });
  // Color per edge kind (relation kind) - deterministic (in sorted kind order), generated in the edge band.
  const ek = [...new Set(edges.map(e => e.type))].sort();
  edgeTypeColor = {};
  ek.forEach((t, i) => { edgeTypeColor[t] = catColor(i, true); });
}

function applyGraph(g) {
  const seen = new Set();
  let added = false;
  nodes = g.nodes.map(n => {
    let p = posById.get(n.id);
    if (!p) {
      added = true;
      const i = spiralN++, a = i * 2.39996, r = 60 + 30 * Math.sqrt(i);
      p = { x: innerWidth/2 + r*Math.cos(a), y: innerHeight/2 + r*Math.sin(a), vx:0, vy:0 };
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
  // hid the very peer-marker tour that a push triggers.
  if (added || removed) wake(firstData ? 0.7 : 0.4);

  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  edges = g.edges.map(e => Object.assign({}, e, { a: byId[e.from], b: byId[e.to] }))
                 .filter(e => e.a && e.b);
  // Bridge nodes: nodes connected to another type (group) - linking/navigation points that join groups.
  bridgeSet.clear();
  for (const e of edges) if (e.a.type !== e.b.type) { bridgeSet.add(e.a.id); bridgeSet.add(e.b.id); }
  assignColors();
  renderLegend();
  // Keep the federation panel fresh while it is open (hub health / diff / peers move on their own).
  const peersPanel = document.querySelector('.tabpanel[data-panel="peers"]');
  if (peersPanel && peersPanel.classList.contains("on")) refreshPeers();
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
  if (firstData && nodes.length) { firstData = false; needFit = true; }
  // wake() only fires when the node set changed; a tier, kind or contested flag that moved
  // repaints without moving anything, so it has to ask for the frame itself.
  requestFrame();
}

// Federation panel: hubs (health + per-workspace diff vs this node) and, on a hub, the known-peer
// registry (who checked in, what they did, how long ago). Data = /api/federation (wiring-layer blob).
async function refreshPeers() {
  const host = document.getElementById("peersBody");
  const ct = document.getElementById("peersCt");
  try {
    const r = await fetch("/api/federation", { cache: "no-store" });
    const f = await r.json();
    if (!f || f.configured === false) {
      host.innerHTML = '<div class="empty">federation is not configured on this node</div>';
      ct.textContent = "";
      return;
    }
    let html = `<div class="hint">this node: ${esc(String(f.node_id || "").slice(0, 16))} (${esc(f.role || "client")})</div>`;
    const hubs = f.servers || [];
    if (hubs.length) {
      html += `<div class="fsec">Hubs</div>`;
      for (const s of hubs) {
        const dot = s.healthy ? TEAL : "#d96a5f";
        html += `<div class="fed"><span class="dot" style="background:${dot}"></span>`
          + `<span class="furl" title="${esc(s.url)}">${esc(s.url.replace(/^https?:\/\//, ""))}</span>`
          + (s.version ? `<span class="hint">v${esc(s.version)}</span>` : "") + `</div>`;
        for (const w of (s.workspaces || [])) {
          const insync = !(w.local_ahead | 0) && !(w.hub_ahead | 0);
          html += `<div class="fws">${esc(w.workspace)}: ` + (insync
            ? `<span style="color:${TEAL}">in sync</span>`
            : `local +${w.local_ahead | 0} / hub +${w.hub_ahead | 0}`) + `</div>`;
        }
      }
    }
    const peers = f.known_peers || [];
    if (peers.length) {
      html += `<div class="fsec">Known peers</div>`;
      for (const p of peers) {
        const ago = f.updated_ms && p.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - p.last_seen_ms) / 1000)) : null;
        html += `<div class="fed"><span class="dot" style="background:${GOLD}"></span>`
          + `<span class="furl" title="${esc(p.node_id)}">${esc(p.node_id.slice(0, 16))}</span>`
          + `<span class="hint">${esc(p.last_action)}${ago !== null ? " " + ago + "s ago" : ""} (${p.hits})</span></div>`;
      }
    } else if (f.role === "hub") {
      html += `<div class="fsec">Known peers</div><div class="empty">no peer has checked in yet</div>`;
    }
    // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
    host.innerHTML = html;
    const healthy = hubs.filter(s => s.healthy).length;
    ct.textContent = hubs.length ? `${healthy}/${hubs.length}` : (peers.length || "");
  } catch (e) {
    host.innerHTML = '<div class="empty">federation status unavailable</div>';
  }
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
  { id: "peers", label: "Peers", render: renderFedSection },
];
let settingsTab = "about";

async function renderSettings() {
  const strip = document.getElementById("settingsTabs");
  const host = document.getElementById("settingsBody");
  strip.textContent = "";
  for (const t of SETTINGS_TABS) {
    const b = document.createElement("button");
    b.className = "stab" + (t.id === settingsTab ? " on" : "");
    b.textContent = t.label;
    b.onclick = () => { settingsTab = t.id; renderSettings(); };
    strip.appendChild(b);
  }
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
  const row = (k, v) => `<dt>${esc(k)}</dt><dd>${v}</dd>`;
  // eslint-disable-next-line no-unsanitized/property -- every interpolation goes through esc()
  host.innerHTML = `<dl class="kv">`
    + row("package", esc(String(a.name || "supragnosis")))
    + row("version", esc(String(a.version || "")))
    + row("licence", esc(String(a.license || "")))
    + (repo ? row("source", `<a href="${esc(repo)}" target="_blank" rel="noreferrer noopener">${esc(repo)}</a>`) : "")
    + `</dl>`
    + `<div class="note">Dependency licences are not restated here - a hand-kept list is one that `
    + `rots without saying so. They are in the manifest and lockfile at the source above, which `
    + `cannot disagree with this build.</div>`;
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

  let html = `<div class="fsec">This node</div>`
    + `<div class="prow"><span class="pid">${esc(String(f.node_id || ""))}</span>`
    + `<span class="hint"> ${esc(f.role || "client")}</span></div>`;

  const hubs = f.servers || [];
  if (hubs.length) {
    html += `<div class="fsec">Hubs this node syncs to</div>`;
    for (const s of hubs) {
      const dot = s.healthy ? TEAL : "#d96a5f";
      html += `<div class="prow"><span class="dot" style="background:${dot}"></span> `
        + `<span class="pid">${esc(String(s.url || "").replace(/^https?:\/\//, ""))}</span>`
        + (s.version ? `<span class="hint"> v${esc(s.version)}</span>` : "")
        + (s.healthy ? "" : `<span class="hint"> unreachable</span>`);
      for (const w of (s.workspaces || [])) {
        const insync = !(w.local_ahead | 0) && !(w.hub_ahead | 0);
        html += `<div class="hint">${esc(w.workspace)}: ` + (insync
          ? `in sync`
          : `local +${w.local_ahead | 0} / hub +${w.hub_ahead | 0}`) + `</div>`;
      }
      html += `</div>`;
    }
  }

  const admitted = f.admitted || [];
  if (f.role === "hub") {
    html += `<div class="fsec">Peers admitted here, and what each may read</div>`;
    if (!admitted.length) {
      html += `<div class="prow none">no peer is admitted - add one in supragnosis.toml</div>`;
    }
    for (const a of admitted) {
      const ws = a.shared_workspaces || [];
      html += `<div class="prow"><span class="pid">${esc(String(a.node_id || ""))}</span>`
        + `<div class="wschips">`;
      if (!ws.length) {
        html += `<span class="none">admitted, may read nothing</span>`;
      } else {
        for (const w of ws) {
          const armed = fedCfgArmed === a.node_id + "\u0000" + w;
          html += `<span class="wsc${armed ? " armed" : ""}" data-node="${esc(a.node_id)}" `
            + `data-ws="${esc(w)}" title="${armed ? "click again to stop sharing" : "stop sharing this workspace with this peer"}">`
            + `${esc(w)}<span class="x">${armed ? "confirm" : "x"}</span></span>`;
        }
      }
      html += `</div></div>`;
    }
    html += `<div class="note">Removing a grant takes effect at once and is written to `
      + `supragnosis.toml. It stops FUTURE reads - it does not recall what has already synced. `
      + `Granting a workspace, and adding or removing a peer, stay in the file.</div>`;
  }

  // eslint-disable-next-line no-unsanitized/property -- every interpolation above goes through esc()
  host.innerHTML = html;

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
      refreshPeers();
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
      const el = document.createElement("span");
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      el.appendChild(sw); el.appendChild(document.createTextNode(t || "(none)"));
      el.onclick = () => { if (offSet.has(t)) offSet.delete(t); else offSet.add(t); renderLegend(); };
      // Hovering a chip highlights its nodes/edges on the graph (render-only - no sim wake) and
      // shows the type's T-Box definition in the styled tooltip (Principle 8: a type has a stated
      // meaning - surfaced right where the type is read, not only in the Types tab).
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
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  tip.innerHTML = `<b>${esc(t || "(none)")}</b> <span class="k">${target} type</span>`
    + (def
      ? `<div class="tdef">${esc(def.description)}</div><span class="k">${def.sources} src</span>`
      : `<div class="tdef none">no definition recorded - give this type a meaning with define_type</div>`);
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
  if (searchTerm) {
    const ns = new Set();
    for (const n of nodes) if (n.name.toLowerCase().includes(searchTerm)) ns.add(n.id);
    return { ns, es: new Set() };
  }
  return null;
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
      const c = document.createElement("span");
      c.className = "chip" + (val === cur ? " on" : "");
      c.dataset.ws = val; c.textContent = label;
      c.onclick = () => {
        if (wsInput.value.trim() === val) return;
        wsInput.value = val; beginWorkspaceTransition(); renderChipsActive(); poll();
      };
      return c;
    };
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = "workspaces:";
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
    let h = `<div class="item"><span class="nm">${esc(x.name)}</span>`
      + (spansWorkspaces ? `<span class="gws">${esc(String(x.workspace || ""))}</span>` : "")
      + `<span class="src">${x.sources} src</span>`
      + `<div class="def">${esc(x.description)}</div>`;
    // Contested definitions reuse the shared contested UI (keep / use this), so a type conflict and
    // an entity-kind conflict read identically - one mediation pattern (IR5).
    if (x.competitors && x.competitors.length) {
      h += `<div class="contested${x.contested ? " hot" : ""}">`
        + contestedRows(x.description, "", x.def_source, x.competitors, x.contested)
        + `</div>`;
    }
    return h + `</div>`;
  };
  const section = (title, items) => `<div class="gsec">${title} (${items.length})</div>`
    + (items.length ? items.map(item).join("") : `<div class="empty">none defined - use define_type</div>`);
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  glossaryBodyEl.innerHTML =
    section("entity types", group("entity")) + section("relation types", group("relation"));
  glossCtEl.textContent = glossaryTypes.length || "";
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
function renderCuration() {
  if (!curation) { curationBodyEl.innerHTML = '<div class="empty">no signals yet</div>'; reviewCtEl.textContent = ""; return; }
  const nchip = n => `<span class="nchip" data-id="${esc(n.id)}" title="focus ${esc(n.name)} (deg ${n.degree}, ${n.sources} src)">${esc(n.name)}<span class="ty">${esc(n.type)}</span></span>`;
  const dup = curation.duplicates || [], gb = curation.grab_bags || [], orph = curation.orphans || [];
  const con = curation.contradictions || [];
  // Contested beliefs first (resolution.md 4.2): the signals where the system explicitly has no
  // ground to choose, so a human call is the only thing that settles them. Confirm = /api/resolve.
  let html = `<div class="csec">contested beliefs (${con.length})</div>`;
  html += con.length
    ? con.map(c =>
        `<div class="grp"><span class="nchip" data-id="${esc(c.id)}" title="focus ${esc(c.name)}">${esc(c.name)}</span>`
        + `<div class="contested${c.contested ? " hot" : ""}">`
        + contestedRows(c.current, "", c.kind_source, c.competitors || [], c.contested)
        + `</div></div>`).join("")
    : `<div class="empty">none - no live kind conflicts</div>`;
  // Contradictory accepted merges (Principle 6): the projection resolves the cycle by parity, but
  // the cycle itself is surfaced - the remedy is a settling entity_merge proposal, never an edit.
  const mc = curation.merge_cycles || [];
  html += `<div class="csec">merge cycles (${mc.length})</div>`;
  html += mc.length
    ? mc.map(c =>
        `<div class="grp"><div class="chips">${(c.members || []).map(nchip).join("")}</div>`
        + `<div class="hint">these accepted merges fold into each other - settle with a new entity_merge</div></div>`).join("")
    : `<div class="empty">none - no contradictory merges</div>`;
  // Merge band (resolution-identity.md Section 3, Principle 15): embedding-near distinct-name pairs
  // the substrate proposes as merge candidates. A suggestion commits nothing (IR2); "propose"
  // opens an entity_merge through the gate, which then rides the accept flow in the Proposals tab.
  const ms = curation.merge_suggestions || [];
  html += `<div class="csec">merge suggestions (${ms.length})</div>`;
  html += ms.length
    ? ms.map(m =>
        `<div class="ms" data-a="${esc(m.a)}" data-b="${esc(m.b)}">`
        + `<span class="nchip" data-id="${esc(m.a)}" title="focus ${esc(m.a_name)}">${esc(m.a_name)}</span>`
        + `<span class="msx">~</span>`
        + `<span class="nchip" data-id="${esc(m.b)}" title="focus ${esc(m.b_name)}">${esc(m.b_name)}</span>`
        + `<span class="mssim" title="embedding similarity (recall aid) / shared neighbors">${m.similarity.toFixed(2)}${m.shared_neighbors ? " / " + m.shared_neighbors + "nb" : ""}</span>`
        + `<button class="propmerge" title="open an entity_merge proposal for this pair (folds the first into the second) - reviewed in the Proposals tab">propose</button>`
        + `</div>`).join("")
    : `<div class="empty">${bandEmptyReason(curation.merge_band)}</div>`;
  // Name variants: the deterministic sibling of the merge band (Principle 15/16). Entity ids already
  // fold case/whitespace, so these are the orthographic collisions nothing else catches - and unlike
  // the band above they need no embedder, so this section is populated on every node. A pair reuses
  // the .ms row so the existing propose wiring applies unchanged; 3+ members show as chips only
  // (propose_merge takes a pair). Commits nothing either way - propose routes through the gate.
  const nv = curation.name_variants || [];
  html += `<div class="csec">name variants (${nv.length})</div>`;
  html += nv.length
    ? nv.map(v => {
        const meta = `<span class="mssim" title="which normalization rung grouped them / shared neighbors (structural corroboration)">${esc(v.rung)}${v.shared_neighbors ? " / " + v.shared_neighbors + "nb" : ""}</span>`;
        const m = v.members || [];
        if (m.length === 2) {
          // data-src tells the server which surface produced this, so the proposal records the real
          // evidence instead of inheriting the merge band's "embedding-near" rationale.
          return `<div class="ms" data-a="${esc(m[0].id)}" data-b="${esc(m[1].id)}" data-src="variant:${esc(v.rung)}">`
            + `<span class="nchip" data-id="${esc(m[0].id)}" title="focus ${esc(m[0].name)}">${esc(m[0].name)}</span>`
            + `<span class="msx">~</span>`
            + `<span class="nchip" data-id="${esc(m[1].id)}" title="focus ${esc(m[1].name)}">${esc(m[1].name)}</span>`
            + meta
            + `<button class="propmerge" title="open an entity_merge proposal for this pair (folds the first into the second) - reviewed in the Proposals tab">propose</button>`
            + `</div>`;
        }
        return `<div class="grp"><span class="gk">${esc(v.key)}</span>${meta}<div class="chips">${m.map(nchip).join("")}</div></div>`;
      }).join("")
    : `<div class="empty">none - no orthographic variants</div>`;
  html += `<div class="csec">cross-workspace name collisions (${dup.length})</div>`;
  html += dup.length
    ? dup.map(g => `<div class="grp"><span class="gk">${esc(g.key)}</span><div class="chips">${g.members.map(nchip).join("")}</div></div>`).join("")
    : `<div class="empty">none - this signal only fires across workspaces (same-workspace variants are above)</div>`;
  html += `<div class="csec">grab-bag contexts (${gb.length})</div>`;
  html += gb.length
    ? gb.map(b => { const nm = b.member_names.slice(0, 10).join(", ") + (b.member_names.length > 10 ? ", ..." : ""); return `<div class="gb" data-hid="${esc(b.id)}"><span class="sz">${b.size}</span>${esc(nm)}<button class="reify" title="assert this context as a group entity + member_of relations (a lineage-bearing observation - the hyperedge itself stays a derived view)">reify</button></div>`; }).join("")
    : `<div class="empty">none - no oversized clusters</div>`;
  html += `<div class="csec">orphans (${orph.length})</div>`;
  html += orph.length ? `<div class="chips">${orph.map(nchip).join("")}</div>` : `<div class="empty">none - all nodes linked</div>`;
  // T-Box axis collisions (Principle 9): a name defined on both the entity and relation axes -
  // informative, usually a mistake. Mediation lives in the Types tab (per-definition confirm).
  const ax = curation.type_axis_collisions || [];
  if (ax.length) {
    html += `<div class="csec">type axis collisions (${ax.length})</div>`;
    html += `<div class="gb">${ax.map(n => `<span class="gk">${esc(n)}</span>`).join(", ")}<div class="hint">defined as both an entity type and a relation type - see the Types tab</div></div>`;
  }
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  curationBodyEl.innerHTML = html;
  const s = curation.stats || {};
  reviewCtEl.textContent =
    (s.contradictions || 0) + (s.merge_cycles || 0) + (s.merge_suggestions || 0)
    + (s.name_variants || 0)
    + (s.duplicate_groups || 0) + (s.grab_bags || 0) + (s.orphans || 0)
    + (s.type_axis_collisions || 0) || "";
  curationBodyEl.querySelectorAll(".nchip").forEach(c => {
    c.onclick = () => { const n = nodeById(c.dataset.id); if (n) { focus = n; renderDetail(n); focusView(n); } };
  });
  curationBodyEl.querySelectorAll(".confirm").forEach(b => {
    b.onclick = (ev) => { ev.stopPropagation(); resolveBelief(b.dataset.obs); };
  });
  // Reify (Principle 11 promotion path): name the context, assert it as a group entity through
  // /api/reify (an ordinary lineage-bearing observation - free ingest, no gate on the assertion).
  // DOM-built inline form (no innerHTML sink for the user-typed name).
  curationBodyEl.querySelectorAll(".gb .reify").forEach(b => {
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
        try { await fetch("/api/reify" + q, { cache: "no-store" }); } catch (e) { /* poll re-syncs */ }
        await poll();   // the group node + member_of edges appear; the panel re-renders
      };
      form.append(inp, ok);
      row.appendChild(form);
      inp.focus();
    };
  });
  // Merge band (Principle 15): "propose" opens an entity_merge through the gate (/api/propose_merge).
  // The pair leaves the band (now in flight) and appears in the Proposals tab for accept/reject.
  curationBodyEl.querySelectorAll(".ms .propmerge").forEach(b => {
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
      try { await fetch("/api/propose_merge" + q, { cache: "no-store" }); } catch (e) { /* poll re-syncs */ }
      await poll();   // the suggestion drops (now open); the proposal shows in the Proposals tab
    };
  });
}

async function refreshCuration() {
  if (!panelOn("review")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/curation" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const c = await r.json(); if (!c.error) curation = c; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderCuration();
}

// Proposals panel (the gated curation console, Principle 23). Read + accept/reject. Accept goes through
// the gated verdict path (/api/review -> engine.review_proposal, a verdict observation), not a direct write.
function nameOf(id) { const n = nodeById(id); return n ? n.name : "(" + id.slice(0, 8) + ")"; }
function renderProposals() {
  const open = proposals.filter(p => p.state === "open");
  propCtEl.textContent = open.length || (proposals.length ? proposals.length : "");
  if (!proposals.length) { proposalsBodyEl.innerHTML = '<div class="empty">no proposals - open one with the propose tool, or from a merge candidate</div>'; return; }
  const chip = (id, into) => `<span class="nchip${id === into ? " into" : ""}" data-id="${esc(id)}" title="focus ${esc(nameOf(id))}${id === into ? " (canonical / into)" : ""}">${esc(nameOf(id))}</span>`;
  let html = `<div class="hint">click a proposal to preview the change on the graph; accept records a gated verdict</div>`;
  for (const p of proposals) {
    const st = esc(p.state);
    const sel = proposalSel && proposalSel.id === p.id ? " sel" : "";
    html += `<div class="prop${sel}" data-pid="${esc(p.id)}"><div class="phead"><span class="pkind">${esc(p.kind)}</span>`
      + `<span class="pstate ${st}">${st}${p.verdicts ? " " + p.verdicts + "v" : ""}</span></div>`;
    if (p.rationale) html += `<div class="prat">${esc(p.rationale)}</div>`;
    html += `<div class="ptargets">${(p.targets || []).map(id => chip(id, p.into)).join("")}</div>`;
    if (p.affected_types && p.affected_types.length) {   // tbox_change scope - what lights up on the graph
      const aty = a => `<span class="atype" title="${esc(a.target)} type"><span>${esc(a.name)}</span><span class="ax">${a.target === "relation" ? "edge" : "node"}</span></span>`;
      html += `<div class="atypes">${p.affected_types.map(aty).join("")}</div>`;
    }
    // The selected proposal shows its computed diff: what accepting would change, before accepting.
    if (sel) html += diffHtml(proposalDiff) + checksHtml(proposalChecks);
    if (p.state === "open") {
      const blocked = sel && proposalChecks && proposalChecks.some(c => c.blocking && !c.passed);
      html += `<div class="pacts"><button data-act="merge" data-id="${esc(p.id)}"${blocked ? " disabled title=\"a blocking check fails - the fold would refuse this merge\"" : ""}>accept</button>`
        + `<button data-act="reject" data-id="${esc(p.id)}">reject</button></div>`;
    }
    html += `</div>`;
  }
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  proposalsBodyEl.innerHTML = html;
  // Click a proposal row -> preview the change on the graph (belief-diff visualization). Chips/buttons
  // keep their own actions (stopPropagation), so only the row body toggles the preview.
  proposalsBodyEl.querySelectorAll(".prop").forEach(row => {
    row.onclick = () => selectProposal(proposals.find(x => x.id === row.dataset.pid));
  });
  proposalsBodyEl.querySelectorAll(".nchip").forEach(c => {
    c.onclick = (ev) => { ev.stopPropagation(); const n = nodeById(c.dataset.id); if (n) { focus = n; renderDetail(n); focusView(n); } };
  });
  proposalsBodyEl.querySelectorAll(".pacts button").forEach(b => {
    b.onclick = async (ev) => {
      ev.stopPropagation();
      const ws = wsInput.value.trim();
      const q = "?proposal=" + encodeURIComponent(b.dataset.id) + "&decision=" + b.dataset.act + (ws ? "&workspace=" + encodeURIComponent(ws) : "");
      // No CSRF header needed: the socket (0600) admits no third-party origin - this page, proxied
      // into the desktop shell, is the only one on the surface - so there is no attacker origin for
      // a forged request to come from. The transport itself is the write gate.
      try { await fetch("/api/review" + q, { cache: "no-store" }); } catch (e) { /* ignore */ }
      refreshProposals();
    };
  });
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
      renderProposals();
    }
  } catch (e) { /* the overlay still works - the diff is additive */ }
}

// Render the diff as a before -> after comparison. An uncomputable diff says so: for the three
// proposal kinds that still enforce nothing, an empty diff would read as "changes nothing".
function diffHtml(d) {
  if (!d) return `<div class="dnote">computing the diff...</div>`;
  if (!d.computable) return `<div class="dnote">no diff - ${esc(d.note || "not computable for this kind")}</div>`;
  const tiers = (d.tier_changes || []).map(t =>
    `<div class="drow"><span class="dk">tier</span><code>${esc(t.observation.slice(0, 10))}</code>`
    + `<span class="dfrom">${esc(t.from)}</span><span class="darr">-&gt;</span><span class="dto">${esc(t.to)}</span></div>`).join("");
  const beliefs = (d.overturned || []).map(b => {
    const settled = b.contested_before && !b.contested_after;
    const created = !b.contested_before && b.contested_after;
    const flag = settled ? `<span class="dsettled">settles a contradiction</span>`
               : created ? `<span class="dcreated">creates a contradiction</span>` : "";
    return `<div class="drow"><span class="dk">${esc(b.field)}</span>`
      + `<span class="nchip" data-id="${esc(b.entity)}" title="focus ${esc(b.name)}">${esc(b.name)}</span>`
      + `<span class="dfrom">${esc(b.from || "(none)")}</span><span class="darr">-&gt;</span>`
      + `<span class="dto">${esc(b.to || "(none)")}</span>${flag}</div>`;
  }).join("");
  // entity_merge: which references move onto the canonical id, and which edges stop existing.
  // A self-loop is dropped by graph(), so that edge vanishes on accept - not readable from the
  // canvas overlay, which can only accent edges incident to a target.
  const rewires = (d.rewired || []).map(r =>
    `<div class="drow"><span class="dk">edge</span><code>${esc(r.kind)}</code>`
    + `<span class="dfrom">${esc(r.from_name)}</span><span class="darr">-&gt;</span>`
    + `<span class="dto">${esc(r.to_name)}</span>`
    + `<span class="dother">(${esc(r.other_name)})</span>`
    + (r.becomes_self_loop ? `<span class="dcreated">becomes a self-loop, edge disappears</span>` : "")
    + `</div>`).join("");
  if (!tiers && !beliefs && !rewires) return `<div class="dnote">computed: this proposal overturns no current belief</div>`;
  return `<div class="ddiff">${tiers}${beliefs}${rewires}</div>`;
}

// Failing blocking checks, shown above the accept button. A blocking failure means the fold would
// refuse the merge anyway, so surfacing it here turns a silent "nothing happened" into a reason.
function checksHtml(cs) {
  if (!cs || !cs.length) return "";
  const bad = cs.filter(c => c.blocking && !c.passed);
  if (!bad.length) return `<div class="dnote">checks pass</div>`;
  return `<div class="cblock">${bad.map(c =>
    `<div class="crow"><span class="cbad">blocked</span><span class="ck">${esc(c.name)}</span>`
    + `<span class="cd">${esc(c.detail)}</span></div>`).join("")}</div>`;
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
  renderProposals();
}

async function refreshProposals() {
  if (!panelOn("proposals")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/proposals" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const p = await r.json(); if (Array.isArray(p)) proposals = p; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderProposals();
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
function tierDot(t) { return `<span class="tdot t-${esc(String(t))}" title="${esc(String(t))}"></span>`; }

// Content-first row: a quiet meta line (tier dot + compact time) over the observation content (the
// primary element, clamped to 2 lines). Host and the rest move to the expanded provenance.
// A proposal event said in names. Its stored content is machine text with raw ids in it, and that
// text is inside the content address so it can never be rewritten - the log read as rows of hashes.
// The server resolves the ids (it can still name a merged-away entity, which this side cannot, since
// the graph folds those away); this only phrases the result. Falls back to the raw content whenever
// the server had nothing to add, so an untranslatable row still says something.
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
    : p.event === "verdict" ? `${p.decision === "merge" ? "accepted" : esc(p.decision || "verdict")} ${kind}`
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
  return `<div class="obs" data-id="${esc(o.id)}">`
    + `<div class="ohead" title="open observation detail">`
    +   `<div class="ometa">${tierDot(o.effective_tier)}`
    +     `<span class="owhen">${esc(obsWhen(a0.observed_at))}</span></div>`
    +   `<div class="${cls}">${esc(text)}</div>`
    + `</div></div>`;
}

// The observation detail CARD: a focused, dismissable view of one observation - full content plus
// its provenance (every attestation), lineage (derived_from), and the entities/relations it asserted.
// The single detail surface for both the node log column and the workspace Log tab (a log row opens
// it, so the list stays a scannable list - the earlier cramped in-column expansion is gone).
function obsCardHtml(o) {
  const a0 = (o.attestations || [])[0] || {};
  let h = `<div class="card">`
    + `<button class="close" title="close" aria-label="close">`
    +   `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`
    +   `<path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg></button>`
    + `<div class="oc-meta">${tierDot(o.effective_tier)}<span class="octier">${esc(String(o.effective_tier))}</span>`
    +   `<span class="ocwhen">${esc(obsWhenFull(a0.observed_at))}</span></div>`
    // A proposal event leads with the readable form, but keeps the stored text underneath: this
    // card is the dereference surface an id resolves to (P2/P14), so what is literally in the log
    // has to stay inspectable here even once it is no longer what the reader is shown.
    + `<div class="oc-content">${esc(o.proposal ? proposalLine(o.proposal) : o.content)}</div>`
    + (o.proposal ? `<div class="ocraw" title="the text stored in the log - fixed inside the content address">${esc(o.content)}</div>` : "")
    + `<div class="osec">provenance (${(o.attestations || []).length})</div>`;
  for (const a of o.attestations || []) {
    h += `<div class="prow"><span class="phost">${esc(a.host)}</span>`
      + (a.on_behalf_of ? `<span class="pobo">for ${esc(a.on_behalf_of)}</span>` : "")
      + `<span class="ptier">${esc(String(a.trust_tier))}${a.evaluated_tier !== a.trust_tier ? " -> " + esc(String(a.evaluated_tier)) : ""}</span>`
      + (a.confidence != null ? `<span class="pconf">conf ${esc(String(a.confidence))}</span>` : "")
      + (a.origin_node ? `<span class="poid">node ${esc(String(a.origin_node).slice(0, 8))}</span>` : "")
      + `<span class="pwhen">${esc(obsWhenFull(a.observed_at))}</span></div>`;
  }
  if (o.derived_from && o.derived_from.length)
    h += `<div class="osec">derived from</div><div class="oids">`
      + o.derived_from.map(id => `<span class="oid">${esc(String(id).slice(0, 10))}</span>`).join("") + `</div>`;
  if (o.proposal) {
    const p = o.proposal;
    h += `<div class="osec">proposal</div>`
      + `<div class="ocprop"><span class="pkind">${esc(p.kind || "proposal")}</span>`
      +   `<span class="pstate ${esc(p.state || "")}">${esc(p.state || "")}</span>`
      +   `<span class="oid">${esc(String(p.proposal).slice(0, 10))}</span></div>`;
    const ts = p.targets || [];
    if (ts.length) {
      // A target that no longer has a node is one this merge folded away - it stays listed (the act
      // touched it) but is not offered as a jump, because there is nothing left to jump to.
      const chip = t => {
        const live = !!nodeById(t.id);
        const into = p.into && t.id === p.into.id;
        const label = esc(t.name || String(t.id).slice(0, 8)) + (into ? " <span class=\"rk\">kept</span>" : "");
        return live
          ? `<span class="echip" data-id="${esc(t.id)}" title="focus ${esc(t.name || t.id)}">${label}</span>`
          : `<span class="echip off" title="folded away by this merge - no node to focus">${label}</span>`;
      };
      h += `<div class="osec">targets</div><div class="echips">${ts.map(chip).join("")}</div>`;
    }
  }
  if (o.entities && o.entities.length)
    h += `<div class="osec">entities</div><div class="echips">`
      + o.entities.map(e => `<span class="echip" data-id="${esc(e.id)}" title="focus ${esc(e.name)}">${esc(e.name)}</span>`).join("") + `</div>`;
  if (o.relations && o.relations.length)
    h += `<div class="osec">relations</div>`
      + o.relations.map(r => `<div class="ocrel">${esc(r.from)} <span class="rk">${esc(r.type)}</span> ${esc(r.to)}</div>`).join("");
  h += `<div class="ocid">obs ${esc(String(o.id).slice(0, 16))}</div></div>`;
  return h;
}
function hideObsCard() { obscardEl.className = ""; obscardEl.innerHTML = ""; }
function showObsCard(o) {
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  obscardEl.innerHTML = obsCardHtml(o);
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
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  container.innerHTML = list.length ? list.map(obsRowHtml).join("") : '<div class="empty">no observations</div>';
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

async function refreshLog() {
  if (!panelOn("log")) return;
  const ws = wsInput.value.trim();
  try {
    const r = await fetch("/api/observations" + (ws ? "?workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    if (r.ok) { const l = await r.json(); if (Array.isArray(l)) obsLog = l; }
  } catch (e) { /* auxiliary - keep the last render */ }
  renderLog();
}

// The node inspector's "log" column: this node's supporting observations (the evidence behind its
// belief), always visible beside the edge columns. Cached per node so it survives the inspector's
// poll-driven re-render, and it stays live (re-fetched, re-rendered only when the set changes).
let nodeLogCache = { id: null, list: [] };
async function fillNodeLog(node, colEl, secEl) {
  const sameNode = nodeLogCache.id === node.id;
  if (!sameNode) nodeLogCache = { id: node.id, list: [] };
  const setCount = n => { if (secEl) secEl.textContent = "log (" + n + ")"; };
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
function logRow(html) {
  const row = document.createElement("div");
  row.className = "row";
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  row.innerHTML = `<span class="t">${new Date().toLocaleTimeString()}</span>${html}`;
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
    logRow(`<b>observe</b> +${(ev.entities||[]).length} ent, +${ev.relations||0} rel <span class="t">ws ${esc(ev.workspace)}</span>`);
    await poll();                       // wait for the new nodes to enter the graph, then pulse
    ids = ev.entities || [];
  } else if (ev.kind === "search") {
    logRow(`<b>search</b> "${esc(ev.query)}" -> ${ev.hits} hits <span class="t">${esc(ev.mode)}</span>`);
    if (switched) await poll();          // if the workspace switched, load that graph (so hits are visible)
    ids = ev.nodes || [];
  } else if (ev.kind === "get_entity") {
    logRow(`<b>get_entity</b> ${esc(ev.name || ev.id.slice(0,8))} <span class="t">${ev.found ? "found" : "unknown"}</span>`);
    ids = ev.found ? [ev.id] : [];
  } else if (ev.kind === "sync") {
    // Federation hit: who touched this store, which direction, how much - the live remote feed.
    logRow(`<b>sync</b> ${esc(ev.direction)} ${esc(ev.workspace)} &lt;-&gt; ${esc(String(ev.peer).slice(0, 18))} (${ev.count})`);
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
    logRow(`<b>traverse</b> ${esc(sn ? sn.name : ev.start.slice(0,8))} -> ${(ev.reached||[]).length}`);
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
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  tip.innerHTML = `<b>peer ${esc(id.slice(0, 6))}</b><br>`
    + `<span class="k">node</span> ${esc(id.slice(0, 16))}<br>`
    + `<span class="k">last</span> ${esc(m.action || "-")}${ago !== null ? " " + ago + "s ago" : ""} `
    + `&nbsp; <span class="k">hits</span> ${m.count | 0}`;
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
  try { await fetch("/api/resolve" + q, { cache: "no-store" }); } catch (e) { /* transport hiccup - poll re-syncs */ }
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
  const row = (v, t, obs, cur) =>
    `<div class="crow${cur ? " cur" : ""}"><span class="cv">${esc(v)}</span>`
    + (cur
        ? `<span class="curtag${committed ? " confirmed" : ""}">${committed ? "confirmed" : "current"}</span>`
        : `<span class="ctier">${esc(String(t))}</span>`)
    + (obs && !committed
      ? `<button class="confirm" data-obs="${esc(obs)}" title="${cur ? "lock in the current value (mark it human-confirmed so recency cannot flip it)" : "make this the confirmed value instead (mark it human-confirmed)"}">${cur ? "keep" : "use this"}</button>`
      : "")
    + `</div>`;
  const head = committed
    ? (contested
        ? "human-confirmed - competing values still tie (both confirmed)"
        : "confirmed by a human - this belief is settled")
    : (contested
        ? "these values tie on trust - confirm which is correct"
        : "resolved by trust - other asserted values shown for reference");
  return `<div class="chead">${head}</div>`
    + row(current, tier, curObs, true)
    + competitors.map(c => row(c.value, c.trust_tier, c.observation, false)).join("");
}

// The inspector "why (evidence & decision)" body from /api/explain: per-field belief resolution
// (each single-valued field's ranked candidates - winner / alias / competitor) plus the supporting
// observation log. An explanation OF the projection - the winner IS what the graph shows.
function candRowHtml(c) {
  return `<div class="cand ${esc(c.role)}"><span class="cv">${esc(c.value)}</span>`
    + `<span class="crole">${esc(c.role)}</span>`
    + `<span class="ctier">${esc(String(c.trust_tier))}</span>`
    + `<span class="cobs" title="asserting observation ${esc(c.observation)}">${esc(String(c.observation).slice(0, 8))}</span></div>`;
}
function fieldHtml(f) {
  return `<div class="wfield${f.contested ? " hot" : ""}">`
    + `<div class="wfhead">${esc(f.field)}${f.contested ? ' <span class="wtag">contested</span>' : ""}</div>`
    + (f.candidates || []).map(candRowHtml).join("")
    + `</div>`;
}
function renderExplain(ex) {
  // The supporting observations (the evidence) live in the inspector's "log" column now, so this
  // disclosure is just the per-field decision: which value won each single-valued field, and why.
  return (ex.fields || []).map(fieldHtml).join("");
}
// Cache the fetched explanation + open state per node, so the disclosure survives the inspector's
// poll-driven re-render (renderDetail rebuilds innerHTML while a node stays focused).
let whyCache = null;   // { id, open, ex }
function fillWhy(whyBody, ex) {
  if (ex && ex.fields) {
    // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
    whyBody.innerHTML = renderExplain(ex);
  } else {
    whyBody.textContent = "no explanation available";
  }
}

// --- Detail inspector: shows the clicked node's connections (neighbors + relations), and click a neighbor to explore ---
function renderDetail(node) {
  if (!node) { detailEl.className = ""; detailEl.innerHTML = ""; return; }
  const outs = edges.filter(e => e.a === node && !typeOff.has(e.b.type));
  const ins = edges.filter(e => e.b === node && !typeOff.has(e.a.type));
  const rowHtml = (rel, other, dir, desc) =>
    `<div class="row" data-id="${esc(other.id)}" title="${desc ? esc(desc) : "focus " + esc(other.name)}">`
    + `<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>`
    + `<span class="rel">${dir} ${esc(rel)}</span>`
    + `<span class="nm">${esc(other.name)}</span></div>`;
  const list = (arr, dir) => arr.length
    ? arr.map(e => rowHtml(e.type, dir === "->" ? e.b : e.a, dir, e.description)).join("")
    : `<div class="empty">none</div>`;
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  detailEl.innerHTML =
    `<button class="close" title="close" aria-label="close">`
      + `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">`
      + `<path d="M2 2 L10 10 M10 2 L2 10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>`
      + `</svg></button>`
    + `<h2>${esc(node.name)}</h2>`
    + `<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span> `
    + `${esc(node.type)} / deg ${node.degree || 0} / src ${node.sources} / ${esc(String(node.trust_tier))}</div>`
    + (node.aliases && node.aliases.length ? `<div class="meta">merged: ${esc(node.aliases.join(", "))}</div>` : "")
    + (node.origins && node.origins.length ? `<div class="meta">from: ${esc(node.origins.join(", "))}</div>` : "")
    + (node.description ? `<div class="desc">${esc(node.description)}</div>` : "")
    + (node.competitors && node.competitors.length
        ? `<div class="contested${node.contested ? " hot" : ""}">`
          + contestedRows(node.type, node.trust_tier, node.kind_source, node.competitors, node.contested)
          + `</div>`
        : "")
    // Why this value won: a lazy-loaded disclosure (the per-field decision comes from /api/explain
    // only when opened - the graph poll stays light). The evidence itself is the "log" column below.
    + `<div class="why"><button class="whytoggle" type="button">belief decision (why this value)</button><div class="whybody"></div></div>`
    // Bottom columns: the node's edges (outgoing / incoming) and its observation log (the evidence
    // behind its belief), side by side.
    + `<div class="rels">`
    +   `<div class="relcol"><div class="sec">outgoing (${outs.length})</div>${list(outs, "->")}</div>`
    +   `<div class="relcol"><div class="sec">incoming (${ins.length})</div>${list(ins, "<-")}</div>`
    +   `<div class="relcol"><div class="sec">log</div><div class="logcol"></div></div>`
    + `</div>`;
  detailEl.className = "on";
  detailEl.querySelector(".close").onclick = () => { focus = null; renderDetail(null); };
  detailEl.querySelectorAll(".row").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) { focus = n; renderDetail(n); focusView(n); }
    };
  });
  detailEl.querySelectorAll(".confirm").forEach(b => {
    b.onclick = (ev) => { ev.stopPropagation(); resolveBelief(b.dataset.obs); };
  });
  // "Belief decision" disclosure: fetch /api/explain on first open, render the per-field decision.
  // The result and open state are cached per node so it survives the inspector's poll-driven
  // re-render (and reopening a fetched node is instant, no re-fetch).
  const why = detailEl.querySelector(".why"), whyBody = why.querySelector(".whybody");
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
  // The "log" column: this node's supporting observations (the evidence behind its belief), always
  // visible beside the edge columns. Cached + an expanded-row set, so it survives the poll re-render.
  const logcol = detailEl.querySelector(".logcol");
  fillNodeLog(node, logcol, logcol.previousElementSibling);
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
  // Hulls (hyperedge overlay) - reuse the graph's exact rounded-hull geometry (hullGeom +
  // roundedHullPath, which balloons past the node glyphs and wraps corners with arcs), just scaled
  // into the minimap via a world->minimap transform, so the smooth corner-wrapping matches the main
  // canvas instead of a coarse polygon. Only while the overlay is on.
  if (hyperMode && hyperedges.length) {
    const nodeMap = new Map(src.map(n => [n.id, n]));
    mctx.save();
    mctx.transform(k, 0, 0, k, ox, oy);   // compose world->minimap on top of the DPR transform
    mctx.globalAlpha = 0.14;
    for (const he of hyperedges) {
      if ((he.size || 0) < 3) continue;
      const ms = (he.members || []).map(id => nodeMap.get(id)).filter(Boolean);
      if (ms.length < 3) continue;
      const g = hullGeom(ms);
      if (!g) continue;
      roundedHullPath(mctx, g);
      mctx.fillStyle = hyperColor(he.id);
      mctx.fill();
    }
    mctx.globalAlpha = 1;
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
  // Viewport rectangle: the screen corners in world coords, mapped in and clamped to the frame.
  // A corner that is clamped onto the frame corner takes the PANEL's radius (the rect then traces
  // the rounded outline instead of slicing through it); free corners keep a small radius.
  const x1 = Math.max(0.5, Math.min(MINI_W - 0.5, (-cam.x / cam.s) * k + ox));
  const y1 = Math.max(0.5, Math.min(MINI_H - 0.5, (-cam.y / cam.s) * k + oy));
  const x2 = Math.max(0.5, Math.min(MINI_W - 0.5, ((innerWidth - cam.x) / cam.s) * k + ox));
  const y2 = Math.max(0.5, Math.min(MINI_H - 0.5, ((innerHeight - cam.y) / cam.s) * k + oy));
  const w = Math.max(2, x2 - x1), h = Math.max(2, y2 - y1);
  mctx.strokeStyle = GOLD; mctx.lineWidth = 1;
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
  camT.x = innerWidth/2 - wx * camT.s;
  camT.y = innerHeight/2 - wy * camT.s;
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
  frameBox(box, 0, innerWidth, TOP_INSET, innerHeight - BOTTOM_INSET, pad, 2.5);
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
  const hull = convexHull(ms.map(m => ({ x: m.x, y: m.y })));
  if (hull.length < 3) return null;
  return { hull, r, cx, cy };
}
// Trace the outward-offset rounded hull as a SINGLE closed path: each edge is pushed out by r (past the
// node glyphs) and consecutive edges are joined by a corner arc sampled into short segments. Filling this
// one path gives the same rounded blob a thick round-join stroke would - but with no stroke, so there is
// no fill/stroke overlap (no seam) and no offscreen compositing is needed; the caller just fills it once
// at the hull's opacity, directly on the canvas, and overlapping hulls blend naturally.
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
  // edge. Each hull is a SINGLE outward-offset rounded path filled once, directly on the canvas, at the
  // hull's opacity - no stroke (so no fill/stroke seam) and no offscreen compositing (cheap: N path fills,
  // not N large drawImage copies). Overlapping hulls blend naturally (C1: overlap = connective tissue).
  // While a node is active, its own hulls are emphasized and the rest fade. Labels are collected per hull.
  let hullLabels = [];
  if (hyperMode && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    const items = [];
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
      if (ms.length < 3) continue;
      const g = hullGeom(ms);
      if (!g) continue;
      const hot = hlAnchor && h.members.includes(hlAnchor.id);   // is the active (click/hover) node a member of this context
      // Legend-chip hover: does this context contain any highlighted member? Hulls without one fade,
      // so the highlighted type/relation stands out against the hull layer too (same inspection language).
      const lgHit = typeHl ? ms.some(m => m.type === typeHl)
        : edgeTypeHl ? ms.some(m => lgEndpoints.has(m.id))
        : false;
      const rep = ms.reduce((a, m) => (m.degree||0) > (a.degree||0) ? m : a, ms[0]);   // hub = highest-degree member
      items.push({ g, col: hyperColor(h.id), hot, lgHit, rep, size: ms.length });
    }
    const lgActive = !!(typeHl || edgeTypeHl);
    const paint = (it) => {
      roundedHullPath(ctx, it.g);
      ctx.globalAlpha = lgActive ? (it.lgHit ? HULL_LAYER_ALPHA : HULL_LAYER_DIM)
        : hlAnchor ? (it.hot ? HULL_ACTIVE_ALPHA : HULL_LAYER_DIM) : HULL_LAYER_ALPHA;
      ctx.fillStyle = it.col; ctx.fill();
      hullLabels.push({ cx: it.g.cx, cy: it.g.cy, text: it.rep.name + " (" + it.size + ")", col: it.col, hot: it.hot, lgHit: it.lgHit, size: it.size });
    };
    for (const it of items) if (!it.hot) paint(it);   // active hulls painted last so they sit on top
    for (const it of items) if (it.hot) paint(it);
    ctx.globalAlpha = 1;
    // Group labels: their own layer, right after the hulls (below edges/nodes). Greedy placement -
    // active first, then larger contexts - skips any label whose box overlaps one already placed, so
    // the map reads as spaced region names instead of a wall of overlapping text. A soft pill keeps
    // each name legible over the busy hull fills. Screen space, so restore the world transform after.
    if (showLabels && hullLabels.length) {
      ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      if ("letterSpacing" in ctx) ctx.letterSpacing = "0.3px";
      // Region-name style: font scales with context size (bigger domain -> bigger name), sorted so the
      // largest / active contexts win placement; anything overlapping an already-placed one is skipped.
      const cand = hullLabels
        .map(l => ({ l, px: l.cx*cam.s + cam.x, py: l.cy*cam.s + cam.y, fs: Math.min(30, Math.round(12 + Math.sqrt(l.size) * 2.2)) }))
        .filter(o => o.px > -80 && o.px < innerWidth + 80 && o.py > -30 && o.py < innerHeight + 30)
        .sort((a, b) => (b.l.hot - a.l.hot) || (b.l.size - a.l.size));
      const placed = [];
      for (const o of cand) {
        const l = o.l;
        ctx.font = (l.hot ? "600 " : "500 ") + o.fs + "px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
        const w = ctx.measureText(l.text).width + o.fs*0.5, h = o.fs*1.3, x = o.px - w/2, y = o.py - h/2;
        if (placed.some(p => x < p.x + p.w && x + w > p.x && y < p.y + p.h && y + h > p.y)) continue;
        placed.push({ x, y, w, h });
        // Blend into the map without a chip: a crisp background-color stroke (a cutout that matches the
        // canvas) keeps the group-color name sharp over the hull fills; lower idle opacity lets it recede.
        const a = lgActive ? (l.lgHit ? 0.75 : HULL_LABEL_HOVER_FADE)
          : hlAnchor ? (l.hot ? 1 : HULL_LABEL_HOVER_FADE) : 0.6;
        ctx.globalAlpha = a;
        ctx.lineWidth = Math.max(3, o.fs * 0.16); ctx.strokeStyle = SURFACE; ctx.strokeText(l.text, o.px, o.py);
        ctx.fillStyle = l.col; ctx.fillText(l.text, o.px, o.py);
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
    ctx.strokeStyle = e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER);
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // constant thickness on screen
    if (hot) {
      // Active edges show flow direction: marching dashes travel source -> target. Screen-constant
      // dash/speed (divide by cam.s) so it reads the same at any zoom. Negative offset -> forward flow.
      ctx.setLineDash([7/cam.s, 6/cam.s]);
      ctx.lineDashOffset = -(flowPhase * 0.6) / cam.s;
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
    ctx.globalAlpha = on ? 1 : 0.12;
    const r = nodeRadius(n);
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    // Default stroke (background-color halo): sharpens the node boundary and separates it from edges/neighbors (visibility).
    // Being the background color, it stays a cutout that matches the background even when the theme changes.
    ctx.lineWidth = nodeStrokeW(n); ctx.strokeStyle = SURFACE; ctx.stroke();
    if (n === anchor) { ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = INK; ctx.stroke(); }
    // Conversation footprint: nodes this session touched are marked with a persistent thin teal ring (footprint toggle).
    if (showFootprint && footprint.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 3.5, 0, 7);
      ctx.lineWidth = 1.5/cam.s; ctx.strokeStyle = TEAL; ctx.stroke();
    }
    // Group mode: bridge nodes (connected to another group) are marked with a faint ring - cross-group transit points.
    if (clusterMode && bridgeSet.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 2, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = INK; ctx.stroke();
    }
    // Contested belief (resolution.md R6): distinct kind values tie at the top trust tier, so the
    // current winner stands on recency alone - a dashed amber ring invites mediation (click the node,
    // the inspector shows the competing values with a confirm action).
    if (n.contested) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 5, 0, 7);
      ctx.setLineDash([4/cam.s, 3/cam.s]);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = GOLD; ctx.stroke();
      ctx.setLineDash([]);
    }
  }
  // Event pulses (nodes the agent touched) - an expanding, fading ring. rAF always runs, so it keeps
  // animating even after cooling.
  for (const [id, ttl] of pulses) {
    const n = nodeById(id);
    if (!n || ttl <= 0 || typeOff.has(n.type)) { pulses.delete(id); continue; }
    if (showPulses) {
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
  // the target's incident edges accented (they rewire), a ring on each target and a canonical ring on
  // `into`. For an open proposal both nodes are still present (a before-preview); for a merged one the
  // targets are already folded away, so only the canonical is highlighted (the result).
  if (proposalSel && proposalSel.kind === "entity_merge") {
    const into = nodeById(proposalSel.into);
    const tgts = (proposalSel.targets || []).map(nodeById).filter(n => n && (!into || n.id !== into.id));
    const tgtIds = new Set(tgts.map(n => n.id));
    const ACC = GOLD, CANON = TEAL;
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
      ctx.beginPath(); ctx.arc(t.x, t.y, nodeRadius(t) + 4, 0, 7); ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = ACC; ctx.stroke();
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
    if (into) {
      ctx.beginPath(); ctx.arc(into.x, into.y, nodeRadius(into) + 6, 0, 7); ctx.lineWidth = 3/cam.s; ctx.strokeStyle = CANON; ctx.stroke();
    }
    ctx.globalAlpha = 1;
  }

  // Proposal preview - T-Box change (belief-diff hint via affected_types): accent every edge whose
  // relation kind is being (re)defined and ring every node whose entity type is. A tbox_change edits
  // type definitions, which are edge kinds / node types (not first-class nodes), so the change shows as
  // a highlight over the members of those types rather than as a fold arrow. Kinds hidden via the legend
  // stay hidden (respect typeOff/edgeTypeOff) so the preview never contradicts the visible graph.
  if (proposalSel && proposalSel.affected_types && proposalSel.affected_types.length) {
    const { rel, ent } = affectedTypeSets(proposalSel);
    const ACC = GOLD;
    ctx.strokeStyle = ACC; ctx.globalAlpha = 0.95; ctx.lineWidth = 2.5/cam.s;
    if (rel.size) for (const e of edges) {
      if (typeOff.has(e.a.type) || typeOff.has(e.b.type) || edgeTypeOff.has(e.type)) continue;
      if (rel.has(e.type)) { ctx.beginPath(); ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke(); }
    }
    if (ent.size) for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      if (ent.has(n.type)) { ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 4, 0, 7); ctx.stroke(); }
    }
    ctx.globalAlpha = 1;
  }

  // Labels (nodes + hulls) - turned on/off by the labels toggle. In screen coordinates (DPR), so constant size regardless of zoom.
  if (showLabels) {
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    ctx.font = "12px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
    ctx.textBaseline = "middle";
    // Label thinning: everything when small (<=40) or zoomed in enough (cam.s>1.4); on a large graph,
    // only hubs (high degree >= cut) + hover/focus/active. Removes the hairball's wall of labels.
    const cut = (nodes.length <= 40 || cam.s > 1.4) ? 0 : Math.max(4, Math.round(nodes.length / 25));
    for (const n of nodes) {
      if (typeOff.has(n.type)) continue;
      // Legend hover: label exactly the highlighted set (bypasses the degree cut - hover is transient).
      const lg = typeHl ? n.type === typeHl : edgeTypeHl ? lgEndpoints.has(n.id) : null;
      if (lg === false) continue;
      const on = lg === true ? true : (act ? act.ns.has(n.id) : true);
      const show = on && (lg === true || n === hover || n === focus || (act && act.ns.has(n.id)) || (n.degree || 0) >= cut);
      if (!show) continue;
      const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = nodeRadius(n)*cam.s;
      ctx.globalAlpha = on ? 1 : 0.25;
      ctx.lineWidth = 3; ctx.strokeStyle = SURFACE; ctx.strokeText(n.name, px + r + 5, py);
      ctx.fillStyle = (n === focus || n === hover) ? INK : INK2; ctx.fillText(n.name, px + r + 5, py);
    }
    ctx.globalAlpha = 1;
    // (Group / hull labels are drawn in their own pass right after the hulls - see drawHullLabels below.)
    // Active-edge relation labels: only the hovered/focused node's edges, shortest first, capped so a
    // hub does not flood the canvas; the overflow is summarized as "+K more" under the active node.
    if (edgeLabels && edgeLabels.length) {
      edgeLabels.sort((p, q) => p.len - q.len);
      const shown = Math.min(edgeLabels.length, EDGE_LABEL_MAX);
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.font = "10.5px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
      const pill = (px, py, w) => {
        ctx.beginPath();
        if (ctx.roundRect) ctx.roundRect(px - w/2 - 5, py - 8, w + 10, 16, 4);
        else ctx.rect(px - w/2 - 5, py - 8, w + 10, 16);
        ctx.fill();
      };
      for (let k = 0; k < shown; k++) {
        const L = edgeLabels[k];
        const px = L.mx*cam.s + cam.x, py = L.my*cam.s + cam.y, w = ctx.measureText(L.text).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = L.col; ctx.fillText(L.text, px, py);
      }
      const omitted = edgeLabels.length - shown, an = focus || hover;
      if (omitted > 0 && an) {
        ctx.font = "10px 'IBM Plex Mono',ui-monospace,'SF Mono',Menlo,monospace";
        const t = "+" + omitted + " more", px = an.x*cam.s + cam.x, py = an.y*cam.s + cam.y + nodeRadius(an)*cam.s + 15, w = ctx.measureText(t).width;
        ctx.globalAlpha = 0.9; ctx.fillStyle = SURFACE; pill(px, py, w);
        ctx.globalAlpha = 1; ctx.fillStyle = INK2; ctx.fillText(t, px, py);
      }
      ctx.textAlign = "left"; ctx.textBaseline = "alphabetic"; ctx.globalAlpha = 1;
    }
  }
  // Peer cursor-dots (server mode) - drawn last so they float above the graph and labels.
  stepPeers(); drawPeers();
  drawMinimap();
  if (animating(act)) requestFrame();
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
  // esc() every string field: node name/type come from untrusted observe calls and land in innerHTML
  // here (Principle 18). Numeric fields are coerced, not escaped.
  // eslint-disable-next-line no-unsanitized/property -- value is built from esc()-escaped strings
  tip.innerHTML = `<b>${esc(n.name)}</b><br>`
    + `<span class="k">type</span> ${esc(n.type)} &nbsp; <span class="k">degree</span> ${n.degree || 0}<br>`
    + `<span class="k">sources</span> ${n.sources || 0} &nbsp; <span class="k">trust</span> ${esc(n.trust_tier)}`;
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
    if (proposalSel) { proposalSel = null; renderProposals(); }
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

const searchWrap = searchEl.closest(".searchwrap");
searchEl.addEventListener("input", () => {
  searchTerm = searchEl.value.trim().toLowerCase();
  // Keep the "/" badge clear of the text once the field carries a term (CSS .searchwrap.filled).
  // Optional: the badge is decoration, so losing its wrapper must never break search itself.
  searchWrap?.classList.toggle("filled", searchEl.value !== "");
});
searchEl.addEventListener("keydown", ev => {
  if (ev.key === "Enter" && searchTerm) {
    const hits = nodes.filter(n => n.name.toLowerCase().includes(searchTerm));
    if (hits.length) { fitView(hits, 140); userMoved = true; }
  } else if (ev.key === "Escape") {
    // Hand the keyboard back to the canvas, so "/" -> type -> Escape is a closed loop.
    // stopPropagation: the window handler reads Escape as "dismiss the obs card / view popover",
    // which is not what leaving the search field should do.
    ev.stopPropagation();
    searchEl.blur();
  }
});
document.getElementById("reload").onclick = () => { loadWorkspaces(); poll(); };
document.getElementById("zin").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1.2);
document.getElementById("zout").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1/1.2);
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
// Per-dock drawer handles (screen-edge tabs) replace the old all-or-nothing header toggle: each
// dock collapses independently, the header stays a pure title/search bar, and the choice persists
// across reloads. The docks are side panels (not canvas layers) - toggling never reheats the sim.
// Chevron points where clicking will move the edge: outward "<" collapses, inward ">" expands.
const dockLTab = document.getElementById("dockLTab"), dockRTab = document.getElementById("dockRTab");
function applyDock(isLeft, on) {
  (isLeft ? dockLEl : dockREl).classList.toggle("on", on);
  const tab = isLeft ? dockLTab : dockRTab;
  tab.classList.toggle("open", on); // the handle slides with its dock (CSS .docktab.open)
  tab.textContent = (isLeft ? on : !on) ? "<" : ">";
  try { localStorage.setItem(isLeft ? "supra.dockL" : "supra.dockR", on ? "1" : "0"); } catch (_) { /* private mode */ }
}
function toggleDock(isLeft) { applyDock(isLeft, !(isLeft ? dockLEl : dockREl).classList.contains("on")); }
dockLTab.onclick = () => toggleDock(true);
dockRTab.onclick = () => toggleDock(false);
// View options popover (HUD "view" button): preferences stay off the data rails; a click
// elsewhere or Escape dismisses it.
const viewBtn = document.getElementById("viewBtn"), viewPop = document.getElementById("viewpop");
function setViewPop(on) { viewPop.classList.toggle("on", on); viewBtn.classList.toggle("on", on); }
viewBtn.onclick = ev => { ev.stopPropagation(); setViewPop(!viewPop.classList.contains("on")); };
document.addEventListener("click", ev => { if (viewPop.classList.contains("on") && !viewPop.contains(ev.target)) setViewPop(false); });
// [ / ] toggle the docks, "/" jumps to search - but never while typing in a field.
window.addEventListener("keydown", e => {
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
  else if (e.key === "Escape") { if (obscardEl.classList.contains("on")) hideObsCard(); else setViewPop(false); }
});
// Restore the persisted state (default: both open).
try {
  applyDock(true, localStorage.getItem("supra.dockL") !== "0");
  applyDock(false, localStorage.getItem("supra.dockR") !== "0");
} catch (_) { /* private mode - defaults stand */ }
// Fetch the glossary lazily when its section is expanded (and keep it fresh via poll while open).
// Dock tabs: clicking a tab shows only that panel (one at a time, fixed-height body - no upward growth),
// and refreshes the newly active data panel.
document.querySelectorAll(".dock .tabs").forEach(tabs => {
  tabs.addEventListener("click", ev => {
    const btn = ev.target.closest(".tab");
    if (!btn) return;
    const dock = tabs.closest(".dock");
    tabs.querySelectorAll(".tab").forEach(t => t.classList.toggle("on", t === btn));
    dock.querySelectorAll(".tabpanel").forEach(p => p.classList.toggle("on", p.dataset.panel === btn.dataset.tab));
    if (btn.dataset.tab === "glossary") refreshGlossary();
    else if (btn.dataset.tab === "peers") refreshPeers();
    else if (btn.dataset.tab === "review") refreshCuration();
    else if (btn.dataset.tab === "proposals") refreshProposals();
    else if (btn.dataset.tab === "log") refreshLog();
  });
});
// Pure render toggles (no layout/data change - rAF reflects them every frame, so wake/poll is unnecessary).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; e.currentTarget.classList.toggle("on", showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; e.currentTarget.classList.toggle("on", showEdges); };
document.getElementById("arrowsBtn").onclick = e => { showArrows = !showArrows; e.currentTarget.classList.toggle("on", showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; e.currentTarget.classList.toggle("on", showFootprint); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; e.currentTarget.classList.toggle("on", showPulses); };
document.getElementById("histBtn").onclick = e => { showSuperseded = !showSuperseded; e.currentTarget.classList.toggle("on", showSuperseded); };
document.getElementById("settingsBtn").onclick = openSettings;
document.getElementById("miniBtn").onclick = e => { showMini = !showMini; e.currentTarget.classList.toggle("on", showMini); };

// Restore the workspace selection from the URL (deep link / reload) before the first poll.
{
  const qws = new URLSearchParams(location.search).get("workspace");
  if (qws !== null) wsInput.value = qws;
}
resize(); loadWorkspaces(); poll(); connectEvents(); detectServerMode();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); setInterval(refreshPeerRoster, 5000); draw();
