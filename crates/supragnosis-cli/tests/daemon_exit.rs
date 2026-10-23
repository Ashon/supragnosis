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
