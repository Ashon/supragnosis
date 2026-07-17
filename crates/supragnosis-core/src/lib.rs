//! supragnosis-core — 도메인 모델 + 포트(트레이트).
//!
//! 이 크레이트는 **IO 의존이 없다**(순수 도메인). 저장소/임베딩/동기화 등 부수효과는
//! 여기 정의한 트레이트(포트)를 다른 크레이트가 어댑터로 구현한다.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// epoch millis.
pub type Timestamp = u64;

/// 현재 시각(epoch millis). M0는 노드 벽시계 사용 — 멀티호스트 결정적 순서(HLC)는 M4에서 도입.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 출처(Provenance) — 모든 사실에 붙는 1급 시민.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// 관측을 만든 호스트 id.
    pub host: String,
    /// 작업 공간.
    pub workspace: String,
    /// 원본 참조(파일/URL/툴 등).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    /// 관측 시각.
    pub observed_at: Timestamp,
    /// 신뢰도 0.0~1.0.
    pub confidence: f32,
}

/// 관측(Observation) — 진실의 원천. 불변이며 **콘텐츠 주소**로 식별된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    pub provenance: Provenance,
}

impl Observation {
    /// 콘텐츠 주소 ID = blake3(workspace ⊕ content). 어떤 경로(서버/피어)로 들어와도 동일 id → dedup.
    pub fn new(content: String, provenance: Provenance) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(provenance.workspace.as_bytes());
        hasher.update(b"\0");
        hasher.update(content.as_bytes());
        let id = hasher.finalize().to_hex().to_string();
        Self { id, content, provenance }
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
    /// 결정적 엔티티 ID = blake3(workspace ⊕ normalized_name).
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Entity,
    Observation,
}

/// 저장소 포트. in-memory(M0) / Cozo(M1) 등 어댑터가 구현한다.
pub trait KnowledgeStore: Send + Sync {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    fn get_entity(&self, id: &str) -> Option<Entity>;
    /// entity.id 기준 upsert.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// from 또는 to 가 entity_id 인 관계들.
    fn relations_of(&self, entity_id: &str) -> Vec<Relation>;
    fn search(&self, query: &str, workspace: Option<&str>, limit: usize) -> Vec<SearchHit>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}
