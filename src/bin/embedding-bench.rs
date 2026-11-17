use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use std::time::Instant;
fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("Pass the local model folder")?;
    let start = Instant::now();
    let model = StaticModel::from_pretrained(&path, None, Some(true), None)?;
    println!("load_ms={:.2}", start.elapsed().as_secs_f64() * 1000.);
    let docs = [
        "A photograph of a golden retriever playing in the park",
        "An invoice for a laptop purchase with a total amount due",
        "Meeting notes about database performance and query optimization",
        "A recipe for chocolate cake and vanilla frosting",
    ];
    let vectors = model.encode(&docs.map(str::to_owned));
    for (query, expected) in [
        ("dog outdoors", 0),
        ("computer receipt", 1),
        ("slow SQL", 2),
        ("baking dessert", 3),
    ] {
        let vector = model.encode_single(query);
        let scores: Vec<f32> = vectors
            .iter()
            .map(|v| v.iter().zip(&vector).map(|(a, b)| a * b).sum())
            .collect();
        let best = (0..scores.len())
            .max_by(|a, b| scores[*a].total_cmp(&scores[*b]))
            .unwrap();
        println!("query={query:?} best={best} scores={scores:?}");
        anyhow::ensure!(best == expected, "Semantic relevance fixture failed");
    }
    let mut times = Vec::new();
    for _ in 0..1000 {
        let start = Instant::now();
        std::hint::black_box(model.encode_single("dog playing outside in a park"));
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "query_embedding_ms p50={:.3} p95={:.3} p99={:.3}",
        times[500], times[950], times[990]
    );
    if let Some(database) = std::env::args().nth(2) {
        use not_bad_search::{
            open_db, open_reader,
            semantic::{self, LocalModel, VectorIndex},
        };
        let database = std::path::Path::new(&database);
        anyhow::ensure!(!database.exists(), "Use a new benchmark database");
        let mut db = open_db(database)?;
        let tx = db.transaction()?;
        for i in 1..=10_000_i64 {
            tx.execute(
                "INSERT INTO files VALUES(?1,'fixture',?2,?2,?2)",
                rusqlite::params![i, format!("document-{i}.md")],
            )?;
            tx.execute(
                "INSERT INTO documents VALUES(?1,?1,'text','hash',1,1,'ready','')",
                [i],
            )?;
            tx.execute(
                "INSERT INTO chunks VALUES(?1,?1,0,'',1,?2,?2)",
                rusqlite::params![i, docs[(i as usize) % docs.len()]],
            )?;
        }
        tx.commit()?;
        let local = LocalModel::load(std::path::Path::new(&path))?;
        let started = Instant::now();
        while semantic::index_batch(&mut db, &local, 64)? > 0 {}
        println!(
            "index_10000_chunks_ms={:.2}",
            started.elapsed().as_secs_f64() * 1000.
        );
        let reader = open_reader(database)?;
        let mut index = VectorIndex::default();
        let started = Instant::now();
        index.refresh(&reader)?;
        println!(
            "vector_load_ms={:.2}",
            started.elapsed().as_secs_f64() * 1000.
        );
        let mut timings = vec![];
        for _ in 0..100 {
            let started = Instant::now();
            let vector = semantic::embed_query(&local, "computer receipt")?;
            let hits = index.search(&reader, vector, 100)?;
            anyhow::ensure!(!hits.is_empty(), "No semantic matches");
            timings.push(started.elapsed().as_secs_f64() * 1000.);
        }
        timings.sort_by(f64::total_cmp);
        println!(
            "semantic_search_10000_ms p50={:.3} p95={:.3} p99={:.3}",
            timings[50], timings[95], timings[99]
        );
    }
    Ok(())
}
