//! Constant-pitch time stretching by waveform-similarity overlap-add (WSOLA).
//!
//! Published algorithm: W. Verhelst and M. Roelands, "An overlap-add technique based on
//! waveform similarity (WSOLA) for high quality time-scale modification of speech",
//! ICASSP 1993. Implemented here from the paper's description.
//!
//! Each call to [`Wsola::synthesize_hop`] emits `hop` output frames. The caller advances its
//! read position by `hop * speed` frames per call, so the output plays `1 / speed` times as
//! long as the input while every waveform period keeps its length (and the voice its pitch).
//! Frames are `2 * hop` long and Hann-windowed; consecutive frames overlap by half, where the
//! periodic Hann window sums to exactly one.

use crate::{DspError, validate_format};

/// Synthesis hop: half of a 30 ms analysis frame, which suits speech.
const HOP_SECONDS: f64 = 0.015;
/// How far around the nominal read position the alignment search looks.
const TOLERANCE_SECONDS: f64 = 0.010;
/// Coarse search step in frames; the best coarse candidate is refined at step 1.
const COARSE_STEP: usize = 4;

/// A streaming WSOLA time stretcher for one interleaved source.
///
/// Allocates only in [`Wsola::new`]; every other method is real-time safe.
#[derive(Debug, Clone)]
pub struct Wsola {
    channels: usize,
    hop: usize,
    tolerance: usize,
    window: Vec<f32>,
    overlap: Vec<f32>,
    reference: Vec<f32>,
    /// Source frame where the previous output frame's natural continuation starts.
    natural: Option<usize>,
}

impl Wsola {
    /// Creates a stretcher for audio at `sample_rate` with `channels` interleaved channels.
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, DspError> {
        let channels = validate_format(sample_rate, channels)?;
        let rate = f64::from(sample_rate);
        // Bounded by MAX_SAMPLE_RATE * 0.015 = 11 520, so the casts cannot overflow.
        let hop = ((rate * HOP_SECONDS).round() as usize).max(8);
        let tolerance = ((rate * TOLERANCE_SECONDS).round() as usize).max(2);
        let len = hop * 2;
        let window = (0..len).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / len as f64).cos()) as f32).collect();
        Ok(Self { channels, hop, tolerance, window, overlap: vec![0.0; hop * channels], reference: vec![0.0; hop], natural: None })
    }

    /// Frames produced by each [`Wsola::synthesize_hop`] call.
    pub fn hop_frames(&self) -> usize {
        self.hop
    }

    /// Number of interleaved channels.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Source frame where seamless continuation of the output so far would start, if any.
    pub fn continuation(&self) -> Option<usize> {
        self.natural
    }

    /// Forgets all history: the next hop fades in from silence.
    pub fn reset(&mut self) {
        self.overlap.fill(0.0);
        self.natural = None;
    }

    /// Seeds the stretcher so that output continues seamlessly from source frame `pos`,
    /// as if the frames before `pos` had just been played at normal speed.
    pub fn prime(&mut self, src: &[f32], pos: usize) {
        let ch = self.channels;
        let tail = self.window.get(self.hop..).unwrap_or(&[]);
        for (i, (dst, w)) in self.overlap.chunks_exact_mut(ch).zip(tail).enumerate() {
            for (c, d) in dst.iter_mut().enumerate() {
                *d = w * sample(src, ch, pos.saturating_add(i), c);
            }
        }
        self.natural = Some(pos);
    }

    /// Writes the next `hop_frames()` interleaved frames into `out`, reading the source
    /// around frame `nominal`. Reads past the end of `src` see silence.
    pub fn synthesize_hop(&mut self, src: &[f32], nominal: usize, out: &mut [f32]) -> Result<(), DspError> {
        let expected = self.hop * self.channels;
        if out.len() != expected {
            return Err(DspError::BadBuffer { expected, got: out.len() });
        }
        let chosen = match self.natural {
            None => nominal,
            Some(natural) => self.search(src, nominal, natural),
        };
        let ch = self.channels;
        let (head, tail) = self.window.split_at(self.hop);
        let frames = out.chunks_exact_mut(ch).zip(self.overlap.chunks_exact_mut(ch)).zip(head.iter().zip(tail));
        for (i, ((o, ov), (wh, wt))) in frames.enumerate() {
            let a = chosen.saturating_add(i);
            let b = a.saturating_add(self.hop);
            for (c, (o, ov)) in o.iter_mut().zip(ov.iter_mut()).enumerate() {
                *o = *ov + wh * sample(src, ch, a, c);
                *ov = wt * sample(src, ch, b, c);
            }
        }
        self.natural = Some(chosen.saturating_add(self.hop));
        Ok(())
    }

    /// Finds the source frame near `nominal` whose start best matches the natural
    /// continuation at `natural` (normalised cross-correlation on a mono sum).
    fn search(&mut self, src: &[f32], nominal: usize, natural: usize) -> usize {
        if nominal == natural {
            return natural;
        }
        let ch = self.channels;
        for (i, r) in self.reference.iter_mut().enumerate() {
            *r = mono(src, ch, natural.saturating_add(i));
        }
        let lo = nominal.saturating_sub(self.tolerance);
        let hi = nominal.saturating_add(self.tolerance);
        let reference = &self.reference;
        let best_of = |cands: &mut dyn Iterator<Item = usize>, start: (usize, f32)| {
            cands.fold(start, |best, cand| {
                let score = similarity(src, ch, cand, reference);
                if score > best.1 { (cand, score) } else { best }
            })
        };
        let coarse = best_of(&mut (lo..=hi).step_by(COARSE_STEP), (nominal, f32::NEG_INFINITY));
        let fine_lo = coarse.0.saturating_sub(COARSE_STEP - 1).max(lo);
        let fine_hi = coarse.0.saturating_add(COARSE_STEP - 1).min(hi);
        best_of(&mut (fine_lo..=fine_hi), coarse).0
    }
}

/// Normalised correlation of the source at `cand` against `reference`, on every other frame.
fn similarity(src: &[f32], ch: usize, cand: usize, reference: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut energy = 0.0f32;
    for (i, r) in reference.iter().enumerate().step_by(2) {
        let m = mono(src, ch, cand.saturating_add(i));
        dot += m * r;
        energy += m * m;
    }
    dot / (energy + 1e-9).sqrt()
}

/// Sample of channel `c` at frame `frame`, or silence outside the buffer.
fn sample(src: &[f32], ch: usize, frame: usize, c: usize) -> f32 {
    frame.checked_mul(ch).and_then(|s| s.checked_add(c)).and_then(|i| src.get(i)).copied().unwrap_or(0.0)
}

/// Mean of all channels at `frame`.
fn mono(src: &[f32], ch: usize, frame: usize) -> f32 {
    let Some(start) = frame.checked_mul(ch) else { return 0.0 };
    match src.get(start..start.saturating_add(ch)) {
        Some(f) => f.iter().sum::<f32>() / ch as f32,
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f64, seconds: f64, channels: usize) -> Vec<f32> {
        let n = (f64::from(rate) * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (std::f64::consts::TAU * freq * i as f64 / f64::from(rate)).sin() as f32 * 0.5;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn stretch(src: &[f32], rate: u32, channels: u16, speed: f64) -> Vec<f32> {
        let mut w = Wsola::new(rate, channels).unwrap();
        let ch = usize::from(channels);
        let frames = src.len() / ch;
        let hop = w.hop_frames();
        let mut out = Vec::new();
        let mut buf = vec![0.0; hop * ch];
        let mut pos = 0.0f64;
        w.prime(src, 0);
        while (pos as usize) + 2 * hop < frames {
            w.synthesize_hop(src, pos as usize, &mut buf).unwrap();
            out.extend_from_slice(&buf);
            pos += hop as f64 * speed;
        }
        out
    }

    /// Estimates frequency from rising zero crossings of channel 0.
    fn frequency(buf: &[f32], rate: u32, ch: usize) -> f64 {
        let ch0: Vec<f32> = buf.iter().step_by(ch).copied().collect();
        let crossings: Vec<usize> = ch0.windows(2).enumerate().filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0).map(|(i, _)| i).collect();
        let (first, last) = (crossings[0], *crossings.last().unwrap());
        (crossings.len() - 1) as f64 * f64::from(rate) / (last - first) as f64
    }

    #[test]
    fn slowing_down_keeps_pitch_and_lengthens_duration() {
        let rate = 16_000;
        let src = sine(rate, 440.0, 2.0, 1);
        for speed in [0.5, 0.75, 1.5, 2.0] {
            let out = stretch(&src, rate, 1, speed);
            let f = frequency(&out, rate, 1);
            assert!((f - 440.0).abs() < 440.0 * 0.02, "speed {speed}: pitch drifted to {f} Hz");
            let expected = src.len() as f64 / speed;
            let ratio = out.len() as f64 / expected;
            assert!((0.9..1.05).contains(&ratio), "speed {speed}: length ratio {ratio}");
        }
    }

    #[test]
    fn normal_speed_reconstructs_the_source() {
        let rate = 8_000;
        let src = sine(rate, 300.0, 0.5, 2);
        let out = stretch(&src, rate, 2, 1.0);
        for (a, b) in out.iter().zip(&src) {
            assert!((a - b).abs() < 1e-5, "{a} vs {b}");
        }
    }

    #[test]
    fn stereo_channels_stay_aligned() {
        let rate = 8_000;
        let src: Vec<f32> = sine(rate, 200.0, 1.0, 1).into_iter().flat_map(|v| [v, -v]).collect();
        let out = stretch(&src, rate, 2, 0.6);
        for f in out.as_chunks::<2>().0 {
            assert!((f[0] + f[1]).abs() < 1e-6, "channels diverged: {f:?}");
        }
    }

    #[test]
    fn rejects_bad_formats_and_buffers() {
        assert_eq!(Wsola::new(0, 1).unwrap_err(), DspError::BadSampleRate(0));
        assert_eq!(Wsola::new(8_000, 0).unwrap_err(), DspError::BadChannels(0));
        let mut w = Wsola::new(8_000, 2).unwrap();
        let mut short = vec![0.0; 3];
        assert!(matches!(w.synthesize_hop(&[], 0, &mut short), Err(DspError::BadBuffer { .. })));
    }

    #[test]
    fn reading_past_the_end_or_at_huge_positions_yields_silence() {
        let mut w = Wsola::new(8_000, 1).unwrap();
        let mut buf = vec![1.0; w.hop_frames()];
        w.prime(&[0.5; 10], usize::MAX);
        w.synthesize_hop(&[0.5; 10], usize::MAX, &mut buf).unwrap();
        w.synthesize_hop(&[0.5; 10], usize::MAX - 3, &mut buf).unwrap();
        assert!(buf.iter().all(|v| *v == 0.0));
    }

    proptest::proptest! {
        #[test]
        fn any_position_and_source_is_total(src in proptest::collection::vec(-1.0f32..1.0, 0..400), pos in 0usize..1000, natural in proptest::option::of(0usize..1000)) {
            let mut w = Wsola::new(8_000, 2).unwrap();
            if let Some(n) = natural { w.prime(&src, n); }
            let mut buf = vec![0.0; w.hop_frames() * 2];
            w.synthesize_hop(&src, pos, &mut buf).unwrap();
            proptest::prop_assert!(buf.iter().all(|v| v.is_finite() && v.abs() <= 2.0));
        }
    }
}
