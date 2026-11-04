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
mod reply;
mod resources;
mod sync;
mod tools;
mod usage;

pub use dto::*;
use reply::*;
use resources::*;
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
