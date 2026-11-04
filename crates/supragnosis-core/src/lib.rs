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

#[cfg(test)]
mod tests {

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
