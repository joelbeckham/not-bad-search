use crate::{Config, Hit};
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::HashSet, fs::File, io::Read, path::Path};

const EXTRACTOR_VERSION: i64 = 1;
const CHUNKER_VERSION: i64 = 1;

pub(crate) struct Chunk {
    pub(crate) heading: String,
    pub(crate) line: usize,
    pub(crate) body: String,
}

// Keep raw Markdown (including code) searchable. Bounded chunks respect lines
// where possible; even a single very long line is bounded on Unicode boundaries.
pub(crate) fn chunks(text: &str) -> Vec<Chunk> {
    let mut result = Vec::new();
    let mut heading = String::new();
    let mut body = String::new();
    let mut start = 1;
    for (i, line) in text.lines().enumerate() {
        let title = line
            .trim_start()
            .strip_prefix('#')
            .filter(|s| s.starts_with('#') || s.starts_with(' '));
        if !body.is_empty() && (body.len() + line.len() > 4096 || title.is_some()) {
            result.push(Chunk {
                heading: heading.clone(),
                line: start,
                body: std::mem::take(&mut body),
            });
        }
        if let Some(title) = title {
            heading = title
                .trim_start_matches('#')
                .trim()
                .chars()
                .take(256)
                .collect();
        }
        for ch in line.chars().chain(std::iter::once('\n')) {
            if body.is_empty() {
                start = i + 1;
            }
            body.push(ch);
            if body.len() >= 4096 {
                result.push(Chunk {
                    heading: heading.clone(),
                    line: start,
                    body: std::mem::take(&mut body),
                });
            }
        }
    }
    if !body.trim().is_empty() {
        result.push(Chunk {
            heading,
            line: start,
            body,
        });
    }
    result
}

fn read_text(path: &Path, cap: u64) -> Result<(String, String)> {
    let file = File::open(path)?;
    if file.metadata()?.len() > cap {
        bail!("File exceeds text size limit");
    }
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        bail!("File exceeds text size limit");
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let text = String::from_utf8(bytes)?;
    if text.contains('\0') {
        bail!("Binary content is not supported");
    }
    Ok((hash, text.trim_start_matches('\u{feff}').to_owned()))
}

pub(crate) fn index(db: &mut Connection, config: &Config) -> Result<Vec<String>> {
    let files: Vec<(i64, String, String)> = db
        .prepare(
            "SELECT id,path,root FROM files WHERE id NOT IN (SELECT file_id FROM remote_items)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut failures = 0;
    let available_roots: HashSet<_> = config
        .content_folders
        .iter()
        .filter(|p| p.is_dir())
        .collect();
    for (file_id, path, root) in files {
        if config.source_paused(Path::new(&root)) {
            continue;
        }
        let supported = Path::new(&path)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt")
            });
        if !supported || !config.content_folders.iter().any(|p| p == Path::new(&root)) {
            db.execute(
                "DELETE FROM documents WHERE file_id=?1 AND kind='text'",
                [file_id],
            )?;
            continue;
        }
        // Preserve the last searchable snapshot for disconnected roots.
        if !available_roots
            .iter()
            .any(|p| p.as_path() == Path::new(&root))
        {
            continue;
        }
        let extracted = read_text(Path::new(&path), config.max_content_bytes);
        let (hash, text, status, detail) = match extracted {
            Ok((hash, text)) => (hash, text, "ready", String::new()),
            Err(e) => {
                failures += 1;
                (String::new(), String::new(), "error", e.to_string())
            }
        };
        let previous: Option<(i64,String,i64,i64,String,String)> = db.query_row(
            "SELECT id,source_hash,extractor_version,chunker_version,status,detail FROM documents WHERE file_id=?1 AND kind='text'",
            [file_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        if previous.as_ref().is_some_and(|p| {
            p.1 == hash
                && p.2 == EXTRACTOR_VERSION
                && p.3 == CHUNKER_VERSION
                && p.4 == status
                && p.5 == detail
        }) {
            continue;
        }
        let chunks = chunks(&text);
        // No filesystem I/O or extraction while holding the writer transaction.
        let tx = db.transaction()?;
        tx.execute("INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(?1,'text',?2,?3,?4,?5,?6) ON CONFLICT(file_id,kind) DO UPDATE SET source_hash=excluded.source_hash,extractor_version=excluded.extractor_version,chunker_version=excluded.chunker_version,status=excluded.status,detail=excluded.detail",
            params![file_id,hash,EXTRACTOR_VERSION,CHUNKER_VERSION,status,detail])?;
        let id: i64 = tx.query_row(
            "SELECT id FROM documents WHERE file_id=?1 AND kind='text'",
            [file_id],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM chunks WHERE document_id=?1", [id])?;
        for (ordinal, chunk) in chunks.into_iter().enumerate() {
            let hash = blake3::hash(chunk.body.as_bytes()).to_hex().to_string();
            tx.execute("INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(?1,?2,?3,?4,?5,?6)",params![id,ordinal as i64,chunk.heading,chunk.line as i64,chunk.body,hash])?;
        }
        tx.commit()?;
    }
    Ok(if failures == 0 {
        vec![]
    } else {
        vec![format!("{failures} text files could not be indexed (size, encoding, or read error). Filenames remain searchable.")]
    })
}

pub(crate) fn search_content(
    db: &Connection,
    terms: &[String],
    limit: usize,
    hits: &mut Vec<Hit>,
    filters: crate::SearchFilters,
) -> Result<()> {
    if (!filters.sort.by_date() && hits.len() >= limit) || !terms.iter().any(|s| s.chars().any(char::is_alphanumeric)) {
        return Ok(());
    }
    // AND literal terms and quoted phrases; never expose raw FTS syntax.
    let pattern = crate::query::fts(terms);
    let date_order=if filters.sort.by_date() { format!(" ORDER BY {}",filters.sort.sql_order()) } else { " ORDER BY rank LIMIT 1000".into() };
    let mut stmt = db.prepare(&format!("SELECT f.path,f.name,snippet(chunk_search,1,'[',']',' ... ',24),c.start_line,d.kind,(SELECT timestamp FROM result_dates WHERE file_id=f.id) AS sort_date FROM chunk_search JOIN chunks c ON c.id=chunk_search.rowid JOIN documents d ON d.id=c.document_id JOIN files f ON f.id=d.file_id WHERE chunk_search MATCH ?1 AND ((d.kind='text' AND ?2) OR (d.kind IN ('image_text','image_description') AND ?3)) AND (?4='null' OR f.root IN (SELECT value FROM json_each(?4))){date_order}"))?;
    let mut rows = stmt.query(params![
        pattern,
        filters.text,
        filters.images,
        serde_json::to_string(&filters.roots)?
    ])?;
    let mut seen: HashSet<String> = hits.iter().map(|h| h.path.clone()).collect();
    let initial_count=hits.len();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if seen.insert(path.clone()) {
            let excerpt: String = row.get(2)?;
            let line: i64 = row.get(3)?;
            let kind: String = row.get(4)?;
            let label = match kind.as_str() {
                "image_text" => "Image text".to_owned(),
                "image_description" => "Image description".to_owned(),
                _ => format!("Text · line {line}"),
            };
            hits.push(Hit {
                original_url: None,
                jira: None, gmail: None, timestamp: None, notion: false, slack: false,
                path,
                name: row.get(1)?,
                snippet: Some(format!(
                    "{label}: {}",
                    excerpt.split_whitespace().collect::<Vec<_>>().join(" ")
                )),
            });
            if hits.len() >= if filters.sort.by_date() {initial_count+limit} else {limit} {
                break;
            }
        }
    }
    Ok(())
}
