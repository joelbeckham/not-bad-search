use anyhow::Result;
use not_bad_search::{
    images::{self, Extraction, Extractor},
    open_db, reconcile, search, Config,
};
use std::{cell::Cell, path::Path};

struct Fake {
    calls: Cell<usize>,
    fail: bool,
}
impl Extractor for Fake {
    fn extract(&self, _: &str, _: &str, _: &[u8]) -> Result<Extraction> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            anyhow::bail!("Simulated outage");
        }
        Ok(Extraction {
            extracted_text: "invoice omega 2026".into(),
            description: "a purple mountain".into(),
        })
    }
}
fn png(path: &Path, color: u8) -> Result<()> {
    image::RgbImage::from_pixel(8, 8, image::Rgb([color, 0, 0])).save(path)?;
    Ok(())
}

#[test]
fn extraction_cache_opt_out_and_changed_source() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("one.png");
    png(&path, 10)?;
    let mut db = open_db(Path::new(":memory:"))?;
    let mut config = Config {
        folders: vec![dir.path().into()],
        image_folders: vec![dir.path().into()],
        ..Default::default()
    };
    let fake = Fake {
        calls: Cell::new(0),
        fail: false,
    };
    reconcile(&mut db, &config)?;
    assert!(images::process_one(&mut db, &fake, || config.clone())?);
    assert_eq!(search(&db, "invoice", 10)?.len(), 1);
    assert!(search(&db, "invoice", 10)?[0]
        .snippet
        .as_ref()
        .unwrap()
        .starts_with("Image text"));
    assert!(search(&db, "purple", 10)?[0]
        .snippet
        .as_ref()
        .unwrap()
        .starts_with("Image description"));
    // Both source kinds coexist and survive ordinary text reconciliation.
    reconcile(&mut db, &config)?;
    assert!(!images::process_one(&mut db, &fake, || config.clone())?);
    assert_eq!(
        db.query_row("SELECT count(*) FROM documents", [], |r| r.get::<_, i64>(0))?,
        2
    );
    std::fs::copy(&path, dir.path().join("duplicate.png"))?;
    reconcile(&mut db, &config)?;
    assert!(images::process_one(&mut db, &fake, || config.clone())?);
    assert_eq!(fake.calls.get(), 1);
    png(&path, 20)?;
    reconcile(&mut db, &config)?;
    assert_eq!(search(&db, "invoice", 10)?.len(), 1);
    images::process_one(&mut db, &fake, || config.clone())?;
    assert_eq!(fake.calls.get(), 2);
    config.image_folders.clear();
    reconcile(&mut db, &config)?;
    assert!(search(&db, "invoice", 10)?.is_empty());
    assert_eq!(
        db.query_row("SELECT count(*) FROM image_cache", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn retry_pause_budget_and_interruption() -> Result<()> {
    let dir = tempfile::tempdir()?;
    png(&dir.path().join("one.png"), 10)?;
    let mut db = open_db(Path::new(":memory:"))?;
    let mut config = Config {
        folders: vec![dir.path().into()],
        image_folders: vec![dir.path().into()],
        image_daily_requests: 0,
        ..Default::default()
    };
    let fake = Fake {
        calls: Cell::new(0),
        fail: true,
    };
    reconcile(&mut db, &config)?;
    assert!(!images::process_one(&mut db, &fake, || config.clone())?);
    config.image_daily_requests = 10;
    config.images_paused = true;
    assert!(!images::process_one(&mut db, &fake, || config.clone())?);
    config.images_paused = false;
    assert!(images::process_one(&mut db, &fake, || config.clone())?);
    assert!(!images::process_one(&mut db, &fake, || config.clone())?);
    images::retry_failed(&db)?;
    assert!(images::process_one(&mut db, &fake, || config.clone())?);
    assert_eq!(fake.calls.get(), 2);
    db.execute("UPDATE image_jobs SET state='running'", [])?;
    images::recover(&db)?;
    assert!(!images::process_one(&mut db, &fake, || config.clone())?);
    assert!(images::summary(&db)?.contains("Interrupted request"));
    Ok(())
}

#[test]
fn migration_preserves_chunk_identity_and_cascades() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("index.sqlite");
    let old = rusqlite::Connection::open(&path)?;
    old.execute_batch(include_str!("../src/schema.sql"))?;
    old.execute_batch("INSERT INTO files VALUES(1,'root','file.md','file.md','file.md'); INSERT INTO documents VALUES(7,1,'hash',1,1,'ready',''); INSERT INTO chunks VALUES(9,7,0,'',1,'migrationneedle','hash'); PRAGMA user_version=1;")?;
    drop(old);
    let db = open_db(&path)?;
    assert_eq!(search(&db, "migrationneedle", 10)?.len(), 1);
    assert_eq!(
        db.query_row("SELECT id FROM chunks", [], |r| r.get::<_, i64>(0))?,
        9
    );
    db.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(1,'image_text','hash',1,1,'ready','')",[])?;
    db.execute("DELETE FROM files", [])?;
    assert!(search(&db, "migrationneedle", 10)?.is_empty());
    assert_eq!(
        db.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?,
        0
    );
    db.execute(
        "INSERT INTO chunk_search(chunk_search,rank) VALUES('integrity-check',1)",
        [],
    )?;
    Ok(())
}

#[test]
fn rejects_partial_refused_and_invalid_outputs() {
    use serde_json::json;
    assert!(images::parse_response(&json!({"status":"incomplete"})).is_err());
    assert!(images::parse_response(
        &json!({"status":"completed","output":[{"content":[{"type":"refusal"}]}]})
    )
    .is_err());
    assert!(images::parse_response(&json!({"status":"completed","output":[{"content":[{"type":"output_text","text":"{\"description\":\"missing transcription\"}"}]}]})).is_err());
}

#[test]
fn changed_during_request_is_discarded_and_pause_caches_paid_result() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("one.png");
    png(&path, 10)?;
    let mut db = open_db(Path::new(":memory:"))?;
    let config = Config {
        folders: vec![dir.path().into()],
        image_folders: vec![dir.path().into()],
        ..Default::default()
    };
    struct Mutating<'a>(&'a Path);
    impl Extractor for Mutating<'_> {
        fn extract(&self, _: &str, _: &str, _: &[u8]) -> Result<Extraction> {
            png(self.0, 99)?;
            Ok(Extraction {
                extracted_text: "stale".into(),
                description: "old image".into(),
            })
        }
    }
    reconcile(&mut db, &config)?;
    images::process_one(&mut db, &Mutating(&path), || config.clone())?;
    assert!(search(&db, "stale", 10)?.is_empty());
    reconcile(&mut db, &config)?;
    let fake = Fake {
        calls: Cell::new(0),
        fail: false,
    };
    let checks = Cell::new(0);
    images::process_one(&mut db, &fake, || {
        checks.set(checks.get() + 1);
        let mut c = config.clone();
        c.images_paused = checks.get() >= 3;
        c
    })?;
    assert!(search(&db, "invoice", 10)?.is_empty());
    images::process_one(&mut db, &fake, || config.clone())?;
    assert_eq!(fake.calls.get(), 1);
    assert_eq!(search(&db, "invoice", 10)?.len(), 1);
    Ok(())
}
