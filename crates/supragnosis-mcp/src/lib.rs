//! supragnosis-mcp - MCP surface (tools + resources).
//!
//! Defines tools via rmcp macros and delegates to [`supragnosis_engine::Engine`].
//! Tools: `observe`, `get_entity`, `search_knowledge`, `traverse`.
//! Resources: `supragnosis://workspace/{ws}/graph` - read view of the ontology graph (node-link),
//! `supragnosis://observation/{id}` - observation back-reference (raw content + provenance + lineage, Principles 2/14).

use std::future::Future;
use std::sync::Arc;

pub mod prompts;
pub mod remote;

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
    SearchMode, TypeDefInput as EngineTypeDefInput, TypeTarget, GATE_KINDS,
};

// --- Transport DTOs (JSON Schema auto-generated) ----------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveRequest {
    /// Knowledge fragment to ingest (natural language or structured text).
    pub content: String,
    /// Workspace (defaults to the node default when omitted). Knowledge is organized into named workspaces,
    /// so name the one this fragment belongs to: silently taking the node default splits a body of knowledge
    /// across two workspaces, and later reads scoped to either will see only half.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Source reference (file path/URL/tool, etc.).
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Confidence 0.0-1.0. If omitted, it is preserved as unspecified (no default substitution) -
    /// omit it when you cannot assess it. Out-of-range values are rejected on ingest.
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub confidence: Option<f32>,
    /// (Optional) Delegation subject - the person/principal this agent acts on behalf of (e.g. "ashon"). Principle 2.
    #[serde(default)]
    pub on_behalf_of: Option<String>,
    /// (Optional) Source observation ids this knowledge was derived from - lineage for contamination tracking. Principle 18.
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// (Optional) Entities extracted by the client.
    #[serde(default)]
    pub entities: Vec<EntityInput>,
    /// (Optional) Relations extracted by the client.
    #[serde(default)]
    pub relations: Vec<RelationInput>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct EntityInput {
    /// Canonical name of the entity.
    pub name: String,
    /// Entity type (e.g. Concept, Person, Project, Tool). Defaults to Concept when omitted.
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    /// (Optional) Human-readable explanation of this entity - what it is, so the ontology captures the
    /// definition and not just the name/type. Updates the entity's description (latest wins).
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RelationInput {
    /// Name of the source entity.
    pub from: String,
    /// Relation type (e.g. depends_on, part_of, relates_to). The server canonicalizes the spelling
    /// (depends-on/dependsOn -> depends_on) - spelling variation does not become a different edge.
    #[serde(rename = "type")]
    pub kind: String,
    /// Name of the target entity.
    pub to: String,
    /// (Optional) Human-readable explanation of this connection - what it means / why from relates to to
    /// this way, so the ontology captures the reasoning behind the edge and not just its type.
    #[serde(default)]
    pub description: Option<String>,
    /// (Optional) Valid-time start, epoch millis. When the relation became true in the world - use it
    /// to retroactively record a fact that was true in the past. Interpreted as from the observation time when omitted.
    #[serde(default)]
    pub valid_from: Option<u64>,
    /// (Optional) Valid-time end, epoch millis. Use it to record a fact that has already ended
    /// ("true until last month"). Interpreted as true until refuted when omitted.
    #[serde(default)]
    pub valid_to: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DefineTypeRequest {
    /// Workspace (defaults to the node default when omitted). The T-Box is scoped to the workspace.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Source reference (file path/URL/tool, etc.).
    #[serde(default)]
    pub source_ref: Option<String>,
    /// (Optional) Delegation subject this agent acts on behalf of. Principle 2.
    #[serde(default)]
    pub on_behalf_of: Option<String>,
    /// The type definitions to record (at least one).
    pub defs: Vec<TypeDefItem>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeDefItem {
    /// Which vocabulary: "entity" (a kind of thing, e.g. Driver) or "relation" (a kind of connection, e.g. depends_on).
    pub target: String,
    /// The type name being defined (e.g. Driver, depends_on).
    pub name: String,
    /// Natural-language definition of what this type means. Required (Principle 8: a type has no meaning without it).
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProposeRequest {
    /// Workspace (defaults to the node default when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
    /// Proposal kind, one of:
    /// - `entity_merge` - fold duplicate entities into one canonical id.
    /// - `entity_split` - reverse a merge. The folded entities separate again and the pair stops
    ///   being suggested for merging, without becoming unmergeable.
    /// - `claim_promotion` / `claim_demotion` - raise or lower the trust tier of observations. A
    ///   merged verdict is what the resolution policy consumes, so this settles a contested entity
    ///   kind; demotion is also the revert path for knowledge that turned out wrong.
    /// - `tbox_change` - add or revise a type in the workspace's vocabulary, promoting one that was
    ///   so far only induced from use into one the canon declares. It has no commit effect yet, so it
    ///   can be opened, commented on, rejected or withdrawn, and no surface merges it until it has one.
    /// - `recall` - bulk retraction of everything derived from a contamination source, for when bad
    ///   input has already been built on. It has no commit effect yet either, so no surface merges
    ///   one until it has; then its verdict is a human's direct act, which `review` never casts from
    ///   this surface. Open it, comment on it, and tell the person, who can reject it in the Review
    ///   panel.
    pub kind: String,
    /// Entity/observation ids the proposal acts on (get them from the Review/curation signals or a search hit).
    /// For entity_merge an entity may also be named by its name in the workspace; it is stored as its id.
    /// For entity_split this is exactly one PROPOSAL id - the entity_merge being reversed.
    pub targets: Vec<String>,
    /// For entity_merge: the canonical target id the other targets fold into (must be one of targets).
    #[serde(default)]
    pub into: Option<String>,
    /// For claim_promotion/claim_demotion: the requested trust tier - unverified | agent_extracted |
    /// host_signed | human_confirmed. Required for those kinds; targets are then OBSERVATION ids.
    /// Note: a verdict cast through this MCP surface grants at most host_signed - human_confirmed
    /// requires the human console (a human's direct act, Principle 18).
    #[serde(default)]
    pub tier: Option<String>,
    /// Why (natural language) - the proposal rationale. Required to re-merge a pair that was split:
    /// a proposal is its content, so a verbatim re-open is the same proposal and stays reversed. Say
    /// what makes the split wrong this time.
    #[serde(default)]
    pub rationale: Option<String>,
    /// For tbox_change: the entity/relation types this proposal defines or changes, so the viewer can
    /// highlight the affected nodes (entity types) / edges (relation types) when previewing it. Each
    /// item is {target, name} where target is "entity" or "relation". One name may not be listed on
    /// both axes - the proposal is blocked rather than merely flagged.
    #[serde(default)]
    pub affected_types: Vec<AffectedTypeItem>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub on_behalf_of: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AffectedTypeItem {
    /// Which vocabulary: "entity" (a node type, e.g. Driver) or "relation" (an edge type, e.g. depends_on).
    pub target: String,
    /// The type name being defined or changed (e.g. Driver, depends_on).
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReviewRequest {
    /// The proposal's workspace, where the verdict is filed and counts. On this node it may be left
    /// out, or '*', and the proposal's own is used.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The proposal id (from propose / list_proposals).
    pub proposal: String,
    /// Verdict/action: merge | reject | comment | withdraw.
    pub decision: String,
    /// Optional note/comment.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub on_behalf_of: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListProposalsRequest {
    /// Scope to a workspace (node default when omitted; '*'/'all' means all workspaces).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetProposalRequest {
    #[serde(default)]
    pub workspace: Option<String>,
    /// The proposal id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SyncStatusRequest {
    /// Workspace to report (node default when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SyncPullRequest {
    /// Workspace to pull (node default when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SyncPushRequest {
    /// Workspace to push (node default when omitted; must be on the share whitelist).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetEntityRequest {
    /// Entity id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRequest {
    /// Search scope: "local" (this node only, default), "remote" (the configured sync servers'
    /// ontologies - federated retrieval), or "both". Local state can lag the remote store - use
    /// remote/both when a local miss may just mean "not synced yet". Remote hits are the server's
    /// retrieval surface: sync_pull the workspace to materialize them locally before traverse/get_entity.
    #[serde(default)]
    pub scope: Option<String>,
    /// Search query.
    pub query: String,
    /// Scope to a workspace (all workspaces when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
    /// Maximum number of results (defaults to 20 when omitted).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TraverseRequest {
    /// Start entity id.
    pub id: String,
    /// Maximum number of hops (defaults to 3 when omitted).
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// Maximum number of results (defaults to 100 when omitted).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceMapRequest {
    /// Workspace (defaults to the node default when omitted; '*'/'all' means all workspaces). Knowledge is
    /// organized into named workspaces, so the node default is routinely empty by design - prefer '*' for a
    /// cold-start survey, and read an empty default as a wrong scope rather than an absent ontology.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Maximum number of clusters (defaults to 20 when omitted). Truncated in descending order of size.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Minimum cluster size = number of co-occurring entities (defaults to 2 when omitted). Values of 3 or more exclude trivial pairs.
    #[serde(default)]
    pub min_size: Option<usize>,
}

// --- Server ------------------------------------------------------------------

/// The principal the remote listener authenticated for this request (docs/remote-server.md R2). It
/// rides the HTTP request's extensions, which rmcp hands every handler as `http::request::Parts`.
fn principal_of(context: &RequestContext<RoleServer>) -> Option<remote::Principal> {
    context
        .extensions
        .get::<http::request::Parts>()
        .and_then(|parts| parts.extensions.get::<remote::Principal>().cloned())
}

#[derive(Clone)]
/// Federation wiring handed to the MCP surface (M4 Phase 4, docs/federation.md Section 9).
/// Present only when the node is configured for sync (supragnosis.toml); without it the `sync_*`
/// tools answer with setup guidance instead of failing (P5: absence is explained, not an error).
pub struct SyncContext {
    pub node: Arc<supragnosis_sync::SyncNode>,
    /// Outbound share whitelist (P17/F9) - what may leave this node.
    pub share_workspaces: Vec<String>,
    /// The shared workspaces this node lets a hub serve to its principals (docs/remote-server.md
    /// Section 4.5), said to the hub on every round.
    pub serve_workspaces: Vec<String>,
    /// Keys a hub vouched for at a join, by origin, and the last sequence each verifies in each
    /// workspace (docs/pairing.md Section 4.3.1): their keys are in `origin_keys`, and an event past
    /// the bound - or in a workspace with none - is refused as from an unknown origin.
    pub vouched: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>>,
    /// What a join recorded this node receives (docs/pairing.md Section 4.5, PA12): the round and
    /// `sync_pull` pull these and the shared ones, and no others, whatever a hub admits. `None` on
    /// a node no join set up, which pulls what it is asked to.
    pub receive: Option<Vec<String>>,
    /// The sync servers (hubs) this node talks to, each with the credential presented to it.
    pub servers: Vec<supragnosis_sync::ServerLink>,
    /// Federation configuration that was worked around rather than obeyed, one line each.
    ///
    /// A degrade that only ever reached a startup log is a silent degrade a week later. P5 asks
    /// explicit configuration to work or fail and never to slide quietly; carrying the reason onto
    /// the surface an operator actually reads is what keeps the second half true.
    pub config_notes: Vec<String>,
    /// What each host last said this node may reach. Written by the daemon's background health
    /// loop; read here under a lock. A handler never negotiates - that would add a round trip per
    /// host to a call that already blocks (F11, negotiated-surface.md N1).
    pub surfaces: supragnosis_sync::NegotiatedSurfaces,
    /// Origin-key directory {node_id -> public key hex} for verifying pulled events (F6).
    /// Phase 5 supersedes this with the log-borne canon-policy binding.
    pub origin_keys: std::collections::BTreeMap<String, String>,
    /// Known-peer registry (hub role): runtime observability of who checked in (None on a
    /// client-only node).
    pub peer_registry: Option<std::sync::Arc<supragnosis_sync::http::PeerRegistry>>,
    /// One sync round at a time, whatever starts it: the daemon's own timer or a `sync_*` tool
    /// (docs/pairing.md Section 4.5).
    pub round: Arc<tokio::sync::Mutex<()>>,
    /// What the daemon's last round did, said by `sync_status` (P24, P5).
    pub last_round: Arc<std::sync::RwLock<Option<serde_json::Value>>>,
    /// The hub's listener, `{listening, address, why}` - null on a node with no `[server]`
    /// (docs/pairing.md Section 4.1). Written by the wiring layer whenever it opens or closes.
    pub listener: Arc<std::sync::RwLock<serde_json::Value>>,
}

/// First contact (docs/onboarding.md Section 5): which AI apps have talked to this daemon since it
/// started, by the name each client gives itself. Kept in memory only - never in the store, never on
/// the read tier, never sent to a peer (O7) - so that setup can say an app reached the daemon without
/// writing anything to learn it.
#[derive(Default)]
pub struct ClientLog {
    seen: std::sync::Mutex<std::collections::BTreeMap<String, ClientSeen>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClientSeen {
    pub version: String,
    /// Milliseconds since the epoch, of the first request and of the last.
    pub first: u64,
    pub last: u64,
}

impl ClientLog {
    /// The name a client that gave none is listed under.
    pub const UNNAMED: &'static str = "an unnamed client";

    pub fn note(&self, name: &str, version: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let name = if name.trim().is_empty() { Self::UNNAMED } else { name.trim() };
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        seen.entry(name.to_string())
            .and_modify(|s| {
                s.last = now;
                if !version.is_empty() {
                    s.version = version.to_string();
                }
            })
            .or_insert(ClientSeen { version: version.to_string(), first: now, last: now });
    }

    /// `{"clients": [{"name", "version", "first", "last"}]}`, in name order.
    pub fn snapshot(&self) -> serde_json::Value {
        let seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let list: Vec<serde_json::Value> = seen
            .iter()
            .map(|(name, s)| {
                serde_json::json!({ "name": name, "version": s.version, "first": s.first, "last": s.last })
            })
            .collect();
        serde_json::json!({ "clients": list })
    }
}

pub struct SupragnosisServer {
    engine: Arc<Engine>,
    /// First contact, recorded on the local daemon's surface only (docs/onboarding.md Section 5).
    clients: Option<Arc<ClientLog>>,
    sync: Option<Arc<SyncContext>>,
    /// The hub's listener state, there from the daemon's start, so a node with no sync that an
    /// invitation made a hub still says what its listener is doing (docs/pairing.md Section 4.1).
    listener: Option<Arc<std::sync::RwLock<serde_json::Value>>>,
    /// Set when this server serves principals over the network (docs/remote-server.md Section 4):
    /// every call is then admitted per principal, and one with no principal is refused (R2).
    remote: Option<remote::Surface>,
    tool_router: ToolRouter<SupragnosisServer>,
}

impl SupragnosisServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            clients: None,
            sync: None,
            listener: None,
            remote: None,
            tool_router: Self::tool_router(),
        }
    }

    /// Records which clients talk to this server (docs/onboarding.md Section 5). Ignored on a remote
    /// surface: a hub's listener for principals records nothing (O7).
    pub fn with_clients(mut self, log: Arc<ClientLog>) -> Self {
        self.clients = Some(log);
        self
    }

    /// Notes a request's client in the first-contact log: the name its session's `initialize` gave,
    /// or - in a session that skipped `initialize`, as newer protocol versions allow - the client
    /// information the request carries in its metadata, or an unnamed client.
    fn note_contact(&self, context: &RequestContext<RoleServer>) {
        let (Some(log), None) = (&self.clients, &self.remote) else {
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

    /// Serves principals instead of this machine (docs/remote-server.md). The listener in front
    /// authenticates each request and attaches its [`remote::Principal`]; this server admits each
    /// call against that principal's grants and refuses any call that arrives without one.
    pub fn with_remote(mut self, surface: remote::Surface) -> Self {
        self.remote = Some(surface);
        self
    }

    /// The tool names this server declares, for the test that holds the remote policy table to
    /// them (R3).
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect()
    }

    /// The remote checks for a resource read: an authenticated principal (R2), a granted workspace
    /// (R3) that may be served (R5). A refusal reads as "not found for you", never as absence.
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
    /// Section 29.15). The remote surface refuses `*` before a write reaches the tool (`admit_write`);
    /// this is the same answer for the local one, which used to file into a workspace named `*`.
    fn all_is_no_place(&self, tool: &str, ws: Option<&str>) -> Option<String> {
        match ws {
            Some(w) if remote::current().is_none() && remote::is_all(w) => Some(format!(
                "{tool} writes to one workspace - name it, or leave it out for {}",
                self.engine.default_workspace()
            )),
            _ => None,
        }
    }

    fn remote_resource_ok(
        &self,
        surface: &remote::Surface,
        context: &RequestContext<RoleServer>,
        uri: &str,
    ) -> Result<(), ErrorData> {
        let Some(p) = principal_of(context) else {
            return Err(ErrorData::invalid_request(
                "this surface serves authenticated principals only",
                None,
            ));
        };
        let (ws, by_id) = match parse_resource_uri(uri) {
            Some(ResourceUri::Graph(ws))
            | Some(ResourceUri::Hypergraph(ws))
            | Some(ResourceUri::Types(ws)) => (ws.to_string(), None),
            Some(ResourceUri::Observation(id)) => match self.engine.get_observation(id) {
                Ok(Some(obs)) => (obs.workspace().to_string(), Some(id)),
                _ => return Ok(()), // absent or unreadable: the normal path answers that
            },
            Some(ResourceUri::Entity(id)) => match self.engine.get_entity(id) {
                Ok(Some(view)) => {
                    let ws = view
                        .entity
                        .provenance
                        .first()
                        .map(|p| p.workspace.clone())
                        .unwrap_or_default();
                    if !p.can_read(&ws) {
                        // An entity id is computable from a workspace and a name: one outside the
                        // grants answers exactly as an unknown one (remote-viewer.md Section 3.4).
                        return Err(unknown_entity_resource(id, true));
                    }
                    return (surface.servable)(&ws)
                        .map_err(|why| ErrorData::resource_not_found(why, None));
                }
                _ => return Ok(()), // absent or unreadable: the normal path answers that
            },
            Some(ResourceUri::Proposal(id)) => match self.engine.proposal_workspace(id) {
                Ok(Some(ws)) => {
                    if !p.can_read(&ws) {
                        // Outside the grants answers exactly as an unknown proposal would.
                        return Err(unknown_proposal_resource(id, true));
                    }
                    return (surface.servable)(&ws)
                        .map_err(|why| ErrorData::resource_not_found(why, None));
                }
                _ => return Ok(()), // absent or unreadable: the normal path answers that
            },
            _ => return Ok(()),
        };
        if !p.can_read(&ws) {
            // A workspace URI is refused whether or not the workspace exists. An observation id
            // outside the grants answers exactly as an unknown one: its id is computable from a
            // workspace and a content, so a different answer would confirm what was recorded where
            // (remote-viewer.md Section 3.4).
            return Err(match by_id {
                Some(id) => unknown_observation(id, true),
                None => ErrorData::resource_not_found(
                    format!("{uri} is not in a workspace granted to {}", p.name),
                    None,
                ),
            });
        }
        (surface.servable)(&ws).map_err(|why| ErrorData::resource_not_found(why, None))
    }

    /// For a remote call that resolved a workspace from an id: may its principal read `ws`, and
    /// may `ws` be served at all (R3, R5)? Always `Ok` on the local surface.
    fn remote_sight(&self, ws: &str) -> Result<(), Unseen> {
        let Some(p) = remote::current() else {
            return Ok(());
        };
        if !p.can_read(ws) {
            return Err(Unseen::NotGranted);
        }
        match &self.remote {
            Some(surface) => (surface.servable)(ws).map_err(Unseen::Unservable),
            None => Ok(()),
        }
    }

    /// The answer to an entity id that is unknown - or, remotely, outside the grants, which must
    /// read the same (remote-viewer.md Section 3.4).
    fn unknown_entity(&self, id: &str) -> String {
        self.engine
            .emit(Event::GetEntity { id: id.to_string(), name: None, found: false });
        let note = if remote::current().is_some() {
            "no entity with that id in the workspaces you may read - unknown, not a negation \
             (open-world assumption)"
        } else {
            "unknown - not found is not a negation (open-world assumption)"
        };
        // A name passed as an id is said to be one, from the id's shape alone, so it reads the same
        // on every surface (design-system.md Section 29.20, remote-viewer.md Section 3.4).
        let looks_like_id = id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit());
        let note = if looks_like_id {
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

#[tool_router]
impl SupragnosisServer {
    #[tool(
        description = "Ingest a knowledge fragment - how anything new enters the graph. Stores it as an immutable observation (the source of truth: superseded later, never edited or deleted) and links the entities and relations you extracted alongside it into the ontology. Use it as facts, decisions and the reasons behind them come up; to reshape knowledge that is already stored - merging duplicates, changing trust, retracting - use `propose` instead. Returns the observation id, the linked entity/relation ids and the workspace written to; a write that makes a new workspace adds a note naming the ones that hold knowledge."
    )]
    async fn observe(&self, Parameters(req): Parameters<ObserveRequest>) -> String {
        if let Some(why) = self.all_is_no_place("observe", req.workspace.as_deref()) {
            return err_json(&why);
        }
        // The workspace actually used (node default when omitted) - carried on the event so the viewer knows the scope.
        let workspace = req
            .workspace
            .clone()
            .unwrap_or_else(|| self.engine.default_workspace().to_string());
        let input = ObserveInput {
            content: req.content,
            workspace: req.workspace,
            source_ref: req.source_ref,
            confidence: req.confidence,
            on_behalf_of: req.on_behalf_of,
            derived_from: req.derived_from,
            entities: req
                .entities
                .into_iter()
                .map(|e| EngineEntityInput {
                    name: e.name,
                    kind: e.kind,
                    description: e.description,
                })
                .collect(),
            relations: req
                .relations
                .into_iter()
                .map(|r| EngineRelationInput {
                    from: r.from,
                    kind: r.kind,
                    to: r.to,
                    description: r.description,
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
        };
        // Run the blocking store call on spawn_blocking so concurrent HTTP requests do not starve
        // the tokio workers (harmless with a single stdio client, but essential for a daemon with multiple connections).
        // Whether this write makes a new workspace, asked before it (design-system.md Section 29.20).
        // The remote surface already refuses a workspace outside the write grants.
        let new_ws = remote::current().is_none()
            && matches!(self.engine.holds_observations(&workspace), Ok(false));
        let engine = self.engine.clone();
        let app = writing_app();
        let as_app = app.clone();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(as_app.as_deref(), || engine.observe(input))
        })
        .await
        {
            Ok(Ok(out)) => {
                // UI observability: publish the ingest activity (viewer live log + new-node pulse).
                self.engine.emit_from(
                    app.as_deref(),
                    Event::Observe {
                        observation: out.observation_id.clone(),
                        entities: out.entities.clone(),
                        relations: out.relations.len(),
                        workspace: workspace.clone(),
                    },
                );
                let mut answer = serde_json::to_value(&out).unwrap_or(serde_json::Value::Null);
                answer["workspace"] = workspace.clone().into();
                if new_ws {
                    if let Some(note) = self.new_workspace_note(&workspace) {
                        answer["note"] = note.into();
                    }
                }
                answer.to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(description = "Look up an entity and its relations/provenance by entity id.")]
    async fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        let id = req.id.clone();
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.get_entity(&id)).await {
            Ok(Ok(Some(view))) => {
                // A remote principal sees an entity only in a workspace it holds (R3, R5). The
                // refusal does not claim absence (P5) - only that this caller may not see it.
                let ws =
                    view.entity.provenance.first().map(|p| p.workspace.clone()).unwrap_or_default();
                match self.remote_sight(&ws) {
                    Ok(()) => {}
                    Err(Unseen::NotGranted) => return self.unknown_entity(&req.id),
                    Err(Unseen::Unservable(why)) => return err_json(&why),
                }
                self.engine.emit_from(
                    writing_app().as_deref(),
                    Event::GetEntity {
                        id: req.id.clone(),
                        name: Some(view.entity.canonical_name.clone()),
                        found: true,
                    },
                );
                to_json(&view)
            }
            // Open-world assumption (Principle 5): absence is not falsehood but unknown.
            // We do not return "not found" as an error, so the LLM does not misread absence as negation.
            Ok(Ok(None)) => self.unknown_entity(&req.id),
            // A backend failure differs from absence (Principle 5) - surfaced as an explicit error so it is not misread as "nothing there".
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Search knowledge (entities/observations). The response mode reports the surface actually used: hybrid (semantic+keyword) or keyword (keyword-only degrade - an empty result in this mode may be a recall failure). score is for rank comparison only - its scale differs per mode, so the absolute value is not a confidence."
    )]
    async fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        // The keyword rule takes an empty query for a phrase every text contains, so a blank one
        // would answer the first rows of the store as if they matched.
        if req.query.trim().is_empty() {
            return err_json(
                "query is empty - search for words; to see what a workspace holds, use workspace_map",
            );
        }
        let scope = req.scope.as_deref().unwrap_or("local");
        // Federated retrieval (scope remote/both): query the configured sync servers' ontologies -
        // local state can lag the remote store, so a local miss may just mean "not synced yet".
        let mut remote_results: Vec<serde_json::Value> = Vec::new();
        if scope == "remote" || scope == "both" {
            match &self.sync {
                None => remote_results.push(serde_json::json!({
                    "error": "no sync servers configured on this node (supragnosis.toml [sync])"
                })),
                Some(ctx) => {
                    let ws_name = req
                        .workspace
                        .clone()
                        .unwrap_or_else(|| self.engine.default_workspace().to_string());
                    // A query names what it is about, so it leaves only for a workspace this node
                    // shares, as a push does (sync-correctness.md Section 9). The local half below
                    // still answers.
                    if !ctx.share_workspaces.iter().any(|w| w == &ws_name) {
                        remote_results.push(serde_json::json!({
                            "skipped_workspace": ws_name,
                            "why": "not in [sync] share_workspaces - a query does not leave for a \
                                    workspace this node does not share. Add it there to search \
                                    hosts for it",
                        }));
                    } else {
                        // Ask only hosts that admit this workspace, and name the ones left out. Without
                        // the naming a narrowed recall is indistinguishable from an empty world, which is
                        // the reading P5 exists to prevent (N5); without the narrowing every non-
                        // admitting host answers 403 and the misses drown in denials.
                        let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws_name);
                        if !routed.skipped.is_empty() {
                            remote_results.push(serde_json::json!({
                                "skipped": routed.skipped,
                                "why": "these hosts do not admit this workspace - not consulted, not empty",
                            }));
                        }
                        for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
                            let server = &link.url;
                            let client = match supragnosis_sync::http::SyncClient::for_link(link) {
                                Ok(c) => c,
                                Err(e) => {
                                    remote_results.push(
                                        serde_json::json!({"server": server, "error": e.said()}),
                                    );
                                    continue;
                                }
                            };
                            match client.search(&ws_name, &req.query, req.limit.unwrap_or(20)).await
                            {
                                Ok(resp) => {
                                    self.engine.emit(Event::Sync {
                                        direction: "search".into(),
                                        peer: server.clone(),
                                        workspace: ws_name.clone(),
                                        count: resp.hits.len(),
                                    });
                                    remote_results.push(serde_json::json!({
                                        "server": server,
                                        "mode": resp.mode,
                                        "hits": resp.hits,
                                        "note": "remote retrieval surface - sync_pull this workspace to materialize the hits locally before traverse/get_entity",
                                    }));
                                }
                                Err(e) => remote_results
                                    .push(serde_json::json!({"server": server, "error": e.said()})),
                            }
                        }
                    }
                }
            }
        }
        if scope == "remote" {
            return to_json(&serde_json::json!({ "scope": "remote", "remote": remote_results }));
        }
        // A remote principal's `*` is the union of its grants, searched one workspace at a time and
        // merged by score - never the node-wide search, which would rank across workspaces it may
        // not read (R3). Ties break on id so the merge is deterministic (P16).
        if let (Some(p), Some("*")) = (remote::current(), req.workspace.as_deref()) {
            let limit = req.limit.unwrap_or(20);
            let mut hits = Vec::new();
            let mut mode = None;
            for ws in p.readable() {
                let (engine, query) = (self.engine.clone(), req.query.clone());
                match tokio::task::spawn_blocking(move || engine.search(&query, Some(&ws), limit))
                    .await
                {
                    Ok(Ok(out)) => {
                        mode = Some(out.mode);
                        hits.extend(out.hits);
                    }
                    Ok(Err(e)) => return store_failure_json(&e),
                    Err(e) => return err_json(&format!("task join error: {e}")),
                }
            }
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.id.cmp(&b.id))
            });
            hits.truncate(limit);
            let mut resp = serde_json::json!({ "mode": mode, "hits": self.named_hits(&hits), "workspaces": p.readable() });
            if hits.is_empty() {
                resp["note"] =
                    "no hits in the workspaces granted to you - absence is unknown, not a negation"
                        .into();
            }
            return resp.to_string();
        }
        let query = req.query.clone();
        // `*` is every workspace here, as an omitted one is (design-system.md Section 29.15).
        let ws = req
            .workspace
            .clone()
            .filter(|w| remote::current().is_some() || !remote::is_all(w));
        let limit = req.limit.unwrap_or(20);
        let engine = self.engine.clone();
        let searched =
            tokio::task::spawn_blocking(move || engine.search(&query, ws.as_deref(), limit)).await;
        let searched = match searched {
            Ok(r) => r,
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        match searched {
            Ok(out) => {
                // UI observability: publish the search activity (viewer log + hit-node highlight).
                self.engine.emit_from(
                    writing_app().as_deref(),
                    Event::Search {
                        query: req.query.clone(),
                        workspace: req.workspace.clone(),
                        hits: out.hits.len(),
                        nodes: out.hits.iter().map(|h| h.id.clone()).collect(),
                        mode: match out.mode {
                            SearchMode::Hybrid => "hybrid",
                            SearchMode::Keyword => "keyword",
                        }
                        .to_string(),
                    },
                );
                let mut resp =
                    serde_json::json!({ "mode": out.mode, "hits": self.named_hits(&out.hits) });
                if !remote_results.is_empty() {
                    resp["remote"] = serde_json::Value::Array(remote_results);
                }
                // Open-world assumption (Principle 5): zero hits is unknown, not a negation. For a keyword
                // degrade, we also signal that zero hits is more likely a recall failure, to aid self-correction (Principle 21).
                if out.hits.is_empty() {
                    resp["note"] = serde_json::Value::String(match out.mode {
                        supragnosis_engine::SearchMode::Hybrid => {
                            "no hits - absence is unknown, not a negation (open-world). \
                             The knowledge may not be ingested or phrased differently; \
                             try other terms or traverse from a related entity"
                                .into()
                        }
                        supragnosis_engine::SearchMode::Keyword => {
                            "no hits under keyword-only degrade (semantic retrieval UNAVAILABLE) \
                             - a miss here is weak evidence of absence, not a negation. Every \
                             term must appear (in any order, each as a substring), so retry with \
                             fewer or shorter terms the knowledge would contain"
                                .into()
                        }
                    });
                }
                resp.to_string()
            }
            Err(e) => store_failure_json(&e),
        }
    }

    #[tool(
        description = "Traverse the graph from an entity, following relation direction (from->to). Returns the entities reachable within the maximum hops (max_depth), along with their shortest distance. Reachability runs THROUGH a relation endpoint that no entity row describes yet (partial ingest), but such an endpoint is not listed - so a hit can sit deeper than any listed neighbor. That gap is unknown, not a missing edge."
    )]
    async fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        // A remote walk starts only from an entity the principal may read. Relations never cross a
        // workspace - both endpoints are minted in the observing workspace - so a granted start
        // keeps the whole walk inside the grant.
        if remote::current().is_some() {
            let id2 = req.id.clone();
            let engine2 = self.engine.clone();
            match tokio::task::spawn_blocking(move || engine2.get_entity(&id2)).await {
                Ok(Ok(Some(view))) => {
                    let ws = view
                        .entity
                        .provenance
                        .first()
                        .map(|p| p.workspace.clone())
                        .unwrap_or_default();
                    match self.remote_sight(&ws) {
                        Ok(()) => {}
                        Err(Unseen::NotGranted) => return unknown_remote_start(),
                        Err(Unseen::Unservable(why)) => return err_json(&why),
                    }
                }
                Ok(Ok(None)) => return unknown_remote_start(),
                Ok(Err(e)) => return store_failure_json(&e),
                Err(e) => return err_json(&format!("task join error: {e}")),
            }
        }
        let id = req.id.clone();
        let max_depth = req.max_depth.unwrap_or(3);
        let limit = req.limit.unwrap_or(100);
        let engine = self.engine.clone();
        let traversed =
            tokio::task::spawn_blocking(move || engine.traverse(&id, max_depth, limit)).await;
        let hits = match traversed {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // UI observability: publish the traverse activity (highlight the start + reached nodes).
        self.engine.emit_from(
            writing_app().as_deref(),
            Event::Traverse {
                start: req.id.clone(),
                reached: hits.iter().map(|h| h.id.clone()).collect(),
            },
        );
        let mut resp = serde_json::json!({ "hits": hits });
        // Distinguish the cause of zero hits (Principles 5/21): a missing start entity (unknown) and
        // "exists but has no outgoing relations" are situations the LLM must correct for differently.
        // The confirming get_entity is also blocking, so offload it on spawn_blocking.
        if hits.is_empty() {
            let id2 = req.id.clone();
            let engine2 = self.engine.clone();
            let exists = tokio::task::spawn_blocking(move || engine2.get_entity(&id2)).await;
            resp["note"] = serde_json::Value::String(match exists {
                Ok(Ok(Some(_))) => "start entity exists but reached no entities - it has \
                                    no outgoing relations within max_depth (absence of \
                                    edges is unknown, not a negation)"
                    .into(),
                Ok(Ok(None)) => "start entity id not found - unknown, not a negation \
                                 (open-world). Find the id via search_knowledge first"
                    .into(),
                _ => "empty result; start entity could not be checked due to a \
                      storage failure - do not conclude absence"
                    .into(),
            });
        }
        resp.to_string()
    }

    #[tool(
        description = "Survey a workspace's main co-occurrence contexts (hyperedges - the set of entities asserted together in a single observation). For cold-start orientation: before searching, grasp by name 'what is here and in what clusters'. Clusters are sorted by size (number of co-occurring entities) and sources is the number of supporting observations. This is a directional signal, not an asserted relation - confirm actual relations/details with search_knowledge/get_entity. To solidify a recurring context into first-class structure, observe a group entity (e.g. type Context) with member_of relations from each member and derived_from listing the co-asserting observation ids - the assertion then carries provenance and trust like any other edge."
    )]
    async fn workspace_map(&self, Parameters(req): Parameters<WorkspaceMapRequest>) -> String {
        // Workspace resolution: omitted -> node default, '*'/'all'/'' -> all (None) (same as the graph resource).
        let ws_arg: Option<String> = match req.workspace.as_deref() {
            None => Some(self.engine.default_workspace().to_string()),
            Some("") | Some("*") | Some("all") => None,
            Some(s) => Some(s.to_string()),
        };
        let limit = req.limit.unwrap_or(20);
        // The minimum size cannot go below 2 (size<2 is not a hyperedge).
        let min_size = req.min_size.unwrap_or(2).max(2);
        let engine = self.engine.clone();
        let ws_call = ws_arg.clone();
        let mapped =
            tokio::task::spawn_blocking(move || engine.hypergraph(ws_call.as_deref())).await;
        let hg = match mapped {
            Ok(Ok(hg)) => hg,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // A name-centric, readable summary (Principle 21). Hyperedges are already sorted (size desc, id asc).
        let qualifying = hg.hyperedges.iter().filter(|h| h.size >= min_size).count();
        let clusters: Vec<serde_json::Value> = hg
            .hyperedges
            .iter()
            .filter(|h| h.size >= min_size)
            .take(limit)
            .map(|h| {
                // Names for reading, ids for the next call (P2/P14): the hyperedge id names the
                // cluster stably, and the member ids are what get_entity takes.
                serde_json::json!({
                    "id": h.id,
                    "concepts": h.member_names,
                    "members": h.members,
                    "size": h.size,
                    "sources": h.sources,
                    "trust_tier": h.trust_tier,
                })
            })
            .collect();
        let shown = clusters.len();
        // ws_arg moves into the response below; keep the scope for the empty-result diagnosis.
        let scope = ws_arg.clone();
        let mut resp = serde_json::json!({
            "workspace": ws_arg,
            "clusters": clusters,
            "stats": {
                "node_count": hg.stats.node_count,
                "hyperedge_count": hg.stats.hyperedge_count,
                "max_size": hg.stats.max_size,
                "shown": shown,
                "matched": qualifying,
            },
        });
        // Do not silence truncation (no silent caps) + zero hits is absence != negation (Principle 5).
        if qualifying > shown {
            resp["note"] = serde_json::Value::String(format!(
                "showing top {shown} of {qualifying} clusters (by size). raise limit or lower \
                 min_size to see more. clusters are co-occurrence contexts (entities asserted \
                 together), not asserted relations - drill in with get_entity by member id, or \
                 search_knowledge by concept name"
            ));
        } else if shown == 0 && hg.stats.node_count == 0 {
            // An empty scope is usually a wrong scope, not an absent ontology: the node default workspace is
            // an addressing default, and knowledge is routinely organized into named workspaces instead. A
            // reader who does not already know those names has no next move, so name them here - the dead end
            // corrects itself within the same call rather than requiring a lucky retry with '*'.
            let engine = self.engine.clone();
            let listed = tokio::task::spawn_blocking(move || engine.workspaces()).await;
            let elsewhere: Vec<String> = match listed {
                Ok(Ok(all)) => {
                    all.into_iter().filter(|w| Some(w.as_str()) != scope.as_deref()).collect()
                }
                // Enumeration is a diagnostic nicety, never a reason to fail the survey.
                _ => Vec::new(),
            };
            let where_ = match &scope {
                Some(ws) => format!("workspace '{ws}'"),
                None => "this node".to_string(),
            };
            resp["note"] = serde_json::Value::String(if elsewhere.is_empty() {
                format!(
                    "{where_} holds no entities - nothing has been observed into it yet. absence is \
                     unknown, not a negation (open-world). observe to populate it"
                )
            } else {
                format!(
                    "{where_} holds no entities, but this node does hold knowledge - a scope miss, not \
                     an absent ontology. The node default workspace is only an addressing default; \
                     knowledge is organized into named workspaces. Knowledge exists in: {}. Re-run \
                     scoped to one of those, or workspace '*' to survey all",
                    elsewhere.join(", ")
                )
            });
            resp["knowledge_in_workspaces"] = serde_json::json!(elsewhere);
        } else if shown == 0 {
            resp["note"] = serde_json::Value::String(
                "no co-occurrence clusters at this min_size - absence is unknown, not a negation \
                 (open-world). Lower min_size, widen workspace ('*'), or the workspace may be \
                 sparsely linked (entities observed alone). observe more, or use search_knowledge"
                    .into(),
            );
        }
        resp.to_string()
    }

    #[tool(
        description = "Define ontology types (T-Box) for a workspace: give an entity type or relation type a name and a natural-language definition of what it means. Use this to record the vocabulary of the ontology so a type is not just a bare label but has a stated meaning (e.g. entity type 'Driver' = 'a kernel module that ...'; relation type 'depends_on' = 'X requires Y at runtime'). A description is required. Scoped to the workspace. Read them back via the supragnosis://workspace/{ws}/types resource."
    )]
    async fn define_type(&self, Parameters(req): Parameters<DefineTypeRequest>) -> String {
        if let Some(why) = self.all_is_no_place("define_type", req.workspace.as_deref()) {
            return err_json(&why);
        }
        // Map the string target to the typed axis, rejecting anything else (self-correctable error).
        let mut defs = Vec::with_capacity(req.defs.len());
        for d in req.defs {
            let target = match d.target.trim().to_lowercase().as_str() {
                "entity" => TypeTarget::Entity,
                "relation" => TypeTarget::Relation,
                other => {
                    return err_json(&format!(
                        "unknown target '{other}' for type '{}'. use \"entity\" (a kind of thing) \
                         or \"relation\" (a kind of connection)",
                        d.name
                    ))
                }
            };
            defs.push(EngineTypeDefInput { target, name: d.name, description: d.description });
        }
        let written = req
            .workspace
            .clone()
            .unwrap_or_else(|| self.engine.default_workspace().to_string());
        let input = DefineTypeInput {
            workspace: req.workspace,
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
            defs,
        };
        let engine = self.engine.clone();
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || engine.define_type(input))
        })
        .await
        {
            Ok(Ok(observation_id)) => {
                serde_json::json!({ "observation_id": observation_id, "workspace": written })
                    .to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Open a proposal to change the canon (Principle 23: the gate to canon) - the one path by which anything enters shared knowledge. Use it to reshape what is already there rather than to add: entity_merge and entity_split for identity, claim_promotion and claim_demotion for trust, tbox_change for the type vocabulary, recall for retracting a contaminated derivation tree - those two have no commit effect yet, so neither can be merged until it has one. To state something new, use `observe` instead. A proposal is itself an observation and changes nothing until `review` accepts it; `get_proposal` shows where it stands, and `kind` describes what each one does and which arguments it needs. Returns the proposal id, the workspace, its state and found - true when an identical proposal was already there, since a proposal is its content - with a note saying what is left."
    )]
    async fn propose(&self, Parameters(req): Parameters<ProposeRequest>) -> String {
        if let Some(why) = self.all_is_no_place("propose", req.workspace.as_deref()) {
            return err_json(&why);
        }
        // Map each affected type's string axis to the typed vocabulary (mirror define_type).
        let mut affected_types = Vec::with_capacity(req.affected_types.len());
        for a in req.affected_types {
            let target = match a.target.trim().to_lowercase().as_str() {
                "entity" => TypeTarget::Entity,
                "relation" => TypeTarget::Relation,
                other => {
                    return err_json(&format!(
                        "unknown target '{other}' for affected type '{}'. use \"entity\" \
                         (a node type) or \"relation\" (an edge type)",
                        a.name
                    ))
                }
            };
            affected_types.push(EngineAffectedType { target, name: a.name });
        }
        // Remotely, a gate proposal names only observations its own workspace holds. The engine
        // checks a target against the whole log, so a principal could otherwise learn whether a
        // guessed observation exists in a workspace it was never granted - the id is computable
        // from the workspace and the content (remote-viewer.md Section 3.4). One answer for
        // "absent" and "elsewhere" keeps the two indistinguishable.
        if remote::current().is_some() && GATE_KINDS.contains(&req.kind.as_str()) {
            let ws = req.workspace.clone().unwrap_or_default();
            for t in &req.targets {
                let here =
                    matches!(self.engine.get_observation(t), Ok(Some(o)) if o.workspace() == ws);
                if !here {
                    return err_json(&format!(
                        "target observation '{t}' is not in workspace '{ws}' - a remote proposal \
                         names only observations its own workspace holds"
                    ));
                }
            }
        }
        // A split names a merge of its own workspace, checked the same way.
        if remote::current().is_some() && req.kind == "entity_split" {
            let ws = req.workspace.clone().unwrap_or_default();
            for t in &req.targets {
                let here = matches!(self.engine.proposal_workspace(t), Ok(Some(h)) if h == ws);
                if !here {
                    return err_json(&format!(
                        "merge '{t}' is not a proposal of workspace '{ws}' - a remote split names \
                         only merges its own workspace holds"
                    ));
                }
            }
        }
        // A merge's targets are checked the same way: an entity of another workspace answers exactly
        // as one that does not exist, so a guessed id says nothing about a workspace the principal
        // was never granted. The engine's own refusal, which names the other workspace, is for the
        // local surfaces (design-system.md Section 29.15).
        if remote::current().is_some() && req.kind == "entity_merge" {
            let ws = req.workspace.clone().unwrap_or_default();
            for t in req.targets.iter().chain(req.into.iter()) {
                let id = if t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
                    t.clone()
                } else {
                    supragnosis_core::Entity::make_id(&ws, t)
                };
                let here = matches!(
                    self.engine.store().get_entity(&id),
                    Ok(Some(e)) if e.provenance.first().is_some_and(|p| p.workspace == ws)
                );
                if !here {
                    return err_json(&format!(
                        "target '{t}' is not an entity of workspace '{ws}' - a remote merge names \
                         only entities its own workspace holds"
                    ));
                }
            }
        }
        let written = req
            .workspace
            .clone()
            .unwrap_or_else(|| self.engine.default_workspace().to_string());
        let note = no_effect_note(&req.kind);
        let input = EngineProposeInput {
            workspace: req.workspace,
            kind: req.kind,
            targets: req.targets,
            into: req.into,
            tier: req.tier,
            rationale: req.rationale,
            affected_types,
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
        };
        let engine = self.engine.clone();
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || engine.propose_reporting(input))
        })
        .await
        {
            Ok(Ok(opened)) => {
                let mut answer = serde_json::json!({
                    "proposal_id": opened.id,
                    "workspace": written,
                    "state": opened.state,
                    "found": opened.found,
                });
                let note = [
                    note.map(|n| if opened.found { no_effect_found(&n) } else { n }),
                    opened.found.then(|| found_note(&opened.state)),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                if !note.is_empty() {
                    answer["note"] = note.join(". ").into();
                }
                answer.to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Cast a verdict on a proposal (Principle 23). decision is merge (accept), reject, comment, or withdraw. In a single-user workspace the verdict is self-attested. The state is a deterministic fold of the events; a merge is the absorbing outcome. A claim_promotion merged through this agent surface grants at most host_signed - promotion to human_confirmed is a human's direct act and only the human console can grant it (Principle 18). A merge on a recall is refused here for the same reason (I17), and a merge on a tbox_change or a recall is refused on every surface while the kind has no commit effect (Principle 23: no merge without a diff); comment, reject and withdraw are accepted. Returns the recorded observation id and the workspace it was filed in, the proposal's own."
    )]
    async fn review(&self, Parameters(req): Parameters<ReviewRequest>) -> String {
        let engine = self.engine.clone();
        let (mut ws, prop, dec, note, obo) =
            (req.workspace, req.proposal, req.decision, req.note, req.on_behalf_of);
        // A verdict is filed in its proposal's workspace, the only place it counts (proposal-workflow.md
        // I2). This node may read every workspace, so an omitted one or `*` means the proposal's own;
        // the remote surface has already named the principal's (design-system.md Section 29.15).
        if remote::current().is_none() && ws.as_deref().is_none_or(remote::is_all) {
            match self.engine.proposal_workspace(&prop) {
                Ok(Some(home)) => ws = Some(home),
                Ok(None) => {
                    return err_json(&format!(
                        "no proposal {prop} in any workspace - ids come from propose and list_proposals"
                    ))
                }
                Err(e) => return store_failure_json(&e),
            }
        }
        let written = ws.clone().unwrap_or_else(|| self.engine.default_workspace().to_string());
        // This is the agent-facing surface (resolution.md Section 6): the engine stamps the Agent
        // marker, capping what a merged promotion can grant. Never client-suppliable.
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || {
                engine.review_proposal(
                    ws,
                    prop,
                    dec,
                    note,
                    obo,
                    supragnosis_engine::VerdictSurface::Agent,
                )
            })
        })
        .await
        {
            Ok(Ok(id)) => {
                serde_json::json!({ "observation_id": id, "workspace": written }).to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "List proposals in a workspace (newest first) with their folded state (open/blocked/merged/rejected/withdrawn), kind, targets, verdict count and the workspace each lives in; a merged claim_promotion also carries granted_tier, the tier it granted. A verdict counts only in its proposal's workspace. A tbox_change or a recall carries no_effect_yet: its kind has no commit effect yet, so it cannot be merged."
    )]
    async fn list_proposals(&self, Parameters(req): Parameters<ListProposalsRequest>) -> String {
        let ws: Option<String> = match req.workspace.as_deref() {
            None => Some(self.engine.default_workspace().to_string()),
            Some("") | Some("*") | Some("all") => None,
            Some(s) => Some(s.to_string()),
        };
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.list_proposals(ws.as_deref())).await {
            Ok(Ok(list)) => to_json(&list),
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Get one proposal's folded state (kind/targets/into/rationale/state/verdicts/workspace) by id, with its belief diff and checks, read in the workspace it lives in; a merged claim_promotion also carries granted_tier, the tier it granted. A tbox_change or a recall carries no_effect_yet: its kind has no commit effect yet, so it cannot be merged."
    )]
    async fn get_proposal(&self, Parameters(req): Parameters<GetProposalRequest>) -> String {
        let ws: Option<String> = match req.workspace.as_deref() {
            None => Some(self.engine.default_workspace().to_string()),
            Some("") | Some("*") | Some("all") => None,
            Some(s) => Some(s.to_string()),
        };
        let id = req.id.clone();
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.get_proposal(ws.as_deref(), &id)).await {
            Ok(Ok(Some(p))) => to_json(&p),
            // Open-world (Principle 5): absence is a well-formed answer, as for `get_entity` - an
            // error envelope would read as a failed call rather than as "not here".
            Ok(Ok(None)) => serde_json::json!({
                "found": false,
                "id": req.id,
                "note": "unknown - no proposal with that id in this scope, which is not a negation \
                         (open-world). A proposal lives in one workspace: pass workspace '*' to look \
                         in all of them, or use list_proposals",
            })
            .to_string(),
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    // --- Federation sync tools (M4 Phase 4, docs/federation.md Section 9) ------------------------
    // Administrative surface over the sync client. Wire auth/trust semantics live in the sync layer
    // (F6/F10/F13); these tools only drive rounds and report. Without sync configuration they answer
    // with setup guidance (P5: an unconfigured capability is explained, never a silent failure).

    #[tool(
        description = "Federation sync status of this node: node id, public key, share whitelist, configured sync servers, the workspace's version vector (what this node holds per origin), `config_notes` (federation configuration this build worked around rather than obeyed - read these, they are the reason something is not syncing), and `negotiated` - what each host last said this node may reach, as a three-way difference against the share whitelist (`both` / `local_only` = listed here but not admitted there, a setup error / `peer_only` = admitted there but not shared here). A host with state `unknown` has not answered yet; that is not an empty grant set. Read-only."
    )]
    async fn sync_status(&self, Parameters(req): Parameters<SyncStatusRequest>) -> String {
        let Some(ctx) = &self.sync else {
            // A node that syncs with no hub may still be one: an invitation made its listener
            // while it ran, and the listener's state is said rather than "not configured".
            let listener = self
                .listener
                .as_ref()
                .and_then(|l| l.read().ok().map(|v| v.clone()))
                .filter(|v| v.get("listening").is_some());
            return match listener {
                Some(listener) => serde_json::json!({
                    "sync": "this node syncs with no hub; it is a hub, and its listener is as below",
                    "listener": listener,
                })
                .to_string(),
                None => sync_unconfigured(),
            };
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        let store = self.engine.store();
        let vv = match tokio::task::spawn_blocking({
            let ws = ws.clone();
            move || supragnosis_sync::version_vector(store.as_ref(), &ws)
        })
        .await
        {
            Ok(Ok(vv)) => vv,
            Ok(Err(e)) => {
                return err_json(&format!("store failure while reading the version vector: {e}"))
            }
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // What each host admits for this node, against what this node shares - reported as the
        // three-way difference rather than the intersection, because the disagreement is the part
        // worth seeing: intersecting hides the misconfiguration that produced it (N6).
        let negotiated: Vec<serde_json::Value> = {
            let map = ctx.surfaces.read().ok();
            ctx.servers
                .iter()
                .map(|link| {
                    let entry = map.as_ref().and_then(|m| m.get(&link.url));
                    match entry.and_then(|e| e.admits.as_ref()) {
                        Some(admits) => {
                            let d = supragnosis_sync::surface_diff(&ctx.share_workspaces, admits);
                            serde_json::json!({
                                "server": link.url,
                                "state": "negotiated",
                                "negotiated_at": entry.and_then(|e| e.negotiated_at),
                                "both": d.both,
                                "local_only": d.local_only,
                                "peer_only": d.peer_only,
                            })
                        }
                        None => serde_json::json!({
                            "server": link.url,
                            "state": "unknown",
                            "note": "this host has not answered since the daemon started, or its \
                                     last health check failed. Unknown is not an empty grant set - \
                                     do not read it as a revoked grant",
                        }),
                    }
                })
                .collect()
        };
        let mut status = serde_json::json!({
            "config_notes": ctx.config_notes,
            "negotiated": negotiated,
            "node_id": ctx.node.node_id(),
            "public_key": ctx.node.public_key_hex(),
            "workspace": ws,
            "share_workspaces": ctx.share_workspaces,
            "servers": ctx.servers.iter().map(|l| &l.url).collect::<Vec<_>>(),
            "version_vector": vv,
        });
        // Hub role: the runtime known-peer registry (who checked in, when, how much - resets with
        // the process; the allowlist stays the admission authority).
        if let Some(reg) = &ctx.peer_registry {
            status["known_peers"] = serde_json::to_value(reg.snapshot()).unwrap_or_default();
        }
        // The daemon's own rounds (docs/pairing.md Section 4.5): when the last ran, what it moved,
        // and why it failed - null until one has run.
        status["last_round"] = ctx.last_round.read().map(|l| l.clone()).ok().flatten().into();
        status["listener"] = ctx.listener.read().map(|l| l.clone()).unwrap_or_default();
        to_json(&status)
    }

    #[tool(
        description = "Pull the workspace's missing knowledge from the configured sync servers and apply it (verify signatures -> dedup/absorb -> re-materialize the projection). Returns per-server accepted/rejected counts."
    )]
    async fn sync_pull(&self, Parameters(req): Parameters<SyncPullRequest>) -> String {
        let Some(ctx) = &self.sync else {
            return sync_unconfigured();
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        // A node a join set up pulls what it shares and what the join recorded, whatever a hub
        // would admit (docs/pairing.md Section 4.5, PA12).
        if let Some(receive) = &ctx.receive {
            if !ctx.share_workspaces.contains(&ws) && !receive.contains(&ws) {
                return err_json(&format!(
                    "this node pulls only the workspaces it shares and the ones its join recorded \
                     ({}); {ws} is neither",
                    ctx.share_workspaces
                        .iter()
                        .chain(receive)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        // One round at a time, whoever starts it (docs/pairing.md Section 4.5).
        let _round = ctx.round.lock().await;
        to_json(&pull_workspace(&self.engine, ctx, &ws).await)
    }

    #[tool(
        description = "Push the workspace's local knowledge to the configured sync servers (backfill-stamp unstamped attestations, then send what each server lacks per its advertised version vector). Only share-whitelisted workspaces leave this node."
    )]
    async fn sync_push(&self, Parameters(req): Parameters<SyncPushRequest>) -> String {
        let Some(ctx) = &self.sync else {
            return sync_unconfigured();
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        let _round = ctx.round.lock().await;
        to_json(&push_workspace(&self.engine, ctx, &ws).await)
    }
}

/// One workspace pulled from every host that admits it and applied: verify, absorb, re-materialize.
/// It is what the `sync_pull` tool answers, and what a daemon's own round does (docs/pairing.md,
/// Section 4.5). The caller holds the context's round lock.
pub async fn pull_workspace(
    engine: &Arc<Engine>,
    ctx: &SyncContext,
    ws: &str,
) -> serde_json::Value {
    let ws = ws.to_string();
    // Narrow the round to hosts that admit this workspace, and say which were left out. The two
    // halves ship together on purpose: filtering alone replaces a host's loud `403` with a
    // silent skip, which trades a legible failure for an invisible one (N5).
    let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws);
    let mut results = Vec::new();
    for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
        let server = &link.url;
        let client = match supragnosis_sync::http::SyncClient::for_link(link) {
            Ok(c) => c.with_serve(ctx.serve_workspaces.clone()),
            Err(e) => {
                results.push(serde_json::json!({"server": server, "error": e.said()}));
                continue;
            }
        };
        let store = engine.store();
        let mine = match tokio::task::spawn_blocking({
            let ws = ws.clone();
            move || supragnosis_sync::version_vector(store.as_ref(), &ws)
        })
        .await
        {
            Ok(Ok(vv)) => vv,
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        };
        // Page after page (sync-correctness.md Section 11); each page is applied off the
        // async runtime, as one pull was.
        let pulled = client
            .pull_all(&ws, mine, |events| {
                let (store, node) = (engine.store(), ctx.node.clone());
                let (keys, ws2) = (ctx.origin_keys.clone(), ws.clone());
                let bounds = vouched_bounds(ctx, &ws);
                async move {
                    tokio::task::spawn_blocking(move || {
                        let mut vv = supragnosis_sync::VersionVector::default();
                        node.apply_wire_bounded(
                            store.as_ref(),
                            &ws2,
                            events,
                            &keys,
                            &bounds,
                            None,
                            &mut vv,
                        )
                    })
                    .await
                    .map_err(|e| {
                        supragnosis_sync::http::TransportError::Io(std::io::Error::other(format!(
                            "join: {e}"
                        )))
                    })?
                    .map_err(Into::into)
                }
            })
            .await;
        match pulled {
            Ok(report) => {
                engine.emit(Event::Sync {
                    direction: "pull".into(),
                    peer: server.clone(),
                    workspace: ws.clone(),
                    count: report.accepted,
                });
                results.push(serde_json::json!({
                    "server": server,
                    "applied": report.accepted,
                    "rejected": report.rejected.len(),
                }))
            }
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    // Re-materialize once after all pulls (Prop C: convergence point).
    let engine = engine.clone();
    let ws2 = ws.clone();
    let reprojected = match tokio::task::spawn_blocking(move || engine.reproject(Some(&ws2))).await
    {
        Ok(Ok(r)) => {
            serde_json::json!({"observations": r.observations, "entities": r.entities, "relations": r.relations})
        }
        Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
        Err(e) => serde_json::json!({"error": format!("join: {e}")}),
    };
    serde_json::json!({"workspace": ws, "pulled": results, "skipped": routed.skipped, "reprojected": reprojected})
}

/// One shared workspace pushed to every host that admits it - what the `sync_push` tool answers, and
/// what a daemon's own round does. The caller holds the context's round lock.
pub async fn push_workspace(
    engine: &Arc<Engine>,
    ctx: &SyncContext,
    ws: &str,
) -> serde_json::Value {
    let ws = ws.to_string();
    if !ctx.share_workspaces.iter().any(|w| w == &ws) {
        return serde_json::json!({"error": format!(
            "workspace {ws:?} is not on the share whitelist (P17: nothing leaves by default) - \
             add it to [sync] share_workspaces in supragnosis.toml to share it"
        )});
    }
    // Ask each routed host first. Its copy of this node's own stream floors the seq counter
    // before anything is stamped (sync-correctness.md Section 5), and the version vector it
    // advertises is what the push below is computed against.
    let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws);
    let mut results = Vec::new();
    let mut asked = Vec::new();
    for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
        let server = &link.url;
        let client = match supragnosis_sync::http::SyncClient::for_link(link) {
            Ok(c) => c.with_serve(ctx.serve_workspaces.clone()),
            Err(e) => {
                results.push(serde_json::json!({"server": server, "error": e.said()}));
                continue;
            }
        };
        match client.advertise(&ws).await {
            Ok(remote) => asked.push((server.clone(), client, remote)),
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    {
        let store = engine.store();
        let node = ctx.node.clone();
        let ws2 = ws.clone();
        let seen: Vec<u64> = asked.iter().map(|(_, _, r)| r.vv.get(node.node_id(), &ws)).collect();
        let floored = tokio::task::spawn_blocking(move || {
            seen.into_iter().try_for_each(|s| node.floor_seq(store.as_ref(), &ws2, s))
        })
        .await;
        match floored {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        }
    }
    // Stamp before export (export-boundary stamping, Phase 2).
    let mut reprojected = serde_json::Value::Null;
    {
        let store = engine.store();
        let node = ctx.node.clone();
        let ws2 = ws.clone();
        match tokio::task::spawn_blocking(move || node.backfill(store.as_ref(), &ws2)).await {
            Ok(Err(e)) => {
                return serde_json::json!({"error": format!("backfill stamping failed: {e}")})
            }
            // Stamping rewrites log rows the projection carries, so the graph is behind the log
            // until re-materialized (crash-recovery.md K2). A failure here is not the push's -
            // the rows stay owed and the next open repays them.
            Ok(Ok(stamped)) if stamped > 0 => {
                let engine = engine.clone();
                let ws3 = ws.clone();
                reprojected = match tokio::task::spawn_blocking(move || {
                    engine.reproject(Some(&ws3))
                })
                .await
                {
                    Ok(Ok(r)) => {
                        serde_json::json!({"stamped": stamped, "entities": r.entities, "relations": r.relations})
                    }
                    Ok(Err(e)) => {
                        serde_json::json!({"stamped": stamped, "error": e.to_string()})
                    }
                    Err(e) => {
                        serde_json::json!({"stamped": stamped, "error": format!("join: {e}")})
                    }
                };
            }
            _ => {}
        }
    }
    // The round was narrowed above to hosts that admit this workspace, and `skipped` says which
    // were left out. The two halves ship together on purpose: filtering alone replaces a host's
    // loud `403` with a silent skip, which trades a legible failure for an invisible one (N5).
    for (server, client, remote) in asked {
        let server = &server;
        let store = engine.store();
        let share = ctx.share_workspaces.clone();
        let ws2 = ws.clone();
        let surplus = match tokio::task::spawn_blocking(move || {
            supragnosis_sync::export_delta(store.as_ref(), &ws2, &remote.vv, &share)
        })
        .await
        {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        };
        if surplus.is_empty() {
            results.push(serde_json::json!({"server": server, "pushed": 0}));
            continue;
        }
        match client.push(&ws, surplus).await {
            Ok(resp) => {
                engine.emit(Event::Sync {
                    direction: "push".into(),
                    peer: server.clone(),
                    workspace: ws.clone(),
                    count: resp.accepted,
                });
                results.push(serde_json::json!({
                    "server": server,
                    "pushed": resp.accepted,
                    "rejected": resp.rejected,
                }))
            }
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    serde_json::json!({"workspace": ws, "pushed": results, "skipped": routed.skipped, "reprojected": reprojected})
}

/// One round over the shared workspaces: each pushed, then pulled (docs/pairing.md Section 4.5).
/// It takes the round lock, so a tool's push or pull and the daemon's timer never overlap, and
/// records what it did for `sync_status`: when, how much moved, and the first failure.
pub async fn sync_round(engine: &Arc<Engine>, ctx: &SyncContext) -> serde_json::Value {
    let _round = ctx.round.lock().await;
    let (mut pushed, mut applied, mut errors) = (0u64, 0u64, Vec::<String>::new());
    let count = |v: &serde_json::Value, list: &str, field: &str, errors: &mut Vec<String>| -> u64 {
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            errors.push(e.to_string());
        }
        let mut n = 0;
        for r in v.get(list).and_then(|l| l.as_array()).into_iter().flatten() {
            n += r.get(field).and_then(|x| x.as_u64()).unwrap_or(0);
            if let Some(e) = r.get("error").and_then(|e| e.as_str()) {
                let server = r.get("server").and_then(|s| s.as_str()).unwrap_or("a host");
                errors.push(format!("{server}: {e}"));
            }
        }
        n
    };
    for ws in &ctx.share_workspaces {
        let push = push_workspace(engine, ctx, ws).await;
        pushed += count(&push, "pushed", "pushed", &mut errors);
        let pull = pull_workspace(engine, ctx, ws).await;
        applied += count(&pull, "pulled", "applied", &mut errors);
    }
    // What a join recorded this node receives and does not send: pulled only.
    let received: Vec<&String> = ctx
        .receive
        .iter()
        .flatten()
        .filter(|w| !ctx.share_workspaces.contains(w))
        .collect();
    for ws in &received {
        let pull = pull_workspace(engine, ctx, ws).await;
        applied += count(&pull, "pulled", "applied", &mut errors);
    }
    let workspaces: Vec<&String> = ctx.share_workspaces.iter().chain(received).collect();
    let record = serde_json::json!({
        "at": supragnosis_core::now_millis(),
        "workspaces": workspaces,
        "servers": ctx.servers.len(),
        "pushed": pushed,
        "applied": applied,
        "ok": errors.is_empty(),
        "error": errors.first(),
    });
    if let Ok(mut last) = ctx.last_round.write() {
        *last = Some(record.clone());
    }
    record
}

/// The bounds of the keys a hub vouched for, in one workspace: each vouched origin's last sequence
/// there, and none - nothing verifies - where it gave none (docs/pairing.md Section 4.3.1).
pub fn vouched_bounds(
    ctx: &SyncContext,
    workspace: &str,
) -> std::collections::BTreeMap<String, u64> {
    supragnosis_sync::bounds_in(&ctx.vouched, workspace)
}

/// Guidance answer for the `sync_*` tools on a node with no sync configuration (P5).
fn sync_unconfigured() -> String {
    err_json(
        "federation sync is not configured on this node. Create supragnosis.toml (next to the data \
         dir, or set SUPRAGNOSIS_CONFIG) with a [sync] section - servers, auth_token, \
         share_workspaces - and restart. See docs/federation.md Section 9",
    )
}

// The router is named explicitly so the handler reads the one built once in `new`. Left implicit,
// rmcp calls `Self::tool_router()` instead, which rebuilds the whole table on every tools/list and
// tools/call - and leaves the field dead, which is how the change would announce itself.
#[tool_handler(router = self.tool_router)]
impl ServerHandler for SupragnosisServer {
    /// rmcp's own `initialize`, plus first contact: the session's client name is recorded before
    /// the negotiation (docs/onboarding.md Section 5).
    fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<rmcp::model::InitializeResult, ErrorData>> + Send + '_ {
        if let (Some(log), None) = (&self.clients, &self.remote) {
            log.note(&request.client_info.name, &request.client_info.version);
        }
        context.peer.set_peer_info(request.clone());
        std::future::ready(self.negotiate_initialize(&request))
    }

    fn get_info(&self) -> ServerConfig {
        // Built through the constructor rather than a struct literal: rmcp marks this
        // #[non_exhaustive], so a literal - even one ending in `..Default::default()` - does not
        // compile from outside the crate.
        //
        // The server names itself explicitly. `Implementation::from_build_env()` reads like "this
        // build", but its `env!` expands inside rmcp, so every client was told the server is rmcp
        // at rmcp's version (docs/compatibility.md Section 7).
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(
            Implementation::new("supragnosis", env!("CARGO_PKG_VERSION"))
                .with_website_url("https://supragnosis.dev"),
        )
        .with_instructions(
            "supragnosis: an MCP server that turns knowledge across multiple hosts/workspaces \
                 into an ontology. Ingest knowledge with observe and explore it with \
                 search_knowledge/get_entity/traverse. Survey a workspace's main co-occurrence \
                 contexts (clusters) with workspace_map (orientation before searching). \
                 Workspaces: knowledge is deliberately organized into named workspaces (e.g. one \
                 per project), so the node default workspace is an addressing default and is \
                 routinely empty by design - an empty default means 'wrong scope', never 'no \
                 knowledge here'. Orient with workspace '*' (survey all) before concluding \
                 anything is absent, and scope observe to the workspace the knowledge belongs to \
                 so it is not split across two: name it after the project you work in - the \
                 repository's top-level directory name - and keep using that name. \
                 search_knowledge already spans all workspaces unless you narrow it. \
                 Resources: supragnosis://workspace/{ws}/graph is the full ontology graph \
                 (node-link), supragnosis://workspace/{ws}/hypergraph is the co-occurrence \
                 second-order structure (hyperedges), supragnosis://observation/{id} is the \
                 observation back-reference (raw content + provenance + lineage - use it to \
                 confirm the basis of a search hit), supragnosis://entity/{id} is an entity as \
                 get_entity answers it, and supragnosis://proposal/{id} is a proposal as \
                 get_proposal answers it.",
        )
    }

    /// Tool calls, hand-written so the remote surface admits each one first (docs/remote-server.md
    /// Section 4.3). On the local surface this is exactly what `#[tool_handler]` would generate.
    ///
    /// Remotely: no principal on the request is a refusal (R2); [`remote::admit`] rewrites or refuses
    /// the arguments (R3, R4); every workspace the call will read must be servable (R5); and the call
    /// then runs as its principal, so tools that resolve a workspace from an id can check it too.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.note_contact(&context);
        let Some(surface) = &self.remote else {
            // A local session: what it writes, and the activity it causes, name its app
            // (design-system.md Section 29.8). A server that serves principals never sets it (O7).
            let app = client_name(&context);
            let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
            return flag_error(WRITING_APP.scope(app, self.tool_router.call(tcc)).await);
        };
        let refuse = |why: String| -> Result<CallToolResponse, ErrorData> {
            Ok(CallToolResult::error(vec![ContentBlock::text(
                serde_json::json!({ "error": why }).to_string(),
            )])
            .into())
        };
        let Some(p) = principal_of(&context) else {
            return refuse("this surface serves authenticated principals only".into());
        };
        let mut request = request;
        let mut args = request.arguments.take().unwrap_or_default();
        if let Err(why) = remote::admit(&p, &request.name, &mut args) {
            tracing::info!(principal = %p.name, tool = %request.name, admitted = false, "remote call");
            return refuse(why);
        }
        let ws = args.get("workspace").and_then(|v| v.as_str()).map(str::to_string);
        // R5: the workspaces this call reads must be servable. `propose` too - its answer can
        // describe what the workspace already holds.
        let reads: Vec<String> = match (&ws, remote::policy(&request.name)) {
            (Some(w), Some(remote::Policy::Read)) if remote::is_all(w) => p.readable(),
            (Some(w), Some(remote::Policy::Read)) => vec![w.clone()],
            (Some(w), _) if request.name == "propose" => vec![w.clone()],
            _ => Vec::new(),
        };
        for w in &reads {
            if let Err(why) = (surface.servable)(w) {
                tracing::info!(principal = %p.name, tool = %request.name, workspace = %w, admitted = false, "remote call");
                return refuse(why);
            }
        }
        tracing::info!(principal = %p.name, tool = %request.name, workspace = ?ws, admitted = true, "remote call");
        request.arguments = Some(args);
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        flag_error(remote::as_principal(p, self.tool_router.call(tcc)).await)
    }

    /// Tool listing, hand-written so that the SEP-2549 cache hints are always present.
    ///
    /// `#[tool_handler]` generates this method only when the impl does not define one, and the
    /// version it generates gates `ttlMs`/`cacheScope` on the NEGOTIATED protocol being at least
    /// 2026-07-28 - the version rmcp's own field documentation calls them required at. A client
    /// that negotiates 2025-11-25 and validates them anyway therefore receives a result with both
    /// fields absent and rejects the whole response, which does not degrade the surface, it deletes
    /// it: every tool disappears at once and the failure reads as "tools fetch failed" rather than
    /// as a version disagreement. That is what a Claude Code client does today, and it is how this
    /// was found - a daemon that had not been restarted since before rmcp 3 kept working while the
    /// shipped binary did not.
    ///
    /// Emitting them unconditionally is safe in the other direction: a client older than the field
    /// is a client that ignores unknown members, so nothing below 2026-07-28 can be broken by their
    /// presence, while everything at or above it is owed them.
    ///
    /// `ttl_ms = 0` means "do not treat this as fresh", which is what rmcp hands a 2026-07-28
    /// client and is the answer this server wants regardless: the list is fixed at compile time, so
    /// a long TTL would buy nothing, and it would let a client keep serving a tool list from before
    /// an upgrade that added one.
    ///
    /// `cache_scope` is Private where rmcp's generated version says Public. The list itself is the
    /// same for every caller, but reaching it at all requires this node's bearer token, and
    /// Principle 17's argument for that token is that loopback confines the surface to the host
    /// rather than to a user. Labelling a token-gated response as shareable across authorization
    /// contexts contradicts the reason the token exists, and at `ttl_ms = 0` the honest label costs
    /// nothing because nothing is cached either way.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        self.note_contact(&context);
        std::future::ready(Ok(ListToolsResult::with_all_items(self.tool_router.list_all())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)))
    }

    /// The prompts (docs/prompts.md). Carries the same cache hints as the tool list - a client that
    /// validates the 2025-11-25 schema rejects a list without them. The remote surface lists none
    /// until a digest is scoped to a principal's grants (PR6).
    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, ErrorData>> + Send + '_ {
        let items = if self.remote.is_some() { Vec::new() } else { prompts::list() };
        std::future::ready(Ok(ListPromptsResult::with_all_items(items)
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)))
    }

    /// One prompt: the instruction and a bounded digest of current state. Nothing is written (PR1).
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        if self.remote.is_some() {
            return Err(ErrorData::invalid_request(
                format!(
                    "prompt {:?} is not served on the remote surface yet: its digest reads the \
                     whole workspace, and is not yet scoped to a principal's grants \
                     (docs/prompts.md Section 6). Use the tools, which are",
                    request.name
                ),
                None,
            ));
        }
        let engine = self.engine.clone();
        let args = request.arguments.unwrap_or_default();
        let name = request.name.clone();
        let built = tokio::task::spawn_blocking(move || prompts::get(&engine, &name, &args)).await;
        match built {
            Ok(Ok(result)) => Ok(result.into()),
            Ok(Err(prompts::PromptError::Invalid(why))) => {
                Err(ErrorData::invalid_params(why, None))
            }
            Ok(Err(prompts::PromptError::Store(e))) => Err(ErrorData::internal_error(
                format!("store failure - NOT an empty store (Principle 5): {e}"),
                None,
            )),
            Err(e) => Err(ErrorData::internal_error(format!("task join error: {e}"), None)),
        }
    }

    /// Concrete resource listing: exposes the node's default workspace graph (other workspaces via templates).
    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        // Remotely the concrete resources are the principal's own workspaces, never the node's
        // default - which may not be granted, and whose name alone would be a disclosure (R3).
        if self.remote.is_some() {
            let items: Vec<Resource> = principal_of(&context)
                .map(|p| p.readable())
                .unwrap_or_default()
                .into_iter()
                .map(|ws| {
                    Resource::new(
                        format!("supragnosis://workspace/{ws}/graph"),
                        format!("{ws} ontology graph"),
                    )
                    .with_mime_type("application/json")
                })
                .collect();
            return std::future::ready(Ok(ListResourcesResult::with_all_items(items)
                .with_ttl_ms(0)
                .with_cache_scope(CacheScope::Private)));
        }
        // Workspace discovery entry point - let the client see which workspaces exist first.
        let ws_list = Resource::new("supragnosis://workspaces", "Workspace list")
            .with_description(
                "Sorted list of workspace names that hold knowledge. Entry point for discovering which workspaces exist.",
            )
            .with_mime_type("application/json");

        let ws = self.engine.default_workspace();
        let res = Resource::new(
            format!("supragnosis://workspace/{ws}/graph"),
            format!("{ws} ontology graph"),
        )
        .with_description(
            "Node-link graph of entities (nodes) + relations (edges). Includes provenance/trust tier/valid interval; a read-only derived view.",
        )
        .with_mime_type("application/json");

        // Co-occurrence second-order structure (Principle 11) - also expose the default workspace's hypergraph as a concrete resource.
        let hyper = Resource::new(
            format!("supragnosis://workspace/{ws}/hypergraph"),
            format!("{ws} hypergraph (co-occurrence)"),
        )
        .with_description(
            "Derived view of the entity sets (hyperedges) a single observation asserted together - recovery of the context that binary relations discard (Principle 11).",
        )
        .with_mime_type("application/json");

        // T-Box glossary (Principle 8/11) - the default workspace's type definitions.
        let types = Resource::new(
            format!("supragnosis://workspace/{ws}/types"),
            format!("{ws} type glossary"),
        )
        .with_description(
            "Type vocabulary (T-Box) of the workspace: entity types and relation types with their natural-language definitions (via define_type).",
        )
        .with_mime_type("application/json");
        // Same cache hints as list_tools, and here `ttl_ms = 0` is not merely harmless but
        // required: this list grows when a workspace does, so any freshness window is a window in
        // which a client is told a workspace does not exist. Principle 5 draws that line for the
        // read path and a cache would move it back.
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![
            ws_list, res, hyper, types,
        ])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)))
    }

    /// Resource templates: let clients query any workspace's graph and observation back-references via URI patterns.
    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        let graph = ResourceTemplate::new(
            "supragnosis://workspace/{workspace}/graph",
            "workspace-graph",
        )
        .with_title("Workspace ontology graph")
        .with_description(
            "Entity-relation graph (node-link) of a specific workspace. Fill in {workspace} to query.",
        )
        .with_mime_type("application/json");
        // Co-occurrence second-order structure (Principle 11): the set of entities a single observation asserted together = a hyperedge.
        let hypergraph = ResourceTemplate::new(
            "supragnosis://workspace/{workspace}/hypergraph",
            "workspace-hypergraph",
        )
        .with_title("Workspace hypergraph (co-occurrence second-order structure)")
        .with_description(
            "Derived view that revives, as hyperedges, the entity sets a single observation \
             asserted together - recovery of the context (what was said together) that binary \
             relations discard. Fill in {workspace} to query.",
        )
        .with_mime_type("application/json");
        // T-Box glossary (Principle 8/11): the workspace's type vocabulary + definitions.
        let types =
            ResourceTemplate::new("supragnosis://workspace/{workspace}/types", "workspace-types")
                .with_title("Workspace type glossary (T-Box)")
                .with_description(
                    "Entity types and relation types defined for a workspace, each with its \
             natural-language definition (recorded via define_type). Fill in {workspace} to query.",
                )
                .with_mime_type("application/json");
        // Observation back-reference (Principles 2/14): a path from a search hit's observation id to
        // the raw content + provenance + lineage - the surface that answers "where did this answer come from".
        let observation = ResourceTemplate::new("supragnosis://observation/{id}", "observation")
            .with_title("Observation (raw content + provenance + lineage)")
            .with_description(
                "By observation id (the kind=observation id from a search_knowledge hit), query the \
                 raw content, the full provenance attestation, and the derived_from lineage.",
            )
            .with_mime_type("application/json");
        // An entity by its id (Principle 14): what get_entity answers, dereferenceable as a URI.
        // A proposal by its id (Principle 14), read where it lives, as get_proposal answers it.
        let proposal = ResourceTemplate::new("supragnosis://proposal/{id}", "proposal")
            .with_title("Proposal (targets, verdicts, state, belief diff, checks)")
            .with_description(
                "By proposal id (from propose or list_proposals), query the proposal where it lives, \
                 as get_proposal answers it under workspace '*'.",
            )
            .with_mime_type("application/json");
        let entity = ResourceTemplate::new("supragnosis://entity/{id}", "entity")
            .with_title("Entity (canonical name, type, attestations)")
            .with_description(
                "By entity id (the kind=entity id from a search_knowledge hit or a graph node), \
                 query the entity as get_entity answers it.",
            )
            .with_mime_type("application/json");
        // The template set IS fixed at compile time, unlike the concrete list above, but it gets
        // the same zero: a client that cached templates while refusing to cache the resources they
        // expand over would hold two halves of one answer with different ages.
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            graph,
            hypergraph,
            types,
            observation,
            entity,
            proposal,
        ])
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private)))
    }

    /// Resource read: parse the workspace from the URI and return the graph projection JSON.
    /// An unknown URI returns resource_not_found (absence is unknown, Principle 5) with a self-correction hint.
    ///
    /// Carries the same SEP-2549 cache hints as the three list methods, and for the same reason:
    /// `ttl_ms`/`cache_scope` are `Option` with `skip_serializing_if`, so leaving them unset omits
    /// them from the wire, and a client that validates them rejects the entire response. The
    /// failure is not a narrowed surface but a deleted one - the read returns a schema error
    /// instead of the resource, so every resource becomes unreadable at once and the cause reads
    /// as a malformed server rather than as a version disagreement.
    ///
    /// `ttl_ms = 0` on every branch, including `supragnosis://observation/{id}`. The tempting
    /// exception is that one: an observation is immutable (Principle 3), so its *content* could
    /// carry a long TTL. Its *payload* cannot. `Observation::provenance` is a monotonic union that
    /// absorbs a new attestation whenever the same content re-arrives from another host, and the
    /// trust tier inside it moves under `claim_promotion`/`claim_demotion`. A cached observation
    /// read would therefore pin a provenance list and a tier that the node has since revised,
    /// which is exactly the basis a caller consults this resource to check. The graph, hypergraph,
    /// types and workspace-list branches need no such argument - they change on every `observe`.
    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResponse, ErrorData>> + Send + '_ {
        let uri = request.uri;
        let gate = match &self.remote {
            Some(surface) => self.remote_resource_ok(surface, &context, &uri),
            None => Ok(()),
        };
        let (engine, remote, reader) =
            (self.engine.clone(), self.remote.is_some(), principal_of(&context));
        async move {
            gate?;
            // Every branch reads the store and most fold the whole log (graph, hypergraph,
            // types): synchronous work that holds a worker thread for as long as it takes, and
            // the worker is shared with every other tool call. The tools offload theirs; so does
            // this.
            let read = tokio::task::spawn_blocking(move || read_uri(&engine, uri, remote, reader))
                .await
                .map_err(|e| {
                    ErrorData::internal_error(format!("resource read failed: {e}"), None)
                })??;
            // rmcp 3 wraps the read result in ReadResourceResponse so a server can answer a read
            // with "I need input first" instead of content. This server never does - every branch
            // of `read_uri` either has the bytes or has an error - so every Ok is the Complete arm.
            //
            // The hints are applied here rather than in each branch so that a new resource URI
            // cannot be added without them: there is one place that turns a read into a response.
            Ok(read.with_ttl_ms(0).with_cache_scope(CacheScope::Private).into())
        }
    }
}

/// One resource read: the URI's content, or the error that stands for it. `remote` and `reader`
/// say whether the read came over the hub's agent surface and for which principal; a remote
/// enumeration is filtered to the reader's grants (R3).
fn read_uri(
    engine: &Engine,
    uri: String,
    remote: bool,
    reader: Option<remote::Principal>,
) -> Result<ReadResourceResult, ErrorData> {
    match parse_resource_uri(&uri) {
        // Workspace list (for discovery) - array of workspace names that hold knowledge.
        Some(ResourceUri::Workspaces) => match engine.workspaces() {
            Ok(list) => {
                // Remotely, enumeration is filtered to the grants (R3).
                let list: Vec<String> = match &reader {
                    Some(p) if remote => list.into_iter().filter(|w| p.can_read(w)).collect(),
                    _ => list,
                };
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&list), uri)]))
            }
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Graph(ws)) => {
            match engine.graph(Some(ws).filter(|w| remote || !remote::is_all(w))) {
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
            .hypergraph(Some(ws).filter(|w| remote || !remote::is_all(w)))
        {
            Ok(hg) => Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&hg), uri)])),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        // T-Box glossary (Principle 8/11): the workspace's type definitions (entity + relation types + meanings).
        Some(ResourceUri::Types(ws)) => {
            match engine.types(Some(ws).filter(|w| remote || !remote::is_all(w))) {
                Ok(types) => {
                    Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&types), uri)]))
                }
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            }
        }
        // Observation back-reference (Principles 2/14): whoever knows the id can query the substance (raw content/provenance/lineage).
        Some(ResourceUri::Observation(id)) => match engine.get_observation(id) {
            Ok(Some(obs)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&obs), uri)]))
            }
            Ok(None) => Err(unknown_observation(id, remote)),
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
            Ok(None) => Err(unknown_proposal_resource(id, remote)),
            Err(e) => Err(ErrorData::internal_error(
                format!("storage backend failure (not a missing resource): {e}"),
                None,
            )),
        },
        Some(ResourceUri::Entity(id)) => match engine.get_entity(id) {
            Ok(Some(view)) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(to_json(&view), uri)]))
            }
            Ok(None) => Err(unknown_entity_resource(id, remote)),
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
        if id.is_empty() || id.contains('/') {
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
    /// The client name of the local MCP call in progress, as its session named itself - set only on
    /// a server that serves no principals (design-system.md Section 29.8, onboarding.md O7).
    static WRITING_APP: Option<String>;
}

/// The app of the local call in progress, or `None` outside one (a remote principal's, the CLI's).
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

/// Why a remote call may not see the workspace an id resolved to (remote-viewer.md Section 3.4).
enum Unseen {
    /// Outside the principal's grants. Answered exactly as an unknown id: entity and observation
    /// ids are computable from a workspace and a name or a content, so a distinct answer would
    /// confirm what exists in a workspace the principal was never granted.
    NotGranted,
    /// Granted, but the hub may not serve it (R5). The principal holds the grant, so the refusal
    /// may say why.
    Unservable(String),
}

/// A remote walk whose start is unknown - or outside the grants, which must read the same.
fn unknown_remote_start() -> String {
    serde_json::json!({
        "hits": [],
        "note": "start entity id not found in a workspace you may read - unknown, not a negation. \
                 Find the id via search_knowledge first"
    })
    .to_string()
}

/// An observation id that is not here - or, remotely, not in a workspace the principal may read,
/// which must read the same.
/// A proposal resource no proposal answers - or, remotely, one outside the reader's grants, said
/// the same way (Principle 5, remote-viewer.md Section 3.4).
fn unknown_proposal_resource(id: &str, remote: bool) -> ErrorData {
    let msg = if remote {
        format!(
            "no proposal with id {id} in the workspaces you may read - absence is not a \
             negation (open-world). Use an id from propose or list_proposals"
        )
    } else {
        format!(
            "proposal id not found: {id} - absence is not a negation (open-world). Use an id \
             from propose or list_proposals"
        )
    };
    ErrorData::resource_not_found(msg, None)
}

/// An entity resource no entity answers - or, remotely, one outside the reader's grants, said the
/// same way (Principle 5, remote-viewer.md Section 3.4).
fn unknown_entity_resource(id: &str, remote: bool) -> ErrorData {
    let msg = if remote {
        format!(
            "no entity with id {id} in the workspaces you may read - absence is not a negation \
             (open-world). Use the kind=entity id from a search_knowledge hit or a graph node"
        )
    } else {
        format!(
            "entity id not found: {id} - absence is not a negation (open-world). Use the \
             kind=entity id from a search_knowledge hit or a graph node"
        )
    };
    ErrorData::resource_not_found(msg, None)
}

fn unknown_observation(id: &str, remote: bool) -> ErrorData {
    let msg = if remote {
        format!(
            "no observation with id {id} in the workspaces you may read - absence is not a \
             negation (open-world). Use the kind=observation id from a search_knowledge hit"
        )
    } else {
        format!(
            "observation id not found: {id} - absence is not a negation (open-world). Use the \
             kind=observation id from a search_knowledge hit"
        )
    };
    ErrorData::resource_not_found(msg, None)
}

#[cfg(test)]
mod all_is_a_view {
    //! design-system.md Section 29.15 on the agent surface: `*` is a view, a verdict is filed where
    //! its proposal lives, and a remote merge names only entities of its own workspace.
    use super::*;
    use supragnosis_core::Entity;
    use supragnosis_store::InMemoryStore;

    fn server() -> (Arc<Engine>, SupragnosisServer) {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        for (ws, names) in [
            ("infra", &["Payment service", "Payments"][..]),
            ("secret", &["Payment service"][..]),
        ] {
            engine
                .observe(ObserveInput {
                    content: format!("in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: names
                        .iter()
                        .map(|n| EngineEntityInput {
                            name: (*n).into(),
                            kind: None,
                            description: None,
                        })
                        .collect(),
                    relations: vec![],
                })
                .expect("observe");
        }
        (engine.clone(), SupragnosisServer::new(engine))
    }

    fn req<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).expect("request"))
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("json")
    }

    #[tokio::test]
    async fn star_is_a_view_on_the_local_surface() {
        let (engine, s) = server();
        let out =
            json(&s.observe(req(serde_json::json!({ "content": "x", "workspace": "*" }))).await);
        assert!(
            out["error"].as_str().is_some_and(|e| e.contains("writes to one workspace")),
            "{out}"
        );
        assert!(
            !engine.workspaces().expect("list").iter().any(|w| w == "*"),
            "a workspace named * exists"
        );
        let hits = json(
            &s.search_knowledge(req(serde_json::json!({ "query": "Payments", "workspace": "*" })))
                .await,
        );
        assert!(
            hits["hits"].as_array().is_some_and(|h| !h.is_empty()),
            "`*` searched a workspace named *: {hits}"
        );
        // A verdict with no workspace is filed where its proposal lives.
        let here = Entity::make_id("infra", "Payment service");
        let p = json(
            &s.propose(req(serde_json::json!({
                "workspace": "infra", "kind": "entity_merge",
                "targets": [Entity::make_id("infra", "Payments"), here], "into": here,
            })))
            .await,
        );
        let pid = p["proposal_id"].as_str().expect("proposal id").to_string();
        let v = json(
            &s.review(req(serde_json::json!({ "proposal": pid, "decision": "reject" })))
                .await,
        );
        assert!(v["observation_id"].is_string(), "{v}");
        let state = engine.get_proposal(Some("infra"), &pid).expect("get").expect("proposal").state;
        assert_eq!(state, "rejected", "the verdict was not filed with its proposal");
        let v = json(
            &s.review(req(serde_json::json!({ "proposal": "nope", "decision": "comment" })))
                .await,
        );
        assert!(
            v["error"]
                .as_str()
                .is_some_and(|e| e.contains("no proposal nope in any workspace")),
            "{v}"
        );
    }

    #[tokio::test]
    async fn a_remote_merge_names_only_its_own_workspaces_entities() {
        let (_, s) = server();
        let p = remote::Principal {
            name: "alice".into(),
            read: ["infra".to_string()].into(),
            write: ["infra".to_string()].into(),
        };
        let here = Entity::make_id("infra", "Payment service");
        let elsewhere = Entity::make_id("secret", "Payment service");
        let absent = "ab".repeat(32);
        let mut said = Vec::new();
        for other in [elsewhere, absent] {
            let r = remote::as_principal(
                p.clone(),
                s.propose(req(serde_json::json!({
                    "workspace": "infra", "kind": "entity_merge", "targets": [other.clone(), here], "into": here,
                }))),
            )
            .await;
            let e = json(&r)["error"].as_str().expect("refused").replace(&other, "<id>");
            assert!(
                !e.contains("secret"),
                "the refusal names a workspace the principal was never granted: {e}"
            );
            said.push(e);
        }
        assert_eq!(said[0], said[1], "an entity elsewhere answers differently from an absent one");
        let ok = remote::as_principal(
            p.clone(),
            s.propose(req(serde_json::json!({
                "workspace": "infra", "kind": "entity_merge",
                "targets": ["Payments", "Payment service"], "into": "Payment service",
            }))),
        )
        .await;
        assert!(json(&ok)["proposal_id"].is_string(), "{ok}");
        // A split names a merge of its own workspace: one of another answers as an absent one.
        let secret_merge = s
            .engine
            .propose(EngineProposeInput {
                workspace: Some("secret".into()),
                kind: "entity_merge".into(),
                targets: vec![Entity::make_id("secret", "Payment service"), "cd".repeat(32)],
                into: Some(Entity::make_id("secret", "Payment service")),
                tier: None,
                rationale: None,
                affected_types: vec![],
                source_ref: None,
                on_behalf_of: None,
            })
            .expect("a merge in secret");
        let mut said = Vec::new();
        for other in [secret_merge, "ef".repeat(32)] {
            let r = remote::as_principal(
                p.clone(),
                s.propose(req(serde_json::json!({
                    "workspace": "infra", "kind": "entity_split", "targets": [other.clone()],
                }))),
            )
            .await;
            said.push(json(&r)["error"].as_str().expect("refused").replace(&other, "<id>"));
        }
        assert_eq!(said[0], said[1], "a merge elsewhere answers differently from an absent one");
        assert!(!said[0].contains("secret"), "{}", said[0]);
    }
}

#[cfg(test)]
mod agent_answers {
    //! design-system.md Section 29.20: the agent surface says where it wrote, names its hits, says a
    //! name is one, and names the existing workspaces when a write makes a new one.
    use super::*;
    use supragnosis_store::InMemoryStore;

    fn req<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).expect("request"))
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("json")
    }

    #[tokio::test]
    async fn a_write_says_where_and_a_new_workspace_is_said() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        let first = json(&s.observe(req(serde_json::json!({ "content": "a", "workspace": "acme-api", "entities": [{ "name": "A" }] }))).await);
        assert_eq!(first["workspace"], "acme-api");
        assert!(
            first["note"]
                .as_str()
                .is_some_and(|n| n.contains("this node held no knowledge before")),
            "{first}"
        );
        for ws in ["home-lab", "zeta", "acme-web"] {
            s.observe(req(
                serde_json::json!({ "content": ws, "workspace": ws, "entities": [{ "name": ws }] }),
            ))
            .await;
        }
        let again = json(
            &s.observe(req(serde_json::json!({ "content": "b", "workspace": "acme-api" })))
                .await,
        );
        assert!(
            again.get("note").is_none(),
            "a workspace that holds knowledge is said to be new"
        );
        let typo = json(
            &s.observe(req(serde_json::json!({ "content": "c", "workspace": "acme_api" })))
                .await,
        );
        let note = typo["note"].as_str().expect("a note for a new workspace");
        assert!(
            note.starts_with("workspace acme_api is new; the workspaces that hold knowledge are acme-api, acme-web"),
            "the nearest name is not first: {note}"
        );
        let p = json(&s.propose(req(serde_json::json!({
            "workspace": "acme-api", "kind": "entity_merge", "targets": ["x".repeat(64), "y".repeat(64)], "into": "y".repeat(64),
        }))).await);
        assert!(p["workspace"] == "acme-api" || p["error"].is_string(), "{p}");
    }

    #[tokio::test]
    async fn hits_name_their_entity_and_a_name_is_said_to_be_one() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        for ws in ["billing", "home-lab"] {
            engine
                .observe(ObserveInput {
                    content: format!("PostgreSQL in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EngineEntityInput {
                        name: "PostgreSQL".into(),
                        kind: Some("Database".into()),
                        description: None,
                    }],
                    relations: vec![],
                })
                .expect("observe");
        }
        let s = SupragnosisServer::new(engine);
        let found =
            json(&s.search_knowledge(req(serde_json::json!({ "query": "PostgreSQL" }))).await);
        let named: Vec<(String, String)> = found["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .filter(|h| h["kind"] == "entity")
            .map(|h| {
                (
                    h["workspace"].as_str().unwrap_or("").to_string(),
                    h["name"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        assert!(named.contains(&("billing".into(), "PostgreSQL".into())), "{found}");
        assert!(named.contains(&("home-lab".into(), "PostgreSQL".into())), "{found}");
        let by_name = json(&s.get_entity(req(serde_json::json!({ "id": "PostgreSQL" }))).await);
        assert!(
            by_name["note"].as_str().is_some_and(|n| n.contains("This looks like a name")),
            "{by_name}"
        );
        let by_id = json(&s.get_entity(req(serde_json::json!({ "id": "ab".repeat(32) }))).await);
        assert!(
            !by_id["note"].as_str().unwrap_or("").contains("looks like a name"),
            "an id read as a name"
        );
    }

    /// design-system.md Section 29.23: an identical proposal is the one already there, and the answer
    /// says what it found.
    #[tokio::test]
    async fn a_repeat_says_what_it_found() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        s.observe(req(serde_json::json!({ "content": "a and b", "workspace": "w", "entities": [{ "name": "A" }, { "name": "B" }] }))).await;
        let merge = serde_json::json!({ "workspace": "w", "kind": "entity_merge", "targets": ["A", "B"], "into": "A", "rationale": "one thing" });
        let first = json(&s.propose(req(merge.clone())).await);
        assert_eq!(
            (first["state"].as_str(), first["found"].as_bool()),
            (Some("open"), Some(false)),
            "{first}"
        );
        assert!(first.get("note").is_none());
        let again = json(&s.propose(req(merge.clone())).await);
        assert_eq!(again["proposal_id"], first["proposal_id"]);
        assert_eq!(again["found"], true);
        assert!(
            again["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("an identical proposal is already open")),
            "{again}"
        );
        let id = first["proposal_id"].as_str().expect("id").to_string();
        s.review(req(serde_json::json!({ "proposal": id, "decision": "reject" }))).await;
        let rejected = json(&s.propose(req(merge)).await);
        assert_eq!(rejected["state"], "rejected");
        assert!(
            rejected["note"]
                .as_str()
                .is_some_and(|n| n.contains("a different rationale opens a new one")),
            "{rejected}"
        );
        let change = serde_json::json!({ "workspace": "w", "kind": "tbox_change", "targets": ["A"], "rationale": "a type" });
        s.propose(req(change.clone())).await;
        let found = json(&s.propose(req(change)).await);
        assert!(
            found["note"].as_str().is_some_and(|n| n.starts_with("tbox_change has no commit effect yet, so it cannot be merged until it has one. an identical proposal is already open")),
            "{found}"
        );
    }

    /// docs/pairing.md Section 4.5 and PA12: a node a join set up pulls what it shares and what the
    /// join recorded, and no other workspace, whatever a hub would admit.
    #[tokio::test]
    async fn a_joined_node_pulls_only_what_its_join_recorded() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let ctx = Arc::new(SyncContext {
            node: Arc::new(supragnosis_sync::SyncNode::new(
                supragnosis_core::NodeIdentity::from_secret_bytes([3; 32]),
            )),
            share_workspaces: vec!["acme".into()],
            receive: Some(vec!["acme".into(), "docs".into()]),
            vouched: Default::default(),
            serve_workspaces: Vec::new(),
            servers: Vec::new(),
            config_notes: Vec::new(),
            surfaces: Default::default(),
            origin_keys: Default::default(),
            peer_registry: None,
            round: Default::default(),
            last_round: Default::default(),
            listener: Default::default(),
        });
        let s = SupragnosisServer::new(engine.clone()).with_sync(ctx.clone());
        let refused = json(&s.sync_pull(req(serde_json::json!({ "workspace": "other" }))).await);
        assert!(
            refused["error"].as_str().is_some_and(|e| e.contains("its join recorded")),
            "{refused}"
        );
        let docs = json(&s.sync_pull(req(serde_json::json!({ "workspace": "docs" }))).await);
        assert!(!docs.to_string().contains("its join recorded"), "{docs}");
        let round = sync_round(&engine, &ctx).await;
        assert_eq!(round["workspaces"], serde_json::json!(["acme", "docs"]), "{round}");
    }

    /// docs/pairing.md Section 4.1: a node with no sync that an invitation made a hub says what its
    /// listener is doing, rather than that sync is not configured.
    #[tokio::test]
    async fn a_hub_with_no_sync_says_what_its_listener_does() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let state: Arc<std::sync::RwLock<serde_json::Value>> = Default::default();
        let s = SupragnosisServer::new(engine).with_listener(state.clone());
        let before = json(&s.sync_status(req(serde_json::json!({}))).await);
        assert!(
            before["error"].as_str().is_some_and(|e| e.contains("not configured")),
            "{before}"
        );
        *state.write().unwrap() = serde_json::json!({"listening": false, "address": "0.0.0.0:7420", "why": "nothing is admitted"});
        let after = json(&s.sync_status(req(serde_json::json!({}))).await);
        assert_eq!(after["listener"]["why"], "nothing is admitted", "{after}");
        assert!(after.get("error").is_none());
    }

    /// design-system.md Section 29.22: a kind with no commit effect yet says so when it is opened and
    /// listed, and a merge of it is refused with why.
    #[tokio::test]
    async fn what_has_no_effect_says_so() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        s.observe(req(serde_json::json!({ "content": "a and b", "workspace": "w", "entities": [{ "name": "A" }, { "name": "B" }] }))).await;
        let opened = json(&s.propose(req(serde_json::json!({ "workspace": "w", "kind": "tbox_change", "targets": ["A"], "rationale": "a type" }))).await);
        assert!(
            opened["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("tbox_change has no commit effect yet")),
            "{opened}"
        );
        let merge = json(&s.propose(req(serde_json::json!({ "workspace": "w", "kind": "entity_merge", "targets": ["A", "B"], "into": "A" }))).await);
        assert!(merge.get("note").is_none(), "a kind with an effect carries no note: {merge}");
        let id = opened["proposal_id"].as_str().expect("id").to_string();
        let refused =
            json(&s.review(req(serde_json::json!({ "proposal": id, "decision": "merge" }))).await);
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|e| e.contains("cannot be merged until it has one")),
            "{refused}"
        );
        let listed = json(&s.list_proposals(req(serde_json::json!({ "workspace": "w" }))).await);
        let row = listed
            .as_array()
            .expect("rows")
            .iter()
            .find(|p| p["id"] == id.as_str())
            .expect("row");
        assert_eq!(row["no_effect_yet"], true);
        assert_eq!(row["state"], "open", "the refused merge decided nothing");
    }
}
