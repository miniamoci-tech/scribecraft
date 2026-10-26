# scribecraft-ui-egui (L6)

The desktop window: job list, the "Pedal and keys" window (pedal device, what each pedal
does, learn; which key runs each function, assigned by pressing it), a guided pedal setup
(choose the device, press each pedal, say what it does) over the same `pedal.json`, transport
bar with seek bar, speed/slow/fast, volume,
channels, auto-backstep, dictation details, notes, bookmarks, drag-and-drop, in-window keys
(F9 play/stop, F7/F8 hold rewind/forward, F5 slow, F6 fast, F10 copy time stamp, Ctrl+B,
Ctrl+S). Uses egui's bundled fonts only (see ATTRIBUTION.toml).

**Themes.** Dark and light, chosen in the toolbar. `src/theme.rs` holds both palettes field for
field and the tests check WCAG AA contrast for every text pair in both; the light one is not an
inversion (white cards, a slightly darker window). The choice is saved with the language, and
the toolbar wraps so the minimum 640 pt window loses nothing.

**Languages.** The interface speaks English and Italian. The selector in the toolbar applies
at once and is remembered in `ui.json` next to the job list (atomic write, language code
only). Every string is in `src/i18n.rs`; adding one without translating it does not compile,
and the tests check both languages use the same placeholders. Strings that come from the
engine, the platform or a device (decoder errors, hotkey names, a pedal's own path) stay as
the system or file gives them.

**Quitting.** On macOS the window closes through `-[NSApplication terminate:]`, never by letting
eframe drop the window: AppKit's Touch Bar finder keeps a KVO observer on the window's view, and
dropping the view under the running run loop made the next display cycle abort the process
(`-[_NSTouchBarFinderObservation invalidate]`, "Cannot remove an observer …"; PdfCraft #1833).
`src/quit.rs` cancels the close from `App::logic` — which runs even while the window is
minimized — and schedules `terminate:` on the main queue with safe `objc2` calls. winit's
`applicationWillTerminate:` then runs eframe's exit handling, so `on_exit` still saves the job
list. Other platforms close the window exactly as before.

**Tests.** Headless egui tests cover the transport bar's scan ownership and taps, the filter
chips (including a real click), the painted rows, the palette contrast, the scrolling panes,
the details write-back, the details layout in both languages, the pedal wizard's step order
and guards, the language table and its persistence, both palettes, the queue/report sentences,
and the macOS close takeover. End-to-end window tests (M2.4) and the waveform are still open.
