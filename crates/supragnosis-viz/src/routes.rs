//! The path table: the endpoints the wiring layer hooks in, then the engine's routes, and the
//! response they all answer with.

use supragnosis_engine::Engine;

use crate::read_api::{
    about_response, curation_response, explain_response, graph_response, health_response,
    hypergraph_response, observations_response, proposal_response, proposals_response,
    search_response, surface_response, types_response, workspaces_response,
};
use crate::{
    namesakes_response, narrow_share_response, propose_merge_response, propose_split_response,
    reify_response, resolve_preview_response, resolve_response, resolve_undo_response,
    review_response, ClientsView, FedStatus, NarrowShare, VIEWER_CSS, VIEWER_GL_JS, VIEWER_HTML,
    VIEWER_JS, VIEWER_SIM_JS,
};

/// The response to one non-streaming request: the two endpoints the wiring layer hooks in, then
/// the engine's routes.
pub(crate) fn respond(
    engine: &Engine,
    method: &str,
    path: &str,
    query: &str,
    fed: Option<&FedStatus>,
    narrow: Option<&NarrowShare>,
    clients: Option<&ClientsView>,
) -> Response {
    if path == "/api/peer/share" {
        // POST, unlike every other endpoint here. The GET-with-side-effect convention the rest of
        // this surface uses is defensible because no third-party origin can reach a 0600 unix socket,
        // and that argument still holds - but this one changes a sharing boundary rather than
        // appending a gated verdict, and the method is the cheapest place to say so. Parameters stay
        // in the query string, so the minimal server still needs no body parser.
        narrow_share_response(method, query, narrow)
    } else if path == "/api/federation" {
        // Federation status (hubs, health, per-workspace diff, known peers) - maintained by the
        // wiring layer; absent on a standalone node.
        Response {
            status: "200 OK",
            content_type: "application/json",
            body: fed
                .map(|f| f.read().map(|v| v.to_string()).unwrap_or_else(|_| "{}".into()))
                .unwrap_or_else(|| "{\"configured\":false}".to_string()),
        }
    } else if path == "/api/clients" {
        // First contact - kept by the MCP server, absent where nothing records it (a viewer with no
        // daemon behind it), which is said rather than shown as an empty list (P5).
        Response {
            status: "200 OK",
            content_type: "application/json",
            body: clients.map(|c| c().to_string()).unwrap_or_else(|| {
                "{\"clients\":null,\"note\":\"this server does not record first contact\"}"
                    .to_string()
            }),
        }
    } else {
        route(engine, method, path, query)
    }
}

/// (status line, content-type, body) - the three fixed components of a response.
pub(crate) struct Response {
    pub(crate) status: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) body: String,
}

fn route(engine: &Engine, method: &str, path: &str, query: &str) -> Response {
    if method != "GET" {
        return Response {
            status: "405 Method Not Allowed",
            content_type: "application/json",
            body: err_body("only GET is supported"),
        };
    }
    match path {
        "/" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: VIEWER_HTML.to_string(),
        },
        // The stylesheet and script are served as their own same-origin assets (compile-time embedded
        // via include_str!, so still a single binary and offline). Splitting them out of one inline
        // document gives the frontend real .css/.js files - editor tooling, linting (ESLint
        // no-unsanitized guards the innerHTML sinks), and clean diffs.
        "/viewer.css" => Response {
            status: "200 OK",
            content_type: "text/css; charset=utf-8",
            body: VIEWER_CSS.to_string(),
        },
        "/viewer.js" => Response {
            status: "200 OK",
            content_type: "text/javascript; charset=utf-8",
            body: VIEWER_JS.to_string(),
        },
        // The layout (design-system.md Section 27.5): a classic script before viewer.js, and the
        // worker the page starts from it - the policy admits a worker from a served file.
        "/viewer-sim.js" => Response {
            status: "200 OK",
            content_type: "text/javascript; charset=utf-8",
            body: VIEWER_SIM_JS.to_string(),
        },
        // The GPU renderer (design-system.md Section 27.9): a classic script before viewer.js.
        "/viewer-gl.js" => Response {
            status: "200 OK",
            content_type: "text/javascript; charset=utf-8",
            body: VIEWER_GL_JS.to_string(),
        },
        "/api/graph" => graph_response(engine, query),
        "/api/hypergraph" => hypergraph_response(engine, query),
        "/api/types" => types_response(engine, query),
        // The store's most used types, which a page gives the type band's colors once
        // (design-system.md Section 29.9, D4): every workspace on the local console.
        "/api/common_types" => match engine.common_types(None) {
            Ok(types) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: serde_json::json!({ "types": types }).to_string(),
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("store error: {e}")),
            },
        },
        "/api/curation" => curation_response(engine, query),
        "/api/proposals" => proposals_response(engine, query),
        // Review verdict: a GET carrying the action in the query. GET-with-side-effect is intentional here -
        // the minimal server does not parse request bodies, and the effect is a gated append-only verdict
        // (engine.review_proposal records a verdict observation; the fold decides), which is idempotent for
        // merge (absorbing state, I14/I16). The unix socket's 0600 mode is the write gate (Principle 17 /
        // F19: only the owning user can connect). It routes through the gate, never a direct
        // projection/log write (I18 / proposal-workflow.md 14.3).
        //
        // What makes a state-changing GET safe here is that no THIRD-PARTY origin can reach the socket,
        // not that no browser can: the desktop shell proxies this surface into a webview (`viz://`), and
        // the console needs these very endpoints. With no attacker origin there is nothing for CSRF or
        // rebinding to ride, and the class that remains is stored XSS in the page we serve - which is
        // what the escaping guard and the `no-unsanitized` lint exist for (architecture.md Section 10).
        "/api/review" => review_response(engine, query),
        // One-click mediation for a contested belief (resolution.md Section 4.2): opens a
        // claim_promotion for the chosen observation(s) and immediately casts the Console merge
        // verdict - both are gated, appended events (propose + review), never a direct write. Solo
        // self-approval is the P23 exception; the Console surface is what permits human_confirmed.
        "/api/resolve" => resolve_response(engine, query),
        // What a settle would do, without casting it (design-system.md Section 29.2).
        "/api/resolve/preview" => resolve_preview_response(engine, query),
        // What reverses a merged confirmation or demotion, and its diff (Section 29.3).
        "/api/resolve/undo" => resolve_undo_response(engine, query),
        // Hyperedge promotion path (Principle 11): reify a co-occurrence context into a group
        // entity + member_of relations, as an ordinary lineage-bearing observation (free ingest,
        // P22 - trust promotion of the reified claim goes through the gate afterwards).
        "/api/reify" => reify_response(engine, query),
        // Merge-band action (resolution-identity.md Section 3): open an entity_merge proposal for a
        // suggested pair - the suggestion is a retrieval aid; committing goes through the gate (P23).
        // The opened proposal then rides the normal accept flow in the proposals panel (IR2).
        "/api/proposal" => proposal_response(engine, query),
        "/api/propose_merge" => propose_merge_response(engine, query),
        "/api/propose_split" => propose_split_response(engine, query),
        "/api/namesakes" => namesakes_response(engine, query),
        "/api/workspaces" => workspaces_response(engine),
        "/api/about" => about_response(),
        // Which surface the page is on (remote-viewer.md Section 3.5): this socket is the owner's
        // full console, the only surface since the hub's read tier was removed in 0.11.0. The page
        // no longer reads `surface`; it stays in the answer (remote-server.md Section 13), so a page
        // loaded before an upgrade still finds it.
        // `node` is this node's host id: the page keys what it remembers locally - the layout - by it
        // (design-system.md Section 5), and names a host "this node" only when it matches (Section 9).
        "/api/surface" => surface_response(engine, query),
        // What this node's store owes and last repaid (crash-recovery.md K5) - runtime state, kept
        // apart from /api/about, which describes the build.
        "/api/health" => health_response(engine),
        // The observation log (source of truth, Principle 1), newest-first; `entity=<id>` narrows to
        // the evidence set behind one node. A read-only projection (Principle 5: failure != empty).
        "/api/observations" => observations_response(engine, query),
        // "Why is this node projected this way" (resolution.md): per-field belief resolution
        // (evidence + decision) + the supporting log for one entity. Read-only, consistent with graph.
        "/api/explain" => explain_response(engine, query),
        // The viewer's search (design-system.md Section 8): the engine's own ranking and mode label,
        // entity hits forwarded through accepted merges. A read, with a read-tier policy (V2).
        "/api/search" => search_response(engine, query),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "unknown path - try /, /api/surface, /api/proposal, /api/graph, /api/hypergraph, /api/types, /api/curation, /api/proposals, /api/review, /api/resolve, /api/reify, /api/propose_merge, /api/propose_split, /api/namesakes, /api/workspaces, /api/observations, /api/explain, or /api/events",
            ),
        },
    }
}

pub(crate) fn err_body(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
