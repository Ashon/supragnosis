//! fastembed(ONNX 로컬 모델) 기반 임베딩 어댑터. `fastembed` feature 에서만 컴파일된다.
//!
//! 기본 모델은 BGE-small-en-v1.5(384차원). 모델 파일은 첫 사용 시 캐시 디렉터리로
//! 내려받는다(네트워크 필요). 코어는 [`EmbeddingProvider`] 포트만 알고, 이 어댑터가
//! 없거나 실패하면 시스템은 키워드 검색으로 degrade 한다(원칙 19).

use std::path::PathBuf;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use supragnosis_core::{EmbedError, EmbeddingProvider};

/// BGE-small-en-v1.5 임베딩 차원.
const BGE_SMALL_EN_V15_DIMS: usize = 384;

/// 모델 캐시 디렉터리. 실행 위치(CWD)에 흩어지지 않도록 안정 경로로 고정한다.
/// `SUPRAGNOSIS_MODEL_DIR` 로 재지정 가능, 기본은 `~/.supragnosis/models`.
fn model_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SUPRAGNOSIS_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".supragnosis").join("models")
}

/// fastembed 로컬 ONNX 임베더.
pub struct FastEmbedProvider {
    model: TextEmbedding,
    dims: usize,
}

impl FastEmbedProvider {
    /// 기본 모델(BGE-small-en-v1.5)로 초기화한다. 모델이 캐시에 없으면 내려받는다.
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

    /// 실제 모델 다운로드/추론이 필요하므로 기본 실행에서 제외한다(네트워크/디스크 의존).
    /// 수동 검증: `cargo test -p supragnosis-embed --features fastembed -- --ignored`
    #[test]
    #[ignore = "네트워크로 모델을 내려받는다 - 수동 검증용"]
    fn real_model_produces_semantic_embeddings() {
        let e = FastEmbedProvider::try_default().expect("model init");
        assert_eq!(e.dimensions(), BGE_SMALL_EN_V15_DIMS);

        let v = e
            .embed(&["rust compiler", "python interpreter", "rust compiler"])
            .expect("embed");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].len(), BGE_SMALL_EN_V15_DIMS);

        // 결정성: 같은 문장 -> 사실상 같은 벡터.
        assert!(cosine_similarity(&v[0], &v[2]) > 0.999);
        // 의미: 동일 주제(rust-rust)가 이질 주제(rust-python)보다 유사.
        assert!(cosine_similarity(&v[0], &v[2]) > cosine_similarity(&v[0], &v[1]));
    }
}
