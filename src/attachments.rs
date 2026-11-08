//! Shared remote-image records; connector-specific discovery stays at the boundary.
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{io::Read, path::Path, time::Duration};

pub fn sync_jira(
    db: &mut Connection,
    source: &str,
    settings: &crate::jira::Settings,
    client: &mut crate::bifrost::Mcp,
    cache: &Path,
    cancel: impl Fn() -> bool,
) -> Result<()> {
    let parents: Vec<(i64,String,String)> = db.prepare("SELECT r.file_id,r.issue_key,r.remote_updated FROM remote_items r LEFT JOIN attachment_scans s ON s.parent_file_id=r.file_id WHERE r.source_id=?1 AND (s.remote_updated IS NULL OR s.remote_updated!=r.remote_updated) ORDER BY r.file_id LIMIT 100")?
        .query_map([source], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    std::fs::create_dir_all(cache)?;
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    for (parent, key, updated) in parents {
        if cancel() {
            return Ok(());
        }
        let detail=client.call("jira_getJiraIssue",json!({"cloudId":settings.cloud_id,"issueIdOrKey":key,"fields":["attachment"],"updateHistory":false}))?;
        let detail = detail.get("data").unwrap_or(&detail);
        let entries = detail["fields"]["attachment"]
            .as_array()
            .context("Jira omitted attachment list")?;
        for item in entries {
            if cancel() {
                return Ok(());
            }
            let mime = item["mimeType"].as_str().unwrap_or("");
            let ext = match mime {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => continue,
            };
            let size = item["size"].as_u64().context("Attachment omitted size")?;
            ensure!(size <= 10 * 1024 * 1024, "Jira image exceeds 10 MiB");
            let id = item["id"].as_str().context("Attachment omitted ID")?;
            let response =
                client.probe_v2_download(json!({"cloudId":settings.cloud_id,"attachmentId":id}))?;
            let url = download_url(&response).context("Download tool omitted HTTPS URL")?;
            let response = http
                .get(url)
                .send()
                .map_err(|_| anyhow::anyhow!("Attachment download connection failed"))?;
            ensure!(
                response.status().is_success(),
                "Attachment download HTTP {}",
                response.status().as_u16()
            );
            let mut bytes = Vec::new();
            response
                .take(10 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 == size,
                "Attachment size differs from metadata"
            );
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let identity = blake3::hash(format!("{source}/{parent}/{id}").as_bytes())
                .to_hex()
                .to_string();
            let path = cache.join(format!("{identity}-{hash}.{ext}"));
            std::fs::write(&path, &bytes)?;
            crate::images::image_bytes(&path)?;
            image::load_from_memory(&bytes)?.thumbnail(64,64).save(path.with_extension("thumb.png"))?;
            if cancel() {
                return Ok(());
            }
            let unchanged: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM attachments WHERE parent_file_id=?1 AND remote_id=?2 AND content_hash=?3)",params![parent,id,hash],|r|r.get(0))?;
            if unchanged {continue;}
            let filename = item["filename"].as_str().unwrap_or("image");
            let name = format!("{filename} · {key}");
            let tx = db.transaction()?;
            tx.execute("DELETE FROM files WHERE id IN (SELECT file_id FROM attachments WHERE parent_file_id=?1 AND remote_id=?2)",params![parent,id])?;
            tx.execute(
                "INSERT INTO files(root,path,name,folded) VALUES(?1,?2,?3,?4)",
                params![
                    format!("jira:{source}"),
                    path.to_string_lossy(),
                    name,
                    name.to_lowercase()
                ],
            )?;
            let file = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO attachments VALUES(?1,?2,'jira',?3,?4,?5,?6,?7,?8,?9)",
                params![
                    file,
                    parent,
                    source,
                    id,
                    filename,
                    mime,
                    size as i64,
                    hash,
                    crate::jira::now()
                ],
            )?;
            tx.commit()?;
        }
        db.execute(
            "INSERT OR REPLACE INTO attachment_scans VALUES(?1,?2)",
            params![parent, updated],
        )?;
    }
    Ok(())
}

fn download_url(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => {
            let url = reqwest::Url::parse(s).ok()?;
            let host = url.host_str()?;
            (url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && (host.ends_with(".atlassian.com") || host.ends_with(".atl-paas.net")))
            .then_some(s)
        }
        Value::Object(o) => o.values().find_map(download_url),
        Value::Array(a) => a.iter().find_map(download_url),
        _ => None,
    }
}

pub fn prune_cache(db: &Connection, cache: &Path) -> Result<()> {
    if !cache.is_dir() {return Ok(());}
    let retained:Vec<String>=db.prepare("SELECT path FROM files f JOIN attachments a ON a.file_id=f.id")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for entry in std::fs::read_dir(cache)? {
        let entry=entry?;let metadata=entry.metadata()?;
        if !metadata.is_file() || metadata.modified()?.elapsed().unwrap_or_default()<Duration::from_secs(86400) {continue;}
        let path=entry.path();
        let referenced:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM files WHERE path=?1)",[path.to_string_lossy()],|r|r.get(0))?;
        if !referenced && !retained.iter().any(|p|Path::new(p).with_extension("thumb.png")==path) {std::fs::remove_file(path)?;}
    }
    Ok(())
}
