//! The season start («Новогодний вид», `holiday`): on the first main-menu frame of a season (from 1
//! December to 15 January), the Christmas mod is downloaded from the hub (when it is not there),
//! played (as the new-game window's «Мод» does), the look is turned on and the welcome window opens.
//! Once per season (`holiday_asked` names the season year). A failure (offline, the download) is
//! logged and the start is tried again on the next start. The menu only: nothing is loaded here.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use razdor::hub::{HolidayRef, Hub, Source};
use razdor::i18n::lang;
use razdor::local;

/// The season start's worker is running (the main menu's loader shows «Новогодние файлы…» for it).
static START_RUNNING: AtomicBool = AtomicBool::new(false);
/// A holiday file check (`files_tick`) or the season start has finished since the audio last looked:
/// the menu's music may now have its holiday file (`take_landed`).
static LANDED: AtomicBool = AtomicBool::new(false);

/// The loader's text while a background job runs, from the three jobs' states (pure): the season
/// start or the holiday files (the Christmas texts), else the hub's catalog fetch. None when none runs.
/// The keys are English; `tr` gives the language's text.
pub fn loading_label(season_start: bool, files: bool, catalog: bool) -> Option<&'static str> {
    if season_start || files {
        Some("New Year's files…")
    } else if catalog {
        Some("Loading…")
    } else {
        None
    }
}

/// The loader's text this frame (`loading_label` of the live jobs). In a snapshot the catalog and the
/// holiday jobs never run, so `RAZDOR_LOADER=holiday|catalog` shows one of them for the screenshots.
pub fn loader_label() -> Option<&'static str> {
    if super::clock::is_snapshot() {
        return match std::env::var("RAZDOR_LOADER").ok().as_deref() {
            Some("holiday") => loading_label(true, false, false),
            Some("catalog") => loading_label(false, false, true),
            _ => None,
        };
    }
    loading_label(START_RUNNING.load(Ordering::Relaxed), files_busy(), super::launch::fetching())
}

/// Whether the holiday files are being checked and downloaded right now.
fn files_busy() -> bool {
    FILES.with(|s| matches!(*s.borrow(), Files::Installing(_)))
}

/// Takes the one-shot «holiday files may have landed» flag: the audio asks it each frame and, when the
/// menu's music plays without its holiday file, starts that music again from the file.
pub fn take_landed() -> bool {
    LANDED.swap(false, Ordering::Relaxed)
}

use super::{finished, holiday, holiday_windows, launch, App};

/// The start's result: the mod's id and whether it was on the disk before (`false`: it was downloaded).
pub struct Start {
    pub id: String,
    pub was_installed: bool,
}

/// The worker: the hub's catalog (fresh, from the address), the mod it names, and its download and
/// install when it is not on the disk. Runs on a thread.
fn run(local: PathBuf) -> Result<Start, String> {
    let hub = Hub { source: Source::from_env(), local };
    let catalog = hub.fetch_catalog()?;
    let id = catalog.holiday.as_ref().and_then(|h| h.mod_id.clone()).ok_or_else(|| "the hub names no Christmas mod".to_string())?;
    let was_installed = hub.installed().has(&id);
    if !was_installed {
        hub.install(&catalog, &id, lang())?;
    }
    // The bell and the sleigh come with the season start. A failure is not fatal: the look's copy
    // (`files_tick`) tries them again on the next start.
    if let Some(h) = &catalog.holiday {
        if let Err(why) = hub.install_holiday(h) {
            razdor::diag!("holiday: the files are not all there ({why}); they are tried again on the next start");
        }
    }
    Ok(Start { id, was_installed })
}

/// The holiday files' copy for this launch (see [`files_tick`]).
enum Files {
    /// The launch's catalog has not come yet, or the season start is still due (it brings the files).
    Waiting,
    /// The files are being checked and, when they differ, downloaded (`Hub::install_holiday`).
    Installing(Receiver<Result<usize, String>>),
    /// Decided for this launch.
    Done,
}

local! {
    static FILES: std::cell::RefCell<Files> = const { std::cell::RefCell::new(Files::Waiting) };
}

/// Each frame: while the holiday look is on and the season start is not due, the launch's catalog's
/// holiday files (the music, the bell, the sleigh) are kept in the hub folder, on a thread. Files that
/// are already the catalog's are not downloaded again. A failure is logged and tried on the next launch.
pub fn files_tick(start_due: bool) {
    let state = FILES.with(|s| std::mem::replace(&mut *s.borrow_mut(), Files::Done));
    let next = match state {
        Files::Done => Files::Done,
        Files::Waiting if !holiday::on() || start_due => Files::Waiting,
        Files::Waiting => match launch::catalog() {
            None => Files::Waiting,
            Some(Err(why)) => {
                razdor::diag!("holiday: no catalog this launch: {why}");
                Files::Done
            }
            Some(Ok(catalog)) => match catalog.holiday.clone() {
                Some(h) => start_files(h),
                None => Files::Done,
            },
        },
        Files::Installing(rx) => match rx.try_recv() {
            Err(TryRecvError::Empty) => Files::Installing(rx),
            // The check has ended (even part-way: a file may have landed): the audio may pick it up.
            Ok(Ok(0)) => {
                LANDED.store(true, Ordering::Relaxed);
                Files::Done
            }
            Ok(Ok(n)) => {
                razdor::diag!("holiday: {n} file(s) of the look are updated from the catalog");
                LANDED.store(true, Ordering::Relaxed);
                Files::Done
            }
            Ok(Err(why)) => {
                razdor::diag!("holiday: the files are not all there ({why}); they are tried again on the next launch");
                LANDED.store(true, Ordering::Relaxed);
                Files::Done
            }
            Err(TryRecvError::Disconnected) => {
                LANDED.store(true, Ordering::Relaxed);
                Files::Done
            }
        },
    };
    FILES.with(|s| *s.borrow_mut() = next);
}

/// Starts the holiday files' check on a thread (the hub's address and the hub folder), or Done when there is
/// no folder for the hub.
fn start_files(h: HolidayRef) -> Files {
    let Some(local) = razdor::hub::local_dir() else { return Files::Done };
    let hub = Hub { source: Source::from_env(), local };
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("holiday-files".into()).spawn(move || {
        let _ = tx.send(hub.install_holiday(&h));
    });
    match spawned {
        Ok(_) => Files::Installing(rx),
        Err(e) => {
            razdor::diag!("holiday: the files cannot start: {e}");
            Files::Done
        }
    }
}

impl App {
    /// Runs the season start when it is due, once per session (each frame, from the main menu): starts
    /// the worker, and takes its result when it lands.
    pub(super) fn holiday_start_tick(&mut self) {
        if let Some(job) = self.holiday_job.as_ref() {
            let Some(done) = finished(job) else { return };
            self.holiday_job = None;
            START_RUNNING.store(false, Ordering::Relaxed);
            LANDED.store(true, Ordering::Relaxed);
            match done {
                Ok(Ok(start)) => self.holiday_started(start),
                Ok(Err(why)) => razdor::diag!("holiday: the season start is not done ({why}); it is tried again on the next start"),
                Err(()) => razdor::diag!("holiday: the season start's worker ended without a result; it is tried again on the next start"),
            }
            return;
        }
        if self.holiday_tried || self.mod_switch.is_some() || super::saves_move::busy() || super::game_import::busy() {
            return;
        }
        if !holiday::start_due(&self.audio.settings, holiday::today()) {
            return;
        }
        let Some(local) = razdor::hub::local_dir() else { return };
        self.holiday_tried = true;
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("holiday-start".into()).spawn(move || {
            let _ = tx.send(run(local));
        });
        if spawned.is_ok() {
            START_RUNNING.store(true, Ordering::Relaxed);
            self.holiday_job = Some(rx);
        }
    }

    /// The start is done: the look is on, the season is answered, the mod is played (if it is not
    /// yet), and the welcome window opens.
    fn holiday_started(&mut self, start: Start) {
        let today = holiday::today();
        let Some(year) = holiday::season(today.0, today.1, today.2) else { return };
        self.refresh_mods();
        let active = self.active_mod.as_deref() == Some(start.id.as_str());
        let steps = holiday::steps(true, active, self.audio.settings.map_holiday);
        if !start.was_installed {
            razdor::diag!("holiday: the Christmas mod {} is downloaded", start.id);
        }
        self.audio.settings.map_holiday = true;
        self.audio.settings.holiday_asked = Some(year);
        if steps.switch {
            self.choose_mod(Some(start.id.clone()));
        }
        razdor::diag!("holiday: the season start is done (mod {}, switch {})", start.id, steps.switch);
        holiday_windows::open_greeting();
    }
}

/// Whether the menu's music must start again from the holiday file (pure): the holiday files have just
/// landed (`take_landed`), the file is there now, and the music that plays was not read from it.
pub fn pickup_due(landed: bool, file_now: bool, file_played: bool) -> bool {
    landed && file_now && !file_played
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loader_shows_the_holiday_text_first_then_the_catalog_fetch() {
        assert_eq!(loading_label(true, true, true), Some("New Year's files…"));
        assert_eq!(loading_label(false, true, true), Some("New Year's files…"));
        assert_eq!(loading_label(true, false, false), Some("New Year's files…"), "the season start alone");
        assert_eq!(loading_label(false, false, true), Some("Loading…"), "the catalog alone");
        assert_eq!(loading_label(false, false, false), None, "nothing runs: no loader");
    }

    #[test]
    fn the_menu_music_is_picked_up_only_when_a_file_landed_and_the_music_lacks_it() {
        assert!(pickup_due(true, true, false), "the file landed and the install's track plays");
        assert!(!pickup_due(false, true, false), "no landing this frame");
        assert!(!pickup_due(true, false, false), "still no file");
        assert!(!pickup_due(true, true, true), "the holiday track already plays");
    }
}
