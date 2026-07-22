//! supragnosis-mcp - MCP surface (tools + resources).
//!
//! Defines tools via rmcp macros and delegates to [`supragnosis_engine::Engine`].
//! Tools: `observe`, `get_entity`, `search_knowledge`, `traverse`.
//! Resources: `supragnosis://workspace/{ws}/graph` - read view of the ontology graph (node-link),
//! `supragnosis://observation/{id}` - observation back-reference (raw content + provenance + lineage, Principles 2/14).

use std::future::Future;
use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::{RequestContext, RoleServer},
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::{Deserialize, Serialize};

use supragnosis_engine::{
    DefineTypeInput, Engine, EntityInput as EngineEntityInput, Event, ObserveInput,
    ProposeInput as EngineProposeInput, RelationInput as EngineRelationInput, SearchMode,
    TypeDefInput as EngineTypeDefInput, TypeTarget,
};

// --- Transport DTOs (JSON Schema auto-generated) ----------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ObserveRequest {
    /// Knowledge fragment to ingest (natural language or structured text).
    pub content: String,
    /// Workspace (defaults to the node default when omitted).
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
    /// Proposal kind: entity_merge | claim_promotion | claim_demotion | tbox_change | recall.
    pub kind: String,
    /// Entity/observation ids the proposal acts on (get them from the Review/curation signals or a search hit).
    pub targets: Vec<String>,
    /// For entity_merge: the canonical target id the other targets fold into (must be one of targets).
    #[serde(default)]
    pub into: Option<String>,
    /// Why (natural language) - the proposal rationale.
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub on_behalf_of: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReviewRequest {
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
pub struct GetEntityRequest {
    /// Entity id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchRequest {
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
    /// Workspace (defaults to the node default when omitted; '*'/'all' means all workspaces).
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

#[derive(Clone)]
pub struct SupragnosisServer {
    engine: Arc<Engine>,
    tool_router: ToolRouter<SupragnosisServer>,
}

impl SupragnosisServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl SupragnosisServer {
    #[tool(
        description = "Ingest a knowledge fragment. Stores it as an immutable observation (the source of truth) and links the entities/relations provided alongside it into the ontology. Each entity and each relation may carry an optional description - a human-readable explanation of what the entity is or what the connection means, so the ontology captures the definitions/reasoning, not just names and types. Returns the observation id and the linked entity/relation ids."
    )]
    async fn observe(&self, Parameters(req): Parameters<ObserveRequest>) -> String {
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
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.observe(input)).await {
            Ok(Ok(out)) => {
                // UI observability: publish the ingest activity (viewer live log + new-node pulse).
                self.engine.emit(Event::Observe {
                    observation: out.observation_id.clone(),
                    entities: out.entities.clone(),
                    relations: out.relations.len(),
                    workspace,
                });
                to_json(&out)
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
                self.engine.emit(Event::GetEntity {
                    id: req.id.clone(),
                    name: Some(view.entity.canonical_name.clone()),
                    found: true,
                });
                to_json(&view)
            }
            // Open-world assumption (Principle 5): absence is not falsehood but unknown.
            // We do not return "not found" as an error, so the LLM does not misread absence as negation.
            Ok(Ok(None)) => {
                self.engine.emit(Event::GetEntity {
                    id: req.id.clone(),
                    name: None,
                    found: false,
                });
                serde_json::json!({
                    "found": false,
                    "id": req.id,
                    "note": "unknown - not found is not a negation (open-world assumption)"
                })
                .to_string()
            }
            // A backend failure differs from absence (Principle 5) - surfaced as an explicit error so it is not misread as "nothing there".
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Search knowledge (entities/observations). The response mode reports the surface actually used: hybrid (semantic+keyword) or keyword (keyword-only degrade - an empty result in this mode may be a recall failure). score is for rank comparison only - its scale differs per mode, so the absolute value is not a confidence."
    )]
    async fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let query = req.query.clone();
        let ws = req.workspace.clone();
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
                self.engine.emit(Event::Search {
                    query: req.query.clone(),
                    workspace: req.workspace.clone(),
                    hits: out.hits.len(),
                    nodes: out.hits.iter().map(|h| h.id.clone()).collect(),
                    mode: match out.mode {
                        SearchMode::Hybrid => "hybrid",
                        SearchMode::Keyword => "keyword",
                    }
                    .to_string(),
                });
                let mut resp = serde_json::json!({ "mode": out.mode, "hits": out.hits });
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
                            "no hits under keyword-only degrade (semantic recall UNAVAILABLE) \
                             - a miss here is weak evidence of absence, not a negation. \
                             Try exact terms the knowledge would contain"
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
        description = "Traverse the graph from an entity, following relation direction (from->to). Returns the entities reachable within the maximum hops (max_depth), along with their shortest distance."
    )]
    async fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
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
        self.engine.emit(Event::Traverse {
            start: req.id.clone(),
            reached: hits.iter().map(|h| h.id.clone()).collect(),
        });
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
        description = "Survey a workspace's main co-occurrence contexts (hyperedges - the set of entities asserted together in a single observation). For cold-start orientation: before searching, grasp by name 'what is here and in what clusters'. Clusters are sorted by size (number of co-occurring entities) and sources is the number of supporting observations. This is a directional signal, not an asserted relation - confirm actual relations/details with search_knowledge/get_entity."
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
                serde_json::json!({
                    "concepts": h.member_names,
                    "size": h.size,
                    "sources": h.sources,
                    "trust_tier": h.trust_tier,
                })
            })
            .collect();
        let shown = clusters.len();
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
                 together), not asserted relations - drill in with search_knowledge/get_entity \
                 by concept name"
            ));
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
            defs.push(EngineTypeDefInput {
                target,
                name: d.name,
                description: d.description,
            });
        }
        let input = DefineTypeInput {
            workspace: req.workspace,
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
            defs,
        };
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.define_type(input)).await {
            Ok(Ok(observation_id)) => {
                serde_json::json!({ "observation_id": observation_id }).to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Open a proposal to change the canon (Principle 23: the gate to canon). kind is one of entity_merge (fold duplicate entities into one canonical id), claim_promotion, claim_demotion, tbox_change, recall. A proposal is itself an observation and does not change anything until it is accepted via `review`; use `get_proposal` to see its state (and, once available, its belief diff). For entity_merge, pass the entity ids in `targets` and the canonical one in `into`."
    )]
    async fn propose(&self, Parameters(req): Parameters<ProposeRequest>) -> String {
        let input = EngineProposeInput {
            workspace: req.workspace,
            kind: req.kind,
            targets: req.targets,
            into: req.into,
            rationale: req.rationale,
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
        };
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.propose(input)).await {
            Ok(Ok(id)) => serde_json::json!({ "proposal_id": id }).to_string(),
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Cast a verdict on a proposal (Principle 23). decision is merge (accept), reject, comment, or withdraw. In a single-user workspace the verdict is self-attested. The state is a deterministic fold of the events; a merge is the absorbing outcome. Returns the recorded observation id."
    )]
    async fn review(&self, Parameters(req): Parameters<ReviewRequest>) -> String {
        let engine = self.engine.clone();
        let (ws, prop, dec, note, obo) =
            (req.workspace, req.proposal, req.decision, req.note, req.on_behalf_of);
        match tokio::task::spawn_blocking(move || engine.review_proposal(ws, prop, dec, note, obo)).await {
            Ok(Ok(id)) => serde_json::json!({ "observation_id": id }).to_string(),
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "List proposals in a workspace (newest first) with their folded state (open/merged/rejected/withdrawn), kind, targets, and verdict count."
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

    #[tool(description = "Get one proposal's folded state (kind/targets/into/rationale/state/verdicts) by id.")]
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
            Ok(Ok(None)) => err_json(&format!(
                "proposal not found: {} - absence is not a negation (open-world). Use list_proposals",
                req.id
            )),
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }
}

#[tool_handler]
impl ServerHandler for SupragnosisServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "supragnosis: an MCP server that turns knowledge across multiple hosts/workspaces \
                 into an ontology. Ingest knowledge with observe and explore it with \
                 search_knowledge/get_entity/traverse. Survey a workspace's main co-occurrence \
                 contexts (clusters) with workspace_map (orientation before searching). \
                 Resources: supragnosis://workspace/{ws}/graph is the full ontology graph \
                 (node-link), supragnosis://workspace/{ws}/hypergraph is the co-occurrence \
                 second-order structure (hyperedges), supragnosis://observation/{id} is the \
                 observation back-reference (raw content + provenance + lineage - use it to \
                 confirm the basis of a search hit)."
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    /// Concrete resource listing: exposes the node's default workspace graph (other workspaces via templates).
    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        // Workspace discovery entry point - let the client see which workspaces exist first.
        let mut ws_list = RawResource::new(
            "supragnosis://workspaces".to_string(),
            "Workspace list".to_string(),
        );
        ws_list.description = Some(
            "Sorted list of workspace names that hold knowledge. Entry point for discovering which workspaces exist."
                .to_string(),
        );
        ws_list.mime_type = Some("application/json".to_string());

        let ws = self.engine.default_workspace();
        let mut res = RawResource::new(
            format!("supragnosis://workspace/{ws}/graph"),
            format!("{ws} ontology graph"),
        );
        res.description = Some(
            "Node-link graph of entities (nodes) + relations (edges). Includes provenance/trust tier/valid interval; a read-only derived view."
                .to_string(),
        );
        res.mime_type = Some("application/json".to_string());

        // Co-occurrence second-order structure (Principle 11) - also expose the default workspace's hypergraph as a concrete resource.
        let mut hyper = RawResource::new(
            format!("supragnosis://workspace/{ws}/hypergraph"),
            format!("{ws} hypergraph (co-occurrence)"),
        );
        hyper.description = Some(
            "Derived view of the entity sets (hyperedges) a single observation asserted together - recovery of the context that binary relations discard (Principle 11)."
                .to_string(),
        );
        hyper.mime_type = Some("application/json".to_string());

        // T-Box glossary (Principle 8/11) - the default workspace's type definitions.
        let mut types = RawResource::new(
            format!("supragnosis://workspace/{ws}/types"),
            format!("{ws} type glossary"),
        );
        types.description = Some(
            "Type vocabulary (T-Box) of the workspace: entity types and relation types with their natural-language definitions (via define_type)."
                .to_string(),
        );
        types.mime_type = Some("application/json".to_string());
        std::future::ready(Ok(ListResourcesResult::with_all_items(vec![
            ws_list.no_annotation(),
            res.no_annotation(),
            hyper.no_annotation(),
            types.no_annotation(),
        ])))
    }

    /// Resource templates: let clients query any workspace's graph and observation back-references via URI patterns.
    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        let graph = RawResourceTemplate {
            uri_template: "supragnosis://workspace/{workspace}/graph".to_string(),
            name: "workspace-graph".to_string(),
            title: Some("Workspace ontology graph".to_string()),
            description: Some(
                "Entity-relation graph (node-link) of a specific workspace. Fill in {workspace} to query."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        // Co-occurrence second-order structure (Principle 11): the set of entities a single observation asserted together = a hyperedge.
        let hypergraph = RawResourceTemplate {
            uri_template: "supragnosis://workspace/{workspace}/hypergraph".to_string(),
            name: "workspace-hypergraph".to_string(),
            title: Some("Workspace hypergraph (co-occurrence second-order structure)".to_string()),
            description: Some(
                "Derived view that revives, as hyperedges, the entity sets a single observation \
                 asserted together - recovery of the context (what was said together) that binary \
                 relations discard. Fill in {workspace} to query."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        // T-Box glossary (Principle 8/11): the workspace's type vocabulary + definitions.
        let types = RawResourceTemplate {
            uri_template: "supragnosis://workspace/{workspace}/types".to_string(),
            name: "workspace-types".to_string(),
            title: Some("Workspace type glossary (T-Box)".to_string()),
            description: Some(
                "Entity types and relation types defined for a workspace, each with its \
                 natural-language definition (recorded via define_type). Fill in {workspace} to query."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        // Observation back-reference (Principles 2/14): a path from a search hit's observation id to
        // the raw content + provenance + lineage - the surface that answers "where did this answer come from".
        let observation = RawResourceTemplate {
            uri_template: "supragnosis://observation/{id}".to_string(),
            name: "observation".to_string(),
            title: Some("Observation (raw content + provenance + lineage)".to_string()),
            description: Some(
                "By observation id (the kind=observation id from a search_knowledge hit), query the \
                 raw content, the full provenance attestation, and the derived_from lineage."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
            icons: None,
        };
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(vec![
            graph.no_annotation(),
            hypergraph.no_annotation(),
            types.no_annotation(),
            observation.no_annotation(),
        ])))
    }

    /// Resource read: parse the workspace from the URI and return the graph projection JSON.
    /// An unknown URI returns resource_not_found (absence is unknown, Principle 5) with a self-correction hint.
    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, ErrorData>> + Send + '_ {
        let uri = request.uri;
        let result = match parse_resource_uri(&uri) {
            // Workspace list (for discovery) - array of workspace names that hold knowledge.
            Some(ResourceUri::Workspaces) => match self.engine.workspaces() {
                Ok(list) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&list), uri)],
                }),
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            },
            Some(ResourceUri::Graph(ws)) => match self.engine.graph(Some(ws)) {
                Ok(graph) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&graph), uri)],
                }),
                // A backend failure is surfaced as an internal error, not not_found (absence) - preserving the Principle 5 distinction.
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            },
            // Co-occurrence second-order structure (Principle 11): the entity sets an observation asserted together, as hyperedges.
            Some(ResourceUri::Hypergraph(ws)) => match self.engine.hypergraph(Some(ws)) {
                Ok(hg) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&hg), uri)],
                }),
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            },
            // T-Box glossary (Principle 8/11): the workspace's type definitions (entity + relation types + meanings).
            Some(ResourceUri::Types(ws)) => match self.engine.types(Some(ws)) {
                Ok(types) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&types), uri)],
                }),
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            },
            // Observation back-reference (Principles 2/14): whoever knows the id can query the substance (raw content/provenance/lineage).
            Some(ResourceUri::Observation(id)) => match self.engine.get_observation(id) {
                Ok(Some(obs)) => Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(to_json(&obs), uri)],
                }),
                Ok(None) => Err(ErrorData::resource_not_found(
                    format!(
                        "observation id not found: {id} - absence is not a negation (open-world). \
                         Use the kind=observation id from a search_knowledge hit"
                    ),
                    None,
                )),
                Err(e) => Err(ErrorData::internal_error(
                    format!("storage backend failure (not a missing resource): {e}"),
                    None,
                )),
            },
            None => Err(ErrorData::resource_not_found(
                format!(
                    "unknown resource URI: {uri} - only the forms \
                     supragnosis://workspace/{{workspace}}/graph and \
                     supragnosis://observation/{{id}} are supported"
                ),
                None,
            )),
        };
        std::future::ready(result)
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
    None
}

// --- Serialization helpers (tools return JSON strings) ----------------------

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

fn err_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}
