//! The version lives in eight places, so something has to hold them equal.
//!
//! `[workspace.package].version` is inherited by every crate through `version.workspace = true`, and
//! that half takes care of itself. The other half does not:
//!
//! - the seven `[workspace.dependencies]` internal entries repeat the number as a literal, because
//!   `version.workspace` is not accepted inside `[workspace.dependencies]`;
//! - `app/Cargo.toml` is a SEPARATE workspace (the root excludes it explicitly), so it inherits
//!   nothing and carries its own literal;
//! - `server.json` names the version and an exact image tag, both of which a registry entry is
//!   published against.
//!
//! v0.2.2 moved three of those and left the seven internal requirements at `0.2.1`. That was
//! harmless - `0.2.1` is a caret range and 0.2.2 satisfies it - and harmlessness is the problem: the
//! same stale manifest fails `cargo publish` at the next MINOR, where `^0.2.1` does not match 0.3.0,
//! and the failure arrives at publish time pointing at a mistake made a release earlier.
//!
//! The release workflow already checks `server.json` against the git tag, which is the one comparison
//! that needs a tag to exist. Everything here needs only the tree, so it runs on every push instead
//! of once per release - the same reasoning `principle_coverage.rs` applies to the design documents:
//! a coupling that only a human remembers is not a coupling.

/// The manifests are read from the source tree rather than through `env!("CARGO_PKG_VERSION")`,
/// because the point is to compare the FILES. A constant Cargo handed us would agree with itself.
const ROOT_MANIFEST: &str = include_str!("../../../Cargo.toml");
const APP_MANIFEST: &str = include_str!("../../../app/Cargo.toml");
const SERVER_JSON: &str = include_str!("../../../server.json");

/// The internal crates whose `[workspace.dependencies]` entry carries a hand-written version.
/// Listed rather than discovered, so deleting an entry is as visible as letting one drift.
const INTERNAL: &[&str] = &[
    "supragnosis-core",
    "supragnosis-store",
    "supragnosis-sync",
    "supragnosis-engine",
    "supragnosis-embed",
    "supragnosis-mcp",
    "supragnosis-viz",
];

fn root() -> toml::Value {
    ROOT_MANIFEST.parse::<toml::Value>().expect("the root manifest parses")
}

/// Owned rather than borrowed, so a caller can write `workspace_version(&root())` without keeping
/// the parsed document alive for the sake of one string.
fn workspace_version(root: &toml::Value) -> String {
    root["workspace"]["package"]["version"]
        .as_str()
        .expect("[workspace.package].version is a string")
        .to_string()
}

/// Every internal path dependency requires the version this workspace actually builds.
///
/// The failure this prevents is not a broken build - it is a `cargo publish` that resolves an
/// internal dependency to an OLDER published crate, or refuses to resolve it at all, on the strength
/// of a number nothing else in the tree reads.
#[test]
fn internal_dependencies_require_the_version_this_workspace_builds() {
    let root = root();
    let want = workspace_version(&root);
    let deps = &root["workspace"]["dependencies"];
    for name in INTERNAL {
        let entry = deps
            .get(name)
            .unwrap_or_else(|| panic!("[workspace.dependencies] has no entry for {name}"));
        // A path-only entry cannot be published at all, so its absence is as much a drift as a
        // stale value - it just fails later and less legibly.
        let got = entry.get("version").and_then(toml::Value::as_str).unwrap_or_else(|| {
            panic!(
                "[workspace.dependencies].{name} carries no `version` - a path-only entry cannot \
                 be published. Add version = \"{want}\""
            )
        });
        assert_eq!(
            got,
            want.as_str(),
            "\n[workspace.dependencies].{name} requires {got:?} but this workspace builds \
             {want:?}.\nHarmless while both are 0.2.x (a caret range covers it) and fatal at the \
             next minor, which is why it is checked here and not at publish time.\nFix: set every \
             internal entry in Cargo.toml to version = \"{want}\"."
        );
    }
}

/// The desktop shell is its own workspace, so its version is a second copy that inherits nothing
/// from the server's.
///
/// `app/Cargo.toml` says so itself: "The one thing membership did give for free is version sync, and
/// it is now manual: the version below has to move with the workspace version above in the same
/// `release:` commit." A comment is where that obligation lived; this is where it is enforced. The
/// shell ships under the same release tag as the daemon it supervises, and a pair reporting two
/// numbers is a support question nobody can answer from the outside.
#[test]
fn the_desktop_shell_declares_the_same_version() {
    let want = workspace_version(&root());
    let app = APP_MANIFEST.parse::<toml::Value>().expect("the app manifest parses");
    // Its `[package]` inherits from its OWN `[workspace.package]`, which is the literal that moves.
    let got = workspace_version(&app);
    assert_eq!(
        got, want,
        "\napp/Cargo.toml declares {got:?} but the server workspace builds {want:?}.\nThe shell is \
         excluded from the root workspace on purpose (it would drag 111 Tauri packages into this \
         lockfile), so it inherits nothing and this is the only thing holding the two equal."
    );
}

/// `server.json` names the version and the exact image tag the MCP Registry entry points at.
///
/// The release workflow compares both against the git tag, which is the check that needs a tag. This
/// one needs only the tree, so a bump that moves Cargo.toml and forgets server.json fails on the
/// commit that made the mistake rather than on the tag push that publishes it.
#[test]
fn the_registry_entry_names_the_version_this_workspace_builds() {
    let want = workspace_version(&root());
    let server: serde_json::Value = serde_json::from_str(SERVER_JSON).expect("server.json parses");
    let got = server["version"].as_str().expect("server.json version is a string");
    assert_eq!(
        got,
        want.as_str(),
        "\nserver.json declares version {got:?} but this workspace builds {want:?}."
    );

    let identifier = server["packages"][0]["identifier"]
        .as_str()
        .expect("server.json packages[0].identifier is a string");
    let (image, tag) = identifier
        .rsplit_once(':')
        .unwrap_or_else(|| panic!("packages[0].identifier {identifier:?} carries no :tag"));
    assert_eq!(
        tag, want.as_str(),
        "\nserver.json points the registry at {image}:{tag} but this workspace builds {want:?}.\nA \
         registry entry naming an image nobody published is not a broken build - it is an install \
         that fails for everyone who finds it."
    );
}
