//! The join's wire (docs/pairing.md Section 4.3.2): the code a hub gives, the three words both
//! screens show, the commitment that keeps those words from being chosen, the seal on the
//! credential and the signature on the answer.
//!
//! Both sides compute every one of these, so all of it is fixed by the code's `sgjoin1` prefix: a
//! change to any of it is a new prefix, which a release that does not know it refuses by name.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// What every code starts with, and the family a later version would share.
pub const PREFIX: &str = "sgjoin1-";
const FAMILY: &str = "sgjoin";

const CHECKSUM_CONTEXT: &str = "supragnosis sgjoin1 2026-10-22 code checksum";
const WORDS_CONTEXT: &str = "supragnosis sgjoin1 2026-10-22 words";
const COMMITMENT_CONTEXT: &str = "supragnosis sgjoin1 2026-10-22 commitment";
const SEAL_CONTEXT: &str = "supragnosis sgjoin1 2026-10-22 seal";
const SECRET_HASH_CONTEXT: &str = "supragnosis sgjoin1 2026-10-22 secret hash";
/// The context the hub's node key signs a join's answer under; no attestation is signed under it.
pub const ANSWER_CONTEXT: &str = "sgjoin1 answer";

/// How the joining machine checks the hub's certificate (Section 3a, D13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CertCheck {
    /// Exactly the certificate whose SHA-256 this is, from any address.
    Fingerprint([u8; 32]),
    /// The built-in root bundle and the address's DNS name.
    ByName,
}

/// What a code carries (Section 4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// The hub's node id: the answer must be signed by the key it derives from.
    pub hub: String,
    /// Seconds since the epoch.
    pub expires: u64,
    pub secret: [u8; 32],
    pub check: CertCheck,
    /// `host:port`, each reached over HTTPS, in the order to try them.
    pub addresses: Vec<String>,
}

/// Why a pasted code is not one to use, each said before any connection is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    NotACode,
    Newer,
    Damaged,
}

impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CodeError::NotACode => "This is not a join code: a code starts with \"sgjoin1-\".",
            CodeError::Newer => {
                "This code was made by a newer release of supragnosis. Update this machine, then \
                 use the code."
            }
            CodeError::Damaged => "This code is incomplete or changed. Copy it again.",
        })
    }
}

impl std::error::Error for CodeError {}

impl Code {
    pub fn encode(&self) -> Result<String> {
        let hub = crate::pair::hex_bytes(&self.hub).context("the hub's node id is not hex")?;
        anyhow::ensure!(hub.len() == 16, "the hub's node id is not 32 hex digits");
        anyhow::ensure!(
            !self.addresses.is_empty() && self.addresses.len() <= 16,
            "a code names one to sixteen addresses"
        );
        let mut body = Vec::with_capacity(160);
        body.extend_from_slice(&hub);
        body.extend_from_slice(&self.expires.to_be_bytes());
        body.extend_from_slice(&self.secret);
        match &self.check {
            CertCheck::Fingerprint(f) => {
                body.push(0);
                body.extend_from_slice(f);
            }
            CertCheck::ByName => body.push(1),
        }
        body.push(self.addresses.len() as u8);
        for a in &self.addresses {
            anyhow::ensure!(!a.is_empty() && a.len() <= 255, "an address is 1 to 255 bytes: {a:?}");
            body.push(a.len() as u8);
            body.extend_from_slice(a.as_bytes());
        }
        let sum = blake3::derive_key(CHECKSUM_CONTEXT, &body);
        body.extend_from_slice(&sum[..4]);
        Ok(format!("{PREFIX}{}", base32(&body)))
    }

    /// Reads a pasted code. Whitespace anywhere in it - a line break from a narrow window - is
    /// ignored; anything else that is wrong is said as one of [`CodeError`]'s three.
    pub fn decode(text: &str) -> Result<Code, CodeError> {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = text.to_ascii_lowercase();
        let Some(rest) = lower.strip_prefix(PREFIX) else {
            return Err(if lower.starts_with(FAMILY) {
                CodeError::Newer
            } else {
                CodeError::NotACode
            });
        };
        let raw = unbase32(rest).ok_or(CodeError::Damaged)?;
        if raw.len() < 4 {
            return Err(CodeError::Damaged);
        }
        let (body, sum) = raw.split_at(raw.len() - 4);
        if blake3::derive_key(CHECKSUM_CONTEXT, body)[..4] != *sum {
            return Err(CodeError::Damaged);
        }
        let mut r = Reader(body);
        let hub = hex(r.take(16)?);
        let expires = u64::from_be_bytes(r.take(8)?.try_into().map_err(|_| CodeError::Damaged)?);
        let secret: [u8; 32] = r.take(32)?.try_into().map_err(|_| CodeError::Damaged)?;
        let check = match r.take(1)?[0] {
            0 => CertCheck::Fingerprint(r.take(32)?.try_into().map_err(|_| CodeError::Damaged)?),
            1 => CertCheck::ByName,
            _ => return Err(CodeError::Damaged),
        };
        let count = r.take(1)?[0] as usize;
        let mut addresses = Vec::with_capacity(count);
        for _ in 0..count {
            let len = r.take(1)?[0] as usize;
            let a = std::str::from_utf8(r.take(len)?).map_err(|_| CodeError::Damaged)?;
            addresses.push(a.to_string());
        }
        if !r.0.is_empty() || addresses.is_empty() {
            return Err(CodeError::Damaged);
        }
        Ok(Code { hub, expires, secret, check, addresses })
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], CodeError> {
        if self.0.len() < n {
            return Err(CodeError::Damaged);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }
}

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// RFC 4648 base32, lowercase, without padding.
fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = BASE32.iter().position(|&a| a == c)? as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    // The bits left over are padding, and an encoder writes them as zero.
    (bits < 5 && buf & ((1 << bits) - 1) == 0).then_some(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2)
        || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn hex32(s: &str) -> Option<[u8; 32]> {
    hex_bytes(s)?.try_into().ok()
}

/// 32 bytes from the system's entropy.
pub fn random32() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
    Ok(b)
}

/// The list the three words come from. Fixed for `sgjoin1`: both machines must read the same
/// word for the same byte, whatever release each runs.
const WORDS: [&str; 256] = [
    "almond", "anchor", "anvil", "apple", "apricot", "arrow", "badge", "bagel", "banana", "barn",
    "basket", "beach", "bean", "bear", "beaver", "bell", "bench", "berry", "biscuit", "bison",
    "blanket", "bottle", "bread", "bridge", "bucket", "butter", "button", "cabbage", "cabin",
    "cable", "cactus", "camel", "camera", "candle", "canoe", "canyon", "carpet", "carrot",
    "cashew", "castle", "cave", "celery", "chair", "cheese", "cherry", "chess", "chili", "cliff",
    "clock", "cloud", "cocoa", "coconut", "comet", "compass", "cookie", "coral", "corn", "cottage",
    "crane", "crayon", "cream", "creek", "crow", "crown", "cup", "curry", "daisy", "deer",
    "desert", "desk", "dolphin", "donkey", "donut", "drum", "dune", "eagle", "ember", "engine",
    "falcon", "farm", "fence", "fern", "ferret", "finch", "flag", "flute", "forest", "fork", "fox",
    "frost", "galaxy", "garden", "garlic", "gecko", "ginger", "glacier", "goat", "golf", "goose",
    "grape", "guitar", "hammer", "hamster", "harbor", "helmet", "heron", "hill", "hockey", "honey",
    "horse", "island", "ivy", "jungle", "kettle", "key", "kite", "koala", "ladder", "lagoon",
    "lake", "lamp", "lantern", "leaf", "lemon", "lemur", "lettuce", "library", "lily", "lion",
    "llama", "lobster", "magnet", "mango", "maple", "market", "marsh", "meadow", "melon", "mill",
    "mint", "mirror", "moon", "moose", "moss", "mouse", "muffin", "museum", "needle", "noodle",
    "notebook", "ocean", "olive", "onion", "orange", "orchid", "otter", "oven", "owl", "paddle",
    "panda", "parrot", "pasta", "peach", "peanut", "pear", "pebble", "pelican", "pencil",
    "penguin", "pepper", "piano", "pickle", "pillow", "pizza", "planet", "plum", "pocket", "pond",
    "pony", "potato", "prairie", "pretzel", "puffin", "pumpkin", "rabbit", "radish", "rain",
    "raisin", "raven", "reef", "rice", "river", "robin", "rocket", "rope", "rose", "saddle",
    "salad", "salmon", "sand", "scarf", "seal", "shark", "sheep", "shovel", "sloth", "snail",
    "snow", "sock", "soup", "sparrow", "spoon", "squid", "stamp", "star", "station", "stone",
    "storm", "sugar", "sun", "swan", "table", "taco", "tea", "tennis", "thunder", "ticket",
    "tiger", "toad", "toast", "tomato", "tower", "trout", "trumpet", "tulip", "tunnel", "turtle",
    "umbrella", "valley", "village", "violin", "volcano", "waffle", "wagon", "wallet", "walnut",
    "walrus", "whale", "whistle", "willow", "wind", "window", "wolf", "yoga", "zebra", "zipper",
];

/// The three words both screens show for one request (Section 4.3).
pub fn words(
    secret: &[u8; 32],
    node_key: &[u8; 32],
    onetime_key: &[u8; 32],
    joiner_nonce: &[u8; 32],
    hub_nonce: &[u8; 32],
) -> [&'static str; 3] {
    let mut m = Vec::with_capacity(160);
    for part in [secret, node_key, onetime_key, joiner_nonce, hub_nonce] {
        m.extend_from_slice(part);
    }
    let k = blake3::derive_key(WORDS_CONTEXT, &m);
    [WORDS[k[0] as usize], WORDS[k[1] as usize], WORDS[k[2] as usize]]
}

/// The joining machine's commitment to its nonce, bound to the keys it asks with.
pub fn commitment(node_key: &[u8; 32], onetime_key: &[u8; 32], nonce: &[u8; 32]) -> [u8; 32] {
    let mut m = Vec::with_capacity(96);
    for part in [node_key, onetime_key, nonce] {
        m.extend_from_slice(part);
    }
    blake3::derive_key(COMMITMENT_CONTEXT, &m)
}

/// A credential sealed to one X25519 key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    /// The hub's key for this seal alone, hex.
    pub ephemeral: String,
    pub ciphertext: String,
}

fn seal_key(shared: &[u8; 32], ephemeral: &[u8; 32], to: &[u8; 32]) -> Result<[u8; 32]> {
    // An all-zero shared secret means the other key was a point of small order: nothing secret
    // came out of the exchange, so nothing is sealed or opened with it.
    anyhow::ensure!(shared.iter().any(|b| *b != 0), "the one-time key is not a usable X25519 key");
    let mut m = Vec::with_capacity(96);
    for part in [shared, ephemeral, to] {
        m.extend_from_slice(part);
    }
    Ok(blake3::derive_key(SEAL_CONTEXT, &m))
}

fn aead(key: &[u8; 32]) -> Result<ring::aead::LessSafeKey> {
    let unbound = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, key)
        .map_err(|_| anyhow::anyhow!("the seal's key was refused"))?;
    Ok(ring::aead::LessSafeKey::new(unbound))
}

/// Seals `plaintext` to the X25519 public key `to`, with a key made for this seal alone - so the
/// all-zero nonce is never used twice under one key.
pub fn seal(to: &[u8; 32], plaintext: &[u8]) -> Result<Sealed> {
    use curve25519_dalek::MontgomeryPoint;
    let eph_secret = random32()?;
    let ephemeral = MontgomeryPoint::mul_base_clamped(eph_secret).to_bytes();
    let shared = MontgomeryPoint(*to).mul_clamped(eph_secret).to_bytes();
    let key = seal_key(&shared, &ephemeral, to)?;
    let mut buf = plaintext.to_vec();
    aead(&key)?
        .seal_in_place_append_tag(
            ring::aead::Nonce::assume_unique_for_key([0; 12]),
            ring::aead::Aad::empty(),
            &mut buf,
        )
        .map_err(|_| anyhow::anyhow!("sealing failed"))?;
    Ok(Sealed { ephemeral: hex(&ephemeral), ciphertext: hex(&buf) })
}

/// A one-time X25519 key: the joining machine's, made for one join.
pub struct OneTime {
    secret: [u8; 32],
}

impl OneTime {
    #[cfg(test)]
    pub fn new() -> Result<OneTime> {
        Ok(OneTime { secret: random32()? })
    }

    pub fn from_secret(secret: [u8; 32]) -> OneTime {
        OneTime { secret }
    }

    #[cfg(test)]
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    pub fn public(&self) -> [u8; 32] {
        curve25519_dalek::MontgomeryPoint::mul_base_clamped(self.secret).to_bytes()
    }

    /// Opens what was sealed to this key, or says it cannot.
    pub fn open(&self, sealed: &Sealed) -> Result<Vec<u8>> {
        let ephemeral = hex32(&sealed.ephemeral).context("the seal's key is not 32 hex bytes")?;
        let shared =
            curve25519_dalek::MontgomeryPoint(ephemeral).mul_clamped(self.secret).to_bytes();
        let key = seal_key(&shared, &ephemeral, &self.public())?;
        let mut buf = hex_bytes(&sealed.ciphertext).context("the sealed credential is not hex")?;
        let plain = aead(&key)?
            .open_in_place(
                ring::aead::Nonce::assume_unique_for_key([0; 12]),
                ring::aead::Aad::empty(),
                &mut buf,
            )
            .map_err(|_| anyhow::anyhow!("the credential was not sealed to this machine's key"))?;
        Ok(plain.to_vec())
    }
}

/// A key the hub vouches for (Section 4.3.1), and the last sequence it stored from that origin in
/// each invited workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vouched {
    pub node_id: String,
    pub public_key: String,
    pub bounds: BTreeMap<String, u64>,
}

/// What the hub signs on Allow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerBody {
    pub hub_key: String,
    pub hub_name: String,
    /// The exchange this answers: the invitation (its secret's hash), the joining node, its
    /// one-time key and both nonces - so it verifies for no other request and no other invitation.
    pub invitation: String,
    pub joining_node: String,
    pub onetime_key: String,
    pub joiner_nonce: String,
    pub hub_nonce: String,
    pub workspaces: Vec<String>,
    /// Whether the hub serves principals: the joining machine's serve consent rests on this word,
    /// not on the unsigned preview.
    pub serves_principals: bool,
    pub vouched: Vec<Vouched>,
    /// The hub's certificate in PEM, when the code pins it.
    pub certificate: Option<String>,
    pub credential: Sealed,
}

/// An answer as it travels: the body's exact bytes, and the hub's signature over them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedAnswer {
    pub body: String,
    pub signature: String,
}

/// Signs an answer with the hub's node key; \`sign\` is [\`supragnosis_core::NodeIdentity::sign_message\`]
/// or the sync node's, which holds the same key.
pub fn sign_answer(
    sign: impl FnOnce(&str, &[u8]) -> String,
    body: &AnswerBody,
) -> Result<SignedAnswer> {
    let body = serde_json::to_string(body)?;
    let signature = sign(ANSWER_CONTEXT, body.as_bytes());
    Ok(SignedAnswer { body, signature })
}

/// The invitation's id as an answer names it: the hash the hub keeps of its secret.
pub fn secret_hash(secret: &[u8; 32]) -> String {
    hex(&secret_hash_bytes(secret))
}

pub fn secret_hash_bytes(secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(SECRET_HASH_CONTEXT, secret)
}

/// Whether an X25519 key can be sealed to: not one of the points of small order, which give every
/// key the same all-zero secret.
pub fn usable_onetime_key(key: &[u8; 32]) -> bool {
    random32().is_ok_and(|probe| {
        curve25519_dalek::MontgomeryPoint(*key).mul_clamped(probe).to_bytes() != [0; 32]
    })
}

/// What the joining machine expects an answer to name.
pub struct Expected<'a> {
    pub code: &'a Code,
    pub joining_node: &'a str,
    pub onetime_key: &'a [u8; 32],
    pub joiner_nonce: &'a [u8; 32],
    pub hub_nonce: &'a [u8; 32],
}

/// Checks an answer the way the joining machine must before acting on it (PA14): signed by the
/// key the code's node id names, for this request, with a pinned certificate that is the one the
/// code names. Returns the body; opening the credential is the one-time key's.
pub fn verify_answer(expected: &Expected<'_>, signed: &SignedAnswer) -> Result<AnswerBody> {
    let body: AnswerBody =
        serde_json::from_str(&signed.body).context("the hub's answer does not parse")?;
    let key = hex32(&body.hub_key).context("the hub's key is not 32 hex bytes")?;
    anyhow::ensure!(
        supragnosis_core::node_id_from_public_key(&key) == expected.code.hub,
        "the answer is not from the hub the code names"
    );
    anyhow::ensure!(
        supragnosis_core::verify_message(
            &body.hub_key,
            ANSWER_CONTEXT,
            signed.body.as_bytes(),
            &signed.signature
        ),
        "the answer's signature does not verify"
    );
    anyhow::ensure!(
        body.invitation == secret_hash(&expected.code.secret)
            && body.joining_node == expected.joining_node
            && body.onetime_key == hex(expected.onetime_key)
            && body.joiner_nonce == hex(expected.joiner_nonce)
            && body.hub_nonce == hex(expected.hub_nonce),
        "the answer is for another request"
    );
    if let CertCheck::Fingerprint(pin) = &expected.code.check {
        let pem = body.certificate.as_deref().context("the answer carries no certificate")?;
        anyhow::ensure!(
            &certificate_fingerprint(pem)? == pin,
            "the answer's certificate is not the one the code names"
        );
    }
    Ok(body)
}

/// The SHA-256 of a PEM certificate's DER, as the code pins it.
pub fn certificate_fingerprint(pem: &str) -> Result<[u8; 32]> {
    let der = supragnosis_sync::http::certificate_der(pem.as_bytes())?;
    Ok(sha256(&der))
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let d = ring::digest::digest(&ring::digest::SHA256, bytes);
    d.as_ref().try_into().expect("SHA-256 is 32 bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> Code {
        Code {
            hub: "0123456789abcdef0123456789abcdef".into(),
            expires: 1_792_000_000,
            secret: [7; 32],
            check: CertCheck::Fingerprint([9; 32]),
            addresses: vec!["mac-mini.local:7420".into(), "192.168.1.5:7420".into()],
        }
    }

    /// A code survives the trip, line breaks included, and one cut short, changed, from another
    /// version or not a code at all is each said as what it is.
    #[test]
    fn a_code_is_read_back_or_said_to_be_wrong() {
        let c = code();
        let text = c.encode().unwrap();
        assert!(text.starts_with(PREFIX), "{text}");
        assert_eq!(Code::decode(&text).unwrap(), c);
        let wrapped = format!("  {}\n{}  ", &text[..40], &text[40..]);
        assert_eq!(Code::decode(&wrapped).unwrap(), c, "whitespace is ignored");
        assert_eq!(Code::decode(&text.to_uppercase()).unwrap(), c, "case is ignored");
        let by_name = Code { check: CertCheck::ByName, ..c.clone() };
        assert_eq!(Code::decode(&by_name.encode().unwrap()).unwrap(), by_name);
        assert_eq!(Code::decode(&text[..text.len() - 3]), Err(CodeError::Damaged));
        let mut changed = text.clone().into_bytes();
        let i = PREFIX.len() + 20;
        changed[i] = if changed[i] == b'a' { b'b' } else { b'a' };
        assert_eq!(Code::decode(std::str::from_utf8(&changed).unwrap()), Err(CodeError::Damaged));
        assert_eq!(Code::decode(&text.replace("sgjoin1-", "sgjoin2-")), Err(CodeError::Newer));
        assert_eq!(Code::decode("hello"), Err(CodeError::NotACode));
        assert_eq!(Code::decode("sgjoin1-!!"), Err(CodeError::Damaged));
    }

    /// Known answers (Section 4.3.2): both machines of a join may run different releases, so the
    /// code's bytes, the commitment and the words are pinned for fixed inputs. A change here is a new
    /// prefix, never an edited answer.
    #[test]
    fn the_wire_holds_its_known_answers() {
        assert_eq!(code().encode().unwrap(), KNOWN_CODE);
        assert_eq!(hex(&commitment(&[2; 32], &[3; 32], &[4; 32])), KNOWN_COMMITMENT);
        assert_eq!(words(&[1; 32], &[2; 32], &[3; 32], &[4; 32], &[5; 32]), KNOWN_WORDS);
        assert_eq!(secret_hash(&[7; 32]), KNOWN_SECRET_HASH);
    }

    const KNOWN_CODE: &str = "sgjoin1-aerukz4jvpg66ajdivtytk6n54aaaaaanlh4aaaha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4aascijbeeqscijbeeqscijbeeqscijbeeqscijbeeqscijbeeqsciccnwwcyznnvuw42jonrxwgylmhi3timrqcaytsmroge3dqlrrfy2tunzugiyde6ibh4";
    const KNOWN_COMMITMENT: &str =
        "807134898a8ac279d11caed76d01f449b53b6b424c70a40c8fc93ef4c71681f1";
    const KNOWN_WORDS: [&str; 3] = ["oven", "rose", "whistle"];
    const KNOWN_SECRET_HASH: &str =
        "23edf4a7b711baea0aef0aa1d0ea63e3a4ec45c2429983ed095832d7f2e815ac";

    /// The words are the same for the same request on both sides, and change with any of its parts.
    #[test]
    fn the_words_depend_on_every_part_of_the_request() {
        let w = words(&[1; 32], &[2; 32], &[3; 32], &[4; 32], &[5; 32]);
        assert_eq!(w, words(&[1; 32], &[2; 32], &[3; 32], &[4; 32], &[5; 32]));
        assert_ne!(w, words(&[9; 32], &[2; 32], &[3; 32], &[4; 32], &[5; 32]), "secret");
        assert_ne!(w, words(&[1; 32], &[9; 32], &[3; 32], &[4; 32], &[5; 32]), "node key");
        assert_ne!(w, words(&[1; 32], &[2; 32], &[6; 32], &[4; 32], &[5; 32]), "one-time key");
        assert_ne!(w, words(&[1; 32], &[2; 32], &[3; 32], &[9; 32], &[5; 32]), "its nonce");
        assert_ne!(w, words(&[1; 32], &[2; 32], &[3; 32], &[4; 32], &[6; 32]), "hub nonce");
        let mut seen = std::collections::BTreeSet::new();
        assert!(WORDS.iter().all(|w| seen.insert(*w)), "every word once");
        assert!(WORDS.iter().all(|w| w.bytes().all(|b| b.is_ascii_lowercase())));
        assert_ne!(
            commitment(&[2; 32], &[3; 32], &[4; 32]),
            commitment(&[2; 32], &[6; 32], &[4; 32])
        );
    }

    /// What is sealed to a one-time key opens with that key alone.
    #[test]
    fn a_seal_opens_only_with_its_key() {
        let mine = OneTime::new().unwrap();
        let sealed = seal(&mine.public(), b"credential").unwrap();
        assert_eq!(mine.open(&sealed).unwrap(), b"credential");
        assert!(OneTime::new().unwrap().open(&sealed).is_err(), "another key");
        let mut bent = sealed.clone();
        bent.ciphertext
            .replace_range(0..2, if &bent.ciphertext[0..2] == "00" { "01" } else { "00" });
        assert!(mine.open(&bent).is_err(), "a changed seal");
        assert!(seal(&[0; 32], b"x").is_err(), "a small-order key seals nothing");
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(!usable_onetime_key(&[0; 32]) && !usable_onetime_key(&one), "small order");
        assert!(usable_onetime_key(&mine.public()));
        let again = OneTime::from_secret(*mine.secret());
        assert_eq!(again.open(&sealed).unwrap(), b"credential", "the kept key opens it");
    }

    /// An answer is acted on only when the code's hub signed it, for this request.
    #[test]
    fn an_answer_verifies_only_from_the_codes_hub_for_this_request() {
        let hub = supragnosis_core::NodeIdentity::from_secret_bytes([11; 32]);
        let onetime = OneTime::new().unwrap();
        let pem = rcgen::generate_simple_self_signed(vec!["hub.local".into()]).unwrap().cert.pem();
        let c = Code {
            hub: hub.node_id(),
            check: CertCheck::Fingerprint(certificate_fingerprint(&pem).unwrap()),
            ..code()
        };
        let body = AnswerBody {
            hub_key: hub.public_key_hex(),
            hub_name: "Mac mini".into(),
            invitation: secret_hash(&c.secret),
            joining_node: "joiner".into(),
            onetime_key: hex(&onetime.public()),
            joiner_nonce: hex(&[4; 32]),
            hub_nonce: hex(&[5; 32]),
            workspaces: vec!["acme".into()],
            serves_principals: false,
            vouched: vec![],
            certificate: Some(pem),
            credential: seal(&onetime.public(), b"cred").unwrap(),
        };
        let pk = onetime.public();
        let expect = Expected {
            code: &c,
            joining_node: "joiner",
            onetime_key: &pk,
            joiner_nonce: &[4; 32],
            hub_nonce: &[5; 32],
        };
        fn by(id: &supragnosis_core::NodeIdentity) -> impl FnOnce(&str, &[u8]) -> String + '_ {
            move |ctx, m| id.sign_message(ctx, m)
        }
        let signed = sign_answer(by(&hub), &body).unwrap();
        assert_eq!(verify_answer(&expect, &signed).unwrap(), body);
        let forger = supragnosis_core::NodeIdentity::from_secret_bytes([12; 32]);
        let forged = sign_answer(
            by(&forger),
            &AnswerBody { hub_key: forger.public_key_hex(), ..body.clone() },
        )
        .unwrap();
        assert!(verify_answer(&expect, &forged).is_err(), "another key, even self-consistent");
        let mut tampered = signed.clone();
        tampered.body = tampered.body.replace("acme", "acmf");
        assert!(verify_answer(&expect, &tampered).is_err(), "a changed body");
        for other in [
            Expected { hub_nonce: &[6; 32], ..expect },
            Expected { joiner_nonce: &[6; 32], ..expect },
            Expected { joining_node: "someone", ..expect },
        ] {
            assert!(verify_answer(&other, &signed).is_err(), "another request");
        }
        let elsewhere = Code { secret: [8; 32], ..c.clone() };
        assert!(
            verify_answer(&Expected { code: &elsewhere, ..expect }, &signed).is_err(),
            "another invitation"
        );
        let swapped = AnswerBody {
            certificate: Some(
                rcgen::generate_simple_self_signed(vec!["x".into()]).unwrap().cert.pem(),
            ),
            ..body
        };
        assert!(
            verify_answer(&expect, &sign_answer(by(&hub), &swapped).unwrap()).is_err(),
            "another certificate"
        );
    }
}
