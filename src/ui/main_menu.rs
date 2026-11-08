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

/// Opens the folder `dir` in the system's file manager (Razdor's own, a player's request: the
/// Files app on Android and iOS). True when it was handed to the system.
pub fn open_folder(dir: &std::path::Path) -> bool {
    let text = dir.to_string_lossy();
    #[cfg(target_os = "android")]
    return super::android::open_folder(&text);
    #[cfg(target_os = "ios")]
    return super::mobile::open_folder(&text);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    razdor::update::open_in_browser(&text)
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
    let y = foot_y(k);
    let ink = Color::new(0.92, 0.92, 0.92, 1.0);
    let Some(dp) = super::display::dp_px() else {
        // The desktop's: 11 × k, the copyright at 264 × k.
        if let Some(v) = chrome::ui_text("SpecialTexts", "VerInfo") {
            chrome::shadow_text(&v, 5.0 * k, y, 11.0 * k, ink);
        }
        if let Some(c) = chrome::ui_text("SpecialTexts", "CopyrightsMessage") {
            chrome::shadow_text(&c, 264.0 * k, y, 11.0 * k, ink);
        }
        return;
    };
    // On a phone the lines are the version label's size (13 dp, a player's report: 11 × k was
    // unreadable). The version is the top line and the copyright follows it, or, when the two are
    // wider than the screen, the copyright's lines come under the version. Held sideways the
    // bottom-left corner's rounding can reach the lines: they start past it (`foot_x0`).
    let x0 = foot_x0(k, y, VERSION_PHONE_DP * dp, screen_height(), &super::display::safe_area());
    let f = foot(k, dp, screen_width(), x0);
    let pitch = f.pitch();
    let n = if f.stacked { f.copyright.len() as f32 } else { 0.0 };
    super::dt_font::legacy_metrics_if(true, || {
        if let Some(v) = &f.version {
            chrome::shadow_text(v, x0, y - n * pitch, f.size, ink);
        }
        let last = f.copyright.len() as f32 - 1.0;
        for (i, line) in f.copyright.iter().enumerate() {
            let base = y - (last - i as f32) * pitch;
            chrome::shadow_text(line, f.copy_x, base, f.size, ink);
        }
    });
}

/// The baseline of the foot's bottom row: just above the chips held upright, else 4 × k above the bottom.
fn foot_y(k: f32) -> f32 {
    upright_menu().map_or(screen_height() - 4.0 * k, |m| m.foot_y)
}

/// The horizontal inset (px) of a circle of `radius` at `depth` px above its bottom edge: how far a
/// rounded bottom corner cuts into a row of text that high. 0 at `depth` >= `radius`, `radius` at 0.
fn corner_clearance(radius: f32, depth: f32) -> f32 {
    let depth = depth.max(0.0);
    if radius <= 0.0 || depth >= radius {
        return 0.0;
    }
    let d = radius - depth;
    radius - (radius * radius - d * d).max(0.0).sqrt()
}

/// Where the phone's foot text starts from the left (px): 5 × k, or, when the bottom-left corner's
/// rounding reaches the lines, past it (`corner_clearance`) and 4 × k more. `y` is the bottom row's
/// baseline and `size` its text size on a `sh`-high screen; `safe` is the screen's safe area.
fn foot_x0(k: f32, y: f32, size: f32, sh: f32, safe: &super::display::SafeArea) -> f32 {
    // The lowest glyph reaches a quarter of the size below the baseline.
    let depth = (sh - (y + 0.25 * size)).max(0.0);
    (5.0 * k).max(safe.left + corner_clearance(safe.corner_bl, depth) + 4.0 * k)
}

/// The phone's foot text: the version (`VerInfo`) and the copyright (`CopyrightsMessage`).
struct Foot {
    size: f32,
    version: Option<String>,
    /// The copyright's lines: one, after the version, or wrapped under it when stacked.
    copyright: Vec<String>,
    /// Where the one-row copyright starts (after the version and a 24 × k gap); `x0` when stacked.
    copy_x: f32,
    stacked: bool,
}

impl Foot {
    /// The baseline distance of the foot's lines.
    fn pitch(&self) -> f32 {
        self.size * 1.2
    }

    /// The height the lines take above the bottom row (0 when the copyright stays on it).
    fn above(&self) -> f32 {
        if self.stacked { self.copyright.len() as f32 * self.pitch() } else { 0.0 }
    }
}

/// The phone's foot text for a `sw`-wide screen at `k` and `dp`, its lines starting at `x0` (see
/// `foot_x0`): one row when the version, a gap and the copyright fit between `x0` and the screen's
/// right side (10 × k of margin), else the copyright wrapped under the version.
fn foot(k: f32, dp: f32, sw: f32, x0: f32) -> Foot {
    let size = VERSION_PHONE_DP * dp;
    super::dt_font::legacy_metrics_if(true, || {
        let version = chrome::ui_text("SpecialTexts", "VerInfo");
        let copy = chrome::ui_text("SpecialTexts", "CopyrightsMessage");
        let vw = version.as_ref().map_or(0.0, |v| measure(v, size).width);
        let gap = if version.is_some() { 24.0 * k } else { 0.0 };
        let (copyright, stacked) = match &copy {
            None => (Vec::new(), false),
            Some(c) => {
                let cw = measure(c, size).width;
                if x0 + vw + gap + cw <= sw - 10.0 * k {
                    (vec![c.clone()], false)
                } else {
                    (wrap_label(c, sw - 10.0 * k - x0, size), true)
                }
            }
        };
        let copy_x = if stacked { x0 } else { x0 + vw + gap };
        Foot { size, version, copyright, copy_x, stacked }
    })
}

/// How much taller the phone's foot text is than the desktop's 11 × k, held sideways (0 elsewhere):
/// the corner links and the version line above them lift by it, so the text stays clear of them.
fn sideways_foot_extra(k: f32) -> f32 {
    match super::display::dp_px() {
        Some(dp) if !super::window_layout::portrait() => {
            let x0 = foot_x0(k, foot_y(k), VERSION_PHONE_DP * dp, screen_height(), &super::display::safe_area());
            let f = foot(k, dp, screen_width(), x0);
            (f.size - 11.0 * k).max(0.0) + f.above()
        }
        _ => 0.0,
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
    /// The baseline of the foot lines' bottom row (the version and copyright), just above the chips.
    foot_y: f32,
    /// The height the foot's lines take above `foot_y` (a copyright too wide for the screen goes on
    /// lines of its own above the bottom row); the version and notice lines sit above them.
    foot_above: f32,
    /// The top of the logo: the space above it is free for the not-found block.
    logo_top: f32,
    /// The chosen mod's full-width tile, above the four tiles, when a mod is installed (its height
    /// and gap lift the foot lines and the button column; see [`portrait_menu`]).
    mod_row: Option<Rect>,
}

impl PortraitMenu {
    /// The baseline of the version line (above the foot lines).
    fn version_y(&self, k: f32) -> f32 {
        self.foot_y - 24.0 * k - self.foot_above
    }

    /// The baseline of an installed update's notice (above the version line).
    fn notice_y(&self, k: f32) -> f32 {
        self.foot_y - 48.0 * k - self.foot_above
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
fn portrait_menu(sw: f32, sh: f32, k: f32, dp: f32, safe_top: f32, mod_row: bool, reserve: f32) -> PortraitMenu {
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
    // `reserve`: the space above the logo that the not-found block takes (zero without it).
    let min_first = safe_top + reserve + logo_h + 24.0 * k;
    let gap = ((column_end - min_first - 5.0 * size.y) / 4.0).clamp(6.0 * k, 20.0 * k);
    let span = 5.0 * size.y + 4.0 * gap;
    let first = (middle - span / 2.0).clamp(min_first, (column_end - span).max(min_first));
    let logo_top = first - 24.0 * k - logo_h;
    let x = sw / 2.0 - size.x / 2.0;
    let buttons = std::array::from_fn(|i| Rect::new(x, first + i as f32 * (size.y + gap), size.x, size.y));
    let safe = super::display::safe_area();
    let foot_x = foot_x0(k, foot_y, VERSION_PHONE_DP * dp, sh, &safe);
    let foot_above = foot(k, dp, sw, foot_x).above();
    PortraitMenu { buttons, chip_top, chip_h, foot_y, foot_above, logo_top, mod_row: row }
}

local! {
    /// Whether a mod is installed, as the title menu last drew it (`frame`): the chosen mod's row
    /// is held above the tiles, and the title windows keep above it too (`portrait_foot_top`).
    static MOD_ROW: Cell<bool> = const { Cell::new(false) };
    /// The space the not-found block takes above the upright menu's logo, as this frame draws it
    /// (`screens::upright_reserve`; zero when an install is found).
    static BLOCK_RESERVE: Cell<f32> = const { Cell::new(0.0) };
}

/// The top of the footer lines (the version and copyright row) of the portrait menu on a `sw` ×
/// `sh` screen at scale `k`: the title windows keep above it (`window_layout::portrait_title`).
pub fn portrait_foot_top(sw: f32, sh: f32, k: f32, dp: f32, safe_top: f32) -> f32 {
    portrait_menu(sw, sh, k, dp, safe_top, MOD_ROW.with(Cell::get), BLOCK_RESERVE.with(Cell::get)).foot_y
}

/// The current menu's portrait geometry, when the phone is held upright.
fn upright_menu() -> Option<PortraitMenu> {
    let dp = upright_dp()?;
    let safe_top = super::display::safe_area().top;
    Some(portrait_menu(screen_width(), screen_height(), chrome::k(), dp, safe_top, MOD_ROW.with(Cell::get), BLOCK_RESERVE.with(Cell::get)))
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

/// The tallest the not-found block may be on an upright phone, in px: the menu keeps its logo and
/// its five ovals at their smallest gaps above the bottom bar (`portrait_menu`), and the block takes
/// the rest from the safe area. Unbounded when the phone is not held upright.
pub fn upright_block_budget(k: f32) -> f32 {
    let Some(dp) = upright_dp() else {
        return f32::MAX;
    };
    let g = super::display::button_grow();
    let size = OVAL * (PORTRAIT_SCALE * k);
    let lift = if MOD_ROW.with(Cell::get) { TILE_DP * dp + CHIP_GAP_DP * dp } else { 0.0 };
    let column_foot = screen_height() - 16.0 * k - (TOUCH_DP * dp).max(24.0 * k) * g - 12.0 * k - lift;
    let column_end = column_foot - 70.0 * k;
    let logo_h = 840.0 * 128.0 / 896.0 * k;
    let span_min = 5.0 * size.y + 4.0 * 6.0 * k;
    column_end - span_min - logo_h - 24.0 * k - 16.0 * k - 8.0 * k - super::display::safe_area().top
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
    BLOCK_RESERVE.with(|r| r.set(if no_install { super::screens::upright_reserve(chrome::k()) } else { 0.0 }));
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
            screen_height() - 22.0 * k - 26.0 * k - sideways_foot_extra(k)
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
/// battle, the maps and mods, the Discord and the language. (The map editor's link is gone: the
/// editor is not ready; its code and `--editor` stay.) Returns what was picked.
fn corner_links(k: f32, mods: &[(String, razdor::hub::Text)], mod_choice: Option<&super::new_game::ModChoice>) -> Option<Pick> {
    let mut pick = None;
    let y = screen_height() - 22.0 * k - sideways_foot_extra(k);
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
fn map_window(title: &str, w: f32, h: f32, shrink: f32) -> (Rect, bool) {
    let k = chrome::k();
    let r = window_layout::over_map_rect((w, h)).map(|r| shrink_about_centre(r, shrink)).unwrap_or_else(|| {
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
    audio.expert_ai.unwrap_or_else(install_expert_ai)
}

/// The install's improved enemy AI in battle (`OptValue9`).
fn install_expert_ai() -> bool {
    chrome::ui_text("Options", "OptValue9").is_some_and(|v| razdor::dt::ini::loose_int(&v) == 1)
}

/// The original's own values: the install's `[Options]`, never the player's choices. They are
/// what a gameplay setting is compared with for the run's mark
/// (`audio::Settings::gameplay_differs_from_original`).
pub fn install_originals() -> super::audio::Originals {
    super::audio::Originals {
        improved_ai: install_expert_ai(),
        wide_row: chrome::install_option("OptValue11").is_none_or(|v| razdor::dt::ini::loose_int(&v) == 1),
        impossible: install_on("OptValue10", false),
        pursue: install_on("OptValue7", true),
    }
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
    g.keep_xp_on_promotion = audio.keep_xp_on_promotion;
    g.keep_formation = audio.keep_formation;
    g.allow_unit_benching = audio.allow_unit_benching;
    g.battle_marks = audio.battle_marks;
    g.army_list = audio.army_list;
    g.leader_caption = audio.leader_caption;
    // The run's mark follows what it uses now: a gameplay setting that is not the original's
    // (`Game::note_settings`; the rules and the cheats are read from the game itself).
    g.note_settings(audio.gameplay_differs_from_original(&install_originals()));
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
    super::gestures::set_twist(audio.map_twist);
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

/// The settings window's height in video px: the original's rows (ending 425 px down), the
/// group buttons' row under them (`desktop_group_block`: one row, two when the text is grown), 12 px
/// to its top, then Apply and Cancel with their margin (the window's inner top is 27 px down).
fn options_h() -> f32 {
    let g = super::display::button_grow();
    484.0 + desktop_group_block(g) + 32.0 * g
}

/// The widths of a settings window's bottom buttons in `avail` px, with `gap` between them: as
/// given when they and the gaps fit, else all scaled down together so they do (their labels then
/// fit in the narrower buttons, not over each other).
fn bottom_row_widths(avail: f32, widths: &[f32], gap: f32) -> Vec<f32> {
    let total: f32 = widths.iter().sum();
    let gaps = (widths.len() as f32 - 1.0) * gap;
    if total + gaps <= avail {
        return widths.to_vec();
    }
    let f = ((avail - gaps) / total).max(0.0);
    widths.iter().map(|w| w * f).collect()
}

local! {
    /// The settings group window open over the settings (`None`: the settings window alone).
    static GROUP: std::cell::Cell<Option<Group>> = const { std::cell::Cell::new(None) };
    /// The settings as the window found them: «Отмена» puts them back (the original's
    /// Cancel drops what was changed, its Apply keeps it).
    static OPENED_WITH: std::cell::RefCell<Option<super::audio::Settings>> = const { std::cell::RefCell::new(None) };
}

local! {
    /// The keys window («Клавиши») is open over the advanced settings.
    static KEYS_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The F1 list's «Изменить клавиши…» was pressed (`take_keys_request` takes it).
    static KEYS_ASKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The keys window was opened from the F1 list: closing it closes the settings too, back to the list.
    static KEYS_FROM_HELP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The settings closed from the keys window opened from the F1 list: the list reopens over the map
    /// (`take_back_to_help`, read by the map's frame).
    static BACK_TO_HELP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// A snapshot scene's scroll (`RAZDOR_SCENE_SCROLL`): the phone's options list starts `px` down
/// (clamped to its end when drawn).
pub fn scene_scroll(px: f32) {
    PHONE_SCROLL.with(|c| c.set(px));
    // The phone's advanced window starts that far down too (clamped to its content).
    ADV_SCROLL.with(|s| s.set(px));
}

/// Opens a settings group's window over the settings window (the snapshot scenes, the keys row).
pub fn open_group(group: Group) {
    GROUP.with(|g| g.set(Some(group)));
    ADV_SCROLL.with(|s| s.set(0.0));
}

/// Opens the keys window over the advanced settings (a snapshot scene, and the keys row).
pub fn open_keys() {
    open_group(Group::Controls);
    KEYS_OPEN.with(|o| o.set(true));
    KEYS_FROM_HELP.with(|f| f.set(false));
}

/// Opens the keys window for the F1 list's «Изменить клавиши…»: as `open_keys`, but closing it
/// returns to the list over the map (`take_back_to_help`), not to the advanced settings.
pub fn open_keys_from_help() {
    open_keys();
    KEYS_FROM_HELP.with(|f| f.set(true));
}

/// True once after the keys window opened from the F1 list closed the settings (`open_keys_from_help`).
pub fn take_back_to_help() -> bool {
    BACK_TO_HELP.with(|b| b.replace(false))
}

/// The keys window has closed (its own «Принять», «Отмена» or Esc). From the F1 list the whole
/// settings close with the changes kept, back to the list; otherwise the advanced settings stay.
/// True when the settings close.
fn after_keys_closed() -> bool {
    KEYS_OPEN.with(|o| o.set(false));
    if !KEYS_FROM_HELP.with(|f| f.replace(false)) {
        return false;
    }
    GROUP.with(|g| g.set(None));
    OPENED_WITH.with(|o| *o.borrow_mut() = None);
    BACK_TO_HELP.with(|b| b.set(true));
    true
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

/// The settings groups under the original's settings window (the owner's design, 2026-11-08,
/// backlog 1559): each button opens its own window with that group's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Map,
    Battle,
    Screen,
    Controls,
    Other,
}

impl Group {
    /// The groups, in the settings window's order.
    pub const ALL: [Group; 5] = [Group::Map, Group::Battle, Group::Screen, Group::Controls, Group::Other];

    /// The group's button in the settings window.
    fn button(self) -> &'static str {
        match self {
            Group::Map => tr("Map…"),
            Group::Battle => tr("Battle and army…"),
            Group::Screen => tr("Screen…"),
            Group::Controls => tr("Controls…"),
            Group::Other => tr("Other…"),
        }
    }

    /// The group's window title.
    fn title(self) -> &'static str {
        match self {
            Group::Map => tr("Map"),
            Group::Battle => tr("Battle and army"),
            Group::Screen => tr("Screen"),
            Group::Controls => tr("Controls"),
            Group::Other => tr("Other"),
        }
    }

    /// The group named in a snapshot scene (`options:<group>`).
    pub fn from_name(name: &str) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.name() == name)
    }

    /// The group's name in the snapshot scenes.
    fn name(self) -> &'static str {
        match self {
            Group::Map => "map",
            Group::Battle => "battle",
            Group::Screen => "screen",
            Group::Controls => "controls",
            Group::Other => "other",
        }
    }

    /// The rows the group shows, top to bottom. Every row is in exactly one group (a test checks it).
    pub fn rows(self) -> &'static [Row] {
        match self {
            Group::Map => &[Row::MapZoom, Row::MapView, Row::SpriteSize, Row::TwoFingerTurn, Row::MapEdge, Row::MapBattles, Row::ArmyList, Row::LeaderName, Row::MinimapCorner, Row::MinimapReset],
            Group::Battle => &[Row::AiStrength, Row::UnitBattle, Row::KeepFormation, Row::MaxGuard, Row::ArmyArrangement, Row::Experience],
            Group::Screen => &[Row::Screen, Row::InterfaceScale, Row::BottomBar, Row::Font, Row::TextSize, Row::LargeWindows, Row::Orientation, Row::Language],
            Group::Controls => &[Row::Keys, Row::Touchpad],
            Group::Other => &[Row::Updates, Row::Import],
        }
    }

    /// The window's height on the desktop, in video px (a phone's fills the room; both scroll).
    fn height(self) -> f32 {
        match self {
            Group::Map => 540.0,
            Group::Battle => 440.0,
            Group::Screen => 370.0,
            Group::Controls => 200.0,
            Group::Other => 250.0,
        }
    }
}

/// One setting's row in a group window (its label, its button or slider, its hint).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    MapZoom,
    MapView,
    SpriteSize,
    TwoFingerTurn,
    MapEdge,
    MapBattles,
    ArmyList,
    LeaderName,
    MinimapCorner,
    MinimapReset,
    AiStrength,
    UnitBattle,
    KeepFormation,
    MaxGuard,
    ArmyArrangement,
    Experience,
    Screen,
    InterfaceScale,
    BottomBar,
    Font,
    TextSize,
    LargeWindows,
    Orientation,
    Language,
    Keys,
    Touchpad,
    Updates,
    Import,
}

impl Row {
    /// Every row (the group table's test checks each is in one group).
    #[cfg(test)]
    pub const ALL: [Row; 28] = [
        Row::MapZoom,
        Row::MapView,
        Row::SpriteSize,
        Row::TwoFingerTurn,
        Row::MapEdge,
        Row::MapBattles,
        Row::ArmyList,
        Row::LeaderName,
        Row::MinimapCorner,
        Row::MinimapReset,
        Row::AiStrength,
        Row::UnitBattle,
        Row::KeepFormation,
        Row::MaxGuard,
        Row::ArmyArrangement,
        Row::Experience,
        Row::Screen,
        Row::InterfaceScale,
        Row::BottomBar,
        Row::Font,
        Row::TextSize,
        Row::LargeWindows,
        Row::Orientation,
        Row::Language,
        Row::Keys,
        Row::Touchpad,
        Row::Updates,
        Row::Import,
    ];

    /// Whether the row is drawn now: the 2.5D rows only with the 2.5D view on, the screen
    /// orientation on Android, the touchpad mode where it exists.
    fn shown(self, audio: &super::audio::Settings) -> bool {
        match self {
            Row::SpriteSize | Row::TwoFingerTurn => audio.map_tilt,
            Row::Orientation => cfg!(target_os = "android"),
            // The bottom bar's size is a phone's (Razdor's; the desktop has no such row).
            Row::BottomBar => super::display::dp_px().is_some(),
            Row::Touchpad => super::touchpad::MOBILE,
            _ => true,
        }
    }
}

/// The group window's width on the desktop, in video px (the settings window's 594 and a margin).
const GROUP_W: f32 = 600.0;

/// The label size of the group buttons and the settings window's button rows (video px or dp).
const GROUP_LABEL: f32 = 14.0;

/// The width of the group buttons' area in the settings window, video px (its 594 less the margins).
const GROUP_AREA_W: f32 = 538.0;

/// The width in px of `label` drawn at `size` px, as a button draws it.
fn label_px(label: &str, size: f32) -> f32 {
    super::typography::Role::Button.with(|| super::dt_font::legacy_metrics_if(true, || measure(label, size).width))
}

/// The buttons per row of a group of buttons: as many as the widest label fits, five, then three,
/// else two. `widths` and `avail` are in one unit; `pad` is the room a label needs on each side.
fn group_per_row(widths: &[f32], avail: f32, gap: f32, pad: f32) -> usize {
    let widest = widths.iter().cloned().fold(0.0_f32, f32::max);
    for n in [5, 3] {
        if widest + 2.0 * pad <= (avail - (n as f32 - 1.0) * gap) / n as f32 {
            return n;
        }
    }
    2
}

/// The rectangles of the group buttons in `area`, one per width in `widths`: `group_per_row` to a
/// row, the rows `gap` apart, each `row_h` high. The buttons of a row share its width.
fn group_rects(area: Rect, widths: &[f32], pad: f32, gap: f32, row_h: f32) -> Vec<Rect> {
    let per = group_per_row(widths, area.w, gap, pad);
    let bw = (area.w - (per as f32 - 1.0) * gap) / per as f32;
    (0..widths.len())
        .map(|i| {
            let (row, col) = (i / per, i % per);
            Rect::new(area.x + col as f32 * (bw + gap), area.y + row as f32 * (row_h + gap), bw, row_h)
        })
        .collect()
}

/// The widths of the group labels at `size` px, in Group::ALL's order.
fn group_label_widths(size: f32) -> Vec<f32> {
    Group::ALL.iter().map(|g| label_px(g.button(), size)).collect()
}

/// The height in video px of the desktop settings window's group buttons at the text size `g`
/// (the rows `group_rects` makes, with the labels measured as the window draws them).
fn desktop_group_block(g: f32) -> f32 {
    let k0 = chrome::k();
    // The labels measured in px at their size, then taken back to the window's units.
    let widths: Vec<f32> = Group::ALL.iter().map(|grp| label_px(grp.button(), GROUP_LABEL * g * k0) / k0).collect();
    let rows = (Group::ALL.len() as f32 / group_per_row(&widths, GROUP_AREA_W, 6.0, 14.0) as f32).ceil();
    rows * 28.0 * g + (rows - 1.0) * 6.0
}

/// A row's layout in a group window (px at its scale): the desktop's and the phone's.
struct RowGeo {
    k: f32,
    /// A button's height.
    row_h: f32,
    /// The gap under a row.
    gap: f32,
    /// A row's label size.
    size: f32,
    /// The line pitch of a wrapped label or hint.
    lh: f32,
    /// A hint's size.
    hint: f32,
    /// The narrowest value button.
    min_w: f32,
    /// The buttons' label size (the phone's); None: the marble's own (the desktop's).
    btn_size: Option<f32>,
}

impl RowGeo {
    /// The desktop's: the original's 28 px rows and 14 px labels, the hints at 12.
    fn desktop(k: f32, g: f32) -> RowGeo {
        RowGeo { k, row_h: 28.0 * k * g, gap: 8.0 * k, size: 14.0 * k, lh: 17.0 * k, hint: 12.0 * k, min_w: 170.0 * k, btn_size: None }
    }

    /// The phone's, in dp (`k` px per dp): 36 dp rows grown by the text size, never under 40 dp;
    /// 16.5 sp labels and hints.
    fn phone(k: f32, g: f32) -> RowGeo {
        RowGeo { k, row_h: (36.0 * g).max(40.0) * k, gap: 8.0 * k, size: 16.5 * k, lh: 20.0 * k, hint: 16.5 * k, min_w: 120.0 * k, btn_size: Some(16.5 * k) }
    }
}

/// A column of a group window: its left, width and the next row's top (px).
struct RowCol {
    x: f32,
    w: f32,
    y: f32,
}

/// A button's drawing and its press (`widgets::button_state`); `size` is its label's size (None:
/// the marble's own).
fn button_draw(r: Rect, label: &str, size: Option<f32>) -> bool {
    let (_, down, pressed) = super::widgets::button_state(r, true);
    match size {
        Some(s) => chrome::marble_button_sized(r, label, true, down, s),
        None => chrome::marble_button(r, label, true, down),
    }
    if pressed {
        cue(Cue::Button);
    }
    pressed
}

/// A button in a group window's scroll view: drawn where it shows, clicked only where it shows.
fn scroll_button(r: Rect, label: &str, size: Option<f32>) -> bool {
    if let Some(view) = ADV_VIEW.with(|v| v.get()) {
        if !overlaps(r, view) {
            return false;
        }
        let pressed = button_draw(r, label, size);
        let (mx, my) = crate::ui::widgets::pointer();
        return pressed && in_rect((mx, my), view);
    }
    button_draw(r, label, size)
}

/// A row: its label on the left (wrapped to the room beside the button, or on its own line above
/// the button when a word does not fit) and its button on the right. Returns whether the button
/// was clicked, and its rectangle.
fn labelled_row(c: &mut RowCol, geo: &RowGeo, label: &str, value: &str) -> (bool, Rect) {
    let vw = super::typography::Role::Button.with(|| super::dt_font::legacy_metrics_if(true, || measure(value, geo.size).width));
    let bw = (vw + 28.0 * geo.k).max(geo.min_w);
    let room = c.w - bw - 10.0 * geo.k;
    let beside = wrap_label(label, room, geo.size);
    if beside.iter().any(|l| measure(l, geo.size).width > room) {
        let lines = wrap_label(label, c.w, geo.size);
        for (i, line) in lines.iter().enumerate() {
            chrome::shadow_text(line, c.x, c.y + i as f32 * geo.lh + geo.size * 0.9, geo.size, chrome::CREAM);
        }
        let top = c.y + lines.len() as f32 * geo.lh + 4.0 * geo.k;
        let r = Rect::new(c.x + c.w - bw, top, bw, geo.row_h);
        c.y = top + geo.row_h + geo.gap;
        return (scroll_button(r, value, geo.btn_size), r);
    }
    let h = geo.row_h.max(beside.len() as f32 * geo.lh);
    let centre = c.y + h / 2.0;
    for (i, line) in beside.iter().enumerate() {
        let base = centre - (beside.len() as f32 - 1.0) * geo.lh / 2.0 + i as f32 * geo.lh + geo.size * 0.36;
        chrome::shadow_text(line, c.x, base, geo.size, chrome::CREAM);
    }
    let r = Rect::new(c.x + c.w - bw, c.y + (h - geo.row_h) / 2.0, bw, geo.row_h);
    c.y += h + geo.gap;
    (scroll_button(r, value, geo.btn_size), r)
}

/// A hint under a row: its lines across the column.
fn hint_under(c: &mut RowCol, geo: &RowGeo, text: &str) {
    let lines = wrap(text, c.w, geo.hint);
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_text(line, c.x, c.y + i as f32 * geo.lh + geo.hint * 0.9, geo.hint, WHITE);
    }
    c.y += lines.len() as f32 * geo.lh + geo.gap;
}

/// A checkbox row whose label may wrap (the box stays on the first line). True when clicked.
fn check_row(c: &mut RowCol, geo: &RowGeo, label: &str, on: bool, ink: Color) -> bool {
    let lines = wrap_label(label, c.w - 34.0 * geo.k, geo.size);
    let clicked = option_check_lines(c.x, c.y, c.w, &lines, on, ink, geo.k);
    c.y += (10.0 + lines.len() as f32 * 18.0).max(38.0) * geo.k + geo.gap;
    clicked
}

/// One row of a group window (the same widgets the settings window had them in).
fn draw_row(row: Row, c: &mut RowCol, geo: &RowGeo, audio: &mut super::audio::Settings, over_map: bool) {
    let yes_no = |on: bool| -> &'static str {
        if on {
            tr("On")
        } else {
            tr("Off")
        }
    };
    match row {
        Row::MapZoom => {
            // The map's own zoom (Razdor's): the wheel steps from it, 0 comes back to it; locked,
            // the wheel and +/- leave it alone. The slider sits under the row.
            let lock = if audio.zoom_locked { tr("Locked") } else { tr("Wheel and +/-") };
            if labelled_row(c, geo, tr("Map zoom"), lock).0 {
                audio.zoom_locked = !audio.zoom_locked;
            }
            let (lo, hi) = super::world_view::ZOOM_RANGE;
            let value = format!("×{:.2}", audio.map_zoom);
            let value_w = measure(&value, geo.size).width;
            let track = Rect::new(c.x, c.y, (c.w - value_w - 12.0 * geo.k).max(60.0 * geo.k), 32.0 * geo.k);
            chrome::shadow_text(&value, track.x + track.w + 8.0 * geo.k, track.y + 21.0 * geo.k, geo.size, chrome::GOLD);
            if let Some(v) = slider(track, (audio.map_zoom - lo) / (hi - lo)).filter(|_| in_adv_view(track)) {
                audio.map_zoom = ((lo + v * (hi - lo)) / 0.05).round() * 0.05;
            }
            c.y += track.h + geo.gap;
            hint_under(c, geo, tr("The zoom the map plays at. The wheel steps away from it and the 0 key comes back to it; locked, the map always shows it."));
        }
        Row::MapView => {
            // Razdor's (the owner's decision, 2026-11-02): the world map's 2.5D view.
            if labelled_row(c, geo, tr("Map view"), map_view_label(audio)).0 {
                audio.map_tilt = !audio.map_tilt;
            }
        }
        Row::SpriteSize => {
            let value = sprite_size_label(audio.map_sprite_size);
            if labelled_row(c, geo, tr("Sprite size"), &value).0 {
                audio.map_sprite_size = super::world_view::next_sprite_size(audio.map_sprite_size);
            }
        }
        Row::TwoFingerTurn => {
            if labelled_row(c, geo, tr("Two-finger turn"), yes_no(audio.map_twist)).0 {
                audio.map_twist = !audio.map_twist;
            }
        }
        Row::MapEdge => {
            // «Продолжать карту за краем» (Razdor's, off by default).
            if labelled_row(c, geo, tr("Extend the map past its edges"), yes_no(audio.map_extended)).0 {
                audio.map_extended = !audio.map_extended;
            }
            hint_under(c, geo, tr("Off: past the map's edges is black, as in the original. On: a short stretch of ground goes on past the edges, then thickens into the same black fog as the unexplored map."));
        }
        Row::MapBattles => {
            if labelled_row(c, geo, tr("Battles on the map"), yes_no(audio.battle_marks)).0 {
                audio.battle_marks = !audio.battle_marks;
            }
        }
        Row::ArmyList => {
            if labelled_row(c, geo, tr("Minimap army list"), yes_no(audio.army_list)).0 {
                audio.army_list = !audio.army_list;
            }
        }
        Row::LeaderName => {
            if labelled_row(c, geo, tr("Leader's name"), yes_no(audio.leader_caption)).0 {
                audio.leader_caption = !audio.leader_caption;
            }
        }
        Row::MinimapCorner => {
            // Razdor's (a player's report): the minimap's corner, cycled clockwise.
            let corner = match minimap::corner() {
                minimap::Corner::TopRight => tr("Top right"),
                minimap::Corner::BottomRight => tr("Bottom right"),
                minimap::Corner::BottomLeft => tr("Bottom left"),
                minimap::Corner::TopLeft => tr("Top left"),
            };
            if labelled_row(c, geo, tr("Minimap corner"), corner).0 {
                minimap::set_corner(minimap::corner().next());
            }
        }
        Row::MinimapReset => {
            if labelled_row(c, geo, tr("Minimap size and place"), tr("Reset")).0 {
                minimap::reset();
            }
        }
        Row::AiStrength => {
            let strength = ai_strength(audio);
            let label = match strength {
                AiStrength::Original => tr("As in the original"),
                AiStrength::Improved => tr("Improved"),
                AiStrength::Max => tr("Maximum (Razdor)"),
            };
            if labelled_row(c, geo, tr("Battle AI strength"), label).0 {
                set_ai_strength(audio, strength.next());
            }
            hint_under(c, geo, tr("Maximum: before each move the enemy of your battles weighs every move and answer ahead and plays each out to the end of the battle."));
        }
        Row::UnitBattle => {
            if labelled_row(c, geo, tr("Unit battle participation"), yes_no(audio.allow_unit_benching)).0 {
                audio.allow_unit_benching = !audio.allow_unit_benching;
            }
        }
        Row::KeepFormation => {
            // Razdor's deviation (the owner's request): units go back to their cells after a battle.
            if labelled_row(c, geo, tr("Keep formation after battle"), yes_no(audio.keep_formation)).0 {
                audio.keep_formation = !audio.keep_formation;
            }
        }
        Row::MaxGuard => {
            if labelled_row(c, geo, tr("Max guards casters"), yes_no(audio.max_guard)).0 {
                audio.max_guard = !audio.max_guard;
            }
        }
        Row::ArmyArrangement => {
            // Razdor's (the owner's decision after players' reports): clicks, dragging or both.
            let label = match audio.formation_controls {
                super::unit_drag::Controls::Clicks => tr("Clicks"),
                super::unit_drag::Controls::Drag => tr("Dragging"),
                super::unit_drag::Controls::Both => tr("Both"),
            };
            if labelled_row(c, geo, tr("Army arrangement"), label).0 {
                audio.formation_controls = audio.formation_controls.next();
            }
            hint_under(c, geo, tr("Clicks: the first click picks a unit or an item, the second places it. Dragging: a click only selects, and units and items are moved by dragging. Both: clicks and dragging at once."));
        }
        Row::Experience => {
            // Razdor's deviation (backlog 1529): a promoted unit's XP is dropped as in the original, or kept.
            let label = if audio.keep_xp_on_promotion { tr("Kept (Razdor)") } else { tr("Reset (original)") };
            if labelled_row(c, geo, tr("Experience on promotion"), label).0 {
                audio.keep_xp_on_promotion = !audio.keep_xp_on_promotion;
            }
            hint_under(c, geo, tr("As in the original, a unit's experience is dropped when it is promoted; Kept keeps it. Experience left over after a level is kept either way."));
        }
        Row::Screen => {
            // Windowed, borderless or full screen, set at once (`display::follow_settings`); a phone has two.
            let phone = super::display::dp_px().is_some();
            let label = if phone { tr("Screen") } else { tr("Screen (Alt+Enter)") };
            if labelled_row(c, geo, label, audio.display.for_device(phone).label()).0 {
                audio.display = audio.display.next_on(phone);
            }
        }
        Row::BottomBar => {
            // «Размер нижней панели» (Razdor's, phones only): the bar's size as the phone shows it.
            if labelled_row(c, geo, tr("Bottom bar size"), super::display::bar_size_label(audio.bottom_bar_size)).0 {
                audio.bottom_bar_size = super::display::next_bar_size(audio.bottom_bar_size);
            }
        }
        Row::InterfaceScale => {
            let fit = chrome::fit_k();
            let value = super::display::scale_label(audio.ui_scale, fit);
            if labelled_row(c, geo, tr("Interface scale"), &value).0 {
                audio.ui_scale = super::display::next_scale(audio.ui_scale, fit);
            }
        }
        Row::Font => {
            let label = match audio.font {
                super::dt_font::FontMode::Original => tr("As in the original"),
                super::dt_font::FontMode::Modern => tr("Modern"),
            };
            if labelled_row(c, geo, tr("Font"), label).0 {
                audio.font = audio.font.next();
            }
        }
        Row::TextSize => {
            if labelled_row(c, geo, tr("Text size"), super::display::text_size_label(audio.text_size)).0 {
                audio.text_size = super::display::next_text_size(audio.text_size);
            }
        }
        Row::LargeWindows => {
            if labelled_row(c, geo, tr("Large message windows"), yes_no(audio.large_windows)).0 {
                audio.large_windows = !audio.large_windows;
            }
        }
        Row::Orientation => {
            if labelled_row(c, geo, tr("Screen orientation"), orientation_label(audio.orientation)).0 {
                audio.orientation = match audio.orientation {
                    None => Some(super::audio::Orientation::Landscape),
                    Some(super::audio::Orientation::Landscape) => Some(super::audio::Orientation::Portrait),
                    Some(super::audio::Orientation::Portrait) => None,
                };
            }
        }
        Row::Language => {
            // The language list (Razdor's): Russian, English and the translations found in the install.
            let current = super::language::current_label();
            let (clicked, lr) = labelled_row(c, geo, tr("Language"), &current);
            if clicked || super::snapshot::open_language_list() {
                super::language::open_list(lr, geo.k);
            }
            hint_under(c, geo, super::language::hint());
        }
        Row::Keys => {
            // The keys window (Razdor's, the owner's request): every action's key, rebound by a click.
            if labelled_row(c, geo, tr("Keyboard shortcuts"), tr("Keys…")).0 {
                KEYS_OPEN.with(|o| o.set(true));
                KEYS_FROM_HELP.with(|f| f.set(false));
            }
        }
        Row::Touchpad => {
            let on = audio.touchpad.unwrap_or(false);
            let label = own("Options", "RazdorTouchpad", n_("Touchpad mode (the screen moves a cursor)"));
            if check_row(c, geo, &label, on, CHECK_INK) {
                audio.touchpad = Some(!on);
            }
        }
        Row::Updates => {
            // Updates from the GitHub releases (`razdor::update`); on a phone Ask and Off only.
            let phone_ui = updates_phone();
            if labelled_row(c, geo, tr("Updates"), updates_label(audio.updates, phone_ui)).0 {
                audio.updates = audio.updates.next_on(phone_ui);
            }
            let state = super::update_view::status_label();
            if labelled_row(c, geo, &state, tr("Check now")).0 {
                super::update_view::check_by_hand();
            }
            let hint = if phone_ui { tr(UPDATES_PHONE_HINT) } else { tr("Ask: a newer release is offered in a window. Always: it is downloaded and starts the next time. The check never holds up the game.") };
            hint_under(c, geo, hint);
        }
        Row::Import => {
            // «Обновить игру из архива…» (Razdor's): the button, or why it is not offered.
            super::game_import::settings_row(Rect::new(c.x, c.y, c.w, geo.row_h), over_map);
            c.y += geo.row_h + geo.gap;
        }
    }
}

/// A group window: its rows in one column, scrolling above Ok when they run past the view. The
/// desktop's window is `GROUP_W` wide and the group's height; a phone's fills the room (both
/// orientations). Returns true when closed (Ok, Esc, Enter, the close box).
fn group_window(audio: &mut super::audio::Settings, over_map: bool, group: Group) -> bool {
    // The scroll bar's width is the new-game list's (`new_game::list_track`), whatever the window's scale.
    let list_k = chrome::k();
    let title = group.title();
    let (_scale, geo, inner, closed) = match super::display::dp_px() {
        Some(dp) => {
            let scale = super::display::enter_window_scale(Some(dp));
            let k = chrome::k();
            let geo = RowGeo::phone(k, super::display::button_grow());
            let (sw, sh) = (screen_width(), screen_height());
            let safe = super::display::safe_area();
            let top = safe.top + 8.0 * k;
            let bottom = if over_map { sh - chrome::bar_height() - 8.0 * k } else { window_layout::title_bottom() - 8.0 * k };
            // On a phone the window keeps clear margins (as PHONE_WINDOW_SHRINK).
            let w = ((sw - safe.left - safe.right) * PHONE_WINDOW_SHRINK).min(900.0 * k);
            let h = (bottom - top) * PHONE_WINDOW_SHRINK;
            let frame = Rect::new((sw - w) / 2.0, top + (bottom - top - h) / 2.0, w, h);
            let (inner, closed) = chrome::window(frame, title, chrome::Skin::Marble, true);
            (scale, geo, inner, closed)
        }
        None => {
            let (ww, wh) = (GROUP_W, group.height());
            let scale = window_layout::window_scale(over_map, (ww, wh));
            let k = chrome::k();
            let geo = RowGeo::desktop(k, super::display::button_grow());
            let (inner, closed) = if over_map {
                map_window(title, ww, wh, 1.0)
            } else {
                let (w, h) = (ww * k, wh * k);
                let r = window_layout::title_rect((ww, wh)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, ((screen_height() - h) / 2.0).max(0.0), w, h));
                chrome::window(r, title, chrome::Skin::Marble, true)
            };
            (scale, geo, inner, closed)
        }
    };
    let k = geo.k;
    let on_phone = geo.btn_size.is_some();
    let pad = 12.0 * k;
    let (ok_h, gap) = (geo.row_h, geo.gap);
    let ok_w = if on_phone { 120.0 * k } else { 96.0 * k };
    // The rows scroll in the view above the Ok button, by what the last frame found they ran past it.
    let view_h = inner.h - gap - ok_h - 4.0 * k;
    let view = Rect::new(inner.x, inner.y, inner.w, view_h);
    let most = scroll_most(ADV_BOTTOM.with(|b| b.get()), view_h);
    let mut scroll = ADV_SCROLL.with(|s| s.get()).clamp(0.0, most);
    if most > 0.0 {
        super::widgets::swipe_rows(view, 40.0 * k);
        if crate::ui::widgets::mouse_in(view.x, view.y, view.w, view.h) {
            scroll = (scroll - wheel() * 40.0 * k).clamp(0.0, most);
        }
        let bar = Rect::new(inner.x + inner.w - 28.0 * list_k, inner.y, 24.0 * list_k, view_h);
        super::widgets::scrollbar_px(bar, &mut scroll, most, view_h, 40.0 * k);
    }
    // Kept while the content's height is unknown (the first frame): a snapshot's start scroll waits for it.
    if most > 0.0 {
        ADV_SCROLL.with(|s| s.set(scroll));
    }
    phone_clip(Some(view));
    ADV_VIEW.with(|v| v.set(Some(view)));
    let top = inner.y - scroll;
    let mut c = RowCol { x: inner.x + pad, w: inner.w - 2.0 * pad - 28.0 * list_k, y: top + pad };
    for row in group.rows() {
        if row.shown(audio) {
            draw_row(*row, &mut c, &geo, audio, over_map);
        }
    }
    let content_bottom = c.y;
    phone_clip(None);
    ADV_VIEW.with(|v| v.set(None));
    ADV_BOTTOM.with(|b| b.set(content_bottom - top));
    let ok = Rect::new(inner.x + inner.w - pad - ok_w, inner.y + inner.h - gap - ok_h, ok_w, ok_h);
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let done = closed || key(KeyCode::Escape) || key(KeyCode::Enter) || button_draw(ok, &ok_label, geo.btn_size);
    if done {
        ADV_SCROLL.with(|s| s.set(0.0));
    }
    done
}

/// Whether two rectangles share a point.
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Whether the point is inside the rectangle.
fn in_rect(p: (f32, f32), r: Rect) -> bool {
    p.0 >= r.x && p.0 < r.x + r.w && p.1 >= r.y && p.1 < r.y + r.h
}

/// Whether a control at `r` may take input now: in a group window's scroll view only where it shows.
fn in_adv_view(r: Rect) -> bool {
    ADV_VIEW.with(|v| v.get()).is_none_or(|view| overlaps(r, view))
}

/// A scroll range: how far content that ends `bottom` px down goes past a `view` px high window
/// (0 when it fits).
fn scroll_most(bottom: f32, view: f32) -> f32 {
    (bottom - view).max(0.0)
}

/// The phone's window scale and frame, as a share of the fit (the owner's request, 2026-11-03).
const PHONE_WINDOW_SHRINK: f32 = 0.88;

/// `r` scaled by `s` about its centre (the same centre, so the window stays centred in its room).
fn shrink_about_centre(r: Rect, s: f32) -> Rect {
    let (w, h) = (r.w * s, r.h * s);
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    Rect::new(cx - w / 2.0, cy - h / 2.0, w, h)
}

/// The «Обновления» row's hint on a phone (the row cycles Ask and Off there, `Mode::next_on`).
const UPDATES_PHONE_HINT: &str = n_("Ask: at start a newer version is offered in a window. Off: Razdor does not check for updates.");

/// The update row is the phone's (Ask and Off, the phone's hint) on a phone, held upright or
/// sideways, and on a phone-sized layout (`display::dp_px`).
fn updates_phone() -> bool {
    razdor::platform::MOBILE || super::display::dp_px().is_some()
}

/// The «Обновления» row's value: the mode the row acts on (a stored "Always" reads as "Ask" on a phone).
fn updates_label(mode: razdor::update::Mode, phone: bool) -> &'static str {
    match razdor::update::effective_on(mode, phone) {
        razdor::update::Mode::Ask => tr("Ask"),
        razdor::update::Mode::Always => tr("Always"),
        razdor::update::Mode::Off => tr("Off"),
    }
}

/// The world map's view row: «Плоский» or «2.5D».
fn map_view_label(audio: &super::audio::Settings) -> &'static str {
    if audio.map_tilt { "2.5D" } else { tr("Flat") }
}

/// The sprites' size as its row shows it: `1×`, `1.5×`, `2×`, or a stored value.
fn sprite_size_label(f: f32) -> String {
    format!("{}×", (f * 100.0).round() / 100.0)
}

/// A row button of the keys window and the settings (Razdor's own rows), true when clicked.
fn row_button(r: Rect, label: &str) -> bool {
    // In a group window's scroll view a row outside it is neither drawn nor clicked, and one half in it
    // is clicked only where the pointer is in it.
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

fn settings_button(r: Rect, label: &str) -> bool {
    let (_, down, pressed) = super::widgets::button_state(r, true);
    chrome::marble_button(r, label, true, down);
    if pressed {
        cue(Cue::Button);
    }
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
/// second line; the rows scroll when they are taller than the window. Its last rows are the five
/// group buttons (backlog 1559), two or three to a row. Desktop and sideways keep
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
        // The group buttons (backlog 1559): as many to a row as their labels fit (`group_per_row`), in a
        // fixed block above the buttons row (44 units high, 10 above its bottom), not in the scroll list.
        let widths = group_label_widths(14.0 * u);
        let per = group_per_row(&widths, cw, 8.0 * u, 10.0 * u);
        let group_rows = (Group::ALL.len() as f32 / per as f32).ceil();
        let group_h = group_rows * 44.0 * u + (group_rows - 1.0) * 8.0 * u;
        let group_top = inner.y + inner.h - 12.0 * u - 40.0 * u - 8.0 * u - group_h;
        // The list between the window's top and the group block.
        let list = Rect::new(inner.x, inner.y + 6.0 * u, inner.w, group_top - 8.0 * u - (inner.y + 6.0 * u));
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
        let checks: Vec<(&'static str, String, bool, Color)> = vec![
            ("Option4", own("Options", "Option4", n_("Do not load music and sounds (needs a restart)")), no_sound(audio), CHECK_ORANGE),
            ("Option5", own("Options", "Option5", n_("Show FPS")), audio.show_fps, CHECK_INK),
            ("Option6", own("Options", "Option6", n_("Turn off hints")), hints_off(audio), CHECK_INK),
            ("Option7", own("Options", "Option7", n_("Automatically pursue the chosen army")), pursue(audio), CHECK_INK),
            ("Option8", own("Options", "Option8", n_("Automatically save the current game")), autosave(audio), CHECK_INK),
            ("Option9", own("Options", "Option9", n_("Improved enemy AI in battle")), expert_ai(audio), CHECK_INK),
            ("Option10", own("Options", "Option10", n_("Impossible difficulty (needs a restart)")), impossible(audio), CHECK_INK),
            ("Option11", own("Options", "Option11", n_("Wide front row in battle (for new games)")), wide_row(audio, install_wide), CHECK_INK),
        ];
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
        let content = y + 8.0 * u;

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
                    _ => audio.wide_row = Some(!on),
                }
            }
        }
        phone_clip(None);
        // The group buttons: their block is fixed above the buttons row, so they are not scrolled.
        let mut opened = None;
        for (grp, r) in Group::ALL.into_iter().zip(group_rects(Rect::new(x0, group_top, cw, 44.0 * u), &widths, 10.0 * u, 8.0 * u, 44.0 * u)) {
            if button_draw(r, grp.button(), Some(14.0 * u)) {
                opened = Some(grp);
            }
        }
        if let Some(grp) = opened {
            GROUP.with(|g| g.set(Some(grp)));
            return false;
        }

        // The buttons row: «Принять» and «Отмена» at the right, 40 units high.
        let bh = 40.0 * u;
        let by = inner.y + inner.h - 12.0 * u - bh;
        // The two scale down together when their widths would overlap (`bottom_row_widths`).
        let w = bottom_row_widths(inner.w - 2.0 * pad, &[110.0 * u, 110.0 * u], 8.0 * u);
        let cancel = Rect::new(inner.x + inner.w - pad - w[1], by, w[1], bh);
        let apply = Rect::new(cancel.x - 8.0 * u - w[0], by, w[0], bh);
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

/// The settings window (the original's sound, graphics and gameplay rows) with the five group
/// buttons under them (`group_window`), and «Принять» / «Отмена» at the bottom. A group window
/// open over it takes the input meanwhile. Returns true when closed.
pub fn options_window(audio: &mut super::audio::Settings, install_wide: bool, over_map: bool) -> bool {
    if let Some(group) = GROUP.with(|g| g.get()) {
        // The keys window stands over the group's window, which takes no input meanwhile.
        if KEYS_OPEN.with(|o| o.get()) {
            if keys_window(audio, over_map) && after_keys_closed() {
                return true;
            }
            return false;
        }
        if group_window(audio, over_map, group) {
            GROUP.with(|g| g.set(None));
            // What the group window set is kept, whatever the settings window ends with; the
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
    let (inner, closed) = if over_map { map_window(&title, 594.0, h, 1.0) } else { window(&title, 594.0, h) };
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
    // The five group buttons (backlog 1559), a row under the checkboxes: each opens its window.
    let group_y = cy(7) + 36.0 * k;
    let row_h = 28.0 * k * g;
    let widths = group_label_widths(GROUP_LABEL * k * g);
    let mut rects = group_rects(Rect::new(inner.x + 14.0 * k, group_y, inner.w - 28.0 * k, row_h), &widths, 14.0 * k, 6.0 * k, row_h);
    let mut opened = None;
    for (grp, r) in Group::ALL.into_iter().zip(rects.drain(..)) {
        if button_draw(r, grp.button(), None) {
            opened = Some(grp);
        }
    }
    // «Принять» keeps what was changed, «Отмена» (the close box and Esc too) puts it back.
    let bh = 32.0 * g;
    let button_y = outer.y + (h - 10.0 - bh) * k;
    let w = bottom_row_widths(inner.w - 28.0 * k, &[89.0 * k * g, 89.0 * k * g], 8.0 * k);
    let cancel = Rect::new(inner.x + inner.w - 14.0 * k - w[1], button_y, w[1], bh * k);
    let apply = Rect::new(cancel.x - 8.0 * k - w[0], cancel.y, w[0], bh * k);
    let (apply_label, cancel_label) = (own("Buttons", "Apply", n_("Apply")), own("Buttons", "Cancel", n_("Cancel")));
    // Both act when the button is let go over it (PR #5's buttons).
    let apply_pressed = settings_button(apply, &apply_label);
    let cancel_pressed = settings_button(cancel, &cancel_label);
    if let Some(grp) = opened {
        GROUP.with(|g| g.set(Some(grp)));
        return false;
    }
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
/// settings button, a second press: the owner's request). The group and keys windows close
/// with it; the settings already set in `audio` stay.
pub fn close_options_kept() {
    GROUP.with(|g| g.set(None));
    KEYS_OPEN.with(|o| o.set(false));
    KEYS_FROM_HELP.with(|f| f.set(false));
    BACK_TO_HELP.with(|b| b.set(false));
    OPENED_WITH.with(|o| *o.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bottom_row_scales_down_to_its_room_and_keeps_its_gaps() {
        // Fits: as given.
        assert_eq!(bottom_row_widths(400.0, &[120.0, 89.0, 89.0], 8.0), vec![120.0, 89.0, 89.0]);
        // Too wide: scaled together, the buttons and the gaps fill the room exactly, none overlaps.
        let w = bottom_row_widths(300.0, &[120.0, 110.0, 110.0], 8.0);
        assert!((w.iter().sum::<f32>() + 16.0 - 300.0).abs() < 1e-3, "{w:?}");
        assert!(w.iter().all(|x| *x > 0.0));
        assert!((w[0] / 120.0 - w[1] / 110.0).abs() < 1e-4 && (w[1] - w[2]).abs() < 1e-4, "{w:?}");
        // No room at all: never negative.
        assert!(bottom_row_widths(10.0, &[120.0, 110.0, 110.0], 8.0).iter().all(|x| *x == 0.0));
    }

    #[test]
    fn the_corner_clearance_is_the_radius_at_the_edge_and_none_above_the_curve() {
        assert_eq!(corner_clearance(110.0, 0.0), 110.0);
        assert_eq!(corner_clearance(110.0, 110.0), 0.0);
        assert_eq!(corner_clearance(110.0, 300.0), 0.0);
        assert_eq!(corner_clearance(0.0, 0.0), 0.0);
        // At depth r - r/sqrt(2) the curve is at 45 degrees: the inset is r - r/sqrt(2).
        let r = 110.0_f32;
        let mid = corner_clearance(r, r - r / 2.0_f32.sqrt());
        assert!((mid - (r - r / 2.0_f32.sqrt())).abs() < 1e-3, "{mid}");
        // Higher up the curve the inset is smaller, never negative.
        let mut last = f32::MAX;
        for i in 0..=110 {
            let c = corner_clearance(r, i as f32);
            assert!(c <= last + 1e-4 && c >= 0.0, "depth {i}: {c} after {last}");
            last = c;
        }
    }

    /// The bar's settings button closes the window with what was changed kept: the state the
    /// window opened with is dropped (no revert), and the sub-windows close with it.
    #[test]
    fn closing_from_the_bar_keeps_the_changes() {
        OPENED_WITH.with(|o| *o.borrow_mut() = Some(super::super::audio::Settings::default()));
        GROUP.with(|a| a.set(Some(Group::Map)));
        KEYS_OPEN.with(|o| o.set(true));
        close_options_kept();
        assert!(OPENED_WITH.with(|o| o.borrow().is_none()));
        assert!(!GROUP.with(|a| a.get().is_some()) && !KEYS_OPEN.with(|o| o.get()));
    }

    /// The keys window opened from the F1 list: its close closes the settings too (the advanced
    /// window and the keys with them, the changes kept), and the list reopens once.
    #[test]
    fn keys_from_the_help_list_close_back_to_it() {
        open_keys_from_help();
        assert!(after_keys_closed());
        assert!(!GROUP.with(|a| a.get().is_some()) && !KEYS_OPEN.with(|o| o.get()));
        assert!(OPENED_WITH.with(|o| o.borrow().is_none()));
        assert!(take_back_to_help());
        assert!(!take_back_to_help());
    }

    /// Keys opened any other way (the advanced window's row, the snapshot scene) close to the
    /// advanced window, as before: the list does not come back.
    #[test]
    fn plain_keys_close_to_the_advanced_window() {
        open_keys();
        assert!(!after_keys_closed());
        assert!(GROUP.with(|a| a.get().is_some()) && !KEYS_OPEN.with(|o| o.get()));
        assert!(!take_back_to_help());
    }

    /// Closing the settings from the bar clears the list's return with the rest.
    #[test]
    fn closing_from_the_bar_forgets_the_list_return() {
        open_keys_from_help();
        close_options_kept();
        assert!(!take_back_to_help());
        assert!(!KEYS_FROM_HELP.with(|f| f.get()));
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
        let m = portrait_menu(sw, sh, k, dp, 0.0, false, 0.0);
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
            let m = portrait_menu(1080.0, sh, 1.125, dp, 0.0, false, 0.0);
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
        let m = portrait_menu(sw, sh, k, dp, 120.0, false, 0.0);
        assert!(m.logo_top >= 120.0, "logo at {} is above the inset", m.logo_top);
        assert!(m.buttons[0].y >= m.logo_top + 840.0 * 128.0 / 896.0 * k);
        // And with the logo on the inset the column still ends above the lines.
        let last = m.buttons[4];
        assert!(last.y + last.h <= m.notice_y(k) - 16.0 * k, "{last:?} runs into the lines");
    }

    #[test]
    fn upright_chips_are_touch_sized_and_on_the_bottom_of_the_screen() {
        let (sw, sh, k, dp) = PHONE;
        let m = portrait_menu(sw, sh, k, dp, 0.0, false, 0.0);
        assert!(m.chip_h >= TOUCH_DP * dp, "chips are {} px tall", m.chip_h);
        assert!(m.chip_top + m.chip_h <= sh, "chips leave the screen");
        assert!(m.foot_y <= m.chip_top, "the lines run into the chips");
        assert!(m.version_y(k) < m.foot_y && m.notice_y(k) < m.version_y(k));
    }

    #[test]
    fn a_mod_row_sits_above_the_four_tiles_and_lifts_the_foot_lines() {
        let (sw, sh, k, dp) = PHONE;
        let plain = portrait_menu(sw, sh, k, dp, 0.0, false, 0.0);
        let with = portrait_menu(sw, sh, k, dp, 0.0, true, 0.0);
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
            let m = portrait_menu(sw, sh, k, dp, 0.0, true, 0.0);
            let last = m.buttons[4];
            assert!(last.y + last.h <= m.notice_y(k), "{sh}: the last button runs into the foot lines");
            assert!(m.logo_top >= 0.0, "{sh}: the logo leaves the screen");
        }
    }

    #[test]
    fn upright_geometry_fits_a_small_phone_too() {
        // 720×1600 at 2 px per dp: the fit is 0.75.
        let m = portrait_menu(720.0, 1600.0, 0.75, 2.0, 0.0, false, 0.0);
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

    /// On a phone the advanced window scrolls by what its content runs past the view above Ok;
    /// content that fits does not scroll at all (the desktop's case).
    #[test]
    fn the_advanced_window_scrolls_by_what_runs_past_its_view() {
        assert_eq!(scroll_most(700.0, 573.0), 127.0);
        assert_eq!(scroll_most(500.0, 573.0), 0.0);
        assert_eq!(scroll_most(573.0, 573.0), 0.0);
    }

    /// Every setting row is in exactly one group, and the groups are the five the owner asked for.
    #[test]
    fn every_setting_row_belongs_to_exactly_one_group() {
        for row in Row::ALL {
            let groups = Group::ALL.iter().filter(|g| g.rows().contains(&row)).count();
            assert_eq!(groups, 1, "{row:?} is in {groups} groups");
        }
        let listed: usize = Group::ALL.iter().map(|g| g.rows().len()).sum();
        assert_eq!(listed, Row::ALL.len());
        assert_eq!(Group::ALL.len(), 5);
        for g in Group::ALL {
            assert_eq!(Group::from_name(g.name()), Some(g));
        }
    }

    /// The group buttons: five to a row when their labels fit, else three, else two.
    #[test]
    fn the_group_buttons_take_five_three_or_two_to_a_row() {
        // A 60 px label needs 76 px with its pad: five fit in 500 (95 px each), three in 300.
        assert_eq!(group_per_row(&[60.0; 5], 500.0, 6.0, 8.0), 5);
        assert_eq!(group_per_row(&[60.0; 5], 300.0, 6.0, 8.0), 3);
        assert_eq!(group_per_row(&[150.0; 5], 200.0, 6.0, 8.0), 2);
        let area = Rect::new(0.0, 0.0, 300.0, 0.0);
        let rects = group_rects(area, &[60.0; 5], 8.0, 6.0, 20.0);
        assert_eq!(rects.len(), 5);
        assert!(rects.iter().all(|r| r.x >= 0.0 && r.x + r.w <= 300.0 + 1e-3));
        // The second row starts under the first.
        assert!(rects[3].y >= rects[0].y + 20.0);
    }
}
