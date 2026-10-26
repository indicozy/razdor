//! «Обновить игру из архива…» in the settings window of the main menu: the platform's file picker
//! (as «Добавить карту…», `ui::map_import`), the archive examined, a question before a whole game
//! replaces the install, and the import on a thread with its progress (`dt::game_import`).
//!
//! Only the phones' unpack folder is written (`dt::game_import::target`): a player's own copy
//! elsewhere, a `RAZDOR_DT_DIR` or a Steam or GOG folder is never replaced, and the row says so.
//! The row is not offered over a map (the game under way reads the install).
//!
//! The pick: the desktop's dialog (`rfd`) on a thread, or on the main thread on macOS; Android's
//! document picker through the JNI (`ui::android`), which the Java side copies into the app's
//! cache by streaming; iOS has no picker, so the row says so.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use macroquad::prelude::*;
use razdor::dt::game_import::{self as import, Done, Error};
use razdor::dt::unpack::{Found, Kind, UnpackError};
use razdor::i18n::tr;
use razdor::local;
use razdor::platform;
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome;
use super::map_import;
use super::widgets::*;

/// The window's title.
fn title() -> &'static str {
    tr("Update the game")
}

/// The picked file: its name, where its bytes are, and whether the copy there is Razdor's to delete.
struct Picked {
    name: String,
    path: PathBuf,
    cached: bool,
}

/// Where the pick stands.
#[derive(Default)]
enum Picker {
    #[default]
    Idle,
    /// The desktop dialog's answer (`None`: cancelled), sent by its thread.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Desktop(std::sync::mpsc::Receiver<Option<PathBuf>>),
    /// The Android picker is open: its answer comes through `ui::android::pick_outcome`.
    #[cfg(target_os = "android")]
    Android,
}

/// An archive examined and waiting for the player's answer: the install folder it goes to, and
/// the cache copy to delete when it is done with.
struct Pending {
    found: Found,
    target: PathBuf,
    cleanup: Option<PathBuf>,
}

/// A window of this module, shown over the settings until the player answers it.
enum Modal {
    /// The whole game is to be replaced: Replace or Cancel.
    Confirm(Pending),
    /// What happened, or why nothing did: OK.
    Notice { title: String, lines: Vec<String> },
}

/// The import running on a thread: its handle and the bytes done and in all.
struct Job {
    name: String,
    handle: Option<JoinHandle<Result<Done, Error>>>,
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

local! {
    static REQUESTED: Cell<bool> = const { Cell::new(false) };
    static PICKER: RefCell<Picker> = const { RefCell::new(Picker::Idle) };
    static MODAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
    /// The folder an import writes to this frame (`dt::game_import::target`), None when not offered.
    static TARGET: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// The row's button was pressed: the picker opens at the next frame (`tick`).
fn request() {
    REQUESTED.with(|r| r.set(true));
}

/// True while the import has a window or a job of its own: the settings under it take no input.
pub fn busy() -> bool {
    MODAL.with_borrow(|m| m.is_some()) || JOB.with_borrow(|j| j.is_some()) || PICKER.with_borrow(|p| !matches!(p, Picker::Idle))
}

/// Whether the row offers the import: the install is the phones' unpack folder (or there is none
/// yet). The snapshot's `RAZDOR_SCENE_IMPORT` shows the offer without an install.
fn offered() -> bool {
    TARGET.with_borrow(|t| t.is_some()) || snapshot_offer()
}

fn snapshot_offer() -> bool {
    std::env::var_os("RAZDOR_SNAPSHOT").is_some() && std::env::var_os("RAZDOR_SCENE_IMPORT").is_some()
}

/// Each frame: sets the folder an import writes to (`install` is the current install's folder),
/// opens a requested picker, takes its answer and finishes a job. True when the install may have
/// changed (the caller reloads it).
pub fn tick(install: Option<&Path>) -> bool {
    let unpack = platform::unpack_dir();
    let target = import::target(install, unpack.as_deref());
    TARGET.with_borrow_mut(|t| *t = target);
    if REQUESTED.with(|r| r.replace(false)) && !busy() && offered() {
        start_picker();
    }
    let changed = finish_job();
    match poll_picker() {
        Poll::Nothing | Poll::Cancelled => {}
        #[cfg(target_os = "android")]
        Poll::Failed(why) => notice(vec![why]),
        Poll::Got(picked) => picked_archive(picked),
    }
    changed
}

/// The picked archive: examined (nothing is written), then either started, or asked about when it
/// replaces a game that is there.
fn picked_archive(picked: Picked) {
    let cleanup = picked.cached.then(|| picked.path.clone());
    let discard = |cleanup: &Option<PathBuf>| {
        if let Some(copy) = cleanup {
            let _ = std::fs::remove_file(copy);
        }
    };
    let Some(target) = TARGET.with_borrow(|t| t.clone()) else {
        discard(&cleanup);
        notice(vec![tr("Only the game in Razdor's own folder can be updated here.").to_string()]);
        return;
    };
    let found = match import::examine(&picked.path, &picked.name) {
        Ok(found) => found,
        Err(e) => {
            discard(&cleanup);
            notice(vec![error_text(&e)]);
            return;
        }
    };
    let installed = target.is_dir();
    if found.kind == Kind::Patch && !installed {
        discard(&cleanup);
        notice(vec![error_text(&Error::NoInstall)]);
        return;
    }
    let pending = Pending { found, target, cleanup };
    if pending.found.kind == Kind::Game && installed {
        MODAL.with_borrow_mut(|m| *m = Some(Modal::Confirm(pending)));
    } else {
        start(pending);
    }
}

/// Starts the import on a thread; its end is seen by [`finish_job`].
fn start(pending: Pending) {
    let name = pending.found.name.clone();
    let total = Arc::new(AtomicU64::new(pending.found.size));
    let done = Arc::new(AtomicU64::new(0));
    let (d, t) = (done.clone(), total.clone());
    let spawned = std::thread::Builder::new().name("game-import".into()).spawn(move || {
        let Pending { found, target, cleanup } = pending;
        let result = import::import(&found, &target, &|bytes, all| {
            d.store(bytes, Ordering::Relaxed);
            t.store(all, Ordering::Relaxed);
        });
        if let Some(copy) = cleanup {
            let _ = std::fs::remove_file(copy);
        }
        result
    });
    match spawned {
        Ok(handle) => JOB.with_borrow_mut(|j| *j = Some(Job { name, handle: Some(handle), done, total })),
        Err(e) => notice(vec![trf!("The import cannot start: {e}.", e = e)]),
    }
}

/// Each frame: when the import's thread has ended, its outcome is shown. Always asks for a reload
/// of the install (a failed one may have moved folders, and the reload is then the right state).
fn finish_job() -> bool {
    let ended = JOB.with_borrow(|j| j.as_ref().and_then(|j| j.handle.as_ref()).is_some_and(|h| h.is_finished()));
    if !ended {
        return false;
    }
    let Some(mut job) = JOB.with_borrow_mut(Option::take) else { return false };
    let text = match job.handle.take().map(|h| h.join()) {
        Some(Ok(Ok(Done::Replaced))) => trf!("The game is updated: «{name}». The saves stay.", name = job.name.clone()),
        Some(Ok(Ok(Done::Patched(n)))) => trf!("Written over the game: {n} files.", n = n),
        Some(Ok(Err(e))) => error_text(&e),
        _ => tr("The update stopped unexpectedly.").to_string(),
    };
    notice(vec![text]);
    true
}

/// A notice of this module (its title is [`title`]).
fn notice(lines: Vec<String>) {
    MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title: title().to_string(), lines }));
}

/// The words for a failed import, as the player reads them.
fn error_text(e: &Error) -> String {
    match e {
        Error::NotGame(name) => trf!("«{name}» holds neither the game nor a patch for it.", name = name.clone()),
        Error::Unreadable(why) => trf!("The archive cannot be read: {why}.", why = why.clone()),
        Error::TooBig(name) => trf!("«{name}» is too big for the game.", name = name.clone()),
        Error::NoInstall => tr("A patch goes over the game, and there is no game yet: import the whole game archive first.").to_string(),
        Error::NotInstall => tr("The archive does not hold a Discord Times install: the game is not changed.").to_string(),
        Error::Unpack(UnpackError::NoRoomOrAccess(dir)) => trf!("Could not unpack: no room or no access to the folder {dir}", dir = dir.display()),
        Error::Unpack(other) => trf!("Could not unpack: {error}", error = other.to_string()),
        Error::Io(why) => trf!("The update failed: {why}.", why = why.clone()),
    }
}

/// The window of this module, drawn over the settings (after them, so it is on top).
pub fn modal() {
    let modal = MODAL.with_borrow_mut(Option::take);
    match modal {
        Some(Modal::Confirm(pending)) => match confirm_window(&pending.found.name) {
            None => MODAL.with_borrow_mut(|m| *m = Some(Modal::Confirm(pending))),
            Some(true) => start(pending),
            Some(false) => {
                if let Some(copy) = &pending.cleanup {
                    let _ = std::fs::remove_file(copy);
                }
            }
        },
        Some(Modal::Notice { title, lines }) => {
            if !map_import::notice_window(&title, &lines) {
                MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title, lines }));
            }
        }
        None => progress_window(),
    }
}

/// The question before a whole game is replaced: Replace (`Some(true)`), Cancel (`Some(false)`),
/// or no answer yet.
fn confirm_window(name: &str) -> Option<bool> {
    let text = trf!("The game will be replaced with «{name}»: the game files now in the folder are removed. Your saves stay. Replace?", name = name.to_string());
    let (inner, closed) = map_import::frame(title(), &[text], true);
    match map_import::buttons(inner, &[tr("Replace"), tr("Cancel")]) {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if closed || is_key_pressed(KeyCode::Escape) => Some(false),
        None => None,
    }
}

/// The import's progress while its thread runs: the percent and a bar.
fn progress_window() {
    let Some((done, total)) = JOB.with_borrow(|j| j.as_ref().map(|j| (j.done.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed)))) else {
        return;
    };
    let pct = if total == 0 { 0 } else { (done * 100 / total).min(100) };
    let (inner, _) = map_import::frame(title(), &[trf!("Unpacking… {pct}%", pct = pct)], false);
    let k = chrome::k();
    let bar = Rect::new(inner.x + 24.0 * k, inner.y + 60.0 * k, inner.w - 48.0 * k, 14.0 * k);
    draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(0.0, 0.0, 0.0, 0.45));
    draw_rectangle(bar.x, bar.y, bar.w * pct as f32 / 100.0, bar.h, Color::new(0.85, 0.65, 0.25, 1.0));
    draw_rectangle_lines(bar.x, bar.y, bar.w, bar.h, 1.0, Color::new(0.0, 0.0, 0.0, 0.8));
}

/// The settings window's row (`main_menu::options_window`), `r` its full width: the button, or
/// the reason it is not offered, in its place.
pub fn settings_row(r: Rect, over_map: bool) {
    let k = chrome::k();
    let reason = if over_map {
        Some(tr("The game is updated from the main menu."))
    } else if cfg!(target_os = "ios") {
        Some(tr("The update from an archive is not on iPhone yet."))
    } else if !offered() {
        Some(tr("Only the game in Razdor's own folder can be updated here."))
    } else {
        None
    };
    if let Some(text) = reason {
        chrome::shadow_text(text, r.x + 4.0 * k, r.y + 19.0 * k, 13.0 * k, Color::new(0.93, 0.87, 0.75, 0.75));
        return;
    }
    let enabled = !busy();
    let (_, down, pressed) = button_state(r, enabled);
    chrome::marble_button(r, tr("Update the game from an archive…"), enabled, down);
    if pressed {
        cue(Cue::Button);
        request();
    }
}

/// The words of the picker's failure (Android) or of the cancel, for the picker's answer.
enum Poll {
    Nothing,
    Cancelled,
    /// The Android picker reported an error (its reason).
    #[cfg(target_os = "android")]
    Failed(String),
    Got(Picked),
}

/// Opens the platform's picker (the state is then `Desktop` or `Android`).
#[cfg(target_os = "ios")]
fn start_picker() {}

#[cfg(target_os = "android")]
fn start_picker() {
    let state = if super::android::start_pick() {
        Picker::Android
    } else {
        notice(vec![tr("The file picker cannot be opened on this phone.").to_string()]);
        Picker::Idle
    };
    PICKER.with_borrow_mut(|p| *p = state);
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn start_picker() {
    let (tx, rx) = std::sync::mpsc::channel();
    if cfg!(target_os = "macos") {
        // macOS: the dialog runs here, on the main thread, as its dialogs must.
        let _ = tx.send(desktop_dialog());
        PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
    } else {
        let spawned = std::thread::Builder::new().name("game-picker".into()).spawn(move || {
            let _ = tx.send(desktop_dialog());
        });
        match spawned {
            Ok(_) => PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx)),
            Err(e) => notice(vec![trf!("The file picker cannot be opened: {e}.", e = e)]),
        }
    }
}

/// The desktop's open-file dialog: a game archive (.zip or .7z), or any file.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Update the game from an archive…"))
        .add_filter(tr("Game archives"), &["zip", "7z"])
        .add_filter(tr("All files"), &["*"])
        .pick_file()
}

/// The picker's answer this frame, if it has one.
fn poll_picker() -> Poll {
    let state = PICKER.with_borrow_mut(std::mem::take);
    match state {
        Picker::Idle => Poll::Nothing,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        Picker::Desktop(rx) => match rx.try_recv() {
            Ok(None) => Poll::Cancelled,
            Ok(Some(path)) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Poll::Got(Picked { name, path, cached: false })
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
                Poll::Nothing
            }
            // The dialog's thread ended without an answer: taken as cancelled.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Poll::Cancelled,
        },
        #[cfg(target_os = "android")]
        Picker::Android => {
            let outcome = super::android::pick_outcome();
            match outcome.as_str() {
                "" => {
                    PICKER.with_borrow_mut(|p| *p = Picker::Android);
                    Poll::Nothing
                }
                "cancel" => Poll::Cancelled,
                text if text.starts_with("error: ") => Poll::Failed(text["error: ".len()..].to_string()),
                text => match text.strip_prefix("ok\n").and_then(|rest| rest.split_once('\n')) {
                    Some((name, path)) => Poll::Got(Picked { name: name.to_string(), path: PathBuf::from(path), cached: true }),
                    None => Poll::Failed(trf!("The picked file cannot be read: {why}.", why = text.to_string())),
                },
            }
        }
    }
}
