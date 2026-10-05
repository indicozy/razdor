use std::cell::RefCell;

use macroquad::prelude::*;

pub const INK: Color = Color::new(0.93, 0.90, 0.82, 1.0);
pub const DIM: Color = Color::new(0.65, 0.62, 0.55, 1.0);
pub const ACCENT: Color = Color::new(0.95, 0.78, 0.30, 1.0);
pub const PANEL: Color = Color::new(0.12, 0.11, 0.10, 0.92);

thread_local! {
    /// A TrueType font with Cyrillic, for text the built-in pixel font cannot draw (the
    /// original's names and descriptions).
    static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
    /// Sharp stand-ins for the original's bitmap faces: text, bold, titles.
    static FACE_FONTS: RefCell<[Option<Font>; 3]> = const { RefCell::new([None, None, None]) };
}

/// The fonts that ship with Razdor (`data/fonts`, SIL Open Font License; see
/// `docs/superpowers/specs/2026-09-28-interface-fonts.md`): PT Sans for text and bold, Kurale
/// for titles and names, in place of the original's small bitmap faces. `RAZDOR_FONT`,
/// `RAZDOR_FONT_BOLD` and `RAZDOR_FONT_TITLE` name other files (e.g. a bought Benguiat).
const FACE_FILES: [(&str, &[u8]); 3] = [
    ("RAZDOR_FONT", include_bytes!("../../data/fonts/PT_Sans-Regular.ttf")),
    ("RAZDOR_FONT_BOLD", include_bytes!("../../data/fonts/PT_Sans-Bold.ttf")),
    ("RAZDOR_FONT_TITLE", include_bytes!("../../data/fonts/Kurale-Regular.ttf")),
];

pub async fn load_font() {
    for (i, (var, bundled)) in FACE_FILES.iter().enumerate() {
        let own = match std::env::var(var) {
            Ok(path) => load_ttf_font(&path).await.map_err(|e| razdor::diag!("{var}={path}: {e}")).ok(),
            Err(_) => None,
        };
        let font = own.or_else(|| load_ttf_font_from_bytes(bundled).map_err(|e| razdor::diag!("bundled font {i}: {e}")).ok());
        FACE_FONTS.with(|f| f.borrow_mut()[i] = font);
    }
    // The text face doubles as the font for everything else.
    let text = FACE_FONTS.with(|f| f.borrow()[0].clone());
    if text.is_none() {
        razdor::diag!("no TrueType font could be loaded; transliterating");
    }
    FONT.with(|f| *f.borrow_mut() = text);
}

/// The sharp stand-in for bitmap face `i` (0 text, 1 bold, 2 titles), if installed.
pub fn face_font(i: usize) -> Option<Font> {
    FACE_FONTS.with(|f| f.borrow().get(i).cloned().flatten())
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

/// Runs `f` with the font to use for `s` (`None` = the built-in one) and the text to draw.
/// The TrueType font draws everything when there is one, as the original uses one smooth
/// font for all its texts.
fn with_font<R>(s: &str, f: impl FnOnce(Option<&Font>, &str) -> R) -> R {
    if let Some(font) = super::dt_font::current_ttf() {
        return f(Some(&font), s);
    }
    FONT.with(|font| match font.borrow().as_ref() {
        Some(font) => f(Some(font), s),
        None if s.is_ascii() => f(None, s),
        None => f(None, &transliterate(s)),
    })
}

fn measure_ttf(s: &str, size: f32) -> TextDimensions {
    with_font(s, |font, s| measure_text(s, font, size as u16, 1.0))
}

fn text_ttf(s: &str, x: f32, y: f32, size: f32, color: Color) {
    with_font(s, |font, s| {
        draw_text_ex(s, x, y, TextParams { font, font_size: size as u16, color, ..Default::default() });
    });
}

/// Size of `s`: in the original's font of the current face when the install has it
/// (`ui::dt_font`), else in the TrueType font.
pub fn measure(s: &str, size: f32) -> TextDimensions {
    let bitmap = super::dt_font::with_current(|f| {
        let width = f.width(s, size, &|c| measure_ttf(c.encode_utf8(&mut [0; 4]), size).width);
        let cap = f.cap_height(size);
        TextDimensions { width, height: size, offset_y: cap }
    });
    bitmap.unwrap_or_else(|| measure_ttf(s, size))
}

thread_local! {
    /// A modal dialog is open: the screen below draws but takes no input.
    static BLOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// Debug snapshots: where the pointer is said to be (`RAZDOR_MOUSE=x,y`, `ui::snapshot`).
    static POINTER: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
}

/// The pointer's position: the mouse's, or the snapshot's stand-in.
pub fn pointer() -> (f32, f32) {
    POINTER.with(|p| p.get()).unwrap_or_else(mouse_position)
}

/// Puts the pointer at `at` for good (debug snapshots only).
pub fn set_pointer(at: Option<(f32, f32)>) {
    POINTER.with(|p| p.set(at));
}

thread_local! {
    /// This frame's click and key presses were taken (a wait cut by them): nothing else
    /// acts on them.
    static SWALLOWED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

pub fn mouse_in(x: f32, y: f32, w: f32, h: f32) -> bool {
    let (mx, my) = pointer();
    !input_blocked() && mx >= x && mx < x + w && my >= y && my < y + h
}

pub fn clicked() -> bool {
    !input_blocked() && !input_swallowed() && is_mouse_button_pressed(MouseButton::Left)
}

pub fn right_clicked() -> bool {
    !input_blocked() && !input_swallowed() && is_mouse_button_pressed(MouseButton::Right)
}

pub fn key(k: KeyCode) -> bool {
    !input_blocked() && !input_swallowed() && is_key_pressed(k)
}

thread_local! {
    /// The original's one held key (interface.md §2, 0x475748).
    static HELD_KEY: std::cell::Cell<Option<KeyCode>> = const { std::cell::Cell::new(None) };
}

/// Once per frame, before the screens: the original keeps no key table, only the last key
/// that went down, and any key going up clears it (so with two keys down, letting one go
/// leaves none held). Alt and F10 are system keys and never become the held key.
pub fn track_held_key() {
    SWALLOWED.with(|s| s.set(false));
    let up = !get_keys_released().is_empty();
    let down = get_keys_pressed().into_iter().find(|k| !matches!(k, KeyCode::LeftAlt | KeyCode::RightAlt | KeyCode::F10));
    HELD_KEY.with(|h| {
        if up {
            h.set(None);
        }
        if down.is_some() {
            h.set(down);
        }
    });
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
/// any other key Yes (Enter, Space, N or a letter alike), except Tab, Alt and the Up and
/// Down arrows, which do nothing. `None` while no such key went down this frame.
pub fn answer_key() -> Option<bool> {
    if input_blocked() || input_swallowed() {
        return None;
    }
    answer_of(get_keys_pressed().into_iter())
}

fn answer_of(keys: impl Iterator<Item = KeyCode>) -> Option<bool> {
    let ignored = |k: &KeyCode| matches!(k, KeyCode::Tab | KeyCode::LeftAlt | KeyCode::RightAlt | KeyCode::F10 | KeyCode::Up | KeyCode::Down);
    let keys: Vec<KeyCode> = keys.filter(|k| !ignored(k)).collect();
    if keys.contains(&KeyCode::Escape) {
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

/// Drawing clipped to a rectangle (screen coordinates) while the guard lives.
pub struct Clip;

impl Clip {
    pub fn new(r: Rect) -> Clip {
        let dpi = screen_dpi_scale();
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.scissor(Some(((r.x * dpi) as i32, (r.y * dpi) as i32, (r.w * dpi).ceil() as i32, (r.h * dpi).ceil() as i32)));
        Clip
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        unsafe { get_internal_gl() }.quad_gl.scissor(None);
    }
}

/// A translucent panel of lines next to the mouse.
pub fn tooltip(lines: &[(String, Color)]) {
    if lines.is_empty() {
        return;
    }
    let w = lines.iter().map(|(s, _)| measure(s, 17.0).width).fold(0.0, f32::max) + 24.0;
    let h = lines.len() as f32 * 21.0 + 14.0;
    let (mx, my) = pointer();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - h - 4.0);
    tooltip_panel(Rect::new(x, y, w, h));
    for (i, (s, c)) in lines.iter().enumerate() {
        super::chrome::shadow_text(s, x + 12.0, y + 24.0 + i as f32 * 21.0, 17.0, *c);
    }
}

/// The translucent green-marble panel of hover tooltips, with a silver edge.
pub fn tooltip_panel(r: Rect) {
    // Translucent marble: the ground shows through, as in the original's map tooltips.
    super::chrome::surface_alpha(r, super::chrome::Skin::Marble, 0.72);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.05, 0.03, 0.2));
    super::chrome::silver_frame(r, 1.5);
}

/// Draws a button and returns true when it was clicked this frame.
pub fn button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    button_sounding(x, y, w, h, label, enabled, super::audio::Cue::Button)
}

/// [`button`] with its own press sound (the money buttons play `Item-Gold`).
pub fn button_sounding(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool, sound: super::audio::Cue) -> bool {
    let hover = enabled && mouse_in(x, y, w, h);
    super::chrome::marble_button(Rect::new(x, y, w, h), label, enabled, hover);
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(sound);
    }
    pressed
}

/// Draws `s` with its baseline at `y` (see [`measure`] for the font).
pub fn text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let drawn = super::dt_font::with_current(|f| {
        f.draw(s, x, y, size, color, &|c, px, py| {
            let ch = c.encode_utf8(&mut [0; 4]).to_string();
            text_ttf(&ch, px, py, size, color);
            measure_ttf(&ch, size).width
        })
    });
    if drawn.is_none() {
        text_ttf(s, x, y, size, color);
    }
}

/// The font size, at most `size` and not below 70% of it, at which `s` fits in `width`
/// (Russian texts run longer than the English ones the layouts were drawn for).
pub fn fit_size(s: &str, width: f32, size: f32) -> f32 {
    let mut fit = size;
    while fit > (size * 0.7).max(9.0) && measure(s, fit).width > width {
        fit -= 1.0;
    }
    fit
}

/// `s` shortened with "…" until it fits in `width` at `size`.
pub fn ellipsize(s: &str, width: f32, size: f32) -> String {
    if measure(s, size).width <= width {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while t.chars().count() > 1 && measure(&format!("{t}…"), size).width > width {
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
fn glyph_width(c: char, size: f32) -> f32 {
    let c = if c == '_' { ' ' } else { c };
    measure(c.encode_utf8(&mut [0; 4]), size).width
}

/// The rows of `text` read as the original's markup (see [`razdor::dt::markup`]) in a box
/// `room` wide at `size`: each line word-wrapped as TextList.AddText 0x47e46c does.
pub fn markup_rows(text: &str, room: f32, size: f32) -> Vec<MarkupRow> {
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
    use razdor::dt::markup::Align;
    let color = ink_color(row.ink);
    let shown = row.text.replace('_', " ");
    match row.align {
        Align::Centre => {
            let w: f32 = row.text.chars().map(|c| glyph_width(c, size)).sum();
            text(&shown, x + ((room - w) / 2.0).trunc(), y, size, color);
        }
        Align::Justify if row.last || !row.text.contains(' ') => text(&shown, x, y, size, color),
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
                text(&word, x + xs[i], y, size, color);
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

/// The field that has the keyboard: its id and, for number fields, the digits typed so far.
struct Focus {
    id: u64,
    buf: String,
}

/// An open drop-down list.
struct Popup {
    id: u64,
    rect: Rect,
    options: Vec<(i64, String)>,
    scroll: usize,
    filter: String,
}

thread_local! {
    static FOCUS: RefCell<Option<Focus>> = const { RefCell::new(None) };
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
    /// A choice made in the popup: (drop-down id, value), taken by that drop-down.
    static PICKED: std::cell::Cell<Option<(u64, i64)>> = const { std::cell::Cell::new(None) };
    /// Some field took this frame's click.
    static CLAIMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// When Backspace went down (for key repeat).
    static HELD: std::cell::Cell<Option<f64>> = const { std::cell::Cell::new(None) };
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

fn claim() {
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

fn focused(id: u64) -> bool {
    FOCUS.with(|f| f.borrow().as_ref().is_some_and(|f| f.id == id))
}

fn set_focus(id: u64, buf: String) {
    FOCUS.with(|f| *f.borrow_mut() = Some(Focus { id, buf }));
}

/// Applies typing to `s`: `chars` are added (control characters skipped) up to
/// `max_chars`, then `backspaces` characters are removed, and `newline` adds a line break
/// in a multi-line field. True if `s` changed.
pub fn apply_typing(s: &mut String, chars: &[char], backspaces: usize, newline: bool, multiline: bool, max_chars: usize) -> bool {
    let before = s.clone();
    for &c in chars {
        if !c.is_control() && s.chars().count() < max_chars {
            s.push(c);
        }
    }
    for _ in 0..backspaces {
        s.pop();
    }
    if newline && multiline && s.chars().count() < max_chars {
        s.push('\n');
    }
    *s != before
}

/// Parses a typed number, clamped to `min..=max`. Empty or `-` alone gives `None`.
pub fn parse_number(text: &str, min: i64, max: i64) -> Option<i64> {
    text.trim().parse::<i64>().ok().map(|v| v.clamp(min, max))
}

/// Backspace pressed this frame, or held (repeats after 0.4 s, 25 per second).
fn backspace_repeat() -> usize {
    if is_key_pressed(KeyCode::Backspace) {
        HELD.with(|h| h.set(Some(get_time())));
        return 1;
    }
    if !is_key_down(KeyCode::Backspace) {
        HELD.with(|h| h.set(None));
        return 0;
    }
    let Some(t0) = HELD.with(|h| h.get()) else { return 0 };
    let t = get_time() - t0 - 0.4;
    if t < 0.0 {
        return 0;
    }
    let prev = ((t - get_frame_time() as f64) * 25.0).floor().max(-1.0);
    ((t * 25.0).floor() - prev).max(0.0) as usize
}

/// This frame's typing for the focused field.
fn typed() -> (Vec<char>, usize, bool) {
    let mut chars = Vec::new();
    while let Some(c) = get_char_pressed() {
        chars.push(c);
    }
    let enter = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter);
    (chars, backspace_repeat(), enter)
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
    let mut changed = false;
    if active && !input_blocked() {
        let (chars, back, enter) = typed();
        changed = apply_typing(value, &chars, back, enter, multiline, 2000);
        if (enter && !multiline) || is_key_pressed(KeyCode::Escape) {
            clear_focus();
        }
    }
    draw_rectangle(x, y, w, h, FIELD_BG);
    draw_rectangle_lines(x, y, w, h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else if hover { INK } else { DIM });
    let size = 17.0;
    let caret = if active && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    if multiline {
        let lines = wrap(&format!("{value}{caret}"), w - 10.0, size);
        let fit = ((h - 6.0) / 19.0).floor().max(1.0) as usize;
        let skip = if active { lines.len().saturating_sub(fit) } else { 0 };
        for (i, line) in lines.iter().skip(skip).take(fit).enumerate() {
            text(line, x + 5.0, y + 17.0 + i as f32 * 19.0, size, INK);
        }
    } else {
        // Show the end of a long line.
        let mut shown: String = format!("{value}{caret}");
        while measure(&shown, size).width > w - 10.0 && !shown.is_empty() {
            shown.remove(0);
        }
        text(&shown, x + 5.0, y + (h + 12.0) / 2.0, size, INK);
    }
    changed
}

/// A whole number between `min` and `max`: − and + step it (Shift: by 10), the wheel over
/// it too, and a click on the number lets you type one (Enter or a click elsewhere takes
/// it). Returns the new value when it changed.
pub fn number_field(key: &str, x: f32, y: f32, w: f32, value: i64, min: i64, max: i64) -> Option<i64> {
    let id = field_id(key);
    let h = 24.0;
    let bw = 22.0;
    let step = if is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift) { 10 } else { 1 };
    let mut out = None;
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
                set_focus(id, String::new());
            }
        }
    }
    let active = focused(id);
    let mut shown = value.to_string();
    if active && !input_blocked() {
        let (chars, back, enter) = typed();
        let mut buf = FOCUS.with(|f| f.borrow().as_ref().map(|f| f.buf.clone()).unwrap_or_default());
        let digits: Vec<char> = chars.into_iter().filter(|c| c.is_ascii_digit() || (*c == '-' && min < 0)).collect();
        apply_typing(&mut buf, &digits, back, false, false, 7);
        let away = clicked() && !hover;
        if enter || away {
            if let Some(v) = parse_number(&buf, min, max) {
                out = Some(v);
            }
            clear_focus();
        } else if is_key_pressed(KeyCode::Escape) {
            clear_focus();
        } else {
            FOCUS.with(|f| {
                if let Some(f) = f.borrow_mut().as_mut() {
                    f.buf = buf.clone();
                }
            });
            let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
            shown = format!("{buf}{caret}");
        }
    }
    draw_rectangle(tx, y, tw, h, FIELD_BG);
    draw_rectangle_lines(tx, y, tw, h, if active { 2.0 } else { 1.0 }, if active { ACCENT } else if hover { INK } else { DIM });
    let d = measure(&shown, 17.0);
    text(&shown, tx + (tw - d.width) / 2.0, y + 17.0, 17.0, INK);
    out.filter(|v| *v != value)
}

/// A compact button (no sound, for steppers and list rows).
pub fn small_button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    let hover = enabled && mouse_in(x, y, w, h);
    let bg = match (enabled, hover) {
        (false, _) => Color::new(0.16, 0.16, 0.16, 1.0),
        (true, true) => Color::new(0.45, 0.35, 0.2, 1.0),
        (true, false) => Color::new(0.26, 0.21, 0.14, 1.0),
    };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 1.0, if enabled { DIM } else { Color::new(0.3, 0.3, 0.3, 1.0) });
    let size = fit_size(label, w - 6.0, 17.0);
    let label = &ellipsize(label, w - 6.0, size);
    let d = measure(label, size);
    text(label, x + (w - d.width) / 2.0, y + (h + d.offset_y) / 2.0 - 1.0, size, if enabled { INK } else { DIM });
    hover && clicked()
}

/// A button that stays lit while `on` (tool and mode pickers). True when clicked.
pub fn toggle_button(x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let hover = mouse_in(x, y, w, h);
    let bg = if on { Color::new(0.55, 0.42, 0.18, 1.0) } else if hover { Color::new(0.36, 0.28, 0.18, 1.0) } else { Color::new(0.22, 0.18, 0.12, 1.0) };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 1.0, if on { ACCENT } else { DIM });
    let size = fit_size(label, w - 6.0, 17.0);
    let label = &ellipsize(label, w - 6.0, size);
    let d = measure(label, size);
    text(label, x + (w - d.width) / 2.0, y + (h + d.offset_y) / 2.0 - 1.0, size, INK);
    let pressed = hover && clicked();
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

/// A drop-down list: shows the option with value `current` (or its number) and opens a list
/// to pick from (type to filter, wheel to scroll). Returns the value picked.
pub fn dropdown(key: &str, x: f32, y: f32, w: f32, current: i64, options: &[(i64, String)]) -> Option<i64> {
    let id = field_id(key);
    let picked = PICKED.with(|p| match p.get() {
        Some((pid, v)) if pid == id => {
            p.set(None);
            Some(v)
        }
        _ => None,
    });
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
        claim();
        clear_focus();
        let rows = options.len().min(POPUP_ROWS) as f32;
        let ph = rows * POPUP_ROW_H + 30.0;
        let py = if y + h + ph > screen_height() { (y - ph).max(0.0) } else { y + h };
        let pw = w.max(220.0);
        let px = x.min(screen_width() - pw).max(0.0);
        let at = options.iter().position(|o| o.0 == current).unwrap_or(0);
        POPUP.with(|p| {
            *p.borrow_mut() = Some(Popup { id, rect: Rect::new(px, py, pw, ph), options: options.to_vec(), scroll: at.saturating_sub(POPUP_ROWS / 2), filter: String::new() })
        });
    }
    picked.filter(|v| *v != current)
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
        // Typing filters; Backspace deletes from the filter.
        let (chars, back, _) = typed();
        apply_typing(&mut pop.filter, &chars, back, false, false, 40);
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
        let hint = if pop.filter.is_empty() { razdor::i18n::tr("type to filter").to_string() } else { razdor::trf!("filter: {filter}", filter = pop.filter) };
        text(&hint, r.x + 8.0, r.y + 19.0, 15.0, DIM);
        for (i, (v, label)) in shown.iter().skip(pop.scroll).take(POPUP_ROWS).enumerate() {
            let ry = r.y + 26.0 + i as f32 * POPUP_ROW_H;
            let hover = Rect::new(r.x, ry, r.w, POPUP_ROW_H).contains(Vec2::from(pointer()));
            if hover {
                draw_rectangle(r.x + 2.0, ry, r.w - 4.0, POPUP_ROW_H, Color::new(0.4, 0.3, 0.15, 1.0));
            }
            let mut l = label.clone();
            while measure(&l, 16.0).width > r.w - 16.0 && !l.is_empty() {
                l.pop();
            }
            text(&l, r.x + 8.0, ry + 16.0, 16.0, INK);
            if hover && is_mouse_button_pressed(MouseButton::Left) {
                PICKED.with(|k| k.set(Some((pop.id, *v))));
                close = true;
            }
        }
        if shown.len() > POPUP_ROWS {
            let frac = pop.scroll as f32 / max_scroll.max(1) as f32;
            let track = r.h - 30.0;
            draw_rectangle(r.x + r.w - 5.0, r.y + 26.0 + frac * (track - 20.0), 3.0, 20.0, DIM);
        }
        let outside = is_mouse_button_pressed(MouseButton::Left) && !r.contains(Vec2::from(pointer()));
        if outside || is_key_pressed(KeyCode::Escape) {
            close = true;
        }
    });
    if close {
        POPUP.with(|p| *p.borrow_mut() = None);
        claim();
    }
}

/// A row of tabs; the clicked one becomes `selected`. Returns the height used.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_box_takes_any_key_but_four_as_yes() {
        use KeyCode::*;
        assert_eq!(answer_of([Escape].into_iter()), Some(false));
        for k in [Enter, Space, Y, N, A, F1, Left, LeftShift] {
            assert_eq!(answer_of([k].into_iter()), Some(true), "{k:?}");
        }
        for k in [Tab, LeftAlt, RightAlt, Up, Down] {
            assert_eq!(answer_of([k].into_iter()), None, "{k:?}");
        }
        assert_eq!(answer_of(std::iter::empty()), None);
    }

    #[test]
    fn typing_edits_text() {
        let mut s = String::from("Ка");
        assert!(apply_typing(&mut s, &['м', 'е', 'н', 'ь'], 0, false, false, 100));
        assert_eq!(s, "Камень");
        assert!(apply_typing(&mut s, &[], 2, false, false, 100));
        assert_eq!(s, "Каме");
        // Control characters are not text; Enter breaks lines only in multi-line fields.
        assert!(!apply_typing(&mut s, &['\u{8}', '\r'], 0, true, false, 100));
        assert!(apply_typing(&mut s, &[], 0, true, true, 100));
        assert_eq!(s, "Каме\n");
        let mut t = String::new();
        apply_typing(&mut t, &['a', 'b', 'c'], 0, false, false, 2);
        assert_eq!(t, "ab");
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
}
