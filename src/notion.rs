//! Read-only Notion subtree ingestion. Durable queue preserves traversal across retries.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, VecDeque},
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pages: String,
    pub teamspaces: String,
    pub poll_seconds: u64,
    pub reload_generation: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            pages: String::new(),
            teamspaces: String::new(),
            poll_seconds: 3600,
            reload_generation: 0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.pages.trim().is_empty() || !self.teamspaces.trim().is_empty(),
            "Enter a Notion page URL or teamspace name/ID"
        );
        for page in self.pages.lines().map(str::trim).filter(|s| !s.is_empty()) {
            page_id(page)?;
        }
        ensure!(
            (30..=86400).contains(&self.poll_seconds),
            "Poll interval must be 30–86400 seconds"
        );
        Ok(())
    }
}
pub trait Provider {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value>;
    fn download(&mut self, url: &str) -> Result<Vec<u8>>;
}
impl Provider for crate::bifrost::Mcp {
    fn read(&mut self, tool: &str, args: Value) -> Result<Value> {
        self.notion_read(tool, args)
    }
    fn download(&mut self, url: &str) -> Result<Vec<u8>> {
        download_image(url)
    }
}
pub fn page_id(input: &str) -> Result<String> {
    let input = input.trim().trim_start_matches("{{").trim_end_matches("}}");
    let value = if input.contains("://") {
        let u = reqwest::Url::parse(input)?;
        ensure!(
            u.scheme() == "https"
                && u.host_str().is_some_and(|h| h == "notion.so"
                    || h.ends_with(".notion.so")
                    || h == "app.notion.com"
                    || h.ends_with(".notion.site")),
            "Enter a Notion page URL or ID"
        );
        u.path_segments()
            .and_then(|mut p| p.next_back())
            .unwrap_or("")
            .to_owned()
    } else {
        input.into()
    };
    let compact = value.replace('-', "");
    ensure!(compact.len() >= 32, "Notion page URL is missing its ID");
    let id = compact
        .get(compact.len() - 32..)
        .context("Invalid Notion page ID")?;
    ensure!(
        id.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid Notion page ID"
    );
    Ok(id.to_lowercase())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Task {
    Page(String),
    Database {
        url: String,
        after: String,
    },
    Search {
        field: String,
        scope: String,
        start: String,
        end: String,
    },
}
#[derive(Default, Serialize, Deserialize)]
struct Queue {
    pending: VecDeque<Task>,
    seen: BTreeSet<String>,
}
fn search_task(field: &str, scope: &str) -> Task {
    Task::Search {
        field: field.into(),
        scope: scope.into(),
        start: "0001-01-01".into(),
        end: (chrono::Utc::now() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
    }
}
fn add_page(q: &mut Queue, page: &str) -> Result<()> {
    let id = page_id(page)?;
    if q.seen.insert(id.clone()) {
        q.pending.push_back(Task::Page(id));
    }
    ensure!(
        q.seen.len() <= 100000,
        "Notion source exceeds 100,000 pages; narrow its scope"
    );
    Ok(())
}
pub fn sync(
    db: &mut Connection,
    source: &str,
    s: &Settings,
    p: &mut impl Provider,
    cache: &Path,
    images: bool,
    cancel: impl Fn() -> bool,
) -> Result<crate::jira::Outcome> {
    use crate::jira::{now, Outcome};
    s.validate()?;
    let scope = blake3::hash(
        serde_json::to_string(&json!([
            "notion-v3-page-properties",
            s.pages,
            s.teamspaces,
            s.reload_generation,
            images
        ]))?
        .as_bytes(),
    )
    .to_hex()
    .to_string();
    db.execute(
        "INSERT INTO remote_sync(source_id,scope_hash) VALUES(?1,?2) ON CONFLICT DO NOTHING",
        params![source, scope],
    )?;
    db.execute("UPDATE remote_sync SET scope_hash=?2,run_id=NULL,next_token=NULL,last_success=NULL WHERE source_id=?1 AND scope_hash!=?2",params![source,scope])?;
    let (active, stored): (Option<String>, Option<String>) = db.query_row(
        "SELECT run_id,next_token FROM remote_sync WHERE source_id=?1",
        [source],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let run = active.clone().unwrap_or_else(now);
    let mut q = if active.is_some() {
        serde_json::from_str::<Queue>(
            stored
                .as_deref()
                .context("Notion traversal checkpoint missing")?,
        )?
    } else {
        let mut q = Queue::default();
        for page in s.pages.lines().map(str::trim).filter(|s| !s.is_empty()) {
            add_page(&mut q, page)?;
        }
        for team in s
            .teamspaces
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if cancel() {
                return Ok(Outcome::Cancelled);
            }
            let id = if let Ok(id) = page_id(team) {
                id
            } else {
                let teams = p.read("get-teams", json!({"query":team}))?;
                let matches: Vec<_> = ["joinedTeams", "otherTeams"]
                    .iter()
                    .flat_map(|key| teams[*key].as_array().into_iter().flatten())
                    .filter(|t| {
                        t["in_trash"] != true
                            && t["name"]
                                .as_str()
                                .is_some_and(|n| n.eq_ignore_ascii_case(team))
                    })
                    .collect();
                ensure!(
                    matches.len() == 1,
                    "Teamspace name is missing or ambiguous; enter its exact name or ID"
                );
                matches[0]["id"]
                    .as_str()
                    .context("Notion teamspace ID missing")?
                    .into()
            };
            q.pending.push_back(search_task("teamspace_id", &id));
        }
        db.execute(
            "UPDATE remote_sync SET run_id=?2,next_token=?3,pages=0,items=0 WHERE source_id=?1",
            params![source, run, serde_json::to_string(&q)?],
        )?;
        q
    };
    for _ in 0..20 {
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let Some(task) = q.pending.pop_front() else {
            break;
        };
        let mut page = None;
        let mut cached = vec![];
        match task {
            Task::Database { url, after } => {
                let next = database_page(p, &url, &after)?;
                for page in &next {
                    add_page(&mut q, page)?;
                }
                // SQL's has_more describes its result, not rows beyond our LIMIT.
                // Continue until an explicit empty batch, including short batches.
                if let Some(last) = next.last() {
                    q.pending.push_front(Task::Database {
                        url,
                        after: last.clone(),
                    });
                }
            }
            Task::Search {
                field,
                scope: filter,
                start,
                end,
            } => {
                let mut args = json!({"query":"","page_size":50,"max_highlight_length":0,"filters":{"created_date_range":{"start_date":start,"end_date":end}}});
                args[&field] = json!(filter);
                let response = p.read("search", args)?;
                let results = response["results"]
                    .as_array()
                    .context("Notion omitted search results")?;
                if results.len() >= 50 {
                    let lo = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d")?;
                    let hi = chrono::NaiveDate::parse_from_str(&end, "%Y-%m-%d")?;
                    ensure!((hi-lo).num_days()>1,"Notion search caps at 50 results and exposes no pagination. A single creation day still reaches that limit; this source is incomplete. Use narrower page roots or a connector with paginated discovery.");
                    let mid = lo + chrono::Duration::days((hi - lo).num_days() / 2);
                    // Overlap the boundary day; IDs are deduplicated, avoiding date inclusivity gaps.
                    q.pending.push_front(Task::Search {
                        field: field.clone(),
                        scope: filter.clone(),
                        start: mid.format("%Y-%m-%d").to_string(),
                        end,
                    });
                    q.pending.push_front(Task::Search {
                        field,
                        scope: filter,
                        start,
                        end: mid.format("%Y-%m-%d").to_string(),
                    });
                } else {
                    for result in results {
                        add_page(
                            &mut q,
                            result["id"]
                                .as_str()
                                .context("Notion search result missing ID")?,
                        )?;
                    }
                }
            }
            Task::Page(id) => {
                let mut fetched = p.read("fetch", json!({"id":id,"include_file_urls":images}))?;
                let parsed = parse_page(&fetched)?;
                for child in &parsed.children {
                    add_page(&mut q, child)?;
                }
                for data in &parsed.databases {
                    let key = format!("data:{data}");
                    if q.seen.insert(key) {
                        q.pending.push_back(Task::Database {
                            url: data.clone(),
                            after: String::new(),
                        });
                    }
                }
                if images {
                    for reference in &parsed.images {
                        if cancel() {
                            return Ok(Outcome::Cancelled);
                        }
                        let url = fetched["references"][reference]
                            .as_str()
                            .unwrap_or(reference);
                        let bytes = p.download(url).context("Notion image download failed")?;
                        cached.push(cache_image(cache, source, &id, reference, &bytes)?);
                    }
                }
                if let Some(o) = fetched.as_object_mut() {
                    o.remove("references");
                } // Never persist expiring signed credentials.
                page = Some((id, parsed, fetched));
            }
        }
        if cancel() {
            return Ok(Outcome::Cancelled);
        }
        let tx = db.transaction()?;
        if let Some((id, page, raw)) = &page {
            publish(&tx, source, &run, &scope, id, page, raw, &cached, images)?;
        }
        tx.execute("UPDATE remote_sync SET next_token=?2,pages=pages+1,items=items+?3,last_attempt=?4,error='',failures=0,retry_at=0 WHERE source_id=?1",params![source,serde_json::to_string(&q)?,i64::from(page.is_some()),now()])?;
        if q.pending.is_empty() {
            tx.execute("UPDATE remote_sync SET run_id=NULL,next_token=NULL,last_success=?2 WHERE source_id=?1",params![source,now()])?;
        }
        tx.commit()?;
        if q.pending.is_empty() {
            return Ok(Outcome::Complete);
        }
    }
    Ok(Outcome::More)
}
#[derive(Debug)]
pub struct Page {
    pub title: String,
    pub url: String,
    pub updated: String,
    pub text: String,
    pub children: Vec<String>,
    pub databases: Vec<String>,
    pub images: Vec<String>,
}
fn attr(tag: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=\"");
    let value = tag.split_once(&marker)?.1.split('"').next()?;
    Some(
        value
            .trim_start_matches("{{")
            .trim_end_matches("}}")
            .replace("&amp;", "&"),
    )
}
fn tags(text: &str, name: &str, key: &str) -> Vec<String> {
    let marker = format!("<{name} ");
    text.split(&marker)
        .skip(1)
        .filter_map(|s| attr(s.split('>').next().unwrap_or(""), key))
        .collect()
}
pub fn parse_page(v: &Value) -> Result<Page> {
    let raw = v["text"].as_str().context("Notion omitted page content")?;
    ensure!(
        !raw.contains("<truncated") && v["truncated"] != true,
        "Notion returned truncated content"
    );
    let kind = v["metadata"]["type"].as_str().unwrap_or("");
    ensure!(
        matches!(kind, "page" | "database"),
        "Unsupported Notion object: {kind}"
    );
    let body = if kind == "page" {
        if let Some((_, content)) = raw.split_once("<content>") {
            content
                .rsplit_once("</content>")
                .context("Notion page content incomplete")?
                .0
        } else if raw.contains("<blank-page>") && raw.contains("</blank-page>") {
            // Database rows can have populated properties and no body blocks.
            ""
        } else {
            anyhow::bail!("Notion page content boundary missing");
        }
    } else {
        raw
    };
    let mut properties = String::new();
    if kind == "page" {
        if let Some((_, tail)) = raw.split_once("<properties>") {
            let encoded = tail
                .split_once("</properties>")
                .context("Notion page properties incomplete")?
                .0;
            let values: Value =
                serde_json::from_str(encoded).context("Invalid Notion page properties")?;
            for (name, value) in values
                .as_object()
                .context("Notion page properties must be an object")?
            {
                if name == "url" || value.is_null() {
                    continue;
                }
                let text = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                if !text.trim().is_empty() {
                    properties.push_str(&format!("{}: {}\n", name.trim(), text));
                }
            }
        }
    }
    // Ignore literal examples inside fenced code while following structural child links.
    let mut structural = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in body.lines() {
        let t = line.trim_start();
        let c = t.chars().next().unwrap_or(' ');
        let n = t.chars().take_while(|x| *x == c).count();
        if (c == '`' || c == '~') && n >= 3 {
            if fence.is_none() {
                fence = Some((c, n));
            } else if fence.is_some_and(|(f, k)| c == f && n >= k) {
                fence = None;
            }
            continue;
        }
        if fence.is_none() {
            structural.push_str(line);
            structural.push('\n');
        }
    }
    let mut children = tags(&structural, "page", "url");
    children.extend(tags(&structural, "database", "url"));
    if kind == "database" {
        if let Some(url) = v["url"].as_str() {
            children.retain(|s| page_id(s).ok() != page_id(url).ok());
        }
    }
    ensure!(
        !structural.contains("<folder "),
        "Notion folder traversal is not exposed by this connector; source is incomplete"
    );
    let databases = tags(&structural, "data-source", "url");
    let mut images = tags(&structural, "image", "source");
    for tail in structural.split("![").skip(1) {
        if let Some((_, rest)) = tail.split_once("](") {
            if let Some((url, _)) = rest.split_once(')') {
                images.push(url.trim().trim_matches(['<', '>']).to_owned());
            }
        }
    }
    images.sort();
    images.dedup();
    let title = v["title"].as_str().unwrap_or("Untitled").to_owned();
    let text = format!("{title}\n\n{properties}\n{body}");
    let url = v["url"]
        .as_str()
        .context("Notion omitted page link")?
        .to_owned();
    page_id(&url)?;
    let updated = v["page_last_edited_at"].as_str().unwrap_or("").to_owned();
    Ok(Page {
        title,
        url,
        updated,
        text,
        children,
        databases,
        images,
    })
}
struct Cached {
    reference: String,
    path: PathBuf,
    hash: String,
    ext: String,
    size: usize,
}
fn cache_image(
    cache: &Path,
    source: &str,
    page: &str,
    reference: &str,
    bytes: &[u8],
) -> Result<Cached> {
    ensure!(
        bytes.len() <= 10 * 1024 * 1024,
        "Notion image exceeds 10 MiB"
    );
    let ext = match image::guess_format(bytes)? {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::WebP => "webp",
        _ => anyhow::bail!("Notion image must be PNG, JPEG or WebP"),
    };
    let hash = blake3::hash(bytes).to_hex().to_string();
    let identity = blake3::hash(format!("notion/{source}/{page}/{reference}").as_bytes())
        .to_hex()
        .to_string();
    std::fs::create_dir_all(cache)?;
    let path = cache.join(format!("{identity}-{hash}.{ext}"));
    std::fs::write(&path, bytes)?;
    crate::images::image_bytes(&path)?;
    image::load_from_memory(bytes)?
        .thumbnail(64, 64)
        .save(path.with_extension("thumb.png"))?;
    Ok(Cached {
        reference: reference.into(),
        path,
        hash,
        ext: ext.into(),
        size: bytes.len(),
    })
}
fn publish(
    db: &Connection,
    source: &str,
    run: &str,
    scope: &str,
    id: &str,
    p: &Page,
    raw: &Value,
    images: &[Cached],
    index_images: bool,
) -> Result<()> {
    let uri = format!("not-bad-search://notion/{source}/{id}");
    let root = format!("notion:{source}");
    let hash = blake3::hash(p.text.as_bytes()).to_hex().to_string();
    db.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET name=excluded.name,folded=excluded.folded",params![root,uri,p.title,p.title.to_lowercase()])?;
    let file: i64 = db.query_row("SELECT id FROM files WHERE path=?1", [uri], |r| r.get(0))?;
    let old: Option<String> = db
        .query_row(
            "SELECT content_hash FROM remote_items WHERE file_id=?1",
            [file],
            |r| r.get(0),
        )
        .optional()?;
    db.execute("INSERT INTO remote_items(file_id,source_id,connection_id,site,remote_id,issue_key,title,original_url,remote_updated,synced_at,content_hash,raw_content,searchable_text,warnings,last_seen_run,scope_hash,content_format) VALUES(?1,?2,'default','https://www.notion.so',?3,?3,?4,?5,?6,?7,?8,?9,?10,'[]',?11,?12,'notion') ON CONFLICT(file_id) DO UPDATE SET title=excluded.title,original_url=excluded.original_url,remote_updated=excluded.remote_updated,synced_at=excluded.synced_at,content_hash=excluded.content_hash,raw_content=excluded.raw_content,searchable_text=excluded.searchable_text,last_seen_run=excluded.last_seen_run,scope_hash=excluded.scope_hash",params![file,source,id,p.title,p.url,p.updated,crate::jira::now(),hash,serde_json::to_string(raw)?,p.text,run,scope])?;
    if old.as_deref() != Some(&hash) {
        db.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(?1,'text',?2,1,1,'ready','') ON CONFLICT(file_id,kind) DO UPDATE SET source_hash=excluded.source_hash,status='ready'",params![file,hash])?;
        let doc: i64 = db.query_row(
            "SELECT id FROM documents WHERE file_id=?1 AND kind='text'",
            [file],
            |r| r.get(0),
        )?;
        db.execute("DELETE FROM chunks WHERE document_id=?1", [doc])?;
        for (i, c) in crate::content::chunks(&p.text).into_iter().enumerate() {
            db.execute("INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![doc,i as i64,c.heading,c.line as i64,c.body,blake3::hash(c.body.as_bytes()).to_hex().to_string()])?;
        }
    }
    if index_images {
        let paths: Vec<_> = images
            .iter()
            .map(|i| i.path.to_string_lossy().into_owned())
            .collect();
        db.execute("DELETE FROM files WHERE id IN (SELECT a.file_id FROM attachments a JOIN files f ON f.id=a.file_id WHERE a.parent_file_id=?1 AND f.path NOT IN (SELECT value FROM json_each(?2)))",params![file,serde_json::to_string(&paths)?])?;
        for (n, i) in images.iter().enumerate() {
            let name = format!("{} · image {}.{}", p.title, n + 1, i.ext);
            db.execute("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET name=excluded.name,folded=excluded.folded",params![root,i.path.to_string_lossy(),name,name.to_lowercase()])?;
            let child: i64 = db.query_row(
                "SELECT id FROM files WHERE path=?1",
                [i.path.to_string_lossy()],
                |r| r.get(0),
            )?;
            db.execute(
                "INSERT OR REPLACE INTO attachments VALUES(?1,?2,'notion',?3,?4,?5,?6,?7,?8,?9)",
                params![
                    child,
                    file,
                    source,
                    i.reference,
                    name,
                    format!("image/{}", if i.ext == "jpg" { "jpeg" } else { &i.ext }),
                    i.size as i64,
                    i.hash,
                    crate::jira::now()
                ],
            )?;
        }
    }
    Ok(())
}
fn download_image(url: &str) -> Result<Vec<u8>> {
    use std::net::{IpAddr, ToSocketAddrs};
    let url = reqwest::Url::parse(url).context("Invalid Notion image URL")?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443),
        "Notion images require public HTTPS URLs"
    );
    let host = url.host_str().context("Image host missing")?;
    let addresses: Vec<_> = (host, 443)
        .to_socket_addrs()
        .context("Image host lookup failed")?
        .collect();
    ensure!(
        !addresses.is_empty()
            && addresses.iter().all(|a| match a.ip() {
                IpAddr::V4(v) =>
                    !v.is_private()
                        && !v.is_loopback()
                        && !v.is_link_local()
                        && !v.is_unspecified()
                        && !v.is_multicast()
                        && v.octets()[0] != 0
                        && v.octets()[0] < 224,
                IpAddr::V6(v) =>
                    !v.is_loopback()
                        && !v.is_unspecified()
                        && !v.is_multicast()
                        && (v.segments()[0] & 0xfe00) != 0xfc00
                        && (v.segments()[0] & 0xffc0) != 0xfe80
                        && v.to_ipv4_mapped().is_none(),
            }),
        "Image host must resolve to a public address"
    );
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .build()?;
    let response = client
        .get(url)
        .send()
        .map_err(|_| anyhow::anyhow!("Image download connection failed"))?;
    ensure!(
        response.status().is_success(),
        "Image download HTTP {}",
        response.status().as_u16()
    );
    let mut bytes = Vec::new();
    response
        .take(10 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .context("Image read failed")?;
    ensure!(
        bytes.len() <= 10 * 1024 * 1024,
        "Notion image exceeds 10 MiB"
    );
    Ok(bytes)
}

/// Keyset pagination avoids the search cap and offset drift. Only the URL column
/// is queried; fetch remains responsible for faithful Markdown and image references.
pub fn database_page(p: &mut impl Provider, url: &str, after: &str) -> Result<Vec<String>> {
    let id = url
        .strip_prefix("collection://")
        .context("Invalid Notion data source URL")?;
    ensure!(
        id.replace('-', "").len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'),
        "Invalid Notion data source ID"
    );
    let query = format!("SELECT url FROM [{url}] WHERE url > ? ORDER BY url LIMIT 100");
    let response = p.read(
        "query-data-sources",
        json!({"data":{"mode":"sql","data_source_urls":[url],"query":query,"params":[after]}}),
    )?;
    let rows = response["results"]
        .as_array()
        .context("Notion omitted database query rows")?;
    ensure!(
        !rows.is_empty() || response["has_more"] != true,
        "Notion returned an empty incomplete query batch"
    );
    let mut result = Vec::new();
    let mut previous = after;
    for row in rows {
        let page = row["url"]
            .as_str()
            .context("Notion database row omitted its URL")?;
        page_id(page)?;
        ensure!(
            page > previous,
            "Notion database pagination did not advance in URL order"
        );
        result.push(page.to_owned());
        previous = page;
    }
    Ok(result)
}
