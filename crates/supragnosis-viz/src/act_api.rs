//! The console's acting routes, and the workspace an act is filed in: a verdict, a reification,
//! a merge or split opened, the namesakes a merge is picked from, and a peer's share narrowed.

use supragnosis_engine::Engine;

use crate::routes::{err_body, Response};
use crate::server::percent_decode;
use crate::NarrowShare;

/// A workspace to WRITE into. The read endpoints resolve `*`/`all`/empty to "every workspace" - a
/// VIEW, not a name, and a view is not somewhere an observation can be filed. Forwarding the
/// sentinel would create a workspace literally called `*` and quietly strand everything written
/// there. None means the node default, which is what an unspecified workspace has always meant on
/// the write path. Read and write must not disagree about what `*` is (Principle 17).
pub(crate) fn write_workspace(raw: Option<String>) -> Option<String> {
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
pub(crate) fn review_response(engine: &Engine, query: &str) -> Response {
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

/// `/api/reify?hyperedge=<id>[&name=<text>][&kind=<type>][&workspace=<ws>]` - reify a co-occurrence
/// context into first-class structure (Principle 11's promotion path): asserts a group entity +
/// member_of relations through the normal observe ingest, with `derived_from` naming every
/// co-asserting observation (P18 lineage). The hyperedge itself is untouched (a derived view);
/// the reified claim enters at the default tier and rises only through the gate.
pub(crate) fn reify_response(engine: &Engine, query: &str) -> Response {
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
pub(crate) fn propose_split_response(engine: &Engine, query: &str) -> Response {
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
pub(crate) fn namesakes_response(engine: &Engine, query: &str) -> Response {
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
pub(crate) fn propose_merge_response(engine: &Engine, query: &str) -> Response {
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
pub(crate) fn opened_response(opened: &supragnosis_engine::Proposed) -> Response {
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
pub(crate) fn narrow_share_response(
    method: &str,
    query: &str,
    narrow: Option<&NarrowShare>,
) -> Response {
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
