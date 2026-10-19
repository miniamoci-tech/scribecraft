//! Channel routing and gain, applied in place to interleaved buffers.

/// Which input channels the listener hears (Express Scribe's "multi-channel control").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelMode {
    /// Every channel as recorded.
    #[default]
    Stereo,
    /// The first channel on every output channel.
    Left,
    /// The second channel on every output channel (the first, for mono sources).
    Right,
    /// The mean of all channels on every output channel.
    Mix,
}

impl ChannelMode {
    /// Stable lowercase name used by settings files and automation tools.
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelMode::Stereo => "stereo",
            ChannelMode::Left => "left",
            ChannelMode::Right => "right",
            ChannelMode::Mix => "mix",
        }
    }

    /// Parses [`ChannelMode::as_str`] names.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "stereo" => Some(ChannelMode::Stereo),
            "left" => Some(ChannelMode::Left),
            "right" => Some(ChannelMode::Right),
            "mix" => Some(ChannelMode::Mix),
            _ => None,
        }
    }

    /// Routes `buf` (interleaved, `channels` per frame) in place. Mono and zero-channel
    /// buffers are left unchanged; a trailing partial frame is ignored.
    pub fn apply(self, buf: &mut [f32], channels: usize) {
        if channels < 2 || self == ChannelMode::Stereo {
            return;
        }
        for frame in buf.chunks_exact_mut(channels) {
            let v = match self {
                ChannelMode::Stereo => continue,
                ChannelMode::Left => frame.first().copied().unwrap_or(0.0),
                ChannelMode::Right => frame.get(1).copied().unwrap_or(0.0),
                ChannelMode::Mix => frame.iter().sum::<f32>() / channels as f32,
            };
            frame.fill(v);
        }
    }
}

/// Converts decibels to a linear gain; non-finite input gives unity gain.
pub fn db_to_gain(db: f32) -> f32 {
    if db.is_finite() { 10f32.powf(db.clamp(-96.0, 24.0) / 20.0) } else { 1.0 }
}

/// Multiplies `buf` by `gain` and clips to [-1, 1]. Non-finite gain or samples become silence.
pub fn apply_gain(buf: &mut [f32], gain: f32) {
    let gain = if gain.is_finite() { gain.max(0.0) } else { 0.0 };
    for s in buf {
        let v = *s * gain;
        *s = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_channels() {
        let mut b = vec![0.2, 0.6, 0.4, 0.8, 1.0];
        ChannelMode::Left.apply(&mut b, 2);
        assert_eq!(b, vec![0.2, 0.2, 0.4, 0.4, 1.0]);
        let mut b = vec![0.2, 0.6];
        ChannelMode::Right.apply(&mut b, 2);
        assert_eq!(b, vec![0.6, 0.6]);
        let mut b = vec![0.2, 0.6];
        ChannelMode::Mix.apply(&mut b, 2);
        assert!((b[0] - 0.4).abs() < 1e-6 && b[0] == b[1]);
        let mut mono = vec![0.3];
        ChannelMode::Right.apply(&mut mono, 1);
        assert_eq!(mono, vec![0.3]);
        for m in [ChannelMode::Stereo, ChannelMode::Left, ChannelMode::Right, ChannelMode::Mix] {
            assert_eq!(ChannelMode::parse(m.as_str()), Some(m));
        }
    }

    #[test]
    fn gain_clips_and_rejects_nan() {
        let mut b = vec![0.5, -0.9, f32::NAN];
        apply_gain(&mut b, 2.0);
        assert_eq!(b, vec![1.0, -1.0, 0.0]);
        let mut b = vec![0.5];
        apply_gain(&mut b, f32::INFINITY);
        assert_eq!(b, vec![0.0]);
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_gain(-6.0) - 0.501).abs() < 1e-3);
        assert_eq!(db_to_gain(f32::NAN), 1.0);
    }
}
