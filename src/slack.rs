//! Read-only Slack channel and thread ingestion through Bifrost.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use base64::Engine;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub channel: String,
    pub workspace: String,
    pub since: String,
    pub update_lookback_days: u32,
    pub reload_generation: u64,
    pub poll_seconds: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self { channel: String::new(), workspace: String::new(), since: (chrono::Utc::now()-chrono::Duration::days(30)).format("%Y-%m-%d").to_string(), poll_seconds: 300, update_lookback_days: 30, reload_generation: 0 }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.channel.starts_with(['C','G','D']) && self.channel.len()>=8 && self.channel.chars().all(|c| c.is_ascii_alphanumeric()), "Enter a Slack channel ID (C..., G..., or D...)");
        let url=reqwest::Url::parse(&self.workspace)?;
        ensure!(url.scheme()=="https" && url.host_str().is_some_and(|h|h.ends_with(".slack.com")) && url.path()=="/" && url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none(), "Enter the workspace URL, e.g. https://your-team.slack.com");
        chrono::NaiveDate::parse_from_str(&self.since,"%Y-%m-%d").context("Start date must be YYYY-MM-DD")?;
        ensure!((1..=36500).contains(&self.update_lookback_days), "Update lookback must be 1–36500 days");
        ensure!((30..=86400).contains(&self.poll_seconds), "Poll interval must be 30–86400 seconds");
        Ok(())
    }
}
pub trait Provider {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value>;
}
impl Provider for crate::bifrost::Mcp {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value> { self.slack_read(tool,args) }
}
#[derive(Debug)]
pub struct Message { pub ts: String, pub author: String, pub body: String, pub replies: bool, pub files: Vec<String> }
pub struct Page { pub messages: Vec<Message>, pub next: Option<String> }
fn valid_ts(ts: &str) -> bool {
    ts.split_once('.').is_some_and(|(a,b)| !a.is_empty() && b.len()==6 && a.bytes().chain(b.bytes()).all(|c|c.is_ascii_digit()))
}
/// The deployed connector wraps a formatted transcript in MCP JSON text.
pub fn parse_page(raw: Value) -> Result<Page> {
    let value=crate::bifrost::tool_payload(raw)?;
    let body=value["messages"].as_str().context("Slack omitted message transcript")?;
    ensure!(body.len()<2*1024*1024 && !body.contains("[truncated]"), "Slack transcript is incomplete or too large");
    let mut messages=Vec::new();
    let lines: Vec<&str>=body.lines().collect();
    let mut header=String::new();
    let mut current: Option<Message>=None;
    for line in lines {
        if line.starts_with("=== Message from ") || line.starts_with("=== THREAD ") || line.starts_with("--- Reply ") {
            if let Some(m)=current.take() { messages.push(m); }
            header=line.trim().to_owned();
        } else if line.starts_with("From: ") || line.starts_with("Time: ") {
            header.push('\n'); header.push_str(line);
        } else if let Some(ts)=line.strip_prefix("Message TS: ") {
            ensure!(valid_ts(ts.trim()), "Invalid Slack message timestamp");
            ensure!(current.is_none(), "Unrecognized Slack transcript boundary");
            current=Some(Message {ts:ts.trim().into(),author:header.clone(),body:String::new(),replies:false,files:vec![]});
        } else if let Some(m)=&mut current {
            m.body.push_str(line); m.body.push('\n');
            if line.starts_with("Thread: ") { m.replies=true; }
            if line.starts_with("Files: ") {
                for part in line.split("(ID: ").skip(1) {
                    if let Some((id, rest))=part.split_once(',') {
                        if rest.contains("image/png") || rest.contains("image/jpeg") || rest.contains("image/webp") {
                            ensure!(id.starts_with('F') && id.chars().all(|c|c.is_ascii_alphanumeric()), "Invalid Slack file ID");
                            m.files.push(id.into());
                        }
                    }
                }
            }
        }
    }
    if let Some(m)=current { messages.push(m); }
    ensure!(!messages.is_empty() || body.trim().is_empty() || body.to_lowercase().contains("no messages"), "Unrecognized Slack transcript; refusing to advance");
    let pagination=value["pagination_info"].as_str().unwrap_or("");
    let next=if let Some((_,rest))=pagination.split_once("cursor: `") {
        let cursor=rest.split('`').next().unwrap_or("");
        ensure!(!cursor.is_empty(), "Slack omitted continuation cursor"); Some(cursor.into())
    } else {
        ensure!(pagination.to_lowercase().contains("no more messages") || (!pagination.to_lowercase().contains("more messages") && !pagination.to_lowercase().contains("more results")), "Slack pagination format is unsupported"); None
    };
    Ok(Page{messages,next})
}

pub fn image_data(raw: &Value) -> Result<(Vec<u8>, &'static str)> {
    ensure!(raw["isError"].as_bool()!=Some(true), "Slack file read failed");
    for block in raw["content"].as_array().context("Slack file response missing content")? {
        let (encoded,mime)=if block["type"]=="image" {
            (block["data"].as_str().unwrap_or(""),block["mimeType"].as_str().unwrap_or(""))
        } else if let Some(text)=block["text"].as_str() {
            let Some((_,rest))=text.split_once("[Image Response: ") else { continue; };
            let Some((data,mime))=rest.split_once(", MIME: ") else { continue; };
            (data,mime.split(']').next().unwrap_or(""))
        } else {continue};
        let ext=match mime {"image/png"=>"png","image/jpeg"=>"jpg","image/webp"=>"webp",_=>continue};
        ensure!(encoded.len()<=14*1024*1024,"Slack image exceeds size limit");
        let bytes=base64::engine::general_purpose::STANDARD.decode(encoded)?;
        ensure!(bytes.len()<=10*1024*1024,"Slack image exceeds 10 MiB");
        return Ok((bytes,ext));
    }
    anyhow::bail!("Bifrost did not return supported image bytes")
}

pub fn sync(db: &mut Connection, source: &str, s: &Settings, provider: &mut impl Provider, cancel: impl Fn()->bool) -> Result<crate::jira::Outcome> {
    use crate::jira::{now,Outcome};
    s.validate()?;
    // Poll frequency and rolling lookback do not invalidate the history import.
    let scope=blake3::hash(serde_json::to_string(&json!(["slack-incremental-v1",s.channel,s.workspace,s.since,s.reload_generation]))?.as_bytes()).to_hex().to_string();
    db.execute("INSERT INTO remote_sync(source_id,scope_hash) VALUES(?1,?2) ON CONFLICT DO NOTHING",params![source,scope])?;
    db.execute("UPDATE remote_sync SET scope_hash=?2,run_id=NULL,next_token=NULL,last_success=NULL,checkpoint=NULL,candidate=NULL WHERE source_id=?1 AND scope_hash!=?2",params![source,scope])?;
    let started=now();
    let checkpoint:Option<String>=db.query_row("SELECT checkpoint FROM remote_sync WHERE source_id=?1",[source],|r|r.get(0))?;
    let latest_time=chrono::Utc::now().timestamp();
    let lower=scan_start(s,checkpoint.as_deref().map(str::parse::<i64>).transpose()?,latest_time)?;
    // Pin both bounds for all pages/retries. Downtime expands the window back to
    // the previous completed scan, so new messages cannot fall through a gap.
    db.execute("UPDATE remote_sync SET run_id=?2,run_jql=?3,candidate=?4,next_token=NULL,pages=0,items=0 WHERE source_id=?1 AND run_id IS NULL",params![source,started,latest_time.to_string(),lower.to_string()])?;
    let (run,latest,oldest,mut cursor):(String,String,String,Option<String>)=db.query_row("SELECT run_id,run_jql,candidate,next_token FROM remote_sync WHERE source_id=?1",[source],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let mut seen=std::collections::HashSet::new();
    if let Some(c)=&cursor {seen.insert(c.clone());}
    for _ in 0..5 {
        if cancel() { return Ok(Outcome::Cancelled); }
        let mut args=json!({"channel_id":s.channel,"limit":50,"oldest":oldest,"latest":latest,"response_format":"detailed"});
        if let Some(c)=&cursor {args["cursor"]=json!(c);}
        let mut page=parse_page(provider.read("slack_read_channel",args)?)?;
        if let Some(c)=&page.next {ensure!(seen.insert(c.clone()),"Slack repeated a continuation cursor");}
        let parents:Vec<String>=page.messages.iter().filter(|m|m.replies).map(|m|m.ts.clone()).collect();
        for ts in parents {
            let mut thread_cursor=None;
            let mut thread_seen=std::collections::HashSet::new();
            for n in 0..100 {
                if cancel() {return Ok(Outcome::Cancelled);}
                let mut args=json!({"channel_id":s.channel,"message_ts":ts,"limit":100,"response_format":"detailed"});
                if let Some(c)=&thread_cursor {args["cursor"]=json!(c);}
                let replies=parse_page(provider.read("slack_read_thread",args)?)?;
                page.messages.extend(replies.messages.into_iter().filter(|m|m.ts!=ts));
                thread_cursor=replies.next;
                let Some(c)=&thread_cursor else {break};
                ensure!(thread_seen.insert(c.clone()) && n<99,"Slack thread pagination did not complete");
            }
        }
        if cancel() {return Ok(Outcome::Cancelled);}
        let tx=db.transaction()?;
        for m in &page.messages {publish(&tx,source,s,&run,&scope,m)?;}
        tx.execute("UPDATE remote_sync SET next_token=?2,pages=pages+1,items=items+?3,error='',failures=0,retry_at=0,last_attempt=?4 WHERE source_id=?1",params![source,page.next,page.messages.len() as i64,now()])?;
        if page.next.is_none() {tx.execute("UPDATE remote_sync SET run_id=NULL,last_success=?2,checkpoint=run_jql WHERE source_id=?1",params![source,now()])?;}
        tx.commit()?;
        if page.next.is_none() {return Ok(Outcome::Complete);}
        cursor=page.next;
    }
    Ok(Outcome::More)
}
pub fn scan_start(s:&Settings,checkpoint:Option<i64>,now:i64)->Result<i64> {
    let history=chrono::NaiveDate::parse_from_str(&s.since,"%Y-%m-%d")?.and_hms_opt(0,0,0).unwrap().and_utc().timestamp();
    Ok(checkpoint.map_or(history,|last| history.max(last.saturating_sub(1).min(now.saturating_sub(i64::from(s.update_lookback_days)*86400)))))
}
fn publish(db:&Connection,source:&str,s:&Settings,run:&str,scope:&str,m:&Message)->Result<()> {
    use rusqlite::OptionalExtension;
    let uri=format!("good-search://slack/{source}/{}/{}",s.channel,m.ts);
    let title=format!("{} · {}",s.channel,m.body.lines().next().unwrap_or("Message").chars().take(120).collect::<String>());
    let text=format!("{}\n{}\n\n{}",s.channel,m.author,m.body);
    let hash=blake3::hash(text.as_bytes()).to_hex().to_string();
    db.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET name=excluded.name,folded=excluded.folded",params![format!("slack:{source}"),uri,title,title.to_lowercase()])?;
    let file:i64=db.query_row("SELECT id FROM files WHERE path=?1",[uri],|r|r.get(0))?;
    let old:Option<String>=db.query_row("SELECT content_hash FROM remote_items WHERE file_id=?1",[file],|r|r.get(0)).optional()?;
    let url=format!("{}/archives/{}/p{}",s.workspace.trim_end_matches('/'),s.channel,m.ts.replace('.',""));
    // Slack's deployed transcript omits edit timestamps; do not label the posted time as updated.
    let raw=serde_json::to_string(&json!({"files":m.files,"ts":m.ts}))?;
    db.execute("INSERT INTO remote_items(file_id,source_id,connection_id,site,remote_id,issue_key,title,original_url,remote_updated,synced_at,content_hash,raw_content,searchable_text,warnings,last_seen_run,scope_hash,content_format) VALUES(?1,?2,'default',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'[]',?13,?14,'slack') ON CONFLICT(file_id) DO UPDATE SET title=excluded.title,original_url=excluded.original_url,synced_at=excluded.synced_at,content_hash=excluded.content_hash,raw_content=excluded.raw_content,searchable_text=excluded.searchable_text,last_seen_run=excluded.last_seen_run,scope_hash=excluded.scope_hash",params![file,source,s.workspace,format!("{}/{}",s.channel,m.ts),s.channel,title,url,m.ts,crate::jira::now(),hash,raw,text,run,scope])?;
    if old.as_deref()==Some(&hash) {return Ok(());}
    db.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(?1,'text',?2,1,1,'ready','') ON CONFLICT(file_id,kind) DO UPDATE SET source_hash=excluded.source_hash",params![file,hash])?;
    let doc:i64=db.query_row("SELECT id FROM documents WHERE file_id=?1 AND kind='text'",[file],|r|r.get(0))?;
    db.execute("DELETE FROM chunks WHERE document_id=?1",[doc])?;
    for (i,c) in crate::content::chunks(&text).into_iter().enumerate() {db.execute("INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![doc,i as i64,c.heading,c.line as i64,c.body,blake3::hash(c.body.as_bytes()).to_hex().to_string()])?;}
    Ok(())
}

pub fn sync_images(db:&mut Connection,source:&str,provider:&mut impl Provider,cache:&Path,cancel:impl Fn()->bool)->Result<()> {
    let parents:Vec<(i64,String,String)>=db.prepare("SELECT r.file_id,r.raw_content,r.content_hash FROM remote_items r LEFT JOIN attachment_scans s ON s.parent_file_id=r.file_id WHERE r.source_id=?1 AND (s.remote_updated IS NULL OR s.remote_updated!=r.content_hash) LIMIT 50")?.query_map([source],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    std::fs::create_dir_all(cache)?;
    for (parent,raw,revision) in parents {
        let raw:Value=serde_json::from_str(&raw)?;
        let ids=raw["files"].as_array().context("Missing Slack file list")?;
        for id in ids {
            if cancel() {return Ok(());}
            let id=id.as_str().context("Invalid Slack file ID")?;
            let exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM attachments WHERE parent_file_id=?1 AND remote_id=?2)",params![parent,id],|r|r.get(0))?;
            if exists {continue;}
            let (bytes,ext)=image_data(&provider.read("slack_read_file",json!({"file_id":id}))?)?;
            let hash=blake3::hash(&bytes).to_hex().to_string();
            let identity=blake3::hash(format!("slack/{source}/{parent}/{id}").as_bytes()).to_hex().to_string();
            let path=cache.join(format!("{identity}-{hash}.{ext}"));
            std::fs::write(&path,&bytes)?;
            crate::images::image_bytes(&path)?;
            image::load_from_memory(&bytes)?.thumbnail(64,64).save(path.with_extension("thumb.png"))?;
            if cancel() {return Ok(());}
            let tx=db.transaction()?;
            let name=format!("{id}.{ext} · Slack");
            tx.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO NOTHING",params![format!("slack:{source}"),path.to_string_lossy(),name,name.to_lowercase()])?;
            let file:i64=tx.query_row("SELECT id FROM files WHERE path=?1",[path.to_string_lossy()],|r|r.get(0))?;
            tx.execute("INSERT OR IGNORE INTO attachments VALUES(?1,?2,'slack',?3,?4,?5,?6,?7,?8,?9)",params![file,parent,source,id,name,format!("image/{}",if ext=="jpg"{"jpeg"}else{ext}),bytes.len() as i64,hash,crate::jira::now()])?;
            tx.commit()?;
        }
        if cancel() {return Ok(());}
        db.execute("DELETE FROM files WHERE id IN (SELECT file_id FROM attachments WHERE parent_file_id=?1 AND remote_id NOT IN (SELECT value FROM json_each(?2)))",params![parent,serde_json::to_string(ids)?])?;
        db.execute("INSERT OR REPLACE INTO attachment_scans VALUES(?1,?2)",params![parent,revision])?;
    }
    Ok(())
}
