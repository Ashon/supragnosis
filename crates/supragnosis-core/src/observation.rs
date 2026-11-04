//! Observations and their assertions: the content address, the ingest gate, the monotonic
//! absorb and the fold-ordering key, with the length-prefixed hashing the ids share.

use serde::{Deserialize, Serialize};

use crate::{normalize_relation_kind, Hlc, Provenance, SyncMeta, Timestamp, TrustTier};

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
    /// Proposal-workflow events (Principle 23 / I1: a proposal and its verdicts are observations, no
    /// separate side store). The proposal state is a deterministic fold of these events (I2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposal_events: Vec<ProposalEventAssertion>,
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

/// Kind of proposal-workflow event (I1: all are observations). The state machine folds these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalEventKind {
    /// Open a proposal (the opening observation's id becomes the proposal id).
    Opened,
    /// Cast a verdict (merge/reject - carried in the payload). The deciding event (I3).
    Verdict,
    /// Withdraw by the proposer.
    Withdrawn,
    /// A review comment (not a verdict).
    Comment,
}

impl ProposalEventKind {
    /// Every event, so a reader that has a word for each can be checked against the whole list.
    pub const ALL: [ProposalEventKind; 4] = [
        ProposalEventKind::Opened,
        ProposalEventKind::Verdict,
        ProposalEventKind::Withdrawn,
        ProposalEventKind::Comment,
    ];

    /// Stable discriminant byte for content-address hashing.
    fn tag(self) -> u8 {
        match self {
            ProposalEventKind::Opened => 0,
            ProposalEventKind::Verdict => 1,
            ProposalEventKind::Withdrawn => 2,
            ProposalEventKind::Comment => 3,
        }
    }
}

/// One proposal-workflow event enclosed in an observation (Principle 23 / I1). The `payload` is a JSON
/// string whose shape depends on `event` (opened: {kind, targets, into, rationale}; verdict:
/// {decision}) - kept as an opaque string here so the core stays free of workflow-specific structs; the
/// engine parses it. Content identity: a different event/payload is a different observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalEventAssertion {
    /// Target proposal id. Empty on `opened` (the opening observation's id becomes the proposal id).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proposal: String,
    pub event: ProposalEventKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub payload: String,
}

/// Feeds a field into the hash with a length-prefix. Delimiter (`\0`) concatenation has ambiguous boundaries, so
/// planting a delimiter in content could construct the same byte stream as a different combination of fields
/// (id hijacking, Principle 18). A length prefix lets the stream itself fix each field's extent, so boundary manipulation is impossible.
pub(crate) fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
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
        // Destructured with no `..`: a field added to the envelope is a compile error here until it
        // is counted, or an event carrying only it would be serialized as carrying nothing.
        let Assertions { entities, relations, type_defs, proposal_events } = self;
        entities.is_empty()
            && relations.is_empty()
            && type_defs.is_empty()
            && proposal_events.is_empty()
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
        let Assertions { entities, relations, type_defs, proposal_events } = self;
        hasher.update(&(entities.len() as u64).to_le_bytes());
        for e in entities {
            let EntityAssertion { name, kind, description } = e;
            hash_field(hasher, name.as_bytes());
            hash_opt_field(hasher, kind.as_deref());
            hash_opt_field(hasher, description.as_deref());
        }
        hasher.update(&(relations.len() as u64).to_le_bytes());
        for r in relations {
            let RelationAssertion { from, kind, to, description, valid_from, valid_to } = r;
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
        hasher.update(&(proposal_events.len() as u64).to_le_bytes());
        for p in proposal_events {
            let ProposalEventAssertion { proposal, event, payload } = p;
            hash_field(hasher, proposal.as_bytes());
            hasher.update(&[event.tag()]);
            hash_field(hasher, payload.as_bytes());
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
    /// **Not included in the content-address id computation** - an embedding is only a local aid that widens retrieval,
    /// not content identity, and using a different model per node does not shake identity/convergence.
    ///
    /// Excluded from serde entirely (Principle 21, symmetric with Entity.embedding): when an observation goes out as an MCP
    /// resource (observation/{id}), hundreds of floats must not pollute the LLM context. Persistence is handled by the store adapter with a hand-rolled encoding.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

/// The current content-address formula: blake3(workspace + content + assertions), length-prefixed.
/// Exposed so callers can detect **legacy-format rows** - observations stored under an earlier id
/// formula (0.x evolution: the description field, type_defs, proposal_events each extended the
/// assertion encoding). A row whose stored id no longer matches this recomputation is local history:
/// its signatures cannot verify remotely, so it never crosses the sync wire (the migrate command
/// re-creates it under the current id).
pub fn observation_content_id(workspace: &str, content: &str, assertions: &Assertions) -> String {
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, workspace.as_bytes());
    hash_field(&mut hasher, content.as_bytes());
    assertions.hash_into(&mut hasher);
    hasher.finalize().to_hex().to_string()
}

impl Observation {
    /// Content-address ID = blake3(workspace + content). The same id no matter the path (server/peer) it comes in through -> dedup.
    pub fn new(content: String, provenance: Provenance) -> Self {
        Self::with_assertions(content, provenance, Assertions::default())
    }

    /// An observation enclosing structured assertions. If the assertions are empty the id is identical to `new`,
    /// and if there are assertions they are included in the id computation. Every field is encoded with a
    /// length-prefix, so boundary manipulation that plants a delimiter in content cannot collide it with another observation.
    pub fn with_assertions(
        content: String,
        provenance: Provenance,
        assertions: Assertions,
    ) -> Self {
        let id = observation_content_id(&provenance.workspace, &content, &assertions);
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

    /// Principle 2: knowledge without provenance cannot exist. The constructors take one attestation
    /// by value, but the type is a list and a literal can leave it empty, so this is checked where
    /// an observation enters - by [`Observation::check_well_formed`] at ingest and by every store
    /// adapter's `add_observation` - rather than left to how the value happened to be built.
    pub fn check_attested(&self) -> Result<(), String> {
        if self.provenance.is_empty() {
            return Err("the observation carries no attestation - knowledge without provenance \
                        cannot exist (Principle 2)"
                .into());
        }
        Ok(())
    }

    /// Well-formedness of a reconstructed observation - the ingest gate every surface must apply before
    /// the permanent, append-only log accepts it (Principle 1: well-formedness only, never content
    /// censorship; Principle 2: the confidence range is schema-enforced at ingest). A signature proves
    /// origin, not that the content holds (Principle 18), so the sync apply path runs this on a verified
    /// peer event: without it a signed-but-malformed event (an out-of-range confidence, an empty-referent
    /// assertion) would contaminate the log even though the local observe path refuses the same shape.
    /// This checks structure only - notation is preserved verbatim and normalization stays the projection's job.
    pub fn check_well_formed(&self) -> Result<(), String> {
        self.check_attested()?;
        for p in &self.provenance {
            if let Some(c) = p.confidence {
                // `contains` is false for NaN too, so a non-finite confidence is caught here.
                if !(0.0..=1.0).contains(&c) {
                    return Err(format!("confidence is out of the range 0.0~1.0 (received: {c})"));
                }
            }
        }
        // Destructured with no `..`: a field added to the envelope is a compile error here until it
        // is checked, or decided to need no check - a peer's event would carry it in unchecked.
        let Assertions { entities, relations, type_defs, proposal_events } = &self.assertions;
        // A proposal event's payload is data the proposal fold interprets at read time, where one it
        // cannot read counts as empty; the gate judges structure, not proposals (proposal-workflow.md
        // I1).
        let _ = proposal_events;
        for e in entities {
            if e.name.trim().is_empty() {
                return Err(
                    "an entity assertion has an empty name (a non-assertion with no referent)"
                        .into(),
                );
            }
            if e.kind.as_deref().is_some_and(|k| k.trim().is_empty()) {
                return Err(format!(
                    "entity '{}' has an empty-string type (omit the type instead)",
                    e.name
                ));
            }
        }
        for r in relations {
            if r.from.trim().is_empty() || r.to.trim().is_empty() {
                return Err(format!(
                    "a relation endpoint is empty (from: {:?}, to: {:?})",
                    r.from, r.to
                ));
            }
            if normalize_relation_kind(&r.kind).is_empty() {
                return Err(format!(
                    "the relation type normalizes to an empty string (received: {:?})",
                    r.kind
                ));
            }
        }
        for t in type_defs {
            if t.name.trim().is_empty() || t.description.trim().is_empty() {
                return Err(
                    "a type definition has an empty name or description (Principle 8)".into()
                );
            }
        }
        Ok(())
    }

    /// **Monotonically merges** a re-arrival at the same content address (Principle 3: no overwriting).
    /// Accumulates non-identity fields (provenance attestations, derived_from lineage) as a union -
    /// the union is commutative/associative/idempotent, so it converges to the same result regardless of arrival order (Principle 16).
    /// Relay duplicates (fully identical attestations) are naturally deduped, and independent re-observations (attestations
    /// differing in any field) accumulate. Identity fields (content/assertions) are identical when the id matches, so they
    /// are not touched. An embedding is only a retrieval aid (Principle 19), so the existing value is
    /// kept and one is only taken when absent.
    pub fn absorb(&mut self, other: Observation) {
        debug_assert_eq!(self.id, other.id, "absorb only between the same content address");
        // Full destructuring (no `..`, Principle 14: mechanical enforcement of structural evolution): when a
        // field is added to Observation (M4: origin/hlc/signature planned) this becomes a compile error,
        // forcing an explicit decision on "how that field is merged on re-arrival" -
        // a silent omission is the loss of that field on merge. Identity fields (id/content/assertions) are
        // discarded since the content address guarantees identity (a `_` binding is also a notation of the decision).
        let Observation { id: _, content: _, provenance, assertions: _, derived_from, embedding } =
            other;
        self.provenance.extend(provenance);
        self.provenance.sort_by(provenance_order);
        self.provenance
            .dedup_by(|a, b| provenance_order(a, b) == std::cmp::Ordering::Equal);
        // Stamp upgrade (federation.md Phase 2, backfill): a sync-stamped attestation SUPERSEDES an
        // unstamped attestation with identical base fields - the stamp is transport metadata enriching
        // the same attestation, not a second attestation, so keeping both would double-count one act.
        // Monotonic (a stamp is never lost, only gained - same family as embedding take-when-absent),
        // commutative/idempotent, so union convergence (P16) is preserved.
        if self.provenance.iter().any(|p| p.sync.is_some()) {
            let stamped_bases: Vec<Provenance> = self
                .provenance
                .iter()
                .filter(|p| p.sync.is_some())
                .map(|p| Provenance { sync: None, ..p.clone() })
                .collect();
            self.provenance.retain(|p| {
                p.sync.is_some()
                    || !stamped_bases
                        .iter()
                        .any(|b| provenance_order(b, p) == std::cmp::Ordering::Equal)
            });
        }
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
    type Key<'a> = (
        &'a str,
        Option<&'a str>,
        &'a str,
        Option<&'a str>,
        Timestamp,
        Option<u32>,
        TrustTier,
        Option<(&'a str, u64, &'a Hlc, &'a str, &'a [String])>,
    );
    fn key(p: &Provenance) -> Key<'_> {
        let Provenance {
            host,
            on_behalf_of,
            workspace,
            source_ref,
            observed_at,
            confidence,
            trust_tier,
            sync,
        } = p;
        (
            host.as_str(),
            on_behalf_of.as_deref(),
            workspace.as_str(),
            source_ref.as_deref(),
            *observed_at,
            confidence.map(f32::to_bits),
            *trust_tier,
            // Sync metadata IS an attestation-distinguishing axis (federation.md Section 3): a relay
            // copy is byte-identical (dedups), while re-stamps / distinct origins stay separate
            // attestations. Exhaustive destructuring, same discipline as the outer struct.
            sync.as_ref().map(|s| {
                let SyncMeta { origin_node, origin_seq, hlc, signature, lineage } = s;
                (origin_node.as_str(), *origin_seq, hlc, signature.as_str(), lineage.as_slice())
            }),
        )
    }
    key(a).cmp(&key(b))
}

/// The observation's deterministic fold-ordering key (federation.md Section 4): the earliest effective
/// HLC over its attestations - the authoring attestation for the common single-attestation case, the
/// minimum for the rare same-content-authored-twice case (convergent: a minimum over a fixed set).
/// A pre-federation attestation falls back to [`Hlc::legacy`] on its `observed_at`. Convergent but not
/// monotonic under true concurrency - finality rests on absorbing states + causal stability, never on
/// this ordering alone (federation.md Section 4/7a).
pub fn ordering_hlc(obs: &Observation) -> Hlc {
    obs.provenance
        .iter()
        .map(|p| {
            p.sync
                .as_ref()
                .map(|s| s.hlc.clone())
                .unwrap_or_else(|| Hlc::legacy(p.observed_at))
        })
        .min()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{prov, stamped};
    use crate::NodeIdentity;

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
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
            },
        );
        assert_ne!(plain.id, asserted.id);

        // Different assertion content gives a different id (type assignment is also content identity).
        let retyped = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: Some("Project".into()),
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
            },
        );
        assert_ne!(asserted.id, retyped.id);

        // The same assertion gives the same id no matter which path it comes through (determinism).
        let again = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
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
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
            },
        );
        assert_ne!(crafted.id, asserted.id, "a boundary manipulation collision must be blocked");

        // Option presence encoding: an unspecified type and an empty-string type are different assertions.
        let untyped = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: None,
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
            },
        );
        let empty_typed = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    description: None,
                    name: "rmcp".into(),
                    kind: Some(String::new()),
                }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
            },
        );
        assert_ne!(untyped.id, empty_typed.id);
    }

    /// absorb's monotonic union: the same result regardless of arrival order (commutative), relay duplicates are
    /// naturally deduped (idempotent), and independent re-observations accumulate (Principle 3/16).
    #[test]
    fn absorb_union_is_order_independent_and_idempotent() {
        let prov_a = Provenance { host: "host-a".into(), confidence: Some(0.9), ..prov() };
        let prov_b = Provenance { host: "host-b".into(), confidence: Some(0.1), ..prov() };

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
        let state =
            |o: &Observation| serde_json::to_value((&o.provenance, &o.derived_from)).unwrap();
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
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
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
    fn cross_node_identical_id_dedups_and_unions() {
        // F2: identical content on two nodes -> identical id (sync metadata is outside the address).
        let ida = NodeIdentity::from_secret_bytes([7u8; 32]);
        let idb = NodeIdentity::from_secret_bytes([9u8; 32]);
        let a = Observation::new("shared fact".into(), prov());
        let pa = stamped(&ida, &a.id, 1, Hlc { wall: 10, counter: 0, node: ida.node_id() });
        let pb = stamped(&idb, &a.id, 1, Hlc { wall: 11, counter: 0, node: idb.node_id() });
        let mut obs_a = Observation::new("shared fact".into(), pa.clone());
        let obs_b = Observation::new("shared fact".into(), pb);
        assert_eq!(obs_a.id, obs_b.id, "sync metadata must not fork the content address (F2)");
        // Distinct attestations union (P3)...
        obs_a.absorb(obs_b.clone());
        assert_eq!(obs_a.provenance.len(), 2, "independent origins accumulate");
        // ...while a relay copy (byte-identical attestation) dedups.
        obs_a.absorb(Observation::new("shared fact".into(), pa));
        assert_eq!(obs_a.provenance.len(), 2, "a relay duplicate must dedup (F7)");
    }

    #[test]
    fn ordering_hlc_takes_earliest_and_falls_back_to_legacy() {
        // Legacy attestation -> Hlc::legacy(observed_at).
        let legacy = Observation::new("old".into(), Provenance { observed_at: 40, ..prov() });
        assert_eq!(ordering_hlc(&legacy), Hlc::legacy(40));
        // Stamped + legacy mixed -> the earliest effective HLC wins (federation.md Section 4).
        let identity = NodeIdentity::from_secret_bytes([1u8; 32]);
        let mut obs = Observation::new("fact".into(), Provenance { observed_at: 100, ..prov() });
        let stamped_p =
            stamped(&identity, &obs.id, 1, Hlc { wall: 60, counter: 2, node: identity.node_id() });
        obs.absorb(Observation::new("fact".into(), stamped_p));
        assert_eq!(ordering_hlc(&obs), Hlc { wall: 60, counter: 2, node: identity.node_id() });
    }

    #[test]
    fn absorb_stamp_upgrade_supersedes_unstamped_base() {
        // The stamped version of the same attestation REPLACES its unstamped base (backfill
        // write-back, federation.md Phase 2) - both directions, so the upgrade is commutative (P16).
        let identity = NodeIdentity::from_secret_bytes([3u8; 32]);
        let base = Observation::new("fact".into(), prov());
        let stamped_p =
            stamped(&identity, &base.id, 1, Hlc { wall: 5, counter: 0, node: identity.node_id() });

        // unstamped + stamped -> single stamped attestation.
        let mut o = Observation::new("fact".into(), prov());
        o.absorb(Observation::new("fact".into(), stamped_p.clone()));
        assert_eq!(o.provenance.len(), 1, "stamp must upgrade, not duplicate");
        assert!(o.provenance[0].sync.is_some());

        // stamped + unstamped (reverse arrival) -> still the single stamped attestation.
        let mut o2 = Observation::new("fact".into(), stamped_p);
        o2.absorb(Observation::new("fact".into(), prov()));
        assert_eq!(o2.provenance.len(), 1, "upgrade must be arrival-order independent");
        assert!(o2.provenance[0].sync.is_some());

        // A genuinely different attestation (different observed_at) is NOT superseded (P3).
        let mut o3 = Observation::new("fact".into(), prov());
        let other = Provenance { observed_at: 99, ..prov() };
        let s2 =
            stamped(&identity, &o3.id, 2, Hlc { wall: 6, counter: 0, node: identity.node_id() });
        o3.absorb(Observation::new("fact".into(), s2));
        o3.absorb(Observation::new("fact".into(), other));
        assert_eq!(o3.provenance.len(), 2, "distinct attestations still union");
    }
}
