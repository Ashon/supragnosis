//! Embedder factory for evals - used by the EVAL_EMBEDDERS axis (hashing vs fastembed A/B).
//!
//! "fastembed" is a real model (bge-small-en-v1.5, ONNX), so it needs a `--features real-embed`
//! build. It isolates, on the same fixture, the recall-quality difference between lexical hashing
//! and semantic embedding.

use std::sync::Arc;

use supragnosis_core::EmbeddingProvider;
use supragnosis_embed::HashingEmbedder;

/// Builds an embedder by name. The returned Arc may be shared across several engines via clone
/// (so the real-model initialization cost is paid only once).
pub fn make_embedder(name: &str) -> Arc<dyn EmbeddingProvider> {
    match name {
        "hashing" => Arc::new(HashingEmbedder::default()),
        #[cfg(feature = "real-embed")]
        "fastembed" => Arc::new(
            supragnosis_embed::FastEmbedProvider::try_default().expect("fastembed initialization"),
        ),
        #[cfg(not(feature = "real-embed"))]
        "fastembed" => panic!(
            "the fastembed embedder must be built with `--features real-embed` \
             (e.g. cargo test -p supragnosis-e2e --features real-embed ...)"
        ),
        other => panic!("unknown embedder: {other} (hashing | fastembed)"),
    }
}
