use anyhow::{bail, Result};
use good_search::{
    jira::{self, Outcome, Provider, Settings},
    open_db,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
fn issue(id: &str, text: &str, updated: &str) -> Value {
    json!({"id":id,"key":format!("CORE-{id}"),"fields":{"summary":format!("Example {id}"),"updated":updated,"description":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}}})
}
fn page(issues: Vec<Value>, next: Option<&str>) -> Value {
    json!({"issues":issues,"isLast":next.is_none(),"nextPageToken":next})
}
struct Fake {
    pages: VecDeque<Option<Value>>,
    tokens: Vec<Option<String>>,
    detail: Option<Value>,
    detail_calls: usize,
}
impl Fake {
    fn new(pages: Vec<Option<Value>>) -> Self {
        Self {
            pages: pages.into(),
            tokens: vec![],
            detail: None,
            detail_calls: 0,
        }
    }
}
impl Provider for Fake {
    fn page(&mut self, _: &Settings, _: &str, token: Option<&str>) -> Result<Value> {
        self.tokens.push(token.map(str::to_owned));
        self.pages
            .pop_front()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("transient transport failure"))
    }
    fn issue(&mut self, _: &Settings, _: &str) -> Result<Value> {
        self.detail_calls += 1;
        self.detail
            .clone()
            .ok_or_else(|| anyhow::anyhow!("access denied to issue"))
    }
}
#[test]
fn page_failure_resume_checkpoint_and_duplicate_edit_invalidation() -> Result<()> {
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let settings = Settings::default();
    let a = issue("1", "marmalade telescope", "2026-10-01T12:00:00.000+0000");
    let b = issue("2", "other description", "2026-10-02T12:00:00.000+0000");
    let mut fake = Fake::new(vec![Some(page(vec![a.clone()], Some("second"))), None]);
    assert!(jira::sync(&mut db, "source", &settings, &mut fake, || false).is_err());
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    assert!(db
        .query_row("SELECT checkpoint FROM remote_sync", [], |r| r
            .get::<_, Option<String>>(0))?
        .is_none());
    fake.pages.push_back(Some(page(vec![b], None)));
    assert_eq!(
        jira::sync(&mut db, "source", &settings, &mut fake, || false)?,
        Outcome::Complete
    );
    assert_eq!(fake.tokens.last().unwrap().as_deref(), Some("second"));
    assert_eq!(
        db.query_row("SELECT checkpoint FROM remote_sync", [], |r| r
            .get::<_, String>(0))?,
        "2026-10-02T12:00:00.000Z"
    );
    let chunk: i64 = db.query_row("SELECT min(id) FROM chunks", [], |r| r.get(0))?;
    db.execute("INSERT INTO embeddings(chunk_id,model,content_hash,dimensions,vector) SELECT id,?1,content_hash,256,zeroblob(1024) FROM chunks",[good_search::semantic::MODEL])?;
    fake.pages.push_back(Some(page(vec![a], None)));
    jira::sync(&mut db, "source", &settings, &mut fake, || false)?;
    assert_eq!(
        chunk,
        db.query_row("SELECT min(id) FROM chunks", [], |r| r.get::<_, i64>(0))?
    );
    fake.pages.push_back(Some(page(
        vec![issue("1", "blueberry telescope", "2026-10-03T12:00:00Z")],
        None,
    )));
    jira::sync(&mut db, "source", &settings, &mut fake, || false)?;
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM embeddings WHERE chunk_id=?1",
            [chunk],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    assert!(good_search::search(&db, "marmalade", 10)?.is_empty());
    let hits = good_search::search(&db, "blueberry", 10)?;
    assert_eq!(hits.len(), 1);
    assert_eq!(
        jira::open_target(&db, &hits[0].path)?,
        "https://bombbomb.atlassian.net/browse/CORE-1"
    );
    let inspected = good_search::inspection::inspect(&mut db, &hits[0].path)?;
    assert!(inspected["remote_item"][0]["raw_adf"]
        .as_str()
        .unwrap()
        .contains("blueberry"));
    // An empty incremental response and local folder reconciliation never delete remote issues.
    fake.pages.push_back(Some(page(vec![], None)));
    jira::sync(&mut db, "source", &settings, &mut fake, || false)?;
    good_search::reconcile(&mut db, &Default::default())?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM remote_items", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    Ok(())
}
#[test]
fn fallback_adf_large_content_and_cancelled_page() -> Result<()> {
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let settings = Settings::default();
    let text = "Large linked description with searchable words. ".repeat(1600);
    let detail = issue("3", &text, "2026-10-03T14:00:00+0200");
    let mut search = detail.clone();
    search["fields"]["description"] = "plain response".into();
    let mut fake = Fake::new(vec![Some(page(vec![search.clone()], None))]);
    fake.detail = Some(detail);
    jira::sync(&mut db, "source", &settings, &mut fake, || false)?;
    assert_eq!(fake.detail_calls, 1);
    let stored: String =
        db.query_row("SELECT searchable_text FROM remote_items", [], |r| r.get(0))?;
    assert!(stored.contains(text.trim()));
    assert!(db.query_row("SELECT count(*) FROM chunks", [], |r| r.get::<_, i64>(0))? > 10);
    fake.pages.push_back(Some(page(vec![search], None)));
    jira::sync(&mut db, "source", &settings, &mut fake, || false)?;
    assert_eq!(fake.detail_calls, 1);
    assert_eq!(
        jira::sync(&mut db, "source", &settings, &mut fake, || true)?,
        Outcome::Cancelled
    );
    Ok(())
}
#[test]
fn malformed_pagination_and_detail_failures_do_not_commit_checkpoint() -> Result<()> {
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let mut fake = Fake::new(vec![Some(json!({"issues":[],"isLast":false}))]);
    assert!(jira::sync(&mut db, "s", &Settings::default(), &mut fake, || false).is_err());
    jira::record_error(&db, "s", "Bifrost HTTP 403: access denied")?;
    assert!(db
        .query_row("SELECT error FROM remote_sync", [], |r| r
            .get::<_, String>(0))?
        .contains("403"));
    assert!(db
        .query_row("SELECT checkpoint FROM remote_sync", [], |r| r
            .get::<_, Option<String>>(0))?
        .is_none());
    Ok(())
}
#[test]
fn adf_structure_links_unsupported_nodes_and_timezones() -> Result<()> {
    let raw = json!({"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Title"}]},{"type":"paragraph","content":[{"type":"text","text":"Link","marks":[{"type":"link","attrs":{"href":"https://example.com/a"}}]},{"type":"hardBreak"},{"type":"text","text":"Next line"}]},{"type":"table","content":[{"type":"tableRow","content":[{"type":"tableCell","content":[{"type":"text","text":"Cell"}]}]}]},{"type":"media","attrs":{"id":"123"}}]});
    let extracted = good_search::adf::extract(&raw)?;
    assert!(extracted.text.contains("## Title\n"));
    assert!(extracted
        .text
        .contains("Link (https://example.com/a)\nNext line"));
    assert!(extracted.text.contains(" | Cell"));
    assert_eq!(extracted.warnings, vec!["Unsupported ADF node: media"]);
    assert_eq!(
        jira::normalized_timestamp("2026-10-01T14:00:00.000+0200")?,
        "2026-10-01T12:00:00.000Z"
    );
    let q = jira::scope_jql(
        &Settings {
            jql: "summary ~ \"order by text\" ORDER BY created DESC".into(),
            ..Default::default()
        },
        Some("2026-10-03T12:00:00.000Z"),
    )?;
    assert!(q.contains("updated >= \"2026-10-01\""));
    assert!(q.contains("\"order by text\""));
    assert!(!q.contains("created DESC"));
    Ok(())
}
#[test]
fn mcp_allowlist_and_response_envelopes() -> Result<()> {
    use good_search::bifrost::*;
    assert!(allowed_name("mcp__bifrost__jira_getJiraIssue").is_some());
    assert!(allowed_name("jira-getJiraIssue").is_some());
    assert!(allowed_name("jira_service_user-getJiraIssue").is_none());
    assert!(allowed_name("jira_service_user_getJiraIssue").is_none());
    assert!(allowed_name("jira_editJiraIssue").is_none());
    let payload = tool_payload(json!({"content":[{"type":"text","text":"{\"issues\":[]}"}]}))?;
    assert_eq!(payload["issues"], json!([]));
    assert!(tool_payload(json!({"isError":true,"content":[]})).is_err());
    let rpc = decode_rpc(
        b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n",
        true,
        1,
    )?;
    assert!(rpc["result"].is_object());
    if decode_rpc(b"{}", false, 1).is_ok() {
        bail!("Missing response ID accepted")
    };
    Ok(())
}

#[test]
fn streamable_http_discovery_session_and_read_call() -> Result<()> {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> Result<()> {
        for expected in [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/call",
        ] {
            let (mut stream, _) = listener.accept()?;
            let mut reader = std::io::BufReader::new(stream.try_clone()?);
            let mut length = 0;
            let mut header = String::new();
            let mut authorization = false;
            let mut session = false;
            loop {
                header.clear();
                reader.read_line(&mut header)?;
                if header == "\r\n" {
                    break;
                }
                let lower = header.to_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse::<usize>()?;
                }
                authorization |= lower.trim() == "authorization: bearer fixture-key";
                session |= lower.trim() == "mcp-session-id: fixture-session";
            }
            assert!(authorization);
            if expected != "initialize" {
                assert!(session);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            let request: Value = serde_json::from_slice(&body)?;
            assert_eq!(request["method"], expected);
            if expected == "notifications/initialized" {
                stream.write_all(
                    b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )?;
                continue;
            }
            let result = match expected {
                "initialize" => {
                    json!({"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"fixture","version":"1"}})
                }
                "tools/list" => {
                    json!({"tools":[{"name":"jira_searchJiraIssuesUsingJql","inputSchema":{"type":"object","properties":{}}},{"name":"jira_getJiraIssue","inputSchema":{"type":"object"}},{"name":"jira_editJiraIssue","inputSchema":{"type":"object"}}]})
                }
                _ => {
                    assert_eq!(request["params"]["name"], "jira_searchJiraIssuesUsingJql");
                    json!({"structuredContent":{"issues":[],"isLast":true}})
                }
            };
            let body =
                serde_json::to_vec(&json!({"jsonrpc":"2.0","id":request["id"],"result":result}))?;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: fixture-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len())?;
            stream.write_all(&body)?;
        }
        Ok(())
    });
    let mut client = good_search::bifrost::Mcp::connect(good_search::bifrost::Connection {
        endpoint: format!("http://{address}/mcp"),
        api_key: "fixture-key".into(),
    })?;
    assert!(client.call("jira_editJiraIssue", json!({})).is_err());
    assert_eq!(
        client.call("jira_searchJiraIssuesUsingJql", json!({}))?["isLast"],
        true
    );
    server.join().unwrap()?;
    Ok(())
}
