//! The main hall's quest and rumour list drawn on the CPU as the original draws it (0x4bedec,
//! 0x4d9060 with text mode 5, 0x476790 with its flag set): the paper, its ornaments and grey
//! bands are taken off the 5-6-5 screen, then each entry's glyphs are taken off it, so a
//! quest's ink (the yellow font) and a rumour's (the green one) come out of the original's
//! table arithmetic and not out of a clean GPU subtract. The panel is built at the original's
//! 1x pixel scale; the caller shows it scaled by the interface scale.
//!
//! The glyphs come from the install's Benguiat sheet. A glyph pixel of level g is the font's
//! colour: g less the font's offset per channel (0,0,100 for the yellow 0xae24a0, 100,0,100
//! for the green 0xae249c), packed to 5-6-5. Level 0 is transparent, as the original's
//! mode-1 blit skips zero source pixels.

use super::dt_font::BitmapFont;
use super::subtract16::{pack565, sub565, unpack565};
use razdor::dt::gfx::Image;

/// A list row's height at 1x (the original's 20 px rows, 0x47e46c).
pub const ROW_H: f32 = 20.0;

/// The highlight a picked row carries (the GPU path's red at 34%), as blended on the paper.
const HIGHLIGHT: ([f32; 3], f32) = ([184.0, 18.0, 10.0], 0.34);

/// The grey band the paper loses at its edge (48 per channel, 0x4768f8).
const BAND_GREY: u8 = 48;

/// One list row at 1x.
pub struct Row<'a> {
    /// The row's top, in the panel's pixels.
    pub top: f32,
    /// The row is the picked one (highlighted under its text).
    pub picked: bool,
    /// The text drawn on the panel, with the font's offset from the glyph level (see the
    /// module docs); `None` when the row's text is drawn elsewhere.
    pub text: Option<(&'a str, [u8; 3])>,
}

/// The panel's content: its size, the paper (a tile from its top-left), the two side
/// ornaments already adjusted as the original loads them, the row width and the rows.
pub struct Panel<'a> {
    pub w: u32,
    pub h: u32,
    pub paper: &'a Image,
    /// Left and right ornaments (`None` when the panel is too narrow for them, as the original
    /// draws none).
    pub sides: Option<[&'a Image; 2]>,
    /// The width of a row's highlight and the text's box (the list less its scroll bar).
    pub row_w: f32,
    pub rows: Vec<Row<'a>>,
}

/// Width of `text` at 1x in the sheet's pixels.
pub fn text_width(sheet: &BitmapFont, text: &str) -> f32 {
    text.chars().filter_map(|c| sheet.cell(c)).map(|g| g.w as f32).sum()
}

/// Draws the panel as a 1x RGBA image: the paper, the ornaments, the grey bands, the picked
/// row's highlight, then the glyphs of every row over them, centred in `row_w`.
pub fn draw(panel: &Panel, sheet: &BitmapFont) -> Image {
    let (w, h) = (panel.w as usize, panel.h as usize);
    let mut px = vec![0u16; w * h];
    tile_paper(&mut px, w, h, panel.paper);
    if let Some(sides) = panel.sides {
        // Repeated down from the top (0x48e4e8), each copy as tall as the art.
        for (img, x) in [(sides[0], 4), (sides[1], w as i64 - sides[1].width as i64 - 4)] {
            let step = (img.height as i64).max(1);
            for y in (0..h as i64).step_by(step as usize) {
                take_image(&mut px, w, h, img, x, y);
            }
        }
    }
    band_edges(&mut px, w, h);
    for row in panel.rows.iter().filter(|r| r.picked) {
        highlight(&mut px, w, h, panel.row_w, row.top);
    }
    for row in &panel.rows {
        if let Some((text, offset)) = row.text {
            glyphs(&mut px, w, sheet, text, offset, row.top, panel.row_w);
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in px {
        let [r, g, b] = unpack565(p);
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    Image { width: w as u32, height: h as u32, rgba }
}

fn tile_paper(px: &mut [u16], w: usize, h: usize, paper: &Image) {
    let (pw, ph) = (paper.width as usize, paper.height as usize);
    if pw == 0 || ph == 0 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let p = &paper.rgba[((y % ph) * pw + x % pw) * 4..];
            px[y * w + x] = pack565(p[0], p[1], p[2]);
        }
    }
}

/// Takes the non-zero pixels of `img` off the panel at (x, y), clipped to it.
fn take_image(px: &mut [u16], w: usize, h: usize, img: &Image, x: i64, y: i64) {
    let (iw, ih) = (img.width as usize, img.height as usize);
    for iy in 0..ih {
        let dy = y + iy as i64;
        if dy < 0 || dy >= h as i64 {
            continue;
        }
        for ix in 0..iw {
            let dx = x + ix as i64;
            if dx < 0 || dx >= w as i64 {
                continue;
            }
            let p = &img.rgba[(iy * iw + ix) * 4..];
            let src = pack565(p[0], p[1], p[2]);
            let at = dy as usize * w + dx as usize;
            px[at] = sub565(px[at], src);
        }
    }
}

/// The grey bands off the paper's edge: 48 off the outer 2 px and again off the 4 px ring
/// (0x4bef1b..0x4bf00f), so the outer 2 px lose two greys.
fn band_edges(px: &mut [u16], w: usize, h: usize) {
    let grey = pack565(BAND_GREY, BAND_GREY, BAND_GREY);
    for band in [2usize, 4] {
        for y in 0..h {
            for x in 0..w {
                if x.min(y).min(w - 1 - x).min(h - 1 - y) < band.min(w / 2).min(h / 2) {
                    px[y * w + x] = sub565(px[y * w + x], grey);
                }
            }
        }
    }
}

/// The picked row's highlight: the GPU path's red blended over the paper, before its text.
fn highlight(px: &mut [u16], w: usize, h: usize, row_w: f32, top: f32) {
    let (c, a) = HIGHLIGHT;
    let y0 = top.round().max(0.0) as usize;
    for y in y0..(y0 + ROW_H as usize).min(h) {
        for x in 0..(row_w.round().max(0.0) as usize).min(w) {
            let [r, g, b] = unpack565(px[y * w + x]);
            let mix = |d: u8, s: f32| (d as f32 * (1.0 - a) + s * a) as u8;
            px[y * w + x] = pack565(mix(r, c[0]), mix(g, c[1]), mix(b, c[2]));
        }
    }
}

/// One row's text, centred in `row_w`, its baseline at `row.top` plus the row's half height
/// and the original's text drop (5.76, 16 × 0.36).
fn glyphs(px: &mut [u16], w: usize, sheet: &BitmapFont, text: &str, offset: [u8; 3], row_top: f32, row_w: f32) {
    let total = text_width(sheet, text);
    let baseline = row_top + ROW_H / 2.0 + 16.0 * 0.36;
    let mut pen = (row_w / 2.0 - total / 2.0).round();
    let top = (baseline - sheet.baseline()).round();
    let h = px.len() / w;
    for c in text.chars() {
        let Some(cell) = sheet.cell(c) else { continue };
        if c != ' ' {
            for cy in 0..cell.h {
                for cx in 0..cell.w {
                    let level = sheet.grey_at(cell.x + cx, cell.y + cy);
                    if level == 0 {
                        continue;
                    }
                    let [dr, dg, db] = offset;
                    let ink = pack565(level.saturating_sub(dr), level.saturating_sub(dg), level.saturating_sub(db));
                    let (x, y) = (pen as i64 + cx as i64, top as i64 + cy as i64);
                    if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                        continue;
                    }
                    let at = y as usize * w + x as usize;
                    px[at] = sub565(px[at], ink);
                }
            }
        }
        pen += cell.w as f32;
    }
}
