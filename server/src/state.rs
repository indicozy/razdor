//! The shared state: the two databases, the files' root, the clock, the settings and the in-memory
//! rate windows. Cheap to clone (an `Arc`).

use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use sha2::{Digest, Sha256};

use crate::abuse::Abuse;
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
    pub fn open(cfg: Config, clock: Clock) -> Result<AppState, Box<dyn std::error::Error>> {
        let root = cfg.data_dir.clone();
        for folder in [files::BATTLES, files::CRASHES, files::SAVES] {
            std::fs::create_dir_all(root.join(folder))?;
        }
        let stats = Db::open(&root.join("stats.sqlite"), schema::STATS)?;
        let cloud = Db::open(&root.join("cloud.sqlite"), schema::CLOUD)?;
        let ip_salt = load_or_create_salt(&root.join("ip_salt"))?;
        Ok(AppState(Arc::new(Inner {
            cfg,
            stats,
            cloud,
            root,
            clock,
            ip_salt,
            abuse: Mutex::new(Abuse::default()),
            sessions: Mutex::new(HashMap::new()),
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
