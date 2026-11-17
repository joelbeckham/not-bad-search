use anyhow::Result;
use not_bad_search::{
    jira::Outcome,
    notion::{self, Provider, Settings},
};
use serde_json::{json, Value};
use std::collections::VecDeque;
const ROOT: &str = "11111111111111111111111111111111";
const CHILD: &str = "22222222222222222222222222222222";
fn page(id: &str, body: &str) -> Value {
    json!({"metadata":{"type":"page"},"title":"Notion fixture","url":format!("https://www.notion.so/{id}"),"page_last_edited_at":"2026-11-08T12:00:00Z","text":format!("<page><content>{body}</content></page>"),"references":{"notion-file-block://image":"https://example.com/image.png?temporary-secret"}})
}
struct Fake {
    calls: VecDeque<(&'static str, Value)>,
    bytes: Vec<u8>,
}
impl Provider for Fake {
    fn read(&mut self, tool: &str, _: Value) -> Result<Value> {
        let (t, v) = self.calls.pop_front().expect("unexpected read");
        assert_eq!(tool, t);
        if v.is_null() {
            anyhow::bail!("temporary failure")
        }
        Ok(v)
    }
    fn download(&mut self, url: &str) -> Result<Vec<u8>> {
        assert_eq!(url, "https://example.com/image.png?temporary-secret");
        Ok(self.bytes.clone())
    }
}
fn settings() -> Settings {
    Settings {
        pages: ROOT.into(),
        ..Default::default()
    }
}
#[test]
fn nested_pages_images_resume_and_open_parent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut db = not_bad_search::open_db(&tmp.path().join("index.sqlite"))?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(4, 4).write_to(&mut bytes, image::ImageFormat::Png)?;
    let root=page(ROOT,&format!("root text\n<page url=\"https://www.notion.so/{CHILD}\">Child</page>\n<image source=\"notion-file-block://image\" />"));
    let mut fake = Fake {
        calls: VecDeque::from([("fetch", root), ("fetch", Value::Null)]),
        bytes: bytes.into_inner(),
    };
    assert!(notion::sync(
        &mut db,
        "s",
        &settings(),
        &mut fake,
        tmp.path(),
        true,
        || false
    )
    .is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM attachments", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    let raw: String = db.query_row("SELECT raw_content FROM remote_items", [], |r| r.get(0))?;
    assert!(!raw.contains("temporary-secret"));
    fake.calls.push_back((
        "fetch",
        page(
            CHILD,
            &format!("replyneedle\n<page url=\"https://www.notion.so/{ROOT}\">Cycle</page>"),
        ),
    ));
    assert_eq!(
        notion::sync(
            &mut db,
            "s",
            &settings(),
            &mut fake,
            tmp.path(),
            true,
            || false
        )?,
        Outcome::Complete
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    let hits = not_bad_search::search(&db, "fixture", 10)?;
    assert!(hits
        .iter()
        .all(|h| h.notion && h.open_label() == "Open in Notion" && h.jira.is_none()));
    let attachment: String = db.query_row(
        "SELECT f.path FROM files f JOIN attachments a ON a.file_id=f.id",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(
        not_bad_search::jira::open_target(&db, &attachment)?,
        format!("https://www.notion.so/{ROOT}")
    );
    let mut source = not_bad_search::sources::Source::notion_source();
    source.id = "s".into();
    source.paused = false;
    source.notion = Some(settings());
    not_bad_search::images::discover(
        &mut db,
        &not_bad_search::Config {
            sources: vec![source],
            ..Default::default()
        },
    )?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM image_jobs", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}
#[test]
fn structure_ignores_links_and_code_and_keeps_database_children() -> Result<()> {
    let body=format!("```xml\n<page url=\"https://www.notion.so/{CHILD}\">Example</page>\n```\n[ordinary link](https://www.notion.so/{CHILD})\n<database url=\"{{{{https://www.notion.so/{ROOT}}}}}\">Database</database>\n![Screenshot](https://example.com/image.png)");
    let p = notion::parse_page(&page(ROOT, &body))?;
    assert_eq!(p.children, vec![format!("https://www.notion.so/{ROOT}")]);
    assert_eq!(p.images, vec!["https://example.com/image.png"]);
    assert!(notion::page_id("https://evil.example/11111111111111111111111111111111").is_err());
    Ok(())
}
#[test]
fn capped_teamspace_search_splits_and_does_not_claim_completion() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut db = not_bad_search::open_db(&tmp.path().join("index.sqlite"))?;
    let s = Settings {
        teamspaces: ROOT.into(),
        ..Default::default()
    };
    let results = vec![json!({"id":CHILD}); 50];
    let mut fake = Fake {
        calls: VecDeque::from([
            ("search", json!({"results":results})),
            ("search", Value::Null),
        ]),
        bytes: vec![],
    };
    assert!(notion::sync(&mut db, "s", &s, &mut fake, tmp.path(), false, || false).is_err());
    let state: String = db.query_row("SELECT next_token FROM remote_sync", [], |r| r.get(0))?;
    assert!(state.contains("Search"));
    let completed: Option<String> =
        db.query_row("SELECT last_success FROM remote_sync", [], |r| r.get(0))?;
    assert!(completed.is_none());
    Ok(())
}

#[test]
fn database_queries_resume_after_short_batches_without_search() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut db = not_bad_search::open_db(&tmp.path().join("index.sqlite"))?;
    let data = "collection://33333333-3333-3333-3333-333333333333";
    let root = json!({"metadata":{"type":"database"},"title":"Database","url":format!("https://www.notion.so/{ROOT}"),"text":format!("<database><data-source url=\"{data}\"></data-source></database>")});
    let child = format!("https://www.notion.so/{CHILD}");
    let mut fake = Fake {
        calls: VecDeque::from([
            ("fetch", root),
            (
                "query-data-sources",
                json!({"results":[{"url":child}],"has_more":false}),
            ),
            ("query-data-sources", Value::Null),
        ]),
        bytes: vec![],
    };
    assert!(notion::sync(
        &mut db,
        "s",
        &settings(),
        &mut fake,
        tmp.path(),
        false,
        || false
    )
    .is_err());
    let checkpoint: String =
        db.query_row("SELECT next_token FROM remote_sync", [], |r| r.get(0))?;
    assert!(checkpoint.contains(&child));
    assert!(checkpoint.contains("Database"));
    assert!(!checkpoint.contains("Search"));
    fake.calls.extend([
        ("query-data-sources", json!({"results":[],"has_more":false})),
        ("fetch", page(CHILD, "database row body")),
    ]);
    assert_eq!(
        notion::sync(
            &mut db,
            "s",
            &settings(),
            &mut fake,
            tmp.path(),
            false,
            || false
        )?,
        Outcome::Complete
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    Ok(())
}
#[test]
fn database_query_uses_bound_keyset_and_rejects_stalled_rows() -> Result<()> {
    struct Query;
    impl Provider for Query {
        fn read(&mut self, tool: &str, args: Value) -> Result<Value> {
            assert_eq!(tool, "query-data-sources");
            assert_eq!(args["data"]["mode"], "sql");
            assert!(args["data"]["query"]
                .as_str()
                .unwrap()
                .contains("WHERE url > ? ORDER BY url LIMIT 100"));
            Ok(json!({"results":[{"url":args["data"]["params"][0]}]}))
        }
        fn download(&mut self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }
    assert!(notion::database_page(
        &mut Query,
        "collection://33333333-3333-3333-3333-333333333333",
        &format!("https://www.notion.so/{ROOT}")
    )
    .is_err());
    assert!(notion::database_page(&mut Query, "collection://bad] DROP TABLE x", "").is_err());
    Ok(())
}

#[test]
fn blank_database_rows_index_properties_and_do_not_stop_traversal() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let mut db = not_bad_search::open_db(&tmp.path().join("index.sqlite"))?;
    let blank = json!({"metadata":{"type":"page"},"title":"Postgres health","url":format!("https://www.notion.so/{ROOT}"),"text":r#"<page><properties>{" Details":"Query tuning guide","Discipline":"SRE","Concern":"🟡"}</properties><blank-page>This page is blank and has no content.</blank-page></page>"#});
    let parsed = notion::parse_page(&blank)?;
    assert!(parsed.text.contains("Details: Query tuning guide"));
    assert!(parsed.text.contains("Discipline: SRE"));
    assert!(parsed.children.is_empty());
    let mut fake = Fake {
        calls: VecDeque::from([("fetch", blank)]),
        bytes: vec![],
    };
    assert_eq!(
        notion::sync(
            &mut db,
            "s",
            &settings(),
            &mut fake,
            tmp.path(),
            true,
            || false
        )?,
        Outcome::Complete
    );
    let text: String =
        db.query_row("SELECT searchable_text FROM remote_items", [], |r| r.get(0))?;
    assert!(text.contains("Query tuning guide"));
    let mut malformed = page(ROOT, "body");
    malformed["text"] = json!("<page><properties>{}</properties></page>");
    assert!(notion::parse_page(&malformed).is_err());
    malformed["text"] = json!("<page><content>incomplete");
    assert!(notion::parse_page(&malformed).is_err());
    Ok(())
}
#[test]
fn nonempty_pages_keep_properties_and_body() -> Result<()> {
    let mut value = page(ROOT, "Body needle");
    value["text"]=json!("<page><properties>{\" Details\":\"Property needle\"}</properties><content>Body needle</content></page>");
    let p = notion::parse_page(&value)?;
    assert!(p.text.contains("Property needle") && p.text.contains("Body needle"));
    Ok(())
}
