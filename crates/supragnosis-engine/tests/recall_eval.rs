//! Recall regression eval.
//!
//! Principles Appendix B: "search/recall changes are verified with a memory-benchmark-style regression eval."
//! This test is that eval - it measures recall@k over labeled (query -> gold id) fixtures and sets a threshold
//! as a regression guard. Any change to the search path must pass these numbers.
//!
//! Deterministic/offline: it uses [`HashingEmbedder`](supragnosis_embed::HashingEmbedder) (lexical hashing),
//! so it stays resident in `cargo test` without a network/model. "Meaning" is approximated by lexical overlap,
//! an intended stand-in for determinism (Principle 16) and reproducibility (the real-model end-to-end is semantic_e2e).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use supragnosis_core::SearchHit;
use supragnosis_engine::{Engine, EntityInput, ObserveInput, RelationInput};
use supragnosis_embed::HashingEmbedder;
use supragnosis_store::InMemoryStore;

const WS: &str = "recall";

/// One corpus entry: an observation body + enclosed entities/relations. The observation creates and links the entities.
struct Doc {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)], // (name, type)
    relations: &'static [(&'static str, &'static str, &'static str)], // (from, kind, to)
}

/// One query: a natural-language query + the set of gold ids (specified as entity canonical names or observation bodies).
struct Query {
    name: &'static str,
    query: &'static str,
    /// If the gold is an entity, write its canonical name; if an observation, write its full body verbatim (resolved to an id).
    gold_entities: &'static [&'static str],
    gold_observations: &'static [&'static str],
}

/// The evaluation corpus. Mixes entity-gold queries (exposing Gap A) and observation-gold queries (the existing semantic path).
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
        // Noise: not the gold for any query.
        Doc {
            content: "a simple banana bread recipe with walnuts and cinnamon",
            entities: &[],
            relations: &[],
        },
    ]
}

fn queries() -> Vec<Query> {
    vec![
        // --- Entity gold (Gap A: unreachable without entity embeddings) ---
        // Tokens overlap the entity name, but the query is not a substring of the entity name/observation.
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
        // --- Observation gold (the existing semantic observation path) ---
        // Not a substring, but tokens overlap the observation body.
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

/// Loads the corpus and returns an (observation body -> actual observation id) mapping. Since the observation id
/// includes the enclosed assertions in its hash (core), we use the id observe returned as the gold, not one recomputed from the body.
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
                        valid_from: None,
                        valid_to: None,
                    })
                    .collect(),
            })
            .unwrap();
        obs_ids.insert(d.content, out.observation_id);
    }
    obs_ids
}

/// The set of gold ids for a query. Entities are deterministically resolved by canonical name; observations look up the actual id in the load mapping.
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

/// Returns per-query recall@k = |top-k intersect gold| / |gold| as (query, is-entity-gold, recall).
fn recall_per_query(
    engine: &Engine,
    obs_ids: &HashMap<&'static str, String>,
    k: usize,
) -> Vec<(&'static str, bool, f32)> {
    queries()
        .iter()
        .map(|q| {
            let gold = gold_ids(q, obs_ids);
            let hits: Vec<SearchHit> = engine.search(q.query, Some(WS), k).unwrap().hits;
            let got: HashSet<String> = hits.iter().take(k).map(|h| h.id.clone()).collect();
            let found = gold.iter().filter(|g| got.contains(*g)).count();
            let r = found as f32 / gold.len() as f32;
            eprintln!("[recall] {:<28} recall@{k} = {r:.2}", q.name);
            (q.name, !q.gold_entities.is_empty(), r)
        })
        .collect()
}

/// Graph enrichment: recalls a neighbor caught by neither lexical nor semantic means via a graph edge.
/// Written in degrade mode with no embedder, so keyword (substring) match cannot catch the neighbor either - the
/// neighbor is reached only through the 1-hop relation of a matched seed. Proves in isolation that the graph edge adds the recall.
#[test]
fn graph_enrichment_recalls_neighbor() {
    // No embedder (degrade): zero semantic path, keyword substring only.
    let engine = Engine::new(Arc::new(InMemoryStore::new()), "enrich-host", WS);

    engine
        .observe(ObserveInput {
            content: "alpha service overview".into(),
            workspace: Some(WS.into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "alpha service".into(), kind: Some("Component".into()) },
                EntityInput { name: "zeta backend".into(), kind: Some("Component".into()) },
            ],
            relations: vec![RelationInput {
                from: "alpha service".into(),
                kind: "depends_on".into(),
                to: "zeta backend".into(),
                valid_from: None,
                valid_to: None,
            }],
        })
        .unwrap();

    let alpha = supragnosis_core::Entity::make_id(WS, "alpha service");
    let zeta = supragnosis_core::Entity::make_id(WS, "zeta backend");

    // The query catches "alpha service" as the seed ("zeta backend" is not a substring match).
    let hits = engine.search("alpha service", Some(WS), 5).unwrap().hits;
    let ids: HashSet<String> = hits.iter().map(|h| h.id.clone()).collect();

    assert!(ids.contains(&alpha), "the seed (alpha service) should match: {ids:?}");
    // Key point: zeta backend is caught by no lexical/semantic path yet is recalled as a graph neighbor.
    assert!(
        ids.contains(&zeta),
        "the graph neighbor (zeta backend) should be recalled by enrichment: {ids:?}"
    );

    // A neighbor is a weaker signal than a seed: with a decayed score it ranks below the seed (order check).
    let alpha_rank = hits.iter().position(|h| h.id == alpha).unwrap();
    let zeta_rank = hits.iter().position(|h| h.id == zeta).unwrap();
    assert!(zeta_rank > alpha_rank, "the neighbor should be below the seed: {hits:?}");

    // With no relation, the neighbor is not recalled - isolating that the graph edge is what created the recall.
    let control = Engine::new(Arc::new(InMemoryStore::new()), "enrich-host", WS);
    control
        .observe(ObserveInput {
            content: "alpha service overview".into(),
            workspace: Some(WS.into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "alpha service".into(), kind: Some("Component".into()) },
                EntityInput { name: "zeta backend".into(), kind: Some("Component".into()) },
            ],
            relations: vec![], // no relation
        })
        .unwrap();
    let control_ids: HashSet<String> = control
        .search("alpha service", Some(WS), 5)
        .unwrap()
        .hits
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert!(
        !control_ids.contains(&zeta),
        "with no relation, zeta should not be recalled (the graph edge is the cause): {control_ids:?}"
    );
}

#[test]
fn recall_at_5_meets_baseline() {
    let engine = Engine::new(Arc::new(InMemoryStore::new()), "recall-host", WS)
        .with_embedder(Arc::new(HashingEmbedder::default()));
    let obs_ids = load(&engine);

    let per_query = recall_per_query(&engine, &obs_ids, 5);
    let mean = per_query.iter().map(|(_, _, r)| r).sum::<f32>() / per_query.len() as f32;
    eprintln!("[recall] mean recall@5 = {mean:.3}");

    // Overall regression guard: the baseline without embeddings was 0.400 (all entity-gold queries miss).
    // After introducing entity embeddings (Gap A), 1.000. Pinned to 0.9 to catch regressions.
    assert!(
        mean >= 0.9,
        "mean recall@5 = {mean:.3} - recall regression (below the 0.9 threshold)"
    );

    // Gap A-specific guard: entity-gold queries are recalled only by entity embeddings (unreachable via observation
    // lexicon). If this subset collapses it is an entity semantic-path regression - caught separately from the observation path.
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
        "entity-gold recall@5 = {entity_mean:.3} - entity semantic (Gap A) regression"
    );
}
