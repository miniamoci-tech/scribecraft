//! Streaming conversion from the source format to the output device's format.

/// Linear-interpolation sample-rate and channel-count converter.
///
/// Speech intelligibility, not mastering, is the goal; a windowed-sinc resampler is tracked
/// in `docs/gaps.md`. Pull-based: [`Converter::fill`] asks a callback for source frames.
#[derive(Debug, Clone)]
pub struct Converter {
    in_channels: usize,
    out_channels: usize,
    step: f64,
    phase: f64,
    prev: Vec<f32>,
    next: Vec<f32>,
    primed: bool,
}

impl Converter {
    /// Converts from `in_rate`/`in_channels` to `out_rate`/`out_channels`. Invalid values
    /// are clamped to 1 so the converter is always usable.
    pub fn new(in_rate: u32, in_channels: u16, out_rate: u32, out_channels: u16) -> Self {
        let in_channels = usize::from(in_channels.max(1));
        let out_channels = usize::from(out_channels.max(1));
        let step = f64::from(in_rate.max(1)) / f64::from(out_rate.max(1));
        Self { in_channels, out_channels, step, phase: 0.0, prev: vec![0.0; in_channels], next: vec![0.0; in_channels], primed: false }
    }

    /// Drops interpolation history (after a seek).
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.primed = false;
    }

    /// Fills `out` (interleaved, output format). `pull` writes one source frame into its
    /// argument and must not allocate.
    pub fn fill(&mut self, out: &mut [f32], mut pull: impl FnMut(&mut [f32])) {
        if !self.primed {
            pull(&mut self.prev);
            pull(&mut self.next);
            self.primed = true;
        }
        for frame in out.chunks_exact_mut(self.out_channels) {
            while self.phase >= 1.0 {
                std::mem::swap(&mut self.prev, &mut self.next);
                pull(&mut self.next);
                self.phase -= 1.0;
            }
            let t = self.phase as f32;
            let in_ch = self.in_channels;
            for (c, o) in frame.iter_mut().enumerate() {
                // Map output channels onto input channels: mono feeds every output,
                // otherwise output c takes input c, and extra outputs repeat the last input.
                let src = if in_ch == 1 { 0 } else { c.min(in_ch - 1) };
                let a = self.prev.get(src).copied().unwrap_or(0.0);
                let b = self.next.get(src).copied().unwrap_or(0.0);
                *o = a + (b - a) * t;
            }
            self.phase += self.step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_format_is_a_one_frame_delay_line() {
        let mut c = Converter::new(8_000, 2, 8_000, 2);
        let mut n = 0.0;
        let mut out = vec![0.0; 8];
        c.fill(&mut out, |f| {
            n += 1.0;
            f.fill(n);
        });
        assert_eq!(out, vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0]);
    }

    #[test]
    fn upsampling_doubles_frames_and_mono_feeds_both_outputs() {
        let mut c = Converter::new(8_000, 1, 16_000, 2);
        let mut pulled = 0;
        let mut out = vec![0.0; 2 * 100];
        c.fill(&mut out, |f| {
            pulled += 1;
            f.fill(pulled as f32);
        });
        assert!((50..=52).contains(&pulled), "pulled {pulled}");
        assert!(out.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
        assert_eq!(out[2], 1.5);
    }
}
