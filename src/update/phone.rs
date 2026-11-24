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
    use crate::parts::RangeReply;
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

    /// Downloads `url` into the file `path`: it stops after `STALL_SECONDS` without a byte (a
    /// stall fails with `STALLED`, its reason in the log) and after `seconds` in all. Blocks; a
    /// cancel from another thread (`download_cancel`) makes it return "cancelled".
    pub fn download(url: &str, path: &Path, seconds: u32) -> Result<(), String> {
        crate::mirror::wait_host(url, &|| false);
        let text = java_download(url, path, seconds, -1, -1)?;
        if text.is_empty() {
            crate::mirror::answered(url);
            return Ok(());
        }
        if let Some((code, retry_after)) = crate::mirror::http_reply(&text) {
            crate::mirror::note_failure(url, &format!("HTTP {code}"), retry_after);
        }
        Err(failure(&text))
    }

    /// Downloads bytes `start..=end` of `url` into the file `path` (a piece of a download, see
    /// `parts`). Blocks like `download`. The server's answer: a 206 with the file's size, or a 200
    /// (the range was ignored: nothing was written).
    pub fn download_range(url: &str, path: &Path, start: u64, end: u64, seconds: u32) -> Result<RangeReply, String> {
        crate::mirror::wait_host(url, &|| false);
        let text = java_download(url, path, seconds, start as i64, end as i64)?;
        if text == "200" {
            return Ok(RangeReply { status: 200, total: None, retry_after: None });
        }
        if let Some(total) = text.strip_prefix("206 ") {
            return Ok(RangeReply { status: 206, total: total.trim().parse::<i64>().ok().filter(|t| *t >= 0).map(|t| t as u64), retry_after: None });
        }
        // A refused status comes back as the reply itself, with its Retry-After: parts decides what it means.
        if let Some((code, retry_after)) = crate::mirror::http_reply(&text) {
            return Ok(RangeReply { status: code, total: None, retry_after });
        }
        Err(failure(&text))
    }

    /// The reason of a download that failed with the Java side's reply `text`: a stall is
    /// `STALLED` (its own words go to the log), anything else is the text itself.
    fn failure(text: &str) -> String {
        if let Some((code, _)) = crate::mirror::http_reply(text) {
            return format!("HTTP {code}");
        }
        if text.starts_with("stalled") {
            crate::diag!("download: {text}");
            return crate::update::STALLED.into();
        }
        if text.is_empty() { "no reply".into() } else { text.to_string() }
    }

    /// The Java side's `razdorDownload` for `url` into `path`, the whole file (`start` and `end`
    /// -1) or the bytes `start..=end`. Its reply text, as the Java side gives it (see there).
    fn java_download(url: &str, path: &Path, seconds: u32, start: i64, end: i64) -> Result<String, String> {
        secure(url)?;
        let path = path.to_str().ok_or("the file's path is not text")?;
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;Ljava/lang/String;IIJJ)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorDownload", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorDownload is missing".into());
            }
            let stall = crate::mirror::stall_seconds(url) as i32;
            let (url, path) = (string(env, url), string(env, path));
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid, url.obj, path.obj, stall, seconds as i32, start, end));
            if cleared(env) {
                return Err("the download failed".into());
            }
            to_text(env, reply.obj).ok_or_else(|| "no reply".into())
        }
    }

    /// Asks the download of the file `path` to stop (`razdorDownloadCancelPath`): it returns
    /// "cancelled" at once, closing its connection if it is waiting for a byte. The other downloads
    /// under way (the pieces and hedges of a file) run on.
    pub fn download_cancel(path: &Path) {
        let Some(path) = path.to_str() else { return };
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;)V";
            let mid = method(env, activity_class(env).obj, "razdorDownloadCancelPath", sig, false);
            if mid.is_null() || cleared(env) {
                return;
            }
            let path = string(env, path);
            ((**env).CallVoidMethod.unwrap())(env, ACTIVITY, mid, path.obj);
            let _ = cleared(env);
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

    /// How the installer ended the last session (`razdorInstallOutcome`): "installed",
    /// "cancelled", "failed: <why>", or "" while it has not ended (or the call is missing).
    pub fn installer_outcome() -> String {
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "()Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorInstallOutcome", sig, false);
            if mid.is_null() || cleared(env) {
                return String::new();
            }
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid));
            if cleared(env) {
                return String::new();
            }
            to_text(env, reply.obj).unwrap_or_default()
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

    /// The body of `url`, through a file in the cache folder. Razdor's own: the file has a name of
    /// its own (`update::fetch_temp_name`), so fetches at the same time never share it.
    pub fn get(url: &str, seconds: u32) -> Result<Vec<u8>, String> {
        let path = cache_dir()?.join(crate::update::fetch_temp_name());
        let body = download(url, &path, seconds).and_then(|()| std::fs::read(&path).map_err(|e| e.to_string()));
        let _ = std::fs::remove_file(&path);
        body
    }

    /// One POST of `body` to `url` (the analytics' requests). The body goes through a file in the cache
    /// folder, read by the Java side (`razdorPost`), which answers `HTTP <code>` and the reply's body.
    /// `version` and `commit` are the API headers of the build.
    pub fn post(url: &str, body: &[u8], seconds: u32, version: &str, commit: &str) -> Result<(u16, Vec<u8>, Option<u64>), String> {
        secure(url)?;
        let path = cache_dir()?.join(crate::update::fetch_temp_name());
        std::fs::write(&path, body).map_err(|e| format!("post: {e}"))?;
        let text = java_post(url, &path, seconds, version, commit);
        let _ = std::fs::remove_file(&path);
        crate::update::parse_post_reply(&text?)
    }

    /// One cloud request (`cloud::http::Phone`): `verb` to `url` with the account `key` (its header) and
    /// the `body` (written to a file in the cache folder, read by the Java side's `razdorRequest`). The
    /// reply's body comes back as bytes, from a file the Java side writes. Any status is `Ok`. `version`
    /// and `commit` are the API headers of the build.
    pub fn request(verb: &str, url: &str, key: Option<&str>, body: Option<&[u8]>, seconds: u32, version: &str, commit: &str) -> Result<(u16, Vec<u8>, Option<u64>), String> {
        secure(url)?;
        let dir = cache_dir()?;
        let stem = crate::update::fetch_temp_name();
        let reply = dir.join(format!("{stem}-reply"));
        let body_path = body.map(|_| dir.join(format!("{stem}-body")));
        if let (Some(path), Some(bytes)) = (&body_path, body) {
            std::fs::write(path, bytes).map_err(|e| format!("request: {e}"))?;
        }
        let text = java_request(verb, url, key.unwrap_or(""), body_path.as_deref(), &reply, seconds, version, commit);
        let result = text.and_then(|text| match crate::update::http_status(&text) {
            Some((code, retry_after)) => std::fs::read(&reply).map(|bytes| (code, bytes, retry_after)).map_err(|e| format!("request: {e}")),
            None => Err(if text.is_empty() { "no reply".to_string() } else { text }),
        });
        let _ = std::fs::remove_file(&reply);
        if let Some(path) = &body_path {
            let _ = std::fs::remove_file(path);
        }
        result
    }

    /// The Java side's `razdorRequest`: its reply text (see there); the body is in the `reply` file.
    fn java_request(verb: &str, url: &str, key: &str, body: Option<&Path>, reply: &Path, seconds: u32, version: &str, commit: &str) -> Result<String, String> {
        let reply = reply.to_str().ok_or("the file's path is not text")?;
        let body = match body {
            Some(path) => path.to_str().ok_or("the file's path is not text")?,
            None => "",
        };
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;II)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorRequest", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorRequest is missing".into());
            }
            let stall = crate::mirror::stall_seconds(url) as i32;
            let (verb, url, key, body, reply) = (string(env, verb), string(env, url), string(env, key), string(env, body), string(env, reply));
            let (version, commit) = (string(env, version), string(env, commit));
            let answer = Local::new(
                env,
                ((**env).CallObjectMethod.unwrap())(
                    env,
                    ACTIVITY,
                    mid,
                    verb.obj,
                    url.obj,
                    key.obj,
                    body.obj,
                    reply.obj,
                    version.obj,
                    commit.obj,
                    stall,
                    seconds as i32,
                ),
            );
            if cleared(env) {
                return Err("the request failed".into());
            }
            to_text(env, answer.obj).ok_or_else(|| "no reply".into())
        }
    }

    /// The Java side's `razdorPost`: the file `path` POSTed to `url` (see there for its reply).
    fn java_post(url: &str, path: &Path, seconds: u32, version: &str, commit: &str) -> Result<String, String> {
        let path = path.to_str().ok_or("the file's path is not text")?;
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;II)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorPost", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorPost is missing".into());
            }
            let stall = crate::mirror::stall_seconds(url) as i32;
            let (url, path) = (string(env, url), string(env, path));
            let (version, commit) = (string(env, version), string(env, commit));
            let reply = Local::new(
                env,
                ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid, url.obj, path.obj, version.obj, commit.obj, stall, seconds as i32),
            );
            if cleared(env) {
                return Err("the post failed".into());
            }
            to_text(env, reply.obj).ok_or_else(|| "no reply".into())
        }
    }

    /// A conditional request for `url` (`update::fetch_conditional`): the Java side sends the validators
    /// and gives "304", or "200" with the body's file and its validators (see `razdorFetchConditional`).
    pub fn get_conditional(url: &str, seconds: u32, have: &crate::update::Validators) -> Result<crate::update::Conditional, String> {
        use crate::update::{Conditional, Validators};
        crate::mirror::wait_host(url, &|| false);
        secure(url)?;
        let path = cache_dir()?.join(crate::update::fetch_temp_name());
        let text = java_conditional(url, &path, seconds, have);
        let result = match text {
            Ok(text) if text == "304" => Ok(Conditional::NotModified),
            Ok(text) if text.starts_with("200\n") => {
                let mut lines = text.splitn(3, '\n').skip(1);
                let line = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(String::from);
                let validators = Validators { etag: line(lines.next()), last_modified: line(lines.next()) }.clean();
                std::fs::read(&path).map(|body| Conditional::Fresh { body, validators }).map_err(|e| e.to_string())
            }
            Ok(text) if crate::mirror::http_reply(&text).is_some() => Err(text),
            Ok(text) if text.starts_with("stalled") => {
                crate::diag!("download: {text}");
                Err(crate::update::STALLED.into())
            }
            Ok(text) if text.is_empty() => Err("no reply".into()),
            Ok(text) => Err(text),
            Err(e) => Err(e),
        };
        let _ = std::fs::remove_file(&path);
        result
    }

    /// The Java side's `razdorFetchConditional` for `url` into `path`: its reply text (see there).
    fn java_conditional(url: &str, path: &Path, seconds: u32, have: &crate::update::Validators) -> Result<String, String> {
        secure(url)?;
        let path = path.to_str().ok_or("the file's path is not text")?;
        // SAFETY: as in `cache_dir`.
        unsafe {
            let env = attach_jni_env();
            let sig = "(Ljava/lang/String;Ljava/lang/String;IILjava/lang/String;Ljava/lang/String;)Ljava/lang/String;";
            let mid = method(env, activity_class(env).obj, "razdorFetchConditional", sig, false);
            if mid.is_null() || cleared(env) {
                return Err("razdorFetchConditional is missing".into());
            }
            let stall = crate::mirror::stall_seconds(url) as i32;
            let (url, path) = (string(env, url), string(env, path));
            let etag = string(env, have.etag.as_deref().unwrap_or(""));
            let modified = string(env, have.last_modified.as_deref().unwrap_or(""));
            let reply = Local::new(env, ((**env).CallObjectMethod.unwrap())(env, ACTIVITY, mid, url.obj, path.obj, stall, seconds as i32, etag.obj, modified.obj));
            if cleared(env) {
                return Err("the download failed".into());
            }
            to_text(env, reply.obj).ok_or_else(|| "no reply".into())
        }
    }
}

#[cfg(target_os = "android")]
pub use android::{cache_dir, download, download_cancel, download_range, get, get_conditional, install, installer_outcome, post, request};

/// iOS: no POST yet (the analytics stay queued there; `telemetry` retries later).
#[cfg(target_os = "ios")]
pub fn post(_url: &str, _body: &[u8], _seconds: u32, _version: &str, _commit: &str) -> Result<(u16, Vec<u8>, Option<u64>), String> {
    Err("post: not on iOS yet".into())
}

/// iOS: a download is one `NSData` call that cannot be stopped in the middle (the caller's
/// cancel is seen when it returns). Nothing to do.
#[cfg(target_os = "ios")]
pub fn download_cancel(_path: &std::path::Path) {}

#[cfg(target_os = "ios")]
pub fn get(url: &str, _seconds: u32) -> Result<Vec<u8>, String> {
    // No stall limit here: `NSData dataWithContentsOfURL:` has no timeout to set (the stall
    // limit of `update::STALL_SECONDS` is a desktop curl option and the Android read timeout).
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
