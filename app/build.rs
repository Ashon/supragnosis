/// The commands the app's own pages call - Settings (docs/settings-page.md Section 4) and Welcome
/// (docs/onboarding.md Section 6). Declaring them in the app manifest is what makes them closed by
/// default: Tauri opens every registered command to every window unless the app declares its
/// commands, and then only a capability can grant one. Keep this list equal to APP_COMMANDS in
/// src/main.rs - a test there holds the two together.
const APP_COMMANDS: &[&str] = &[
    "settings_state",
    "app_toggle",
    "capture_set",
    "login_set",
    "daemon_restart",
    "search_rebuild",
    "peer_narrow",
    "setup_check",
    "setup_run",
    "setup_stop",
    "setup_running",
    "welcome_state",
    "welcome_set",
    "app_helper",
    "clipboard_copy",
    "diagnostics",
    "reveal",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("failed to run the tauri build script");
}
