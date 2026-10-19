# scribecraft-dsp (L0)

Pure, dependency-free signal processing used by the transport:

- `Wsola` — constant-pitch time stretching by waveform-similarity overlap-add
  (Verhelst & Roelands, ICASSP 1993). Works on interleaved buffers of any channel count;
  the alignment search runs on a mono sum so every channel gets the same offset and the
  stereo image never smears.
- `ChannelMode` — Express-Scribe-style multi-channel control: both channels, left only,
  right only, or a mono mix (useful for two-microphone interviews and phone recordings).
- `apply_gain` / `db_to_gain` — volume with hard clipping protection.

Rules: no allocation after construction (`Wsola::synthesize_hop` is called from the
audio callback), no indexing with input-derived positions, every public function total.
