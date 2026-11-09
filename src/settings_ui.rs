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
    pub remote_status: String,
    endpoint: String,
    api_key: String,
    connection_status: String,
    connection_rx: Option<mpsc::Receiver<String>>,
    tab: Tab,
    pub selected: Option<String>,
    stats: Vec<Status>,
    receiver: Option<mpsc::Receiver<Result<Vec<Status>, String>>>,
    checked: Option<Instant>,
    error: String,
}
impl App {
    fn draw_notion_source(&mut self,ui:&mut egui::Ui,index:usize) {
        let saved_paused=self.shared_config.lock().unwrap().sources.iter().find(|s|s.id==self.config.sources[index].id).map(|s|s.paused);
        let source=&mut self.config.sources[index];ui.heading(&source.name);ui.weak("Notion through Bifrost · read-only");
        source_status_card(ui,self.source_settings.stats.iter().find(|s|s.id==source.id),&mut source.paused,saved_paused);
        ui.label("Name");ui.text_edit_singleline(&mut source.name);
        let settings=source.notion.get_or_insert_with(Default::default);
        ui.label("Root page URLs or IDs — one per line");ui.add(egui::TextEdit::multiline(&mut settings.pages).desired_rows(3).desired_width(f32::INFINITY));
        ui.label("Teamspace names or IDs — one per line");ui.add(egui::TextEdit::multiline(&mut settings.teamspaces).desired_rows(2).desired_width(f32::INFINITY));
        ui.weak("Includes child pages and database pages under each root, and accessible pages in each teamspace. Duplicate pages within a source are indexed once.");
        ui.horizontal(|ui|{ui.label("Poll every");ui.add(egui::DragValue::new(&mut settings.poll_seconds).range(30..=86400).suffix(" seconds"));});
        ui.checkbox(&mut source.images,"Index images: text and descriptions");
        ui.weak("Uses the image settings in Processing. Supports uploaded or public HTTPS PNG, JPEG and WebP images up to 10 MiB.");
        ui.weak("Databases are enumerated directly using the query tool. Teamspace discovery still uses search and reports incomplete if a single creation day reaches its 50-result cap; use page roots in that case.");
        if ui.button("Restart traversal on next save").clicked(){settings.reload_generation=settings.reload_generation.saturating_add(1);}
        ui.weak("Deleted or inaccessible pages retain their last local copy until the source is removed. Image download or access errors are shown in sync status.");
        ui.horizontal(|ui|{
            if ui.button("Sync enabled sources now").clicked(){let _=self.remote_wake.try_send(());self.source_settings.checked=None;}
            if ui.button("Remove source").clicked(){self.config.sources.remove(index);self.source_settings.selected=None;}
        });
        ui.weak("Uses the shared Bifrost connection in Processing. Save changes to apply.");
    }
    fn draw_gmail_source(&mut self, ui:&mut egui::Ui,index:usize) {
        let saved_paused=self.shared_config.lock().unwrap().sources.iter().find(|s|s.id==self.config.sources[index].id).map(|s|s.paused);
        let source=&mut self.config.sources[index];
        ui.heading(&source.name); ui.weak("Gmail through Bifrost · read-only");
        source_status_card(ui,self.source_settings.stats.iter().find(|s|s.id==source.id),&mut source.paused,saved_paused);
        ui.label("Name"); ui.text_edit_singleline(&mut source.name);
        let settings=source.gmail.get_or_insert_with(Default::default);
        ui.label("Gmail search query"); ui.text_edit_singleline(&mut settings.query);
        ui.weak("Examples: in:inbox, label:engineering, from:person@example.com. Leave blank for all mail except spam and trash.");
        ui.label("History start date (YYYY-MM-DD)");ui.text_edit_singleline(&mut settings.since);
        ui.weak("Imports full conversations with messages matching the query and start date, including earlier replies in those conversations.");
        ui.horizontal(|ui|{ui.label("Poll every");ui.add(egui::DragValue::new(&mut settings.poll_seconds).range(30..=86400).suffix(" seconds"));});
        ui.weak("Rechecks matching conversations each cycle. Unchanged text is not re-embedded. Narrow the query for large mailboxes.");
        if ui.button("Restart import on next save").clicked(){settings.reload_generation=settings.reload_generation.saturating_add(1);}
        ui.weak("Indexes message text, senders, recipients and dates. Attachment contents are not indexed. Reading does not mark mail as read.");
        ui.weak("Messages that are deleted or leave the query remain in the local index until this source is removed.");
        ui.horizontal(|ui|{
            if ui.button("Sync enabled sources now").clicked(){let _=self.remote_wake.try_send(());self.source_settings.checked=None;}
            if ui.button("Remove source").clicked(){self.config.sources.remove(index);self.source_settings.selected=None;}
        });
        ui.weak("Uses the shared Bifrost connection in Processing. Save changes to apply.");
    }
    fn draw_slack_source(&mut self, ui: &mut egui::Ui, index: usize) {
        let saved_paused=self.shared_config.lock().unwrap().sources.iter().find(|s|s.id==self.config.sources[index].id).map(|s|s.paused);
        let source=&mut self.config.sources[index];
        ui.heading(&source.name);
        ui.weak("Slack through Bifrost · read-only");
        source_status_card(ui,self.source_settings.stats.iter().find(|s|s.id==source.id),&mut source.paused,saved_paused);
        ui.label("Name"); ui.text_edit_singleline(&mut source.name);
        let settings=source.slack.get_or_insert_with(Default::default);
        ui.label("Workspace URL"); ui.text_edit_singleline(&mut settings.workspace);
        ui.label("Channel ID"); ui.text_edit_singleline(&mut settings.channel);
        ui.weak("Copy the channel ID from Slack channel details. Add a source for each channel.");
        ui.label("History start date (YYYY-MM-DD, UTC)"); ui.text_edit_singleline(&mut settings.since);
        ui.weak("The initial import includes messages posted on or after this date and their thread replies.");
        ui.horizontal(|ui| {
            ui.label("Update cutoff: recheck the last");
            ui.add(egui::DragValue::new(&mut settings.update_lookback_days).range(1..=36500).suffix(" days"));
        });
        if let Ok(cutoff)=good_search::slack::scan_start(settings,Some(chrono::Utc::now().timestamp()),chrono::Utc::now().timestamp()) {
            if let Some(date)=chrono::DateTime::from_timestamp(cutoff,0) {
                ui.weak(format!("Current update cutoff: {} UTC. Older indexed messages remain searchable.",date.format("%Y-%m-%d")));
            }
        }
        ui.weak("After the initial import, checks edits and replies only for parent messages posted within this window. A longer offline gap is caught up automatically. Changes apply to the next sync cycle.");
        if ui.button("Recheck full history on next save").clicked() {
            settings.reload_generation=settings.reload_generation.saturating_add(1);
        }
        ui.horizontal(|ui| {ui.label("Poll every"); ui.add(egui::DragValue::new(&mut settings.poll_seconds).range(30..=86400).suffix(" seconds"));});
        ui.checkbox(&mut source.images,"Index shared images: text and descriptions");
        ui.weak("Images use the OpenAI settings in Processing. PNG, JPEG and WebP up to 10 MiB are supported.");
        ui.weak("Replies to parents older than the update cutoff are not checked again unless you recheck full history. Deleted messages and lost access retain the last local copy until the source is removed.");
        ui.horizontal(|ui| {
            if ui.button("Sync enabled sources now").clicked() {let _=self.remote_wake.try_send(()); self.source_settings.checked=None;}
            if ui.button("Remove source").clicked() {self.config.sources.remove(index);self.source_settings.selected=None;}
        });
        ui.weak("Uses the shared Bifrost connection in Processing. Save changes to apply.");
    }
    fn draw_jira_source(&mut self, ui: &mut egui::Ui, index: usize) {
        let saved_paused = self
            .shared_config
            .lock()
            .unwrap()
            .sources
            .iter()
            .find(|s| s.id == self.config.sources[index].id)
            .map(|s| s.paused);
        let source = &mut self.config.sources[index];
        ui.heading(&source.name);
        ui.label("Jira through Bifrost · ordinary account · read-only");
        source_status_card(
            ui,
            self.source_settings
                .stats
                .iter()
                .find(|s| s.id == source.id),
            &mut source.paused,
            saved_paused,
        );
        ui.label("Name");
        ui.text_edit_singleline(&mut source.name);
        let settings = source.jira.get_or_insert_with(Default::default);
        ui.label("Jira site");
        ui.add(egui::TextEdit::singleline(&mut settings.site).desired_width(f32::INFINITY));
        ui.label("Cloud ID");
        ui.text_edit_singleline(&mut settings.cloud_id);
        ui.label("Project key");
        ui.text_edit_singleline(&mut settings.project);
        ui.label("JQL scope");
        ui.add(
            egui::TextEdit::multiline(&mut settings.jql)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.weak("Start with a few keys, e.g. key in (CORE-123, CORE-456). The project constraint is always applied; sync orders by updated time.");
        ui.horizontal(|ui| {
            ui.label("Poll every");
            ui.add(
                egui::DragValue::new(&mut settings.poll_seconds)
                    .range(30..=86400)
                    .suffix(" seconds"),
            );
            ui.label("Issues per page");
            ui.add(egui::DragValue::new(&mut settings.page_size).range(1..=100));
        });

        if ui.button("Reload all tickets on next save").on_hover_text("Re-fetch the full saved JQL scope. Existing results remain searchable until refreshed.").clicked() {
            settings.reload_generation=settings.reload_generation.saturating_add(1);
        }
        ui.weak("Reload applies when you save changes. Paused sources reload when resumed.");
        ui.label("Indexes summaries, Markdown descriptions (HTML when needed), and supported image attachments. Search and previews use the local index.");
        ui.weak("Missing issues are retained: deletion/access-change feeds are unverified. Inspect an issue's local sync time and source freshness before relying on cached content.");
        ui.label(&self.source_settings.remote_status);
        ui.horizontal(|ui| {
            if ui.button("Sync enabled Jira sources now").clicked() {
                let _ = self.remote_wake.try_send(());
                self.source_settings.checked = None;
            }
            if ui.button("Remove source").clicked() {
                self.config.sources.remove(index);
                self.source_settings.selected = None;
            }
        });
        ui.weak("Connection credentials are shared in Processing → Bifrost. Manual sync uses saved settings.");
    }
    fn draw_bifrost_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bifrost MCP connection");
        ui.weak("Shared connection for remote sources. Endpoint and API key are stored together in Windows Credential Manager, never config.toml.");
        ui.label("MCP endpoint");
        ui.add(
            egui::TextEdit::singleline(&mut self.source_settings.endpoint)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.label("API key");
        ui.add(
            egui::TextEdit::singleline(&mut self.source_settings.api_key)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if ui.button("Store connection").clicked() {
                let connection = good_search::bifrost::Connection {
                    endpoint: self.source_settings.endpoint.trim().into(),
                    api_key: self.source_settings.api_key.trim().into(),
                };
                match connection.save() {
                    Ok(()) => {
                        self.source_settings.endpoint.clear();
                        self.source_settings.api_key.clear();
                        self.source_settings.connection_status = "Connection saved".into();
                    }
                    Err(e) => self.source_settings.connection_status = e.to_string(),
                }
            }
            if ui
                .add_enabled(
                    self.source_settings.connection_rx.is_none(),
                    egui::Button::new("Test saved connection"),
                )
                .clicked()
            {
                let (tx, rx) = mpsc::channel();
                self.source_settings.connection_rx = Some(rx);
                self.source_settings.connection_status = "Checking ordinary Jira account…".into();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = (|| -> anyhow::Result<String> {
                        let mut m = good_search::bifrost::Mcp::connect(
                            good_search::bifrost::Connection::load()?,
                        )?;
                        let user = m.call("jira_atlassianUserInfo", serde_json::json!({}))?;
                        Ok(format!(
                            "Connected · {}",
                            user["displayName"]
                                .as_str()
                                .or(user["name"].as_str())
                                .unwrap_or("ordinary Jira account")
                        ))
                    })();
                    let _ = tx.send(result.unwrap_or_else(|e| format!("Connection failed: {e:#}")));
                    ctx.request_repaint();
                });
            }
            if ui.button("Remove connection").clicked() {
                match good_search::credentials::remove_named("Bifrost/default") {
                    Ok(()) => self.source_settings.connection_status = "Connection removed".into(),
                    Err(e) => self.source_settings.connection_status = e.to_string(),
                }
            }
        });
        if let Some(rx) = &self.source_settings.connection_rx {
            if let Ok(message) = rx.try_recv() {
                self.source_settings.connection_status = message;
                self.source_settings.connection_rx = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        ui.label(&self.source_settings.connection_status);
        ui.separator();
    }
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
            let back_label = if self.source_settings.selected.is_some() {
                "Back to sources"
            } else if self.inspector.is_some() {
                "Back to indexed data"
            } else if self.preview.is_some() {
                "Back to preview"
            } else {
                "Back to search"
            };
            if action_icon(ui, false, back_label).clicked() {
                self.back();
            }
            if action_icon(ui, true, "Save changes").clicked() {
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
            let Some(index) = self.config.sources.iter().position(|s| s.id == id) else {
                self.source_settings.selected = None;
                return;
            };
            if self.config.sources[index].kind == good_search::sources::SourceKind::Notion { self.draw_notion_source(ui,index); return; }
            if self.config.sources[index].kind == good_search::sources::SourceKind::Gmail { self.draw_gmail_source(ui,index); return; }
            if self.config.sources[index].kind == good_search::sources::SourceKind::Slack {
                self.draw_slack_source(ui,index); return;
            }
            if !self.config.sources[index].is_folder() {
                self.draw_jira_source(ui, index);
                return;
            }
            let saved_paused = self
                .shared_config
                .lock()
                .unwrap()
                .sources
                .iter()
                .find(|s| s.id == self.config.sources[index].id)
                .map(|s| s.paused);
            let source = &mut self.config.sources[index];
            ui.heading(&source.name);
            ui.weak("Folder source · Windows and WSL paths supported");
            source_status_card(
                ui,
                self.source_settings.stats.iter().find(|s| s.id == id),
                &mut source.paused,
                saved_paused,
            );
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
            if ui.button("+ Add Jira").clicked() {
                let source = Source::jira_source();
                self.source_settings.selected = Some(source.id.clone());
                self.config.sources.push(source);
            }
            if ui.button("+ Add Notion").clicked() {let source=Source::notion_source();self.source_settings.selected=Some(source.id.clone());self.config.sources.push(source);}
            if ui.button("+ Add Gmail").clicked() { let source=Source::gmail_source(); self.source_settings.selected=Some(source.id.clone()); self.config.sources.push(source); }
            if ui.button("+ Add Slack").clicked() {
                let source=Source::slack_source(); self.source_settings.selected=Some(source.id.clone()); self.config.sources.push(source);
            }
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
        ui.weak(
            "Each source has its own scope and indexing options. Counts are items, not chunks.",
        );
        if !self.source_settings.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.source_settings.error);
        }
        let saved_config = self.shared_config.lock().unwrap().clone();
        let mut toggle_source=None;
        for source in &mut self.config.sources {
            let paused = saved_config
                .sources
                .iter()
                .find(|s| s.id == source.id)
                .map_or(source.paused, |s| s.paused);
            egui::Frame::new()
                .fill(Color32::from_rgb(34, 36, 39))
                .corner_radius(6)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width((ui.available_width() - 4.).max(0.));
                    ui.horizontal(|ui| {

                        let (uri,bytes)=match source.kind {
                            good_search::sources::SourceKind::Folder=>good_search::file_icons::folder_icon(),
                            good_search::sources::SourceKind::Jira=>good_search::file_icons::jira_icon(),
                            good_search::sources::SourceKind::Slack=>good_search::file_icons::slack_icon(),
                            good_search::sources::SourceKind::Gmail=>good_search::file_icons::gmail_icon(),
                            good_search::sources::SourceKind::Notion=>good_search::file_icons::notion_icon(),
                        };
                        ui.add(egui::Image::from_bytes(uri,bytes).fit_to_exact_size(egui::vec2(22.,22.))).on_hover_text(source.label());
                        let title=RichText::new(&source.name).size(17.);
                        let title_width=ui.painter().layout_no_wrap(source.name.clone(),egui::FontId::proportional(17.),ui.visuals().text_color()).size().x.min((ui.available_width()-76.).max(30.));
                        ui.add_sized(egui::vec2(title_width,28.),egui::Label::new(title).truncate()).on_hover_text(&source.name);
                        let action=if paused {"Resume processing"} else {"Pause processing"};
                        let saved=saved_config.sources.iter().any(|s|s.id==source.id);
                        let toggle=ui.add_enabled(saved,egui::Button::image(status_icon(paused)).min_size(egui::vec2(28.,28.)))
                            .on_hover_text(format!("{action} immediately"))
                            .on_disabled_hover_text("Save this new source before starting processing.");
                        toggle.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,true,action));
                        if toggle.clicked() { toggle_source=Some((source.id.clone(),!paused)); }
                        if edit_source_icon(ui,&format!("Edit {}",source.name)).clicked() {
                            self.source_settings.selected = Some(source.id.clone());
                        }

                    });
                    if source.paused!=paused {
                        ui.weak(if source.paused {"Pending: pause processing. Save to apply."} else {"Pending: resume processing. Save to apply."});
                    }
                    ui.label(if let Some(notion)=&source.notion { format!("{} page roots · {} teamspaces",notion.pages.lines().filter(|s|!s.trim().is_empty()).count(),notion.teamspaces.lines().filter(|s|!s.trim().is_empty()).count()) } else if let Some(gmail)=&source.gmail { format!("{} Â· since {}",gmail.query,gmail.since) } else if let Some(slack)=&source.slack {
                        format!("{} · {}",slack.workspace,slack.channel)
                    } else if source.is_folder() {
                        source.path.to_string_lossy().into_owned()
                    } else {
                        source
                            .jira
                            .as_ref()
                            .map(|j| format!("{} · {}", j.site, j.project))
                            .unwrap_or_default()
                    });
                    if let Some(stats) = self
                        .source_settings
                        .stats
                        .iter()
                        .find(|s| s.id == source.id)
                    {
                        draw_status(ui, stats, paused, false);
                    } else {
                        ui.weak("Not indexed yet · save changes to begin");
                    }
                });
            ui.add_space(8.);
        }
        if let Some((id,paused))=toggle_source {
            match self.set_source_paused_now(&id,paused) {
                Ok(())=>self.source_settings.checked=None,
                Err(error)=>self.error=format!("Could not change source processing: {error:#}"),
            }
        }
        if self.config.sources.is_empty() {
            ui.label("Add a folder to start searching your files.");
        }

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
        self.draw_bifrost_settings(ui);
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
fn action_icon(ui: &mut egui::Ui, save: bool, label: &str) -> egui::Response {
    let response = ui
        .add(egui::Button::new("").min_size(egui::vec2(32., 28.)))
        .on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let c = response.rect.center();
    let stroke = egui::Stroke::new(1.7, ui.visuals().text_color());
    let point = |x, y| c + egui::vec2(x, y);
    let line = |a, b| {
        ui.painter().line_segment([a, b], stroke);
    };
    if save {
        let points = vec![
            point(-8., -8.),
            point(5., -8.),
            point(8., -5.),
            point(8., 8.),
            point(-8., 8.),
            point(-8., -8.),
        ];
        ui.painter().add(egui::Shape::line(points, stroke));
        for (a, b) in [
            ((-4., -8.), (-4., -2.)),
            ((-4., -2.), (4., -2.)),
            ((4., -2.), (4., -8.)),
            ((-4., 8.), (-4., 3.)),
            ((-4., 3.), (4., 3.)),
            ((4., 3.), (4., 8.)),
        ] {
            line(point(a.0, a.1), point(b.0, b.1));
        }
    } else {
        line(point(8., 0.), point(-8., 0.));
        line(point(-8., 0.), point(-2., -6.));
        line(point(-8., 0.), point(-2., 6.));
    }
    response
}

fn status_icon(paused: bool) -> egui::Image<'static> {
    let (uri, svg): (&str, &[u8]) = if paused {
        ("bytes://status/paused.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><path d="M6 4v12M14 4v12" stroke="#f0be64" stroke-width="3"/></svg>"##)
    } else {
        ("bytes://status/enabled.svg", br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><path d="M5 3l12 7-12 7z" fill="#78c3cc"/></svg>"##)
    };
    egui::Image::from_bytes(uri, svg).fit_to_exact_size(egui::vec2(20., 20.))
}

fn source_status_card(
    ui: &mut egui::Ui,
    stats: Option<&Status>,
    draft_paused: &mut bool,
    saved_paused: Option<bool>,
) {
    let paused = saved_paused.unwrap_or(*draft_paused);
    let color = if paused {
        Color32::from_rgb(240, 190, 100)
    } else {
        Color32::from_rgb(120, 195, 204)
    };
    egui::Frame::new()
        .fill(Color32::from_rgb(34, 36, 39))
        .corner_radius(6)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 4.).max(0.));
            let label = if *draft_paused {
                "Paused · Resume"
            } else {
                "Enabled · Pause"
            };
            let response = ui
                .add(
                    egui::Button::image_and_text(
                        status_icon(*draft_paused),
                        RichText::new(label).size(17.).strong(),
                    )
                    .min_size(egui::vec2(190., 36.)),
                )
                .on_hover_text("Toggle background processing. Save changes to apply.");
            if response.clicked() {
                *draft_paused = !*draft_paused;
            }
            if paused {
                ui.label("This source will not import updates while paused.");
            }
            if saved_paused.is_some_and(|saved| saved != *draft_paused) {
                ui.colored_label(
                    color,
                    if *draft_paused {
                        "Pending change: pause processing. Save to apply."
                    } else {
                        "Pending change: resume processing. Save to apply."
                    },
                );
            }
            if let Some(stats) = stats {
                draw_status(ui, stats, paused, true);
            } else {
                ui.weak(if saved_paused.is_none() {
                    "Save this source to begin."
                } else {
                    "Loading source status…"
                });
            }
        });
    ui.add_space(8.);
}

fn draw_status(ui: &mut egui::Ui, s: &Status, paused: bool, detail: bool) {
    if let Some(status) = &s.remote_status {
        ui.label(format!(
            "{}{} items indexed locally",
            if paused { "Paused · " } else { "" },
            s.items
        ));
        ui.label(status);
        ui.weak(format!(
            "{} issues ready for semantic search · {} queued",
            s.embedding_ready, s.embedding_queued
        ));
        return;
    }
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

fn edit_source_icon(ui:&mut egui::Ui,label:&str)->egui::Response {
    let image=egui::Image::from_bytes("bytes://settings/edit-source.svg",br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="m15 5 4 4M4 20l5-1L20 8a2.8 2.8 0 0 0-4-4L5 15l-1 5Z" fill="none" stroke="#cdd6f4" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##).fit_to_exact_size(egui::vec2(18.,18.));
    let response=ui.add(egui::Button::image(image).min_size(egui::vec2(28.,28.))).on_hover_text(label);
    response.widget_info(||egui::WidgetInfo::labeled(egui::Role::Button,true,label));
    response
}