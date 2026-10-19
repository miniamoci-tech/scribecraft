# Installing and starting ScribeCraft

> **Status:** M0/M1 preview · **Last updated:** 2026-10-19

ScribeCraft is an open-source transcription player: it plays recordings slower or faster
without changing the voice's pitch, steps back a little every time you stop, keeps a list of
the dictations you have to transcribe, and lets you control playback from the keyboard.

## Windows (10 or 11)

**With the installer** — `ScribeCraft-<version>-windows-x64-setup.exe`
1. Double-click the file. Windows SmartScreen may say "Windows protected your PC" because the
   installer is not signed yet: click **More info → Run anyway**.
2. Follow the steps (no administrator rights needed). Start ScribeCraft from the Start menu.

**Without installing** — `ScribeCraft-<version>-windows-x64.zip`
1. Right-click the zip → **Extract All…**
2. Open the extracted folder and double-click **`scribecraft.exe`**.

## macOS (11 Big Sur or later, Apple silicon and Intel)

`ScribeCraft-<version>-macos.dmg`
1. Double-click the `.dmg`, then drag **ScribeCraft** onto **Applications**.
2. The first time, open **Applications**, **right-click ScribeCraft → Open**, then **Open**
   again. (The app is not notarized by Apple yet, so a normal double-click is refused the
   first time.) If macOS still refuses, run once in Terminal:
   `xattr -dr com.apple.quarantine /Applications/ScribeCraft.app`

## Linux

`ScribeCraft-<version>-linux-x86_64.tar.gz`: extract it and run `./scribecraft`.

## Building it yourself (any platform)

1. Install Rust from <https://rustup.rs> (on Windows also the "Desktop development with C++"
   workload of Visual Studio Build Tools; on Linux `libasound2-dev`, `libgtk-3-dev`,
   `libxkbcommon-dev`).
2. In the source folder run: `cargo run --release -p scribecraft`
3. To make the packages: `cargo xtask dist` (on Windows, install Inno Setup 6 first to also
   get the setup `.exe`; on macOS it produces the `.dmg`).

> Windows tip: if the build fails with `LNK1104` the folder path is too long. Build with a
> short target folder, e.g. `set CARGO_TARGET_DIR=C:\sct` (cmd) or
> `$env:CARGO_TARGET_DIR="C:\sct"` (PowerShell).

## Using it

- **Add recordings…** or drop audio/video files on the window; **Add folder…** imports a
  whole folder. Click a dictation to open it; ScribeCraft remembers where you stopped.
- Keys (while the ScribeCraft window is focused): **F9** play/stop · **F7** / **F8** hold to
  rewind / fast-forward · **F5** slow · **F6** fast · **F10** copy the time stamp
  (`[00:01:23.4]`) to paste into your document · **Ctrl+B** bookmark · **Ctrl+S** save.
- Speed 10 %–400 % without pitch change; auto-backstep, slow/fast presets, volume and
  left/right/mono channel selection are in the second row.

Not yet available (see `docs/gaps.md`): foot pedals, hotkeys that work while you type in
another program, DSS/DS2/WMA dictation formats, e-mail/FTP intake.

## Where your data is

The job list and settings live in `%APPDATA%\ScribeCraft` (Windows),
`~/Library/Application Support/ScribeCraft` (macOS) or `~/.local/share/scribecraft` (Linux).
ScribeCraft never changes or uploads your recordings.
