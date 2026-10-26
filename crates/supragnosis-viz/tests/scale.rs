//! docs/design-system.md Sections 26 and 27, the map at scale and at any size: tripwires over the
//! viewer's scripts for the rules that keep its cost in proportion (U28-U39). The timings themselves
//! are the scale bench's (Section 26.1, U31 and U40), which CI does not run.

const SCRIPT: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
/// The layout (Section 27.5): its constants, a node's radius, and the engine `createLayout` makes.
const SIM: &str = include_str!("../assets/viewer-sim.js");
const GL: &str = include_str!("../assets/viewer-gl.js");

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
    // A cell is never narrower than the reach it is binned for, so no pair within reach is missed.
    assert!(
        engine("binGrid").contains("cell = Math.max(cell, "),
        "a cell can shrink below the reach"
    );
    assert!(
        function("nodeById").contains("nodesById()"),
        "a lookup builds its own map of the nodes"
    );
}

/// U30: what the map draws follows the screen. A node or relation that cannot reach the window is not
/// drawn; a node's size on the screen decides its detail continuously - its halo growing in from 1.2
/// pixels of radius to 3, its ring, the arrowheads into it and the bend of its lanes from 2 to 4, a
/// square under 1 pixel on the 2D canvas - unless it is hovered, selected or active; a relation fades
/// in by its length; every fade is eased in 32 steps; relations, arrowheads, halos and fills of one
/// look are one path each; names are placed on a grid of cells with a budget per cell; and zooming
/// out stops where the whole map fits, however large.
#[test]
fn what_the_map_draws_follows_the_screen() {
    let draw = function("draw");
    // An arrowhead is a filled triangle left open: the fill closes it, and closing each of thousands
    // in one path made a frame at a reading zoom eighty times slower in Chrome.
    assert!(!draw.contains("h.closePath()"), "the arrowheads' path closes every head again");
    for part in [
        "for (const k of few || nodesIn(vx0, vy0, vx1, vy1)) {",
        "n.y > vy1))) continue;\n    n._vf = drawFrame; n._o = weightOf(n);\n    if (n._o === 0) continue;",
        "n._d = whole1 ? 1 : stepped(ease(HALO_FROM, HALO_TO, rs));",
        "for (const i of few ? relationsOfNodes(few) : relationsIn(vx0, vy0, vx1, vy1)) {",
        "Math.max(e.a.x, e.b.x) + LANE_MAX < vx0",
    ] {
        assert!(draw.contains(part), "the frame draws what is off the screen: {part}");
    }
    for part in [
        "const lit1 = n => n === hover || n === focus || (act !== null && act.ns.has(n.id));",
        "n._f = whole1 ? 1 : stepped(ease(FULL_FROM, FULL_TO, rs));",
        "n._sq = !whole1 && bulk === ctx && rs < SQUARE_PX;",
        "const lengthA = hot ? 1 : stepped(ease(EDGE_FROM, EDGE_TO, d * cam.s));",
        "const full = hot ? 1 : fullNow(e.b);",
        "const lane = full > 0 ? (lst.indexOf(i) - (lst.length - 1) / 2) * full : 0;",
        "const alen = full * Math.min(",
        "const r = rOf(n) + nodeStrokeW(n) * n._d / 2;",
        "const inner = Math.max(0.5, r - nodeStrokeW(n) * n._d / 2);",
        "bulk.globalAlpha = stepped(base * n._f);",
        "if (n._sq) { f.rect(",
    ] {
        assert!(draw.contains(part), "a node's detail steps with its size: {part}");
    }
    for part in [
        "bulk.stroke(l.line);",
        "bulk.fill(l.heads);",
        "bulk.fill(halo);",
        "bulk.fill(f);",
    ] {
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
    let mini = function("drawMinimap");
    for part in [
        "if (m.look !== look || m.nodes !== nodes || m.hyper !== hyperedges || m.gen !== positionsGen) {",
        "if (m.dirty && (!m.c || !simWarm() || performance.now() - m.at >= MINI_MOVING_MS)) {",
        "paintMiniature(dpr, k, ox, oy, R, round);",
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
    // The miniature keeps nodes as the far map does (Section 27.8), so it is painted whole while the
    // layout moves too: there is no quick one.
    assert!(!SCRIPT.contains("m.quick"), "a moving map's miniature is quick again");
    let paint = function("paintMiniature");
    assert!(
        paint.contains(
            "mctx.drawImage(paintMiniDots(dots, dpr, mx, my, weight), 0, 0, MINI_W, MINI_H);"
        ) && !paint.contains("mctx.arc(mx(n)"),
        "the minimap draws each dot through the canvas again"
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
    let html = include_str!("../assets/viewer.html");
    let sim = html
        .find("<script src=\"/viewer-sim.js\"></script>")
        .expect("the page loads the layout");
    assert!(
        sim < html.find("<script src=\"/viewer.js\"></script>").expect("and the viewer"),
        "the layout loads after the viewer that uses it"
    );
    // A held node is where the pointer is: a report does not write back where the layout last heard
    // it was, and where it is let go reaches the layout before it is let go. Writing reports over it
    // lost the node in WebKit - 129 px behind the pointer after 15 moves.
    assert!(
        SCRIPT.contains(
            "const v = nodes[i]; if (v === drag) continue; v.x = st.x[i]; v.y = st.y[i];"
        ),
        "a layout report pulls the held node back"
    );
    let release =
        &SCRIPT[SCRIPT.find("addEventListener(\"mouseup\", ev => {").expect("the release")..];
    let sync = release
        .find("layoutSync();")
        .expect("the release tells the layout where the node is");
    // The release's own let-go, not the one that drops a gesture spanning a reload.
    let let_go = release.find("\n  drag = null;\n").expect("the release lets go");
    assert!(sync < let_go, "the node is let go before the layout hears where");
}

/// U37, the Log's half: the Log lists the scope's newest observations, LOG_ROWS at a time, asking
/// with the ETag of what it holds; its rows are wired by their place, not found again in the list;
/// and an observation it does not hold is read once by id. That a limited read answers the newest
/// rows first is the server's (`the_log_answers_its_newest_rows_first`).
#[test]
fn the_log_draws_its_newest_rows() {
    assert!(SCRIPT.contains("const LOG_ROWS = 200;"), "the Log's page moved");
    assert!(
        function("refreshLog").contains("const r = await fetch(url, askedWith(heldLog, url));"),
        "the Log fetches its whole scope again"
    );
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
/// isolated nodes on a spiral through its extent, and each member starts on a spiral in its community's disc;
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
    // Started at the density it settles to, the map settles smaller than at the reveal, so the view
    // fitted then is fitted again when the layout first rests, unless the person moved it.
    assert!(
        SCRIPT.contains("cam.s = camT.s; cam.x = camT.x; cam.y = camT.y; refitAtRest = true; }")
            && function("draw").contains("if (refitAtRest && !simWarm()) { refitAtRest = false; if (!userMoved) fitView(); }"),
        "the view fitted at the reveal is not fitted again at rest"
    );
    for c in ["const COARSE_SPACING = 40;", "const COARSE_WORK = 2e7;"] {
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
        draw.contains(
            "for (const i of few ? relationsOfNodes(few) : relationsIn(vx0, vy0, vx1, vy1)) {"
        ),
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
    assert!(SCRIPT.contains("for (let i = 0; i < n; i++) { const v = nodes[i]; if (v === drag) continue; v.x = st.x[i]; v.y = st.y[i]; }\n    positionsGen++;"), "a layout report does not say the positions moved");
    assert!(
        SCRIPT.contains("if (!st.x) return;")
            && SIM.contains("if (st.gen === sentGen && st.steps === sentSteps) { self.postMessage({ kind: \"state\", ...st }); return; }"),
        "a report that moved nothing says the positions moved"
    );
}

/// U38: zoomed out, the map keeps its shape by keeping its best-connected nodes where they are. It
/// keeps whole about one node for each 12 by 12 pixels of the screen it covers - the nodes' mean
/// spacing on the screen, from their mean distance from their centre, against 12 pixels, not bounded
/// by their number - and the next as many fade out, the least-connected first, eased in 32 steps; a
/// faded node is clipped: not drawn, named or under the pointer, and a frame that keeps fewer nodes
/// than its window holds visits only those and their relations; a node kept is at least 1.6 pixels in
/// radius; a hovered, selected, active, pulsing or contested node never fades; no node stands for
/// another - no group, haze or count; and the minimap keeps one for each 5 by 5 of its pixels by the
/// same rule, with only its 48 widest regions of at least 12 pixels.
#[test]
fn a_far_map_keeps_its_shape() {
    let ranks = function("ranksOf");
    for part in [
        "for (const i of degreeOrder()) {",
        "r.spacing = r.count > 1 ? Math.max(1, 1.5 * md * Math.sqrt(Math.PI / r.count)) : 1;",
    ] {
        assert!(
            ranks.contains(part),
            "the nodes are not ranked and spaced as Section 27.8 says: {part}"
        );
    }
    assert!(
        function("degreeOrder")
            .contains(".sort((p, q) => (nodes[q].degree || 0) - (nodes[p].degree || 0) || p - q);"),
        "the rank is not by relations"
    );
    assert!(
        SCRIPT.contains("function keptAt(r, s, px) { return r.count * (r.spacing * s / px) ** 2; }")
            && SCRIPT.contains(
                "function keptWeight(rank, c) { return rank < 0 || rank >= 2 * c ? 0 : rank < c ? 1 : stepped(1 - ease(c, 2 * c, rank)); }"
            ),
        "the map keeps or fades nodes by another rule"
    );
    let draw = function("draw");
    for part in [
        "const R = ranksOf(), keep = keptAt(R, cam.s, KEEP_PX), fading = keep < R.count;",
        "const never = n => n === hover || n === focus || (act !== null && act.ns.has(n.id)) || n.contested || pulses.has(n.id);",
        "const weightOf = n => (!fading || never(n) ? 1 : keptWeight(R.of[n._k], keep));",
        "const few = fading && 2 * keep < (vx1 - vx0) * (vy1 - vy0) / R.spacing ** 2 ? keptNodes(R, 2 * keep, act) : null;",
        "n._hr = nodeRadius(n) * cam.s < KEEP_MIN_PX ? KEEP_MIN_PX / cam.s : 0;",
        "if (oa === 0 || ob === 0) continue;   // an end faded out, and the relation with it",
        "const named = few || nodesIn(",
    ] {
        assert!(draw.contains(part), "the far map does not keep its shape as Section 27.8 says: {part}");
    }
    let kept = function("keptNodes");
    assert!(
        kept.contains("add(hover); add(focus);")
            && kept.contains("for (const id of pulses.keys()) add(nodeById(id));")
            && kept.contains("for (const i of r.contested) add(nodes[i]);"),
        "a far frame leaves out a node that never fades"
    );
    assert!(
        function("nodeAt").contains(
            "if (typeOff.has(n.type) || (onScreen(n) ? n._o === 0 : frameKept)) continue;"
        ) && function("nodeAt")
            .contains("if (d < Math.max(nodeRadius(n), KEEP_MIN_PX / cam.s) + 6 && d < bd) { bd = d; best = n; }"),
        "a faded node is still under the pointer, or a kept one is not where it is drawn"
    );
    // The minimap keeps by the same rule at its own scale, and draws only its widest regions.
    let paint = function("paintMiniature");
    for part in [
        "const r = ranksOf(), keep = keptAt(r, k, MINI_KEEP_PX), weight = n => keptWeight(r.of[n._k], keep);",
        "for (let j = Math.min(r.count, Math.ceil(2 * keep)) - 1; j >= 0; j--) dots.push(nodes[r.order[j]]);",
        "const a = Math.min(weight(e.a), weight(e.b));",
        "if (n >= 3 && span >= MINI_REGION_PX) wide.push([span, h]);",
        "for (const [, h] of wide.slice(0, MINI_REGIONS)) {",
    ] {
        assert!(paint.contains(part), "the minimap does not keep nodes as the map does: {part}");
    }
}

/// U39: where WebGL2 renders to half-float targets, the GPU draws the map's bulk under a 2D canvas of
/// text and badges. The renderer starts only with half-float rendering and gives way when its context
/// is lost; the page draws the bulk with whichever renderer holds - the 2D one under forced colors -
/// through the same calls; the field is added up in a half-float target; and the page loads the
/// renderer before viewer.js, with its canvas under the map's.
#[test]
fn the_gpu_draws_the_bulk() {
    for part in [
        "if (!gl || !gl.getExtension(\"EXT_color_buffer_float\")) return null;",
        "canvas.addEventListener(\"webglcontextlost\", ev => { ev.preventDefault(); lost = true; onLost(); });",
    ] {
        assert!(GL.contains(part), "the GPU renderer is not built as Section 27.9 says: {part}");
    }
    assert!(
        SCRIPT.contains("bulk = gpu && !FORCED_COLORS.matches ? gpu : ctx;")
            && SCRIPT.contains("if (bulk === gpu) gpu.end();"),
        "the page does not choose its renderer per frame"
    );
    assert!(
        function("drawCloudField").contains("if (bulk !== ctx) { bulk.field(items,"),
        "the field is worked out on the processor under the GPU"
    );
    let draw = function("draw");
    for part in [
        "drawWsHulls(bulk, hullLabels);",
        "bulk.stroke(l.line);",
        "bulk.fillStyle = SURFACE; bulk.fill(halo);",
    ] {
        assert!(draw.contains(part), "the bulk is drawn on the 2D canvas alone: {part}");
    }
    assert!(
        function("trustRing").contains("bulk.stroke();"),
        "a trust ring is drawn on the 2D canvas alone"
    );
    let html = include_str!("../assets/viewer.html");
    let (gl, sim, viewer) = (
        html.find("<script src=\"/viewer-gl.js\"></script>")
            .expect("viewer-gl.js is not loaded"),
        html.find("<script src=\"/viewer-sim.js\"></script>")
            .expect("viewer-sim.js is not loaded"),
        html.find("<script src=\"/viewer.js\"></script>")
            .expect("viewer.js is not loaded"),
    );
    assert!(sim < gl && gl < viewer, "the renderer does not load before viewer.js");
    assert!(
        html.find("<canvas id=\"gl\"").expect("no GPU canvas")
            < html.find("<canvas id=\"c\"").expect("no map canvas"),
        "the GPU's canvas is not under the map's"
    );
}
