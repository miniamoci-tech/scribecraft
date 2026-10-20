# Gaps to Express Scribe Pro, ranked

> **Status:** current · **Last updated:** 2026-10-20

| Rank | Gap | Why it matters | Parity ids | Milestone |
|---|---|---|---|---|
| 1 | DSS/DS2 dictation | Most professional dictation recorders produce it | fmt.dss | M1 |
| 2 | Foot pedals and global hotkeys: proof on real hardware | Implemented (USB HID, serial COM, system-wide keys) but checked only by tests and by hand on Windows; needs a physical pedal, macOS and Linux checks, game-port pedals | ctl.foot-pedal, ctl.hotkeys-global | M3 |
| 3 | Type time stamps into the word processor | Completes "transcribe without leaving the editor" | int.word-processor | M3 |
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
- 2026-10-20 — foot pedals (USB HID, serial) and system-wide hotkeys implemented; DSS moves to rank 1.
- 2026-10-19 — first ranking after M0.
