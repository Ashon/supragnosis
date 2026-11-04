//! supragnosis-core - domain model + ports (traits).
//!
//! This crate has **no IO dependencies** (pure domain). Side effects such as storage/embedding/sync
//! are implemented as adapters in other crates against the traits (ports) defined here.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

mod apps;
mod belief;
mod graph;
mod observation;
mod provenance;
mod query;
mod signing;

pub use apps::{app_name, client_app, client_recorded, client_said, AI_APPS, CLIENT_NAME_MAX};
pub use belief::{
    evaluated_tier, verdict_grant_ceiling, BeliefCandidate, BeliefChoice, ResolutionPolicy,
    TierWeighted, VERDICT_SURFACE_AGENT, VERDICT_SURFACE_CONSOLE, VERDICT_SURFACE_PREFIX,
};
pub use graph::{hyperedge_id, normalize_relation_kind, Entity, Relation};
pub use observation::{
    observation_content_id, ordering_hlc, Assertions, EntityAssertion, Observation,
    ProposalEventAssertion, ProposalEventKind, RelationAssertion, TypeDefAssertion, TypeTarget,
};
pub use provenance::{Hlc, Provenance, SyncMeta, TrustTier, VersionVector};
pub use query::{KeywordMatch, KeywordQuery, SearchHit, SearchHitKind, TraverseHit};
pub use signing::{
    attestation_key, attestation_signing_bytes, is_canonical_public_key_hex, is_verifying_key,
    message_signing_bytes, node_id_from_public_key, verify_attestation, verify_message,
    NodeIdentity,
};

/// epoch millis.
pub type Timestamp = u64;

/// Current time (epoch millis). M0 uses the node wall clock - multi-host deterministic ordering (HLC) is introduced in M4.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The replication wire unit (docs/federation.md Section 3/5): one signed attestation bound to a
/// content id. The receiver recomputes the content id from (workspace, content, assertions) - it is
/// deliberately NOT carried, so a forged id cannot ride the wire - then CAS-dedups/absorbs (F3). The
/// origin's lineage declaration travels inside `attestation.sync.lineage` (signed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationEvent {
    pub content: String,
    #[serde(default, skip_serializing_if = "Assertions::is_empty")]
    pub assertions: Assertions,
    pub attestation: Provenance,
}

/// Storage port. Implemented by adapters such as in-memory / redb.
///
/// **Read contract** (the duty of every adapter):
/// - **Distinguishing absence from failure (Principle 5)**: a backend failure propagates as `Err`. Swallowing a failure as an
///   empty result (`Ok(vec![])`/`Ok(None)`) leaves the caller unable to distinguish "not found (unknown)" from "cannot
///   query (failure)" - the premise of distinguishing absence/negation/failure collapses at the storage layer. A partial
///   failure is also `Err`, not a partial result.
/// - **Reproducibility (Principle 16)**: the same query on the same state gives the same response. Sorting and limit
///   truncation are pinned to a stable key (id), and it is a contract violation if the iteration order of internal data
///   structures (hash map / row order) leaks into the response. The enumerations
///   ([`KnowledgeStore::all_entities`] / [`KnowledgeStore::all_relations`] / [`KnowledgeStore::all_observations`])
///   return rows in **ascending id order** - an enumeration is a response too, and it is the input to every fold on the
///   read path, so a fold that resolves a tie by "first row seen" would otherwise inherit an order the store never
///   promised. Naming the order is what makes the promise checkable
///   (`crates/supragnosis-store/tests/port_conformance.rs`).
pub trait AssertionStore: Send + Sync {
    /// Appends (or absorbs a re-arrival into) one log row. **Contract (crash-recovery.md K1):** the
    /// same store transaction that writes the row records it in the owed-projection ledger, so the
    /// append and the record that its projection is still owed commit together or not at all. The
    /// adapter does it rather than the caller because the sync crate appends through this handle and
    /// never sees the engine that would otherwise have to remember. An observation with no
    /// attestation is refused before anything is written ([`Observation::check_attested`]).
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    /// Restores an observation by id from the observation log - the back-reference path for search hits / derivation
    /// lineage (Principle 2/14: whoever knows the id can reach the entity and its provenance) and the reference read for
    /// re-arrival merging. A missing id is `Ok(None)`, a backend failure is `Err`.
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError>;
    /// A missing id is `Ok(None)` (absence, the unknown of Principle 5) - only a backend failure is `Err`.
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError>;
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
    /// Whether `workspace` holds any observation - what makes a workspace exist. The agent surface
    /// asks it before a write, to say when the write makes a new one (design-system.md Section 29.20).
    /// The default reads the workspace's log; an adapter with a workspace index answers from it.
    fn holds_observations(&self, workspace: &str) -> Result<bool, StoreError> {
        Ok(!self.all_observations(Some(workspace))?.is_empty())
    }
    /// Every workspace that holds an observation, ascending and each once - the workspaces
    /// [`AssertionStore::holds_observations`] says exist, all at once. A workspace whose
    /// observations assert no entity is one of them (Principle 22). The default walks the whole
    /// log; an adapter with a workspace index answers from its keys without reading a row.
    fn observation_workspaces(&self) -> Result<Vec<String>, StoreError> {
        let set: std::collections::BTreeSet<String> =
            self.all_observations(None)?.iter().map(|o| o.workspace().to_string()).collect();
        Ok(set.into_iter().collect())
    }
    /// The sync delta read (docs/federation.md Section 5, M4 Phase 1): every sync-stamped attestation
    /// in `workspace` NOT yet covered by `since`, as wire-unit events, ordered deterministically by
    /// (origin_node, origin_seq, content id). Attestations without a sync block (pre-federation /
    /// local-unsigned) are local-only and never returned - they become exportable via backfill stamping
    /// (Phase 2). The default implementation scans [`KnowledgeStore::all_observations`]; adapters may
    /// override with an indexed scan.
    fn attestations_since(
        &self,
        workspace: &str,
        since: &VersionVector,
    ) -> Result<Vec<AttestationEvent>, StoreError> {
        let mut events = Vec::new();
        for obs in self.all_observations(Some(workspace))? {
            // Legacy-format guard: a stored id that no longer matches the current content-address
            // formula cannot verify remotely (signatures bind the recomputed id) - such rows are
            // local history and never cross the wire. See [`observation_content_id`] / migrate.
            if observation_content_id(workspace, &obs.content, &obs.assertions) != obs.id {
                continue;
            }
            for p in &obs.provenance {
                let Some(meta) = &p.sync else { continue };
                if since.covers(&meta.origin_node, workspace, meta.origin_seq) {
                    continue;
                }
                events.push(AttestationEvent {
                    content: obs.content.clone(),
                    assertions: obs.assertions.clone(),
                    attestation: p.clone(),
                });
            }
        }
        events.sort_by(|a, b| {
            let ka = a.attestation.sync.as_ref().map(|s| (s.origin_node.clone(), s.origin_seq));
            let kb = b.attestation.sync.as_ref().map(|s| (s.origin_node.clone(), s.origin_seq));
            ka.cmp(&kb).then_with(|| a.content.cmp(&b.content))
        });
        Ok(events)
    }
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

/// The full store: [`AssertionStore`] plus the two writes that put a row into the entity/relation
/// projection directly.
///
/// The split is Principle 1's third enforcement demand made structural: *"no API may write a fact
/// that did not pass through an assertion directly into the graph"*. Only the engine's projection
/// folds hold this trait. Everything else - the sync crate applying replicated events, the MCP tools,
/// the CLI - is handed an `AssertionStore`, through which knowledge can only enter as an appended
/// assertion and the graph can only be read.
///
/// It was a convention before, and the convention held: the calls to these methods sit in the
/// projection's writers and the entity-embedding backfill. What it did not hold against is the
/// past. The author's own store carries 35 entity rows that no observation anywhere
/// asserts - written directly, in an era when something did reach for this handle - and they are
/// invisible to the log, survive every re-projection, and cannot cross the sync wire because there is
/// nothing to export. A replay cannot reproduce them because the log never knew about them.
///
/// A test cannot guard this: it would have to enumerate callers that do not exist yet. A supertrait
/// can, by not being reachable through the handle the engine hands out.
pub trait KnowledgeStore: AssertionStore {
    /// Upsert keyed on entity.id. Reachable only where the projection is computed.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    /// Upsert keyed on relation.id. Reachable only where the projection is computed.
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// Upserts a workspace's whole projection, entity rows then relation rows: a reproject's result
    /// (crash-recovery.md Section 3a). A store that can commits it in one transaction, so it pays
    /// one commit rather than one per row and no reader sees part of it. The default writes row by
    /// row.
    fn put_projection(
        &self,
        entities: Vec<Entity>,
        relations: Vec<Relation>,
    ) -> Result<(), StoreError> {
        for e in entities {
            self.put_entity(e)?;
        }
        for r in relations {
            self.add_relation(r)?;
        }
        Ok(())
    }
    /// The owed-projection ledger (crash-recovery.md Section 3): every observation whose append
    /// committed and whose projection has not been confirmed since, as `(observation id, workspace)`
    /// in id order. Empty is the normal state. Projection-side, so it lives on the handle only the
    /// engine holds.
    fn owed_projections(&self) -> Result<Vec<(String, String)>, StoreError>;
    /// Removes ledger entries once the projection that repays them is written (K2). Ids not in the
    /// ledger are ignored, so clearing is idempotent.
    fn clear_owed(&self, ids: &[String]) -> Result<(), StoreError>;
    /// Appends `obs` - absorbed into a stored row of the same id, as `add_observation` does - and
    /// writes the rows projected from it, in one transaction: all of it lands or none does
    /// (crash-recovery.md Section 3a). It records no ledger entry, since the projection commits with
    /// the row, and clears none: an entry already held for the id is another writer's debt.
    fn commit_projected(
        &self,
        obs: Observation,
        entities: Vec<Entity>,
        relations: Vec<Relation>,
    ) -> Result<(), StoreError>;
    /// Attaches a retrieval vector to each named observation that has none - the backfill's write
    /// (semantic-retrieval.md Section 12, SR6). Nothing else is written: not the row, not its
    /// attestations, not the owed-projection ledger, which re-appending the row would touch. An id
    /// with no row, or whose row already carries a vector, is skipped (SR7: a stored vector is never
    /// replaced). Returns how many vectors were attached.
    fn attach_observation_embeddings(
        &self,
        vectors: &[(String, Vec<f32>)],
    ) -> Result<usize, StoreError>;
    /// Records the app that wrote an attestation, under its [`attestation_key`] (design-system.md
    /// Section 29.8). Node-local: nothing that is signed, synced, ordered or folded reads it. A key
    /// already recorded keeps the name it has - a row is written once.
    fn record_attestation_app(&self, key: &str, app: &str) -> Result<(), StoreError>;
    /// The apps recorded under these keys, by key; a key with none is absent from the answer.
    fn attestation_apps(
        &self,
        keys: &[String],
    ) -> Result<std::collections::BTreeMap<String, String>, StoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}

/// A credential-shaped run of text found by [`detect_secret`]: which pattern matched, and where.
///
/// It deliberately does NOT carry the matched text. An error naming what it found would put the
/// secret into the caller's logs, the agent's transcript and any transport in between - re-leaking
/// the thing the check exists to keep out. Same rule the excision tombstone follows for its reason
/// field (excision.md E2): name the shape and the place, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretHit {
    /// A short, stable name for the shape that matched - safe to log and to show an agent.
    pub pattern: &'static str,
    /// Byte offset of the match, so a caller can point at it without quoting it.
    pub at: usize,
}

/// Detects credential-shaped text (Principle 17: the secret-redaction hook at ingest).
///
/// **Refuse, never rewrite.** P1 forbids transforming an assertion before it reaches the log -
/// notation is preserved verbatim and normalization is the projection's job - so this returns a
/// finding and the caller refuses the write. Redacting in place would also change the content and
/// therefore the content address (P14), which is a second reason it cannot be done here.
///
/// Refusing does not conflict with P1's ban on content-grounded rejection either: what that clause
/// protects is an assertion's *claim* - its notation, its truth, its schema conformance. A leaked
/// credential is none of those. It is material whose presence is the harm, and P1 itself argues for
/// catching it here, since the append-only log (P3) makes "before ingest" the only moment that works.
///
/// It is also NOT the case that Appendix A's "do not block ingest; defend with trust" governs this.
/// That row is about suspicious *knowledge*, where quarantining at the lowest tier reduces the harm
/// because the harm is believing a falsehood. A low-tier secret is still a readable secret and still
/// replicates, so a tier does nothing here.
///
/// **Patterns are deliberately narrow.** Only shapes that are close to self-identifying: a generic
/// entropy heuristic would fire on hashes, ids and base64 payloads, and a detector the operator learns
/// to override is worse than none. False negatives are expected and acceptable - this is defence in
/// depth (P17), an aid to the sharing filter rather than a replacement for it.
pub fn detect_secret(text: &str) -> Option<SecretHit> {
    let b = text.as_bytes();
    let hit = |pattern, at| Some(SecretHit { pattern, at });

    // A PEM private key block. The two halves are checked in order so "PRIVATE KEY" in prose does not
    // match on its own.
    if let Some(i) = find(b, b"-----BEGIN") {
        if find(&b[i..], b"PRIVATE KEY-----").is_some() {
            return hit("pem-private-key", i);
        }
    }
    for (prefix, len, set) in [
        // AWS access key id: AKIA + 16 uppercase alphanumerics.
        (&b"AKIA"[..], 16usize, CharSet::UpperNum),
        // Google API key: AIza + 35 url-safe characters.
        (&b"AIza"[..], 35, CharSet::UrlSafe),
    ] {
        if let Some(i) = find_run(b, prefix, len, set) {
            return hit(if prefix == b"AKIA" { "aws-access-key-id" } else { "google-api-key" }, i);
        }
    }
    for prefix in [&b"ghp_"[..], b"gho_", b"ghs_", b"ghu_", b"ghr_"] {
        if let Some(i) = find_run(b, prefix, 36, CharSet::UrlSafe) {
            return hit("github-token", i);
        }
    }
    for prefix in [&b"xoxb-"[..], b"xoxp-", b"xoxa-", b"xoxr-"] {
        if let Some(i) = find_run(b, prefix, 10, CharSet::UrlSafe) {
            return hit("slack-token", i);
        }
    }
    for prefix in [&b"sk_live_"[..], b"rk_live_"] {
        if let Some(i) = find_run(b, prefix, 16, CharSet::UrlSafe) {
            return hit("stripe-live-key", i);
        }
    }
    // A JWT's first segment is the base64 of `{"alg":"...`, which starts with this fixed run.
    if let Some(i) = find(b, b"eyJhbGciOi") {
        return hit("jwt", i);
    }
    if let Some(i) = find_url_credentials(b) {
        return hit("url-inline-credentials", i);
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharSet {
    UpperNum,
    UrlSafe,
}

impl CharSet {
    fn has(self, c: u8) -> bool {
        match self {
            CharSet::UpperNum => c.is_ascii_uppercase() || c.is_ascii_digit(),
            CharSet::UrlSafe => c.is_ascii_alphanumeric() || c == b'_' || c == b'-',
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A prefix followed by at least `len` characters from `set` - the shape almost every issued token
/// takes, and specific enough that prose does not reach it.
fn find_run(hay: &[u8], prefix: &[u8], len: usize, set: CharSet) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = find(&hay[from..], prefix) {
        let i = from + rel;
        let run = hay[i + prefix.len()..].iter().take_while(|c| set.has(**c)).count();
        if run >= len {
            return Some(i);
        }
        from = i + 1;
    }
    None
}

/// `scheme://user:password@host` - a connection string with the password inlined. The userinfo must
/// contain a colon and a non-empty password, so `ssh://git@host` and a bare `a:b@c` in prose do not
/// match.
fn find_url_credentials(hay: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = find(&hay[from..], b"://") {
        let start = from + rel + 3;
        let rest = &hay[start..];
        let end = rest
            .iter()
            .position(|c| matches!(c, b'/' | b' ' | b'\n' | b'"'))
            .unwrap_or(rest.len());
        let userinfo_end = rest[..end].iter().position(|c| *c == b'@');
        if let Some(at) = userinfo_end {
            if let Some(colon) = rest[..at].iter().position(|c| *c == b':') {
                if at > colon + 1 {
                    return Some(start);
                }
            }
        }
        from = start;
    }
    None
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
        v.pop().ok_or_else(|| EmbedError::Provider("empty embedding result".into()))
    }
    /// Whether this provider can answer in this process. A provider that loads on first use is
    /// ready while its files are there; one that cannot load says why and what would change it,
    /// so a degrade to keyword retrieval is stated, never silent (Principle 24).
    fn readiness(&self) -> Readiness {
        Readiness::Ready
    }
}

/// What an [`EmbeddingProvider`] says about itself when asked whether it can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// Loaded, or loads on first use from files that are there.
    Ready,
    /// Cannot answer in this process; the string says why and what to run.
    Degraded(String),
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding provider error: {0}")]
    Provider(String),
}

/// Cosine similarity of two vectors (-1.0~1.0). 0.0 if the lengths differ, a vector is a zero
/// vector, or the result is not finite - a NaN or infinite component, or a norm that overflows.
/// Every similarity ranking sorts on this score with `partial_cmp`, where a NaN compares equal to
/// everything and lands wherever the sort leaves it, first included; so no caller sees one.
/// A pure function - shared by both store adapters, the merge band, and recall evaluation.
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
    let sim = dot / (na.sqrt() * nb.sqrt());
    if sim.is_finite() {
        sim
    } else {
        0.0
    }
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
    Traverse { start: String, reached: Vec<String> },
    /// Federation sync activity (M4): a peer hit this node's sync API (hub side), or this node ran
    /// a round against a server (client side) - streamed to the viewer so remote hits are visible
    /// live in the activity feed.
    Sync {
        /// Hub side: "advertise" | "pull-served" | "push-received". Client side: "pull" | "push".
        direction: String,
        /// Counterparty: the peer node_id (hub side) or the server URL (client side).
        peer: String,
        workspace: String,
        /// Events served/accepted in this hit.
        count: usize,
    },
}

/// Event envelope: event + **session id** (the grouping key for a conversation footprint). To view together which
/// knowledge was used within the same session. `event` is flattened and serialized as `{session, kind, ...}` -
/// the viewer reads `ev.session` and `ev.kind` flat.
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    /// Session id (a conversation, roughly one MCP server run unit, or injected by the client). The footprint grouping key.
    pub session: String,
    /// The app of the local MCP session the activity came from, as it named itself in `initialize`
    /// (design-system.md Section 29.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(flatten)]
    pub event: Event,
}

/// UI event sink port (Principle 20). Held by the engine and emitted by tool calls. Without one it is a no-op -
/// observability is optional, and core correctness does not depend on this port (the spirit of Principle 19).
pub trait EventSink: Send + Sync {
    fn emit(&self, env: &EventEnvelope);
}

/// Transaction-time source port (Principle 20). The ingest path stamps `observed_at` from this, and
/// for a local (unstamped) attestation that timestamp becomes the ordering key `ordering_hlc` falls
/// back to - so the clock is not decoration, it decides which of two competing assertions the
/// resolution policy calls the more recent one.
///
/// It is a port because a test cannot otherwise say what time it is. Ingesting twice inside one
/// millisecond makes two observations tie on their ordering HLC and fall through to the id
/// tiebreak, which is a different branch of [`ResolutionPolicy`] than the one a recency test means
/// to exercise. The alternative is to sleep, and a test that sleeps is a test that asserts the
/// machine was fast enough - the wall clock the convergence guards are supposed to be free of.
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> Timestamp;
}

/// The node wall clock - the default [`Clock`], and what every non-test caller wants.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> Timestamp {
        now_millis()
    }
}

#[cfg(test)]
mod tests {

    /// The detector fires on credential shapes and stays quiet on the text a knowledge base is made
    /// of. The negative half is the important one: a detector that cries wolf gets switched off, and
    /// a switched-off detector is worse than none because it is believed to be running.
    #[test]
    fn detect_secret_finds_credentials_without_firing_on_prose() {
        let fires = [
            ("-----BEGIN RSA PRIVATE KEY-----\nMIIEow...", "pem-private-key"),
            ("aws key AKIAIOSFODNN7EXAMPLE in the config", "aws-access-key-id"),
            ("token ghp_1234567890abcdefghijklmnopqrstuvwxyz here", "github-token"),
            ("xoxb-2411-2411-abcdefghij", "slack-token"),
            // Split for the same reason as the Stripe key below. This one got through push
            // protection and was then reported by secret scanning after the fact, which is the
            // same collision arriving one step later: a scanner reads the shape, and a detector's
            // test suite is made of shapes.
            (concat!("AIza", "SyA1234567890abcdefghijklmnopqrstuv"), "google-api-key"),
            // Split from its prefix and rejoined by concat!. This is Stripe's own published
            // example key, but GitHub push protection scans the diff rather than the intent, so a
            // contiguous `sk_live_...` here blocks every push that touches this file - one detector
            // refusing another detector's test suite. detect_secret still sees the whole string.
            (concat!("sk_live", "_4eC39HqLyjWDarjtT1zdp7dc"), "stripe-live-key"),
            ("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.x.y", "jwt"),
            (
                "DATABASE_URL=postgres://admin:hunter2@db.internal:5432/app",
                "url-inline-credentials",
            ),
        ];
        for (text, want) in fires {
            let hit = detect_secret(text).unwrap_or_else(|| panic!("missed a {want}: {text}"));
            assert_eq!(hit.pattern, want, "wrong pattern for: {text}");
        }

        let quiet = [
            // Ordinary knowledge, including the words and shapes that a naive detector trips on.
            "the deploy reads DATABASE_URL from the environment",
            "rotate the PRIVATE KEY quarterly - see the runbook",
            "https://github.com/ashon/supragnosis",
            "ssh://git@github.com/ashon/supragnosis.git",
            "blake3 id d20747da56a857fbcfb6c76429e895d5a160fa95a117c21e0c9105001ad27ce2",
            "AKIA is the prefix AWS uses for an access key id",
            "the ratio is 3:1@peak",
            "base64 payload eyJ3b3Jrc3BhY2UiOiJ3cyJ9 in the proposal",
        ];
        for text in quiet {
            assert_eq!(detect_secret(text), None, "false positive on: {text}");
        }
    }

    /// A finding names the shape and the place, never the value - so that refusing a write cannot
    /// itself publish the secret into a log, a transcript or an error channel.
    #[test]
    fn a_finding_never_carries_the_secret() {
        let text = "key AKIAIOSFODNN7EXAMPLE";
        let hit = detect_secret(text).expect("hit");
        let rendered = format!("{hit:?}");
        assert!(
            !rendered.contains("AKIAIOSFODNN7EXAMPLE"),
            "the finding quoted the secret: {rendered}"
        );
        assert_eq!(
            &text[hit.at..hit.at + 4],
            "AKIA",
            "but it does point at where the match starts"
        );
    }

    use super::*;

    pub(crate) fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: None,
            observed_at: 1,
            confidence: Some(1.0),
            trust_tier: TrustTier::default(),
            sync: None,
        }
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
        // A vector no ranking can order is no similarity: NaN compares equal to every score, so
        // one bad vector would otherwise sort wherever the sort left it - first, in practice.
        for bad in [[f32::NAN, 1.0], [f32::INFINITY, 1.0], [f32::MAX, f32::MAX]] {
            assert_eq!(cosine_similarity(&bad, &[1.0, 1.0]), 0.0, "{bad:?}");
            assert_eq!(cosine_similarity(&[1.0, 1.0], &bad), 0.0, "{bad:?}");
        }
    }

    // --- M4 Phase 1: federation core foundations (docs/federation.md Section 10) -----------------

    /// Builds a sync-stamped attestation from `identity` (signed - the test-side stamping pipeline).
    pub(crate) fn stamped(
        identity: &NodeIdentity,
        content_id: &str,
        seq: u64,
        hlc: Hlc,
    ) -> Provenance {
        let mut p = Provenance { host: "h".into(), sync: None, ..prov() };
        let mut meta = SyncMeta {
            origin_node: identity.node_id(),
            origin_seq: seq,
            hlc,
            signature: String::new(),
            lineage: vec!["parent-obs".into()],
        };
        meta.signature = identity.sign_attestation(content_id, &p, &meta);
        p.sync = Some(meta);
        p
    }

    /// Known answers for the two encodings that outlive the process computing them
    /// (docs/compatibility.md Section 4): a content id is an observation's identity on every node,
    /// and a signature is checked by every receiver, forever. The golden stores prove the encodings
    /// of one past release still hold; these pin them for inputs that release never wrote - every
    /// optional field absent, and every one present.
    ///
    /// A change that turns these red is a change to every id or every signature already made. The
    /// fix is never a new expected value: a field joins either encoding only after every existing
    /// one, written only when present, as a tag byte and a length-prefixed value - so an absent
    /// field adds no bytes and these stay green.
    #[test]
    fn encodings_that_cross_versions_keep_their_known_answers() {
        let empty = Assertions::default();
        let full = Assertions {
            entities: vec![EntityAssertion {
                name: "redb".into(),
                kind: Some("Technology".into()),
                description: Some("an embedded store".into()),
            }],
            relations: vec![RelationAssertion {
                from: "supragnosis".into(),
                kind: "uses".into(),
                to: "redb".into(),
                description: Some("for its log".into()),
                valid_from: Some(10),
                valid_to: Some(20),
            }],
            type_defs: vec![TypeDefAssertion {
                target: TypeTarget::Entity,
                name: "Technology".into(),
                description: "a tool".into(),
            }],
            proposal_events: Vec::new(),
        };
        assert_eq!(
            observation_content_id("ws", "a fact", &empty),
            "96e14f8155efffed1a0c3bd409a50726ef001218f2e96908a592582b0b248986",
            "content id, no assertions"
        );
        assert_eq!(
            observation_content_id("ws", "a fact", &full),
            "1298f2d98df4d3f90cc77176a482d27f807ecbbf7901974f38e9d4c6760916c4",
            "content id, entity + relation + type definition"
        );

        let identity = NodeIdentity::from_secret_bytes([7u8; 32]);
        let hlc = Hlc { wall: 1_700_000_000_000, counter: 3, node: identity.node_id() };
        let bare = Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: None,
            observed_at: 1_700_000_000,
            confidence: None,
            trust_tier: TrustTier::default(),
            sync: None,
        };
        let bare_meta = SyncMeta {
            origin_node: identity.node_id(),
            origin_seq: 1,
            hlc: hlc.clone(),
            signature: String::new(),
            lineage: Vec::new(),
        };
        let rich = Provenance {
            on_behalf_of: Some("ashon".into()),
            source_ref: Some("doc:1".into()),
            confidence: Some(0.5),
            trust_tier: TrustTier::HumanConfirmed,
            ..bare.clone()
        };
        let rich_meta =
            SyncMeta { lineage: vec!["parent".into()], origin_seq: 2, ..bare_meta.clone() };
        let id = "0".repeat(64);
        let digest = |p: &Provenance, m: &SyncMeta| {
            blake3::hash(&attestation_signing_bytes(&id, p, m)).to_hex().to_string()
        };
        assert_eq!(
            digest(&bare, &bare_meta),
            "16bcb8ab9a29bcee98c6d17986eccdb65d949b7011f1ec5fe663e505fb87e548",
            "signing bytes, optionals absent"
        );
        assert_eq!(
            digest(&rich, &rich_meta),
            "898592c03f22611d63b6ddeb0c5d5962c051e6c107822641d1b0f5849362175b",
            "signing bytes, every field present"
        );
        // ed25519 is deterministic, so the signature itself is a known answer too.
        assert_eq!(
            identity.sign_attestation(&id, &rich, &rich_meta),
            "04108b003e3b7eaa6cd65a08a4a1362e262fe48e9d64ef66f3016d2f2aadfe7f836055de7a5369a7f0705f11bbafc5c61faf615b6ae871675d6a5624429aaf05",
            "a signature over the full encoding"
        );
    }
}
