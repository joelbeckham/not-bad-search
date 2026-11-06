use crate::{Config, Event};
use eframe::egui;
use good_search::semantic::{self, LocalModel, VectorIndex};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) struct Worker {
    pending: Arc<Mutex<Option<(u64, String, good_search::SearchFilters)>>>,
    wake: mpsc::SyncSender<()>,
}
impl Worker {
    pub fn request(&self, id: u64, query: String, filters: good_search::SearchFilters) {
        *self.pending.lock().unwrap() = Some((id, query, filters));
        self.wake();
    }
    pub fn cancel(&self) {
        self.pending.lock().unwrap().take();
    }
    pub fn wake(&self) {
        let _ = self.wake.try_send(());
    }
    pub fn start(
        path: PathBuf,
        config: Arc<Mutex<Config>>,
        events: mpsc::Sender<Event>,
        ctx: egui::Context,
    ) -> Self {
        let pending = Arc::new(Mutex::new(
            None::<(u64, String, good_search::SearchFilters)>,
        ));
        let work = pending.clone();
        let (wake, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let status = |message: String, ready: bool, changed: bool| {
                let _ = events.send(Event::SemanticStatus(message, ready, changed));
                ctx.request_repaint();
            };
            let run = || -> anyhow::Result<()> {
                crate::platform::background_priority()?;
                let mut writer = good_search::open_writer(&path)?;
                let reader = good_search::open_reader(&path)?;
                let folder = path
                    .parent()
                    .unwrap()
                    .join("models")
                    .join(format!("potion-8M-{}", semantic::REVISION));
                let mut model = None;
                let mut index = VectorIndex::default();
                let mut cache: VecDeque<(String, Vec<f32>)> = VecDeque::new();
                let mut next_attempt = Instant::now();
                let mut report = Instant::now();
                let mut dirty = false;
                let mut was_enabled = false;
                loop {
                    let enabled = config.lock().unwrap().semantic_enabled;
                    if enabled && !was_enabled {
                        next_attempt = Instant::now();
                    }
                    if !enabled {
                        model = None;
                        index = VectorIndex::default();
                        cache.clear();
                        work.lock().unwrap().take();
                        if was_enabled {
                            status("Semantic search disabled".into(), false, false);
                        }
                    }
                    was_enabled = enabled;
                    if enabled && model.is_none() && Instant::now() >= next_attempt {
                        status(
                            "Preparing local semantic model (one-time download: about 30 MiB)..."
                                .into(),
                            false,
                            false,
                        );
                        match LocalModel::prepare(&folder) {
                            Ok(loaded) => {
                                model = Some(loaded);
                                status("Local semantic model ready".into(), true, true);
                            }
                            Err(e) => {
                                status(format!("Semantic model unavailable: {e:#}. Retrying in 60 seconds."),false,false);
                                next_attempt = Instant::now() + Duration::from_secs(60);
                            }
                        }
                    }
                    if enabled {
                        if let Some(model) = &model {
                            let request = work.lock().unwrap().take();
                            if let Some((id, query, filters)) = request {
                                let start = Instant::now();
                                let result = (|| -> anyhow::Result<_> {
                                    index.refresh(&reader)?;
                                    let vector = if let Some((_, vector)) =
                                        cache.iter().find(|(text, _)| text == &query)
                                    {
                                        vector.clone()
                                    } else {
                                        let vector = semantic::embed_query(model, &query)?;
                                        cache.push_back((query.clone(), vector.clone()));
                                        if cache.len() > 64 {
                                            cache.pop_front();
                                        }
                                        vector
                                    };
                                    index.search_filtered(&reader, vector, 100, filters)
                                })();
                                let result = result.map_err(|e| format!("{e:#}"));
                                let _ = events.send(Event::SemanticResults(
                                    id,
                                    query,
                                    result,
                                    start.elapsed(),
                                ));
                                ctx.request_repaint();
                            }
                            // A small batch bounds the time a new query waits behind indexing.
                            let paused_roots = config.lock().unwrap().paused_roots_json();
                            match semantic::index_batch_excluding(&mut writer, model, 32, &paused_roots) {
                                Ok(count) => {
                                    dirty |= count > 0;
                                    if count == 0 || report.elapsed() >= Duration::from_secs(1) {
                                        let indexed: i64 = writer.query_row(
                                            "SELECT count(*) FROM embeddings WHERE model=?1",
                                            [semantic::MODEL],
                                            |r| r.get(0),
                                        )?;
                                        let total:i64=writer.query_row("SELECT count(*) FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.status='ready'",[],|r|r.get(0))?;
                                        status(
                                            format!(
                                                "Local semantic index: {indexed} / {total} chunks"
                                            ),
                                            true,
                                            dirty,
                                        );
                                        dirty = false;
                                        report = Instant::now();
                                    }
                                    if count > 0 {
                                        continue;
                                    }
                                }
                                Err(e) => status(
                                    format!("Semantic indexing will retry: {e:#}"),
                                    true,
                                    false,
                                ),
                            }
                        }
                    }
                    // Notifications from indexing, edits and queries wake us; fallback is infrequent.
                    if let Err(mpsc::RecvTimeoutError::Disconnected) =
                        rx.recv_timeout(Duration::from_secs(30))
                    {
                        break;
                    }
                }
                Ok(())
            };
            if let Err(e) = run() {
                status(format!("Semantic worker stopped: {e:#}"), false, false);
            }
        });
        Self { pending, wake }
    }
}
