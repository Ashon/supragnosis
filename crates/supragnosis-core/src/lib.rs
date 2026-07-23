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
    /// Federation sync metadata (M4, docs/federation.md Section 3) - assigned once by the origin at
    /// authoring/stamping time, preserved verbatim through relays. `None` for pre-federation or
    /// local-unsigned attestations (they stay local-only until backfill-stamped at first export).
    /// **Not part of the content address** (F2): it lives on the attestation, and `Provenance` is not
    /// hashed - identical content on two nodes keeps one id. It IS an attestation-distinguishing axis
    /// (see [`provenance_order`], the P14 compile-forced enumeration).
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Hlc {
    pub wall: u64,
    pub counter: u32,
    pub node: String,
}

impl Hlc {
    /// Advance for a local event: wall catches up to physical time, the counter breaks same-wall ties.
    pub fn tick(prev: &Hlc, now: Timestamp, node: &str) -> Hlc {
        let wall = prev.wall.max(now);
        let counter = if wall == prev.wall { prev.counter + 1 } else { 0 };
        Hlc { wall, counter, node: node.to_string() }
    }

    /// Merge on receiving a remote stamp (standard HLC receive rule): never runs backwards, and lands
    /// strictly after both `prev` and `remote` when physical time has not caught up.
    pub fn merge(prev: &Hlc, remote: &Hlc, now: Timestamp, node: &str) -> Hlc {
        let wall = prev.wall.max(remote.wall).max(now);
        let c_prev = if wall == prev.wall { prev.counter + 1 } else { 0 };
        let c_remote = if wall == remote.wall { remote.counter + 1 } else { 0 };
        Hlc { wall, counter: c_prev.max(c_remote), node: node.to_string() }
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

/// Node identity for federation (docs/federation.md Section 2): an ed25519 keypair whose public-key
/// fingerprint IS the node_id (self-certifying, immutable - it keys the version vector and is the final
/// HLC tiebreak). Key material persistence (data_dir/node.key) is the caller's IO concern - core only
/// takes the 32 secret bytes, staying zero-IO (P20).
pub struct NodeIdentity {
    signing: ed25519_dalek::SigningKey,
}

impl NodeIdentity {
    /// Builds the identity from 32 secret bytes (generated once by the caller and persisted under
    /// data_dir; tests may use fixed bytes).
    pub fn from_secret_bytes(secret: [u8; 32]) -> Self {
        Self { signing: ed25519_dalek::SigningKey::from_bytes(&secret) }
    }

    /// Hex public key - what other nodes put on their allowlist / the canon policy binding.
    pub fn public_key_hex(&self) -> String {
        hex_encode(self.signing.verifying_key().as_bytes())
    }

    /// node_id = blake3(public key), truncated to 32 hex chars (128 bits). Never configured, never
    /// changed (F14) - a display label is a separate field.
    pub fn node_id(&self) -> String {
        node_id_from_public_key(self.signing.verifying_key().as_bytes())
    }

    /// Signs an attestation's canonical bytes ([`attestation_signing_bytes`]); returns hex.
    /// `meta.signature` is ignored as input and is what this output should be stored into.
    pub fn sign_attestation(&self, content_id: &str, p: &Provenance, meta: &SyncMeta) -> String {
        use ed25519_dalek::Signer;
        let bytes = attestation_signing_bytes(content_id, p, meta);
        hex_encode(&self.signing.sign(&bytes).to_bytes())
    }
}

/// Derives the immutable node_id from raw public-key bytes (federation.md Section 2).
pub fn node_id_from_public_key(pubkey: &[u8]) -> String {
    blake3::hash(pubkey).to_hex().to_string()[..32].to_string()
}

/// Verifies an attestation signature against a hex public key. `false` for any malformed input -
/// verification failure is rejection, not an error path (F6).
pub fn verify_attestation(pubkey_hex: &str, content_id: &str, p: &Provenance, meta: &SyncMeta) -> bool {
    use ed25519_dalek::Verifier;
    let Some(pk_bytes) = hex_decode(pubkey_hex) else { return false };
    let Ok(pk_arr) = <[u8; 32]>::try_from(pk_bytes.as_slice()) else { return false };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&pk_arr) else { return false };
    let Some(sig_bytes) = hex_decode(&meta.signature) else { return false };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else { return false };
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    key.verify(&attestation_signing_bytes(content_id, p, meta), &sig).is_ok()
}

/// Canonical bytes an origin signs (docs/federation.md Section 3): `(content_id, origin_node,
/// origin_seq, hlc, host, on_behalf_of, workspace, source_ref, observed_at, confidence, trust_tier,
/// lineage-as-declared)`, in the same length-prefixed deterministic encoding as [`hash_field`]
/// (P14 anti-collision) - a relay reproduces identical bytes, so the origin's signature verifies
/// unchanged downstream. `meta.signature` itself is (necessarily) not part of the signed bytes.
///
/// Full destructuring (no `..`, Principle 14): adding a field to `Provenance`/`SyncMeta` breaks this
/// function, forcing an explicit decision on whether the new field is signed (origin-asserted) or not.
pub fn attestation_signing_bytes(content_id: &str, p: &Provenance, meta: &SyncMeta) -> Vec<u8> {
    let Provenance {
        host,
        on_behalf_of,
        workspace,
        source_ref,
        observed_at,
        confidence,
        trust_tier,
        sync: _, // the block being signed is passed as `meta`; never sign a signature
    } = p;
    let SyncMeta { origin_node, origin_seq, hlc, signature: _, lineage } = meta;
    // Exhaustively destructure the nested Hlc too (Principle 14: mechanical enforcement). Field access
    // (`hlc.wall`) would compile silently when a field is added to Hlc, letting that field ride the wire
    // and distinguish attestations (via Hlc's derive(Ord)) while staying OUTSIDE the signed bytes - a
    // field a relay could forge unnoticed. Destructuring forces an explicit signed-vs-derived decision.
    let Hlc { wall, counter, node: hlc_node } = hlc;
    let mut buf = Vec::new();
    push_field(&mut buf, content_id.as_bytes());
    push_field(&mut buf, origin_node.as_bytes());
    buf.extend_from_slice(&origin_seq.to_le_bytes());
    buf.extend_from_slice(&wall.to_le_bytes());
    buf.extend_from_slice(&counter.to_le_bytes());
    push_field(&mut buf, hlc_node.as_bytes());
    push_field(&mut buf, host.as_bytes());
    push_opt_field(&mut buf, on_behalf_of.as_deref());
    push_field(&mut buf, workspace.as_bytes());
    push_opt_field(&mut buf, source_ref.as_deref());
    buf.extend_from_slice(&observed_at.to_le_bytes());
    match confidence {
        Some(c) => {
            buf.push(1);
            buf.extend_from_slice(&c.to_bits().to_le_bytes());
        }
        None => buf.push(0),
    }
    buf.push(match trust_tier {
        TrustTier::Unverified => 0,
        TrustTier::AgentExtracted => 1,
        TrustTier::HostSigned => 2,
        TrustTier::HumanConfirmed => 3,
    });
    buf.extend_from_slice(&(lineage.len() as u64).to_le_bytes());
    for l in lineage {
        push_field(&mut buf, l.as_bytes());
    }
    buf
}

/// Length-prefixed append (the signing-bytes counterpart of [`hash_field`]).
fn push_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn push_opt_field(buf: &mut Vec<u8>, v: Option<&str>) {
    match v {
        Some(s) => {
            buf.push(1);
            push_field(buf, s.as_bytes());
        }
        None => buf.push(0),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
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
        self.entities.is_empty()
            && self.relations.is_empty()
            && self.type_defs.is_empty()
            && self.proposal_events.is_empty()
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
            proposal_events,
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
    /// **Not included in the content-address id computation** - an embedding is only a local aid that widens recall,
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
    pub fn with_assertions(content: String, provenance: Provenance, assertions: Assertions) -> Self {
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

    /// Well-formedness of a reconstructed observation - the ingest gate every surface must apply before
    /// the permanent, append-only log accepts it (Principle 1: well-formedness only, never content
    /// censorship; Principle 2: the confidence range is schema-enforced at ingest). A signature proves
    /// origin, not that the content holds (Principle 18), so the sync apply path runs this on a verified
    /// peer event: without it a signed-but-malformed event (an out-of-range confidence, an empty-referent
    /// assertion) would contaminate the log even though the local observe path refuses the same shape.
    /// This checks structure only - notation is preserved verbatim and normalization stays the projection's job.
    pub fn check_well_formed(&self) -> Result<(), String> {
        for p in &self.provenance {
            if let Some(c) = p.confidence {
                // `contains` is false for NaN too, so a non-finite confidence is caught here.
                if !(0.0..=1.0).contains(&c) {
                    return Err(format!("confidence is out of the range 0.0~1.0 (received: {c})"));
                }
            }
        }
        for e in &self.assertions.entities {
            if e.name.trim().is_empty() {
                return Err("an entity assertion has an empty name (a non-assertion with no referent)".into());
            }
            if e.kind.as_deref().is_some_and(|k| k.trim().is_empty()) {
                return Err(format!("entity '{}' has an empty-string type (omit the type instead)", e.name));
            }
        }
        for r in &self.assertions.relations {
            if r.from.trim().is_empty() || r.to.trim().is_empty() {
                return Err(format!("a relation endpoint is empty (from: {:?}, to: {:?})", r.from, r.to));
            }
            if normalize_relation_kind(&r.kind).is_empty() {
                return Err(format!("the relation type normalizes to an empty string (received: {:?})", r.kind));
            }
        }
        for t in &self.assertions.type_defs {
            if t.name.trim().is_empty() || t.description.trim().is_empty() {
                return Err("a type definition has an empty name or description (Principle 8)".into());
            }
        }
        Ok(())
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
        .map(|p| p.sync.as_ref().map(|s| s.hlc.clone()).unwrap_or_else(|| Hlc::legacy(p.observed_at)))
        .min()
        .unwrap_or_default()
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
            sync: None,
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
                proposal_events: vec![],
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
                proposal_events: vec![],
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
                entities: vec![EntityAssertion { description: None,
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
                entities: vec![EntityAssertion { description: None, name: "rmcp".into(), kind: None }],
                relations: vec![],
                type_defs: vec![],
                proposal_events: vec![],
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
                proposal_events: vec![],
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

    // --- M4 Phase 1: federation core foundations (docs/federation.md Section 10) -----------------

    /// Builds a sync-stamped attestation from `identity` (signed - the test-side stamping pipeline).
    fn stamped(identity: &NodeIdentity, content_id: &str, seq: u64, hlc: Hlc) -> Provenance {
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
        // Legacy fallback is deterministic and orders by observed_at.
        assert!(Hlc::legacy(5) < Hlc::legacy(6));
    }

    #[test]
    fn signature_roundtrip_verifies_and_tamper_fails() {
        let identity = NodeIdentity::from_secret_bytes([42u8; 32]);
        let obs = Observation::new("signed fact".into(), prov());
        let p = stamped(&identity, &obs.id, 1, Hlc { wall: 7, counter: 0, node: identity.node_id() });
        let meta = p.sync.clone().expect("stamped");
        let pubkey = identity.public_key_hex();
        assert!(verify_attestation(&pubkey, &obs.id, &p, &meta), "round-trip must verify");
        // Tampering any signed field breaks verification (F6): trust tier...
        let mut t1 = p.clone();
        t1.trust_tier = TrustTier::HumanConfirmed;
        assert!(!verify_attestation(&pubkey, &obs.id, &t1, &meta), "tier tamper must fail");
        // ...the lineage declaration (F13: a relay cannot forge/strip lineage)...
        let mut m2 = meta.clone();
        m2.lineage = Vec::new();
        assert!(!verify_attestation(&pubkey, &obs.id, &p, &m2), "lineage strip must fail");
        // ...and a different key cannot claim the event.
        let other = NodeIdentity::from_secret_bytes([43u8; 32]);
        assert!(!verify_attestation(&other.public_key_hex(), &obs.id, &p, &meta));
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

    #[test]
    fn ordering_hlc_takes_earliest_and_falls_back_to_legacy() {
        // Legacy attestation -> Hlc::legacy(observed_at).
        let legacy = Observation::new("old".into(), Provenance { observed_at: 40, ..prov() });
        assert_eq!(ordering_hlc(&legacy), Hlc::legacy(40));
        // Stamped + legacy mixed -> the earliest effective HLC wins (federation.md Section 4).
        let identity = NodeIdentity::from_secret_bytes([1u8; 32]);
        let mut obs = Observation::new("fact".into(), Provenance { observed_at: 100, ..prov() });
        let stamped_p = stamped(&identity, &obs.id, 1, Hlc { wall: 60, counter: 2, node: identity.node_id() });
        obs.absorb(Observation::new("fact".into(), stamped_p));
        assert_eq!(ordering_hlc(&obs), Hlc { wall: 60, counter: 2, node: identity.node_id() });
    }

    #[test]
    fn absorb_stamp_upgrade_supersedes_unstamped_base() {
        // The stamped version of the same attestation REPLACES its unstamped base (backfill
        // write-back, federation.md Phase 2) - both directions, so the upgrade is commutative (P16).
        let identity = NodeIdentity::from_secret_bytes([3u8; 32]);
        let base = Observation::new("fact".into(), prov());
        let stamped_p = stamped(&identity, &base.id, 1, Hlc { wall: 5, counter: 0, node: identity.node_id() });

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
        let s2 = stamped(&identity, &o3.id, 2, Hlc { wall: 6, counter: 0, node: identity.node_id() });
        o3.absorb(Observation::new("fact".into(), s2));
        o3.absorb(Observation::new("fact".into(), other));
        assert_eq!(o3.provenance.len(), 2, "distinct attestations still union");
    }

    #[test]
    fn node_id_derives_from_public_key_and_is_stable() {
        let a = NodeIdentity::from_secret_bytes([5u8; 32]);
        let b = NodeIdentity::from_secret_bytes([5u8; 32]);
        let c = NodeIdentity::from_secret_bytes([6u8; 32]);
        assert_eq!(a.node_id(), b.node_id(), "same key -> same node_id (immutable, F14)");
        assert_ne!(a.node_id(), c.node_id());
        assert_eq!(a.node_id().len(), 32);
        assert_eq!(a.node_id(), node_id_from_public_key(&hex_decode(&a.public_key_hex()).unwrap()));
    }
}
