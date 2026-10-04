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

/// The agent surface's routes: MCP at `/mcp`, behind principal authentication and the body cap.
pub fn router(engine: Arc<Engine>, directory: Arc<Directory>, servable: Servable) -> axum::Router {
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
        assert!(text.contains("not in a workspace granted"), "{text}");
        let (_, text) = c.call("traverse", serde_json::json!({"id": secret_id})).await;
        assert!(text.contains("error") || text.contains("not found"), "{text}");

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
