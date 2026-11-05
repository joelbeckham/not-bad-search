use anyhow::Result;
use good_search::{open_db, open_reader, reconcile, search, Config};
use rusqlite::Connection;

#[test]
fn text_lifecycle_and_opt_in() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("files");
    std::fs::create_dir(&root)?;
    let note = root.join("notes.md");
    std::fs::write(&note, "# Planning\nThe marmalade telescope is ready.\n")?;
    std::fs::write(root.join("marmalade.txt"), "marmalade")?;
    let mut db = open_db(&dir.path().join("index.sqlite"))?;
    let mut config = Config {
        folders: vec![root.clone()],
        ..Default::default()
    };
    reconcile(&mut db, &config)?;
    assert!(search(&db, "telescope", 10)?.is_empty());
    config.content_folders.push(root.clone());
    assert!(reconcile(&mut db, &config)?.is_empty());
    let hits = search(&db, "marmalade", 10)?;
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].name, "marmalade.txt");
    assert!(hits[0].snippet.is_none());
    assert!(hits[1].snippet.as_ref().unwrap().contains("[marmalade]"));
    let chunk: i64 = db.query_row("SELECT min(id) FROM chunks", [], |r| r.get(0))?;
    reconcile(&mut db, &config)?;
    assert_eq!(
        chunk,
        db.query_row("SELECT min(id) FROM chunks", [], |r| r.get::<_, i64>(0))?
    );
    std::fs::write(&note, "# Planning\nThe blueberry telescope is ready.\n")?;
    reconcile(&mut db, &config)?;
    assert_eq!(search(&db, "marmalade", 10)?.len(), 1);
    assert_eq!(search(&db, "blueberry", 10)?.len(), 1);
    std::fs::rename(&note, root.join("renamed.md"))?;
    reconcile(&mut db, &config)?;
    assert_eq!(search(&db, "blueberry", 10)?[0].name, "renamed.md");
    config.exclusions.push("renamed.md".into());
    reconcile(&mut db, &config)?;
    assert!(search(&db, "blueberry", 10)?.is_empty());
    config.content_folders.clear();
    reconcile(&mut db, &config)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?,
        0
    );
    assert_eq!(search(&db, "marmalade", 10)?.len(), 1);
    db.execute(
        "INSERT INTO chunk_search(chunk_search,rank) VALUES('integrity-check',1)",
        [],
    )?;
    Ok(())
}

#[test]
fn malformed_large_unicode_and_version_invalidation() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let config = Config {
        folders: vec![dir.path().into()],
        content_folders: vec![dir.path().into()],
        max_content_bytes: 20000,
        ..Default::default()
    };
    std::fs::write(dir.path().join("bad.txt"), [0xff, 0xfe])?;
    std::fs::write(dir.path().join("binary.txt"), b"needle\0payload")?;
    std::fs::write(dir.path().join("huge.md"), "needle".repeat(10000))?;
    std::fs::write(
        dir.path().join("valid.markdown"),
        format!("{}\nuniqueorchid café 東京", "é".repeat(3000)),
    )?;
    assert!(!reconcile(&mut db, &config)?.is_empty());
    assert!(search(&db, "needle", 10)?.is_empty());
    assert_eq!(search(&db, "uniqueorchid", 10)?.len(), 1);
    assert!(search(&db, "\" OR *", 10).is_ok());
    assert!(search(&db, "!!!", 10)?.is_empty());
    let chunk: i64 = db.query_row("SELECT min(id) FROM chunks", [], |r| r.get(0))?;
    db.execute("UPDATE documents SET chunker_version=0", [])?;
    reconcile(&mut db, &config)?;
    assert!(db.query_row("SELECT min(id) FROM chunks", [], |r| r.get::<_, i64>(0))? > chunk);
    std::fs::write(dir.path().join("valid.markdown"), [0xff])?;
    reconcile(&mut db, &config)?;
    assert!(search(&db, "uniqueorchid", 10)?.is_empty());
    Ok(())
}

#[test]
fn migrate_legacy_and_read_during_write() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("index.sqlite");
    let legacy = Connection::open(&path)?;
    legacy.execute_batch(
        include_str!("../src/schema.sql")
            .split("CREATE TABLE documents")
            .next()
            .unwrap(),
    )?;
    legacy.execute(
        "INSERT INTO files VALUES(42,'root','C:\\legacy.md','legacy.md','legacy.md')",
        [],
    )?;
    drop(legacy);
    let mut db = open_db(&path)?;
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        3
    );
    assert_eq!(search(&db, "legacy", 10)?.len(), 1);
    let reader = open_reader(&path)?;
    let tx = db.transaction()?;
    tx.execute("DELETE FROM files", [])?;
    assert_eq!(search(&reader, "legacy", 10)?.len(), 1);
    assert!(reader.execute("DELETE FROM files", []).is_err());
    tx.commit()?;
    assert!(search(&reader, "legacy", 10)?.is_empty());
    drop(db);
    let db = open_db(&path)?;
    db.pragma_update(None, "user_version", 99)?;
    assert!(open_db(&path).is_err());
    Ok(())
}

#[test]
fn terms_phrases_and_literal_operators() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (name, body) in [
        ("alpha.md", "budget annual project approval"),
        ("beta.md", "project budget approval next year"),
        ("gamma.md", "project-budget next year"),
        ("project annual budget.txt", "nothing"),
        ("project budget.txt", "nothing"),
        ("literal.md", "project OR budget"),
    ] {
        std::fs::write(dir.path().join(name), body)?;
    }
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let config = Config {
        folders: vec![dir.path().into()],
        content_folders: vec![dir.path().into()],
        ..Default::default()
    };
    reconcile(&mut db, &config)?;
    let names = |q| -> Result<Vec<String>> {
        Ok(search(&db, q, 100)?.into_iter().map(|h| h.name).collect())
    };
    assert_eq!(names("project budget")?.len(), 6);
    assert_eq!(names("budget project")?.len(), 6);
    assert_eq!(names("\"project budget\"")?.len(), 3);
    assert_eq!(names("\"project budget")?, names("\"project budget\"")?);
    assert_eq!(names("\"project budget\" approval")?, vec!["beta.md"]);
    assert_eq!(names("\"project budget\" \"next year\"")?.len(), 2);
    assert_eq!(names("project OR budget")?, vec!["literal.md"]);
    assert!(names("\"\"")?.is_empty());
    assert!(names("budget absent")?.is_empty());
    assert!(names("budget %")?.is_empty());
    Ok(())
}
