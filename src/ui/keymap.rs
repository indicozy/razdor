//! The keys the player can change (Razdor's, the owner's request after a player found F5 twice
//! in the F1 list): each [`Action`] has a default key, the player's overrides are kept in the
//! settings (`Settings::keys`) and read through [`key`]. Keys inside the dialogs, the arrows,
//! the zoom keys, Space and the editor's keys stay fixed.

use std::cell::RefCell;
use std::collections::HashMap;

use macroquad::prelude::*;

use razdor::i18n::n_;

/// A key the player can rebind. The label is the window's and the F1 list's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Wait one hour on the map (the time panel's button too).
    Wait1,
    /// Wait four hours on the map.
    Wait4,
    /// Community F4: a wait without end, held while the key is down.
    EndlessWait,
    /// Community F5: ends the endless wait (shares its key with the quick save).
    EndEndlessWait,
    /// The minimap window.
    Minimap,
    /// Razdor's journal window.
    Journal,
    /// The spell book.
    SpellBook,
    /// The hero and army window.
    HeroArmy,
    /// Centres the camera on the hero.
    CentreHero,
    /// Razdor's debug overlay.
    Debug,
    /// The cheat console.
    Console,
    /// The key list (F1).
    Help,
    /// The next language of the list.
    Language,
    /// Quick save.
    QuickSave,
    /// Quick load.
    QuickLoad,
    /// Load the autosave written last.
    LatestAutosave,
    /// Music on or off.
    Music,
}

/// Every action, in the order of the settings window.
pub const ALL: [Action; 17] = [
    Action::Wait1,
    Action::Wait4,
    Action::EndlessWait,
    Action::EndEndlessWait,
    Action::Minimap,
    Action::Journal,
    Action::SpellBook,
    Action::HeroArmy,
    Action::CentreHero,
    Action::Debug,
    Action::Console,
    Action::Help,
    Action::Language,
    Action::QuickSave,
    Action::QuickLoad,
    Action::LatestAutosave,
    Action::Music,
];

impl Action {
    /// The key the action has when the player has not changed it (today's key).
    pub fn default_key(self) -> KeyCode {
        match self {
            Action::Wait1 => KeyCode::Key1,
            Action::Wait4 => KeyCode::Key4,
            Action::EndlessWait => KeyCode::F4,
            Action::EndEndlessWait | Action::QuickSave => KeyCode::F5,
            Action::Minimap => KeyCode::M,
            Action::Journal => KeyCode::J,
            Action::SpellBook => KeyCode::B,
            Action::HeroArmy => KeyCode::A,
            Action::CentreHero => KeyCode::Tab,
            Action::Debug => KeyCode::F3,
            Action::Console => KeyCode::GraveAccent,
            Action::Help => KeyCode::F1,
            Action::Language => KeyCode::F2,
            Action::QuickLoad => KeyCode::F9,
            Action::LatestAutosave => KeyCode::F10,
            Action::Music => KeyCode::N,
        }
    }

    /// The action's name in the settings window and the F1 list.
    pub fn label(self) -> &'static str {
        match self {
            Action::Wait1 => n_("wait 1 hour"),
            Action::Wait4 => n_("wait 4 hours"),
            Action::EndlessWait => n_("wait without end"),
            Action::EndEndlessWait => n_("stop waiting without end"),
            Action::Minimap => n_("minimap"),
            Action::Journal => n_("journal"),
            Action::SpellBook => n_("spell book"),
            Action::HeroArmy => n_("hero and army"),
            Action::CentreHero => n_("centre the camera on the hero"),
            Action::Debug => n_("debug: event points, lanterns, events of places"),
            Action::Console => n_("cheat console"),
            Action::Help => n_("this list of keys"),
            Action::Language => n_("the next language of the list"),
            Action::QuickSave => n_("quick save"),
            Action::QuickLoad => n_("quick load"),
            Action::LatestAutosave => n_("load the latest autosave"),
            Action::Music => n_("music off / on"),
        }
    }
}

/// The keys a player can bind, with their saved names. Letters and digits are named as
/// they are typed; the rest as macroquad names them.
const NAMED: &[(&str, KeyCode)] = &[
    ("A", KeyCode::A),
    ("B", KeyCode::B),
    ("C", KeyCode::C),
    ("D", KeyCode::D),
    ("E", KeyCode::E),
    ("F", KeyCode::F),
    ("G", KeyCode::G),
    ("H", KeyCode::H),
    ("I", KeyCode::I),
    ("J", KeyCode::J),
    ("K", KeyCode::K),
    ("L", KeyCode::L),
    ("M", KeyCode::M),
    ("N", KeyCode::N),
    ("O", KeyCode::O),
    ("P", KeyCode::P),
    ("Q", KeyCode::Q),
    ("R", KeyCode::R),
    ("S", KeyCode::S),
    ("T", KeyCode::T),
    ("U", KeyCode::U),
    ("V", KeyCode::V),
    ("W", KeyCode::W),
    ("X", KeyCode::X),
    ("Y", KeyCode::Y),
    ("Z", KeyCode::Z),
    ("0", KeyCode::Key0),
    ("1", KeyCode::Key1),
    ("2", KeyCode::Key2),
    ("3", KeyCode::Key3),
    ("4", KeyCode::Key4),
    ("5", KeyCode::Key5),
    ("6", KeyCode::Key6),
    ("7", KeyCode::Key7),
    ("8", KeyCode::Key8),
    ("9", KeyCode::Key9),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F7", KeyCode::F7),
    ("F8", KeyCode::F8),
    ("F9", KeyCode::F9),
    ("F10", KeyCode::F10),
    ("F11", KeyCode::F11),
    ("F12", KeyCode::F12),
    ("Tab", KeyCode::Tab),
    ("Space", KeyCode::Space),
    ("GraveAccent", KeyCode::GraveAccent),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Slash", KeyCode::Slash),
    ("Semicolon", KeyCode::Semicolon),
    ("Apostrophe", KeyCode::Apostrophe),
    ("LeftBracket", KeyCode::LeftBracket),
    ("RightBracket", KeyCode::RightBracket),
    ("Backslash", KeyCode::Backslash),
];

/// The saved name of a key; empty for a key that cannot be bound (see [`bindable`]).
pub fn key_name(k: KeyCode) -> &'static str {
    NAMED.iter().find(|(_, c)| *c == k).map_or("", |(n, _)| n)
}

/// The key a saved name stands for, if it is one of the bindable keys.
pub fn key_from_name(name: &str) -> Option<KeyCode> {
    NAMED.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Whether a key can be bound (a key the settings can save).
pub fn bindable(k: KeyCode) -> bool {
    !key_name(k).is_empty()
}

/// How a key shows in the UI: "F5", "Tab", "J", "~" for the grave accent, "1".
pub fn display(k: KeyCode) -> String {
    match key_name(k) {
        "GraveAccent" => "~".to_string(),
        "" => String::from("?"),
        name => name.to_string(),
    }
}

thread_local! {
    /// The current bindings: the defaults, with the player's overrides.
    static BINDINGS: RefCell<HashMap<Action, KeyCode>> = RefCell::new(defaults());
}

thread_local! {
    /// The action the keys window waits for a key for, if any: while it waits, no shortcut
    /// fires (the pressed key is the new binding).
    static LISTENING: std::cell::Cell<Option<Action>> = const { std::cell::Cell::new(None) };
}

/// The action whose new key is awaited, if any.
pub fn listening() -> Option<Action> {
    LISTENING.with(|l| l.get())
}

/// Starts (`Some`) or ends (`None`) waiting for the new key of an action.
pub fn listen(a: Option<Action>) {
    LISTENING.with(|l| l.set(a));
}

fn defaults() -> HashMap<Action, KeyCode> {
    ALL.into_iter().map(|a| (a, a.default_key())).collect()
}

/// Whether two actions may share a key: the endless wait's F5 and the quick save's (as
/// they do by default; the endless wait takes the key first, `Guard::endless`).
fn shares(a: Action, b: Action) -> bool {
    matches!((a, b), (Action::EndEndlessWait, Action::QuickSave) | (Action::QuickSave, Action::EndEndlessWait))
}

/// Gives `k` to `a` in `map`. If another action has `k`, that action takes `a`'s old key (a
/// swap), except the actions that may share it ([`shares`]).
fn assign(map: &mut HashMap<Action, KeyCode>, a: Action, k: KeyCode) {
    let old = map.get(&a).copied().unwrap_or(a.default_key());
    map.insert(a, k);
    if let Some(b) = ALL.into_iter().find(|&b| b != a && map.get(&b) == Some(&k) && !shares(a, b)) {
        map.insert(b, old);
    }
}

/// The key `a` is bound to now.
pub fn key(a: Action) -> KeyCode {
    BINDINGS.with(|b| b.borrow().get(&a).copied().unwrap_or(a.default_key()))
}

/// Binds `a` to `k` (see [`assign`] for a key another action has). Keys that cannot be bound
/// are ignored.
pub fn set(a: Action, k: KeyCode) {
    if bindable(k) {
        BINDINGS.with(|b| assign(&mut b.borrow_mut(), a, k));
    }
}

/// Puts every action back on its default key.
pub fn reset() {
    BINDINGS.with(|b| *b.borrow_mut() = defaults());
}

/// The bindings that differ from the defaults, as (action, key) names for the settings.
pub fn overrides() -> Vec<(String, String)> {
    BINDINGS.with(|b| {
        let b = b.borrow();
        ALL.into_iter()
            .filter(|a| b.get(a).copied().unwrap_or(a.default_key()) != a.default_key())
            .map(|a| (format!("{a:?}"), key_name(key(a)).to_string()))
            .collect()
    })
}

/// Takes the player's overrides from the settings' (action, key) names; unknown names are
/// ignored and the rest of the bindings stay on their defaults.
pub fn load(saved: &[(String, String)]) {
    reset();
    BINDINGS.with(|b| {
        let mut b = b.borrow_mut();
        for (action, name) in saved {
            let (Some(a), Some(k)) = (ALL.into_iter().find(|a| format!("{a:?}") == *action), key_from_name(name)) else {
                continue;
            };
            b.insert(a, k);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Puts the bindings back on the defaults (each test runs on its own thread, so this
    /// only matters within one).
    fn fresh() {
        reset();
    }

    #[test]
    fn defaults_are_today_keys() {
        fresh();
        assert_eq!(key(Action::Wait1), KeyCode::Key1);
        assert_eq!(key(Action::EndlessWait), KeyCode::F4);
        assert_eq!(key(Action::EndEndlessWait), KeyCode::F5);
        assert_eq!(key(Action::QuickSave), KeyCode::F5);
        assert_eq!(key(Action::Console), KeyCode::GraveAccent);
        assert_eq!(key(Action::CentreHero), KeyCode::Tab);
        assert_eq!(key(Action::Music), KeyCode::N);
        assert!(overrides().is_empty());
        assert_eq!(display(KeyCode::GraveAccent), "~");
        assert_eq!(display(KeyCode::F5), "F5");
        assert_eq!(display(KeyCode::Tab), "Tab");
        assert_eq!(display(KeyCode::J), "J");
        assert_eq!(display(KeyCode::Key1), "1");
    }

    #[test]
    fn set_without_conflict_moves_only_that_action() {
        fresh();
        set(Action::Minimap, KeyCode::K);
        assert_eq!(key(Action::Minimap), KeyCode::K);
        assert_eq!(key(Action::Journal), KeyCode::J);
        assert_eq!(overrides(), vec![("Minimap".to_string(), "K".to_string())]);
    }

    #[test]
    fn set_with_conflict_swaps() {
        fresh();
        set(Action::Minimap, KeyCode::J);
        assert_eq!(key(Action::Minimap), KeyCode::J);
        assert_eq!(key(Action::Journal), KeyCode::M, "the journal takes the minimap's old key");
    }

    #[test]
    fn the_f5_pair_may_share_a_key() {
        fresh();
        assert_eq!(key(Action::QuickSave), key(Action::EndEndlessWait));
        set(Action::QuickSave, KeyCode::F5);
        assert_eq!(key(Action::EndEndlessWait), KeyCode::F5, "setting it to F5 again keeps the pair");
        set(Action::Minimap, KeyCode::F5);
        assert_eq!(key(Action::Minimap), KeyCode::F5);
        assert_eq!(key(Action::QuickSave), KeyCode::F5, "a swap leaves the quick save on F5");
        assert_ne!(key(Action::EndEndlessWait), KeyCode::F5, "only one of the others takes the old key");
    }

    #[test]
    fn overrides_round_trip_through_names() {
        fresh();
        set(Action::Journal, KeyCode::Comma);
        set(Action::Minimap, KeyCode::Key2);
        set(Action::Console, KeyCode::Key3);
        let saved = overrides();
        assert_eq!(saved.len(), 3, "one name pair per changed action: {saved:?}");
        let before: Vec<KeyCode> = ALL.into_iter().map(key).collect();
        reset();
        load(&saved);
        let after: Vec<KeyCode> = ALL.into_iter().map(key).collect();
        assert_eq!(before, after);
        assert_eq!(key_from_name("Comma"), Some(KeyCode::Comma));
        assert_eq!(key_name(KeyCode::Comma), "Comma");
        fresh();
    }

    #[test]
    fn unknown_names_are_ignored() {
        fresh();
        load(&[("Nothing".to_string(), "J".to_string()), ("Journal".to_string(), "NoSuchKey".to_string()), ("Minimap".to_string(), "K".to_string())]);
        assert_eq!(key(Action::Journal), KeyCode::J);
        assert_eq!(key(Action::Minimap), KeyCode::K);
        assert_eq!(key_from_name("NoSuchKey"), None);
        fresh();
    }

    #[test]
    fn keys_that_cannot_be_saved_are_not_bound() {
        fresh();
        set(Action::Minimap, KeyCode::Escape);
        assert_eq!(key(Action::Minimap), KeyCode::M);
        fresh();
    }
}
