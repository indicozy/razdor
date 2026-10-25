//! The phones' side of `update`: their requests (no `curl` there) and, on Android, the install.
//!
//! Android: the app's own `HttpURLConnection` and `PackageInstaller`, called through the JNI
//! (`razdorDownload`, `razdorInstall`, `razdorCacheDir` in `android/MainActivityInject.java`).
//! The install hands the verified file to the system, which asks the player to confirm it.
//! iOS: `NSData dataWithContentsOfURL:`, for the check's small files only (iOS offers the
//! release's page, it installs nothing itself). Both block: they run on the update's threads.

/// Whether `url` is an https address (the only kind Razdor asks for).
#[cfg(any(target_os = "android", target_os = "ios"))]
fn secure(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("not an https address: {url}"))
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::jni::{activity_class, cleared, method, string, to_text, Local};
    use macroquad::miniquad::native::android::{attach_jni_env, ndk_sys as sys, ACTIVITY};

    /// The app's own folder for files that are not kept (`getCacheDir`).
    pub fn cache_dir() -> Result<PathBuf, String> {
        // SAFETY: the update runs once the activity exists; every reference is local.
        unsafe {
            let env = attach_jni_env();
            let mid = method(env, activity_class(env).obj, "razdorCacheDir", "()Ljava/lang/String;", false);
            if mid.is_null() || cleared(env) {
                return Err("razdorCacheDir is missing".into());
            }
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid));
            if cleared(env) {
                return Err("razdorCacheDir failed".into());
            }
            to_text(env, reply.obj).map(PathBuf::from).ok_or_else(|| "no cache folder".into())
        }
    }

    /// Downloads `url` into the file `path` (at most `seconds` without an answer).
    pub fn download(url: &str, path: &Path, seconds: u32) -> Result<(), String> {
        secure(url)?;
        let path = path.to_str().ok_or("the file's path is not text")?;
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorDownload", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorDownload is missing".into());
            }
            let (url, path) = (string(env, url), string(env, path));
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid, url.obj, path.obj, seconds as i32));
            if cleared(env) {
                return Err("the download failed".into());
            }
            reply_or_error(env, reply.obj)
        }
    }

    /// Hands the APK at `path` to the system installer (`razdorInstall`); the file is copied by
    /// the time this returns.
    pub fn install(path: &Path) -> Result<(), String> {
        let path = path.to_str().ok_or("the file's path is not text")?;
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorInstall", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorInstall is missing".into());
            }
            let path = string(env, path);
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid, path.obj));
            if cleared(env) {
                return Err("the installer call failed".into());
            }
            reply_or_error(env, reply.obj)
        }
    }

    /// The Java reply of a call that returns "" when it worked, else the reason.
    ///
    /// # Safety
    /// `env` is the current thread's JNI environment and `reply` a local reference of it.
    unsafe fn reply_or_error(env: *mut sys::JNIEnv, reply: sys::jobject) -> Result<(), String> {
        match to_text(env, reply) {
            Some(text) if text.is_empty() => Ok(()),
            Some(text) => Err(text),
            None => Err("no reply".into()),
        }
    }

    /// The body of `url`, through a file in the cache folder.
    pub fn get(url: &str, seconds: u32) -> Result<Vec<u8>, String> {
        let path = cache_dir()?.join("razdor-fetch");
        let body = download(url, &path, seconds).and_then(|()| std::fs::read(&path).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&path);
        body
    }
}

#[cfg(target_os = "android")]
pub use android::{cache_dir, download, get, install};

#[cfg(target_os = "ios")]
pub fn get(url: &str, _seconds: u32) -> Result<Vec<u8>, String> {
    use macroquad::miniquad::native::apple::frameworks::{class, msg_send, nil, sel, sel_impl, ObjcId};
    use std::ffi::{c_void, CString};
    secure(url)?;
    let text = CString::new(url).map_err(|e| e.to_string())?;
    // SAFETY: the Foundation classes are the app's own; the objects are autoreleased by their
    // class factories and the strings live through the calls.
    unsafe {
        let ns_string: ObjcId = msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()];
        let ns_url: ObjcId = msg_send![class!(NSURL), URLWithString: ns_string];
        if ns_url == nil {
            return Err(format!("not a URL: {url}"));
        }
        let data: ObjcId = msg_send![class!(NSData), dataWithContentsOfURL: ns_url];
        if data == nil {
            return Err("no answer".into());
        }
        let len: usize = msg_send![data, length];
        let bytes: *const c_void = msg_send![data, bytes];
        if bytes.is_null() {
            return Err("an empty answer".into());
        }
        Ok(std::slice::from_raw_parts(bytes as *const u8, len).to_vec())
    }
}
