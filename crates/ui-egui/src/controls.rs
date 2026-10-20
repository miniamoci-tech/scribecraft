//! Foot pedal and hotkey control: the "Pedal and keys" window (choose the pedal device,
//! what each pedal does, and which key runs each function), and the shared "current
//! transport" slot that the pedal reader thread and the system-wide hotkey handler act on
//! directly, so both work while another program (the word processor) has the focus.

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
    /// The "Pedal and keys" window is shown.
    pub open: bool,
    pub tab: Tab,
    hotkeys: Option<GlobalHotkeys>,
    hotkey_cfg: HotkeyConfig,
    hotkey_problems: Vec<String>,
    /// Waiting for the user to press the key for this function.
    capturing: Option<PedalAction>,
    notice: Option<String>,
    error: Option<String>,
}

/// Which part of the "Pedal and keys" window is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Pedal,
    Keys,
}

/// Functions offered on the Keys tab, in display order.
const KEY_FUNCTIONS: [PedalAction; 9] = [
    PedalAction::PlayToggle,
    PedalAction::Play,
    PedalAction::Stop,
    PedalAction::Rewind,
    PedalAction::FastForward,
    PedalAction::SkipBack,
    PedalAction::SkipForward,
    PedalAction::Slow,
    PedalAction::Fast,
];

/// Ctrl, Shift, Alt and the Windows/Command keys pressed on their own.
fn is_modifier_key(key: egui::Key) -> bool {
    let name = key.name();
    ["Control", "Shift", "Alt", "Meta", "Super", "Command", "Option"].iter().any(|m| name.starts_with(m))
}

fn is_function_key(key: egui::Key) -> bool {
    let name = key.name();
    name.len() >= 2 && name.starts_with('F') && name.get(1..).is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))
}

/// The binding text (e.g. `"Ctrl+Shift+P"`, `"F9"`) for a key pressed while capturing.
/// Function keys may be used alone; any other key needs Ctrl or Alt, because a plain letter,
/// digit or space bound system-wide would stop it from being typed in the word processor.
pub(crate) fn binding_text(key: egui::Key, m: egui::Modifiers) -> Result<String, String> {
    use egui::Key;
    let name = key.name();
    let plain_ok = is_function_key(key);
    let bindable = plain_ok
        || (name.len() == 1 && name.bytes().all(|b| b.is_ascii_alphanumeric()))
        || matches!(
            key,
            Key::Space
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::ArrowLeft
                | Key::ArrowRight
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Insert
                | Key::Delete
        );
    if !bindable {
        return Err(format!("{name} cannot be used; choose a function key (F1-F20) or a letter, digit or arrow with Ctrl or Alt"));
    }
    if !plain_ok && !(m.ctrl || m.alt || m.mac_cmd) {
        return Err(format!("{name} alone would stop that key from typing; hold Ctrl or Alt with it, or use a function key"));
    }
    let mut text = String::new();
    for (on, part) in [(m.ctrl, "Ctrl+"), (m.alt, "Alt+"), (m.shift, "Shift+"), (m.mac_cmd, "Cmd+")] {
        if on {
            text.push_str(part);
        }
    }
    text.push_str(name);
    Ok(text)
}

/// Reads binding text back into (ctrl, alt, shift, key), for use inside the window. Accepts
/// the `KeyA` / `Digit1` spellings too.
pub(crate) fn parse_binding(text: &str) -> Option<(bool, bool, bool, egui::Key)> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key = parts.pop()?;
    let key = key.strip_prefix("Key").filter(|k| k.len() == 1).or_else(|| key.strip_prefix("Digit")).unwrap_or(key);
    let key = egui::Key::from_name(key)?;
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    for p in parts {
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" | "cmdorctrl" | "commandorcontrol" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    Some((ctrl, alt, shift, key))
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
            open: false,
            tab: Tab::Pedal,
            hotkeys: None,
            hotkey_cfg,
            hotkey_problems: Vec::new(),
            capturing: None,
            notice: None,
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
        if self.global_active() || self.capturing.is_some() {
            return None;
        }
        let pressed = (modifiers.ctrl || modifiers.mac_cmd, modifiers.alt, modifiers.shift, key);
        self.hotkey_cfg.bindings.iter().find(|b| parse_binding(&b.keys) == Some(pressed)).map(|b| b.action)
    }

    /// Whether the window is waiting for a key to assign (the app must not act on keys then).
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
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

    /// The "Pedal and keys" window.
    pub fn window(&mut self, ctx: &egui::Context) {
        if !self.open {
            if self.capturing.take().is_some() {
                self.apply_hotkeys();
            }
            return;
        }
        let mut open = self.open;
        egui::Window::new("Pedal and keys").open(&mut open).resizable(true).default_width(540.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Pedal, "Foot pedal");
                ui.selectable_value(&mut self.tab, Tab::Keys, "Keys");
            });
            ui.separator();
            match self.tab {
                Tab::Pedal => self.pedal_tab(ui),
                Tab::Keys => self.keys_tab(ui, ctx),
            }
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        self.open = open;
        if open && (lock(&self.pedal).learning().is_some() || self.capturing.is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn pedal_tab(&mut self, ui: &mut egui::Ui) {
        ui.label("1. Connect the pedal (USB or serial/COM port) and choose it. 2. For each pedal click Learn and press it. 3. Choose what it does.");
        ui.horizontal(|ui| {
            if self.devices.is_empty() || ui.button("Refresh").clicked() {
                self.devices = list_devices();
            }
            let current = lock(&self.pedal).config().source.clone();
            let label = match &current {
                PedalSource::None => "no pedal".to_owned(),
                PedalSource::Serial { port } => format!("Serial: {port}"),
                PedalSource::Hid { vendor_id, product_id, name, .. } => format!("USB: {name} ({vendor_id:04x}:{product_id:04x})"),
            };
            let mut chosen: Option<PedalSource> = None;
            egui::ComboBox::from_label("device").selected_text(label).width(340.0).show_ui(ui, |ui| {
                if ui.selectable_label(current == PedalSource::None, "no pedal").clicked() {
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
                let cfg = lock(&self.pedal).config().clone();
                let cfg = match &source {
                    // A serial pedal starts from the standard wiring; a USB pedal is learned.
                    PedalSource::Serial { port } if cfg.buttons.is_empty() || matches!(cfg.source, PedalSource::Hid { .. }) => {
                        PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) }
                    }
                    PedalSource::Hid { .. } if matches!(cfg.source, PedalSource::Serial { .. }) => {
                        PedalConfig { source, buttons: Vec::new(), play_mode: cfg.play_mode }
                    }
                    _ => PedalConfig { source, ..cfg },
                };
                if let Err(e) = lock(&self.pedal).set_config(cfg) {
                    self.error = Some(e.to_string());
                }
                self.save_pedal();
                self.restart_reader();
            }
        });
        match self.reader.as_ref().map(PedalReader::status) {
            Some(ReaderStatus::Waiting(why)) => {
                ui.colored_label(ui.visuals().warn_fg_color, why);
            }
            Some(ReaderStatus::Connected) => {
                ui.label("connected: press a pedal to see it light up below");
            }
            Some(s) => {
                ui.label(format!("{s:?}").to_lowercase());
            }
            None => {
                ui.weak("no pedal selected");
            }
        }
        ui.separator();

        let mut c = lock(&self.pedal);
        let mut cfg = c.config().clone();
        let pressed = c.pressed();
        let learning = c.learning();
        let mut changed = false;
        let mut learn: Option<usize> = None;
        let mut remove: Option<usize> = None;
        ui.horizontal(|ui| {
            ui.label("Play pedal:");
            changed |= ui.radio_value(&mut cfg.play_mode, PlayMode::Hold, "plays while held, stops when released").changed();
            changed |= ui.radio_value(&mut cfg.play_mode, PlayMode::Toggle, "press to play, press again to stop").changed();
        });
        egui::Grid::new("pedals").num_columns(5).striped(true).show(ui, |ui| {
            ui.strong("Pedal");
            ui.strong("");
            ui.strong("Function");
            ui.end_row();
            let n = cfg.buttons.len();
            for (i, b) in cfg.buttons.iter_mut().enumerate() {
                let down = pressed.get(i).copied().unwrap_or(false);
                ui.label(format!("Pedal {}", i + 1));
                ui.label(if down { "● pressed" } else { "○" });
                changed |= action_combo(ui, ("pedal-action", i), &mut b.action);
                if ui.button("Learn again").clicked() {
                    learn = Some(i);
                }
                if ui.small_button("remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
            if n < scribecraft_engine::pedal::MAX_PEDALS && ui.button("Learn a new pedal").clicked() {
                learn = Some(n);
            }
            ui.end_row();
        });
        if let Some(i) = remove
            && i < cfg.buttons.len()
        {
            cfg.buttons.remove(i);
            changed = true;
        }
        if let Some(i) = learning {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Release every pedal, then press pedal {} now…", i + 1));
            if ui.button("Cancel learning").clicked() {
                c.cancel_learning();
            }
        }
        if let PedalSource::Serial { port } = &cfg.source
            && ui.button("Standard serial wiring: pin 1 play, pin 6 rewind, pins 8/9 fast-forward").clicked()
        {
            cfg = PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) };
            changed = true;
        }
        ui.weak(format!("last report from the device: {:02x?}", c.last_report()));
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
    }

    fn keys_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut changed = false;
        if ui.checkbox(&mut self.hotkey_cfg.global, "Keys work while another program (e.g. the word processor) is in front").changed() {
            changed = true;
        }
        ui.weak("Click Assign, then press the key or combination. Function keys work alone; other keys need Ctrl or Alt.");
        if let Some(action) = self.capturing {
            ui.colored_label(ui.visuals().warn_fg_color, format!("Press the key for \"{}\" (Esc to cancel)…", action_name(action)));
            // Modifier keys arrive as key events of their own; wait for the key they modify.
            let pressed = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if !is_modifier_key(*key) => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, mods)) = pressed {
                if key == egui::Key::Escape {
                    self.capturing = None;
                    changed = true;
                } else {
                    match binding_text(key, mods) {
                        Ok(text) => {
                            self.notice = self.assign(action, &text);
                            self.capturing = None;
                            changed = true;
                        }
                        Err(why) => self.notice = Some(why),
                    }
                }
            }
        }
        let mut capture: Option<PedalAction> = None;
        let mut clear: Option<PedalAction> = None;
        egui::Grid::new("keys").num_columns(4).striped(true).show(ui, |ui| {
            ui.strong("Function");
            ui.strong("Key");
            ui.end_row();
            for a in KEY_FUNCTIONS {
                let keys: Vec<&str> = self.hotkey_cfg.bindings.iter().filter(|b| b.action == a).map(|b| b.keys.as_str()).collect();
                ui.label(action_name(a));
                ui.monospace(if keys.is_empty() { "—".to_owned() } else { keys.join(" / ") });
                if ui.button(if self.capturing == Some(a) { "press a key…" } else { "Assign…" }).clicked() {
                    capture = Some(a);
                }
                if !keys.is_empty() && ui.small_button("clear").clicked() {
                    clear = Some(a);
                }
                ui.end_row();
            }
        });
        if let Some(a) = capture {
            // Unregister the system-wide keys while capturing, or the key being pressed would
            // go to playback instead of to this window.
            if let Some(h) = &mut self.hotkeys {
                h.clear();
            }
            self.capturing = Some(a);
            self.notice = None;
        }
        if let Some(a) = clear {
            self.hotkey_cfg.bindings.retain(|b| b.action != a);
            changed = true;
        }
        if ui.button("Restore the default keys").clicked() {
            self.hotkey_cfg = HotkeyConfig { global: self.hotkey_cfg.global, ..HotkeyConfig::default() };
            changed = true;
        }
        if changed && self.capturing.is_none() {
            self.apply_hotkeys();
            if let Some(d) = &self.dir
                && let Err(e) = self.hotkey_cfg.save(&d.join("hotkeys.json"))
            {
                self.hotkey_problems.push(e.to_string());
            }
        }
        if let Some(n) = &self.notice {
            ui.label(n);
        }
        for p in &self.hotkey_problems {
            ui.colored_label(ui.visuals().warn_fg_color, p);
        }
    }

    /// Gives `action` the key `text` (one key per function). A key used by another function
    /// moves here; the message says so.
    fn assign(&mut self, action: PedalAction, text: &str) -> Option<String> {
        let wanted = parse_binding(text);
        let same = |b: &HotkeyBinding| wanted.is_some() && parse_binding(&b.keys) == wanted;
        let moved_from = self.hotkey_cfg.bindings.iter().find(|b| b.action != action && same(b)).map(|b| b.action);
        self.hotkey_cfg.bindings.retain(|b| b.action != action && !same(b));
        if self.hotkey_cfg.bindings.len() >= scribecraft_engine::hotkeys::MAX_BINDINGS {
            return Some("too many keys; clear one first".into());
        }
        self.hotkey_cfg.bindings.push(HotkeyBinding { keys: text.to_owned(), action });
        Some(match moved_from {
            Some(old) => format!("{text} now runs \"{}\" (it no longer runs \"{}\")", action_name(action), action_name(old)),
            None => format!("{text} now runs \"{}\"", action_name(action)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    #[test]
    fn captured_keys_become_bindings_that_read_back() {
        assert_eq!(binding_text(Key::F9, Modifiers::NONE).unwrap(), "F9");
        let ctrl_shift = Modifiers { ctrl: true, shift: true, ..Modifiers::NONE };
        assert_eq!(binding_text(Key::P, ctrl_shift).unwrap(), "Ctrl+Shift+P");
        assert_eq!(parse_binding("Ctrl+Shift+P"), Some((true, false, true, Key::P)));
        assert_eq!(parse_binding("Ctrl+Shift+KeyP"), Some((true, false, true, Key::P)));
        assert_eq!(parse_binding("F9"), Some((false, false, false, Key::F9)));
        assert_eq!(parse_binding("Alt+Digit1"), Some((false, true, false, Key::Num1)));
        assert_eq!(parse_binding("Hyper+F9"), None);
    }

    #[test]
    fn keys_that_would_block_typing_are_refused() {
        assert!(binding_text(Key::A, Modifiers::NONE).is_err());
        assert!(binding_text(Key::Space, Modifiers::SHIFT).is_err());
        assert!(binding_text(Key::Escape, Modifiers::CTRL).is_err());
        assert!(binding_text(Key::A, Modifiers::ALT).is_ok());
    }

    #[test]
    fn modifier_keys_alone_are_not_captured() {
        for k in [Key::ControlLeft, Key::ControlRight, Key::ShiftLeft, Key::AltRight, Key::SuperLeft] {
            assert!(is_modifier_key(k), "{k:?}");
        }
        for k in [Key::P, Key::F9, Key::Space] {
            assert!(!is_modifier_key(k), "{k:?}");
        }
    }
}
