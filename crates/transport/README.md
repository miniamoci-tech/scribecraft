# scribecraft-transport (L2)

What a pedal, hotkey or button does to playback, and the audio that results.

- `Transport`: play, stop (auto-backstep only when it was playing), toggle, rewind and
  fast-forward while held (`release` returns to the previous state), skip, seek, speed
  0.1–4.0 with slow/fast presets that toggle, volume, channel routing.
- `render(out)` is real-time safe (called from the audio callback). At speed 1.0 it copies
  the decoded audio bit-exactly; otherwise it runs WSOLA from `scribecraft-dsp`.
- `render_range` produces exactly what the listener hears, for tests and `transport_render`.
- `format_timestamp` / `parse_time` for `[hh:mm:ss.t]` stamps and user-typed times.

Defaults (`Settings::default`) are ScribeCraft's own choices.
