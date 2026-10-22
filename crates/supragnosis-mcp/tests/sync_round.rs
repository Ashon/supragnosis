//! docs/pairing.md Section 4.5: the daemon syncs on its own. A round is the `sync_*` tools' push and
//! pull over the shared workspaces, under one lock; these cases run it against a real hub.

use std::collections::BTreeMap;
use std::sync::Arc;

use supragnosis_core::NodeIdentity;
use supragnosis_engine::{Engine, EntityInput, ObserveInput};
use supragnosis_mcp::{sync_round, SyncContext};
use supragnosis_store::InMemoryStore;
use supragnosis_sync::http::{bearer_hash, bind, AllowEntry, Hooks, PeerDirectory};
use supragnosis_sync::{ServerLink, SyncNode};

fn allow(node: &SyncNode, token: &str) -> AllowEntry {
    AllowEntry {
        node_id: node.node_id().to_string(),
        public_key_hex: node.public_key_hex(),
        bearer_hash: bearer_hash(token),
        shared_workspaces: vec!["ws".into()],
    }
}

fn context(node: SyncNode, hub: &str, token: &str, keys: &BTreeMap<String, String>) -> SyncContext {
    SyncContext {
        node: Arc::new(node),
        share_workspaces: vec!["ws".into()],
        receive: None,
        vouched: Default::default(),
        serve_workspaces: Vec::new(),
        servers: vec![ServerLink {
            url: hub.into(),
            auth_token: token.into(),
            ..Default::default()
        }],
        config_notes: Vec::new(),
        surfaces: Default::default(),
        origin_keys: keys.clone(),
        peer_registry: None,
        round: Default::default(),
        last_round: Default::default(),
        listener: Default::default(),
    }
}

/// GIVEN two nodes syncing to one hub, WHEN one records something and each runs its own round, THEN
/// the other holds it - no tool was called - and `last_round` says what each round moved.
#[tokio::test]
async fn a_round_carries_a_write_to_the_other_node_with_no_call() {
    let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
    let a_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
    let b_node = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
    let peers = Arc::new(PeerDirectory::new(
        vec![allow(&a_node, "token-a"), allow(&b_node, "token-b")],
        hub_node.node_id(),
        &hub_node.public_key_hex(),
    ));
    let keys = peers.admitted().origin_keys;
    let hub_store: Arc<dyn supragnosis_core::AssertionStore> = Arc::new(InMemoryStore::new());
    let bound = bind(
        hub_store,
        hub_node,
        "127.0.0.1:0".parse().unwrap(),
        None,
        peers,
        Hooks::default(),
    )
    .await
    .expect("the hub binds loopback");
    let hub = format!("http://{}", bound.local_addr().expect("addr"));
    tokio::spawn(bound.run());

    let a = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "a", "ws"));
    let b = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "b", "ws"));
    let (ctx_a, ctx_b) =
        (context(a_node, &hub, "token-a", &keys), context(b_node, &hub, "token-b", &keys));
    a.observe(ObserveInput {
        content: "the ledger is a table".into(),
        workspace: None,
        source_ref: None,
        confidence: None,
        on_behalf_of: None,
        derived_from: vec![],
        entities: vec![EntityInput {
            name: "Ledger".into(),
            kind: Some("Table".into()),
            description: None,
        }],
        relations: vec![],
    })
    .expect("observe on a");

    let first = sync_round(&a, &ctx_a).await;
    assert_eq!(first["ok"], true, "{first}");
    assert!(first["pushed"].as_u64().unwrap_or(0) >= 1, "{first}");
    let second = sync_round(&b, &ctx_b).await;
    assert_eq!(second["ok"], true, "{second}");
    assert!(second["applied"].as_u64().unwrap_or(0) >= 1, "{second}");
    let id = supragnosis_core::Entity::make_id("ws", "Ledger");
    assert!(b.get_entity(&id).expect("read").is_some(), "the write reached the other node");
    let said = ctx_b.last_round.read().expect("lock").clone().expect("a round was recorded");
    assert_eq!(said["applied"], second["applied"]);
}

/// A round against a hub that does not answer says why, and is recorded as failed - and that the
/// node goes on offline and syncs once the hub is reached (docs/pairing.md, decision D16).
#[tokio::test]
async fn a_round_that_fails_says_why() {
    let node = SyncNode::new(NodeIdentity::from_secret_bytes([3u8; 32]));
    let ctx = context(node, "http://127.0.0.1:9", "t", &BTreeMap::new());
    let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "c", "ws"));
    let round = sync_round(&engine, &ctx).await;
    assert_eq!(round["ok"], false, "{round}");
    assert!(round["error"].as_str().is_some_and(|e| !e.is_empty()), "{round}");
    assert!(
        round["error"].as_str().is_some_and(|e| e.contains("keeps working offline")),
        "a hub out of reach is a partition the node works through: {round}"
    );
    assert_eq!(ctx.last_round.read().expect("lock").clone().expect("recorded")["ok"], false);
}
