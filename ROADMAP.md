# ScribeCraft roadmap

**Stage: pre-alpha** · next: alpha (global hotkeys + pedals) · parity 17/45 (weighted ≈ 46 %, ready for real work ≈ 25 %)

> **Last reviewed:** 2026-10-25 · **Last updated:** 2026-10-25 · **Change:** fixed pedal / hotkey fast-forward being cancelled by the transport bar · **Target:** Express Scribe Pro (public feature description)

- Milestones: [`docs/roadmap.md`](docs/roadmap.md) · Gaps: [`docs/gaps.md`](docs/gaps.md) · Measurement: [`docs/target-app-parity.md`](docs/target-app-parity.md)

## Progress log
- 2026-10-25 — Fix: the transport bar ended any scan not backed by its own button or F8, so a fast-forward started by a foot pedal or a system-wide key stopped after one frame (play was unaffected). The bar now ends only a scan it started; holding ⏪/⏩ scans and a tap skips by the configured amount. Pedal learning waits until every pedal is released and refuses a bit another pedal already uses, so releasing a pedal held while Learn was clicked no longer stores a duplicate. First headless UI tests (transport bar).
- 2026-10-20 — M3: foot pedals on USB (HID, learned with one press) and serial COM ports (NCH public wiring defaults), system-wide hotkeys (F7/F8 hold, F9, F4, F6; remappable) that work while the word processor is in front; `scribecraft-cli pedal list|monitor`; pedal automation tools. Checked by hand on Windows 10; no physical pedal yet.
- 2026-10-19 — 0.1.1: restored the `cargo xtask` alias (#1); CI green on Windows, macOS and Linux; first packaged release (setup .exe, .dmg, tar.gz) from the release workflow.
- 2026-10-19 — M0: workspace, WSOLA constant-pitch speed, Symphonia decode with limits, transport (backstep, held rewind/FF, presets), job list with resume and bookmarks, 21 automation tools with `--root` confinement, CLI with real audio output, xtask gates (layers, assets, parity, wasm, ci, dist). M2.0: first desktop window, checked by hand on Windows 10.
