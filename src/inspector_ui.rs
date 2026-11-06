use crate::App;
use eframe::egui;
use serde_json::Value;

pub(super) struct Inspector {
    pub path: String,
    receiver: std::sync::mpsc::Receiver<Result<Value, String>>,
    data: Option<Result<Value, String>>,
}
impl App {
    pub(super) fn inspect_file(&mut self, path: String) {
        let database = self.config_path.with_file_name("index.sqlite");
        let (sender, receiver) = std::sync::mpsc::channel();
        let selected = path.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut db = good_search::open_reader(&database)?;
                good_search::inspection::inspect(&mut db, &selected)
            })();
            let _ = sender.send(result.map_err(|e: anyhow::Error| format!("{e:#}")));
        });
        self.inspector = Some(Inspector {
            path,
            receiver,
            data: None,
        });
    }

    pub(super) fn draw_inspector(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        if inspector.data.is_none() {
            match inspector.receiver.try_recv() {
                Ok(result) => inspector.data = Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    inspector.data = Some(Err("Inspector worker stopped".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16))
                }
            }
        }
        let mut refresh = false;
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(16, 10))
            .show(ui, |ui| {
                let location=inspector.data.as_ref().and_then(|d|d.as_ref().ok()).and_then(|d|d["remote_item"][0]["original_url"].as_str()).unwrap_or(&inspector.path);
                ui.add(egui::Label::new(location).truncate()).on_hover_text(location);
                ui.label("Local index snapshot. Refresh to see new indexing results.");
                ui.horizontal(|ui| {
                    refresh = ui.button("Refresh").clicked();
                    if let Some(Ok(data)) = &inspector.data {
                        if ui.button("Copy all as JSON").clicked() {
                            ctx.copy_text(serde_json::to_string_pretty(data).unwrap_or_default());
                        }
                    }
                });
                egui::ScrollArea::both()
                    .id_salt("inspector_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match &inspector.data {
                        None => {
                            ui.spinner();
                            ui.label("Loading indexed data…");
                        }
                        Some(Err(error)) => {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        Some(Ok(data)) => {
                            section(ui, "File metadata", &data["file"], true);
                            for (key, title) in [
                                ("remote_item", "Remote item metadata and raw content"),
                                ("sync", "Source synchronization"),
                                ("remote_failures", "Unresolved synchronization failures"),
                                ("documents", "Extracted documents"),
                                ("image_jobs", "Image processing jobs"),
                                ("image_cache", "Cached image extraction"),
                            ] {
                                let entries = data[key].as_array().unwrap();
                                ui.heading(format!("{title} ({})", entries.len()));
                                if entries.is_empty() {
                                    ui.label("None stored");
                                }
                                for (index, entry) in entries.iter().enumerate() {
                                    ui.push_id((key, index), |ui| {
                                        section(
                                            ui,
                                            entry["kind"].as_str().unwrap_or(title),
                                            entry,
                                            false,
                                        );
                                    });
                                }
                            }
                        }
                    });
            });
        let path = inspector.path.clone();
        if refresh {
            self.inspect_file(path);
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str, value: &Value, expanded: bool) {
    egui::CollapsingHeader::new(title)
        .default_open(expanded)
        .show(ui, |ui| {
            if let Some(fields) = value.as_object() {
                for (name, value) in fields {
                    ui.push_id(name, |ui| {
                        if let Some(entries) = value.as_array() {
                            ui.label(format!("{name} ({})", entries.len()));
                            for (i, entry) in entries.iter().enumerate() {
                                ui.push_id(i, |ui| {
                                    section(
                                        ui,
                                        &if name == "embeddings" {
                                            entry["model"]
                                                .as_str()
                                                .unwrap_or("Embedding")
                                                .to_owned()
                                        } else {
                                            format!("Chunk {}", entry["ordinal"])
                                        },
                                        entry,
                                        false,
                                    );
                                });
                            }
                        } else if let Some(text) = value.as_str().filter(|_| {
                            matches!(
                                name.as_str(),
                                "body"
                                    | "extracted_text"
                                    | "description"
                                    | "raw_adf"
                                    | "searchable_text"
                                    | "warnings"
                            )
                        }) {
                            ui.horizontal(|ui| {
                                ui.strong(name);
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(text.to_owned());
                                }
                            });
                            if text.is_empty() {
                                ui.label("(empty)");
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("text")
                                    .max_height(180.)
                                    .show(ui, |ui| {
                                        let mut readonly = text;
                                        ui.add(
                                            egui::TextEdit::multiline(&mut readonly)
                                                .desired_width(f32::INFINITY)
                                                .font(egui::TextStyle::Monospace),
                                        );
                                    });
                            }
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("{name}:"));
                                ui.add(
                                    egui::Label::new(
                                        value
                                            .as_str()
                                            .map(str::to_owned)
                                            .unwrap_or_else(|| value.to_string()),
                                    )
                                    .selectable(true),
                                );
                            });
                        }
                    });
                }
            }
        });
}
