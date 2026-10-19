# ScribeCraft

A clean-room, open-source, Rust-native **transcription player**, targeting parity with
Express Scribe Pro: constant-pitch variable-speed playback, auto-backstep, a dictation job
list, keyboard control — and next, foot pedals and system-wide hotkeys. Windows, macOS,
Linux and FreeBSD, with the engine also building for WebAssembly.

A sibling of PdfCraft and PhotoCraft; it follows the same rules (`AGENTS.md`, `CLAUDE.md`).
ScribeCraft is not affiliated with NCH Software.

**Install / start:** [`docs/install.md`](docs/install.md) · **Roadmap:** [`ROADMAP.md`](ROADMAP.md) ·
**What's missing:** [`docs/gaps.md`](docs/gaps.md)

## Quick start (from source)

```bash
cargo run --release -p scribecraft                 # desktop app
cargo run -p scribecraft-cli -- tone --out demo.wav     # a synthetic dictation to try it on
cargo run -p scribecraft-cli -- play demo.wav --speed 0.7
cargo run -p scribecraft-cli -- render demo.wav --speed 0.5 --out slow.wav
cargo run -p scribecraft-cli -- tools              # the headless automation tools
cargo xtask ci                                     # every quality gate
cargo xtask dist                                   # installer / zip / .dmg for this OS
```

## Layout

| Layer | Crate | What it does |
|---|---|---|
| L0 | `crates/dsp` | WSOLA constant-pitch time stretch, channel routing, gain, rate conversion (no deps) |
| L1 | `crates/media` | decoding (Symphonia), WAV export, synthetic test media |
| L2 | `crates/transport` | play/stop + auto-backstep, rewind/FF while held, speed presets, time stamps |
| L3 | `crates/engine` | dictation job list, folder intake, bookmarks, session |
| L4 | `crates/automation` | every feature as a JSON tool, `--root` confined |
| L5 | `crates/platform` | audio output (cpal); pedals and global hotkeys next |
| L6 | `crates/ui-egui` | desktop interface |
| L7 | `apps/scribecraft`, `apps/scribecraft-cli` | the programs |

Licence: MIT OR Apache-2.0. Third-party notices: [`NOTICE`](NOTICE), [`ATTRIBUTION.toml`](ATTRIBUTION.toml).
