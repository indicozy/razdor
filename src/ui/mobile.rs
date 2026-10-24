//! The phones' not-found screen and its install poll, for Android and iOS (the platform-neutral
//! part; the Android JNI calls are in `ui::android`). The title screen shows a block with
//! «Check again» while no install is found; the player's press, or a poll every
//! [`POLL_SECONDS`] that finds the install in place, loads it without a restart.
//!
//! The storage is the only difference: Android needs the all-files access (see
//! `ui::android::storage_granted`), iOS has no permission at all (the app's Documents folder).

use razdor::local;
use std::cell::Cell;

/// How often the not-found screen looks for the install while the storage is granted.
const POLL_SECONDS: f64 = 5.0;

local! {
    /// The player asked to look again ("Check again" on the not-found screen).
    static RECHECK: Cell<bool> = const { Cell::new(false) };
    /// The next time the not-found screen polls for the install (macroquad's clock).
    static NEXT_POLL: Cell<f64> = const { Cell::new(0.0) };
    /// The screen size the safe area was last read for, and the frames since the last read.
    static INSETS_SEEN: Cell<(f32, f32, u32)> = const { Cell::new((0.0, 0.0, 0)) };
}

/// Frames between two reads of the safe area: the system's insets settle a little after a turn,
/// so they are read again now and then (a few JNI calls, twice a second at 60 fps).
const INSETS_EVERY: u32 = 30;

/// Reads the phone's safe area into `platform::safe_insets` when the screen turns, and every
/// [`INSETS_EVERY`] frames. A turn the system has not laid out yet is read again on the next
/// frame. Called on Android and iOS only (desktop keeps its margins at zero).
pub fn refresh_safe_insets() {
    use macroquad::window::{screen_height, screen_width};
    let (w, h) = (screen_width(), screen_height());
    let (seen_w, seen_h, frames) = INSETS_SEEN.with(|c| c.get());
    let turned = (w, h) != (seen_w, seen_h);
    if !turned && frames < INSETS_EVERY {
        INSETS_SEEN.with(|c| c.set((seen_w, seen_h, frames + 1)));
        return;
    }
    #[cfg(target_os = "android")]
    let read = super::android::read_safe_insets();
    #[cfg(target_os = "ios")]
    let read = Some(razdor::platform::ios_safe_margins(h >= w, macroquad::window::screen_dpi_scale()));
    if let Some(insets) = read {
        razdor::platform::set_safe_insets(insets);
        INSETS_SEEN.with(|c| c.set((w, h, 0)));
    } else {
        // Not laid out yet: the old size is kept, so a turn is read again on the next frame.
        INSETS_SEEN.with(|c| c.set((seen_w, seen_h, 0)));
    }
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

/// The first frame without an install asks for the storage access, once in the run (Android:
/// the all-files settings page, `ui::android::ask_once`). iOS has no permission to ask for.
pub fn ask_access_once() {
    #[cfg(target_os = "android")]
    super::android::ask_once();
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

/// iOS: opens `url` in the browser through `UIApplication`'s `openURL:options:completionHandler:`
/// (no completion handler, no options). A link that does not parse is logged, not sent.
#[cfg(target_os = "ios")]
pub fn open_url(url: &str) {
    use macroquad::miniquad::native::apple::frameworks::{class, msg_send, nil, sel, sel_impl, ObjcId};
    let Ok(text) = std::ffi::CString::new(url) else { return };
    // SAFETY: the Foundation and UIKit classes are the app's own; every object is autoreleased
    // by its class factory, and the strings and the URL live through the call.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
        let ns_url: ObjcId = msg_send![class!(NSURL), URLWithString: ns_string];
        if ns_url == nil {
            razdor::diag!("could not open {url}");
            return;
        }
        let options: ObjcId = msg_send![class!(NSDictionary), dictionary];
        let app: ObjcId = msg_send![class!(UIApplication), sharedApplication];
        let () = msg_send![app, openURL: ns_url options: options completionHandler: nil];
    }
}
