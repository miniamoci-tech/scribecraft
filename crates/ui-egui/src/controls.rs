//! Foot pedal and hotkey control: settings windows, and the shared "current transport" slot
//! that the pedal reader thread and the system-wide hotkey handler act on directly, so both
//! work while another program (the word processor) has the focus.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui;
use scribecraft_engine::hotkeys::{HotkeyBinding, HotkeyConfig, apply_hotkey};
use scribecraft_engine::pedal::{PedalAction, PedalConfig, PedalController, PedalSource, PlayMode};
use scribecraft_engine::{Transport, lock_transport};
use scribecraft_platform::hotkeys::GlobalHotkeys;
use scribecraft_platform::pedal::{PedalDevice, PedalReader, ReaderStatus, list_devices};

type Slot = Arc<Mutex<Option<Arc<Mutex<Transport>>>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn action_name(a: PedalAction) -> &'static str {
    match a {
        PedalAction::None => "nothing",
        PedalAction::Rewind => "rewind (hold)",
        PedalAction::Play => "play",
        PedalAction::FastForward => "fast-forward (hold)",
        PedalAction::Stop => "stop",
        PedalAction::PlayToggle => "play / stop",
        PedalAction::SkipBack => "skip back",
        PedalAction::SkipForward => "skip forward",
        PedalAction::Slow => "slow speed",
        PedalAction::Fast => "fast speed",
    }
}

fn action_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut PedalAction) -> bool {
    let before = *value;
    egui::ComboBox::from_id_salt(id).selected_text(action_name(*value)).show_ui(ui, |ui| {
        for a in PedalAction::ALL {
            ui.selectable_value(value, a, action_name(a));
        }
    });
    *value != before
}

pub(crate) struct Controls {
    ctx: egui::Context,
    dir: Option<PathBuf>,
    slot: Slot,
    pedal: Arc<Mutex<PedalController>>,
    reader: Option<PedalReader>,
    devices: Vec<PedalDevice>,
    pub pedal_open: bool,
    hotkeys: Option<GlobalHotkeys>,
    hotkey_cfg: HotkeyConfig,
    hotkey_draft: HotkeyConfig,
    hotkey_problems: Vec<String>,
    pub hotkeys_open: bool,
    error: Option<String>,
}

impl Controls {
    /// Loads the saved pedal and hotkey settings and starts them. Call on the UI thread.
    pub fn new(ctx: egui::Context, dir: Option<PathBuf>) -> Self {
        let mut error = None;
        let pedal_cfg = match dir.as_ref().map(|d| PedalConfig::load(&d.join("pedal.json"))) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                error = Some(e.to_string());
                PedalConfig::default()
            }
            None => PedalConfig::default(),
        };
        let hotkey_cfg = match dir.as_ref().map(|d| HotkeyConfig::load(&d.join("hotkeys.json"))) {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                error = Some(e.to_string());
                HotkeyConfig::default()
            }
            None => HotkeyConfig::default(),
        };
        let pedal = Arc::new(Mutex::new(PedalController::new(pedal_cfg).unwrap_or_default()));
        let mut c = Self {
            ctx,
            dir,
            slot: Arc::new(Mutex::new(None)),
            pedal,
            reader: None,
            devices: Vec::new(),
            pedal_open: false,
            hotkeys: None,
            hotkey_draft: hotkey_cfg.clone(),
            hotkey_cfg,
            hotkey_problems: Vec::new(),
            hotkeys_open: false,
            error,
        };
        c.restart_reader();
        c.apply_hotkeys();
        c
    }

    /// The transport the pedal and hotkeys control (the open dictation), or none.
    pub fn set_transport(&self, t: Option<Arc<Mutex<Transport>>>) {
        *lock(&self.slot) = t;
    }

    /// Whether the transport keys are registered system-wide (then the window must not
    /// handle them a second time).
    pub fn global_active(&self) -> bool {
        self.hotkeys.as_ref().is_some_and(|h| !h.is_empty())
    }

    /// The action bound to `key` for use inside the window when hotkeys are not global.
    pub fn window_action(&self, key: egui::Key, modifiers: egui::Modifiers) -> Option<PedalAction> {
        if self.global_active() || modifiers.any() {
            return None;
        }
        self.hotkey_cfg.bindings.iter().find(|b| egui::Key::from_name(b.keys.trim()) == Some(key)).map(|b| b.action)
    }

    /// Short text for the toolbar: which keys do what.
    pub fn key_hint(&self) -> String {
        let keys: Vec<String> = self.hotkey_cfg.bindings.iter().map(|b| format!("{} {}", b.keys, action_name(b.action))).collect();
        let scope = if self.global_active() { "everywhere" } else { "in this window" };
        format!("{} ({scope})", keys.join(" · "))
    }

    /// One line for the status bar.
    pub fn status(&self) -> String {
        let pedal = match self.reader.as_ref().map(PedalReader::status) {
            None => "pedal: none".to_owned(),
            Some(ReaderStatus::Connected) => "pedal: connected".to_owned(),
            Some(ReaderStatus::Connecting) => "pedal: connecting".to_owned(),
            Some(ReaderStatus::Waiting(_)) => "pedal: waiting for the device".to_owned(),
            Some(ReaderStatus::Stopped) => "pedal: stopped".to_owned(),
        };
        let keys = match &self.hotkeys {
            Some(h) if !h.is_empty() => format!("{} system-wide hotkeys", h.len()),
            _ => "hotkeys: window only".to_owned(),
        };
        format!("{pedal} · {keys}")
    }

    fn save_pedal(&mut self) {
        if let Some(d) = &self.dir {
            let cfg = lock(&self.pedal).config().clone();
            if let Err(e) = cfg.save(&d.join("pedal.json")) {
                self.error = Some(e.to_string());
            }
        }
    }

    fn restart_reader(&mut self) {
        self.reader = None;
        let source = lock(&self.pedal).config().source.clone();
        if source == PedalSource::None {
            return;
        }
        let (pedal, slot, ctx) = (self.pedal.clone(), self.slot.clone(), self.ctx.clone());
        let started = PedalReader::start(&source, move |report| {
            let transport = lock(&slot).clone();
            let mut c = lock(&pedal);
            match transport {
                Some(t) => {
                    c.feed(report, Some(&mut lock_transport(&t)));
                }
                None => {
                    c.feed(report, None);
                }
            }
            ctx.request_repaint();
        });
        match started {
            Ok(r) => self.reader = Some(r),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn apply_hotkeys(&mut self) {
        self.hotkey_problems.clear();
        if !self.hotkey_cfg.global {
            self.hotkeys = None;
            return;
        }
        if self.hotkeys.is_none() {
            match GlobalHotkeys::new() {
                Ok(h) => self.hotkeys = Some(h),
                Err(e) => {
                    self.hotkey_problems.push(e.to_string());
                    return;
                }
            }
        }
        let (slot, ctx) = (self.slot.clone(), self.ctx.clone());
        if let Some(h) = &mut self.hotkeys {
            self.hotkey_problems = h.set(&self.hotkey_cfg.bindings, move |action, pressed| {
                if let Some(t) = lock(&slot).clone() {
                    apply_hotkey(action, pressed, &mut lock_transport(&t));
                }
                ctx.request_repaint();
            });
        }
    }

    pub fn pedal_window(&mut self, ctx: &egui::Context) {
        let mut open = self.pedal_open;
        egui::Window::new("Foot pedal").open(&mut open).resizable(true).default_width(460.0).show(ctx, |ui| {
            ui.label("Connect a USB foot pedal or a pedal on a serial (COM) port, choose it here, then teach ScribeCraft each pedal.");
            if self.devices.is_empty() || ui.button("Refresh device list").clicked() {
                self.devices = list_devices();
            }
            let current = lock(&self.pedal).config().source.clone();
            let label = match &current {
                PedalSource::None => "none".to_owned(),
                PedalSource::Serial { port } => format!("Serial: {port}"),
                PedalSource::Hid { vendor_id, product_id, name, .. } => format!("USB: {name} ({vendor_id:04x}:{product_id:04x})"),
            };
            let mut chosen: Option<PedalSource> = None;
            egui::ComboBox::from_label("device").selected_text(label).width(320.0).show_ui(ui, |ui| {
                if ui.selectable_label(current == PedalSource::None, "none").clicked() {
                    chosen = Some(PedalSource::None);
                }
                for d in &self.devices {
                    let text = if d.likely_pedal { format!("★ {}", d.label) } else { d.label.clone() };
                    if ui.selectable_label(d.source == current, text).clicked() {
                        chosen = Some(d.source.clone());
                    }
                }
            });
            if let Some(source) = chosen {
                let mut cfg = lock(&self.pedal).config().clone();
                let was_serial_default = cfg.buttons.is_empty();
                cfg = match (&source, was_serial_default) {
                    (PedalSource::Serial { port }, true) => PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) },
                    _ => PedalConfig { source, ..cfg },
                };
                if let Err(e) = lock(&self.pedal).set_config(cfg) {
                    self.error = Some(e.to_string());
                }
                self.save_pedal();
                self.restart_reader();
            }
            if let Some(r) = &self.reader {
                match r.status() {
                    ReaderStatus::Waiting(why) => {
                        ui.colored_label(ui.visuals().warn_fg_color, why);
                    }
                    s => {
                        ui.label(format!("status: {s:?}").to_lowercase());
                    }
                }
            }
            ui.separator();

            let mut c = lock(&self.pedal);
            let mut cfg = c.config().clone();
            let pressed = c.pressed();
            let learning = c.learning();
            let mut changed = false;
            let mut learn: Option<usize> = None;
            egui::ComboBox::from_label("play pedal")
                .selected_text(if cfg.play_mode == PlayMode::Hold { "plays while held" } else { "press to play / stop" })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut cfg.play_mode, PlayMode::Hold, "plays while held").changed();
                    changed |= ui.selectable_value(&mut cfg.play_mode, PlayMode::Toggle, "press to play / stop").changed();
                });
            egui::Grid::new("pedals").num_columns(4).striped(true).show(ui, |ui| {
                let n = cfg.buttons.len();
                for (i, b) in cfg.buttons.iter_mut().enumerate() {
                    let down = pressed.get(i).copied().unwrap_or(false);
                    ui.label(format!("Pedal {}", i + 1));
                    ui.label(if down { "● down" } else { "○" });
                    changed |= action_combo(ui, ("pedal-action", i), &mut b.action);
                    if ui.button("Learn again").clicked() {
                        learn = Some(i);
                    }
                    ui.end_row();
                }
                if n < scribecraft_engine::pedal::MAX_PEDALS && ui.button("Learn a new pedal").clicked() {
                    learn = Some(n);
                }
                ui.end_row();
            });
            if let Some(i) = learning {
                ui.colored_label(ui.visuals().warn_fg_color, format!("Release every pedal, then press pedal {} now…", i + 1));
                if ui.button("Cancel learning").clicked() {
                    c.cancel_learning();
                }
            }
            if let PedalSource::Serial { port } = &cfg.source
                && ui.button("Use standard serial wiring (pin 1 play, pin 6 rewind, pins 8/9 forward)").clicked()
            {
                cfg = PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) };
                changed = true;
            }
            ui.weak(format!("last report: {:02x?}", c.last_report()));
            if changed && let Err(e) = c.set_config(cfg) {
                self.error = Some(e.to_string());
            }
            if let Some(i) = learn
                && let Err(e) = c.start_learning(i)
            {
                self.error = Some(e.to_string());
            }
            drop(c);
            if changed || learning.is_some() {
                self.save_pedal();
            }
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        self.pedal_open = open;
        if open && lock(&self.pedal).learning().is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub fn hotkeys_window(&mut self, ctx: &egui::Context) {
        let mut open = self.hotkeys_open;
        egui::Window::new("Hotkeys").open(&mut open).resizable(true).default_width(420.0).show(ctx, |ui| {
            ui.checkbox(&mut self.hotkey_draft.global, "Work while another program is in front (system-wide)");
            ui.weak("Key names: F1–F24, KeyA–KeyZ, Digit0–9, with Ctrl+, Shift+, Alt+ (e.g. Ctrl+Shift+KeyP). On Linux this needs X11.");
            let mut remove = None;
            egui::Grid::new("hotkeys").num_columns(3).show(ui, |ui| {
                for (i, b) in self.hotkey_draft.bindings.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut b.keys).desired_width(140.0));
                    action_combo(ui, ("hotkey-action", i), &mut b.action);
                    if ui.small_button("remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove
                && i < self.hotkey_draft.bindings.len()
            {
                self.hotkey_draft.bindings.remove(i);
            }
            ui.horizontal(|ui| {
                if self.hotkey_draft.bindings.len() < scribecraft_engine::hotkeys::MAX_BINDINGS && ui.button("Add key").clicked() {
                    self.hotkey_draft.bindings.push(HotkeyBinding { keys: "F12".into(), action: PedalAction::PlayToggle });
                }
                if ui.button("Restore defaults").clicked() {
                    self.hotkey_draft = HotkeyConfig::default();
                }
                if ui.button("Apply").clicked() {
                    match self.hotkey_draft.validate() {
                        Ok(()) => {
                            self.hotkey_cfg = self.hotkey_draft.clone();
                            self.apply_hotkeys();
                            if let Some(d) = &self.dir
                                && let Err(e) = self.hotkey_cfg.save(&d.join("hotkeys.json"))
                            {
                                self.hotkey_problems.push(e.to_string());
                            }
                        }
                        Err(e) => self.hotkey_problems = vec![e.to_string()],
                    }
                }
            });
            for p in &self.hotkey_problems {
                ui.colored_label(ui.visuals().warn_fg_color, p);
            }
        });
        self.hotkeys_open = open;
    }
}
