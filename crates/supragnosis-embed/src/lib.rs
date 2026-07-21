//! supragnosis-embed - embedding provider adapters.
//!
//! Implementations of the [`supragnosis_core::EmbeddingProvider`] port. The core
//! only knows about this port; the actual model is swapped in here (Principle 19:
//! probabilistic boundary).
//!
//! - [`HashingEmbedder`]: a deterministic embedder based on token feature-hashing.
//!   Zero external dependencies. The same text always maps to the same vector, and
//!   texts that share words map to high cosine similarity - used for offline
//!   development and recall regression tests (Principle 16: deterministic).
//! - [`FastEmbedProvider`](fastembed feature): a semantic embedder based on a local
//!   ONNX model.

use supragnosis_core::{EmbedError, EmbeddingProvider};

#[cfg(feature = "fastembed")]
mod fastembed_provider;
#[cfg(feature = "fastembed")]
pub use fastembed_provider::FastEmbedProvider;

/// Deterministic embedder based on token feature-hashing.
///
/// Splits text into lowercase alphanumeric tokens, hashes each token with FNV-1a,
/// accumulates term frequency into one of `dims` buckets, then L2-normalizes.
/// It is not a learned semantic embedding, but it reflects lexical overlap as
/// cosine similarity, which makes it a sufficient stand-in for deterministic,
/// reproducible search/recall tests.
#[derive(Debug, Clone)]
pub struct HashingEmbedder {
    dims: usize,
}

impl HashingEmbedder {
    /// An embedder with `dims` dimensions. 0 is clamped to 1.
    pub fn new(dims: usize) -> Self {
        Self { dims: dims.max(1) }
    }

    fn embed_text(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0f32; self.dims];
        for tok in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
        {
            let lower = tok.to_lowercase();
            let idx = (fnv1a(lower.as_bytes()) as usize) % self.dims;
            v[idx] += 1.0;
        }
        // L2 normalization (a zero vector is left as-is - cosine_similarity returns 0).
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

impl Default for HashingEmbedder {
    /// Default 256 dimensions - enough that hash collisions are rare in a small workspace.
    fn default() -> Self {
        Self::new(256)
    }
}

impl EmbeddingProvider for HashingEmbedder {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn id(&self) -> String {
        format!("hashing-{}", self.dims)
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| self.embed_text(t)).collect())
    }
}

/// FNV-1a 64-bit. Implemented directly to avoid the cross-version non-determinism
/// of the standard library hasher (Principle 16: no non-determinism in
/// projection/embedding).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::cosine_similarity;

    #[test]
    fn deterministic_and_lexically_meaningful() {
        let e = HashingEmbedder::default();
        assert_eq!(e.dimensions(), 256);

        // Determinism: same text -> same vector.
        let a1 = e.embed_one("the rust compiler is fast").unwrap();
        let a2 = e.embed_one("the rust compiler is fast").unwrap();
        assert_eq!(a1, a2);

        // Lexical overlap shows up as cosine similarity: more shared words means more similar.
        let shared = e.embed_one("rust compiler performance").unwrap();
        let unrelated = e.embed_one("banana smoothie recipe").unwrap();
        let sim_shared = cosine_similarity(&a1, &shared);
        let sim_unrelated = cosine_similarity(&a1, &unrelated);
        assert!(
            sim_shared > sim_unrelated,
            "shared-word text should rank higher: {sim_shared} vs {sim_unrelated}"
        );
    }
}
