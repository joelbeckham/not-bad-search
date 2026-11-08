use crate::App;
use eframe::egui::{self, Color32, RichText};
use good_search::{preview::Content, Hit};
use std::sync::{mpsc, Arc, Mutex};

type Request = (u64, String, String);
type Reply = (u64, Result<Content, String>);

/// One worker per app, with one replaceable pending request and one result slot.
/// Slow reads (including WSL paths) never block the UI or create more threads.
pub(super) struct Loader {
    pending: Arc<Mutex<Option<Request>>>,
    reply: Arc<Mutex<Option<Reply>>>,
    wake: mpsc::SyncSender<()>,
    generation: u64,
}
impl Loader {
    pub fn new(ctx: egui::Context, database: std::path::PathBuf) -> Self {
        let pending = Arc::new(Mutex::new(None::<Request>));
        let reply = Arc::new(Mutex::new(None));
        let (wake, rx) = mpsc::sync_channel(1);
        let work = pending.clone();
        let output = reply.clone();
        std::thread::spawn(move || {
            let _ = crate::platform::background_priority();
            while rx.recv().is_ok() {
                let request = work.lock().unwrap().take();
                if let Some((id, path, query)) = request {
                    let result = (|| -> anyhow::Result<Content> {
                        if path.starts_with("good-search://jira/") {
                            let db=good_search::open_reader(&database)?;
                            let (text,warnings):(String,String)=db.query_row("SELECT searchable_text,warnings FROM remote_items r JOIN files f ON f.id=r.file_id WHERE f.path=?1",[&path],|r|Ok((r.get(0)?,r.get(1)?)))?;
                            let text=if warnings!="[]"{format!("{text}\n\nIndexing notes: {warnings}")}else{text};
                            Ok(good_search::preview::text_preview(&text,&query,false))
                        }else {good_search::preview::load(std::path::Path::new(&path), &query)}
                    })()
                        .map_err(|e| format!("{e:#}"));
                    *output.lock().unwrap() = Some((id, result));
                    ctx.request_repaint();
                }
            }
        });
        Self {
            pending,
            reply,
            wake,
            generation: 0,
        }
    }

    fn request(&mut self, path: String, query: String) -> u64 {
        self.generation += 1;
        *self.reply.lock().unwrap() = None;
        *self.pending.lock().unwrap() = Some((self.generation, path, query));
        let _ = self.wake.try_send(());
        self.generation
    }

    pub fn cancel(&mut self) {
        self.pending.lock().unwrap().take();
        self.reply.lock().unwrap().take();
    }
}

pub(super) struct Preview {
    pub items: Vec<Hit>,
    pub selected: usize,
    request: u64,
    content: Option<Result<Content, String>>,
    texture: Option<egui::TextureHandle>,
    initial_scroll: bool,
    open_error: Option<String>,
    zoom: f32,
}

impl App {
    pub(super) fn start_preview(&mut self, ctx: &egui::Context) {
        let Some(hit) = self.hits.get(self.selected) else {
            return;
        };
        let loader = self.preview_loader.get_or_insert_with(|| {
            Loader::new(ctx.clone(), self.config_path.with_file_name("index.sqlite"))
        });
        let request = loader.request(hit.path.clone(), self.query.clone());
        self.preview = Some(Preview {
            items: self.hits.clone(),
            selected: self.selected,
            request,
            content: None,
            texture: None,
            initial_scroll: true,
            open_error: None,
            zoom: 1.0,
        });
        crate::trace(
            &self.config_path.with_file_name("worker-trace.log"),
            &format!("preview requested path={:?}", hit.path),
        );
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        self.focus = false;
    }

    pub(super) fn draw_preview(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let preview = self.preview.as_mut().unwrap();
        let loader = self.preview_loader.as_mut().unwrap();
        let previous = preview.selected;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            preview.selected = (preview.selected + 1).min(preview.items.len() - 1);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            preview.selected = preview.selected.saturating_sub(1);
        }
        let mut open = fresh_enter(&ctx);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(preview.selected > 0, egui::Button::new("Previous"))
                .clicked()
            {
                preview.selected -= 1;
            }
            if ui
                .add_enabled(
                    preview.selected + 1 < preview.items.len(),
                    egui::Button::new("Next"),
                )
                .clicked()
            {
                preview.selected += 1;
            }
            ui.label(format!(
                "{} of {}",
                preview.selected + 1,
                preview.items.len()
            ));
            open |= ui
                .button(if preview.items[preview.selected].original_url.is_some() {
                    "Open in Jira"
                } else {
                    "Open file"
                })
                .clicked();
        });
        let hit = &preview.items[preview.selected];
        if previous != preview.selected {
            preview.request = loader.request(hit.path.clone(), self.query.clone());
            preview.content = None;
            preview.texture = None;
            preview.initial_scroll = true;
            preview.open_error = None;
        }
        if open {
            preview.open_error = crate::platform::open_file(
                &hit.path,
                &self.config_path.with_file_name("index.sqlite"),
            )
            .err()
            .map(|e| e.to_string());
            crate::trace(
                &self.config_path.with_file_name("worker-trace.log"),
                &format!(
                    "preview open path={:?} success={}",
                    hit.path,
                    preview.open_error.is_none()
                ),
            );
        }
        ui.add(
            egui::Label::new(RichText::new(hit.location()).size(12.).color(Color32::GRAY))
                .truncate(),
        )
        .on_hover_text(hit.location());
        jira_summary(ui, hit, true);
        if let Some(info) = &hit.jira {
            ui.horizontal_wrapped(|ui| {
                for (label, value) in [("Assignee", &info.assignee), ("Type", &info.issue_type), ("Priority", &info.priority), ("Project", &info.project)] {
                    if !value.is_empty() { ui.label(format!("{label}: {value}")); }
                }
            });
            ui.collapsing("Ticket details", |ui| {
                if !info.created.is_empty() { ui.label(format!("Created: {}", full_date(&info.created))); }
                if !info.reporter.is_empty() { ui.label(format!("Reporter: {}", info.reporter)); }
                if !info.labels.is_empty() { ui.label(format!("Labels: {}", info.labels.join(", "))); }
                ui.label(format!("Last synced: {}", full_date(&info.synced)));
            });
        }
        if let Some(snippet) = &hit.snippet {
            ui.add(
                egui::Label::new(RichText::new(snippet).color(Color32::from_rgb(120, 195, 204)))
                    .truncate(),
            )
            .on_hover_text(snippet);
        }
        if let Some(error) = &preview.open_error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if let Some((id, result)) = loader.reply.lock().unwrap().take() {
            if id == preview.request {
                crate::trace(
                    &self.config_path.with_file_name("worker-trace.log"),
                    &format!(
                        "preview ready path={:?} success={}",
                        hit.path,
                        result.is_ok()
                    ),
                );
                preview.content = Some(result);
            }
        }
        if let Some(Ok(Content::Image {
            width,
            height,
            rgba,
            ..
        })) = preview.content.as_mut()
        {
            if preview.texture.is_none() {
                preview.texture = Some(ctx.load_texture(
                    "file_preview",
                    egui::ColorImage::from_rgba_unmultiplied([*width, *height], rgba),
                    egui::TextureOptions::LINEAR,
                ));
                rgba.clear();
                rgba.shrink_to_fit();
            }
        }
        match &preview.content {
            None => {
                ui.spinner();
                ui.label("Loading preview...");
                // Only poll while loading to cover a worker repaint coalesced with this frame.
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
            Some(Err(error)) => {
                ui.label(error);
            }
            Some(Ok(Content::Image { original, .. })) => {
                ui.horizontal(|ui| {
                    if ui.button("−").clicked() {preview.zoom=(preview.zoom/1.25).max(0.25);}
                    if ui.button("+").clicked() {preview.zoom=(preview.zoom*1.25).min(8.0);}
                    if ui.button("Fit").clicked() {preview.zoom=1.0;}
                    ui.weak("Scroll or drag to pan when zoomed");
                });
                ui.label(
                    RichText::new(format!("{} x {}", original.0, original.1))
                        .small()
                        .color(Color32::GRAY),
                );
                let texture = preview.texture.as_ref().unwrap();
                let available = ui.available_size();
                let scale = (available.x / texture.size_vec2().x)
                    .min(available.y / texture.size_vec2().y)
                    .clamp(0.01, 1.0);
                egui::ScrollArea::both().scroll_source(egui::scroll_area::ScrollSource::ALL).show(ui,|ui| {
                    ui.image((texture.id(), texture.size_vec2() * scale * preview.zoom));
                });
            }
            Some(Ok(Content::Text {
                lines,
                truncated,
                first_match,
            })) => {
                if *truncated {
                    ui.label(
                        "Partial preview (size or line limit). Open the file to read everything.",
                    );
                }
                if lines.is_empty() {
                    ui.label("This file is empty.");
                }
                let height = ui.text_style_height(&egui::TextStyle::Monospace);
                let mut scroll = egui::ScrollArea::both()
                    .id_salt("preview_text")
                    .auto_shrink([false, false]);
                if preview.initial_scroll {
                    scroll = scroll.scroll_offset(egui::vec2(
                        0.,
                        first_match.saturating_sub(3) as f32
                            * (height + ui.spacing().item_spacing.y),
                    ));
                    preview.initial_scroll = false;
                }
                scroll.show_rows(ui, height, lines.len(), |ui, rows| {
                    for row in rows {
                        let line = &lines[row];
                        let mut job = egui::text::LayoutJob::default();
                        let normal = egui::TextFormat {
                            font_id: egui::FontId::monospace(13.),
                            color: Color32::from_rgb(225, 228, 232),
                            ..Default::default()
                        };
                        job.append(
                            &format!("{:>5}  ", row + 1),
                            0.,
                            egui::TextFormat {
                                color: Color32::GRAY,
                                ..normal.clone()
                            },
                        );
                        let mut start = 0;
                        for range in &line.matches {
                            job.append(&line.text[start..range.start], 0., normal.clone());
                            job.append(
                                &line.text[range.clone()],
                                0.,
                                egui::TextFormat {
                                    background: Color32::from_rgb(55, 88, 93),
                                    color: Color32::WHITE,
                                    ..normal.clone()
                                },
                            );
                            start = range.end;
                        }
                        job.append(&line.text[start..], 0., normal);
                        ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                    }
                });
            }
        }
    }
}

/// A held Enter must not preview and immediately launch the file on key repeat.
pub(super) fn fresh_enter(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    repeat: false,
                    ..
                }
            )
        })
    })
}

fn full_date(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&chrono::Local).format("%b %d, %Y %H:%M:%S %:z").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

fn relative_date(value: &str) -> String {
    let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) else { return value.to_owned(); };
    let seconds = chrono::Utc::now().signed_duration_since(date).num_seconds();
    if seconds < 0 { return full_date(value); }
    let (count, unit) = if seconds < 60 { return "just now".into(); }
        else if seconds < 3600 { (seconds / 60, "minute") }
        else if seconds < 86400 { (seconds / 3600, "hour") }
        else { (seconds / 86400, "day") };
    format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

pub(super) fn jira_summary(ui: &mut egui::Ui, hit: &Hit, expanded: bool) {
    let Some(info) = &hit.jira else { return; };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.;
        if !hit.path.starts_with("good-search://jira/") { ui.weak("Ticket:"); }
        ui.hyperlink_to(&info.key, hit.location());
        let color = match info.category.as_str() {
            "done" => Color32::from_rgb(100, 205, 140),
            "indeterminate" => Color32::from_rgb(100, 175, 245),
            _ => Color32::GRAY,
        };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(14., 16.), egui::Sense::hover());
        let center = rect.center();
        let painter = ui.painter();
        painter.circle_stroke(center, 5.5, egui::Stroke::new(1.4, color));
        if info.category == "done" {
            painter.line_segment([center + egui::vec2(-3., 0.), center + egui::vec2(-0.5, 2.5)], egui::Stroke::new(1.5, color));
            painter.line_segment([center + egui::vec2(-0.5, 2.5), center + egui::vec2(3., -2.5)], egui::Stroke::new(1.5, color));
        } else if info.category == "indeterminate" {
            let points = (0..=16).map(|i| {
                let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 16.;
                center + egui::vec2(angle.cos(), angle.sin()) * 4.
            }).collect();
            painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
        }
        let status = if info.status.is_empty() { "Status unavailable" } else { &info.status };
        response.on_hover_text(status);
        ui.add(egui::Label::new(RichText::new(status).small().color(color)).truncate()).on_hover_text(status);
        let date = if expanded { full_date(&info.updated) } else { relative_date(&info.updated) };
        ui.add(egui::Label::new(RichText::new(format!("Updated {date}")).small().weak()).truncate()).on_hover_text(full_date(&info.updated));
    });
}
