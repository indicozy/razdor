//! The saves of an older release (Android). Razdor keeps its files in its own folder and asks for no
//! storage permission, so the saves a player kept in the shared «Razdor» folder come in through the
//! system's folder picker: the player picks that folder, the Java side copies its saves into the
//! cache (`razdorPickOldSaves`, `razdorOldSavesOutcome`, `razdorOldSavesProgress`), and they are
//! merged into the app's folder with the import's own rules (`transfer::import_folder`). The copy
//! shows its progress in a window («Moving the saves»); the import runs on a thread behind the same
//! window, and the screen under it takes no input until the move ends.
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
use std::thread::JoinHandle;

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

/// What the marker says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Fresh,
    Existing,
    Done,
}

/// The copy's progress from the picker's poll (`razdorOldSavesProgress`): the files copied so far,
/// and the total once the folder is counted (None while it is being counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

/// The import's result: the counts of the saves and the hub, or the reason it failed.
type Import = Result<(transfer::Imported, Option<hub::OldHubImport>), String>;

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

/// The snapshot's popup fake: `RAZDOR_SCENE_SAVES_MOVE` with `RAZDOR_SNAPSHOT` (a copy's fake is
/// another scene, see [`copy_fake`]).
fn snapshot_popup() -> bool {
    std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var("RAZDOR_SCENE_SAVES_MOVE").is_ok_and(|scene| copy_fake(&scene).is_none())
}

/// Offers the popup once in this run, when [`due`] says so (`loaded`: a usable install is loaded).
pub fn offer_if_due(loaded: bool) {
    if SHOWN.get() || STATE.with_borrow(|s| !matches!(s, State::Hidden)) {
        return;
    }
    if due(loaded, snapshot_popup(), decision()) {
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

/// The older release's hub folder in the copy (`hub`: its installed maps and mods) merged into the
/// app's hub (`hub::import_old`, into `local`). None when the copy has none or the import fails: the
/// saves are already in, and a failure is only logged.
fn copy_hub(copy: &std::path::Path, local: Option<&std::path::Path>) -> Option<hub::OldHubImport> {
    let src = copy.join("hub");
    if !src.is_dir() {
        return None;
    }
    match hub::import_old(&src, local?) {
        Ok(report) => Some(report),
        Err(why) => {
            razdor::diag::log(&format!("saves move: the hub's maps and mods are not moved: {why}"));
            None
        }
    }
}

/// The copied folder (`old-saves` in the cache) imported into the app's saves folder on a thread (the
/// import only reads and writes files); the copy is removed when it ends, either way. The folders are
/// taken here, on the main thread.
fn start_import(popup: bool) -> State {
    let copy = platform::cache_dir().map(|dir| dir.join("old-saves"));
    let root = platform::app_dir().map(|dir| dir.join("saves"));
    let (Some(copy), Some(root)) = (copy, root) else {
        return State::Offer(popup, Some(tr("The folder cannot be read.").to_string()));
    };
    let local = hub::local_dir();
    let cleanup = copy.clone();
    let spawned = std::thread::Builder::new().name("saves-move".into()).spawn(move || {
        let result = transfer::import_folder(&copy, &root).map(|imported| (imported, copy_hub(&copy, local.as_deref())));
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

/// The import's end, on the main thread: a success records the move as done (the popup is gone for
/// good); a failure is offered again with its reason.
fn finish_import(popup: bool, joined: std::thread::Result<Import>) -> State {
    match joined {
        Ok(Ok((imported, old_hub))) => {
            write_decision("done");
            let mut line = trf!("Saves moved: {n}.", n = imported.saves);
            if let Some(hub) = old_hub {
                line = format!("{line} {}", trf!("Maps and mods: {n}.", n = hub.entries));
            }
            if imported.settings_kept {
                line = format!("{line} {}", tr("Settings were not replaced: they are already there."));
            }
            State::Done(line)
        }
        Ok(Err(why)) => State::Offer(popup, Some(why)),
        Err(_) => {
            razdor::diag::log("saves move: the import ended without a result");
            State::Offer(popup, Some(tr("The folder cannot be read.").to_string()))
        }
    }
}

/// The snapshot's copy fake (`RAZDOR_SCENE_SAVES_MOVE`, with `RAZDOR_SNAPSHOT`): see [`copy_fake`].
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
}
