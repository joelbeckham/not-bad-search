use anyhow::Result;
use good_search::{
    gmail::{self, Provider, Settings},
    jira::Outcome,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
struct Fake(VecDeque<(&'static str, Value)>);
impl Provider for Fake {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value> {
        if tool == "search_threads" {
            assert!(args["query"].as_str().unwrap().contains("after:"));
        }
        let (e, v) = self.0.pop_front().expect("unexpected call");
        assert_eq!(e, tool);
        if v.is_null() {
            anyhow::bail!("retry")
        }
        Ok(v)
    }
}
fn thread(body: &str) -> Value {
    json!({"id":"abc","messageCount":2,"viewUrl":"https://mail.google.com/mail/u/0/#all/abc","messages":[{"subject":"Release planning","sender":"alice@example.com","toRecipients":["bob@example.com"],"date":"2026-10-30T10:00:00Z","plaintextBody":"First message"},{"subject":"Re: Release planning","sender":"bob@example.com","date":"2026-11-07T12:00:00Z","plaintextBody":body}]})
}
#[test]
fn resume_atomic_pages_and_update_conversations() -> Result<()> {
    let mut db = good_search::open_db(std::path::Path::new(":memory:"))?;
    let settings = Settings::default();
    let mut fake = Fake(VecDeque::from([
        (
            "search_threads",
            json!({"threads":[{"id":"abc"}],"nextPageToken":"next"}),
        ),
        ("get_thread", thread("oldneedle")),
        ("search_threads", Value::Null),
    ]));
    assert!(gmail::sync(&mut db, "mail", &settings, &mut fake, || false).is_err());
    assert_eq!(
        db.query_row("SELECT next_token FROM remote_sync", [], |r| r
            .get::<_, String>(0))?,
        "next"
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    fake.0.push_back(("search_threads", json!({"threads":[]})));
    assert_eq!(
        gmail::sync(&mut db, "mail", &settings, &mut fake, || false)?,
        Outcome::Complete
    );
    fake.0.extend([
        ("search_threads", json!({"threads":[{"id":"abc"}]})),
        ("get_thread", thread("newneedle")),
    ]);
    gmail::sync(&mut db, "mail", &settings, &mut fake, || false)?;
    let text: String =
        db.query_row("SELECT searchable_text FROM remote_items", [], |r| r.get(0))?;
    assert!(
        text.contains("First message") && text.contains("newneedle") && !text.contains("oldneedle")
    );
    let hits = good_search::search(&db, "Release", 10)?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].open_label(), "Open in Gmail");
    assert!(hits[0].jira.is_none());
    assert_eq!(
        hits[0].timestamp,
        Some(chrono::DateTime::parse_from_rfc3339("2026-11-07T12:00:00Z")?.timestamp())
    );
    assert_eq!(
        good_search::jira::open_target(&db, &hits[0].path)?,
        hits[0].location()
    );
    Ok(())
}
#[test]
fn incomplete_conversation_does_not_advance_or_publish() -> Result<()> {
    let mut db = good_search::open_db(std::path::Path::new(":memory:"))?;
    let mut incomplete = thread("x");
    incomplete["messageCount"] = json!(3);
    let mut fake = Fake(VecDeque::from([
        ("search_threads", json!({"threads":[{"id":"abc"}]})),
        ("get_thread", incomplete),
    ]));
    assert!(gmail::sync(&mut db, "mail", &Settings::default(), &mut fake, || false).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        db.query_row("SELECT pages FROM remote_sync", [], |r| r.get::<_, i64>(0))?,
        0
    );
    Ok(())
}
#[test]
fn new_source_is_paused_and_config_roundtrips() -> Result<()> {
    let source = good_search::sources::Source::gmail_source();
    assert!(source.paused);
    assert!(!source.images);
    assert!(source.root().starts_with("gmail:"));
    source.gmail.as_ref().unwrap().validate()?;
    let encoded = toml::to_string(&source)?;
    let restored: good_search::sources::Source = toml::from_str(&encoded)?;
    assert_eq!(source.root(), restored.root());
    Ok(())
}
