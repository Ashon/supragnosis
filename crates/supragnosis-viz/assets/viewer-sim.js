// The map's layout (docs/design-system.md Sections 26.3, 26.4 and 27.4-27.5): the force simulation,
// on typed arrays, with nothing of the page in it.
//
// One file, two places. Loaded as a classic script before viewer.js, it gives the page its layout
// constants, the radius of a node and the engine itself. Started as a worker (U35), it runs the
// engine on a thread of its own and talks to the page in messages, so a step never holds a frame
// the page is drawing. The policy's `script-src 'self'` admits a worker from a served file and not
// from a `blob:` address, so the file is the worker. When no worker can start, the page steps the
// same engine from draw, as it always did.

// --- the forces' constants ----------------------------------------------------------------------
const CLUSTER_PULL = 0.03;       // pull toward the group target point (stronger than the center attraction)
const HYPER_PULL = 0.03;         // base hyperedge centroid cohesion (scaled by hull size - see hullSizeNorm)
// Cohesion scales with member count: larger hulls pull their nodes tighter (small ones stay loose).
const HULL_SIZE_REF = 8;         // members at/above which a hull gets full cohesion weight
const HULL_COH_MIN = 0.4, HULL_COH_MAX = 1.15;    // cohesion factor range (x HYPER_PULL)
const HULL_PAD = 24;             // target gap between hulls (world px). Kept small to avoid over-separation at high density
const HULL_SEP = 0.008;          // separation force between hulls (gentle - scaled by cooling alpha)
const HULL_R_CAP = 160;          // upper bound on the hull radius used for separation - so a huge grab-bag cannot push the whole layout
const HULL_MAX_PUSH = 4;         // per-frame separation displacement cap per hull - prevents divergence accumulating across many pairs
// Workspaces cohere while the boundaries draw them: a node outside its workspace's natural radius -
// WS_RADIUS x sqrt(its node count), about the disk its nodes fill at their usual spacing - is pulled
// toward the workspace's centroid, and two workspaces whose disks overlap are pushed apart.
const WS_RADIUS = 18;            // world px per sqrt(node): a workspace of 100 nodes rests within ~180px of its centre
const WS_PULL = 0.012;           // pull per px beyond that radius (x cooling alpha) - far stronger than the global centre pull on a large graph
const WS_PAD = 70;               // gap kept between two workspaces' disks
const WS_SEP = 0.01;             // push apart per px of overlap (x cooling alpha)
const WS_MAX_PUSH = 6;           // per-frame push cap per workspace, as HULL_MAX_PUSH is per hull
const WS_CROSS_REPULSE = 0.25;   // node-to-node repulsion across two workspaces: the workspaces already push apart as wholes
// alpha decays multiplicatively (x0.9772 a step), so ALPHA_MIN buys time logarithmically: 0.02
// applies force for ~170 steps from a cold load, 0.008 for ~209. Lowered because the tail of the
// layout is where it is still worth nudging - the forces down there are tiny, and stopping while
// they are still meaningful is what made the settle read as a cut rather than a stop. The coasting
// afterwards is not governed by this at all; that is DAMPING, and it is measured (see motion).
const ALPHA_DECAY = 0.0228, ALPHA_MIN = 0.008;
// Layout-loader gating: while the layout is reheated at/above SETTLE_ENTER (initial load, data change,
// group toggle - the big rearrangements), the graph is hidden behind a loader; it is revealed once
// alpha cools to REVEAL_ALPHA. Small wakes (drag/focus at 0.3) stay below SETTLE_ENTER, so those never
// trigger the loader.
const SETTLE_ENTER = 0.5, REVEAL_ALPHA = 0.08;
const MOTION_MIN_PX = 0.05;   // screen px per step below which the picture is not changing
// The settle's last collision pass (Section 26.3): once the layout cools, the overlaps the cooling
// left are cleared for at most this many steps, and then no pair is visited until something warms the
// layout again. It used to run in every frame for good, which made every pan and hover pay for every
// pair of nodes.
const OVERLAP_STEPS = 90;
// How many cells span the repulsion's range while force applies (Section 26.4): the near field is a
// node's own cell and the eight around it, the far field the rest of the range, cell by cell.
const FAR_SPLIT = 6;
// Base force parameters. The larger the graph, the wider it should spread, so a step scales by node count (spread).
const REPULSE = 7000;
// Links fit the density around them. A link's rest length grows with the smaller degree of its two
// ends - a leaf sits close to its hub, two hubs keep room between them for their leaves - and with how
// crowded its ends are against the graph's average, so a crowded patch spreads and a sparse one
// gathers. Its strength falls with that degree, as d3's links do, so a hub is not held by every link
// at once, and the end with fewer links does more of the moving.
const LINK_BASE = 70;              // rest length of a leaf's link, before crowding
const LINK_PER_DEG = 14;           // added per sqrt(smaller degree)
const LINK_K = 0.09;               // a leaf link's strength; LINK_K / sqrt(smaller degree) otherwise
const DENSITY_R = 160;             // a node's crowd: the nodes within this many world px
const CROWD_MIN = 0.7, CROWD_MAX = 1.5;    // how far crowding may shorten or lengthen a link
const CENTER_BASE = 0.0015; // center-attraction base - weakened for large graphs (prevents central clumping)
const ANCHOR_K = 0.5;       // central-axis anchor: fraction of the centroid-offset corrected each step (rigid recenter, positions only) - pins the whole cluster to the world center so it cannot drift off, even when dormant
const RANGE_BASE = 240;     // repulsion range base - widened for large graphs (pushes out farther)
const COLLIDE_PAD = 16, DAMPING = 0.85;
const MIN_SEP = 12;        // repulsion denominator floor - prevents force blowup (flinging) when very close
const MAX_V = 30;          // per-step max speed base - raised for large graphs
const MAX_PUSH = 6;        // per-step per-node collision displacement cap - prevents hub blowup
// Node size is proportional to neighbor count (degree) (sqrt, to flatten a wide range). The enlarged
// radius feeds directly into collision separation (minD), so spacing widens with neighbor count too,
// and nodes with few neighbors stay small and dense.
const NODE_R_BASE = 4;         // radius at degree 0
const NODE_R_SCALE = 3.4;      // sqrt(degree) coefficient
const NODE_R_MAX = 28;         // radius upper bound (prevents hub runaway)
const REPULSE_HUB_MAX = 2.5;   // hub-hub repulsion weight cap (prevents divergence)
// Once the map shows, the layout takes at most one step this often, so the cooling looks as it did
// at sixty frames a second whatever the page's frames cost (Section 27.5).
const SHOWN_STEP_MS = 1000 / 60;

function radiusOfDegree(degree) { return Math.min(NODE_R_MAX, NODE_R_BASE + Math.sqrt(degree || 0) * NODE_R_SCALE); }
function hullSizeNorm(size) { return Math.max(0, Math.min(1, (size - 2) / (HULL_SIZE_REF - 2))); }

// --- the multilevel start (Section 27.6, U36) ----------------------------------------------------
// A fresh layout of many nodes starts from its communities: they are found by label propagation, the
// graph of communities is laid out first, each member starts on a spiral within its community's
// disc, and the full layout continues from a cooler start, so it reveals in fewer steps. The forces
// are the full layout's own; only where they begin changes.
const MULTI_MIN = 200;        // nodes from which a fresh layout starts from its communities
const MULTI_ALPHA = 0.3;      // how hot the full layout starts once its communities are placed
const LPA_ROUNDS = 12;        // label propagation's rounds, at most
const COARSE_STEPS = 160;     // the graph of communities' own settle, at most
const COARSE_WORK = 2e7;      // pairs it meets in all: fewer steps for many communities
const COMMUNITY_MIN = 6;      // a smaller community joins the neighbour it shares the most relations with
const COARSE_SPACING = 22;    // world px per sqrt(member): a community's disc

// Communities by label propagation over the relations (Raghavan and others, 2007): every node starts
// as its own, and in a fixed order takes the label most of its neighbours carry, the lower label on a
// tie, until a round changes nothing - so a graph gives the same communities every time. A node with
// no relation stays alone. The labels come back numbered from 0 in order of first appearance.
function communitiesOf(n, ea, eb) {
  const start = new Int32Array(n + 1);
  for (let e = 0; e < ea.length; e++) { start[ea[e] + 1]++; start[eb[e] + 1]++; }
  for (let i = 0; i < n; i++) start[i + 1] += start[i];
  const adj = new Int32Array(start[n]), cur = start.slice(0, n);
  for (let e = 0; e < ea.length; e++) { adj[cur[ea[e]]++] = eb[e]; adj[cur[eb[e]]++] = ea[e]; }
  const label = new Int32Array(n), count = new Int32Array(n), seen = new Int32Array(n);
  for (let i = 0; i < n; i++) label[i] = i;
  for (let round = 0; round < LPA_ROUNDS; round++) {
    let changed = 0;
    for (let i = 0; i < n; i++) {
      const a = start[i], b = start[i + 1];
      if (a === b) continue;
      let t = 0;
      for (let k = a; k < b; k++) { const l = label[adj[k]]; if (count[l]++ === 0) seen[t++] = l; }
      let best = -1, most = 0;
      for (let k = 0; k < t; k++) { const l = seen[k], c = count[l]; if (c > most || (c === most && l < best)) { best = l; most = c; } }
      for (let k = 0; k < t; k++) count[seen[k]] = 0;
      if (best !== label[i]) { label[i] = best; changed++; }
    }
    if (!changed) break;
  }
  // On a sparse graph propagation leaves many communities of two or three. Each smaller than
  // COMMUNITY_MIN joins, in label order, the community its members share the most relations with -
  // the lower label on a tie - so the graph of communities is the communities a reader would see.
  const size = new Int32Array(n), into = new Int32Array(n);
  for (let i = 0; i < n; i++) { size[label[i]]++; into[i] = i; }
  const root = l => { while (into[l] !== l) l = into[l] = into[into[l]]; return l; };
  const members = new Map();
  for (let i = 0; i < n; i++) if (size[label[i]] < COMMUNITY_MIN) { const l = label[i]; if (!members.has(l)) members.set(l, []); members.get(l).push(i); }
  for (const l of [...members.keys()].sort((a, b) => a - b)) {
    const own = root(l);
    if (size[own] >= COMMUNITY_MIN) continue;
    let t = 0;
    for (const i of members.get(l)) for (let k = start[i]; k < start[i + 1]; k++) {
      const o = root(label[adj[k]]);
      if (o !== own && count[o]++ === 0) seen[t++] = o;
    }
    let best = -1, most = 0;
    for (let k = 0; k < t; k++) { const o = seen[k], c = count[o]; if (c > most || (c === most && o < best)) { best = o; most = c; } }
    for (let k = 0; k < t; k++) count[seen[k]] = 0;
    if (best >= 0) { into[own] = best; size[best] += size[own]; }
  }
  const at = new Int32Array(n).fill(-1), of = new Int32Array(n);
  let K = 0;
  for (let i = 0; i < n; i++) { const l = root(label[i]); if (at[l] < 0) at[l] = K++; of[i] = at[l]; }
  return { of, count: K };
}

// The graph of communities laid out on its own: each a disc of COARSE_SPACING x sqrt(its members),
// pushed apart from every other and off any it overlaps, and pulled toward the communities it shares
// relations with, around (cx, cy). Few enough to meet every pair.
function coarseLayout(K, size, links, cx, cy) {
  const R = new Float64Array(K), x = new Float64Array(K), y = new Float64Array(K), vx = new Float64Array(K), vy = new Float64Array(K);
  for (let c = 0; c < K; c++) R[c] = COARSE_SPACING * Math.sqrt(size[c]);
  // The largest nearest the centre, on a spiral that leaves each its disc's room.
  const order = Array.from({ length: K }, (_, c) => c).sort((a, b) => size[b] - size[a] || a - b);
  let area = 0;
  order.forEach((c, k) => {
    area += R[c] * R[c];
    const a = k * 2.39996, r = 1.15 * Math.sqrt(area);
    x[c] = cx + r * Math.cos(a); y[c] = cy + r * Math.sin(a);
  });
  const steps = Math.max(30, Math.min(COARSE_STEPS, Math.floor(COARSE_WORK / Math.max(1, K * K / 2))));
  for (let s = 0, heat = 1; s < steps; s++, heat *= 1 - 4 / steps) {
    for (let i = 0; i < K; i++) for (let j = i + 1; j < K; j++) {
      let dx = x[j] - x[i], dy = y[j] - y[i];
      const d = Math.hypot(dx, dy) || 0.01, want = R[i] + R[j] + COLLIDE_PAD;
      dx /= d; dy /= d;
      let f = 0.02 * R[i] * R[j] / Math.max(d, want) * heat;
      if (d < want) f += (want - d) * 0.25;
      vx[i] -= f * dx; vy[i] -= f * dy; vx[j] += f * dx; vy[j] += f * dy;
    }
    for (const [i, j, w] of links) {
      let dx = x[j] - x[i], dy = y[j] - y[i];
      const d = Math.hypot(dx, dy) || 0.01, f = (d - (R[i] + R[j] + COLLIDE_PAD)) * 0.01 * Math.min(1, w / 3) * heat;
      dx /= d; dy /= d;
      vx[i] += f * dx; vy[i] += f * dy; vx[j] -= f * dx; vy[j] -= f * dy;
    }
    for (let c = 0; c < K; c++) {
      vx[c] = (vx[c] + (cx - x[c]) * 0.002 * heat) * 0.6; vy[c] = (vy[c] + (cy - y[c]) * 0.002 * heat) * 0.6;
      x[c] += vx[c]; y[c] += vy[c];
    }
  }
  return { x, y, R };
}

// --- the engine ---------------------------------------------------------------------------------
// The layout's whole state. `graph` takes the page's nodes, relations and contexts as arrays: each
// node's index in the graph before (`prev`, -1 for a new one) lets a node that stays keep the
// position, velocity and crowd it has here, while a new one starts where the page placed it, and
// `moved` says the page moved every node itself (a remembered picture brought to its centre).
function createLayout() {
  let N = 0, E = 0, gen = 0;
  let x = new Float64Array(0), y = new Float64Array(0), vx = new Float64Array(0), vy = new Float64Array(0);
  let rad = new Float64Array(0), deg = new Float64Array(0), crowd = new Float64Array(0), crowdSet = new Uint8Array(0);
  // Each node's workspace by index - nodes with none share one - and which of them are named: the far
  // field counts every one, the workspaces' cohesion only the named.
  let ws = new Int32Array(0), wsCount = 0, wsNamed = new Uint8Array(0), wsNamedCount = 0, typ = new Int32Array(0);
  let ea = new Int32Array(0), eb = new Int32Array(0);
  let regCount = 0, regStart = new Int32Array(1), regMem = new Int32Array(0), regDrawn = new Int32Array(0), mark = new Int32Array(0);
  let alpha = 1, overlapOwed = 0, motion = 0, steps = 0, stepMs = 0, graphMs = 0;   // what a step and a graph cost
  let comm = { of: new Int32Array(0), count: 0 };   // the graph's communities (multilevel start, semantic zoom)
  // What the page says about the moment: modes, the window's centre, the zoom (the coast is measured
  // on the screen), the held and the focused node, and the group targets of group mode.
  const p = { hullForce: true, clusterMode: false, wsMode: false, wcx: 0, wcy: 0, camS: 1, drag: -1, dragX: 0, dragY: 0, focus: -1, targets: null };
  // Kept between steps: the near-force grid (a counting sort of the nodes into cells, so a cell's nodes
  // are order[start[c] .. start[c + 1])), the far field's per-cell sums, and the regions' geometry.
  let gOrder = new Int32Array(0), gStart = new Int32Array(0), gCursor = new Int32Array(0), gCell = new Int32Array(0);
  let gW = 0, gH = 0, gSize = 0;
  let farN = new Float64Array(0), farX = new Float64Array(0), farY = new Float64Array(0), farR = new Float64Array(0), farWs = new Int32Array(0);
  let cdx = new Float64Array(0), cdy = new Float64Array(0), near = new Float32Array(0);
  let regCx = new Float64Array(0), regCy = new Float64Array(0), regR = new Float64Array(0), regSx = new Float64Array(0), regSy = new Float64Array(0);

  function graph(g) {
    const t0 = performance.now();
    takeGraph(g);
    graphMs = performance.now() - t0;
  }
  function takeGraph(g) {
    const n = g.deg.length, nx = new Float64Array(n), ny = new Float64Array(n), nvx = new Float64Array(n), nvy = new Float64Array(n);
    const nc = new Float64Array(n), ncs = new Uint8Array(n);
    for (let i = 0; i < n; i++) {
      const k = g.prev ? g.prev[i] : -1;
      if (!g.moved && k >= 0 && k < N) {
        nx[i] = x[k]; ny[i] = y[k]; nvx[i] = vx[k]; nvy[i] = vy[k]; nc[i] = crowd[k]; ncs[i] = crowdSet[k];
      } else {
        nx[i] = g.x[i]; ny[i] = g.y[i]; nvx[i] = g.vx ? g.vx[i] : 0; nvy[i] = g.vy ? g.vy[i] : 0;
        if (k >= 0 && k < N) { nc[i] = crowd[k]; ncs[i] = crowdSet[k]; }
      }
    }
    N = n; x = nx; y = ny; vx = nvx; vy = nvy; crowd = nc; crowdSet = ncs;
    deg = g.deg; rad = new Float64Array(N);
    for (let i = 0; i < N; i++) rad[i] = radiusOfDegree(deg[i]);
    ws = g.ws; wsNamed = g.wsNamed; wsCount = wsNamed.length; typ = g.typ;
    wsNamedCount = 0;
    for (let w = 0; w < wsCount; w++) wsNamedCount += wsNamed[w];
    ea = g.ea; eb = g.eb; E = ea.length;
    regStart = g.regStart; regMem = g.regMem; regDrawn = g.regDrawn; regCount = regStart.length - 1;
    mark = new Int32Array(N).fill(-1);
    gen = g.gen;
    comm = communitiesOf(N, ea, eb);
    // A fresh layout of many nodes - most of them new to it, none moved by the page - starts from its
    // communities (U36).
    let fresh = 0;
    for (let i = 0; i < N; i++) if (!(g.prev && g.prev[i] >= 0)) fresh++;
    if (!g.moved && N >= MULTI_MIN && fresh >= 0.8 * N) multilevel();
  }
  // The communities' layout, carried to their members: the k-th member of a community, in index order,
  // starts at k on a spiral within its disc; the full layout then continues from MULTI_ALPHA.
  function multilevel() {
    const K = comm.count, of = comm.of, size = new Float64Array(K);
    let cx = 0, cy = 0;
    for (let i = 0; i < N; i++) { size[of[i]]++; cx += x[i]; cy += y[i]; }
    cx /= N; cy /= N;
    // A node with no relation is a community of one with nothing to lay it out against: it waits on a
    // ring outside the others, and the repulsion takes it from there as it always did.
    const alone = new Uint8Array(K);
    for (let i = 0; i < N; i++) if (size[of[i]] === 1 && deg[i] === 0) alone[of[i]] = 1;
    const between = new Map();
    for (let e = 0; e < E; e++) {
      const a = of[ea[e]], b = of[eb[e]];
      if (a === b) continue;
      const key = a < b ? a * K + b : b * K + a;
      between.set(key, (between.get(key) || 0) + 1);
    }
    const links = [];
    for (const [key, w] of between) links.push([Math.floor(key / K), key % K, w]);
    // The communities that are laid out, by a dense index of their own.
    const laid = new Int32Array(K).fill(-1), back = [];
    for (let k = 0; k < K; k++) if (!alone[k]) { laid[k] = back.length; back.push(k); }
    const lsize = Float64Array.from(back, k => size[k]);
    const llinks = links.filter(([a, b]) => laid[a] >= 0 && laid[b] >= 0).map(([a, b, w]) => [laid[a], laid[b], w]);
    const c = coarseLayout(back.length, lsize, llinks, cx, cy), placed = new Float64Array(K);
    let reach = 0;
    for (let j = 0; j < back.length; j++) reach = Math.max(reach, Math.hypot(c.x[j] - cx, c.y[j] - cy) + c.R[j]);
    let outside = 0;
    for (let i = 0; i < N; i++) {
      const k = of[i];
      if (alone[k]) {
        const a = outside * 2.39996, r = reach + COLLIDE_PAD + 30 * Math.sqrt(outside++);
        x[i] = cx + r * Math.cos(a); y[i] = cy + r * Math.sin(a);
      } else {
        const d = laid[k], j = placed[k]++, a = j * 2.39996, r = 0.9 * c.R[d] * Math.sqrt((j + 0.5) / size[k]);
        x[i] = c.x[d] + r * Math.cos(a); y[i] = c.y[d] + r * Math.sin(a);
      }
      vx[i] = 0; vy[i] = 0;
    }
    alpha = Math.min(alpha, MULTI_ALPHA);
  }
  function set(q) { Object.assign(p, q); }
  function wake(a) { alpha = Math.max(alpha, a); }
  // A remembered picture shows at once: the layout starts no hotter than the reveal.
  function cool(a) { alpha = Math.min(alpha, a); }
  function pinned(i) { return i === p.drag || i === p.focus; }
  // Whether the layout still moves: force applies, the last collision pass is owed, a node is held,
  // or the coast is still visible on the screen (Section 26.3, U28).
  function warm() { return N > 0 && (alpha >= ALPHA_MIN || overlapOwed > 0 || p.drag >= 0 || motion * p.camS > MOTION_MIN_PX); }
  function state() { return { gen, alpha, overlapOwed, motion, steps, stepMs, graphMs }; }

  // The near-force grid: a counting sort of the nodes into square cells at least `cell` wide, grown
  // so the cells stay within about four per node (Section 26.4, U29).
  function binGrid(cell) {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (let i = 0; i < N; i++) {
      if (x[i] < minX) minX = x[i]; if (x[i] > maxX) maxX = x[i];
      if (y[i] < minY) minY = y[i]; if (y[i] > maxY) maxY = y[i];
    }
    const area = Math.max(1, (maxX - minX) * (maxY - minY));
    cell = Math.max(cell, Math.sqrt(area / (4 * N + 64)));
    gSize = cell;
    gW = Math.floor((maxX - minX) / cell) + 1;
    gH = Math.floor((maxY - minY) / cell) + 1;
    const cells = gW * gH;
    if (gCell.length < N) { gCell = new Int32Array(N * 2); gOrder = new Int32Array(N * 2); }
    if (gStart.length < cells + 1) { gStart = new Int32Array((cells + 1) * 2); gCursor = new Int32Array((cells + 1) * 2); }
    gStart.fill(0, 0, cells + 1);
    for (let i = 0; i < N; i++) {
      const c = Math.floor((x[i] - minX) / cell) + Math.floor((y[i] - minY) / cell) * gW;
      gCell[i] = c; gStart[c + 1]++;
    }
    for (let c = 0; c < cells; c++) gStart[c + 1] += gStart[c];
    gCursor.set(gStart.subarray(0, cells));
    for (let i = 0; i < N; i++) gOrder[gCursor[gCell[i]]++] = i;
  }

  function step() {
    if (N === 0) return;   // no graph yet: nothing to step, and nothing to count
    const t0 = performance.now();
    forces();
    steps++; stepMs = performance.now() - t0;
  }
  function forces() {
    motion = 0;
    if (N === 0) return;
    if (p.drag >= 0 && p.drag < N) { x[p.drag] = p.dragX; y[p.drag] = p.dragY; }
    const cooling = alpha >= ALPHA_MIN;
    if (cooling) alpha += (0 - alpha) * ALPHA_DECAY;
    const active = alpha >= ALPHA_MIN;   // dormant once cooling is done - no force is applied
    // The step that cools the layout owes the last collision pass; a layout at rest visits no pair.
    if (cooling && !active) overlapOwed = OVERLAP_STEPS;
    const pairs = active || overlapOwed > 0;
    let overlapped = false;

    // Scale by node count: more nodes spread wider (repulsion range/strength up, center attraction down).
    // Prevents a hairball (central clumping) - base is tuned for small graphs, large ones expand via spread.
    const spread = Math.min(4, Math.max(1, Math.sqrt(N / 20)));
    const range = RANGE_BASE * spread, centerG = CENTER_BASE / spread;
    const repulse = REPULSE * spread, maxV = MAX_V * Math.min(spread, 2);

    // Collision displacement is accumulated per node, then clamped to a cap - stops a hub overlapping
    // many neighbors from flinging far in a single step (instead of moving directly).
    if (cdx.length < N) { cdx = new Float64Array(N * 2); cdy = new Float64Array(N * 2); near = new Float32Array(N * 2); }
    cdx.fill(0, 0, N); cdy.fill(0, 0, N);
    if (active) near.fill(0, 0, N);
    // While workspaces cohere (below), two of them push apart as wholes, so the repulsion between their
    // nodes is turned down: on a large graph its range reaches about 960px, and a hundred nodes of one
    // workspace pushing each node of a small one kept the small one strung out across the map.
    const wsApart = p.wsMode && !p.clusterMode && wsNamedCount > 1;

    // Near forces from a grid (Section 26.4, U29). Every force in this loop acts within a reach -
    // repulsion its range, crowding DENSITY_R, collision two radii and the padding - so the nodes are
    // binned into cells and a node meets only the nodes of its own cell and the adjacent ones. A cell
    // pairs with itself and four forward neighbours, so each pair is met once, and the lower index goes
    // first, so a coincident pair still parts the same way. While force applies the cells are a sixth
    // of the repulsion's range: pairs in a node's own and the adjacent cells are met one by one - every
    // collision and every crowd among them, since a cell is never narrower than those reaches - and the
    // repulsion of the cells beyond, out to the range, is taken from each cell as a whole (the far field).
    const reach = active ? Math.max(range, DENSITY_R) : 2 * NODE_R_MAX + COLLIDE_PAD;
    const reach2 = reach * reach;
    if (pairs) binGrid(active ? Math.max(DENSITY_R, 2 * NODE_R_MAX + COLLIDE_PAD, range / FAR_SPLIT) : reach);
    const FORWARD = [[1, 0], [-1, 1], [0, 1], [1, 1]];
    if (pairs) for (let cy = 0; cy < gH; cy++) for (let cx = 0; cx < gW; cx++) {
      const c = cx + cy * gW, c0 = gStart[c], c1 = gStart[c + 1];
      if (c0 === c1) continue;
      for (let nb = -1; nb < FORWARD.length; nb++) {
        let n0, n1;
        if (nb < 0) { n0 = c0; n1 = c1; }
        else {
          const ox = cx + FORWARD[nb][0], oy = cy + FORWARD[nb][1];
          if (ox < 0 || ox >= gW || oy >= gH) continue;
          const o = ox + oy * gW; n0 = gStart[o]; n1 = gStart[o + 1];
          if (n0 === n1) continue;
        }
        for (let pp = c0; pp < c1; pp++) for (let q = nb < 0 ? pp + 1 : n0; q < n1; q++) {
          const gi = gOrder[pp], gj = gOrder[q];
          const i = gi < gj ? gi : gj, j = gi < gj ? gj : gi;
          let dx = x[j] - x[i], dy = y[j] - y[i];
          if (dx * dx + dy * dy >= reach2) continue;   // beyond every force's reach
          let d = Math.hypot(dx, dy);
          if (d < 0.5) {
            // (Nearly) coincident coordinates have a zero direction and cannot be pushed apart - separate in a deterministic direction (prevents degeneracy).
            const ang = ((i * 7 + j * 13) % 628) / 100;
            dx = Math.cos(ang); dy = Math.sin(ang); d = 0.5;
          } else { dx /= d; dy /= d; }
          const d2 = d * d;
          // Repulsion only when active (cooling), and near-range only (0 outside the range).
          if (active && d < range) {
            // The larger a node's neighbor count (radius), the more gently repulsion is weighted up - spacing around a hub widens (clamped to a cap).
            const w = Math.min(REPULSE_HUB_MAX, (rad[i] + rad[j]) / (2 * NODE_R_BASE))
              * (wsApart && ws[i] !== ws[j] ? WS_CROSS_REPULSE : 1);
            const rf = repulse * alpha * w * (1 - d / range) / Math.max(d2, MIN_SEP * MIN_SEP);
            vx[i] -= rf * dx; vy[i] -= rf * dy; vx[j] += rf * dx; vy[j] += rf * dy;
          }
          if (active && d < DENSITY_R) { near[i]++; near[j]++; }
          // Collision minimum gap = sum of the two radii + padding -> a node with more neighbors (larger) gets wider spacing around it.
          const minD = rad[i] + rad[j] + COLLIDE_PAD;
          if (d < minD) {
            overlapped = true;
            const push = (minD - d) / 2;
            cdx[i] -= dx * push; cdy[i] -= dy * push; cdx[j] += dx * push; cdy[j] += dy * push;
          }
        }
      }
    }

    // The far field (Section 26.4): beyond a node's own and the adjacent cells, out to the repulsion's
    // range, each cell pushes as one body at its nodes' centre - its count weighted as every pair would
    // weight it, by the node's radius and the cell's mean radius and by how many of its nodes are in
    // another workspace. It is the one approximation the layout makes (Barnes-Hut's).
    if (active && gW * gH > 9) {
      const cells = gW * gH;
      if (farN.length < cells) {
        farN = new Float64Array(cells * 2); farX = new Float64Array(cells * 2);
        farY = new Float64Array(cells * 2); farR = new Float64Array(cells * 2);
      }
      farN.fill(0, 0, cells); farX.fill(0, 0, cells); farY.fill(0, 0, cells); farR.fill(0, 0, cells);
      const wc = wsApart ? wsCount : 0;
      if (wc) {
        if (farWs.length < cells * wc) farWs = new Int32Array(cells * wc * 2);
        farWs.fill(0, 0, cells * wc);
      }
      for (let k = 0; k < N; k++) {
        const c = gCell[k];
        farN[c]++; farX[c] += x[k]; farY[c] += y[k]; farR[c] += rad[k];
        if (wc) farWs[c * wc + ws[k]]++;
      }
      const span = Math.ceil(range / gSize), range2 = range * range, floor2 = MIN_SEP * MIN_SEP;
      for (let i = 0; i < N; i++) {
        if (pinned(i)) continue;
        const ci = gCell[i], cx = ci % gW, cy = (ci - cx) / gW;
        for (let oy = -span; oy <= span; oy++) {
          const yy = cy + oy;
          if (yy < 0 || yy >= gH) continue;
          for (let ox = -span; ox <= span; ox++) {
            if (ox >= -1 && ox <= 1 && oy >= -1 && oy <= 1) continue;   // near cells were met pair by pair
            const xx = cx + ox;
            if (xx < 0 || xx >= gW) continue;
            const c = xx + yy * gW, n = farN[c];
            if (!n) continue;
            const mx = farX[c] / n - x[i], my = farY[c] / n - y[i], dd = mx * mx + my * my;
            if (dd >= range2) continue;
            const d = Math.sqrt(dd) || 0.5;
            const same = wc ? farWs[c * wc + ws[i]] : n;
            const count = same + (n - same) * WS_CROSS_REPULSE;
            const w = Math.min(REPULSE_HUB_MAX, (rad[i] + farR[c] / n) / (2 * NODE_R_BASE)) * count;
            const rf = repulse * alpha * w * (1 - d / range) / Math.max(dd, floor2);
            vx[i] -= rf * mx / d; vy[i] -= rf * my / d;
          }
        }
      }
    }

    // Residual overlap is cleaned up by the collision displacement below, for the steps the cooling
    // owes and no longer: the pass ends as soon as nothing overlaps. Do not reheat here - wake only
    // from events like a new node, a drag or a resize.
    if (!active && overlapOwed > 0) overlapOwed = overlapped ? overlapOwed - 1 : 0;
    if (active) {
      // Each node's crowd, smoothed over steps so a link's length does not flicker as its ends move,
      // and the graph's average crowd that a link's ends are measured against.
      let crowdSum = 0;
      for (let k = 0; k < N; k++) {
        crowd[k] = crowdSet[k] ? crowd[k] + (near[k] - crowd[k]) * 0.15 : near[k];
        crowdSet[k] = 1;
        crowdSum += crowd[k];
      }
      const crowdRef = Math.sqrt(crowdSum / N + 1);
      for (let e = 0; e < E; e++) {
        const a = ea[e], b = eb[e];
        let dx = x[b] - x[a], dy = y[b] - y[a];
        const d = Math.hypot(dx, dy) || 1;
        const da = Math.max(1, deg[a] || 1), db = Math.max(1, deg[b] || 1), lo = Math.sqrt(Math.min(da, db));
        const cr = Math.min(CROWD_MAX, Math.max(CROWD_MIN, Math.sqrt((crowd[a] + crowd[b]) / 2 + 1) / crowdRef));
        const f = (d - (LINK_BASE + LINK_PER_DEG * lo) * cr) * (LINK_K / lo) * alpha; dx /= d; dy /= d;
        // The end with fewer links moves more: a leaf comes to its hub rather than the hub to the leaf.
        const ma = 2 * db / (da + db), mb = 2 - ma;
        vx[a] += f * dx * ma; vy[a] += f * dy * ma; vx[b] -= f * dx * mb; vy[b] -= f * dy * mb;
      }
    }

    // Hyperedge layout (Principle 11 second-order structure "well cohered"): (1) pull members toward
    // each context's centroid to cohere the hull tightly, and (2) push apart the centroids of
    // non-overlapping regions to widen the gap. Regions that share a node stay naturally close because
    // the shared node is pulled to both centroids at once (C1: overlap = connective tissue). The
    // membership comes kept as index arrays (Section 27.4, U34) and the geometry goes into arrays
    // kept between steps, so a step allocates nothing per region. Group mode takes precedence.
    if (p.hullForce && !p.clusterMode && active && regCount) {
      const H = regCount, start = regStart, mem = regMem;
      if (regCx.length < H) {
        regCx = new Float64Array(H * 2); regCy = new Float64Array(H * 2); regR = new Float64Array(H * 2);
        regSx = new Float64Array(H * 2); regSy = new Float64Array(H * 2);
      }
      // Geometry: each context's centre and its members' mean distance from it, clamped to a cap - so
      // a huge grab-bag cannot push the whole layout.
      for (let h = 0; h < H; h++) {
        const a = start[h], b = start[h + 1];
        let cx = 0, cy = 0;
        for (let k = a; k < b; k++) { cx += x[mem[k]]; cy += y[mem[k]]; }
        cx /= b - a; cy /= b - a;
        let r = 0;
        for (let k = a; k < b; k++) r += Math.hypot(x[mem[k]] - cx, y[mem[k]] - cy);
        regCx[h] = cx; regCy[h] = cy; regR[h] = Math.min(r / (b - a), HULL_R_CAP);
      }
      // (1) Cohesion: member -> its own centroid, scaled by size - larger hulls pull tighter, small ones
      // stay loose. Every context of two or more pulls; one of two is a spring between two entities seen
      // together.
      for (let h = 0; h < H; h++) {
        const a = start[h], b = start[h + 1];
        const cf = HYPER_PULL * (HULL_COH_MIN + hullSizeNorm(b - a) * (HULL_COH_MAX - HULL_COH_MIN)) * alpha;
        for (let k = a; k < b; k++) {
          const m = mem[k];
          if (pinned(m)) continue;
          vx[m] += (regCx[h] - x[m]) * cf; vy[m] += (regCy[h] - y[m]) * cf;
        }
      }
      // (2) Separation: push apart only disjoint regions (not sharing a node) - a shared one is a
      // connected context and must stay attached (C1). Each region's net push is clamped to
      // HULL_MAX_PUSH and then given to its members. Only the regions the map draws - three or more
      // members - push apart: ten thousand contexts of two, pushed apart, were a second repulsion on
      // top of the first. Regions within reach have centres at most 2 * HULL_R_CAP + HULL_PAD apart,
      // so the centres are binned into cells that wide and a region meets only its own and the
      // adjacent cells' (Section 26.4); the shared-member test runs only for a pair close enough to
      // push, and marks one region's members and reads the other's.
      regSx.fill(0, 0, H); regSy.fill(0, 0, H);
      const RC = 2 * HULL_R_CAP + HULL_PAD, cellOf = new Map();
      const hk = (gx, gy) => (gx + 32768) * 65536 + (gy + 32768);
      for (const i of regDrawn) {
        const k = hk(Math.floor(regCx[i] / RC), Math.floor(regCy[i] / RC));
        const l = cellOf.get(k);
        if (l) l.push(i); else cellOf.set(k, [i]);
      }
      for (const i of regDrawn) {
        for (let k = start[i]; k < start[i + 1]; k++) mark[mem[k]] = i;
        const gx = Math.floor(regCx[i] / RC), gy = Math.floor(regCy[i] / RC);
        for (let oy = -1; oy <= 1; oy++) for (let ox = -1; ox <= 1; ox++) {
          for (const j of cellOf.get(hk(gx + ox, gy + oy)) || []) {
            if (j <= i) continue;
            const dx = regCx[j] - regCx[i], dy = regCy[j] - regCy[i], d = Math.hypot(dx, dy) || 0.01;
            const want = regR[i] + regR[j] + HULL_PAD;
            if (d >= want) continue;
            let shared = false;
            for (let k = start[j]; k < start[j + 1] && !shared; k++) shared = mark[mem[k]] === i;
            if (shared) continue;   // a connected context stays (C1)
            const mag = (want - d) * HULL_SEP * alpha, ux = dx / d * mag, uy = dy / d * mag;
            regSx[i] -= ux; regSy[i] -= uy; regSx[j] += ux; regSy[j] += uy;
          }
        }
      }
      for (const i of regDrawn) {
        let mx = regSx[i], my = regSy[i]; const mm = Math.hypot(mx, my);
        if (mm === 0) continue;
        if (mm > HULL_MAX_PUSH) { mx *= HULL_MAX_PUSH / mm; my *= HULL_MAX_PUSH / mm; }
        for (let k = start[i]; k < start[i + 1]; k++) { const m = mem[k]; if (!pinned(m)) { vx[m] += mx; vy[m] += my; } }
      }
    }

    // Workspaces cohere (Section 5). While the boundaries draw workspaces and the scope holds more than
    // one, a node outside its workspace's natural radius is pulled toward that workspace's centroid,
    // and workspaces whose disks overlap are pushed apart, as disjoint regions are. Inside the radius
    // nothing pulls, so a large workspace keeps the shape its relations give it.
    if (p.wsMode && !p.clusterMode && active && wsNamedCount > 1) {
      const cnt = new Float64Array(wsCount), wx = new Float64Array(wsCount), wy = new Float64Array(wsCount);
      for (let k = 0; k < N; k++) { const w = ws[k]; if (!wsNamed[w]) continue; cnt[w]++; wx[w] += x[k]; wy[w] += y[k]; }
      const live = [];
      for (let w = 0; w < wsCount; w++) if (cnt[w]) { wx[w] /= cnt[w]; wy[w] /= cnt[w]; live.push(w); }
      if (live.length > 1) {
        // The pull's radius is what the nodes would fill; the push's is what they do fill (the 90th
        // percentile of their distances from the centre), or a small workspace's centre settles inside a
        // large one's spread and its nodes are pulled into the other's crowd.
        const dist = live.map(() => []);
        const slot = new Int32Array(wsCount).fill(-1);
        live.forEach((w, s) => { slot[w] = s; });
        for (let k = 0; k < N; k++) { const w = ws[k]; if (slot[w] >= 0) dist[slot[w]].push(Math.hypot(x[k] - wx[w], y[k] - wy[w])); }
        const r = new Float64Array(wsCount), extent = new Float64Array(wsCount);
        live.forEach((w, s) => {
          const d = dist[s].sort((a, b) => a - b);
          r[w] = WS_RADIUS * Math.sqrt(cnt[w]);
          extent[w] = Math.max(r[w], d[Math.floor(d.length * 0.9)]);
        });
        for (let k = 0; k < N; k++) {
          const w = ws[k];
          if (!cnt[w] || pinned(k)) continue;
          const dx = wx[w] - x[k], dy = wy[w] - y[k], d = Math.hypot(dx, dy);
          if (d > r[w]) { const f = (d - r[w]) / d * WS_PULL * alpha; vx[k] += dx * f; vy[k] += dy * f; }
        }
        const sx = new Float64Array(wsCount), sy = new Float64Array(wsCount);
        for (let a = 0; a < live.length; a++) for (let b = a + 1; b < live.length; b++) {
          const i = live[a], j = live[b];
          const dx = wx[j] - wx[i], dy = wy[j] - wy[i], d = Math.hypot(dx, dy) || 0.01, want = extent[i] + extent[j] + WS_PAD;
          if (d < want) {
            const mag = (want - d) * WS_SEP * alpha, ux = dx / d * mag, uy = dy / d * mag;
            sx[i] -= ux; sy[i] -= uy; sx[j] += ux; sy[j] += uy;
          }
        }
        for (const w of live) {
          let mx = sx[w], my = sy[w]; const mm = Math.hypot(mx, my);
          if (mm === 0) continue;
          if (mm > WS_MAX_PUSH) { mx *= WS_MAX_PUSH / mm; my *= WS_MAX_PUSH / mm; }
          for (let k = 0; k < N; k++) if (ws[k] === w && !pinned(k)) { vx[k] += mx; vy[k] += my; }
        }
      }
    }

    // Group mode: the group-target attraction replaces the center attraction (the page places the
    // targets, one per type on a circle).
    const tgt = p.clusterMode && active ? p.targets : null;
    for (let k = 0; k < N; k++) {
      if (pinned(k)) { vx[k] = 0; vy[k] = 0; continue; }
      if (active) {
        if (tgt) {
          const t = typ[k], gx = t >= 0 && 2 * t + 1 < tgt.length ? tgt[2 * t] : p.wcx, gy = t >= 0 && 2 * t + 1 < tgt.length ? tgt[2 * t + 1] : p.wcy;
          vx[k] += (gx - x[k]) * CLUSTER_PULL * alpha; vy[k] += (gy - y[k]) * CLUSTER_PULL * alpha;
        } else {
          vx[k] += (p.wcx - x[k]) * centerG * alpha; vy[k] += (p.wcy - y[k]) * centerG * alpha;
        }
      }
      vx[k] *= DAMPING; vy[k] *= DAMPING;
      // speed cap - even under a large force, nothing flies off-screen.
      const sp = Math.hypot(vx[k], vy[k]);
      if (sp > maxV) { vx[k] *= maxV / sp; vy[k] *= maxV / sp; }
      // the collision displacement is also clamped to a per-node cap before adding.
      let mx = cdx[k], my = cdy[k]; const m = Math.hypot(mx, my);
      if (m > MAX_PUSH) { mx *= MAX_PUSH / m; my *= MAX_PUSH / m; }
      x[k] += vx[k] + mx; y[k] += vy[k] + my;
      // What this node actually moved: damping and integration run whether or not force applies, so
      // nodes coast for roughly half a second after the last force.
      const stepped = Math.hypot(vx[k] + mx, vy[k] + my);
      if (stepped > motion) motion = stepped;
    }

    // Central-axis anchor: the pairwise forces are action-reaction symmetric, but the clamps break that
    // symmetry, so the constellation slowly drifts to one side. Rigidly translate every node so the
    // centroid returns to the window's centre - positions only, no momentum. Skipped while dragging (do
    // not fight the pointer); pinned nodes are held in place.
    if (p.drag < 0) {
      let sx = 0, sy = 0;
      for (let k = 0; k < N; k++) { sx += x[k]; sy += y[k]; }
      const offx = (p.wcx - sx / N) * ANCHOR_K, offy = (p.wcy - sy / N) * ANCHOR_K;
      if (offx || offy) for (let k = 0; k < N; k++) if (!pinned(k)) { x[k] += offx; y[k] += offy; }
      // The recenter shifts every node, so it is motion too.
      const shifted = Math.hypot(offx, offy);
      if (shifted > motion) motion = shifted;
    }
    // The coast ends once it is too slow to see on the screen, and the layout is then at rest: its
    // velocities and its motion are dropped, so a zoom into it later magnifies no motion that would
    // step it again (U28). A wake starts it afresh.
    if (!active && overlapOwed === 0 && p.drag < 0 && motion * p.camS <= MOTION_MIN_PX) {
      vx.fill(0, 0, N); vy.fill(0, 0, N);
      motion = 0;
    }
  }

  return { graph, set, wake, cool, step, warm, state, positions: () => ({ x, y, n: N }) };
}

// --- as a worker --------------------------------------------------------------------------------
// The page sends {kind: "graph"}, {kind: "set"}, {kind: "wake"}, {kind: "cool"} and {kind: "shown"}; the worker steps
// while the layout is warm and sends {kind: "state"}, with the positions as 32-bit floats when a step
// or a graph has moved them since the last. Behind the loader it steps as fast as it can and reports
// at most every frame's worth of time; once the map shows, it takes at most one step every
// SHOWN_STEP_MS.
if (typeof WorkerGlobalScope !== "undefined" && self instanceof WorkerGlobalScope) {
  const L = createLayout();
  let shown = false, timer = 0, sentGen = -1, sentSteps = -1;
  // A message that moves nothing - the camera's place at rest, a cooling - is answered without the
  // positions, so the page keeps what it has worked out from them (Section 27.7).
  const report = () => {
    const st = L.state();
    if (st.gen === sentGen && st.steps === sentSteps) { self.postMessage({ kind: "state", ...st }); return; }
    sentGen = st.gen; sentSteps = st.steps;
    const { x, y, n } = L.positions(), fx = new Float32Array(n), fy = new Float32Array(n);
    for (let i = 0; i < n; i++) { fx[i] = x[i]; fy[i] = y[i]; }
    self.postMessage({ kind: "state", ...st, x: fx, y: fy }, [fx.buffer, fy.buffer]);
  };
  const run = () => {
    timer = 0;
    if (!L.warm()) { report(); return; }
    const t0 = performance.now();
    if (shown) L.step();
    else do { L.step(); } while (L.warm() && performance.now() - t0 < SHOWN_STEP_MS);
    report();
    if (L.warm()) timer = setTimeout(run, shown ? Math.max(0, SHOWN_STEP_MS - (performance.now() - t0)) : 0);
  };
  const kick = () => { if (!timer) timer = setTimeout(run, 0); };
  self.onmessage = ev => {
    const m = ev.data;
    if (m.kind === "graph") L.graph(m);
    else if (m.kind === "set") L.set(m.set);
    else if (m.kind === "wake") L.wake(m.alpha);
    else if (m.kind === "cool") L.cool(m.alpha);
    else if (m.kind === "shown") shown = m.shown;
    kick();
  };
  self.postMessage({ kind: "ready" });
}
