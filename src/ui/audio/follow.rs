//! Following the system's default audio output (Windows; a no-op elsewhere, as no default changes
//! are read there).
//!
//! The watcher (`output::current`) reads the default output's endpoint id every couple of seconds.
//! [`Follow`] decides, for each id it sees, whether the sounds must move to it: once per change,
//! not for the same id again, and with a back-off after a failed move that gives up after
//! [`MAX_TRIES`] tries for that id (a new default restarts the count).

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Tries for one default output before giving up on it (until the default changes again).
pub const MAX_TRIES: u32 = 3;

/// What the output is open on, and the move that failed for a default output, if any.
pub struct Follow {
    opened: Option<String>,
    pending: Option<Pending>,
}

struct Pending {
    id: String,
    failures: u32,
    retry_at: Instant,
}

/// The wait after the `failures`th failed move: 2 s, then 4 s.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs(2u64 << (failures.max(1) - 1).min(4))
}

impl Follow {
    /// `opened`: the endpoint id the output was opened on (`None`: it was not opened).
    pub fn new(opened: Option<String>) -> Follow {
        Follow { opened, pending: None }
    }

    /// Whether the sounds must move to the default output `current` (read now; `None`: no
    /// default output, or not read yet, which keeps the current one).
    pub fn wants(&self, now: Instant, current: Option<&str>) -> bool {
        let Some(cur) = current else { return false };
        if self.opened.as_deref() == Some(cur) {
            return false;
        }
        match &self.pending {
            Some(p) if p.id == cur => p.failures < MAX_TRIES && now >= p.retry_at,
            _ => true,
        }
    }

    /// The move to `id` was tried at `now`: `ok` once the sounds play on it. A failure waits
    /// before the next try.
    pub fn reopened(&mut self, now: Instant, id: &str, ok: bool) {
        if ok {
            self.opened = Some(id.to_string());
            self.pending = None;
            return;
        }
        let failures = match &self.pending {
            Some(p) if p.id == id => p.failures + 1,
            _ => 1,
        };
        self.pending = Some(Pending { id: id.to_string(), failures, retry_at: now + backoff(failures) });
    }
}

/// The loaded sounds' folder, and the follow state for the output they are on.
pub struct DeviceFollow {
    pub dir: PathBuf,
    pub state: Follow,
}

impl DeviceFollow {
    pub fn new(dir: PathBuf, opened_on: Option<String>) -> DeviceFollow {
        DeviceFollow { dir, state: Follow::new(opened_on) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> Instant {
        // A base well before any test's offsets, so `now + backoff` never underflows anything.
        Instant::now() + Duration::from_secs(secs)
    }

    #[test]
    fn the_same_default_output_keeps_the_sounds() {
        let f = Follow::new(Some("A".into()));
        assert!(!f.wants(at(0), Some("A")));
    }

    #[test]
    fn no_default_output_keeps_the_sounds() {
        let f = Follow::new(Some("A".into()));
        assert!(!f.wants(at(0), None));
        let none_opened = Follow::new(None);
        assert!(!none_opened.wants(at(0), None));
    }

    #[test]
    fn a_changed_default_moves_the_sounds_once() {
        let mut f = Follow::new(Some("A".into()));
        assert!(f.wants(at(0), Some("B")));
        f.reopened(at(0), "B", true);
        // The watcher keeps reading B: no second move.
        for s in 1..10 {
            assert!(!f.wants(at(s), Some("B")));
        }
        // Switching back is a change again.
        assert!(f.wants(at(10), Some("A")));
    }

    #[test]
    fn an_output_that_was_never_opened_is_opened_when_a_default_appears() {
        let mut f = Follow::new(None);
        assert!(f.wants(at(0), Some("A")));
        f.reopened(at(0), "A", true);
        assert!(!f.wants(at(1), Some("A")));
    }

    #[test]
    fn a_failed_move_waits_then_retries_then_gives_up() {
        let mut f = Follow::new(Some("A".into()));
        assert!(f.wants(at(0), Some("B")));
        f.reopened(at(0), "B", false);
        // The back-off after the first failure is 2 s.
        assert!(!f.wants(at(1), Some("B")));
        assert!(f.wants(at(2), Some("B")));
        f.reopened(at(2), "B", false);
        // The second failure waits 4 s.
        assert!(!f.wants(at(5), Some("B")));
        assert!(f.wants(at(6), Some("B")));
        f.reopened(at(6), "B", false);
        // The third failure gives up on B, however long the wait.
        assert!(!f.wants(at(10_000), Some("B")));
        // Another default output is still tried at once.
        assert!(f.wants(at(6), Some("C")));
    }

    #[test]
    fn a_success_after_failures_starts_the_count_again() {
        let mut f = Follow::new(Some("A".into()));
        f.reopened(at(0), "B", false);
        f.reopened(at(2), "B", false);
        assert!(f.wants(at(10), Some("B")));
        f.reopened(at(10), "B", true);
        assert!(!f.wants(at(11), Some("B")));
        // Back to A, then B again: the count starts from one.
        assert!(f.wants(at(12), Some("A")));
        f.reopened(at(12), "A", false);
        assert!(f.wants(at(14), Some("A")));
    }

    /// A fake default-output source: one read per tick, moving the sounds through `reopen`, which
    /// stands for the real move and answers whether it worked. Returns the moves made.
    fn drive(f: &mut Follow, reads: &[Option<&str>], ok: bool) -> Vec<String> {
        let mut moves = Vec::new();
        for (tick, read) in reads.iter().enumerate() {
            let now = at(tick as u64 * 10);
            if f.wants(now, *read) {
                let id = read.unwrap();
                moves.push(id.to_string());
                f.reopened(now, id, ok);
            }
        }
        moves
    }

    #[test]
    fn a_device_switch_and_back_moves_twice_and_never_loops() {
        let mut f = Follow::new(Some("A".into()));
        let reads = [Some("A"), Some("A"), Some("B"), Some("B"), Some("B"), Some("A"), None, Some("A")];
        assert_eq!(drive(&mut f, &reads, true), vec!["B", "A"]);
    }

    #[test]
    fn a_move_that_keeps_failing_is_tried_a_bounded_number_of_times() {
        let mut f = Follow::new(Some("A".into()));
        let reads = vec![Some("B"); 200];
        assert_eq!(drive(&mut f, &reads, false).len(), MAX_TRIES as usize);
    }
}
