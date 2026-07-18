//! 회상 회귀 평가셋 (recall regression eval).
//!
//! 헌법 부록 B: "검색/회상 변경은 메모리 벤치마크 스타일의 회귀 평가셋으로 검증한다."
//! 이 테스트가 그 평가셋이다 - 라벨링된 (질의 -> 정답 id) 픽스처 위에서 recall@k 를
//! 재고, 임계값을 회귀 가드로 건다. 검색 경로를 바꾸는 어떤 변경도 이 수치를 통과해야 한다.
//!
//! 결정적/오프라인: [`HashingEmbedder`](supragnosis_embed::HashingEmbedder)(어휘 해싱)를
//! 쓰므로 네트워크/모델 없이 `cargo test` 에 상주한다. "의미"는 어휘 중첩으로 근사되며,
//! 이는 결정성(원칙 16)과 재현성을 위한 의도된 스탠드인이다 (실모델 종단은 semantic_e2e).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use supragnosis_core::SearchHit;
use supragnosis_engine::{Engine, EntityInput, ObserveInput, RelationInput};
use supragnosis_embed::HashingEmbedder;
use supragnosis_store::InMemoryStore;

const WS: &str = "recall";

/// 코퍼스 한 건: 관측 본문 + 동봉 엔티티/관계. 엔티티는 관측이 만들고 링크한다.
struct Doc {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)], // (name, type)
    relations: &'static [(&'static str, &'static str, &'static str)], // (from, kind, to)
}

/// 질의 한 건: 자연어 질의 + 정답 id 집합(엔티티 정규명 또는 관측 본문으로 지정).
struct Query {
    name: &'static str,
    query: &'static str,
    /// 정답이 엔티티면 정규명, 관측이면 본문 전체를 그대로 적는다(id 로 해소한다).
    gold_entities: &'static [&'static str],
    gold_observations: &'static [&'static str],
}

/// 평가 코퍼스. 엔티티 정답 질의(Gap A 를 노출)와 관측 정답 질의(기존 시맨틱 경로)를 섞는다.
fn corpus() -> Vec<Doc> {
    vec![
        Doc {
            content: "we built a vector similarity index to speed up recall",
            entities: &[("vector similarity index", "Concept")],
            relations: &[],
        },
        Doc {
            content: "the observation log uses content addressed storage for deduplication",
            entities: &[("content addressed storage", "Concept")],
            relations: &[],
        },
        Doc {
            content: "provenance records a delegation chain identity for each writer",
            entities: &[("delegation chain identity", "Concept")],
            relations: &[],
        },
        Doc {
            content: "the tokio crate provides an asynchronous runtime for the rust language",
            entities: &[("tokio", "Tool"), ("rust", "Language")],
            relations: &[("tokio", "part_of", "rust")],
        },
        Doc {
            content: "reciprocal rank fusion merges ranked lists by their rank position",
            entities: &[("reciprocal rank fusion", "Concept")],
            relations: &[],
        },
        // 노이즈: 어떤 질의의 정답도 아니다.
        Doc {
            content: "a simple banana bread recipe with walnuts and cinnamon",
            entities: &[],
            relations: &[],
        },
    ]
}

fn queries() -> Vec<Query> {
    vec![
        // --- 엔티티 정답 (Gap A: 엔티티 임베딩 없이는 도달 불가) ---
        // 엔티티명과 토큰이 겹치지만, 질의가 엔티티명/관측의 부분문자열이 아니다.
        Query {
            name: "entity: vector index",
            query: "index for vector similarity lookups",
            gold_entities: &["vector similarity index"],
            gold_observations: &[],
        },
        Query {
            name: "entity: content storage",
            query: "addressed storage of content blocks",
            gold_entities: &["content addressed storage"],
            gold_observations: &[],
        },
        Query {
            name: "entity: delegation identity",
            query: "chain of delegation for identity",
            gold_entities: &["delegation chain identity"],
            gold_observations: &[],
        },
        // --- 관측 정답 (기존 시맨틱 관측 경로) ---
        // 부분문자열은 아니지만 관측 본문과 토큰이 겹친다.
        Query {
            name: "obs: async runtime",
            query: "asynchronous runtime for the rust language",
            gold_entities: &[],
            gold_observations: &[
                "the tokio crate provides an asynchronous runtime for the rust language",
            ],
        },
        Query {
            name: "obs: rank fusion",
            query: "merges ranked lists by rank position",
            gold_entities: &[],
            gold_observations: &["reciprocal rank fusion merges ranked lists by their rank position"],
        },
    ]
}

/// 코퍼스를 적재하고 (관측 본문 -> 실제 관측 id) 매핑을 돌려준다. 관측 id 는 동봉 주장까지
/// 해시에 포함하므로(core), 본문에서 재계산하지 않고 observe 가 돌려준 id 를 정답에 쓴다.
fn load(engine: &Engine) -> HashMap<&'static str, String> {
    let mut obs_ids = HashMap::new();
    for d in corpus() {
        let out = engine
            .observe(ObserveInput {
                content: d.content.into(),
                workspace: Some(WS.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: d
                    .entities
                    .iter()
                    .map(|(n, t)| EntityInput {
                        name: (*n).into(),
                        kind: Some((*t).into()),
                    })
                    .collect(),
                relations: d
                    .relations
                    .iter()
                    .map(|(f, k, t)| RelationInput {
                        from: (*f).into(),
                        kind: (*k).into(),
                        to: (*t).into(),
                    })
                    .collect(),
            })
            .unwrap();
        obs_ids.insert(d.content, out.observation_id);
    }
    obs_ids
}

/// 질의의 정답 id 집합. 엔티티는 정규명으로 결정적 해소, 관측은 적재 매핑에서 실제 id 를 찾는다.
fn gold_ids(q: &Query, obs_ids: &HashMap<&'static str, String>) -> HashSet<String> {
    use supragnosis_core::Entity;
    let mut set = HashSet::new();
    for name in q.gold_entities {
        set.insert(Entity::make_id(WS, name));
    }
    for content in q.gold_observations {
        set.insert(obs_ids[content].clone());
    }
    set
}

/// 질의별 recall@k = |top-k ∩ gold| / |gold| 를 (질의, 엔티티정답여부, recall) 로 돌려준다.
fn recall_per_query(
    engine: &Engine,
    obs_ids: &HashMap<&'static str, String>,
    k: usize,
) -> Vec<(&'static str, bool, f32)> {
    queries()
        .iter()
        .map(|q| {
            let gold = gold_ids(q, obs_ids);
            let hits: Vec<SearchHit> = engine.search(q.query, Some(WS), k);
            let got: HashSet<String> = hits.iter().take(k).map(|h| h.id.clone()).collect();
            let found = gold.iter().filter(|g| got.contains(*g)).count();
            let r = found as f32 / gold.len() as f32;
            eprintln!("[recall] {:<28} recall@{k} = {r:.2}", q.name);
            (q.name, !q.gold_entities.is_empty(), r)
        })
        .collect()
}

#[test]
fn recall_at_5_meets_baseline() {
    let engine = Engine::new(Arc::new(InMemoryStore::new()), "recall-host", WS)
        .with_embedder(Arc::new(HashingEmbedder::default()));
    let obs_ids = load(&engine);

    let per_query = recall_per_query(&engine, &obs_ids, 5);
    let mean = per_query.iter().map(|(_, _, r)| r).sum::<f32>() / per_query.len() as f32;
    eprintln!("[recall] mean recall@5 = {mean:.3}");

    // 전체 회귀 가드: 임베딩 없던 베이스라인은 0.400(엔티티 정답 질의가 전부 miss).
    // 엔티티 임베딩(Gap A) 도입 후 1.000. 회귀를 잡도록 0.9 로 못박는다.
    assert!(
        mean >= 0.9,
        "mean recall@5 = {mean:.3} - 회상 회귀(임계 0.9 하회)"
    );

    // Gap A 전용 가드: 엔티티 정답 질의는 엔티티 임베딩으로만 회상된다(관측 어휘로 도달
    // 불가). 이 부분집합이 무너지면 엔티티 시맨틱 경로 회귀다 - 관측 경로와 구별해 잡는다.
    let entity_mean = {
        let e: Vec<f32> = per_query
            .iter()
            .filter(|(_, is_entity, _)| *is_entity)
            .map(|(_, _, r)| *r)
            .collect();
        e.iter().sum::<f32>() / e.len() as f32
    };
    assert!(
        entity_mean >= 0.99,
        "엔티티 정답 recall@5 = {entity_mean:.3} - 엔티티 시맨틱(Gap A) 회귀"
    );
}
