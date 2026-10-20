//! Operating-system services (layer L5). The only crate below the UI that talks to devices.
//!
//! - [`AudioOutput`]: plays a shared [`Transport`] on the default output device.
//! - [`pedal`]: USB HID and serial (COM) foot pedal readers.
//! - [`hotkeys`]: system-wide hotkeys that work while another program has the focus.

use std::sync::{Arc, Mutex};

use scribecraft_engine::Transport;

/// Errors from operating-system services, phrased so the user can act on them.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("no audio output device is available; connect headphones or speakers and try again")]
    NoOutputDevice,
    #[error("the audio device cannot be used: {0}")]
    Device(String),
    #[error("this build has no audio output (built without the \"audio\" feature)")]
    NoAudioSupport,
}

#[cfg(feature = "audio")]
mod audio;

#[cfg(feature = "audio")]
pub use audio::AudioOutput;

#[cfg(feature = "pedal")]
pub mod pedal;

#[cfg(feature = "hotkeys")]
pub mod hotkeys;

/// Without the `audio` feature, opening an output is a clear error.
#[cfg(not(feature = "audio"))]
#[derive(Debug)]
pub struct AudioOutput;

#[cfg(not(feature = "audio"))]
impl AudioOutput {
    pub fn open(_transport: Arc<Mutex<Transport>>) -> Result<Self, PlatformError> {
        Err(PlatformError::NoAudioSupport)
    }

    /// Same surface as the real output; never reached because `open` always fails.
    pub fn device_name(&self) -> &str {
        ""
    }
}

/// The per-user folder for ScribeCraft's job list and settings:
/// `%APPDATA%\ScribeCraft` on Windows, `~/Library/Application Support/ScribeCraft` on macOS,
/// `$XDG_DATA_HOME/scribecraft` (default `~/.local/share/scribecraft`) elsewhere.
/// `None` when the environment gives no home folder. `SCRIBECRAFT_DATA_DIR` overrides all of
/// these (tests and screenshots use it so they never touch the user's real job list).
pub fn data_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = env("SCRIBECRAFT_DATA_DIR") {
        Some(dir)
    } else if cfg!(windows) {
        env("APPDATA").map(|p| p.join("ScribeCraft"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|p| p.join("Library").join("Application Support").join("ScribeCraft"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local").join("share"))).map(|p| p.join("scribecraft"))
    }
}

/// Keeps the `Arc`/`Mutex` imports used in every feature combination.
#[doc(hidden)]
pub type SharedTransport = Arc<Mutex<Transport>>;
