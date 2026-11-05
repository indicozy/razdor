//! The original's main menu (the 18:08 frame of the gameplay video): the burning ruins
//! (`Windows/Castle.lit`) over the whole screen, the silver "Времена Раздора" logo
//! (`Title_RUS.ugs`) with flames rising off its letters (`TitleFlame_RUS.lit`), and five oval
//! buttons (`mb2.ugs`, the last one pointed) whose marble middles swirl in violet
//! (`MenuFlame.lit`) under the words of `SMText_RUS.lit`; the button under the mouse turns
//! gold. Razdor's own extras, the custom battle and the interface language,
//! sit small in the bottom corners. Layout numbers are pixels of the 960×720 video, scaled by [`chrome::k`].

use std::cell::Cell;

use razdor::local;
use crate::ui::input::{get_keys_pressed, is_mouse_button_down};
use macroquad::prelude::*;

use razdor::i18n::{self, n_, tr, Lang};

use super::audio::{cue, Cue};
use super::chrome::{self, Fx};
use super::keymap;
use super::minimap;
use super::window_layout;
use super::widgets::*;

/// What the player picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    NewGame,
    Load,
    Options,
    Authors,
    Exit,
    /// Razdor's custom battle (`ui::custom_battle`).
    Custom,
    /// Razdor's "Maps and mods" (`ui::hub_view`).
    Hub,
    /// Razdor Studio, the package editor (`ui::studio`); desktop builds only.
    #[cfg(all(feature = "studio", not(any(target_os = "android", target_os = "ios"))))]
    Studio,
}

/// The five buttons, top to bottom, with the row of `SMText_RUS` holding their word.
const BUTTONS: [(Pick, &str); 5] = [
    (Pick::NewGame, n_("New game")),
    (Pick::Load, n_("Load")),
    (Pick::Options, n_("Options")),
    (Pick::Authors, n_("Authors")),
    (Pick::Exit, n_("Exit")),
];

/// Size of an oval button (video pixels): `mb2.ugs` drawn 1:1 at 1024×768.
const OVAL: Vec2 = Vec2::new(214.0, 112.0);

/// Centre y of landscape oval `i` (video px from the canvas top): the original's column
/// (232 + 92 · i). The ovals keep the original's size at every text size: their words are large
/// already, and grown ovals ran into each other and the foot lines.
fn button_centre(i: usize) -> f32 {
    232.0 + i as f32 * 92.0
}

/// A phone held upright (`upright_dp`): the buttons are this many times the landscape size,
/// in one column under the logo, and the links are chips at the bottom (the owner's choice,
/// 2026-10-26, mockup "A: ×1.6").
const PORTRAIT_SCALE: f32 = 1.6;
/// The smallest touch target of a phone's link, in dp.
const TOUCH_DP: f32 = 48.0;
/// A tile of the bottom bar held upright, in dp (the mockup's four tiles, 64–72 dp tall).
const TILE_DP: f32 = 68.0;
/// The bottom bar's margin at the screen's sides and the gap between its tiles, in dp.
const CHIP_MARGIN_DP: f32 = 12.0;
const CHIP_GAP_DP: f32 = 8.0;
/// The version label's text on a phone, in dp (its sp; the menu's other small texts are 12 sp and up).
const VERSION_PHONE_DP: f32 = 13.0;
/// The chosen mod's list (the title's link or its tile), its key in the popup lists.
const MOD_LIST: &str = "razdor:menu-mod";

// Final title-flame placement from the temporary development tuner.
const LOGO_FLAME_X: f32 = -16.0;
const LOGO_FLAME_Y: f32 = 12.0;
const LOGO_FLAME_WIDTH: f32 = 0.95;
const LOGO_FLAME_HEIGHT: f32 = 1.52;
const LOGO_FLAME_GLOW: f32 = 0.85;

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
    // Held upright, the logo sits over the column (`portrait_menu`); otherwise on the canvas.
    let y = upright_menu().map_or(chrome::canvas_top() + 50.0 * k, |m| m.logo_top);
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
    let c = r.center();
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
        let frame = if hover { Color::new(1.0, 0.80, 0.42, 1.0) } else { WHITE };
        let (src, dst) = oval_picture(c, k * 0.9375, t.width(), t.height(), pointed);
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
pub(super) const DISCORD_INVITE: &str = "https://discord.gg/wmMksmWFDM";
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

/// A small text link (the Razdor extras) of text `size`. Returns true when clicked.
fn link(label: &str, x: f32, y: f32, right: bool, size: f32) -> bool {
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
    // The ruins, the logo and the foot are the interface's own: a title window's scale (set by
    // the window's frame, `window_layout::title_scale`) does not reach them.
    let _scale = super::display::enter_window_scale(None);
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
/// `k`, centred on a `sw` × `sh` screen; neighbouring ovals overlap by 20 px at Normal).
fn landscape_buttons(sw: f32, sh: f32, k: f32) -> [Rect; 5] {
    let top = chrome::centred_top(sw, sh, 720.0 * k);
    std::array::from_fn(|i| {
        let c = vec2(sw / 2.0, top + button_centre(i) * k);
        let size = OVAL * k;
        Rect::new(c.x - size.x / 2.0, c.y - size.y / 2.0, size.x, size.y)
    })
}

/// Where the two ovals of `mb2.ugs` (`w` × `h` texels) cut: the round one is rows `0..120`
/// (its body ends at row 96; rows 96..120 are empty) and the pointed one (the last button)
/// starts at row 120 with its top ornament. The half (`h / 2` = 125.5) cut the pointed one's
/// top rows off and drew them under the round oval, as a loose strip.
const MB2_SPLIT: f32 = 120.0;

/// The source and destination rects of an oval's picture, for the oval centred on `c`, at `s`
/// screen px per texel. The round oval's middle is 46 texels below the top of its piece. The
/// pointed one is drawn in the left 196 texels, and its body keeps the place the half cut gave
/// it (its piece top is `h / 2 - MB2_SPLIT` texels higher), so the landscape body is unchanged.
fn oval_picture(c: Vec2, s: f32, w: f32, h: f32, pointed: bool) -> (Rect, Rect) {
    if pointed {
        let dx = 16.0;
        let top = c.y - 46.0 * s - (h / 2.0 - MB2_SPLIT) * s;
        (
            Rect::new(0.0, MB2_SPLIT, w, h - MB2_SPLIT),
            Rect::new(c.x - (w / 2.0 - dx) * s, top, w * s, (h - MB2_SPLIT) * s),
        )
    } else {
        (Rect::new(0.0, 0.0, w, MB2_SPLIT), Rect::new(c.x - w / 2.0 * s, c.y - 46.0 * s, w * s, MB2_SPLIT * s))
    }
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
    /// The chosen mod's full-width tile, above the four tiles, when a mod is installed (its height
    /// and gap lift the foot lines and the button column; see [`portrait_menu`]).
    mod_row: Option<Rect>,
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
/// per dp, its top safe inset `safe_top` (physical px). `mod_row`: a mod is installed, so the
/// chosen mod's tile sits above the four tiles and lifts the foot lines and the buttons by its
/// height and gap.
fn portrait_menu(sw: f32, sh: f32, k: f32, dp: f32, safe_top: f32, mod_row: bool) -> PortraitMenu {
    // The logo (`logo`): 840 px wide, 128/896 of that high.
    let logo_h = 840.0 * 128.0 / 896.0 * k;
    // The chips grow with the text size (`button_grow`, the owner's decision of 2026-10-31); the
    // ovals keep their size (see `button_centre`).
    let g = super::display::button_grow();
    let size = OVAL * (PORTRAIT_SCALE * k);
    let chip_h = TILE_DP * dp;
    let chip_top = sh - 16.0 * k - chip_h;
    // The mod row above the tiles: its top is the stack's top, and the foot lines sit above it.
    let lift = if mod_row { chip_h + CHIP_GAP_DP * dp } else { 0.0 };
    let row = mod_row.then(|| Rect::new(CHIP_MARGIN_DP * dp, chip_top - lift, sw - 2.0 * CHIP_MARGIN_DP * dp, chip_h));
    let foot_y = chip_top - lift - 12.0 * k;
    // The column keeps the place it had over the old, shorter link row (its foot line at
    // `column_foot`), so the menu's buttons do not move with the taller tiles.
    let column_foot = sh - 16.0 * k - (TOUCH_DP * dp).max(24.0 * k) * g - 12.0 * k - lift;
    // The column ends above the lines. Its middle is the middle of the area between the top safe
    // inset and the foot lines; the logo stays 24 px above the column and never above the inset,
    // and the column moves down only when it must. Its gaps are shared out between the five
    // buttons (at least 6 px, at most 20 px).
    let column_end = column_foot - 70.0 * k;
    let middle = (safe_top + column_foot) / 2.0;
    let min_first = safe_top + logo_h + 24.0 * k;
    let gap = ((column_end - min_first - 5.0 * size.y) / 4.0).clamp(6.0 * k, 20.0 * k);
    let span = 5.0 * size.y + 4.0 * gap;
    let first = (middle - span / 2.0).clamp(min_first, (column_end - span).max(min_first));
    let logo_top = first - 24.0 * k - logo_h;
    let x = sw / 2.0 - size.x / 2.0;
    let buttons = std::array::from_fn(|i| Rect::new(x, first + i as f32 * (size.y + gap), size.x, size.y));
    PortraitMenu { buttons, chip_top, chip_h, foot_y, logo_top, mod_row: row }
}

local! {
    /// Whether a mod is installed, as the title menu last drew it (`frame`): the chosen mod's row
    /// is held above the tiles, and the title windows keep above it too (`portrait_foot_top`).
    static MOD_ROW: Cell<bool> = const { Cell::new(false) };
}

/// The top of the footer lines (the version and copyright row) of the portrait menu on a `sw` ×
/// `sh` screen at scale `k`: the title windows keep above it (`window_layout::portrait_title`).
pub fn portrait_foot_top(sw: f32, sh: f32, k: f32, dp: f32, safe_top: f32) -> f32 {
    portrait_menu(sw, sh, k, dp, safe_top, MOD_ROW.with(Cell::get)).foot_y
}

/// The current menu's portrait geometry, when the phone is held upright.
fn upright_menu() -> Option<PortraitMenu> {
    let dp = upright_dp()?;
    let safe_top = super::display::safe_area().top;
    Some(portrait_menu(screen_width(), screen_height(), chrome::k(), dp, safe_top, MOD_ROW.with(Cell::get)))
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
pub fn frame(no_install: bool, mods: &[(String, razdor::hub::Text)], active: Option<&str>, busy: bool) -> Option<Pick> {
    // The mod row is held only with a mod installed, as the menu draws it below.
    MOD_ROW.with(|m| m.set(!mods.is_empty()));
    let mod_choice = (!mods.is_empty()).then(|| super::new_game::mod_choice(mods, active, busy));
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
            if let Some(p) = chip_links(&m, k, mods, mod_choice.as_ref()) {
                pick = Some(p);
            }
            m.version_y(k)
        }
        None => {
            if let Some(p) = corner_links(k, mods, mod_choice.as_ref()) {
                pick = Some(p);
            }
            screen_height() - 22.0 * k - 26.0 * k
        }
    };
    // This program's version, small in the corner above the links.
    let version = format!("v{}", super::update_view::shown_version(env!("CARGO_PKG_VERSION"), super::clock::is_snapshot()));
    // On a phone the version is 13 dp (a player's report of 1503: 12 px grey over the picture was
    // hard to read) and brighter; the desktop's is as it was.
    let phone = super::display::dp_px();
    let (vsize, vcolour) = match phone {
        Some(dp) => (VERSION_PHONE_DP * dp, Color::new(0.95, 0.88, 0.72, 0.9)),
        None => (12.0 * k, Color::new(0.95, 0.88, 0.72, 0.6)),
    };
    super::dt_font::legacy_metrics_if(phone.is_some(), || {
        let vw = measure(&version, vsize).width;
        chrome::shadow_text(&version, screen_width() - 16.0 * k - vw, version_y, vsize, vcolour);
    });
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

/// The links in the landscape corners, over the original's version and copyright lines: the custom
/// battle, the maps and mods, Razdor Studio (desktop builds), the Discord and the language.
/// Returns what was picked.
fn corner_links(k: f32, mods: &[(String, razdor::hub::Text)], mod_choice: Option<&super::new_game::ModChoice>) -> Option<Pick> {
    let mut pick = None;
    let y = screen_height() - 22.0 * k;
    // The links grow with the text size (`button_grow`, the owner's decision of 2026-10-31).
    let size = 18.0 * k * super::display::button_grow();
    let custom = tr("Custom battle");
    if link(custom, 16.0 * k, y, false, size) {
        pick = Some(Pick::Custom);
    }
    let mut x = 16.0 * k + measure(custom, size).width + 28.0 * k;
    if link(tr("Maps and mods"), x, y, false, size) {
        pick = Some(Pick::Hub);
    }
    x += measure(tr("Maps and mods"), size).width + 28.0 * k;
    // The chosen mod, between «Maps and mods» and Discord, with a mod installed only (Razdor's, the
    // owner's request of 2026-11-02: the mod played is seen on the title screen).
    if let Some(mc) = mod_choice {
        let label = mc.shown();
        let w = measure(label, size).width;
        let pressed = link(label, x, y, false, size);
        mod_link_action(Rect::new(x, y - size, w, size * 1.3), pressed, mods, mc, k);
        x += w + 28.0 * k;
    }
    // Razdor Studio, the package editor: desktop builds only (the studio is compiled out of phones).
    #[cfg(all(feature = "studio", not(any(target_os = "android", target_os = "ios"))))]
    {
        if link(tr("Editor"), x, y, false, size) {
            pick = Some(Pick::Studio);
        }
        x += measure(tr("Editor"), size).width + 28.0 * k;
    }
    // The players' Discord (the owner's wish): the invite of the site, in the browser; on phones
    // too (`open_link` hands it to the system's browser there).
    if link("Discord", x, y, false, size) {
        open_link(DISCORD_INVITE);
    }
    // The language list (Razdor's): the current language; a click opens the list over it.
    let lang = super::language::current_label();
    let lw = measure(&lang, size).width;
    let anchor = Rect::new(screen_width() - 16.0 * k - lw, y - size, lw, size * 1.3);
    if link(&lang, screen_width() - 16.0 * k, y, true, size) || super::snapshot::open_language_list() {
        super::language::open_list(anchor, k);
    }
    if super::widgets::mouse_in(anchor.x, anchor.y, anchor.w, anchor.h) {
        super::widgets::tooltip(&[(super::language::hint().to_string(), super::widgets::INK)]);
    }
    pick
}

/// The links held upright: a bottom bar of four equal tiles (the mockup's), each a marble frame
/// with its icon (the game bar's own art, when the install has it) and its label under it. A
/// tile is a touch target of [`TILE_DP`] high. The language tile opens the list above the bar.
fn chip_links(m: &PortraitMenu, k: f32, mods: &[(String, razdor::hub::Text)], mod_choice: Option<&super::new_game::ModChoice>) -> Option<Pick> {
    let mut pick = None;
    let dp = upright_dp().unwrap_or(1.0);
    let lang = super::language::current_label();
    let labels: [&str; 4] = [tr("Custom battle"), tr("Maps and mods"), "Discord", &lang];
    // The game bar's icons (`button-icon-N`): 6 the crossed swords, 8 the map; the Discord and
    // language tiles have no icon in the install and show their label alone.
    let icons: [Option<&str>; 4] = [Some("button-icon-6"), Some("button-icon-8"), None, None];
    let margin = CHIP_MARGIN_DP * dp;
    let gap = CHIP_GAP_DP * dp;
    let w = (screen_width() - 2.0 * margin - 3.0 * gap) / 4.0;
    let marble = chrome::win("Win-marble");
    // One size for all four labels (grown by `button_grow`): the largest that fits the widest one.
    let size = super::dt_font::legacy_metrics_if(true, || {
        let base = 14.0 * dp * super::display::button_grow();
        labels.iter().fold(base, |size, l| {
            let tw = measure(l, size).width;
            if tw > w - 8.0 * dp { size * (w - 8.0 * dp) / tw } else { size }
        })
    });
    // The chosen mod's full-width tile above the four: the same size as their labels, shrunk only
    // if its text is wider than the row (a mod's title can be long).
    if let (Some(mc), Some(row)) = (mod_choice, m.mod_row) {
        let label = mc.shown();
        let fit = super::dt_font::legacy_metrics_if(true, || {
            let tw = measure(label, size).width;
            let room = row.w - 8.0 * dp;
            if tw > room { size * room / tw } else { size }
        });
        let pressed = chip_tile(row, k, dp, label, fit, None, marble.as_ref());
        mod_link_action(row, pressed, mods, mc, k);
    }
    let (mut lang_tile, mut lang_pressed) = (Rect::new(0.0, 0.0, 0.0, 0.0), false);
    for (j, label) in labels.iter().enumerate() {
        let r = Rect::new(margin + j as f32 * (w + gap), m.chip_top, w, m.chip_h);
        let pressed = chip_tile(r, k, dp, label, size, icons[j], marble.as_ref());
        match j {
            0 if pressed => pick = Some(Pick::Custom),
            1 if pressed => pick = Some(Pick::Hub),
            2 if pressed => open_link(DISCORD_INVITE),
            3 => (lang_tile, lang_pressed) = (r, pressed),
            _ => {}
        }
    }
    if lang_pressed || super::snapshot::open_language_list() {
        super::language::open_list(lang_tile, k);
    }
    if super::widgets::mouse_in(lang_tile.x, lang_tile.y, lang_tile.w, lang_tile.h) {
        super::widgets::tooltip(&[(super::language::hint().to_string(), super::widgets::INK)]);
    }
    pick
}

/// One marble tile of the bottom bar held upright, at `r`, with its label (and the game bar's icon
/// `icon`, when the install has it). Returns whether it was pressed this frame.
fn chip_tile(r: Rect, k: f32, dp: f32, label: &str, size: f32, icon: Option<&str>, marble: Option<&Texture2D>) -> bool {
    let (hover, down, pressed) = super::widgets::button_state(r, true);
    // The marble tile, its silver edge and the gold when the pointer is on it.
    match marble {
        Some(t) => chrome::tile(t, r, k, Color::new(0.8, 0.8, 0.8, 1.0)),
        None => plate(r, 10.0 * k, Color::new(0.08, 0.05, 0.03, 1.0)),
    }
    let edge = if down || hover { chrome::GOLD } else { chrome::SILVER };
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, edge);
    let color = if down || hover { chrome::GOLD } else { Color::new(0.95, 0.88, 0.72, 1.0) };
    // The label is drawn at its own size, not snapped to the original's bitmap sizes (phones).
    super::dt_font::legacy_metrics_if(true, || {
        let tw = measure(label, size).width;
        let text_y = match icon.and_then(|n| chrome::win_fx(n, Fx::Glow)) {
            Some(t) => {
                let ih = 28.0 * dp;
                let iw = t.width() * ih / t.height();
                chrome::tex(&t, Rect::new(r.x + (r.w - iw) / 2.0, r.y + 7.0 * dp, iw, ih), WHITE);
                r.y + r.h - 9.0 * dp
            }
            // Without an icon the label is centred in the tile.
            None => r.y + r.h / 2.0 + size * 0.35,
        };
        chrome::shadow_text(label, r.x + (r.w - tw) / 2.0, text_y, size, color);
    });
    if pressed {
        cue(Cue::Button);
    }
    pressed
}

/// The chosen mod's link (the landscape corner's or the upright tile, at `r`) for this frame: a
/// pick made in its list is asked of the app (`new_game::request_mod`: played from the next frame),
/// and a click opens the list over `r`. A mod being built takes no choice (1441).
fn mod_link_action(r: Rect, pressed: bool, mods: &[(String, razdor::hub::Text)], mc: &super::new_game::ModChoice, k: f32) {
    if let Some(v) = take_pick(MOD_LIST).filter(|&v| v != mc.current) {
        super::new_game::request_mod(super::new_game::mod_id_at(mods, v));
    }
    if !mc.busy && (pressed || super::snapshot::open_mod_list()) {
        let options: Vec<(i64, String)> = mc.labels.iter().enumerate().map(|(i, l)| (i as i64, l.clone())).collect();
        open_list(MOD_LIST, r, mc.current, &options, k);
    }
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
const THANKS: [&str; 9] = ["Нлтчк", "ivantykhonchuk", "Теневой Лорд", "! TuttiFrutti", "turgrin", "9ibatnk", "OBANAI77", "nik0N", "Hristos"];
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

/// Razdor's developer, the rest of the credits' names: the role in gold, the name in white.
fn thanks_lines() -> Vec<CreditLine> {
    let line = |text: &str, size: f32, color: Color, gap: f32| CreditLine { text: text.to_string(), size, color, gap };
    let mut lines = vec![
        line(tr("Razdor developer"), 20.0, chrome::GOLD, 0.0),
        line("indicozy", 22.0, WHITE, 0.0),
        line(tr("Contributors"), 20.0, chrome::GOLD, 14.0),
        line("Eizenvald", 18.0, WHITE, 0.0),
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
        // Under the window, down to the footer on a phone held upright (the screen's bottom else).
    let strip = Rect::new(m, page.y + page.h + m, sw - 2.0 * m, window_layout::title_bottom() - page.y - page.h - 2.0 * m);
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
    let r = window_layout::title_rect((w, h)).unwrap_or_else(|| {
        let (w, h) = (w * k, h * k);
        Rect::new((screen_width() - w) / 2.0, (chrome::canvas_top() + 190.0 * k).min(screen_height() - h), w, h)
    });
    chrome::window(r, title, chrome::Skin::Marble, true)
}

/// A window over the map, centred over the view above the bottom bar, as the original's map
/// windows are (over its 1024×682 view), its foot never on the bar. On a phone held upright it
/// is centred between the top inset and the bar at its own scale (`window_layout::over_map_rect`,
/// the caller's `window_layout::window_scale` guard).
fn map_window(title: &str, w: f32, h: f32) -> (Rect, bool) {
    let k = chrome::k();
    let r = window_layout::over_map_rect((w, h)).unwrap_or_else(|| {
        let (w, h) = (w * k, h * k);
        let view = screen_height() - chrome::bar_height();
        Rect::new((screen_width() - w) / 2.0, ((view - h) / 2.0).max(0.0), w, h)
    });
    chrome::window(r, title, chrome::Skin::Marble, true)
}

/// "Информация об авторах": the splash of scattered drawings with the credits rolling up
/// over it (`started` is when the window opened). A click or Esc closes it.
pub fn authors(started: f64) -> bool {
    backdrop();
    let _scale = window_layout::title_scale((594.0, 480.0));
    let title = own("Authors", "Title", n_("About the authors"));
    // The standard window, as the original's (4c82c4): the splash in a framed page, the
    // marble under it with «Ок» in its middle.
    let (page, closed) = window(&title, 594.0, 480.0);
    let k = chrome::k();
    // Razdor's own (the owner's decision, 2026-10-31): the OK button grows with the text size
    // (`button_grow`), its bottom edge where it was and the picture's frame above it.
    let g = super::display::button_grow();
    let inner = Rect::new(page.x + 2.0 * k, page.y + 2.0 * k, page.w - 4.0 * k, page.h - (22.0 + 32.0 * g) * k);
    let ok = Rect::new(page.center().x - 44.0 * k * g, page.y + page.h - (12.0 + 32.0 * g) * k, 89.0 * k * g, 32.0 * k * g);
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
    g.max_guard = audio.max_guard;
    g.pursue = pursue(audio);
    g.walk_pace = walk_pace(audio);
    g.friends_let_pass = audio.friends_let_pass;
    g.avoid_armies = audio.avoid_armies;
    g.keep_formation = audio.keep_formation;
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

/// "Скорость перемещения героя": the install's `WalkSpeed` (0..100, the original's range) until
/// the player sets one; the player's own value is 0..`WALK_SPEED_MAX` (200).
pub fn walk_speed(audio: &super::audio::Settings) -> f32 {
    let max = razdor::rules::game::WALK_SPEED_MAX as f32;
    audio.walk_speed.map(|v| v.clamp(0.0, max)).unwrap_or_else(|| install_int("WalkSpeed", 100).clamp(0, 100) as f32)
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
    super::world_view::set_tilt_view(audio.map_tilt);
    super::world_view::set_sprite_size(audio.map_sprite_size);
    chrome::set_option_override("ScrollSpeed", audio.scroll_speed.map(|v| (v.round() as i32).to_string()));
    chrome::set_option_override("OptValue8", audio.autosave.map(|on| if on { "1" } else { "0" }.to_string()));
    set_hints(!hints_off(audio));
    super::unit_drag::set_controls(audio.formation_controls);
    // The touchpad mode takes effect on «Принять», not when its box is ticked: while the window
    // is open the mode stays as it opened (see `touchpad_in_effect`).
    let opened_with = OPENED_WITH.with(|o| o.borrow().as_ref().map(|before| before.touchpad));
    super::touchpad::set_setting(touchpad_in_effect(audio.touchpad, opened_with));
}

/// The touchpad mode the game runs with: while the settings window is open (`opened_with` holds
/// the settings as the window found them) the value it opened with, so a ticked box applies on
/// «Принять» only; once closed, the live setting.
fn touchpad_in_effect(live: Option<bool>, opened_with: Option<Option<bool>>) -> Option<bool> {
    opened_with.unwrap_or(live)
}

/// "Настройки звука, графики и геймплея", with what Razdor lets the player change: the
/// music and sound volumes (saved at once) and the interface language. Returns true when
/// closed.
pub fn options(audio: &mut super::audio::Settings, install_wide: bool) -> bool {
    backdrop();
    let _scale = window_layout::title_scale((594.0, options_h()));
    options_window(audio, install_wide, false)
}

/// The settings window's height in video px (its rows, with the phone's own rows).
fn options_h() -> f32 {
    // On a phone the touchpad mode's row (`ui::touchpad`) takes 28 more px under the checkboxes.
    let touch_row = super::touchpad::MOBILE;
    // The orientation row is Android's: iOS keeps the system's rotation lock for now, so it has no row there.
    let orientation_row = cfg!(target_os = "android");
    // Razdor's two rows at the end (the text size and the large windows, the owner's request:
    // phones' text too small) take 76 px more: the frame line under the checkboxes sits 82 px above
    // the bottom edge, the buttons 68 above. The import row («Обновить игру из архива…», `ui::game_import`)
    // takes 30 px more under the large message windows' row.
    // Razdor's own (the owner's decision, 2026-10-31): the three rows under the checkboxes and the
    // buttons grow with the text size; the window takes the 84 px their rows add and the 32 px
    // its buttons add (`button_grow`).
    (if orientation_row { 556.0 } else if touch_row { 528.0 } else { 500.0 }) + 76.0 + 30.0 + 116.0 * (super::display::button_grow() - 1.0)
}

/// The step between the options window's rows under the checkboxes (video px): 30 at Normal,
/// and a button grown by `g` (28 × g high) keeps its 2 px gap (`button_grow`).
fn option_row_step(g: f32) -> f32 {
    30.0 + 28.0 * (g - 1.0)
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
    /// The F1 list's «Изменить клавиши…» was pressed (`take_keys_request` takes it).
    static KEYS_ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

local! {
    /// The advanced window's scroll on a phone: px of its content above the top of the window.
    static ADV_SCROLL: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// Where the advanced window's content ends on a phone, px below the window's content top (as
    /// the last frame drew it, unscrolled): the scroll's range is this less the view.
    static ADV_BOTTOM: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// The area the advanced window's rows show in on a phone (screen px); None elsewhere.
    static ADV_VIEW: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// Opens the advanced settings over the settings window (a snapshot scene).
pub fn open_advanced() {
    ADVANCED.with(|a| a.set(true));
    ADV_SCROLL.with(|s| s.set(0.0));
}

/// Opens the keys window over the advanced settings (a snapshot scene, and the F1 list's button).
pub fn open_keys() {
    open_advanced();
    KEYS_OPEN.with(|o| o.set(true));
}

/// The F1 list (over the map) asks for the keys window: the map's frame then opens the settings
/// over the map with the keys window on top, as the bar's gears and «Ещё…» → «Клавиши…» do.
pub fn ask_keys() {
    KEYS_ASKED.with(|a| a.set(true));
}

/// True once after `ask_keys`.
pub fn take_keys_request() -> bool {
    KEYS_ASKED.with(|a| a.replace(false))
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
    // On a phone the advanced window's rows show only in its scroll area: a row outside it is
    // neither drawn nor clicked, and one half in it is clicked only where the pointer is in it.
    if let Some(view) = ADV_VIEW.with(|v| v.get()) {
        if !overlaps(r, view) {
            return false;
        }
        let pressed = settings_button(r, label);
        let (mx, my) = crate::ui::widgets::pointer();
        return pressed && in_rect((mx, my), view);
    }
    settings_button(r, label)
}

/// Whether two rectangles share a point.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Whether the point is inside the rectangle.
fn in_rect(p: (f32, f32), r: Rect) -> bool {
    p.0 >= r.x && p.0 < r.x + r.w && p.1 >= r.y && p.1 < r.y + r.h
}

/// Whether a control at `r` may take input now: on a phone's advanced window, only in its scroll area.
fn in_adv_view(r: Rect) -> bool {
    ADV_VIEW.with(|v| v.get()).is_none_or(|view| overlaps(r, view))
}

/// A scroll range: how far content that ends `bottom` px down goes past a `view` px high window
/// (0 when it fits).
fn scroll_most(bottom: f32, view: f32) -> f32 {
    (bottom - view).max(0.0)
}

/// Razdor's advanced settings (the settings window's "Ещё…"): rules where Razdor can differ
/// from the original on the left, the screen, the interface scale, the font and the
/// language on the right. Returns true when closed.
fn advanced_window(audio: &mut super::audio::Settings, over_map: bool) -> bool {
    // 650 high since «Расстановка армии» joined the left column (it still fits the 682 px
    // view above the bar).
    let (ww, wh) = (900.0, 650.0);
    // Its own scale on a title screen or in game on a phone held upright (its own window, not the
    // settings window's).
    // The scale the window came in with: the new-game list's scroll bar takes it (`new_game::list_track`).
    let list_k = chrome::k();
    let _scale = window_layout::window_scale(over_map, (ww, wh));
    // Razdor's own window, wider than the original's: centred (over the map, above the bar),
    // not hung from the menu's 190, where its foot reached the screen's bottom edge.
    let k = chrome::k();
    // The rows' and the Ok button's height grows with the text size (`button_grow`, the owner's
    // decision of 2026-10-31); the Ok button keeps its bottom edge.
    let row_h = 28.0 * k * super::display::button_grow();
    let (inner, closed) = if over_map {
        map_window(tr("Advanced settings"), ww, wh)
    } else {
        let (w, h) = (ww * k, wh * k);
        let r = window_layout::title_rect((ww, wh)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, ((screen_height() - h) / 2.0).max(0.0), w, h));
        chrome::window(r, tr("Advanced settings"), chrome::Skin::Marble, true)
    };
    // On a phone (in either orientation) both columns scroll together above the Ok button: the
    // content's top moves up by `scroll`, drawn and clicked in `view` only, with a finger's slide
    // or the wheel and a bar at the right. The desktop draws every row, as before.
    let on_phone = super::display::dp_px().is_some();
    let view_h = inner.h - 16.0 * k - row_h - 4.0 * k;
    let view = Rect::new(inner.x, inner.y, inner.w, view_h);
    let most = if on_phone { scroll_most(ADV_BOTTOM.with(|b| b.get()), view_h) } else { 0.0 };
    let mut scroll = if on_phone { ADV_SCROLL.with(|s| s.get()).clamp(0.0, most) } else { 0.0 };
    if most > 0.0 {
        super::widgets::swipe_rows(view, 40.0 * k);
        if crate::ui::widgets::mouse_in(view.x, view.y, view.w, view.h) {
            scroll = (scroll - wheel() * 40.0 * k).clamp(0.0, most);
        }
        // The original's bar at twice the new-game list's width: 24 × k wide, 4 × k from the window's right.
        let bar = Rect::new(inner.x + inner.w - 28.0 * list_k, inner.y, 24.0 * list_k, view_h);
        super::widgets::scrollbar_px(bar, &mut scroll, most, view_h, 40.0 * k);
    }
    if on_phone {
        ADV_SCROLL.with(|s| s.set(scroll));
        phone_clip(Some(view));
        ADV_VIEW.with(|v| v.set(Some(view)));
    }
    let top = inner.y - scroll;
    // The lowest drawn point of the content, in the scrolled coordinates (see `ADV_BOTTOM`).
    let mut content_bottom = top;
    // The left column: the rules, each with its explanation.
    let col = Rect::new(inner.x, top, 560.0 * k, inner.h);
    let y = col.y + 30.0 * k;
    chrome::shadow_text(tr("Stepping onto a friendly army"), col.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let br = Rect::new(col.x + col.w - 24.0 * k - 170.0 * k, y, 170.0 * k, row_h);
    let label = if audio.friends_let_pass { tr("Let pass") } else { tr("Battle (original)") };
    if row_button(br, label) {
        audio.friends_let_pass = !audio.friends_let_pass;
    }
    let hint = tr("With no event of the map for the meeting, the original opens a battle against a friendly army the hero steps onto; Razdor can let him pass instead. An army chosen by a click is still fought.");
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
    if let Some(v) = slider(track, (audio.map_zoom - lo) / (hi - lo)).filter(|_| in_adv_view(track)) {
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
    let army_lines = wrap(hint, col.w - 48.0 * k, 12.0 * k);
    for (i, line) in army_lines.iter().enumerate() {
        chrome::shadow_text(line, col.x + 24.0 * k, y + 56.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // The last block of the left column (the blocks go down the column).
    content_bottom = content_bottom.max(y + 56.0 * k + army_lines.len() as f32 * 17.0 * k);
    // The right column: how Razdor shows the game.
    let rx = inner.x + 580.0 * k;
    // A phone keeps its right margin for the scroll bar (24 × k wide, 4 × k to its right and 4 × k to its left).
    let margin = if on_phone { 32.0 * list_k } else { 24.0 * k };
    let rw = inner.x + inner.w - margin - rx;
    let bw = 150.0 * k;
    let ok_h = row_h;
    // The column's hints, wrapped first: they set the row pitch, which is the original's 44 px
    // unless the column would run past the Ok button's top. The column is twelve pitches from its
    // first row to the language row, the hint slot (44 px plus its lines), and the language's
    // description (48 px down, then its lines).
    let ai_hint = wrap(tr("Maximum: before each move the enemy of your battles weighs every move and answer ahead and plays each out to the end of the battle."), rw, 12.0 * k);
    let lang_hint = wrap(super::language::hint(), rw, 12.0 * k);
    let hints_h = 44.0 * k + ai_hint.len() as f32 * 17.0 * k + 48.0 * k + lang_hint.len() as f32 * 17.0 * k;
    let ok_top = inner.h - 16.0 * k - ok_h - 4.0 * k;
    // The 2.5D view's sprite size row is there only with the 2.5D view on: one more pitch then.
    let extra_rows = if audio.map_tilt { 1.0 } else { 0.0 };
    let mut pitch = right_column_pitch(30.0 * k, ok_top, 12.0 + extra_rows, hints_h, 44.0 * k);
    // At a grown size the rows keep a 2 px gap under their grown buttons (at Normal nothing).
    if row_h > 28.0 * k {
        pitch = pitch.max(row_h + 2.0 * k);
    }
    let mut y = top + 30.0 * k;
    let row = |label: &str, value: &str, y: &mut f32| -> bool {
        chrome::shadow_text(label, rx, *y + 18.0 * k, 14.0 * k, chrome::CREAM);
        let r = Rect::new(rx + rw - bw, *y, bw, row_h);
        *y += pitch;
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
    // The hint sits 8 px under the fourth row's button and the next row 8 px under the hint (the
    // original's 44 px pitch: the hint's top at 4 × 44 - 8 px).
    let hint_top = y - pitch + 36.0 * k;
    for (i, line) in ai_hint.iter().enumerate() {
        chrome::shadow_text(line, rx, hint_top + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    y = hint_top + ai_hint.len() as f32 * 17.0 * k + 8.0 * k;
    if row(tr("Unit battle participation"), if audio.allow_unit_benching { tr("On") } else { tr("Off") }, &mut y) {
        audio.allow_unit_benching = !audio.allow_unit_benching;
    }
    // Razdor's option: the route closes every army's cell (off: the original's route).
    if row(tr("Route around armies"), if audio.avoid_armies { tr("On") } else { tr("Off") }, &mut y) {
        audio.avoid_armies = !audio.avoid_armies;
    }
    // Razdor's deviation (the owner's request): after a battle every unit goes back to its cell
    // from before it (`Game::keep_formation`); off, the original's write-back.
    if row(tr("Keep formation after battle"), if audio.keep_formation { tr("On") } else { tr("Off") }, &mut y) {
        audio.keep_formation = !audio.keep_formation;
    }
    // «Максимум бережёт магов» (`Battle::set_max_guard`), on by default (the owner's decision, 2026-10-31):
    // off by default, a gameplay change of «Максимум».
    if row(tr("Max guards casters"), if audio.max_guard { tr("On") } else { tr("Off") }, &mut y) {
        audio.max_guard = !audio.max_guard;
    }
    // Razdor's options: a battle between armies on the map shows a mark and a notice (on by
    // default); the armies the player sees are listed under the minimap (off by default).
    if row(tr("Battles on the map"), if audio.battle_marks { tr("On") } else { tr("Off") }, &mut y) {
        audio.battle_marks = !audio.battle_marks;
    }
    if row(tr("Minimap army list"), if audio.army_list { tr("On") } else { tr("Off") }, &mut y) {
        audio.army_list = !audio.army_list;
    }
    // Razdor's option (the owner's decision, 2026-11-02): the world map's 2.5D view, a tilted
    // camera with the standing sprites as billboards (`world_view::tilt_view`); off by default.
    let view_label = if audio.map_tilt { "2.5D" } else { tr("Flat") };
    if row(tr("Map view"), view_label, &mut y) {
        audio.map_tilt = !audio.map_tilt;
    }
    // Razdor's (the owner's decision, 2026-11-05): the standing sprites' size, shown with the 2.5D
    // view it changes (1×, 1.5× or 2×; a stored size of another value shows as it is).
    if audio.map_tilt && row(tr("Sprite size"), &sprite_size_label(audio.map_sprite_size), &mut y) {
        audio.map_sprite_size = super::world_view::next_sprite_size(audio.map_sprite_size);
    }
    // Razdor's options (a player's report: the minimap dragged where it is hard to move back):
    // its corner, cycled clockwise, and «Сбросить» puts its size and place back to the default.
    let corner_label = match minimap::corner() {
        minimap::Corner::TopRight => tr("Top right"),
        minimap::Corner::BottomRight => tr("Bottom right"),
        minimap::Corner::BottomLeft => tr("Bottom left"),
        minimap::Corner::TopLeft => tr("Top left"),
    };
    if row(tr("Minimap corner"), corner_label, &mut y) {
        minimap::set_corner(minimap::corner().next());
    }
    if row(tr("Minimap size and place"), tr("Reset"), &mut y) {
        minimap::reset();
    }
    // The keys window (Razdor's, the owner's request): every action's key, rebound by a click.
    if row(tr("Keyboard shortcuts"), tr("Keys…"), &mut y) {
        KEYS_OPEN.with(|o| o.set(true));
    }
    // The language list (Razdor's): Russian, English and the translations of the data found
    // in the install; the button opens it.
    chrome::shadow_text(tr("Language"), rx, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let lw = (rw - 100.0 * k).max(bw);
    let lr = Rect::new(rx + rw - lw, y, lw, row_h);
    if row_button(lr, &super::language::current_label()) || super::snapshot::open_language_list() {
        super::language::open_list(lr, k);
    }
    for (i, line) in lang_hint.iter().enumerate() {
        chrome::shadow_text(line, rx, y + 48.0 * k + i as f32 * 17.0 * k, 12.0 * k, WHITE);
    }
    // The language row is the right column's last (its description's lines end the column).
    content_bottom = content_bottom.max(y + 48.0 * k + lang_hint.len() as f32 * 17.0 * k);
    if on_phone {
        phone_clip(None);
        ADV_VIEW.with(|v| v.set(None));
        ADV_BOTTOM.with(|b| b.set(content_bottom - top));
    }
    let ok = Rect::new(inner.x + inner.w - 120.0 * k, inner.y + inner.h - 16.0 * k - ok_h, 96.0 * k, ok_h);
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let done = closed || key(KeyCode::Escape) || key(KeyCode::Enter) || settings_button(ok, &ok_label);
    if done {
        ADV_SCROLL.with(|s| s.set(0.0));
    }
    done
}

/// The sprite size as the settings button shows it: `1×`, `1.5×`, `2×`, or the stored value (`1.3×`).
fn sprite_size_label(f: f32) -> String {
    format!("{}×", (f * 100.0).round() / 100.0)
}

/// The right column's row pitch in the advanced window (px at its scale): `max` (the original's
/// 44 px), or less when the column's `rows` pitches and its `hints` (px, the hint slots and the
/// language's description) would run past `limit` (the Ok button's top), counted from `top`.
fn right_column_pitch(top: f32, limit: f32, rows: f32, hints: f32, max: f32) -> f32 {
    ((limit - top - hints) / rows).min(max)
}

fn settings_button(r: Rect, label: &str) -> bool {
    let (_, down, pressed) = super::widgets::button_state(r, true);
    chrome::marble_button(r, label, true, down);
    if pressed { cue(Cue::Button); }
    pressed
}

/// A phone held upright: a slider row with its label on its own line over ◀ the track ▶, which
/// takes the row's whole width (a long label is never cut by the track).
fn option_slider_stacked(r: Rect, label: &str, value: f32, size: f32) -> Option<f32> {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    chrome::silver_frame(r, 1.0);
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        chrome::shadow_text(label, r.x + 8.0 * k, r.y + 6.0 * k + size, size, OPTION_INK);
    });
    let a = 22.0 * k;
    let y = r.y + r.h - 6.0 * k - a;
    let left = Rect::new(r.x + 8.0 * k, y, a, a);
    let right = Rect::new(r.x + r.w - 8.0 * k - a, y, a, a);
    let track = Rect::new(left.x + left.w, y, right.x - left.x - left.w, a);
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

/// One of the original's slider rows (0x4d01c9: 32 px boxes 40 apart from 45 px under the
/// window's top, 18 px in from its sides, a dark fill and a frame, the label 8 px in): the
/// label on the left, ◀ the track ▶ on the right. Returns the new value (0..1) when moved.
fn option_slider(r: Rect, label: &str, value: f32) -> Option<f32> {
    option_slider_sized(r, label, value, 15.0 * chrome::k())
}

/// [`option_slider`] with its label at `size` px (the phone's 14 sp).
fn option_slider_sized(r: Rect, label: &str, value: f32, size: f32) -> Option<f32> {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    chrome::silver_frame(r, 1.0);
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        chrome::shadow_text(label, r.x + 8.0 * k, r.y + r.h * 0.5 + 6.0 * k, size, OPTION_INK);
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
/// The keys window's rows (nine to a column), its hint and its height in video px at the text
/// size's growth `g` (Razdor's own, the owner's decision of 2026-10-31): a row's button is 28 × g
/// high and keeps its 4 px gap, the hint and the bottom buttons (30 × g) follow the rows down.
/// At Normal: 32 px pitch, the hint at 318, a 420 px window.
fn keys_layout(g: f32) -> (f32, f32, f32) {
    let row_h = 28.0 * g;
    let pitch = row_h + 4.0;
    let hint_y = 16.0 + 8.0 * pitch + row_h + 18.0;
    let height = 420.0 + (hint_y + 45.0 + 30.0 * g) - (318.0 + 45.0 + 30.0);
    (pitch, row_h, height)
}

/// The keys window on a phone held upright (the owner's notes, 2026-11-02; Razdor's own, not in the
/// original): it takes the phone's width and is laid out in dp (`dp` px per dp, so one unit is one
/// dp and a 14 px label is 14 sp). Two columns of actions, each with its key button (52 × 44 dp),
/// the hint under the rows, «По умолчанию» and «Ok» (44 dp) at the bottom. A label that takes more
/// than two lines is set at 13 sp, else 12 sp. The desktop keeps [`keys_window`]'s layout.
fn keys_window_upright(audio: &mut super::audio::Settings, over_map: bool, dp: f32) -> bool {
    let _scale = super::display::enter_window_scale(Some(dp));
    let k = chrome::k();
    let g = super::display::button_grow();
    let (sw, sh) = (screen_width(), screen_height());
    let margin = 8.0 * k;
    let w = sw - 2.0 * margin;
    let row_h = 44.0 * g * k;
    let pitch = row_h + 4.0 * k;
    let bh = 44.0 * g * k;
    let hint = tr("Click a key, then press the new one. Esc cancels; a key another action has goes to that action.");
    let hint_size = 14.0 * k;
    let hint_lines = super::dt_font::legacy_metrics_if(true, || wrap(hint, w - 4.0 * k - 20.0 * k, hint_size));
    let rows_h = 9.0 * pitch - 4.0 * k;
    let hint_h = hint_lines.len().max(1) as f32 * 19.0 * k;
    let content = 10.0 * k + rows_h + 12.0 * k + hint_h + 12.0 * k + bh + 10.0 * k;
    let h = content + 44.0 * k;
    // Between the top inset and the bottom bar (over the map) or the title screen's footer.
    let safe = super::display::safe_area();
    let top = safe.top + 8.0 * k;
    let bottom = if over_map { sh - chrome::bar_height() - 8.0 * k } else { window_layout::title_bottom() - 8.0 * k };
    let h = h.min(bottom - top);
    let y = top + ((bottom - top - h) / 2.0).max(0.0);
    let (inner, closed) = chrome::window(Rect::new(margin, y, w, h), tr("Keys"), chrome::Skin::Marble, true);
    let waiting = keymap::listening();
    let was_waiting = waiting.is_some();
    let col_gap = 12.0 * k;
    let col_w = (inner.w - 20.0 * k - col_gap) / 2.0;
    let key_w = 52.0 * k;
    let mut started = false;
    super::dt_font::legacy_metrics_if(true, || {
        for (i, a) in keymap::ALL.into_iter().enumerate() {
            let x = inner.x + 10.0 * k + (i / 9) as f32 * (col_w + col_gap);
            let y = inner.y + 10.0 * k + (i % 9) as f32 * pitch;
            let (size, lines) = upright_label(tr(a.label()), col_w - key_w - 8.0 * k, k);
            if lines.len() == 1 {
                text_fit(&lines[0], x, y + row_h / 2.0 + size * 0.35, col_w - key_w - 8.0 * k, size, chrome::CREAM);
            } else {
                let top = y + (row_h - lines.len() as f32 * size * 1.15) / 2.0 + size * 0.85;
                for (n, line) in lines.iter().enumerate() {
                    chrome::shadow_text(line, x, top + n as f32 * size * 1.15, size, chrome::CREAM);
                }
            }
        }
    });
    for (i, a) in keymap::ALL.into_iter().enumerate() {
        let x = inner.x + 10.0 * k + (i / 9) as f32 * (col_w + col_gap);
        let y = inner.y + 10.0 * k + (i % 9) as f32 * pitch;
        let br = Rect::new(x + col_w - key_w, y, key_w, row_h);
        let shown = if waiting == Some(a) { tr("press a key…").to_string() } else { keymap::display(keymap::key(a)) };
        if row_button(br, &shown) && waiting != Some(a) {
            keymap::listen(Some(a));
            started = true;
        }
    }
    super::dt_font::legacy_metrics_if(true, || {
        for (n, line) in hint_lines.iter().enumerate() {
            chrome::shadow_text(line, inner.x + 10.0 * k, inner.y + 10.0 * k + rows_h + 12.0 * k + hint_size * 0.85 + n as f32 * 19.0 * k, hint_size, WHITE);
        }
    });
    take_new_key(audio, waiting, started);
    let by = inner.bottom() - 10.0 * k - bh;
    let ok_w = 120.0 * k;
    let defaults = Rect::new(inner.x + 10.0 * k, by, inner.w - 20.0 * k - ok_w - 10.0 * k, bh);
    let ok = Rect::new(inner.right() - 10.0 * k - ok_w, by, ok_w, bh);
    if settings_button(defaults, tr("Defaults")) {
        reset_keys(audio);
    }
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let ok_pressed = settings_button(ok, &ok_label);
    closed || ok_pressed || (!was_waiting && key(KeyCode::Escape))
}

/// A key action's label on a phone's keys window: 14 px (sp at one unit per dp), set at 13 or 12
/// when 14 would take more than two lines of `width`; one line at 12 when even that does not fit
/// (the caller ellipsizes it). Returns the size and the lines.
fn upright_label(s: &str, width: f32, k: f32) -> (f32, Vec<String>) {
    for n in [14.0, 13.0, 12.0] {
        let size = n * k;
        let lines = wrap(s, width, size);
        if lines.len() <= 2 {
            return (size, lines);
        }
    }
    (12.0 * k, vec![s.to_string()])
}

/// The new key for the action waiting for one (`keymap::listen`): the key pressed, unless the
/// click on a key button just started the wait (`started`) or Esc or a click cancels it. The keys
/// are swallowed while waiting.
fn take_new_key(audio: &mut super::audio::Settings, waiting: Option<keymap::Action>, started: bool) {
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
        }
        swallow_input();
    }
}

/// «По умолчанию»: every key back to its default, and no wait.
fn reset_keys(audio: &mut super::audio::Settings) {
    keymap::reset();
    keymap::listen(None);
    audio.keys = keymap::overrides();
}

fn keys_window(audio: &mut super::audio::Settings, over_map: bool) -> bool {
    if let Some(dp) = super::display::dp_px().filter(|_| window_layout::portrait()) {
        return keys_window_upright(audio, over_map, dp);
    }
    // The rows grow with the text size, unless that only makes the window's scale smaller (a phone
    // held sideways fits the window to its height): then they keep their Normal size, and the
    // window's text stays as large as before.
    let g = {
        let k_at = |h: f32| {
            let _scale = window_layout::window_scale(over_map, (760.0, h));
            chrome::k()
        };
        let g = super::display::button_grow();
        if g > 1.0 && k_at(keys_layout(g).2) < k_at(keys_layout(1.0).2) * 0.999 { 1.0 } else { g }
    };
    let (pitch, row_h, keys_h) = keys_layout(g);
    let _scale = window_layout::window_scale(over_map, (760.0, keys_h));
    let k = chrome::k();
    let (w, h) = (760.0 * k, keys_h * k);
    let r = window_layout::window_rect(over_map, (760.0, keys_h)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, ((screen_height() - h) / 2.0).max(0.0), w, h));
    let (inner, closed) = chrome::window(r, tr("Keys"), chrome::Skin::Marble, true);
    let waiting = keymap::listening();
    let was_waiting = waiting.is_some();
    let col_w = (inner.w - 60.0 * k) / 2.0;
    let mut started = false;
    for (i, a) in keymap::ALL.into_iter().enumerate() {
        let x = inner.x + 20.0 * k + (i / 9) as f32 * (col_w + 20.0 * k);
        let y = inner.y + 16.0 * k + (i % 9) as f32 * pitch * k;
        text_fit(tr(a.label()), x, y + row_h * k / 2.0 + 4.0 * k, col_w - 170.0 * k, 14.0 * k, chrome::CREAM);
        let br = Rect::new(x + col_w - 150.0 * k, y, 150.0 * k, row_h * k);
        let shown = if waiting == Some(a) { tr("press a key…").to_string() } else { keymap::display(keymap::key(a)) };
        if row_button(br, &shown) && waiting != Some(a) {
            keymap::listen(Some(a));
            started = true;
        }
    }
    take_new_key(audio, waiting, started);
    let hint = tr("Click a key, then press the new one. Esc cancels; a key another action has goes to that action.");
    let hint_y = 16.0 + 8.0 * pitch + row_h + 18.0;
    text_fit(hint, inner.x + 20.0 * k, inner.y + hint_y * k, inner.w - 40.0 * k, 12.0 * k, WHITE);
    let bh = 30.0 * g * k;
    let defaults = Rect::new(inner.x + 20.0 * k, inner.y + inner.h - 14.0 * k - bh, 150.0 * k, bh);
    let ok = Rect::new(inner.x + inner.w - 120.0 * k, inner.y + inner.h - 14.0 * k - bh, 96.0 * k, bh);
    if settings_button(defaults, tr("Defaults")) {
        reset_keys(audio);
    }
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let ok_pressed = settings_button(ok, &ok_label);
    closed || ok_pressed || (!was_waiting && key(KeyCode::Escape))
}

// The settings window's scroll on a phone held upright, in dp.
local! {
    static PHONE_SCROLL: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// Cuts the drawing off outside `clip` (screen px), or lifts the cut (`None`).
fn phone_clip(clip: Option<Rect>) {
    let dpi = screen_dpi_scale();
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.scissor(clip.map(|r| ((r.x * dpi) as i32, (r.y * dpi) as i32, (r.w * dpi).ceil() as i32, (r.h * dpi).ceil() as i32)));
}

/// The words of `label` in lines of at most `max_w` px at `size` (a word never splits).
fn wrap_label(label: &str, max_w: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in label.split(' ') {
        let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if !cur.is_empty() && measure(&cand, size).width > max_w {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// A checkbox row whose label may wrap: the box stays on the first line. True when clicked.
fn option_check_lines(x: f32, y: f32, width: f32, lines: &[String], on: bool, ink: Color, u: f32) -> bool {
    let b = Rect::new(x, y, 26.0 * u, 26.0 * u);
    let (art, mask) = if on { ("CBoxChecked", "CBoxChecked-Alpha") } else { ("CBoxUn", "CBoxUn-Alpha") };
    match chrome::win_masked(art, mask) {
        Some(t) => chrome::tex(&t, b, WHITE),
        None => {
            draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.0, 0.0, 0.0, 0.5));
            chrome::silver_frame(b, 1.0);
            if on {
                draw_line(b.x + 3.0 * u, b.y + 9.0 * u, b.x + 8.0 * u, b.y + 15.0 * u, 2.5 * u, GREEN);
                draw_line(b.x + 8.0 * u, b.y + 15.0 * u, b.x + 16.0 * u, b.y + 2.0 * u, 2.5 * u, GREEN);
            }
        }
    }
    let tx = x + 34.0 * u;
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_text(line, tx, y + 19.0 * u + i as f32 * 18.0 * u, 14.0 * u, ink);
    }
    let h = (10.0 + lines.len() as f32 * 18.0) * u;
    let hit = Rect::new(x, y, width, h.max(26.0 * u));
    let clicked_it = hit.contains(crate::ui::widgets::pointer().into()) && !input_blocked() && clicked();
    if clicked_it {
        cue(Cue::Button);
    }
    clicked_it
}

/// The options window on a phone held upright, in dp: the window takes the phone's width and
/// the room from the top inset to the bar (over a map) or to the title's foot; one unit is one
/// dp, so the labels are 14 units (14 sp) and the rows 40 units. A long checkbox label wraps to a
/// second line; the rows scroll when they are taller than the window. Desktop and sideways keep
/// [`options_window`]'s own layout. Returns true when closed.
fn phone_options(audio: &mut super::audio::Settings, install_wide: bool, over_map: bool, u: f32) -> bool {
    super::dt_font::legacy_metrics_if(true, || {
        let (sw, sh) = (screen_width(), screen_height());
        let top = super::display::safe_area().top + 8.0 * u;
        let bottom = if over_map { sh - chrome::bar_height() - 8.0 * u } else { window_layout::title_bottom() - 8.0 * u };
        // The scale the window came in with (the title's or the map's), which the new-game list's
        // scroll bar also takes (`new_game::list_track`): the bar's size comes from it, not from `u`.
        let list_k = chrome::k();
        let _scale = super::display::enter_window_scale(Some(u));
        let title = own("Options", "Title", n_("Sound, graphics and gameplay settings"));
        let rect = Rect::new(8.0 * u, top, sw - 16.0 * u, (bottom - top).max(200.0 * u));
        let (inner, closed) = chrome::window(rect, &title, chrome::Skin::Marble, true);
        let pad = 12.0 * u;
        let x0 = inner.x + pad;
        // The scroll bar: twice the new-game list's (`new_game::list_track`, 12 × k), 24 × k wide, 4 × k
        // from the window's right edge; the rows stop 4 × k before it.
        let bar_x = inner.x + inner.w - 28.0 * list_k;
        let cw = bar_x - 4.0 * list_k - x0;
        // The list between the window's top and the buttons row (44 units high, 10 above its bottom).
        let list = Rect::new(inner.x, inner.y + 6.0 * u, inner.w, inner.h - 6.0 * u - 66.0 * u);
        let row_h = 40.0 * u;
        let pitch = 46.0 * u;
        // A slider row stacks its label over its track, so a long label is never cut by the track.
        let (slider_h, slider_pitch) = (58.0 * u, 64.0 * u);
        let mod_rows = chrome::install_option("Option0").is_some() && chrome::install_option("OptionSld0").is_none();
        let slider_label = |n: usize, ours: &'static str| own("Options", &if mod_rows { format!("Option{n}") } else { format!("OptionSld{n}") }, ours);
        let anim_label = if mod_rows { tr(n_("Battle animation speed")).to_string() } else { slider_label(4, n_("Battle animation speed")) };
        let slider_rows: [(String, f32); 5] = [
            (slider_label(0, n_("Background music volume")), audio.music_volume),
            (slider_label(1, n_("Sound effects volume")), audio.sfx_volume),
            (slider_label(2, n_("Map scroll speed")), scroll_speed(audio) / 100.0),
            (slider_label(3, n_("Hero's walking speed")), walk_speed(audio) / razdor::rules::game::WALK_SPEED_MAX as f32),
            (anim_label, anim_speed(audio).unwrap_or(0.0) / 99.0),
        ];
        let touch_row = super::touchpad::MOBILE;
        let orientation_row = cfg!(target_os = "android");
        let checks: Vec<(&'static str, String, bool, Color)> = {
            let mut c = vec![
                ("Option4", own("Options", "Option4", n_("Do not load music and sounds (needs a restart)")), no_sound(audio), CHECK_ORANGE),
                ("Option5", own("Options", "Option5", n_("Show FPS")), audio.show_fps, CHECK_INK),
                ("Option6", own("Options", "Option6", n_("Turn off hints")), hints_off(audio), CHECK_INK),
                ("Option7", own("Options", "Option7", n_("Automatically pursue the chosen army")), pursue(audio), CHECK_INK),
                ("Option8", own("Options", "Option8", n_("Automatically save the current game")), autosave(audio), CHECK_INK),
                ("Option9", own("Options", "Option9", n_("Improved enemy AI in battle")), expert_ai(audio), CHECK_INK),
                ("Option10", own("Options", "Option10", n_("Impossible difficulty (needs a restart)")), impossible(audio), CHECK_INK),
                ("Option11", own("Options", "Option11", n_("Wide front row in battle (for new games)")), wide_row(audio, install_wide), CHECK_INK),
            ];
            if touch_row {
                c.push(("RazdorTouchpad", own("Options", "RazdorTouchpad", n_("Touchpad mode (the screen moves a cursor)")), audio.touchpad.unwrap_or(false), CHECK_INK));
            }
            c
        };
        // Each checkbox's lines, and the content's height in units from the list's top.
        let check_lines: Vec<Vec<String>> = checks.iter().map(|c| wrap_label(&c.1, cw - 34.0 * u, 14.0 * u)).collect();
        let check_h: Vec<f32> = check_lines.iter().map(|l| (10.0 + l.len() as f32 * 18.0).max(38.0) * u).collect();
        let slider_top = 0.0;
        let checks_top = slider_top + slider_rows.len() as f32 * slider_pitch + 6.0 * u;
        let mut check_y = Vec::with_capacity(checks.len());
        let mut y = checks_top;
        for h in &check_h {
            check_y.push(y);
            y += h;
        }
        let mut rest_y = y + 6.0 * u;
        let orient_y = rest_y;
        if orientation_row {
            rest_y += pitch;
        }
        let text_y = rest_y;
        let large_y = text_y + pitch;
        let import_y = large_y + pitch;
        let content = import_y + row_h;

        // Scroll: the wheel and the bar at the list's right edge.
        let view = list.h;
        let most = (content - view).max(0.0);
        let mut scroll = PHONE_SCROLL.with(|c| c.get()).clamp(0.0, most);
        if most > 0.0 {
            // A finger's slide over the list turns the wheel a row (24 dp), as the wheel does; the
            // bar at the right (registered after, so it wins over the list) takes its own slide.
            super::widgets::swipe_rows(list, 24.0 * u);
            if crate::ui::widgets::mouse_in(list.x, list.y, list.w, list.h) {
                scroll = (scroll - wheel() * 24.0 * u).clamp(0.0, most);
            }
            // The original's bar (its arrows, track and knob), twice the new-game list's width.
            let bar = Rect::new(bar_x, list.y, 24.0 * list_k, view);
            super::widgets::scrollbar_px(bar, &mut scroll, most, view, 24.0 * u);
        }
        PHONE_SCROLL.with(|c| c.set(scroll));
        // A row's screen top, and whether it is on the list at all.
        let on_list = |cy: f32, h: f32| cy + h > 0.0 && cy < view;
        let at = |cy: f32| list.y + cy - scroll;

        phone_clip(Some(list));
        for (i, (label, value)) in slider_rows.iter().enumerate() {
            let cy = slider_top + i as f32 * slider_pitch;
            if !on_list(cy - scroll, slider_h) {
                continue;
            }
            // The rows stop before the scroll bar (`bar_x`).
            let r = Rect::new(x0 - pad + 2.0 * u, at(cy), bar_x - 4.0 * list_k - (x0 - pad + 2.0 * u), slider_h);
            if let Some(v) = option_slider_stacked(r, label, *value, 14.0 * u) {
                match i {
                    0 | 1 => {
                        let steps = ((v - value) / super::audio::VOLUME_STEP).round() as i32;
                        if i == 0 {
                            audio.music_muted = false;
                            audio.step_music(steps);
                        } else {
                            audio.sfx_muted = false;
                            audio.step_sfx(steps);
                        }
                    }
                    2 => audio.scroll_speed = Some((v * 100.0).round()),
                    3 => audio.walk_speed = Some((v * razdor::rules::game::WALK_SPEED_MAX as f32).round()),
                    _ => audio.anim_speed = Some((v * 99.0).round()),
                }
            }
        }
        for (i, (key, _, on, ink)) in checks.iter().enumerate() {
            let cy = check_y[i];
            let h = check_h[i];
            if !on_list(cy - scroll, h) {
                continue;
            }
            if option_check_lines(x0, at(cy), cw, &check_lines[i], *on, *ink, u) {
                match *key {
                    "Option4" => audio.no_sound = Some(!on),
                    "Option5" => audio.show_fps = !on,
                    "Option6" => audio.hints_off = Some(!on),
                    "Option7" => audio.pursue = Some(!on),
                    "Option8" => audio.autosave = Some(!on),
                    "Option9" => toggle_expert_ai(audio),
                    "Option10" => audio.impossible = Some(!on),
                    "Option11" => audio.wide_row = Some(!on),
                    _ => audio.touchpad = Some(!on),
                }
            }
        }
        if orientation_row && on_list(orient_y - scroll, row_h) {
            let y = at(orient_y);
            chrome::shadow_text(tr("Screen orientation"), x0, y + 26.0 * u, 14.0 * u, chrome::CREAM);
            let r = Rect::new(inner.x + inner.w - pad - 150.0 * u, y + 4.0 * u, 150.0 * u, 36.0 * u);
            if row_button(r, orientation_label(audio.orientation)) {
                audio.orientation = match audio.orientation {
                    None => Some(super::audio::Orientation::Landscape),
                    Some(super::audio::Orientation::Landscape) => Some(super::audio::Orientation::Portrait),
                    Some(super::audio::Orientation::Portrait) => None,
                };
            }
        }
        if on_list(text_y - scroll, row_h) {
            let y = at(text_y);
            chrome::shadow_text(tr("Text size"), x0, y + 26.0 * u, 14.0 * u, chrome::CREAM);
            let r = Rect::new(inner.x + inner.w - pad - 150.0 * u, y + 4.0 * u, 150.0 * u, 36.0 * u);
            if row_button(r, super::display::text_size_label(audio.text_size)) {
                audio.text_size = super::display::next_text_size(audio.text_size);
            }
        }
        if on_list(large_y - scroll, row_h) {
            let y = at(large_y);
            chrome::shadow_text(tr("Large message windows"), x0, y + 26.0 * u, 14.0 * u, chrome::CREAM);
            let r = Rect::new(inner.x + inner.w - pad - 150.0 * u, y + 4.0 * u, 150.0 * u, 36.0 * u);
            if row_button(r, if audio.large_windows { tr("On") } else { tr("Off") }) {
                audio.large_windows = !audio.large_windows;
            }
        }
        if on_list(import_y - scroll, row_h) {
            super::game_import::settings_row(Rect::new(x0, at(import_y), cw, row_h), over_map);
        }
        phone_clip(None);

        // The buttons row: «Ещё…» at the left, «Принять» and «Отмена» at the right, 44 units high.
        let bh = 40.0 * u;
        let by = inner.y + inner.h - 12.0 * u - bh;
        let advanced = Rect::new(x0, by, 120.0 * u, bh);
        let cancel = Rect::new(inner.x + inner.w - pad - 110.0 * u, by, 110.0 * u, bh);
        let apply = Rect::new(cancel.x - 8.0 * u - 110.0 * u, by, 110.0 * u, bh);
        if row_button(advanced, tr("Advanced…")) {
            ADVANCED.with(|a| a.set(true));
            return false;
        }
        let (apply_label, cancel_label) = (own("Buttons", "Apply", n_("Apply")), own("Buttons", "Cancel", n_("Cancel")));
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
    })
}

pub fn options_window(audio: &mut super::audio::Settings, install_wide: bool, over_map: bool) -> bool {
    if ADVANCED.with(|a| a.get()) {
        // The keys window stands over the advanced one, which takes no input meanwhile.
        if KEYS_OPEN.with(|o| o.get()) {
            if keys_window(audio, over_map) {
                KEYS_OPEN.with(|o| o.set(false));
            }
            return false;
        }
        if advanced_window(audio, over_map) {
            ADVANCED.with(|a| a.set(false));
            // What the advanced window set is kept, whatever the settings window ends with; the
            // touchpad mode stays as the settings window opened it, until «Принять».
            OPENED_WITH.with(|o| {
                let mut o = o.borrow_mut();
                let mut kept = audio.clone();
                if let Some(before) = o.as_ref() {
                    kept.touchpad = before.touchpad;
                }
                *o = Some(kept);
            });
        }
        return false;
    }
    if OPENED_WITH.with(|o| o.borrow().is_none()) {
        OPENED_WITH.with(|o| *o.borrow_mut() = Some(audio.clone()));
    }
    // A phone held upright: its own layout (`phone_options`).
    if let Some(u) = upright_dp() {
        return phone_options(audio, install_wide, over_map, u);
    }
    let title = own("Options", "Title", n_("Sound, graphics and gameplay settings"));
    let h = options_h();
    // The rows below depend on the phone's own: see `options_h`.
    let touch_row = super::touchpad::MOBILE;
    let orientation_row = cfg!(target_os = "android");
    let g = super::display::button_grow();
    // In game on a phone, the window at its own scale (`map_window`, `window_layout::over_map_scale`); the title's is set by `options`.
    let _scale = over_map.then(|| window_layout::over_map_scale((594.0, h)));
    // Razdor's own: grown by the text size, the window may be taller than the room it has (above
    // the bar over the map); it is then drawn at the scale that fits it. Never at Normal.
    let _fit = (g > 1.0).then(|| {
        let room = if over_map { screen_height() - chrome::bar_height() } else { screen_height() };
        let need = (h + 30.0) * chrome::k();
        (need > room).then(|| super::display::enter_window_scale(Some(chrome::k() * room / need)))
    });
    let (inner, closed) = if over_map { map_window(&title, 594.0, h) } else { window(&title, 594.0, h) };
    let k = chrome::k();
    let outer = Rect::new(inner.x - 2.0 * k, inner.y - (26.0 * k).round() - 1.0, 594.0 * k, h * k);
    // A window with 0x48d710's inner frames (the options window is c2953a, 634×556, 4d2f48).
    chrome::window_inner_frames(outer, chrome::opx());
    let row = |i: usize| Rect::new(inner.x + 10.0 * k, inner.y + 8.0 * k + i as f32 * 37.5 * k, inner.w - 20.0 * k, 30.0 * k);
    // The Evolution Mod has four slider rows, labelled `Option0`…`Option3` (its exe builds the key
    // from `Option` and the row number); the Community's five are `OptionSld0`…`OptionSld4`. A
    // Community install names its rows the same way with `OptionSld`.
    let mod_rows = chrome::install_option("Option0").is_some() && chrome::install_option("OptionSld0").is_none();
    let slider_label = |n: usize, ours: &'static str| own("Options", &if mod_rows { format!("Option{n}") } else { format!("OptionSld{n}") }, ours);
    // The volumes, in steps of 1%.
    for (i, music) in [(0, true), (1, false)] {
        let (ours, value) = if music { (n_("Background music volume"), audio.music_volume) } else { (n_("Sound effects volume"), audio.sfx_volume) };
        if let Some(v) = option_slider(row(i), &slider_label(i, ours), value) {
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
    if let Some(v) = option_slider(row(2), &slider_label(2, n_("Map scroll speed")), scroll_speed(audio) / 100.0) {
        audio.scroll_speed = Some((v * 100.0).round());
    }
    // The slider runs 0..`WALK_SPEED_MAX`: the original's fastest (100) is its middle.
    let max = razdor::rules::game::WALK_SPEED_MAX as f32;
    if let Some(v) = option_slider(row(3), &slider_label(3, n_("Hero's walking speed")), walk_speed(audio) / max) {
        audio.walk_speed = Some((v * max).round());
    }
    // The Community's "Скорость анимаций в битве" (`AnimationSpeed`): it caps a strike's
    // slide at (100 − S)·5 ms and its effect at (100 − S)·3 ms (0x4afd45, 0x4afe7c). The
    // Evolution Mod's window has only four sliders (its `Option4` is a checkbox); Razdor shows
    // this row for every install, under its own label (the owner's decision, 2026-10-29).
    let anim_label = if mod_rows { tr(n_("Battle animation speed")).to_string() } else { slider_label(4, n_("Battle animation speed")) };
    if let Some(v) = option_slider(row(4), &anim_label, anim_speed(audio).unwrap_or(0.0) / 99.0) {
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
        let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, y, 170.0 * k, 28.0 * k * g);
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
    let step = option_row_step(g) * k;
    let text_y = last_y + step;
    chrome::shadow_text(tr("Text size"), cx, text_y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, text_y, 170.0 * k, 28.0 * k * g);
    if row_button(r, super::display::text_size_label(audio.text_size)) {
        audio.text_size = super::display::next_text_size(audio.text_size);
    }
    let large_y = text_y + step;
    chrome::shadow_text(tr("Large message windows"), cx, large_y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let r = Rect::new(inner.x + inner.w - 14.0 * k - 170.0 * k, large_y, 170.0 * k, 28.0 * k * g);
    if row_button(r, if audio.large_windows { tr("On") } else { tr("Off") }) {
        audio.large_windows = !audio.large_windows;
    }
    // «Обновить игру из архива…» (Razdor's, a row of its own under the large windows): the
    // button, or why it is not offered (`ui::game_import`). Over a map it is only said.
    let import_y = large_y + step;
    super::game_import::settings_row(Rect::new(inner.x + 14.0 * k, import_y, inner.w - 28.0 * k, 28.0 * k), over_map);
    // Razdor's own settings, in a window of their own.
    let bh = 32.0 * g;
    let button_y = outer.y + (h - 10.0 - bh) * k;
    let xr = Rect::new(inner.x + 14.0 * k, button_y, 120.0 * k * g, bh * k);
    if row_button(xr, tr("Advanced…")) {
        ADVANCED.with(|a| a.set(true));
        return false;
    }
    // «Принять» keeps what was changed, «Отмена» (the close box and Esc too) puts it back.
    let cancel = Rect::new(inner.x + inner.w - 14.0 * k - 89.0 * k * g, button_y, 89.0 * k * g, bh * k);
    let apply = Rect::new(cancel.x - 8.0 * k - 89.0 * k * g, cancel.y, 89.0 * k * g, bh * k);
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

/// Closes the in-game settings window with its changes kept, as «Принять» does (the bar's
/// settings button, a second press: the owner's request). The advanced and keys windows close
/// with it; the settings already set in `audio` stay.
pub fn close_options_kept() {
    ADVANCED.with(|a| a.set(false));
    KEYS_OPEN.with(|o| o.set(false));
    OPENED_WITH.with(|o| *o.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bar's settings button closes the window with what was changed kept: the state the
    /// window opened with is dropped (no revert), and the sub-windows close with it.
    #[test]
    fn closing_from_the_bar_keeps_the_changes() {
        OPENED_WITH.with(|o| *o.borrow_mut() = Some(super::super::audio::Settings::default()));
        ADVANCED.with(|a| a.set(true));
        KEYS_OPEN.with(|o| o.set(true));
        close_options_kept();
        assert!(OPENED_WITH.with(|o| o.borrow().is_none()));
        assert!(!ADVANCED.with(|a| a.get()) && !KEYS_OPEN.with(|o| o.get()));
    }

    /// A phone of 1080×2376 px at 2.625 px per dp, its interface scale the fit (1.125).
    /// The device the owner's screenshot came from (OnePlus): 1080×2376 at 2.75 px per dp.
    const PHONE: (f32, f32, f32, f32) = (1080.0, 2376.0, 1.125, 2.75);

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn oval_pieces_tile_the_picture_with_no_gap() {
        // `mb2.ugs` is 228 × 251 texels; the two pieces cover it exactly.
        let (w, h) = (228.0, 251.0);
        for s in [0.9375, 0.9375 * 1.125, 0.9375 * PORTRAIT_SCALE * 1.125] {
            let c = vec2(540.0, 900.0);
            let (rs, _) = oval_picture(c, s, w, h, false);
            let (ps, _) = oval_picture(c, s, w, h, true);
            assert!(close(rs.y, 0.0) && close(rs.y + rs.h, ps.y) && close(ps.y + ps.h, h), "{rs:?} {ps:?}");
            // The round piece's dst is its texels at `s`, with no stray rows of the pointed one.
            let (_, rd) = oval_picture(c, s, w, h, false);
            assert!(close(rd.h, MB2_SPLIT * s), "{rd:?}");
            // The pointed piece's dst maps its texels 1:1 at `s` and its body keeps the old place:
            // the texel row the half cut used (h / 2) still lands 46 texels above `c`'s row offset.
            let (_, pd) = oval_picture(c, s, w, h, true);
            assert!(close(pd.h, (h - MB2_SPLIT) * s), "{pd:?}");
            assert!(close(pd.y + (h / 2.0 - MB2_SPLIT) * s, c.y - 46.0 * s), "{pd:?}");
        }
    }

    #[test]
    fn upright_buttons_are_a_column_of_touch_sized_ovals_above_the_chips() {
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp, 0.0, false);
        for r in &m.buttons {
            assert!(close(r.h, OVAL.y * PORTRAIT_SCALE * k), "{r:?}");
            assert!(close(r.w, OVAL.x * PORTRAIT_SCALE * k), "{r:?}");
            assert!(r.h >= TOUCH_DP * dp, "{r:?} is under 48 dp");
            assert!(r.x >= 0.0 && r.x + r.w <= sw, "{r:?} leaves the screen");
        }
        for pair in m.buttons.windows(2) {
            assert!(pair[0].y + pair[0].h <= pair[1].y, "{pair:?} overlap");
        }
        // Above the version and notice lines.
        let last = m.buttons[4];
        assert!(last.y + last.h <= m.notice_y(k) - 16.0 * k, "{last:?} runs into the lines");
        // The not-found block sits above the logo, clear of the column.
        assert!(m.block_bottom(k) < m.logo_top && m.block_bottom(k) > 0.0, "block over the logo");
    }

    #[test]
    fn upright_column_is_centred_between_the_top_and_the_lines() {
        // 1080×2376 and 1080×1920 (the owner's phone and a shorter one): the column's middle is
        // the middle of the area above the foot lines, within 4 px.
        for (sh, dp) in [(2376.0, 2.75), (1920.0, 2.75)] {
            let m = portrait_menu(1080.0, sh, 1.125, dp, 0.0, false);
            let first = m.buttons[0];
            let last = m.buttons[4];
            let centre = (first.y + last.y + last.h) / 2.0;
            // The column keeps the place of the old, shorter link row (`portrait_menu`'s `column_foot`).
            let column_foot = sh - 16.0 * 1.125 - TOUCH_DP * dp - 12.0 * 1.125;
            assert!((centre - column_foot / 2.0).abs() <= 4.0, "{sh}: column centre {centre}, target {}", column_foot / 2.0);
            // The logo's bottom is 24 px above the first button and its top is below the inset.
            let logo_bottom = m.logo_top + 840.0 * 128.0 / 896.0 * 1.125;
            assert!(close(logo_bottom + 24.0 * 1.125, first.y), "logo {} over the column", m.logo_top);
            assert!(m.logo_top > 0.0);
        }
    }

    #[test]
    fn upright_logo_never_goes_above_the_safe_inset() {
        // A cutout of 120 px: the logo's top is at the inset, and the column moves down to suit.
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp, 120.0, false);
        assert!(m.logo_top >= 120.0, "logo at {} is above the inset", m.logo_top);
        assert!(m.buttons[0].y >= m.logo_top + 840.0 * 128.0 / 896.0 * k);
        // And with the logo on the inset the column still ends above the lines.
        let last = m.buttons[4];
        assert!(last.y + last.h <= m.notice_y(k) - 16.0 * k, "{last:?} runs into the lines");
    }

    #[test]
    fn upright_chips_are_touch_sized_and_on_the_bottom_of_the_screen() {
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp, 0.0, false);
        assert!(m.chip_h >= TOUCH_DP * dp, "chips are {} px tall", m.chip_h);
        assert!(m.chip_top + m.chip_h <= sh, "chips leave the screen");
        assert!(m.foot_y <= m.chip_top, "the lines run into the chips");
        assert!(m.version_y(k) < m.foot_y && m.notice_y(k) < m.version_y(k));
    }

    #[test]
    fn a_mod_row_sits_above_the_four_tiles_and_lifts_the_foot_lines() {
        let (sw, sh, k, dp) = PHONE;
        let plain = portrait_menu(sw, sh, k, dp, 0.0, false);
        let with = portrait_menu(sw, sh, k, dp, 0.0, true);
        assert_eq!(plain.mod_row, None, "no row without a mod");
        assert!(close(with.chip_top, plain.chip_top), "the four tiles stay at the bottom");
        let row = with.mod_row.expect("a row with a mod installed");
        assert!(row.h >= TOUCH_DP * dp, "the row is {} px tall", row.h);
        assert!(close(row.y + row.h + CHIP_GAP_DP * dp, with.chip_top), "one gap between the row and the tiles");
        assert!(close(row.x, CHIP_MARGIN_DP * dp) && close(row.w, sw - 2.0 * CHIP_MARGIN_DP * dp), "the row is full width");
        assert!(close(with.foot_y, row.y - 12.0 * k), "the foot lines sit above the row");
        assert!(close(with.foot_y, plain.foot_y - row.h - CHIP_GAP_DP * dp), "the foot lines lift by the row and its gap");
    }

    #[test]
    fn the_buttons_end_above_the_foot_lines_with_a_mod_row() {
        for (sw, sh, k, dp) in [PHONE, (720.0, 1600.0, 0.75, 2.0)] {
            let m = portrait_menu(sw, sh, k, dp, 0.0, true);
            let last = m.buttons[4];
            assert!(last.y + last.h <= m.notice_y(k), "{sh}: the last button runs into the foot lines");
            assert!(m.logo_top >= 0.0, "{sh}: the logo leaves the screen");
        }
    }

    #[test]
    fn upright_geometry_fits_a_small_phone_too() {
        // 720×1600 at 2 px per dp: the fit is 0.75.
        let m = portrait_menu(720.0, 1600.0, 0.75, 2.0, 0.0, false);
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

    #[test]
    fn the_touchpad_mode_takes_effect_on_apply_not_when_ticked() {
        // Window open: the value it opened with, whatever the box says now.
        assert_eq!(touchpad_in_effect(Some(true), Some(None)), None);
        assert_eq!(touchpad_in_effect(Some(true), Some(Some(false))), Some(false));
        assert_eq!(touchpad_in_effect(None, Some(Some(true))), Some(true));
        // Window closed: the live setting.
        assert_eq!(touchpad_in_effect(Some(true), None), Some(true));
        assert_eq!(touchpad_in_effect(Some(false), None), Some(false));
        assert_eq!(touchpad_in_effect(None, None), None);
    }

    /// The advanced window's right column in units of k: the inner height of the 650-high window
    /// (its 29 px strip and frame off), the Ok button's top 48 px above the bottom, four lines of
    /// the battle AI hint and four of the language's. The pitch keeps the column above the Ok
    /// button, and the original's 44 px when the window has the room.
    #[test]
    fn advanced_right_column_fits_above_ok() {
        let (top, inner_h, ok_top) = (30.0, 650.0 - 29.0, 650.0 - 29.0 - 48.0);
        let hints = 44.0 + 4.0 * 17.0 + 48.0 + 4.0 * 17.0;
        let p = right_column_pitch(top, ok_top, 9.0, hints, 44.0);
        assert!((32.0..44.0).contains(&p), "pitch {p}");
        assert!(top + 9.0 * p + hints <= ok_top + 1e-3, "the language description ends above Ok");
        assert_eq!(right_column_pitch(top, inner_h + 200.0, 9.0, hints, 44.0), 44.0);
    }

    /// On a phone the advanced window scrolls by what its content runs past the view above Ok;
    /// content that fits does not scroll at all (the desktop's case).
    #[test]
    fn the_advanced_window_scrolls_by_what_runs_past_its_view() {
        assert_eq!(scroll_most(700.0, 573.0), 127.0);
        assert_eq!(scroll_most(500.0, 573.0), 0.0);
        assert_eq!(scroll_most(573.0, 573.0), 0.0);
    }
}
