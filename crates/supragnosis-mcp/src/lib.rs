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
        if let Some(log) = &self.clients {
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
                 Workspaces: knowledge is organized into named workspaces, one per project. A write \
                 (observe, define_type, propose) names its workspace: name it after the project you \
                 work in - the repository's name, also from a git worktree, never the worktree's \
                 directory - and keep using that name, so one project's knowledge is not split \
                 across two. A read that names no workspace reads every workspace. Before you \
                 change a design or settle a question the project may already have settled, \
                 search its records: two or three English topic words and the project's \
                 workspace; a hit that is an observation opens with \
                 supragnosis://observation/{id}. Confirming \
                 knowledge as human_confirmed is the person's act in the Supragnosis app: never \
                 do it yourself, by any tool or other way. \
                 Resources: supragnosis://workspace/{ws}/graph is the full ontology graph \
                 (node-link), supragnosis://workspace/{ws}/hypergraph is the co-occurrence \
                 second-order structure (hyperedges), supragnosis://observation/{id} is the \
                 observation back-reference (raw content + provenance + lineage - use it to \
                 confirm the basis of a search hit), supragnosis://entity/{id} is an entity as \
                 get_entity answers it, and supragnosis://proposal/{id} is a proposal as \
                 get_proposal answers it.",
        )
    }

    /// Tool calls, hand-written so a session's writes, and the activity they cause, name its app
    /// (design-system.md Section 29.8); otherwise exactly what `#[tool_handler]` would generate.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.note_contact(&context);
        let app = client_name(&context);
        let started = std::time::Instant::now();
        let name = request.name.to_string();
        let arg = |k: &str| {
            request
                .arguments
                .as_ref()
                .and_then(|a| a.get(k))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        let (workspace, source) = (arg("workspace"), arg("source_ref"));
        // How many records an observe names as what it built on (docs/recall.md Section 7.1).
        let cites = request
            .arguments
            .as_ref()
            .and_then(|a| a.get("derived_from"))
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
        let resp = flag_error(WRITING_APP.scope(app.clone(), self.tool_router.call(tcc)).await);
        if self.usage.is_some() {
            let said = match &resp {
                Ok(CallToolResponse::Complete(r)) => answered(
                    &name,
                    r.content.first().and_then(|c| c.as_text()).map(|t| t.text.as_str()),
                    r.is_error == Some(true),
                ),
                Ok(_) => ("ok", None, None),
                Err(_) => ("error", None, None),
            };
            let observe = name == "observe";
            let commit = observe.then(|| commit_of(source.as_deref())).flatten();
            let cites = (observe && cites > 0).then_some(cites);
            // A name the surface does not have is the caller's text, not a function (UL2).
            let known = self.tool_router.list_all().iter().any(|t| t.name == name);
            let name = if known { name.as_str() } else { "unknown" };
            self.note_use(
                "tool",
                name,
                workspace.as_deref(),
                app.as_deref(),
                said,
                serde_json::json!({ "commit": commit, "cites": cites }),
                started,
            );
        }
        resp
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
    /// validates the 2025-11-25 schema rejects a list without them.
    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, ErrorData>> + Send + '_ {
        let items = prompts::list();
        std::future::ready(Ok(ListPromptsResult::with_all_items(items)
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)))
    }

    /// One prompt: the instruction and a bounded digest of current state. Nothing is written (PR1).
    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        let started = std::time::Instant::now();
        let engine = self.engine.clone();
        let args = request.arguments.unwrap_or_default();
        let name = request.name.clone();
        let workspace =
            args.get("workspace").and_then(serde_json::Value::as_str).map(str::to_string);
        // A name the surface does not have is the caller's text, not a prompt (UL2).
        let used = if prompts::PROMPTS.iter().any(|p| p.0 == name) {
            name.clone()
        } else {
            "unknown".into()
        };
        let built = tokio::task::spawn_blocking(move || prompts::get(&engine, &name, &args)).await;
        let failed = !matches!(built, Ok(Ok(_)));
        let app = client_name(&context);
        let said = if failed { ("error", None, None) } else { ("ok", None, None) };
        self.note_use(
            "prompt",
            &used,
            workspace.as_deref(),
            app.as_deref(),
            said,
            serde_json::Value::Null,
            started,
        );
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
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
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
        let engine = self.engine.clone();
        let started = std::time::Instant::now();
        let app = client_name(&context);
        let (kind, workspace, via) = resource_use(&uri);
        async move {
            let read = self.read_resource_inner(engine, uri).await;
            let said = match &read {
                Ok(_) => ("ok", None, None),
                Err(e) if e.code == rmcp::model::ErrorCode::RESOURCE_NOT_FOUND => {
                    ("not_found", None, None)
                }
                Err(_) => ("error", None, None),
            };
            self.note_use(
                "resource",
                kind,
                workspace.as_deref(),
                app.as_deref(),
                said,
                serde_json::json!({ "via": via }),
                started,
            );
            read
        }
    }
}

impl SupragnosisServer {
    async fn read_resource_inner(
        &self,
        engine: Arc<Engine>,
        uri: String,
    ) -> Result<ReadResourceResponse, ErrorData> {
        {
            // Every branch reads the store and most fold the whole log (graph, hypergraph,
            // types): synchronous work that holds a worker thread for as long as it takes, and
            // the worker is shared with every other tool call. The tools offload theirs; so does
            // this.
            let read = tokio::task::spawn_blocking(move || read_uri(&engine, uri)).await.map_err(
                |e| ErrorData::internal_error(format!("resource read failed: {e}"), None),
            )??;
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
