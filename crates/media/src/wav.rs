//! Atomic WAV export.

use std::path::{Path, PathBuf};

use crate::{AudioBuffer, MediaError, check_format};

/// Sample format of an exported WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WavFormat {
    /// 16-bit integer PCM (plays everywhere).
    #[default]
    Pcm16,
    /// 32-bit float (lossless for our `f32` pipeline).
    Float32,
}

/// Writes `audio` to `path` as WAV. The data goes to a temporary file next to `path`,
/// which is renamed over `path` only once complete, so a failure never leaves a torn file.
pub fn write_wav(path: &Path, audio: &AudioBuffer, format: WavFormat) -> Result<(), MediaError> {
    check_format(audio.sample_rate, audio.channels)?;
    let tmp = temp_path(path);
    let result = write_to(&tmp, audio, format)
        .and_then(|()| std::fs::rename(&tmp, path).map_err(|source| MediaError::Io { path: path.display().to_string(), source }));
    if result.is_err() {
        // Best effort: the temporary file may not exist, and the original error matters more.
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

fn write_to(tmp: &Path, audio: &AudioBuffer, format: WavFormat) -> Result<(), MediaError> {
    let err = |e: hound::Error| match e {
        hound::Error::IoError(source) => MediaError::Io { path: tmp.display().to_string(), source },
        other => MediaError::Malformed(other.to_string()),
    };
    let (bits_per_sample, sample_format) = match format {
        WavFormat::Pcm16 => (16, hound::SampleFormat::Int),
        WavFormat::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: audio.channels, sample_rate: audio.sample_rate, bits_per_sample, sample_format };
    let mut writer = hound::WavWriter::create(tmp, spec).map_err(err)?;
    // Only whole frames are written; a trailing partial frame would corrupt the file.
    let whole = audio.frames() * usize::from(audio.channels);
    for s in audio.samples.iter().take(whole) {
        let s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        match format {
            WavFormat::Pcm16 => writer.write_sample((s * f32::from(i16::MAX)).round() as i16).map_err(err)?,
            WavFormat::Float32 => writer.write_sample(s).map_err(err)?,
        }
    }
    writer.finalize().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Limits, decode_file};

    #[test]
    fn round_trips_through_wav() {
        let dir = std::env::temp_dir().join(format!("scribecraft-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let audio = crate::synth::dictation(16_000, 2, 1.0);
        for format in [WavFormat::Pcm16, WavFormat::Float32] {
            write_wav(&path, &audio, format).unwrap();
            let back = decode_file(&path, &Limits::default()).unwrap();
            assert_eq!((back.sample_rate, back.channels, back.frames()), (16_000, 2, audio.frames()));
            let tolerance = if format == WavFormat::Pcm16 { 1e-4 } else { 0.0 };
            assert!(back.samples.iter().zip(&audio.samples).all(|(a, b)| (a - b).abs() <= tolerance));
            assert!(!temp_path(&path).exists());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_no_file() {
        let path = std::env::temp_dir().join("scribecraft-no-such-dir-xyz").join("out.wav");
        let audio = crate::synth::dictation(8_000, 1, 0.1);
        assert!(matches!(write_wav(&path, &audio, WavFormat::Pcm16), Err(MediaError::Io { .. })));
        assert!(!path.exists());
    }
}
