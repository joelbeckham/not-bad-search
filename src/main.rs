#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod appearance;
mod inspector_ui;
mod platform;
use anyhow::{Context, Result};
use eframe::egui;
use good_search::{open_db, open_reader, open_writer, reconcile, search, Config, Hit};
use notify::Watcher;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

enum Event {
    Results(u64, String, Vec<Hit>, Duration),
    SearchFailed(u64, String),
    Status(String),
    Error(String),
    ImageStatus(String, bool),
}
fn trace(path: &std::path::Path, message: &str) {
    if std::env::var_os("GOOD_SEARCH_WORKER_TRACE").is_some() {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(file, "{message}");
        }
    }
}
struct App {
    inspector: Option<inspector_ui::Inspector>,
    _instance: platform::Instance,
    submitted_query: String,
    searching: bool,
    completed_generation: u64,
    query_error: Option<String>,
    trace_results: bool,
    query: String,
    hits: Vec<Hit>,
    selected: usize,
    generation: u64,
    elapsed: Duration,
    pending: Arc<Mutex<Option<(u64, String)>>>,
    wake: mpsc::SyncSender<()>,
    events: mpsc::Receiver<Event>,
    config: Config,
    config_path: PathBuf,
    folders: String,
    exclusions: String,
    content_folders: Vec<PathBuf>,
    content_limit_mib: u64,
    image_status: String,
    image_wake: mpsc::SyncSender<bool>,
    api_key_input: String,
    key_saved: bool,
    settings: bool,
    status: String,
    error: String,
    index_wake: mpsc::SyncSender<()>,
    shared_config: Arc<Mutex<Config>>,
    _hotkey: Option<platform::Hotkey>,
    focus: bool,
}

fn start_image_worker(
    path: PathBuf,
    config: Arc<Mutex<Config>>,
    events: mpsc::Sender<Event>,
    ctx: egui::Context,
) -> mpsc::SyncSender<bool> {
    let (wake, rx) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let run = || -> Result<()> {
            platform::background_priority()?;
            let mut db = open_writer(&path)?;
            good_search::images::recover(&db)?;
            let client = good_search::images::OpenAi::new()?;
            let mut previous = String::new();
            loop {
                let current = config.lock().unwrap().clone();
                let key = good_search::credentials::load()?.is_some();
                let changed = if key {
                    good_search::images::process_one(&mut db, &client, || {
                        config.lock().unwrap().clone()
                    })?
                } else {
                    false
                };
                let prefix = if current.images_paused {
                    "Paused. "
                } else if !key {
                    "Add an API key to process images. "
                } else {
                    ""
                };
                let status = format!("{prefix}{}", good_search::images::summary(&db)?);
                if changed || status != previous {
                    previous = status.clone();
                    let _ = events.send(Event::ImageStatus(status, changed));
                    ctx.request_repaint();
                }
                if changed {
                    continue;
                }
                match rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(true) => good_search::images::retry_failed(&db)?,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    _ => {}
                }
            }
            Ok(())
        };
        if let Err(e) = run() {
            let _ = events.send(Event::ImageStatus(
                format!("Image worker stopped: {e}"),
                false,
            ));
            ctx.request_repaint();
        }
    });
    wake
}

fn main() {
    if let Err(e) = run() {
        // Also show startup failures in release builds without a console.
        let message: Vec<u16> = format!("{e:#}\0").encode_utf16().collect();
        let title: Vec<u16> = "Good Search\0".encode_utf16().collect();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                0x10,
            );
        }
    }
}
fn run() -> Result<()> {
    let data = std::env::var_os("GOOD_SEARCH_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("GoodSearch")
        });
    if !data.is_absolute() || data.to_string_lossy().starts_with("\\\\") {
        anyhow::bail!("App data must be an absolute Windows-local path");
    }
    std::fs::create_dir_all(&data)?;
    let Some(instance) = platform::Instance::claim(&data)? else {
        return Ok(());
    };
    let config_path = data.join("config.toml");
    let config: Config = if config_path.exists() {
        toml::from_str(&std::fs::read_to_string(&config_path)?)?
    } else {
        Config::default()
    };
    config.validate()?;
    let db_path = data.join("index.sqlite");
    open_db(&db_path)?;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("Good Search")
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size([780., 500.])
            .with_min_inner_size([600., 360.]),
        ..Default::default()
    };
    eframe::run_native("Good Search",options,Box::new(move |cc| {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (tx,events)=mpsc::channel();
        let pending=Arc::new(Mutex::new(None::<(u64,String)>));
        let (wake,rx)=mpsc::sync_channel(1);
        let p=pending.clone(); let sender=tx.clone(); let context=cc.egui_ctx.clone(); let path=db_path.clone();
        std::thread::spawn(move || {
            let db=match open_reader(&path) {Ok(db)=>db,Err(e)=>{let _=sender.send(Event::Error(e.to_string()));context.request_repaint();return}};
            while rx.recv().is_ok() {
                let job=p.lock().unwrap().take();
                if let Some((id,q))=job {
                    let start=Instant::now();
                    let event=match search(&db,&q,100) {Ok(hits)=>Event::Results(id,q.clone(),hits,start.elapsed()),Err(e)=>Event::SearchFailed(id,e.to_string())};
                    let _=sender.send(event); context.request_repaint();
                    trace(&path.with_file_name("worker-trace.log"),&format!("completed id={id} query={q:?} elapsed={:?}",start.elapsed()));
                }
            }
        });
        let shared_config=Arc::new(Mutex::new(config.clone()));
        let image_wake=start_image_worker(db_path.clone(),shared_config.clone(),tx.clone(),cc.egui_ctx.clone());
        let image_notify=image_wake.clone();
        let (index_wake,index_rx)=mpsc::sync_channel(1);
        let cfg=shared_config.clone(); let context=cc.egui_ctx.clone(); let notify_tx=index_wake.clone();
        std::thread::spawn(move || {
            if let Err(e)=platform::background_priority() {let _=tx.send(Event::Error(e.to_string()));}
            let mut db=match open_writer(&db_path) {Ok(db)=>db,Err(e)=>{let _=tx.send(Event::Error(e.to_string()));context.request_repaint();return}};
            let mut watched=Vec::<PathBuf>::new();
            let ignored_data=db_path.parent().unwrap().to_path_buf();
            let mut watcher=notify::recommended_watcher(move |event:notify::Result<notify::Event>| {
                if matches!(event,Ok(ref e) if !e.paths.is_empty() && e.paths.iter().all(|p|p.starts_with(&ignored_data))) {return;}
                if !matches!(event,Ok(ref e) if matches!(e.kind,notify::EventKind::Access(_))) {let _=notify_tx.try_send(());}
            }).ok();
            loop {
                let config=cfg.lock().unwrap().clone();
                if let Some(watcher)=watcher.as_mut() {
                    for root in &watched {let _=watcher.unwatch(root);}
                    watched.clear();
                    for root in &config.folders {
                        match watcher.watch(root,notify::RecursiveMode::Recursive) {
                            Ok(())=>watched.push(root.clone()),
                            Err(e)=>{let _=tx.send(Event::Error(format!("Watcher: {e}. Periodic reconciliation remains active.")));}
                        }
                    }
                }
                let start=Instant::now();
                let _=tx.send(Event::Status("Indexing selected folders…".into())); context.request_repaint();
                match reconcile(&mut db,&config) {
                    Ok(errors)=>{
                        let count:i64=db.query_row("SELECT count(*) FROM files",[],|r|r.get(0)).unwrap_or(0);
                        let _=tx.send(Event::Status(format!("{count} files · indexed in {:.0} ms",start.elapsed().as_secs_f64()*1000.)));
                        if !errors.is_empty() { let _=tx.send(Event::Error(errors.join("\n"))); }
                    },
                    Err(e)=>{let _=tx.send(Event::Error(e.to_string()));}
                }
                context.request_repaint();
                let _=image_notify.try_send(false);
                match index_rx.recv_timeout(Duration::from_secs(config.reconcile_seconds)) {
                    Err(mpsc::RecvTimeoutError::Disconnected)=>break,
                    _=>{std::thread::sleep(Duration::from_millis(250)); while index_rx.try_recv().is_ok() {}}
                }
            }
        });
        let window=match cc.window_handle()?.as_raw() {RawWindowHandle::Win32(h)=>h.hwnd.get(),_=>return Err("Windows window handle unavailable".into())};
        instance.attach(window)?;
        let (hotkey,error)=match platform::Hotkey::register(cc.egui_ctx.clone(),window) {Ok(h)=>(Some(h),String::new()),Err(e)=>(None,e.to_string())};
        Ok(Box::new(App {inspector:None,_instance:instance,completed_generation:0,submitted_query:String::new(),searching:false,query_error:None,trace_results:false,folders:config.folders.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>().join("\n"), exclusions:config.exclusions.join("\n"), image_status:String::new(),image_wake,api_key_input:String::new(),key_saved:good_search::credentials::load().ok().flatten().is_some(),settings:config.folders.is_empty(), content_folders:config.content_folders.clone(), content_limit_mib:config.max_content_bytes.div_ceil(1024*1024), config,config_path,query:String::new(),hits:vec![],selected:0,generation:0,elapsed:Duration::ZERO,pending,wake,events,status:"Starting…".into(),error,index_wake,shared_config,_hotkey:hotkey,focus:true}))
    })).map_err(|e|anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
impl App {
    fn back(&mut self) {
        if self.settings {
            self.settings = false;
        } else {
            self.inspector = None;
        }
        self.focus = !self.settings;
    }

    fn submit(&mut self) {
        self.generation += 1;
        self.submitted_query.clone_from(&self.query);
        self.query_error = None;
        self.trace_results = false;
        self.searching = !self.query.trim().is_empty();
        if !self.searching {
            self.completed_generation = self.generation;
            // Clear immediately; invalidate any older in-flight result without querying SQLite.
            self.pending.lock().unwrap().take();
            self.hits.clear();
            self.elapsed = Duration::ZERO;
            self.selected = 0;
            self.trace_results = true;
            return;
        }
        trace(
            &self.config_path.with_file_name("worker-trace.log"),
            &format!("submitted id={} query={:?}", self.generation, self.query),
        );
        *self.pending.lock().unwrap() = Some((self.generation, self.query.clone()));
        if matches!(
            self.wake.try_send(()),
            Err(mpsc::TrySendError::Disconnected(_))
        ) {
            self.searching = false;
            self.completed_generation = self.generation;
            self.query_error = Some("Search worker stopped. Restart Good Search.".into());
        }
    }
    fn save(&mut self) -> Result<()> {
        let config = Config {
            folders: self
                .folders
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .collect(),
            exclusions: self
                .exclusions
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            content_folders: self.content_folders.clone(),
            max_content_bytes: self.content_limit_mib * 1024 * 1024,
            ..self.config.clone()
        };
        config.validate()?;
        std::fs::write(&self.config_path, toml::to_string_pretty(&config)?)
            .context("Saving settings")?;
        *self.shared_config.lock().unwrap() = config.clone();
        self.config = config;
        self.hits.clear();
        self.generation += 1;
        self.searching = !self.query.trim().is_empty();
        self.error.clear();
        let _ = self.index_wake.try_send(());
        let _ = self.image_wake.try_send(false);
        Ok(())
    }
}
impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Results(id, query, hits, time)
                    if id == self.generation && query == self.query =>
                {
                    trace(
                        &self.config_path.with_file_name("worker-trace.log"),
                        &format!("accepted id={id} hits={}", hits.len()),
                    );
                    self.hits = hits;
                    self.searching = false;
                    self.completed_generation = id;
                    self.elapsed = time;
                    self.selected = 0;
                    self.trace_results = true;
                }
                Event::Status(s) => {
                    let done = !s.starts_with("Indexing");
                    self.status = s;
                    if done {
                        self.submit();
                    }
                }
                Event::Error(e) => self.error = e,
                Event::ImageStatus(status, changed) => {
                    self.image_status = status;
                    if changed {
                        self.submit();
                    }
                }
                Event::SearchFailed(id, error) if id == self.generation => {
                    self.completed_generation = id;
                    self.searching = false;
                    self.query_error = Some(error);
                    self.hits.clear();
                }
                _ => {}
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && self._hotkey.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.focus = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
            self.settings = true;
            self.focus = false;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.inspector.is_some() || self.settings {
                self.back();
            } else if !self.query.is_empty() {
                self.query.clear();
                self.submit();
                self.focus = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.focus = true;
            }
        }
        self.draw_window(ui);

        self.searching =
            !self.query.trim().is_empty() && self.completed_generation != self.generation;
        // A fast worker can finish after logic() drains the channel but during the
        // same UI pass. Its repaint can be coalesced with that pass. Keep a bounded
        // follow-up scheduled until the matching response is consumed. No idle polling.
        if self.searching {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.trace_results && std::env::var_os("GOOD_SEARCH_TRACE").is_some() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.config_path.with_file_name("ui-trace.log"))
            {
                let _ = writeln!(
                    f,
                    "query={:?} hits={} db_ms={:.3} error={:?} generation={}",
                    self.query,
                    self.hits.len(),
                    self.elapsed.as_secs_f64() * 1000.,
                    self.error,
                    self.generation
                );
            }
        }
        self.trace_results = false;
    }
}
