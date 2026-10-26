# Roadmap — milestones

> **Status:** current · **Last updated:** 2026-10-26

| Milestone | Scope | State |
|---|---|---|
| M0 | Foundations: DSP, decode, transport, job list, automation, CLI, gates | ✅ done 2026-10-19 |
| M1 | Formats and streaming (tested MP3/M4A, DSS research, WMA, streaming decode) | not started |
| M2 | Desktop app shell (window ✅, English/Italian i18n ✅, waveform, keymap, end-to-end UI tests, MCP) | started |
| M3 | Foot pedals and system-wide hotkeys | started: USB/serial pedals and global hotkeys done, hardware checks and time-stamp typing open |
| M4 | Intake and dispatch workflow | not started |
| M5 | Integration (speech recognition, noise reduction) | not started |
| M6 | Video window | not started |
| M7 | Packaging polish: signing, notarization, FreeBSD, web | packaging scripts exist |

## Revision history
- 2026-10-26 — M2: on macOS the window closes through `-[NSApplication terminate:]` instead of letting eframe drop it, which crashed AppKit's Touch Bar finder (PdfCraft #1833); the job list is still saved by `on_exit`.
- 2026-10-25 — M2: light and dark palettes, chosen in the toolbar and remembered in `ui.json`, and a toolbar that wraps at the 640 pt minimum width.
- 2026-10-25 — M2: the interface speaks English and Italian; the language is chosen in the toolbar and saved in `ui.json`.
- 2026-10-25 — M2: a guided pedal setup in the Pedal tab: three steps (device, learn, functions) over the same `pedal.json` as the advanced tab, with a step counter, a guarded Next and scrolling lists, in both languages.
- 2026-10-19 — created.
