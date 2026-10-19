# Gaps to Express Scribe Pro, ranked

> **Status:** current · **Last updated:** 2026-10-19

| Rank | Gap | Why it matters | Parity ids | Milestone |
|---|---|---|---|---|
| 1 | System-wide hotkeys | A transcriptionist types in another program; in-window keys are not enough | ctl.hotkeys-global, int.word-processor | M3 |
| 2 | USB foot pedals | The standard way to control playback hands-free | ctl.foot-pedal | M3 |
| 3 | DSS/DS2 dictation | Most professional dictation recorders produce it | fmt.dss | M1 |
| 4 | Tested compressed formats | MP3/M4A decode is compiled in but unpinned by tests | fmt.compressed, fmt.video-audio | M1 |
| 5 | WMA, DCT and other dictation formats | Common in older workflows | fmt.wma, fmt.dct, fmt.other-dictation | M1 |
| 6 | Streaming decode | Multi-hour recordings exceed the in-memory cap | — | M1 |
| 7 | Automatic intake and dispatch | Timed folder watch, FTP/e-mail (opt-in) | job.folder-watch, job.remote-intake, job.dispatch | M4 |
| 8 | Waveform, remappable keys, UI tests, i18n | 1.0 polish | play.waveform, ctl.hotkeys-window | M2 |
| 9 | Signed/notarized installers | SmartScreen and Gatekeeper warnings | plat.windows, plat.macos | M7 |
| 10 | Video window | Video transcription | play.video | M6 |

Engineering debts: linear output resampler; WSOLA graininess below 40 %; dependency upgrades
(cpal 0.18, Symphonia 0.6).

## Revision history
- 2026-10-19 — first ranking after M0.
