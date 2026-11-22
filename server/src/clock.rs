//! The server's clock, in seconds since 1970. Tests fix it; the service reads the system's.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct Clock {
    /// 0: the system clock. Any other value: that fixed time (tests, the retention job's runs).
    fixed: AtomicI64,
}

impl Clock {
    pub fn system() -> Clock {
        Clock::default()
    }

    pub fn fixed(at: i64) -> Clock {
        let c = Clock::default();
        c.set(at);
        c
    }

    pub fn now(&self) -> i64 {
        match self.fixed.load(Ordering::Relaxed) {
            0 => SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
            at => at,
        }
    }

    pub fn set(&self, at: i64) {
        self.fixed.store(at, Ordering::Relaxed);
    }
}
