//! Synthetic test "dictations": deterministic, contain no personal data, and exercise what a
//! transcription player cares about (syllable-like bursts, pauses, distinct stereo channels).
//! These are the only recordings ScribeCraft commits or publishes (`AGENTS.md` §1.2).

use crate::AudioBuffer;

/// Fundamental frequency of the voice on channel 0 ("interviewer").
pub const LEFT_PITCH_HZ: f64 = 140.0;
/// Fundamental frequency of the voice on channel 1 ("interviewee").
pub const RIGHT_PITCH_HZ: f64 = 230.0;

/// A speech-like recording: harmonic "syllables" of 180 ms with 70 ms gaps and a 400 ms pause
/// every eight syllables. With two or more channels, channel 1 carries a second voice at a
/// different pitch that talks during channel 0's pauses, like a two-microphone interview.
///
/// `sample_rate` and `channels` are clamped to valid values; `seconds` to 0..=3600.
pub fn dictation(sample_rate: u32, channels: u16, seconds: f64) -> AudioBuffer {
    let rate = sample_rate.clamp(1_000, crate::MAX_SAMPLE_RATE);
    let ch = channels.clamp(1, crate::MAX_CHANNELS);
    let seconds = if seconds.is_finite() { seconds.clamp(0.0, 3600.0) } else { 0.0 };
    let frames = (f64::from(rate) * seconds) as usize;
    let mut samples = Vec::with_capacity(frames * usize::from(ch));
    for i in 0..frames {
        let t = i as f64 / f64::from(rate);
        let left = voice(t, LEFT_PITCH_HZ, 0.0);
        let right = voice(t, RIGHT_PITCH_HZ, 0.125);
        for c in 0..ch {
            samples.push(match c {
                0 => left,
                1 => right,
                _ => 0.5 * (left + right),
            });
        }
    }
    AudioBuffer { sample_rate: rate, channels: ch, samples, codec: "synthetic".into(), repairs: Vec::new() }
}

/// A pure sine tone, for pitch measurements.
pub fn tone(sample_rate: u32, channels: u16, freq_hz: f64, seconds: f64) -> AudioBuffer {
    let rate = sample_rate.clamp(1_000, crate::MAX_SAMPLE_RATE);
    let ch = channels.clamp(1, crate::MAX_CHANNELS);
    let seconds = if seconds.is_finite() { seconds.clamp(0.0, 3600.0) } else { 0.0 };
    let freq = if freq_hz.is_finite() { freq_hz.clamp(1.0, f64::from(rate) / 2.0) } else { 440.0 };
    let frames = (f64::from(rate) * seconds) as usize;
    let samples = (0..frames)
        .flat_map(|i| {
            let v = (0.5 * (std::f64::consts::TAU * freq * i as f64 / f64::from(rate)).sin()) as f32;
            std::iter::repeat_n(v, usize::from(ch))
        })
        .collect();
    AudioBuffer { sample_rate: rate, channels: ch, samples, codec: "synthetic".into(), repairs: Vec::new() }
}

/// One voice: a 250 ms syllable cycle (180 ms on), silent for 400 ms after every 8 syllables.
/// `phase` shifts the pattern (in fractions of the 2.4 s phrase) so two voices alternate.
fn voice(t: f64, pitch: f64, phase: f64) -> f32 {
    const PHRASE: f64 = 2.4;
    let p = (t / PHRASE + phase).fract() * PHRASE;
    if p >= 2.0 {
        return 0.0;
    }
    let s = p % 0.25;
    if s >= 0.18 {
        return 0.0;
    }
    let envelope = (std::f64::consts::PI * s / 0.18).sin();
    // Slight pitch movement within the syllable, like intonation.
    let f = pitch * (1.0 + 0.05 * (std::f64::consts::TAU * 0.8 * t).sin());
    let w = std::f64::consts::TAU * f * t;
    let wave = 0.6 * w.sin() + 0.25 * (2.0 * w).sin() + 0.15 * (3.0 * w).sin();
    (0.6 * envelope * wave) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_is_deterministic_bounded_and_has_pauses() {
        let a = dictation(16_000, 2, 3.0);
        assert_eq!(a, dictation(16_000, 2, 3.0));
        assert_eq!(a.frames(), 48_000);
        assert!(a.samples.iter().all(|s| s.abs() <= 1.0));
        let silent = a.samples.iter().step_by(2).filter(|s| **s == 0.0).count();
        assert!(silent > 10_000, "expected pauses, found {silent} silent frames");
    }

    #[test]
    fn hostile_parameters_are_clamped() {
        assert_eq!(dictation(0, 0, f64::NAN).frames(), 0);
        assert_eq!(tone(u32::MAX, u16::MAX, f64::INFINITY, 0.001).channels, crate::MAX_CHANNELS);
    }
}
