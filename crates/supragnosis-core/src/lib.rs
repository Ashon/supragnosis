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
    /// 신뢰도 0.0~1.0. **무표기(None)는 보존한다** (원칙 2 4차 개정): 기본값으로
    /// 치환하면 "주장하지 않음"과 "만점으로 주장함"의 구별이 소실된다(캡처 손실).
    /// 무표기의 해석(기본 가중)은 해소 정책의 몫이다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
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

/// 필드를 length-prefix 로 해시에 넣는다. 구분자(`\0`) 연접은 경계가 모호해 content 에
/// 구분자를 심으면 다른 필드 조합과 같은 바이트열을 구성할 수 있었다(id 선점, 원칙 18).
/// 길이 접두는 각 필드의 범위를 스트림 자체가 확정하므로 경계 조작이 불가능하다.
fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// Option 필드는 presence 바이트를 앞세운다 - None 과 Some("") 이 구별된다.
fn hash_opt_field(hasher: &mut blake3::Hasher, v: Option<&str>) {
    match v {
        Some(s) => {
            hasher.update(&[1]);
            hash_field(hasher, s.as_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

/// Option<u64> 필드도 presence 바이트 + 고정폭 LE 로 인코딩한다.
fn hash_opt_u64(hasher: &mut blake3::Hasher, v: Option<u64>) {
    match v {
        Some(x) => {
            hasher.update(&[1]);
            hasher.update(&x.to_le_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
}

impl Assertions {
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.relations.is_empty()
    }

    /// 콘텐츠 주소 해시용 결정적 바이트 인코딩. serde 포맷에 결합하지 않는 수제
    /// 인코딩으로, 직렬화 라이브러리가 바뀌어도 id 가 흔들리지 않게 한다.
    /// 개수와 각 필드를 length-prefix 로 넣으므로 빈 주장(0,0)도 명시적으로 인코딩된다.
    ///
    /// 완전 구조분해(`..` 없음)로 쓴다 (원칙 14: 구조 진화의 기계적 강제): 주장 구조에
    /// 필드가 추가되면 여기가 컴파일 에러가 되어 "새 필드가 내용 정체성인가(해시 포함
    /// 여부)"의 명시적 결정을 강제한다 - 누락되면 그 필드만 다른 두 주장이 같은 콘텐츠
    /// 주소로 붕괴한다 ("같은 텍스트에 다른 주장이면 다른 관측" 불변식 파괴).
    fn hash_into(&self, hasher: &mut blake3::Hasher) {
        let Assertions {
            entities,
            relations,
        } = self;
        hasher.update(&(entities.len() as u64).to_le_bytes());
        for e in entities {
            let EntityAssertion { name, kind } = e;
            hash_field(hasher, name.as_bytes());
            hash_opt_field(hasher, kind.as_deref());
        }
        hasher.update(&(relations.len() as u64).to_le_bytes());
        for r in relations {
            let RelationAssertion {
                from,
                kind,
                to,
                valid_from,
                valid_to,
            } = r;
            hash_field(hasher, from.as_bytes());
            hash_field(hasher, kind.as_bytes());
            hash_field(hasher, to.as_bytes());
            hash_opt_u64(hasher, *valid_from);
            hash_opt_u64(hasher, *valid_to);
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
/// 유효구간(원칙 4)은 선택 - 소급 관측("지난달까지 참이었다")을 적재 시점에 캡처한다.
/// 표면이 받지 못하는 것은 로그에 실리지 않고, 로그에 없는 것은 재프로젝션으로도
/// 복원할 수 없다 - 시간여행 질의 로직은 미뤄도 캡처는 미룰 수 없는 이유.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationAssertion {
    pub from: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub to: String,
    /// 유효시간 시작(원칙 4). None = 관측 시점부터로 해석 (근사 기본값).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<Timestamp>,
    /// 유효시간 종료. None = 반증 전까지.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<Timestamp>,
}

/// 관측(Observation) - 진실의 원천. 불변이며 **콘텐츠 주소**로 식별된다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: String,
    pub content: String,
    /// 출처 attestation 목록 (원칙 2, 최소 1개). 같은 콘텐츠 주소로 재도착한 관측은
    /// 덮어쓰지 않고 이 목록의 **단조 합집합**으로 흡수된다(원칙 3) - [`Observation::absorb`].
    /// 콘텐츠 주소가 워크스페이스를 포함하므로 모든 attestation 은 같은 워크스페이스다.
    pub provenance: Vec<Provenance>,
    /// 동봉된 구조화 주장 (원칙 1: 엔티티/관계 그래프는 이 로그의 프로젝션이어야 한다).
    /// **콘텐츠 주소 id 계산에 포함한다** - 계보/임베딩과 달리 주장은 내용 정체성이다.
    /// 같은 텍스트라도 다른 주장을 동봉하면 다른 관측이다 (덮어쓰기 dedup 방지).
    #[serde(default, skip_serializing_if = "Assertions::is_empty")]
    pub assertions: Assertions,
    /// 이 관측이 파생된 원천 관측 id들 (원칙 18: 오염 소독의 리콜 계보).
    /// 비어 있으면 1차 관측. (id 계산에는 포함하지 않는다 - 계보는 내용 정체성이 아니다.)
    ///
    /// **원천 id 의 존재 검증을 하지 않는 것이 의도다** (원칙 16: 위상 독립 수렴):
    /// 원천 관측이 sync 로 나중에 도착할 수 있으므로 전방 참조는 허용되어야 한다 -
    /// 존재 검증의 추가는 도착 순서에 의미론을 결합시키는 회귀다.
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// (선택) 의미 검색용 임베딩 벡터 (원칙 19: 확률적 경계).
    /// **콘텐츠 주소 id 계산에 포함하지 않는다** - 임베딩은 회상을 넓히는 로컬 보조일 뿐
    /// 내용 정체성이 아니며, 노드마다 다른 모델을 써도 정체성/수렴이 흔들리지 않는다.
    ///
    /// serde 에서 완전히 제외한다(원칙 21, Entity.embedding 과 대칭): 관측이 MCP
    /// 리소스(observation/{id})로 나갈 때 수백 개 float 가 LLM 컨텍스트를 오염시키면
    /// 안 된다. 영속은 스토어 어댑터가 수제 인코딩으로 담당한다.
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

impl Observation {
    /// 콘텐츠 주소 ID = blake3(workspace + content). 어떤 경로(서버/피어)로 들어와도 동일 id -> dedup.
    pub fn new(content: String, provenance: Provenance) -> Self {
        Self::with_assertions(content, provenance, Assertions::default())
    }

    /// 구조화 주장을 동봉한 관측. 주장이 비어 있으면 id 는 `new` 와 동일하고,
    /// 주장이 있으면 id 계산에 포함된다. 모든 필드는 length-prefix 로 인코딩되어
    /// content 에 구분자를 심는 경계 조작으로는 다른 관측과 충돌시킬 수 없다.
    pub fn with_assertions(content: String, provenance: Provenance, assertions: Assertions) -> Self {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, provenance.workspace.as_bytes());
        hash_field(&mut hasher, content.as_bytes());
        assertions.hash_into(&mut hasher);
        let id = hasher.finalize().to_hex().to_string();
        Self {
            id,
            content,
            provenance: vec![provenance],
            assertions,
            derived_from: Vec::new(),
            embedding: None,
        }
    }

    /// 이 관측의 워크스페이스. 콘텐츠 주소가 워크스페이스를 포함하므로 모든
    /// attestation 이 같은 워크스페이스를 지닌다 - 첫 항목이 대표다.
    pub fn workspace(&self) -> &str {
        self.provenance.first().map(|p| p.workspace.as_str()).unwrap_or("")
    }

    /// 같은 콘텐츠 주소의 재도착을 **단조 병합**한다 (원칙 3: 덮어쓰기 금지).
    /// 비정체성 필드(provenance attestation, derived_from 계보)를 합집합으로 누적한다 -
    /// 합집합은 교환/결합/멱등이라 도착 순서와 무관하게 같은 결과로 수렴한다(원칙 16).
    /// 릴레이 중복(완전 동일 attestation)은 자연 dedup 되고, 독립 재관측(어느 필드든
    /// 다른 attestation)은 누적된다. 정체성 필드(content/assertions)는 id 가 같으면
    /// 동일하므로 건드리지 않는다. 임베딩은 회상 보조일 뿐이라(원칙 19) 기존 값을
    /// 유지하고 없을 때만 받는다.
    pub fn absorb(&mut self, other: Observation) {
        debug_assert_eq!(self.id, other.id, "absorb 는 같은 콘텐츠 주소끼리만");
        // 완전 구조분해(`..` 없음, 원칙 14: 구조 진화의 기계적 강제): Observation 에
        // 필드가 추가되면(M4: origin/hlc/signature 예정) 여기가 컴파일 에러가 되어
        // "재도착 병합에서 그 필드를 어떻게 합치는가"의 명시적 결정을 강제한다 -
        // 침묵 누락은 병합 시 그 필드의 소실이다. 정체성 필드(id/content/assertions)는
        // 콘텐츠 주소가 동일성을 보증하므로 버린다(`_` 바인딩도 결정의 표기다).
        let Observation {
            id: _,
            content: _,
            provenance,
            assertions: _,
            derived_from,
            embedding,
        } = other;
        self.provenance.extend(provenance);
        self.provenance.sort_by(provenance_order);
        self.provenance
            .dedup_by(|a, b| provenance_order(a, b) == std::cmp::Ordering::Equal);
        self.derived_from.extend(derived_from);
        self.derived_from.sort();
        self.derived_from.dedup();
        if self.embedding.is_none() {
            self.embedding = embedding;
        }
    }
}

/// attestation 의 결정적 전순서 - 합집합의 정렬/중복 제거에 쓴다. 필드 전체를
/// 비교하므로 "같음"은 완전 동일 attestation(릴레이 중복)뿐이고, 어떤 필드든 다르면
/// (독립 재관측) 별개로 남는다. confidence 는 to_bits 로 전순서화한다 - 무표기(None)는
/// Option 의 Ord 로 어떤 표기 값보다 앞이며, 표기 1.0 과 구별되는 별개 attestation 이다.
///
/// 완전 구조분해(`..` 없음)로 쓴다 (원칙 14: 구조 진화의 기계적 강제): Provenance 에
/// 필드가 추가되면 여기가 컴파일 에러가 되어 "새 필드가 attestation 구별 축인가"의
/// 명시적 결정을 강제한다 - 열거에서 침묵 누락되면 dedup 이 그 필드만 다른 별개
/// attestation 을 하나로 붕괴시킨다 (원칙 3 위반).
fn provenance_order(a: &Provenance, b: &Provenance) -> std::cmp::Ordering {
    fn key(
        p: &Provenance,
    ) -> (
        &str,
        Option<&str>,
        &str,
        Option<&str>,
        Timestamp,
        Option<u32>,
        TrustTier,
    ) {
        let Provenance {
            host,
            on_behalf_of,
            workspace,
            source_ref,
            observed_at,
            confidence,
            trust_tier,
        } = p;
        (
            host.as_str(),
            on_behalf_of.as_deref(),
            workspace.as_str(),
            source_ref.as_deref(),
            *observed_at,
            confidence.map(f32::to_bits),
            *trust_tier,
        )
    }
    key(a).cmp(&key(b))
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
    /// 결정적 엔티티 ID = blake3(workspace + normalized_name), length-prefix 인코딩.
    /// M0 해소 규칙: 정규명 완전일치(대소문자/공백 정규화). 임베딩 유사도 해소는 M3.
    pub fn make_id(workspace: &str, canonical_name: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, workspace.as_bytes());
        hash_field(&mut hasher, canonical_name.trim().to_lowercase().as_bytes());
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
    /// 결정적 관계 ID = blake3(from + normalized_kind + to), length-prefix 인코딩.
    /// kind 는 [`normalize_relation_kind`] 를 거치므로 표기 요동이 같은 엣지 id 로
    /// 수렴한다. (from/to 는 이미 해소된 정규 엔티티 id.)
    pub fn make_id(from: &str, kind: &str, to: &str) -> String {
        let mut hasher = blake3::Hasher::new();
        hash_field(&mut hasher, from.as_bytes());
        hash_field(&mut hasher, normalize_relation_kind(kind).as_bytes());
        hash_field(&mut hasher, to.as_bytes());
        hasher.finalize().to_hex().to_string()
    }
}

/// 하이퍼엣지(공동출현 이차 구조, 원칙 11 "유도의 기반")의 결정적 식별자 =
/// blake3(멤버 수 + 정렬된 멤버 id 들), length-prefix 인코딩. 하이퍼엣지는 저장
/// 원소가 아니라 프로젝션이므로 core 에는 id 유도(순수 함수)만 두고 뷰 타입은
/// engine 이 소유한다. **멤버 집합이 정체성**이라(원칙 14) 같은 집합은 어떤 관측에서
/// 유도돼도 같은 id 로 수렴한다 - 관측은 그 집합의 attestation 이다(원칙 3). 호출자가
/// 정렬/중복 제거한 id 슬라이스를 넘긴다. length-prefix 라 멤버 경계가 모호하지 않다
/// ([`Observation`] 의 해시 규율과 동일 - 원칙 18).
pub fn hyperedge_id(sorted_member_ids: &[String]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(sorted_member_ids.len() as u64).to_le_bytes());
    for id in sorted_member_ids {
        hash_field(&mut hasher, id.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// 검색 결과 한 건.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: SearchHitKind,
    pub id: String,
    pub snippet: String,
    /// 랭킹 점수 - **순위 비교 전용**. 스케일은 검색 표면마다 다르다(키워드 상수,
    /// 코사인 유사도, RRF 융합값). 절대값을 신뢰도/확신도로 해석하면 오독이다.
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
///
/// **읽기 계약** (모든 어댑터의 의무):
/// - **부재와 고장의 구별 (원칙 5)**: 백엔드 실패는 `Err` 로 전파한다. 실패를 빈
///   결과(`Ok(vec![])`/`Ok(None)`)로 삼키면 호출자가 "찾지 못함(미지)"과 "조회
///   불능(고장)"을 구별할 수 없다 - 부재/부정/실패 구별의 전제가 저장 계층에서
///   무너진다. 부분 실패도 부분 결과가 아니라 `Err` 다.
/// - **재현성 (원칙 16)**: 같은 상태에 같은 질의는 같은 응답이다. 정렬과 limit
///   절단은 안정 키(id)로 못박고, 내부 자료구조의 반복 순서(해시맵/행 순서)가
///   응답에 새면 계약 위반이다.
pub trait KnowledgeStore: Send + Sync {
    fn add_observation(&self, obs: Observation) -> Result<(), StoreError>;
    /// 관측 로그에서 id 로 관측을 복원한다 - 검색 히트/파생 계보의 역참조 경로이자
    /// (원칙 2/14: id 를 아는 자는 실체와 provenance 에 도달할 수 있다) 재도착 병합의
    /// 기준 읽기. 없는 id 는 `Ok(None)`, 백엔드 실패는 `Err`.
    fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError>;
    /// 없는 id 는 `Ok(None)`(부재, 원칙 5의 미지) - 백엔드 실패만 `Err`.
    fn get_entity(&self, id: &str) -> Result<Option<Entity>, StoreError>;
    /// entity.id 기준 upsert.
    fn put_entity(&self, entity: Entity) -> Result<(), StoreError>;
    fn add_relation(&self, rel: Relation) -> Result<(), StoreError>;
    /// from 또는 to 가 entity_id 인 관계들.
    fn relations_of(&self, entity_id: &str) -> Result<Vec<Relation>, StoreError>;
    fn search(
        &self,
        query: &str,
        workspace: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError>;
    /// start_id 에서 방향(from->to)을 따라 최대 `max_depth` 홉까지 도달하는 엔티티들.
    fn traverse(
        &self,
        start_id: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<Vec<TraverseHit>, StoreError>;
    /// 워크스페이스의 모든 엔티티를 열거한다(그래프 프로젝션의 읽기 경로). `None` 이면 전체.
    /// 온톨로지 시각화/관측가능성의 노드 집합 - search 처럼 질의어가 아니라 전수 열거다.
    fn all_entities(&self, workspace: Option<&str>) -> Result<Vec<Entity>, StoreError>;
    /// 워크스페이스의 모든 관계를 열거한다(그래프 프로젝션의 엣지 집합). `None` 이면 전체.
    /// 관계의 워크스페이스는 provenance.workspace 로 판단한다.
    fn all_relations(&self, workspace: Option<&str>) -> Result<Vec<Relation>, StoreError>;
    /// 워크스페이스의 모든 관측을 열거한다(로그 전수). `None` 이면 전체. search 처럼
    /// 질의어가 아니라 전수 열거다 - 부재는 빈 Vec. 개별 get 만으로는 로그 재생/공동출현
    /// 유도가 구조적으로 불가능하므로 열거를 포트에 둔다 - 하이퍼엣지(이차 구조, 원칙 11)와
    /// 재프로젝션(원칙 1)의 공통 읽기 경로다.
    ///
    /// **degrade 규약**: 개별 행의 복원 실패(일부 legacy/손상 row)는 전체 열거를 막지
    /// 않는다 - 그 행을 제외하고 나머지를 돌려주되 **침묵하지 않는다**(제외를 로그로 남긴다,
    /// 원칙 19 의 degrade 정신). 파생 오버레이(하이퍼엣지)를 한 행 때문에 통째로 못 쓰게
    /// 만들지 않기 위함이다. 반면 point-read([`KnowledgeStore::get_observation`])는 fail-fast 다
    /// (재도착 병합의 기준 읽기라 실패를 부재로 오인하면 attestation 이 파괴된다, 원칙 3).
    /// 질의/스키마 수준의 백엔드 고장(개별 행이 아닌)은 여전히 `Err`.
    /// 주의: 완전성이 필요한 재프로젝션(M3)은 제외 행을 드롭이 아니라 복원/복구로 다뤄야 한다.
    fn all_observations(&self, workspace: Option<&str>) -> Result<Vec<Observation>, StoreError>;
    /// 임베딩이 있는 관측을 질의 벡터와의 코사인 유사도로 검색한다 (원칙 19: 회상 확장).
    /// 임베딩이 없는 관측은 후보에서 제외된다. `score` 는 코사인 유사도(-1.0~1.0).
    /// 기본 구현은 빈 결과 - 벡터를 저장하지 않는 어댑터는 재정의할 필요가 없다.
    fn search_semantic(
        &self,
        _query_embedding: &[f32],
        _workspace: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
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
    ) -> Result<Vec<SearchHit>, StoreError> {
        Ok(Vec::new())
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
    /// 임베더의 안정 식별자 (모델명 + 차원, 예: "hashing-256", "bge-small-en-v1.5-384").
    /// 저장소가 벡터 인덱스와 함께 기록해, 다른 임베더로 재오픈하는 교체를 감지한다 -
    /// 모델이 다르면 벡터 공간이 달라 구/신 벡터를 한 인덱스에 섞으면 유사도가
    /// 무의미해지기 때문이다 (원칙 19: 어댑터 교체가 코어 정확성을 해치면 안 된다).
    fn id(&self) -> String;
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

/// UI 활동 이벤트 (관측가능성). MCP 도구 호출이 **의도 단위로** 발행하고, 뷰어가
/// 실시간 로그/노드 강조에 소비한다. core 는 데이터 타입 + 포트만 정의한다(원칙 20) -
/// 실제 전송(SSE/브로드캐스트)은 어댑터가 [`EventSink`] 로 구현한다. 노드 id 목록
/// (entities/reached/id)은 뷰어가 그래프 노드와 매칭해 펄스/포커스에 쓴다.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// 지식 적재: 관측 + 링크된 엔티티/관계.
    Observe {
        observation: String,
        entities: Vec<String>,
        relations: usize,
        workspace: String,
    },
    /// 검색: 질의 + 히트. `nodes` 는 히트 id(뷰어가 그래프 노드와 매칭해 강조 -
    /// 관측 id 는 노드가 아니라 매칭 안 되고 무시된다).
    Search {
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        hits: usize,
        nodes: Vec<String>,
        mode: String,
    },
    /// 엔티티 조회.
    GetEntity {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        found: bool,
    },
    /// 그래프 순회: 시작 노드 + 도달 노드들.
    Traverse {
        start: String,
        reached: Vec<String>,
    },
}

/// 이벤트 봉투: 이벤트 + **세션 id**(대화 발자국의 그룹 키). 같은 세션에서 어떤 지식이
/// 쓰였는지 묶어 보기 위한 것. `event` 는 flatten 되어 `{session, kind, ...}` 로
/// 직렬화된다 - 뷰어는 `ev.session` 과 `ev.kind` 를 평면으로 읽는다.
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    /// 세션 id (대화 ≈ MCP 서버 실행 단위, 또는 클라이언트가 주입). 발자국 그룹 키.
    pub session: String,
    #[serde(flatten)]
    pub event: Event,
}

/// UI 이벤트 싱크 포트 (원칙 20). 엔진이 보유하고 도구 호출이 발행한다. 없으면 no-op -
/// 관측가능성은 선택이며, 코어 정확성은 이 포트에 의존하지 않는다(원칙 19의 정신).
pub trait EventSink: Send + Sync {
    fn emit(&self, env: &EventEnvelope);
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
            confidence: Some(1.0),
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

    /// length-prefix 인코딩: content 에 구분자를 심어 주장 블록을 위조하는 경계 조작이
    /// 다른 관측과 같은 id 를 만들 수 없다 (구분자 연접 시절엔 충돌이 구성 가능했다).
    #[test]
    fn length_prefix_blocks_boundary_collision() {
        let crafted = Observation::new("x\0E\0rmcp\0Tool\0".into(), prov());
        let asserted = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                }],
                relations: vec![],
            },
        );
        assert_ne!(crafted.id, asserted.id, "경계 조작 충돌은 막혀야 한다");

        // Option presence 인코딩: 타입 미지정과 빈 문자열 타입은 다른 주장이다.
        let untyped = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion { name: "rmcp".into(), kind: None }],
                relations: vec![],
            },
        );
        let empty_typed = Observation::with_assertions(
            "x".into(),
            prov(),
            Assertions {
                entities: vec![EntityAssertion {
                    name: "rmcp".into(),
                    kind: Some(String::new()),
                }],
                relations: vec![],
            },
        );
        assert_ne!(untyped.id, empty_typed.id);
    }

    /// absorb 의 단조 합집합: 도착 순서와 무관하게 같은 결과(교환), 릴레이 중복은
    /// 자연 dedup(멱등), 독립 재관측은 누적된다 (원칙 3/16).
    #[test]
    fn absorb_union_is_order_independent_and_idempotent() {
        let prov_a = Provenance {
            host: "host-a".into(),
            confidence: Some(0.9),
            ..prov()
        };
        let prov_b = Provenance {
            host: "host-b".into(),
            confidence: Some(0.1),
            ..prov()
        };

        let make = |p: &Provenance, derived: &[&str]| {
            let mut o = Observation::new("same fact".into(), p.clone());
            o.derived_from = derived.iter().map(|s| s.to_string()).collect();
            o
        };

        // a 먼저 vs b 먼저 - 같은 attestation/계보 집합으로 수렴.
        let mut ab = make(&prov_a, &["o1"]);
        ab.absorb(make(&prov_b, &["o2"]));
        let mut ba = make(&prov_b, &["o2"]);
        ba.absorb(make(&prov_a, &["o1"]));
        assert_eq!(ab.provenance.len(), 2);
        let hosts = |o: &Observation| -> Vec<String> {
            o.provenance.iter().map(|p| p.host.clone()).collect()
        };
        assert_eq!(hosts(&ab), hosts(&ba), "합집합은 순서 무관");
        assert_eq!(ab.derived_from, ba.derived_from);
        assert_eq!(ab.derived_from, vec!["o1".to_string(), "o2".to_string()]);

        // 릴레이 중복(완전 동일 attestation)은 늘지 않는다 (멱등).
        ab.absorb(make(&prov_a, &["o1"]));
        assert_eq!(ab.provenance.len(), 2);
        assert_eq!(ab.derived_from.len(), 2);
    }

    /// 관측 로그 병합의 위상 독립 property (원칙 16): 같은 attestation 집합이 어떤
    /// 도착 순서로, 릴레이 중복을 얼마나 섞어 들어와도 같은 로그로 수렴한다.
    /// 무작위 순서는 seed 고정 LCG 로 생성해 재현 가능하다(벽시계/OS 난수 없음).
    /// 그래프 프로젝션 계층의 property test 는 M4 이연(architecture 14절) - 이 테스트는
    /// 이미 수렴 규범을 구현한 로그 계층을 상시 가드한다.
    #[test]
    fn absorb_converges_under_random_arrival_orders() {
        const N: usize = 8;
        // 서로 다른 attestation N개 - 무표기 confidence 포함(구별 보존 검증).
        let sources: Vec<(Provenance, Vec<String>)> = (0..N)
            .map(|i| {
                let p = Provenance {
                    host: format!("host-{i}"),
                    confidence: if i % 3 == 0 { None } else { Some(i as f32 / N as f32) },
                    observed_at: (100 - i) as u64,
                    ..prov()
                };
                (p, vec![format!("src-{}", i % 4)])
            })
            .collect();
        let make = |(p, d): &(Provenance, Vec<String>)| {
            let mut o = Observation::new("same fact".into(), p.clone());
            o.derived_from = d.clone();
            o
        };
        // 로그 상태의 비교 가능 표현 (Provenance 는 PartialEq 미구현 - serde 값으로).
        let state = |o: &Observation| {
            serde_json::to_value((&o.provenance, &o.derived_from)).unwrap()
        };
        let fold = |order: &[usize]| {
            let mut acc = make(&sources[order[0]]);
            for &i in &order[1..] {
                acc.absorb(make(&sources[i]));
            }
            acc
        };

        let baseline = state(&fold(&(0..N).collect::<Vec<_>>()));
        // seed 고정 LCG 로 Fisher-Yates 셔플 + 앞머리를 복제해 릴레이 중복 시뮬레이션.
        let mut lcg: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = |bound: usize| {
            lcg = lcg
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((lcg >> 33) as usize) % bound
        };
        for round in 0..32 {
            let mut order: Vec<usize> = (0..N).collect();
            for i in (1..N).rev() {
                order.swap(i, next(i + 1));
            }
            let dup = order[next(N)];
            order.push(dup); // 릴레이 중복
            assert_eq!(
                state(&fold(&order)),
                baseline,
                "round {round}: 도착 순서 {order:?} 가 다른 로그를 만들면 수렴 위반"
            );
        }
    }

    /// 원칙 2 (4차): confidence 무표기는 만점 주장과 다른 정보로 보존된다 - 같은
    /// 출처가 무표기로 한 번, 1.0 으로 한 번 주장하면 두 attestation 이 별개로 남는다.
    #[test]
    fn unstated_confidence_is_distinct_from_full_confidence() {
        let unstated = Provenance { confidence: None, ..prov() };
        let full = Provenance { confidence: Some(1.0), ..prov() };
        let mut o = Observation::new("fact".into(), unstated);
        o.absorb(Observation::new("fact".into(), full));
        assert_eq!(
            o.provenance.len(),
            2,
            "무표기와 만점 주장이 하나로 붕괴하면 캡처 손실: {:?}",
            o.provenance
        );
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
