//! Embedding adapter based on fastembed (a local ONNX model). Compiled only under
//! the `fastembed` feature.
//!
//! The default model is BGE-small-en-v1.5 (384 dimensions). The model files are
//! downloaded to the cache directory on first use (network required). The core only
//! knows about the [`EmbeddingProvider`] port; if this adapter is absent or fails,
//! the system degrades to keyword search (Principle 19).

use std::path::PathBuf;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use supragnosis_core::{EmbedError, EmbeddingProvider};

/// BGE-small-en-v1.5 embedding dimensions.
const BGE_SMALL_EN_V15_DIMS: usize = 384;

/// Model cache directory. Pinned to a stable path so files do not scatter across the
/// working directory (CWD). Can be overridden with `SUPRAGNOSIS_MODEL_DIR`; defaults
/// to `~/.supragnosis/models`.
fn model_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SUPRAGNOSIS_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".supragnosis").join("models")
}

/// fastembed local ONNX embedder.
pub struct FastEmbedProvider {
    model: TextEmbedding,
    dims: usize,
}

impl FastEmbedProvider {
    /// Initializes with the default model (BGE-small-en-v1.5). Downloads the model if it is not in the cache.
    pub fn try_default() -> Result<Self, EmbedError> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_show_download_progress(false)
                .with_cache_dir(model_cache_dir()),
        )
        .map_err(|e| EmbedError::Provider(e.to_string()))?;
        Ok(Self {
            model,
            dims: BGE_SMALL_EN_V15_DIMS,
        })
    }
}

impl EmbeddingProvider for FastEmbedProvider {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn id(&self) -> String {
        format!("bge-small-en-v1.5-{}", self.dims)
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let docs: Vec<&str> = texts.to_vec();
        self.model
            .embed(docs, None)
            .map_err(|e| EmbedError::Provider(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::cosine_similarity;

    /// Requires a real model download/inference, so it is excluded from the default
    /// run (network/disk dependent).
    /// Manual verification: `cargo test -p supragnosis-embed --features fastembed -- --ignored`
    #[test]
    #[ignore = "downloads the model over the network - for manual verification"]
    fn real_model_produces_semantic_embeddings() {
        let e = FastEmbedProvider::try_default().expect("model init");
        assert_eq!(e.dimensions(), BGE_SMALL_EN_V15_DIMS);

        let v = e
            .embed(&["rust compiler", "python interpreter", "rust compiler"])
            .expect("embed");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].len(), BGE_SMALL_EN_V15_DIMS);

        // Determinism: same sentence -> effectively the same vector.
        assert!(cosine_similarity(&v[0], &v[2]) > 0.999);
        // Semantics: the same topic (rust-rust) is more similar than a different topic (rust-python).
        assert!(cosine_similarity(&v[0], &v[2]) > cosine_similarity(&v[0], &v[1]));
    }
}
