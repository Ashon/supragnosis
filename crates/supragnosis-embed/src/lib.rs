//! supragnosis-embed - embedding provider adapters.
//!
//! Implementations of the [`supragnosis_core::EmbeddingProvider`] port. The core
//! only knows about this port; the actual model is swapped in here (Principle 19:
//! probabilistic boundary).
//!
//! - [`HashingEmbedder`]: a deterministic embedder based on token feature-hashing.
//!   Zero external dependencies. The same text always maps to the same vector, and
//!   texts that share words map to high cosine similarity - used for offline
//!   development and retrieval regression tests (Principle 16: deterministic).
//! - [`FastEmbedProvider`](fastembed feature): a semantic embedder based on a local
//!   ONNX model.

use std::sync::Mutex;

use supragnosis_core::{EmbedError, EmbeddingProvider, Readiness};

#[cfg(feature = "fastembed")]
mod fastembed_provider;
#[cfg(feature = "fastembed")]
pub use fastembed_provider::{model_cache_dir, FastEmbedProvider, Model};

/// What a [`LazyEmbedder`]'s loader found when asked for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The model's files are not in the cache. The loader is not asked again until they are
    /// (semantic-retrieval.md SR2); the string says what to run.
    Missing(String),
    /// The files are there and loading them failed. Not retried in this process.
    Failed(String),
}

/// Loads the real provider, from the cache only.
pub type Loader = Box<dyn Fn() -> Result<Box<dyn EmbeddingProvider>, LoadError> + Send + Sync>;
/// Whether the model's files are in the cache now.
pub type Present = Box<dyn Fn() -> bool + Send + Sync>;

enum Lazy {
    Unloaded,
    Loaded(Box<dyn EmbeddingProvider>),
    Missing(String),
    Failed(String),
}

/// A provider that loads its model on first use and remembers a load that found nothing
/// (semantic-retrieval.md Section 6). Its identity and dimensions are known without loading, so
/// the engine and the store's embedder guard see the model before it is in memory. A missing model
/// answers every embed with the same error, which the engine already treats as a degrade to
/// keyword retrieval, and [`EmbeddingProvider::readiness`] carries the reason to `status`.
pub struct LazyEmbedder {
    id: String,
    dims: usize,
    present: Present,
    loader: Loader,
    state: Mutex<Lazy>,
}

impl LazyEmbedder {
    pub fn new(id: impl Into<String>, dims: usize, present: Present, loader: Loader) -> Self {
        Self { id: id.into(), dims, present, loader, state: Mutex::new(Lazy::Unloaded) }
    }
}

impl EmbeddingProvider for LazyEmbedder {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // The lock is held through the load: a second call during it waits for the one load
        // rather than starting another.
        let mut st = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let attempt = match &*st {
            Lazy::Unloaded => true,
            // Files that were missing may have been fetched since (SR2).
            Lazy::Missing(_) => (self.present)(),
            Lazy::Loaded(_) | Lazy::Failed(_) => false,
        };
        if attempt {
            *st = match (self.loader)() {
                Ok(p) => Lazy::Loaded(p),
                Err(LoadError::Missing(note)) => Lazy::Missing(note),
                Err(LoadError::Failed(note)) => Lazy::Failed(note),
            };
        }
        match &*st {
            Lazy::Loaded(p) => p.embed(texts),
            Lazy::Missing(note) | Lazy::Failed(note) => Err(EmbedError::Provider(note.clone())),
            Lazy::Unloaded => unreachable!("an attempt was just made"),
        }
    }

    fn readiness(&self) -> Readiness {
        let st = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*st {
            Lazy::Loaded(_) => Readiness::Ready,
            Lazy::Unloaded if (self.present)() => Readiness::Ready,
            Lazy::Unloaded => Readiness::Degraded(match (self.loader)() {
                // Asked before any embed: say what the first embed will find, without keeping
                // the answer - the first embed decides.
                Ok(_) => return Readiness::Ready,
                Err(LoadError::Missing(n)) | Err(LoadError::Failed(n)) => n,
            }),
            Lazy::Missing(_) if (self.present)() => Readiness::Ready,
            Lazy::Missing(note) | Lazy::Failed(note) => Readiness::Degraded(note.clone()),
        }
    }
}

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
        for tok in text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()) {
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

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A lazy embedder over a counting loader: `present` says whether the files are there, the
    /// loader answers Missing while they are not and a hashing embedder once they are.
    fn lazy_over(present: Arc<AtomicBool>, loads: Arc<AtomicUsize>) -> LazyEmbedder {
        let p = present.clone();
        let loader: Loader = Box::new(move || {
            loads.fetch_add(1, Ordering::SeqCst);
            if p.load(Ordering::SeqCst) {
                Ok(Box::new(HashingEmbedder::new(4)) as Box<dyn EmbeddingProvider>)
            } else {
                Err(LoadError::Missing("no files - run supragnosis embed fetch".into()))
            }
        });
        let present_fn: Present = Box::new(move || present.load(Ordering::SeqCst));
        LazyEmbedder::new("test-model-4", 4, present_fn, loader)
    }

    /// SR1 and SR2: a missing model answers the degrade note, is asked for once and not again
    /// while the files are absent, says so to `readiness`, and loads on the next embed after the
    /// files appear.
    #[test]
    fn a_missing_model_degrades_to_keyword_and_says_why() {
        let present = Arc::new(AtomicBool::new(false));
        let loads = Arc::new(AtomicUsize::new(0));
        let e = lazy_over(present.clone(), loads.clone());
        assert_eq!((e.id().as_str(), e.dimensions()), ("test-model-4", 4), "known before loading");

        let err = e.embed(&["a"]).expect_err("missing");
        assert!(err.to_string().contains("embed fetch"), "{err}");
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        let _ = e.embed(&["b"]).expect_err("still missing");
        assert_eq!(loads.load(Ordering::SeqCst), 1, "not asked again while the files are absent");
        assert!(matches!(e.readiness(), Readiness::Degraded(n) if n.contains("embed fetch")));

        present.store(true, Ordering::SeqCst);
        assert_eq!(e.readiness(), Readiness::Ready, "files are there: ready, loads on first use");
        assert_eq!(e.embed(&["c"]).expect("loaded").len(), 1);
        assert_eq!(loads.load(Ordering::SeqCst), 2, "one more attempt, the one that loaded");
        assert_eq!(e.readiness(), Readiness::Ready);
    }

    /// SR2: the loader is not run at construction, runs once on the first embed, and not again;
    /// a load that failed with the files present is not retried in this process.
    #[test]
    fn the_model_loads_once_on_first_use() {
        let present = Arc::new(AtomicBool::new(true));
        let loads = Arc::new(AtomicUsize::new(0));
        let e = lazy_over(present, loads.clone());
        assert_eq!(loads.load(Ordering::SeqCst), 0, "nothing at construction");
        e.embed(&["a"]).expect("first");
        e.embed(&["b", "c"]).expect("second");
        assert_eq!(loads.load(Ordering::SeqCst), 1);

        let fails = Arc::new(AtomicUsize::new(0));
        let f = fails.clone();
        let loader: Loader = Box::new(move || {
            f.fetch_add(1, Ordering::SeqCst);
            Err(LoadError::Failed("the files did not load".into()))
        });
        let broken = LazyEmbedder::new("broken-4", 4, Box::new(|| true), loader);
        let _ = broken.embed(&["a"]).expect_err("failed");
        let _ = broken.embed(&["b"]).expect_err("failed");
        assert_eq!(fails.load(Ordering::SeqCst), 1, "a failed load is not retried");
        assert!(matches!(broken.readiness(), Readiness::Degraded(n) if n.contains("did not load")));
    }

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
