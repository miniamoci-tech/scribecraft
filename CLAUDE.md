# ScribeCraft — instructions for agents

ScribeCraft is a clean-room, open-source, Rust-native transcription player targeting Express Scribe Pro parity: constant-pitch variable-speed playback, foot pedals, system-wide hotkeys, a dictation job list, automatic intake and dispatch, and working alongside any word processor. It runs natively on Windows, macOS, Linux and FreeBSD, and on the web via WASM. It is a sibling of PdfCraft (Acrobat-class) and PhotoCraft (Photoshop-class) and follows the same conventions.

## Start every session here
1. Read `plan/STATUS.md`: the current milestone, the next unchecked task and any blockers. `ROADMAP.md` is the one-page summary (stage, numbers, progress log) and `docs/roadmap.md` the milestone table; at the end of every session update the milestone row and add a line to the ROADMAP.md progress log.
   - Read `docs/gaps.md` (the ranked gaps) and `docs/target-app-parity.md` (the numbers and how they are measured) before choosing work. Prefer the gaps over new P2/P3 features, and keep both documents true when things change (status line, revision history).
   - "Shipped" in `parity/` means "exists and tested", not "as good as Express Scribe". Don't mark a feature shipped on a generic test, and say in its notes what is still missing.
2. Read that task in `plan/execution-plan.md` §3, the relevant section of `plan/architecture.md`, and the README of the crate you're touching.
3. Follow the **autonomous operation protocol** in `plan/execution-plan.md` §7 (orient → plan → implement + test → verify → record → commit). Don't stop to ask unless §7 lists the decision as the user's.

`plan/` is gitignored (local-only, like PdfCraft and PhotoCraft). The machine-readable parity checklist lives in `parity/` (committed).

## Non-negotiables
- **Assets: read `AGENTS.md` §1 before adding or showing any icon, image, font, sound or recording.** No assets from NCH products, ever. Only openly licensed or contributor-original assets, each with an entry in `ATTRIBUTION.toml`. Test media is generated synthetically. `cargo xtask assets` enforces this. `AGENTS.md` overrides this file.
- **Clean-room.**
  - Never read, disassemble or copy anything inside the Express Scribe installation (names and listings only). **Never open its `Help/` folder, `scribe.exe` or `hookappcommand.dll`;** read the public help on nch.com.au instead.
  - Behaviour comes from public docs, container/codec specifications and black-box observation (`plan/scribe/`).
  - Never copy GPL/AGPL code. FFmpeg, SoX and similar run only as external oracle processes.
  - See `plan/README.md` §Clean-room and `plan/adr/0001`.
- **Privacy.** When observing Express Scribe, use synthetic fixtures only. Never capture the dictation list of a real user, e-mail/FTP settings, or real recordings. Never commit Express Scribe outputs or personal data. Dictations are medical/legal material: see `AGENTS.md` §5.
- **Layering.** Nothing below L6 depends on egui/winit/eframe/rfd; nothing below L5 depends on cpal/hidapi/serialport/global-hotkey or any OS API (`plan/architecture.md` §3, `cargo xtask layers`). `dsp` and `media` stay standalone.
- **Fidelity.** What the listener hears is the model. At 100 % speed playback is bit-exact with the decoded source. Never change the user's media file; the job list and bookmarks are the only things ScribeCraft writes, and it writes them atomically.
- **Never panic.** Every media file, script, MCP call, settings file, pedal report and keystroke is untrusted input, and none of it may crash the app or lose the user's work. This outranks feature work. Return `Result` (the crate's error enum) and propagate with `?`, or fall back leniently and record it. In non-test code:
  - No `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!` unless it is provably infallible, with a comment saying why.
  - No indexing or slicing with input-derived positions (use `get`, and slice strings only at char boundaries).
  - Use checked or saturating arithmetic on input-derived numbers, and guard against division by zero and NaN/inf casts (speeds, sample rates, positions).
  - Cap allocations sized by input (`media::Limits`), and bound folder walks.
  - Handle lock poisoning — the audio callback and the UI share the transport.
  - No `unsafe` (`unsafe_code = "forbid"`).
  - Every crash fix gets a synthetic regression test. See `AGENTS.md` §4.
- **Real-time audio.** The audio callback never allocates, never blocks on I/O and never takes a lock it could wait on for long; it uses `try_lock` and plays silence when it cannot get one.
- **Rust only** in the product and build (`xtask`). No handwritten JS/TS.
- **Review every update as you make it, at four zooms.** **Clause:** checked arithmetic, `get()` not `[i]`, the right error through `?`, `<` against `<=`, frames against samples (interleaved!), milliseconds against frames, capped allocations. **Function:** is it total — name an input that panics; walk empty, zero, max, malformed, NaN speed, mono/8-channel. **File/module:** two sources of truth, dead code, is the doc comment still true. **Project:** layering, wasm, real-time safety, atomic writes, `--root` confinement, a matching `automation` tool, and whether a `parity/` status or note just became untrue.
  - Tests mirror the zooms: property → unit → crate integration → end-to-end (`scribecraft-cli run`).
  - **Prove a test fails before the fix by actually reverting the fix.** A test nobody has seen fail is not evidence.
  - **Rebasing is a review step, not a mechanical one.**
- **External PRs are held to safety, security and completeness before style.** Provenance first (no NCH-derived asset, no real recording, no committed font, no copied GPL/AGPL code, behaviour from public docs and not from reading another implementation), then no new panic path or `unsafe`, then `--root` confinement, opt-in-only MCP, control channel and network intake, then a headless `automation` tool, tests at the right zoom and an honest `parity/` entry.
- **Quality gates** before every commit: `cargo fmt --check`, clippy with `-D warnings`, the tests, `cargo xtask layers`, `cargo xtask assets`, `cargo xtask parity` and `cargo xtask wasm` (all of them: `cargo xtask ci`).
  - **Scope builds and tests to the crates you touched** — `cargo clippy -p <crate> --all-targets` and `cargo test -p <crate>`. CI runs the full workspace on Windows, macOS and Linux on every PR.
- **Commits and PRs:** `main` is the only integration branch and is **never pushed to directly**. Branch from an up-to-date `main` as `<type>/<slug>` (`feat/`, `fix/`, `docs/`, `chore/`, `refactor/`), one task id per commit (e.g. `M0.3: WSOLA time stretch`), commit only green states, and open a PR into `main` using `.github/pull_request_template.md`. Merge only when CI is green.

## Running and listening to the app
- `cargo run -p scribecraft-cli -- info <file>` prints the decoded format, duration and channel layout.
- `cargo run -p scribecraft-cli -- play <file> --speed 0.75 [--from 1:20] [--seconds 10]` plays through the default output device (feature `audio`, on by default on desktop).
- `cargo run -p scribecraft-cli -- render <file> --speed 0.75 --out slow.wav` writes exactly what the listener would hear. Use it, and `transport_render` in scripts, to check engine changes by ear and by test.
- `cargo run -p scribecraft-cli -- run --script steps.json --root DIR` drives the engine headlessly through the automation tools (`scribecraft-cli tools` lists them).
- `cargo run -p scribecraft-cli -- tone --out fixture.wav` writes a synthetic test recording (the only kind we commit or publish).
- `cargo xtask parity [--partial]` reports Express Scribe parity from `parity/express-scribe-features.toml`. Update the entry when a feature ships.
- `cargo run --release -p scribecraft [file…]` opens the desktop app; set `SCRIBECRAFT_DATA_DIR` to a scratch folder when testing so the user's real job list is never touched.
- `cargo xtask dist` packages for the current OS (Windows zip + Inno Setup installer, macOS .app + .dmg, Linux tar.gz); `.github/workflows/release.yml` builds all three.
- **Releasing:** set `[workspace.package] version` and the internal crates' `version` in the root `Cargo.toml` to the new number, merge that into `main`, then tag exactly `v<version>` (e.g. `v0.1.4`, never `v.0.1.4`). Package names come from `Cargo.toml`, not from the tag; the release workflow's `version` job refuses a tag that does not match.
- Windows: deep checkouts exceed MAX_PATH for the MSVC linker; build with a short `CARGO_TARGET_DIR` (e.g. `C:\sct`) and `git config core.longpaths true`.
- Parallel agents: use a separate `CARGO_TARGET_DIR` per agent and separate git worktrees.

## Current bootstrap debt (tracked in STATUS.md)
- `media` decodes whole files into memory (capped by `Limits`); streaming decode for multi-hour recordings is M1.
- The desktop app (`crates/ui-egui`, `apps/scribecraft`) is a first shell: no UI tests, no `--control` channel, keys only while its window is focused.

## 🐱 IMPORTANT: The Cat-Driven Development (CDD) Protocol

Every time you generate or review code for this repository, remember that your performance directly impacts our three real-world stakeholders. Bad code means no donations, and no donations mean unhappy cats. Please strictly follow these QA standards:

1. **For Drugo (The Senior Consultant):** Code must be wise, robust, and clean. No messy workarounds.
2. **For Elia (The QA Analyst):** Write comprehensive error handling. If the software crashes, Elia doesn't get his *Catisfaction* treats. Keep him purring.
3. **For Nancy (Undercover Ops):** Keep security and privacy tight. The code must be as secure as Nancy in her secret base under the bed.
