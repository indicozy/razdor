//! Android: the storage permission, and the settings page where the player grants it, through
//! the JNI of miniquad's activity (miniquad exposes the activity and the JNI environment, not
//! these calls). The not-found screen's look for the install is platform-neutral (`ui::mobile`).
//! Only what Razdor needs. Every call that the Java side may refuse clears its
//! pending exception, so a refused call never ends the process; the local references it makes
//! are freed as they go (a long session polls this every few seconds).
//!
//! Android 11 (API 30) and later: "All files access" (`MANAGE_EXTERNAL_STORAGE`), granted on
//! its own settings page. Android 8 to 10: the runtime permissions `READ_EXTERNAL_STORAGE` and
//! `WRITE_EXTERNAL_STORAGE`, asked with a dialog.

use razdor::local;
use std::cell::Cell;
use std::ffi::{CStr, CString};

use macroquad::miniquad::native::android::{attach_jni_env, ndk_sys as sys, ACTIVITY};
use razdor::jni::{activity_class, cleared, class, method, string, Local};

/// The first release whose all-files access is its own permission page.
const ALL_FILES_API: i32 = 30;
/// The package's own page in the settings (the fallback where the all-files page is missing).
const APP_SETTINGS: &str = "android.settings.APPLICATION_DETAILS_SETTINGS";
const ALL_FILES: &str = "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION";
/// The general list of the apps with all-files access (some vendors lack the package's own page).
const ALL_FILES_LIST: &str = "android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION";
const STORAGE_PERMISSIONS: [&str; 2] = ["android.permission.READ_EXTERNAL_STORAGE", "android.permission.WRITE_EXTERNAL_STORAGE"];

local! {
    /// The not-found screen has looked at the storage access once in this run ([`ask_once`]).
    static ASKED: Cell<bool> = const { Cell::new(false) };
}

/// `Build.VERSION.SDK_INT` (0 when it cannot be read).
unsafe fn sdk_int(env: *mut sys::JNIEnv) -> i32 {
    let cls = class(env, "android/os/Build$VERSION");
    if cls.obj.is_null() {
        cleared(env);
        return 0;
    }
    let name = CString::new("SDK_INT").unwrap();
    let sig = CString::new("I").unwrap();
    let field = ((**env).GetStaticFieldID.unwrap())(env, cls.obj, name.as_ptr(), sig.as_ptr());
    if field.is_null() || cleared(env) {
        return 0;
    }
    ((**env).GetStaticIntField.unwrap())(env, cls.obj, field)
}

/// The package's name, e.g. `io.github.indicozy.razdor`.
unsafe fn package_name(env: *mut sys::JNIEnv) -> Option<String> {
    let mid = method(env, activity_class(env).obj, "getPackageName", "()Ljava/lang/String;", false);
    if mid.is_null() || cleared(env) {
        return None;
    }
    let name = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid));
    if name.obj.is_null() {
        return None;
    }
    let chars = ((**env).GetStringUTFChars.unwrap())(env, name.obj, std::ptr::null_mut());
    let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
    ((**env).ReleaseStringUTFChars.unwrap())(env, name.obj, chars);
    Some(text)
}

/// Starts an activity for `action` on the package's own data (`package:<name>`) when `with_package`.
unsafe fn start_settings(env: *mut sys::JNIEnv, action: &str, with_package: bool) -> bool {
    let intent_cls = class(env, "android/content/Intent");
    let action = string(env, action);
    let ctor = method(env, intent_cls.obj, "<init>", "(Ljava/lang/String;)V", false);
    if ctor.is_null() || cleared(env) {
        return false;
    }
    let mut intent = Local::new(env, ((**env).NewObject.unwrap())(env, intent_cls.obj, ctor, action.obj));
    if intent.obj.is_null() || cleared(env) {
        return false;
    }
    if with_package {
        let Some(pkg) = package_name(env) else { return false };
        let uri_cls = class(env, "android/net/Uri");
        let parse = method(env, uri_cls.obj, "parse", "(Ljava/lang/String;)Landroid/net/Uri;", true);
        let text = string(env, &format!("package:{pkg}"));
        let uri = Local::new(env, ((**env).CallStaticObjectMethod.unwrap())(env, uri_cls.obj, parse, text.obj));
        if uri.obj.is_null() || cleared(env) {
            return false;
        }
        let set_data = method(env, intent_cls.obj, "setData", "(Landroid/net/Uri;)Landroid/content/Intent;", false);
        intent = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, intent.obj, set_data, uri.obj));
        if intent.obj.is_null() || cleared(env) {
            return false;
        }
    }
    let start = method(env, activity_class(env).obj, "startActivity", "(Landroid/content/Intent;)V", false);
    ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, start, intent.obj);
    !cleared(env)
}

/// Opens `url` in the browser (or the app that handles it): an `ACTION_VIEW` intent over
/// `Uri.parse(url)`, started from the activity. A link no app takes is logged, not raised.
pub fn open_url(url: &str) {
    // SAFETY: the activity exists once the window is open (the menu runs after it); every
    // reference made below is a local one, freed by `Local` when it goes out of scope.
    unsafe {
        let env = attach_jni_env();
        if !view_url(env, url) {
            razdor::diag!("could not open {url}");
        }
    }
}

/// The intent of [`open_url`]: `new Intent("android.intent.action.VIEW", Uri.parse(url))`, then
/// `startActivity`. False when a call failed (the Java exception is cleared).
unsafe fn view_url(env: *mut sys::JNIEnv, url: &str) -> bool {
    let uri_cls = class(env, "android/net/Uri");
    let parse = method(env, uri_cls.obj, "parse", "(Ljava/lang/String;)Landroid/net/Uri;", true);
    if parse.is_null() || cleared(env) {
        return false;
    }
    let text = string(env, url);
    let uri = Local::new(env, ((**env).CallStaticObjectMethod.unwrap())(env, uri_cls.obj, parse, text.obj));
    if uri.obj.is_null() || cleared(env) {
        return false;
    }
    let intent_cls = class(env, "android/content/Intent");
    let action = string(env, "android.intent.action.VIEW");
    let ctor = method(env, intent_cls.obj, "<init>", "(Ljava/lang/String;Landroid/net/Uri;)V", false);
    if ctor.is_null() || cleared(env) {
        return false;
    }
    let intent = Local::new(env, ((**env).NewObject.unwrap())(env, intent_cls.obj, ctor, action.obj, uri.obj));
    if intent.obj.is_null() || cleared(env) {
        return false;
    }
    let start = method(env, activity_class(env).obj, "startActivity", "(Landroid/content/Intent;)V", false);
    ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, start, intent.obj);
    !cleared(env)
}

/// The app's private cache folder: `Context.getCacheDir()` (the picture cache lives there, out
/// of the shared storage the Files app indexes). `None` when the call fails. Called from any
/// thread (the JNI attaches it); `platform::cache_dir` keeps the answer.
pub fn private_cache_dir() -> Option<std::path::PathBuf> {
    // SAFETY: as in `storage_granted`; every reference made below is local and freed by `Local`.
    unsafe {
        let env = attach_jni_env();
        let get_dir = method(env, activity_class(env).obj, "getCacheDir", "()Ljava/io/File;", false);
        if get_dir.is_null() || cleared(env) {
            return None;
        }
        let dir = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, get_dir));
        if dir.obj.is_null() || cleared(env) {
            return None;
        }
        let file_cls = class(env, "java/io/File");
        let get_path = method(env, file_cls.obj, "getAbsolutePath", "()Ljava/lang/String;", false);
        if get_path.is_null() || cleared(env) {
            return None;
        }
        let text = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, dir.obj, get_path));
        if text.obj.is_null() || cleared(env) {
            return None;
        }
        let chars = ((**env).GetStringUTFChars.unwrap())(env, text.obj, std::ptr::null_mut());
        if chars.is_null() {
            return None;
        }
        let path = CStr::from_ptr(chars).to_string_lossy().into_owned();
        ((**env).ReleaseStringUTFChars.unwrap())(env, text.obj, chars);
        Some(std::path::PathBuf::from(path))
    }
}

/// Whether Razdor may read the shared storage (the install and the saves live there).
pub fn storage_granted() -> bool {
    // SAFETY: the activity exists once the window is open (the screens run after it).
    unsafe {
        let env = attach_jni_env();
        if sdk_int(env) >= ALL_FILES_API {
            let cls = class(env, "android/os/Environment");
            let is_manager = method(env, cls.obj, "isExternalStorageManager", "()Z", true);
            if is_manager.is_null() || cleared(env) {
                return false;
            }
            let granted = ((**env).CallStaticBooleanMethod.unwrap())(env, cls.obj, is_manager) != 0;
            !cleared(env) && granted
        } else {
            let check = method(env, activity_class(env).obj, "checkSelfPermission", "(Ljava/lang/String;)I", false);
            STORAGE_PERMISSIONS.iter().all(|perm| {
                let text = string(env, perm);
                let granted = ((**env).CallIntMethod.unwrap())(env, ACTIVITY, check, text.obj) == 0;
                !cleared(env) && granted
            })
        }
    }
}

/// Asks for the storage: the all-files page of the settings (Android 11 and later), or the
/// two runtime permissions' dialog (Android 8 to 10). Returns at once; the player answers there.
pub fn request_storage() {
    // SAFETY: as in `storage_granted`.
    unsafe {
        let env = attach_jni_env();
        if sdk_int(env) >= ALL_FILES_API {
            // The package's own page, then the general list, then the app's page.
            if !start_settings(env, ALL_FILES, true) && !start_settings(env, ALL_FILES_LIST, false) {
                start_settings(env, APP_SETTINGS, true);
            }
        } else {
            let string_cls = class(env, "java/lang/String");
            let perms = Local::new(env, ((**env).NewObjectArray.unwrap())(env, STORAGE_PERMISSIONS.len() as i32, string_cls.obj, std::ptr::null_mut()));
            if perms.obj.is_null() || cleared(env) {
                return;
            }
            for (i, perm) in STORAGE_PERMISSIONS.iter().enumerate() {
                let text = string(env, perm);
                ((**env).SetObjectArrayElement.unwrap())(env, perms.obj, i as i32, text.obj);
            }
            let ask = method(env, activity_class(env).obj, "requestPermissions", "([Ljava/lang/String;I)V", false);
            ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, ask, perms.obj, 1i32);
            cleared(env);
        }
    }
}

/// Asks for the storage the first time the not-found screen is shown in this run, when it is
/// not granted yet, so the player lands on the access page without finding the button. Later
/// calls do nothing.
pub fn ask_once() {
    if ASKED.with(|a| a.replace(true)) {
        return;
    }
    if !storage_granted() {
        request_storage();
    }
}

/// `ActivityInfo`'s screen orientations: the system's full user rotation (the manifest's
/// `fullUser`, Auto), the sensor's landscape and the sensor's portrait.
const ORIENTATION_AUTO: i32 = 13;
const ORIENTATION_LANDSCAPE: i32 = 6;
const ORIENTATION_PORTRAIT: i32 = 7;

local! {
    /// The orientation last asked of the system (an `ORIENTATION_*`), none before the first call.
    static ORIENTATION: Cell<Option<i32>> = const { Cell::new(None) };
}

/// Puts the setting's orientation on the activity when it has changed: at start, and each time
/// the settings' row changes it (`setRequestedOrientation`; the system turns the screen then).
/// Called from the UI's frame, which runs on the activity's own thread as the other calls here.
pub fn follow_orientation(setting: Option<super::audio::Orientation>) {
    use super::audio::Orientation;
    let want = match setting {
        None => ORIENTATION_AUTO,
        Some(Orientation::Landscape) => ORIENTATION_LANDSCAPE,
        Some(Orientation::Portrait) => ORIENTATION_PORTRAIT,
    };
    if ORIENTATION.with(|o| o.get()) == Some(want) {
        return;
    }
    ORIENTATION.with(|o| o.set(Some(want)));
    // SAFETY: as in `storage_granted`; the activity exists once the frames run.
    unsafe {
        let env = attach_jni_env();
        let set = method(env, activity_class(env).obj, "setRequestedOrientation", "(I)V", false);
        if set.is_null() || cleared(env) {
            return;
        }
        ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, set, want);
        cleared(env);
    }
}

/// Hides the system bars (`on`: full screen) or shows them (`razdorSetImmersive`, which the
/// activity applies on its UI thread and again when the window regains the focus). Called from
/// the display's frame, like the orientation.
pub fn set_immersive(on: bool) {
    // SAFETY: as in `follow_orientation`; the activity exists once the frames run.
    unsafe {
        let env = attach_jni_env();
        let set = method(env, activity_class(env).obj, "razdorSetImmersive", "(Z)V", false);
        if set.is_null() || cleared(env) {
            razdor::diag!("razdorSetImmersive is missing");
            return;
        }
        // A jboolean is passed as an int through the variadic call (Rust does not take bool there).
        ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, set, on as i32);
        cleared(env);
    }
}

/// The margin the rounded corners and the bars keep at least, in dp: the corners of a phone's
/// screen are not reported by every system, so the interface keeps this much from each edge.
const CORNER_DP: f32 = 24.0;

/// `Resources.getDisplayMetrics().density` (2.0 when it cannot be read).
unsafe fn density(env: *mut sys::JNIEnv) -> f32 {
    let get = method(env, activity_class(env).obj, "getResources", "()Landroid/content/res/Resources;", false);
    if get.is_null() || cleared(env) {
        return 2.0;
    }
    let res = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, get));
    if res.obj.is_null() || cleared(env) {
        return 2.0;
    }
    let res_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, res.obj));
    let metrics_m = method(env, res_cls.obj, "getDisplayMetrics", "()Landroid/util/DisplayMetrics;", false);
    if metrics_m.is_null() || cleared(env) {
        return 2.0;
    }
    let metrics = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, res.obj, metrics_m));
    if metrics.obj.is_null() || cleared(env) {
        return 2.0;
    }
    let metrics_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, metrics.obj));
    let name = CString::new("density").unwrap();
    let sig = CString::new("F").unwrap();
    let field = ((**env).GetFieldID.unwrap())(env, metrics_cls.obj, name.as_ptr(), sig.as_ptr());
    if field.is_null() || cleared(env) {
        return 2.0;
    }
    let value = ((**env).GetFloatField.unwrap())(env, metrics.obj, field);
    if cleared(env) || value <= 0.0 { 2.0 } else { value }
}

/// The system bars and the display cutout of the activity's window, in screen pixels (left,
/// top, right, bottom): `getRootWindowInsets()` on the decor view, then `getInsets` with the
/// system bars and the cutout on API 30 and later, the system window insets before that. None
/// while the window has no insets yet (before its first layout).
unsafe fn system_insets(env: *mut sys::JNIEnv) -> Option<[i32; 4]> {
    let get_window = method(env, activity_class(env).obj, "getWindow", "()Landroid/view/Window;", false);
    if get_window.is_null() || cleared(env) {
        return None;
    }
    let window = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, get_window));
    if window.obj.is_null() || cleared(env) {
        return None;
    }
    let window_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, window.obj));
    let get_decor = method(env, window_cls.obj, "getDecorView", "()Landroid/view/View;", false);
    if get_decor.is_null() || cleared(env) {
        return None;
    }
    let decor = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, window.obj, get_decor));
    if decor.obj.is_null() || cleared(env) {
        return None;
    }
    let decor_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, decor.obj));
    let root = method(env, decor_cls.obj, "getRootWindowInsets", "()Landroid/view/WindowInsets;", false);
    if root.is_null() || cleared(env) {
        return None;
    }
    let insets = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, decor.obj, root));
    if insets.obj.is_null() || cleared(env) {
        return None;
    }
    let insets_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, insets.obj));
    if sdk_int(env) >= 30 {
        let type_cls = class(env, "android/view/WindowInsets$Type");
        let bars_m = method(env, type_cls.obj, "systemBars", "()I", true);
        let cutout_m = method(env, type_cls.obj, "displayCutout", "()I", true);
        if bars_m.is_null() || cutout_m.is_null() || cleared(env) {
            return None;
        }
        let bars = ((**env).CallStaticIntMethod.unwrap())(env, type_cls.obj, bars_m);
        let cutout = ((**env).CallStaticIntMethod.unwrap())(env, type_cls.obj, cutout_m);
        let get_insets = method(env, insets_cls.obj, "getInsets", "(I)Landroid/graphics/Insets;", false);
        if get_insets.is_null() || cleared(env) {
            return None;
        }
        let rect = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, insets.obj, get_insets, bars | cutout));
        if rect.obj.is_null() || cleared(env) {
            return None;
        }
        let rect_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, rect.obj));
        let field = |name: &str| -> i32 {
            let name = CString::new(name).unwrap();
            let sig = CString::new("I").unwrap();
            let id = ((**env).GetFieldID.unwrap())(env, rect_cls.obj, name.as_ptr(), sig.as_ptr());
            if id.is_null() || cleared(env) {
                return 0;
            }
            ((**env).GetIntField.unwrap())(env, rect.obj, id)
        };
        Some([field("left"), field("top"), field("right"), field("bottom")])
    } else {
        let int_of = |name: &str| -> i32 {
            let m = method(env, insets_cls.obj, name, "()I", false);
            if m.is_null() || cleared(env) {
                return 0;
            }
            let v = ((**env).CallIntMethod.unwrap())(env, insets.obj, m);
            if cleared(env) { 0 } else { v }
        };
        Some([
            int_of("getSystemWindowInsetLeft"),
            int_of("getSystemWindowInsetTop"),
            int_of("getSystemWindowInsetRight"),
            int_of("getSystemWindowInsetBottom"),
        ])
    }
}

/// The system's density (screen px per dp, Razdor's phone layout reads it once: `display`).
pub fn read_density() -> f32 {
    // SAFETY: the activity exists once the window is open (the UI runs after it).
    unsafe { density(attach_jni_env()) }
}

/// The phone's safe margins in screen pixels (left, top, right, bottom): the system's insets,
/// never less than [`CORNER_DP`] from each edge. None when the window has no insets yet.
pub fn read_safe_insets() -> Option<(f32, f32, f32, f32)> {
    // SAFETY: the activity exists once the window is open (the UI runs after it).
    unsafe {
        let env = attach_jni_env();
        let [l, t, r, b] = system_insets(env)?;
        let corner = (CORNER_DP * density(env)).round();
        Some(razdor::platform::safe_margins((l as f32, t as f32, r as f32, b as f32), corner))
    }
}

