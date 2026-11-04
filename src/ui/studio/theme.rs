//! Razdor Studio's look (UX1, spec §8 and §12): one token set (colours, sizes, spacing), applied to
//! egui's `Visuals` and `Style` in one place (`apply`), the UI font (Noto Sans Regular with the
//! Cyrillic letters) and the Phosphor icons (`install_fonts`). The values are the approved prototype's
//! (`ux_proto.rs`, module `tok`). The UI scale (80 to 150 %) is egui's zoom factor; a text token never
//! draws under 12 px on screen (`floor`).

use std::sync::atomic::{AtomicBool, Ordering};

use egui_macroquad::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2};

/// The window background behind the panels.
pub const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1b);
/// Panels, the top and status bars, the drawer, popups.
pub const PANEL: Color32 = Color32::from_rgb(0x1f, 0x22, 0x27);
/// Inactive widgets (buttons, fields).
pub const RAISED: Color32 = Color32::from_rgb(0x29, 0x2d, 0x34);
/// Hovered and active widgets.
pub const HOVER: Color32 = Color32::from_rgb(0x33, 0x38, 0x41);
/// Input wells (text fields, the field frames).
pub const WELL: Color32 = Color32::from_rgb(0x13, 0x15, 0x18);
/// Panel and widget strokes, frame borders.
pub const LINE: Color32 = Color32::from_rgb(0x35, 0x3a, 0x43);
/// Body text and active labels.
pub const TEXT: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xec);
/// Secondary text and icon hints.
pub const DIM: Color32 = Color32::from_rgb(0xa0, 0xa7, 0xb2);
/// Hovered borders, tertiary text, placeholders.
pub const FAINT: Color32 = Color32::from_rgb(0x6e, 0x75, 0x80);
/// The one accent: selection, the active tool, changed-field markers, primary buttons.
pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x8c, 0xff);
/// Errors and destructive confirmations.
pub const DANGER: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x6c);

/// Text sizes, in points at 100 %.
pub const TEXT_SIZE: f32 = 13.5;
pub const SMALL: f32 = 12.0;
pub const ICON: f32 = 16.0;
pub const HEADING: f32 = 16.0;
pub const MONO: f32 = 12.0;
/// The smallest text on screen, in pixels, at any scale.
pub const MIN_TEXT_PX: f32 = 12.0;

/// Sizes in points at 100 %.
pub const BTN_H: f32 = 26.0;
#[allow(dead_code)] // the Inspector and the Map (UX2, UX3)
pub const ROW_H: f32 = 24.0;
#[allow(dead_code)] // the Inspector and the Map (UX2, UX3)
pub const LABEL_W: f32 = 112.0;
pub const TOP_H: f32 = 36.0;
pub const STATUS_H: f32 = 24.0;
pub const DRAWER_OPEN_H: f32 = 186.0;
pub const DRAWER_CLOSED_H: f32 = 24.0;
#[allow(dead_code)] // the Inspector and the Map (UX2, UX3)
pub const TOOL_W: f32 = 44.0;
pub const PALETTE_W: f32 = 200.0;
pub const RIGHT_W: f32 = 304.0;
pub const INSPECTOR_W: f32 = 380.0;
pub const SMALL_BTN: f32 = 24.0;
pub const RADIUS: u8 = 4;
pub const WINDOW_RADIUS: u8 = 6;

/// The interface scales the setting offers, in per cent (10 % steps).
pub const SCALES: [u32; 8] = [80, 90, 100, 110, 120, 130, 140, 150];

/// The studio's scale in per cent: the nearest offered value (100 when none is set).
pub fn scale_pct(pct: u32) -> u32 {
    if pct == 0 {
        return 100;
    }
    *SCALES.iter().min_by_key(|s| s.abs_diff(pct)).unwrap_or(&100)
}

/// The egui zoom factor of a scale in per cent.
pub fn zoom(pct: u32) -> f32 {
    scale_pct(pct) as f32 / 100.0
}

/// A text size, raised so that it draws at least `MIN_TEXT_PX` on screen at this zoom: at 80 % the
/// 12 px token draws at 9.6 px, so it is taken as 15 points (12 ÷ 0.8).
pub fn floor(size: f32, zoom: f32) -> f32 {
    size.max(MIN_TEXT_PX / zoom)
}

/// The studio's UI font (Noto Sans Regular, subset with the Cyrillic letters; `data/fonts`).
const UI_FONT: &[u8] = include_bytes!("../../../data/fonts/NotoSans-Regular.ttf");

static FONTS_READY: AtomicBool = AtomicBool::new(false);

/// The studio's fonts: the UI font for proportional text, egui's Hack for code, and Phosphor's icons
/// as a fallback. egui's other default fonts (Ubuntu-Light, the emoji fonts) are not used.
pub fn font_definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    let hack = FontDefinitions::default().font_data.remove("Hack");
    defs.font_data.insert("noto-sans".into(), FontData::from_static(UI_FONT).into());
    defs.families.insert(FontFamily::Proportional, vec!["noto-sans".into()]);
    if let Some(hack) = hack {
        defs.font_data.insert("hack".into(), hack);
        defs.families.insert(FontFamily::Monospace, vec!["hack".into()]);
    } else {
        defs.families.insert(FontFamily::Monospace, vec!["noto-sans".into()]);
    }
    egui_phosphor::add_to_fonts(&mut defs, egui_phosphor::Variant::Regular);
    defs
}

/// Applies the fonts once (the egui context lives as long as the studio's window).
fn install_fonts(ctx: &egui::Context) {
    if !FONTS_READY.swap(true, Ordering::Relaxed) {
        ctx.set_fonts(font_definitions());
    }
}

/// The studio's look for this frame: the fonts once, the zoom for `scale_pct`, the visuals and the
/// style from the tokens. Called at the start of every studio frame.
pub fn apply(ctx: &egui::Context, scale: u32) {
    install_fonts(ctx);
    let z = zoom(scale);
    if (ctx.zoom_factor() - z).abs() > f32::EPSILON {
        ctx.set_zoom_factor(z);
    }
    ctx.style_mut(|s| {
        s.visuals = egui::Visuals::dark();
        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
        v.menu_corner_radius = CornerRadius::same(WINDOW_RADIUS);
        v.extreme_bg_color = WELL;
        v.faint_bg_color = RAISED;
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.error_fg_color = DANGER;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        let r = CornerRadius::same(RADIUS);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = r;
        }
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, FAINT);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.active.weak_bg_fill = HOVER;
        v.widgets.active.bg_fill = HOVER;
        v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.open.weak_bg_fill = RAISED;
        v.widgets.open.bg_fill = RAISED;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        s.spacing.item_spacing = Vec2::new(6.0, 4.0);
        s.spacing.button_padding = Vec2::new(8.0, 3.0);
        s.spacing.interact_size.y = BTN_H;
        s.spacing.window_margin = Margin::same(8);
        s.spacing.scroll.bar_width = 8.0;
        s.text_styles.insert(TextStyle::Body, FontId::proportional(floor(TEXT_SIZE, z)));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(floor(TEXT_SIZE, z)));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(floor(SMALL, z)));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(floor(HEADING, z)));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(floor(MONO, z)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_are_the_ten_per_cent_steps() {
        assert_eq!(SCALES, [80, 90, 100, 110, 120, 130, 140, 150]);
        assert_eq!(scale_pct(0), 100, "an unset scale is 100 %");
        assert_eq!(scale_pct(83), 80);
        assert_eq!(scale_pct(149), 150);
        assert_eq!(scale_pct(999), 150);
    }

    #[test]
    fn no_text_draws_under_twelve_pixels() {
        for pct in SCALES {
            let z = zoom(pct);
            for size in [TEXT_SIZE, SMALL, HEADING, MONO] {
                assert!(floor(size, z) * z >= MIN_TEXT_PX - 1e-4, "{size} at {pct} %");
            }
        }
        // At 80 % the small token is raised to 15 points, as the spec's section 12 says.
        assert!((floor(SMALL, zoom(80)) - 15.0).abs() < 1e-4);
        // At 100 % nothing changes.
        assert_eq!(floor(SMALL, 1.0), SMALL);
    }

    #[test]
    fn the_ui_font_has_the_russian_alphabet() {
        let face = ttf_parser_cmap_check();
        assert_eq!(face, 66, "the 66 Russian letters (U+0401, U+0410-044F, U+0451) are in the UI font");
    }

    /// Counts the Russian letters the bundled font maps, through the font's own cmap table.
    fn ttf_parser_cmap_check() -> usize {
        let letters: Vec<char> = ('\u{0410}'..='\u{044F}').chain(['\u{0401}', '\u{0451}']).collect();
        letters.iter().filter(|c| font_has_char(UI_FONT, **c)).count()
    }

    /// Whether the font's `cmap` (format 4 or 12 subtables) maps `c`. A small reader: the studio's
    /// fonts are plain TTFs (`glyf` outlines), so only the table directory and `cmap` are read.
    fn font_has_char(font: &[u8], c: char) -> bool {
        let be16 = |o: usize| u16::from_be_bytes([font[o], font[o + 1]]) as usize;
        let be32 = |o: usize| u32::from_be_bytes([font[o], font[o + 1], font[o + 2], font[o + 3]]) as usize;
        let n = be16(4);
        let Some(cmap) = (0..n).map(|i| 12 + i * 16).find(|&r| &font[r..r + 4] == b"cmap").map(|r| be32(r + 8)) else {
            return false;
        };
        let code = c as usize;
        let subs = be16(cmap + 2);
        for i in 0..subs {
            let rec = cmap + 4 + i * 8;
            if be16(rec) != 3 || be16(rec + 2) != 1 {
                continue;
            }
            let t = cmap + be32(rec + 4);
            if be16(t) != 4 {
                continue;
            }
            let seg2 = be16(t + 6);
            let ends = t + 14;
            let starts = ends + seg2 + 2;
            let deltas = starts + seg2;
            let ranges = deltas + seg2;
            for s in 0..seg2 / 2 {
                let end = be16(ends + s * 2);
                let start = be16(starts + s * 2);
                if code < start || code > end {
                    continue;
                }
                let ro = be16(ranges + s * 2);
                if ro == 0 {
                    return (code + be16(deltas + s * 2)) & 0xFFFF != 0;
                }
                let gi = be16(ranges + s * 2 + ro + (code - start) * 2);
                return gi != 0;
            }
        }
        false
    }

    #[test]
    fn phosphor_icons_are_in_the_fonts() {
        let defs = font_definitions();
        assert!(defs.font_data.contains_key("phosphor"), "the Phosphor icon font is registered");
        let prop = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        assert_eq!(prop.first().map(String::as_str), Some("noto-sans"), "the UI font comes first");
        assert!(prop.iter().any(|f| f == "phosphor"), "icons fall back to Phosphor");
    }
}
