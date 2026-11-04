//! The sync API's routes: bearer authentication, the five handlers, and the router around them.

use super::*;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::tests::{entry, spawn_server};
    use supragnosis_core::NodeIdentity;
    use supragnosis_store::InMemoryStore;

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
}
