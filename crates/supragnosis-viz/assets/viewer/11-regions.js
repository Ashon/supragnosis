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

