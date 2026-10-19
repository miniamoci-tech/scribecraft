//! Whole-file decoding through Symphonia, bounded by [`Limits`] and guarded against panics.

use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::{AudioBuffer, Limits, MediaError, check_format};

/// At most this many corrupt packets are skipped before the file is declared malformed.
const MAX_REPAIRS: usize = 1000;

/// Decodes the first audio track of the file at `path`.
pub fn decode_file(path: &Path, limits: &Limits) -> Result<AudioBuffer, MediaError> {
    let io = |source| MediaError::Io { path: path.display().to_string(), source };
    let file = std::fs::File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    if size > limits.max_file_bytes {
        return Err(MediaError::TooLarge { size, limit: limits.max_file_bytes });
    }
    let ext = path.extension().and_then(|e| e.to_str());
    guarded(Box::new(file), ext, limits)
}

/// Decodes the first audio track of an in-memory file (used by the web build and tests).
/// `extension` is a hint such as `"mp3"`; the content is probed either way.
pub fn decode_bytes(bytes: Vec<u8>, extension: Option<&str>, limits: &Limits) -> Result<AudioBuffer, MediaError> {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > limits.max_file_bytes {
        return Err(MediaError::TooLarge { size, limit: limits.max_file_bytes });
    }
    guarded(Box::new(Cursor::new(bytes)), extension, limits)
}

/// Runs the decoder behind `catch_unwind`: a decoder bug on a hostile file becomes an error.
fn guarded(source: Box<dyn MediaSource>, extension: Option<&str>, limits: &Limits) -> Result<AudioBuffer, MediaError> {
    match catch_unwind(AssertUnwindSafe(|| decode(source, extension, limits))) {
        Ok(result) => result,
        Err(_) => Err(MediaError::Malformed("the decoder failed on this file".into())),
    }
}

fn decode(source: Box<dyn MediaSource>, extension: Option<&str>, limits: &Limits) -> Result<AudioBuffer, MediaError> {
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let mut format = probed.format;
    let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or(MediaError::NoAudio)?;
    let track_id = track.id;
    let codec =
        symphonia::default::get_codecs().get_codec(track.codec_params.codec).map(|d| d.short_name.to_string()).unwrap_or_else(|| "unknown".into());
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| MediaError::Unsupported(e.to_string()))?;

    let mut samples: Vec<f32> = Vec::new();
    let mut format_seen: Option<(u32, u16)> = None;
    let mut repairs = Vec::new();
    let mut scratch: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => {
                if samples.is_empty() {
                    return Err(MediaError::Malformed(e.to_string()));
                }
                repairs.push(format!("stopped reading at a damaged container section: {e}"));
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(e @ (SymError::DecodeError(_) | SymError::IoError(_))) => {
                repairs.push(format!("skipped a damaged packet: {e}"));
                if repairs.len() > MAX_REPAIRS {
                    return Err(MediaError::Malformed("too many damaged packets".into()));
                }
                continue;
            }
            Err(e) => return Err(MediaError::Malformed(e.to_string())),
        };
        let spec = *decoded.spec();
        let rate = spec.rate;
        let channels = u16::try_from(spec.channels.count()).unwrap_or(u16::MAX);
        match format_seen {
            None => {
                check_format(rate, channels)?;
                format_seen = Some((rate, channels));
            }
            Some(seen) if seen != (rate, channels) => {
                repairs.push(format!("skipped a packet whose format changed to {rate} Hz, {channels} channels"));
                continue;
            }
            Some(_) => {}
        }
        let capacity = decoded.capacity() as u64;
        let buf = match &mut scratch {
            Some(b) if b.capacity() as u64 >= capacity * u64::from(channels) => b,
            _ => scratch.insert(SampleBuffer::new(capacity, spec)),
        };
        buf.copy_interleaved_ref(decoded);
        let chunk = buf.samples();
        if samples.len().saturating_add(chunk.len()) > limits.max_samples {
            return Err(MediaError::TooLong { limit: limits.max_samples });
        }
        samples.extend(chunk.iter().map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }));
    }
    let (sample_rate, channels) = format_seen.ok_or(MediaError::NoAudio)?;
    Ok(AudioBuffer { sample_rate, channels, samples, codec, repairs })
}
