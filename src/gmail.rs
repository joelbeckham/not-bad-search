//! Read-only Gmail conversation indexing through Bifrost.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub query: String,
    pub since: String,
    pub poll_seconds: u64,
    pub reload_generation: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            query: "in:inbox".into(),
            since: (chrono::Utc::now() - chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string(),
            poll_seconds: 300,
            reload_generation: 0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        chrono::NaiveDate::parse_from_str(&self.since, "%Y-%m-%d")
            .context("History start date must be YYYY-MM-DD")?;
        ensure!(self.query.len() <= 4096, "Gmail query is too long");
        ensure!(
            (30..=86400).contains(&self.poll_seconds),
            "Poll interval must be 30-86400 seconds"
        );
        Ok(())
    }
}
pub trait Provider {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value>;
}
impl Provider for crate::bifrost::Mcp {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value> {
        self.gmail_read(tool, args)
    }
}
pub fn sync(
    db: &mut Connection,
    source: &str,
    s: &Settings,
    p: &mut impl Provider,
    cancel: impl Fn() -> bool,
) -> Result<crate::jira::Outcome> {
    use crate::jira::{now, Outcome};
    s.validate()?;
    let scope = blake3::hash(
        serde_json::to_string(&json!(["gmail-v1", s.query, s.since, s.reload_generation]))?
            .as_bytes(),
    )
    .to_hex()
    .to_string();
    db.execute(
        "INSERT INTO remote_sync(source_id,scope_hash) VALUES(?1,?2) ON CONFLICT DO NOTHING",
        params![source, scope],
    )?;
    db.execute("UPDATE remote_sync SET scope_hash=?2,run_id=NULL,next_token=NULL,last_success=NULL,checkpoint=NULL WHERE source_id=?1 AND scope_hash!=?2",params![source,scope])?;
    // Search matching conversations again so label changes and replies on old threads are included.
    // Gmail's connector has no history/change-feed endpoint. Hashes avoid re-embedding unchanged text.
    let query = format!(
        "({}) after:{}",
        if s.query.trim().is_empty() {
            "-in:trash"
        } else {
            &s.query
        },
        s.since.replace('-', "/")
    );
    db.execute("UPDATE remote_sync SET run_id=?2,run_jql=?3,next_token=NULL,pages=0,items=0 WHERE source_id=?1 AND run_id IS NULL",params![source,now(),query])?;
    let (run, query, mut cursor): (String, String, Option<String>) = db.query_row(
        "SELECT run_id,run_jql,next_token FROM remote_sync WHERE source_id=?1",
        [source],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if cancel() {
        return Ok(Outcome::Cancelled);
    }
    let label_response = p.read("list_labels", json!({}))?;
    let mut label_names = serde_json::Map::new();
    for label in label_response["labels"]
        .as_array()
        .context("Gmail omitted labels")?
    {
        if label["labelType"] == "USER" {
            if let (Some(id), Some(name)) = (label["labelId"].as_str(), label["name"].as_str()) {
                label_names.insert(id.into(), json!(name));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(c) = &cursor {
        seen.insert(c.clone());
    }
    for _ in 0..5 {
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let mut args = json!({"query":query,"pageSize":20,"view":"THREAD_VIEW_IDS_ONLY"});
        if let Some(c) = &cursor {
            args["pageToken"] = json!(c);
        }
        let page = p.read("search_threads", args)?;
        let threads = page
            .get("threads")
            .map(|v| v.as_array().context("Invalid Gmail thread list"))
            .transpose()?;
        ensure!(
            threads.is_some()
                || page
                    .as_object()
                    .is_some_and(|o| o.is_empty() || o.contains_key("resultCountEstimate")),
            "Unrecognized Gmail search response"
        );
        let next = page
            .get("nextPageToken")
            .map(|v| v.as_str().context("Invalid Gmail page token"))
            .transpose()?
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if let Some(c) = &next {
            ensure!(seen.insert(c.clone()), "Gmail repeated a page token");
        }
        let mut fetched = Vec::new();
        for t in threads.into_iter().flatten() {
            if cancel() {
                return Ok(Outcome::Cancelled);
            }
            let id = t["id"].as_str().context("Gmail omitted thread ID")?;
            let mut thread = p.read(
                "get_thread",
                json!({"threadId":id,"messageFormat":"PLAIN_TEXT"}),
            )?;
            ensure!(thread["id"] == id, "Gmail returned the wrong conversation");
            thread["labelNames"] = json!(label_names);
            fetched.push(thread);
        }
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let tx = db.transaction()?;
        for t in &fetched {
            publish(&tx, source, &run, &scope, t)?;
        }
        tx.execute("UPDATE remote_sync SET next_token=?2,pages=pages+1,items=items+?3,error='',failures=0,retry_at=0,last_attempt=?4 WHERE source_id=?1",params![source,next,fetched.len() as i64,now()])?;
        if next.is_none() {
            tx.execute(
                "UPDATE remote_sync SET run_id=NULL,last_success=?2 WHERE source_id=?1",
                params![source, now()],
            )?;
        }
        tx.commit()?;
        if next.is_none() {
            return Ok(Outcome::Complete);
        }
        cursor = next;
    }
    Ok(Outcome::More)
}
fn display(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        s.into()
    } else if let Some(a) = v.as_array() {
        a.iter().map(display).collect::<Vec<_>>().join(", ")
    } else if v.is_null() {
        String::new()
    } else {
        v.to_string()
    }
}
fn publish(db: &Connection, source: &str, run: &str, scope: &str, t: &Value) -> Result<()> {
    let id = t["id"].as_str().context("Gmail omitted thread ID")?;
    ensure!(
        !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Invalid Gmail thread ID"
    );
    let messages = t["messages"]
        .as_array()
        .context("Gmail omitted conversation messages")?;
    ensure!(!messages.is_empty(), "Gmail returned an empty conversation");
    if let Some(count) = t["messageCount"].as_u64() {
        ensure!(
            count as usize == messages.len(),
            "Gmail returned an incomplete conversation"
        );
    }
    let title = messages[0]["subject"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("(No subject)");
    let url = t["viewUrl"]
        .as_str()
        .or_else(|| messages[0]["viewUrl"].as_str())
        .context("Gmail omitted conversation link")?;
    let parsed = reqwest::Url::parse(url)?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str() == Some("mail.google.com")
            && parsed.username().is_empty()
            && parsed.password().is_none(),
        "Invalid Gmail conversation link"
    );
    let mut text = format!("{title}\n\n");
    let mut latest = None;
    for m in messages {
        let date = m["date"].as_str().context("Gmail omitted message date")?;
        let date = chrono::DateTime::parse_from_rfc3339(date)?;
        latest = Some(latest.map_or(date, |d: chrono::DateTime<chrono::FixedOffset>| d.max(date)));
        text.push_str(&format!(
            "From: {}\nTo: {}\nDate: {}\n",
            display(&m["sender"]),
            display(&m["toRecipients"]),
            date.to_rfc3339()
        ));
        if !m["ccRecipients"].is_null() {
            text.push_str(&format!("Cc: {}\n", display(&m["ccRecipients"])));
        }
        text.push('\n');
        text.push_str(
            m["plaintextBody"]
                .as_str()
                .context("Gmail omitted plain text body")?,
        );
        text.push_str("\n\n---\n\n");
    }
    ensure!(
        text.len() <= 8 * 1024 * 1024,
        "Gmail conversation exceeds indexing size limit"
    );
    let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
    let uri = format!("not-bad-search://gmail/{source}/{id}");
    db.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET name=excluded.name,folded=excluded.folded",params![format!("gmail:{source}"),uri,title,title.to_lowercase()])?;
    let file: i64 = db.query_row("SELECT id FROM files WHERE path=?1", [uri], |r| r.get(0))?;
    let old: Option<String> = db
        .query_row(
            "SELECT content_hash FROM remote_items WHERE file_id=?1",
            [file],
            |r| r.get(0),
        )
        .optional()?;
    db.execute("INSERT INTO remote_items(file_id,source_id,connection_id,site,remote_id,issue_key,title,original_url,remote_updated,synced_at,content_hash,raw_content,searchable_text,warnings,last_seen_run,scope_hash,content_format) VALUES(?1,?2,'default','https://mail.google.com',?3,?3,?4,?5,?6,?7,?8,?9,?10,'[]',?11,?12,'gmail') ON CONFLICT(file_id) DO UPDATE SET title=excluded.title,original_url=excluded.original_url,remote_updated=excluded.remote_updated,synced_at=excluded.synced_at,content_hash=excluded.content_hash,raw_content=excluded.raw_content,searchable_text=excluded.searchable_text,last_seen_run=excluded.last_seen_run,scope_hash=excluded.scope_hash",params![file,source,id,title,url,latest.unwrap().to_rfc3339(),crate::jira::now(),hash,serde_json::to_string(t)?,text,run,scope])?;
    if old.as_deref() == Some(&hash) {
        return Ok(());
    }
    db.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(?1,'text',?2,1,1,'ready','') ON CONFLICT(file_id,kind) DO UPDATE SET source_hash=excluded.source_hash,status='ready'",params![file,hash])?;
    let doc: i64 = db.query_row(
        "SELECT id FROM documents WHERE file_id=?1 AND kind='text'",
        [file],
        |r| r.get(0),
    )?;
    db.execute("DELETE FROM chunks WHERE document_id=?1", [doc])?;
    for (i, c) in crate::content::chunks(&text).into_iter().enumerate() {
        db.execute("INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![doc,i as i64,c.heading,c.line as i64,c.body,blake3::hash(c.body.as_bytes()).to_hex().to_string()])?;
    }
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub from: String,
    pub date: String,
    pub labels: Vec<String>,
    pub messages: usize,
}
impl Metadata {
    pub fn from_thread(t: &Value) -> Self {
        let messages = t["messages"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let latest = messages.iter().max_by_key(|m| {
            m["date"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp())
        });
        let Some(m) = latest else {
            return Self::default();
        };
        let address = m["sender"].as_str().unwrap_or("");
        let name = m["addressDisplayNames"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["emailAddress"] == address))
            .and_then(|v| v["displayName"].as_str())
            .filter(|s| !s.is_empty());
        let from = name
            .map(|n| format!("{n} <{address}>"))
            .unwrap_or_else(|| address.into());
        let mut labels = Vec::new();
        for message in messages {
            for id in message["labelIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let label = t["labelNames"][id]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| match id {
                        "INBOX" => "Inbox".into(),
                        "UNREAD" => "Unread".into(),
                        "STARRED" => "Starred".into(),
                        "IMPORTANT" => "Important".into(),
                        "SENT" => "Sent".into(),
                        "DRAFT" => "Draft".into(),
                        "TRASH" => "Trash".into(),
                        "SPAM" => "Spam".into(),
                        _ if id.starts_with("CATEGORY_") => {
                            let s = id.trim_start_matches("CATEGORY_").to_lowercase();
                            format!("{}{}", s[..1].to_uppercase(), &s[1..])
                        }
                        _ => String::new(),
                    });
                if !label.is_empty() && !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        labels.sort();
        Self {
            from,
            date: m["date"].as_str().unwrap_or("").into(),
            labels,
            messages: messages.len(),
        }
    }
}
pub fn metadata(db: &Connection, path: &str) -> Result<Option<Metadata>> {
    let raw:Option<String>=db.query_row("SELECT raw_content FROM remote_items r JOIN files f ON f.id=r.file_id WHERE f.path=?1 AND r.content_format='gmail'",[path],|r|r.get(0)).optional()?;
    Ok(raw
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .map(|t| Metadata::from_thread(&t)))
}
