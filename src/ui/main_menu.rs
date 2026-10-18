//! The original's main menu (the 18:08 frame of the gameplay video): the burning ruins
//! (`Windows/Castle.lit`) over the whole screen, the silver "Времена Раздора" logo
//! (`Title_RUS.ugs`) with flames rising off its letters (`TitleFlame_RUS.lit`), and five oval
//! buttons (`mb2.ugs`, the last one pointed) whose marble middles swirl in violet
//! (`MenuFlame.lit`) under the words of `SMText_RUS.lit`; the button under the mouse turns
//! gold. Razdor's own extras, the map editor, the custom battle and the interface language,
//! sit small in the bottom corners. Layout numbers are pixels of the 960×720 video, scaled by [`chrome::k`].

use crate::ui::input::{is_mouse_button_down};
use macroquad::prelude::*;

use razdor::i18n::{self, n_, tr, Lang};

use super::audio::{cue, Cue};
use super::chrome::{self, Fx};
use super::widgets::*;

/// What the player picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    NewGame,
    Load,
    Options,
    Authors,
    Exit,
    Editor,
    /// Razdor's custom battle (`ui::custom_battle`).
    Custom,
}

/// The five buttons, top to bottom, with the row of `SMText_RUS` holding their word.
const BUTTONS: [(Pick, &str); 5] = [
    (Pick::NewGame, n_("New game")),
    (Pick::Load, n_("Load")),
    (Pick::Options, n_("Options")),
    (Pick::Authors, n_("Authors")),
    (Pick::Exit, n_("Exit")),
];

/// Centre of button `i` (video pixels, x from the screen centre).
fn button_centre(i: usize) -> Vec2 {
    vec2(0.0, 232.0 + i as f32 * 92.0)
}

/// Size of an oval button (video pixels): `mb2.ugs` drawn 1:1 at 1024×768.
const OVAL: Vec2 = Vec2::new(214.0, 112.0);

// Final title-flame placement from the temporary development tuner.
const LOGO_FLAME_X: f32 = -16.0;
const LOGO_FLAME_Y: f32 = 12.0;
const LOGO_FLAME_WIDTH: f32 = 0.95;
const LOGO_FLAME_HEIGHT: f32 = 1.52;
const LOGO_FLAME_GLOW: f32 = 0.85;

fn video(p: Vec2) -> Vec2 {
    let k = chrome::k();
    vec2(screen_width() / 2.0 + p.x * k, p.y * k)
}

/// The burning ruins over the whole screen, a little warmer where the fire glows.
fn background() {
    match chrome::win("Castle") {
        Some(t) => chrome::tex(&t, Rect::new(0.0, 0.0, screen_width(), screen_height()), WHITE),
        None => clear_background(Color::from_rgba(60, 30, 12, 255)),
    }
}

/// The current and next frame, and the blend between them, for a looping animation.
fn flame_phase(t: f64, fps: f64, frames: usize) -> (usize, usize, f32) {
    let phase = (t * fps).rem_euclid(frames as f64);
    let current = phase.floor() as usize;
    (current, (current + 1) % frames, phase.fract() as f32)
}

/// One cell of the 4×4 sheet of 128×128 flame frames.
fn flame_frame(frame: usize) -> Rect {
    Rect::new((frame % 4) as f32 * 128.0, (frame / 4) as f32 * 128.0, 128.0, 128.0)
}

/// Cross-fades consecutive frames so the additive flame grows and dies away smoothly.
fn draw_flame_frames(
    texture: &Texture2D,
    t: f64,
    fps: f64,
    frames: usize,
    source: impl Fn(usize) -> Rect,
    dest: Rect,
    tint: Color,
) {
    let (current, next, blend) = flame_phase(t, fps, frames);
    chrome::additive(|| {
        let mut current_tint = tint;
        current_tint.a *= 1.0 - blend;
        chrome::tex_src(texture, source(current), dest, current_tint);
        if blend > 0.0 {
            let mut next_tint = tint;
            next_tint.a *= blend;
            chrome::tex_src(texture, source(next), dest, next_tint);
        }
    });
}

/// The logo, with the flames of `TitleFlame_RUS` (9 frames of the two words, one above the
/// other) licking up behind its letters.
fn logo() {
    let k = chrome::k();
    let w = 840.0 * k;
    let h = w * 128.0 / 896.0;
    // The letters sit left in the picture: the video shows them centred.
    let x = screen_width() / 2.0 - w / 2.0 + 25.0 * k;
    let y = 50.0 * k;
    if let Some(fl) = chrome::win_fx("TitleFlame_RUS", Fx::Glow) {
        let fh = fl.height() / 9.0;
        let fw = fl.width() / 2.0;
        // Keep the flame frame's aspect ratio and centre it over the visible lettering.
        // The source frame follows the word's silhouette; centring lets it fill the whole
        // text shape while the silver logo drawn below stays legible on top.
        let base_w = w / 2.0;
        let base_h = base_w * fh / fw;
        let base_y = y + (h - base_h) / 2.0;
        // Let the glow extend past the logo's sides and top while keeping the bottom
        // aligned with the letter flames in the source art.
        let flame_w = base_w * LOGO_FLAME_WIDTH;
        let flame_h = base_h * LOGO_FLAME_HEIGHT;
        let flame_y = base_y + base_h - flame_h + LOGO_FLAME_Y * k;
        let t = get_time();
        // The two words, each half of the sheet, over each half of the logo.
        for half in 0..2 {
            let dst_x = x + half as f32 * base_w - (flame_w - base_w) / 2.0 + LOGO_FLAME_X * k;
            let dst = Rect::new(dst_x, flame_y, flame_w, flame_h);
            draw_flame_frames(
                &fl,
                t,
                10.0,
                9,
                |frame| {
                    Rect::new(half as f32 * fw, frame as f32 * fh, fw, fh)
                },
                dst,
                Color::new(1.0, 0.55, 0.2, LOGO_FLAME_GLOW),
            );
        }
    }
    match chrome::win_ugs("Title_RUS") {
        Some(t) => chrome::tex(&t, Rect::new(x, y, w, h), WHITE),
        None => text_centered(tr("A time of discord"), screen_width() / 2.0, y + h * 0.7, 64.0 * k, chrome::SILVER),
    }
}

thread_local! {
    /// The main menu item under the pointer at the last frame.
    static HOVERED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// An oval button: the violet swirl, the frame (gold under the mouse) and its word. Returns
/// true when clicked.
fn oval(i: usize, label: &str, hot: Option<usize>) -> bool {
    let k = chrome::k();
    let c = video(button_centre(i));
    let size = OVAL * k;
    let r = oval_rect(i);
    let (hover, _down, pressed) = super::widgets::button_state(r, hot == Some(i));
    // The bell as the pointer comes onto an item (interface.md §4, 0x4b9b3c); leaving clears
    // the light.
    HOVERED.with(|h| {
        if hover && h.get() != Some(i) {
            h.set(Some(i));
            cue(Cue::MenuSelect);
        } else if !hover && h.get() == Some(i) {
            h.set(None);
        }
    });
    // Fire behind the button (the glow around the column of buttons in the video).
    if let Some(fl) = chrome::win_fx("MenuFlame", Fx::Glow) {
        let f = Rect::new(c.x - 190.0 * k, c.y - 130.0 * k, 380.0 * k, 260.0 * k);
        draw_flame_frames(
            &fl,
            // The previous per-button offset jumped almost four frames at a time;
            // keep a small ordered phase step down the menu instead.
            get_time() * 0.8 + i as f64 * 0.10,
            6.0,
            16,
            flame_frame,
            f,
            Color::new(1.0, 0.55, 0.2, 0.5),
        );
    }
    // `mb2.ugs`: the round oval on top, the pointed one (the last button) below.
    if let Some(t) = chrome::win_ugs("mb2") {
        let pointed = i + 1 == BUTTONS.len();
        let half = t.height() / 2.0;
        let src = Rect::new(0.0, if pointed { half } else { 0.0 }, t.width(), half);
        let frame = if hover { Color::new(1.0, 0.80, 0.42, 1.0) } else { WHITE };
        let s = k * 0.9375;
        // Each oval's middle is 46 px below the top of its half; the pointed one is drawn
        // in the left 196 px of the picture.
        let dx = if pointed { 16.0 } else { 0.0 };
        let dst = Rect::new(c.x - (t.width() / 2.0 - dx) * s, c.y - 46.0 * s, t.width() * s, half * s);
        chrome::tex_src(&t, src, dst, frame);
    } else {
        draw_ellipse_lines(c.x, c.y, size.x / 2.0, size.y / 2.0, 0.0, 3.0, if hover { chrome::GOLD } else { chrome::SILVER });
    }
    // The marble middle glows violet (warm under the mouse), a flame swirling in it.
    let pointed = i + 1 == BUTTONS.len();
    let (iw, ih) = if pointed { (0.54, 0.42) } else { (0.62, 0.46) };
    let inner = Rect::new(c.x - r.w * iw / 2.0, c.y - r.h * ih / 2.0, r.w * iw, r.h * ih);
    let wash = if hover { Color::new(0.75, 0.40, 0.45, 0.40) } else { Color::new(0.30, 0.26, 0.90, 0.42) };
    draw_ellipse(inner.center().x, inner.center().y, inner.w / 2.0, inner.h / 2.0, 0.0, wash);
    if let Some(fl) = chrome::win_fx("MenuFlame", Fx::Glow) {
        let tint = if hover { Color::new(1.0, 0.8, 0.7, 0.5) } else { Color::new(0.8, 0.65, 1.0, 0.5) };
        draw_flame_frames(
            &fl,
            get_time() + i as f64 * 0.37,
            12.0,
            16,
            flame_frame,
            inner,
            tint,
        );
    }
    word(i, label, c, hover);
    if pressed {
        cue(Cue::MenuPress);
        return true;
    }
    false
}

/// How much bigger than the sheet the words of `SMText_RUS` are drawn.
const WORD_SCALE: f32 = 1.05;
/// The engraved words' capital height and the widest word's width (sheet pixels of
/// `SMText_RUS`: «Н» of «Новая Игра» is 16 rows, «Н» of «Настройки» 15; «Новая Игра» 124
/// columns): an English word is drawn as tall, and at most as wide.
const WORD_CAP: f32 = 15.5;
const WORD_WIDTH: f32 = 124.0;
/// The dark rim round a word (video pixels).
const WORD_RIM: f32 = 1.5;
const WORD_EDGE: Color = Color::new(0.12, 0.05, 0.25, 0.8);

/// The button's word: its row of `SMText_RUS` in Russian, else the text, as large and with
/// the same rim.
fn word(i: usize, label: &str, c: Vec2, hover: bool) {
    let k = chrome::k();
    let color = if hover { Color::new(1.0, 0.93, 0.80, 1.0) } else { Color::new(0.93, 0.90, 1.0, 1.0) };
    let sheet = (i18n::lang() == Lang::Ru).then(|| chrome::win_fx("SMText_RUS", Fx::Glow)).flatten();
    match sheet {
        Some(t) => {
            let rh = t.height() / BUTTONS.len() as f32;
            let src = Rect::new(0.0, i as f32 * rh, t.width(), rh);
            let (w, h) = (t.width() * WORD_SCALE * k, rh * WORD_SCALE * k);
            let dst = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
            // A dark rim, then the word.
            for (dx, dy) in [(-WORD_RIM, 0.0), (WORD_RIM, 0.0), (0.0, -WORD_RIM), (0.0, WORD_RIM)] {
                chrome::tex_src(&t, src, Rect::new(dst.x + dx * k, dst.y + dy * k, dst.w, dst.h), WORD_EDGE);
            }
            chrome::tex_src(&t, src, dst, color);
        }
        None => {
            let box_ = (WORD_CAP * WORD_SCALE * k, WORD_WIDTH * WORD_SCALE * k);
            chrome::engraved_centered(label, c, box_, (WORD_RIM * k, (0.6 * k).max(1.0)), color, WORD_EDGE);
        }
    }
}

/// A small text link (the Razdor extras). Returns true when clicked.
fn link(label: &str, x: f32, y: f32, right: bool) -> bool {
    let size = 18.0 * chrome::k();
    let w = measure(label, size).width;
    let x = if right { x - w } else { x };
    let (_, down, pressed) = super::widgets::button_state(Rect::new(x, y - size, w, size * 1.3), true);
    let color = if down { chrome::GOLD } else { Color::new(0.95, 0.88, 0.72, 0.85) };
    chrome::shadow_text(label, x, y, size, color);
    if pressed {
        cue(Cue::Button);
        return true;
    }
    false
}

/// The menu's ruins and logo without the buttons (behind the load and new-game windows).
pub fn backdrop() {
    background();
    logo();
    foot_lines();
}

/// The menu's foot (4c7774): the version line (`[SpecialTexts] VerInfo`) at the left and the
/// copyright (`CopyrightsMessage`) after it, small and white, on the bottom row.
fn foot_lines() {
    let k = chrome::k();
    let size = 11.0 * k;
    let y = screen_height() - 4.0 * k;
    let ink = Color::new(0.92, 0.92, 0.92, 1.0);
    if let Some(v) = chrome::ui_text("SpecialTexts", "VerInfo") {
        chrome::shadow_text(&v, 5.0 * k, y, size, ink);
    }
    if let Some(c) = chrome::ui_text("SpecialTexts", "CopyrightsMessage") {
        chrome::shadow_text(&c, 264.0 * k, y, size, ink);
    }
}

/// The screen rectangle of oval button `i`.
fn oval_rect(i: usize) -> Rect {
    let k = chrome::k();
    let c = video(button_centre(i));
    let size = OVAL * k;
    Rect::new(c.x - size.x / 2.0, c.y - size.y / 2.0, size.x, size.y)
}

/// The one button under the pointer: neighbouring ovals overlap by 20 px, and the original's
/// hit test (0x4743c8) reports a single widget, the first in order. Without this the pointer in
/// an overlap counted as on two buttons, which took the light from each other every frame and
/// rang the bell each time.
fn hot_button(pointer: Vec2, n: usize) -> Option<usize> {
    (0..n).find(|&i| oval_rect(i).contains(pointer))
}

/// Draws the menu; returns what was picked.
pub fn frame() -> Option<Pick> {
    backdrop();
    let mut pick = None;
    let hot = if input_blocked() { None } else { hot_button(crate::ui::widgets::pointer().into(), BUTTONS.len()) };
    for (i, (p, label)) in BUTTONS.iter().enumerate() {
        if oval(i, tr(label), hot) {
            pick = Some(*p);
        }
    }
    let k = chrome::k();
    // Razdor's links, over the original's version and copyright lines.
    let y = screen_height() - 22.0 * k;
    let editor = tr("Map editor");
    if link(editor, 16.0 * k, y, false) {
        pick = Some(Pick::Editor);
    }
    let x = 16.0 * k + measure(editor, 18.0 * k).width + 28.0 * k;
    if link(tr("Custom battle"), x, y, false) {
        pick = Some(Pick::Custom);
    }
    // The language list (Razdor's): the current language; a click opens the list over it.
    let lang = super::language::current_label();
    let size = 18.0 * k;
    let lw = measure(&lang, size).width;
    let anchor = Rect::new(screen_width() - 16.0 * k - lw, y - size, lw, size * 1.3);
    if link(&lang, screen_width() - 16.0 * k, y, true) || super::snapshot::open_language_list() {
        super::language::open_list(anchor, k);
    }
    if super::widgets::mouse_in(anchor.x, anchor.y, anchor.w, anchor.h) {
        super::widgets::tooltip(&[(super::language::hint().to_string(), super::widgets::INK)]);
    }
    // This program's version, small in the corner above the language switch.
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let vw = measure(version, 12.0 * k).width;
    chrome::shadow_text(version, screen_width() - 16.0 * k - vw, y - 26.0 * k, 12.0 * k, Color::new(0.95, 0.88, 0.72, 0.6));
    // An update put in place says so (an "Always" update has no window of its own).
    super::update_view::title_line();
    // Esc quits the game at once, without a question, as in the original (0x4c8059); a
    // gamepad's Start, which is Esc elsewhere, does not (Razdor's: pressed to open a menu,
    // it would end the game).
    if key(KeyCode::Escape) && !crate::ui::input::pad_key_pressed(KeyCode::Escape) {
        pick = Some(Pick::Exit);
    }
    pick
}

/// The credits picture of the install: its dark outline (`Credits_Alpha` as the alpha of
/// black), its letters (`Credits_Color`, drawn additively), and the row of its first letters
/// (it starts with a blank stretch).
#[derive(Clone)]
struct Credits {
    shadow: Texture2D,
    letters: Texture2D,
    first_row: f32,
}

thread_local! {
    /// The credits, made once.
    static CREDITS: std::cell::OnceCell<Option<Credits>> = const { std::cell::OnceCell::new() };
    /// The credits window the player scrolled (when it opened, how far from the timed scroll
    /// in the picture's pixels).
    static CREDITS_NUDGE: std::cell::Cell<(f64, f32)> = const { std::cell::Cell::new((f64::NAN, 0.0)) };
}

/// Razdor's own lines over the original's credits: their height in the picture's pixels.
const OWN_CREDITS_H: f32 = 96.0;

fn credits() -> Option<Credits> {
    CREDITS.with(|c| {
        c.get_or_init(|| {
            let mut color = chrome::image("Windows/Credits_Color.lit")?;
            let alpha = chrome::image("Windows/Credits_Alpha.lit")?;
            if (color.width, color.height) != (alpha.width, alpha.height) {
                return None;
            }
            let (w, h) = (u16::try_from(color.width).ok()?, u16::try_from(color.height).ok()?);
            let shadow: Vec<u8> = alpha.rgba.chunks_exact(4).flat_map(|a| [0, 0, 0, a[0].max(a[1]).max(a[2])]).collect();
            color.rgba.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            let make = |rgba: &[u8]| {
                let t = Texture2D::from_rgba8(w, h, rgba);
                t.set_filter(FilterMode::Linear);
                t
            };
            let width = color.width as usize;
            let first_row = color.rgba.chunks_exact(width * 4).position(|row| row.chunks_exact(4).any(|p| p[0].max(p[1]).max(p[2]) > 60)).unwrap_or(0);
            // A margin above, for the faint edges of the first letters.
            let first_row = first_row.saturating_sub(16);
            Some(Credits { shadow: make(&shadow), letters: make(&color.rgba), first_row: first_row as f32 })
        })
        .clone()
    })
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own(section: &str, key: &str, ours: &'static str) -> String {
    let t = (i18n::lang() == Lang::Ru).then(|| chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// A window over the ruins, `w`×`h` video pixels, under the logo. Returns its inside and
/// whether its close box was clicked.
fn window(title: &str, w: f32, h: f32) -> (Rect, bool) {
    let k = chrome::k();
    let (w, h) = (w * k, h * k);
    let r = Rect::new((screen_width() - w) / 2.0, (190.0 * k).min(screen_height() - h), w, h);
    chrome::window(r, title, chrome::Skin::Marble, true)
}

/// A window over the map, centred over the view above the bottom bar, as the original's map
/// windows are (over its 1024×682 view), its foot never on the bar.
fn map_window(title: &str, w: f32, h: f32) -> (Rect, bool) {
    let k = chrome::k();
    let (w, h) = (w * k, h * k);
    let view = screen_height() - chrome::bar_height();
    let r = Rect::new((screen_width() - w) / 2.0, ((view - h) / 2.0).max(0.0), w, h);
    chrome::window(r, title, chrome::Skin::Marble, true)
}

/// "Информация об авторах": the splash of scattered drawings with the credits rolling up
/// over it (`started` is when the window opened). A click or Esc closes it.
pub fn authors(started: f64) -> bool {
    backdrop();
    let title = own("Authors", "Title", n_("About the authors"));
    // The standard window, as the original's (4c82c4): the splash in a framed page, the
    // marble under it with «Ок» in its middle.
    let (page, closed) = window(&title, 594.0, 480.0);
    let k = chrome::k();
    let inner = Rect::new(page.x + 2.0 * k, page.y + 2.0 * k, page.w - 4.0 * k, page.h - 54.0 * k);
    let ok = Rect::new(page.center().x - 44.0 * k, page.y + page.h - 44.0 * k, 89.0 * k, 32.0 * k);
    let over_ok = ok.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    let ok_label = own("Buttons", "Ok", n_("OK"));
    chrome::marble_button(ok, &ok_label, true, over_ok);
    match chrome::win("Authors-Splash") {
        Some(t) => chrome::tex(&t, inner, WHITE),
        None => chrome::text_box(inner),
    }
    if let Some(c) = credits() {
        // As the original (0x4c82c4): the picture's top at the page's top to begin with (its
        // own blank start lets the first lines rise from below), 35 of its pixels a second,
        // and it stays on its last view once the end is in sight. Razdor's own lines come
        // first, before the original's; the wheel and the arrow keys scroll by hand, and the
        // scroll goes on from there.
        let px = k * 0.9375;
        let w = c.letters.width() * px;
        let total = c.letters.height() + OWN_CREDITS_H;
        let most = (total - inner.h / px).max(0.0);
        let (opened, mut nudge) = CREDITS_NUDGE.with(|n| n.get());
        if opened != started {
            nudge = 0.0;
        }
        let over = inner.contains(crate::ui::widgets::pointer().into());
        let step = 48.0;
        nudge -= if over { wheel() * step } else { 0.0 };
        if key(KeyCode::Down) {
            nudge += step;
        }
        if key(KeyCode::Up) {
            nudge -= step;
        }
        if key(KeyCode::PageDown) {
            nudge += inner.h / px * 0.8;
        }
        if key(KeyCode::PageUp) {
            nudge -= inner.h / px * 0.8;
        }
        let timed = (get_time() - started) as f32 * 35.0;
        // The clock while the credits roll on their own (Credits_OnOpen 0x4cfc6c calls
        // 0x48ecf0); the arrow once their time is up (0x4c84e7), whatever the hand scroll.
        if timed < most {
            super::cursor::set(super::cursor::Shape::Clock);
        }
        let scrolled = (timed + nudge).clamp(0.0, most);
        // Held at an end, the scroll owes nothing: turning back moves at once.
        CREDITS_NUDGE.with(|n| n.set((started, scrolled - timed)));
        let _clip = Clip::new(inner);
        // The picture's part from row `from` (its pixels), drawn from page row `at`.
        let picture = |from: f32, to: f32, at: f32| {
            let top = (inner.y + (at - scrolled) * px).max(inner.y);
            let bottom = (inner.y + (at + to - from - scrolled) * px).min(inner.y + inner.h);
            if bottom > top {
                let skip = (top - (inner.y + (at - scrolled) * px)) / px;
                let src = Rect::new(0.0, from + skip, c.letters.width(), (bottom - top) / px);
                let dst = Rect::new(inner.center().x - w / 2.0, top, w, bottom - top);
                chrome::tex_src(&c.shadow, src, dst, WHITE);
                chrome::additive(|| chrome::tex_src(&c.letters, src, dst, WHITE));
            }
        };
        picture(0.0, c.first_row, 0.0);
        // Razdor's modder, in the credits' colours: the role in gold, the name in white.
        let y = inner.y + (c.first_row + 24.0 - scrolled) * px;
        for (line, dy, size, color) in [(tr("RAZDOR modder"), 0.0, 20.0, chrome::GOLD), ("indicozy", 26.0, 22.0, WHITE)] {
            let lw = measure(line, size * px).width;
            chrome::shadow_text(line, inner.center().x - lw / 2.0, y + dy * px, size * px, color);
        }
        picture(c.first_row, c.letters.height(), c.first_row + OWN_CREDITS_H);
    }
    chrome::silver_frame(inner, 1.0);
    closed || key(KeyCode::Escape) || key(KeyCode::Enter) || (over_ok && clicked()) || (clicked() && inner.contains(crate::ui::widgets::pointer().into()))
}

/// A horizontal slider of the original (`HSTexture` track, `HSlider` knob): returns the
/// new value (0..1) while dragged or clicked.
fn slider(r: Rect, value: f32) -> Option<f32> {
    let k = chrome::k();
    match chrome::win("HSTexture") {
        Some(t) => chrome::three_slice(&t, r, 12.0, WHITE),
        None => draw_rectangle(r.x, r.y + r.h / 3.0, r.w, r.h / 3.0, chrome::SILVER_DARK),
    }
    let knob_w = 32.0 * k * r.h / (18.0 * k);
    let x = r.x + (r.w - knob_w) * value.clamp(0.0, 1.0);
    match chrome::win("HSlider") {
        Some(t) => chrome::tex(&t, Rect::new(x, r.y, knob_w, r.h), WHITE),
        None => draw_rectangle(x, r.y, knob_w, r.h, chrome::SILVER),
    }
    let (mx, my) = crate::ui::widgets::pointer();
    let over = !input_blocked() && Rect::new(r.x, r.y - 4.0 * k, r.w, r.h + 8.0 * k).contains(vec2(mx, my));
    (over && is_mouse_button_down(MouseButton::Left)).then(|| ((mx - r.x - knob_w / 2.0) / (r.w - knob_w)).clamp(0.0, 1.0))
}

/// The battle AI's level: the player's choice, else the install's "improved enemy AI in
/// battle" (`[Options] OptValue9`), else easy.
pub fn expert_ai(audio: &super::audio::Settings) -> bool {
    audio.expert_ai.unwrap_or_else(|| chrome::ui_text("Options", "OptValue9").is_some_and(|v| razdor::dt::ini::loose_int(&v) == 1))
}

/// Razdor's «Сила ИИ в бою» (the owner's request): the original's AI, its improved level
/// (`OptValue9`, [`expert_ai`]) or Razdor's «Максимум» for the enemy of the player's battles
/// (`razdor::rules::battle::Battle::max_choice`), which plays on the improved level's base.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiStrength {
    Original,
    Improved,
    Max,
}

impl AiStrength {
    pub fn next(self) -> AiStrength {
        match self {
            AiStrength::Original => AiStrength::Improved,
            AiStrength::Improved => AiStrength::Max,
            AiStrength::Max => AiStrength::Original,
        }
    }
}

/// The battle AI's strength now: «Максимум» if chosen, else by the improved AI's checkbox.
pub fn ai_strength(audio: &super::audio::Settings) -> AiStrength {
    match (audio.max_ai, expert_ai(audio)) {
        (true, _) => AiStrength::Max,
        (false, true) => AiStrength::Improved,
        (false, false) => AiStrength::Original,
    }
}

/// Chooses the battle AI's strength; the improved AI's checkbox (`OptValue9`) follows it.
pub fn set_ai_strength(audio: &mut super::audio::Settings, s: AiStrength) {
    audio.expert_ai = Some(s != AiStrength::Original);
    audio.max_ai = s == AiStrength::Max;
}

/// The improved AI's checkbox clicked: on gives the improved level, off the original's (and
/// so ends «Максимум»).
fn toggle_expert_ai(audio: &mut super::audio::Settings) {
    let on = expert_ai(audio);
    set_ai_strength(audio, if on { AiStrength::Original } else { AiStrength::Improved });
}

/// The battle animation speed (percent): the player's choice, else the install's
/// `[Options] AnimationSpeed` (the Community's); `None` without either (the vanilla game).
pub fn anim_speed(audio: &super::audio::Settings) -> Option<f32> {
    audio.anim_speed.or_else(|| chrome::options_value("AnimationSpeed").and_then(|v| v.trim().parse::<f32>().ok()).map(|s| s.clamp(0.0, 99.0)))
}

/// The front row's width for new games: the player's choice, else the install's "wide
/// front row in battle" (`[Options] OptValue11`).
pub fn wide_row(audio: &super::audio::Settings, install: bool) -> bool {
    audio.wide_row.unwrap_or(install)
}

/// An `[Options]` number of the install, `default` without it.
fn install_int(key: &str, default: i32) -> i32 {
    chrome::install_option(key).map_or(default, |v| razdor::dt::ini::loose_int(&v))
}

/// An `[Options]` checkbox of the install: on when its value reads 1 (interface.md §16);
/// `no_install` when Razdor runs without the install's texts.
fn install_on(key: &str, no_install: bool) -> bool {
    if !chrome::has_texts() {
        return no_install;
    }
    install_int(key, 0) == 1
}

/// "Скорость скроллинга карты" (`ScrollSpeed`, 0..100).
pub fn scroll_speed(audio: &super::audio::Settings) -> f32 {
    audio.scroll_speed.unwrap_or_else(|| install_int("ScrollSpeed", 100).clamp(0, 100) as f32)
}

/// "Скорость перемещения героя" (`WalkSpeed`, 0..100).
pub fn walk_speed(audio: &super::audio::Settings) -> f32 {
    audio.walk_speed.unwrap_or_else(|| install_int("WalkSpeed", 100).clamp(0, 100) as f32)
}

/// The world's real-time pace for the hero's speed (`Game::walk_pace`).
pub fn walk_pace(audio: &super::audio::Settings) -> f32 {
    razdor::rules::game::walk_pace(walk_speed(audio).round() as i32)
}

/// "Автоматически преследовать выбранную армию" (`OptValue7`); on without an install.
pub fn pursue(audio: &super::audio::Settings) -> bool {
    audio.pursue.unwrap_or_else(|| install_on("OptValue7", true))
}

/// "Отключить всплывающие подсказки" (`OptValue6`): hints off.
pub fn hints_off(audio: &super::audio::Settings) -> bool {
    audio.hints_off.unwrap_or_else(|| install_on("OptValue6", false))
}

/// "Автоматически сохранять текущую игру" (`OptValue8`); on without an install.
pub fn autosave(audio: &super::audio::Settings) -> bool {
    audio.autosave.unwrap_or_else(|| super::saves::autosaves_on(chrome::has_texts().then(|| chrome::install_option("OptValue8")).flatten().as_deref()))
}

/// "Не загружать музыку и звуки" (`OptValue4`), read at start.
pub fn no_sound(audio: &super::audio::Settings) -> bool {
    audio.no_sound.unwrap_or_else(|| install_on("OptValue4", false))
}

/// "Невозможный уровень сложности игры" (`OptValue10`), read at start.
pub fn impossible(audio: &super::audio::Settings) -> bool {
    audio.impossible.unwrap_or_else(|| install_on("OptValue10", false))
}

/// Puts the settings window's choices where the interface reads the install's `[Options]`
/// (the original writes them back into its ini): the scroll speed, the autosave, the hints.
pub fn apply_options(audio: &super::audio::Settings) {
    chrome::set_option_override("ScrollSpeed", audio.scroll_speed.map(|v| (v.round() as i32).to_string()));
    chrome::set_option_override("OptValue8", audio.autosave.map(|on| if on { "1" } else { "0" }.to_string()));
    set_hints(!hints_off(audio));
    super::unit_drag::set_controls(audio.formation_controls);
}

/// "Настройки звука, графики и геймплея", with what Razdor lets the player change: the
/// music and sound volumes (saved at once) and the interface language. Returns true when
/// closed.
pub fn options(audio: &mut super::audio::Settings, install_wide: bool) -> bool {
    backdrop();
    options_window(audio, install_wide, false)
}

thread_local! {
    /// The advanced settings window is open over the settings.
    static ADVANCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The settings as the window found them: «Отмена» puts them back (the original's
    /// Cancel drops what was changed, its Apply keeps it).
    static OPENED_WITH: std::cell::Cell<Option<super::audio::Settings>> = const { std::cell::Cell::new(None) };
}

/// Opens the advanced settings over the settings window (a snapshot scene).
pub fn open_advanced() {
    ADVANCED.with(|a| a.set(true));
}

/// A button of Razdor's own rows (`label` on the marble), true when clicked.
fn row_button(r: Rect, label: &str) -> bool {
    settings_button(r, label)
}

/// Razdor's advanced settings (the settings window's "Ещё…"): rules where Razdor can differ
/// from the original on the left, the screen, the interface scale, the font and the
/// language on the right. Returns true when closed.
fn advanced_window(audio: &mut super::audio::Settings, over_map: bool) -> bool {
    // Razdor's own window, wider than the original's: centred (over the map, above the bar),
    // not hung from the menu's 190, where its foot reached the screen's bottom edge.
    let k = chrome::k();
    // 650 high since «Расстановка армии» joined the left column (it still fits the 682 px
    // view above the bar).
    let (ww, wh) = (900.0, 650.0);
    let (inner, closed) = if over_map {
        map_window(tr("Advanced settings"), ww, wh)
    } else {
        let (w, h) = (ww * k, wh * k);
        let r = Rect::new((screen_width() - w) / 2.0, ((screen_height() - h) / 2.0).max(0.0), w, h);
        chrome::window(r, tr("Advanced settings"), chrome::Skin::Marble, true)
    };
    // The left column: the rules, each with its explanation.
    let col = Rect::new(inner.x, inner.y, 560.0 * k, inner.h);
    let y = col.y + 30.0 * k;
    chrome::shadow_text(tr("Stepping onto a friendly army"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let br = Rect::new(col.x + col.w - 24.0 * k - 170.0 * k, y, 170.0 * k, 28.0 * k);
    let label = if audio.friends_let_pass { tr("Let pass") } else { tr("Battle (original)") };
    if row_button(br, label) {
        audio.friends_let_pass = !audio.friends_let_pass;
    }
    let hint = tr("With no event of the map for the meeting, the original opens a battle against a friendly army the hero steps onto; Razdor can let him pass instead.");
    for (i, line) in wrap(hint, col.w - 48.0 * k, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 56.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // Armies only passing by on a bridge (Razdor's): the original meets each one.
    let y = y + 100.0 * k;
    chrome::shadow_text(tr("Armies on bridges"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let pr = Rect::new(br.x, y, br.w, br.h);
    let label = if audio.pass_on_bridges { tr("Let pass") } else { tr("Stop (original)") };
    if row_button(pr, label) {
        audio.pass_on_bridges = !audio.pass_on_bridges;
    }
    let hint = tr("The original stops the hero at any army on a bridge. Razdor can let him through those only passing by; guards, enemies and the army clicked still meet him.");
    for (i, line) in wrap(hint, col.w - 48.0 * k, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 56.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // Updates from the GitHub releases (`razdor::update`): offered, put in place quietly, or
    // not looked for; "Check now" asks at once and shows the answer.
    let y = y + 100.0 * k;
    chrome::shadow_text(tr("Updates"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let ur = Rect::new(br.x, y, br.w, br.h);
    let mode = match audio.updates {
        razdor::update::Mode::Ask => tr("Ask"),
        razdor::update::Mode::Always => tr("Always"),
        razdor::update::Mode::Off => tr("Off"),
    };
    if row_button(ur, mode) {
        audio.updates = audio.updates.next();
    }
    let cr = Rect::new(ur.x, y + 40.0 * k, ur.w, ur.h);
    if row_button(cr, tr("Check now")) {
        super::update_view::check_by_hand();
    }
    let state = super::update_view::status_label();
    let sw = measure(&state, 13.0 * k).width;
    chrome::shadow_text(&state, cr.x - 16.0 * k - sw, cr.y + 19.0 * k, 13.0 * k, chrome::GOLD);
    let hint = tr("Ask: a newer release is offered in a window. Always: it is downloaded and starts the next time. The check never holds up the game.");
    for (i, line) in wrap(hint, col.w - 48.0 * k, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 92.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // The map's own zoom (Razdor's): the wheel steps from it, 0 comes back to it; locked, the
    // wheel and +/- leave it alone.
    let y = y + 140.0 * k;
    chrome::shadow_text(tr("Map zoom"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let zr = Rect::new(br.x, y, br.w, br.h);
    let lock_label = if audio.zoom_locked { tr("Locked") } else { tr("Wheel and +/-") };
    if row_button(zr, lock_label) {
        audio.zoom_locked = !audio.zoom_locked;
    }
    let (lo, hi) = super::world_view::ZOOM_RANGE;
    let value = format!("×{:.2}", audio.map_zoom);
    let track = Rect::new(col.x + 24.0 * k, y + 44.0 * k, col.w - 48.0 * k - 70.0 * k, 18.0 * k);
    chrome::shadow_text(&value, track.x + track.w + 16.0 * k, track.y + 14.0 * k, 14.0 * k, chrome::GOLD);
    if let Some(v) = slider(track, (audio.map_zoom - lo) / (hi - lo)) {
        audio.map_zoom = ((lo + v * (hi - lo)) / 0.05).round() * 0.05;
    }
    let hint = tr("The zoom the map plays at. The wheel steps away from it and the 0 key comes back to it; locked, the map always shows it.");
    for (i, line) in wrap(hint, col.w - 48.0 * k, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 80.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // The army's arrangement outside battle (Razdor's, the owner's decision after a player's
    // report): dragging only, or the original's press to pick and press to move (0x4c346c)
    // with dragging on top (`unit_drag::Controls`).
    let y = y + 136.0 * k;
    chrome::shadow_text(tr("Arranging the army"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let ar = Rect::new(br.x, y, br.w, br.h);
    let label = match audio.formation_controls {
        super::unit_drag::Controls::Drag => tr("Dragging"),
        super::unit_drag::Controls::Original => tr("As in the original"),
    };
    if row_button(ar, label) {
        audio.formation_controls = audio.formation_controls.next();
    }
    let hint = tr("Dragging: a click on a unit only selects it; units and items are moved by dragging. As in the original: the first click picks a unit, the second moves it; dragging works too.");
    for (i, line) in wrap(hint, col.w - 48.0 * k, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 56.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // The right column: how Razdor shows the game.
    let rx = inner.x + 580.0 * k;
    let rw = inner.x + inner.w - 24.0 * k - rx;
    let bw = 150.0 * k;
    let mut y = inner.y + 30.0 * k;
    let row = |label: &str, value: &str, y: &mut f32| -> bool {
        chrome::shadow_text(label, rx, *y + 18.0 * k, 14.0 * k, chrome::CREAM);
        let r = Rect::new(rx + rw - bw, *y, bw, 28.0 * k);
        *y += 52.0 * k;
        row_button(r, value)
    };
    // Windowed, borderless or full screen, set at once (`display::follow_settings`).
    if row(tr("Screen (Alt+Enter)"), audio.display.label(), &mut y) {
        audio.display = audio.display.next();
    }
    // The interface scale, as Minecraft's: Auto (the largest that fits) or a fixed one below it.
    let fit = chrome::fit_k();
    if row(tr("Interface scale"), &super::display::scale_label(audio.ui_scale, fit), &mut y) {
        audio.ui_scale = super::display::next_scale(audio.ui_scale, fit);
    }
    // The interface's fonts (Razdor's choice): the original's sheets from the install, or PT
    // Serif; at once.
    let font_label = match audio.font {
        super::dt_font::FontMode::Original => tr("As in the original"),
        super::dt_font::FontMode::Modern => tr("Modern"),
    };
    if row(tr("Font"), font_label, &mut y) {
        audio.font = audio.font.next();
    }
    // The battle AI's strength (Razdor's): the original's, its improved level, or «Максимум».
    let strength = ai_strength(audio);
    let strength_label = match strength {
        AiStrength::Original => tr("As in the original"),
        AiStrength::Improved => tr("Improved"),
        AiStrength::Max => tr("Maximum (Razdor)"),
    };
    if row(tr("Battle AI strength"), strength_label, &mut y) {
        set_ai_strength(audio, strength.next());
    }
    let hint = tr("Maximum: before each move the enemy of your battles weighs every move and answer ahead and plays each out to the end of the battle.");
    let lines = wrap(hint, rw, 12.0 * k);
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_text(line, rx, y - 8.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    y += lines.len() as f32 * 17.0 * k;
    // The language list (Razdor's): Russian, English and the translations of the data found
    // in the install; the button opens it.
    chrome::shadow_text(tr("Language"), rx, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let lw = (rw - 100.0 * k).max(bw);
    let lr = Rect::new(rx + rw - lw, y, lw, 28.0 * k);
    if row_button(lr, &super::language::current_label()) || super::snapshot::open_language_list() {
        super::language::open_list(lr, k);
    }
    for (i, line) in wrap(super::language::hint(), rw, 12.0 * k).iter().enumerate() {
        chrome::shadow_text(line, rx, y + 48.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    let ok = Rect::new(inner.x + inner.w - 120.0 * k, inner.y + inner.h - 44.0 * k, 96.0 * k, 28.0 * k);
    let ok_label = own("Buttons", "Ok", n_("OK"));
    closed || key(KeyCode::Escape) || key(KeyCode::Enter) || settings_button(ok, &ok_label)
}

fn settings_button(r: Rect, label: &str) -> bool {
    let (_, down, pressed) = super::widgets::button_state(r, true);
    chrome::marble_button(r, label, true, down);
    if pressed { cue(Cue::Button); }
    pressed
}

/// One of the original's slider rows (0x4d01c9: 32 px boxes 40 apart from 45 px under the
/// window's top, 18 px in from its sides, a dark fill and a frame, the label 8 px in): the
/// label on the left, ◀ the track ▶ on the right. Returns the new value (0..1) when moved.
fn option_slider(r: Rect, label: &str, value: f32) -> Option<f32> {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    chrome::silver_frame(r, 1.0);
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        chrome::shadow_text(label, r.x + 8.0 * k, r.y + r.h * 0.5 + 6.0 * k, 15.0 * k, OPTION_INK);
    });
    let a = 17.0 * k;
    let left = Rect::new(r.x + r.w * 0.505, r.y + (r.h - a) / 2.0, a, a);
    let right = Rect::new(r.x + r.w - 6.0 * k - a, left.y, a, a);
    let track = Rect::new(left.x + left.w, left.y, right.x - left.x - left.w, a);
    for (art, ar) in [("HArrowLeft", left), ("HArrowRight", right)] {
        match chrome::win(art) {
            Some(t) => chrome::tex(&t, ar, WHITE),
            None => draw_rectangle(ar.x, ar.y, ar.w, ar.h, chrome::SILVER),
        }
    }
    let over = |ar: Rect| ar.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    if over(left) && clicked() {
        return Some((value - 0.05).max(0.0));
    }
    if over(right) && clicked() {
        return Some((value + 0.05).min(1.0));
    }
    slider(track, value)
}

/// The labels of the settings window's slider rows: pale green, as the original's.
const OPTION_INK: Color = Color::new(0.78, 0.95, 0.55, 1.0);
/// A checkbox's label (the ae24a0 tint) and the "no sound" one (ae24a4), as the original's.
const CHECK_INK: Color = Color::new(0.98, 0.92, 0.35, 1.0);
const CHECK_ORANGE: Color = Color::new(1.0, 0.55, 0.15, 1.0);

/// One of the original's checkboxes (`CBoxChecked` / `CBoxUn` with their alpha masks): true
/// when clicked (on the box or its label).
fn option_check(x: f32, y: f32, label: &str, on: bool, ink: Color) -> bool {
    let k = chrome::k();
    // The art is 28 px square, drawn at its size (as the original's 0x4d0334 widgets).
    let b = Rect::new(x, y - 4.0 * k, 26.0 * k, 26.0 * k);
    let (art, mask) = if on { ("CBoxChecked", "CBoxChecked-Alpha") } else { ("CBoxUn", "CBoxUn-Alpha") };
    match chrome::win_masked(art, mask) {
        Some(t) => chrome::tex(&t, b, WHITE),
        None => {
            draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.0, 0.0, 0.0, 0.5));
            chrome::silver_frame(b, 1.0);
            if on {
                draw_line(b.x + 3.0 * k, b.y + 9.0 * k, b.x + 8.0 * k, b.y + 15.0 * k, 2.5 * k, GREEN);
                draw_line(b.x + 8.0 * k, b.y + 15.0 * k, b.x + 16.0 * k, b.y + 2.0 * k, 2.5 * k, GREEN);
            }
        }
    }
    let size = 15.0 * k;
    let w = super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        chrome::shadow_text(label, b.x + b.w + 6.0 * k, b.y + b.h * 0.5 + 6.0 * k, size, ink);
        measure(label, size).width
    });
    let hit = Rect::new(b.x, b.y, b.w + 6.0 * k + w, b.h);
    let clicked_it = hit.contains(crate::ui::widgets::pointer().into()) && !input_blocked() && clicked();
    if clicked_it {
        cue(Cue::Button);
    }
    clicked_it
}

/// The settings window alone (over the main menu, or `over_map` from the bar's gears: then
/// above the bar, not over it): the original's "Настройки звука, графики и геймплея"
/// (0x4d01c9, interface.md §16): five slider rows, eight checkboxes, «Принять» and
/// «Отмена»; Razdor's own settings behind «Ещё…».
pub fn options_window(audio: &mut super::audio::Settings, install_wide: bool, over_map: bool) -> bool {
    if ADVANCED.with(|a| a.get()) {
        if advanced_window(audio, over_map) {
            ADVANCED.with(|a| a.set(false));
            // What the advanced window set is kept, whatever the settings window ends with.
            OPENED_WITH.with(|o| o.set(Some(*audio)));
        }
        return false;
    }
    if OPENED_WITH.with(|o| o.get()).is_none() {
        OPENED_WITH.with(|o| o.set(Some(*audio)));
    }
    let title = own("Options", "Title", n_("Sound, graphics and gameplay settings"));
    let (inner, closed) = if over_map { map_window(&title, 594.0, 500.0) } else { window(&title, 594.0, 500.0) };
    let k = chrome::k();
    let row = |i: usize| Rect::new(inner.x + 10.0 * k, inner.y + 8.0 * k + i as f32 * 37.5 * k, inner.w - 20.0 * k, 30.0 * k);
    // The volumes, in steps of 1%.
    for (i, music) in [(0, true), (1, false)] {
        let (key, ours, value) = if music { ("OptionSld0", n_("Background music volume"), audio.music_volume) } else { ("OptionSld1", n_("Sound effects volume"), audio.sfx_volume) };
        if let Some(v) = option_slider(row(i), &own("Options", key, ours), value) {
            let steps = ((v - value) / super::audio::VOLUME_STEP).round() as i32;
            if music {
                audio.music_muted = false;
                audio.step_music(steps);
            } else {
                audio.sfx_muted = false;
                audio.step_sfx(steps);
            }
        }
    }
    // The map's edge scroll (`ScrollSpeed`, world_view) and the hero's step (`WalkSpeed`).
    if let Some(v) = option_slider(row(2), &own("Options", "OptionSld2", n_("Map scroll speed")), scroll_speed(audio) / 100.0) {
        audio.scroll_speed = Some((v * 100.0).round());
    }
    if let Some(v) = option_slider(row(3), &own("Options", "OptionSld3", n_("Hero's walking speed")), walk_speed(audio) / 100.0) {
        audio.walk_speed = Some((v * 100.0).round());
    }
    // The Community's "Скорость анимаций в битве" (`AnimationSpeed`): it caps a strike's
    // slide at (100 − S)·5 ms and its effect at (100 − S)·3 ms (0x4afd45, 0x4afe7c).
    if let Some(v) = option_slider(row(4), &own("Options", "OptionSld4", n_("Battle animation speed")), anim_speed(audio).unwrap_or(0.0) / 99.0) {
        audio.anim_speed = Some((v * 99.0).round());
    }
    // The eight checkboxes (`Option4`…`Option11`), a gap before the three of battle and
    // difficulty.
    let cx = inner.x + 14.0 * k;
    let cy = |i: usize| row(5).y + 4.0 * k + i as f32 * 27.0 * k + if i >= 5 { 11.0 * k } else { 0.0 };
    let checks: [(&str, &'static str, bool, Color); 8] = [
        ("Option4", n_("Do not load music and sounds (needs a restart)"), no_sound(audio), CHECK_ORANGE),
        ("Option5", n_("Show FPS"), audio.show_fps, CHECK_INK),
        ("Option6", n_("Turn off hints"), hints_off(audio), CHECK_INK),
        ("Option7", n_("Automatically pursue the chosen army"), pursue(audio), CHECK_INK),
        ("Option8", n_("Automatically save the current game"), autosave(audio), CHECK_INK),
        ("Option9", n_("Improved enemy AI in battle"), expert_ai(audio), CHECK_INK),
        ("Option10", n_("Impossible difficulty (needs a restart)"), impossible(audio), CHECK_INK),
        ("Option11", n_("Wide front row in battle (for new games)"), wide_row(audio, install_wide), CHECK_INK),
    ];
    for (i, (key, ours, on, ink)) in checks.into_iter().enumerate() {
        if option_check(cx, cy(i), &own("Options", key, ours), on, ink) {
            match i {
                0 => audio.no_sound = Some(!on),
                1 => audio.show_fps = !on,
                2 => audio.hints_off = Some(!on),
                3 => audio.pursue = Some(!on),
                4 => audio.autosave = Some(!on),
                5 => toggle_expert_ai(audio),
                6 => audio.impossible = Some(!on),
                _ => audio.wide_row = Some(!on),
            }
        }
    }
    // Razdor's own settings, in a window of their own.
    let xr = Rect::new(inner.x + 14.0 * k, inner.y + inner.h - 42.0 * k, 120.0 * k, 32.0 * k);
    if row_button(xr, tr("Advanced…")) {
        ADVANCED.with(|a| a.set(true));
        return false;
    }
    // «Принять» keeps what was changed, «Отмена» (the close box and Esc too) puts it back.
    let cancel = Rect::new(inner.x + inner.w - 14.0 * k - 89.0 * k, inner.y + inner.h - 42.0 * k, 89.0 * k, 32.0 * k);
    let apply = Rect::new(cancel.x - 8.0 * k - 89.0 * k, cancel.y, 89.0 * k, 32.0 * k);
    let (apply_label, cancel_label) = (own("Buttons", "Apply", n_("Apply")), own("Buttons", "Cancel", n_("Cancel")));
    // Both act when the button is let go over it (PR #5's buttons).
    let apply_pressed = settings_button(apply, &apply_label);
    let cancel_pressed = settings_button(cancel, &cancel_label);
    if apply_pressed || key(KeyCode::Enter) {
        OPENED_WITH.with(|o| o.set(None));
        return true;
    }
    if cancel_pressed || closed || key(KeyCode::Escape) {
        if let Some(before) = OPENED_WITH.with(|o| o.take()) {
            *audio = before;
        }
        return true;
    }
    false
}
