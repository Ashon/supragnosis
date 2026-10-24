//! Embedder factory for evals - used by the EVAL_EMBEDDERS axis (hashing vs fastembed A/B).
//!
//! "fastembed" is the release build's real model (the default `Model`, ONNX, from the local model
//! cache), so it needs a `--features real-embed` build and `supragnosis embed fetch` once. It
//! isolates, on the same fixture, the recall-quality difference between lexical hashing and
//! semantic embedding.

use std::sync::Arc;

use supragnosis_core::EmbeddingProvider;
use supragnosis_embed::HashingEmbedder;

/// Builds an embedder by name. The returned Arc may be shared across several engines via clone
/// (so the real-model initialization cost is paid only once).
pub fn make_embedder(name: &str) -> Arc<dyn EmbeddingProvider> {
    match name {
        "hashing" => Arc::new(HashingEmbedder::default()),
        #[cfg(feature = "real-embed")]
        "fastembed" => Arc::new(supragnosis_embed::FastEmbedProvider::lazy(
            supragnosis_embed::Model::DEFAULT,
            supragnosis_embed::model_cache_dir(),
        )),
        #[cfg(not(feature = "real-embed"))]
        "fastembed" => panic!(
            "the fastembed embedder must be built with `--features real-embed` \
             (e.g. cargo test -p supragnosis-e2e --features real-embed ...)"
        ),
        other => panic!("unknown embedder: {other} (hashing | fastembed)"),
    }
}
