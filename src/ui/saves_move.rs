//! The saves of an older release (Android). Razdor keeps its files in its own folder and asks for no
//! storage permission, so the saves a player kept in the shared «Razdor» folder come in through the
//! system's folder picker: the player picks that folder, the Java side copies its saves into the
//! cache (`razdorPickOldSaves`, `razdorOldSavesOutcome`), and they are merged into the app's folder
//! with the import's own rules (`transfer::import_folder`).
//!
//! Who is offered the popup (an existing player): decided once, at the process's start, before
//! anything writes to the cache ([`decide_at_start`]). A private cache that already holds files
//! is an older install (older releases kept their picture cache and their update files there);
//! an empty one is a fresh install, which never sees the popup. The decision is kept in the app's
//! folder (`saves-move`: "existing", "fresh" or "done"). The popup comes back at each start until
//! the import ends; the settings' «Перенести сохранения из старой папки Razdor…» is always there.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
#[cfg(target_os = "android")]
use std::path::Path;

use razdor::i18n::tr;
use razdor::{hub, local, platform, transfer, trf};

use super::audio::{cue, Cue};
use super::chrome;
use super::dt_font;
use super::map_import;
use super::widgets;
use macroquad::prelude::*;

/// The file in the app's folder that keeps the decision (see the module doc).
const MARKER: &str = "saves-move";

/// The popup's text: what changed, and what to pick.
const OFFER: &str = "In version 1.5.0 Razdor keeps its saves in its own folder. To move the old saves, pick the Razdor folder in the device storage.";

/// What the marker says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Fresh,
    Existing,
    Done,
}

/// Where the move stands.
#[derive(Default)]
enum State {
    #[default]
    Hidden,
    /// The popup (`true`: the one at start; `false`: the settings' window, which cancels to nothing),
    /// with a note from the last try, if any.
    Offer(bool, Option<String>),
    /// The picker is open or its folder is being copied (the flag as in `Offer`).
    Picking(bool),
    /// The result, one line, to OK.
    Done(String),
}

local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Hidden) };
    /// The popup was offered in this run: it is not offered again until the next start.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

fn marker() -> Option<PathBuf> {
    platform::app_dir().map(|dir| dir.join(MARKER))
}

/// The decision a marker's text says; None for anything else.
fn parse(text: &str) -> Option<Decision> {
    match text.trim() {
        "existing" => Some(Decision::Existing),
        "fresh" => Some(Decision::Fresh),
        "done" => Some(Decision::Done),
        _ => None,
    }
}

fn decision() -> Option<Decision> {
    parse(&fs::read_to_string(marker()?).ok()?)
}

fn write_decision(text: &str) {
    let Some(path) = marker() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, text);
}

/// The start of the process, before anything writes to the cache (Android): records whether this
/// is an older install (`cache` holds files) or a fresh one. A decision already made stands.
#[cfg(target_os = "android")]
pub fn decide_at_start(cache: Option<&Path>) {
    if decision().is_some() {
        return;
    }
    let had_files = cache.and_then(|dir| fs::read_dir(dir).ok()).is_some_and(|mut entries| entries.next().is_some());
    write_decision(if had_files { "existing" } else { "fresh" });
}

/// Whether the popup at start is due: only once a usable game install is loaded (never over the
/// no-install screen or a download), and only for an older install (the decision says existing).
/// `snapshot` is the snapshot's `RAZDOR_SCENE_SAVES_MOVE` fake, which stands in for the decision.
fn due(loaded: bool, snapshot: bool, decision: Option<Decision>) -> bool {
    loaded && (snapshot || decision == Some(Decision::Existing))
}

/// Offers the popup once in this run, when [`due`] says so (`loaded`: a usable install is loaded).
/// The snapshot's `RAZDOR_SCENE_SAVES_MOVE` shows it (with `RAZDOR_SNAPSHOT`).
pub fn offer_if_due(loaded: bool) {
    if SHOWN.get() || STATE.with_borrow(|s| !matches!(s, State::Hidden)) {
        return;
    }
    let snapshot = std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var_os("RAZDOR_SCENE_SAVES_MOVE").is_some();
    if due(loaded, snapshot, decision()) {
        SHOWN.set(true);
        STATE.with_borrow_mut(|s| *s = State::Offer(true, None));
    }
}

/// The settings' button: the window opens at once (it stays available after the popup's «Не сейчас»).
pub fn open_from_settings() {
    STATE.with_borrow_mut(|s| {
        if matches!(s, State::Hidden) {
            *s = State::Offer(false, None);
        }
    });
}

/// Whether a window of this module is open: the screen under it takes no input.
pub fn open() -> bool {
    STATE.with_borrow(|s| !matches!(s, State::Hidden))
}

/// Whether the picker is open or the folder is being copied: the game's start and load wait.
pub fn busy() -> bool {
    STATE.with_borrow(|s| matches!(s, State::Picking(_)))
}

/// A game's start or load was asked while [`busy`]: nothing is started (the log says so).
pub fn refuse() {
    razdor::diag::log("saves move: the start waits until the folder is read");
}

#[cfg(target_os = "android")]
fn pick_start() -> bool {
    super::android::start_old_saves_pick()
}

#[cfg(not(target_os = "android"))]
fn pick_start() -> bool {
    false
}

#[cfg(target_os = "android")]
fn pick_outcome() -> String {
    super::android::old_saves_outcome()
}

#[cfg(not(target_os = "android"))]
fn pick_outcome() -> String {
    String::new()
}

/// Each frame: the picker's answer, when it has one. A copied folder is imported at once.
pub fn tick() {
    let popup = match STATE.with_borrow(|s| if let State::Picking(popup) = s { Some(*popup) } else { None }) {
        Some(popup) => popup,
        None => return,
    };
    let outcome = pick_outcome();
    let next = match outcome.as_str() {
        "" => return,
        "cancel" => if popup { State::Offer(true, None) } else { State::Hidden },
        "wrong" => State::Offer(popup, Some(tr("This folder holds no Razdor saves. Pick the Razdor folder in the device storage.").to_string())),
        "copied" => finish_copy(popup),
        other => {
            razdor::diag::log(&format!("saves move: the folder cannot be read: {other}"));
            State::Offer(popup, Some(tr("The folder cannot be read.").to_string()))
        }
    };
    STATE.with_borrow_mut(|s| *s = next);
}

/// The older release's hub folder in the copy (`hub`: its installed maps and mods) merged into the
/// app's hub (`hub::import_old`). None when the copy has none or the import fails: the saves are
/// already in, and a failure is only logged.
fn copy_hub(copy: &std::path::Path) -> Option<hub::OldHubImport> {
    let src = copy.join("hub");
    if !src.is_dir() {
        return None;
    }
    let local = hub::local_dir()?;
    match hub::import_old(&src, &local) {
        Ok(report) => Some(report),
        Err(why) => {
            razdor::diag::log(&format!("saves move: the hub's maps and mods are not moved: {why}"));
            None
        }
    }
}

/// The copied folder (`old-saves` in the cache) imported into the app's saves folder; the copy is
/// removed either way. A success records the move as done (the popup is gone for good).
fn finish_copy(popup: bool) -> State {
    let copy = platform::cache_dir().map(|dir| dir.join("old-saves"));
    let root = platform::app_dir().map(|dir| dir.join("saves"));
    let result = match (&copy, root) {
        (Some(copy), Some(root)) => transfer::import_folder(copy, &root).map(|imported| (imported, copy_hub(copy))),
        _ => Err(tr("The folder cannot be read.").to_string()),
    };
    if let Some(copy) = &copy {
        let _ = fs::remove_dir_all(copy);
    }
    match result {
        Ok((imported, hub)) => {
            write_decision("done");
            let mut line = trf!("Saves moved: {n}.", n = imported.saves);
            if let Some(hub) = hub {
                line = format!("{line} {}", trf!("Maps and mods: {n}.", n = hub.entries));
            }
            if imported.settings_kept {
                line = format!("{line} {}", tr("Settings were not replaced: they are already there."));
            }
            State::Done(line)
        }
        Err(why) => State::Offer(popup, Some(why)),
    }
}

/// The window of this module, drawn over the screen (after the others, so it is on top).
pub fn modal() {
    let state = STATE.with_borrow_mut(std::mem::take);
    let next = match state {
        State::Hidden => State::Hidden,
        State::Offer(popup, note) => {
            let mut lines = vec![tr(OFFER).to_string()];
            lines.extend(note.clone());
            let k = chrome::k();
            let f = map_import::phone_factor();
            // Two full-width buttons, one under the other: the frame reserves room for one row only, so
            // blank lines make the room for the second (a blank line is one text line of the window).
            let btn_h = super::display::dp_px().map_or(28.0 * k, |dp| 44.0 * dp);
            let blank = ((2.0 * btn_h + 36.0 * k * f - 58.0 * k * f) / (20.0 * k * f)).ceil().max(0.0) as usize;
            lines.extend(std::iter::repeat_n("\u{200B}".to_string(), blank));
            let (inner, closed) = map_import::frame(tr("Saves from an older release"), &lines, false);
            let x = inner.x + 24.0 * k * f;
            let w = inner.w - 48.0 * k * f;
            let second = Rect::new(x, inner.y + inner.h - 14.0 * k * f - btn_h, w, btn_h);
            let first = Rect::new(x, second.y - 12.0 * k * f - btn_h, w, btn_h);
            let labels = [tr("Pick the Razdor folder"), tr("Not now")];
            let mut picked = None;
            for (i, (r, label)) in [first, second].into_iter().zip(labels).enumerate() {
                let (_, down, pressed) = widgets::button_state(r, true);
                if f > 1.0 {
                    dt_font::with_legacy_metrics(|| chrome::marble_button_sized(r, label, true, down, 14.0 * k * f));
                } else {
                    chrome::marble_button(r, label, true, down);
                }
                if pressed {
                    cue(Cue::Button);
                    picked = Some(i);
                }
            }
            match picked {
                Some(0) => {
                    if pick_start() {
                        State::Picking(popup)
                    } else {
                        State::Offer(popup, Some(tr("The folder picker did not open.").to_string()))
                    }
                }
                Some(_) => State::Hidden,
                None if closed => State::Hidden,
                None => State::Offer(popup, note),
            }
        }
        State::Picking(popup) => State::Picking(popup),
        State::Done(line) => {
            if map_import::notice_window(tr("Saves from an older release"), std::slice::from_ref(&line)) {
                State::Hidden
            } else {
                State::Done(line)
            }
        }
    };
    STATE.with_borrow_mut(|s| *s = next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_at_start_waits_for_a_loaded_game_and_an_older_install() {
        // Never over the no-install screen or a download: only once a usable install is loaded.
        assert!(!due(false, false, Some(Decision::Existing)));
        assert!(!due(false, true, None), "the snapshot's fake needs a loaded game too");
        // An older install's saves not yet moved: due, with the game loaded.
        assert!(due(true, false, Some(Decision::Existing)));
        // A fresh install, or a move already done, never sees it.
        assert!(!due(true, false, Some(Decision::Fresh)));
        assert!(!due(true, false, Some(Decision::Done)));
        assert!(!due(true, false, None));
    }

    #[test]
    fn the_marker_says_existing_fresh_or_done() {
        assert_eq!(parse("existing\n"), Some(Decision::Existing));
        assert_eq!(parse("fresh"), Some(Decision::Fresh));
        assert_eq!(parse(" done "), Some(Decision::Done));
        assert_eq!(parse("moved by hand"), None, "anything else is no decision");
        assert_eq!(parse(""), None);
    }
}
