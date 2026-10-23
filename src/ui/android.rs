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

use std::cell::Cell;
use std::ffi::{CStr, CString};

use macroquad::miniquad::native::android::{attach_jni_env, ndk_sys as sys, ACTIVITY};

/// The first release whose all-files access is its own permission page.
const ALL_FILES_API: i32 = 30;
/// The package's own page in the settings (the fallback where the all-files page is missing).
const APP_SETTINGS: &str = "android.settings.APPLICATION_DETAILS_SETTINGS";
const ALL_FILES: &str = "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION";
/// The general list of the apps with all-files access (some vendors lack the package's own page).
const ALL_FILES_LIST: &str = "android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION";
const STORAGE_PERMISSIONS: [&str; 2] = ["android.permission.READ_EXTERNAL_STORAGE", "android.permission.WRITE_EXTERNAL_STORAGE"];

thread_local! {
    /// The not-found screen has looked at the storage access once in this run ([`ask_once`]).
    static ASKED: Cell<bool> = const { Cell::new(false) };
}

/// A local JNI reference, freed when dropped.
struct Local {
    env: *mut sys::JNIEnv,
    obj: sys::jobject,
}

impl Local {
    fn new(env: *mut sys::JNIEnv, obj: sys::jobject) -> Local {
        Local { env, obj }
    }
}

impl Drop for Local {
    fn drop(&mut self) {
        if !self.obj.is_null() {
            // SAFETY: `obj` is a local reference of `env`'s thread, freed once.
            unsafe { ((**self.env).DeleteLocalRef.unwrap())(self.env, self.obj) };
        }
    }
}

/// Clears a Java exception raised by the last call; true when there was one.
unsafe fn cleared(env: *mut sys::JNIEnv) -> bool {
    let pending = ((**env).ExceptionCheck.unwrap())(env) != 0;
    if pending {
        ((**env).ExceptionClear.unwrap())(env);
    }
    pending
}

unsafe fn class(env: *mut sys::JNIEnv, name: &str) -> Local {
    let name = CString::new(name).unwrap();
    Local::new(env, ((**env).FindClass.unwrap())(env, name.as_ptr()))
}

unsafe fn string(env: *mut sys::JNIEnv, text: &str) -> Local {
    let text = CString::new(text).unwrap();
    Local::new(env, ((**env).NewStringUTF.unwrap())(env, text.as_ptr()))
}

unsafe fn method(env: *mut sys::JNIEnv, cls: sys::jclass, name: &str, sig: &str, is_static: bool) -> sys::jmethodID {
    let name = CString::new(name).unwrap();
    let sig = CString::new(sig).unwrap();
    if is_static {
        ((**env).GetStaticMethodID.unwrap())(env, cls, name.as_ptr(), sig.as_ptr())
    } else {
        ((**env).GetMethodID.unwrap())(env, cls, name.as_ptr(), sig.as_ptr())
    }
}

/// The activity's class (its methods are looked up on it).
unsafe fn activity_class(env: *mut sys::JNIEnv) -> Local {
    Local::new(env, ((**env).GetObjectClass.unwrap())(env, ACTIVITY))
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

