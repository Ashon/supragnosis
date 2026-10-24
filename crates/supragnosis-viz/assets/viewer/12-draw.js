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
      * (!hot && ((trustOff.size && (trustOff.has(e.a.trust_tier) || trustOff.has(e.b.trust_tier))) || saidOut(e.a) || saidOut(e.b)) ? DIM_EDGE / EDGE_ALPHA : 1)
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
    const base = stepped((on && !trustOff.has(n.trust_tier) && !saidOut(n) ? 1 : DIM_NODE) * n._o), r = rOf(n);
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
    n.last_said ? html`<br><span class="k">Last said ${obsWhen(n.last_said)}</span>` : "",
    first ? html`<div class="tdesc">${first.length > 120 ? first.slice(0, 120).trimEnd() + "..." : first}</div>` : "",
    n.contested ? html`<div class="tcontest">Two sources disagree</div>` : ""]}`;
}


