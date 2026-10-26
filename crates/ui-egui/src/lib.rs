//! ScribeCraft's desktop interface (layer L6).
//!
//! One window: the dictation job list on the left, the open dictation on the right with its
//! transport bar, speed / volume / channel controls, notes and bookmarks.
//!
//! Control: a foot pedal (USB or serial) and hotkeys that work system-wide by default
//! (F7 rewind, F8 fast-forward, F9 play/stop, F4 stop, F6 slow; see `controls`), plus, in the
//! window, F10 copy time stamp · Ctrl+B bookmark · Ctrl+S save.

mod controls;
mod job_list;
mod theme;
mod transport_bar;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use scribecraft_engine::transport::{ChannelMode, format_timestamp};
use scribecraft_engine::{Priority, Session, Settings, State, Status, Transport, lock_transport, media, scan_folder};
use scribecraft_platform::AudioOutput;

use controls::Controls;
use job_list::Filter;
use transport_bar::TransportBar;

/// Opens the main window; `files` are recordings to add and open (from the command line or
/// the operating system's "Open with").
pub fn run(files: Vec<PathBuf>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("ScribeCraft").with_inner_size([1100.0, 700.0]).with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("ScribeCraft", options, Box::new(move |cc| Ok(Box::new(App::new(cc.egui_ctx.clone(), files))))).map_err(|e| e.to_string())
}

struct App {
    session: Session,
    settings_path: Option<PathBuf>,
    output: Option<AudioOutput>,
    transport: Option<Arc<Mutex<Transport>>>,
    message: Option<(String, bool)>,
    bookmark_label: String,
    bar: TransportBar,
    controls: Controls,
    filter: Filter,
}

impl App {
    fn new(ctx: egui::Context, files: Vec<PathBuf>) -> Self {
        theme::apply(&ctx);
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
        let controls = Controls::new(ctx, dir.clone());
        let mut app = Self {
            session,
            settings_path,
            output: None,
            transport: None,
            message,
            bookmark_label: String::new(),
            bar: TransportBar::default(),
            controls,
            filter: Filter::default(),
        };
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
        self.bar.reset();
        self.controls.set_transport(None);
        match self.session.open(id) {
            Ok(t) => {
                match AudioOutput::open(t.clone()) {
                    Ok(o) => self.output = Some(o),
                    Err(e) => self.report(Err(e.to_string())),
                }
                self.controls.set_transport(Some(t.clone()));
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
        let events: Vec<(Key, bool, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed, repeat: false, modifiers, .. } => Some((*key, *pressed, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        if self.controls.capturing() {
            // The Pedal and keys window is waiting for a key to assign.
            return;
        }
        for (key, pressed, modifiers) in events {
            // Transport keys: handled here only when they are not registered system-wide
            // (then the system delivers them to the hotkey handler instead).
            if let Some(action) = self.controls.window_action(key, modifiers) {
                self.with_transport(|t| scribecraft_engine::hotkeys::apply_hotkey(action, pressed, t));
                continue;
            }
            if !pressed {
                continue;
            }
            match (key, modifiers.command) {
                (Key::F10, _) => self.copy_timestamp(ctx),
                (Key::B, true) => self.bookmark(),
                (Key::S, true) => self.save(),
                _ => {}
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
            brand(ui);
            ui.add_space(6.0);
            let add = egui::Button::new(egui::RichText::new("Add recordings…").strong().color(theme::ACCENT_INK))
                .fill(theme::ACCENT)
                .stroke(egui::Stroke::new(1.0, theme::ACCENT))
                .corner_radius(8);
            if ui.add(add).on_hover_text("Choose recordings to add to the job list").clicked()
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
            if ui.button("Pedal and keys…").clicked() {
                self.controls.open = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak("F10 stamp · Ctrl+B bookmark").on_hover_text(self.controls.key_hint());
            });
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        let open_id = self.session.open_id();
        let rows: Vec<job_list::Row> = self.session.jobs().queue().iter().map(|d| job_list::Row::of(d)).collect();
        ui.horizontal(|ui| {
            ui.heading("Dictations");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(format!("{} in queue", rows.len()));
            });
        });
        ui.add_space(4.0);
        job_list::chips(ui, &mut self.filter, &rows);
        if rows.is_empty() {
            ui.add_space(6.0);
            ui.label("No dictations yet. Use “Add recordings…” or drop files on the window.");
            return;
        }
        let mut open = None;
        let mut shown = 0usize;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for row in &rows {
                if !self.filter.matches(row.status) {
                    continue;
                }
                shown += 1;
                if job_list::row(ui, row, open_id == Some(row.id)).clicked() && open_id != Some(row.id) {
                    open = Some(row.id);
                }
                ui.add_space(2.0);
            }
        });
        if shown == 0 {
            ui.add_space(6.0);
            ui.label(format!("Nothing under “{}”. Pick “All” to see the whole queue.", self.filter.label()));
        }
        if let Some(id) = open {
            self.open(id);
        }
    }

    /// The dock at the bottom: the open dictation's transport, or a hint when none is open.
    fn transport_dock(&mut self, ui: &mut egui::Ui) {
        let Some(t) = self.transport.clone() else {
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("No dictation open").strong().color(theme::TEXT));
                    ui.label(egui::RichText::new("Add or open a recording to start transcribing.").color(theme::MUTED));
                });
            });
            return;
        };
        let bookmarks: Vec<u64> = self
            .session
            .open_id()
            .and_then(|id| self.session.jobs().get(id).ok())
            .map(|job| job.bookmarks.iter().map(|b| b.ms).collect())
            .unwrap_or_default();
        let skip = self.session.settings().skip_ms;
        self.bar.show(ui, &t, skip, &bookmarks);
    }

    fn settings_row(&mut self, ui: &mut egui::Ui) {
        let mut s: Settings = self.session.settings().clone();
        let before = s.clone();
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(theme::caption("PLAYBACK"));
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                field(ui, "volume", |ui| {
                    ui.spacing_mut().slider_width = 90.0;
                    ui.add(egui::Slider::new(&mut s.volume_db, -30.0..=12.0).suffix(" dB"));
                });
                egui::ComboBox::from_label("channels").selected_text(channel_name(s.channel_mode)).show_ui(ui, |ui| {
                    for m in [ChannelMode::Stereo, ChannelMode::Left, ChannelMode::Right, ChannelMode::Mix] {
                        ui.selectable_value(&mut s.channel_mode, m, channel_name(m));
                    }
                });
                field(ui, "auto-backstep", |ui| {
                    ui.add(egui::DragValue::new(&mut s.auto_backstep_ms).range(0..=60_000).speed(50).suffix(" ms"));
                });
                field(ui, "slow", |ui| {
                    let mut slow = s.slow_speed * 100.0;
                    if ui.add(egui::DragValue::new(&mut slow).range(10.0..=400.0).suffix(" %")).changed() {
                        s.slow_speed = slow / 100.0;
                    }
                });
                field(ui, "fast", |ui| {
                    let mut fast = s.fast_speed * 100.0;
                    if ui.add(egui::DragValue::new(&mut fast).range(10.0..=400.0).suffix(" %")).changed() {
                        s.fast_speed = fast / 100.0;
                    }
                });
            });
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
        ui.add_space(6.0);
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(theme::caption("DICTATION"));
            ui.add_space(2.0);
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
                egui::ComboBox::from_label("status").selected_text(job_list::status_name(job.status)).show_ui(ui, |ui| {
                    for s in [Status::New, Status::InProgress, Status::Done, Status::Dispatched] {
                        ui.selectable_value(&mut job.status, s, job_list::status_name(s));
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
                egui::CollapsingHeader::new(format!("Bookmarks ({})", job.bookmarks.len())).show(ui, |ui| {
                    for b in &job.bookmarks {
                        if ui.link(format!("{}  {}", format_timestamp(b.ms, true), b.label)).clicked() {
                            jump = Some(b.ms);
                        }
                    }
                });
            }
            ui.add_space(6.0);
            ui.label(theme::caption("NOTES"));
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
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.files_dropped(ctx);
        self.keyboard(ctx);
        egui::Panel::top("toolbar").frame(theme::top_frame()).show(root, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").frame(theme::status_frame()).show(root, |ui| {
            ui.horizontal(|ui| {
                match &self.message {
                    Some((m, true)) => ui.colored_label(ui.visuals().error_fg_color, m),
                    Some((m, false)) => ui.label(m),
                    None => ui.weak(if self.session.is_dirty() { "unsaved changes" } else { "ready" }),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(self.controls.status()));
            });
        });
        egui::Panel::bottom("transport").frame(theme::dock_frame()).show(root, |ui| self.transport_dock(ui));
        self.controls.window(ctx);
        egui::Panel::left("jobs").frame(theme::side_frame()).resizable(true).default_size(300.0).show(root, |ui| self.job_list(ui));
        egui::CentralPanel::default().frame(theme::central_frame()).show(root, |ui| {
            // The dock keeps its height at the bottom; a short window scrolls the rest.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.settings_row(ui);
                self.details(ui, ctx);
            });
        });
        // Keep the clock moving while sound plays.
        if self.with_transport(|t| t.state() != State::Stopped).unwrap_or(false) {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.controls.set_transport(None);
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

/// The product mark and name in the toolbar. The mark is drawn here, not loaded: an original
/// shape, no asset file to license.
fn brand(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 6.0, theme::ACCENT);
    ui.painter().rect_filled(rect.shrink(5.5), 2.0, theme::ACCENT_INK);
    ui.label(egui::RichText::new("ScribeCraft").size(15.0).strong());
}

/// A label and its widget kept together when the settings row wraps.
fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(label).color(theme::MUTED));
        add(ui);
    });
}

fn channel_name(m: ChannelMode) -> &'static str {
    match m {
        ChannelMode::Stereo => "both",
        ChannelMode::Left => "left only",
        ChannelMode::Right => "right only",
        ChannelMode::Mix => "mono mix",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dock and the status bar keep their height at the bottom of the window, so on a
    /// short window the pane between them must scroll. Without the scroll area the lower
    /// fields would simply be cut off and unreachable.
    #[test]
    fn a_short_window_scrolls_the_pane_above_the_dock() {
        // The same panel order as `App::ui`: status, dock, then the central pane.
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 320.0));
        let draw = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut marker = f32::NAN;
            let mut clock = f32::NAN;
            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::bottom("status").frame(theme::status_frame()).show(ui, |ui| {
                    ui.label("ready");
                });
                egui::Panel::bottom("transport").frame(theme::dock_frame()).show(ui, |ui| {
                    clock = ui.label("00:00:12.4").rect.top();
                });
                egui::CentralPanel::default().frame(theme::central_frame()).show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        marker = ui.label("marker").rect.top();
                        for i in 0..60 {
                            ui.label(format!("line {i}"));
                        }
                    });
                });
            });
            output.textures_delta.clear();
            (marker, clock)
        };

        // egui smooths a wheel event over the next frames: the offset is applied on the
        // frames after the event, so measure once the movement has settled.
        let (marker_top, clock_top) = {
            draw(Vec::new());
            draw(Vec::new())
        };
        let pointer = screen.center();
        draw(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -100.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            },
        ]);
        draw(Vec::new());
        let (scrolled_top, clock_after) = draw(Vec::new());

        assert!(scrolled_top < marker_top - 20.0, "the pane should scroll: {marker_top} -> {scrolled_top}");
        assert_eq!(clock_top, clock_after, "scrolling must not move the transport dock");
    }
}
