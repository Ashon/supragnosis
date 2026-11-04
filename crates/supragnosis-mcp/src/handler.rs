//! The `ServerHandler` impl: first contact, tool calls and lists, prompts and resources.

use super::*;

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
