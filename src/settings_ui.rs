use crate::App;
use eframe::egui::{self, Color32, RichText};
use good_search::sources::{Source, Status};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Sources,
    Search,
    Processing,
}
#[derive(Default)]
pub(super) struct State {
    tab: Tab,
    pub selected: Option<String>,
    stats: Vec<Status>,
    receiver: Option<mpsc::Receiver<Result<Vec<Status>, String>>>,
    checked: Option<Instant>,
    error: String,
}
impl App {
    fn source_status_update(&mut self, ui: &egui::Ui) {
        let state = &mut self.source_settings;
        if let Some(rx) = &state.receiver {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(stats) => {
                            state.stats = stats;
                            state.error.clear();
                        }
                        Err(e) => state.error = e,
                    }
                    state.receiver = None;
                    state.checked = Some(Instant::now());
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.receiver = None;
                    state.checked = Some(Instant::now());
                    state.error = "Source status worker stopped".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if state.receiver.is_none()
            && state
                .checked
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            let (tx, rx) = mpsc::channel();
            state.receiver = Some(rx);
            let config = self.shared_config.lock().unwrap().clone();
            let path = self.config_path.with_file_name("index.sqlite");
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = (|| {
                    crate::platform::background_priority()?;
                    let db = good_search::open_reader(&path)?;
                    good_search::sources::snapshot(&db, &config)
                })();
                let _ = tx.send(result.map_err(|e: anyhow::Error| e.to_string()));
                ctx.request_repaint();
            });
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
    pub(super) fn draw_settings(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.);
        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.add_space(20.);
            for (tab, label) in [
                (Tab::Sources, "Sources"),
                (Tab::Search, "Search"),
                (Tab::Processing, "Processing"),
            ] {
                if ui
                    .selectable_label(self.source_settings.tab == tab, label)
                    .clicked()
                {
                    self.source_settings.tab = tab;
                    self.source_settings.selected = None;
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Save changes").clicked() {
                match self.save() {
                    Ok(()) => {
                        self.source_settings.checked = None;
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            ui.weak("Changes apply when saved. Removing a source only removes its local index.");
        });
        if !self.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.error);
        }
        if !self.index_error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.index_error);
        }
        ui.add_space(8.);
        if self.source_settings.tab == Tab::Sources {
            self.source_status_update(ui);
        }
        egui::ScrollArea::vertical()
            .id_salt("settings_content")
            .auto_shrink([false, false])
            .show(ui, |ui| match self.source_settings.tab {
                Tab::Sources => self.draw_sources(ui),
                Tab::Search => self.draw_search_settings(ui),
                Tab::Processing => self.draw_processing_settings(ui),
            });
    }
    fn draw_sources(&mut self, ui: &mut egui::Ui) {
        if !self.source_settings.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.source_settings.error);
        }
        let selected = self.source_settings.selected.clone();
        if let Some(id) = selected {
            if ui.button("Back to sources").clicked() {
                self.source_settings.selected = None;
                return;
            }
            let Some(index) = self.config.sources.iter().position(|s| s.id == id) else {
                self.source_settings.selected = None;
                return;
            };
            let source = &mut self.config.sources[index];
            ui.heading(&source.name);
            ui.weak("Folder source · Windows and WSL paths supported");
            ui.label("Name");
            ui.text_edit_singleline(&mut source.name);
            ui.label("Folder path");
            let mut path = source.path.to_string_lossy().into_owned();
            if ui
                .add(egui::TextEdit::singleline(&mut path).desired_width(f32::INFINITY))
                .changed()
            {
                source.path = path.into();
            }
            ui.add_space(8.);
            ui.label("What to index");
            ui.label("Filenames are always searchable after discovery.");
            ui.checkbox(&mut source.text, "Text contents · Markdown and UTF-8 text");
            ui.checkbox(&mut source.images, "Image text and descriptions · OpenAI");
            if source.images {
                ui.weak("PNG, JPEG and WebP files are sent to OpenAI during indexing. API charges apply.");
            }
            ui.checkbox(
                &mut source.paused,
                "Pause this source's background processing",
            );
            ui.weak(
                "Existing results stay searchable. An operation already in progress may finish.",
            );
            ui.add_space(8.);
            ui.label("Exclude files and folders");
            let mut exclusions = source.exclusions.join("\n");
            if ui
                .add(
                    egui::TextEdit::multiline(&mut exclusions)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                source.exclusions = exclusions
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            ui.weak("One pattern per line, e.g. *.tmp or node_modules.");
            ui.separator();
            if let Some(stats) = self.source_settings.stats.iter().find(|s| s.id == id) {
                draw_status(ui, stats, source.paused, true);
            } else {
                ui.label("Save this source to begin discovery.");
            }
            let root = source.path.clone();
            ui.horizontal(|ui| {
                if ui.button("Check for changes").clicked(){let _=self.index_wake.try_send(());self.source_settings.checked=None;}
                if ui.button("Retry failed images").clicked(){
                    let path=self.config_path.with_file_name("index.sqlite");let wake=self.image_wake.clone();
                    // A retry only changes this source's persisted queue; no UI-thread DB writes.
                    let config=self.shared_config.lock().unwrap().clone();let ctx=ui.ctx().clone();
                    let (tx,rx)=mpsc::channel();self.source_settings.receiver=Some(rx);
                    std::thread::spawn(move || {
                        let result=(|| -> anyhow::Result<_> {
                            crate::platform::background_priority()?;
                            let db=good_search::open_writer(&path)?;
                            db.execute("UPDATE image_jobs SET state='pending',attempts=0,retry_at=0,detail='' WHERE state='failed' AND file_id IN (SELECT id FROM files WHERE root=?1)",[root.to_string_lossy().as_ref()])?;
                            let _=wake.try_send(false);
                            let reader=good_search::open_reader(&path)?;
                            good_search::sources::snapshot(&reader,&config)
                        })();
                        let _=tx.send(result.map_err(|e|e.to_string()));ctx.request_repaint();
                    });
                }
                if ui.button("Remove source").clicked(){self.config.sources.remove(index);self.source_settings.selected=None;}
            });
            return;
        }
        ui.horizontal(|ui| {
            ui.heading("Data sources");
            if ui.button("+ Add folder").clicked() {
                let mut source = Source::folder(std::path::PathBuf::new());
                source.id = blake3::hash(format!("{:?}", std::time::SystemTime::now()).as_bytes())
                    .to_hex()
                    .to_string();
                source.name = "New folder".into();
                source.exclusions = vec![
                    ".git".into(),
                    "node_modules".into(),
                    "target".into(),
                    "*.tmp".into(),
                ];
                self.source_settings.selected = Some(source.id.clone());
                self.config.sources.push(source);
            }
        });
        ui.weak("Each folder has its own indexing options and exclusions. Counts are files, not chunks.");
        if !self.source_settings.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.source_settings.error);
        }
        for source in &self.config.sources {
            egui::Frame::new()
                .fill(Color32::from_rgb(34, 36, 39))
                .corner_radius(6)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width((ui.available_width() - 4.).max(0.));
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new(&source.name).size(17.)).clicked() {
                            self.source_settings.selected = Some(source.id.clone());
                        }
                        ui.weak("Folder");
                    });
                    ui.label(source.path.to_string_lossy());
                    if let Some(stats) = self
                        .source_settings
                        .stats
                        .iter()
                        .find(|s| s.id == source.id)
                    {
                        draw_status(ui, stats, source.paused, false);
                    } else {
                        ui.weak("Not indexed yet · save changes to begin");
                    }
                });
            ui.add_space(8.);
        }
        if self.config.sources.is_empty() {
            ui.label("Add a folder to start searching your files.");
        }
        ui.weak("Counts reflect saved settings and the latest local index. Status refreshes every two seconds while this tab is open; offline sources retain their previous snapshot.");
    }
    fn draw_search_settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Semantic search").size(13.));
        ui.checkbox(
            &mut self.config.semantic_enabled,
            "Find related content using local embeddings",
        );
        ui.label("Potion 8M runs on your CPU. One-time model download: about 30 MiB. Files and searches stay on this computer.");
        ui.horizontal(|ui| {
            ui.label("Typing pause before semantic search");
            ui.add(
                egui::DragValue::new(&mut self.config.semantic_delay_ms)
                    .range(50..=2000)
                    .suffix(" ms"),
            );
        });
        ui.label(&self.semantic_status);
        ui.label("Applies with Save changes. Exact text results appear immediately. Quoted phrases remain strict.");
        ui.add_space(12.);
    }
    fn draw_processing_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Background processing");
        ui.horizontal(|ui| {
            ui.label("Maximum text file size");
            ui.add(
                egui::DragValue::new(&mut self.content_limit_mib)
                    .range(1..=64)
                    .suffix(" MiB"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Fallback folder check interval");
            ui.add(
                egui::DragValue::new(&mut self.config.reconcile_seconds)
                    .range(5..=3600)
                    .suffix(" seconds"),
            );
        });
        ui.weak("Indexing runs at low priority. File notifications trigger checks sooner; the fallback also supports WSL folders.");
        ui.separator();
        ui.heading("Image analysis");
        ui.horizontal(|ui| {
            ui.label("Image analysis model");
            egui::ComboBox::from_id_salt("image_analysis_model")
                .selected_text(match self.config.image_model.as_str() {
                    good_search::DEFAULT_IMAGE_MODEL => "GPT-6.1 Terra (default)",
                    "gpt-6-luna" => "GPT-6 Luna",
                    _ => "Choose a model",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.config.image_model,
                        good_search::DEFAULT_IMAGE_MODEL.into(),
                        "GPT-6.1 Terra (default)",
                    );
                    ui.selectable_value(
                        &mut self.config.image_model,
                        "gpt-6-luna".into(),
                        "GPT-6 Luna",
                    );
                });
        });
        ui.label("Apply with Save changes. Changing models reanalyzes selected images and uses your daily request cap.");
        ui.horizontal(|ui| {
            ui.label("Daily request cap (UTC)");
            ui.add(egui::DragValue::new(&mut self.config.image_daily_requests).range(0..=10000));
        });
        ui.label("Requests include retries. This is a request cap, not a dollar budget.");
        ui.checkbox(
            &mut self.config.images_paused,
            "Pause image processing (apply with Save changes)",
        );
        ui.label(if self.key_saved {
            "API key saved in Windows Credential Manager"
        } else {
            "No API key saved"
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.api_key_input)
                .password(true)
                .hint_text("OpenAI API key"),
        );
        ui.horizontal(|ui| {
            if ui.button("Store key").clicked() {
                match good_search::credentials::save(&self.api_key_input) {
                    Ok(()) => {
                        self.api_key_input.clear();
                        self.key_saved = true;
                        let _ = self.image_wake.try_send(false);
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            if ui.button("Remove key").clicked() {
                match good_search::credentials::remove() {
                    Ok(()) => self.key_saved = false,
                    Err(e) => self.error = e.to_string(),
                }
            }
            if ui.button("Retry failed images").clicked() {
                let _ = self.image_wake.try_send(true);
            }
        });
        ui.label(&self.image_status);
    }
}
fn draw_status(ui: &mut egui::Ui, s: &Status, paused: bool, detail: bool) {
    let state = if paused {
        "Paused"
    } else if !s.available {
        "Folder unavailable"
    } else if s.attention > 0 {
        "Needs attention"
    } else if s.processing > 0 || s.queued > 0 {
        "Processing"
    } else if s.items == 0 {
        "No items discovered"
    } else {
        "Up to date"
    };
    ui.label(format!(
        "{state} · {} items · {} searchable by filename",
        s.items, s.items
    ));
    ui.weak(format!(
        "{} queued · {} processing · {} ready · {} need attention · {} skipped",
        s.queued, s.processing, s.ready, s.attention, s.skipped
    ));
    if detail {
        ui.label(format!("Text extracted: {} files", s.text_ready));
        ui.label(format!("Images analyzed: {} files", s.images_ready));
        ui.label(format!(
            "Embeddings: {} files ready · {} files queued",
            s.embedding_ready, s.embedding_queued
        ));
        ui.weak("Ready means all enabled stages are complete in the current index snapshot. Filename-only files are ready after discovery.");
    }
}
