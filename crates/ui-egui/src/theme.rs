//! The interface's palette, type, spacing and frames.
//!
//! One place to change how ScribeCraft looks. The palette is checked in the tests at the
//! bottom of this file: text must reach WCAG AA contrast (4.5:1) on every surface it is
//! drawn on, and the accent and bookmark colours at least 3:1. Separators are decorative
//! and stay quiet, as in the mockup.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Stroke};

/// Behind everything: the window, the central area and the gap around the transport dock.
pub(crate) const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1a);
/// Toolbar, job list and other side panels.
pub(crate) const PANEL: Color32 = Color32::from_rgb(0x1c, 0x1f, 0x25);
/// Cards and buttons that sit on top of a panel.
pub(crate) const CARD: Color32 = Color32::from_rgb(0x22, 0x26, 0x2e);
/// A raised or hovered card.
pub(crate) const CARD_HI: Color32 = Color32::from_rgb(0x2b, 0x31, 0x3b);
/// Separators and widget outlines.
pub(crate) const LINE: Color32 = Color32::from_rgb(0x2d, 0x32, 0x3c);
/// Text fields, slider rails and the deepest recesses.
pub(crate) const TRACK: Color32 = Color32::from_rgb(0x12, 0x15, 0x1a);

/// Body text.
pub(crate) const TEXT: Color32 = Color32::from_rgb(0xe8, 0xea, 0xee);
/// Secondary text: metadata, hints and units.
pub(crate) const MUTED: Color32 = Color32::from_rgb(0xa8, 0xb0, 0xbb);
/// The quietest text that still has to be read.
pub(crate) const FAINT: Color32 = Color32::from_rgb(0x84, 0x8d, 0x9b);

/// The product colour: playback, progress and selection.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x5e, 0xea, 0xd4);
/// Text drawn on [`ACCENT`].
pub(crate) const ACCENT_INK: Color32 = Color32::from_rgb(0x08, 0x21, 0x1e);
/// The dim accent used for selected and active backgrounds.
pub(crate) const ACCENT_DIM: Color32 = Color32::from_rgb(0x1f, 0x4a, 0x45);
/// Text drawn on [`ACCENT_DIM`].
pub(crate) const ACCENT_SOFT: Color32 = Color32::from_rgb(0xc7, 0xf9, 0xf2);

/// Bookmarks and other things the transcriptionist must find again.
pub(crate) const AMBER: Color32 = Color32::from_rgb(0xf4, 0xb8, 0x60);
/// The dim amber used for scanning backgrounds.
pub(crate) const AMBER_DIM: Color32 = Color32::from_rgb(0x4a, 0x3a, 0x1d);
/// Text drawn on [`AMBER_DIM`].
pub(crate) const AMBER_SOFT: Color32 = Color32::from_rgb(0xff, 0xd8, 0x9a);
/// Errors and urgent jobs.
pub(crate) const RED: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);

/// Height of the seek bar's rail. [`apply`] and the bookmark marks share it.
pub(crate) const RAIL_HEIGHT: f32 = 6.0;

/// Applies the palette and the shared widget style.
pub(crate) fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = TRACK;
        visuals.text_edit_bg_color = Some(TRACK);
        visuals.faint_bg_color = CARD;
        visuals.code_bg_color = TRACK;
        visuals.override_text_color = Some(TEXT);
        visuals.weak_text_color = Some(FAINT);
        visuals.hyperlink_color = ACCENT;
        visuals.error_fg_color = RED;
        visuals.warn_fg_color = AMBER;
        visuals.window_stroke = Stroke::new(1.0, LINE);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.selection = egui::style::Selection { bg_fill: ACCENT_DIM, stroke: Stroke::new(1.0, ACCENT) };
        visuals.slider_trailing_fill = true;

        let radius = CornerRadius::same(8);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = BG;
        widgets.noninteractive.weak_bg_fill = PANEL;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
        widgets.noninteractive.corner_radius = radius;

        widgets.inactive.bg_fill = TRACK;
        widgets.inactive.weak_bg_fill = CARD;
        widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.inactive.corner_radius = radius;

        widgets.hovered.bg_fill = CARD_HI;
        widgets.hovered.weak_bg_fill = CARD_HI;
        widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
        widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.hovered.corner_radius = radius;

        widgets.active.bg_fill = ACCENT_DIM;
        widgets.active.weak_bg_fill = ACCENT_DIM;
        widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        widgets.active.fg_stroke = Stroke::new(1.0, ACCENT_SOFT);
        widgets.active.corner_radius = radius;

        widgets.open.bg_fill = CARD;
        widgets.open.weak_bg_fill = CARD;
        widgets.open.bg_stroke = Stroke::new(1.0, LINE);
        widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
        widgets.open.corner_radius = radius;

        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.slider_width = 120.0;
        style.spacing.slider_rail_height = RAIL_HEIGHT;
        style.animation_time = 0.12;

        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// The toolbar at the top of the window.
pub(crate) fn top_frame() -> Frame {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(14, 8))
}

/// The job list on the left.
pub(crate) fn side_frame() -> Frame {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(12, 12))
}

/// The status line at the very bottom.
pub(crate) fn status_frame() -> Frame {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(Margin::symmetric(14, 4))
}

/// The strip that holds the transport dock. The dock itself is a [`card_frame`].
pub(crate) fn dock_frame() -> Frame {
    Frame::new().fill(BG).inner_margin(Margin::symmetric(14, 10))
}

/// Around the open dictation: settings and notes.
pub(crate) fn central_frame() -> Frame {
    Frame::new().fill(BG).inner_margin(Margin::symmetric(16, 12))
}

/// A card: a raised surface for one group of controls.
pub(crate) fn card_frame() -> Frame {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(70) })
}

/// A pill: a small rounded label such as the transport state or a job status.
pub(crate) fn pill_frame(fill: Color32) -> Frame {
    Frame::new().fill(fill).corner_radius(CornerRadius::same(99)).inner_margin(Margin::symmetric(9, 3))
}

/// A section caption: small, spaced capitals in [`MUTED`].
pub(crate) fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(11.5).strong().color(MUTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f32 {
        fn channel(v: u8) -> f32 {
            let s = f32::from(v) / 255.0;
            if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        }
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        for (fg, bg, what) in [
            (TEXT, BG, "text on the window"),
            (TEXT, PANEL, "text on a panel"),
            (TEXT, CARD, "text on a card"),
            (TEXT, CARD_HI, "text on a hovered card"),
            (TEXT, ACCENT_DIM, "text on the open row"),
            (MUTED, BG, "muted text on the window"),
            (MUTED, PANEL, "muted text on a panel"),
            (MUTED, CARD, "muted text on a card"),
            (MUTED, CARD_HI, "muted text on a hovered row"),
            (MUTED, ACCENT_DIM, "muted text on the open row"),
            (FAINT, BG, "faint text on the window"),
            (FAINT, PANEL, "faint text on a panel"),
            (FAINT, CARD, "faint text on a card"),
            (ACCENT_INK, ACCENT, "the play button"),
            (ACCENT_SOFT, ACCENT_DIM, "the playing pill"),
            (AMBER_SOFT, AMBER_DIM, "the scanning pill"),
            (RED, PANEL, "an error on the status line"),
        ] {
            let ratio = contrast(fg, bg);
            assert!(ratio >= 4.5, "{what}: {ratio:.2}:1, need 4.5:1");
        }
    }

    #[test]
    fn accents_stand_out_from_their_surfaces() {
        for (fg, bg, what) in [
            (ACCENT, BG, "the accent on the window"),
            (ACCENT, PANEL, "the accent on a panel"),
            (ACCENT, CARD, "the accent on a card"),
            (AMBER, BG, "a bookmark on the window"),
            (AMBER, CARD, "a bookmark on a card"),
            (AMBER, PANEL, "a high-priority stripe"),
            (RED, PANEL, "an urgent stripe"),
            (RED, CARD_HI, "an urgent stripe on a hovered row"),
        ] {
            let ratio = contrast(fg, bg);
            assert!(ratio >= 3.0, "{what}: {ratio:.2}:1, need 3:1");
        }
    }

    #[test]
    fn applying_the_theme_is_idempotent() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let first = ctx.style_of(egui::Theme::Dark);
        apply(&ctx);
        let second = ctx.style_of(egui::Theme::Dark);
        assert_eq!(first.visuals.panel_fill, second.visuals.panel_fill);
        assert_eq!(first.text_styles, second.text_styles);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
    }
}
