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

mod read_api;
mod routes;
mod server;

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

/// A workspace to WRITE into. The read endpoints resolve `*`/`all`/empty to "every workspace" - a
/// VIEW, not a name, and a view is not somewhere an observation can be filed. Forwarding the
/// sentinel would create a workspace literally called `*` and quietly strand everything written
/// there. None means the node default, which is what an unspecified workspace has always meant on
/// the write path. Read and write must not disagree about what `*` is (Principle 17).
fn write_workspace(raw: Option<String>) -> Option<String> {
    raw.filter(|s| !is_all(s))
}

/// Whether a workspace parameter names the all-workspaces view rather than a workspace.
fn is_all(ws: &str) -> bool {
    matches!(ws.trim(), "" | "*" | "all")
}

/// A storage failure while deciding where an act is filed: a 500, never a decision to file in the
/// node default (P5).
fn store_failure(e: &impl std::fmt::Display) -> Response {
    Response {
        status: "500 Internal Server Error",
        content_type: "application/json",
        body: err_body(&format!("storage backend failure: {e}")),
    }
}

/// The workspace an entity lives in, read off its row: the one its id was minted in.
fn entity_home(engine: &Engine, id: &str) -> Result<Option<String>, Response> {
    engine
        .store()
        .get_entity(id)
        .map(|e| e.and_then(|e| e.provenance.first().map(|p| p.workspace.clone())))
        .map_err(|e| store_failure(&e))
}

/// The workspace a proposal lives in.
fn proposal_home(engine: &Engine, id: &str) -> Result<Option<String>, Response> {
    engine.proposal_workspace(id).map_err(|e| store_failure(&e))
}

/// The workspace a console act is filed in. All is a view and no place to file anything
/// (design-system.md Section 29.15), so an act taken under All is filed where its item lives, which
/// `home` reads; a named workspace is filed as named, and the engine refuses it where the item does
/// not live. `None` is the node default, as an omitted workspace has always meant here.
fn act_workspace(
    raw: Option<String>,
    home: impl FnOnce() -> Result<Option<String>, Response>,
) -> Result<Option<String>, Response> {
    match raw {
        Some(w) if is_all(&w) => home(),
        other => Ok(other),
    }
}

/// `/api/review?proposal=<id>&decision=merge|reject|withdraw[&workspace=<ws>]` - cast a verdict from the
/// curation console. Goes through the gated verdict path (engine.review_proposal appends a verdict
/// observation, the fold decides) - never a direct projection/log write (I18). Self-attested (solo).
fn review_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let (Some(proposal), Some(decision)) = (param("proposal"), param("decision")) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("review needs ?proposal=<id>&decision=merge|reject|withdraw"),
        };
    };
    // A verdict is filed in its proposal's workspace (proposal-workflow.md I2); under All an id this
    // node holds nowhere is said so, rather than looked for in the node default.
    let raw = param("workspace");
    let under_all = raw.as_deref().is_some_and(is_all);
    let workspace = match act_workspace(raw, || proposal_home(engine, &proposal)) {
        Ok(None) if under_all => {
            return Response {
                status: "400 Bad Request",
                content_type: "application/json",
                body: err_body(&format!("no proposal {proposal} in any workspace")),
            }
        }
        Ok(w) => w,
        Err(r) => return r,
    };
    // The Console surface (resolution.md Section 6): this server is reachable only through the 0600
    // unix socket, i.e. by the local OS principal - the engine stamps the console marker, which is
    // what permits a merged promotion to grant human_confirmed (a human's direct act, Principle 18).
    match engine.review_proposal(
        workspace,
        proposal,
        decision,
        None,
        None,
        supragnosis_engine::VerdictSurface::Console,
    ) {
        Ok(id) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({ "observation_id": id }).to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

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

/// `/api/reify?hyperedge=<id>[&name=<text>][&kind=<type>][&workspace=<ws>]` - reify a co-occurrence
/// context into first-class structure (Principle 11's promotion path): asserts a group entity +
/// member_of relations through the normal observe ingest, with `derived_from` naming every
/// co-asserting observation (P18 lineage). The hyperedge itself is untouched (a derived view);
/// the reified claim enters at the default tier and rises only through the gate.
fn reify_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let Some(hyperedge) = param("hyperedge") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("reify needs ?hyperedge=<id> (ids come from /api/hypergraph)"),
        };
    };
    match engine.reify_hyperedge(supragnosis_engine::ReifyInput {
        workspace: write_workspace(param("workspace")),
        hyperedge,
        name: param("name"),
        kind: param("kind"),
        source_ref: None,
        on_behalf_of: None,
    }) {
        Ok(out) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({
                "observation_id": out.observation_id,
                "entities": out.entities,
                "relations": out.relations,
            })
            .to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// `/api/propose_split?merge=<proposal id>[&workspace=<ws>]` - open an entity_split to reverse a
/// merge that already committed (unmerge.md). Like `propose_merge` this only OPENS the proposal; the
/// human accepts it in the proposals panel, so a reversal is as gated as the merge it undoes (P23).
///
/// The rationale is fixed rather than free-text for the reason `propose_merge`'s is: a proposal is an
/// immutable observation, and what it says about its own provenance must be true and must not be
/// shaped by the client (Principles 2, 18).
fn propose_split_response(engine: &Engine, query: &str) -> Response {
    let merge = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("merge="))
        .map(percent_decode)
        .filter(|s| !s.is_empty());
    let workspace = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode)
        .filter(|s| !s.is_empty());
    let Some(merge) = merge else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(
                "propose_split needs ?merge=<proposal id> (the entity_merge being reversed)",
            ),
        };
    };
    // A split is filed in its merge's workspace, where the merge it reverses counts.
    let workspace = match act_workspace(workspace, || proposal_home(engine, &merge)) {
        Ok(w) => w,
        Err(r) => return r,
    };
    // A person's act: asked again when the split it would land on no longer holds, and said as
    // found when it waits or is in force (design-system.md Section 29.23).
    match engine.propose_anew(
        supragnosis_engine::ProposeInput {
            workspace,
            kind: "entity_split".into(),
            targets: vec![merge],
            into: None,
            tier: None,
            rationale: Some("console reversal of an accepted merge".into()),
            affected_types: Vec::new(),
            source_ref: None,
            on_behalf_of: None,
        },
        true,
    ) {
        Ok(opened) => opened_response(&opened),
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// `/api/namesakes?entity=<id>&in=<ws>&in=<ws>...` - the entities of the listed workspaces that carry
/// this entity's name (design-system.md Section 29.16). The name is matched by the id's own rule -
/// trimmed and lower-cased with the workspace - so each workspace costs one lookup, and the page
/// sends the list it already holds rather than this server scanning for one. A namesake its own
/// workspace merged into another entity is left out, as the curation report's `duplicates` leaves
/// it out.
fn namesakes_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    // One `in` per workspace, each percent-encoded, so a name with a comma in it stays one name.
    let list: Vec<String> = query
        .split('&')
        .filter_map(|kv| kv.strip_prefix("in="))
        .map(percent_decode)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(id) = param("entity") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("namesakes needs ?entity=<id>&in=<workspace>&in=<workspace>"),
        };
    };
    let workspaces: Vec<&str> = list.iter().map(String::as_str).collect();
    let out = match engine.namesakes(&id, &workspaces) {
        Ok(Some(out)) => out,
        Ok(None) => {
            return Response {
                status: "404 Not Found",
                content_type: "application/json",
                body: err_body("no entity by that id"),
            }
        }
        Err(e) => return store_failure(&e),
    };
    Response {
        status: "200 OK",
        content_type: "application/json",
        body: serde_json::json!({ "namesakes": out }).to_string(),
    }
}

/// `/api/propose_merge?a=<id>&b=<id>[&workspace=<ws>]` - open an entity_merge proposal for a merge-
/// band suggestion (resolution-identity.md Section 3). `b` is the canonical (`into`) target the pair
/// folds into. This only OPENS the proposal (a gated appended event); the human accepts it in the
/// proposals panel, so the suggestion -> proposal -> verdict path stays gated (IR2/P23).
fn propose_merge_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let (Some(a), Some(b)) = (param("a"), param("b")) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("propose_merge needs ?a=<id>&b=<id> (ids from a merge suggestion)"),
        };
    };
    // Which surface produced the suggestion decides the recorded rationale. A proposal is an
    // immutable observation, so the reason it carries has to be TRUE: the deterministic name-variant
    // ladder needs no embedder, and filing its candidates as "embedding-near" would put a false
    // account of the evidence into the permanent log (Principle 2). Whitelisted rather than
    // free-text, so a client cannot shape what the log says about its own provenance (Principle 18).
    // An absent source is the merge band, which is what the viewer sends for a band pair; a source
    // this server does not know is refused rather than recorded as the band's reason.
    let rationale = match param("src").as_deref() {
        Some("variant:separator") => "name-variant ladder (separator/case normalization)",
        Some("variant:plural") => "name-variant ladder (plural fold)",
        Some("variant:alias") => "name-variant ladder (alias match)",
        // Same as... and Keep ... instead (design-system.md Section 29.17): the reason says why; the
        // console's mark says who.
        Some("picked") => "two entities named as one from a search",
        Some("reverse") => "the other name kept, instead of a merge the other way",
        None => "merge-band suggestion (embedding-near names)",
        Some(other) => {
            return Response {
                status: "400 Bad Request",
                content_type: "application/json",
                body: err_body(&format!("unknown suggestion source '{other}'")),
            }
        }
    };
    // The console opens no merge across workspaces (D9, principles.md Principle 15), and one taken
    // under All is filed in the workspace its pair shares (design-system.md Section 29.15).
    let (home_a, home_b) = match (entity_home(engine, &a), entity_home(engine, &b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(r), _) | (_, Err(r)) => return r,
    };
    if home_a.is_some() && home_b.is_some() && home_a != home_b {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("The console opens no merge across workspaces."),
        };
    }
    let workspace = match act_workspace(param("workspace"), || Ok(home_b.clone().or(home_a))) {
        Ok(w) => w,
        Err(r) => return r,
    };
    // A merge the console opens again after its un-merges says how many came before, so it is a
    // different proposal from each one a split reversed (unmerge.md S10). Counted from the log.
    let filed_in = workspace.clone().unwrap_or_else(|| engine.default_workspace().to_string());
    let before = match engine.unmerges_of(&filed_in, &[a.clone(), b.clone()], &b) {
        Ok(n) => n,
        Err(e) => return store_failure(&e),
    };
    let rationale = match before {
        0 => rationale.to_string(),
        1 => format!("{rationale}, again after 1 un-merge"),
        n => format!("{rationale}, after {n} un-merges"),
    };
    // S10's count is in the reason first; ", again" follows when the merge it would land on no
    // longer holds (design-system.md Section 29.23).
    match engine.propose_anew(
        supragnosis_engine::ProposeInput {
            workspace,
            kind: "entity_merge".into(),
            targets: vec![a, b.clone()],
            into: Some(b),
            tier: None,
            rationale: Some(rationale),
            affected_types: Vec::new(),
            source_ref: None,
            on_behalf_of: None,
        },
        true,
    ) {
        Ok(opened) => opened_response(&opened),
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// The answer of a console act that opens a proposal: its id, the state it is in, and whether it
/// was already in the log - so the notice says what the act found (design-system.md Section 29.23).
fn opened_response(opened: &supragnosis_engine::Proposed) -> Response {
    Response {
        status: "200 OK",
        content_type: "application/json",
        body: serde_json::json!({
            "proposal_id": opened.id,
            "state": opened.state,
            "found": opened.found,
        })
        .to_string(),
    }
}

/// `/api/workspaces` - the list of workspaces that hold knowledge (sorted, Principle 16). The viewer's
/// workspace picker consumes it - letting you click to pick rather than type a name. A failure is 500 (Principle 5).
/// `POST /api/peer/share?node_id=<id>&workspaces=a,b` - narrows what one federation peer may read.
///
/// Narrow-only, enforced by the injected handler: a request naming a workspace the peer does not
/// already hold is refused rather than partially applied. An empty `workspaces` is a legitimate
/// narrowing and means "admitted, but may read nothing" - which is why an absent parameter and an
/// empty one are distinguished here rather than both defaulting to "no change".
///
/// This does not remove the peer. Revocation, and the question of what it means for knowledge that
/// has already synced, is the deferred workflow in federation.md Section 11.
fn narrow_share_response(method: &str, query: &str, narrow: Option<&NarrowShare>) -> Response {
    if method != "POST" {
        return Response {
            status: "405 Method Not Allowed",
            content_type: "application/json",
            body: err_body("narrowing a peer's shared workspaces is a POST"),
        };
    }
    let Some(narrow) = narrow else {
        return Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "this server has no federation server role - there is no allowlist to narrow \
                 (docs/federation.md Section 9)",
            ),
        };
    };
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let Some(node_id) = param("node_id").filter(|s| !s.is_empty()) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(
                "needs ?node_id=<peer node id>&workspaces=<comma separated, may be empty>",
            ),
        };
    };
    // Absent means "you did not say", empty means "none" - collapsing them would turn a missing
    // parameter into a silent full revocation of reads.
    let Some(raw) = param("workspaces") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(
                "needs ?workspaces=... - pass it empty to narrow to nothing, omit nothing by accident",
            ),
        };
    };
    let keep: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

    match narrow(&node_id, &keep) {
        Ok(now) => Response {
            status: "200 OK",
            content_type: "application/json",
            body: serde_json::json!({"node_id": node_id, "shared_workspaces": now}).to_string(),
        },
        Err(e) => Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body(&e),
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
