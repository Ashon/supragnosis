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
