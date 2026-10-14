//! The original's bitmap fonts (`Graphics/Windows/SanSerif.lit`, `Benguiat.lit`, …): glyph
//! sheets where each character sits in a blue box, in the order of the exe's character
//! string [`ORDER`]. The boxes give each glyph's cell; the capital H gives the baseline and
//! the capital height. Text is drawn scaled so that its capitals are as tall as the
//! TrueType font's would be at the same size, so the layouts keep working; a character the
//! sheet lacks (×, —, «…) is drawn with the TrueType font.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use macroquad::prelude::*;

use razdor::dt::gfx::Image;

/// The characters of every sheet, box by box (row by row, left to right): the exe's string.
/// After the space comes byte 1, a narrow empty box.
pub const ORDER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz1234567890\\=-+?!,.'\":;()<>/% \u{1}\
АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЬЫЪЭЮЯабвгдеёжзийклмнопрстуфхцчшщьыъэюя@";

/// Capital height of the TrueType font per pixel of font size (Liberation Sans, DejaVu Sans).
const TTF_CAP: f32 = 0.72;

/// Which of the original's fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// `SanSerif.lit`: most texts.
    Body,
    /// `Benguiat.lit`: window titles and names.
    Title,
    /// `SanSerif_Bold.lit`: the bar's time panel.
    Bold,
}

impl Face {
    fn file(self) -> &'static str {
        match self {
            Face::Body => "Windows/SanSerif.lit",
            Face::Title => "Windows/Benguiat.lit",
            Face::Bold => "Windows/SanSerif_Bold.lit",
        }
    }

    /// The sheet the original draws the face's shadow with, 1 px down and right (the window
    /// fonts' shadows ae24b0 / ae24d8, loaded by 4e5c9c and 4dbc26).
    fn shadow_file(self) -> &'static str {
        match self {
            Face::Body => "Windows/SanSerif_Shadow.lit",
            Face::Title => "Windows/Benguiat_Shadow.lit",
            Face::Bold => "Windows/SanSerif_Bold_Shadow.lit",
        }
    }
}

/// A glyph's cell in the sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The glyph boxes of a sheet in reading order (row by row, left to right): the inside of
/// every blue rectangle. A box's top-left corner is a blue pixel with blue to its right and
/// below it but not diagonally; its right side is the next blue column below the top line,
/// its bottom the next blue row right of the left line.
pub fn glyph_boxes(img: &Image) -> Vec<Glyph> {
    let (w, h) = (img.width, img.height);
    let blue = |x: u32, y: u32| {
        if x >= w || y >= h {
            return false;
        }
        let [r, g, b, _] = img.pixel(x, y);
        b > 150 && r < 100 && g < 100
    };
    let mut boxes = Vec::new();
    for y in 0..h.saturating_sub(2) {
        for x in 0..w.saturating_sub(2) {
            if !(blue(x, y) && blue(x + 1, y) && blue(x, y + 1) && !blue(x + 1, y + 1)) {
                continue;
            }
            let Some(right) = (x + 1..w).take_while(|&xx| blue(xx, y)).find(|&xx| blue(xx, y + 1)) else { continue };
            let Some(bottom) = (y + 1..h).take_while(|&yy| blue(x, yy)).find(|&yy| blue(x + 1, yy)) else { continue };
            if right > x + 1 && bottom > y + 1 {
                boxes.push(Glyph { x: x + 1, y: y + 1, w: right - x - 1, h: bottom - y - 1 });
            }
        }
    }
    boxes.sort_by_key(|g| (g.y, g.x));
    boxes
}

/// The pixel-sharp scale for an exact one (`BitmapFont::scale`): the nearest whole number
/// when it changes the size by little (at most 15% smaller or 12% bigger), else the exact
/// scale, drawn smoothed (snapping further down halved text shrunk a little to fit).
fn whole_scale(raw: f32) -> f32 {
    let near = raw.round();
    if near >= 1.0 && near >= raw * 0.85 && near <= raw * 1.12 { near } else { raw }
}

/// A sheet turned into a font: white glyphs whose alpha is their brightness, the blue boxes
/// cleared.
pub struct BitmapFont {
    /// The sheet drawn without smoothing (whole-number scales) and with it (a fractional
    /// scale).
    texture: Texture2D,
    smooth: Texture2D,
    glyphs: HashMap<char, Glyph>,
    /// Rows from a cell's top to the baseline, and the capital height (pixels of the sheet).
    baseline: f32,
    cap: f32,
}

/// Rows `(top, bottom)` of a glyph's cell that hold ink (brightness over a half).
fn ink_rows(img: &Image, g: Glyph) -> Option<(u32, u32)> {
    let rows: Vec<u32> = (0..g.h).filter(|&r| (0..g.w).any(|c| img.pixel(g.x + c, g.y + r)[..3].iter().max().copied().unwrap_or(0) > 128)).collect();
    Some((*rows.first()?, *rows.last()?))
}

impl BitmapFont {
    /// The font of a decoded sheet, if it has one box per character of [`ORDER`].
    pub fn from_sheet(mut img: Image) -> Option<BitmapFont> {
        let boxes = glyph_boxes(&img);
        let chars: Vec<char> = ORDER.chars().collect();
        if boxes.len() != chars.len() {
            razdor::diag!("Discord Times font: {} glyph boxes, expected {}", boxes.len(), chars.len());
            return None;
        }
        let glyphs: HashMap<char, Glyph> = chars.into_iter().zip(boxes).collect();
        let (top, bottom) = ink_rows(&img, glyphs[&'H'])?;
        let (baseline, cap) = ((bottom + 1) as f32, (bottom + 1 - top) as f32);
        for p in img.rgba.chunks_exact_mut(4) {
            let blue = p[2] > 150 && p[0] < 100 && p[1] < 100;
            let m = if blue { 0 } else { p[0].max(p[1]).max(p[2]) };
            p.copy_from_slice(&[255, 255, 255, m]);
        }
        let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
        // Pixel-sharp: whole-number scales (`scale`) drawn without smoothing.
        let texture = Texture2D::from_rgba8(w, h, &img.rgba);
        texture.set_filter(FilterMode::Nearest);
        let smooth = Texture2D::from_rgba8(w, h, &img.rgba);
        smooth.set_filter(FilterMode::Linear);
        Some(BitmapFont { texture, smooth, glyphs, baseline, cap })
    }

    /// Screen pixels per sheet pixel at font size `size`: a whole number near what makes the
    /// capitals as tall as the TrueType font's ([`whole_scale`]), so every sheet pixel becomes
    /// a square of screen pixels; a scale far from a whole number stays as it is, smoothed.
    fn scale(&self, size: f32) -> f32 {
        whole_scale(size * TTF_CAP / self.cap)
    }

    /// Width of `s` at `size`, with `fallback` giving the width of a character not in the sheet.
    pub fn width(&self, s: &str, size: f32, fallback: &dyn Fn(char) -> f32) -> f32 {
        let k = self.scale(size);
        s.chars().map(|c| self.glyphs.get(&c).map_or_else(|| fallback(c), |g| g.w as f32 * k)).sum()
    }

    /// Capital height at `size` (what `TextDimensions::offset_y` gives for centring).
    pub fn cap_height(&self, size: f32) -> f32 {
        self.cap * self.scale(size)
    }

    /// Draws `s` with its baseline at `y`; `fallback` draws a character not in the sheet and
    /// returns its width.
    pub fn draw(&self, s: &str, x: f32, y: f32, size: f32, color: Color, fallback: &dyn Fn(char, f32, f32) -> f32) {
        self.draw_cells(self, s, vec2(x, y), size, (color, 0.0), fallback);
    }

    /// Draws `s`'s cells from `sheet` (this font or its shadow sheet, whose boxes stand at the
    /// same places) from `at` on this font's pen positions, in `color`, `offset` sheet pixels
    /// down and right. The pen and the cell tops sit on whole screen pixels.
    fn draw_cells(&self, sheet: &BitmapFont, s: &str, at: Vec2, size: f32, (color, offset): (Color, f32), fallback: &dyn Fn(char, f32, f32) -> f32) {
        let (x, y) = (at.x, at.y);
        let k = self.scale(size);
        let texture = if k.fract() == 0.0 { &sheet.texture } else { &sheet.smooth };
        let mut pen = x.round();
        let top = (y - self.baseline * k).round() + offset * k;
        for c in s.chars() {
            match (self.glyphs.get(&c), sheet.glyphs.get(&c)) {
                (Some(g), Some(cell)) => {
                    if c != ' ' {
                        let src = Rect::new(cell.x as f32, cell.y as f32, cell.w as f32, cell.h as f32);
                        let dest = vec2(cell.w as f32 * k, cell.h as f32 * k);
                        draw_texture_ex(texture, pen + offset * k, top, color, DrawTextureParams { dest_size: Some(dest), source: Some(src), ..Default::default() });
                    }
                    pen += g.w as f32 * k;
                }
                (Some(g), None) => pen += g.w as f32 * k,
                (None, _) => pen += fallback(c, pen, y),
            }
        }
    }
}

thread_local! {
    static FONTS: RefCell<HashMap<Face, Option<BitmapFont>>> = RefCell::new(HashMap::new());
    static SHADOWS: RefCell<HashMap<Face, Option<BitmapFont>>> = RefCell::new(HashMap::new());
    static FACE: Cell<Face> = const { Cell::new(Face::Body) };
}

/// Runs `f` with texts drawn in `face`.
pub fn with_face<R>(face: Face, f: impl FnOnce() -> R) -> R {
    let before = FACE.with(|c| c.replace(face));
    let r = f();
    FACE.with(|c| c.set(before));
    r
}

/// The TrueType font of the current face: only a `RAZDOR_FONT*` file; the install's sheets
/// otherwise.
pub fn current_ttf() -> Option<Font> {
    let i = match FACE.with(|c| c.get()) {
        Face::Body => 0,
        Face::Bold => 1,
        Face::Title => 2,
    };
    super::widgets::face_font(i)
}

/// Runs `f` with the current face's bitmap font, if the install has it and no TrueType
/// stand-in loaded for the face.
pub fn with_current<R>(f: impl FnOnce(&BitmapFont) -> R) -> Option<R> {
    if current_ttf().is_some() {
        return None;
    }
    let face = FACE.with(|c| c.get());
    FONTS.with(|fonts| {
        let mut fonts = fonts.borrow_mut();
        let font = fonts.entry(face).or_insert_with(|| super::chrome::image(face.file()).and_then(BitmapFont::from_sheet));
        font.as_ref().map(f)
    })
}

/// Draws `s`'s shadow as the original does: the face's shadow sheet 1 px (of the sheet)
/// down and right, darkening what is under it. False when the text is not drawn from a
/// sheet or the install has no shadow sheet for it.
pub fn draw_shadow(s: &str, x: f32, y: f32, size: f32, alpha: f32) -> bool {
    let face = FACE.with(|c| c.get());
    with_current(|font| {
        SHADOWS.with(|shadows| {
            let mut shadows = shadows.borrow_mut();
            let shadow = shadows.entry(face).or_insert_with(|| super::chrome::image(face.shadow_file()).and_then(BitmapFont::from_sheet));
            let Some(shadow) = shadow.as_ref() else { return false };
            font.draw_cells(shadow, s, vec2(x, y), size, (Color::new(0.0, 0.0, 0.0, alpha), 1.0), &|c, _, _| {
                super::widgets::measure_ttf(c.encode_utf8(&mut [0; 4]), size).width
            });
            true
        })
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_scale_by_whole_numbers_only_near_them() {
        assert_eq!(whole_scale(1.0), 1.0);
        assert_eq!(whole_scale(1.1), 1.0);
        assert_eq!(whole_scale(1.95), 2.0);
        assert_eq!(whole_scale(2.8), 3.0);
        // Far from a whole number: its own scale, never halved.
        assert_eq!(whole_scale(1.6), 1.6);
        assert_eq!(whole_scale(1.3), 1.3);
        // Under 1 (shrunk to fit): its own scale.
        assert_eq!(whole_scale(0.8), 0.8);
        for raw in [0.5_f32, 0.9, 1.1, 1.3, 1.5, 1.6, 2.2, 2.45, 3.4] {
            let k = whole_scale(raw);
            assert!(k <= raw * 1.12 + 1e-6 && k >= raw * 0.85 - 1e-6, "{raw} -> {k}");
        }
    }

    #[test]
    fn every_face_has_a_shadow_sheet_of_the_original() {
        for face in [Face::Body, Face::Bold, Face::Title] {
            assert!(face.shadow_file().ends_with("_Shadow.lit"), "{face:?}");
            assert!(face.shadow_file().starts_with(face.file().trim_end_matches(".lit")), "{face:?}");
        }
    }

    /// A sheet of `boxes` blue rectangles of the given inner widths, in one row of height 5
    /// (inside), sharing their borders.
    fn sheet(widths: &[u32]) -> Image {
        let w = widths.iter().map(|w| w + 1).sum::<u32>() + 1;
        let h = 7;
        let mut img = Image { width: w, height: h, rgba: vec![0; (w * h * 4) as usize] };
        let mut set = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            img.rgba[i..i + 4].copy_from_slice(&[0, 0, 255, 255]);
        };
        let mut x = 0;
        for (i, &bw) in widths.iter().enumerate() {
            for xx in x..=x + bw + 1 {
                set(xx, 0);
                set(xx, h - 1);
            }
            for y in 0..h {
                set(x, y);
                if i + 1 == widths.len() {
                    set(x + bw + 1, y);
                }
            }
            x += bw + 1;
        }
        img
    }

    #[test]
    fn boxes_are_found_in_reading_order() {
        let img = sheet(&[3, 5, 2]);
        let b = glyph_boxes(&img);
        assert_eq!(b, vec![Glyph { x: 1, y: 1, w: 3, h: 5 }, Glyph { x: 5, y: 1, w: 5, h: 5 }, Glyph { x: 11, y: 1, w: 2, h: 5 }]);
    }

    #[test]
    fn the_order_has_one_character_per_box_of_the_originals_sheets() {
        assert_eq!(ORDER.chars().count(), 149);
        assert_eq!(ORDER.chars().filter(|&c| c == ' ').count(), 1);
    }
}
