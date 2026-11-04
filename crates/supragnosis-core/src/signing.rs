//! Node identity and signing: the node key, message and attestation signatures, the
//! canonical attestation encodings (the signed bytes and the app record key) and the one hex
//! spelling.

use crate::{Hlc, Provenance, SyncMeta, TrustTier};

#[cfg(doc)]
use crate::hash_field;

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

    /// Signs a message that is not an attestation, such as a join's answer (docs/pairing.md Section
    /// 4.3.2), under `context`; returns hex. The signed bytes are [`message_signing_bytes`], which
    /// never parse as an attestation's, so neither signature passes for the other.
    pub fn sign_message(&self, context: &str, message: &[u8]) -> String {
        use ed25519_dalek::Signer;
        hex_encode(&self.signing.sign(&message_signing_bytes(context, message)).to_bytes())
    }
}

/// What [`NodeIdentity::sign_message`] signs: a fixed tag, the context, a zero byte, the message.
///
/// An attestation's signed bytes open with its content id's length as eight little-endian bytes,
/// so they start with a small number. These start with the ASCII tag "supragnosis message", whose
/// first eight bytes read as a length above 2^62 - a buffer no attestation can have. A context
/// keeps one kind of message from passing for another.
pub fn message_signing_bytes(context: &str, message: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + context.len() + message.len());
    buf.extend_from_slice(b"supragnosis message\0");
    buf.extend_from_slice(context.as_bytes());
    buf.push(0);
    buf.extend_from_slice(message);
    buf
}

/// Verifies a [`NodeIdentity::sign_message`] signature. `false` for any malformed input.
pub fn verify_message(
    pubkey_hex: &str,
    context: &str,
    message: &[u8],
    signature_hex: &str,
) -> bool {
    use ed25519_dalek::Verifier;
    let (Some(pk), Some(sig)) = (hex_decode(pubkey_hex), hex_decode(signature_hex)) else {
        return false;
    };
    let (Ok(pk), Ok(sig)) =
        (<[u8; 32]>::try_from(pk.as_slice()), <[u8; 64]>::try_from(sig.as_slice()))
    else {
        return false;
    };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    key.verify(
        &message_signing_bytes(context, message),
        &ed25519_dalek::Signature::from_bytes(&sig),
    )
    .is_ok()
}

/// Whether `s` is an ed25519 public key in the one spelling this software writes: 64 lowercase hex
/// digits. Configuration is checked with it when read, so a key that would never verify is named
/// rather than silently failing every event it was meant to admit (sync-correctness.md Section 7).
pub fn is_canonical_public_key_hex(s: &str) -> bool {
    s.len() == 64 && hex_decode(s).is_some()
}

/// Whether 32 bytes are an ed25519 public key a signature can be checked against - a point on the
/// curve, not merely the right length.
pub fn is_verifying_key(bytes: &[u8; 32]) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(bytes).is_ok()
}

/// Derives the immutable node_id from raw public-key bytes (federation.md Section 2).
pub fn node_id_from_public_key(pubkey: &[u8]) -> String {
    blake3::hash(pubkey).to_hex().to_string()[..32].to_string()
}

/// Verifies an attestation signature against a hex public key. `false` for any malformed input -
/// verification failure is rejection, not an error path (F6).
pub fn verify_attestation(
    pubkey_hex: &str,
    content_id: &str,
    p: &Provenance,
    meta: &SyncMeta,
) -> bool {
    use ed25519_dalek::Verifier;
    let Some(pk_bytes) = hex_decode(pubkey_hex) else {
        return false;
    };
    let Ok(pk_arr) = <[u8; 32]>::try_from(pk_bytes.as_slice()) else {
        return false;
    };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&pk_arr) else {
        return false;
    };
    let Some(sig_bytes) = hex_decode(&meta.signature) else {
        return false;
    };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else {
        return false;
    };
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

/// The key a node keeps the app that wrote an attestation under (design-system.md Section 29.8):
/// the observation's id and a hash of every field of the attestation but its sync stamp. Every
/// field is named here, as in [`attestation_signing_bytes`] and `provenance_order`, so one added
/// later is decided for this key too (Principle 14); the stamp is left out so that stamping a local
/// attestation at its first export does not move it. Two attestations that differ in any other
/// field have two keys.
pub fn attestation_key(observation_id: &str, p: &Provenance) -> String {
    let Provenance {
        host,
        on_behalf_of,
        workspace,
        source_ref,
        observed_at,
        confidence,
        trust_tier,
        sync: _, // added at first export (federation.md Section 3) - not part of which attestation it is
    } = p;
    let mut buf = Vec::new();
    push_field(&mut buf, observation_id.as_bytes());
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
    format!("{observation_id}:{}", blake3::hash(&buf).to_hex())
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

/// Decodes the one spelling [`hex_encode`] writes: lowercase digits, in pairs (sync-correctness.md
/// Section 7). `u8::from_str_radix` alone also takes uppercase and a leading `+`, so one signature
/// had several spellings that all verified, and absorb, comparing them as strings, kept each as one
/// more attestation of the same act.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{prov, stamped};
    use crate::Observation;

    #[test]
    fn signature_roundtrip_verifies_and_tamper_fails() {
        let identity = NodeIdentity::from_secret_bytes([42u8; 32]);
        let obs = Observation::new("signed fact".into(), prov());
        let p =
            stamped(&identity, &obs.id, 1, Hlc { wall: 7, counter: 0, node: identity.node_id() });
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

    /// A message signature (docs/pairing.md Section 4.3.2) verifies under its own context and key
    /// only, and it and an attestation's signature never pass for each other.
    #[test]
    fn a_message_signature_is_not_an_attestation_signature() {
        let identity = NodeIdentity::from_secret_bytes([42u8; 32]);
        let pubkey = identity.public_key_hex();
        let sig = identity.sign_message("join answer", b"body");
        assert!(verify_message(&pubkey, "join answer", b"body", &sig));
        assert!(!verify_message(&pubkey, "join answer", b"bodx", &sig), "the message is signed");
        assert!(!verify_message(&pubkey, "other", b"body", &sig), "the context is signed");
        let other = NodeIdentity::from_secret_bytes([43u8; 32]);
        assert!(!verify_message(&other.public_key_hex(), "join answer", b"body", &sig));
        assert!(!verify_message(&pubkey, "join answer", b"body", "zz"), "malformed is false");
        // The two encodings cannot meet: an attestation's bytes open with a small length, and a
        // message's with a tag that reads as one larger than any buffer.
        let obs = Observation::new("signed fact".into(), prov());
        let p =
            stamped(&identity, &obs.id, 1, Hlc { wall: 7, counter: 0, node: identity.node_id() });
        let meta = p.sync.clone().expect("stamped");
        let att = attestation_signing_bytes(&obs.id, &p, &meta);
        let msg = message_signing_bytes("join answer", b"body");
        let len = |b: &[u8]| u64::from_le_bytes(b[..8].try_into().unwrap());
        assert!(len(&att) < 1 << 20 && len(&msg) > 1 << 62);
        assert!(!verify_message(&pubkey, "", &att, &meta.signature));
        // Known answers (compatibility.md Section 4): the bytes and a signature over them, pinned.
        assert_eq!(
            hex_encode(&msg),
            "7375707261676e6f736973206d657373616765006a6f696e20616e7377657200626f6479"
        );
        assert_eq!(sig, "9a7b8c2b3b39c990b333f8183b1c86dcc833d9da5c204f34761dd222439b653aaf2447d4f2f9a403b4e3f847f95c28712b0a73a79454a1abed72d7e0f7e1660a");
    }

    /// sync-correctness.md Section 7 (D5): a signature has one spelling. The uppercase and
    /// `+`-prefixed forms of the same bytes used to verify too, and absorb kept each as another
    /// attestation of one act.
    #[test]
    fn a_signature_verifies_in_one_spelling_only() {
        let identity = NodeIdentity::from_secret_bytes([42u8; 32]);
        let obs = Observation::new("signed fact".into(), prov());
        let p =
            stamped(&identity, &obs.id, 1, Hlc { wall: 7, counter: 0, node: identity.node_id() });
        let meta = p.sync.clone().expect("stamped");
        let key = identity.public_key_hex();
        assert!(verify_attestation(&key, &obs.id, &p, &meta));
        let upper = SyncMeta { signature: meta.signature.to_uppercase(), ..meta.clone() };
        assert!(!verify_attestation(&key, &obs.id, &p, &upper), "uppercase is another spelling");
        // `+a` reads as the byte `0a`, so a byte below 0x10 had a second spelling.
        let i = (0..meta.signature.len())
            .step_by(2)
            .find(|&i| meta.signature.as_bytes()[i] == b'0')
            .expect("this signature has a byte below 0x10");
        let mut respelled = meta.signature.clone();
        respelled.replace_range(i..=i, "+");
        let plus = SyncMeta { signature: respelled, ..meta.clone() };
        assert!(!verify_attestation(&key, &obs.id, &p, &plus), "a leading + is another spelling");
        assert!(!verify_attestation(&key.to_uppercase(), &obs.id, &p, &meta));
        assert!(is_canonical_public_key_hex(&key));
        assert!(!is_canonical_public_key_hex(&key.to_uppercase()));
        assert!(!is_canonical_public_key_hex(&key[..62]));
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
