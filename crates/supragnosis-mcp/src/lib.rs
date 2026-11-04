//! supragnosis-mcp - MCP surface (tools + resources).
//!
//! Defines tools via rmcp macros and delegates to [`supragnosis_engine::Engine`].
//! Tools: `observe`, `get_entity`, `search_knowledge`, `traverse`.
//! Resources: `supragnosis://workspace/{ws}/graph` - read view of the ontology graph (node-link),
//! `supragnosis://observation/{id}` - observation back-reference (raw content + provenance + lineage, Principles 2/14).

use std::future::Future;
use std::sync::Arc;

pub mod prompts;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::{Deserialize, Serialize};

use supragnosis_engine::{
    AffectedType as EngineAffectedType, DefineTypeInput, Engine, EntityInput as EngineEntityInput,
    Event, ObserveInput, ProposeInput as EngineProposeInput, RelationInput as EngineRelationInput,
    SearchMode, TypeDefInput as EngineTypeDefInput, TypeTarget,
};

mod dto;
mod handler;
mod sync;
mod tools;
mod usage;

pub use dto::*;
pub use sync::*;
pub use usage::*;

// --- Server ------------------------------------------------------------------

/// Whether a workspace argument names every workspace: `*`, `all` or nothing (design-system.md
/// Section 29.15).
fn is_all(ws: &str) -> bool {
    matches!(ws.trim(), "" | "*" | "all")
}

pub struct SupragnosisServer {
    engine: Arc<Engine>,
    /// First contact, recorded on the local daemon's surface only (docs/onboarding.md Section 5).
    clients: Option<Arc<ClientLog>>,
    /// The usage log, on the local daemon's surface only (docs/usage.md).
    usage: Option<Arc<UsageLog>>,
    sync: Option<Arc<SyncContext>>,
    /// The hub's listener state, there from the daemon's start, so a node with no sync that an
    /// invitation made a hub still says what its listener is doing (docs/pairing.md Section 4.1).
    listener: Option<Arc<std::sync::RwLock<serde_json::Value>>>,
    tool_router: ToolRouter<SupragnosisServer>,
}

impl SupragnosisServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            clients: None,
            usage: None,
            sync: None,
            listener: None,
            tool_router: Self::tool_router(),
        }
    }

    /// Records which clients talk to this server (docs/onboarding.md Section 5).
    pub fn with_clients(mut self, log: Arc<ClientLog>) -> Self {
        self.clients = Some(log);
        self
    }

    /// Records every call this surface answers in the usage log (docs/usage.md).
    pub fn with_usage(mut self, log: Arc<UsageLog>) -> Self {
        self.usage = Some(log);
        self
    }

    /// One usage line: the call's kind and name, the workspace it named, its app, what it came to,
    /// the fields only some calls have (`commit`, `cites`, `via`), and how long it took
    /// (docs/usage.md Section 2).
    #[allow(clippy::too_many_arguments)]
    fn note_use(
        &self,
        kind: &str,
        name: &str,
        workspace: Option<&str>,
        app: Option<&str>,
        (outcome, n, mode): (&str, Option<usize>, Option<String>),
        more: serde_json::Value,
        started: std::time::Instant,
    ) {
        let Some(log) = &self.usage else {
            return;
        };
        let mut line = serde_json::json!({
            "t": supragnosis_core::now_millis(),
            "kind": kind,
            "name": name,
            "outcome": outcome,
            "ms": started.elapsed().as_millis() as u64,
        });
        let fields = [
            ("workspace", workspace.map(serde_json::Value::from)),
            ("app", app.map(serde_json::Value::from)),
            ("n", n.map(serde_json::Value::from)),
            ("mode", mode.map(serde_json::Value::from)),
        ];
        for (k, v) in fields {
            if let Some(v) = v {
                line[k] = v;
            }
        }
        if let serde_json::Value::Object(more) = more {
            for (k, v) in more.into_iter().filter(|(_, v)| !v.is_null()) {
                line[k.as_str()] = v;
            }
        }
        log.record(line);
    }

    /// Notes a request's client in the first-contact log: the name its session's `initialize` gave,
    /// or - in a session that skipped `initialize`, as newer protocol versions allow - the client
    /// information the request carries in its metadata, or an unnamed client.
    fn note_contact(&self, context: &RequestContext<RoleServer>) {
        let Some(log) = &self.clients else {
            return;
        };
        if let Some(info) = context.peer.peer_info() {
            log.note(&info.client_info.name, &info.client_info.version);
        } else if let Some(info) = context.meta.client_info() {
            log.note(&info.name, &info.version);
        } else {
            log.note(ClientLog::UNNAMED, "");
        }
    }

    /// Search hits as an agent reads them (design-system.md Section 29.20): an entity hit from this node
    /// carries its name, type and workspace, read from its projected row, so two hits named alike
    /// can be told apart. A hit whose row cannot be read keeps what it had.
    fn named_hits(&self, hits: &[supragnosis_core::SearchHit]) -> serde_json::Value {
        let store = self.engine.store();
        serde_json::Value::Array(
            hits.iter()
                .map(|h| {
                    let mut v = serde_json::to_value(h).unwrap_or(serde_json::Value::Null);
                    if h.kind == supragnosis_core::SearchHitKind::Entity {
                        if let Ok(Some(e)) = store.get_entity(&h.id) {
                            v["name"] = e.canonical_name.clone().into();
                            v["type"] = e.kind.clone().into();
                            if let Some(p) = e.provenance.first() {
                                v["workspace"] = p.workspace.clone().into();
                            }
                        }
                    }
                    v
                })
                .collect(),
        )
    }

    /// The note a local `observe` adds when it makes a new workspace (design-system.md Section
    /// 29.20): the workspaces that hold knowledge, nearest to the new name first by the distance
    /// between their letters and digits, then by name, ten at most and how many more.
    fn new_workspace_note(&self, ws: &str) -> Option<String> {
        let mut known: Vec<String> = match self.engine.workspaces() {
            Ok(list) => list.into_iter().filter(|w| w != ws).collect(),
            Err(_) => return None,
        };
        if known.is_empty() {
            return Some(format!("workspace {ws} is new; this node held no knowledge before"));
        }
        let key = |w: &str| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<Vec<_>>()
        };
        let target = key(ws);
        known.sort_by_cached_key(|w| (edit_distance(&key(w), &target), w.clone()));
        let more = known.len().saturating_sub(10);
        known.truncate(10);
        let mut list = known.join(", ");
        if more > 0 {
            list.push_str(&format!(" and {more} more"));
        }
        Some(format!(
            "workspace {ws} is new; the workspaces that hold knowledge are {list} - observe there if \
             this was meant for one of them"
        ))
    }

    /// On this node `*` is the all-workspaces view, and a write is filed in one workspace (design-system.md
    /// Section 29.15); it used to file into a workspace named `*`.
    fn all_is_no_place(&self, tool: &str, ws: &str) -> Option<String> {
        is_all(ws).then(|| format!("{tool} writes to one workspace - name it"))
    }

    /// The answer to an entity id that is unknown. `observation` says the id names an observation
    /// instead: the answer stays not found, and its note points at the read that takes the id
    /// (Principle 21).
    fn unknown_entity(&self, id: &str, observation: bool) -> String {
        self.engine
            .emit(Event::GetEntity { id: id.to_string(), name: None, found: false });
        let note = "unknown - not found is not a negation (open-world assumption)";
        // A name passed as an id is said to be one, from the id's shape alone, so it reads the same
        // whatever the caller (design-system.md Section 29.20).
        let looks_like_id = id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit());
        let note = if observation {
            format!(
                "{note}. This id is an observation's, not an entity's: read it as the resource \
                 supragnosis://observation/{id}, or pass get_entity the id of an entity hit"
            )
        } else if looks_like_id {
            note.to_string()
        } else {
            format!(
                "{note}. This looks like a name; an entity's id is the 64-character hex id \
                 search_knowledge returns - search for the name"
            )
        };
        serde_json::json!({ "found": false, "id": id, "note": note }).to_string()
    }

    /// Attaches the federation sync wiring (M4 Phase 4) - enables the `sync_*` tools.
    pub fn with_sync(mut self, sync: Arc<SyncContext>) -> Self {
        self.sync = Some(sync);
        self
    }

    /// The hub's listener state, for `sync_status` on a node with no sync of its own.
    pub fn with_listener(mut self, listener: Arc<std::sync::RwLock<serde_json::Value>>) -> Self {
        self.listener = Some(listener);
        self
    }
}

/// One resource read: the URI's content, or the error that stands for it.
fn read_uri(engine: &Engine, uri: String) -> Result<ReadResourceResult, ErrorData> {
    match parse_resource_uri(&uri) {
        // Workspace list (for discovery) - array of workspace names that hold knowledge.
        Some(ResourceUri::Workspaces) => match engine.workspaces() {
            Ok(list) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&list), uri)]))
            }
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Graph(ws)) => {
            match engine.graph(Some(ws).filter(|w| !is_all(w))) {
                Ok(graph) => {
                    Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&graph), uri)]))
                }
                // A backend failure is surfaced as an internal error, not not_found (absence) - preserving the Principle 5 distinction.
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            }
        }
        // Co-occurrence second-order structure (Principle 11): the entity sets an observation asserted together, as hyperedges.
        Some(ResourceUri::Hypergraph(ws)) => match engine
            .hypergraph(Some(ws).filter(|w| !is_all(w)))
        {
            Ok(hg) => Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&hg), uri)])),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // T-Box glossary (Principle 8/11): the workspace's type definitions (entity + relation types + meanings).
        Some(ResourceUri::Types(ws)) => match engine.types(Some(ws).filter(|w| !is_all(w))) {
            Ok(types) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&types), uri)]))
            }
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // Observation back-reference (Principles 2/14): whoever knows the id can query the substance (raw content/provenance/lineage).
        Some(ResourceUri::Observation(id)) => match engine.get_observation(id) {
            Ok(Some(obs)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&obs), uri)]))
            }
            Ok(None) => Err(unknown_observation(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // An entity by its id (Principle 14), as get_entity answers it.
        // A proposal by its id (Principle 14), read where it lives.
        Some(ResourceUri::Proposal(id)) => match engine.get_proposal(None, id) {
            Ok(Some(view)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&view), uri)]))
            }
            Ok(None) => Err(unknown_proposal_resource(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Entity(id)) => match engine.get_entity(id) {
            Ok(Some(view)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&view), uri)]))
            }
            Ok(None) => Err(unknown_entity_resource(id)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        None => Err(ErrorData::resource_not_found(
            format!(
                "unknown resource URI: {uri} - the forms are supragnosis://workspaces, \
                 supragnosis://workspace/{{workspace}}/graph, .../hypergraph and .../types, \
                 supragnosis://observation/{{id}}, supragnosis://entity/{{id}} and \
                 supragnosis://proposal/{{id}}"
            ),
            None,
        )),
    }
}

/// Kinds of resource URI.
enum ResourceUri<'a> {
    /// `supragnosis://workspaces` - list of workspaces that hold knowledge (intro/discovery).
    Workspaces,
    /// `supragnosis://workspace/<ws>/graph` - workspace ontology graph.
    Graph(&'a str),
    /// `supragnosis://workspace/<ws>/hypergraph` - co-occurrence second-order structure (Principle 11 second-order structure).
    Hypergraph(&'a str),
    /// `supragnosis://workspace/<ws>/types` - the T-Box type glossary (Principle 8/11).
    Types(&'a str),
    /// `supragnosis://observation/<id>` - observation back-reference (Principles 2/14).
    Observation(&'a str),
    /// `supragnosis://entity/<id>` - an entity, as get_entity answers it (Principle 14).
    Entity(&'a str),
    /// `supragnosis://proposal/<id>` - a proposal, read where it lives (Principle 14).
    Proposal(&'a str),
}

/// The mark an index line puts after an observation's id (docs/recall.md Section 7.1, D12). The
/// record read is the same with or without it; the usage log notes that it was found in an index.
const VIA_INDEX: &str = "?via=index";

/// An id as `derived_from` keeps it: an agent that copies a record's link from an index line or a
/// search hit names the record, so the link's scheme and the index mark are taken off (docs/recall.md
/// Section 7.1). Anything else is kept as written.
fn lineage_id(given: &str) -> String {
    let id = given.trim();
    let id = id.strip_prefix("supragnosis://observation/").unwrap_or(id);
    id.strip_suffix(VIA_INDEX).unwrap_or(id).to_string()
}

/// A resource read's kind, the workspace its URI names, and `index` when an index line gave the
/// URI, for the usage log (docs/usage.md): the kind's word only, never an id (UL2).
fn resource_use(uri: &str) -> (&'static str, Option<String>, Option<&'static str>) {
    let (kind, workspace) = match parse_resource_uri(uri) {
        Some(ResourceUri::Workspaces) => ("workspaces", None),
        Some(ResourceUri::Graph(ws)) => ("graph", Some(ws.to_string())),
        Some(ResourceUri::Hypergraph(ws)) => ("hypergraph", Some(ws.to_string())),
        Some(ResourceUri::Types(ws)) => ("types", Some(ws.to_string())),
        Some(ResourceUri::Observation(_)) => ("observation", None),
        Some(ResourceUri::Entity(_)) => ("entity", None),
        Some(ResourceUri::Proposal(_)) => ("proposal", None),
        None => ("unknown", None),
    };
    let via = (kind == "observation" && uri.ends_with(VIA_INDEX)).then_some("index");
    (kind, workspace, via)
}

/// Resource URI parser. Returns None on malformed input. A segment must not contain `/`.
fn parse_resource_uri(uri: &str) -> Option<ResourceUri<'_>> {
    // Exact match first - "workspaces" (plural) does not overlap the "workspace/" (singular+slash) prefix.
    if uri == "supragnosis://workspaces" {
        return Some(ResourceUri::Workspaces);
    }
    if let Some(rest) = uri.strip_prefix("supragnosis://workspace/") {
        // Check "/hypergraph" first - "hypergraph" does not end with "/graph" (preceded by 'r'),
        // but we keep the explicit order to make the intent clear. Reject if a segment contains '/' (single segment).
        if let Some(ws) = rest.strip_suffix("/hypergraph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Hypergraph(ws));
        }
        if let Some(ws) = rest.strip_suffix("/graph") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Graph(ws));
        }
        if let Some(ws) = rest.strip_suffix("/types") {
            if ws.is_empty() || ws.contains('/') {
                return None;
            }
            return Some(ResourceUri::Types(ws));
        }
        return None;
    }
    if let Some(id) = uri.strip_prefix("supragnosis://observation/") {
        let id = id.strip_suffix(VIA_INDEX).unwrap_or(id);
        if id.is_empty() || id.contains(['/', '?', '#']) {
            return None;
        }
        return Some(ResourceUri::Observation(id));
    }
    if let Some(id) = uri.strip_prefix("supragnosis://entity/") {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        return Some(ResourceUri::Entity(id));
    }
    if let Some(id) = uri.strip_prefix("supragnosis://proposal/") {
        if id.is_empty() || id.contains('/') {
            return None;
        }
        return Some(ResourceUri::Proposal(id));
    }
    None
}

// --- Serialization helpers (tools return JSON strings) ----------------------

tokio::task_local! {
    /// The client name of the MCP call in progress, as its session named itself (design-system.md
    /// Section 29.8, onboarding.md O7).
    static WRITING_APP: Option<String>;
}

/// The app of the call in progress, or `None` outside one (the CLI's).
fn writing_app() -> Option<String> {
    WRITING_APP.try_with(|a| a.clone()).ok().flatten()
}

/// A request's client name: its session's `initialize`, else the client information the request
/// carries in its metadata, recorded as a node records it (cut to 64 characters).
fn client_name(context: &RequestContext<RoleServer>) -> Option<String> {
    context
        .peer
        .peer_info()
        .map(|i| i.client_info.name.clone())
        .or_else(|| context.meta.client_info().map(|i| i.name.clone()))
        .and_then(|n| supragnosis_core::client_recorded(&n))
}

fn to_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| err_json(&e.to_string()))
}

/// Response for a storage backend failure (Principles 5/21). A failure is a different event from
/// absence, so we state "this is not an empty result but a query failure" and guide the next action
/// so the LLM does not conclude that knowledge is absent.
fn store_failure_json(e: &impl std::fmt::Display) -> String {
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
fn flag_error(resp: Result<CallToolResponse, ErrorData>) -> Result<CallToolResponse, ErrorData> {
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
fn edit_distance(a: &[char], b: &[char]) -> usize {
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

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// What `propose` adds when its content names a proposal already in the log: what that one is and
/// what is left (design-system.md Section 29.23).
fn found_note(state: &str) -> String {
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
fn no_effect_found(note: &str) -> String {
    note.split(':').next().unwrap_or(note).trim().to_string()
        + ", so it cannot be merged until it has one"
}

/// What `propose` adds when it opens a kind with no commit effect yet, so the agent can tell the
/// person what is left to do with it (design-system.md Section 29.22).
fn no_effect_note(kind: &str) -> Option<String> {
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
fn unknown_proposal_resource(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "proposal id not found: {id} - absence is not a negation (open-world). Use an id \
             from propose or list_proposals"
        ),
        None,
    )
}

/// An entity resource no entity answers (Principle 5).
fn unknown_entity_resource(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "entity id not found: {id} - absence is not a negation (open-world). Use the \
             kind=entity id from a search_knowledge hit or a graph node"
        ),
        None,
    )
}

fn unknown_observation(id: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!(
            "observation id not found: {id} - absence is not a negation (open-world). Use the \
             kind=observation id from a search_knowledge hit"
        ),
        None,
    )
}

#[cfg(test)]
mod usage_tests {
    use super::{lineage_id, parse_resource_uri, resource_use, ResourceUri};

    /// recall.md Section 7.1: a record's link copied into `derived_from` names the record, and an
    /// observation URI takes the index mark and no other query.
    #[test]
    fn a_copied_link_names_its_record() {
        let id = "ab".repeat(32);
        for given in [
            id.clone(),
            format!("supragnosis://observation/{id}"),
            format!("supragnosis://observation/{id}?via=index"),
            format!("{id}?via=index"),
        ] {
            assert_eq!(lineage_id(&given), id, "{given}");
        }
        assert_eq!(lineage_id("something else"), "something else");
        let marked = format!("supragnosis://observation/{id}?via=index");
        assert!(
            matches!(parse_resource_uri(&marked), Some(ResourceUri::Observation(i)) if i == id)
        );
        assert_eq!(resource_use(&marked).2, Some("index"));
        for not in [format!("{marked}?via=index"), format!("supragnosis://observation/{id}?x=1")] {
            assert!(parse_resource_uri(&not).is_none(), "{not}");
        }
    }
}
