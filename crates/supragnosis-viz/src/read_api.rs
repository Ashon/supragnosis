//! The read routes: the graph and its projections, the log, search, proposals, and what the
//! node says about itself.

use supragnosis_engine::Engine;

use crate::first_view;
use crate::routes::{err_body, Response};
use crate::server::percent_decode;

/// `/api/graph` - resolves the workspace from the query and produces the graph projection.
/// - unspecified -> the node's default workspace (scoped view)
/// - `*` / `all` / empty value -> everything (None)
///
/// A storage failure is 500 + error body (Principle 5: a failure is not an empty graph).
pub(crate) fn graph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.graph(ws_owned.as_deref()) {
        Ok(graph) => match serde_json::to_string(&graph) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty graph (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/hypergraph` - co-occurrence second-order structure (hyperedge) projection (Principle 11 second-order structure).
/// Workspace resolution is identical to `/api/graph`. A read-only derived view (Principle 1) that the viewer
/// consumes as a hull overlay. A storage failure is 500 + error body (Principle 5: a failure is not an empty graph).
pub(crate) fn hypergraph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.hypergraph(ws_owned.as_deref()) {
        Ok(hg) => match serde_json::to_string(&hg) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty hypergraph (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/types` - the workspace type glossary (T-Box: entity/relation type definitions - Principles 8/11).
/// Workspace resolution is identical to `/api/graph`. A read-only projection (Principle 1). A failure is 500 (Principle 5).
pub(crate) fn types_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.types(ws_owned.as_deref()) {
        Ok(types) => match serde_json::to_string(&types) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty glossary (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/curation` - read-only curation signals (merge candidates / grab-bags / orphans, Principle 7
/// "generate not commit"). Workspace resolution is identical to `/api/graph`. A pure projection
/// (Principle 1/16); it commits nothing. A failure is 500 (Principle 5).
pub(crate) fn curation_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.curation(ws_owned.as_deref()) {
        Ok(report) => match serde_json::to_string(&report) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty curation report (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/observations[?workspace=&entity=&limit=|?id=]` - the observation log (the source of truth,
/// Principle 1), newest-first. Workspace resolution is identical to `/api/graph`. `entity=<id>`
/// narrows to the evidence set behind one node (forwarded through accepted merges); `limit` keeps
/// the newest N. A storage failure is 500 + error body (Principle 5: a failure is not an empty log).
pub(crate) fn observations_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let ws_owned: Option<String> = match param("workspace").as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    let entity = param("entity").filter(|s| !s.is_empty());
    let limit = param("limit").and_then(|s| s.parse::<usize>().ok());
    // `id=<observation id>`: that one, in whichever workspace holds it, so a card opens without the
    // scope's log (design-system.md Section 29.7). A list of one, or empty for an unknown id.
    let read = match param("id").filter(|s| !s.is_empty()) {
        // The local console's reads name each attestation's app (design-system.md Section 29.8).
        Some(id) => engine.observation_local(&id, None).map(|o| o.into_iter().collect::<Vec<_>>()),
        None => engine.observation_log_local(ws_owned.as_deref(), entity.as_deref(), limit),
    };
    match read {
        Ok(log) => match serde_json::to_string(&log) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty log (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/explain?entity=<id>` - "why is this node projected this way" (resolution.md Section 4): the
/// per-field belief resolution (evidence + decision) and the supporting observation log for one
/// entity. The workspace is derived from the entity itself, so only the id is needed.
/// Consistent-by-construction with `/api/graph` (built on `get_entity`). A missing `entity` is 400;
/// an id that resolves to no entity is 404 (absence, Principle 5); a storage failure is 500.
/// `/api/search?q=...&workspace=...&limit=...` (design-system.md Section 8, U6). An omitted, `*` or
/// `all` workspace searches the node, as the local trust surface may; an empty answer says what the
/// agent surface says - absence in what was searched is unknown, not a negation (P5).
pub(crate) fn search_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let Some(q) = param("q").filter(|s| !s.trim().is_empty()) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("search needs ?q=<words>"),
        };
    };
    // The scope reads as /api/graph reads it, so the list and the map answer over one set: no
    // parameter is the default workspace, and an empty one or "*" is every workspace.
    let ws: Option<String> = match param("workspace").as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    let limit = param("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(20).clamp(1, 50);
    match engine.search_canonical(&q, ws.as_deref(), limit) {
        Ok(out) => {
            let mut v = serde_json::json!({ "mode": out.mode, "hits": out.hits });
            if out.hits.is_empty() {
                v["note"] = "no hits here - absence is unknown, not a negation".into();
            }
            Response { status: "200 OK", content_type: "application/json", body: v.to_string() }
        }
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&format!("store error: {e}")),
        },
    }
}

pub(crate) fn explain_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
    };
    let Some(entity) = param("entity").filter(|s| !s.is_empty()) else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("explain needs ?entity=<id> (ids come from /api/graph nodes)"),
        };
    };
    match engine.explain_entity_local(&entity) {
        Ok(Some(ex)) => match serde_json::to_string(&ex) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Ok(None) => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "no entity for that id (it may have been merged away, or never existed)",
            ),
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an absent entity (Principle 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/proposals` - the workspace's proposals with folded state (Principle 23). Read-only projection.
/// `/api/proposal?id=<id>` - ONE proposal with its computed belief diff (proposal-workflow.md
/// Section 5). Separate from `/api/proposals` on purpose: a diff is two full belief folds, which is
/// the right cost for the proposal being reviewed and the wrong cost per row of a list. This is what
/// lets the console show what a verdict would change BEFORE casting it, rather than the canvas
/// overlay's guess from target ids alone.
pub(crate) fn proposal_response(engine: &Engine, query: &str) -> Response {
    let param = |k: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
            .map(percent_decode)
            .filter(|s| !s.is_empty())
    };
    let Some(id) = param("id") else {
        return Response {
            status: "400 Bad Request",
            content_type: "application/json",
            body: err_body("proposal needs ?id=<proposal id>"),
        };
    };
    let ws = param("workspace").filter(|s| s != "*" && s != "all");
    match engine.get_proposal(ws.as_deref(), &id) {
        Ok(Some(view)) => match serde_json::to_string(&view) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        // Principle 5: an unknown id is not-found, not an error, and says which it is.
        Ok(None) => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body("no proposal with that id in this workspace"),
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&format!("store error: {e}")),
        },
    }
}

pub(crate) fn proposals_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.list_proposals(ws_owned.as_deref()) {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string(), "note": "storage backend failure (Principle 5)" }).to_string(),
        },
    }
}

/// `/api/about` - what this build IS: name, version, license, source.
///
/// Read from the crate's own compile-time metadata rather than restated here, so the version in the
/// settings dialog is the version of the binary that answered the request. A constant would agree
/// with itself while the package moved.
///
/// Third-party notices are deliberately not assembled here. A hand-kept list of dependency licences
/// is a list that rots silently, and this surface has no way to know when it has - so it points at
/// the manifest, which cannot disagree with the build.
pub(crate) fn about_response() -> Response {
    let body = serde_json::json!({
        // The product, not the crate that happens to serve this route. `CARGO_PKG_NAME` here is
        // `supragnosis-viz`, which is an implementation detail no reader of a settings dialog wants.
        "name": "supragnosis",
        "version": env!("CARGO_PKG_VERSION"),
        "license": env!("CARGO_PKG_LICENSE"),
        "repository": env!("CARGO_PKG_REPOSITORY"),
    });
    Response { status: "200 OK", content_type: "application/json", body: body.to_string() }
}

/// `/api/health` - the owed-projection ledger's size and the last recovery this process ran
/// (crash-recovery.md K5). Read by `supragnosis status`, which is where an operator looks; a
/// recovery reported only in a startup log would have scrolled away (Principle 24).
pub(crate) fn health_response(engine: &Engine) -> Response {
    match engine.owed_count() {
        Ok(owed) => {
            let body = serde_json::json!({
                "owed_projections": owed,
                "last_recovery": engine.last_recovery(),
                // Which model, and whether it answers (semantic-retrieval.md SR3); null without one.
                "semantic": engine.semantic(),
            });
            Response { status: "200 OK", content_type: "application/json", body: body.to_string() }
        }
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: err_body(&e.to_string()),
        },
    }
}

/// `/api/surface`: which surface and node the page is on, the workspace an omitted one resolves to
/// (the node's default, so the page calls it by its name, design-system.md Section 29.10), and,
/// asked with `first_view=1`, where the first view opens, with the counts it was decided from. That
/// half reads every entity, so only a page whose address named no scope asks for it. When the
/// entities cannot be read `first_view` is left out and the page stays on the default (P5: unread
/// is not empty).
pub(crate) fn surface_response(engine: &Engine, query: &str) -> Response {
    let default = engine.default_workspace();
    let mut body = serde_json::json!({
        "surface": "local",
        "node": engine.host(),
        "default_workspace": default,
    });
    let asked = query.split('&').any(|kv| kv == "first_view=1");
    if let Some(g) = asked.then(|| engine.graph(None).ok()).flatten() {
        let (held, total) =
            (g.nodes.iter().filter(|n| n.workspace == default).count(), g.nodes.len());
        body["first_view"] = serde_json::json!({
            "scope": first_view(held, total),
            "default_holds": held,
            "of": total,
        });
    }
    Response { status: "200 OK", content_type: "application/json", body: body.to_string() }
}

pub(crate) fn workspaces_response(engine: &Engine) -> Response {
    match engine.workspaces() {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => Response { status: "200 OK", content_type: "application/json", body: json },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty list (Principle 5)"
            })
            .to_string(),
        },
    }
}
