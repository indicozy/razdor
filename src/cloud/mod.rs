//! Cloud saves on the client (`docs/superpowers/specs/2026-11-22-cloud-saves-design.md`). No
//! macroquad: the UI phase calls [`Cloud`], and never waits on it.
//!
//! - [`account`]: the account key (0600 in the cloud folder) and the recovery phrase.
//! - [`select`]: which local saves this device mirrors (pinned, then «Продолжить», then newest; the
//!   device's cap and the quota) and each save's state (☁, ⏳).
//! - [`merge`]: marks (union), settings (newest change per key), newer-Razdor detection.
//! - [`sync`]: the device's mirror (push deletes its own stale rows, uploads the chosen ones), the list
//!   of other devices' saves (metadata only), and the download on demand, over a [`sync::Local`].
//! - [`api`] and [`http`]: the typed calls and the transport (curl on desktop).
//! - [`pack`]: zstd on the client, and the hash the server checks.
//!
//! The sync runs on one background thread ([`Cloud`]): a request returns at once, offline it waits
//! and retries with a growing pause, and nothing in the game waits for it. With no account the
//! worker does no network at all.

pub mod account;
pub mod api;
pub mod http;
pub mod merge;
pub mod pack;
pub mod select;
pub mod sync;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod e2e;

pub use api::CloudError;
pub use select::SaveState;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use razdor_wire::cloud::{LinkIssued, SaveEntry};

use self::api::Api;
use self::http::Net;
use self::sync::Local;

/// The first pause after an offline failure, in seconds; it doubles up to [`MAX_RETRY_SECS`].
const FIRST_RETRY_SECS: u64 = 60;
const MAX_RETRY_SECS: u64 = 6 * 3600;

/// The next pause after `prev` seconds (0 for none yet): doubled, from a minute, up to six hours.
pub fn next_backoff(prev: u64) -> u64 {
    if prev == 0 {
        FIRST_RETRY_SECS
    } else {
        (prev * 2).min(MAX_RETRY_SECS)
    }
}

/// The wait with jitter: `secs` times a factor from 0.8 to 1.2 (80 + `random` % 41 percent), never under
/// one second, so devices that failed together do not come back in step. `random` is any u64 (see
/// [`random_u64`]); the function is pure, so its bounds are tested.
pub fn jittered(secs: u64, random: u64) -> u64 {
    let percent = 80 + random % 41;
    (secs.saturating_mul(percent) / 100).max(1)
}

/// The wait before a retry: the jittered backoff, or the server's `Retry-After` when that is longer.
pub fn retry_wait_secs(backoff: u64, random: u64, retry_after: u64) -> u64 {
    jittered(backoff, random).max(retry_after)
}

/// Eight random bytes from the OS, or None when it gives none.
fn random_u64() -> Option<u64> {
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).ok()?;
    Some(u64::from_le_bytes(raw))
}

/// One request to the worker. Each one runs in order on the background thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// «Включить облачные сохранения»: a new account, its key kept on this device.
    CreateAccount,
    /// «Привязать устройство»: a link code for another device (in [`Shared::link`]).
    Link,
    /// The second device types the code and keeps the account's key.
    Redeem(String),
    /// «Удалить аккаунт»: the server deletes it; this device forgets the key.
    DeleteAccount,
    /// Pull, then push. The start of the game and «Синхронизировать сейчас» use it.
    Sync,
    /// The list, the marks, the settings and the Hub offer (no upload).
    Pull,
    /// On exit and on minimising: this device's mirror only.
    Push,
    /// The «Облако» tab's list of the other devices' saves (no download, no upload).
    Refresh,
    /// A save of another device, downloaded on demand (its cloud id); it becomes a local save.
    Download(String),
}

/// Where the cloud stands, for the settings row and the load list's notice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// No account on this device: nothing is sent.
    #[default]
    Off,
    Idle,
    Working,
    /// No answer from the server: it waits and retries.
    Offline,
    /// The account's quota is full: «Облако заполнено, удалите старые сохранения».
    Full,
    /// The last request was refused or its answer did not check out (see [`Shared::last_error`]).
    Failed,
}

/// The state the UI reads (a copy, taken with [`Cloud::snapshot`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shared {
    pub linked: bool,
    pub phase: Phase,
    /// The unix time of the last sync that got through.
    pub last_sync: Option<u64>,
    /// Each local save's state, by the store's id.
    pub states: HashMap<String, SaveState>,
    /// Other devices' saves this device does not hold: listed in the «Облако» tab, downloaded on demand.
    pub others: Vec<SaveEntry>,
    /// The account's saves from a newer Razdor: listed locked, never downloaded or deleted.
    pub newer: Vec<SaveEntry>,
    /// The quota in use (bytes, after zstd) at the last round.
    pub used_bytes: u64,
    /// The last link code issued (for the settings row), with its expiry.
    pub link: Option<LinkIssued>,
    /// The last round's per-save errors.
    pub errors: Vec<String>,
    pub last_error: Option<CloudError>,
    /// A list is being read (the «Облако» tab's refresh job runs).
    pub refreshing: bool,
    /// The remote id of the save whose download job runs.
    pub downloading: Option<String>,
    /// The last download that landed: (the remote id, the local save's id).
    pub landed: Option<(String, String)>,
    /// The remote id of the last download that failed (cleared by the next one that lands).
    pub download_failed: Option<String>,
    /// How many lists have been read: the «Облако» tab knows its list is new when this moves.
    pub listed: u64,
    /// A sync failed (not offline, not full): the automatic syncs wait until the player asks again
    /// («Синхронизировать», turning the cloud on) or the app restarts. Never hammer the server.
    pub paused: bool,
}

/// The handle the UI keeps. Cheap to call: requests are queued, state is read from a copy.
pub struct Cloud {
    tx: mpsc::Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    /// Requests queued and not yet run (see [`Cloud::finish`]).
    queued: Arc<AtomicUsize>,
}

impl Cloud {
    /// Starts the worker for `local`, talking to `base` (the API's address from remote config) with
    /// `net`. `cloud_dir` holds the account key. The worker thread stops when the handle is dropped.
    pub fn start(local: Box<dyn Local>, net: Arc<dyn Net>, base: &str, cloud_dir: PathBuf) -> Cloud {
        let key = account::load_key(&cloud_dir);
        let linked = key.is_some();
        let shared = Arc::new(Mutex::new(Shared {
            linked,
            phase: if linked { Phase::Idle } else { Phase::Off },
            ..Shared::default()
        }));
        let (tx, rx) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let worker = Worker {
            queued: queued.clone(),
            local,
            net,
            base: base.to_string(),
            cloud_dir,
            key,
            shared: shared.clone(),
            retry: None,
            backoff_secs: 0,
        };
        // A failed spawn leaves the requests unanswered: the game plays on, the cloud stays off.
        let _ = std::thread::Builder::new().name("razdor-cloud".into()).spawn(move || worker.run(rx));
        Cloud { tx, shared, queued }
    }

    /// Queues an automatic request (the start, the exit, minimising). Never waits. While the syncs are
    /// paused after a failure, a sync, pull, push or refresh is dropped; the player's own jobs are not.
    pub fn request(&self, job: Job) {
        let paused = lock(&self.shared).paused;
        if paused && matches!(job, Job::Sync | Job::Pull | Job::Push | Job::Refresh) {
            crate::diag!("cloud: {job:?} skipped: paused after a failed sync");
            return;
        }
        self.enqueue(job);
    }

    /// Queues a request the player made («Синхронизировать», turning the cloud on): it also lifts the
    /// pause after a failed sync. Never waits.
    pub fn request_by_player(&self, job: Job) {
        lock(&self.shared).paused = false;
        self.enqueue(job);
    }

    fn enqueue(&self, job: Job) {
        self.queued.fetch_add(1, Ordering::SeqCst);
        if self.tx.send(job).is_err() {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Waits, at most `wait`, until the queued requests have run (the exit's push). True when they
    /// all ran; a retry of an offline round is not waited for.
    pub fn finish(&self, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        while self.queued.load(Ordering::SeqCst) > 0 {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// The state as of the last finished request.
    pub fn snapshot(&self) -> Shared {
        lock(&self.shared).clone()
    }

    /// The load list's state of one local save (`LocalOnly` until a round has seen it).
    pub fn state_of(&self, id: &str) -> SaveState {
        lock(&self.shared).states.get(id).copied().unwrap_or(SaveState::LocalOnly)
    }

    /// The load list's state of one local save as the round left it (`None` before a round saw it,
    /// and for a save the mirror does not hold), with the worker's phase.
    pub fn mark_of(&self, id: &str) -> (Option<SaveState>, Phase) {
        let sh = lock(&self.shared);
        (sh.states.get(id).copied(), sh.phase)
    }

    /// Whether this device has an account.
    pub fn linked(&self) -> bool {
        lock(&self.shared).linked
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

struct Worker {
    local: Box<dyn Local>,
    net: Arc<dyn Net>,
    base: String,
    cloud_dir: PathBuf,
    key: Option<String>,
    shared: Arc<Mutex<Shared>>,
    /// The sync to retry after an offline failure, and when.
    retry: Option<(Instant, Job)>,
    backoff_secs: u64,
    queued: Arc<AtomicUsize>,
}

impl Worker {
    fn run(mut self, rx: mpsc::Receiver<Job>) {
        loop {
            let due = match &self.retry {
                Some((at, _)) if *at <= Instant::now() => self.retry.take().map(|(_, job)| job),
                _ => None,
            };
            let due_was_none = due.is_none();
            let job = match due {
                Some(job) => job,
                None => {
                    let wait = match &self.retry {
                        Some((at, _)) => at.saturating_duration_since(Instant::now()),
                        None => Duration::from_secs(24 * 3600),
                    };
                    match rx.recv_timeout(wait) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
            };
            let requested = due_was_none;
            self.execute(job);
            if requested {
                self.queued.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    fn execute(&mut self, job: Job) {
        let download = match &job {
            Job::Download(id) => Some(id.clone()),
            _ => None,
        };
        {
            let mut sh = lock(&self.shared);
            sh.phase = Phase::Working;
            sh.refreshing = matches!(job, Job::Refresh);
            sh.downloading = download.clone();
        }
        let retryable = matches!(job, Job::Sync | Job::Pull | Job::Push | Job::Refresh | Job::CreateAccount);
        match self.run_job(&job) {
            Ok(()) => {
                let mut sh = lock(&self.shared);
                sh.phase = if sh.linked { Phase::Idle } else { Phase::Off };
                sh.last_error = None;
                if download.is_some() {
                    sh.download_failed = None;
                }
                self.backoff_secs = 0;
            }
            Err(e) => {
                crate::diag!("cloud: {job:?} failed: {e}");
                let mut sh = lock(&self.shared);
                if download.is_some() {
                    sh.download_failed = download.clone();
                }
                sh.phase = match e {
                    CloudError::Offline(_) | CloudError::Throttled { .. } => Phase::Offline,
                    CloudError::Full => Phase::Full,
                    _ => Phase::Failed,
                };
                if sh.phase == Phase::Failed {
                    sh.paused = true;
                }
                let offline = matches!(e, CloudError::Offline(_) | CloudError::Throttled { .. });
                let server_wait = match &e {
                    CloudError::Throttled { retry_after, .. } => *retry_after,
                    _ => 0,
                };
                sh.last_error = Some(e);
                drop(sh);
                if offline && retryable {
                    self.backoff_secs = next_backoff(self.backoff_secs);
                    let wait = retry_wait_secs(self.backoff_secs, random_u64().unwrap_or(20), server_wait);
                    self.retry = Some((Instant::now() + Duration::from_secs(wait), job));
                }
            }
        }
        let mut sh = lock(&self.shared);
        sh.refreshing = false;
        sh.downloading = None;
    }

    fn api(&self) -> Option<Api> {
        Some(Api::new(self.net.clone(), &self.base, Some(self.key.clone()?)))
    }

    fn run_job(&mut self, job: &Job) -> Result<(), CloudError> {
        match job {
            Job::CreateAccount => {
                // Already linked (a retry that runs after the player's own try): no second account.
                if self.key.is_some() {
                    return Ok(());
                }
                // The device's own key is on disk before the request, so a retry sends the same one.
                let pending = account::pending_key(&self.cloud_dir).map_err(CloudError::Refused)?;
                let key = Api::new(self.net.clone(), &self.base, None).create_account(&pending)?;
                self.keep_key(&key)?;
                // The account key is kept; a pending file left by a failed removal is harmless.
                let _ = account::forget_pending(&self.cloud_dir);
                Ok(())
            }
            Job::Redeem(code) => {
                let key = Api::new(self.net.clone(), &self.base, None).link_redeem(code)?;
                self.keep_key(&key)
            }
            Job::Link => {
                let api = self.api().ok_or(CloudError::Unauthorized)?;
                let link = api.link_issue()?;
                lock(&self.shared).link = Some(link);
                Ok(())
            }
            Job::DeleteAccount => {
                let api = self.api().ok_or(CloudError::Unauthorized)?;
                api.delete_account()?;
                let _ = account::forget_key(&self.cloud_dir);
                self.key = None;
                let mut sh = lock(&self.shared);
                sh.linked = false;
                sh.states.clear();
                sh.newer.clear();
                Ok(())
            }
            Job::Download(id) => {
                let api = self.api().ok_or(CloudError::Unauthorized)?;
                let local_id = sync::download(&api, self.local.as_mut(), id)?;
                lock(&self.shared).landed = Some((id.clone(), local_id));
                // The list changed: the downloaded save is no longer «on another device».
                let report = sync::refresh(&api, self.local.as_ref())?;
                self.show_list(report);
                Ok(())
            }
            Job::Refresh => {
                // Without an account nothing is sent, and nothing fails.
                let Some(api) = self.api() else { return Ok(()) };
                let report = sync::refresh(&api, self.local.as_ref())?;
                self.show_list(report);
                Ok(())
            }
            Job::Sync | Job::Pull | Job::Push => {
                // Without an account nothing is sent, and nothing fails.
                let Some(api) = self.api() else { return Ok(()) };
                let report = match job {
                    Job::Pull => sync::pull(&api, self.local.as_mut())?,
                    Job::Push => sync::push(&api, self.local.as_mut())?,
                    _ => sync::sync(&api, self.local.as_mut())?,
                };
                let full = report.full;
                let mut sh = lock(&self.shared);
                sh.states = report.states;
                sh.others = report.others;
                sh.newer = report.newer;
                sh.used_bytes = report.used_bytes;
                sh.errors = report.errors;
                sh.last_sync = Some(sync::now_secs());
                drop(sh);
                if full {
                    Err(CloudError::Full)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Shows a list read on its own (the «Облако» tab's refresh, or after a download).
    fn show_list(&self, report: sync::Report) {
        let mut sh = lock(&self.shared);
        sh.others = report.others;
        sh.newer = report.newer;
        sh.used_bytes = report.used_bytes;
        sh.listed += 1;
    }

    /// Stores the account's key on this device and marks it linked.
    fn keep_key(&mut self, key: &str) -> Result<(), CloudError> {
        account::store_key(&self.cloud_dir, key).map_err(|e| CloudError::Refused(format!("the key file: {e}")))?;
        self.key = Some(key.to_string());
        lock(&self.shared).linked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::http::Reply;
    use crate::cloud::testing::MemLocal;
    use std::sync::atomic::AtomicUsize;

    /// A transport that never answers in time: each call sleeps, then fails like no network.
    struct Slow;

    impl Net for Slow {
        fn send(&self, _: &str, _: &str, _: Option<&str>, _: Option<&[u8]>) -> Result<Reply, String> {
            std::thread::sleep(Duration::from_millis(1500));
            Err("curl: exit 28".into())
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-cloud-mod-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn without_an_account_nothing_is_sent_and_nothing_waits() {
        let d = dir("off");
        let cloud = Cloud::start(Box::new(MemLocal::new("ПК")), Arc::new(Slow), "http://127.0.0.1:9", d.clone());
        assert!(!cloud.linked());
        let t = Instant::now();
        cloud.request(Job::Sync);
        cloud.request(Job::Push);
        // A request never waits for the network.
        assert!(t.elapsed() < Duration::from_millis(500));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(cloud.snapshot().phase, Phase::Off);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreachable_server_is_offline_and_the_game_goes_on() {
        let d = dir("offline");
        account::store_key(&d, &"ab".repeat(32)).expect("stores a key");
        let cloud = Cloud::start(Box::new(MemLocal::new("ПК")), Arc::new(Slow), "http://127.0.0.1:9", d.clone());
        assert!(cloud.linked());
        let t = Instant::now();
        cloud.request(Job::Sync);
        assert!(t.elapsed() < Duration::from_millis(500), "the request returns at once");
        // The worker's call takes 1.5 s and then reports offline.
        let mut phase = Phase::Working;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(100));
            phase = cloud.snapshot().phase;
            if phase == Phase::Offline {
                break;
            }
        }
        assert_eq!(phase, Phase::Offline);
        assert_eq!(cloud.state_of("anything"), SaveState::LocalOnly, "the game still shows its saves");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A transport that answers every call with `status` and counts the calls.
    struct Refusing {
        status: u16,
        calls: Arc<AtomicUsize>,
    }

    impl Net for Refusing {
        fn send(&self, _: &str, _: &str, _: Option<&str>, _: Option<&[u8]>) -> Result<Reply, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Reply { status: self.status, body: b"{\"error\":\"bad\"}".to_vec(), retry_after: None })
        }
    }

    fn wait_for_phase(cloud: &Cloud, want: Phase) -> bool {
        for _ in 0..100 {
            if cloud.snapshot().phase == want {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_failed_sync_pauses_the_automatic_ones_until_the_player_asks() {
        let d = dir("paused");
        account::store_key(&d, &"ab".repeat(32)).expect("stores a key");
        let calls = Arc::new(AtomicUsize::new(0));
        let net = Refusing { status: 400, calls: calls.clone() };
        let cloud = Cloud::start(Box::new(MemLocal::new("ПК")), Arc::new(net), "http://127.0.0.1:9", d.clone());
        cloud.request(Job::Sync);
        assert!(wait_for_phase(&cloud, Phase::Failed), "a 400 is a failed sync");
        assert!(cloud.snapshot().paused);
        assert!(cloud.finish(Duration::from_secs(5)));
        let after_failure = calls.load(Ordering::SeqCst);
        assert!(after_failure > 0);

        // The automatic path sends nothing while paused.
        cloud.request(Job::Push);
        cloud.request(Job::Pull);
        assert!(cloud.finish(Duration::from_secs(5)));
        assert_eq!(calls.load(Ordering::SeqCst), after_failure, "no automatic request after a failure");

        // The player's own sync runs again and lifts the pause.
        cloud.request_by_player(Job::Sync);
        assert!(cloud.finish(Duration::from_secs(5)));
        assert!(calls.load(Ordering::SeqCst) > after_failure, "the player's sync is sent");
        assert!(cloud.snapshot().paused, "it failed again, so the pause is back");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_backoff_grows_and_stops_at_six_hours() {
        let mut secs = 0u64;
        let mut seen = Vec::new();
        for _ in 0..12 {
            secs = next_backoff(secs);
            seen.push(secs);
        }
        assert_eq!(&seen[..3], &[60, 120, 240]);
        assert_eq!(*seen.last().expect("some"), MAX_RETRY_SECS);
    }

    #[test]
    fn the_jitter_stays_within_a_fifth_either_way_and_never_under_a_second() {
        assert_eq!(jittered(100, 0), 80);
        assert_eq!(jittered(100, 40), 120);
        assert_eq!(jittered(100, 41), 80, "the factor wraps through 80..=120 percent");
        assert_eq!(jittered(0, 5), 1, "never below one second");
        assert_eq!(jittered(1, 0), 1);
        for secs in [1u64, 2, 60, 3600, MAX_RETRY_SECS] {
            let mut seen = std::collections::HashSet::new();
            for random in 0..500u64 {
                let v = jittered(secs, random);
                assert!(v >= (secs * 80 / 100).max(1) && v <= secs * 120 / 100, "{secs} s -> {v} s");
                seen.insert(v);
            }
            if secs >= 60 {
                assert!(seen.len() > 10, "{secs} s: the jitter spreads ({} values)", seen.len());
            }
        }
    }

    #[test]
    fn a_server_wait_is_honoured_when_it_is_longer_than_the_backoff() {
        // The backoff (jittered) when the server asks nothing, and the server's wait when it asks more.
        assert_eq!(retry_wait_secs(60, 0, 0), 48);
        assert_eq!(retry_wait_secs(60, 0, 300), 300);
        assert_eq!(retry_wait_secs(60, 40, 30), 72, "a shorter wait does not cut the backoff");
    }

    /// A transport for the account creation: the first request gets no answer, then every request is
    /// answered with the key it carries (as the server does for a new account). It keeps the bodies.
    struct Creating {
        bodies: Arc<Mutex<Vec<String>>>,
        fail_first: std::sync::atomic::AtomicBool,
    }

    impl Net for Creating {
        fn send(&self, _: &str, _: &str, _: Option<&str>, body: Option<&[u8]>) -> Result<Reply, String> {
            let text = String::from_utf8_lossy(body.unwrap_or_default()).into_owned();
            self.bodies.lock().expect("lock").push(text.clone());
            if self.fail_first.swap(false, Ordering::SeqCst) {
                return Err("curl: exit 28".into());
            }
            let sent: serde_json::Value = serde_json::from_str(&text).expect("json");
            let key = sent["key"].as_str().expect("a key").to_string();
            Ok(Reply { status: 200, body: format!("{{\"key\":\"{key}\"}}").into_bytes(), retry_after: None })
        }
    }

    #[test]
    fn a_lost_creation_is_retried_under_the_same_key() {
        let d = dir("create-retry");
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let net = Creating { bodies: bodies.clone(), fail_first: std::sync::atomic::AtomicBool::new(true) };
        let cloud = Cloud::start(Box::new(MemLocal::new("ПК")), Arc::new(net), "http://127.0.0.1:9", d.clone());
        cloud.request_by_player(Job::CreateAccount);
        assert!(wait_for_phase(&cloud, Phase::Offline), "the first try gets no answer");
        assert!(cloud.finish(Duration::from_secs(5)));
        assert!(account::load_key(&d).is_none(), "no account key yet");
        assert!(d.join(account::PENDING_FILE).exists(), "the device's key is on disk before the answer");

        cloud.request_by_player(Job::CreateAccount);
        assert!(cloud.finish(Duration::from_secs(5)));
        let sent = bodies.lock().expect("lock").clone();
        assert_eq!(sent.len(), 2, "two requests");
        let key_of = |body: &str| {
            let v: serde_json::Value = serde_json::from_str(body).expect("json");
            v["key"].as_str().expect("a key").to_string()
        };
        assert_eq!(key_of(&sent[0]), key_of(&sent[1]), "the retry carries the same key");
        assert_eq!(account::load_key(&d).as_deref(), Some(key_of(&sent[1]).as_str()), "the account key is kept");
        assert!(!d.join(account::PENDING_FILE).exists(), "the pending key is gone");
        assert!(cloud.linked());

        // A retry that runs after the account exists sends nothing: no second account.
        cloud.request(Job::CreateAccount);
        assert!(cloud.finish(Duration::from_secs(5)));
        assert_eq!(bodies.lock().expect("lock").len(), 2, "nothing is sent for a linked device");
        let _ = std::fs::remove_dir_all(&d);
    }
}
