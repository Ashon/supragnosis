//! client-connect.md Section 6: `supragnosis` with no subcommand is the store-opening stdio server a
//! client launches through a pipe, and the help a person sees in a terminal. Both run the real
//! binary in a home of its own.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn temp_home(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("sg-bare-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Through a pipe, the bare command is the stdio server: it answers `initialize` on stdout.
#[test]
fn a_bare_command_from_a_client_serves_stdio() {
    let home = temp_home("pipe");
    let mut server = Command::new(env!("CARGO_BIN_EXE_supragnosis"))
        .env_clear()
        .env("HOME", &home)
        .env("SUPRAGNOSIS_STORE", "mem")
        .env("SUPRAGNOSIS_EMBED", "none")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bare command starts");
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}"#;
    let mut stdin = server.stdin.take().unwrap();
    writeln!(stdin, "{init}").unwrap();
    let stdout = server.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let answer = rx.recv_timeout(Duration::from_secs(20));
    drop(stdin);
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&home);
    let answer = answer.expect("an answer to initialize");
    assert!(answer.contains("\"serverInfo\""), "{answer}");
}

/// In a terminal, the bare command prints its help and opens no store. A pseudo-terminal from
/// `script` stands in for the person typing it.
#[test]
fn a_bare_command_in_a_terminal_prints_help_and_opens_no_store() {
    let home = temp_home("tty");
    let bin = env!("CARGO_BIN_EXE_supragnosis");
    let mut script = Command::new("script");
    if cfg!(target_os = "macos") {
        script.args(["-q", "/dev/null", bin]);
    } else {
        script.args(["-qec", bin, "/dev/null"]);
    }
    let out = script
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .expect("script runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let store = home.join(".supragnosis");
    let opened = store.exists();
    let _ = std::fs::remove_dir_all(&home);
    assert!(out.status.success(), "{}: {text}", out.status);
    assert!(text.contains("Usage:"), "the help: {text}");
    assert!(!opened, "nothing under {}", store.display());
}
