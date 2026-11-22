//! The game's analytics (`docs/superpowers/specs/2026-11-22-analytics-design.md`): the daily ping
//! (always), and with the player's yes the play events, the crash reports, the performance and the
//! battle records. Everything is queued on disk and sent by a background thread (`worker`); nothing
//! here waits for the network, and the game is fully playable with no server.
//!
//! Gating, twice: an item of the `Consent` class is refused at queueing unless the consent is yes, and
//! `worker::step` sends it only while the consent is yes. Turning the consent off purges the queued
//! consent items. The ping and the delete request are the `Always` class.

pub mod crash;
pub mod events;
pub mod perf;
pub mod queue;
pub mod sanitize;
pub mod state;
pub mod transport;
pub mod worker;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use queue::{Class, Meta, PushError, Queue};
use razdor_wire::analytics::{InstallDelete, Ping};
use razdor_wire::{paths, Validate};
use state::{Consent, State};

/// The queue's folder under the telemetry folder.
const QUEUE_DIR: &str = "queue";

/// Why an item is not queued.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// A consent item while the player has not said yes.
    NoConsent,
    /// The body does not pass its own checks (`razdor_wire::Validate`); nothing is sent.
    Invalid(String),
    Queue(PushError),
}

/// The analytics' state and queue. One per process (`start`); tests make their own in a folder.
pub struct Hub {
    dir: PathBuf,
    queue: Mutex<Queue>,
    /// Serializes the state's read-modify-write within this process. The file is the truth: every
    /// access reloads it, so several game windows on one folder see each other's pings and ids.
    state: Mutex<()>,
    /// Set when an item is queued: the sender wakes up at once.
    kick: AtomicBool,
}

impl Hub {
    /// Opens the state and the queue in `dir`. Consent items left while the consent is not yes are purged.
    pub fn open(dir: &Path, cap: u64) -> std::io::Result<Hub> {
        let mut queue = Queue::open(&dir.join(QUEUE_DIR), cap)?;
        if state::load(dir).consent != Consent::Yes {
            queue.purge_consent();
        }
        Ok(Hub { dir: dir.to_path_buf(), queue: Mutex::new(queue), state: Mutex::new(()), kick: AtomicBool::new(false) })
    }

    /// The queue, for the sender and the tests. A poisoned lock is used anyway (the queue's files are the truth).
    pub fn queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reads the state file, applies `f` and writes it back, under the process lock.
    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let _guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = state::load(&self.dir);
        let out = f(&mut st);
        if let Err(e) = state::save(&self.dir, &st) {
            crate::diag!("telemetry: the state is not saved: {e}");
        }
        out
    }

    pub fn consent(&self) -> Consent {
        self.with_state(|s| s.consent)
    }

    /// The consent as the file holds it, read without writing the state (the play events check it once a second).
    pub fn consent_now(&self) -> Consent {
        state::load(&self.dir).consent
    }

    /// Sets the player's answer. `version` is the Razdor version the question was answered in. A
    /// answer other than yes purges the queued consent items.
    pub fn set_consent(&self, consent: Consent, version: &str) {
        self.with_state(|s| {
            s.consent = consent;
            s.asked_version = Some(version.to_string());
        });
        if consent != Consent::Yes {
            self.queue().purge_consent();
        }
    }

    /// Whether the question «Помочь Раздору?» is due: never answered, or not asked in this version yet.
    pub fn question_due(&self, version: &str) -> bool {
        self.with_state(|s| s.consent == Consent::Unknown || s.asked_version.as_deref() != Some(version))
    }

    /// The install id, made at the first call (a random id, `state::new_install_id`).
    pub fn install_id(&self) -> String {
        self.with_state(|s| {
            if s.install_id.is_none() {
                s.install_id = Some(state::new_install_id());
            }
            s.install_id.clone().unwrap_or_default()
        })
    }

    /// The server ids of the crash reports sent, by their local token.
    pub fn crash_ids(&self) -> BTreeMap<String, String> {
        self.with_state(|s| s.crash_ids.clone())
    }

    pub fn remember_crash_id(&self, token: &str, id: String) {
        self.with_state(|s| {
            s.crash_ids.insert(token.to_string(), id);
        });
        self.kick();
    }

    /// Queues the daily ping for `today` (days since 1970), once a day. Always sent (not a consent item).
    pub fn ping_if_due(&self, today: u64, game: &str, lang: &str) -> Result<bool, Refused> {
        // The day is checked and marked in one locked step, so two windows starting together ping once.
        let install = self.with_state(|s| {
            if s.last_ping_day == Some(today) {
                return None;
            }
            if s.install_id.is_none() {
                s.install_id = Some(state::new_install_id());
            }
            s.last_ping_day = Some(today);
            s.install_id.clone()
        });
        let Some(install) = install else { return Ok(false) };
        let ping = Ping {
            schema: razdor_wire::SCHEMA,
            install_id: razdor_wire::InstallId::parse(&install).map_err(|e| Refused::Invalid(e.to_string()))?,
            razdor: env!("CARGO_PKG_VERSION").to_string(),
            platform: platform(),
            game: game.to_string(),
            lang: lang.to_string(),
        };
        self.enqueue_checked(paths::PING, Class::Always, &ping)?;
        Ok(true)
    }

    /// «Удалить мои данные»: the server is asked to delete the old id's data; the queue is emptied
    /// first (nothing of the old id is sent after it), and a new id is made.
    pub fn delete_my_data(&self) -> Result<String, Refused> {
        let old = self.install_id();
        self.queue().purge_all();
        let delete = InstallDelete {
            schema: razdor_wire::SCHEMA,
            install_id: razdor_wire::InstallId::parse(&old).map_err(|e| Refused::Invalid(e.to_string()))?,
        };
        self.enqueue_checked(paths::INSTALL_DELETE, Class::Always, &delete)?;
        self.with_state(|s| {
            s.install_id = Some(state::new_install_id());
            s.last_ping_day = None;
            s.crash_ids.clear();
        });
        Ok(old)
    }

    /// «Новый идентификатор»: a new id; the old one's queued items are not sent under the new one.
    pub fn new_install_id(&self) -> String {
        self.queue().purge_consent();
        self.with_state(|s| {
            s.install_id = Some(state::new_install_id());
            s.last_ping_day = None;
        });
        self.install_id()
    }

    /// The folder of the state and the queue (`state::telemetry_dir` in the app).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Checks a body (`Validate`), serializes it and queues it for `path`.
    pub fn enqueue_checked<T: serde::Serialize + Validate>(&self, path: &str, class: Class, body: &T) -> Result<u64, Refused> {
        self.enqueue_token(path, class, body, None)
    }

    /// As [`Hub::enqueue_checked`], with a token: the item's server id is kept under it (a crash report).
    pub fn enqueue_token<T: serde::Serialize + Validate>(&self, path: &str, class: Class, body: &T, token: Option<String>) -> Result<u64, Refused> {
        body.validate().map_err(|e| Refused::Invalid(e.to_string()))?;
        let json = serde_json::to_vec(body).map_err(|e| Refused::Invalid(e.to_string()))?;
        self.enqueue_bytes(path, class, json, None, token, None)
    }

    /// Queues a framed body: `header` (JSON) and the raw `body`. `token` names this item (a crash
    /// report's id is kept under it); `after` makes the item wait for that token's server id.
    pub fn enqueue_frame(
        &self,
        path: &str,
        class: Class,
        header: serde_json::Value,
        body: Vec<u8>,
        token: Option<String>,
        after: Option<String>,
    ) -> Result<u64, Refused> {
        self.enqueue_bytes(path, class, body, Some(header), token, after)
    }

    /// Queues a JSON body that was checked by the caller (the tests, and the events' batches).
    pub fn enqueue_json(&self, path: &str, class: Class, json: Vec<u8>) -> Result<u64, Refused> {
        self.enqueue_bytes(path, class, json, None, None, None)
    }

    fn enqueue_bytes(
        &self,
        path: &str,
        class: Class,
        body: Vec<u8>,
        header: Option<serde_json::Value>,
        token: Option<String>,
        after: Option<String>,
    ) -> Result<u64, Refused> {
        if class == Class::Consent && self.consent() != Consent::Yes {
            return Err(Refused::NoConsent);
        }
        let meta = Meta { seq: 0, path: path.to_string(), class, header, token, after, tries: 0, not_before: 0 };
        let seq = self.queue().push(meta, &body).map_err(Refused::Queue)?;
        self.kick();
        Ok(seq)
    }

    fn kick(&self) {
        self.kick.store(true, Ordering::SeqCst);
    }

    /// True once after each queued item (or a kick): the sender's wake-up.
    pub fn take_kick(&self) -> bool {
        self.kick.swap(false, Ordering::SeqCst)
    }
}

/// The platform as the replay header writes it: `os arch bits`.
pub fn platform() -> String {
    format!("{} {} {}", std::env::consts::OS, std::env::consts::ARCH, usize::BITS)
}

/// The page that says what is sent (the site's and the wiki's «what we send» page). A placeholder
/// until the owner sets the domain: change it here, the one place in the game.
pub const WHAT_WE_SEND_URL: &str = "https://razdor.invalid/what-we-send";

static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

/// The process's analytics, once [`start`] has run.
pub fn hub() -> Option<&'static Hub> {
    HUB.get().map(|h| &**h)
}

/// Whether the analytics run in this process at all: not under `cargo test`, not in the off-screen
/// scenes or snapshots (their screens must not carry a question), and not with `RAZDOR_TELEMETRY=off`.
pub fn active() -> bool {
    !cfg!(test)
        && std::env::var_os("RAZDOR_TELEMETRY").is_none_or(|v| v != "off")
        && std::env::var_os("RAZDOR_SNAPSHOT").is_none()
        && std::env::var_os("RAZDOR_SCENE").is_none()
}

/// The install folder named at start (`RAZDOR_DT_DIR`); the crash report's clean-up names it.
static DT_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Set by the first [`set_context`]: the last run's crash report is looked for once per run.
static CRASH_CHECKED: AtomicBool = AtomicBool::new(false);

/// Start-up: opens the analytics in the app's data folder and starts the sender thread. Nothing is
/// queued here: the game id and the language are known only once the install is loaded, so the
/// day's ping and the last run's crash report are queued by [`set_context`]. `dt` is the install
/// folder (cleaned out of the report). Does nothing when [`active`] is false.
pub fn start(dt: Option<&Path>) {
    if !active() {
        return;
    }
    let Some(dir) = state::telemetry_dir() else { return };
    let opened = match Hub::open(&dir, razdor_wire::limits::CLIENT_QUEUE_BYTES) {
        Ok(h) => Arc::new(h),
        Err(e) => {
            crate::diag!("telemetry: the queue is not opened: {e}");
            return;
        }
    };
    let _ = DT_DIR.set(dt.map(Path::to_path_buf));
    if HUB.set(opened).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("telemetry".into()).spawn(|| {
        let Some(hub) = hub() else { return };
        worker::run(hub, &transport::Live, &crate::remote_config::api, &now_secs);
    });
    if let Err(e) = spawned {
        crate::diag!("telemetry: the sender does not start: {e}");
    }
}

/// Called once the install is loaded (or known to be missing) and the interface language is set:
/// `game` is the install's game id (`dt`, or the active mod's id) and `lang` the language code
/// (`ru` or `en`). Queues the day's ping (once a UTC day: a second call that day queues nothing,
/// so a phone that finds its install later keeps the first call's id for that day) and, on the
/// first call of the run, the last run's crash report with the newest save. Nothing happens before
/// [`start`].
pub fn set_context(game: &str, lang: &str) {
    let Some(hub) = hub() else { return };
    if let Err(e) = hub.ping_if_due(now_secs() / 86_400, game, lang) {
        crate::diag!("telemetry: no ping: {e:?}");
    }
    if CRASH_CHECKED.swap(true, Ordering::SeqCst) {
        return;
    }
    // The crashed run's log: `diag::init` moved it aside when this run started.
    let previous = crate::diag::log_path().map(|p| p.with_file_name("razdor.previous.log"));
    let saves = crate::rules::save::games_dir();
    let dt = DT_DIR.get().and_then(|d| d.as_deref());
    let names = sanitize::Paths::for_this_machine(dt, saves.as_deref(), &[]);
    let newest = saves.as_deref().and_then(crash::newest_save);
    let _ = crash::collect(hub, previous.as_deref(), &names, game, BTreeMap::new(), newest);
}

/// The install is loaded (`installed`: a game install was found) and the language is set: the game
/// id is the active mod's id when a mod is the one played, else `dt`. Calls [`set_context`].
pub fn install_loaded(installed: bool) {
    let mod_id = if installed { crate::hub::local_dir().and_then(|l| crate::hub::active_mod(&l)) } else { None };
    let game = mod_id.unwrap_or_else(|| "dt".to_string());
    set_context(&game, crate::i18n::lang().code());
}

/// Unix seconds now.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub(name: &str) -> (Hub, PathBuf) {
        let dir = std::env::temp_dir().join(format!("razdor-hub-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Hub::open(&dir, razdor_wire::limits::CLIENT_QUEUE_BYTES).unwrap(), dir)
    }

    #[test]
    fn before_consent_only_the_ping_is_queued() {
        let (hub, dir) = hub("before");
        assert_eq!(hub.consent(), Consent::Unknown);
        assert_eq!(hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()), Err(Refused::NoConsent));
        assert_eq!(hub.ping_if_due(20_000, "dt", "ru"), Ok(true));
        let paths: Vec<String> = hub.queue().metas().map(|m| m.path.clone()).collect();
        assert_eq!(paths, [paths::PING.to_string()], "nothing but the ping");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ping_is_queued_once_a_day() {
        let (hub, dir) = hub("daily");
        assert_eq!(hub.ping_if_due(20_000, "dt", "ru"), Ok(true));
        assert_eq!(hub.ping_if_due(20_000, "dt", "ru"), Ok(false));
        assert_eq!(hub.ping_if_due(20_001, "dt", "ru"), Ok(true));
        assert_eq!(hub.queue().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_ping_per_utc_day_across_restarts_and_several_windows() {
        let (first, dir) = hub("one-ping");
        let cap = razdor_wire::limits::CLIENT_QUEUE_BYTES;
        assert_eq!(first.ping_if_due(20_000, "dt", "ru"), Ok(true));
        // A second game window on the same folder, opened while the first runs: no second ping today.
        let second = Hub::open(&dir, cap).unwrap();
        assert_eq!(second.ping_if_due(20_000, "dt", "ru"), Ok(false));
        // A restart the same day: still none.
        drop((first, second));
        let restarted = Hub::open(&dir, cap).unwrap();
        assert_eq!(restarted.ping_if_due(20_000, "dt", "ru"), Ok(false));
        assert_eq!(restarted.queue().len(), 1, "a failed send stays the same day's one ping");
        // The next UTC day: one more.
        assert_eq!(restarted.ping_if_due(20_001, "dt", "ru"), Ok(true));
        assert_eq!(restarted.queue().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ping_carries_the_game_id_and_language_it_was_queued_with() {
        let (hub, dir) = hub("game");
        assert_eq!(hub.ping_if_due(20_000, "evolution", "ru"), Ok(true));
        // A later call the same day (the install found later, another language) queues nothing.
        assert_eq!(hub.ping_if_due(20_000, "dt", "en"), Ok(false));
        let seq = hub.queue().metas().next().map(|m| m.seq).expect("the ping");
        let body = hub.queue().body(seq).unwrap();
        let ping: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(ping["game"], "evolution");
        assert_eq!(ping["lang"], "ru");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saying_no_purges_the_consent_items_and_keeps_the_ping() {
        let (hub, dir) = hub("no");
        hub.set_consent(Consent::Yes, "1.5.4");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        hub.ping_if_due(20_000, "dt", "ru").unwrap();
        hub.set_consent(Consent::No, "1.5.4");
        let paths: Vec<String> = hub.queue().metas().map(|m| m.path.clone()).collect();
        assert_eq!(paths, [paths::PING.to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_question_is_due_until_answered_and_again_after_an_update() {
        let (hub, dir) = hub("question");
        assert!(hub.question_due("1.5.4"));
        hub.set_consent(Consent::No, "1.5.4");
        assert!(!hub.question_due("1.5.4"));
        assert!(hub.question_due("1.5.5"), "asked once after an update");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_the_data_sends_the_delete_then_gives_a_new_id() {
        let (hub, dir) = hub("delete");
        let before = hub.install_id();
        hub.set_consent(Consent::Yes, "1.5.4");
        hub.enqueue_json("/v1/events", Class::Consent, b"{}".to_vec()).unwrap();
        let old = hub.delete_my_data().unwrap();
        assert_eq!(old, before);
        assert_ne!(hub.install_id(), before);
        let paths: Vec<String> = hub.queue().metas().map(|m| m.path.clone()).collect();
        assert_eq!(paths, [paths::INSTALL_DELETE.to_string()], "the old id's events are not sent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_install_id_is_kept_across_a_reopen() {
        let (hub, dir) = hub("keep");
        let id = hub.install_id();
        drop(hub);
        let again = Hub::open(&dir, razdor_wire::limits::CLIENT_QUEUE_BYTES).unwrap();
        assert_eq!(again.install_id(), id);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
