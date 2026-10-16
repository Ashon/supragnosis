//! Embedding adapter based on fastembed (a local ONNX model). Compiled only under the `fastembed`
//! feature. docs/semantic-retrieval.md is the design.
//!
//! The provider handed to the engine is a [`LazyEmbedder`]: the model's identity and dimensions
//! are known from its name, and the weights load on the first embed, from the cache only. The
//! daemon never downloads (SR1); [`FastEmbedProvider::fetch`] does, when a person or the installer
//! asks. If the adapter cannot load, the engine degrades to keyword search (Principle 19) and the
//! reason reaches `status` through [`EmbeddingProvider::readiness`].

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use supragnosis_core::{EmbedError, EmbeddingProvider};

use crate::{LazyEmbedder, LoadError};

/// The models the prebuilt binary can run (semantic-retrieval.md Section 5). Both are 384
/// dimensions, so a store's vector index has one width whichever is chosen; the store still
/// refuses to mix them, since their spaces differ (SR4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// paraphrase-multilingual-MiniLM-L12-v2, quantized. The default: the one that works on a
    /// Korean and English store (the A/B in Section 2).
    Multilingual,
    /// BGE-small-en-v1.5. English only, half the download and a third of the memory.
    BgeSmallEn,
}

impl Model {
    pub const DEFAULT: Model = Model::Multilingual;

    /// The two names `SUPRAGNOSIS_EMBED_MODEL` takes; empty means the default. Anything else is
    /// refused naming both (Principle 24: an unknown value is a refusal, as an unknown key is).
    pub fn parse(value: &str) -> Result<Model, String> {
        match value.trim() {
            "" | "multilingual" => Ok(Model::Multilingual),
            "bge-small-en" => Ok(Model::BgeSmallEn),
            other => Err(format!(
                "SUPRAGNOSIS_EMBED_MODEL={other:?} names no model - it is `multilingual` (the \
                 default) or `bge-small-en`"
            )),
        }
    }

    /// The configuration value that names this model.
    pub fn name(self) -> &'static str {
        match self {
            Model::Multilingual => "multilingual",
            Model::BgeSmallEn => "bge-small-en",
        }
    }

    /// The embedder identity the store records (model and dimensions, SR4).
    pub fn id(self) -> &'static str {
        match self {
            Model::Multilingual => "paraphrase-multilingual-minilm-l12-v2-q-384",
            Model::BgeSmallEn => "bge-small-en-v1.5-384",
        }
    }

    pub fn dims(self) -> usize {
        384
    }

    /// What the first fetch downloads, for the messages that warn about it (measured 2026-10-16).
    pub fn download_mb(self) -> u32 {
        match self {
            Model::Multilingual => 224,
            Model::BgeSmallEn => 129,
        }
    }

    fn fastembed(self) -> EmbeddingModel {
        match self {
            Model::Multilingual => EmbeddingModel::ParaphraseMLMiniLML12V2Q,
            Model::BgeSmallEn => EmbeddingModel::BGESmallENV15,
        }
    }

    /// The Hugging Face repository the files come from, which names their directory in the cache.
    fn repo(self) -> &'static str {
        match self {
            Model::Multilingual => "Qdrant/paraphrase-multilingual-MiniLM-L12-v2-onnx-Q",
            Model::BgeSmallEn => "Xenova/bge-small-en-v1.5",
        }
    }

    /// Where this model's files sit under the cache directory (hf-hub's layout).
    pub fn cache_path(self, cache: &Path) -> PathBuf {
        cache.join(format!("models--{}", self.repo().replace('/', "--")))
    }

    /// Whether the weights are in the cache: a file over a megabyte under the model's directory.
    /// The tokenizer and the config are small; only the weights are worth a download. Symlinks are
    /// skipped: hf-hub keeps the files once under `blobs` and links them from `snapshots`.
    pub fn present(self, cache: &Path) -> bool {
        fn has_large_file(dir: &Path) -> bool {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return false;
            };
            entries.flatten().any(|e| {
                let p = e.path();
                let Ok(meta) = std::fs::symlink_metadata(&p) else {
                    return false;
                };
                if meta.is_dir() {
                    has_large_file(&p)
                } else {
                    meta.is_file() && meta.len() > 1 << 20
                }
            })
        }
        has_large_file(&self.cache_path(cache))
    }
}

/// Model cache directory: `~/.supragnosis/models`, inside the state directory so removing it
/// removes everything. `SUPRAGNOSIS_MODEL_DIR` overrides it.
pub fn model_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SUPRAGNOSIS_MODEL_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".supragnosis").join("models")
}

/// The loaded model. Behind a mutex because fastembed 7 made `TextEmbedding::embed` take `&mut
/// self`, while the port is `&self` and `Send + Sync` (the engine shares one provider across
/// tool calls). That serializes embedding calls; an embed is one short ONNX run, observe embeds
/// outside the write guard, and the daemon serves one principal, so a queue of two is the
/// realistic worst case.
struct Loaded {
    model: Mutex<TextEmbedding>,
    dims: usize,
    id: &'static str,
}

impl EmbeddingProvider for Loaded {
    fn dimensions(&self) -> usize {
        self.dims
    }

    fn id(&self) -> String {
        self.id.to_string()
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let docs: Vec<&str> = texts.to_vec();
        // A poisoned lock means an earlier embed panicked mid-run. The session holds no per-call
        // state, so the model is still usable - recover it rather than turning one panic into every
        // later embed failing, which would degrade the node to keyword search for good.
        let mut model = self.model.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        model.embed(docs, None).map_err(|e| EmbedError::Provider(e.to_string()))
    }
}

/// The fastembed adapter's two entry points: the lazy provider for the engine, and the fetch.
pub struct FastEmbedProvider;

impl FastEmbedProvider {
    /// The provider the engine gets: identity and dimensions now, the weights on first use, from
    /// `cache` only (SR1, SR2).
    pub fn lazy(model: Model, cache: PathBuf) -> LazyEmbedder {
        let present_cache = cache.clone();
        let present = Box::new(move || model.present(&present_cache));
        let loader = Box::new(move || Self::load(model, &cache));
        LazyEmbedder::new(model.id(), model.dims(), present, loader)
    }

    /// Loads the model from the cache. Never downloads: files that are not there are
    /// [`LoadError::Missing`] with the command that fetches them.
    fn load(model: Model, cache: &Path) -> Result<Box<dyn EmbeddingProvider>, LoadError> {
        if !model.present(cache) {
            return Err(LoadError::Missing(format!(
                "semantic retrieval is off: the embedding model {} is not in {} - run `supragnosis \
                 embed fetch` once ({} MB); retrieval is keyword only until then",
                model.name(),
                cache.display(),
                model.download_mb()
            )));
        }
        let options = TextInitOptions::new(model.fastembed())
            .with_show_download_progress(false)
            .with_cache_dir(cache.to_path_buf());
        match TextEmbedding::try_new(options) {
            Ok(m) => {
                Ok(Box::new(Loaded { model: Mutex::new(m), dims: model.dims(), id: model.id() }))
            }
            Err(e) => Err(LoadError::Failed(format!(
                "semantic retrieval is off: the embedding model {} in {} did not load ({e}) - \
                 `supragnosis embed fetch` downloads it again; restart the daemon afterwards",
                model.name(),
                cache.display()
            ))),
        }
    }

    /// Downloads the model into `cache` (and loads it once, which is how fastembed fetches), with
    /// progress on the terminal when asked. Idempotent: files that are there are not fetched
    /// again. Returns the bytes the model's directory holds afterwards.
    pub fn fetch(model: Model, cache: &Path, progress: bool) -> Result<u64, EmbedError> {
        std::fs::create_dir_all(cache)
            .map_err(|e| EmbedError::Provider(format!("creating {}: {e}", cache.display())))?;
        let options = TextInitOptions::new(model.fastembed())
            .with_show_download_progress(progress)
            .with_cache_dir(cache.to_path_buf());
        TextEmbedding::try_new(options).map_err(|e| EmbedError::Provider(e.to_string()))?;
        Ok(dir_size(&model.cache_path(cache)))
    }
}

/// Bytes under `dir`, counting each file once: the links hf-hub keeps under `snapshots` point at
/// the `blobs` that are already counted.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => dir_size(&p),
                Ok(m) if m.is_file() => m.len(),
                _ => 0,
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::{cosine_similarity, Readiness};

    /// Section 5: the two names map to their identities and dimensions, the empty value is the
    /// default, and any other value is refused naming both.
    #[test]
    fn the_model_choice_is_parsed_and_identified() {
        assert_eq!(Model::parse(""), Ok(Model::Multilingual));
        assert_eq!(Model::parse("multilingual"), Ok(Model::Multilingual));
        assert_eq!(Model::parse(" bge-small-en "), Ok(Model::BgeSmallEn));
        assert_eq!(Model::DEFAULT.id(), "paraphrase-multilingual-minilm-l12-v2-q-384");
        assert_eq!(Model::BgeSmallEn.id(), "bge-small-en-v1.5-384");
        assert!(Model::Multilingual.dims() == 384 && Model::BgeSmallEn.dims() == 384);
        let err = Model::parse("gpt").expect_err("refused");
        assert!(err.contains("multilingual") && err.contains("bge-small-en"), "{err}");
        let cache = Path::new("/x/models");
        assert_eq!(
            Model::Multilingual.cache_path(cache),
            Path::new("/x/models/models--Qdrant--paraphrase-multilingual-MiniLM-L12-v2-onnx-Q")
        );
    }

    /// SR1: with no files in the cache the lazy provider never reaches the network - it answers
    /// the degrade note naming the fetch command, and reports it before any embed. A directory
    /// with the weights counts as present; one with only small files does not.
    #[test]
    fn an_empty_cache_is_missing_and_never_fetched() {
        let dir =
            std::env::temp_dir().join(format!("supragnosis-embed-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let e = FastEmbedProvider::lazy(Model::Multilingual, dir.clone());
        assert_eq!(e.id(), Model::Multilingual.id());
        assert_eq!(e.dimensions(), 384);
        let Readiness::Degraded(note) = e.readiness() else {
            panic!("an empty cache is a degrade");
        };
        assert!(note.contains("supragnosis embed fetch") && note.contains("224 MB"), "{note}");
        let err = e.embed(&["x"]).expect_err("no files, no vectors");
        assert!(err.to_string().contains("embed fetch"), "{err}");
        assert!(!dir.exists(), "nothing was downloaded or created");

        let small = Model::Multilingual.cache_path(&dir).join("snapshots/a");
        std::fs::create_dir_all(&small).unwrap();
        std::fs::write(small.join("config.json"), "{}").unwrap();
        assert!(!Model::Multilingual.present(&dir), "small files are not the weights");
        std::fs::write(small.join("model_optimized.onnx"), vec![0u8; (1 << 20) + 1]).unwrap();
        assert!(Model::Multilingual.present(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Requires a real model download and inference, so it is excluded from the default run.
    /// Manual verification: `cargo test -p supragnosis-embed --features fastembed -- --ignored`
    #[test]
    #[ignore = "downloads the model over the network - for manual verification"]
    fn real_model_produces_semantic_embeddings() {
        let cache = model_cache_dir();
        FastEmbedProvider::fetch(Model::DEFAULT, &cache, false).expect("fetch");
        let e = FastEmbedProvider::lazy(Model::DEFAULT, cache);
        assert_eq!(e.dimensions(), 384);

        let v = e
            .embed(&["rust compiler", "python interpreter", "rust compiler"])
            .expect("embed");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].len(), 384);

        // Determinism: same sentence -> effectively the same vector.
        assert!(cosine_similarity(&v[0], &v[2]) > 0.999);
        // Semantics: the same topic (rust-rust) is more similar than a different topic (rust-python).
        assert!(cosine_similarity(&v[0], &v[2]) > cosine_similarity(&v[0], &v[1]));
        assert_eq!(e.readiness(), Readiness::Ready);
    }
}
