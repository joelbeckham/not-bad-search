use anyhow::{Context, Result};
use rusqlite::{types::ValueRef, Connection};
use serde_json::{Map, Value};

fn rows(db: &Connection, sql: &str, id: i64) -> Result<Vec<Value>> {
    let mut stmt = db.prepare(sql)?;
    let columns: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
    let mut rows = stmt.query([id])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let mut object = Map::new();
        for (i, name) in columns.iter().enumerate() {
            let value = match row.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(v) => v.into(),
                ValueRef::Real(v) => serde_json::json!(v),
                ValueRef::Text(v) => String::from_utf8_lossy(v).into_owned().into(),
                ValueRef::Blob(v) => format!("{} bytes", v.len()).into(),
            };
            object.insert(name.clone(), value);
        }
        result.push(Value::Object(object));
    }
    Ok(result)
}

/// A consistent local database snapshot. No source-file reads or API calls.
pub fn inspect(db: &mut Connection, path: &str) -> Result<Value> {
    let tx = db.transaction()?;
    let id: i64 = tx
        .query_row("SELECT id FROM files WHERE path=?1", [path], |r| r.get(0))
        .context(
            "This file is no longer in the index. Search again or refresh after indexing finishes.",
        )?;
    let file = rows(&tx, "SELECT * FROM files WHERE id=?1", id)?.remove(0);
    let mut documents = rows(
        &tx,
        "SELECT * FROM documents WHERE file_id=?1 ORDER BY kind,id",
        id,
    )?;
    for document in &mut documents {
        let document_id = document["id"].as_i64().context("Invalid document ID")?;
        document["chunks"] = rows(
            &tx,
            "SELECT * FROM chunks WHERE document_id=?1 ORDER BY ordinal,id",
            document_id,
        )?
        .into();
        for chunk in document["chunks"].as_array_mut().unwrap() {
            chunk["embeddings"] = rows(
                &tx,
                "SELECT * FROM embeddings WHERE chunk_id=?1",
                chunk["id"].as_i64().context("Invalid chunk ID")?,
            )?
            .into();
        }
    }
    let jobs = rows(&tx, "SELECT * FROM image_jobs WHERE file_id=?1", id)?;
    let cache=rows(&tx,"SELECT c.* FROM image_cache c JOIN image_jobs j ON j.source_hash=c.source_hash AND j.model=c.model AND j.version=c.version WHERE j.file_id=?1",id)?;
    let remote = rows(&tx, "SELECT * FROM remote_items WHERE file_id=?1", id)?;
    let sync = rows(&tx,"SELECT s.* FROM remote_sync s JOIN remote_items r ON r.source_id=s.source_id WHERE r.file_id=?1",id)?;
    let failures = rows(&tx,"SELECT e.* FROM remote_failures e JOIN remote_items r ON r.source_id=e.source_id AND r.site=e.site AND r.remote_id=e.remote_id WHERE r.file_id=?1",id)?;
    let attachments=rows(&tx,"SELECT a.*,r.title AS parent_title,r.original_url AS parent_url FROM attachments a JOIN remote_items r ON r.file_id=a.parent_file_id WHERE a.file_id=?1",id)?;
    let value = serde_json::json!({"file":file,"attachments":attachments,"remote_item":remote,"sync":sync,"remote_failures":failures,"documents":documents,"image_jobs":jobs,"image_cache":cache});
    tx.commit()?;
    Ok(value)
}
