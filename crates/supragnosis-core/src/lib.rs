//! supragnosis-core - 도메인 모델 + 포트(트레이트).
//!
//! 이 크레이트는 **IO 의존이 없다**(순수 도메인). 저장소/임베딩/동기화 등 부수효과는
//! 여기 정의한 트레이트(포트)를 다른 크레이트가 어댑터로 구현한다.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// epoch millis.
pub type Timestamp = u64;

/// 현재 시각(epoch millis). M0는 노드 벽시계 사용 - 멀티호스트 결정적 순서(HLC)는 M4에서 도입.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 출처 신뢰 등급 (원칙 18: 쓰기는 공격 표면). 낮음 -> 높음.
/// **승격은 명시적 검증으로만** 일어난다 - 시간이 지났다고 저절로 오르지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    /// 미검증 - 외부/불명 출처.
    Unverified,
    /// 서명된 호스트의 에이전트가 추출/주장 (observe 기본값).
    #[default]
    AgentExtracted,
    /// 서명된 신뢰 호스트.
    HostSigned,
    /// 사람이 확인함.
    HumanConfirmed,
}

/// 출처(Provenance) - 모든 사실에 붙는 1급 시민 (원칙 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// 실제로 행동한 호스트 id (acting host).
    pub host: String,
    /// 위임 사슬(원칙 2): acting host 가 대리하는 principal (예: "ashon").
    /// 없으면 acting host 단독 - 신뢰 평가에서 그만큼 낮게 취급.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
    /// 작업 공간.
    pub workspace: String,
    /// 원본 참조(파일/URL/툴 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// 관측 시각 = **기록 시간(transaction time)** (원칙 4).
    pub observed_at: Timestamp,
    /// 신뢰도 0.0~1.0.
    pub confidence: f32,
    /// 신뢰 등급 (원칙 18). 기본 `AgentExtracted`, 승격은 명시적으로만.
    #[serde(default)]
    pub trust_tier: TrustTier,
}

/// 관측(Observation) - 진실의 원천. 불변이며 **콘텐츠 주소**로 식별된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    pub provenance: Provenance,
    /// 이 관측이 파생된 원천 관측 id들 (원칙 18: 오염 소독의 리콜 계보).
    /// 비어 있으면 1차 관측. (id 계산에는 포함하지 않는다 - 계보는 내용 정체성이 아니다.)
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// (선택) 의미 검색용 임베딩 벡터 (원칙 19: 확률적 경계).
    /// **콘텐츠 주소 id 계산에 포함하지 않는다** - 임베딩은 회상을 넓히는 로컬 보조일 뿐
    /// 내용 정체성이 아니며, 노드마다 다른 모델을 써도 정체성/수렴이 흔들리지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

impl Observation {
    /// 콘텐츠 주소 ID = blake3(workspace + content). 어떤 경로(서버/피어)로 들어와도 동일 id -> dedup.
    pub fn new(content: String, provenance: Provenance) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(provenance.workspace.as_bytes());
        hasher.update(b"\0");
        hasher.update(content.as_bytes());
        let id = hasher.finalize().to_hex().to_string();
        Self {
            id,
            content,
            provenance,
            derived_from: Vec::new(),
            embedding: None,
        }
    }
}

/// 엔티티(개념 노드).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub canonical_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
}

impl Entity {
    /// 결정적 엔티티 ID = blake3(workspace + normalized_name).
    /// M0 해소 규칙: 정규명 완전일치(대소문자/공백 정규화). 임베딩 유사도 해소는 M3.
    pub fn make_id(workspace: &str, canonical_name: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(workspace.as_bytes());
        hasher.update(b"\0");
        hasher.update(canonical_name.trim().to_lowercase().as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// 타입된 관계(엣지).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub provenance: Provenance,
    /// 유효시간(원칙 4): 관계가 세계에서 참인 구간의 시작.
    /// None = "관측 시점(provenance.observed_at)부터 반증 전까지".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Timestamp>,
    /// 유효시간 종료. 반증되면 삭제가 아니라 이 값을 세팅(supersede, 원칙 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

impl Relation {
    pub fn make_id(from: &str, kind: &str, to: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(from.as_bytes());
        hasher.update(b"\0");
        hasher.update(kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(to.as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// 검색 결과 한 건.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: SearchHitKind,
    pub id: String,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Entity,
    Observation,
}

/// 그래프 순회 결과 한 건 (시작 엔티티로부터 `depth` 홉 떨어진 엔티티).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraverseHit {
    pub id: String,
    pub depth: usize,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// 저장소 포트. in-memory / Cozo(RocksDB) 등 어댑터가 구현한다.
pub trait KnowledgeStore: Send + Sync {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    fn get_entity(&self, id: &str) -> Option<Entity>;
    /// entity.id 기준 upsert.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// from 또는 to 가 entity_id 인 관계들.
    fn relations_of(&self, entity_id: &str) -> Vec<Relation>;
    fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit>;
    /// start_id 에서 방향(from->to)을 따라 최대 `max_depth` 홉까지 도달하는 엔티티들.
    fn traverse(&self, start_id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit>;
    /// 임베딩이 있는 관측을 질의 벡터와의 코사인 유사도로 검색한다 (원칙 19: 회상 확장).
    /// 임베딩이 없는 관측은 후보에서 제외된다. `score` 는 코사인 유사도(-1.0~1.0).
    /// 기본 구현은 빈 결과 - 벡터를 저장하지 않는 어댑터는 재정의할 필요가 없다.
    fn search_semantic(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Vec<SearchHit> {
        Vec::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}

/// 임베딩 공급 포트 (원칙 19: 확률적 경계). 코어는 이 포트만 알고,
/// 실제 모델(fastembed/원격 등)은 교체 가능한 어댑터가 구현한다. 없으면 키워드 검색으로 degrade.
pub trait EmbeddingProvider: Send + Sync {
    /// 임베딩 벡터의 차원.
    fn dimensions(&self) -> usize;
    /// 텍스트 배치를 임베딩한다. 입력 순서와 출력 순서는 1:1 대응한다.
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    /// 단일 텍스트 임베딩 편의 메서드.
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut v = self.embed(&[text])?;
        v.pop()
            .ok_or_else(|| EmbedError::Provider("empty embedding result".into()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding provider error: {0}")]
    Provider(String),
}

/// 두 벡터의 코사인 유사도(-1.0~1.0). 길이가 다르거나 영벡터면 0.0.
/// 순수 함수 - InMemory 어댑터와 회상 평가가 공유한다.
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
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_basics() {
        // 동일 방향 = 1, 직교 = 0, 반대 = -1.
        assert!((cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        // 방어: 길이 불일치/영벡터는 0.0.
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}
