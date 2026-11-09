//! The move of the player's files into the app's own folder on Android (`migration`, release N).
//! It starts on the first frame when the files are still in the shared storage, runs on a thread,
//! shows its percent on the title screen (where the update's notice is), and holds a game's start
//! and its load until it is done. The paths flip when it ends ([`tick`]). Desktop and iOS never
//! start it, so [`busy`] is false there.

use super::chrome;
use super::map_import;
use super::widgets::text_centered;
use macroquad::window::screen_width;
use razdor::i18n::tr;
use razdor::local;
use razdor::migration::{self, Failure, Moved, Plan};
use razdor::trf;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// What the App does when the move ends (see [`tick`]).
pub enum Outcome {
    /// The paths are the app's own now (on Android); `game_skipped`: the game stayed where it was,
    /// `game_in_app`: because the app's folder has a game already.
    Moved { game_skipped: bool, game_in_app: bool },
    /// Not enough room for the Razdor folder: nothing moved, the next start tries again.
    NoRoom { needed: u64, free: u64 },
    /// The move failed (its reason is logged); the next start tries again.
    Failed(String),
}

enum State {
    Idle,
    Running { handle: Option<JoinHandle<Result<Moved, Failure>>>, done: Arc<AtomicU64>, total: Arc<AtomicU64> },
    /// Ended (its outcome was handed out), or never started.
    Finished,
}

local! {
    static MOVE: RefCell<State> = const { RefCell::new(State::Idle) };
}

/// The app's folder the files are moved into, while they are still in the shared storage
/// (Android). None elsewhere, and once the paths are the app's own.
#[cfg(target_os = "android")]
fn files_to_move() -> Option<PathBuf> {
    if razdor::platform::android_migrated() {
        return None;
    }
    razdor::platform::android_files_dir().map(PathBuf::from)
}

/// Nothing is moved off Android: the paths are the app's own there.
#[cfg(not(target_os = "android"))]
fn files_to_move() -> Option<PathBuf> {
    None
}

/// Starts the move once, when the files are still in the shared storage (see [`files_to_move`]).
pub fn start_if_needed() {
    let Some(files) = files_to_move() else { return };
    MOVE.with_borrow_mut(|state| {
        if !matches!(state, State::Idle) {
            return;
        }
        let done = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(0));
        let (d, t) = (done.clone(), total.clone());
        let spawned = std::thread::Builder::new().name("move-files".into()).spawn(move || {
            let plan = Plan::android(&files);
            let progress = |bytes: u64, all: u64| {
                d.store(bytes, Ordering::Relaxed);
                t.store(all, Ordering::Relaxed);
            };
            migration::migrate(
                &plan,
                &|dir| razdor::dt::game_import::free_space(dir),
                &mut |from, to, len| migration::copy_file(from, to, len),
                &mut |from, to| std::fs::rename(from, to),
                &progress,
            )
        });
        *state = match spawned {
            Ok(handle) => State::Running { handle: Some(handle), done, total },
            Err(e) => {
                razdor::diag::log(&format!("move: the thread did not start: {e}"));
                State::Finished
            }
        };
    });
}

/// Each frame: when the thread has ended, its outcome, once. On success the paths flip here
/// (`platform::set_android_migrated`, after the marker is written by the move itself).
pub fn tick() -> Option<Outcome> {
    let ended = MOVE.with_borrow_mut(|state| {
        let finished = matches!(state, State::Running { handle: Some(h), .. } if h.is_finished());
        if !finished {
            return None;
        }
        match std::mem::replace(state, State::Finished) {
            State::Running { handle: Some(h), .. } => Some(h.join()),
            _ => None,
        }
    });
    Some(match ended? {
        Ok(Ok(moved)) => {
            #[cfg(target_os = "android")]
            {
                razdor::platform::set_android_migrated();
                // The housekeeping of the app's folder (`.nomedia`, the old cache) runs on the new paths.
                std::thread::spawn(razdor::platform::tidy_app_dir);
            }
            Outcome::Moved { game_skipped: moved.game_skipped, game_in_app: moved.game_in_app }
        }
        Ok(Err(Failure::NoRoom { needed, free })) => Outcome::NoRoom { needed, free },
        Ok(Err(Failure::Failed(why))) => Outcome::Failed(why),
        Err(_) => Outcome::Failed("the move stopped unexpectedly".into()),
    })
}

/// True while the files are being moved. A game is not started or loaded then.
pub fn busy() -> bool {
    MOVE.with_borrow(|state| matches!(state, State::Running { .. }))
}

/// The move's progress in percent, while it runs.
fn percent() -> Option<u64> {
    MOVE.with_borrow(|state| match state {
        State::Running { done, total, .. } => {
            let all = total.load(Ordering::Relaxed);
            Some(if all == 0 { 0 } else { done.load(Ordering::Relaxed) * 100 / all })
        }
        _ => None,
    })
}

/// The title screen's line while the files are moved: «Перенос файлов… N%», on the update
/// notice's line (`update_view::title_line`).
pub fn title_line() {
    let Some(pct) = percent() else { return };
    let k = chrome::k();
    let line = trf!("Moving files… {pct}%", pct = pct);
    text_centered(&line, screen_width() / 2.0, super::main_menu::notice_y(k), 16.0 * k, chrome::GOLD);
}

/// A game's start or load pressed while the files are moved: the press is not taken, and the
/// player is told why (the title's line shows the progress).
pub fn refuse() {
    notify(vec![tr("The files are being moved: wait until it is done.").to_string()]);
}

/// Shows `lines` in the move's notice window.
pub fn notify(lines: Vec<String>) {
    map_import::notice(tr("Moving the files"), lines);
}

/// The notice after a move: the files are the app's own; with the game left behind, the archive
/// is what the player picks when the legacy folder can no longer be read.
pub fn moved_message(game_skipped: bool, game_in_app: bool) -> Vec<String> {
    let mut lines = vec![tr("Razdor's files were moved to the app's folder. The old Razdor and DiscordTimes folders can be deleted.").to_string()];
    if game_in_app {
        lines.push(tr("The game was not moved: the app's folder has a game already, and it is kept. The game in the old DiscordTimes folder stays there.").to_string());
    } else if game_skipped {
        lines.push(tr("The game folder was not moved: there was not enough room. If the game is not found, pick its archive.").to_string());
    }
    lines
}

/// The notice when the Razdor folder does not fit (the sizes in MB, rounded up and down).
pub fn no_room_message(needed: u64, free: u64) -> Vec<String> {
    vec![trf!("Not enough room to move the files: {need} MB needed, {free} MB free.", need = needed.div_ceil(1 << 20), free = free / (1 << 20))]
}
