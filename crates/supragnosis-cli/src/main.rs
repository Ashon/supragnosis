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
    /// How this node is used: calls by MCP function and workspace, by app, and what the Claude
    /// Code hook gave and asked, from the local usage log (docs/usage.md)
    Usage(usage_cli::UsageArgs),
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
mod daemon_ctl;
mod embedding;
mod engine_setup;
mod fed;
mod managers;
mod pairing_cli;
mod serve;
mod service;
mod setup_cli;
mod store_cmds;
mod sync_wiring;
mod usage_cli;

use config::*;
use connect_cli::*;
use daemon_ctl::*;
use embedding::*;
use engine_setup::*;
use managers::*;
use pairing_cli::*;
use serve::*;
use service::*;
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
        Cmd::Usage(a) => usage_cli::usage_cmd(a),
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

/// main.rs and the modules split out of it, as text, for the tests that read the CLI's own code.
#[cfg(test)]
const MAIN_SOURCES: &[&str] = &[
    include_str!("main.rs"),
    include_str!("config.rs"),
    include_str!("embedding.rs"),
    include_str!("engine_setup.rs"),
    include_str!("sync_wiring.rs"),
    include_str!("usage_cli.rs"),
    include_str!("store_cmds.rs"),
    include_str!("connect_cli.rs"),
    include_str!("setup_cli.rs"),
    include_str!("pairing_cli.rs"),
    include_str!("serve.rs"),
    include_str!("managers.rs"),
    include_str!("daemon_ctl.rs"),
    include_str!("service.rs"),
    include_str!("fed.rs"),
];

#[cfg(test)]
mod json_contract;
