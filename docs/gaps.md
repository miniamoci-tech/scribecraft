# Gaps to Express Scribe Pro, ranked

> **Status:** current · **Last updated:** 2026-10-25

| Rank | Gap | Why it matters | Parity ids | Milestone |
|---|---|---|---|---|
| 1 | DSS/DS2 dictation | Most professional dictation recorders produce it | fmt.dss | M1 |
| 2 | Foot pedals and global hotkeys: proof on real hardware | Implemented (USB HID, serial COM, system-wide keys) but checked only by tests and by hand on Windows; needs a physical pedal, macOS and Linux checks, game-port pedals | ctl.foot-pedal, ctl.hotkeys-global | M3 |
| 3 | Type time stamps into the word processor | Completes "transcribe without leaving the editor" | int.word-processor | M3 |
| 4 | Tested compressed formats | MP3/M4A decode is compiled in but unpinned by tests | fmt.compressed, fmt.video-audio | M1 |
| 5 | WMA, DCT and other dictation formats | Common in older workflows | fmt.wma, fmt.dct, fmt.other-dictation | M1 |
| 6 | Streaming decode | Multi-hour recordings exceed the in-memory cap | — | M1 |
| 7 | Automatic intake and dispatch | Timed folder watch, FTP/e-mail (opt-in) | job.folder-watch, job.remote-intake, job.dispatch | M4 |
| 8 | Waveform, end-to-end UI tests, more UI languages | 1.0 polish; English and Italian ship now, engine/device messages stay English | play.waveform, ctl.hotkeys-window | M2 |
| 9 | Signed/notarized installers | SmartScreen and Gatekeeper warnings | plat.windows, plat.macos | M7 |
| 10 | Video window | Video transcription | play.video | M6 |

Engineering debts: linear output resampler; WSOLA graininess below 40 %; dependency upgrades
(cpal 0.18, Symphonia 0.6).

## Revision history
- 2026-10-25 — the pedal setup is now a guided three-step wizard (device, learn, functions) sharing the advanced tab's configuration and pedal table; step order and its guards are tested, but it still needs a physical pedal (rank 2).
- 2026-10-25 — the interface speaks English and Italian: one string table in `i18n.rs` (a missing translation does not compile), a toolbar selector, the choice saved atomically in `ui.json`, placeholders checked for both languages; engine and device messages stay as they come. The longer Italian labels showed the bookmark row overflowing the details panel and widening it frame after frame; the label field and the buttons are now on separate lines, with a headless regression test in both languages.
- 2026-10-25 — queue rows start playback when picked (the open row is not restarted); multi-file add says how many it added, opens the first and says several can be chosen.
- 2026-10-25 — restyle phase 3: dictation fields and bookmarks in a resizable right-hand details panel with a status pill; the notes editor moved to the centre, where there is room for it; write-back extracted into `apply_edits` and tested against bookmarks and pasted text.
- 2026-10-25 — restyle phase 2: filter chips with counts above the queue (All / To do / In progress / Done / Sent) and hand-painted job rows with priority stripes, title elision, meta line and right-aligned duration; contrast covered by theme tests, filtering by a headless click test.
- 2026-10-25 — restyle phase 1: one dark palette in `theme.rs` (WCAG-AA contrast checked by tests), a transport dock at the bottom with clock, state pill, remaining time and bookmark marks on the seek bar, and a scrolling pane for the open dictation on short windows.
- 2026-10-25 — the transport bar no longer ends a scan it did not start (pedal and system-wide fast-forward survived one frame); a scan button now scans while held and skips on a tap; pedal learning waits until every pedal is up and ignores a bit another pedal uses. First UI tests (transport bar, headless egui).
- 2026-10-20 — foot pedals (USB HID, serial) and system-wide hotkeys implemented; DSS moves to rank 1.
- 2026-10-19 — first ranking after M0.
