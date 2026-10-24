//! Live recall eval (docs/recall.md Section 7): how well the real embedding model finds a person's
//! real records, for the queries agents write and the requests people make.
//!
//! Not a regression guard and not a fixture. The store and the queries are a person's knowledge,
//! so neither lives in this repository: the test reads the two paths it is given, and skips,
//! saying so, when they are not set.
//!
//! Run (on a copy of the store, since the daemon holds the original open):
//!   cp ~/.supragnosis/redb/knowledge.redb /tmp/eval-store.redb
//!   SUPRAGNOSIS_EVAL_STORE=/tmp/eval-store.redb SUPRAGNOSIS_EVAL_QUERIES=<queries.json> \
//!     cargo test -p supragnosis-e2e --features real-embed --test recall_live_eval \
//!     -- --ignored --nocapture
//!
//! The query file is a JSON array of `{"query", "workspace" (a name, or null for every
//! workspace), "form", "gold": [ids]}`. `form` says how the query is written and the summary is
//! grouped by it; `gold` holds the observation or entity ids that answer it. A query is found at
//! the rank of its first gold hit among the search's first 10. Optional: `SUPRAGNOSIS_EVAL_K`, the
//! cutoff for recall (default 5).

#![cfg(feature = "real-embed")]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use supragnosis_core::EmbeddingProvider;
use supragnosis_embed::{model_cache_dir, FastEmbedProvider, Model};
use supragnosis_engine::{Engine, SearchMode};
use supragnosis_store::RedbStore;

/// How many hits each query reads: the depth of the reciprocal rank.
const DEPTH: usize = 10;

#[test]
#[ignore = "reads a person's store and queries - see the module docs"]
fn recall_on_a_real_store() {
    let (Ok(store_path), Ok(queries_path)) = (
        std::env::var("SUPRAGNOSIS_EVAL_STORE"),
        std::env::var("SUPRAGNOSIS_EVAL_QUERIES"),
    ) else {
        eprintln!(
            "skipped: set SUPRAGNOSIS_EVAL_STORE (a copy of a knowledge.redb) and \
             SUPRAGNOSIS_EVAL_QUERIES (a query file) - docs/recall.md Section 7"
        );
        return;
    };
    let k: usize = std::env::var("SUPRAGNOSIS_EVAL_K")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let text = std::fs::read_to_string(&queries_path).expect("the query file reads");
    let queries: Vec<Value> = serde_json::from_str(&text).expect("the query file is a JSON array");

    let store = RedbStore::open(&store_path).expect("the store copy opens");
    let embedder = FastEmbedProvider::lazy(Model::DEFAULT, model_cache_dir());
    if let Some(recorded) = store.embedder().expect("the store says which model embedded it") {
        assert_eq!(recorded, embedder.id(), "the store was embedded by another model");
    }
    let engine = Engine::new(Arc::new(store), "eval", "default").with_embedder(Arc::new(embedder));

    // form -> (queries, found within k, sum of reciprocal ranks)
    let mut by_form: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    println!("rank  form      workspace      query");
    for q in &queries {
        let query = q["query"].as_str().expect("every entry has a query");
        let workspace = q["workspace"].as_str();
        let form = q["form"].as_str().unwrap_or("-").to_string();
        let gold: Vec<&str> = q["gold"]
            .as_array()
            .expect("every entry has gold ids")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        let out = engine.search(query, workspace, DEPTH).expect("the search answers");
        assert_eq!(out.mode, SearchMode::Hybrid, "the semantic surface did not answer {query:?}");
        let rank = out.hits.iter().position(|h| gold.contains(&h.id.as_str())).map(|i| i + 1);
        let row = by_form.entry(form.clone()).or_default();
        row.0 += 1;
        row.1 += usize::from(rank.is_some_and(|r| r <= k));
        row.2 += rank.map_or(0.0, |r| 1.0 / r as f64);
        let shown = rank.map_or_else(|| "-".to_string(), |r| r.to_string());
        println!("{shown:>4}  {form:<9} {:<14} {query}", workspace.unwrap_or("*"));
    }

    println!("\nform      queries  recall@{k}  MRR@{DEPTH}");
    let mut all = (0, 0, 0.0);
    for (form, (n, found, rr)) in &by_form {
        println!("{form:<9} {n:>7}  {:>8.2}  {:>6.2}", *found as f64 / *n as f64, rr / *n as f64);
        all = (all.0 + n, all.1 + found, all.2 + rr);
    }
    let n = all.0.max(1) as f64;
    println!("{:<9} {:>7}  {:>8.2}  {:>6.2}", "all", all.0, all.1 as f64 / n, all.2 / n);
}
