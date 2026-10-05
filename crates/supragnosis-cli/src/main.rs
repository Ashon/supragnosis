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
use supragnosis_core::{AssertionStore, EmbeddingProvider, KnowledgeStore, VersionVector};
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
    /// Connect an AI app (Claude Desktop, Claude Code, Cursor, VS Code, Codex, Gemini) to this
    /// node through the bridge; with no app, list them and how each is connected
    Connect(ConnectArgs),
    /// Which supragnosis server this machine's AI apps use - this machine's daemon, or a remote one
    /// (docs/remote-server.md); with no subcommand, list the profiles and check the active one
    Server(ServerArgs),
    /// The people and agents this hub admits to MCP on its [server] listener (docs/remote-server.md)
    Principal {
        #[command(subcommand)]
        cmd: PrincipalCmd,
    },
    /// The always-on daemon as a login item (macOS LaunchAgent com.supragnosis.daemon)
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Show this node's federation identity (node id + public key); --hash-token hashes a bearer token for an allowlist entry
    Identity(IdentityArgs),
    /// One-shot federation sync round against the configured servers (requires supragnosis.toml; stop the daemon first - the store is single-process)
    Sync(SyncArgs),
    /// Re-materialize a workspace's entity/relation projection from the observation log (HLC-ordered replay; stop the daemon first)
    Reproject(SyncArgs),
    /// Migrate legacy-id observations (pre-0.1.x content-address eras) to the current formula so they can sync (stop the daemon first)
    Migrate(SyncArgs),
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

#[derive(Subcommand)]
enum ServiceCmd {
    /// Generate the LaunchAgent and load it: the daemon starts now and at every login
    Install(ServiceInstallArgs),
    /// Unload the LaunchAgent and retire its plist: the daemon stops and no longer starts at login
    Uninstall,
}

// Read only by the macOS implementation; elsewhere `service` refuses before looking at them.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Args, Clone, Default)]
struct ServiceInstallArgs {
    /// Retire every other manager first (a brew services job, a pidfile daemon, a retired label) and
    /// move a hand-written plist aside instead of refusing.
    #[arg(long)]
    take_over: bool,
    /// Set a SUPRAGNOSIS_* variable in the job's environment (repeatable). Variables a replaced plist
    /// set are carried forward on their own.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    env: Vec<String>,
}

#[derive(Args, Clone, Default)]
struct ConnectArgs {
    /// The AI app: claude-desktop, claude-code, cursor, vscode, codex or gemini. Omit to list them.
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
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod lifecycle;
mod principal;
mod profile;

fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Serve(RunArgs::default())) {
        Cmd::Serve(a) => run_blocking(resolve(a, false)),
        Cmd::Start(a) => start(resolve(a, true)),
        Cmd::Stop => stop(),
        Cmd::Restart(a) => restart(resolve(a, true)),
        Cmd::Status(a) => status(a.json),
        Cmd::Bridge => bridge_cmd(),
        Cmd::Connect(a) => connect_cmd(a),
        Cmd::Server(a) => server_cmd(a),
        Cmd::Principal { cmd } => principal_cmd(cmd),
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
            tracing::warn!(
                kind = other,
                "unknown SUPRAGNOSIS_EMBED - proceeding with keyword search"
            );
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
    let embedder = build_embedder(&cfg.embed_kind);
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
    // Create the event channel only when something subscribes to it - the local viewer, or a hub's
    // read tier. The engine sink and the SSE subscriptions share it.
    let hub = fedcfg.as_ref().is_some_and(|c| c.server.is_some());
    let events =
        (cfg.viz.is_some() || hub).then(|| tokio::sync::broadcast::channel::<String>(256).0);
    let engine = build_engine(&cfg, events.as_ref())?;

    // Federation status blob (viewer /api/federation) - exists only when supragnosis.toml does.
    let fed_status: Option<supragnosis_viz::FedStatus> = fedcfg
        .as_ref()
        .map(|_| Arc::new(std::sync::RwLock::new(serde_json::json!({"configured": true}))));

    // Federation wiring (M4 Phase 4, docs/federation.md Section 9): optional supragnosis.toml.
    // Absent = standalone node (no behavior change); present-but-broken = fail loud (P5). Built
    // before the viewer so a misconfigured federation dies before any socket is bound, and so the
    // console can be handed the admission handler this produces.
    let (sync_ctx, narrow) =
        build_sync_context(&engine, fedcfg, fed_status.clone(), events.clone())?;

    if let (Some(sock), Some(tx)) = (cfg.viz.as_ref(), events.as_ref()) {
        spawn_viz(&engine, sock, tx.clone(), fed_status, narrow).await;
    }

    match cfg.http.as_deref() {
        Some(http) => {
            serve_http_daemon(
                engine,
                sync_ctx,
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
    fed_status: Option<supragnosis_viz::FedStatus>,
    events: Option<tokio::sync::broadcast::Sender<String>>,
) -> Result<(Option<Arc<supragnosis_mcp::SyncContext>>, Option<supragnosis_viz::NarrowShare>)> {
    let Some(fc) = fedcfg else {
        return Ok((None, None));
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
    for n in &config_notes {
        tracing::error!("federation configuration: {n}");
    }
    // One handle, written by the health loop and read by the tools. Created here because this is
    // where both sides are wired, and typed in the sync crate so neither adapter has to depend on
    // the other to see it (negotiated-surface.md Section 2).
    let surfaces: supragnosis_sync::NegotiatedSurfaces = Default::default();
    // Runtime peer observability (hub role): who actually checked in, when, how much.
    // Set on a hub: the live admission directory, published so the console can show who is in.
    let mut admitted: Option<Arc<supragnosis_sync::http::PeerDirectory>> = None;
    let peer_registry = Arc::new(supragnosis_sync::http::PeerRegistry::default());
    if let Some(srv) = &fc.server {
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
        // Federated recall: remote peers search THIS node's full recall surface (hybrid when the
        // embedder is present), not just the store's keyword path.
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
        // The agent surface (docs/remote-server.md Section 4): MCP for principals on this listener.
        // Mounted whenever there is a listener - principals added later are admitted without a
        // restart - and counted toward the bind rule. Knowledge another node originated is served
        // only with its consent (R5), which nodes give on their sync rounds and the book keeps.
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
                principal::servable(engine.clone(), node.node_id().to_string(), consented),
                events,
            ),
            admitted: srv.principals.len(),
        };
        if !srv.principals.is_empty() {
            tracing::info!(
                principals = srv.principals.len(),
                "agent surface: MCP for principals at /mcp on the sync listener"
            );
        }
        let hooks = supragnosis_sync::http::Hooks {
            on_applied: Some(on_applied),
            on_activity: Some(on_activity),
            on_search: Some(on_search),
            peer_registry: Some(peer_registry.clone()),
            extra: Some(extra),
            on_consent: Some(on_consent),
        };
        admitted = Some(spawn_sync_server(engine.store(), node.clone(), srv.clone(), hooks)?);
    }
    let mut origin_keys = trusted_origin_keys(&fc.sync);
    origin_keys.insert(node.node_id().to_string(), node.public_key_hex());
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
            serve_workspaces,
            servers: links,
            config_notes,
            surfaces: surfaces.clone(),
            insecure_tls: fc.sync.insecure_tls,
            origin_keys,
            // Only a hub (server role) observes peers; a client-only node reports none.
            peer_registry: fc.server.is_some().then_some(peer_registry),
        })),
        narrow,
    ))
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
                let mut ws_json = Vec::new();
                let mut diff = supragnosis_sync::SurfaceDiff::default();
                let mut negotiated_at = None;
                if let Ok(client) = supragnosis_sync::http::SyncClient::new(
                    server,
                    &link.auth_token,
                    sync.insecure_tls,
                )
                .map(|c| c.with_serve(sync.serve_set().0))
                {
                    match client.ping().await {
                        Ok(p) => {
                            healthy = true;
                            version = Some(p.version);
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

/// Starts the federation sync API (the ONLY surface allowed to bind non-loopback - and only with
/// TLS + a non-empty allowlist, F10). A misconfigured [server] section fails daemon startup loudly
/// (P5) instead of silently running without the role.
fn spawn_sync_server(
    store: Arc<dyn AssertionStore>,
    node: Arc<supragnosis_sync::SyncNode>,
    srv: fed::ServerSection,
    hooks: supragnosis_sync::http::Hooks,
) -> Result<Arc<supragnosis_sync::http::PeerDirectory>> {
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
    // Admission is created here and handed BOTH to the server and back to the caller, so who may
    // connect stops being a startup snapshot: the returned handle is what a management surface
    // changes, and the running server reads it per request.
    let peers = Arc::new(sync_http::PeerDirectory::new(
        srv.allowlist,
        node.node_id(),
        &node.public_key_hex(),
    ));
    let serving = peers.clone();
    tokio::spawn(async move {
        if let Err(e) = sync_http::serve(store, node, listen, tls, serving, hooks).await {
            tracing::error!(error = %e, "federation sync API terminated");
        }
    });
    Ok(peers)
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
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let store = engine.store();
        let mut keys = trusted_origin_keys(&fc.sync);
        keys.insert(node.node_id().to_string(), node.public_key_hex());
        for link in &links {
            let server = &link.url;
            let client = supragnosis_sync::http::SyncClient::new(
                server,
                &link.auth_token,
                fc.sync.insecure_tls,
            )?
            .with_serve(fc.sync.serve_set().0);
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
    let (config, _) = client.config(&env);
    let before = client.entry(&env);
    if a.remove {
        if before == Entry::None {
            println!("{} has no supragnosis entry - nothing to remove", client.name());
            return Ok(());
        }
        connect_remove(client, &env)?;
        println!("removed the supragnosis entry from {}", client.name());
        return Ok(());
    }
    match before {
        Entry::Bridge => {
            println!("{} is already connected through the bridge", client.name());
            return Ok(());
        }
        Entry::Unknown => anyhow::bail!(
            "{} could not be read - not touching a file this cannot parse (client-connect.md C3)",
            config.display()
        ),
        Entry::None => {}
        other if !a.replace => anyhow::bail!(
            "{} already has a supragnosis entry ({}){} - re-run with --replace to switch it to the bridge",
            client.name(),
            other.as_str(),
            if other == Entry::Http { ", which holds a copy of the token" } else { "" }
        ),
        other => {
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
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let edited =
                connect::upsert(&text, section, connect::NAME, &connect::bridge_entry(&program))
                    .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
            if let Some(b) = connect::back_up(&path, client.id(), &env.home, unix_now())? {
                println!("  backup  {}", b.display());
            }
            connect::write_replacing(&path, &edited)
                .with_context(|| format!("writing {}", path.display()))?;
        }
    }
    let after = client.entry(&env);
    if after != Entry::Bridge {
        anyhow::bail!(
            "{} did not take the entry - it reads as {} in {}",
            client.name(),
            after.as_str(),
            config.display()
        );
    }
    println!("connected {} -> {program} bridge", client.name());
    println!("  next    {}", client.next_step());
    Ok(())
}

fn connect_remove(client: connect::Client, env: &connect::Env) -> Result<()> {
    match connect::remove_argv(client) {
        Some(argv) => run_client_cli(client, env, &argv),
        None => {
            let (path, section) = client.config(env);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let Some(edited) = connect::remove(&text, section, connect::NAME)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
            else {
                return Ok(());
            };
            if let Some(b) = connect::back_up(&path, client.id(), &env.home, unix_now())? {
                println!("  backup  {}", b.display());
            }
            connect::write_replacing(&path, &edited)
                .with_context(|| format!("writing {}", path.display()))
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
            serde_json::json!({
                "id": c.id(), "name": c.name(), "installed": installed,
                "entry": entry.as_str(), "config": c.config(env).0,
                "via": match c.via() { connect::Via::Cli => "cli", connect::Via::File => "file" },
            })
        })
        .collect();
    serde_json::json!({ "schema": JSON_SCHEMA, "program": program, "clients": clients })
}

fn connect_list(env: &connect::Env, program: &str, json: bool) -> Result<()> {
    use connect::{Entry, CLIENTS};
    let rows: Vec<_> = CLIENTS
        .iter()
        .map(|c| (*c, c.installed(env), if c.installed(env) { c.entry(env) } else { Entry::None }))
        .collect();
    if json {
        println!("{}", connect_document(env, program));
        return Ok(());
    }
    println!(
        "AI apps - connect one with `supragnosis connect <app>`, or from the Supragnosis menu"
    );
    for (c, installed, entry) in rows {
        let state = match (installed, entry) {
            (false, _) => "not installed".to_string(),
            (true, Entry::None) => "not connected".to_string(),
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
            (true, Entry::Unknown) => format!("its config could not be read ({})", c.config(env).0.display()),
        };
        println!("  {:<15} {:<15} {state}", c.id(), c.name());
    }
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

/// `supragnosis principal` (docs/remote-server.md Section 4.2). Edits `[server]` in supragnosis.toml
/// in place - comments and layout survive - and the running hub follows the edit on its next request.
fn principal_cmd(cmd: PrincipalCmd) -> Result<()> {
    let path = fed::config_path();
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!("{} - principals belong to a hub's [server] section", path.display())
    })?;
    match cmd {
        PrincipalCmd::Add { name, read, write } => {
            let (edited, credential) = principal::add(&text, &name, &read, &write)?;
            connect::write_replacing(&path, &edited)
                .with_context(|| format!("writing {}", path.display()))?;
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
            Ok(())
        }
        PrincipalCmd::Remove { name } => {
            let edited = principal::remove(&text, &name)?;
            connect::write_replacing(&path, &edited)
                .with_context(|| format!("writing {}", path.display()))?;
            println!("revoked {name} - its credential stops working at its next request");
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
                    for c in
                        reqwest::Certificate::from_pem_bundle(&pem).map_err(|e| e.to_string())?
                    {
                        b = b.add_root_certificate(c);
                    }
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
        if let Err(e) = supragnosis_viz::serve(engine, listener, events, fed, narrow).await {
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
async fn serve_http_daemon(
    engine: Arc<Engine>,
    sync_ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
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
    let router = mcp_router(engine, sync_ctx, token);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind MCP daemon at {addr}"))?;
    tracing::info!(%host, %workspace, %session, %addr, auth, "supragnosis / MCP streamable-http daemon: http://{addr}/mcp");
    axum::serve(listener, router).await?;
    Ok(())
}

/// The daemon's MCP router: the streamable-HTTP service behind its guards. Separate from the bind
/// so the bridge's tests can serve exactly what the daemon serves (client-connect.md C4).
fn mcp_router(
    engine: Arc<Engine>,
    sync_ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    token: Option<String>,
) -> axum::Router {
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
/// rejected - the MCP surface is the local trust surface; remote MCP access is not a supported
/// topology (federation is the sync surface). A hostname (localhost) is not accepted because it
/// would require DNS resolution (removing ambiguity). This TCP port is the last one standing: the
/// viewer already serves over a unix socket, and this bind follows once MCP clients can reach the
/// daemon through the stdio proxy shim.
fn parse_loopback_addr(s: &str) -> Result<std::net::SocketAddr> {
    let addr: std::net::SocketAddr = s.trim().parse().with_context(|| {
        format!("invalid MCP bind address: {s:?} - must be in IP:port form (e.g. 127.0.0.1:7373)")
    })?;
    if !addr.ip().is_loopback() {
        anyhow::bail!(
            "MCP bind address {addr} is not loopback - refusing (Principle 17: knowledge \
             sovereignty). Use 127.0.0.1:<port>; network sharing goes through the sync surface \
             (TLS + allowlist), never the MCP port"
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
fn live_pidfile() -> Option<i32> {
    read_pid().filter(|p| pid_alive(*p) && pid_is_supragnosis(*p))
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
    if read_pid().is_some() && live_pidfile().is_none() {
        let _ = std::fs::remove_file(pid_path());
    }
}

/// The running daemon's version, from the viewer's `/api/about` over its unix socket. `None` when
/// the socket does not answer - and then the version is unknown, never assumed (Section 5).
#[cfg(unix)]
fn running_version() -> Option<String> {
    lifecycle::parse_about_version(&viz_get("/api/about")?)
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
    use std::io::{Read, Write};
    let sock = std::env::var("SUPRAGNOSIS_VIZ_SOCK")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_viz_sock);
    let mut s = std::os::unix::net::UnixStream::connect(sock).ok()?;
    let limit = Some(std::time::Duration::from_millis(800));
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
    let plist = canonical_plist_path();
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
    use lifecycle::{Manager, Situation};
    clear_stale_pidfile();
    match lifecycle::classify(&observe()) {
        Situation::Stopped => {
            println!("not running.");
            Ok(())
        }
        Situation::One(Manager::Pidfile { pid }) => stop_pidfile(pid as i32),
        Situation::One(Manager::Launchd(job)) => launchd_bootout(&job),
        Situation::Conflict(managers) => anyhow::bail!(lifecycle::conflict_message(&managers)),
        Situation::Unrecognized => anyhow::bail!(
            "a daemon is responding on {} but no known manager runs it (no pidfile, no known launchd label) - stop it via its own supervisor.",
            status_http_addr()
        ),
    }
}

#[cfg(unix)]
fn restart(cfg: Config) -> Result<()> {
    use lifecycle::{Manager, Situation};
    clear_stale_pidfile();
    match lifecycle::classify(&observe()) {
        // Stopped, but the canonical job is installed (a `stop` unloaded it): reload THAT job. Starting
        // a pidfile daemon here would leave the login item installed beside it - two owners again
        // at the next login (L1).
        Situation::Stopped if canonical_plist_state() != "absent" => launchd_bootstrap_canonical(),
        // Nothing installed and nothing running - start a fresh self-managed daemon.
        Situation::Stopped => start(cfg),
        Situation::One(Manager::Pidfile { pid }) => {
            stop_pidfile(pid as i32)?;
            std::thread::sleep(std::time::Duration::from_millis(400)); // wait for the port to release
            start(cfg)
        }
        Situation::One(Manager::Launchd(job)) => {
            launchd_kickstart(&job)?;
            await_daemon(job.label)
        }
        Situation::Conflict(managers) => anyhow::bail!(lifecycle::conflict_message(&managers)),
        Situation::Unrecognized => anyhow::bail!(
            "something no known manager runs is serving {} or holding the store (no pidfile, no known launchd label) - cannot restart it from here.\n{}",
            status_http_addr(),
            lifecycle::UNRECOGNIZED_HOLDER
        ),
    }
}

/// After a launchd restart, wait briefly for the new process and say which version it serves - a
/// restart's whole point after an upgrade is the version, and "restarted" alone does not show it.
#[cfg(unix)]
/// L4: after loading or restarting a launchd job, the job counts as up only when the daemon answers.
/// It counts as failed when launchd shows it failing - no process and a non-zero last exit - for
/// three seconds running: a KeepAlive job crash-looping on the store lock sits like that between
/// respawns, while one being restarted shows its previous exit only for a moment. A slow start that
/// is still running is reported as slow, not failed: a daemon repaying owed projections before it
/// binds (crash-recovery.md K3) can take a while.
#[cfg(target_os = "macos")]
fn await_daemon(label: &str) -> Result<()> {
    let here = env!("CARGO_PKG_VERSION");
    let tick = std::time::Duration::from_millis(250);
    let mut failing_for = 0u32;
    for _ in 0..120 {
        std::thread::sleep(tick);
        if let Some(v) = running_version() {
            match lifecycle::drift(Some(&v), here) {
                lifecycle::Drift::Differs { running, here } => println!(
                    "  now serving {running}, but this binary is {here} - the job runs a different binary than this one"
                ),
                _ => println!("  now serving {v}"),
            }
            return Ok(());
        }
        let job = launchd_jobs().into_iter().find(|j| j.label == label);
        let failing = job
            .as_ref()
            .is_some_and(|j| j.pid.is_none() && j.last_exit.is_some_and(|c| c != 0));
        failing_for = if failing { failing_for + 1 } else { 0 };
        if failing_for >= 12 {
            let code = job.and_then(|j| j.last_exit).unwrap_or_default();
            anyhow::bail!(
                "{label} is loaded but not running - it exited with status {code} and launchd is retrying it. The reason is in {}",
                log_dir().join("supragnosis.err.log").display()
            );
        }
    }
    println!("  still starting - `supragnosis status` shows the version once the daemon answers");
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn await_daemon(_label: &str) -> Result<()> {
    Ok(())
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
    plist: std::path::PathBuf,
    plist_state: &'static str,
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
        "store": f.health,
        "server": f.server,
        "service": {
            "label": lifecycle::CANONICAL_LABEL,
            "plist": f.plist,
            "state": f.plist_state,
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
            plist: canonical_plist_path(),
            plist_state: canonical_plist_state(),
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
            println!("running (unrecognized manager - no pidfile, no known launchd label)");
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
    match canonical_plist_state() {
        "generated" => println!("  login   starts at login ({})", lifecycle::CANONICAL_LABEL),
        "hand_written" => println!(
            "  login   starts at login ({}, hand-written plist)",
            lifecycle::CANONICAL_LABEL
        ),
        _ => println!("  login   not installed - supragnosis service install"),
    }
    if matches!(situation, Situation::One(_)) {
        println!("  control supragnosis restart | supragnosis stop");
    }
    Ok(())
}

// --- The canonical LaunchAgent (docs/daemon-lifecycle.md Section 4) --------------------------

/// Where the canonical plist lives, and what kind of file is there now.
#[cfg(unix)]
fn canonical_plist_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", lifecycle::CANONICAL_LABEL))
}

/// absent | generated | hand_written - the state the app's Start at Login item reflects.
#[cfg(unix)]
fn canonical_plist_state() -> &'static str {
    match std::fs::read_to_string(canonical_plist_path()) {
        Err(_) => "absent",
        Ok(text) if lifecycle::is_generated(&text) => "generated",
        Ok(_) => "hand_written",
    }
}

#[cfg(target_os = "macos")]
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
    let dst = lifecycle::moved_aside_path(&home, label, unix_secs());
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
#[cfg(target_os = "macos")]
fn wait_until_released(store: Option<&std::path::Path>) -> bool {
    for _ in 0..100 {
        if !port_open(&status_http_addr()) && !store.is_some_and(supragnosis_store::redb_in_use) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// A Homebrew job is retired by Homebrew: the plist is its file, and only `brew services stop`
/// removes it from LaunchAgents (a bootout alone would let it load again at the next login).
#[cfg(target_os = "macos")]
fn brew_services_stop(token: &str) -> Result<()> {
    let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
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

#[cfg(target_os = "macos")]
fn service_install(args: ServiceInstallArgs) -> Result<()> {
    use lifecycle::{LabelKind, Manager};
    let home = std::env::var("HOME").context("HOME is not set")?;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_plist_path();
    let state = canonical_plist_state();

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
    let others = lifecycle::plan_install(&observed, args.take_over, state == "hand_written")
        .map_err(|e| anyhow::anyhow!(e))?;

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
    await_daemon(lifecycle::CANONICAL_LABEL)
}

#[cfg(target_os = "macos")]
fn service_uninstall() -> Result<()> {
    use lifecycle::LabelKind;
    let uid = launchd_uid().context("could not determine uid (id -u) for the launchd domain")?;
    let plist = canonical_plist_path();
    let state = canonical_plist_state();
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

#[cfg(not(target_os = "macos"))]
fn service_install(_args: ServiceInstallArgs) -> Result<()> {
    anyhow::bail!("service install manages a macOS LaunchAgent; on Linux use the systemd user unit in deploy/systemd/")
}
#[cfg(not(target_os = "macos"))]
fn service_uninstall() -> Result<()> {
    anyhow::bail!("service uninstall manages a macOS LaunchAgent; on Linux use the systemd user unit in deploy/systemd/")
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
        /// Accept a self-signed hub certificate (internal VM) - content authenticity stays with the
        /// event signatures (F6), this only affects transport privacy against an active MITM.
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
                    .map(|e| supragnosis_sync::ServerLink {
                        url: e.url.clone(),
                        auth_token: e.auth_token.clone(),
                    })
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
                .map(|url| supragnosis_sync::ServerLink {
                    url: url.clone(),
                    auth_token: token.clone(),
                })
                .collect();
            (links, notes)
        }
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
        let path = config_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(cfg))
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
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut doc: toml_edit::DocumentMut =
            text.parse().with_context(|| format!("parsing {}", path.display()))?;

        let entries = doc
            .get_mut("server")
            .and_then(|s| s.get_mut("allowlist"))
            .and_then(|a| a.as_array_of_tables_mut())
            .ok_or_else(|| anyhow::anyhow!("no [[server.allowlist]] in {}", path.display()))?;

        let entry = entries
            .iter_mut()
            .find(|t| t.get("node_id").and_then(|v| v.as_str()) == Some(node_id))
            .ok_or_else(|| anyhow::anyhow!("no allowlist entry for node_id {node_id}"))?;

        let current: Vec<String> = entry
            .get("shared_workspaces")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        let widened: Vec<&String> = keep.iter().filter(|w| !current.contains(w)).collect();
        if !widened.is_empty() {
            anyhow::bail!(
                "refusing to widen: {node_id} does not currently share {widened:?} (it shares \
                 {current:?}). This surface only narrows - grant a workspace by editing {}",
                path.display()
            );
        }

        let mut arr = toml_edit::Array::new();
        for w in keep {
            arr.push(w.as_str());
        }
        entry["shared_workspaces"] = toml_edit::value(arr);
        std::fs::write(&path, doc.to_string())
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(keep.to_vec())
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

            // Both shapes at once is refused, not resolved by precedence. A precedence rule here
            // would decide silently which credential a host is given, which is exactly the class of
            // configuration failure P5 asks to be loud (negotiated-surface.md N8).
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

        /// Narrowing rewrites only the one array it was asked to change, and refuses to widen.
        ///
        /// Comment preservation is not cosmetic here. `supragnosis.toml` is a hand-maintained
        /// declaration - the hub config in the author's own deployment carries paragraphs explaining
        /// why it has no `[sync]` section - and a console that reformatted it would make operators
        /// stop trusting the console. Serializing the parsed struct back out would do exactly that,
        /// which is why this edits the document rather than the model.
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
listen = "0.0.0.0:7420"   # F10: non-loopback needs TLS + a non-empty allowlist

# ashons-MacBook-Air
[[server.allowlist]]
node_id = "peer-a"
public_key_hex = "aa"
bearer_hash = "hh"
shared_workspaces = ["alpha", "beta", "gamma"]
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

            // A subset is applied.
            let kept = narrow_shared_workspaces("peer-a", &["alpha".into()]).expect("narrow");
            assert_eq!(kept, vec!["alpha".to_string()]);
            let after = std::fs::read_to_string(&path).expect("read");
            assert!(
                after.contains(r#"shared_workspaces = ["alpha"]"#),
                "the array narrowed: {after}"
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
            assert!(after.contains("shared_workspaces = []"), "{after}");

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

    #[cfg(unix)]
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
            health: Some(serde_json::json!({"owed_projections": 0, "last_recovery": null})),
            server: serde_json::json!({"name": "lab", "url": "https://lab:7420/mcp", "remote": true}),
            plist: std::path::PathBuf::from("/x/com.supragnosis.daemon.plist"),
            plist_state: "generated",
        });
        assert_eq!(doc["schema"], JSON_SCHEMA);
        same_shape(&example("status"), &doc, "status").unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn connect_json_matches_its_example() {
        let env =
            connect::Env { home: std::env::temp_dir().join("no-such-home"), path: Vec::new() };
        let doc = connect_document(&env, "/x/supragnosis");
        assert_eq!(doc["schema"], JSON_SCHEMA);
        same_shape(&example("connect"), &doc, "connect").unwrap_or_else(|e| panic!("{e}"));
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
