//! Provenance and federation sync metadata: the trust tier, the attestation, its sync
//! stamp, the hybrid logical clock and the version vector.

use serde::{Deserialize, Serialize};

use crate::Timestamp;

#[cfg(doc)]
use crate::{attestation_signing_bytes, NodeIdentity};

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
    /// Federation sync metadata (M4, docs/federation.md Section 3) - assigned once by the origin at
    /// authoring/stamping time, preserved verbatim through relays. `None` for pre-federation or
    /// local-unsigned attestations (they stay local-only until backfill-stamped at first export).
    /// **Not part of the content address** (F2): it lives on the attestation, and `Provenance` is not
    /// hashed - identical content on two nodes keeps one id. It IS an attestation-distinguishing axis
    /// (see `provenance_order` in observation.rs, the P14 compile-forced enumeration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncMeta>,
}

/// The four sync fields of federation (docs/federation.md Section 3) plus the origin's lineage
/// declaration, stored as one block: an attestation either is sync-stamped (all fields present) or is
/// not (block absent). `lineage` is `derived_from` exactly as the origin declared it at stamping time -
/// it rides inside the signed bytes so a relay cannot forge or strip lineage undetected (F13), and it is
/// kept here so the signature stays verifiable after the observation-level `derived_from` union grows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncMeta {
    /// The node_id (public-key fingerprint, [`NodeIdentity::node_id`]) that authored this attestation.
    pub origin_node: String,
    /// Monotonic counter the origin keeps per (origin_node, workspace) - dense within a workspace (F7).
    pub origin_seq: u64,
    /// Hybrid logical clock stamp at authoring time (federation.md Section 4, I11).
    pub hlc: Hlc,
    /// Hex ed25519 signature over [`attestation_signing_bytes`] by the origin's node key.
    pub signature: String,
    /// The origin's `derived_from` declaration at stamping time (inside the signed bytes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<String>,
}

/// Hybrid logical clock (docs/federation.md Section 4). Total order by the derived
/// `(wall, counter, node)` tuple - field declaration order makes `derive(Ord)` give exactly that.
/// `now_millis()` alone is not a total order across hosts; the HLC is the deterministic order key for
/// cross-node folds (I11). It does NOT replace `observed_at` (P4 transaction time stays human-facing).
///
/// [`Hlc::tick`] and [`Hlc::merge`] are the clock rules Section 4 specifies. No stamp uses them yet:
/// a stamp carries its attestation's authoring time (sync-correctness.md Section 3), so the causal
/// update I11 asks for is owed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Hlc {
    pub wall: u64,
    pub counter: u32,
    pub node: String,
}

impl Hlc {
    /// The smallest `(wall, counter)` after this one: the next counter, or - when the counter is
    /// full - the next wall. A remote stamp is a peer's signed claim and may carry any counter, so
    /// `counter + 1` must neither overflow nor wrap round to a time before the one it follows.
    fn successor(&self) -> (u64, u32) {
        match self.counter.checked_add(1) {
            Some(counter) => (self.wall, counter),
            None => (self.wall.saturating_add(1), 0),
        }
    }

    /// Advance for a local event: wall catches up to physical time, the counter breaks same-wall ties.
    pub fn tick(prev: &Hlc, now: Timestamp, node: &str) -> Hlc {
        let (wall, counter) = prev.successor().max((now, 0));
        Hlc { wall, counter, node: node.to_string() }
    }

    /// Merge on receiving a remote stamp (standard HLC receive rule): never runs backwards, and lands
    /// strictly after both `prev` and `remote` when physical time has not caught up.
    pub fn merge(prev: &Hlc, remote: &Hlc, now: Timestamp, node: &str) -> Hlc {
        let (wall, counter) = prev.successor().max(remote.successor()).max((now, 0));
        Hlc { wall, counter, node: node.to_string() }
    }

    /// Deterministic fallback for pre-federation attestations (no sync block): wall = `observed_at`,
    /// counter 0, empty node. Keeps HLC-ordered folds total and arrival-order independent even over a
    /// legacy log (federation.md Section 4).
    pub fn legacy(observed_at: Timestamp) -> Hlc {
        Hlc { wall: observed_at, counter: 0, node: String::new() }
    }
}

/// Version vector (docs/federation.md Section 5): what a node holds, per (origin node, workspace) -
/// dense within a shared workspace, whole workspaces absent under selective sharing (F7/F9).
///
/// Serde note: JSON object keys must be strings, so the tuple-keyed map cannot derive its wire form.
/// The wire representation is a sorted list of `(node, workspace, seq)` triples - deterministic
/// (BTreeMap order), language-neutral, and stable for the sync API (Phase 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionVector(pub std::collections::BTreeMap<(String, String), u64>);

impl Serialize for VersionVector {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(|((n, w), q)| (n, w, q)))
    }
}

impl<'de> Deserialize<'de> for VersionVector {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let triples: Vec<(String, String, u64)> = Vec::deserialize(d)?;
        Ok(VersionVector(triples.into_iter().map(|(n, w, q)| ((n, w), q)).collect()))
    }
}

impl VersionVector {
    /// Highest origin_seq held for (node, workspace); 0 = nothing held.
    pub fn get(&self, node: &str, workspace: &str) -> u64 {
        *self.0.get(&(node.to_string(), workspace.to_string())).unwrap_or(&0)
    }

    /// Monotonic advance (max) - never runs backwards, so relaying/replaying is idempotent.
    pub fn advance(&mut self, node: &str, workspace: &str, seq: u64) {
        let e = self.0.entry((node.to_string(), workspace.to_string())).or_insert(0);
        *e = (*e).max(seq);
    }

    /// Is the attestation (node, workspace, seq) already covered (held)?
    pub fn covers(&self, node: &str, workspace: &str, seq: u64) -> bool {
        seq <= self.get(node, workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlc_is_monotonic_and_merge_lands_after_both() {
        let mut clock = Hlc::default();
        // Ticking never runs backwards, even when the wall clock stalls (same `now`).
        for _ in 0..5 {
            let next = Hlc::tick(&clock, 100, "n1");
            assert!(next > clock, "tick must be strictly increasing");
            clock = next;
        }
        // Receiving a remote stamp ahead of us lands strictly after both (I11 causal propagation).
        let remote = Hlc { wall: 500, counter: 3, node: "n2".into() };
        let merged = Hlc::merge(&clock, &remote, 100, "n1");
        assert!(merged > clock && merged > remote, "merge must land after both sides");
        // A remote counter is a peer's signed claim and can be anything, the largest included.
        // The next stamp carries into the wall: no overflow panic (which would also poison the
        // node's clock mutex), and no wrap to a time before the stamp it follows.
        let full = Hlc { wall: 500, counter: u32::MAX, node: "n2".into() };
        let merged = Hlc::merge(&clock, &full, 100, "n1");
        assert!(merged > full, "merge lands after a full counter: {merged:?}");
        assert!(Hlc::tick(&full, 100, "n2") > full, "and so does a tick");
        // Legacy fallback is deterministic and orders by observed_at.
        assert!(Hlc::legacy(5) < Hlc::legacy(6));
    }

    #[test]
    fn version_vector_covers_and_advances_monotonically() {
        let mut vv = VersionVector::default();
        assert!(!vv.covers("n1", "ws", 1));
        vv.advance("n1", "ws", 3);
        assert!(vv.covers("n1", "ws", 3) && vv.covers("n1", "ws", 1));
        assert!(!vv.covers("n1", "ws", 4));
        assert!(!vv.covers("n1", "other-ws", 1), "scoped per (node, workspace)");
        vv.advance("n1", "ws", 2); // never runs backwards
        assert_eq!(vv.get("n1", "ws"), 3);
    }
}
