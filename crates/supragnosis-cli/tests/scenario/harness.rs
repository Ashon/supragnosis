//! The scenario harness (docs/e2e.md Section 3): a daemon in a home of its own, an MCP client that
//! speaks to it as an AI app does, and the console over its socket. Every process it starts is
//! stopped when its handle drops (J5), and every wait has a deadline that fails with what it last
//! saw (J3).

#![allow(dead_code)] // each scenario file uses the part of the harness its stories need

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// The binary under test: the one cargo built from this tree (J1).
pub const BIN: &str = env!("CARGO_BIN_EXE_supragnosis");

/// A home of the scenario's own. Removed when it drops, unless SUPRAGNOSIS_SCENARIO_KEEP is set,
/// which leaves it - with each daemon's log - for a failing scenario to be read.
pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    pub fn new(name: &str) -> Home {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sg-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Home { dir }
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        if std::env::var_os("SUPRAGNOSIS_SCENARIO_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Polls `f` until it answers, or panics with what it last saw once `limit` passes (J3).
pub fn wait_for<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Result<T, String>) -> T {
    let deadline = Instant::now() + limit;
    loop {
        match f() {
            Ok(v) => return v,
            Err(last) if Instant::now() > deadline => {
                panic!("{what}: still {last} after {limit:?}")
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The environment a scenario's processes start from (J2): nothing inherited but a coverage
/// profile's destination, then the scenario's own home, socket, embedder and - once a daemon has
/// bound one - its address, so a later `bridge`, `setup` or `status` reaches this daemon and never
/// the person's.
pub fn scenario_env(cmd: &mut Command, home: &Path, sock: &Path, addr: Option<&str>) {
    cmd.env_clear()
        .env("HOME", home)
        .env("SUPRAGNOSIS_VIZ_SOCK", sock)
        .env("SUPRAGNOSIS_EMBED", "hashing")
        .env("SUPRAGNOSIS_STORE", "redb");
    if let Some(p) = std::env::var_os("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", p);
    }
    if let Some(a) = addr {
        cmd.env("SUPRAGNOSIS_HTTP_ADDR", a);
    }
}

/// `supragnosis serve`, in a home, on the address it bound.
pub struct Daemon {
    pub home: PathBuf,
    pub sock: PathBuf,
    pub addr: String,
    pub token: String,
    child: Option<Child>,
}

impl Daemon {
    /// Starts a daemon on a port the system chooses, and waits until it answers.
    pub fn start(home: &Home) -> Daemon {
        let sock = std::env::temp_dir().join(format!(
            "sg-{}-{}.sock",
            std::process::id(),
            home.dir.file_name().unwrap().to_string_lossy().len() + rand_suffix()
        ));
        Daemon::spawn(&home.dir, sock, "127.0.0.1:0")
    }

    /// Starts a daemon again on the home and address the previous one had (A4, A5), retrying while
    /// the port is still held, with a deadline.
    pub fn restart(home: &Home, previous: &Daemon) -> Daemon {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let mut d = Daemon::spawn_unwaited(&home.dir, previous.sock.clone(), &previous.addr);
            match d.wait_up(Duration::from_secs(10), Some(previous.addr.clone())) {
                Ok(()) => return d,
                Err(why) if Instant::now() < deadline => {
                    d.stop();
                    eprintln!("restart retry: {why}");
                    std::thread::sleep(Duration::from_millis(250));
                }
                Err(why) => panic!("the daemon did not come back on {}: {why}", previous.addr),
            }
        }
    }

    fn spawn(home: &Path, sock: PathBuf, addr: &str) -> Daemon {
        let mut d = Daemon::spawn_unwaited(home, sock, addr);
        if let Err(why) = d.wait_up(Duration::from_secs(20), None) {
            panic!("the daemon did not come up: {why}\n{}", d.log());
        }
        d
    }

    fn spawn_unwaited(home: &Path, sock: PathBuf, addr: &str) -> Daemon {
        let _ = std::fs::remove_file(home.join(".supragnosis/listening"));
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.join("daemon.log"))
            .unwrap();
        let mut cmd = Command::new(BIN);
        cmd.args(["serve", "--http", addr, "--viz"]).arg(&sock);
        scenario_env(&mut cmd, home, &sock, None);
        let child = cmd
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("the daemon starts");
        Daemon {
            home: home.to_path_buf(),
            sock,
            addr: addr.to_string(),
            token: String::new(),
            child: Some(child),
        }
    }

    /// Waits for the address (from the listening file, or the one it was given), the socket and
    /// the token. A daemon that exits first is a failure to report, with its log.
    fn wait_up(&mut self, limit: Duration, fixed: Option<String>) -> Result<(), String> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.child.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                return Err(format!("it exited ({status}): {}", self.log_tail()));
            }
            let addr = match &fixed {
                Some(a) => Some(a.clone()),
                None => {
                    std::fs::read_to_string(self.home.join(".supragnosis/listening")).ok().and_then(
                        |t| t.lines().find_map(|l| l.strip_prefix("mcp ").map(str::to_string)),
                    )
                }
            };
            let up = addr.as_ref().is_some_and(|a| std::net::TcpStream::connect(a).is_ok())
                && std::os::unix::net::UnixStream::connect(&self.sock).is_ok();
            if up {
                self.addr = addr.unwrap();
                self.token = std::fs::read_to_string(self.home.join(".supragnosis/mcp.token"))
                    .map_err(|e| e.to_string())?
                    .trim()
                    .to_string();
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("not answering on {addr:?}: {}", self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// SIGTERM, which is a normal exit (daemon-lifecycle.md), then the exit status. One that does
    /// not exit in time is killed (J5).
    pub fn stop(&mut self) -> Option<ExitStatus> {
        let mut child = self.child.take()?;
        if let Ok(Some(s)) = child.try_wait() {
            return Some(s);
        }
        let _ = Command::new("kill").args(["-s", "TERM", &child.id().to_string()]).status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(s)) = child.try_wait() {
                return Some(s);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        child.wait().ok()
    }

    /// SIGKILL, as a crash (A9).
    pub fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// This binary, run as one more process of the scenario: with its environment, reaching this
    /// daemon.
    pub fn cli(&self, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new(BIN);
        cmd.args(args);
        scenario_env(&mut cmd, &self.home, &self.sock, Some(&self.addr));
        cmd.stdin(Stdio::null()).output().expect("the CLI runs")
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.home.join("daemon.log")).unwrap_or_default()
    }

    fn log_tail(&self) -> String {
        let log = self.log();
        log.lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// One request to the viewer over its socket, as the console makes it: the status and the
    /// body, parsed when it is JSON.
    pub fn viz(&self, target: &str) -> (u16, Value) {
        use std::io::{Read, Write};
        let mut s = std::os::unix::net::UnixStream::connect(&self.sock).expect("the viewer socket");
        s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        write!(s, "GET {target} HTTP/1.1\r\nHost: viz\r\nConnection: close\r\n\r\n").unwrap();
        let mut raw = String::new();
        s.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
        let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_string()));
        (status, body)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_file(&self.sock);
    }
}

fn rand_suffix() -> usize {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as usize;
    n * 1_000_003 + nanos % 1_000_003
}

/// The JSON-RPC messages in a server-sent-events body (the bridge's `sse_messages`).
fn sse_messages(body: &str) -> Vec<Value> {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data: Vec<&str> = event
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .map(|d| d.strip_prefix(' ').unwrap_or(d))
                .collect();
            let data = data.join("\n");
            (!data.trim().is_empty()).then(|| serde_json::from_str(&data).ok()).flatten()
        })
        .collect()
}

/// One HTTP/1.1 request on a fresh connection, read to its end: the status, the headers (lower
/// case) and the body, with chunked transfer decoded. Enough for MCP's streamable HTTP, whose
/// answers here are one JSON body or one finished event stream.
pub fn http(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> (u16, Vec<(String, String)>, String) {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).expect("the daemon's address");
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("a header block");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut rest = &raw[split + 4..];
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let headers: Vec<(String, String)> = head
        .lines()
        .skip(1)
        .filter_map(|l| {
            l.split_once(':')
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let chunked = headers.iter().any(|(k, v)| k == "transfer-encoding" && v.contains("chunked"));
    let body = if chunked {
        let mut out = Vec::new();
        loop {
            let line_end = rest.windows(2).position(|w| w == b"\r\n").unwrap_or(rest.len());
            let size = usize::from_str_radix(String::from_utf8_lossy(&rest[..line_end]).trim(), 16)
                .unwrap_or(0);
            if size == 0 || line_end + 2 + size > rest.len() {
                break;
            }
            out.extend_from_slice(&rest[line_end + 2..line_end + 2 + size]);
            rest = &rest[(line_end + 2 + size + 2).min(rest.len())..];
        }
        String::from_utf8_lossy(&out).to_string()
    } else {
        String::from_utf8_lossy(rest).to_string()
    };
    (status, headers, body)
}

/// An MCP client as an AI app speaks it: streamable HTTP, the bearer token, one session.
pub struct Mcp {
    addr: String,
    token: String,
    session: Option<String>,
    protocol: Option<String>,
    id: u64,
}

impl Mcp {
    /// A session, initialized as an app named `name` initializes one.
    pub fn connect(d: &Daemon, name: &str) -> Mcp {
        let mut m = Mcp {
            addr: d.addr.clone(),
            token: d.token.clone(),
            session: None,
            protocol: None,
            id: 0,
        };
        let r = m
            .rpc(
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": name, "version": "1.0.0" } }),
            )
            .expect("initialize");
        m.protocol = r["protocolVersion"].as_str().map(str::to_string);
        m.post(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        m
    }

    fn post(&mut self, msg: &Value) -> (u16, Vec<Value>) {
        let mut headers = vec![
            ("Accept", "application/json, text/event-stream".to_string()),
            ("Content-Type", "application/json".to_string()),
            ("Authorization", format!("Bearer {}", self.token)),
        ];
        if let Some(s) = &self.session {
            headers.push(("Mcp-Session-Id", s.clone()));
        }
        if let Some(p) = &self.protocol {
            headers.push(("MCP-Protocol-Version", p.clone()));
        }
        let (status, hs, body) = http(&self.addr, "POST", "/mcp", &headers, &msg.to_string());
        if let Some((_, v)) = hs.iter().find(|(k, _)| k == "mcp-session-id") {
            self.session = Some(v.clone());
        }
        let sse = hs
            .iter()
            .any(|(k, v)| k == "content-type" && v.starts_with("text/event-stream"));
        let msgs = if status != 200 {
            vec![]
        } else if sse {
            sse_messages(&body)
        } else {
            serde_json::from_str(&body).map(|v| vec![v]).unwrap_or_default()
        };
        (status, msgs)
    }

    /// A request and its result; a JSON-RPC error is `Err` with its text.
    pub fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        let id = self.id;
        let (status, msgs) =
            self.post(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let reply = msgs
            .into_iter()
            .find(|m| m["id"] == json!(id))
            .ok_or_else(|| format!("{method}: no reply (HTTP {status})"))?;
        if !reply["error"].is_null() {
            return Err(format!("{method}: {}", reply["error"]));
        }
        Ok(reply["result"].clone())
    }

    /// A tool call, its text answer parsed as JSON. A refusal is `Err` with its text: a tool error,
    /// or an answer that is an `error` object, which is how a tool refuses what it was given.
    pub fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let r = self.rpc("tools/call", json!({ "name": name, "arguments": args }))?;
        let text = r["content"][0]["text"].as_str().unwrap_or_default().to_string();
        if r["isError"].as_bool() == Some(true) {
            return Err(text);
        }
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if v.get("error").is_some() {
            return Err(text);
        }
        Ok(v)
    }
}
