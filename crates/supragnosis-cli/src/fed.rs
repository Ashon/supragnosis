//! supragnosis.toml and the node's identity: the federation configuration, read and edited, and
//! the keypair a node is known by.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// supragnosis.toml - federation wiring. Absent file = a standalone node (every field optional);
/// a present-but-malformed file is a loud error (P5: explicit configuration must work or fail,
/// never silently degrade). Unknown keys are rejected so a typo cannot silently disable a role.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Display-only label. node_id derives from the keypair and is never configured (F14).
    #[allow(dead_code)]
    pub host_label: Option<String>,
    #[serde(default)]
    pub sync: SyncSection,
    pub server: Option<ServerSection>,
}

#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSection {
    /// Outbound share whitelist (P17/F9: nothing leaves by default).
    #[serde(default)]
    pub share_workspaces: Vec<String>,
    /// Sync server (hub) base URLs, e.g. "https://10.60.16.75:7420".
    #[serde(default)]
    pub servers: Vec<String>,
    /// Bearer token presented to those servers. The older flat shape: one credential for every
    /// host in `servers`. Superseded by `[[sync.server]]`, which carries one each.
    pub auth_token: Option<String>,
    /// Per-server entries, each with its own credential. Named `server` so the file reads
    /// `[[sync.server]]`, mirroring the per-peer `[[server.allowlist]]` shape.
    #[serde(default)]
    pub server: Vec<ServerEntry>,
    /// Retired (sync-correctness.md Section 10). It accepted any certificate, so every round
    /// handed the bearer to whoever answered. Still parsed, so a file that sets it keeps working,
    /// but never read: setting it only earns a note saying to name the hub's certificate with
    /// `ca` instead.
    #[serde(default)]
    pub insecure_tls: bool,
    /// Origin-key directory {node_id -> public key hex} for verifying pulled events (F6).
    /// Superseded by the log-borne canon-policy binding in Phase 5.
    #[serde(default)]
    pub origin_keys: std::collections::BTreeMap<String, String>,
    /// No longer read: principals, whom a hub served these to, were removed in 0.11.0
    /// (remote-server.md Section 13). Parsed so a config that still names them starts the
    /// daemon; [`SyncSection::serve_note`] says so.
    #[serde(default)]
    pub serve_workspaces: Vec<String>,
}

impl SyncSection {
    /// The note for `serve_workspaces` left in a config: read, and not used.
    pub fn serve_note(&self) -> Option<String> {
        (!self.serve_workspaces.is_empty()).then(|| {
            "[sync] serve_workspaces is no longer read: hubs serve no principals since 0.11.0, \
             and this node tells every hub to serve none of its workspaces. Remove the key."
                .to_string()
        })
    }
}

/// One sync server and the credential this node presents to it. Denies unknown keys for the
/// reason every other section does: a typo in the place that decides who this node authenticates
/// to is where being wrong is most expensive (P5/P18).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerEntry {
    pub url: String,
    pub auth_token: String,
    /// The CA, or the hub's self-signed certificate, to trust for this hub: a PEM file. How a
    /// self-signed hub is reached without `insecure_tls` (sync-correctness.md Section 10).
    #[serde(default)]
    pub ca: Option<std::path::PathBuf>,
}

impl SyncSection {
    /// The servers to talk to, each with its own credential.
    ///
    /// Two shapes are accepted and never blended. Both present at once is refused rather than
    /// resolved by precedence: a precedence rule here is a silent degrade wearing a
    /// specification, and P5 asks explicit configuration to work or fail
    /// (negotiated-surface.md N8).
    /// The servers to talk to, each with its own credential - plus what had to be worked
    /// around to get there.
    ///
    /// **Nothing here fails.** A federation configuration mistake disables or narrows
    /// federation; it does not stop a node serving its own knowledge. That is the shape this
    /// codebase already uses for a subsystem it cannot bring up - a missing embedder proceeds
    /// with keyword search, a viewer socket that will not bind proceeds without the viewer - and
    /// a sync token is not different in kind. Refusing to start was the first version, and it
    /// turned a broken hub link into a dead node.
    ///
    /// It does not degrade silently either, which is the half P5 actually asks for. Every
    /// workaround comes back as a note: logged at startup and carried into `sync_status`, so it
    /// sits on the surface an operator reads rather than in a log they scrolled past once.
    pub fn links(&self) -> (Vec<supragnosis_sync::ServerLink>, Vec<String>) {
        let mut notes = Vec::new();
        if self.insecure_tls {
            notes.push(
                "[sync] insecure_tls is no longer read: it accepted any certificate, so the \
                 bearer went to whoever answered. Every link now verifies its hub. For a \
                 self-signed hub, copy its tls_cert and name it with `ca = \"<pem file>\"` in \
                 its [[sync.server]] entry; for a hub on this machine, http:// needs no \
                 certificate. Remove insecure_tls once that is done."
                    .to_string(),
            );
        }
        let flat_present = !self.servers.is_empty() || self.auth_token.is_some();

        if !self.server.is_empty() {
            if flat_present {
                // Precedence, loudly. An earlier version refused, on the ground that choosing
                // between two stated intents is a guess - true, but the cost fell on an upgrade
                // rather than on the mistake. Per-server entries win because the other direction
                // is worse than dropping a host: one flat token presented to a host that has its
                // own would be the WRONG credential on the wire.
                notes.push(format!(
                    "[sync] has both per-server entries and the flat servers/auth_token keys. \
                     The {} entr(ies) are used and the flat keys are IGNORED, including {} host(s) \
                     listed only there. Remove the flat keys once they are folded in.",
                    self.server.len(),
                    self.servers.len()
                ));
            }
            let links = self
                .server
                .iter()
                .filter_map(|e| self.link(&e.url, &e.auth_token, e.ca.as_deref(), &mut notes))
                .collect();
            return (links, notes);
        }

        if self.servers.is_empty() {
            return (Vec::new(), notes);
        }
        let Some(token) = self.auth_token.clone() else {
            notes.push(format!(
                "[sync] lists {} server(s) but no auth_token, and every hub refuses an \
                 unauthenticated caller - so federation is OFF rather than failing every round. \
                 Set auth_token, or move each host to a [[sync.server]] entry with its own.",
                self.servers.len()
            ));
            return (Vec::new(), notes);
        };
        let links = self
            .servers
            .iter()
            .filter_map(|url| self.link(url, &token, None, &mut notes))
            .collect();
        (links, notes)
    }

    /// One host as this node will reach it (sync-correctness.md Section 10). The bearer is
    /// never sent where it could be read in transit: a plain `http://` URL to another machine
    /// disables the link, and every link verifies its host - by the system's roots, or by the
    /// certificate `ca` names. These are ignored rather than refused, because ignoring them can
    /// only make a link fail, never send more (P24), and each says what to change.
    fn link(
        &self,
        url: &str,
        token: &str,
        ca: Option<&std::path::Path>,
        notes: &mut Vec<String>,
    ) -> Option<supragnosis_sync::ServerLink> {
        let parsed = reqwest::Url::parse(url).ok();
        let loopback =
            parsed.as_ref().and_then(|u| u.host_str()).is_some_and(crate::is_loopback_host);
        if parsed.as_ref().is_some_and(|u| u.scheme() == "http") && !loopback {
            notes.push(format!(
                "[sync] {url} is plain HTTP to another machine, so its bearer would cross the \
                 network unencrypted. The link is DISABLED for this run. Use https:// - the \
                 hub's [server] listener terminates TLS."
            ));
            return None;
        }
        let ca_pem = match ca {
            None => None,
            Some(path) => match std::fs::read(path) {
                Ok(pem) => Some(pem),
                Err(e) => {
                    notes.push(format!(
                        "[sync] {url}: its ca {} could not be read ({e}). The link is \
                         DISABLED for this run rather than trusting a certificate it did not \
                         name.",
                        path.display()
                    ));
                    return None;
                }
            },
        };
        Some(supragnosis_sync::ServerLink {
            url: url.to_string(),
            auth_token: token.to_string(),
            ca_pem,
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSection {
    /// Sync API bind (IP:port). Non-loopback demands TLS and an admitted node or an open
    /// invitation (pairing.md PA1).
    pub listen: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    #[serde(default)]
    pub allowlist: Vec<supragnosis_sync::http::AllowEntry>,
    /// Principals are no longer read: the hub's agent surface was removed in 0.11.0
    /// (remote-server.md Section 13). Parsed, whatever their shape, so a config that still
    /// names them starts the daemon; [`principals_note`] says so.
    #[serde(default)]
    pub principals: Vec<toml::Value>,
}

/// The note for `[[server.principals]]` left in a config: read, and not used.
pub fn principals_note(srv: Option<&ServerSection>) -> Option<String> {
    srv.filter(|s| !s.principals.is_empty()).map(|_| {
        "[[server.principals]] is no longer read: the hub's agent surface was removed in \
         0.11.0, and the hub serves only sync and pairing. Remove the entries."
            .to_string()
    })
}

/// `~/.supragnosis` - where the node key, the config and the daemon token live. Public because
/// the MCP token is not a federation concern but wants the same directory (and the same 0700
/// that already protects it) rather than a second answer to "where does state go".
pub fn fed_base_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".supragnosis")
}

/// SUPRAGNOSIS_CONFIG, or ~/.supragnosis/supragnosis.toml.
pub fn config_path() -> PathBuf {
    std::env::var("SUPRAGNOSIS_CONFIG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| fed_base_dir().join("supragnosis.toml"))
}

pub fn load() -> Result<Option<FileConfig>> {
    load_at(&config_path())
}

pub fn load_at(path: &std::path::Path) -> Result<Option<FileConfig>> {
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(cfg))
}

/// Edits `supragnosis.toml` in place under the lock file beside it (docs/pairing.md Section
/// 4.4). The file is re-read inside the lock, so two edits - pairing's, the narrowing act's,
/// `principal`'s - never lose each other's change; `edit` returns the new text, built with the
/// format-preserving editor so every line the person wrote stays; and the new text replaces the
/// old through a rename, so a reader never sees half of it. A file that does not exist yet is
/// read as empty and made owner-only.
pub fn edit_config_at<T>(
    path: &std::path::Path,
    edit: impl FnOnce(&str) -> Result<(String, T)>,
) -> Result<T> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("supragnosis.toml");
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let lock_path = path.with_file_name(format!(".{name}.lock"));
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    lock.lock().with_context(|| format!("locking {}", lock_path.display()))?;
    let (text, existed) = match std::fs::read_to_string(path) {
        Ok(t) => (t, true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let (edited, out) = edit(&text)?;
    if edited != text {
        // Written to a file that is owner-only from its first byte, then given the old file's
        // mode and renamed over it: the file may hold a sync credential, and no reader sees
        // half of it. A new file stays owner-only; nothing touches the directory it lands
        // in, which SUPRAGNOSIS_CONFIG may put anywhere.
        use std::io::Write;
        let tmp = path.with_file_name(format!(".{name}.supragnosis-new"));
        let _ = std::fs::remove_file(&tmp);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(edited.as_bytes())?;
        f.sync_all()?;
        if existed {
            if let Ok(meta) = std::fs::metadata(path) {
                std::fs::set_permissions(&tmp, meta.permissions())?;
            }
        }
        std::fs::rename(&tmp, path).with_context(|| format!("installing {}", path.display()))?;
    }
    Ok(out)
}

/// Narrows one peer's `shared_workspaces` in `supragnosis.toml`, preserving the file's comments
/// and layout, and returns the set that is now granted.
///
/// **Narrowing only.** The requested set must be a subset of what the peer already has; a request
/// naming anything it does not currently hold is refused rather than partially applied. That
/// asymmetry is the whole safety argument for putting this on a surface at all: the act can only
/// move in the direction P17 already prefers (less sharing), so the worst outcome of a mistake -
/// or of a console left open - is that a peer reads less than intended. Widening stays where it
/// is, in the file, where it is a deliberate act with the operator's full context.
///
/// The file is re-read here rather than edited from the process's startup copy, so an operator
/// who hand-edited it while the daemon ran does not have their change silently reverted by a
/// console click. Removing every workspace is permitted and means the peer stays admitted but may
/// read nothing; removing the peer itself is not this function's job (the deferred revocation
/// workflow, federation.md Section 11).
pub fn narrow_shared_workspaces(node_id: &str, keep: &[String]) -> Result<Vec<String>> {
    let path = config_path();
    edit_config_at(&path, |text| narrow_text(&path, text, node_id, keep))
}

fn narrow_text(
    path: &std::path::Path,
    text: &str,
    node_id: &str,
    keep: &[String],
) -> Result<(String, Vec<String>)> {
    let mut doc: toml_edit::DocumentMut =
        text.parse().with_context(|| format!("parsing {}", path.display()))?;

    let entries = doc
        .get_mut("server")
        .and_then(|s| s.get_mut("allowlist"))
        .and_then(|a| a.as_array_of_tables_mut())
        .ok_or_else(|| anyhow::anyhow!("no [[server.allowlist]] in {}", path.display()))?;

    // Every entry for the peer, not the first: a rotated token leaves two, each admitted on
    // its own bearer, and narrowing one would leave the other reading the old grant.
    let mut matching: Vec<&mut toml_edit::Table> = entries
        .iter_mut()
        .filter(|t| t.get("node_id").and_then(|v| v.as_str()) == Some(node_id))
        .collect();
    if matching.is_empty() {
        anyhow::bail!("no allowlist entry for node_id {node_id}");
    }
    let shared = |t: &toml_edit::Table| -> Vec<String> {
        t.get("shared_workspaces")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let current: Vec<Vec<String>> = matching.iter().map(|t| shared(t)).collect();
    let union: Vec<&String> = current.iter().flatten().collect();
    let widened: Vec<&String> = keep.iter().filter(|w| !union.contains(w)).collect();
    if !widened.is_empty() {
        anyhow::bail!(
            "refusing to widen: {node_id} does not currently share {widened:?} (it shares \
             {union:?}). This surface only narrows - grant a workspace by editing {}",
            path.display()
        );
    }

    // Each entry keeps what it already had of `keep`, so none is widened by another's grant.
    let mut now: Vec<String> = Vec::new();
    for (entry, had) in matching.iter_mut().zip(&current) {
        let mut arr = toml_edit::Array::new();
        for w in keep.iter().filter(|w| had.contains(w)) {
            arr.push(w.as_str());
            if !now.contains(w) {
                now.push(w.clone());
            }
        }
        entry["shared_workspaces"] = toml_edit::value(arr);
    }
    // Written by `edit_config_at`, through a temp file and a rename: the file holds the node's
    // sync credentials, and a reader - the daemon re-reads it right after this - must never
    // see half of it.
    Ok((doc.to_string(), now))
}

/// Loads (or generates exactly once) the node keypair at ~/.supragnosis/node.key - 32 raw
/// secret bytes, mode 0600. The node_id derives from the public key and never changes (F14).
pub fn load_or_create_identity() -> Result<supragnosis_core::NodeIdentity> {
    let path = fed_base_dir().join("node.key");
    if path.exists() {
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("{} must be exactly 32 bytes", path.display()))?;
        return Ok(supragnosis_core::NodeIdentity::from_secret_bytes(arr));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("entropy source failed: {e}"))?;
    super::write_secret(&path, &secret)?;
    tracing::info!(
        path = %path.display(),
        "generated the node keypair (once - the node_id is immutable, F14). The key and node.seq \
         beside it are this node: move them with its store, never copy them to a second running \
         node (docs/sync-correctness.md Section 5)"
    );
    Ok(supragnosis_core::NodeIdentity::from_secret_bytes(secret))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// remote-server.md Section 13: `serve_workspaces` left in a config parses and is said to do
    /// nothing.
    #[test]
    fn serve_workspaces_left_in_a_config_is_said_to_do_nothing() {
        let cfg: FileConfig = toml::from_str(
            "[sync]\nshare_workspaces = [\"team\"]\nserve_workspaces = [\"team\", \"private\"]\n",
        )
        .unwrap();
        let note = cfg.sync.serve_note().expect("a note");
        assert!(note.contains("no longer read") && note.contains("0.11.0"), "{note}");
        let none: FileConfig = toml::from_str("[sync]\nshare_workspaces = [\"team\"]\n").unwrap();
        assert_eq!(none.sync.serve_note(), None);
    }

    /// remote-server.md Section 13: a hub's config that still names principals, from before 0.11.0,
    /// parses - so the daemon starts - and the key is said to be no longer read, whatever the
    /// entries hold.
    #[test]
    fn principals_left_in_a_config_are_read_and_said_to_do_nothing() {
        let left = r#"
                [server]
                listen = "127.0.0.1:7420"
                [[server.principals]]
                name = "alice"
                token_hash = "abc"
                read = ["team"]
                [[server.principals]]
                anything = 1
            "#;
        let cfg: FileConfig = toml::from_str(left).expect("an old config still parses");
        let note = principals_note(cfg.server.as_ref()).expect("a note");
        assert!(note.contains("no longer read") && note.contains("0.11.0"), "{note}");
        let none: FileConfig = toml::from_str("[server]\nlisten = \"127.0.0.1:7420\"\n").unwrap();
        assert_eq!(principals_note(none.server.as_ref()), None);
    }

    /// The documented supragnosis.toml shape parses; unknown keys are rejected loudly (P24, N8).
    #[test]
    fn config_parses_and_rejects_typos() {
        let good = r#"
                host_label = "knowledge-vm"
                [sync]
                share_workspaces = ["supragnosis"]
                servers = ["https://10.60.16.75:7420"]
                auth_token = "tok"
                insecure_tls = true
                [sync.origin_keys]
                "abc" = "deadbeef"
                [server]
                listen = "0.0.0.0:7420"
                tls_cert = "/etc/supragnosis/cert.pem"
                tls_key = "/etc/supragnosis/key.pem"
                [[server.allowlist]]
                node_id = "abc"
                public_key_hex = "deadbeef"
                bearer_hash = "hash"
                shared_workspaces = ["supragnosis"]
            "#;
        let cfg: FileConfig = toml::from_str(good).expect("documented shape must parse");
        assert_eq!(cfg.sync.servers.len(), 1);
        assert!(cfg.sync.insecure_tls);
        let srv = cfg.server.expect("server section");
        assert_eq!(srv.allowlist.len(), 1);
        assert_eq!(cfg.sync.origin_keys.get("abc").map(String::as_str), Some("deadbeef"));

        // The per-server shape, which carries a credential each rather than one for all.
        let per_server = r#"
                [sync]
                share_workspaces = ["supragnosis"]
                [[sync.server]]
                url = "https://hub-cloud.internal:7420"
                auth_token = "tok-cloud"
                [[sync.server]]
                url = "https://hub-net.internal:7420"
                auth_token = "tok-net"
            "#;
        let cfg: FileConfig = toml::from_str(per_server).expect("per-server shape must parse");
        let (links, notes) = cfg.sync.links();
        assert!(notes.is_empty(), "a clean per-server config needs no workaround: {notes:?}");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].auth_token, "tok-cloud");
        assert_eq!(links[1].auth_token, "tok-net", "each host gets its own credential");

        // The flat shape still resolves, to the same token repeated - which is what the
        // per-server shape exists to replace, not something to break on the way there.
        let flat = r#"
                [sync]
                servers = ["https://a:7420", "https://b:7420"]
                auth_token = "shared"
            "#;
        let cfg: FileConfig = toml::from_str(flat).expect("the flat shape still parses");
        let (links, notes) = cfg.sync.links();
        assert!(notes.is_empty(), "the flat shape alone is still a supported configuration");
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.auth_token == "shared"));

        // Both shapes at once resolve by a stated precedence, loudly: a rule that chose silently
        // which credential a host is given is the class of configuration failure P5 asks to be
        // loud about, and the note below is what keeps it loud (negotiated-surface.md N8).
        let both = r#"
                [sync]
                servers = ["https://a:7420"]
                auth_token = "shared"
                [[sync.server]]
                url = "https://a:7420"
                auth_token = "own"
            "#;
        let cfg: FileConfig = toml::from_str(both).expect("both shapes parse individually");
        let (links, notes) = cfg.sync.links();
        // Precedence, loudly. Refusing was the first version and it broke an upgrade over a
        // mistake that costs nothing to work around; per-server wins because the other way round
        // would put one flat token on the wire to a host that has its own.
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].auth_token, "own", "the per-server credential is the one used");
        assert_eq!(notes.len(), 1, "and the operator is told the flat keys were ignored");
        assert!(notes[0].contains("IGNORED"), "the note names what was dropped: {notes:?}");

        // `servers` with no token cannot reach any hub, so it fails at load rather than at the
        // first round - every hub refuses an unauthenticated caller (F6).
        let tokenless = "[sync]\nservers = [\"https://a:7420\"]\n";
        let cfg: FileConfig = toml::from_str(tokenless).expect("parses");
        let (links, notes) = cfg.sync.links();
        // Federation off, node up. A missing sync token is a subsystem this build cannot bring
        // up, which is the case the embedder and the viewer socket already answer by degrading
        // with a loud line rather than by killing a node that serves its own knowledge.
        assert!(links.is_empty(), "no link can be built without a credential");
        assert_eq!(notes.len(), 1, "and it is not silent: {notes:?}");
        assert!(notes[0].contains("federation is OFF"), "the note says what stopped: {notes:?}");

        // A typo must fail loudly, not silently disable a role (P5).
        let typo = "share_workspace = [\"x\"]\n";
        assert!(toml::from_str::<FileConfig>(typo).is_err());

        // The same rule one level down, inside an allowlist entry. This was the one struct that
        // did not deny unknown keys (federation.md Section 9 recorded it as a standing gap), so a
        // misspelled key in the place that decides who may connect and what they may read parsed
        // clean and did nothing. An entry is where a typo is most expensive (P17/P18): the safe
        // way to be wrong about it is to refuse to start.
        let entry_typo = r#"
                [server]
                listen = "127.0.0.1:7420"
                [[server.allowlist]]
                node_id = "abc"
                public_key_hex = "deadbeef"
                bearer_hash = "hash"
                shared_workspace = ["supragnosis"]
            "#;
        let err = toml::from_str::<FileConfig>(entry_typo)
            .expect_err("a misspelled key inside an allowlist entry must not parse");
        assert!(
            err.to_string().contains("shared_workspace"),
            "the error has to name the offending key: {err}"
        );
    }

    /// Narrowing rewrites only the peer's arrays - every one of them - and refuses to widen.
    ///
    /// Comment preservation is not cosmetic here. `supragnosis.toml` is a hand-maintained
    /// declaration - the hub config in the author's own deployment carries paragraphs explaining
    /// why it has no `[sync]` section - and a console that reformatted it would make operators
    /// stop trusting the console. Serializing the parsed struct back out would do exactly that,
    /// which is why this edits the document rather than the model.
    #[test]
    fn a_round_waits_five_minutes_and_backs_off_to_an_hour() {
        use std::time::Duration;
        let five = Duration::from_secs(300);
        assert_eq!(crate::next_round_wait(Duration::ZERO, false), five);
        assert_eq!(crate::next_round_wait(Duration::ZERO, true), Duration::from_secs(600));
        let mut wait = Duration::ZERO;
        for _ in 0..10 {
            wait = crate::next_round_wait(wait, true);
        }
        assert_eq!(wait, Duration::from_secs(3600), "a failing hub is asked at most hourly");
        assert_eq!(
            crate::next_round_wait(wait, false),
            five,
            "one round that works resets the wait"
        );
    }

    /// docs/pairing.md Section 4.4: edits made at once, from many threads, each re-read the file
    /// inside the lock, so none loses another's change, and every line the person wrote stays.
    #[test]
    fn two_edits_never_lose_each_others_change() {
        let dir = std::env::temp_dir().join(format!("sg-edit-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("supragnosis.toml");
        std::fs::write(&path, "# the person's line\nhost_label = \"hub\"\n").unwrap();
        let edits: Vec<_> = (0..16)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    edit_config_at(&path, |text| {
                        let mut doc: toml_edit::DocumentMut = text.parse()?;
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        doc[&format!("k{i}")] = toml_edit::value(i as i64);
                        Ok((doc.to_string(), ()))
                    })
                    .unwrap()
                })
            })
            .collect();
        edits.into_iter().for_each(|e| e.join().unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# the person's line\n"), "{text}");
        for i in 0..16 {
            assert!(text.contains(&format!("k{i} = {i}")), "edit {i} was lost: {text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn narrowing_preserves_the_file_and_refuses_to_widen() {
        let dir = std::env::temp_dir().join(format!(
            "supragnosis-narrow-{}-{}",
            std::process::id(),
            supragnosis_core::now_millis()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("supragnosis.toml");
        let original = r#"# supragnosis hub - explains itself, and must keep explaining itself
host_label = "hub"

[server]
listen = "0.0.0.0:7420"   # F10: non-loopback needs TLS and an admitted node or an open invitation

# ashons-MacBook-Air
[[server.allowlist]]
node_id = "peer-a"
public_key_hex = "aa"
bearer_hash = "hh"
shared_workspaces = ["alpha", "beta", "gamma"]

# the same peer, after a token rotation left its old entry in place
[[server.allowlist]]
node_id = "peer-a"
public_key_hex = "aa"
bearer_hash = "h2"
shared_workspaces = ["alpha", "gamma"]
"#;
        std::fs::write(&path, original).expect("write fixture");
        let prev = std::env::var("SUPRAGNOSIS_CONFIG").ok();
        std::env::set_var("SUPRAGNOSIS_CONFIG", &path);

        // Widening is refused, and says what is actually granted.
        let err = narrow_shared_workspaces("peer-a", &["alpha".into(), "delta".into()])
            .expect_err("widening must be refused");
        assert!(err.to_string().contains("delta"), "names what was refused: {err}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            original,
            "a refused request must not touch the file"
        );

        // A subset is applied to every entry for the peer: each bearer is admitted on its
        // own, so one left wide would still read what the console says was taken away.
        let kept =
            narrow_shared_workspaces("peer-a", &["alpha".into(), "beta".into()]).expect("narrow");
        assert_eq!(kept, vec!["alpha".to_string(), "beta".to_string()]);
        let after = std::fs::read_to_string(&path).expect("read");
        assert!(
            after.contains(r#"shared_workspaces = ["alpha", "beta"]"#)
                && after.contains(r#"shared_workspaces = ["alpha"]"#)
                && !after.contains("gamma"),
            "both entries narrowed, neither widened by the other's grant: {after}"
        );
        assert!(
            after.contains("# supragnosis hub - explains itself")
                && after.contains("# ashons-MacBook-Air")
                && after.contains("# F10: non-loopback"),
            "every comment survives the edit: {after}"
        );
        assert!(after.contains(r#"host_label = "hub""#), "untouched keys stay put");
        // It still parses as the config the daemon loads - an edit that produced valid TOML but
        // an invalid config would only be discovered at the next restart.
        toml::from_str::<FileConfig>(&after).expect("the edited file is still a valid config");

        // Emptying is a legitimate narrowing: admitted, but may read nothing.
        narrow_shared_workspaces("peer-a", &[]).expect("empty is a narrowing");
        let after = std::fs::read_to_string(&path).expect("read");
        assert_eq!(after.matches("shared_workspaces = []").count(), 2, "{after}");

        // An unknown peer is an error, not a silent no-op.
        assert!(narrow_shared_workspaces("nobody", &[]).is_err());

        match prev {
            Some(v) => std::env::set_var("SUPRAGNOSIS_CONFIG", v),
            None => std::env::remove_var("SUPRAGNOSIS_CONFIG"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
