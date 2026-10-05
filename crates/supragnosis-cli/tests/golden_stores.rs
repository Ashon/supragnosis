//! Golden stores (docs/compatibility.md Section 5): a store a real release wrote, opened by this
//! build.
//!
//! Every other format test builds its data with the structs of the build under test, so a change to
//! an encoding passes them all - the old bytes and the new reader are never in the same test. These
//! are. Each `fixtures/stores/format-N.redb.gz` was written by the last release of format N, through
//! its own MCP surface and its own sync round (`fixtures/stores/make.sh`), and is read back here
//! through the engine. What that proves for the redb era is Principle 3's "every encoding the log has
//! ever used stays readable", and Principle 14's "an identity or a signature, once made, verifies
//! forever" (compatibility.md Section 4): every content id is recomputed and every signature
//! re-verified by the code of today.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use supragnosis_core::{
    observation_content_id, verify_attestation, AssertionStore, Entity, KnowledgeStore,
};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_store::{RedbStore, StoreFormat, FORMAT, MIN_READER};

/// What `make.sh` recorded beside the store: who wrote it, and the keys its signatures verify under.
struct Golden {
    store: PathBuf,
    written_by: String,
    origin_keys: BTreeMap<String, String>,
}

/// Unpacks `format-N` into a fresh directory, so a test opens (and upgrades) a copy and the fixture
/// itself is never written.
fn unpack(format: u32) -> Golden {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stores");
    let mut gz = flate2::read::GzDecoder::new(
        std::fs::File::open(dir.join(format!("format-{format}.redb.gz"))).expect("fixture"),
    );
    let mut bytes = Vec::new();
    gz.read_to_end(&mut bytes).expect("gunzip");
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join(format!("format-{format}.json"))).expect("fixture json"),
    )
    .expect("parse");
    assert_eq!(meta["format"], format, "the json describes this store");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let work = std::env::temp_dir()
        .join(format!("supragnosis-golden-{format}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&work).expect("workdir");
    let store = work.join("knowledge.redb");
    std::fs::write(&store, bytes).expect("write copy");
    Golden {
        store,
        written_by: meta["written_by"].as_str().expect("written_by").to_string(),
        origin_keys: meta["origin_keys"]
            .as_object()
            .expect("origin_keys")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("key").to_string()))
            .collect(),
    }
}

/// v0.4.7 wrote format 2 with no record of it. This build reads it, dates it by its structure,
/// records the date, and reads every row back with the encodings of today.
#[test]
fn the_format_2_store_v0_4_7_wrote_reads_whole() {
    let golden = unpack(2);
    assert_eq!(golden.written_by, "0.4.7");
    let store = RedbStore::open(&golden.store).expect("this build opens what v0.4.7 wrote");
    assert_eq!(
        store.format().expect("format"),
        StoreFormat {
            format: FORMAT.max(2),
            min_reader: MIN_READER.max(2),
            format_by: Some(env!("CARGO_PKG_VERSION").to_string()),
        },
        "an open records the store's era (compatibility.md Section 3.4)"
    );
    assert!(
        !golden.store.with_file_name("knowledge.redb.format-2").exists(),
        "format 2 to format 2 raises nothing, so nothing is copied"
    );

    // Every row parses, and every content id is still the hash of what it names.
    let log = store.all_observations(None).expect("the log reads");
    // make.sh: three observations, a type definition and a proposal on the spoke (each its own log
    // row), and one observation pulled from the hub.
    assert_eq!(log.len(), 6, "every row v0.4.7 wrote is enumerated, none dropped");
    for obs in &log {
        let ws = &obs.provenance[0].workspace;
        assert_eq!(
            observation_content_id(ws, &obs.content, &obs.assertions),
            obs.id,
            "the content id v0.4.7 computed is the one this build computes"
        );
    }

    // Every signature verifies under today's signing bytes - the spoke's own, and the hub's.
    let mut signed_by: BTreeMap<&str, usize> = BTreeMap::new();
    for obs in &log {
        for p in &obs.provenance {
            let Some(meta) = &p.sync else { continue };
            let key = golden.origin_keys.get(&meta.origin_node).expect("a known origin");
            assert!(
                verify_attestation(key, &obs.id, p, meta),
                "a signature v0.4.7 made verifies under this build's signing bytes ({})",
                obs.id
            );
            *signed_by.entry(meta.origin_node.as_str()).or_default() += 1;
        }
    }
    assert_eq!(signed_by.len(), 2, "attestations signed by both nodes: {signed_by:?}");

    // The engine reads the projection, the T-Box and the proposal fold out of it.
    let store: Arc<dyn KnowledgeStore> = Arc::new(store);
    let engine =
        Engine::new(store, "golden", "shared").with_embedder(Arc::new(HashingEmbedder::default()));
    engine.repay_owed().expect("nothing in the ledger fails to repay");

    let supragnosis = engine
        .get_entity(&Entity::make_id("shared", "supragnosis"))
        .expect("read")
        .expect("the entity the spoke asserted");
    assert_eq!(
        supragnosis.entity.description.as_deref(),
        Some("A local-first knowledge server.")
    );
    assert!(supragnosis.relations.iter().any(|r| r.kind == "uses"), "its relation projected");
    assert!(
        engine.get_entity(&Entity::make_id("shared", "hub")).expect("read").is_some(),
        "the entity pulled from the hub"
    );

    let types = engine.types(Some("shared")).expect("types");
    assert!(types.iter().any(|t| t.name == "Technology"), "the type definition");

    let proposals = engine.list_proposals(Some("shared")).expect("proposals");
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        (proposals[0].kind.as_str(), proposals[0].state.as_str()),
        ("entity_merge", "open")
    );

    let found = engine.search("embedded key-value store", Some("shared"), 5).expect("search");
    assert!(!found.hits.is_empty(), "the log is searchable");

    let _ = std::fs::remove_dir_all(golden.store.parent().expect("workdir"));
}
