//! The federation wiring, and the pulls, pushes and rounds the `sync_*` tools and the daemon run.

use super::*;

#[derive(Clone)]
/// Federation wiring handed to the MCP surface (M4 Phase 4, docs/federation.md Section 9).
/// Present only when the node is configured for sync (supragnosis.toml); without it the `sync_*`
/// tools answer with setup guidance instead of failing (P5: absence is explained, not an error).
pub struct SyncContext {
    pub node: Arc<supragnosis_sync::SyncNode>,
    /// Outbound share whitelist (P17/F9) - what may leave this node.
    pub share_workspaces: Vec<String>,
    /// Keys a hub vouched for at a join, by origin, and the last sequence each verifies in each
    /// workspace (docs/pairing.md Section 4.3.1): their keys are in `origin_keys`, and an event past
    /// the bound - or in a workspace with none - is refused as from an unknown origin.
    pub vouched: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>>,
    /// What a join recorded this node receives (docs/pairing.md Section 4.5, PA12): the round and
    /// `sync_pull` pull these and the shared ones, and no others, whatever a hub admits. `None` on
    /// a node no join set up, which pulls what it is asked to.
    pub receive: Option<Vec<String>>,
    /// The sync servers (hubs) this node talks to, each with the credential presented to it.
    pub servers: Vec<supragnosis_sync::ServerLink>,
    /// Federation configuration that was worked around rather than obeyed, one line each.
    ///
    /// A degrade that only ever reached a startup log is a silent degrade a week later. P5 asks
    /// explicit configuration to work or fail and never to slide quietly; carrying the reason onto
    /// the surface an operator actually reads is what keeps the second half true.
    pub config_notes: Vec<String>,
    /// What each host last said this node may reach. Written by the daemon's background health
    /// loop; read here under a lock. A handler never negotiates - that would add a round trip per
    /// host to a call that already blocks (F11, negotiated-surface.md N1).
    pub surfaces: supragnosis_sync::NegotiatedSurfaces,
    /// Origin-key directory {node_id -> public key hex} for verifying pulled events (F6).
    /// Phase 5 supersedes this with the log-borne canon-policy binding.
    pub origin_keys: std::collections::BTreeMap<String, String>,
    /// Known-peer registry (hub role): runtime observability of who checked in (None on a
    /// client-only node).
    pub peer_registry: Option<std::sync::Arc<supragnosis_sync::http::PeerRegistry>>,
    /// One sync round at a time, whatever starts it: the daemon's own timer or a `sync_*` tool
    /// (docs/pairing.md Section 4.5).
    pub round: Arc<tokio::sync::Mutex<()>>,
    /// What the daemon's last round did, said by `sync_status` (P24, P5).
    pub last_round: Arc<std::sync::RwLock<Option<serde_json::Value>>>,
    /// The hub's listener, `{listening, address, why}` - null on a node with no `[server]`
    /// (docs/pairing.md Section 4.1). Written by the wiring layer whenever it opens or closes.
    pub listener: Arc<std::sync::RwLock<serde_json::Value>>,
}

/// One workspace pulled from every host that admits it and applied: verify, absorb, re-materialize.
/// It is what the `sync_pull` tool answers, and what a daemon's own round does (docs/pairing.md,
/// Section 4.5). The caller holds the context's round lock.
pub async fn pull_workspace(
    engine: &Arc<Engine>,
    ctx: &SyncContext,
    ws: &str,
) -> serde_json::Value {
    let ws = ws.to_string();
    // Narrow the round to hosts that admit this workspace, and say which were left out. The two
    // halves ship together on purpose: filtering alone replaces a host's loud `403` with a
    // silent skip, which trades a legible failure for an invisible one (N5).
    let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws);
    let mut results = Vec::new();
    for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
        let server = &link.url;
        let client = match supragnosis_sync::http::SyncClient::for_link(link) {
            Ok(c) => c,
            Err(e) => {
                results.push(serde_json::json!({"server": server, "error": e.said()}));
                continue;
            }
        };
        let store = engine.store();
        let mine = match tokio::task::spawn_blocking({
            let ws = ws.clone();
            move || supragnosis_sync::version_vector(store.as_ref(), &ws)
        })
        .await
        {
            Ok(Ok(vv)) => vv,
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        };
        // Page after page (sync-correctness.md Section 11); each page is applied off the
        // async runtime, as one pull was.
        let pulled = client
            .pull_all(&ws, mine, |events| {
                let (store, node) = (engine.store(), ctx.node.clone());
                let (keys, ws2) = (ctx.origin_keys.clone(), ws.clone());
                let bounds = vouched_bounds(ctx, &ws);
                async move {
                    tokio::task::spawn_blocking(move || {
                        let mut vv = supragnosis_sync::VersionVector::default();
                        node.apply_wire_bounded(
                            store.as_ref(),
                            &ws2,
                            events,
                            &keys,
                            &bounds,
                            None,
                            &mut vv,
                        )
                    })
                    .await
                    .map_err(|e| {
                        supragnosis_sync::http::TransportError::Io(std::io::Error::other(format!(
                            "join: {e}"
                        )))
                    })?
                    .map_err(Into::into)
                }
            })
            .await;
        match pulled {
            Ok(report) => {
                engine.emit(Event::Sync {
                    direction: "pull".into(),
                    peer: server.clone(),
                    workspace: ws.clone(),
                    count: report.accepted,
                });
                results.push(serde_json::json!({
                    "server": server,
                    "applied": report.accepted,
                    "rejected": report.rejected.len(),
                }))
            }
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    // Re-materialize once after all pulls (Prop C: convergence point).
    let engine = engine.clone();
    let ws2 = ws.clone();
    let reprojected = match tokio::task::spawn_blocking(move || engine.reproject(Some(&ws2))).await
    {
        Ok(Ok(r)) => {
            serde_json::json!({"observations": r.observations, "entities": r.entities, "relations": r.relations})
        }
        Ok(Err(e)) => serde_json::json!({"error": e.to_string()}),
        Err(e) => serde_json::json!({"error": format!("join: {e}")}),
    };
    serde_json::json!({"workspace": ws, "pulled": results, "skipped": routed.skipped, "reprojected": reprojected})
}

/// One shared workspace pushed to every host that admits it - what the `sync_push` tool answers, and
/// what a daemon's own round does. The caller holds the context's round lock.
pub async fn push_workspace(
    engine: &Arc<Engine>,
    ctx: &SyncContext,
    ws: &str,
) -> serde_json::Value {
    let ws = ws.to_string();
    if !ctx.share_workspaces.iter().any(|w| w == &ws) {
        return serde_json::json!({"error": format!(
            "workspace {ws:?} is not on the share whitelist (P17: nothing leaves by default) - \
             add it to [sync] share_workspaces in supragnosis.toml to share it"
        )});
    }
    // Ask each routed host first. Its copy of this node's own stream floors the seq counter
    // before anything is stamped (sync-correctness.md Section 5), and the version vector it
    // advertises is what the push below is computed against.
    let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws);
    let mut results = Vec::new();
    let mut asked = Vec::new();
    for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
        let server = &link.url;
        let client = match supragnosis_sync::http::SyncClient::for_link(link) {
            Ok(c) => c,
            Err(e) => {
                results.push(serde_json::json!({"server": server, "error": e.said()}));
                continue;
            }
        };
        match client.advertise(&ws).await {
            Ok(remote) => asked.push((server.clone(), client, remote)),
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    {
        let store = engine.store();
        let node = ctx.node.clone();
        let ws2 = ws.clone();
        let seen: Vec<u64> = asked.iter().map(|(_, _, r)| r.vv.get(node.node_id(), &ws)).collect();
        let floored = tokio::task::spawn_blocking(move || {
            seen.into_iter().try_for_each(|s| node.floor_seq(store.as_ref(), &ws2, s))
        })
        .await;
        match floored {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        }
    }
    // Stamp before export (export-boundary stamping, Phase 2).
    let mut reprojected = serde_json::Value::Null;
    {
        let store = engine.store();
        let node = ctx.node.clone();
        let ws2 = ws.clone();
        match tokio::task::spawn_blocking(move || node.backfill(store.as_ref(), &ws2)).await {
            Ok(Err(e)) => {
                return serde_json::json!({"error": format!("backfill stamping failed: {e}")})
            }
            // Stamping rewrites log rows the projection carries, so the graph is behind the log
            // until re-materialized (crash-recovery.md K2). A failure here is not the push's -
            // the rows stay owed and the next open repays them.
            Ok(Ok(stamped)) if stamped > 0 => {
                let engine = engine.clone();
                let ws3 = ws.clone();
                reprojected = match tokio::task::spawn_blocking(move || {
                    engine.reproject(Some(&ws3))
                })
                .await
                {
                    Ok(Ok(r)) => {
                        serde_json::json!({"stamped": stamped, "entities": r.entities, "relations": r.relations})
                    }
                    Ok(Err(e)) => {
                        serde_json::json!({"stamped": stamped, "error": e.to_string()})
                    }
                    Err(e) => {
                        serde_json::json!({"stamped": stamped, "error": format!("join: {e}")})
                    }
                };
            }
            _ => {}
        }
    }
    // The round was narrowed above to hosts that admit this workspace, and `skipped` says which
    // were left out. The two halves ship together on purpose: filtering alone replaces a host's
    // loud `403` with a silent skip, which trades a legible failure for an invisible one (N5).
    for (server, client, remote) in asked {
        let server = &server;
        let store = engine.store();
        let share = ctx.share_workspaces.clone();
        let ws2 = ws.clone();
        let surplus = match tokio::task::spawn_blocking(move || {
            supragnosis_sync::export_delta(store.as_ref(), &ws2, &remote.vv, &share)
        })
        .await
        {
            Ok(Ok(ev)) => ev,
            Ok(Err(e)) => return serde_json::json!({"error": format!("store failure: {e}")}),
            Err(e) => return serde_json::json!({"error": format!("task join error: {e}")}),
        };
        if surplus.is_empty() {
            results.push(serde_json::json!({"server": server, "pushed": 0}));
            continue;
        }
        match client.push(&ws, surplus).await {
            Ok(resp) => {
                engine.emit(Event::Sync {
                    direction: "push".into(),
                    peer: server.clone(),
                    workspace: ws.clone(),
                    count: resp.accepted,
                });
                results.push(serde_json::json!({
                    "server": server,
                    "pushed": resp.accepted,
                    "rejected": resp.rejected,
                }))
            }
            Err(e) => results.push(serde_json::json!({"server": server, "error": e.said()})),
        }
    }
    serde_json::json!({"workspace": ws, "pushed": results, "skipped": routed.skipped, "reprojected": reprojected})
}

/// One round over the shared workspaces: each pushed, then pulled (docs/pairing.md Section 4.5).
/// It takes the round lock, so a tool's push or pull and the daemon's timer never overlap, and
/// records what it did for `sync_status`: when, how much moved, and the first failure.
pub async fn sync_round(engine: &Arc<Engine>, ctx: &SyncContext) -> serde_json::Value {
    let _round = ctx.round.lock().await;
    let (mut pushed, mut applied, mut errors) = (0u64, 0u64, Vec::<String>::new());
    let count = |v: &serde_json::Value, list: &str, field: &str, errors: &mut Vec<String>| -> u64 {
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            errors.push(e.to_string());
        }
        let mut n = 0;
        for r in v.get(list).and_then(|l| l.as_array()).into_iter().flatten() {
            n += r.get(field).and_then(|x| x.as_u64()).unwrap_or(0);
            if let Some(e) = r.get("error").and_then(|e| e.as_str()) {
                let server = r.get("server").and_then(|s| s.as_str()).unwrap_or("a host");
                errors.push(format!("{server}: {e}"));
            }
        }
        n
    };
    for ws in &ctx.share_workspaces {
        let push = push_workspace(engine, ctx, ws).await;
        pushed += count(&push, "pushed", "pushed", &mut errors);
        let pull = pull_workspace(engine, ctx, ws).await;
        applied += count(&pull, "pulled", "applied", &mut errors);
    }
    // What a join recorded this node receives and does not send: pulled only.
    let received: Vec<&String> = ctx
        .receive
        .iter()
        .flatten()
        .filter(|w| !ctx.share_workspaces.contains(w))
        .collect();
    for ws in &received {
        let pull = pull_workspace(engine, ctx, ws).await;
        applied += count(&pull, "pulled", "applied", &mut errors);
    }
    let workspaces: Vec<&String> = ctx.share_workspaces.iter().chain(received).collect();
    let record = serde_json::json!({
        "at": supragnosis_core::now_millis(),
        "workspaces": workspaces,
        "servers": ctx.servers.len(),
        "pushed": pushed,
        "applied": applied,
        "ok": errors.is_empty(),
        "error": errors.first(),
    });
    if let Ok(mut last) = ctx.last_round.write() {
        *last = Some(record.clone());
    }
    record
}

/// The bounds of the keys a hub vouched for, in one workspace: each vouched origin's last sequence
/// there, and none - nothing verifies - where it gave none (docs/pairing.md Section 4.3.1).
pub fn vouched_bounds(
    ctx: &SyncContext,
    workspace: &str,
) -> std::collections::BTreeMap<String, u64> {
    supragnosis_sync::bounds_in(&ctx.vouched, workspace)
}

/// Guidance answer for the `sync_*` tools on a node with no sync configuration (P5).
pub(crate) fn sync_unconfigured() -> String {
    err_json(
        "federation sync is not configured on this node. Create supragnosis.toml (next to the data \
         dir, or set SUPRAGNOSIS_CONFIG) with a [sync] section - servers, auth_token, \
         share_workspaces - and restart. See docs/federation.md Section 9",
    )
}
