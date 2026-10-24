//! The resolved run configuration: `Config`, how `resolve` fills it from the flags, the
//! environment and the defaults, and the default paths it falls back to.

use super::*;

/// Resolved run configuration.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) host: String,
    pub(crate) workspace: String,
    pub(crate) store_kind: String,
    pub(crate) data_dir: String,
    pub(crate) embed_kind: String,
    pub(crate) session: String,
    /// Some = streamable-http daemon, None = stdio.
    pub(crate) http: Option<String>,
    /// Some = accompanied by the live viewer (unix socket path).
    pub(crate) viz: Option<String>,
    /// Whether the streamable-http daemon requires its bearer token (Principle 17). Default on.
    /// Only consulted when `http` is Some - stdio is already confined to the process that spawned it.
    pub(crate) mcp_auth: bool,
}

/// Resolves a Config from RunArgs + environment variables + defaults. When
/// `daemon=true` (start/restart), stdio is meaningless, so http/viz are filled in
/// with their loopback defaults.
pub(crate) fn resolve(a: RunArgs, daemon: bool) -> Config {
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

pub(crate) fn default_data_dir_for(store_kind: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    match store_kind {
        // The pre-0.2 Cozo location, kept only so the un-migrated-store guard knows where to look.
        "cozo" => format!("{home}/.supragnosis/db"),
        _ => format!("{home}/.supragnosis/redb"),
    }
}

/// The redb database file inside a store directory. One file, named rather than derived at each
/// call site, so the migration command and the server cannot disagree about where the store lives.
pub(crate) fn redb_path(data_dir: &str) -> std::path::PathBuf {
    std::path::Path::new(data_dir).join("knowledge.redb")
}

/// Default viewer socket path (the viewer serves HTTP over UDS only - no TCP; the socket file's
/// 0600 mode is the access control).
pub(crate) fn default_viz_sock() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.supragnosis/viz.sock")
}

/// Default embedder kind based on the compiled features. If built with fastembed enabled, that is the default.
pub(crate) fn default_embed_kind() -> &'static str {
    if cfg!(feature = "fastembed") {
        "fastembed"
    } else {
        "none"
    }
}
