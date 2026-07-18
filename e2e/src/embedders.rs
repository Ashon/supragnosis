//! eval 용 임베더 팩토리 - EVAL_EMBEDDERS 축(hashing vs fastembed A/B)에 쓴다.
//!
//! "fastembed" 는 실모델(bge-small-en-v1.5, ONNX)이라 `--features real-embed` 빌드가
//! 필요하다. 어휘 해싱과 의미 임베딩의 회수 품질 차이를 같은 픽스처에서 격리 측정한다.

use std::sync::Arc;

use supragnosis_core::EmbeddingProvider;
use supragnosis_embed::HashingEmbedder;

/// 이름으로 임베더를 만든다. 반환 Arc 는 여러 엔진에 clone 으로 공유해도 된다
/// (실모델 초기화 비용을 한 번만 치르기 위해서다).
pub fn make_embedder(name: &str) -> Arc<dyn EmbeddingProvider> {
    match name {
        "hashing" => Arc::new(HashingEmbedder::default()),
        #[cfg(feature = "real-embed")]
        "fastembed" => Arc::new(
            supragnosis_embed::FastEmbedProvider::try_default().expect("fastembed 초기화"),
        ),
        #[cfg(not(feature = "real-embed"))]
        "fastembed" => panic!(
            "fastembed 임베더는 `--features real-embed` 로 빌드해야 한다 \
             (예: cargo test -p supragnosis-e2e --features real-embed ...)"
        ),
        other => panic!("알 수 없는 임베더: {other} (hashing | fastembed)"),
    }
}
