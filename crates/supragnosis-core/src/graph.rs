//! Entities and relations, the projection's rows: their deterministic ids, relation-type
//! normalization and the hyperedge id.

use serde::{Deserialize, Serialize};

use crate::observation::hash_field;
use crate::{Provenance, Timestamp};

#[cfg(doc)]
use crate::Observation;

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
    /// (Optional) embedding vector for semantic search (Principle 19: probabilistic boundary). A retrieval aid that lets the
    /// node be reached by the meaning of its name/aliases - like observations, **not included in the id computation** (it is
    /// recall expansion, not identity, and using a different model per node does not shake identity/convergence).
    ///
    /// Excluded from serde entirely (Principle 21): this vector is only an internal recall machine, so leaking it through the MCP
    /// surface (get_entity) would pollute the LLM context with hundreds of floats. Persistence is handled by the store
    /// adapter with a hand-rolled encoding (the row's stored JSON), so it is not a domain serialization target.
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
    /// The authoring attestation of the HLC-latest observation asserting this edge. It decides the
    /// edge's confidence, and its tier only when no observation in the log states the edge - the
    /// tier is otherwise the highest effective tier among the observations that do.
    pub provenance: Provenance,
    /// Every attestation of every observation asserting this edge, credited once per observation,
    /// as an entity's provenance is (Principle 3: accumulated, never replaced). It says how many
    /// support the edge and changes nothing about how far it is trusted.
    /// Empty on a row written before relations accumulated, until the next re-materialization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attestations: Vec<Provenance>,
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
        assert_eq!(normalize_relation_kind(&normalize_relation_kind("dependsOn")), "depends_on");
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
}
