//! The hub's catalog, read once per launch: the main menu's promo ad (`ui::ads::sync_tick`) and the
//! game check with no install (`ui::game_import`) both read this one result, so a launch makes one
//! fetch. It starts at the first frame of the app (`start`), on a thread, and is never read again
//! from the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use razdor::hub::{self, Catalog, Hub, Source};

/// The launch's catalog as it came: the catalog, or why it could not be read.
static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

/// Starts the launch's fetch on a thread (once). Not in a snapshot: its fakes stand in for the hub.
pub fn start() {
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() || STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("hub-catalog".into()).spawn(|| {
        let _ = CATALOG.set(fetch());
    });
    if let Err(e) = spawned {
        razdor::diag!("hub catalog: the fetch cannot start: {e}");
    }
}

/// The launch's catalog once it has come: None while it is being read.
pub fn catalog() -> Option<&'static Result<Catalog, String>> {
    CATALOG.get()
}

fn fetch() -> Result<Catalog, String> {
    let local = hub::local_dir().ok_or("no folder for the hub")?;
    Hub { source: Source::from_env(), local }.fetch_catalog()
}
