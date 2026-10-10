//! The original's main menu (the 18:08 frame of the gameplay video): the burning ruins
//! (`Windows/Castle.lit`) over the whole screen, the silver "Времена Раздора" logo
//! (`Title_RUS.ugs`) with flames rising off its letters (`TitleFlame_RUS.lit`), and five oval
//! buttons (`mb2.ugs`, the last one pointed) whose marble middles swirl in violet
//! (`MenuFlame.lit`) under the words of `SMText_RUS.lit`; the button under the mouse turns
//! gold. Razdor's own extras, the map editor and the interface language, sit small in the
//! bottom corners. Layout numbers are pixels of the 960×720 video, scaled by [`chrome::k`].

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

/// One frame of a 4×4 sheet of 128×128 flame frames, for time `t`.
fn flame_frame(t: f64) -> Rect {
    let f = ((t * 12.0) as usize) % 16;
    Rect::new((f % 4) as f32 * 128.0, (f / 4) as f32 * 128.0, 128.0, 128.0)
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
        let rows = 9.0;
        let fh = fl.height() / rows;
        let row = ((get_time() * 10.0) as usize % 9) as f32;
        // The two words, each half of the sheet, over each half of the logo.
        for half in 0..2 {
            let src = Rect::new(half as f32 * fl.width() / 2.0, row * fh, fl.width() / 2.0, fh);
            let dst = Rect::new(x + half as f32 * w / 2.0, y - h * 0.18, w / 2.0, h * 0.9);
            chrome::additive(|| chrome::tex_src(&fl, src, dst, Color::new(1.0, 0.55, 0.2, 0.7)));
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
    let hover = hot == Some(i);
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
        chrome::additive(|| chrome::tex_src(&fl, flame_frame(get_time() * 0.8 + i as f64 * 0.61), f, Color::new(1.0, 0.55, 0.2, 0.5)));
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
        chrome::additive(|| chrome::tex_src(&fl, flame_frame(get_time() + i as f64 * 0.37), inner, tint));
    }
    word(i, label, c, hover);
    if hover && clicked() {
        cue(Cue::MenuPress);
        return true;
    }
    false
}

/// The button's word: its row of `SMText_RUS` in Russian, else the text.
fn word(i: usize, label: &str, c: Vec2, hover: bool) {
    let k = chrome::k();
    let color = if hover { Color::new(1.0, 0.93, 0.80, 1.0) } else { Color::new(0.93, 0.90, 1.0, 1.0) };
    let sheet = (i18n::lang() == Lang::Ru).then(|| chrome::win_fx("SMText_RUS", Fx::Glow)).flatten();
    match sheet {
        Some(t) => {
            let rh = t.height() / BUTTONS.len() as f32;
            let src = Rect::new(0.0, i as f32 * rh, t.width(), rh);
            let (w, h) = (t.width() * 1.05 * k, rh * 1.05 * k);
            let dst = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
            // A dark rim, then the word.
            for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                chrome::tex_src(&t, src, Rect::new(dst.x + dx * k, dst.y + dy * k, dst.w, dst.h), Color::new(0.12, 0.05, 0.25, 0.8));
            }
            chrome::tex_src(&t, src, dst, color);
        }
        None => chrome::strong_centered(label, c.x, c.y + 8.0 * k, 24.0 * k, color),
    }
}

/// A small text link (the Razdor extras). Returns true when clicked.
fn link(label: &str, x: f32, y: f32, right: bool) -> bool {
    let size = 18.0 * chrome::k();
    let w = measure(label, size).width;
    let x = if right { x - w } else { x };
    let hover = !input_blocked() && mouse_in(x, y - size, w, size * 1.3);
    let color = if hover { chrome::GOLD } else { Color::new(0.95, 0.88, 0.72, 0.85) };
    chrome::shadow_text(label, x, y, size, color);
    if hover && clicked() {
        cue(Cue::Button);
        return true;
    }
    false
}

/// The menu's ruins and logo without the buttons (behind the load and new-game windows).
pub fn backdrop() {
    background();
    logo();
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
    let y = screen_height() - 16.0 * k;
    if link(tr("Map editor"), 16.0 * k, y, false) {
        pick = Some(Pick::Editor);
    }
    let lang = format!("{} / {}", Lang::En.label(), Lang::Ru.label());
    if link(&lang, screen_width() - 16.0 * k, y, true) {
        super::language::toggle();
    }
    // Esc quits the game at once, without a question, as in the original (0x4c8059).
    if key(KeyCode::Escape) {
        pick = Some(Pick::Exit);
    }
    pick
}

thread_local! {
    /// The credits scroll: its dark outline (`Credits_Alpha` as the alpha of black) and its
    /// letters (`Credits_Color`, drawn additively), made once.
    static CREDITS: std::cell::OnceCell<Option<(Texture2D, Texture2D)>> = const { std::cell::OnceCell::new() };
}

fn credits() -> Option<(Texture2D, Texture2D)> {
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
            Some((make(&shadow), make(&color.rgba)))
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

/// "Информация об авторах": the splash of scattered drawings with the credits rolling up
/// over it (`started` is when the window opened). A click or Esc closes it.
pub fn authors(started: f64) -> bool {
    backdrop();
    let title = own("Authors", "Title", n_("About the authors"));
    let (inner, closed) = window(&title, 632.0, 460.0);
    let k = chrome::k();
    match chrome::win("Authors-Splash") {
        Some(t) => chrome::tex(&t, inner, WHITE),
        None => chrome::surface(inner, chrome::Skin::Brown),
    }
    if let Some((shadow, t)) = credits() {
        // As the original (0x4c82c4): the picture's top at the page's top to begin with (its
        // own blank start lets the first lines rise from below), 35 of its pixels a second,
        // and it stays on its last view once the end is in sight.
        let px = k * 0.9375;
        let w = t.width() * px;
        let h = t.height() * px;
        let timed = (get_time() - started) as f32 * 35.0 * px;
        let most = (h - inner.h).max(0.0);
        // The clock while the credits roll on their own (Credits_OnOpen 0x4cfc6c calls
        // 0x48ecf0); the arrow once their time is up (0x4c84e7).
        if timed < most {
            super::cursor::set(super::cursor::Shape::Clock);
        }
        let scrolled = timed.min(most);
        let y = inner.y - scrolled;
        // Only the part inside the page.
        let top = y.max(inner.y);
        let bottom = (y + h).min(inner.y + inner.h);
        if bottom > top {
            let src = Rect::new(0.0, (top - y) / (k * 0.9375), t.width(), (bottom - top) / (k * 0.9375));
            let dst = Rect::new(inner.center().x - w / 2.0, top, w, bottom - top);
            chrome::tex_src(&shadow, src, dst, WHITE);
            chrome::additive(|| chrome::tex_src(&t, src, dst, WHITE));
        }
    }
    closed || key(KeyCode::Escape) || (clicked() && inner.contains(crate::ui::widgets::pointer().into()))
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

/// "Настройки звука, графики и геймплея", with what Razdor lets the player change: the
/// music and sound volumes (saved at once) and the interface language. Returns true when
/// closed.
pub fn options(audio: &mut super::audio::Settings, install_wide: bool) -> bool {
    backdrop();
    options_window(audio, install_wide)
}

/// The settings window alone (over the main menu, or over the map from the bar's gears).
pub fn options_window(audio: &mut super::audio::Settings, install_wide: bool) -> bool {
    let title = own("Options", "Title", n_("Sound, graphics and gameplay settings"));
    let (inner, closed) = window(&title, 594.0, 426.0);
    let k = chrome::k();
    let rows = [
        (own("Options", "OptionSld0", n_("Background music volume")), audio.music_volume, true),
        (own("Options", "OptionSld1", n_("Sound effects volume")), audio.sfx_volume, false),
    ];
    for (i, (label, value, music)) in rows.into_iter().enumerate() {
        let y = inner.y + 36.0 * k + i as f32 * 58.0 * k;
        chrome::shadow_text(&label, inner.x + 24.0 * k, y, 14.0 * k, chrome::CREAM);
        let pct = format!("{:.0}%", value * 100.0);
        chrome::shadow_text(&pct, inner.x + inner.w - 24.0 * k - measure(&pct, 14.0 * k).width, y, 14.0 * k, chrome::GOLD);
        let track = Rect::new(inner.x + 24.0 * k, y + 10.0 * k, inner.w - 48.0 * k, 18.0 * k);
        if let Some(v) = slider(track, value) {
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
    // The Community's "Скорость анимаций в битве" (`AnimationSpeed`): it caps a strike's
    // slide at (100 − S)·5 ms and its effect at (100 − S)·3 ms (0x4afd45, 0x4afe7c).
    let y = inner.y + 36.0 * k + 2.0 * 58.0 * k;
    let label = own("Options", "OptionSld4", n_("Battle animation speed"));
    chrome::shadow_text(&label, inner.x + 24.0 * k, y, 14.0 * k, chrome::CREAM);
    let speed = anim_speed(audio).unwrap_or(0.0);
    let pct = format!("{speed:.0}%");
    chrome::shadow_text(&pct, inner.x + inner.w - 24.0 * k - measure(&pct, 14.0 * k).width, y, 14.0 * k, chrome::GOLD);
    let track = Rect::new(inner.x + 24.0 * k, y + 10.0 * k, inner.w - 48.0 * k, 18.0 * k);
    if let Some(v) = slider(track, speed / 99.0) {
        audio.anim_speed = Some((v * 99.0).round());
    }
    let lang = format!("{} / {}", Lang::En.label(), Lang::Ru.label());
    let lr = Rect::new(inner.x + 24.0 * k, inner.y + inner.h - 44.0 * k, 140.0 * k, 28.0 * k);
    let over = lr.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    chrome::marble_button(lr, &lang, true, over);
    if over && clicked() {
        cue(Cue::Button);
        super::language::toggle();
    }
    let fr = Rect::new(lr.x + lr.w + 16.0 * k, lr.y, 150.0 * k, lr.h);
    let fps_label = if audio.show_fps { tr("FPS shown") } else { tr("FPS hidden") };
    let over_fps = fr.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    chrome::marble_button(fr, fps_label, true, over_fps);
    if over_fps && clicked() {
        cue(Cue::Button);
        audio.show_fps = !audio.show_fps;
    }
    // The battle AI, as the original's "Улучшенный интеллект противника в битве": easy or
    // expert, from the install's setting until chosen here.
    let y = inner.y + 36.0 * k + 3.0 * 58.0 * k;
    let label = own("Options", "Option9", n_("Improved enemy AI in battle"));
    chrome::shadow_text(&label, inner.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let expert = expert_ai(audio);
    let ar = Rect::new(inner.x + inner.w - 24.0 * k - 150.0 * k, y, 150.0 * k, 28.0 * k);
    let ai_label = if expert { tr("Expert") } else { tr("Easy") };
    let over_ai = ar.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    chrome::marble_button(ar, ai_label, true, over_ai);
    if over_ai && clicked() {
        cue(Cue::Button);
        audio.expert_ai = Some(!expert);
    }
    // The front row's width (Razdor's switch for the original's "wide front row in battle",
    // which only the install's ini set): for the games started from now on.
    let y = y + 58.0 * k;
    chrome::shadow_text(tr("Front row in battle (new games)"), inner.x + 24.0 * k, y + 18.0 * k, 14.0 * k, chrome::CREAM);
    let wide = wide_row(audio, install_wide);
    let rr = Rect::new(ar.x, y, ar.w, ar.h);
    let row_label = if wide { tr("6 cells") } else { tr("4 cells") };
    let over_row = rr.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    chrome::marble_button(rr, row_label, true, over_row);
    if over_row && clicked() {
        cue(Cue::Button);
        audio.wide_row = Some(!wide);
    }
    let ok = Rect::new(inner.x + inner.w - 120.0 * k, inner.y + inner.h - 44.0 * k, 96.0 * k, 28.0 * k);
    let over_ok = ok.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
    let ok_label = own("Buttons", "Ok", n_("OK"));
    chrome::marble_button(ok, &ok_label, true, over_ok);
    closed || key(KeyCode::Escape) || key(KeyCode::Enter) || (over_ok && clicked())
}
