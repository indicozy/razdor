//! The original's main menu (the 18:08 frame of the gameplay video): the burning ruins
//! (`Windows/Castle.lit`) over the whole screen, the silver "Времена Раздора" logo
//! (`Title_RUS.ugs`) with flames rising off its letters (`TitleFlame_RUS.lit`), and five oval
//! buttons (`mb2.ugs`, the last one pointed) whose marble middles swirl in violet
//! (`MenuFlame.lit`) under the words of `SMText_RUS.lit`; the button under the mouse turns
//! gold. Razdor's own extras, the map editor, the custom battle and the interface language,
//! sit small in the bottom corners. Layout numbers are pixels of the 960×720 video, scaled by [`chrome::k`].

use razdor::local;
use crate::ui::input::{get_keys_pressed, is_mouse_button_down};
use macroquad::prelude::*;

use razdor::i18n::{self, n_, tr, Lang};

use super::audio::{cue, Cue};
use super::chrome::{self, Fx};
use super::keymap;
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

/// A phone held upright (`upright_dp`): the buttons are this many times the landscape size,
/// in one column under the logo, and the links are chips at the bottom (the owner's choice,
/// 2026-10-26, mockup "A: ×1.6").
const PORTRAIT_SCALE: f32 = 1.6;
/// The smallest touch target of a phone's link, in dp.
const TOUCH_DP: f32 = 48.0;

// Final title-flame placement from the temporary development tuner.
const LOGO_FLAME_X: f32 = -16.0;
const LOGO_FLAME_Y: f32 = 12.0;
const LOGO_FLAME_WIDTH: f32 = 0.95;
const LOGO_FLAME_HEIGHT: f32 = 1.52;
const LOGO_FLAME_GLOW: f32 = 0.85;

fn video(p: Vec2) -> Vec2 {
    let k = chrome::k();
    vec2(screen_width() / 2.0 + p.x * k, chrome::canvas_top() + p.y * k)
}

/// The burning ruins over the whole screen, a little warmer where the fire glows. Not panned:
/// the screen behind a panned title stays covered (`ui::pan`).
fn background() {
    super::pan::unpanned(|| match chrome::win("Castle") {
        Some(t) => {
            // A phone in portrait covers the screen with the picture's centre (keeping its
            // aspect), not stretched; other screens stretch it as before.
            let (sw, sh) = (screen_width(), screen_height());
            let screen = Rect::new(0.0, 0.0, sw, sh);
            match chrome::cover_source((t.width(), t.height()), (sw, sh)) {
                Some(src) => chrome::tex_src(&t, src, screen, WHITE),
                None => chrome::tex(&t, screen, WHITE),
            }
        }
        None => clear_background(Color::from_rgba(60, 30, 12, 255)),
    });
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
    let y = chrome::canvas_top() + 50.0 * k;
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
        let t = crate::ui::clock::now();
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

local! {
    /// The main menu item under the pointer at the last frame.
    static HOVERED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// An oval button: the violet swirl, the frame (gold under the mouse) and its word. Returns
/// true when clicked.
fn oval(i: usize, label: &str, hot: Option<usize>) -> bool {
    let upright = upright_dp().is_some();
    let k = chrome::k() * if upright { PORTRAIT_SCALE } else { 1.0 };
    let r = oval_rect(i);
    let c = if upright { r.center() } else { video(button_centre(i)) };
    let size = OVAL * k;
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
            crate::ui::clock::now() * 0.8 + i as f64 * 0.10,
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
            crate::ui::clock::now() + i as f64 * 0.37,
            12.0,
            16,
            flame_frame,
            inner,
            tint,
        );
    }
    word(i, label, c, hover, k);
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
fn word(i: usize, label: &str, c: Vec2, hover: bool, k: f32) {
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

/// The Razdor players' Discord server, as the site links it (where the community shares the game).
pub(super) const DISCORD_INVITE: &str = "https://discord.gg/cdTtYz5dH";
/// The illustrated install guide on the website (the pages of the gh-pages branch of the repo),
/// in Russian and in English: the not-found block's «How to install» picks by the interface language.
const INSTALL_GUIDE_RU: &str = "https://indicozy.github.io/razdor/install.html";
const INSTALL_GUIDE_EN: &str = "https://indicozy.github.io/razdor/install-en.html";

/// The install guide's page in the current interface language.
pub fn install_guide() -> &'static str {
    if razdor::i18n::lang() == razdor::i18n::Lang::Ru { INSTALL_GUIDE_RU } else { INSTALL_GUIDE_EN }
}

/// Opens `url` in the player's browser: `xdg-open`, `explorer` or `open` on desktop, an intent
/// on Android (`ui::android::open_url`), the app's `openURL` on iOS (`ui::mobile::open_url`).
pub fn open_link(url: &str) {
    #[cfg(target_os = "android")]
    super::android::open_url(url);
    #[cfg(target_os = "ios")]
    super::mobile::open_url(url);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    razdor::update::open_in_browser(url);
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
    let y = upright_menu().map_or(screen_height() - 4.0 * k, |m| m.foot_y);
    let ink = Color::new(0.92, 0.92, 0.92, 1.0);
    if let Some(v) = chrome::ui_text("SpecialTexts", "VerInfo") {
        chrome::shadow_text(&v, 5.0 * k, y, size, ink);
    }
    if let Some(c) = chrome::ui_text("SpecialTexts", "CopyrightsMessage") {
        chrome::shadow_text(&c, 264.0 * k, y, size, ink);
    }
}

/// The dp of a phone held upright (its stacked menu), else None.
fn upright_dp() -> Option<f32> {
    super::display::dp_px().filter(|_| super::window_layout::portrait())
}

/// The five buttons in the landscape layout: the original's column (video geometry, 1:1 at
/// `k`, centred on a `sw` × `sh` screen; neighbouring ovals overlap by 20 px).
fn landscape_buttons(sw: f32, sh: f32, k: f32) -> [Rect; 5] {
    let top = chrome::centred_top(sw, sh, 720.0 * k);
    std::array::from_fn(|i| {
        let c = vec2(sw / 2.0, top + button_centre(i).y * k);
        let size = OVAL * k;
        Rect::new(c.x - size.x / 2.0, c.y - size.y / 2.0, size.x, size.y)
    })
}

/// The menu held upright: the buttons are a column of [`PORTRAIT_SCALE`] × the landscape size
/// starting under the logo, and the links are a row of chips at the bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PortraitMenu {
    buttons: [Rect; 5],
    /// The chips' row: its top and height (a touch target of at least [`TOUCH_DP`]).
    chip_top: f32,
    chip_h: f32,
    /// The baseline of the foot lines (the version and copyright), just above the chips.
    foot_y: f32,
    /// The top of the logo: the space above it is free for the not-found block.
    logo_top: f32,
}

impl PortraitMenu {
    /// The baseline of the version line (above the foot lines).
    fn version_y(&self, k: f32) -> f32 {
        self.foot_y - 24.0 * k
    }

    /// The baseline of an installed update's notice (above the version line).
    fn notice_y(&self, k: f32) -> f32 {
        self.foot_y - 48.0 * k
    }

    /// The bottom of the not-found block: above the logo, where the screen is empty (the column
    /// under the logo would cover a block below it).
    fn block_bottom(&self, k: f32) -> f32 {
        self.logo_top - 16.0 * k
    }
}

/// The portrait menu on a `sw` × `sh` screen at interface scale `k`, a phone of `dp` screen px
/// per dp.
fn portrait_menu(sw: f32, sh: f32, k: f32, dp: f32) -> PortraitMenu {
    let top = chrome::centred_top(sw, sh, 720.0 * k);
    // The logo (`logo`): 50 px under the canvas top, 840 px wide, 128/896 of that high.
    let logo_top = top + 50.0 * k;
    let logo_bottom = logo_top + 840.0 * 128.0 / 896.0 * k;
    let size = OVAL * (PORTRAIT_SCALE * k);
    let chip_h = (TOUCH_DP * dp).max(24.0 * k);
    let chip_top = sh - 16.0 * k - chip_h;
    let foot_y = chip_top - 12.0 * k;
    // The column starts 24 px under the logo and ends above the lines, its gaps shared out
    // between the five buttons (at least 6 px, at most 20 px).
    let first = logo_bottom + 24.0 * k;
    let column_end = foot_y - 70.0 * k;
    let gap = ((column_end - first - 5.0 * size.y) / 4.0).clamp(6.0 * k, 20.0 * k);
    let x = sw / 2.0 - size.x / 2.0;
    let buttons = std::array::from_fn(|i| Rect::new(x, first + i as f32 * (size.y + gap), size.x, size.y));
    PortraitMenu { buttons, chip_top, chip_h, foot_y, logo_top }
}

/// The current menu's portrait geometry, when the phone is held upright.
fn upright_menu() -> Option<PortraitMenu> {
    let dp = upright_dp()?;
    Some(portrait_menu(screen_width(), screen_height(), chrome::k(), dp))
}

/// The screen rectangle of oval button `i`.
fn oval_rect(i: usize) -> Rect {
    match upright_menu() {
        Some(m) => m.buttons[i],
        None => landscape_buttons(screen_width(), screen_height(), chrome::k())[i],
    }
}

/// The left, width and bottom of the phones' not-found block (`screens::menu_not_found`): beside
/// the buttons' column when the menu is landscape, under the column (full width, above the
/// lines) when held upright.
pub fn not_found_box(k: f32) -> (f32, f32, f32) {
    match upright_menu() {
        Some(m) => (16.0 * k, screen_width() - 32.0 * k, m.block_bottom(k)),
        None => (16.0 * k, (left_edge() - 32.0 * k).max(120.0 * k), screen_height() - 60.0 * k),
    }
}

/// The baseline of the title screen's update notice (`update_view::title_line`).
pub fn notice_y(k: f32) -> f32 {
    match upright_menu() {
        Some(m) => m.notice_y(k),
        None => screen_height() - 44.0 * k,
    }
}

/// The one button under the pointer: neighbouring ovals overlap by 20 px, and the original's
/// hit test (0x4743c8) reports a single widget, the first in order. Without this the pointer in
/// an overlap counted as on two buttons, which took the light from each other every frame and
/// rang the bell each time.
fn hot_button(pointer: Vec2, n: usize) -> Option<usize> {
    (0..n).find(|&i| oval_rect(i).contains(pointer))
}

/// The screen x of the leftmost button (its oval's left edge): the phones' not-found block sits
/// to the left of it (on the desktop too, where the not-found block sits in the same column).
pub fn left_edge() -> f32 {
    (0..BUTTONS.len()).map(|i| oval_rect(i).x).fold(f32::MAX, f32::min)
}

/// Draws the menu; returns what was picked. `no_install`: the install was not found, so the
/// phones' not-found block is drawn beside the buttons.
pub fn frame(no_install: bool) -> Option<Pick> {
    backdrop();
    let mut pick = None;
    let hot = if input_blocked() { None } else { hot_button(crate::ui::widgets::pointer().into(), BUTTONS.len()) };
    for (i, (p, label)) in BUTTONS.iter().enumerate() {
        if oval(i, tr(label), hot) {
            pick = Some(*p);
        }
    }
    super::screens::menu_not_found(no_install);
    let k = chrome::k();
    // The version line sits above the links: the landscape corner's, or the chips' row when
    // held upright.
    let version_y = match upright_menu() {
        Some(m) => {
            if let Some(p) = chip_links(&m, k) {
                pick = Some(p);
            }
            m.version_y(k)
        }
        None => {
            if let Some(p) = corner_links(k) {
                pick = Some(p);
            }
            screen_height() - 22.0 * k - 26.0 * k
        }
    };
    // This program's version, small in the corner above the links.
    let version = format!("v{}", super::update_view::shown_version(env!("CARGO_PKG_VERSION"), super::clock::is_snapshot()));
    let vw = measure(&version, 12.0 * k).width;
    chrome::shadow_text(&version, screen_width() - 16.0 * k - vw, version_y, 12.0 * k, Color::new(0.95, 0.88, 0.72, 0.6));
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

/// The links in the landscape corners, over the original's version and copyright lines: the map
/// editor, the custom battle, the Discord and the language. Returns what was picked.
fn corner_links(k: f32) -> Option<Pick> {
    let mut pick = None;
    let y = screen_height() - 22.0 * k;
    let editor = tr("Map editor");
    if link(editor, 16.0 * k, y, false) {
        pick = Some(Pick::Editor);
    }
    let x = 16.0 * k + measure(editor, 18.0 * k).width + 28.0 * k;
    if link(tr("Custom battle"), x, y, false) {
        pick = Some(Pick::Custom);
    }
    // The players' Discord (the owner's wish): the invite of the site, in the browser; on phones
    // too (`open_link` hands it to the system's browser there).
    let x = x + measure(tr("Custom battle"), 18.0 * k).width + 28.0 * k;
    if link("Discord", x, y, false) {
        open_link(DISCORD_INVITE);
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
    pick
}

/// The links held upright: a row of chips at the bottom, each a dark rounded plate and a touch
/// target of at least [`TOUCH_DP`]. The language chip opens the list above the row.
fn chip_links(m: &PortraitMenu, k: f32) -> Option<Pick> {
    let mut pick = None;
    let lang = super::language::current_label();
    let labels: [&str; 4] = [tr("Map editor"), tr("Custom battle"), "Discord", &lang];
    let size = 22.0 * k;
    let pad = 18.0 * k;
    let gap = 12.0 * k;
    let widths: Vec<f32> = labels.iter().map(|l| measure(l, size).width + 2.0 * pad).collect();
    let total = widths.iter().sum::<f32>() + gap * (labels.len() - 1) as f32;
    let mut x = (screen_width() - total) / 2.0;
    let (mut lang_chip, mut lang_pressed) = (Rect::new(0.0, 0.0, 0.0, 0.0), false);
    for (j, label) in labels.iter().enumerate() {
        let r = Rect::new(x, m.chip_top, widths[j], m.chip_h);
        let (hover, down, pressed) = super::widgets::button_state(r, true);
        plate(r, 14.0 * k, Color::new(0.08, 0.05, 0.03, 1.0));
        let color = if down || hover { chrome::GOLD } else { Color::new(0.95, 0.88, 0.72, 1.0) };
        let tw = measure(label, size).width;
        chrome::shadow_text(label, r.x + (r.w - tw) / 2.0, r.y + r.h / 2.0 + size * 0.35, size, color);
        if pressed {
            cue(Cue::Button);
        }
        match j {
            0 if pressed => pick = Some(Pick::Editor),
            1 if pressed => pick = Some(Pick::Custom),
            2 if pressed => open_link(DISCORD_INVITE),
            3 => (lang_chip, lang_pressed) = (r, pressed),
            _ => {}
        }
        x += widths[j] + gap;
    }
    if lang_pressed || super::snapshot::open_language_list() {
        super::language::open_list(lang_chip, k);
    }
    if super::widgets::mouse_in(lang_chip.x, lang_chip.y, lang_chip.w, lang_chip.h) {
        super::widgets::tooltip(&[(super::language::hint().to_string(), super::widgets::INK)]);
    }
    pick
}

/// A rounded rectangle of radius `rad` (at most half its sides).
fn plate(r: Rect, rad: f32, c: Color) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    draw_rectangle(r.x + rad, r.y, r.w - 2.0 * rad, r.h, c);
    draw_rectangle(r.x, r.y + rad, r.w, r.h - 2.0 * rad, c);
    for (x, y) in [(r.x + rad, r.y + rad), (r.x + r.w - rad, r.y + rad), (r.x + rad, r.y + r.h - rad), (r.x + r.w - rad, r.y + r.h - rad)] {
        draw_circle(x, y, rad, c);
    }
}

/// The credits picture of the install: its dark outline (`Credits_Alpha` as the alpha of
/// black) and its letters (`Credits_Color`, drawn additively).
#[derive(Clone)]
struct Credits {
    shadow: Texture2D,
    letters: Texture2D,
}

local! {
    /// The credits, made once.
    static CREDITS: std::cell::OnceCell<Option<Credits>> = const { std::cell::OnceCell::new() };
    /// The credits window the player scrolled (when it opened, how far from the timed scroll
    /// in the picture's pixels).
    static CREDITS_NUDGE: std::cell::Cell<(f64, f32)> = const { std::cell::Cell::new((f64::NAN, 0.0)) };
}

/// The players who helped Razdor with bug reports and contributions, by their Discord names
/// (the owner's wish, 2026-10-22): those with a fixed report (the owner, 2026-10-24), most fixed first; a display name in letters the fonts lack
/// (Cyrillic Extended-B, Greek, IPA) is given as the Discord tag instead.
const THANKS: [&str; 10] = ["Нлтчк", "ivantykhonchuk", "Теневой Лорд", "Eisenwald", "! TuttiFrutti", "turgrin", "9ibatnk", "OBANAI77", "nik0N", "Hristos"];
/// The names column beside the credits window (the owner's choice, 2026-10-23): its margin
/// from the screen's edges and from the window, in px at the UI scale (k).
const COLUMN_MARGIN: f32 = 16.0;
/// The narrowest side the column takes (px at k); narrower, the other side, else under the window.
const SIDE_MIN_W: f32 = 120.0;
/// Line spacing of the column, times the line's type size.
const COLUMN_LEAD: f32 = 1.3;
/// The smallest scale the column is drawn at before it takes another column under the window.
const COLUMN_MIN_SCALE: f32 = 0.6;

/// One line of the names column: its text, type size (px at k), colour, and the gap above it.
struct CreditLine {
    text: String,
    size: f32,
    color: Color,
    gap: f32,
}

impl CreditLine {
    /// Its row's height at scale `s` (k included in `s`): the line and the gap above.
    fn row(&self, s: f32) -> f32 {
        (self.size * COLUMN_LEAD + self.gap) * s
    }

    fn width(&self, s: f32) -> f32 {
        measure(&self.text, self.size * s).width
    }
}

/// Razdor's modder, the rest of the credits' names: the role in gold, the name in white.
fn thanks_lines() -> Vec<CreditLine> {
    let line = |text: &str, size: f32, color: Color, gap: f32| CreditLine { text: text.to_string(), size, color, gap };
    let mut lines = vec![
        line(tr("RAZDOR modder"), 20.0, chrome::GOLD, 0.0),
        line("indicozy", 22.0, WHITE, 0.0),
        line(tr("Thanks to the players"), 20.0, chrome::GOLD, 14.0),
    ];
    lines.extend(THANKS.iter().map(|name| line(name, 18.0, WHITE, 0.0)));
    lines
}

/// The lines in `n` columns, in order, each line on its own row.
fn columns(lines: &[CreditLine], n: usize) -> Vec<&[CreditLine]> {
    lines.chunks(lines.len().div_ceil(n.max(1))).collect()
}

/// The columns' height and width at scale `s`, with the gaps between them.
fn columns_size(lines: &[CreditLine], n: usize, k: f32) -> (f32, f32) {
    let cols = columns(lines, n);
    let height = cols.iter().map(|c| c.iter().map(|l| l.row(k)).sum::<f32>()).fold(0.0, f32::max);
    let widths: f32 = cols.iter().map(|c| c.iter().map(|l| l.width(k)).fold(0.0, f32::max)).sum();
    (height, widths + COLUMN_MARGIN * k * (cols.len() as f32 - 1.0))
}

/// The scale (up to 1) at which `n` columns of `lines` fit `area`.
fn columns_fit(lines: &[CreditLine], n: usize, area: Rect, k: f32) -> f32 {
    let (height, width) = columns_size(lines, n, k);
    (area.h / height).min(area.w / width).min(1.0)
}

/// Draws the lines in `n` columns centred in `area`, at the largest scale that fits it.
fn draw_columns(lines: &[CreditLine], n: usize, area: Rect, k: f32) {
    let s = columns_fit(lines, n, area, k).max(0.3) * k;
    let (height, width) = columns_size(lines, n, s);
    let mut x = area.center().x - width / 2.0;
    let top = (area.center().y - height / 2.0).max(area.y);
    for col in columns(lines, n) {
        let cw = col.iter().map(|l| l.width(s)).fold(0.0, f32::max);
        let mut y = top;
        for l in col {
            let size = l.size * s;
            let lw = measure(&l.text, size).width;
            chrome::shadow_text(&l.text, x + (cw - lw) / 2.0, y + l.gap * s, size, l.color);
            y += l.row(s);
        }
        x += cw + COLUMN_MARGIN * s;
    }
}

/// The names beside the credits window, outside it and never in the roll: on the right of the
/// window, else on its left, else under it (one line per name, smaller, in as many columns as
/// it takes). Always drawn.
fn thanks_column(page: Rect) {
    let k = chrome::k();
    let m = COLUMN_MARGIN * k;
    let lines = thanks_lines();
    let (sw, sh) = (screen_width(), screen_height());
    let right = sw - (page.x + page.w) - 2.0 * m;
    let left = page.x - 2.0 * m;
    let min_w = SIDE_MIN_W * k;
    // Centred on the window's middle, as far as the screen's margins allow.
    let cy = page.center().y;
    let half = (cy - m).min(sh - m - cy).max(0.0);
    let side = |x: f32, w: f32| draw_columns(&lines, 1, Rect::new(x, cy - half, w, 2.0 * half), k);
    if right >= min_w {
        side(page.x + page.w + m, right);
    } else if left >= min_w {
        side(m, left);
    } else {
        let strip = Rect::new(m, page.y + page.h + m, sw - 2.0 * m, sh - page.y - page.h - 2.0 * m);
        let n = (1..=lines.len()).find(|&n| columns_fit(&lines, n, strip, k) >= COLUMN_MIN_SCALE).unwrap_or(lines.len());
        draw_columns(&lines, n, strip, k);
    }
}

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
            Some(Credits { shadow: make(&shadow), letters: make(&color.rgba) })
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
    let r = Rect::new((screen_width() - w) / 2.0, (chrome::canvas_top() + 190.0 * k).min(screen_height() - h), w, h);
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
        // and it stays on its last view once the end is in sight. The roll is the original's
        // alone; Razdor's names stand beside the window (`thanks_column`), the wheel and the
        // arrow keys scroll by hand, and the scroll goes on from there.
        let px = k * 0.9375;
        let w = c.letters.width() * px;
        let total = c.letters.height();
        let most = (total - inner.h / px).max(0.0);
        let (opened, mut nudge) = CREDITS_NUDGE.with(|n| n.get());
        if opened != started {
            nudge = 0.0;
        }
        let over = inner.contains(crate::ui::widgets::pointer().into());
        let step = 48.0;
        swipe_rows(inner, step);
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
        let timed = (crate::ui::clock::now() - started) as f32 * 35.0;
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
        picture(0.0, c.letters.height(), 0.0);
    }
    thanks_column(page);
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

/// The settings a game follows that its saves do not keep (the battle AI's strength, the
/// walk, the options of passing armies): every frame, and at once on a game just restored,
/// before the battle it may start (a pending foe, a battle's «Рестарт»). Before #19 a
/// restored battle began with the defaults, the original's AI on its normal level: with
/// «Максимум» chosen, its «Рестарт» was played by another AI than the battle itself.
pub fn follow_settings(audio: &super::audio::Settings, g: &mut razdor::rules::game::Game) {
    g.improved_ai = expert_ai(audio);
    g.max_ai = ai_strength(audio) == AiStrength::Max;
    g.pursue = pursue(audio);
    g.walk_pace = walk_pace(audio);
    g.friends_let_pass = audio.friends_let_pass;
    g.avoid_armies = audio.avoid_armies;
    g.allow_unit_benching = audio.allow_unit_benching;
    g.pass_on_bridges = audio.pass_on_bridges;
    g.battle_marks = audio.battle_marks;
    g.army_list = audio.army_list;
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
    super::touchpad::set_setting(audio.touchpad);
}

/// "Настройки звука, графики и геймплея", with what Razdor lets the player change: the
/// music and sound volumes (saved at once) and the interface language. Returns true when
/// closed.
pub fn options(audio: &mut super::audio::Settings, install_wide: bool) -> bool {
    backdrop();
    options_window(audio, install_wide, false)
}

local! {
    /// The advanced settings window is open over the settings.
    static ADVANCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The settings as the window found them: «Отмена» puts them back (the original's
    /// Cancel drops what was changed, its Apply keeps it).
    static OPENED_WITH: std::cell::RefCell<Option<super::audio::Settings>> = const { std::cell::RefCell::new(None) };
}

local! {
    /// The keys window («Клавиши») is open over the advanced settings.
    static KEYS_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Opens the advanced settings over the settings window (a snapshot scene).
pub fn open_advanced() {
    ADVANCED.with(|a| a.set(true));
}

/// Opens the keys window over the advanced settings (a snapshot scene).
pub fn open_keys() {
    open_advanced();
    KEYS_OPEN.with(|o| o.set(true));
}

/// The orientation row's value: Auto (None), Landscape or Portrait.
fn orientation_label(orientation: Option<super::audio::Orientation>) -> &'static str {
    match orientation {
        None => tr("Auto"),
        Some(super::audio::Orientation::Landscape) => tr("Landscape"),
        Some(super::audio::Orientation::Portrait) => tr("Portrait"),
    }
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
    // Phones have their own update window (Android installs through the system, iOS opens the
    // release's page; `razdor::update::opens_page`): the row is not drawn there and the zoom
    // row takes its place.
    let y = if razdor::platform::MOBILE {
        y
    } else {
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
        y + 40.0 * k
    };
    // The map's own zoom (Razdor's): the wheel steps from it, 0 comes back to it; locked, the
    // wheel and +/- leave it alone.
    let y = y + 100.0 * k;
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
    // The army's arrangement outside battle (Razdor's, the owner's decision after players'
    // reports): clicks only (the original's, 0x4c346c), dragging only, or both (`unit_drag::Controls`).
    let y = y + 136.0 * k;
    chrome::shadow_text(tr("Army arrangement"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let ar = Rect::new(br.x, y, br.w, br.h);
    let label = match audio.formation_controls {
        super::unit_drag::Controls::Clicks => tr("Clicks"),
        super::unit_drag::Controls::Drag => tr("Dragging"),
        super::unit_drag::Controls::Both => tr("Both"),
    };
    if row_button(ar, label) {
        audio.formation_controls = audio.formation_controls.next();
    }
    let hint = tr("Clicks: the first click picks a unit or an item, the second places it. Dragging: a click only selects, and units and items are moved by dragging. Both: clicks and dragging at once.");
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
        *y += 44.0 * k;
        row_button(r, value)
    };
    // Windowed, borderless or full screen, set at once (`display::follow_settings`). A phone
    // has two: windowed (the bars shown) and full screen (the bars hidden).
    let phone = super::display::dp_px().is_some();
    let screen_label = if phone { tr("Screen") } else { tr("Screen (Alt+Enter)") };
    if row(screen_label, audio.display.for_device(phone).label(), &mut y) {
        audio.display = audio.display.next_on(phone);
    }
    // The interface scale: Auto (the largest that fits) or a fixed one, kept to the fit (a
    // scale above it is kept to the fit: players got stuck with the settings off-screen in 1.0.0).
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
    if row(tr("Unit battle participation"), if audio.allow_unit_benching { tr("On") } else { tr("Off") }, &mut y) {
        audio.allow_unit_benching = !audio.allow_unit_benching;
    }
    // Razdor's option: the route closes every army's cell (off: the original's route).
    if row(tr("Route around armies"), if audio.avoid_armies { tr("On") } else { tr("Off") }, &mut y) {
        audio.avoid_armies = !audio.avoid_armies;
    }
    // Razdor's options: a battle between armies on the map shows a mark and a notice (on by
    // default); the armies the player sees are listed under the minimap (off by default).
    if row(tr("Battles on the map"), if audio.battle_marks { tr("On") } else { tr("Off") }, &mut y) {
        audio.battle_marks = !audio.battle_marks;
    }
    if row(tr("Minimap army list"), if audio.army_list { tr("On") } else { tr("Off") }, &mut y) {
        audio.army_list = !audio.army_list;
    }
    // The keys window (Razdor's, the owner's request): every action's key, rebound by a click.
    if row(tr("Keyboard shortcuts"), tr("Keys…"), &mut y) {
        KEYS_OPEN.with(|o| o.set(true));
    }
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
/// «Клавиши» (Razdor's keys window): every action with its key, in two columns. A click on
/// a key waits for the new one (`keymap::listen`); Esc or a click elsewhere cancels, and a
/// key another action has goes to that action (`keymap::set`). «По умолчанию» puts every key
/// back; «Ок» (or Esc, when not waiting) closes it. Returns true when closed.
fn keys_window(audio: &mut super::audio::Settings) -> bool {
    let k = chrome::k();
    let (w, h) = (760.0 * k, 420.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, ((screen_height() - h) / 2.0).max(0.0), w, h);
    let (inner, closed) = chrome::window(r, tr("Keys"), chrome::Skin::Marble, true);
    let waiting = keymap::listening();
    let was_waiting = waiting.is_some();
    let col_w = (inner.w - 60.0 * k) / 2.0;
    let mut started = false;
    for (i, a) in keymap::ALL.into_iter().enumerate() {
        let x = inner.x + 20.0 * k + (i / 9) as f32 * (col_w + 20.0 * k);
        let y = inner.y + 16.0 * k + (i % 9) as f32 * 32.0 * k;
        text_fit(tr(a.label()), x, y + 18.0 * k, col_w - 170.0 * k, 14.0 * k, chrome::CREAM);
        let br = Rect::new(x + col_w - 150.0 * k, y, 150.0 * k, 28.0 * k);
        let shown = if waiting == Some(a) { tr("press a key…").to_string() } else { keymap::display(keymap::key(a)) };
        if row_button(br, &shown) && waiting != Some(a) {
            keymap::listen(Some(a));
            started = true;
        }
    }
    if let Some(a) = waiting {
        // The key pressed is the new binding: no shortcut fires on it (`hotkeys`, `mod`).
        if !started {
            if key(KeyCode::Escape) || clicked() {
                keymap::listen(None);
            } else if let Some(pressed) = get_keys_pressed().into_iter().find(|&k| keymap::bindable(k)) {
                keymap::set(a, pressed);
                keymap::listen(None);
                audio.keys = keymap::overrides();
            }
            swallow_input();
        }
    }
    let hint = tr("Click a key, then press the new one. Esc cancels; a key another action has goes to that action.");
    text_fit(hint, inner.x + 20.0 * k, inner.y + 318.0 * k, inner.w - 40.0 * k, 12.0 * k, WHITE);
    let defaults = Rect::new(inner.x + 20.0 * k, inner.y + inner.h - 44.0 * k, 150.0 * k, 30.0 * k);
    let ok = Rect::new(inner.x + inner.w - 120.0 * k, inner.y + inner.h - 44.0 * k, 96.0 * k, 30.0 * k);
    if settings_button(defaults, tr("Defaults")) {
        keymap::reset();
        keymap::listen(None);
        audio.keys = keymap::overrides();
    }
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let ok_pressed = settings_button(ok, &ok_label);
    closed || ok_pressed || (!was_waiting && key(KeyCode::Escape))
}

pub fn options_window(audio: &mut super::audio::Settings, install_wide: bool, over_map: bool) -> bool {
    if ADVANCED.with(|a| a.get()) {
        // The keys window stands over the advanced one, which takes no input meanwhile.
        if KEYS_OPEN.with(|o| o.get()) {
            if keys_window(audio) {
                KEYS_OPEN.with(|o| o.set(false));
            }
            return false;
        }
        if advanced_window(audio, over_map) {
            ADVANCED.with(|a| a.set(false));
            // What the advanced window set is kept, whatever the settings window ends with.
            OPENED_WITH.with(|o| *o.borrow_mut() = Some(audio.clone()));
        }
        return false;
    }
    if OPENED_WITH.with(|o| o.borrow().is_none()) {
        OPENED_WITH.with(|o| *o.borrow_mut() = Some(audio.clone()));
    }
    let title = own("Options", "Title", n_("Sound, graphics and gameplay settings"));
    // On a phone the touchpad mode's row (`ui::touchpad`) takes 28 more px under the checkboxes.
    let touch_row = super::touchpad::MOBILE;
    // The orientation row is Android's: iOS keeps the system's rotation lock for now, so it has no row there.
    let orientation_row = cfg!(target_os = "android");
    // Razdor's two rows at the end (the text size and the large windows, the owner's request:
    // phones' text too small) take 76 px more: the frame line under the checkboxes sits 82 px above
    // the bottom edge, the buttons 68 above.
    // The import row («Обновить игру из архива…», `ui::game_import`) takes 30 px more under the large
    // message windows' row.
    let h = (if orientation_row { 556.0 } else if touch_row { 528.0 } else { 500.0 }) + 76.0 + 30.0;
    let (inner, closed) = if over_map { map_window(&title, 594.0, h) } else { window(&title, 594.0, h) };
    let k = chrome::k();
    let outer = Rect::new(inner.x - 2.0 * k, inner.y - (26.0 * k).round() - 1.0, 594.0 * k, h * k);
    // A window with 0x48d710's inner frames (the options window is c2953a, 634×556, 4d2f48).
    chrome::window_inner_frames(outer, chrome::opx());
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
    // Razdor's touchpad mode (phones and tablets only), off unless the player turned it on.
    if touch_row {
        let on = audio.touchpad.unwrap_or(false);
        if option_check(cx, cy(8), &own("Options", "RazdorTouchpad", n_("Touchpad mode (the screen moves a cursor)")), on, CHECK_INK) {
            audio.touchpad = Some(!on);
        }
    }
    // Razdor's screen orientation (Android only): Auto (the system's, the device's turn), then
    // landscape or portrait, cycled by the button. `ui::android::follow_orientation` applies it.
    if orientation_row {
        let y = cy(9) - 4.0 * k;
        chrome::shadow_text(tr("Screen orientation"), cx, y + 18.0 * k, 14.0 * k, chrome::CREAM);
        let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, y, 170.0 * k, 28.0 * k);
        if row_button(r, orientation_label(audio.orientation)) {
            audio.orientation = match audio.orientation {
                None => Some(super::audio::Orientation::Landscape),
                Some(super::audio::Orientation::Landscape) => Some(super::audio::Orientation::Portrait),
                Some(super::audio::Orientation::Portrait) => None,
            };
        }
    }
    // Razdor's text size and large pop-up message windows (the owner's request: phones' text
    // too small), one row each under the others: «Размер текста» cycles, «Крупные окна
    // сообщений» toggles. The step is 30 px, the button 170 px wide at the right, as the
    // orientation row's.
    let last_y = if orientation_row { cy(9) - 4.0 * k } else if touch_row { cy(8) } else { cy(7) };
    let text_y = last_y + 30.0 * k;
    chrome::shadow_text(tr("Text size"), cx, text_y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, text_y, 170.0 * k, 28.0 * k);
    if row_button(r, super::display::text_size_label(audio.text_size)) {
        audio.text_size = super::display::next_text_size(audio.text_size);
    }
    let large_y = text_y + 30.0 * k;
    chrome::shadow_text(tr("Large message windows"), cx, large_y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, large_y, 170.0 * k, 28.0 * k);
    if row_button(r, if audio.large_windows { tr("On") } else { tr("Off") }) {
        audio.large_windows = !audio.large_windows;
    }
    // «Обновить игру из архива…» (Razdor's, a row of its own under the large windows): the
    // button, or why it is not offered (`ui::game_import`). Over a map it is only said.
    let import_y = large_y + 30.0 * k;
    super::game_import::settings_row(Rect::new(inner.x + 14.0 * k, import_y, inner.w - 28.0 * k, 28.0 * k), over_map);
    // Razdor's own settings, in a window of their own.
    let button_y = outer.y + (h - 42.0) * k;
    let xr = Rect::new(inner.x + 14.0 * k, button_y, 120.0 * k, 32.0 * k);
    if row_button(xr, tr("Advanced…")) {
        ADVANCED.with(|a| a.set(true));
        return false;
    }
    // «Принять» keeps what was changed, «Отмена» (the close box and Esc too) puts it back.
    let cancel = Rect::new(inner.x + inner.w - 14.0 * k - 89.0 * k, button_y, 89.0 * k, 32.0 * k);
    let apply = Rect::new(cancel.x - 8.0 * k - 89.0 * k, cancel.y, 89.0 * k, 32.0 * k);
    let (apply_label, cancel_label) = (own("Buttons", "Apply", n_("Apply")), own("Buttons", "Cancel", n_("Cancel")));
    // Both act when the button is let go over it (PR #5's buttons).
    let apply_pressed = settings_button(apply, &apply_label);
    let cancel_pressed = settings_button(cancel, &cancel_label);
    if apply_pressed || key(KeyCode::Enter) {
        OPENED_WITH.with(|o| *o.borrow_mut() = None);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone of 1080×2376 px at 2.625 px per dp, its interface scale the fit (1.125).
    const PHONE: (f32, f32, f32, f32) = (1080.0, 2376.0, 1.125, 2.625);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn upright_buttons_are_a_column_of_touch_sized_ovals_above_the_chips() {
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp);
        for r in &m.buttons {
            assert!(close(r.h, OVAL.y * PORTRAIT_SCALE * k), "{r:?}");
            assert!(close(r.w, OVAL.x * PORTRAIT_SCALE * k), "{r:?}");
            assert!(r.h >= TOUCH_DP * dp, "{r:?} is under 48 dp");
            assert!(r.x >= 0.0 && r.x + r.w <= sw, "{r:?} leaves the screen");
        }
        for pair in m.buttons.windows(2) {
            assert!(pair[0].y + pair[0].h <= pair[1].y, "{pair:?} overlap");
        }
        // Under the logo (its bottom 170 px under the canvas top), above the version and notice lines.
        let top = chrome::centred_top(sw, sh, 720.0 * k);
        assert!(m.buttons[0].y >= top + 170.0 * k, "{:?} is under the logo", m.buttons[0]);
        let last = m.buttons[4];
        assert!(last.y + last.h <= m.notice_y(k) - 16.0 * k, "{last:?} runs into the lines");
        // The not-found block sits above the logo, clear of the column.
        assert!(m.block_bottom(k) < m.logo_top && m.block_bottom(k) > top, "block over the logo");
    }

    #[test]
    fn upright_chips_are_touch_sized_and_on_the_bottom_of_the_screen() {
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp);
        assert!(m.chip_h >= TOUCH_DP * dp, "chips are {} px tall", m.chip_h);
        assert!(m.chip_top + m.chip_h <= sh, "chips leave the screen");
        assert!(m.foot_y <= m.chip_top, "the lines run into the chips");
        assert!(m.version_y(k) < m.foot_y && m.notice_y(k) < m.version_y(k));
    }

    #[test]
    fn upright_geometry_fits_a_small_phone_too() {
        // 720×1600 at 2 px per dp: the fit is 0.75.
        let m = portrait_menu(720.0, 1600.0, 0.75, 2.0);
        for pair in m.buttons.windows(2) {
            assert!(pair[0].y + pair[0].h <= pair[1].y);
        }
        assert!(m.buttons[4].y + m.buttons[4].h <= m.notice_y(0.75));
        assert!(m.chip_h >= TOUCH_DP * 2.0);
    }

    #[test]
    fn landscape_buttons_keep_the_original_column() {
        // 2376×1080 at the fit (1.5): the original's column, 1:1 video geometry, centred.
        let (sw, sh, k) = (2376.0, 1080.0, 1.5);
        let rects = landscape_buttons(sw, sh, k);
        let first = rects[0];
        assert!(close(first.w, 214.0 * k) && close(first.h, 112.0 * k));
        assert!(close(first.x, sw / 2.0 - 107.0 * k));
        assert!(close(first.y, 232.0 * k - 56.0 * k), "{first:?}");
        for pair in rects.windows(2) {
            assert!(close(pair[1].y - pair[0].y, 92.0 * k), "pitch of the column");
            assert!(close(pair[0].y + pair[0].h - pair[1].y, 20.0 * k), "the 20 px overlap");
        }
    }
}
