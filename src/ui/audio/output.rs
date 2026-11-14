//! The audio output: macroquad's everywhere but Windows, Razdor's own there.
//!
//! On Windows macroquad keeps its audio context private, and quad-snd opens the default output
//! device once, when its context is made, so a changed default output was never followed. Razdor
//! opens its own quad-snd context instead: [`open`] makes one on the default device and drops the
//! previous one, and the watcher ([`current`]) tells [`super::follow`] when the default changes.
//! A sound belongs to the context it was loaded in: it is deleted from that one only, so a sound
//! that outlived its context does nothing (its mixer is gone).

#[cfg(all(windows, feature = "audio"))]
mod imp {
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
    use std::time::Duration;

    pub use quad_snd::PlaySoundParams;
    use quad_snd::{AudioContext, Sound as Native};
    use winapi::shared::guiddef::IID;
    use winapi::shared::ntdef::LPWSTR;
    use winapi::shared::winerror::HRESULT;
    use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL};
    use winapi::um::mmdeviceapi::{eConsole, eRender, IMMDevice, IMMDeviceEnumerator, CLSID_MMDeviceEnumerator};
    use winapi::um::objbase::COINIT_MULTITHREADED;

    // winapi imports CoTaskMemFree from combase.dll, which Windows 7 does not have (it came with
    // Windows 8). ole32.dll exports the same function on every Windows, Windows 7 included, so it is
    // declared here against ole32 (the other Co* functions above already come from ole32).
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(pv: *mut std::ffi::c_void);
    }

    /// How often the watcher reads the default output.
    const WATCH_EVERY: Duration = Duration::from_secs(2);

    // winapi does not export this one (quad-snd defines it the same way).
    static IID_IMMDEVICE_ENUMERATOR: IID = IID {
        Data1: 0xa95664d2,
        Data2: 0x9614,
        Data3: 0x4f35,
        Data4: [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6],
    };

    /// The open output: its context, and the generation that its sounds must match.
    struct Live {
        ctx: AudioContext,
        generation: u64,
    }

    static LIVE: Mutex<Option<Live>> = Mutex::new(None);
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
    /// The endpoint id the output was opened on (`None`: not open).
    static OPENED_ON: Mutex<Option<String>> = Mutex::new(None);
    /// The endpoint id of the default output as the watcher last read it (shared, so that the
    /// per-frame check copies no string).
    static CURRENT: Mutex<Option<Arc<str>>> = Mutex::new(None);
    static WATCHING: OnceLock<()> = OnceLock::new();

    /// A lock that a panic elsewhere has poisoned still works: the data is only handles.
    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The endpoint id of the default output (`Ok(None)`: no output device), read on any thread.
    pub fn default_output_id() -> Result<Option<String>, String> {
        // HRESULT_FROM_WIN32(ERROR_NOT_FOUND): no default endpoint.
        const NOT_FOUND: HRESULT = 0x8007_0490u32 as HRESULT;
        unsafe {
            let init = CoInitializeEx(null_mut(), COINIT_MULTITHREADED);
            let result = query_default();
            if init >= 0 {
                CoUninitialize();
            }
            match result {
                Err(hr) if hr == NOT_FOUND => Ok(None),
                Err(hr) => Err(format!("default output: HRESULT 0x{:08x}", hr as u32)),
                Ok(id) => Ok(Some(id)),
            }
        }
    }

    /// The id of the default render endpoint; `Err` carries the failing HRESULT.
    unsafe fn query_default() -> Result<String, HRESULT> {
        let mut enumerator: *mut IMMDeviceEnumerator = null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDeviceEnumerator,
            null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator as *mut _ as *mut _,
        );
        if hr < 0 {
            return Err(hr);
        }
        let mut device: *mut IMMDevice = null_mut();
        let hr = (*enumerator).GetDefaultAudioEndpoint(eRender, eConsole, &mut device);
        let result = if hr < 0 {
            Err(hr)
        } else {
            let mut id: LPWSTR = null_mut();
            let hr = (*device).GetId(&mut id);
            let result = if hr < 0 {
                Err(hr)
            } else {
                let mut len = 0;
                while *id.add(len) != 0 {
                    len += 1;
                }
                let text = String::from_utf16_lossy(std::slice::from_raw_parts(id, len));
                CoTaskMemFree(id as _);
                Ok(text)
            };
            (*device).Release();
            result
        };
        (*enumerator).Release();
        result
    }

    /// Starts the watcher once: it reads the default output every [`WATCH_EVERY`]. A failed read
    /// keeps the last id.
    fn start_watch() {
        WATCHING.get_or_init(|| {
            let _ = std::thread::Builder::new().name("default-output".into()).spawn(|| loop {
                if let Ok(id) = default_output_id() {
                    *lock(&CURRENT) = id.map(Arc::from);
                }
                std::thread::sleep(WATCH_EVERY);
            });
        });
    }

    /// The endpoint id of the default output as the watcher last read it (`None`: none, or not
    /// read yet).
    pub fn current() -> Option<Arc<str>> {
        lock(&CURRENT).clone()
    }

    /// The endpoint id the output is open on (`None`: not open).
    pub fn opened_on() -> Option<String> {
        lock(&OPENED_ON).clone()
    }

    /// Opens the output on the default device, dropping the previous one. The caller has deleted
    /// its sounds first (dropping its backend), so the old mixer holds nothing. `false`: no
    /// default device (nothing is open, and sounds cannot be loaded).
    pub fn open() -> bool {
        start_watch();
        let id = match default_output_id() {
            Ok(id) => id,
            Err(e) => {
                razdor::diag!("audio: {e}");
                None
            }
        };
        let mut live = lock(&LIVE);
        *live = None;
        *lock(&OPENED_ON) = id.clone();
        if id.is_none() {
            return false;
        }
        *live = Some(Live { ctx: AudioContext::new(), generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed) });
        true
    }

    /// A sound loaded in one output; it is deleted from that output when dropped, and only then.
    #[derive(Clone)]
    pub struct Sound(Arc<Guard>);

    struct Guard {
        native: Native,
        generation: u64,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let live = lock(&LIVE);
            if let Some(l) = live.as_ref().filter(|l| l.generation == self.generation) {
                self.native.delete(&l.ctx);
            }
        }
    }

    /// Loads WAV bytes into the open output (`None` when none is open, or the data is bad: a
    /// panic from quad-snd's decoder is the caller's to catch).
    pub fn load(wav: &[u8]) -> Option<Sound> {
        let live = lock(&LIVE);
        let l = live.as_ref()?;
        let native = Native::load(&l.ctx, wav);
        Some(Sound(Arc::new(Guard { native, generation: l.generation })))
    }

    /// Runs `f` on the sound's output, if it is the open one.
    fn with<R>(sound: &Sound, f: impl FnOnce(&AudioContext, &Native) -> R) -> Option<R> {
        let live = lock(&LIVE);
        let l = live.as_ref().filter(|l| l.generation == sound.0.generation)?;
        Some(f(&l.ctx, &sound.0.native))
    }

    pub fn play_sound(sound: &Sound, params: PlaySoundParams) {
        with(sound, |ctx, s| {
            s.play(ctx, params);
        });
    }

    pub fn stop_sound(sound: &Sound) {
        with(sound, |ctx, s| s.stop(ctx));
    }

    pub fn set_sound_volume(sound: &Sound, volume: f32) {
        with(sound, |ctx, s| s.set_volume(ctx, volume));
    }
}

#[cfg(not(all(windows, feature = "audio")))]
mod imp {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    pub use macroquad::audio::{play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
    use macroquad::audio::load_sound_from_bytes;

    /// macroquad's output is open from the start; there is nothing to open.
    pub fn open() -> bool {
        true
    }

    /// Not followed here: the output is macroquad's.
    pub fn opened_on() -> Option<String> {
        None
    }

    /// Not followed here.
    pub fn current() -> Option<std::sync::Arc<str>> {
        None
    }

    /// Loads WAV bytes. On native targets macroquad's loader never waits, so one poll finishes it.
    pub fn load(wav: &[u8]) -> Option<Sound> {
        let mut f = std::pin::pin!(load_sound_from_bytes(wav));
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(r) => r.ok(),
            Poll::Pending => None,
        }
    }
}

pub use imp::*;
