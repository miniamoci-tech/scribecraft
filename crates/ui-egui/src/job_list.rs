//! The dictation queue: filter chips that keep it short, and one row per job.
//!
//! Rows are painted rather than built from widgets so that the priority stripe, the truncated
//! title, the meta line and the duration keep their places at any width. Metadata is drawn in
//! [`theme::muted()`] (or [`theme::accent_soft()`] on the open row), never in the quietest colour:
//! small text on a hovered or selected row still has to reach WCAG AA.

use eframe::egui::{self, Color32, FontFamily, FontId, Stroke};

use scribecraft_engine::transport::format_timestamp;
use scribecraft_engine::{Dictation, Priority, Status};

use crate::i18n::{Lang, Text};
use crate::theme;

/// Height of one row in the queue.
const ROW_HEIGHT: f32 = 50.0;
/// Corner radius of a row's background.
const ROW_RADIUS: f32 = 8.0;

fn title_font() -> FontId {
    FontId::new(14.0, FontFamily::Proportional)
}

fn small_font() -> FontId {
    FontId::new(12.0, FontFamily::Proportional)
}

/// What the transcriptionist wants to see in the queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Filter {
    #[default]
    All,
    Todo,
    InProgress,
    Done,
    Dispatched,
}

impl Filter {
    /// Every filter, in the order the chips are shown.
    pub(crate) const ALL: [Filter; 5] = [Filter::All, Filter::Todo, Filter::InProgress, Filter::Done, Filter::Dispatched];

    pub(crate) fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::All => Text::FilterAll.in_lang(lang),
            Self::Todo => Text::FilterTodo.in_lang(lang),
            Self::InProgress => Text::FilterInProgress.in_lang(lang),
            Self::Done => Text::FilterDone.in_lang(lang),
            Self::Dispatched => Text::FilterSent.in_lang(lang),
        }
    }

    /// Whether a dictation with `status` belongs under this filter.
    pub(crate) fn matches(self, status: Status) -> bool {
        match self {
            Self::All => true,
            Self::Todo => status == Status::New,
            Self::InProgress => status == Status::InProgress,
            Self::Done => status == Status::Done,
            Self::Dispatched => status == Status::Dispatched,
        }
    }
}

/// The bit of a dictation the queue shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) priority: Priority,
    pub(crate) status: Status,
    pub(crate) duration_ms: Option<u64>,
}

impl Row {
    pub(crate) fn of(d: &Dictation) -> Self {
        Self { id: d.id, title: d.title.clone(), author: d.author.clone(), priority: d.priority, status: d.status, duration_ms: d.duration_ms }
    }

    /// The stripe on the left: urgent work in red, high priority in amber, the rest quiet.
    fn stripe(&self) -> Option<Color32> {
        match self.priority {
            Priority::Urgent => Some(theme::red()),
            Priority::High => Some(theme::amber()),
            Priority::Normal | Priority::Low => None,
        }
    }

    /// The title as shown: never an empty line when a file has no name.
    fn display_title(&self, lang: Lang) -> &str {
        if self.title.trim().is_empty() { Text::Untitled.in_lang(lang) } else { &self.title }
    }

    /// The second line: status, then the author (or an honest "no author").
    fn meta(&self, lang: Lang) -> String {
        let author = if self.author.trim().is_empty() { Text::NoAuthor.in_lang(lang) } else { self.author.trim() };
        format!("{} · {author}", status_name(self.status, lang))
    }

    fn duration(&self) -> String {
        self.duration_ms.map(|d| format_timestamp(d, false)).unwrap_or_else(|| "--:--:--".into())
    }
}

/// The chips above the queue. Returns true when the filter changed.
pub(crate) fn chips(ui: &mut egui::Ui, filter: &mut Filter, rows: &[Row], lang: Lang) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for candidate in Filter::ALL {
            let text = format!("{} {}", candidate.label(lang), count(rows, candidate));
            if chip(ui, &text, *filter == candidate).clicked() {
                *filter = candidate;
                changed = true;
            }
        }
    });
    changed
}

/// How many rows a filter keeps.
pub(crate) fn count(rows: &[Row], filter: Filter) -> usize {
    rows.iter().filter(|r| filter.matches(r.status)).count()
}

/// One pill-shaped filter button.
fn chip(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), small_font(), theme::text());
    let padding = egui::vec2(10.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(galley.size() + 2.0 * padding, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (fill, stroke, colour) = if selected {
        (theme::accent_dim(), theme::accent_dim(), theme::accent_soft())
    } else if response.hovered() {
        (theme::card_hi(), theme::line(), theme::text())
    } else {
        (Color32::TRANSPARENT, theme::line(), theme::muted())
    };
    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, ROW_RADIUS * 10.0, fill);
    }
    painter.rect_stroke(rect, ROW_RADIUS * 10.0, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    painter.galley(rect.min + padding, galley, colour);
    response
}

/// One dictation: a priority stripe, the title, its state and author, and the duration.
pub(crate) fn row(ui: &mut egui::Ui, row: &Row, open: bool, lang: Lang) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (fill, title_colour, meta_colour) = if open {
        (theme::accent_dim(), theme::text(), theme::accent_soft())
    } else if response.hovered() {
        (theme::card_hi(), theme::text(), theme::muted())
    } else {
        (Color32::TRANSPARENT, theme::text(), theme::muted())
    };
    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, ROW_RADIUS, fill);
    }
    if open {
        painter.rect_stroke(rect, ROW_RADIUS, Stroke::new(1.0, theme::accent()), egui::StrokeKind::Inside);
    }
    if let Some(colour) = row.stripe() {
        let stripe = egui::Rect::from_min_size(egui::pos2(rect.left() + 4.0, rect.top() + 9.0), egui::vec2(3.0, rect.height() - 18.0));
        painter.rect_filled(stripe, 2.0, colour);
    }
    let left = rect.left() + 14.0;
    let right = rect.right() - 10.0;
    let duration = one_line(painter, &row.duration(), small_font(), meta_colour, right - left);
    let text_width = (right - left - duration.size().x - 10.0).max(0.0);
    let title = one_line(painter, row.display_title(lang), title_font(), title_colour, text_width);
    let meta = one_line(painter, &row.meta(lang), small_font(), meta_colour, text_width);
    painter.galley(egui::pos2(left, rect.top() + 8.0), title, title_colour);
    painter.galley(egui::pos2(left, rect.top() + 27.0), meta, meta_colour);
    painter.galley(egui::pos2(right - duration.size().x, rect.center().y - duration.size().y / 2.0), duration, meta_colour);
    response
}

/// What the right-click menu on a queue row asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowAction {
    /// Flag the dictation as completed.
    MarkDone,
    /// Put a completed dictation back among the work to do.
    MarkToDo,
    /// Remove it from the list (after a confirmation; the recording file stays).
    Remove,
}

impl RowAction {
    /// The status a dictation gets from this action, if it changes the status.
    pub(crate) fn status(self) -> Option<Status> {
        match self {
            RowAction::MarkDone => Some(Status::Done),
            RowAction::MarkToDo => Some(Status::InProgress),
            RowAction::Remove => None,
        }
    }
}

/// The right-click menu of a queue row: flag it completed (or back to do), or remove it.
pub(crate) fn row_menu(ui: &mut egui::Ui, row: &Row, lang: Lang) -> Option<RowAction> {
    let mut action = None;
    let finished = matches!(row.status, Status::Done | Status::Dispatched);
    if finished {
        if ui.button(Text::MarkToDo.in_lang(lang)).clicked() {
            action = Some(RowAction::MarkToDo);
        }
    } else if ui.button(Text::MarkDone.in_lang(lang)).clicked() {
        action = Some(RowAction::MarkDone);
    }
    ui.separator();
    if ui.button(egui::RichText::new(Text::RemoveDictation.in_lang(lang)).color(theme::red())).on_hover_text(Text::RemoveHint.in_lang(lang)).clicked()
    {
        action = Some(RowAction::Remove);
    }
    if action.is_some() {
        ui.close();
    }
    action
}

/// Lays out `text` on one row, eliding it with an ellipsis past `max_width`.
fn one_line(painter: &egui::Painter, text: &str, font: FontId, colour: Color32, max_width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat { font_id: font, color: colour, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

pub(crate) fn status_name(s: Status, lang: Lang) -> &'static str {
    match s {
        Status::New => Text::StatusNew.in_lang(lang),
        Status::InProgress => Text::StatusInProgress.in_lang(lang),
        Status::Done => Text::StatusDone.in_lang(lang),
        Status::Dispatched => Text::StatusDispatched.in_lang(lang),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(status: Status, priority: Priority) -> Row {
        Row { id: 1, title: "Cardiology follow-up".into(), author: "Dr. Example".into(), priority, status, duration_ms: Some(42_000) }
    }

    /// Draws one row at a known screen size and returns the shapes it painted.
    fn draw(job: &Row, open: bool) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            row(ui, job, open, Lang::English);
        });
        let shapes = output.shapes.iter().map(|s| s.shape.clone()).collect();
        output.textures_delta.clear();
        shapes
    }

    fn rects(shapes: &[egui::Shape]) -> Vec<&egui::epaint::RectShape> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    /// Runs one interface frame over the chip bar and returns what it painted.
    fn pass(ctx: &egui::Context, events: Vec<egui::Event>, filter: &mut Filter, rows: &[Row]) -> Vec<egui::Shape> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            chips(ui, filter, rows, Lang::English);
        });
        let shapes = output.shapes.iter().map(|s| s.shape.clone()).collect();
        output.textures_delta.clear();
        shapes
    }

    /// The five chips, left to right, as painted on screen.
    fn chip_rects(shapes: &[egui::Shape]) -> Vec<egui::Rect> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 => Some(r.rect),
                _ => None,
            })
            .collect()
    }

    /// A real click on "Done" keeps only finished work (the screenshot check needs a window server).
    #[test]
    fn clicking_a_chip_shows_only_that_status() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let rows = [sample(Status::New, Priority::Normal), sample(Status::Done, Priority::Low)];
        let mut filter = Filter::All;
        pass(&ctx, Vec::new(), &mut filter, &rows);
        let rects = chip_rects(&pass(&ctx, Vec::new(), &mut filter, &rows));
        assert_eq!(rects.len(), Filter::ALL.len(), "one chip per filter");
        assert_eq!(Filter::ALL[3], Filter::Done, "the fourth chip is Done");
        let target = rects[3].center();
        let click = vec![
            egui::Event::PointerMoved(target),
            egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() },
            egui::Event::PointerButton { pos: target, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() },
        ];
        pass(&ctx, click, &mut filter, &rows);
        assert_eq!(filter, Filter::Done, "clicking Done must filter the queue to finished work");
        assert_eq!(count(&rows, filter), 1);
    }

    /// The queue speaks the chosen language: chips, status line and the empty-title fallback.
    #[test]
    fn the_queue_speaks_the_chosen_language() {
        assert_eq!(Filter::All.label(Lang::Italiano), "Tutte");
        assert_eq!(Filter::Dispatched.label(Lang::Italiano), "Inviate");
        assert_eq!(status_name(Status::InProgress, Lang::Italiano), "in corso");
        assert_eq!(status_name(Status::Done, Lang::English), "done");
        let row = Row { title: "  ".into(), ..sample(Status::New, Priority::Normal) };
        assert_eq!(row.display_title(Lang::Italiano), "Dettatura senza titolo");
        assert_eq!(row.meta(Lang::Italiano), "nuova · Dr. Example");
    }

    /// Every status belongs to exactly one chip other than "All".
    #[test]
    fn the_chips_partition_the_statuses() {
        for status in [Status::New, Status::InProgress, Status::Done, Status::Dispatched] {
            assert!(Filter::All.matches(status), "All must keep every status");
            let owners: Vec<Filter> = Filter::ALL.iter().copied().filter(|f| *f != Filter::All && f.matches(status)).collect();
            assert_eq!(owners.len(), 1, "{status:?} must be under exactly one chip, found {owners:?}");
        }
    }

    #[test]
    fn the_counts_add_up_to_the_queue() {
        let rows = [sample(Status::New, Priority::Normal), sample(Status::InProgress, Priority::High), sample(Status::Done, Priority::Low)];
        assert_eq!(count(&rows, Filter::All), 3);
        assert_eq!(count(&rows, Filter::Todo), 1);
        assert_eq!(count(&rows, Filter::InProgress), 1);
        assert_eq!(count(&rows, Filter::Done), 1);
        assert_eq!(count(&rows, Filter::Dispatched), 0);
    }

    #[test]
    fn an_urgent_row_paints_a_red_stripe() {
        let shapes = draw(&sample(Status::New, Priority::Urgent), false);
        assert!(rects(&shapes).iter().any(|r| r.fill == theme::red()), "an urgent dictation must be marked in red");
    }

    #[test]
    fn a_high_priority_row_paints_an_amber_stripe() {
        let shapes = draw(&sample(Status::New, Priority::High), false);
        assert!(rects(&shapes).iter().any(|r| r.fill == theme::amber()), "a high-priority dictation must be marked in amber");
    }

    #[test]
    fn a_normal_row_has_no_stripe() {
        let shapes = draw(&sample(Status::New, Priority::Normal), false);
        let stripes = rects(&shapes).iter().filter(|r| r.fill == theme::amber() || r.fill == theme::red()).count();
        assert_eq!(stripes, 0, "ordinary work must stay quiet");
    }

    #[test]
    fn the_open_row_is_highlighted() {
        let shapes = draw(&sample(Status::InProgress, Priority::Normal), true);
        let rects = rects(&shapes);
        assert!(rects.iter().any(|r| r.fill == theme::accent_dim()), "the open dictation must stand out");
        assert!(rects.iter().any(|r| r.stroke.color == theme::accent()), "the open dictation gets an accent outline");
    }

    #[test]
    fn a_long_title_is_elided_instead_of_pushing_the_duration_out() {
        let ctx = egui::Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let mut measured = None;
        let mut output = ctx.run_ui(input, |ui| {
            let long = "a".repeat(400);
            let galley = one_line(ui.painter(), &long, title_font(), theme::text(), 120.0);
            measured = Some((galley.size().x, galley.elided));
        });
        output.textures_delta.clear();
        let (width, elided) = measured.expect("the row must be measured");
        assert!(elided, "a title wider than its column must be elided");
        assert!(width <= 121.0, "an elided title must not exceed its column: {width} pt");
    }
}
