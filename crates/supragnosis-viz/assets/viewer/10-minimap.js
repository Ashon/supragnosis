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
    // Under 760 the toolbar sits at the map's foot above the open sheet, however high (Section 29.18).
    if (innerWidth < 760 || hb.top > innerHeight / 2) B = Math.min(B, hb.top - 8);
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
  // The neighbourhood, not the namesakes the selection also lights: they can sit across the map
  // (Section 29.16).
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
  ["workspace", "A workspace outline around each workspace: a sharing unit. The same name in two workspaces is two entities", "Workspace"],
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

