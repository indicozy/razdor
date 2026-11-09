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

/// Razdor's own (a player's request): opens the folder `dir` in the Files app. Any path under
/// `/storage/emulated/0` gets a document URI (`platform::android_document_uri`), the app's own folder
/// under `Android/data` included. Android 11 and later may refuse that URI in the Files app, and then
/// the call falls back to the storage root as it does for any folder. The document's own URI with the
/// directory type first; if the system refuses it, the storage root. True when one of them started.
/// (A later task replaces these uses with an export.)
pub fn open_folder(dir: &str) -> bool {
    /// `FLAG_GRANT_READ_URI_PERMISSION` | `FLAG_ACTIVITY_NEW_TASK`.
    const FLAGS: i32 = 0x1 | 0x1000_0000;
    let Some(doc) = razdor::platform::android_document_uri(dir) else { return false };
    // SAFETY: as in `open_url`: the activity exists while the menu runs; the locals are freed by `Local`.
    unsafe {
        let env = attach_jni_env();
        if view_document(env, &doc, Some("vnd.android.document/directory"), FLAGS) {
            return true;
        }
        let root = "content://com.android.externalstorage.documents/root/primary";
        view_document(env, root, None, FLAGS)
    }
}

/// An `ACTION_VIEW` intent over the content URI `uri` (with its MIME type when given), with `flags`
/// added, started from the activity. False when a call failed (the Java exception is cleared).
unsafe fn view_document(env: *mut sys::JNIEnv, uri: &str, mime: Option<&str>, flags: i32) -> bool {
    let uri_cls = class(env, "android/net/Uri");
    let parse = method(env, uri_cls.obj, "parse", "(Ljava/lang/String;)Landroid/net/Uri;", true);
    if parse.is_null() || cleared(env) {
        return false;
    }
    let text = string(env, uri);
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
    let mut intent = Local::new(env, ((**env).NewObject.unwrap())(env, intent_cls.obj, ctor, action.obj, uri.obj));
    if intent.obj.is_null() || cleared(env) {
        return false;
    }
    if let Some(mime) = mime {
        let mime = string(env, mime);
        let set = method(env, intent_cls.obj, "setDataAndType", "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;", false);
        intent = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, intent.obj, set, uri.obj, mime.obj));
        if intent.obj.is_null() || cleared(env) {
            return false;
        }
    }
    let add = method(env, intent_cls.obj, "addFlags", "(I)Landroid/content/Intent;", false);
    intent = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, intent.obj, add, flags));
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

/// The app's own folder: `getExternalFilesDir(null)`, or `getFilesDir()` without an external volume
/// (`razdorFilesDir`, MainActivityInject.java). `None` when the call fails; `platform` keeps the answer.
pub fn files_dir() -> Option<std::path::PathBuf> {
    // SAFETY: as in `text_call`.
    unsafe { text_call("razdorFilesDir") }.filter(|path| !path.is_empty()).map(std::path::PathBuf::from)
}

/// A call of the activity that takes no argument and returns text (`razdor::jni::to_text`), or
/// `None` when the call is missing or refused.
///
/// # Safety
/// Called from the game's thread; every reference made here is local and freed by `Local`.
unsafe fn text_call(name: &str) -> Option<String> {
    let env = attach_jni_env();
    let mid = method(env, activity_class(env).obj, name, "()Ljava/lang/String;", false);
    if mid.is_null() || cleared(env) {
        return None;
    }
    let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid));
    if cleared(env) {
        return None;
    }
    razdor::jni::to_text(env, reply.obj)
}

/// Opens the system's document picker for a map (`razdorPickFile`, the Java side). False when the
/// call fails; the answer comes through [`pick_outcome`].
pub fn start_pick() -> bool {
    start_picker_call("razdorPickFile")
}

/// Opens the system's document picker for a game archive (`razdorPickArchive`): the answer is a
/// descriptor, read in place (see [`pick_outcome`]). False when the call fails.
pub fn start_archive_pick() -> bool {
    start_picker_call("razdorPickArchive")
}

fn start_picker_call(name: &str) -> bool {
    // SAFETY: as in `text_call`.
    unsafe { text_call(name) }.is_some_and(|reply| reply.is_empty())
}

/// The picker's answer (`razdorPickOutcome`): "" while it is open, "cancel", "error: <why>", or
/// "ok\n<display name>\n<path of a copy in the app's cache>" for a map (the caller deletes the copy),
/// or "fd\n<display name>\n<descriptor>" for an archive (the caller takes ownership of it).
pub fn pick_outcome() -> String {
    // SAFETY: as in `text_call`.
    unsafe { text_call("razdorPickOutcome") }.unwrap_or_default()
}

/// Whether Razdor may read the shared storage (the legacy folders). Nothing waits for it now: a
/// new player's install is in the app's own folder.
#[allow(dead_code)]
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
/// No screen calls it now (kept: the settings page, for a player who wants to grant it by hand).
#[allow(dead_code)]
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

local! {
    /// The orientation last asked of the system (an `ORIENTATION_*`), none before the first call.
    static ORIENTATION: Cell<Option<i32>> = const { Cell::new(None) };
}

local! {
    /// The input subscriber that watches the activity's resume (see [`resumed_since_last`]).
    static RESUME_SUB: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Watches the window's restore: miniquad reports the activity's Resume as `window_restored_event`.
struct ResumeWatch(bool);

impl macroquad::miniquad::EventHandler for ResumeWatch {
    fn update(&mut self) {}
    fn draw(&mut self) {}
    fn window_restored_event(&mut self) {
        self.0 = true;
    }
}

/// Whether the activity resumed since the last call (its own input subscriber, read each frame:
/// the pointer's watch does not read it on a phone, where the gestures draw no pointer).
fn resumed_since_last() -> bool {
    use macroquad::input::utils::{register_input_subscriber, repeat_all_miniquad_input};
    let sub = RESUME_SUB.with(|r| r.get()).unwrap_or_else(register_input_subscriber);
    let mut watch = ResumeWatch(false);
    repeat_all_miniquad_input(&mut watch, sub);
    RESUME_SUB.with(|r| r.set(Some(sub)));
    watch.0
}

/// Forgets the orientation last asked, so the next [`follow_orientation`] asks it again: the
/// system may have put the screen back in its own orientation while the app was away.
pub fn orientation_stale() {
    ORIENTATION.with(|o| o.set(None));
}

/// Puts the setting's orientation on the activity when it has changed: at start, each time
/// the settings' row changes it (`setRequestedOrientation`; the system turns the screen then), and
/// again after the activity resumed (the cache is reset then, so a minimised app comes back in the setting's
/// orientation, not the system's). Called from the UI's frame, which runs on the activity's own thread as the other calls here.
pub fn follow_orientation(setting: Option<super::audio::Orientation>) {
    if resumed_since_last() {
        orientation_stale();
    }
    let want = super::audio::requested_orientation(setting);
    if !super::audio::must_request(ORIENTATION.with(|o| o.get()), want) {
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

/// Moves the app to the background at once, as the game quits (`main::exit_now`, first of all:
/// the saves are written off the screen). The activity's `razdorHideForExit` does it on its UI
/// thread (`moveTaskToBack`).
pub fn hide_for_exit() {
    // SAFETY: as in `set_immersive`; the activity exists while the game quits.
    unsafe {
        let env = attach_jni_env();
        let hide = method(env, activity_class(env).obj, "razdorHideForExit", "()V", false);
        if hide.is_null() || cleared(env) {
            razdor::diag!("razdorHideForExit is missing");
            return;
        }
        ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, hide);
        cleared(env);
    }
}

/// The margin the rounded corners and the bars keep at least, in dp: the corners of a phone's
/// screen are not reported by every system, so the interface keeps this much from each edge.
const CORNER_DP: f32 = 24.0;

/// The system's language as a BCP 47 tag, `java.util.Locale.getDefault().toLanguageTag()` (e.g.
/// "ru-RU"); None when the call fails or gives nothing. Read through the JNI and not through
/// sys-locale's system properties: an app may not read `persist.sys.locale` (SELinux), and some
/// global ROMs set `ro.product.locale` to en-US whatever the player's language is.
pub fn read_locale() -> Option<String> {
    // SAFETY: as in `density`; every reference made here is local and freed by `Local`.
    unsafe { locale_tag(attach_jni_env()) }
}

/// See [`read_locale`]. Every failed call clears its Java exception and frees its references.
///
/// # Safety
/// `env` is the JNI environment of the current thread.
unsafe fn locale_tag(env: *mut sys::JNIEnv) -> Option<String> {
    let locale_cls = class(env, "java/util/Locale");
    if locale_cls.obj.is_null() || cleared(env) {
        return None;
    }
    let get_default = method(env, locale_cls.obj, "getDefault", "()Ljava/util/Locale;", true);
    if get_default.is_null() || cleared(env) {
        return None;
    }
    let locale = Local::new(env, ((**env).CallStaticObjectMethod.unwrap())(env, locale_cls.obj, get_default));
    if locale.obj.is_null() || cleared(env) {
        return None;
    }
    let tag_m = method(env, locale_cls.obj, "toLanguageTag", "()Ljava/lang/String;", false);
    if tag_m.is_null() || cleared(env) {
        return None;
    }
    let tag = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, locale.obj, tag_m));
    if cleared(env) {
        return None;
    }
    razdor::jni::to_text(env, tag.obj).filter(|t| !t.trim().is_empty())
}

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
/// The activity window's root insets (`WindowInsets` of the decor view); None before the first layout.
unsafe fn root_window_insets(env: *mut sys::JNIEnv) -> Option<Local> {
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
    Some(insets)
}

/// `WindowInsets.getInsets(type)` as a rect (left, top, right, bottom) in physical px; API 30+.
unsafe fn insets_of_type(env: *mut sys::JNIEnv, insets: &Local, type_mask: i32) -> Option<[i32; 4]> {
    let insets_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, insets.obj));
    let get_insets = method(env, insets_cls.obj, "getInsets", "(I)Landroid/graphics/Insets;", false);
    if get_insets.is_null() || cleared(env) {
        return None;
    }
    let rect = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, insets.obj, get_insets, type_mask));
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
}

/// The on-screen keyboard's height in physical px, from the bottom edge: the IME inset
/// (`WindowInsets.Type.ime()`, API 30+; 0 before that, where the keyboard is not measured, so
/// the windows do not move). 0 while it is hidden. None before the window's first layout.
pub fn read_keyboard_inset() -> Option<f32> {
    // SAFETY: as in `read_safe_insets`; every reference made below is local and freed by `Local`.
    unsafe {
        let env = attach_jni_env();
        let insets = root_window_insets(env)?;
        if sdk_int(env) < 30 {
            return Some(0.0);
        }
        let type_cls = class(env, "android/view/WindowInsets$Type");
        let ime_m = method(env, type_cls.obj, "ime", "()I", true);
        if ime_m.is_null() || cleared(env) {
            return Some(0.0);
        }
        let ime = ((**env).CallStaticIntMethod.unwrap())(env, type_cls.obj, ime_m);
        Some(insets_of_type(env, &insets, ime).map_or(0.0, |[_, _, _, bottom]| bottom.max(0) as f32))
    }
}

unsafe fn system_insets(env: *mut sys::JNIEnv) -> Option<[i32; 4]> {
    let insets = root_window_insets(env)?;
    if sdk_int(env) >= 30 {
        let type_cls = class(env, "android/view/WindowInsets$Type");
        let bars_m = method(env, type_cls.obj, "systemBars", "()I", true);
        let cutout_m = method(env, type_cls.obj, "displayCutout", "()I", true);
        if bars_m.is_null() || cutout_m.is_null() || cleared(env) {
            return None;
        }
        let bars = ((**env).CallStaticIntMethod.unwrap())(env, type_cls.obj, bars_m);
        let cutout = ((**env).CallStaticIntMethod.unwrap())(env, type_cls.obj, cutout_m);
        insets_of_type(env, &insets, bars | cutout)
    } else {
        let insets_cls = Local::new(env, ((**env).GetObjectClass.unwrap())(env, insets.obj));
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

/// The phone's safe area from the activity's `razdorSafeArea` (physical px: left, top, right,
/// bottom, then the bottom-left, bottom-right, top-left and top-right corners' radii, -1 where the
/// system does not report them; see `display::safe_area_from_system`). None while the window has
/// no insets yet.
pub fn read_safe_area() -> Option<[f32; 8]> {
    // SAFETY: as in `read_safe_insets`; every reference made below is local and freed by `Local`.
    unsafe {
        let env = attach_jni_env();
        let get = method(env, activity_class(env).obj, "razdorSafeArea", "()Ljava/lang/String;", false);
        if get.is_null() || cleared(env) {
            return None;
        }
        let text = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, get));
        if text.obj.is_null() || cleared(env) {
            return None;
        }
        let chars = ((**env).GetStringUTFChars.unwrap())(env, text.obj, std::ptr::null_mut());
        if chars.is_null() {
            return None;
        }
        let read = CStr::from_ptr(chars).to_string_lossy().into_owned();
        ((**env).ReleaseStringUTFChars.unwrap())(env, text.obj, chars);
        let values: Vec<f32> = read.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        <[f32; 8]>::try_from(values).ok()
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

/// `MainActivity.razdorOnPause` (the native method of `android/MainActivityInject.java`, called
/// from `onPause`). The app is going to the background, and the frame loop does not run there:
/// the newest state of the session is written now, on the activity's UI thread, not on a frame.
/// Declared in the Java inject; the name is the JNI name of the class in `Cargo.toml`'s package.
#[no_mangle]
pub extern "C" fn Java_io_github_indicozy_razdor_MainActivity_razdorOnPause(_env: *mut sys::JNIEnv, _class: sys::jclass) {
    super::session::flush_now();
}

/// Keeps the screen awake (`FLAG_KEEP_SCREEN_ON` on the activity's window, set and cleared on its UI thread by
/// `razdorKeepAwake`): the performance test holds it for its run, so the screen does not dim mid-measure.
pub fn keep_awake(on: bool) {
    // SAFETY: as in `set_immersive`; the activity exists once the frames run.
    unsafe {
        let env = attach_jni_env();
        let keep = method(env, activity_class(env).obj, "razdorKeepAwake", "(Z)V", false);
        if keep.is_null() || cleared(env) {
            razdor::diag!("razdorKeepAwake is missing");
            return;
        }
        // A jboolean is passed as an int through the variadic call (Rust does not take bool there).
        ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, keep, on as i32);
        cleared(env);
    }
}
