//! Source configuration is independent of connector implementation and UI.
use crate::Config;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub path: PathBuf,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub text: bool,
    #[serde(default)]
    pub images: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub jira: Option<crate::jira::Settings>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Folder,
    Jira,
}
impl Source {
    pub fn is_folder(&self) -> bool {
        self.kind == SourceKind::Folder
    }
    pub fn jira_source() -> Self {
        let mut s = Self::folder(PathBuf::new());
        s.id = blake3::hash(format!("{:?}", std::time::SystemTime::now()).as_bytes())
            .to_hex()
            .to_string();
        s.name = "CORE · Jira".into();
        s.kind = SourceKind::Jira;
        s.jira = Some(Default::default());
        s.text = true;
        s.paused = true;
        s
    }
    pub fn folder(path: PathBuf) -> Self {
        let id = blake3::hash(path.to_string_lossy().to_lowercase().as_bytes())
            .to_hex()
            .to_string();
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        Self {
            id,
            name,
            kind: SourceKind::Folder,
            path,
            exclusions: vec![],
            text: false,
            images: false,
            paused: false,
            jira: None,
        }
    }
}
impl Config {
    /// One-time adoption of legacy roots; IDs persist when a source is renamed or moved.
    pub fn migrate_sources(&mut self) {
        if self.sources.is_empty() {
            self.sources = self
                .folders
                .iter()
                .map(|path| {
                    let mut source = Source::folder(path.clone());
                    source.exclusions = self.exclusions.clone();
                    source.text = self.content_folders.contains(path);
                    source.images = self.image_folders.contains(path);
                    source
                })
                .collect();
            if !self.sources.is_empty() {
                self.exclusions.clear();
            }
        }
    }
    pub fn sync_sources(&mut self) {
        self.folders = self
            .sources
            .iter()
            .filter(|s| s.is_folder())
            .map(|s| s.path.clone())
            .collect();
        self.content_folders = self
            .sources
            .iter()
            .filter(|s| s.is_folder() && s.text)
            .map(|s| s.path.clone())
            .collect();
        self.image_folders = self
            .sources
            .iter()
            .filter(|s| s.is_folder() && s.images)
            .map(|s| s.path.clone())
            .collect();
    }
    pub fn source_paused(&self, path: &Path) -> bool {
        self.sources.iter().any(|s| s.path == path && s.paused)
    }
    pub fn paused_roots_json(&self) -> String {
        serde_json::to_string(
            &self
                .sources
                .iter()
                .filter(|s| s.paused)
                .map(|s| {
                    if s.is_folder() {
                        s.path.to_string_lossy().into_owned()
                    } else {
                        format!("jira:{}", s.id)
                    }
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    pub fn source_matcher(&self, path: &Path) -> Result<globset::GlobSet> {
        let mut config = self.clone();
        if let Some(source) = self.sources.iter().find(|s| s.path == path) {
            config.exclusions.extend(source.exclusions.clone());
        }
        config.matcher()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub id: String,
    pub available: bool,
    pub remote_status: Option<String>,
    pub items: usize,
    pub ready: usize,
    pub queued: usize,
    pub processing: usize,
    pub attention: usize,
    pub skipped: usize,
    pub text_ready: usize,
    pub images_ready: usize,
    pub embedding_ready: usize,
    pub embedding_queued: usize,
}
/// Counts files, not chunks. Filename search is available for every discovered item.
pub fn snapshot(db: &rusqlite::Connection, config: &Config) -> Result<Vec<Status>> {
    let tx = db.unchecked_transaction()?;
    let db = &tx;
    let mut result = vec![];
    for source in &config.sources {
        let remote = !source.is_folder();
        let root = if remote {
            format!("jira:{}", source.id)
        } else {
            source.path.to_string_lossy().into_owned()
        };
        let mut status = Status {
            id: source.id.clone(),
            available: remote || source.path.is_dir(),
            ..Default::default()
        };
        if remote {
            use rusqlite::OptionalExtension;
            status.remote_status=Some(db.query_row("SELECT coalesce(last_success,'Never'),error,run_id IS NOT NULL,pages,items FROM remote_sync WHERE source_id=?1",[&source.id],|r| {
                let error:String=r.get(1)?;
                Ok(format!("Last successful sync: {} · {} pages / {} issues this run{}{}",r.get::<_,String>(0)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,if r.get::<_,bool>(2)? {" · import pending"}else{""},if error.is_empty(){String::new()}else{format!(" · {error}")}))
            }).optional()?.unwrap_or_else(||"Not synced yet".into()));
            let stale:i64=db.query_row("SELECT count(*) FROM remote_items r JOIN remote_sync s ON s.source_id=r.source_id WHERE r.source_id=?1 AND r.scope_hash!=s.scope_hash",[&source.id],|r|r.get(0))?;
            let unresolved: i64 = db.query_row(
                "SELECT count(*) FROM remote_failures WHERE source_id=?1",
                [&source.id],
                |r| r.get(0),
            )?;
            let warnings: i64 = db.query_row(
                "SELECT count(*) FROM remote_items WHERE source_id=?1 AND warnings!='[]'",
                [&source.id],
                |r| r.get(0),
            )?;
            if let Some(message) = &mut status.remote_status {
                message.push_str(&format!(" · {stale} not confirmed in current scope · {unresolved} unresolved · {warnings} with content warnings"));
            }
        }
        let mut stmt = db.prepare("SELECT f.name, (SELECT status FROM documents WHERE file_id=f.id AND kind='text'), (SELECT state FROM image_jobs WHERE file_id=f.id), EXISTS(SELECT 1 FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.file_id=f.id AND d.status='ready'), EXISTS(SELECT 1 FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.file_id=f.id AND d.status='ready' AND NOT EXISTS(SELECT 1 FROM embeddings e WHERE e.chunk_id=c.id AND e.model=?2 AND e.content_hash=c.content_hash)) FROM files f WHERE f.root=?1")?;
        let rows = stmt.query_map(rusqlite::params![root, crate::semantic::MODEL], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, bool>(4)?,
            ))
        })?;
        for row in rows {
            let (name, text, image, has_chunks, missing_embeddings) = row?;
            status.items += 1;
            let ext = Path::new(&name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let needs_text =
                source.text && (remote || matches!(ext.as_str(), "md" | "markdown" | "txt"));
            let needs_image =
                source.images && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp");
            if needs_text && text.as_deref() == Some("ready") {
                status.text_ready += 1;
            }
            if needs_image && image.as_deref() == Some("done") {
                status.images_ready += 1;
            }
            if has_chunks && !missing_embeddings {
                status.embedding_ready += 1;
            }
            if config.semantic_enabled && missing_embeddings {
                status.embedding_queued += 1;
            }
            if (needs_text && text.as_deref() == Some("error"))
                || (needs_image && image.as_deref() == Some("failed"))
            {
                status.attention += 1;
            } else if needs_image && image.as_deref() == Some("running") {
                status.processing += 1;
            } else if needs_image && image.as_deref() == Some("skipped") {
                status.skipped += 1;
            } else if (needs_text && text.as_deref() != Some("ready"))
                || (needs_image && image.as_deref() != Some("done"))
                || (config.semantic_enabled && missing_embeddings)
            {
                status.queued += 1;
            } else {
                status.ready += 1;
            }
        }
        result.push(status);
    }
    Ok(result)
}
