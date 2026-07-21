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

    match cfg.http.as_deref() {
        Some(http) => serve_http_daemon(engine, http, &cfg.host, &cfg.workspace, &cfg.session).await,
        None => {
            tracing::info!(host = %cfg.host, workspace = %cfg.workspace, session = %cfg.session, "supragnosis / starting stdio MCP server");
            let service = SupragnosisServer::new(engine).serve(stdio()).await?;
            service.waiting().await?;
            Ok(())
        }
    }
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
    let addr = match supragnosis_viz::parse_local_addr(addr_str) {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(error = %e, "ignoring SUPRAGNOSIS_VIZ_ADDR - proceeding without the viewer");
            return;
        }
    };
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
    http_addr: &str,
    host: &str,
    workspace: &str,
    session: &str,
) -> Result<()> {
    let addr = supragnosis_viz::parse_local_addr(http_addr)?; // reject non-local binds (Principle 17)
    let service = StreamableHttpService::new(
        move || Ok(SupragnosisServer::new(engine.clone())),
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

#[cfg(unix)]
fn stop() -> Result<()> {
    let Some(pid) = read_pid() else {
        println!("not running (no pidfile).");
        return Ok(());
    };
    if !pid_alive(pid) {
        let _ = std::fs::remove_file(pid_path());
        println!("not running (cleaned up stale pidfile, pid {pid}).");
        return Ok(());
    }
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| "failed to run kill")?;
    // Wait for shutdown (up to ~10s). Waits for a graceful exit after SIGTERM.
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
fn restart(cfg: Config) -> Result<()> {
    let _ = stop();
    std::thread::sleep(std::time::Duration::from_millis(400)); // wait for the port to be released
    start(cfg)
}

#[cfg(unix)]
fn status() -> Result<()> {
    let http = std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    let up = port_open(&http);
    match read_pid() {
        // A daemon managed by this CLI (start).
        Some(pid) if pid_alive(pid) => {
            println!("running (pid {pid})");
            println!(
                "  MCP http://{http}/mcp  ({})",
                if up { "responding" } else { "port not responding" }
            );
        }
        // No pidfile but the port responds - an externally managed daemon (e.g. launchd serve, another instance).
        _ if up => {
            println!("running (external; no pidfile - e.g. launchd serve)");
            println!("  MCP http://{http}/mcp  (responding)");
        }
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
