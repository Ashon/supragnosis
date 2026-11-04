//! The transport request types: what each tool takes, its JSON Schema derived.

use super::*;

// --- Transport DTOs (JSON Schema auto-generated) ----------------------------

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveRequest {
    /// Knowledge fragment to ingest (natural language or structured text).
    pub content: String,
    /// The workspace this fragment belongs to. Name it after the project you work in - the
    /// repository's name, also from a git worktree, never the worktree's directory - and keep using
    /// that name, so one project's knowledge stays in one workspace.
    pub workspace: String,
    /// Source reference (file path/URL/tool, etc.).
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Confidence 0.0-1.0. If omitted, it is preserved as unspecified (no default substitution) -
    /// omit it when you cannot assess it. Out-of-range values are rejected on ingest.
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
    pub confidence: Option<f32>,
    /// (Optional) Delegation subject - the person/principal this agent acts on behalf of (e.g. "ashon").
    #[serde(default)]
    pub on_behalf_of: Option<String>,
    /// (Optional) Ids of the observations this knowledge was derived from: the records you read that shaped it. Lineage, for contamination tracking.
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct DefineTypeRequest {
    /// The workspace whose vocabulary this is, named as for observe. The T-Box is scoped to the workspace.
    pub workspace: String,
    /// Source reference (file path/URL/tool, etc.).
    #[serde(default)]
    pub source_ref: Option<String>,
    /// (Optional) Delegation subject this agent acts on behalf of.
    #[serde(default)]
    pub on_behalf_of: Option<String>,
    /// The type definitions to record (at least one).
    pub defs: Vec<TypeDefItem>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeDefItem {
    /// Which vocabulary: "entity" (a kind of thing, e.g. Driver) or "relation" (a kind of connection, e.g. depends_on).
    pub target: String,
    /// The type name being defined (e.g. Driver, depends_on).
    pub name: String,
    /// Natural-language definition of what this type means. Required: a type has no meaning without it.
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeRequest {
    /// The workspace the proposal acts in, the one its targets live in.
    pub workspace: String,
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
    /// Note: a verdict cast through this surface grants at most host_signed. human_confirmed is the
    /// person's own act in the Supragnosis app: never cast it yourself, by this tool or any other way.
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
#[serde(deny_unknown_fields)]
pub struct AffectedTypeItem {
    /// Which vocabulary: "entity" (a node type, e.g. Driver) or "relation" (an edge type, e.g. depends_on).
    pub target: String,
    /// The type name being defined or changed (e.g. Driver, depends_on).
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ListProposalsRequest {
    /// Scope to a workspace (all workspaces when omitted or '*').
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetProposalRequest {
    /// Scope to a workspace (when omitted or '*', the proposal is read in the workspace it lives in).
    #[serde(default)]
    pub workspace: Option<String>,
    /// The proposal id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncStatusRequest {
    /// Workspace to report (node default when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncPullRequest {
    /// Workspace to pull (node default when omitted).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncPushRequest {
    /// Workspace to push (node default when omitted; must be on the share whitelist).
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEntityRequest {
    /// Entity id.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct WorkspaceMapRequest {
    /// Scope to a workspace (all workspaces when omitted or '*').
    #[serde(default)]
    pub workspace: Option<String>,
    /// Maximum number of clusters (defaults to 20 when omitted). Truncated in descending order of size.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Minimum cluster size = number of co-occurring entities (defaults to 2 when omitted). Values of 3 or more exclude trivial pairs.
    #[serde(default)]
    pub min_size: Option<usize>,
}
