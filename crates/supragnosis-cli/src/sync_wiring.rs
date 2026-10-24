//! Federation sync in a serving process: the sync context and hub wiring built from
//! supragnosis.toml, the background sync rounds, and the status the viewer reads.

use super::*;

/// What `build_sync_context` wires: the sync tools' context, the viewer's narrowing act, and the
/// hub's wiring - built now when the node has a `[server]` section, and by its first invitation
/// when it does not.
pub(crate) type SyncWiring = (
    Option<Arc<supragnosis_mcp::SyncContext>>,
    Option<supragnosis_viz::NarrowShare>,
    Arc<hub::HubSlot>,
);

/// Builds the MCP sync context from supragnosis.toml, and the hub's listener when a `[server]`
/// section is present. No sync context on a standalone node.
pub(crate) async fn build_sync_context(
    engine: &Arc<Engine>,
    fedcfg: Option<fed::FileConfig>,
    fed_status: Option<supragnosis_viz::FedStatus>,
    listener_state: Arc<std::sync::RwLock<serde_json::Value>>,
) -> Result<SyncWiring> {
    // Runtime peer observability (hub role): who actually checked in, when, how much.
    let peer_registry = Arc::new(supragnosis_sync::http::PeerRegistry::default());
    let Some(fc) = fedcfg else {
        let slot = hub_slot(engine.clone(), None, peer_registry, listener_state);
        return Ok((None, None, slot));
    };
    // One identity + one SyncNode per process - the server role and the sync tools share the HLC
    // clock and the per-workspace seq counters (two live counters over one store would collide).
    let identity = fed::load_or_create_identity()?;
    let node = Arc::new(
        supragnosis_sync::SyncNode::new(identity)
            .with_seq_mark(fed::fed_base_dir().join("node.seq")),
    );
    tracing::info!(node_id = %node.node_id(), "federation identity loaded");
    let (links, mut config_notes) = fc.sync.links();
    config_notes.extend(drop_self_admission(node.node_id(), fc.server.as_ref()));
    config_notes.extend(fed::principals_note(fc.server.as_ref()));
    config_notes.extend(noncanonical_key_notes(&fc));
    config_notes.extend(fc.sync.serve_note());
    // What a join recorded this node receives (docs/pairing.md Section 4.5, PA12).
    let (receive, receive_note) = join::receive_list(&join::Paths::default_paths());
    config_notes.extend(receive_note);
    for n in &config_notes {
        tracing::error!("federation configuration: {n}");
    }
    // One handle, written by the health loop and read by the tools. Created here because this is
    // where both sides are wired, and typed in the sync crate so neither adapter has to depend on
    // the other to see it (negotiated-surface.md Section 2).
    let surfaces: supragnosis_sync::NegotiatedSurfaces = Default::default();
    // Set on a hub: the live admission directory, published so the console can show who is in.
    let mut admitted: Option<Arc<supragnosis_sync::http::PeerDirectory>> = None;
    let slot = hub_slot(
        engine.clone(),
        Some(node.clone()),
        peer_registry.clone(),
        listener_state.clone(),
    );
    if let Some(srv) = &fc.server {
        // The listener is up only while the bind rule allows it, and a listener that cannot open
        // leaves the node serving and says why (docs/pairing.md Section 4.1, Principle 24).
        let hub = slot.hub().await?;
        hub.apply(srv).await;
        admitted = Some(hub.peers.clone());
    }
    let mut origin_keys = trusted_origin_keys(&fc.sync);
    origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
    // Keys a hub vouched for at a join, each good up to its bound (docs/pairing.md Section 4.3.1).
    let (vouched, vouched_note) =
        join::vouched_keys(&join::Paths::default_paths(), &mut origin_keys);
    config_notes.extend(vouched_note);
    // Federation status task: health-checks the configured hubs (connectivity + auth +
    // authorization in one round trip), computes per-workspace diffs vs each hub, snapshots the
    // known-peer registry (hub role), and publishes it all to the viewer's /api/federation.
    if let Some(fs) = fed_status.clone() {
        spawn_fed_status(FedStatusTask {
            engine: engine.clone(),
            fed: fs,
            node_id: node.node_id().to_string(),
            is_hub: fc.server.is_some(),
            sync: fc.sync.clone(),
            links: links.clone(),
            surfaces: surfaces.clone(),
            registry: peer_registry.clone(),
            admitted: admitted.clone(),
        });
    }
    // The console's narrowing act, built here because this is where the config and the live
    // admission directory both are - the viewer only routes it (P20: the viz crate knows nothing
    // about the sync crate or the config file).
    //
    // The file is written first and then re-read to refresh the directory, so the running state is
    // whatever the file says rather than a second copy maintained in parallel. One source of truth
    // survives the round trip, and a hand-edit made in the meantime is picked up rather than lost.
    let narrow: Option<supragnosis_viz::NarrowShare> = admitted.clone().map(|dir| {
        let status = fed_status.clone();
        let handler = move |node_id: &str, keep: &[String]| -> Result<Vec<String>, String> {
            let now = fed::narrow_shared_workspaces(node_id, keep).map_err(|e| e.to_string())?;
            let reloaded = fed::load()
                .map_err(|e| format!("narrowed on disk, but re-reading the config failed: {e}"))?
                .and_then(|c| c.server)
                .ok_or_else(|| {
                    "narrowed on disk, but the config no longer has a [server] section".to_string()
                })?;
            dir.replace(reloaded.allowlist);
            // Publish immediately rather than waiting for the status task's next pass. That task
            // recomputes `admitted` from this same directory, so the two agree either way - but it
            // runs on a slow interval, and a console that showed the old grant for up to a minute
            // after a narrowing would be displaying the wrong sharing boundary, which is the one
            // thing this surface exists to get right (P17).
            if let Some(fs) = &status {
                if let Ok(mut blob) = fs.write() {
                    if let Some(obj) = blob.as_object_mut() {
                        obj.insert(
                            "admitted".into(),
                            serde_json::to_value(admitted_json(&dir)).unwrap_or_default(),
                        );
                    }
                }
            }
            tracing::info!(peer = node_id, granted = ?now, "narrowed a peer's shared workspaces");
            Ok(now)
        };
        Arc::new(handler) as supragnosis_viz::NarrowShare
    });

    Ok((
        Some(Arc::new(supragnosis_mcp::SyncContext {
            node,
            share_workspaces: fc.sync.share_workspaces.clone(),
            receive,
            vouched,
            servers: links,
            config_notes,
            surfaces: surfaces.clone(),
            origin_keys,
            // Only a hub (server role) observes peers; a client-only node reports none.
            peer_registry: fc.server.is_some().then_some(peer_registry),
            round: Default::default(),
            last_round: Default::default(),
            listener: listener_state.clone(),
        })),
        narrow,
        slot,
    ))
}

/// The hub's wiring, made when it is first needed: at start for a node with a `[server]` section,
/// or by its first invitation (docs/pairing.md Section 4.1). `node` is the sync tools' own, when the
/// node has one - one identity and one SyncNode per process, since the two share the HLC clock and
/// the seq counters; without one, the node key is loaded, or made once, here.
pub(crate) fn hub_slot(
    engine: Arc<Engine>,
    node: Option<Arc<supragnosis_sync::SyncNode>>,
    peer_registry: Arc<supragnosis_sync::http::PeerRegistry>,
    listener_state: Arc<std::sync::RwLock<serde_json::Value>>,
) -> Arc<hub::HubSlot> {
    Arc::new(hub::HubSlot::new(move || {
        let node = match &node {
            Some(node) => node.clone(),
            None => {
                let identity = fed::load_or_create_identity()?;
                Arc::new(
                    supragnosis_sync::SyncNode::new(identity)
                        .with_seq_mark(fed::fed_base_dir().join("node.seq")),
                )
            }
        };
        // Post-apply hook: re-materialize the workspace after inbound pushes (Prop C).
        let hook_engine = engine.clone();
        let on_applied: supragnosis_sync::http::OnApplied = Arc::new(move |ws: &str| {
            match hook_engine.reproject(Some(ws)) {
                Ok(r) => tracing::info!(
                    workspace = ws,
                    entities = r.entities,
                    relations = r.relations,
                    "re-materialized after inbound sync"
                ),
                Err(e) => {
                    tracing::error!(workspace = ws, error = %e, "re-materialization after inbound sync failed")
                }
            }
        });
        // Live observability: stream sync hits into the viewer's event feed (SSE activity log).
        let act_engine = engine.clone();
        let on_activity: supragnosis_sync::http::OnActivity =
            Arc::new(move |a: supragnosis_sync::http::SyncActivity| {
                act_engine.emit(Event::Sync {
                    direction: a.direction.to_string(),
                    peer: a.peer,
                    workspace: a.workspace,
                    count: a.count,
                });
            });
        // Federated retrieval: remote peers search THIS node's full retrieval surface (hybrid when
        // the embedder is present), not just the store's keyword path.
        let search_engine = engine.clone();
        let on_search: supragnosis_sync::http::OnSearch = Arc::new(move |ws, q, lim| {
            search_engine
                .search(q, Some(ws), lim)
                .map(|o| {
                    let mode = match o.mode {
                        SearchMode::Hybrid => "hybrid",
                        SearchMode::Keyword => "keyword",
                    };
                    (o.hits, mode.to_string())
                })
                .map_err(|e| e.to_string())
        });
        let hooks = supragnosis_sync::http::Hooks {
            on_applied: Some(on_applied),
            on_activity: Some(on_activity),
            on_search: Some(on_search),
            peer_registry: Some(peer_registry.clone()),
            // The pairing calls ride beside the sync API; the listener adds them when it opens.
            extra: None,
        };
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let paths = invite::Paths::default_paths();
        let pairing = Arc::new(invite::Pairing::new(
            paths.clone(),
            node.clone(),
            engine.store(),
            invite::hub_name(&paths.config),
        ));
        let hub = Arc::new(hub::HubListener::new(
            engine.store(),
            node,
            hooks,
            peers,
            pairing,
            listener_state.clone(),
        ));
        hub.follow();
        Ok(hub)
    }))
}

/// How long the daemon waits before its next sync round (docs/pairing.md Section 4.5): five minutes
/// after one that worked, and twice the last wait after one that failed, up to an hour.
pub(crate) fn next_round_wait(last: std::time::Duration, failed: bool) -> std::time::Duration {
    const EVERY: std::time::Duration = std::time::Duration::from_secs(300);
    const MOST: std::time::Duration = std::time::Duration::from_secs(3600);
    if failed {
        (last.max(EVERY) * 2).min(MOST)
    } else {
        EVERY
    }
}

/// The daemon syncs on its own (docs/pairing.md Section 4.5): a round soon after it starts, then on
/// `next_round_wait`'s timer. Each round is the `sync_*` tools' push and pull over the shared
/// workspaces, under the same round lock, and `sync_status` reads what the last one did.
pub(crate) fn spawn_sync_rounds(engine: Arc<Engine>, ctx: Arc<supragnosis_mcp::SyncContext>) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut wait = std::time::Duration::ZERO;
        loop {
            let round = supragnosis_mcp::sync_round(&engine, &ctx).await;
            let failed = round.get("ok").and_then(|o| o.as_bool()) == Some(false);
            if failed {
                tracing::warn!(error = ?round.get("error"), "sync round failed");
            } else {
                tracing::info!(pushed = ?round.get("pushed"), applied = ?round.get("applied"), "sync round");
            }
            wait = next_round_wait(wait, failed);
            tokio::time::sleep(wait).await;
        }
    });
}

/// The federation status loop (every 60s): pings each configured hub (an authenticated no-op that
/// verifies connectivity, auth, and authorization in one round trip), computes the per-workspace
/// version-vector diff against each healthy hub ("who is ahead by how many events"), snapshots the
/// known-peer registry (hub role), and publishes everything to the viewer at /api/federation.
/// Health state CHANGES stream to the activity feed (hc-ok / hc-fail); steady state stays quiet.
/// The admitted set as the console reads it: node id and what each peer may read, nothing else.
///
/// `bearer_hash` is deliberately absent. It is a hash rather than a token, so publishing it would not
/// hand anyone a credential - but nothing on this surface reads it, and a credential-shaped field
/// with no reader is only a liability (P17/P18).
pub(crate) fn admitted_json(dir: &supragnosis_sync::http::PeerDirectory) -> Vec<serde_json::Value> {
    dir.admitted()
        .allowlist
        .iter()
        .map(
            |e| serde_json::json!({"node_id": e.node_id, "shared_workspaces": e.shared_workspaces}),
        )
        .collect()
}

/// Inputs to the federation status loop, as a struct rather than positional arguments - the list
/// crossed clippy's threshold when the resolved server links joined it, and a loop that reads eight
/// unrelated things is easier to call wrongly than to read.
pub(crate) struct FedStatusTask {
    engine: Arc<Engine>,
    fed: supragnosis_viz::FedStatus,
    node_id: String,
    is_hub: bool,
    sync: fed::SyncSection,
    links: Vec<supragnosis_sync::ServerLink>,
    surfaces: supragnosis_sync::NegotiatedSurfaces,
    registry: Arc<supragnosis_sync::http::PeerRegistry>,
    admitted: Option<Arc<supragnosis_sync::http::PeerDirectory>>,
}

pub(crate) fn spawn_fed_status(task: FedStatusTask) {
    let FedStatusTask { engine, fed, node_id, is_hub, sync, links, surfaces, registry, admitted } =
        task;
    /// Events `a` holds that `b` lacks, approximated by per-(origin, workspace) seq gaps.
    fn vv_ahead(a: &VersionVector, b: &VersionVector) -> u64 {
        a.0.iter().map(|(k, sa)| sa.saturating_sub(*b.0.get(k).unwrap_or(&0))).sum()
    }
    tokio::spawn(async move {
        let mut last: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        loop {
            let mut servers_json = Vec::new();
            for link in &links {
                let server = &link.url;
                let mut healthy = false;
                let mut version = None;
                let mut protocol = None;
                let mut ws_json = Vec::new();
                let mut diff = supragnosis_sync::SurfaceDiff::default();
                let mut negotiated_at = None;
                if let Ok(client) = supragnosis_sync::http::SyncClient::for_link(link) {
                    match client.ping().await {
                        Ok(p) => {
                            healthy = true;
                            version = Some(p.version);
                            protocol = Some(p.protocol);
                            // The authorization half of the answer, kept rather than dropped. This
                            // is the only place negotiation happens: handlers read the map under a
                            // lock and never ping, so nothing is added to a call that already
                            // blocks (F11, negotiated-surface.md N1).
                            diff = supragnosis_sync::surface_diff(
                                &sync.share_workspaces,
                                &p.shared_workspaces,
                            );
                            negotiated_at = Some(supragnosis_core::now_millis());
                            if let Ok(mut m) = surfaces.write() {
                                m.insert(
                                    server.clone(),
                                    supragnosis_sync::NegotiatedSurface {
                                        admits: Some(p.shared_workspaces.clone()),
                                        negotiated_at,
                                    },
                                );
                            }
                            // Drift only for what both sides hold. Asking a host to advertise a
                            // workspace it does not admit answers 403, which this loop used to
                            // discard with a bare `continue` - so a misconfigured workspace simply
                            // left the view. It is now named in `local_only` instead.
                            for ws in &diff.both {
                                let store = engine.store();
                                let ws_owned = ws.clone();
                                let local = tokio::task::spawn_blocking(move || {
                                    supragnosis_sync::version_vector(store.as_ref(), &ws_owned)
                                })
                                .await;
                                let (Ok(Ok(local)), Ok(remote)) =
                                    (local, client.advertise(ws).await)
                                else {
                                    continue;
                                };
                                ws_json.push(serde_json::json!({
                                    "workspace": ws,
                                    "local_ahead": vv_ahead(&local, &remote.vv),
                                    "hub_ahead": vv_ahead(&remote.vv, &local),
                                }));
                            }
                        }
                        Err(e) => {
                            // Unreachable is *unknown*, never an empty grant set: a host that is
                            // down must not read as a grant that was revoked (F21 clause 4, F12).
                            // The distinction lives in `record_ping`, where a case holds it.
                            supragnosis_sync::record_ping(&surfaces, server, None, 0);
                            if last.get(server).copied() != Some(false) {
                                tracing::warn!(%server, error = %e, "hub health check failed");
                            }
                        }
                    }
                }
                if healthy && last.get(server).copied() != Some(true) {
                    tracing::info!(%server, "hub health check ok");
                }
                if last.get(server).copied() != Some(healthy) {
                    engine.emit(Event::Sync {
                        direction: if healthy { "hc-ok".into() } else { "hc-fail".into() },
                        peer: server.clone(),
                        workspace: "-".into(),
                        count: 0,
                    });
                    last.insert(server.clone(), healthy);
                }
                servers_json.push(serde_json::json!({
                    "url": server,
                    "healthy": healthy,
                    "version": version,
                    "protocol": protocol,
                    // Unchanged key, narrowed meaning: drift rows for the workspaces both sides
                    // hold. The two new buckets are what the old view had no way to say.
                    "workspaces": ws_json,
                    "local_only": diff.local_only,
                    "peer_only": diff.peer_only,
                    "negotiated_at": negotiated_at,
                }));
            }
            let peers_json = if is_hub {
                serde_json::to_value(registry.snapshot()).unwrap_or_default()
            } else {
                serde_json::Value::Array(Vec::new())
            };
            // Who is admitted RIGHT NOW, read from the live directory rather than from the config
            // this process started with - so the console shows the running state, which is the whole
            // point of admission no longer being a startup snapshot. `bearer_hash` is deliberately
            // not published: it is a hash rather than a token, but nothing on this surface needs it,
            // and a credential-shaped field is not worth carrying for no reader (P17/P18).
            let allowlist_json = admitted.as_ref().map(|d| admitted_json(d)).unwrap_or_default();
            let blob = serde_json::json!({
                "configured": true,
                "node_id": node_id,
                "role": if is_hub { "hub" } else { "client" },
                "updated_ms": supragnosis_core::now_millis(),
                "servers": servers_json,
                "known_peers": peers_json,
                "admitted": allowlist_json,
            });
            if let Ok(mut w) = fed.write() {
                *w = blob;
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
}

/// sync-correctness.md Section 10: the bearer is never sent where it could be read in transit.
#[cfg(test)]
mod link_transport {
    use super::*;

    /// docs/pairing.md Section 4.1 and Principle 24: a `[server]` whose listener cannot open leaves
    /// the node serving and says why, and the listener opens and closes as the bind rule allows -
    /// an open invitation counting as an admission until it expires (D5). It stopped the daemon
    /// before, and launchd restarted it into the same exit.
    #[tokio::test]
    async fn a_listener_that_cannot_open_leaves_the_node_serving() {
        let store: Arc<dyn supragnosis_core::AssertionStore> =
            Arc::new(supragnosis_store::InMemoryStore::new());
        let node = Arc::new(supragnosis_sync::SyncNode::new(
            supragnosis_core::NodeIdentity::from_secret_bytes([7u8; 32]),
        ));
        let state: Arc<std::sync::RwLock<serde_json::Value>> = Default::default();
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let dir = std::env::temp_dir().join(format!("sg-listener-{}", std::process::id()));
        let paths = invite::Paths {
            hub: dir.join("hub"),
            config: dir.join("supragnosis.toml"),
            listen: "192.0.2.1:7420".into(),
        };
        let pairing = Arc::new(invite::Pairing::new(
            paths.clone(),
            node.clone(),
            store.clone(),
            "hub".into(),
        ));
        let listener = hub::HubListener::new(
            store,
            node,
            supragnosis_sync::http::Hooks::default(),
            peers,
            pairing.clone(),
            state.clone(),
        );
        let srv = |listen: &str, cert: Option<&str>, key: Option<&str>| fed::ServerSection {
            listen: listen.into(),
            tls_cert: cert.map(String::from),
            tls_key: key.map(String::from),
            allowlist: Vec::new(),
            principals: Vec::new(),
        };
        let unreadable = listener
            .apply(&srv("127.0.0.1:0", Some("/nonexistent/hub.crt"), Some("/nonexistent/hub.key")))
            .await;
        assert_eq!(unreadable["listening"], false, "{unreadable}");
        assert!(
            unreadable["why"].as_str().is_some_and(|w| w.contains("could not start")),
            "{unreadable}"
        );
        let loopback = listener.apply(&srv("127.0.0.1:0", None, None)).await;
        assert_eq!(loopback["listening"], true, "loopback needs no admission: {loopback}");
        let open = listener.apply(&srv("0.0.0.0:0", None, None)).await;
        assert_eq!(open["listening"], false, "{open}");
        assert!(open["why"].as_str().is_some_and(|w| w.contains("TLS")), "{open}");
        assert_eq!(
            state.read().expect("lock")["listening"],
            false,
            "sync_status reads the same state"
        );
        // An address no interface holds: the bind rule is all that differs between the two applies.
        // Nothing admitted refuses before binding; an open invitation passes the rule and the bind
        // itself then fails - no listener opens on this machine during the test.
        let (cert, key) =
            invite::make_certificate(&paths.hub, &["hub.local".into()], invite::now())
                .expect("cert");
        let elsewhere = srv("192.0.2.1:7420", Some(&cert), Some(&key));
        let closed = listener.apply(&elsewhere).await;
        assert!(
            closed["why"].as_str().is_some_and(|w| w.contains("no open invitation")),
            "{closed}"
        );
        let t = invite::now();
        pairing
            .invite(vec![], vec!["hub.local:7420".into()], [0; 32], t)
            .expect("invites");
        let invited = listener.apply(&elsewhere).await;
        assert!(
            invited["why"].as_str().is_some_and(|w| w.contains("could not start")),
            "an open invitation counts as an admission: {invited}"
        );
        pairing.expire(t + invite::VALID_FOR).expect("expires");
        let expired = listener.apply(&elsewhere).await;
        assert!(
            expired["why"].as_str().is_some_and(|w| w.contains("no open invitation")),
            "and stops counting at its expiry: {expired}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn links_of(toml: &str) -> (Vec<supragnosis_sync::ServerLink>, Vec<String>) {
        let fc: fed::FileConfig = toml::from_str(toml).expect("parses");
        fc.sync.links()
    }

    #[test]
    fn the_bearer_only_crosses_the_network_encrypted_to_a_verified_host() {
        let ca = std::env::temp_dir().join(format!("supragnosis-ca-{}.pem", std::process::id()));
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\n").expect("write");
        let (links, notes) = links_of(&format!(
            r#"
            [sync]
            insecure_tls = true
            [[sync.server]]
            url = "https://hub.example:7420"
            auth_token = "a"
            [[sync.server]]
            url = "http://hub.example:7420"
            auth_token = "b"
            [[sync.server]]
            url = "http://127.0.0.1:7420"
            auth_token = "c"
            [[sync.server]]
            url = "https://named.example:7420"
            auth_token = "e"
            ca = "{}"
            [[sync.server]]
            url = "https://missing.example:7420"
            auth_token = "f"
            ca = "/nonexistent/ca.pem"
            "#,
            ca.display()
        ));
        let by_url = |u: &str| links.iter().find(|l| l.url == u);
        assert!(by_url("https://hub.example:7420").is_some(), "kept, and verified by the system");
        assert!(by_url("http://hub.example:7420").is_none(), "plain HTTP off loopback is off");
        assert!(by_url("http://127.0.0.1:7420").is_some(), "plain HTTP on loopback stays");
        assert!(by_url("https://named.example:7420").expect("named").ca_pem.is_some());
        assert!(by_url("https://missing.example:7420").is_none(), "an unreadable ca disables it");
        let said = |needle: &str| notes.iter().any(|n| n.contains(needle));
        assert!(said("insecure_tls is no longer read"), "{notes:?}");
        assert!(said("http://hub.example:7420 is plain HTTP"), "{notes:?}");
        assert!(said("/nonexistent/ca.pem"), "{notes:?}");
        let _ = std::fs::remove_file(ca);
    }
}
