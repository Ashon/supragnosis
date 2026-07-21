//! Full M2 end-to-end verification: fastembed (ONNX) + Cozo HNSW (native ANN) + hybrid search.
//! Compiled only under the `fastembed` feature, and excluded from the default run because it requires a model download/inference.
//! Manual verification: `cargo test -p supragnosis-cli --features fastembed -- --ignored`
#![cfg(feature = "fastembed")]

use std::sync::Arc;

use supragnosis_core::EmbeddingProvider;
use supragnosis_embed::FastEmbedProvider;
use supragnosis_engine::{Engine, ObserveInput};
use supragnosis_store::CozoStore;

fn observe(engine: &Engine, content: &str) {
    engine
        .observe(ObserveInput {
            content: content.into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![],
            relations: vec![],
        })
        .unwrap();
}

#[test]
#[ignore = "fastembed model download + ONNX inference - for manual verification"]
fn fastembed_cozo_hnsw_end_to_end() {
    let embedder = Arc::new(FastEmbedProvider::try_default().expect("fastembed init"));
    let dim = embedder.dimensions();
    assert_eq!(dim, 384);

    let dir = std::env::temp_dir().join(format!("supragnosis-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store =
        Arc::new(CozoStore::open_with_embedder(&dir, &embedder.id(), dim).expect("open cozo"));
    let engine = Engine::new(store, "h", "ws").with_embedder(embedder);

    observe(
        &engine,
        "the rust compiler lowers code to native machine binaries",
    );
    observe(&engine, "a simple recipe for banana bread with walnuts");
    observe(
        &engine,
        "python is a dynamically typed interpreted language",
    );

    // The query is not a substring of any observation - it relies on purely semantic (embedding) recall.
    let hits = engine
        .search(
            "compiling systems code into executable binaries",
            Some("ws"),
            3,
        )
        .unwrap()
        .hits;
    assert!(!hits.is_empty(), "semantic search should return hits");
    assert!(
        hits[0].snippet.contains("rust"),
        "top semantic hit should be the compiler observation, got {:?}",
        hits.first()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
