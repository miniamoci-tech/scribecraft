# scribecraft-platform (L5)

The only crate below the UI that talks to the operating system.

- `AudioOutput::open(transport)`: default output device via cpal; rate/channel conversion;
  `try_lock` in the callback so the UI can never stall audio.
- `data_dir()`: per-user folder for the job list and settings (`SCRIBECRAFT_DATA_DIR` overrides).
- `pedal`: `list_devices()` (HID devices except keyboards/mice, serial ports) and `PedalReader`
  (own thread, reconnects after unplugging; serial pedals read DCD/DSR/CTS/RI with DTR raised).
- `hotkeys::GlobalHotkeys`: system-wide keys on Windows, macOS and X11. The crate's event
  handler can be installed only once per process, so events go through a replaceable route.
