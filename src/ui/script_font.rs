//! Text in any script (a Razdor feature, not the original's: its fonts hold Latin and
//! Cyrillic only). A character the current font lacks comes from a fallback chain: PT Sans,
//! the bundled Noto Sans (Latin, Greek, Cyrillic extended) and Noto Sans Devanagari, then a
//! system font found at run time (fontconfig on Linux, the Windows and macOS font folders)
//! for anything else, e.g. Chinese and Japanese. Text the current font covers takes no
//! detour (`runs` gives `None`), so Russian and English look exactly as before.
//!
//! A run of fallback characters is shaped with rustybuzz (Devanagari's conjuncts and its
//! vowel sign ि before its consonant, Arabic's joining) and its glyphs are rasterised by
//! index into cached textures, laid on the same baseline at the same pixel size as the
//! main font, with the same `dpi_scale` handling as macroquad's own text.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ops::{Range, RangeInclusive};
use std::path::PathBuf;
use std::rc::Rc;

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use macroquad::prelude::*;
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};
use rustybuzz::{BufferFlags, Face, UnicodeBuffer};

/// Noto Sans Regular (OFL, `data/fonts/Noto-OFL.txt`): Latin extended, Greek, Cyrillic
/// extended, general punctuation; subset.
const NOTO_SANS: &[u8] = include_bytes!("../../data/fonts/NotoSans-Regular.ttf");
/// Noto Sans Devanagari Regular (OFL, `data/fonts/Noto-OFL.txt`), with its shaping tables.
const NOTO_DEVANAGARI: &[u8] = include_bytes!("../../data/fonts/NotoSansDevanagari-Regular.ttf");

/// Which font draws a run: the current font as macroquad draws it, or font `n` (of the
/// chain, or the current one when its run must be shaped) shaped here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Primary,
    Fallback(usize),
}

/// A font's character coverage: a bitmap below U+0800 (Latin, Greek, Cyrillic: nearly all
/// text, checked every frame), the cmap above.
struct Cover {
    face: Face<'static>,
    low: [u64; 32],
}

impl Cover {
    /// The coverage of font `index` of `data`, less the characters in `wrong` (below
    /// U+0800: glyphs a font maps there but draws as something else, `widgets::wrong_glyphs`).
    fn new(data: &'static [u8], index: u32, wrong: &[RangeInclusive<char>]) -> Option<Cover> {
        let face = Face::from_slice(data, index)?;
        let mut low = [0u64; 32];
        for u in 0..0x800u32 {
            if char::from_u32(u).is_some_and(|c| has_glyph(&face, c) && !wrong.iter().any(|r| r.contains(&c))) {
                low[(u / 64) as usize] |= 1 << (u % 64);
            }
        }
        Some(Cover { face, low })
    }

    fn covers(&self, c: char) -> bool {
        let u = c as u32;
        if u < 0x800 { self.low[(u / 64) as usize] & (1 << (u % 64)) != 0 } else { has_glyph(&self.face, c) }
    }
}

/// Whether `face` draws `c`: it maps it to a glyph with an outline (a space, a joiner or
/// another invisible character needs none). Some fonts map characters to empty glyphs
/// (Becker's «, », —).
fn has_glyph(face: &Face, c: char) -> bool {
    let Some(g) = face.glyph_index(c).filter(|g| g.0 != 0) else { return false };
    c.is_whitespace() || invisible(c) || face.glyph_bounding_box(g).is_some()
}

/// Characters drawn as nothing: soft hyphen, zero-width spaces and joiners, marks of
/// direction, the byte order mark.
fn invisible(c: char) -> bool {
    matches!(c as u32, 0x00AD | 0x034F | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF)
}

/// Characters of scripts whose letters change shape or place by their neighbours
/// (Indic scripts, Arabic, Syriac, Thaana, Thai, Lao, Tibetan, Myanmar, Khmer, Mongolian):
/// a run of them is always shaped, even in the current font.
pub fn complex(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x08FF | 0x0900..=0x0DFF | 0x0E00..=0x0FFF | 0x1000..=0x109F | 0x1780..=0x18AF | 0xA8E0..=0xA8FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
    )
}

/// Characters that never send a text to another font: controls (a font has no glyph for
/// them, and drawing them is the caller's business).
fn ignorable(c: char) -> bool {
    (c as u32) < 0x20 || c == '\u{7f}'
}

/// Characters that stay in the run before them when its font has them: spaces (so a
/// sentence in Hindi or Arabic is shaped as one run, words in their order), joiners,
/// combining marks and variation selectors (they belong to the letter before).
fn sticky(c: char) -> bool {
    c.is_whitespace() || matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C..=0x200D | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
}

/// Splits `s` into runs by font: `covers(pick, c)` says whether a font has `c`, `pick_for(c)`
/// picks the font for a character that starts a new run (the first of the chain that has
/// it). Neighbouring characters of one font form one run.
pub fn split(s: &str, covers: impl Fn(Pick, char) -> bool, mut pick_for: impl FnMut(char) -> Pick) -> Vec<(Range<usize>, Pick)> {
    let mut runs: Vec<(Range<usize>, Pick)> = Vec::new();
    for (i, c) in s.char_indices() {
        let end = i + c.len_utf8();
        let pick = match runs.last() {
            Some(&(_, last)) if ignorable(c) || (sticky(c) && covers(last, c)) => last,
            _ if ignorable(c) => Pick::Primary,
            _ => pick_for(c),
        };
        match runs.last_mut() {
            Some((r, last)) if *last == pick => r.end = end,
            _ => runs.push((i..end, pick)),
        }
    }
    runs
}

/// The first font of the chain (the current one first) that has `c` (what `Chain` does, over
/// coverage functions).
#[cfg(test)]
pub fn first_covering(c: char, primary: impl Fn(char) -> bool, fallbacks: &[&dyn Fn(char) -> bool]) -> Option<Pick> {
    if primary(c) {
        return Some(Pick::Primary);
    }
    fallbacks.iter().position(|f| f(c)).map(Pick::Fallback)
}

/// A glyph of a shaped run: its id and pen position (raster pixels, y up).
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub glyph: u16,
    /// The text's character the glyph comes from (its byte offset).
    #[cfg_attr(not(test), allow(dead_code))]
    pub cluster: u32,
    pub x: f32,
    pub y: f32,
}

/// A shaped run at one raster size: its glyphs left to right, its advance, and how far its
/// ink reaches above and below the baseline (raster pixels).
#[derive(Clone, Debug)]
pub struct Shaped {
    pub glyphs: Vec<Placed>,
    pub advance: f32,
    pub top: f32,
    pub bottom: f32,
}

/// Shapes `s` in `face` at `px` pixels per em: script, direction and language guessed from
/// the text; no dotted circle for a mark that stands alone (a run split mid-word measures
/// as its letters, not with a placeholder).
pub fn shape(face: &Face, s: &str, px: f32) -> Shaped {
    let mut buf = UnicodeBuffer::new();
    buf.push_str(s);
    buf.guess_segment_properties();
    buf.set_flags(BufferFlags::DO_NOT_INSERT_DOTTED_CIRCLE);
    let out = rustybuzz::shape(face, &[], buf);
    let k = px / face.units_per_em().max(1) as f32;
    let (mut pen, mut top, mut bottom) = (0.0f32, 0.0f32, 0.0f32);
    let mut glyphs = Vec::with_capacity(out.len());
    for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
        let glyph = info.glyph_id as u16;
        let (x, y) = (pen + pos.x_offset as f32 * k, pos.y_offset as f32 * k);
        if let Some(bb) = face.glyph_bounding_box(GlyphId(glyph)) {
            top = top.max(y + bb.y_max as f32 * k);
            bottom = bottom.min(y + bb.y_min as f32 * k);
        }
        glyphs.push(Placed { glyph, cluster: info.cluster, x, y });
        pen += pos.x_advance as f32 * k;
    }
    Shaped { glyphs, advance: pen, top, bottom }
}

/// A glyph rasterised: its texture and where its top-left corner stands from the pen on the
/// baseline (raster pixels, y down).
struct GlyphTex {
    texture: Texture2D,
    left: f32,
    top: f32,
    w: f32,
    h: f32,
}

/// Feeds a glyph's outline (font units, y up) to the rasteriser (pixels, y down).
struct Outline<'a> {
    r: &'a mut Rasterizer,
    k: f32,
    x0: f32,
    y1: f32,
    start: Point,
    last: Point,
}

impl Outline<'_> {
    fn at(&self, x: f32, y: f32) -> Point {
        point(x * self.k - self.x0, self.y1 - y * self.k)
    }
}

impl OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.at(x, y);
        self.last = self.start;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.r.draw_line(self.last, p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.at(x1, y1), self.at(x, y));
        self.r.draw_quad(self.last, c, p);
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.r.draw_cubic(self.last, c1, c2, p);
        self.last = p;
    }
    fn close(&mut self) {
        if self.last != self.start {
            self.r.draw_line(self.last, self.start);
        }
        self.last = self.start;
    }
}

/// The coverage of glyph `glyph` of `face` at `px` pixels per em, with its box's left and
/// top edges (pixels from the pen, y up) and size; `None` for a glyph without an outline.
pub fn rasterize(face: &Face, glyph: u16, px: f32) -> Option<(Vec<u8>, f32, f32, usize, usize)> {
    let bb = face.glyph_bounding_box(GlyphId(glyph))?;
    let k = px / face.units_per_em().max(1) as f32;
    let (x0, x1) = ((bb.x_min as f32 * k).floor(), (bb.x_max as f32 * k).ceil());
    let (y0, y1) = ((bb.y_min as f32 * k).floor(), (bb.y_max as f32 * k).ceil());
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    if w == 0 || h == 0 || w > 2048 || h > 2048 {
        return None;
    }
    let mut r = Rasterizer::new(w, h);
    let mut outline = Outline { r: &mut r, k, x0, y1, start: point(0.0, 0.0), last: point(0.0, 0.0) };
    face.outline_glyph(GlyphId(glyph), &mut outline)?;
    let mut alpha = vec![0u8; w * h];
    r.for_each_pixel(|i, a| alpha[i] = (a.clamp(0.0, 1.0) * 255.0).round() as u8);
    Some((alpha, x0, y1, w, h))
}

/// A font Razdor shapes text in: one of the chain, or a current font that has a complex
/// script's letters.
struct Loaded {
    /// Where it came from (a bundled name or a system file), for the log.
    name: String,
    cover: Cover,
}

/// The fonts, the fallback chain's order, the current fonts' coverage, and the shaped runs
/// and glyph textures.
struct Chain {
    /// Every font loaded; `Pick::Fallback` indexes this.
    fonts: Vec<Loaded>,
    /// The fallback chain: indices into `fonts`, in order.
    order: Vec<usize>,
    /// The current fonts, by their data's address: their index in `fonts`.
    primaries: HashMap<usize, Option<usize>>,
    /// System font files loaded (or found unreadable).
    files: HashSet<PathBuf>,
    /// Characters the system was searched for already.
    searched: HashSet<char>,
    shaped: HashMap<(usize, u16, String), Rc<Shaped>>,
    glyphs: HashMap<(usize, u16, u16), Option<Rc<GlyphTex>>>,
}

impl Chain {
    fn new() -> Chain {
        let mut chain = Chain {
            fonts: Vec::new(),
            order: Vec::new(),
            primaries: HashMap::new(),
            files: HashSet::new(),
            searched: HashSet::new(),
            shaped: HashMap::new(),
            glyphs: HashMap::new(),
        };
        let bundled: [(&str, &'static [u8]); 3] =
            [("PT Sans", super::widgets::FALLBACK_FILE), ("Noto Sans", NOTO_SANS), ("Noto Sans Devanagari", NOTO_DEVANAGARI)];
        for (name, data) in bundled {
            if let Some(cover) = Cover::new(data, 0, &[]) {
                chain.fonts.push(Loaded { name: name.into(), cover });
                chain.order.push(chain.fonts.len() - 1);
            }
        }
        chain
    }

    /// The current font of file `data`, loaded once.
    fn primary(&mut self, data: &'static [u8]) -> Option<usize> {
        let key = data.as_ptr() as usize;
        if let Some(&i) = self.primaries.get(&key) {
            return i;
        }
        let i = Cover::new(data, 0, super::widgets::wrong_glyphs(data)).map(|cover| {
            self.fonts.push(Loaded { name: "the current font".into(), cover });
            self.fonts.len() - 1
        });
        self.primaries.insert(key, i);
        i
    }

    fn covers(&self, font: usize, c: char) -> bool {
        self.fonts.get(font).is_some_and(|f| f.cover.covers(c))
    }

    /// The chain's first font that has `c`, searching the system's fonts once per character
    /// the bundled ones lack.
    fn fallback_for(&mut self, c: char) -> Option<usize> {
        if let Some(&i) = self.order.iter().find(|&&i| self.fonts[i].cover.covers(c)) {
            return Some(i);
        }
        if !self.searched.insert(c) {
            return None;
        }
        for (path, index) in system_candidates(c) {
            if !self.files.insert(path.clone()) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            let data: &'static [u8] = Box::leak(data.into_boxed_slice());
            let Some(cover) = Cover::new(data, index, &[]) else { continue };
            razdor::diag!("system font for U+{:04X}: {}", c as u32, path.display());
            let covers = cover.covers(c);
            self.fonts.push(Loaded { name: path.display().to_string(), cover });
            self.order.push(self.fonts.len() - 1);
            if covers {
                return Some(self.fonts.len() - 1);
            }
        }
        None
    }

    fn shaped(&mut self, font: usize, s: &str, px: u16) -> Rc<Shaped> {
        if self.shaped.len() > 4096 {
            self.shaped.clear();
        }
        let face = &self.fonts[font].cover.face;
        self.shaped.entry((font, px, s.to_string())).or_insert_with(|| Rc::new(shape(face, s, px as f32))).clone()
    }

    fn glyph(&mut self, font: usize, glyph: u16, px: u16) -> Option<Rc<GlyphTex>> {
        if self.glyphs.len() > 8192 {
            self.glyphs.clear();
        }
        let face = &self.fonts[font].cover.face;
        self.glyphs
            .entry((font, glyph, px))
            .or_insert_with(|| {
                let (alpha, left, top, w, h) = rasterize(face, glyph, px as f32)?;
                let rgba: Vec<u8> = alpha.iter().flat_map(|&a| [255, 255, 255, a]).collect();
                let texture = Texture2D::from_rgba8(w as u16, h as u16, &rgba);
                texture.set_filter(FilterMode::Linear);
                Some(Rc::new(GlyphTex { texture, left, top, w: w as f32, h: h as f32 }))
            })
            .clone()
    }
}

thread_local! {
    static CHAIN: RefCell<Option<Chain>> = const { RefCell::new(None) };
}

fn with_chain<R>(f: impl FnOnce(&mut Chain) -> R) -> R {
    CHAIN.with(|c| f(c.borrow_mut().get_or_insert_with(Chain::new)))
}

/// The runs of `s` in the current font (`primary`, its file's data) and the fallback chain;
/// `None` when the current font draws all of it as is (the common case). A complex script's
/// run is shaped (`Pick::Fallback`) even in the current font.
pub fn runs(primary: &'static [u8], s: &str) -> Option<Vec<(Range<usize>, Pick)>> {
    with_chain(|chain| {
        let main = chain.primary(primary)?;
        if s.chars().all(|c| ignorable(c) || (chain.covers(main, c) && !complex(c))) {
            return None;
        }
        // Both closures below need the chain, one at a time (the system search adds fonts).
        let chain = RefCell::new(chain);
        let covers = |pick: Pick, c: char| match pick {
            Pick::Primary => chain.borrow().covers(main, c) && !complex(c),
            Pick::Fallback(i) => chain.borrow().covers(i, c),
        };
        let pick_for = |c: char| {
            let mut chain = chain.borrow_mut();
            if chain.covers(main, c) {
                return if complex(c) { Pick::Fallback(main) } else { Pick::Primary };
            }
            chain.fallback_for(c).map_or(Pick::Primary, Pick::Fallback)
        };
        Some(split(s, covers, pick_for))
    })
}


/// The raster size for a text size: macroquad's own (`text::draw_text_ex`), so a fallback run
/// is as big as the main font's beside it on any display.
fn raster(size: u16) -> (u16, f32) {
    let dpi = macroquad::miniquad::window::dpi_scale();
    ((size as f32 * dpi).ceil() as u16, dpi)
}

/// Size of `s` in fallback font `fallback` at `size`, as `measure_text` gives it.
pub fn measure(fallback: usize, s: &str, size: u16) -> TextDimensions {
    let (px, dpi) = raster(size);
    let shaped = with_chain(|chain| chain.shaped(fallback, s, px));
    TextDimensions { width: shaped.advance / dpi, height: (shaped.top - shaped.bottom) / dpi, offset_y: shaped.top / dpi }
}

/// Draws `s` in fallback font `fallback` with its baseline at `y`; returns its width.
pub fn draw(fallback: usize, s: &str, x: f32, y: f32, size: u16, color: Color) -> f32 {
    let (px, dpi) = raster(size);
    let (shaped, glyphs) = with_chain(|chain| {
        let shaped = chain.shaped(fallback, s, px);
        let glyphs: Vec<_> = shaped.glyphs.iter().map(|g| (*g, chain.glyph(fallback, g.glyph, px))).collect();
        (shaped, glyphs)
    });
    for (g, tex) in glyphs {
        let Some(tex) = tex else { continue };
        let gx = x + (g.x + tex.left) / dpi;
        let gy = y - (g.y + tex.top) / dpi;
        draw_texture_ex(&tex.texture, gx, gy, color, DrawTextureParams { dest_size: Some(vec2(tex.w / dpi, tex.h / dpi)), ..Default::default() });
    }
    shaped.advance / dpi
}

/// The fallback chain's fonts in order, for the log and the fonts scene.
pub fn chain_names() -> Vec<String> {
    with_chain(|chain| chain.order.iter().map(|&i| chain.fonts[i].name.clone()).collect())
}

/// System font files that may have `c`, best first: fontconfig's sorted matches on Linux
/// (then the usual places), the fonts folder's script fonts on Windows and macOS.
fn system_candidates(c: char) -> Vec<(PathBuf, u32)> {
    #[cfg_attr(any(target_os = "android", target_os = "ios"), allow(unused_mut))]
    let mut found = Vec::new();
    // Android and iOS have no fontconfig and no such font files: none found, the bundled fonts only.
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "ios"))))]
    {
        let pattern = format!(":charset={:x}", c as u32);
        if let Ok(out) = std::process::Command::new("fc-match").args(["-s", "--format=%{file}\t%{index}\n", &pattern]).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines().take(6) {
                let (file, index) = line.split_once('\t').unwrap_or((line, "0"));
                if !file.is_empty() {
                    found.push((PathBuf::from(file), index.trim().parse().unwrap_or(0)));
                }
            }
        }
        const USUAL: &[&str] = &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/google-droid-sans-fonts/DroidSansFallbackFull.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansArabic-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        ];
        found.extend(USUAL.iter().map(|p| (PathBuf::from(p), 0)));
    }
    #[cfg(windows)]
    {
        let dir = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts");
        const FILES: &[&str] = &[
            "Nirmala.ttc", "Nirmala.ttf", "msyh.ttc", "msjh.ttc", "YuGothM.ttc", "meiryo.ttc", "msgothic.ttc", "malgun.ttf",
            "segoeui.ttf", "arial.ttf", "tahoma.ttf", "seguisym.ttf", "Leelawad.ttf", "LeelawUI.ttf", "ebrima.ttf", "mangal.ttf",
            "simsun.ttc",
        ];
        found.extend(FILES.iter().map(|f| (dir.join(f), 0)));
    }
    #[cfg(target_os = "macos")]
    {
        const FILES: &[&str] = &[
            "/System/Library/Fonts/Kohinoor.ttc",
            "/System/Library/Fonts/Supplemental/Devanagari Sangam MN.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/GeezaPro.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ];
        found.extend(FILES.iter().map(|f| (PathBuf::from(f), 0)));
    }
    let _ = c;
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(data: &'static [u8]) -> Face<'static> {
        Face::from_slice(data, 0).unwrap()
    }

    /// The chain of the original's mode for a text the sheet lacks: PT Sans, Noto Sans,
    /// Noto Sans Devanagari, by their real coverage.
    fn bundled() -> Vec<Cover> {
        [super::super::widgets::FALLBACK_FILE, NOTO_SANS, NOTO_DEVANAGARI].into_iter().map(|d| Cover::new(d, 0, &[]).unwrap()).collect()
    }

    fn pick(fonts: &[Cover], c: char) -> Option<Pick> {
        let rest: Vec<Box<dyn Fn(char) -> bool + '_>> = fonts[1..].iter().map(|f| Box::new(move |c| f.covers(c)) as Box<dyn Fn(char) -> bool>).collect();
        let rest: Vec<&dyn Fn(char) -> bool> = rest.iter().map(|b| b.as_ref()).collect();
        first_covering(c, |c| fonts[0].covers(c), &rest)
    }

    #[test]
    fn each_character_takes_the_first_font_of_the_chain_that_has_it() {
        let fonts = bundled();
        // Latin, Cyrillic and German stay in the main font (PT Sans here).
        for c in ['A', 'ж', 'Ё', 'ä', 'ß', 'Ü', '«', '—'] {
            assert_eq!(pick(&fonts, c), Some(Pick::Primary), "{c}");
        }
        // Greek and Vietnamese from Noto Sans, Hindi from Noto Sans Devanagari.
        for c in ['α', 'λ', 'ῶ', 'ệ'] {
            assert_eq!(pick(&fonts, c), Some(Pick::Fallback(0)), "{c}");
        }
        for c in ['ह', 'ि', '्', '।'] {
            assert_eq!(pick(&fonts, c), Some(Pick::Fallback(1)), "{c}");
        }
        // Chinese is left to the system's fonts: never bundled.
        assert_eq!(pick(&fonts, '中'), None);
    }

    #[test]
    fn text_splits_into_runs_by_font_with_spaces_kept_in_a_script_run() {
        let fonts = bundled();
        let covers = |p: Pick, c: char| match p {
            Pick::Primary => fonts[0].covers(c),
            Pick::Fallback(i) => fonts[i + 1].covers(c),
        };
        let pick_for = |c| pick(&fonts, c).unwrap_or(Pick::Primary);
        let s = "Язык: हिन्दी भाषा, Ελληνικά!";
        let runs: Vec<(&str, Pick)> = split(s, covers, pick_for).into_iter().map(|(r, p)| (&s[r], p)).collect();
        assert_eq!(
            runs,
            vec![
                ("Язык: ", Pick::Primary),
                ("हिन्दी भाषा", Pick::Fallback(1)),
                (", ", Pick::Primary),
                ("Ελληνικά", Pick::Fallback(0)),
                ("!", Pick::Primary),
            ]
        );
        // A text the main font has is one run: the fast path draws it as before.
        assert_eq!(split("Привет, world", covers, pick_for), vec![(0..19, Pick::Primary)]);
        // A control character never starts a run of its own.
        assert_eq!(split("a\nb", covers, pick_for), vec![(0..3, Pick::Primary)]);
    }

    #[test]
    fn hindi_is_shaped_with_the_vowel_sign_before_its_consonant() {
        let deva = face(NOTO_DEVANAGARI);
        let gid = |c: char| deva.glyph_index(c).unwrap().0;
        // "हिन्दी": ह ि न ् द ी. The vowel sign ि (U+093F) is written after ह but drawn
        // before it; न + ् + द join into a conjunct, so fewer glyphs than characters.
        let shaped = shape(&deva, "हिन्दी", 32.0);
        let ids: Vec<u16> = shaped.glyphs.iter().map(|g| g.glyph).collect();
        assert!(ids.len() < 6, "{ids:?}");
        let ha = ids.iter().position(|&g| g == gid('ह')).expect("ह drawn");
        assert!(ha > 0, "the vowel sign stands left of ह: {ids:?}");
        assert_eq!(shaped.glyphs[0].cluster, 0, "the first glyph drawn belongs to the first syllable");
        assert_ne!(ids[0], gid('ह'));
        // Positions run left to right, and the run has a width and ink above the baseline.
        assert!(shaped.glyphs.windows(2).all(|w| w[0].x <= w[1].x + 0.01));
        assert!(shaped.advance > 32.0 && shaped.top > 10.0);
    }

    #[test]
    fn a_font_does_not_cover_what_it_draws_blank_or_wrong() {
        let becker_data = super::super::widgets::face_data(2, super::super::dt_font::FontMode::Modern).unwrap();
        let becker = Cover::new(becker_data, 0, super::super::widgets::wrong_glyphs(becker_data)).unwrap();
        assert!(becker.covers('ж') && becker.covers('A') && becker.covers(' '));
        // Its ö and ß are Cyrillic letters, its «, » and — empty: those come from the chain.
        for c in ['ö', 'ß', 'Ü', '«', '»', '—'] {
            assert!(!becker.covers(c), "{c}");
        }
        // Devanagari and Arabic are shaped even when the current font has them (Kurale has
        // Devanagari); Latin, Cyrillic, Greek and Chinese are not complex.
        assert!(complex('ि') && complex('ह') && complex('ب'));
        assert!(!complex('a') && !complex('ж') && !complex('λ') && !complex('中'));
    }

    #[test]
    fn glyphs_rasterise_by_index_with_ink() {
        let deva = face(NOTO_DEVANAGARI);
        let g = deva.glyph_index('ह').unwrap().0;
        let (alpha, left, top, w, h) = rasterize(&deva, g, 24.0).unwrap();
        assert_eq!(alpha.len(), w * h);
        assert!(alpha.iter().any(|&a| a > 200));
        assert!(left >= -2.0 && top > 5.0 && top < 30.0, "{left} {top}");
        // A space has no outline: nothing to draw.
        assert!(rasterize(&deva, deva.glyph_index(' ').unwrap().0, 24.0).is_none());
    }
}
