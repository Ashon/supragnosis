//! supragnosis executable (single-binary CLI).
//!
//! Subcommands control the server. Launched **with no arguments** through a pipe, it starts a
//! stdio MCP server - backward compatibility for an MCP client that launches it as a child
//! process. Typed in a terminal with no arguments, it prints its help.
//!
//!   supragnosis                  stdio MCP server through a pipe; help in a terminal
//!   supragnosis serve [options]   foreground run (--http for a streamable-http daemon, --viz for the viewer)
//!   supragnosis start [options]   start the background daemon (default MCP 127.0.0.1:7373 + viewer socket ~/.supragnosis/viz.sock)
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
//! stop/restart/status are supervisor-aware: they recognize every manager the product has
//! installed - the pidfile, the canonical LaunchAgent, Homebrew's `brew services` job and the
//! retired labels - and act on whichever single one is present (restart = kickstart -k, stop =
//! bootout for launchd). More than one is a conflict they report and refuse, rather than guess
//! (docs/daemon-lifecycle.md; the decisions live in `lifecycle.rs`).

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{stdio, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServiceExt;
use supragnosis_core::{EmbeddingProvider, KnowledgeStore, VersionVector};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, Event, SearchMode};
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::{InMemoryStore, RedbStore};

#[derive(Parser)]
#[command(
    name = "supragnosis",
    version,
    about = "MCP server that turns knowledge from many hosts/workspaces into an ontology"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Foreground run (stdio by default; --http for a streamable-http daemon)
    Serve(RunArgs),
    /// Start the background daemon (default MCP :7373 + viewer socket ~/.supragnosis/viz.sock)
    Start(RunArgs),
    /// Stop the background daemon
    Stop,
    /// Restart the daemon (stop then start)
    Restart(RunArgs),
    /// Query daemon status: who manages it, whether it answers, and which version it runs
    Status(StatusArgs),
    /// Stdio MCP server that relays to the running daemon - what an AI app launches. Opens no store
    /// and holds no token in the app's config (docs/client-connect.md)
    Bridge,
    /// Connect an AI app (Claude Desktop, Claude Code, Cursor, VS Code, Codex, Gemini, OpenCode) to
    /// this node through the bridge; with no app, list them and how each is connected
    Connect(ConnectArgs),
    /// From install to the first entry: keep the daemon running, connect your AI apps, download the
    /// search model, and try it. Shows the plan and asks first (docs/onboarding.md)
    Setup(SetupArgs),
    /// The embedding model behind semantic retrieval (docs/semantic-retrieval.md)
    Embed {
        #[command(subcommand)]
        cmd: EmbedCmd,
    },
    /// Answer a Claude Code hook event from stdin: after a commit the agent made, ask it for the
    /// decision behind the change; at a session's start, say what the workspace holds here.
    /// Registered by `connect claude-code` (docs/client-connect.md Section 12)
    Hook(HookArgs),
    /// Let another machine join this one: prints a one-time code, then each request's words and
    /// asks to allow it (docs/pairing.md Section 3a)
    Invite(InviteArgs),
    /// Join another machine's hub with the code it gave: reads the code on stdin, shows the words
    /// to compare, and on the hub's Allow starts syncing (docs/pairing.md Section 4.3)
    Join(JoinArgs),
    /// The hub this machine joined: leave it (docs/pairing.md Section 4.6)
    Hub {
        #[command(subcommand)]
        cmd: HubCmd,
    },
    /// The machines that joined this hub: list them, or remove one (docs/pairing.md Section 4.6)
    Peer {
        #[command(subcommand)]
        cmd: PeerCmd,
    },
    /// The always-on daemon as a login item (the LaunchAgent com.supragnosis.daemon on macOS, the systemd user unit supragnosis.service on Linux)
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Show this node's federation identity (node id + public key); --hash-token hashes a bearer token for an allowlist entry
    Identity(IdentityArgs),
    /// One-shot federation sync round against the configured servers (requires supragnosis.toml; stop the daemon first - the store is single-process)
    Sync(WorkspaceArgs),
    /// Re-materialize a workspace's entity/relation projection from the observation log (HLC-ordered replay; stop the daemon first)
    Reproject(WorkspaceArgs),
    /// Migrate legacy-id observations (pre-0.1.x content-address eras) to the current formula so they can sync (stop the daemon first)
    Migrate(WorkspaceArgs),
    /// Re-create a workspace's knowledge under another name, provenance intact (stop the daemon first)
    RekeyWorkspace(RekeyArgs),
}

#[derive(Args, Clone)]
struct RekeyArgs {
    /// Workspace to read from.
    #[arg(long)]
    from: String,
    /// Workspace to re-create the knowledge under.
    #[arg(long)]
    to: String,
    /// Report what would move and write nothing.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Args, Default)]
struct InviteArgs {
    /// Workspaces the two machines keep in step, comma-separated; none shares nothing yet
    #[arg(long)]
    keep: Option<String>,
    /// How the joining machine reaches this one - a DNS, Tailscale or VPN name, or an address;
    /// repeatable. Without it the code carries the .local name and the IPv4 address
    #[arg(long)]
    address: Vec<String>,
    /// Print the open invitation and any waiting request, and ask again
    #[arg(long)]
    show: bool,
    /// End the open invitation
    #[arg(long)]
    cancel: bool,
    /// Print the invitation as JSON and return at once, without waiting (the desktop app reads this)
    #[arg(long)]
    json: bool,
}

#[derive(Args, Default)]
struct JoinArgs {
    /// Workspaces this machine also sends to the hub, comma-separated; asked one by one in a
    /// terminal when omitted. It receives every workspace the invitation keeps either way
    #[arg(long)]
    send: Option<String>,
    /// The name the hub shows for this machine (default: this machine's name)
    #[arg(long)]
    name: Option<String>,
    /// One JSON object per line - the preview, the words, the outcome - and no questions (the
    /// desktop app reads this)
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum HubCmd {
    /// Stop syncing with the hub this machine joined, and ask it to remove this machine
    Leave {
        /// Do not ask first
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum PeerCmd {
    /// The machines that joined this hub, and those removed
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove a joined machine: it stops syncing with this one now, and keeps what it received
    Remove {
        /// The name it joined with, or its node id
        name: String,
        /// Do not ask first
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum EmbedCmd {
    /// Download the model into ~/.supragnosis/models, once. `service install` does this too; the
    /// daemon never downloads, and says so in `status` while the model is missing
    Fetch {
        /// One JSON object per line instead of a progress bar: the bytes on disk against the
        /// model's size, about twice a second (the desktop app reads this).
        #[arg(long)]
        events: bool,
    },
    /// Remove every stored vector and the model the store recorded, with the daemon stopped: the
    /// next start embeds the store with the model it runs (docs/semantic-retrieval.md Section 13)
    Reset,
    /// Rebuild the search index from the log: stop the daemon this CLI manages, reset the store and
    /// start the daemon again, which embeds every row with the model it runs (docs/semantic-retrieval.md
    /// Section 13)
    Rebuild,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// Generate the LaunchAgent (macOS) or the user unit (Linux) and load it: the daemon starts now and at every login
    Install(ServiceInstallArgs),
    /// Unload the LaunchAgent or the user unit and retire its file: the daemon stops and no longer starts at login
    Uninstall,
}

#[derive(Args, Clone, Default)]
struct ServiceInstallArgs {
    /// Retire every other manager first (a brew services job, a pidfile daemon, a retired label) and
    /// move a hand-written plist or a copied unit aside instead of refusing.
    #[arg(long)]
    take_over: bool,
    /// Set a SUPRAGNOSIS_* variable in the job's environment (repeatable). Variables a replaced plist
    /// set are carried forward on their own.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    env: Vec<String>,
    /// Do not download the search model; `supragnosis embed fetch` does it later. The daemon does
    /// not wait for it either way (`setup` downloads it as its own step).
    #[arg(long)]
    no_model: bool,
}

#[derive(Args, Clone, Default)]
struct SetupArgs {
    /// Apply the plan with its defaults, asking nothing. Never replaces an entry `connect` did not
    /// write, and never takes over a manager setup only reports.
    #[arg(long, conflicts_with = "check")]
    yes: bool,
    /// Only read, and print the checklist.
    #[arg(long, conflicts_with = "step")]
    check: bool,
    /// Apply one step: server, app=<id>, model or wait.
    #[arg(long, value_name = "STEP")]
    step: Option<String>,
    /// One JSON object per line instead of text (the desktop app reads this).
    #[arg(long)]
    events: bool,
    /// How long `--step wait` waits for the first entry, in seconds.
    #[arg(long, value_name = "SECONDS", default_value_t = 600)]
    timeout: u64,
    /// Claude Code only: connect it without the capture hook (docs/client-connect.md Section 12).
    #[arg(long)]
    no_capture: bool,
    /// The pid of the daemon the desktop app spawned, which this CLI cannot otherwise tell from a
    /// process it does not know. The app passes it.
    #[arg(long, value_name = "PID", hide = true)]
    app_daemon: Option<u32>,
}

#[derive(Args, Clone, Default)]
struct ConnectArgs {
    /// The AI app: claude-desktop, claude-code, cursor, vscode, codex, gemini or opencode. Omit to
    /// list them.
    client: Option<String>,
    /// Remove the supragnosis entry from the app instead of adding it.
    #[arg(long)]
    remove: bool,
    /// Replace a supragnosis entry that is not the bridge - an HTTP entry holding a copy of the
    /// token, or the store-opening stdio server.
    #[arg(long)]
    replace: bool,
    /// Machine-readable listing (the desktop app reads this).
    #[arg(long)]
    json: bool,
    /// Claude Code only: register the bridge without the capture hook (docs/client-connect.md
    /// Section 12).
    #[arg(long)]
    no_capture: bool,
}

#[derive(Args)]
struct HookArgs {
    /// The client whose hook event is on stdin: claude-code.
    client: String,
}

#[derive(Args, Clone, Default)]
struct StatusArgs {
    /// Machine-readable output (the desktop shell reads this).
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Default)]
struct IdentityArgs {
    /// Print the blake3 hash of this bearer token (what a server allowlist entry stores).
    #[arg(long, value_name = "TOKEN")]
    hash_token: Option<String>,
}

/// What `sync`, `reproject` and `migrate` take: the workspace each acts on.
#[derive(Args, Clone, Default)]
struct WorkspaceArgs {
    /// Workspace to act on (default: the node default workspace).
    #[arg(long)]
    workspace: Option<String>,
}

/// Shared run options for serve/start/restart. When unspecified, resolved in the order SUPRAGNOSIS_* environment variable -> default.
#[derive(Args, Clone, Default)]
struct RunArgs {
    /// MCP streamable-http bind address (loopback). When omitted, serve uses stdio and start uses 127.0.0.1:7373.
    #[arg(long, value_name = "ADDR")]
    http: Option<String>,
    /// Live ontology viewer unix socket path. start defaults to ~/.supragnosis/viz.sock.
    #[arg(long, value_name = "PATH")]
    viz: Option<String>,
    /// Store: redb (default, file-persistent) | mem (non-persistent).
    #[arg(long)]
    store: Option<String>,
    /// Store directory (default ~/.supragnosis/redb).
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
    /// MCP daemon bearer auth: on (default) | off. `off` exposes the full tool surface to every
    /// local OS account on this host - loopback confines the surface to the host, not to a user.
    #[arg(long, value_name = "on|off")]
    mcp_auth: Option<String>,
}

// The launchd half of the lifecycle is macOS-only. Elsewhere its parsing and plist generation stay
// compiled and tested (CI runs on Linux) but are never called, which is not dead code to fix.
mod bridge;
mod connect;
mod hook;
mod hub;
mod invite;
mod join;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod lifecycle;
mod pair;
mod setup;

mod config;
mod connect_cli;
mod embedding;
mod engine_setup;
mod pairing_cli;
mod setup_cli;
mod store_cmds;
mod sync_wiring;

use config::*;
use connect_cli::*;
use embedding::*;
use engine_setup::*;
use pairing_cli::*;
use setup_cli::*;
use store_cmds::*;
use sync_wiring::*;

fn main() -> Result<()> {
    let cmd = match Cli::parse().cmd {
        Some(cmd) => cmd,
        // Bare, from a terminal: a person asking what the command does. The store-opening stdio
        // server is what a client launches, through a pipe; started here it would take the store
        // lock and wait on the keyboard (client-connect.md Section 6).
        None if std::io::IsTerminal::is_terminal(&std::io::stdin()) => {
            // As clap's own --help does, a reader that closed early (`| head`) is not an error.
            let _ = <Cli as clap::CommandFactory>::command().print_help();
            return Ok(());
        }
        None => Cmd::Serve(RunArgs::default()),
    };
    match cmd {
        Cmd::Serve(a) => run_blocking(resolve(a, false)),
        Cmd::Start(a) => start(resolve(a, true)),
        Cmd::Stop => stop(),
        Cmd::Restart(a) => restart(resolve(a, true)),
        Cmd::Status(a) => status(a.json),
        Cmd::Bridge => bridge_cmd(),
        Cmd::Connect(a) => connect_cmd(a),
        Cmd::Setup(a) => setup_cmd(a),
        Cmd::Embed { cmd: EmbedCmd::Fetch { events } } => embed_fetch(true, events),
        Cmd::Embed { cmd: EmbedCmd::Reset } => embed_reset(),
        Cmd::Embed { cmd: EmbedCmd::Rebuild } => embed_rebuild(),
        Cmd::Hook(a) => hook_cmd(a),
        Cmd::Invite(a) => invite_cmd(a),
        Cmd::Join(a) => join_cmd(a),
        Cmd::Hub { cmd } => hub_cmd(cmd),
        Cmd::Peer { cmd } => peer_cmd(cmd),
        Cmd::Service { cmd: ServiceCmd::Install(a) } => service_install(a),
        Cmd::Service { cmd: ServiceCmd::Uninstall } => service_uninstall(),
        Cmd::Identity(a) => identity_cmd(a),
        Cmd::Sync(a) => sync_cmd(a),
        Cmd::Reproject(a) => reproject_cmd(a),
        Cmd::Migrate(a) => migrate_cmd(a),
        Cmd::RekeyWorkspace(a) => rekey_workspace_cmd(a),
    }
}

/// Actual server run (async). With http, a streamable-http daemon; without it,
/// stdio. With viz, the live viewer is started alongside it in the same process.
async fn run(cfg: Config) -> Result<()> {
    let fedcfg = fed::load()?;
    // The event channel the engine emits into, for the local viewer.
    let events = Some(tokio::sync::broadcast::channel::<String>(256).0);
    let engine = build_engine(&cfg, events.as_ref())?;
    // What was written before the model gets its vectors once the model is there
    // (semantic-retrieval.md Section 12): a thread of its own, since it embeds and waits, and only
    // in a serving process - the one-shot commands that also build an engine do not start it.
    {
        let engine = engine.clone();
        std::thread::Builder::new()
            .name("embed-backfill".into())
            .spawn(move || engine.backfill_when_ready(std::time::Duration::from_secs(30), 8))
            .context("starting the embedding backfill")?;
    }

    // Federation status blob (viewer /api/federation) - exists only when supragnosis.toml does.
    let fed_status: Option<supragnosis_viz::FedStatus> = fedcfg
        .as_ref()
        .map(|_| Arc::new(std::sync::RwLock::new(serde_json::json!({"configured": true}))));

    // Federation wiring (M4 Phase 4, docs/federation.md Section 9): optional supragnosis.toml.
    // Absent = standalone node (no behavior change); present-but-broken = fail loud (P5). Built
    // before the viewer so a misconfigured federation dies before any socket is bound, and so the
    // console can be handed the admission handler this produces.
    // The hub's listener state: there from the start, so a hub an invitation makes says what its
    // listener is doing at once, in `sync_status` too (docs/pairing.md Section 4.1).
    let listener_state: Arc<std::sync::RwLock<serde_json::Value>> = Default::default();
    let (sync_ctx, narrow, hub_slot) =
        build_sync_context(&engine, fedcfg, fed_status.clone(), listener_state.clone()).await?;
    if let Some(ctx) = sync_ctx.as_ref().filter(|c| !c.servers.is_empty()) {
        spawn_sync_rounds(engine.clone(), ctx.clone());
    }
    // This user's processes reach the running daemon here: Sync now, the hub re-reading its
    // admissions, and pairing's acts (docs/pairing.md Section 4).
    hub::spawn_admin(engine.clone(), sync_ctx.clone(), hub_slot).await;

    if let (Some(sock), Some(tx)) = (cfg.viz.as_ref(), events.as_ref()) {
        spawn_viz(&engine, sock, tx.clone(), fed_status, narrow).await;
    }

    match cfg.http.as_deref() {
        Some(http) => {
            serve_http_daemon(
                engine,
                SyncSurface { ctx: sync_ctx, listener: listener_state },
                http,
                cfg.mcp_auth,
                &cfg.host,
                &cfg.workspace,
                &cfg.session,
            )
            .await
        }
        None => {
            tracing::info!(host = %cfg.host, workspace = %cfg.workspace, session = %cfg.session, "supragnosis / starting stdio MCP server");
            let mut server = SupragnosisServer::new(engine).with_listener(listener_state);
            if let Some(ctx) = sync_ctx {
                server = server.with_sync(ctx);
            }
            let service = server.serve(stdio()).await?;
            service.waiting().await?;
            Ok(())
        }
    }
}

/// Whether a URL's host is this machine: `localhost`, or a loopback address in either family.
fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A credential file's content, trimmed; `None` when absent or empty.
fn read_secret(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

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
    sock_path: &str,
    events: tokio::sync::broadcast::Sender<String>,
    fed: Option<supragnosis_viz::FedStatus>,
    narrow: Option<supragnosis_viz::NarrowShare>,
) {
    // The TCP viewer is gone: the viewer serves HTTP over a unix socket only. Point out stale
    // configuration loudly instead of silently ignoring it (Principle 5).
    for gone in ["SUPRAGNOSIS_VIZ_ADDR", "SUPRAGNOSIS_VIZ_PUBLIC"] {
        if std::env::var_os(gone).is_some() {
            tracing::warn!(
                "{gone} is no longer supported - the viewer serves HTTP over a unix socket \
                 (SUPRAGNOSIS_VIZ_SOCK / --viz <path>); another machine reaches this knowledge by \
                 syncing through a hub"
            );
        }
    }
    let path = std::path::PathBuf::from(sock_path);
    let listener = match supragnosis_viz::bind_uds(&path).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, path = %path.display(), "viz bind failed - proceeding without the viewer");
            return;
        }
    };
    tracing::info!(
        "ontology viewer socket: {} (e.g. curl --unix-socket {} http://viz/api/graph)",
        path.display(),
        path.display()
    );
    let engine = Arc::clone(engine);
    tokio::spawn(async move {
        let clients: supragnosis_viz::ClientsView = Arc::new(|| client_log().snapshot());
        if let Err(e) =
            supragnosis_viz::serve(engine, listener, events, fed, narrow, Some(clients)).await
        {
            tracing::error!(error = %e, "viz server terminated");
        }
    });
}

/// Standalone daemon: keeps the MCP streamable-http server running continuously. A
/// factory builds a `SupragnosisServer` per session while sharing the same
/// `Arc<Engine>` (same db).
///
/// Two guards, and they answer different questions. `parse_loopback_addr` refuses a non-local bind,
/// which keeps the surface on this HOST. A bearer token ([`require_token`]) keeps it to this USER,
/// which loopback never did - the sentence "loopback is the local trust surface, so no
/// authentication is justified" stood here and was wrong on any multi-user machine.
/// What the MCP surface reads about sync: the sync tools' context on a node with a hub, and the
/// hub's listener state on every node (docs/pairing.md Section 4.1).
struct SyncSurface {
    ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    listener: Arc<std::sync::RwLock<serde_json::Value>>,
}

async fn serve_http_daemon(
    engine: Arc<Engine>,
    sync: SyncSurface,
    http_addr: &str,
    auth: bool,
    host: &str,
    workspace: &str,
    session: &str,
) -> Result<()> {
    let addr = parse_loopback_addr(http_addr)?; // reject non-local binds (Principle 17)
    let token = match auth {
        true => {
            let token = load_or_create_mcp_token()?;
            tracing::info!(path = %mcp_token_path().display(), "MCP daemon requires a bearer token");
            Some(token)
        }
        false => {
            // Loud, every start, and naming what is exposed rather than that a setting is off. An
            // operator who chose this on a single-user box should see it confirmed; one who inherited
            // it from a stale environment variable should see what it costs.
            tracing::warn!(
                "MCP daemon authentication is DISABLED (SUPRAGNOSIS_MCP_AUTH=off) - every local OS \
                 account on this host can observe, review and sync_push through {addr}"
            );
            None
        }
    };
    let router = mcp_router(engine, sync.ctx, Some(sync.listener), token);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind MCP daemon at {addr}"))?;
    // The address it got, which differs from the one it was asked for when that was port 0.
    let bound = listener.local_addr().context("the MCP listener's address")?;
    tracing::info!(%host, %workspace, %session, addr = %bound, auth, "supragnosis / MCP streamable-http daemon: http://{bound}/mcp");
    if addr.port() == 0 {
        write_listening(&fed::fed_base_dir(), &[("mcp", bound)])?;
    }
    axum::serve(listener, router).with_graceful_shutdown(sigterm()).await?;
    Ok(())
}

/// Tells whoever asked for port 0 which port they got (docs/e2e.md Section 3.1): one line per
/// listener, `<name> <address>`, in `<base>/listening`. Written once the listener is bound, by
/// replacing the file whole, so a reader never sees half of it. A daemon given a fixed address
/// writes nothing - that one is already known.
fn write_listening(
    base: &std::path::Path,
    listeners: &[(&str, std::net::SocketAddr)],
) -> Result<()> {
    std::fs::create_dir_all(base).with_context(|| format!("creating {}", base.display()))?;
    let text: String = listeners.iter().map(|(name, addr)| format!("{name} {addr}\n")).collect();
    let path = base.join("listening");
    let tmp = base.join("listening.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Resolves when the process is asked to stop - SIGTERM, which `supragnosis stop`, launchd and the
/// desktop app send - and then gives requests in flight three seconds before the server stops waiting
/// for them. MCP sessions hold their event streams open, so a shutdown that waited for every
/// connection would never end. The daemon then exits normally, which is also what lets an
/// instrumented build write its coverage profile (docs/e2e.md Section 7).
async fn sigterm() {
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
                tracing::info!("SIGTERM - no longer accepting; exiting once requests in flight end, at most in 3s");
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    tracing::info!("exiting with connections still open");
                    std::process::exit(0);
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "SIGTERM cannot be caught - the daemon stops as the signal's default");
                std::future::pending::<()>().await;
            }
        }
    }
}

/// First contact (docs/onboarding.md Section 5): the one log this daemon process keeps of which AI
/// apps talked to it, shared by every MCP session and read by the viewer's `/api/clients`.
fn client_log() -> Arc<supragnosis_mcp::ClientLog> {
    static LOG: std::sync::OnceLock<Arc<supragnosis_mcp::ClientLog>> = std::sync::OnceLock::new();
    LOG.get_or_init(Default::default).clone()
}

/// The daemon's MCP router: the streamable-HTTP service behind its guards. Separate from the bind
/// so the bridge's tests can serve exactly what the daemon serves (client-connect.md C4).
fn mcp_router(
    engine: Arc<Engine>,
    sync_ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    listener: Option<Arc<std::sync::RwLock<serde_json::Value>>>,
    token: Option<String>,
) -> axum::Router {
    let service = StreamableHttpService::new(
        move || {
            let mut server = SupragnosisServer::new(engine.clone()).with_clients(client_log());
            if let Some(ctx) = &sync_ctx {
                server = server.with_sync(ctx.clone());
            }
            if let Some(listener) = &listener {
                server = server.with_listener(listener.clone());
            }
            Ok(server)
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    // DNS-rebinding defense (MCP spec: validate Origin). The daemon is loopback-bound, so a rebound
    // browser page (attacker.com -> 127.0.0.1) is the only way a foreign origin reaches it; the guard
    // refuses any non-loopback Host/Origin so such a page cannot drive observe/search/review.
    //
    // rmcp grew its own Host allowlist in 1.4.0 (GHSA-89vp-x53w-74fx), which the default config above
    // switches on, so the Host half is now checked twice. The guard stays: it is what covered this
    // daemon for the whole time the dependency did not, and it is the only half that reads Origin,
    // which rmcp leaves unchecked by default (`allowed_origins` defaults to empty = no check).
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        // Inner: rewrites the service's own stale-session answer. Outer: the origin guard, so a foreign
        // origin is refused before it reaches either.
        .layer(axum::middleware::from_fn(expired_session_is_not_found))
        .layer(axum::middleware::from_fn(guard_local_origin));
    // Outermost, so an unauthenticated request is refused before any other layer reads it - including
    // the session bookkeeping, which would otherwise let an unauthenticated caller allocate state.
    match token {
        Some(token) => {
            let token = Arc::new(token);
            router.layer(axum::middleware::from_fn(move |req, next| {
                require_token(token.clone(), req, next)
            }))
        }
        None => router,
    }
}

/// DNS-rebinding guard for the loopback MCP daemon (MCP spec: validate Origin/Host). A non-browser
/// MCP client omits Origin and presents a loopback Host, so it passes; a browser page rebound from a
/// foreign name carries that name in Host/Origin and is refused before it reaches the service.
async fn guard_local_origin(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    use axum::http::header::{HOST, ORIGIN};
    let headers = req.headers();
    let host_ok = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .map(host_hdr_is_loopback)
        .unwrap_or(true); // absent Host: a non-browser client on the loopback-bound socket
    let origin_ok = headers
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(origin_is_loopback)
        .unwrap_or(true); // absent Origin: a non-browser MCP client (no CSRF risk)
    if host_ok && origin_ok {
        Ok(next.run(req).await)
    } else {
        Err(axum::http::StatusCode::FORBIDDEN)
    }
}

/// True when a response is rmcp's "this session id is unknown to me" answer.
///
/// Kept separate from the middleware so the classification is testable on its own: deciding to turn a
/// 401 into a 404 on the strength of a body string is the whole risk here, and it must not be reachable
/// for any other 401. rmcp emits two: "Session not found" (the session expired - the client should start
/// a new one) and "Session ID is required" (the request carried no session id at all, which is a
/// malformed request and stays 401).
fn is_expired_session(status: axum::http::StatusCode, body: &[u8]) -> bool {
    status == axum::http::StatusCode::UNAUTHORIZED
        && String::from_utf8_lossy(body).contains("Session not found")
}

/// Answers an expired MCP session with 404 instead of rmcp's 401, so clients can recover on their own.
///
/// MCP Streamable HTTP says a 404 to a request carrying `Mcp-Session-Id` means "start a new session",
/// and clients re-initialize on it. rmcp 0.16 answers an unknown session with `401 Unauthorized:
/// Session not found` (`streamable_http_server/tower.rs`), which is not that signal: a client reads 401
/// as an auth failure - Claude Code takes it for an OAuth challenge, requests OAuth discovery, and
/// reports the resulting parse error instead of reconnecting. The practical cost is that every daemon
/// restart strands connected clients until each is reconnected by hand, since nothing in the exchange
/// tells them the session merely aged out.
async fn expired_session_is_not_found(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let res = next.run(req).await;
    // Every other response - including the SSE streams - passes through untouched and unbuffered.
    if res.status() != axum::http::StatusCode::UNAUTHORIZED {
        return res;
    }
    let (parts, body) = res.into_parts();
    // These are short constant strings; the cap is what keeps this from buffering anything unbounded.
    let Ok(bytes) = axum::body::to_bytes(body, 4096).await else {
        // The body is consumed and cannot be replayed. Keep the original status rather than guess:
        // answering 404 for a response we never managed to identify would invent a session expiry.
        return (parts.status, "Unauthorized").into_response();
    };
    if is_expired_session(parts.status, &bytes) {
        return (
            axum::http::StatusCode::NOT_FOUND,
            "Not Found: the MCP session has expired - start a new session by sending initialize \
             without a session id (MCP Streamable HTTP)",
        )
            .into_response();
    }
    // Not a stale session: replay the original response byte for byte (the reused parts keep
    // content-length truthful, since the body is unchanged).
    axum::response::Response::from_parts(parts, axum::body::Body::from(bytes))
}

/// Parses the MCP streamable-http bind address and **verifies it is loopback** (Principle 17).
///
/// Accepts only a `host:port` IP literal (e.g. `127.0.0.1:7373`). Non-loopback addresses are
/// rejected - this port is the local trust surface, and MCP never leaves loopback (the hub's agent
/// surface was removed in 0.11.0, remote-server.md Section 13). A hostname
/// (localhost) is not accepted because it would require DNS resolution (removing ambiguity). This
/// TCP port is the last one standing: the viewer serves over a unix socket, and clients reach this
/// one through `supragnosis bridge`, which relays to it.
fn parse_loopback_addr(s: &str) -> Result<std::net::SocketAddr> {
    let addr: std::net::SocketAddr = s.trim().parse().with_context(|| {
        format!("invalid MCP bind address: {s:?} - must be in IP:port form (e.g. 127.0.0.1:7373)")
    })?;
    if !addr.ip().is_loopback() {
        anyhow::bail!(
            "MCP bind address {addr} is not loopback - refusing (Principle 17: knowledge \
             sovereignty). Use 127.0.0.1:<port>; other machines reach this knowledge by syncing \
             through a hub, never through this port"
        );
    }
    Ok(addr)
}

/// True if a `host[:port]` (Host header value) names a loopback address.
fn host_hdr_is_loopback(h: &str) -> bool {
    let hostname = if let Some(rest) = h.strip_prefix('[') {
        rest.split_once(']').map(|(a, _)| a).unwrap_or(rest) // IPv6 literal: [::1] or [::1]:port
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h) // host or host:port
    };
    matches!(hostname, "127.0.0.1" | "localhost" | "::1")
}

/// True if an Origin (`scheme://host[:port]`) names a loopback address.
fn origin_is_loopback(o: &str) -> bool {
    let after = o.split_once("://").map(|(_, r)| r).unwrap_or(o);
    host_hdr_is_loopback(after)
}

// --- MCP daemon single-user confinement (Principle 17) --------------------------------------------

/// The daemon's bearer token file. Mode 0600, in the 0700 `~/.supragnosis` dir - the same access
/// control the viewer socket uses, applied to the one surface that could not use a socket.
fn mcp_token_path() -> std::path::PathBuf {
    fed::fed_base_dir().join("mcp.token")
}

/// Creates `dir` closed to other accounts, and closes it if it was found open. architecture.md puts
/// the token, the node key and the store "in the 0700 ~/.supragnosis dir", and until 2026-10 that
/// was a description of intent: the directory was made with the default mode (0755 here), its store
/// file 0644, so any other local account could copy the whole store and never need the token.
/// Closing the directory closes everything under it at once, whatever mode a file was written with.
fn private_dir(dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir)?.permissions().mode();
        if mode & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("chmod 0700 {}", dir.display()))?;
        }
    }
    Ok(())
}

/// Writes a secret so that it is never readable by anyone else, not even for a moment: a new file
/// created 0600, filled, then renamed over the old one. Writing in place and chmodding afterwards -
/// what this replaced - leaves the secret readable at the default mode between the two calls.
fn write_secret(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().context("a secret needs a parent directory")?;
    private_dir(dir)?;
    let tmp = path.with_file_name(format!(
        ".{}.new",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("secret")
    ));
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path).with_context(|| format!("installing {}", path.display()))
}

/// Loads (or generates exactly once) the daemon's bearer token.
///
/// **Why a token and not a unix socket.** The viewer repaid this by moving off TCP entirely, and its
/// socket's 0600 mode became the whole access control. The MCP daemon cannot follow: MCP clients
/// reach it as `http://127.0.0.1:7373/mcp`, and an HTTP-over-UDS transport is not something
/// `claude mcp add --transport http` can speak. So the confinement moves from the socket to a
/// secret, and the secret gets the property the socket had - a file only this user can read.
///
/// 32 bytes of entropy, hex. Same shape and same generator as `node.key`; unlike the node key it is
/// not an identity and may be deleted and regenerated freely (the cost is re-adding the client).
fn load_or_create_mcp_token() -> Result<String> {
    let path = mcp_token_path();
    if path.exists() {
        let tok = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .trim()
            .to_string();
        if !tok.is_empty() {
            return Ok(tok);
        }
        // An empty file is a half-written one (an interrupted first start, an editor). Regenerating
        // is safe and is the only outcome that leaves a working daemon; treating it as a valid
        // empty token would authenticate everyone.
        tracing::warn!(path = %path.display(), "the MCP token file is empty - generating a new token");
    }
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
    let tok = raw.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    });
    write_secret(&path, tok.as_bytes())?;
    tracing::info!(path = %path.display(), "generated the MCP daemon token (0600)");
    Ok(tok)
}

/// Constant-time equality over two blake3 digests.
///
/// The digests are what is compared, not the tokens - the same choice the sync API's bearer check
/// makes, and it is the load-bearing one: a prefix leak on a hash tells an attacker nothing about
/// the token that produced it. Constant time on top of that costs one fold and removes the need to
/// make that argument at all.
fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extracts a bearer token from the Authorization header.
fn bearer_of(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Decides one request against the expected token. Split out from the middleware so the decision is
/// testable without standing up a server - the middleware is then only plumbing.
fn token_admits(headers: &axum::http::HeaderMap, expected: &str) -> bool {
    let Some(presented) = bearer_of(headers) else {
        return false;
    };
    digests_equal(
        blake3::hash(presented.as_bytes()).as_bytes(),
        blake3::hash(expected.as_bytes()).as_bytes(),
    )
}

/// Confines the MCP daemon to the single OS user that owns its token file (Principle 17).
///
/// **The gap this closes.** `parse_loopback_addr` refuses a non-loopback bind, and that guard is
/// real - but loopback is host-local, not user-local. On a multi-user host every local account could
/// reach the full tool surface: `observe`, `review`, `sync_push`, the workspace enumeration, a
/// `search_knowledge` with no workspace scope. P17's "stdio, single user" held for the stdio
/// transport and never for this one, and architecture.md Section 14 carried it as an overdue M4
/// entry condition ("Repay the way the viewer was repaid: a unix-socket transport, or an auth
/// layer").
///
/// It sits OUTSIDE `guard_local_origin` in the layer stack, so an unauthenticated request is refused
/// before anything else looks at it.
///
/// 401 with `WWW-Authenticate: Bearer` is the honest status: this is an authentication failure, and
/// a client that gets one has enough to know what to present. It is deliberately not the 404 that
/// `expired_session_is_not_found` produces - that rewrite is for a *stale session*, a client that
/// authenticated fine and should reconnect. Conflating the two would tell a client to retry a
/// handshake that will fail identically.
async fn require_token(
    expected: Arc<String>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if token_admits(req.headers(), &expected) {
        return next.run(req).await;
    }
    (
        axum::http::StatusCode::UNAUTHORIZED,
        [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
        "Unauthorized: this MCP daemon requires the local bearer token. Loopback confines the \
         surface to this HOST, not to one user, so the token is what makes it yours. Read it from \
         ~/.supragnosis/mcp.token (mode 0600) and present it as `Authorization: Bearer <token>` - \
         `supragnosis status` prints the client command.",
    )
        .into_response()
}

#[cfg(test)]
mod mcp_token_tests {
    use super::{digests_equal, token_admits};
    use axum::http::HeaderMap;

    fn headers(auth: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = auth {
            h.insert(axum::http::header::AUTHORIZATION, v.parse().unwrap());
        }
        h
    }

    /// The whole point of the layer: the right token gets in and nothing else does.
    ///
    /// The near-miss cases are the ones worth listing. A token that is a prefix of the real one, or
    /// the real one with something appended, is what a comparison written against the wrong length
    /// lets through - and `blake3` is what makes both of them ordinary mismatches rather than
    /// special cases.
    #[test]
    fn only_the_exact_token_is_admitted() {
        let real = "8f14e45fceea167a5a36dedd4bea2543";
        assert!(token_admits(&headers(Some(&format!("Bearer {real}"))), real));

        let refused = [
            None,                                             // no header at all
            Some("Bearer "),                                  // header present, token empty
            Some(""),                                         // empty header
            Some(real),                                       // token without the Bearer scheme
            Some("Basic 8f14e45fceea167a5a36dedd4bea2543"),   // wrong scheme
            Some("Bearer 8f14e45fceea167a5a36dedd4bea254"),   // one char short (prefix)
            Some("Bearer 8f14e45fceea167a5a36dedd4bea25433"), // one char long (extension)
            Some("Bearer 8F14E45FCEEA167A5A36DEDD4BEA2543"),  // case differs - a token is bytes
            Some("Bearer "),                                  // whitespace only after the scheme
        ];
        for r in refused {
            assert!(!token_admits(&headers(r), real), "must refuse Authorization: {r:?}");
        }
    }

    /// A constant-time comparison is only useful if it is also a CORRECT one - the failure mode of
    /// hand-rolling one is a fold that returns true for everything.
    #[test]
    fn digest_equality_separates_digests_that_differ_anywhere() {
        let a = *blake3::hash(b"one").as_bytes();
        assert!(digests_equal(&a, &a));
        assert!(!digests_equal(&a, blake3::hash(b"two").as_bytes()));
        // Differing in the last byte only: a comparison that stops early on the first match, or one
        // that folds with the wrong operator, passes every other case in this file and fails here.
        let mut tail = a;
        tail[31] ^= 1;
        assert!(!digests_equal(&a, &tail));
        let mut head = a;
        head[0] ^= 1;
        assert!(!digests_equal(&a, &head));
    }
}

#[cfg(test)]
mod daemon_guard_tests {
    use super::{host_hdr_is_loopback, origin_is_loopback, parse_loopback_addr};

    #[test]
    fn parse_loopback_addr_accepts_loopback_rejects_public() {
        assert!(parse_loopback_addr("127.0.0.1:7373").is_ok());
        assert!(parse_loopback_addr("127.0.0.1:0").is_ok());
        assert!(parse_loopback_addr("[::1]:7373").is_ok());
        // Non-loopback binds are rejected (Principle 17).
        assert!(parse_loopback_addr("0.0.0.0:7373").is_err());
        assert!(parse_loopback_addr("192.168.1.10:7373").is_err());
        // Format error.
        assert!(parse_loopback_addr("localhost:7373").is_err());
        assert!(parse_loopback_addr("nonsense").is_err());
    }

    #[test]
    fn loopback_hosts_and_origins_pass_foreign_ones_refused() {
        // IPv6 always arrives bracketed in a Host header ([::1] / [::1]:port), never bare.
        for ok in ["127.0.0.1", "127.0.0.1:7373", "localhost:7373", "[::1]:7373", "[::1]"] {
            assert!(host_hdr_is_loopback(ok), "{ok} should be loopback");
        }
        for bad in [
            "evil.example.com",
            "evil.example.com:7373",
            "10.0.0.5:7373",
            "attacker.127.0.0.1.nip.io",
        ] {
            assert!(!host_hdr_is_loopback(bad), "{bad} must be refused");
        }
        assert!(origin_is_loopback("http://127.0.0.1:7373"));
        assert!(origin_is_loopback("http://localhost"));
        assert!(!origin_is_loopback("https://evil.example.com"));
    }

    /// An expired session must be 404 (the MCP signal to re-initialize), and nothing else may be.
    ///
    /// The translation keys off a body string, so the risk is over-reach: a 401 that is not an aged-out
    /// session must keep its status, or a genuine auth failure would be reported to the client as a
    /// recoverable session expiry and retried forever.
    #[test]
    fn only_an_unknown_session_is_translated_to_not_found() {
        use super::is_expired_session;
        use axum::http::StatusCode;

        let unauthorized = StatusCode::UNAUTHORIZED;

        // rmcp's exact wording for an unknown/aged-out session - the one case that must flip.
        assert!(
            is_expired_session(unauthorized, b"Unauthorized: Session not found"),
            "an unknown session must become 404 so the client starts a new session"
        );

        // rmcp's other 401: the request carried no session id. That is malformed, not expired -
        // re-initializing would not fix it, so it must stay 401.
        assert!(
            !is_expired_session(unauthorized, b"Unauthorized: Session ID is required"),
            "a missing session id is a malformed request, not an expired session"
        );

        // Any other 401 is a real authorization failure and must not be masked as a session expiry.
        for body in [&b"missing bearer token"[..], &b"unknown bearer token"[..], &b""[..]] {
            assert!(
                !is_expired_session(unauthorized, body),
                "an unrelated 401 must keep its status: {}",
                String::from_utf8_lossy(body)
            );
        }

        // The status is part of the predicate: the same wording under a success or a different
        // failure must not be rewritten.
        for status in [StatusCode::OK, StatusCode::FORBIDDEN, StatusCode::NOT_ACCEPTABLE] {
            assert!(
                !is_expired_session(status, b"Unauthorized: Session not found"),
                "{status} must not be translated - only a 401 carries this meaning"
            );
        }

        // A non-UTF-8 body must be judged, not panic (from_utf8_lossy, not unwrap).
        let invalid_utf8 = [0xff, 0xfe, 0x00];
        assert!(!is_expired_session(unauthorized, &invalid_utf8));
    }
}

// --- Background daemon lifecycle (start/stop/restart/status) -------------------------
// Self-managed via a pidfile + logs. Uses only kill (-0/SIGTERM)/TcpStream, so no unsafe/libc is needed.

fn base_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".supragnosis")
}
fn pid_path() -> std::path::PathBuf {
    base_dir().join("supragnosis.pid")
}
fn log_dir() -> std::path::PathBuf {
    base_dir().join("log")
}
fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_path()).ok().and_then(|s| s.trim().parse().ok())
}
/// Checks whether the process is alive via `kill -0` (portable, without unsafe/libc).
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

// --- Observing and acting on the daemon's managers (docs/daemon-lifecycle.md) -----------------
// The decisions are pure and live in `lifecycle.rs`; what is here only looks (launchctl, the pidfile,
// the sockets) and acts (kickstart, bootout, a signal).

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

/// Every loaded launchd job under a label the product has ever installed (L2). A job that is
/// loaded but has no process is included - a KeepAlive job failing on start is still a manager.
#[cfg(target_os = "macos")]
fn launchd_jobs() -> Vec<lifecycle::Job> {
    lifecycle::KNOWN_LABELS
        .iter()
        .filter_map(|&(label, kind)| {
            let out =
                std::process::Command::new("launchctl").arg("list").arg(label).output().ok()?;
            if !out.status.success() {
                return None;
            }
            let (pid, last_exit) =
                lifecycle::parse_launchctl_list(&String::from_utf8_lossy(&out.stdout));
            Some(lifecycle::Job { label, kind, pid, last_exit })
        })
        .collect()
}
#[cfg(not(target_os = "macos"))]
fn launchd_jobs() -> Vec<lifecycle::Job> {
    Vec::new()
}

/// Every systemd user unit under a name the product runs the daemon as (L2) that manages it now:
/// enabled, or active (daemon-lifecycle.md Section 4, "On Linux"). Empty where there is no user
/// manager to ask, and everywhere but Linux.
fn systemd_units() -> Vec<lifecycle::Job> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    lifecycle::KNOWN_UNITS
        .iter()
        .filter_map(|&(label, kind)| {
            let st = unit_show(label)?;
            st.manages().then_some(lifecycle::Job {
                label,
                kind,
                pid: st.pid,
                last_exit: st.last_exit,
            })
        })
        .collect()
}

/// What the person's systemd manager says about one unit; `None` when it cannot be asked.
fn unit_show(unit: &str) -> Option<lifecycle::UnitShow> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", unit])
        .args(["-p", "LoadState", "-p", "ActiveState", "-p", "UnitFileState", "-p", "MainPID"])
        .args(["-p", "ExecMainStatus", "-p", "ExecMainExitTimestampMonotonic"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| lifecycle::parse_systemctl_show(&String::from_utf8_lossy(&out.stdout)))
}

/// Whether this session can reach a systemd user manager, and why not when it cannot - no systemctl,
/// or no user bus, as in many containers.
fn systemd_user_available() -> std::result::Result<(), String> {
    match std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
    {
        Err(e) => Err(format!("systemctl was not found ({e})")),
        Ok(o) if !o.status.success() => {
            let why = String::from_utf8_lossy(&o.stderr).trim().to_string();
            Err(if why.is_empty() { "systemctl --user did not answer".to_string() } else { why })
        }
        Ok(_) => Ok(()),
    }
}

/// The user name, for lingering and a unit's `%u`.
fn current_user() -> Option<String> {
    let out = std::process::Command::new("id").arg("-un").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// Whether the person's systemd manager outlives their sessions: logind keeps one file per lingering
/// user. `None` when the user name is not known.
fn linger_state() -> Option<bool> {
    let user = current_user()?;
    Some(std::path::Path::new("/var/lib/systemd/linger").join(user).exists())
}

/// Runs `systemctl --user` with these arguments; a failure carries systemctl's own words (L4).
fn systemctl_user(args: &[&str]) -> Result<()> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("failed to run systemctl")?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// What the lifecycle commands decide from. Reads only - a stale pidfile is cleared by the commands
/// that act, never by `status`.
fn observe() -> lifecycle::Observed {
    observe_with(daemon_store_path(|k| std::env::var(k).ok()).as_deref())
}

/// What the lifecycle commands observe, with the store probed at `store` - the path the daemon in
/// question would open, which for `service install` comes from the job's environment rather than
/// this process's.
fn observe_with(store: Option<&std::path::Path>) -> lifecycle::Observed {
    lifecycle::Observed {
        pidfile: live_pidfile().and_then(|p| u32::try_from(p).ok()),
        jobs: launchd_jobs(),
        units: systemd_units(),
        answering: port_open(&status_http_addr()),
        store_held: store.is_some_and(supragnosis_store::redb_in_use),
    }
}

/// The redb file a daemon running with this environment opens, resolved the way `serve` resolves
/// it. `None` for the in-memory store, which nothing else can hold.
fn daemon_store_path(env: impl Fn(&str) -> Option<String>) -> Option<std::path::PathBuf> {
    let get = |k: &str| env(k).filter(|v| !v.trim().is_empty());
    let kind = get("SUPRAGNOSIS_STORE").unwrap_or_else(|| "redb".to_string());
    if matches!(kind.as_str(), "mem" | "memory") {
        return None;
    }
    let dir = get("SUPRAGNOSIS_DATA_DIR").unwrap_or_else(|| default_data_dir_for(&kind));
    Some(redb_path(&dir))
}

/// The pidfile's process, only when it is alive AND is supragnosis (L9). A pid outlives the process
/// it named, and acting on a reused one would signal an unrelated program.
fn pidfile_now() -> lifecycle::Pidfile {
    let text = std::fs::read_to_string(pid_path()).ok();
    lifecycle::pidfile_state(text.as_deref(), pid_alive, pid_is_supragnosis)
}

fn live_pidfile() -> Option<i32> {
    match pidfile_now() {
        lifecycle::Pidfile::Live(pid) => Some(pid),
        _ => None,
    }
}

fn pid_is_supragnosis(pid: i32) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|o| {
            o.status.success()
                && lifecycle::is_supragnosis_command(&String::from_utf8_lossy(&o.stdout))
        })
        .unwrap_or(false)
}

fn clear_stale_pidfile() {
    if let lifecycle::Pidfile::Stale(_) = pidfile_now() {
        let _ = std::fs::remove_file(pid_path());
    }
}

/// The running daemon's version, from the viewer's `/api/about` over its unix socket. `None` when
/// the socket does not answer - and then the version is unknown, never assumed (Section 5).
fn running_version() -> Option<String> {
    lifecycle::parse_about_version(&viz_get("/api/about")?)
}

/// First contact as the running daemon reports it, the `clients` list of `/api/clients`. `None`
/// when the socket does not answer or the daemon does not record contacts (one older than this).
fn first_contacts() -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&viz_get("/api/clients")?)
        .ok()?
        .get("clients")
        .filter(|c| c.is_array())
        .cloned()
}

/// The running daemon's store health - the owed-projection ledger and the last recovery
/// (crash-recovery.md K5). `None` when the viewer socket does not answer, or answers without the
/// route (a daemon older than this binary): both are unknown, not healthy (Principle 5).
fn store_health() -> Option<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&viz_get("/api/health")?)
        .ok()
        .filter(|v| v.get("owed_projections").is_some())
}

/// One GET over the viewer's unix socket, returning the body. Short timeouts: `status` must answer
/// even when the daemon is wedged.
fn viz_get(path: &str) -> Option<String> {
    viz_get_within(path, 800)
}

/// [`viz_get`] with its own timeout, for a read that walks the log.
fn viz_get_within(path: &str, millis: u64) -> Option<String> {
    use std::io::{Read, Write};
    let sock = std::env::var("SUPRAGNOSIS_VIZ_SOCK")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_viz_sock);
    let mut s = std::os::unix::net::UnixStream::connect(sock).ok()?;
    let limit = Some(std::time::Duration::from_millis(millis));
    s.set_read_timeout(limit).ok()?;
    s.set_write_timeout(limit).ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nConnection: close\r\n\r\n").as_bytes())
        .ok()?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).ok()?;
    let (_, body) = raw.split_once("\r\n\r\n")?;
    Some(body.to_string())
}

/// Restart a launchd job in place (`kickstart -k`), whichever known label it runs under. The plist's
/// environment is re-applied, so the viewer comes back with the server.
#[cfg(target_os = "macos")]
fn launchd_kickstart(job: &lifecycle::Job) -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{}", job.label);
    let st = std::process::Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl kickstart")?;
    if !st.success() {
        anyhow::bail!("launchctl kickstart {target} failed");
    }
    println!("restarted launchd job {} (MCP server + viewer).", job.label);
    Ok(())
}

/// Stop a launchd job (`bootout`), so KeepAlive does not respawn it. A Homebrew job's plist stays
/// where Homebrew put it and loads again at the next login - said here, because "stopped" alone
/// would be true only until then.
#[cfg(target_os = "macos")]
fn launchd_bootout(job: &lifecycle::Job) -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let target = format!("gui/{uid}/{}", job.label);
    let st = std::process::Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .status()
        .with_context(|| "failed to run launchctl bootout")?;
    if !st.success() {
        anyhow::bail!("launchctl bootout {target} failed (already stopped?).");
    }
    match job.kind {
        lifecycle::LabelKind::Homebrew(token) => println!(
            "stopped launchd job {}. It is Homebrew's and loads again at the next login - retire it with `brew services stop {token}`.",
            job.label
        ),
        _ => println!(
            "stopped launchd job {}. It stays down until `supragnosis restart` (or the next login) loads it again.",
            job.label
        ),
    }
    Ok(())
}

/// Load the installed canonical job (`bootstrap`) - `restart` after a `stop`.
#[cfg(target_os = "macos")]
fn launchd_bootstrap_canonical() -> Result<()> {
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let st = std::process::Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{uid}"))
        .arg(&plist)
        .status()
        .context("failed to run launchctl bootstrap")?;
    if !st.success() {
        anyhow::bail!("launchctl bootstrap gui/{uid} {} failed", plist.display());
    }
    println!("loaded launchd job {} (MCP server + viewer).", lifecycle::CANONICAL_LABEL);
    await_daemon(lifecycle::CANONICAL_LABEL)
}
#[cfg(not(target_os = "macos"))]
fn launchd_bootstrap_canonical() -> Result<()> {
    anyhow::bail!("a LaunchAgent plist was found on a system without launchd")
}

#[cfg(not(target_os = "macos"))]
fn launchd_kickstart(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}
#[cfg(not(target_os = "macos"))]
fn launchd_bootout(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}

/// Restart a systemd user unit in place. Its program is the path an upgrade repoints, so the new
/// binary is what starts (daemon-lifecycle.md Section 4, "On Linux").
fn systemd_restart(job: &lifecycle::Job) -> Result<()> {
    systemctl_user(&["restart", job.label])?;
    println!("restarted systemd unit {} (MCP server + viewer).", job.label);
    await_daemon(job.label)
}

/// Stop a systemd user unit. It stays enabled, so it starts again at the next login - said here,
/// because "stopped" alone would be true only until then.
fn systemd_stop(job: &lifecycle::Job) -> Result<()> {
    systemctl_user(&["stop", job.label])?;
    match job.kind {
        lifecycle::LabelKind::Homebrew(token) => println!(
            "stopped systemd unit {}. It is Homebrew's and starts again at the next login - retire it with `brew services stop {token}`.",
            job.label
        ),
        _ => println!(
            "stopped systemd unit {}. While it is enabled it starts again at the next login - `supragnosis restart` starts it now, `supragnosis service uninstall` keeps it off.",
            job.label
        ),
    }
    Ok(())
}

/// Start the installed canonical unit - `restart` when it is installed but neither enabled nor active.
fn systemd_start_canonical() -> Result<()> {
    systemctl_user(&["start", lifecycle::CANONICAL_UNIT])?;
    println!("started systemd unit {} (MCP server + viewer).", lifecycle::CANONICAL_UNIT);
    await_daemon(lifecycle::CANONICAL_UNIT)
}

/// Prints how to connect an AI app, on `start` and `status` - where a person looking for it looks.
///
/// `supragnosis connect` comes first: it registers the bridge, which reads the token from its file,
/// so no client holds a copy (docs/client-connect.md). The HTTP form follows for a client that takes
/// only a URL, with the token read by the shell rather than printed - agents run `status`, and a
/// printed token lands in their transcripts (daemon-lifecycle.md Section 11).
fn print_client_command(http: &str) {
    let path = mcp_token_path();
    println!("  token   {} (0600 - read from the file, never printed)", path.display());
    println!("  connect supragnosis connect claude-code   (`supragnosis connect` lists every app)");
    println!(
        "  http    http://{http}/mcp with --header \"Authorization: Bearer $(cat {})\"",
        path.display()
    );
}

/// Resolved MCP http address for status/lifecycle checks (env var or default).
fn status_http_addr() -> String {
    std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string())
}

fn start(cfg: Config) -> Result<()> {
    let http = cfg.http.clone().unwrap_or_else(|| "127.0.0.1:7373".to_string());
    // A pidfile left by a daemon that is gone would read, after the fork, as this one gone.
    clear_stale_pidfile();
    if let Some(pid) = live_pidfile() {
        anyhow::bail!("already running (pid {pid}). Run 'supragnosis stop' and try again.");
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
        .map(|v| format!("unix:{v}"))
        .unwrap_or_else(|| "(off)".to_string());
    // Generated here rather than in the child, so the connect line can be printed to the terminal
    // the operator is actually looking at. It is the same file the daemon then loads.
    if cfg.mcp_auth {
        load_or_create_mcp_token()?;
    }
    // fork/setsid/pidfile/stdio redirect. The daemonized child serves; this process waits for it
    // to answer, so `start` says started only once it is, and fails with the reason when the
    // daemon exits first (daemon-lifecycle.md L4).
    let parent = match daemonize::Daemonize::new()
        .pid_file(pid_path())
        .stdout(out)
        .stderr(err)
        .execute()
    {
        daemonize::Outcome::Child(Ok(_)) => return run_blocking(cfg),
        daemonize::Outcome::Child(Err(e)) | daemonize::Outcome::Parent(Err(e)) => {
            anyhow::bail!("daemonization failed: {e}")
        }
        daemonize::Outcome::Parent(Ok(parent)) => parent,
    };
    if parent.first_child_exit_code != 0 {
        anyhow::bail!(
            "daemonization failed: its first child exited with status {}",
            parent.first_child_exit_code
        );
    }
    match await_forked(&http) {
        lifecycle::Forked::Serving => {
            println!("supragnosis daemon started - MCP http://{http}/mcp  viewer {viz_msg}")
        }
        lifecycle::Forked::Slow => println!(
            "supragnosis daemon still starting - `supragnosis status` shows it once http://{http}/mcp answers"
        ),
        lifecycle::Forked::Gone(pid) => {
            let log = log_dir().join("supragnosis.err.log");
            let reason = std::fs::read_to_string(&log)
                .ok()
                .and_then(|t| lifecycle::logged_error(&t))
                .unwrap_or_else(|| "it logged no error".to_string());
            anyhow::bail!(
                "the daemon (pid {pid}) exited before it answered on {http}: {reason}\n  the log is {}",
                log.display()
            );
        }
    }
    println!("  pidfile {}  logs {}", pid_path().display(), log_dir().display());
    if cfg.mcp_auth {
        print_client_command(&http);
    }
    Ok(())
}

/// Watches the daemon `start` just forked until its address answers, its process is gone, or thirty
/// seconds pass (`lifecycle::forked`).
fn await_forked(http: &str) -> lifecycle::Forked {
    let mut looks = 0;
    loop {
        std::thread::sleep(lifecycle::START_TICK);
        looks += 1;
        if let Some(seen) = lifecycle::forked(port_open(http), &pidfile_now(), looks) {
            return seen;
        }
    }
}

/// Sends SIGTERM to the self-managed (pidfile) daemon and waits for graceful exit.
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

fn stop() -> Result<()> {
    use lifecycle::Manager;
    clear_stale_pidfile();
    let situation = lifecycle::classify(&observe());
    match lifecycle::stop_or_refuse(&situation, &status_http_addr()).map_err(anyhow::Error::msg)? {
        None => {
            println!("not running.");
            Ok(())
        }
        Some(Manager::Pidfile { pid }) => stop_pidfile(pid as i32),
        Some(Manager::Launchd(job)) => launchd_bootout(&job),
        Some(Manager::Systemd(job)) => systemd_stop(&job),
    }
}

fn restart(cfg: Config) -> Result<()> {
    use lifecycle::{Manager, Restart};
    clear_stale_pidfile();
    let situation = lifecycle::classify(&observe());
    let installed = || canonical_service_state() != "absent";
    match lifecycle::restart_or_refuse(&situation, installed, &status_http_addr())
        .map_err(anyhow::Error::msg)?
    {
        Restart::LoadCanonical => {
            if cfg!(target_os = "linux") {
                systemd_start_canonical()
            } else {
                launchd_bootstrap_canonical()
            }
        }
        Restart::StartFresh => start(cfg),
        Restart::Bounce(Manager::Pidfile { pid }) => {
            stop_pidfile(pid as i32)?;
            std::thread::sleep(std::time::Duration::from_millis(400)); // wait for the port to release
            start(cfg)
        }
        Restart::Bounce(Manager::Launchd(job)) => {
            launchd_kickstart(&job)?;
            await_daemon(job.label)
        }
        Restart::Bounce(Manager::Systemd(job)) => systemd_restart(&job),
    }
}

/// L4: after loading or restarting a launchd job or a systemd unit, it counts as up only when the
/// daemon answers, and then says which version it serves - a restart's whole point after an upgrade
/// is the version, and "restarted" alone does not show it. It counts as failed when its manager shows
/// it failing - no process and a non-zero last exit - for three seconds running: a job crash-looping
/// on the store lock sits like that between respawns, as does a unit waiting out `RestartSec`, while
/// one being restarted shows its previous exit only for a moment. A slow start that is still running
/// is reported as slow, not failed: a daemon repaying owed projections before it binds
/// (crash-recovery.md K3) can take a while.
fn await_daemon(label: &str) -> Result<()> {
    let here = env!("CARGO_PKG_VERSION");
    let mut watch = lifecycle::StartWatch::default();
    loop {
        std::thread::sleep(lifecycle::START_TICK);
        let serving = running_version();
        let job = if serving.is_some() {
            None
        } else {
            launchd_jobs().into_iter().chain(systemd_units()).find(|j| j.label == label)
        };
        let look = lifecycle::StartLook {
            serving,
            pid: job.as_ref().and_then(|j| j.pid),
            last_exit: job.as_ref().and_then(|j| j.last_exit),
        };
        match watch.see(look) {
            None => {}
            Some(lifecycle::Start::Serving(v)) => {
                match lifecycle::drift(Some(&v), here) {
                    lifecycle::Drift::Differs { running, here } => println!(
                        "  now serving {running}, but this binary is {here} - the job runs a different binary than this one"
                    ),
                    _ => println!("  now serving {v}"),
                }
                return Ok(());
            }
            Some(lifecycle::Start::Failed(code)) => {
                let (manager, log) = if cfg!(target_os = "linux") {
                    ("systemd", format!("`journalctl --user -u {label}`"))
                } else {
                    ("launchd", log_dir().join("supragnosis.err.log").display().to_string())
                };
                anyhow::bail!(
                    "{label} is loaded but not running - it exited with status {code} and {manager} is retrying it. The reason is in {log}"
                );
            }
            Some(lifecycle::Start::Slow) => {
                println!("  still starting - `supragnosis status` shows the version once the daemon answers");
                return Ok(());
            }
        }
    }
}

/// Everything `status --json` reports, already gathered. [`status_document`] only shapes it, so a
/// test can build the document without a daemon (docs/compatibility.md Section 6).
struct StatusFacts<'a> {
    situation: &'a lifecycle::Situation,
    answering: bool,
    store_held: bool,
    http: String,
    here: &'a str,
    running: Option<String>,
    health: Option<serde_json::Value>,
    server: serde_json::Value,
    /// First contact (docs/onboarding.md Section 5): the AI apps that reached the daemon since it
    /// started; null when it does not answer, or does not record them.
    clients: Option<serde_json::Value>,
    /// The model the store recorded and the one the daemon would open it with, when the daemon does
    /// not answer because they differ (semantic-retrieval.md Section 13, SR8).
    embedder_conflict: Option<(String, String)>,
    /// The canonical LaunchAgent's plist, or the canonical user unit on Linux, and its state.
    plist: std::path::PathBuf,
    plist_state: &'static str,
    /// Whether the person's systemd manager outlives their sessions; null off Linux, or unknown.
    linger: Option<bool>,
}

/// One line on semantic retrieval, from the daemon's health report (semantic-retrieval.md SR3):
/// on with the model, or off with the reason and what to run. No report at all is a node with no
/// embedder - a build without the model, or `SUPRAGNOSIS_EMBED=none`.
fn semantic_line(report: Option<&serde_json::Value>) -> String {
    match report {
        Some(r) if r["state"].as_str() == Some("on") => {
            let model = r["model"].as_str().unwrap_or("?");
            let b = &r["backfill"];
            match b["state"].as_str() {
                Some("running") => format!(
                    "semantic on ({model}) - embedding what was written before the model: {} of {}",
                    b["embedded"].as_u64().unwrap_or(0),
                    b["found"].as_u64().unwrap_or(0)
                ),
                Some("stopped") => format!(
                    "semantic on ({model}) - embedding what was written before the model stopped at {} of {}, the rest is found by keyword until the next start: {}",
                    b["embedded"].as_u64().unwrap_or(0),
                    b["found"].as_u64().unwrap_or(0),
                    b["note"].as_str().unwrap_or("no reason given")
                ),
                _ => format!("semantic on ({model})"),
            }
        }
        Some(r) if r["state"].as_str() == Some("off") => format!(
            "semantic off - {}",
            r["note"].as_str().unwrap_or("the daemon gave no reason")
        ),
        _ => "semantic off - keyword only (no embedding model in this build, or SUPRAGNOSIS_EMBED=none)"
            .to_string(),
    }
}

/// `status --json`, which the desktop app reads for its tray line and its Settings page.
fn status_document(f: &StatusFacts) -> serde_json::Value {
    use lifecycle::{Manager, Situation};
    let managers: Vec<&Manager> = match f.situation {
        Situation::One(m) => vec![m],
        Situation::Conflict(ms) => ms.iter().collect(),
        Situation::Stopped | Situation::Unrecognized => vec![],
    };
    let manager_json = |m: &Manager| match m {
        Manager::Pidfile { pid } => serde_json::json!({ "type": "pidfile", "pid": pid }),
        Manager::Launchd(j) => {
            let (source, formula) = match j.kind {
                lifecycle::LabelKind::Canonical => ("canonical", None),
                lifecycle::LabelKind::Homebrew(t) => ("homebrew", Some(t)),
                lifecycle::LabelKind::Retired => ("retired", None),
            };
            serde_json::json!({
                "type": "launchd", "label": j.label, "source": source, "formula": formula,
                "pid": j.pid, "last_exit": j.last_exit,
            })
        }
        Manager::Systemd(j) => {
            let (source, formula) = match j.kind {
                lifecycle::LabelKind::Canonical => ("canonical", None),
                lifecycle::LabelKind::Homebrew(t) => ("homebrew", Some(t)),
                lifecycle::LabelKind::Retired => ("retired", None),
            };
            serde_json::json!({
                "type": "systemd", "unit": j.label, "label": j.label, "source": source,
                "formula": formula, "pid": j.pid, "last_exit": j.last_exit,
            })
        }
    };
    serde_json::json!({
        "schema": JSON_SCHEMA,
        "situation": match f.situation {
            Situation::Stopped => "stopped",
            Situation::One(_) => "one",
            Situation::Conflict(_) => "conflict",
            Situation::Unrecognized => "unrecognized",
        },
        "managers": managers.iter().map(|m| manager_json(m)).collect::<Vec<_>>(),
        "answering": f.answering,
        "store_held": f.store_held,
        "mcp": format!("http://{}/mcp", f.http),
        "version": { "here": f.here, "running": f.running },
        // The health report minus the semantic part, which has its own field below.
        "store": f.health.as_ref().map(|h| {
            let mut s = h.clone();
            if let Some(o) = s.as_object_mut() {
                o.remove("semantic");
            }
            s
        }),
        // The daemon's own report on semantic retrieval (SR3); null when it is not answering.
        "semantic": f.health.as_ref().and_then(|h| h.get("semantic")).cloned(),
        // A store the daemon cannot open for its model (SR8); null otherwise.
        "embedder_conflict": f.embedder_conflict.as_ref().map(|(recorded, daemon)| {
            serde_json::json!({ "recorded": recorded, "daemon": daemon })
        }),
        "server": f.server,
        "clients": f.clients,
        // `plist` keeps its name in schema 1 and holds the unit's path on Linux; `path` says the same
        // without the name (daemon-lifecycle.md Section 4, "On Linux").
        "service": {
            "label": canonical_service_label(),
            "plist": f.plist,
            "path": f.plist,
            "state": f.plist_state,
            "linger": f.linger,
        },
    })
}

fn status(json: bool) -> Result<()> {
    use lifecycle::{Drift, Situation};
    let http = status_http_addr();
    let observed = observe();
    let situation = lifecycle::classify(&observed);
    let here = env!("CARGO_PKG_VERSION");
    let running = if observed.answering { running_version() } else { None };
    let drift = lifecycle::drift(running.as_deref(), here);
    let health = if observed.answering { store_health() } else { None };
    // A daemon that does not answer may be refused its store for the model (SR8): what the store
    // recorded, read without holding it, against the model the daemon's environment names.
    let embedder_conflict = if observed.answering { None } else { daemon_embedder_conflict() };
    // Always this daemon since remote servers were removed in 0.11.0; kept so `status --json`'s
    // schema stays 1 (remote-server.md Section 13).
    let server = serde_json::json!({"name": "local", "remote": false});

    if json {
        let facts = StatusFacts {
            situation: &situation,
            answering: observed.answering,
            store_held: observed.store_held,
            http: http.clone(),
            here,
            running: running.clone(),
            health,
            server,
            clients: if observed.answering { first_contacts() } else { None },
            embedder_conflict: embedder_conflict.clone(),
            plist: canonical_service_path(),
            plist_state: canonical_service_state(),
            linger: if cfg!(target_os = "linux") { linger_state() } else { None },
        };
        println!("{}", status_document(&facts));
        return Ok(());
    }

    match &situation {
        Situation::Stopped => {
            match read_pid() {
                Some(pid) => println!("stopped (stale pidfile, pid {pid})"),
                None => println!("stopped"),
            }
            if let Some((recorded, daemon)) = &embedder_conflict {
                println!("{}", embedder_conflict_line(recorded, daemon));
            }
            // Where a person lands on a first run, or after a restart that did not bring it back
            // (docs/onboarding.md Section 7): how to run it now, how to keep it, and setup.
            println!("  start   supragnosis start            runs it until you log out");
            println!("  keep    supragnosis service install  runs it now and at every login");
            println!("  setup   supragnosis setup            the whole first run, step by step");
            return Ok(());
        }
        Situation::One(m) => {
            println!(
                "{} ({})",
                if observed.answering { "running" } else { "not responding" },
                m.describe()
            );
        }
        Situation::Conflict(ms) => {
            println!("CONFLICT: {}", lifecycle::conflict_message(ms));
        }
        Situation::Unrecognized if observed.answering => {
            println!(
                "running (unrecognized manager - no pidfile, no known launchd label or systemd unit)"
            );
        }
        Situation::Unrecognized => {
            println!(
                "store held by a process no manager runs - an MCP client's stdio server, or a `supragnosis serve` in a terminal"
            );
        }
    }
    println!(
        "  MCP     http://{http}/mcp  ({})",
        if observed.answering { "responding" } else { "not responding" }
    );
    match &drift {
        Drift::Same(v) => println!("  version {v}"),
        Drift::Differs { running, here } => println!(
            "  version RUNNING {running}, this binary {here} - the daemon still runs the old image; `supragnosis restart` loads this one"
        ),
        Drift::Unknown if observed.answering => {
            println!("  version unknown (the viewer socket did not answer)")
        }
        Drift::Unknown => {}
    }
    if let Some((recorded, daemon)) = &embedder_conflict {
        println!("{}", embedder_conflict_line(recorded, daemon));
    }
    if let Some(h) = &health {
        let owed = h["owed_projections"].as_u64().unwrap_or(0);
        if owed > 0 {
            println!(
                "  store   {owed} observation(s) not yet projected - a sync, a backfill or a re-key has not re-materialized them; the log has them, and `supragnosis restart` re-projects them"
            );
        }
        if let Some(r) = h["last_recovery"].as_object() {
            let wss: Vec<&str> = r
                .get("workspaces")
                .and_then(|w| w.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            println!(
                "  store   recovered at start: {1} observation(s) were owed a projection (an interrupted write, or this store's first open by a build that keeps the ledger), so {0} was re-projected",
                wss.join(", "),
                r.get("observations").and_then(|v| v.as_u64()).unwrap_or(0)
            );
        }
        println!("  {}", semantic_line(h.get("semantic")));
    }
    if observed.answering {
        // The connect line is reported for a RUNNING daemon whose token file exists. Read, never
        // generated: `status` must not create state, and a missing file here is the honest report
        // that this daemon is running without auth rather than an invitation to mint a token.
        let token = std::fs::read_to_string(mcp_token_path())
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        match token {
            Some(_) => print_client_command(&http),
            None => println!(
                "  auth    none - every local OS account on this host can reach the tool surface"
            ),
        }
    }
    let label = canonical_service_label();
    match canonical_service_state() {
        "generated" => println!("  login   starts at login ({label})"),
        "hand_written" if cfg!(target_os = "linux") => {
            println!("  login   starts at login ({label}, a unit not generated by service install)")
        }
        "hand_written" => println!("  login   starts at login ({label}, hand-written plist)"),
        _ => println!("  login   not installed - supragnosis service install"),
    }
    // A user manager stops with the person's last session unless it lingers (Section 4, "On Linux").
    if cfg!(target_os = "linux")
        && canonical_service_state() != "absent"
        && linger_state() == Some(false)
    {
        println!(
            "  linger  off - it stops when you log out; `loginctl enable-linger` keeps it running"
        );
    }
    if matches!(situation, Situation::One(_)) {
        println!("  control supragnosis restart | supragnosis stop");
    }
    Ok(())
}

// --- The canonical LaunchAgent and user unit (docs/daemon-lifecycle.md Section 4) ------------

/// The canonical manager's name: the LaunchAgent's label, or the user unit's name on Linux.
fn canonical_service_label() -> &'static str {
    if cfg!(target_os = "linux") {
        lifecycle::CANONICAL_UNIT
    } else {
        lifecycle::CANONICAL_LABEL
    }
}

/// Where the canonical plist lives - or on Linux the canonical user unit, under `$XDG_CONFIG_HOME`
/// when it is set, as systemd reads it.
fn canonical_service_path() -> std::path::PathBuf {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()));
    if cfg!(target_os = "linux") {
        let config = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|v| v.starts_with('/'))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        return config.join("systemd/user").join(lifecycle::CANONICAL_UNIT);
    }
    home.join("Library/LaunchAgents")
        .join(format!("{}.plist", lifecycle::CANONICAL_LABEL))
}

/// absent | generated | hand_written - the state the app's Start at Login item reflects.
fn canonical_service_state() -> &'static str {
    match std::fs::read_to_string(canonical_service_path()) {
        Err(_) => "absent",
        Ok(text) if lifecycle::is_generated(&text) => "generated",
        Ok(_) => "hand_written",
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The EnvironmentVariables of an existing plist, read through plutil (launchd's own reader) rather
/// than a hand-rolled XML parse - a hand-written plist can be in any of the forms launchd accepts.
#[cfg(target_os = "macos")]
fn plist_env(path: &std::path::Path) -> Result<std::collections::BTreeMap<String, String>> {
    let out = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()
        .context("failed to run plutil")?;
    if !out.status.success() {
        anyhow::bail!(
            "plutil could not read {} - fix or remove it by hand: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(v.get("EnvironmentVariables")
        .and_then(|e| e.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default())
}

/// Move a plist the product did not write out of LaunchAgents, keeping it (L5).
#[cfg(target_os = "macos")]
fn move_aside(path: &std::path::Path, label: &str) -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    move_file_aside(path, lifecycle::moved_aside_path(&home, label, unix_secs()))
}

/// Move a file the product did not write to `dst`, keeping it (L5).
fn move_file_aside(path: &std::path::Path, dst: std::path::PathBuf) -> Result<std::path::PathBuf> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::rename(path, &dst)
        .with_context(|| format!("failed to move {} aside to {}", path.display(), dst.display()))?;
    Ok(dst)
}

#[cfg(target_os = "macos")]
fn launchctl_quiet(args: &[&str]) -> bool {
    std::process::Command::new("launchctl")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Wait for the MCP port to close and the store to be released after a job is booted out, so the
/// next owner does not race the last one for the store lock. False when either is still held after
/// ten seconds.
fn wait_until_released(store: Option<&std::path::Path>) -> bool {
    let mut first = true;
    lifecycle::released(std::iter::from_fn(|| {
        if !first {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        first = false;
        // The store is probed only once the address went quiet, as before.
        let answering = port_open(&status_http_addr());
        Some((answering, !answering && store.is_some_and(supragnosis_store::redb_in_use)))
    }))
}

/// A Homebrew job is retired by Homebrew: the plist or the unit is its file, and only `brew services
/// stop` removes it (a bootout or a `systemctl stop` alone would let it start again at the next login).
fn brew_services_stop(token: &str) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_default();
    let brew = [
        "/opt/homebrew/bin/brew".to_string(),
        "/usr/local/bin/brew".to_string(),
        "/home/linuxbrew/.linuxbrew/bin/brew".to_string(),
        format!("{home}/.linuxbrew/bin/brew"),
    ]
    .iter()
    .map(std::path::PathBuf::from)
    .find(|p| p.exists());
    let Some(brew) = brew else {
        anyhow::bail!(
            "a brew services job ({token}) owns the daemon but brew was not found - run `brew services stop {token}` and install again"
        );
    };
    let st = std::process::Command::new(&brew)
        .args(["services", "stop", token])
        .status()
        .context("failed to run brew services stop")?;
    if !st.success() {
        anyhow::bail!("`brew services stop {token}` failed - run it by hand, then install again");
    }
    Ok(())
}

/// `service install`: the LaunchAgent on macOS, the user unit on Linux.
fn service_install(args: ServiceInstallArgs) -> Result<()> {
    if cfg!(target_os = "linux") {
        systemd_install(args)
    } else {
        launchd_install(args)
    }
}

/// `service uninstall`: the LaunchAgent on macOS, the user unit on Linux.
fn service_uninstall() -> Result<()> {
    if cfg!(target_os = "linux") {
        systemd_uninstall()
    } else {
        launchd_uninstall()
    }
}

#[cfg(target_os = "macos")]
fn launchd_install(args: ServiceInstallArgs) -> Result<()> {
    use lifecycle::{LabelKind, Manager};
    let home = std::env::var("HOME").context("HOME is not set")?;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let state = canonical_service_state();

    // The job's environment: whatever the plist being replaced set, verbatim, then --env on top.
    let mut env = if state == "absent" { Default::default() } else { plist_env(&plist)? };
    for a in &args.env {
        let (k, v) = lifecycle::parse_env_arg(a).map_err(|e| anyhow::anyhow!(e))?;
        env.insert(k, v);
    }
    env.entry("SUPRAGNOSIS_HTTP_ADDR".to_string())
        .or_insert_with(|| "127.0.0.1:7373".to_string());
    // L7, before anything is touched: the job adds no exposure, whichever source a setting came from.
    lifecycle::refuse_exposure(&env, |a| parse_loopback_addr(a).is_ok())
        .map_err(|e| anyhow::anyhow!(e))?;

    // The store the JOB will open - from its own environment, since launchd does not hand it this
    // shell's - is what a second writer would contend for.
    let store = daemon_store_path(|k| env.get(k).cloned());
    let observed = observe_with(store.as_deref());
    let canonical_loaded = observed.jobs.iter().any(|j| j.kind == LabelKind::Canonical);

    // L1 and L5: refuse to share, refuse a holder nothing names, and refuse to overwrite a person's
    // file - the first and last unless told to take over.
    let hand = (state == "hand_written").then_some(lifecycle::HAND_WRITTEN_PLIST);
    let others =
        lifecycle::plan_install(&observed, args.take_over, hand).map_err(|e| anyhow::anyhow!(e))?;

    let exe = std::env::current_exe()?.canonicalize()?;
    let program = lifecycle::stable_program(&exe, |p| p.exists());
    let program_str = program.to_string_lossy().to_string();
    if program_str.contains("/target/debug/") || program_str.contains("/target/release/") {
        println!(
            "note: the job will run a build-tree binary ({program_str}); `cargo clean` removes it"
        );
    }

    // Retire the others (take-over).
    for m in &others {
        match m {
            Manager::Pidfile { pid } => stop_pidfile(*pid as i32)?,
            // Units exist only on Linux, which installs through systemd_install.
            Manager::Systemd(_) => {}
            Manager::Launchd(job) => match job.kind {
                LabelKind::Homebrew(token) => {
                    brew_services_stop(token)?;
                    println!(
                        "retired brew services job {} (brew services stop {token})",
                        job.label
                    );
                }
                LabelKind::Retired => {
                    launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", job.label)]);
                    let old = std::path::PathBuf::from(&home)
                        .join("Library/LaunchAgents")
                        .join(format!("{}.plist", job.label));
                    if old.exists() {
                        let to = move_aside(&old, job.label)?;
                        println!("retired {} - its plist is now {}", job.label, to.display());
                    } else {
                        println!("retired {}", job.label);
                    }
                }
                LabelKind::Canonical => {}
            },
        }
    }
    if canonical_loaded {
        launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", lifecycle::CANONICAL_LABEL)]);
    }
    // The plan saw a manager for whatever answered; this checks that retiring them freed the store.
    // A holder that outlives them (an app-spawned daemon beside a failing job) is the one the plan
    // could not see, and installing now would start the crash loop all the same.
    if !wait_until_released(store.as_deref()) {
        anyhow::bail!(
            "{} still answers, or the store is still held, after retiring the managers above - something else holds it, so not installing beside it.\n{}",
            status_http_addr(),
            lifecycle::UNRECOGNIZED_HOLDER
        );
    }
    if state == "hand_written" {
        let to = move_aside(&plist, lifecycle::CANONICAL_LABEL)?;
        println!("moved the hand-written plist aside: {}", to.display());
    }

    std::fs::create_dir_all(plist.parent().context("LaunchAgents path has no parent")?)?;
    std::fs::create_dir_all(log_dir())?;
    let text = lifecycle::render_plist(&program_str, &home, &env, env!("CARGO_PKG_VERSION"));
    std::fs::write(&plist, text).with_context(|| format!("failed to write {}", plist.display()))?;
    let st = std::process::Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{uid}"))
        .arg(&plist)
        .status()
        .context("failed to run launchctl bootstrap")?;
    if !st.success() {
        anyhow::bail!("launchctl bootstrap gui/{uid} {} failed", plist.display());
    }
    println!("installed {} -> {}", lifecycle::CANONICAL_LABEL, plist.display());
    println!("  program {program_str} serve");
    for (k, v) in &env {
        println!("  env     {k}={v}");
    }
    println!("  the daemon starts now and at every login (supragnosis service uninstall to undo)");
    // The model, so the first tool call does not find it missing (semantic-retrieval.md Section
    // 6). A failure is reported and does not fail the install: the daemon is useful without it.
    // `setup` asks for none and fetches it as its own step, which nothing waits on (O5).
    if args.no_model {
        println!("  model   not fetched here (--no-model) - `supragnosis embed fetch` fetches it");
    } else if let Err(e) = embed_fetch(true, false) {
        println!("  model   not fetched: {e:#} - run `supragnosis embed fetch` when online");
    }
    await_daemon(lifecycle::CANONICAL_LABEL)
}

#[cfg(target_os = "macos")]
fn launchd_uninstall() -> Result<()> {
    use lifecycle::LabelKind;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_service_path();
    let state = canonical_service_state();
    let observed = observe();
    if observed.jobs.iter().any(|j| j.kind == LabelKind::Canonical) {
        launchctl_quiet(&["bootout", &format!("gui/{uid}/{}", lifecycle::CANONICAL_LABEL)]);
        wait_until_released(daemon_store_path(|k| std::env::var(k).ok()).as_deref());
        println!("stopped {}", lifecycle::CANONICAL_LABEL);
    }
    match lifecycle::uninstall_file_step(state) {
        lifecycle::FileStep::Remove => {
            std::fs::remove_file(&plist)
                .with_context(|| format!("failed to remove {}", plist.display()))?;
            println!("removed {} - the daemon no longer starts at login", plist.display());
        }
        lifecycle::FileStep::MoveAside => {
            let to = move_aside(&plist, lifecycle::CANONICAL_LABEL)?;
            println!(
                "moved the hand-written plist aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        lifecycle::FileStep::Nothing => println!("no canonical job is installed"),
    }
    let rest: Vec<String> = observed
        .jobs
        .iter()
        .filter(|j| j.kind != LabelKind::Canonical)
        .map(|j| format!("  launchd {}", j.label))
        .collect();
    if !rest.is_empty() {
        println!("other managers are still loaded (not touched):\n{}", rest.join("\n"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn launchd_install(_args: ServiceInstallArgs) -> Result<()> {
    anyhow::bail!("service install manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither - `supragnosis serve` runs the daemon in the foreground")
}
#[cfg(not(target_os = "macos"))]
fn launchd_uninstall() -> Result<()> {
    anyhow::bail!("service uninstall manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither")
}

/// Why there is no user manager to install into, and what runs the daemon instead.
fn no_user_manager(why: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "systemctl --user does not work here ({why}) - as in many containers, or a session without a user bus - so there is no user manager to keep the daemon running. `supragnosis serve` runs it in the foreground, and `supragnosis start` runs it until you log out"
    )
}

/// The directories a unit's daemon writes outside `~/.supragnosis`, which `ProtectSystem=strict` and
/// `ProtectHome=read-only` would otherwise refuse it: a data directory or viewer socket its
/// environment moved elsewhere.
fn unit_writable_paths(
    env: &std::collections::BTreeMap<String, String>,
    home: &str,
) -> Vec<String> {
    let state = std::path::PathBuf::from(home).join(".supragnosis");
    let mut out: Vec<String> = Vec::new();
    let data = env.get("SUPRAGNOSIS_DATA_DIR").map(std::path::PathBuf::from);
    let sock = env
        .get("SUPRAGNOSIS_VIZ_SOCK")
        .and_then(|v| std::path::Path::new(v).parent().map(std::path::Path::to_path_buf));
    for dir in [data, sock].into_iter().flatten() {
        let d = dir.to_string_lossy().to_string();
        if dir.is_absolute() && !dir.starts_with(&state) && !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// `service install` on Linux (daemon-lifecycle.md Section 4, "On Linux"): the canonical user unit,
/// generated, enabled and started, with lingering asked for. The rules are the LaunchAgent's - refuse
/// to share, refuse a holder nothing names, never overwrite a file it did not write, carry the
/// environment, add no exposure - read for a unit.
fn systemd_install(args: ServiceInstallArgs) -> Result<()> {
    use lifecycle::{LabelKind, Manager};
    systemd_user_available().map_err(|why| no_user_manager(&why))?;
    let home = std::env::var("HOME").context("HOME is not set")?;
    let user = current_user().context("could not determine the user name (id -un)")?;
    let unit = canonical_service_path();
    let state = canonical_service_state();

    // The unit's environment: whatever the unit being replaced set, read as systemd reads it, then
    // --env on top (L5's carrying).
    let mut env = if state == "absent" {
        Default::default()
    } else {
        let text = std::fs::read_to_string(&unit)
            .with_context(|| format!("failed to read {}", unit.display()))?;
        lifecycle::parse_unit_env(&text, &home, &user)
            .map_err(|e| anyhow::anyhow!("{}: {e}", unit.display()))?
    };
    for a in &args.env {
        let (k, v) = lifecycle::parse_env_arg(a).map_err(|e| anyhow::anyhow!(e))?;
        env.insert(k, v);
    }
    env.entry("SUPRAGNOSIS_HTTP_ADDR".to_string())
        .or_insert_with(|| "127.0.0.1:7373".to_string());
    lifecycle::refuse_exposure(&env, |a| parse_loopback_addr(a).is_ok())
        .map_err(|e| anyhow::anyhow!(e))?;

    let store = daemon_store_path(|k| env.get(k).cloned());
    let observed = observe_with(store.as_deref());
    let hand = (state == "hand_written").then_some(lifecycle::HAND_WRITTEN_UNIT);
    let others =
        lifecycle::plan_install(&observed, args.take_over, hand).map_err(|e| anyhow::anyhow!(e))?;

    let exe = std::env::current_exe()?.canonicalize()?;
    let program = lifecycle::stable_program(&exe, |p| p.exists());
    let program_str = program.to_string_lossy().to_string();
    if program_str.contains("/target/debug/") || program_str.contains("/target/release/") {
        println!(
            "note: the unit will run a build-tree binary ({program_str}); `cargo clean` removes it"
        );
    }
    let writable = unit_writable_paths(&env, &home);
    let text = lifecycle::render_unit(&program_str, &env, &writable, env!("CARGO_PKG_VERSION"))
        .map_err(|e| anyhow::anyhow!("the unit cannot be written: {e}"))?;

    for m in &others {
        match m {
            Manager::Pidfile { pid } => stop_pidfile(*pid as i32)?,
            Manager::Systemd(job) => {
                if let LabelKind::Homebrew(token) = job.kind {
                    brew_services_stop(token)?;
                    println!(
                        "retired brew services unit {} (brew services stop {token})",
                        job.label
                    );
                }
            }
            Manager::Launchd(_) => {}
        }
    }
    // Whatever is at the canonical path stops before its file changes: a generated unit is
    // rewritten, a copied one moved aside.
    if state != "absent" || observed.units.iter().any(|j| j.kind == LabelKind::Canonical) {
        let _ = systemctl_user(&["disable", "--now", lifecycle::CANONICAL_UNIT]);
    }
    if !wait_until_released(store.as_deref()) {
        anyhow::bail!(
            "{} still answers, or the store is still held, after retiring the managers above - something else holds it, so not installing beside it.\n{}",
            status_http_addr(),
            lifecycle::UNRECOGNIZED_HOLDER
        );
    }
    let moved = if state == "hand_written" {
        let to = move_file_aside(&unit, lifecycle::moved_aside_unit_path(&home, unix_secs()))?;
        println!("moved the unit that was not generated aside: {}", to.display());
        Some(to)
    } else {
        None
    };

    std::fs::create_dir_all(unit.parent().context("the unit path has no parent")?)?;
    std::fs::create_dir_all(base_dir())?;
    for dir in &writable {
        std::fs::create_dir_all(dir).with_context(|| format!("failed to create {dir}"))?;
    }
    std::fs::write(&unit, text).with_context(|| format!("failed to write {}", unit.display()))?;
    systemctl_user(&["daemon-reload"])?;
    systemctl_user(&["enable", "--now", lifecycle::CANONICAL_UNIT])?;
    println!("installed {} -> {}", lifecycle::CANONICAL_UNIT, unit.display());
    println!("  program {program_str} serve");
    for (k, v) in &env {
        println!("  env     {k}={v}");
    }
    if let Some(to) = moved {
        println!(
            "  carried only the Environment= lines of the unit moved aside - compare the rest (an ExecStart with its own flags, ordering) with {}",
            to.display()
        );
    }
    // A user manager stops with the person's last session unless it lingers. polkit lets a person
    // turn it on for themselves on most distributions; where it does not, an administrator has to
    // (L8: asked, never escalated).
    match linger_state() {
        Some(true) => println!("  linger  on - it keeps running after you log out"),
        _ => {
            let out = std::process::Command::new("loginctl").arg("enable-linger").output();
            match out {
                Ok(o) if o.status.success() => println!(
                    "  linger  turned on (loginctl enable-linger) - it keeps running after you log out"
                ),
                Ok(o) => println!(
                    "  linger  off, and turning it on was refused ({}) - it stops when you log out until an administrator runs `sudo loginctl enable-linger {user}`",
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => println!(
                    "  linger  off, and loginctl could not be run ({e}) - it stops when you log out until an administrator runs `sudo loginctl enable-linger {user}`"
                ),
            }
        }
    }
    println!(
        "  the daemon starts now and at every login (supragnosis service uninstall to undo); its log is `journalctl --user -u {}`",
        lifecycle::CANONICAL_UNIT
    );
    if args.no_model {
        println!("  model   not fetched here (--no-model) - `supragnosis embed fetch` fetches it");
    } else if let Err(e) = embed_fetch(true, false) {
        println!("  model   not fetched: {e:#} - run `supragnosis embed fetch` when online");
    }
    await_daemon(lifecycle::CANONICAL_UNIT)
}

/// `service uninstall` on Linux: the canonical unit is disabled and stopped, and its file removed -
/// or moved aside when this command did not write it (L5). Lingering is left as it is, since other
/// units may need it.
fn systemd_uninstall() -> Result<()> {
    use lifecycle::LabelKind;
    systemd_user_available().map_err(|why| no_user_manager(&why))?;
    let home = std::env::var("HOME").context("HOME is not set")?;
    let unit = canonical_service_path();
    let state = canonical_service_state();
    let observed = observe();
    if state != "absent" || observed.units.iter().any(|j| j.kind == LabelKind::Canonical) {
        let _ = systemctl_user(&["disable", "--now", lifecycle::CANONICAL_UNIT]);
        wait_until_released(daemon_store_path(|k| std::env::var(k).ok()).as_deref());
        println!("stopped and disabled {}", lifecycle::CANONICAL_UNIT);
    }
    match lifecycle::uninstall_file_step(state) {
        lifecycle::FileStep::Remove => {
            std::fs::remove_file(&unit)
                .with_context(|| format!("failed to remove {}", unit.display()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!("removed {} - the daemon no longer starts at login", unit.display());
        }
        lifecycle::FileStep::MoveAside => {
            let to = move_file_aside(&unit, lifecycle::moved_aside_unit_path(&home, unix_secs()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!(
                "moved the unit that was not generated aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        lifecycle::FileStep::Nothing => println!("no canonical unit is installed"),
    }
    let rest: Vec<String> = observed
        .units
        .iter()
        .filter(|j| j.kind != LabelKind::Canonical)
        .map(|j| format!("  systemd {}", j.label))
        .collect();
    if !rest.is_empty() {
        println!("other managers are still enabled (not touched):\n{}", rest.join("\n"));
    }
    if linger_state() == Some(true) {
        println!("lingering is left on - other units may need it (`loginctl disable-linger` turns it off)");
    }
    Ok(())
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
        /// Bearer token presented to those servers. The older flat shape: one credential for every
        /// host in `servers`. Superseded by `[[sync.server]]`, which carries one each.
        pub auth_token: Option<String>,
        /// Per-server entries, each with its own credential. Named `server` so the file reads
        /// `[[sync.server]]`, mirroring the per-peer `[[server.allowlist]]` shape.
        #[serde(default)]
        pub server: Vec<ServerEntry>,
        /// Retired (sync-correctness.md Section 10). It accepted any certificate, so every round
        /// handed the bearer to whoever answered. Still parsed, so a file that sets it keeps working,
        /// but never read: setting it only earns a note saying to name the hub's certificate with
        /// `ca` instead.
        #[serde(default)]
        pub insecure_tls: bool,
        /// Origin-key directory {node_id -> public key hex} for verifying pulled events (F6).
        /// Superseded by the log-borne canon-policy binding in Phase 5.
        #[serde(default)]
        pub origin_keys: std::collections::BTreeMap<String, String>,
        /// No longer read: principals, whom a hub served these to, were removed in 0.11.0
        /// (remote-server.md Section 13). Parsed so a config that still names them starts the
        /// daemon; [`SyncSection::serve_note`] says so.
        #[serde(default)]
        pub serve_workspaces: Vec<String>,
    }

    impl SyncSection {
        /// The note for `serve_workspaces` left in a config: read, and not used.
        pub fn serve_note(&self) -> Option<String> {
            (!self.serve_workspaces.is_empty()).then(|| {
                "[sync] serve_workspaces is no longer read: hubs serve no principals since 0.11.0, \
                 and this node tells every hub to serve none of its workspaces. Remove the key."
                    .to_string()
            })
        }
    }

    /// One sync server and the credential this node presents to it. Denies unknown keys for the
    /// reason every other section does: a typo in the place that decides who this node authenticates
    /// to is where being wrong is most expensive (P5/P18).
    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ServerEntry {
        pub url: String,
        pub auth_token: String,
        /// The CA, or the hub's self-signed certificate, to trust for this hub: a PEM file. How a
        /// self-signed hub is reached without `insecure_tls` (sync-correctness.md Section 10).
        #[serde(default)]
        pub ca: Option<std::path::PathBuf>,
    }

    impl SyncSection {
        /// The servers to talk to, each with its own credential.
        ///
        /// Two shapes are accepted and never blended. Both present at once is refused rather than
        /// resolved by precedence: a precedence rule here is a silent degrade wearing a
        /// specification, and P5 asks explicit configuration to work or fail
        /// (negotiated-surface.md N8).
        /// The servers to talk to, each with its own credential - plus what had to be worked
        /// around to get there.
        ///
        /// **Nothing here fails.** A federation configuration mistake disables or narrows
        /// federation; it does not stop a node serving its own knowledge. That is the shape this
        /// codebase already uses for a subsystem it cannot bring up - a missing embedder proceeds
        /// with keyword search, a viewer socket that will not bind proceeds without the viewer - and
        /// a sync token is not different in kind. Refusing to start was the first version, and it
        /// turned a broken hub link into a dead node.
        ///
        /// It does not degrade silently either, which is the half P5 actually asks for. Every
        /// workaround comes back as a note: logged at startup and carried into `sync_status`, so it
        /// sits on the surface an operator reads rather than in a log they scrolled past once.
        pub fn links(&self) -> (Vec<supragnosis_sync::ServerLink>, Vec<String>) {
            let mut notes = Vec::new();
            if self.insecure_tls {
                notes.push(
                    "[sync] insecure_tls is no longer read: it accepted any certificate, so the \
                     bearer went to whoever answered. Every link now verifies its hub. For a \
                     self-signed hub, copy its tls_cert and name it with `ca = \"<pem file>\"` in \
                     its [[sync.server]] entry; for a hub on this machine, http:// needs no \
                     certificate. Remove insecure_tls once that is done."
                        .to_string(),
                );
            }
            let flat_present = !self.servers.is_empty() || self.auth_token.is_some();

            if !self.server.is_empty() {
                if flat_present {
                    // Precedence, loudly. An earlier version refused, on the ground that choosing
                    // between two stated intents is a guess - true, but the cost fell on an upgrade
                    // rather than on the mistake. Per-server entries win because the other direction
                    // is worse than dropping a host: one flat token presented to a host that has its
                    // own would be the WRONG credential on the wire.
                    notes.push(format!(
                        "[sync] has both per-server entries and the flat servers/auth_token keys. \
                         The {} entr(ies) are used and the flat keys are IGNORED, including {} host(s) \
                         listed only there. Remove the flat keys once they are folded in.",
                        self.server.len(),
                        self.servers.len()
                    ));
                }
                let links = self
                    .server
                    .iter()
                    .filter_map(|e| self.link(&e.url, &e.auth_token, e.ca.as_deref(), &mut notes))
                    .collect();
                return (links, notes);
            }

            if self.servers.is_empty() {
                return (Vec::new(), notes);
            }
            let Some(token) = self.auth_token.clone() else {
                notes.push(format!(
                    "[sync] lists {} server(s) but no auth_token, and every hub refuses an \
                     unauthenticated caller - so federation is OFF rather than failing every round. \
                     Set auth_token, or move each host to a [[sync.server]] entry with its own.",
                    self.servers.len()
                ));
                return (Vec::new(), notes);
            };
            let links = self
                .servers
                .iter()
                .filter_map(|url| self.link(url, &token, None, &mut notes))
                .collect();
            (links, notes)
        }

        /// One host as this node will reach it (sync-correctness.md Section 10). The bearer is
        /// never sent where it could be read in transit: a plain `http://` URL to another machine
        /// disables the link, and every link verifies its host - by the system's roots, or by the
        /// certificate `ca` names. These are ignored rather than refused, because ignoring them can
        /// only make a link fail, never send more (P24), and each says what to change.
        fn link(
            &self,
            url: &str,
            token: &str,
            ca: Option<&std::path::Path>,
            notes: &mut Vec<String>,
        ) -> Option<supragnosis_sync::ServerLink> {
            let parsed = reqwest::Url::parse(url).ok();
            let loopback =
                parsed.as_ref().and_then(|u| u.host_str()).is_some_and(crate::is_loopback_host);
            if parsed.as_ref().is_some_and(|u| u.scheme() == "http") && !loopback {
                notes.push(format!(
                    "[sync] {url} is plain HTTP to another machine, so its bearer would cross the \
                     network unencrypted. The link is DISABLED for this run. Use https:// - the \
                     hub's [server] listener terminates TLS."
                ));
                return None;
            }
            let ca_pem = match ca {
                None => None,
                Some(path) => match std::fs::read(path) {
                    Ok(pem) => Some(pem),
                    Err(e) => {
                        notes.push(format!(
                            "[sync] {url}: its ca {} could not be read ({e}). The link is \
                             DISABLED for this run rather than trusting a certificate it did not \
                             name.",
                            path.display()
                        ));
                        return None;
                    }
                },
            };
            Some(supragnosis_sync::ServerLink {
                url: url.to_string(),
                auth_token: token.to_string(),
                ca_pem,
            })
        }
    }

    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ServerSection {
        /// Sync API bind (IP:port). Non-loopback demands TLS and an admitted node or an open
        /// invitation (pairing.md PA1).
        pub listen: String,
        pub tls_cert: Option<String>,
        pub tls_key: Option<String>,
        #[serde(default)]
        pub allowlist: Vec<supragnosis_sync::http::AllowEntry>,
        /// Principals are no longer read: the hub's agent surface was removed in 0.11.0
        /// (remote-server.md Section 13). Parsed, whatever their shape, so a config that still
        /// names them starts the daemon; [`principals_note`] says so.
        #[serde(default)]
        pub principals: Vec<toml::Value>,
    }

    /// The note for `[[server.principals]]` left in a config: read, and not used.
    pub fn principals_note(srv: Option<&ServerSection>) -> Option<String> {
        srv.filter(|s| !s.principals.is_empty()).map(|_| {
            "[[server.principals]] is no longer read: the hub's agent surface was removed in \
             0.11.0, and the hub serves only sync and pairing. Remove the entries."
                .to_string()
        })
    }

    /// `~/.supragnosis` - where the node key, the config and the daemon token live. Public because
    /// the MCP token is not a federation concern but wants the same directory (and the same 0700
    /// that already protects it) rather than a second answer to "where does state go".
    pub fn fed_base_dir() -> PathBuf {
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
        load_at(&config_path())
    }

    pub fn load_at(path: &std::path::Path) -> Result<Option<FileConfig>> {
        if !path.exists() {
            return Ok(None);
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(cfg))
    }

    /// Edits `supragnosis.toml` in place under the lock file beside it (docs/pairing.md Section
    /// 4.4). The file is re-read inside the lock, so two edits - pairing's, the narrowing act's,
    /// `principal`'s - never lose each other's change; `edit` returns the new text, built with the
    /// format-preserving editor so every line the person wrote stays; and the new text replaces the
    /// old through a rename, so a reader never sees half of it. A file that does not exist yet is
    /// read as empty and made owner-only.
    pub fn edit_config_at<T>(
        path: &std::path::Path,
        edit: impl FnOnce(&str) -> Result<(String, T)>,
    ) -> Result<T> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("supragnosis.toml");
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let lock_path = path.with_file_name(format!(".{name}.lock"));
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("opening {}", lock_path.display()))?;
        lock.lock().with_context(|| format!("locking {}", lock_path.display()))?;
        let (text, existed) = match std::fs::read_to_string(path) {
            Ok(t) => (t, true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let (edited, out) = edit(&text)?;
        if edited != text {
            // Written to a file that is owner-only from its first byte, then given the old file's
            // mode and renamed over it: the file may hold a sync credential, and no reader sees
            // half of it. A new file stays owner-only; nothing touches the directory it lands
            // in, which SUPRAGNOSIS_CONFIG may put anywhere.
            use std::io::Write;
            let tmp = path.with_file_name(format!(".{name}.supragnosis-new"));
            let _ = std::fs::remove_file(&tmp);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f =
                options.open(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(edited.as_bytes())?;
            f.sync_all()?;
            if existed {
                if let Ok(meta) = std::fs::metadata(path) {
                    std::fs::set_permissions(&tmp, meta.permissions())?;
                }
            }
            std::fs::rename(&tmp, path)
                .with_context(|| format!("installing {}", path.display()))?;
        }
        Ok(out)
    }

    /// Narrows one peer's `shared_workspaces` in `supragnosis.toml`, preserving the file's comments
    /// and layout, and returns the set that is now granted.
    ///
    /// **Narrowing only.** The requested set must be a subset of what the peer already has; a request
    /// naming anything it does not currently hold is refused rather than partially applied. That
    /// asymmetry is the whole safety argument for putting this on a surface at all: the act can only
    /// move in the direction P17 already prefers (less sharing), so the worst outcome of a mistake -
    /// or of a console left open - is that a peer reads less than intended. Widening stays where it
    /// is, in the file, where it is a deliberate act with the operator's full context.
    ///
    /// The file is re-read here rather than edited from the process's startup copy, so an operator
    /// who hand-edited it while the daemon ran does not have their change silently reverted by a
    /// console click. Removing every workspace is permitted and means the peer stays admitted but may
    /// read nothing; removing the peer itself is not this function's job (the deferred revocation
    /// workflow, federation.md Section 11).
    pub fn narrow_shared_workspaces(node_id: &str, keep: &[String]) -> Result<Vec<String>> {
        let path = config_path();
        edit_config_at(&path, |text| narrow_text(&path, text, node_id, keep))
    }

    fn narrow_text(
        path: &std::path::Path,
        text: &str,
        node_id: &str,
        keep: &[String],
    ) -> Result<(String, Vec<String>)> {
        let mut doc: toml_edit::DocumentMut =
            text.parse().with_context(|| format!("parsing {}", path.display()))?;

        let entries = doc
            .get_mut("server")
            .and_then(|s| s.get_mut("allowlist"))
            .and_then(|a| a.as_array_of_tables_mut())
            .ok_or_else(|| anyhow::anyhow!("no [[server.allowlist]] in {}", path.display()))?;

        // Every entry for the peer, not the first: a rotated token leaves two, each admitted on
        // its own bearer, and narrowing one would leave the other reading the old grant.
        let mut matching: Vec<&mut toml_edit::Table> = entries
            .iter_mut()
            .filter(|t| t.get("node_id").and_then(|v| v.as_str()) == Some(node_id))
            .collect();
        if matching.is_empty() {
            anyhow::bail!("no allowlist entry for node_id {node_id}");
        }
        let shared = |t: &toml_edit::Table| -> Vec<String> {
            t.get("shared_workspaces")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        let current: Vec<Vec<String>> = matching.iter().map(|t| shared(t)).collect();
        let union: Vec<&String> = current.iter().flatten().collect();
        let widened: Vec<&String> = keep.iter().filter(|w| !union.contains(w)).collect();
        if !widened.is_empty() {
            anyhow::bail!(
                "refusing to widen: {node_id} does not currently share {widened:?} (it shares \
                 {union:?}). This surface only narrows - grant a workspace by editing {}",
                path.display()
            );
        }

        // Each entry keeps what it already had of `keep`, so none is widened by another's grant.
        let mut now: Vec<String> = Vec::new();
        for (entry, had) in matching.iter_mut().zip(&current) {
            let mut arr = toml_edit::Array::new();
            for w in keep.iter().filter(|w| had.contains(w)) {
                arr.push(w.as_str());
                if !now.contains(w) {
                    now.push(w.clone());
                }
            }
            entry["shared_workspaces"] = toml_edit::value(arr);
        }
        // Written by `edit_config_at`, through a temp file and a rename: the file holds the node's
        // sync credentials, and a reader - the daemon re-reads it right after this - must never
        // see half of it.
        Ok((doc.to_string(), now))
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
        getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
        super::write_secret(&path, &secret)?;
        tracing::info!(
            path = %path.display(),
            "generated the node keypair (once - the node_id is immutable, F14). The key and node.seq \
             beside it are this node: move them with its store, never copy them to a second running \
             node (docs/sync-correctness.md Section 5)"
        );
        Ok(supragnosis_core::NodeIdentity::from_secret_bytes(secret))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// remote-server.md Section 13: `serve_workspaces` left in a config parses and is said to do
        /// nothing.
        #[test]
        fn serve_workspaces_left_in_a_config_is_said_to_do_nothing() {
            let cfg: FileConfig = toml::from_str(
                "[sync]\nshare_workspaces = [\"team\"]\nserve_workspaces = [\"team\", \"private\"]\n",
            )
            .unwrap();
            let note = cfg.sync.serve_note().expect("a note");
            assert!(note.contains("no longer read") && note.contains("0.11.0"), "{note}");
            let none: FileConfig =
                toml::from_str("[sync]\nshare_workspaces = [\"team\"]\n").unwrap();
            assert_eq!(none.sync.serve_note(), None);
        }

        /// remote-server.md Section 13: a hub's config that still names principals, from before 0.11.0,
        /// parses - so the daemon starts - and the key is said to be no longer read, whatever the
        /// entries hold.
        #[test]
        fn principals_left_in_a_config_are_read_and_said_to_do_nothing() {
            let left = r#"
                [server]
                listen = "127.0.0.1:7420"
                [[server.principals]]
                name = "alice"
                token_hash = "abc"
                read = ["team"]
                [[server.principals]]
                anything = 1
            "#;
            let cfg: FileConfig = toml::from_str(left).expect("an old config still parses");
            let note = principals_note(cfg.server.as_ref()).expect("a note");
            assert!(note.contains("no longer read") && note.contains("0.11.0"), "{note}");
            let none: FileConfig =
                toml::from_str("[server]\nlisten = \"127.0.0.1:7420\"\n").unwrap();
            assert_eq!(principals_note(none.server.as_ref()), None);
        }

        /// The documented supragnosis.toml shape parses; unknown keys are rejected loudly (P24, N8).
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

            // The per-server shape, which carries a credential each rather than one for all.
            let per_server = r#"
                [sync]
                share_workspaces = ["supragnosis"]
                [[sync.server]]
                url = "https://hub-cloud.internal:7420"
                auth_token = "tok-cloud"
                [[sync.server]]
                url = "https://hub-net.internal:7420"
                auth_token = "tok-net"
            "#;
            let cfg: FileConfig = toml::from_str(per_server).expect("per-server shape must parse");
            let (links, notes) = cfg.sync.links();
            assert!(notes.is_empty(), "a clean per-server config needs no workaround: {notes:?}");
            assert_eq!(links.len(), 2);
            assert_eq!(links[0].auth_token, "tok-cloud");
            assert_eq!(links[1].auth_token, "tok-net", "each host gets its own credential");

            // The flat shape still resolves, to the same token repeated - which is what the
            // per-server shape exists to replace, not something to break on the way there.
            let flat = r#"
                [sync]
                servers = ["https://a:7420", "https://b:7420"]
                auth_token = "shared"
            "#;
            let cfg: FileConfig = toml::from_str(flat).expect("the flat shape still parses");
            let (links, notes) = cfg.sync.links();
            assert!(notes.is_empty(), "the flat shape alone is still a supported configuration");
            assert_eq!(links.len(), 2);
            assert!(links.iter().all(|l| l.auth_token == "shared"));

            // Both shapes at once resolve by a stated precedence, loudly: a rule that chose silently
            // which credential a host is given is the class of configuration failure P5 asks to be
            // loud about, and the note below is what keeps it loud (negotiated-surface.md N8).
            let both = r#"
                [sync]
                servers = ["https://a:7420"]
                auth_token = "shared"
                [[sync.server]]
                url = "https://a:7420"
                auth_token = "own"
            "#;
            let cfg: FileConfig = toml::from_str(both).expect("both shapes parse individually");
            let (links, notes) = cfg.sync.links();
            // Precedence, loudly. Refusing was the first version and it broke an upgrade over a
            // mistake that costs nothing to work around; per-server wins because the other way round
            // would put one flat token on the wire to a host that has its own.
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].auth_token, "own", "the per-server credential is the one used");
            assert_eq!(notes.len(), 1, "and the operator is told the flat keys were ignored");
            assert!(notes[0].contains("IGNORED"), "the note names what was dropped: {notes:?}");

            // `servers` with no token cannot reach any hub, so it fails at load rather than at the
            // first round - every hub refuses an unauthenticated caller (F6).
            let tokenless = "[sync]\nservers = [\"https://a:7420\"]\n";
            let cfg: FileConfig = toml::from_str(tokenless).expect("parses");
            let (links, notes) = cfg.sync.links();
            // Federation off, node up. A missing sync token is a subsystem this build cannot bring
            // up, which is the case the embedder and the viewer socket already answer by degrading
            // with a loud line rather than by killing a node that serves its own knowledge.
            assert!(links.is_empty(), "no link can be built without a credential");
            assert_eq!(notes.len(), 1, "and it is not silent: {notes:?}");
            assert!(
                notes[0].contains("federation is OFF"),
                "the note says what stopped: {notes:?}"
            );

            // A typo must fail loudly, not silently disable a role (P5).
            let typo = "share_workspace = [\"x\"]\n";
            assert!(toml::from_str::<FileConfig>(typo).is_err());

            // The same rule one level down, inside an allowlist entry. This was the one struct that
            // did not deny unknown keys (federation.md Section 9 recorded it as a standing gap), so a
            // misspelled key in the place that decides who may connect and what they may read parsed
            // clean and did nothing. An entry is where a typo is most expensive (P17/P18): the safe
            // way to be wrong about it is to refuse to start.
            let entry_typo = r#"
                [server]
                listen = "127.0.0.1:7420"
                [[server.allowlist]]
                node_id = "abc"
                public_key_hex = "deadbeef"
                bearer_hash = "hash"
                shared_workspace = ["supragnosis"]
            "#;
            let err = toml::from_str::<FileConfig>(entry_typo)
                .expect_err("a misspelled key inside an allowlist entry must not parse");
            assert!(
                err.to_string().contains("shared_workspace"),
                "the error has to name the offending key: {err}"
            );
        }

        /// Narrowing rewrites only the peer's arrays - every one of them - and refuses to widen.
        ///
        /// Comment preservation is not cosmetic here. `supragnosis.toml` is a hand-maintained
        /// declaration - the hub config in the author's own deployment carries paragraphs explaining
        /// why it has no `[sync]` section - and a console that reformatted it would make operators
        /// stop trusting the console. Serializing the parsed struct back out would do exactly that,
        /// which is why this edits the document rather than the model.
        #[test]
        fn a_round_waits_five_minutes_and_backs_off_to_an_hour() {
            use std::time::Duration;
            let five = Duration::from_secs(300);
            assert_eq!(crate::next_round_wait(Duration::ZERO, false), five);
            assert_eq!(crate::next_round_wait(Duration::ZERO, true), Duration::from_secs(600));
            let mut wait = Duration::ZERO;
            for _ in 0..10 {
                wait = crate::next_round_wait(wait, true);
            }
            assert_eq!(wait, Duration::from_secs(3600), "a failing hub is asked at most hourly");
            assert_eq!(
                crate::next_round_wait(wait, false),
                five,
                "one round that works resets the wait"
            );
        }

        /// docs/pairing.md Section 4.4: edits made at once, from many threads, each re-read the file
        /// inside the lock, so none loses another's change, and every line the person wrote stays.
        #[test]
        fn two_edits_never_lose_each_others_change() {
            let dir = std::env::temp_dir().join(format!("sg-edit-lock-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("supragnosis.toml");
            std::fs::write(&path, "# the person's line\nhost_label = \"hub\"\n").unwrap();
            let edits: Vec<_> = (0..16)
                .map(|i| {
                    let path = path.clone();
                    std::thread::spawn(move || {
                        edit_config_at(&path, |text| {
                            let mut doc: toml_edit::DocumentMut = text.parse()?;
                            std::thread::sleep(std::time::Duration::from_millis(2));
                            doc[&format!("k{i}")] = toml_edit::value(i as i64);
                            Ok((doc.to_string(), ()))
                        })
                        .unwrap()
                    })
                })
                .collect();
            edits.into_iter().for_each(|e| e.join().unwrap());
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.starts_with("# the person's line\n"), "{text}");
            for i in 0..16 {
                assert!(text.contains(&format!("k{i} = {i}")), "edit {i} was lost: {text}");
            }
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn narrowing_preserves_the_file_and_refuses_to_widen() {
            let dir = std::env::temp_dir().join(format!(
                "supragnosis-narrow-{}-{}",
                std::process::id(),
                supragnosis_core::now_millis()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let path = dir.join("supragnosis.toml");
            let original = r#"# supragnosis hub - explains itself, and must keep explaining itself
host_label = "hub"

[server]
listen = "0.0.0.0:7420"   # F10: non-loopback needs TLS and an admitted node or an open invitation

# ashons-MacBook-Air
[[server.allowlist]]
node_id = "peer-a"
public_key_hex = "aa"
bearer_hash = "hh"
shared_workspaces = ["alpha", "beta", "gamma"]

# the same peer, after a token rotation left its old entry in place
[[server.allowlist]]
node_id = "peer-a"
public_key_hex = "aa"
bearer_hash = "h2"
shared_workspaces = ["alpha", "gamma"]
"#;
            std::fs::write(&path, original).expect("write fixture");
            let prev = std::env::var("SUPRAGNOSIS_CONFIG").ok();
            std::env::set_var("SUPRAGNOSIS_CONFIG", &path);

            // Widening is refused, and says what is actually granted.
            let err = narrow_shared_workspaces("peer-a", &["alpha".into(), "delta".into()])
                .expect_err("widening must be refused");
            assert!(err.to_string().contains("delta"), "names what was refused: {err}");
            assert_eq!(
                std::fs::read_to_string(&path).expect("read"),
                original,
                "a refused request must not touch the file"
            );

            // A subset is applied to every entry for the peer: each bearer is admitted on its
            // own, so one left wide would still read what the console says was taken away.
            let kept = narrow_shared_workspaces("peer-a", &["alpha".into(), "beta".into()])
                .expect("narrow");
            assert_eq!(kept, vec!["alpha".to_string(), "beta".to_string()]);
            let after = std::fs::read_to_string(&path).expect("read");
            assert!(
                after.contains(r#"shared_workspaces = ["alpha", "beta"]"#)
                    && after.contains(r#"shared_workspaces = ["alpha"]"#)
                    && !after.contains("gamma"),
                "both entries narrowed, neither widened by the other's grant: {after}"
            );
            assert!(
                after.contains("# supragnosis hub - explains itself")
                    && after.contains("# ashons-MacBook-Air")
                    && after.contains("# F10: non-loopback"),
                "every comment survives the edit: {after}"
            );
            assert!(after.contains(r#"host_label = "hub""#), "untouched keys stay put");
            // It still parses as the config the daemon loads - an edit that produced valid TOML but
            // an invalid config would only be discovered at the next restart.
            toml::from_str::<FileConfig>(&after).expect("the edited file is still a valid config");

            // Emptying is a legitimate narrowing: admitted, but may read nothing.
            narrow_shared_workspaces("peer-a", &[]).expect("empty is a narrowing");
            let after = std::fs::read_to_string(&path).expect("read");
            assert_eq!(after.matches("shared_workspaces = []").count(), 2, "{after}");

            // An unknown peer is an error, not a silent no-op.
            assert!(narrow_shared_workspaces("nobody", &[]).is_err());

            match prev {
                Some(v) => std::env::set_var("SUPRAGNOSIS_CONFIG", v),
                None => std::env::remove_var("SUPRAGNOSIS_CONFIG"),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// docs/compatibility.md Section 6: each machine-read answer has one example document in
/// tests/fixtures/json, read by these tests and by the desktop app's. The producer and its example
/// must have the same keys at every level and the same kind of value wherever both hold one. A
/// field this side renames or drops fails here; a field the app reads that the example lacks fails
/// on the app's side.
#[cfg(test)]
mod service_files {
    use super::*;

    /// L5: the operator's file is theirs - nothing deletes a plist or a unit the product did not
    /// write. Uninstall removes only the file it generated and moves one a person wrote aside; the
    /// move is a rename that keeps every byte at the timestamped path, under a directory it makes,
    /// and one that cannot be made leaves the file where it was. Every service command's file step
    /// goes through these: a removal appears only in uninstall's arm for the generated file.
    #[test]
    fn an_operators_file_is_moved_aside_never_deleted() {
        use lifecycle::{uninstall_file_step, FileStep};
        assert_eq!(uninstall_file_step("generated"), FileStep::Remove);
        assert_eq!(uninstall_file_step("hand_written"), FileStep::MoveAside);
        assert_eq!(uninstall_file_step("absent"), FileStep::Nothing);

        let home = std::env::temp_dir().join(format!("sg-move-aside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let agents = home.join("Library/LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let plist = agents.join("com.supragnosis.daemon.plist");
        std::fs::write(&plist, "<plist>the operator's own</plist>").unwrap();
        let dst =
            lifecycle::moved_aside_path(home.to_str().unwrap(), lifecycle::CANONICAL_LABEL, 7);
        assert_eq!(move_file_aside(&plist, dst.clone()).unwrap(), dst);
        assert!(!plist.exists(), "out of LaunchAgents, so launchd stops loading it");
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "<plist>the operator's own</plist>");

        let unit = home.join(".config/systemd/user/supragnosis.service");
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "[Service]\n").unwrap();
        // The place for the copy is taken by a file, so its directory cannot be made.
        std::fs::write(home.join(".supragnosis/systemd"), "").unwrap();
        let blocked = lifecycle::moved_aside_unit_path(home.to_str().unwrap(), 7);
        assert!(move_file_aside(&unit, blocked).is_err());
        assert_eq!(std::fs::read_to_string(&unit).unwrap(), "[Service]\n", "left where it was");
        let _ = std::fs::remove_dir_all(&home);

        // Each function's working body, not a stub for another system: the real ones open with a
        // `use` or take their arguments.
        let main = include_str!("main.rs");
        let body = |head: &str| {
            let at = main.find(head).unwrap_or_else(|| panic!("no {head}"));
            let rest = &main[at..];
            &rest[..rest.find("\n}\n").expect("it closes")]
        };
        for uninstall in [
            "fn launchd_uninstall() -> Result<()> {\n    use",
            "fn systemd_uninstall() -> Result<()> {\n    use",
        ] {
            let b = body(uninstall);
            let remove = b.find("FileStep::Remove =>").expect("the generated arm");
            let aside = b.find("FileStep::MoveAside =>").expect("the hand-written arm");
            assert_eq!(b.matches("remove_file(").count(), 1, "{uninstall}: one removal");
            let removal = b.find("remove_file(").unwrap();
            assert!(remove < removal && removal < aside, "{uninstall}: only of the generated file");
            assert!(
                b[aside..].contains("move_"),
                "{uninstall}: the hand-written one is moved aside"
            );
        }
        for install in ["fn launchd_install(args", "fn systemd_install(args"] {
            let b = body(install);
            assert!(b.contains("move_"), "{install} moves a hand-written file aside");
            assert!(!b.contains("remove_file("), "{install} removes no file");
        }
    }
}

#[cfg(test)]
mod json_contract {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    fn example(name: &str) -> Value {
        let text = match name {
            "status" => include_str!("../tests/fixtures/json/status.json"),
            "connect" => include_str!("../tests/fixtures/json/connect.json"),
            other => panic!("no example named {other}"),
        };
        serde_json::from_str(text).expect("the example is JSON")
    }

    fn same_shape(example: &Value, made: &Value, at: &str) -> Result<(), String> {
        match (example, made) {
            (Value::Null, _) | (_, Value::Null) => Ok(()),
            (Value::Object(e), Value::Object(m)) => {
                let (ek, mk): (BTreeSet<_>, BTreeSet<_>) = (e.keys().collect(), m.keys().collect());
                if ek != mk {
                    return Err(format!("{at}: the example has {ek:?}, the CLI sends {mk:?}"));
                }
                ek.iter().try_for_each(|k| same_shape(&e[*k], &m[*k], &format!("{at}.{k}")))
            }
            (Value::Array(e), Value::Array(m)) => {
                if e.len() != m.len() {
                    return Err(format!("{at}: {} in the example, {} sent", e.len(), m.len()));
                }
                e.iter()
                    .zip(m)
                    .enumerate()
                    .try_for_each(|(i, (x, y))| same_shape(x, y, &format!("{at}[{i}]")))
            }
            (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_)) => Ok(()),
            _ => Err(format!("{at}: the example holds {example}, the CLI sends {made}")),
        }
    }

    /// SR3: the line says on with the model, off with the daemon's reason, and off for a node
    /// with no embedder at all; the document carries the daemon's report as it came.
    #[test]
    fn status_names_the_semantic_state() {
        let on = serde_json::json!({"model": "paraphrase-multilingual-minilm-l12-v2-q-384", "dims": 384, "state": "on"});
        assert_eq!(
            semantic_line(Some(&on)),
            "semantic on (paraphrase-multilingual-minilm-l12-v2-q-384)"
        );
        let off = serde_json::json!({"model": "m-384", "dims": 384, "state": "off",
            "note": "semantic retrieval is off: the embedding model multilingual is not in /x - run `supragnosis embed fetch` once (224 MB); retrieval is keyword only until then"});
        let line = semantic_line(Some(&off));
        assert!(line.starts_with("semantic off - ") && line.contains("embed fetch"), "{line}");
        let none = semantic_line(None);
        assert!(none.starts_with("semantic off - keyword only"), "{none}");
        assert_eq!(semantic_line(Some(&serde_json::Value::Null)), none);

        // semantic-retrieval.md Section 12: the backfill's progress and its failure are said; a
        // finished one says nothing more.
        let running = serde_json::json!({"model": "m-384", "dims": 384, "state": "on",
            "backfill": {"state": "running", "found": 120, "embedded": 40}});
        assert_eq!(
            semantic_line(Some(&running)),
            "semantic on (m-384) - embedding what was written before the model: 40 of 120"
        );
        let stopped = serde_json::json!({"model": "m-384", "dims": 384, "state": "on",
            "backfill": {"state": "stopped", "found": 120, "embedded": 40, "note": "an embed failed: x"}});
        let line = semantic_line(Some(&stopped));
        assert!(
            line.contains("stopped at 40 of 120") && line.ends_with("an embed failed: x"),
            "{line}"
        );
        let done = serde_json::json!({"model": "m-384", "dims": 384, "state": "on",
            "backfill": {"state": "done", "found": 120, "embedded": 120}});
        assert_eq!(semantic_line(Some(&done)), "semantic on (m-384)");
    }

    /// SR10 (semantic-retrieval.md Section 13): the rebuild stops only a daemon this CLI manages for
    /// this home - a `start` daemon, or the login job whose file is here - and refuses every other
    /// holder with nothing changed; with nothing running it resets, and starts the login job if this
    /// home installs one.
    #[test]
    fn rebuild_stops_only_a_daemon_this_home_manages() {
        use lifecycle::{Job, LabelKind, Manager, Situation};
        let job =
            |kind| Job { label: lifecycle::CANONICAL_LABEL, kind, pid: Some(7), last_exit: None };
        assert_eq!(
            rebuild_plan(&Situation::Stopped, "absent"),
            RebuildPlan::ResetOnly { start_job: false }
        );
        assert_eq!(
            rebuild_plan(&Situation::Stopped, "generated"),
            RebuildPlan::ResetOnly { start_job: true }
        );
        assert_eq!(
            rebuild_plan(&Situation::One(Manager::Pidfile { pid: 7 }), "absent"),
            RebuildPlan::StopResetStart
        );
        for state in ["generated", "hand_written"] {
            assert_eq!(
                rebuild_plan(&Situation::One(Manager::Launchd(job(LabelKind::Canonical))), state),
                RebuildPlan::StopResetStart,
                "the login job this home installs"
            );
            assert_eq!(
                rebuild_plan(&Situation::One(Manager::Systemd(job(LabelKind::Canonical))), state),
                RebuildPlan::StopResetStart
            );
        }
        let refused = |plan: RebuildPlan| match plan {
            RebuildPlan::Refuse(why) => why,
            other => panic!("expected a refusal, got {other:?}"),
        };
        // A login job is the user's, not the home's: one loaded from another home's file is refused.
        let other_home = refused(rebuild_plan(
            &Situation::One(Manager::Launchd(job(LabelKind::Canonical))),
            "absent",
        ));
        let brew = refused(rebuild_plan(
            &Situation::One(Manager::Launchd(job(LabelKind::Homebrew("supragnosis-server")))),
            "generated",
        ));
        let holder = refused(rebuild_plan(&Situation::Unrecognized, "generated"));
        let two = refused(rebuild_plan(
            &Situation::Conflict(vec![
                Manager::Pidfile { pid: 1 },
                Manager::Launchd(job(LabelKind::Canonical)),
            ]),
            "generated",
        ));
        for why in [&other_home, &brew, &holder, &two] {
            assert!(
                why.contains("nothing was changed"),
                "a refusal changes nothing and says so: {why}"
            );
        }
        for why in [&other_home, &brew, &holder] {
            assert!(why.contains("supragnosis embed reset"), "a refusal names the way on: {why}");
        }
    }

    /// SR8 (semantic-retrieval.md Section 13): a store the daemon cannot open for its model says so,
    /// with both models and both ways on - in the refusal at open, in `status`'s line, and in
    /// `status --json`'s field - and a store read for it is read without being held or written.
    #[test]
    fn a_refused_store_names_both_ways_on() {
        let dir = std::env::temp_dir().join(format!(
            "supragnosis-refused-{}-{}",
            std::process::id(),
            unix_secs()
        ));
        let path = dir.join("knowledge.redb");
        {
            let store = RedbStore::open(&path).expect("open");
            store.set_embedder("hashing-256").expect("the first model");
        }
        let other = "paraphrase-multilingual-minilm-l12-v2-q-384";
        assert_eq!(
            embedder_conflict(&path, Some(other.to_string())),
            Some(("hashing-256".to_string(), other.to_string())),
            "a store embedded by another model is named, with the daemon's"
        );
        assert_eq!(
            embedder_conflict(&path, Some("hashing-256".into())),
            None,
            "the same model is no conflict"
        );
        assert_eq!(
            embedder_conflict(&path, None),
            None,
            "a daemon without a model opens any store"
        );
        assert_eq!(
            embedder_conflict(&dir.join("absent.redb"), Some(other.into())),
            None,
            "no store, no conflict"
        );
        assert_eq!(
            supragnosis_store::recorded_embedder(&path).expect("read").as_deref(),
            Some("hashing-256"),
            "reading the conflict wrote nothing"
        );

        for text in [
            embedder_refusal("hashing-256", other),
            embedder_conflict_line("hashing-256", other),
            embedder_conflict_attention("hashing-256", other),
        ] {
            for part in [
                "'hashing-256'",
                "'paraphrase-multilingual-minilm-l12-v2-q-384'",
                "supragnosis embed rebuild",
                "Rebuild index",
            ] {
                assert!(text.contains(part), "the way on is named ({part}): {text}");
            }
        }
        for text in [
            embedder_refusal("hashing-256", other),
            embedder_conflict_line("hashing-256", other),
        ] {
            assert!(text.contains("SUPRAGNOSIS_EMBED=none"), "the way around is named: {text}");
        }
        let hashing = |k: &str| (k == "SUPRAGNOSIS_EMBED").then(|| "hashing".to_string());
        assert_eq!(embedder_id_for(&hashing).expect("id").as_deref(), Some("hashing-256"));
        let none = |k: &str| (k == "SUPRAGNOSIS_EMBED").then(|| "none".to_string());
        assert_eq!(
            embedder_id_for(&none).expect("id"),
            None,
            "a daemon without a model records none"
        );

        let situation = lifecycle::Situation::Stopped;
        let doc = status_document(&StatusFacts {
            situation: &situation,
            answering: false,
            store_held: false,
            http: "127.0.0.1:7373".into(),
            here: "1.0.0",
            running: None,
            health: None,
            server: serde_json::json!({"name": "local", "remote": false}),
            clients: None,
            embedder_conflict: Some(("hashing-256".into(), other.into())),
            plist: std::path::PathBuf::from("/x/com.supragnosis.daemon.plist"),
            plist_state: "generated",
            linger: None,
        });
        assert_eq!(doc["embedder_conflict"]["recorded"], "hashing-256");
        assert_eq!(doc["embedder_conflict"]["daemon"], other);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_json_matches_its_example() {
        use lifecycle::{Job, LabelKind, Manager, Situation};
        let situation = Situation::Conflict(vec![
            Manager::Pidfile { pid: 1 },
            Manager::Launchd(Job {
                label: lifecycle::CANONICAL_LABEL,
                kind: LabelKind::Canonical,
                pid: Some(2),
                last_exit: Some(0),
            }),
        ]);
        let doc = status_document(&StatusFacts {
            situation: &situation,
            answering: true,
            store_held: true,
            http: "127.0.0.1:7373".into(),
            here: "1.0.0",
            running: Some("0.9.0".into()),
            health: Some(serde_json::json!({
                "owed_projections": 0, "last_recovery": null,
                "semantic": {"model": "m-384", "dims": 384, "state": "on",
                    "backfill": {"state": "done", "found": 3, "embedded": 3}}
            })),
            server: serde_json::json!({"name": "local", "remote": false}),
            clients: Some(serde_json::json!([
                {"name": "claude-code", "version": "2.1.0", "first": 1, "last": 2}
            ])),
            embedder_conflict: None,
            plist: std::path::PathBuf::from("/x/com.supragnosis.daemon.plist"),
            plist_state: "generated",
            linger: None,
        });
        assert_eq!(doc["schema"], JSON_SCHEMA);
        same_shape(&example("status"), &doc, "status").unwrap_or_else(|e| panic!("{e}"));

        // On Linux a unit is a manager of type systemd, named as a unit and as a label, and the
        // service object says whether the person's manager lingers (daemon-lifecycle.md Section 4,
        // "On Linux"). Additive, so schema 1 holds (compatibility.md Section 6).
        let unit = Job {
            label: lifecycle::CANONICAL_UNIT,
            kind: LabelKind::Canonical,
            pid: Some(3),
            last_exit: None,
        };
        let one = Situation::One(Manager::Systemd(unit));
        let doc = status_document(&StatusFacts {
            situation: &one,
            answering: true,
            store_held: true,
            http: "127.0.0.1:7373".into(),
            here: "1.0.0",
            running: Some("1.0.0".into()),
            health: None,
            server: serde_json::json!({"name": "local", "remote": false}),
            clients: None,
            embedder_conflict: None,
            plist: std::path::PathBuf::from("/home/you/.config/systemd/user/supragnosis.service"),
            plist_state: "generated",
            linger: Some(false),
        });
        assert_eq!(doc["schema"], JSON_SCHEMA);
        assert_eq!(
            doc["managers"][0],
            serde_json::json!({"type": "systemd", "unit": "supragnosis.service",
                "label": "supragnosis.service", "source": "canonical", "formula": null,
                "pid": 3, "last_exit": null})
        );
        assert_eq!(doc["service"]["linger"], false);
        assert_eq!(doc["service"]["path"], doc["service"]["plist"]);
    }

    /// L8: nothing the lifecycle runs asks for privilege. Every systemctl call goes to the person's own
    /// manager (`--user`), every launchctl target is their `gui/<uid>` domain, lingering is asked for
    /// this user alone (no name, so polkit decides), and nothing runs sudo, pkexec or doas. Read from
    /// the lifecycle's own code, between its two section comments.
    #[test]
    fn no_lifecycle_command_asks_for_privilege() {
        let src = include_str!("main.rs");
        let from = src.find("// The decisions are pure and live in `lifecycle.rs`").unwrap();
        let to = src.find("// --- Federation configuration").unwrap();
        let code = &src[from..to];
        let calls = |name: &str| -> Vec<&str> {
            code.match_indices(&format!("Command::new(\"{name}\")"))
                .map(|(i, _)| &code[i..(i + 160).min(code.len())])
                .collect()
        };
        for escalate in ["sudo", "pkexec", "doas"] {
            assert!(calls(escalate).is_empty(), "the lifecycle runs {escalate}");
        }
        let systemctl = calls("systemctl");
        assert!(systemctl.len() >= 3, "the lifecycle's systemctl calls moved out of reach");
        for call in systemctl {
            assert!(call.contains("\"--user\""), "a systemctl call without --user: {call}");
        }
        let loginctl = calls("loginctl");
        assert_eq!(loginctl.len(), 1, "one loginctl call, the linger request");
        assert!(loginctl[0].contains(".arg(\"enable-linger\").output()"), "{}", loginctl[0]);
        assert!(code.contains("format!(\"gui/{uid}"));
        assert!(!code.contains("\"system/") && !code.contains("format!(\"system/"));
    }

    /// `setup --events` and `embed fetch --events` (docs/onboarding.md Section 3): a schema line,
    /// then events whose fields are the example's and whose states are the six the app draws. The
    /// example is what the Welcome page's tests read.
    #[test]
    fn setup_events_match_their_example() {
        let text = include_str!("../tests/fixtures/json/setup-events.jsonl");
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("each line is JSON"))
            .collect();
        assert_eq!(lines[0], serde_json::json!({ "schema": setup::EVENTS_SCHEMA }));
        let fields: BTreeSet<&str> =
            ["step", "id", "state", "say", "cmd", "out", "quote", "app", "entity", "checks"].into();
        let states: BTreeSet<&str> =
            ["todo", "working", "done", "attention", "failed", "skipped"].into();
        let steps: BTreeSet<&str> = ["server", "apps", "app", "model", "first", "summary"].into();
        for l in &lines[1..] {
            let keys: BTreeSet<&str> = l.as_object().unwrap().keys().map(String::as_str).collect();
            assert!(keys.is_subset(&fields) && keys.contains("say"), "{l}");
            assert!(states.contains(l["state"].as_str().unwrap_or("")), "{l}");
            assert!(steps.contains(l["step"].as_str().unwrap_or("")), "{l}");
        }
        assert_eq!(lines.last().unwrap()["step"], "summary", "the last line summarizes");
        // What the CLI writes has the same fields: every one of them, when an act runs.
        let mut e = setup::Event::new("app", Some("cursor"), setup::State::Working, "Running.");
        e.cmd = Some("supragnosis connect cursor".into());
        e.out = Some("  wrote   /x".into());
        e.quote = Some("Auth is JWT.".into());
        e.app = Some("Claude Code".into());
        e.entity = Some("e1".into());
        e.checks = vec!["Check.".into()];
        let made = serde_json::to_value(&e).unwrap();
        let keys: BTreeSet<&str> = made.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, fields);
    }

    #[test]
    fn connect_json_matches_its_example() {
        let env = connect::Env {
            home: std::env::temp_dir().join("no-such-home"),
            path: Vec::new(),
            config_home: std::env::temp_dir().join("no-such-home/.config"),
        };
        let doc = connect_document(&env, "/x/supragnosis");
        assert_eq!(doc["schema"], JSON_SCHEMA);
        same_shape(&example("connect"), &doc, "connect").unwrap_or_else(|e| panic!("{e}"));
        // The example lists every app this build knows, so the app's pages, held to the example,
        // name every one (client-connect.md Section 13.4).
        let ids = |v: &serde_json::Value| -> Vec<String> {
            v["clients"].as_array().unwrap().iter().map(|c| c["id"].to_string()).collect()
        };
        assert_eq!(ids(&example("connect")), ids(&doc));
    }

    /// client-connect.md C5 and Section 4: `connect` without an app only reads. Every app the host
    /// lets the fixture install looks installed and connected in a temp HOME - each config file holds
    /// a bridge entry, and each app's CLI is a stand-in that leaves a mark beside itself if anything
    /// runs it (with a shell builtin, since a client CLI runs under the fixture's PATH alone). Claude
    /// Desktop counts as installed only where Claude.app exists, and on a Mac with Visual Studio Code
    /// the bundled `code` is found instead of the stand-in. The listing, as text and as JSON, must
    /// read the fixture - every app connected through the bridge - and leave every file, byte, mode,
    /// inode and time where it was. A HOME that does not exist is not made.
    #[test]
    fn connect_without_an_app_only_reads() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let root = std::env::temp_dir().join(format!("sg-connect-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["claude", "codex", "gemini", "opencode", "code"] {
            let p = bin.join(name);
            std::fs::write(&p, "#!/bin/sh\n: > \"$0.ran\"\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = connect::Env {
            home: root.join("home"),
            path: vec![bin],
            config_home: root.join("xdg"),
        };
        let bridge = serde_json::json!({"command": "/x/supragnosis", "args": ["bridge"]});
        for c in connect::CLIENTS {
            let (path, section) = c.config(&env);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let text = match c {
                connect::Client::Codex => format!(
                    "[{section}.supragnosis]\ncommand = \"/x/supragnosis\"\nargs = [\"bridge\"]\n"
                ),
                connect::Client::OpenCode => serde_json::json!({"mcp": {"servers": {
                    "supragnosis": {"type": "local", "command": ["/x/supragnosis", "bridge"]}
                }}})
                .to_string(),
                _ => serde_json::json!({ section: { "supragnosis": bridge } }).to_string(),
            };
            std::fs::write(&path, text).unwrap();
        }
        let settings = connect::claude_code_settings(&env);
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{\"theme\": \"dark\"}\n").unwrap();

        fn tree(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let m = std::fs::symlink_metadata(&p).unwrap();
                // FNV-1a, so a failure prints a line per file rather than every byte.
                let sum = if m.is_dir() {
                    0
                } else {
                    std::fs::read(&p).unwrap().iter().fold(0xcbf29ce484222325u64, |h, b| {
                        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
                    })
                };
                out.push(format!(
                    "{} len={} sum={sum:x} mode={:o} ino={} mtime={}.{} ctime={}.{}",
                    p.display(),
                    m.len(),
                    m.permissions().mode(),
                    m.ino(),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec()
                ));
                if m.is_dir() {
                    tree(&p, out);
                }
            }
        }
        let snapshot = || {
            let mut v = Vec::new();
            tree(&root, &mut v);
            v.sort();
            v
        };
        let before = snapshot();
        let text = connect_listing(&env, "/x/supragnosis", false);
        let doc: serde_json::Value =
            serde_json::from_str(&connect_listing(&env, "/x/supragnosis", true)).unwrap();
        assert_eq!(snapshot(), before, "the listing wrote, moved or ran nothing");

        // It read the fixture: every app but Claude Desktop is installed on any host, and every
        // installed one is connected through the bridge, as the text says too.
        for c in doc["clients"].as_array().unwrap() {
            let id = c["id"].as_str().unwrap();
            if id != "claude-desktop" {
                assert_eq!(c["installed"], true, "{id} is installed in the fixture");
            }
            if c["installed"] == true {
                assert_eq!(c["entry"], "bridge", "{id} read from the fixture");
                let line = text.lines().find(|l| l.trim_start().starts_with(id)).unwrap();
                assert!(line.contains("connected (bridge)"), "{line}");
            }
        }
        let claude = doc["clients"].as_array().unwrap().iter().find(|c| c["id"] == "claude-code");
        assert_eq!(claude.unwrap()["capture"], "off", "the capture state read from the fixture");

        let absent = connect::Env {
            home: root.join("absent"),
            path: Vec::new(),
            config_home: root.join("absent/.config"),
        };
        connect_listing(&absent, "/x/supragnosis", false);
        connect_listing(&absent, "/x/supragnosis", true);
        assert!(!absent.home.exists(), "a HOME that does not exist is not made");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// client-connect.md Section 13.3: OpenCode reads two files, so `--remove` takes the entry out
    /// of each that holds it, a backup first; and a file that does not parse stops the removal with
    /// neither file touched.
    #[test]
    fn opencode_removal_clears_both_files_or_neither() {
        let root = std::env::temp_dir().join(format!("sg-opencode-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let env = connect::Env {
            home: root.join("home"),
            path: Vec::new(),
            config_home: root.join("xdg"),
        };
        let [json, jsonc] = connect::opencode_files(&env);
        std::fs::create_dir_all(json.parent().unwrap()).unwrap();
        let entry = r#"{"type": "local", "command": ["/x/supragnosis", "bridge"]}"#;
        let a = format!("{{\"theme\": 1, \"mcp\": {{\"servers\": {{\"supragnosis\": {entry}}}}}}}");
        let b = format!(
            "{{\n  // keep me\n  \"mcp\": {{\"servers\": {{\"supragnosis\": {entry},}}}},\n}}"
        );
        std::fs::write(&json, &a).unwrap();
        std::fs::write(&jsonc, &b).unwrap();
        assert_eq!(connect::Client::OpenCode.entry(&env), connect::Entry::Bridge);

        // One of the two does not parse: nothing is written, the other keeps its entry.
        std::fs::write(&jsonc, "{\"mcp\": ").unwrap();
        assert!(connect_remove(connect::Client::OpenCode, &env).is_err());
        assert_eq!(std::fs::read_to_string(&json).unwrap(), a, "untouched");

        std::fs::write(&jsonc, &b).unwrap();
        connect_remove(connect::Client::OpenCode, &env).expect("removed");
        assert_eq!(connect::Client::OpenCode.entry(&env), connect::Entry::None);
        assert_eq!(
            std::fs::read_to_string(&json).unwrap(),
            "{\"theme\": 1, \"mcp\": {\"servers\": {}}}"
        );
        assert!(std::fs::read_to_string(&jsonc).unwrap().contains("// keep me"));
        let backups: Vec<String> = std::fs::read_dir(env.home.join(".supragnosis/connect"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(backups.len(), 2, "{backups:?}");
        assert!(
            backups.iter().any(|f| f.ends_with(".jsonc"))
                && backups.iter().any(|f| f.ends_with(".json"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The shape check itself: a renamed field and a changed kind of value both fail it.
    #[test]
    fn a_renamed_or_retyped_field_fails_the_shape_check() {
        let e = serde_json::json!({"a": {"b": 1}, "c": "x"});
        assert!(same_shape(&e, &serde_json::json!({"a": {"b": 2}, "c": "y"}), "t").is_ok());
        assert!(same_shape(&e, &serde_json::json!({"a": {"bb": 1}, "c": "x"}), "t").is_err());
        assert!(same_shape(&e, &serde_json::json!({"a": {"b": "1"}, "c": "x"}), "t").is_err());
    }
}

/// docs/e2e.md Section 3.1: the listening file names each listener with the address it got, and is
/// replaced whole.
#[cfg(test)]
mod listening_file {
    #[test]
    fn the_listening_file_names_each_listener_and_is_replaced_whole() {
        let base = std::env::temp_dir().join(format!("sg-listening-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a: std::net::SocketAddr = "127.0.0.1:41234".parse().unwrap();
        let b: std::net::SocketAddr = "127.0.0.1:41235".parse().unwrap();
        super::write_listening(&base, &[("mcp", a), ("sync", b)]).unwrap();
        let text = std::fs::read_to_string(base.join("listening")).unwrap();
        assert_eq!(text, "mcp 127.0.0.1:41234\nsync 127.0.0.1:41235\n");
        super::write_listening(&base, &[("mcp", b)]).unwrap();
        assert_eq!(
            std::fs::read_to_string(base.join("listening")).unwrap(),
            "mcp 127.0.0.1:41235\n"
        );
        assert!(!base.join("listening.tmp").exists(), "no half-written file is left beside it");
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// main.rs and the modules split out of it, as text, for the tests that read the CLI's own code.
#[cfg(test)]
const MAIN_SOURCES: &[&str] = &[
    include_str!("main.rs"),
    include_str!("config.rs"),
    include_str!("embedding.rs"),
    include_str!("engine_setup.rs"),
    include_str!("sync_wiring.rs"),
    include_str!("store_cmds.rs"),
    include_str!("connect_cli.rs"),
    include_str!("setup_cli.rs"),
    include_str!("pairing_cli.rs"),
];
