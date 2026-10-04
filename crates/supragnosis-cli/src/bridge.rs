//! `supragnosis bridge` - a client's stdio, relayed to the local daemon (docs/client-connect.md
//! Section 3).
//!
//! Every client speaks MCP over stdio, but the stdio server this binary ships opens the store, and
//! the store admits one writer - the daemon. The bridge is a stdio server that owns nothing: each
//! line the client writes is POSTed to the daemon's streamable-HTTP endpoint, and each message the
//! daemon answers with is written back as a line. It does not read what it relays (C4), never
//! opens the store and never starts a daemon (C1), and reads the bearer token from its 0600 file
//! on every request, so no client configuration has to hold it (C2).
//!
//! The daemon sends nothing unsolicited today - every server message answers a client request - so
//! the bridge relays request/response pairs and opens no standalone GET stream. A server-initiated
//! notification added to the daemon would need one.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

/// What the bridge relays to, and how long it waits for a daemon that is not answering yet.
pub struct Config {
    /// The daemon's MCP endpoint, e.g. `http://127.0.0.1:7373/mcp`.
    pub url: String,
    /// Reads the bearer token, on every request - so a regenerated token is picked up without
    /// restarting the client that launched the bridge.
    pub token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// How long one request keeps retrying an unreachable daemon before answering with an error.
    /// A login job starting, or a restart repaying owed projections, takes seconds.
    pub wait: Duration,
    /// A private CA's PEM bundle, for a remote server whose certificate it issued (remote-server.md
    /// Section 3). The system roots are trusted either way; nothing turns verification off.
    pub ca_pem: Option<Vec<u8>>,
    /// What a person can do when nothing answers - start the local daemon, or check the network.
    pub unreachable_hint: String,
    /// Where the credential is read from, named when the server refuses it.
    pub token_source: String,
}

/// The fix for an unreachable local daemon, named in every error the bridge gives instead of an
/// answer.
pub const NOT_RUNNING: &str = "the supragnosis daemon is not running - open the Supragnosis app, \
     or turn on Start at Login in its menu (or run `supragnosis service install`)";

/// The session the bridge holds with the daemon on the client's behalf, and the client's own
/// handshake, kept so a new session can be opened when the daemon restarts.
#[derive(Default)]
struct Session {
    id: Option<String>,
    protocol: Option<String>,
    initialize: Option<Value>,
    initialized: Option<Value>,
    /// Bumped on every re-initialization, so concurrent requests that all saw the old session
    /// expire re-initialize once between them rather than once each.
    generation: u64,
}

struct Bridge {
    cfg: Config,
    http: reqwest::Client,
    session: Mutex<Session>,
    /// Serializes re-initialization: one replay of the handshake at a time.
    reinit: Mutex<()>,
}

enum Failure {
    /// Nothing accepted the connection within the wait.
    Unreachable,
    /// The daemon no longer knows this session - it restarted.
    SessionGone,
    /// The daemon refused the token (or none was found).
    Unauthorized,
    Http(u16, String),
}

/// Runs the bridge until `input` ends - how a client stops a stdio server.
pub async fn run<R, W>(cfg: Config, input: R, output: W) -> anyhow::Result<()>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = reqwest::Client::builder();
    if let Some(pem) = &cfg.ca_pem {
        for cert in reqwest::Certificate::from_pem_bundle(pem)? {
            builder = builder.add_root_certificate(cert);
        }
    }
    let http = builder
        .connect_timeout(Duration::from_secs(2))
        // Loopback only, and a proxy configured for the person's browsing must not see this.
        .no_proxy()
        // A fresh connection per request. A pooled one outlives a daemon restart, and a request sent
        // on it fails in a way that cannot tell "never delivered" from "delivered, then the daemon
        // died" - retrying the second would run a write twice. A refused connect is unambiguous, and
        // on loopback it costs nothing worth saving.
        .pool_max_idle_per_host(0)
        .build()?;
    let bridge = Arc::new(Bridge {
        cfg,
        http,
        session: Mutex::new(Session::default()),
        reinit: Mutex::new(()),
    });

    // One writer, so concurrent replies never interleave inside a line.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut output = output;
        while let Some(line) = rx.recv().await {
            if output.write_all(line.as_bytes()).await.is_err()
                || output.write_all(b"\n").await.is_err()
                || output.flush().await.is_err()
            {
                break;
            }
        }
    });

    let mut lines = BufReader::new(input).lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(error_reply(&Value::Null, -32700, &format!("parse error: {e}")));
                continue;
            }
        };
        // The handshake is relayed in order - a client sends nothing else before the initialize
        // answer, and `initialized` must reach the daemon before the requests that follow it.
        if is_method(&msg, "initialize") || is_method(&msg, "notifications/initialized") {
            relay(&bridge, msg, &tx).await;
        } else {
            let (bridge, tx) = (bridge.clone(), tx.clone());
            tasks.spawn(async move { relay(&bridge, msg, &tx).await });
        }
    }
    while tasks.join_next().await.is_some() {}
    bridge.close().await;
    drop(tx);
    let _ = writer.await;
    Ok(())
}

async fn relay(bridge: &Bridge, msg: Value, tx: &mpsc::UnboundedSender<String>) {
    let id = msg.get("id").cloned();
    let is_request = msg.get("method").is_some() && id.is_some();
    let outcome = if is_method(&msg, "initialize") {
        bridge.initialize(msg).await
    } else {
        if is_method(&msg, "notifications/initialized") {
            bridge.session.lock().await.initialized = Some(msg.clone());
        }
        bridge.forward(msg).await
    };
    match outcome {
        Ok(replies) => {
            for r in replies {
                let _ = tx.send(r);
            }
        }
        Err(f) if is_request => {
            let id = id.unwrap_or(Value::Null);
            let _ = tx.send(error_reply(&id, -32000, &describe(&f, &bridge.cfg)));
        }
        // A notification or a response has nowhere to carry an error back; stderr is the client's
        // log for its stdio server.
        Err(f) => eprintln!("supragnosis bridge: {}", describe(&f, &bridge.cfg)),
    }
}

impl Bridge {
    async fn initialize(&self, msg: Value) -> Result<Vec<String>, Failure> {
        let (replies, session) = self.post(&msg, None, None).await?;
        let protocol = replies.iter().find_map(|r| protocol_of(r));
        let mut s = self.session.lock().await;
        s.initialize = Some(msg);
        s.id = session;
        s.protocol = protocol;
        Ok(replies)
    }

    /// Relays one message on the current session. A session the daemon no longer knows means it
    /// restarted: the client's own handshake is replayed for a new one, and the message is sent
    /// once more.
    async fn forward(&self, msg: Value) -> Result<Vec<String>, Failure> {
        let (id, protocol, generation) = {
            let s = self.session.lock().await;
            (s.id.clone(), s.protocol.clone(), s.generation)
        };
        match self.post(&msg, id.as_deref(), protocol.as_deref()).await {
            Err(Failure::SessionGone) => {
                self.reopen(generation).await?;
                let (id, protocol) = {
                    let s = self.session.lock().await;
                    (s.id.clone(), s.protocol.clone())
                };
                self.post(&msg, id.as_deref(), protocol.as_deref()).await.map(|(r, _)| r)
            }
            other => other.map(|(r, _)| r),
        }
    }

    async fn reopen(&self, seen: u64) -> Result<(), Failure> {
        let _one_at_a_time = self.reinit.lock().await;
        let (initialize, initialized) = {
            let s = self.session.lock().await;
            if s.generation != seen {
                return Ok(()); // another request already opened the new session
            }
            (s.initialize.clone(), s.initialized.clone())
        };
        let Some(initialize) = initialize else {
            // Nothing to replay: the client never initialized, so the daemon is right to refuse.
            return Err(Failure::SessionGone);
        };
        // The answer to a replayed handshake is the bridge's, not the client's - the client already
        // has its capabilities, and the daemon's do not change across a restart of the same build.
        let (replies, session) = self.post(&initialize, None, None).await?;
        let protocol = replies.iter().find_map(|r| protocol_of(r));
        {
            let mut s = self.session.lock().await;
            s.id = session;
            s.protocol = protocol;
            s.generation += 1;
        }
        if let Some(n) = initialized {
            let (id, protocol) = {
                let s = self.session.lock().await;
                (s.id.clone(), s.protocol.clone())
            };
            self.post(&n, id.as_deref(), protocol.as_deref()).await?;
        }
        Ok(())
    }

    /// One POST, retried while nothing accepts the connection, up to the configured wait.
    async fn post(
        &self,
        msg: &Value,
        session: Option<&str>,
        protocol: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>), Failure> {
        let deadline = tokio::time::Instant::now() + self.cfg.wait;
        loop {
            let mut req = self
                .http
                .post(&self.cfg.url)
                .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(msg.to_string());
            if let Some(t) = (self.cfg.token)() {
                req = req.bearer_auth(t);
            }
            if let Some(s) = session {
                req = req.header("Mcp-Session-Id", s);
            }
            if let Some(p) = protocol {
                req = req.header("MCP-Protocol-Version", p);
            }
            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) if e.is_connect() || e.is_timeout() => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(Failure::Unreachable);
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                Err(e) => return Err(Failure::Http(0, e.to_string())),
            };
            let status = resp.status().as_u16();
            let new_session = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let is_sse = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|c| c.starts_with("text/event-stream"));
            let body = resp.text().await.unwrap_or_default();
            return match status {
                200 if is_sse => Ok((sse_messages(&body), new_session)),
                200 => Ok((one_line(&body).into_iter().collect(), new_session)),
                202 => Ok((Vec::new(), new_session)),
                404 if session.is_some() => Err(Failure::SessionGone),
                401 | 403 => Err(Failure::Unauthorized),
                _ => Err(Failure::Http(status, body.chars().take(200).collect())),
            };
        }
    }

    /// Tells the daemon the session is over, so it does not hold it until it times out.
    async fn close(&self) {
        let id = self.session.lock().await.id.clone();
        if let Some(id) = id {
            let mut req = self.http.delete(&self.cfg.url).header("Mcp-Session-Id", id);
            if let Some(t) = (self.cfg.token)() {
                req = req.bearer_auth(t);
            }
            let _ = req.timeout(Duration::from_secs(1)).send().await;
        }
    }
}

fn is_method(msg: &Value, method: &str) -> bool {
    msg.get("method").and_then(Value::as_str) == Some(method)
}

fn protocol_of(reply: &str) -> Option<String> {
    let v: Value = serde_json::from_str(reply).ok()?;
    v.pointer("/result/protocolVersion")?.as_str().map(str::to_string)
}

/// The JSON-RPC messages in a server-sent-events body, each re-encoded onto one line - a stdio
/// transport is newline-delimited, and a `data:` field may span lines.
pub fn sse_messages(body: &str) -> Vec<String> {
    let body = body.replace("\r\n", "\n");
    body.split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|d| d.strip_prefix(' ').unwrap_or(d))
                .collect();
            if data.is_empty() {
                return None;
            }
            one_line(&data.join("\n"))
        })
        .collect()
}

/// A JSON body compacted onto one line; `None` for an empty one (a priming event, say).
fn one_line(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    Some(match serde_json::from_str::<Value>(body) {
        Ok(v) => v.to_string(),
        Err(_) => body.replace('\n', " "),
    })
}

fn error_reply(id: &Value, code: i64, message: &str) -> String {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        .to_string()
}

fn describe(f: &Failure, cfg: &Config) -> String {
    let url = &cfg.url;
    match f {
        Failure::Unreachable => format!("nothing answers at {url}: {}", cfg.unreachable_hint),
        Failure::SessionGone => "the server restarted before this client initialized".into(),
        Failure::Unauthorized => format!(
            "{url} refused the credential in {} - it is read from that file on every request, so \
             a server started under another account, a replaced credential, or a revoked one is \
             the likely cause",
            cfg.token_source
        ),
        Failure::Http(0, e) => format!("could not reach {url}: {e}"),
        Failure::Http(code, body) => format!("{url} answered {code}: {body}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_engine::Engine;
    use supragnosis_store::InMemoryStore;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    #[test]
    fn server_sent_events_become_one_message_per_line() {
        let body = "id: 0\nretry: 3000\ndata: \n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\n\
                    data:  \"id\":1,\"result\":{}}\n\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"x\"}\r\n\r\n";
        assert_eq!(
            sse_messages(body),
            vec![
                r#"{"id":1,"jsonrpc":"2.0","result":{}}"#.to_string(),
                r#"{"jsonrpc":"2.0","method":"x"}"#.to_string()
            ]
        );
    }

    /// The daemon's own router on an ephemeral port, as `serve` builds it.
    async fn daemon(
        at: Option<std::net::SocketAddr>,
        store: Arc<InMemoryStore>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let engine = Arc::new(Engine::new(store, "h", "ws"));
        let router = crate::mcp_router(engine, None, Some("tok".to_string()));
        let listener = tokio::net::TcpListener::bind(at.unwrap_or(([127, 0, 0, 1], 0).into()))
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (addr, handle)
    }

    /// A client on the other end of the bridge: writes lines, reads lines.
    struct Client {
        to_bridge: tokio::io::DuplexStream,
        from_bridge: tokio::io::Lines<BufReader<tokio::io::DuplexStream>>,
    }

    impl Client {
        fn start(url: String, wait: Duration, token: &'static str) -> Client {
            let (to_bridge, bridge_in) = tokio::io::duplex(1 << 16);
            let (bridge_out, from_bridge) = tokio::io::duplex(1 << 16);
            let cfg = Config {
                url,
                token: Arc::new(move || Some(token.to_string())),
                wait,
                ca_pem: None,
                unreachable_hint: NOT_RUNNING.to_string(),
                token_source: "~/.supragnosis/mcp.token".to_string(),
            };
            tokio::spawn(run(cfg, bridge_in, bridge_out));
            Client { to_bridge, from_bridge: BufReader::new(from_bridge).lines() }
        }
        async fn send(&mut self, v: Value) {
            self.to_bridge.write_all(format!("{v}\n").as_bytes()).await.expect("write");
        }
        async fn recv(&mut self) -> Value {
            let line = tokio::time::timeout(Duration::from_secs(10), self.from_bridge.next_line())
                .await
                .expect("a reply in time")
                .expect("read")
                .expect("a line");
            serde_json::from_str(&line).expect("json")
        }
        async fn handshake(&mut self) -> Value {
            self.send(serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
                .await;
            let init = self.recv().await;
            self.send(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
                .await;
            init
        }
        async fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
            self.send(serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await;
            self.recv().await
        }
    }

    /// C4: through the bridge a client sees the daemon's surface - its tool list, and a tool call
    /// that lands in the daemon's store.
    #[tokio::test]
    async fn the_bridge_relays_the_daemons_surface_unchanged() {
        let store = Arc::new(InMemoryStore::new());
        let (addr, _d) = daemon(None, store.clone()).await;
        let mut c = Client::start(format!("http://{addr}/mcp"), Duration::from_secs(2), "tok");
        let init = c.handshake().await;
        assert!(init["result"]["capabilities"]["tools"].is_object(), "{init}");

        let tools = c.call(1, "tools/list", serde_json::json!({})).await;
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        assert!(names.contains(&"observe") && names.contains(&"search_knowledge"), "{names:?}");

        let observed = c
            .call(
                2,
                "tools/call",
                serde_json::json!({"name":"observe","arguments":{
                "content":"relayed through the bridge","entities":[{"name":"bridge"}]}}),
            )
            .await;
        assert!(observed["result"].is_object(), "{observed}");
        use supragnosis_core::AssertionStore;
        assert_eq!(
            store.all_observations(None).expect("log").len(),
            1,
            "the call reached the daemon's store"
        );
    }

    /// A daemon restart drops every session. The bridge replays the client's own handshake and
    /// retries, so the client - which cannot know the daemon restarted - gets its answer.
    #[tokio::test]
    async fn a_daemon_restart_is_invisible_through_the_bridge() {
        let store = Arc::new(InMemoryStore::new());
        let (addr, first) = daemon(None, store.clone()).await;
        let mut c = Client::start(format!("http://{addr}/mcp"), Duration::from_secs(5), "tok");
        c.handshake().await;
        assert!(c.call(1, "tools/list", serde_json::json!({})).await["result"].is_object());

        first.abort();
        let _ = first.await;
        let (_, _second) = daemon(Some(addr), store).await;
        let after = c.call(2, "tools/list", serde_json::json!({})).await;
        assert!(after["result"]["tools"].is_array(), "answered after the restart: {after}");
    }

    /// With nothing listening, a request is answered - not left hanging - with an error a person can
    /// act on.
    #[tokio::test]
    async fn the_bridge_says_what_to_do_when_no_daemon_answers() {
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr");
        let mut c = Client::start(format!("http://{free}/mcp"), Duration::from_millis(300), "tok");
        c.send(serde_json::json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}))
            .await;
        let reply = c.recv().await;
        assert_eq!(reply["id"], 0);
        let message = reply["error"]["message"].as_str().expect("an error");
        assert!(message.contains("Start at Login"), "{message}");
    }

    /// A token the daemon refuses is reported as such, naming the file it was read from.
    #[tokio::test]
    async fn a_refused_token_is_reported_with_its_file() {
        let (addr, _d) = daemon(None, Arc::new(InMemoryStore::new())).await;
        let mut c = Client::start(format!("http://{addr}/mcp"), Duration::from_secs(2), "wrong");
        let reply = c.handshake().await;
        let message = reply["error"]["message"].as_str().expect("an error");
        assert!(message.contains("mcp.token"), "{message}");
    }
}
