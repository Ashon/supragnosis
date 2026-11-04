//! The answers' wording: JSON helpers, error and not-found answers, the app of the call.

use super::*;

// --- Serialization helpers (tools return JSON strings) ----------------------

tokio::task_local! {
    /// The client name of the MCP call in progress, as its session named itself (design-system.md
    /// Section 29.8, onboarding.md O7).
    pub(crate) static WRITING_APP: Option<String>;
}

/// The app of the call in progress, or `None` outside one (the CLI's).
pub(crate) fn writing_app() -> Option<String> {
    WRITING_APP.try_with(|a| a.clone()).ok().flatten()
}

/// A request's client name: its session's `initialize`, else the client information the request
/// carries in its metadata, recorded as a node records it (cut to 64 characters).
pub(crate) fn client_name(context: &RequestContext<RoleServer>) -> Option<String> {
    context
        .peer
        .peer_info()
        .map(|i| i.client_info.name.clone())
        .or_else(|| context.meta.client_info().map(|i| i.name.clone()))
        .and_then(|n| supragnosis_core::client_recorded(&n))
}

pub(crate) fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| err_json(&e.to_string()))
}

/// Response for a storage backend failure (Principles 5/21). A failure is a different event from
/// absence, so we state "this is not an empty result but a query failure" and guide the next action
/// so the LLM does not conclude that knowledge is absent.
pub(crate) fn store_failure_json(e: &impl std::fmt::Display) -> String {
    serde_json::json!({
        "error": e.to_string(),
        "note": "storage backend failure - this is NOT an empty result. \
                 Do not conclude that knowledge is absent. Retry, or ask the user to check the storage status"
    })
    .to_string()
}

/// A tool answer that is a top-level `error` object is flagged as a tool error, MCP's own convention,
/// so a client tells a failure from a result without reading it (design-system.md Section 29.20). A
/// not-found answer is a result and stays one (P5); an error inside a larger answer - one server's
/// failure in a federated search or a sync round - stays inside it.
pub(crate) fn flag_error(
    resp: Result<CallToolResponse, ErrorData>,
) -> Result<CallToolResponse, ErrorData> {
    resp.map(|r| match r {
        CallToolResponse::Complete(mut result) => {
            let failed = result
                .content
                .first()
                .and_then(|c| c.as_text())
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t.text).ok())
                .is_some_and(|v| v.as_object().is_some_and(|o| o.contains_key("error")));
            if failed {
                result.is_error = Some(true);
            }
            CallToolResponse::Complete(result)
        }
        other => other,
    })
}

/// The Levenshtein distance between two letter sequences: how a new workspace's name ranks the ones
/// that hold knowledge (design-system.md Section 29.20).
pub(crate) fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

pub(crate) fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// What `propose` adds when its content names a proposal already in the log: what that one is and
/// what is left (design-system.md Section 29.23).
pub(crate) fn found_note(state: &str) -> String {
    match state {
        "open" => "an identical proposal is already open - a proposal is its content, so this is that one",
        "blocked" => "an identical proposal was accepted but a check holds it back - get_proposal names the check",
        "rejected" => "an identical proposal was rejected - a reject is provisional, and a different rationale opens a new one",
        "merged" => "an identical proposal was already accepted - if a later act has superseded it, a different rationale opens a new one",
        "withdrawn" => "an identical proposal was withdrawn - a different rationale opens a new one",
        other => return format!("an identical proposal already exists, {other}"),
    }
    .to_string()
}

/// Section 29.22's note for a found proposal: what is left is in the found note.
pub(crate) fn no_effect_found(note: &str) -> String {
    note.split(':').next().unwrap_or(note).trim().to_string()
        + ", so it cannot be merged until it has one"
}

/// What `propose` adds when it opens a kind with no commit effect yet, so the agent can tell the
/// person what is left to do with it (design-system.md Section 29.22).
pub(crate) fn no_effect_note(kind: &str) -> Option<String> {
    (supragnosis_engine::PROPOSAL_KINDS.contains(&kind)
        && !supragnosis_engine::has_commit_effect(kind))
    .then(|| {
        format!(
            "{kind} has no commit effect yet: it can be commented on, rejected or withdrawn, \
                 and cannot be merged until it has one"
        )
    })
}

/// A proposal resource no proposal answers (Principle 5).
pub(crate) fn unknown_proposal_resource(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "proposal id not found: {id} - absence is not a negation (open-world). Use an id \
             from propose or list_proposals"
        ),
        None,
    )
}

/// An entity resource no entity answers (Principle 5).
pub(crate) fn unknown_entity_resource(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "entity id not found: {id} - absence is not a negation (open-world). Use the \
             kind=entity id from a search_knowledge hit or a graph node"
        ),
        None,
    )
}

pub(crate) fn unknown_observation(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "observation id not found: {id} - absence is not a negation (open-world). Use the \
             kind=observation id from a search_knowledge hit"
        ),
        None,
    )
}
