//! The shared state: the two databases, the files' root, the clock, the settings and the in-memory
//! rate windows. Cheap to clone (an `Arc`).

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::abuse::{self, Abuse};
use crate::budget::{self, Budget, FreeFn};
use crate::clock::Clock;
use crate::config::Config;
use crate::db::Db;
use crate::files;
use crate::schema;

pub struct Inner {
    pub cfg: Config,
    pub stats: Db,
    pub cloud: Db,
    pub root: PathBuf,
    pub clock: Clock,
    /// Salt of the hashed IP keys (the bans). Kept in `root/ip_salt`, never sent anywhere.
    pub ip_salt: String,
    pub abuse: Mutex<Abuse>,
    /// Admin sessions: SHA-256 of the cookie's token -> expiry (seconds since 1970).
    pub sessions: Mutex<HashMap<String, i64>>,
    /// The stored files' running total and the disk's free space (see `budget`).
    pub budget: Budget,
    /// Permits for the requests with a large or unknown-length body (`config.large_body_permits`).
    pub body_gate: Arc<Semaphore>,
    /// One argon2 check at a time: a password check costs tens of megabytes and a CPU's worth of time.
    pub hash_gate: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    /// Opens the service on `cfg.data_dir`, with the disk's free space read by statvfs.
    pub fn open(cfg: Config, clock: Clock) -> Result<AppState, Box<dyn std::error::Error>> {
        let free = budget::disk_free(&cfg.data_dir);
        AppState::open_with(cfg, clock, free)
    }

    /// Opens the service with the free space read by `free` (the tests inject it).
    pub fn open_with(cfg: Config, clock: Clock, free: FreeFn) -> Result<AppState, Box<dyn std::error::Error>> {
        let root = cfg.data_dir.clone();
        for folder in [files::BATTLES, files::CRASHES, files::SAVES] {
            std::fs::create_dir_all(root.join(folder))?;
        }
        let stats = Db::open(&root.join("stats.sqlite"), schema::STATS)?;
        let cloud = Db::open(&root.join("cloud.sqlite"), schema::CLOUD)?;
        let now = clock.now();
        cloud.setup(|c| schema::migrate_cloud(c, now))?;
        let bans = stats.setup(abuse::load_bans)?;
        let ip_salt = load_or_create_salt(&root.join("ip_salt"))?;
        let used = budget::scan(&root);
        let budget = Budget::new(used, cfg.limits.max_data_bytes, cfg.limits.min_free_bytes, free);
        let body_gate = Arc::new(Semaphore::new(cfg.large_body_permits));
        Ok(AppState(Arc::new(Inner {
            cfg,
            stats,
            cloud,
            root,
            clock,
            ip_salt,
            abuse: Mutex::new(Abuse::with_bans(bans)),
            sessions: Mutex::new(HashMap::new()),
            budget,
            body_gate,
            hash_gate: Arc::new(Semaphore::new(1)),
        })))
    }

    pub fn abuse(&self) -> MutexGuard<'_, Abuse> {
        self.abuse.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn file(&self, folder: &str, name: &str) -> PathBuf {
        files::path(&self.root, folder, name)
    }
}

fn load_or_create_salt(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let t = text.trim().to_string();
        if t.len() == 64 {
            return Ok(t);
        }
    }
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| format!("random: {e}"))?;
    let salt = files::hex(&b);
    std::fs::write(path, &salt)?;
    Ok(salt)
}

/// SHA-256 of the parts, as lowercase hex.
pub fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    files::hex(&h.finalize())
}
