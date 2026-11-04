//! The sync transport's round trips: a server and its clients together, over a real socket.

use super::*;
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

pub(super) fn entry(node: &SyncNode, token: &str, ws: &[&str]) -> AllowEntry {
    AllowEntry {
        node_id: node.node_id().to_string(),
        public_key_hex: node.public_key_hex(),
        bearer_hash: blake3::hash(token.as_bytes()).to_hex().to_string(),
        shared_workspaces: ws.iter().map(|s| s.to_string()).collect(),
    }
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
        err.to_string().contains("401") || err.to_string().to_lowercase().contains("unauthorized"),
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
pub(super) fn minted_tls() -> (rcgen::CertifiedKey<rcgen::KeyPair>, TlsPaths) {
    let mut params = rcgen::CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()])
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
    let dir =
        std::env::temp_dir().join(format!("supragnosis-tls-{}-{nanos}-{n}", std::process::id()));
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
    let server = tokio::spawn(serve(store, hub.clone(), addr, Some(tls), peers, Hooks::default()));

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
pub(super) async fn spawn_server(state: Arc<ServerState>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    addr
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
