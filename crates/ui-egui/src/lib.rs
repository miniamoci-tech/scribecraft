//! ScribeCraft's desktop interface (layer L6).
//!
//! One window: the dictation job list on the left, the open dictation on the right with its
//! transport bar, speed / volume / channel controls, notes and bookmarks.
//!
//! Keyboard (while the window has focus; system-wide hotkeys and foot pedals are M3):
//! F9 play/stop · F7 rewind (hold) · F8 fast-forward (hold) · F5 slow · F6 fast ·
//! F10 copy time stamp · Ctrl+B bookmark · Ctrl+S save.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use scribecraft_engine::transport::{ChannelMode, MAX_SPEED, MIN_SPEED, SpeedPreset, format_timestamp};
use scribecraft_engine::{Priority, Session, Settings, State, Status, Transport, lock_transport, media, scan_folder};
use scribecraft_platform::AudioOutput;

/// Opens the main window; `files` are recordings to add and open (from the command line or
/// the operating system's "Open with").
pub fn run(files: Vec<PathBuf>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ScribeCraft").with_inner_size([1100.0, 700.0]).with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("ScribeCraft", options, Box::new(move |_cc| Ok(Box::new(App::new(files))))).map_err(|e| e.to_string())
}

struct App {
    session: Session,
    settings_path: Option<PathBuf>,
    output: Option<AudioOutput>,
    transport: Option<Arc<Mutex<Transport>>>,
    message: Option<(String, bool)>,
    bookmark_label: String,
    scrub: Option<f64>,
}

impl App {
    fn new(files: Vec<PathBuf>) -> Self {
        let dir = scribecraft_platform::data_dir();
        let mut message = None;
        if let Some(d) = &dir
            && let Err(e) = std::fs::create_dir_all(d)
        {
            message = Some((format!("cannot create {}: {e}; the job list will not be saved", d.display()), true));
        }
        let settings_path = dir.as_ref().map(|d| d.join("settings.json"));
        let settings = settings_path.as_deref().and_then(load_settings).unwrap_or_default();
        let session = match dir.as_ref().map(|d| d.join("jobs.json")) {
            Some(jobs) => match Session::with_job_file(&jobs, settings.clone()) {
                Ok(s) => s,
                Err(e) => {
                    // A corrupt job list is reported and left on disk untouched; work goes on
                    // in memory so nothing overwrites it.
                    message = Some((e.to_string(), true));
                    Session::new(settings.clone()).unwrap_or_default()
                }
            },
            None => Session::new(settings.clone()).unwrap_or_default(),
        };
        let mut app = Self { session, settings_path, output: None, transport: None, message, bookmark_label: String::new(), scrub: None };
        let mut last = None;
        for f in files {
            last = app.add_file(&f).or(last);
        }
        if let Some(id) = last {
            app.open(id);
        }
        app
    }

    fn report(&mut self, result: Result<String, String>) {
        self.message = Some(match result {
            Ok(m) => (m, false),
            Err(e) => (e, true),
        });
    }

    fn add_file(&mut self, path: &Path) -> Option<u64> {
        if let Some(existing) = self.session.jobs().queue().iter().find(|d| d.path == path).map(|d| d.id) {
            return Some(existing);
        }
        match self.session.add_file(path, None, "") {
            Ok(id) => {
                self.save();
                Some(id)
            }
            Err(e) => {
                self.report(Err(e.to_string()));
                None
            }
        }
    }

    fn open(&mut self, id: u64) {
        // Stop the old stream before the old transport goes away.
        self.output = None;
        self.transport = None;
        match self.session.open(id) {
            Ok(t) => {
                match AudioOutput::open(t.clone()) {
                    Ok(o) => self.output = Some(o),
                    Err(e) => self.report(Err(e.to_string())),
                }
                self.transport = Some(t);
                self.save();
            }
            Err(e) => self.report(Err(e.to_string())),
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.session.save() {
            self.report(Err(e.to_string()));
        }
    }

    fn save_settings(&mut self) {
        let Some(path) = self.settings_path.clone() else { return };
        let result = serde_json::to_vec_pretty(self.session.settings())
            .map_err(|e| e.to_string())
            .and_then(|json| scribecraft_engine::write_atomic(&path, &json).map_err(|e| e.to_string()));
        if let Err(e) = result {
            self.report(Err(e));
        }
    }

    fn with_transport<R>(&self, f: impl FnOnce(&mut Transport) -> R) -> Option<R> {
        self.transport.as_ref().map(|t| f(&mut lock_transport(t)))
    }

    fn copy_timestamp(&mut self, ctx: &egui::Context) {
        if let Some(ms) = self.with_transport(|t| t.position_ms()) {
            let text = format!("[{}]", format_timestamp(ms, true));
            ctx.copy_text(text.clone());
            self.report(Ok(format!("copied {text}")));
        }
    }

    fn bookmark(&mut self) {
        let label = std::mem::take(&mut self.bookmark_label);
        let r = self.session.bookmark_here(&label).map(|ms| format!("bookmark at {}", format_timestamp(ms, true))).map_err(|e| e.to_string());
        self.report(r);
        self.save();
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        use egui::Key;
        let (pressed, released, ctrl) = ctx.input(|i| {
            let mut pressed = Vec::new();
            let mut released = Vec::new();
            for e in &i.events {
                if let egui::Event::Key { key, pressed: down, repeat: false, .. } = e {
                    if *down { pressed.push(*key) } else { released.push(*key) }
                }
            }
            (pressed, released, i.modifiers.command)
        });
        for key in pressed {
            match (key, ctrl) {
                (Key::F9, _) => {
                    self.with_transport(Transport::toggle_play);
                }
                (Key::F7, _) => {
                    self.with_transport(Transport::rewind);
                }
                (Key::F8, _) => {
                    self.with_transport(Transport::fast_forward);
                }
                (Key::F5, _) => {
                    self.with_transport(Transport::toggle_slow);
                }
                (Key::F6, _) => {
                    self.with_transport(Transport::toggle_fast);
                }
                (Key::F10, _) => self.copy_timestamp(ctx),
                (Key::B, true) => self.bookmark(),
                (Key::S, true) => self.save(),
                _ => {}
            }
        }
        for key in released {
            if matches!(key, Key::F7 | Key::F8) {
                self.with_transport(Transport::release);
            }
        }
    }

    fn files_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        let mut last = None;
        for p in dropped {
            if media::is_supported_path(&p) {
                last = self.add_file(&p).or(last);
            } else {
                self.report(Err(format!("{} is not a supported recording", p.display())));
            }
        }
        if let Some(id) = last {
            self.open(id);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Add recordings…").clicked()
                && let Some(files) = rfd::FileDialog::new().add_filter("Recordings", media::SUPPORTED_EXTENSIONS).pick_files()
            {
                let mut last = None;
                for f in files {
                    last = self.add_file(&f).or(last);
                }
                if let Some(id) = last {
                    self.open(id);
                }
            }
            if ui.button("Add folder…").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                let r = scan_folder(&dir, 2, self.session.jobs()).map_err(|e| e.to_string());
                match r {
                    Ok(found) => {
                        let n = found.len();
                        for f in found {
                            self.add_file(&f);
                        }
                        self.report(Ok(format!("added {n} recording(s)")));
                    }
                    Err(e) => self.report(Err(e)),
                }
            }
            if ui.button("Save").on_hover_text("Ctrl+S").clicked() {
                self.save();
            }
            ui.separator();
            ui.weak("F9 play/stop · F7/F8 hold to rewind/forward · F5 slow · F6 fast · F10 copy time · Ctrl+B bookmark");
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dictations");
        let open_id = self.session.open_id();
        let rows: Vec<(u64, String, String, Priority, Status, Option<u64>)> =
            self.session.jobs().queue().iter().map(|d| (d.id, d.title.clone(), d.author.clone(), d.priority, d.status, d.duration_ms)).collect();
        if rows.is_empty() {
            ui.label("No dictations yet. Use “Add recordings…” or drop files on the window.");
        }
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (id, title, author, priority, status, duration) in rows {
                let text = format!(
                    "{}{}\n{} · {} · {}{}",
                    priority_mark(priority),
                    title,
                    status_name(status),
                    duration.map(|d| format_timestamp(d, false)).unwrap_or_else(|| "--:--:--".into()),
                    if author.is_empty() { "no author" } else { &author },
                    "",
                );
                if ui.selectable_label(open_id == Some(id), text).clicked() && open_id != Some(id) {
                    open = Some(id);
                }
            }
        });
        if let Some(id) = open {
            self.open(id);
        }
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        let Some(t) = self.transport.clone() else {
            ui.label("Open a dictation to start transcribing.");
            return;
        };
        let (state, pos, dur, speed, preset) = {
            let t = lock_transport(&t);
            (t.state(), t.position_ms(), t.duration_ms(), t.speed(), t.preset())
        };
        ui.horizontal(|ui| {
            ui.monospace(format!("{} / {}", format_timestamp(pos, true), format_timestamp(dur, true)));
            ui.label(match state {
                State::Stopped => "stopped",
                State::Playing => "playing",
                State::Rewinding => "rewinding",
                State::FastForwarding => "fast-forwarding",
            });
        });
        // Seek bar: dragging previews the target; releasing seeks.
        let mut value = self.scrub.unwrap_or(pos as f64);
        let normal_width = ui.spacing().slider_width;
        ui.spacing_mut().slider_width = (ui.available_width() - 20.0).max(100.0);
        let slider = ui.add(egui::Slider::new(&mut value, 0.0..=(dur.max(1) as f64)).show_value(false).trailing_fill(true));
        ui.spacing_mut().slider_width = normal_width;
        if slider.dragged() {
            self.scrub = Some(value);
        }
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            lock_transport(&t).seek_ms(value as u64);
            self.scrub = None;
        }
        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("Start").clicked() {
                lock_transport(&t).seek_ms(0);
            }
            let skip = i64::try_from(self.session.settings().skip_ms).unwrap_or(i64::MAX);
            if ui.button("« 5 s").on_hover_text("Skip back").clicked() {
                lock_transport(&t).skip_ms(-skip);
            }
            let rew = ui.button("⏪").on_hover_text("Rewind (hold) — F7");
            if rew.is_pointer_button_down_on() {
                if state != State::Rewinding {
                    lock_transport(&t).rewind();
                }
            } else if state == State::Rewinding && !ui.input(|i| i.key_down(egui::Key::F7)) {
                lock_transport(&t).release();
            }
            let label = if state == State::Playing { "⏸ Stop" } else { "▶ Play" };
            if ui.add(egui::Button::new(label).min_size(egui::vec2(90.0, 0.0))).on_hover_text("F9").clicked() {
                lock_transport(&t).toggle_play();
            }
            let ff = ui.button("⏩").on_hover_text("Fast-forward (hold) — F8");
            if ff.is_pointer_button_down_on() {
                if state != State::FastForwarding {
                    lock_transport(&t).fast_forward();
                }
            } else if state == State::FastForwarding && !ui.input(|i| i.key_down(egui::Key::F8)) {
                lock_transport(&t).release();
            }
            if ui.button("5 s »").on_hover_text("Skip forward").clicked() {
                lock_transport(&t).skip_ms(skip);
            }
            ui.separator();
            if ui.selectable_label(preset == SpeedPreset::Slow, "Slow").on_hover_text("F5").clicked() {
                lock_transport(&t).toggle_slow();
            }
            if ui.selectable_label(preset == SpeedPreset::Fast, "Fast").on_hover_text("F6").clicked() {
                lock_transport(&t).toggle_fast();
            }
            let mut pct = speed * 100.0;
            let r = ui.add(egui::Slider::new(&mut pct, MIN_SPEED * 100.0..=MAX_SPEED * 100.0).suffix(" %").text("speed"));
            if r.changed() {
                let _ = lock_transport(&t).set_speed((pct / 100.0).clamp(MIN_SPEED, MAX_SPEED));
            }
            if ui.button("100 %").clicked() {
                let _ = lock_transport(&t).set_speed(1.0);
            }
        });
    }

    fn settings_row(&mut self, ui: &mut egui::Ui) {
        let mut s: Settings = self.session.settings().clone();
        let before = s.clone();
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut s.volume_db, -30.0..=12.0).suffix(" dB").text("volume"));
            egui::ComboBox::from_label("channels").selected_text(channel_name(s.channel_mode)).show_ui(ui, |ui| {
                for m in [ChannelMode::Stereo, ChannelMode::Left, ChannelMode::Right, ChannelMode::Mix] {
                    ui.selectable_value(&mut s.channel_mode, m, channel_name(m));
                }
            });
            ui.add(egui::DragValue::new(&mut s.auto_backstep_ms).range(0..=60_000).speed(50).suffix(" ms"));
            ui.label("auto-backstep");
            let mut slow = s.slow_speed * 100.0;
            if ui.add(egui::DragValue::new(&mut slow).range(10.0..=400.0).suffix(" %")).changed() {
                s.slow_speed = slow / 100.0;
            }
            ui.label("slow");
            let mut fast = s.fast_speed * 100.0;
            if ui.add(egui::DragValue::new(&mut fast).range(10.0..=400.0).suffix(" %")).changed() {
                s.fast_speed = fast / 100.0;
            }
            ui.label("fast");
        });
        if s != before {
            match self.session.set_settings(s) {
                Ok(()) => self.save_settings(),
                Err(e) => self.report(Err(e.to_string())),
            }
        }
    }

    fn details(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(id) = self.session.open_id() else { return };
        let Ok(job) = self.session.jobs().get(id) else { return };
        let mut job = job.clone();
        let before = job.clone();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Title");
            ui.text_edit_singleline(&mut job.title);
            ui.label("Author");
            ui.text_edit_singleline(&mut job.author);
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("priority").selected_text(format!("{:?}", job.priority)).show_ui(ui, |ui| {
                for p in [Priority::Low, Priority::Normal, Priority::High, Priority::Urgent] {
                    ui.selectable_value(&mut job.priority, p, format!("{p:?}"));
                }
            });
            egui::ComboBox::from_label("status").selected_text(status_name(job.status)).show_ui(ui, |ui| {
                for s in [Status::New, Status::InProgress, Status::Done, Status::Dispatched] {
                    ui.selectable_value(&mut job.status, s, status_name(s));
                }
            });
            ui.label("Due");
            ui.add(egui::TextEdit::singleline(&mut job.due).desired_width(110.0).hint_text("YYYY-MM-DD"));
        });
        ui.horizontal(|ui| {
            if ui.button("Copy time stamp").on_hover_text("F10").clicked() {
                self.copy_timestamp(ctx);
            }
            ui.add(egui::TextEdit::singleline(&mut self.bookmark_label).hint_text("bookmark label").desired_width(200.0));
            if ui.button("Add bookmark").on_hover_text("Ctrl+B").clicked() {
                self.bookmark();
            }
        });
        let mut jump = None;
        if !job.bookmarks.is_empty() {
            ui.collapsing(format!("Bookmarks ({})", job.bookmarks.len()), |ui| {
                for b in &job.bookmarks {
                    if ui.link(format!("{}  {}", format_timestamp(b.ms, true), b.label)).clicked() {
                        jump = Some(b.ms);
                    }
                }
            });
        }
        ui.label("Notes");
        ui.add(egui::TextEdit::multiline(&mut job.notes).desired_rows(6).desired_width(f32::INFINITY));
        if let Some(ms) = jump {
            self.with_transport(|t| t.seek_ms(ms));
        }
        let edited = job.title != before.title
            || job.author != before.author
            || job.priority != before.priority
            || job.status != before.status
            || job.due != before.due
            || job.notes != before.notes;
        // Field by field: a bookmark added above this frame must not be overwritten by the copy.
        if edited && let Ok(d) = self.session.jobs_mut().get_mut(id) {
            let limit = |s: String| s.chars().take(scribecraft_engine::MAX_TEXT_CHARS).collect::<String>();
            d.title = limit(job.title);
            d.author = limit(job.author);
            d.priority = job.priority;
            d.status = job.status;
            d.due = limit(job.due);
            d.notes = limit(job.notes);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.files_dropped(ctx);
        self.keyboard(ctx);
        egui::Panel::top("toolbar").show(root, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(root, |ui| {
            match &self.message {
                Some((m, true)) => ui.colored_label(ui.visuals().error_fg_color, m),
                Some((m, false)) => ui.label(m),
                None => ui.weak(if self.session.is_dirty() { "unsaved changes" } else { "ready" }),
            };
        });
        egui::Panel::left("jobs").resizable(true).default_size(300.0).show(root, |ui| self.job_list(ui));
        egui::CentralPanel::default().show(root, |ui| {
            self.transport_bar(ui);
            ui.separator();
            self.settings_row(ui);
            self.details(ui, ctx);
        });
        // Keep the clock moving while sound plays.
        if self.with_transport(|t| t.state() != State::Stopped).unwrap_or(false) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.output = None;
        self.session.close();
        let _ = self.session.save();
    }
}

fn load_settings(path: &Path) -> Option<Settings> {
    let bytes = std::fs::read(path).ok()?;
    let s: Settings = serde_json::from_slice(&bytes).ok()?;
    s.validate().ok().map(|()| s)
}

fn priority_mark(p: Priority) -> &'static str {
    match p {
        Priority::Urgent => "‼ ",
        Priority::High => "! ",
        Priority::Normal | Priority::Low => "",
    }
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::New => "new",
        Status::InProgress => "in progress",
        Status::Done => "done",
        Status::Dispatched => "dispatched",
    }
}

fn channel_name(m: ChannelMode) -> &'static str {
    match m {
        ChannelMode::Stereo => "both",
        ChannelMode::Left => "left only",
        ChannelMode::Right => "right only",
        ChannelMode::Mix => "mono mix",
    }
}
