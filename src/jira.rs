use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub site: String,
    pub cloud_id: String,
    pub project: String,
    pub jql: String,
    pub poll_seconds: u64,
    pub page_size: usize,
    pub reload_generation: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            site: "https://bombbomb.atlassian.net".into(),
            cloud_id: "bombbomb.atlassian.net".into(),
            project: "CORE".into(),
            jql: "project = CORE".into(),
            poll_seconds: 300,
            page_size: 2,
            reload_generation: 0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.site)?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "Jira site must be an HTTPS site URL without a path or credentials"
        );
        ensure!(
            !self.cloud_id.is_empty()
                && !self.project.is_empty()
                && self
                    .project
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
            "Enter a valid Jira project key and cloud ID"
        );
        ensure!(
            !self.jql.trim().is_empty() && self.jql.len() <= 8192,
            "Enter a bounded JQL query"
        );
        ensure!(
            (30..=86400).contains(&self.poll_seconds) && (1..=100).contains(&self.page_size),
            "Jira polling must be 30–86400 seconds and pages 1–100 issues"
        );
        Ok(())
    }
}
pub fn normalized_timestamp(value: &str) -> Result<String> {
    let date = DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f%z"))?;
    Ok(date
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub status: String,
    pub category: String,
    pub assignee: String,
    pub issue_type: String,
    pub priority: String,
    pub project: String,
    pub created: String,
    pub reporter: String,
    pub labels: Vec<String>,
    pub key: String,
    pub updated: String,
    pub synced: String,
}
impl Metadata {
    fn from_fields(f: &Value) -> Self {
        let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
        Self {
            status: text(&f["status"]["name"]),
            category: text(&f["status"]["statusCategory"]["key"]),
            assignee: if f.get("assignee").is_some_and(Value::is_null) { "Unassigned".into() } else { text(&f["assignee"]["displayName"]) },
            issue_type: text(&f["issuetype"]["name"]),
            priority: text(&f["priority"]["name"]),
            project: text(&f["project"]["name"]),
            created: text(&f["created"]),
            reporter: text(&f["reporter"]["displayName"]),
            labels: f["labels"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
            ..Self::default()
        }
    }
}

pub fn metadata(db: &Connection, path: &str) -> Result<Option<Metadata>> {
    let row: Option<(String, String, String, String)> = db.query_row(
        "SELECT r.metadata,r.issue_key,r.remote_updated,r.synced_at FROM files f LEFT JOIN attachments a ON a.file_id=f.id JOIN remote_items r ON r.file_id=coalesce(a.parent_file_id,f.id) WHERE f.path=?1",
        [path], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).optional()?;
    row.map(|(json,key,updated,synced)| {
        let mut value: Metadata = serde_json::from_str(&json)?;
        value.key=key; value.updated=updated; value.synced=synced;
        Ok(value)
    }).transpose()
}
pub fn scope_jql(settings: &Settings, checkpoint: Option<&str>) -> Result<String> {
    let input = settings.jql.trim();
    let lower = input.to_ascii_lowercase();
    let mut quote = None;
    let mut escaped = false;
    let mut end = input.len();
    for (i, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if lower[i..].starts_with("order by ") {
            end = i;
            break;
        }
    }
    ensure!(quote.is_none(), "Unclosed quote in JQL");
    let mut query = format!(
        "project = \"{}\" AND ({})",
        settings.project,
        input[..end].trim()
    );
    if let Some(checkpoint) = checkpoint {
        // Date-only lower bound intentionally overlaps at least 48 hours, covering
        // Jira user/site timezone differences without relying on its timezone setting.
        let lower = DateTime::parse_from_rfc3339(checkpoint)? - chrono::Duration::hours(48);
        query.push_str(&format!(" AND updated >= \"{}\"", lower.format("%Y-%m-%d")));
    }
    query.push_str(" ORDER BY updated ASC");
    Ok(query)
}
pub trait Provider {
    fn page(&mut self, settings: &Settings, jql: &str, token: Option<&str>) -> Result<Value>;
    fn issue(&mut self, settings: &Settings, key: &str) -> Result<Value>;
}
impl Provider for crate::bifrost::Mcp {
    fn page(&mut self, s: &Settings, jql: &str, token: Option<&str>) -> Result<Value> {
        let mut args = json!({"cloudId":s.cloud_id,"jql":jql,"maxResults":s.page_size,"fields":["id","key","summary","description","updated","status","assignee","issuetype","priority","project","created","reporter","labels"],"searchResultMode":"issues","view":"full","responseContentFormat":"markdown"});
        if let Some(token) = token {
            args["nextPageToken"] = token.into();
        }
        self.call("jira_searchJiraIssuesUsingJql", args).map(normalize_format)
    }
    fn issue(&mut self, s: &Settings, key: &str) -> Result<Value> {
        let mut args=json!({"cloudId":s.cloud_id,"issueIdOrKey":key,"fields":["summary","description","updated","status","assignee","issuetype","priority","project","created","reporter","labels"],"responseContentFormat":"markdown","view":"full"});
        let value=normalize_format(self.call("jira_getJiraIssue",args.clone())?);
        if description_format(&value)==Some("mixed") {
            args["responseContentFormat"]=json!("html");
            return self.call("jira_getJiraIssue",args).map(normalize_format);
        }
        Ok(value)
    }
}
pub fn normalize_format(mut value: Value) -> Value {
    let format=value.get("appliedContentFormat").cloned().unwrap_or(json!("markdown"));
    if let Some(issues)=value.get_mut("issues").and_then(Value::as_array_mut) {
        for issue in issues {normalize_issue(issue,&format);}
    } else {normalize_issue(&mut value,&format);}
    value
}
fn normalize_issue(value: &mut Value, inherited: &Value) {
    // The full v2 view omits empty description fields, even when explicitly requested.
    if let Some(fields)=value.get_mut("fields").and_then(Value::as_object_mut) {
        fields.entry("description").or_insert(Value::Null);
    }
    if value["fields"]["description"].is_null() {
        value["appliedContentFormat"]=json!("markdown");
    } else if value.get("appliedContentFormat").is_none() {
        value["appliedContentFormat"]=inherited.clone();
    }
}
fn description_format(value: &Value) -> Option<&str> {
    value["fields"].get("appliedContentFormat").or_else(||value.get("appliedContentFormat")).and_then(Value::as_str)
}
pub fn extract_description(value: &Value, format: &str) -> Result<crate::adf::Extracted> {
    if value.is_null() || value.is_object() {return crate::adf::extract(value);}
    let body=value.as_str().context("Unexpected description type")?;
    ensure!(body.len()<=2*1024*1024,"Description exceeds 2 MiB");
    let text=match format {
        "markdown" => body.to_owned(),
        "html" => html2text::from_read(body.as_bytes(),120)?,
        _=>anyhow::bail!("Unsupported Jira description format: {format}")
    };
    Ok(crate::adf::Extracted{text,warnings:vec![]})
}
struct Item {
    id: String,
    key: String,
    title: String,
    updated: String,
    adf: Value,
    text: String,
    warnings: Vec<String>,
    hash: String,
    format: String,
    metadata: Metadata,
}
fn item(value: Value, project: &str) -> Result<Item> {
    ensure!(
        value.get("truncated").and_then(Value::as_bool) != Some(true),
        "Issue content was explicitly truncated"
    );
    let id = value["id"]
        .as_str()
        .context("Issue omitted stable ID")?
        .to_owned();
    let key = value["key"]
        .as_str()
        .context("Issue omitted key")?
        .to_owned();
    ensure!(
        id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty(),
        "Invalid stable issue ID"
    );
    ensure!(
        key.starts_with(&format!("{project}-"))
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "Returned issue is outside the configured project"
    );
    let fields = &value["fields"];
    let title = fields["summary"]
        .as_str()
        .context("Issue omitted summary")?
        .to_owned();
    let updated = normalized_timestamp(
        fields["updated"]
            .as_str()
            .context("Issue omitted updated timestamp")?,
    )?;
    let adf = fields
        .get("description")
        .context("Issue omitted description field")?
        .clone();
    let format=description_format(&value).unwrap_or(if adf.is_object(){"adf"}else{"markdown"}).to_owned();
    let extracted = extract_description(&adf, &format)?;
    let text = format!("# {key}: {title}\n\n{}", extracted.text);
    let hash = blake3::hash(
        format!(
            "jira-content-v2\n{format}\n{key}\n{title}\n{}",
            serde_json::to_string(&adf)?
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string();
    Ok(Item {
        id,
        key,
        title,
        updated,
        adf,
        text,
        warnings: extracted.warnings,
        hash,
        format,
        metadata: Metadata::from_fields(fields),
    })
}
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Complete,
    More,
    Cancelled,
}
/// Network and conversion happen before each short, atomic page publication.
/// Failed pages retain the previous continuation token. Duplicate retries are idempotent.
pub fn sync(
    db: &mut Connection,
    source_id: &str,
    settings: &Settings,
    provider: &mut impl Provider,
    cancel: impl Fn() -> bool,
) -> Result<Outcome> {
    settings.validate()?;
    let hash = blake3::hash(format!("metadata-v1:{}",serde_json::to_string(settings)?).as_bytes())
        .to_hex()
        .to_string();
    db.execute("INSERT INTO remote_sync(source_id,scope_hash) VALUES(?1,?2) ON CONFLICT(source_id) DO NOTHING",params![source_id,hash])?;
    let previous: String = db.query_row(
        "SELECT scope_hash FROM remote_sync WHERE source_id=?1",
        [source_id],
        |r| r.get(0),
    )?;
    if previous != hash {
        db.execute("UPDATE remote_sync SET scope_hash=?2,checkpoint=NULL,run_id=NULL,run_jql=NULL,next_token=NULL,candidate=NULL,last_success=NULL,error='',pages=0,items=0 WHERE source_id=?1",params![source_id,hash])?;
    }
    let checkpoint: Option<String> = db.query_row(
        "SELECT checkpoint FROM remote_sync WHERE source_id=?1",
        [source_id],
        |r| r.get(0),
    )?;
    let pending: Option<String> = db.query_row(
        "SELECT run_id FROM remote_sync WHERE source_id=?1",
        [source_id],
        |r| r.get(0),
    )?;
    if pending.is_none() {
        db.execute("UPDATE remote_sync SET run_id=?2,run_jql=?3,next_token=NULL,candidate=checkpoint,pages=0,items=0,last_attempt=?2 WHERE source_id=?1",params![source_id,now(),scope_jql(settings,checkpoint.as_deref())?])?;
    }
    let (run, jql, mut token, mut candidate): (String, String, Option<String>, Option<String>) = db
        .query_row(
            "SELECT run_id,run_jql,next_token,candidate FROM remote_sync WHERE source_id=?1",
            [source_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    let mut seen = std::collections::HashSet::new();
    if let Some(token) = &token {
        seen.insert(token.clone());
    }
    for _ in 0..50 {
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let page = provider.page(settings, &jql, token.as_deref())?;
        ensure!(
            page.get("truncated").and_then(Value::as_bool) != Some(true),
            "Jira page was explicitly truncated"
        );
        let issues = page["issues"]
            .as_array()
            .context("Jira response omitted issues array")?;
        let last = page["isLast"]
            .as_bool()
            .context("Jira response omitted isLast; refusing to advance checkpoint")?;
        let next = page["nextPageToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if !last {
            ensure!(next.is_some(), "Non-final page omitted nextPageToken");
            ensure!(
                seen.insert(next.clone().unwrap()),
                "Jira repeated a continuation token"
            );
        }
        ensure!(issues.len() <= 100, "Jira page exceeded item limit");
        let mut items = vec![];
        for issue in issues {
            if cancel() {
                return Ok(Outcome::Cancelled);
            }
            let updated = normalized_timestamp(
                issue["fields"]["updated"]
                    .as_str()
                    .context("Search omitted updated timestamp")?,
            )?;
            let key = issue["key"].as_str().context("Search omitted issue key")?;
            let id = issue["id"]
                .as_str()
                .context("Search omitted stable issue ID")?;
            // Even unchanged items are touched so freshness reflects this completed scope.
            let stored:Option<String>=db.query_row("SELECT remote_updated FROM remote_items WHERE source_id=?1 AND remote_id=?2 AND site=?3",params![source_id,id,settings.site],|r|r.get(0)).optional()?;
            let structured = issue["fields"]
                .get("description")
                .is_some_and(|v| v.is_object() || v.is_null() || matches!(description_format(issue),Some("markdown"|"html")));
            let value = if structured {
                issue.clone()
            } else if stored.as_deref() == Some(&updated) && description_format(issue)!=Some("mixed") {
                let raw:String=db.query_row("SELECT raw_content FROM remote_items WHERE source_id=?1 AND remote_id=?2 AND site=?3",params![source_id,id,settings.site],|r|r.get(0))?;
                let mut copy = issue.clone();
                copy["fields"]["description"] = serde_json::from_str(&raw)?;
                copy
            } else {
                let detail = match provider.issue(settings, key) {
                    Ok(detail) => detail,
                    Err(e) => {
                        record_item_error(
                            db,
                            source_id,
                            &settings.site,
                            id,
                            key,
                            &format!("{e:#}"),
                        )?;
                        return Err(e);
                    }
                };
                ensure!(
                    detail["id"] == issue["id"],
                    "Issue detail identity mismatch"
                );
                detail
            };
            let entry = match item(value, &settings.project) {
                Ok(entry) => entry,
                Err(e) => {
                    record_item_error(db, source_id, &settings.site, id, key, &format!("{e:#}"))?;
                    return Err(e);
                }
            };
            if candidate.as_ref().is_none_or(|old| entry.updated > *old) {
                candidate = Some(entry.updated.clone());
            }
            items.push(entry);
        }
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let tx = db.transaction()?;
        for entry in &items {
            publish(&tx, source_id, settings, &run, entry)?;
        }
        tx.execute("UPDATE remote_sync SET next_token=?2,candidate=?3,pages=pages+1,items=items+?4,error='',failures=0,retry_at=0 WHERE source_id=?1",params![source_id,next,candidate,items.len() as i64])?;
        if last {
            tx.execute("UPDATE remote_sync SET checkpoint=candidate,run_id=NULL,run_jql=NULL,next_token=NULL,last_success=?2 WHERE source_id=?1",params![source_id,now()])?;
        }
        tx.commit()?;
        if last {
            return Ok(Outcome::Complete);
        }
        token = next;
    }
    Ok(Outcome::More)
}
fn publish(db: &Connection, source: &str, s: &Settings, run: &str, item: &Item) -> Result<()> {
    let existing:Option<String>=db.query_row("SELECT remote_updated FROM remote_items WHERE source_id=?1 AND site=?2 AND remote_id=?3",params![source,s.site,item.id],|r|r.get(0)).optional()?;
    if existing
        .as_ref()
        .is_some_and(|timestamp| timestamp > &item.updated)
    {
        return Ok(());
    }
    let site_id = blake3::hash(s.site.as_bytes()).to_hex().to_string();
    let uri = format!("good-search://jira/{source}/{site_id}/{}", item.id);
    let root = format!("jira:{source}");
    let name = format!("{} · {}", item.key, item.title);
    db.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET name=excluded.name,folded=excluded.folded WHERE name!=excluded.name",params![root,uri,name,name.to_lowercase()])?;
    let file: i64 = db.query_row("SELECT id FROM files WHERE path=?1", [uri], |r| r.get(0))?;
    let old: Option<String> = db
        .query_row(
            "SELECT content_hash FROM remote_items WHERE file_id=?1",
            [file],
            |r| r.get(0),
        )
        .optional()?;
    let url = format!("{}/browse/{}", s.site.trim_end_matches('/'), item.key);
    db.execute("INSERT INTO remote_items(file_id,source_id,connection_id,site,remote_id,issue_key,title,original_url,remote_updated,synced_at,content_hash,raw_content,searchable_text,warnings,last_seen_run,scope_hash) VALUES(?1,?2,'default',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(file_id) DO UPDATE SET issue_key=excluded.issue_key,title=excluded.title,original_url=excluded.original_url,remote_updated=excluded.remote_updated,synced_at=excluded.synced_at,content_hash=excluded.content_hash,raw_content=excluded.raw_content,searchable_text=excluded.searchable_text,warnings=excluded.warnings,last_seen_run=excluded.last_seen_run,scope_hash=excluded.scope_hash",params![file,source,s.site,item.id,item.key,item.title,url,item.updated,now(),item.hash,serde_json::to_string(&item.adf)?,item.text,serde_json::to_string(&item.warnings)?,run,blake3::hash(format!("metadata-v1:{}",serde_json::to_string(s)?).as_bytes()).to_hex().to_string()])?;
    db.execute("UPDATE remote_items SET content_format=?2,metadata=?3 WHERE file_id=?1",params![file,item.format,serde_json::to_string(&item.metadata)?])?;
    db.execute(
        "DELETE FROM remote_failures WHERE source_id=?1 AND site=?2 AND remote_id=?3",
        params![source, s.site, item.id],
    )?;
    if old.as_deref() == Some(&item.hash) {
        return Ok(());
    }
    db.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(?1,'text',?2,1,1,'ready',?3) ON CONFLICT(file_id,kind) DO UPDATE SET source_hash=excluded.source_hash,detail=excluded.detail",params![file,item.hash,item.warnings.join("\n")])?;
    let doc: i64 = db.query_row(
        "SELECT id FROM documents WHERE file_id=?1 AND kind='text'",
        [file],
        |r| r.get(0),
    )?;
    db.execute("DELETE FROM chunks WHERE document_id=?1", [doc])?;
    for (i, chunk) in crate::content::chunks(&item.text).into_iter().enumerate() {
        db.execute("INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![doc,i as i64,chunk.heading,chunk.line as i64,chunk.body,blake3::hash(chunk.body.as_bytes()).to_hex().to_string()])?;
    }
    Ok(())
}
pub fn record_error(db: &Connection, source: &str, error: &str) -> Result<()> {
    db.execute("UPDATE remote_sync SET error=?2,run_id=CASE WHEN failures>=2 THEN NULL ELSE run_id END,next_token=CASE WHEN failures>=2 THEN NULL ELSE next_token END,failures=failures+1,retry_at=?3+min(3600,30*(1 << min(failures,7))),last_attempt=?4 WHERE source_id=?1",params![source,error,Utc::now().timestamp(),now()])?;
    Ok(())
}
pub fn open_target(db: &Connection, path: &str) -> Result<String> {
    if !path.starts_with("good-search://") {
        let parent:Option<String>=db.query_row("SELECT r.original_url FROM attachments a JOIN files f ON f.id=a.file_id JOIN remote_items r ON r.file_id=a.parent_file_id WHERE f.path=?1",[path],|r|r.get(0)).optional()?;
        return Ok(parent.unwrap_or_else(||path.into()));
    }
    let url: String = db.query_row(
        "SELECT original_url FROM remote_items r JOIN files f ON f.id=r.file_id WHERE f.path=?1",
        [path],
        |r| r.get(0),
    )?;
    ensure!(
        reqwest::Url::parse(&url)?.scheme() == "https",
        "Remote result must open an HTTPS URL"
    );
    Ok(url)
}

fn record_item_error(
    db: &Connection,
    source: &str,
    site: &str,
    id: &str,
    key: &str,
    error: &str,
) -> Result<()> {
    db.execute("INSERT INTO remote_failures VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(source_id,site,remote_id) DO UPDATE SET error=excluded.error,attempted_at=excluded.attempted_at",params![source,site,id,key,error,now()])?;
    Ok(())
}
