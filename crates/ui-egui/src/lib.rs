//! ScribeCraft's desktop interface (layer L6).
//!
//! One window: the dictation job list on the left, the open dictation on the right with its
//! transport bar, speed / volume / channel controls, notes and bookmarks.
//!
//! Control: a foot pedal (USB or serial) and hotkeys that work system-wide by default
//! (F7 rewind, F8 fast-forward, F9 play/stop, F4 stop, F6 slow; see `controls`), plus, in the
//! window, F10 copy time stamp · Ctrl+B bookmark · Ctrl+S save.

mod controls;
mod i18n;
mod job_list;
mod quit;
mod theme;
mod transport_bar;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use scribecraft_engine::transport::{ChannelMode, format_timestamp};
use scribecraft_engine::{Dictation, Priority, Session, Settings, State, Status, Transport, lock_transport, media, scan_folder};
use scribecraft_platform::AudioOutput;

use controls::Controls;
use i18n::{Lang, Text};
use job_list::Filter;
use theme::Theme;
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
    /// Where the language and theme choices are kept, next to the job list.
    ui_path: Option<PathBuf>,
    lang: Lang,
    theme: Theme,
    output: Option<AudioOutput>,
    transport: Option<Arc<Mutex<Transport>>>,
    message: Option<(String, bool)>,
    bookmark_label: String,
    bar: TransportBar,
    controls: Controls,
    filter: Filter,
    /// The dictation whose removal is waiting for a confirmation, if any. Nothing is thrown
    /// away on a single click, and removing does not touch the recording file.
    confirm_remove: Option<u64>,
}

impl App {
    fn new(ctx: egui::Context, files: Vec<PathBuf>) -> Self {
        let dir = scribecraft_platform::data_dir();
        let ui_path = dir.as_ref().map(|d| d.join("ui.json"));
        let saved = ui_path.as_deref().map_or_else(i18n::Preferences::default, i18n::Preferences::load);
        let (lang, theme) = (saved.lang, saved.theme);
        // After reading the choice, so the first frame is already in the right palette.
        theme::apply(&ctx, theme);
        let mut message = None;
        if let Some(d) = &dir
            && let Err(e) = std::fs::create_dir_all(d)
        {
            message = Some((i18n::cannot_create(&d.display().to_string(), &e.to_string(), lang), true));
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
            ui_path,
            lang,
            theme,
            output: None,
            transport: None,
            message,
            bookmark_label: String::new(),
            bar: TransportBar::default(),
            controls,
            filter: Filter::default(),
            confirm_remove: None,
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

    /// Remembers the language and the theme for the next start. A failure is reported but
    /// never stops the interface: the choice still applies until the window closes.
    fn save_preferences(&mut self) {
        let Some(path) = self.ui_path.clone() else { return };
        if let Err(e) = (i18n::Preferences { lang: self.lang, theme: self.theme }).save(&path) {
            self.report(Err(e));
        }
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
        self.confirm_remove = None;
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

    /// Removes a dictation from the list, after the confirmation. The open transport is stopped
    /// and let go before the job disappears, and the recording file on disk is never touched.
    fn remove_dictation(&mut self, id: u64) {
        if self.session.open_id() == Some(id) {
            self.output = None;
            self.transport = None;
            self.bar.reset();
            self.controls.set_transport(None);
        }
        self.confirm_remove = None;
        match self.session.remove(id) {
            Ok(job) => {
                self.report(Ok(i18n::removed(&job_title(&job, self.lang), self.lang)));
                self.save();
            }
            Err(e) => self.report(Err(e.to_string())),
        }
    }

    /// Carries out a choice from a queue row's right-click menu. Removing only asks here; the
    /// question is answered in the details panel (open job) or the confirmation window.
    fn row_action(&mut self, id: u64, action: job_list::RowAction) {
        let lang = self.lang;
        match action.status() {
            Some(status) => {
                let title = match self.session.jobs_mut().get_mut(id) {
                    Ok(d) => {
                        d.status = status;
                        job_title(d, lang)
                    }
                    Err(e) => return self.report(Err(e.to_string())),
                };
                self.report(Ok(i18n::marked(&title, status == Status::Done, lang)));
                self.save();
            }
            None => self.confirm_remove = Some(id),
        }
    }

    /// The confirmation window for removing a dictation that is not the open one (the open
    /// one asks inside its details panel). Escape or a click outside cancels.
    fn remove_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_remove else { return };
        if self.session.open_id() == Some(id) {
            return;
        }
        let lang = self.lang;
        let Ok(job) = self.session.jobs().get(id) else {
            self.confirm_remove = None;
            return;
        };
        let title = job_title(job, lang);
        let modal = egui::Modal::new(egui::Id::new("remove-dictation")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            remove_controls(ui, lang, true, &title)
        });
        match modal.inner {
            RemoveRequest::Confirm => self.remove_dictation(id),
            RemoveRequest::Cancel => self.confirm_remove = None,
            RemoveRequest::Ask | RemoveRequest::None => {
                if modal.should_close() {
                    self.confirm_remove = None;
                }
            }
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
            self.report(Ok(i18n::copied(&text, self.lang)));
        }
    }

    fn bookmark(&mut self) {
        let label = std::mem::take(&mut self.bookmark_label);
        let r = self.session.bookmark_here(&label).map(|ms| i18n::bookmark_at(&format_timestamp(ms, true), self.lang)).map_err(|e| e.to_string());
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
                // Escape answers the removal question the safe way.
                (Key::Escape, false) if self.confirm_remove.is_some() => self.confirm_remove = None,
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
                let path = p.display().to_string();
                self.report(Err(i18n::not_supported(&path, self.lang)));
            }
        }
        if let Some(id) = last {
            self.open(id);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // Wrapped: at the window's minimum width (640 pt) the buttons and both pickers do not
        // fit on one line, and a plain row would push the last of them out of the window.
        ui.horizontal_wrapped(|ui| {
            brand(ui);
            ui.add_space(6.0);
            let add = egui::Button::new(egui::RichText::new(Text::AddRecordings.in_lang(lang)).strong().color(theme::accent_ink()))
                .fill(theme::accent())
                .stroke(egui::Stroke::new(1.0, theme::accent()))
                .corner_radius(8);
            if ui.add(add).on_hover_text(Text::AddRecordingsHint.in_lang(lang)).clicked()
                && let Some(files) = rfd::FileDialog::new().add_filter(Text::RecordingsFilter.in_lang(lang), media::SUPPORTED_EXTENSIONS).pick_files()
            {
                let mut added = Vec::new();
                let mut failed = 0;
                for f in files {
                    match self.add_file(&f) {
                        Some(id) => added.push(id),
                        None => failed += 1,
                    }
                }
                if failed == 0 && !added.is_empty() {
                    self.report(Ok(i18n::added_recordings(added.len(), lang)));
                }
                // Open the first one picked, so the list starts at what was just added.
                if let Some(id) = added.first() {
                    self.open(*id);
                }
            }
            if ui.button(Text::AddFolder.in_lang(lang)).clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                let r = scan_folder(&dir, 2, self.session.jobs()).map_err(|e| e.to_string());
                match r {
                    Ok(found) => {
                        let n = found.len();
                        for f in found {
                            self.add_file(&f);
                        }
                        self.report(Ok(i18n::added_recordings(n, lang)));
                    }
                    Err(e) => self.report(Err(e)),
                }
            }
            if ui.button(Text::Save.in_lang(lang)).on_hover_text("Ctrl+S").clicked() {
                self.save();
            }
            if ui.button(Text::PedalAndKeys.in_lang(lang)).clicked() {
                self.controls.open = true;
            }
            let before = self.lang;
            let picker = egui::ComboBox::from_id_salt("language").selected_text(self.lang.native_name()).width(92.0).show_ui(ui, |ui| {
                for candidate in Lang::ALL {
                    ui.selectable_value(&mut self.lang, candidate, candidate.native_name());
                }
            });
            picker.response.on_hover_text(Text::LanguageHint.in_lang(lang));
            let theme_before = self.theme;
            let theme_picker = egui::ComboBox::from_id_salt("theme").selected_text(self.theme.label(lang)).width(76.0).show_ui(ui, |ui| {
                for candidate in Theme::ALL {
                    ui.selectable_value(&mut self.theme, candidate, candidate.label(lang));
                }
            });
            theme_picker.response.on_hover_text(Text::ThemeHint.in_lang(lang));
            if self.theme != theme_before {
                theme::apply(ui.ctx(), self.theme);
            }
            if self.lang != before || self.theme != theme_before {
                self.save_preferences();
            }
            ui.weak(Text::ToolbarKeys.in_lang(lang)).on_hover_text(self.controls.key_hint(lang));
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let open_id = self.session.open_id();
        let rows: Vec<job_list::Row> = self.session.jobs().queue().iter().map(|d| job_list::Row::of(d)).collect();
        ui.horizontal(|ui| {
            ui.heading(Text::Dictations.in_lang(lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(i18n::in_queue(rows.len(), lang));
            });
        });
        ui.add_space(4.0);
        job_list::chips(ui, &mut self.filter, &rows, lang);
        if rows.is_empty() {
            ui.add_space(6.0);
            ui.label(Text::NoDictations.in_lang(lang));
            return;
        }
        let mut open = None;
        let mut shown = 0usize;
        let mut menu: Option<(u64, job_list::RowAction)> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for row in &rows {
                if !self.filter.matches(row.status) {
                    continue;
                }
                shown += 1;
                let response = job_list::row(ui, row, open_id == Some(row.id), lang);
                if response.clicked() && opens_on_click(row.id, open_id) {
                    open = Some(row.id);
                }
                // Right click: flag it completed (or back to do), or remove it from the list.
                response.context_menu(|ui| {
                    if let Some(action) = job_list::row_menu(ui, row, lang) {
                        menu = Some((row.id, action));
                    }
                });
                ui.add_space(2.0);
            }
        });
        if let Some((id, action)) = menu {
            self.row_action(id, action);
        }
        if shown == 0 {
            ui.add_space(6.0);
            ui.label(i18n::fill(Text::NothingUnder.in_lang(lang), &[("{filter}", self.filter.label(lang)), ("{all}", Filter::All.label(lang))]));
        }
        if let Some(id) = open {
            // Selecting a recording only brings it into the transport: the transcriptionist
            // starts it with Play (or the pedal), so opening a job never surprises the room.
            self.open(id);
        }
    }

    /// The dock at the bottom: the open dictation's transport, or a hint when none is open.
    fn transport_dock(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let Some(t) = self.transport.clone() else {
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(Text::NoOpen.in_lang(lang)).strong().color(theme::text()));
                    ui.label(egui::RichText::new(Text::AddOrOpen.in_lang(lang)).color(theme::muted()));
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
        self.bar.show(ui, &t, skip, &bookmarks, lang);
    }

    fn settings_row(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let mut s: Settings = self.session.settings().clone();
        let before = s.clone();
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(theme::caption(Text::Playback.in_lang(lang)));
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                field(ui, Text::Volume.in_lang(lang), |ui| {
                    ui.spacing_mut().slider_width = 90.0;
                    ui.add(egui::Slider::new(&mut s.volume_db, -30.0..=12.0).suffix(" dB"));
                });
                egui::ComboBox::from_label(Text::Channels.in_lang(lang)).selected_text(channel_name(s.channel_mode, lang)).show_ui(ui, |ui| {
                    for m in [ChannelMode::Stereo, ChannelMode::Left, ChannelMode::Right, ChannelMode::Mix] {
                        ui.selectable_value(&mut s.channel_mode, m, channel_name(m, lang));
                    }
                });
                field(ui, Text::AutoBackstep.in_lang(lang), |ui| {
                    ui.add(egui::DragValue::new(&mut s.auto_backstep_ms).range(0..=60_000).speed(50).suffix(" ms"));
                });
                field(ui, Text::SlowField.in_lang(lang), |ui| {
                    let mut slow = s.slow_speed * 100.0;
                    if ui.add(egui::DragValue::new(&mut slow).range(10.0..=400.0).suffix(" %")).changed() {
                        s.slow_speed = slow / 100.0;
                    }
                });
                field(ui, Text::FastField.in_lang(lang), |ui| {
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

    /// The right-hand card: what the open dictation is and what has to be done with it.
    fn details(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let lang = self.lang;
        let Some(id) = self.session.open_id() else { return };
        let Ok(job) = self.session.jobs().get(id) else { return };
        let mut job = job.clone();
        let before = job.clone();
        let mut jump = None;
        let mut remove = RemoveRequest::None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.horizontal(|ui| {
                ui.label(theme::caption(Text::Dictation.in_lang(lang)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| status_pill(ui, job.status, lang));
            });
            ui.add_space(4.0);
            detail_row(ui, Text::Title.in_lang(lang), |ui| {
                let width = ui.available_width();
                ui.add(egui::TextEdit::singleline(&mut job.title).desired_width(width));
            });
            detail_row(ui, Text::Author.in_lang(lang), |ui| {
                let width = ui.available_width();
                ui.add(egui::TextEdit::singleline(&mut job.author).hint_text(Text::WhoDictated.in_lang(lang)).desired_width(width));
            });
            detail_row(ui, Text::Priority.in_lang(lang), |ui| {
                let width = ui.available_width();
                egui::ComboBox::from_id_salt("details_priority").selected_text(priority_name(job.priority, lang)).width(width).show_ui(ui, |ui| {
                    for p in [Priority::Low, Priority::Normal, Priority::High, Priority::Urgent] {
                        ui.selectable_value(&mut job.priority, p, priority_name(p, lang));
                    }
                });
            });
            detail_row(ui, Text::Status.in_lang(lang), |ui| {
                let width = ui.available_width();
                egui::ComboBox::from_id_salt("details_status").selected_text(job_list::status_name(job.status, lang)).width(width).show_ui(
                    ui,
                    |ui| {
                        for s in [Status::New, Status::InProgress, Status::Done, Status::Dispatched] {
                            ui.selectable_value(&mut job.status, s, job_list::status_name(s, lang));
                        }
                    },
                );
            });
            detail_row(ui, Text::Due.in_lang(lang), |ui| {
                let width = ui.available_width();
                ui.add(egui::TextEdit::singleline(&mut job.due).hint_text(Text::DueFormat.in_lang(lang)).desired_width(width));
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(theme::caption(Text::Bookmarks.in_lang(lang)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(job.bookmarks.len().to_string());
                });
            });
            match bookmark_controls(ui, lang, &mut self.bookmark_label) {
                BookmarkRequest::None => {}
                BookmarkRequest::Add => self.bookmark(),
                BookmarkRequest::CopyTime => self.copy_timestamp(ctx),
            }
            if job.bookmarks.is_empty() {
                ui.label(egui::RichText::new(Text::NoBookmarks.in_lang(lang)).size(12.5).color(theme::muted()));
            }
            for b in &job.bookmarks {
                ui.horizontal(|ui| {
                    let link = ui.add(egui::Link::new(egui::RichText::new(format_timestamp(b.ms, true)).monospace().color(theme::amber())));
                    if link.on_hover_text(Text::JumpHere.in_lang(lang)).clicked() {
                        jump = Some(b.ms);
                    }
                    ui.label(egui::RichText::new(&b.label).size(12.5).color(theme::muted()));
                });
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);
            remove = remove_controls(ui, lang, self.confirm_remove == Some(id), &job_title(&job, lang));
        });
        if let Some(ms) = jump {
            self.with_transport(|t| t.seek_ms(ms));
        }
        let edited = job.title != before.title
            || job.author != before.author
            || job.priority != before.priority
            || job.status != before.status
            || job.due != before.due;
        // Field by field: a bookmark added above this frame must not be overwritten by the copy.
        if edited && let Ok(d) = self.session.jobs_mut().get_mut(id) {
            apply_edits(d, &job);
        }
        match remove {
            RemoveRequest::None => {}
            RemoveRequest::Ask => self.confirm_remove = Some(id),
            RemoveRequest::Confirm => self.remove_dictation(id),
            RemoveRequest::Cancel => self.confirm_remove = None,
        }
    }

    /// The notes: the transcriptionist's own words, in the room the window has to spare.
    fn notes(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let Some(id) = self.session.open_id() else { return };
        let Ok(job) = self.session.jobs().get(id) else { return };
        let mut notes = job.notes.clone();
        let before = notes.clone();
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(theme::caption(Text::Notes.in_lang(lang)));
            ui.add_space(4.0);
            ui.add(egui::TextEdit::multiline(&mut notes).desired_rows(10).desired_width(f32::INFINITY).hint_text(Text::NotesHint.in_lang(lang)));
        });
        if notes != before
            && let Ok(d) = self.session.jobs_mut().get_mut(id)
        {
            d.notes = limited(&notes);
        }
    }

    /// The middle of the window: what is playing, big enough to read across the desk.
    fn recording(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let Some(id) = self.session.open_id() else {
            theme::card_frame().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(egui::RichText::new(Text::NoOpen.in_lang(lang)).strong().color(theme::text()));
                ui.label(egui::RichText::new(Text::PickFromQueue.in_lang(lang)).color(theme::muted()));
            });
            return;
        };
        let Ok(job) = self.session.jobs().get(id) else { return };
        let file = job.path.file_name().map_or_else(|| job.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let duration = job.duration_ms.map(|ms| format_timestamp(ms, false)).unwrap_or_else(|| "--:--:--".into());
        let title = if job.title.trim().is_empty() { Text::Untitled.in_lang(lang) } else { job.title.as_str() };
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::caption(Text::Recording.in_lang(lang)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(duration).monospace().color(theme::muted()));
                });
            });
            ui.add_space(6.0);
            ui.label(egui::RichText::new(title).size(20.0).strong().color(theme::text()));
            ui.add_space(2.0);
            ui.label(egui::RichText::new(file).size(12.5).color(theme::muted()));
        });
    }
}

impl eframe::App for App {
    /// Runs before `ui` and also while the window is minimized: the place for a decision that
    /// must not wait for a frame. Closing the window is one — see [`quit`].
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        quit::take_over_close(ctx);
    }

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
                    None => ui.weak(if self.session.is_dirty() { Text::Unsaved.in_lang(self.lang) } else { Text::Ready.in_lang(self.lang) }),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(self.controls.status(self.lang)));
            });
        });
        egui::Panel::bottom("transport").frame(theme::dock_frame()).show(root, |ui| self.transport_dock(ui));
        self.controls.window(ctx, self.lang);
        egui::Panel::left("jobs").frame(theme::side_frame()).resizable(true).default_size(300.0).show(root, |ui| self.job_list(ui));
        if self.session.open_id().is_some() {
            egui::Panel::right("details")
                .frame(theme::side_frame())
                .resizable(true)
                .default_size(280.0)
                .min_size(240.0)
                .max_size(380.0)
                .show(root, |ui| self.details(ui, ctx));
        }
        egui::CentralPanel::default().frame(theme::central_frame()).show(root, |ui| {
            // The dock keeps its height at the bottom; a short window scrolls the rest.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.recording(ui);
                ui.add_space(10.0);
                self.notes(ui);
                ui.add_space(10.0);
                self.settings_row(ui);
            });
        });
        self.remove_window(ctx);
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
    ui.painter().rect_filled(rect, 6.0, theme::accent());
    ui.painter().rect_filled(rect.shrink(5.5), 2.0, theme::accent_ink());
    ui.label(egui::RichText::new("ScribeCraft").size(15.0).strong());
}

/// A label and its widget kept together when the settings row wraps.
fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(label).color(theme::muted()));
        add(ui);
    });
}

/// What the bookmark controls asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BookmarkRequest {
    None,
    Add,
    CopyTime,
}

/// The bookmark label field with the add and copy buttons under it.
///
/// They used to share one line, with the field given `available - 150` points. That reserve
/// held for "Add" + "Copy time", but the Italian "Aggiungi" + "Copia tempo" need about 190:
/// the row then ran 41 points past the panel, and a resizable `Panel` grows to fit whatever
/// its content reports, so it crept from 280 to its limit and clipped every label. Two lines
/// fit in every language, and the field gets the whole width.
fn bookmark_controls(ui: &mut egui::Ui, lang: Lang, label: &mut String) -> BookmarkRequest {
    let width = ui.available_width();
    ui.add(egui::TextEdit::singleline(label).hint_text(Text::BookmarkLabel.in_lang(lang)).desired_width(width));
    let mut request = BookmarkRequest::None;
    // Wrapped, so a translation with even longer button labels grows downwards instead of
    // pushing the panel open again.
    ui.horizontal_wrapped(|ui| {
        if ui.button(Text::Add.in_lang(lang)).on_hover_text("Ctrl+B").clicked() {
            request = BookmarkRequest::Add;
        }
        if ui.button(Text::CopyTime.in_lang(lang)).on_hover_text(Text::CopyTimeHint.in_lang(lang)).clicked() {
            request = BookmarkRequest::CopyTime;
        }
    });
    request
}

/// What the details card asks about the open dictation's place in the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoveRequest {
    None,
    /// The button was pressed: ask before throwing the row away.
    Ask,
    /// The question was answered: remove the dictation from the list.
    Confirm,
    /// The question was answered the other way.
    Cancel,
}

/// The "remove from list" button and the question it asks before anything is thrown away.
/// One click only asks: the recording file is never deleted, and the job list changes only
/// after the transcriptionist confirms.
fn remove_controls(ui: &mut egui::Ui, lang: Lang, asking: bool, title: &str) -> RemoveRequest {
    let danger = |text: &str| {
        egui::Button::new(egui::RichText::new(text).color(theme::red()))
            .fill(theme::card())
            .stroke(egui::Stroke::new(1.0, theme::red()))
            .corner_radius(8)
    };
    if !asking {
        let button = danger(Text::RemoveDictation.in_lang(lang));
        return if ui.add(button).on_hover_text(Text::RemoveHint.in_lang(lang)).clicked() { RemoveRequest::Ask } else { RemoveRequest::None };
    }
    ui.label(egui::RichText::new(i18n::remove_question(title, lang)).size(13.0).color(theme::text()));
    ui.label(egui::RichText::new(Text::RemoveKeepsFile.in_lang(lang)).size(12.0).color(theme::muted()));
    let mut request = RemoveRequest::None;
    // Wrapped, so a translation with longer labels grows downwards instead of widening the panel.
    ui.horizontal_wrapped(|ui| {
        if ui.add(danger(Text::RemoveConfirm.in_lang(lang))).clicked() {
            request = RemoveRequest::Confirm;
        }
        if ui.button(Text::Cancel.in_lang(lang)).clicked() {
            request = RemoveRequest::Cancel;
        }
    });
    request
}

/// One field in the details card: the label keeps a fixed column so the widgets line up.
fn detail_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.allocate_ui_with_layout(egui::vec2(64.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(label).size(12.0).color(theme::muted()));
        });
        add(ui);
    });
}

/// A pill naming the state of the open dictation, coloured so finished work reads at a glance.
fn status_pill(ui: &mut egui::Ui, status: Status, lang: Lang) {
    let (fill, ink) = match status {
        Status::New => (theme::card_hi(), theme::text()),
        Status::InProgress => (theme::accent_dim(), theme::accent_soft()),
        Status::Done => (theme::amber_dim(), theme::amber_soft()),
        Status::Dispatched => (theme::card_hi(), theme::muted()),
    };
    theme::pill_frame(fill).show(ui, |ui| {
        ui.label(egui::RichText::new(job_list::status_name(status, lang)).size(11.5).color(ink));
    });
}

/// A click on a queue row brings it into the transport; it is never started by the click, and
/// clicking the row that is already open changes nothing.
fn opens_on_click(clicked: u64, open: Option<u64>) -> bool {
    open != Some(clicked)
}

/// The title a message uses for a dictation: never an empty pair of quotes.
fn job_title(job: &Dictation, lang: Lang) -> String {
    if job.title.trim().is_empty() { Text::Untitled.in_lang(lang).to_owned() } else { job.title.trim().to_owned() }
}

/// Copies the edited fields back, leaving bookmarks, the path and the saved position alone.
/// Free text is capped so a paste cannot outgrow the job file.
fn apply_edits(target: &mut Dictation, edited: &Dictation) {
    target.title = limited(&edited.title);
    target.author = limited(&edited.author);
    target.priority = edited.priority;
    target.status = edited.status;
    target.due = limited(&edited.due);
    target.notes = limited(&edited.notes);
}

/// A copy of free text that fits the job file; a paste must not outgrow it.
fn limited(text: &str) -> String {
    text.chars().take(scribecraft_engine::MAX_TEXT_CHARS).collect()
}

fn priority_name(p: Priority, lang: Lang) -> &'static str {
    match p {
        Priority::Low => Text::PriorityLow.in_lang(lang),
        Priority::Normal => Text::PriorityNormal.in_lang(lang),
        Priority::High => Text::PriorityHigh.in_lang(lang),
        Priority::Urgent => Text::PriorityUrgent.in_lang(lang),
    }
}

fn channel_name(m: ChannelMode, lang: Lang) -> &'static str {
    match m {
        ChannelMode::Stereo => Text::ChannelStereo.in_lang(lang),
        ChannelMode::Left => Text::ChannelLeft.in_lang(lang),
        ChannelMode::Right => Text::ChannelRight.in_lang(lang),
        ChannelMode::Mix => Text::ChannelMix.in_lang(lang),
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
        theme::apply(&ctx, Theme::Dark);
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

    /// The details card is on the right and scrolls on its own. On a short window the fields
    /// and the bookmark list would otherwise be cut off with no way to reach them.
    #[test]
    fn the_details_panel_scrolls_instead_of_cutting_off_the_bookmarks() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, Theme::Dark);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 240.0));
        let draw = |events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut last = f32::NAN;
            let mut output = ctx.run_ui(input, |ui| {
                egui::Panel::right("details").frame(theme::side_frame()).default_size(280.0).show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for i in 0..30 {
                            let rect = ui.label(format!("bookmark {i}")).rect;
                            if i == 29 {
                                last = rect.top();
                            }
                        }
                    });
                });
            });
            output.textures_delta.clear();
            last
        };
        draw(Vec::new());
        let first = draw(Vec::new());
        assert!(first > screen.bottom(), "the list continues below the panel: {first}");

        // egui applies a wheel event on the frames after it, so settle before measuring.
        let pointer = egui::pos2(screen.right() - 120.0, 60.0);
        draw(vec![egui::Event::PointerMoved(pointer)]);
        draw(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            },
        ]);
        draw(Vec::new());
        let scrolled = draw(Vec::new());
        assert!(scrolled < first - 20.0, "the wheel must bring the rest into view: {first} -> {scrolled}");
    }

    /// The Italian bookmark buttons are wider than the English ones. They used to sit on the
    /// label field's line with a fixed 150 pt reserve, overflowed the details panel by 41 pt,
    /// and the panel grew to its maximum width frame after frame, clipping the whole card.
    /// Whatever the language, the controls must stay inside the panel and the panel must keep
    /// one width.
    #[test]
    fn the_bookmark_controls_fit_the_details_panel_in_both_languages() {
        for lang in [Lang::English, Lang::Italiano] {
            let ctx = egui::Context::default();
            theme::apply(&ctx, Theme::Dark);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
            let draw = || {
                let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let mut frame = None;
                let mut output = ctx.run_ui(input, |ui| {
                    egui::Panel::right("details")
                        .frame(theme::side_frame())
                        .resizable(true)
                        .default_size(280.0)
                        .min_size(240.0)
                        .max_size(380.0)
                        .show(ui, |ui| {
                            // The size the panel settles on for this frame, before its
                            // content can report anything wider.
                            let panel = ui.max_rect();
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 3.0;
                                ui.label(theme::caption(Text::Dictation.in_lang(lang)));
                                let mut field = String::new();
                                for label in [Text::Title, Text::Author, Text::Priority, Text::Status, Text::Due] {
                                    detail_row(ui, label.in_lang(lang), |ui| {
                                        let width = ui.available_width();
                                        ui.add(egui::TextEdit::singleline(&mut field).desired_width(width));
                                    });
                                }
                                ui.separator();
                                let _ = bookmark_controls(ui, lang, &mut field);
                                let _ = remove_controls(ui, lang, true, "Cardiology follow-up");
                                frame = Some((panel.width(), ui.min_rect().right(), panel.right()));
                            });
                        });
                });
                output.textures_delta.clear();
                frame.unwrap_or((f32::NAN, f32::NAN, f32::NAN))
            };

            // The panel stores a content-sized rect and re-reads it next frame, so an
            // overflowing row widens it a little more each time: watch several frames.
            let mut widths = Vec::new();
            for _ in 0..5 {
                let (width, content_right, panel_right) = draw();
                assert!(content_right <= panel_right + 0.5, "{lang:?}: the bookmark row runs {} pt past the panel", content_right - panel_right);
                widths.push(width);
            }
            let first = widths[0];
            assert!(widths.iter().all(|w| (w - first).abs() < 0.5), "{lang:?}: the panel changed width across frames: {widths:?}");
            assert!(first < 280.0, "{lang:?}: the panel is already at its limit: {first}");
        }
    }

    /// The right-click menu flags a dictation completed or back to do; removing only asks.
    #[test]
    fn the_row_menu_flags_completed_or_back_to_do_and_removing_only_asks() {
        use job_list::RowAction;
        assert_eq!(RowAction::MarkDone.status(), Some(Status::Done));
        assert_eq!(RowAction::MarkToDo.status(), Some(Status::InProgress));
        assert_eq!(RowAction::Remove.status(), None, "removing goes through the confirmation, not a status");
        assert_eq!(i18n::marked("Referto", true, Lang::Italiano), "«Referto» segnata come completata");
        assert_eq!(i18n::marked("Referto", false, Lang::Italiano), "«Referto» riportata tra le da fare");
        assert_eq!(i18n::marked("Note", true, Lang::English), "“Note” marked as completed");
    }

    /// The menu offers "mark as completed" on work to do and "back to do" on finished work,
    /// and always the removal, which never removes on its own.
    #[test]
    fn the_row_menu_offers_the_right_status_change() {
        use job_list::{Row, row_menu};
        let ctx = egui::Context::default();
        theme::apply(&ctx, Theme::Dark);
        let texts = |status: Status| {
            let row = Row { id: 1, title: "Note".into(), author: String::new(), priority: Priority::Normal, status, duration_ms: None };
            let input =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 300.0))), ..Default::default() };
            let mut picked = None;
            let mut out = ctx.run_ui(input, |ui| picked = row_menu(ui, &row, Lang::Italiano));
            out.textures_delta.clear();
            assert_eq!(picked, None, "drawing the menu must not act");
            out.shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let open = texts(Status::InProgress);
        assert!(open.iter().any(|t| t == "Segna come completata"), "{open:?}");
        assert!(open.iter().any(|t| t == "Rimuovi dalla coda…"), "{open:?}");
        let done = texts(Status::Done);
        assert!(done.iter().any(|t| t == "Riporta tra le da fare"), "{done:?}");
        assert!(!done.iter().any(|t| t == "Segna come completata"), "{done:?}");
    }

    /// A click on a queue row brings it into the transport without starting playback; a click
    /// on the row that is already open changes nothing.
    #[test]
    fn clicking_a_row_opens_it_but_does_not_start_it() {
        assert!(opens_on_click(2, Some(1)), "another job must open");
        assert!(opens_on_click(1, None), "the first job must open");
        assert!(!opens_on_click(1, Some(1)), "the open job must keep its place and its position");
    }

    /// One frame of the remove controls: the request it produced and the rectangles of the
    /// buttons it painted, left to right.
    fn draw_remove(ctx: &egui::Context, lang: Lang, asking: bool, events: Vec<egui::Event>) -> (RemoveRequest, Vec<egui::Rect>) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 420.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut request = RemoveRequest::None;
        let mut output = ctx.run_ui(input, |ui| {
            request = remove_controls(ui, lang, asking, "Cardiology follow-up");
        });
        let mut rects: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.rect.height() > 16.0 => Some(r.rect),
                _ => None,
            })
            .collect();
        output.textures_delta.clear();
        rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
        (request, rects)
    }

    fn click_at(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() },
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() },
        ]
    }

    /// Removing is never one click: the button only asks, Cancel keeps the job, and only the
    /// confirmation asks `details` to remove it. The recording file is never part of the
    /// question, because it is never touched.
    #[test]
    fn removing_a_dictation_asks_first_and_can_be_cancelled() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, Theme::Dark);
        assert_eq!(draw_remove(&ctx, Lang::English, false, Vec::new()).0, RemoveRequest::None, "the button alone removes nothing");
        let (_, rects) = draw_remove(&ctx, Lang::English, false, Vec::new());
        assert_eq!(rects.len(), 1, "one button before the question: {rects:?}");
        let (asked, _) = draw_remove(&ctx, Lang::English, false, click_at(rects[0].center()));
        assert_eq!(asked, RemoveRequest::Ask, "a click on the button must ask, not remove");

        let (_, rects) = draw_remove(&ctx, Lang::English, true, Vec::new());
        assert_eq!(rects.len(), 2, "Remove and Cancel while the question is shown: {rects:?}");
        let (cancelled, _) = draw_remove(&ctx, Lang::English, true, click_at(rects[1].center()));
        assert_eq!(cancelled, RemoveRequest::Cancel, "Cancel throws nothing away");
        let (confirmed, _) = draw_remove(&ctx, Lang::English, true, click_at(rects[0].center()));
        assert_eq!(confirmed, RemoveRequest::Confirm, "only the confirmation removes the job");
    }

    /// The removal wording names the dictation, and an untitled file is never quoted as an
    /// empty name.
    #[test]
    fn the_removal_wording_names_the_dictation_in_both_languages() {
        assert_eq!(i18n::remove_question("Referto", Lang::Italiano), "Rimuovere «Referto» dalla coda?");
        assert_eq!(i18n::removed("Referto", Lang::Italiano), "«Referto» rimossa dalla coda");
        assert_eq!(i18n::removed("Dictation", Lang::English), "removed “Dictation” from the list");
        let blank = Dictation { title: "   ".into(), ..Default::default() };
        assert_eq!(job_title(&blank, Lang::Italiano), Text::Untitled.in_lang(Lang::Italiano));
        let named = Dictation { title: "  Referto  ".into(), ..Default::default() };
        assert_eq!(job_title(&named, Lang::English), "Referto");
    }

    #[test]
    fn the_change_report_counts_the_recordings_in_both_languages() {
        assert_eq!(i18n::added_recordings(1, Lang::English), "added 1 recording");
        assert_eq!(i18n::added_recordings(3, Lang::English), "added 3 recordings");
        assert_eq!(i18n::added_recordings(1, Lang::Italiano), "aggiunta 1 registrazione");
        assert_eq!(i18n::added_recordings(3, Lang::Italiano), "aggiunte 3 registrazioni");
    }

    /// The details card and the status pill follow the chosen language.
    #[test]
    fn the_open_dictation_is_described_in_the_chosen_language() {
        assert_eq!(priority_name(Priority::Urgent, Lang::Italiano), "urgente");
        assert_eq!(channel_name(ChannelMode::Mix, Lang::Italiano), "mix mono");
        assert_eq!(job_list::status_name(Status::Dispatched, Lang::Italiano), "inviata");
    }

    /// A bookmark added while the frame is open is written straight into the job; the copy of
    /// the edited fields must not bring the old list back, and a paste must not outgrow the file.
    #[test]
    fn applying_edits_leaves_bookmarks_and_the_saved_position_alone() {
        let mut target = Dictation {
            path: PathBuf::from("/tmp/dictation.wav"),
            position_ms: 12_400,
            bookmarks: vec![scribecraft_engine::Bookmark { ms: 5_000, label: "history".into() }],
            ..Dictation::default()
        };
        let mut edited = target.clone();
        edited.status = Status::Done;
        edited.priority = Priority::Urgent;
        edited.title = "t".repeat(scribecraft_engine::MAX_TEXT_CHARS + 30);
        edited.notes = "n".repeat(scribecraft_engine::MAX_TEXT_CHARS + 30);
        apply_edits(&mut target, &edited);
        assert_eq!(target.status, Status::Done);
        assert_eq!(target.priority, Priority::Urgent);
        assert_eq!(target.title.chars().count(), scribecraft_engine::MAX_TEXT_CHARS);
        assert_eq!(target.notes.chars().count(), scribecraft_engine::MAX_TEXT_CHARS);
        assert_eq!(target.bookmarks, edited.bookmarks, "a bookmark added during the frame must survive the copy");
        assert_eq!(target.position_ms, 12_400);
        assert_eq!(target.path, edited.path);
    }
}
