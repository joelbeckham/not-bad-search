use anyhow::Result;
use not_bad_search::{
    open_db, reconcile,
    sources::{self, Source},
    Config,
};

#[test]
fn image_queue_counts_are_disjoint_and_paused_sources_do_not_extract() -> Result<()> {
    let temp=tempfile::tempdir()?;
    let root=temp.path().join("images");std::fs::create_dir(&root)?;
    for name in ["pending","running","done","failed","skipped"] {std::fs::write(root.join(format!("{name}.png")),b"fixture")?;}
    let mut source=Source::folder(root);source.images=true;
    let mut config=Config {sources:vec![source],semantic_enabled:false,..Default::default()};config.sync_sources();
    let mut db=open_db(&temp.path().join("index.sqlite"))?;reconcile(&mut db,&config)?;
    for state in ["pending","running","done","failed","skipped"] {
        db.execute("UPDATE image_jobs SET state=?1 WHERE file_id IN (SELECT id FROM files WHERE name=?2)",rusqlite::params![state,format!("{state}.png")])?;
    }
    let stats=sources::snapshot(&db,&config)?;
    let s=&stats[0];
    assert_eq!((s.items,s.ready,s.queued,s.processing,s.attention,s.skipped,s.images_ready),(5,1,1,1,1,1,1));
    struct Never;
    impl not_bad_search::images::Extractor for Never {
        fn extract(&self,_:&str,_:&str,_:&[u8])->Result<not_bad_search::images::Extraction>{panic!("Paused source must not upload")}
    }
    config.sources[0].paused=true;
    assert!(!not_bad_search::images::process_one(&mut db,&Never,||config.clone())?);
    Ok(())
}

#[test]
fn legacy_settings_migrate_without_changing_scope_and_ids_persist() -> Result<()> {
    let root = std::env::temp_dir().join("notes");
    let mut config = Config {
        folders: vec![root.clone()],
        content_folders: vec![root.clone()],
        image_folders: vec![root.clone()],
        ..Default::default()
    };
    let exclusions = config.exclusions.clone();
    config.migrate_sources();
    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.sources[0].exclusions, exclusions);
    assert!(config.sources[0].text && config.sources[0].images);
    let id = config.sources[0].id.clone();
    config.sources[0].name = "Renamed source".into();
    config.sources[0].path = std::env::temp_dir().join("moved");
    config.sync_sources();
    let mut loaded: Config = toml::from_str(&toml::to_string(&config)?)?;
    loaded.migrate_sources();
    assert_eq!(loaded.sources[0].id, id);
    assert_eq!(loaded.folders, loaded.content_folders);
    assert_eq!(loaded.folders, loaded.image_folders);
    loaded.sources.clear();
    loaded.sync_sources();
    loaded.migrate_sources();
    assert!(loaded.sources.is_empty() && loaded.folders.is_empty());
    Ok(())
}

#[test]
fn source_exclusions_pause_resume_and_removal() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    std::fs::create_dir_all(&a)?;
    std::fs::create_dir_all(&b)?;
    for root in [&a, &b] {
        std::fs::write(root.join("keep.md"), "hello world")?;
        std::fs::write(root.join("skip.md"), "excluded only from a")?;
    }
    let mut first = Source::folder(a.clone());
    first.text = true;
    first.exclusions = vec!["skip.md".into()];
    let mut second = Source::folder(b.clone());
    second.text = true;
    let mut config = Config {
        sources: vec![first, second],
        semantic_enabled: false,
        ..Default::default()
    };
    config.sync_sources();
    let mut db = open_db(&temp.path().join("index.sqlite"))?;
    assert!(reconcile(&mut db, &config)?.is_empty());
    let stats = sources::snapshot(&db, &config)?;
    assert_eq!(
        (stats[0].items, stats[0].ready, stats[0].text_ready),
        (1, 1, 1)
    );
    assert_eq!((stats[1].items, stats[1].ready), (2, 2));
    config.sources[0].paused = true;
    std::fs::write(a.join("new.md"), "new")?;
    std::fs::write(a.join("keep.md"), "updated")?;
    reconcile(&mut db, &config)?;
    assert_eq!(sources::snapshot(&db, &config)?[0].items, 1);
    assert_eq!(not_bad_search::search(&db, "hello", 10)?.len(), 2);
    config.sources[0].paused = false;
    reconcile(&mut db, &config)?;
    assert_eq!(sources::snapshot(&db, &config)?[0].items, 2);
    config.sources.remove(0);
    config.sync_sources();
    reconcile(&mut db, &config)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))?,
        2
    );
    assert!(a.join("keep.md").exists());
    Ok(())
}

#[test]
fn status_counts_items_once_and_respects_embedding_completion() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("files");
    std::fs::create_dir(&root)?;
    for (name, body) in [("note.md", "# One\nhello world"), ("plain.bin", "binary")] {
        std::fs::write(root.join(name), body)?;
    }
    let mut source = Source::folder(root);
    source.text = true;
    let mut config = Config {
        sources: vec![source],
        ..Default::default()
    };
    config.sync_sources();
    let mut db = open_db(&temp.path().join("index.sqlite"))?;
    reconcile(&mut db, &config)?;
    let stats = sources::snapshot(&db, &config)?;
    assert_eq!(
        (
            stats[0].items,
            stats[0].ready,
            stats[0].queued,
            stats[0].embedding_queued
        ),
        (2, 1, 1, 1)
    );
    config.semantic_enabled = false;
    assert_eq!(sources::snapshot(&db, &config)?[0].ready, 2);
    struct Never;
    impl not_bad_search::semantic::Embedder for Never {
        fn embed(&self, _: &[String]) -> Result<Vec<Vec<f32>>> {
            panic!("Paused source must not generate embeddings")
        }
    }
    config.sources[0].paused = true;
    assert_eq!(not_bad_search::semantic::index_batch_excluding(&mut db, &Never, 32, &config.paused_roots_json())?, 0);
    Ok(())
}
