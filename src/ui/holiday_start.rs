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
        Some(razdor::i18n::n_("New Year's files…"))
    } else if catalog {
        Some(razdor::i18n::n_("Loading…"))
    } else {
        None
    }
}

/// The loader's text this frame (`loading_label` of the live jobs). In a snapshot the catalog and the
/// holiday jobs never run, so `RAZDOR_LOADER=holiday|catalog` shows one of them for the screenshots.
pub fn loader_label() -> Option<&'static str> {
    // The switch waits for the game (`game_ready`): while the game downloads, the loader says so.
    if !super::clock::is_snapshot() && holiday::setting() && super::game_import::busy() {
        return Some("Loading the game");
    }
    if super::clock::is_snapshot() {
        return match std::env::var("RAZDOR_LOADER").ok().as_deref() {
            Some("holiday") => loading_label(true, false, false),
            Some("catalog") => loading_label(false, false, true),
            _ => None,
        };
    }
    loading_label(START_RUNNING.load(Ordering::Relaxed), files_busy(), super::launch::fetching()).or_else(super::en_pack::loader_label)
}

/// Whether the holiday files are being checked and downloaded right now.
pub fn files_busy() -> bool {
    FILES.with(|s| matches!(*s.borrow(), Files::Installing(_)))
}

/// Takes the one-shot «holiday files may have landed» flag: the audio asks it each frame and, when the
/// menu's music plays without its holiday file, starts that music again from the file.
pub fn take_landed() -> bool {
    LANDED.swap(false, Ordering::Relaxed)
}

use super::{finished, holiday, holiday_windows, launch, App};

/// The switch's result: the mod's id, whether it was downloaded (not on the disk before), whether an
/// installed copy was downloaded again because its files differ from the catalog's (`hub::update_available`),
/// and whether that check had to wait: only the cached catalog was there (`stale`), so it runs again when the
/// launch's fresh catalog comes (`rerun_due`).
pub struct Start {
    pub id: String,
    pub downloaded: bool,
    pub updated: bool,
    pub stale: bool,
}

/// What the worker's update check does with the catalog it has (pure): an installed Christmas mod that is
/// pending or played is checked against the launch's fresh catalog; with only the cached one the check waits.
#[derive(Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    /// Compare the installed files with the fresh catalog now.
    Now,
    /// Only the cached catalog is there: the check is due and runs once the fresh catalog comes.
    WaitForFresh,
    /// Not due: the mod is not installed, or it is neither pending nor played.
    NotDue,
}

pub fn update_check(installed: bool, fresh: bool, pending: bool, played_is_mod: bool) -> UpdateCheck {
    if !installed || !(pending || played_is_mod) {
        UpdateCheck::NotDue
    } else if fresh {
        UpdateCheck::Now
    } else {
        UpdateCheck::WaitForFresh
    }
}

/// Whether a waiting check runs now (pure): it waited for the cached catalog and the fresh one has come.
pub fn rerun_due(stale: bool, fresh_now: bool) -> bool {
    stale && fresh_now
}

/// A catalog's date for the log: the unix time it was read or saved, `?` when unknown.
fn dated(at: Option<std::time::SystemTime>) -> String {
    at.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or("?".to_string(), |d| format!("unix {}", d.as_secs()))
}

/// Whether the worker runs this launch (pure): the holiday switch is pending (a turn-on or a launch that left
/// it pending), or a mod is played, which may be the Christmas mod whose files a new catalog changed.
pub fn check_due(pending: bool, mod_played: bool) -> bool {
    pending || mod_played
}

/// Whether the holiday steps (the catalog, the mod, the files, the switch) may run this frame (pure). The
/// game comes first (the owner's rule, 2026-11-22): the install is loaded and no game download or import
/// runs. A failed download leaves no loaded install, so the steps wait for the next successful load.
pub fn game_ready(game_loaded: bool, game_busy: bool) -> bool {
    game_loaded && !game_busy
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

/// The worker: the launch's fresh catalog (`launch::fresh`, read by the launch's one fetch and its retries),
/// else the cached one, and the mod it names. It downloads the mod when it is not on the disk (only while the
/// switch is pending). An installed copy whose files differ from the catalog's (`hub::update_available`, by
/// SHA-256) is downloaded again through the Hub's install, when the switch is pending or the mod is the one
/// played (`played`). With the cached catalog only, the check is left for `Start::stale`. The whole catalog is
/// read, so the listing's `visible_from` does not hide the mod from the switch. Runs on a thread.
fn run(local: PathBuf, pending: bool, played: Option<String>) -> Result<Start, String> {
    let hub = Hub { source: Source::from_env(), local };
    let (catalog, fresh_at) = match launch::fresh() {
        Some((c, at)) => (c.clone(), Some(at)),
        None => (hub.cached_catalog().ok_or("the hub cannot be read and no catalog is cached")?, None),
    };
    let id = catalog.holiday.as_ref().and_then(|h| h.mod_id.clone()).ok_or_else(|| "the hub names no Christmas mod".to_string())?;
    let installed = hub.installed().has(&id);
    if !installed && !pending {
        return Ok(Start { id, downloaded: false, updated: false, stale: false });
    }
    if !installed {
        hub.install(&catalog, &id, lang())?;
    }
    let played_is_mod = played.as_deref() == Some(id.as_str());
    let mut updated = false;
    let mut stale = false;
    match update_check(installed, fresh_at.is_some(), pending, played_is_mod) {
        UpdateCheck::NotDue => {}
        UpdateCheck::WaitForFresh => {
            stale = true;
            razdor::diag!("holiday: the update check of the Christmas mod {id} waits: only the cached catalog (saved {}) is here", dated(hub.cache_time()));
        }
        UpdateCheck::Now => {
            razdor::diag!("holiday: the update check of the Christmas mod {id} uses the fresh catalog (fetched {})", dated(fresh_at));
            let code = match lang() {
                razdor::i18n::Lang::En => "en",
                razdor::i18n::Lang::Ru => "ru",
            };
            if razdor::hub::update_available(&hub.local, &catalog, &hub.installed(), &id, code) {
                match hub.install(&catalog, &id, lang()) {
                    Ok(_) => {
                        updated = true;
                        razdor::diag!("holiday: the Christmas mod {id} differs from the catalog; it is downloaded again");
                    }
                    Err(why) => razdor::diag!("holiday: the Christmas mod {id} could not be updated ({why}); the installed copy stays"),
                }
            } else {
                razdor::diag!("holiday: the Christmas mod {id} has the catalog's files; nothing is downloaded");
            }
        }
    }
    // The bell, the music and the sleigh come with the switch. A failure is not fatal: the look's copy
    // (`files_tick`) tries them again on the next launch. Offline (the cached catalog) they wait.
    if let (Some(_), true, Some(h)) = (fresh_at, pending, &catalog.holiday) {
        if let Err(why) = install_files(&hub, h) {
            razdor::diag!("holiday: the files are not all there ({why}); they are tried again on the next launch");
        }
    }
    Ok(Start { id, downloaded: !installed, updated, stale })
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
pub fn files_tick(start_due: bool, game_ready: bool) {
    let state = FILES.with(|s| std::mem::replace(&mut *s.borrow_mut(), Files::Done));
    let next = match state {
        Files::Done => Files::Done,
        Files::Waiting if !game_ready || !holiday::setting() || start_due || super::en_pack::busy() => Files::Waiting,
        Files::Waiting => match launch::fresh() {
            Some((catalog, _)) => match catalog.holiday.clone() {
                Some(h) => start_files(h),
                None => Files::Done,
            },
            // No fresh catalog yet: the launch's fetch or a retry may still bring one.
            None if !launch::settled() => Files::Waiting,
            None => {
                razdor::diag!("holiday: no catalog this launch; the look's files are tried on the next launch");
                Files::Done
            }
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
                Ok(Ok(start)) => {
                self.holiday_stale = start.stale;
                self.holiday_started(start);
            }
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
        let played = self.active_mod.clone();
        // A check that waited for the cached catalog runs again once the launch's fresh catalog has come.
        if rerun_due(self.holiday_stale, launch::fresh().is_some()) {
            self.holiday_stale = false;
            self.holiday_tried = false;
            razdor::diag!("holiday: the launch's fresh catalog came; the update check runs again");
        }
        if !setting || !check_due(pending, played.is_some()) || self.holiday_tried {
            return;
        }
        // The game first: the steps wait for the install to be loaded (a first launch's download, `game_import`).
        if !game_ready(self.assets.dt.is_some(), super::game_import::busy()) {
            return;
        }
        // The launch's one catalog fetch is still running: the check reads its result, not the cache.
        if launch::fetching() {
            return;
        }
        if self.mod_switch.is_some() || super::saves_move::busy() || super::game_import::busy() || super::en_pack::busy() {
            return;
        }
        let Some(local) = razdor::hub::local_dir() else { return };
        self.holiday_tried = true;
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("holiday-switch".into()).spawn(move || {
            let _ = tx.send(run(local, pending, played));
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
        if start.downloaded {
            razdor::diag!("holiday: the Christmas mod {} is downloaded", start.id);
        }
        let wanted = holiday::wanted(self.audio.settings.map_holiday);
        let pending = self.audio.settings.holiday_pending || holiday::env_forced();
        let christmas_played = self.active_mod.as_deref() == Some(start.id.as_str());
        // The played mod was downloaded again: its game folder is built from the new files and the install
        // adopted (`apply_mod`), so the new menu pictures show without a restart.
        if start.updated && wanted && christmas_played {
            razdor::diag!("holiday: the game plays the updated Christmas mod {}; it is loaded again", start.id);
            self.choose_mod(Some(start.id.clone()));
        }
        if !wanted || !pending {
            if pending {
                razdor::diag!("holiday: the switch is off since the download; the game stays as it is");
            }
            return;
        }
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
    fn the_worker_runs_while_pending_or_while_a_mod_is_played() {
        assert!(check_due(true, false), "a pending switch");
        assert!(check_due(false, true), "a mod played: its files may have changed in the catalog");
        assert!(!check_due(false, false), "nothing pending and the base game played: no catalog fetch");
    }

    #[test]
    fn the_holiday_steps_wait_for_the_game() {
        assert!(!game_ready(false, false), "no game install (a first launch, or a failed download): wait");
        assert!(!game_ready(true, true), "the game download or import still runs: wait");
        assert!(game_ready(true, false), "the game install is loaded and nothing downloads: proceed");
    }

    #[test]
    fn the_menu_music_is_picked_up_only_when_a_file_landed_and_the_music_lacks_it() {
        assert!(pickup_due(true, true, false), "the file landed and the install's track plays");
        assert!(!pickup_due(false, true, false), "no landing this frame");
        assert!(!pickup_due(true, false, false), "still no file");
        assert!(!pickup_due(true, true, true), "the holiday track already plays");
    }

    #[test]
    fn the_update_check_uses_the_fresh_catalog_or_waits_for_it_never_the_cache_alone() {
        assert_eq!(update_check(true, true, true, false), UpdateCheck::Now, "installed, pending, fresh catalog");
        assert_eq!(update_check(true, true, false, true), UpdateCheck::Now, "installed and played, fresh catalog");
        assert_eq!(update_check(true, false, true, false), UpdateCheck::WaitForFresh, "only the cached catalog: wait for the fresh one");
        assert_eq!(update_check(true, false, false, true), UpdateCheck::WaitForFresh, "played, cached catalog only");
        assert_eq!(update_check(false, true, true, false), UpdateCheck::NotDue, "not installed: nothing to compare");
        assert_eq!(update_check(true, true, false, false), UpdateCheck::NotDue, "neither pending nor played");
    }

    #[test]
    fn a_waiting_check_runs_again_only_once_the_fresh_catalog_has_come() {
        assert!(rerun_due(true, true), "it waited and the fresh catalog came");
        assert!(!rerun_due(true, false), "it waits: no fresh catalog yet");
        assert!(!rerun_due(false, true), "nothing waited: no second check");
    }
}
