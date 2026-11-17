use crate::App;
use eframe::egui::{
    self, Align, Color32, FontId, Frame, Layout, Margin, RichText, Sense, Stroke, Vec2,
};

const BG: Color32 = Color32::from_rgb(22, 23, 25);
const HEADER: Color32 = Color32::from_rgb(28, 29, 31);
const LINE: Color32 = Color32::from_rgb(46, 48, 51);
const TEXT: Color32 = Color32::from_rgb(235, 236, 238);
const MUTED: Color32 = Color32::from_rgb(144, 148, 155);
const ACCENT: Color32 = Color32::from_rgb(120, 195, 204);

fn key(ui: &mut egui::Ui, text: &str) {
    Frame::new()
        .fill(Color32::from_rgb(44, 46, 49))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(11.)
                    .color(Color32::from_rgb(191, 194, 200)),
            );
        });
}

impl App {
    pub(super) fn draw_window(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.style_mut().visuals.override_text_color = Some(TEXT);
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(57, 91, 99);
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 36, 39);
        ui.style_mut().spacing.item_spacing = Vec2::new(8., 6.);
        let outer = ui.max_rect().shrink(1.);
        ui.painter().rect_filled(outer, 12, BG);
        ui.painter()
            .rect_stroke(outer, 12, Stroke::new(1., LINE), egui::StrokeKind::Inside);

        // Panels reserve the header/footer first; the result area fills the remainder.
        egui::Panel::top("search_header")
            .exact_size(76.)
            .frame(
                Frame::new()
                    .fill(HEADER)
                    .corner_radius(egui::CornerRadius {
                        nw: 12,
                        ne: 12,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin::symmetric(20, 20)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(28.), Sense::hover());
                    let center = icon.center();
                    ui.painter().circle_stroke(
                        center - Vec2::splat(3.),
                        7.,
                        Stroke::new(2., ACCENT),
                    );
                    ui.painter().line_segment(
                        [center + Vec2::splat(2.), center + Vec2::splat(10.)],
                        Stroke::new(2., ACCENT),
                    );
                    ui.add_space(8.);
                    if self.settings {
                        ui.label(RichText::new("Settings").size(21.).color(TEXT));
                        return;
                    }
                    if let Some(preview) = &self.preview {
                        let title = if self.settings {
                            "Settings"
                        } else {
                            &preview.items[preview.selected].name
                        };
                        ui.add(
                            egui::Label::new(RichText::new(title).size(21.).color(TEXT)).truncate(),
                        );
                        return;
                    }
                    if self.inspector.is_some() && !self.settings {
                        ui.label(RichText::new("Indexed data").size(21.).color(TEXT));
                        return;
                    }
                    let search_id = ui.make_persistent_id("search_input");
                    let search_screen =
                        !self.settings && self.inspector.is_none() && self.preview.is_none();
                    // Claim focus before TextEdit processes this frame's keyboard events,
                    // including the first keystroke after clicking or scrolling elsewhere.
                    if search_screen {
                        ui.memory_mut(|memory| memory.request_focus(search_id));
                    }
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(search_id)
                            .frame(Frame::NONE)
                            .font(FontId::proportional(21.))
                            .desired_width(f32::INFINITY),
                    );
                    if search_screen || (self.focus && !self.settings) {
                        input.request_focus();
                        self.focus = false;
                    }
                    if self.query != self.submitted_query {
                        self.hits.clear();
                        self.submit();
                    }
                });
            });
        egui::Panel::bottom("search_footer")
            .exact_size(48.)
            .frame(
                Frame::new()
                    .fill(HEADER)
                    .corner_radius(egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 12,
                        se: 12,
                    })
                    .inner_margin(Margin::symmetric(18, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let label = if self.settings && self.source_settings.selected.is_some() {
                        "Back to sources"
                    } else if self.settings && self.inspector.is_some() {
                        "Back to indexed data"
                    } else if self.settings && self.preview.is_some() {
                        "Back to preview"
                    } else if self.settings || self.inspector.is_some() || self.preview.is_some() {
                        "‹  Back to search"
                    } else {
                        "Settings"
                    };
                    let show_settings_key =
                        !self.settings && self.inspector.is_none() && self.preview.is_none();
                    if self.settings {
                        let response = ui.add(egui::Button::new("").min_size(Vec2::new(28., 28.)).frame(false))
                            .on_hover_text("Shut down Not Bad Search");
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Shut down Not Bad Search"));
                        let center = response.rect.center();
                        let stroke = egui::Stroke::new(1.7, MUTED);
                        ui.painter().line_segment([center + Vec2::new(-5., -5.), center + Vec2::new(5., 5.)], stroke);
                        ui.painter().line_segment([center + Vec2::new(-5., 5.), center + Vec2::new(5., -5.)], stroke);
                        let text_response = ui.add(
                            egui::Button::new(RichText::new("Shut down Not Bad Search").size(12.).color(MUTED))
                                .frame(false),
                        );
                        if response.clicked() || text_response.clicked() {
                            std::process::exit(0);
                        }
                    } else if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(13.).color(MUTED))
                                .frame(false),
                        )
                        .clicked()
                    {
                        if self.inspector.is_some() || self.settings || self.preview.is_some() {
                            self.back();
                        } else {
                            self.settings = true;
                        }
                        self.focus = !self.settings;
                    }
                    if show_settings_key {
                        key(ui, "Ctrl + ,");
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.inspector.is_some() && !self.settings {
                            key(ui, "Esc");
                            ui.label(RichText::new("Back to search").size(12.).color(MUTED));
                        } else if self.settings {
                            key(ui, "Esc");
                            let back_label = if self.source_settings.selected.is_some() {
                                "Back to sources"
                            } else if self.inspector.is_some() {
                                "Back to indexed data"
                            } else if self.preview.is_some() {
                                "Back to preview"
                            } else {
                                "Back to search"
                            };
                            ui.label(RichText::new(back_label).size(12.).color(MUTED));
                            ui.label(
                                RichText::new("Ctrl + Alt + Space to summon")
                                    .size(11.)
                                    .color(MUTED),
                            );
                        } else {
                            key(ui, "Enter");
                            ui.label(
                                RichText::new(self.preview.as_ref().map_or("Preview", |p| p.items[p.selected].open_label()))
                                .size(12.)
                                .color(MUTED),
                            );
                            ui.add_space(12.);
                            key(ui, "Up / Down");
                            ui.label(RichText::new("Navigate").size(12.).color(MUTED));
                            ui.add_space(12.);
                            key(ui, "Esc");
                            let action = if self.preview.is_some() {
                                "Back to search"
                            } else if self.query.is_empty() {
                                "Hide"
                            } else {
                                "Clear search"
                            };
                            ui.label(RichText::new(action).size(12.).color(MUTED));
                        }
                    });
                });
            });
        egui::CentralPanel::no_frame()
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(8, 8)))
            .show(ui, |ui| {
                if self.inspector.is_some() && !self.settings {
                    self.draw_inspector(ui);
                    return;
                }
                if self.settings {
                    self.draw_settings(ui);
                    return;
                }
                if self.preview.is_some() {
                    self.draw_preview(ui);
                    return;
                }
                if !self.error.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&self.error)
                                .size(12.)
                                .color(Color32::from_rgb(226, 177, 112)),
                        )
                        .wrap(),
                    );
                }
                if !self.index_error.is_empty() {
                    ui.colored_label(Color32::from_rgb(226, 177, 112), &self.index_error);
                }
                let mut filters_changed = false;
                ui.horizontal(|ui| {
                    ui.add_space(8.);
                    filters_changed = self.draw_filters(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(12.);
                        ui.label(
                            RichText::new(if self.query.trim().is_empty() {
                                "Local index".to_owned()
                            } else {
                                format!("{} results", self.hits.len())
                            })
                            .size(11.)
                            .color(MUTED),
                        )
                        .on_hover_text(format!(
                            "{}\nLast query: {:.2} ms",
                            self.status,
                            self.elapsed.as_secs_f64() * 1000.
                        ));
                    });
                });
                if filters_changed { self.hits.clear(); self.submit(); self.focus=true; }
                ui.add_space(4.);
                let moved = ctx.input(|i| {
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                });
                if !egui::Popup::is_any_open(&ctx) && self.inspector.is_none() && ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    self.selected = if self.selection_started { (self.selected + 1).min(self.hits.len().saturating_sub(1)) } else { 0 };
                    self.selection_started = !self.hits.is_empty();
                }
                if !egui::Popup::is_any_open(&ctx) && self.inspector.is_none() && ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.selected = self.selected.saturating_sub(1);
                    self.selection_started = !self.hits.is_empty();
                }
                let mut open = None;
                let mut inspect = None;
                let mut preview = false;
                egui::ScrollArea::vertical()
                    .id_salt("search_results")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, hit) in self.hits.iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), (if hit.snippet.is_some() { 78. } else { 58. }) + if hit.jira.is_some() || hit.gmail.is_some() || hit.slack_replies.is_some() { 22. } else { 0. }),
                                Sense::click(),
                            );
                            let selected = index == self.selected;
                            if selected || response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    6,
                                    if selected {
                                        Color32::from_rgb(41, 44, 48)
                                    } else {
                                        Color32::from_rgb(32, 34, 37)
                                    },
                                );
                            }
                            let icon = egui::Rect::from_min_size(
                                rect.min + Vec2::new(14., 18.),
                                Vec2::splat(22.),
                            );
                            let (uri, bytes) = if hit.path.starts_with("not-bad-search://notion/") {not_bad_search::file_icons::notion_icon()} else if hit.path.starts_with("not-bad-search://gmail/") {
                                not_bad_search::file_icons::gmail_icon()
                            } else if hit.path.starts_with("not-bad-search://slack/") {
                                not_bad_search::file_icons::slack_icon()
                            } else if hit.path.starts_with("not-bad-search://jira/") {
                                not_bad_search::file_icons::jira_icon()
                            } else {
                                not_bad_search::file_icons::icon(if hit.original_url.is_some() { &hit.path } else { &hit.name })
                            };
                            if hit.original_url.is_some() && !hit.path.starts_with("not-bad-search://") {
                                let thumbnail=std::path::Path::new(&hit.path).with_extension("thumb.png");
                                egui::Image::new(format!("file:///{}",thumbnail.to_string_lossy())).paint_at(ui,icon);
                            } else {egui::Image::from_bytes(uri, bytes).paint_at(ui, icon);}
                            let ext = std::path::Path::new(&hit.name)
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file");
                            let ext: String = if hit.notion {"NOTION".into()} else if hit.path.starts_with("not-bad-search://gmail/") { "GMAIL".into() } else if hit.slack { "SLACK".into() } else if hit.original_url.is_some() { "JIRA".into() } else {
                                ext.chars().take(8).collect::<String>().to_uppercase() };
                            let text_rect = egui::Rect::from_min_max(
                                rect.min + Vec2::new(48., 8.),
                                rect.max - Vec2::new(85., 5.),
                            );
                            let mut row_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .sense(Sense::hover())
                                    .max_rect(text_rect)
                                    .layout(Layout::top_down(Align::Min)),
                            );
                            row_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
                            row_ui.spacing_mut().item_spacing.y = 3.;
                            row_ui.add(
                                egui::Label::new(RichText::new(&hit.name).size(16.).color(TEXT))
                                    .truncate().sense(Sense::hover()),
                            );
                            row_ui.add(
                                egui::Label::new(RichText::new(hit.location()).size(11.).color(MUTED))
                                    .truncate().sense(Sense::hover()),
                            );
                            if let Some(snippet) = &hit.snippet {
                                row_ui.add(egui::Label::new(RichText::new(snippet).size(12.).color(ACCENT)).truncate().sense(Sense::hover()));
                            }
                            crate::preview_ui::jira_summary(&mut row_ui, hit, false);
                            crate::preview_ui::gmail_summary(&mut row_ui, hit, false);
                            crate::preview_ui::slack_summary(&mut row_ui, hit);
                            ui.painter().text(
                                rect.right_center() - Vec2::new(15., 0.),
                                egui::Align2::RIGHT_CENTER,
                                ext,
                                FontId::proportional(10.),
                                MUTED,
                            );
                            if response.secondary_clicked() { self.selected=index; self.selection_started=true; }
                            response.context_menu(|ui| {
                                if ui.button("Preview").clicked() { self.selected=index; self.selection_started=true; preview=true;ui.close(); }
                                if ui.button("Inspect indexed data").clicked() {inspect=Some(hit.path.clone());ui.close();}
                                if ui.button(if hit.original_url.is_some() {"Copy link"} else {"Copy path"}).clicked() {ctx.copy_text(hit.location().to_owned());ui.close();}
                                if ui.button(hit.open_label()).clicked() {open=Some(hit.path.clone());ui.close();}
                            });
                            if response.clicked() {
                                self.selected = index;
                                self.selection_started = true;
                                preview = true;
                            }
                            if moved && selected {
                                response.scroll_to_me(Some(Align::Center));
                            }
                            response.on_hover_text(hit.location());
                        }
                        if self.hits.is_empty() {
                            ui.add_space(62.);
                            ui.vertical_centered(|ui| {
                                let (heading, detail) = if self.query.trim().is_empty() {
                                    (
                                        "Find something good.",
                                        "Search names, titles and indexed content from your sources.",
                                    )
                                } else if self.searching || self.semantic_pending || self.semantic_due.is_some() {
                                    ("Searching your files…", "Results will appear here.")
                                } else if self.query_error.is_some() {
                                    (
                                        "Search couldn’t finish",
                                        self.query_error.as_deref().unwrap_or_default(),
                                    )
                                } else {
                                    (
                                        "No matching results",
                                        "Try another search or adjust your filters.",
                                    )
                                };
                                ui.label(RichText::new(heading).size(18.).color(TEXT));
                                ui.add_space(6.);
                                ui.label(RichText::new(detail).size(13.).color(MUTED));
                            });
                        }
                    });
                if !egui::Popup::is_any_open(&ctx) && crate::preview_ui::fresh_enter(&ctx) {
                    if ctx.input(|i| i.modifiers.ctrl) {
                        open = self.hits.get(self.selected).map(|h| h.path.clone());
                    } else {
                        preview = true;
                    }
                }
                if preview { self.start_preview(&ctx); }
                if let Some(path) = inspect { self.inspect_file(path); }
                if let Some(path) = open {
                    let result = crate::platform::open_file(&path,&self.config_path.with_file_name("index.sqlite"));
                    crate::trace(&self.config_path.with_file_name("worker-trace.log"), &format!("direct open path={path:?} success={}", result.is_ok()));
                    if let Err(e) = result {
                        self.error = e.to_string();
                    }
                }
            });
    }
}
