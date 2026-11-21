//! The season start («Новогодний вид», `holiday`): on the first main-menu frame of a season (from 1
//! December to 15 January), the Christmas mod is downloaded from the hub (when it is not there),
//! played (as the new-game window's «Мод» does), the look is turned on and the welcome window opens.
//! Once per season (`holiday_asked` names the season year). A failure (offline, the download) is
//! logged and the start is tried again on the next start. The menu only: nothing is loaded here.

use std::path::PathBuf;
use std::sync::mpsc::channel;

use razdor::hub::{Hub, Source};
use razdor::i18n::lang;

use super::{finished, holiday, holiday_windows, App};

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
    Ok(Start { id, was_installed })
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
