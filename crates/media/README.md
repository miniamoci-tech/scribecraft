# scribecraft-media (L1)

Turns recordings into interleaved `f32` PCM (`AudioBuffer`) and back into WAV.

- Decoding uses [Symphonia](https://github.com/pdeljanov/Symphonia) (MPL-2.0, pure Rust,
  used unmodified as a dependency): WAV/PCM/ADPCM, MP3, FLAC, Ogg Vorbis, AAC/M4A, ALAC,
  AIFF/AIFC, CAF, and the audio track of MP4/MOV/M4V and MKV/WebM video.
- Proprietary dictation formats (DSS/DS2, DCT, DVF, MSV, WMA, AMR, VOX…) are **not**
  decoded yet; `docs/gaps.md` ranks them. They are implemented only from public
  specifications (`AGENTS.md` §2).
- Every decode is bounded by `Limits` (file size, total samples) and runs behind
  `catch_unwind`, so a malformed file is an error, never a crash. Corrupt packets are
  skipped and listed in `AudioBuffer::repairs`.
- `write_wav` writes atomically (temporary file, then rename).
- `synth` generates the synthetic "dictations" used by tests and demos. Real recordings
  are never committed (`AGENTS.md` §1.2).

Decoding is whole-file for now (bootstrap debt, streaming decode is M1).
