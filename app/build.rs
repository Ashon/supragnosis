/// The commands the settings page calls (docs/settings-page.md Section 4). Declaring them in the
/// app manifest is what makes them closed by default: Tauri opens every registered command to every
/// window unless the app declares its commands, and then only a capability can grant one. Keep this
/// list equal to SETTINGS_COMMANDS in src/main.rs - a test there holds the two together.
const SETTINGS_COMMANDS: &[&str] = &[
    "settings_state",
    "server_use",
    "server_add",
    "server_remove",
    "app_toggle",
    "login_set",
    "daemon_restart",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(SETTINGS_COMMANDS)),
    )
    .expect("failed to run the tauri build script");
}
