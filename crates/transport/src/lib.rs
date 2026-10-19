//! The transcription transport (layer L2): what a foot pedal, a hotkey or a button does to
//! playback, and the audio the listener hears as a result.
//!
//! [`Transport::render`] is called from the audio callback: it never allocates, never
//! blocks and never panics. Everything else is called from the UI or automation.
//!
//! Behaviour follows the public description of Express Scribe (play, stop with auto
//! backstep, rewind and fast-forward while held, slow and fast speed presets, constant
//! pitch, multi-channel control). Default values are ScribeCraft's own choices.

mod settings;
mod time;

use std::sync::Arc;

pub use scribecraft_dsp::ChannelMode;
use scribecraft_dsp::{Wsola, apply_gain, db_to_gain};
use scribecraft_media::{AudioBuffer, frames_to_ms, ms_to_frames};

pub use settings::{MAX_BACKSTEP_MS, MAX_SPEED, MIN_SPEED, Settings};
pub use time::{format_timestamp, parse_time};

/// Errors from transport commands.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TransportError {
    #[error("speed {0} is out of range (allowed: {MIN_SPEED}..={MAX_SPEED}, 1.0 is normal speed)")]
    BadSpeed(f64),
    #[error("{0}")]
    BadSetting(String),
    #[error("the recording cannot be played: {0}")]
    BadAudio(String),
}

/// What the transport is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Stopped,
    Playing,
    /// Moving backwards while a rewind control is held; silent.
    Rewinding,
    /// Moving forwards while a fast-forward control is held; silent.
    FastForwarding,
}

/// Which speed preset is active. Toggling a preset that is already active returns to normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedPreset {
    #[default]
    Normal,
    Slow,
    Fast,
    Custom,
}

/// Playback of one recording.
#[derive(Debug)]
pub struct Transport {
    audio: Arc<AudioBuffer>,
    channels: usize,
    settings: Settings,
    state: State,
    /// State to return to when a held rewind/fast-forward control is released.
    resume: State,
    /// Read position in source frames.
    pos: f64,
    speed: f64,
    preset: SpeedPreset,
    gain: f32,
    wsola: Wsola,
    /// One WSOLA hop of output, of which `pending_at..` has not been delivered yet.
    pending: Vec<f32>,
    pending_at: usize,
    needs_prime: bool,
}

impl Transport {
    /// Creates a stopped transport at the start of `audio`.
    pub fn new(audio: Arc<AudioBuffer>, settings: Settings) -> Result<Self, TransportError> {
        settings.validate()?;
        let wsola = Wsola::new(audio.sample_rate, audio.channels).map_err(|e| TransportError::BadAudio(e.to_string()))?;
        let channels = wsola.channels();
        let pending = vec![0.0; wsola.hop_frames() * channels];
        let pending_at = pending.len();
        let gain = db_to_gain(settings.volume_db);
        Ok(Self {
            audio,
            channels,
            settings,
            state: State::Stopped,
            resume: State::Stopped,
            pos: 0.0,
            speed: 1.0,
            preset: SpeedPreset::Normal,
            gain,
            wsola,
            pending,
            pending_at,
            needs_prime: true,
        })
    }

    pub fn audio(&self) -> &Arc<AudioBuffer> {
        &self.audio
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn speed(&self) -> f64 {
        self.speed
    }
    pub fn preset(&self) -> SpeedPreset {
        self.preset
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn sample_rate(&self) -> u32 {
        self.audio.sample_rate
    }

    /// Current position in milliseconds from the start.
    pub fn position_ms(&self) -> u64 {
        frames_to_ms(self.pos.max(0.0) as u64, self.audio.sample_rate)
    }

    /// Length of the recording in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.audio.duration_ms()
    }

    fn total_frames(&self) -> f64 {
        self.audio.frames() as f64
    }

    /// Replaces the settings (volume and channel routing apply immediately).
    pub fn set_settings(&mut self, settings: Settings) -> Result<(), TransportError> {
        settings.validate()?;
        self.gain = db_to_gain(settings.volume_db);
        self.settings = settings;
        Ok(())
    }

    /// Starts playing from the current position. At the end, starts again from the beginning.
    pub fn play(&mut self) {
        if self.pos >= self.total_frames() {
            self.pos = 0.0;
        }
        self.set_state(State::Playing);
        self.resume = State::Playing;
    }

    /// Stops. When it was playing, steps back by the auto-backstep time so that the next
    /// play repeats the last words heard.
    pub fn stop(&mut self) {
        if self.state == State::Playing {
            let back = ms_to_frames(self.settings.auto_backstep_ms, self.audio.sample_rate) as f64;
            self.pos = (self.pos - back).max(0.0);
        }
        self.set_state(State::Stopped);
        self.resume = State::Stopped;
    }

    /// Play when stopped, stop (with backstep) otherwise.
    pub fn toggle_play(&mut self) {
        if self.state == State::Playing { self.stop() } else { self.play() }
    }

    /// Starts rewinding while a control is held; [`Transport::release`] ends it.
    pub fn rewind(&mut self) {
        self.hold(State::Rewinding);
    }

    /// Starts fast-forwarding while a control is held; [`Transport::release`] ends it.
    pub fn fast_forward(&mut self) {
        self.hold(State::FastForwarding);
    }

    fn hold(&mut self, state: State) {
        if !matches!(self.state, State::Rewinding | State::FastForwarding) {
            self.resume = self.state;
        }
        self.set_state(state);
    }

    /// Releases a held rewind or fast-forward: playback resumes if it was playing before.
    pub fn release(&mut self) {
        if matches!(self.state, State::Rewinding | State::FastForwarding) {
            let resume = self.resume;
            self.set_state(resume);
        }
    }

    /// Moves to `ms` (clamped to the recording).
    pub fn seek_ms(&mut self, ms: u64) {
        self.pos = (ms_to_frames(ms, self.audio.sample_rate) as f64).min(self.total_frames());
        self.needs_prime = true;
        self.pending_at = self.pending.len();
    }

    /// Moves by `delta_ms` (negative is backwards), clamped to the recording.
    pub fn skip_ms(&mut self, delta_ms: i64) {
        let now = self.position_ms();
        let target = if delta_ms < 0 { now.saturating_sub(delta_ms.unsigned_abs()) } else { now.saturating_add(delta_ms.unsigned_abs()) };
        self.seek_ms(target);
    }

    /// Sets the playback speed (1.0 = normal) without changing pitch.
    pub fn set_speed(&mut self, speed: f64) -> Result<(), TransportError> {
        if !speed.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
            return Err(TransportError::BadSpeed(speed));
        }
        self.apply_speed(speed);
        self.preset = if speed == 1.0 { SpeedPreset::Normal } else { SpeedPreset::Custom };
        Ok(())
    }

    /// Toggles the slow-speed preset.
    pub fn toggle_slow(&mut self) {
        self.toggle_preset(SpeedPreset::Slow, self.settings.slow_speed);
    }

    /// Toggles the fast-speed preset.
    pub fn toggle_fast(&mut self) {
        self.toggle_preset(SpeedPreset::Fast, self.settings.fast_speed);
    }

    fn toggle_preset(&mut self, preset: SpeedPreset, speed: f64) {
        if self.preset == preset {
            self.apply_speed(1.0);
            self.preset = SpeedPreset::Normal;
        } else {
            self.apply_speed(speed);
            self.preset = preset;
        }
    }

    fn apply_speed(&mut self, speed: f64) {
        let was_stretching = self.speed != 1.0;
        self.speed = speed;
        if was_stretching && speed == 1.0 {
            // Continue exactly where the stretched output left off.
            if let Some(next) = self.wsola.continuation() {
                self.pos = (next as f64).min(self.total_frames());
            }
        }
        if !was_stretching && speed != 1.0 {
            self.needs_prime = true;
        }
    }

    fn set_state(&mut self, state: State) {
        if state != self.state {
            self.state = state;
            self.needs_prime = true;
            self.pending_at = self.pending.len();
        }
    }

    /// Fills `out` with interleaved audio at the recording's sample rate and channel count,
    /// and advances the transport. Real-time safe. Returns the number of frames that carry
    /// sound (the rest is silence because playback stopped or never ran).
    pub fn render(&mut self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        let frames = out.len() / ch;
        let mut written = 0;
        let mut audible = 0;
        while written < frames {
            let Some(rest) = out.get_mut(written * ch..frames * ch) else { break };
            let n = match self.state {
                State::Stopped => {
                    rest.fill(0.0);
                    break;
                }
                State::Rewinding | State::FastForwarding => {
                    rest.fill(0.0);
                    let rate = if self.state == State::Rewinding { -self.settings.rewind_rate } else { self.settings.fast_forward_rate };
                    // Held at either end until the control is released.
                    let moved = self.pos + rate * (rest.len() / ch) as f64;
                    self.pos = moved.clamp(0.0, self.total_frames());
                    break;
                }
                State::Playing => {
                    if self.pos >= self.total_frames() && self.pending_at >= self.pending.len() {
                        self.state = State::Stopped;
                        self.resume = State::Stopped;
                        rest.fill(0.0);
                        break;
                    }
                    // Stretched output already synthesised is delivered before switching to
                    // the direct path, which then continues where it ends.
                    let draining = self.pending_at < self.pending.len();
                    let n = if self.speed == 1.0 && !draining { self.copy_direct(rest) } else { self.copy_stretched(rest) };
                    audible += n;
                    n
                }
            };
            if n == 0 {
                break;
            }
            written += n;
        }
        if let Some(played) = out.get_mut(..audible * ch) {
            self.settings.channel_mode.apply(played, ch);
            apply_gain(played, self.gain);
        }
        audible
    }

    /// Bit-exact copy at normal speed.
    fn copy_direct(&mut self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        let start = self.pos as usize;
        let src = self.audio.samples.get(start.saturating_mul(ch)..).unwrap_or(&[]);
        let n = (out.len() / ch).min(src.len() / ch);
        if let (Some(dst), Some(s)) = (out.get_mut(..n * ch), src.get(..n * ch)) {
            dst.copy_from_slice(s);
        }
        self.pos = (start + n) as f64;
        n
    }

    /// Constant-pitch stretched copy, one WSOLA hop at a time.
    fn copy_stretched(&mut self, out: &mut [f32]) -> usize {
        let ch = self.channels;
        if self.pending_at >= self.pending.len() {
            if self.needs_prime {
                self.wsola.prime(&self.audio.samples, self.pos as usize);
                self.needs_prime = false;
            }
            if self.wsola.synthesize_hop(&self.audio.samples, self.pos as usize, &mut self.pending).is_err() {
                return 0;
            }
            self.pending_at = 0;
            self.pos = (self.pos + self.wsola.hop_frames() as f64 * self.speed).min(self.total_frames());
        }
        let avail = self.pending.get(self.pending_at..).unwrap_or(&[]);
        let n = (out.len() / ch).min(avail.len() / ch);
        if let (Some(dst), Some(s)) = (out.get_mut(..n * ch), avail.get(..n * ch)) {
            dst.copy_from_slice(s);
        }
        self.pending_at += n * ch;
        n
    }

    /// Renders `duration_ms` of what the listener hears, starting at `from_ms`, as a new
    /// buffer (used by `transport_render` and the CLI to verify changes by ear). Leaves the
    /// transport stopped at the end of the rendered range.
    pub fn render_range(&mut self, from_ms: u64, duration_ms: u64) -> AudioBuffer {
        self.seek_ms(from_ms);
        self.set_state(State::Playing);
        let out_frames = ms_to_frames(duration_ms, self.audio.sample_rate);
        let cap = (self.audio.frames() as f64 / MIN_SPEED) as u64 + 1;
        let out_frames = usize::try_from(out_frames.min(cap)).unwrap_or(0);
        let mut samples = vec![0.0; out_frames.saturating_mul(self.channels)];
        let audible = self.render(&mut samples);
        samples.truncate(audible * self.channels);
        self.set_state(State::Stopped);
        self.resume = State::Stopped;
        AudioBuffer { sample_rate: self.audio.sample_rate, channels: self.audio.channels, samples, codec: "pcm_f32".into(), repairs: Vec::new() }
    }
}

#[cfg(test)]
mod tests;
