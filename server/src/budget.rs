//! The storage budget: the bytes of the files under the data directory (battles, crash reports and
//! saves) as a running total, and the disk's free space. Past `max_data_bytes`, or with less than
//! `min_free_bytes` free, the service takes no new uploads: the cloud answers 507, the analytics
//! writes 429, and pings and reads go on. The databases are not counted. Every write and delete of a
//! stored file goes through the helpers here, so the total stays true.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::files::{self, BATTLES, CRASHES, SAVES};
use crate::http::ApiError;
use crate::state::AppState;

/// Free bytes of the disk that holds the data directory. `u64::MAX` when unknown (no refusal).
pub type FreeFn = Box<dyn Fn() -> u64 + Send + Sync>;

pub struct Budget {
    used: AtomicI64,
    max_bytes: u64,
    min_free: u64,
    free: FreeFn,
    /// The clock's second of the last refusal log line (a line at most a minute).
    last_log: Mutex<i64>,
}

/// A storage figure as the dashboard shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Figures {
    pub used_bytes: u64,
    pub max_bytes: u64,
    pub free_bytes: u64,
    pub min_free_bytes: u64,
    pub over: bool,
}

impl Budget {
    pub fn new(used: u64, max_bytes: u64, min_free: u64, free: FreeFn) -> Budget {
        Budget { used: AtomicI64::new(used as i64), max_bytes, min_free, free, last_log: Mutex::new(0) }
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed).max(0) as u64
    }

    pub fn free_bytes(&self) -> u64 {
        (self.free)()
    }

    /// Over the byte cap, or the disk has less free than the floor.
    pub fn over(&self) -> bool {
        self.used() >= self.max_bytes || self.free_bytes() < self.min_free
    }

    pub fn figures(&self) -> Figures {
        Figures {
            used_bytes: self.used(),
            max_bytes: self.max_bytes,
            free_bytes: self.free_bytes(),
            min_free_bytes: self.min_free,
            over: self.over(),
        }
    }

    fn add(&self, delta: i64) {
        self.used.fetch_add(delta, Ordering::Relaxed);
    }

    /// A refusal by the budget, logged at most once a minute.
    pub fn note_refusal(&self, now: i64) {
        let mut last = self.last_log.lock().unwrap_or_else(|e| e.into_inner());
        if now - *last >= 60 {
            *last = now;
            let f = self.figures();
            tracing::warn!(
                "storage over its budget: {} of {} bytes used, {} bytes free (floor {}); uploads refused",
                f.used_bytes,
                f.max_bytes,
                f.free_bytes,
                f.min_free_bytes
            );
        }
    }
}

/// The analytics writes (crash, battle, perf, events): over the budget, 429, no strike.
pub fn analytics_gate(st: &AppState) -> Result<(), ApiError> {
    if st.budget.over() {
        st.budget.note_refusal(st.clock.now());
        // When the budget frees is not known here: the usual minute.
        return Err(ApiError::Limited(60));
    }
    Ok(())
}

/// The cloud's uploads (saves, new accounts): over the budget, 507, which the client shows as a full cloud.
pub fn cloud_gate(st: &AppState) -> Result<(), ApiError> {
    if st.budget.over() {
        st.budget.note_refusal(st.clock.now());
        return Err(ApiError::Full("Облако заполнено, попробуйте позже".into()));
    }
    Ok(())
}

fn size_of(path: &Path) -> i64 {
    std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
}

/// The bytes under `dir`, at every level.
fn tree_size(dir: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => tree_size(&p),
                _ => size_of(&p),
            }
        })
        .sum()
}

/// The bytes already stored under the data directory (scanned once, at start).
pub fn scan(root: &Path) -> u64 {
    [BATTLES, CRASHES, SAVES].iter().map(|f| tree_size(&root.join(f))).sum::<i64>() as u64
}

/// Writes a stored file atomically and counts it: a file replaced gives back its old size.
pub fn write(b: &Budget, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let old = size_of(path);
    files::write_atomic(path, bytes)?;
    b.add(bytes.len() as i64 - old);
    Ok(())
}

/// Removes a stored file and uncounts it. A missing file is no error.
pub fn remove(b: &Budget, path: &Path) {
    let old = size_of(path);
    match std::fs::remove_file(path) {
        Ok(()) => b.add(-old),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("remove {}: {e}", path.display()),
    }
}

/// Removes a directory tree of stored files and uncounts it. A missing directory is no error.
pub fn remove_tree(b: &Budget, dir: &Path) -> io::Result<()> {
    let old = tree_size(dir);
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {
            b.add(-old);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// The free bytes of the disk that holds `path`, by statvfs. A failed call is no refusal.
pub fn disk_free(path: &Path) -> FreeFn {
    let c: Option<CString> = CString::new(path.as_os_str().as_bytes()).ok();
    Box::new(move || {
        let Some(c) = &c else {
            return u64::MAX;
        };
        let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: statvfs writes the struct when it returns 0, and it is read only then.
        let s = unsafe {
            if libc::statvfs(c.as_ptr(), s.as_mut_ptr()) != 0 {
                return u64::MAX;
            }
            s.assume_init()
        };
        // The field types differ by system (macOS: `f_bavail` is u32), so both are widened to u64; on Linux
        // they already are, which clippy calls a useless conversion.
        #[allow(clippy::useless_conversion)]
        let (blocks, size) = (u64::from(s.f_bavail), u64::from(s.f_frsize));
        blocks.saturating_mul(size)
    })
}
