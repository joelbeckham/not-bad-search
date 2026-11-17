use crate::{Config, Event};
use eframe::egui;
use not_bad_search::{
    bifrost::{Connection, Mcp},
    jira::{self, Outcome},
};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

pub(super) fn start(
    path: PathBuf,
    config: Arc<Mutex<Config>>,
    events: mpsc::Sender<Event>,
    ctx: egui::Context,
) -> mpsc::SyncSender<()> {
    let (wake, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let run = || -> anyhow::Result<()> {
            crate::platform::background_priority()?;
            let mut db = not_bad_search::open_writer(&path)?;
            db.execute("UPDATE remote_sync SET retry_at=0,failures=0 WHERE error LIKE '%Unsupported Jira description format: mixed%'",[])?;
            let mut manual = false;
            let mut last_cache_cleanup=std::time::Instant::now();
            loop {
                let current = config.lock().unwrap().clone();
                let ids: Vec<_> = current
                    .sources
                    .iter()
                    .filter(|s| !s.is_folder())
                    .map(|s| s.id.clone())
                    .collect();
                // Only an explicitly removed source removes local remote-item records.
                db.execute("DELETE FROM files WHERE id IN (SELECT file_id FROM remote_items WHERE source_id NOT IN (SELECT value FROM json_each(?1)))",[serde_json::to_string(&ids)?])?;
                db.execute("DELETE FROM remote_sync WHERE source_id NOT IN (SELECT value FROM json_each(?1))",[serde_json::to_string(&ids)?])?;
                db.execute("DELETE FROM remote_failures WHERE source_id NOT IN (SELECT value FROM json_each(?1))",[serde_json::to_string(&ids)?])?;
                if last_cache_cleanup.elapsed()>=Duration::from_secs(3600) {
                    not_bad_search::attachments::prune_cache(&db,&path.parent().unwrap().join("attachments"))?;
                    last_cache_cleanup=std::time::Instant::now();
                }
                for source in current
                    .sources
                    .iter()
                    .filter(|s| !s.is_folder() && !s.paused)
                {
                    let poll_seconds = source.notion.as_ref().map(|s|s.poll_seconds).or_else(||source.gmail.as_ref().map(|s|s.poll_seconds)).or_else(||source.slack.as_ref().map(|s|s.poll_seconds)).or_else(||source.jira.as_ref().map(|s|s.poll_seconds)).unwrap_or(300);
                    db.execute("INSERT INTO remote_sync(source_id,scope_hash) VALUES(?1,'') ON CONFLICT DO NOTHING",[&source.id])?;
                    let (last,retry,pending):(Option<String>,i64,bool)=db.query_row("SELECT last_success,retry_at,run_id IS NOT NULL FROM remote_sync WHERE source_id=?1",[&source.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                    let due = last
                        .as_deref()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .is_none_or(|last| {
                            chrono::Utc::now().timestamp() - last.timestamp()
                                >= poll_seconds as i64
                        });
                    if !manual && (retry > chrono::Utc::now().timestamp() || (!due && !pending)) {
                        continue;
                    }
                    let expected = serde_json::to_string(source)?;
                    let cancel = || {
                        config
                            .lock()
                            .unwrap()
                            .sources
                            .iter()
                            .find(|s| s.id == source.id)
                            .is_none_or(|s| {
                                s.paused || serde_json::to_string(s).ok().as_deref() != Some(expected.as_str())
                            })
                    };
                    let result = (|| -> anyhow::Result<Outcome> {
                        if let Some(settings)=&source.notion { let mut client=Mcp::connect_discovery(Connection::load()?)?; let outcome=not_bad_search::notion::sync(&mut db,&source.id,settings,&mut client,&path.parent().unwrap().join("attachments"),source.images,cancel); not_bad_search::images::discover(&mut db,&config.lock().unwrap().clone())?; return outcome; }
                        if let Some(settings)=&source.gmail { let mut client=Mcp::connect_discovery(Connection::load()?)?; return not_bad_search::gmail::sync(&mut db,&source.id,settings,&mut client,cancel); }
                        if let Some(settings) = &source.slack {
                            let mut client = Mcp::connect_discovery(Connection::load()?)?;
                            let outcome=not_bad_search::slack::sync(&mut db,&source.id,settings,&mut client,cancel)?;
                            if source.images && !cancel() {
                                not_bad_search::slack::sync_images(&mut db,&source.id,&mut client,&path.parent().unwrap().join("attachments"),cancel)?;
                                not_bad_search::images::discover(&mut db,&config.lock().unwrap().clone())?;
                            }
                            return Ok(outcome);
                        }
                        let settings=source.jira.as_ref().ok_or_else(||anyhow::anyhow!("Missing source settings"))?;
                        let mut client = Mcp::connect(Connection::load()?)?;
                        if manual {
                            db.execute("DELETE FROM attachment_scans WHERE parent_file_id IN (SELECT file_id FROM remote_items WHERE source_id=?1)",[&source.id])?;
                        }
                        let outcome=jira::sync(&mut db, &source.id, settings, &mut client, cancel)?;
                        if !cancel() {
                            not_bad_search::attachments::sync_jira(&mut db,&source.id,settings,&mut client,&path.parent().unwrap().join("attachments"),cancel)?;
                            not_bad_search::images::discover(&mut db,&config.lock().unwrap().clone())?;
                        }
                        Ok(outcome)
                    })();
                    let message = match result {
                        Ok(Outcome::Complete) => format!("{}: sync complete", source.name),
                        Ok(Outcome::More) => format!("{}: import will continue", source.name),
                        Ok(Outcome::Cancelled) => {
                            format!("{}: sync paused or scope changed", source.name)
                        }
                        Err(e) => {
                            let error = format!("{e:#}");
                            if source.notion.is_some() {
                                // Keep the traversal checkpoint even when the same child fails repeatedly.
                                db.execute("UPDATE remote_sync SET error=?2,failures=failures+1,retry_at=?3+min(3600,30*(1 << min(failures,7))),last_attempt=?4 WHERE source_id=?1",rusqlite::params![source.id,error,chrono::Utc::now().timestamp(),jira::now()])?;
                            } else { jira::record_error(&db, &source.id, &error)?; }
                            format!("{}: {error}", source.name)
                        }
                    };
                    let _ = events.send(Event::RemoteStatus(message, true));
                    ctx.request_repaint();
                }
                match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) => manual = true,
                    Err(mpsc::RecvTimeoutError::Timeout) => manual = false,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            Ok(())
        };
        if let Err(e) = run() {
            let _ = events.send(Event::RemoteStatus(
                format!("Remote sync worker stopped: {e:#}"),
                false,
            ));
            ctx.request_repaint();
        }
    });
    wake
}
