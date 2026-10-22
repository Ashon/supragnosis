//! The hub's listener and the daemon's admin socket (docs/pairing.md Sections 4 and 4.1).
//!
//! The listener is up only while the bind rule allows it (F10, Principle 24): TLS off loopback and
//! something admitted - a node, a principal or an open invitation (D5). When it cannot open -
//! nothing admitted, no TLS, the port taken, a certificate it cannot read - the node keeps serving
//! everything else and says why, rather than exiting into launchd's restart of the same exit.
//! `apply` is the one place that decides, at start, whenever the configuration changes and whenever
//! an invitation opens or closes.
//!
//! The admin socket is how this user's own processes change the running daemon: start a sync round,
//! re-read the hub's admissions, invite, allow, deny, remove. It is a `0600` unix socket in the
//! owner-only directory, so it needs no token and is open to no other account, no agent (it is not
//! MCP) and no page (the app's proxy reaches only the viewer socket).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use supragnosis_core::AssertionStore;
use supragnosis_sync::http::{self as sync_http, Hooks, PeerDirectory};
use supragnosis_sync::SyncNode;

use crate::{fed, invite};

/// The hub's listener, opened and closed by the bind rule.
pub struct HubListener {
    store: Arc<dyn AssertionStore>,
    node: Arc<SyncNode>,
    hooks: Hooks,
    /// Who may connect; the running server reads it per request, and `apply` swaps it.
    pub peers: Arc<PeerDirectory>,
    /// The hub's invitations and the machines they admitted (docs/pairing.md Section 4.3).
    pub pairing: Arc<invite::Pairing>,
    running: Mutex<Option<(SocketAddr, tokio::task::JoinHandle<()>)>>,
    /// Held across a whole `apply`: two at once would both see no listener and both bind, and the
    /// one that lost would record a refusal while the other listened.
    applying: tokio::sync::Mutex<()>,
    /// `{listening, address, why}`, read by `sync_status` and the Sync pane.
    state: Arc<RwLock<serde_json::Value>>,
}

impl HubListener {
    pub fn new(
        store: Arc<dyn AssertionStore>,
        node: Arc<SyncNode>,
        hooks: Hooks,
        peers: Arc<PeerDirectory>,
        pairing: Arc<invite::Pairing>,
        state: Arc<RwLock<serde_json::Value>>,
    ) -> Self {
        Self {
            store,
            node,
            hooks,
            peers,
            pairing,
            running: Mutex::new(None),
            applying: tokio::sync::Mutex::new(()),
            state,
        }
    }

    /// Re-reads `[server]` and applies it; with none, the listener closes.
    pub async fn refresh(&self) -> serde_json::Value {
        let path = self.pairing.config_path().to_path_buf();
        let why = match fed::load_at(&path) {
            Ok(Some(fed::FileConfig { server: Some(srv), .. })) => return self.apply(&srv).await,
            Ok(_) => "the config has no [server] section".to_string(),
            Err(e) => format!("the config could not be read: {e:#}"),
        };
        let _applying = self.applying.lock().await;
        self.stop();
        let state = serde_json::json!({"listening": false, "address": null, "why": why});
        if let Ok(mut s) = self.state.write() {
            *s = state.clone();
        }
        state
    }

    /// Follows the hub's invitations: one that opens or closes - made, used, denied, cancelled,
    /// ended - brings the listener to what the bind rule now allows, and so does the expiry of the
    /// one open, which this loop waits for itself. An invitation left open by a daemon that
    /// restarted still closes on time.
    pub fn follow(self: &Arc<Self>) {
        let hub = Arc::downgrade(self);
        let changed = self.pairing.changed.clone();
        let pairing = self.pairing.clone();
        tokio::spawn(async move {
            loop {
                let next = {
                    let pairing = pairing.clone();
                    tokio::task::spawn_blocking(move || pairing.next_expiry()).await.ok().flatten()
                };
                let wait = next.map(|at| at.saturating_sub(invite::now()) + 1);
                tokio::select! {
                    _ = changed.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_secs(wait.unwrap_or(0))), if wait.is_some() => {
                        let pairing = pairing.clone();
                        let _ = tokio::task::spawn_blocking(move || pairing.expire(invite::now())).await;
                    }
                }
                let Some(hub) = hub.upgrade() else { return };
                hub.refresh().await;
            }
        });
    }

    /// Brings the listener to what `srv` and the bind rule allow: opens it, keeps it, moves it or
    /// closes it, and records why it is down when it is. Never fails the daemon.
    pub async fn apply(&self, srv: &fed::ServerSection) -> serde_json::Value {
        let _applying = self.applying.lock().await;
        self.peers.replace(srv.allowlist.clone());
        // An invitations file that cannot be read counts no invitation, and the refusal says why
        // rather than that none is open (P5).
        let (invitations, unread) = {
            let pairing = self.pairing.clone();
            match tokio::task::spawn_blocking(move || pairing.open_count(invite::now())).await {
                Ok(Ok(n)) => (n, None),
                Ok(Err(e)) => (0, Some(format!("{e:#}"))),
                Err(e) => (0, Some(e.to_string())),
            }
        };
        let admitted = self.peers.admitted().allowlist.len() + srv.principals.len() + invitations;
        let wanted = self.wanted(srv, admitted);
        let current = self.running.lock().ok().and_then(|r| r.as_ref().map(|(a, _)| *a));
        let state = match wanted {
            Err(why) => {
                self.stop();
                let why = match unread {
                    Some(e) => format!("{why}; the invitations could not be read: {e}"),
                    None => why,
                };
                serde_json::json!({"listening": false, "address": srv.listen, "why": why})
            }
            Ok((addr, _)) if current == Some(addr) && !addr.port().eq(&0) => {
                serde_json::json!({"listening": true, "address": addr.to_string(), "why": null})
            }
            Ok((addr, tls)) => {
                self.stop();
                // Beside the sync API: the principals' surface, and the pairing calls an open
                // invitation answers (D5). Both count toward the bind rule.
                let mut hooks = self.hooks.clone();
                let pairing = invite::router(self.pairing.clone());
                let extra_admitted = srv.principals.len() + invitations;
                hooks.extra = Some(match hooks.extra.take() {
                    Some(mut extra) => {
                        extra.router = extra.router.merge(pairing);
                        extra.admitted = extra_admitted;
                        extra
                    }
                    None => sync_http::ExtraSurface { router: pairing, admitted: extra_admitted },
                });
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

/// The hub's wiring, built at start when `[server]` exists and by the first invitation when it does
/// not (docs/pairing.md Section 4.1), so that invitation needs no restart.
pub struct HubSlot {
    built: tokio::sync::OnceCell<Arc<HubListener>>,
    make: Box<dyn Fn() -> Result<Arc<HubListener>> + Send + Sync>,
}

impl HubSlot {
    pub fn new(make: impl Fn() -> Result<Arc<HubListener>> + Send + Sync + 'static) -> Self {
        Self { built: tokio::sync::OnceCell::new(), make: Box::new(make) }
    }

    /// The hub, if it has been built.
    pub fn get(&self) -> Option<Arc<HubListener>> {
        self.built.get().cloned()
    }

    /// The hub, built now if it was not.
    pub async fn hub(&self) -> Result<Arc<HubListener>> {
        let hub = self.built.get_or_try_init(|| async { (self.make)() }).await?;
        Ok(hub.clone())
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
    hub: Arc<HubSlot>,
) {
    let path = admin_socket();
    match bind_admin(&path).await {
        Ok(listener) => {
            let app = router(engine, sync, hub, invite::Paths::default_paths());
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

/// What an admin call that could not do its act answers: the reason, as the CLI prints it.
fn refusal(e: anyhow::Error) -> serde_json::Value {
    serde_json::json!({"error": format!("{e:#}")})
}

/// Runs a pairing act off the async threads - each reads and writes its files under a lock.
async fn blocking(
    f: impl FnOnce() -> Result<serde_json::Value> + Send + 'static,
) -> serde_json::Value {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => refusal(e),
        Err(e) => refusal(e.into()),
    }
}

/// What `invite` asks the daemon for.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InviteAsk {
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub addresses: Vec<String>,
}

/// Makes an invitation (docs/pairing.md Section 4.2): the hub's `[server]` and certificate when it
/// has none, its wiring when it was not built, the code, the listener opened for it, and the
/// timer that closes it at its expiry.
async fn make_invitation(
    hub: &HubSlot,
    paths: &invite::Paths,
    ask: InviteAsk,
) -> Result<serde_json::Value> {
    for w in &ask.workspaces {
        crate::profile::valid_name(w)
            .map_err(|e| anyhow::anyhow!(e.replace("profile", "workspace")))?;
    }
    let (p, addresses) = (paths.clone(), ask.addresses.clone());
    let srv = tokio::task::spawn_blocking(move || {
        invite::may_invite(&p)?;
        invite::ensure_server(&p, &addresses)
    })
    .await??;
    let born = hub.get().is_none();
    let hub = hub.hub().await?;
    let port = srv.listen.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(7420);
    let addresses = match ask.addresses.is_empty() {
        true => invite::default_addresses(port),
        false => ask
            .addresses
            .iter()
            .map(|a| {
                if a.contains(':') && !a.starts_with('[') {
                    a.clone()
                } else {
                    format!("{a}:{port}")
                }
            })
            .collect(),
    };
    anyhow::ensure!(
        !addresses.is_empty(),
        "this machine has no address to put in the code - name one with --address"
    );
    let check = invite::check_for(paths, &srv, &ask.addresses)?;
    let pairing = hub.pairing.clone();
    let workspaces = ask.workspaces;
    let made = tokio::task::spawn_blocking(move || {
        pairing.invite(workspaces, addresses, check, invite::now())
    })
    .await??;
    // The listener's loop opens it for the invitation and closes it at the expiry (`follow`).
    let listener = hub.refresh().await;
    let mut notes = invite::notes();
    let loopback = srv.listen.parse::<std::net::SocketAddr>().is_ok_and(|a| a.ip().is_loopback());
    if loopback {
        notes.push(format!(
            "This hub listens on {} only, which no other machine reaches. Set [server] listen to \
             0.0.0.0:{port} in {} to let one join.",
            srv.listen,
            paths.config.display()
        ));
    }
    if born {
        notes.push(
            "The Sync pane lists the machines this hub admits after Supragnosis restarts; \
             `supragnosis peer list` lists them now."
                .into(),
        );
    }
    let mut out = serde_json::to_value(&made)?;
    out["listener"] = listener;
    out["notes"] = serde_json::json!(notes);
    Ok(out)
}

/// The admin calls: `POST /sync` runs a round now, `GET /sync` says what the last did, `POST
/// /server/refresh` re-reads the hub's admissions and opens or closes its listener, and the `/hub`
/// calls invite, allow, deny, cancel, list and remove (docs/pairing.md Sections 4.2 to 4.6).
pub fn router(
    engine: Arc<supragnosis_engine::Engine>,
    sync: Option<Arc<supragnosis_mcp::SyncContext>>,
    hub: Arc<HubSlot>,
    paths: invite::Paths,
) -> axum::Router {
    use axum::routing::{get, post};
    use axum::Json;
    let unsynced = || serde_json::json!({"error": "sync is not set up on this node"});
    let (run_engine, run_sync) = (engine, sync.clone());
    let no_hub = || serde_json::json!({"error": "this node has no [server] section"});
    let refresh_hub = hub.clone();
    let invite_hub = (hub.clone(), paths.clone());
    let status_hub = hub.clone();
    let act_hub = hub.clone();
    let peers_paths = paths.clone();
    let remove_hub = hub;
    // An act on the open invitation, which exists only if the hub was built. Allow and Deny name
    // the request the person was shown (PA11); cancel names none.
    let act = move |state: Option<invite::State>, ask: Option<invite::Answering>| {
        let hub = act_hub.clone();
        async move {
            let Some(hub) = hub.get() else {
                return Json(serde_json::json!({"error": "no invitation is open"}));
            };
            let pairing = hub.pairing.clone();
            Json(
                blocking(move || match (state, ask) {
                    (None, Some(ask)) => pairing.allow(&ask, invite::now()),
                    (None, None) => anyhow::bail!("Allow names the request it answers"),
                    (Some(s), ask) => pairing.close(s, ask.as_ref(), invite::now()),
                })
                .await,
            )
        }
    };
    let (allow, deny, cancel) = (act.clone(), act.clone(), act);
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
                let hub = refresh_hub.clone();
                async move {
                    match hub.get() {
                        Some(hub) => Json(hub.refresh().await),
                        None => Json(no_hub()),
                    }
                }
            }),
        )
        .route(
            "/hub/invite",
            post(move |Json(ask): Json<InviteAsk>| {
                let (hub, paths) = invite_hub.clone();
                async move {
                    Json(make_invitation(&hub, &paths, ask).await.unwrap_or_else(refusal))
                }
            })
            .get(move || {
                let hub = status_hub.clone();
                async move {
                    let Some(hub) = hub.get() else {
                        return Json(serde_json::json!({"open": null, "last": null}));
                    };
                    let pairing = hub.pairing.clone();
                    Json(blocking(move || pairing.status(invite::now())).await)
                }
            }),
        )
        .route(
            "/hub/allow",
            post(move |Json(ask): Json<invite::Answering>| allow(None, Some(ask))),
        )
        .route(
            "/hub/deny",
            post(move |Json(ask): Json<invite::Answering>| deny(Some(invite::State::Denied), Some(ask))),
        )
        .route("/hub/cancel", post(move || cancel(Some(invite::State::Cancelled), None)))
        .route(
            "/hub/peers",
            get(move || {
                let paths = peers_paths.clone();
                async move { Json(blocking(move || invite::peers_json(&paths)).await) }
            }),
        )
        .route(
            "/hub/peer/remove",
            post(move |Json(body): Json<serde_json::Value>| {
                let hub = remove_hub.clone();
                async move {
                    let who = body.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string();
                    // A node no invitation made a hub has no machine to remove, and is not
                    // made one by asking.
                    let Some(hub) = hub.get() else {
                        return Json(serde_json::json!({"error": format!("no machine named {who} joined this hub")}));
                    };
                    let pairing = hub.pairing.clone();
                    Json(blocking(move || pairing.remove(&who, invite::now())).await)
                }
            }),
        )
        .route("/", get(|| async { "supragnosis admin" }))
}

/// Calls this user's daemon on its admin socket and returns its JSON answer, or `None` when no
/// daemon answers there.
pub fn call(method: &str, path: &str) -> Result<Option<serde_json::Value>> {
    call_at(&admin_socket(), method, path, None)
}

/// [`call`] with a JSON body.
pub fn call_with(
    method: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    call_at(&admin_socket(), method, path, Some(body))
}

pub fn call_at(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>> {
    use std::io::{Read, Write};
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket) else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(std::time::Duration::from_secs(600)))?;
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let kind = if body.is_empty() { "" } else { "Content-Type: application/json\r\n" };
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: supragnosis\r\n{kind}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
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
        let slot = Arc::new(HubSlot::new(|| anyhow::bail!("this test builds no hub")));
        let paths = invite::Paths {
            hub: dir.join("hub"),
            config: dir.join("supragnosis.toml"),
            listen: "192.0.2.1:7420".into(),
        };
        tokio::spawn(async move { axum::serve(listener, router(engine, None, slot, paths)).await });
        let call = |m: &'static str, p: &'static str| {
            let path = path.clone();
            tokio::task::spawn_blocking(move || call_at(&path, m, p, None))
        };
        let sync = call("POST", "/sync").await.unwrap().expect("answers").expect("a daemon");
        assert_eq!(sync["error"], "sync is not set up on this node");
        let refresh = call("POST", "/server/refresh")
            .await
            .unwrap()
            .expect("answers")
            .expect("a daemon");
        assert_eq!(refresh["error"], "this node has no [server] section");
        let open = call("GET", "/hub/invite").await.unwrap().expect("answers").expect("a daemon");
        assert!(open["open"].is_null(), "{open}");
        let path_ = path.clone();
        let allow = tokio::task::spawn_blocking(move || {
            call_at(&path_, "POST", "/hub/allow", Some(&serde_json::json!({})))
        })
        .await
        .unwrap()
        .expect("answers")
        .expect("a daemon");
        assert_eq!(allow["error"], "no invitation is open");
        let remove = tokio::task::spawn_blocking({
            let path = path.clone();
            move || {
                call_at(&path, "POST", "/hub/peer/remove", Some(&serde_json::json!({"name": "x"})))
            }
        })
        .await
        .unwrap()
        .expect("answers")
        .expect("a daemon");
        assert_eq!(
            remove["error"], "no machine named x joined this hub",
            "and builds no hub to say so"
        );
        let peers = call("GET", "/hub/peers").await.unwrap().expect("answers").expect("a daemon");
        assert_eq!(peers["joined"], serde_json::json!([]));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            call_at(&path, "GET", "/sync", None).expect("no error").is_none(),
            "no daemon there now"
        );
    }

    /// A hub built for one test at `dir`: its pairing kept there, its `[server]`, once an invitation
    /// makes one, at an address no interface holds - the bind rule passes and the bind then fails,
    /// so no port opens to the network.
    fn slot_at(dir: &Path) -> (Arc<HubSlot>, invite::Paths, Arc<RwLock<serde_json::Value>>) {
        let paths = invite::Paths {
            hub: dir.join("hub"),
            config: dir.join("supragnosis.toml"),
            listen: "192.0.2.1:7420".into(),
        };
        let state: Arc<RwLock<serde_json::Value>> = Default::default();
        let (p, st) = (paths.clone(), state.clone());
        let slot = Arc::new(HubSlot::new(move || {
            let store: Arc<dyn AssertionStore> = Arc::new(supragnosis_store::InMemoryStore::new());
            let node =
                Arc::new(SyncNode::new(supragnosis_core::NodeIdentity::from_secret_bytes([5; 32])));
            let peers =
                Arc::new(PeerDirectory::new(Vec::new(), node.node_id(), &node.public_key_hex()));
            let pairing = Arc::new(invite::Pairing::new(
                p.clone(),
                node.clone(),
                store.clone(),
                "hub".into(),
            ));
            let hub = Arc::new(HubListener::new(
                store,
                node,
                Hooks::default(),
                peers,
                pairing,
                st.clone(),
            ));
            hub.follow();
            Ok(hub)
        }));
        (slot, paths, state)
    }

    /// docs/pairing.md Section 4.1: on a node with no `supragnosis.toml`, the first invitation
    /// builds the hub, makes its certificate and `[server]`, and its listener's state - what
    /// `sync_status` reads - says at once that the bind rule let it try; the invitation says the
    /// Sync pane catches up at the next start.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_first_invitation_makes_a_hub_of_a_node_with_no_config() {
        let dir = std::env::temp_dir().join(format!("sg-born-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (slot, paths, state) = slot_at(&dir);
        assert!(slot.get().is_none());
        let ask = InviteAsk {
            workspaces: vec!["acme".into()],
            addresses: vec!["hub.example.net".into()],
        };
        let made = make_invitation(&slot, &paths, ask).await.expect("invites");
        assert!(slot.get().is_some(), "the invitation built the hub");
        assert_eq!(made["addresses"], serde_json::json!(["hub.example.net:7420"]));
        assert_eq!(made["check"], "fingerprint", "a certificate the hub made is pinned");
        let srv = fed::load_at(&paths.config).unwrap().unwrap().server.unwrap();
        assert_eq!(srv.listen, paths.listen);
        let why = state.read().unwrap()["why"].as_str().unwrap_or_default().to_string();
        assert!(why.contains("could not start"), "the bind rule counted the invitation: {why}");
        assert!(made["notes"].to_string().contains("after Supragnosis restarts"), "{made}");
        let again = make_invitation(
            &slot,
            &paths,
            InviteAsk { workspaces: vec![], addresses: vec!["hub.example.net".into()] },
        )
        .await
        .expect("invites again");
        assert_eq!(again["replaced"], true);
        assert!(!again["notes"].to_string().contains("restarts"), "the hub was there already");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// docs/pairing.md Section 4.1 and PA1: the listener's own loop closes it when an unused
    /// invitation expires - nothing else is called.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_unused_invitation_closes_the_listener_at_its_expiry() {
        let dir = std::env::temp_dir().join(format!("sg-expiry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (slot, paths, state) = slot_at(&dir);
        invite::ensure_server(&paths, &[]).unwrap();
        let hub = slot.hub().await.unwrap();
        // Made as if nine minutes and fifty-eight seconds ago: two seconds left.
        let pairing = hub.pairing.clone();
        tokio::task::spawn_blocking(move || {
            pairing.invite(
                vec![],
                vec!["hub.local:7420".into()],
                crate::pair::CertCheck::ByName,
                invite::now() + 2 - invite::VALID_FOR,
            )
        })
        .await
        .unwrap()
        .unwrap();
        hub.refresh().await;
        let why = || state.read().unwrap()["why"].as_str().unwrap_or_default().to_string();
        assert!(why().contains("could not start"), "open while invited: {}", why());
        let mut closed = false;
        for _ in 0..80 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if why().contains("no open invitation") {
                closed = true;
                break;
            }
        }
        assert!(closed, "closed at the expiry by the loop alone: {}", why());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
