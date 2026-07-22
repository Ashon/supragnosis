//! supragnosis executable (single-binary CLI).
//!
//! Subcommands control the server. Running it **with no arguments** starts a stdio
//! MCP server - this is backward compatibility for the path where an MCP client
//! launches it as a child process.
//!
//!   supragnosis                  stdio MCP server (default, no arguments)
//!   supragnosis serve [options]   foreground run (--http for a streamable-http daemon, --viz for the viewer)
//!   supragnosis start [options]   start the background daemon (default MCP 127.0.0.1:7373 + viewer :7374)
//!   supragnosis stop             stop the background daemon
//!   supragnosis restart [options] stop then start
//!   supragnosis status           daemon status
//!
//! Each option uses its corresponding environment variable (SUPRAGNOSIS_*) as a
//! fallback/default (the option takes precedence). HTTP/viewer are loopback-only
//! (Principle 17). The background daemon is a self-managed process tracked via a
//! pidfile (~/.supragnosis/supragnosis.pid) and logs (~/.supragnosis/log), so it
//! works without launchd (for OS service registration such as auto-start on login,
//! see deploy/README.md).
//!
//! stop/restart/status are supervisor-aware: if the running daemon is managed by
//! launchd (the macOS deploy) rather than this CLI's pidfile, they detect it and
//! drive it via launchctl (restart = kickstart -k, stop = bootout). So a single
//! `supragnosis restart` restarts the MCP server + viewer regardless of who started it.

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{stdio, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServiceExt;
use supragnosis_core::{EmbeddingProvider, KnowledgeStore};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::{CozoStore, InMemoryStore};

#[derive(Parser)]
#[command(name = "supragnosis", version, about = "MCP server that turns knowledge from many hosts/workspaces into an ontology")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Foreground run (stdio by default; --http for a streamable-http daemon)
    Serve(RunArgs),
    /// Start the background daemon (default MCP :7373 + viewer :7374)
    Start(RunArgs),
    /// Stop the background daemon
    Stop,
    /// Restart the daemon (stop then start)
    Restart(RunArgs),
    /// Query daemon status
    Status,
    /// Show this node's federation identity (node id + public key); --hash-token hashes a bearer token for an allowlist entry
    Identity(IdentityArgs),
    /// One-shot federation sync round against the configured servers (requires supragnosis.toml; stop the daemon first - the store is single-process)
    Sync(SyncArgs),
    /// Re-materialize a workspace's entity/relation projection from the observation log (HLC-ordered replay; stop the daemon first)
    Reproject(SyncArgs),
    /// Migrate legacy-id observations (pre-0.1.x content-address eras) to the current formula so they can sync (stop the daemon first)
    Migrate(SyncArgs),
}

#[derive(Args, Clone, Default)]
struct IdentityArgs {
    /// Print the blake3 hash of this bearer token (what a server allowlist entry stores).
    #[arg(long, value_name = "TOKEN")]
    hash_token: Option<String>,
}

#[derive(Args, Clone, Default)]
struct SyncArgs {
    /// Workspace to sync (default: the node default workspace).
    #[arg(long)]
    workspace: Option<String>,
}

/// Shared run options for serve/start/restart. When unspecified, resolved in the order SUPRAGNOSIS_* environment variable -> default.
#[derive(Args, Clone, Default)]
struct RunArgs {
    /// MCP streamable-http bind address (loopback). When omitted, serve uses stdio and start uses 127.0.0.1:7373.
    #[arg(long, value_name = "ADDR")]
    http: Option<String>,
    /// Live ontology viewer bind address (loopback). start defaults to 127.0.0.1:7374.
    #[arg(long, value_name = "ADDR")]
    viz: Option<String>,
    /// Store: cozo (default, file-persistent) | mem (non-persistent).
    #[arg(long)]
    store: Option<String>,
    /// Cozo data directory (default ~/.supragnosis/db).
    #[arg(long, value_name = "DIR")]
    data_dir: Option<String>,
    /// Host id for provenance (default localhost).
    #[arg(long)]
    host: Option<String>,
    /// Default workspace (default default).
    #[arg(long)]
    workspace: Option<String>,
    /// Embedder: fastembed | hashing | none.
    #[arg(long)]
    embed: Option<String>,
    /// Session id (footprint grouping key).
    #[arg(long)]
    session: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Serve(RunArgs::default())) {
        Cmd::Serve(a) => run_blocking(resolve(a, false)),
        Cmd::Start(a) => start(resolve(a, true)),
        Cmd::Stop => stop(),
        Cmd::Restart(a) => restart(resolve(a, true)),
        Cmd::Status => status(),
        Cmd::Identity(a) => identity_cmd(a),
        Cmd::Sync(a) => sync_cmd(a),
        Cmd::Reproject(a) => reproject_cmd(a),
        Cmd::Migrate(a) => migrate_cmd(a),
    }
}

/// Resolved run configuration.
struct Config {
    host: String,
    workspace: String,
    store_kind: String,
    data_dir: String,
    embed_kind: String,
    session: String,
    /// Some = streamable-http daemon, None = stdio.
    http: Option<String>,
    /// Some = accompanied by the live viewer.
    viz: Option<String>,
}

/// Resolves a Config from RunArgs + environment variables + defaults. When
/// `daemon=true` (start/restart), stdio is meaningless, so http/viz are filled in
/// with their loopback defaults.
fn resolve(a: RunArgs, daemon: bool) -> Config {
    let env = |k: &str| std::env::var(k).ok().filter(|s| !s.trim().is_empty());
    let host = a
        .host
        .or_else(|| env("SUPRAGNOSIS_HOST"))
        .unwrap_or_else(|| "localhost".to_string());
    Config {
        workspace: a
            .workspace
            .or_else(|| env("SUPRAGNOSIS_WORKSPACE"))
            .unwrap_or_else(|| "default".to_string()),
        store_kind: a
            .store
            .or_else(|| env("SUPRAGNOSIS_STORE"))
            .unwrap_or_else(|| "cozo".to_string()),
        data_dir: a
            .data_dir
            .or_else(|| env("SUPRAGNOSIS_DATA_DIR"))
            .unwrap_or_else(default_data_dir),
        embed_kind: a
            .embed
            .or_else(|| env("SUPRAGNOSIS_EMBED"))
            .unwrap_or_else(|| default_embed_kind().to_string()),
        session: a
            .session
            .or_else(|| env("SUPRAGNOSIS_SESSION"))
            .or_else(|| env("CLAUDE_CODE_SESSION_ID"))
            .unwrap_or_else(|| format!("{host}-{}", supragnosis_core::now_millis())),
        http: a
            .http
            .or_else(|| env("SUPRAGNOSIS_HTTP_ADDR"))
            .or_else(|| daemon.then(|| "127.0.0.1:7373".to_string())),
        viz: a
            .viz
            .or_else(|| env("SUPRAGNOSIS_VIZ_ADDR"))
            .or_else(|| daemon.then(|| "127.0.0.1:7374".to_string())),
        host,
    }
}

fn default_data_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.supragnosis/db")
}

/// Default embedder kind based on the compiled features. If built with fastembed enabled, that is the default.
fn default_embed_kind() -> &'static str {
    if cfg!(feature = "fastembed") {
        "fastembed"
    } else {
        "none"
    }
}

/// Selects the embedding provider from the SUPRAGNOSIS_EMBED value. Failure/absence yields None (degrade to keyword search).
fn build_embedder(kind: &str) -> Option<Arc<dyn EmbeddingProvider>> {
    match kind {
        "none" | "" => None,
        // Deterministic but non-semantic (lexical hashing) - a development/offline stand-in.
        "hashing" => {
            tracing::info!("embed=hashing (deterministic/lexical, for development)");
            Some(Arc::new(HashingEmbedder::default()))
        }
        "fastembed" => build_fastembed(),
        other => {
            tracing::warn!(kind = other, "unknown SUPRAGNOSIS_EMBED - proceeding with keyword search");
            None
        }
    }
}

#[cfg(feature = "fastembed")]
fn build_fastembed() -> Option<Arc<dyn EmbeddingProvider>> {
    match supragnosis_embed::FastEmbedProvider::try_default() {
        Ok(p) => {
            tracing::info!("embed=fastembed (BGE-small-en-v1.5, 384d)");
            Some(Arc::new(p))
        }
        Err(e) => {
            tracing::warn!(error = %e, "fastembed initialization failed - proceeding with keyword search");
            None
        }
    }
}

#[cfg(not(feature = "fastembed"))]
fn build_fastembed() -> Option<Arc<dyn EmbeddingProvider>> {
    tracing::warn!("fastembed feature not compiled in - build with `--features fastembed`. Proceeding with keyword search");
    None
}

/// Assembles the store/embedder/engine from the configuration. If `events` is present, attaches a UI event sink (the viewer).
fn build_engine(
    cfg: &Config,
    events: Option<&tokio::sync::broadcast::Sender<String>>,
) -> Result<Arc<Engine>> {
    let embedder = build_embedder(&cfg.embed_kind);
    let embed_dim = embedder.as_ref().map(|e| e.dimensions());
    let store: Arc<dyn KnowledgeStore> = match cfg.store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (non-persistent)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            // Record/check the embedder identifier (model + dimensions) in the store -
            // an explicit failure instead of silent corruption when reopened with a
            // different embedder.
            let store = match &embedder {
                Some(e) => CozoStore::open_with_embedder(&cfg.data_dir, &e.id(), e.dimensions()),
                None => CozoStore::open(&cfg.data_dir),
            }
            .with_context(|| format!("failed to open Cozo store at {}", cfg.data_dir))?;
            tracing::info!(data_dir = %cfg.data_dir, ?embed_dim, "store=cozo (RocksDB, persistent)");
            Arc::new(store)
        }
    };
    let mut engine =
        Engine::new(store, cfg.host.clone(), cfg.workspace.clone()).with_session(cfg.session.clone());
    if let Some(e) = embedder {
        engine = engine.with_embedder(e);
    }
    if let Some(tx) = events {
        engine = engine.with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone())));
    }
    Ok(Arc::new(engine))
}

/// Actual server run (async). With http, a streamable-http daemon; without it,
/// stdio. With viz, the live viewer is started alongside it in the same process.
async fn run(cfg: Config) -> Result<()> {
    // Create the event channel only when the viewer is present - the engine sink and SSE subscription share it.
    let events = cfg
        .viz
        .as_ref()
        .map(|_| tokio::sync::broadcast::channel::<String>(256).0);
    let engine = build_engine(&cfg, events.as_ref())?;

    if let (Some(addr), Some(tx)) = (cfg.viz.as_ref(), events.as_ref()) {
        spawn_viz(&engine, addr, tx.clone()).await;
    }

    // Federation wiring (M4 Phase 4, docs/federation.md Section 9): optional supragnosis.toml.
    // Absent = standalone node (no behavior change); present-but-broken = fail loud (P5).
    let sync_ctx = build_sync_context(&engine, fed::load()?)?;

    match cfg.http.as_deref() {
        Some(http) => {
            serve_http_daemon(engine, sync_ctx, http, &cfg.host, &cfg.workspace, &cfg.session).await
        }
        None => {
            tracing::info!(host = %cfg.host, workspace = %cfg.workspace, session = %cfg.session, "supragnosis / starting stdio MCP server");
            let mut server = SupragnosisServer::new(engine);
            if let Some(ctx) = sync_ctx {
                server = server.with_sync(ctx);
            }
            let service = server.serve(stdio()).await?;
            service.waiting().await?;
            Ok(())
        }
    }
}

/// Builds the MCP sync context from supragnosis.toml (and starts the sync API server when a
/// `[server]` section is present). Returns None on a standalone node.
fn build_sync_context(
    engine: &Arc<Engine>,
    fedcfg: Option<fed::FileConfig>,
) -> Result<Option<Arc<supragnosis_mcp::SyncContext>>> {
    let Some(fc) = fedcfg else { return Ok(None) };
    // One identity + one SyncNode per process - the server role and the sync tools share the HLC
    // clock and the per-workspace seq counters (two live counters over one store would collide).
    let identity = fed::load_or_create_identity()?;
    let node = Arc::new(supragnosis_sync::SyncNode::new(identity));
    tracing::info!(node_id = %node.node_id(), "federation identity loaded");
    if let Some(srv) = &fc.server {
        // Post-apply hook: re-materialize the workspace after inbound pushes (Prop C).
        let hook_engine = engine.clone();
        let on_applied: supragnosis_sync::http::OnApplied = Arc::new(move |ws: &str| {
            match hook_engine.reproject(Some(ws)) {
                Ok(r) => tracing::info!(workspace = ws, entities = r.entities, relations = r.relations, "re-materialized after inbound sync"),
                Err(e) => tracing::error!(workspace = ws, error = %e, "re-materialization after inbound sync failed"),
            }
        });
        spawn_sync_server(engine.store(), node.clone(), srv.clone(), on_applied)?;
    }
    let mut origin_keys = fc.sync.origin_keys.clone();
    origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
    Ok(Some(Arc::new(supragnosis_mcp::SyncContext {
        node,
        share_workspaces: fc.sync.share_workspaces.clone(),
        servers: fc.sync.servers.clone(),
        auth_token: fc.sync.auth_token.clone().unwrap_or_default(),
        insecure_tls: fc.sync.insecure_tls,
        origin_keys,
    })))
}

/// Starts the federation sync API (the ONLY surface allowed to bind non-loopback - and only with
/// TLS + a non-empty allowlist, F10). A misconfigured [server] section fails daemon startup loudly
/// (P5) instead of silently running without the role.
fn spawn_sync_server(
    store: Arc<dyn KnowledgeStore>,
    node: Arc<supragnosis_sync::SyncNode>,
    srv: fed::ServerSection,
    on_applied: supragnosis_sync::http::OnApplied,
) -> Result<()> {
    use supragnosis_sync::http as sync_http;
    let listen: std::net::SocketAddr = srv
        .listen
        .parse()
        .with_context(|| format!("invalid [server] listen address: {:?} (IP:port)", srv.listen))?;
    let tls = match (&srv.tls_cert, &srv.tls_key) {
        (Some(c), Some(k)) => Some(sync_http::TlsPaths { cert_pem: c.into(), key_pem: k.into() }),
        (None, None) => None,
        _ => anyhow::bail!("[server] tls_cert and tls_key must be set together"),
    };
    // Validate at startup so a misconfigured daemon dies here, not inside a spawned task (F10).
    sync_http::validate_bind(&listen, tls.is_some(), srv.allowlist.len())?;
    tracing::info!(%listen, allowlist = srv.allowlist.len(), tls = tls.is_some(), "starting federation sync API");
    tokio::spawn(async move {
        if let Err(e) = sync_http::serve(store, node, listen, tls, srv.allowlist, Some(on_applied)).await {
            tracing::error!(error = %e, "federation sync API terminated");
        }
    });
    Ok(())
}

/// `supragnosis identity` - prints the node's federation identity (generating the keypair on first
/// use); --hash-token prints what a server allowlist entry stores for a peer's bearer token.
fn identity_cmd(a: IdentityArgs) -> Result<()> {
    init_tracing();
    let id = fed::load_or_create_identity()?;
    println!("node_id:     {}", id.node_id());
    println!("public_key:  {}", id.public_key_hex());
    if let Some(tok) = a.hash_token {
        println!("bearer_hash: {}", blake3::hash(tok.as_bytes()).to_hex());
    }
    Ok(())
}

/// `supragnosis migrate` - one-shot legacy-id migration (docs/federation.md: a stored id that
/// predates the current content-address formula cannot verify remotely; the row is re-created under
/// the current id with lineage back to the legacy row). Re-materializes afterwards.
fn migrate_cmd(a: SyncArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let migrated = supragnosis_sync::migrate_legacy_ids(engine.store().as_ref(), &ws)?;
        let r = engine.reproject(Some(&ws))?;
        println!(
            "migrated {} legacy-id observation(s); reprojected {}: {} observations -> {} entities, {} relations",
            migrated, ws, r.observations, r.entities, r.relations
        );
        anyhow::Ok(())
    })
}

/// `supragnosis reproject` - one-shot re-materialization (HLC-ordered replay, Prop C). For a node
/// whose log advanced without projection (e.g. a hub that received pushes before the reproject hook
/// existed). The store is single-process: stop the daemon first.
fn reproject_cmd(a: SyncArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let r = engine.reproject(Some(&ws))?;
        println!(
            "reprojected {}: {} observations -> {} entities, {} relations",
            ws, r.observations, r.entities, r.relations
        );
        anyhow::Ok(())
    })
}

/// `supragnosis sync` - one full sync round (push surplus, pull deficit, re-materialize) against
/// every configured server. Requires supragnosis.toml with [sync] servers + auth_token. The store
/// is single-process (RocksDB lock): with a running daemon, use the sync_* MCP tools instead.
fn sync_cmd(a: SyncArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let fc = fed::load()?.ok_or_else(|| {
        anyhow::anyhow!(
            "no federation config at {} - create it with a [sync] section (servers, auth_token, \
             share_workspaces). See docs/federation.md Section 9",
            fed::config_path().display()
        )
    })?;
    if fc.sync.servers.is_empty() {
        anyhow::bail!("[sync] servers is empty - nothing to sync against");
    }
    let token = fc
        .sync
        .auth_token
        .clone()
        .ok_or_else(|| anyhow::anyhow!("[sync] auth_token is not set"))?;
    let identity = fed::load_or_create_identity()?;
    let node = supragnosis_sync::SyncNode::new(identity);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let store = engine.store();
        let mut keys = fc.sync.origin_keys.clone();
        keys.insert(node.node_id().to_string(), node.public_key_hex());
        for server in &fc.sync.servers {
            let client = supragnosis_sync::http::SyncClient::new(server, &token, fc.sync.insecure_tls)?;
            let s = client
                .sync_workspace(&store, &node, &ws, &fc.sync.share_workspaces, &keys)
                .await?;
            println!(
                "{server}: pushed {} pulled {} (rejected: by server {}, locally {})",
                s.pushed, s.pulled, s.rejected_by_server, s.rejected_locally
            );
        }
        let r = engine.reproject(Some(&ws))?;
        println!(
            "reprojected {}: {} observations -> {} entities, {} relations",
            ws, r.observations, r.entities, r.relations
        );
        anyhow::Ok(())
    })
}

/// Initializes the stderr log subscriber (idempotent). stdout is the MCP stdio channel, so logs must go to stderr.
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

/// Builds a tokio runtime and runs [`run`] in blocking fashion. Manual construction
/// instead of `#[tokio::main]` so that start's daemonization (fork) can happen
/// **before** the runtime is created (prevents a broken runtime after fork).
fn run_blocking(cfg: Config) -> Result<()> {
    init_tracing();
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(run(cfg))
}

/// Starts the live ontology viewer as an opt-in. A bind/configuration failure is
/// only logged and does not block server startup (the viewer is an auxiliary channel
/// - Principle 21). `events` is the same broadcast Sender as the engine sink.
async fn spawn_viz(
    engine: &Arc<Engine>,
    addr_str: &str,
    events: tokio::sync::broadcast::Sender<String>,
) {
    // SUPRAGNOSIS_VIZ_PUBLIC=1: the owner's explicit opt-in to read-only network exposure of the
    // viewer (federation.md 6d interim; writes stay loopback-gated per connection, F19).
    let viz_public = std::env::var("SUPRAGNOSIS_VIZ_PUBLIC")
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false);
    let addr = match supragnosis_viz::parse_viz_addr(addr_str, viz_public) {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(error = %e, "ignoring SUPRAGNOSIS_VIZ_ADDR - proceeding without the viewer");
            return;
        }
    };
    if !addr.ip().is_loopback() {
        tracing::warn!(
            %addr,
            "viewer exposed beyond loopback (owner opt-in, READ-ONLY: /api/review stays \
             loopback-gated) - the authenticated read tier is federation Phase 3.5"
        );
    }
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, %addr, "viz bind failed - proceeding without the viewer");
            return;
        }
    };
    let bound = listener.local_addr().unwrap_or(addr);
    tracing::info!("ontology viewer started: http://{bound}");
    let engine = Arc::clone(engine);
    tokio::spawn(async move {
        if let Err(e) = supragnosis_viz::serve(engine, listener, events).await {
            tracing::error!(error = %e, "viz server terminated");
        }
    });
}

/// Standalone daemon: keeps the MCP streamable-http server running continuously. A
/// factory builds a `SupragnosisServer` per session while sharing the same
/// `Arc<Engine>` (same db). Loopback-only bind (Principle 17: local trust surface -
/// no authentication is justified).
async fn serve_http_daemon(
    engine: Arc<Engine>,
    sync_ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    http_addr: &str,
    host: &str,
    workspace: &str,
    session: &str,
) -> Result<()> {
    let addr = supragnosis_viz::parse_local_addr(http_addr)?; // reject non-local binds (Principle 17)
    let service = StreamableHttpService::new(
        move || {
            let mut server = SupragnosisServer::new(engine.clone());
            if let Some(ctx) = &sync_ctx {
                server = server.with_sync(ctx.clone());
            }
            Ok(server)
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind MCP daemon at {addr}"))?;
    tracing::info!(%host, %workspace, %session, %addr, "supragnosis / MCP streamable-http daemon: http://{addr}/mcp");
    axum::serve(listener, router).await?;
    Ok(())
}

// --- Background daemon lifecycle (start/stop/restart/status) -------------------------
// Self-managed via a pidfile + logs. Uses only kill (-0/SIGTERM)/TcpStream, so no unsafe/libc is needed.

#[cfg(unix)]
fn base_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".supragnosis")
}
#[cfg(unix)]
fn pid_path() -> std::path::PathBuf {
    base_dir().join("supragnosis.pid")
}
#[cfg(unix)]
fn log_dir() -> std::path::PathBuf {
    base_dir().join("log")
}
#[cfg(unix)]
fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}
/// Checks whether the process is alive via `kill -0` (portable, without unsafe/libc).
#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
/// Whether a listener is at the address (a successful connection attempt = in use).
#[cfg(unix)]
fn port_open(addr: &str) -> bool {
    use std::net::ToSocketAddrs;
    addr.to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|sa| {
            std::net::TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(300)).is_ok()
        })
        .unwrap_or(false)
}

// --- launchd (macOS) awareness -------------------------------------------------------
// The deploy LaunchAgent supervises the daemon out-of-band (no pidfile). These helpers let
// the lifecycle commands detect and drive it, so `supragnosis restart/stop` control the
// actual running instance instead of a separate self-managed daemon.

/// launchd label used by the deploy LaunchAgent (deploy/launchd/<label>.plist).
#[cfg(target_os = "macos")]
const LAUNCHD_LABEL: &str = "com.supragnosis.daemon";

/// User id for the `gui/<uid>` launchd domain target (via `id -u` - no libc/unsafe).
#[cfg(target_os = "macos")]
fn launchd_uid() -> Option<String> {
    let out = std::process::Command::new("id").arg("-u").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let uid = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!uid.is_empty()).then_some(uid)
}

/// Whether a launchd job with our label is currently loaded for this user.
#[cfg(target_os = "macos")]
fn launchd_loaded() -> bool {
    std::process::Command::new("launchctl")
        .arg("list")
        .arg(LAUNCHD_LABEL)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Restart the launchd daemon in place (`kickstart -k`). The plist environment
/// (including SUPRAGNOSIS_VIZ_ADDR) is re-applied, so the viewer comes back too.
#[cfg(target_os = "macos")]
fn launchd_kickstart() -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{LAUNCHD_LABEL}");
    let st = std::process::Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl kickstart")?;
    if !st.success() {
        anyhow::bail!("launchctl kickstart {target} failed");
    }
    println!("restarted launchd daemon {LAUNCHD_LABEL} (MCP server + viewer).");
    Ok(())
}

/// Stop the launchd daemon (`bootout`). It stays down until reloaded (so KeepAlive
/// does not respawn it - unlike a bare SIGTERM).
#[cfg(target_os = "macos")]
fn launchd_bootout() -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{LAUNCHD_LABEL}");
    let st = std::process::Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl bootout")?;
    if !st.success() {
        anyhow::bail!("launchctl bootout {target} failed (already stopped?).");
    }
    println!("stopped launchd daemon {LAUNCHD_LABEL}. It stays down until reloaded (deploy/install.sh or launchctl bootstrap).");
    Ok(())
}

/// Resolved MCP http address for status/lifecycle checks (env var or default).
#[cfg(unix)]
fn status_http_addr() -> String {
    std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string())
}

#[cfg(unix)]
fn start(cfg: Config) -> Result<()> {
    let http = cfg
        .http
        .clone()
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            anyhow::bail!("already running (pid {pid}). Run 'supragnosis stop' and try again.");
        }
    }
    if port_open(&http) {
        anyhow::bail!(
            "{http} is already in use (another instance or a launchd daemon?). Stop it or use a different address with --http."
        );
    }
    std::fs::create_dir_all(log_dir()).with_context(|| "failed to create log directory")?;
    let out = std::fs::File::create(log_dir().join("supragnosis.out.log"))?;
    let err = std::fs::File::create(log_dir().join("supragnosis.err.log"))?;
    let viz_msg = cfg
        .viz
        .as_deref()
        .map(|v| format!("http://{v}"))
        .unwrap_or_else(|| "(off)".to_string());
    println!("supragnosis daemon started - MCP http://{http}/mcp  viewer {viz_msg}");
    println!("  pidfile {}  logs {}", pid_path().display(), log_dir().display());
    // fork/setsid/pidfile/stdio redirect. The code after this runs only in the daemonized child.
    daemonize::Daemonize::new()
        .pid_file(pid_path())
        .stdout(out)
        .stderr(err)
        .start()
        .map_err(|e| anyhow::anyhow!("daemonization failed: {e}"))?;
    run_blocking(cfg)
}

/// Sends SIGTERM to the self-managed (pidfile) daemon and waits for graceful exit.
#[cfg(unix)]
fn stop_pidfile(pid: i32) -> Result<()> {
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| "failed to run kill")?;
    // Wait for shutdown (up to ~10s) - a graceful exit after SIGTERM.
    for _ in 0..50 {
        if !pid_alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if pid_alive(pid) {
        anyhow::bail!("timed out waiting for stop (pid {pid}). Check manually: kill {pid}");
    }
    let _ = std::fs::remove_file(pid_path());
    println!("daemon stopped (pid {pid}).");
    Ok(())
}

#[cfg(unix)]
fn stop() -> Result<()> {
    // (1) Self-managed (pidfile) daemon takes priority.
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            return stop_pidfile(pid);
        }
        let _ = std::fs::remove_file(pid_path()); // stale - fall through to the supervisor check
    }
    // (2) launchd-managed daemon (macOS): stop via bootout so KeepAlive does not respawn it.
    #[cfg(target_os = "macos")]
    if launchd_loaded() {
        return launchd_bootout();
    }
    // (3) Something else is responding but is not under our control.
    let http = status_http_addr();
    if port_open(&http) {
        anyhow::bail!("a daemon is responding on {http} but is not managed by this CLI or launchd - stop it via its own supervisor.");
    }
    println!("not running.");
    Ok(())
}

#[cfg(unix)]
fn restart(cfg: Config) -> Result<()> {
    // (1) Self-managed daemon: stop then start.
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            stop_pidfile(pid)?;
            std::thread::sleep(std::time::Duration::from_millis(400)); // wait for the port to release
            return start(cfg);
        }
        let _ = std::fs::remove_file(pid_path());
    }
    // (2) launchd-managed daemon (macOS): restart in place - the viewer returns via the plist env.
    #[cfg(target_os = "macos")]
    if launchd_loaded() {
        return launchd_kickstart();
    }
    // (3) Nothing under our control - start a fresh self-managed daemon (unless a stranger holds the port).
    let http = cfg
        .http
        .clone()
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    if port_open(&http) {
        anyhow::bail!("a daemon is responding on {http} but is not managed by this CLI or launchd - cannot restart it from here.");
    }
    start(cfg)
}

#[cfg(unix)]
fn status() -> Result<()> {
    let http = status_http_addr();
    let up = port_open(&http);
    // (1) Self-managed (pidfile) daemon.
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            println!("running (self-managed, pid {pid})");
            println!(
                "  MCP http://{http}/mcp  ({})",
                if up { "responding" } else { "port not responding" }
            );
            return Ok(());
        }
    }
    // (2) launchd-managed daemon (macOS) - controllable via supragnosis restart/stop.
    #[cfg(target_os = "macos")]
    if launchd_loaded() {
        println!("running (launchd: {LAUNCHD_LABEL})");
        println!(
            "  MCP http://{http}/mcp  ({})",
            if up { "responding" } else { "not responding" }
        );
        println!("  control: supragnosis restart | supragnosis stop");
        return Ok(());
    }
    // (3) External/unknown supervisor, or stopped.
    if up {
        println!("running (external; no pidfile, not launchd)");
        println!("  MCP http://{http}/mcp  (responding)");
        return Ok(());
    }
    match read_pid() {
        Some(pid) => println!("stopped (stale pidfile, pid {pid})"),
        None => println!("stopped"),
    }
    Ok(())
}

// Non-unix: daemon lifecycle unsupported - point to serve --http.
#[cfg(not(unix))]
fn start(_cfg: Config) -> Result<()> {
    anyhow::bail!("the background daemon (start) is supported only on unix (macOS/Linux). Use 'supragnosis serve --http <ADDR>'.")
}
#[cfg(not(unix))]
fn stop() -> Result<()> {
    anyhow::bail!("the background daemon is unix-only.")
}
#[cfg(not(unix))]
fn restart(_cfg: Config) -> Result<()> {
    anyhow::bail!("the background daemon is unix-only.")
}
#[cfg(not(unix))]
fn status() -> Result<()> {
    anyhow::bail!("the background daemon is unix-only.")
}

// --- Federation configuration + node identity (M4 Phase 4, docs/federation.md Section 9) ---------

mod fed {
    use anyhow::{Context, Result};
    use std::path::PathBuf;

    /// supragnosis.toml - federation wiring. Absent file = a standalone node (every field optional);
    /// a present-but-malformed file is a loud error (P5: explicit configuration must work or fail,
    /// never silently degrade). Unknown keys are rejected so a typo cannot silently disable a role.
    #[derive(Debug, Default, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct FileConfig {
        /// Display-only label. node_id derives from the keypair and is never configured (F14).
        #[allow(dead_code)]
        pub host_label: Option<String>,
        #[serde(default)]
        pub sync: SyncSection,
        pub server: Option<ServerSection>,
    }

    #[derive(Debug, Default, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct SyncSection {
        /// Outbound share whitelist (P17/F9: nothing leaves by default).
        #[serde(default)]
        pub share_workspaces: Vec<String>,
        /// Sync server (hub) base URLs, e.g. "https://10.60.16.75:7420".
        #[serde(default)]
        pub servers: Vec<String>,
        /// Bearer token presented to those servers.
        pub auth_token: Option<String>,
        /// Accept a self-signed hub certificate (internal VM) - content authenticity stays with the
        /// event signatures (F6), this only affects transport privacy against an active MITM.
        #[serde(default)]
        pub insecure_tls: bool,
        /// Origin-key directory {node_id -> public key hex} for verifying pulled events (F6).
        /// Superseded by the log-borne canon-policy binding in Phase 5.
        #[serde(default)]
        pub origin_keys: std::collections::BTreeMap<String, String>,
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ServerSection {
        /// Sync API bind (IP:port). Non-loopback demands TLS + a non-empty allowlist (F10).
        pub listen: String,
        pub tls_cert: Option<String>,
        pub tls_key: Option<String>,
        #[serde(default)]
        pub allowlist: Vec<supragnosis_sync::http::AllowEntry>,
    }

    fn fed_base_dir() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".supragnosis")
    }

    /// SUPRAGNOSIS_CONFIG, or ~/.supragnosis/supragnosis.toml.
    pub fn config_path() -> PathBuf {
        std::env::var("SUPRAGNOSIS_CONFIG")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| fed_base_dir().join("supragnosis.toml"))
    }

    pub fn load() -> Result<Option<FileConfig>> {
        let path = config_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(cfg))
    }

    /// Loads (or generates exactly once) the node keypair at ~/.supragnosis/node.key - 32 raw
    /// secret bytes, mode 0600. The node_id derives from the public key and never changes (F14).
    pub fn load_or_create_identity() -> Result<supragnosis_core::NodeIdentity> {
        let path = fed_base_dir().join("node.key");
        if path.exists() {
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("{} must be exactly 32 bytes", path.display()))?;
            return Ok(supragnosis_core::NodeIdentity::from_secret_bytes(arr));
        }
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret)
            .map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, secret)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        tracing::info!(path = %path.display(), "generated the node keypair (once - the node_id is immutable, F14)");
        Ok(supragnosis_core::NodeIdentity::from_secret_bytes(secret))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The documented supragnosis.toml shape parses; unknown keys are rejected loudly (P5).
        #[test]
        fn config_parses_and_rejects_typos() {
            let good = r#"
                host_label = "knowledge-vm"
                [sync]
                share_workspaces = ["supragnosis"]
                servers = ["https://10.60.16.75:7420"]
                auth_token = "tok"
                insecure_tls = true
                [sync.origin_keys]
                "abc" = "deadbeef"
                [server]
                listen = "0.0.0.0:7420"
                tls_cert = "/etc/supragnosis/cert.pem"
                tls_key = "/etc/supragnosis/key.pem"
                [[server.allowlist]]
                node_id = "abc"
                public_key_hex = "deadbeef"
                bearer_hash = "hash"
                shared_workspaces = ["supragnosis"]
            "#;
            let cfg: FileConfig = toml::from_str(good).expect("documented shape must parse");
            assert_eq!(cfg.sync.servers.len(), 1);
            assert!(cfg.sync.insecure_tls);
            let srv = cfg.server.expect("server section");
            assert_eq!(srv.allowlist.len(), 1);
            assert_eq!(cfg.sync.origin_keys.get("abc").map(String::as_str), Some("deadbeef"));

            // A typo must fail loudly, not silently disable a role (P5).
            let typo = "share_workspace = [\"x\"]\n";
            assert!(toml::from_str::<FileConfig>(typo).is_err());
        }
    }
}
