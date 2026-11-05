use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
mod content;
mod storage;
pub use storage::{open_db, open_reader, open_writer};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub folders: Vec<PathBuf>,
    pub exclusions: Vec<String>,
    pub reconcile_seconds: u64,
    pub content_folders: Vec<PathBuf>,
    pub max_content_bytes: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
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
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        self.matcher()?;
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
    pub path: String,
    pub name: String,
    pub snippet: Option<String>,
}

pub fn search(db: &Connection, query: &str, limit: usize) -> Result<Vec<Hit>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let (sql, pattern) = if q.chars().count() >= 3 {
        ("WITH candidates AS MATERIALIZED (SELECT f.path,f.name,f.folded FROM names JOIN files f ON f.id=names.rowid WHERE names MATCH ?1 AND instr(f.folded,?2)>0 LIMIT 1000) SELECT path,name FROM candidates ORDER BY (folded=?2) DESC,length(name),path LIMIT ?3", format!("\"{}\"",q.replace('"', "\"\"")))
    } else {
        ("SELECT path,name FROM files WHERE folded LIKE ?1 ESCAPE '\\' ORDER BY folded COLLATE NOCASE LIMIT ?3", format!("{}%",q.replace('\\',"\\\\").replace('%',"\\%").replace('_',"\\_")))
    };
    let mut stmt = db.prepare(sql)?;
    // Numbered parameters allow the short-query branch to share this binding.
    let rows = stmt.query_map(params![pattern, q, limit.min(200) as i64], |r| {
        Ok(Hit {
            path: r.get(0)?,
            name: r.get(1)?,
            snippet: None,
        })
    })?;
    let mut hits: Vec<Hit> = rows.collect::<rusqlite::Result<_>>()?;
    content::search_content(db, &q, limit.min(200), &mut hits)?;
    Ok(hits)
}

/// Each root is published atomically. Failed or disconnected roots retain their last snapshot.
/// Symlinks/junctions are never followed, so selected roots bound the traversal.
pub fn reconcile(db: &mut Connection, config: &Config) -> Result<Vec<String>> {
    config.validate()?;
    let matcher = config.matcher()?;
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
        "DELETE FROM files WHERE root NOT IN (SELECT path FROM roots)",
        [],
    )?;
    tx.commit()?;
    for root in &config.folders {
        let result = (|| -> Result<()> {
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
                snapshot.push((path.to_owned(), name.to_owned()));
            }
            let tx = db.transaction()?;
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS seen(path TEXT PRIMARY KEY); DELETE FROM seen;",
            )?;
            for (path, name) in &snapshot {
                tx.prepare_cached("INSERT INTO seen VALUES(?1)")?
                    .execute([path])?;
                tx.prepare_cached("INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4) ON CONFLICT(path) DO UPDATE SET root=excluded.root,name=excluded.name,folded=excluded.folded WHERE files.root != excluded.root OR files.name != excluded.name")?.execute(params![root.to_string_lossy(),path,name,name.to_lowercase()])?;
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
