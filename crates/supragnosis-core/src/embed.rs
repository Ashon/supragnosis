//! The embedding port (Principle 19), its readiness and error, and cosine similarity.

/// Embedding provider port (Principle 19: probabilistic boundary). The core knows only this port,
/// and the actual model (fastembed/remote, etc.) is implemented by a swappable adapter. Without one, it degrades to keyword search.
pub trait EmbeddingProvider: Send + Sync {
    /// Dimension of the embedding vector.
    fn dimensions(&self) -> usize;
    /// Stable identifier of the embedder (model name + dimension, e.g. "hashing-256", "bge-small-en-v1.5-384").
    /// The store records it alongside the vector index to detect a swap that reopens with a different embedder -
    /// because a different model means a different vector space, so mixing old/new vectors in one index makes
    /// similarity meaningless (Principle 19: an adapter swap must not harm core correctness).
    fn id(&self) -> String;
    /// Embeds a batch of texts. Input order and output order correspond 1:1.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    /// Convenience method for embedding a single text.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut v = self.embed(&[text])?;
        v.pop().ok_or_else(|| EmbedError::Provider("empty embedding result".into()))
    }
    /// Whether this provider can answer in this process. A provider that loads on first use is
    /// ready while its files are there; one that cannot load says why and what would change it,
    /// so a degrade to keyword retrieval is stated, never silent (Principle 24).
    fn readiness(&self) -> Readiness {
        Readiness::Ready
    }
}

/// What an [`EmbeddingProvider`] says about itself when asked whether it can answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// Loaded, or loads on first use from files that are there.
    Ready,
    /// Cannot answer in this process; the string says why and what to run.
    Degraded(String),
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding provider error: {0}")]
    Provider(String),
}

/// Cosine similarity of two vectors (-1.0~1.0). 0.0 if the lengths differ, a vector is a zero
/// vector, or the result is not finite - a NaN or infinite component, or a norm that overflows.
/// Every similarity ranking sorts on this score with `partial_cmp`, where a NaN compares equal to
/// everything and lands wherever the sort leaves it, first included; so no caller sees one.
/// A pure function - shared by both store adapters, the merge band, and recall evaluation.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    let sim = dot / (na.sqrt() * nb.sqrt());
    if sim.is_finite() {
        sim
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_basics() {
        // Same direction = 1, orthogonal = 0, opposite = -1.
        assert!((cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        // Defensive: length mismatch / zero vector is 0.0.
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        // A vector no ranking can order is no similarity: NaN compares equal to every score, so
        // one bad vector would otherwise sort wherever the sort left it - first, in practice.
        for bad in [[f32::NAN, 1.0], [f32::INFINITY, 1.0], [f32::MAX, f32::MAX]] {
            assert_eq!(cosine_similarity(&bad, &[1.0, 1.0]), 0.0, "{bad:?}");
            assert_eq!(cosine_similarity(&[1.0, 1.0], &bad), 0.0, "{bad:?}");
        }
    }
}
