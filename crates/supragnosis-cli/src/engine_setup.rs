//! Building the engine: the refusal of an un-migrated store, the checks on the keys and
//! admissions supragnosis.toml holds, and `build_engine`.

use super::*;

/// The last release that could read a Cozo (RocksDB) store was v0.1.21. This build cannot, so an
/// un-migrated store is knowledge this binary can neither open nor explain.
///
/// The failure mode that matters is not the error - it is the absence of one. A daemon that came up
/// on an empty redb store beside a full Cozo directory would answer every read with "nothing here",
/// which is exactly what a node with no knowledge looks like (Principle 5: absence must be
/// distinguishable from unavailable, and here the honest answer is unavailable). Worse, it would then
/// start accumulating a second, divergent log.
///
/// So this is a refusal with an instruction, and it is also what carries Principle 3's demand that
/// every encoding the log has ever used stays readable, for the encodings before redb: they are
/// still reachable, through the release that wrote them, and skipping that step is made impossible
/// rather than merely discouraged. Within redb the store records its format and a golden store per
/// format is read back by every build (docs/compatibility.md).
///
/// Detection is `CURRENT`, the file RocksDB always writes and redb never does. A bare directory is
/// not enough - an empty `~/.supragnosis/db` left behind by a completed migration must not block a
/// clean start forever.
///
/// Two locations, because the Cozo adapter did not put the marker where the first version of this
/// function looked for it: it handed RocksDB a `data` subdirectory of the configured dir and kept
/// `manifest` beside it, so a real store has `data/CURRENT`. Checking only the top level made the
/// guard match nothing on any store it was written to catch. The flat form stays accepted, since a
/// store that was moved or unpacked by hand is still one.
pub(crate) fn legacy_cozo_store_at(data_dir: &str) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(data_dir);
    for marker in [dir.join("data").join("CURRENT"), dir.join("CURRENT")] {
        if marker.is_file() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Refuses to start when an un-migrated Cozo store is present and the redb store does not yet exist.
///
/// Both conditions are required. Once the redb store exists the migration has run, and the Cozo
/// directory is a rollback artifact the operator may keep as long as they like - blocking on it then
/// would punish exactly the cautious path.
///
/// The skip is keyed on the in-memory store and nothing else, because that is the only value
/// [`build_engine`] treats as not-redb: everything it does not recognise opens redb. Keyed the other
/// way - skip unless the kind is exactly `redb` - a stale `SUPRAGNOSIS_STORE=cozo` walked past this
/// guard and then opened redb anyway, inside the legacy directory, which is precisely the empty
/// store beside a full one that this function exists to prevent. A configuration naming a store this
/// build dropped has to reach the refusal, not slip under it.
pub(crate) fn refuse_unmigrated_store(cfg: &Config) -> Result<()> {
    if matches!(cfg.store_kind.as_str(), "mem" | "memory") {
        return Ok(());
    }
    if redb_path(&cfg.data_dir).exists() {
        return Ok(());
    }
    // The configured dir first. A node that was ever told where to keep its data - `SUPRAGNOSIS_DATA_DIR`
    // in a systemd unit is the ordinary case - has its store there and not under the default, so a
    // guard that consulted only the default was blind on exactly the deployments that had been
    // administered. Checking the configured dir is also what makes the redb-exists skip above
    // symmetric: that test already reads `cfg.data_dir`.
    let default_legacy = default_data_dir_for("cozo");
    let Some(found) =
        legacy_cozo_store_at(&cfg.data_dir).or_else(|| legacy_cozo_store_at(&default_legacy))
    else {
        return Ok(());
    };
    anyhow::bail!(
        "found a Cozo store at {} and no redb store at {}.\n\n\
         This build reads only redb. The knowledge is not lost - it is in a format the previous \
         release can still read - but this binary cannot open it, and starting empty beside it \
         would silently begin a second log.\n\n\
         Migrate with v0.1.21, which reads both:\n  \
         curl -fsSL https://supragnosis.dev/install.sh | sh -s -- --version v0.1.21\n  \
         supragnosis migrate-store\n\n\
         Then upgrade again. The Cozo store is opened read-only and left untouched, so the step is \
         reversible. Full procedure: docs/store-migration.md",
        found.display(),
        redb_path(&cfg.data_dir).display(),
    )
}

/// A key not in the one spelling this software writes could never verify an event
/// (sync-correctness.md Section 7), so the entry carrying it is IGNORED for the run, and the file is
/// left alone for the operator to correct. Ignoring an allowlist entry drops a live credential and
/// ignoring an origin key drops what that origin's events would land as: both share less (P24).
pub(crate) fn noncanonical_key_notes(fc: &fed::FileConfig) -> Vec<String> {
    let bad = |k: &str| !supragnosis_core::is_canonical_public_key_hex(k);
    let mut notes = Vec::new();
    for e in fc.server.iter().flat_map(|s| &s.allowlist) {
        if bad(&e.public_key_hex) {
            notes.push(format!(
                "[[server.allowlist]] {}: public_key_hex is not 64 lowercase hex digits, so no event \
                 it signs could verify. The entry is IGNORED for this run. Copy the key exactly as \
                 `supragnosis identity` prints it on that node.",
                e.node_id
            ));
        }
    }
    for (id, k) in &fc.sync.origin_keys {
        if bad(k) {
            notes.push(format!(
                "[sync.origin_keys] {id}: the key is not 64 lowercase hex digits, so no event it \
                 signs could verify. It is IGNORED for this run. Copy the key exactly as \
                 `supragnosis identity` prints it on that node."
            ));
        }
    }
    notes
}

/// The origin keys this node verifies pulled events against: `[sync.origin_keys]` without the
/// entries [`noncanonical_key_notes`] names.
pub(crate) fn trusted_origin_keys(
    sync: &fed::SyncSection,
) -> std::collections::BTreeMap<String, String> {
    let mut keys = sync.origin_keys.clone();
    keys.retain(|_, k| supragnosis_core::is_canonical_public_key_hex(k));
    keys
}

/// F14's other half: a node whose own id sits in its own allowlist admits itself as a peer.
///
/// It would answer its own pulls, and once routing consults a host's answer it would negotiate with
/// itself. Nothing consumed such an entry before, so nothing ever complained - the invariant claimed
/// a refusal the code did not perform. An entry naming this node is either a copied template or a
/// mistyped id.
///
/// **Dropped rather than refused, and the file is left alone.** Refusing was the first version and it
/// broke an upgrade over a mistake that costs nothing to work around: an allowlist entry is a live
/// credential (admission authenticates by bearer hash, not by node id), so removing this one from the
/// running directory can only share LESS - the one direction 6a says a mistake may move on its own.
/// The file keeps the entry, so the operator's intent stays visible and a mistyped id is still theirs
/// to correct; the note says what was ignored and why.
pub(crate) fn drop_self_admission(
    node_id: &str,
    server: Option<&fed::ServerSection>,
) -> Vec<String> {
    let Some(srv) = server else {
        return Vec::new();
    };
    if srv.allowlist.iter().any(|e| e.node_id == node_id) {
        return vec![format!(
            "[server] allowlist admits this node's own id {node_id} - a node cannot be its own peer. \
             The entry is IGNORED for this run (it is a live credential, so ignoring it only shares \
             less). It is still in supragnosis.toml: remove it, or correct the id if it was mistyped."
        )];
    }
    Vec::new()
}

/// Assembles the store/embedder/engine from the configuration. If `events` is present, attaches a UI event sink (the viewer).
pub(crate) fn build_engine(
    cfg: &Config,
    events: Option<&tokio::sync::broadcast::Sender<String>>,
) -> Result<Arc<Engine>> {
    build_engine_in(cfg, events, &fed::fed_base_dir())
}

/// [`build_engine`] with the state directory named, so a test can stand one in.
pub(crate) fn build_engine_in(
    cfg: &Config,
    events: Option<&tokio::sync::broadcast::Sender<String>>,
    state_dir: &std::path::Path,
) -> Result<Arc<Engine>> {
    refuse_unmigrated_store(cfg)?;
    // Every writer passes here, so this is where the state directory is closed if it was found open.
    private_dir(state_dir)?;
    let embedder = build_embedder(&cfg.embed_kind)?;
    let embed_dim = embedder.as_ref().map(|e| e.dimensions());
    let store: Arc<dyn KnowledgeStore> = match cfg.store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (non-persistent)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            let path = redb_path(&cfg.data_dir);
            let store = RedbStore::open(&path)
                .with_context(|| format!("failed to open redb store at {}", path.display()))?;
            // Two models share no vector space, so a swapped embedder is refused at open rather than
            // discovered later as quietly worse recall.
            if let Some(e) = &embedder {
                // Refused with the way on named (SR8): the store's own refusal cannot name commands.
                if let Some(recorded) = store.embedder()?.filter(|r| *r != e.id()) {
                    anyhow::bail!(embedder_refusal(&recorded, &e.id()));
                }
                store.set_embedder(&e.id())?;
            }
            tracing::info!(path = %path.display(), ?embed_dim, "store=redb (persistent)");
            Arc::new(store)
        }
    };
    // The ingest secret scan is on unless the operator turns it off (P17 defence in depth). Opt-out
    // rather than opt-in, because a miss cannot be undone: the log is append-only and it replicates.
    let scan = !matches!(
        std::env::var("SUPRAGNOSIS_SCAN_SECRETS").ok().as_deref(),
        Some("0") | Some("off") | Some("false")
    );
    if !scan {
        tracing::warn!(
            "SUPRAGNOSIS_SCAN_SECRETS=off - credential-shaped text will not be refused at ingest"
        );
    }
    let mut engine = Engine::new(store, cfg.host.clone(), cfg.workspace.clone())
        .with_session(cfg.session.clone())
        .with_secret_scan(scan);
    if let Some(e) = embedder {
        engine = engine.with_embedder(e);
    }
    if let Some(tx) = events {
        engine = engine.with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone())));
    }
    // crash-recovery.md K3: every caller of this function writes, so it repays the owed-projection
    // ledger here - before the caller binds a socket or accepts a request. A failure refuses to
    // start rather than serve a graph known to be behind its log (Principle 24).
    let recovery = engine.repay_owed().context(
        "repaying owed projections failed - the log holds writes the graph does not show, and \
         serving would answer \"not found\" for them",
    )?;
    if let Some(r) = recovery {
        tracing::warn!(
            workspaces = ?r.workspaces,
            observations = r.observations,
            "log rows were owed a projection - an interrupted write, or the first open by a build \
             that keeps the ledger - so their workspaces were re-projected before serving \
             (crash-recovery.md); `supragnosis status` reports it"
        );
    }
    Ok(Arc::new(engine))
}

#[cfg(test)]
mod legacy_store_guard_tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before the unix epoch")
            .as_nanos();
        let d = std::env::temp_dir().join(format!(
            "supragnosis-guard-{tag}-{}-{n}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    /// architecture.md's "0700 ~/.supragnosis" as behavior rather than description: a directory found
    /// open is closed, and a secret is written 0600 without ever existing at another mode - which is
    /// also why a rewrite leaves no temp file behind.
    #[test]
    fn the_state_directory_and_its_secrets_are_closed_to_other_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let dir = tmp("private");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        private_dir(&dir).expect("close");
        assert_eq!(mode(&dir), 0o700);

        let token = dir.join("mcp.token");
        write_secret(&token, b"first").expect("write");
        assert_eq!(mode(&token), 0o600);
        write_secret(&token, b"second").expect("rewrite");
        assert_eq!(std::fs::read(&token).unwrap(), b"second");
        assert_eq!(mode(&token), 0o600);
        let leftovers: Vec<_> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("mcp.token")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F14: an allowlist entry naming this node is reported and ignored, not fatal.
    ///
    /// The ignoring happens in `PeerDirectory` (pinned there, through both construction paths); this
    /// covers the operator-facing half - that the workaround is stated rather than done quietly.
    #[test]
    fn a_node_that_admits_itself_is_reported_and_ignored() {
        let entry = |id: &str| supragnosis_sync::http::AllowEntry {
            node_id: id.into(),
            public_key_hex: "deadbeef".into(),
            bearer_hash: "hash".into(),
            shared_workspaces: vec!["ws".into()],
        };
        let section = |ids: &[&str]| fed::ServerSection {
            listen: "127.0.0.1:7420".into(),
            tls_cert: None,
            tls_key: None,
            allowlist: ids.iter().map(|i| entry(i)).collect(),
            principals: Vec::new(),
        };

        assert!(
            drop_self_admission("self", None).is_empty(),
            "a client-only node has nothing to say"
        );
        assert!(drop_self_admission("self", Some(&section(&["peer-a", "peer-b"]))).is_empty());

        let notes = drop_self_admission("self", Some(&section(&["peer-a", "self"])));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("own id self"), "names what it found: {notes:?}");
        assert!(notes[0].contains("IGNORED"), "and says it was worked around: {notes:?}");
    }

    /// A RocksDB directory is recognised by `CURRENT`, which redb never writes. A directory that
    /// merely exists is not a store: an empty `~/.supragnosis/db` left behind after a completed
    /// migration must not block a clean start forever.
    ///
    /// The first version of this case built `dir/CURRENT` by hand, which is the shape the function
    /// assumed rather than the shape Cozo wrote. Both were wrong the same way, so the test passed
    /// while the guard matched no real store. It now builds the layout a live store has.
    #[test]
    fn a_legacy_store_is_recognised_by_its_rocksdb_marker() {
        let at = |d: &std::path::Path| legacy_cozo_store_at(d.to_str().expect("utf8")).is_some();

        let empty = tmp("detect-empty");
        assert!(!at(&empty), "an empty directory is not a store");

        // What the Cozo adapter actually produced: RocksDB under `data`, `manifest` alongside.
        let real = tmp("detect-real");
        std::fs::create_dir_all(real.join("data")).expect("data dir");
        std::fs::write(real.join("manifest"), b"storage_version").expect("manifest");
        std::fs::write(real.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        assert!(at(&real), "data/CURRENT is where a Cozo-era store keeps its marker");

        // A store moved or unpacked flat is still a store.
        let flat = tmp("detect-flat");
        std::fs::write(flat.join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        assert!(at(&flat), "a marker at the top level is accepted too");

        // And a redb directory is not mistaken for one, in either position.
        let redb = tmp("detect-redb");
        std::fs::write(redb.join("knowledge.redb"), b"redb").expect("redb file");
        assert!(!at(&redb), "redb writes no CURRENT anywhere");

        for d in [empty, real, flat, redb] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// The store a node was configured to use is the one the guard has to look at. Every deployment
    /// that set `SUPRAGNOSIS_DATA_DIR` - the systemd case - kept its knowledge outside the default,
    /// so a guard reading only the default would wave through the upgrade it exists to stop.
    #[test]
    fn the_guard_looks_where_the_node_was_configured_to_keep_its_data() {
        let dir = tmp("configured-legacy");
        std::fs::create_dir_all(dir.join("data")).expect("data dir");
        std::fs::write(dir.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");

        let cfg = Config {
            host: "h".into(),
            workspace: "default".into(),
            store_kind: "redb".into(),
            data_dir: dir.to_str().expect("utf8").to_string(),
            embed_kind: "none".into(),
            session: "s".into(),
            http: None,
            viz: None,
            mcp_auth: true,
        };
        // HOME is never read on this path: the configured dir answers first, so this case needs
        // none of the environment juggling the default-location case does.
        let err =
            refuse_unmigrated_store(&cfg).expect_err("a legacy store at data_dir must refuse");
        let msg = err.to_string();
        assert!(
            msg.contains(dir.to_str().expect("utf8")),
            "the refusal names the store it found: {msg}"
        );

        // Once the redb store exists the migration has run, and the leftover is the operator's.
        std::fs::write(redb_path(&cfg.data_dir), b"redb").expect("redb file");
        assert!(
            refuse_unmigrated_store(&cfg).is_ok(),
            "a migrated node starts with the old directory still present"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The refusal names both stores, says the knowledge still exists, and gives the exact release
    /// that can read it. An error that only said "cannot open" would leave the operator with a
    /// directory they cannot interpret and no path forward.
    ///
    /// This is what carries Principle 3's "every encoding the log has ever used stays readable" now
    /// that the adapter which read the older encodings is gone: they remain reachable through the
    /// release that wrote them, and skipping that step fails loudly instead of starting empty.
    #[test]
    fn an_unmigrated_store_is_refused_with_the_way_out() {
        let dir = tmp("refuse");
        std::fs::create_dir_all(dir.join("data")).expect("data dir");
        std::fs::write(dir.join("data").join("CURRENT"), b"MANIFEST-000001\n").expect("marker");
        // Point the default cozo location at the fixture, so the guard sees a legacy store.
        let home = tmp("home");
        std::fs::create_dir_all(home.join(".supragnosis")).expect("home");
        std::fs::rename(&dir, home.join(".supragnosis/db")).expect("place legacy store");
        let prev = std::env::var("HOME").ok();
        // SAFETY-free: set_var is safe on this edition; the guard reads HOME through default_data_dir_for.
        std::env::set_var("HOME", &home);

        let cfg = Config {
            host: "h".into(),
            workspace: "default".into(),
            store_kind: "redb".into(),
            data_dir: home.join(".supragnosis/redb").to_string_lossy().into_owned(),
            embed_kind: "none".into(),
            session: "s".into(),
            http: None,
            viz: None,
            mcp_auth: true,
        };
        let err = refuse_unmigrated_store(&cfg).expect_err("must refuse").to_string();
        assert!(err.contains("Cozo store"), "names what it found: {err}");
        assert!(err.contains("v0.1.21"), "names the release that can read it: {err}");
        assert!(err.contains("migrate-store"), "names the command: {err}");
        assert!(err.contains("not lost"), "says the knowledge survives: {err}");

        // A config still naming the dropped store must reach the same refusal. It used to skip the
        // guard and then open redb inside the legacy directory - the exact silent-empty-start this
        // guards against, reached by the one setting most likely to survive an upgrade.
        let stale = Config { store_kind: "cozo".into(), ..cfg.clone() };
        assert!(
            refuse_unmigrated_store(&stale).is_err(),
            "a stale store kind must not walk past the migration guard"
        );
        // The in-memory store touches no directory, so it is the one kind that legitimately skips.
        let mem = Config { store_kind: "mem".into(), ..cfg.clone() };
        assert!(
            refuse_unmigrated_store(&mem).is_ok(),
            "the in-memory store opens nothing on disk"
        );

        // Once the redb store exists the migration has run; the leftover directory must not block.
        std::fs::create_dir_all(home.join(".supragnosis/redb")).expect("redb dir");
        std::fs::write(redb_path(&cfg.data_dir), b"").expect("redb file");
        assert!(
            refuse_unmigrated_store(&cfg).is_ok(),
            "a rollback artifact beside a migrated store is not an error"
        );

        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[cfg(test)]
mod repayment {
    use super::*;

    /// crash-recovery.md K3: a writer process that opens a store with owed entries re-projects those
    /// workspaces before it serves a read or accepts a write. An append whose projection never ran is
    /// left in a scratch redb store; the engine every serving entry is handed comes back from
    /// `build_engine` with nothing owed and the entity in its graph - so whatever the caller then
    /// binds serves a graph that is not behind its log. Each serving entry takes its engine from
    /// there.
    #[test]
    fn a_daemon_repays_what_it_owes_before_it_serves() {
        let dir = std::env::temp_dir().join(format!("sg-repay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let prov = supragnosis_core::Provenance {
            host: "host-a".into(),
            on_behalf_of: None,
            workspace: "ws1".into(),
            source_ref: None,
            observed_at: 1,
            confidence: None,
            trust_tier: Default::default(),
            sync: None,
        };
        let assertions = supragnosis_core::Assertions {
            entities: vec![supragnosis_core::EntityAssertion {
                name: "Owed Entity".into(),
                kind: None,
                description: None,
            }],
            ..Default::default()
        };
        {
            let store = RedbStore::open(redb_path(data.to_str().unwrap())).expect("open");
            let obs = supragnosis_core::Observation::with_assertions(
                "appended, then the process died".into(),
                prov,
                assertions,
            );
            use supragnosis_core::AssertionStore;
            store.add_observation(obs).expect("append");
            use supragnosis_core::KnowledgeStore;
            assert_eq!(store.owed_projections().unwrap().len(), 1, "owed, as a crash leaves it");
        }
        let cfg = Config {
            host: "host-a".into(),
            workspace: "ws1".into(),
            store_kind: "redb".into(),
            data_dir: data.to_str().unwrap().into(),
            embed_kind: "none".into(),
            session: "s".into(),
            http: None,
            viz: None,
            mcp_auth: true,
        };
        let engine = build_engine_in(&cfg, None, &dir.join("state")).expect("the engine");
        assert_eq!(engine.owed_count().unwrap(), 0, "repaid before the engine is handed over");
        let graph = engine.graph(Some("ws1")).expect("graph");
        assert!(graph.nodes.iter().any(|n| n.name == "Owed Entity"), "the owed append is in it");
        drop(engine);
        let _ = std::fs::remove_dir_all(&dir);

        // The serving entries take the engine from build_engine and nowhere else.
        let app: String = MAIN_SOURCES
            .iter()
            .map(|src| &src[..src.find("#[cfg(test)]\nmod ").unwrap_or(src.len())])
            .collect();
        assert_eq!(app.matches("Engine::new(").count(), 1, "one place builds the engine");
        let run = &app[app.find("async fn run(cfg: Config)").expect("run")..];
        let run = &run[..run.find("\n}\n").expect("it closes")];
        assert!(
            run.contains("build_engine(&cfg"),
            "the daemon and stdio server take it from there"
        );
    }
}

/// sync-correctness.md Section 7: a key in another spelling is named and ignored, in both places a
/// configuration holds keys, and the canonical entries beside it are kept.
#[cfg(test)]
mod key_spelling {
    use super::*;

    #[test]
    fn a_key_in_another_spelling_is_named_and_ignored() {
        let good = "ab".repeat(32);
        let toml = format!(
            r#"
            [sync]
            share_workspaces = ["ws"]
            [sync.origin_keys]
            hub = "{good}"
            loud = "{upper}"

            [server]
            listen = "127.0.0.1:7420"
            [[server.allowlist]]
            node_id = "peer"
            public_key_hex = "{upper}"
            bearer_hash = "{good}"
            shared_workspaces = ["ws"]
            "#,
            upper = good.to_uppercase()
        );
        let fc: fed::FileConfig = toml::from_str(&toml).expect("parses");
        let notes = noncanonical_key_notes(&fc);
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].contains("peer") && notes[1].contains("loud"));
        let keys = trusted_origin_keys(&fc.sync);
        assert_eq!(keys.keys().collect::<Vec<_>>(), vec!["hub"]);
    }
}
