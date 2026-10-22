//! supragnosis executable (single-binary CLI).
//!
//! Subcommands control the server. Running it **with no arguments** starts a stdio
//! MCP server - this is backward compatibility for the path where an MCP client
//! launches it as a child process.
//!
//!   supragnosis                  stdio MCP server (default, no arguments)
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
    /// Which supragnosis server this machine's AI apps use - this machine's daemon, or a remote one
    /// (docs/remote-server.md); with no subcommand, list the profiles and check the active one
    Server(ServerArgs),
    /// The people and agents this hub admits to MCP on its [server] listener (docs/remote-server.md)
    Principal {
        #[command(subcommand)]
        cmd: PrincipalCmd,
    },
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

#[derive(Args)]
struct ServerArgs {
    #[command(subcommand)]
    cmd: Option<ServerCmd>,
    /// Machine-readable listing (the desktop app reads this).
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum ServerCmd {
    /// Add a remote server profile. The credential is read from stdin, never from the command line
    Add {
        /// Profile name (letters, digits, '-', '_')
        name: String,
        /// The server's MCP URL, e.g. https://hub.example:7420/mcp
        url: String,
        /// PEM bundle of a private CA that issued the server's certificate
        #[arg(long)]
        ca: Option<String>,
    },
    /// Make a profile - or `local`, this machine's daemon - the one AI apps here use
    Use { name: String },
    /// Remove a remote profile and its credential file
    Remove { name: String },
}

#[derive(Subcommand)]
enum PrincipalCmd {
    /// Admit a person or agent to this hub's agent surface; prints its credential once
    Add {
        /// Principal name (letters, digits, '-', '_')
        name: String,
        /// Workspaces it may read, comma-separated
        #[arg(long)]
        read: Option<String>,
        /// Workspaces it may write (and read), comma-separated
        #[arg(long)]
        write: Option<String>,
    },
    /// Revoke a principal - its credential stops working at its next request
    Remove { name: String },
    /// List the principals and their grants (never their credentials)
    List,
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
    /// Let the hub serve what this machine sends to the people it admits, when it serves any
    #[arg(long)]
    serve: bool,
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

// Read by the macOS and Linux implementations; elsewhere `service` refuses before looking at them.
#[cfg_attr(not(unix), allow(dead_code))]
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
mod principal;
mod profile;
mod setup;

fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Serve(RunArgs::default())) {
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
        Cmd::Server(a) => server_cmd(a),
        Cmd::Principal { cmd } => principal_cmd(cmd),
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

/// Resolved run configuration.
#[derive(Clone)]
struct Config {
    host: String,
    workspace: String,
    store_kind: String,
    data_dir: String,
    embed_kind: String,
    session: String,
    /// Some = streamable-http daemon, None = stdio.
    http: Option<String>,
    /// Some = accompanied by the live viewer (unix socket path).
    viz: Option<String>,
    /// Whether the streamable-http daemon requires its bearer token (Principle 17). Default on.
    /// Only consulted when `http` is Some - stdio is already confined to the process that spawned it.
    mcp_auth: bool,
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
    let http = a
        .http
        .or_else(|| env("SUPRAGNOSIS_HTTP_ADDR"))
        .or_else(|| daemon.then(|| "127.0.0.1:7373".to_string()));
    // An http daemon defaults the viewer socket on, exactly like `start`: a daemon without its
    // socket strands every client of the human channel - worse, the desktop shell would try to
    // spawn a second daemon into the single-process store lock. This matters for supervisors
    // that run `serve --http` directly (brew services, systemd) rather than `start`.
    let daemonish = daemon || http.is_some();
    let store_kind = a
        .store
        .or_else(|| env("SUPRAGNOSIS_STORE"))
        .unwrap_or_else(|| "redb".to_string());
    // The two file-backed stores get separate default directories rather than sharing one. RocksDB
    // owns its directory, so dropping a redb file inside it invites the two to trip over each
    // other - and keeping them apart is what lets both exist at once, which is the whole point of
    // an opt-in migration: the Cozo store stays untouched and readable while redb is being tried.
    let data_dir = a
        .data_dir
        .or_else(|| env("SUPRAGNOSIS_DATA_DIR"))
        .unwrap_or_else(|| default_data_dir_for(&store_kind));
    Config {
        workspace: a
            .workspace
            .or_else(|| env("SUPRAGNOSIS_WORKSPACE"))
            .unwrap_or_else(|| "default".to_string()),
        store_kind,
        data_dir,
        embed_kind: a
            .embed
            .or_else(|| env("SUPRAGNOSIS_EMBED"))
            .unwrap_or_else(|| default_embed_kind().to_string()),
        session: a
            .session
            .or_else(|| env("SUPRAGNOSIS_SESSION"))
            .or_else(|| env("CLAUDE_CODE_SESSION_ID"))
            .unwrap_or_else(|| format!("{host}-{}", supragnosis_core::now_millis())),
        viz: a
            .viz
            .or_else(|| env("SUPRAGNOSIS_VIZ_SOCK"))
            .or_else(|| daemonish.then(default_viz_sock)),
        // Defence in depth is opt-OUT, for the reason `Engine::with_secret_scan` is: the cost of
        // being wrong is unbounded and one-directional. Anything other than an explicit "off" means
        // on, so a typo fails safe rather than silently opening the surface.
        mcp_auth: !a
            .mcp_auth
            .or_else(|| env("SUPRAGNOSIS_MCP_AUTH"))
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("off")),
        http,
        host,
    }
}

fn default_data_dir_for(store_kind: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    match store_kind {
        // The pre-0.2 Cozo location, kept only so the un-migrated-store guard knows where to look.
        "cozo" => format!("{home}/.supragnosis/db"),
        _ => format!("{home}/.supragnosis/redb"),
    }
}

/// The redb database file inside a store directory. One file, named rather than derived at each
/// call site, so the migration command and the server cannot disagree about where the store lives.
fn redb_path(data_dir: &str) -> std::path::PathBuf {
    std::path::Path::new(data_dir).join("knowledge.redb")
}

/// Default viewer socket path (the viewer serves HTTP over UDS only - no TCP; the socket file's
/// 0600 mode is the access control).
fn default_viz_sock() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.supragnosis/viz.sock")
}

/// Default embedder kind based on the compiled features. If built with fastembed enabled, that is the default.
fn default_embed_kind() -> &'static str {
    if cfg!(feature = "fastembed") {
        "fastembed"
    } else {
        "none"
    }
}

/// Selects the embedding provider from the SUPRAGNOSIS_EMBED value. Absence yields None (keyword
/// search); a model that is not there degrades later, loudly (semantic-retrieval.md Section 6).
/// The one refusal is a `SUPRAGNOSIS_EMBED_MODEL` that names no model (Principle 24).
fn build_embedder(kind: &str) -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    Ok(match kind {
        "none" | "" => None,
        // Deterministic but non-semantic (lexical hashing) - a development/offline stand-in.
        "hashing" => {
            tracing::info!("embed=hashing (deterministic/lexical, for development)");
            Some(Arc::new(HashingEmbedder::default()))
        }
        "fastembed" => build_fastembed()?,
        other => {
            tracing::warn!(
                kind = other,
                "unknown SUPRAGNOSIS_EMBED - proceeding with keyword search"
            );
            None
        }
    })
}

/// The model `SUPRAGNOSIS_EMBED_MODEL` names, or the default (semantic-retrieval.md Section 5).
#[cfg(feature = "fastembed")]
fn embed_model() -> Result<supragnosis_embed::Model> {
    supragnosis_embed::Model::parse(&std::env::var("SUPRAGNOSIS_EMBED_MODEL").unwrap_or_default())
        .map_err(|e| anyhow::anyhow!(e))
}

/// The lazy provider: identity now, weights on first use from the cache, never a download (SR1,
/// SR2). A missing model is said once here, at start, and again by `status` for as long as it is.
#[cfg(feature = "fastembed")]
fn build_fastembed() -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    use supragnosis_core::Readiness;
    use supragnosis_embed::{model_cache_dir, FastEmbedProvider};
    let model = embed_model()?;
    let cache = model_cache_dir();
    let provider = FastEmbedProvider::lazy(model, cache.clone());
    match provider.readiness() {
        Readiness::Ready => tracing::info!(
            model = model.name(),
            id = model.id(),
            cache = %cache.display(),
            "embed=fastembed - the model loads on first use"
        ),
        Readiness::Degraded(note) => tracing::warn!("{note}"),
    }
    Ok(Some(Arc::new(provider)))
}

#[cfg(not(feature = "fastembed"))]
fn build_fastembed() -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    tracing::warn!("this build has no embedding model (built without the fastembed feature) - proceeding with keyword search");
    Ok(None)
}

/// Why a store is refused for the model it is opened with, and the two ways on
/// (semantic-retrieval.md Section 13, SR8).
fn embedder_refusal(recorded: &str, opened: &str) -> String {
    format!(
        "the store was embedded by '{recorded}' and this process opens it with '{opened}' - vectors \
         from two models share no space, so the store is refused (docs/semantic-retrieval.md Section \
         13). Either run `supragnosis embed rebuild` - in the app, Settings > This Mac > Rebuild \
         index - which stops the daemon, removes the old index and starts it again with '{opened}'; \
         or run without semantic search and leave the store as it is: SUPRAGNOSIS_EMBED=none, for \
         the login job `supragnosis service install --env SUPRAGNOSIS_EMBED=none`"
    )
}

/// The environment the login job runs the daemon with, when one is installed: launchd and systemd
/// start it with that and none of this shell's (semantic-retrieval.md Section 13). `None` when no
/// job is installed, or its environment cannot be read.
#[cfg(unix)]
fn job_env() -> Option<std::collections::BTreeMap<String, String>> {
    if canonical_service_state() == "absent" {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        plist_env(&canonical_service_path()).ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let text = std::fs::read_to_string(canonical_service_path()).ok()?;
        let home = std::env::var("HOME").unwrap_or_default();
        lifecycle::parse_unit_env(&text, &home, &current_user().unwrap_or_default()).ok()
    }
}
#[cfg(not(unix))]
fn job_env() -> Option<std::collections::BTreeMap<String, String>> {
    None
}

/// A variable of the daemon's environment: the login job's when one is installed, this process's
/// otherwise; an empty value is no value, as `serve` reads it.
fn daemon_var(
    job: &Option<std::collections::BTreeMap<String, String>>,
    key: &str,
) -> Option<String> {
    match job {
        Some(vars) => vars.get(key).cloned(),
        None => std::env::var(key).ok(),
    }
    .filter(|v| !v.trim().is_empty())
}

/// The identity of the model a daemon with this environment opens its store with - what the store
/// records - without loading it; `None` when it runs without one.
fn embedder_id_for(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<String>> {
    let kind = get("SUPRAGNOSIS_EMBED").unwrap_or_else(|| default_embed_kind().to_string());
    Ok(match kind.as_str() {
        "hashing" => Some(HashingEmbedder::default().id()),
        "fastembed" => fastembed_id(&get("SUPRAGNOSIS_EMBED_MODEL").unwrap_or_default())?,
        _ => None,
    })
}
#[cfg(feature = "fastembed")]
fn fastembed_id(model: &str) -> Result<Option<String>> {
    let model = supragnosis_embed::Model::parse(model).map_err(|e| anyhow::anyhow!(e))?;
    Ok(Some(model.id().to_string()))
}
#[cfg(not(feature = "fastembed"))]
fn fastembed_id(_model: &str) -> Result<Option<String>> {
    Ok(None)
}

/// The model the store at `store` recorded and the one the daemon would open it with, when they
/// differ and nothing holds the store: why a daemon cannot start, which `status` names (SR8).
fn embedder_conflict(store: &std::path::Path, daemon: Option<String>) -> Option<(String, String)> {
    if supragnosis_store::redb_in_use(store) {
        return None;
    }
    let recorded = supragnosis_store::recorded_embedder(store).ok().flatten()?;
    let daemon = daemon?;
    (recorded != daemon).then_some((recorded, daemon))
}

/// `status`'s line for a store the daemon cannot open (SR8).
fn embedder_conflict_line(recorded: &str, daemon: &str) -> String {
    format!(
        "  store   the daemon cannot open it: it was embedded by '{recorded}' and the daemon runs \
         '{daemon}'. `supragnosis embed rebuild` - in the app, Settings > This Mac > Rebuild index - \
         rebuilds it with '{daemon}'; or `supragnosis service install --env SUPRAGNOSIS_EMBED=none` \
         runs without semantic search"
    )
}

/// `supragnosis embed reset` (semantic-retrieval.md Section 13, SR9): on a store no process holds,
/// removes every stored vector and the model the store recorded, so the next start records the
/// model the daemon runs and its backfill embeds every row with it. The store is the daemon's: the
/// one its login job's environment names, when one is installed.
fn embed_reset() -> Result<()> {
    let job = job_env();
    let get = |k: &str| daemon_var(&job, k);
    let Some(path) = daemon_store_path(get) else {
        println!("The daemon's store is in memory, so there is nothing to reset.");
        return Ok(());
    };
    if !path.exists() {
        println!("There is no store at {} - nothing to reset.", path.display());
        return Ok(());
    }
    if supragnosis_store::redb_in_use(&path) {
        anyhow::bail!(
            "the store at {} is open in another process - the daemon, or an AI app's stdio server - \
             so nothing was changed. Stop it (`supragnosis stop`) and run this again, or run \
             `supragnosis embed rebuild`, which stops and starts a daemon this CLI manages",
            path.display()
        );
    }
    let store = RedbStore::open(&path)
        .with_context(|| format!("failed to open redb store at {}", path.display()))?;
    let r = store.reset_embeddings()?;
    drop(store);
    match &r.model {
        None if r.observations + r.entities == 0 => {
            println!("The store holds no vectors and records no model - nothing to reset.")
        }
        Some(model) if r.observations + r.entities == 0 => println!(
            "The store held no vectors; removed its record of '{model}'. The log and the graph are \
             as they were."
        ),
        model => println!(
            "Removed {} vectors ({} for observations, {} for entities) made by {}, and the store's \
             record of that model. The log and the graph are as they were.",
            r.observations + r.entities,
            r.observations,
            r.entities,
            model.as_deref().map_or("no recorded model".to_string(), |m| format!("'{m}'"))
        ),
    }
    match embedder_id_for(&get)? {
        Some(id) => println!(
            "The next start records '{id}' and embeds every observation and entity with it; \
             `supragnosis status` shows it while it runs."
        ),
        None => println!(
            "The daemon runs without a model (SUPRAGNOSIS_EMBED={}), so the store stays without \
             vectors and search matches words. `supragnosis service install --env \
             SUPRAGNOSIS_EMBED=fastembed` gives the login job the model.",
            get("SUPRAGNOSIS_EMBED").unwrap_or_else(|| default_embed_kind().to_string())
        ),
    }
    Ok(())
}

/// What `supragnosis embed rebuild` does for the daemon it finds (semantic-retrieval.md Section 13,
/// SR10). It stops only a daemon this CLI manages for this home - a `supragnosis start` daemon,
/// whose pidfile is this home's, or the login job whose file is in this home - and refuses every
/// other holder with nothing changed: a login job is per user, not per home, so a job loaded from
/// another home's file is not this store's to stop.
#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
enum RebuildPlan {
    /// Nothing runs: reset the store, and start the login job when this home installs one.
    ResetOnly { start_job: bool },
    /// Stop the daemon, reset the store, start the daemon again.
    StopResetStart,
    /// Refuse, changing nothing, with the reason and the way on.
    Refuse(String),
}

#[cfg(unix)]
fn rebuild_plan(s: &lifecycle::Situation, service_state: &str) -> RebuildPlan {
    use lifecycle::{LabelKind, Manager, Situation};
    let installed = service_state != "absent";
    match s {
        Situation::Stopped => RebuildPlan::ResetOnly { start_job: installed },
        Situation::One(Manager::Pidfile { .. }) => RebuildPlan::StopResetStart,
        Situation::One(Manager::Launchd(j) | Manager::Systemd(j))
            if j.kind == LabelKind::Canonical && installed =>
        {
            RebuildPlan::StopResetStart
        }
        Situation::One(m) => RebuildPlan::Refuse(format!(
            "the daemon runs under {}, which this command does not stop and start for this home - \
             nothing was changed. Stop it there, run `supragnosis embed reset`, and start it again",
            m.describe()
        )),
        Situation::Conflict(ms) => RebuildPlan::Refuse(format!(
            "{} - nothing was changed",
            lifecycle::conflict_message(ms)
        )),
        Situation::Unrecognized => RebuildPlan::Refuse(
            "the store is held by a process no manager runs - an AI app's stdio server, or a \
             `supragnosis serve` in a terminal - so nothing was changed. Stop it, then run \
             `supragnosis embed reset`"
                .to_string(),
        ),
    }
}

/// `supragnosis embed rebuild` (semantic-retrieval.md Section 13, SR10): the search index made again
/// from the log by the model the daemon runs. It stops the daemon this CLI manages, resets the store
/// (SR9) and starts the daemon again, whose backfill embeds every row; with no daemon running it
/// resets the store and starts the login job when one is installed.
#[cfg(unix)]
fn embed_rebuild() -> Result<()> {
    clear_stale_pidfile();
    let plan = rebuild_plan(&lifecycle::classify(&observe()), canonical_service_state());
    let start = match plan {
        RebuildPlan::Refuse(why) => anyhow::bail!(why),
        RebuildPlan::ResetOnly { start_job } => start_job,
        RebuildPlan::StopResetStart => {
            stop()?;
            // The stopped process lets go of the store as it exits.
            let job = job_env();
            if let Some(path) = daemon_store_path(|k| daemon_var(&job, k)) {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                while supragnosis_store::redb_in_use(&path) {
                    if std::time::Instant::now() > deadline {
                        anyhow::bail!(
                            "the daemon stopped but the store at {} is still held after 15 seconds - \
                             `supragnosis status` says what holds it",
                            path.display()
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            true
        }
    };
    embed_reset()?;
    if start {
        restart(resolve(RunArgs::default(), true))?;
    } else {
        println!("No daemon runs here; the next `supragnosis start` embeds the store.");
    }
    Ok(())
}
#[cfg(not(unix))]
fn embed_rebuild() -> Result<()> {
    anyhow::bail!(
        "`embed rebuild` stops and starts the daemon, which this platform's build does not manage - \
         stop it, run `supragnosis embed reset`, and start it again"
    )
}

/// The model the daemon's store recorded and the one it runs, when the daemon does not answer
/// because they differ (SR8); what `status` and setup's server step say.
fn daemon_embedder_conflict() -> Option<(String, String)> {
    let job = job_env();
    let get = |k: &str| daemon_var(&job, k);
    daemon_store_path(get).and_then(|p| embedder_conflict(&p, embedder_id_for(&get).ok().flatten()))
}

/// Setup's server step for a daemon that cannot open its store (onboarding.md Section 4.1).
fn embedder_conflict_attention(recorded: &str, daemon: &str) -> String {
    format!(
        "The daemon cannot start: its store was indexed by '{recorded}' and it runs '{daemon}'. \
         Rebuild index in Settings > This Mac, or `supragnosis embed rebuild`, rebuilds the index \
         with '{daemon}'."
    )
}

/// `supragnosis embed fetch` (semantic-retrieval.md Section 6): the one download, by the person's
/// or the installer's command. Idempotent, and says what it found or fetched. With `events`, the
/// progress bar gives way to one JSON line about twice a second with the bytes on disk, and the last
/// line says whether the model is complete (docs/onboarding.md Section 4.3).
#[cfg(feature = "fastembed")]
fn embed_fetch(progress: bool, events: bool) -> Result<()> {
    use supragnosis_embed::{model_cache_dir, FastEmbedProvider};
    let model = embed_model()?;
    let cache = model_cache_dir();
    if events {
        return embed_fetch_events(model, &cache);
    }
    let had = model.present(&cache);
    if !had {
        println!(
            "fetching the embedding model {} ({} MB, once) into {}",
            model.name(),
            model.download_mb(),
            cache.display()
        );
    }
    let bytes = FastEmbedProvider::fetch(model, &cache, progress && !had)
        .map_err(|e| anyhow::anyhow!("fetching the embedding model {}: {e}", model.name()))?;
    println!(
        "{} the embedding model {} ({} MB on disk) in {}",
        if had { "found" } else { "fetched" },
        model.name(),
        bytes >> 20,
        cache.display()
    );
    Ok(())
}

/// `embed fetch --events`: the fetch on a thread, and the model's directory measured beside it. A
/// download in progress is `working`; only a complete model is `done` (O5).
#[cfg(feature = "fastembed")]
fn embed_fetch_events(model: supragnosis_embed::Model, cache: &std::path::Path) -> Result<()> {
    use setup::{Event, State};
    use supragnosis_embed::FastEmbedProvider;
    let emit = |e: &Event| println!("{}", serde_json::to_string(e).unwrap_or_default());
    println!("{}", serde_json::json!({ "schema": setup::EVENTS_SCHEMA }));
    let mb = model.download_mb();
    if model.present(cache) {
        emit(&Event::new(
            "model",
            None,
            State::Done,
            "Search by meaning is on: the model is here.",
        ));
        return Ok(());
    }
    emit(&Event::new(
        "model",
        None,
        State::Working,
        format!(
            "Downloading the search model {} ({mb} MB, once) into {}.",
            model.name(),
            cache.display()
        ),
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    let to = cache.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(FastEmbedProvider::fetch(model, &to, false));
    });
    let result = loop {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(r) => break r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => emit(&Event::new(
                "model",
                None,
                State::Working,
                setup::progress_say(model.bytes_on_disk(cache), mb),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break Err(supragnosis_core::EmbedError::Provider("the download stopped".into()))
            }
        }
    };
    match result {
        Ok(_) if model.present(cache) => {
            emit(&Event::new(
                "model",
                None,
                State::Done,
                "Search by meaning is on: the model is here.",
            ));
            Ok(())
        }
        Ok(_) => {
            let say = format!("The download ended, but the model in {} is not complete. Run setup again to fetch what is missing.", cache.display());
            emit(&Event::new("model", None, State::Failed, say.clone()));
            anyhow::bail!(say)
        }
        Err(e) => {
            let say = format!("The download failed ({e}). Search matches words until it is here; run setup again when online.");
            emit(&Event::new("model", None, State::Failed, say.clone()));
            anyhow::bail!(say)
        }
    }
}

#[cfg(not(feature = "fastembed"))]
fn embed_fetch(_progress: bool, _events: bool) -> Result<()> {
    anyhow::bail!(
        "this build has no embedding model (built without the fastembed feature); retrieval is keyword only"
    )
}

/// The last release that could read a Cozo (RocksDB) store was v0.1.21. This build cannot, so an
/// un-migrated store is knowledge this binary can neither open nor explain.
///
/// The failure mode that matters is not the error - it is the absence of one. A daemon that came up
/// on an empty redb store beside a full Cozo directory would answer every read with "nothing here",
/// which is exactly what a node with no knowledge looks like (Principle 5: absence must be
/// distinguishable from unavailable, and here the honest answer is unavailable). Worse, it would then
/// start accumulating a second, divergent log.
///
/// So this is a refusal with an instruction, and it is also what carries Principle 3's demand that
/// every encoding the log has ever used stays readable, for the encodings before redb: they are
/// still reachable, through the release that wrote them, and skipping that step is made impossible
/// rather than merely discouraged. Within redb the store records its format and a golden store per
/// format is read back by every build (docs/compatibility.md).
///
/// Detection is `CURRENT`, the file RocksDB always writes and redb never does. A bare directory is
/// not enough - an empty `~/.supragnosis/db` left behind by a completed migration must not block a
/// clean start forever.
///
/// Two locations, because the Cozo adapter did not put the marker where the first version of this
/// function looked for it: it handed RocksDB a `data` subdirectory of the configured dir and kept
/// `manifest` beside it, so a real store has `data/CURRENT`. Checking only the top level made the
/// guard match nothing on any store it was written to catch. The flat form stays accepted, since a
/// store that was moved or unpacked by hand is still one.
fn legacy_cozo_store_at(data_dir: &str) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(data_dir);
    for marker in [dir.join("data").join("CURRENT"), dir.join("CURRENT")] {
        if marker.is_file() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Refuses to start when an un-migrated Cozo store is present and the redb store does not yet exist.
///
/// Both conditions are required. Once the redb store exists the migration has run, and the Cozo
/// directory is a rollback artifact the operator may keep as long as they like - blocking on it then
/// would punish exactly the cautious path.
///
/// The skip is keyed on the in-memory store and nothing else, because that is the only value
/// [`build_engine`] treats as not-redb: everything it does not recognise opens redb. Keyed the other
/// way - skip unless the kind is exactly `redb` - a stale `SUPRAGNOSIS_STORE=cozo` walked past this
/// guard and then opened redb anyway, inside the legacy directory, which is precisely the empty
/// store beside a full one that this function exists to prevent. A configuration naming a store this
/// build dropped has to reach the refusal, not slip under it.
fn refuse_unmigrated_store(cfg: &Config) -> Result<()> {
    if matches!(cfg.store_kind.as_str(), "mem" | "memory") {
        return Ok(());
    }
    if redb_path(&cfg.data_dir).exists() {
        return Ok(());
    }
    // The configured dir first. A node that was ever told where to keep its data - `SUPRAGNOSIS_DATA_DIR`
    // in a systemd unit is the ordinary case - has its store there and not under the default, so a
    // guard that consulted only the default was blind on exactly the deployments that had been
    // administered. Checking the configured dir is also what makes the redb-exists skip above
    // symmetric: that test already reads `cfg.data_dir`.
    let default_legacy = default_data_dir_for("cozo");
    let Some(found) =
        legacy_cozo_store_at(&cfg.data_dir).or_else(|| legacy_cozo_store_at(&default_legacy))
    else {
        return Ok(());
    };
    anyhow::bail!(
        "found a Cozo store at {} and no redb store at {}.\n\n\
         This build reads only redb. The knowledge is not lost - it is in a format the previous \
         release can still read - but this binary cannot open it, and starting empty beside it \
         would silently begin a second log.\n\n\
         Migrate with v0.1.21, which reads both:\n  \
         curl -fsSL https://supragnosis.dev/install.sh | sh -s -- --version v0.1.21\n  \
         supragnosis migrate-store\n\n\
         Then upgrade again. The Cozo store is opened read-only and left untouched, so the step is \
         reversible. Full procedure: docs/store-migration.md",
        found.display(),
        redb_path(&cfg.data_dir).display(),
    )
}

/// A key not in the one spelling this software writes could never verify an event
/// (sync-correctness.md Section 7), so the entry carrying it is IGNORED for the run, and the file is
/// left alone for the operator to correct. Ignoring an allowlist entry drops a live credential and
/// ignoring an origin key drops what that origin's events would land as: both share less (P24).
fn noncanonical_key_notes(fc: &fed::FileConfig) -> Vec<String> {
    let bad = |k: &str| !supragnosis_core::is_canonical_public_key_hex(k);
    let mut notes = Vec::new();
    for e in fc.server.iter().flat_map(|s| &s.allowlist) {
        if bad(&e.public_key_hex) {
            notes.push(format!(
                "[[server.allowlist]] {}: public_key_hex is not 64 lowercase hex digits, so no event \
                 it signs could verify. The entry is IGNORED for this run. Copy the key exactly as \
                 `supragnosis identity` prints it on that node.",
                e.node_id
            ));
        }
    }
    for (id, k) in &fc.sync.origin_keys {
        if bad(k) {
            notes.push(format!(
                "[sync.origin_keys] {id}: the key is not 64 lowercase hex digits, so no event it \
                 signs could verify. It is IGNORED for this run. Copy the key exactly as \
                 `supragnosis identity` prints it on that node."
            ));
        }
    }
    notes
}

/// The origin keys this node verifies pulled events against: `[sync.origin_keys]` without the
/// entries [`noncanonical_key_notes`] names.
fn trusted_origin_keys(sync: &fed::SyncSection) -> std::collections::BTreeMap<String, String> {
    let mut keys = sync.origin_keys.clone();
    keys.retain(|_, k| supragnosis_core::is_canonical_public_key_hex(k));
    keys
}

/// F14's other half: a node whose own id sits in its own allowlist admits itself as a peer.
///
/// It would answer its own pulls, and once routing consults a host's answer it would negotiate with
/// itself. Nothing consumed such an entry before, so nothing ever complained - the invariant claimed
/// a refusal the code did not perform. An entry naming this node is either a copied template or a
/// mistyped id.
///
/// **Dropped rather than refused, and the file is left alone.** Refusing was the first version and it
/// broke an upgrade over a mistake that costs nothing to work around: an allowlist entry is a live
/// credential (admission authenticates by bearer hash, not by node id), so removing this one from the
/// running directory can only share LESS - the one direction 6a says a mistake may move on its own.
/// The file keeps the entry, so the operator's intent stays visible and a mistyped id is still theirs
/// to correct; the note says what was ignored and why.
fn drop_self_admission(node_id: &str, server: Option<&fed::ServerSection>) -> Vec<String> {
    let Some(srv) = server else {
        return Vec::new();
    };
    if srv.allowlist.iter().any(|e| e.node_id == node_id) {
        return vec![format!(
            "[server] allowlist admits this node's own id {node_id} - a node cannot be its own peer. \
             The entry is IGNORED for this run (it is a live credential, so ignoring it only shares \
             less). It is still in supragnosis.toml: remove it, or correct the id if it was mistyped."
        )];
    }
    Vec::new()
}

/// Assembles the store/embedder/engine from the configuration. If `events` is present, attaches a UI event sink (the viewer).
fn build_engine(
    cfg: &Config,
    events: Option<&tokio::sync::broadcast::Sender<String>>,
) -> Result<Arc<Engine>> {
    refuse_unmigrated_store(cfg)?;
    // Every writer passes here, so this is where the state directory is closed if it was found open.
    private_dir(&fed::fed_base_dir())?;
    let embedder = build_embedder(&cfg.embed_kind)?;
    let embed_dim = embedder.as_ref().map(|e| e.dimensions());
    let store: Arc<dyn KnowledgeStore> = match cfg.store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (non-persistent)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            let path = redb_path(&cfg.data_dir);
            let store = RedbStore::open(&path)
                .with_context(|| format!("failed to open redb store at {}", path.display()))?;
            // Two models share no vector space, so a swapped embedder is refused at open rather than
            // discovered later as quietly worse recall.
            if let Some(e) = &embedder {
                // Refused with the way on named (SR8): the store's own refusal cannot name commands.
                if let Some(recorded) = store.embedder()?.filter(|r| *r != e.id()) {
                    anyhow::bail!(embedder_refusal(&recorded, &e.id()));
                }
                store.set_embedder(&e.id())?;
            }
            tracing::info!(path = %path.display(), ?embed_dim, "store=redb (persistent)");
            Arc::new(store)
        }
    };
    // The ingest secret scan is on unless the operator turns it off (P17 defence in depth). Opt-out
    // rather than opt-in, because a miss cannot be undone: the log is append-only and it replicates.
    let scan = !matches!(
        std::env::var("SUPRAGNOSIS_SCAN_SECRETS").ok().as_deref(),
        Some("0") | Some("off") | Some("false")
    );
    if !scan {
        tracing::warn!(
            "SUPRAGNOSIS_SCAN_SECRETS=off - credential-shaped text will not be refused at ingest"
        );
    }
    let mut engine = Engine::new(store, cfg.host.clone(), cfg.workspace.clone())
        .with_session(cfg.session.clone())
        .with_secret_scan(scan);
    if let Some(e) = embedder {
        engine = engine.with_embedder(e);
    }
    if let Some(tx) = events {
        engine = engine.with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone())));
    }
    // crash-recovery.md K3: every caller of this function writes, so it repays the owed-projection
    // ledger here - before the caller binds a socket or accepts a request. A failure refuses to
    // start rather than serve a graph known to be behind its log (Principle 24).
    let recovery = engine.repay_owed().context(
        "repaying owed projections failed - the log holds writes the graph does not show, and \
         serving would answer \"not found\" for them",
    )?;
    if let Some(r) = recovery {
        tracing::warn!(
            workspaces = ?r.workspaces,
            observations = r.observations,
            "log rows were owed a projection - an interrupted write, or the first open by a build \
             that keeps the ledger - so their workspaces were re-projected before serving \
             (crash-recovery.md); `supragnosis status` reports it"
        );
    }
    Ok(Arc::new(engine))
}

/// Actual server run (async). With http, a streamable-http daemon; without it,
/// stdio. With viz, the live viewer is started alongside it in the same process.
async fn run(cfg: Config) -> Result<()> {
    // Federation config first: a hub serves the viewer's read tier to its principals
    // (docs/remote-viewer.md), and that tier's event stream needs the channel the engine emits into.
    let fedcfg = fed::load()?;
    // The event channel the engine emits into, for the local viewer and a hub's read tier. Made
    // whether or not this node is a hub yet: an invitation can make it one while it runs
    // (docs/pairing.md Section 4.1), and a channel no one subscribes to costs a failed send.
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
    let (sync_ctx, narrow, hub_slot) = build_sync_context(
        &engine,
        fedcfg,
        fed_status.clone(),
        events.clone(),
        listener_state.clone(),
    )
    .await?;
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

/// What `build_sync_context` wires: the sync tools' context, the viewer's narrowing act, and the
/// hub's wiring - built now when the node has a `[server]` section, and by its first invitation
/// when it does not.
type SyncWiring = (
    Option<Arc<supragnosis_mcp::SyncContext>>,
    Option<supragnosis_viz::NarrowShare>,
    Arc<hub::HubSlot>,
);

/// Builds the MCP sync context from supragnosis.toml, and the hub's listener when a `[server]`
/// section is present. No sync context on a standalone node.
async fn build_sync_context(
    engine: &Arc<Engine>,
    fedcfg: Option<fed::FileConfig>,
    fed_status: Option<supragnosis_viz::FedStatus>,
    events: Option<tokio::sync::broadcast::Sender<String>>,
    listener_state: Arc<std::sync::RwLock<serde_json::Value>>,
) -> Result<SyncWiring> {
    // Runtime peer observability (hub role): who actually checked in, when, how much.
    let peer_registry = Arc::new(supragnosis_sync::http::PeerRegistry::default());
    let Some(fc) = fedcfg else {
        let slot = hub_slot(engine.clone(), None, events, peer_registry, listener_state);
        return Ok((None, None, slot));
    };
    // One identity + one SyncNode per process - the server role and the sync tools share the HLC
    // clock and the per-workspace seq counters (two live counters over one store would collide).
    let identity = fed::load_or_create_identity()?;
    let node = Arc::new(
        supragnosis_sync::SyncNode::new(identity)
            .with_seq_mark(fed::fed_base_dir().join("node.seq")),
    );
    tracing::info!(node_id = %node.node_id(), "federation identity loaded");
    let (links, mut config_notes) = fc.sync.links();
    config_notes.extend(drop_self_admission(node.node_id(), fc.server.as_ref()));
    config_notes.extend(noncanonical_key_notes(&fc));
    let (serve_workspaces, serve_notes) = fc.sync.serve_set();
    config_notes.extend(serve_notes);
    // What a join recorded this node receives (docs/pairing.md Section 4.5, PA12).
    let (receive, receive_note) = join::receive_list(&join::Paths::default_paths());
    config_notes.extend(receive_note);
    for n in &config_notes {
        tracing::error!("federation configuration: {n}");
    }
    // One handle, written by the health loop and read by the tools. Created here because this is
    // where both sides are wired, and typed in the sync crate so neither adapter has to depend on
    // the other to see it (negotiated-surface.md Section 2).
    let surfaces: supragnosis_sync::NegotiatedSurfaces = Default::default();
    // Set on a hub: the live admission directory, published so the console can show who is in.
    let mut admitted: Option<Arc<supragnosis_sync::http::PeerDirectory>> = None;
    let slot = hub_slot(
        engine.clone(),
        Some(node.clone()),
        events,
        peer_registry.clone(),
        listener_state.clone(),
    );
    if let Some(srv) = &fc.server {
        // The listener is up only while the bind rule allows it, and a listener that cannot open
        // leaves the node serving and says why (docs/pairing.md Section 4.1, Principle 24).
        let hub = slot.hub().await?;
        hub.apply(srv).await;
        admitted = Some(hub.peers.clone());
    }
    let mut origin_keys = trusted_origin_keys(&fc.sync);
    origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
    // Keys a hub vouched for at a join, each good up to its bound (docs/pairing.md Section 4.3.1).
    let (vouched, vouched_note) =
        join::vouched_keys(&join::Paths::default_paths(), &mut origin_keys);
    config_notes.extend(vouched_note);
    // Federation status task: health-checks the configured hubs (connectivity + auth +
    // authorization in one round trip), computes per-workspace diffs vs each hub, snapshots the
    // known-peer registry (hub role), and publishes it all to the viewer's /api/federation.
    if let Some(fs) = fed_status.clone() {
        spawn_fed_status(FedStatusTask {
            engine: engine.clone(),
            fed: fs,
            node_id: node.node_id().to_string(),
            is_hub: fc.server.is_some(),
            sync: fc.sync.clone(),
            links: links.clone(),
            surfaces: surfaces.clone(),
            registry: peer_registry.clone(),
            admitted: admitted.clone(),
        });
    }
    // The console's narrowing act, built here because this is where the config and the live
    // admission directory both are - the viewer only routes it (P20: the viz crate knows nothing
    // about the sync crate or the config file).
    //
    // The file is written first and then re-read to refresh the directory, so the running state is
    // whatever the file says rather than a second copy maintained in parallel. One source of truth
    // survives the round trip, and a hand-edit made in the meantime is picked up rather than lost.
    let narrow: Option<supragnosis_viz::NarrowShare> = admitted.clone().map(|dir| {
        let status = fed_status.clone();
        let handler = move |node_id: &str, keep: &[String]| -> Result<Vec<String>, String> {
            let now = fed::narrow_shared_workspaces(node_id, keep).map_err(|e| e.to_string())?;
            let reloaded = fed::load()
                .map_err(|e| format!("narrowed on disk, but re-reading the config failed: {e}"))?
                .and_then(|c| c.server)
                .ok_or_else(|| {
                    "narrowed on disk, but the config no longer has a [server] section".to_string()
                })?;
            dir.replace(reloaded.allowlist);
            // Publish immediately rather than waiting for the status task's next pass. That task
            // recomputes `admitted` from this same directory, so the two agree either way - but it
            // runs on a slow interval, and a console that showed the old grant for up to a minute
            // after a narrowing would be displaying the wrong sharing boundary, which is the one
            // thing this surface exists to get right (P17).
            if let Some(fs) = &status {
                if let Ok(mut blob) = fs.write() {
                    if let Some(obj) = blob.as_object_mut() {
                        obj.insert(
                            "admitted".into(),
                            serde_json::to_value(admitted_json(&dir)).unwrap_or_default(),
                        );
                    }
                }
            }
            tracing::info!(peer = node_id, granted = ?now, "narrowed a peer's shared workspaces");
            Ok(now)
        };
        Arc::new(handler) as supragnosis_viz::NarrowShare
    });

    Ok((
        Some(Arc::new(supragnosis_mcp::SyncContext {
            node,
            share_workspaces: fc.sync.share_workspaces.clone(),
            receive,
            vouched,
            serve_workspaces,
            servers: links,
            config_notes,
            surfaces: surfaces.clone(),
            origin_keys,
            // Only a hub (server role) observes peers; a client-only node reports none.
            peer_registry: fc.server.is_some().then_some(peer_registry),
            round: Default::default(),
            last_round: Default::default(),
            listener: listener_state.clone(),
        })),
        narrow,
        slot,
    ))
}

/// The hub's wiring, made when it is first needed: at start for a node with a `[server]` section,
/// or by its first invitation (docs/pairing.md Section 4.1). `node` is the sync tools' own, when the
/// node has one - one identity and one SyncNode per process, since the two share the HLC clock and
/// the seq counters; without one, the node key is loaded, or made once, here.
fn hub_slot(
    engine: Arc<Engine>,
    node: Option<Arc<supragnosis_sync::SyncNode>>,
    events: Option<tokio::sync::broadcast::Sender<String>>,
    peer_registry: Arc<supragnosis_sync::http::PeerRegistry>,
    listener_state: Arc<std::sync::RwLock<serde_json::Value>>,
) -> Arc<hub::HubSlot> {
    Arc::new(hub::HubSlot::new(move || {
        let node = match &node {
            Some(node) => node.clone(),
            None => {
                let identity = fed::load_or_create_identity()?;
                Arc::new(
                    supragnosis_sync::SyncNode::new(identity)
                        .with_seq_mark(fed::fed_base_dir().join("node.seq")),
                )
            }
        };
        // Post-apply hook: re-materialize the workspace after inbound pushes (Prop C).
        let hook_engine = engine.clone();
        let on_applied: supragnosis_sync::http::OnApplied = Arc::new(move |ws: &str| {
            match hook_engine.reproject(Some(ws)) {
                Ok(r) => tracing::info!(
                    workspace = ws,
                    entities = r.entities,
                    relations = r.relations,
                    "re-materialized after inbound sync"
                ),
                Err(e) => {
                    tracing::error!(workspace = ws, error = %e, "re-materialization after inbound sync failed")
                }
            }
        });
        // Live observability: stream sync hits into the viewer's event feed (SSE activity log).
        let act_engine = engine.clone();
        let on_activity: supragnosis_sync::http::OnActivity =
            Arc::new(move |a: supragnosis_sync::http::SyncActivity| {
                act_engine.emit(Event::Sync {
                    direction: a.direction.to_string(),
                    peer: a.peer,
                    workspace: a.workspace,
                    count: a.count,
                });
            });
        // Federated retrieval: remote peers search THIS node's full retrieval surface (hybrid when
        // the embedder is present), not just the store's keyword path.
        let search_engine = engine.clone();
        let on_search: supragnosis_sync::http::OnSearch = Arc::new(move |ws, q, lim| {
            search_engine
                .search(q, Some(ws), lim)
                .map(|o| {
                    let mode = match o.mode {
                        SearchMode::Hybrid => "hybrid",
                        SearchMode::Keyword => "keyword",
                    };
                    (o.hits, mode.to_string())
                })
                .map_err(|e| e.to_string())
        });
        // The agent surface (docs/remote-server.md Section 4): MCP for principals on this
        // listener. Mounted whenever there is a listener - principals added later are admitted
        // without a restart - and counted toward the bind rule when `apply` opens it. Knowledge
        // another node originated is served only with its consent (R5), which nodes give on their
        // sync rounds and the book keeps.
        let book =
            Arc::new(principal::ConsentBook::open(fed::fed_base_dir().join("served-consent.json")));
        let reader = book.clone();
        let consented: principal::Consented = Arc::new(move |ws| reader.consented(ws));
        let recorder = book.clone();
        let on_consent: supragnosis_sync::http::OnConsent =
            Arc::new(move |node, ws, serve| recorder.record(node, ws, serve));
        let extra = supragnosis_sync::http::ExtraSurface {
            router: principal::router(
                engine.clone(),
                Arc::new(principal::Directory::new(fed::config_path())),
                principal::servable(engine.clone(), node.node_id().to_string(), consented, {
                    let peers = peer_registry.clone();
                    Arc::new(move |n: &str| peers.release_of(n))
                }),
                events.clone(),
            ),
            admitted: 0,
        };
        let hooks = supragnosis_sync::http::Hooks {
            on_applied: Some(on_applied),
            on_activity: Some(on_activity),
            on_search: Some(on_search),
            peer_registry: Some(peer_registry.clone()),
            extra: Some(extra),
            on_consent: Some(on_consent),
        };
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let paths = invite::Paths::default_paths();
        let pairing = Arc::new(invite::Pairing::new(
            paths.clone(),
            node.clone(),
            engine.store(),
            invite::hub_name(&paths.config),
        ));
        let hub = Arc::new(hub::HubListener::new(
            engine.store(),
            node,
            hooks,
            peers,
            pairing,
            listener_state.clone(),
        ));
        hub.follow();
        Ok(hub)
    }))
}

/// How long the daemon waits before its next sync round (docs/pairing.md Section 4.5): five minutes
/// after one that worked, and twice the last wait after one that failed, up to an hour.
fn next_round_wait(last: std::time::Duration, failed: bool) -> std::time::Duration {
    const EVERY: std::time::Duration = std::time::Duration::from_secs(300);
    const MOST: std::time::Duration = std::time::Duration::from_secs(3600);
    if failed {
        (last.max(EVERY) * 2).min(MOST)
    } else {
        EVERY
    }
}

/// The daemon syncs on its own (docs/pairing.md Section 4.5): a round soon after it starts, then on
/// `next_round_wait`'s timer. Each round is the `sync_*` tools' push and pull over the shared
/// workspaces, under the same round lock, and `sync_status` reads what the last one did.
fn spawn_sync_rounds(engine: Arc<Engine>, ctx: Arc<supragnosis_mcp::SyncContext>) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut wait = std::time::Duration::ZERO;
        loop {
            let round = supragnosis_mcp::sync_round(&engine, &ctx).await;
            let failed = round.get("ok").and_then(|o| o.as_bool()) == Some(false);
            if failed {
                tracing::warn!(error = ?round.get("error"), "sync round failed");
            } else {
                tracing::info!(pushed = ?round.get("pushed"), applied = ?round.get("applied"), "sync round");
            }
            wait = next_round_wait(wait, failed);
            tokio::time::sleep(wait).await;
        }
    });
}

/// The federation status loop (every 60s): pings each configured hub (an authenticated no-op that
/// verifies connectivity, auth, and authorization in one round trip), computes the per-workspace
/// version-vector diff against each healthy hub ("who is ahead by how many events"), snapshots the
/// known-peer registry (hub role), and publishes everything to the viewer at /api/federation.
/// Health state CHANGES stream to the activity feed (hc-ok / hc-fail); steady state stays quiet.
/// The admitted set as the console reads it: node id and what each peer may read, nothing else.
///
/// `bearer_hash` is deliberately absent. It is a hash rather than a token, so publishing it would not
/// hand anyone a credential - but nothing on this surface reads it, and a credential-shaped field
/// with no reader is only a liability (P17/P18).
fn admitted_json(dir: &supragnosis_sync::http::PeerDirectory) -> Vec<serde_json::Value> {
    dir.admitted()
        .allowlist
        .iter()
        .map(
            |e| serde_json::json!({"node_id": e.node_id, "shared_workspaces": e.shared_workspaces}),
        )
        .collect()
}

/// Inputs to the federation status loop, as a struct rather than positional arguments - the list
/// crossed clippy's threshold when the resolved server links joined it, and a loop that reads eight
/// unrelated things is easier to call wrongly than to read.
struct FedStatusTask {
    engine: Arc<Engine>,
    fed: supragnosis_viz::FedStatus,
    node_id: String,
    is_hub: bool,
    sync: fed::SyncSection,
    links: Vec<supragnosis_sync::ServerLink>,
    surfaces: supragnosis_sync::NegotiatedSurfaces,
    registry: Arc<supragnosis_sync::http::PeerRegistry>,
    admitted: Option<Arc<supragnosis_sync::http::PeerDirectory>>,
}

fn spawn_fed_status(task: FedStatusTask) {
    let FedStatusTask { engine, fed, node_id, is_hub, sync, links, surfaces, registry, admitted } =
        task;
    /// Events `a` holds that `b` lacks, approximated by per-(origin, workspace) seq gaps.
    fn vv_ahead(a: &VersionVector, b: &VersionVector) -> u64 {
        a.0.iter().map(|(k, sa)| sa.saturating_sub(*b.0.get(k).unwrap_or(&0))).sum()
    }
    tokio::spawn(async move {
        let mut last: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        loop {
            let mut servers_json = Vec::new();
            for link in &links {
                let server = &link.url;
                let mut healthy = false;
                let mut version = None;
                let mut protocol = None;
                let mut ws_json = Vec::new();
                let mut diff = supragnosis_sync::SurfaceDiff::default();
                let mut negotiated_at = None;
                if let Ok(client) = supragnosis_sync::http::SyncClient::for_link(link)
                    .map(|c| c.with_serve(sync.serve_set().0))
                {
                    match client.ping().await {
                        Ok(p) => {
                            healthy = true;
                            version = Some(p.version);
                            protocol = Some(p.protocol);
                            // The authorization half of the answer, kept rather than dropped. This
                            // is the only place negotiation happens: handlers read the map under a
                            // lock and never ping, so nothing is added to a call that already
                            // blocks (F11, negotiated-surface.md N1).
                            diff = supragnosis_sync::surface_diff(
                                &sync.share_workspaces,
                                &p.shared_workspaces,
                            );
                            negotiated_at = Some(supragnosis_core::now_millis());
                            if let Ok(mut m) = surfaces.write() {
                                m.insert(
                                    server.clone(),
                                    supragnosis_sync::NegotiatedSurface {
                                        admits: Some(p.shared_workspaces.clone()),
                                        negotiated_at,
                                    },
                                );
                            }
                            // Drift only for what both sides hold. Asking a host to advertise a
                            // workspace it does not admit answers 403, which this loop used to
                            // discard with a bare `continue` - so a misconfigured workspace simply
                            // left the view. It is now named in `local_only` instead.
                            for ws in &diff.both {
                                let store = engine.store();
                                let ws_owned = ws.clone();
                                let local = tokio::task::spawn_blocking(move || {
                                    supragnosis_sync::version_vector(store.as_ref(), &ws_owned)
                                })
                                .await;
                                let (Ok(Ok(local)), Ok(remote)) =
                                    (local, client.advertise(ws).await)
                                else {
                                    continue;
                                };
                                ws_json.push(serde_json::json!({
                                    "workspace": ws,
                                    "local_ahead": vv_ahead(&local, &remote.vv),
                                    "hub_ahead": vv_ahead(&remote.vv, &local),
                                }));
                            }
                        }
                        Err(e) => {
                            // Unreachable is *unknown*, never an empty grant set: a host that is
                            // down must not read as a grant that was revoked (F21 clause 4, F12).
                            // The distinction lives in `record_ping`, where a case holds it.
                            supragnosis_sync::record_ping(&surfaces, server, None, 0);
                            if last.get(server).copied() != Some(false) {
                                tracing::warn!(%server, error = %e, "hub health check failed");
                            }
                        }
                    }
                }
                if healthy && last.get(server).copied() != Some(true) {
                    tracing::info!(%server, "hub health check ok");
                }
                if last.get(server).copied() != Some(healthy) {
                    engine.emit(Event::Sync {
                        direction: if healthy { "hc-ok".into() } else { "hc-fail".into() },
                        peer: server.clone(),
                        workspace: "-".into(),
                        count: 0,
                    });
                    last.insert(server.clone(), healthy);
                }
                servers_json.push(serde_json::json!({
                    "url": server,
                    "healthy": healthy,
                    "version": version,
                    "protocol": protocol,
                    // Unchanged key, narrowed meaning: drift rows for the workspaces both sides
                    // hold. The two new buckets are what the old view had no way to say.
                    "workspaces": ws_json,
                    "local_only": diff.local_only,
                    "peer_only": diff.peer_only,
                    "negotiated_at": negotiated_at,
                }));
            }
            let peers_json = if is_hub {
                serde_json::to_value(registry.snapshot()).unwrap_or_default()
            } else {
                serde_json::Value::Array(Vec::new())
            };
            // Who is admitted RIGHT NOW, read from the live directory rather than from the config
            // this process started with - so the console shows the running state, which is the whole
            // point of admission no longer being a startup snapshot. `bearer_hash` is deliberately
            // not published: it is a hash rather than a token, but nothing on this surface needs it,
            // and a credential-shaped field is not worth carrying for no reader (P17/P18).
            let allowlist_json = admitted.as_ref().map(|d| admitted_json(d)).unwrap_or_default();
            let blob = serde_json::json!({
                "configured": true,
                "node_id": node_id,
                "role": if is_hub { "hub" } else { "client" },
                "updated_ms": supragnosis_core::now_millis(),
                "servers": servers_json,
                "known_peers": peers_json,
                "admitted": allowlist_json,
            });
            if let Ok(mut w) = fed.write() {
                *w = blob;
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    });
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
fn migrate_cmd(a: WorkspaceArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let m = engine.migrate_legacy_ids(&ws)?;
        let r = engine.reproject(Some(&ws))?;
        println!(
            "migrated {} legacy-id observation(s); reprojected {}: {} observations -> {} entities, {} relations",
            m.migrated, ws, r.observations, r.entities, r.relations
        );
        if m.skipped_proposal_events > 0 {
            println!(
                "left {} legacy-id proposal-event row(s) as they are - re-keying one would orphan its verdicts",
                m.skipped_proposal_events
            );
        }
        if m.promotions_left_behind > 0 {
            println!(
                "{} migrated row(s) had a merged promotion, which names the legacy id - promote them again",
                m.promotions_left_behind
            );
        }
        anyhow::Ok(())
    })
}

/// `supragnosis rekey-workspace` - re-create a workspace's knowledge under another name.
///
/// The workspace is inside the content address, so this cannot be a move: the re-keyed rows are new
/// observations and the originals stay (Principle 3). It is not a re-ingest either - every
/// attestation is copied verbatim, so the original observation times and authors survive, which
/// pushing the text back through `observe` would destroy. The store is single-process: stop the
/// daemon first.
fn rekey_workspace_cmd(a: RekeyArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let rep = engine.rekey_workspace(&a.from, &a.to, a.dry_run)?;
        let verb = if a.dry_run { "would re-key" } else { "re-keyed" };
        println!(
            "{verb} {} observation(s) from {} to {} ({} already there, {} proposal-event row(s) left behind)",
            rep.moved, a.from, a.to, rep.already, rep.skipped_proposal_events
        );
        if rep.promotions_left_behind > 0 {
            println!(
                "{} of them had a merged promotion, which stays in {} - promote them again in {} if they should be",
                rep.promotions_left_behind, a.from, a.to
            );
        }
        if a.dry_run {
            println!("dry run - nothing was written");
        } else if rep.moved > 0 {
            let r = engine.reproject(Some(&a.to))?;
            println!(
                "reprojected {}: {} observations -> {} entities, {} relations",
                a.to, r.observations, r.entities, r.relations
            );
        }
        anyhow::Ok(())
    })
}

/// `supragnosis reproject` - one-shot re-materialization (HLC-ordered replay, Prop C). For a node
/// whose log advanced without projection (e.g. a hub that received pushes before the reproject hook
/// existed). The store is single-process: stop the daemon first.
fn reproject_cmd(a: WorkspaceArgs) -> Result<()> {
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
fn sync_cmd(a: WorkspaceArgs) -> Result<()> {
    init_tracing();
    // A running daemon holds the store, so it runs the round itself, over its shared workspaces
    // (docs/pairing.md Section 4.5); only with none running does this process sync on its own.
    if let Some(round) = hub::call("POST", "/sync")? {
        if let Some(e) = round
            .get("error")
            .and_then(|e| e.as_str())
            .filter(|_| round.get("at").is_none())
        {
            anyhow::bail!("{e}");
        }
        let shared: Vec<&str> = round["workspaces"]
            .as_array()
            .map(|w| w.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if round["servers"] == 0 {
            println!("nothing to sync: [sync] names no server");
            return Ok(());
        }
        if shared.is_empty() {
            println!("nothing to sync: [sync] shares no workspace (share_workspaces is empty)");
            return Ok(());
        }
        println!(
            "the running daemon synced {}: pushed {}, applied {}",
            shared.join(", "),
            round["pushed"],
            round["applied"]
        );
        if round["ok"] == false {
            anyhow::bail!(
                "the round failed: {}",
                round["error"].as_str().unwrap_or("no reason given")
            );
        }
        return Ok(());
    }
    let cfg = resolve(RunArgs::default(), false);
    let fc = fed::load()?.ok_or_else(|| {
        anyhow::anyhow!(
            "no federation config at {} - create it with a [sync] section (servers, auth_token, \
             share_workspaces). See docs/federation.md Section 9",
            fed::config_path().display()
        )
    })?;
    let (links, notes) = fc.sync.links();
    for n in &notes {
        tracing::error!("federation configuration: {n}");
    }
    anyhow::ensure!(
        !links.is_empty(),
        "[sync] names no server - nothing to sync against. Add [[sync.server]] entries with a url \
         and an auth_token each"
    );
    let identity = fed::load_or_create_identity()?;
    let node = supragnosis_sync::SyncNode::new(identity)
        .with_seq_mark(fed::fed_base_dir().join("node.seq"));
    // A node a join set up syncs what it shares and what its join recorded, as its daemon's round
    // does, and no other workspace (docs/pairing.md Section 4.5, PA12); any other node syncs the
    // one workspace named, or its default.
    let share = &fc.sync.share_workspaces;
    let workspaces: Vec<String> = match (a.workspace, {
        let (receive, note) = join::receive_list(&join::Paths::default_paths());
        if let Some(note) = note {
            eprintln!("{note}");
        }
        (receive, ())
    }) {
        (Some(ws), (Some(receive), _)) => {
            anyhow::ensure!(
                share.contains(&ws) || receive.contains(&ws),
                "this node syncs only the workspaces it shares and the ones its join recorded \
                 ({}); {ws} is neither",
                share.iter().chain(&receive).cloned().collect::<Vec<_>>().join(", ")
            );
            vec![ws]
        }
        (Some(ws), _) => vec![ws],
        (None, (Some(receive), _)) => {
            let mut all = share.clone();
            all.extend(receive.into_iter().filter(|w| !share.contains(w)));
            all
        }
        (None, (None, _)) => vec![cfg.workspace.clone()],
    };
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let store = engine.store();
        let mut keys = trusted_origin_keys(&fc.sync);
        keys.insert(node.node_id().to_string(), node.public_key_hex());
        let (vouched, vouched_note) = join::vouched_keys(&join::Paths::default_paths(), &mut keys);
        if let Some(note) = vouched_note {
            eprintln!("{note}");
        }
        for ws in &workspaces {
            for link in &links {
                let server = &link.url;
                let client = supragnosis_sync::http::SyncClient::for_link(link)?
                    .with_serve(fc.sync.serve_set().0)
                    .with_vouched(vouched.clone());
                let s = client.sync_workspace(&store, &node, ws, share, &keys).await?;
                println!(
                    "{server} {ws}: pushed {} pulled {} (rejected: by server {}, locally {})",
                    s.pushed, s.pulled, s.rejected_by_server, s.rejected_locally
                );
            }
            let r = engine.reproject(Some(ws))?;
            println!(
                "reprojected {}: {} observations -> {} entities, {} relations",
                ws, r.observations, r.entities, r.relations
            );
        }
        anyhow::Ok(())
    })
}

/// Initializes the stderr log subscriber (idempotent). stdout is the MCP stdio channel, so logs must go to stderr.
/// The program an AI app launches as `<program> bridge`: the stable path, so a Homebrew upgrade does
/// not leave the app pointing at a removed keg (client-connect.md Section 4).
fn bridge_program() -> Result<String> {
    let exe = std::env::current_exe()?.canonicalize()?;
    Ok(lifecycle::stable_program(&exe, |p| p.exists()).to_string_lossy().to_string())
}

/// `supragnosis connect [app]` (docs/client-connect.md Section 4).
fn connect_cmd(a: ConnectArgs) -> Result<()> {
    use connect::{Client, Entry, CLIENTS};
    let env = connect::Env::from_process();
    let program = bridge_program()?;
    let Some(id) = a.client else {
        return connect_list(&env, &program, a.json);
    };
    let client = Client::parse(&id).with_context(|| {
        let ids: Vec<&str> = CLIENTS.iter().map(|c| c.id()).collect();
        format!("unknown app {id:?} - one of: {}", ids.join(", "))
    })?;
    if !client.installed(&env) {
        anyhow::bail!("{} is not installed on this machine", client.name());
    }
    let before = client.entry(&env);
    if a.remove {
        let hook_gone = client == Client::ClaudeCode && capture_remove(&env)?;
        if before == Entry::None {
            if !hook_gone {
                println!("{} has no supragnosis entry - nothing to remove", client.name());
            }
            return Ok(());
        }
        connect_remove(client, &env)?;
        println!("removed the supragnosis entry from {}", client.name());
        return Ok(());
    }
    match connect::plan_connect(before, a.replace) {
        Ok(connect::Plan::AlreadyConnected) => {
            println!("{} is already connected through the bridge", client.name());
            match capture_act(client, a.no_capture) {
                Some(true) => capture_add(&env, &program)?,
                Some(false) if !capture_remove(&env)? => {
                    println!("Claude Code has no capture hook - nothing to remove");
                }
                Some(false) | None => {}
            }
            return Ok(());
        }
        Err(Entry::Unknown) => anyhow::bail!(
            "{} could not be read - not touching a file this cannot parse (client-connect.md C3)",
            files_named(client, &env)
        ),
        Err(other) => anyhow::bail!(
            "{} already has a supragnosis entry ({}){} - re-run with --replace to switch it to the bridge",
            client.name(),
            other.as_str(),
            if other == Entry::Http { ", which holds a copy of the token" } else { "" }
        ),
        Ok(connect::Plan::Add) => {}
        Ok(connect::Plan::Replace(other)) => {
            connect_remove(client, &env)?;
            println!("removed the previous supragnosis entry ({})", other.as_str());
            if other == Entry::Http {
                println!("  the copy of the token it held is gone with it");
            }
        }
    }
    if program.contains("/target/debug/") || program.contains("/target/release/") {
        println!(
            "note: the app will launch a build-tree binary ({program}); `cargo clean` removes it"
        );
    }
    match connect::add_argv(client, &program) {
        Some(argv) => run_client_cli(client, &env, &argv)?,
        None => {
            let (path, section) = client.config(&env);
            let text = connect::read_config(&path).with_context(|| {
                format!("{} could not be read - not touching it", path.display())
            })?;
            let edited =
                connect::upsert(&text, section, connect::NAME, &connect::bridge_entry(&program))
                    .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            let copy = connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                .with_context(|| format!("writing {}", path.display()))?;
            if let Some(b) = copy {
                println!("  backup  {}", b.display());
            }
            // Every file setup changes is named where it ran (docs/onboarding.md O2).
            println!("  wrote   {}", path.display());
        }
    }
    let after = client.entry(&env);
    if after != Entry::Bridge {
        anyhow::bail!(
            "{} did not take the entry - it reads as {} in {}",
            client.name(),
            after.as_str(),
            files_named(client, &env)
        );
    }
    println!("connected {} -> {program} bridge", client.name());
    if client == Client::ClaudeCode && !a.no_capture {
        capture_add(&env, &program)?;
    }
    println!("  next    {}", client.next_step());
    // The entry is written either way, and the app fails until a server answers - said here rather
    // than at the app's first tool call (docs/onboarding.md Section 4.2).
    if let Ok(target) = profile::active(&env.home, |k| std::env::var(k).ok()) {
        if !check_server(&target).answering {
            match target {
                profile::Target::Local => println!(
                    "  warning no daemon answers at http://{}/mcp, so {} cannot reach it yet - `supragnosis setup` starts one and keeps it running",
                    status_http_addr(),
                    client.name()
                ),
                profile::Target::Remote { name, url, .. } => println!(
                    "  warning the server {name:?} ({url}) does not answer, so {} cannot reach it yet - `supragnosis server` checks it",
                    client.name()
                ),
            }
        }
    }
    Ok(())
}

/// Claude Code's settings as text, refusing a file that exists and cannot be read (C3).
fn claude_code_settings_text(env: &connect::Env) -> Result<(std::path::PathBuf, String)> {
    let path = connect::claude_code_settings(env);
    let text = connect::read_config(&path)
        .with_context(|| format!("{} could not be read - not touching it", path.display()))?;
    Ok((path, text))
}

/// Registers the capture hook in Claude Code's settings (client-connect.md Section 12.4), unless
/// it is there already. The file is copied aside first and only the two hook groups are added.
fn capture_add(env: &connect::Env, program: &str) -> Result<()> {
    let (path, mut text) = claude_code_settings_text(env)?;
    match connect::capture_state(&text) {
        connect::Capture::On => {
            println!("  capture is on ({})", path.display());
            return Ok(());
        }
        connect::Capture::Unknown => anyhow::bail!(
            "{} could not be parsed - not touching a file this cannot parse (client-connect.md C3); \
             re-run with --no-capture to connect without the hook",
            path.display()
        ),
        connect::Capture::Off => {}
    }
    for event in connect::HOOK_EVENTS {
        text = connect::add_hook(&text, event, &connect::hook_group(event, program))
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    }
    let copy =
        connect::write_backed_up(&path, "claude-code-settings", &env.home, unix_now(), &text)
            .with_context(|| format!("writing {}", path.display()))?;
    if let Some(b) = copy {
        println!("  backup  {}", b.display());
    }
    println!(
        "  capture on: after a commit the agent is asked for the decision behind it, and a session \
         starts knowing what the workspace holds ({})",
        path.display()
    );
    Ok(())
}

/// Takes the capture hook out of Claude Code's settings. `Ok(true)` when something was removed.
/// What `connect` does to Claude Code's capture hook on an app already connected through the bridge:
/// adds it, or with `--no-capture` removes it and keeps the bridge (client-connect.md Section 12.4),
/// so the Settings page's capture switch is one command spelled two ways. `None` for any other app.
fn capture_act(client: connect::Client, no_capture: bool) -> Option<bool> {
    (client == connect::Client::ClaudeCode).then_some(!no_capture)
}

fn capture_remove(env: &connect::Env) -> Result<bool> {
    let (path, text) = claude_code_settings_text(env)?;
    if connect::capture_state(&text) == connect::Capture::Unknown {
        anyhow::bail!(
            "{} could not be parsed - not touching a file this cannot parse (client-connect.md C3)",
            path.display()
        );
    }
    let mut edited: Option<String> = None;
    for event in connect::HOOK_EVENTS {
        let current = edited.as_deref().unwrap_or(&text);
        if let Some(next) = connect::remove_hook(current, event)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        {
            edited = Some(next);
        }
    }
    let Some(edited) = edited else {
        return Ok(false);
    };
    let copy =
        connect::write_backed_up(&path, "claude-code-settings", &env.home, unix_now(), &edited)
            .with_context(|| format!("writing {}", path.display()))?;
    if let Some(b) = copy {
        println!("  backup  {}", b.display());
    }
    println!("removed the capture hook from Claude Code's settings");
    Ok(true)
}

/// `supragnosis hook <client>`: the hook event on stdin, the answer on stdout, nothing else
/// (docs/client-connect.md Section 12). Silence is exit 0 with no output; an input this cannot
/// read is an error on stderr, which Claude Code shows and carries on from.
fn hook_cmd(a: HookArgs) -> Result<()> {
    if a.client != "claude-code" {
        anyhow::bail!(
            "unknown hook client {:?} - only claude-code has one (docs/client-connect.md Section 12)",
            a.client
        );
    }
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)
        .context("reading the hook event from stdin")?;
    let input: serde_json::Value =
        serde_json::from_str(&raw).context("the hook event on stdin is not JSON")?;
    let git = |dir: &std::path::Path, args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let world = hook::World {
        now: unix_now(),
        git: &git,
        daemon: &viz_get,
        workspace_override: std::env::var("SUPRAGNOSIS_WORKSPACE").ok(),
    };
    if let Some(v) = hook::answer(&input, &world).map_err(|e| anyhow::anyhow!(e))? {
        println!("{v}");
    }
    Ok(())
}

/// The files a client's entry is read from, for a message: one, or OpenCode's two.
fn files_named(client: connect::Client, env: &connect::Env) -> String {
    let files: Vec<String> = client.files(env).iter().map(|f| f.display().to_string()).collect();
    files.join(" or ")
}

fn connect_remove(client: connect::Client, env: &connect::Env) -> Result<()> {
    match connect::remove_argv(client) {
        Some(argv) => run_client_cli(client, env, &argv),
        // OpenCode reads two files, so the entry goes from each that holds it. Both are read and
        // edited before either is written: a file that does not parse stops the removal with
        // neither touched (client-connect.md Section 13.3).
        None if client == connect::Client::OpenCode => {
            let mut edits = Vec::new();
            for path in connect::opencode_files(env) {
                let text = connect::read_config(&path).with_context(|| {
                    format!("{} could not be read - not touching it", path.display())
                })?;
                if let Some(edited) =
                    connect::remove_jsonc(&text, &connect::OPENCODE_SECTION, connect::NAME)
                        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
                {
                    edits.push((path, edited));
                }
            }
            for (path, edited) in edits {
                let copy =
                    connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                        .with_context(|| format!("writing {}", path.display()))?;
                if let Some(b) = copy {
                    println!("  backup  {}", b.display());
                }
                println!("  wrote   {}", path.display());
            }
            Ok(())
        }
        None => {
            let (path, section) = client.config(env);
            let text = connect::read_config(&path).with_context(|| {
                format!("{} could not be read - not touching it", path.display())
            })?;
            let Some(edited) = connect::remove(&text, section, connect::NAME)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
            else {
                return Ok(());
            };
            let copy = connect::write_backed_up(&path, client.id(), &env.home, unix_now(), &edited)
                .with_context(|| format!("writing {}", path.display()))?;
            if let Some(b) = copy {
                println!("  backup  {}", b.display());
            }
            Ok(())
        }
    }
}

fn run_client_cli(client: connect::Client, env: &connect::Env, argv: &[String]) -> Result<()> {
    let cli = client
        .cli(env)
        .with_context(|| format!("{}'s command-line tool was not found", client.name()))?;
    let out = std::process::Command::new(&cli)
        .args(argv)
        .env("PATH", env.path_var())
        .stdin(std::process::Stdio::null())
        .output()
        .with_context(|| format!("running {}", cli.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let msg = String::from_utf8_lossy(&out.stdout);
        anyhow::bail!(
            "{} {} failed: {}",
            cli.display(),
            argv.first().map(String::as_str).unwrap_or(""),
            if err.trim().is_empty() {
                msg.trim().to_string()
            } else {
                err.trim().to_string()
            }
        );
    }
    // The command that changed the app's settings, since no file of ours names the change
    // (docs/onboarding.md Section 4.2, O2).
    println!("  ran     {} {}", cli.display(), argv.join(" "));
    Ok(())
}

/// The schema of every machine-read CLI answer: `status --json`, `connect --json` and
/// `server --json` (docs/compatibility.md Section 6). Adding a field keeps it. Removing or renaming
/// one, changing its type or what a value means raises it, and the desktop app - which reads these -
/// then says to update itself instead of misreading the answer. Each has an example document in
/// tests/fixtures/json that this crate's tests and the app's both read.
const JSON_SCHEMA: u64 = 1;

/// `connect --json`: every AI app this build knows, and how each is connected.
fn connect_document(env: &connect::Env, program: &str) -> serde_json::Value {
    use connect::{Entry, CLIENTS};
    let clients: Vec<_> = CLIENTS
        .iter()
        .map(|c| {
            let installed = c.installed(env);
            let entry = if installed { c.entry(env) } else { Entry::None };
            let capture = match c {
                connect::Client::ClaudeCode => {
                    serde_json::Value::String(capture_state_of(env).as_str().to_string())
                }
                _ => serde_json::Value::Null,
            };
            serde_json::json!({
                "id": c.id(), "name": c.name(), "installed": installed,
                "entry": entry.as_str(), "config": c.config(env).0,
                "via": match c.via() { connect::Via::Cli => "cli", connect::Via::File => "file" },
                "capture": capture,
            })
        })
        .collect();
    serde_json::json!({ "schema": JSON_SCHEMA, "program": program, "clients": clients })
}

fn connect_list(env: &connect::Env, program: &str, json: bool) -> Result<()> {
    print!("{}", connect_listing(env, program, json));
    Ok(())
}

/// What `connect` without an app prints, built by reading and nothing else (C5).
fn connect_listing(env: &connect::Env, program: &str, json: bool) -> String {
    use connect::{Entry, CLIENTS};
    use std::fmt::Write as _;
    if json {
        return format!("{}\n", connect_document(env, program));
    }
    let rows: Vec<_> = CLIENTS
        .iter()
        .map(|c| (*c, c.installed(env), if c.installed(env) { c.entry(env) } else { Entry::None }))
        .collect();
    let mut out = String::from(
        "AI apps - connect one with `supragnosis connect <app>`, or from the Supragnosis menu\n",
    );
    for (c, installed, entry) in rows {
        let state = match (installed, entry) {
            (false, _) => "not installed".to_string(),
            (true, Entry::None) => "not connected".to_string(),
            (true, Entry::Bridge) if c == connect::Client::ClaudeCode => {
                format!("connected (bridge), capture {}", capture_state_of(env).as_str())
            }
            (true, Entry::Bridge) => "connected (bridge)".to_string(),
            (true, Entry::Http) => format!(
                "connected over HTTP, holding a copy of the token - `supragnosis connect {} --replace` moves it to the bridge",
                c.id()
            ),
            (true, Entry::Stdio) => format!(
                "connected to the store-opening stdio server, which cannot run beside the daemon - `supragnosis connect {} --replace`",
                c.id()
            ),
            (true, Entry::Other) => "has another entry named supragnosis".to_string(),
            (true, Entry::Unknown) => format!("its config could not be read ({})", files_named(c, env)),
        };
        let _ = writeln!(out, "  {:<15} {:<15} {state}", c.id(), c.name());
    }
    out
}

/// What Claude Code's settings say about the capture hook, read and nothing else (C5).
fn capture_state_of(env: &connect::Env) -> connect::Capture {
    match connect::read_config(&connect::claude_code_settings(env)) {
        Ok(text) => connect::capture_state(&text),
        Err(_) => connect::Capture::Unknown,
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// --- First run: `supragnosis setup` (docs/onboarding.md) ---------------------------------------
// The decisions and their words are in setup.rs; what is here looks (the lifecycle's observation,
// `connect --json`, the viewer socket) and acts by running this binary's own subcommands, so what a
// person would have read in a terminal is what reaches the app's log drawer.

/// Where setup writes: one JSON object per line for the app, or rows for a person. A test keeps the
/// lines instead of printing them.
#[cfg(unix)]
struct SetupOut {
    events: bool,
    kept: Option<std::cell::RefCell<Vec<String>>>,
}

#[cfg(unix)]
impl SetupOut {
    fn new(events: bool) -> Self {
        SetupOut { events, kept: None }
    }
    fn line(&self, line: String) {
        match &self.kept {
            Some(k) => k.borrow_mut().push(line),
            None => println!("{line}"),
        }
    }
    fn emit(&self, e: &setup::Event) {
        if self.events {
            self.line(serde_json::to_string(e).unwrap_or_default());
        } else if let Some(line) = &e.out {
            self.line(format!("    {line}"));
        } else if e.cmd.is_none() {
            self.line(setup::text_row(e));
        }
    }
    fn list(&self, list: &[setup::Event]) {
        for e in list {
            self.emit(e);
        }
    }
}

/// An act setup can take for a step that is `todo`.
#[cfg(unix)]
#[derive(Clone, Debug, PartialEq)]
enum SetupAct {
    Server,
    App(String),
    Model,
}

#[cfg(unix)]
impl SetupAct {
    fn describe(&self, o: &setup::Observed) -> String {
        match self {
            SetupAct::Server => "keep the server running, now and at every login".to_string(),
            SetupAct::App(id) => format!(
                "connect {}",
                o.apps.iter().find(|a| &a.id == id).map_or(id.as_str(), |a| a.name.as_str())
            ),
            SetupAct::Model => match o.model {
                setup::Model::Missing(mb) => format!("download the search model ({mb} MB)"),
                _ => "download the search model".to_string(),
            },
        }
    }
}

/// The plan: every step the checklist says setup can do, in the order a terminal runs them - the
/// model last, since nothing waits for it (Section 4.3).
#[cfg(unix)]
fn setup_plan(list: &[setup::Event]) -> Vec<SetupAct> {
    use setup::State;
    let mut plan = Vec::new();
    if list.iter().any(|e| e.step == "server" && e.state == State::Todo) {
        plan.push(SetupAct::Server);
    }
    for e in list.iter().filter(|e| e.step == "app" && e.state == State::Todo) {
        plan.extend(e.id.clone().map(SetupAct::App));
    }
    if list.iter().any(|e| e.step == "model" && e.state == State::Todo) {
        plan.push(SetupAct::Model);
    }
    plan
}

#[cfg(unix)]
fn setup_cmd(a: SetupArgs) -> Result<()> {
    let out = SetupOut::new(a.events);
    if a.events {
        println!("{}", serde_json::json!({ "schema": setup::EVENTS_SCHEMA }));
    }
    if let Some(step) = a.step.clone() {
        let ok = setup_step(&a, &out, &step)?;
        return if ok { Ok(()) } else { std::process::exit(1) };
    }
    let observed = setup_observe(a.app_daemon)?;
    let list = setup::checklist(&observed);
    if a.check {
        out.list(&list);
        out.emit(&setup::summary(&list));
        return Ok(());
    }
    let plan = setup_plan(&list);
    if !out.events {
        println!("supragnosis setup - from install to the first entry\n");
        out.list(&list);
        println!();
    }
    if plan.is_empty() {
        out.emit(&setup::summary(&list));
        setup_next(&out, &list);
        return Ok(());
    }
    let chosen = if a.yes {
        plan
    } else {
        if a.events {
            anyhow::bail!(
                "with --events setup asks nothing: add --yes to apply the plan, or --check to only read"
            );
        }
        setup_ask(&observed, plan)?
    };
    if chosen.is_empty() {
        out.emit(&setup::summary(&list));
        return Ok(());
    }
    let mut ok = true;
    for act in &chosen {
        let step = match act {
            SetupAct::Server => "server".to_string(),
            SetupAct::App(id) => format!("app={id}"),
            SetupAct::Model => "model".to_string(),
        };
        ok &= setup_step(&a, &out, &step)?;
    }
    let list = setup::checklist(&setup_observe(a.app_daemon)?);
    if !out.events {
        println!();
        out.list(&list);
        println!();
    }
    out.emit(&setup::summary(&list));
    setup_next(&out, &list);
    if ok {
        Ok(())
    } else {
        std::process::exit(1)
    }
}

/// Shows the plan and asks, as rustup's installer does: Enter applies it, `n` leaves, `c` asks act by
/// act. Asking needs a terminal; without one, `--yes` or `--check` says which the caller meant.
#[cfg(unix)]
fn setup_ask(o: &setup::Observed, plan: Vec<SetupAct>) -> Result<Vec<SetupAct>> {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "setup asks before it acts, and there is no terminal to ask in: add --yes to apply the plan, or --check to only read"
        );
    }
    println!("Setup will:");
    for act in &plan {
        println!("  - {}", act.describe(o));
    }
    // An answer is read per question. The end of input answers no, never the default: a closed
    // terminal or a short script must not consent to a download (or anything else) for the person.
    let read = |q: &str| -> Result<Option<String>> {
        print!("{q}");
        std::io::stdout().flush()?;
        let mut s = String::new();
        if std::io::stdin().lock().read_line(&mut s)? == 0 {
            println!();
            return Ok(None);
        }
        Ok(Some(s.trim().to_ascii_lowercase()))
    };
    let yes = |a: &Option<String>| matches!(a.as_deref(), Some("" | "y" | "yes"));
    let answer = read("Proceed? [Y/n, c to choose] ")?;
    if answer.as_deref() == Some("c") {
        let mut chosen = Vec::new();
        for act in plan {
            let a = read(&format!("  {}? [Y/n] ", act.describe(o)))?;
            if a.is_none() {
                break;
            }
            if yes(&a) {
                chosen.push(act);
            }
        }
        return Ok(chosen);
    }
    if yes(&answer) {
        return Ok(plan);
    }
    println!("Nothing was changed. `supragnosis setup` shows this plan again.");
    Ok(Vec::new())
}

/// What to do next in a terminal, once the acts are done: the question to ask, and how to wait.
#[cfg(unix)]
fn setup_next(out: &SetupOut, list: &[setup::Event]) {
    if out.events {
        return;
    }
    if list.iter().any(|e| e.step == "first" && e.state == setup::State::Working) {
        println!(
            "\nNext: in a connected app, ask \"{}\"\n`supragnosis setup --step wait` waits for the entry, and the viewer shows it.",
            setup::TRY_IT
        );
    }
}

/// One step, applied: `server`, `app=<id>`, `model` or `wait`. Re-reads the state first, does
/// nothing to a step that is not `todo`, and ends with the step's state as it is afterwards. `false`
/// when the step failed or could not be applied (O2).
#[cfg(unix)]
fn setup_step(a: &SetupArgs, out: &SetupOut, step: &str) -> Result<bool> {
    use setup::{Event, State};
    let observed = setup_observe(a.app_daemon)?;
    let list = setup::checklist(&observed);
    let (name, id) = match step.split_once('=') {
        Some(("app", id)) => ("app", Some(id)),
        None if matches!(step, "server" | "model" | "wait") => (step, None),
        _ => anyhow::bail!("unknown step {step:?} - one of: server, app=<id>, model, wait"),
    };
    let row = |l: &[Event], name: &str, id: Option<&str>| {
        l.iter().find(|e| e.step == name && e.id.as_deref() == id).cloned()
    };
    if name == "wait" {
        return setup_wait(a, out, &observed);
    }
    let name: &'static str = if name == "app" {
        "app"
    } else if name == "server" {
        "server"
    } else {
        "model"
    };
    let Some(before) = row(&list, name, id) else {
        let ids: Vec<&str> = connect::CLIENTS.iter().map(|c| c.id()).collect();
        anyhow::bail!("unknown app {:?} - one of: {}", id.unwrap_or(""), ids.join(", "));
    };
    if before.state != State::Todo {
        let ok = matches!(before.state, State::Done | State::Skipped | State::Working);
        out.emit(&before);
        return Ok(ok);
    }
    let args: Vec<String> = match name {
        "server" => match setup::server_act(&observed.runner) {
            Ok(Some(args)) => args.into_iter().map(String::from).collect(),
            Ok(None) => {
                out.emit(&before);
                return Ok(true);
            }
            Err(why) => {
                out.emit(&Event::new("server", None, State::Attention, why));
                return Ok(false);
            }
        },
        "app" => {
            let id = id.unwrap_or_default();
            let mut v = vec!["connect".to_string(), id.to_string()];
            if a.no_capture && id == connect::Client::ClaudeCode.id() {
                v.push("--no-capture".to_string());
            }
            v
        }
        _ if out.events => vec!["embed".into(), "fetch".into(), "--events".into()],
        _ => vec!["embed".into(), "fetch".into()],
    };
    let exe = std::env::current_exe().context("the path of this binary")?;
    let ran = setup_run(out, &exe, name, id, &args)?;
    if let Err(why) = ran {
        let hint = match name {
            "server" => "`supragnosis status` says who runs the daemon.",
            "app" => "The app's settings are as they were, or the copy made first is named above.",
            _ => "Search matches words until the model is here; run setup again when online.",
        };
        out.emit(&Event::new(name, id, State::Failed, format!("{why} {hint}")));
        return Ok(false);
    }
    let after = setup::checklist(&setup_observe(a.app_daemon)?);
    let row_after = row(&after, name, id).unwrap_or(before);
    out.emit(&row_after);
    if name == "app" {
        if let Some(apps) = row(&after, "apps", None) {
            out.emit(&apps);
        }
    }
    Ok(row_after.state != State::Failed)
}

/// Runs `supragnosis <args>` - `exe`, this same binary. In a terminal its output is the person's to
/// read as it comes; with `--events` each line of it is an event naming the command, and a child that
/// writes events itself (`embed fetch --events`) has them passed on as they are. `Err` carries why
/// it failed: the child's last error line, or its exit status.
#[cfg(unix)]
fn setup_run(
    out: &SetupOut,
    exe: &std::path::Path,
    step: &'static str,
    id: Option<&str>,
    args: &[String],
) -> Result<std::result::Result<(), String>> {
    use setup::{Event, State};
    use std::io::BufRead;
    let shown = format!("supragnosis {}", args.join(" "));
    if !out.events {
        println!("$ {shown}");
        let st = std::process::Command::new(exe)
            .args(args)
            .status()
            .with_context(|| format!("running {shown}"))?;
        return Ok(if st.success() { Ok(()) } else { Err(format!("`{shown}` failed ({st}).")) });
    }
    let running = format!("Running {shown}.");
    let mut start = Event::new(step, id, State::Working, running.clone());
    start.cmd = Some(shown.clone());
    out.emit(&start);
    let mut child = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("running {shown}"))?;
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let pipes: Vec<(bool, Box<dyn std::io::Read + Send>)> = vec![
        (false, Box::new(child.stdout.take().context("the child's stdout")?)),
        (true, Box::new(child.stderr.take().context("the child's stderr")?)),
    ];
    for (is_err, pipe) in pipes {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = tx.send((is_err, line));
            }
        });
    }
    drop(tx);
    let mut last_err = String::new();
    for (is_err, line) in rx {
        if line.trim().is_empty() {
            continue;
        }
        if is_err {
            last_err = line.trim().trim_start_matches("Error: ").to_string();
        }
        match serde_json::from_str::<serde_json::Value>(&line) {
            // A child's own events, passed on; its schema line is this run's already.
            Ok(v) if v.get("step").is_some() => out.line(v.to_string()),
            Ok(v) if v.get("schema").is_some() => {}
            _ => {
                let mut e = Event::new(step, id, State::Working, running.clone());
                e.out = Some(line);
                out.emit(&e);
            }
        }
    }
    let st = child.wait().with_context(|| format!("waiting for {shown}"))?;
    Ok(if st.success() {
        Ok(())
    } else if last_err.is_empty() {
        Err(format!("`{shown}` failed ({st})."))
    } else {
        Err(format!("`{shown}` failed: {last_err}."))
    })
}

/// `--step wait` (Section 4.4): polls until the store holds an entry, for `--timeout` seconds. Exits
/// 0 when one is there and 2 when none came, which is not a failure; it writes nothing (O1).
#[cfg(unix)]
fn setup_wait(a: &SetupArgs, out: &SetupOut, o: &setup::Observed) -> Result<bool> {
    use setup::{Event, FirstEntry, State};
    if let Some(r) = &o.remote {
        out.emit(&Event::new(
            "first",
            None,
            State::Skipped,
            format!("Entries go to the remote server \"{r}\"."),
        ));
        return Ok(true);
    }
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(a.timeout);
    let mut said = false;
    let mut checked = false;
    loop {
        match newest_entry() {
            entry @ FirstEntry::Some { .. } => {
                // The checklist's sentence, and the entry's own words under it (Section 29.10).
                let e = setup::entry_event(&entry).expect("an entry");
                out.emit(&e);
                if !out.events {
                    out.line(format!("      \"{}\"", e.quote.as_deref().unwrap_or_default()));
                }
                return Ok(true);
            }
            _ if said && !checked && started.elapsed().as_secs() >= 120 => {
                // Two minutes and nothing: what to check, as the page shows it then.
                checked = true;
                let mut e = Event::new(
                    "first",
                    None,
                    State::Working,
                    "Nothing yet after 2 minutes. Check:",
                );
                e.checks = setup::TRY_IT_CHECKS.iter().map(|c| c.to_string()).collect();
                out.emit(&e);
                if !out.events {
                    for c in &e.checks {
                        out.line(format!("      - {c}"));
                    }
                }
            }
            _ if !said => {
                said = true;
                out.emit(&Event::new(
                    "first",
                    None,
                    State::Working,
                    format!(
                        "Waiting for the first entry. Ask a connected app: \"{}\"",
                        setup::TRY_IT
                    ),
                ));
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            out.emit(&Event::new(
                "first",
                None,
                State::Todo,
                format!(
                    "No entry yet after {} seconds. It appears when a connected app records one; `supragnosis setup --step wait` waits again.",
                    a.timeout
                ),
            ));
            std::process::exit(2);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// What setup reads before it decides anything, every time (O3).
#[cfg(unix)]
fn setup_observe(app_daemon: Option<u32>) -> Result<setup::Observed> {
    let env = connect::Env::from_process();
    // An unreadable profile is reported, never read as "local" (P5).
    let remote = match profile::active(&env.home, |k| std::env::var(k).ok())
        .map_err(|e| anyhow::anyhow!("the server profile could not be read: {e}"))?
    {
        profile::Target::Local => None,
        profile::Target::Remote { name, .. } => Some(name),
    };
    let observed = observe();
    let situation = lifecycle::classify(&observed);
    let app_child = app_daemon
        .is_some_and(|p| i32::try_from(p).is_ok_and(|p| pid_alive(p) && pid_is_supragnosis(p)));
    // The login manager setup can install into: the LaunchAgent, the user unit where a user manager
    // answers, and none where it does not (docs/onboarding.md Section 4.1).
    let platform = if cfg!(target_os = "macos") {
        setup::Platform::Launchd
    } else if systemd_user_available().is_ok() {
        setup::Platform::Systemd { linger: linger_state() }
    } else {
        setup::Platform::Without
    };
    let runner = setup::runner(
        &situation,
        observed.answering,
        canonical_service_state(),
        app_child,
        platform,
    );
    // A daemon refused its store for the model it runs says why, with the way on (SR8): starting it
    // again would fail the same way.
    let runner = match (observed.answering, daemon_embedder_conflict()) {
        (false, Some((recorded, daemon))) => {
            setup::Runner::Attention(embedder_conflict_attention(&recorded, &daemon))
        }
        _ => runner,
    };

    // First contact is this daemon's, so it says nothing about apps that use a remote one.
    let reported = if remote.is_none() && observed.answering { first_contacts() } else { None };
    let contacts_known = reported.is_some();
    let contacts: Vec<(String, u64)> = reported
        .as_ref()
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["name"].as_str()?.to_string(), c["first"].as_u64()?)))
        .collect();
    let now = supragnosis_core::now_millis();
    let doc = connect_document(&env, &bridge_program()?);
    let apps = doc["clients"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let id = c["id"].as_str().unwrap_or_default().to_string();
            setup::App {
                name: c["name"].as_str().unwrap_or_default().to_string(),
                installed: c["installed"].as_bool().unwrap_or(false),
                entry: c["entry"].as_str().unwrap_or("unknown").to_string(),
                next: connect::Client::parse(&id)
                    .map(|c| c.next_step())
                    .unwrap_or_default()
                    .to_string(),
                reached: contacts
                    .iter()
                    .filter(|(n, _)| setup::client_app(n) == Some(id.as_str()))
                    .map(|(_, first)| *first)
                    .min()
                    .map(|t| setup::ago(t, now)),
                id,
            }
        })
        .collect();
    let other_contacts = contacts
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| setup::client_app(n).is_none())
        .collect();
    let health = if observed.answering { store_health() } else { None };
    Ok(setup::Observed {
        remote,
        runner,
        apps,
        contacts_known,
        other_contacts,
        model: setup_model(health.as_ref()),
        first: if observed.answering { newest_entry() } else { setup::FirstEntry::Unknown },
    })
}

/// The search model: the running daemon's own report when it answers (semantic-retrieval.md SR3),
/// and otherwise what this binary finds in the cache under its own environment.
fn setup_model(health: Option<&serde_json::Value>) -> setup::Model {
    let Some(h) = health else {
        return local_model();
    };
    match h.get("semantic") {
        Some(r) if r["state"].as_str() == Some("on") => setup::Model::On,
        Some(r) if r.is_object() => match local_model() {
            // Complete on disk and still off: the daemon tried it and it did not load.
            setup::Model::On => setup::Model::Broken(
                r["note"].as_str().unwrap_or("the search model is downloaded but did not load").to_string(),
            ),
            other => other,
        },
        _ => setup::Model::Skipped(
            "This server runs without a search model (SUPRAGNOSIS_EMBED=none, or a build without one), so search matches words."
                .to_string(),
        ),
    }
}

#[cfg(feature = "fastembed")]
fn local_model() -> setup::Model {
    let kind = std::env::var("SUPRAGNOSIS_EMBED").unwrap_or_default();
    if !matches!(kind.trim(), "" | "fastembed") {
        return setup::Model::Skipped(format!(
            "SUPRAGNOSIS_EMBED={} here, so there is no search model to fetch.",
            kind.trim()
        ));
    }
    match embed_model() {
        Ok(m) if m.present(&supragnosis_embed::model_cache_dir()) => setup::Model::On,
        Ok(m) => setup::Model::Missing(m.download_mb()),
        Err(e) => setup::Model::Broken(format!("{e:#}")),
    }
}

#[cfg(not(feature = "fastembed"))]
fn local_model() -> setup::Model {
    setup::Model::Skipped("This build has no search model, so search matches words.".to_string())
}

/// The newest entry in the store, read through the viewer socket across every workspace. A socket
/// that does not answer, or answers with an error, is unknown, never empty (P5).
#[cfg(unix)]
fn newest_entry() -> setup::FirstEntry {
    let Some(body) = viz_get_within("/api/observations?workspace=*&limit=1", 10_000) else {
        return setup::FirstEntry::Unknown;
    };
    let Ok(serde_json::Value::Array(log)) = serde_json::from_str::<serde_json::Value>(&body) else {
        return setup::FirstEntry::Unknown;
    };
    let Some(o) = log.first() else {
        return setup::FirstEntry::None;
    };
    let mut said: String = o["content"].as_str().unwrap_or_default().chars().take(140).collect();
    if said.len() < o["content"].as_str().unwrap_or_default().len() {
        said.push_str("...");
    }
    // The local read names the app that wrote an attestation (design-system.md Section 29.8); the
    // first one that does is the entry's.
    let app = o["attestations"]
        .as_array()
        .and_then(|a| a.iter().find_map(|t| t["app"].as_str()))
        .map(str::to_string);
    let entity = o["entities"]
        .as_array()
        .and_then(|e| e.first())
        .and_then(|e| e["id"].as_str())
        .map(str::to_string);
    setup::FirstEntry::Some {
        workspace: o["workspace"].as_str().map(str::to_string),
        ago: setup::ago(o["hlc"]["wall"].as_u64().unwrap_or(0), supragnosis_core::now_millis()),
        said,
        app,
        entity,
    }
}

#[cfg(not(unix))]
fn setup_cmd(_a: SetupArgs) -> Result<()> {
    anyhow::bail!("setup is supported only on unix (macOS/Linux)")
}

/// `supragnosis bridge`: stdout carries JSON-RPC and nothing else, so nothing here logs to it.
///
/// The target is the active server profile (remote-server.md Section 3): the loopback daemon by
/// default, or the remote server a profile names. Every AI app launches this same command either
/// way, which is what lets a change of server leave all of them untouched.
fn bridge_cmd() -> Result<()> {
    let home = connect::Env::from_process().home;
    let target =
        profile::active(&home, |k| std::env::var(k).ok()).map_err(|e| anyhow::anyhow!(e))?;
    let cfg = match target {
        profile::Target::Local => {
            let addr = std::env::var("SUPRAGNOSIS_HTTP_ADDR")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "127.0.0.1:7373".to_string());
            let addr = parse_loopback_addr(&addr)?; // loopback only, as `serve` binds (P17)
            bridge::Config {
                url: format!("http://{addr}/mcp"),
                // Read on every request, never copied anywhere (client-connect.md C2).
                token: Arc::new(|| read_secret(&mcp_token_path())),
                wait: std::time::Duration::from_secs(10),
                ca_pem: None,
                unreachable_hint: bridge::NOT_RUNNING.to_string(),
                token_source: mcp_token_path().display().to_string(),
            }
        }
        profile::Target::Remote { name, url, ca, token_file } => {
            let ca_pem = match &ca {
                Some(p) => {
                    Some(std::fs::read(p).with_context(|| format!("reading CA {}", p.display()))?)
                }
                None => None,
            };
            let source = token_file.display().to_string();
            bridge::Config {
                url: url.clone(),
                token: Arc::new(move || read_secret(&token_file)),
                wait: std::time::Duration::from_secs(10),
                ca_pem,
                unreachable_hint: format!(
                    "the server of profile {name:?} does not answer - check the network, or switch \
                     this machine back with `supragnosis server use local`"
                ),
                token_source: source,
            }
        }
    };
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(bridge::run(cfg, tokio::io::stdin(), tokio::io::stdout()))
}

/// `supragnosis server` (docs/remote-server.md Section 3).
fn server_cmd(a: ServerArgs) -> Result<()> {
    let home = connect::Env::from_process().home;
    let mut file = profile::load(&home).map_err(|e| anyhow::anyhow!(e))?;
    let save = |file: &profile::ClientFile| -> Result<()> {
        private_dir(&home.join(".supragnosis"))?;
        connect::write_replacing(&profile::client_path(&home), &profile::render(file))
            .context("writing the server profiles")
    };
    match a.cmd {
        None => server_list(&home, &file, a.json),
        Some(ServerCmd::Add { name, url, ca }) => {
            profile::valid_name(&name).map_err(|e| anyhow::anyhow!(e))?;
            if file.servers.contains_key(&name) {
                anyhow::bail!(
                    "a profile named {name:?} exists - `supragnosis server remove {name}` first"
                );
            }
            let url = profile::normalize_url(&url).map_err(|e| anyhow::anyhow!(e))?;
            let ca = match ca {
                Some(p) => Some(
                    std::fs::canonicalize(&p)
                        .with_context(|| format!("CA bundle {p}"))?
                        .display()
                        .to_string(),
                ),
                None => None,
            };
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() {
                eprintln!("paste the credential the server's operator gave you, then press Enter:");
            }
            let mut token = String::new();
            std::io::stdin()
                .read_line(&mut token)
                .context("reading the credential from stdin")?;
            let token = token.trim();
            if token.is_empty() {
                anyhow::bail!("no credential on stdin - pipe it in, e.g. `supragnosis server add {name} {url} < token-file`");
            }
            let token_file = profile::token_path(&home, &name);
            write_secret(&token_file, token.as_bytes())?;
            file.servers.insert(
                name.clone(),
                profile::ServerEntry {
                    url: url.clone(),
                    ca,
                    token_file: token_file.display().to_string(),
                },
            );
            save(&file)?;
            println!("added server profile {name} -> {url}");
            println!("  credential {} (0600)", token_file.display());
            println!("  use it with `supragnosis server use {name}`");
            Ok(())
        }
        Some(ServerCmd::Use { name }) => {
            if name != profile::LOCAL && !file.servers.contains_key(&name) {
                anyhow::bail!("no server profile named {name:?} - `supragnosis server` lists them");
            }
            file.active = (name != profile::LOCAL).then(|| name.clone());
            save(&file)?;
            println!("AI apps on this machine now use {name} - from their next session; a running session keeps the server it started with");
            Ok(())
        }
        Some(ServerCmd::Remove { name }) => {
            let Some(entry) = file.servers.remove(&name) else {
                anyhow::bail!("no server profile named {name:?}");
            };
            let was_active = file.active.as_deref() == Some(name.as_str());
            if was_active {
                file.active = None;
            }
            save(&file)?;
            let _ = std::fs::remove_file(&entry.token_file);
            println!("removed server profile {name} and its credential");
            if was_active {
                println!("  it was active - AI apps on this machine use local again");
            }
            Ok(())
        }
    }
}

/// Tells a running daemon the hub's admissions changed, so its listener opens or closes as the bind
/// rule now allows (docs/pairing.md Section 4.1). With no daemon running there is nothing to tell.
/// What the admin calls answer when no daemon runs here.
fn no_daemon() -> anyhow::Error {
    anyhow::anyhow!(
        "No supragnosis daemon answers on this machine. Start it - `supragnosis start`, or open the \
         app - and try again."
    )
}

/// An admin call's answer, or its refusal as an error.
fn admin(method: &str, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value> {
    let answer = match body {
        Some(b) => hub::call_with(method, path, b)?,
        None => hub::call(method, path)?,
    }
    .ok_or_else(no_daemon)?;
    if let Some(e) = answer.get("error").and_then(|e| e.as_str()) {
        anyhow::bail!("{e}");
    }
    Ok(answer)
}

fn ask_yes(question: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// `supragnosis invite` (docs/pairing.md Section 3a): asks the daemon for an invitation, prints the
/// code, and waits for a request - its words and Allow in this terminal. Leaving it leaves the
/// invitation open until it expires; `--show` comes back to it.
fn invite_cmd(a: InviteArgs) -> Result<()> {
    if a.cancel {
        admin("POST", "/hub/cancel", None)?;
        println!("Cancelled the open invitation.");
        return Ok(());
    }
    let made = match a.show {
        true => None,
        false => {
            let keep = a
                .keep
                .as_deref()
                .map(|k| {
                    k.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect()
                })
                .unwrap_or_default();
            let ask = hub::InviteAsk { workspaces: keep, addresses: a.address.clone() };
            Some(admin("POST", "/hub/invite", Some(&serde_json::to_value(ask)?))?)
        }
    };
    if a.json {
        let out = match &made {
            Some(m) => m.clone(),
            None => admin("GET", "/hub/invite", None)?,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    let (code, expires, keep, id) = match &made {
        Some(m) => (
            m["code"].as_str().map(str::to_string),
            m["expires"].as_u64(),
            m["workspaces"].clone(),
            m["id"].as_str().unwrap_or_default().to_string(),
        ),
        None => {
            let st = admin("GET", "/hub/invite", None)?;
            let open = &st["open"];
            if open.is_null() {
                match st["last"]["said"].as_str() {
                    Some(said) => println!("No invitation is open. The last one: {said}"),
                    None => println!("No invitation is open. `supragnosis invite --keep <workspaces>` makes one."),
                }
                return Ok(());
            }
            (
                open["code"].as_str().map(str::to_string),
                open["expires"].as_u64(),
                open["workspaces"].clone(),
                open["id"].as_str().unwrap_or_default().to_string(),
            )
        }
    };
    let left = expires.unwrap_or(0).saturating_sub(invite::now()).div_ceil(60);
    let keep: Vec<&str> =
        keep.as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
    if let Some(m) = &made {
        if m["replaced"] == true {
            println!("The invitation that was open is cancelled.");
        }
        if m["listener"]["listening"] != true {
            if let Some(why) = m["listener"]["why"].as_str() {
                println!("The hub is not listening yet: {why}");
            }
        }
        for note in m["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
            println!("{note}");
        }
    }
    match code {
        Some(code) => {
            println!("The code, good for one join and {left} more minute(s):");
            println!();
            println!("  {code}");
            println!();
        }
        None => println!(
            "The code can no longer be shown - the daemon restarted since it was made. It still \
             works for {left} more minute(s), or make a new one."
        ),
    }
    match keep.is_empty() {
        true => println!("It shares no workspace yet."),
        false => println!("The two machines will keep {} in step.", keep.join(", ")),
    }
    println!(
        "On the other machine: Settings > Sync > Join another Mac, or `supragnosis join` and paste it."
    );
    println!("Waiting for it to ask. Leaving here leaves the invitation open; `supragnosis invite --show` comes back.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let st = admin("GET", "/hub/invite", None)?;
        let open = &st["open"];
        if open.is_null() {
            let said = st["last"]["said"].as_str().unwrap_or("The invitation closed.");
            match (st["last"]["state"].as_str(), st["last"]["name"].as_str()) {
                (Some("used"), Some(name)) => println!("{name} joined."),
                _ => println!("{said}"),
            }
            return Ok(());
        }
        if open["id"].as_str() != Some(id.as_str()) {
            println!(
                "A newer invitation replaced this one; `supragnosis invite --show` follows it."
            );
            return Ok(());
        }
        let request = &open["request"];
        let (Some(name), Some(node), Some(words)) = (
            request["name"].as_str(),
            request["node_id"].as_str(),
            request["words"].as_array(),
        ) else {
            continue;
        };
        let words: Vec<String> =
            words.iter().filter_map(|w| w.as_str().map(str::to_string)).collect();
        // The words on a line of their own, apart from the name the joining machine chose.
        println!();
        println!("A machine named \"{name}\" asks to join. Its words:");
        println!();
        println!("    {}", words.join("   "));
        println!();
        // What the person answers is this request and these words: the hub refuses the answer
        // if the request has changed since (PA11).
        let answering = serde_json::json!({"invitation": id, "node_id": node, "words": words});
        if ask_yes("Are they the words the other machine shows? Allow it?")? {
            let joined = admin("POST", "/hub/allow", Some(&answering))?;
            let keep: Vec<&str> = joined["joined"]["workspaces"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|w| w.as_str())
                .collect();
            println!(
                "{name} joined.{}",
                match keep.is_empty() {
                    true => " It shares no workspace yet.".to_string(),
                    false => format!(" The two keep {} in step.", keep.join(", ")),
                }
            );
        } else {
            admin("POST", "/hub/deny", Some(&answering))?;
            println!("Denied. The invitation is closed; make a new code to try again.");
        }
        return Ok(());
    }
}

/// The terminal's side of a join: the preview's questions, and the words.
struct TerminalPerson {
    send: Option<Vec<String>>,
    serve: bool,
    json: bool,
}

/// What the hub's unsigned preview says, as a terminal may print it: no control characters, so a
/// name cannot move the cursor or rewrite the line.
fn printable(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

impl join::Person for TerminalPerson {
    fn preview(
        &mut self,
        hub: &str,
        workspaces: &[String],
        serves_principals: bool,
    ) -> Result<join::Choices> {
        let hub = printable(hub);
        let hub = hub.as_str();
        if self.json {
            println!(
                "{}",
                serde_json::json!({"preview": {"hub_name": hub, "workspaces": workspaces, "serves_principals": serves_principals}})
            );
            return Ok(join::Choices {
                send: self.send.clone().unwrap_or_default(),
                serve: self.serve,
            });
        }
        println!();
        match workspaces.is_empty() {
            true => println!("{hub} shares no workspace yet."),
            false => println!(
                "From {hub}, this machine will receive {}.",
                workspaces.iter().map(|w| printable(w)).collect::<Vec<_>>().join(", ")
            ),
        }
        let send = match &self.send {
            Some(send) => send.clone(),
            None => {
                let mut send = Vec::new();
                for w in workspaces {
                    if ask_yes(&format!("Also send {} from this machine to {hub}?", printable(w)))?
                    {
                        send.push(w.clone());
                    }
                }
                send
            }
        };
        let serve = match (serves_principals, send.is_empty(), self.send.is_some()) {
            (true, false, false) => ask_yes(&format!(
                "{hub} also serves people it admits. May it serve what this machine sends to them?"
            ))?,
            (true, false, true) => self.serve,
            _ => false,
        };
        Ok(join::Choices { send, serve })
    }

    fn words(&mut self, hub: &str, words: &[&str]) {
        let hub = printable(hub);
        if self.json {
            println!("{}", serde_json::json!({"words": words, "hub_name": hub}));
            return;
        }
        println!();
        println!("This machine's words:");
        println!();
        println!("    {}", words.join("   "));
        println!();
        println!("Allow it on {hub} if it shows the same words. Waiting for {hub}...");
    }
}

/// `supragnosis join` (docs/pairing.md Section 4.3): the code on stdin, never as an argument, so it
/// never sits in a process list.
fn join_cmd(a: JoinArgs) -> Result<()> {
    use std::io::{BufRead, IsTerminal, Write};
    init_tracing();
    let interactive = std::io::stdin().is_terminal() && !a.json;
    if interactive {
        print!("Paste the code from the hub, then press Return: ");
        std::io::stdout().flush()?;
    }
    // A code pasted across lines is read until it is whole; from a pipe, all of it is the code.
    let code = if interactive {
        let mut text = String::new();
        let mut lines = 0;
        loop {
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line)? == 0 {
                break pair::Code::decode(&text)?;
            }
            text.push_str(&line);
            lines += 1;
            match pair::Code::decode(&text) {
                Ok(code) => break code,
                Err(pair::CodeError::Damaged) if lines < 6 => continue,
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
        pair::Code::decode(&text)?
    };
    let send = a
        .send
        .as_deref()
        .map(|s| s.split(',').map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect());
    let mut person = TerminalPerson {
        send: send.or(if a.json { Some(Vec::new()) } else { None }),
        serve: a.serve,
        json: a.json,
    };
    let paths = join::Paths::default_paths();
    let me = fed::load_or_create_identity()?;
    let name = a.name.unwrap_or_else(|| invite::hub_name(&paths.config));
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    let joined = rt.block_on(join::join(
        &paths,
        &me,
        &name,
        &code,
        &mut person,
        std::time::Duration::from_secs(2),
    ));
    let joined = match joined {
        Ok(j) => j,
        Err(e) if a.json => {
            println!("{}", serde_json::json!({"error": format!("{e:#}")}));
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    // `[sync]` is read at start: a running daemon restarts to sync with the hub.
    let restarted = restart_the_answering_daemon();
    if a.json {
        println!("{}", serde_json::json!({"joined": joined, "daemon": restarted}));
        return Ok(());
    }
    println!();
    println!("Joined {}.", joined.hub_name);
    match joined.sent.is_empty() {
        true => println!(
            "This machine receives {} and sends nothing yet.",
            list_or_none(&joined.receive)
        ),
        false => println!(
            "This machine receives {} and sends {}.",
            list_or_none(&joined.receive),
            joined.sent.join(", ")
        ),
    }
    match restarted {
        "restarted" => {
            println!("Supragnosis restarted and syncs with {} on its own.", joined.hub_name)
        }
        "not_restarted" => println!("Restart Supragnosis to start syncing."),
        _ => println!("Start Supragnosis - `supragnosis start`, or the app - to start syncing."),
    }
    Ok(())
}

fn list_or_none(list: &[String]) -> String {
    match list.is_empty() {
        true => "no workspace yet".into(),
        false => list.join(", "),
    }
}

/// Restarts the daemon that answers this user's admin socket - and only when it is the one this
/// CLI's service manager runs, so a join made under another HOME, or a daemon started by hand,
/// never restarts a different one. Says "restarted", "not_restarted" or "not_running".
fn restart_the_answering_daemon() -> &'static str {
    let Ok(Some(who)) = hub::call("GET", "/who") else {
        return "not_running";
    };
    let answering = who["pid"].as_u64();
    let managed = match lifecycle::classify(&observe()) {
        lifecycle::Situation::One(lifecycle::Manager::Pidfile { pid }) => Some(pid),
        lifecycle::Situation::One(lifecycle::Manager::Launchd(job))
        | lifecycle::Situation::One(lifecycle::Manager::Systemd(job)) => job.pid,
        _ => None,
    };
    if answering.is_none() || managed.map(u64::from) != answering {
        return "not_restarted";
    }
    match restart(resolve(RunArgs::default(), true)) {
        Ok(()) => "restarted",
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the daemon did not restart");
            "not_restarted"
        }
    }
}

/// `supragnosis hub` (docs/pairing.md Section 4.6).
fn hub_cmd(cmd: HubCmd) -> Result<()> {
    init_tracing();
    match cmd {
        HubCmd::Leave { yes } => {
            let paths = join::Paths::default_paths();
            let hubs = join::joined(&paths)?;
            let Some((_, car)) = hubs.first() else {
                anyhow::bail!("This machine joined no hub through pairing.");
            };
            println!(
                "This machine stops syncing with {} now. What each already holds stays on it.",
                car.name
            );
            if !yes && !ask_yes(&format!("Stop syncing with {}?", car.name))? {
                println!("Nothing changed.");
                return Ok(());
            }
            let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
            let left = rt.block_on(join::leave(&paths))?;
            match left.hub_answered {
                true => println!("{} removed this machine.", left.hub_name),
                false => println!(
                    "{} did not answer, so it keeps this machine's row until it is removed there \
                     (`supragnosis peer remove`). This machine no longer syncs with it.",
                    left.hub_name
                ),
            }
            if restart_the_answering_daemon() == "not_restarted" {
                println!("Restart Supragnosis to stop the rounds it runs.");
            }
            Ok(())
        }
    }
}

/// `supragnosis peer` (docs/pairing.md Section 4.6).
fn peer_cmd(cmd: PeerCmd) -> Result<()> {
    match cmd {
        PeerCmd::List { json } => {
            let peers = invite::peers_json(&invite::Paths::default_paths())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&peers)?);
                return Ok(());
            }
            let joined = peers["joined"].as_array().cloned().unwrap_or_default();
            if joined.is_empty() {
                println!("No machine has joined this one. `supragnosis invite` lets one join.");
            }
            for j in &joined {
                let keep: Vec<&str> = j["workspaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str())
                    .collect();
                println!(
                    "  {:<20} {}  keeps: {}",
                    j["name"].as_str().unwrap_or("?"),
                    j["node_id"].as_str().unwrap_or("?"),
                    if keep.is_empty() { "nothing yet".to_string() } else { keep.join(", ") }
                );
            }
            Ok(())
        }
        PeerCmd::Remove { name, yes } => {
            println!(
                "{name} stops syncing with this machine now. What it already received stays on it."
            );
            if !yes && !ask_yes(&format!("Remove {name}?"))? {
                println!("Nothing changed.");
                return Ok(());
            }
            let out = admin("POST", "/hub/peer/remove", Some(&serde_json::json!({"name": name})))?;
            println!("Removed {}.", out["removed"].as_str().unwrap_or(&name));
            Ok(())
        }
    }
}

fn refresh_hub() {
    match hub::call("POST", "/server/refresh") {
        Ok(Some(state)) if state["listening"] == true => {
            println!("the hub listens on {}", state["address"].as_str().unwrap_or("its address"));
        }
        Ok(Some(state)) => {
            if let Some(why) = state["why"].as_str().or(state["error"].as_str()) {
                println!("the hub is not listening: {why}");
            }
        }
        Ok(None) => {}
        Err(e) => eprintln!("could not tell the running daemon: {e:#}"),
    }
}

/// `supragnosis principal` (docs/remote-server.md Section 4.2). Edits `[server]` in supragnosis.toml
/// in place - comments and layout survive - and the running hub follows the edit on its next request.
fn principal_cmd(cmd: PrincipalCmd) -> Result<()> {
    let path = fed::config_path();
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!("{} - principals belong to a hub's [server] section", path.display())
    })?;
    match cmd {
        PrincipalCmd::Add { name, read, write } => {
            // Under the configuration's lock, on the file as it is now (pairing.md Section 4.4).
            let (edited, credential) = fed::edit_config(|now| {
                let (edited, credential) = principal::add(now, &name, &read, &write)?;
                Ok((edited.clone(), (edited, credential)))
            })?;
            let listen = toml::from_str::<fed::FileConfig>(&edited)
                .ok()
                .and_then(|c| c.server)
                .map(|s| (s.listen, s.tls_cert.is_some()))
                .unwrap_or_default();
            println!("admitted {name}. Its credential, shown this once and stored only as a hash:");
            println!();
            println!("  {credential}");
            println!();
            println!("hand it over privately. On their machine:");
            println!(
                "  supragnosis server add <profile> {}://<this hub's name>:{}/mcp   (paste the credential when asked)",
                if listen.1 { "https" } else { "http" },
                listen.0.rsplit(':').next().unwrap_or("7420")
            );
            println!("or, for an agent's own MCP client: Authorization: Bearer <credential>");
            refresh_hub();
            Ok(())
        }
        PrincipalCmd::Remove { name } => {
            fed::edit_config(|now| Ok((principal::remove(now, &name)?, ())))?;
            println!("revoked {name} - its credential stops working at its next request");
            refresh_hub();
            Ok(())
        }
        PrincipalCmd::List => {
            let cfg: fed::FileConfig =
                toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            let list = cfg.server.map(|s| s.principals).unwrap_or_default();
            if list.is_empty() {
                println!(
                    "no principals - `supragnosis principal add <name> --read <ws> --write <ws>`"
                );
            }
            for p in list {
                println!(
                    "  {:<16} read: {:<30} write: {}",
                    p.name,
                    p.read.join(","),
                    p.write.join(",")
                );
            }
            Ok(())
        }
    }
}

/// What a check of the active server found: whether it answered, and whether it took the credential.
struct ServerCheck {
    answering: bool,
    credential: Option<bool>,
    detail: Option<String>,
}

fn check_server(target: &profile::Target) -> ServerCheck {
    match target {
        profile::Target::Local => ServerCheck {
            answering: port_open(&status_http_addr()),
            credential: None,
            detail: None,
        },
        profile::Target::Remote { url, ca, token_file, .. } => {
            let probe = async {
                let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(4));
                if let Some(p) = ca {
                    let pem = std::fs::read(p).map_err(|e| e.to_string())?;
                    let tls =
                        supragnosis_sync::http::tls_trusting(&pem).map_err(|e| e.to_string())?;
                    b = b.use_preconfigured_tls(tls);
                }
                let client = b.build().map_err(|e| e.to_string())?;
                let mut req = client.get(url).header("Accept", "text/event-stream");
                if let Some(t) = read_secret(token_file) {
                    req = req.bearer_auth(t);
                }
                req.send().await.map(|r| r.status().as_u16()).map_err(|e| e.to_string())
            };
            let status = tokio::runtime::Runtime::new()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(probe));
            match status {
                Ok(401 | 403) => ServerCheck {
                    answering: true,
                    credential: Some(false),
                    detail: Some(format!(
                        "the server refused the credential in {}",
                        token_file.display()
                    )),
                },
                Ok(_) => ServerCheck { answering: true, credential: Some(true), detail: None },
                Err(e) => ServerCheck { answering: false, credential: None, detail: Some(e) },
            }
        }
    }
}

/// `server --json`: the server profiles, which one is active, and whether it answers.
fn servers_document(
    active: &str,
    rows: &[(String, String, bool)],
    check: &ServerCheck,
) -> serde_json::Value {
    let servers: Vec<_> = rows
        .iter()
        .map(|(n, u, remote)| {
            serde_json::json!({"name": n, "url": u, "remote": remote, "active": n == active})
        })
        .collect();
    serde_json::json!({
        "schema": JSON_SCHEMA,
        "active": active,
        "servers": servers,
        "check": {"answering": check.answering, "credential": check.credential, "detail": check.detail},
    })
}

fn server_list(home: &std::path::Path, file: &profile::ClientFile, json: bool) -> Result<()> {
    let target =
        profile::active(home, |k| std::env::var(k).ok()).map_err(|e| anyhow::anyhow!(e))?;
    let local_url = format!("http://{}/mcp", status_http_addr());
    let mut rows = vec![(profile::LOCAL.to_string(), local_url, false)];
    rows.extend(file.servers.iter().map(|(n, e)| (n.clone(), e.url.clone(), true)));
    if let profile::Target::Remote { name, url, .. } = &target {
        if name == "env" {
            rows.push((name.clone(), url.clone(), true));
        }
    }
    let check = check_server(&target);
    if json {
        println!("{}", servers_document(target.name(), &rows, &check));
        return Ok(());
    }
    println!("servers - the bridge sends this machine's AI apps to the active one (*)");
    for (n, u, remote) in &rows {
        let mark = if n == target.name() { "*" } else { " " };
        let what = if *remote { "" } else { "  this machine's daemon" };
        println!("{mark} {n:<12} {u}{what}");
    }
    let verdict = match (check.answering, check.credential) {
        (true, Some(false)) => "answers, but refused the credential".to_string(),
        (true, _) => "answers".to_string(),
        (false, _) => "does not answer".to_string(),
    };
    println!("active server {}: {verdict}", target.name());
    if let Some(d) = check.detail {
        println!("  {d}");
    }
    Ok(())
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
                 (SUPRAGNOSIS_VIZ_SOCK / --viz <path>); the authenticated network read tier is \
                 federation Phase 3.5"
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
    #[cfg(unix)]
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
    #[cfg(not(unix))]
    std::future::pending::<()>().await;
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
/// rejected - this port is the local trust surface; MCP beyond loopback is the hub's governed
/// agent surface on its TLS listener (docs/remote-server.md R1), never this port. A hostname
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
             sovereignty). Use 127.0.0.1:<port>; remote access goes through a hub's TLS listener \
             (sync, and MCP for admitted principals), never this port"
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
    #[cfg(unix)]
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
    #[cfg(unix)]
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
    std::fs::read_to_string(pid_path()).ok().and_then(|s| s.trim().parse().ok())
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
#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_jobs() -> Vec<lifecycle::Job> {
    Vec::new()
}

/// Every systemd user unit under a name the product runs the daemon as (L2) that manages it now:
/// enabled, or active (daemon-lifecycle.md Section 4, "On Linux"). Empty where there is no user
/// manager to ask, and everywhere but Linux.
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
fn current_user() -> Option<String> {
    let out = std::process::Command::new("id").arg("-un").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !name.is_empty()).then_some(name)
}

/// Whether the person's systemd manager outlives their sessions: logind keeps one file per lingering
/// user. `None` when the user name is not known.
#[cfg(unix)]
fn linger_state() -> Option<bool> {
    let user = current_user()?;
    Some(std::path::Path::new("/var/lib/systemd/linger").join(user).exists())
}

/// Runs `systemctl --user` with these arguments; a failure carries systemctl's own words (L4).
#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(unix)]
fn clear_stale_pidfile() {
    if let lifecycle::Pidfile::Stale(_) = pidfile_now() {
        let _ = std::fs::remove_file(pid_path());
    }
}

/// The running daemon's version, from the viewer's `/api/about` over its unix socket. `None` when
/// the socket does not answer - and then the version is unknown, never assumed (Section 5).
#[cfg(unix)]
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
#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_bootstrap_canonical() -> Result<()> {
    anyhow::bail!("a LaunchAgent plist was found on a system without launchd")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_kickstart(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}
#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_bootout(job: &lifecycle::Job) -> Result<()> {
    anyhow::bail!("launchd job {} reported on a system without launchd", job.label)
}

/// Restart a systemd user unit in place. Its program is the path an upgrade repoints, so the new
/// binary is what starts (daemon-lifecycle.md Section 4, "On Linux").
#[cfg(unix)]
fn systemd_restart(job: &lifecycle::Job) -> Result<()> {
    systemctl_user(&["restart", job.label])?;
    println!("restarted systemd unit {} (MCP server + viewer).", job.label);
    await_daemon(job.label)
}

/// Stop a systemd user unit. It stays enabled, so it starts again at the next login - said here,
/// because "stopped" alone would be true only until then.
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
fn status_http_addr() -> String {
    std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string())
}

#[cfg(unix)]
fn start(cfg: Config) -> Result<()> {
    let http = cfg.http.clone().unwrap_or_else(|| "127.0.0.1:7373".to_string());
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
    println!("supragnosis daemon started - MCP http://{http}/mcp  viewer {viz_msg}");
    println!("  pidfile {}  logs {}", pid_path().display(), log_dir().display());
    // Generated here rather than in the child, so the connect line can be printed to the terminal
    // the operator is actually looking at. It is the same file the daemon then loads.
    if cfg.mcp_auth {
        load_or_create_mcp_token()?;
        print_client_command(&http);
    }
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

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(unix)]
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
    // The server this machine's AI apps use (remote-server.md): this daemon, or a remote one. An
    // unreadable profile file is reported, not read as "local" (P5).
    let server =
        match profile::active(&connect::Env::from_process().home, |k| std::env::var(k).ok()) {
            Ok(profile::Target::Local) => serde_json::json!({"name": "local", "remote": false}),
            Ok(profile::Target::Remote { name, url, .. }) => {
                serde_json::json!({"name": name, "url": url, "remote": true})
            }
            Err(e) => serde_json::json!({"error": e}),
        };

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
    if server["remote"].as_bool() == Some(true) {
        println!(
            "  server  AI apps here use {} ({}), not this daemon - `supragnosis server` checks it",
            server["name"].as_str().unwrap_or("?"),
            server["url"].as_str().unwrap_or("?")
        );
    } else if let Some(e) = server["error"].as_str() {
        println!("  server  the server profile could not be read: {e}");
    }
    if let Some(h) = &health {
        let owed = h["owed_projections"].as_u64().unwrap_or(0);
        if owed > 0 {
            println!(
                "  store   {owed} observation(s) not yet projected - a write failed after its append; the log has them, and `supragnosis restart` re-projects them"
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
#[cfg(unix)]
fn canonical_service_label() -> &'static str {
    if cfg!(target_os = "linux") {
        lifecycle::CANONICAL_UNIT
    } else {
        lifecycle::CANONICAL_LABEL
    }
}

/// Where the canonical plist lives - or on Linux the canonical user unit, under `$XDG_CONFIG_HOME`
/// when it is set, as systemd reads it.
#[cfg(unix)]
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
#[cfg(unix)]
fn canonical_service_state() -> &'static str {
    match std::fs::read_to_string(canonical_service_path()) {
        Err(_) => "absent",
        Ok(text) if lifecycle::is_generated(&text) => "generated",
        Ok(_) => "hand_written",
    }
}

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
fn service_install(args: ServiceInstallArgs) -> Result<()> {
    if cfg!(target_os = "linux") {
        systemd_install(args)
    } else {
        launchd_install(args)
    }
}

/// `service uninstall`: the LaunchAgent on macOS, the user unit on Linux.
#[cfg(unix)]
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
    match state {
        "generated" => {
            std::fs::remove_file(&plist)
                .with_context(|| format!("failed to remove {}", plist.display()))?;
            println!("removed {} - the daemon no longer starts at login", plist.display());
        }
        "hand_written" => {
            let to = move_aside(&plist, lifecycle::CANONICAL_LABEL)?;
            println!(
                "moved the hand-written plist aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        _ => println!("no canonical job is installed"),
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

#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_install(_args: ServiceInstallArgs) -> Result<()> {
    anyhow::bail!("service install manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither - `supragnosis serve` runs the daemon in the foreground")
}
#[cfg(all(unix, not(target_os = "macos")))]
fn launchd_uninstall() -> Result<()> {
    anyhow::bail!("service uninstall manages a macOS LaunchAgent or a Linux systemd user unit, and this system has neither")
}
#[cfg(not(unix))]
fn service_install(_args: ServiceInstallArgs) -> Result<()> {
    anyhow::bail!("service install manages a macOS LaunchAgent or a Linux systemd user unit; use 'supragnosis serve --http <ADDR>' here")
}
#[cfg(not(unix))]
fn service_uninstall() -> Result<()> {
    anyhow::bail!("service uninstall manages a macOS LaunchAgent or a Linux systemd user unit")
}

/// Why there is no user manager to install into, and what runs the daemon instead.
#[cfg(unix)]
fn no_user_manager(why: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "systemctl --user does not work here ({why}) - as in many containers, or a session without a user bus - so there is no user manager to keep the daemon running. `supragnosis serve` runs it in the foreground, and `supragnosis start` runs it until you log out"
    )
}

/// The directories a unit's daemon writes outside `~/.supragnosis`, which `ProtectSystem=strict` and
/// `ProtectHome=read-only` would otherwise refuse it: a data directory or viewer socket its
/// environment moved elsewhere.
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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
    match state {
        "generated" => {
            std::fs::remove_file(&unit)
                .with_context(|| format!("failed to remove {}", unit.display()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!("removed {} - the daemon no longer starts at login", unit.display());
        }
        "hand_written" => {
            let to = move_file_aside(&unit, lifecycle::moved_aside_unit_path(&home, unix_secs()))?;
            let _ = systemctl_user(&["daemon-reload"]);
            println!(
                "moved the unit that was not generated aside ({}) - the daemon no longer starts at login",
                to.display()
            );
        }
        _ => println!("no canonical unit is installed"),
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
fn status(_json: bool) -> Result<()> {
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
        /// The shared workspaces a hub may serve to its principals (docs/remote-server.md Section
        /// 4.5). Sharing replicates; serving discloses to people this node never heard of, so it is
        /// asked for separately. Must be a subset of `share_workspaces`.
        #[serde(default)]
        pub serve_workspaces: Vec<String>,
    }

    impl SyncSection {
        /// `serve_workspaces` narrowed to what is shared, and a note for each entry dropped - a
        /// workspace that does not leave this node cannot be served by a hub, and dropping it only
        /// ever discloses less (P24's permitted direction).
        pub fn serve_set(&self) -> (Vec<String>, Vec<String>) {
            let mut notes = Vec::new();
            let serve = self
                .serve_workspaces
                .iter()
                .filter(|w| {
                    let shared = self.share_workspaces.contains(w);
                    if !shared {
                        notes.push(format!(
                            "[sync] serve_workspaces names {w:?}, which is not in share_workspaces - ignored: a workspace that is not shared cannot be served"
                        ));
                    }
                    shared
                })
                .cloned()
                .collect();
            (serve, notes)
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
            let loopback = parsed
                .as_ref()
                .and_then(|u| u.host_str())
                .is_some_and(crate::profile::is_loopback_host);
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
        /// Sync API bind (IP:port). Non-loopback demands TLS and an admitted node or principal (F10).
        pub listen: String,
        pub tls_cert: Option<String>,
        pub tls_key: Option<String>,
        #[serde(default)]
        pub allowlist: Vec<supragnosis_sync::http::AllowEntry>,
        /// The hub's agent surface (docs/remote-server.md Section 4.2): people and agents admitted to
        /// MCP on this listener, each with a credential hash and per-workspace grants.
        #[serde(default)]
        pub principals: Vec<PrincipalEntry>,
    }

    /// One principal: a name, the blake3 hash of its bearer credential, and its grants. Unknown keys
    /// are refused, as in an allowlist entry - this decides who may read and write what.
    #[derive(Debug, Clone, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct PrincipalEntry {
        pub name: String,
        pub token_hash: String,
        #[serde(default)]
        pub read: Vec<String>,
        #[serde(default)]
        pub write: Vec<String>,
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
            #[cfg(unix)]
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

    pub fn edit_config<T>(edit: impl FnOnce(&str) -> Result<(String, T)>) -> Result<T> {
        edit_config_at(&config_path(), edit)
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

        /// docs/remote-server.md Section 4.5: a node can let a hub serve only what it shares - an
        /// entry naming an unshared workspace is dropped with a note, never honored.
        #[test]
        fn serving_is_narrowed_to_what_is_shared() {
            let cfg: FileConfig = toml::from_str(
                "[sync]\nshare_workspaces = [\"team\", \"docs\"]\nserve_workspaces = [\"team\", \"private\"]\n",
            )
            .unwrap();
            let (serve, notes) = cfg.sync.serve_set();
            assert_eq!(serve, vec!["team".to_string()]);
            assert_eq!(notes.len(), 1);
            assert!(notes[0].contains("private"));
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
listen = "0.0.0.0:7420"   # F10: non-loopback needs TLS and an admitted node or principal

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

#[cfg(test)]
mod legacy_store_guard_tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before the unix epoch")
            .as_nanos();
        let d = std::env::temp_dir().join(format!(
            "supragnosis-guard-{tag}-{}-{n}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// architecture.md's "0700 ~/.supragnosis" as behavior rather than description: a directory found
    /// open is closed, and a secret is written 0600 without ever existing at another mode - which is
    /// also why a rewrite leaves no temp file behind.
    #[cfg(unix)]
    #[test]
    fn the_state_directory_and_its_secrets_are_closed_to_other_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tmp("private");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).expect("close");
        assert_eq!(mode(&dir), 0o700);

        let token = dir.join("mcp.token");
        write_secret(&token, b"first").expect("write");
        assert_eq!(mode(&token), 0o600);
        write_secret(&token, b"second").expect("rewrite");
        assert_eq!(std::fs::read(&token).unwrap(), b"second");
        assert_eq!(mode(&token), 0o600);
        let leftovers: Vec<_> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("mcp.token")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F14: an allowlist entry naming this node is reported and ignored, not fatal.
    ///
    /// The ignoring happens in `PeerDirectory` (pinned there, through both construction paths); this
    /// covers the operator-facing half - that the workaround is stated rather than done quietly.
    #[test]
    fn a_node_that_admits_itself_is_reported_and_ignored() {
        let entry = |id: &str| supragnosis_sync::http::AllowEntry {
            node_id: id.into(),
            public_key_hex: "deadbeef".into(),
            bearer_hash: "hash".into(),
            shared_workspaces: vec!["ws".into()],
        };
        let section = |ids: &[&str]| fed::ServerSection {
            listen: "127.0.0.1:7420".into(),
            tls_cert: None,
            tls_key: None,
            allowlist: ids.iter().map(|i| entry(i)).collect(),
            principals: Vec::new(),
        };

        assert!(
            drop_self_admission("self", None).is_empty(),
            "a client-only node has nothing to say"
        );
        assert!(drop_self_admission("self", Some(&section(&["peer-a", "peer-b"]))).is_empty());

        let notes = drop_self_admission("self", Some(&section(&["peer-a", "self"])));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("own id self"), "names what it found: {notes:?}");
        assert!(notes[0].contains("IGNORED"), "and says it was worked around: {notes:?}");
    }

    /// A RocksDB directory is recognised by `CURRENT`, which redb never writes. A directory that
    /// merely exists is not a store: an empty `~/.supragnosis/db` left behind after a completed
    /// migration must not block a clean start forever.
    ///
    /// The first version of this case built `dir/CURRENT` by hand, which is the shape the function
    /// assumed rather than the shape Cozo wrote. Both were wrong the same way, so the test passed
    /// while the guard matched no real store. It now builds the layout a live store has.
    #[test]
    fn a_legacy_store_is_recognised_by_its_rocksdb_marker() {
        let at = |d: &std::path::Path| legacy_cozo_store_at(d.to_str().expect("utf8")).is_some();

        let empty = tmp("detect-empty");
        assert!(!at(&empty), "an empty directory is not a store");

        // What the Cozo adapter actually produced: RocksDB under `data`, `manifest` alongside.
        let real = tmp("detect-real");
        std::fs::create_dir_all(real.join("data")).expect("data dir");
        std::fs::write(real.join("manifest"), b"storage_version").expect("manifest");
        std::fs::write(real.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        assert!(at(&real), "data/CURRENT is where a Cozo-era store keeps its marker");

        // A store moved or unpacked flat is still a store.
        let flat = tmp("detect-flat");
        std::fs::write(flat.join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        assert!(at(&flat), "a marker at the top level is accepted too");

        // And a redb directory is not mistaken for one, in either position.
        let redb = tmp("detect-redb");
        std::fs::write(redb.join("knowledge.redb"), b"redb").expect("redb file");
        assert!(!at(&redb), "redb writes no CURRENT anywhere");

        for d in [empty, real, flat, redb] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// The store a node was configured to use is the one the guard has to look at. Every deployment
    /// that set `SUPRAGNOSIS_DATA_DIR` - the systemd case - kept its knowledge outside the default,
    /// so a guard reading only the default would wave through the upgrade it exists to stop.
    #[test]
    fn the_guard_looks_where_the_node_was_configured_to_keep_its_data() {
        let dir = tmp("configured-legacy");
        std::fs::create_dir_all(dir.join("data")).expect("data dir");
        std::fs::write(dir.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");

        let cfg = Config {
            host: "h".into(),
            workspace: "default".into(),
            store_kind: "redb".into(),
            data_dir: dir.to_str().expect("utf8").to_string(),
            embed_kind: "none".into(),
            session: "s".into(),
            http: None,
            viz: None,
            mcp_auth: true,
        };
        // HOME is never read on this path: the configured dir answers first, so this case needs
        // none of the environment juggling the default-location case does.
        let err =
            refuse_unmigrated_store(&cfg).expect_err("a legacy store at data_dir must refuse");
        let msg = err.to_string();
        assert!(
            msg.contains(dir.to_str().expect("utf8")),
            "the refusal names the store it found: {msg}"
        );

        // Once the redb store exists the migration has run, and the leftover is the operator's.
        std::fs::write(redb_path(&cfg.data_dir), b"redb").expect("redb file");
        assert!(
            refuse_unmigrated_store(&cfg).is_ok(),
            "a migrated node starts with the old directory still present"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The refusal names both stores, says the knowledge still exists, and gives the exact release
    /// that can read it. An error that only said "cannot open" would leave the operator with a
    /// directory they cannot interpret and no path forward.
    ///
    /// This is what carries Principle 3's "every encoding the log has ever used stays readable" now
    /// that the adapter which read the older encodings is gone: they remain reachable through the
    /// release that wrote them, and skipping that step fails loudly instead of starting empty.
    #[test]
    fn an_unmigrated_store_is_refused_with_the_way_out() {
        let dir = tmp("refuse");
        std::fs::create_dir_all(dir.join("data")).expect("data dir");
        std::fs::write(dir.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        // Point the default cozo location at the fixture, so the guard sees a legacy store.
        let home = tmp("home");
        std::fs::create_dir_all(home.join(".supragnosis")).expect("home");
        std::fs::rename(&dir, home.join(".supragnosis/db")).expect("place legacy store");
        let prev = std::env::var("HOME").ok();
        // SAFETY-free: set_var is safe on this edition; the guard reads HOME through default_data_dir_for.
        std::env::set_var("HOME", &home);

        let cfg = Config {
            host: "h".into(),
            workspace: "default".into(),
            store_kind: "redb".into(),
            data_dir: home.join(".supragnosis/redb").to_string_lossy().into_owned(),
            embed_kind: "none".into(),
            session: "s".into(),
            http: None,
            viz: None,
            mcp_auth: true,
        };
        let err = refuse_unmigrated_store(&cfg).expect_err("must refuse").to_string();
        assert!(err.contains("Cozo store"), "names what it found: {err}");
        assert!(err.contains("v0.1.21"), "names the release that can read it: {err}");
        assert!(err.contains("migrate-store"), "names the command: {err}");
        assert!(err.contains("not lost"), "says the knowledge survives: {err}");

        // A config still naming the dropped store must reach the same refusal. It used to skip the
        // guard and then open redb inside the legacy directory - the exact silent-empty-start this
        // guards against, reached by the one setting most likely to survive an upgrade.
        let stale = Config { store_kind: "cozo".into(), ..cfg.clone() };
        assert!(
            refuse_unmigrated_store(&stale).is_err(),
            "a stale store kind must not walk past the migration guard"
        );
        // The in-memory store touches no directory, so it is the one kind that legitimately skips.
        let mem = Config { store_kind: "mem".into(), ..cfg.clone() };
        assert!(
            refuse_unmigrated_store(&mem).is_ok(),
            "the in-memory store opens nothing on disk"
        );

        // Once the redb store exists the migration has run; the leftover directory must not block.
        std::fs::create_dir_all(home.join(".supragnosis/redb")).expect("redb dir");
        std::fs::write(redb_path(&cfg.data_dir), b"").expect("redb file");
        assert!(
            refuse_unmigrated_store(&cfg).is_ok(),
            "a rollback artifact beside a migrated store is not an error"
        );

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}

/// docs/compatibility.md Section 6: each machine-read answer has one example document in
/// tests/fixtures/json, read by these tests and by the desktop app's. The producer and its example
/// must have the same keys at every level and the same kind of value wherever both hold one. A
/// field this side renames or drops fails here; a field the app reads that the example lacks fails
/// on the app's side.
#[cfg(test)]
mod json_contract {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    fn example(name: &str) -> Value {
        let text = match name {
            "status" => include_str!("../tests/fixtures/json/status.json"),
            "connect" => include_str!("../tests/fixtures/json/connect.json"),
            "servers" => include_str!("../tests/fixtures/json/servers.json"),
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

    #[cfg(unix)]
    /// SR10 (semantic-retrieval.md Section 13): the rebuild stops only a daemon this CLI manages for
    /// this home - a `start` daemon, or the login job whose file is here - and refuses every other
    /// holder with nothing changed; with nothing running it resets, and starts the login job if this
    /// home installs one.
    #[cfg(unix)]
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
    #[cfg(unix)]
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
            server: serde_json::json!({"name": "lab", "url": "https://lab:7420/mcp", "remote": true}),
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

    #[test]
    fn server_json_matches_its_example() {
        let rows = vec![
            ("local".to_string(), "http://127.0.0.1:7373/mcp".to_string(), false),
            ("lab".to_string(), "https://lab:7420/mcp".to_string(), true),
        ];
        let check = ServerCheck { answering: true, credential: Some(true), detail: None };
        let doc = servers_document("lab", &rows, &check);
        assert_eq!(doc["schema"], JSON_SCHEMA);
        same_shape(&example("servers"), &doc, "servers").unwrap_or_else(|e| panic!("{e}"));
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

/// sync-correctness.md Section 10: the bearer is never sent where it could be read in transit.
#[cfg(test)]
mod link_transport {
    use super::*;

    /// docs/pairing.md Section 4.1 and Principle 24: a `[server]` whose listener cannot open leaves
    /// the node serving and says why, and the listener opens and closes as the bind rule allows -
    /// an open invitation counting as an admission until it expires (D5). It stopped the daemon
    /// before, and launchd restarted it into the same exit.
    #[tokio::test]
    async fn a_listener_that_cannot_open_leaves_the_node_serving() {
        let store: Arc<dyn supragnosis_core::AssertionStore> =
            Arc::new(supragnosis_store::InMemoryStore::new());
        let node = Arc::new(supragnosis_sync::SyncNode::new(
            supragnosis_core::NodeIdentity::from_secret_bytes([7u8; 32]),
        ));
        let state: Arc<std::sync::RwLock<serde_json::Value>> = Default::default();
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let dir = std::env::temp_dir().join(format!("sg-listener-{}", std::process::id()));
        let paths = invite::Paths {
            hub: dir.join("hub"),
            config: dir.join("supragnosis.toml"),
            listen: "192.0.2.1:7420".into(),
        };
        let pairing = Arc::new(invite::Pairing::new(
            paths.clone(),
            node.clone(),
            store.clone(),
            "hub".into(),
        ));
        let listener = hub::HubListener::new(
            store,
            node,
            supragnosis_sync::http::Hooks::default(),
            peers,
            pairing.clone(),
            state.clone(),
        );
        let srv = |listen: &str, cert: Option<&str>, key: Option<&str>| fed::ServerSection {
            listen: listen.into(),
            tls_cert: cert.map(String::from),
            tls_key: key.map(String::from),
            allowlist: Vec::new(),
            principals: Vec::new(),
        };
        let unreadable = listener
            .apply(&srv("127.0.0.1:0", Some("/nonexistent/hub.crt"), Some("/nonexistent/hub.key")))
            .await;
        assert_eq!(unreadable["listening"], false, "{unreadable}");
        assert!(
            unreadable["why"].as_str().is_some_and(|w| w.contains("could not start")),
            "{unreadable}"
        );
        let loopback = listener.apply(&srv("127.0.0.1:0", None, None)).await;
        assert_eq!(loopback["listening"], true, "loopback needs no admission: {loopback}");
        let open = listener.apply(&srv("0.0.0.0:0", None, None)).await;
        assert_eq!(open["listening"], false, "{open}");
        assert!(open["why"].as_str().is_some_and(|w| w.contains("TLS")), "{open}");
        assert_eq!(
            state.read().expect("lock")["listening"],
            false,
            "sync_status reads the same state"
        );
        // An address no interface holds: the bind rule is all that differs between the two applies.
        // Nothing admitted refuses before binding; an open invitation passes the rule and the bind
        // itself then fails - no listener opens on this machine during the test.
        let (cert, key) =
            invite::make_certificate(&paths.hub, &["hub.local".into()], invite::now())
                .expect("cert");
        let elsewhere = srv("192.0.2.1:7420", Some(&cert), Some(&key));
        let closed = listener.apply(&elsewhere).await;
        assert!(
            closed["why"].as_str().is_some_and(|w| w.contains("no open invitation")),
            "{closed}"
        );
        let t = invite::now();
        pairing
            .invite(vec![], vec!["hub.local:7420".into()], pair::CertCheck::ByName, t)
            .expect("invites");
        let invited = listener.apply(&elsewhere).await;
        assert!(
            invited["why"].as_str().is_some_and(|w| w.contains("could not start")),
            "an open invitation counts as an admission: {invited}"
        );
        pairing.expire(t + invite::VALID_FOR).expect("expires");
        let expired = listener.apply(&elsewhere).await;
        assert!(
            expired["why"].as_str().is_some_and(|w| w.contains("no open invitation")),
            "and stops counting at its expiry: {expired}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn links_of(toml: &str) -> (Vec<supragnosis_sync::ServerLink>, Vec<String>) {
        let fc: fed::FileConfig = toml::from_str(toml).expect("parses");
        fc.sync.links()
    }

    #[test]
    fn the_bearer_only_crosses_the_network_encrypted_to_a_verified_host() {
        let ca = std::env::temp_dir().join(format!("supragnosis-ca-{}.pem", std::process::id()));
        std::fs::write(&ca, "-----BEGIN CERTIFICATE-----\n").expect("write");
        let (links, notes) = links_of(&format!(
            r#"
            [sync]
            insecure_tls = true
            [[sync.server]]
            url = "https://hub.example:7420"
            auth_token = "a"
            [[sync.server]]
            url = "http://hub.example:7420"
            auth_token = "b"
            [[sync.server]]
            url = "http://127.0.0.1:7420"
            auth_token = "c"
            [[sync.server]]
            url = "https://named.example:7420"
            auth_token = "e"
            ca = "{}"
            [[sync.server]]
            url = "https://missing.example:7420"
            auth_token = "f"
            ca = "/nonexistent/ca.pem"
            "#,
            ca.display()
        ));
        let by_url = |u: &str| links.iter().find(|l| l.url == u);
        assert!(by_url("https://hub.example:7420").is_some(), "kept, and verified by the system");
        assert!(by_url("http://hub.example:7420").is_none(), "plain HTTP off loopback is off");
        assert!(by_url("http://127.0.0.1:7420").is_some(), "plain HTTP on loopback stays");
        assert!(by_url("https://named.example:7420").expect("named").ca_pem.is_some());
        assert!(by_url("https://missing.example:7420").is_none(), "an unreadable ca disables it");
        let said = |needle: &str| notes.iter().any(|n| n.contains(needle));
        assert!(said("insecure_tls is no longer read"), "{notes:?}");
        assert!(said("http://hub.example:7420 is plain HTTP"), "{notes:?}");
        assert!(said("/nonexistent/ca.pem"), "{notes:?}");
        let _ = std::fs::remove_file(ca);
    }
}

/// sync-correctness.md Section 7: a key in another spelling is named and ignored, in both places a
/// configuration holds keys, and the canonical entries beside it are kept.
#[cfg(test)]
mod key_spelling {
    use super::*;

    #[test]
    fn a_key_in_another_spelling_is_named_and_ignored() {
        let good = "ab".repeat(32);
        let toml = format!(
            r#"
            [sync]
            share_workspaces = ["ws"]
            [sync.origin_keys]
            hub = "{good}"
            loud = "{upper}"

            [server]
            listen = "127.0.0.1:7420"
            [[server.allowlist]]
            node_id = "peer"
            public_key_hex = "{upper}"
            bearer_hash = "{good}"
            shared_workspaces = ["ws"]
            "#,
            upper = good.to_uppercase()
        );
        let fc: fed::FileConfig = toml::from_str(&toml).expect("parses");
        let notes = noncanonical_key_notes(&fc);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("peer") && notes[1].contains("loud"));
        let keys = trusted_origin_keys(&fc.sync);
        assert_eq!(keys.keys().collect::<Vec<_>>(), vec!["hub"]);
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

/// docs/onboarding.md: what setup reads, what it runs, and what it says when an act fails.
#[cfg(all(test, unix))]
mod first_run {
    use super::*;

    /// This file's setup section, from its banner to the bridge command after it.
    fn section() -> &'static str {
        let src = include_str!("main.rs");
        let start = src.find("// --- First run: `supragnosis setup`").expect("the setup section");
        let end = start + src[start..].find("/// `supragnosis bridge`: stdout").expect("its end");
        &src[start..end]
    }

    /// O1 and O6: setup reaches the daemon only through reads of the viewer socket, and acts only by
    /// running this binary's `service install`, `connect` and `embed fetch` - nothing that observes,
    /// syncs, adds a server, or replaces an entry `connect` did not write.
    #[test]
    fn setup_reads_the_daemon_and_runs_only_its_own_acts() {
        let s = section();
        let mut reads: Vec<&str> = s
            .match_indices("viz_get(\"")
            .chain(s.match_indices("viz_get_within(\""))
            .map(|(i, m)| {
                let r = &s[i + m.len()..];
                &r[..r.find('"').expect("a closing quote")]
            })
            .collect();
        reads.sort_unstable();
        assert_eq!(reads, ["/api/observations?workspace=*&limit=1"]);
        // The other reads go through the helpers `status` uses: /api/clients and /api/health.
        assert!(s.contains("first_contacts()") && s.contains("store_health()"));
        for helper in ["fn first_contacts(", "fn store_health("] {
            let src = include_str!("main.rs");
            let body = &src[src.find(helper).expect(helper)..];
            let body = &body[..body.find("\n}\n").expect("its end")];
            assert_eq!(body.matches("viz_get(").count(), 1, "{helper} reads one path");
        }
        assert_eq!(s.matches("Command::new(").count(), 2, "a subprocess other than this binary");
        assert_eq!(s.matches("Command::new(exe)").count(), 2);
        let acts = &s[s.find("let args: Vec<String> = match name {").expect("the acts")..];
        let acts = &acts[..acts.find("let exe =").expect("their end")];
        assert!(acts.contains("setup::server_act(&observed.runner)"));
        assert!(acts.contains("\"connect\".to_string()") && acts.contains("\"embed\".into()"));
        for word in [
            "\"observe\"",
            "\"sync\"",
            "\"principal\"",
            "\"server\".into",
            "--replace",
            "--take-over",
        ] {
            assert!(!acts.contains(word), "setup runs `{word}`");
        }
    }

    /// O3: the plan is what the checklist says is `todo`, and only that - a step done, skipped or
    /// waiting on the person is not acted on - and the model comes last, since nothing waits on it
    /// (O5).
    #[test]
    fn the_plan_is_what_is_todo_and_the_model_goes_last() {
        use setup::{Event, State};
        let list = vec![
            Event::new("server", None, State::Done, ""),
            Event::new("apps", None, State::Todo, ""),
            Event::new("app", Some("cursor"), State::Attention, ""),
            Event::new("model", None, State::Todo, ""),
            Event::new("app", Some("codex"), State::Todo, ""),
            Event::new("app", Some("gemini"), State::Skipped, ""),
            Event::new("first", None, State::Working, ""),
        ];
        assert_eq!(setup_plan(&list), vec![SetupAct::App("codex".into()), SetupAct::Model]);
        let mut fresh = list.clone();
        fresh[0].state = State::Todo;
        assert_eq!(setup_plan(&fresh)[0], SetupAct::Server, "the server first");
        let done: Vec<Event> =
            list.into_iter().map(|e| Event { state: State::Done, ..e }).collect();
        assert!(setup_plan(&done).is_empty(), "running again changes nothing that is done");
    }

    /// O2: an act's command is named before it runs, each line it writes is an event (a file it
    /// changed among them), a child's own events pass through as they are, and a failure carries
    /// the child's reason.
    #[test]
    fn a_failed_act_says_why_and_names_what_it_ran() {
        let out = SetupOut { events: true, kept: Some(Default::default()) };
        let script = "echo '{\"schema\":1}'; \
                      echo '{\"step\":\"model\",\"state\":\"working\",\"say\":\"1 of 2 MB.\"}'; \
                      echo '  wrote   /x/mcp.json'; \
                      echo 'Error: the file could not be read' >&2; exit 3";
        let ran = setup_run(
            &out,
            std::path::Path::new("/bin/sh"),
            "app",
            Some("cursor"),
            &["-c".to_string(), script.to_string()],
        )
        .expect("it ran");
        let why = ran.expect_err("it failed");
        assert!(why.ends_with("failed: the file could not be read."), "{why}");
        let lines: Vec<serde_json::Value> = out
            .kept
            .expect("kept")
            .into_inner()
            .iter()
            .map(|l| serde_json::from_str(l).expect("JSON"))
            .collect();
        assert!(
            lines[0]["cmd"].as_str().is_some_and(|c| c.starts_with("supragnosis -c")),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l["out"] == "  wrote   /x/mcp.json" && l["id"] == "cursor"));
        assert!(lines.iter().any(|l| l["step"] == "model" && l["say"] == "1 of 2 MB."));
        assert!(lines.iter().all(|l| l.get("schema").is_none()), "the child's schema line");
    }
}

#[cfg(test)]
mod capture_switch_tests {
    use super::{capture_act, connect::Client};

    /// client-connect.md Section 12.4: on an app already connected through the bridge, `connect
    /// claude-code` adds the capture hook and `--no-capture` removes it, which is the Settings page's
    /// switch (design-system.md Section 6a); any other app has no hook to change.
    #[test]
    fn the_capture_switch_is_connect_spelled_two_ways() {
        assert_eq!(capture_act(Client::ClaudeCode, false), Some(true));
        assert_eq!(capture_act(Client::ClaudeCode, true), Some(false));
        assert_eq!(capture_act(Client::Cursor, false), None);
        assert_eq!(capture_act(Client::Cursor, true), None);
    }
}
