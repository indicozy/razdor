//! The hub's catalog, read once per launch: the main menu's promo ad (`ui::ads::sync_tick`) and the
//! game check with no install (`ui::game_import`) both read this one result, so a launch makes one
//! fetch. It starts at the first frame of the app (`start`), on a thread. When that fetch fails, the
//! thread tries again a few times, after a wait that grows (`RETRY_WAITS`), and a catalog that comes
//! that way is the launch's fresh one too (`fresh`): the holiday look and the Hub window read it
//! instead of the cached catalog. No other fetch runs on a timer.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use razdor::hub::{self, Catalog, Hub, Source};

/// The launch's catalog as it came: the catalog, or why it could not be read.
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
/// When the first fetch's catalog came (only when it came).
static FETCHED_AT: OnceLock<SystemTime> = OnceLock::new();
/// A catalog a retry read after the first fetch failed, and when.
static RETRIED: OnceLock<(Catalog, SystemTime)> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
/// The first fetch and every retry have ended (with or without a catalog).
static SETTLED: AtomicBool = AtomicBool::new(false);

/// The waits before the retries of a failed first fetch: three retries, after 10 s, 60 s and 5 min. The
/// fetch itself honours the hosts' pauses (`mirror::wait_host`), so a throttled host is not hit sooner.
pub const RETRY_WAITS: [Duration; 3] = [Duration::from_secs(10), Duration::from_secs(60), Duration::from_secs(300)];

/// Starts the launch's fetch on a thread (once). Not in a snapshot: its fakes stand in for the hub.
pub fn start() {
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("hub-catalog".into()).spawn(|| {
        let first = fetch();
        if first.is_ok() {
            let _ = FETCHED_AT.set(SystemTime::now());
        }
        let failed = first.as_ref().err().cloned();
        let _ = CATALOG.set(first);
        if let Some(why) = failed {
            retry(&why);
        }
        SETTLED.store(true, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        razdor::diag!("hub catalog: the fetch cannot start: {e}");
        SETTLED.store(true, Ordering::SeqCst);
    }
}

/// The bounded retries of a failed first fetch (`RETRY_WAITS`): the first one that reads a catalog is kept
/// in `RETRIED`; the rest are not tried. Each failure is logged.
fn retry(first_why: &str) {
    for (n, wait) in RETRY_WAITS.iter().enumerate() {
        std::thread::sleep(*wait);
        match fetch_catalog() {
            Ok(c) => {
                razdor::diag!("hub catalog: the first fetch failed ({first_why}); retry {} read the fresh catalog", n + 1);
                let _ = RETRIED.set((c, SystemTime::now()));
                return;
            }
            Err(why) => razdor::diag!("hub catalog: retry {} failed: {why}", n + 1),
        }
    }
}

/// The launch's catalog once it has come: None while it is being read.
pub fn catalog() -> Option<&'static Result<Catalog, String>> {
    CATALOG.get()
}

/// The launch's fresh catalog and when it was read: the first fetch's, else a retry's. None while no fresh
/// catalog has come (the first fetch failed or is still running, and no retry has read one).
pub fn fresh() -> Option<(&'static Catalog, SystemTime)> {
    if let (Some(Ok(c)), Some(at)) = (CATALOG.get(), FETCHED_AT.get()) {
        return Some((c, *at));
    }
    RETRIED.get().map(|(c, at)| (c, *at))
}

/// Whether the first fetch and its retries have all ended: no fresh catalog will come after this, unless a
/// snapshot or a launch never started it (then this stays false).
pub fn settled() -> bool {
    SETTLED.load(Ordering::SeqCst)
}

/// Whether the launch's fetch is running now (started, and its catalog not come yet): the main menu's loader.
pub fn fetching() -> bool {
    STARTED.load(Ordering::SeqCst) && CATALOG.get().is_none()
}

fn local() -> Option<std::path::PathBuf> {
    hub::local_dir()
}

fn fetch_catalog() -> Result<Catalog, String> {
    let local = local().ok_or("no folder for the hub")?;
    Hub { source: Source::from_env(), local }.fetch_catalog()
}

fn fetch() -> Result<Catalog, String> {
    let local = local().ok_or("no folder for the hub")?;
    // The zips older installs kept for their mods are not kept any more (backlog 1609 part 2, step 3).
    hub::drop_all_kept_archives(&local);
    Hub { source: Source::from_env(), local }.fetch_catalog()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_retries_are_three_and_bounded_in_time() {
        assert_eq!(RETRY_WAITS.len(), 3, "three retries once per launch, not a loop");
        let total: Duration = RETRY_WAITS.iter().sum();
        assert!(total <= Duration::from_secs(7 * 60), "the whole wait stays within about six minutes (370 s)");
        assert!(RETRY_WAITS.windows(2).all(|w| w[0] < w[1]), "each wait is longer than the one before");
    }
}
