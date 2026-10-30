use razdor::local;
use std::cell::RefCell;

use crate::ui::input::{get_keys_pressed, get_keys_released, hint_hover, is_key_down, is_key_pressed, is_mouse_button_down, is_mouse_button_pressed, is_mouse_button_released, mouse_position, mouse_wheel};
use macroquad::prelude::*;

use super::text_edit::{self, TextEdit};

pub const INK: Color = Color::new(0.93, 0.90, 0.82, 1.0);
pub const DIM: Color = Color::new(0.65, 0.62, 0.55, 1.0);
pub const ACCENT: Color = Color::new(0.95, 0.78, 0.30, 1.0);
pub const PANEL: Color = Color::new(0.12, 0.11, 0.10, 0.92);

local! {
    /// A TrueType font with Cyrillic, for text the built-in pixel font cannot draw (the
    /// original's names and descriptions).
    static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    /// The faces of the modern font mode: text, bold, titles, buttons and names.
    static FACE_FONTS: RefCell<[[Option<Font>; 4]; 1]> = const { RefCell::new([[None, None, None, None]]) };
    /// `RAZDOR_FONT*` files, which win in every mode.
    static OWN_FONTS: RefCell<[Option<Font>; 4]> = const { RefCell::new([None, None, None, None]) };
    /// The files of `OWN_FONTS` (`script_font` reads which characters they have).
    static OWN_DATA: RefCell<[Option<&'static [u8]>; 4]> = const { RefCell::new([None, None, None, None]) };
}

/// The fonts that ship with Razdor (`data/fonts`, licences next to them), by font mode
/// (`dt_font::FontMode`) and face (text, bold, titles, buttons and names). The original's
/// mode draws the install's own bitmap sheets and needs none of them.
/// Modern: PT Serif (OFL) for text and bold, Becker Medium (free for personal and commercial
/// use) for titles, Kurale (OFL) for buttons and names.
static MODE_FILES: [[&[u8]; 4]; 1] = [
    [
        include_bytes!("../../data/fonts/PT_Serif-Regular.ttf"),
        include_bytes!("../../data/fonts/PT_Serif-Bold.ttf"),
        include_bytes!("../../data/fonts/Becker-Medium.ttf"),
        include_bytes!("../../data/fonts/Kurale-Regular.ttf"),
    ],
];

/// Characters each mode's face lacks (checked against the Russian catalog and the install's
/// texts), drawn as a near look-alike instead.
const MODE_MISSING: [[&[(char, &str)]; 4]; 1] = [[&[('→', "->")], &[('→', "->")], &[], &[]]];

/// `RAZDOR_FONT`, `RAZDOR_FONT_BOLD`, `RAZDOR_FONT_TITLE` and `RAZDOR_FONT_SUBTITLE` name
/// other files (e.g. a bought Benguiat), used in every mode.
const OWN_VARS: [&str; 4] = ["RAZDOR_FONT", "RAZDOR_FONT_BOLD", "RAZDOR_FONT_TITLE", "RAZDOR_FONT_SUBTITLE"];

/// PT Sans, the font for characters the original's sheets lack and for everything when no
/// face is set.
pub(crate) static FALLBACK_FILE: &[u8] = include_bytes!("../../data/fonts/PT_Sans-Regular.ttf");

pub async fn load_font() {
    for (i, var) in OWN_VARS.iter().enumerate() {
        // Kept for good: `script_font` reads the file's coverage while it is the face.
        let data: Option<&'static [u8]> = match std::env::var(var) {
            Ok(path) => std::fs::read(&path).map_err(|e| razdor::diag!("{var}={path}: {e}")).ok().map(|d| &*Box::leak(d.into_boxed_slice())),
            Err(_) => None,
        };
        let own = data.and_then(|d| load_ttf_font_from_bytes(d).map_err(|e| razdor::diag!("{var}: {e}")).ok());
        OWN_DATA.with(|f| f.borrow_mut()[i] = own.as_ref().and(data));
        OWN_FONTS.with(|f| f.borrow_mut()[i] = own);
    }
    for (m, files) in MODE_FILES.iter().enumerate() {
        for (i, bytes) in files.iter().enumerate() {
            let font = load_ttf_font_from_bytes(bytes).map_err(|e| razdor::diag!("bundled font {m}/{i}: {e}")).ok();
            FACE_FONTS.with(|f| f.borrow_mut()[m][i] = font);
        }
    }
    let text = OWN_FONTS.with(|f| f.borrow()[0].clone()).or_else(|| load_ttf_font_from_bytes(FALLBACK_FILE).map_err(|e| razdor::diag!("bundled fallback font: {e}")).ok());
    if text.is_none() {
        razdor::diag!("no TrueType font could be loaded; transliterating");
    }
    FONT.with(|f| *f.borrow_mut() = text);
}

/// The TrueType font of face `i` (0 text, 1 bold, 2 titles, 3 buttons and names) in `mode`:
/// a `RAZDOR_FONT*` file, else the mode's bundled face; `None` in the original's mode.
pub fn face_font(i: usize, mode: super::dt_font::FontMode) -> Option<Font> {
    if let Some(own) = OWN_FONTS.with(|f| f.borrow().get(i).cloned().flatten()) {
        return Some(own);
    }
    let m = match mode {
        super::dt_font::FontMode::Original => return None,
        super::dt_font::FontMode::Modern => 0,
    };
    FACE_FONTS.with(|f| f.borrow()[m].get(i).cloned().flatten())
}

/// Characters a bundled font maps to the wrong glyph: Becker's Latin-1 letters (Ä, ö, ß…)
/// are Cyrillic letters (a font made for Windows-1251), so those come from the fallback
/// chain (`script_font`).
pub(crate) fn wrong_glyphs(data: &'static [u8]) -> &'static [std::ops::RangeInclusive<char>] {
    const BECKER: &[std::ops::RangeInclusive<char>] = &['\u{C0}'..='\u{FF}'];
    if std::ptr::eq(data, MODE_FILES[0][2]) { BECKER } else { &[] }
}

/// The file of [`face_font`]`(i, mode)`.
pub(crate) fn face_data(i: usize, mode: super::dt_font::FontMode) -> Option<&'static [u8]> {
    if let Some(own) = OWN_DATA.with(|f| f.borrow().get(i).copied().flatten()) {
        return Some(own);
    }
    match mode {
        super::dt_font::FontMode::Original => None,
        super::dt_font::FontMode::Modern => MODE_FILES[0].get(i).copied(),
    }
}

/// How much bigger or smaller the current mode's face is drawn than the size asked for
/// (every bundled face is drawn at the size asked for).
fn face_scale() -> f32 {
    1.0
}

/// `s` with the characters the current mode's face lacks replaced by look-alikes.
fn with_missing(s: &str) -> std::borrow::Cow<'_, str> {
    let m = match super::dt_font::mode() {
        super::dt_font::FontMode::Original => return s.into(),
        super::dt_font::FontMode::Modern => 0,
    };
    let missing = MODE_MISSING[m][super::dt_font::face().index()];
    if !s.chars().any(|c| missing.iter().any(|(m, _)| *m == c)) {
        return s.into();
    }
    s.chars()
        .map(|c| missing.iter().find(|(m, _)| *m == c).map_or_else(|| c.to_string(), |(_, r)| r.to_string()))
        .collect::<String>()
        .into()
}

/// Whether a TrueType font (with Cyrillic) was found.
pub fn has_font() -> bool {
    FONT.with(|f| f.borrow().is_some())
}

/// Latin stand-ins for Russian letters, used when no TrueType font is available.
fn transliterate(s: &str) -> String {
    const RU: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя";
    const LAT: [&str; 33] = [
        "a", "b", "v", "g", "d", "e", "e", "zh", "z", "i", "y", "k", "l", "m", "n", "o", "p", "r", "s", "t", "u", "f",
        "kh", "ts", "ch", "sh", "sch", "", "y", "", "e", "yu", "ya",
    ];
    s.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            match RU.chars().position(|r| r == lower) {
                Some(i) if c != lower => {
                    let mut t = LAT[i].to_string();
                    if let Some(f) = t.get_mut(0..1) {
                        f.make_ascii_uppercase();
                    }
                    t
                }
                Some(i) => LAT[i].to_string(),
                None if c.is_ascii() => c.to_string(),
                None if c == '·' => "-".to_string(),
                None => "?".to_string(),
            }
        })
        .collect()
}

/// Runs `f` with the font to use for `s` (`None` = the built-in one), its file (for the
/// characters it has, `script_font`) and the text to draw. The TrueType font draws
/// everything when there is one, as the original uses one smooth font for all its texts.
fn with_font<R>(s: &str, f: impl FnOnce(Option<&Font>, Option<&'static [u8]>, &str) -> R) -> R {
    if let Some(font) = super::dt_font::current_ttf() {
        let data = face_data(super::dt_font::face().index(), super::dt_font::mode());
        return f(Some(&font), data, &with_missing(s));
    }
    let data = OWN_DATA.with(|d| d.borrow()[0]).unwrap_or(FALLBACK_FILE);
    FONT.with(|font| match font.borrow().as_ref() {
        Some(font) => f(Some(font), Some(data), s),
        None if s.is_ascii() => f(None, None, s),
        None => f(None, None, &transliterate(s)),
    })
}

/// The size of text drawn in runs side by side: their widths added, from the highest ink
/// to the lowest.
fn joined(parts: impl Iterator<Item = TextDimensions>) -> TextDimensions {
    let (mut width, mut top, mut bottom) = (0.0f32, f32::MIN, f32::MAX);
    for d in parts {
        width += d.width;
        top = top.max(d.offset_y);
        bottom = bottom.min(d.offset_y - d.height);
    }
    if top < bottom {
        return TextDimensions { width, height: 0.0, offset_y: 0.0 };
    }
    TextDimensions { width, height: top - bottom, offset_y: top }
}

/// The pixel size a TrueType text is drawn at: above 14 px only even sizes, so the glyph
/// atlases (one entry per letter and size) stay half as big.
fn ttf_size(size: f32) -> u16 {
    let n = size.max(1.0) as u16;
    if n > 14 { n & !1 } else { n }
}

pub fn measure_ttf(s: &str, size: f32) -> TextDimensions {
    let size = size * if super::dt_font::current_ttf().is_some() { face_scale() } else { 1.0 };
    with_font(s, |font, data, s| {
        let px = ttf_size(size);
        // A character the font lacks comes from the fallback chain (`script_font`).
        match data.and_then(|d| super::script_font::runs(d, s)) {
            None => measure_text(s, font, px, 1.0),
            Some(runs) => joined(runs.into_iter().map(|(r, pick)| match pick {
                super::script_font::Pick::Primary => measure_text(&s[r], font, px, 1.0),
                super::script_font::Pick::Fallback(i) => super::script_font::measure(i, &s[r], px),
            })),
        }
    })
}

fn text_ttf(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let size = size * if super::dt_font::current_ttf().is_some() { face_scale() } else { 1.0 };
    with_font(s, |font, data, s| {
        let px = ttf_size(size);
        if let Some(runs) = data.and_then(|d| super::script_font::runs(d, s)) {
            // Runs side by side, each moving the pen by its measured width, so the text is
            // as wide as `measure_ttf` says.
            let mut pen = x;
            for (r, pick) in runs {
                let run = &s[r];
                pen += match pick {
                    super::script_font::Pick::Primary => {
                        let w = measure_text(run, font, px, 1.0).width;
                        draw_text_ex(run, pen, y, TextParams { font, font_size: px, color, ..Default::default() });
                        w
                    }
                    super::script_font::Pick::Fallback(i) => super::script_font::draw(i, run, pen, y, px, color),
                };
            }
            return;
        }
        // Measuring first puts the string's new glyphs into the atlas before any is drawn:
        // macroquad sends the whole atlas to the GPU again at each draw after a glyph was
        // added, so drawing straight away sent it once per new letter (freezes once the
        // atlas had grown, 4096² on a 2× display).
        measure_text(s, font, ttf_size(size), 1.0);
        draw_text_ex(s, x, y, TextParams { font, font_size: ttf_size(size), color, ..Default::default() });
    });
}

/// Size of `s`: in the original's font of the current face when the install has it
/// (`ui::dt_font`), else in the TrueType font.
pub fn measure(s: &str, size: f32) -> TextDimensions {
    measure_exact(s, super::dt_font::native_size(super::dt_font::face(), size))
}

/// [`measure`] at `size` as it is: no native sizing, so a caller that shrinks a text to fit
/// measures and draws (`text_exact`) at the same size. Equal to `measure` outside native
/// metrics.
pub fn measure_exact(s: &str, size: f32) -> TextDimensions {
    let bitmap = super::dt_font::with_current(|f| {
        let width = f.width(s, size, &|run| measure_ttf(run, size).width);
        let cap = f.cap_height(size);
        TextDimensions { width, height: size, offset_y: cap }
    });
    bitmap.unwrap_or_else(|| measure_ttf(s, size))
}

local! {
    /// A modal dialog is open: the screen below draws but takes no input.
    static BLOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

local! {
    /// Debug snapshots: where the pointer is said to be (`RAZDOR_MOUSE=x,y`, `ui::snapshot`).
    static POINTER: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
    /// This frame an outside tap closes the bar window on screen ([`set_outside_tap`]).
    static OUTSIDE_TAP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The pointer's position: the mouse's (or a gamepad's, `ui::input`), or the snapshot's
/// stand-in.
pub fn pointer() -> (f32, f32) {
    POINTER.with(|p| p.get()).unwrap_or_else(mouse_position)
}

/// Puts the pointer at `at` for good (debug snapshots only).
pub fn set_pointer(at: Option<(f32, f32)>) {
    POINTER.with(|p| p.set(at));
}

local! {
    /// This frame's click and key presses were taken (a wait cut by them): nothing else
    /// acts on them.
    static SWALLOWED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

local! {
    /// The dialog button where the left-button press began. Its action runs only when that
    /// same button receives the release while still under the pointer.
    static BUTTON_CAPTURE: std::cell::Cell<Option<[u32; 4]>> = const { std::cell::Cell::new(None) };
}

/// The rest of this frame sees no click and no key press ([`clicked`], [`key`],
/// [`held_key`], the global keys); cleared by [`track_held_key`] at the next frame.
pub fn swallow_input() {
    SWALLOWED.with(|s| s.set(true));
}

/// This frame's presses were taken ([`swallow_input`]).
pub fn input_swallowed() -> bool {
    SWALLOWED.with(|s| s.get())
}

/// Enter (or the keypad's) went down this frame and was not taken (Alt+Enter, the display
/// mode, swallows it).
pub fn enter_pressed() -> bool {
    !input_swallowed() && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter))
}

/// A key went down this frame (any key, Alt and F10 aside, as the held key).
pub fn any_key_pressed() -> bool {
    !input_blocked() && !input_swallowed() && get_keys_pressed().iter().any(|k| !matches!(k, KeyCode::LeftAlt | KeyCode::RightAlt | KeyCode::F10))
}

pub fn set_input_blocked(blocked: bool) {
    BLOCKED.with(|b| b.set(blocked));
}

pub fn input_blocked() -> bool {
    BLOCKED.with(|b| b.get())
}

local! {
    /// The rectangles the pointer is tested against this frame (`mouse_in`, `button_state`),
    /// and last frame's, which the phone's finger areas keep clear (`finger_hit`). Phones only.
    static FINGER_NOW: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    static FINGER_LAST: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    /// The windows and popups drawn this frame (`chrome::note_window`), and last frame's: a
    /// finger area never grows out of the smallest one that holds its target. Phones only.
    static FINGER_WINDOWS_NOW: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    static FINGER_WINDOWS_LAST: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
}

/// Android's minimum touch target, in dp: a phone's finger areas grow to this size where there
/// is room (`finger_hit_at`).
pub const FINGER_DP: f32 = 48.0;

/// Notes a rectangle as a pointer obstacle for the next frame's finger areas. Only on a phone
/// (`display::dp_px`); nothing is kept on the desktop.
fn note_finger_obstacle(r: Rect) {
    if super::display::dp_px().is_some() {
        FINGER_NOW.with(|n| n.borrow_mut().push(r));
    }
}

/// Notes a window or popup (its on-screen rectangle, `chrome::note_window`) for the next frame's
/// finger areas. Only on a phone, as [`note_finger_obstacle`].
pub fn note_finger_window(on: Rect) {
    if super::display::dp_px().is_some() {
        FINGER_WINDOWS_NOW.with(|n| n.borrow_mut().push(on));
    }
}

/// Starts a frame's finger obstacles and windows: this frame's become last frame's, and this
/// frame starts empty. `input::begin_frame` calls it once per frame, before anything is tested.
pub fn finger_frame() {
    let now = FINGER_NOW.with(|n| std::mem::take(&mut *n.borrow_mut()));
    FINGER_LAST.with(|l| *l.borrow_mut() = now);
    let windows = FINGER_WINDOWS_NOW.with(|n| std::mem::take(&mut *n.borrow_mut()));
    FINGER_WINDOWS_LAST.with(|l| *l.borrow_mut() = windows);
}

/// The room a finger area of `r` may grow into: the smallest of `windows` that fully holds `r`
/// (the window it is drawn in, so a button at a window's edge stays in that window), or `screen`
/// when no window holds it. Pure; see [`finger_hit`].
pub fn finger_room(r: Rect, windows: &[Rect], screen: Rect) -> Rect {
    let holds = |w: &Rect| w.x <= r.x && w.y <= r.y && w.right() >= r.right() && w.bottom() >= r.bottom();
    windows
        .iter()
        .filter(|w| holds(w))
        .min_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
        .copied()
        .unwrap_or(screen)
}

/// The finger-sized hit area of the target `r` on a phone, computed by [`finger_hit_at`] from
/// last frame's obstacles, with the room of [`finger_room`]: the smallest window drawn last frame
/// that holds `r`, else the screen. The desktop gets `r` back at once, at no cost.
pub fn finger_hit(r: Rect) -> Rect {
    let dp = super::display::dp_px();
    if dp.is_none() {
        return r;
    }
    let screen = Rect::new(0.0, 0.0, screen_width(), screen_height());
    FINGER_WINDOWS_LAST.with(|w| {
        let room = finger_room(r, &w.borrow(), screen);
        FINGER_LAST.with(|l| finger_hit_at(r, dp, room, super::pan::bar_top(), &l.borrow()))
    })
}

/// Grows a target's hit area to a finger's size on a phone, as a pure function of its inputs.
///
/// `r` is the target as drawn. `dp` is the phone's pixels per dp; `None` (the desktop) returns
/// `r` unchanged, as does a target with no area. `screen` is the screen, `bar_top` the top of
/// the bottom bar, and `others` the rectangles the pointer is tested against (the obstacles).
///
/// Each axis grows to [`FINGER_DP`] dp (`want`), centred on `r`, as far as there is room:
/// - the screen's edges are free room;
/// - the bottom bar's top: a target on the bar (its top at or under `bar_top`) does not grow
///   above it, a target above the bar does not grow into it, and a target that straddles it
///   has no bar limit;
/// - an obstacle that overlaps `r` on one axis (its projections meet on the other axis) and is
///   beside it on the first limits the growth to the midline between the two, so two areas
///   never share room. An obstacle that contains `r`, is inside it or overlaps it (the target
///   itself, a container, or something drawn in it) is skipped: it has its own pointer test;
/// - an obstacle apart from `r` on both axes (diagonal) is handled after the growth: if the
///   grown area still overlaps it, the axis on which the two are farther apart (the bigger gap)
///   is cut at the midline between them.
///
/// Room blocked on one side goes to the other side, so a target can grow to `want` on an axis
/// with room on one side only. The result is never smaller than `r` and never leaves the
/// screen, unless `r` itself does. Razdor's phone choice, not the original's: the drawing stays
/// `r`.
pub fn finger_hit_at(r: Rect, dp: Option<f32>, screen: Rect, bar_top: f32, others: &[Rect]) -> Rect {
    let dp = match dp {
        Some(dp) if r.w > 0.0 && r.h > 0.0 => dp,
        _ => return r,
    };
    let want = FINGER_DP * dp;
    let (mut x_lo, mut x_hi) = (screen.x, screen.x + screen.w);
    let (mut y_lo, mut y_hi) = (screen.y, screen.y + screen.h);
    if r.y >= bar_top - 0.5 {
        y_lo = y_lo.max(bar_top);
    } else if r.y + r.h <= bar_top + 0.5 {
        y_hi = y_hi.min(bar_top);
    }
    let mut diagonal = Vec::new();
    for &o in others {
        if !(o.w > 0.0 && o.h > 0.0) {
            continue;
        }
        let x_over = o.x < r.right() && r.x < o.right();
        let y_over = o.y < r.bottom() && r.y < o.bottom();
        if x_over && y_over {
            // The target itself, a container or something inside it: not a neighbour.
            continue;
        }
        if x_over {
            if o.bottom() <= r.y {
                y_lo = y_lo.max((o.bottom() + r.y) / 2.0);
            }
            if o.y >= r.bottom() {
                y_hi = y_hi.min((r.bottom() + o.y) / 2.0);
            }
        } else if y_over {
            if o.right() <= r.x {
                x_lo = x_lo.max((o.right() + r.x) / 2.0);
            }
            if o.x >= r.right() {
                x_hi = x_hi.min((r.right() + o.x) / 2.0);
            }
        } else {
            diagonal.push(o);
        }
    }
    let (x, w) = grow_axis(r.x, r.w, x_lo, x_hi, want);
    let (y, h) = grow_axis(r.y, r.h, y_lo, y_hi, want);
    let mut hit = Rect::new(x, y, w, h);
    for o in diagonal {
        let overlaps = o.x < hit.right() && hit.x < o.right() && o.y < hit.bottom() && hit.y < o.bottom();
        if !overlaps {
            continue;
        }
        let gap_x = if o.x >= r.right() { o.x - r.right() } else { r.x - o.right() };
        let gap_y = if o.y >= r.bottom() { o.y - r.bottom() } else { r.y - o.bottom() };
        if gap_x >= gap_y {
            // Cut on x: the grown edge facing `o` stops at the midline, never inside `r`.
            let (left, right) = (hit.x, hit.right());
            if o.x >= r.right() {
                hit.w = (r.right() + o.x) / 2.0 - left;
            } else {
                let new_left = (o.right() + r.x) / 2.0;
                hit.x = new_left;
                hit.w = right - new_left;
            }
        } else {
            // Cut on y, the same way.
            let (top, bottom) = (hit.y, hit.bottom());
            if o.y >= r.bottom() {
                hit.h = (r.bottom() + o.y) / 2.0 - top;
            } else {
                let new_top = (o.bottom() + r.y) / 2.0;
                hit.y = new_top;
                hit.h = bottom - new_top;
            }
        }
    }
    hit
}

/// One axis of [`finger_hit_at`]: the start and length of the span `start..start + len` grown
/// to `want` within the free interval `lo..hi`, centred on the span. The result is never smaller
/// than the span and stays inside `lo..hi` unless the span does not.
fn grow_axis(start: f32, len: f32, lo: f32, hi: f32, want: f32) -> (f32, f32) {
    let lo = lo.min(start);
    let hi = hi.max(start + len);
    let grown = len.max(want.min(hi - lo));
    let at = (start + len / 2.0 - grown / 2.0).max(lo).min(hi - grown);
    (at, grown)
}

/// Whether the pointer may act on the rectangle at (`x`, `y`) of size `w` × `h`: it is over it,
/// no dialog is open, and no window covers it.
fn pointer_in(x: f32, y: f32, w: f32, h: f32) -> bool {
    let (mx, my) = pointer();
    // A window's controls take no pointer over the bottom bar (`pan::cover_bar`).
    !input_blocked() && !super::pan::covered((mx, my)) && mx >= x && mx < x + w && my >= y && my < y + h
}

/// Whether the pointer may act on the rectangle (`pointer_in`). The rectangle is also a
/// pointer obstacle for the phone's finger areas of the next frame (`finger_hit`).
pub fn mouse_in(x: f32, y: f32, w: f32, h: f32) -> bool {
    note_finger_obstacle(Rect::new(x, y, w, h));
    pointer_in(x, y, w, h)
}

pub fn clicked() -> bool {
    !input_blocked() && !input_swallowed() && is_mouse_button_pressed(MouseButton::Left)
}

/// Tracks a button from mouse-down to mouse-up. Returns its hover, held-down visual state,
/// and activation edge; dragging out cancels activation and dragging back in restores it.
/// The pointer is tested against the hit area, not the drawn `r`: on a phone that is the
/// finger-sized area of [`finger_hit`], where there is room. The press is captured on `r`.
/// A phone choice of Razdor, not the original's.
#[track_caller]
pub fn button_state(r: Rect, enabled: bool) -> (bool, bool, bool) {
    note_finger_obstacle(r);
    let hit = finger_hit(r);
    super::touch_probe::record("button", hit, enabled);
    let hover = enabled && !input_swallowed() && pointer_in(hit.x, hit.y, hit.w, hit.h);
    let capture = [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()];
    let (next, down, pressed) = capture_step(
        BUTTON_CAPTURE.with(|c| c.get()),
        capture,
        hover,
        is_mouse_button_pressed(MouseButton::Left),
        is_mouse_button_released(MouseButton::Left),
        is_mouse_button_down(MouseButton::Left),
    );
    BUTTON_CAPTURE.with(|c| c.set(next));
    (hover, down, pressed)
}

/// One frame of a button's press capture (`button_state`): `captured` is the rect the left
/// press began on, `mine` this button's rect. Returns the new capture, whether the button
/// shows as held, and whether it was clicked this frame. A release ends the press that is
/// captured on this button: released on it, that is a click, and the capture is used up, so
/// a second release with no new press (a worn switch bouncing) clicks nothing; released off
/// it, the press is cancelled. A release on another button leaves this one's capture alone.
fn capture_step(captured: Option<[u32; 4]>, mine: [u32; 4], hover: bool, down_edge: bool, up_edge: bool, held: bool) -> (Option<[u32; 4]>, bool, bool) {
    let mut next = captured;
    if down_edge && hover {
        next = Some(mine);
    } else if !held && !up_edge {
        next = None;
    }
    let captured_here = next == Some(mine);
    let down = captured_here && hover && held;
    let pressed = captured_here && hover && up_edge;
    if captured_here && up_edge {
        next = None;
    }
    (next, down, pressed)
}

pub fn right_clicked() -> bool {
    !input_blocked() && !input_swallowed() && is_mouse_button_pressed(MouseButton::Right)
}

pub fn key(k: KeyCode) -> bool {
    !input_blocked() && !input_swallowed() && if is_escape(k) { esc_pressed() } else { is_key_pressed(k) }
}

/// The keys that are Esc: the keyboard's Esc and the system's Back (Android's Back key comes
/// as `KeyCode::Back`; on a phone it does what Esc does everywhere).
pub fn is_escape(k: KeyCode) -> bool {
    matches!(k, KeyCode::Escape | KeyCode::Back)
}

/// Esc went down this frame, from the keyboard or the system's Back. Raw: no input-blocked
/// check (use [`key`] for that). Also true on a frame an outside tap closes a bar window
/// ([`set_outside_tap`]).
pub fn esc_pressed() -> bool {
    OUTSIDE_TAP.with(|t| t.get()) || is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Back)
}

/// Sets for this frame's dispatch that a tap outside the bar window closed it: the window
/// reads it as Esc ([`esc_pressed`]). The frame loop sets it and clears it again.
pub fn set_outside_tap(on: bool) {
    OUTSIDE_TAP.with(|t| t.set(on));
}

/// A tap (not a drag) outside every window in `windows` and off the bottom bar `bar`: the
/// press and the release both outside, the pointer moved less than `slop` between them. The
/// release decides nothing on its own: both ends must be outside. Coordinates on the screen.
pub fn outside_tap(press: Vec2, release: Vec2, windows: &[Rect], bar: Rect, slop: f32) -> bool {
    let outside = |p: Vec2| !bar.contains(p) && !windows.iter().any(|r| r.contains(p));
    (release - press).length() < slop && outside(press) && outside(release)
}

/// The system keys that never become the held key: Alt, F10 and Android's Back.
fn is_system_key(k: KeyCode) -> bool {
    matches!(k, KeyCode::LeftAlt | KeyCode::RightAlt | KeyCode::F10 | KeyCode::Back)
}

local! {
    /// The original's one held key (interface.md §2, 0x475748).
    static HELD_KEY: std::cell::Cell<Option<KeyCode>> = const { std::cell::Cell::new(None) };
    /// The key the system's typematic repeats and when it went down ([`next_repeat`]).
    static REPEAT_KEY: std::cell::Cell<Option<(KeyCode, f64)>> = const { std::cell::Cell::new(None) };
}

/// Once per frame, before the screens: the original keeps no key table, only the last key
/// that went down, and any key going up clears it (so with two keys down, letting one go
/// leaves none held). Alt and F10 are system keys and never become the held key.
pub fn track_held_key() {
    SWALLOWED.with(|s| s.set(false));
    let (pressed, released) = (get_keys_pressed(), get_keys_released());
    let (now, dt) = (crate::ui::clock::now(), crate::ui::clock::frame_dt() as f64);
    let repeat = REPEAT_KEY.with(|r| next_repeat(r.get(), pressed.iter().copied(), &released, now, is_key_down));
    REPEAT_KEY.with(|r| r.set(repeat));
    HELD_KEY.with(|h| h.set(repeated_held(next_held(h.get(), pressed, !released.is_empty(), is_key_down), repeat, now, dt)));
}

/// The key the system repeats after a frame's key events: the last key that went down, while
/// it stays down (Windows' and X's typematic repeats only the newest key, and a key let go
/// ends its repeat without passing it to another key still down). Alt and F10 aside, as the
/// held key. The time is when it went down.
fn next_repeat(repeat: Option<(KeyCode, f64)>, pressed: impl IntoIterator<Item = KeyCode>, released: &std::collections::HashSet<KeyCode>, now: f64, down: impl Fn(KeyCode) -> bool) -> Option<(KeyCode, f64)> {
    if let Some(k) = pressed.into_iter().find(|&k| !is_system_key(k) && down(k)) {
        return Some((k, now));
    }
    repeat.filter(|&(k, _)| !released.contains(&k) && down(k))
}

/// The held key once the system's repeats of this frame came in: the original's window
/// procedure stores every WM_KEYDOWN as the held key without looking at the repeat bit
/// (0x475820–0x475837), so a repeat makes its key held again after another key's release
/// cleared it (0x475841: any WM_KEYUP clears the held key). Held Up, Right pressed, Up let
/// go: the map stops, then scrolls right again from Right's first repeat (500 ms after it
/// went down, Windows' default). Razdor took only the first press of a key (macroquad's
/// pressed set leaves the repeats out), so there the map stayed stuck while Right was held.
fn repeated_held(held: Option<KeyCode>, repeat: Option<(KeyCode, f64)>, now: f64, dt: f64) -> Option<KeyCode> {
    match repeat {
        Some((k, t0)) if repeats(now - t0, dt) > 0 => Some(k),
        _ => held,
    }
}

/// The held key after a frame's key events: any key up clears it, and a key that went down
/// becomes it while it is still down. The original takes its messages in order (0x475748),
/// so a key pressed and let go within one frame (a quick tap, a slow frame) ends up not held;
/// Razdor sees that frame's presses and releases only as sets, and a tap kept as held stayed
/// held for good, cutting each walk at its first step.
fn next_held(held: Option<KeyCode>, pressed: impl IntoIterator<Item = KeyCode>, any_up: bool, down: impl Fn(KeyCode) -> bool) -> Option<KeyCode> {
    let held = if any_up { None } else { held };
    let pressed = pressed.into_iter().find(|&k| !is_system_key(k) && down(k));
    pressed.or(held)
}

/// Forgets the held key: one the cheat console took (its ~ or Esc, still held as it closes)
/// does not then scroll the map or stop the hero's walk. The next key pressed is held again.
pub fn forget_held_key() {
    HELD_KEY.with(|h| h.set(None));
    REPEAT_KEY.with(|r| r.set(None));
}

/// The held key as the original sees it ([`track_held_key`]); none while input is blocked.
pub fn held_key() -> Option<KeyCode> {
    if input_blocked() || input_swallowed() {
        None
    } else {
        HELD_KEY.with(|h| h.get())
    }
}

/// The keys of the original's Yes / No box (interface.md §11, 0x4c811c): Esc answers No and
/// any other key Yes (Enter, Space, Y or a letter alike), except Tab, Alt and the Up and
/// Down arrows, which do nothing. N answers No, as it did in Razdor before the parity pass
/// (the original takes it for Yes). `None` while no such key went down this frame.
pub fn answer_key() -> Option<bool> {
    if input_blocked() || input_swallowed() {
        return None;
    }
    answer_of(get_keys_pressed().into_iter())
}

/// A key of the original's one-button event window went down this frame (0x4cd558): any key
/// but Tab, Alt and the Up and Down arrows, and Razdor's scroll keys of a long text.
pub fn ok_key_pressed() -> bool {
    !input_blocked() && !input_swallowed() && get_keys_pressed().into_iter().any(|k| !ok_ignored(&k))
}

/// Such a key came up this frame: the window's OK is clicked (0x4cd68c).
pub fn ok_key_released() -> bool {
    !input_blocked() && !input_swallowed() && get_keys_released().into_iter().any(|k| !ok_ignored(&k))
}

fn ok_ignored(k: &KeyCode) -> bool {
    answer_ignored(k) || matches!(k, KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End)
}

fn answer_ignored(k: &KeyCode) -> bool {
    matches!(k, KeyCode::Tab | KeyCode::LeftAlt | KeyCode::RightAlt | KeyCode::F10 | KeyCode::Up | KeyCode::Down)
}

fn answer_of(keys: impl Iterator<Item = KeyCode>) -> Option<bool> {
    let keys: Vec<KeyCode> = keys.filter(|k| !answer_ignored(k)).collect();
    if keys.iter().any(|&k| is_escape(k)) || keys.contains(&KeyCode::N) {
        Some(false)
    } else if keys.is_empty() {
        None
    } else {
        Some(true)
    }
}

/// Mouse wheel steps this frame (up is positive), 0 while input is blocked.
pub fn wheel() -> f32 {
    if input_blocked() {
        0.0
    } else {
        mouse_wheel().1
    }
}

/// A list or text box `r` of rows `row_px` screen pixels high: a one-finger swipe over it turns
/// the wheel a step per row (`ui::gestures`), as the wheel does here. Call where it is drawn.
pub fn swipe_rows(r: Rect, row_px: f32) {
    crate::ui::input::swipe_area(r, crate::ui::gestures::SwipeKind::ScrollRows { row_px });
}

/// Drawing clipped to a rectangle (screen coordinates) while the guard lives.
pub struct Clip;

impl Clip {
    pub fn new(r: Rect) -> Clip {
        // The canvas rect on the screen with the pan, cut above the bottom bar while a window
        // covers it (`pan::cover_bar`).
        let r = super::pan::clipped(super::pan::on_screen(r));
        let dpi = screen_dpi_scale();
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.scissor(Some(((r.x * dpi) as i32, (r.y * dpi) as i32, (r.w * dpi).ceil() as i32, (r.h * dpi).ceil() as i32)));
        Clip
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        super::pan::restore_scissor();
    }
}

/// A translucent panel of lines next to the mouse: the original's ordinary hint box, which
/// fades in to 160/255 over 300 ms (49ee4b–49ef0d: `elapsed × 160 / 300`).
/// Not shown with the hints off (interface.md hint boxes: only pushed and spell-target hints
/// stay), nor on a phone unless a long press holds (gestures) or after a finger lifted (touchpad:
/// `input::hint_hover`).
pub fn tooltip(lines: &[(String, Color)]) {
    if !hints_on() || !hint_hover() {
        return;
    }
    tooltip_at_alpha(lines, hint_fade(lines, 160.0 / 255.0));
}

/// A queued hint box: its lines and its fade.
type Queued = (Vec<(String, Color)>, f32);

local! {
    /// Hint boxes queued by [`tooltip_later`] with their fade, drawn by [`draw_later`].
    static LATER: RefCell<Vec<Queued>> = const { RefCell::new(Vec::new()) };
}

/// A hint box for a window whose frame lines are drawn after its content (the army and
/// building windows): queued now, drawn by [`draw_later`] once the frame is up, so no steel
/// line crosses the box (the original draws its pop-ups over the frame).
pub fn tooltip_later(lines: &[(String, Color)]) {
    if !hints_on() || !hint_hover() {
        return;
    }
    let alpha = hint_fade(lines, 160.0 / 255.0);
    LATER.with(|q| q.borrow_mut().push((lines.to_vec(), alpha)));
}

/// Draws the hint boxes queued by [`tooltip_later`], in the order they were asked for.
pub fn draw_later() {
    let queued = LATER.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for (lines, alpha) in queued {
        tooltip_at_alpha(&lines, alpha);
    }
}

/// The bottom panel's hint box (49d5ac): a pushed hint (kind 4), which the original fades in
/// to full opacity over 300 ms (49ee4b: `elapsed × 255 / 300`), so the bar under it does not
/// show through once it is up.
pub fn panel_hint(lines: &[(String, Color)]) {
    tooltip_at_alpha(lines, hint_fade(lines, 1.0));
}

local! {
    /// The hint box shown: its text, when it appeared and when it was last drawn (the same
    /// text is not restarted while it stays up).
    static HINT_SHOWN: std::cell::RefCell<(String, f64, f64)> = const { std::cell::RefCell::new((String::new(), 0.0, 0.0)) };
}

/// The opacity of a hint box `ms` after it appeared, fading in to `full` over 300 ms.
pub fn hint_alpha(ms: f64, full: f32) -> f32 {
    ((ms / 300.0).clamp(0.0, 1.0) as f32) * full
}

/// The opacity of the hint box with these lines now: its fade restarts when its text changes or
/// when it was not drawn for a moment (it closed in between).
fn hint_fade(lines: &[(String, Color)], full: f32) -> f32 {
    let text: String = lines.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>().join("\n");
    let now = crate::ui::clock::now() * 1000.0;
    let start = HINT_SHOWN.with(|h| {
        let mut h = h.borrow_mut();
        if h.0 != text || now - h.2 > 100.0 {
            *h = (text, now, now);
        } else {
            h.2 = now;
        }
        h.1
    });
    hint_alpha(now - start, full)
}

local! {
    /// The hint boxes are shown: off with "Отключить всплывающие подсказки" (`OptValue6`).
    static HINTS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Shows or hides the hint boxes (the settings window's "hints off").
pub fn set_hints(on: bool) {
    HINTS.with(|h| h.set(on));
}

/// Whether hint boxes are shown (interface.md §16: on unless `OptValue6` is 1).
pub fn hints_on() -> bool {
    HINTS.with(|h| h.get())
}

/// The hint box that follows the mouse (kind 7, 0x49e9b8): `w` wide, its lines centred, at
/// the mouse +21, +37, put on the other side of the mouse where it would leave the screen,
/// at full opacity (0x49ee4b). Shown whatever option 6 says: the spell-target hints always
/// show (interface.md §10, 49e710 kind 7).
pub fn hint_box_always(lines: &[(String, Color)], w: f32) {
    if lines.is_empty() {
        return;
    }
    let k = super::chrome::k();
    let size = super::typography::Role::Hint.size(17.0 * k);
    let line_h = super::typography::Role::Hint.line_height(21.0 * k);
    let w = w.min(screen_width() - 8.0);
    let h = lines.len() as f32 * line_h + 14.0 * k;
    let (mx, my) = pointer();
    let mut x = mx + 21.0 * k;
    let mut y = my + 37.0 * k;
    if x + w > screen_width() {
        x = mx - 21.0 * k - w;
    }
    if y + h > screen_height() {
        y = my - 37.0 * k - h;
    }
    let r = Rect::new(x.clamp(4.0, (screen_width() - w - 4.0).max(4.0)), y.clamp(4.0, (screen_height() - h - 4.0).max(4.0)), w, h);
    super::chrome::surface_alpha(r, super::chrome::Skin::Marble, 1.0);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.05, 0.03, 0.2));
    super::chrome::silver_frame(r, 1.5);
    for (i, (s, c)) in lines.iter().enumerate() {
        let shown = ellipsize(s, w - 16.0 * k, size);
        super::chrome::shadow_centered(&shown, r.center().x, r.y + 7.0 * k + (i as f32 + 0.5) * line_h + size * 0.36, size, *c);
    }
}

fn tooltip_at_alpha(lines: &[(String, Color)], alpha: f32) {
    super::typography::Role::Hint.with(|| tooltip_content(lines, alpha));
}

fn tooltip_content(lines: &[(String, Color)], alpha: f32) {
    if lines.is_empty() {
        return;
    }
    let w = lines.iter().map(|(s, _)| measure(s, 17.0).width).fold(0.0, f32::max) + 24.0;
    let size = super::dt_font::native_size(super::dt_font::face(), 17.0);
    let line_h = super::dt_font::native_line_height(super::dt_font::face(), 21.0);
    let h = lines.len() as f32 * line_h + 14.0;
    let (mx, my) = pointer();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - h - 4.0);
    let r = Rect::new(x, y, w, h);
    super::chrome::surface_alpha(r, super::chrome::Skin::Marble, alpha);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.05, 0.03, 0.2 * alpha));
    let edge = Color { a: super::chrome::SILVER.a * alpha, ..super::chrome::SILVER };
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, edge);
    // The box and its text fade together (the whole box is blended, 49ee4b).
    let ink = alpha / alpha.max(160.0 / 255.0).max(f32::EPSILON);
    for (i, (s, c)) in lines.iter().enumerate() {
        super::chrome::shadow_text(s, x + 12.0, y + 7.0 + size * 0.8 + i as f32 * line_h, size, Color { a: c.a * ink.min(1.0), ..*c });
    }
}

/// The three frames of the original's map tooltips (0x4ca8ac → 0x48d3f8 styles 0, 1, 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipStyle {
    /// Green marble (`Win-marble`).
    Normal,
    /// Red marble (`Win-red`): an army or building that is not friendly.
    Hostile,
    /// Dull brown marble: bridges, ruins nobody guards.
    Neutral,
}

/// The map tooltip's box `r` in one of the original's styles (0x48d3f8, called by 0x4ca8ac):
/// `Win-marble` (normal) or the red `Win-red` (hostile) tiled, or for the neutral style the
/// marble turned dull brown ([`neutral_tip_pixel`]); its edge; the title strip's line 27 px
/// down; the title corners at (2, 3) and at the right end, added (0x476790 mode 0). The box
/// is opaque: the original only fades it in (0x4ca728), the ground never shows through.
pub fn tooltip_panel_styled(r: Rect, style: TipStyle, u: f32) {
    use super::chrome::{self, Skin};
    let k = chrome::k();
    match style {
        TipStyle::Normal => chrome::surface(r, Skin::Marble),
        TipStyle::Hostile => chrome::surface(r, Skin::Red),
        TipStyle::Neutral => match chrome::win_adjusted("Win-marble", "neutral-tip", neutral_tip_image) {
            Some(t) => chrome::tile(&t, r, k.min(1.4), WHITE),
            None => draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.30, 0.20, 0.13, 1.0)),
        },
    }
    // The line under the title: 0x48d1d4 from (2, 27) to (w − 2, 29), 2 px. `u` is the
    // tooltip's own scale (its title strip grows with the text, Razdor's).
    draw_rectangle(r.x + 2.0 * u, r.y + 27.0 * u, r.w - 4.0 * u, 2.0 * u, chrome::SILVER_DARK);
    draw_line(r.x + 2.0 * u, r.y + 27.0 * u, r.right() - 2.0 * u, r.y + 27.0 * u, 1.0, chrome::SILVER);
    for (name, right) in [("Corner-Left", false), ("Corner-Right", true)] {
        let Some(t) = chrome::win_adjusted(name, style.corner_tag(), |img| tip_corner_image(img, style)) else { continue };
        let (w, h) = (t.width() * u, t.height() * u);
        let x = if right { r.right() - 2.0 * u - w } else { r.x + 2.0 * u };
        chrome::additive(|| chrome::tex(&t, Rect::new(x, r.y + 3.0 * u, w, h), WHITE));
    }
    chrome::silver_frame(r, 1.5);
}

impl TipStyle {
    fn corner_tag(self) -> &'static str {
        match self {
            TipStyle::Normal => "tip-corner-normal",
            TipStyle::Hostile => "tip-corner-hostile",
            TipStyle::Neutral => "tip-corner-neutral",
        }
    }
}

/// What the start-up takes off the tooltips' title corners (0x4768f8 mode 1 with
/// 0x476374(r, g, b)): the normal pair `ae2554/58` loses (192, 160, 160) (0x4dbd83..0x4dbddc),
/// the hostile pair `ae255c/60`, loaded again from the same files, (128, 192, 250)
/// (0x4dbe0b..0x4dbe64): left a dark orange, added onto the red.
pub fn tip_corner_taken(style: TipStyle) -> [i32; 3] {
    match style {
        TipStyle::Hostile => [128, 192, 250],
        TipStyle::Normal | TipStyle::Neutral => [192, 160, 160],
    }
}

/// One pixel of a tooltip's title corner: [`tip_corner_taken`] off it, down to black; the
/// neutral style draws the normal pair, then turns the whole box ([`neutral_tip_pixel`]).
pub fn tip_corner_pixel(style: TipStyle, p: [u8; 3]) -> [u8; 3] {
    let minus = tip_corner_taken(style);
    let q: [u8; 3] = std::array::from_fn(|i| (p[i] as i32 - minus[i]).clamp(0, 255) as u8);
    if style == TipStyle::Neutral { neutral_tip_pixel(q) } else { q }
}

/// The neutral style's turn of the box (0x48d3f8 style 2): the green channel into all three
/// (0x48dd40), then scaled by 1024, 0x2aa and 0x1c7 per 1024 (0x48da5c): a dull brown.
pub fn neutral_tip_pixel(p: [u8; 3]) -> [u8; 3] {
    let g = p[1] as i32;
    [g, g * 0x2aa / 1024, g * 0x1c7 / 1024].map(|c| c.clamp(0, 255) as u8)
}

fn tip_corner_image(img: &mut razdor::dt::gfx::Image, style: TipStyle) {
    for p in img.rgba.chunks_exact_mut(4) {
        let q = tip_corner_pixel(style, [p[0], p[1], p[2]]);
        p[..3].copy_from_slice(&q);
    }
}

fn neutral_tip_image(img: &mut razdor::dt::gfx::Image) {
    for p in img.rgba.chunks_exact_mut(4) {
        let q = neutral_tip_pixel([p[0], p[1], p[2]]);
        p[..3].copy_from_slice(&q);
    }
}

/// Draws a button and returns true when it was clicked this frame.
#[track_caller]
pub fn button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    button_sounding(x, y, w, h, label, enabled, super::audio::Cue::Button)
}

/// [`button`] in the original's red (its buttons with their channels turned, 0x4dba90): the
/// army window's Dismiss and its confirmation, the defeat report.
#[track_caller]
pub fn red_button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    super::touch_probe::record("button", Rect::new(x, y, w, h), enabled);
    let hover = enabled && mouse_in(x, y, w, h);
    super::chrome::defeat_button(Rect::new(x, y, w, h), label, enabled, hover);
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    pressed
}

/// [`button`] with its own press sound (the money buttons play `Item-Gold`).
#[track_caller]
pub fn button_sounding(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool, sound: super::audio::Cue) -> bool {
    let r = Rect::new(x, y, w, h);
    let (_, down, pressed) = button_state(r, enabled);
    super::chrome::marble_button(Rect::new(x, y, w, h), label, enabled, down);
    if pressed {
        super::audio::cue(sound);
    }
    pressed
}

/// Draws `s` with its baseline at `y` (see [`measure`] for the font).
pub fn text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    text_exact(s, x, y, super::dt_font::native_size(super::dt_font::face(), size), color);
}

/// [`text`] at `size` as it is (see [`measure_exact`]).
pub fn text_exact(s: &str, x: f32, y: f32, size: f32, color: Color) {
    if super::text_probe::on() {
        let cap = measure_exact("Н", size).offset_y;
        super::text_probe::record(s, x, y, measure_exact(s, size).width, size, cap);
    }
    let color = super::typography::color(color);
    let drawn = super::dt_font::with_current(|f| {
        f.draw(s, x, y, size, color, &|run, px, py| {
            text_ttf(run, px, py, size, color);
            measure_ttf(run, size).width
        })
    });
    if drawn.is_none() {
        text_ttf(s, x, y, size, color);
    }
}

/// `s` with its byte range `marked` lit (a search's match): that part in `mark` on a dark
/// band, the rest in `color`.
pub fn text_marked(s: &str, marked: Option<std::ops::Range<usize>>, x: f32, y: f32, size: f32, color: Color, mark: Color) {
    let Some(r) = marked.filter(|r| r.end <= s.len() && s.is_char_boundary(r.start) && s.is_char_boundary(r.end)) else {
        text(s, x, y, size, color);
        return;
    };
    let (head, hit, tail) = (&s[..r.start], &s[r.clone()], &s[r.end..]);
    let hx = x + measure(head, size).width;
    let hw = measure(hit, size).width;
    draw_rectangle(hx - 1.0, y - size * 0.85, hw + 2.0, size * 1.1, Color::new(0.35, 0.22, 0.02, 0.9));
    text(head, x, y, size, color);
    text(hit, hx, y, size, mark);
    text(tail, hx + hw, y, size, color);
}

/// The font size, at most `size` and not below 70% of it, at which `s` fits in `width`
/// (Russian texts run longer than the English ones the layouts were drawn for).
pub fn fit_size(s: &str, width: f32, size: f32) -> f32 {
    if super::typography::fixed_size() {
        return super::dt_font::native_size(super::dt_font::face(), size);
    }
    let mut fit = size;
    while fit > (size * 0.7).max(9.0) && measure(s, fit).width > width {
        fit -= 1.0;
    }
    fit
}

/// `s` shortened with "…" until it fits in `width` at `size`.
pub fn ellipsize(s: &str, width: f32, size: f32) -> String {
    if width <= 0.0 { return String::new(); }
    if measure(s, size).width <= width {
        return s.to_string();
    }
    if measure("…", size).width > width { return String::new(); }
    let mut t: String = s.to_string();
    while !t.is_empty() && measure(&format!("{t}…"), size).width > width {
        t.pop();
    }
    format!("{}…", t.trim_end())
}

/// Text in a box `width` wide: smaller if it must be (see [`fit_size`]), then shortened.
pub fn text_fit(s: &str, x: f32, y: f32, width: f32, size: f32, color: Color) {
    let fit = fit_size(s, width, size);
    text(&ellipsize(s, width, fit), x, y, fit, color);
}

pub fn text_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let dim = measure(s, size);
    text(s, cx - dim.width / 2.0, y, size, color);
}

/// Splits `s` into lines no wider than `width` at `size`.
pub fn wrap(s: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && measure(&candidate, size).width > width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// The font state [`measure`] reads: the face, the font mode, native metrics and, with them,
/// the interface scale (`native_size`).
#[derive(Clone, Copy, PartialEq)]
struct MeasureState {
    face: super::dt_font::Face,
    mode: super::dt_font::FontMode,
    native: bool,
    k: u32,
}

/// One [`wrap_cached`] result, with the font state its widths were measured in.
struct WrapEntry {
    text: String,
    width: u32,
    size: u32,
    state: MeasureState,
    lines: Vec<String>,
}

/// The most [`wrap_cached`] results kept: the message windows on the screen (a few at most).
const WRAP_CACHE_MAX: usize = 16;

local! {
    /// The last [`wrap_cached`] results, oldest first.
    static WRAPS: RefCell<Vec<WrapEntry>> = const { RefCell::new(Vec::new()) };
}

/// The cache's answer for `s` at `width` and `size` in `state`: the kept lines when the same
/// text, width, size and font state were wrapped before, else what `compute` gives (kept,
/// the oldest result dropped when the cache is full).
fn cached_wrap(cache: &mut Vec<WrapEntry>, state: MeasureState, s: &str, width: f32, size: f32, compute: impl FnOnce() -> Vec<String>) -> Vec<String> {
    let (width_key, size_key) = (width.to_bits(), size.to_bits());
    if let Some(e) = cache.iter().find(|e| e.width == width_key && e.size == size_key && e.state == state && e.text == s) {
        return e.lines.clone();
    }
    let lines = compute();
    if cache.len() >= WRAP_CACHE_MAX {
        cache.remove(0);
    }
    cache.push(WrapEntry { text: s.to_string(), width: width_key, size: size_key, state, lines: lines.clone() });
    lines
}

/// [`wrap`], kept per text, width, size and the font state its widths come from ([`MeasureState`]).
/// A window lays out its text every frame with the same answer: this skips the re-measuring of
/// every word, and the result is what [`wrap`] gives for the same arguments.
pub fn wrap_cached(s: &str, width: f32, size: f32) -> Vec<String> {
    let native = super::dt_font::native_metrics();
    let state = MeasureState {
        face: super::dt_font::face(),
        mode: super::dt_font::mode(),
        native,
        k: if native { super::chrome::k().to_bits() } else { 0 },
    };
    WRAPS.with(|cache| cached_wrap(&mut cache.borrow_mut(), state, s, width, size, || wrap(s, width, size)))
}

/// [`wrap`] for words in their own colours: `parts` are pieces of one paragraph (each may hold
/// several words), the lines keep each word's colour.
pub fn wrap_coloured(parts: &[(String, Color)], width: f32, size: f32) -> Vec<Vec<(String, Color)>> {
    let words: Vec<(&str, Color)> = parts.iter().flat_map(|(t, c)| t.split_whitespace().map(move |w| (w, *c))).collect();
    let text: Vec<&str> = words.iter().map(|w| w.0).collect();
    let mut lines = Vec::new();
    let mut next = 0;
    for line in wrap(&text.join(" "), width, size) {
        let n = line.split_whitespace().count();
        lines.push(words[next..next + n].iter().map(|&(w, c)| (w.to_string(), c)).collect());
        next += n;
    }
    lines
}

/// Draws a line of coloured words from [`wrap_coloured`] with the shadow, its left end at
/// `x` (or centred on `x` when `centred`), the words a space apart.
pub fn shadow_coloured(line: &[(String, Color)], x: f32, y: f32, size: f32, centred: bool) {
    let all: Vec<&str> = line.iter().map(|w| w.0.as_str()).collect();
    let all = all.join(" ");
    let left = if centred { x - measure(&all, size).width / 2.0 } else { x };
    let mut before = String::new();
    for (w, c) in line {
        // The word starts where the text before it and the word end, less the word.
        let at = measure(&format!("{before}{w}"), size).width - measure(w, size).width;
        super::chrome::shadow_text(w, left + at, y, size, *c);
        before.push_str(w);
        before.push(' ');
    }
}

/// One row of a marked-up text (0x48e438) laid out in a box.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkupRow {
    pub text: String,
    pub ink: razdor::dt::markup::Ink,
    pub align: razdor::dt::markup::Align,
    /// The last row of its line: left aligned when the line is justified.
    pub last: bool,
}

/// A character's advance at `size`: `_` has the space's glyph (0x47866c).
pub fn glyph_width(c: char, size: f32) -> f32 {
    let c = if c == '_' { ' ' } else { c };
    measure(c.encode_utf8(&mut [0; 4]), size).width
}

/// The rows of `text` read as the original's markup (see [`razdor::dt::markup`]) in a box
/// `room` wide at `size`: each line word-wrapped as TextList.AddText 0x47e46c does.
pub fn markup_rows(text: &str, room: f32, size: f32) -> Vec<MarkupRow> {
    // The original draws its marked texts in Benguiat's tints (0x48e438).
    super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || markup_rows_in_face(text, room, size))
}

/// A plain text's pieces, as 0x47ea80 splits it: at CR LF or at `#\`, nothing else read.
fn plain_pieces(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some(i) = [rest.find("\r\n"), rest.find("#\\")].into_iter().flatten().min() {
        pieces.push(&rest[..i]);
        rest = &rest[i + 2..];
    }
    pieces.push(rest);
    pieces
}

/// A plain text's rows in a chosen font face, as List_AddWrappedText 0x47ea80 adds them: the
/// text split at CR LF or `#\`, each piece word-wrapped and justified (its last row left),
/// with no marks read and no indent added (only [`markup_rows`]' reader 0x48e1cc does those).
pub fn plain_rows_face(text: &str, room: f32, size: f32, face: super::dt_font::Face) -> Vec<MarkupRow> {
    use razdor::dt::markup::{Align, Ink};
    super::dt_font::with_face(face, || {
        super::dt_font::with_original_face(face, || {
            plain_pieces(text)
                .into_iter()
                .flat_map(|piece| {
                    razdor::dt::markup::wrap(piece, room, |c| glyph_width(c, size))
                        .into_iter()
                        .map(|r| MarkupRow { text: r.text, ink: Ink::Plain, align: Align::Justify, last: r.last })
                })
                .collect()
        })
    })
}

fn markup_rows_in_face(text: &str, room: f32, size: f32) -> Vec<MarkupRow> {
    razdor::dt::markup::parse(text)
        .into_iter()
        .flat_map(|line| {
            razdor::dt::markup::wrap(&line.text, room, |c| glyph_width(c, size))
                .into_iter()
                .map(move |r| MarkupRow { text: r.text, ink: line.ink, align: line.align, last: r.last })
        })
        .collect()
}

/// The colour of a markup font (the original's tints of Benguiat).
pub fn ink_color(ink: razdor::dt::markup::Ink) -> Color {
    let [r, g, b] = ink.rgb();
    Color::from_rgba(r, g, b, 255)
}

/// Draws a row of [`markup_rows`] in the box from `x`, `room` wide: centred, justified
/// (TextDrawJustified 0x478a9c) or, the last row of a justified line, left aligned. No
/// shadow: these lists get no shadow font (0x48e1cc passes 0). `_` draws as a space.
pub fn draw_markup_row(row: &MarkupRow, x: f32, y: f32, room: f32, size: f32) {
    draw_markup_row_face(row, x, y, room, size, super::dt_font::Face::Subtitle, None);
}

/// [`draw_markup_row`] in another of the original's fonts (the hero window's diary draws its
/// entry in SanSerif, 0x49c388).
pub fn draw_markup_row_face(row: &MarkupRow, x: f32, y: f32, room: f32, size: f32, face: super::dt_font::Face, color: Option<Color>) {
    let color = color.unwrap_or_else(|| ink_color(row.ink));
    super::dt_font::with_original_face(face, || draw_markup_row_in_face(row, x, y, room, size, &|s, x, y| text(s, x, y, size, color)));
}

/// [`draw_markup_row_face`] over the face's shadow font, as a text list line with a shadow
/// font set (0x47d2bc draws the shadow first, then the text): the original's shadow sheet
/// where the font is a sheet, else a dark copy 1 px down and right.
pub fn draw_markup_row_shadowed(row: &MarkupRow, x: f32, y: f32, room: f32, size: f32, face: super::dt_font::Face, color: Color) {
    super::dt_font::with_original_face(face, || {
        draw_markup_row_in_face(row, x, y, room, size, &|s, x, y| super::chrome::shadow_text(s, x, y, size, color));
    });
}

fn draw_markup_row_in_face(row: &MarkupRow, x: f32, y: f32, room: f32, size: f32, text: &dyn Fn(&str, f32, f32)) {
    use razdor::dt::markup::Align;
    let shown = row.text.replace('_', " ");
    match row.align {
        Align::Centre => {
            let w: f32 = row.text.chars().map(|c| glyph_width(c, size)).sum();
            text(&shown, x + ((room - w) / 2.0).trunc(), y);
        }
        Align::Justify if row.last || !row.text.contains(' ') => text(&shown, x, y),
        Align::Justify => {
            let xs = razdor::dt::markup::justify(&row.text, room, |c| glyph_width(c, size));
            let chars: Vec<char> = shown.chars().collect();
            let raw: Vec<char> = row.text.chars().collect();
            let mut i = 0;
            while i < raw.len() {
                if raw[i] == ' ' {
                    i += 1;
                    continue;
                }
                let j = (i..raw.len()).find(|&j| raw[j] == ' ').unwrap_or(raw.len());
                let word: String = chars[i..j].iter().collect();
                text(&word, x + xs[i], y);
                i = j;
            }
        }
    }
}

/// Colour of experience: bars, badges, level labels.
pub const XP_COLOR: Color = Color::new(0.35, 0.95, 0.95, 1.0);

/// "Lv 3 · XP 45/118".
pub fn level_label(level: i32, xp: i32, need: i32) -> String {
    razdor::trf!("Lv {level} · XP {xp}/{need}", level, xp, need)
}

// ------------------------------------------------------------------------------------------
// Form fields: text, numbers, check boxes, drop-down lists and tabs (the map editor's panels).
//
// Immediate mode with a little state kept here: the focused field (typing goes to it) and
// the open drop-down list, which is drawn last, over everything, by [`draw_popup`]. A
// screen using them calls [`fields_begin_frame`] first and [`fields_end_frame`] last.
// ------------------------------------------------------------------------------------------

/// The field that has the keyboard: its id, its caret and selection, and, for number
/// fields, the digits typed so far.
struct Focus {
    id: u64,
    buf: String,
    edit: TextEdit,
}

/// An open drop-down list.
struct Popup {
    id: u64,
    rect: Rect,
    options: Vec<(i64, String)>,
    scroll: usize,
    filter: String,
    edit: TextEdit,
    /// Its size against the editor's (rows, text): the game's windows pass `chrome::k()`.
    scale: f32,
}

local! {
    static FOCUS: RefCell<Option<Focus>> = const { RefCell::new(None) };
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// A choice made in the popup: (drop-down id, value), taken by that drop-down.
    static PICKED: std::cell::Cell<Option<(u64, i64)>> = const { std::cell::Cell::new(None) };
    /// Some field took this frame's click.
    static CLAIMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The editing keys held down and when each went down (for key repeat).
    static HELD: RefCell<Vec<(KeyCode, f64)>> = const { RefCell::new(Vec::new()) };
    /// The field whose text the mouse is selecting (pressed in it, not let go yet).
    static DRAG: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

pub const FIELD_BG: Color = Color::new(0.07, 0.07, 0.065, 1.0);
const POPUP_ROWS: usize = 12;
const POPUP_ROW_H: f32 = 22.0;

/// A stable id for a field from its key (unique per record and field, e.g. `"b12:name"`).
pub fn field_id(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

pub fn fields_begin_frame() {
    CLAIMED.with(|c| c.set(false));
}

/// A click nobody claimed takes the keyboard away from the focused field.
pub fn fields_end_frame() {
    if is_mouse_button_pressed(MouseButton::Left) && !CLAIMED.with(|c| c.get()) && !popup_open() {
        FOCUS.with(|f| *f.borrow_mut() = None);
    }
}

pub(super) fn claim() {
    CLAIMED.with(|c| c.set(true));
}

/// A text or number field (or the drop-down filter) has the keyboard: screens should not
/// treat keys as shortcuts.
pub fn typing() -> bool {
    FOCUS.with(|f| f.borrow().is_some()) || popup_open()
}

pub fn clear_focus() {
    FOCUS.with(|f| *f.borrow_mut() = None);
}

local! {
    /// A text field drew itself active this frame ([`want_keyboard`]).
    static KEYBOARD_WANTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The on-screen keyboard as last asked for ([`sync_keyboard`]).
    static KEYBOARD_SHOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The text of the active field this frame and its caret (a char index): the strip shows it.
    static KEYBOARD_TEXT: RefCell<(String, usize)> = const { RefCell::new((String::new(), 0)) };
}

/// A text field is active this frame: the on-screen keyboard should show ([`sync_keyboard`]),
/// and the strip ([`draw_keyboard_strip`]) shows its text `s` with the caret at char `caret`.
pub fn want_keyboard(s: &str, caret: usize) {
    KEYBOARD_WANTED.with(|k| k.set(true));
    KEYBOARD_TEXT.with(|t| *t.borrow_mut() = (s.to_string(), caret));
}

/// The keyboard request of this frame so far, its text and caret, if a field asked for it.
#[cfg(test)]
pub fn keyboard_request() -> Option<(String, usize)> {
    KEYBOARD_WANTED.with(|k| k.get()).then(|| KEYBOARD_TEXT.with(|t| t.borrow().clone()))
}

/// Clears the keyboard request, so a test starts from no request whatever ran before it.
#[cfg(test)]
pub fn reset_keyboard_request() {
    KEYBOARD_WANTED.with(|k| k.set(false));
}

/// What the player did to a name field this frame, for the phone keyboard ([`name_keyboard`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameTouch {
    /// Nothing that concerns the field.
    Nothing,
    /// A tap in the field's tap target ([`tap_rect`]).
    FieldTapped,
    /// Another save was picked from the list, which replaces the name.
    RowPicked,
    /// The system hid the on-screen keyboard (its own arrow, Back) since the last frame
    /// (`display::keyboard_closed_by_system`): the field stops typing.
    Hidden,
}

/// Whether a name field that is always drawn active keeps the on-screen keyboard this frame,
/// given whether it had it before (`typing`). The desktop has no on-screen keyboard and the
/// name is typed straight into the field: always. On a phone the keyboard comes only with a tap
/// in the field: a tap shows it, a pick of another save hides it, and nothing else changes it.
/// A window starts with `typing` false, so opening it does not pop the keyboard up.
pub fn name_keyboard(phone: bool, typing: bool, touch: NameTouch) -> bool {
    if !phone {
        return true;
    }
    match touch {
        NameTouch::FieldTapped => true,
        NameTouch::RowPicked | NameTouch::Hidden => false,
        NameTouch::Nothing => typing,
    }
}

/// What Enter does on a name window this frame (Enter is also the window's OK).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameEnter {
    /// No Enter this frame.
    Nothing,
    /// Enter is the window's OK.
    Confirm,
    /// A phone with the name field's keyboard up: Enter is the keyboard's own key. It closes the
    /// keyboard and the window stays.
    HideKeyboard,
}

/// The decision for Enter on a name window: on a phone whose name field has the keyboard up
/// (`typing`) Enter hides the keyboard; otherwise (the desktop, or the keyboard hidden) it is OK.
pub fn name_enter_decision(phone: bool, typing: bool, enter: bool) -> NameEnter {
    match (enter, phone && typing) {
        (false, _) => NameEnter::Nothing,
        (true, true) => NameEnter::HideKeyboard,
        (true, false) => NameEnter::Confirm,
    }
}

/// Enter on a name window this frame, given the field's keyboard flag (`typing`). Returns true
/// when it confirms the window. A phone's Enter with the keyboard up clears `typing`, which hides
/// the keyboard at the end of the frame ([`sync_keyboard`]) and confirms nothing.
pub fn name_enter(phone: bool, typing: &mut bool, enter: bool) -> bool {
    match name_enter_decision(phone, *typing, enter) {
        NameEnter::Confirm => true,
        NameEnter::HideKeyboard => {
            *typing = false;
            false
        }
        NameEnter::Nothing => false,
    }
}

/// The phone's name-field touch this frame: a tap in the field's tap target (`tap`) wins; else
/// the system hiding the keyboard ends the typing; else nothing.
pub fn name_touch(tapped: bool) -> NameTouch {
    if tapped {
        NameTouch::FieldTapped
    } else if super::display::keyboard_closed_by_system() {
        NameTouch::Hidden
    } else {
        NameTouch::Nothing
    }
}

/// A name field's tap target. On a phone it is at least 48 dp tall (`display::dp_px`), grown
/// from the field's top edge down (`up` false) or from its bottom edge up (`up` true); on the
/// desktop it is the field's own rect. Only the keyboard request uses it: the drawing does not.
pub fn tap_rect(r: Rect, up: bool) -> Rect {
    tap_rect_at(r, up, super::display::dp_px())
}

/// [`tap_rect`] with the phone's density given (`None`: the desktop).
pub fn tap_rect_at(r: Rect, up: bool, dp: Option<f32>) -> Rect {
    let Some(dp) = dp else { return r };
    let h = r.h.max(48.0 * dp);
    let y = if up { r.y + r.h - h } else { r.y };
    Rect::new(r.x, y, r.w, h)
}

/// The left button was pressed in `r` this frame.
#[track_caller]
pub fn tapped_in(r: Rect) -> bool {
    super::touch_probe::record("tap", r, true);
    is_mouse_button_pressed(MouseButton::Left) && mouse_in(r.x, r.y, r.w, r.h)
}

/// Shows or hides the on-screen keyboard (Android and iOS; the desktop has none).
#[cfg(any(target_os = "android", target_os = "ios"))]
fn set_keyboard(show: bool) {
    macroquad::miniquad::window::show_keyboard(show);
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn set_keyboard(_show: bool) {}

/// Once a frame, at its end: the on-screen keyboard follows the text fields. It shows while a
/// field is active and hides when none is. A tap in an active field shows it again, since the
/// player may have dismissed it with Back.
pub fn sync_keyboard() {
    let wanted = KEYBOARD_WANTED.with(|k| k.replace(false));
    let shown = KEYBOARD_SHOWN.with(|k| k.get());
    if wanted != shown || (wanted && is_mouse_button_pressed(MouseButton::Left)) {
        KEYBOARD_SHOWN.with(|k| k.set(wanted));
        set_keyboard(wanted);
    }
}

/// The line of `s` that holds char `caret`, and the caret's char index in that line.
fn caret_line(s: &str, caret: usize) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let caret = caret.min(chars.len());
    let start = chars[..caret].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
    let end = chars[caret..].iter().position(|&c| c == '\n').map_or(chars.len(), |i| caret + i);
    (chars[start..end].iter().collect(), caret - start)
}

/// Mobile only: while a field wants the keyboard, the line it types is drawn in a dark strip
/// across the top of the screen, over the frame. The keyboard covers the field on a small
/// screen; the strip keeps the typed text in sight. Called before [`sync_keyboard`].
pub fn draw_keyboard_strip() {
    if !razdor::platform::MOBILE || !KEYBOARD_WANTED.with(|k| k.get()) {
        return;
    }
    let (line, caret) = KEYBOARD_TEXT.with(|t| {
        let t = t.borrow();
        caret_line(&t.0, t.1)
    });
    let k = super::chrome::bar_k();
    let size = 17.0 * k;
    // The strip sits inside the safe area: away from the rounded corners, the cutout and the bars.
    let (left, top, right, _) = razdor::platform::safe_insets();
    let (x0, y0) = (left, top);
    let (w, h, pad) = (screen_width() - left - right, (size * 2.0).round(), 8.0 * k);
    draw_rectangle(x0, y0, w, h, Color::new(0.0, 0.0, 0.0, 0.8));
    let advance = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
    let xs = text_edit::caret_xs(&line, advance);
    let room = w - 2.0 * pad;
    let first = text_edit::first_shown(&xs, caret, room);
    let n = xs.len() - 1;
    let last = (first..=n).rev().find(|&i| xs[i] - xs[first] <= room).unwrap_or(first);
    let x_of = |i: usize| x0 + pad + xs[i] - xs[first];
    // The baseline that centres the text in the strip (as [`edit_line`] places a caret).
    let y = y0 + h / 2.0 + size * 0.325;
    let part: String = line.chars().skip(first).take(last - first).collect();
    text(&part, x0 + pad, y, size, INK);
    if caret_on() && (first..=last).contains(&caret) {
        let cx = x_of(caret).round();
        draw_line(cx, y - size * 0.8, cx, y + size * 0.15, (size / 12.0).max(1.0), INK);
    }
}

/// The field of `key` ([`field_id`]) has the keyboard.
pub fn has_focus(key: &str) -> bool {
    focused(field_id(key))
}

/// Gives the keyboard to the field of `key` (a field that draws itself, like the inventory
/// filter).
pub fn take_focus(key: &str) {
    set_focus(field_id(key), String::new());
}

fn focused(id: u64) -> bool {
    FOCUS.with(|f| f.borrow().as_ref().is_some_and(|f| f.id == id))
}

fn set_focus(id: u64, buf: String) {
    FOCUS.with(|f| *f.borrow_mut() = Some(Focus { id, buf, edit: TextEdit::at_end() }));
}

/// The caret and selection of the focused field `id` (the end when it has none).
fn edit_of(id: u64) -> TextEdit {
    FOCUS.with(|f| f.borrow().as_ref().filter(|f| f.id == id).map_or(TextEdit::at_end(), |f| f.edit))
}

/// Keeps `edit` as the caret and selection of field `id`, if it still has the keyboard.
fn store_edit(id: u64, edit: TextEdit) {
    FOCUS.with(|f| {
        if let Some(f) = f.borrow_mut().as_mut().filter(|f| f.id == id) {
            f.edit = edit;
        }
    });
}

/// The caret and selection of the field of `key` (a field that draws itself, like the
/// inventory filter).
pub fn focus_edit(key: &str) -> TextEdit {
    edit_of(field_id(key))
}

pub fn set_focus_edit(key: &str, edit: TextEdit) {
    store_edit(field_id(key), edit);
}

/// Parses a typed number, clamped to `min..=max`. Empty or `-` alone gives `None`.
pub fn parse_number(text: &str, min: i64, max: i64) -> Option<i64> {
    text.trim().parse::<i64>().ok().map(|v| v.clamp(min, max))
}

/// `k` pressed this frame (1), or its repeats while held ([`repeats`]). Call it once a frame
/// for each key.
pub fn key_repeat(k: KeyCode) -> usize {
    if is_key_pressed(k) {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            h.retain(|e| e.0 != k);
            h.push((k, crate::ui::clock::now()));
        });
        return 1;
    }
    let down = is_key_down(k);
    let t0 = HELD.with(|h| {
        let mut h = h.borrow_mut();
        if !down {
            h.retain(|e| e.0 != k);
        }
        h.iter().find(|e| e.0 == k).map(|e| e.1)
    });
    t0.map_or(0, |t0| repeats(crate::ui::clock::now() - t0, crate::ui::clock::frame_dt() as f64))
}

/// A held key's repeats in the frame of `dt` seconds ending `held` seconds after it went
/// down. The original's edit box (0x47a624) takes one Backspace per WM_KEYDOWN its window
/// procedure stores as the held key (0x475748, which never looks at the repeat bit), so a
/// held key repeats at the system's typematic rate: Windows' defaults, a first repeat after
/// 500 ms and then about 30 a second.
pub fn repeats(held: f64, dt: f64) -> usize {
    const DELAY: f64 = 0.5;
    const RATE: f64 = 30.0;
    let t = held - DELAY;
    if t < 0.0 {
        return 0;
    }
    // Repeats due by `t` (the first at 0), a hair over so that frame times summing to a
    // whole step do not lose it to rounding.
    let due = |t: f64| (t * RATE + 1e-6).floor().max(-1.0);
    (due(t) - due(t - dt)).max(0.0) as usize
}

/// Ctrl (or a Mac's Command) is held without Alt: keys are shortcuts, not text (Windows'
/// AltGr comes as Ctrl+Alt, and types).
pub fn shortcut_held() -> bool {
    let ctrl = [KeyCode::LeftControl, KeyCode::RightControl, KeyCode::LeftSuper, KeyCode::RightSuper].into_iter().any(is_key_down);
    ctrl && !is_key_down(KeyCode::LeftAlt) && !is_key_down(KeyCode::RightAlt)
}

/// This frame's typed characters (none while [`shortcut_held`]: Ctrl+V is no `v`).
pub fn typed_chars() -> Vec<char> {
    let mut chars = Vec::new();
    while let Some(c) = get_char_pressed() {
        chars.push(c);
    }
    if shortcut_held() {
        chars.clear();
    }
    chars
}

/// What a field takes.
#[derive(Clone, Copy)]
pub struct EditRules {
    pub max_chars: usize,
    /// Enter and pasted line breaks break the line.
    pub multiline: bool,
    /// The characters it takes (typed or pasted).
    pub allow: fn(char) -> bool,
}

impl EditRules {
    /// One line of any text, at most `max_chars` long.
    pub fn line(max_chars: usize) -> EditRules {
        EditRules { max_chars, multiline: false, allow: |_| true }
    }
}

/// What a frame's keys did to an edited text.
#[derive(Clone, Copy, Debug, Default)]
pub struct Edited {
    pub changed: bool,
    pub enter: bool,
}

/// Applies this frame's keys to `s` as a normal edit box does (Razdor's own: the original's
/// edit box only adds and deletes at the end, interface.md §5): the `chars` typed go in at
/// the caret, over the selection; Backspace, Delete, Left and Right repeat while held; Shift
/// with Left, Right, Home and End selects; Ctrl+A selects all; Ctrl+C and Ctrl+X copy and
/// cut the selection, Ctrl+V (or Shift+Insert) pastes; Enter breaks the line in a
/// multi-line field.
pub fn edit_keys(s: &mut String, ed: &mut TextEdit, chars: &[char], rules: EditRules) -> Edited {
    use macroquad::miniquad::window::{clipboard_get, clipboard_set};
    ed.fit(s);
    let before = s.clone();
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    let cmd = shortcut_held();
    if cmd && is_key_pressed(KeyCode::A) {
        ed.select_all(s);
    }
    if cmd && (is_key_pressed(KeyCode::C) || is_key_pressed(KeyCode::Insert)) && ed.has_selection() {
        clipboard_set(ed.selected(s));
    }
    if cmd && is_key_pressed(KeyCode::X) {
        if let Some(cut) = ed.cut(s) {
            clipboard_set(&cut);
        }
    }
    if (cmd && is_key_pressed(KeyCode::V)) || (shift && !cmd && is_key_pressed(KeyCode::Insert)) {
        if let Some(pasted) = clipboard_get() {
            let pasted: String = text_edit::clean(&pasted, rules.multiline).chars().filter(|&c| c == '\n' || (rules.allow)(c)).collect();
            ed.paste(s, &pasted, rules.max_chars, rules.multiline);
        }
    }
    let typed: Vec<char> = chars.iter().copied().filter(|&c| (rules.allow)(c)).collect();
    ed.type_chars(s, &typed, rules.max_chars);
    for _ in 0..key_repeat(KeyCode::Backspace) {
        ed.backspace(s);
    }
    for _ in 0..key_repeat(KeyCode::Delete) {
        ed.delete(s);
    }
    for _ in 0..key_repeat(KeyCode::Left) {
        ed.left(s, shift);
    }
    for _ in 0..key_repeat(KeyCode::Right) {
        ed.right(s, shift);
    }
    if is_key_pressed(KeyCode::Home) {
        ed.home(s, shift);
    }
    if is_key_pressed(KeyCode::End) {
        ed.end(s, shift);
    }
    let enter = enter_pressed();
    if enter && rules.multiline {
        ed.replace(s, "\n", rules.max_chars);
    }
    Edited { changed: *s != before, enter }
}

/// How an edited line is drawn.
#[derive(Clone, Copy)]
pub struct LineLook {
    pub size: f32,
    pub color: Color,
    /// Centred in its box (else from its left edge).
    pub centred: bool,
    /// With the interface's shadow (`chrome::shadow_text`).
    pub shadow: bool,
}

/// The band behind selected text.
pub const SELECTION: Color = Color::new(0.20, 0.32, 0.62, 0.85);

/// The caret's blink: on half of each second.
fn caret_on() -> bool {
    (crate::ui::clock::now() * 2.0) as i64 % 2 == 0
}

/// Caret and selection by the mouse for the field `id` drawn in `hit`: a press in it puts
/// the caret at `at(x, y)` (Shift: selects to there), and dragging on selects.
fn edit_mouse(id: u64, ed: &mut TextEdit, s: &str, hit: Rect, at: impl Fn(f32, f32) -> usize) {
    let (mx, my) = pointer();
    let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    if clicked() && hit.contains(vec2(mx, my)) {
        ed.move_to(s, at(mx, my), shift);
        DRAG.with(|d| d.set(Some(id)));
    } else if is_mouse_button_down(MouseButton::Left) && DRAG.with(|d| d.get()) == Some(id) {
        ed.move_to(s, at(mx, my), true);
    }
    if !is_mouse_button_down(MouseButton::Left) && DRAG.with(|d| d.get()) == Some(id) {
        DRAG.with(|d| d.set(None));
    }
}

/// Draws the line `s` in `r`, on the baseline `y`: the part that keeps the caret in sight,
/// and when `active` the selection's band and the blinking caret. With `mouse`, a press in
/// `r` places the caret and a drag selects ([`edit_mouse`]).
#[allow(clippy::too_many_arguments)]
pub fn edit_line(id: u64, s: &str, ed: &mut TextEdit, active: bool, mouse: bool, r: Rect, y: f32, look: LineLook) {
    edit_line_kb(id, s, ed, active, mouse, r, y, look, active);
}

/// [`edit_line`] whose active line asks for the on-screen keyboard only when `keyboard` (a
/// phone's name field, see [`name_keyboard`]); it is drawn the same either way.
#[allow(clippy::too_many_arguments)]
pub fn edit_line_kb(id: u64, s: &str, ed: &mut TextEdit, active: bool, mouse: bool, r: Rect, y: f32, look: LineLook, keyboard: bool) {
    if active && keyboard {
        want_keyboard(s, ed.caret);
    }
    ed.fit(s);
    let size = look.size;
    let xs = text_edit::caret_xs(s, |c| measure(c.encode_utf8(&mut [0; 4]), size).width);
    let n = xs.len() - 1;
    let first = text_edit::first_shown(&xs, if active { ed.caret } else { n }, r.w);
    let last = (first..=n).rev().find(|&i| xs[i] - xs[first] <= r.w).unwrap_or(first);
    let x0 = if look.centred { r.center().x - (xs[last] - xs[first]) / 2.0 } else { r.x };
    let x_of = |i: usize| x0 + xs[i] - xs[first];
    if active && mouse {
        let shown: Vec<f32> = xs[first..=last].iter().map(|x| x - xs[first]).collect();
        edit_mouse(id, ed, s, r, |mx, _| first + text_edit::index_at(&shown, mx - x0));
    }
    if active && ed.has_selection() {
        let sel = ed.selection();
        let (a, b) = (sel.start.clamp(first, last), sel.end.clamp(first, last));
        if b > a {
            draw_rectangle(x_of(a), y - size * 0.82, x_of(b) - x_of(a), size * 1.05, SELECTION);
        }
    }
    let part: String = s.chars().skip(first).take(last - first).collect();
    if look.shadow {
        super::chrome::shadow_text(&part, x0, y, size, look.color);
    } else {
        text(&part, x0, y, size, look.color);
    }
    if active && caret_on() && (first..=last).contains(&ed.caret) {
        let cx = x_of(ed.caret).round();
        draw_line(cx, y - size * 0.8, cx, y + size * 0.15, (size / 12.0).max(1.0), look.color);
    }
}

/// Draws the multi-line text `s` in `r`, wrapped, `pitch` apart: the rows that keep the
/// caret in sight, and when `active` the selection's band, the blinking caret, and the
/// mouse placing the caret and selecting ([`edit_mouse`]).
#[allow(clippy::too_many_arguments)]
pub fn edit_rows(id: u64, s: &str, ed: &mut TextEdit, active: bool, r: Rect, size: f32, pitch: f32, color: Color) {
    if active {
        want_keyboard(s, ed.caret);
    }
    ed.fit(s);
    let advance = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
    let rows = text_edit::wrap_rows(s, r.w, advance);
    let chars: Vec<char> = s.chars().collect();
    let fit = (r.h / pitch).floor().max(1.0) as usize;
    let skip = if active { (text_edit::row_of(&rows, ed.caret) + 1).saturating_sub(fit) } else { 0 };
    let row_text = |i: usize| chars[rows[i].clone()].iter().collect::<String>();
    if active {
        edit_mouse(id, ed, s, r, |mx, my| {
            let i = (skip + ((my - r.y) / pitch).floor().max(0.0) as usize).min(rows.len() - 1);
            rows[i].start + text_edit::index_at(&text_edit::caret_xs(&row_text(i), advance), mx - r.x)
        });
    }
    let caret_row = text_edit::row_of(&rows, ed.caret);
    let sel = ed.selection();
    for (j, i) in (skip..rows.len()).take(fit).enumerate() {
        let y = r.y + size * 0.82 + j as f32 * pitch;
        let row = rows[i].clone();
        let xs = text_edit::caret_xs(&row_text(i), advance);
        if active && ed.has_selection() {
            let (a, b) = (sel.start.clamp(row.start, row.end), sel.end.clamp(row.start, row.end));
            if b > a {
                draw_rectangle(r.x + xs[a - row.start], y - size * 0.82, xs[b - row.start] - xs[a - row.start], size * 1.05, SELECTION);
            }
        }
        text(&row_text(i), r.x, y, size, color);
        if active && caret_on() && i == caret_row {
            let cx = (r.x + xs[ed.caret.clamp(row.start, row.end) - row.start]).round();
            draw_line(cx, y - size * 0.8, cx, y + size * 0.15, (size / 12.0).max(1.0), color);
        }
    }
}

/// A text field. Click to type (any script the keyboard gives, Cyrillic included); Enter
/// ends a one-line field or breaks the line in a multi-line one; Esc or a click elsewhere
/// ends typing. Edits `value` as it is typed; true when it changed.
pub fn text_field(key: &str, x: f32, y: f32, w: f32, h: f32, value: &mut String, multiline: bool) -> bool {
    let id = field_id(key);
    let hover = mouse_in(x, y, w, h);
    if hover && clicked() {
        claim();
        if !focused(id) {
            set_focus(id, String::new());
        }
    }
    let active = focused(id);
    let mut ed = edit_of(id);
    let mut changed = false;
    if active && !input_blocked() {
        let out = edit_keys(value, &mut ed, &typed_chars(), EditRules { max_chars: 2000, multiline, allow: |_| true });
        changed = out.changed;
        if (out.enter && !multiline) || esc_pressed() {
            clear_focus();
        }
    }
    let active = focused(id);
    draw_rectangle(x, y, w, h, FIELD_BG);
    draw_rectangle_lines(x, y, w, h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else if hover { INK } else { DIM });
    let size = 17.0;
    if multiline {
        edit_rows(id, value, &mut ed, active, Rect::new(x + 5.0, y + 3.0, w - 10.0, h - 6.0), size, 19.0, INK);
    } else {
        // A long line shows its end, or the part around the caret while typed in.
        let look = LineLook { size, color: INK, centred: false, shadow: false };
        edit_line(id, value, &mut ed, active, true, Rect::new(x + 5.0, y, w - 10.0, h), y + (h + 12.0) / 2.0, look);
    }
    store_edit(id, ed);
    changed
}

/// A whole number between `min` and `max`: − and + step it (Shift: by 10), the wheel over
/// it too, and a click on the number lets you type one (Enter or a click elsewhere takes
/// it). Returns the new value when it changed.
#[track_caller]
pub fn number_field(key: &str, x: f32, y: f32, w: f32, value: i64, min: i64, max: i64) -> Option<i64> {
    let id = field_id(key);
    let h = 24.0;
    let bw = 22.0;
    let step = if is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift) { 10 } else { 1 };
    let mut out = None;
    let mut focusing = false;
    let clamp = |v: i64| v.clamp(min, max);
    if small_button(x, y, bw, h, "-", value > min) {
        claim();
        out = Some(clamp(value - step));
    }
    if small_button(x + w - bw, y, bw, h, "+", value < max) {
        claim();
        out = Some(clamp(value + step));
    }
    let (tx, tw) = (x + bw + 2.0, w - 2.0 * bw - 4.0);
    let hover = mouse_in(tx, y, tw, h);
    if hover {
        let wh = wheel();
        if wh != 0.0 {
            out = Some(clamp(value + if wh > 0.0 { step } else { -step }));
        }
        if clicked() {
            claim();
            if !focused(id) {
                // The number as it stands, all of it selected: typing replaces it.
                set_focus(id, value.to_string());
                focusing = true;
            }
        }
    }
    let active = focused(id);
    let mut shown = value.to_string();
    let mut ed = edit_of(id);
    if active && !input_blocked() {
        let mut buf = FOCUS.with(|f| f.borrow().as_ref().map(|f| f.buf.clone()).unwrap_or_default());
        let allow: fn(char) -> bool = if min < 0 { |c| c.is_ascii_digit() || c == '-' } else { |c| c.is_ascii_digit() };
        let keys = edit_keys(&mut buf, &mut ed, &typed_chars(), EditRules { max_chars: 7, multiline: false, allow });
        let away = clicked() && !hover;
        if keys.enter || away {
            if let Some(v) = parse_number(&buf, min, max) {
                out = Some(v);
            }
            clear_focus();
        } else if esc_pressed() {
            clear_focus();
        } else {
            FOCUS.with(|f| {
                if let Some(f) = f.borrow_mut().as_mut() {
                    f.buf = buf.clone();
                }
            });
            shown = buf;
        }
    }
    let active = focused(id);
    draw_rectangle(tx, y, tw, h, FIELD_BG);
    draw_rectangle_lines(tx, y, tw, h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else if hover { INK } else { DIM });
    let look = LineLook { size: 17.0, color: INK, centred: true, shadow: false };
    edit_line(id, &shown, &mut ed, active, true, Rect::new(tx + 4.0, y, tw - 8.0, h), y + 17.0, look);
    if focusing {
        ed = TextEdit::all(&shown);
    }
    store_edit(id, ed);
    out.filter(|v| *v != value)
}

/// A compact button (no sound, for steppers and list rows).
#[track_caller]
pub fn small_button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    let r = Rect::new(x, y, w, h);
    let (_, down, pressed) = button_state(r, enabled);
    let bg = match (enabled, down) {
        (false, _) => Color::new(0.16, 0.16, 0.16, 1.0),
        (true, true) => Color::new(0.45, 0.35, 0.2, 1.0),
        (true, false) => Color::new(0.26, 0.21, 0.14, 1.0),
    };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 1.0, if enabled { DIM } else { Color::new(0.3, 0.3, 0.3, 1.0) });
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        let size = fit_size(label, w - 6.0, 17.0);
        let label = &ellipsize(label, w - 6.0, size);
        let d = measure(label, size);
        text(label, x + (w - d.width) / 2.0, y + (h + d.offset_y) / 2.0 - 1.0, size, if enabled { INK } else { DIM });
    });
    pressed
}

/// A button that stays lit while `on` (tool and mode pickers). True when clicked.
#[track_caller]
pub fn toggle_button(x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let (hover, down, pressed) = button_state(Rect::new(x, y, w, h), true);
    let bg = if on { Color::new(0.55, 0.42, 0.18, 1.0) } else if down { Color::new(0.36, 0.28, 0.18, 1.0) } else if hover { Color::new(0.28, 0.22, 0.14, 1.0) } else { Color::new(0.22, 0.18, 0.12, 1.0) };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 1.0, if on { ACCENT } else { DIM });
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        let size = fit_size(label, w - 6.0, 17.0);
        let label = &ellipsize(label, w - 6.0, size);
        let d = measure(label, size);
        text(label, x + (w - d.width) / 2.0, y + (h + d.offset_y) / 2.0 - 1.0, size, INK);
    });
    if pressed {
        claim();
    }
    pressed
}

/// A check box with its label, at most `max_w` wide (a long label shrinks, then is
/// shortened); returns the new state when clicked.
pub fn checkbox(x: f32, y: f32, max_w: f32, label: &str, value: bool) -> Option<bool> {
    let size = fit_size(label, max_w - 28.0, 17.0);
    let label = &ellipsize(label, max_w - 28.0, size);
    let w = 22.0 + measure(label, size).width + 6.0;
    let hover = mouse_in(x, y, w, 22.0);
    draw_rectangle(x, y + 2.0, 18.0, 18.0, FIELD_BG);
    draw_rectangle_lines(x, y + 2.0, 18.0, 18.0, 1.0, if hover { INK } else { DIM });
    if value {
        draw_line(x + 4.0, y + 11.0, x + 8.0, y + 16.0, 2.5, ACCENT);
        draw_line(x + 8.0, y + 16.0, x + 15.0, y + 5.0, 2.5, ACCENT);
    }
    text(label, x + 24.0, y + 16.0, size, INK);
    if hover && clicked() {
        claim();
        return Some(!value);
    }
    None
}

/// A check box beside its label on one line: the box left, the label right at `size` exactly
/// (not the font's native size, never shrunk). The box is 0.8 of the label size, at
/// least 18 px (the settings' check box). The whole row is the target: on a phone it grows
/// upward to 48 dp ([`tap_rect`]), so `r` is the row's place and its bottom stays. Returns the
/// new state when clicked.
pub fn checkbox_row(r: Rect, size: f32, label: &str, value: bool) -> Option<bool> {
    let side = (size * 0.8).max(18.0);
    let hit = tap_rect(Rect::new(r.x, r.y, r.w, side), true);
    let (hover, _down, pressed) = button_state(hit, true);
    draw_rectangle(r.x, r.y + r.h - side, side, side, FIELD_BG);
    draw_rectangle_lines(r.x, r.y + r.h - side, side, side, 1.0, if hover { INK } else { DIM });
    if value {
        let (bx, by, k) = (r.x, r.y + r.h - side, side / 18.0);
        draw_line(bx + 4.0 * k, by + 11.0 * k, bx + 8.0 * k, by + 16.0 * k, 2.5 * k, ACCENT);
        draw_line(bx + 8.0 * k, by + 16.0 * k, bx + 15.0 * k, by + 5.0 * k, 2.5 * k, ACCENT);
    }
    let label = ellipsize(label, (r.w - side - 8.0).max(0.0), size);
    text_exact(&label, r.x + side + 8.0, r.y + r.h - side / 2.0 + size * 0.35, size, INK);
    if pressed {
        claim();
        return Some(!value);
    }
    None
}

/// A drop-down list: shows the option with value `current` (or its number) and opens a list
/// to pick from (type to filter, wheel to scroll). Returns the value picked.
pub fn dropdown(key: &str, x: f32, y: f32, w: f32, current: i64, options: &[(i64, String)]) -> Option<i64> {
    let picked = take_pick(key);
    let h = 24.0;
    let hover = mouse_in(x, y, w, h);
    let label = options.iter().find(|o| o.0 == current).map_or(format!("#{current}"), |o| o.1.clone());
    draw_rectangle(x, y, w, h, FIELD_BG);
    draw_rectangle_lines(x, y, w, h, 1.0, if hover { INK } else { DIM });
    let mut shown = label;
    while measure(&shown, 16.0).width > w - 24.0 && !shown.is_empty() {
        shown.pop();
    }
    text(&shown, x + 5.0, y + 17.0, 16.0, INK);
    draw_triangle(vec2(x + w - 15.0, y + 9.0), vec2(x + w - 5.0, y + 9.0), vec2(x + w - 10.0, y + 16.0), DIM);
    if hover && clicked() {
        open_list(key, Rect::new(x, y, w, h), current, options, 1.0);
    }
    picked.filter(|v| *v != current)
}

/// Opens the drop-down list of `key` under `anchor` (over it when there is no room below),
/// sized by `scale`; the value picked comes from [`take_pick`]. For a drop-down drawn its own
/// way (the game's language list).
pub fn open_list(key: &str, anchor: Rect, current: i64, options: &[(i64, String)], scale: f32) {
    claim();
    clear_focus();
    let rows = options.len().min(POPUP_ROWS) as f32;
    let ph = (rows * POPUP_ROW_H + 30.0) * scale;
    let (y, h) = (anchor.y, anchor.h);
    let py = if y + h + ph > screen_height() { (y - ph).max(0.0) } else { y + h };
    let pw = anchor.w.max(220.0 * scale);
    let px = anchor.x.min(screen_width() - pw).max(0.0);
    let at = options.iter().position(|o| o.0 == current).unwrap_or(0);
    let popup = Popup { id: field_id(key), rect: Rect::new(px, py, pw, ph), options: options.to_vec(), scroll: at.saturating_sub(POPUP_ROWS / 2), filter: String::new(), edit: TextEdit::at_end(), scale };
    POPUP.with(|p| *p.borrow_mut() = Some(popup));
}

/// The value picked in the drop-down list of `key` since the last call, if any.
pub fn take_pick(key: &str) -> Option<i64> {
    let id = field_id(key);
    PICKED.with(|p| match p.get() {
        Some((pid, v)) if pid == id => {
            p.set(None);
            Some(v)
        }
        _ => None,
    })
}

/// The drop-down list of `key` is open.
pub fn list_open(key: &str) -> bool {
    POPUP.with(|p| p.borrow().as_ref().is_some_and(|pop| pop.id == field_id(key)))
}

pub fn popup_open() -> bool {
    POPUP.with(|p| p.borrow().is_some())
}

/// Draws the open drop-down list over everything and handles it. Call last in the frame,
/// with input not blocked.
pub fn draw_popup() {
    let mut close = false;
    POPUP.with(|p| {
        let mut p = p.borrow_mut();
        let Some(pop) = p.as_mut() else { return };
        let r = pop.rect;
        super::chrome::note_window(r);
        // Typing filters; Backspace deletes from the filter.
        edit_keys(&mut pop.filter, &mut pop.edit, &typed_chars(), EditRules::line(40));
        // The filter is typed in while the list is open: it asks for the keyboard as a field does.
        want_keyboard(&pop.filter, pop.edit.caret);
        let needle = pop.filter.to_lowercase();
        let shown: Vec<&(i64, String)> = pop.options.iter().filter(|o| needle.is_empty() || o.1.to_lowercase().contains(&needle) || o.0.to_string() == needle).collect();
        let max_scroll = shown.len().saturating_sub(POPUP_ROWS);
        if r.contains(Vec2::from(pointer())) {
            let wh = mouse_wheel().1;
            if wh > 0.0 {
                pop.scroll = pop.scroll.saturating_sub(3);
            } else if wh < 0.0 {
                pop.scroll += 3;
            }
        }
        pop.scroll = pop.scroll.min(max_scroll);
        draw_rectangle(r.x + 4.0, r.y + 4.0, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.4));
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.09, 0.08, 0.98));
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
        let s = pop.scale;
        let row_h = POPUP_ROW_H * s;
        let hint = if pop.filter.is_empty() { razdor::i18n::tr("type to filter").to_string() } else { razdor::trf!("filter: {filter}", filter = pop.filter) };
        text(&hint, r.x + 8.0 * s, r.y + 19.0 * s, 15.0 * s, DIM);
        for (i, (v, label)) in shown.iter().skip(pop.scroll).take(POPUP_ROWS).enumerate() {
            let ry = r.y + 26.0 * s + i as f32 * row_h;
            let hover = Rect::new(r.x, ry, r.w, row_h).contains(Vec2::from(pointer()));
            if hover {
                draw_rectangle(r.x + 2.0, ry, r.w - 4.0, row_h, Color::new(0.4, 0.3, 0.15, 1.0));
            }
            let mut l = label.clone();
            while measure(&l, 16.0 * s).width > r.w - 16.0 * s && !l.is_empty() {
                l.pop();
            }
            text(&l, r.x + 8.0 * s, ry + 16.0 * s, 16.0 * s, INK);
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                PICKED.with(|k| k.set(Some((pop.id, *v))));
                close = true;
            }
        }
        if shown.len() > POPUP_ROWS {
            let frac = pop.scroll as f32 / max_scroll.max(1) as f32;
            let track = r.h - 30.0 * s;
            draw_rectangle(r.x + r.w - 5.0, r.y + 26.0 * s + frac * (track - 20.0 * s), 3.0, 20.0 * s, DIM);
        }
        let outside = is_mouse_button_pressed(MouseButton::Left) && !r.contains(Vec2::from(pointer()));
        if outside || esc_pressed() {
            close = true;
        }
    });
    if close {
        POPUP.with(|p| *p.borrow_mut() = None);
        claim();
    }
}

/// A row of tabs; the clicked one becomes `selected`. Returns the height used.
#[track_caller]
pub fn tabs(x: f32, y: f32, w: f32, labels: &[&str], selected: &mut usize) -> f32 {
    let n = labels.len().max(1) as f32;
    let tw = w / n;
    for (i, l) in labels.iter().enumerate() {
        if toggle_button(x + i as f32 * tw, y, tw - 2.0, 26.0, l, *selected == i) {
            *selected = i;
        }
    }
    30.0
}

local! {
    /// A scrollbar's knob held by the mouse: the bar's rectangle (to know which bar) and the
    /// knob's grab point below its top.
    static SCROLL_DRAG: std::cell::Cell<Option<(Rect, f32)>> = const { std::cell::Cell::new(None) };
}

/// A vertical scrollbar in the original's art (`VArrowUp` / `VArrowDown`, the `VSTexture`
/// track, the `VSlider` knob) for a list of `total` rows showing `shown` from `first`. The
/// arrows move one row, a click on the track a page, and the knob drags. Nothing is drawn
/// when everything fits. Returns true when `first` changed.
pub fn scrollbar(r: Rect, first: &mut usize, shown: usize, total: usize) -> bool {
    use super::chrome;
    if total <= shown {
        return false;
    }
    let most = total - shown;
    let before = *first;
    let w = r.w;
    let up = Rect::new(r.x, r.y, w, w);
    let down = Rect::new(r.x, r.y + r.h - w, w, w);
    let track = Rect::new(r.x, r.y + w, w, r.h - 2.0 * w);
    match chrome::win("VSTexture") {
        Some(t) => chrome::tex(&t, track, WHITE),
        None => draw_rectangle(track.x + w / 3.0, track.y, w / 3.0, track.h, chrome::SILVER_DARK),
    }
    let knob_h = chrome::win("VSlider").map_or(w * 1.8, |t| w * t.height() / t.width()).min(track.h / 2.0);
    let room = track.h - knob_h;
    let at = |first: usize| track.y + room * first as f32 / most as f32;
    let (mx, my) = pointer();
    let free = !input_blocked() && !input_swallowed();
    let held = is_mouse_button_down(MouseButton::Left);
    // Dragging the knob: the row whose place is nearest.
    let drag = SCROLL_DRAG.with(|d| d.get()).filter(|(dr, _)| *dr == r);
    if let (Some((_, grab)), true) = (drag, held) {
        let t = ((my - grab - track.y) / room).clamp(0.0, 1.0);
        *first = (t * most as f32).round() as usize;
    } else if drag.is_some() {
        SCROLL_DRAG.with(|d| d.set(None));
    }
    let knob = Rect::new(track.x, at((*first).min(most)), w, knob_h);
    if free && is_mouse_button_pressed(MouseButton::Left) {
        let p = vec2(mx, my);
        if knob.contains(p) {
            SCROLL_DRAG.with(|d| d.set(Some((r, my - knob.y))));
        } else if up.contains(p) {
            *first = first.saturating_sub(1);
        } else if down.contains(p) {
            *first = (*first + 1).min(most);
        } else if track.contains(p) {
            *first = if my < knob.y { first.saturating_sub(shown) } else { (*first + shown).min(most) };
        }
    }
    *first = (*first).min(most);
    let knob = Rect::new(track.x, at(*first), w, knob_h);
    for (name, rect, pressed) in [("VArrowUp", up, held && up.contains(vec2(mx, my))), ("VArrowDown", down, held && down.contains(vec2(mx, my)))] {
        let tint = if pressed { Color::new(0.8, 0.8, 0.8, 1.0) } else { WHITE };
        match chrome::win(name) {
            Some(t) => chrome::tex(&t, rect, tint),
            None => draw_rectangle(rect.x, rect.y, rect.w, rect.h, chrome::SILVER),
        }
    }
    match chrome::win("VSlider") {
        Some(t) => chrome::tex(&t, knob, WHITE),
        None => draw_rectangle(knob.x, knob.y, knob.w, knob.h, chrome::SILVER),
    }
    *first != before
}

/// [`scrollbar`], drawn even when everything fits: then idle, the knob at the top and the
/// clicks doing nothing, as the original's lists that always show their bar (the building
/// window's quests, 0x4bedec).
pub fn scrollbar_always(r: Rect, first: &mut usize, shown: usize, total: usize) -> bool {
    use super::chrome;
    if total > shown {
        return scrollbar(r, first, shown, total);
    }
    let w = r.w;
    let track = Rect::new(r.x, r.y + w, w, r.h - 2.0 * w);
    match chrome::win("VSTexture") {
        Some(t) => chrome::tex(&t, track, WHITE),
        None => draw_rectangle(track.x + w / 3.0, track.y, w / 3.0, track.h, chrome::SILVER_DARK),
    }
    for (name, rect) in [("VArrowUp", Rect::new(r.x, r.y, w, w)), ("VArrowDown", Rect::new(r.x, r.y + r.h - w, w, w))] {
        match chrome::win(name) {
            Some(t) => chrome::tex(&t, rect, WHITE),
            None => draw_rectangle(rect.x, rect.y, rect.w, rect.h, chrome::SILVER),
        }
    }
    let knob_h = chrome::win("VSlider").map_or(w * 1.8, |t| w * t.height() / t.width()).min(track.h / 2.0);
    let knob = Rect::new(track.x, track.y, w, knob_h);
    match chrome::win("VSlider") {
        Some(t) => chrome::tex(&t, knob, WHITE),
        None => draw_rectangle(knob.x, knob.y, knob.w, knob.h, chrome::SILVER),
    }
    *first = 0;
    false
}

/// A pixel scroll of `scroll` (of `most`, with `view` in sight) as a scrollbar's (first,
/// shown, total) in steps of `step`: the content's end is the bar's end.
fn bar_steps(scroll: f32, most: f32, view: f32, step: f32) -> (usize, usize, usize) {
    let shown = ((view / step).floor() as usize).max(1);
    let steps = (most / step).ceil() as usize;
    let first = if scroll >= most { steps } else { (scroll / step).round() as usize };
    (first.min(steps), shown, shown + steps)
}

/// [`scrollbar`] for content scrolled in pixels: `scroll` of `most`, `view` in sight, an
/// arrow moving `step`. Returns true when `scroll` changed.
pub fn scrollbar_px(r: Rect, scroll: &mut f32, most: f32, view: f32, step: f32) -> bool {
    if most <= 0.0 {
        return false;
    }
    let (mut first, shown, total) = bar_steps(*scroll, most, view, step);
    if !scrollbar(r, &mut first, shown, total) {
        return false;
    }
    *scroll = (first as f32 * step).min(most);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `capture_step` over (down edge, up edge, held, hover) frames for one button and
    /// returns each frame's `pressed`.
    fn clicks(frames: &[(bool, bool, bool, bool)]) -> Vec<bool> {
        let mine = [1, 2, 3, 4];
        let mut captured = None;
        frames
            .iter()
            .map(|&(down, up, held, hover)| {
                let (next, _, pressed) = capture_step(captured, mine, hover, down, up, held);
                captured = next;
                pressed
            })
            .collect()
    }

    /// A press on the button, its release, then a second release with no press in between (a
    /// worn switch's bounce) clicks once: the press's capture is used up by the first click.
    #[test]
    fn a_stray_second_release_without_a_press_does_not_click_again() {
        let frames = [(true, false, true, true), (false, true, false, true), (false, true, false, true)];
        assert_eq!(clicks(&frames), [false, true, false]);
        // Several bounces in a row, and a bounce on a frame with no press at all: still none.
        let bounces = [(true, false, true, true), (false, true, false, true), (false, true, false, true), (false, true, false, true), (false, false, false, true)];
        assert_eq!(clicks(&bounces).iter().filter(|&&p| p).count(), 1);
    }

    /// A release seen by another button (drawn earlier in the frame) does not take this button's
    /// press: its own release still clicks it.
    #[test]
    fn a_release_on_another_button_leaves_the_capture_alone() {
        let (next, _, pressed) = capture_step(Some([1, 2, 3, 4]), [9, 9, 9, 9], false, false, true, false);
        assert_eq!((next, pressed), (Some([1, 2, 3, 4]), false));
        let (next, _, pressed) = capture_step(next, [1, 2, 3, 4], true, false, true, false);
        assert_eq!((next, pressed), (None, true));
    }

    /// A normal press and release clicks once, and a press and release in one frame (a tap on a
    /// phone) clicks once too.
    #[test]
    fn a_press_and_release_click_once() {
        assert_eq!(clicks(&[(true, false, true, true), (false, true, false, true)]), [false, true]);
        assert_eq!(clicks(&[(true, true, false, true)]), [true]);
        // The next press clicks again.
        let twice = [(true, false, true, true), (false, true, false, true), (true, false, true, true), (false, true, false, true)];
        assert_eq!(clicks(&twice), [false, true, false, true]);
    }

    /// Holding the button clicks nothing until the release; sliding off and releasing there
    /// cancels; a press that began off the button never clicks it.
    #[test]
    fn holding_sliding_off_and_pressing_outside_do_not_click() {
        // Held, then released on the button.
        assert_eq!(clicks(&[(true, false, true, true), (false, false, true, true), (false, true, false, true)]), [false, false, true]);
        // Slide off and release off: no click, and a later release on the button has no press.
        assert_eq!(clicks(&[(true, false, true, true), (false, false, true, false), (false, true, false, false), (false, true, false, true)]), [false, false, false, false]);
        // Press off the button, slide on and release: no capture, no click.
        assert_eq!(clicks(&[(true, false, true, false), (false, false, true, true), (false, true, false, true)]), [false, false, false]);
    }

    /// The map tooltip's title corners (0x4dbd83, 0x4dbe0b): the hostile pair loses
    /// (128, 192, 250), leaving a dark orange, the normal one (192, 160, 160); the neutral box
    /// takes the green into all three, scaled 1, 0x2aa/1024, 0x1c7/1024 (0x48dd40, 0x48da5c).
    #[test]
    fn the_tooltip_corners_are_dimmed_as_the_original_s() {
        assert_eq!(tip_corner_pixel(TipStyle::Hostile, [255, 255, 255]), [127, 63, 5]);
        assert_eq!(tip_corner_pixel(TipStyle::Hostile, [100, 100, 100]), [0, 0, 0]);
        assert_eq!(tip_corner_pixel(TipStyle::Normal, [255, 255, 255]), [63, 95, 95]);
        assert_eq!(tip_corner_pixel(TipStyle::Neutral, [255, 255, 255]), [95, 63, 42]);
        assert_eq!(neutral_tip_pixel([10, 200, 30]), [200, 133, 88]);
    }

    /// The scenario description is added as plain lines (0x47ea80, a player's report): split
    /// only at CR LF and `#\`, its marks and leading spaces kept as typed, no indent added.
    #[test]
    fn a_plain_text_splits_only_at_its_breaks() {
        assert_eq!(plain_pieces("  Вуальшпиль - *подземелье*.\r\nНаконец#\\^конец"), vec!["  Вуальшпиль - *подземелье*.", "Наконец", "^конец"]);
        assert_eq!(plain_pieces(""), vec![""]);
    }

    /// The hint box fades in over 300 ms (49ee4b–49ef0d): to 160/255 for an ordinary hint, to
    /// full for a pushed one, and stays there.
    #[test]
    fn a_hint_box_fades_in_over_300_ms() {
        let ordinary = 160.0 / 255.0;
        assert_eq!(hint_alpha(0.0, ordinary), 0.0);
        assert!((hint_alpha(150.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((hint_alpha(300.0, ordinary) - ordinary).abs() < 1e-6);
        assert!((hint_alpha(5000.0, ordinary) - ordinary).abs() < 1e-6);
    }

    #[test]
    fn a_pixel_scrollbar_spans_the_whole_content() {
        assert_eq!(bar_steps(0.0, 100.0, 396.0, 15.0), (0, 26, 33), "at the top");
        assert_eq!(bar_steps(100.0, 100.0, 396.0, 15.0), (7, 26, 33), "the content's end is the bar's end");
        assert_eq!(bar_steps(44.0, 100.0, 396.0, 15.0), (3, 26, 33));
        assert_eq!(bar_steps(0.0, 0.0, 396.0, 15.0), (0, 26, 26), "all in sight: nothing to scroll");
    }

    #[test]
    fn a_key_tapped_within_a_frame_is_not_held() {
        // Enter pressed and let go in the same frame (a dialog closed with a quick tap): no key
        // stays held to cut every walk after it.
        assert_eq!(next_held(None, [KeyCode::Enter], true, |_| false), None);
        assert_eq!(next_held(Some(KeyCode::J), [KeyCode::Enter], true, |_| false), None);
        // Pressed and still down: held until a key comes up.
        assert_eq!(next_held(None, [KeyCode::Enter], false, |_| true), Some(KeyCode::Enter));
        assert_eq!(next_held(Some(KeyCode::Enter), [], false, |_| true), Some(KeyCode::Enter));
        assert_eq!(next_held(Some(KeyCode::Enter), [], true, |_| false), None);
        // Another key let go as this one goes down: this one is held.
        assert_eq!(next_held(Some(KeyCode::A), [KeyCode::B], true, |k| k == KeyCode::B), Some(KeyCode::B));
        // System keys are never held.
        assert_eq!(next_held(None, [KeyCode::LeftAlt], false, |_| true), None);
    }

    #[test]
    fn the_system_repeat_holds_the_newest_key_again() {
        use std::collections::HashSet;
        let none = HashSet::new();
        let dt = 1.0 / 60.0;
        // Up held, Right pressed at 1.0 s, Up let go at 1.1 s: nothing held (0x475841).
        let down = |k: KeyCode| k == KeyCode::Right;
        let rep = next_repeat(Some((KeyCode::Up, 0.0)), [KeyCode::Right], &none, 1.0, |k| k == KeyCode::Right || k == KeyCode::Up);
        assert_eq!(rep, Some((KeyCode::Right, 1.0)));
        let up: HashSet<_> = [KeyCode::Up].into();
        let rep = next_repeat(rep, [], &up, 1.1, down);
        let held = repeated_held(next_held(Some(KeyCode::Right), [], true, down), rep, 1.1, dt);
        assert_eq!(held, None, "the original stops the scroll too");
        // Right's first repeat (500 ms after it went down) holds it again (0x475820).
        assert_eq!(repeated_held(None, next_repeat(rep, [], &none, 1.3, down), 1.3, dt), None);
        assert_eq!(repeated_held(None, next_repeat(rep, [], &none, 1.5, down), 1.5, dt), Some(KeyCode::Right));
        // Right held, A tapped: the repeat is A's and ends with it; Right stays not held.
        let both = |k: KeyCode| k == KeyCode::Right || k == KeyCode::A;
        let rep = next_repeat(Some((KeyCode::Right, 0.0)), [KeyCode::A], &none, 2.0, both);
        let a: HashSet<_> = [KeyCode::A].into();
        let rep = next_repeat(rep, [], &a, 2.1, down);
        assert_eq!(rep, None);
        assert_eq!(repeated_held(None, next_repeat(rep, [], &none, 3.0, down), 3.0, dt), None);
        // System keys never repeat into the held key.
        assert_eq!(next_repeat(None, [KeyCode::LeftAlt], &none, 0.0, |_| true), None);
    }

    #[test]
    fn a_forgotten_key_is_not_held() {
        HELD_KEY.with(|h| h.set(Some(KeyCode::GraveAccent)));
        assert_eq!(held_key(), Some(KeyCode::GraveAccent));
        forget_held_key();
        assert_eq!(held_key(), None, "the console's ~ does not stop the walk once it closes");
    }

    #[test]
    fn esc_and_the_system_back_are_the_same_key() {
        assert!(is_escape(KeyCode::Escape));
        assert!(is_escape(KeyCode::Back));
        for k in [KeyCode::Enter, KeyCode::Space, KeyCode::N, KeyCode::Tab] {
            assert!(!is_escape(k), "{k:?}");
        }
        assert!(!crate::ui::keymap::bindable(KeyCode::Back), "Back is never bound");
        assert_eq!(answer_of([KeyCode::Back].into_iter()), Some(false), "Back answers No, as Esc");
        assert_eq!(next_held(None, [KeyCode::Back], false, |_| true), None, "Back is no held key");
        assert_eq!(next_repeat(None, [KeyCode::Back], &Default::default(), 0.0, |_| true), None);
    }

    #[test]
    fn the_question_box_takes_any_key_but_four_as_yes() {
        use KeyCode::*;
        assert_eq!(answer_of([Escape].into_iter()), Some(false));
        assert_eq!(answer_of([N].into_iter()), Some(false), "Razdor's N is No");
        for k in [Enter, Space, Y, A, F1, Left, LeftShift] {
            assert_eq!(answer_of([k].into_iter()), Some(true), "{k:?}");
        }
        for k in [Tab, LeftAlt, RightAlt, Up, Down] {
            assert_eq!(answer_of([k].into_iter()), None, "{k:?}");
        }
        assert_eq!(answer_of(std::iter::empty()), None);
    }

    #[test]
    fn a_held_key_repeats_after_half_a_second_at_thirty_a_second() {
        assert_eq!(repeats(0.3, 1.0 / 60.0), 0, "not yet");
        assert_eq!(repeats(0.5, 1.0 / 60.0), 1, "the first repeat at 500 ms");
        let total: usize = (1..=60).map(|i| repeats(0.5 + i as f64 / 60.0, 1.0 / 60.0)).sum();
        assert_eq!(total, 30, "then 30 a second");
        assert_eq!(repeats(2.0, 0.2), 6, "a slow frame catches up");
        // No field reads Backspace's press alone (which deletes one character however long
        // it is held): the hero's name and the save's name repeat as the original's edit box.
        for (file, src) in [("screens.rs", include_str!("screens.rs")), ("saves.rs", include_str!("saves.rs")), ("new_game.rs", include_str!("new_game.rs")), ("console.rs", include_str!("console.rs")), ("item_filter.rs", include_str!("item_filter.rs"))] {
            assert!(!src.contains("is_key_pressed(KeyCode::Backspace)"), "{file} reads Backspace without repeat");
        }
    }

    #[test]
    fn numbers_parse_and_clamp() {
        assert_eq!(parse_number("42", 0, 100), Some(42));
        assert_eq!(parse_number("420", 0, 100), Some(100));
        assert_eq!(parse_number("-5", -3, 3), Some(-3));
        assert_eq!(parse_number("", 0, 9), None);
        assert_eq!(parse_number("-", -9, 9), None);
        assert_ne!(field_id("b1:name"), field_id("b2:name"));
    }

    #[test]
    fn transliterates_russian() {
        assert_eq!(transliterate("Привет, Мир"), "Privet, Mir");
        assert_eq!(transliterate("ok 12"), "ok 12");
    }

    #[test]
    fn the_strip_shows_the_line_of_the_caret() {
        assert_eq!(caret_line("ab\ncd", 4), ("cd".to_string(), 1));
        assert_eq!(caret_line("ab\ncd", 2), ("ab".to_string(), 2), "a caret at a break ends its line");
        assert_eq!(caret_line("", 0), (String::new(), 0));
        assert_eq!(caret_line("Имя", 9), ("Имя".to_string(), 3), "a caret past the end is the end");
    }

    #[test]
    fn an_active_field_wants_the_keyboard_with_its_text() {
        KEYBOARD_WANTED.with(|k| k.set(false));
        want_keyboard("Вася", 2);
        assert!(KEYBOARD_WANTED.with(|k| k.get()));
        assert_eq!(KEYBOARD_TEXT.with(|t| t.borrow().clone()), ("Вася".to_string(), 2));
    }

    /// The cache measures a text again only when its text, width, size or font state changes;
    /// otherwise the kept lines come back. `measured` counts the measurements.
    #[test]
    fn the_wrap_cache_measures_again_only_on_a_change() {
        use super::super::dt_font::{Face, FontMode};
        let state = MeasureState { face: Face::Body, mode: FontMode::Original, native: false, k: 0 };
        let mut cache = Vec::new();
        let measured = std::cell::Cell::new(0);
        let get = |cache: &mut Vec<WrapEntry>, state: MeasureState, s: &str, w: f32, z: f32| {
            cached_wrap(cache, state, s, w, z, || {
                measured.set(measured.get() + 1);
                vec![format!("{s}|{w}|{z}")]
            })
        };
        assert_eq!(get(&mut cache, state, "a", 300.0, 14.0), ["a|300|14"]);
        assert_eq!(get(&mut cache, state, "a", 300.0, 14.0), ["a|300|14"]);
        assert_eq!(measured.get(), 1, "the same text, width and size come back kept");
        assert_eq!(get(&mut cache, state, "a", 120.0, 14.0), ["a|120|14"], "a new width");
        assert_eq!(get(&mut cache, state, "a", 120.0, 20.0), ["a|120|20"], "a new size");
        assert_eq!(get(&mut cache, state, "b", 120.0, 20.0), ["b|120|20"], "a new text");
        assert_eq!(measured.get(), 4);
        let modern = MeasureState { mode: FontMode::Modern, ..state };
        assert_eq!(get(&mut cache, modern, "b", 120.0, 20.0), ["b|120|20"], "a new font mode");
        let native = MeasureState { native: true, k: 7, ..state };
        get(&mut cache, native, "b", 120.0, 20.0);
        get(&mut cache, MeasureState { k: 8, ..native }, "b", 120.0, 20.0);
        assert_eq!(measured.get(), 7, "a new face-scale or native state is measured too");
        assert_eq!(get(&mut cache, state, "b", 120.0, 20.0), ["b|120|20"]);
        assert_eq!(measured.get(), 7, "the original state's entry is still kept");
    }

    /// Past [`WRAP_CACHE_MAX`] results the oldest goes, and the cache still answers.
    #[test]
    fn the_wrap_cache_drops_its_oldest_result_when_full() {
        use super::super::dt_font::{Face, FontMode};
        let state = MeasureState { face: Face::Body, mode: FontMode::Original, native: false, k: 0 };
        let mut cache = Vec::new();
        for i in 0..=WRAP_CACHE_MAX {
            cached_wrap(&mut cache, state, &format!("t{i}"), 100.0, 14.0, || vec![format!("t{i}")]);
        }
        assert_eq!(cache.len(), WRAP_CACHE_MAX);
        assert_eq!(cache[0].text, "t1", "t0 was dropped");
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    /// Desktop: the name is typed straight in, whatever the touch.
    #[test]
    fn the_desktop_name_field_always_has_the_keyboard_request() {
        assert!(name_keyboard(false, false, NameTouch::Nothing));
        assert!(name_keyboard(false, false, NameTouch::RowPicked));
    }

    /// A phone: opening (no tap, not typing) shows nothing; a tap in the field shows it; a
    /// pick of another save hides it; with nothing happening it stays as it was.
    #[test]
    fn a_phone_name_field_shows_the_keyboard_only_after_a_tap_in_it() {
        assert!(!name_keyboard(true, false, NameTouch::Nothing), "opening the window: no keyboard");
        assert!(name_keyboard(true, false, NameTouch::FieldTapped), "a tap in the field: keyboard");
        assert!(!name_keyboard(true, false, NameTouch::RowPicked), "a save picked: no keyboard");
        assert!(!name_keyboard(true, true, NameTouch::RowPicked), "a pick hides a keyboard that was up");
        assert!(name_keyboard(true, true, NameTouch::Nothing), "typing goes on without a new tap");
    }

    /// A phone with the name field's keyboard up: Enter hides it and is not OK; with the keyboard
    /// down (or on the desktop) Enter is OK, as before.
    #[test]
    fn enter_on_a_phone_with_the_keyboard_up_hides_it_and_is_not_ok() {
        assert_eq!(name_enter_decision(true, true, true), NameEnter::HideKeyboard);
        assert_eq!(name_enter_decision(true, false, true), NameEnter::Confirm, "the keyboard hidden: a hardware Enter is OK");
        assert_eq!(name_enter_decision(false, true, true), NameEnter::Confirm, "the desktop: Enter is OK");
        assert_eq!(name_enter_decision(true, true, false), NameEnter::Nothing, "no Enter");
        let mut typing = true;
        assert!(!name_enter(true, &mut typing, true), "the keyboard's Enter confirms nothing");
        assert!(!typing, "and the keyboard goes");
        assert!(name_enter(true, &mut typing, true), "the next Enter is OK");
        let mut desk = true;
        assert!(name_enter(false, &mut desk, true));
        assert!(desk, "the desktop's name field keeps its flag");
    }

    /// The system hiding the keyboard (its arrow, Back) ends the typing; a tap after it starts again.
    #[test]
    fn a_keyboard_the_system_hides_ends_the_typing_until_the_next_tap() {
        assert!(!name_keyboard(true, true, NameTouch::Hidden), "hidden by the system: not typing");
        assert!(name_keyboard(true, false, NameTouch::FieldTapped), "a tap after it: keyboard again");
        assert!(!name_keyboard(true, false, NameTouch::Hidden));
    }

    /// The tap target on a phone is 48 dp tall, grown from the field's top or bottom edge; the
    /// desktop keeps the field's own rect.
    #[test]
    fn the_phone_tap_target_is_48_dp_tall_and_the_desktop_keeps_the_field() {
        let field = Rect::new(10.0, 100.0, 300.0, 30.0);
        assert_eq!(tap_rect_at(field, true, None), field, "desktop: unchanged");
        let down = tap_rect_at(field, false, Some(2.75));
        let up = tap_rect_at(field, true, Some(2.75));
        assert_eq!(down, Rect::new(10.0, 100.0, 300.0, 48.0 * 2.75));
        assert_eq!(up.h, 48.0 * 2.75);
        assert_eq!(up.y + up.h, field.y + field.h, "grown up: the bottom edge stays");
    }
}

#[cfg(test)]
mod outside_tap_tests {
    use super::*;

    #[test]
    fn a_tap_outside_a_bar_window_closes_it() {
        let bar = Rect::new(100.0, 700.0, 760.0, 80.0);
        let window = Rect::new(200.0, 150.0, 400.0, 300.0);
        let popup = Rect::new(620.0, 120.0, 150.0, 90.0);
        let windows = [window, popup];
        let slop = 6.0;
        let at = |x: f32, y: f32| Vec2::new(x, y);

        // A tap outside the window and off the bar closes it.
        assert!(outside_tap(at(50.0, 50.0), at(51.0, 52.0), &windows, bar, slop), "outside both");
        // A tap inside the window does not.
        assert!(!outside_tap(at(300.0, 200.0), at(300.0, 200.0), &windows, bar, slop), "inside the window");
        // A tap on the bar does not.
        assert!(!outside_tap(at(400.0, 740.0), at(400.0, 740.0), &windows, bar, slop), "on the bar");
        // A press outside but a release inside (a drag in) does not.
        assert!(!outside_tap(at(195.0, 200.0), at(200.5, 200.0), &windows, bar, slop), "released inside");
        // A drag (moved more than the slop) does not, even outside.
        assert!(!outside_tap(at(50.0, 50.0), at(90.0, 50.0), &windows, bar, slop), "a drag");
        // A tap inside a second recorded rect (a popup over the window) does not.
        assert!(!outside_tap(at(640.0, 130.0), at(640.0, 130.0), &windows, bar, slop), "inside the popup");
    }
}

#[cfg(test)]
mod finger_tests {
    use super::*;

    /// The phone of the checks: 2.75 px per dp, a 1080 × 2400 screen, the bar's top at 2300.
    const DP: Option<f32> = Some(2.75);
    const BAR: f32 = 2300.0;
    const EPS: f32 = 1e-3;

    fn screen() -> Rect {
        Rect::new(0.0, 0.0, 1080.0, 2400.0)
    }

    fn hit(r: Rect, others: &[Rect]) -> Rect {
        finger_hit_at(r, DP, screen(), BAR, others)
    }

    fn same(a: Rect, b: Rect) -> bool {
        (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS && (a.w - b.w).abs() < EPS && (a.h - b.h).abs() < EPS
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    fn contains(outer: Rect, inner: Rect) -> bool {
        outer.x <= inner.x + EPS && outer.y <= inner.y + EPS && outer.right() + EPS >= inner.right() && outer.bottom() + EPS >= inner.bottom()
    }

    #[test]
    fn desktop_gets_the_drawn_rect() {
        let r = Rect::new(500.0, 1000.0, 50.0, 20.0);
        assert!(same(finger_hit_at(r, None, screen(), BAR, &[]), r));
    }

    #[test]
    fn a_lone_small_target_grows_to_48_dp_around_its_centre() {
        let r = Rect::new(500.0, 1000.0, 50.0, 20.0);
        let g = hit(r, &[]);
        assert!((g.w - 132.0).abs() < EPS && (g.h - 132.0).abs() < EPS, "{g:?}");
        assert!((g.x + g.w / 2.0 - (r.x + r.w / 2.0)).abs() < EPS, "{g:?}");
        assert!((g.y + g.h / 2.0 - (r.y + r.h / 2.0)).abs() < EPS, "{g:?}");
    }

    #[test]
    fn a_target_at_the_screens_top_grows_down_from_zero() {
        let g = hit(Rect::new(500.0, 0.0, 30.0, 30.0), &[]);
        assert!(g.y.abs() < EPS, "{g:?}");
        assert!((g.h - 132.0).abs() < EPS, "{g:?}");
    }

    #[test]
    fn neighbours_share_the_midline_and_each_keeps_its_free_side() {
        let a = Rect::new(300.0, 1000.0, 60.0, 30.0);
        let b = Rect::new(370.0, 1000.0, 60.0, 30.0);
        let ga = hit(a, &[b]);
        let gb = hit(b, &[a]);
        assert!(ga.right() <= gb.x + EPS, "{ga:?} {gb:?}");
        assert!(!overlaps(ga, gb), "{ga:?} {gb:?}");
        assert!((ga.w - 132.0).abs() < EPS, "{ga:?}");
        assert!((gb.w - 132.0).abs() < EPS, "{gb:?}");
        assert!((ga.right() - 365.0).abs() < EPS, "a stops at the midline: {ga:?}");
        assert!((gb.x - 365.0).abs() < EPS, "b starts at the midline: {gb:?}");
    }

    #[test]
    fn a_target_above_the_bar_never_crosses_it() {
        let g = hit(Rect::new(500.0, 2200.0, 50.0, 50.0), &[]);
        assert!(g.bottom() <= BAR + EPS, "{g:?}");
        assert!((g.h - 132.0).abs() < EPS, "{g:?}");
    }

    #[test]
    fn a_target_on_the_bar_never_goes_above_it() {
        let g = hit(Rect::new(500.0, 2310.0, 50.0, 20.0), &[]);
        assert!(g.y >= BAR - EPS, "{g:?}");
        assert!(g.h >= 20.0 - EPS, "{g:?}");
    }

    #[test]
    fn a_container_or_an_overlapping_rect_is_no_obstacle() {
        let r = Rect::new(500.0, 1000.0, 50.0, 20.0);
        let plain = hit(r, &[]);
        let container = Rect::new(400.0, 950.0, 300.0, 200.0);
        let partly = Rect::new(530.0, 1010.0, 100.0, 100.0);
        assert!(same(hit(r, &[container]), plain));
        assert!(same(hit(r, &[partly]), plain));
        assert!(same(hit(r, &[r]), plain));
    }

    #[test]
    fn a_target_already_big_enough_is_unchanged() {
        let r = Rect::new(400.0, 800.0, 200.0, 200.0);
        assert!(same(hit(r, &[]), r));
    }

    #[test]
    fn a_diagonal_neighbour_is_not_entered() {
        let r = Rect::new(300.0, 1000.0, 50.0, 20.0);
        let o = Rect::new(360.0, 1040.0, 50.0, 20.0);
        let g = hit(r, &[o]);
        assert!(!overlaps(g, o), "{g:?}");
        assert!(contains(g, r), "{g:?}");
    }

    #[test]
    fn the_room_is_the_smallest_window_that_holds_the_target_else_the_screen() {
        let big = Rect::new(100.0, 100.0, 800.0, 800.0);
        let small = Rect::new(300.0, 300.0, 200.0, 200.0);
        let windows = [big, small];
        let screen = screen();
        assert!(same(finger_room(Rect::new(350.0, 350.0, 20.0, 20.0), &windows, screen), small));
        assert!(same(finger_room(Rect::new(120.0, 120.0, 20.0, 20.0), &windows, screen), big));
        // Sticking out of the small window but inside the big one: the big one.
        assert!(same(finger_room(Rect::new(480.0, 350.0, 40.0, 20.0), &windows, screen), big));
        // Held by no window: the screen.
        assert!(same(finger_room(Rect::new(10.0, 10.0, 20.0, 20.0), &windows, screen), screen));
        assert!(same(finger_room(Rect::new(10.0, 10.0, 20.0, 20.0), &[], screen), screen));
    }

    #[test]
    fn the_result_is_never_smaller_than_the_target_nor_off_the_screen() {
        let others = [Rect::new(340.0, 1100.0, 40.0, 40.0), Rect::new(150.0, 900.0, 30.0, 300.0)];
        // Targets wholly on the screen: a target that sticks out keeps its own edge (the rule).
        for x in (0..=1050).step_by(97) {
            for y in (0..=2380).step_by(151) {
                let r = Rect::new(x as f32, y as f32, 30.0, 20.0);
                let g = hit(r, &others);
                assert!(contains(g, r), "{g:?} must contain {r:?}");
                assert!(g.x >= -EPS && g.y >= -EPS && g.right() <= 1080.0 + EPS && g.bottom() <= 2400.0 + EPS, "{g:?}");
            }
        }
    }
}
