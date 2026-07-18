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
/// 변형 선언 순서가 곧 신뢰 서열이라 derive Ord 가 "낮음 -> 높음" 을 그대로 준다
/// (해소 가중/그래프 대표 등급 계산에서 max 로 쓴다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
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

/// 관측에 동봉된 구조화 주장(후보 엔티티/관계) - architecture.md 2.3 의 `assertions`.
/// 원칙 1: 주장은 클라이언트가 말한 **원문 그대로** 로그에 남는다. 정규화/해소는
/// 프로젝션(해소 계층)의 일이며, 로그를 재생하면 언제든 다른 정책으로 재계산할 수 있다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<EntityAssertion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationAssertion>,
}

impl Assertions {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.relations.is_empty()
    }

    /// 콘텐츠 주소 해시용 결정적 바이트 인코딩. serde 포맷에 결합하지 않는 수제
    /// 인코딩으로, 직렬화 라이브러리가 바뀌어도 id 가 흔들리지 않게 한다.
    fn hash_into(&self, hasher: &mut blake3::Hasher) {
        for e in &self.entities {
            hasher.update(b"E\0");
            hasher.update(e.name.as_bytes());
            hasher.update(b"\0");
            hasher.update(e.kind.as_deref().unwrap_or("").as_bytes());
            hasher.update(b"\0");
        }
        for r in &self.relations {
            hasher.update(b"R\0");
            hasher.update(r.from.as_bytes());
            hasher.update(b"\0");
            hasher.update(r.kind.as_bytes());
            hasher.update(b"\0");
            hasher.update(r.to.as_bytes());
            hasher.update(b"\0");
        }
    }
}

/// 엔티티 주장: "이 이름의 것이 있고, (선택) 이 타입이다".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityAssertion {
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// 관계 주장: "from -kind-> to". from/to 는 이름(해소 전), kind 는 원문 표기.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationAssertion {
    pub from: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub to: String,
}

/// 관측(Observation) - 진실의 원천. 불변이며 **콘텐츠 주소**로 식별된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    pub provenance: Provenance,
    /// 동봉된 구조화 주장 (원칙 1: 엔티티/관계 그래프는 이 로그의 프로젝션이어야 한다).
    /// **콘텐츠 주소 id 계산에 포함한다** - 계보/임베딩과 달리 주장은 내용 정체성이다.
    /// 같은 텍스트라도 다른 주장을 동봉하면 다른 관측이다 (덮어쓰기 dedup 방지).
    #[serde(default, skip_serializing_if = "Assertions::is_empty")]
    pub assertions: Assertions,
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
        Self::with_assertions(content, provenance, Assertions::default())
    }

    /// 구조화 주장을 동봉한 관측. 주장이 비어 있으면 id 는 `new` 와 동일하고
    /// (텍스트 전용 관측의 id 호환), 주장이 있으면 id 계산에 포함된다.
    pub fn with_assertions(content: String, provenance: Provenance, assertions: Assertions) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(provenance.workspace.as_bytes());
        hasher.update(b"\0");
        hasher.update(content.as_bytes());
        if !assertions.is_empty() {
            hasher.update(b"\0");
            assertions.hash_into(&mut hasher);
        }
        let id = hasher.finalize().to_hex().to_string();
        Self {
            id,
            content,
            provenance,
            assertions,
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
    /// (선택) 의미 검색용 임베딩 벡터 (원칙 19: 확률적 경계). 이름/별칭의 의미로 노드에
    /// 도달하게 하는 회상 보조 - 관측과 마찬가지로 **id 계산에 포함하지 않는다**(정체성이
    /// 아니라 회상 확장이며, 노드마다 다른 모델을 써도 정체성/수렴이 흔들리지 않는다).
    ///
    /// serde 에서 완전히 제외한다(원칙 21): 이 벡터는 내부 회상 기계일 뿐이라 MCP 표면
    /// (get_entity)으로 새면 LLM 컨텍스트를 수백 개 float 로 오염시킨다. 영속은 스토어
    /// 어댑터가 수제 인코딩으로 담당하므로(Cozo data JSON), 도메인 직렬화 대상이 아니다.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
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

/// 관계 타입 표기의 결정적 정규화(정준형). LLM 추출기가 주 클라이언트인 시스템에서
/// 표기 요동(`depends_on`/`dependsOn`/`depends-on`/`Depends On`)은 상수이므로,
/// id 에 구워지기 전에 하나의 정준형(`depends_on`)으로 수렴시킨다.
///
/// 규칙: trim -> 구분자 연속(`-`, `_`, 공백)은 `_` 하나로 -> camelCase 경계
/// (소문자/숫자 뒤 대문자)에 `_` 삽입 -> 전부 lowercase.
/// 순수 함수 - 어떤 노드에서 어떤 순서로 프로젝션해도 같은 결과 (원칙 16).
pub fn normalize_relation_kind(kind: &str) -> String {
    let mut out = String::with_capacity(kind.len() + 4);
    let mut pending_sep = false;
    let mut prev: Option<char> = None;
    for ch in kind.trim().chars() {
        if ch == '-' || ch == '_' || ch.is_whitespace() {
            if !out.is_empty() {
                pending_sep = true;
            }
            continue;
        }
        if ch.is_uppercase() {
            if let Some(p) = prev {
                if p.is_lowercase() || p.is_numeric() {
                    pending_sep = true;
                }
            }
        }
        if pending_sep {
            out.push('_');
            pending_sep = false;
        }
        for lc in ch.to_lowercase() {
            out.push(lc);
        }
        prev = Some(ch);
    }
    out
}

impl Relation {
    /// 결정적 관계 ID = blake3(from + normalized_kind + to). kind 는
    /// [`normalize_relation_kind`] 를 거치므로 표기 요동이 같은 엣지 id 로 수렴한다.
    /// (from/to 는 이미 해소된 정규 엔티티 id.)
    pub fn make_id(from: &str, kind: &str, to: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(from.as_bytes());
        hasher.update(b"\0");
        hasher.update(normalize_relation_kind(kind).as_bytes());
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
    /// 워크스페이스의 모든 엔티티를 열거한다(그래프 프로젝션의 읽기 경로). `None` 이면 전체.
    /// 온톨로지 시각화/관측가능성의 노드 집합 - search 처럼 질의어가 아니라 전수 열거다.
    fn all_entities(&self, workspace: Option<&str>) -> Vec<Entity>;
    /// 워크스페이스의 모든 관계를 열거한다(그래프 프로젝션의 엣지 집합). `None` 이면 전체.
    /// 관계의 워크스페이스는 provenance.workspace 로 판단한다.
    fn all_relations(&self, workspace: Option<&str>) -> Vec<Relation>;
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

    /// 임베딩이 있는 엔티티를 질의 벡터와의 코사인 유사도로 검색한다 (원칙 19: 회상 확장).
    /// 엔티티 **이름/별칭의 의미**로 노드에 도달하게 한다 - 어떤 관측도 그 노드를 어휘로
    /// 언급하지 않아도 회상된다(관측 전용 시맨틱의 회상 공백을 메운다). `SearchHitKind::Entity`
    /// 히트를 돌려주며, `score` 는 코사인 유사도. 기본 구현은 빈 결과.
    fn search_semantic_entities(
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
    fn relation_kind_normalization_converges() {
        // 같은 의미의 표기 요동은 전부 하나의 정준형으로.
        for variant in [
            "depends_on",
            "dependsOn",
            "depends-on",
            "Depends On",
            " depends  on ",
            "DEPENDS_ON",
            "depends--on",
        ] {
            assert_eq!(
                normalize_relation_kind(variant),
                "depends_on",
                "variant {variant:?} should normalize to depends_on"
            );
        }
        // 이미 정준형이면 불변 (멱등).
        assert_eq!(
            normalize_relation_kind(&normalize_relation_kind("dependsOn")),
            "depends_on"
        );
        assert_eq!(normalize_relation_kind("relates_to"), "relates_to");
        // 숫자 뒤 대문자도 camelCase 경계.
        assert_eq!(normalize_relation_kind("layer2Uses"), "layer2_uses");
    }

    #[test]
    fn relation_id_is_notation_independent() {
        let (a, b) = ("id-a", "id-b");
        let canonical = Relation::make_id(a, "depends_on", b);
        assert_eq!(Relation::make_id(a, "dependsOn", b), canonical);
        assert_eq!(Relation::make_id(a, "depends-on", b), canonical);
        // 다른 의미의 kind 는 다른 id.
        assert_ne!(Relation::make_id(a, "part_of", b), canonical);
    }

    fn prov() -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: None,
            workspace: "ws".into(),
            source_ref: None,
            observed_at: 1,
            confidence: 1.0,
            trust_tier: TrustTier::default(),
        }
    }

    #[test]
    fn observation_id_includes_assertions() {
        let plain = Observation::new("supragnosis uses rmcp".into(), prov());
        // 빈 주장은 텍스트 전용 관측과 id 가 같다 (기존 id 체계 호환).
        let empty = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions::default(),
        );
        assert_eq!(plain.id, empty.id);

        // 주장이 붙으면 다른 관측이다 - 같은 텍스트라도 dedup 으로 주장이 소실되지 않는다.
        let asserted = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
            },
        );
        assert_ne!(plain.id, asserted.id);

        // 주장 내용이 다르면 id 도 다르다 (타입 배정도 내용 정체성).
        let retyped = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    name: "rmcp".into(),
                    kind: Some("Project".into()),
                }],
                relations: vec![],
            },
        );
        assert_ne!(asserted.id, retyped.id);

        // 같은 주장이면 어떤 경로로 와도 같은 id (결정성).
        let again = Observation::with_assertions(
            "supragnosis uses rmcp".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
            },
        );
        assert_eq!(asserted.id, again.id);
    }

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
