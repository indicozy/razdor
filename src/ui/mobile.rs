//! The phones' not-found screen and its install poll, for Android and iOS (the platform-neutral
//! part; the Android JNI calls are in `ui::android`). The title screen shows a block with
//! «Check again» while no install is found; the player's press, or a poll every
//! [`POLL_SECONDS`] that finds the install in place, loads it without a restart.
//!
//! The storage is the only difference: Android needs the all-files access (see
//! `ui::android::storage_granted`), iOS has no permission at all (the app's Documents folder).

use std::cell::Cell;

/// How often the not-found screen looks for the install while the storage is granted.
const POLL_SECONDS: f64 = 5.0;

thread_local! {
    /// The player asked to look again ("Check again" on the not-found screen).
    static RECHECK: Cell<bool> = const { Cell::new(false) };
    /// The next time the not-found screen polls for the install (macroquad's clock).
    static NEXT_POLL: Cell<f64> = const { Cell::new(0.0) };
}

/// Whether Razdor may read the install's folder: Android's storage access; iOS has no
/// permission (the app's own Documents folder), so it is always granted there.
pub fn storage_granted() -> bool {
    #[cfg(target_os = "android")]
    {
        super::android::storage_granted()
    }
    #[cfg(target_os = "ios")]
    {
        true
    }
}

/// The player pressed "Check again" on the not-found screen.
pub fn ask_recheck() {
    RECHECK.with(|r| r.set(true));
}

/// True once after "Check again" was pressed.
pub fn take_recheck() -> bool {
    RECHECK.with(|r| r.replace(false))
}

/// True at most every [`POLL_SECONDS`]: the not-found screen's own look for the install.
pub fn poll_due(now: f64) -> bool {
    NEXT_POLL.with(|next| {
        if now >= next.get() {
            next.set(now + POLL_SECONDS);
            true
        } else {
            false
        }
    })
}

/// A cheap look for an install (its folder, or a folder of the search roots' children): no
/// deep search, so it can run every few seconds. Finds what the full search finds there.
pub fn install_listed() -> bool {
    razdor::platform::install_roots().iter().any(|root| {
        razdor::dt::install::is_install(root)
            || std::fs::read_dir(root).into_iter().flatten().flatten().any(|e| razdor::dt::install::is_install(&e.path()))
    })
}
