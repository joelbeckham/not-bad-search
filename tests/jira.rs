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

#[test]
fn metadata_refreshes_without_replacing_content_and_reaches_attachments() -> Result<()> {
    let mut db = open_db(std::path::Path::new(":memory:"))?;
    let mut ticket = issue("1", "metadata needle", "2026-10-01T12:00:00Z");
    ticket["fields"]["status"] = json!({"name":"In Review","statusCategory":{"key":"indeterminate"}});
    ticket["fields"]["assignee"] = Value::Null;
    ticket["fields"]["issuetype"] = json!({"name":"Bug"});
    ticket["fields"]["priority"] = json!({"name":"High"});
    ticket["fields"]["project"] = json!({"name":"Core"});
    ticket["fields"]["created"] = json!("2026-09-01T12:00:00Z");
    ticket["fields"]["reporter"] = json!({"displayName":"Reporter"});
    ticket["fields"]["labels"] = json!(["customer"]);
    let mut fake = Fake::new(vec![Some(page(vec![ticket.clone()], None))]);
    jira::sync(&mut db, "source", &Settings::default(), &mut fake, || false)?;
    let hits = good_search::search(&db, "needle", 10)?;
    let metadata = hits[0].jira.as_ref().unwrap();
    assert_eq!(metadata.status, "In Review");
    assert_eq!(metadata.category, "indeterminate");
    assert_eq!(metadata.assignee, "Unassigned");
    assert_eq!(metadata.issue_type, "Bug");
    assert_eq!(metadata.priority, "High");
    assert_eq!(metadata.project, "Core");
    assert_eq!(metadata.reporter, "Reporter");
    assert_eq!(metadata.labels, ["customer"]);
    assert_eq!(metadata.created, "2026-09-01T12:00:00Z");
    db.execute_batch("CREATE TRIGGER no_chunk_replacement BEFORE DELETE ON chunks BEGIN SELECT RAISE(ABORT,'metadata must preserve chunks'); END;
        INSERT INTO files(root,path,name,folded) VALUES('jira:source','attachment.png','attachment','attachment');
        INSERT INTO attachments SELECT last_insert_rowid(),file_id,'jira','source','123','attachment.png','image/png',1,'hash','now' FROM remote_items;")?;
    ticket["fields"]["status"] = json!({"name":"Resolved","statusCategory":{"key":"done"}});
    ticket["fields"]["updated"] = json!("2026-10-02T12:00:00Z");
    fake.pages.push_back(Some(page(vec![ticket], None)));
    jira::sync(&mut db, "source", &Settings::default(), &mut fake, || false)?;
    let hits = good_search::search(&db, "attachment", 10)?;
    let metadata = hits[0].jira.as_ref().unwrap();
    assert_eq!(metadata.status, "Resolved");
    assert_eq!(metadata.category, "done");
    assert_eq!(metadata.updated, "2026-10-02T12:00:00.000Z");
    assert_eq!(metadata.key, "CORE-1");
    Ok(())
}

#[test]
fn mixed_page_formats_resolve_per_issue_and_refresh_unchanged_items() -> Result<()> {
    let mut db=open_db(std::path::Path::new(":memory:"))?;
    let mut markdown=issue("1","","2026-10-01T12:00:00Z");
    markdown["fields"]["description"]=json!("**markdownneedle**");
    markdown["appliedContentFormat"]=json!("markdown");
    let mut html=issue("2","","2026-10-01T12:00:00Z");
    html["fields"]["description"]=json!("<p>htmlneedle</p>");
    html["fields"]["appliedContentFormat"]=json!("html");
    let mut ambiguous=issue("3","","2026-10-01T12:00:00Z");
    ambiguous["fields"]["description"]=json!("ambiguous");
    let mut mixed=page(vec![markdown,html,ambiguous],None);
    let mut empty=issue("4","","2026-10-01T12:00:00Z");
    empty["fields"].as_object_mut().unwrap().remove("description");
    mixed["issues"].as_array_mut().unwrap().push(empty);
    mixed["appliedContentFormat"]=json!("mixed");
    let normalized=jira::normalize_format(mixed);
    let mut detail=issue("3","","2026-10-01T12:00:00Z");
    detail["fields"]["description"]=json!("<p>resolvedneedle</p>");
    detail["appliedContentFormat"]=json!("html");
    let mut fake=Fake::new(vec![Some(normalized.clone())]);fake.detail=Some(detail);
    jira::sync(&mut db,"source",&Settings::default(),&mut fake,||false)?;
    assert_eq!(fake.detail_calls,1);
    for query in ["markdownneedle","htmlneedle","resolvedneedle"] {assert_eq!(good_search::search(&db,query,10)?.len(),1);}
    fake.pages.push_back(Some(normalized));
    jira::sync(&mut db,"source",&Settings::default(),&mut fake,||false)?;
    assert_eq!(fake.detail_calls,2);
    assert_eq!(db.query_row("SELECT count(*) FROM remote_items WHERE content_format='mixed'",[],|r|r.get::<_,i64>(0))?,0);
    Ok(())
}

#[test]
fn markdown_html_reload_and_chunk_replacement() -> Result<()> {
    let markdown="## Heading\n\n**needle** [link](https://example.com/path)\n\n- task";
    assert_eq!(jira::extract_description(&json!(markdown),"markdown")?.text,markdown);
    let html=jira::extract_description(&json!("<h2>Heading</h2><p>needle &amp; text <a href='https://example.com/path'>link</a></p><table><tr><td>alpha</td><td>beta</td></tr></table>"),"html")?;
    assert!(html.text.contains("needle & text") && html.text.contains("https://example.com/path") && html.text.contains("alpha"));
    let mut db=open_db(std::path::Path::new(":memory:"))?;
    let mut settings=Settings::default();
    let mut first=issue("1","oldword","2026-10-01T12:00:00Z");
    let mut fake=Fake::new(vec![Some(page(vec![first.clone()],None))]);
    jira::sync(&mut db,"source",&settings,&mut fake,||false)?;
    first["fields"]["description"]=json!(markdown);first["appliedContentFormat"]=json!("markdown");
    settings.reload_generation+=1;
    fake.pages.push_back(Some(page(vec![first],None)));
    jira::sync(&mut db,"source",&settings,&mut fake,||false)?;
    assert!(good_search::search(&db,"oldword",10)?.is_empty());
    assert_eq!(good_search::search(&db,"needle",10)?.len(),1);
    assert_eq!(db.query_row("SELECT content_format FROM remote_items",[],|r|r.get::<_,String>(0))?,"markdown");
    let mut fallback=issue("1","","2026-10-01T12:00:00Z");
    fallback["fields"]["description"]=json!("<p>htmlneedle &amp; text</p>");
    fallback["appliedContentFormat"]=json!("html");
    fake.pages.push_back(Some(page(vec![fallback],None)));
    jira::sync(&mut db,"source",&settings,&mut fake,||false)?;
    assert_eq!(good_search::search(&db,"htmlneedle",10)?.len(),1);
    assert_eq!(db.query_row("SELECT content_format FROM remote_items",[],|r|r.get::<_,String>(0))?,"html");
    assert!(!db.query_row("SELECT searchable_text FROM remote_items",[],|r|r.get::<_,String>(0))?.contains("<p>"));
    assert_eq!(db.query_row("SELECT count(*) FROM remote_items",[],|r|r.get::<_,i64>(0))?,1);
    Ok(())
}

#[test]
fn attachment_search_preview_pause_and_parent_removal() -> Result<()> {
    use good_search::{images, Config, SearchFilters};
    let dir=tempfile::tempdir()?;
    let path=dir.path().join("image.png");
    image::RgbImage::from_pixel(8,8,image::Rgb([1,2,3])).save(&path)?;
    let mut db=open_db(std::path::Path::new(":memory:"))?;
    let mut fake=Fake::new(vec![Some(page(vec![issue("1","parent","2026-10-01T12:00:00Z")],None))]);
    jira::sync(&mut db,"source",&Settings::default(),&mut fake,||false)?;
    let parent:i64=db.query_row("SELECT file_id FROM remote_items",[],|r|r.get(0))?;
    db.execute("INSERT INTO files(root,path,name,folded) VALUES('jira:source',?1,'screenshot · CORE-1','screenshot · core-1')",[path.to_string_lossy()])?;
    let file=db.last_insert_rowid();
    db.execute("INSERT INTO attachments VALUES(?1,?2,'jira','source','123','screenshot.png','image/png',1,'hash','now')",rusqlite::params![file,parent])?;
    let mut source=good_search::sources::Source::jira_source();source.id="source".into();source.paused=false;
    let mut config=Config{sources:vec![source],..Default::default()};
    images::discover(&mut db,&config)?;
    struct Extract;
    impl images::Extractor for Extract {fn extract(&self,_:&str,_:&str,_:&[u8])->Result<images::Extraction>{Ok(images::Extraction{extracted_text:"screenshot needle".into(),description:"diagram".into()})}}
    assert!(images::process_one(&mut db,&Extract,||config.clone())?);
    let filters=SearchFilters{text:false,roots:Some(vec!["jira:source".into()]),..Default::default()};
    let hits=good_search::search_filtered(&db,"screenshot",10,filters)?;
    assert_eq!(hits.len(),1);
    assert_eq!(hits[0].original_url.as_deref(),Some("https://bombbomb.atlassian.net/browse/CORE-1"));
    assert_eq!(jira::open_target(&db,&hits[0].path)?,hits[0].original_url.clone().unwrap());
    assert!(matches!(good_search::preview::load(&path,"")?,good_search::preview::Content::Image{..}));
    config.sources[0].paused=true;
    assert!(!images::process_one(&mut db,&Extract,||config.clone())?);
    assert_eq!(good_search::search(&db,"needle",10)?.len(),1);
    db.execute("DELETE FROM files WHERE id=?1",[parent])?;
    assert!(good_search::search(&db,"needle",10)?.is_empty());
    assert_eq!(db.query_row("SELECT count(*) FROM files",[],|r|r.get::<_,i64>(0))?,0);
    Ok(())
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
    assert!(inspected["remote_item"][0]["raw_content"]
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
