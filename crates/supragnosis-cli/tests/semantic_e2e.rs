//! Full M2 종단 검증: fastembed(ONNX) + Cozo HNSW(네이티브 ANN) + 하이브리드 검색.
//! `fastembed` feature 에서만 컴파일되고, 모델 다운로드/추론이 필요해 기본 실행에서 제외한다.
//! 수동 검증: `cargo test -p supragnosis-cli --features fastembed -- --ignored`
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
#[ignore = "fastembed 모델 다운로드 + ONNX 추론 - 수동 검증용"]
fn fastembed_cozo_hnsw_end_to_end() {
    let embedder = Arc::new(FastEmbedProvider::try_default().expect("fastembed init"));
    let dim = embedder.dimensions();
    assert_eq!(dim, 384);

    let dir = std::env::temp_dir().join(format!("supragnosis-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Arc::new(CozoStore::open_with_embedding_dim(&dir, dim).expect("open cozo"));
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

    // 질의는 어느 관측의 부분문자열도 아니다 - 순수 의미(임베딩) 회상에 의존한다.
    let hits = engine.search(
        "compiling systems code into executable binaries",
        Some("ws"),
        3,
    );
    assert!(!hits.is_empty(), "semantic search should return hits");
    assert!(
        hits[0].snippet.contains("rust"),
        "top semantic hit should be the compiler observation, got {:?}",
        hits.first()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
