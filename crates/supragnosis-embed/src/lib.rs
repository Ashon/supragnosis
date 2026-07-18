//! supragnosis-embed - 임베딩 공급 어댑터.
//!
//! [`supragnosis_core::EmbeddingProvider`] 포트의 구현. 코어는 이 포트만 알고
//! 실제 모델은 여기서 교체된다 (원칙 19: 확률적 경계).
//!
//! - [`HashingEmbedder`]: 토큰 feature-hashing 기반 결정적 임베더. 외부 의존 0.
//!   같은 텍스트는 항상 같은 벡터로, 단어를 공유하는 텍스트는 높은 코사인 유사도로
//!   매핑된다 - 오프라인 개발과 회상 회귀 테스트(원칙 16: 결정적)에 쓴다.
//! - [`FastEmbedProvider`](fastembed feature): ONNX 로컬 모델 기반 의미 임베더.

use supragnosis_core::{EmbedError, EmbeddingProvider};

#[cfg(feature = "fastembed")]
mod fastembed_provider;
#[cfg(feature = "fastembed")]
pub use fastembed_provider::FastEmbedProvider;

/// 토큰 feature-hashing 기반 결정적 임베더.
///
/// 텍스트를 소문자 alphanumeric 토큰으로 쪼개고, 각 토큰을 FNV-1a 로 해시해
/// `dims` 개 버킷 중 하나에 term-frequency 를 누적한 뒤 L2 정규화한다.
/// 학습된 의미 임베딩은 아니지만 어휘 중첩을 코사인 유사도로 반영하므로,
/// 결정적이면서 재현 가능한 검색/회상 테스트의 스탠드인으로 충분하다.
#[derive(Debug, Clone)]
pub struct HashingEmbedder {
    dims: usize,
}

impl HashingEmbedder {
    /// `dims` 차원의 임베더. 0 은 1 로 클램프된다.
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
        // L2 정규화 (영벡터는 그대로 - cosine_similarity 가 0 을 돌려준다).
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
    /// 기본 256 차원 - 소규모 워크스페이스에서 해시 충돌이 드물 만큼.
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

/// FNV-1a 64bit. 표준 라이브러리 해셔의 버전 간 비결정성을 피하려 직접 구현한다
/// (원칙 16: 프로젝션/임베딩에 비결정성 금지).
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

        // 결정성: 같은 텍스트 -> 같은 벡터.
        let a1 = e.embed_one("the rust compiler is fast").unwrap();
        let a2 = e.embed_one("the rust compiler is fast").unwrap();
        assert_eq!(a1, a2);

        // 어휘 중첩이 코사인 유사도로 나타난다: 공유 단어가 많을수록 유사.
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
