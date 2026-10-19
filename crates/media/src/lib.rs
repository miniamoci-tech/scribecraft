//! Decoding recordings to interleaved PCM, WAV export, and synthetic test media (layer L1).

mod decode;
pub mod synth;
mod wav;

use std::path::Path;

pub use decode::{decode_bytes, decode_file};
pub use wav::{WavFormat, write_wav};

/// Errors from reading or writing media.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("cannot read or write {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("the file is {size} bytes, more than the {limit}-byte limit")]
    TooLarge { size: u64, limit: u64 },
    #[error("the decoded audio would exceed {limit} samples; split the recording or raise the limit")]
    TooLong { limit: usize },
    #[error("unsupported or unrecognised format: {0}")]
    Unsupported(String),
    #[error("the file has no audio track")]
    NoAudio,
    #[error("malformed audio data: {0}")]
    Malformed(String),
    #[error("unsupported audio format: {sample_rate} Hz, {channels} channels (allowed: 1..={MAX_SAMPLE_RATE} Hz, 1..={MAX_CHANNELS} channels)")]
    BadFormat { sample_rate: u32, channels: u16 },
}

/// Caps on what a single decode may allocate. Every input is untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest file accepted, in bytes.
    pub max_file_bytes: u64,
    /// Largest decoded size, in interleaved samples (frames × channels).
    pub max_samples: usize,
}

impl Default for Limits {
    /// 2 GiB files; 400 M samples (1.6 GB of `f32`, about 1 h 9 min of 48 kHz stereo or
    /// 6 h 56 min of 16 kHz mono dictation).
    fn default() -> Self {
        Self { max_file_bytes: 2 << 30, max_samples: 400_000_000 }
    }
}

/// Decoded audio: interleaved `f32` samples in [-1, 1].
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
    /// Codec name as reported by the decoder (for display only).
    pub codec: String,
    /// Problems that were skipped over while decoding (corrupt packets and the like).
    pub repairs: Vec<String>,
}

/// Highest sample rate accepted.
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted.
pub const MAX_CHANNELS: u16 = 32;

impl AudioBuffer {
    /// Creates a buffer after checking the format.
    pub fn new(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self, MediaError> {
        check_format(sample_rate, channels)?;
        Ok(Self { sample_rate, channels, samples, codec: "pcm_f32".into(), repairs: Vec::new() })
    }

    /// Number of whole frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Duration in milliseconds, rounded down.
    pub fn duration_ms(&self) -> u64 {
        frames_to_ms(self.frames() as u64, self.sample_rate)
    }
}

/// Rejects formats the rest of ScribeCraft cannot play.
pub fn check_format(sample_rate: u32, channels: u16) -> Result<(), MediaError> {
    if sample_rate == 0 || sample_rate > MAX_SAMPLE_RATE || channels == 0 || channels > MAX_CHANNELS {
        return Err(MediaError::BadFormat { sample_rate, channels });
    }
    Ok(())
}

/// Converts a frame count to milliseconds without overflow.
pub fn frames_to_ms(frames: u64, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    (u128::from(frames) * 1000 / u128::from(sample_rate)).try_into().unwrap_or(u64::MAX)
}

/// Converts milliseconds to a frame count without overflow.
pub fn ms_to_frames(ms: u64, sample_rate: u32) -> u64 {
    (u128::from(ms) * u128::from(sample_rate) / 1000).try_into().unwrap_or(u64::MAX)
}

/// File extensions this build decodes (lowercase, without the dot).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "mp2", "flac", "ogg", "oga", "m4a", "aac", "mp4", "m4v", "mov", "3gp", "aif", "aiff", "aifc", "caf", "mkv", "mka", "webm",
];

/// Whether `path` has an extension this build can decode.
pub fn is_supported_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| SUPPORTED_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_conversions_are_total() {
        assert_eq!(frames_to_ms(48_000, 48_000), 1000);
        assert_eq!(frames_to_ms(5, 0), 0);
        assert_eq!(frames_to_ms(u64::MAX, 1), u64::MAX);
        assert_eq!(ms_to_frames(1500, 16_000), 24_000);
        assert_eq!(ms_to_frames(u64::MAX, MAX_SAMPLE_RATE), u64::MAX);
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert!(is_supported_path(Path::new("a/b/Dictation.MP3")));
        assert!(!is_supported_path(Path::new("notes.docx")));
        assert!(!is_supported_path(Path::new("no_extension")));
    }

    #[test]
    fn bad_formats_are_rejected() {
        assert!(AudioBuffer::new(0, 1, vec![]).is_err());
        assert!(AudioBuffer::new(8000, 0, vec![]).is_err());
        assert!(AudioBuffer::new(8000, 33, vec![]).is_err());
        assert_eq!(AudioBuffer::new(8000, 2, vec![0.0; 5]).unwrap().frames(), 2);
    }
}
