//! The join's wire (docs/pairing.md Section 4.3.2): the code a hub gives, the three words both
//! screens show and the request's proof.
//!
//! Both sides compute every one of these, so all of it is fixed by the code's `sgjoin2` prefix: a
//! change to any of it is a new prefix, which a release that does not know it refuses by name.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// What every code starts with: the family, this version of the wire, a dash.
pub const PREFIX: &str = "sgjoin2-";
const FAMILY: &str = "sgjoin";
const VERSION: u32 = 2;

const CHECKSUM_CONTEXT: &str = "supragnosis sgjoin2 2026-10-23 code checksum";
const WORDS_CONTEXT: &str = "supragnosis sgjoin2 2026-10-23 words";
const SECRET_HASH_CONTEXT: &str = "supragnosis sgjoin2 2026-10-23 secret hash";
/// The context the joining machine's node key signs its request under; no attestation is signed
/// under it.
pub const REQUEST_CONTEXT: &str = "sgjoin2 request";

/// What a code carries (Section 4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    /// The hub's node id: the answer's hub key must derive it.
    pub hub: String,
    /// Seconds since the epoch.
    pub expires: u64,
    pub secret: [u8; 32],
    /// The SHA-256 of the hub's certificate: the joining machine accepts that one alone.
    pub fingerprint: [u8; 32],
    /// `host:port`, each reached over HTTPS, in the order to try them.
    pub addresses: Vec<String>,
}

/// Why a pasted code is not one to use, each said before any connection is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    NotACode,
    Newer,
    Older,
    Damaged,
}

impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CodeError::NotACode => "This is not a join code: a code starts with \"sgjoin2-\".",
            CodeError::Newer => {
                "This code was made by a newer release of supragnosis. Update this machine, then \
                 use the code."
            }
            CodeError::Older => {
                "This code was made by an older release of supragnosis. Update the machine that \
                 made it, then make a new code."
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
        body.extend_from_slice(&self.fingerprint);
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
    /// ignored; anything else that is wrong is said as one of [`CodeError`]'s four.
    pub fn decode(text: &str) -> Result<Code, CodeError> {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = text.to_ascii_lowercase();
        let Some(rest) = lower.strip_prefix(PREFIX) else {
            let version = lower
                .strip_prefix(FAMILY)
                .and_then(|v| v.split_once('-'))
                .and_then(|(v, _)| v.parse::<u32>().ok());
            return Err(match version {
                Some(v) if v < VERSION => CodeError::Older,
                Some(_) => CodeError::Newer,
                None => CodeError::NotACode,
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
        let fingerprint: [u8; 32] = r.take(32)?.try_into().map_err(|_| CodeError::Damaged)?;
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
        Ok(Code { hub, expires, secret, fingerprint, addresses })
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

/// The list the three words come from. Fixed for `sgjoin2`: both machines must read the same
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
pub fn words(secret: &[u8; 32], node_key: &[u8; 32]) -> [&'static str; 3] {
    let mut m = Vec::with_capacity(64);
    m.extend_from_slice(secret);
    m.extend_from_slice(node_key);
    let k = blake3::derive_key(WORDS_CONTEXT, &m);
    [WORDS[k[0] as usize], WORDS[k[1] as usize], WORDS[k[2] as usize]]
}

/// What a request's proof signs: the invitation (its secret's hash), the node key and the name.
/// The name comes last, so the bytes read back one way only.
pub fn request_bytes(invitation: &[u8; 32], node_key: &[u8; 32], name: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(64 + name.len());
    m.extend_from_slice(invitation);
    m.extend_from_slice(node_key);
    m.extend_from_slice(name.as_bytes());
    m
}

/// Whether `proof` is `node_key`'s signature over this request.
pub fn proof_verifies(invitation: &[u8; 32], node_key: &[u8; 32], name: &str, proof: &str) -> bool {
    supragnosis_core::verify_message(
        &hex(node_key),
        REQUEST_CONTEXT,
        &request_bytes(invitation, node_key, name),
        proof,
    )
}

/// A key the hub vouches for (Section 4.3.1), and the last sequence it stored from that origin in
/// each invited workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vouched {
    pub node_id: String,
    pub public_key: String,
    pub bounds: BTreeMap<String, u64>,
}

/// The hub's answer on Allow. It travels over the pinned connection, which is what keeps it whole
/// and the credential in it unread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerBody {
    pub hub_key: String,
    pub hub_name: String,
    pub workspaces: Vec<String>,
    /// Whether the hub serves principals: always false since 0.11.0 removed them (remote-server.md
    /// Section 13). Kept on the wire because a 0.10 joiner requires it, and declared because this
    /// struct denies unknown fields and a 0.10 hub sends it; read by no one here.
    pub serves_principals: bool,
    pub vouched: Vec<Vouched>,
    /// The hub's certificate in PEM: the one the code pins.
    pub certificate: String,
    pub credential: String,
}

/// Checks an answer the way the joining machine must before acting on it (PA14): its hub key
/// derives the node id the code names, and its certificate is the one the code pins.
pub fn check_answer(code: &Code, body: &AnswerBody) -> Result<()> {
    let key = hex32(&body.hub_key).context("the hub's key is not 32 hex bytes")?;
    anyhow::ensure!(
        supragnosis_core::node_id_from_public_key(&key) == code.hub,
        "the answer is not from the hub the code names"
    );
    anyhow::ensure!(
        certificate_fingerprint(&body.certificate)? == code.fingerprint,
        "the answer's certificate is not the one the code names"
    );
    Ok(())
}

/// The invitation's id: the hash the hub keeps of its secret.
pub fn secret_hash(secret: &[u8; 32]) -> String {
    hex(&secret_hash_bytes(secret))
}

pub fn secret_hash_bytes(secret: &[u8; 32]) -> [u8; 32] {
    blake3::derive_key(SECRET_HASH_CONTEXT, secret)
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
            fingerprint: [9; 32],
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
        assert_eq!(Code::decode(&text[..text.len() - 3]), Err(CodeError::Damaged));
        let mut changed = text.clone().into_bytes();
        let i = PREFIX.len() + 20;
        changed[i] = if changed[i] == b'a' { b'b' } else { b'a' };
        assert_eq!(Code::decode(std::str::from_utf8(&changed).unwrap()), Err(CodeError::Damaged));
        assert_eq!(Code::decode(&text.replace("sgjoin2-", "sgjoin3-")), Err(CodeError::Newer));
        assert_eq!(Code::decode(&text.replace("sgjoin2-", "sgjoin1-")), Err(CodeError::Older));
        assert_eq!(Code::decode("hello"), Err(CodeError::NotACode));
        assert_eq!(Code::decode("sgjoin2-!!"), Err(CodeError::Damaged));
    }

    /// Known answers (Section 4.3.2): both machines of a join may run different releases, so the
    /// code's bytes, the words and the proof are pinned for fixed inputs. A change here is a new
    /// prefix, never an edited answer.
    #[test]
    fn the_wire_holds_its_known_answers() {
        assert_eq!(code().encode().unwrap(), KNOWN_CODE);
        assert_eq!(words(&[1; 32], &[2; 32]), KNOWN_WORDS);
        assert_eq!(secret_hash(&[7; 32]), KNOWN_SECRET_HASH);
        let me = supragnosis_core::NodeIdentity::from_secret_bytes([11; 32]);
        let key = hex32(&me.public_key_hex()).unwrap();
        let bytes = request_bytes(&[3; 32], &key, "mac");
        assert_eq!(hex(&bytes[64..]), "6d6163", "the name follows the two 32-byte parts");
        let proof = me.sign_message(REQUEST_CONTEXT, &bytes);
        assert_eq!(proof, KNOWN_PROOF);
    }

    const KNOWN_CODE: &str = "sgjoin2-aerukz4jvpg66ajdivtytk6n54aaaaaanlh4aaaha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4eqscijbeeqscijbeeqscijbeeqscijbeeqscijbeeqscijbeeqsaqtnvqwgllnnfxgsltmn5rwc3b2g42demaqge4telrrgy4c4mjogu5donbsgcvtod4w";
    const KNOWN_WORDS: [&str; 3] = ["daisy", "castle", "whistle"];
    const KNOWN_SECRET_HASH: &str =
        "0387fcd61edd2cccd34d0fc895d403996a3bbe00e8763ad41c74138752f1f72e";
    const KNOWN_PROOF: &str = "95d1b177082d8c50b664e8e9f8747f07d936002155a20d11060521771fbc99552c3ed04482b6f045c5ddbfffd5b8b399f0ad6a2db13d3b40e74b55734325be01";

    /// The words are the same for the same request on both sides, and change with either part.
    #[test]
    fn the_words_depend_on_the_secret_and_the_node_key() {
        let w = words(&[1; 32], &[2; 32]);
        assert_eq!(w, words(&[1; 32], &[2; 32]));
        assert_ne!(w, words(&[9; 32], &[2; 32]), "secret");
        assert_ne!(w, words(&[1; 32], &[9; 32]), "node key");
        let mut seen = std::collections::BTreeSet::new();
        assert!(WORDS.iter().all(|w| seen.insert(*w)), "every word once");
        assert!(WORDS.iter().all(|w| w.bytes().all(|b| b.is_ascii_lowercase())));
    }

    /// A proof verifies for its own invitation, key and name, and for no other.
    #[test]
    fn a_proof_verifies_only_for_its_request() {
        let me = supragnosis_core::NodeIdentity::from_secret_bytes([11; 32]);
        let other = supragnosis_core::NodeIdentity::from_secret_bytes([12; 32]);
        let key = hex32(&me.public_key_hex()).unwrap();
        let theirs = hex32(&other.public_key_hex()).unwrap();
        let proof = me.sign_message(REQUEST_CONTEXT, &request_bytes(&[3; 32], &key, "mac"));
        assert!(proof_verifies(&[3; 32], &key, "mac", &proof));
        assert!(!proof_verifies(&[4; 32], &key, "mac", &proof), "another invitation");
        assert!(!proof_verifies(&[3; 32], &key, "mad", &proof), "another name");
        assert!(!proof_verifies(&[3; 32], &theirs, "mac", &proof), "another key");
        let elsewhere = me.sign_message("sgjoin1 answer", &request_bytes(&[3; 32], &key, "mac"));
        assert!(!proof_verifies(&[3; 32], &key, "mac", &elsewhere), "another context");
    }

    /// An answer is acted on only when its hub key is the code's hub and its certificate the
    /// code's pin.
    #[test]
    fn an_answer_is_checked_against_the_code() {
        let hub = supragnosis_core::NodeIdentity::from_secret_bytes([11; 32]);
        let pem = rcgen::generate_simple_self_signed(vec!["hub.local".into()]).unwrap().cert.pem();
        let c = Code {
            hub: hub.node_id(),
            fingerprint: certificate_fingerprint(&pem).unwrap(),
            ..code()
        };
        let body = AnswerBody {
            hub_key: hub.public_key_hex(),
            hub_name: "Mac mini".into(),
            workspaces: vec!["acme".into()],
            serves_principals: false,
            vouched: vec![],
            certificate: pem,
            credential: "cred".into(),
        };
        check_answer(&c, &body).unwrap();
        let forger = supragnosis_core::NodeIdentity::from_secret_bytes([12; 32]);
        let forged = AnswerBody { hub_key: forger.public_key_hex(), ..body.clone() };
        assert!(check_answer(&c, &forged).is_err(), "another hub key");
        let swapped = AnswerBody {
            certificate: rcgen::generate_simple_self_signed(vec!["x".into()]).unwrap().cert.pem(),
            ..body
        };
        assert!(check_answer(&c, &swapped).is_err(), "another certificate");
    }
}
