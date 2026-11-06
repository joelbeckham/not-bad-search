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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Folder,
}
impl Source {
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
        self.folders = self.sources.iter().map(|s| s.path.clone()).collect();
        self.content_folders = self
            .sources
            .iter()
            .filter(|s| s.text)
            .map(|s| s.path.clone())
            .collect();
        self.image_folders = self
            .sources
            .iter()
            .filter(|s| s.images)
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
                .map(|s| s.path.to_string_lossy())
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
        let mut status = Status {
            id: source.id.clone(),
            available: source.path.is_dir(),
            ..Default::default()
        };
        let mut stmt = db.prepare("SELECT f.name, (SELECT status FROM documents WHERE file_id=f.id AND kind='text'), (SELECT state FROM image_jobs WHERE file_id=f.id), EXISTS(SELECT 1 FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.file_id=f.id AND d.status='ready'), EXISTS(SELECT 1 FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.file_id=f.id AND d.status='ready' AND NOT EXISTS(SELECT 1 FROM embeddings e WHERE e.chunk_id=c.id AND e.model=?2 AND e.content_hash=c.content_hash)) FROM files f WHERE f.root=?1")?;
        let rows = stmt.query_map(
            rusqlite::params![source.path.to_string_lossy(), crate::semantic::MODEL],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, bool>(4)?,
                ))
            },
        )?;
        for row in rows {
            let (name, text, image, has_chunks, missing_embeddings) = row?;
            status.items += 1;
            let ext = Path::new(&name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let needs_text = source.text && matches!(ext.as_str(), "md" | "markdown" | "txt");
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
