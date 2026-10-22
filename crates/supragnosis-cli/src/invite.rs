//! The hub's side of pairing (docs/pairing.md Sections 4.2, 4.3 and 4.6): invitations, the join's
//! requests and answers, and the machines pairing admitted.
//!
//! Everything here is kept in two owner-only files beside `supragnosis.toml` rather than in it:
//! `hub/invites.toml`, the invitations by their secret's hash with what each request and answer
//! was, and `hub/peers.toml`, the machines pairing admitted and the keys of those it removed. Each
//! act re-reads them under one lock, so a restart of the daemon, mid-join or after Allow, loses
//! nothing a joining machine is waiting for.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use supragnosis_core::AssertionStore;
use supragnosis_sync::SyncNode;

use crate::pair::{self, hex, hex32};

/// How long a code is good for (Section 4.2).
pub const VALID_FOR: u64 = 600;
/// How long a closed invitation is kept after its expiry, so a code tried late is told what became
/// of it - used, denied, expired - rather than that the hub never made it (PA2).
pub const TOMBSTONE_FOR: u64 = 86_400;

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    Used,
    Denied,
    Cancelled,
    Ended,
    Expired,
}

impl State {
    /// What a joining machine is told about an invitation in this state.
    fn said(self) -> &'static str {
        match self {
            State::Open => "The invitation is open.",
            State::Used => "This code was already used. Make a new code on the hub.",
            State::Denied => "The hub denied this request.",
            State::Cancelled => "This invitation was cancelled on the hub. Make a new code there.",
            State::Ended => {
                "This invitation ended: a second request came with the same code. Make a new code \
                 on the hub."
            }
            State::Expired => "This code has expired. Make a new code on the hub.",
        }
    }

    fn name(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Used => "used",
            State::Denied => "denied",
            State::Cancelled => "cancelled",
            State::Ended => "ended",
            State::Expired => "expired",
        }
    }
}

/// A request as the hub keeps it: fixed by the first `join`, its nonce added when revealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub node_key: String,
    pub name: String,
    pub onetime_key: String,
    pub commitment: String,
    pub hub_nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// The three words, once the nonce is revealed - computed then, since the hub keeps no secret.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
}

impl Request {
    fn node_id(&self) -> String {
        supragnosis_core::node_id_from_public_key(&hex32(&self.node_key).unwrap_or_default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub secret_hash: String,
    pub workspaces: Vec<String>,
    pub expires: u64,
    pub state: State,
    /// Whether the code pins the certificate, so the answer carries it.
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Request>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<pair::SignedAnswer>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Invites {
    #[serde(default)]
    invite: Vec<Invite>,
}

/// A machine pairing admitted (Section 4.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joined {
    pub name: String,
    pub node_id: String,
    pub public_key: String,
    pub workspaces: Vec<String>,
    pub joined_at: u64,
}

/// A machine pairing removed: its key verifies its events up to `bounds` and admits nothing
/// (Section 4.3.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Removed {
    pub name: String,
    pub node_id: String,
    pub public_key: String,
    pub removed_at: u64,
    pub bounds: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Peers {
    #[serde(default)]
    pub joined: Vec<Joined>,
    #[serde(default)]
    pub removed: Vec<Removed>,
}

/// What `join` carries (Section 4.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub secret: String,
    pub node_key: String,
    pub name: String,
    pub onetime_key: String,
    pub commitment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

/// Where pairing keeps its records, and the configuration it edits.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~/.supragnosis/hub`
    pub hub: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    pub fn default_paths() -> Paths {
        Paths { hub: crate::fed::fed_base_dir().join("hub"), config: crate::fed::config_path() }
    }

    fn invites(&self) -> PathBuf {
        self.hub.join("invites.toml")
    }

    fn peers(&self) -> PathBuf {
        self.hub.join("peers.toml")
    }
}

/// The hub's pairing: its invitations, their requests, and the machines it admitted.
pub struct Pairing {
    paths: Paths,
    node: Arc<SyncNode>,
    store: Arc<dyn AssertionStore>,
    pub hub_name: String,
    /// Serializes every act on the two files, and holds the open invitation's code, which no file
    /// does, so `invite --show` can print it again while this process runs.
    live: Mutex<Option<(String, String)>>,
    /// Woken whenever an invitation opens or closes, so the listener follows the bind rule.
    pub changed: Arc<tokio::sync::Notify>,
}

/// What `invite` answers (Section 4.2).
#[derive(Debug, Clone, Serialize)]
pub struct Made {
    pub code: String,
    pub expires: u64,
    pub workspaces: Vec<String>,
    pub addresses: Vec<String>,
    pub check: &'static str,
    /// An open invitation this one cancelled.
    pub replaced: bool,
}

fn read_toml<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("parsing {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_toml<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    let text = toml::to_string(value).context("encoding a pairing record")?;
    crate::write_secret(path, text.as_bytes())
}

fn clean_name(s: &str) -> String {
    let n: String = s.chars().filter(|c| !c.is_control()).collect();
    let n: String = n.trim().chars().take(64).collect();
    if n.is_empty() {
        "a machine".into()
    } else {
        n
    }
}

fn digest_matches(hex_a: &str, b: &[u8; 32]) -> bool {
    hex32(hex_a).is_some_and(|a| crate::digests_equal(&a, b))
}

impl Pairing {
    pub fn new(
        paths: Paths,
        node: Arc<SyncNode>,
        store: Arc<dyn AssertionStore>,
        hub_name: String,
    ) -> Pairing {
        Pairing {
            paths,
            node,
            store,
            hub_name,
            live: Mutex::new(None),
            changed: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn config_path(&self) -> &std::path::Path {
        &self.paths.config
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Option<(String, String)>> {
        self.live.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reads the invitations, closing those past their expiry and dropping tombstones a day past
    /// it; the second value says whether an open one closed.
    fn invites(&self, now: u64) -> Result<(Invites, bool)> {
        let mut book: Invites = read_toml(&self.paths.invites())?;
        let mut closed = false;
        for i in &mut book.invite {
            if i.state == State::Open && i.expires <= now {
                i.state = State::Expired;
                closed = true;
            }
        }
        book.invite
            .retain(|i| i.state == State::Open || i.expires + TOMBSTONE_FOR > now);
        Ok((book, closed))
    }

    fn save(&self, book: &Invites) -> Result<()> {
        write_toml(&self.paths.invites(), book)
    }

    pub fn peers(&self) -> Result<Peers> {
        read_toml(&self.paths.peers())
    }

    /// How many invitations are open now - what the bind rule counts (D5).
    pub fn open_count(&self, now: u64) -> usize {
        let _g = self.guard();
        read_toml::<Invites>(&self.paths.invites())
            .map(|b| b.invite.iter().filter(|i| i.state == State::Open && i.expires > now).count())
            .unwrap_or(0)
    }

    fn admits_none(&self, fc: &crate::fed::FileConfig) -> Result<()> {
        admits_none(&self.paths, fc)
    }

    /// Opens an invitation (Section 4.2), cancelling one already open.
    pub fn invite(
        &self,
        workspaces: Vec<String>,
        addresses: Vec<String>,
        check: pair::CertCheck,
        now: u64,
    ) -> Result<Made> {
        may_invite(&self.paths)?;
        let mut live = self.guard();
        let (mut book, _) = self.invites(now)?;
        let mut replaced = false;
        for i in book.invite.iter_mut().filter(|i| i.state == State::Open) {
            i.state = State::Cancelled;
            replaced = true;
        }
        let secret = pair::random32()?;
        let hash = pair::secret_hash(&secret);
        let expires = now + VALID_FOR;
        let pinned = matches!(check, pair::CertCheck::Fingerprint(_));
        let code = pair::Code {
            hub: self.node.node_id().to_string(),
            expires,
            secret,
            check,
            addresses: addresses.clone(),
        }
        .encode()?;
        book.invite.push(Invite {
            secret_hash: hash.clone(),
            workspaces: workspaces.clone(),
            expires,
            state: State::Open,
            pinned,
            request: None,
            answer: None,
        });
        self.save(&book)?;
        *live = Some((hash, code.clone()));
        drop(live);
        self.changed.notify_one();
        Ok(Made {
            code,
            expires,
            workspaces,
            addresses,
            check: if pinned { "fingerprint" } else { "by name" },
            replaced,
        })
    }

    /// The open invitation, its code while this process holds it, and its request.
    pub fn status(&self, now: u64) -> Result<serde_json::Value> {
        let live = self.guard();
        let (book, closed) = self.invites(now)?;
        if closed {
            self.save(&book)?;
            self.changed.notify_one();
        }
        let latest = book.invite.last();
        let open = book.invite.iter().find(|i| i.state == State::Open);
        Ok(json!({
            "open": open.map(|i| json!({
                "expires": i.expires,
                "workspaces": i.workspaces,
                "code": live.as_ref().filter(|(h, _)| *h == i.secret_hash).map(|(_, c)| c),
                "request": i.request.as_ref().map(|r| json!({
                    "name": r.name,
                    "node_id": r.node_id(),
                    "words": (!r.words.is_empty()).then_some(&r.words),
                })),
            })),
            "last": latest.filter(|i| i.state != State::Open).map(|i| json!({
                "state": i.state.name(),
                "said": i.state.said(),
                "name": i.request.as_ref().map(|r| &r.name),
            })),
        }))
    }

    fn position(book: &Invites, secret: &[u8; 32]) -> Option<usize> {
        let hash = *blake3::hash(secret).as_bytes();
        book.invite.iter().position(|i| digest_matches(&i.secret_hash, &hash))
    }

    /// `preview` (Section 4.3): what the invitation would grant, changing nothing.
    pub fn preview(&self, secret: &str, now: u64) -> Result<serde_json::Value> {
        let _g = self.guard();
        let secret = hex32(secret).context("the secret is not 32 hex bytes")?;
        let (mut book, _) = self.invites(now)?;
        let serves = self.serves_principals();
        Ok(match Self::position(&book, &secret).map(|at| &mut book.invite[at]) {
            None => json!({"state": "unknown", "said": UNKNOWN}),
            Some(i) if i.state != State::Open => {
                json!({"state": i.state.name(), "said": i.state.said()})
            }
            Some(i) => json!({
                "state": "open",
                "hub_name": self.hub_name,
                "workspaces": i.workspaces,
                "serves_principals": serves,
                "expires": i.expires,
            }),
        })
    }

    fn serves_principals(&self) -> bool {
        crate::fed::load_at(&self.paths.config)
            .ok()
            .flatten()
            .and_then(|c| c.server)
            .is_some_and(|s| !s.principals.is_empty())
    }

    /// A refusal that leaves the invitation as it is: the request is not one the hub could show.
    fn refused(why: impl std::fmt::Display) -> serde_json::Value {
        json!({"state": "refused", "said": why.to_string()})
    }

    /// `join` (Section 4.3): holds the request, answers "pending" with the hub's nonce, takes the
    /// revealed nonce, and once the person has answered on the hub, says what they answered.
    pub fn join(&self, req: &JoinRequest, now: u64) -> Result<serde_json::Value> {
        let live = self.guard();
        let parse =
            |s: &str, what: &str| hex32(s).with_context(|| format!("{what} is not 32 hex bytes"));
        let secret = parse(&req.secret, "the secret")?;
        let node_key = parse(&req.node_key, "the node key")?;
        parse(&req.onetime_key, "the one-time key")?;
        parse(&req.commitment, "the commitment")?;
        let nonce = req.nonce.as_deref().map(|n| parse(n, "the nonce")).transpose()?;
        let node_id = supragnosis_core::node_id_from_public_key(&node_key);
        let (mut book, closed) = self.invites(now)?;
        let Some(at) = Self::position(&book, &secret) else {
            if closed {
                self.save(&book)?;
            }
            return Ok(json!({"state": "unknown", "said": UNKNOWN}));
        };
        let invite = &mut book.invite[at];
        let same = |r: &Request| {
            r.node_key == req.node_key
                && r.onetime_key == req.onetime_key
                && r.commitment == req.commitment
        };
        // Allowed: the answer, to the request it answers and no other.
        if invite.state == State::Used {
            if let (Some(r), Some(answer)) = (&invite.request, &invite.answer) {
                if same(r) {
                    return Ok(json!({"state": "allowed", "answer": answer}));
                }
            }
        }
        if invite.state != State::Open {
            let said = json!({"state": invite.state.name(), "said": invite.state.said()});
            if closed {
                self.save(&book)?;
                self.changed.notify_one();
            }
            return Ok(said);
        }
        // Who may join (PA8, PA9). These refusals leave the invitation open: the hub shows no
        // request it could never admit.
        if node_id == self.node.node_id() {
            return Ok(Self::refused(
                "This machine has the hub's own identity: a machine cannot join itself.",
            ));
        }
        if let Some(fc) = crate::fed::load_at(&self.paths.config)? {
            if fc
                .server
                .as_ref()
                .is_some_and(|s| s.allowlist.iter().any(|e| e.node_id == node_id))
            {
                return Ok(Self::refused("This machine is already admitted to this hub."));
            }
            if let Err(e) = self.admits_none(&fc) {
                return Ok(Self::refused(e));
            }
        }
        let answer = match &mut invite.request {
            None => {
                let hub_nonce = hex(&pair::random32()?);
                invite.request = Some(Request {
                    node_key: req.node_key.clone(),
                    name: clean_name(&req.name),
                    onetime_key: req.onetime_key.clone(),
                    commitment: req.commitment.clone(),
                    hub_nonce: hub_nonce.clone(),
                    nonce: None,
                    words: Vec::new(),
                });
                tracing::info!(node = %node_id, "a machine asks to join with an open invitation");
                json!({"state": "pending", "hub_nonce": hub_nonce})
            }
            Some(r) if !same(r) => {
                invite.state = State::Ended;
                tracing::warn!(node = %node_id, "a second request came with the same code; the invitation ended");
                json!({"state": "ended", "said": State::Ended.said()})
            }
            Some(r) => {
                match (nonce, r.nonce.clone()) {
                    (Some(n), None) => {
                        let node_key = hex32(&r.node_key).unwrap_or_default();
                        let onetime = hex32(&r.onetime_key).unwrap_or_default();
                        if hex(&pair::commitment(&node_key, &onetime, &n)) != r.commitment {
                            invite.state = State::Ended;
                            tracing::warn!(node = %node_id, "a nonce did not match its commitment; the invitation ended");
                        } else {
                            let hub_nonce = hex32(&r.hub_nonce).unwrap_or_default();
                            r.words = pair::words(&secret, &node_key, &onetime, &n, &hub_nonce)
                                .iter()
                                .map(|w| w.to_string())
                                .collect();
                            r.nonce = Some(hex(&n));
                        }
                    }
                    (Some(n), Some(kept)) if hex(&n) != kept => invite.state = State::Ended,
                    _ => {}
                }
                if invite.state == State::Ended {
                    json!({"state": "ended", "said": State::Ended.said()})
                } else {
                    json!({"state": "pending", "hub_nonce": r.hub_nonce})
                }
            }
        };
        let ended = invite.state != State::Open;
        self.save(&book)?;
        drop(live);
        if ended || closed {
            self.changed.notify_one();
        }
        Ok(answer)
    }

    /// The open invitation with a request whose words are shown, or why there is none to answer.
    fn answerable(book: &mut Invites) -> Result<&mut Invite> {
        let invite = book
            .invite
            .iter_mut()
            .find(|i| i.state == State::Open)
            .context("no invitation is open")?;
        let request = invite.request.as_ref().context("no machine has asked to join yet")?;
        anyhow::ensure!(
            !request.words.is_empty(),
            "the request has not finished asking - wait for its words"
        );
        Ok(invite)
    }

    /// Allow (Section 4.3): admits the machine that asked and keeps the signed, sealed answer.
    pub fn allow(&self, now: u64) -> Result<serde_json::Value> {
        let live = self.guard();
        let (mut book, _) = self.invites(now)?;
        let invite = Self::answerable(&mut book)?;
        let request = invite.request.clone().context("no request")?;
        let node_id = request.node_id();
        let onetime = hex32(&request.onetime_key).context("the one-time key")?;
        let credential = crate::principal::new_credential()?;
        let workspaces = invite.workspaces.clone();
        // The allowlist entry, under the configuration's lock: a hand-written entry is never
        // touched, and one already present for this node, or for any node, refuses (PA8, PA9).
        crate::fed::edit_config_at(&self.paths.config, |text| {
            let fc: crate::fed::FileConfig =
                toml::from_str(text).context("parsing supragnosis.toml")?;
            self.admits_none(&fc)?;
            let mut doc: toml_edit::DocumentMut =
                text.parse().context("parsing supragnosis.toml")?;
            let server = doc
                .get_mut("server")
                .and_then(|s| s.as_table_mut())
                .context("this hub has no [server] section")?;
            let list = server
                .entry("allowlist")
                .or_insert(toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()))
                .as_array_of_tables_mut()
                .context("[server] allowlist is not an array of tables")?;
            let mut t = toml_edit::Table::new();
            t["node_id"] = toml_edit::value(node_id.as_str());
            t["public_key_hex"] = toml_edit::value(request.node_key.as_str());
            t["bearer_hash"] = toml_edit::value(supragnosis_sync::http::bearer_hash(&credential));
            let mut arr = toml_edit::Array::new();
            workspaces.iter().for_each(|w| arr.push(w.as_str()));
            t["shared_workspaces"] = toml_edit::value(arr);
            list.push(t);
            Ok((doc.to_string(), ()))
        })?;
        let mut peers = self.peers()?;
        peers.joined.push(Joined {
            name: request.name.clone(),
            node_id: node_id.clone(),
            public_key: request.node_key.clone(),
            workspaces: workspaces.clone(),
            joined_at: now,
        });
        write_toml(&self.paths.peers(), &peers)?;
        let vouched = peers
            .removed
            .iter()
            .filter(|r| r.node_id != node_id && r.node_id != self.node.node_id())
            .map(|r| pair::Vouched {
                node_id: r.node_id.clone(),
                public_key: r.public_key.clone(),
                bounds: r
                    .bounds
                    .iter()
                    .filter(|(w, _)| workspaces.contains(w))
                    .map(|(w, s)| (w.clone(), *s))
                    .collect(),
            })
            .filter(|v| !v.bounds.is_empty())
            .collect();
        let certificate = match invite.pinned {
            true => Some(self.certificate_pem()?),
            false => None,
        };
        let body = pair::AnswerBody {
            hub_key: self.node.public_key_hex(),
            hub_name: self.hub_name.clone(),
            invitation: invite.secret_hash.clone(),
            joining_node: node_id.clone(),
            onetime_key: request.onetime_key.clone(),
            joiner_nonce: request.nonce.clone().unwrap_or_default(),
            hub_nonce: request.hub_nonce.clone(),
            workspaces: workspaces.clone(),
            serves_principals: self.serves_principals(),
            vouched,
            certificate,
            credential: pair::seal(&onetime, credential.as_bytes())?,
        };
        let answer = pair::sign_answer(|c, m| self.node.sign_message(c, m), &body)?;
        invite.state = State::Used;
        invite.answer = Some(answer);
        self.save(&book)?;
        drop(live);
        self.changed.notify_one();
        tracing::info!(node = %node_id, name = %request.name, "a machine joined this hub");
        Ok(json!({"joined": {"name": request.name, "node_id": node_id, "workspaces": workspaces}}))
    }

    fn certificate_pem(&self) -> Result<String> {
        let path = crate::fed::load_at(&self.paths.config)?
            .and_then(|c| c.server)
            .and_then(|s| s.tls_cert)
            .context("this hub has no certificate")?;
        std::fs::read_to_string(&path).with_context(|| format!("reading {path}"))
    }

    /// Deny (Section 4.3) or cancel (Section 4.2): the open invitation becomes a tombstone.
    pub fn close(&self, state: State, now: u64) -> Result<serde_json::Value> {
        let live = self.guard();
        let (mut book, _) = self.invites(now)?;
        let invite = book
            .invite
            .iter_mut()
            .find(|i| i.state == State::Open)
            .context("no invitation is open")?;
        if state == State::Denied {
            invite.request.as_ref().context("no machine has asked to join yet")?;
        }
        invite.state = state;
        self.save(&book)?;
        drop(live);
        self.changed.notify_one();
        Ok(json!({"state": state.name()}))
    }

    /// Closes the invitation `hash` if it is still open and its expiry has come.
    pub fn expire(&self, now: u64) -> Result<()> {
        let _g = self.guard();
        let (book, closed) = self.invites(now)?;
        if closed {
            self.save(&book)?;
            self.changed.notify_one();
        }
        Ok(())
    }

    /// `peer remove` (Section 4.6): deletes an entry pairing made and keeps its key verify-only, up
    /// to the last sequence this hub stored from it. An entry written by hand stays.
    pub fn remove(&self, who: &str, now: u64) -> Result<serde_json::Value> {
        let _g = self.guard();
        let mut peers = self.peers()?;
        let Some(at) = peers.joined.iter().position(|j| j.name == who || j.node_id == who) else {
            let by_hand = crate::fed::load_at(&self.paths.config)?
                .and_then(|c| c.server)
                .is_some_and(|s| s.allowlist.iter().any(|e| e.node_id == who));
            anyhow::bail!(if by_hand {
                format!(
                    "{who} was written into {} by hand, and pairing removes only what it added - \
                     edit the file to remove it",
                    self.paths.config.display()
                )
            } else {
                format!("no machine named {who} joined this hub")
            });
        };
        let joined = peers.joined.remove(at);
        let mut bounds = BTreeMap::new();
        for ws in &joined.workspaces {
            let vv = supragnosis_sync::version_vector(self.store.as_ref(), ws)
                .with_context(|| format!("reading what {ws} holds"))?;
            if let Some(seq) = vv.0.get(&(joined.node_id.clone(), ws.clone())) {
                bounds.insert(ws.clone(), *seq);
            }
        }
        crate::fed::edit_config_at(&self.paths.config, |text| {
            let mut doc: toml_edit::DocumentMut =
                text.parse().context("parsing supragnosis.toml")?;
            if let Some(list) = doc
                .get_mut("server")
                .and_then(|s| s.get_mut("allowlist"))
                .and_then(|a| a.as_array_of_tables_mut())
            {
                list.retain(|t| t.get("node_id").and_then(|v| v.as_str()) != Some(&joined.node_id));
            }
            Ok((doc.to_string(), ()))
        })?;
        peers.removed.retain(|r| r.node_id != joined.node_id);
        peers.removed.push(Removed {
            name: joined.name.clone(),
            node_id: joined.node_id.clone(),
            public_key: joined.public_key.clone(),
            removed_at: now,
            bounds,
        });
        write_toml(&self.paths.peers(), &peers)?;
        self.changed.notify_one();
        tracing::info!(node = %joined.node_id, name = %joined.name, "removed a joined machine");
        Ok(json!({
            "removed": joined.name,
            "said": "It stops syncing with this machine now. What it already received stays on it.",
        }))
    }

    /// `leave` (Section 4.6): a joined machine, by its credential, removes its own entry.
    pub fn leave(&self, bearer: &str, now: u64) -> Result<Option<serde_json::Value>> {
        let presented = *blake3::hash(bearer.as_bytes()).as_bytes();
        let entry = crate::fed::load_at(&self.paths.config)?.and_then(|c| c.server).and_then(|s| {
            s.allowlist.into_iter().find(|e| digest_matches(&e.bearer_hash, &presented))
        });
        let Some(entry) = entry else {
            return Ok(None);
        };
        if !self.peers()?.joined.iter().any(|j| j.node_id == entry.node_id) {
            return Ok(None);
        }
        self.remove(&entry.node_id, now).map(Some)
    }
}

const UNKNOWN: &str = "This hub has no invitation for this code. Make a new code on the hub.";

/// The network's pairing calls (Section 4.3): `preview` and `join` with an invitation's secret,
/// `leave` with a joined machine's credential. Nothing else of the hub answers either.
pub fn router(pairing: Arc<Pairing>) -> axum::Router {
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::Json;
    let bad =
        |e: anyhow::Error| (StatusCode::BAD_REQUEST, Json(json!({"error": format!("{e:#}")})));
    let (p1, p2, p3) = (pairing.clone(), pairing.clone(), pairing);
    axum::Router::new()
        .route(
            "/pair/preview",
            post(move |Json(body): Json<serde_json::Value>| {
                let p = p1.clone();
                async move {
                    let secret = body.get("secret").and_then(|s| s.as_str()).unwrap_or_default().to_string();
                    tokio::task::spawn_blocking(move || p.preview(&secret, now()))
                        .await
                        .map_err(|e| bad(e.into()))?
                        .map(Json)
                        .map_err(bad)
                }
            }),
        )
        .route(
            "/pair/join",
            post(move |Json(req): Json<JoinRequest>| {
                let p = p2.clone();
                async move {
                    tokio::task::spawn_blocking(move || p.join(&req, now()))
                        .await
                        .map_err(|e| bad(e.into()))?
                        .map(Json)
                        .map_err(bad)
                }
            }),
        )
        .route(
            "/pair/leave",
            post(move |headers: axum::http::HeaderMap| {
                let p = p3.clone();
                async move {
                    let Some(bearer) = crate::bearer_of(&headers).map(str::to_string) else {
                        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "no credential"}))));
                    };
                    match tokio::task::spawn_blocking(move || p.leave(&bearer, now())).await {
                        Ok(Ok(Some(v))) => Ok(Json(v)),
                        Ok(Ok(None)) => Err((
                            StatusCode::UNAUTHORIZED,
                            Json(json!({"error": "this credential names no machine pairing admitted"})),
                        )),
                        Ok(Err(e)) => Err(bad(e)),
                        Err(e) => Err(bad(e.into())),
                    }
                }
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
}

/// Who may invite (D6, PA9): not a machine that syncs to a hub, and not a hub that already admits
/// a node, joined or written by hand.
pub fn may_invite(paths: &Paths) -> Result<()> {
    let Some(fc) = crate::fed::load_at(&paths.config)? else {
        return Ok(());
    };
    let (links, _) = fc.sync.links();
    if let Some(link) = links.first() {
        anyhow::bail!(
            "This machine syncs with {}. In this version a machine syncs with one other.",
            link.url
        );
    }
    admits_none(paths, &fc)
}

fn admits_none(paths: &Paths, fc: &crate::fed::FileConfig) -> Result<()> {
    let Some(entry) = fc.server.as_ref().and_then(|s| s.allowlist.first()) else {
        return Ok(());
    };
    let name = read_toml::<Peers>(&paths.peers())?
        .joined
        .into_iter()
        .find(|j| j.node_id == entry.node_id)
        .map(|j| j.name)
        .unwrap_or_else(|| format!("{} (written into the file by hand)", entry.node_id));
    anyhow::bail!(
        "This hub already syncs with {name}. In this version a hub syncs with one other; remove it \
         first."
    )
}

/// The machines pairing admitted and removed, as `peer list` and the Sync pane read them.
pub fn peers_json(paths: &Paths) -> Result<serde_json::Value> {
    let peers: Peers = read_toml(&paths.peers())?;
    Ok(json!({
        "joined": peers.joined.iter().map(|j| json!({
            "name": j.name, "node_id": j.node_id, "workspaces": j.workspaces, "joined_at": j.joined_at,
        })).collect::<Vec<_>>(),
        "removed": peers.removed.iter().map(|r| json!({
            "name": r.name, "node_id": r.node_id, "removed_at": r.removed_at,
        })).collect::<Vec<_>>(),
    }))
}

/// The host part of `host:port`, `[v6]:port` or a bare name.
pub fn host_of(address: &str) -> String {
    if let Some(rest) = address.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest).to_string();
    }
    match address.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host.to_string(),
        _ => address.to_string(),
    }
}

/// The hub's `[server]`, made on the first invitation of a node with none (Section 4.1): a
/// certificate of its own under `hub/`, and `listen`, `tls_cert` and `tls_key` written into
/// `supragnosis.toml`. A `[server]` already present keeps what it names.
pub fn ensure_server(paths: &Paths, addresses: &[String]) -> Result<crate::fed::ServerSection> {
    if let Some(srv) = crate::fed::load_at(&paths.config)?.and_then(|c| c.server) {
        return Ok(srv);
    }
    let mut names = vec!["localhost".to_string()];
    names.extend(local_name());
    for a in addresses {
        let host = host_of(a);
        if !names.contains(&host) {
            names.push(host);
        }
    }
    let (cert, key) = make_certificate(&paths.hub, &names, now())?;
    crate::fed::edit_config_at(&paths.config, |text| {
        let doc: toml_edit::DocumentMut = text.parse().context("parsing supragnosis.toml")?;
        if doc.get("server").is_some() {
            return Ok((text.to_string(), ()));
        }
        // Appended as text, after every line the person wrote: a table inserted by the editor
        // would land before a file's trailing comments.
        let quote = |s: &str| toml_edit::Value::from(s).to_string();
        let mut out = text.to_string();
        if !out.is_empty() {
            out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
        }
        out.push_str(&format!(
            "[server]\nlisten = \"0.0.0.0:7420\"\ntls_cert = {}\ntls_key = {}\n",
            quote(&cert),
            quote(&key)
        ));
        out.parse::<toml_edit::DocumentMut>()
            .context("the [server] lines do not parse")?;
        Ok((out, ()))
    })?;
    tracing::info!(cert = %cert, "made this hub's certificate and its [server] section");
    crate::fed::load_at(&paths.config)?
        .and_then(|c| c.server)
        .context("the [server] section was not written")
}

/// A self-signed end-entity certificate (not a CA) for `names`, ECDSA P-256, valid for ten years,
/// its key `0600` (Section 4.1). Returns the two paths.
pub fn make_certificate(
    dir: &std::path::Path,
    names: &[String],
    now: u64,
) -> Result<(String, String)> {
    crate::private_dir(dir)?;
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .context("making the certificate's key")?;
    let mut params =
        rcgen::CertificateParams::new(names.to_vec()).context("naming the certificate")?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, "supragnosis hub");
    params.is_ca = rcgen::IsCa::ExplicitNoCa;
    let (y, m, d) = civil_date(now);
    params.not_before = rcgen::date_time_ymd(y, m, d);
    params.not_after = rcgen::date_time_ymd(y + 10, m, d.min(28));
    let cert = params.self_signed(&key).context("signing the certificate")?;
    let (cert_path, key_path) = (dir.join("cert.pem"), dir.join("key.pem"));
    crate::write_secret(&key_path, key.serialize_pem().as_bytes())?;
    crate::write_secret(&cert_path, cert.pem().as_bytes())?;
    Ok((cert_path.display().to_string(), key_path.display().to_string()))
}

/// The UTC calendar date of a time, for the certificate's validity.
fn civil_date(secs: u64) -> (i32, u8, u8) {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y as i32, m as u8, d as u8)
}

/// How the code says to check the hub's certificate (Section 3a, D13): by name when it is not one
/// this hub made and it verifies against the built-in roots for every name `--address` gave - never
/// for an IP address - and otherwise pinned by its fingerprint.
pub fn check_for(
    paths: &Paths,
    srv: &crate::fed::ServerSection,
    asked: &[String],
) -> Result<pair::CertCheck> {
    let cert = srv
        .tls_cert
        .as_deref()
        .context("[server] names no tls_cert, and a joining machine reaches the hub over TLS")?;
    let pem = std::fs::read(cert).with_context(|| format!("reading {cert}"))?;
    let made_here = std::path::Path::new(cert).starts_with(&paths.hub);
    let names: Vec<String> = asked.iter().map(|a| host_of(a)).collect();
    if !made_here && !names.is_empty() && supragnosis_sync::http::verifies_by_name(&pem, &names) {
        return Ok(pair::CertCheck::ByName);
    }
    let der = supragnosis_sync::http::certificate_der(&pem)?;
    Ok(pair::CertCheck::Fingerprint(pair::sha256(&der)))
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| command_line("hostname", &[]))
        .map(|h| h.split('.').next().unwrap_or(&h).to_string())
}

/// The host's `.local` name where mDNS answers for it: always on macOS, and on Linux while avahi
/// runs.
pub fn local_name() -> Option<String> {
    if cfg!(target_os = "macos") {
        return command_line("scutil", &["--get", "LocalHostName"]).map(|n| format!("{n}.local"));
    }
    std::path::Path::new("/run/avahi-daemon/socket")
        .exists()
        .then(hostname)
        .flatten()
        .map(|n| format!("{n}.local"))
}

/// The IPv4 address the host's default route leaves from, if it has one. Connecting a UDP socket
/// sends nothing; it only asks the system which address it would use.
fn primary_ipv4() -> Option<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// The addresses a code carries when `--address` gives none (Section 3a): the `.local` name, then
/// the IPv4 address of the default route.
pub fn default_addresses(port: u16) -> Vec<String> {
    let mut out: Vec<String> = local_name().into_iter().map(|n| format!("{n}:{port}")).collect();
    out.extend(primary_ipv4().map(|ip| format!("{ip}:{port}")));
    out
}

/// The name the joining machine shows for this hub: `host_label`, or the machine's own name.
pub fn hub_name(config: &std::path::Path) -> String {
    crate::fed::load_at(config)
        .ok()
        .flatten()
        .and_then(|c| c.host_label)
        .or_else(|| {
            if cfg!(target_os = "macos") {
                command_line("scutil", &["--get", "ComputerName"])
            } else {
                hostname()
            }
        })
        .unwrap_or_else(|| "this hub".into())
}

/// What an invitation should say about staying up (Section 3a): on Linux, that the user unit stops
/// with the person's last session while lingering is off.
pub fn notes() -> Vec<String> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let user = std::env::var("USER").unwrap_or_default();
    match command_line("loginctl", &["show-user", &user, "--property=Linger"]).as_deref() {
        Some("Linger=no") => vec![format!(
            "Lingering is off for {user}, so the daemon - and this hub - stops when your last \
             session ends. `loginctl enable-linger {user}` keeps it up."
        )],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests_support {
    use super::*;
    pub use crate::pair::{CertCheck, Code, OneTime};
    pub use supragnosis_core::NodeIdentity;

    pub const HUB_SECRET: [u8; 32] = [1; 32];

    /// A hub in a directory of its own: a `[server]` with a certificate it made, and a line the
    /// person wrote that every edit keeps.
    pub struct Hub {
        pub dir: PathBuf,
        pub paths: Paths,
        pub store: Arc<dyn AssertionStore>,
    }

    impl Hub {
        pub fn new(name: &str) -> Hub {
            let dir = std::env::temp_dir().join(format!("sg-pair-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let paths = Paths { hub: dir.join("hub"), config: dir.join("supragnosis.toml") };
            let (cert, key) =
                make_certificate(&paths.hub, &["hub.local".into(), "localhost".into()], now())
                    .expect("certificate");
            std::fs::write(
                &paths.config,
                format!(
                    "# the person's own line\nhost_label = \"Mac mini\"\n\n[server]\nlisten = \"127.0.0.1:0\"\ntls_cert = \"{cert}\"\ntls_key = \"{key}\"\n"
                ),
            )
            .unwrap();
            Hub { dir, paths, store: Arc::new(supragnosis_store::InMemoryStore::new()) }
        }

        /// The hub's pairing as a fresh process would open it.
        pub fn pairing(&self) -> Pairing {
            let node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes(HUB_SECRET)));
            Pairing::new(self.paths.clone(), node, self.store.clone(), hub_name(&self.paths.config))
        }

        pub fn config(&self) -> String {
            std::fs::read_to_string(&self.paths.config).unwrap()
        }
    }

    impl Drop for Hub {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    pub fn invite(p: &Pairing, at: u64) -> Code {
        let pem = std::fs::read(p.paths.hub.join("cert.pem")).unwrap();
        let der = supragnosis_sync::http::certificate_der(&pem).unwrap();
        let made = p
            .invite(
                vec!["acme".into()],
                vec!["hub.local:7420".into()],
                CertCheck::Fingerprint(pair::sha256(&der)),
                at,
            )
            .expect("invites");
        Code::decode(&made.code).expect("its own code reads back")
    }

    /// The test's joining machine: a node key, a one-time key and a nonce.
    pub struct Joiner {
        pub id: NodeIdentity,
        pub onetime: OneTime,
        pub nonce: [u8; 32],
    }

    impl Joiner {
        pub fn new(seed: u8) -> Joiner {
            Joiner {
                id: NodeIdentity::from_secret_bytes([seed; 32]),
                onetime: OneTime::new().unwrap(),
                nonce: pair::random32().unwrap(),
            }
        }

        pub fn node_key(&self) -> [u8; 32] {
            hex32(&self.id.public_key_hex()).unwrap()
        }

        pub fn ask(&self, code: &Code, reveal: bool) -> JoinRequest {
            JoinRequest {
                secret: hex(&code.secret),
                node_key: self.id.public_key_hex(),
                name: "MacBook\u{7}".into(),
                onetime_key: hex(&self.onetime.public()),
                commitment: hex(&pair::commitment(
                    &self.node_key(),
                    &self.onetime.public(),
                    &self.nonce,
                )),
                nonce: reveal.then(|| hex(&self.nonce)),
            }
        }

        pub fn words(&self, code: &Code, hub_nonce: &str) -> Vec<String> {
            pair::words(
                &code.secret,
                &self.node_key(),
                &self.onetime.public(),
                &self.nonce,
                &hex32(hub_nonce).unwrap(),
            )
            .iter()
            .map(|w| w.to_string())
            .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;

    fn state(v: &serde_json::Value) -> &str {
        v["state"].as_str().unwrap_or("?")
    }

    /// The whole join from the hub's side (Section 4.3): pending with the hub's nonce, the words
    /// once the nonce is revealed, Allow, and an answer that verifies for this request under the
    /// hub's key and opens only with the one-time key it was sealed to. The allowlist entry it
    /// adds admits the credential inside, and the person's own line stays.
    #[test]
    fn a_join_is_allowed_and_answered_for_its_request_alone() {
        let hub = Hub::new("allow");
        let p = hub.pairing();
        let t = now();
        let code = invite(&p, t);
        assert_eq!(p.open_count(t), 1, "an open invitation counts toward the bind rule");
        let preview = p.preview(&hex(&code.secret), t).unwrap();
        assert_eq!(preview["hub_name"], "Mac mini");
        assert_eq!(preview["workspaces"], serde_json::json!(["acme"]));
        let j = Joiner::new(9);
        let first = p.join(&j.ask(&code, false), t).unwrap();
        assert_eq!(state(&first), "pending", "{first}");
        let hub_nonce = first["hub_nonce"].as_str().unwrap().to_string();
        assert!(
            p.status(t).unwrap()["open"]["request"]["words"].is_null(),
            "no words before the reveal"
        );
        assert!(p.allow(t).is_err(), "no Allow before the words are shown");
        let again = p.join(&j.ask(&code, true), t).unwrap();
        assert_eq!(again["hub_nonce"], hub_nonce.as_str(), "the hub's nonce is fixed");
        let status = p.status(t).unwrap();
        let shown: Vec<String> =
            serde_json::from_value(status["open"]["request"]["words"].clone()).unwrap();
        assert_eq!(shown, j.words(&code, &hub_nonce), "both screens show the same words");
        assert_eq!(status["open"]["request"]["name"], "MacBook", "a name is shown as text");
        let joined = p.allow(t).unwrap();
        assert_eq!(joined["joined"]["workspaces"], serde_json::json!(["acme"]));
        let answered = p.join(&j.ask(&code, true), t).unwrap();
        assert_eq!(state(&answered), "allowed", "{answered}");
        let signed: pair::SignedAnswer =
            serde_json::from_value(answered["answer"].clone()).unwrap();
        let pk = j.onetime.public();
        let expect = pair::Expected {
            code: &code,
            joining_node: &j.id.node_id(),
            onetime_key: &pk,
            joiner_nonce: &j.nonce,
            hub_nonce: &hex32(&hub_nonce).unwrap(),
        };
        let body = pair::verify_answer(&expect, &signed).expect("the answer verifies");
        assert_eq!(body.workspaces, vec!["acme".to_string()]);
        assert!(!body.serves_principals);
        assert!(OneTime::new().unwrap().open(&body.credential).is_err(), "sealed to one key");
        let credential = String::from_utf8(j.onetime.open(&body.credential).unwrap()).unwrap();
        let fc = crate::fed::load_at(&hub.paths.config).unwrap().unwrap();
        let entry = &fc.server.unwrap().allowlist[0];
        assert_eq!(entry.node_id, j.id.node_id(), "the id is derived from the key");
        assert_eq!(entry.bearer_hash, supragnosis_sync::http::bearer_hash(&credential));
        assert_eq!(entry.shared_workspaces, vec!["acme".to_string()]);
        assert!(hub.config().starts_with("# the person's own line\n"), "{}", hub.config());
        assert!(!hub.config().contains(&credential), "the hub keeps only the hash");
        assert!(!std::fs::read_to_string(hub.paths.invites())
            .unwrap()
            .contains(&hex(&code.secret)));
        assert_eq!(p.open_count(t), 0, "a used invitation admits nothing");
        let peers = peers_json(&hub.paths).unwrap();
        assert_eq!(peers["joined"][0]["name"], "MacBook");
    }

    /// An invitation that is no longer open says what became of it (PA2), and one that never was
    /// says so too; a second request, another commitment or a wrong nonce ends it (PA11).
    #[test]
    fn an_invitation_says_what_became_of_it() {
        let hub = Hub::new("states");
        let p = hub.pairing();
        let t = now();
        let (a, b) = (Joiner::new(9), Joiner::new(10));
        let wrong = Code { secret: [42; 32], ..invite(&p, t) };
        assert_eq!(state(&p.join(&a.ask(&wrong, false), t).unwrap()), "unknown");
        assert_eq!(state(&p.preview(&hex(&wrong.secret), t).unwrap()), "unknown");

        let code = invite(&p, t);
        assert_eq!(state(&p.join(&a.ask(&code, false), t + VALID_FOR).unwrap()), "expired");
        assert_eq!(state(&p.preview(&hex(&code.secret), t + VALID_FOR).unwrap()), "expired");

        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        let second = p.join(&b.ask(&code, false), t).unwrap();
        assert_eq!(state(&second), "ended", "a second request ends it: {second}");
        assert_eq!(state(&p.join(&a.ask(&code, true), t).unwrap()), "ended", "both are told");

        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        let mut other = a.ask(&code, false);
        other.commitment = hex(&[3; 32]);
        assert_eq!(state(&p.join(&other, t).unwrap()), "ended", "another commitment ends it");

        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        let mut lie = a.ask(&code, true);
        lie.nonce = Some(hex(&[4; 32]));
        assert_eq!(state(&p.join(&lie, t).unwrap()), "ended", "a nonce must match its commitment");

        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        p.join(&a.ask(&code, true), t).unwrap();
        assert_eq!(p.close(State::Denied, t).unwrap()["state"], "denied");
        assert_eq!(state(&p.join(&a.ask(&code, true), t).unwrap()), "denied");

        let code = invite(&p, t);
        let replaced = invite(&p, t);
        assert_eq!(
            state(&p.join(&a.ask(&code, false), t).unwrap()),
            "cancelled",
            "a new code cancels the open one"
        );
        p.close(State::Cancelled, t).unwrap();
        assert_eq!(state(&p.preview(&hex(&replaced.secret), t).unwrap()), "cancelled");

        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        p.join(&a.ask(&code, true), t).unwrap();
        p.allow(t).unwrap();
        let reused = p.join(&b.ask(&code, false), t).unwrap();
        assert_eq!(state(&reused), "used", "a used code is said as used: {reused}");
        assert_eq!(reused["said"], State::Used.said());
        // A tombstone stays for a day past its expiry, then the hub forgets the code.
        let late = t + VALID_FOR + TOMBSTONE_FOR;
        assert_eq!(state(&p.join(&b.ask(&code, false), late - 1).unwrap()), "used");
        assert_eq!(state(&p.join(&b.ask(&code, false), late).unwrap()), "unknown");
    }

    /// Who may join and who may invite (PA8, PA9): a request with the hub's own key, or an admitted
    /// one, is refused and leaves the invitation open; a hub that admits a machine, by pairing or by
    /// hand, and a machine that syncs to a hub, do not invite.
    #[test]
    fn who_may_join_and_who_may_invite() {
        let hub = Hub::new("who");
        let p = hub.pairing();
        let t = now();
        let code = invite(&p, t);
        let me = Joiner { id: NodeIdentity::from_secret_bytes(HUB_SECRET), ..Joiner::new(9) };
        let own = p.join(&me.ask(&code, false), t).unwrap();
        assert_eq!(state(&own), "refused", "{own}");
        let a = Joiner::new(9);
        p.join(&a.ask(&code, false), t).unwrap();
        p.join(&a.ask(&code, true), t).unwrap();
        p.allow(t).unwrap();
        let refused = p.invite(vec![], vec!["hub.local:7420".into()], CertCheck::ByName, t);
        assert!(
            refused.unwrap_err().to_string().contains("already syncs with MacBook"),
            "one joined machine per hub"
        );
        assert!(may_invite(&hub.paths).is_err());

        let by_hand = Hub::new("by-hand");
        std::fs::write(
            &by_hand.paths.config,
            "[server]\nlisten = \"127.0.0.1:0\"\n[[server.allowlist]]\nnode_id = \"abc\"\npublic_key_hex = \"00\"\nbearer_hash = \"00\"\nshared_workspaces = []\n",
        )
        .unwrap();
        let e = may_invite(&by_hand.paths).unwrap_err().to_string();
        assert!(e.contains("written into the file by hand"), "{e}");
        let removed = by_hand.pairing().remove("abc", t).unwrap_err().to_string();
        assert!(removed.contains("by hand"), "pairing removes only what it added: {removed}");

        let spoke = Hub::new("spoke");
        std::fs::write(
            &spoke.paths.config,
            "[sync]\n[[sync.server]]\nurl = \"https://hub.example:7420\"\nauth_token = \"t\"\n",
        )
        .unwrap();
        let e = may_invite(&spoke.paths).unwrap_err().to_string();
        assert!(
            e.contains("syncs with https://hub.example:7420"),
            "a joined machine does not invite: {e}"
        );
    }

    /// The hub fixes a request at its first `join` and keeps it, and the answer after Allow, in its
    /// file: a restart mid-join or after Allow loses nothing the joining machine waits for.
    #[test]
    fn the_request_and_the_answer_survive_a_restart() {
        let hub = Hub::new("restart");
        let t = now();
        let a = Joiner::new(9);
        let code = invite(&hub.pairing(), t);
        let nonce = hub.pairing().join(&a.ask(&code, false), t).unwrap()["hub_nonce"].clone();
        let after = hub.pairing().join(&a.ask(&code, true), t).unwrap();
        assert_eq!(after["hub_nonce"], nonce, "a restarted hub keeps its nonce");
        let status = hub.pairing().status(t).unwrap();
        assert!(status["open"]["code"].is_null(), "the code itself is not kept");
        assert!(status["open"]["request"]["words"].is_array());
        hub.pairing().allow(t).unwrap();
        let answered = hub.pairing().join(&a.ask(&code, true), t).unwrap();
        assert_eq!(state(&answered), "allowed", "{answered}");
    }

    /// Removing a joined machine (Section 4.6) drops its entry and keeps its key verify-only, and
    /// its credential then admits nothing, `leave` included.
    #[test]
    fn a_removed_machine_is_refused_and_its_key_kept() {
        let hub = Hub::new("remove");
        let p = hub.pairing();
        let t = now();
        let a = Joiner::new(9);
        let code = invite(&p, t);
        let first = p.join(&a.ask(&code, false), t).unwrap();
        p.join(&a.ask(&code, true), t).unwrap();
        p.allow(t).unwrap();
        let signed: pair::SignedAnswer =
            serde_json::from_value(p.join(&a.ask(&code, true), t).unwrap()["answer"].clone())
                .unwrap();
        let body: pair::AnswerBody = serde_json::from_str(&signed.body).unwrap();
        let credential = String::from_utf8(a.onetime.open(&body.credential).unwrap()).unwrap();
        assert!(first["hub_nonce"].is_string());
        let removed = p.remove("MacBook", t).unwrap();
        assert!(removed["said"].as_str().unwrap().contains("stays on it"));
        let fc = crate::fed::load_at(&hub.paths.config).unwrap().unwrap();
        assert!(fc.server.unwrap().allowlist.is_empty(), "its entry is gone");
        assert!(p.leave(&credential, t).unwrap().is_none(), "its credential admits nothing");
        let peers = p.peers().unwrap();
        assert!(peers.joined.is_empty());
        assert_eq!(peers.removed[0].public_key, a.id.public_key_hex(), "kept, verify-only");
        assert!(may_invite(&hub.paths).is_ok(), "removing it lifts the one-machine limit");
    }

    /// `leave` with a joined machine's credential removes that machine alone.
    #[test]
    fn a_joined_machine_leaves_with_its_own_credential() {
        let hub = Hub::new("leave");
        let p = hub.pairing();
        let t = now();
        let a = Joiner::new(9);
        let code = invite(&p, t);
        p.join(&a.ask(&code, false), t).unwrap();
        p.join(&a.ask(&code, true), t).unwrap();
        p.allow(t).unwrap();
        let signed: pair::SignedAnswer =
            serde_json::from_value(p.join(&a.ask(&code, true), t).unwrap()["answer"].clone())
                .unwrap();
        let body: pair::AnswerBody = serde_json::from_str(&signed.body).unwrap();
        let credential = String::from_utf8(a.onetime.open(&body.credential).unwrap()).unwrap();
        assert!(p.leave("not a credential", t).unwrap().is_none());
        assert_eq!(p.leave(&credential, t).unwrap().unwrap()["removed"], "MacBook");
    }

    #[test]
    fn the_certificate_lasts_ten_years_from_today() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(1_792_000_000), (2026, 10, 14));
        assert_eq!(civil_date(951_782_400), (2000, 2, 29));
        assert_eq!(civil_date(4_102_444_800), (2100, 1, 1));
        assert_eq!(host_of("hub.local:7420"), "hub.local");
        assert_eq!(host_of("[fe80::1]:7420"), "fe80::1");
        assert_eq!(host_of("hub.example.net"), "hub.example.net");
        let hub = Hub::new("cert");
        let pem = std::fs::read(hub.paths.hub.join("cert.pem")).unwrap();
        assert!(
            !supragnosis_sync::http::verifies_by_name(&pem, &["hub.local".into()]),
            "a certificate the hub made is never checked by name"
        );
        assert!(!supragnosis_sync::http::verifies_by_name(&pem, &["10.0.0.1".into()]));
        let srv = crate::fed::load_at(&hub.paths.config).unwrap().unwrap().server.unwrap();
        assert!(matches!(
            check_for(&hub.paths, &srv, &["hub.example.net".into()]).unwrap(),
            CertCheck::Fingerprint(_)
        ));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(hub.paths.hub.join("key.pem")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the key is owner-only");
    }

    /// The first invitation on a node with no `[server]` makes the certificate and writes the
    /// section; one already present keeps what it names (Section 4.1).
    #[test]
    fn the_first_invitation_makes_the_server_section() {
        let hub = Hub::new("first");
        std::fs::write(&hub.paths.config, "# mine\n").unwrap();
        let srv = ensure_server(&hub.paths, &["hub.example.net:7420".into()]).unwrap();
        assert_eq!(srv.listen, "0.0.0.0:7420");
        assert!(srv.tls_cert.as_deref().unwrap().starts_with(hub.paths.hub.to_str().unwrap()));
        assert!(hub.config().starts_with("# mine\n"), "{}", hub.config());
        let again = ensure_server(&hub.paths, &[]).unwrap();
        assert_eq!(again.tls_cert, srv.tls_cert, "a [server] already there stays");
        let none = Hub::new("none");
        std::fs::remove_file(&none.paths.config).unwrap();
        ensure_server(&none.paths, &[]).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&none.paths.config).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "a config it creates is owner-only");
    }
}

#[cfg(test)]
mod over_the_network {
    use super::tests_support::*;
    use super::*;

    /// The pairing calls on the hub's own listener (Section 4.3): `preview` and `join` with the
    /// code's secret, Allow on the hub, the minted credential admitted by the sync API at once, and
    /// refused again once the machine is removed (PA7). On loopback, so the test opens no port to
    /// the network, and over TLS with the certificate the hub made, pinned as the code pins it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_machine_joins_over_the_listener_and_is_refused_once_removed() {
        let hub = Hub::new("network");
        // A port of its own rather than 0, which every apply would move.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let config = hub.config().replace("127.0.0.1:0", &format!("127.0.0.1:{port}"));
        std::fs::write(&hub.paths.config, config).unwrap();
        let pairing = Arc::new(hub.pairing());
        let node =
            Arc::new(SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes(HUB_SECRET)));
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let listener = Arc::new(crate::hub::HubListener::new(
            hub.store.clone(),
            node,
            supragnosis_sync::http::Hooks::default(),
            peers,
            pairing.clone(),
            Default::default(),
        ));
        listener.follow();
        let t = now();
        let code = invite(&pairing, t);
        let state = listener.refresh().await;
        assert_eq!(state["listening"], true, "{state}");
        let base = format!("https://{}", state["address"].as_str().unwrap());
        let pem = std::fs::read(hub.paths.hub.join("cert.pem")).unwrap();
        let tls = supragnosis_sync::http::tls_trusting(&pem).unwrap();
        let http = reqwest::Client::builder().use_preconfigured_tls(tls).build().unwrap();
        let link = |token: &str| supragnosis_sync::ServerLink {
            url: base.clone(),
            auth_token: token.to_string(),
            ca_pem: Some(pem.clone()),
        };
        let post = |path: &str, body: serde_json::Value| {
            let (http, url) = (http.clone(), format!("{base}{path}"));
            async move {
                http.post(url)
                    .json(&body)
                    .send()
                    .await
                    .unwrap()
                    .json::<serde_json::Value>()
                    .await
                    .unwrap()
            }
        };
        let preview = post("/pair/preview", serde_json::json!({"secret": hex(&code.secret)})).await;
        assert_eq!(preview["state"], "open", "{preview}");
        let j = Joiner::new(9);
        let pending = post("/pair/join", serde_json::to_value(j.ask(&code, false)).unwrap()).await;
        assert_eq!(pending["state"], "pending", "{pending}");
        post("/pair/join", serde_json::to_value(j.ask(&code, true)).unwrap()).await;
        pairing.allow(t).unwrap();
        let allowed = post("/pair/join", serde_json::to_value(j.ask(&code, true)).unwrap()).await;
        let signed: pair::SignedAnswer = serde_json::from_value(allowed["answer"].clone()).unwrap();
        let body: pair::AnswerBody = serde_json::from_str(&signed.body).unwrap();
        let credential = String::from_utf8(j.onetime.open(&body.credential).unwrap()).unwrap();
        // The listener follows the change on its own: the entry admits at once.
        let mut admitted = false;
        for _ in 0..50 {
            let client = supragnosis_sync::http::SyncClient::for_link(&link(&credential)).unwrap();
            if client.ping().await.is_ok() {
                admitted = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(admitted, "the joined machine's credential is admitted");
        pairing.remove("MacBook", t).unwrap();
        let mut refused = false;
        for _ in 0..50 {
            let client = supragnosis_sync::http::SyncClient::for_link(&link(&credential)).unwrap();
            if client.ping().await.is_err() {
                refused = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(refused, "a removed machine's next request is refused");
        let left = http
            .post(format!("{base}/pair/leave"))
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap();
        assert_eq!(left.status(), 401, "and its credential leaves nothing either");
    }
}
