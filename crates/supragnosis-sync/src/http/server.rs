//! The sync API's server: who is admitted, the state and hooks it serves with, and its listener.

use super::*;

/// What a request's headers say about the peer that sent it. `(0, None)` from a peer before them.
fn peer_speaks(headers: &HeaderMap) -> (u32, Option<String>) {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let protocol = text(PROTOCOL_HEADER).and_then(|v| v.parse().ok()).unwrap_or(0);
    (protocol, text(RELEASE_HEADER))
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
    pub(super) on_applied: Option<OnApplied>,
    on_activity: Option<OnActivity>,
    pub(super) on_search: Option<OnSearch>,
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

    pub(super) fn seen(&self, node_id: &str, action: &str, headers: &HeaderMap) {
        if let Some(r) = &self.registry {
            r.record(node_id, action, peer_speaks(headers));
        }
    }

    pub(super) fn activity(
        &self,
        direction: &'static str,
        peer: &str,
        workspace: &str,
        count: usize,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::tests::{entry, minted_tls};
    use supragnosis_core::NodeIdentity;
    use supragnosis_store::InMemoryStore;

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
}
