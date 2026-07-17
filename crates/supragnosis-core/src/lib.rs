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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// start_id 에서 방향(from->to)을 따라 최대 `max_depth` 홉까지 도달하는 엔티티들.
    fn traverse(&self, start_id: &str, max_depth: usize, limit: usize) -> Vec<TraverseHit>;
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store backend error: {0}")]
    Backend(String),
}
