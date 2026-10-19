//! docs/design-system.md Section 26, the map at scale: tripwires over the viewer's script for the
//! rules that keep its cost in proportion (U28-U30). The timings themselves are the scale bench's
//! (Section 26.1, U31), which CI does not run.

const SCRIPT: &str = include_str!("../assets/viewer.js");

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// U28: drawing never moves the layout. A frame steps the simulation only while it is warm -
/// settling, applying force, a node held, the settle's last collision pass owed, or still coasting
/// on the screen - so a pan, a zoom or a hover only redraws. Once cooled, the pairs are visited only
/// for the collision pass the cooling owes, which ends as soon as nothing overlaps.
#[test]
fn drawing_never_moves_the_layout() {
    let draw = function("draw");
    let calls: Vec<usize> = draw.match_indices("stepSim()").map(|(i, _)| i).collect();
    assert!(!calls.is_empty(), "draw no longer steps the layout at all");
    for at in calls {
        let line = &draw[draw[..at].rfind('\n').map_or(0, |p| p + 1)..at];
        assert!(
            line.contains("if (simWarm())") || line.contains("settling && REDUCED_MOTION"),
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
    let step = function("stepSim");
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
            "if (!active && overlapOwed === 0 && !drag && simMotion * cam.s <= MOTION_MIN_PX) {"
        ) && step.contains("for (const v of nodes) { v.vx = 0; v.vy = 0; }\n    simMotion = 0;"),
        "a layout at rest keeps a coast a zoom can wake"
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
    let step = function("stepSim");
    assert!(
        !step.contains("for (let j = i + 1; j < N; j++)")
            && !step.contains("for (let j = i + 1; j < hgs.length; j++)"),
        "the layout visits every pair again"
    );
    for part in [
        "if (pairs) binGrid(N, active ? Math.max(DENSITY_R, 2 * NODE_R_MAX + COLLIDE_PAD, range / FAR_SPLIT) : reach);",
        "const FORWARD = [[1, 0], [-1, 1], [0, 1], [1, 1]];",
        "if (dx * dx + dy * dy >= reach2) continue;",
        "const i = gi < gj ? gi : gj, j = gi < gj ? gj : gi;",
        "if (ox >= -1 && ox <= 1 && oy >= -1 && oy <= 1) continue;",
        "const count = same + (n - same) * WS_CROSS_REPULSE;",
        "const RC = 2 * HULL_R_CAP + HULL_PAD",
        "if (d >= want || hullsShareMember(a.ids, b.ids)) continue;",
    ] {
        assert!(step.contains(part), "the grids' pairing changed: {part}");
    }
    assert!(SCRIPT.contains("const FAR_SPLIT = 6;"), "the far field's cells moved");
    // A cell is never narrower than the reach it is binned for, so no pair within reach is missed.
    assert!(
        function("binGrid").contains("cell = Math.max(cell, "),
        "a cell can shrink below the reach"
    );
    // The step's scratch and its lookups are kept between steps.
    assert!(
        !step.contains("new Array(N)") && !step.contains("new Float32Array(N)"),
        "the step allocates per-node scratch again"
    );
    assert!(
        !step.contains("new Map(nodes.map(") && function("nodeById").contains("nodesById()"),
        "a step or a lookup builds its own map of the nodes"
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
        "n._vis = !typeOff.has(n.type) && n.x >= vx0 && n.x <= vx1 && n.y >= vy0 && n.y <= vy1;",
        "Math.max(e.a.x, e.b.x) + LANE_MAX < vx0",
        "if (!n._vis) continue;",
        "if (!n.contested || !n._vis) continue;",
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
        "const full = hot || e.b._lod === LOD_FULL_TIER;",
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
        "if (!miniStillSame(src, look)) paintMiniature(src, dpr, k, ox, oy, R, round);",
        "mctx.drawImage(miniStill.c, 0, 0, MINI_W, MINI_H);",
    ] {
        assert!(mini.contains(part), "the minimap paints its miniature every frame again: {part}");
    }
    assert!(
        function("miniStillSame").contains(
            "if (same && (xy[3 * i] !== n.x || xy[3 * i + 1] !== n.y || xy[3 * i + 2] !== d || ty[i] !== n.type)) same = false;"
        ) && mini.contains("edges.length, hyperedges.length, Object.keys(typeColor)"),
        "the miniature's key lost an input"
    );
    // The regions' members and hulls are kept while the nodes stand still; only the reach, which
    // follows the zoom, is taken again each frame.
    let geo = function("regionGeometry");
    for part in [
        "let same = r.nodes === nodes && r.hyper === hyperedges && r.filt === filt && r.xy !== null && r.xy.length === 2 * N;",
        "if (same && (xy[2 * i] !== v.x || xy[2 * i + 1] !== v.y)) same = false;",
        "if (same) return r.list;",
    ] {
        assert!(geo.contains(part), "the regions are worked out every frame again: {part}");
    }
    let draw = function("draw");
    assert!(
        draw.contains("for (const it of regionGeometry()) {")
            && draw
                .contains("if (reach !== it.g.r) { it.g.r = reach; it.shape = cloudShape(it.g); }"),
        "the map's regions do not come from the kept geometry"
    );
}
