use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
pub mod adf;
pub mod bifrost;
mod content;
pub mod credentials;
pub mod file_icons;
pub mod images;
pub mod inspection;
pub mod jira;
pub mod preview;
mod query;
pub mod semantic;
pub mod sources;
mod storage;
pub use storage::{open_db, open_reader, open_writer};

pub const DEFAULT_IMAGE_MODEL: &str = "gpt-6.1-terra";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub sources: Vec<sources::Source>,
    pub semantic_enabled: bool,
    pub semantic_delay_ms: u64,
    pub folders: Vec<PathBuf>,
    pub exclusions: Vec<String>,
    pub reconcile_seconds: u64,
    pub content_folders: Vec<PathBuf>,
    pub max_content_bytes: u64,
    pub image_folders: Vec<PathBuf>,
    pub image_model: String,
    pub image_daily_requests: u32,
    pub images_paused: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            sources: vec![],
            semantic_enabled: true,
            semantic_delay_ms: 200,
            folders: vec![],
            exclusions: vec![
                ".git".into(),
                "node_modules".into(),
                "target".into(),
                "*.tmp".into(),
            ],
            reconcile_seconds: 30,
            content_folders: vec![],
            max_content_bytes: 2 * 1024 * 1024,
            image_folders: vec![],
            image_model: DEFAULT_IMAGE_MODEL.into(),
            image_daily_requests: 100,
            images_paused: false,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        for source in &self.sources {
            if source.id.is_empty()
                || !ids.insert(&source.id)
                || (source.is_folder()
                    && !paths.insert(source.path.to_string_lossy().to_lowercase()))
            {
                bail!("Each source must have a unique ID and folder path");
            }
            if source.name.trim().is_empty() {
                bail!("Give each source a name");
            }
            self.source_matcher(&source.path)?;
            if source.kind == sources::SourceKind::Notion {
                source.notion.as_ref().context("Notion source needs settings")?.validate()?;
            } else if source.kind == sources::SourceKind::Gmail {
                source.gmail.as_ref().context("Gmail source needs settings")?.validate()?;
            } else if source.kind == sources::SourceKind::Slack {
                source.slack.as_ref().context("Slack source needs settings")?.validate()?;
            } else if !source.is_folder() {
                source
                    .jira
                    .as_ref()
                    .context("Jira source needs settings")?
                    .validate()?;
            }
        }
        if !(50..=2000).contains(&self.semantic_delay_ms) {
            bail!("Semantic search delay must be between 50 and 2000 ms");
        }
        self.matcher()?;
        if self.image_model.trim().is_empty() || self.image_model.len() > 100 {
            bail!("Choose an image model");
        }
        for root in &self.image_folders {
            if !self.folders.contains(root) {
                bail!("Image folder must also be selected: {}", root.display());
            }
        }
        if self.max_content_bytes == 0 || self.max_content_bytes > 64 * 1024 * 1024 {
            bail!("Text size limit must be between 1 byte and 64 MiB");
        }
        for root in &self.content_folders {
            if !self.folders.contains(root) {
                bail!(
                    "Text indexing folder must also be a selected folder: {}",
                    root.display()
                );
            }
        }
        for root in &self.folders {
            if !root.is_absolute() {
                bail!("Folder must be absolute: {}", root.display());
            }
        }
        if self.reconcile_seconds < 5 {
            bail!("Reconciliation interval must be at least 5 seconds");
        }
        Ok(())
    }
    pub fn matcher(&self) -> Result<GlobSet> {
        let mut builder = GlobSetBuilder::new();
        for pattern in &self.exclusions {
            builder.add(
                GlobBuilder::new(&pattern.replace('\\', "/"))
                    .case_insensitive(true)
                    .literal_separator(true)
                    .build()?,
            );
        }
        Ok(builder.build()?)
    }
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub timestamp: Option<i64>,
    pub original_url: Option<String>,
    pub jira: Option<jira::Metadata>,
    pub gmail: Option<gmail::Metadata>,
    pub slack: bool,
    pub notion: bool,
    pub path: String,
    pub name: String,
    pub snippet: Option<String>,
}
impl Hit {
    pub fn open_label(&self) -> &str { if self.notion { "Open in Notion" } else if self.path.starts_with("good-search://gmail/") { "Open in Gmail" } else if self.slack { "Open in Slack" } else if self.original_url.is_some() { "Open in Jira" } else { "Open file" } }
    pub fn location(&self) -> &str {
        self.original_url.as_deref().unwrap_or(&self.path)
    }
}
pub(crate) fn remote_locations(db: &Connection, hits: &mut [Hit]) -> Result<()> {
    for hit in hits.iter_mut() {
        hit.timestamp = db.query_row("SELECT d.timestamp FROM result_dates d JOIN files f ON f.id=d.file_id WHERE f.path=?1",[&hit.path],|r|r.get(0))?;
    }
    for hit in hits
        .iter_mut()
        .filter(|h| h.path.starts_with("good-search://"))
    {
        hit.original_url=Some(db.query_row("SELECT original_url FROM remote_items r JOIN files f ON f.id=r.file_id WHERE f.path=?1",[&hit.path],|r|r.get(0))?);
    }
    for hit in hits.iter_mut().filter(|h| h.original_url.is_none()) {
        hit.original_url=db.query_row("SELECT r.original_url FROM attachments a JOIN remote_items r ON r.file_id=a.parent_file_id JOIN files f ON f.id=a.file_id WHERE f.path=?1",[&hit.path],|r|r.get(0)).optional()?;
    }
    for hit in hits.iter_mut().filter(|h| h.original_url.is_some()) {
        hit.notion = db.query_row("SELECT r.content_format='notion' FROM files f LEFT JOIN attachments a ON a.file_id=f.id JOIN remote_items r ON r.file_id=coalesce(a.parent_file_id,f.id) WHERE f.path=?1",[&hit.path],|r|r.get(0))?;
        hit.slack = db.query_row("SELECT r.content_format='slack' FROM files f LEFT JOIN attachments a ON a.file_id=f.id JOIN remote_items r ON r.file_id=coalesce(a.parent_file_id,f.id) WHERE f.path=?1",[&hit.path],|r|r.get(0))?;
        if hit.path.starts_with("good-search://gmail/") { hit.gmail = gmail::metadata(db, &hit.path)?; }
        if !hit.slack && !hit.notion && !hit.path.starts_with("good-search://gmail/") { hit.jira = jira::metadata(db, &hit.path)?; }
    }
    Ok(())
}

pub mod filters;
pub mod sorting;
pub mod attachments;
pub mod notion;
pub mod gmail;
pub mod slack;
pub mod slack_display;
pub use filters::SearchFilters;
pub fn search(db: &Connection, query: &str, limit: usize) -> Result<Vec<Hit>> {
    search_filtered(db, query, limit, SearchFilters::default())
}
pub fn search_filtered(
    db: &Connection,
    query: &str,
    limit: usize,
    options: SearchFilters,
) -> Result<Vec<Hit>> {
    if !options.text_matches || (!options.text && !options.images) {
        return Ok(vec![]);
    }
    let terms = query::terms(query);
    if terms.is_empty() || limit == 0 {
        return Ok(vec![]);
    }
    let q = terms.join(" ");
    let long: Vec<String> = terms
        .iter()
        .filter(|s| s.chars().count() >= 3)
        .cloned()
        .collect();
    let mut values: Vec<rusqlite::types::Value> = vec![];
    let mut filters = vec![];
    let from = if long.is_empty() {
        "files f"
    } else {
        values.push(query::fts(&long).into());
        filters.push("names MATCH ?1".to_owned());
        "names JOIN files f ON f.id=names.rowid"
    };
    if terms.len() == 1 && long.is_empty() {
        let escaped = terms[0]
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        values.push(format!("{escaped}%").into());
        filters.push(format!("f.folded LIKE ?{} ESCAPE '\\'", values.len()));
    } else {
        for term in &terms {
            values.push(term.clone().into());
            filters.push(format!("instr(f.folded,?{})>0", values.len()));
        }
    }
    let roots = serde_json::to_string(&options.roots)?;
    values.push(roots.into());
    let root_param = values.len();
    filters.push(format!(
        "(?{root_param}='null' OR f.root IN (SELECT value FROM json_each(?{root_param})))"
    ));
    let image = "(f.path NOT LIKE 'good-search://%' AND (EXISTS(SELECT 1 FROM attachments a WHERE a.file_id=f.id) OR EXISTS(SELECT 1 FROM documents d WHERE d.file_id=f.id AND d.kind IN ('image_text','image_description')) OR lower(f.name) GLOB '*.png' OR lower(f.name) GLOB '*.jpg' OR lower(f.name) GLOB '*.jpeg' OR lower(f.name) GLOB '*.webp' OR lower(f.name) GLOB '*.gif' OR lower(f.name) GLOB '*.bmp' OR lower(f.name) GLOB '*.tif' OR lower(f.name) GLOB '*.tiff' OR lower(f.name) GLOB '*.svg' OR lower(f.name) GLOB '*.avif'))";
    if !options.images {
        filters.push(format!("NOT {image}"));
    }
    if !options.text {
        filters.push(image.into());
    }
    values.push(q.into());
    let exact = values.len();
    values.push((limit.min(200) as i64).into());
    let order = if options.sort.by_date() { options.sort.sql_order().to_owned() } else if terms.len() == 1 && long.is_empty() {
        "folded COLLATE NOCASE".to_owned()
    } else {
        format!("(folded=?{exact}) DESC,length(name),path")
    };
    let date_order = format!(" ORDER BY {}", options.sort.sql_order());
    let candidate_order = if options.sort.by_date() { date_order.as_str() } else if terms.len() == 1 && long.is_empty() {
        " ORDER BY f.folded COLLATE NOCASE"
    } else {
        ""
    };
    let sql = format!("WITH candidates AS MATERIALIZED (SELECT f.path,f.name,f.folded,(SELECT timestamp FROM result_dates WHERE file_id=f.id) AS sort_date FROM {from} WHERE {}{candidate_order} LIMIT 1000) SELECT path,name FROM candidates ORDER BY {order} LIMIT ?{}",filters.join(" AND "),values.len());
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values), |r| {
        Ok(Hit {
            original_url: None,
            jira: None, gmail: None, timestamp: None, notion: false, slack: false,
            path: r.get(0)?,
            name: r.get(1)?,
            snippet: None,
        })
    })?;
    let mut hits: Vec<Hit> = rows.collect::<rusqlite::Result<_>>()?;
    let sort=options.sort;
    content::search_content(db, &terms, limit.min(200), &mut hits, options)?;
    remote_locations(db, &mut hits)?;
    if sort.by_date() { sorting::sort(&mut hits, query, sort, chrono::Utc::now().timestamp()); }
    hits.truncate(limit.min(200));
    Ok(hits)
}

/// Each root is published atomically. Failed or disconnected roots retain their last snapshot.
/// Symlinks/junctions are never followed, so selected roots bound the traversal.
pub fn reconcile(db: &mut Connection, config: &Config) -> Result<Vec<String>> {
    config.validate()?;
    let database: String = db.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |r| r.get(0),
    )?;
    let data_dir = if database.is_empty() {
        None
    } else {
        Path::new(&database).parent().map(Path::to_path_buf)
    };
    let mut errors = vec![];
    // Remove roots that the user no longer authorizes, even if another root is offline.
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS roots(path TEXT PRIMARY KEY); DELETE FROM roots;",
    )?;
    for root in &config.folders {
        tx.execute(
            "INSERT OR IGNORE INTO roots VALUES(?1)",
            [root.to_string_lossy().as_ref()],
        )?;
    }
    tx.execute(
        "DELETE FROM files WHERE root NOT IN (SELECT path FROM roots) AND id NOT IN (SELECT file_id FROM remote_items) AND id NOT IN (SELECT file_id FROM attachments)",
        [],
    )?;
    tx.commit()?;
    for root in &config.folders {
        let result = (|| -> Result<()> {
            if config.source_paused(root) {
                return Ok(());
            }
            let matcher = config.source_matcher(root)?;
            if !root.is_dir() {
                bail!("Folder unavailable: {}", root.display());
            }
            let mut snapshot = Vec::new();
            for entry in WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    if data_dir.as_deref() == Some(e.path()) {
                        return false;
                    }
                    if e.depth() == 0 {
                        return true;
                    }
                    let rel = e
                        .path()
                        .strip_prefix(root)
                        .unwrap_or(e.path())
                        .to_string_lossy()
                        .replace('\\', "/");
                    !matcher.is_match(&rel) && !matcher.is_match(e.file_name())
                })
            {
                let entry = entry.with_context(|| format!("Scanning {}", root.display()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path().to_str().context("Path is not valid Unicode")?;
                let name = entry
                    .file_name()
                    .to_str()
                    .context("Filename is not valid Unicode")?;
                let modified=entry.metadata().ok().and_then(|m|m.modified().ok()).and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|d.as_secs() as i64);
                snapshot.push((path.to_owned(), name.to_owned(), modified));
            }
            let tx = db.transaction()?;
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS seen(path TEXT PRIMARY KEY); DELETE FROM seen;",
            )?;
            for (path, name, modified) in &snapshot {
                tx.prepare_cached("INSERT INTO seen VALUES(?1)")?
                    .execute([path])?;
                tx.prepare_cached("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET root=excluded.root,name=excluded.name,folded=excluded.folded WHERE files.root != excluded.root OR files.name != excluded.name")?.execute(params![root.to_string_lossy(),path,name,name.to_lowercase()])?;
                tx.execute("INSERT INTO file_times(file_id,modified) SELECT id,?2 FROM files WHERE path=?1 ON CONFLICT(file_id) DO UPDATE SET modified=excluded.modified WHERE modified IS NOT excluded.modified",params![path,modified])?;
            }
            tx.execute(
                "DELETE FROM files WHERE root=?1 AND path NOT IN (SELECT path FROM seen)",
                [root.to_string_lossy().as_ref()],
            )?;
            tx.commit()?;
            Ok(())
        })();
        if let Err(e) = result {
            errors.push(format!("{e:#}"));
        }
    }
    errors.extend(content::index(db, config)?);
    images::discover(db, config)?;
    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusions_change_and_deleted_files_are_removed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("private"))?;
        std::fs::write(dir.path().join("private/note.md"), "")?;
        std::fs::write(dir.path().join("public.md"), "")?;
        let mut db = open_db(Path::new(":memory:"))?;
        let mut config = Config {
            folders: vec![dir.path().into()],
            ..Default::default()
        };
        reconcile(&mut db, &config)?;
        assert_eq!(search(&db, "note", 10)?.len(), 1);
        config.exclusions.push("private/**".into());
        std::fs::remove_file(dir.path().join("public.md"))?;
        reconcile(&mut db, &config)?;
        assert!(search(&db, "note", 10)?.is_empty());
        assert!(search(&db, "public", 10)?.is_empty());
        Ok(())
    }
    #[test]
    fn database_directory_is_never_indexed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let data = dir.path().join("data");
        std::fs::create_dir(&data)?;
        std::fs::write(data.join("private-config.toml"), "")?;
        std::fs::write(dir.path().join("public.md"), "")?;
        let mut db = open_db(&data.join("index.sqlite"))?;
        reconcile(
            &mut db,
            &Config {
                folders: vec![dir.path().into()],
                ..Default::default()
            },
        )?;
        assert!(search(&db, "private", 10)?.is_empty());
        assert!(search(&db, "sqlite", 10)?.is_empty());
        assert_eq!(search(&db, "public", 10)?.len(), 1);
        Ok(())
    }
    #[test]
    fn config_rejects_relative_paths_and_invalid_globs() {
        assert!(Config {
            folders: vec!["relative".into()],
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Config {
            exclusions: vec!["[".into()],
            ..Default::default()
        }
        .validate()
        .is_err());
        #[cfg(windows)]
        assert!(Config {
            folders: vec![r"\\wsl.localhost\Ubuntu\home\user".into()],
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
    #[test]
    fn indexing_search_exclusions_and_reconciliation() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("node_modules"))?;
        std::fs::write(dir.path().join("node_modules/secret.txt"), "")?;
        for name in [
            "Report.MD",
            "another-report.txt",
            "résumé.md",
            "100%_done.txt",
            "skip.tmp",
        ] {
            std::fs::write(dir.path().join(name), "")?;
        }
        let mut db = open_db(Path::new(":memory:"))?;
        let mut config = Config {
            folders: vec![dir.path().into()],
            ..Default::default()
        };
        assert!(reconcile(&mut db, &config)?.is_empty());
        assert_eq!(search(&db, "REPORT", 20)?.len(), 2);
        assert_eq!(search(&db, "ré", 20)?.len(), 1);
        assert_eq!(search(&db, "%_d", 20)?.len(), 1);
        assert!(search(&db, "secret", 20)?.is_empty());
        assert!(search(&db, "skip", 20)?.is_empty());
        assert!(search(&db, "\" OR *", 20)?.is_empty());
        std::fs::rename(dir.path().join("Report.MD"), dir.path().join("renamed.md"))?;
        reconcile(&mut db, &config)?;
        assert_eq!(search(&db, "report", 20)?.len(), 1);
        assert_eq!(search(&db, "renamed", 20)?.len(), 1);
        config.folders.clear();
        reconcile(&mut db, &config)?;
        assert!(search(&db, "report", 20)?.is_empty());
        Ok(())
    }
    #[test]
    fn offline_root_retains_snapshot() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("root");
        std::fs::create_dir(&root)?;
        std::fs::write(root.join("hello.txt"), "")?;
        let mut db = open_db(Path::new(":memory:"))?;
        let config = Config {
            folders: vec![root.clone()],
            ..Default::default()
        };
        reconcile(&mut db, &config)?;
        std::fs::rename(&root, dir.path().join("offline"))?;
        assert_eq!(reconcile(&mut db, &config)?.len(), 1);
        assert_eq!(search(&db, "hello", 10)?.len(), 1);
        Ok(())
    }
}
