"use strict";
// The landing's stage (docs/design-system.md Section 11, U10): how knowledge moves through
// supragnosis, in five scenes drawn in the viewer's own marks. An entity is its type's color, by the
// viewer's own function of the type's name, with a halo of the ground; a gold ring is human confirmed;
// the coral badge is a contested belief; teal is an agent at work, as the viewer's footprint glow
// marks what an agent's session touched, and a person and their verdicts are ink. Every frame is a
// pure function of (scene, time), with nothing random, so every load draws the same frames - the way
// every node holding the same log holds the same graph. Words worth reading live in the page's steps,
// not here: the canvas is hidden from assistive technology.
(function (global) {
  // The landing's tokens (site/style.css), one value each; a test holds them equal.
  const C = {
    bg: "#10131b",       // --panel, the stage's ground and every label's halo
    well: "#161a23",     // --panel-2, the log
    ink: "#e9e4d6",      // --ink
    ink2: "#cad2de",     // --ink2
    muted: "#b0b8c7",    // --muted
    gold: "#d9a544",     // --accent, a merge waiting for a person
    goldHi: "#f0c469",   // --gold-bright, human confirmed
    teal: "#56b3a2",     // --ok, an agent at work
    bad: "#e07a5f",      // --bad, contested
  };
  // The viewer's neutral relation line (viewer.js EDGE_NEUTRAL), drawn at its alpha.
  const EDGE_NEUTRAL = "#6b7383", EDGE_ALPHA = 0.42;
  const rgbOf = hex => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16)).join(",");
  const RGB = { teal: rgbOf(C.teal), ink: rgbOf(C.ink) };
  // Labels are names and short captions, so they are set in the reading face (design-system.md
  // Section 4), the same stack site/style.css names.
  const SANS = '"Pretendard Variable", Pretendard, system-ui, -apple-system, "Segoe UI", sans-serif';

  // ---- a type's color: the viewer's functions, copied character for character (a test compares) ----
  const TYPE_BAND = [
    "#889d37", "#82d48c", "#3cd1f5", "#67acf7", "#8986de", "#c38fe3", "#c670ab", "#ff9abd",
    "#adc35e", "#53a45e", "#009cba", "#4c8fd8", "#aeadff", "#aa76c8", "#ffb7e6", "#e17da1",
  ];
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

  // ---- a made-up project (no real store) ----
  const AGENTS = ["claude-code", "cursor", "codex"];
  // Type names whose colors in the viewer's band sit apart from each other and from gold, coral and
  // teal, so no fill is read as a mark.
  const TYPE = stableColors(["App", "Database", "Feature", "Pattern", "Event"]);
  // u, v place a node in the graph's box; side is where its label sits; key labels stay when the
  // last scene draws two machines at a smaller size
  const NODES = [
    { id: "api", name: "payments-api", type: "App", u: .50, v: .38, r: 11, side: "r", key: 1 },
    { id: "pgs", name: "postgres", type: "Database", u: .80, v: .10, r: 8, side: "r" },
    { id: "pg", name: "pg", type: "Database", u: .95, v: .32, r: 6, side: "r" },
    { id: "hook", name: "webhooks", type: "Feature", u: .30, v: .58, r: 9, side: "l" },
    { id: "idem", name: "idempotency", type: "Pattern", u: .50, v: .84, r: 8, side: "r", key: 1 },
    { id: "retry", name: "retry-policy", type: "Pattern", u: .10, v: .24, r: 7, side: "r" },
    { id: "inc", name: "incident-912", type: "Event", u: .16, v: .94, r: 6, side: "r" },
    { id: "ledger", name: "ledger-db", type: "Database", u: .32, v: .04, r: 6, side: "r" },
    { id: "rate", name: "rate-limit", type: "Feature", u: .72, v: .60, r: 6, side: "r" },
    { id: "deploy", name: "deploy", type: "Event", u: .90, v: .80, r: 5, side: "r" },
  ];
  const NODE = Object.fromEntries(NODES.map(n => [n.id, n]));
  // unlabeled neighbours, so the graph reads as a graph and not a diagram
  const SATS = [
    ["api", .62, .24], ["api", .40, .26], ["api", .62, .48], ["hook", .20, .72], ["pgs", .68, .00],
    ["ledger", .20, .10], ["idem", .62, .97], ["rate", .82, .50], ["inc", .05, .82],
  ];
  const EDGES = [
    ["api", "pgs"], ["api", "hook"], ["hook", "idem"], ["hook", "retry"], ["api", "ledger"],
    ["inc", "hook"], ["inc", "idem"], ["rate", "api"], ["deploy", "api"], ["pg", "api"],
  ];
  // The log: who wrote each entry, the relations it asserts, and what else it is about. The log
  // records the machine and the person it acts for, not which agent app wrote an entry; the
  // agents' names are drawn beside it, never on it.
  const LOG = [
    { by: 0, edges: [0] }, { by: 1, edges: [1, 3] }, { by: 2, edges: [5, 6] },
    { by: 0, edges: [2] }, { by: 1, edges: [4, 7] }, { by: 2, edges: [8] },
    { by: 2, edges: [9] },                                     // one agent calls postgres "pg"
    { by: 0, edges: [], about: ["retry"] },                    // a second type for retry-policy
    { by: 1, kind: "proposal", edges: [], about: ["pg", "pgs"] },   // merge pg into postgres?
    { by: 2, kind: "proposal", edges: [], about: ["hook", "idem"] }, // confirm entry 3's claim?
    { by: "you", kind: "verdict", edges: [], about: ["pg", "pgs"] },
    { by: "you", kind: "verdict", edges: [], about: ["hook", "idem"] },
  ];
  // the claim an agent asks you to confirm: webhooks need an idempotency key
  const CLAIM = 3;
  const REGION = ["hook", "idem", "inc"];
  // a search for webhook idempotency: what it finds, and the entries that say so (a verdict is
  // not a receipt; the entries it decided on are)
  const MATCH = ["hook", "idem", "inc"], MATCH_EDGES = [2, 5, 6], RECEIPTS = [2, CLAIM];
  const asserts = LOG.map(o => [...new Set(o.edges.flatMap(e => EDGES[e]))]);
  const firstSeen = {};
  asserts.forEach((ids, i) => ids.forEach(id => { if (!(id in firstSeen)) firstSeen[id] = i; }));

  // ---- time ----
  const clamp = (x, a = 0, b = 1) => Math.max(a, Math.min(b, x));
  const lerp = (a, b, p) => a + (b - a) * p;
  const outCubic = x => 1 - Math.pow(1 - x, 3);
  const inOut = x => x < .5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2;
  const outBack = x => { const c = 1.6; return 1 + (c + 1) * Math.pow(x - 1, 3) + c * Math.pow(x - 1, 2); };
  // eased progress of a segment that starts at a and lasts d. It is 1 at t = Infinity, so a
  // scene's resting frame is the same function evaluated at Infinity.
  const seg = (t, a, d, e = outCubic) => t <= a ? 0 : t >= a + d ? 1 : e((t - a) / d);
  const bump = (t, a, d) => t <= a || t >= a + d ? 0 : Math.sin(Math.PI * (t - a) / d);

  function blank() {
    const nodes = {};
    for (const n of NODES) nodes[n.id] = { a: 0, s: 0, grow: 0, ring: "", ringP: 0, bad: 0, dim: 0, hi: 0, mu: 0, mv: 0 };
    return {
      agents: AGENTS.map(() => 0), you: 0, youGlow: 0, nodes, sats: SATS.map(() => 0), satDim: 0,
      edges: EDGES.map(() => ({ p: 0, hi: 0, a: 1 })), ticks: [], fly: [], rise: [],
      region: 0, proposal: 0, ripple: 0, trace: 0, prov: 0, ask: 0, answered: 0, receipts: 0,
      cam: 0, b: null, hub: 0, signed: 0,
    };
  }
  const clone = s => JSON.parse(JSON.stringify(s));

  // an entry leaves its writer at t0 and lands in the log 0.8s later
  function write(st, t, i, t0) {
    const o = LOG[i], p = seg(t, t0, .8, inOut), you = o.by === "you";
    const g = bump(t, t0 - .1, .7);
    if (you) st.youGlow = Math.max(st.youGlow, g); else st.agents[o.by] = Math.max(st.agents[o.by], g);
    if (p > 0 && p < 1) st.fly.push({ from: you ? { you: 1 } : { agent: o.by }, to: { tick: i }, p, c: you ? "ink" : "teal", path: "down" });
    if (p >= 1) st.ticks[i] = { kind: o.kind || "obs", flash: 1 - seg(t, t0 + .8, 1.1), hi: 0, conf: 0 };
    return t0 + .8;
  }
  // the projector: an entry rises out of the log into what it is about
  function project(st, t, i, t0) {
    const pr = seg(t, t0, .45);
    for (const id of [...asserts[i], ...(LOG[i].about || [])]) {
      if (pr > 0 && pr < 1) st.rise.push({ tick: i, node: id, p: pr, c: LOG[i].by === "you" ? "ink" : "teal" });
      const n = st.nodes[id], pn = seg(t, t0 + .3, .55, outBack);
      if (firstSeen[id] === i) { n.a = Math.min(1, pn * 1.4); n.s = pn; }
      else if (asserts[i].includes(id)) n.grow += 1.2 * seg(t, t0 + .3, .4);
    }
    SATS.forEach((s, k) => { if (firstSeen[s[0]] === i) st.sats[k] = seg(t, t0 + .5, .6); });
    for (const e of LOG[i].edges) st.edges[e].p = seg(t, t0 + .45, .5);
    return t0 + .45;
  }

  // What the agents' disagreement and your two verdicts leave on a graph. Every machine that holds
  // the log draws it the same way: a verdict is an entry, and the tier it grants is read from the log
  // (resolution.md Section 6), so the second machine calls this exactly as the first does. The
  // confirmed entry's tick turns gold, as the viewer's Log draws a human-confirmed row, and the
  // entities it names take the gold ring (a node's trust is its best support's tier).
  function settle(w, bad, merge, confirm) {
    w.nodes.retry.bad = bad;
    const pg = w.nodes.pg;
    pg.mu = (NODE.pgs.u - NODE.pg.u) * merge; pg.mv = (NODE.pgs.v - NODE.pg.v) * merge;
    pg.a = Math.min(pg.a, 1 - merge); w.edges[9].a = 1 - merge; w.nodes.pgs.grow += 1.5 * merge; w.proposal *= 1 - merge;
    for (const id of asserts[CLAIM]) { w.nodes[id].ring = confirm > 0 ? "gold" : ""; w.nodes[id].ringP = confirm; }
    if (w.ticks[CLAIM]) w.ticks[CLAIM].conf = confirm;
  }

  // ---- scenes: each starts from the previous scene's resting frame ----
  const SCENES = [
    { id: "overview", dur: 0, run() {} },
    { id: "write", dur: 3.9, run(st, t) {
      for (let i = 0; i < 6; i++) write(st, t, i, .25 + i * .42);
      st.prov = seg(t, 1.15, .45);
    } },
    { id: "structure", dur: 4.3, run(st, t) {
      st.prov *= 1 - seg(t, 0, .4);
      for (let i = 0; i < 6; i++) { project(st, t, i, .2 + i * .48); st.ticks[i].flash = bump(t, .2 + i * .48, .5); }
      st.region = seg(t, 3.3, .8);
    } },
    { id: "govern", dur: 4.8, run(st, t) {
      project(st, t, 6, write(st, t, 6, .1));                               // pg arrives
      const l7 = write(st, t, 7, .45); project(st, t, 7, l7);               // retry-policy, typed twice
      const l8 = write(st, t, 8, 1.0); project(st, t, 8, l8);               // a merge is proposed
      st.proposal = seg(t, l8 + .3, .4);
      project(st, t, 9, write(st, t, 9, 1.3));                              // and a claim put up to confirm
      st.you = seg(t, 1.7, .4);
      const l10 = write(st, t, 10, 2.3); project(st, t, 10, l10);           // you accept the merge
      const l11 = write(st, t, 11, 2.9); project(st, t, 11, l11);           // and the claim
      settle(st, seg(t, l7 + .35, .5, outBack), seg(t, l10 + .3, .8, inOut), seg(t, l11 + .3, .7, inOut));
    } },
    { id: "recall", dur: 4.6, run(st, t) {
      st.agents[1] = Math.max(bump(t, 0, .9), bump(t, 3.85, .8));
      st.ask = seg(t, .1, .35);
      st.ripple = t > .45 && t < 1.85 ? seg(t, .45, 1.4, x => x) : 0;
      const dim = seg(t, .8, .5);
      for (const n of NODES) { const hit = MATCH.includes(n.id); st.nodes[n.id].dim = hit ? 0 : dim; st.nodes[n.id].hi = hit ? dim : 0; }
      st.satDim = dim;
      MATCH_EDGES.forEach(e => { st.edges[e].hi = seg(t, 1.15, .5); });
      st.trace = seg(t, 1.7, .6, inOut);
      for (const i of RECEIPTS) st.ticks[i].hi = seg(t, 2.1, .3);
      st.receipts = seg(t, 2.3, .4);
      const ans = seg(t, 3.1, .8, inOut);
      if (ans > 0 && ans < 1) st.fly.push({ from: { node: "idem" }, to: { agent: 1 }, p: ans, c: "ink", path: "arc" });
      st.answered = seg(t, 3.9, .3);
    } },
    { id: "travel", dur: 5.0, run(st, t) {
      const out = 1 - seg(t, 0, .45);
      for (const n of NODES) { st.nodes[n.id].dim *= out; st.nodes[n.id].hi *= out; }
      st.satDim *= out; st.trace *= out; st.ask *= out; st.answered *= out; st.receipts *= out;
      st.edges.forEach(e => { e.hi *= out; }); st.ticks.forEach(k => { k.hi *= out; });
      st.cam = seg(t, .35, 1.0, inOut); st.hub = seg(t, 1.0, .4);
      const b = st.b = blank();
      b.a = seg(t, .9, .5);
      const land = i => 1.4 + i * .16 + .55;
      for (let i = 0; i < LOG.length; i++) {
        const t0 = 1.4 + i * .16, p = seg(t, t0, .55, inOut);
        if (p > 0 && p < 1) st.fly.push({ from: { tick: i }, to: { btick: i }, p, c: "ink", path: "hub" });
        if (p >= 1) {
          b.ticks[i] = { kind: LOG[i].kind || "obs", flash: 1 - seg(t, land(i), .6), hi: 0, conf: 0 };
          if (asserts[i].length) project(b, t, i, land(i) - .3);
        }
      }
      // your verdicts are entries too: they replay here, so the merge and the gold ring arrive with them
      settle(b, seg(t, land(7) + .1, .4, outBack), seg(t, land(10) + .1, .5, inOut), seg(t, land(11) + .1, .5, inOut));
      b.region = seg(t, land(5), .6);
      st.signed = seg(t, land(11) + .5, .4);
    } },
  ];

  const rest = [];
  function resting(i) {
    if (rest[i]) return rest[i];
    return (rest[i] = i === 0 ? resting(3) : run(i, Infinity));
  }
  function run(i, t) {
    const s = clone(i === 1 ? blank() : resting(i - 1));
    s.fly = []; s.rise = [];
    SCENES[i].run(s, t);
    return s;
  }
  const stateAt = (i, t) => (t === Infinity || i === 0 ? resting(i) : run(i, t));

  // ---- layout: wide (16:10) or tall (4:5), chosen by the canvas's own shape ----
  function layout(w, h) {
    const tall = w / h < 1.1, W = tall ? 800 : 1000, H = tall ? 1000 : 625;
    const k = Math.min(w / W, h / H), base = { tall, W, H, k, w, h, ox: (w - W * k) / 2, oy: (h - H * k) / 2 };
    return Object.assign(base, tall ? {
      g: [70, 200, 690, 790], agents: [[150, 84], [330, 84], [510, 84]], you: [680, 84], log: [40, 760, 880],
      halves: [{ s: .55, x: 180, y: -100 }, { s: .55, x: 180, y: 405 }], hubAt: [730, 465],
    } : {
      g: [300, 46, 930, 460], agents: [[240, 120], [240, 205], [240, 290]], you: [240, 400], log: [300, 960, 548],
      halves: [{ s: .68, x: -178, y: 96 }, { s: .68, x: 312, y: 96 }], hubAt: [500, 64],
    });
  }

  // ---- drawing ----
  let sprites = {}, spriteDpr = 0;
  // a glow is one radial gradient per color, rasterized once per device scale and stamped with
  // drawImage; shadowBlur would blur again on every draw
  function glowSprite(rgb, dpr) {
    if (spriteDpr !== dpr) { sprites = {}; spriteDpr = dpr; }
    if (sprites[rgb]) return sprites[rgb];
    const s = Math.round(64 * dpr), c = document.createElement("canvas");
    c.width = c.height = s;
    const g = c.getContext("2d"), r = g.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
    r.addColorStop(0, `rgba(${rgb},1)`); r.addColorStop(.35, `rgba(${rgb},.35)`); r.addColorStop(1, `rgba(${rgb},0)`);
    g.fillStyle = r; g.fillRect(0, 0, s, s);
    return (sprites[rgb] = c);
  }
  function glow(ctx, rgb, x, y, size, a, dpr) {
    if (a <= .005) return;
    ctx.save(); ctx.globalCompositeOperation = "lighter"; ctx.globalAlpha = Math.min(1, a);
    ctx.drawImage(glowSprite(rgb, dpr), x - size / 2, y - size / 2, size, size); ctx.restore();
  }
  function rrect(ctx, x, y, w, h, r) {
    ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
  }
  // The contested badge's mark (docs/design-system.md Section 28.3, U41): Lucide's circle-alert bar and
  // dot, in its own 24-unit grid, with the badge's disc of radius r taken as the icon's 10-unit circle -
  // the set's 2-unit stroke, never under 1.5 pixels. The viewer draws the same mark.
  const ALERT_MARK = new Path2D("M12 8v4M12 16h.01");
  function alertMark(ctx, x, y, r, color) {
    const k = r / 10;
    ctx.save();
    ctx.translate(x - 12 * k, y - 12 * k); ctx.scale(k, k);
    ctx.lineCap = "round"; ctx.strokeStyle = color; ctx.lineWidth = Math.max(2, 1.5 / k);
    ctx.stroke(ALERT_MARK);
    ctx.restore();
  }
  function label(ctx, s, x, y, color, a, o = {}) {
    if (a <= .01) return;
    ctx.globalAlpha = Math.min(1, a);
    ctx.font = `${o.weight || 400} ${o.size || 11}px ${SANS}`; ctx.textAlign = o.align || "left"; ctx.textBaseline = "middle";
    // a label near an edge of a narrow stage stays on the canvas, whichever way it is aligned
    const tw = ctx.measureText(s).width, cw = ctx.canvas.width / ctx.getTransform().a;
    if (o.align === "right") x = clamp(x, tw + 6, cw - 6);
    else if (o.align === "center") x = clamp(x, tw / 2 + 6, cw - tw / 2 - 6);
    else x = clamp(x, 6, cw - tw - 6);
    if (o.halo !== false) { ctx.lineJoin = "round"; ctx.lineWidth = 3; ctx.strokeStyle = C.bg; ctx.strokeText(s, x, y); }
    ctx.fillStyle = color; ctx.fillText(s, x, y);
    ctx.globalAlpha = 1;
  }

  const ID = { s: 1, x: 0, y: 0 };
  // stage units -> css px, through a camera {s, x, y} in stage units
  const P = (L, cam, x, y) => [L.ox + (cam.x + x * cam.s) * L.k, L.oy + (cam.y + y * cam.s) * L.k];
  function nodeXY(L, cam, id, w) {
    const d = NODE[id], n = w.nodes[id], g = L.g;
    return P(L, cam, g[0] + (d.u + n.mu) * (g[2] - g[0]), g[1] + (d.v + n.mv) * (g[3] - g[1]));
  }
  const nodeR = (L, cam, id, w) => Math.max(2.6, (NODE[id].r + w.nodes[id].grow) * L.k * cam.s * (L.tall ? 1.2 : 1));
  function tickXY(L, cam, i) {
    const [x0, x1, y] = L.log, step = Math.min(30, (x1 - x0 - 40) / 11);
    return P(L, cam, x0 + 22 + i * step, y);
  }
  const agentXY = (L, i) => P(L, ID, ...L.agents[i]);

  function drawWorld(ctx, L, cam, w, o) {
    const { dpr, alpha } = o, sc = cam.s;
    // the log: one well, an entry per tick, colored as the viewer's Log colors a row - an agent's entry
    // ink2, a confirmed one gold - with proposals muted and your verdicts ink
    const a0 = P(L, cam, L.log[0], L.log[2]), a1 = P(L, cam, L.log[1], L.log[2]), lh = Math.max(14, 30 * L.k * sc);
    ctx.globalAlpha = alpha; ctx.fillStyle = C.well;
    rrect(ctx, a0[0], a0[1] - lh / 2, a1[0] - a0[0], lh, Math.min(8, lh / 2)); ctx.fill();
    w.ticks.forEach((k, i) => {
      if (!k) return;
      const [x, y] = tickXY(L, cam, i), tw = Math.max(3, 7 * L.k * sc), th = lh * .56;
      const col = k.conf > .5 ? C.goldHi : k.hi > .5 || k.kind === "verdict" ? C.ink : k.kind === "proposal" ? C.muted : C.ink2;
      ctx.globalAlpha = alpha * (k.kind === "obs" ? .5 + .5 * Math.max(k.flash, k.hi, k.conf) : .95);
      ctx.fillStyle = col; rrect(ctx, x - tw / 2, y - th / 2, tw, th, Math.min(2, tw / 2)); ctx.fill();
      glow(ctx, k.kind === "verdict" ? RGB.ink : RGB.teal, x, y, 34, k.flash * .8 * alpha, dpr);
    });
    ctx.globalAlpha = 1;
    // a region: what was said together, a soft field under its members
    if (w.region > 0) {
      const pts = REGION.map(id => nodeXY(L, cam, id, w));
      const cx = pts.reduce((s, p) => s + p[0], 0) / pts.length, cy = pts.reduce((s, p) => s + p[1], 0) / pts.length;
      const rx = Math.max(...pts.map(p => Math.abs(p[0] - cx))) + 40 * L.k * sc, ry = Math.max(...pts.map(p => Math.abs(p[1] - cy))) + 34 * L.k * sc;
      ctx.save(); ctx.translate(cx, cy); ctx.scale(1, ry / rx);
      const gr = ctx.createRadialGradient(0, 0, 0, 0, 0, rx);
      gr.addColorStop(0, `rgba(${RGB.ink},0.07)`); gr.addColorStop(.75, `rgba(${RGB.ink},0.04)`); gr.addColorStop(1, `rgba(${RGB.ink},0)`);
      ctx.globalAlpha = w.region * alpha * (1 - .6 * w.satDim); ctx.fillStyle = gr; ctx.beginPath(); ctx.arc(0, 0, rx, 0, 7); ctx.fill(); ctx.restore();
      if (o.labels) label(ctx, "webhooks + idempotency", cx - rx * .9, cy + ry * .96, C.muted, w.region * alpha * (1 - w.satDim));
    }
    // relations, drawn from one end toward the other as they are projected
    ctx.lineCap = "round";
    EDGES.forEach(([a, b], i) => {
      const e = w.edges[i];
      if (e.p <= 0 || e.a <= .01) return;
      const pa = nodeXY(L, cam, a, w), pb = nodeXY(L, cam, b, w), dim = Math.max(w.nodes[a].dim, w.nodes[b].dim);
      ctx.globalAlpha = alpha * e.a * (e.hi > 0 ? lerp(EDGE_ALPHA, .95, e.hi) : EDGE_ALPHA * (1 - .7 * dim));
      ctx.strokeStyle = e.hi > .3 ? C.ink : EDGE_NEUTRAL; ctx.lineWidth = e.hi > .3 ? 1.6 : 1.1;
      ctx.beginPath(); ctx.moveTo(pa[0], pa[1]); ctx.lineTo(lerp(pa[0], pb[0], e.p), lerp(pa[1], pb[1], e.p)); ctx.stroke();
    });
    SATS.forEach(([pid, u, v], k) => {
      const a = w.sats[k];
      if (a <= 0) return;
      const g = L.g, p = P(L, cam, g[0] + u * (g[2] - g[0]), g[1] + v * (g[3] - g[1])), q = nodeXY(L, cam, pid, w);
      ctx.globalAlpha = alpha * a * .3 * (1 - .7 * w.satDim); ctx.strokeStyle = EDGE_NEUTRAL; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(q[0], q[1]); ctx.lineTo(p[0], p[1]); ctx.stroke();
      ctx.globalAlpha = alpha * a * .75 * (1 - .75 * w.satDim); ctx.fillStyle = TYPE[NODE[pid].type];
      ctx.beginPath(); ctx.arc(p[0], p[1], Math.max(1.8, 3.2 * L.k * sc), 0, 7); ctx.fill();
    });
    // a merge waiting for a person, drawn as the viewer previews one: an arrow from the name folded
    // away into the one kept, and the relations that would move with it drawn thin (viewer.js, the
    // proposal preview)
    if (w.proposal > .01) {
      const a = nodeXY(L, cam, "pg", w), b = nodeXY(L, cam, "pgs", w), api = nodeXY(L, cam, "api", w);
      const ra = nodeR(L, cam, "pg", w), rb = nodeR(L, cam, "pgs", w);
      const dx = b[0] - a[0], dy = b[1] - a[1], d = Math.hypot(dx, dy) || 1, ux = dx / d, uy = dy / d;
      const tx = b[0] - ux * (rb + 5), ty = b[1] - uy * (rb + 5), hl = clamp(rb * .9, 6, 10), hw = hl * .55;
      ctx.globalAlpha = alpha * w.proposal; ctx.strokeStyle = C.gold; ctx.fillStyle = C.gold;
      ctx.lineWidth = 1.2;
      ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(api[0], api[1]); ctx.stroke();
      ctx.lineWidth = 2;
      ctx.beginPath(); ctx.moveTo(a[0] + ux * (ra + 4), a[1] + uy * (ra + 4)); ctx.lineTo(tx, ty); ctx.stroke();
      ctx.beginPath(); ctx.moveTo(tx, ty); ctx.lineTo(tx - ux * hl - uy * hw, ty - uy * hl + ux * hw);
      ctx.lineTo(tx - ux * hl + uy * hw, ty - uy * hl - ux * hw); ctx.closePath(); ctx.fill();
      if (o.labels) label(ctx, "same thing?", (a[0] + b[0]) / 2 + 10, (a[1] + b[1]) / 2 + 2, C.ink2, alpha * w.proposal);
    }
    // entities
    for (const d of NODES) {
      const n = w.nodes[d.id];
      if (n.a <= .01) continue;
      const [x, y] = nodeXY(L, cam, d.id, w), r = nodeR(L, cam, d.id, w) * Math.max(0, n.s), al = alpha * n.a * (1 - .78 * n.dim);
      glow(ctx, RGB.ink, x, y, r * 2 + 36, n.hi * .3 * alpha, dpr);
      ctx.globalAlpha = al; ctx.fillStyle = TYPE[d.type];
      ctx.beginPath(); ctx.arc(x, y, r, 0, 7); ctx.fill();
      ctx.lineWidth = clamp(r * .35, 1.5, 4); ctx.strokeStyle = C.bg; ctx.stroke();
      // human confirmed: the viewer's trust ring (viewer.js trustRing), drawn closing as it arrives
      if (n.ring === "gold" && n.ringP > 0) {
        ctx.beginPath(); ctx.arc(x, y, r + 3.5, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * n.ringP);
        ctx.lineWidth = 2.4; ctx.strokeStyle = C.goldHi; ctx.stroke();
      }
      if (o.labels || d.key) {
        const l = d.side === "l";
        label(ctx, d.name, l ? x - r - 7 : x + r + 7, y + .5, n.hi > .5 ? C.ink : C.ink2, al * Math.min(1, n.s), { align: l ? "right" : "left", weight: 500 });
      }
    }
    // contested: the viewer's badge, upper right, at full strength
    for (const d of NODES) {
      const n = w.nodes[d.id];
      if (n.bad <= 0 || n.a <= .01) continue;
      const [x, y] = nodeXY(L, cam, d.id, w), r = nodeR(L, cam, d.id, w), bx = x + r * 0.75, by = y - r * 0.75;
      ctx.globalAlpha = alpha * Math.min(1, n.bad);
      ctx.beginPath(); ctx.arc(bx, by, 7 * n.bad, 0, 7); ctx.fillStyle = C.bad; ctx.fill();
      ctx.lineWidth = 1.5; ctx.strokeStyle = C.bg; ctx.stroke();
      if (n.bad > .6) { ctx.globalAlpha = Math.min(1, alpha); alertMark(ctx, bx, by, 7, C.bg); }
    }
    ctx.globalAlpha = 1;
  }

  function draw(ctx, L, st, dpr) {
    ctx.clearRect(0, 0, L.w, L.h);
    const cam = st.cam, h0 = L.halves[0];
    const camA = cam > 0 ? { s: lerp(1, h0.s, cam), x: lerp(0, h0.x, cam), y: lerp(0, h0.y, cam) } : ID;
    const camB = L.halves[1];
    // who writes: the agents on this machine, and you. They leave before the camera moves.
    const col = 1 - clamp(cam * 4);
    if (col > .01) {
      const right = !L.tall, at = (p, dx = 0, dy = 0) => P(L, ID, p[0] + dx, p[1] + dy);
      const head = right ? at(L.agents[0], 0, -58) : at([L.agents[0][0] - 70, L.agents[0][1] - 44]);
      label(ctx, "agents on this laptop", head[0] + (right ? 8 : 0), head[1], C.muted, col, { align: right ? "right" : "left", halo: false });
      const who = (x, y, name, rgb, color, g, a, weight) => {
        glow(ctx, rgb, x, y, 40, g * .9 * a, dpr);
        ctx.globalAlpha = a; ctx.fillStyle = color; ctx.beginPath(); ctx.arc(x, y, 5, 0, 7); ctx.fill();
        if (right) label(ctx, name, x - 14, y + .5, color === C.teal ? C.ink2 : color, a, { size: 12, align: "right", weight, halo: false });
        else label(ctx, name, x, y + 18, color === C.teal ? C.ink2 : color, a, { align: "center", weight, halo: false });
      };
      AGENTS.forEach((name, i) => { const [x, y] = agentXY(L, i); who(x, y, name, RGB.teal, C.teal, st.agents[i], col); });
      if (st.you > 0) { const [x, y] = P(L, ID, ...L.you); who(x, y, "you", RGB.ink, C.ink, st.youGlow, col * st.you, 500); }
      if (st.ask > 0) {
        const [x, y] = agentXY(L, 1), msg = st.answered > .5 ? "got 3 hits, with receipts" : "asks: webhook idempotency";
        if (right) label(ctx, msg, x + 14, y + .5, st.answered > .5 ? C.ink : C.ink2, col * st.ask);
        else label(ctx, msg, L.w / 2, P(L, ID, 0, 162)[1], st.answered > .5 ? C.ink : C.ink2, col * st.ask, { align: "center" });
      }
      const lp = right ? at([L.agents[0][0], L.log[2]]) : at([L.log[0], L.log[2] + 44]);
      if (right) { label(ctx, "the log", lp[0] + 8, lp[1] - 7, C.ink2, col, { size: 12, align: "right", halo: false }); label(ctx, "append-only", lp[0] + 8, lp[1] + 9, C.muted, col, { align: "right", halo: false }); }
      else label(ctx, "the log: append-only", lp[0], lp[1], C.muted, col, { halo: false });
    }
    drawWorld(ctx, L, camA, st, { dpr, alpha: 1, labels: cam < .5 });
    if (st.b && st.b.a > 0) drawWorld(ctx, L, camB, st.b, { dpr, alpha: st.b.a, labels: false });
    // two machines and the hub between them, in the last scene
    if (cam > 0) {
      // each machine is named under the right end of its own log, clear of the log's well
      const name = (c, s, a) => {
        const p = P(L, c, L.log[1], L.log[2]), y = p[1] + Math.max(14, 30 * L.k * c.s) / 2 + 12;
        label(ctx, s, p[0], y, C.ink2, a, { size: 12, weight: 500, align: "right", halo: false });
      };
      name(camA, "laptop", cam);
      if (st.b) name(camB, "desktop", st.b.a);
      if (st.hub > 0) {
        const [x, y] = P(L, ID, ...L.hubAt);
        ctx.globalAlpha = st.hub; ctx.fillStyle = C.ink2; ctx.beginPath(); ctx.arc(x, y, 4, 0, 7); ctx.fill();
        label(ctx, "hub", x, y - 13, C.muted, st.hub, { align: "center", halo: false });
      }
    }

    // the first entry's provenance, said once in the second scene
    if (st.prov > .01) {
      const [x, y] = tickXY(L, camA, 0);
      label(ctx, "laptop, for you, 0.9 sure", x - 3, y - 34, C.ink, st.prov);
      label(ctx, "never edited, only superseded", x - 3, y - 19, C.muted, st.prov);
    }
    // on the receiver the ring came with the verdict, which says where it was cast
    if (st.signed > .01 && st.b) {
      const [x, y] = nodeXY(L, camB, "idem", st.b);
      label(ctx, "confirmed in Review, on laptop", x + 14, y + 15, C.ink2, st.signed * st.b.a);
    }
    // in flight: entries into the log, the log into the graph, answers back, the log across machines
    const hub = P(L, ID, ...L.hubAt);
    const at = ref => ref.agent !== undefined ? agentXY(L, ref.agent) : ref.you ? P(L, ID, ...L.you)
      : ref.tick !== undefined ? tickXY(L, camA, ref.tick) : ref.btick !== undefined ? tickXY(L, camB, ref.btick)
      : nodeXY(L, camA, ref.node, st);
    for (const r of st.rise) {
      const a = tickXY(L, camA, r.tick), b = nodeXY(L, camA, r.node, st), x = lerp(a[0], b[0], r.p), y = lerp(a[1], b[1], r.p);
      ctx.globalAlpha = .35 * (1 - r.p * .5); ctx.strokeStyle = r.c === "ink" ? C.ink : C.teal; ctx.lineWidth = 1;
      ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(x, y); ctx.stroke();
      glow(ctx, RGB[r.c], x, y, 16, .8, dpr);
    }
    if (st.b) for (const r of st.b.rise) {
      const a = tickXY(L, camB, r.tick), b = nodeXY(L, camB, r.node, st.b);
      glow(ctx, RGB.teal, lerp(a[0], b[0], r.p), lerp(a[1], b[1], r.p), 12, .6, dpr);
    }
    for (const f of st.fly) {
      const a = at(f.from), b = at(f.to);
      const c = f.path === "hub" ? hub : f.path === "down" ? (L.tall ? [a[0], b[1]] : [b[0], a[1]])
        : [(a[0] + b[0]) / 2, Math.min(a[1], b[1]) - Math.abs(b[0] - a[0]) * .18 - 10];
      const pt = p => [(1 - p) * (1 - p) * a[0] + 2 * (1 - p) * p * c[0] + p * p * b[0], (1 - p) * (1 - p) * a[1] + 2 * (1 - p) * p * c[1] + p * p * b[1]];
      const color = f.c === "teal" ? C.teal : C.ink;
      for (let j = 4; j >= 1; j--) {
        const q = pt(Math.max(0, f.p - j * .035));
        ctx.globalAlpha = .18 * (5 - j) / 4; ctx.fillStyle = color; ctx.beginPath(); ctx.arc(q[0], q[1], 1.6, 0, 7); ctx.fill();
      }
      const [x, y] = pt(f.p);
      glow(ctx, RGB[f.c], x, y, 18, .9, dpr);
      ctx.globalAlpha = 1; ctx.fillStyle = color; ctx.beginPath(); ctx.arc(x, y, 2.4, 0, 7); ctx.fill();
    }
    // the search: a ripple from the asking agent, then the receipts traced back into the log
    if (st.ripple > 0) {
      const [x, y] = agentXY(L, 1);
      ctx.globalAlpha = (1 - st.ripple) * .45; ctx.strokeStyle = C.ink; ctx.lineWidth = 1.2;
      ctx.beginPath(); ctx.arc(x, y, st.ripple * L.w * .9, 0, 7); ctx.stroke(); ctx.globalAlpha = 1;
    }
    if (st.trace > .01) {
      const a = nodeXY(L, camA, "idem", st);
      ctx.strokeStyle = C.ink2; ctx.lineWidth = 1;
      for (const i of RECEIPTS) {
        const b = tickXY(L, camA, i);
        ctx.globalAlpha = .75 * Math.min(1, st.trace * 2);
        ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(lerp(a[0], b[0], st.trace), lerp(a[1], b[1], st.trace)); ctx.stroke();
      }
      ctx.globalAlpha = 1;
      const b = tickXY(L, camA, RECEIPTS[RECEIPTS.length - 1]);
      label(ctx, "receipts", b[0] + 12, b[1] - 20, C.ink, st.receipts);
    }
  }

  // ---- canvases ----
  const maxDpr = () => Math.min(2, global.devicePixelRatio || 1);
  function sizeOf(canvas) { const r = canvas.getBoundingClientRect(); return [Math.max(1, r.width), Math.max(1, r.height)]; }
  // one still frame: scene i at time t (Infinity = where the scene comes to rest)
  function still(canvas, i, t = Infinity) {
    const dpr = maxDpr(), [w, h] = sizeOf(canvas), ctx = canvas.getContext("2d");
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
    ctx.setTransform(canvas.width / w, 0, 0, canvas.height / h, 0, 0);
    draw(ctx, layout(w, h), stateAt(i, t), dpr);
  }

  // a live stage: go(i) plays scene i when it is the next one and cuts to it otherwise; nothing
  // runs between scenes, so an idle stage costs no frames
  function mount(canvas, opt = {}) {
    const ctx = canvas.getContext("2d"), reduce = global.matchMedia ? matchMedia("(prefers-reduced-motion: reduce)") : { matches: false };
    let scene = opt.scene || 0, start = 0, playing = false, raf = 0, snap = null, snapAt = 0, css = [1, 1], dpr = 1;
    const FADE = 350;
    function frame(now) {
      raf = 0;
      let t = playing ? (now - start) / 1000 : Infinity;
      if (playing && t >= SCENES[scene].dur) { playing = false; t = Infinity; }
      ctx.setTransform(canvas.width / css[0], 0, 0, canvas.height / css[1], 0, 0);
      draw(ctx, layout(css[0], css[1]), stateAt(scene, t), dpr);
      const f = snap ? 1 - (now - snapAt) / FADE : 0;
      if (f > 0) { ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.globalAlpha = f; ctx.drawImage(snap, 0, 0); ctx.globalAlpha = 1; }
      else snap = null;
      if (playing || snap) raf = requestAnimationFrame(frame);
      if (opt.onframe) opt.onframe(scene, t);
    }
    const kick = () => { if (!raf) raf = requestAnimationFrame(frame); };
    // exact device pixels where the browser reports them (not Safari), css px x dpr otherwise
    function resize(entry) {
      dpr = maxDpr();
      const box = entry && entry.contentBoxSize && entry.contentBoxSize[0];
      css = box ? [box.inlineSize, box.blockSize] : sizeOf(canvas);
      const dev = entry && entry.devicePixelContentBoxSize && entry.devicePixelContentBoxSize[0];
      const exact = dev && (global.devicePixelRatio || 1) <= 2;
      const pw = exact ? dev.inlineSize : Math.round(css[0] * dpr), ph = exact ? dev.blockSize : Math.round(css[1] * dpr);
      if (canvas.width !== pw || canvas.height !== ph) { canvas.width = pw; canvas.height = ph; snap = null; }
      if (raf) cancelAnimationFrame(raf);
      raf = 0; frame(performance.now());
    }
    function go(i, replay) {
      if (i === scene && !replay) return;
      const next = replay || i === scene + 1;
      if (!reduce.matches && canvas.width > 1) {
        snap = snap || document.createElement("canvas"); snap.width = canvas.width; snap.height = canvas.height;
        snap.getContext("2d").drawImage(canvas, 0, 0); snapAt = performance.now();
      }
      scene = i; start = performance.now();
      playing = next && !reduce.matches && SCENES[i].dur > 0 && !document.hidden;
      kick();
    }
    const ro = new ResizeObserver(es => resize(es[0]));
    try { ro.observe(canvas, { box: "device-pixel-content-box" }); } catch (e) { ro.observe(canvas); }
    // a window moved to a screen of another density: Safari reports no device-pixel box
    (function watch() {
      if (!global.matchMedia) return;
      matchMedia(`(resolution: ${global.devicePixelRatio || 1}dppx)`).addEventListener("change", () => { resize(); watch(); }, { once: true });
    })();
    document.addEventListener("visibilitychange", () => { if (document.hidden && playing) { playing = false; kick(); } });
    if (reduce.addEventListener) reduce.addEventListener("change", () => { playing = false; snap = null; kick(); });
    if (document.fonts && document.fonts.load) document.fonts.load(`11px ${SANS}`).then(kick, kick);
    return { go, replay: () => go(scene, true), get scene() { return scene; }, get playing() { return playing; } };
  }

  global.Stage = { mount, still, scenes: SCENES.map(s => ({ id: s.id, dur: s.dur })) };
})(window);
