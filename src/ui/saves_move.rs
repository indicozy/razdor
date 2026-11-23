//! The saves of an older release (Android). Razdor keeps its files in its own folder and asks for no
//! storage permission, so the saves a player kept in the shared «Razdor» folder come in through the
//! system's folder picker: the player picks that folder, the Java side copies its saves into the
//! cache (`razdorPickOldSaves`, `razdorOldSavesOutcome`, `razdorOldSavesProgress`), and they are
//! merged into the app's folder with the import's own rules (`transfer::import_folder`). The copy
//! shows its progress in a window («Moving the saves»); the import runs on a thread behind the same
//! window, and the screen under it takes no input until the move ends.
//!
//! The popup is no longer offered at start (the owner's decision, 2026-11-23). The old release's
//! saves come in only from the settings' «Перенести сохранения из старой папки Razdor…», which
//! opens this window ([`open_from_settings`]). [`offer_if_due`] serves only the snapshot scene
//! `RAZDOR_SCENE_SAVES_MOVE`; it writes nothing.

use std::cell::{Cell, RefCell};
use std::fs;
use std::thread::JoinHandle;

use razdor::i18n::tr;
use razdor::rules::save;
use razdor::{hub, local, platform, transfer, trf};

use super::audio::{cue, Cue};
use super::chrome;
use super::dt_font;
use super::map_import;
use super::widgets;
use macroquad::prelude::*;

/// The copy's progress from the picker's poll (`razdorOldSavesProgress`): the files copied so far,
/// and the total once the folder is counted (None while it is being counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

/// The move's result: what came in (`transfer::move_old_release`), or the reason it failed.
type Import = Result<transfer::OldRelease, String>;

/// Where the move stands.
#[derive(Default)]
enum State {
    #[default]
    Hidden,
    /// The popup (`true`: the one at start; `false`: the settings' window, which cancels to nothing),
    /// with a note from the last try, if any.
    Offer(bool, Option<String>),
    /// The picker is open (the flag as in `Offer`).
    Picking(bool),
    /// The picked folder is copied into the cache, with its progress so far (the flag as in `Offer`).
    Copying(bool, Progress),
    /// The copied folder is imported on a thread (the flag as in `Offer`); the copy is removed when it ends.
    Importing(bool, JoinHandle<Import>),
    /// The result, one line, to OK.
    Done(String),
}

local! {
    static STATE: RefCell<State> = const { RefCell::new(State::Hidden) };
    /// The popup was offered in this run: it is not offered again until the next start.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
    /// A move ended with its saves and hub in place, not yet read by the App ([`take_finished`]).
    static FINISHED: Cell<bool> = const { Cell::new(false) };
}

/// Whether the popup is due: never at start any more (the owner's decision, 2026-11-23). Only the
/// snapshot's `RAZDOR_SCENE_SAVES_MOVE` fake, and only once a usable game install is loaded (never
/// over the no-install screen or a download).
fn due(loaded: bool, snapshot: bool) -> bool {
    loaded && snapshot
}

/// The snapshot's popup fake: `RAZDOR_SCENE_SAVES_MOVE` with `RAZDOR_SNAPSHOT` (a copy's fake is
/// another scene, see [`copy_fake`]).
fn snapshot_popup() -> bool {
    std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var("RAZDOR_SCENE_SAVES_MOVE").is_ok_and(|scene| copy_fake(&scene).is_none())
}

/// Offers the popup once in this run, when [`due`] says so: only the snapshot scene
/// `RAZDOR_SCENE_SAVES_MOVE` gets it (`loaded`: a usable install is loaded). Writes nothing.
pub fn offer_if_due(loaded: bool) {
    if SHOWN.get() || STATE.with_borrow(|s| !matches!(s, State::Hidden)) {
        return;
    }
    if due(loaded, snapshot_popup()) {
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

/// Whether the picker is open, or the folder is being copied or imported: the game's start and load wait.
pub fn busy() -> bool {
    STATE.with_borrow(|s| matches!(s, State::Picking(_) | State::Copying(..) | State::Importing(..)))
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

#[cfg(target_os = "android")]
fn pick_progress() -> String {
    super::android::old_saves_progress()
}

#[cfg(not(target_os = "android"))]
fn pick_progress() -> String {
    String::new()
}

/// The poll's progress text ("copied/total", total -1 while counted); None for "" (no copy yet) and
/// anything else.
fn parse_progress(text: &str) -> Option<Progress> {
    let (done, total) = text.trim().split_once('/')?;
    let total: i64 = total.parse().ok()?;
    Some(Progress { done: done.parse().ok()?, total: u64::try_from(total).ok() })
}

/// One frame of the picker's poll, while the picker is open or the folder is copied. The outcome
/// when it has one (`"copied"` goes to `copied`, which starts the import); else the copy's progress
/// once the copy runs (`last` keeps it when the poll has none).
fn poll(popup: bool, last: Option<Progress>, outcome: &str, progress: &str, copied: impl FnOnce(bool) -> State) -> State {
    match outcome {
        "" => match parse_progress(progress).or(last) {
            Some(now) => State::Copying(popup, now),
            None => State::Picking(popup),
        },
        "cancel" => {
            if popup {
                State::Offer(true, None)
            } else {
                State::Hidden
            }
        }
        "wrong" => State::Offer(popup, Some(tr("This folder holds no Razdor saves. Pick the Razdor folder in the device storage.").to_string())),
        "copied" => copied(popup),
        other => {
            razdor::diag::log(&format!("saves move: the folder cannot be read: {other}"));
            State::Offer(popup, Some(tr("The folder cannot be read.").to_string()))
        }
    }
}

/// Each frame: the picker's answer and the copy's progress, and the import's end.
pub fn tick() {
    let next = match STATE.with_borrow_mut(std::mem::take) {
        State::Picking(popup) => poll(popup, None, &pick_outcome(), &pick_progress(), start_import),
        State::Copying(popup, last) => poll(popup, Some(last), &pick_outcome(), &pick_progress(), start_import),
        State::Importing(popup, handle) => {
            if handle.is_finished() {
                finish_import(popup, handle.join())
            } else {
                State::Importing(popup, handle)
            }
        }
        other => other,
    };
    STATE.with_borrow_mut(|s| *s = next);
}

/// The copied folder (`old-saves` in the cache) moved into the app's saves folder on a thread (the
/// move only reads and writes files, see `transfer::move_old_release`); the copy is removed when it
/// ends, either way. The folders and the install are taken here, on the main thread.
fn start_import(popup: bool) -> State {
    let copy = platform::cache_dir().map(|dir| dir.join("old-saves"));
    let root = platform::app_dir().map(|dir| dir.join("saves"));
    let (Some(copy), Some(root)) = (copy, root) else {
        return State::Offer(popup, Some(tr("The folder cannot be read.").to_string()));
    };
    let local = hub::local_dir();
    let install = save::install_dir();
    let cleanup = copy.clone();
    let spawned = std::thread::Builder::new().name("saves-move".into()).spawn(move || {
        let result = transfer::move_old_release(&copy, &root, local.as_deref(), install.as_deref());
        let _ = fs::remove_dir_all(&copy);
        result
    });
    match spawned {
        Ok(handle) => State::Importing(popup, handle),
        Err(_) => {
            let _ = fs::remove_dir_all(&cleanup);
            State::Offer(popup, Some(tr("The folder cannot be read.").to_string()))
        }
    }
}

/// The import's end, on the main thread: a success is shown and the App reads the hub again; a
/// failure is offered again with its reason.
fn finish_import(popup: bool, joined: std::thread::Result<Import>) -> State {
    match joined {
        Ok(Ok(release)) => {
            FINISHED.set(true);
            State::Done(outcome_text(&release))
        }
        Ok(Err(why)) => State::Offer(popup, Some(why)),
        Err(_) => {
            razdor::diag::log("saves move: the import ended without a result");
            State::Offer(popup, Some(tr("The folder cannot be read.").to_string()))
        }
    }
}

/// The move's result as the player reads it: what came into this game, what this game already had,
/// what waits for another map or mod (it shows when that game is played), the hub's maps and mods,
/// and the settings kept. Never «moved» for a save that is not there.
fn outcome_text(release: &transfer::OldRelease) -> String {
    let mut parts: Vec<String> = Vec::new();
    if release.placed > 0 {
        parts.push(trf!("Saves moved: {n}.", n = release.placed));
    }
    if release.duplicates > 0 {
        parts.push(trf!("Saves already in this game: {n}.", n = release.duplicates));
    }
    if release.left > 0 {
        parts.push(trf!("Saves of other maps and mods, they show when their game is played: {n}.", n = release.left));
    }
    if parts.is_empty() {
        parts.push(tr("No saves of this game were found in the folder.").to_string());
    }
    if let Some(hub) = &release.hub {
        parts.push(trf!("Maps and mods: {n}.", n = hub.entries));
    }
    if release.imported.settings_kept {
        parts.push(tr("Settings were not replaced: they are already there.").to_string());
    }
    parts.join(" ")
}

/// Whether a move ended since the last look: the App reads the hub's maps and mods again then (the
/// mod choice and the new-game list), since a move brings them in while the game runs.
pub fn take_finished() -> bool {
    FINISHED.replace(false)
}

/// The snapshot's copy fake (`RAZDOR_SNAPSHOT`, `RAZDOR_SCENE_SAVES_MOVE`): see [`copy_fake`].
fn snapshot_copy() -> Option<Progress> {
    std::env::var_os("RAZDOR_SNAPSHOT")?;
    copy_fake(&std::env::var("RAZDOR_SCENE_SAVES_MOVE").ok()?)
}

/// The copy's fake for a snapshot: `copying:<percent>` (the bar at that share) or `copying` (no total:
/// the animated line). None for any other scene.
fn copy_fake(scene: &str) -> Option<Progress> {
    let rest = scene.strip_prefix("copying")?;
    match rest.strip_prefix(':') {
        Some(percent) => Some(Progress { done: percent.parse::<u64>().ok()?.min(100), total: Some(100) }),
        None if rest.is_empty() => Some(Progress { done: 0, total: None }),
        None => None,
    }
}

/// The window of the copy and of the import, «Moving the saves»: its line, and while the copy runs
/// with a known total, the files so far («N of M files») with the bar. Without one (the files are
/// being counted, and the import) an animated ellipsis and no bar. No buttons: the screen under it waits.
fn moving_window(progress: Option<Progress>) {
    let title = tr("Moving the saves");
    let line = tr("Moving the saves, maps and mods…").to_string();
    match progress.and_then(|p| p.total.map(|total| (p.done, total))) {
        Some((done, total)) => {
            let share = if total == 0 { 1.0 } else { (done as f32 / total as f32).min(1.0) };
            let count = trf!("{done} of {total} files", done = done, total = total);
            let (_, _, bar) = map_import::frame_bar(title, &[line, count], Some(share), false);
            if let Some(bar) = bar {
                map_import::draw_bar(bar, share);
            }
        }
        None => {
            let ellipsis = "…".repeat((get_time() * 2.0) as usize % 3 + 1);
            map_import::frame_bar(title, &[line, ellipsis], None, false);
        }
    }
}

/// The window of this module, drawn over the screen (after the others, so it is on top).
pub fn modal() {
    let state = STATE.with_borrow_mut(std::mem::take);
    let next = match state {
        State::Hidden => {
            if let Some(progress) = snapshot_copy() {
                moving_window(Some(progress));
            }
            State::Hidden
        }
        State::Offer(popup, note) => {
            // The text is the owner's (2026-11-12): the literal here, where the catalog scan reads it.
            let mut lines = vec![tr("In version 1.5.0 Razdor keeps its files in the app's folder. To move the saves, maps and mods, pick the Razdor folder in the device storage.").to_string()];
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
        State::Copying(popup, progress) => {
            moving_window(Some(progress));
            State::Copying(popup, progress)
        }
        State::Importing(popup, handle) => {
            moving_window(None);
            State::Importing(popup, handle)
        }
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
    fn the_popup_is_never_due_at_start_only_for_the_snapshot_fake() {
        // A real player never sees the popup at start, whatever the install holds.
        assert!(!due(true, false));
        // The snapshot's fake needs a loaded game too.
        assert!(due(true, true));
        assert!(!due(false, true));
    }

    #[test]
    fn the_settings_row_opens_the_window() {
        assert!(!open());
        open_from_settings();
        assert!(open());
        STATE.with_borrow_mut(|s| *s = State::Hidden);
    }

    #[test]
    fn the_copy_progress_is_done_over_total_or_counting() {
        assert_eq!(parse_progress("2/10"), Some(Progress { done: 2, total: Some(10) }));
        assert_eq!(parse_progress(" 0/-1 \n"), Some(Progress { done: 0, total: None }), "the folder is being counted");
        assert_eq!(parse_progress(""), None, "the picker is still open: no copy yet");
        assert_eq!(parse_progress("3"), None);
        assert_eq!(parse_progress("x/3"), None);
    }

    #[test]
    fn the_snapshot_copy_fake_gives_a_bar_or_none() {
        assert_eq!(copy_fake("copying:40"), Some(Progress { done: 40, total: Some(100) }));
        assert_eq!(copy_fake("copying:250"), Some(Progress { done: 100, total: Some(100) }));
        assert_eq!(copy_fake("copying"), Some(Progress { done: 0, total: None }));
        assert_eq!(copy_fake("copying:x"), None);
        assert_eq!(copy_fake("1"), None, "the popup's fake is another scene");
    }

    #[test]
    fn the_picker_poll_goes_from_the_picker_to_the_copy_and_the_import() {
        let copied = |_: bool| State::Done("copied".to_string());
        // The picker is open: nothing is copied yet.
        assert!(matches!(poll(true, None, "", "", copied), State::Picking(true)));
        // The copy has begun: its progress, while the folder is counted first.
        assert!(matches!(poll(true, None, "", "0/-1", copied), State::Copying(true, Progress { done: 0, total: None })));
        assert!(matches!(poll(true, None, "", "4/9", copied), State::Copying(true, Progress { done: 4, total: Some(9) })));
        // A poll with no progress keeps the last one.
        let last = Some(Progress { done: 4, total: Some(9) });
        assert!(matches!(poll(false, last, "", "", copied), State::Copying(false, Progress { done: 4, total: Some(9) })));
        // The copy's answer starts the import (`copied`), whatever the progress was.
        assert!(matches!(poll(true, last, "copied", "9/9", copied), State::Done(_)));
        // Cancel: the popup goes back; the settings' window closes.
        assert!(matches!(poll(true, last, "cancel", "", copied), State::Offer(true, None)));
        assert!(matches!(poll(false, None, "cancel", "", copied), State::Hidden));
        // A folder without saves is offered again, with the reason.
        assert!(matches!(poll(false, last, "wrong", "", copied), State::Offer(false, Some(_))));
    }

    #[test]
    fn the_move_says_what_is_in_this_game_and_never_moved_for_a_save_that_is_not_there() {
        let release = |placed, duplicates, left| transfer::OldRelease { placed, duplicates, left, ..Default::default() };
        // Saves that came into this game: «moved».
        assert_eq!(outcome_text(&release(2, 0, 0)), trf!("Saves moved: {n}.", n = 2));
        // The game already had them: not «moved», and not a «0 moved» line.
        assert_eq!(outcome_text(&release(0, 3, 0)), trf!("Saves already in this game: {n}.", n = 3));
        // Saves of other maps and mods wait for their game, said plainly.
        let waiting = trf!("Saves of other maps and mods, they show when their game is played: {n}.", n = 1);
        assert_eq!(outcome_text(&release(2, 0, 1)), format!("{} {waiting}", trf!("Saves moved: {n}.", n = 2)));
        // Nothing for this game at all: said so.
        assert_eq!(outcome_text(&release(0, 0, 0)), tr("No saves of this game were found in the folder."));
        // A failed hub merge is not in the line; a merged hub is.
        let with_hub = transfer::OldRelease { hub: Some(hub::OldHubImport { entries: 4, skipped: 0 }), ..release(1, 0, 0) };
        assert!(outcome_text(&with_hub).ends_with(&trf!("Maps and mods: {n}.", n = 4)));
    }
}
