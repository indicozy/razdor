//! The holiday switch («Новогодний вид», `Settings::map_holiday`). Turned on: the Christmas mod (and the
//! look's music, bell and sleigh) is downloaded from the hub in the background, the game played is
//! remembered (`holiday_previous`) and the game switches to the Christmas mod. Turned off: the game goes
//! back to the one remembered. The season start (1 December to 15 January, once a season, `holiday_asked`)
//! turns the switch on and opens the welcome window. A failure (offline, the download) is logged; the
//! switch stays pending (`holiday_pending`) and is tried again on the next launch or the next turn-on.
//! The worker runs on a thread; the App takes its result each frame (`holiday_switch_tick`).

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

/// The switch's result: the mod's id and whether it was on the disk before (`false`: it was downloaded).
pub struct Start {
    pub id: String,
    pub was_installed: bool,
}

/// The holiday files are written by one thread at a time: the switch's worker and the launch's files check can
/// both run at a launch with the switch on, and their `.part` files of the same names would clash (a file's
/// download failed with «No such file or directory» on the race). The second one finds the files there and skips them.
static FILES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `Hub::install_holiday` under [`FILES_LOCK`].
fn install_files(hub: &Hub, h: &HolidayRef) -> Result<usize, String> {
    let _one_at_a_time = FILES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    hub.install_holiday(h)
}

/// The worker: the hub's catalog (fresh from the address, else the cached one when offline), the mod it
/// names, and its download and install when it is not on the disk. The whole catalog is read, so the
/// listing's `visible_from` does not hide the mod from the switch. Runs on a thread.
fn run(local: PathBuf) -> Result<Start, String> {
    let hub = Hub { source: Source::from_env(), local };
    let (catalog, fresh) = match hub.fetch_catalog() {
        Ok(c) => (c, true),
        Err(why) => (hub.cached_catalog().ok_or(why)?, false),
    };
    let id = catalog.holiday.as_ref().and_then(|h| h.mod_id.clone()).ok_or_else(|| "the hub names no Christmas mod".to_string())?;
    let was_installed = hub.installed().has(&id);
    if !was_installed {
        hub.install(&catalog, &id, lang())?;
    }
    // The bell, the music and the sleigh come with the switch. A failure is not fatal: the look's copy
    // (`files_tick`) tries them again on the next launch. Offline (the cached catalog) they wait.
    if let (true, Some(h)) = (fresh, &catalog.holiday) {
        if let Err(why) = install_files(&hub, h) {
            razdor::diag!("holiday: the files are not all there ({why}); they are tried again on the next launch");
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
        Files::Waiting if !holiday::setting() || start_due => Files::Waiting,
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
        let _ = tx.send(install_files(&hub, &h));
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
    /// Each frame from the main menu: takes the switch's worker result when it lands; turns the switch on at
    /// the season start (once a season); takes the switch's edges (on, off); and starts the worker when the
    /// switch is pending (on the turn-on, at a launch that left it pending, or with `RAZDOR_HOLIDAY=1`). The
    /// worker runs once per launch or turn-on (`holiday_tried`): a failure waits for the next one.
    pub(super) fn holiday_switch_tick(&mut self) {
        // The live reload hook: the game played or the look changed (`holiday::game_generation`). Nothing else
        // needs doing here: a mod switch (`apply_mod`) already replaces the menu's pictures (`Assets::adopt_install`
        // resets the chrome textures), the audio follows the install and restarts the menu's music when the look
        // changed (`Audio::tick`), and the snow reads `holiday::on` each frame. The log shows the change.
        let generation = holiday::game_generation();
        if generation != self.holiday_generation {
            self.holiday_generation = generation;
            razdor::diag!("holiday: the game or the look changed (generation {generation}, on {})", holiday::on());
        }
        self.holiday_switch_check();
        if let Some(job) = self.holiday_job.as_ref() {
            let Some(done) = finished(job) else { return };
            self.holiday_job = None;
            START_RUNNING.store(false, Ordering::Relaxed);
            LANDED.store(true, Ordering::Relaxed);
            match done {
                Ok(Ok(start)) => self.holiday_started(start),
                Ok(Err(why)) => razdor::diag!("holiday: the Christmas mod is not ready ({why}); the switch waits for the next launch or turn-on"),
                Err(()) => razdor::diag!("holiday: the worker ended without a result; the switch waits for the next launch or turn-on"),
            }
            return;
        }
        let today = holiday::today();
        if holiday::start_due(&self.audio.settings, today) {
            // The season start is the switch's turn-on, asked once per season; its welcome opens on success.
            if let Some(year) = holiday::season(today.0, today.1, today.2) {
                self.audio.settings.map_holiday = true;
                self.audio.settings.holiday_asked = Some(year);
                self.audio.settings.holiday_pending = true;
                self.holiday_greet = true;
                self.audio.settings.save();
            }
        }
        let setting = holiday::wanted(self.audio.settings.map_holiday);
        match holiday::edge(self.holiday_seen, setting) {
            Some(true) => {
                self.audio.settings.holiday_pending = true;
                self.holiday_tried = false;
                self.audio.settings.save();
                razdor::diag!("holiday: the look is switched on");
            }
            Some(false) => self.holiday_switched_off(),
            None => {}
        }
        self.holiday_seen = setting;
        let pending = self.audio.settings.holiday_pending || holiday::env_forced();
        if !setting || !pending || self.holiday_tried {
            return;
        }
        if self.mod_switch.is_some() || super::saves_move::busy() || super::game_import::busy() {
            return;
        }
        let Some(local) = razdor::hub::local_dir() else { return };
        self.holiday_tried = true;
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("holiday-switch".into()).spawn(move || {
            let _ = tx.send(run(local));
        });
        if spawned.is_ok() {
            START_RUNNING.store(true, Ordering::Relaxed);
            self.holiday_job = Some(rx);
        }
    }

    /// The switch-on worker is done: when the switch is still on and pending, the game switches to the
    /// Christmas mod, remembering the game played before (`holiday_previous`); the welcome opens after a
    /// season start. A switch turned off while the worker ran switches nothing.
    fn holiday_started(&mut self, start: Start) {
        self.refresh_mods();
        if !start.was_installed {
            razdor::diag!("holiday: the Christmas mod {} is downloaded", start.id);
        }
        let wanted = holiday::wanted(self.audio.settings.map_holiday);
        let pending = self.audio.settings.holiday_pending || holiday::env_forced();
        if !wanted || !pending {
            razdor::diag!("holiday: the switch is off since the download; the game stays as it is");
            return;
        }
        let christmas_played = self.active_mod.as_deref() == Some(start.id.as_str());
        let steps = holiday::on_steps(true, christmas_played);
        if steps.switch {
            // The switch stays pending until the game plays the mod (`holiday_switch_check`); a choice made
            // while another switch runs waits behind it (`choose_mod`).
            self.audio.settings.holiday_previous = self.active_mod.clone();
            self.holiday_awaiting = Some(start.id.clone());
            self.choose_mod(Some(start.id.clone()));
        } else {
            self.audio.settings.holiday_pending = false;
        }
        self.audio.settings.save();
        razdor::diag!("holiday: the look is on (mod {}, switched {})", start.id, steps.switch);
        if std::mem::take(&mut self.holiday_greet) {
            holiday_windows::open_greeting();
        }
    }

    /// The switch to the Christmas mod (`holiday_awaiting`) is checked each frame: done when the game plays the
    /// mod (the switch is no longer pending); missed when no mod switch runs or waits and it is not played (the
    /// switch stays pending for the next launch or turn-on, as a failed download does).
    fn holiday_switch_check(&mut self) {
        let Some(id) = self.holiday_awaiting.clone() else { return };
        let busy = self.mod_switch.is_some() || self.mod_queued.is_some();
        match holiday::awaited(&id, self.active_mod.as_deref(), busy) {
            holiday::Awaited::Waiting => {}
            holiday::Awaited::Done => {
                self.holiday_awaiting = None;
                self.audio.settings.holiday_pending = false;
                self.audio.settings.save();
                razdor::diag!("holiday: the game plays the Christmas mod {id}; the switch is done");
            }
            holiday::Awaited::Missed => {
                self.holiday_awaiting = None;
                razdor::diag!("holiday: the game did not switch to the Christmas mod {id}; the switch stays pending");
            }
        }
    }

    /// The switch is turned off: nothing is pending any more. When the Christmas mod is the game played, the
    /// game goes back to the one remembered (None: the base game). Otherwise the game stays as it is.
    fn holiday_switched_off(&mut self) {
        self.audio.settings.holiday_pending = false;
        let previous = self.audio.settings.holiday_previous.take();
        self.audio.settings.save();
        // A switch to the Christmas mod still owed is cancelled: a queued one is dropped, and one that runs goes
        // back to the game remembered once it lands.
        if let Some(id) = self.holiday_awaiting.take() {
            if self.mod_queued.as_ref() == Some(&Some(id.clone())) {
                self.mod_queued = None;
            }
            if self.mod_switch.as_ref().is_some_and(|s| s.id.as_deref() == Some(id.as_str())) {
                self.mod_queued = Some(previous.clone());
                razdor::diag!("holiday: the look is off while the Christmas mod loads; the game goes back to {} when it lands", previous.as_deref().unwrap_or("the base game"));
                return;
            }
        }
        if let Some(target) = holiday::off_target(holiday::mod_running(), previous) {
            razdor::diag!("holiday: the look is off; the game goes back to {}", target.as_deref().unwrap_or("the base game"));
            self.choose_mod(target);
        }
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
