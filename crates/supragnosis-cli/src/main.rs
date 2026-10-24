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
        let src = concat!(
            include_str!("managers.rs"),
            include_str!("daemon_ctl.rs"),
            include_str!("service.rs"),
            include_str!("fed.rs"),
        );
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
    include_str!("serve.rs"),
    include_str!("managers.rs"),
    include_str!("daemon_ctl.rs"),
    include_str!("service.rs"),
    include_str!("fed.rs"),
];
