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

