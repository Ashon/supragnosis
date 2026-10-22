//! daemon-lifecycle.md L4: every CLI lifecycle command exits non-zero on failure with the reason.
//!
//! The binary itself runs here, with `PATH` holding nothing but stand-in tools. It cannot reach
//! the real launchd, `kill` or `ps` of the machine running the test - a command missing from that
//! directory is a command it cannot run - and its address points at a port nothing listens on, so
//! the daemon this machine runs is never observed or touched.
#![cfg(target_os = "macos")]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn tool(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A conflict - the login job serving and Homebrew's job loaded and failing - is refused by `stop`
/// and `restart` alike: exit status 1, the reason naming every manager and the command that resolves
/// it, and no launchctl verb other than `list` ever run.
#[test]
fn a_refused_lifecycle_command_exits_non_zero_with_the_reason() {
    let root = std::env::temp_dir().join(format!("sg-lifecycle-exit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (bin, home, data) = (root.join("bin"), root.join("home"), root.join("data"));
    for dir in [&bin, &home, &data] {
        std::fs::create_dir_all(dir).unwrap();
    }
    // `list <label>` answers for the two managers and fails for every other label, as launchctl
    // does for a job it has not loaded; any other verb leaves a mark beside the tool.
    tool(
        &bin,
        "launchctl",
        r#"case "$1 $2" in
  "list com.supragnosis.daemon")
    printf '{\n\t"Label" = "com.supragnosis.daemon";\n\t"PID" = 2292;\n};\n'; exit 0;;
  "list sh.brew.supragnosis-server")
    printf '{\n\t"Label" = "sh.brew.supragnosis-server";\n\t"LastExitStatus" = 256;\n};\n'; exit 0;;
  list*) exit 113;;
esac
: > "$0.acted"
"#,
    );
    tool(&bin, "id", "echo 501\n");

    for command in ["stop", "restart"] {
        let out = Command::new(env!("CARGO_BIN_EXE_supragnosis"))
            .arg(command)
            .env_clear()
            .env("PATH", &bin)
            .env("HOME", &home)
            .env("SUPRAGNOSIS_DATA_DIR", &data)
            .env("SUPRAGNOSIS_HTTP_ADDR", "127.0.0.1:1")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{command} refuses with a non-zero exit: {said}");
        for name in ["com.supragnosis.daemon", "sh.brew.supragnosis-server", "--take-over"] {
            assert!(said.contains(name), "{command}'s reason names {name}: {said}");
        }
        assert!(!bin.join("launchctl.acted").exists(), "{command} acted on nothing");
    }
    let _ = std::fs::remove_dir_all(&root);
}
