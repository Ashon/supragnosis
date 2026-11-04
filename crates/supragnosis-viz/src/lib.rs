//! supragnosis-viz - live ontology visualization (local unix-socket viewer).
//!
//! A **human-facing read channel**, distinct from the MCP tool surface (Principle 21). It rides
//! inside the server process and shares the same `Arc<Engine>` (the embedded store is single-process
//! constraint), so it exposes the `engine.graph()` projection directly, without the lock conflict
//! that opening the db from a separate process would cause.
//!
//! - `GET /` -> self-contained canvas viewer (0 external CDNs). Polls `/api/graph` every few seconds to refresh.
//! - `GET /api/graph[?workspace=<ws>]` -> `engine.graph(ws)` JSON (Principle 16: deterministic ordering).
//!
//! It speaks HTTP over a **unix domain socket**, never TCP (Principle 17: knowledge sovereignty).
//! The socket file (0600, inside the 0700 `~/.supragnosis` dir) is the whole access control: only
//! the owning user can connect, so every request is attributable to the local principal (F19), and
//! the browser-borne attack classes a localhost port invites (DNS rebinding, CSRF, cross-site
//! fetch) cannot reach a unix socket at all. Clients are the desktop shell, or any HTTP-over-UDS
//! client (e.g. `curl --unix-socket`). A hub's read tier for principals served this page over the
//! network until 0.11.0 removed it (remote-server.md Section 13).

use std::sync::Arc;

mod fonts {
    include!(concat!(env!("OUT_DIR"), "/fonts.rs"));
}

mod icons {
    include!(concat!(env!("OUT_DIR"), "/icons.rs"));
}

/// A file the page ships with it - the faces under `/fonts/` (docs/design-system.md Section 4,
/// U11) and the icons under `/icons/` (Section 28, U41) - as its content type and bytes.
pub fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    fonts::FONTS
        .iter()
        .chain(icons::ICONS)
        .find(|(p, _, _)| *p == path)
        .map(|(_, kind, bytes)| (*kind, *bytes))
}

use supragnosis_engine::{Engine, EventEnvelope, EventSink};
use tokio::sync::broadcast;

mod act_api;
mod read_api;
mod routes;
mod server;

use act_api::{opened_response, write_workspace};
use routes::{err_body, Response};
use server::percent_decode;
pub use server::{bind_uds, serve, ASSET_CACHE, CSP};

/// [`EventSink`] adapter that streams MCP events to the browser (SSE). Once attached to the engine,
/// tool calls are published here, and `/api/events` SSE connections subscribe via broadcast.
/// With no receivers (no open viewer), send is dropped - observability is optional (the spirit of Principle 19).
pub struct BroadcastSink {
    tx: broadcast::Sender<String>,
}

impl BroadcastSink {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self { tx }
    }
}

impl EventSink for BroadcastSink {
    fn emit(&self, env: &EventEnvelope) {
        // Called from a synchronous context (tool handler) - send is non-blocking. A serialization
        // failure or missing receiver is dropped silently (tool behavior must be unaffected even
        // when no viewer is open).
        if let Ok(json) = serde_json::to_string(env) {
            let _ = self.tx.send(json);
        }
    }
}

/// Accepts connections on the injected listener and serves the viewer/graph API (infinite accept loop).
///
/// Binding is done by **the caller** (so a test can bind port 0 and look up the actual port).
/// Each connection is split off into a task, but an individual connection failure is swallowed
/// so it does not kill the server.
/// Live federation status blob, maintained by the wiring layer (the CLI's status task) and served
/// verbatim at /api/federation - the viz stays decoupled from the sync crate (it renders JSON).
pub type FedStatus = Arc<std::sync::RwLock<serde_json::Value>>;

/// Narrows one peer's shared workspaces: `(node_id, keep) -> Ok(now granted) | Err(reason)`.
///
/// Injected by the wiring layer rather than implemented here, for the same reason [`FedStatus`] is a
/// JSON blob: this crate does not depend on the sync crate or know what a config file is. It renders
/// and routes; the policy - narrow-only, re-read before write, update the live directory - lives
/// where the config and the admission directory already do.
pub type NarrowShare = Arc<dyn Fn(&str, &[String]) -> Result<Vec<String>, String> + Send + Sync>;

/// First contact (docs/onboarding.md Section 5): which AI apps have talked to this daemon since it
/// started, as `{"clients": [...]}`. Read from the MCP server's log by the wiring layer; served on
/// this local socket only (O7).
pub type ClientsView = Arc<dyn Fn() -> serde_json::Value + Send + Sync>;

/// `/api/resolve?observation=<id>[&observation=<id>...]&tier=<tier>[&kind=claim_demotion][&workspace=<ws>][&rationale=<text>]` -
/// mediate a contested belief from the curation console (resolution.md Section 4.2): opens a
/// claim_promotion for the chosen observation(s) at the requested tier - or, with
/// `kind=claim_demotion`, the demotion that undoes one (design-system.md Section 7) - and immediately
/// casts the Console merge verdict. Both steps are gated appended events (I1/I18) - the projection
/// changes only because the fold consumes the verdict, never by a direct write. Solo self-approval is
/// the P23 exception, and the verdict stays self-attested.
///
/// The act is filed in the workspace of the observations it changes, whatever scope the console
/// shows: from All the query says `*`, which would file it in the node default, where the fold that
/// reads the observations' own workspace never sees it.
/// The settle preview (design-system.md Section 29.2): the query `/api/resolve` takes - the
/// observations, the tier and the kind - and once more with the suffix `2` for a second act, which a
/// "Confirm instead" and its Undo need. Answers the acts' belief diff and checks, applied in order at
/// the console's grant, and writes nothing. Observations from two workspaces are refused, as
/// `/api/resolve` refuses them, since the fold that would read the act reads one workspace.
/// The undo read (design-system.md Section 29.3): `?proposal=<id>` names a merged claim promotion or
/// demotion, and the answer is the acts that reverse it - each target back to the tier it has with
/// that proposal's merge verdicts left out - with their diff and checks. Read in the proposal's own
/// workspace, the one its opening observation was written to. Writes nothing.
fn resolve_undo_response(engine: &Engine, query: &str) -> Response {
    let Some(id) = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("proposal="))
        .map(percent_decode)
        .filter(|s| !s.is_empty())
    else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(
                "undo needs ?proposal=<id> - a merged claim_promotion or claim_demotion",
            ),
        };
    };
    let fail = |e: &dyn std::fmt::Display| Response {
        status: "500 Internal Server Error",
        content_type: "application/json",
        body: err_body(&e.to_string()),
    };
    let workspace = match engine.get_observation(&id) {
        Ok(o) => o.and_then(|o| o.provenance.first().map(|p| p.workspace.clone())),
        Err(e) => return fail(&e),
    };
    match engine.undo_gate(workspace.as_deref(), &id) {
        Ok(Some(u)) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::to_string(&u).unwrap_or_else(|e| err_body(&e.to_string())),
        },
        Ok(None) => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(&format!(
                "{id} is not a merged claim_promotion or claim_demotion - there is nothing to undo"
            )),
        },
        Err(e) => fail(&e),
    }
}

fn resolve_preview_response(engine: &Engine, query: &str) -> Response {
    let bad = |m: String| Response {
        status: "400 Bad Request",
        content_type: "application/json",
        body: err_body(&m),
    };
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let mut acts = Vec::new();
    for suffix in ["", "2"] {
        let observations: Vec<String> = query
            .split('&')
            .filter_map(|kv| kv.strip_prefix(&format!("observation{suffix}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
            .collect();
        if observations.is_empty() {
            if suffix.is_empty() {
                return bad("preview needs at least one ?observation=<id>, with the tier and kind                             /api/resolve would take"
                    .into());
            }
            continue;
        }
        let Some(tier) = param(&format!("tier{suffix}")).as_deref().and_then(parse_tier_param)
        else {
            return bad(format!(
                "preview needs ?tier{suffix}=<unverified|agent_extracted|host_signed|human_confirmed>"
            ));
        };
        let kind = match param(&format!("kind{suffix}")).as_deref() {
            None | Some("claim_promotion") => "claim_promotion",
            Some("claim_demotion") => "claim_demotion",
            Some(other) => {
                return bad(format!(
                    "resolve casts claim_promotion or claim_demotion, not '{other}'"
                ))
            }
        };
        acts.push(supragnosis_engine::GateAct { kind: kind.into(), targets: observations, tier });
    }
    let mut homes = std::collections::BTreeSet::new();
    for id in acts.iter().flat_map(|a| a.targets.iter()) {
        match engine.get_observation(id) {
            Ok(Some(o)) => homes.extend(o.provenance.first().map(|p| p.workspace.clone())),
            Ok(None) => {}
            Err(e) => {
                return Response {
                    status: "500 Internal Server Error",
                    content_type: "application/json",
                    body: err_body(&e.to_string()),
                }
            }
        }
    }
    if homes.len() > 1 {
        return bad(format!(
            "the observations are in different workspaces ({}) - resolve them one workspace at a time",
            homes.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let workspace = homes.into_iter().next().or_else(|| write_workspace(param("workspace")));
    match engine.preview_gate(workspace.as_deref(), &acts) {
        Ok(p) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::to_string(&p).unwrap_or_else(|e| err_body(&e.to_string())),
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// A tier as `/api/resolve`'s query spells it.
fn parse_tier_param(t: &str) -> Option<supragnosis_engine::TrustTier> {
    serde_json::from_value(serde_json::Value::String(t.to_string())).ok()
}

fn resolve_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let observations: Vec<String> = query
        .split('&')
        .filter_map(|kv| kv.strip_prefix("observation="))
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(tier) = param("tier") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("resolve needs ?observation=<id>&tier=<unverified|agent_extracted|host_signed|human_confirmed>"),
        };
    };
    if observations.is_empty() {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("resolve needs at least one ?observation=<id> (the asserting observation of the value you confirm)"),
        };
    }
    let kind = match param("kind").as_deref() {
        None | Some("claim_promotion") => "claim_promotion",
        Some("claim_demotion") => "claim_demotion",
        Some(other) => {
            return Response {
                status: "400 Bad Request",
                content_type: "application/json",
                body: err_body(&format!(
                    "resolve casts claim_promotion or claim_demotion, not '{other}'"
                )),
            }
        }
    };
    let mut homes = std::collections::BTreeSet::new();
    for id in &observations {
        match engine.get_observation(id) {
            Ok(Some(o)) => homes.extend(o.provenance.first().map(|p| p.workspace.clone())),
            // An observation that is not here is refused by propose, with its own reason.
            Ok(None) => {}
            Err(e) => {
                return Response {
                    status: "500 Internal Server Error",
                    content_type: "application/json",
                    body: err_body(&e.to_string()),
                }
            }
        }
    }
    if homes.len() > 1 {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&format!(
                "the observations are in different workspaces ({}) - resolve them one workspace at a time",
                homes.into_iter().collect::<Vec<_>>().join(", ")
            )),
        };
    }
    let workspace = homes.into_iter().next().or_else(|| write_workspace(param("workspace")));
    let rationale = param("rationale");
    // A person's act, asked again when the act it would land on was superseded - Confirm after Undo
    // confirms - and otherwise said as found (design-system.md Section 29.23). Opened without the
    // console's mark, as before; its verdict carries it.
    let opened = match engine.propose_anew(
        supragnosis_engine::ProposeInput {
            workspace: workspace.clone(),
            kind: kind.into(),
            targets: observations,
            into: None,
            tier: Some(tier),
            rationale: rationale.or_else(|| {
                Some("confirmed from the curation console (contested belief mediation)".into())
            }),
            affected_types: Vec::new(),
            source_ref: None,
            on_behalf_of: None,
        },
        false,
    ) {
        Ok(opened) => opened,
        Err(e) => {
            return Response {
                status: "400 Bad Request",
                content_type: "application/json",
                body: err_body(&e.to_string()),
            }
        }
    };
    let proposal = opened.id.clone();
    // Found in force: the act is already so, and nothing is cast. Found Blocked: refused with its
    // check, so a two-act Confirm instead stops at its first act. Found open: the verdict is the act.
    match opened.state.as_str() {
        "merged" if opened.found => return opened_response(&opened),
        "blocked" if opened.found => {
            let why = engine
                .get_proposal(workspace.as_deref(), &proposal)
                .ok()
                .flatten()
                .and_then(|v| {
                    v.checks.into_iter().find(|c| c.blocking && !c.passed).map(|c| c.detail)
                })
                .unwrap_or_else(|| "a check holds it back".into());
            return Response {
                status: "409 Conflict",
                content_type: "application/json",
                body: err_body(&format!(
                    "this act was accepted before, but a check holds it back: {why}"
                )),
            };
        }
        _ => {}
    }
    match engine.review_proposal(
        workspace,
        proposal.clone(),
        "merge".into(),
        None,
        None,
        supragnosis_engine::VerdictSurface::Console,
    ) {
        Ok(verdict) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({ "proposal_id": proposal, "verdict_observation_id": verdict })
                .to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            // The proposal was opened but the verdict failed - report both so the user can review it
            // by hand from the proposals panel (the proposal itself commits nothing, P23).
            body: serde_json::json!({ "error": e.to_string(), "proposal_id": proposal })
                .to_string(),
        },
    }
}

/// Where a page whose address names no scope opens (design-system.md Section 29.21), from the
/// node's entities: `"all"` when other workspaces hold some and the default holds at most half of
/// them - nothing included - and `"default"` otherwise.
pub fn first_view(default_count: usize, total: usize) -> &'static str {
    if total > default_count && default_count * 2 <= total {
        "all"
    } else {
        "default"
    }
}

/// Self-contained live viewer (0 external CDNs). A canvas graph explorer: zoom/pan, hover neighbor
/// highlight, click focus/pin, search, fit-to-view, type-legend filter, label thinning. Colors come
/// from the dataviz skill's validated dark categorical palette (fixed order, "other" from the 9th
/// onward instead of cycling). alpha cooling + radius-based collision separation prevent overlap.
/// It polls `/api/graph` periodically for live refresh, and keeps node positions across polls by id
/// so the view does not jump.
// Embedded by build.rs from assets/ - verbatim in debug, minified in release (see build.rs).
const VIEWER_HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.html"));
const VIEWER_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.css"));
const VIEWER_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer.js"));
const VIEWER_SIM_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer-sim.js"));
const VIEWER_GL_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/viewer-gl.js"));

#[cfg(test)]
mod tests {

    /// The desktop shell decorates this page from outside (app/assets/shell-init.js): the header
    /// becomes the title bar and its drag region, the status bar's gear opens the app's Settings
    /// page, and the event stream arrives through a stand-in for EventSource that feeds `onmessage`
    /// and nothing else. The shell ships in the app and this page in the daemon, and the two can be
    /// a release apart, so what the shell finds here is this page's contract with it
    /// (docs/compatibility.md Section 8). Changing one of these needs the shell changed too, and an
    /// app from before that change will leave the page undecorated rather than half done.
    #[test]
    fn the_page_keeps_what_the_desktop_shell_finds() {
        let html = include_str!("../assets/viewer.html");
        for (needle, what) in [
            ("<header", "the header the shell makes its title bar"),
            ("<h1", "the title it makes part of the drag region"),
            (r#"id="settingsBtn""#, "the gear it routes to the app's Settings page"),
            (r#"id="syncPill""#, "the sync pill it routes to Settings > Sync"),
            // The menu bar's map commands press these (design-system.md Section 29.13).
            (r#"id="panelL""#, "the filters panel's toggle View > Filters Panel presses"),
            (r#"id="panelR""#, "the log panel's toggle View > Log Panel presses"),
            (r#"id="mapTab""#, "the Map tab View > as Map presses"),
            (r#"id="listBtn""#, "the List tab View > as List presses"),
            (r#"id="fit""#, "the Fit button View > Fit to View presses"),
            (r#"id="keysOpen""#, "the Keys... button View > Keys... presses"),
            (
                r#"id="keysDlg""#,
                "the Keys dialog that marks a viewer with the selection history",
            ),
        ] {
            assert!(html.contains(needle), "viewer.html lost {what} ({needle})");
        }
        let js = include_str!(concat!(env!("OUT_DIR"), "/viewer.src.js"));
        assert!(
            js.contains(r#"new EventSource("/api/events")"#) && js.contains("es.onmessage ="),
            "the shell's EventSource stand-in serves /api/events through onmessage only"
        );
        // The keys the menu bar sends: Cmd+K opens search, Cmd+[ and Cmd+] step the selection.
        assert!(
            js.contains(r#"e.key.toLowerCase() === "k""#)
                && js.contains("stepSelection(e.key === \"[\" ? -1 : 1);")
        );
        assert!(
            html.contains(r#"aria-pressed="true" title="Filters panel ([)""#)
                || js.contains("name(\"panelL\", \"Filters panel\", \"[\")")
        );
    }
}
