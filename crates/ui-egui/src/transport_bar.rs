//! The transport bar: time, seek bar, start / skip, held rewind and fast-forward, play/stop
//! and speed.
//!
//! A scan belongs to whoever started it. A pedal or a system-wide key can put the transport
//! into [`State::Rewinding`] or [`State::FastForwarding`] while no button is down, and that
//! scan must survive the interface's next frame: the bar may end only a scan it started
//! itself. A short tap on a scan button skips by the configured amount; holding it scans
//! while held (see [`TAP_SECONDS`]).

use std::sync::{Arc, Mutex};

use eframe::egui;
use scribecraft_engine::transport::{MAX_SPEED, MIN_SPEED, SpeedPreset, format_timestamp};
use scribecraft_engine::{State, Transport, lock_transport};

/// A press shorter than this on a scan button is a skip; a longer press scans while held.
pub const TAP_SECONDS: f64 = 0.15;

/// Which way one scan button scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Rewind,
    FastForward,
}

impl Direction {
    fn state(self) -> State {
        match self {
            Self::Rewind => State::Rewinding,
            Self::FastForward => State::FastForwarding,
        }
    }

    fn start(self, t: &mut Transport) {
        match self {
            Self::Rewind => t.rewind(),
            Self::FastForward => t.fast_forward(),
        }
    }

    /// The skip a tap performs: backwards for rewind.
    fn skip(self, skip_ms: i64) -> i64 {
        match self {
            // `skip_ms` is non-negative (converted from u64), so negating cannot overflow.
            Self::Rewind => -skip_ms,
            Self::FastForward => skip_ms,
        }
    }
}

/// What a scan button asks the transport to do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanAction {
    None,
    /// Start scanning (the button took the scan).
    Start,
    /// End the scan the button started.
    Stop,
    /// A tap: end the button's short scan and skip from where the press began.
    StopThenSkip,
    /// A tap that never had a held frame: skip from the current position.
    Skip,
}

/// One held or tapped scan button.
#[derive(Debug)]
struct ScanButton {
    direction: Direction,
    down: bool,
    /// This button started the scan that is running, so it may stop it.
    owns_scan: bool,
    pressed_at: Option<f64>,
    press_pos: u64,
}

impl ScanButton {
    fn new(direction: Direction) -> Self {
        Self { direction, down: false, owns_scan: false, pressed_at: None, press_pos: 0 }
    }

    /// Whether the transport is scanning for any control, not just this button.
    fn scanning(state: State) -> bool {
        matches!(state, State::Rewinding | State::FastForwarding)
    }

    /// Turns this frame's button state into an action. `down` and `clicked` come from the
    /// button's [`egui::Response`]; `now` is [`egui::InputState::time`] and `state`/`pos` are
    /// the transport's state before any action here.
    fn update(&mut self, down: bool, clicked: bool, now: f64, state: State, pos: u64) -> ScanAction {
        let was_down = std::mem::replace(&mut self.down, down);
        if down {
            if was_down {
                return ScanAction::None;
            }
            self.pressed_at = Some(now);
            self.press_pos = pos;
            // A pedal or a key may already be scanning this way. Do not take it over, or
            // releasing the button would stop a scan the pedal still holds.
            self.owns_scan = state != self.direction.state();
            return if self.owns_scan { ScanAction::Start } else { ScanAction::None };
        }
        if was_down {
            let held = self.pressed_at.take().map_or(f64::INFINITY, |start| (now - start).max(0.0));
            let owns_scan = std::mem::take(&mut self.owns_scan);
            let tap = clicked && held < TAP_SECONDS;
            if owns_scan {
                return if tap { ScanAction::StopThenSkip } else { ScanAction::Stop };
            }
            return if tap && !Self::scanning(state) { ScanAction::Skip } else { ScanAction::None };
        }
        // Press and release arrived in one frame: egui reports the click but never a held
        // frame, and `is_pointer_button_down_on` stays false.
        if clicked && !Self::scanning(state) { ScanAction::Skip } else { ScanAction::None }
    }

    /// Applies an action to the shared transport.
    fn apply(&self, action: ScanAction, transport: &Arc<Mutex<Transport>>, skip_ms: i64) {
        match action {
            ScanAction::None => {}
            ScanAction::Start => self.direction.start(&mut lock_transport(transport)),
            ScanAction::Stop => lock_transport(transport).release(),
            ScanAction::StopThenSkip => {
                let mut t = lock_transport(transport);
                t.release();
                let delta = self.direction.skip(skip_ms);
                let target =
                    if delta < 0 { self.press_pos.saturating_sub(delta.unsigned_abs()) } else { self.press_pos.saturating_add(delta.unsigned_abs()) };
                t.seek_ms(target);
            }
            ScanAction::Skip => lock_transport(transport).skip_ms(self.direction.skip(skip_ms)),
        }
    }
}

/// The transport bar. Keeps only the seek preview and the scan buttons' own state.
#[derive(Debug)]
pub(crate) struct TransportBar {
    scrub: Option<f64>,
    rewind: ScanButton,
    forward: ScanButton,
}

impl Default for TransportBar {
    fn default() -> Self {
        Self { scrub: None, rewind: ScanButton::new(Direction::Rewind), forward: ScanButton::new(Direction::FastForward) }
    }
}

impl TransportBar {
    /// Forgets the seek preview and any scan ownership, for when another recording opens.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, transport: &Arc<Mutex<Transport>>, skip_ms: u64) {
        let (state, pos, dur, speed, preset, rewind_rate, fast_rate) = {
            let t = lock_transport(transport);
            (t.state(), t.position_ms(), t.duration_ms(), t.speed(), t.preset(), t.settings().rewind_rate, t.settings().fast_forward_rate)
        };
        ui.horizontal(|ui| {
            ui.monospace(format!("{} / {}", format_timestamp(pos, true), format_timestamp(dur, true)));
            ui.label(state_name(state));
        });

        // Seek bar: dragging previews the target; releasing seeks.
        let mut value = self.scrub.unwrap_or(pos as f64);
        let normal_width = ui.spacing().slider_width;
        ui.spacing_mut().slider_width = (ui.available_width() - 20.0).max(100.0);
        let slider = ui.add(egui::Slider::new(&mut value, 0.0..=(dur.max(1) as f64)).show_value(false).trailing_fill(true));
        ui.spacing_mut().slider_width = normal_width;
        if slider.dragged() {
            self.scrub = Some(value);
        }
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            lock_transport(transport).seek_ms(value as u64);
            self.scrub = None;
        }

        let now = ui.input(|i| i.time);
        let skip = i64::try_from(skip_ms).unwrap_or(i64::MAX);
        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("Go to the start").clicked() {
                lock_transport(transport).seek_ms(0);
            }
            if ui.button(format!("« {}", skip_label(skip_ms))).on_hover_text("Skip back").clicked() {
                lock_transport(transport).skip_ms(-skip);
            }

            let rew = ui.add(egui::Button::new("⏪")).on_hover_text(format!("Rewind at {rewind_rate:.0}× — hold to scan, tap to skip back (F7)"));
            let action = self.rewind.update(rew.is_pointer_button_down_on(), rew.clicked(), now, state, pos);
            self.rewind.apply(action, transport, skip);

            let (label, hint) = if state == State::Playing { ("⏸ Stop", "Stop (F9)") } else { ("▶ Play", "Play (F9)") };
            if ui.add(egui::Button::new(label).min_size(egui::vec2(90.0, 0.0))).on_hover_text(hint).clicked() {
                lock_transport(transport).toggle_play();
            }

            let ff =
                ui.add(egui::Button::new("⏩")).on_hover_text(format!("Fast-forward at {fast_rate:.0}× — hold to scan, tap to skip forward (F8)"));
            let action = self.forward.update(ff.is_pointer_button_down_on(), ff.clicked(), now, state, pos);
            self.forward.apply(action, transport, skip);

            if ui.button(format!("{} »", skip_label(skip_ms))).on_hover_text("Skip forward").clicked() {
                lock_transport(transport).skip_ms(skip);
            }

            ui.separator();
            if ui.selectable_label(preset == SpeedPreset::Slow, "Slow").on_hover_text("F5").clicked() {
                lock_transport(transport).toggle_slow();
            }
            if ui.selectable_label(preset == SpeedPreset::Fast, "Fast").on_hover_text("F6").clicked() {
                lock_transport(transport).toggle_fast();
            }
            let mut pct = speed * 100.0;
            let r = ui.add(egui::Slider::new(&mut pct, MIN_SPEED * 100.0..=MAX_SPEED * 100.0).suffix(" %").text("speed"));
            if r.changed() {
                let _ = lock_transport(transport).set_speed((pct / 100.0).clamp(MIN_SPEED, MAX_SPEED));
            }
            if ui.button("100 %").clicked() {
                let _ = lock_transport(transport).set_speed(1.0);
            }
        });
    }
}

fn state_name(state: State) -> &'static str {
    match state {
        State::Stopped => "stopped",
        State::Playing => "playing",
        State::Rewinding => "rewinding",
        State::FastForwarding => "fast-forwarding",
    }
}

/// Short text for the skip buttons, e.g. `5 s` or `750 ms`.
fn skip_label(ms: u64) -> String {
    if ms.is_multiple_of(1_000) { format!("{} s", ms / 1_000) } else { format!("{ms} ms") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribecraft_engine::media::synth;
    use scribecraft_engine::transport::Settings;

    fn transport() -> Transport {
        Transport::new(Arc::new(synth::dictation(8_000, 1, 10.0)), Settings::default()).unwrap()
    }

    fn advance(t: &mut Transport, ms: u64) {
        let mut buf = vec![0.0; (ms * 8) as usize];
        t.render(&mut buf);
    }

    /// Draws one complete interface frame with an empty input state.
    fn frame(bar: &mut TransportBar, t: &Arc<Mutex<Transport>>) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| bar.show(ui, t, 5_000));
        // No renderer here; drop the font-atlas deltas as a renderer would consume them.
        output.textures_delta.clear();
    }

    #[test]
    fn a_fast_forward_started_by_a_pedal_survives_a_ui_frame() {
        // Regression: the bar used to release any scan that was not backed by the mouse
        // button or F8, which cancelled a pedal-started fast-forward one frame after it began.
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().fast_forward();
        let mut bar = TransportBar::default();
        frame(&mut bar, &t);
        assert_eq!(t.lock().unwrap().state(), State::FastForwarding, "the bar must not end a scan it did not start");
        frame(&mut bar, &t);
        assert_eq!(t.lock().unwrap().state(), State::FastForwarding);
    }

    #[test]
    fn a_rewind_started_by_a_pedal_survives_a_ui_frame() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().rewind();
        let mut bar = TransportBar::default();
        frame(&mut bar, &t);
        assert_eq!(t.lock().unwrap().state(), State::Rewinding);
    }

    #[test]
    fn holding_the_fast_forward_button_scans_and_releasing_stops() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().play();
        let mut b = ScanButton::new(Direction::FastForward);
        let start = t.lock().unwrap().position_ms();
        assert_eq!(b.update(true, false, 1.0, State::Playing, start), ScanAction::Start);
        b.apply(ScanAction::Start, &t, 5_000);
        assert_eq!(t.lock().unwrap().state(), State::FastForwarding);

        advance(&mut t.lock().unwrap(), 500);
        let pos = t.lock().unwrap().position_ms();
        assert!(pos >= start + 3_500, "8× for half a second: {pos} ms from {start}");

        assert_eq!(b.update(false, false, 1.7, State::FastForwarding, pos), ScanAction::Stop);
        b.apply(ScanAction::Stop, &t, 5_000);
        assert_eq!(t.lock().unwrap().state(), State::Playing, "a release resumes playback");
    }

    #[test]
    fn a_held_rewind_does_not_jump_when_released() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().play();
        t.lock().unwrap().seek_ms(5_000);
        let mut b = ScanButton::new(Direction::Rewind);
        assert_eq!(b.update(true, false, 2.0, State::Playing, 5_000), ScanAction::Start);
        b.apply(ScanAction::Start, &t, 5_000);
        advance(&mut t.lock().unwrap(), 200);
        let pos = t.lock().unwrap().position_ms();
        assert_eq!(b.update(false, false, 2.4, State::Rewinding, pos), ScanAction::Stop);
        b.apply(ScanAction::Stop, &t, 5_000);
        let after = t.lock().unwrap().position_ms();
        assert_eq!(t.lock().unwrap().state(), State::Playing);
        assert_eq!(after, pos, "ending a held scan leaves the position where it stopped");
    }

    #[test]
    fn a_tap_on_a_scan_button_skips_by_the_configured_amount() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().seek_ms(2_000);
        let mut b = ScanButton::new(Direction::FastForward);
        assert_eq!(b.update(true, false, 3.0, State::Stopped, 2_000), ScanAction::Start);
        b.apply(ScanAction::Start, &t, 5_000);
        assert_eq!(b.update(false, true, 3.05, State::FastForwarding, 2_000), ScanAction::StopThenSkip);
        b.apply(ScanAction::StopThenSkip, &t, 5_000);
        let t = t.lock().unwrap();
        assert_eq!(t.state(), State::Stopped);
        assert_eq!(t.position_ms(), 7_000, "a tap skips from where the press began");
    }

    #[test]
    fn a_tap_that_fits_in_one_frame_still_skips() {
        // On a slow frame the press and the release arrive together: the button was never
        // seen down, but egui still reports a click.
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().seek_ms(1_000);
        let mut b = ScanButton::new(Direction::FastForward);
        assert_eq!(b.update(false, true, 4.0, State::Stopped, 1_000), ScanAction::Skip);
        b.apply(ScanAction::Skip, &t, 5_000);
        assert_eq!(t.lock().unwrap().position_ms(), 6_000);
    }

    #[test]
    fn a_tap_on_rewind_skips_back_and_stops_at_the_start() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().seek_ms(1_000);
        let b = ScanButton::new(Direction::Rewind);
        b.apply(ScanAction::Skip, &t, 5_000);
        assert_eq!(t.lock().unwrap().position_ms(), 0, "a skip back never goes before the start");
    }

    #[test]
    fn a_button_pressed_during_a_pedal_scan_does_not_take_it_over() {
        let t = Arc::new(Mutex::new(transport()));
        t.lock().unwrap().fast_forward();
        let mut b = ScanButton::new(Direction::FastForward);
        assert_eq!(b.update(true, false, 5.0, State::FastForwarding, 0), ScanAction::None, "the pedal owns the scan");
        assert_eq!(b.update(false, true, 5.05, State::FastForwarding, 0), ScanAction::None);
        assert_eq!(t.lock().unwrap().state(), State::FastForwarding);
    }

    #[test]
    fn skip_labels_are_short_and_honest() {
        assert_eq!(skip_label(5_000), "5 s");
        assert_eq!(skip_label(750), "750 ms");
        assert_eq!(skip_label(0), "0 s");
    }
}
