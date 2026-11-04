//! supragnosis-core - domain model + ports (traits).
//!
//! This crate has **no IO dependencies** (pure domain). Side effects such as storage/embedding/sync
//! are implemented as adapters in other crates against the traits (ports) defined here.

use std::time::{SystemTime, UNIX_EPOCH};

mod apps;
mod belief;
mod embed;
mod events;
mod graph;
mod observation;
mod provenance;
mod query;
mod secret;
mod signing;
mod store;
#[cfg(test)]
mod tests;

pub use apps::{app_name, client_app, client_recorded, client_said, AI_APPS, CLIENT_NAME_MAX};
pub use belief::{
    evaluated_tier, verdict_grant_ceiling, BeliefCandidate, BeliefChoice, ResolutionPolicy,
    TierWeighted, VERDICT_SURFACE_AGENT, VERDICT_SURFACE_CONSOLE, VERDICT_SURFACE_PREFIX,
};
pub use embed::{cosine_similarity, EmbedError, EmbeddingProvider, Readiness};
pub use events::{Clock, Event, EventEnvelope, EventSink, SystemClock};
pub use graph::{hyperedge_id, normalize_relation_kind, Entity, Relation};
pub use observation::{
    observation_content_id, ordering_hlc, Assertions, EntityAssertion, Observation,
    ProposalEventAssertion, ProposalEventKind, RelationAssertion, TypeDefAssertion, TypeTarget,
};
pub use provenance::{Hlc, Provenance, SyncMeta, TrustTier, VersionVector};
pub use query::{KeywordMatch, KeywordQuery, SearchHit, SearchHitKind, TraverseHit};
pub use secret::{detect_secret, SecretHit};
pub use signing::{
    attestation_key, attestation_signing_bytes, is_canonical_public_key_hex, is_verifying_key,
    message_signing_bytes, node_id_from_public_key, verify_attestation, verify_message,
    NodeIdentity,
};
pub use store::{AssertionStore, AttestationEvent, KnowledgeStore, StoreError};

/// epoch millis.
pub type Timestamp = u64;

/// Current time (epoch millis). M0 uses the node wall clock - multi-host deterministic ordering (HLC) is introduced in M4.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
