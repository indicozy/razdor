//! Razdor Studio's commands (UX1, spec §9): every action once, with its id, its name (the English
//! text, which is the key of `data/lang/ru/studio.txt`), its Phosphor icon, its default shortcut and
//! its context. Buttons, tooltips and the shortcuts read this table, so a name or a key is written
//! once. The author's changed shortcuts are in the settings (`StudioPrefs::keys`, the command's id
//! and its binding text); a binding that clashes with another one in an overlapping context is refused.
//!
//! egui-miniquad delivers a fixed key set (its `egui_key_from_mq_key`): the keys below are the ones it
//! delivers. The function keys (F1 to F12) are not in that set, so they are read from macroquad's own
//! key state (`mq_presses`). Ctrl+C, Ctrl+X and Ctrl+V are not key events at all: egui-miniquad turns
//! them into the window's Copy, Cut and Paste events (`clipboard`), so they are fixed, not rebindable.

use std::collections::BTreeMap;

use egui_macroquad::egui::{self, Event, Key, Modifiers};
use egui_phosphor::regular as ic;
use razdor::i18n::{n_, tr};

/// The shortcut settings: the command id and its binding text (only the changed ones).
pub type Prefs = BTreeMap<String, String>;

/// Where a command applies. `Global` applies everywhere; the others only in their workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Map,
    /// The Inspector has the focus (the pointer was in it last frame).
    Inspector,
    /// The Сюжет workspace's story graph (the keys of a node: frame, find, duplicate).
    Story,
    /// The Данные workspace's spreadsheets (find in the sheet).
    Data,
}

/// Whether two scopes can take the same key: a global command overlaps every other.
pub fn overlaps(a: Scope, b: Scope) -> bool {
    a == b || a == Scope::Global || b == Scope::Global
}

/// One studio action.
pub struct Command {
    /// The stable id: the saved bindings refer to it.
    pub id: &'static str,
    /// The English text, which is the catalogue's key (`data/lang/ru/studio.txt`).
    pub name: &'static str,
    /// The Phosphor icon (Regular).
    pub icon: &'static str,
    /// The default shortcut's text (`""` for none).
    pub default: &'static str,
    /// A second shortcut that always works (Ctrl+Y for redo); not rebindable (`""` for none).
    pub also: &'static str,
    pub scope: Scope,
    /// The key is a clipboard key (an event, see `clipboard`): not rebindable.
    pub fixed: bool,
    /// Why the command does nothing yet, as a catalogue key (`""` when it works): it is shown
    /// disabled and its key is not dispatched.
    pub later: &'static str,
}

const fn cmd(id: &'static str, name: &'static str, icon: &'static str, default: &'static str) -> Command {
    Command { id, name, icon, default, also: "", scope: Scope::Global, fixed: false, later: "" }
}

/// Every studio command. The order is the order of the shortcut help.
pub const COMMANDS: &[Command] = &[
    cmd("save", n_("Save"), ic::FLOPPY_DISK, "Ctrl+S"),
    Command { id: "save-all", name: n_("Save all"), default: "Ctrl+Shift+S", ..cmd("", "", ic::FLOPPY_DISK_BACK, "") },
    cmd("undo", n_("Undo"), ic::ARROW_COUNTER_CLOCKWISE, "Ctrl+Z"),
    Command { id: "redo", name: n_("Redo"), also: "Ctrl+Y", ..cmd("", "", ic::ARROW_CLOCKWISE, "Ctrl+Shift+Z") },
    cmd("check", n_("Check"), ic::SHIELD_CHECK, "F7"),
    Command { later: n_("Comes in UX6"), ..cmd("play", n_("Play"), ic::PLAY, "F5") },
    Command { later: n_("Comes in UX6"), ..cmd("palette", n_("Command palette"), ic::MAGNIFYING_GLASS, "Ctrl+Shift+P") },
    Command { later: n_("Comes in UX6"), ..cmd("goto", n_("Go to object or file"), ic::CROSSHAIR, "Ctrl+P") },
    cmd("ws-map", n_("Map workspace"), ic::MAP_TRIFOLD, "Alt+1"),
    cmd("ws-story", n_("Story workspace"), ic::FLOW_ARROW, "Alt+2"),
    cmd("ws-data", n_("Data workspace"), ic::TABLE, "Alt+3"),
    cmd("ws-test", n_("Test workspace"), ic::FLASK, "Alt+4"),
    cmd("drawer", n_("Bottom panel"), ic::CARET_UP, "Ctrl+J"),
    cmd("side-panels", n_("Hide side panels"), ic::SIDEBAR_SIMPLE, "Tab"),
    Command { scope: Scope::Inspector, ..cmd("find", n_("Find a field"), ic::MAGNIFYING_GLASS, "Ctrl+F") },
    cmd("open-in-file", n_("Open in file"), ic::ARROW_SQUARE_OUT, "Ctrl+Shift+O"),
    cmd("cancel", n_("Cancel"), ic::X, "Esc"),
    cmd("settings", n_("Settings"), ic::GEAR, "Ctrl+,"),
    Command { scope: Scope::Map, fixed: true, ..cmd("copy", n_("Copy"), ic::COPY, "Ctrl+C") },
    Command { scope: Scope::Map, fixed: true, ..cmd("cut", n_("Cut"), ic::SCISSORS, "Ctrl+X") },
    Command { scope: Scope::Map, fixed: true, ..cmd("paste", n_("Paste"), ic::CLIPBOARD, "Ctrl+V") },
    Command { scope: Scope::Map, fixed: true, ..cmd("paste-in-place", n_("Paste in place"), ic::CLIPBOARD_TEXT, "Ctrl+Shift+V") },
    // The selection (spec §5.6, §9.3): Delete is the command below; Enter commits a floating paste.
    Command { scope: Scope::Map, ..cmd("select-all", n_("Select all"), ic::SELECTION_ALL, "Ctrl+A") },
    Command { scope: Scope::Map, ..cmd("deselect", n_("Deselect"), ic::SELECTION_BACKGROUND, "Ctrl+D") },
    Command { scope: Scope::Map, ..cmd("invert", n_("Invert selection"), ic::SELECTION_INVERSE, "Ctrl+Shift+I") },
    Command { scope: Scope::Map, ..cmd("save-stamp", n_("Save selection as stamp"), ic::STAMP, "Ctrl+Alt+S") },
    Command { scope: Scope::Map, ..cmd("commit-paste", n_("Commit the paste"), ic::CHECK, "Enter") },
    // The Map tools (spec §9.3): the tool strip's buttons run these too.
    Command { scope: Scope::Map, ..cmd("tool-select", n_("Select and move"), ic::CURSOR, "V") },
    Command { scope: Scope::Map, ..cmd("tool-brush", n_("Brush"), ic::PAINT_BRUSH, "B") },
    Command { scope: Scope::Map, ..cmd("tool-eraser", n_("Eraser"), ic::ERASER, "E") },
    Command { scope: Scope::Map, ..cmd("tool-fill", n_("Fill"), ic::PAINT_BUCKET, "G") },
    Command { scope: Scope::Map, ..cmd("tool-eyedrop", n_("Eyedropper"), ic::EYEDROPPER, "I") },
    Command { scope: Scope::Map, ..cmd("tool-marquee", n_("Marquee"), ic::SELECTION, "M") },
    Command { scope: Scope::Map, ..cmd("tool-lasso", n_("Lasso"), ic::LASSO, "L") },
    Command { scope: Scope::Map, ..cmd("tool-wand", n_("Magic wand"), ic::MAGIC_WAND, "W") },
    Command { scope: Scope::Map, ..cmd("tool-stamp", n_("Stamp"), ic::STAMP, "S") },
    Command { scope: Scope::Map, ..cmd("tool-army", n_("Army"), ic::SWORD, "A") },
    Command { scope: Scope::Map, ..cmd("tool-building", n_("Building"), ic::BUILDINGS, "U") },
    Command { scope: Scope::Map, ..cmd("tool-point", n_("Point"), ic::MAP_PIN, "P") },
    Command { scope: Scope::Map, later: n_("not in the package format yet"), ..cmd("tool-trigger", n_("Trigger zone"), ic::POLYGON, "R") },
    Command { scope: Scope::Map, ..cmd("tool-hand", n_("Hand"), ic::HAND, "H") },
    Command { scope: Scope::Map, ..cmd("brush-smaller", n_("Brush smaller"), ic::MINUS, "[") },
    Command { scope: Scope::Map, ..cmd("brush-larger", n_("Brush larger"), ic::PLUS, "]") },
    Command { scope: Scope::Map, ..cmd("density-less", n_("Density less"), ic::MINUS, "Shift+[") },
    Command { scope: Scope::Map, ..cmd("density-more", n_("Density more"), ic::PLUS, "Shift+]") },
    Command { scope: Scope::Map, ..cmd("size-1", n_("Brush size 1"), ic::CIRCLE, "1") },
    Command { scope: Scope::Map, ..cmd("size-2", n_("Brush size 2"), ic::CIRCLE, "2") },
    Command { scope: Scope::Map, ..cmd("size-3", n_("Brush size 3"), ic::CIRCLE, "3") },
    Command { scope: Scope::Map, ..cmd("size-4", n_("Brush size 4"), ic::CIRCLE, "4") },
    Command { scope: Scope::Map, ..cmd("size-5", n_("Brush size 5"), ic::CIRCLE, "5") },
    Command { scope: Scope::Map, ..cmd("size-6", n_("Brush size 6"), ic::CIRCLE, "6") },
    Command { scope: Scope::Map, ..cmd("size-9", n_("Brush size 9"), ic::CIRCLE, "9") },
    Command { scope: Scope::Map, ..cmd("swap-picks", n_("Swap the last two picks"), ic::ARROWS_LEFT_RIGHT, "X") },
    Command { scope: Scope::Map, ..cmd("zoom-in", n_("Zoom in"), ic::MAGNIFYING_GLASS_PLUS, "Ctrl+=") },
    Command { scope: Scope::Map, ..cmd("zoom-out", n_("Zoom out"), ic::MAGNIFYING_GLASS_MINUS, "Ctrl+-") },
    Command { scope: Scope::Map, ..cmd("zoom-fit", n_("Fit the map"), ic::FRAME_CORNERS, "Ctrl+0") },
    Command { scope: Scope::Map, ..cmd("zoom-100", n_("Zoom 100 %"), ic::CROP, "Ctrl+1") },
    Command { also: "Backspace", ..cmd("delete", n_("Delete"), ic::TRASH, "Delete") },
    Command { scope: Scope::Story, ..cmd("find-node", n_("Find node"), ic::MAGNIFYING_GLASS, "Ctrl+F") },
    Command { scope: Scope::Story, ..cmd("frame-node", n_("Frame the selection"), ic::CROSSHAIR, "F") },
    Command { scope: Scope::Story, ..cmd("fit-nodes", n_("Fit the whole graph"), ic::FRAME_CORNERS, "Home") },
    Command { scope: Scope::Story, ..cmd("dup-node", n_("Duplicate node"), ic::COPY, "Ctrl+D") },
    Command { scope: Scope::Data, ..cmd("find-data", n_("Find in the sheet"), ic::MAGNIFYING_GLASS, "Ctrl+F") },
    cmd("reset-layout", n_("Reset layout"), ic::ARROWS_CLOCKWISE, ""),
    cmd("exit", n_("Exit"), ic::SIGN_OUT, ""),
];

/// The command with this id.
pub fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A shortcut: the Ctrl (egui's command), Shift and Alt modifiers and one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

/// The keys a shortcut can name: the ones egui-miniquad delivers, and the function keys (macroquad).
const KEY_NAMES: &[(&str, Key)] = &[
    ("A", Key::A), ("B", Key::B), ("C", Key::C), ("D", Key::D), ("E", Key::E), ("F", Key::F), ("G", Key::G),
    ("H", Key::H), ("I", Key::I), ("J", Key::J), ("K", Key::K), ("L", Key::L), ("M", Key::M), ("N", Key::N),
    ("O", Key::O), ("P", Key::P), ("Q", Key::Q), ("R", Key::R), ("S", Key::S), ("T", Key::T), ("U", Key::U),
    ("V", Key::V), ("W", Key::W), ("X", Key::X), ("Y", Key::Y), ("Z", Key::Z),
    ("0", Key::Num0), ("1", Key::Num1), ("2", Key::Num2), ("3", Key::Num3), ("4", Key::Num4),
    ("5", Key::Num5), ("6", Key::Num6), ("7", Key::Num7), ("8", Key::Num8), ("9", Key::Num9),
    ("F1", Key::F1), ("F2", Key::F2), ("F3", Key::F3), ("F4", Key::F4), ("F5", Key::F5), ("F6", Key::F6),
    ("F7", Key::F7), ("F8", Key::F8), ("F9", Key::F9), ("F10", Key::F10), ("F11", Key::F11), ("F12", Key::F12),
    ("Tab", Key::Tab), ("Esc", Key::Escape), ("Enter", Key::Enter), ("Space", Key::Space),
    ("Backspace", Key::Backspace), ("Delete", Key::Delete), ("Insert", Key::Insert), ("Home", Key::Home),
    ("End", Key::End), ("PageUp", Key::PageUp), ("PageDown", Key::PageDown), ("Up", Key::ArrowUp),
    ("Down", Key::ArrowDown), ("Left", Key::ArrowLeft), ("Right", Key::ArrowRight),
    ("[", Key::OpenBracket), ("]", Key::CloseBracket), (",", Key::Comma), (".", Key::Period),
    ("-", Key::Minus), ("=", Key::Equals), (";", Key::Semicolon), ("\\", Key::Backslash), ("`", Key::Backtick),
];

fn key_of(name: &str) -> Option<Key> {
    KEY_NAMES.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

fn name_of(key: Key) -> Option<&'static str> {
    KEY_NAMES.iter().find(|(_, k)| *k == key).map(|(n, _)| *n)
}

impl Binding {
    /// Reads a binding text such as `Ctrl+Shift+Z`, `Alt+1` or `F7`. `None` for an empty or unknown text.
    pub fn parse(text: &str) -> Option<Binding> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (last, mods) = parts.split_last()?;
        let mut b = Binding { ctrl: false, shift: false, alt: false, key: key_of(last)? };
        for m in mods {
            match *m {
                "Ctrl" => b.ctrl = true,
                "Shift" => b.shift = true,
                "Alt" => b.alt = true,
                _ => return None,
            }
        }
        Some(b)
    }

    /// The binding's text: modifiers in the order Ctrl, Alt, Shift, then the key.
    pub fn text(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(name_of(self.key).unwrap_or("?"));
        s
    }

    /// The binding of an egui key event.
    pub fn from_event(key: Key, m: Modifiers) -> Binding {
        Binding { ctrl: m.command, shift: m.shift, alt: m.alt, key }
    }

    /// A key a text field takes as typing (no Ctrl or Alt): refused while a field has the keyboard.
    fn typing(&self) -> bool {
        !self.ctrl && !self.alt
    }
}

/// The binding a command has now: the author's, else the default (`None` when unbound).
pub fn binding_of(c: &Command, prefs: &Prefs) -> Option<Binding> {
    match prefs.get(c.id) {
        Some(text) => Binding::parse(text),
        None => Binding::parse(c.default),
    }
}

/// The text a tooltip shows: the command's name and its shortcut (`Save · Ctrl+S`), and why it is
/// disabled when it is.
pub fn tip(c: &Command, prefs: &Prefs) -> String {
    let mut s = tr(c.name).to_string();
    if let Some(b) = binding_of(c, prefs) {
        s.push_str(" · ");
        s.push_str(&b.text());
    }
    if !c.later.is_empty() {
        s.push_str(" — ");
        s.push_str(tr(c.later));
    }
    s
}

/// Why a rebinding is refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The command's key is fixed (a clipboard key).
    Fixed,
    /// Another command has this key in an overlapping context (its id).
    Clash(&'static str),
}

/// Sets a command's binding (`None` clears it). A key that another command has in an overlapping
/// context is refused, and nothing is stored. A binding equal to the default is not stored at all.
pub fn rebind(prefs: &mut Prefs, id: &str, b: Option<Binding>) -> Result<(), Refused> {
    let Some(c) = command(id) else { return Ok(()) };
    if c.fixed {
        return Err(Refused::Fixed);
    }
    if let Some(b) = b {
        for other in COMMANDS.iter().filter(|o| o.id != c.id && overlaps(o.scope, c.scope)) {
            let held = binding_of(other, prefs).is_some_and(|x| x == b) || (!other.also.is_empty() && Binding::parse(other.also) == Some(b));
            if held {
                return Err(Refused::Clash(other.id));
            }
        }
    }
    let default = Binding::parse(c.default);
    if b == default {
        prefs.remove(c.id);
    } else {
        prefs.insert(c.id.to_string(), b.map_or_else(String::new, |b| b.text()));
    }
    Ok(())
}

/// Puts a command back to its default. With no id, every command.
pub fn reset(prefs: &mut Prefs, id: Option<&str>) {
    match id {
        Some(id) => {
            prefs.remove(id);
        }
        None => prefs.clear(),
    }
}

/// The commands whose key was pressed this frame in the active contexts, and the key events they took
/// (removed from the frame, so a text field does not see them). `extra` are the function keys read
/// from macroquad (`mq_presses`). While a text field has the keyboard, a typed key (no Ctrl or Alt)
/// is left to it.
pub fn pressed(ctx: &egui::Context, prefs: &Prefs, active: impl Fn(Scope) -> bool, extra: &[Binding]) -> Vec<&'static str> {
    let wants = ctx.wants_keyboard_input();
    let mut out = Vec::new();
    let find = |b: Binding| -> Option<&'static str> {
        if wants && b.typing() {
            return None;
        }
        COMMANDS.iter().find_map(|c| {
            if c.fixed || !c.later.is_empty() || !active(c.scope) {
                return None;
            }
            let hit = binding_of(c, prefs) == Some(b) || (!c.also.is_empty() && Binding::parse(c.also) == Some(b));
            hit.then_some(c.id)
        })
    };
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            Event::Key { key, pressed: true, repeat: false, modifiers, .. } => match find(Binding::from_event(*key, *modifiers)) {
                Some(id) => {
                    out.push(id);
                    false
                }
                None => true,
            },
            _ => true,
        });
    });
    for b in extra {
        if let Some(id) = find(*b) {
            out.push(id);
        }
    }
    out
}

/// A clipboard action of this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    Copy,
    Cut,
    Paste,
    /// Ctrl+Shift+V: the clip goes back where it was copied from.
    PasteInPlace,
}

/// The clipboard actions among the frame's events, which are removed from the frame. egui-miniquad
/// sends Ctrl+C as `Copy`, Ctrl+X as `Cut` and Ctrl+V as the clipboard's text (a `Text` event, with the
/// command key held: a typed letter never comes with it). While a text field has the keyboard the
/// events are left to it.
pub fn clipboard(events: &mut Vec<Event>, command_held: bool, shift_held: bool, wants_keyboard: bool) -> Vec<Clip> {
    if wants_keyboard {
        return Vec::new();
    }
    let mut out = Vec::new();
    let paste = if shift_held { Clip::PasteInPlace } else { Clip::Paste };
    events.retain(|e| match e {
        Event::Copy => {
            out.push(Clip::Copy);
            false
        }
        Event::Cut => {
            out.push(Clip::Cut);
            false
        }
        Event::Paste(_) => {
            out.push(paste);
            false
        }
        Event::Text(_) if command_held => {
            out.push(paste);
            false
        }
        _ => true,
    });
    out
}

/// The function keys pressed this frame, read from macroquad (egui-miniquad does not deliver them).
pub fn mq_presses() -> Vec<Binding> {
    use macroquad::prelude::KeyCode as K;
    const FN: [(K, Key); 12] = [
        (K::F1, Key::F1), (K::F2, Key::F2), (K::F3, Key::F3), (K::F4, Key::F4), (K::F5, Key::F5), (K::F6, Key::F6),
        (K::F7, Key::F7), (K::F8, Key::F8), (K::F9, Key::F9), (K::F10, Key::F10), (K::F11, Key::F11), (K::F12, Key::F12),
    ];
    // Through the UI's input layer (`ui::input`), as every screen reads its keys.
    let pressed = crate::ui::input::get_keys_pressed();
    let down = |a: K, b: K| crate::ui::input::is_key_down(a) || crate::ui::input::is_key_down(b);
    let (ctrl, shift, alt) = (down(K::LeftControl, K::RightControl), down(K::LeftShift, K::RightShift), down(K::LeftAlt, K::RightAlt));
    FN.iter()
        .filter(|(mq, _)| pressed.contains(mq))
        .map(|(_, key)| Binding { ctrl, shift, alt, key: *key })
        .collect()
}

/// The key a capture reads: a binding, or Escape (cancel).
#[derive(Debug, PartialEq, Eq)]
pub enum Capture {
    Key(Binding),
    Cancel,
}

/// The next key pressed while a binding is being captured (the settings page), if any.
pub fn capture(ctx: &egui::Context, mq: &[Binding]) -> Option<Capture> {
    let mut got = None;
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            Event::Key { key, pressed: true, repeat: false, modifiers, .. } if got.is_none() => {
                got = Some(if *key == Key::Escape && !modifiers.command && !modifiers.shift && !modifiers.alt {
                    Capture::Cancel
                } else {
                    Capture::Key(Binding::from_event(*key, *modifiers))
                });
                false
            }
            _ => true,
        });
    });
    got.or_else(|| mq.first().map(|b| Capture::Key(*b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Prefs {
        Prefs::new()
    }

    fn ctx_with(events: Vec<Event>, modifiers: Modifiers) -> egui::Context {
        let ctx = egui::Context::default();
        let raw = egui::RawInput { modifiers, events, ..Default::default() };
        ctx.begin_pass(raw);
        ctx
    }

    #[test]
    fn ids_are_unique_and_every_default_parses() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len(), "command ids are unique");
        for c in COMMANDS {
            if !c.default.is_empty() {
                let b = Binding::parse(c.default).unwrap_or_else(|| panic!("{} default {:?} parses", c.id, c.default));
                assert_eq!(b.text(), c.default, "{} default round-trips", c.id);
            }
            if !c.also.is_empty() {
                assert!(Binding::parse(c.also).is_some(), "{} also {:?} parses", c.id, c.also);
            }
        }
    }

    #[test]
    fn every_default_key_is_unique_in_its_context() {
        for a in COMMANDS {
            for b in COMMANDS {
                if a.id >= b.id || !overlaps(a.scope, b.scope) {
                    continue;
                }
                let ka = Binding::parse(a.default);
                let kb = Binding::parse(b.default);
                assert!(ka.is_none() || ka != kb, "{} and {} share {:?}", a.id, b.id, a.default);
                let also_a = Binding::parse(a.also);
                assert!(also_a.is_none() || also_a != kb && also_a != Binding::parse(b.also), "{} and {} share the also key", a.id, b.id);
            }
        }
    }

    #[test]
    fn a_binding_round_trips_through_its_text() {
        for text in ["Ctrl+S", "Ctrl+Shift+Z", "Alt+1", "F7", "Shift+[", "Ctrl+,", "Ctrl+Alt+S", "Esc"] {
            let b = Binding::parse(text).unwrap_or_else(|| panic!("{text} parses"));
            assert_eq!(b.text(), text);
        }
        assert_eq!(Binding::parse(""), None, "an empty text is unbound");
        assert_eq!(Binding::parse("Ctrl+Hyper+S"), None);
    }

    #[test]
    fn a_clashing_rebinding_is_refused_and_not_stored() {
        let mut prefs = defaults();
        let ctrl_s = Binding::parse("Ctrl+S");
        assert_eq!(rebind(&mut prefs, "undo", ctrl_s), Err(Refused::Clash("save")));
        assert!(prefs.is_empty(), "a refused binding is not saved");
        assert_eq!(rebind(&mut prefs, "copy", Binding::parse("Ctrl+K")), Err(Refused::Fixed));
        assert_eq!(rebind(&mut prefs, "undo", Binding::parse("Ctrl+Y")), Err(Refused::Clash("redo")), "the redo's Ctrl+Y clashes");
    }

    #[test]
    fn a_rebinding_round_trips_and_a_default_is_not_stored() {
        let mut prefs = defaults();
        let k = Binding::parse("Ctrl+Alt+Z");
        rebind(&mut prefs, "undo", k).expect("free key");
        assert_eq!(prefs.get("undo").map(String::as_str), Some("Ctrl+Alt+Z"));
        let text = serde_json::to_string(&prefs).expect("prefs are JSON");
        let back: Prefs = serde_json::from_str(&text).expect("and read back");
        assert_eq!(binding_of(command("undo").expect("undo"), &back), k);
        rebind(&mut prefs, "undo", Binding::parse("Ctrl+Z")).expect("back to the default");
        assert!(!prefs.contains_key("undo"), "the default is not stored");
        rebind(&mut prefs, "undo", k).expect("free key");
        reset(&mut prefs, Some("undo"));
        assert!(prefs.is_empty(), "reset puts one command back");
    }

    #[test]
    fn save_all_wins_over_save_when_shift_is_held() {
        let ctx = ctx_with(
            vec![Event::Key { key: Key::S, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers { command: true, shift: true, ..Default::default() } }],
            Modifiers { command: true, shift: true, ..Default::default() },
        );
        let ids = pressed(&ctx, &defaults(), |_| true, &[]);
        assert_eq!(ids, vec!["save-all"], "the exact modifiers pick Save all, not Save");
    }

    #[test]
    fn a_typed_key_is_left_to_a_text_field_and_a_shortcut_is_taken() {
        // Alt+1 is a shortcut; a plain 7 belongs to a focused field (no command has it by default; a plain 1 is
        // the brush size of the Map tools).
        let typed = Event::Key { key: Key::Num7, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let alt = Event::Key { key: Key::Num1, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers { alt: true, ..Default::default() } };
        let ctx = ctx_with(vec![alt.clone()], Modifiers { alt: true, ..Default::default() });
        assert_eq!(pressed(&ctx, &defaults(), |_| true, &[]), vec!["ws-map"]);
        let ctx = ctx_with(vec![typed], Modifiers::NONE);
        assert!(pressed(&ctx, &defaults(), |_| true, &[]).is_empty());
    }

    #[test]
    fn the_selection_shortcuts_are_the_spec_s() {
        let want = [
            ("tool-marquee", "M"),
            ("tool-lasso", "L"),
            ("tool-wand", "W"),
            ("select-all", "Ctrl+A"),
            ("deselect", "Ctrl+D"),
            ("invert", "Ctrl+Shift+I"),
            ("save-stamp", "Ctrl+Alt+S"),
            ("commit-paste", "Enter"),
            ("delete", "Delete"),
        ];
        for (id, key) in want {
            let c = command(id).unwrap_or_else(|| panic!("{id} is a command"));
            assert_eq!(binding_of(c, &defaults()), Binding::parse(key), "{id} is {key}");
            assert!(c.later.is_empty(), "{id} works now");
        }
        assert_eq!(command("paste-in-place").map(|c| c.default), Some("Ctrl+Shift+V"));
    }

    #[test]
    fn clipboard_comes_from_the_events_not_from_keys() {
        let mut events = vec![Event::Copy, Event::Cut, Event::Text("клип".into()), Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }];
        let got = clipboard(&mut events, true, false, false);
        assert_eq!(got, vec![Clip::Copy, Clip::Cut, Clip::Paste]);
        assert_eq!(events.len(), 1, "the clipboard events are taken, the key is kept");
        // Ctrl+Shift+V is a paste in place.
        let mut events = vec![Event::Paste("x".into())];
        assert_eq!(clipboard(&mut events, true, true, false), vec![Clip::PasteInPlace]);
        // A text field with the keyboard keeps its own clipboard.
        let mut events = vec![Event::Copy];
        assert!(clipboard(&mut events, true, false, true).is_empty());
        assert_eq!(events.len(), 1);
        // Typed text without the command key is not a paste.
        let mut events = vec![Event::Text("а".into())];
        assert!(clipboard(&mut events, false, false, false).is_empty());
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn the_function_keys_are_bindable_and_the_names_are_unique() {
        assert_eq!(key_of("F7"), Some(Key::F7));
        assert_eq!(key_of("["), Some(Key::OpenBracket));
        let mut names: Vec<&str> = KEY_NAMES.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KEY_NAMES.len(), "each key name once");
    }

    #[test]
    fn the_tooltip_has_the_name_and_the_shortcut() {
        let prefs = defaults();
        let save = command("save").expect("save");
        assert_eq!(tip(save, &prefs), "Save · Ctrl+S", "the English catalogue text is the name");
        let play = command("play").expect("play");
        assert!(tip(play, &prefs).starts_with("Play · F5"), "{}", tip(play, &prefs));
    }
}
