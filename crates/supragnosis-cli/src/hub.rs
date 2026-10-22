//! The hub's listener and the daemon's admin socket (docs/pairing.md Sections 4 and 4.1).
//!
//! The listener is up only while the bind rule allows it (F10, Principle 24): TLS off loopback and
//! something admitted. When it cannot open - nothing admitted, no TLS, the port taken, a certificate
//! it cannot read - the node keeps serving everything else and says why, rather than exiting into
//! launchd's restart of the same exit. `apply` is the one place that decides, at start and whenever
//! the configuration changes.
//!
//! The admin socket is how this user's own processes change the running daemon: start a sync round,
//! re-read the hub's admissions. It is a `0600` unix socket in the owner-only directory, so it needs
//! no token and is open to no other account, no agent (it is not MCP) and no page (the app's proxy
//! reaches only the viewer socket).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use supragnosis_core::AssertionStore;
use supragnosis_sync::http::{self as sync_http, Hooks, PeerDirectory};
use supragnosis_sync::SyncNode;

use crate::fed;

/// The hub's listener, opened and closed by the bind rule.
pub struct HubListener {
    store: Arc<dyn AssertionStore>,
    node: Arc<SyncNode>,
    hooks: Hooks,
    /// Who may connect; the running server reads it per request, and `apply` swaps it.
    pub peers: Arc<PeerDirectory>,
    running: Mutex<Option<(SocketAddr, tokio::task::JoinHandle<()>)>>,
    /// `{listening, address, why}`, read by `sync_status` and the Sync pane.
    state: Arc<RwLock<serde_json::Value>>,
}

impl HubListener {
    pub fn new(
        store: Arc<dyn AssertionStore>,
        node: Arc<SyncNode>,
        hooks: Hooks,
        peers: Arc<PeerDirectory>,
        state: Arc<RwLock<serde_json::Value>>,
    ) -> Self {
        Self { store, node, hooks, peers, running: Mutex::new(None), state }
    }

    /// Brings the listener to what `srv` and the bind rule allow: opens it, keeps it, moves it or
    /// closes it, and records why it is down when it is. Never fails the daemon.
    pub async fn apply(&self, srv: &fed::ServerSection) -> serde_json::Value {
        self.peers.replace(srv.allowlist.clone());
        let admitted = self.peers.admitted().allowlist.len() + srv.principals.len();
        let wanted = self.wanted(srv, admitted);
        let current = self.running.lock().ok().and_then(|r| r.as_ref().map(|(a, _)| *a));
        let state = match wanted {
            Err(why) => {
                self.stop();
                serde_json::json!({"listening": false, "address": srv.listen, "why": why})
            }
            Ok((addr, _)) if current == Some(addr) && !addr.port().eq(&0) => {
                serde_json::json!({"listening": true, "address": addr.to_string(), "why": null})
            }
            Ok((addr, tls)) => {
                self.stop();
                let mut hooks = self.hooks.clone();
                if let Some(extra) = hooks.extra.as_mut() {
                    extra.admitted = srv.principals.len();
                }
                match sync_http::bind(
                    self.store.clone(),
                    self.node.clone(),
                    addr,
                    tls,
                    self.peers.clone(),
                    hooks,
                )
                .await
                {
                    Ok(bound) => {
                        let at = bound.local_addr().unwrap_or(addr);
                        let task = tokio::spawn(async move {
                            if let Err(e) = bound.run().await {
                                tracing::error!(error = %e, "federation sync API terminated");
                            }
                        });
                        if let Ok(mut r) = self.running.lock() {
                            *r = Some((at, task));
                        }
                        tracing::info!(listen = %at, admitted, "federation sync API listening");
                        serde_json::json!({"listening": true, "address": at.to_string(), "why": null})
                    }
                    Err(e) => {
                        let why = format!("the listener on {addr} could not start: {e}");
                        serde_json::json!({"listening": false, "address": srv.listen, "why": why})
                    }
                }
            }
        };
        if state["listening"] != true {
            tracing::warn!(why = ?state["why"], "federation sync API is not listening; the node serves without it");
        }
        if let Ok(mut s) = self.state.write() {
            *s = state.clone();
        }
        state
    }

    /// The address and TLS paths `srv` asks for, if the bind rule allows them, or why not.
    fn wanted(
        &self,
        srv: &fed::ServerSection,
        admitted: usize,
    ) -> Result<(SocketAddr, Option<sync_http::TlsPaths>), String> {
        let addr: SocketAddr = srv
            .listen
            .parse()
            .map_err(|_| format!("[server] listen {:?} is not an IP:port", srv.listen))?;
        let tls = match (&srv.tls_cert, &srv.tls_key) {
            (Some(c), Some(k)) => {
                Some(sync_http::TlsPaths { cert_pem: c.into(), key_pem: k.into() })
            }
            (None, None) => None,
            _ => return Err("[server] tls_cert and tls_key must be set together".into()),
        };
        sync_http::validate_bind(&addr, tls.is_some(), admitted).map_err(|e| e.to_string())?;
        Ok((addr, tls))
    }

    fn stop(&self) {
        if let Ok(mut r) = self.running.lock() {
            if let Some((addr, task)) = r.take() {
                task.abort();
                tracing::info!(listen = %addr, "federation sync API closed");
            }
        }
    }
}

/// Where this user's daemon answers its admin calls.
pub fn admin_socket() -> PathBuf {
    fed::fed_base_dir().join("admin.sock")
}

/// Serves the admin socket for this daemon, unless another live daemon already does. A socket it
/// cannot open is logged and left: the admin calls then say there is no daemon, and the node serves
/// on.
pub async fn spawn_admin(
    engine: Arc<supragnosis_engine::Engine>,
    sync: Option<Arc<supragnosis_mcp::SyncContext>>,
    hub: Option<Arc<HubListener>>,
) {
    let path = admin_socket();
    match bind_admin(&path).await {
        Ok(listener) => {
            let app = router(engine, sync, hub);
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!(error = %e, "admin socket stopped");
                }
            });
        }
        Err(e) => tracing::warn!(error = %format!("{e:#}"), "admin socket not served"),
    }
}

async fn bind_admin(path: &Path) -> Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let mode = std::fs::metadata(dir)?.permissions().mode();
        if mode & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    if path.exists() {
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            anyhow::bail!("another daemon already serves {}", path.display());
        }
        std::fs::remove_file(path).with_context(|| format!("removing stale {}", path.display()))?;
    }
    let listener = tokio::net::UnixListener::bind(path)
        .with_context(|| format!("binding {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// The admin calls: `POST /sync` runs a round now, `GET /sync` says what the last did, and `POST
/// /server/refresh` re-reads the hub's admissions and opens or closes its listener.
pub fn router(
    engine: Arc<supragnosis_engine::Engine>,
    sync: Option<Arc<supragnosis_mcp::SyncContext>>,
    hub: Option<Arc<HubListener>>,
) -> axum::Router {
    use axum::routing::{get, post};
    use axum::Json;
    let unsynced = || serde_json::json!({"error": "sync is not set up on this node"});
    let (run_engine, run_sync) = (engine, sync.clone());
    axum::Router::new()
        .route(
            "/sync",
            post(move || {
                let (engine, sync) = (run_engine.clone(), run_sync.clone());
                async move {
                    Json(match sync {
                        Some(ctx) => supragnosis_mcp::sync_round(&engine, &ctx).await,
                        None => unsynced(),
                    })
                }
            })
            .get(move || {
                let sync = sync.clone();
                async move {
                    Json(match sync {
                        Some(ctx) => ctx.last_round.read().map(|l| l.clone()).ok().flatten().into(),
                        None => unsynced(),
                    })
                }
            }),
        )
        .route(
            "/server/refresh",
            post(move || {
                let hub = hub.clone();
                async move {
                    let Some(hub) = hub else {
                        return Json(serde_json::json!({"error": "this node has no [server] section"}));
                    };
                    let srv = match fed::load() {
                        Ok(Some(fc)) => fc.server,
                        Ok(None) => None,
                        Err(e) => {
                            return Json(serde_json::json!({"error": format!("the config could not be read: {e:#}")}))
                        }
                    };
                    match srv {
                        Some(srv) => Json(hub.apply(&srv).await),
                        None => Json(serde_json::json!({"error": "the config no longer has a [server] section"})),
                    }
                }
            }),
        )
        .route("/", get(|| async { "supragnosis admin" }))
}

/// Calls this user's daemon on its admin socket and returns its JSON answer, or `None` when no
/// daemon answers there.
pub fn call(method: &str, path: &str) -> Result<Option<serde_json::Value>> {
    call_at(&admin_socket(), method, path)
}

pub fn call_at(socket: &Path, method: &str, path: &str) -> Result<Option<serde_json::Value>> {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(std::time::Duration::from_secs(600)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: supragnosis\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").context("the daemon's answer has no body")?;
    anyhow::ensure!(
        head.starts_with("HTTP/1.1 200"),
        "the daemon answered {}",
        head.lines().next().unwrap_or("")
    );
    Ok(Some(serde_json::from_str(body).context("the daemon's answer is not JSON")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The admin socket is this user's alone, and its calls answer what they can: with no sync and no
    /// `[server]` they say so rather than fail.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_admin_socket_is_owner_only_and_says_what_it_cannot_do() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sg-admin-{}", std::process::id()));
        let path = dir.join("admin.sock");
        let listener = bind_admin(&path).await.expect("binds");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o077, 0);
        let engine = Arc::new(supragnosis_engine::Engine::new(
            Arc::new(supragnosis_store::InMemoryStore::new()),
            "h",
            "ws",
        ));
        tokio::spawn(async move { axum::serve(listener, router(engine, None, None)).await });
        let call = |m: &'static str, p: &'static str| {
            let path = path.clone();
            tokio::task::spawn_blocking(move || call_at(&path, m, p))
        };
        let sync = call("POST", "/sync").await.unwrap().expect("answers").expect("a daemon");
        assert_eq!(sync["error"], "sync is not set up on this node");
        let refresh = call("POST", "/server/refresh")
            .await
            .unwrap()
            .expect("answers")
            .expect("a daemon");
        assert_eq!(refresh["error"], "this node has no [server] section");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            call_at(&path, "GET", "/sync").expect("no error").is_none(),
            "no daemon there now"
        );
    }
}
