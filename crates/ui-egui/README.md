# scribecraft-ui-egui (L6)

The desktop window: job list, the "Pedal and keys" window (pedal device, what each pedal
does, learn; which key runs each function, assigned by pressing it), a guided pedal setup
(choose the device, press each pedal, say what it does) over the same `pedal.json`, transport
bar with seek bar, speed/slow/fast, volume,
channels, auto-backstep, dictation details, notes, bookmarks, drag-and-drop, in-window keys
(F9 play/stop, F7/F8 hold rewind/forward, F5 slow, F6 fast, F10 copy time stamp, Ctrl+B,
Ctrl+S). Uses egui's bundled fonts only (see ATTRIBUTION.toml).

**Languages.** The interface speaks English and Italian. The selector in the toolbar applies
at once and is remembered in `ui.json` next to the job list (atomic write, language code
only). Every string is in `src/i18n.rs`; adding one without translating it does not compile,
and the tests check both languages use the same placeholders. Strings that come from the
engine, the platform or a device (decoder errors, hotkey names, a pedal's own path) stay as
the system or file gives them.

**Tests.** Headless egui tests cover the transport bar's scan ownership and taps, the filter
chips (including a real click), the painted rows, the palette contrast, the scrolling panes,
the details write-back, the details layout in both languages, the pedal wizard's step order
and guards, the language table and its persistence, and the queue/report sentences. End-to-end window tests (M2.4) and the waveform are still open.
