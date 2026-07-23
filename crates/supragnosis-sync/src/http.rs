//! Sync transport (M4 Phase 3, docs/federation.md Section 6a): the axum sync API
//! (`/sync/advertise|pull|push`) with in-process rustls TLS and bearer/allowlist wire auth, plus the
//! reqwest client. This is the ONLY surface permitted to bind non-loopback, and only with TLS enabled
//! and a non-empty allowlist (F10) - the MCP/viz loopback guard is untouched. Wire auth here is
//! transport authentication (who is on the wire); content trust stays with the apply pipeline's
//! signature verification (F6) and the read side (F13).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use supragnosis_core::{AttestationEvent, KnowledgeStore, SearchHit, VersionVector};

use crate::{export_delta, version_vector, SyncError, SyncNode};

/// One admitted peer node (docs/federation.md 6a): wire credentials + what it may read/write.
/// `bearer_hash` is the blake3 hex of the peer's bearer token - the server never stores the token.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[error("refusing to bind {addr}: {reason} (F10: non-loopback needs TLS + a non-empty allowlist)")]
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

/// F10 bind guard: loopback is always fine (local trust surface); a non-loopback bind demands both
/// in-process TLS and a non-empty allowlist. This mirrors, and never relaxes, the MCP/viz guard.
pub fn validate_bind(addr: &SocketAddr, tls: bool, allowlist_len: usize) -> Result<(), TransportError> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if !tls {
        return Err(TransportError::Bind { addr: *addr, reason: "TLS is not enabled".into() });
    }
    if allowlist_len == 0 {
        return Err(TransportError::Bind { addr: *addr, reason: "the allowlist is empty".into() });
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

/// Federated-recall hook: the wiring layer injects the engine's hybrid search so a remote read
/// query answers from this node's full recall surface (hits, mode). Without it the handler falls
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
    /// Epoch millis of the most recent authenticated request.
    pub last_seen_ms: u64,
    /// The most recent action ("ping" | "advertise" | "pull" | "push" | "search").
    pub last_action: String,
    /// Authenticated requests since process start.
    pub hits: u64,
}

impl PeerRegistry {
    pub fn record(&self, node_id: &str, action: &str) {
        let mut m = self.peers.lock().unwrap();
        let e = m.entry(node_id.to_string()).or_insert_with(|| PeerStatus {
            node_id: node_id.to_string(),
            last_seen_ms: 0,
            last_action: String::new(),
            hits: 0,
        });
        e.last_seen_ms = supragnosis_core::now_millis();
        e.last_action = action.to_string();
        e.hits += 1;
    }

    pub fn snapshot(&self) -> Vec<PeerStatus> {
        self.peers.lock().unwrap().values().cloned().collect()
    }
}

/// The wiring-layer hooks injected into the sync API (all optional): re-materialization after
/// inbound pushes, activity streaming to the viewer, and the engine-backed federated recall.
#[derive(Default)]
pub struct Hooks {
    pub on_applied: Option<OnApplied>,
    pub on_activity: Option<OnActivity>,
    pub on_search: Option<OnSearch>,
    /// Shared known-peer registry - the wiring layer keeps a clone so the MCP surface can report it.
    pub peer_registry: Option<Arc<PeerRegistry>>,
}

/// Shared state of the sync API handlers.
pub struct ServerState {
    pub store: Arc<dyn KnowledgeStore>,
    pub node: Arc<SyncNode>,
    pub allowlist: Vec<AllowEntry>,
    /// Origin-key directory for apply verification: every allowlisted key plus this node's own.
    pub origin_keys: BTreeMap<String, String>,
    on_applied: Option<OnApplied>,
    on_activity: Option<OnActivity>,
    on_search: Option<OnSearch>,
    peers: Option<Arc<PeerRegistry>>,
}

impl ServerState {
    pub fn new(store: Arc<dyn KnowledgeStore>, node: Arc<SyncNode>, allowlist: Vec<AllowEntry>) -> Self {
        let mut origin_keys: BTreeMap<String, String> = allowlist
            .iter()
            .map(|e| (e.node_id.clone(), e.public_key_hex.clone()))
            .collect();
        origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
        Self { store, node, allowlist, origin_keys, on_applied: None, on_activity: None, on_search: None, peers: None }
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

    /// Injects the federated-recall hook (the engine's hybrid search).
    pub fn with_on_search(mut self, f: OnSearch) -> Self {
        self.on_search = Some(f);
        self
    }

    /// Shares the known-peer registry (runtime observability, 6a).
    pub fn with_peers(mut self, r: Arc<PeerRegistry>) -> Self {
        self.peers = Some(r);
        self
    }

    fn seen(&self, node_id: &str, action: &str) {
        if let Some(r) = &self.peers {
            r.record(node_id, action);
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullResp {
    pub events: Vec<AttestationEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushReq {
    pub workspace: String,
    pub events: Vec<AttestationEvent>,
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
    /// The recall surface the SERVER used (hybrid/keyword) - results are the server's node-local
    /// recall aid (P16 exemption): to become local graph material they must arrive via pull (F1).
    pub mode: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingResp {
    /// The hub's node id.
    pub node_id: String,
    /// The hub's binary version.
    pub version: String,
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

/// Bearer authentication (6a): hash the presented token and look it up on the allowlist. 401 when
/// absent/unknown; the caller then checks per-workspace authorization (403).
fn authenticate(headers: &HeaderMap, allowlist: &[AllowEntry]) -> Result<AllowEntry, HandlerError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".to_string()))?;
    let hash = blake3::hash(token.as_bytes()).to_hex().to_string();
    allowlist
        .iter()
        .find(|e| e.bearer_hash == hash)
        .cloned()
        .ok_or((StatusCode::UNAUTHORIZED, "unknown bearer token".to_string()))
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
    headers: HeaderMap,
    Json(req): Json<AdvertiseReq>,
) -> Result<Json<AdvertiseResp>, HandlerError> {
    let entry = authenticate(&headers, &state.allowlist)?;
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    // Store calls are offloaded so a blocking backend cannot starve the async runtime (F11).
    let vv = tokio::task::spawn_blocking(move || version_vector(store.as_ref(), &req.workspace))
        .await
        .map_err(internal)?
        .map_err(internal)?;
    // Recorded in the peer registry, but NOT streamed: advertise is a metadata handshake that the
    // status loop fires every minute - the activity feed shows knowledge movement, not heartbeats.
    state.seen(&entry.node_id, "advertise");
    Ok(Json(AdvertiseResp { node_id: state.node.node_id().to_string(), vv }))
}

async fn pull_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<PullReq>,
) -> Result<Json<PullResp>, HandlerError> {
    let entry = authenticate(&headers, &state.allowlist)?;
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    let node = state.node.clone();
    let ws = req.workspace.clone();
    let events = tokio::task::spawn_blocking(move || {
        // Stamp anything still unstamped before exporting (export-boundary stamping, Phase 2), then
        // serve the delta. The per-node share list is the allowlist entry's workspaces (6c).
        node.backfill(store.as_ref(), &req.workspace)?;
        export_delta(store.as_ref(), &req.workspace, &req.since, &entry.shared_workspaces)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    state.seen(&entry.node_id, "pull");
    state.activity("pull-served", &entry.node_id, &ws, events.len());
    Ok(Json(PullResp { events }))
}

async fn push_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<PushReq>,
) -> Result<Json<PushResp>, HandlerError> {
    let entry = authenticate(&headers, &state.allowlist)?;
    authorize_workspace(&entry, &req.workspace)?;
    let store = state.store.clone();
    let node = state.node.clone();
    let keys = state.origin_keys.clone();
    let ws = req.workspace.clone();
    let report = tokio::task::spawn_blocking(move || {
        let mut vv = VersionVector::default();
        node.apply(store.as_ref(), &req.workspace, req.events, &keys, &mut vv)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    state.seen(&entry.node_id, "push");
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

/// Federated recall (P17 second door, honored): an authenticated peer searches THIS node's
/// ontology - gated by the same per-node workspace authorization as sync, answered from the node's
/// recall surface, and streamed to the activity feed like any other hit.
async fn search_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(req): Json<SearchReq>,
) -> Result<Json<SearchResp>, HandlerError> {
    let entry = authenticate(&headers, &state.allowlist)?;
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
    state.seen(&entry.node_id, "search");
    state.activity("search-served", &entry.node_id, &req.workspace, hits.len());
    Ok(Json(SearchResp { mode, hits }))
}

/// Health check (liveness): an authenticated no-op that registers the peer as seen and answers
/// with the hub's identity/version plus the caller's authorized workspaces - so a spoke can verify
/// connectivity, auth, AND authorization at startup in one round trip.
async fn ping_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
) -> Result<Json<PingResp>, HandlerError> {
    let entry = authenticate(&headers, &state.allowlist)?;
    // Registry only (see advertise) - health checks are heartbeats, not knowledge movement.
    state.seen(&entry.node_id, "ping");
    Ok(Json(PingResp {
        node_id: state.node.node_id().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
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
        .with_state(state)
}

/// TLS material for the in-process rustls termination (F10).
#[derive(Debug, Clone)]
pub struct TlsPaths {
    pub cert_pem: std::path::PathBuf,
    pub key_pem: std::path::PathBuf,
}

/// Serves the sync API. Loopback may run plain (a local trust surface); a non-loopback bind is
/// refused unless TLS material is provided AND the allowlist is non-empty (F10).
pub async fn serve(
    store: Arc<dyn KnowledgeStore>,
    node: Arc<SyncNode>,
    listen: SocketAddr,
    tls: Option<TlsPaths>,
    allowlist: Vec<AllowEntry>,
    hooks: Hooks,
) -> Result<(), TransportError> {
    validate_bind(&listen, tls.is_some(), allowlist.len())?;
    let mut state = ServerState::new(store, node, allowlist);
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
    let app = router(state);
    tracing::info!(%listen, tls = tls.is_some(), "sync API listening");
    match tls {
        Some(paths) => {
            // Pin the process-level CryptoProvider: with both aws-lc-rs (axum-server) and ring
            // (reqwest) compiled in, rustls refuses to auto-select. Idempotent - a second install
            // attempt is fine to ignore.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let cfg = axum_server::tls_rustls::RustlsConfig::from_pem_file(paths.cert_pem, paths.key_pem)
                .await
                .map_err(|e| TransportError::Tls(e.to_string()))?;
            axum_server::bind_rustls(listen, cfg)
                .serve(app.into_make_service())
                .await?;
        }
        None => {
            axum_server::bind(listen).serve(app.into_make_service()).await?;
        }
    }
    Ok(())
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
}

impl SyncClient {
    /// `insecure_tls` accepts a self-signed server certificate (an internal-VM hub before a real CA);
    /// the signature layer (F6) still authenticates content end-to-end even then.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>, insecure_tls: bool) -> Result<Self, TransportError> {
        // Same CryptoProvider pin as the server side (idempotent) - the client dials HTTPS hubs.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .danger_accept_invalid_certs(insecure_tls)
            .build()?;
        Ok(Self { base: base_url.into().trim_end_matches('/').to_string(), token: token.into(), http })
    }

    async fn call<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        req: &Req,
    ) -> Result<Resp, TransportError> {
        let resp = self
            .http
            .post(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .json(req)
            .send()
            .await?;
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
        self.call("/sync/advertise", &AdvertiseReq { workspace: workspace.into() }).await
    }

    pub async fn pull(&self, workspace: &str, since: &VersionVector) -> Result<Vec<AttestationEvent>, TransportError> {
        let resp: PullResp = self
            .call("/sync/pull", &PullReq { workspace: workspace.into(), since: since.clone() })
            .await?;
        Ok(resp.events)
    }

    pub async fn push(&self, workspace: &str, events: Vec<AttestationEvent>) -> Result<PushResp, TransportError> {
        self.call("/sync/push", &PushReq { workspace: workspace.into(), events }).await
    }

    /// Health check: verifies connectivity, auth, and per-workspace authorization in one call.
    pub async fn ping(&self) -> Result<PingResp, TransportError> {
        self.call("/sync/ping", &serde_json::json!({})).await
    }

    /// Federated recall: search the SERVER's ontology (its recall surface, mode-labeled). Results
    /// are remote ids - pull the workspace to materialize them locally before traverse/get_entity.
    pub async fn search(&self, workspace: &str, query: &str, limit: usize) -> Result<SearchResp, TransportError> {
        self.call("/sync/search", &SearchReq { workspace: workspace.into(), query: query.into(), limit }).await
    }

    /// One full sync round for a workspace: backfill-stamp local knowledge, push what the server
    /// lacks (per its advertised VV), pull what we lack, and apply it (verify -> CAS -> VV).
    /// Re-materialization (engine reproject) is the caller's follow-up - transport moves the log.
    pub async fn sync_workspace(
        &self,
        store: &Arc<dyn KnowledgeStore>,
        node: &SyncNode,
        workspace: &str,
        share_workspaces: &[String],
        origin_keys: &BTreeMap<String, String>,
    ) -> Result<SyncSummary, TransportError> {
        node.backfill(store.as_ref(), workspace)?;
        let remote = self.advertise(workspace).await?;
        let mine = version_vector(store.as_ref(), workspace)?;
        let surplus = export_delta(store.as_ref(), workspace, &remote.vv, share_workspaces)?;
        let mut summary = SyncSummary::default();
        if !surplus.is_empty() {
            let resp = self.push(workspace, surplus).await?;
            summary.pushed = resp.accepted;
            summary.rejected_by_server = resp.rejected.len();
        }
        let deficit = self.pull(workspace, &mine).await?;
        if !deficit.is_empty() {
            let mut vv = mine;
            let report = node.apply(store.as_ref(), workspace, deficit, origin_keys, &mut vv)?;
            summary.pulled = report.accepted;
            summary.rejected_locally = report.rejected.len();
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
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

    fn entry(node: &SyncNode, token: &str, ws: &[&str]) -> AllowEntry {
        AllowEntry {
            node_id: node.node_id().to_string(),
            public_key_hex: node.public_key_hex(),
            bearer_hash: blake3::hash(token.as_bytes()).to_hex().to_string(),
            shared_workspaces: ws.iter().map(|s| s.to_string()).collect(),
        }
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
        assert!(validate_bind(&pub_addr, true, 0).is_err(), "non-loopback with empty allowlist refused");
        assert!(validate_bind(&pub_addr, true, 1).is_ok());
    }

    #[tokio::test]
    async fn wire_auth_rejects_bad_token_and_unshared_workspace() {
        let hub_store: Arc<dyn KnowledgeStore> = Arc::new(InMemoryStore::new());
        let hub = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        let client_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        let allow = vec![entry(&client_node, "secret-token", &["ws"])];
        let addr = spawn_server(Arc::new(ServerState::new(hub_store, hub, allow))).await;
        let base = format!("http://{addr}");

        // Wrong token -> 401 (F6 wire layer).
        let bad = SyncClient::new(&base, "wrong-token", false).unwrap();
        match bad.advertise("ws").await {
            Err(TransportError::Remote { status: 401, .. }) => {}
            other => panic!("expected 401, got {other:?}"),
        }
        // Right token, unshared workspace -> 403 (per-node authorization, 6c).
        let good = SyncClient::new(&base, "secret-token", false).unwrap();
        match good.advertise("private-ws").await {
            Err(TransportError::Remote { status: 403, .. }) => {}
            other => panic!("expected 403, got {other:?}"),
        }
        // Right token, shared workspace -> OK.
        assert!(good.advertise("ws").await.is_ok());
    }

    /// End to end over HTTP: client A pushes its knowledge to the hub, client B pulls it through the
    /// hub (relay), and all three logs converge (F5 at the transport level).
    #[tokio::test]
    async fn two_clients_converge_through_the_hub() {
        let hub_store: Arc<dyn KnowledgeStore> = Arc::new(InMemoryStore::new());
        let hub_node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes([9u8; 32])));
        // The hub has its own knowledge too.
        hub_store
            .add_observation(Observation::new("hub fact".into(), prov("ws", 5)))
            .unwrap();

        let a_store: Arc<dyn KnowledgeStore> = Arc::new(InMemoryStore::new());
        let a_node = SyncNode::new(NodeIdentity::from_secret_bytes([1u8; 32]));
        a_store.add_observation(Observation::new("alpha".into(), prov("ws", 10))).unwrap();

        let b_store: Arc<dyn KnowledgeStore> = Arc::new(InMemoryStore::new());
        let b_node = SyncNode::new(NodeIdentity::from_secret_bytes([2u8; 32]));
        b_store.add_observation(Observation::new("beta".into(), prov("ws", 20))).unwrap();

        let allow = vec![entry(&a_node, "token-a", &["ws"]), entry(&b_node, "token-b", &["ws"])];
        let state = Arc::new(ServerState::new(hub_store.clone(), hub_node.clone(), allow));
        let keys = state.origin_keys.clone();
        let addr = spawn_server(state).await;
        let base = format!("http://{addr}");
        let share = vec!["ws".to_string()];

        let ca = SyncClient::new(&base, "token-a", false).unwrap();
        let cb = SyncClient::new(&base, "token-b", false).unwrap();
        // A: push alpha, pull hub fact. B: push beta, pull hub fact + alpha (relayed). A again: pull beta.
        let s1 = ca.sync_workspace(&a_store, &a_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s1.pushed, s1.pulled), (1, 1));
        let s2 = cb.sync_workspace(&b_store, &b_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s2.pushed, s2.pulled), (1, 2), "B must receive A's event relayed by the hub");
        let s3 = ca.sync_workspace(&a_store, &a_node, "ws", &share, &keys).await.unwrap();
        assert_eq!((s3.pushed, s3.pulled), (0, 1));

        // All three logs converge to the same 3 observations (F5, log level).
        let ids = |s: &Arc<dyn KnowledgeStore>| {
            let mut v: Vec<String> = s
                .all_observations(Some("ws"))
                .unwrap()
                .into_iter()
                .map(|o| o.id)
                .collect();
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
