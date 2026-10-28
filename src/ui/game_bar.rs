//! The bottom game bar, as the original's (video notes §1, ref 06): four oval buttons on
//! the left (menu, settings, save, load), the time panel in the middle, four on the right
//! (journal, hero and army, spell book, map), and under them the strip with mana, gold,
//! income and wages. The buttons are blue, grey while a window is open, green for the open
//! screen and orange while the minimap shows. Hovering the time panel's message box on the
//! idle map shows the original's three small buttons on it (interface.md §6): wait 1 hour,
//! centre the view on the hero, wait 4 hours.

use razdor::local;
use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::clock::duration_label;
use razdor::rules::game::Game;
use razdor::trf;

use super::chrome::{self, shadow_centered, shadow_text, tex, Fx, CREAM, GOLD};
use super::display::{safe_area, SafeArea};
use super::widgets::{fit_size, measure, panel_hint};

/// A bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    Menu,
    Settings,
    Save,
    Load,
    Hero,
    Squad,
    Spells,
    Map,
}

impl BarButton {
    /// From the screen's left edge inwards, as the original's ids 1–4 (4d46e0): exit menu,
    /// options, load, and save next to the message box.
    const LEFT: [BarButton; 4] = [BarButton::Menu, BarButton::Settings, BarButton::Load, BarButton::Save];
    const RIGHT: [BarButton; 4] = [BarButton::Hero, BarButton::Squad, BarButton::Spells, BarButton::Map];

    fn icon(self) -> usize {
        match self {
            BarButton::Menu => 1,
            BarButton::Settings => 2,
            // Crossed in the original's files: save (id 4) shows `icon_3`, load (id 3) `icon_4`.
            BarButton::Save => 3,
            BarButton::Load => 4,
            BarButton::Hero => 5,
            BarButton::Squad => 6,
            BarButton::Spells => 7,
            BarButton::Map => 8,
        }
    }

    /// The small ovals are the outer two on each side.
    fn small(self) -> bool {
        matches!(self, BarButton::Menu | BarButton::Settings | BarButton::Spells | BarButton::Map)
    }

    /// The hint: the install's own (`[GameMenu]`) in Russian, else ours.
    fn hint(self) -> String {
        let key = match self {
            BarButton::Menu => "cExit",
            BarButton::Settings => "cOptions",
            BarButton::Save => "cSave",
            BarButton::Load => "cLoad",
            BarButton::Hero => "cHero",
            BarButton::Squad => "cArmy",
            BarButton::Spells => "cMagic",
            BarButton::Map => "cMap",
        };
        let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("GameMenu", key)).flatten();
        own.unwrap_or_else(|| self.our_hint())
    }

    /// Razdor's hint; the keys named are the current bindings (`keymap`).
    fn our_hint(self) -> String {
        use super::keymap::{display, key, Action};
        let k = |a: Action| display(key(a));
        match self {
            BarButton::Menu => tr("Main menu (Esc)").to_string(),
            BarButton::Settings => tr("Sound settings").to_string(),
            BarButton::Save => trf!("Save the game ({key}: quick save)", key = k(Action::QuickSave)),
            BarButton::Load => trf!("Load a saved game ({key}: quick load)", key = k(Action::QuickLoad)),
            BarButton::Hero => tr("The hero's characteristics and inventory").to_string(),
            BarButton::Squad => trf!("The hero and his army ({key})", key = k(Action::HeroArmy)),
            BarButton::Spells => trf!("The spell book ({key})", key = k(Action::SpellBook)),
            BarButton::Map => trf!("Map of the scenario ({key})", key = k(Action::Minimap)),
        }
    }
}

/// The three small buttons over the message box (interface.md §6; 0x4d46e0 builds them,
/// 0x4b930c shows them, 0x4b9448 presses them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeButton {
    /// Wait 1 hour (2 ticks).
    Wait1,
    /// The view glides back to the hero (0x4af96c).
    ShowHero,
    /// Wait 4 hours (8 ticks).
    Wait4,
}

/// The original's message box (x, y, w, h) in its 1024×768 pixels: hovering it shows the
/// buttons.
const MESSAGE_BOX: (f32, f32, f32, f32) = (372.0, 684.0, 280.0, 60.0);
/// The original's bottom panel: its top and the screen's middle.
const PANEL_TOP: f32 = 682.0;
const PANEL_MID: f32 = 512.0;
/// The original's panel row; Razdor's bar row stands for it.
const PANEL_ROW: f32 = 64.0;

impl TimeButton {
    pub const ALL: [TimeButton; 3] = [TimeButton::Wait1, TimeButton::ShowHero, TimeButton::Wait4];

    /// Where the original puts it (x, y, w, h): the size of its art.
    fn original_rect(self) -> (f32, f32, f32, f32) {
        match self {
            TimeButton::Wait1 => (378.0, 693.0, 80.0, 42.0),
            TimeButton::ShowHero => (461.0, 687.0, 102.0, 54.0),
            TimeButton::Wait4 => (566.0, 693.0, 80.0, 42.0),
        }
    }

    /// Its art in `Graphics/Windows`: up, down and the alpha mask.
    fn art(self) -> (&'static str, &'static str, &'static str) {
        match self {
            TimeButton::Wait1 => ("GP-ButtonLeft", "GP-ButtonLeftDown", "GP-Button-Alpha"),
            TimeButton::ShowHero => ("GP-ButtonCenter", "GP-ButtonCenterDown", "GP-ButtonCenter-Alpha"),
            TimeButton::Wait4 => ("GP-ButtonRight", "GP-ButtonRightDown", "GP-Button-Alpha"),
        }
    }

    /// The hint: the install's own (`[GameMenu] cp_…`) in Russian, else ours.
    fn hint(self) -> String {
        let (key, ours) = match self {
            TimeButton::Wait1 => ("cp_Wait1Hour", tr("Wait 1 hour, the hero stays where he is (1)")),
            TimeButton::ShowHero => ("cp_ShowHero", tr("Move the view to the hero (Tab)")),
            TimeButton::Wait4 => ("cp_Wait4Hour", tr("Wait 4 hours, the hero stays where he is (4)")),
        };
        let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("GameMenu", key)).flatten();
        own.unwrap_or_else(|| ours.to_string())
    }
}

/// An original panel rectangle on Razdor's bar: its top at `top`, the screen's middle at
/// `mid`, `s` Razdor pixels to the original's.
fn on_bar(r: (f32, f32, f32, f32), top: f32, mid: f32, s: f32) -> Rect {
    Rect::new(mid + (r.0 - PANEL_MID) * s, top + (r.1 - PANEL_TOP) * s, r.2 * s, r.3 * s)
}

/// The message box on the bar (see [`on_bar`]).
pub fn message_box(top: f32, mid: f32, s: f32) -> Rect {
    on_bar(MESSAGE_BOX, top, mid, s)
}

/// Where button `b` stands on the bar (see [`on_bar`]).
pub fn time_button_rect(b: TimeButton, top: f32, mid: f32, s: f32) -> Rect {
    on_bar(b.original_rect(), top, mid, s)
}

local! {
    /// The time buttons' art with its alpha mask applied, by name.
    static TIME_ART: std::cell::RefCell<std::collections::HashMap<&'static str, Option<Texture2D>>> = std::cell::RefCell::new(Default::default());
}

/// Picture `name` of `Graphics/Windows` with `alpha` (greyscale) as its alpha.
fn masked(name: &'static str, alpha: &str) -> Option<Texture2D> {
    TIME_ART.with(|m| {
        m.borrow_mut()
            .entry(name)
            .or_insert_with(|| {
                let mut img = chrome::image(&format!("Windows/{name}.lit"))?;
                let mask = chrome::image(&format!("Windows/{alpha}.lit"))?;
                if (img.width, img.height) != (mask.width, mask.height) {
                    return None;
                }
                for (p, a) in img.rgba.chunks_exact_mut(4).zip(mask.rgba.chunks_exact(4)) {
                    p[3] = a[0].max(a[1]).max(a[2]);
                }
                let t = Texture2D::from_rgba8(u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?, &img.rgba);
                t.set_filter(FilterMode::Linear);
                Some(t)
            })
            .clone()
    })
}

/// One time button; true when pressed (its sound plays: `InterfaceButtonDown`, §14).
fn time_button(b: TimeButton, r: Rect) -> (bool, Option<String>) {
    let (hover, down, pressed) = crate::ui::widgets::button_state(r, true);
    let (up_art, down_art, alpha) = b.art();
    match masked(if down { down_art } else { up_art }, alpha) {
        Some(t) => tex(&t, r, WHITE),
        None => {
            // Without the install: an oval in the art's colours (red for the waits, green
            // for the hero) with a label.
            let base = match b {
                TimeButton::ShowHero => Color::new(0.12, 0.5, 0.2, 1.0),
                _ => Color::new(0.62, 0.12, 0.08, 1.0),
            };
            let c = if down { Color::new(base.r * 0.7, base.g * 0.7, base.b * 0.7, 1.0) } else { base };
            draw_ellipse(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, c);
            draw_ellipse_lines(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, 3.0, chrome::SILVER);
            let label = match b {
                TimeButton::Wait1 => tr("1 h"),
                TimeButton::ShowHero => tr("Hero"),
                TimeButton::Wait4 => tr("4 h"),
            };
            let s = fit_size(label, r.w - 10.0, (r.h * 0.4).round());
            shadow_centered(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + s * 0.36, s, WHITE);
        }
    }
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    (pressed, hover.then(|| b.hint()))
}

/// How a button looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Normal,
    /// A window is open or the button does nothing now.
    Grey,
    /// Its screen is open.
    Lit,
    /// The minimap is open.
    Glow,
}

/// The stat line's four items (mana, gold, income, wages) from the left: where each item's centre
/// stands along the bar's lower strip. An item's icon is at its quarter of the bar's width; its
/// label stands left of it and its value right of it. An item moves inward by the least shift that
/// keeps both of its ends (the label's start, the value's end, at the baseline's lowest ink) off
/// the rounded bottom corners (`safe`): the first two move right, the last two left, by at most
/// half the bar. Razdor's phone choice: the original's stat line has no corners to keep off.
/// `widths` are each item's (label, value) text widths, `base` the baseline's y, `size` the text's
/// size. The lowest ink is a quarter of the size below the baseline, or the screen's edge where
/// that is lower (the bar sits at the edge, so the descenders are cut there). With no corners
/// nothing moves.
pub fn stat_centres(bar: Rect, screen: Vec2, safe: SafeArea, k: f32, widths: [(f32, f32); 4], base: f32, size: f32) -> [f32; 4] {
    let low = (base + 0.25 * size).min(screen.y);
    let mut out = [0.0; 4];
    for (i, (label_w, value_w)) in widths.into_iter().enumerate() {
        let nominal = bar.x + bar.w * (0.125 + 0.25 * i as f32);
        // The label's start and the value's end, for the icon at `cx`.
        let ends = |cx: f32| [cx - label_w - 16.0 * k, cx + 16.0 * k + value_w];
        let ok = |cx: f32| ends(cx).into_iter().all(|x| on_screen(vec2(x, low), screen, safe));
        let sign = if i < 2 { 1.0 } else { -1.0 };
        let shift = smallest_shift(bar.w / 2.0, |s| ok(nominal + sign * s)).unwrap_or(0.0);
        out[i] = nominal + sign * shift;
    }
    out
}

/// The four resources along the bar's lower strip, inside the bar's width `bar` (not the
/// screen's: the bar keeps its own width on a wide screen). Each item moves off the rounded
/// bottom corners (`stat_centres`).
fn resources(game: &Game, y: f32, h: f32, bar: Rect, safe: SafeArea, screen: Vec2) {
    let k = chrome::bar_k();
    let mut wages = format!("- {}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / {}", game.daily_mana_wages());
    }
    let items = [
        (tr("mana"), "Res-Magic", game.mana.to_string(), Color::new(0.45, 0.85, 1.0, 1.0)),
        (tr("gold"), "Res-Money", game.gold.to_string(), GOLD),
        (tr("income"), "Res-Income", format!("+ {}", game.daily_income()), GOLD),
        (tr("wages"), "Res-Payment", wages, Color::new(1.0, 0.62, 0.25, 1.0)),
    ];
    let value_size = super::typography::Role::Name.size((19.0 * k).round());
    let label_size = super::typography::Role::Body.size((19.0 * k).round());
    let base = y + h * 0.5 + value_size * 0.36;
    let widths = items.each_ref().map(|(label, _, value, _)| {
        let lw = super::dt_font::with_face(super::dt_font::Face::Body, || measure(label, label_size).width);
        let vw = super::dt_font::with_face(super::dt_font::Face::Subtitle, || measure(value, value_size).width);
        (lw, vw)
    });
    let centres = stat_centres(bar, screen, safe, k, widths, base, value_size.max(label_size));
    for (i, (label, art, value, color)) in items.iter().enumerate() {
        let cx = centres[i];
        let label_x = cx - widths[i].0 - 16.0 * k;
        let icon = 24.0 * k;
        // Each picture with its own alpha plane and nothing else, as the original's
        // (0x4dcd8d loads them, 0x49d3e5 draws them through 0x476a3c).
        match chrome::win(art) {
            Some(t) => {
                let iw = t.width() * icon / t.height();
                tex(&t, Rect::new(cx - iw / 2.0, y + (h - icon) / 2.0, iw, icon), WHITE);
            }
            None => super::dialog::resource_icon(
                [super::dialog::Resource::Mana, super::dialog::Resource::Gold, super::dialog::Resource::Income, super::dialog::Resource::Wages][i],
                cx,
                y + h / 2.0,
                icon,
            ),
        }
        super::dt_font::with_face(super::dt_font::Face::Body, || {
            shadow_text(label, label_x, base, label_size, *color);
        });
        super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
            shadow_text(value, cx + 16.0 * k, base, value_size, *color);
        });
    }
}

fn lower_strip_gradient(bottom_tone: f32) -> Texture2D {
    local! {
        static TEXTURE: std::cell::RefCell<Option<(u8, Texture2D)>> = const { std::cell::RefCell::new(None) };
    }
    let bottom = (bottom_tone.clamp(0.0, 1.0) * 255.0).round() as u8;
    TEXTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|(saved, _)| *saved != bottom) {
            let height = 128usize;
            let mut rgba = Vec::with_capacity(height * 4);
            for y in 0..height {
                let value = (y as f32 / (height - 1) as f32 * bottom as f32).round() as u8;
                rgba.extend_from_slice(&[value, value, value, 255]);
            }
            let texture = Texture2D::from_rgba8(1, height as u16, &rgba);
            texture.set_filter(FilterMode::Linear);
            *slot = Some((bottom, texture));
        }
        slot.as_ref().unwrap().1.clone()
    })
}

/// One oval button; true when clicked (never while greyed).
fn oval(b: BarButton, r: Rect, look: Look) -> (bool, Option<String>) {
    let (hover, down, pressed) = crate::ui::widgets::button_state(r, look != Look::Grey);
    let side = if BarButton::LEFT.contains(&b) { "ML" } else { "MR" };
    let size = if b.small() { 2 } else { 1 };
    let state = if down { "Down" } else { "Up" };
    let name = format!("{side}Btn{size}{state}");
    let (fx, tint) = match look {
        // The video's buttons are a deeper blue than the art alone.
        Look::Normal if hover => (Fx::Plain, Color::new(0.8, 0.98, 1.0, 1.0)),
        Look::Normal => (Fx::Plain, Color::new(0.6, 0.82, 0.86, 1.0)),
        Look::Grey => (Fx::Grey, WHITE),
        Look::Lit => (Fx::Grey, Color::new(0.45, 1.0, 0.45, 1.0)),
        Look::Glow => (Fx::Grey, Color::new(1.0, 0.55, 0.2, 1.0)),
    };
    match chrome::win_fx(&name, fx) {
        Some(t) => tex(&t, r, tint),
        None => {
            let base = match look {
                Look::Normal => Color::new(0.16, 0.30, 0.62, 1.0),
                Look::Grey => Color::new(0.33, 0.33, 0.35, 1.0),
                Look::Lit => Color::new(0.2, 0.55, 0.22, 1.0),
                Look::Glow => Color::new(0.75, 0.38, 0.1, 1.0),
            };
            let c = if hover { Color::new(base.r * 1.3, base.g * 1.3, base.b * 1.3, 1.0) } else { base };
            draw_ellipse(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, c);
            draw_ellipse_lines(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, 2.0, chrome::SILVER);
        }
    }
    // The glyph: white-on-black art made to glow, tinted by the button's state.
    let glyph = match look {
        // The video's cyan glyphs: (92, 216, 207).
        Look::Normal => Color::new(0.36, 0.85, 0.81, 1.0),
        Look::Grey => Color::new(0.85, 0.85, 0.85, 0.9),
        Look::Lit => Color::new(0.75, 1.0, 0.75, 1.0),
        Look::Glow => Color::new(1.0, 0.85, 0.5, 1.0),
    };
    if let Some(t) = chrome::win_fx(&format!("button-icon-{}", b.icon()), Fx::Glow) {
        let k = r.h / 54.0 * if b.small() { 1.25 } else { 1.2 };
        let (w, h) = (t.width() * k, t.height() * k);
        tex(&t, Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h), glyph);
    } else {
        let label = match b {
            BarButton::Menu => "X",
            BarButton::Settings => tr("Opt"),
            BarButton::Save => tr("Save"),
            BarButton::Load => tr("Load"),
            BarButton::Hero => tr("Hero"),
            BarButton::Squad => tr("Army"),
            BarButton::Spells => tr("Book"),
            BarButton::Map => tr("Map"),
        };
        let s = fit_size(label, r.w - 8.0, (r.h * 0.34).round());
        shadow_centered(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + s * 0.36, s, glyph);
    }
    // A panel icon's own sound (interface.md §14); the window it opens is silent.
    if pressed {
        super::audio::cue(super::audio::Cue::Panel);
    }
    (pressed, hover.then(|| b.hint()))
}

/// Draws the bar. `look` says how each button looks; returns the button clicked.
pub fn draw(game: &Game, look: impl Fn(BarButton) -> Look) -> Option<BarButton> {
    draw_with_time(game, look, false).0
}

/// Draws the bar; `time_buttons`: the map is idle, so hovering the message box shows the
/// three time buttons over it instead of its text. Returns the bar button and the time
/// button pressed.
pub fn draw_with_time(game: &Game, look: impl Fn(BarButton) -> Look, time_buttons: bool) -> (Option<BarButton>, Option<TimeButton>) {
    // The bar stays at the screen's bottom whatever the windows' pan is (`ui::pan`), and at
    // the fitted scale, its text included (`display::with_fitted`).
    super::pan::unpanned(|| super::display::with_fitted(super::chrome::fit_k(), || draw_bar(game, &look, time_buttons)))
}

fn draw_bar(game: &Game, look: &impl Fn(BarButton) -> Look, time_buttons: bool) -> (Option<BarButton>, Option<TimeButton>) {
    let k = chrome::bar_k();
    let (w, h) = (screen_width(), screen_height());
    let row = (60.0 * k).round();
    let lay = current_layout();
    let bar = lay.bar;
    let y = bar.y;
    // The top row: the original's ribbon (`Win2a`), its ends kept, the wooden middle stretched.
    match chrome::win("Win2a") {
        Some(t) => {
            let s = row / t.height();
            let end = 370.0;
            let ew = end * s;
            chrome::tex_src(&t, Rect::new(0.0, 0.0, end, t.height()), Rect::new(bar.x, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(t.width() - end, 0.0, end, t.height()), Rect::new(bar.x + bar.w - ew, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(end, 0.0, t.width() - 2.0 * end, t.height()), Rect::new(bar.x + ew, y, bar.w - 2.0 * ew, row), WHITE);
        }
        None => {
            chrome::surface(Rect::new(bar.x, y, bar.w, row), chrome::Skin::Marble);
            let ew = 370.0 * row / 64.0;
            draw_rectangle(bar.x + ew, y + 3.0, bar.w - 2.0 * ew, row - 6.0, Color::new(0.42, 0.22, 0.10, 1.0));
            draw_rectangle_lines(bar.x + ew, y + 3.0, bar.w - 2.0 * ew, row - 6.0, 2.0, chrome::SILVER);
            draw_rectangle_lines(bar.x, y, bar.w, row, 2.0, chrome::SILVER);
        }
    }
    chrome::silver_line(vec2(bar.x, y), vec2(bar.x + bar.w, y), k.max(1.0));
    // The lower strip, the bar's own width.
    let sy = y + row;
    let sh = bar.h - row;
    let visual = super::window_layout::visual();
    chrome::tex(&lower_strip_gradient(visual.bar_gradient_bottom), Rect::new(bar.x, sy, bar.w, sh), WHITE);
    if let Some(t) = chrome::bottom_bar_ornament() {
        let ow = t.width() * sh / t.height();
        let mut x = bar.x + bar.w * 0.25 - ow / 2.0;
        while x < bar.x + bar.w {
            tex(&t, Rect::new(x, sy, ow, sh), Color::new(1.0, 1.0, 1.0, visual.bar_ornament_opacity));
            x += bar.w * 0.25;
        }
    }
    resources(game, sy, sh, bar, safe_area(), vec2(w, h));

    // The time panel.
    let cx = w / 2.0;
    let size = super::typography::Role::Bold.size((13.0 * k).round());
    // "Время:", the date, and while walking "До конца пути: 4 час" (the video's bar).
    let mut lines = vec![(tr("Time:").to_string(), Color::new(1.0, 0.93, 0.55, 1.0)), (game.clock.label(), WHITE)];
    if game.moving() {
        lines.push((trf!("Path left: {left}", left = duration_label(game.minutes_left() as f64)), GOLD));
    } else if let Some((_, left)) = game.reading() {
        // "Чтение: 2 час", the reading time as the original's book gives it.
        lines.push((trf!("Reading: {left}", left = duration_label(left as f64)), GOLD));
    } else if game.waiting() {
        lines.push((tr("waiting…").to_string(), GOLD));
    }
    // Russian dates run longer: the lines shrink to the time panel's width.
    let room = (lay.time.w - 16.0 * k).max(60.0 * k);
    let size = super::typography::Role::Bold.with(|| lines.iter().map(|(l, _)| fit_size(l, room, size)).fold(size, f32::min));
    let lh = super::typography::Role::Bold.line_height(size * 1.25);
    let top = y + row / 2.0 - lh * (lines.len() as f32 - 1.0) / 2.0 + size * 0.36;
    // The message box hovered on the idle map: the three buttons stand over it, its text
    // hidden (as the original draws it).
    let s = row / PANEL_ROW;
    let pointer = Vec2::from(super::widgets::pointer());
    let buttons = time_buttons && !super::widgets::input_blocked() && message_box(y, cx, s).contains(pointer);
    let mut timed = None;
    let mut timed_hint = None;
    if buttons {
        for b in TimeButton::ALL {
            let (clicked, hint) = time_button(b, time_button_rect(b, y, cx, s));
            if clicked {
                timed = Some(b);
            }
            timed_hint = hint.or(timed_hint);
        }
    } else {
        super::dt_font::with_face(super::dt_font::Face::Bold, || {
            for (i, (l, c)) in lines.iter().enumerate() {
                shadow_centered(&super::widgets::ellipsize(l, room, size), lay.time.x + lay.time.w / 2.0, top + i as f32 * lh, size, *c);
            }
        });
    }

    // The buttons, at their laid-out rects.
    let mut pressed = None;
    let mut hovered_hint = None;
    for (i, b) in BarButton::LEFT.iter().copied().enumerate() {
        let (clicked, hint) = oval(b, lay.left[i], look(b));
        if clicked {
            pressed = Some(b);
        }
        hovered_hint = hint.or(hovered_hint);
    }
    for (i, b) in BarButton::RIGHT.iter().copied().enumerate().rev() {
        let (clicked, hint) = oval(b, lay.right[i], look(b));
        if clicked {
            pressed = Some(b);
        }
        hovered_hint = hint.or(hovered_hint);
    }
    // Both are hover hints: none after a phone's finger lifted (`input::touch_hover`).
    if let Some(hint) = timed_hint.or(hovered_hint).filter(|_| super::input::touch_hover()) {
        panel_hint(&[(hint, CREAM)]);
    }
    (pressed, timed)
}

/// Whether the point `p` is on the screen (`size`) with its rounded corners (the phone's radii,
/// `display::SafeArea`): outside a corner's arc is off the screen.
pub fn on_screen(p: Vec2, size: Vec2, safe: SafeArea) -> bool {
    if p.x < 0.0 || p.y < 0.0 || p.x > size.x || p.y > size.y {
        return false;
    }
    // A corner's arc is the circle that sits in its square: inside the square, the circle decides.
    let corners = [(safe.corner_tl, vec2(0.0, 0.0)), (safe.corner_tr, vec2(size.x, 0.0)), (safe.corner_br, size), (safe.corner_bl, vec2(0.0, size.y))];
    for (r, corner) in corners {
        if r > 0.0 && (p.x - corner.x).abs() <= r && (p.y - corner.y).abs() <= r {
            let centre = vec2(if corner.x > 0.0 { corner.x - r } else { r }, if corner.y > 0.0 { corner.y - r } else { r });
            return (p - centre).length() <= r;
        }
    }
    true
}

/// Whether the disc of radius `rad` at `c` lies on the screen: its centre and 64 points of its
/// rim are on it ([`on_screen`]).
pub fn disc_on_screen(c: Vec2, rad: f32, size: Vec2, safe: SafeArea) -> bool {
    on_screen(c, size, safe)
        && (0..64).all(|i| {
            let a = i as f32 / 64.0 * std::f32::consts::TAU;
            on_screen(c + rad * vec2(a.cos(), a.sin()), size, safe)
        })
}

/// The least shift in `[0, max]` for which `ok` holds, found by bisection to half a pixel; `ok`
/// must turn true as the shift grows. None when even `max` is not enough.
pub fn smallest_shift(max: f32, ok: impl Fn(f32) -> bool) -> Option<f32> {
    if ok(0.0) {
        return Some(0.0);
    }
    if !ok(max) {
        return None;
    }
    let (mut lo, mut hi) = (0.0f32, max);
    while hi - lo > 0.5 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

/// Razdor's bar is the video's 960 reference px wide at the bar's scale, centred; on a wider
/// screen the map shows beside it (Razdor's choice, not the original's 800 px bar).
const BAR_W: f32 = 960.0;

/// The bar's place on the screen: its natural width, centred, at the screen's bottom.
pub fn bar_rect() -> Rect {
    let (w, h) = (screen_width(), screen_height());
    let bar = chrome::bar_height();
    let bw = (BAR_W * chrome::bar_k()).round().min(w);
    Rect::new(((w - bw) / 2.0).round(), h - bar, bw, bar)
}

/// A bar button's size at the bar's row height `row` (the original's art sizes).
fn button_size(b: BarButton, row: f32) -> (f32, f32) {
    let (h, w) = if b.small() { (44.0, 80.0) } else { (54.0, 98.0) };
    (w * row / 64.0, h * row / 64.0)
}

/// The bar's pieces on the screen: every button's rect (the one place the drawing, the hit
/// tests, the hints and the sounds take them from), the time panel between the groups.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub bar: Rect,
    /// The left group in [`BarButton::LEFT`] order, the right one in [`BarButton::RIGHT`] order.
    pub left: [Rect; 4],
    pub right: [Rect; 4],
    pub time: Rect,
}

/// The bar's layout on a `screen` (px) at the bar's scale `k` with the row height `row`, for the
/// phone's safe area `safe`. Each group moves inward by the least shift that keeps every
/// button's disc (70 % of its radius, [`disc_on_screen`]) off the rounded bottom corners and
/// inside the cutout's insets; with no corners and no insets nothing moves.
pub fn layout(bar: Rect, screen: Vec2, k: f32, row: f32, safe: SafeArea) -> Layout {
    let left_at = |s: f32| {
        let mut x = bar.x + 8.0 * k + s;
        let mut out = [Rect::default(); 4];
        for (i, b) in BarButton::LEFT.iter().enumerate() {
            let (w, h) = button_size(*b, row);
            out[i] = Rect::new(x, bar.y + (row - h) / 2.0, w, h);
            x += w;
        }
        out
    };
    let right_at = |s: f32| {
        let mut x = bar.x + bar.w - 8.0 * k - s;
        let mut out = [Rect::default(); 4];
        for (i, b) in BarButton::RIGHT.iter().enumerate().rev() {
            let (w, h) = button_size(*b, row);
            x -= w;
            out[i] = Rect::new(x, bar.y + (row - h) / 2.0, w, h);
        }
        out
    };
    let fits = |r: &Rect| {
        let rad = 0.7 * r.w.min(r.h) / 2.0;
        disc_on_screen(vec2(r.x + r.w / 2.0, r.y + r.h / 2.0), rad, screen, safe)
    };
    let left_ok = |s: f32| left_at(s).iter().all(|r| r.x >= safe.left && fits(r));
    let right_ok = |s: f32| right_at(s).iter().all(|r| r.x + r.w <= screen.x - safe.right && fits(r));
    // A group moves at most half the bar: past that the far buttons would leave the bar (and
    // the ok test would stop being monotone there). The corners only need the first few pixels.
    let max = bar.w / 2.0;
    let ls = smallest_shift(max, left_ok).unwrap_or(0.0);
    let rs = smallest_shift(max, right_ok).unwrap_or(0.0);
    let left = left_at(ls);
    let right = right_at(rs);
    let ew = 370.0 * row / 64.0;
    let x0 = (bar.x + ew).max(left[3].x + left[3].w);
    let x1 = (bar.x + bar.w - ew).min(right[0].x);
    Layout { bar, left, right, time: Rect::new(x0, bar.y, (x1 - x0).max(0.0), row) }
}

/// The layout the bar is drawn and hit with now (the screen's size, the safe area).
pub fn current_layout() -> Layout {
    let bar = bar_rect();
    let row = (60.0 * chrome::bar_k()).round();
    let screen = vec2(screen_width(), screen_height());
    layout(bar, screen, chrome::bar_k(), row, safe_area())
}

/// The time panel in the middle of the bar (clicks on it do not reach the map).
pub fn time_panel() -> Rect {
    current_layout().time
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_left_buttons_stand_and_look_as_the_original() {
        // 4d46e0: ids 1–4 from the edge inwards are exit, options, load, save; save shows
        // icon_3 and load icon_4.
        assert_eq!(BarButton::LEFT, [BarButton::Menu, BarButton::Settings, BarButton::Load, BarButton::Save]);
        assert_eq!((BarButton::Save.icon(), BarButton::Load.icon()), (3, 4));
    }

    /// The button under `p` when the buttons show: only while the map is idle (`shown`) and
    /// the pointer is on the message box, as the original's hover area (0x4b930c, 0x4b93a8).
    /// The drawing takes the same area ([`message_box`]).
    fn time_button_at(p: Vec2, shown: bool, top: f32, mid: f32, s: f32) -> Option<TimeButton> {
        if !shown || !message_box(top, mid, s).contains(p) {
            return None;
        }
        TimeButton::ALL.into_iter().find(|&b| time_button_rect(b, top, mid, s).contains(p))
    }

    /// The original's own layout: its bar at y = 682, the middle at 512, scale 1.
    fn at(x: f32, y: f32, shown: bool) -> Option<TimeButton> {
        time_button_at(vec2(x, y), shown, 682.0, 512.0, 1.0)
    }

    #[test]
    fn the_buttons_stand_where_the_original_puts_them() {
        assert_eq!(time_button_rect(TimeButton::Wait1, 682.0, 512.0, 1.0), Rect::new(378.0, 693.0, 80.0, 42.0));
        assert_eq!(time_button_rect(TimeButton::ShowHero, 682.0, 512.0, 1.0), Rect::new(461.0, 687.0, 102.0, 54.0));
        assert_eq!(time_button_rect(TimeButton::Wait4, 682.0, 512.0, 1.0), Rect::new(566.0, 693.0, 80.0, 42.0));
        assert_eq!(message_box(682.0, 512.0, 1.0), Rect::new(372.0, 684.0, 280.0, 60.0));
        // The harness's measured presses in the running original (tools/difftest).
        assert_eq!(at(418.0, 713.0, true), Some(TimeButton::Wait1));
        assert_eq!(at(607.0, 713.0, true), Some(TimeButton::Wait4));
        assert_eq!(at(512.0, 708.0, true), Some(TimeButton::ShowHero));
    }

    #[test]
    fn only_on_the_message_box_and_only_while_shown() {
        // Between the buttons, and outside the box: none.
        assert_eq!(at(459.5, 713.0, true), None);
        assert_eq!(at(512.0, 760.0, true), None);
        assert_eq!(at(300.0, 713.0, true), None);
        // The map not idle: no buttons at all.
        for b in TimeButton::ALL {
            let r = time_button_rect(b, 682.0, 512.0, 1.0);
            assert_eq!(at(r.x + r.w / 2.0, r.y + r.h / 2.0, false), None);
            assert_eq!(at(r.x + r.w / 2.0, r.y + r.h / 2.0, true), Some(b));
        }
    }

    #[test]
    fn the_layout_scales_around_the_middle() {
        // A bar 1.5 times the original's on a wider screen: the same place, scaled.
        let r = time_button_rect(TimeButton::ShowHero, 900.0, 800.0, 1.5);
        assert_eq!(r, Rect::new(800.0 - 51.0 * 1.5, 900.0 + 5.0 * 1.5, 153.0, 81.0));
        assert_eq!(time_button_at(vec2(800.0, 940.0), true, 900.0, 800.0, 1.5), Some(TimeButton::ShowHero));
    }

    fn bar_on(screen: Vec2, k: f32, x: f32, w: f32) -> Rect {
        let h = (80.0 * k).round();
        Rect::new(x, screen.y - h, w, h)
    }

    /// Whether the button's disc (70 % of its radius) lies on the screen with those corners.
    fn disc_clear(r: &Rect, screen: Vec2, safe: SafeArea) -> bool {
        disc_on_screen(vec2(r.x + r.w / 2.0, r.y + r.h / 2.0), 0.7 * r.w.min(r.h) / 2.0, screen, safe)
    }

    #[test]
    fn no_insets_no_shift() {
        // A 2376x1080 phone (2.625 px per dp): the bar is 960 px at the scale 1.5, centred.
        let screen = vec2(2376.0, 1080.0);
        let (k, row) = (1.5, 90.0);
        let bar = bar_on(screen, k, 468.0, 1440.0);
        let lay = layout(bar, screen, k, row, SafeArea::default());
        assert_eq!(lay.left[0].x, bar.x + 8.0 * k);
        assert_eq!(lay.right[3].x + lay.right[3].w, bar.x + bar.w - 8.0 * k);
        // The time panel stands between the ribbon's ends (the original's placement).
        let ew = 370.0 * row / 64.0;
        assert_eq!(lay.time, Rect::new(bar.x + ew, bar.y, bar.w - 2.0 * ew, row));
    }

    #[test]
    fn a_corner_moves_the_end_buttons_inwards_only_as_far_as_the_disc_needs() {
        // A portrait phone: the bar spans the screen, its bottom corners are `corner` px.
        let screen = vec2(1080.0, 2376.0);
        let (k, row) = (1.125, 68.0);
        let bar = bar_on(screen, k, 0.0, 1080.0);
        let at = |corner: f32| {
            let safe = SafeArea { corner_bl: corner, corner_br: corner, ..Default::default() };
            (layout(bar, screen, k, row, safe), safe)
        };
        // 120 px: the Menu button's disc (70 % of its radius) is already inside the arc, so
        // nothing moves (computed: its far point is 110 px from the arc's centre).
        let (lay, _) = at(120.0);
        assert_eq!(lay.left[0].x, bar.x + 8.0 * k);
        // 200 px: the end buttons move in, and every disc is clear afterwards.
        let (lay, safe) = at(200.0);
        assert!(lay.left[0].x > bar.x + 8.0 * k, "the Menu button moves in");
        assert!(lay.right[3].x + lay.right[3].w < bar.x + bar.w - 8.0 * k, "the Map button moves in");
        for r in lay.left.iter().chain(lay.right.iter()) {
            assert!(disc_clear(r, screen, safe), "{r:?} is clear of the corners");
        }
        // The shift is the least one (to half a pixel): 2 px nearer the middle would be cut.
        let less = Rect { x: lay.left[0].x - 2.0, ..lay.left[0] };
        assert!(!disc_clear(&less, screen, safe));
    }

    #[test]
    fn a_left_cutout_keeps_the_left_group_after_it() {
        let screen = vec2(2376.0, 1080.0);
        let bar = bar_on(screen, 1.5, 0.0, 1440.0);
        let safe = SafeArea { left: 90.0, ..Default::default() };
        let lay = layout(bar, screen, 1.5, 90.0, safe);
        // The bisection stops within half a pixel above the cutout's edge.
        assert!((90.0..=90.5).contains(&lay.left[0].x), "{}", lay.left[0].x);
    }

    #[test]
    fn the_smallest_shift_is_found_by_bisection() {
        let got = smallest_shift(100.0, |s| s >= 30.0).unwrap();
        assert!((30.0..=30.5).contains(&got), "got {got}");
        assert_eq!(smallest_shift(100.0, |_| true), Some(0.0));
        assert_eq!(smallest_shift(100.0, |_| false), None);
    }

    #[test]
    fn corners_cut_the_arc_and_only_the_arc() {
        let size = vec2(1000.0, 1000.0);
        let bl = SafeArea { corner_bl: 120.0, ..Default::default() };
        // Inside the arc: on screen; in the cut-off corner: not.
        assert!(on_screen(vec2(120.0, 880.0), size, bl));
        assert!(!on_screen(vec2(5.0, 995.0), size, bl));
        // The bottom-right corner does not depend on the left one.
        assert!(on_screen(vec2(5.0, 995.0), size, SafeArea::default()));
        let br = SafeArea { corner_br: 120.0, ..Default::default() };
        assert!(!on_screen(vec2(995.0, 995.0), size, br));
        // The top corners, the landscape's bottom ones turned.
        let tl = SafeArea { corner_tl: 120.0, ..Default::default() };
        assert!(!on_screen(vec2(5.0, 5.0), size, tl) && on_screen(vec2(120.0, 120.0), size, tl));
        let tr = SafeArea { corner_tr: 120.0, ..Default::default() };
        assert!(!on_screen(vec2(995.0, 5.0), size, tr) && on_screen(vec2(880.0, 120.0), size, tr));
        assert!(on_screen(vec2(995.0, 5.0), size, SafeArea { corner_bl: 120.0, corner_br: 120.0, ..Default::default() }));
    }

    /// A portrait phone's stat line: the four items (widths in px) under the bottom corners.
    /// Returns the bar, the screen, the centres, and the ends of each item for them.
    fn stat_line(safe: SafeArea) -> (Vec2, [f32; 4], [(f32, f32); 4]) {
        let screen = vec2(1080.0, 2160.0);
        let (k, size) = (1.125, 21.0);
        let bar = bar_on(screen, k, 0.0, 1080.0);
        let widths = [(60.0, 30.0), (70.0, 25.0), (70.0, 50.0), (70.0, 60.0)];
        // The baseline as `resources` puts it: the middle of the lower strip (below the row).
        let row = (60.0 * k).round();
        let base = bar.y + row + (bar.h - row) / 2.0 + 0.36 * size;
        let centres = stat_centres(bar, screen, safe, k, widths, base, size);
        // The four items' label starts and value ends, as `resources` draws them.
        let ends = [0, 1, 2, 3].map(|i| (centres[i] - widths[i].0 - 16.0 * k, centres[i] + 16.0 * k + widths[i].1));
        (screen, centres, ends)
    }

    #[test]
    fn the_stat_line_keeps_its_text_off_the_bottom_corners() {
        // 110 px corners: the first label (its start at 57 px) and the last value (its end at
        // 1023 px) would be cut without a move.
        let safe = SafeArea { corner_bl: 110.0, corner_br: 110.0, ..Default::default() };
        let (screen, centres, ends) = stat_line(safe);
        // The descenders reach the screen's edge (the baseline is a few px above it).
        let low = screen.y;
        assert!(centres[0] > 135.0, "the mana label moves in: {}", centres[0]);
        assert!(centres[3] < 945.0, "the wages value moves in: {}", centres[3]);
        for (a, b) in ends {
            assert!(on_screen(vec2(a, low), screen, safe), "label start {a} is clear of the corner");
            assert!(on_screen(vec2(b, low), screen, safe), "value end {b} is clear of the corner");
        }
    }

    #[test]
    fn the_stat_line_is_unchanged_without_corners() {
        let (_, centres, _) = stat_line(SafeArea::default());
        assert_eq!(centres, [135.0, 405.0, 675.0, 945.0]);
    }
}
