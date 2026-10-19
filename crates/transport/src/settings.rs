//! Playback preferences. Defaults are ScribeCraft's own choices.

use scribecraft_dsp::ChannelMode;
use serde::{Deserialize, Serialize};

use crate::TransportError;

/// Slowest supported speed (10 %).
pub const MIN_SPEED: f64 = 0.1;
/// Fastest supported speed (400 %).
pub const MAX_SPEED: f64 = 4.0;
/// Longest auto-backstep (one minute).
pub const MAX_BACKSTEP_MS: u64 = 60_000;
/// Fastest rewind / fast-forward, as a multiple of real time.
const MAX_SCAN_RATE: f64 = 100.0;

/// Playback preferences shared by every recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// How far playback steps back when stopped, so the last words are heard again.
    pub auto_backstep_ms: u64,
    /// Speed of the slow preset.
    pub slow_speed: f64,
    /// Speed of the fast preset.
    pub fast_speed: f64,
    /// Rewind speed while held, as a multiple of real time.
    pub rewind_rate: f64,
    /// Fast-forward speed while held, as a multiple of real time.
    pub fast_forward_rate: f64,
    /// Step used by "skip back" / "skip forward" commands.
    pub skip_ms: u64,
    /// Volume in decibels (0 = as recorded).
    pub volume_db: f32,
    /// Which channels are heard.
    #[serde(with = "channel_mode")]
    pub channel_mode: ChannelMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_backstep_ms: 1_000,
            slow_speed: 0.6,
            fast_speed: 1.5,
            rewind_rate: 8.0,
            fast_forward_rate: 8.0,
            skip_ms: 5_000,
            volume_db: 0.0,
            channel_mode: ChannelMode::Stereo,
        }
    }
}

impl Settings {
    /// Checks every value; the error names the field and the allowed range.
    pub fn validate(&self) -> Result<(), TransportError> {
        let bad = |what: &str, allowed: &str| Err(TransportError::BadSetting(format!("{what} is out of range (allowed: {allowed})")));
        if self.auto_backstep_ms > MAX_BACKSTEP_MS {
            return bad("auto_backstep_ms", "0..=60000");
        }
        for (name, v) in [("slow_speed", self.slow_speed), ("fast_speed", self.fast_speed)] {
            if !v.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&v) {
                return bad(name, "0.1..=4.0");
            }
        }
        for (name, v) in [("rewind_rate", self.rewind_rate), ("fast_forward_rate", self.fast_forward_rate)] {
            if !v.is_finite() || !(1.0..=MAX_SCAN_RATE).contains(&v) {
                return bad(name, "1..=100");
            }
        }
        if self.skip_ms > 600_000 {
            return bad("skip_ms", "0..=600000");
        }
        if !self.volume_db.is_finite() || !(-60.0..=24.0).contains(&self.volume_db) {
            return bad("volume_db", "-60..=24");
        }
        Ok(())
    }
}

mod channel_mode {
    use scribecraft_dsp::ChannelMode;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(mode: &ChannelMode, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(mode.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ChannelMode, D::Error> {
        let name = String::deserialize(d)?;
        ChannelMode::parse(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown channel mode {name:?} (allowed: stereo, left, right, mix)")))
    }
}
