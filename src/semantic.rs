//! Local embeddings and hybrid ranking, independent of the native UI.
use crate::Hit;
use anyhow::{bail, ensure, Context, Result};
use model2vec_rs::model::StaticModel;
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::HashMap, io::Read, path::Path, time::Duration};

pub const REVISION: &str = "bf8b056651a2c21b8d2565580b8569da283cab23";
pub const MODEL: &str = "potion-base-8M/bf8b0566/heading-body-v1";
pub const DIMENSIONS: usize = 256;

pub trait Embedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
pub struct LocalModel(StaticModel);
impl LocalModel {
    pub fn load(folder: &Path) -> Result<Self> {
        let model = StaticModel::from_pretrained(folder, None, Some(true), None)?;
        ensure!(
            model.dim() == DIMENSIONS,
            "Unexpected embedding model dimensions"
        );
        Ok(Self(model))
    }
    /// Called only by the background worker when semantic search is enabled.
    pub fn prepare(folder: &Path) -> Result<Self> {
        std::fs::create_dir_all(folder)?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        for name in ["config.json", "tokenizer.json", "model.safetensors"] {
            let path = folder.join(name);
            if path.exists() {
                continue;
            }
            let url = format!(
                "https://huggingface.co/minishlab/potion-base-8M/resolve/{REVISION}/{name}"
            );
            let response = client.get(url).send()?.error_for_status()?;
            let mut bytes = Vec::new();
            response
                .take(40 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 40 * 1024 * 1024,
                "Model download exceeds size limit"
            );
            let temporary = path.with_extension("download");
            std::fs::write(&temporary, bytes)?;
            std::fs::rename(temporary, path)?;
        }
        Self::load(folder)
    }
}
impl Embedder for LocalModel {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Single-item calls avoid Rayon thread-pool fanout on the user's machine.
        // 4096 tokens covers the bounded chunks without the default 512-token truncation.
        let mut output = Vec::with_capacity(texts.len());
        for text in texts {
            output.extend(
                self.0
                    .encode_with_args(std::slice::from_ref(text), Some(4096), 1),
            );
        }
        Ok(output)
    }
}

fn normalize(vector: &mut [f32]) -> Result<()> {
    ensure!(
        vector.len() == DIMENSIONS && vector.iter().all(|v| v.is_finite()),
        "Invalid embedding vector"
    );
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0. {
        for value in vector {
            *value /= norm;
        }
    }
    Ok(())
}
fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn decode(bytes: &[u8]) -> Result<Vec<f32>> {
    ensure!(
        bytes.len() == DIMENSIONS * 4,
        "Invalid stored embedding length"
    );
    let mut vector: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    normalize(&mut vector)?;
    Ok(vector)
}

/// Inference happens outside the short publish transaction. Changed/deleted chunks are skipped.
pub fn index_batch(db: &mut Connection, model: &impl Embedder, limit: usize) -> Result<usize> {
    index_batch_excluding(db, model, limit, "[]")
}
pub fn index_batch_excluding(
    db: &mut Connection,
    model: &impl Embedder,
    limit: usize,
    paused: &str,
) -> Result<usize> {
    let rows: Vec<(i64,String,String,String)> = db.prepare("SELECT c.id,c.content_hash,c.heading,c.body FROM chunks c JOIN documents d ON d.id=c.document_id LEFT JOIN embeddings e ON e.chunk_id=c.id AND e.model=?1 AND e.content_hash=c.content_hash WHERE d.status='ready' AND e.chunk_id IS NULL AND d.file_id IN (SELECT id FROM files WHERE root NOT IN (SELECT value FROM json_each(?3))) ORDER BY c.id LIMIT ?2")?
        .query_map(params![MODEL, limit.min(64) as i64, paused], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?.collect::<rusqlite::Result<_>>()?;
    if rows.is_empty() {
        return Ok(0);
    }
    let inputs: Vec<_> = rows
        .iter()
        .map(|(_, _, heading, body)| format!("{heading}\n{body}"))
        .collect();
    let vectors = model.embed(&inputs)?;
    ensure!(vectors.len() == rows.len(), "Embedding count mismatch");
    let tx = db.transaction()?;
    for ((id, hash, heading, body), mut vector) in rows.iter().zip(vectors) {
        normalize(&mut vector)?;
        tx.execute("INSERT OR REPLACE INTO embeddings(chunk_id,model,content_hash,dimensions,vector) SELECT c.id,?2,c.content_hash,?3,?4 FROM chunks c JOIN documents d ON d.id=c.document_id WHERE c.id=?1 AND c.content_hash=?5 AND c.heading=?6 AND c.body=?7 AND d.status='ready'", params![id,MODEL,DIMENSIONS as i64,encode(&vector),hash,heading,body])?;
    }
    tx.commit()?;
    Ok(rows.len())
}

#[derive(Default)]
pub struct VectorIndex {
    entries: Vec<(i64, i64, String, Vec<f32>, bool, String)>,
    data_version: Option<i64>,
}
impl VectorIndex {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn refresh(&mut self, db: &Connection) -> Result<()> {
        let version: i64 = db.query_row("PRAGMA data_version", [], |r| r.get(0))?;
        if self.data_version == Some(version) {
            return Ok(());
        }
        let rows: Vec<(i64,i64,String,Vec<u8>,bool,String)>=db.prepare("SELECT e.chunk_id,d.file_id,e.content_hash,e.vector,d.kind='text',f.root FROM embeddings e JOIN chunks c ON c.id=e.chunk_id AND c.content_hash=e.content_hash JOIN documents d ON d.id=c.document_id JOIN files f ON f.id=d.file_id WHERE e.model=?1 AND e.dimensions=?2 AND d.status='ready'")?
            .query_map(params![MODEL,DIMENSIONS as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?.collect::<rusqlite::Result<_>>()?;
        let entries = rows
            .into_iter()
            .map(|(id, file, hash, blob, text, root)| {
                Ok((id, file, hash, decode(&blob)?, text, root))
            })
            .collect::<Result<_>>()?;
        self.entries = entries;
        self.data_version = Some(version);
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.data_version = None;
    }
    pub fn search(&self, db: &Connection, query: Vec<f32>, limit: usize) -> Result<Vec<Hit>> {
        self.search_filtered(db, query, limit, crate::SearchFilters::default())
    }
    pub fn search_filtered(
        &self,
        db: &Connection,
        mut query: Vec<f32>,
        limit: usize,
        filters: crate::SearchFilters,
    ) -> Result<Vec<Hit>> {
        if !filters.semantic {
            return Ok(vec![]);
        }
        normalize(&mut query)?;
        if limit == 0 || query.iter().all(|v| *v == 0.) {
            return Ok(vec![]);
        }
        let mut best: HashMap<i64, (i64, f32, &str)> = HashMap::new();
        for (id, file, hash, v, text, root) in &self.entries {
            if filters
                .roots
                .as_ref()
                .is_some_and(|roots| !roots.contains(root))
            {
                continue;
            }
            if (*text && !filters.text) || (!*text && !filters.images) {
                continue;
            }
            let score: f32 = v.iter().zip(&query).map(|(a, b)| a * b).sum();
            if score >= 0.35 && best.get(file).is_none_or(|(_, old, _)| score > *old) {
                best.insert(*file, (*id, score, hash));
            }
        }
        let mut scores: Vec<_> = best.into_values().collect();
        scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut hits = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut stmt=db.prepare("SELECT f.path,f.name,d.kind,c.body FROM embeddings e JOIN chunks c ON c.id=e.chunk_id AND c.content_hash=e.content_hash JOIN documents d ON d.id=c.document_id JOIN files f ON f.id=d.file_id WHERE e.chunk_id=?1 AND e.model=?2 AND e.content_hash=?3 AND d.status='ready'")?;
        for (id, _, hash) in scores.into_iter().take(if filters.sort.by_date() {usize::MAX} else {2000}) {
            let row: Option<(String, String, String, String)> = stmt
                .query_row(params![id, MODEL, hash], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .optional()?;
            if let Some((path, name, kind, body)) = row {
                if seen.insert(path.clone()) {
                    let kind = match kind.as_str() {
                        "image_description" => "image description",
                        "image_text" => "image text",
                        _ => "text",
                    };
                    hits.push(Hit {
                        original_url: None,
                        jira: None, timestamp: None, slack: false,
                        path,
                        name,
                        snippet: Some(format!(
                            "Semantic {kind}: {}",
                            body.chars()
                                .take(180)
                                .collect::<String>()
                                .replace(['\n', '\r'], " ")
                        )),
                    });
                    if !filters.sort.by_date() && hits.len() >= limit {
                        break;
                    }
                }
            }
        }
        crate::remote_locations(db, &mut hits)?;
        if filters.sort.by_date() { crate::sorting::sort(&mut hits,"",filters.sort,chrono::Utc::now().timestamp()); }
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Reciprocal-rank fusion: lexical and meaning-based evidence both contribute.
/// Quoted queries retain their strict lexical candidate set; exact filenames stay first.
pub fn merge(query: &str, lexical: Vec<Hit>, semantic: Vec<Hit>, limit: usize) -> Vec<Hit> {
    let mut ranked: HashMap<String, (Hit, f32)> = HashMap::new();
    for (rank, hit) in lexical.into_iter().enumerate() {
        ranked.insert(hit.path.clone(), (hit, 1.0 / (60. + rank as f32)));
    }
    for (rank, hit) in semantic.into_iter().enumerate() {
        let score = 1.0 / (60. + rank as f32);
        if let Some((_, total)) = ranked.get_mut(&hit.path) {
            *total += score;
        } else if !query.contains('"') {
            ranked.insert(hit.path.clone(), (hit, score * 0.85));
        }
    }
    let exact = query.trim().to_lowercase();
    let mut ranked: Vec<_> = ranked.into_values().collect();
    ranked.sort_by(|a, b| {
        (b.0.name.to_lowercase() == exact)
            .cmp(&(a.0.name.to_lowercase() == exact))
            .then(b.1.total_cmp(&a.1))
            .then(a.0.path.cmp(&b.0.path))
    });
    ranked.into_iter().take(limit).map(|(hit, _)| hit).collect()
}

pub fn embed_query(model: &impl Embedder, query: &str) -> Result<Vec<f32>> {
    if query.chars().count() > 2048 {
        bail!("Query is too long for semantic search");
    }
    let mut vectors = model.embed(&[query.to_owned()])?;
    let mut vector = vectors.pop().context("No query embedding returned")?;
    normalize(&mut vector)?;
    Ok(vector)
}
