//! docs/design-system.md Sections 26 and 27, the map at scale and at any size: tripwires over the
//! viewer's scripts for the rules that keep its cost in proportion (U28-U39). The timings themselves
//! are the scale bench's (Section 26.1, U31 and U40), which CI does not run.

const SCRIPT: &str = include_str!("../assets/viewer.js");
/// The layout (Section 27.5): its constants, a node's radius, and the engine `createLayout` makes.
const SIM: &str = include_str!("../assets/viewer-sim.js");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// One of the layout engine's own functions, inside createLayout.
fn engine(name: &str) -> &'static str {
    let at = SIM
        .find(&format!("\n  function {name}("))
        .unwrap_or_else(|| panic!("the engine has no function {name}"));
    &SIM[at..at + SIM[at..].find("\n  }\n").expect("it closes")]
}

/// U28: drawing never moves the layout. A frame steps the simulation only while it is warm -
/// settling, applying force, a node held, the settle's last collision pass owed, or still coasting
/// on the screen - so a pan, a zoom or a hover only redraws. Once cooled, the pairs are visited only
/// for the collision pass the cooling owes, which ends as soon as nothing overlaps. In a worker the
/// layout steps on its own (U35); the page's frame steps it only when it runs here.
#[test]
fn drawing_never_moves_the_layout() {
    let draw = function("draw");
    let calls: Vec<usize> = draw.match_indices("layout.tick()").map(|(i, _)| i).collect();
    assert!(!calls.is_empty(), "draw no longer steps the layout at all");
    for at in calls {
        let line = &draw[draw[..at].rfind('\n').map_or(0, |p| p + 1)..at];
        assert!(
            line.contains("if (simWarm())")
                || line.contains("settling && REDUCED_MOTION && layout.here()"),
            "draw steps the layout unguarded: {line}"
        );
    }
    let warm = function("simWarm");
    for part in [
        "settling",
        "alpha >= ALPHA_MIN",
        "drag !== null",
        "overlapOwed > 0",
        "simMotion * cam.s > MOTION_MIN_PX",
    ] {
        assert!(warm.contains(part), "simWarm forgets {part}");
    }
    let step = engine("forces");
    for part in [
        "if (cooling && !active) overlapOwed = OVERLAP_STEPS;",
        "const pairs = active || overlapOwed > 0;",
        "overlapOwed = overlapped ? overlapOwed - 1 : 0;",
    ] {
        assert!(step.contains(part), "the settle's last collision pass is not bounded: {part}");
    }
    assert!(step.contains("if (pairs) "), "a layout at rest still visits its pairs");
    // At rest the coast is over: what is left of it is dropped, so a zoom cannot magnify it back
    // over the threshold and step the layout.
    assert!(
        step.contains(
            "if (!active && overlapOwed === 0 && p.drag < 0 && motion * p.camS <= MOTION_MIN_PX) {"
        ) && step.contains("vx.fill(0, 0, N); vy.fill(0, 0, N);\n      motion = 0;"),
        "a layout at rest keeps a coast a zoom can wake"
    );
    assert!(
        engine("warm").contains("alpha >= ALPHA_MIN || overlapOwed > 0 || p.drag >= 0 || motion * p.camS > MOTION_MIN_PX"),
        "the layout steps on when the page would call it still"
    );
    assert!(
        function("animating").contains("overlapOwed > 0"),
        "the last collision pass stops halfway"
    );
}

/// U29: the layout's forces come from grids. Within a node's own and the adjacent cells - cells a
/// sixth of the repulsion's range, never narrower than collision's or crowding's reach - every pair
/// is met one by one, exactly; beyond them, out to the range, a cell pushes as one body (the far
/// field, Barnes-Hut's approximation); regions push apart only from a grid of their own reach, with
/// the costly shared-member test after the distance. No loop visits every pair of nodes or regions.
#[test]
fn near_forces_come_from_a_grid() {
    let step = engine("forces");
    assert!(
        !step.contains("for (let j = i + 1; j < N; j++)") && !step.contains("j < H; j++)"),
        "the layout visits every pair again"
    );
    for part in [
        "if (pairs) binGrid(active ? Math.max(DENSITY_R, 2 * NODE_R_MAX + COLLIDE_PAD, range / FAR_SPLIT) : reach);",
        "const FORWARD = [[1, 0], [-1, 1], [0, 1], [1, 1]];",
        "if (dx * dx + dy * dy >= reach2) continue;",
        "const i = gi < gj ? gi : gj, j = gi < gj ? gj : gi;",
        "if (ox >= -1 && ox <= 1 && oy >= -1 && oy <= 1) continue;",
        "const count = same + (n - same) * WS_CROSS_REPULSE;",
        "const RC = 2 * HULL_R_CAP + HULL_PAD",
        "if (d >= want) continue;",
    ] {
        assert!(step.contains(part), "the grids' pairing changed: {part}");
    }
    assert!(SIM.contains("const FAR_SPLIT = 6;"), "the far field's cells moved");
    // A cell is never narrower than the reach it is binned for, so no pair within reach is missed.
    assert!(
        engine("binGrid").contains("cell = Math.max(cell, "),
        "a cell can shrink below the reach"
    );
    // The step's scratch is kept between steps, and it reads no node objects.
    assert!(
        !step.contains("new Array(N)")
            && !step.contains("new Float32Array(N)")
            && !step.contains("nodes["),
        "the step allocates per-node scratch or reads the page's nodes again"
    );
    assert!(
        function("nodeById").contains("nodesById()"),
        "a lookup builds its own map of the nodes"
    );
}

/// U30: what the map draws follows the screen. A node or relation that cannot reach the window is not
/// drawn; a node's size on the screen decides its detail - full from 3 pixels of radius, halo and fill
/// from 1.2, a square below - unless it is hovered, selected or active; a relation into a node smaller
/// than full is straight and headless; relations, arrowheads, halos and fills of one look are one path
/// each; names are placed on a grid of cells with a budget per cell; and zooming out stops where the
/// whole map fits, however large.
#[test]
fn what_the_map_draws_follows_the_screen() {
    let draw = function("draw");
    // An arrowhead is a filled triangle left open: the fill closes it, and closing each of thousands
    // in one path made a frame at a reading zoom eighty times slower in Chrome.
    assert!(!draw.contains("h.closePath()"), "the arrowheads' path closes every head again");
    for part in [
        "for (const k of nodesIn(vx0, vy0, vx1, vy1)) {",
        "if (typeOff.has(n.type)) continue;\n    n._vf = drawFrame; n._lod = lodOf(n, act);",
        "for (const i of relationsIn(vx0, vy0, vx1, vy1)) {",
        "Math.max(e.a.x, e.b.x) + LANE_MAX < vx0",
        "const full = hot || lodNow(e.b) === LOD_FULL_TIER;",
    ] {
        assert!(draw.contains(part), "the frame draws what is off the screen: {part}");
    }
    assert!(
        SCRIPT.contains("const LOD_FULL = 3, LOD_DOT = 1.2;"),
        "the sizes of a node moved"
    );
    let lod = function("lodOf");
    for part in [
        "n === hover || n === focus || (act && act.ns.has(n.id))",
        "rs >= LOD_FULL ? LOD_FULL_TIER : rs >= LOD_DOT ? LOD_SMALL_TIER : LOD_DOT_TIER",
    ] {
        assert!(lod.contains(part), "the level of detail is not the screen's: {part}");
    }
    for part in [
        "const full = hot || lodNow(e.b) === LOD_FULL_TIER;",
        "const arrow = full && (showArrows || hot);",
        "if (n._lod !== LOD_FULL_TIER) continue;",
        "if (n._lod === LOD_DOT_TIER) { f.rect(",
    ] {
        assert!(draw.contains(part), "a small node is drawn in full: {part}");
    }
    for part in ["ctx.stroke(l.line);", "ctx.fill(l.heads);", "ctx.fill(halo);", "ctx.fill(f);"] {
        assert!(draw.contains(part), "a look is not one path: {part}");
    }
    assert!(
        !draw.contains("ctx.beginPath(); ctx.moveTo(sx0, sy0);"),
        "a relation is its own path again"
    );
    assert!(
        !draw.contains("labelBoxes.some(") && draw.contains("labelBoxes.full(o.px, o.py)"),
        "names are checked against every name, or without a budget"
    );
    assert!(function("labelGrid").contains("LABEL_PER_CELL"));
    assert!(
        function("zoomAt").contains("scaleFloor()"),
        "zooming out stops short of the whole map"
    );
}

/// U32: layers that do not move are not redrawn. The region field is kept with the camera it was
/// worked out for, from a key of every region's centre, axis, members' reach, hue and strength and of
/// the look - not of what the zoom alone changes, the halo's padding and the fade - on cells that grow
/// with the clouds' work; while only
/// the camera moves it is drawn moved and scaled, and it is worked out again once the camera has
/// rested, when the moved field would leave the window uncovered or be scaled past half or double, or
/// when the key changes - with a margin beyond the window while the layout is still. The minimap's
/// miniature is a bitmap of its own, painted when the nodes, their positions or the look change. The
/// regions' members and hulls are kept while the nodes stand still.
#[test]
fn still_layers_move_with_the_camera() {
    let field = function("drawCloudField");
    for part in [
        "const same = cloudKeySame(f, items) && f.look === look;",
        "if (k === 1 && ox === 0 && oy === 0) { blitCloudField(f, 1, 0, 0); return; }",
        "if (k > 0.5 && k < 2 && x0 <= 0 && y0 <= 0 && x0 + f.w * e >= innerWidth && y0 + f.h * e >= innerHeight) {",
        "f.timer = setTimeout(() => { f.due = true; requestFrame(); }, CLOUD_REST_MS);",
        "const margin = simWarm() ? 0 : CLOUD_MARGIN * Math.max(innerWidth, innerHeight);",
    ] {
        assert!(field.contains(part), "the region field no longer moves with the camera: {part}");
    }
    assert!(
        SCRIPT.contains("const CLOUD_MARGIN = 0.25;")
            && SCRIPT.contains("const CLOUD_REST_MS = 150;"),
        "the field's margin or its rest moved"
    );
    // The field's cell grows with its work, so zoomed into a large map it stays within a budget.
    assert!(
        field.contains("const cell = cloudCellFor(items, margin), m = Math.ceil(margin / cell);")
            && function("cloudCellFor").contains(
                "while (cell < CLOUD_CELL_MAX && work / (cell * cell) > CLOUD_WORK) cell *= 2;"
            )
            && SCRIPT.contains("const CLOUD_CELL_MAX = 32, CLOUD_WORK = 3e6;")
            && function("blitCloudField").contains("const e = f.cell * k;"),
        "the field's cell no longer grows with its work"
    );
    // The key holds everything the field is worked out from besides the camera.
    assert!(
        function("cloudKeySame").contains(
            "k[o] = sh.cx; k[o + 1] = sh.cy; k[o + 2] = sh.ru; k[o + 3] = sh.rv; k[o + 4] = sh.th; k[o + 5] = it.alpha; k[o + 6] = it.hue;"
        ) && field.contains("const look = cloudL() + \" \" + cloudC() + \" \" + innerWidth + \" \" + innerHeight + \" \" + edges.length;"),
        "the field's key lost an input"
    );
    let mini = function("drawMinimap");
    for part in [
        "const look = [dpr, hyperMode, showEdges, LIGHT.matches, FORCED_COLORS.matches, SURFACE, EDGE_OTHER, [...typeOff].join(\",\"),",
        "if (m.look !== look || m.nodes !== nodes || m.hyper !== hyperedges || m.gen !== positionsGen) {",
        "if (m.dirty && (!m.c || !simWarm() || performance.now() - m.at >= MINI_MOVING_MS)) {",
        "paintMiniature(src, dpr, k, ox, oy, R, round);",
        "mctx.drawImage(miniStill.c, 0, 0, MINI_W, MINI_H);",
        "drawMiniWindow(m.k, m.ox, m.oy, R, round);",
    ] {
        assert!(mini.contains(part), "the minimap paints its miniature every frame again: {part}");
    }
    // While the layout moves the miniature waits a quarter of a second between paints, and its dots
    // are drawn into one pixel buffer, not one canvas call each.
    assert!(
        SCRIPT.contains("const MINI_MOVING_MS = 250;"),
        "the minimap's pace while moving moved"
    );
    let paint = function("paintMiniature");
    assert!(
        paint.contains("mctx.drawImage(paintMiniDots(src, dpr, mx, my), 0, 0, MINI_W, MINI_H);")
            && !paint.contains("mctx.arc(mx(n)"),
        "the minimap draws each dot through the canvas again"
    );
    assert!(
        mini.contains("edges.length, hyperedges.length, Object.keys(typeColor)")
            && mini.contains("m.src = nodes.filter(n => !typeOff.has(n.type));"),
        "the miniature's key lost an input"
    );
    // The regions' members and hulls are kept while the nodes stand still; only the reach, which
    // follows the zoom, is taken again each frame.
    let geo = function("regionGeometry");
    assert!(
        geo.contains("if (r.nodes === nodes && r.hyper === hyperedges && r.filt === filt && r.gen === positionsGen) return r.list;"),
        "the regions are worked out every frame again"
    );
    let draw = function("draw");
    assert!(
        draw.contains("for (const it of regionGeometry()) {")
            && draw
                .contains("if (reach !== it.g.r) { it.g.r = reach; it.shape = cloudShape(it.g); }"),
        "the map's regions do not come from the kept geometry"
    );
}

/// U33: a poll that finds nothing new costs the page nothing. The poll asks with the ETag of the graph
/// and the hypergraph it holds, only for the address it holds them for; a 304 applies nothing and
/// still says the graph is current; a workspace change forgets the tags.
#[test]
fn a_poll_asks_with_what_it_holds() {
    let poll = function("poll");
    for part in [
        "const r = await fetch(url, askedWith(heldGraph, url));",
        "if (r.status === 304) heardBack();",
        "applyGraph(g);\n      hold(heldGraph, url, r);",
        "const hr = await fetch(hurl, askedWith(heldHyper, hurl));",
        "if (hr.status !== 304 && hr.ok) {",
    ] {
        assert!(poll.contains(part), "the poll no longer asks first: {part}");
    }
    assert!(
        function("askedWith")
            .contains("held.url === url && held.tag ? { \"If-None-Match\": held.tag } : {}"),
        "the poll asks with a tag held for another address"
    );
    assert!(
        function("beginWorkspaceTransition")
            .contains("heldGraph.url = heldGraph.tag = heldHyper.url = heldHyper.tag = \"\";"),
        "a workspace change keeps the old tags"
    );
}

/// U34: the layout's region forces grow with the regions' members, not with their pairs or their
/// number. Membership is kept as index arrays while the nodes and contexts are the same; a step
/// allocates nothing per region; the shared-member test marks one region's members and reads the
/// other's; and only regions of three or more members push apart, while every context of two or more
/// keeps its pull.
#[test]
fn region_forces_grow_with_the_members() {
    let kept = function("regionSimOf");
    for part in [
        "if (r.nodes === nodes && r.hyper === hyperedges) return r;",
        "if (mem.length - from >= 3) drawn.push(start.length - 1);",
    ] {
        assert!(kept.contains(part), "the contexts' membership is not kept: {part}");
    }
    assert!(
        function("layoutGraph").contains("regStart: rg.start, regMem: rg.mem, regDrawn: rg.drawn"),
        "the layout does not take the kept membership"
    );
    let step = engine("forces");
    for part in [
        "const H = regCount, start = regStart, mem = regMem;",
        "for (let k = start[i]; k < start[i + 1]; k++) mark[mem[k]] = i;",
        "for (let k = start[j]; k < start[j + 1] && !shared; k++) shared = mark[mem[k]] === i;",
        "for (const i of regDrawn) {",
    ] {
        assert!(step.contains(part), "the region forces grow with their pairs again: {part}");
    }
    let regions = &step[step.find("const H = regCount").expect("region forces")..];
    let regions = &regions[..regions.find("// Workspaces cohere").expect("their end")];
    for alloc in ["new Set(", ".push({", ".map(id =>", "new Array("] {
        assert!(!regions.contains(alloc), "a step allocates per region: {alloc}");
    }
    assert!(
        !SCRIPT.contains("function hullsShareMember") && !SIM.contains("function hullsShareMember"),
        "the set-based share test is back"
    );
}

/// U35: the layout steps on a thread of its own when a worker can start, and the page draws its
/// frames without waiting on a step. The page starts the worker from the served file, sends it the
/// graph with each node's previous index and its state, drops a report for a graph it has replaced,
/// and runs the same engine itself when no worker starts; the worker steps as fast as it can behind
/// the loader and at most every SHOWN_STEP_MS once the map shows.
#[test]
fn the_layout_steps_on_its_own_thread() {
    let host = &SCRIPT[SCRIPT.find("const layout = (() => {").expect("the layout's host")..];
    let host = &host[..host.find("\n})();\n").expect("it closes")];
    for part in [
        "worker = new Worker(\"/viewer-sim.js\");",
        "worker.onerror = () => { here(); requestFrame(); };",
        "} catch (e) { here(); }",
        "if (st.gen !== gen) return;",
        "engine = createLayout();",
        "tick() {\n      if (!engine) return;",
    ] {
        assert!(host.contains(part), "the layout is not off the page's thread: {part}");
    }
    let graph = function("layoutGraph");
    assert!(
        graph.contains("prev[i] = v._k === undefined ? -1 : v._k;")
            && graph.contains("for (let i = 0; i < N; i++) nodes[i]._k = i;"),
        "a node that stays does not keep the layout's position for it"
    );
    assert!(function("wake").contains("layout.wake(a);"), "a wake does not reach the layout");
    let worker =
        &SIM[SIM.find("if (typeof WorkerGlobalScope !== \"undefined\"").expect("the worker")..];
    for part in [
        "if (shown) L.step();",
        "else do { L.step(); } while (L.warm() && performance.now() - t0 < SHOWN_STEP_MS);",
        "timer = setTimeout(run, shown ? Math.max(0, SHOWN_STEP_MS - (performance.now() - t0)) : 0);",
    ] {
        assert!(worker.contains(part), "the worker does not pace itself: {part}");
    }
    assert!(SIM.contains("const SHOWN_STEP_MS = 1000 / 60;"), "the shown pace moved");
    let html = include_str!("../assets/viewer.html");
    let sim = html
        .find("<script src=\"/viewer-sim.js\"></script>")
        .expect("the page loads the layout");
    assert!(
        sim < html.find("<script src=\"/viewer.js\"></script>").expect("and the viewer"),
        "the layout loads after the viewer that uses it"
    );
}

/// U37, the Log's half: the Log lists the scope's newest observations, LOG_ROWS at a time, asking for
/// one more than it shows and with the ETag of what it holds; its rows are wired by their place, not
/// found again in the list; and an observation it does not hold is read once by id.
#[test]
fn the_log_draws_its_newest_rows() {
    assert!(SCRIPT.contains("const LOG_ROWS = 200;"), "the Log's page moved");
    let refresh = function("refreshLog");
    for part in [
        "const url = \"/api/observations?limit=\" + (logLimit + 1)",
        "const r = await fetch(url, askedWith(heldLog, url));",
    ] {
        assert!(refresh.contains(part), "the Log fetches its whole scope again: {part}");
    }
    let render = function("renderLog");
    for part in [
        "const more = obsLog.length > logLimit, shown = more ? obsLog.slice(0, logLimit) : obsLog;",
        "wireObsList(logBodyEl, shown);",
        "b.onclick = () => { logLimit += LOG_ROWS; refreshLog(); };",
    ] {
        assert!(render.contains(part), "the Log draws every row again: {part}");
    }
    let wire = function("wireObsList");
    assert!(
        wire.contains("const o = list[i];") && !wire.contains("list.find("),
        "a row is found again in the whole list to wire it"
    );
    assert!(
        function("observationById").contains("const kept = obsLog.find(x => x.id === id);"),
        "an observation the Log does not hold cannot be opened"
    );
}

/// U36: a fresh layout starts from its communities. Label propagation over the relations finds them
/// in a fixed order, the lower label on a tie; a community smaller than COMMUNITY_MIN joins the
/// neighbour it shares the most relations with; the graph of communities is laid out first, with
/// isolated nodes on a ring outside it, and each member starts on a spiral in its community's disc;
/// the full layout continues from MULTI_ALPHA. A graph most of whose nodes the layout already holds,
/// or one the page moved, does not start over.
#[test]
fn a_fresh_layout_starts_from_its_communities() {
    let lpa = &SIM[SIM.find("function communitiesOf(").expect("communitiesOf")..];
    let lpa = &lpa[..lpa.find("\n}\n").expect("it closes")];
    for part in [
        "if (c > most || (c === most && l < best)) { best = l; most = c; }",
        "if (!changed) break;",
        "for (const l of [...members.keys()].sort((a, b) => a - b)) {",
        "if (best >= 0) { into[own] = best; size[best] += size[own]; }",
    ] {
        assert!(lpa.contains(part), "the communities are not found as Section 27.6 says: {part}");
    }
    let take = engine("takeGraph");
    assert!(
        take.contains("comm = communitiesOf(N, ea, eb);")
            && take.contains("if (!g.moved && N >= MULTI_MIN && fresh >= 0.8 * N) multilevel();"),
        "a fresh layout does not start from its communities"
    );
    let multi = engine("multilevel");
    for part in [
        "if (size[of[i]] === 1 && deg[i] === 0) alone[of[i]] = 1;",
        "const c = coarseLayout(back.length, lsize, llinks, cx, cy), placed = new Float64Array(K);",
        "alpha = Math.min(alpha, MULTI_ALPHA);",
    ] {
        assert!(
            multi.contains(part),
            "the communities' layout is not carried to their members: {part}"
        );
    }
    for c in [
        "const MULTI_MIN = 200;",
        "const MULTI_ALPHA = 0.3;",
        "const COMMUNITY_MIN = 6;",
        "const COARSE_WORK = 2e7;",
    ] {
        assert!(SIM.contains(c), "the multilevel start's bounds moved: {c}");
    }
}

/// U37: a frame touches what the window holds, not every node. The scene grid counting-sorts the
/// nodes into cells and the relations into the cells of their midpoints - a long one into a list every
/// query reads - and is rebuilt only when a position has changed (positionsGen), which a report from
/// the layout that moved nothing does not say; the frame's nodes, relations, badges, pulses and names,
/// and the hit test, come from its queries; and what is kept from the positions - the regions'
/// geometry, the minimap - is keyed on positionsGen rather than read node by node.
#[test]
fn a_frame_touches_what_the_window_holds() {
    let grid = function("sceneOf");
    for part in [
        "if (g.gen === positionsGen && g.nodes === nodes && g.edges === edges) return g;",
        "if (Math.abs(e.a.x - e.b.x) > span || Math.abs(e.a.y - e.b.y) > span) { rCell[k] = -1; long.push(k); continue; }",
        "const c = at((e.a.x + e.b.x) / 2, (e.a.y + e.b.y) / 2);",
    ] {
        assert!(grid.contains(part), "the scene grid is not kept as Section 27.7 says: {part}");
    }
    assert!(
        function("nodesIn").contains("return buf.slice(0, n).sort();"),
        "the nodes come out of their drawing order"
    );
    assert!(
        function("relationsIn").contains("while (j < long.length) out[o++] = long[j++];"),
        "long relations are dropped"
    );
    for name in ["nodesIn", "relationsIn"] {
        let f = function(name);
        assert!(
            f.contains("if (sceneHeld(g, x0, y0, x1, y1)) return g.all")
                && f.contains("sceneMost(g, c)"),
            "{name} sorts the whole map when the window holds most of it"
        );
    }
    let draw = function("draw");
    for part in [
        "for (const n of vis) {\n    const on = typeHl ? n.type === typeHl",
        "for (const n of vis) {\n    if (!n.contested) continue;",
        "if (showPulses && !REDUCED_MOTION && onScreen(n)) {",
        "const inView = nodesIn(",
        "for (const k of named) {",
    ] {
        assert!(draw.contains(part), "a frame passes over every node again: {part}");
    }
    assert!(
        !draw.contains("for (const n of nodes) {\n    if (!n._vis)") && !draw.contains("n._vis"),
        "the frame marks every node again"
    );
    assert!(
        draw.contains("for (const i of relationsIn(vx0, vy0, vx1, vy1)) {"),
        "a frame passes over every relation again"
    );
    assert!(
        function("nodeAt")
            .contains("for (const k of nodesIn(wx - reach, wy - reach, wx + reach, wy + reach)) {"),
        "the hit test passes over every node again"
    );
    for (name, part) in [("applyGraph", "positionsGen++;"), ("layoutGraph", "nodes[i]._k = i;")] {
        assert!(function(name).contains(part), "{name} does not say the positions moved: {part}");
    }
    assert!(SCRIPT.contains("for (let i = 0; i < n; i++) { const v = nodes[i]; v.x = st.x[i]; v.y = st.y[i]; }\n    positionsGen++;"), "a layout report does not say the positions moved");
    assert!(
        SCRIPT.contains("if (!st.x) return;")
            && SIM.contains("if (st.gen === sentGen && st.steps === sentSteps) { self.postMessage({ kind: \"state\", ...st }); return; }"),
        "a report that moved nothing says the positions moved"
    );
}
