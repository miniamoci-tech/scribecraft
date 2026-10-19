//! Real-time-safe signal processing for ScribeCraft (layer L0, no dependencies).
//!
//! Everything here works on **interleaved** `f32` buffers: frame `f`, channel `c` lives at
//! `f * channels + c`. Positions passed in are in frames, never samples.

mod channels;
mod resample;
mod wsola;

pub use channels::{ChannelMode, apply_gain, db_to_gain};
pub use resample::Converter;
pub use wsola::Wsola;

/// Errors from constructing DSP processors with bad parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DspError {
    /// The sample rate is zero or above [`MAX_SAMPLE_RATE`].
    BadSampleRate(u32),
    /// The channel count is zero or above [`MAX_CHANNELS`].
    BadChannels(u16),
    /// An output buffer has the wrong length; `expected` and `got` are in samples.
    BadBuffer { expected: usize, got: usize },
}

/// Highest sample rate accepted (768 kHz covers every real recorder).
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted.
pub const MAX_CHANNELS: u16 = 32;

impl core::fmt::Display for DspError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DspError::BadSampleRate(r) => write!(f, "sample rate {r} Hz is not supported (allowed: 1..={MAX_SAMPLE_RATE})"),
            DspError::BadChannels(c) => write!(f, "{c} channels are not supported (allowed: 1..={MAX_CHANNELS})"),
            DspError::BadBuffer { expected, got } => write!(f, "output buffer holds {got} samples, expected {expected}"),
        }
    }
}

impl std::error::Error for DspError {}

/// Checks a sample rate and channel count and returns the channel count as `usize`.
pub fn validate_format(sample_rate: u32, channels: u16) -> Result<usize, DspError> {
    if sample_rate == 0 || sample_rate > MAX_SAMPLE_RATE {
        return Err(DspError::BadSampleRate(sample_rate));
    }
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(DspError::BadChannels(channels));
    }
    Ok(usize::from(channels))
}
