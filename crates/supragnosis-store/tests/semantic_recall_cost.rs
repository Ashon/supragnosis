//! Where the ANN index starts to pay: native HNSW against a brute-force cosine scan.
//!
//! This is the one capability the Cozo adapter has and the redb adapter does not. Everything else on
//! the port is implemented by both and checked by `port_conformance.rs`, so "is the older backend
//! still needed" reduces to this question and to nothing else - which makes it worth a number rather
//! than an opinion.
//!
//! It measures **semantic search end to end**, not the vector math alone. That is deliberate: a hit
//! carries a snippet, so the row has to be fetched whichever way the candidates were found, and at
//! small scales that fetch is most of the time. An isolated ANN microbenchmark would flatter the
//! index by measuring the part of the work the index actually skips.
//!
//! `#[ignore]`d and slow (the largest point builds two stores of 20k embedded rows, a few minutes in
//! total). It measures this machine, so it is a record rather than a gate - like
//! `read_path_cost::read_path_wall_clock`, the assertions that must hold everywhere live elsewhere.
//!
//! ```text
//! cargo test --release -p supragnosis-store --test semantic_recall_cost -- --ignored --nocapture
//! ```

use std::time::Instant;

use supragnosis_core::{KnowledgeStore, Observation, Provenance, TrustTier};

const DIM: usize = 384;
const WS: &str = "ws1";

fn prov() -> Provenance {
    Provenance {
        host: "host-a".into(),
        on_behalf_of: None,
        workspace: WS.into(),
        source_ref: None,
        observed_at: 1,
        confidence: None,
        trust_tier: TrustTier::default(),
        sync: None,
    }
}

/// A deterministic full-precision vector. Round numbers would understate every cost that touches the
/// stored bytes, which is most of what this file measures.
fn vector(seed: usize) -> Vec<f32> {
    let mut x = (seed as u32).wrapping_mul(2_654_435_761).wrapping_add(12_345);
    (0..DIM)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x as f32 / u32::MAX as f32).mul_add(2.0, -1.0)
        })
        .collect()
}

#[test]
#[ignore = "wall-clock measurement, not a guard - run manually with --ignored --nocapture"]
fn where_the_ann_index_starts_to_pay() {
    let base = std::env::temp_dir().join(format!("supragnosis-ann-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    println!(
        "\n{:<14} {:>7} {:>14} {:>12}",
        "store", "rows", "semantic p50", "ingest"
    );
    for n in [200usize, 1_000, 5_000, 20_000] {
        for backend in ["cozo(HNSW)", "redb(brute)"] {
            let dir = base.join(format!("{}-{n}", backend.replace(['(', ')'], "")));
            let build = Instant::now();
            // Cozo builds a native HNSW index only when opened with a vector dimension; redb has no
            // index and scans, which is a legal degrade on this port (Principle 19).
            let store: Box<dyn KnowledgeStore> = if backend.starts_with("cozo") {
                Box::new(
                    supragnosis_store::CozoStore::open_with_embedder(&dir, "fixed-384", DIM)
                        .expect("cozo open"),
                )
            } else {
                Box::new(
                    supragnosis_store::RedbStore::open(dir.join("knowledge.redb")).expect("redb open"),
                )
            };
            for i in 0..n {
                let mut o = Observation::new(format!("observation number {i}"), prov());
                o.embedding = Some(vector(i));
                store.add_observation(o).expect("observe");
            }
            let ingest = build.elapsed();

            let query = vector(7);
            let mut samples: Vec<u128> = Vec::with_capacity(20);
            for _ in 0..20 {
                let t = Instant::now();
                let hits = store.search_semantic(&query, Some(WS), 10).expect("semantic");
                samples.push(t.elapsed().as_micros());
                // A backend that answered nothing would post a very fast time for doing no work.
                assert!(!hits.is_empty(), "{backend} returned no hits at n={n}");
            }
            samples.sort_unstable();
            println!(
                "{backend:<14} {n:>7} {:>11}us {:>12.1?}",
                samples[samples.len() / 2],
                ingest
            );
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}
