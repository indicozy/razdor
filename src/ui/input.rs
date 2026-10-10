//! Razdor's mouse and keys: macroquad's, merged with a gamepad's (`ui::gamepad`). The screens
//! read input through these functions, named and called as macroquad's (each file imports
//! them by name, which wins over `macroquad::prelude::*`), so a pad's A is a left click, its
//! pointer is the mouse's, its Start an Esc, wherever the game looks.
//!
//! The pad's pointer: it starts where the mouse is and, once the pad has moved it, is the
//! pointer until the real mouse moves or clicks again; then the mouse's is. While the pad's
//! pointer is in use the system pointer is hidden and Razdor draws one there (`ui::cursor`),
//! as the window's pointer cannot be moved. With no pad connected all of it is the mouse
//! and keyboard unchanged.

use std::cell::RefCell;
use std::collections::HashSet;

use macroquad::input as mq;
use macroquad::prelude::{get_frame_time, screen_height, screen_width, KeyCode, MouseButton};

use super::gamepad::{self, Edge, Frame, Mapper};

#[derive(Default)]
struct State {
    mapper: Mapper,
    frame: Frame,
    /// The pad's pointer is the pointer, at `at`.
    active: bool,
    at: (f32, f32),
    /// The mouse at the last frame.
    mouse: Option<(f32, f32)>,
    /// The map screen asked for the right stick this frame and the last ([`map_scroll`]).
    map_claimed: (bool, bool),
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Once per frame, before anything reads input: reads the pad and works out its mouse.
pub fn begin_frame() {
    let pad = gamepad::poll();
    let mouse = mq::mouse_position();
    let mouse_used = [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter().any(mq::is_mouse_button_pressed);
    let screen = (screen_width(), screen_height());
    // Out of focus the pad lets everything go: it is read even then, by its own device.
    let pad = pad.filter(|_| super::cursor::has_focus()).unwrap_or_default();
    STATE.with(|s| {
        let s = &mut *s.borrow_mut();
        if s.mouse.is_some_and(|m| m != mouse) || mouse_used {
            s.active = false;
        }
        s.mouse = Some(mouse);
        let from = if s.active { s.at } else { mouse };
        let map_scrolls = std::mem::take(&mut s.map_claimed.0);
        s.map_claimed.1 = map_scrolls;
        s.frame = s.mapper.step(&pad, get_frame_time(), from, screen, map_scrolls);
        if let Some(at) = s.frame.pointer {
            s.active = true;
            s.at = at;
        }
        // The window grew smaller: the pointer stays on it.
        if s.active {
            s.at = (s.at.0.clamp(0.0, (screen.0 - 1.0).max(0.0)), s.at.1.clamp(0.0, (screen.1 - 1.0).max(0.0)));
        }
    });
}

/// Where the pad's pointer is, while it is the pointer.
pub fn pad_pointer() -> Option<(f32, f32)> {
    STATE.with(|s| {
        let s = s.borrow();
        s.active.then_some(s.at)
    })
}

/// The map screen scrolls with the right stick: called each frame it shows, it gets the
/// stick (x right, y down, -1..1) and from the next frame the stick no longer turns the
/// wheel (the map's zoom).
pub fn map_scroll() -> (f32, f32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.map_claimed.0 = true;
        s.frame.scroll
    })
}

fn with_frame<R>(f: impl FnOnce(&Frame) -> R) -> R {
    STATE.with(|s| f(&s.borrow().frame))
}

fn button(b: MouseButton) -> Edge {
    with_frame(|f| match b {
        MouseButton::Left => f.left,
        MouseButton::Right => f.right,
        _ => Edge::default(),
    })
}

fn key_edge(k: KeyCode) -> Edge {
    with_frame(|f| f.keys.iter().find(|(c, _)| *c == k).map(|(_, e)| *e).unwrap_or_default())
}

fn keys_where(pick: impl Fn(&Edge) -> bool) -> Vec<KeyCode> {
    with_frame(|f| f.keys.iter().filter(|(_, e)| pick(e)).map(|(k, _)| *k).collect())
}

/// The pointer: the pad's while it is in use, else the mouse's.
pub fn mouse_position() -> (f32, f32) {
    pad_pointer().unwrap_or_else(mq::mouse_position)
}

/// The wheel this frame (x, y; up is positive): the mouse's and the pad's.
pub fn mouse_wheel() -> (f32, f32) {
    let (x, y) = mq::mouse_wheel();
    (x, y + with_frame(|f| f.wheel))
}

pub fn is_mouse_button_pressed(b: MouseButton) -> bool {
    mq::is_mouse_button_pressed(b) || button(b).pressed
}

pub fn is_mouse_button_down(b: MouseButton) -> bool {
    mq::is_mouse_button_down(b) || button(b).down
}

pub fn is_key_pressed(k: KeyCode) -> bool {
    mq::is_key_pressed(k) || key_edge(k).pressed
}

pub fn is_key_down(k: KeyCode) -> bool {
    mq::is_key_down(k) || key_edge(k).down
}

pub fn get_keys_pressed() -> HashSet<KeyCode> {
    let mut keys = mq::get_keys_pressed();
    keys.extend(keys_where(|e| e.pressed));
    keys
}

pub fn get_keys_released() -> HashSet<KeyCode> {
    let mut keys = mq::get_keys_released();
    keys.extend(keys_where(|e| e.released));
    keys
}

/// `k` went down this frame on the pad only (not on the keyboard).
pub fn pad_key_pressed(k: KeyCode) -> bool {
    key_edge(k).pressed && !mq::is_key_pressed(k)
}

#[cfg(test)]
mod tests {
    /// Every file of the interface reads the mouse and keys through this module: one calling
    /// macroquad's functions of the same names (through its prelude) would not see the pad.
    #[test]
    fn the_screens_read_input_through_razdors_layer() {
        const NAMES: [&str; 11] = [
            "mouse_position",
            "mouse_wheel",
            "is_mouse_button_pressed",
            "is_mouse_button_down",
            "is_mouse_button_released",
            "is_key_pressed",
            "is_key_down",
            "is_key_released",
            "get_keys_pressed",
            "get_keys_released",
            "get_keys_down",
        ];
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut all = Vec::new();
        files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"), &mut all);
        let mut wrong = Vec::new();
        for path in all {
            if path.ends_with("input.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            // The names this file imports from `input` (`use …input::{a, b};`), Razdor's or
            // macroquad's.
            let imports = |ours: bool| -> Vec<&str> {
                src.match_indices("input::{")
                    .filter(|(i, _)| src[..*i].ends_with("macroquad::") != ours)
                    .filter_map(|(i, m)| src[i + m.len()..].split_once('}').map(|(list, _)| list))
                    .flat_map(|list| list.split(',').map(str::trim))
                    .collect()
            };
            let (imported, macroquads) = (imports(true), imports(false));
            let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).map(|l| format!("{l}\n")).collect();
            for name in NAMES {
                // Used: called or passed on (`unwrap_or_else(mouse_position)`), outside comments.
                let ident = |c: char| c.is_alphanumeric() || c == '_';
                let used = code.match_indices(name).any(|(i, _)| {
                    let before = code[..i].chars().next_back();
                    let after = code[i + name.len()..].chars().next();
                    !before.is_some_and(|c| ident(c) || c == ':') && !after.is_some_and(ident) && !code[..i].ends_with("fn ")
                });
                let imported = imported.contains(&name) || src.contains(&format!("use input::{name};"));
                let qualified = macroquads.contains(&name) || src.contains(&format!("macroquad::input::{name}")) || src.contains(&format!("macroquad::prelude::{name}"));
                if (used && !imported) || qualified {
                    wrong.push(format!("{}: {name}", path.display()));
                }
            }
        }
        assert!(wrong.is_empty(), "read macroquad's input directly instead of ui::input's:\n{}", wrong.join("\n"));
    }
}
