//! The hub's agent surface (docs/remote-server.md Section 4): who is admitted, the HTTP layer that
//! authenticates them, and `supragnosis principal`, which an operator uses to admit and revoke.
//!
//! Principals live in `supragnosis.toml` under `[[server.principals]]`: a name, the blake3 hash of a
//! bearer credential, and per-workspace grants. The running hub re-reads them when the file changes,
//! so admitting or revoking takes effect without a restart - and a file that stops parsing admits
//! no one, because admitting from a stale copy after an edit meant to revoke is the worse failure.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result};
use supragnosis_engine::Engine;
use supragnosis_mcp::remote::{Principal, Servable, Surface};
use supragnosis_mcp::SupragnosisServer;

/// The largest request body the agent surface reads (Section 4.6). MCP messages are small JSON; a
/// bigger body is a mistake or an attack, and reading it whole first is what lets the cap hold.
pub const MAX_BODY: usize = 1024 * 1024;

/// How long one call may run (Section 4.6). The remote surface carries reads and ingest, which
/// answer in well under this; a call still running at the limit is holding the hub's resources for
/// one principal. Only POSTs are timed: a GET is the stream a client keeps open on purpose.
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The admitted principals, re-read from the config file when it changes.
pub struct Directory {
    path: PathBuf,
    cache: Mutex<(Option<SystemTime>, Vec<crate::fed::PrincipalEntry>)>,
}

impl Directory {
    pub fn new(path: PathBuf) -> Directory {
        Directory { path, cache: Mutex::new((None, Vec::new())) }
    }

    fn entries(&self) -> Vec<crate::fed::PrincipalEntry> {
        let modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.0 != modified || modified.is_none() {
            let parsed =
                std::fs::read_to_string(&self.path).map_err(|e| e.to_string()).and_then(|t| {
                    toml::from_str::<crate::fed::FileConfig>(&t).map_err(|e| e.to_string())
                });
            cache.1 = match parsed {
                Ok(cfg) => cfg.server.map(|s| s.principals).unwrap_or_default(),
                Err(e) => {
                    tracing::error!(path = %self.path.display(), error = %e, "the config no longer parses - the agent surface admits no one until it does");
                    Vec::new()
                }
            };
            cache.0 = modified;
        }
        cache.1.clone()
    }

    /// The principal a bearer credential belongs to, compared as blake3 digests in constant time.
    pub fn admit(&self, bearer: &str) -> Option<Principal> {
        let presented = *blake3::hash(bearer.as_bytes()).as_bytes();
        self.entries().into_iter().find_map(|e| {
            let stored = blake3::Hash::from_hex(e.token_hash.trim()).ok()?;
            crate::digests_equal(&presented, stored.as_bytes()).then(|| Principal {
                name: e.name.clone(),
                read: e.read.iter().cloned().collect(),
                write: e.write.iter().cloned().collect(),
            })
        })
    }
}

/// Which nodes have consented to this hub serving which of their workspaces (Section 4.5). Kept in
/// a file, so a restart does not forget a consent and quietly start refusing - or, worse, a
/// withdrawal and quietly keep serving.
pub struct ConsentBook {
    path: PathBuf,
    map: Mutex<std::collections::BTreeMap<String, BTreeSet<String>>>,
}

impl ConsentBook {
    /// Loads the book. One that cannot be read starts empty - consent then has to be given again,
    /// which is the direction that discloses less.
    pub fn open(path: PathBuf) -> ConsentBook {
        let map = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
                tracing::error!(path = %path.display(), error = %e, "the consent record does not parse - starting with no consent");
                Default::default()
            }),
            Err(_) => Default::default(),
        };
        ConsentBook { path, map: Mutex::new(map) }
    }

    /// Records what a node said on its latest round: yes adds it, no withdraws it.
    pub fn record(&self, node: &str, workspace: &str, serve: bool) {
        let mut map = self.map.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let set = map.entry(workspace.to_string()).or_default();
        let changed = if serve { set.insert(node.to_string()) } else { set.remove(node) };
        if !changed {
            return;
        }
        tracing::info!(
            node,
            workspace,
            serve,
            "a node changed its consent to this hub serving its workspace"
        );
        let text = serde_json::to_string_pretty(&*map).unwrap_or_default();
        if let Err(e) = crate::connect::write_replacing(&self.path, &text) {
            tracing::error!(path = %self.path.display(), error = %e, "could not persist the consent record");
        }
    }

    pub fn consented(&self, workspace: &str) -> BTreeSet<String> {
        self.map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(workspace)
            .cloned()
            .unwrap_or_default()
    }
}

/// The nodes that have consented to this hub serving a workspace (Section 4.5).
pub type Consented = Arc<dyn Fn(&str) -> BTreeSet<String> + Send + Sync>;

/// R5 for one hub: a workspace may be served when every node whose attestations it holds is this
/// node or has consented for that workspace.
pub fn servable(engine: Arc<Engine>, self_id: String, consented: Consented) -> Servable {
    Arc::new(move |ws: &str| {
        let origins = engine
            .origins(ws)
            .map_err(|e| format!("could not read workspace {ws:?} to check its origins: {e}"))?;
        let ok = consented(ws);
        let missing: Vec<String> =
            origins.into_iter().filter(|o| *o != self_id && !ok.contains(o)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        Err(format!(
            "workspace {ws:?} holds knowledge from node(s) {} that have not consented to this hub \
             serving it - each can add the workspace to `[sync] serve_workspaces` (docs/remote-server.md \
             Section 4.5)",
            missing.join(", ")
        ))
    })
}

/// The agent surface's routes: MCP at `/mcp` and the viewer's read tier at `/viz/`
/// (docs/remote-viewer.md), both behind principal authentication and the body cap. `events` is the
/// node's viewer event channel; without one the read tier's event stream says so and the page polls.
pub fn router(
    engine: Arc<Engine>,
    directory: Arc<Directory>,
    servable: Servable,
    events: Option<tokio::sync::broadcast::Sender<String>>,
) -> axum::Router {
    let viz = viz_routes(engine.clone(), servable.clone(), events);
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
    let surface = Surface { servable };
    let service = StreamableHttpService::new(
        move || Ok(SupragnosisServer::new(engine.clone()).with_remote(surface.clone())),
        Arc::new(LocalSessionManager::default()),
        // The Host allowlist guards a loopback server from DNS rebinding. This surface is reached by
        // whatever name the operator gave the hub, and every request carries a bearer credential a
        // rebinding page cannot attach, so the check has nothing to protect here.
        StreamableHttpServerConfig::default().disable_allowed_hosts(),
    );
    axum::Router::new()
        .nest_service("/mcp", service)
        .merge(viz)
        .layer(axum::middleware::from_fn(crate::expired_session_is_not_found))
        .layer(axum::middleware::from_fn(move |req, next| {
            admit_request(directory.clone(), req, next)
        }))
}

/// Authenticates one request as a principal (R2), caps its body (Section 4.6), and hands the
/// principal to the MCP layer through the request's extensions.
async fn admit_request(
    directory: Arc<Directory>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let unauthorized = || {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
            "Unauthorized: present the credential this hub's operator issued you as \
             `Authorization: Bearer <credential>` (docs/remote-server.md).",
        )
            .into_response()
    };
    let Some(p) = crate::bearer_of(req.headers()).and_then(|b| directory.admit(b)) else {
        tracing::info!(path = %req.uri().path(), "remote request refused: no admitted credential");
        return unauthorized();
    };
    let (mut parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY).await else {
        return (axum::http::StatusCode::PAYLOAD_TOO_LARGE, "request body over 1 MB")
            .into_response();
    };
    let timed = parts.method == axum::http::Method::POST;
    parts.extensions.insert(p);
    let run = next.run(axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes)));
    if !timed {
        return run.await;
    }
    match tokio::time::timeout(CALL_TIMEOUT, run).await {
        Ok(response) => response,
        Err(_) => (
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            "the call ran past 60 seconds and was stopped",
        )
            .into_response(),
    }
}

// --- The read tier: the viewer at /viz/ (docs/remote-viewer.md Section 3) -----------------------

struct VizCtx {
    engine: Arc<Engine>,
    servable: Servable,
    events: Option<tokio::sync::broadcast::Sender<String>>,
    audit: Audit,
}

/// The read tier's audit (Section 3.8): one line per principal and workspace at most once a minute,
/// carrying the number of reads since the previous line - "who read what, and when" to the minute,
/// without a line for every poll of every open window. Never the content.
#[derive(Default)]
struct Audit {
    seen: Mutex<std::collections::HashMap<(String, String), (std::time::Instant, u64)>>,
}

impl Audit {
    fn note(&self, principal: &str, path: &str, workspace: &str) {
        let mut seen = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (principal.to_string(), workspace.to_string());
        let now = std::time::Instant::now();
        if let Some((since, n)) = seen.get_mut(&key) {
            if now.duration_since(*since) < std::time::Duration::from_secs(60) {
                *n += 1;
                return;
            }
        }
        let reads = seen.get(&key).map(|(_, n)| *n).unwrap_or(0);
        tracing::info!(principal, path, workspace, reads_since_last_line = reads, "remote read");
        seen.insert(key, (now, 0));
    }
}

fn viz_routes(
    engine: Arc<Engine>,
    servable: Servable,
    events: Option<tokio::sync::broadcast::Sender<String>>,
) -> axum::Router {
    let ctx = Arc::new(VizCtx { engine, servable, events, audit: Audit::default() });
    let handler = move |axum::Extension(p): axum::Extension<Principal>,
                        uri: axum::http::Uri,
                        headers: axum::http::HeaderMap| {
        let ctx = ctx.clone();
        async move { viz_get(ctx, p, uri, headers).await }
    };
    // GET only: axum answers any other method with 405 before a handler runs (V2).
    axum::Router::new()
        .route("/viz", axum::routing::get(handler.clone()))
        .route("/viz/", axum::routing::get(handler.clone()))
        .route("/viz/{*rest}", axum::routing::get(handler))
}

async fn viz_get(
    ctx: Arc<VizCtx>,
    p: Principal,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use supragnosis_viz::remote::{Answer, Reader};
    let path = match uri.path().strip_prefix("/viz") {
        None | Some("") => "/".to_string(),
        Some(rest) => rest.to_string(),
    };
    let query = uri.query().unwrap_or("").to_string();
    let reader = Reader { name: p.name.clone(), readable: p.readable().into_iter().collect() };
    if path == "/api/events" {
        return viz_events(&ctx, reader);
    }
    let named = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .unwrap_or("-")
        .to_string();
    ctx.audit.note(&p.name, &path, &named);
    let (engine, servable) = (ctx.engine.clone(), ctx.servable.clone());
    let run = tokio::task::spawn_blocking(move || {
        supragnosis_viz::remote::route(&engine, &reader, &servable, "GET", &path, &query)
    });
    let answer = match tokio::time::timeout(CALL_TIMEOUT, run).await {
        Ok(Ok(a)) => a,
        Ok(Err(e)) => Answer {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": format!("the read failed: {e}") }).to_string(),
        },
        Err(_) => Answer {
            status: 504,
            content_type: "application/json",
            body: r#"{"error":"the read ran past 60 seconds and was stopped"}"#.to_string(),
        },
    };
    viz_respond(answer, &headers)
}

/// The headers every viewer answer carries, plus an `ETag` over a successful body so an unchanged
/// graph costs a hash rather than a transfer across the network (Section 3.8).
fn viz_respond(
    a: supragnosis_viz::remote::Answer,
    headers: &axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::http::header;
    use axum::response::IntoResponse;
    let status =
        axum::http::StatusCode::from_u16(a.status).unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
    let common = [
        (header::CONTENT_SECURITY_POLICY, supragnosis_viz::CSP.to_string()),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        (header::CACHE_CONTROL, "no-cache".to_string()),
    ];
    if a.status != 200 {
        return (status, common, [(header::CONTENT_TYPE, a.content_type)], a.body).into_response();
    }
    let etag = format!("\"{}\"", blake3::hash(a.body.as_bytes()).to_hex());
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == etag);
    if unchanged {
        return (axum::http::StatusCode::NOT_MODIFIED, common, [(header::ETAG, etag)])
            .into_response();
    }
    (
        status,
        common,
        [(header::CONTENT_TYPE, a.content_type.to_string()), (header::ETAG, etag)],
        a.body,
    )
        .into_response()
}

/// The read tier's event stream (Section 3.6): observes in the reader's servable grants, without
/// their sessions. Nothing else this node does reaches a principal.
fn viz_events(ctx: &VizCtx, reader: supragnosis_viz::remote::Reader) -> axum::response::Response {
    use axum::http::header;
    use axum::response::IntoResponse;
    use futures_util::StreamExt;
    let Some(tx) = &ctx.events else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"error":"this hub streams no events - the page polls instead"}"#,
        )
            .into_response();
    };
    tracing::info!(principal = %reader.name, "remote event stream opened");
    let rx = tx.subscribe();
    let servable = ctx.servable.clone();
    let hello = futures_util::stream::once(async {
        Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(b": ok\n\n"))
    });
    let frames = futures_util::stream::unfold(
        (rx, reader, servable),
        |(mut rx, reader, servable)| async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match rx.recv().await {
                    Ok(json) => {
                        if let Some(frame) =
                            supragnosis_viz::remote::filter_event(&json, &reader, &servable)
                        {
                            let bytes = axum::body::Bytes::from(format!("data: {frame}\n\n"));
                            return Some((Ok(bytes), (rx, reader, servable)));
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return None,
                }
            }
        },
    );
    (
        [(header::CONTENT_TYPE, "text/event-stream"), (header::CACHE_CONTROL, "no-store")],
        axum::body::Body::from_stream(hello.chain(frames)),
    )
        .into_response()
}

// --- `supragnosis principal` -------------------------------------------------------------------

/// A new credential: 32 bytes of entropy, hex.
pub fn new_credential() -> Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

fn split_list(s: &Option<String>) -> Vec<String> {
    s.as_deref()
        .map(|s| s.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect())
        .unwrap_or_default()
}

/// Adds a principal to `[server]`, returning the credential - shown once, stored only as a hash.
pub fn add(
    text: &str,
    name: &str,
    read: &Option<String>,
    write: &Option<String>,
) -> Result<(String, String)> {
    crate::profile::valid_name(name)
        .map_err(|e| anyhow::anyhow!(e.replace("profile", "principal")))?;
    let (read, write) = (split_list(read), split_list(write));
    if read.is_empty() && write.is_empty() {
        anyhow::bail!(
            "a principal with no grants can do nothing - give it --read and/or --write workspaces"
        );
    }
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing supragnosis.toml")?;
    let server = doc
        .get_mut("server")
        .and_then(|s| s.as_table_mut())
        .context("this node has no [server] section - principals are admitted by a hub's listener (docs/remote-server.md Section 4.1)")?;
    let list = server
        .entry("principals")
        .or_insert(toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .context("[server] principals is not an array of tables")?;
    if list.iter().any(|t| t.get("name").and_then(|n| n.as_str()) == Some(name)) {
        anyhow::bail!(
            "a principal named {name:?} exists - `supragnosis principal remove {name}` first"
        );
    }
    let credential = new_credential()?;
    let mut t = toml_edit::Table::new();
    t["name"] = toml_edit::value(name);
    t["token_hash"] = toml_edit::value(blake3::hash(credential.as_bytes()).to_hex().to_string());
    let arr = |v: &[String]| {
        let mut a = toml_edit::Array::new();
        v.iter().for_each(|w| a.push(w.as_str()));
        toml_edit::value(a)
    };
    t["read"] = arr(&read);
    t["write"] = arr(&write);
    list.push(t);
    Ok((doc.to_string(), credential))
}

/// Removes a principal; its credential stops working at the next request.
pub fn remove(text: &str, name: &str) -> Result<String> {
    let mut doc: toml_edit::DocumentMut = text.parse().context("parsing supragnosis.toml")?;
    let list = doc
        .get_mut("server")
        .and_then(|s| s.get_mut("principals"))
        .and_then(|p| p.as_array_of_tables_mut())
        .with_context(|| format!("no principal named {name:?}"))?;
    let before = list.len();
    list.retain(|t| t.get("name").and_then(|n| n.as_str()) != Some(name));
    if list.len() == before {
        anyhow::bail!("no principal named {name:?}");
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use supragnosis_core::{AssertionStore, Hlc, Observation, Provenance, SyncMeta, TrustTier};
    use supragnosis_engine::{EntityInput, ObserveInput};
    use supragnosis_store::InMemoryStore;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const HUB: &str = "hub-node";

    fn config(principals: &str) -> String {
        format!("[server]\nlisten = \"127.0.0.1:0\"\n{principals}")
    }

    /// Admitting and revoking through the file, as `principal add/remove` do, and the directory
    /// following each edit without a restart - and admitting no one when the file breaks.
    #[test]
    fn principals_are_admitted_and_revoked_through_the_file() {
        let dir =
            std::env::temp_dir().join(format!("supragnosis-principal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("supragnosis.toml");
        let (text, cred) =
            add(&config(""), "alice", &Some("docs".into()), &Some("team".into())).unwrap();
        std::fs::write(&path, &text).unwrap();
        assert!(!text.contains(&cred), "only the hash is stored");
        let d = Directory::new(path.clone());
        let p = d.admit(&cred).expect("admitted");
        assert_eq!(
            (p.name.as_str(), p.can_write("team"), p.can_read("docs")),
            ("alice", true, true)
        );
        assert!(d.admit("not-the-credential").is_none());
        assert!(add(&text, "alice", &None, &Some("x".into())).is_err(), "names are unique");

        std::thread::sleep(Duration::from_millis(1100)); // a distinct mtime on coarse filesystems
        std::fs::write(&path, remove(&text, "alice").unwrap()).unwrap();
        assert!(d.admit(&cred).is_none(), "revoked without a restart");

        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, &text).unwrap();
        assert!(d.admit(&cred).is_some());
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&path, "[server\nbroken").unwrap();
        assert!(d.admit(&cred).is_none(), "a config that does not parse admits no one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Consent survives a restart, a later "no" withdraws it, and an unreadable record starts empty.
    #[test]
    fn consent_is_kept_and_can_be_withdrawn() {
        let path =
            std::env::temp_dir().join(format!("supragnosis-consent-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let book = ConsentBook::open(path.clone());
        book.record("spoke-a", "team", true);
        book.record("spoke-b", "team", true);
        assert_eq!(
            ConsentBook::open(path.clone()).consented("team").len(),
            2,
            "kept across a restart"
        );
        book.record("spoke-a", "team", false);
        assert_eq!(
            ConsentBook::open(path.clone()).consented("team"),
            ["spoke-b".to_string()].into()
        );
        std::fs::write(&path, "not json").unwrap();
        assert!(ConsentBook::open(path.clone()).consented("team").is_empty());
        let _ = std::fs::remove_file(&path);
    }

    fn observe(engine: &Engine, ws: &str, content: &str, name: &str) {
        engine
            .observe(ObserveInput {
                content: content.into(),
                workspace: Some(ws.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { name: name.into(), kind: None, description: None }],
                relations: vec![],
            })
            .unwrap();
    }

    /// The hub's agent surface on an ephemeral port, as `serve` mounts it, with one principal.
    async fn hub(engine: Arc<Engine>, consented: BTreeSet<String>) -> (String, String) {
        hub_with(engine, consented, None).await
    }

    async fn hub_with(
        engine: Arc<Engine>,
        consented: BTreeSet<String>,
        events: Option<tokio::sync::broadcast::Sender<String>>,
    ) -> (String, String) {
        let dir = std::env::temp_dir().join(format!(
            "supragnosis-hub-{}-{}",
            std::process::id(),
            new_credential().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("supragnosis.toml");
        let (text, cred) =
            add(&config(""), "alice", &Some("docs".into()), &Some("team".into())).unwrap();
        std::fs::write(&path, text).unwrap();
        let ok: Consented = Arc::new(move |_: &str| consented.clone());
        let app = router(
            engine.clone(),
            Arc::new(Directory::new(path)),
            servable(engine, HUB.into(), ok),
            events,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/mcp"), cred)
    }

    /// A client through the bridge, which is how a remote profile reaches the hub.
    struct Client {
        tx: tokio::io::DuplexStream,
        rx: tokio::io::Lines<BufReader<tokio::io::DuplexStream>>,
        next: i64,
    }

    impl Client {
        async fn start(url: String, cred: String) -> Client {
            let (tx, bridge_in) = tokio::io::duplex(1 << 16);
            let (bridge_out, rx) = tokio::io::duplex(1 << 16);
            let cfg = crate::bridge::Config {
                url,
                token: Arc::new(move || Some(cred.clone())),
                wait: Duration::from_secs(2),
                ca_pem: None,
                unreachable_hint: "check the network".into(),
                token_source: "the profile's credential file".into(),
            };
            tokio::spawn(crate::bridge::run(cfg, bridge_in, bridge_out));
            let mut c = Client { tx, rx: BufReader::new(rx).lines(), next: 1 };
            c.send(serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
                .await;
            c.recv().await;
            c.send(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
            c
        }
        async fn send(&mut self, v: serde_json::Value) {
            self.tx.write_all(format!("{v}\n").as_bytes()).await.unwrap();
        }
        async fn recv(&mut self) -> serde_json::Value {
            let line = tokio::time::timeout(Duration::from_secs(10), self.rx.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            serde_json::from_str(&line).unwrap()
        }
        /// A resource read; returns what the server answered - the error, or the result.
        async fn read(&mut self, uri: &str) -> String {
            let id = self.next;
            self.next += 1;
            self.send(serde_json::json!({"jsonrpc":"2.0","id":id,"method":"resources/read",
                "params":{"uri":uri}}))
                .await;
            let r = self.recv().await;
            if r["error"].is_null() {
                r["result"].to_string()
            } else {
                r["error"].to_string()
            }
        }
        /// A tool call; returns (is_error, the text the tool answered with).
        async fn call(&mut self, tool: &str, args: serde_json::Value) -> (bool, String) {
            let id = self.next;
            self.next += 1;
            self.send(serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":tool,"arguments":args}}))
                .await;
            let r = self.recv().await;
            let result = &r["result"];
            let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
            (result["isError"].as_bool().unwrap_or(false), text)
        }
    }

    /// R2-R4 end to end over HTTP: the principal reads its grants and nothing else, writes where it
    /// may and as itself, and governance is refused.
    #[tokio::test]
    async fn a_principal_sees_and_writes_only_what_it_was_granted() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "hub", "default"));
        observe(&engine, "team", "the team uses rust", "rust");
        observe(&engine, "secret", "the launch date is friday", "launch");
        let (url, cred) = hub(engine.clone(), BTreeSet::new()).await;
        let mut c = Client::start(url, cred).await;

        let (err, text) = c
            .call("search_knowledge", serde_json::json!({"query":"rust","workspace":"team"}))
            .await;
        assert!(!err && text.contains("hits"), "{text}");
        let (err, text) = c
            .call("search_knowledge", serde_json::json!({"query":"launch","workspace":"secret"}))
            .await;
        assert!(err && text.contains("not granted"), "{text}");
        let (_, text) = c
            .call("search_knowledge", serde_json::json!({"query":"launch","workspace":"*"}))
            .await;
        assert!(
            !text.contains("friday") && !text.contains("launch"),
            "the union stays inside the grants: {text}"
        );

        let secret_id = supragnosis_core::Entity::make_id("secret", "launch");
        let (_, text) = c.call("get_entity", serde_json::json!({"id": secret_id})).await;
        assert!(text.contains("\"found\":false") && !text.contains("launch"), "{text}");
        let (_, text) = c.call("traverse", serde_json::json!({"id": secret_id})).await;
        assert!(text.contains("not found in a workspace you may read"), "{text}");

        let (err, text) = c
            .call(
                "observe",
                serde_json::json!({"content":"alice was here","on_behalf_of":"mallory"}),
            )
            .await;
        assert!(!err, "{text}");
        let written = engine.store().all_observations(Some("team")).unwrap();
        let mine = written.iter().find(|o| o.content == "alice was here").expect("written to team");
        assert_eq!(mine.provenance[0].on_behalf_of.as_deref(), Some("alice"), "R4");
        assert!(
            mine.provenance[0].trust_tier <= TrustTier::AgentExtracted,
            "R4: no tier above an agent's"
        );
        let (err, _) =
            c.call("observe", serde_json::json!({"content":"x","workspace":"docs"})).await;
        assert!(err, "docs is read-only for alice");
        let (err, text) =
            c.call("review", serde_json::json!({"proposal":"p","decision":"merge"})).await;
        assert!(err && text.contains("signing key"), "{text}");
        let (err, _) = c.call("sync_status", serde_json::json!({})).await;
        assert!(err);
    }

    /// remote-viewer.md Section 3.4: an entity id is computable from a workspace and a name, and an
    /// observation id from a workspace and a content. So every door an id opens answers an id
    /// outside the grants exactly as it answers one that does not exist - otherwise a principal
    /// could test what was recorded in a workspace it was never granted.
    #[tokio::test]
    async fn an_id_outside_the_grants_reads_as_an_unknown_one() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "hub", "default"));
        observe(&engine, "team", "the team uses rust", "rust");
        observe(&engine, "secret", "the launch date is friday", "launch");
        let secret_obs = engine.store().all_observations(Some("secret")).unwrap()[0].id.clone();
        let team_obs = engine.store().all_observations(Some("team")).unwrap()[0].id.clone();
        let nothing = "0".repeat(64);
        let (url, cred) = hub(engine.clone(), BTreeSet::new()).await;
        let mut c = Client::start(url, cred).await;

        let hidden = supragnosis_core::Entity::make_id("secret", "launch");
        let unknown = supragnosis_core::Entity::make_id("secret", "no such thing");
        for tool in ["get_entity", "traverse"] {
            let (e1, t1) = c.call(tool, serde_json::json!({"id": hidden})).await;
            let (e2, t2) = c.call(tool, serde_json::json!({"id": unknown})).await;
            assert_eq!(
                (e1, t1.replace(&hidden, "ID")),
                (e2, t2.replace(&unknown, "ID")),
                "{tool}: an entity outside the grants reads as an unknown one"
            );
        }

        let a = c.read(&format!("supragnosis://observation/{secret_obs}")).await;
        let b = c.read(&format!("supragnosis://observation/{nothing}")).await;
        assert!(!a.contains("friday"), "{a}");
        assert_eq!(a.replace(&secret_obs, "ID"), b.replace(&nothing, "ID"), "observation resource");

        let promote = |target: &str| {
            serde_json::json!({"kind": "claim_promotion", "targets": [target],
                "tier": "agent_extracted", "workspace": "team"})
        };
        let (e1, t1) = c.call("propose", promote(&secret_obs)).await;
        let (e2, t2) = c.call("propose", promote(&nothing)).await;
        assert!(
            t1.contains("\"error\""),
            "a gate proposal outside its workspace is refused: {t1}"
        );
        assert_eq!(
            (e1, t1.replace(&secret_obs, "ID")),
            (e2, t2.replace(&nothing, "ID")),
            "propose"
        );
        let (err, text) = c.call("propose", promote(&team_obs)).await;
        assert!(!err && text.contains("proposal_id"), "its own workspace's observation: {text}");
    }

    /// GETs one read-tier path as alice (or as nobody), returning (status, the ETag, the body).
    async fn viz(
        base: &str,
        cred: Option<&str>,
        path: &str,
        etag: Option<&str>,
    ) -> (u16, Option<String>, String) {
        let mut req = reqwest::Client::new().get(format!("{base}{path}"));
        if let Some(c) = cred {
            req = req.bearer_auth(c);
        }
        if let Some(e) = etag {
            req = req.header("If-None-Match", e);
        }
        let r = req.send().await.unwrap();
        let status = r.status().as_u16();
        let tag = r.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        (status, tag, r.text().await.unwrap())
    }

    /// remote-viewer.md V1-V3 over HTTP: the read tier answers alice only, within her grants, never
    /// changes state, and answers an id outside them as it answers an unknown one.
    #[tokio::test]
    async fn the_read_tier_answers_only_within_the_grants() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "hub", "default"));
        observe(&engine, "docs", "the docs live in the wiki", "wiki");
        observe(&engine, "team", "the team uses rust", "rust");
        observe(&engine, "secret", "the launch date is friday", "launch");
        let (url, cred) = hub(engine.clone(), BTreeSet::new()).await;
        let base = url.replace("/mcp", "/viz");
        let c = Some(cred.as_str());

        assert_eq!(viz(&base, None, "/api/graph", None).await.0, 401, "V1: no anonymous read");
        let (status, _, page) = viz(&base, c, "/", None).await;
        assert!(
            status == 200 && page.contains("<title>supragnosis</title>"),
            "the page: {status}"
        );

        let (_, _, surface) = viz(&base, c, "/api/surface", None).await;
        let surface: serde_json::Value = serde_json::from_str(&surface).unwrap();
        assert_eq!(surface["principal"], "alice");
        assert_eq!(surface["read_only"], true);
        assert_eq!(surface["default_workspace"], "docs");

        let (_, _, ws) = viz(&base, c, "/api/workspaces", None).await;
        assert_eq!(ws, r#"["docs","team"]"#, "enumeration is the grants that hold knowledge");

        let (_, _, g) = viz(&base, c, "/api/graph", None).await;
        let named = |g: &str, n: &str| g.contains(&format!("\"name\":\"{n}\""));
        assert!(named(&g, "wiki") && !named(&g, "rust"), "omitted -> docs: {g}");
        let (_, _, g) = viz(&base, c, "/api/graph?workspace=*", None).await;
        assert!(named(&g, "wiki") && named(&g, "rust") && !named(&g, "launch"), "{g}");
        let (status, _, body) = viz(&base, c, "/api/graph?workspace=secret", None).await;
        assert!(status == 403 && body.contains("not granted"), "{status} {body}");

        let hidden = supragnosis_core::Entity::make_id("secret", "launch");
        let unknown = supragnosis_core::Entity::make_id("secret", "nothing here");
        for path in ["/api/explain?entity=", "/api/observations?entity="] {
            let a = viz(&base, c, &format!("{path}{hidden}"), None).await;
            let b = viz(&base, c, &format!("{path}{unknown}"), None).await;
            assert_eq!((a.0, a.2), (b.0, b.2), "{path}: no existence oracle (Section 3.4)");
        }

        for path in [
            "/api/review?proposal=p&decision=merge",
            "/api/reify?hyperedge=h",
            "/api/federation",
            "/api/health",
        ] {
            assert_eq!(viz(&base, c, path, None).await.0, 403, "{path} is refused");
        }
        let posted = reqwest::Client::new()
            .post(format!("{base}/api/graph"))
            .bearer_auth(&cred)
            .send()
            .await
            .unwrap();
        assert_eq!(posted.status().as_u16(), 405, "GET only");

        let (_, tag, _) = viz(&base, c, "/api/graph?workspace=team", None).await;
        let tag = tag.expect("a successful answer carries an ETag");
        let (status, _, body) = viz(&base, c, "/api/graph?workspace=team", Some(&tag)).await;
        assert!(status == 304 && body.is_empty(), "an unchanged graph is not sent again");
    }

    /// remote-viewer.md V4: a union that includes a workspace the hub may not serve is refused as a
    /// whole, naming why - never shown without it.
    #[tokio::test]
    async fn a_union_with_an_unconsented_workspace_is_refused_whole() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Arc::new(Engine::new(store.clone(), "hub", "default"));
        observe(&engine, "docs", "the docs live in the wiki", "wiki");
        let mut synced = Observation::new(
            "a spoke's knowledge".into(),
            Provenance {
                host: "spoke".into(),
                on_behalf_of: None,
                workspace: "team".into(),
                source_ref: None,
                observed_at: 1,
                confidence: None,
                trust_tier: TrustTier::AgentExtracted,
                sync: None,
            },
        );
        synced.provenance[0].sync = Some(SyncMeta {
            origin_node: "spoke-node".into(),
            origin_seq: 1,
            hlc: Hlc::legacy(1),
            signature: String::new(),
            lineage: vec![],
        });
        store.add_observation(synced).unwrap();
        let (url, cred) = hub(engine, BTreeSet::new()).await;
        let base = url.replace("/mcp", "/viz");
        let c = Some(cred.as_str());

        let (status, _, body) = viz(&base, c, "/api/graph?workspace=*", None).await;
        assert!(status == 403 && body.contains("spoke-node"), "{status} {body}");
        assert_eq!(viz(&base, c, "/api/graph?workspace=docs", None).await.0, 200);
        let (_, _, surface) = viz(&base, c, "/api/surface", None).await;
        assert!(
            surface.contains("\"servable\":false") && surface.contains("spoke-node"),
            "{surface}"
        );
    }

    /// remote-viewer.md V7: the read tier's event stream carries observes in the reader's grants,
    /// without their sessions - not other workspaces, and not anyone's searches.
    #[tokio::test]
    async fn the_read_tier_streams_knowledge_not_activity() {
        let tx = tokio::sync::broadcast::channel::<String>(64).0;
        let engine = Arc::new(
            Engine::new(Arc::new(InMemoryStore::new()), "hub", "default")
                .with_session("operator-session")
                .with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone()))),
        );
        let (url, cred) = hub_with(engine.clone(), BTreeSet::new(), Some(tx)).await;
        let base = url.replace("/mcp", "/viz");
        let mut stream = reqwest::Client::new()
            .get(format!("{base}/api/events"))
            .bearer_auth(&cred)
            .send()
            .await
            .unwrap();
        assert_eq!(stream.status().as_u16(), 200);
        // The stream is open once its greeting arrives; emit only then, or the frames go nowhere.
        let hello = tokio::time::timeout(Duration::from_secs(5), stream.chunk()).await;
        assert!(matches!(hello, Ok(Ok(Some(_)))), "the stream greets");

        // What the MCP tools emit as they run: an observe elsewhere, a search, an observe in a grant.
        let observed = |ws: &str| supragnosis_engine::Event::Observe {
            observation: format!("obs-{ws}"),
            entities: vec![format!("ent-{ws}")],
            relations: 0,
            workspace: ws.into(),
        };
        engine.emit(observed("secret"));
        engine.emit(supragnosis_engine::Event::Search {
            query: "launch".into(),
            workspace: Some("team".into()),
            hits: 1,
            nodes: vec![],
            mode: "keyword".into(),
        });
        engine.emit(observed("team"));

        let mut seen = String::new();
        while !seen.contains("\"workspace\":\"team\"") {
            match tokio::time::timeout(Duration::from_secs(5), stream.chunk()).await {
                Ok(Ok(Some(bytes))) => seen.push_str(&String::from_utf8_lossy(&bytes)),
                other => panic!("the team observe never arrived ({other:?}); saw {seen}"),
            }
        }
        assert!(!seen.contains("secret") && !seen.contains("launch"), "{seen}");
        assert!(!seen.contains("operator-session"), "no session ids: {seen}");
    }

    /// R5: knowledge another node originated is served only with its consent, and without it the
    /// answer is a refusal that names the node - not a quietly smaller result.
    #[tokio::test]
    async fn another_nodes_knowledge_needs_its_consent() {
        let store = Arc::new(InMemoryStore::new());
        let engine = Arc::new(Engine::new(store.clone(), "hub", "default"));
        observe(&engine, "team", "local knowledge", "local");
        let mut synced = Observation::new(
            "a spoke's knowledge".into(),
            Provenance {
                host: "spoke".into(),
                on_behalf_of: None,
                workspace: "team".into(),
                source_ref: None,
                observed_at: 1,
                confidence: None,
                trust_tier: TrustTier::AgentExtracted,
                sync: None,
            },
        );
        synced.provenance[0].sync = Some(SyncMeta {
            origin_node: "spoke-node".into(),
            origin_seq: 1,
            hlc: Hlc::legacy(1),
            signature: String::new(),
            lineage: vec![],
        });
        store.add_observation(synced).unwrap();

        let (url, cred) = hub(engine.clone(), BTreeSet::new()).await;
        let mut c = Client::start(url, cred).await;
        let (err, text) = c
            .call("search_knowledge", serde_json::json!({"query":"knowledge","workspace":"team"}))
            .await;
        assert!(err && text.contains("spoke-node"), "refused, naming the origin: {text}");

        let (url, cred) = hub(engine, ["spoke-node".to_string()].into()).await;
        let mut c = Client::start(url, cred).await;
        let (err, text) = c
            .call("search_knowledge", serde_json::json!({"query":"knowledge","workspace":"team"}))
            .await;
        assert!(!err, "served once the origin consents: {text}");
    }
}
