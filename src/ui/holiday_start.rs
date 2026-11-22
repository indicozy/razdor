//! The season start («Новогодний вид», `holiday`): on the first main-menu frame of a season (from 1
//! December to 15 January), the Christmas mod is downloaded from the hub (when it is not there),
//! played (as the new-game window's «Мод» does), the look is turned on and the welcome window opens.
//! Once per season (`holiday_asked` names the season year). A failure (offline, the download) is
//! logged and the start is tried again on the next start. The menu only: nothing is loaded here.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use razdor::hub::{HolidayRef, Hub, Source};
use razdor::i18n::lang;
use razdor::local;

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
            Ok(Ok(0)) => Files::Done,
            Ok(Ok(n)) => {
                razdor::diag!("holiday: {n} file(s) of the look are updated from the catalog");
                Files::Done
            }
            Ok(Err(why)) => {
                razdor::diag!("holiday: the files are not all there ({why}); they are tried again on the next launch");
                Files::Done
            }
            Err(TryRecvError::Disconnected) => Files::Done,
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
