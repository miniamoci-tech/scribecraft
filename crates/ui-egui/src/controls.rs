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

use crate::i18n::{self, Lang, Text};

type Slot = Arc<Mutex<Option<Arc<Mutex<Transport>>>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn action_name(a: PedalAction, lang: Lang) -> &'static str {
    match a {
        PedalAction::None => Text::ActionNone.in_lang(lang),
        PedalAction::Rewind => Text::ActionRewind.in_lang(lang),
        PedalAction::Play => Text::ActionPlay.in_lang(lang),
        PedalAction::FastForward => Text::ActionFastForward.in_lang(lang),
        PedalAction::Stop => Text::ActionStop.in_lang(lang),
        PedalAction::PlayToggle => Text::ActionPlayToggle.in_lang(lang),
        PedalAction::SkipBack => Text::ActionSkipBack.in_lang(lang),
        PedalAction::SkipForward => Text::ActionSkipForward.in_lang(lang),
        PedalAction::Slow => Text::ActionSlow.in_lang(lang),
        PedalAction::Fast => Text::ActionFast.in_lang(lang),
    }
}

fn action_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut PedalAction, lang: Lang) -> bool {
    let before = *value;
    egui::ComboBox::from_id_salt(id).selected_text(action_name(*value, lang)).show_ui(ui, |ui| {
        for a in PedalAction::ALL {
            ui.selectable_value(value, a, action_name(a, lang));
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
    wizard: Wizard,
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

/// The steps of the guided pedal setup, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WizardStep {
    #[default]
    Device,
    Learn,
    Assign,
}

impl WizardStep {
    /// 1, 2, 3, for the step counter.
    pub(crate) fn number(self) -> usize {
        match self {
            Self::Device => 1,
            Self::Learn => 2,
            Self::Assign => 3,
        }
    }
}

/// The guided pedal setup: choose the device, press each pedal once, say what it does.
///
/// Kept apart from the widgets so the order and its guards can be tested without a window, a
/// device or a global hotkey registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Wizard {
    pub open: bool,
    pub step: WizardStep,
}

impl Wizard {
    /// Opens at the first step, whatever an earlier run left behind.
    pub(crate) fn start(&mut self) {
        self.open = true;
        self.step = WizardStep::Device;
    }

    /// The step after this one, moving to it; `None` on the last step, which stays put.
    pub(crate) fn next(&mut self) -> Option<WizardStep> {
        let next = match self.step {
            WizardStep::Device => WizardStep::Learn,
            WizardStep::Learn => WizardStep::Assign,
            WizardStep::Assign => return None,
        };
        self.step = next;
        Some(next)
    }

    /// The step before this one; `None` on the first step, which stays put.
    pub(crate) fn back(&mut self) -> Option<WizardStep> {
        let back = match self.step {
            WizardStep::Device => return None,
            WizardStep::Learn => WizardStep::Device,
            WizardStep::Assign => WizardStep::Learn,
        };
        self.step = back;
        Some(back)
    }

    pub(crate) fn last(self) -> bool {
        self.step == WizardStep::Assign
    }
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
pub(crate) fn binding_text(key: egui::Key, m: egui::Modifiers, lang: Lang) -> Result<String, String> {
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
        return Err(i18n::fill(Text::KeyCannotUse.in_lang(lang), &[("{key}", name)]));
    }
    if !plain_ok && !(m.ctrl || m.alt || m.mac_cmd) {
        return Err(i18n::fill(Text::KeyNeedsModifier.in_lang(lang), &[("{key}", name)]));
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
            wizard: Wizard::default(),
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
    pub fn key_hint(&self, lang: Lang) -> String {
        let keys: Vec<String> = self.hotkey_cfg.bindings.iter().map(|b| format!("{} {}", b.keys, action_name(b.action, lang))).collect();
        let scope = if self.global_active() { Text::Everywhere } else { Text::InThisWindow };
        format!("{} ({})", keys.join(" · "), scope.in_lang(lang))
    }

    /// One line for the status bar.
    pub fn status(&self, lang: Lang) -> String {
        let pedal = match self.reader.as_ref().map(PedalReader::status) {
            None => Text::PedalNone.in_lang(lang).to_owned(),
            Some(ReaderStatus::Connected) => Text::PedalConnected.in_lang(lang).to_owned(),
            Some(ReaderStatus::Connecting) => Text::PedalConnecting.in_lang(lang).to_owned(),
            Some(ReaderStatus::Waiting(_)) => Text::PedalWaiting.in_lang(lang).to_owned(),
            Some(ReaderStatus::Stopped) => Text::PedalStopped.in_lang(lang).to_owned(),
        };
        let keys = match &self.hotkeys {
            Some(h) if !h.is_empty() => i18n::hotkey_count(h.len(), lang),
            _ => Text::HotkeysWindow.in_lang(lang).to_owned(),
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
    pub fn window(&mut self, ctx: &egui::Context, lang: Lang) {
        if !self.open {
            // Closing the window closes the wizard with it.
            self.wizard.open = false;
            if self.capturing.take().is_some() {
                self.apply_hotkeys();
            }
            return;
        }
        let mut open = self.open;
        egui::Window::new(Text::PedalAndKeysTitle.in_lang(lang)).open(&mut open).resizable(true).default_width(540.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Pedal, Text::FootPedalTab.in_lang(lang));
                ui.selectable_value(&mut self.tab, Tab::Keys, Text::KeysTab.in_lang(lang));
            });
            ui.separator();
            match self.tab {
                Tab::Pedal => self.pedal_tab(ui, lang),
                Tab::Keys => self.keys_tab(ui, ctx, lang),
            }
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        self.open = open;
        if self.open {
            self.wizard_window(ctx, lang);
        }
        if self.open && (lock(&self.pedal).learning().is_some() || self.capturing.is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn pedal_tab(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(Text::PedalIntro.in_lang(lang));
        ui.horizontal(|ui| {
            if ui.button(Text::WizardOpen.in_lang(lang)).clicked() {
                self.wizard.start();
            }
            if self.devices.is_empty() || ui.button(Text::Refresh.in_lang(lang)).clicked() {
                self.devices = list_devices();
            }
            let current = lock(&self.pedal).config().source.clone();
            if let Some(source) = device_picker(ui, &self.devices, &current, lang) {
                self.set_source(source);
            }
        });
        self.reader_status(ui, lang);
        ui.separator();

        let mut c = lock(&self.pedal);
        let mut cfg = c.config().clone();
        let pressed = c.pressed();
        let learning = c.learning();
        let mut changed = play_mode_radios(ui, &mut cfg, lang);
        let edits = pedal_grid(ui, &mut cfg, &pressed, lang);
        changed |= edits.changed;
        if let Some(i) = edits.remove
            && i < cfg.buttons.len()
        {
            cfg.buttons.remove(i);
            changed = true;
        }
        if let Some(i) = learning {
            ui.colored_label(ui.visuals().warn_fg_color, learning_prompt(c.learning_armed(), i, lang));
            if ui.button(Text::CancelLearning.in_lang(lang)).clicked() {
                c.cancel_learning();
            }
        }
        if let PedalSource::Serial { port } = &cfg.source
            && ui.button(Text::SerialWiring.in_lang(lang)).clicked()
        {
            cfg = PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) };
            changed = true;
        }
        ui.weak(i18n::fill(Text::LastReport.in_lang(lang), &[("{report}", &format!("{:02x?}", c.last_report()))]));
        if changed && let Err(e) = c.set_config(cfg) {
            self.error = Some(e.to_string());
        }
        if let Some(i) = edits.learn
            && let Err(e) = c.start_learning(i)
        {
            self.error = Some(e.to_string());
        }
        drop(c);
        if changed || learning.is_some() {
            self.save_pedal();
        }
    }

    /// Chooses a device: the configuration follows it (serial: standard wiring; USB: learn
    /// each pedal), the reader reconnects and the choice is saved.
    fn set_source(&mut self, source: PedalSource) {
        let cfg = lock(&self.pedal).config().clone();
        let cfg = config_for_source(&cfg, source);
        if let Err(e) = lock(&self.pedal).set_config(cfg) {
            self.error = Some(e.to_string());
        }
        self.save_pedal();
        self.restart_reader();
    }

    /// The reader's state, in one line.
    fn reader_status(&self, ui: &mut egui::Ui, lang: Lang) {
        match self.reader.as_ref().map(PedalReader::status) {
            Some(ReaderStatus::Waiting(why)) => {
                ui.colored_label(ui.visuals().warn_fg_color, why);
            }
            Some(ReaderStatus::Connected) => {
                ui.label(Text::ConnectedPress.in_lang(lang));
            }
            Some(s) => {
                ui.label(format!("{s:?}").to_lowercase());
            }
            None => {
                ui.weak(Text::NoPedalSelected.in_lang(lang));
            }
        }
    }

    /// The guided setup: a step at a time, writing the same `pedal.json` as the tab.
    fn wizard_window(&mut self, ctx: &egui::Context, lang: Lang) {
        if !self.wizard.open {
            return;
        }
        let mut open = true;
        egui::Window::new(Text::WizardTitle.in_lang(lang)).open(&mut open).collapsible(false).resizable(true).default_width(560.0).show(ctx, |ui| {
            let current = self.wizard.step;
            ui.horizontal(|ui| {
                for (step, text) in [
                    (WizardStep::Device, Text::WizardDeviceStep),
                    (WizardStep::Learn, Text::WizardLearnStep),
                    (WizardStep::Assign, Text::WizardAssignStep),
                ] {
                    // A step already reached can be revisited; skipping ahead cannot, or a
                    // function would be assigned to a pedal that was never learned.
                    let enabled = step.number() <= current.number();
                    if ui.add_enabled(enabled, egui::Button::selectable(current == step, text.in_lang(lang))).clicked() {
                        self.wizard.step = step;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(i18n::fill(Text::WizardStepOf.in_lang(lang), &[("{n}", &current.number().to_string())]));
                });
            });
            ui.separator();
            // A laptop can show a dozen devices and eight pedals can be learned: cap the body
            // so the window never grows past the screen and the buttons stay in reach.
            egui::ScrollArea::vertical().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| match self.wizard.step {
                WizardStep::Device => self.wizard_device(ui, lang),
                WizardStep::Learn => self.wizard_learn(ui, lang),
                WizardStep::Assign => self.wizard_assign(ui, lang),
            });
            ui.separator();
            let last = self.wizard.last();
            let ready = self.wizard_ready();
            ui.horizontal(|ui| {
                if ui.add_enabled(self.wizard.step != WizardStep::Device, egui::Button::new(Text::WizardBack.in_lang(lang))).clicked() {
                    self.wizard.back();
                }
                let label = if last { Text::WizardFinish } else { Text::WizardNext };
                if ui.add_enabled(ready, egui::Button::new(label.in_lang(lang))).clicked() {
                    self.wizard_advance();
                }
                if !ready {
                    ui.weak(self.wizard_hint(lang));
                }
            });
        });
        self.wizard.open &= open;
    }

    /// Moves the wizard on. Opening the Learn step with no pedal yet starts learning the
    /// first one, so the user only has to press it.
    fn wizard_advance(&mut self) {
        if self.wizard.last() {
            self.wizard.open = false;
            return;
        }
        if let Some(WizardStep::Learn) = self.wizard.next() {
            let empty = lock(&self.pedal).config().buttons.is_empty();
            if empty && let Err(e) = lock(&self.pedal).start_learning(0) {
                self.error = Some(e.to_string());
            }
        }
    }

    /// Whether the current step has what it needs to move on.
    fn wizard_ready(&self) -> bool {
        match self.wizard.step {
            WizardStep::Device => lock(&self.pedal).config().source != PedalSource::None,
            WizardStep::Learn => !lock(&self.pedal).config().buttons.is_empty(),
            WizardStep::Assign => true,
        }
    }

    fn wizard_hint(&self, lang: Lang) -> &'static str {
        match self.wizard.step {
            WizardStep::Device => Text::WizardNeedDevice.in_lang(lang),
            WizardStep::Learn => Text::WizardNeedPedal.in_lang(lang),
            WizardStep::Assign => "",
        }
    }

    fn wizard_device(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(Text::WizardDeviceIntro.in_lang(lang));
        let current = lock(&self.pedal).config().source.clone();
        // The saved device stays in the list, chosen, even when it is unplugged.
        let mut rows: Vec<(PedalSource, String)> = self.devices.iter().map(|d| (d.source.clone(), device_label(d, lang))).collect();
        if current != PedalSource::None && !rows.iter().any(|(source, _)| *source == current) {
            rows.insert(0, (current.clone(), source_label(&current, lang)));
        }
        let mut chosen = None;
        // A dozen HID devices is normal on a laptop, so the list scrolls instead of
        // stretching the window over the document behind it.
        egui::ScrollArea::vertical().max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
            for (source, label) in rows {
                if ui.selectable_label(source == current, label).clicked() {
                    chosen = Some(source);
                }
            }
        });
        if self.devices.is_empty() {
            ui.weak(Text::WizardNoDevices.in_lang(lang));
        }
        ui.horizontal(|ui| {
            if ui.button(Text::Refresh.in_lang(lang)).clicked() {
                self.devices = list_devices();
            }
            self.reader_status(ui, lang);
        });
        if let Some(source) = chosen {
            self.set_source(source);
        }
        if matches!(current, PedalSource::Serial { .. }) {
            ui.weak(Text::SerialWiring.in_lang(lang));
        }
    }

    fn wizard_learn(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(Text::WizardLearnIntro.in_lang(lang));
        let mut c = lock(&self.pedal);
        let mut cfg = c.config().clone();
        let pressed = c.pressed();
        let mut changed = false;
        let mut learn = None;
        let mut remove = None;
        match c.learning() {
            Some(i) => {
                ui.colored_label(ui.visuals().warn_fg_color, learning_prompt(c.learning_armed(), i, lang));
                if ui.button(Text::CancelLearning.in_lang(lang)).clicked() {
                    c.cancel_learning();
                }
            }
            None if cfg.buttons.is_empty() => {
                ui.weak(Text::WizardPressFirst.in_lang(lang));
                if ui.button(Text::LearnNew.in_lang(lang)).clicked() {
                    learn = Some(0);
                }
            }
            None => {
                for i in 0..cfg.buttons.len() {
                    let down = pressed.get(i).copied().unwrap_or(false);
                    ui.horizontal(|ui| {
                        ui.label(i18n::fill(Text::PedalNumber.in_lang(lang), &[("{n}", &(i + 1).to_string())]));
                        ui.label(if down { Text::Pressed.in_lang(lang) } else { "○" });
                        if ui.button(Text::LearnAgain.in_lang(lang)).clicked() {
                            learn = Some(i);
                        }
                        if ui.small_button(Text::Remove.in_lang(lang)).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if cfg.buttons.len() < scribecraft_engine::pedal::MAX_PEDALS && ui.button(Text::LearnNew.in_lang(lang)).clicked() {
                    learn = Some(cfg.buttons.len());
                }
            }
        }
        if let Some(i) = remove
            && i < cfg.buttons.len()
        {
            cfg.buttons.remove(i);
            changed = true;
        }
        if changed && let Err(e) = c.set_config(cfg) {
            self.error = Some(e.to_string());
        }
        if let Some(i) = learn
            && let Err(e) = c.start_learning(i)
        {
            self.error = Some(e.to_string());
        }
        drop(c);
        if changed {
            self.save_pedal();
        }
    }

    fn wizard_assign(&mut self, ui: &mut egui::Ui, lang: Lang) {
        ui.label(Text::WizardAssignIntro.in_lang(lang));
        let mut c = lock(&self.pedal);
        let mut cfg = c.config().clone();
        let pressed = c.pressed();
        let mut changed = play_mode_radios(ui, &mut cfg, lang);
        let edits = pedal_grid(ui, &mut cfg, &pressed, lang);
        changed |= edits.changed;
        if let Some(i) = edits.remove
            && i < cfg.buttons.len()
        {
            cfg.buttons.remove(i);
            changed = true;
        }
        if let PedalSource::Serial { port } = &cfg.source
            && ui.button(Text::SerialWiring.in_lang(lang)).clicked()
        {
            cfg = PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) };
            changed = true;
        }
        if changed && let Err(e) = c.set_config(cfg) {
            self.error = Some(e.to_string());
        }
        if let Some(i) = edits.learn
            && let Err(e) = c.start_learning(i)
        {
            self.error = Some(e.to_string());
        }
        drop(c);
        if changed {
            self.save_pedal();
        }
    }

    fn keys_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, lang: Lang) {
        let mut changed = false;
        if ui.checkbox(&mut self.hotkey_cfg.global, Text::KeysGlobal.in_lang(lang)).changed() {
            changed = true;
        }
        ui.weak(Text::KeysHelp.in_lang(lang));
        if let Some(action) = self.capturing {
            let prompt = i18n::fill(Text::PressKeyFor.in_lang(lang), &[("{action}", action_name(action, lang))]);
            ui.colored_label(ui.visuals().warn_fg_color, prompt);
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
                    match binding_text(key, mods, lang) {
                        Ok(text) => {
                            self.notice = self.assign(action, &text, lang);
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
            ui.strong(Text::FunctionColumn.in_lang(lang));
            ui.strong(Text::KeyColumn.in_lang(lang));
            ui.end_row();
            for a in KEY_FUNCTIONS {
                let keys: Vec<&str> = self.hotkey_cfg.bindings.iter().filter(|b| b.action == a).map(|b| b.keys.as_str()).collect();
                ui.label(action_name(a, lang));
                ui.monospace(if keys.is_empty() { "—".to_owned() } else { keys.join(" / ") });
                if ui.button(if self.capturing == Some(a) { Text::PressAKey.in_lang(lang) } else { Text::Assign.in_lang(lang) }).clicked() {
                    capture = Some(a);
                }
                if !keys.is_empty() && ui.small_button(Text::Clear.in_lang(lang)).clicked() {
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
        if ui.button(Text::RestoreDefaults.in_lang(lang)).clicked() {
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
    fn assign(&mut self, action: PedalAction, text: &str, lang: Lang) -> Option<String> {
        let wanted = parse_binding(text);
        let same = |b: &HotkeyBinding| wanted.is_some() && parse_binding(&b.keys) == wanted;
        let moved_from = self.hotkey_cfg.bindings.iter().find(|b| b.action != action && same(b)).map(|b| b.action);
        self.hotkey_cfg.bindings.retain(|b| b.action != action && !same(b));
        if self.hotkey_cfg.bindings.len() >= scribecraft_engine::hotkeys::MAX_BINDINGS {
            return Some(Text::TooManyKeys.in_lang(lang).to_owned());
        }
        self.hotkey_cfg.bindings.push(HotkeyBinding { keys: text.to_owned(), action });
        Some(match moved_from {
            Some(old) => i18n::fill(
                Text::KeyMoved.in_lang(lang),
                &[("{key}", text), ("{action}", action_name(action, lang)), ("{old}", action_name(old, lang))],
            ),
            None => i18n::fill(Text::KeyAssigned.in_lang(lang), &[("{key}", text), ("{action}", action_name(action, lang))]),
        })
    }
}

/// How a saved device is named in the window.
fn source_label(source: &PedalSource, lang: Lang) -> String {
    match source {
        PedalSource::None => Text::NoPedal.in_lang(lang).to_owned(),
        PedalSource::Serial { port } => format!("{}{port}", Text::SerialPrefix.in_lang(lang)),
        PedalSource::Hid { vendor_id, product_id, name, .. } => format!("USB: {name} ({vendor_id:04x}:{product_id:04x})"),
    }
}

/// A device found by the scan, named in the chosen language and starred when it comes from a
/// known pedal maker. The name is built from the source rather than the platform's `label`,
/// so the serial prefix is translated too.
fn device_label(d: &PedalDevice, lang: Lang) -> String {
    let label = source_label(&d.source, lang);
    if d.likely_pedal { format!("★ {label}") } else { label }
}

/// The device combo: the saved choice and every device found.
fn device_picker(ui: &mut egui::Ui, devices: &[PedalDevice], current: &PedalSource, lang: Lang) -> Option<PedalSource> {
    let mut chosen = None;
    egui::ComboBox::from_label(Text::Device.in_lang(lang)).selected_text(source_label(current, lang)).width(340.0).show_ui(ui, |ui| {
        if ui.selectable_label(*current == PedalSource::None, Text::NoPedal.in_lang(lang)).clicked() {
            chosen = Some(PedalSource::None);
        }
        for d in devices {
            if ui.selectable_label(d.source == *current, device_label(d, lang)).clicked() {
                chosen = Some(d.source.clone());
            }
        }
    });
    chosen
}

/// The configuration after choosing `source`. A serial pedal starts from the standard
/// wiring; a USB pedal starts empty because each of its pedals is learned. Moving between
/// two USB devices keeps the learned pedals, which are the bits of the same kind of report.
fn config_for_source(cfg: &PedalConfig, source: PedalSource) -> PedalConfig {
    match &source {
        PedalSource::Serial { port } if cfg.buttons.is_empty() || matches!(cfg.source, PedalSource::Hid { .. }) => {
            PedalConfig { play_mode: cfg.play_mode, ..PedalConfig::serial_default(port) }
        }
        PedalSource::Hid { .. } if matches!(cfg.source, PedalSource::Serial { .. }) => {
            PedalConfig { source, buttons: Vec::new(), play_mode: cfg.play_mode }
        }
        _ => PedalConfig { source, ..cfg.clone() },
    }
}

/// "Play while held" or "press to play": one line each, because together they are wider than
/// the window in Italian and a radio button does not wrap.
fn play_mode_radios(ui: &mut egui::Ui, cfg: &mut PedalConfig, lang: Lang) -> bool {
    ui.label(Text::PlayPedal.in_lang(lang));
    let mut changed = false;
    ui.indent("play-mode", |ui| {
        changed |= ui.radio_value(&mut cfg.play_mode, PlayMode::Hold, Text::PlayWhileHeld.in_lang(lang)).changed();
        changed |= ui.radio_value(&mut cfg.play_mode, PlayMode::Toggle, Text::PlayToggleMode.in_lang(lang)).changed();
    });
    changed
}

/// What the pedal rows asked for this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PedalEdits {
    changed: bool,
    learn: Option<usize>,
    remove: Option<usize>,
}

/// One row per learned pedal: live state, what it does, learn again, remove.
fn pedal_grid(ui: &mut egui::Ui, cfg: &mut PedalConfig, pressed: &[bool], lang: Lang) -> PedalEdits {
    let mut edits = PedalEdits::default();
    egui::Grid::new("pedals").num_columns(5).striped(true).show(ui, |ui| {
        ui.strong(Text::PedalColumn.in_lang(lang));
        ui.strong("");
        ui.strong(Text::FunctionColumn.in_lang(lang));
        ui.end_row();
        let n = cfg.buttons.len();
        for (i, b) in cfg.buttons.iter_mut().enumerate() {
            let down = pressed.get(i).copied().unwrap_or(false);
            ui.label(i18n::fill(Text::PedalNumber.in_lang(lang), &[("{n}", &(i + 1).to_string())]));
            ui.label(if down { Text::Pressed.in_lang(lang) } else { "○" });
            edits.changed |= action_combo(ui, ("pedal-action", i), &mut b.action, lang);
            if ui.button(Text::LearnAgain.in_lang(lang)).clicked() {
                edits.learn = Some(i);
            }
            if ui.small_button(Text::Remove.in_lang(lang)).clicked() {
                edits.remove = Some(i);
            }
            ui.end_row();
        }
        if n < scribecraft_engine::pedal::MAX_PEDALS && ui.button(Text::LearnNew.in_lang(lang)).clicked() {
            edits.learn = Some(n);
        }
        ui.end_row();
    });
    edits
}

/// "Press pedal N now…" once every pedal is up, otherwise the release request.
fn learning_prompt(armed: bool, index: usize, lang: Lang) -> String {
    if armed {
        i18n::fill(Text::PressPedal.in_lang(lang), &[("{n}", &(index + 1).to_string())])
    } else {
        Text::ReleasePedals.in_lang(lang).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};
    use scribecraft_engine::pedal::{InputBit, PedalButton};

    /// The wizard walks its three steps in order and stops at both ends.
    #[test]
    fn the_pedal_wizard_walks_its_steps_in_order() {
        let mut w = Wizard::default();
        assert_eq!((w.open, w.step), (false, WizardStep::Device));
        w.start();
        assert!(w.open);
        assert!(!w.last());
        assert_eq!(w.back(), None, "the first step has nowhere to go back to");
        assert_eq!(w.next(), Some(WizardStep::Learn));
        assert_eq!(w.step, WizardStep::Learn);
        assert_eq!(w.next(), Some(WizardStep::Assign));
        assert!(w.last());
        assert_eq!(w.next(), None, "the last step must not walk off the end");
        assert_eq!(w.step, WizardStep::Assign);
        assert_eq!(w.back(), Some(WizardStep::Learn));
    }

    /// Reopening starts at the device step, so an old step never meets a new pedal.
    #[test]
    fn reopening_the_wizard_returns_to_the_first_step() {
        let mut w = Wizard { open: false, step: WizardStep::Assign };
        w.start();
        assert_eq!(w.step, WizardStep::Device);
    }

    /// A serial pedal brings the standard wiring, a USB pedal starts empty so each of its
    /// pedals is learned, and moving between two USB devices keeps what was learned.
    #[test]
    fn choosing_a_device_sets_the_pedals_it_starts_from() {
        let serial = config_for_source(&PedalConfig::default(), PedalSource::Serial { port: "COM3".into() });
        assert_eq!(serial.buttons.len(), 4, "standard wiring: play, rewind and two fast-forwards");
        assert!(serial.buttons.iter().any(|b| b.action == PedalAction::Play));
        let usb = PedalSource::Hid { vendor_id: 1, product_id: 2, path: None, name: "Pedal".into() };
        let empty = config_for_source(&serial, usb.clone());
        assert!(empty.buttons.is_empty(), "a USB pedal is learned, not guessed");
        let learned = PedalConfig {
            source: usb,
            buttons: vec![PedalButton { bit: InputBit { byte: 0, mask: 1, active_low: false }, action: PedalAction::Play }],
            play_mode: PlayMode::Toggle,
        };
        let other = PedalSource::Hid { vendor_id: 3, product_id: 4, path: None, name: "Other".into() };
        let kept = config_for_source(&learned, other.clone());
        assert_eq!(kept.buttons, learned.buttons, "another USB device keeps the learned bits");
        assert_eq!(kept.play_mode, PlayMode::Toggle, "the play mode survives a device change");
        assert_eq!(kept.source, other);
    }

    /// The device's name is shown in the chosen language, with the serial prefix.
    #[test]
    fn a_serial_device_is_named_in_the_chosen_language() {
        assert_eq!(source_label(&PedalSource::Serial { port: "COM3".into() }, Lang::Italiano), "Seriale: COM3");
        assert_eq!(source_label(&PedalSource::None, Lang::English), "no pedal");
        assert_eq!(
            source_label(&PedalSource::Hid { vendor_id: 0x3553, product_id: 0x0101, path: None, name: "VEC".into() }, Lang::English),
            "USB: VEC (3553:0101)"
        );
    }

    /// The prompt names the pedal being learned, and asks for released pedals first.
    #[test]
    fn the_learning_prompt_names_the_pedal_in_both_languages() {
        assert_eq!(learning_prompt(true, 0, Lang::English), "Press pedal 1 now…");
        assert_eq!(learning_prompt(true, 2, Lang::Italiano), "Premi ora il pedale 3…");
        assert_eq!(learning_prompt(false, 0, Lang::English), "Release every pedal first; learning starts when they are all up.");
        assert!(learning_prompt(false, 0, Lang::Italiano).starts_with("Rilascia prima tutti i pedali"));
    }

    #[test]
    fn captured_keys_become_bindings_that_read_back() {
        assert_eq!(binding_text(Key::F9, Modifiers::NONE, Lang::English).unwrap(), "F9");
        let ctrl_shift = Modifiers { ctrl: true, shift: true, ..Modifiers::NONE };
        assert_eq!(binding_text(Key::P, ctrl_shift, Lang::English).unwrap(), "Ctrl+Shift+P");
        assert_eq!(parse_binding("Ctrl+Shift+P"), Some((true, false, true, Key::P)));
        assert_eq!(parse_binding("Ctrl+Shift+KeyP"), Some((true, false, true, Key::P)));
        assert_eq!(parse_binding("F9"), Some((false, false, false, Key::F9)));
        assert_eq!(parse_binding("Alt+Digit1"), Some((false, true, false, Key::Num1)));
        assert_eq!(parse_binding("Hyper+F9"), None);
    }

    #[test]
    fn keys_that_would_block_typing_are_refused() {
        assert!(binding_text(Key::A, Modifiers::NONE, Lang::English).is_err());
        assert!(binding_text(Key::Space, Modifiers::SHIFT, Lang::English).is_err());
        assert!(binding_text(Key::Escape, Modifiers::CTRL, Lang::English).is_err());
        assert!(binding_text(Key::A, Modifiers::ALT, Lang::English).is_ok());
    }

    /// The refusal is a sentence the user can act on, in the language they chose.
    #[test]
    fn a_refused_key_explains_itself_in_the_chosen_language() {
        let it = binding_text(Key::A, Modifiers::NONE, Lang::Italiano).unwrap_err();
        assert!(it.contains("Ctrl o Alt"), "{it}");
        assert!(it.contains("tasto funzione"), "{it}");
        let en = binding_text(Key::A, Modifiers::NONE, Lang::English).unwrap_err();
        assert!(en.contains("Ctrl or Alt"), "{en}");
        assert_eq!(action_name(PedalAction::PlayToggle, Lang::Italiano), "riproduci / ferma");
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
