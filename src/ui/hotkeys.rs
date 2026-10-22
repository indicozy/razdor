//! Keys that work across screens (F1 help, F5 quick save, F9 quick load), when they may
//! fire, and the F1 overlay listing every screen's keys (Razdor extras the players asked
//! for). A screen's own keys live in its module; this one only lists them for the overlay.
//!
//! No key fires while the player types (the class screen's hero name, the save name, an
//! editor field) or while a dialog or question is open (there Esc and N mean "No" and any
//! other key "Yes", as in the original but for Razdor's N).

use crate::ui::input::{is_key_pressed, is_mouse_button_pressed};
use macroquad::prelude::*;

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

/// Draws the F1 overlay over the screen; true when it closes (F1, Esc or a click).
pub fn help_overlay(place: Place) -> bool {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
    let own = screen_keys(place);
    let lines = own.len() + EVERYWHERE.len() + 3;
    let (w, h) = (640.0f32.min(sw - 20.0), (90.0 + lines as f32 * 24.0).min(sh - 20.0));
    let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
    chrome::window(Rect::new(x, y, w, h), tr("Keys"), Skin::Marble, false);
    let kx = x + 24.0;
    let dx = x + w * 0.42;
    let mut ly = y + 58.0;
    let section = |title: &str, rows: &[(&str, &str)], ly: &mut f32| {
        text(title, kx, *ly, 19.0, ACCENT);
        *ly += 26.0;
        for (k, what) in rows {
            if *ly > y + h - 34.0 {
                break;
            }
            text_fit(tr(&fill_keys(k)), kx + 10.0, *ly, dx - kx - 16.0, 17.0, INK);
            text_fit(tr(what), dx, *ly, x + w - dx - 16.0, 17.0, DIM);
            *ly += 24.0;
        }
        *ly += 6.0;
    };
    if !own.is_empty() {
        section(place_name(place), &own, &mut ly);
    }
    section(tr("Everywhere"), &EVERYWHERE, &mut ly);
    text_centered(tr("F1, Esc or a click closes this list"), x + w / 2.0, y + h - 14.0, 15.0, DIM);
    is_key_pressed(keymap::key(Action::Help)) || is_key_pressed(KeyCode::Escape) || is_mouse_button_pressed(MouseButton::Left)
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
