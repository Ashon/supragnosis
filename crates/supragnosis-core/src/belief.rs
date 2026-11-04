//! Belief resolution: the receiver-evaluated tier, the verdict surface markers and their
//! grant ceiling, and the replaceable resolution policy.

use crate::{Hlc, Provenance, TrustTier};

// --- Belief resolution (M3a, docs/resolution.md) --------------------------------------------------

/// Receiver-evaluated trust tier of ONE attestation (resolution.md Section 3; Principle 18 "the tier
/// is the receiver's evaluation"). The stored (claimed) tier is log data and stays verbatim (F13);
/// this evaluation is what resolution weighting and display consume:
/// - a local attestation (no sync stamp): the stored tier. Trusting it rests on every stamp-less
///   producer holding the line: the local observe path forces the default (a local writer cannot
///   self-declare above `AgentExtracted`), and the stamp-dropping re-key/migration paths clamp a
///   carried claim to its pre-strip evaluation - a stamp-less claim is never a raw wire claim;
/// - a sync-stamped attestation: min(claimed, `HostSigned`) - a signature proves origin, never a human
///   act, so a wire claim can never evaluate to `HumanConfirmed` by itself (federation.md 6b).
///
/// A pure function of the attestation as it sits in the log, so it converges across nodes whose logs
/// converge (stamps replicate via absorb enrichment). Gate grants (merged claim_promotion/demotion
/// verdicts) override this per observation - that fold lives in the engine (resolution.md Section 5).
pub fn evaluated_tier(p: &Provenance) -> TrustTier {
    match &p.sync {
        None => p.trust_tier,
        Some(_) => p.trust_tier.min(TrustTier::HostSigned),
    }
}

/// The reserved source_ref namespace for engine-stamped surface markers. Local ingest surfaces
/// (observe / define_type / propose - and reify, which routes through observe) refuse a
/// client-supplied source_ref under this prefix, and `review_proposal` is the only local author of
/// a marker - so a log-borne marker on a locally-authored observation is always engine-stamped, and
/// the ceiling fold's trust in it does not rest on no other surface ever reading one. (A replicated
/// marker is a different matter: honoring it is the single-principal federation premise,
/// resolution.md Section 6.)
pub const VERDICT_SURFACE_PREFIX: &str = "surface:";
/// Verdict surface markers (resolution.md Section 6): stamped by the ENGINE into a verdict
/// observation's `source_ref` per call-site - the human console (viz unix socket, local OS principal)
/// vs the agent-facing MCP tool path. Never client-suppliable: the review surfaces accept no
/// source_ref of their own, and the local ingest surfaces refuse the whole namespace
/// ([`VERDICT_SURFACE_PREFIX`]).
pub const VERDICT_SURFACE_CONSOLE: &str = "surface:console";
/// The agent (MCP tool) verdict surface marker - see [`VERDICT_SURFACE_CONSOLE`].
pub const VERDICT_SURFACE_AGENT: &str = "surface:agent";

/// The highest tier a merged promotion verdict may grant, decided by the verdict observation's
/// engine-stamped surface marker (resolution.md Section 6): the human console grants up to
/// `HumanConfirmed` (Principle 18: promotion to human-confirmed is a human's direct act - the
/// I17-family mechanism); the agent path, an absent marker, and any unknown marker cap at
/// `HostSigned`. A pure function of the log-borne marker, applied by the FOLD (never the write path),
/// so the cap is deterministic and converges (I2, P16). Trusting a replicated console marker across
/// nodes is the single-principal federation premise; Phase 5's principal-signed acts supersede it.
pub fn verdict_grant_ceiling(source_ref: Option<&str>) -> TrustTier {
    if source_ref == Some(VERDICT_SURFACE_CONSOLE) {
        TrustTier::HumanConfirmed
    } else {
        TrustTier::HostSigned
    }
}

/// One competing value for a single-valued belief field (entity kind, representative spelling) -
/// the policy's input unit (resolution.md Section 2). `tier` is the asserting observation's
/// EFFECTIVE tier (gate grants applied - never a raw claimed tier, R4), `hlc` its fold-ordering key,
/// `observation` its id (the stable final tiebreak, R2).
#[derive(Debug, Clone)]
pub struct BeliefCandidate {
    pub value: String,
    pub tier: TrustTier,
    pub hlc: Hlc,
    pub observation: String,
}

/// The policy's verdict over one candidate set: which candidate wins, and whether the field is
/// contested - distinct surviving values whose effective tiers tie at the top, i.e. trust did not
/// decide (resolution.md Section 4, R6). A conflict a strictly higher tier resolves is NOT contested
/// (it stays queryable in the curation report - R7).
#[derive(Debug, Clone, Copy)]
pub struct BeliefChoice {
    pub index: usize,
    pub contested: bool,
}

/// The replaceable belief-resolution strategy (Principle 1; resolution.md Section 2, R1). A pure
/// function - no IO, no wall clock, no iteration-order dependence (Principle 16). Changing the policy
/// and re-running reprojection recomputes a different belief from the same log.
pub trait ResolutionPolicy: Send + Sync {
    /// Chooses the winning candidate; `None` iff `candidates` is empty. Must be deterministic in the
    /// candidate SET (independent of slice order).
    fn choose(&self, candidates: &[BeliefCandidate]) -> Option<BeliefChoice>;
}

/// The default policy (resolution.md Section 2.1): highest effective tier wins; within the top tier
/// band, latest ordering-HLC wins; final tie by smallest observation id (R2). Confidence never
/// selects - it is self-reported and uncalibrated, only a sub-signal of the tier (Principle 2); it is
/// carried on results verbatim for display (R3).
pub struct TierWeighted;

impl ResolutionPolicy for TierWeighted {
    fn choose(&self, candidates: &[BeliefCandidate]) -> Option<BeliefChoice> {
        use std::cmp::Reverse;
        let (index, winner) = candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| (c.tier, &c.hlc, Reverse(c.observation.as_str())))?;
        let contested = candidates.iter().any(|c| c.tier == winner.tier && c.value != winner.value);
        Some(BeliefChoice { index, contested })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{prov, stamped};
    use crate::NodeIdentity;

    // --- Belief resolution (M3a, docs/resolution.md) --------------------------------------------

    fn cand(value: &str, tier: TrustTier, wall: u64, obs: &str) -> BeliefCandidate {
        BeliefCandidate {
            value: value.into(),
            tier,
            hlc: Hlc { wall, counter: 0, node: String::new() },
            observation: obs.into(),
        }
    }

    /// guard (resolution.md R2): the default policy selects by effective tier first, then latest
    /// ordering HLC, then smallest observation id - and the choice is slice-order independent.
    #[test]
    fn tier_weighted_selection_order() {
        let p = TierWeighted;
        // Tier beats recency: an older host_signed wins over a newer agent_extracted.
        let cs = vec![
            cand("Library", TrustTier::AgentExtracted, 200, "obs-b"),
            cand("Tool", TrustTier::HostSigned, 100, "obs-a"),
        ];
        let ch = p.choose(&cs).unwrap();
        assert_eq!(cs[ch.index].value, "Tool", "higher tier must beat recency");
        // Within a tier band, the later HLC wins.
        let cs = vec![
            cand("Tool", TrustTier::AgentExtracted, 100, "obs-a"),
            cand("Library", TrustTier::AgentExtracted, 200, "obs-b"),
        ];
        assert_eq!(
            cs[p.choose(&cs).unwrap().index].value,
            "Library",
            "recency decides within a band"
        );
        // Full tie -> smallest observation id, independent of slice order (P16).
        let a = cand("Tool", TrustTier::AgentExtracted, 100, "obs-a");
        let b = cand("Library", TrustTier::AgentExtracted, 100, "obs-b");
        let fwd = vec![a.clone(), b.clone()];
        let rev = vec![b, a];
        assert_eq!(fwd[p.choose(&fwd).unwrap().index].value, "Tool");
        assert_eq!(
            rev[p.choose(&rev).unwrap().index].value,
            "Tool",
            "tie-break must not depend on slice order"
        );
        assert!(p.choose(&[]).is_none(), "no candidates -> no choice");
    }

    /// guard (resolution.md R6): contested iff distinct values survive at the TOP tier - a conflict
    /// a strictly higher tier resolves is not contested (it stays queryable elsewhere, R7).
    #[test]
    fn contested_iff_top_tier_ties() {
        let p = TierWeighted;
        // Tier-tied distinct values -> contested (the winner stands on recency alone).
        let cs = vec![
            cand("Tool", TrustTier::AgentExtracted, 100, "obs-a"),
            cand("Library", TrustTier::AgentExtracted, 200, "obs-b"),
        ];
        assert!(p.choose(&cs).unwrap().contested);
        // A higher tier decides -> not contested.
        let cs = vec![
            cand("Tool", TrustTier::HumanConfirmed, 100, "obs-a"),
            cand("Library", TrustTier::AgentExtracted, 200, "obs-b"),
        ];
        assert!(!p.choose(&cs).unwrap().contested);
        // Same value from many observations is corroboration, not a contest.
        let cs = vec![
            cand("Tool", TrustTier::AgentExtracted, 100, "obs-a"),
            cand("Tool", TrustTier::AgentExtracted, 200, "obs-b"),
        ];
        assert!(!p.choose(&cs).unwrap().contested);
    }

    /// guard (resolution.md R4 / Section 3): a sync-stamped attestation's claimed tier evaluates to
    /// at most HostSigned - a signature proves origin, never a human act - while a local attestation
    /// keeps its stored tier (the local observe path already forces the default).
    #[test]
    fn evaluated_tier_caps_remote_claimed() {
        let local = Provenance { trust_tier: TrustTier::HumanConfirmed, ..prov() };
        assert_eq!(evaluated_tier(&local), TrustTier::HumanConfirmed, "local stored tier stands");
        let identity = NodeIdentity::from_secret_bytes([9u8; 32]);
        let remote_claim = Provenance {
            trust_tier: TrustTier::HumanConfirmed,
            ..stamped(&identity, "cid", 1, Hlc { wall: 5, counter: 0, node: identity.node_id() })
        };
        assert_eq!(
            evaluated_tier(&remote_claim),
            TrustTier::HostSigned,
            "a wire claim never evaluates above HostSigned (F13)"
        );
        let remote_low = Provenance {
            trust_tier: TrustTier::Unverified,
            ..stamped(&identity, "cid", 2, Hlc { wall: 6, counter: 0, node: identity.node_id() })
        };
        assert_eq!(
            evaluated_tier(&remote_low),
            TrustTier::Unverified,
            "min() never raises a claim"
        );
    }

    /// The two markers live inside the reserved namespace the ingest surfaces refuse - if one
    /// moved out from under the prefix, the reservation would guard nothing.
    #[test]
    fn surface_markers_live_under_the_reserved_prefix() {
        assert!(VERDICT_SURFACE_CONSOLE.starts_with(VERDICT_SURFACE_PREFIX));
        assert!(VERDICT_SURFACE_AGENT.starts_with(VERDICT_SURFACE_PREFIX));
    }

    /// guard (resolution.md Section 6): only the console marker permits a HumanConfirmed grant;
    /// the agent marker, an absent marker, and any unknown marker cap at HostSigned.
    #[test]
    fn verdict_ceiling_by_surface_marker() {
        assert_eq!(verdict_grant_ceiling(Some(VERDICT_SURFACE_CONSOLE)), TrustTier::HumanConfirmed);
        assert_eq!(verdict_grant_ceiling(Some(VERDICT_SURFACE_AGENT)), TrustTier::HostSigned);
        assert_eq!(verdict_grant_ceiling(None), TrustTier::HostSigned);
        assert_eq!(verdict_grant_ceiling(Some("surface:web")), TrustTier::HostSigned);
    }
}
