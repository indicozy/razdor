//! Cloud saves on the client (`docs/superpowers/specs/2026-11-22-cloud-saves-design.md`). No
//! macroquad: the UI phase calls [`Cloud`], and never waits on it.
//!
//! - [`account`]: the account key (0600 in the cloud folder) and the recovery phrase.
//! - [`select`]: which local saves this device mirrors (pinned, then «Продолжить», then newest; the
//!   device's cap and the quota) and each save's state (☁, ⏳, 💾).
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

    /// Queues a request. Never waits.
    pub fn request(&self, job: Job) {
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
        lock(&self.shared).phase = Phase::Working;
        let retryable = matches!(job, Job::Sync | Job::Pull | Job::Push | Job::Refresh);
        match self.run_job(&job) {
            Ok(()) => {
                let mut sh = lock(&self.shared);
                sh.phase = if sh.linked { Phase::Idle } else { Phase::Off };
                sh.last_error = None;
                self.backoff_secs = 0;
            }
            Err(e) => {
                let mut sh = lock(&self.shared);
                sh.phase = match e {
                    CloudError::Offline(_) => Phase::Offline,
                    CloudError::Full => Phase::Full,
                    _ => Phase::Failed,
                };
                let offline = matches!(e, CloudError::Offline(_));
                sh.last_error = Some(e);
                drop(sh);
                if offline && retryable {
                    self.backoff_secs = next_backoff(self.backoff_secs);
                    self.retry = Some((Instant::now() + Duration::from_secs(self.backoff_secs), job));
                }
            }
        }
    }

    fn api(&self) -> Option<Api> {
        Some(Api::new(self.net.clone(), &self.base, Some(self.key.clone()?)))
    }

    fn run_job(&mut self, job: &Job) -> Result<(), CloudError> {
        match job {
            Job::CreateAccount => {
                let key = Api::new(self.net.clone(), &self.base, None).create_account()?;
                self.keep_key(&key)
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
                sync::download(&api, self.local.as_mut(), id)?;
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
}
