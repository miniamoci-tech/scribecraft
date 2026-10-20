//! Foot pedals, independent of how they are connected.
//!
//! A pedal device delivers *reports*: raw bytes from a USB HID device, or, for a pedal on a
//! serial (COM) port, one byte built from the modem status lines ([`SERIAL_DCD`] …). A
//! [`PedalConfig`] says which bit of the report each pedal is and what it does; a
//! [`PedalController`] turns successive reports into press/release events, applies them to
//! the transport, and runs the "learn" step that finds a pedal's bit by pressing it once.
//!
//! The serial defaults follow NCH's public pedal wiring note: pedal on pin 1 (DCD) plays,
//! pin 6 (DSR) rewinds, pin 8 (CTS) or pin 9 (RI) fast-forwards, pin 4 (DTR) is the common
//! line (the reader raises it). USB pedals have no published layout, so they are learned.

use std::path::Path;

use scribecraft_transport::Transport;
use serde::{Deserialize, Serialize};

use crate::{EngineError, Result, write_atomic};

/// Most pedals (buttons) one device can drive.
pub const MAX_PEDALS: usize = 8;
/// Longest report considered; longer reports are truncated.
pub const MAX_REPORT_BYTES: usize = 64;

/// Serial pseudo-report bits (byte 0): Data Carrier Detect, pin 1.
pub const SERIAL_DCD: u8 = 0x01;
/// Data Set Ready, pin 6.
pub const SERIAL_DSR: u8 = 0x02;
/// Clear To Send, pin 8.
pub const SERIAL_CTS: u8 = 0x04;
/// Ring Indicator, pin 9.
pub const SERIAL_RI: u8 = 0x08;

/// What a pedal does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PedalAction {
    #[default]
    None,
    /// Rewind while held.
    Rewind,
    /// Play (while held, or toggled — see [`PlayMode`]).
    Play,
    /// Fast-forward while held.
    FastForward,
    /// Stop (with auto-backstep).
    Stop,
    /// Each press toggles play/stop, whatever [`PlayMode`] says.
    PlayToggle,
    SkipBack,
    SkipForward,
    /// Toggle the slow-speed preset.
    Slow,
    /// Toggle the fast-speed preset.
    Fast,
}

impl PedalAction {
    pub const ALL: [PedalAction; 10] = [
        PedalAction::None,
        PedalAction::Rewind,
        PedalAction::Play,
        PedalAction::FastForward,
        PedalAction::Stop,
        PedalAction::PlayToggle,
        PedalAction::SkipBack,
        PedalAction::SkipForward,
        PedalAction::Slow,
        PedalAction::Fast,
    ];
}

/// How the play pedal behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    /// Plays while held; releasing stops (with auto-backstep). The usual transcription habit.
    #[default]
    Hold,
    /// Each press toggles play and stop.
    Toggle,
}

/// Where a pedal's state lives in a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBit {
    pub byte: u16,
    pub mask: u8,
    /// The bit is 0 while the pedal is pressed.
    #[serde(default)]
    pub active_low: bool,
}

impl InputBit {
    /// Whether this pedal is down in `report`. A report too short to hold the bit reads as up.
    pub fn is_pressed(&self, report: &[u8]) -> bool {
        match report.get(usize::from(self.byte)) {
            Some(b) => ((b & self.mask) != 0) != self.active_low,
            None => false,
        }
    }
}

/// One pedal: where it is in the report and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PedalButton {
    pub bit: InputBit,
    pub action: PedalAction,
}

/// Which device the pedal is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PedalSource {
    #[default]
    None,
    /// A USB HID device. `path` pins one physical device when several match.
    Hid {
        vendor_id: u16,
        product_id: u16,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        name: String,
    },
    /// A pedal on a serial (COM) port, read through its modem status lines.
    Serial { port: String },
}

/// The pedal configuration saved in `pedal.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PedalConfig {
    pub source: PedalSource,
    pub buttons: Vec<PedalButton>,
    pub play_mode: PlayMode,
}

impl PedalConfig {
    /// The layout of NCH's public serial wiring note: DCD plays, DSR rewinds, CTS and RI
    /// fast-forward.
    pub fn serial_default(port: &str) -> Self {
        let bit = |mask| InputBit { byte: 0, mask, active_low: false };
        Self {
            source: PedalSource::Serial { port: port.to_owned() },
            buttons: vec![
                PedalButton { bit: bit(SERIAL_DSR), action: PedalAction::Rewind },
                PedalButton { bit: bit(SERIAL_DCD), action: PedalAction::Play },
                PedalButton { bit: bit(SERIAL_CTS), action: PedalAction::FastForward },
                PedalButton { bit: bit(SERIAL_RI), action: PedalAction::FastForward },
            ],
            play_mode: PlayMode::Hold,
        }
    }

    /// Checks limits; the error says what is allowed.
    pub fn validate(&self) -> Result<()> {
        if self.buttons.len() > MAX_PEDALS {
            return Err(EngineError::Invalid(format!("a pedal configuration can have at most {MAX_PEDALS} buttons")));
        }
        for b in &self.buttons {
            if usize::from(b.bit.byte) >= MAX_REPORT_BYTES || b.bit.mask.count_ones() != 1 {
                return Err(EngineError::Invalid(format!(
                    "pedal bit byte {} mask {:#04x} is invalid (byte below {MAX_REPORT_BYTES}, exactly one bit set)",
                    b.bit.byte, b.bit.mask
                )));
            }
        }
        Ok(())
    }

    /// Loads `pedal.json`; a missing file is "no pedal".
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(EngineError::Io { path: path.display().to_string(), source }),
        };
        let cfg: Self = serde_json::from_slice(&bytes)
            .map_err(|e| EngineError::Invalid(format!("{} is not a valid pedal configuration: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Saves atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| EngineError::Invalid(e.to_string()))?;
        write_atomic(path, &json)
    }
}

/// Finds the bit that changed between a report with no pedal down and one with the pedal
/// to learn held down. Returns `None` when nothing changed. When several bits changed, the
/// first one wins (lowest byte, then lowest bit).
pub fn learn_bit(released: &[u8], pressed: &[u8]) -> Option<InputBit> {
    let len = released.len().max(pressed.len()).min(MAX_REPORT_BYTES);
    (0..len).find_map(|i| {
        let a = released.get(i).copied().unwrap_or(0);
        let b = pressed.get(i).copied().unwrap_or(0);
        let diff = a ^ b;
        if diff == 0 {
            return None;
        }
        let mask = 1u8 << diff.trailing_zeros();
        let byte = u16::try_from(i).ok()?;
        Some(InputBit { byte, mask, active_low: a & mask != 0 })
    })
}

/// A pedal press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PedalEvent {
    /// Index into [`PedalConfig::buttons`].
    pub button: usize,
    pub pressed: bool,
    pub action: PedalAction,
}

/// Result of feeding one report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Feed {
    pub events: Vec<PedalEvent>,
    /// Set when a learn step finished: the button index and its new bit.
    pub learned: Option<(usize, InputBit)>,
}

/// Turns reports into transport commands. Holds no OS resources: the platform reader feeds it.
#[derive(Debug, Clone, Default)]
pub struct PedalController {
    config: PedalConfig,
    down: [bool; MAX_PEDALS],
    last: Vec<u8>,
    learning: Option<usize>,
}

impl PedalController {
    pub fn new(config: PedalConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config, ..Self::default() })
    }

    pub fn config(&self) -> &PedalConfig {
        &self.config
    }

    /// Replaces the configuration; pedals held at that moment are considered released.
    pub fn set_config(&mut self, config: PedalConfig) -> Result<()> {
        config.validate()?;
        self.config = config;
        self.down = [false; MAX_PEDALS];
        self.learning = None;
        Ok(())
    }

    /// The most recent report (for a live indicator).
    pub fn last_report(&self) -> &[u8] {
        &self.last
    }

    /// Which configured pedals are down right now.
    pub fn pressed(&self) -> Vec<bool> {
        self.config.buttons.iter().map(|b| b.bit.is_pressed(&self.last)).collect()
    }

    /// Starts learning button `index` (creating it, as [`PedalAction::None`], if it is the
    /// next one): release every pedal, then press the one to learn. The next report that
    /// differs from the current one assigns its bit.
    pub fn start_learning(&mut self, index: usize) -> Result<()> {
        if index > self.config.buttons.len() || index >= MAX_PEDALS {
            return Err(EngineError::Invalid(format!("pedal {index} cannot be learned (next free pedal is {})", self.config.buttons.len())));
        }
        self.learning = Some(index);
        Ok(())
    }

    pub fn learning(&self) -> Option<usize> {
        self.learning
    }

    pub fn cancel_learning(&mut self) {
        self.learning = None;
    }

    /// Feeds one report. Applies press/release events to `transport` when given (and not
    /// learning). Total for any input bytes.
    pub fn feed(&mut self, report: &[u8], transport: Option<&mut Transport>) -> Feed {
        let report = report.get(..report.len().min(MAX_REPORT_BYTES)).unwrap_or(&[]);
        let mut feed = Feed::default();
        if let Some(index) = self.learning {
            // The changed bit must go from "up" to "down" (a release while learning is ignored).
            if let Some(bit) = learn_bit(&self.last, report)
                && !bit.is_pressed(&self.last)
                && bit.is_pressed(report)
            {
                let button = PedalButton { bit, action: PedalAction::None };
                match self.config.buttons.get_mut(index) {
                    Some(b) => b.bit = bit,
                    None => self.config.buttons.push(button),
                }
                self.learning = None;
                feed.learned = Some((index, bit));
            }
            self.last = report.to_vec();
            self.down = self.current(report);
            return feed;
        }
        let now = self.current(report);
        for (i, (was, is)) in self.down.iter().zip(now.iter()).enumerate() {
            if was != is
                && let Some(b) = self.config.buttons.get(i)
            {
                feed.events.push(PedalEvent { button: i, pressed: *is, action: b.action });
            }
        }
        self.down = now;
        self.last = report.to_vec();
        if let Some(t) = transport {
            for e in &feed.events {
                apply(e.action, e.pressed, self.config.play_mode, t);
            }
        }
        feed
    }

    fn current(&self, report: &[u8]) -> [bool; MAX_PEDALS] {
        let mut now = [false; MAX_PEDALS];
        for (slot, b) in now.iter_mut().zip(&self.config.buttons) {
            *slot = b.bit.is_pressed(report);
        }
        now
    }
}

/// What one pedal press or release does to the transport.
pub fn apply(action: PedalAction, pressed: bool, mode: PlayMode, t: &mut Transport) {
    let skip = i64::try_from(t.settings().skip_ms).unwrap_or(i64::MAX);
    match (action, pressed) {
        (PedalAction::Rewind, true) => t.rewind(),
        (PedalAction::FastForward, true) => t.fast_forward(),
        (PedalAction::Rewind | PedalAction::FastForward, false) => t.release(),
        (PedalAction::Play, true) => match mode {
            PlayMode::Hold => t.play(),
            PlayMode::Toggle => t.toggle_play(),
        },
        (PedalAction::Play, false) => {
            if mode == PlayMode::Hold {
                t.stop();
            }
        }
        (PedalAction::Stop, true) => t.stop(),
        (PedalAction::PlayToggle, true) => t.toggle_play(),
        (PedalAction::SkipBack, true) => t.skip_ms(-skip),
        (PedalAction::SkipForward, true) => t.skip_ms(skip),
        (PedalAction::Slow, true) => t.toggle_slow(),
        (PedalAction::Fast, true) => t.toggle_fast(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribecraft_media::synth;
    use scribecraft_transport::{Settings, State};
    use std::sync::Arc;

    fn transport() -> Transport {
        Transport::new(Arc::new(synth::dictation(8_000, 1, 10.0)), Settings::default()).unwrap()
    }

    fn advance(t: &mut Transport, ms: u64) {
        let mut buf = vec![0.0; (ms * 8) as usize];
        t.render(&mut buf);
    }

    #[test]
    fn serial_pedal_hold_to_play_with_backstep_and_rewind_while_held() {
        let mut c = PedalController::new(PedalConfig::serial_default("COM3")).unwrap();
        let mut t = transport();
        c.feed(&[0], Some(&mut t));
        let f = c.feed(&[SERIAL_DCD], Some(&mut t));
        assert_eq!(f.events, vec![PedalEvent { button: 1, pressed: true, action: PedalAction::Play }]);
        assert_eq!(t.state(), State::Playing);
        advance(&mut t, 3_000);
        c.feed(&[0], Some(&mut t));
        assert_eq!((t.state(), t.position_ms()), (State::Stopped, 2_000), "release stops with 1 s backstep");

        // Rewind held while the play pedal is held, then released: playing again.
        c.feed(&[SERIAL_DCD], Some(&mut t));
        c.feed(&[SERIAL_DCD | SERIAL_DSR], Some(&mut t));
        assert_eq!(t.state(), State::Rewinding);
        c.feed(&[SERIAL_DCD], Some(&mut t));
        assert_eq!(t.state(), State::Playing);
        c.feed(&[SERIAL_CTS], Some(&mut t));
        assert_eq!(t.state(), State::FastForwarding);
        c.feed(&[0], Some(&mut t));
        assert_eq!(t.state(), State::Stopped);
    }

    #[test]
    fn toggle_mode_and_other_actions() {
        let mut cfg = PedalConfig::serial_default("COM1");
        cfg.play_mode = PlayMode::Toggle;
        cfg.buttons[0].action = PedalAction::Slow;
        let mut c = PedalController::new(cfg).unwrap();
        let mut t = transport();
        c.feed(&[SERIAL_DCD], Some(&mut t));
        c.feed(&[0], Some(&mut t));
        assert_eq!(t.state(), State::Playing, "release does not stop in toggle mode");
        c.feed(&[SERIAL_DCD], Some(&mut t));
        assert_eq!(t.state(), State::Stopped);
        c.feed(&[SERIAL_DSR], Some(&mut t));
        assert_eq!(t.speed(), 0.6);
    }

    #[test]
    fn learning_finds_the_pressed_bit_including_active_low_pedals() {
        assert_eq!(learn_bit(&[0, 0], &[0, 0x10]), Some(InputBit { byte: 1, mask: 0x10, active_low: false }));
        assert_eq!(learn_bit(&[0xff], &[0xfb]), Some(InputBit { byte: 0, mask: 0x04, active_low: true }));
        assert_eq!(learn_bit(&[1, 2], &[1, 2]), None);
        assert_eq!(learn_bit(&[], &[0, 0, 1]), Some(InputBit { byte: 2, mask: 1, active_low: false }));

        let mut c = PedalController::new(PedalConfig::default()).unwrap();
        c.feed(&[0x01, 0x00, 0x00], None); // idle report (report id 1)
        c.start_learning(0).unwrap();
        assert_eq!(c.feed(&[0x01, 0x00, 0x00], None).learned, None, "identical report learns nothing");
        let f = c.feed(&[0x01, 0x02, 0x00], None);
        assert_eq!(f.learned, Some((0, InputBit { byte: 1, mask: 0x02, active_low: false })));
        assert_eq!(c.learning(), None);
        // Releasing after learning produces no spurious event once an action is set.
        let mut cfg = c.config().clone();
        cfg.buttons[0].action = PedalAction::Play;
        c.set_config(cfg).unwrap();
        let mut t = transport();
        c.feed(&[0x01, 0x02, 0x00], Some(&mut t));
        assert_eq!(t.state(), State::Playing);
        assert!(c.start_learning(5).is_err());
    }

    #[test]
    fn an_active_low_pedal_can_be_learned_and_used() {
        let mut c = PedalController::new(PedalConfig::default()).unwrap();
        c.feed(&[0xff], None);
        c.start_learning(0).unwrap();
        let f = c.feed(&[0xfe], None);
        assert_eq!(f.learned, Some((0, InputBit { byte: 0, mask: 0x01, active_low: true })));
        let mut cfg = c.config().clone();
        cfg.buttons[0].action = PedalAction::Rewind;
        c.set_config(cfg).unwrap();
        let mut t = transport();
        t.seek_ms(5_000);
        c.feed(&[0xff], Some(&mut t));
        c.feed(&[0xfe], Some(&mut t));
        assert_eq!(t.state(), State::Rewinding);
    }

    #[test]
    fn hostile_reports_and_configs_are_harmless() {
        let mut c = PedalController::new(PedalConfig::serial_default("COM1")).unwrap();
        let mut t = transport();
        c.feed(&[], Some(&mut t));
        c.feed(&[0xff; 4096], Some(&mut t));
        assert!(c.last_report().len() <= MAX_REPORT_BYTES);
        let bad = PedalConfig {
            buttons: vec![PedalButton { bit: InputBit { byte: 9999, mask: 3, active_low: false }, action: PedalAction::Play }],
            ..Default::default()
        };
        assert!(PedalController::new(bad).is_err());
    }

    #[test]
    fn config_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("scribecraft-pedal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pedal.json");
        assert_eq!(PedalConfig::load(&path).unwrap(), PedalConfig::default());
        let cfg = PedalConfig {
            source: PedalSource::Hid { vendor_id: 0x05f3, product_id: 0x00ff, path: None, name: "pedal".into() },
            ..PedalConfig::serial_default("COM2")
        };
        cfg.save(&path).unwrap();
        assert_eq!(PedalConfig::load(&path).unwrap(), cfg);
        std::fs::write(&path, b"{ nope").unwrap();
        assert!(PedalConfig::load(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
