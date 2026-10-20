//! System-wide hotkeys: keys that control playback while another program (the word
//! processor) has the focus. The keys themselves are registered by `platform`; this module
//! holds the user's bindings and what each one does.

use std::path::Path;

use scribecraft_transport::Transport;
use serde::{Deserialize, Serialize};

use crate::pedal::{PedalAction, PlayMode, apply};
use crate::{EngineError, Result, write_atomic};

/// Most bindings kept.
pub const MAX_BINDINGS: usize = 16;

/// One key (with optional modifiers, e.g. `"F9"`, `"Ctrl+Shift+KeyP"`) and its action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub keys: String,
    pub action: PedalAction,
}

/// The hotkey settings saved in `hotkeys.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// Register the bindings system-wide. When off they work only in ScribeCraft's window.
    pub global: bool,
    pub bindings: Vec<HotkeyBinding>,
}

impl Default for HotkeyConfig {
    /// ScribeCraft's own layout: F7 rewind (hold), F8 fast-forward (hold), F9 play/stop,
    /// F4 stop, F6 slow. F5 and F10 stay free because word processors use them.
    fn default() -> Self {
        let b = |keys: &str, action| HotkeyBinding { keys: keys.into(), action };
        Self {
            global: true,
            bindings: vec![
                b("F7", PedalAction::Rewind),
                b("F8", PedalAction::FastForward),
                b("F9", PedalAction::PlayToggle),
                b("F4", PedalAction::Stop),
                b("F6", PedalAction::Slow),
            ],
        }
    }
}

impl HotkeyConfig {
    pub fn validate(&self) -> Result<()> {
        if self.bindings.len() > MAX_BINDINGS {
            return Err(EngineError::Invalid(format!("at most {MAX_BINDINGS} hotkeys can be set")));
        }
        for b in &self.bindings {
            let k = b.keys.trim();
            if k.is_empty() || k.len() > 64 || !k.is_ascii() {
                return Err(EngineError::Invalid(format!("hotkey {:?} is not valid (e.g. \"F9\" or \"Ctrl+Shift+KeyP\")", b.keys)));
            }
        }
        Ok(())
    }

    /// Loads `hotkeys.json`; a missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(EngineError::Io { path: path.display().to_string(), source }),
        };
        let cfg: Self = serde_json::from_slice(&bytes)
            .map_err(|e| EngineError::Invalid(format!("{} is not a valid hotkey configuration: {e}", path.display())))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let json = serde_json::to_vec_pretty(self).map_err(|e| EngineError::Invalid(e.to_string()))?;
        write_atomic(path, &json)
    }
}

/// What a hotkey press or release does. A keyboard "play" key toggles; rewind and
/// fast-forward work while the key is held.
pub fn apply_hotkey(action: PedalAction, pressed: bool, t: &mut Transport) {
    apply(action, pressed, PlayMode::Toggle, t);
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribecraft_media::synth;
    use scribecraft_transport::{Settings, State};
    use std::sync::Arc;

    #[test]
    fn default_keys_play_toggle_and_hold_to_rewind() {
        let cfg = HotkeyConfig::default();
        cfg.validate().unwrap();
        let action = |k: &str| cfg.bindings.iter().find(|b| b.keys == k).map(|b| b.action).unwrap();
        let mut t = Transport::new(Arc::new(synth::dictation(8_000, 1, 10.0)), Settings::default()).unwrap();
        t.seek_ms(5_000);
        apply_hotkey(action("F9"), true, &mut t);
        apply_hotkey(action("F9"), false, &mut t);
        assert_eq!(t.state(), State::Playing, "releasing the play key does not stop");
        apply_hotkey(action("F7"), true, &mut t);
        assert_eq!(t.state(), State::Rewinding);
        apply_hotkey(action("F7"), false, &mut t);
        assert_eq!(t.state(), State::Playing);
        apply_hotkey(action("F4"), true, &mut t);
        assert_eq!(t.state(), State::Stopped);
    }

    #[test]
    fn invalid_bindings_are_rejected_and_files_round_trip() {
        let bad = HotkeyConfig { global: true, bindings: vec![HotkeyBinding { keys: " ".into(), action: PedalAction::Play }] };
        assert!(bad.validate().is_err());
        let dir = std::env::temp_dir().join(format!("scribecraft-hotkeys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hotkeys.json");
        assert_eq!(HotkeyConfig::load(&path).unwrap(), HotkeyConfig::default());
        let cfg = HotkeyConfig { global: false, ..HotkeyConfig::default() };
        cfg.save(&path).unwrap();
        assert_eq!(HotkeyConfig::load(&path).unwrap(), cfg);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
