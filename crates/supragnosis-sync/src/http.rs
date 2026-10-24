//! Sync transport (M4 Phase 3, docs/federation.md Section 6a): the axum sync API
//! (`/sync/advertise|pull|push`) with in-process rustls TLS and bearer/allowlist wire auth, plus the
//! reqwest client. This is the ONLY surface permitted to bind non-loopback, and only with TLS enabled
//! and at least one admitted node or open invitation (F10, pairing.md PA1) - the MCP/viz loopback
//! guard is untouched. Wire
//! auth here is transport authentication (who is on the wire); content trust stays with the apply
//! pipeline's signature verification (F6) and the read side (F13).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use supragnosis_core::{AssertionStore, AttestationEvent, SearchHit, VersionVector};

use crate::{export_delta, version_vector, ApplyReport, SyncError, SyncNode};

/// The bounds of one sync request (sync-correctness.md Section 11). Every event fits inside them,
/// because `observe` caps content at 1 MiB.
///
/// The body limit the sync routes declare, rather than inheriting axum's unnamed 2 MB default.
pub const BODY_LIMIT: usize = 8 << 20;
/// A push batch: at most this many encoded bytes, or [`BATCH_EVENTS`] events, whichever comes first.
pub const BATCH_BYTES: usize = 1 << 20;
pub const BATCH_EVENTS: usize = 500;
/// A pull page: at most the events the client asks for (at most [`PAGE_EVENTS`]), and at most
/// [`PAGE_BYTES`] encoded - always at least one event, so a page always makes progress.
pub const PAGE_EVENTS: usize = 500;
pub const PAGE_BYTES: usize = 4 << 20;
/// The sync protocol's version (sync-correctness.md Section 12). Raised by a change an older peer
/// would mishandle - a new enum value on the wire, a new signed field, a changed meaning - and not
/// by an optional field, an endpoint or a header, which older peers ignore. A peer that sends none
/// speaks protocol 0: everything before this number existed.
pub const PROTOCOL: u32 = 1;
/// The headers every request and every response carry: the protocol, and the binary's release.
pub const PROTOCOL_HEADER: &str = "supragnosis-sync";
pub const RELEASE_HEADER: &str = "supragnosis-release";

/// What a request's headers say about the peer that sent it. `(0, None)` from a peer before them.
fn peer_speaks(headers: &HeaderMap) -> (u32, Option<String>) {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let protocol = text(PROTOCOL_HEADER).and_then(|v| v.parse().ok()).unwrap_or(0);
    (protocol, text(RELEASE_HEADER))
}

/// How long one sync request may take, on either side.
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// TLS that trusts the certificates in `ca_pem` (sync-correctness.md Section 10), two ways.
///
/// - A server that presents one of them exactly is that certificate's host, and is trusted as it.
///   This is the self-signed hub: its `tls_cert`, copied to the spoke. `openssl req -x509` marks
///   such a certificate as a CA by default, and a verifier that only builds chains refuses a CA
///   presented as the server's own certificate (`CaUsedAsEndEntity`) - the sandbox's hub did.
/// - Any other server certificate must chain to one of them, with its name checked: the file is
///   then a CA bundle.
///
/// Either way the handshake's signatures are verified against the certificate presented, so a
/// server that does not hold the key of the certificate it shows is refused.
///
/// Shared with every client that names a hub's certificate: the sync link, a remote MCP profile's
/// bridge, and `server`'s check of it - all reach the same `[server]` listener.
pub fn tls_trusting(ca_pem: &[u8]) -> Result<rustls::ClientConfig, TransportError> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(ca_pem)
        .collect::<Result<_, _>>()
        .map_err(|e| TransportError::Tls(format!("the ca file is not PEM certificates: {e}")))?;
    if certs.is_empty() {
        return Err(TransportError::Tls("the ca file holds no certificate".into()));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for cert in &certs {
        // One webpki cannot take as a root is still trusted by exact match below.
        let _ = roots.add(cert.clone());
    }
    let chain = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .ok();
    let verifier = Arc::new(NamedCertificates { pinned: certs, chain, provider: provider.clone() });
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The DER of the first certificate in `pem` - what a pairing code's fingerprint is taken over
/// (docs/pairing.md Section 4.2).
pub fn certificate_der(pem: &[u8]) -> Result<Vec<u8>, TransportError> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    CertificateDer::pem_slice_iter(pem)
        .next()
        .ok_or_else(|| TransportError::Tls("the PEM holds no certificate".into()))?
        .map(|c| c.as_ref().to_vec())
        .map_err(|e| TransportError::Tls(format!("the certificate is not PEM: {e}")))
}

#[derive(Debug)]
struct NamedCertificates {
    pinned: Vec<rustls::pki_types::CertificateDer<'static>>,
    chain: Option<Arc<rustls::client::WebPkiServerVerifier>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for NamedCertificates {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if self.pinned.iter().any(|c| c.as_ref() == end_entity.as_ref()) {
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }
        match &self.chain {
            Some(v) => {
                v.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            }
            None => Err(rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// What a refused pin says, so a joining machine can tell "not the machine that made the code" from
/// a host it could not reach (docs/pairing.md Section 4.3).
pub const NOT_THE_CODES_HUB: &str = "the certificate is not the one the join code names";

/// Says yes to the DER of the one certificate a pin names.
pub type AcceptCertificate = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// The HTTP client a join uses (docs/pairing.md Section 4.3): it accepts exactly the certificate the
/// code's fingerprint names, from any address - `accept` says yes to its DER.
pub fn join_client(accept: AcceptCertificate) -> Result<reqwest::Client, TransportError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // A first address that drops packets - the LAN one, off the LAN - is given up on quickly, so
    // the next is tried well inside the code's ten minutes.
    let builder = reqwest::Client::builder()
        .timeout(CALL_TIMEOUT)
        .connect_timeout(std::time::Duration::from_secs(5));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedBy { accept, provider: provider.clone() });
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(builder.use_preconfigured_tls(tls).build()?)
}

/// Accepts the one certificate a pin names, whatever address presented it; the handshake's
/// signatures are still verified against it, so a server without its key is refused.
struct PinnedBy {
    accept: AcceptCertificate,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl std::fmt::Debug for PinnedBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedBy")
    }
}

impl rustls::client::danger::ServerCertVerifier for PinnedBy {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        match (self.accept)(end_entity.as_ref()) {
            true => Ok(rustls::client::danger::ServerCertVerified::assertion()),
            false => Err(rustls::Error::General(NOT_THE_CODES_HUB.into())),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// The (origin, seq) of a wire event, read without decoding it. Empty for an unstamped one.
fn stream_of(raw: &serde_json::Value) -> (String, u64) {
    let sync = &raw["attestation"]["sync"];
    (
        sync["origin_node"].as_str().unwrap_or_default().to_string(),
        sync["origin_seq"].as_u64().unwrap_or_default(),
    )
}

/// One admitted peer node (docs/federation.md 6a): wire credentials + what it may read/write.
/// `bearer_hash` is the blake3 hex of the peer's bearer token - the server never stores the token.
///
/// Denies unknown keys, like every other section of the config. It was the one struct that did not,
/// and federation.md Section 9 recorded it as a standing gap: an admin marking or a misspelled
/// `shared_workspaces` inside an entry parsed clean and did nothing, which is exactly the failure the
/// rest of the file's `deny_unknown_fields` exists to prevent. An entry is also the place a typo is
/// most expensive - it decides who may connect and what they may read (P17/P18), so failing to parse
/// is the only safe way to be wrong about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowEntry {
    pub node_id: String,
    pub public_key_hex: String,
    pub bearer_hash: String,
    pub shared_workspaces: Vec<String>,
}

/// Transport failure. Distinct from per-event rejections (those ride the apply report, F6) and from
/// store failures (P5: surfaced, never silent).
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error(
        "refusing to bind {addr}: {reason} (F10: non-loopback needs TLS and an admitted node or an open invitation)"
    )]
    Bind { addr: SocketAddr, reason: String },
    #[error("tls setup failed: {0}")]
    Tls(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server returned {status}: {body}")]
    Remote { status: u16, body: String },
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl TransportError {
    /// Whether the host was not reached at all - a partition, not an answer.
    pub fn unreachable(&self) -> bool {
        matches!(self, TransportError::Http(e) if e.is_connect() || e.is_timeout())
    }

    /// What a failed call says to a person: the error and every cause under it, and, when the host
    /// was not reached, that this node goes on offline and syncs once it is reached again - a
    /// partition is a state a node works through, not a fault (docs/pairing.md, decision D16). On a
    /// Mac, "no route" is also how macOS refuses a process without Local Network access.
    pub fn said(&self) -> String {
        let mut out = self.to_string();
        let mut cause = std::error::Error::source(self);
        while let Some(c) = cause {
            let text = c.to_string();
            if !out.contains(&text) {
                out.push_str(": ");
                out.push_str(&text);
            }
            cause = c.source();
        }
        if self.unreachable() {
            out.push_str(
                ". This node keeps working offline, and its next round syncs once the hub can be \
                 reached",
            );
            let no_route = out.contains("os error 65") || out.contains("No route to host");
            if cfg!(target_os = "macos") && no_route {
                out.push_str(
                    ". On a Mac, macOS answers this way for a process without Local Network \
                     access (System Settings > Privacy & Security > Local Network)",
                );
            }
        }
        out
    }
}

/// F10 bind guard: loopback is always fine (local trust surface); a non-loopback bind demands both
/// in-process TLS and someone admitted - a node on the allowlist, or an open invitation the pairing
/// calls the wiring layer mounts beside the sync API answer (docs/pairing.md PA1). The
/// local MCP daemon's own guard is separate and unchanged.
pub fn validate_bind(
    addr: &SocketAddr,
    tls: bool,
    allowlist_len: usize,
) -> Result<(), TransportError> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if !tls {
        return Err(TransportError::Bind { addr: *addr, reason: "TLS is not enabled".into() });
    }
    if allowlist_len == 0 {
        return Err(TransportError::Bind {
            addr: *addr,
            reason: "nothing is admitted (no allowlisted node, no open invitation)".into(),
        });
    }
    Ok(())
}

/// Post-apply hook: called with the workspace after a push lands new events - the wiring layer
/// injects the engine's re-materialization here (Prop C: replay is the convergence point; the sync
/// crate itself stays projection-free).
pub type OnApplied = Arc<dyn Fn(&str) + Send + Sync>;

/// One sync-API hit, for live observability (the hub viewer's activity feed): who (peer node_id),
/// what (direction), where (workspace), and how much (events served/accepted).
#[derive(Debug, Clone)]
pub struct SyncActivity {
    pub direction: &'static str,
    pub peer: String,
    pub workspace: String,
    pub count: usize,
}

/// Activity hook: the wiring layer forwards hits into the node's event stream (viewer SSE).
pub type OnActivity = Arc<dyn Fn(SyncActivity) + Send + Sync>;

/// The header a 0.10 node sent its consent to serve a workspace in, which a 0.10 hub read to serve
/// it to its principals (docs/remote-server.md Section 4.5). Principals were removed in 0.11.0, so a
/// 0.11 node says "no" in it on each advertise, pull and push, and a 0.10 hub withdraws its consent
/// for the workspaces the two still sync (remote-server.md Section 13); revisit when no 0.10 hub
/// remains.
pub const SERVE_HEADER: &str = "supragnosis-serve";

/// Federated-retrieval hook: the wiring layer injects the engine's hybrid search so a remote read
/// query answers from this node's full retrieval surface (hits, mode). Without it the handler falls
/// back to the store's keyword path.
pub type OnSearch =
    Arc<dyn Fn(&str, &str, usize) -> Result<(Vec<SearchHit>, String), String> + Send + Sync>;

/// Runtime peer observability (docs/federation.md 6a): which admitted peers have actually checked
/// in, when, and how much. Distinct from the allowlist - the allowlist ADMITS, this OBSERVES.
/// In-memory (resets with the process); persistence is not a goal, liveness insight is.
#[derive(Default)]
pub struct PeerRegistry {
    peers: std::sync::Mutex<BTreeMap<String, PeerStatus>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerStatus {
    pub node_id: String,
    /// The sync protocol the peer last spoke - 0 for a release before the protocol header.
    pub protocol: u32,
    /// The release the peer last said it runs. `None` for a release before the header.
    pub release: Option<String>,
    /// Epoch millis of the most recent authenticated request.
    pub last_seen_ms: u64,
    /// The most recent action ("ping" | "advertise" | "pull" | "push" | "search").
    pub last_action: String,
    /// Authenticated requests since process start.
    pub hits: u64,
}

impl PeerRegistry {
    /// Records a request and what the peer said about itself (sync-correctness.md Section 12).
    pub fn record(&self, node_id: &str, action: &str, speaks: (u32, Option<String>)) {
        let mut m = self.peers.lock().unwrap();
        let e = m.entry(node_id.to_string()).or_insert_with(|| PeerStatus {
            node_id: node_id.to_string(),
            protocol: 0,
            release: None,
            last_seen_ms: 0,
            last_action: String::new(),
            hits: 0,
        });
        e.last_seen_ms = supragnosis_core::now_millis();
        e.last_action = action.to_string();
        e.hits += 1;
        (e.protocol, e.release) = speaks;
    }

    /// The release a peer last said it runs, if it has said.
    pub fn release_of(&self, node_id: &str) -> Option<String> {
        self.peers.lock().unwrap().get(node_id).and_then(|p| p.release.clone())
    }

    pub fn snapshot(&self) -> Vec<PeerStatus> {
        self.peers.lock().unwrap().values().cloned().collect()
    }
}

/// The wiring-layer hooks injected into the sync API (all optional): re-materialization after
/// inbound pushes, activity streaming to the viewer, and the engine-backed federated retrieval.
#[derive(Default, Clone)]
pub struct Hooks {
    pub on_applied: Option<OnApplied>,
    pub on_activity: Option<OnActivity>,
    pub on_search: Option<OnSearch>,
    /// Shared known-peer registry - the wiring layer keeps a clone so the MCP surface can report it.
    pub peer_registry: Option<Arc<PeerRegistry>>,
    /// A further surface served on this listener - the pairing calls an open invitation answers
    /// (docs/pairing.md Section 4.2). It authenticates its own requests; `admitted` counts toward
    /// the bind rule.
    pub extra: Option<ExtraSurface>,
}

/// Routes the wiring layer serves beside the sync API, on the same TLS listener.
#[derive(Clone)]
pub struct ExtraSurface {
    pub router: Router,
    /// How many it admits at startup - the bind rule needs someone admitted.
    pub admitted: usize,
}

/// The admitted peers, consulted per request instead of captured when the server was spawned.
///
/// Admission used to be a startup snapshot: `[server].allowlist` was read once, moved into the
/// handler state, and the only way to change who may connect was to edit the file and restart. That
/// is the shape federation.md Section 11 recorded as "allowlist is static-edit for now", and it is
/// what a management surface has to replace first - otherwise "remove this peer" means "stop the
/// daemon", which is not a thing an operator can do casually on a hub other nodes sync to.
///
/// `origin_keys` lives in here rather than beside it because it is **derived** from the allowlist:
/// every admitted peer's key, plus this node's own. Two separately-mutable fields where one is a
/// function of the other is a drift waiting to happen, and this particular drift is a security bug -
/// a peer removed from the allowlist whose key stayed in the directory would be refused at the door
/// and then have its relayed events verified anyway. Derivation happens on write, so the stale state
/// is unrepresentable rather than merely avoided.
///
/// **An empty allowlist is fail-closed, and allowed at runtime.** `validate_bind` refuses to *start*
/// a non-loopback surface with no admitted peers (F10), which stops an unauthenticated surface from
/// existing; arriving at empty later rejects every request instead of admitting anyone, so it serves
/// the same end. Refusing the mutation would mean an operator cannot remove their last peer without
/// stopping the daemon - which is exactly the property this type exists to end.
pub struct PeerDirectory {
    inner: std::sync::RwLock<Admitted>,
    /// This node's own id/key. Kept beside the lock because it is a constant of the process and the
    /// one entry in `origin_keys` that does not come from the allowlist - recovering it by asking
    /// which key has no matching entry would break the moment a peer's id collided with it.
    own: (String, String),
}

/// One consistent view of admission for the length of one request. Taken once per request, where
/// the bearer is checked, so a concurrent change cannot land between authenticating a peer and
/// verifying what it sent.
#[derive(Clone)]
pub struct Admitted {
    pub allowlist: Vec<AllowEntry>,
    /// Every admitted peer's origin key, plus this node's own - the directory apply verifies against.
    pub origin_keys: BTreeMap<String, String>,
}

impl PeerDirectory {
    pub fn new(allowlist: Vec<AllowEntry>, self_node_id: &str, self_public_key_hex: &str) -> Self {
        let own = (self_node_id.to_string(), self_public_key_hex.to_string());
        Self { inner: std::sync::RwLock::new(Admitted::derive(allowlist, &own.0, &own.1)), own }
    }

    /// The current view. Cheap enough to clone per request at any allowlist an operator would hand-
    /// maintain, and cloning is what makes the "one view per request" property hold by construction.
    pub fn admitted(&self) -> Admitted {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Replaces the admitted set. The derived key directory is recomputed here, never separately.
    pub fn replace(&self, allowlist: Vec<AllowEntry>) {
        let mut g = self.inner.write().unwrap_or_else(|e| e.into_inner());
        *g = Admitted::derive(allowlist, &self.own.0, &self.own.1);
    }
}

impl Admitted {
    /// Both construction paths funnel through here, which is why the self-admission filter lives in
    /// it rather than in a caller: an entry naming this node would otherwise slip back in through
    /// `replace` the moment a console narrowed some other peer.
    ///
    /// A node cannot be its own peer - it would answer its own pulls, and once routing consults a
    /// host's answer it would negotiate with itself (F14). The entry is dropped rather than refused
    /// because admission authenticates by bearer hash, so such an entry is a live credential and
    /// ignoring it can only share LESS: the one direction 6a lets a mistake move without asking.
    ///
    /// An entry whose key is not in the one spelling this software writes is dropped here too, for
    /// the same reason and in the same direction (sync-correctness.md Section 7): it could never
    /// verify an event, and admitting its bearer anyway would let a peer in whose writes all fail.
    fn derive(allowlist: Vec<AllowEntry>, self_node_id: &str, self_public_key_hex: &str) -> Self {
        let allowlist: Vec<AllowEntry> = allowlist
            .into_iter()
            .filter(|e| e.node_id != self_node_id)
            .filter(|e| supragnosis_core::is_canonical_public_key_hex(&e.public_key_hex))
            .collect();
        let mut origin_keys: BTreeMap<String, String> = allowlist
            .iter()
            .map(|e| (e.node_id.clone(), e.public_key_hex.clone()))
            .collect();
        origin_keys.insert(self_node_id.to_string(), self_public_key_hex.to_string());
        Self { allowlist, origin_keys }
    }
}

/// Shared state of the sync API handlers.
pub struct ServerState {
    pub store: Arc<dyn AssertionStore>,
    pub node: Arc<SyncNode>,
    /// Who may connect, and the keys derived from that - swappable without a restart.
    pub peers: Arc<PeerDirectory>,
    on_applied: Option<OnApplied>,
    on_activity: Option<OnActivity>,
    on_search: Option<OnSearch>,
    registry: Option<Arc<PeerRegistry>>,
}

impl ServerState {
    pub fn new(
        store: Arc<dyn AssertionStore>,
        node: Arc<SyncNode>,
        allowlist: Vec<AllowEntry>,
    ) -> Self {
        let peers = Arc::new(PeerDirectory::new(allowlist, node.node_id(), &node.public_key_hex()));
        Self {
            store,
            node,
            peers,
            on_applied: None,
            on_activity: None,
            on_search: None,
            registry: None,
        }
    }

    /// The admission directory, so the wiring layer can hand it to a management surface. Handing out
    /// the `Arc` rather than a setter is what lets admission change while the server keeps serving.
    pub fn peers(&self) -> Arc<PeerDirectory> {
        self.peers.clone()
    }

    /// Builds state around an existing directory, so the wiring layer keeps the handle it will hand
    /// to a management surface - the same shape as the known-peer registry in [`Hooks`].
    pub fn with_directory(
        store: Arc<dyn AssertionStore>,
        node: Arc<SyncNode>,
        peers: Arc<PeerDirectory>,
    ) -> Self {
        Self {
            store,
            node,
            peers,
            on_applied: None,
            on_activity: None,
            on_search: None,
            registry: None,
        }
    }

    /// Injects the post-apply re-materialization hook (the engine's reproject).
    pub fn with_on_applied(mut self, f: OnApplied) -> Self {
        self.on_applied = Some(f);
        self
    }

    /// Injects the live-activity hook (streams sync hits to the viewer).
    pub fn with_on_activity(mut self, f: OnActivity) -> Self {
        self.on_activity = Some(f);
        self
    }

    /// Injects the federated-retrieval hook (the engine's hybrid search).
    pub fn with_on_search(mut self, f: OnSearch) -> Self {
        self.on_search = Some(f);
        self
    }

    /// Shares the known-peer registry (runtime observability, 6a).
    pub fn with_peers(mut self, r: Arc<PeerRegistry>) -> Self {
        self.registry = Some(r);
        self
    }

    fn seen(&self, node_id: &str, action: &str, headers: &HeaderMap) {
        if let Some(r) = &self.registry {
            r.record(node_id, action, peer_speaks(headers));
        }
    }

    fn activity(&self, direction: &'static str, peer: &str, workspace: &str, count: usize) {
        if let Some(hook) = &self.on_activity {
            hook(SyncActivity {
                direction,
                peer: peer.to_string(),
                workspace: workspace.to_string(),
                count,
            });
        }
    }
}

// --- wire types --------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvertiseReq {
    pub workspace: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvertiseResp {
    pub node_id: String,
    pub vv: VersionVector,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullReq {
    pub workspace: String,
    pub since: VersionVector,
    /// The most events wanted in one page. An older hub ignores it and answers with everything,
    /// which a newer client reads as one page; an older client sends none and gets one page of
    /// [`PAGE_EVENTS`], continued by its next round (sync-correctness.md Section 11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Events cross the wire as JSON values and are decoded one at a time on receipt
/// (sync-correctness.md Section 6): an event this release cannot decode is rejected alone, where a
/// typed list would fail the whole body - every other event with it - every round.
#[derive(Debug, Serialize, Deserialize)]
pub struct PullResp {
    pub events: Vec<serde_json::Value>,
    /// More events remain beyond this page. Absent from an older hub, which sends everything.
    #[serde(default)]
    pub more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushReq {
    pub workspace: String,
    pub events: Vec<serde_json::Value>,
}

/// Whether a rejection a host reported holds its stream there ([`crate::RejectReason::holds`]). A reason
/// travels as its `Debug` form, so the ones that do not hold are told apart by name; an older host
/// holds them too, and then only answers `Held` for what is sent behind them.
fn holds_at_the_host(reason: &str) -> bool {
    !["Malformed", "WorkspaceMismatch", "Unstamped"]
        .iter()
        .any(|r| reason.starts_with(r))
}

/// The wire form of outgoing events.
fn to_wire(events: Vec<AttestationEvent>) -> Vec<serde_json::Value> {
    events
        .into_iter()
        .map(|e| serde_json::to_value(e).expect("an event serializes"))
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchReq {
    pub workspace: String,
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResp {
    /// The retrieval surface the SERVER used (hybrid/keyword) - results are the server's node-local
    /// retrieval aid (P16 exemption): to become local graph material they must arrive via pull (F1).
    pub mode: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingResp {
    /// The hub's node id.
    pub node_id: String,
    /// The hub's binary version.
    pub version: String,
    /// The hub's sync protocol (sync-correctness.md Section 12). 0 from a hub before it.
    #[serde(default)]
    pub protocol: u32,
    /// The workspaces the CALLER is authorized to sync (diagnostics for setup mistakes).
    pub shared_workspaces: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushResp {
    pub accepted: usize,
    /// (origin_node, origin_seq, reason) - rejections are reported, never silently dropped (P5).
    pub rejected: Vec<(String, u64, String)>,
}

type HandlerError = (StatusCode, String);

/// The hash an allowlist entry keeps of a node's bearer credential: blake3, in hex. The hub never
/// keeps the credential itself (docs/pairing.md PA4).
pub fn bearer_hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

/// Bearer authentication (6a): hash the presented token and look it up on the allowlist. 401 when
/// absent/unknown; the caller then checks per-workspace authorization (403).
fn authenticate(headers: &HeaderMap, allowlist: &[AllowEntry]) -> Result<AllowEntry, HandlerError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".to_string()))?;
    let hash = bearer_hash(token);
    // Compared in constant time: an early-exit string compare tells
    // a caller, by how long a refusal takes, how much of a hash it has right.
    let same = |a: &str, b: &str| {
        a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    };
    allowlist
        .iter()
        .find(|e| same(&e.bearer_hash, &hash))
        .cloned()
        .ok_or((StatusCode::UNAUTHORIZED, "unknown bearer token".to_string()))
}

/// Who sent a sync request: the entry its bearer matched, and the admission it was matched against,
/// so the handler decides with the same view.
#[derive(Clone)]
struct Caller {
    entry: AllowEntry,
    admitted: Admitted,
}

/// Every sync route authenticates here, from the headers, before anything reads the body
/// (sync-correctness.md Section 16, D11): a request with no admitted bearer is answered 401 with
/// its body unread, so a credential is what it takes to make the hub parse anything.
async fn authenticated(
    State(state): State<Arc<ServerState>>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, HandlerError> {
    let admitted = state.peers.admitted();
    let entry = authenticate(req.headers(), &admitted.allowlist)?;
    req.extensions_mut().insert(Caller { entry, admitted });
    Ok(next.run(req).await)
}

fn authorize_workspace(entry: &AllowEntry, workspace: &str) -> Result<(), HandlerError> {
    if entry.shared_workspaces.iter().any(|w| w == workspace) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            format!("workspace {workspace:?} is not shared with node {}", entry.node_id),
        ))
    }
}

fn internal(e: impl std::fmt::Display) -> HandlerError {
    // A store failure is a failure, never an empty result (P5/F12).
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn advertise_handler(
    State(state): State<Arc<ServerState>>,
    Extension(Caller { entry, .. }): Extension<Caller>,
    headers: HeaderMap,
    Json(req): Json<AdvertiseReq>,
) -> Result<Json<AdvertiseResp>, HandlerError> {
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    // Store calls are offloaded so a blocking backend cannot starve the async runtime (F11).
    let vv = tokio::task::spawn_blocking(move || version_vector(store.as_ref(), &req.workspace))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    // Recorded in the peer registry, but NOT streamed: advertise is a metadata handshake that the
    // status loop fires every minute - the activity feed shows knowledge movement, not heartbeats.
    state.seen(&entry.node_id, "advertise", &headers);
    Ok(Json(AdvertiseResp { node_id: state.node.node_id().to_string(), vv }))
}

async fn pull_handler(
    State(state): State<Arc<ServerState>>,
    Extension(Caller { entry, .. }): Extension<Caller>,
    headers: HeaderMap,
    Json(req): Json<PullReq>,
) -> Result<Json<PullResp>, HandlerError> {
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    let node = state.node.clone();
    let ws = req.workspace.clone();
    let (stamped, events) = tokio::task::spawn_blocking(move || {
        // Stamp anything still unstamped before exporting (export-boundary stamping, Phase 2), then
        // serve the delta. The per-node share list is the allowlist entry's workspaces (6c).
        let stamped = node.backfill(store.as_ref(), &req.workspace)?;
        export_delta(store.as_ref(), &req.workspace, &req.since, &entry.shared_workspaces)
            .map(|events| (stamped, events))
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    // Stamping rewrites log rows, and the projection carries their attestations, so a backfill that
    // stamped anything leaves the graph behind the log until it is re-materialized - the same debt
    // an inbound push leaves, paid the same way (crash-recovery.md K2).
    if stamped > 0 {
        if let Some(hook) = state.on_applied.clone() {
            let ws = ws.clone();
            let _ = tokio::task::spawn_blocking(move || hook(&ws)).await;
        }
    }
    // One page, in (origin, seq) order - the order `attestations_since` returns - so the caller's
    // version vector advances exactly as far as the page went and the next page continues from it.
    // Encoded one at a time and only up to the page's end: the delta can be the whole workspace
    // (a fresh peer, or `since` left empty), and the page is at most PAGE_EVENTS of it.
    let limit = req.limit.unwrap_or(PAGE_EVENTS).clamp(1, PAGE_EVENTS);
    let total = events.len();
    let mut page = Vec::new();
    let mut bytes = 0usize;
    for ev in events {
        let ev = serde_json::to_value(ev).expect("an event serializes");
        let size = serde_json::to_vec(&ev).map(|v| v.len()).unwrap_or(0);
        if !page.is_empty() && (page.len() >= limit || bytes + size > PAGE_BYTES) {
            break;
        }
        bytes += size;
        page.push(ev);
    }
    let more = page.len() < total;
    state.seen(&entry.node_id, "pull", &headers);
    state.activity("pull-served", &entry.node_id, &ws, page.len());
    Ok(Json(PullResp { events: page, more }))
}

async fn push_handler(
    State(state): State<Arc<ServerState>>,
    Extension(Caller { entry, admitted }): Extension<Caller>,
    headers: HeaderMap,
    Json(req): Json<PushReq>,
) -> Result<Json<PushResp>, HandlerError> {
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    let node = state.node.clone();
    let keys = admitted.origin_keys;
    let ws = req.workspace.clone();
    // Who may write into this workspace through this push (sync-correctness.md Section 8): the
    // pusher, every node granted the workspace here, and this hub. A peer granted A cannot bring in
    // events of an origin this hub admits only to B.
    let admit: std::collections::BTreeSet<String> = admitted
        .allowlist
        .iter()
        .filter(|e| e.node_id == entry.node_id || e.shared_workspaces.contains(&req.workspace))
        .map(|e| e.node_id.clone())
        .chain([state.node.node_id().to_string()])
        .collect();
    let report = tokio::task::spawn_blocking(move || {
        let mut vv = VersionVector::default();
        node.apply_wire(store.as_ref(), &req.workspace, req.events, &keys, Some(&admit), &mut vv)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    state.seen(&entry.node_id, "push", &headers);
    state.activity("push-received", &entry.node_id, &ws, report.accepted);
    // Re-materialize after new events land (Prop C) - injected by the wiring layer.
    if report.accepted > 0 {
        if let Some(hook) = state.on_applied.clone() {
            let _ = tokio::task::spawn_blocking(move || hook(&ws)).await;
        }
    }
    Ok(Json(PushResp {
        accepted: report.accepted,
        rejected: report
            .rejected
            .into_iter()
            .map(|r| (r.origin_node, r.origin_seq, format!("{:?}", r.reason)))
            .collect(),
    }))
}

/// Federated retrieval (P17 second door, honored): an authenticated peer searches THIS node's
/// ontology - gated by the same per-node workspace authorization as sync, answered from the node's
/// retrieval surface, and streamed to the activity feed like any other hit.
async fn search_handler(
    State(state): State<Arc<ServerState>>,
    Extension(Caller { entry, .. }): Extension<Caller>,
    headers: HeaderMap,
    Json(req): Json<SearchReq>,
) -> Result<Json<SearchResp>, HandlerError> {
    authorize_workspace(&entry, &req.workspace)?;
    let limit = req.limit.min(100);
    let ws = req.workspace.clone();
    let (hits, mode) = match state.on_search.clone() {
        Some(hook) => {
            let q = req.query.clone();
            tokio::task::spawn_blocking(move || hook(&ws, &q, limit))
                .await
                .map_err(internal)?
                .map_err(internal)?
        }
        None => {
            let store = state.store.clone();
            let q = req.query.clone();
            let hits = tokio::task::spawn_blocking(move || store.search(&q, Some(&ws), limit))
                .await
                .map_err(internal)?
                .map_err(internal)?;
            (hits, "keyword".to_string())
        }
    };
    state.seen(&entry.node_id, "search", &headers);
    state.activity("search-served", &entry.node_id, &req.workspace, hits.len());
    Ok(Json(SearchResp { mode, hits }))
}

/// Health check (liveness): an authenticated no-op that registers the peer as seen and answers
/// with the hub's identity/version plus the caller's authorized workspaces - so a spoke can verify
/// connectivity, auth, AND authorization at startup in one round trip.
async fn ping_handler(
    State(state): State<Arc<ServerState>>,
    Extension(Caller { entry, .. }): Extension<Caller>,
    headers: HeaderMap,
) -> Result<Json<PingResp>, HandlerError> {
    // Registry only (see advertise) - health checks are heartbeats, not knowledge movement.
    state.seen(&entry.node_id, "ping", &headers);
    Ok(Json(PingResp {
        node_id: state.node.node_id().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol: PROTOCOL,
        shared_workspaces: entry.shared_workspaces.clone(),
    }))
}

/// The sync API router - exposed so the daemon (Phase 4 wiring) can mount it.
pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/sync/ping", post(ping_handler))
        .route("/sync/advertise", post(advertise_handler))
        .route("/sync/pull", post(pull_handler))
        .route("/sync/push", post(push_handler))
        .route("/sync/search", post(search_handler))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), authenticated))
        .layer(axum::extract::DefaultBodyLimit::max(BODY_LIMIT))
        .layer(axum::middleware::from_fn(bounded))
        .with_state(state)
}

/// Every sync response says which protocol and release answered (Section 12), and a request that
/// runs past [`CALL_TIMEOUT`] is answered 504 (Section 11).
async fn bounded(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut response = match tokio::time::timeout(CALL_TIMEOUT, next.run(req)).await {
        Ok(response) => response,
        Err(_) => (StatusCode::GATEWAY_TIMEOUT, "the sync request ran past its time limit")
            .into_response(),
    };
    let h = response.headers_mut();
    h.insert(PROTOCOL_HEADER, axum::http::HeaderValue::from(PROTOCOL));
    h.insert(RELEASE_HEADER, axum::http::HeaderValue::from_static(env!("CARGO_PKG_VERSION")));
    response
}

/// TLS material for the in-process rustls termination (F10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPaths {
    pub cert_pem: std::path::PathBuf,
    pub key_pem: std::path::PathBuf,
}

/// Serves the sync API: [`bind`], then [`Bound::run`]. Loopback may run plain (a local trust
/// surface); a non-loopback bind is refused unless TLS material is provided AND someone is admitted
/// (F10).
pub async fn serve(
    store: Arc<dyn AssertionStore>,
    node: Arc<SyncNode>,
    listen: SocketAddr,
    tls: Option<TlsPaths>,
    peers: Arc<PeerDirectory>,
    hooks: Hooks,
) -> Result<(), TransportError> {
    bind(store, node, listen, tls, peers, hooks).await?.run().await
}

/// The sync API with every way it can fail to start already tried: the bind rule passed, the TLS
/// material loaded, the address bound. What is left is accepting connections ([`Bound::run`]).
pub struct Bound {
    listener: std::net::TcpListener,
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
    app: Router,
}

/// Prepares the sync API to serve, without serving it. A daemon awaits this and then runs the
/// result in the background, so a certificate it cannot read, a port already taken or nobody
/// admitted stops its startup with the reason (P5), instead of ending a spawned task with a log
/// line while the daemon carries on as a hub with no listener.
pub async fn bind(
    store: Arc<dyn AssertionStore>,
    node: Arc<SyncNode>,
    listen: SocketAddr,
    tls: Option<TlsPaths>,
    peers: Arc<PeerDirectory>,
    hooks: Hooks,
) -> Result<Bound, TransportError> {
    // The bind rule is about STARTING an unauthenticated surface, so it is checked against admission
    // as it stands now (F10). Admission may shrink later, including to empty - that rejects every
    // request rather than admitting anyone, which serves the same end (see [`PeerDirectory`]).
    let extra_admitted = hooks.extra.as_ref().map_or(0, |x| x.admitted);
    validate_bind(&listen, tls.is_some(), peers.admitted().allowlist.len() + extra_admitted)?;
    let mut state = ServerState::with_directory(store, node, peers);
    if let Some(hook) = hooks.on_applied {
        state = state.with_on_applied(hook);
    }
    if let Some(hook) = hooks.on_activity {
        state = state.with_on_activity(hook);
    }
    if let Some(hook) = hooks.on_search {
        state = state.with_on_search(hook);
    }
    if let Some(r) = hooks.peer_registry {
        state = state.with_peers(r);
    }
    let state = Arc::new(state);
    let app = match hooks.extra {
        Some(extra) => router(state).merge(extra.router),
        None => router(state),
    };
    let tls = match tls {
        Some(paths) => {
            // Pin the process-level CryptoProvider: with both aws-lc-rs (axum-server) and ring
            // (reqwest) compiled in, rustls refuses to auto-select. Idempotent - a second install
            // attempt is fine to ignore.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let cfg =
                axum_server::tls_rustls::RustlsConfig::from_pem_file(paths.cert_pem, paths.key_pem)
                    .await
                    .map_err(|e| TransportError::Tls(e.to_string()))?;
            Some(cfg)
        }
        None => None,
    };
    let listener = std::net::TcpListener::bind(listen)?;
    listener.set_nonblocking(true)?; // what tokio requires of a listener it adopts
    tracing::info!(%listen, tls = tls.is_some(), "sync API listening");
    Ok(Bound { listener, tls, app })
}

impl Bound {
    /// Where it is bound - the port the OS chose, for an address that named port 0.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accepts connections until the server stops.
    pub async fn run(self) -> Result<(), TransportError> {
        let service = self.app.into_make_service();
        match self.tls {
            Some(cfg) => {
                axum_server::from_tcp_rustls(self.listener, cfg)?.serve(service).await?;
            }
            None => axum_server::from_tcp(self.listener)?.serve(service).await?,
        }
        Ok(())
    }
}

/// Result of one client sync round against a server.
#[derive(Debug, Default, Serialize)]
pub struct SyncSummary {
    pub pushed: usize,
    pub pulled: usize,
    pub rejected_by_server: usize,
    pub rejected_locally: usize,
}

/// The sync client (reqwest): advertise -> push my surplus -> pull my deficit -> apply locally.
pub struct SyncClient {
    base: String,
    token: String,
    http: reqwest::Client,
    /// Keys a hub vouched for at a join, by origin, and the last sequence each verifies in each
    /// workspace (docs/pairing.md Section 4.3.1).
    vouched: BTreeMap<String, BTreeMap<String, u64>>,
}

impl SyncClient {
    /// A client that verifies the host against the system's roots. Every client verifies: there is
    /// no setting that accepts any certificate (sync-correctness.md Section 10).
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, TransportError> {
        Self::build(base_url, token, None)
    }

    /// The client for one configured host: its credential, and the certificate it trusts for it.
    pub fn for_link(link: &crate::ServerLink) -> Result<Self, TransportError> {
        Self::build(&link.url, &link.auth_token, link.ca_pem.as_deref())
    }

    fn build(
        base_url: impl Into<String>,
        token: impl Into<String>,
        ca_pem: Option<&[u8]>,
    ) -> Result<Self, TransportError> {
        // Same CryptoProvider pin as the server side (idempotent) - the client dials HTTPS hubs.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder();
        // A named certificate is how a self-signed hub is trusted without trusting every hub.
        if let Some(pem) = ca_pem {
            builder = builder.use_preconfigured_tls(tls_trusting(pem)?);
        }
        let http = builder.timeout(CALL_TIMEOUT).build()?;
        Ok(Self {
            base: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
            vouched: BTreeMap::new(),
        })
    }

    /// Verifies the vouched origins' events only up to their bound in each workspace, and none in
    /// a workspace without one (docs/pairing.md Section 4.3.1). Their keys must also be among the
    /// origin keys a sync is given; without this, those keys would verify unbounded.
    pub fn with_vouched(mut self, vouched: BTreeMap<String, BTreeMap<String, u64>>) -> Self {
        self.vouched = vouched;
        self
    }

    async fn call<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        req: &Req,
    ) -> Result<Resp, TransportError> {
        self.call_for(path, req, None).await
    }

    /// A workspace request, saying no to serving that workspace to a 0.10 hub's principals
    /// ([`SERVE_HEADER`]).
    async fn call_for<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        req: &Req,
        workspace: Option<&str>,
    ) -> Result<Resp, TransportError> {
        let mut builder = self
            .http
            .post(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .header(PROTOCOL_HEADER, PROTOCOL)
            .header(RELEASE_HEADER, env!("CARGO_PKG_VERSION"))
            .json(req);
        if workspace.is_some() {
            builder = builder.header(SERVE_HEADER, "no");
        }
        let resp = builder.send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(TransportError::Remote {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    }

    pub async fn advertise(&self, workspace: &str) -> Result<AdvertiseResp, TransportError> {
        self.call_for(
            "/sync/advertise",
            &AdvertiseReq { workspace: workspace.into() },
            Some(workspace),
        )
        .await
    }

    pub async fn pull(
        &self,
        workspace: &str,
        since: &VersionVector,
        limit: usize,
    ) -> Result<PullResp, TransportError> {
        let req = PullReq { workspace: workspace.into(), since: since.clone(), limit: Some(limit) };
        self.call_for("/sync/pull", &req, Some(workspace)).await
    }

    /// Pulls page after page of what the host holds beyond `since`, handing each page to `apply`
    /// (sync-correctness.md Section 11). A stream with a rejection that holds (Section 16) in a page is
    /// held: the request claims it whole for the rest of this round, so its later events are neither applied past the
    /// rejected one nor sent again and again in place of the other streams. The next round asks from
    /// the store's own version vector, which is below the rejected event, and offers it again.
    pub async fn pull_all<F, Fut>(
        &self,
        workspace: &str,
        mut since: VersionVector,
        mut apply: F,
    ) -> Result<ApplyReport, TransportError>
    where
        F: FnMut(Vec<serde_json::Value>) -> Fut,
        Fut: std::future::Future<Output = Result<ApplyReport, TransportError>>,
    {
        let mut total = ApplyReport::default();
        loop {
            let page = self.pull(workspace, &since, PAGE_EVENTS).await?;
            if page.events.is_empty() {
                break;
            }
            let streams: Vec<(String, u64)> = page.events.iter().map(stream_of).collect();
            let report = apply(page.events).await?;
            let held: std::collections::HashSet<&str> = report
                .rejected
                .iter()
                .filter(|r| r.reason.holds())
                .map(|r| r.origin_node.as_str())
                .collect();
            let before = since.clone();
            for (origin, seq) in streams.iter().filter(|(o, _)| !o.is_empty()) {
                let to = if held.contains(origin.as_str()) { u64::MAX } else { *seq };
                since.advance(origin, workspace, to);
            }
            total.accepted += report.accepted;
            total.rejected.extend(report.rejected);
            // A host that says there is more but sent nothing new is not asked again this round.
            if !page.more || since == before {
                break;
            }
        }
        Ok(total)
    }

    /// Pushes `events` in batches (sync-correctness.md Section 11), in (origin, seq) order. A stream
    /// with a rejection that holds at the host in one batch is not sent further this round: the host's version vector stays
    /// below the rejected event, so it and everything behind it are offered again next round.
    pub async fn push(
        &self,
        workspace: &str,
        mut events: Vec<AttestationEvent>,
    ) -> Result<PushResp, TransportError> {
        events.sort_by_cached_key(|e| {
            e.attestation.sync.as_ref().map(|m| (m.origin_node.clone(), m.origin_seq))
        });
        let mut total = PushResp { accepted: 0, rejected: Vec::new() };
        let mut held: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut batch: Vec<serde_json::Value> = Vec::new();
        let mut bytes = 0usize;
        for ev in to_wire(events) {
            if held.contains(&stream_of(&ev).0) {
                continue;
            }
            let size = serde_json::to_vec(&ev).map(|v| v.len()).unwrap_or(0);
            if !batch.is_empty() && (batch.len() >= BATCH_EVENTS || bytes + size > BATCH_BYTES) {
                let sent = std::mem::take(&mut batch);
                self.push_batch(workspace, sent, &mut total, &mut held).await?;
                bytes = 0;
                if held.contains(&stream_of(&ev).0) {
                    continue;
                }
            }
            bytes += size;
            batch.push(ev);
        }
        if !batch.is_empty() {
            self.push_batch(workspace, batch, &mut total, &mut held).await?;
        }
        Ok(total)
    }

    async fn push_batch(
        &self,
        workspace: &str,
        events: Vec<serde_json::Value>,
        total: &mut PushResp,
        held: &mut std::collections::HashSet<String>,
    ) -> Result<(), TransportError> {
        let req = PushReq { workspace: workspace.into(), events };
        let resp: PushResp = self.call_for("/sync/push", &req, Some(workspace)).await?;
        held.extend(
            resp.rejected
                .iter()
                .filter(|r| holds_at_the_host(&r.2))
                .map(|r| r.0.clone())
                .filter(|o| !o.is_empty()),
        );
        total.accepted += resp.accepted;
        total.rejected.extend(resp.rejected);
        Ok(())
    }

    /// Health check: verifies connectivity, auth, and per-workspace authorization in one call.
    pub async fn ping(&self) -> Result<PingResp, TransportError> {
        self.call("/sync/ping", &serde_json::json!({})).await
    }

    /// Federated retrieval: search the SERVER's ontology (its retrieval surface, mode-labeled). Results
    /// are remote ids - pull the workspace to materialize them locally before traverse/get_entity.
    pub async fn search(
        &self,
        workspace: &str,
        query: &str,
        limit: usize,
    ) -> Result<SearchResp, TransportError> {
        self.call(
            "/sync/search",
            &SearchReq { workspace: workspace.into(), query: query.into(), limit },
        )
        .await
    }

    /// One full sync round for a workspace: learn what the server holds, backfill-stamp local
    /// knowledge, push what the server lacks (per its advertised VV), pull what we lack, and apply it
    /// (verify -> CAS -> VV). Re-materialization (engine reproject) is the caller's follow-up -
    /// transport moves the log.
    ///
    /// The server is asked first so its copy of this node's own stream floors the seq counter before
    /// anything is stamped (sync-correctness.md Section 5): a node whose store lost stamps it had
    /// already sent continues past them instead of issuing their seqs again.
    pub async fn sync_workspace(
        &self,
        store: &Arc<dyn AssertionStore>,
        node: &SyncNode,
        workspace: &str,
        share_workspaces: &[String],
        origin_keys: &BTreeMap<String, String>,
    ) -> Result<SyncSummary, TransportError> {
        let remote = self.advertise(workspace).await?;
        node.floor_seq(store.as_ref(), workspace, remote.vv.get(node.node_id(), workspace))?;
        node.backfill(store.as_ref(), workspace)?;
        let mine = version_vector(store.as_ref(), workspace)?;
        let surplus = export_delta(store.as_ref(), workspace, &remote.vv, share_workspaces)?;
        let mut summary = SyncSummary::default();
        if !surplus.is_empty() {
            let resp = self.push(workspace, surplus).await?;
            summary.pushed = resp.accepted;
            summary.rejected_by_server = resp.rejected.len();
        }
        let report = self
            .pull_all(workspace, mine, |events| {
                let mut vv = VersionVector::default();
                std::future::ready(
                    node.apply_wire_bounded(
                        store.as_ref(),
                        workspace,
                        events,
                        origin_keys,
                        &crate::bounds_in(&self.vouched, workspace),
                        None,
                        &mut vv,
                    )
                    .map_err(TransportError::from),
                )
            })
            .await?;
        summary.pulled = report.accepted;
        summary.rejected_locally = report.rejected.len();
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RejectReason;
    use supragnosis_core::{NodeIdentity, Observation, Provenance};
    use supragnosis_store::InMemoryStore;

    fn prov(ws: &str, at: u64) -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: ws.into(),
            source_ref: None,
            observed_at: at,
            confidence: None,
            trust_tier: Default::default(),
            sync: None,
        }
    }

    fn entry(node: &SyncNode, token: &str, ws: &[&str]) -> AllowEntry {
        AllowEntry {
            node_id: node.node_id().to_string(),
            public_key_hex: node.public_key_hex(),
            bearer_hash: blake3::hash(token.as_bytes()).to_hex().to_string(),
            shared_workspaces: ws.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Admission is consulted per request, so removing a peer takes effect on the running server.
    ///
    /// The point is not that a `Vec` can be swapped - it is that the derived key directory moves with
    /// it. A peer removed from the allowlist has to stop being verifiable too: if `origin_keys` kept
    /// its entry, the peer would be turned away at the door and then have its relayed events accepted
    /// anyway, which is the drift `PeerDirectory` exists to make unrepresentable.
    /// A node's own id in its own allowlist is dropped, through BOTH construction paths.
    ///
    /// The entry is a live credential - admission matches a bearer hash, not a node id - so dropping
    /// it narrows, which is why it can happen without asking (F14, 6a). It is filtered in `derive`
    /// rather than at the call site precisely so `replace` cannot let it back in.
    #[test]
    fn a_node_is_never_its_own_peer_through_either_path() {
        let me = SyncNode::new(NodeIdentity::from_secret_bytes([5u8; 32]));
        let peer = SyncNode::new(NodeIdentity::from_secret_bytes([6u8; 32]));
        let mine = entry(&me, "self-token", &["ws"]);
        let theirs = entry(&peer, "peer-token", &["ws"]);

        let dir = PeerDirectory::new(
            vec![mine.clone(), theirs.clone()],
            me.node_id(),
            &me.public_key_hex(),
        );
        let ids: Vec<String> = dir.admitted().allowlist.iter().map(|e| e.node_id.clone()).collect();
        assert_eq!(ids, [peer.node_id().to_string()], "the self entry is not admitted at startup");

        // ...and a later replace cannot smuggle it back in.
        dir.replace(vec![mine, theirs]);
        let ids: Vec<String> = dir.admitted().allowlist.iter().map(|e| e.node_id.clone()).collect();
        assert_eq!(ids, [peer.node_id().to_string()], "nor through a runtime replace");
    }

    #[tokio::test]
    async fn admission_changes_take_effect_without_a_restart() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let peer = SyncNode::new(NodeIdentity::from_secret_bytes([8u8; 32]));
        hub_store
            .add_observation(Observation::new("hub fact".into(), prov("ws", 1)))
            .unwrap();

        let state = Arc::new(ServerState::new(
            hub_store.clone(),
            hub_node.clone(),
            vec![entry(&peer, "token-p", &["ws"])],
        ));
        let peers = state.peers();
        assert!(
            peers.admitted().origin_keys.contains_key(peer.node_id()),
            "an admitted peer's key is in the verification directory"
        );

        let addr = spawn_server(state).await;
        let client = SyncClient::new(format!("http://{addr}"), "token-p").unwrap();
        assert_eq!(
            client
                .pull("ws", &VersionVector::default(), PAGE_EVENTS)
                .await
                .unwrap()
                .events
                .len(),
            1,
            "the admitted peer can pull before the change"
        );

        // Revoke, on the running server.
        peers.replace(Vec::new());

        let err = client
            .pull("ws", &VersionVector::default(), PAGE_EVENTS)
            .await
            .expect_err("must be refused now");
        assert!(
            err.to_string().contains("401")
                || err.to_string().to_lowercase().contains("unauthorized"),
            "a removed peer is turned away at the door: {err}"
        );
        assert!(
            !peers.admitted().origin_keys.contains_key(peer.node_id()),
            "and its key left the verification directory with it - the derived half cannot lag"
        );
        assert!(
            peers.admitted().origin_keys.contains_key(hub_node.node_id()),
            "the node's own key survives an allowlist that no longer mentions anyone"
        );

        // Re-admitting restores both halves, so the change is a swap and not a one-way latch.
        peers.replace(vec![entry(&peer, "token-p", &["ws"])]);
        assert_eq!(
            client
                .pull("ws", &VersionVector::default(), PAGE_EVENTS)
                .await
                .unwrap()
                .events
                .len(),
            1
        );
    }

    /// A self-signed certificate for localhost, written to a fresh temp dir. Marked as a CA, as
    /// `openssl req -x509` does by default - the shape a self-signed hub's certificate usually has,
    /// and the one a chain-only verifier refuses as a server's own.
    fn minted_tls() -> (rcgen::CertifiedKey<rcgen::KeyPair>, TlsPaths) {
        let mut params =
            rcgen::CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])
                .expect("params");
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let signing_key = rcgen::KeyPair::generate().expect("key");
        let cert = params.self_signed(&signing_key).expect("mint a self-signed certificate");
        // The clock alone does not make the name unique: macOS reads it in microseconds, so two
        // tests minting at once got one directory, and whichever finished first removed the other's
        // certificate while it was still being read - its listener never started (refused
        // connections) or failed on the PEM instead of at the bind. The counter makes each call's
        // directory its own.
        static MINTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = MINTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir()
            .join(format!("supragnosis-tls-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tls = TlsPaths { cert_pem: dir.join("cert.pem"), key_pem: dir.join("key.pem") };
        std::fs::write(&tls.cert_pem, cert.pem()).unwrap();
        std::fs::write(&tls.key_pem, signing_key.serialize_pem()).unwrap();
        (rcgen::CertifiedKey { cert, signing_key }, tls)
    }

    /// F10's "only with TLS enabled", driven end to end rather than read off a flag.
    ///
    /// `bind_guard_enforces_f10` proves the refusal: a non-loopback bind with no TLS material does
    /// not start. Nothing proved the other half - that with material, the listener actually speaks
    /// TLS - and every other test here serves plain axum on loopback, so `bind_rustls`,
    /// `RustlsConfig::from_pem_file` and the CryptoProvider pin ran in no test at all. An axum-server
    /// major bump passed this suite while that path went unexercised; it was checked by hand.
    ///
    /// Loopback, because binding a public address in a test is not something a test should do. The
    /// branch is the same one a non-loopback bind takes: `serve` matches on `tls`, not on the address.
    #[tokio::test]
    async fn tls_listener_serves_https_and_refuses_plaintext() {
        let (minted, tls) = minted_tls();
        let dir = tls.cert_pem.parent().expect("the temp dir").to_path_buf();

        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let spoke = SyncNode::new(NodeIdentity::from_secret_bytes([8u8; 32]));
        let peers = Arc::new(PeerDirectory::new(
            vec![entry(&spoke, "tok", &["w"])],
            hub.node_id(),
            &hub.public_key_hex(),
        ));
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        // `serve` binds the address itself, so reserve a free port and hand it over.
        let addr = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let server =
            tokio::spawn(serve(store, hub.clone(), addr, Some(tls), peers, Hooks::default()));

        // The hub's own certificate, named: how a self-signed hub is trusted without trusting every
        // certificate (sync-correctness.md Section 10). There is no client that skips verification.
        let link = |token: &str, pem: String| crate::ServerLink {
            url: format!("https://{addr}"),
            auth_token: token.into(),
            ca_pem: Some(pem.into_bytes()),
        };
        let named = |token: &str| SyncClient::for_link(&link(token, minted.cert.pem())).unwrap();

        // HTTPS with an admitted bearer: answered, by this hub, with the caller's grant.
        let client = named("tok");
        let mut ping = client.ping().await;
        for _ in 0..50 {
            if ping.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            ping = client.ping().await;
        }
        let ping = ping.expect("an admitted peer is answered over TLS");
        assert_eq!(ping.node_id, hub.node_id());
        assert_eq!(ping.shared_workspaces, vec!["w".to_string()]);

        // TLS is a transport, not an authorization: the bearer check still runs behind it.
        assert!(
            matches!(named("nope").ping().await, Err(TransportError::Remote { status: 401, .. })),
            "an unknown bearer is refused over TLS too"
        );

        // The port speaks TLS only - a plaintext request gets no HTTP answer at all.
        let plain = SyncClient::new(format!("http://{addr}"), "tok").unwrap();
        assert!(
            matches!(plain.ping().await, Err(TransportError::Http(_))),
            "plaintext must not be served on the TLS port"
        );

        // Without the hub's certificate named, the system roots do not know this hub: refused.
        let strict = SyncClient::new(format!("https://{addr}"), "tok").unwrap();
        assert!(
            matches!(strict.ping().await, Err(TransportError::Http(_))),
            "a verifying client must refuse a self-signed hub it was not told about"
        );

        // And a different certificate is not this hub's.
        let other = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let wrong = SyncClient::for_link(&link("tok", other.cert.pem())).unwrap();
        assert!(
            matches!(wrong.ping().await, Err(TransportError::Http(_))),
            "a different certificate is not this hub's"
        );

        server.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Spawns the sync API on an ephemeral loopback port (plain HTTP - the local trust surface;
    /// TLS material is exercised in production paths, the F10 guard below pins the policy).
    async fn spawn_server(state: Arc<ServerState>) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });
        addr
    }

    #[test]
    fn bind_guard_enforces_f10() {
        let lo: SocketAddr = "127.0.0.1:7420".parse().unwrap();
        let pub_addr: SocketAddr = "0.0.0.0:7420".parse().unwrap();
        assert!(validate_bind(&lo, false, 0).is_ok(), "loopback needs neither TLS nor allowlist");
        assert!(validate_bind(&pub_addr, false, 1).is_err(), "non-loopback without TLS refused");
        assert!(
            validate_bind(&pub_addr, true, 0).is_err(),
            "non-loopback with empty allowlist refused"
        );
        assert!(validate_bind(&pub_addr, true, 1).is_ok());
    }

    /// F10 / pairing.md PA1: a hub may admit no node yet and hold an open invitation, and the bind
    /// rule counts both. `bind_guard_enforces_f10` drives `validate_bind` with a count; this drives
    /// `serve`, where the count is assembled (the allowlist plus what the extra surface admits), on
    /// an address off loopback. It is TEST-NET-1, assigned to no machine, so passing the guard shows
    /// as failing at the bind itself and nothing listens on a public address in a test.
    #[tokio::test]
    async fn an_admission_beside_the_allowlist_passes_the_bind_guard() {
        let (_minted, tls) = minted_tls();
        let dir = tls.cert_pem.parent().expect("the temp dir").to_path_buf();
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let off_loopback: SocketAddr = "192.0.2.1:7420".parse().unwrap();
        let start = |admitted: usize| {
            let no_nodes = PeerDirectory::new(vec![], hub.node_id(), &hub.public_key_hex());
            let extra = ExtraSurface { router: Router::new(), admitted };
            let hooks = Hooks { extra: Some(extra), ..Hooks::default() };
            serve(
                store.clone(),
                hub.clone(),
                off_loopback,
                Some(tls.clone()),
                no_nodes.into(),
                hooks,
            )
        };
        assert!(
            matches!(start(0).await, Err(TransportError::Bind { .. })),
            "no node and no invitation: nobody is admitted, so the listener does not start"
        );
        let one = start(1).await;
        assert!(
            matches!(one, Err(TransportError::Io(_))),
            "an open invitation is someone admitted - past the guard, refused only by the OS: {one:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A listener that cannot start fails `bind`, which the daemon awaits, rather than `run`, which
    /// it spawns: a certificate it cannot read and a port already taken both come back as errors
    /// here, where they used to end a background task with a log line while the daemon went on
    /// as a hub with nothing listening.
    #[tokio::test]
    async fn a_listener_that_cannot_start_fails_bind() {
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let attempt = |listen: SocketAddr, tls: Option<TlsPaths>| {
            let peers = PeerDirectory::new(vec![], hub.node_id(), &hub.public_key_hex());
            bind(store.clone(), hub.clone(), listen, tls, peers.into(), Hooks::default())
        };
        let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let missing = TlsPaths {
            cert_pem: "/nonexistent/cert.pem".into(),
            key_pem: "/nonexistent/key.pem".into(),
        };
        assert!(matches!(attempt(loopback, Some(missing)).await, Err(TransportError::Tls(_))));

        let taken = std::net::TcpListener::bind(loopback).unwrap();
        let busy = taken.local_addr().unwrap();
        assert!(matches!(attempt(busy, None).await, Err(TransportError::Io(_))));
        drop(taken);
        let bound = attempt(loopback, None).await.expect("a free loopback port binds");
        assert_ne!(bound.local_addr().unwrap().port(), 0, "and it is bound before it runs");
    }

    /// `ping` answers with the CALLER's grants, never the host's inventory.
    ///
    /// This is the premise the negotiated surface rests its P17 argument on (federation.md 6e, F21
    /// clause 1): advertising discloses nothing because a peer only learns the set it was already
    /// granted. Return the union, or every admitted workspace, and the argument inverts - the
    /// response would reveal the existence of workspaces the caller may not read.
    #[tokio::test]
    async fn ping_answers_with_the_callers_own_grants_and_not_the_hosts_inventory() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let a = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let b = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
        let allow = vec![entry(&a, "tok-a", &["ws-a"]), entry(&b, "tok-b", &["ws-b", "ws-shared"])];
        let addr = spawn_server(Arc::new(ServerState::new(hub_store, hub, allow))).await;
        let base = format!("http://{addr}");

        let pa = SyncClient::new(&base, "tok-a").unwrap().ping().await.unwrap();
        assert_eq!(pa.shared_workspaces, ["ws-a"], "A learns only its own grant");

        let pb = SyncClient::new(&base, "tok-b").unwrap().ping().await.unwrap();
        assert_eq!(pb.shared_workspaces, ["ws-b", "ws-shared"], "B learns only its own");

        // Neither answer leaks the other's workspaces, which is the whole disclosure question.
        assert!(!pa.shared_workspaces.iter().any(|w| w.starts_with("ws-b")));
        assert!(!pb.shared_workspaces.contains(&"ws-a".to_string()));
    }

    #[tokio::test]
    async fn wire_auth_rejects_bad_token_and_unshared_workspace() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let client_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let allow = vec![entry(&client_node, "secret-token", &["ws"])];
        let addr = spawn_server(Arc::new(ServerState::new(hub_store, hub, allow))).await;
        let base = format!("http://{addr}");

        // Wrong token -> 401 (F6 wire layer).
        let bad = SyncClient::new(&base, "wrong-token").unwrap();
        match bad.advertise("ws").await {
            Err(TransportError::Remote { status: 401, .. }) => {}
            other => panic!("expected 401, got {other:?}"),
        }
        // Right token, unshared workspace -> 403 (per-node authorization, 6c).
        let good = SyncClient::new(&base, "secret-token").unwrap();
        match good.advertise("private-ws").await {
            Err(TransportError::Remote { status: 403, .. }) => {}
            other => panic!("expected 403, got {other:?}"),
        }
        // A federated search is authorized the same way: an unshared workspace is refused too.
        match good.search("private-ws", "anything", 5).await {
            Err(TransportError::Remote { status: 403, .. }) => {}
            other => panic!("expected 403 for a search, got {other:?}"),
        }
        // Right token, shared workspace -> OK.
        assert!(good.advertise("ws").await.is_ok());
    }

    /// Section 16 (D11): the credential is checked before the body is read. A body that is not even
    /// JSON is refused 401 without one, on every route, where reading it first answered 400; with the
    /// credential, the same body is read and refused for what it is.
    #[tokio::test]
    async fn a_request_without_a_credential_is_refused_before_its_body_is_read() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let client_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let allow = vec![entry(&client_node, "secret-token", &["ws"])];
        let addr = spawn_server(Arc::new(ServerState::new(hub_store, hub, allow))).await;
        let http = reqwest::Client::new();
        let send = |route: &str, bearer: Option<&str>| {
            let mut req = http
                .post(format!("http://{addr}{route}"))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body("{not json");
            if let Some(b) = bearer {
                req = req.bearer_auth(b);
            }
            req.send()
        };
        for route in ["/sync/ping", "/sync/advertise", "/sync/pull", "/sync/push", "/sync/search"] {
            for bearer in [None, Some("wrong-token")] {
                let status = send(route, bearer).await.unwrap().status();
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{route} with {bearer:?}");
            }
        }
        let status = send("/sync/push", Some("secret-token")).await.unwrap().status();
        assert_eq!(status, StatusCode::BAD_REQUEST, "an admitted caller's body is read");
    }

    /// Section 16 (D13): a pushing client reads which rejections hold their stream at the host from
    /// the reasons the host reports, so it keeps sending a stream whose rejection was dropped and
    /// stops one that is held - the same split the host applies.
    #[test]
    fn a_client_reads_which_rejections_hold_as_its_host_applies_them() {
        for reason in [
            RejectReason::Unstamped,
            RejectReason::UnknownOrigin,
            RejectReason::BadSignature,
            RejectReason::WorkspaceMismatch,
            RejectReason::Malformed("confidence 5".into()),
            RejectReason::Undecodable("unknown variant".into()),
            RejectReason::Held(3),
            RejectReason::OriginNotAdmitted,
        ] {
            assert_eq!(holds_at_the_host(&format!("{reason:?}")), reason.holds(), "{reason:?}");
        }
    }

    /// A store whose every call fails, as a disk that went away does - the fault-injecting adapter.
    struct FailingStore;

    fn gone() -> supragnosis_core::StoreError {
        supragnosis_core::StoreError::Backend("the disk went away".into())
    }

    impl AssertionStore for FailingStore {
        fn add_observation(&self, _: Observation) -> Result<(), supragnosis_core::StoreError> {
            Err(gone())
        }
        fn get_observation(
            &self,
            _: &str,
        ) -> Result<Option<Observation>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn get_entity(
            &self,
            _: &str,
        ) -> Result<Option<supragnosis_core::Entity>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn relations_of(
            &self,
            _: &str,
        ) -> Result<Vec<supragnosis_core::Relation>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn search(
            &self,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Vec<supragnosis_core::SearchHit>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn traverse(
            &self,
            _: &str,
            _: usize,
            _: usize,
        ) -> Result<Vec<supragnosis_core::TraverseHit>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn all_entities(
            &self,
            _: Option<&str>,
        ) -> Result<Vec<supragnosis_core::Entity>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn all_relations(
            &self,
            _: Option<&str>,
        ) -> Result<Vec<supragnosis_core::Relation>, supragnosis_core::StoreError> {
            Err(gone())
        }
        fn all_observations(
            &self,
            _: Option<&str>,
        ) -> Result<Vec<Observation>, supragnosis_core::StoreError> {
            Err(gone())
        }
    }

    /// F12 (P5): a storage failure is reported as a failure, never as "empty" or "converged". A hub
    /// whose store fails answers advertise, pull and push with a 500 carrying the store's error -
    /// not an empty version vector, an empty page or an accepted batch - and a round run from a
    /// node whose own store fails ends in that error, never in a summary that says nothing moved.
    #[tokio::test]
    async fn a_store_failure_is_reported_as_a_failure() {
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let client_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let allow = vec![entry(&client_node, "tok", &["ws"])];
        let failing: Arc<dyn AssertionStore> = Arc::new(FailingStore);
        let addr = spawn_server(Arc::new(ServerState::new(failing.clone(), hub, allow))).await;
        let client = SyncClient::new(format!("http://{addr}"), "tok").unwrap();
        let failed = |what: &str, r: Result<String, TransportError>| match r {
            Err(TransportError::Remote { status: 500, body }) => {
                assert!(body.contains("the disk went away"), "{what}: {body}")
            }
            other => panic!("{what}: a failing store answered {other:?}"),
        };
        failed("advertise", client.advertise("ws").await.map(|r| format!("{:?}", r.vv)));
        failed(
            "pull",
            client
                .pull("ws", &VersionVector::default(), 10)
                .await
                .map(|r| format!("{}", r.events.len())),
        );

        // A batch a healthy node would accept, refused because the hub cannot store it.
        let mine: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let prov = Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: None,
            observed_at: 1,
            confidence: None,
            trust_tier: Default::default(),
            sync: None,
        };
        mine.add_observation(Observation::new("a fact".into(), prov)).unwrap();
        client_node.backfill(mine.as_ref(), "ws").unwrap();
        let batch =
            export_delta(mine.as_ref(), "ws", &VersionVector::default(), &["ws".into()]).unwrap();
        failed("push", client.push("ws", batch).await.map(|r| format!("{r:?}")));

        // A round from a node whose own store fails: the error, not "nothing to sync".
        let healthy_hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([8u8; 32])));
        let allow = vec![entry(&client_node, "tok", &["ws"])];
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let addr = spawn_server(Arc::new(ServerState::new(hub_store, healthy_hub, allow))).await;
        let client = SyncClient::new(format!("http://{addr}"), "tok").unwrap();
        let round = client
            .sync_workspace(&failing, &client_node, "ws", &["ws".into()], &BTreeMap::new())
            .await;
        match round {
            Err(e) => assert!(e.to_string().contains("the disk went away"), "{e}"),
            Ok(summary) => panic!("a failing store reported a round: {summary:?}"),
        }
    }

    /// remote-server.md Section 13: a 0.11 node says no to serving on each workspace request, so a
    /// 0.10 hub withdraws the consent an earlier release gave it.
    #[tokio::test]
    async fn a_node_says_no_to_serving_on_every_workspace_request() {
        let said = Arc::new(std::sync::Mutex::new(Vec::<Option<String>>::new()));
        let log = said.clone();
        let app = Router::new().route(
            "/sync/advertise",
            axum::routing::post(move |headers: HeaderMap| {
                let log = log.clone();
                async move {
                    log.lock().unwrap().push(
                        headers.get(SERVE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string),
                    );
                    StatusCode::IM_A_TEAPOT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await });
        let client = SyncClient::new(format!("http://{addr}"), "token").unwrap();
        let _ = client.advertise("team").await;
        let _ = client.advertise("private").await;
        assert_eq!(*said.lock().unwrap(), vec![Some("no".to_string()), Some("no".to_string())]);
    }

    /// A pull stamps the hub's own unstamped rows before exporting them, and stamping rewrites rows
    /// the projection carries - so the hub re-materializes after a pull that stamped anything, the
    /// way it does after a push (crash-recovery.md K2). A pull that stamped nothing costs nothing.
    #[tokio::test]
    async fn a_pull_that_stamps_rows_re_materializes_them() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        hub_store
            .add_observation(Observation::new("hub fact".into(), prov("ws", 5)))
            .unwrap();
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let client_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let allow = vec![entry(&client_node, "token", &["ws"])];
        let calls = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = calls.clone();
        let hook: OnApplied = Arc::new(move |ws: &str| seen.lock().unwrap().push(ws.to_string()));
        let state = Arc::new(ServerState::new(hub_store, hub, allow).with_on_applied(hook));
        let keys = state.peers.admitted().origin_keys;
        let addr = spawn_server(state).await;
        let client = SyncClient::new(format!("http://{addr}"), "token").unwrap();
        let mine: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let share = vec!["ws".to_string()];

        let first = client.sync_workspace(&mine, &client_node, "ws", &share, &keys).await.unwrap();
        assert_eq!(first.pulled, 1);
        assert_eq!(
            *calls.lock().unwrap(),
            vec!["ws".to_string()],
            "the stamping pull re-materialized"
        );
        client.sync_workspace(&mine, &client_node, "ws", &share, &keys).await.unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1, "a pull with nothing left to stamp does not");
    }

    /// sync-correctness.md Section 11 (D9): a surplus past any single body - here about 3 MB, over
    /// axum's old unnamed 2 MB default, which answered 413 to the whole push every round - goes in
    /// batches, and another node pulls it back in pages.
    #[tokio::test]
    async fn a_large_surplus_is_pushed_in_batches_and_pulled_in_pages() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let a = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let b = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
        let allow = vec![entry(&a, "token-a", &["ws"]), entry(&b, "token-b", &["ws"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node, allow));
        let keys = state.peers.admitted().origin_keys;
        let addr = spawn_server(state).await;
        let share = vec!["ws".to_string()];

        let a_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let filler = "x".repeat(2_000);
        for i in 0..1_500u64 {
            a_store
                .add_observation(Observation::new(format!("{i} {filler}"), prov("ws", i)))
                .unwrap();
        }
        let ca = SyncClient::new(format!("http://{addr}"), "token-a").unwrap();
        let pushed = ca.sync_workspace(&a_store, &a, "ws", &share, &keys).await.unwrap();
        assert_eq!(pushed.pushed, 1_500, "every batch landed");
        assert_eq!(hub_store.all_observations(Some("ws")).unwrap().len(), 1_500);

        let b_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let cb = SyncClient::new(format!("http://{addr}"), "token-b").unwrap();
        let pulled = cb.sync_workspace(&b_store, &b, "ws", &share, &keys).await.unwrap();
        assert_eq!(pulled.pulled, 1_500, "every page applied");
    }

    /// docs/pairing.md Section 4.3.1: a client given a vouched key verifies that origin's events only
    /// up to its bound in each workspace, and none in a workspace without one - the one-shot sync's
    /// path, as the daemon's round is.
    #[tokio::test]
    async fn a_vouched_key_verifies_only_up_to_its_bound() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let gone = SyncNode::new(NodeIdentity::from_secret_bytes([4u8; 32]));
        let puller = SyncNode::new(NodeIdentity::from_secret_bytes([5u8; 32]));
        let allow = vec![entry(&puller, "token-p", &["ws", "other"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node, allow));
        let addr = spawn_server(state).await;
        // The hub holds three events from a machine it no longer admits, in one workspace, and
        // one in another.
        let src = InMemoryStore::new();
        for i in 0..3u64 {
            src.add_observation(Observation::new(format!("gone {i}"), prov("ws", i)))
                .unwrap();
        }
        src.add_observation(Observation::new("gone elsewhere".into(), prov("other", 9)))
            .unwrap();
        let keys: BTreeMap<String, String> =
            [(gone.node_id().to_string(), gone.public_key_hex())].into();
        for ws in ["ws", "other"] {
            gone.backfill(&src, ws).unwrap();
            let events = src.attestations_since(ws, &VersionVector::default()).unwrap();
            gone.apply(hub_store.as_ref(), ws, events, &keys, &mut VersionVector::default())
                .unwrap();
        }
        let mut with_mine = keys.clone();
        with_mine.insert(puller.node_id().to_string(), puller.public_key_hex());
        let vouched: BTreeMap<String, BTreeMap<String, u64>> =
            [(gone.node_id().to_string(), [("ws".to_string(), 2)].into())].into();
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let client = SyncClient::new(format!("http://{addr}"), "token-p")
            .unwrap()
            .with_vouched(vouched);
        let share = Vec::new();
        let ws = client.sync_workspace(&store, &puller, "ws", &share, &with_mine).await.unwrap();
        assert_eq!((ws.pulled, ws.rejected_locally), (2, 1), "up to the bound, and not past it");
        let other = client
            .sync_workspace(&store, &puller, "other", &share, &with_mine)
            .await
            .unwrap();
        assert_eq!((other.pulled, other.rejected_locally), (0, 1), "none where it has no bound");
    }

    /// Section 11 with Section 6: a stream held by a rejection is claimed whole for the rest of the
    /// round, so pages move on to the other streams instead of resending the held one forever.
    #[tokio::test]
    async fn a_held_stream_does_not_starve_the_others_across_pages() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let (n1, n2) = (
            SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32])),
            SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32])),
        );
        // `first` sorts first, so its held stream is what every page would start with.
        let (first, second) = if n1.node_id() < n2.node_id() { (n1, n2) } else { (n2, n1) };
        let puller = SyncNode::new(NodeIdentity::from_secret_bytes([3u8; 32]));
        let allow = vec![entry(&puller, "token-p", &["ws"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node, allow));
        let addr = spawn_server(state).await;

        let src = InMemoryStore::new();
        for i in 0..(PAGE_EVENTS as u64 + 100) {
            src.add_observation(Observation::new(format!("first {i}"), prov("ws", i)))
                .unwrap();
        }
        first.backfill(&src, "ws").unwrap();
        let other = InMemoryStore::new();
        for i in 0..10u64 {
            other
                .add_observation(Observation::new(format!("second {i}"), prov("ws", i)))
                .unwrap();
        }
        second.backfill(&other, "ws").unwrap();
        // The hub holds a damaged copy of `first`'s seq 1: its signature no longer verifies.
        let mut events = src.attestations_since("ws", &VersionVector::default()).unwrap();
        events.extend(other.attestations_since("ws", &VersionVector::default()).unwrap());
        for ev in events {
            let mut ev = ev;
            let meta = ev.attestation.sync.as_mut().unwrap();
            if meta.origin_node == first.node_id() && meta.origin_seq == 1 {
                meta.signature = "00".repeat(64);
            }
            let obs = Observation::with_assertions(ev.content, ev.attestation, ev.assertions);
            hub_store.add_observation(obs).unwrap();
        }

        let keys: BTreeMap<String, String> = [&first, &second]
            .iter()
            .map(|n| (n.node_id().to_string(), n.public_key_hex()))
            .collect();
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let client = SyncClient::new(format!("http://{addr}"), "token-p").unwrap();
        let summary = client
            .sync_workspace(&store, &puller, "ws", &["ws".to_string()], &keys)
            .await
            .unwrap();
        assert_eq!(summary.pulled, 10, "the other stream arrived");
        let held = version_vector(store.as_ref(), "ws").unwrap();
        assert_eq!(held.get(first.node_id(), "ws"), 0, "nothing of the held stream past its hole");
    }

    /// sync-correctness.md Section 12 (D10): every request and every response says which protocol
    /// and release is talking, a hub's ping answer carries its protocol, and the hub records what each
    /// peer said - 0 and no release for a peer from before the headers.
    #[tokio::test]
    async fn both_sides_say_which_protocol_and_release_they_speak() {
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let spoke = SyncNode::new(NodeIdentity::from_secret_bytes([8u8; 32]));
        let old = SyncNode::new(NodeIdentity::from_secret_bytes([7u8; 32]));
        let peers = Arc::new(PeerDirectory::new(
            vec![entry(&spoke, "tok", &["w"]), entry(&old, "old", &["w"])],
            hub.node_id(),
            &hub.public_key_hex(),
        ));
        let store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let registry = Arc::new(PeerRegistry::default());
        let addr = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let hooks = Hooks { peer_registry: Some(registry.clone()), ..Hooks::default() };
        let server = tokio::spawn(serve(store, hub.clone(), addr, None, peers, hooks));

        let client = SyncClient::new(format!("http://{addr}"), "tok").unwrap();
        let mut ping = client.ping().await;
        for _ in 0..50 {
            if ping.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            ping = client.ping().await;
        }
        assert_eq!(ping.expect("answered").protocol, PROTOCOL);

        // A peer from before the headers: a bare bearer, no protocol, no release.
        let raw = reqwest::Client::new()
            .post(format!("http://{addr}/sync/ping"))
            .bearer_auth("old")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(raw.headers()[PROTOCOL_HEADER], PROTOCOL.to_string().as_str());
        assert_eq!(raw.headers()[RELEASE_HEADER], env!("CARGO_PKG_VERSION"));

        let seen: BTreeMap<String, (u32, Option<String>)> = registry
            .snapshot()
            .into_iter()
            .map(|p| (p.node_id, (p.protocol, p.release)))
            .collect();
        let here = Some(env!("CARGO_PKG_VERSION").to_string());
        assert_eq!(seen[spoke.node_id()], (PROTOCOL, here));
        assert_eq!(seen[old.node_id()], (0, None));

        // And a hub from before them answers a ping without `protocol`, which reads as 0.
        let older: PingResp =
            serde_json::from_str(r#"{"node_id":"n","version":"0.4.7","shared_workspaces":[]}"#)
                .unwrap();
        assert_eq!(older.protocol, 0);
        server.abort();
    }

    #[tokio::test]
    async fn a_peer_cannot_push_for_an_origin_not_granted_the_workspace() {
        // sync-correctness.md Section 8 (D6): P is granted `a`, Q only `b`. Q's event in `a` is
        // genuinely signed, and P has a copy. Pushed by P, it is refused - Q could not push it into
        // `a` itself - while P's own event in the same body lands.
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let p = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let q = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
        let allow = vec![entry(&p, "token-p", &["a"]), entry(&q, "token-q", &["b"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node, allow));
        let addr = spawn_server(state).await;

        let (p_store, q_store) = (InMemoryStore::new(), InMemoryStore::new());
        p_store
            .add_observation(Observation::new("p's own".into(), prov("a", 1)))
            .unwrap();
        q_store
            .add_observation(Observation::new("q's, in a".into(), prov("a", 2)))
            .unwrap();
        p.backfill(&p_store, "a").unwrap();
        q.backfill(&q_store, "a").unwrap();
        let mut events = p_store.attestations_since("a", &VersionVector::default()).unwrap();
        events.extend(q_store.attestations_since("a", &VersionVector::default()).unwrap());

        let client = SyncClient::new(format!("http://{addr}"), "token-p").unwrap();
        let resp = client.push("a", events).await.unwrap();
        assert_eq!(resp.accepted, 1, "p's own event lands");
        assert_eq!(resp.rejected.len(), 1);
        assert_eq!(resp.rejected[0].0, q.node_id());
        assert_eq!(resp.rejected[0].2, "OriginNotAdmitted");
        assert_eq!(hub_store.all_observations(Some("a")).unwrap().len(), 1);
    }

    /// End to end over HTTP: client A pushes its knowledge to the hub, client B pulls it through the
    /// hub (relay), and all three logs converge (F5 at the transport level).
    #[tokio::test]
    async fn two_clients_converge_through_the_hub() {
        let hub_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        // The hub has its own knowledge too.
        hub_store
            .add_observation(Observation::new("hub fact".into(), prov("ws", 5)))
            .unwrap();

        let a_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let a_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        a_store
            .add_observation(Observation::new("alpha".into(), prov("ws", 10)))
            .unwrap();

        let b_store: Arc<dyn AssertionStore> = Arc::new(InMemoryStore::new());
        let b_node = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
        b_store
            .add_observation(Observation::new("beta".into(), prov("ws", 20)))
            .unwrap();

        let allow = vec![entry(&a_node, "token-a", &["ws"]), entry(&b_node, "token-b", &["ws"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node.clone(), allow));
        let keys = state.peers.admitted().origin_keys;
        let addr = spawn_server(state).await;
        let base = format!("http://{addr}");
        let share = vec!["ws".to_string()];

        let ca = SyncClient::new(&base, "token-a").unwrap();
        let cb = SyncClient::new(&base, "token-b").unwrap();
        // A: push alpha, pull hub fact. B: push beta, pull hub fact + alpha (relayed). A again: pull beta.
        let s1 = ca.sync_workspace(&a_store, &a_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s1.pushed, s1.pulled), (1, 1));
        let s2 = cb.sync_workspace(&b_store, &b_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s2.pushed, s2.pulled), (1, 2), "B must receive A's event relayed by the hub");
        let s3 = ca.sync_workspace(&a_store, &a_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s3.pushed, s3.pulled), (0, 1));

        // All three logs converge to the same 3 observations (F5, log level).
        let ids = |s: &Arc<dyn AssertionStore>| {
            let mut v: Vec<String> =
                s.all_observations(Some("ws")).unwrap().into_iter().map(|o| o.id).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&a_store).len(), 3);
        assert_eq!(ids(&a_store), ids(&b_store));
        assert_eq!(ids(&a_store), ids(&hub_store));
        // Idempotence over the wire: another full round changes nothing.
        let s4 = ca.sync_workspace(&a_store, &a_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s4.pushed, s4.pulled), (0, 0));
    }
}
