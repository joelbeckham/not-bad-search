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
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id_salt("search_input")
                            .frame(Frame::NONE)
                            .font(FontId::proportional(21.))
                            .hint_text(RichText::new("Search your files…").color(MUTED))
                            .desired_width(f32::INFINITY),
                    );
                    if self.focus && !self.settings {
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
                    let label = if self.settings && self.inspector.is_some() {
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
                    if ui
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
                            let back_label = if self.inspector.is_some() {
                                "Back to indexed data"
                            } else if self.preview.is_some() {
                                "Back to preview"
                            } else {
                                "Back to search"
                            };
                            ui.label(RichText::new(back_label).size(12.).color(MUTED));
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Quit Good Search").size(12.).color(MUTED),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                std::process::exit(0);
                            }
                            ui.label(
                                RichText::new("Ctrl + Alt + Space to summon")
                                    .size(11.)
                                    .color(MUTED),
                            );
                        } else {
                            key(ui, "Enter");
                            ui.label(
                                RichText::new(if self.preview.is_some() {
                                    "Open file"
                                } else {
                                    "Preview"
                                })
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
                ui.horizontal(|ui| {
                    ui.add_space(12.);
                    ui.label(
                        RichText::new(if self.searching {
                            "Searching…"
                        } else if self.semantic_pending || self.semantic_due.is_some() {
                            "Finding related results..."
                        } else {
                            "Files"
                        })
                        .size(12.)
                        .color(MUTED),
                    );
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
                ui.add_space(4.);
                let moved = ctx.input(|i| {
                    i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                });
                if self.inspector.is_none() && ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    self.selected = (self.selected + 1).min(self.hits.len().saturating_sub(1));
                }
                if self.inspector.is_none() && ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.selected = self.selected.saturating_sub(1);
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
                                Vec2::new(ui.available_width(), if hit.snippet.is_some() { 78. } else { 58. }),
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
                                rect.min + Vec2::new(16., 18.),
                                Vec2::new(17., 22.),
                            );
                            ui.painter().rect_stroke(
                                icon,
                                2,
                                Stroke::new(1.3, if selected { ACCENT } else { MUTED }),
                                egui::StrokeKind::Inside,
                            );
                            for offset in [8., 12., 16.] {
                                ui.painter().line_segment(
                                    [
                                        icon.min + Vec2::new(4., offset),
                                        icon.min + Vec2::new(13., offset),
                                    ],
                                    Stroke::new(1., MUTED),
                                );
                            }
                            let ext = std::path::Path::new(&hit.name)
                                .extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("file");
                            let ext: String =
                                ext.chars().take(8).collect::<String>().to_uppercase();
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
                                egui::Label::new(RichText::new(&hit.path).size(11.).color(MUTED))
                                    .truncate().sense(Sense::hover()),
                            );
                            if let Some(snippet) = &hit.snippet {
                                row_ui.add(egui::Label::new(RichText::new(snippet).size(12.).color(ACCENT)).truncate().sense(Sense::hover()));
                            }
                            ui.painter().text(
                                rect.right_center() - Vec2::new(15., 0.),
                                egui::Align2::RIGHT_CENTER,
                                ext,
                                FontId::proportional(10.),
                                MUTED,
                            );
                            if response.secondary_clicked() { self.selected=index; }
                            response.context_menu(|ui| {
                                if ui.button("Preview").clicked() { self.selected=index; preview=true;ui.close(); }
                                if ui.button("Inspect indexed data").clicked() {inspect=Some(hit.path.clone());ui.close();}
                                if ui.button("Copy path").clicked() {ctx.copy_text(hit.path.clone());ui.close();}
                                if ui.button("Open file").clicked() {open=Some(hit.path.clone());ui.close();}
                            });
                            if response.clicked() {
                                self.selected = index;
                                preview = true;
                            }
                            if moved && selected {
                                response.scroll_to_me(Some(Align::Center));
                            }
                            response.on_hover_text(&hit.path);
                        }
                        if self.hits.is_empty() {
                            ui.add_space(62.);
                            ui.vertical_centered(|ui| {
                                let (heading, detail) = if self.query.trim().is_empty() {
                                    (
                                        "Find something good.",
                                        "Search filenames and enabled text content in your selected folders.",
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
                                        "No matching files",
                                        "Try another name or check your selected folders.",
                                    )
                                };
                                ui.label(RichText::new(heading).size(18.).color(TEXT));
                                ui.add_space(6.);
                                ui.label(RichText::new(detail).size(13.).color(MUTED));
                            });
                        }
                    });
                if crate::preview_ui::fresh_enter(&ctx) {
                    if ctx.input(|i| i.modifiers.ctrl) {
                        open = self.hits.get(self.selected).map(|h| h.path.clone());
                    } else {
                        preview = true;
                    }
                }
                if preview { self.start_preview(&ctx); }
                if let Some(path) = inspect { self.inspect_file(path); }
                if let Some(path) = open {
                    let result = crate::platform::open_file(&path);
                    crate::trace(&self.config_path.with_file_name("worker-trace.log"), &format!("direct open path={path:?} success={}", result.is_ok()));
                    if let Err(e) = result {
                        self.error = e.to_string();
                    }
                }
            });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Make it yours").size(20.).color(TEXT));
                        ui.label(
                            RichText::new("Choose where Good Search looks.")
                                .size(12.)
                                .color(MUTED),
                        );
                        ui.add_space(10.);
                        ui.label(RichText::new("Selected folders").size(13.));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.folders)
                                .id_salt("folder_input")
                                .desired_rows(3)
                                .desired_width(f32::INFINITY)
                                .font(FontId::proportional(13.)),
                        );
                        ui.label(
                            RichText::new("One Windows or WSL folder path per line.")
                                .size(11.)
                                .color(MUTED),
                        );
                        ui.add_space(6.);
                        ui.label(RichText::new("Exclude files & folders").size(13.));
                        ui.add(
                            egui::TextEdit::multiline(&mut self.exclusions)
                                .id_salt("exclusions_input")
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .font(FontId::proportional(13.)),
                        );
                        ui.label(
                            RichText::new(
                                "One pattern per line — for example *.tmp or node_modules.",
                            )
                            .size(11.)
                            .color(MUTED),
                        );
                        ui.add_space(8.);
                        ui.label(RichText::new("Search text inside files").size(13.));
                        ui.label(
                            RichText::new(
                                "Opt in per folder · Markdown and UTF-8 text · stored locally",
                            )
                            .size(11.)
                            .color(MUTED),
                        );
                        let roots: Vec<std::path::PathBuf> = self
                            .folders
                            .lines()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(Into::into)
                            .collect();
                        self.content_folders.retain(|p| roots.contains(p));
                        for root in &roots {
                            let mut enabled = self.content_folders.contains(root);
                            if ui.checkbox(&mut enabled, root.to_string_lossy()).changed() {
                                self.content_folders.retain(|p| p != root);
                                if enabled {
                                    self.content_folders.push(root.clone());
                                }
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Maximum text file size");
                            ui.add(
                                egui::DragValue::new(&mut self.content_limit_mib)
                                    .range(1..=64)
                                    .suffix(" MiB"),
                            );
                        });
                        ui.add_space(12.);
                        ui.label(RichText::new("Semantic search").size(13.));
                        ui.checkbox(&mut self.config.semantic_enabled, "Find related content using local embeddings");
                        ui.label("Potion 8M runs on your CPU. One-time model download: about 30 MiB. Files and searches stay on this computer.");
                        ui.horizontal(|ui| {
                            ui.label("Typing pause before semantic search");
                            ui.add(egui::DragValue::new(&mut self.config.semantic_delay_ms).range(50..=2000).suffix(" ms"));
                        });
                        ui.label(&self.semantic_status);
                        ui.label("Applies with Save changes. Exact text results appear immediately. Quoted phrases remain strict.");
                        ui.add_space(12.);
                        ui.label(RichText::new("Search inside images with OpenAI").size(13.));
                        ui.label("Selected image folders send PNG, JPEG and WebP images to OpenAI during indexing. API charges apply. Searches stay local.");
                        self.config.image_folders.retain(|p|roots.contains(p));
                        for root in &roots {
                            let mut enabled=self.config.image_folders.contains(root);
                            if ui.checkbox(&mut enabled,root.to_string_lossy()).changed() {
                                self.config.image_folders.retain(|p|p!=root);
                                if enabled { self.config.image_folders.push(root.clone()); }
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Image analysis model");
                            egui::ComboBox::from_id_salt("image_analysis_model")
                                .selected_text(match self.config.image_model.as_str() {
                                    good_search::DEFAULT_IMAGE_MODEL => "GPT-6.1 Terra (default)",
                                    "gpt-6-luna" => "GPT-6 Luna",
                                    _ => "Choose a model",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.config.image_model, good_search::DEFAULT_IMAGE_MODEL.into(), "GPT-6.1 Terra (default)");
                                    ui.selectable_value(&mut self.config.image_model, "gpt-6-luna".into(), "GPT-6 Luna");
                                });
                        });
                        ui.label("Apply with Save changes. Changing models reanalyzes selected images and uses your daily request cap.");
                        ui.horizontal(|ui| {
                            ui.label("Daily request cap (UTC)");
                            ui.add(egui::DragValue::new(&mut self.config.image_daily_requests).range(0..=10000));
                        });
                        ui.label("Requests include retries. This is a request cap, not a dollar budget.");
                        ui.checkbox(&mut self.config.images_paused,"Pause image processing (apply with Save changes)");
                        ui.label(if self.key_saved { "API key saved in Windows Credential Manager" } else { "No API key saved" });
                        ui.add(egui::TextEdit::singleline(&mut self.api_key_input).password(true).hint_text("OpenAI API key"));
                        ui.horizontal(|ui| {
                            if ui.button("Store key").clicked() {
                                match good_search::credentials::save(&self.api_key_input) {
                                    Ok(())=>{self.api_key_input.clear();self.key_saved=true;let _=self.image_wake.try_send(false);},
                                    Err(e)=>self.error=e.to_string(),
                                }
                            }
                            if ui.button("Remove key").clicked() {
                                match good_search::credentials::remove() {Ok(())=>self.key_saved=false,Err(e)=>self.error=e.to_string()}
                            }
                            if ui.button("Retry failed images").clicked() {let _=self.image_wake.try_send(true);}
                        });
                        ui.label(&self.image_status);
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Save changes").color(TEXT))
                                    .fill(Color32::from_rgb(47, 78, 83))
                                    .min_size(Vec2::new(112., 30.)),
                            )
                            .clicked()
                        {
                            match self.save() {
                                Ok(()) => {
                                    self.settings = false;
                                    self.focus = true;
                                }
                                Err(e) => self.error = e.to_string(),
                            }
                        }
                        if !self.error.is_empty() {
                            ui.colored_label(Color32::from_rgb(226, 177, 112), &self.error);
                        }
                        ui.label(RichText::new(&self.status).size(11.).color(MUTED));
                    });
            });
    }
}
