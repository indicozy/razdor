//! iOS: the session slot is written when the app goes to the background (`ui::session`). The
//! frame loop stops in the background, so the write cannot wait for a frame. An observer of the
//! system's notifications calls `session::flush_now` on the main thread: when the app enters the
//! background, and when it is about to be terminated. miniquad's own delegate turns the
//! notification of resigning active into the window's pause; the session needs the later one.

use macroquad::miniquad::native::apple::apple_util::str_to_nsstring;
use macroquad::miniquad::native::apple::frameworks::*;

/// The notifications the observer answers, with the iOS names (`UIApplication.h`).
const NOTIFICATIONS: [&str; 2] = ["UIApplicationDidEnterBackgroundNotification", "UIApplicationWillTerminateNotification"];

/// The selector's method: the app's state changed (the notification's object is not used).
extern "C" fn app_state_changed(_this: &Object, _cmd: Sel, _notification: ObjcId) {
    super::session::flush_now();
}

/// Registers the observer once, at start (after the app launched, in `main`): an instance of
/// `RazdorSessionObserver` that receives the notifications above.
pub fn observe_app_state() {
    // SAFETY: plain Objective-C runtime calls on classes that exist on every iOS version Razdor
    // runs on; the class is declared once (this is called once, at start).
    unsafe {
        let Some(mut decl) = ClassDecl::new("RazdorSessionObserver", class!(NSObject)) else { return };
        decl.add_method(sel!(didEnterBackground:), app_state_changed as extern "C" fn(&Object, Sel, ObjcId));
        let class = decl.register();
        // Made with `new` (owned by us) and never released: the observer lives as long as the app.
        let observer: ObjcId = msg_send![class, new];
        let center: ObjcId = msg_send![class!(NSNotificationCenter), defaultCenter];
        for name in NOTIFICATIONS {
            // The name is kept by the notification centre, so it is retained here as well.
            let name: ObjcId = msg_send![str_to_nsstring(name), retain];
            let _: () = msg_send![center, addObserver: observer selector: sel!(didEnterBackground:) name: name object: nil];
        }
    }
}
