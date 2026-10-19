use super::*;
use scribecraft_media::synth;

fn transport(seconds: f64) -> Transport {
    Transport::new(Arc::new(synth::dictation(8_000, 2, seconds)), Settings::default()).unwrap()
}

/// Renders `ms` of output in callback-sized blocks, like an audio device would.
fn run(t: &mut Transport, ms: u64) -> Vec<f32> {
    let ch = t.channels();
    let mut left = ms_to_frames(ms, t.sample_rate()) as usize;
    let mut out = Vec::new();
    while left > 0 {
        let mut block = vec![0.0; left.min(256) * ch];
        t.render(&mut block);
        out.extend_from_slice(&block);
        left -= block.len() / ch;
    }
    out
}

#[test]
fn normal_speed_is_bit_exact() {
    let mut t = transport(2.0);
    t.play();
    let out = run(&mut t, 1_000);
    assert_eq!(&out[..], &t.audio().samples[..out.len()]);
}

#[test]
fn stop_steps_back_by_the_auto_backstep() {
    let mut t = transport(5.0);
    t.play();
    run(&mut t, 3_000);
    let before = t.position_ms();
    t.stop();
    assert_eq!(before - t.position_ms(), 1_000);
    // Stopping again (already stopped) does not step back further.
    t.stop();
    assert_eq!(before - t.position_ms(), 1_000);
}

#[test]
fn backstep_never_goes_before_the_start() {
    let mut t = transport(5.0);
    t.play();
    run(&mut t, 200);
    t.stop();
    assert_eq!(t.position_ms(), 0);
}

#[test]
fn slow_preset_plays_longer_at_the_same_pitch_and_toggles_back() {
    let mut t = transport(4.0);
    t.toggle_slow();
    assert_eq!((t.preset(), t.speed()), (SpeedPreset::Slow, 0.6));
    t.play();
    run(&mut t, 1_000);
    let pos = t.position_ms();
    assert!((560..=640).contains(&pos), "after 1 s at 60 % the position is {pos} ms");
    t.toggle_slow();
    assert_eq!((t.preset(), t.speed()), (SpeedPreset::Normal, 1.0));
}

#[test]
fn rewind_while_held_then_release_resumes_playing() {
    let mut t = transport(10.0);
    t.seek_ms(5_000);
    t.play();
    t.rewind();
    let out = run(&mut t, 250);
    assert!(out.iter().all(|s| *s == 0.0), "rewinding is silent");
    let pos = t.position_ms();
    assert!((2_900..=3_100).contains(&pos), "8x rewind for 250 ms from 5 s lands at {pos}");
    t.release();
    assert_eq!(t.state(), State::Playing);
    t.stop();
    t.fast_forward();
    run(&mut t, 100_000);
    assert_eq!(t.position_ms(), t.duration_ms(), "held at the end");
    t.release();
    assert_eq!(t.state(), State::Stopped);
}

#[test]
fn playback_stops_at_the_end_and_play_restarts() {
    let mut t = transport(0.5);
    t.play();
    run(&mut t, 1_000);
    assert_eq!(t.state(), State::Stopped);
    t.play();
    assert_eq!(t.position_ms(), 0);
}

#[test]
fn speed_changes_mid_play_are_seamless() {
    let mut t = Transport::new(Arc::new(synth::tone(8_000, 1, 230.0, 4.0)), Settings::default()).unwrap();
    t.play();
    let mut out = run(&mut t, 500);
    t.set_speed(0.7).unwrap();
    out.extend(run(&mut t, 500));
    t.set_speed(1.0).unwrap();
    out.extend(run(&mut t, 500));
    // A pure tone obeys x[n+1] = 2 cos(w) x[n] - x[n-1]; a skipped or repeated stretch of
    // audio breaks the recurrence even where the sample values happen to line up.
    let k = 2.0 * (std::f32::consts::TAU * 230.0 / 8_000.0).cos();
    let (at, worst) =
        out.windows(3).map(|w| (w[2] - k * w[1] + w[0]).abs()).enumerate().fold((0, 0.0f32), |a, (i, d)| if d > a.1 { (i, d) } else { a });
    assert!(worst < 0.01, "discontinuity of {worst} at output frame {at}");
}

#[test]
fn channel_modes_and_volume_apply_to_output() {
    let mut t = transport(3.0);
    let s = Settings { channel_mode: ChannelMode::Right, volume_db: -6.0, ..Settings::default() };
    t.set_settings(s).unwrap();
    t.play();
    let out = run(&mut t, 2_000);
    let src = &t.audio().samples;
    for (o, i) in out.as_chunks::<2>().0.iter().zip(src.as_chunks::<2>().0) {
        assert_eq!(o[0], o[1]);
        assert!((o[0] - i[1] * 0.501).abs() < 1e-3);
    }
}

#[test]
fn rejects_bad_speeds_and_settings() {
    let mut t = transport(1.0);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 4.01] {
        assert!(t.set_speed(bad).unwrap_err().to_string().contains("out of range"));
    }
    let s = Settings { auto_backstep_ms: 60_001, ..Settings::default() };
    assert!(t.set_settings(s).is_err());
    let s = Settings { slow_speed: f64::NAN, ..Settings::default() };
    assert!(Transport::new(t.audio().clone(), s).is_err());
}

#[test]
fn render_range_produces_the_slowed_recording() {
    let mut t = transport(4.0);
    t.set_speed(0.5).unwrap();
    let out = t.render_range(1_000, 2_000);
    assert_eq!(out.frames(), 16_000);
    assert_eq!(t.state(), State::Stopped);
    assert!(out.samples.iter().any(|s| *s != 0.0));
}

#[test]
fn empty_recording_is_harmless() {
    let mut t = Transport::new(Arc::new(AudioBuffer::new(8_000, 1, vec![]).unwrap()), Settings::default()).unwrap();
    t.play();
    t.set_speed(0.5).unwrap();
    let out = run(&mut t, 100);
    assert!(out.iter().all(|s| *s == 0.0));
    assert_eq!(t.state(), State::Stopped);
    t.skip_ms(i64::MIN);
    t.skip_ms(i64::MAX);
    t.seek_ms(u64::MAX);
    assert_eq!(t.position_ms(), 0);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
    /// Any sequence of controls keeps the transport inside the recording and the output finite.
    #[test]
    fn any_control_sequence_is_safe(ops in proptest::collection::vec(0u8..12, 1..60), speed in 0.1f64..4.0) {
        let mut t = transport(2.0);
        let mut block = vec![0.0; 160 * 2];
        for op in ops {
            match op {
                0 => t.play(),
                1 => t.stop(),
                2 => t.rewind(),
                3 => t.fast_forward(),
                4 => t.release(),
                5 => t.toggle_slow(),
                6 => t.toggle_fast(),
                7 => { let _ = t.set_speed(speed); }
                8 => t.skip_ms(-3_000),
                9 => t.skip_ms(700),
                10 => t.seek_ms(1_999),
                _ => t.toggle_play(),
            }
            t.render(&mut block);
            proptest::prop_assert!(block.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            proptest::prop_assert!(t.position_ms() <= t.duration_ms());
        }
    }
}
