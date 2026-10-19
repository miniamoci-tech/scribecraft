//! Decoding hostile and damaged input never panics and never exceeds its limits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test-only helpers

use scribecraft_media::{Limits, MediaError, WavFormat, decode_bytes, synth, write_wav};

fn wav_bytes(seconds: f64) -> Vec<u8> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("scribecraft-decode-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("d.wav");
    write_wav(&path, &synth::dictation(8_000, 2, seconds), WavFormat::Pcm16).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    bytes
}

#[test]
fn garbage_is_an_error_not_a_crash() {
    for bytes in [vec![], vec![0u8; 3], b"RIFF\xff\xff\xff\xffWAVEfmt ".to_vec(), (0..=255u8).cycle().take(10_000).collect()] {
        assert!(decode_bytes(bytes, Some("wav"), &Limits::default()).is_err());
    }
}

#[test]
fn a_truncated_recording_decodes_what_is_there() {
    let full = wav_bytes(1.0);
    let half = full[..full.len() / 2].to_vec();
    let audio = decode_bytes(half, Some("wav"), &Limits::default()).unwrap();
    assert!(audio.frames() > 3_000 && audio.frames() < 5_000, "{} frames", audio.frames());
}

#[test]
fn limits_are_enforced() {
    let bytes = wav_bytes(1.0);
    let small = Limits { max_file_bytes: 100, ..Limits::default() };
    assert!(matches!(decode_bytes(bytes.clone(), None, &small), Err(MediaError::TooLarge { .. })));
    let short = Limits { max_samples: 1_000, ..Limits::default() };
    assert!(matches!(decode_bytes(bytes, None, &short), Err(MediaError::TooLong { .. })));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
    #[test]
    fn corrupted_wav_never_panics(flips in proptest::collection::vec((0usize..16_100, proptest::num::u8::ANY), 1..40)) {
        let mut bytes = wav_bytes(1.0);
        for (at, value) in flips {
            if let Some(b) = bytes.get_mut(at) { *b = value; }
        }
        if let Ok(audio) = decode_bytes(bytes, Some("wav"), &Limits::default()) {
            proptest::prop_assert!(audio.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }
}
