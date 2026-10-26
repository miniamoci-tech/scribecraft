//! The interface's palette, type, spacing and frames.
//!
//! Two palettes, dark and light, chosen in the toolbar and remembered in `ui.json`. The
//! colours are read through the small functions at the top ([`text`], [`muted`], …) so a
//! widget never holds a colour from the other theme. The tests at the bottom check both
//! palettes: text must reach WCAG AA contrast (4.5:1) on every surface it is drawn on, and
//! the accent and bookmark colours at least 3:1.

use std::cell::Cell;

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Stroke};
use serde::{Deserialize, Serialize};

use crate::i18n::{Lang, Text};

/// Which palette the window uses. Dark is the default; the choice is saved in `ui.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    /// Both themes, in the order the selector lists them.
    pub(crate) const ALL: [Theme; 2] = [Theme::Dark, Theme::Light];

    /// The theme's name in the interface language.
    pub(crate) fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::Dark => Text::ThemeDark.in_lang(lang),
            Self::Light => Text::ThemeLight.in_lang(lang),
        }
    }

    /// The short code written to `ui.json`.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Reads a saved code; `None` for anything unknown, so a file from another version still
    /// opens in the default theme.
    pub(crate) fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }

    fn egui(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }

    fn palette(self) -> Palette {
        match self {
            Self::Dark => Palette::DARK,
            Self::Light => Palette::LIGHT,
        }
    }
}

/// Every colour the interface draws with. One field per job, so both themes must fill them
/// all and a widget can never end up with a colour from nowhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Palette {
    /// Behind everything: the window, the central area and the gap around the transport dock.
    bg: Color32,
    /// Toolbar, job list and other side panels.
    panel: Color32,
    /// Cards and buttons that sit on top of a panel.
    card: Color32,
    /// A raised or hovered card.
    card_hi: Color32,
    /// Separators and widget outlines.
    line: Color32,
    /// Text fields, slider rails and the deepest recesses.
    track: Color32,
    /// Body text.
    text: Color32,
    /// Secondary text: metadata, hints and units.
    muted: Color32,
    /// The quietest text that still has to be read.
    faint: Color32,
    /// The product colour: playback, progress and selection.
    accent: Color32,
    /// Text drawn on `accent`.
    accent_ink: Color32,
    /// The dim accent used for selected and active backgrounds.
    accent_dim: Color32,
    /// Text drawn on `accent_dim`.
    accent_soft: Color32,
    /// Bookmarks and other things the transcriptionist must find again.
    amber: Color32,
    /// The dim amber used for scanning backgrounds.
    amber_dim: Color32,
    /// Text drawn on `amber_dim`.
    amber_soft: Color32,
    /// Errors and urgent jobs.
    red: Color32,
    /// Opacity of the shadow under a card, out of 255.
    shadow_alpha: u8,
}

impl Palette {
    const DARK: Self = Self {
        bg: Color32::from_rgb(0x14, 0x16, 0x1a),
        panel: Color32::from_rgb(0x1c, 0x1f, 0x25),
        card: Color32::from_rgb(0x22, 0x26, 0x2e),
        card_hi: Color32::from_rgb(0x2b, 0x31, 0x3b),
        line: Color32::from_rgb(0x2d, 0x32, 0x3c),
        track: Color32::from_rgb(0x12, 0x15, 0x1a),
        text: Color32::from_rgb(0xe8, 0xea, 0xee),
        muted: Color32::from_rgb(0xa8, 0xb0, 0xbb),
        faint: Color32::from_rgb(0x84, 0x8d, 0x9b),
        accent: Color32::from_rgb(0x5e, 0xea, 0xd4),
        accent_ink: Color32::from_rgb(0x08, 0x21, 0x1e),
        accent_dim: Color32::from_rgb(0x1f, 0x4a, 0x45),
        accent_soft: Color32::from_rgb(0xc7, 0xf9, 0xf2),
        amber: Color32::from_rgb(0xf4, 0xb8, 0x60),
        amber_dim: Color32::from_rgb(0x4a, 0x3a, 0x1d),
        amber_soft: Color32::from_rgb(0xff, 0xd8, 0x9a),
        red: Color32::from_rgb(0xf8, 0x71, 0x71),
        shadow_alpha: 70,
    };

    /// The light palette keeps the same jobs and the same accent family, darkened far enough
    /// to reach AA on white and near-white surfaces. It is not a colour inversion: cards
    /// stay white and the window a shade darker, so the layers still read.
    const LIGHT: Self = Self {
        bg: Color32::from_rgb(0xe9, 0xec, 0xf0),
        panel: Color32::from_rgb(0xf6, 0xf7, 0xf9),
        card: Color32::from_rgb(0xff, 0xff, 0xff),
        card_hi: Color32::from_rgb(0xed, 0xf0, 0xf4),
        line: Color32::from_rgb(0xd2, 0xd8, 0xdf),
        track: Color32::from_rgb(0xe4, 0xe8, 0xed),
        text: Color32::from_rgb(0x1b, 0x1f, 0x24),
        muted: Color32::from_rgb(0x4c, 0x55, 0x62),
        faint: Color32::from_rgb(0x5a, 0x64, 0x71),
        accent: Color32::from_rgb(0x0f, 0x6f, 0x67),
        accent_ink: Color32::from_rgb(0xff, 0xff, 0xff),
        accent_dim: Color32::from_rgb(0xc9, 0xea, 0xe5),
        accent_soft: Color32::from_rgb(0x0a, 0x3d, 0x38),
        amber: Color32::from_rgb(0x7a, 0x4f, 0x00),
        amber_dim: Color32::from_rgb(0xfa, 0xee, 0xd2),
        amber_soft: Color32::from_rgb(0x4a, 0x30, 0x00),
        red: Color32::from_rgb(0xa4, 0x1f, 0x14),
        shadow_alpha: 26,
    };
}

thread_local! {
    /// The palette in use. The interface is drawn on one thread, and a thread-local rather
    /// than a global keeps two windows (or two tests) from changing each other's colours.
    static CURRENT: Cell<Palette> = const { Cell::new(Palette::DARK) };
}

fn current() -> Palette {
    CURRENT.with(Cell::get)
}

pub(crate) fn bg() -> Color32 {
    current().bg
}
pub(crate) fn panel() -> Color32 {
    current().panel
}
pub(crate) fn card() -> Color32 {
    current().card
}
pub(crate) fn card_hi() -> Color32 {
    current().card_hi
}
pub(crate) fn line() -> Color32 {
    current().line
}
pub(crate) fn text() -> Color32 {
    current().text
}
pub(crate) fn muted() -> Color32 {
    current().muted
}
pub(crate) fn faint() -> Color32 {
    current().faint
}
pub(crate) fn accent() -> Color32 {
    current().accent
}
pub(crate) fn accent_ink() -> Color32 {
    current().accent_ink
}
pub(crate) fn accent_dim() -> Color32 {
    current().accent_dim
}
pub(crate) fn accent_soft() -> Color32 {
    current().accent_soft
}
pub(crate) fn amber() -> Color32 {
    current().amber
}
pub(crate) fn amber_dim() -> Color32 {
    current().amber_dim
}
pub(crate) fn amber_soft() -> Color32 {
    current().amber_soft
}
pub(crate) fn red() -> Color32 {
    current().red
}

/// Height of the seek bar's rail. [`apply`] and the bookmark marks share it.
pub(crate) const RAIL_HEIGHT: f32 = 6.0;

/// Applies a palette and the shared widget style, replacing whatever theme was in use.
pub(crate) fn apply(ctx: &egui::Context, theme: Theme) {
    let p = theme.palette();
    CURRENT.with(|c| c.set(p));
    let chosen = theme.egui();
    ctx.set_theme(chosen);
    ctx.style_mut_of(chosen, |style| {
        let visuals = &mut style.visuals;
        visuals.dark_mode = theme == Theme::Dark;
        visuals.panel_fill = p.panel;
        visuals.window_fill = p.panel;
        visuals.extreme_bg_color = p.track;
        visuals.text_edit_bg_color = Some(p.track);
        visuals.faint_bg_color = p.card;
        visuals.code_bg_color = p.track;
        visuals.override_text_color = Some(p.text);
        visuals.weak_text_color = Some(p.faint);
        visuals.hyperlink_color = p.accent;
        visuals.error_fg_color = p.red;
        visuals.warn_fg_color = p.amber;
        visuals.window_stroke = Stroke::new(1.0, p.line);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.selection = egui::style::Selection { bg_fill: p.accent_dim, stroke: Stroke::new(1.0, p.accent) };
        visuals.slider_trailing_fill = true;

        let radius = CornerRadius::same(8);
        let widgets = &mut visuals.widgets;
        widgets.noninteractive.bg_fill = p.bg;
        widgets.noninteractive.weak_bg_fill = p.panel;
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
        widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.muted);
        widgets.noninteractive.corner_radius = radius;

        widgets.inactive.bg_fill = p.track;
        widgets.inactive.weak_bg_fill = p.card;
        widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
        widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        widgets.inactive.corner_radius = radius;

        widgets.hovered.bg_fill = p.card_hi;
        widgets.hovered.weak_bg_fill = p.card_hi;
        widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent_dim);
        widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
        widgets.hovered.corner_radius = radius;

        widgets.active.bg_fill = p.accent_dim;
        widgets.active.weak_bg_fill = p.accent_dim;
        widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
        widgets.active.fg_stroke = Stroke::new(1.0, p.accent_soft);
        widgets.active.corner_radius = radius;

        widgets.open.bg_fill = p.card;
        widgets.open.weak_bg_fill = p.card;
        widgets.open.bg_stroke = Stroke::new(1.0, p.line);
        widgets.open.fg_stroke = Stroke::new(1.0, p.text);
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
    Frame::new().fill(panel()).stroke(Stroke::new(1.0, line())).inner_margin(Margin::symmetric(14, 8))
}

/// The job list on the left.
pub(crate) fn side_frame() -> Frame {
    Frame::new().fill(panel()).stroke(Stroke::new(1.0, line())).inner_margin(Margin::symmetric(12, 12))
}

/// The status line at the very bottom.
pub(crate) fn status_frame() -> Frame {
    Frame::new().fill(panel()).stroke(Stroke::new(1.0, line())).inner_margin(Margin::symmetric(14, 4))
}

/// The strip that holds the transport dock. The dock itself is a [`card_frame`].
pub(crate) fn dock_frame() -> Frame {
    Frame::new().fill(bg()).inner_margin(Margin::symmetric(14, 10))
}

/// Around the open dictation: settings and notes.
pub(crate) fn central_frame() -> Frame {
    Frame::new().fill(bg()).inner_margin(Margin::symmetric(16, 12))
}

/// A card: a raised surface for one group of controls.
pub(crate) fn card_frame() -> Frame {
    Frame::new()
        .fill(card())
        .stroke(Stroke::new(1.0, line()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(12))
        .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(current().shadow_alpha) })
}

/// A pill: a small rounded label such as the transport state or a job status.
pub(crate) fn pill_frame(fill: Color32) -> Frame {
    Frame::new().fill(fill).corner_radius(CornerRadius::same(99)).inner_margin(Margin::symmetric(9, 3))
}

/// A section caption: small, spaced capitals in [`muted`].
pub(crate) fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text).size(11.5).strong().color(muted())
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
    fn text_is_readable_on_every_surface_in_both_themes() {
        for (theme, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            for (fg, bg, what) in [
                (p.text, p.bg, "text on the window"),
                (p.text, p.panel, "text on a panel"),
                (p.text, p.card, "text on a card"),
                (p.text, p.card_hi, "text on a hovered card"),
                (p.text, p.accent_dim, "text on the open row"),
                (p.text, p.track, "text in a field"),
                (p.muted, p.bg, "muted text on the window"),
                (p.muted, p.panel, "muted text on a panel"),
                (p.muted, p.card, "muted text on a card"),
                (p.muted, p.card_hi, "muted text on a hovered row"),
                (p.muted, p.accent_dim, "muted text on the open row"),
                (p.faint, p.bg, "faint text on the window"),
                (p.faint, p.panel, "faint text on a panel"),
                (p.faint, p.card, "faint text on a card"),
                (p.accent_ink, p.accent, "the play button"),
                (p.accent_soft, p.accent_dim, "the playing pill"),
                (p.amber_soft, p.amber_dim, "the scanning pill"),
                (p.red, p.panel, "an error on the status line"),
            ] {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.5, "{theme}: {what}: {ratio:.2}:1, need 4.5:1");
            }
        }
    }

    #[test]
    fn accents_stand_out_from_their_surfaces_in_both_themes() {
        for (theme, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            for (fg, bg, what) in [
                (p.accent, p.bg, "the accent on the window"),
                (p.accent, p.panel, "the accent on a panel"),
                (p.accent, p.card, "the accent on a card"),
                (p.amber, p.bg, "a bookmark on the window"),
                (p.amber, p.card, "a bookmark on a card"),
                (p.amber, p.panel, "a high-priority stripe"),
                (p.red, p.panel, "an urgent stripe"),
                (p.red, p.card_hi, "an urgent stripe on a hovered row"),
            ] {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 3.0, "{theme}: {what}: {ratio:.2}:1, need 3:1");
            }
        }
    }

    #[test]
    fn applying_a_theme_is_idempotent() {
        let ctx = egui::Context::default();
        for theme in Theme::ALL {
            apply(&ctx, theme);
            let first = ctx.style_of(theme.egui()).clone();
            apply(&ctx, theme);
            let second = ctx.style_of(theme.egui());
            assert_eq!(first.visuals.panel_fill, second.visuals.panel_fill, "{theme:?}");
            assert_eq!(first.text_styles, second.text_styles, "{theme:?}");
            assert_eq!(ctx.theme(), theme.egui(), "{theme:?}");
        }
    }

    /// The two palettes are real palettes, not the same one renamed: the light theme is light
    /// where the dark one is dark, and choosing one replaces the other in the context.
    #[test]
    fn the_light_theme_replaces_the_dark_one() {
        assert!(luminance(Palette::LIGHT.panel) > luminance(Palette::DARK.panel));
        assert!(luminance(Palette::LIGHT.panel) > 0.8, "the light panel must be near white");
        assert!(luminance(Palette::LIGHT.text) < 0.1, "light text must be near black");
        let ctx = egui::Context::default();
        apply(&ctx, Theme::Dark);
        assert_eq!(ctx.style_of(egui::Theme::Dark).visuals.panel_fill, Palette::DARK.panel);
        apply(&ctx, Theme::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(ctx.style_of(egui::Theme::Light).visuals.panel_fill, Palette::LIGHT.panel);
        // The frames read the current palette, not the one captured when the app started.
        assert_eq!(panel(), Palette::LIGHT.panel);
        assert_eq!(text(), Palette::LIGHT.text);
        apply(&ctx, Theme::Dark);
        assert_eq!(panel(), Palette::DARK.panel);
    }

    #[test]
    fn theme_codes_round_trip_and_unknown_codes_are_refused() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_code(theme.code()), Some(theme));
        }
        for unknown in ["", "sepia", "system", "high-contrast"] {
            assert_eq!(Theme::from_code(unknown), None, "{unknown}");
        }
    }
}
