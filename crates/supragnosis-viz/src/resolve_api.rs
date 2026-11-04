//! The settle routes: resolving a contested belief, previewing a settle, and reading what undoes a
//! merged claim promotion or demotion.

use supragnosis_engine::Engine;

use crate::act_api::{opened_response, write_workspace};
use crate::routes::{err_body, Response};
use crate::server::percent_decode;

/// The undo read (design-system.md Section 29.3): `?proposal=<id>` names a merged claim promotion or
/// demotion, and the answer is the acts that reverse it - each target back to the tier it has with
/// that proposal's merge verdicts left out - with their diff and checks. Read in the proposal's own
/// workspace, the one its opening observation was written to. Writes nothing.
pub(crate) fn resolve_undo_response(engine: &Engine, query: &str) -> Response {
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

/// The settle preview (design-system.md Section 29.2): the query `/api/resolve` takes - the
/// observations, the tier and the kind - and once more with the suffix `2` for a second act, which a
/// "Confirm instead" and its Undo need. Answers the acts' belief diff and checks, applied in order at
/// the console's grant, and writes nothing. Observations from two workspaces are refused, as
/// `/api/resolve` refuses them, since the fold that would read the act reads one workspace.
pub(crate) fn resolve_preview_response(engine: &Engine, query: &str) -> Response {
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
pub(crate) fn resolve_response(engine: &Engine, query: &str) -> Response {
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
