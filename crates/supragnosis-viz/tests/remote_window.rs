//! docs/design-system.md Section 29.31 (step 65): the viewer on a read tier - the banner that says
//! which server, as whom, and read-only; the chips of the grants, a muted one for a grant the hub may
//! not serve; the first view on a servable grant; the reasons where the map would be; a refused
//! read's own reason; and Sync and Peers absent off the local console. Source tripwires over the
//! page, its script and its stylesheet; each test names the rule it holds.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");
const CSS: &str = include_str!("../assets/viewer.css");

/// The body of a top-level function of the viewer's script, async or not.
fn function(name: &str) -> &'static str {
    let at = [format!("\nfunction {name}("), format!("\nasync function {name}(")]
        .iter()
        .find_map(|head| SCRIPT.find(head.as_str()))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at + 1..at + 1 + SCRIPT[at + 1..].find("\n}\n").expect("it closes")]
}

/// D1: under the title bar, the window's full width, a strip that does not close names the server as
/// this machine calls it, its host with the port, the principal, and read-only; Open Server shows
/// only where the shell gave its address; and the frame's top edge moves down by the strip's height.
#[test]
fn the_banner_names_the_server_and_says_read_only() {
    assert!(PAGE.contains("<div id=\"tierBar\" role=\"region\" aria-label=\"Server\" hidden>"));
    assert!(PAGE.contains("id=\"tierServer\" hidden>Open Server</button>"));
    let bar = function("renderTierBar");
    for part in [
        "new URL(String(p.url)).host",
        "\" as \" + surfaceInfo.principal",
        "who + \" - \", ro)",
        "ro.className = \"tb-ro\"; ro.textContent = \"read-only\";",
        "go.hidden = !window.supragnosisServer",
        "location.href = window.supragnosisServer",
        "TOP_INSET = 49 + (surfaceInfo ? bar.offsetHeight : 0)",
        "setProperty(\"--top\", TOP_INSET + \"px\")",
    ] {
        assert!(bar.contains(part), "the banner lacks {part}");
    }
    assert!(
        !bar.contains("dismiss")
            && !PAGE.contains("tierBar\" role=\"region\" aria-label=\"Server\" hidden><button")
    );
    assert!(function("loadSurface").contains("renderTierBar()"));
    assert!(SCRIPT.contains("let TOP_INSET = 49;"), "the inset follows the banner");
    let rule = &CSS[CSS.find("#tierBar {").expect("the banner's rule")..];
    assert!(rule.starts_with("#tierBar { position:fixed; top:49px; left:0; right:0;"));
    assert!(
        CSS.contains("#tierBar .tb-t { flex:1; min-width:0; overflow-wrap:anywhere; }"),
        "it wraps at 390"
    );
    for frame in ["#mapwin", "#listwin", "#reviewPage", ".dock", "#detail"] {
        assert!(CSS.contains(&format!("{frame} {{ position:fixed; top:var(--top);")), "{frame}");
    }
    assert!(!CSS.contains("top:49px; bottom"), "a frame still starts under the header alone");
    assert!(!CSS.contains("top:61px"), "a surface still hangs from the header alone");
}

/// D2 and U25: on a read tier the chips are the grants /api/surface lists; one the hub may not serve
/// is muted, its hover four words; and its reason, or each one's under All, is shown where the map
/// would be, with what its operator can do, and the hub's words beneath.
#[test]
fn a_grant_the_hub_may_not_serve_is_muted_and_says_why() {
    let chips = function("loadWorkspaces");
    assert!(chips.contains("let list = tierGrants() && tierGrants().map(g => String(g.name));"));
    assert!(chips.contains(
        "c.classList.add(\"unlisted\"); c.title = tipText(\"Not served by \" + serverName());"
    ));
    assert!(
        chips.contains("muted(mk(defaultName(), \"\"), defaultName())"),
        "the default chip keeps its value"
    );
    assert!(
        chips.contains("const chips = none ? [] :"),
        "a reader granted nothing has no chips"
    );
    let refusal = function("tierRefusal");
    for part in [
        "if (surfaceKind === \"local\" || !grants) return null;",
        "\" may read nothing on \" + name",
        "\"Its operator grants workspaces.\"",
        "all ? grants.filter(g => g.servable === false)",
        "name + \" may not serve \"",
        "\"Its operator can ask the machines that wrote it to share it.\"",
        "g.name + \": \" + (g.reason || \"no reason was given\")",
    ] {
        assert!(refusal.contains(part), "the refusal lacks {part}");
    }
    assert!(function("showRefusal").contains("line(t, \"e-why\")"));
    assert!(CSS.contains("#empty .e-why { font:var(--text-caption)/1.45 var(--mono);"));
    assert!(function("refreshEmpty").starts_with(
        "async function refreshEmpty() {\n  const refused = tierRefusal(currentWs());\n  if (refused) { showRefusal(refused); return; }"
    ));
    assert!(
        function("poll").contains("if (tierRefusal(ws)) { heldGraph.url = heldGraph.tag = \"\"; applyGraph({ nodes: [], edges: [], stats: {} }); return; }"),
        "a scope the tier would refuse is said, not asked for"
    );
}

/// Section 29.21 on a read tier: the first view is a grant the hub may serve - the omitted one when
/// it may, else the first that it may - and All only while the union read answers.
#[test]
fn the_first_view_is_a_grant_the_hub_may_serve() {
    let first = function("decideFirstView");
    assert!(first.contains("if (tierGrants() && unservable(defaultName())) {"));
    assert!(
        first.contains("const open = servable.find(w => held.includes(w)) || servable[0];"),
        "where the knowledge is"
    );
    assert!(
        first.contains("const r = await fetch(\"/api/graph?workspace=*\""),
        "All waits on the union"
    );
}

/// Section 29.31: a granted workspace that holds nothing says what fills it on a read tier, without
/// the console's advice to ask an AI app; the server is called by its profile's name; and a refused
/// or failed read says the answer's own reason.
#[test]
fn the_read_tier_says_its_own_words() {
    let empty = function("refreshEmpty");
    assert!(empty.contains("line(\"Nothing here yet.\", unknown ? \"e-sub\" : \"e-title\")"));
    assert!(empty.contains("\"What is written to \" + (cur && cur !== \"*\" && cur !== \"all\" ? cur : defaultName()) + \" appears here.\""));
    assert!(SCRIPT.contains("function here() { return surfaceNode ? (window.supragnosisHere || \"this machine\") : serverName(); }"));
    assert!(function("serverName").contains("p && p.name ? String(p.name) : \"this server\""));
    assert!(function("poll").contains(
        "if (!r.ok) { let why = \"\"; try { why = (await r.json()).error || \"\"; } catch (_) { why = \"\"; } sayLate(why || \"the server answered HTTP \" + r.status); return; }"
    ));
}

/// remote-viewer.md Section 6: a node's own peers and federation are not a reader's - no Peers
/// switch, no Sync tab in the viewer's own dialog, and no ask for /api/federation on a read tier.
/// Each is decided on the read tier's own answer, not on "not local", so a surface that did not
/// answer leaves the console as it was; and server mode waits on the surface, not the first graph.
#[test]
fn sync_and_peers_are_the_consoles_alone() {
    let surface = function("loadSurface");
    assert!(surface.contains("if (surfaceInfo) for (const id of [\"peersBtn\", \"dPeers\"])"));
    assert!(surface.contains("surfaceAnswered();"), "what waits on the surface is told");
    assert!(SCRIPT.contains("const surfaceReady = new Promise(r => { surfaceAnswered = r; });"));
    // Stronger than the view panel's toggle rule, which would otherwise show it.
    assert!(CSS.contains("#viewpop #peersBtn[hidden], #viewpop #dPeers[hidden] { display:none; }"));
    let detect = function("detectServerMode");
    assert!(detect.contains("await surfaceReady;\n  // A node's own federation is not a reader's"));
    assert!(detect.contains("if (surfaceInfo) return;"));
    assert!(SCRIPT.contains(
        "loadSurface().then(decideFirstView).finally(poll); connectEvents(); detectServerMode();"
    ));
    assert!(function("renderSettings")
        .contains("const tabs = SETTINGS_TABS.filter(t => t.id !== \"sync\" || !surfaceInfo);"));
    assert!(CSS.contains(
        "#log { position:fixed; left:calc(var(--win-l) + 16px); top:calc(var(--top) + 16px);"
    ));
}
