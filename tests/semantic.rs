use anyhow::Result;
use good_search::{
    open_db, open_reader, open_writer,
    semantic::{self, Embedder, VectorIndex, DIMENSIONS},
    Hit,
};
use rusqlite::Connection;

struct Fake;

#[test]
fn mixed_sources_content_matching_and_empty_filters() -> Result<()> {
    use good_search::{jira, search_filtered, SearchFilters};
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    fixture(&db)?;
    db.execute_batch(
        "UPDATE files SET name='needle.md',folded='needle.md'; UPDATE chunks SET body='needle';
        INSERT INTO files VALUES(2,'pictures','C:/needle.png','needle.png','needle.png');
        INSERT INTO documents VALUES(2,2,'image_text','image',1,1,'ready','');
        INSERT INTO chunks VALUES(3,2,0,'',1,'needle','imagehash');",
    )?;
    struct Jira;
    impl jira::Provider for Jira {
        fn page(
            &mut self,
            _: &jira::Settings,
            _: &str,
            _: Option<&str>,
        ) -> Result<serde_json::Value> {
            Ok(
                serde_json::json!({"isLast":true,"issues":[{"id":"42","key":"CORE-42","fields":{"summary":"needle.png","updated":"2026-11-06T12:00:00Z","description":null}}]}),
            )
        }
        fn issue(&mut self, _: &jira::Settings, _: &str) -> Result<serde_json::Value> {
            unreachable!()
        }
    }
    jira::sync(
        &mut db,
        "jira-source",
        &Default::default(),
        &mut Jira,
        || false,
    )?;
    semantic::index_batch(&mut db, &Fake, 32)?;
    let mut index = VectorIndex::default();
    index.refresh(&db)?;
    for (roots, text, images, expected) in [
        (None, true, true, 3),
        (None, true, false, 2),
        (None, false, true, 1),
        (Some(vec!["jira:jira-source".into()]), true, true, 1),
        (Some(vec!["jira:jira-source".into()]), false, true, 0),
        (Some(vec!["root".into(), "pictures".into()]), true, true, 2),
        (Some(vec![]), true, true, 0),
        (None, false, false, 0),
    ] {
        let filters = SearchFilters {
            roots,
            text,
            images,
            ..Default::default()
        };
        assert_eq!(
            search_filtered(&db, "needle", 100, filters.clone())?.len(),
            expected
        );
        assert_eq!(
            index.search_filtered(&db, vector(), 100, filters)?.len(),
            expected
        );
    }
    assert!(search_filtered(
        &db,
        "needle",
        100,
        SearchFilters {
            text_matches: false,
            ..Default::default()
        }
    )?
    .is_empty());
    assert!(index
        .search_filtered(
            &db,
            vector(),
            100,
            SearchFilters {
                semantic: false,
                ..Default::default()
            }
        )?
        .is_empty());
    // Excluded sources must not consume the candidate limit, even for filename hits.
    for id in 100..1200 {
        db.execute(
            "INSERT INTO files VALUES(?1,'excluded',?2,?2,?2)",
            rusqlite::params![id, format!("needle-{id}")],
        )?;
    }
    let restricted = SearchFilters {
        roots: Some(vec!["jira:jira-source".into()]),
        ..Default::default()
    };
    let hits = search_filtered(&db, "needle", 1, restricted)?;
    assert_eq!(hits.len(), 1);
    assert!(hits[0].original_url.is_some());
    Ok(())
}

#[test]
fn source_groups_dynamic_all_and_reset() {
    use good_search::{filters::SourceSelection, sources::Source};
    let mut sources = vec![Source::folder("C:/notes".into()), Source::jira_source()];
    sources[0].paused = true;
    let mut selection = SourceSelection::default();
    assert_eq!(selection.roots(&sources), None);
    let folder = vec![sources[0].id.clone()];
    selection.set(&folder, false, &sources);
    assert_eq!(
        selection.roots(&sources).unwrap(),
        vec![format!("jira:{}", sources[1].id)]
    );
    selection.set(&folder, true, &sources);
    assert_eq!(selection.roots(&sources).unwrap().len(), 2); // Paused source remains searchable.
    sources.push(Source::folder("C:/new".into()));
    assert!(!selection.contains(&sources[2].id));
    selection = Default::default(); // Reset restores dynamic All, including future sources.
    assert!(selection.contains(&sources[2].id));
    assert_eq!(selection.roots(&sources), None);
    let ids = sources.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    selection.set(&ids, false, &sources);
    assert_eq!(selection.roots(&sources), Some(vec![]));
}
impl Embedder for Fake {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|_| {
                let mut v = vec![0.; DIMENSIONS];
                v[0] = 1.;
                v
            })
            .collect())
    }
}
fn fixture(db: &Connection) -> Result<()> {
    db.execute_batch(
        "INSERT INTO files VALUES(1,'root','C:/one.md','one.md','one.md');
    INSERT INTO documents VALUES(1,1,'text','source',1,1,'ready','');
    INSERT INTO chunks VALUES(1,1,0,'title',1,'a dog outside','hash');
    INSERT INTO chunks VALUES(2,1,1,'title',2,'a second chunk','hash2');",
    )?;
    Ok(())
}
fn vector() -> Vec<f32> {
    let mut v = vec![0.; DIMENSIONS];
    v[0] = 1.;
    v
}

#[test]
fn persistent_index_dedup_invalidation_and_deletion() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("index.sqlite");
    let mut db = open_db(&path)?;
    fixture(&db)?;
    let reader = open_reader(&path)?;
    assert_eq!(semantic::index_batch(&mut db, &Fake, 32)?, 2);
    assert_eq!(semantic::index_batch(&mut db, &Fake, 32)?, 0);
    let mut index = VectorIndex::default();
    index.refresh(&reader)?;
    assert_eq!(index.len(), 2);
    assert_eq!(index.search(&reader, vector(), 100)?.len(), 1);
    assert!(index.search(&reader, vec![0.; DIMENSIONS], 100)?.is_empty());
    db.execute(
        "UPDATE chunks SET body='changed',content_hash='new' WHERE id=1",
        [],
    )?;
    index.refresh(&reader)?;
    assert_eq!(index.len(), 1);
    assert_eq!(semantic::index_batch(&mut db, &Fake, 32)?, 1);
    db.execute("DELETE FROM files", [])?;
    // Even a stale in-memory vector cannot surface an opted-out or removed file.
    assert!(index.search(&reader, vector(), 100)?.is_empty());
    assert_eq!(
        db.query_row("SELECT count(*) FROM embeddings", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    index.refresh(&reader)?;
    assert!(index.is_empty());
    Ok(())
}

#[test]
fn publish_rejects_changed_chunks_and_invalid_vectors() -> Result<()> {
    struct Change(std::path::PathBuf);
    impl Embedder for Change {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            open_writer(&self.0)?
                .execute("UPDATE chunks SET body='changed',content_hash='new'", [])?;
            Fake.embed(texts)
        }
    }
    struct Bad;
    impl Embedder for Bad {
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![f32::NAN; DIMENSIONS]).collect())
        }
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("index.sqlite");
    let mut db = open_db(&path)?;
    fixture(&db)?;
    semantic::index_batch(&mut db, &Change(path), 32)?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM embeddings", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert!(semantic::index_batch(&mut db, &Bad, 32).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM embeddings", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn hybrid_ranking_protects_exact_names_quotes_and_deduplicates() {
    let hit = |name: &str| Hit {
        original_url: None,
        jira: None, timestamp: None, slack: false,
        path: format!("C:/{name}"),
        name: name.into(),
        snippet: None,
    };
    let merged = semantic::merge(
        "one.md",
        vec![hit("one.md"), hit("two.md")],
        vec![hit("two.md"), hit("three.md")],
        10,
    );
    assert_eq!(
        merged.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        ["one.md", "two.md", "three.md"]
    );
    let quoted = semantic::merge(
        "\"exact phrase\"",
        vec![hit("one.md")],
        vec![hit("two.md")],
        10,
    );
    assert_eq!(quoted.len(), 1);
    let ranked = semantic::merge(
        "concept",
        vec![hit("one.md"), hit("two.md")],
        vec![hit("two.md")],
        10,
    );
    assert_eq!(ranked[0].name, "two.md");
}

#[test]
fn source_and_match_filters_apply_before_ranking() -> Result<()> {
    use good_search::{search_filtered, SearchFilters};
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    fixture(&db)?;
    db.execute_batch(
        "INSERT INTO files VALUES(2,'root','C:/photo.png','photo.png','photo.png');
    INSERT INTO documents VALUES(2,2,'image_description','image',1,1,'ready','');
    INSERT INTO chunks VALUES(3,2,0,'',1,'a dog outside','hash3');
    INSERT INTO files VALUES(3,'root','C:/dog.bin','dog.bin','dog.bin');",
    )?;
    let all = SearchFilters::default();
    assert_eq!(search_filtered(&db, "dog", 100, all.clone())?.len(), 3);
    assert_eq!(
        search_filtered(
            &db,
            "dog",
            100,
            SearchFilters {
                images: false,
                ..all.clone()
            }
        )?
        .len(),
        2
    );
    assert_eq!(
        search_filtered(
            &db,
            "dog",
            100,
            SearchFilters {
                text: false,
                ..all.clone()
            }
        )?
        .len(),
        1
    );
    let names = search_filtered(
        &db,
        "dog",
        100,
        SearchFilters {
            text: false,
            images: false,
            ..all.clone()
        },
    )?;
    assert!(names.is_empty());

    assert!(search_filtered(
        &db,
        "dog",
        100,
        SearchFilters {
            text_matches: false,
            ..all.clone()
        }
    )?
    .is_empty());
    semantic::index_batch(&mut db, &Fake, 32)?;
    let mut index = VectorIndex::default();
    index.refresh(&db)?;
    assert_eq!(
        index
            .search_filtered(&db, vector(), 100, all.clone())?
            .len(),
        2
    );
    let text = index.search_filtered(
        &db,
        vector(),
        100,
        SearchFilters {
            images: false,
            ..all.clone()
        },
    )?;
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].name, "one.md");
    let images = index.search_filtered(
        &db,
        vector(),
        100,
        SearchFilters {
            text: false,
            ..all.clone()
        },
    )?;
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].name, "photo.png");
    assert!(index
        .search_filtered(
            &db,
            vector(),
            100,
            SearchFilters {
                semantic: false,
                ..all.clone()
            }
        )?
        .is_empty());
    assert!(index
        .search_filtered(
            &db,
            vector(),
            100,
            SearchFilters {
                text: false,
                images: false,
                ..all.clone()
            }
        )?
        .is_empty());
    assert_eq!(
        index
            .search_filtered(
                &db,
                vector(),
                100,
                SearchFilters {
                    text_matches: false,
                    ..all.clone()
                }
            )?
            .len(),
        2
    );
    Ok(())
}

#[test]
fn migration_v2_preserves_extracted_documents() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("index.sqlite");
    let db = Connection::open(&path)?;
    db.execute_batch(include_str!("../src/schema.sql"))?;
    db.execute_batch(include_str!("../src/schema-v2.sql"))?;
    db.pragma_update(None, "user_version", 2)?;
    fixture(&db)?;
    drop(db);
    let db = open_db(&path)?;
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        8
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))?,
        0
    );
    Ok(())
}
