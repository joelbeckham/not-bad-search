//! Console diagnostics: exercises the same database/search and notify backend as the app.
use anyhow::{Context, Result};
use good_search::{open_db, reconcile, search, Config};
use notify::Watcher;
use std::{
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("bench") => {
            let path = Path::new(args.get(2).context("bench requires a new database path")?);
            if path.exists() {
                anyhow::bail!("Benchmark database must not already exist");
            }
            let mut db = open_db(path)?;
            let start = Instant::now();
            let tx = db.transaction()?;
            for i in 0..100_000 {
                let name = format!("project-{:03}-report-{i:06}.md", i % 1000);
                tx.execute(
                    "INSERT INTO files(root,path,name,folded) VALUES('synthetic',?1,?2,?2)",
                    rusqlite::params![format!("C:\\fixture\\{name}"), name],
                )?;
            }
            tx.commit()?;
            println!(
                "insert_100000_ms={:.2}",
                start.elapsed().as_secs_f64() * 1000.
            );
            for query in ["report", "project-042", "000123", "pr", "missing", "md"] {
                let mut timings = vec![];
                for _ in 0..100 {
                    let t = Instant::now();
                    let _ = search(&db, query, 100)?;
                    timings.push(t.elapsed().as_secs_f64() * 1000.);
                }
                timings.sort_by(f64::total_cmp);
                println!(
                    "query={query:?} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3}",
                    timings[50], timings[95], timings[99]
                );
            }
        }
        Some("watch") => {
            let root = Path::new(args.get(2).context("watch requires a folder")?);
            let (tx, rx) = mpsc::channel();
            let mut watcher = notify::recommended_watcher(move |event| {
                let _ = tx.send(event);
            })?;
            watcher.watch(root, notify::RecursiveMode::Recursive)?;
            println!("WATCH_READY {}", root.display());
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed() < Duration::from_secs(20) {
                if let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
                    println!("{}ms {event:?}", start.elapsed().as_millis());
                    count += 1;
                }
            }
            println!("events={count}");
        }
        Some("index") => {
            let mut db = open_db(Path::new(
                args.get(2).context("index requires database and folder")?,
            ))?;
            let config = Config {
                folders: vec![args.get(3).context("missing folder")?.into()],
                ..Default::default()
            };
            let t = Instant::now();
            let errors = reconcile(&mut db, &config)?;
            println!(
                "elapsed_ms={:.2} errors={errors:?}",
                t.elapsed().as_secs_f64() * 1000.
            );
            println!(
                "files={}",
                db.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))?
            );
        }
        _ => anyhow::bail!("Usage: diagnostics bench NEW_DB | watch FOLDER | index DB FOLDER"),
    }
    Ok(())
}
