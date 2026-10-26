//! Serving: the stdio and streamable-http MCP servers, the viewer socket, the loopback and
//! session guards, and the daemon's bearer token.

use super::*;

/// Whether a URL's host is this machine: `localhost`, or a loopback address in either family.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// A credential file's content, trimmed; `None` when absent or empty.
pub(crate) fn read_secret(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub(crate) fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

/// Actual server run (async). With http, a streamable-http daemon; without it,
/// stdio. With viz, the live viewer is started alongside it in the same process.
pub(crate) async fn run(cfg: Config) -> Result<()> {
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
            if let Some(sock) = cfg.viz.as_deref() {
                let _ = USAGE.set(Arc::new(supragnosis_mcp::UsageLog::at(usage_path(sock))));
            }
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

/// Builds a tokio runtime and runs [`run`] in blocking fashion. Manual construction
/// instead of `#[tokio::main]` so that start's daemonization (fork) can happen
/// **before** the runtime is created (prevents a broken runtime after fork).
pub(crate) fn run_blocking(cfg: Config) -> Result<()> {
    init_tracing();
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(run(cfg))
}

/// Starts the live ontology viewer as an opt-in. A bind/configuration failure is
/// only logged and does not block server startup (the viewer is an auxiliary channel
/// - Principle 21). `events` is the same broadcast Sender as the engine sink.
pub(crate) async fn spawn_viz(
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
pub(crate) struct SyncSurface {
    ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    listener: Arc<std::sync::RwLock<serde_json::Value>>,
}

pub(crate) async fn serve_http_daemon(
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
pub(crate) fn write_listening(
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
pub(crate) async fn sigterm() {
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

/// The usage log of the daemon's MCP surface (docs/usage.md), set once when the daemon serves HTTP
/// with a viewer socket; a router built without it (a test's) records nothing.
static USAGE: std::sync::OnceLock<Arc<supragnosis_mcp::UsageLog>> = std::sync::OnceLock::new();

/// Where the usage log lives: beside the viewer's socket, which is where the daemon keeps what it
/// knows only about this machine (docs/usage.md Section 2). The hook finds it the same way.
pub(crate) fn usage_path(viz_sock: &str) -> std::path::PathBuf {
    std::path::Path::new(viz_sock).with_file_name("usage.jsonl")
}

/// First contact (docs/onboarding.md Section 5): the one log this daemon process keeps of which AI
/// apps talked to it, shared by every MCP session and read by the viewer's `/api/clients`.
pub(crate) fn client_log() -> Arc<supragnosis_mcp::ClientLog> {
    static LOG: std::sync::OnceLock<Arc<supragnosis_mcp::ClientLog>> = std::sync::OnceLock::new();
    LOG.get_or_init(Default::default).clone()
}

/// The daemon's MCP router: the streamable-HTTP service behind its guards. Separate from the bind
/// so the bridge's tests can serve exactly what the daemon serves (client-connect.md C4).
pub(crate) fn mcp_router(
    engine: Arc<Engine>,
    sync_ctx: Option<Arc<supragnosis_mcp::SyncContext>>,
    listener: Option<Arc<std::sync::RwLock<serde_json::Value>>>,
    token: Option<String>,
) -> axum::Router {
    let service = StreamableHttpService::new(
        move || {
            let mut server = SupragnosisServer::new(engine.clone()).with_clients(client_log());
            if let Some(log) = USAGE.get() {
                server = server.with_usage(log.clone());
            }
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
pub(crate) async fn guard_local_origin(
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
pub(crate) fn is_expired_session(status: axum::http::StatusCode, body: &[u8]) -> bool {
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
pub(crate) async fn expired_session_is_not_found(
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
pub(crate) fn parse_loopback_addr(s: &str) -> Result<std::net::SocketAddr> {
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
pub(crate) fn host_hdr_is_loopback(h: &str) -> bool {
    let hostname = if let Some(rest) = h.strip_prefix('[') {
        rest.split_once(']').map(|(a, _)| a).unwrap_or(rest) // IPv6 literal: [::1] or [::1]:port
    } else {
        h.rsplit_once(':').map(|(a, _)| a).unwrap_or(h) // host or host:port
    };
    matches!(hostname, "127.0.0.1" | "localhost" | "::1")
}

/// True if an Origin (`scheme://host[:port]`) names a loopback address.
pub(crate) fn origin_is_loopback(o: &str) -> bool {
    let after = o.split_once("://").map(|(_, r)| r).unwrap_or(o);
    host_hdr_is_loopback(after)
}

// --- MCP daemon single-user confinement (Principle 17) --------------------------------------------

/// The daemon's bearer token file. Mode 0600, in the 0700 `~/.supragnosis` dir - the same access
/// control the viewer socket uses, applied to the one surface that could not use a socket.
pub(crate) fn mcp_token_path() -> std::path::PathBuf {
    fed::fed_base_dir().join("mcp.token")
}

/// Creates `dir` closed to other accounts, and closes it if it was found open. architecture.md puts
/// the token, the node key and the store "in the 0700 ~/.supragnosis dir", and until 2026-10 that
/// was a description of intent: the directory was made with the default mode (0755 here), its store
/// file 0644, so any other local account could copy the whole store and never need the token.
/// Closing the directory closes everything under it at once, whatever mode a file was written with.
pub(crate) fn private_dir(dir: &std::path::Path) -> Result<()> {
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
pub(crate) fn write_secret(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
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
pub(crate) fn load_or_create_mcp_token() -> Result<String> {
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
pub(crate) fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extracts a bearer token from the Authorization header.
pub(crate) fn bearer_of(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Decides one request against the expected token. Split out from the middleware so the decision is
/// testable without standing up a server - the middleware is then only plumbing.
pub(crate) fn token_admits(headers: &axum::http::HeaderMap, expected: &str) -> bool {
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
pub(crate) async fn require_token(
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
