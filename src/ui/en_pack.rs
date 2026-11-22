//! The «English maps» pack (`EN_PACK_ID`, the hub's `vanilla-en`): the original's maps machine-translated
//! into English. When the interface language is English, the main menu installs it by itself, in the
//! background, from the launch's catalog (`launch::fresh`, never a fetch of its own). It runs once per
//! launch, and again on each switch to English (`en_tried`). It waits for the game (`holiday_start::game_ready`)
//! and for the holiday jobs, which write the hub folder too. Switching back to Russian removes nothing: the
//! pack is listed under «English». A failure is logged; the next launch or switch tries again. The worker
//! runs on a thread; the App takes its result each frame (`en_pack_tick`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, TryRecvError};

use razdor::hub::{self, Hub, Source};
use razdor::i18n::{lang, tr, Lang};

use super::{game_import, holiday_start, launch, saves_move, App, Screen};

/// The hub's entry for the English maps.
pub const EN_PACK_ID: &str = "vanilla-en";

/// The pack's worker is running (the main menu's loader shows «English maps…» for it).
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether the pack's install is running now. The holiday jobs wait for it (they write the same download folder).
pub fn busy() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// The loader's text while the pack installs.
pub fn loader_label() -> Option<&'static str> {
    busy().then(|| tr("English maps…"))
}

/// Whether the pack is installed this launch (pure): the interface is English, the catalog lists the entry,
/// the game is ready, and the pack is not on the disk or its files differ from the catalog's by hash
/// (`installed`, `differs`: `hub::update_available`, which is false when it is not installed).
pub fn install_wanted(english: bool, installed: bool, differs: bool, in_catalog: bool, game_ready: bool) -> bool {
    english && in_catalog && game_ready && (!installed || differs)
}

/// The worker: the launch's fresh catalog, the pack if it is wanted (`install_wanted`). Returns whether it was
/// installed or downloaded again. A catalog without the entry is skipped without a word.
fn run(local: PathBuf) -> Result<bool, String> {
    let Some((catalog, _)) = launch::fresh() else { return Ok(false) };
    if catalog.entry(EN_PACK_ID).is_none() {
        return Ok(false);
    }
    let hub = Hub { source: Source::from_env(), local };
    let installed = hub.installed();
    let has = installed.has(EN_PACK_ID);
    let differs = hub::update_available(&hub.local, catalog, &installed, EN_PACK_ID, "en");
    if !install_wanted(true, has, differs, true, true) {
        razdor::diag!("english maps: {EN_PACK_ID} is installed and the same as the catalog");
        return Ok(false);
    }
    razdor::diag!("english maps: {EN_PACK_ID} {}", if has { "differs from the catalog; it is downloaded again" } else { "is downloaded" });
    hub.install(catalog, EN_PACK_ID, Lang::En)?;
    razdor::diag!("english maps: {EN_PACK_ID} is installed");
    Ok(true)
}

impl App {
    /// Each frame from the main menu (after `holiday_switch_tick`): takes the worker's result when it lands and,
    /// when the interface is English and no attempt was made for this trigger, starts the worker once the game
    /// is ready and the launch's catalog has come (only the cached one: wait for the fresh one).
    pub(super) fn en_pack_tick(&mut self) {
        let now = lang();
        if self.en_seen != Some(now) {
            // The first frame of the launch, or a language change: an English interface is a trigger of its own.
            if now == Lang::En {
                self.en_tried = false;
            }
            self.en_seen = Some(now);
        }
        if let Some(rx) = self.en_job.as_ref() {
            let done = match rx.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Ok(true)) => {
                    self.scenarios = super::scenarios_of(self.assets.dt.as_ref().map(|d| &d.install));
                    true
                }
                Ok(Ok(false)) => true,
                Ok(Err(why)) => {
                    razdor::diag!("english maps: the pack could not be installed ({why}); tried again on the next launch or switch to English");
                    true
                }
                Err(TryRecvError::Disconnected) => {
                    razdor::diag!("english maps: the worker ended without a result");
                    true
                }
            };
            if done {
                self.en_job = None;
                RUNNING.store(false, Ordering::Relaxed);
            }
            return;
        }
        if now != Lang::En || self.en_tried || super::clock::is_snapshot() || !matches!(self.screen, Screen::MainMenu) {
            return;
        }
        // The game first (`holiday_start::game_ready`), then the jobs that write the hub folder or the maps.
        if !holiday_start::game_ready(self.assets.dt.is_some(), game_import::busy()) || launch::fetching() {
            return;
        }
        if self.holiday_job.is_some() || holiday_start::files_busy() || self.mod_switch.is_some() || saves_move::busy() {
            return;
        }
        if !launch::settled() && launch::fresh().is_none() {
            // Only the cached catalog is there (or none yet): the launch's fresh one is awaited.
            return;
        }
        let Some(local) = hub::local_dir() else {
            self.en_tried = true;
            return;
        };
        self.en_tried = true;
        if launch::fresh().is_none() {
            razdor::diag!("english maps: no catalog this launch; tried on the next launch or switch to English");
            return;
        }
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("english-maps".into()).spawn(move || {
            let _ = tx.send(run(local));
        });
        if spawned.is_ok() {
            RUNNING.store(true, Ordering::Relaxed);
            self.en_job = Some(rx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_installs_for_an_english_interface_when_missing_or_changed() {
        assert!(install_wanted(true, false, false, true, true), "English, not installed, in the catalog, game ready");
        assert!(install_wanted(true, true, true, true, true), "installed but its files differ by hash");
    }

    #[test]
    fn the_pack_is_left_alone_when_it_is_the_same_or_the_conditions_fail() {
        assert!(!install_wanted(true, true, false, true, true), "installed and the same: nothing to download");
        assert!(!install_wanted(false, false, false, true, true), "Russian interface: never downloaded");
        assert!(!install_wanted(true, false, false, false, true), "the catalog lacks the entry: skip");
        assert!(!install_wanted(true, false, false, true, false), "the game is not ready: wait");
    }

    #[test]
    fn switching_back_to_russian_never_removes_the_pack() {
        // The decision has no removal: a Russian interface with the pack installed wants nothing.
        assert!(!install_wanted(false, true, true, true, true));
    }

    #[test]
    fn the_entry_id_is_the_hubs_english_maps() {
        assert_eq!(EN_PACK_ID, "vanilla-en");
    }
}
