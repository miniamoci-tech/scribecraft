//! The transport dock: time, seek bar with bookmark marks, start / skip, held rewind and
//! fast-forward, play/stop and speed.
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

use crate::i18n::{self, Lang, Text};
use crate::theme;

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

/// What the dock needs to know about the transport this frame, read under one lock.
#[derive(Debug, Clone, Copy)]
struct Snapshot {
    state: State,
    pos: u64,
    dur: u64,
    speed: f64,
    preset: SpeedPreset,
    rewind_rate: f64,
    fast_rate: f64,
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

    /// Draws the dock. `bookmarks` are the open dictation's bookmark times, marked on the
    /// seek bar so they can be seen without opening the list.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, transport: &Arc<Mutex<Transport>>, skip_ms: u64, bookmarks: &[u64], lang: Lang) {
        let now = {
            let t = lock_transport(transport);
            Snapshot {
                state: t.state(),
                pos: t.position_ms(),
                dur: t.duration_ms(),
                speed: t.speed(),
                preset: t.preset(),
                rewind_rate: t.settings().rewind_rate,
                fast_rate: t.settings().fast_forward_rate,
            }
        };
        // The speed block only fits beside the transport buttons on a wide window.
        let wide = ui.available_width() >= 780.0;
        theme::card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            header(ui, &now, lang);
            ui.add_space(4.0);
            self.seek_bar(ui, transport, now.pos, now.dur, bookmarks);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                self.buttons(ui, transport, skip_ms, &now, lang);
                if wide {
                    ui.separator();
                    speed_controls(ui, transport, &now, lang);
                }
            });
            if !wide {
                ui.horizontal(|ui| speed_controls(ui, transport, &now, lang));
            }
            ui.add_space(2.0);
            ui.add(egui::Label::new(egui::RichText::new(Text::ScanHint.in_lang(lang)).size(11.0).color(theme::faint())).wrap());
        });
    }

    /// The seek bar: dragging previews the target, releasing seeks; bookmarks are marked on
    /// top and the playhead gets an accent dot.
    fn seek_bar(&mut self, ui: &mut egui::Ui, transport: &Arc<Mutex<Transport>>, pos: u64, dur: u64, bookmarks: &[u64]) {
        let mut value = self.scrub.unwrap_or(pos as f64);
        let normal_width = ui.spacing().slider_width;
        let normal_height = ui.spacing().interact_size.y;
        ui.spacing_mut().slider_width = (ui.available_width() - 2.0).max(80.0);
        // A shallower seek bar than a normal widget: it is wide, not tall.
        ui.spacing_mut().interact_size.y = 22.0;
        let slider = ui.add(egui::Slider::new(&mut value, 0.0..=(dur.max(1) as f64)).show_value(false).trailing_fill(true));
        ui.spacing_mut().slider_width = normal_width;
        ui.spacing_mut().interact_size.y = normal_height;

        let rail = slider.rect;
        let painter = ui.painter();
        for &ms in bookmarks {
            let x = bookmark_x(rail, ms, dur);
            let tick = egui::Rect::from_min_max(
                egui::pos2(x - 1.0, rail.center().y - theme::RAIL_HEIGHT / 2.0 - 5.0),
                egui::pos2(x + 1.0, rail.center().y + theme::RAIL_HEIGHT / 2.0),
            );
            painter.rect_filled(tick, 1.0, theme::amber());
        }
        let shown = self.scrub.unwrap_or(pos as f64);
        painter.circle_filled(egui::pos2(bookmark_x(rail, shown.max(0.0).min(dur as f64) as u64, dur), rail.center().y), 3.5, theme::accent());

        if slider.dragged() {
            self.scrub = Some(value);
        }
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            let target = value.clamp(0.0, dur as f64);
            lock_transport(transport).seek_ms(target as u64);
            self.scrub = None;
        }
    }

    /// Start, skip back, rewind, play/stop, fast-forward and skip forward.
    fn buttons(&mut self, ui: &mut egui::Ui, transport: &Arc<Mutex<Transport>>, skip_ms: u64, now: &Snapshot, lang: Lang) {
        let time = ui.input(|i| i.time);
        let skip = i64::try_from(skip_ms).unwrap_or(i64::MAX);

        if icon_button(ui, "⏮", Text::GoToStart.in_lang(lang)).clicked() {
            lock_transport(transport).seek_ms(0);
        }
        if skip_button(ui, format!("« {}", skip_label(skip_ms)), Text::SkipBack.in_lang(lang)).clicked() {
            lock_transport(transport).skip_ms(-skip);
        }

        let rew = icon_button(ui, "⏪", &i18n::rewind_hint(now.rewind_rate, lang));
        let action = self.rewind.update(rew.is_pointer_button_down_on(), rew.clicked(), time, now.state, now.pos);
        self.rewind.apply(action, transport, skip);

        let (text, hint) = if now.state == State::Playing { (Text::Stop, Text::StopHint) } else { (Text::Play, Text::PlayHint) };
        let play = egui::Button::new(
            egui::RichText::new(format!("{}  {}", if now.state == State::Playing { "⏸" } else { "▶" }, text.in_lang(lang)))
                .size(15.0)
                .strong()
                .color(theme::accent_ink()),
        )
        .fill(theme::accent())
        .stroke(egui::Stroke::new(1.0, theme::accent()))
        .corner_radius(9)
        .min_size(egui::vec2(112.0, 38.0));
        if ui.add(play).on_hover_text(hint.in_lang(lang)).clicked() {
            lock_transport(transport).toggle_play();
        }

        let ff = icon_button(ui, "⏩", &i18n::forward_hint(now.fast_rate, lang));
        let action = self.forward.update(ff.is_pointer_button_down_on(), ff.clicked(), time, now.state, now.pos);
        self.forward.apply(action, transport, skip);

        if skip_button(ui, format!("{} »", skip_label(skip_ms)), Text::SkipForward.in_lang(lang)).clicked() {
            lock_transport(transport).skip_ms(skip);
        }
    }
}

/// The clock, the state pill and the time remaining.
fn header(ui: &mut egui::Ui, now: &Snapshot, lang: Lang) {
    // Below this width the percentage would push the remaining time off the row; the speed
    // row still shows it.
    let compact = ui.available_width() < 620.0;
    ui.horizontal(|ui| {
        let clock = egui::RichText::new(format_timestamp(now.pos, true)).font(egui::FontId::monospace(30.0)).strong().color(theme::text());
        ui.label(clock);
        let total = egui::RichText::new(format!("/ {}", format_timestamp(now.dur, true))).font(egui::FontId::monospace(15.0)).color(theme::faint());
        ui.label(total).on_hover_text(Text::Length.in_lang(lang));
        ui.add_space(4.0);
        state_pill(ui, now.state, now.rewind_rate, now.fast_rate, lang);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remaining = now.dur.saturating_sub(now.pos);
            ui.label(
                egui::RichText::new(format!("−{}", format_timestamp(remaining, true))).font(egui::FontId::monospace(13.0)).color(theme::muted()),
            )
            .on_hover_text(Text::TimeRemaining.in_lang(lang));
            if !compact {
                ui.label(egui::RichText::new(format!("{:.0} %", now.speed * 100.0)).size(12.0).color(theme::faint()));
            }
        });
    });
}

/// What the state pill says, including the rate of a scan.
fn state_text(state: State, rewind_rate: f64, fast_rate: f64, lang: Lang) -> String {
    match state {
        State::Stopped => Text::Stopped.in_lang(lang).to_owned(),
        State::Playing => Text::Playing.in_lang(lang).to_owned(),
        State::Rewinding => i18n::fill(Text::Rewinding.in_lang(lang), &[("{rate}", &format!("{rewind_rate:.0}"))]),
        State::FastForwarding => i18n::fill(Text::FastForwarding.in_lang(lang), &[("{rate}", &format!("{fast_rate:.0}"))]),
    }
}

/// A pill that says what the transport is doing, with a lamp and the scan rate.
fn state_pill(ui: &mut egui::Ui, state: State, rewind_rate: f64, fast_rate: f64, lang: Lang) {
    let (fill, ink, lamp) = match state {
        State::Stopped => (theme::card_hi(), theme::muted(), theme::faint()),
        State::Playing => (theme::accent_dim(), theme::accent_soft(), theme::accent()),
        State::Rewinding | State::FastForwarding => (theme::amber_dim(), theme::amber_soft(), theme::amber()),
    };
    let text = state_text(state, rewind_rate, fast_rate, lang);
    theme::pill_frame(fill).show(ui, |ui| {
        ui.set_min_width(104.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.5, lamp);
            ui.label(egui::RichText::new(text).size(12.0).strong().color(ink));
        });
    });
}

/// Slow / fast presets, the speed slider and the reset.
fn speed_controls(ui: &mut egui::Ui, transport: &Arc<Mutex<Transport>>, now: &Snapshot, lang: Lang) {
    ui.spacing_mut().item_spacing.x = 6.0;
    if ui
        .selectable_label(now.preset == SpeedPreset::Slow, Text::SlowPreset.in_lang(lang))
        .on_hover_text(Text::SlowPresetHint.in_lang(lang))
        .clicked()
    {
        lock_transport(transport).toggle_slow();
    }
    if ui
        .selectable_label(now.preset == SpeedPreset::Fast, Text::FastPreset.in_lang(lang))
        .on_hover_text(Text::FastPresetHint.in_lang(lang))
        .clicked()
    {
        lock_transport(transport).toggle_fast();
    }
    let mut pct = now.speed * 100.0;
    ui.spacing_mut().slider_width = 110.0;
    let slider = ui.add(egui::Slider::new(&mut pct, MIN_SPEED * 100.0..=MAX_SPEED * 100.0).suffix(" %"));
    if slider.changed() {
        let _ = lock_transport(transport).set_speed((pct / 100.0).clamp(MIN_SPEED, MAX_SPEED));
    }
    if ui.button("100 %").on_hover_text(Text::ResetSpeed.in_lang(lang)).clicked() {
        let _ = lock_transport(transport).set_speed(1.0);
    }
}

/// A square button for one transport action.
fn icon_button(ui: &mut egui::Ui, glyph: &str, hint: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(glyph).size(17.0)).min_size(egui::vec2(38.0, 34.0))).on_hover_text(hint)
}

/// A wide button for a skip; the label says how far.
fn skip_button(ui: &mut egui::Ui, text: String, hint: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).size(13.0)).min_size(egui::vec2(58.0, 34.0))).on_hover_text(hint)
}

/// Where `ms` falls on the seek rail, as a fraction. `0` for an empty dictation.
fn fraction_of(ms: u64, dur: u64) -> f32 {
    if dur == 0 {
        return 0.0;
    }
    (ms as f64 / dur as f64).clamp(0.0, 1.0) as f32
}

/// The x of `ms` on a seek bar `rect`, using the same geometry as egui's slider: the handle
/// radius is free at each end of the rail.
fn bookmark_x(rect: egui::Rect, ms: u64, dur: u64) -> f32 {
    let inset = rect.height() / 2.5;
    let range = rect.x_range().shrink(inset);
    range.min + fraction_of(ms, dur) * range.span()
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
        draw(bar, t, &[]);
    }

    /// Draws one complete interface frame and returns the shapes it painted.
    fn draw(bar: &mut TransportBar, t: &Arc<Mutex<Transport>>, bookmarks: &[u64]) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| bar.show(ui, t, 5_000, bookmarks, Lang::English));
        // No renderer here; drop the font-atlas deltas as a renderer would consume them.
        let shapes = output.shapes.iter().map(|s| s.shape.clone()).collect();
        output.textures_delta.clear();
        shapes
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
    fn bookmark_marks_land_where_the_slider_puts_the_playhead() {
        // A 210-point bar: the handle radius (18 / 2.5 = 7.2) is free at each end.
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(210.0, 18.0));
        assert!((bookmark_x(rect, 0, 10_000) - 17.2).abs() < 0.01);
        assert!((bookmark_x(rect, 10_000, 10_000) - 212.8).abs() < 0.01);
        assert!((bookmark_x(rect, 5_000, 10_000) - 115.0).abs() < 0.01, "the middle of the rail");
        assert_eq!(fraction_of(20_000, 10_000), 1.0, "a mark past the end stays at the end");
        assert_eq!(fraction_of(0, 0), 0.0, "an empty dictation has no marks");
    }

    #[test]
    fn bookmarks_are_drawn_on_the_seek_bar() {
        let t = Arc::new(Mutex::new(transport()));
        let mut bar = TransportBar::default();
        let shapes = draw(&mut bar, &t, &[2_500, 7_500]);
        let marks = shapes
            .iter()
            .filter(|s| match s {
                egui::Shape::Rect(r) => r.fill == theme::amber(),
                _ => false,
            })
            .count();
        assert_eq!(marks, 2, "one amber mark per bookmark");
    }

    #[test]
    fn skip_labels_are_short_and_honest() {
        assert_eq!(skip_label(5_000), "5 s");
        assert_eq!(skip_label(750), "750 ms");
        assert_eq!(skip_label(0), "0 s");
    }

    /// The state pill follows the chosen language, and a scan still names its rate.
    #[test]
    fn the_transport_state_is_said_in_the_chosen_language() {
        assert_eq!(state_text(State::Playing, 8.0, 8.0, Lang::English), "playing");
        assert_eq!(state_text(State::Playing, 8.0, 8.0, Lang::Italiano), "in riproduzione");
        assert_eq!(state_text(State::Rewinding, 8.0, 8.0, Lang::Italiano), "riavvolgimento 8×");
        assert_eq!(state_text(State::FastForwarding, 8.0, 12.0, Lang::English), "fast-forwarding 12×");
    }
}
