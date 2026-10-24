//! docs/e2e.md Section 10, the daemon's half of the harness: a daemon asked for port 0 says which
//! port it got, and a daemon asked to stop with SIGTERM exits normally. Both run the real binary in
//! a home of its own, from an environment built for it (J2).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn temp_home(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("sg-exit-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Polls `f` until it answers, or fails with what it last saw once the deadline passes (J3).
fn wait_for<T>(what: &str, limit: Duration, mut f: impl FnMut() -> Result<T, String>) -> T {
    let deadline = Instant::now() + limit;
    loop {
        match f() {
            Ok(v) => return v,
            Err(last) if Instant::now() > deadline => panic!("{what}: still {last}"),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

#[test]
fn a_daemon_on_port_zero_says_where_it_listens_and_exits_normally_on_sigterm() {
    let home = temp_home("serve");
    let sock = std::env::temp_dir().join(format!("sg-exit-{}.sock", std::process::id()));
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_supragnosis"))
        .args(["serve", "--http", "127.0.0.1:0", "--store", "mem", "--embed", "none", "--viz"])
        .arg(&sock)
        .env_clear()
        .env("HOME", &home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the daemon starts");

    let listening = home.join(".supragnosis/listening");
    let addr: std::net::SocketAddr =
        wait_for("the listening file", Duration::from_secs(20), || {
            let text = std::fs::read_to_string(&listening).map_err(|e| e.to_string())?;
            let line = text.lines().find(|l| l.starts_with("mcp ")).ok_or(format!("{text:?}"))?;
            line[4..].trim().parse().map_err(|e| format!("{line:?}: {e}"))
        });
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert_ne!(addr.port(), 0, "the port it got, not the one it was asked for");
    std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .expect("the address answers");

    let term = Command::new("kill")
        .args(["-s", "TERM", &daemon.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(term.success());
    let status = wait_for("the daemon's exit", Duration::from_secs(10), || {
        daemon
            .try_wait()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "running".to_string())
    });
    assert!(status.success(), "SIGTERM is a normal exit, not a death by signal: {status}");

    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_file(&sock);
}

/// A port nothing listens on, from the system's own pick.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// `supragnosis start` in a home of its own: the real `kill` and `ps` on the path, since the
/// pidfile's process is read through them, and a socket path short enough for a unix socket.
fn start(home: &std::path::Path, http: &str, sock: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_supragnosis"))
        .args(["start", "--http", http, "--store", "mem", "--embed", "none", "--viz"])
        .arg(sock)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/bin:/usr/bin")
        .stdin(Stdio::null())
        .output()
        .expect("start runs")
}

/// Stops whatever daemon the home's pidfile names, so a failing assertion leaves nothing running.
struct Reap(PathBuf);
impl Drop for Reap {
    fn drop(&mut self) {
        if let Ok(pid) = std::fs::read_to_string(self.0.join(".supragnosis/supragnosis.pid")) {
            let _ = Command::new("kill").arg(pid.trim()).stderr(Stdio::null()).status();
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// daemon-lifecycle.md L4: `start` says started only once the daemon answers. A daemon that exits
/// first - here on an address it refuses to bind, which only the forked daemon checks - fails
/// `start` with a non-zero exit, the reason from its log, and where that log is.
#[test]
fn a_start_whose_daemon_exits_first_fails_with_the_reason() {
    let home = temp_home("start-gone");
    let _reap = Reap(home.clone());
    let sock = std::env::temp_dir().join(format!("sg-gone-{}.sock", std::process::id()));
    let out = start(&home, &format!("0.0.0.0:{}", free_port()), &sock);
    let (said, err) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(1), "start fails: {said}{err}");
    assert!(!said.contains("started"), "nothing says started: {said}");
    assert!(err.contains("exited before it answered"), "{err}");
    assert!(err.contains("loopback"), "the daemon's own reason: {err}");
    assert!(err.contains("supragnosis.err.log"), "where the log is: {err}");
    let _ = std::fs::remove_file(&sock);
}

/// The other half: a daemon that comes up is reported started, with its address, and answers when
/// `start` returns. It is stopped through its pidfile, never through `stop`, which would look at the
/// managers of the machine running the test.
#[test]
fn a_start_says_started_once_its_daemon_answers() {
    let home = temp_home("start-up");
    let _reap = Reap(home.clone());
    let sock = std::env::temp_dir().join(format!("sg-up-{}.sock", std::process::id()));
    let http = format!("127.0.0.1:{}", free_port());
    let out = start(&home, &http, &sock);
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{said}{}", String::from_utf8_lossy(&out.stderr));
    assert!(said.contains(&format!("started - MCP http://{http}/mcp")), "{said}");
    std::net::TcpStream::connect_timeout(&http.parse().unwrap(), Duration::from_secs(2))
        .expect("it answers when start returns");
    let _ = std::fs::remove_file(&sock);
}
