//! supragnosis-core - domain model + ports (traits).
//!
//! This crate has **no IO dependencies** (pure domain). Side effects such as storage/embedding/sync
//! are implemented as adapters in other crates against the traits (ports) defined here.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// epoch millis.
pub type Timestamp = u64;

/// Current time (epoch millis). M0 uses the node wall clock - multi-host deterministic ordering (HLC) is introduced in M4.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Provenance trust tier (Principle 18: writes are an attack surface). Low -> high.
/// **Promotion happens only through explicit verification** - a tier does not rise on its own just because time has passed.
/// The variant declaration order is itself the trust ranking, so derive Ord gives "low -> high" directly
/// (used as max in resolution weighting / graph representative-tier computation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    /// Unverified - external/unknown source.
    Unverified,
    /// Extracted/asserted by an agent on a signed host (observe default).
    #[default]
    AgentExtracted,
    /// Signed trusted host.
    HostSigned,
    /// Confirmed by a human.
    HumanConfirmed,
}

/// Provenance - a first-class citizen attached to every fact (Principle 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// The host that actually acted (acting host).
    pub host: String,
    /// Delegation chain (Principle 2): the principal the acting host represents (e.g. "ashon").
    /// If absent, the acting host stands alone - treated as correspondingly less trusted in trust evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// Workspace.
    pub workspace: String,
    /// Original reference (file/URL/tool, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// Observation time = **transaction time** (Principle 4).
    pub observed_at: Timestamp,
    /// Confidence 0.0~1.0. **Unstated (None) is preserved** (Principle 2, 4th revision): substituting a
    /// default value loses the distinction between "not asserted" and "asserted with full confidence" (capture loss).
    /// Interpreting the unstated case (default weighting) is the resolution policy's job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Trust tier (Principle 18). Defaults to `AgentExtracted`; promotion only explicitly.
    #[serde(default)]
    pub trust_tier: TrustTier,
}

/// Structured assertions enclosed in an observation (candidate entities/relations) - `assertions` in architecture.md 2.3.
/// Principle 1: assertions are recorded in the log **verbatim** as the client stated them. Normalization/resolution
/// is the projection's (resolution layer's) job, and replaying the log can recompute them under a different policy at any time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityAssertion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationAssertion>,
    /// Type-vocabulary (T-Box) definitions asserted by this observation (Principle 8/11: an explicit
    /// define_type act, scoped to the workspace). Rides the observation log like any other assertion, so
    /// the glossary is a deterministic projection (no parallel storage - Principle 23) and a future
    /// proposal gate can wrap it without rework.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_defs: Vec<TypeDefAssertion>,
}

/// Which vocabulary a type definition targets - entity types vs relation types (the two T-Box axes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeTarget {
    Entity,
    Relation,
}

impl TypeTarget {
    /// Stable discriminant byte for content-address hashing (order-independent of the serde format).
    fn tag(self) -> u8 {
        match self {
            TypeTarget::Entity => 0,
            TypeTarget::Relation => 1,
        }
    }
}

/// Type definition (T-Box): "the <target> type `name` means <description>". Principle 8 requires a
/// natural-language definition, so `description` is mandatory (not Option). Content identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeDefAssertion {
    pub target: TypeTarget,
    pub name: String,
    pub description: String,
}

/// Feeds a field into the hash with a length-prefix. Delimiter (`\0`) concatenation has ambiguous boundaries, so
/// planting a delimiter in content could construct the same byte stream as a different combination of fields
/// (id hijacking, Principle 18). A length prefix lets the stream itself fix each field's extent, so boundary manipulation is impossible.
fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Option fields are prefixed with a presence byte - None and Some("") are distinguished.
fn hash_opt_field(hasher: &mut blake3::Hasher, v: Option<&str>) {
    match v {
        Some(s) => {
            hasher.update(&[1]);
            hash_field(hasher, s.as_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

/// Option<u64> fields are also encoded as a presence byte + fixed-width LE.
fn hash_opt_u64(hasher: &mut blake3::Hasher, v: Option<u64>) {
    match v {
        Some(x) => {
            hasher.update(&[1]);
            hasher.update(&x.to_le_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

impl Assertions {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.relations.is_empty() && self.type_defs.is_empty()
    }

    /// Deterministic byte encoding for the content-address hash. A hand-rolled encoding not coupled to the
    /// serde format, so the id does not shift even if the serialization library changes.
    /// The count and each field go in with a length-prefix, so an empty assertion set (0,0) is also encoded explicitly.
    ///
    /// Written with a full destructuring (no `..`) (Principle 14: mechanical enforcement of structural evolution): when a
    /// field is added to the assertion structure this becomes a compile error, forcing an explicit decision on "is the new
    /// field content identity (whether it is included in the hash)" - if omitted, two assertions differing only in that
    /// field collapse to the same content address (breaking the "different assertions on the same text mean a different observation" invariant).
    fn hash_into(&self, hasher: &mut blake3::Hasher) {
        let Assertions {
            entities,
            relations,
            type_defs,
        } = self;
        hasher.update(&(entities.len() as u64).to_le_bytes());
        for e in entities {
            let EntityAssertion { name, kind, description } = e;
            hash_field(hasher, name.as_bytes());
            hash_opt_field(hasher, kind.as_deref());
            hash_opt_field(hasher, description.as_deref());
        }
        hasher.update(&(relations.len() as u64).to_le_bytes());
        for r in relations {
            let RelationAssertion {
                from,
                kind,
                to,
                description,
                valid_from,
                valid_to,
            } = r;
            hash_field(hasher, from.as_bytes());
            hash_field(hasher, kind.as_bytes());
            hash_field(hasher, to.as_bytes());
            hash_opt_field(hasher, description.as_deref());
            hash_opt_u64(hasher, *valid_from);
            hash_opt_u64(hasher, *valid_to);
        }
        hasher.update(&(type_defs.len() as u64).to_le_bytes());
        for t in type_defs {
            let TypeDefAssertion { target, name, description } = t;
            hasher.update(&[target.tag()]);
            hash_field(hasher, name.as_bytes());
            hash_field(hasher, description.as_bytes());
        }
    }
}

/// Entity assertion: "there is a thing with this name, and (optionally) it is of this type".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityAssertion {
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// (Optional) A human-readable explanation of this entity - what it is / why it is defined this way.
    /// Content identity (part of the observation id): a different description is a different asserted claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Relation assertion: "from -kind-> to". from/to are names (before resolution), kind is the original notation.
/// The valid interval (Principle 4) is optional - it captures a retroactive observation ("was true until last month") at load time.
/// What the surface does not accept is not recorded in the log, and what is not in the log cannot be
/// restored even by reprojection - the reason the time-travel query logic can be deferred but the capture cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationAssertion {
    pub from: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub to: String,
    /// (Optional) A human-readable explanation of this connection - what it means / why from relates to
    /// to this way. Content identity (part of the observation id): a different description is a different claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Valid-time start (Principle 4). None = interpreted as from the observation time (an approximate default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Timestamp>,
    /// Valid-time end. None = until refuted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

/// Observation - the source of truth. Immutable and identified by **content address**.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    /// List of provenance attestations (Principle 2, at least 1). An observation that re-arrives at the same content
    /// address is not overwritten but absorbed into this list as a **monotonic union** (Principle 3) - [`Observation::absorb`].
    /// Since the content address includes the workspace, every attestation shares the same workspace.
    pub provenance: Vec<Provenance>,
    /// Enclosed structured assertions (Principle 1: the entity/relation graph must be a projection of this log).
    /// **Included in the content-address id computation** - unlike lineage/embedding, assertions are content identity.
    /// Even with the same text, enclosing different assertions makes a different observation (preventing overwrite dedup).
    #[serde(default, skip_serializing_if = "Assertions::is_empty")]
    pub assertions: Assertions,
    /// The source observation ids this observation was derived from (Principle 18: the recall lineage for taint sanitization).
    /// Empty means a primary observation. (Not included in the id computation - lineage is not content identity.)
    ///
    /// **Not verifying the existence of source ids is intentional** (Principle 16: topology-independent convergence):
    /// a source observation may arrive later via sync, so forward references must be allowed -
    /// adding existence verification is a regression that couples semantics to arrival order.
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// (Optional) embedding vector for semantic search (Principle 19: probabilistic boundary).
    /// **Not included in the content-address id computation** - an embedding is only a local aid that widens recall,
    /// not content identity, and using a different model per node does not shake identity/convergence.
    ///
    /// Excluded from serde entirely (Principle 21, symmetric with Entity.embedding): when an observation goes out as an MCP
    /// resource (observation/{id}), hundreds of floats must not pollute the LLM context. Persistence is handled by the store adapter with a hand-rolled encoding.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

impl Observation {
    /// Content-address ID = blake3(workspace + content). The same id no matter the path (server/peer) it comes in through -> dedup.
    pub fn new(content: String, provenance: Provenance) -> Self {
        Self::with_assertions(content, provenance, Assertions::default())
    }

    /// An observation enclosing structured assertions. If the assertions are empty the id is identical to `new`,
    /// and if there are assertions they are included in the id computation. Every field is encoded with a
    /// length-prefix, so boundary manipulation that plants a delimiter in content cannot collide it with another observation.
    pub fn with_assertions(content: String, provenance: Provenance, assertions: Assertions) -> Self {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, provenance.workspace.as_bytes());
        hash_field(&mut hasher, content.as_bytes());
        assertions.hash_into(&mut hasher);
        let id = hasher.finalize().to_hex().to_string();
        Self {
            id,
            content,
            provenance: vec![provenance],
            assertions,
            derived_from: Vec::new(),
            embedding: None,
        }
    }

    /// This observation's workspace. Since the content address includes the workspace, every
    /// attestation carries the same workspace - the first item is representative.
    pub fn workspace(&self) -> &str {
        self.provenance.first().map(|p| p.workspace.as_str()).unwrap_or("")
    }

    /// **Monotonically merges** a re-arrival at the same content address (Principle 3: no overwriting).
    /// Accumulates non-identity fields (provenance attestations, derived_from lineage) as a union -
    /// the union is commutative/associative/idempotent, so it converges to the same result regardless of arrival order (Principle 16).
    /// Relay duplicates (fully identical attestations) are naturally deduped, and independent re-observations (attestations
    /// differing in any field) accumulate. Identity fields (content/assertions) are identical when the id matches, so they
    /// are not touched. An embedding is only a recall aid (Principle 19), so the existing value is
    /// kept and one is only taken when absent.
    pub fn absorb(&mut self, other: Observation) {
        debug_assert_eq!(self.id, other.id, "absorb only between the same content address");
        // Full destructuring (no `..`, Principle 14: mechanical enforcement of structural evolution): when a
        // field is added to Observation (M4: origin/hlc/signature planned) this becomes a compile error,
        // forcing an explicit decision on "how that field is merged on re-arrival" -
        // a silent omission is the loss of that field on merge. Identity fields (id/content/assertions) are
        // discarded since the content address guarantees identity (a `_` binding is also a notation of the decision).
        let Observation {
            id: _,
            content: _,
            provenance,
            assertions: _,
            derived_from,
            embedding,
        } = other;
        self.provenance.extend(provenance);
        self.provenance.sort_by(provenance_order);
        self.provenance
            .dedup_by(|a, b| provenance_order(a, b) == std::cmp::Ordering::Equal);
        self.derived_from.extend(derived_from);
        self.derived_from.sort();
        self.derived_from.dedup();
        if self.embedding.is_none() {
            self.embedding = embedding;
        }
    }
}

/// A deterministic total order over attestations - used for sorting/deduplication of the union. Since it compares
/// all fields, "equal" means only a fully identical attestation (a relay duplicate), and if any field differs
/// (an independent re-observation) it stays separate. confidence is totally ordered via to_bits - the unstated (None) case,
/// by Option's Ord, sorts before any stated value and is a separate attestation distinct from a stated 1.0.
///
/// Written with a full destructuring (no `..`) (Principle 14: mechanical enforcement of structural evolution): when a
/// field is added to Provenance this becomes a compile error, forcing an explicit decision on "is the new field an
/// axis that distinguishes attestations" - if silently omitted from the enumeration, dedup collapses two separate
/// attestations differing only in that field into one (a Principle 3 violation).
fn provenance_order(a: &Provenance, b: &Provenance) -> std::cmp::Ordering {
    fn key(
        p: &Provenance,
    ) -> (
        &str,
        Option<&str>,
        &str,
        Option<&str>,
        Timestamp,
        Option<u32>,
        TrustTier,
    ) {
        let Provenance {
            host,
            on_behalf_of,
            workspace,
            source_ref,
            observed_at,
            confidence,
            trust_tier,
        } = p;
        (
            host.as_str(),
            on_behalf_of.as_deref(),
            workspace.as_str(),
            source_ref.as_deref(),
            *observed_at,
            confidence.map(f32::to_bits),
            *trust_tier,
        )
    }
    key(a).cmp(&key(b))
}

/// Entity (concept node).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// (Optional) Human-readable explanation of this entity, projected from the latest asserting observation
    /// (last-write-wins in M0, symmetric with `kind`). Not part of the id - the full history stays in the log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
    /// (Optional) embedding vector for semantic search (Principle 19: probabilistic boundary). A recall aid that lets the
    /// node be reached by the meaning of its name/aliases - like observations, **not included in the id computation** (it is
    /// recall expansion, not identity, and using a different model per node does not shake identity/convergence).
    ///
    /// Excluded from serde entirely (Principle 21): this vector is only an internal recall machine, so leaking it through the MCP
    /// surface (get_entity) would pollute the LLM context with hundreds of floats. Persistence is handled by the store
    /// adapter with a hand-rolled encoding (Cozo data JSON), so it is not a domain serialization target.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

impl Entity {
    /// Deterministic entity ID = blake3(workspace + normalized_name), length-prefix encoding.
    /// M0 resolution rule: exact canonical-name match (case/whitespace normalized). Embedding-similarity resolution is M3.
    pub fn make_id(workspace: &str, canonical_name: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, workspace.as_bytes());
        hash_field(&mut hasher, canonical_name.trim().to_lowercase().as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// Typed relation (edge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// (Optional) Human-readable explanation of this connection, projected from the latest asserting
    /// observation (last-write-wins in M0). Not part of the id - the full history stays in the log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub provenance: Provenance,
    /// Valid time (Principle 4): the start of the interval during which the relation is true in the world.
    /// None = "from the observation time (provenance.observed_at) until refuted".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Timestamp>,
    /// Valid-time end. When refuted, this value is set rather than deleting (supersede, Principle 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

/// Deterministic normalization (canonical form) of relation type notation. In a system where LLM extractors are the primary
/// clients, notation jitter (`depends_on`/`dependsOn`/`depends-on`/`Depends On`) is a constant, so it is
/// converged to a single canonical form (`depends_on`) before being baked into the id.
///
/// Rules: trim -> a run of separators (`-`, `_`, whitespace) becomes a single `_` -> insert `_` at camelCase boundaries
/// (an uppercase letter after a lowercase/digit) -> all lowercase.
/// A pure function - projecting in any order on any node yields the same result (Principle 16).
pub fn normalize_relation_kind(kind: &str) -> String {
    let mut out = String::with_capacity(kind.len() + 4);
    let mut pending_sep = false;
    let mut prev: Option<char> = None;
    for ch in kind.trim().chars() {
        if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !out.is_empty() {
                pending_sep = true;
            }
            continue;
        }
        if ch.is_uppercase() {
            if let Some(p) = prev {
                if p.is_lowercase() || p.is_numeric() {
                    pending_sep = true;
                }
            }
        }
        if pending_sep {
            out.push('_');
            pending_sep = false;
        }
        for lc in ch.to_lowercase() {
            out.push(lc);
        }
        prev = Some(ch);
    }
    out
}

impl Relation {
    /// Deterministic relation ID = blake3(from + normalized_kind + to), length-prefix encoding.
    /// kind goes through [`normalize_relation_kind`], so notation jitter converges to the same edge id.
    /// (from/to are already-resolved canonical entity ids.)
    pub fn make_id(from: &str, kind: &str, to: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, from.as_bytes());
        hash_field(&mut hasher, normalize_relation_kind(kind).as_bytes());
        hash_field(&mut hasher, to.as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// Deterministic identifier for a hyperedge (co-occurrence second-order structure, Principle 11 "the ground for induction") =
/// blake3(member count + sorted member ids), length-prefix encoding. A hyperedge is a projection rather than a stored
/// element, so core holds only the id derivation (a pure function) and the view type is owned by the engine. Since **the
/// member set is the identity** (Principle 14), the same set converges to the same id no matter which observation it is
/// derived from - observations are attestations of that set (Principle 3). The caller passes a sorted/deduplicated id
/// slice. Being length-prefixed, member boundaries are not ambiguous (same hashing discipline as [`Observation`] - Principle 18).
pub fn hyperedge_id(sorted_member_ids: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(sorted_member_ids.len() as u64).to_le_bytes());
    for id in sorted_member_ids {
        hash_field(&mut hasher, id.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: SearchHitKind,
    pub id: String,
    pub snippet: String,
    /// Ranking score - **for ranking comparison only**. The scale differs per search surface (keyword constant,
    /// cosine similarity, RRF fusion value). Interpreting the absolute value as confidence/certainty is a misreading.
    pub score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Entity,
    Observation,
}

/// A single graph-traversal result (an entity `depth` hops away from the start entity).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseHit {
    pub id: String,
    pub depth: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// Storage port. Implemented by adapters such as in-memory / Cozo (RocksDB).
///
/// **Read contract** (the duty of every adapter):
/// - **Distinguishing absence from failure (Principle 5)**: a backend failure propagates as `Err`. Swallowing a failure as an
///   empty result (`Ok(vec![])`/`Ok(None)`) leaves the caller unable to distinguish "not found (unknown)" from "cannot
///   query (failure)" - the premise of distinguishing absence/negation/failure collapses at the storage layer. A partial
///   failure is also `Err`, not a partial result.
/// - **Reproducibility (Principle 16)**: the same query on the same state gives the same response. Sorting and limit
///   truncation are pinned to a stable key (id), and it is a contract violation if the iteration order of internal data
///   structures (hash map / row order) leaks into the response.
pub trait KnowledgeStore: Send + Sync {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    /// Restores an observation by id from the observation log - the back-reference path for search hits / derivation
    /// lineage (Principle 2/14: whoever knows the id can reach the entity and its provenance) and the reference read for
    /// re-arrival merging. A missing id is `Ok(None)`, a backend failure is `Err`.
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError>;
    /// A missing id is `Ok(None)` (absence, the unknown of Principle 5) - only a backend failure is `Err`.
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError>;
    /// Upsert keyed on entity.id.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// Relations whose from or to is entity_id.
    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError>;
    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError>;
    /// Entities reachable from start_id following the direction (from->to) up to `max_depth` hops.
    fn traverse(
        &self,
        start_id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError>;
    /// Enumerates all entities in the workspace (the read path of the graph projection). `None` means all.
    /// The node set for ontology visualization/observability - a full enumeration, not a query term like search.
    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError>;
    /// Enumerates all relations in the workspace (the edge set of the graph projection). `None` means all.
    /// A relation's workspace is determined by provenance.workspace.
    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError>;
    /// Enumerates all observations in the workspace (the entire log). `None` means all. A full enumeration,
    /// not a query term like search - absence is an empty Vec. Since log replay / co-occurrence induction is structurally
    /// impossible with point gets alone, enumeration is placed on the port - it is the common read path for hyperedges
    /// (second-order structure, Principle 11) and reprojection (Principle 1).
    ///
    /// **degrade contract**: a restore failure of an individual row (some legacy/corrupted row) does not block the whole
    /// enumeration - it excludes that row and returns the rest but **does not stay silent** (it logs the exclusion,
    /// the degrade spirit of Principle 19). This is so a derived overlay (hyperedge) is not made entirely unusable because of
    /// one row. In contrast, a point-read ([`KnowledgeStore::get_observation`]) is fail-fast
    /// (being the reference read for re-arrival merging, mistaking a failure for absence destroys attestations, Principle 3).
    /// A query/schema-level backend failure (not an individual row) is still `Err`.
    /// Note: a reprojection that needs completeness (M3) must treat excluded rows as restore/recovery, not a drop.
    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError>;
    /// Searches observations that have an embedding by cosine similarity to the query vector (Principle 19: recall expansion).
    /// Observations without an embedding are excluded from the candidates. `score` is cosine similarity (-1.0~1.0).
    /// The default implementation returns an empty result - an adapter that does not store vectors need not override it.
    fn search_semantic(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
    }

    /// Searches entities that have an embedding by cosine similarity to the query vector (Principle 19: recall expansion).
    /// Lets the node be reached by the **meaning of the entity's name/aliases** - it is recalled even if no observation
    /// mentions that node lexically (filling the recall gap of observation-only semantics). Returns `SearchHitKind::Entity`
    /// hits, and `score` is cosine similarity. The default implementation returns an empty result.
    fn search_semantic_entities(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}

/// Embedding provider port (Principle 19: probabilistic boundary). The core knows only this port,
/// and the actual model (fastembed/remote, etc.) is implemented by a swappable adapter. Without one, it degrades to keyword search.
pub trait EmbeddingProvider: Send + Sync {
    /// Dimension of the embedding vector.
    fn dimensions(&self) -> usize;
    /// Stable identifier of the embedder (model name + dimension, e.g. "hashing-256", "bge-small-en-v1.5-384").
    /// The store records it alongside the vector index to detect a swap that reopens with a different embedder -
    /// because a different model means a different vector space, so mixing old/new vectors in one index makes
    /// similarity meaningless (Principle 19: an adapter swap must not harm core correctness).
    fn id(&self) -> String;
    /// Embeds a batch of texts. Input order and output order correspond 1:1.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    /// Convenience method for embedding a single text.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut v = self.embed(&[text])?;
        v.pop()
            .ok_or_else(|| EmbedError::Provider("empty embedding result".into()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding provider error: {0}")]
    Provider(String),
}

/// Cosine similarity of two vectors (-1.0~1.0). 0.0 if the lengths differ or a vector is a zero vector.
/// A pure function - shared by the InMemory adapter and recall evaluation.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// UI activity event (observability). Emitted by MCP tool calls **per intent**, and consumed by the viewer
/// for real-time log / node highlighting. The core defines only the data type + port (Principle 20) -
/// the actual transport (SSE/broadcast) is implemented by an adapter as [`EventSink`]. The node id lists
/// (entities/reached/id) are matched against graph nodes by the viewer for pulse/focus.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Knowledge load: observation + linked entities/relations.
    Observe {
        observation: String,
        entities: Vec<String>,
        relations: usize,
        workspace: String,
    },
    /// Search: query + hits. `nodes` are hit ids (the viewer matches them against graph nodes to highlight -
    /// observation ids are not nodes, so they do not match and are ignored).
    Search {
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        hits: usize,
        nodes: Vec<String>,
        mode: String,
    },
    /// Entity lookup.
    GetEntity {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        found: bool,
    },
    /// Graph traversal: start node + reached nodes.
    Traverse {
        start: String,
        reached: Vec<String>,
    },
}

/// Event envelope: event + **session id** (the grouping key for a conversation footprint). To view together which
/// knowledge was used within the same session. `event` is flattened and serialized as `{session, kind, ...}` -
/// the viewer reads `ev.session` and `ev.kind` flat.
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    /// Session id (a conversation, roughly one MCP server run unit, or injected by the client). The footprint grouping key.
    pub session: String,
    #[serde(flatten)]
    pub event: Event,
}

/// UI event sink port (Principle 20). Held by the engine and emitted by tool calls. Without one it is a no-op -
/// observability is optional, and core correctness does not depend on this port (the spirit of Principle 19).
pub trait EventSink: Send + Sync {
    fn emit(&self, env: &EventEnvelope);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_kind_normalization_converges() {
        // Notation jitter with the same meaning all goes to a single canonical form.
        for variant in [
            "depends_on",
            "dependsOn",
            "depends-on",
            "Depends On",
            " depends  on ",
            "DEPENDS_ON",
            "depends--on",
        ] {
            assert_eq!(
                normalize_relation_kind(variant),
                "depends_on",
                "variant {variant:?} should normalize to depends_on"
            );
        }
        // Already canonical means unchanged (idempotent).
        assert_eq!(
            normalize_relation_kind(&normalize_relation_kind("dependsOn")),
            "depends_on"
        );
        assert_eq!(normalize_relation_kind("relates_to"), "relates_to");
        // An uppercase letter after a digit is also a camelCase boundary.
        assert_eq!(normalize_relation_kind("layer2Uses"), "layer2_uses");
    }

    #[test]
    fn relation_id_is_notation_independent() {
        let (a, b) = ("id-a", "id-b");
        let canonical = Relation::make_id(a, "depends_on", b);
        assert_eq!(Relation::make_id(a, "dependsOn", b), canonical);
        assert_eq!(Relation::make_id(a, "depends-on", b), canonical);
        // A kind with a different meaning gives a different id.
        assert_ne!(Relation::make_id(a, "part_of", b), canonical);
    }

    fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: None,
            observed_at: 1,
            confidence: Some(1.0),
            trust_tier: TrustTier::default(),
        }
    }

    #[test]
    fn observation_id_includes_assertions() {
        let plain = Observation::new("supragnosis uses rmcp".into(), prov());
        // An empty assertion set has the same id as a text-only observation (compatible with the existing id scheme).
        let empty = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions::default(),
        );
        assert_eq!(plain.id, empty.id);

        // Attaching an assertion makes a different observation - even with the same text, assertions are not lost to dedup.
        let asserted = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        assert_ne!(plain.id, asserted.id);

        // Different assertion content gives a different id (type assignment is also content identity).
        let retyped = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None,
                    name: "rmcp".into(),
                    kind: Some("Project".into()),
                }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        assert_ne!(asserted.id, retyped.id);

        // The same assertion gives the same id no matter which path it comes through (determinism).
        let again = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        assert_eq!(asserted.id, again.id);
    }

    /// length-prefix encoding: boundary manipulation that forges an assertion block by planting a delimiter in content
    /// cannot make the same id as another observation (in the delimiter-concatenation era a collision was constructible).
    #[test]
    fn length_prefix_blocks_boundary_collision() {
        let crafted = Observation::new("x\0E\0rmcp\0Tool\0".into(), prov());
        let asserted = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        assert_ne!(crafted.id, asserted.id, "a boundary manipulation collision must be blocked");

        // Option presence encoding: an unspecified type and an empty-string type are different assertions.
        let untyped = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None, name: "rmcp".into(), kind: None }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        let empty_typed = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { description: None,
                    name: "rmcp".into(),
                    kind: Some(String::new()),
                }],
                relations: vec![],
                type_defs: vec![],
            },
        );
        assert_ne!(untyped.id, empty_typed.id);
    }

    /// absorb's monotonic union: the same result regardless of arrival order (commutative), relay duplicates are
    /// naturally deduped (idempotent), and independent re-observations accumulate (Principle 3/16).
    #[test]
    fn absorb_union_is_order_independent_and_idempotent() {
        let prov_a = Provenance {
            host: "host-a".into(),
            confidence: Some(0.9),
            ..prov()
        };
        let prov_b = Provenance {
            host: "host-b".into(),
            confidence: Some(0.1),
            ..prov()
        };

        let make = |p: &Provenance, derived: &[&str]| {
            let mut o = Observation::new("same fact".into(), p.clone());
            o.derived_from = derived.iter().map(|s| s.to_string()).collect();
            o
        };

        // a first vs b first - converges to the same attestation/lineage set.
        let mut ab = make(&prov_a, &["o1"]);
        ab.absorb(make(&prov_b, &["o2"]));
        let mut ba = make(&prov_b, &["o2"]);
        ba.absorb(make(&prov_a, &["o1"]));
        assert_eq!(ab.provenance.len(), 2);
        let hosts = |o: &Observation| -> Vec<String> {
            o.provenance.iter().map(|p| p.host.clone()).collect()
        };
        assert_eq!(hosts(&ab), hosts(&ba), "the union is order-independent");
        assert_eq!(ab.derived_from, ba.derived_from);
        assert_eq!(ab.derived_from, vec!["o1".to_string(), "o2".to_string()]);

        // A relay duplicate (a fully identical attestation) does not grow the count (idempotent).
        ab.absorb(make(&prov_a, &["o1"]));
        assert_eq!(ab.provenance.len(), 2);
        assert_eq!(ab.derived_from.len(), 2);
    }

    /// The topology-independence property of observation-log merging (Principle 16): the same attestation set converges to
    /// the same log no matter what arrival order it comes in or how many relay duplicates are mixed in.
    /// The random order is generated with a seed-fixed LCG so it is reproducible (no wall clock / OS randomness).
    /// The property test for the graph projection layer is deferred to M4 (architecture section 14) - this test
    /// continuously guards the log layer, which already implements the convergence norm.
    #[test]
    fn absorb_converges_under_random_arrival_orders() {
        const N: usize = 8;
        // N distinct attestations - including unstated confidence (verifying distinction preservation).
        let sources: Vec<(Provenance, Vec<String>)> = (0..N)
            .map(|i| {
                let p = Provenance {
                    host: format!("host-{i}"),
                    confidence: if i % 3 == 0 { None } else { Some(i as f32 / N as f32) },
                    observed_at: (100 - i) as u64,
                    ..prov()
                };
                (p, vec![format!("src-{}", i % 4)])
            })
            .collect();
        let make = |(p, d): &(Provenance, Vec<String>)| {
            let mut o = Observation::new("same fact".into(), p.clone());
            o.derived_from = d.clone();
            o
        };
        // A comparable representation of the log state (Provenance does not implement PartialEq - via serde value).
        let state = |o: &Observation| {
            serde_json::to_value((&o.provenance, &o.derived_from)).unwrap()
        };
        let fold = |order: &[usize]| {
            let mut acc = make(&sources[order[0]]);
            for &i in &order[1..] {
                acc.absorb(make(&sources[i]));
            }
            acc
        };

        let baseline = state(&fold(&(0..N).collect::<Vec<_>>()));
        // Fisher-Yates shuffle with a seed-fixed LCG + duplicating the head to simulate a relay duplicate.
        let mut lcg: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |bound: usize| {
            lcg = lcg
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((lcg >> 33) as usize) % bound
        };
        for round in 0..32 {
            let mut order: Vec<usize> = (0..N).collect();
            for i in (1..N).rev() {
                order.swap(i, next(i + 1));
            }
            let dup = order[next(N)];
            order.push(dup); // relay duplicate
            assert_eq!(
                state(&fold(&order)),
                baseline,
                "round {round}: if arrival order {order:?} produces a different log it is a convergence violation"
            );
        }
    }

    /// Principle 2 (4th): an unstated confidence is preserved as information distinct from a full-confidence assertion - if the
    /// same source asserts once unstated and once with 1.0, the two attestations stay separate.
    #[test]
    fn unstated_confidence_is_distinct_from_full_confidence() {
        let unstated = Provenance { confidence: None, ..prov() };
        let full = Provenance { confidence: Some(1.0), ..prov() };
        let mut o = Observation::new("fact".into(), unstated);
        o.absorb(Observation::new("fact".into(), full));
        assert_eq!(
            o.provenance.len(),
            2,
            "an unstated and a full-confidence assertion collapsing into one is capture loss: {:?}",
            o.provenance
        );
    }

    #[test]
    fn cosine_similarity_basics() {
        // Same direction = 1, orthogonal = 0, opposite = -1.
        assert!((cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        // Defensive: length mismatch / zero vector is 0.0.
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
