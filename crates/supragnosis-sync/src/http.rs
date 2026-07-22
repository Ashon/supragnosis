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

use supragnosis_core::{AttestationEvent, KnowledgeStore, VersionVector};

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

/// Shared state of the sync API handlers.
pub struct ServerState {
    pub store: Arc<dyn KnowledgeStore>,
    pub node: Arc<SyncNode>,
    pub allowlist: Vec<AllowEntry>,
    /// Origin-key directory for apply verification: every allowlisted key plus this node's own.
    pub origin_keys: BTreeMap<String, String>,
}

impl ServerState {
    pub fn new(store: Arc<dyn KnowledgeStore>, node: Arc<SyncNode>, allowlist: Vec<AllowEntry>) -> Self {
        let mut origin_keys: BTreeMap<String, String> = allowlist
            .iter()
            .map(|e| (e.node_id.clone(), e.public_key_hex.clone()))
            .collect();
        origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
        Self { store, node, allowlist, origin_keys }
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
    let events = tokio::task::spawn_blocking(move || {
        // Stamp anything still unstamped before exporting (export-boundary stamping, Phase 2), then
        // serve the delta. The per-node share list is the allowlist entry's workspaces (6c).
        node.backfill(store.as_ref(), &req.workspace)?;
        export_delta(store.as_ref(), &req.workspace, &req.since, &entry.shared_workspaces)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
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
    let report = tokio::task::spawn_blocking(move || {
        let mut vv = VersionVector::default();
        node.apply(store.as_ref(), &req.workspace, req.events, &keys, &mut vv)
    })
    .await
    .map_err(internal)?
    .map_err(internal)?;
    Ok(Json(PushResp {
        accepted: report.accepted,
        rejected: report
            .rejected
            .into_iter()
            .map(|r| (r.origin_node, r.origin_seq, format!("{:?}", r.reason)))
            .collect(),
    }))
}

/// The sync API router - exposed so the daemon (Phase 4 wiring) can mount it.
pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/sync/advertise", post(advertise_handler))
        .route("/sync/pull", post(pull_handler))
        .route("/sync/push", post(push_handler))
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
) -> Result<(), TransportError> {
    validate_bind(&listen, tls.is_some(), allowlist.len())?;
    let state = Arc::new(ServerState::new(store, node, allowlist));
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
