use crate::App;
use eframe::egui;
use good_search::filters::SourceSelection;

fn multiselect(ui: &mut egui::Ui, id: &str, label: String, contents: impl FnOnce(&mut egui::Ui)) {
    ui.push_id(id, |ui| {
        egui::containers::menu::MenuButton::new(label)
            .config(
                egui::containers::menu::MenuConfig::default()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, contents);
    });
}

fn pair(a: bool, b: bool, first: &str, second: &str) -> String {
    match (a, b) {
        (true, true) => "All".into(),
        (false, false) => "None".into(),
        (true, false) => first.into(),
        (false, true) => second.into(),
    }
}
impl App {
    pub(super) fn draw_filters(&mut self, ui: &mut egui::Ui) -> bool {
        let before = (self.filters.clone(), self.source_selection.clone());
        let sources = &self.config.sources;
        let selected: Vec<_> = sources
            .iter()
            .filter(|s| self.source_selection.contains(&s.id))
            .collect();
        let summary = if self.source_selection.0.is_none() {
            "All sources".into()
        } else if selected.is_empty() {
            "No sources".into()
        } else if selected.len() == 1 {
            selected[0].name.chars().take(22).collect::<String>()
        } else if selected.iter().all(|s| s.is_folder()) {
            format!("Folders ({})", selected.len())
        } else if selected.iter().all(|s| s.kind == selected[0].kind) {
            format!("{} ({})", selected[0].label(), selected.len())
        } else {
            format!("{} sources", selected.len())
        };
        multiselect(ui, "source_filters", format!("Sources: {summary}"), |ui| {
            let mut all = self.source_selection.0.is_none();
            if ui
                .checkbox(&mut all, "All sources")
                .on_hover_text("Automatically include newly added sources")
                .changed()
            {
                self.source_selection = if all {
                    SourceSelection::default()
                } else {
                    SourceSelection(Some(Default::default()))
                };
            }
            ui.separator();
            for (kind, label) in [(good_search::sources::SourceKind::Folder, "Folders"), (good_search::sources::SourceKind::Jira, "Jira"), (good_search::sources::SourceKind::Slack, "Slack")] {
                let group: Vec<_> = sources.iter().filter(|s| s.kind == kind).collect();
                if group.is_empty() {
                    continue;
                }
                let ids: Vec<_> = group.iter().map(|s| s.id.clone()).collect();
                let count = ids
                    .iter()
                    .filter(|id| self.source_selection.contains(id))
                    .count();
                let mut enabled = count == ids.len();
                if ui
                    .add(
                        egui::Checkbox::new(&mut enabled, label)
                            .indeterminate(count > 0 && count < ids.len()),
                    )
                    .changed()
                {
                    self.source_selection.set(&ids, enabled, sources);
                }
                ui.indent(label, |ui| {
                    for source in group {
                        let mut enabled = self.source_selection.contains(&source.id);
                        if ui
                            .checkbox(&mut enabled, &source.name)
                            .on_hover_text(if source.paused {
                                "Paused: existing indexed content remains searchable"
                            } else {
                                "Search this source"
                            })
                            .changed()
                        {
                            self.source_selection.set(
                                std::slice::from_ref(&source.id),
                                enabled,
                                sources,
                            );
                        }
                    }
                });
            }
            if sources.is_empty() {
                ui.weak("No sources configured");
            }
        });
        multiselect(
            ui,
            "content_filters",
            format!(
                "Content: {}",
                pair(self.filters.text, self.filters.images, "Text", "Image")
            ),
            |ui| {
                ui.checkbox(&mut self.filters.text, "Text").on_hover_text(
                    "Text items from any selected source, including Slack, Jira and Markdown",
                );
                ui.checkbox(&mut self.filters.images, "Image")
                    .on_hover_text(
                        "Images and their indexed text and descriptions from selected sources",
                    );
            },
        );
        multiselect(
            ui,
            "matching_filters",
            format!(
                "Matching: {}",
                pair(
                    self.filters.text_matches,
                    self.filters.semantic,
                    "Keyword",
                    "Semantic"
                )
            ),
            |ui| {
                ui.checkbox(&mut self.filters.text_matches, "Keyword")
                    .on_hover_text("Name/title and literal content matches");
                let available = self.config.semantic_enabled && self.semantic_ready;
                ui.add_enabled(
                    available,
                    egui::Checkbox::new(&mut self.filters.semantic, "Semantic"),
                );
                if !available {
                    ui.weak(if !self.config.semantic_enabled {
                        "Enable semantic search in Settings → Search."
                    } else if self.semantic_status.is_empty() {
                        "The local semantic model is not ready yet."
                    } else {
                        &self.semantic_status
                    });
                }
            },
        );
        egui::ComboBox::from_id_salt("result_sort")
            .selected_text(format!("Sort: {}", self.filters.sort.label()))
            .show_ui(ui, |ui| {
                use good_search::sorting::SortMode;
                for mode in [SortMode::PreferNewer, SortMode::NewestFirst, SortMode::OldestFirst] {
                    ui.selectable_value(&mut self.filters.sort,mode,mode.label());
                }
            }).response.on_hover_text("Dates: file modified time, Slack message posted time, or Jira ticket updated time. Attachments use their parent message/ticket date. Unknown dates sort last.");
        if ui.small_button("Reset filters").clicked() {
            self.filters = Default::default();
            self.source_selection = Default::default();
        }
        before != (self.filters.clone(), self.source_selection.clone())
    }
}
