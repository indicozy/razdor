//! The bottom game bar, as the original's (video notes §1, ref 06): four oval buttons on
//! the left (menu, settings, save, load), the time panel in the middle, four on the right
//! (journal, hero and army, spell book, map), and under them the strip with mana, gold,
//! income and wages. The buttons are blue, grey while a window is open, green for the open
//! screen and orange while the minimap shows. Hovering the time panel's message box on the
//! idle map shows the original's three small buttons on it (interface.md §6): wait 1 hour,
//! centre the view on the hero, wait 4 hours.

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::clock::duration_label;
use razdor::rules::game::Game;
use razdor::trf;

use super::chrome::{self, shadow_centered, shadow_text, tex, three_slice, Fx, CREAM, GOLD};
use super::widgets::{clicked, fit_size, measure, mouse_in, panel_hint};

/// A bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    Menu,
    Settings,
    Save,
    Load,
    Journal,
    Squad,
    Spells,
    Map,
}

impl BarButton {
    /// From the screen's left edge inwards, as the original's ids 1–4 (4d46e0): exit menu,
    /// options, load, and save next to the message box.
    const LEFT: [BarButton; 4] = [BarButton::Menu, BarButton::Settings, BarButton::Load, BarButton::Save];
    const RIGHT: [BarButton; 4] = [BarButton::Journal, BarButton::Squad, BarButton::Spells, BarButton::Map];

    fn icon(self) -> usize {
        match self {
            BarButton::Menu => 1,
            BarButton::Settings => 2,
            // Crossed in the original's files: save (id 4) shows `icon_3`, load (id 3) `icon_4`.
            BarButton::Save => 3,
            BarButton::Load => 4,
            BarButton::Journal => 5,
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
            BarButton::Journal => "cHero",
            BarButton::Squad => "cArmy",
            BarButton::Spells => "cMagic",
            BarButton::Map => "cMap",
        };
        let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("GameMenu", key)).flatten();
        own.unwrap_or_else(|| self.our_hint().to_string())
    }

    fn our_hint(self) -> &'static str {
        match self {
            BarButton::Menu => tr("Main menu (Esc)"),
            BarButton::Settings => tr("Sound settings"),
            BarButton::Save => tr("Save the game (F5: quick save)"),
            BarButton::Load => tr("Load a saved game (F9: quick load)"),
            BarButton::Journal => tr("The hero's journal (J)"),
            BarButton::Squad => tr("The hero and his army (A)"),
            BarButton::Spells => tr("The spell book (B)"),
            BarButton::Map => tr("Map of the scenario (M)"),
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

/// The button under `p` when the buttons show: only while the map is idle (`shown`) and
/// the pointer is on the message box, as the original's hover area (0x4b930c, 0x4b93a8).
pub fn time_button_at(p: Vec2, shown: bool, top: f32, mid: f32, s: f32) -> Option<TimeButton> {
    if !shown || !message_box(top, mid, s).contains(p) {
        return None;
    }
    TimeButton::ALL.into_iter().find(|&b| time_button_rect(b, top, mid, s).contains(p))
}

thread_local! {
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
fn time_button(b: TimeButton, r: Rect, hover: bool) -> bool {
    let down = hover && is_mouse_button_down(MouseButton::Left);
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
    if hover {
        panel_hint(&[(b.hint(), CREAM)]);
    }
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    pressed
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

/// The four resources along the bar's lower strip.
fn resources(game: &Game, y: f32, h: f32) {
    let k = chrome::k();
    let w = screen_width();
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
    let label_size = (11.0 * k).round();
    let value_size = (19.0 * k).round();
    for (i, (label, art, value, color)) in items.iter().enumerate() {
        let cx = w * (0.125 + 0.25 * i as f32);
        let lw = measure(label, label_size).width;
        let base = y + h * 0.5 + value_size * 0.36;
        shadow_text(label, cx - lw - 16.0 * k, base, label_size, Color::new(0.75, 0.75, 0.75, 1.0));
        let icon = 24.0 * k;
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
        super::dt_font::with_face(super::dt_font::Face::Subtitle, || shadow_text(value, cx + 16.0 * k, base, value_size, *color));
    }
}

/// One oval button; true when clicked (never while greyed).
fn oval(b: BarButton, r: Rect, look: Look) -> bool {
    let hover = look != Look::Grey && mouse_in(r.x, r.y, r.w, r.h);
    let side = if BarButton::LEFT.contains(&b) { "ML" } else { "MR" };
    let size = if b.small() { 2 } else { 1 };
    let state = if hover && is_mouse_button_down(MouseButton::Left) { "Down" } else { "Up" };
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
            BarButton::Journal => tr("Quests"),
            BarButton::Squad => tr("Army"),
            BarButton::Spells => tr("Book"),
            BarButton::Map => tr("Map"),
        };
        let s = fit_size(label, r.w - 8.0, (r.h * 0.34).round());
        shadow_centered(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + s * 0.36, s, glyph);
    }
    if hover {
        panel_hint(&[(b.hint().to_string(), CREAM)]);
    }
    // A panel icon's own sound (interface.md §14); the window it opens is silent.
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Panel);
    }
    pressed
}

/// Draws the bar. `look` says how each button looks; returns the button clicked.
pub fn draw(game: &Game, look: impl Fn(BarButton) -> Look) -> Option<BarButton> {
    draw_with_time(game, look, false).0
}

/// Draws the bar; `time_buttons`: the map is idle, so hovering the message box shows the
/// three time buttons over it instead of its text. Returns the bar button and the time
/// button pressed.
pub fn draw_with_time(game: &Game, look: impl Fn(BarButton) -> Look, time_buttons: bool) -> (Option<BarButton>, Option<TimeButton>) {
    let k = chrome::k();
    let (w, h) = (screen_width(), screen_height());
    let bar = chrome::bar_height();
    let y = h - bar;
    let row = (60.0 * k).round();
    // The top row: the original's ribbon (`Win2a`), its ends kept, the wooden middle stretched.
    match chrome::win("Win2a") {
        Some(t) => {
            let s = row / t.height();
            let end = 370.0;
            let ew = end * s;
            chrome::tex_src(&t, Rect::new(0.0, 0.0, end, t.height()), Rect::new(0.0, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(t.width() - end, 0.0, end, t.height()), Rect::new(w - ew, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(end, 0.0, t.width() - 2.0 * end, t.height()), Rect::new(ew, y, w - 2.0 * ew, row), WHITE);
        }
        None => {
            chrome::surface(Rect::new(0.0, y, w, row), chrome::Skin::Marble);
            let ew = 370.0 * row / 64.0;
            draw_rectangle(ew, y + 3.0, w - 2.0 * ew, row - 6.0, Color::new(0.42, 0.22, 0.10, 1.0));
            draw_rectangle_lines(ew, y + 3.0, w - 2.0 * ew, row - 6.0, 2.0, chrome::SILVER);
            draw_rectangle_lines(0.0, y, w, row, 2.0, chrome::SILVER);
        }
    }
    if let Some(t) = chrome::win("SteelLine") {
        three_slice(&t, Rect::new(0.0, y - 2.0, w, 4.0 * k), 40.0, WHITE);
    }
    // The lower strip.
    let sy = y + row;
    let sh = bar - row;
    draw_rectangle(0.0, sy, w, sh, Color::new(0.13, 0.13, 0.14, 1.0));
    if let Some(t) = chrome::win_fx("DownCorner", Fx::KeyBlack) {
        let ow = t.width() * sh / t.height();
        let mut x = w * 0.25 - ow / 2.0;
        while x < w {
            tex(&t, Rect::new(x, sy, ow, sh), Color::new(1.0, 1.0, 1.0, 0.85));
            x += w * 0.25;
        }
    }
    resources(game, sy, sh);

    // The time panel.
    let ew = 370.0 * row / 64.0;
    let cx = w / 2.0;
    let size = (13.0 * k).round();
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
    // Russian dates run longer: the lines shrink to the ribbon's middle.
    let room = (w - 2.0 * ew - 16.0 * k).max(100.0);
    let size = lines.iter().map(|(l, _)| fit_size(l, room, size)).fold(size, f32::min);
    let lh = size * 1.25;
    let top = y + row / 2.0 - lh * (lines.len() as f32 - 1.0) / 2.0 + size * 0.36;
    // The message box hovered on the idle map: the three buttons stand over it, its text
    // hidden (as the original draws it).
    let s = row / PANEL_ROW;
    let pointer = Vec2::from(super::widgets::pointer());
    let buttons = time_buttons && !super::widgets::input_blocked() && message_box(y, cx, s).contains(pointer);
    let mut timed = None;
    if buttons {
        let hovered = time_button_at(pointer, true, y, cx, s);
        for b in TimeButton::ALL {
            if time_button(b, time_button_rect(b, y, cx, s), hovered == Some(b)) {
                timed = Some(b);
            }
        }
    } else {
        super::dt_font::with_face(super::dt_font::Face::Bold, || {
            for (i, (l, c)) in lines.iter().enumerate() {
                shadow_centered(l, cx, top + i as f32 * lh, size, *c);
            }
        });
    }

    // The buttons.
    let mut pressed = None;
    let bh = |b: BarButton| if b.small() { 44.0 } else { 54.0 } * row / 64.0;
    let bw = |b: BarButton| if b.small() { 80.0 } else { 98.0 } * row / 64.0;
    let mut x = 8.0 * k;
    for b in BarButton::LEFT {
        let r = Rect::new(x, y + (row - bh(b)) / 2.0, bw(b), bh(b));
        if oval(b, r, look(b)) {
            pressed = Some(b);
        }
        x += bw(b) - 4.0 * k;
    }
    let mut x = w - 8.0 * k;
    for b in BarButton::RIGHT.iter().rev().copied() {
        x -= bw(b);
        let r = Rect::new(x, y + (row - bh(b)) / 2.0, bw(b), bh(b));
        if oval(b, r, look(b)) {
            pressed = Some(b);
        }
        x += 4.0 * k;
    }
    (pressed, timed)
}

/// The time buttons' place on the bar as drawn now: the bar's top, the screen's middle and
/// the scale (see [`on_bar`]).
pub fn time_layout() -> (f32, f32, f32) {
    let row = (60.0 * chrome::k()).round();
    (screen_height() - chrome::bar_height(), screen_width() / 2.0, row / PANEL_ROW)
}

/// The time panel in the middle of the bar (it takes the wait clicks on the map).
pub fn time_panel() -> Rect {
    let row = (60.0 * chrome::k()).round();
    let ew = 370.0 * row / 64.0;
    Rect::new(ew, screen_height() - chrome::bar_height(), screen_width() - 2.0 * ew, row)
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
}
