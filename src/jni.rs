//! The JNI helpers of Razdor's Android code: the library's `update::phone` and the window's
//! `ui::android` make their calls through these. A local reference is freed when its [`Local`]
//! goes out of scope (a long session polls some of these every few seconds), and a Java
//! exception raised by a call is cleared by [`cleared`], so a refused call never ends the
//! process.

use macroquad::miniquad::native::android::{ndk_sys as sys, ACTIVITY};
use std::ffi::{CStr, CString};

/// A local JNI reference, freed when dropped.
pub struct Local {
    env: *mut sys::JNIEnv,
    pub obj: sys::jobject,
}

impl Local {
    pub fn new(env: *mut sys::JNIEnv, obj: sys::jobject) -> Local {
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
///
/// # Safety
/// `env` is the JNI environment of the current thread.
pub unsafe fn cleared(env: *mut sys::JNIEnv) -> bool {
    let pending = ((**env).ExceptionCheck.unwrap())(env) != 0;
    if pending {
        ((**env).ExceptionClear.unwrap())(env);
    }
    pending
}

/// The class called `name` (`android/os/Build$VERSION`).
///
/// # Safety
/// `env` is the JNI environment of the current thread.
pub unsafe fn class(env: *mut sys::JNIEnv, name: &str) -> Local {
    let name = CString::new(name).unwrap();
    Local::new(env, ((**env).FindClass.unwrap())(env, name.as_ptr()))
}

/// A new Java string holding `text`.
///
/// # Safety
/// `env` is the JNI environment of the current thread.
pub unsafe fn string(env: *mut sys::JNIEnv, text: &str) -> Local {
    let text = CString::new(text).unwrap();
    Local::new(env, ((**env).NewStringUTF.unwrap())(env, text.as_ptr()))
}

/// The method `name` with signature `sig` of `cls` (a static one when `is_static`); null when
/// there is none.
///
/// # Safety
/// `env` is the JNI environment of the current thread and `cls` a class of it.
pub unsafe fn method(env: *mut sys::JNIEnv, cls: sys::jclass, name: &str, sig: &str, is_static: bool) -> sys::jmethodID {
    let name = CString::new(name).unwrap();
    let sig = CString::new(sig).unwrap();
    if is_static {
        ((**env).GetStaticMethodID.unwrap())(env, cls, name.as_ptr(), sig.as_ptr())
    } else {
        ((**env).GetMethodID.unwrap())(env, cls, name.as_ptr(), sig.as_ptr())
    }
}

/// The activity's class (its methods are looked up on it).
///
/// # Safety
/// The activity exists (the app's window is open) and `env` is the current thread's.
pub unsafe fn activity_class(env: *mut sys::JNIEnv) -> Local {
    Local::new(env, ((**env).GetObjectClass.unwrap())(env, ACTIVITY))
}

/// The Java string `obj` as Rust text; None for null.
///
/// # Safety
/// `env` is the JNI environment of the current thread and `obj` a local reference of it.
pub unsafe fn to_text(env: *mut sys::JNIEnv, obj: sys::jobject) -> Option<String> {
    if obj.is_null() {
        return None;
    }
    let chars = ((**env).GetStringUTFChars.unwrap())(env, obj, std::ptr::null_mut());
    if chars.is_null() {
        return None;
    }
    let text = CStr::from_ptr(chars).to_string_lossy().into_owned();
    ((**env).ReleaseStringUTFChars.unwrap())(env, obj, chars);
    Some(text)
}
