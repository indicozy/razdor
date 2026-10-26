//! Keys that work across screens (F1 help, F5 quick save, F9 quick load), when they may
//! fire, and the F1 overlay listing every screen's keys (Razdor extras the players asked
//! for). A screen's own keys live in its module; this one only lists them for the overlay.
//!
//! No key fires while the player types (the class screen's hero name, the save name, an
//! editor field) or while a dialog or question is open (there Esc and N mean "No" and any
//! other key "Yes", as in the original but for Razdor's N).

use crate::ui::input::{self, is_key_pressed, is_mouse_button_pressed, tap_key};
use macroquad::prelude::*;
use razdor::local;

use razdor::i18n::{n_, tr};

use super::chrome::{self, Skin};
use super::keymap::{self, Action};
use super::widgets::*;

/// Where the keys are pressed: the app's screens, by what their keys do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Title,
    ClassSelect,
    WorldMap,
    Building,
    Army,
    Battle,
    Hero,
    Spellbook,
    Menu,
    Save,
    Load,
    /// Victory or defeat.
    End,
    Editor,
    /// The custom battle setup.
    Custom,
}

/// A key that works across screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    /// F1: the key list.
    Help,
    /// F5: the quick save.
    QuickSave,
    /// F9: load the quick save.
    QuickLoad,
    /// F10: load the autosave written last (Razdor's, the owner's request).
    LatestAutosave,
    /// F2: the next language of the list (`language::cycle`).
    Language,
}

impl Global {
    #[cfg(test)]
    pub const ALL: [Global; 5] = [Global::Help, Global::QuickSave, Global::QuickLoad, Global::LatestAutosave, Global::Language];

    /// The key the player has bound to it (`keymap`).
    pub fn key(self) -> KeyCode {
        match self {
            Global::Help => keymap::key(Action::Help),
            Global::QuickSave => keymap::key(Action::QuickSave),
            Global::QuickLoad => keymap::key(Action::QuickLoad),
            Global::LatestAutosave => keymap::key(Action::LatestAutosave),
            Global::Language => keymap::key(Action::Language),
        }
    }
}

/// What stands in the way of keys this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    /// A text field has the keyboard.
    pub typing: bool,
    /// A dialog or a question is open.
    pub dialog: bool,
    /// A game is loaded.
    pub game: bool,
    /// A battle is pending (saving waits until it is over, as in the menu).
    pub foe: bool,
    /// The Community endless wait (F4) runs: F5 ends it, as in the original, and does not
    /// save.
    pub endless: bool,
}

/// The screens with a text field that always has the keyboard.
pub fn always_typing(place: Place) -> bool {
    matches!(place, Place::ClassSelect | Place::Save)
}

/// Whether the player is typing: on a screen with a text field, or a field has the focus.
pub fn typing(place: Place, field_focused: bool) -> bool {
    always_typing(place) || field_focused
}

/// Whether single keys may act as shortcuts now (N for the music, letters on the map).
pub fn shortcuts_allowed(g: Guard) -> bool {
    !g.typing && !g.dialog
}

/// Whether `key` may fire at `place`. The editor has its own keys (and F2).
pub fn allowed(place: Place, key: Global, g: Guard) -> bool {
    if !shortcuts_allowed(g) || (place == Place::Editor && key != Global::Language) {
        return false;
    }
    match key {
        Global::Help => true,
        Global::QuickSave => {
            g.game && !g.foe && !g.endless && matches!(place, Place::WorldMap | Place::Building | Place::Army | Place::Hero | Place::Spellbook | Place::Menu)
        }
        Global::QuickLoad | Global::LatestAutosave | Global::Language => true,
    }
}

/// The keys of `place` for the F1 overlay: (key, what it does).
pub fn screen_keys(place: Place) -> Vec<(&'static str, &'static str)> {
    match place {
        Place::Title => vec![(n_("Click"), n_("the menu's buttons; a scenario twice: go on")), ("{QuickLoad}", n_("load the quick save")), ("{LatestAutosave}", n_("load the latest autosave")), ("Esc", n_("in the main menu: quit the game"))],
        Place::ClassSelect => vec![(n_("Type"), n_("the hero's name")), ("Backspace", n_("delete a letter")), (n_("Mouse, Shift + arrows"), n_("move the cursor, select")), ("Ctrl+A / C / X / V", n_("select all, copy, cut, paste"))],
        Place::WorldMap => vec![
            (n_("Click"), n_("show the route; a second click on the spot: walk there")),
            (n_("Click / any key while walking"), n_("stop after the step under way")),
            (n_("Right click / Space"), n_("stop (a walk after its step, a wait at once); drop the route shown")),
            (n_("Right button held"), n_("what stands there")),
            (n_("Right button held and moved"), n_("drag the map")),
            (n_("Arrow keys, screen edges"), n_("scroll the map")),
            (n_("Wheel, + / -"), n_("zoom")),
            ("0", n_("back to the map's zoom from the settings")),
            ("{Wait1} / {Wait4}", n_("wait 1 or 4 hours")),
            ("{EndlessWait} / {EndEndlessWait}", n_("wait without end / stop waiting")),
            (n_("Time panel"), n_("hover its centre: buttons to wait 1 hour, see the hero, wait 4 hours")),
            (n_("Click / any key while waiting"), n_("stop after the half hour under way")),
            (n_("Click where you stand"), n_("the building again, or its garrison's battle")),
            ("{Minimap}", n_("minimap")),
            ("{Debug}", n_("debug: event points, lanterns, events of places")),
            ("{CentreHero}", n_("centre the camera on the hero")),
            ("{Journal}", n_("journal")),
            ("{SpellBook}", n_("spell book")),
            ("{HeroArmy}", n_("hero and army")),
            ("{QuickSave} / {QuickLoad}", n_("quick save / quick load")),
            ("Esc", n_("close the minimap, else the game menu")),
            ("{Console}", n_("cheat console (help lists its commands)")),
        ],
        Place::Building => vec![
            (n_("Click"), n_("tabs and buttons")),
            (n_("Type, Ctrl+F"), n_("market: filter the list (Enter picks the first, Esc clears)")),
            ("{QuickSave}", n_("quick save")),
            ("Esc", n_("back to the map")),
        ],
        Place::Army => vec![
            (n_("Click"), n_("a unit, an item")),
            (n_("Type, Ctrl+F"), n_("filter the backpack (Enter: the first to the unit, Esc clears)")),
            ("{QuickSave}", n_("quick save")),
            ("{HeroArmy} / Esc", n_("close")),
        ],
        Place::Battle => vec![
            (n_("Click a framed card"), n_("attack or cast")),
            (n_("Click a lit cell"), n_("step there")),
            (n_("Click your own card"), n_("pass one action")),
            (n_("Space"), n_("as a click on the unit's own card")),
            ("Q", n_("finish the battle automatically")),
            ("Enter", n_("before the first move: quick battle")),
            ("W", n_("watch the AI play both sides / take control back")),
            ("S", n_("while watching: speed 1x, 2x, 4x")),
            (n_("Q while watching"), n_("skip to the end (the same result)")),
            ("Esc", n_("ways out of the battle")),
            ("Enter", n_("OK on the result")),
            ("{Console}", n_("cheat console (help lists its commands)")),
        ],
        Place::Hero => vec![
            (n_("Click an item"), n_("wear it or take it off")),
            (n_("Drag an item"), n_("onto a slot of the hero")),
            (n_("Left / Right"), n_("change the diary's tab")),
            (n_("Up / Down, click"), n_("pick an entry")),
            ("{QuickSave}", n_("quick save")),
            ("{Journal} / Esc", n_("close")),
        ],
        Place::Spellbook => vec![(n_("Click"), n_("pick a spell")), ("Enter", n_("cast on your army")), ("{QuickSave}", n_("quick save")), ("{SpellBook} / Esc", n_("close"))],
        Place::Menu => vec![("{QuickSave}", n_("quick save")), ("Esc", n_("back to the game"))],
        Place::Save => vec![(n_("Type"), n_("the save's name")), ("Ctrl+A / C / X / V", n_("select all, copy, cut, paste")), ("Enter", n_("save")), ("Esc", n_("cancel"))],
        Place::Load => vec![(n_("Click"), n_("pick a save")), ("Enter", n_("load it")), ("Esc", n_("cancel"))],
        Place::End => vec![(n_("Click"), n_("the buttons")), ("{QuickLoad}", n_("load the quick save")), ("{LatestAutosave}", n_("load the latest autosave"))],
        Place::Editor => vec![],
        Place::Custom => vec![
            (n_("Click a unit type"), n_("add it to the army picked")),
            (n_("Click an army's name"), n_("pick that army")),
            (n_("Click a unit"), n_("pick it, to give it items")),
            ("- / + / ×", n_("level down, level up, remove")),
            ("Enter", n_("fight")),
            ("Esc", n_("back to the main menu")),
        ],
    }
}

/// Keys that work on every screen of a game, and in the dialogs.
pub const EVERYWHERE: [(&str, &str); 8] = [
    ("{Help}", n_("this list (F1 or Esc closes it)")),
    ("{Language}", n_("the next language of the list")),
    ("Alt+Enter", n_("screen: windowed, borderless, full screen")),
    ("{QuickLoad}", n_("load the quick save")),
    ("{LatestAutosave}", n_("load the latest autosave")),
    ("{Music}", n_("music off / on")),
    ("Y / Enter", n_("\"Yes\" in a question (Enter: OK)")),
    ("N / Esc", n_("\"No\" in a question")),
];

/// The key column of a row with the keys the player has bound: `{Action}` (the action's
/// name, as in `keymap::Action`) is replaced by its key (`keymap::display`).
pub fn fill_keys(column: &str) -> String {
    let mut text = column.to_string();
    for a in keymap::ALL {
        let token = format!("{{{a:?}}}");
        if text.contains(&token) {
            text = text.replace(&token, &keymap::display(keymap::key(a)));
        }
    }
    text
}

/// A screen's title in the overlay.
fn place_name(place: Place) -> &'static str {
    match place {
        Place::Title => tr("Title screen"),
        Place::ClassSelect => tr("Hero choice"),
        Place::WorldMap => tr("World map"),
        Place::Building => tr("Building"),
        Place::Army => tr("Hero and army"),
        Place::Battle => tr("Battle"),
        Place::Hero => tr("Hero"),
        Place::Spellbook => tr("Spell book"),
        Place::Menu => tr("Game menu"),
        Place::Save => tr("Save"),
        Place::Load => tr("Load"),
        Place::End => tr("End of the game"),
        Place::Editor => tr("Map editor"),
        Place::Custom => tr("Custom battle"),
    }
}

/// A row of the F1 list as a button: a tap runs the key it stands for (`input::tap_key`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    /// What it does, in the row's words (English; translated where drawn).
    pub what: &'static str,
    /// The key a tap presses.
    pub key: KeyCode,
    /// The key as the player sees it (the bound key of an action, or the literal's name).
    pub shown: String,
    /// The action it is bound to; `None` for a literal key such as Esc.
    pub action: Option<Action>,
}

/// The rows of `place` as (label, words, whether the screen's own): its rows, then EVERYWHERE's.
fn all_rows(place: Place) -> Vec<(&'static str, &'static str, bool)> {
    screen_keys(place).into_iter().map(|(l, w)| (l, w, true)).chain(EVERYWHERE.into_iter().map(|(l, w)| (l, w, false))).collect()
}

/// One `/`-separated part of a row's key label as a key: an `{Action}` (its bound key), a
/// literal key name, or a single letter when the label has only that part. None for words
/// (Click, Type, …) and for F1, which is the overlay itself.
fn part_key(part: &str, alone: bool) -> Option<(KeyCode, Option<Action>, String)> {
    if let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
        let a = keymap::ALL.into_iter().find(|a| format!("{a:?}") == name)?;
        // F1 is the overlay itself; the endless wait is held while its key is down (a tap would
        // start it with no release); its stop shares the quick save's F5, which is the tile.
        let held = a == Action::EndlessWait || (a == Action::EndEndlessWait && keymap::key(a) == keymap::key(Action::QuickSave));
        if a == Action::Help || held {
            return None;
        }
        return Some((keymap::key(a), Some(a), keymap::display(keymap::key(a))));
    }
    let key = match part {
        "Esc" => KeyCode::Escape,
        "Enter" => KeyCode::Enter,
        "Backspace" => KeyCode::Backspace,
        _ if part.len() == 1 && part.chars().all(|c| c.is_ascii_alphabetic()) && !alone => return None,
        _ => keymap::key_from_name(part)?,
    };
    Some((key, None, part.to_string()))
}

/// The tiles of one row: one per key part, its words split at " / " when the row has as many.
/// The literal keys of EVERYWHERE (the question's Y, Enter, N, Esc) make none: they answer a
/// question, and on the map they do nothing.
fn row_tiles(label: &'static str, what: &'static str, own: bool) -> Vec<Tile> {
    let parts: Vec<&'static str> = label.split(" / ").collect();
    let words: Vec<&'static str> = what.split(" / ").collect();
    let split = words.len() == parts.len();
    parts
        .iter()
        .enumerate()
        .filter_map(|(i, part)| {
            let (key, action, shown) = part_key(part, parts.len() == 1)?;
            if action.is_none() && !own {
                return None;
            }
            Some(Tile { what: if split { words[i] } else { what }, key, shown, action })
        })
        .collect()
}

/// The F1 list of `place` as buttons: each key once, the screen's own first, then EVERYWHERE's.
pub fn tiles(place: Place) -> Vec<Tile> {
    let mut out: Vec<Tile> = Vec::new();
    for (label, what, own) in all_rows(place) {
        for t in row_tiles(label, what, own) {
            if !out.iter().any(|o| o.key == t.key) {
                out.push(t);
            }
        }
    }
    out
}

/// The rows of `place` that make no tile (the mouse, typing, the arrows): text lines.
pub fn info_rows(place: Place) -> Vec<(&'static str, &'static str)> {
    all_rows(place).into_iter().filter(|(label, what, own)| row_tiles(label, what, *own).is_empty()).map(|(l, w, _)| (l, w)).collect()
}

/// Whether a tile can be tapped now: the key's rules (`allowed`) for the shortcuts, else always.
pub fn tile_enabled(place: Place, tile: &Tile, g: Guard) -> bool {
    match tile.action {
        Some(Action::QuickSave) => allowed(place, Global::QuickSave, g),
        Some(Action::QuickLoad) => allowed(place, Global::QuickLoad, g),
        Some(Action::LatestAutosave) => allowed(place, Global::LatestAutosave, g),
        Some(Action::Language) => allowed(place, Global::Language, g),
        Some(Action::Music) => shortcuts_allowed(g),
        _ => true,
    }
}

local! {
    /// How far the overlay's body is scrolled (pixels), kept while the list is open.
    static SCROLL: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// Draws the F1 overlay over the screen: the keys as a grid of buttons (`tiles`) and the rows
/// that make none below it (`info_rows`). A tap on a button closes the list and runs its key
/// on the screen under it (`input::tap_key`). Returns true when the list closes: F1 or Esc, a
/// press outside the buttons, or a tap on one. `fresh`: the press that opened it (a menu's
/// button) does not close it.
/// A tile's words at `size`, wrapped to `width` in at most `max_lines`: the largest size, down to
/// 60 % of `size`, at which they fit; else one line at the smallest size (drawn shrunk to fit).
fn fit_words(words: &str, width: f32, size: f32, max_lines: usize) -> (f32, Vec<String>) {
    let mut s = size;
    loop {
        let lines = wrap(words, width, s);
        if lines.len() <= max_lines {
            return (s, lines);
        }
        if s <= size * 0.6 {
            return (s, vec![words.to_string()]);
        }
        s -= size * 0.05;
    }
}

/// The screens drawn over the game's bottom bar (the map and the windows over it, the menu and
/// the save and load windows in game): the help overlay keeps above the bar on those.
fn has_bar(place: Place) -> bool {
    matches!(place, Place::WorldMap | Place::Building | Place::Army | Place::Hero | Place::Spellbook | Place::Menu | Place::Save | Place::Load)
}

pub fn help_overlay(place: Place, guard: Guard, fresh: bool) -> bool {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
    // The layout's base: on a phone the phone's text base (one original px per dp, as the message
    // windows); on the desktop the interface scale.
    let k = super::display::text_base(chrome::k(), super::dialog::TEXT_REF);
    // The text's scale: on a phone the phone's text (with «Размер текста»), drawn under the legacy
    // metrics so the sizes are not shrunk to the sheet fonts' own; on the desktop `k`, as before.
    let phone = super::display::dp_px().is_some();
    let t = if phone { super::display::text_for(chrome::k(), super::dialog::TEXT_REF) } else { k };
    let tiles = tiles(place);
    let info = info_rows(place);
    let cols = if sw < sh || sw < 760.0 { 2 } else { 3 };
    let (pad, gap) = (14.0 * k, 8.0 * k);
    let w = if cols == 2 { sw - 16.0 } else { (880.0 * k).min(sw - 40.0) };
    let tile_w = (w - 2.0 * pad - 14.0 * k - gap * (cols as f32 - 1.0)) / cols as f32;
    let size = 18.0 * t;
    // A tile's words wrap to its width: at most two lines on the desktop, three on a phone; a
    // longer text shrinks (to 60 %) until it fits those lines.
    let max_lines = if phone { 3 } else { 2 };
    let fitted: Vec<(f32, Vec<String>)> = super::dt_font::legacy_metrics_if(phone, || tiles.iter().map(|tl| fit_words(tr(tl.what), tile_w - 16.0 * t, size, max_lines)).collect());
    let text_rows = fitted.iter().map(|(_, l)| l.len().clamp(1, max_lines)).max().unwrap_or(1) as f32;
    let tile_h = if phone { (text_rows * 21.0 * t + 33.0 * t).max(60.0 * k) } else { (60.0 * k).max(40.0) };
    let grid_h = tiles.len().div_ceil(cols) as f32 * (tile_h + gap);
    let line = 24.0 * t;
    let info_h = if info.is_empty() { 0.0 } else { 30.0 * t + info.len() as f32 * line + 6.0 * k };
    let content = 4.0 * k + grid_h + 6.0 * k + info_h;
    // On a phone held upright, over a screen with the bottom bar, the overlay stands between the
    // top safe inset and the bar, as the game's windows do; elsewhere over the whole screen.
    let (top, bottom) = if super::window_layout::portrait() && has_bar(place) { (super::display::safe_area().top, sh - chrome::bar_height()) } else { (0.0, sh) };
    let h = (content + 52.0 * t + 72.0 * k).min(bottom - top - 16.0);
    let (x, y) = ((sw - w) / 2.0, top + (bottom - top - h) / 2.0);
    // A phone's list has a close cross (its click area is finger-sized, `chrome::touch_hit`); it closes
    // by Esc and a tap outside, as the desktop's.
    let (inner, box_closed) = chrome::window(Rect::new(x, y, w, h), tr("Keys"), Skin::Marble, phone);
    let body = Rect::new(inner.x, inner.y + 52.0 * t, inner.w, inner.bottom() - 40.0 * k - (inner.y + 52.0 * t));
    let most = (content - body.h).max(0.0);
    let mut scroll = SCROLL.with(|s| s.get()).clamp(0.0, most);
    let (mx, my) = input::screen_pointer();
    let over = |r: Rect| mx >= r.x && mx < r.right() && my >= r.y && my < r.bottom();
    if over(body) {
        scroll = (scroll - input::mouse_wheel().1 * 40.0 * k).clamp(0.0, most);
    }
    super::widgets::swipe_rows(body, 40.0 * k);
    let bar = Rect::new(body.right() - 12.0 * k, body.y, 10.0 * k, body.h);
    let mut on_control = over(bar);
    let mut press = None;
    super::dt_font::legacy_metrics_if(phone, || {
        text(place_name(place), inner.x + pad, inner.y + 22.0 * t, 19.0 * t, ACCENT);
        text_fit(tr("Tap a button to run its action"), inner.x + pad, inner.y + 44.0 * t, inner.w - 2.0 * pad, 15.0 * t, DIM);
        {
            let _clip = super::widgets::Clip::new(body);
            for (i, tl) in tiles.iter().enumerate() {
                let (col, row) = (i % cols, i / cols);
                let ty = body.y + 4.0 * k + row as f32 * (tile_h + gap) - scroll;
                if ty + tile_h <= body.y || ty >= body.bottom() {
                    continue;
                }
                let r = Rect::new(inner.x + pad + col as f32 * (tile_w + gap), ty, tile_w, tile_h);
                let enabled = tile_enabled(place, tl, guard);
                let visible = ty >= body.y - 0.5 && ty + tile_h <= body.bottom() + 0.5;
                let (_, down, pressed) = if visible { super::widgets::button_state(r, enabled) } else { (false, false, false) };
                on_control |= visible && over(r);
                chrome::marble_button(r, "", enabled, down);
                let ink = if enabled { chrome::CREAM } else { DIM };
                let (size, lines) = &fitted[i];
                let step = size * 21.0 / 18.0;
                let top = r.y + (r.h - 17.0 * t - lines.len() as f32 * step) / 2.0 + size * 0.8;
                for (n, l) in lines.iter().enumerate() {
                    // One line shrinks to the tile's width; several lines are set as they wrap.
                    if lines.len() == 1 {
                        text_fit(l, r.x + 8.0 * k, top + n as f32 * step, r.w - 16.0 * k, *size, ink);
                    } else {
                        text(l, r.x + 8.0 * k, top + n as f32 * step, *size, ink);
                    }
                }
                text_fit(&tl.shown, r.x + 8.0 * k, r.bottom() - 8.0 * t, r.w - 16.0 * k, if phone { 17.0 * t } else { 15.0 * t }, ACCENT);
                if pressed {
                    super::audio::cue(super::audio::Cue::Button);
                    press = Some(tl.key);
                }
            }
        }
        let mut ly = body.y + 4.0 * k + grid_h + 6.0 * k - scroll;
        if !info.is_empty() {
            text(tr("Other keys and mouse"), inner.x + pad, ly + 20.0 * t, 17.0 * t, ACCENT);
            ly += 30.0 * t;
            let dx = inner.x + inner.w * 0.42;
            for (key, what) in &info {
                if ly > body.bottom() {
                    break;
                }
                if ly >= body.y - line {
                    text_fit(tr(&fill_keys(key)), inner.x + pad + 8.0 * k, ly + 16.0 * t, dx - inner.x - pad - 16.0 * k, 16.0 * t, INK);
                    text_fit(tr(what), dx, ly + 16.0 * t, inner.right() - dx - pad, 16.0 * t, DIM);
                }
                ly += line;
            }
        }
        text_centered(tr("F1, Esc or a click closes this list"), inner.center().x, inner.bottom() - 12.0 * k, 15.0 * t, DIM);
    });
    if most > 0.0 {
        super::widgets::scrollbar_px(bar, &mut scroll, most, body.h, 40.0 * k);
    }
    SCROLL.with(|s| s.set(scroll));
    let closes = !fresh && is_mouse_button_pressed(MouseButton::Left) && !on_control;
    let done = is_key_pressed(keymap::key(Action::Help)) || esc_pressed() || closes || box_closed || press.is_some();
    if let Some(key) = press {
        tap_key(key);
    }
    if done {
        SCROLL.with(|s| s.set(0.0));
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PLACES: [Place; 14] = [
        Place::Title,
        Place::ClassSelect,
        Place::WorldMap,
        Place::Building,
        Place::Army,
        Place::Battle,
        Place::Hero,
        Place::Spellbook,
        Place::Menu,
        Place::Save,
        Place::Load,
        Place::End,
        Place::Editor,
        Place::Custom,
    ];

    fn in_game() -> Guard {
        Guard { game: true, ..Guard::default() }
    }

    #[test]
    fn no_key_fires_while_typing_or_in_a_dialog() {
        for place in ALL_PLACES {
            for key in Global::ALL {
                assert!(!allowed(place, key, Guard { typing: true, ..in_game() }), "{place:?} {key:?} while typing");
                assert!(!allowed(place, key, Guard { dialog: true, ..in_game() }), "{place:?} {key:?} in a dialog");
            }
        }
        assert!(!shortcuts_allowed(Guard { dialog: true, ..in_game() }), "N is No in a question");
        assert!(typing(Place::Save, false) && typing(Place::ClassSelect, false) && typing(Place::WorldMap, true));
        assert!(!typing(Place::WorldMap, false));
    }

    #[test]
    fn quick_save_only_in_a_game_between_battles() {
        assert!(allowed(Place::WorldMap, Global::QuickSave, in_game()));
        assert!(allowed(Place::Building, Global::QuickSave, in_game()));
        assert!(!allowed(Place::WorldMap, Global::QuickSave, Guard { foe: true, ..in_game() }), "a battle is pending");
        assert!(!allowed(Place::WorldMap, Global::QuickSave, Guard { endless: true, ..in_game() }), "F5 ends the endless wait");
        assert!(!allowed(Place::Battle, Global::QuickSave, in_game()));
        assert!(!allowed(Place::Title, Global::QuickSave, Guard::default()));
        assert!(!allowed(Place::End, Global::QuickSave, in_game()));
    }

    #[test]
    fn quick_load_and_help_work_everywhere_but_the_editor() {
        for place in ALL_PLACES.into_iter().filter(|p| *p != Place::Editor && !always_typing(*p)) {
            assert!(allowed(place, Global::QuickLoad, in_game()), "{place:?}");
            assert!(allowed(place, Global::Help, in_game()), "{place:?}");
        }
        assert!(!allowed(Place::Editor, Global::Help, in_game()));
        assert!(allowed(Place::Editor, Global::Language, in_game()), "F2 works in the editor too");
        assert!(!allowed(Place::Editor, Global::Language, Guard { typing: true, ..in_game() }));
    }

    #[test]
    fn the_key_list_turns_action_keys_into_tiles() {
        keymap::reset();
        let map = tiles(Place::WorldMap);
        let action = |a: Action| map.iter().any(|t| t.action == Some(a));
        assert!(action(Action::QuickSave) && action(Action::QuickLoad), "the map's quick save and load are tiles");
        assert!(action(Action::Minimap) && action(Action::Journal) && action(Action::Wait1));
        let quick_save = map.iter().find(|t| t.action == Some(Action::QuickSave)).unwrap();
        assert_eq!((quick_save.what, quick_save.shown.as_str()), ("quick save", "F5"));
        assert!(map.iter().any(|t| t.key == KeyCode::Escape), "Esc is a literal tile");
        assert!(!map.iter().any(|t| t.action == Some(Action::Help)), "F1 is the overlay itself");
        assert!(map.iter().all(|t| t.what != "Click"), "a click is no tile");
        let info = info_rows(Place::WorldMap);
        assert!(info.iter().any(|(k, _)| *k == "Click"), "a click is a text line");
        assert!(info.iter().any(|(k, _)| *k == "Wheel, + / -"), "the wheel is a text line");
        for (i, a) in map.iter().enumerate() {
            assert!(map[i + 1..].iter().all(|b| b.key != a.key), "each key once: {:?}", a.key);
        }
        let battle = tiles(Place::Battle);
        assert!(battle.iter().any(|t| t.key == KeyCode::Q && t.action.is_none()), "a single letter alone is a tile");
        assert!(tiles(Place::ClassSelect).iter().all(|t| t.key != KeyCode::C), "Ctrl+A / C / X / V is no tile");
        assert!(info_rows(Place::ClassSelect).iter().any(|(k, _)| *k == "Ctrl+A / C / X / V"), "and a text line");
        keymap::reset();
    }

    #[test]
    fn every_screen_lists_its_keys() {
        for place in ALL_PLACES.into_iter().filter(|p| *p != Place::Editor) {
            assert!(!screen_keys(place).is_empty(), "{place:?}");
        }
        let map: Vec<String> = screen_keys(Place::WorldMap).iter().map(|(k, _)| fill_keys(k)).collect();
        for k in ["M", "Tab", "J", "B", "A", "1 / 4", "F4 / F5", "F5 / F9", "F3", "Esc", "~"] {
            assert!(map.iter().any(|m| m == k), "the map lists {k}");
        }
        let battle: Vec<String> = screen_keys(Place::Battle).iter().map(|(k, _)| fill_keys(k)).collect();
        assert!(battle.iter().any(|k| k == "Q"));
        assert!(battle.iter().any(|k| k == "W"), "the watched quick battle");
        assert!(battle.iter().any(|k| k == "~"), "the cheat console");
    }

    #[test]
    fn the_list_shows_the_keys_the_player_bound() {
        use super::super::keymap::{self, Action};
        keymap::reset();
        keymap::set(Action::Journal, macroquad::prelude::KeyCode::Comma);
        keymap::set(Action::QuickSave, macroquad::prelude::KeyCode::F6);
        let map: Vec<String> = screen_keys(Place::WorldMap).iter().map(|(k, _)| fill_keys(k)).collect();
        assert!(map.iter().any(|k| k == "Comma"), "the journal's new key");
        assert!(map.iter().any(|k| k == "F6 / F9"), "the quick save's new key: {map:?}");
        assert!(!map.iter().any(|k| k.contains('{')), "every token filled");
        keymap::reset();
    }
}
