# scribecraft-platform (L5)

The only crate below the UI that talks to the operating system.

- `AudioOutput::open(transport)`: default output device via cpal; rate/channel conversion;
  `try_lock` in the callback so the UI can never stall audio.
- `data_dir()`: per-user folder for the job list and settings (`SCRIBECRAFT_DATA_DIR` overrides).
- Next (M3): USB HID foot pedals and system-wide hotkeys, both opt-in.
