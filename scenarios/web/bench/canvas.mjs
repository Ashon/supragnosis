// The scale bench (docs/design-system.md Section 26.1): what the map costs as a workspace grows. A
// seeded synthetic graph answers /api/graph and /api/hypergraph from the bench's proxy, and the viewer's own
// functions - draw and nodeAt, globals of its classic script - are timed from outside, so the script
// under measure is the one that ships. The layout's steps, in a worker or not (Section 27.5), are
// read from what it reports: layout.stats(), its steps so far and what the last one cost. A measurement, not a test: CI does not run it, since
// frame times on a shared runner are noise.
//
//   node bench/canvas.mjs                         # Chrome, 500 to 10,000 entities
//   SIZES=10000,20000 BROWSER=webkit node bench/canvas.mjs
//
// SUPRAGNOSIS_SCENARIO_BIN names the binary (a release build by default, as for the scenarios);
// BROWSER is chromium (the system's Chrome) or webkit (Playwright's, which `npx playwright install
// webkit` fetches); OUT is where the rows go; SHOTS=1 keeps a screenshot of each size beside them;
// NEAR_SCALE is the reading zoom the pan is taken at again (0.6), and DRAG_LIMIT how long a dragged
// layout is followed to rest.
import { chromium, webkit } from "@playwright/test";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { startDaemon } from "../harness.mjs";

const SIZES = (process.env.SIZES || "500,1000,2000,5000,10000").split(",").map(Number);
const BROWSER = process.env.BROWSER || "chromium";
const REVEAL_LIMIT = Number(process.env.REVEAL_LIMIT || 300000);
const OUT = process.env.OUT || path.resolve(import.meta.dirname, `../results/bench-canvas-${BROWSER}.json`);
const SHOTS = process.env.SHOTS === "1";
// BREAKDOWN=1 also times the layers draw() calls - the region field, the workspace outlines and the
// minimap - so a frame's drawing can be told apart.
const PARTS = process.env.BREAKDOWN === "1" ? ["drawCloudField", "drawWsHulls", "drawMinimap"] : [];
// The pan is taken again at a reading zoom, where nodes are drawn full - rings, arrowheads and names.
const NEAR_SCALE = Number(process.env.NEAR_SCALE || 0.6);
const DRAG_LIMIT = Number(process.env.DRAG_LIMIT || 30000);

const TYPES = ["Component", "Concept", "Decision", "Document", "Feature", "Tool", "Service", "Database", "Process", "Metric", "Release", "Artifact"];
const RELS = ["depends_on", "relates_to", "implements", "part_of", "uses", "reads", "writes_to", "stored_in"];

/// A linear congruential generator, so every run of a size draws the same graph.
function rng(seed) {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}

/// Communities of 20 to 60 entities, about 1.3 relations per entity inside one and 0.2 across, and the
/// contexts a store filled through observe holds (design-system.md Section 27.1): each community's
/// groups of twelve, one observation each, and a context of two for every relation between groups,
/// its own observation. It is the graph scenarios' dev seeding enters, in the same random order.
function graph(n) {
  const r = rng(n);
  const nodes = [], edges = [], hyperedges = [], communities = [];
  for (let i = 0; i < n;) {
    const size = Math.min(n - i, 20 + Math.floor(r() * 40));
    communities.push([i, i + size]);
    i += size;
  }
  for (let i = 0; i < n; i++) {
    nodes.push({
      id: "n" + i, name: "entity " + i, type: TYPES[Math.floor(r() * TYPES.length)], description: "synthetic",
      degree: 0, sources: 1, origins: ["localhost"], workspace: "bench", trust_tier: "agent_extracted",
    });
  }
  const link = (a, b) => {
    if (a === b) return;
    edges.push({ from: "n" + a, to: "n" + b, type: RELS[Math.floor(r() * RELS.length)], description: "", trust_tier: "agent_extracted" });
    nodes[a].degree++;
    nodes[b].degree++;
  };
  const context = members => hyperedges.push({
    id: "h" + hyperedges.length, members, member_names: members.map(id => "entity " + id.slice(1)),
    size: members.length, sources: 1, trust_tier: "agent_extracted",
  });
  const groupOf = new Int32Array(n);
  for (const [lo, hi] of communities) {
    const m = hi - lo;
    for (let k = 0; k < Math.round(m * 1.3); k++) link(lo + Math.floor(r() * m), lo + Math.floor(r() * m));
    for (let a = lo; a < hi; a += 12) {
      const b = Math.min(hi, a + 12);
      for (let i = a; i < b; i++) groupOf[i] = a;
      if (b - a >= 2) context(Array.from({ length: b - a }, (_, j) => "n" + (a + j)));
    }
  }
  for (let k = 0; k < Math.round(n * 0.2); k++) link(Math.floor(r() * n), Math.floor(r() * n));
  for (const e of edges) {
    const a = Number(e.from.slice(1)), b = Number(e.to.slice(1));
    if (groupOf[a] !== groupOf[b]) context([e.from, e.to]);
  }
  const type_counts = {};
  for (const v of nodes) type_counts[v.type] = (type_counts[v.type] || 0) + 1;
  const stats = { node_count: n, edge_count: edges.length, type_counts, trust_counts: { agent_extracted: n } };
  return { graph: { workspace: "bench", nodes, edges, stats }, hyper: { workspace: "bench", nodes, hyperedges, stats } };
}

/// What a stretch of moving frames came to: how many, how often, and how long each took.
function moving(name, { draw, at }) {
  const span = at.length > 1 ? (at[at.length - 1] - at[0]) / 1000 : 0;
  return {
    [name + "_frames"]: draw.length, [name + "_s"]: +span.toFixed(1),
    [name + "_fps"]: span ? +((draw.length - 1) / span).toFixed(1) : null,
    [name + "_frame_p50"]: pct(draw, 0.5), [name + "_frame_p95"]: pct(draw, 0.95),
  };
}

/// The p-th percentile, to two decimals; null for no samples.
function pct(samples, p) {
  if (!samples.length) return null;
  const s = [...samples].sort((a, b) => a - b);
  return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(2);
}

const launcher = BROWSER === "webkit" ? webkit : chromium;
const daemon = await startDaemon();
// The bench's proxy: the scenario harness's, except that the graph and the hypergraph of the size
// being measured are answered here, as the daemon answers them (Section 27.3) - with an ETag, and a
// 304 to a poll that asks with the one the page holds. Without, every poll applied the same graph
// again, which the daemon has not made the page do since.
const served = new Map();
const proxyServer = http.createServer((req, res) => {
  const it = served.get(req.url.split("?")[0]);
  if (it) {
    if (req.headers["if-none-match"] === it.tag) { res.writeHead(304, { ETag: it.tag }).end(); return; }
    res.writeHead(200, { "Content-Type": "application/json", ETag: it.tag }).end(it.body);
    return;
  }
  const up = http.request({ socketPath: daemon.sock, path: req.url, method: req.method, headers: { ...req.headers, host: "viz" } }, u => {
    res.writeHead(u.statusCode, u.headers);
    u.pipe(res);
  });
  up.on("error", e => { if (!res.headersSent) res.writeHead(502); res.end(String(e)); });
  req.pipe(up);
});
await new Promise(ok => proxyServer.listen(0, "127.0.0.1", ok));
const proxy = { url: `http://127.0.0.1:${proxyServer.address().port}/`, close: () => new Promise(ok => { proxyServer.closeAllConnections(); proxyServer.close(() => ok()); }) };
const browser = await launcher.launch(BROWSER === "webkit" ? {} : { channel: "chrome" });
const rows = [];
try {
  for (const n of SIZES) {
    const { graph: g, hyper: h } = graph(n);
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, colorScheme: "dark" });
    const page = await context.newPage();
    served.set("/api/graph", { body: JSON.stringify(g), tag: `"bench-graph-${n}"` });
    served.set("/api/hypergraph", { body: JSON.stringify(h), tag: `"bench-hyper-${n}"` });
    const started = Date.now();
    await page.goto(proxy.url + "?workspace=bench");
    await page.waitForFunction(() => typeof draw === "function" && typeof layout === "object");
    await page.evaluate(parts => {
      window.__bench = { sim: [], draw: [], drawAt: [], hit: [], frames: [], parts: {} };
      const time = (name, into) => {
        const f = window[name];
        window[name] = function (...args) {
          const t = performance.now();
          try { return f.apply(this, args); } finally { window.__bench[into].push(performance.now() - t); }
        };
      };
      time("draw", "draw");
      // A step the layout reports, once for each frame that brings new ones.
      { let seen = 0; const d = window.draw; window.draw = function (...args) { try { return d.apply(this, args); } finally { const st = layout.stats(); if (st.steps > seen) { window.__bench.sim.push(st.stepMs); seen = st.steps; } } }; }
      { const d = window.draw; window.draw = function (...args) { window.__bench.drawAt.push(performance.now()); return d.apply(this, args); }; }
      time("nodeAt", "hit");
      for (const name of parts) {
        const f = window[name];
        window.__bench.parts[name] = [];
        window[name] = function (...args) {
          const t = performance.now();
          try { return f.apply(this, args); } finally { window.__bench.parts[name].push(performance.now() - t); }
        };
      }
    }, PARTS);
    let revealed = true;
    try {
      await page.waitForFunction(() => nodes.length > 0 && !settling, null, { timeout: REVEAL_LIMIT, polling: 200 });
    } catch {
      revealed = false;
    }
    const settle = await page.evaluate(() => ({ sim: __bench.sim.slice(), steps: layout.stats().steps, worker: layout.stats().worker, nodes: nodes.length, edges: edges.length, regions: hyperedges.length }));
    const row = {
      browser: BROWSER, n, edges: settle.edges, regions: settle.regions, revealed, layout_worker: settle.worker,
      first_picture_s: +((Date.now() - started) / 1000).toFixed(1), settle_steps: settle.steps,
      settle_step_p50: pct(settle.sim, 0.5), settle_step_p95: pct(settle.sim, 0.95),
    };
    // The first picture comes before the layout stops: it keeps cooling, and stepping, after the
    // reveal. A pan or a zoom is measured on a map at rest, and the wait for rest is its own number.
    let rested = false;
    if (revealed) {
      const from = await page.evaluate(() => [__bench.draw.length, __bench.drawAt.length]);
      try {
        await page.waitForFunction(() => !animating(), null, { timeout: REVEAL_LIMIT, polling: 200 });
        rested = true;
      } catch { /* still moving at the limit */ }
      const tail = await page.evaluate(() => layout.stats().steps);
      Object.assign(row, { rested, at_rest_s: +((Date.now() - started) / 1000).toFixed(1), steps_to_rest: tail });
      // The frames between the reveal and rest: the layout still steps in every one of them.
      Object.assign(row, moving("cool", await page.evaluate(([d, a]) => ({ draw: __bench.draw.slice(d), at: __bench.drawAt.slice(a) }), from)));
    }
    if (revealed) {
      await page.waitForTimeout(300);
      // A two-second drag across the map; frame intervals come from requestAnimationFrame.
      await page.evaluate(() => {
        __bench.sim = []; __bench.draw = []; __bench.frames = []; window.__stop = false;
        let last = performance.now();
        const tick = t => { __bench.frames.push(t - last); last = t; if (!window.__stop) requestAnimationFrame(tick); };
        requestAnimationFrame(tick);
      });
      // A press on a node drags the node, which rightly moves the layout; a pan starts on empty
      // canvas, so look for a point no node covers, nearest the middle of the map's window.
      const start = await page.evaluate(() => {
        let best = null;
        for (let y = 120; y <= 780; y += 8) for (let x = 320; x <= 1100; x += 8) {
          if (nodeAt(x, y)) continue;
          const d = Math.hypot(x - 710, y - 450);
          if (!best || d < best.d) best = { x, y, d };
        }
        return best;
      });
      await page.evaluate(() => { __bench.sim = []; __bench.draw = []; __bench.hit = []; for (const k in __bench.parts) __bench.parts[k] = []; __bench.steps0 = layout.stats().steps; });
      const sx = start ? start.x : 710, sy = start ? start.y : 450;
      await page.mouse.move(sx, sy);
      await page.mouse.down();
      for (let i = 0; i < 60; i++) {
        await page.mouse.move(sx + Math.sin(i / 6) * 250, sy + Math.cos(i / 8) * 120);
        await page.waitForTimeout(33);
      }
      await page.mouse.up();
      const pan = await page.evaluate(() => { window.__stop = true; return { sim: __bench.sim.slice(), steps: layout.stats().steps - __bench.steps0, here: !layout.stats().worker, draw: __bench.draw.slice(), frames: __bench.frames.slice(1), parts: __bench.parts }; });
      for (const [name, samples] of Object.entries(pan.parts || {})) row["pan_" + name + "_p50"] = pct(samples, 0.5);
      // A step taken in the page's frame is part of that frame's time; one in a worker is not.
      const drawing = pan.here ? pan.draw.map((d, i) => d - (pan.sim[i] || 0)) : pan.draw;
      Object.assign(row, {
        pan_from_empty: !!start, pan_frames: pan.draw.length, pan_sim_calls: pan.steps,
        pan_draw_p50: pct(pan.draw, 0.5), pan_draw_p95: pct(pan.draw, 0.95),
        pan_sim_p50: pct(pan.sim, 0.5), pan_drawing_p50: pct(drawing, 0.5), pan_drawing_p95: pct(drawing, 0.95),
        pan_interval_p50: pct(pan.frames, 0.5), pan_interval_p95: pct(pan.frames, 0.95),
      });
      // Ten wheel steps, five in and five out.
      await page.evaluate(() => { __bench.draw = []; __bench.sim = []; for (const k in __bench.parts) __bench.parts[k] = []; });
      for (let i = 0; i < 10; i++) {
        await page.mouse.wheel(0, i < 5 ? -240 : 240);
        await page.waitForTimeout(60);
      }
      await page.waitForTimeout(400);
      const zoom = await page.evaluate(() => ({ draw: __bench.draw.slice(), parts: __bench.parts }));
      Object.assign(row, { zoom_frames: zoom.draw.length, zoom_draw_p50: pct(zoom.draw, 0.5), zoom_draw_p95: pct(zoom.draw, 0.95) });
      for (const [name, samples] of Object.entries(zoom.parts || {})) {
        row["zoom_" + name + "_p50"] = pct(samples, 0.5);
        row["zoom_" + name + "_p95"] = pct(samples, 0.95);
      }
      // A hover's hit test on every pointer move.
      await page.evaluate(() => { __bench.hit = []; });
      for (let i = 0; i < 40; i++) {
        await page.mouse.move(300 + i * 20, 200 + (i % 7) * 60);
        await page.waitForTimeout(10);
      }
      Object.assign(row, { hover_hit_p50: pct(await page.evaluate(() => __bench.hit.slice()), 0.5) });
      if (SHOTS) await page.screenshot({ path: OUT.replace(/\.json$/, `-${n}.png`) });
      const fit = await page.evaluate(() => ({ s: cam.s, x: cam.x, y: cam.y }));
      // The same pan at a reading zoom, about the middle of the map's window.
      await page.evaluate(scale => {
        const w = mapWindow(), cx = (w.l + w.r) / 2, cy = (w.t + w.b) / 2, wx = (cx - cam.x) / cam.s, wy = (cy - cam.y) / cam.s;
        camT.s = cam.s = scale; camT.x = cam.x = cx - wx * scale; camT.y = cam.y = cy - wy * scale; userMoved = true; requestFrame();
      }, NEAR_SCALE);
      await page.waitForTimeout(600);
      const near = await page.evaluate(() => {
        let best = null;
        for (let y = 120; y <= 780; y += 4) for (let x = 320; x <= 1100; x += 4) {
          if (nodeAt(x, y)) continue;
          const d = Math.hypot(x - 710, y - 450);
          if (!best || d < best.d) best = { x, y, d };
        }
        return best;
      });
      if (near) {
        await page.evaluate(() => { __bench.sim = []; __bench.draw = []; __bench.steps0 = layout.stats().steps; });
        await page.mouse.move(near.x, near.y);
        await page.mouse.down();
        for (let i = 0; i < 60; i++) {
          await page.mouse.move(near.x + Math.sin(i / 6) * 250, near.y + Math.cos(i / 8) * 120);
          await page.waitForTimeout(33);
        }
        await page.mouse.up();
        const nearPan = await page.evaluate(() => ({ draw: __bench.draw.slice(), sim: layout.stats().steps - __bench.steps0 }));
        Object.assign(row, { near_scale: NEAR_SCALE, pan_near_sim_calls: nearPan.sim, pan_near_p50: pct(nearPan.draw, 0.5), pan_near_p95: pct(nearPan.draw, 0.95) });
      }
      // A node near the middle, at the fitted view, held and dragged in a circle for two seconds and
      // let go; the frames run until the layout rests again, or DRAG_LIMIT.
      await page.evaluate(f => { camT.s = cam.s = f.s; camT.x = cam.x = f.x; camT.y = cam.y = f.y; requestFrame(); }, fit);
      await page.waitForTimeout(600);
      const grip = await page.evaluate(() => {
        for (let r = 0; r < 300; r += 4) for (let a = 0; a < 6.28; a += 0.2) {
          const x = Math.round(710 + Math.cos(a) * r), y = Math.round(450 + Math.sin(a) * r);
          if (nodeAt(x, y)) return { x, y };
        }
        return null;
      });
      if (grip) {
        const from = await page.evaluate(() => [__bench.draw.length, __bench.drawAt.length]);
        await page.mouse.move(grip.x, grip.y);
        await page.mouse.down();
        for (let i = 1; i <= 60; i++) {
          await page.mouse.move(grip.x + Math.sin(i / 9.5) * 120, grip.y + (1 - Math.cos(i / 9.5)) * 120);
          await page.waitForTimeout(33);
        }
        await page.mouse.up();
        let settled = true;
        try { await page.waitForFunction(() => !animating(), null, { timeout: DRAG_LIMIT, polling: 200 }); } catch { settled = false; }
        Object.assign(row, { drag_rested: settled }, moving("drag", await page.evaluate(([d, a]) => ({ draw: __bench.draw.slice(d), at: __bench.drawAt.slice(a) }), from)));
      }
    }
    rows.push(row);
    console.log(JSON.stringify(row));
    fs.mkdirSync(path.dirname(OUT), { recursive: true });
    fs.writeFileSync(OUT, JSON.stringify(rows, null, 1));
    await context.close();
  }
} finally {
  await browser.close();
  await proxy.close();
  await daemon.stop();
}
