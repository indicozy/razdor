//! The map under a window, frozen (the owner's decision, 2026-11-13; a player's report: on an Adreno 610
//! phone the map under the settings window was drawn in full every frame, 8 FPS).
//!
//! Every window over the world map draws the map behind it (`world_view::draw_map_backdrop`). Drawn live,
//! that is the whole map again each frame, animations included. Frozen: the frame a window opens over the
//! map, the picture the player sees is copied off the screen into a target of the screen's size
//! ([`capture`]: the backdrop as drawn, its light, fog, weather, dim and the map's resolution included), and
//! the frames after show that copy ([`show`]) while the key holds. The key is the map picture's inputs
//! without the animations (`world_view::freeze_key`) and the settings' look ([`note_settings`]): a changed
//! key copies once more, and so does a changed size (the window's pixels, its DPI). The copy is released
//! the frame after the last frame with a window over the map ([`begin_frame`]).
//!
//! Not frozen: the battle's backdrop (its own picture, `world_view::battle_backdrop`), the watching screen,
//! and the title screens' backdrops.
//!
//! Why a copy of the screen and not a draw into the map's own target: `render_cache::fill` and
//! `map_scale::draw` do not nest (the inner one's end sends the camera back to the screen, and inside a fill
//! the map's scale is dropped, as the battle's picture shows). A copy of the screen takes the frame as drawn.
//!
//! The copy is an RGB8 texture (no alpha). GLES refuses to copy an RGBA texture from a framebuffer without
//! alpha (the phones' EGL surface has none), so an RGBA copy stayed empty and the map showed black (a player's
//! report, 2026-11-15). An RGB copy is valid from both. If a copy still fails (a GL error), it is dropped, the
//! map is drawn live under the windows from then on (the gate, [`CopyGate`]), and the log says why.

use super::audio::Settings;
use macroquad::miniquad::{TextureFormat, TextureParams};
use macroquad::texture::{FilterMode, Texture2D};
use macroquad::window::get_internal_gl;
use std::hash::{Hash, Hasher};

/// The largest copy side in physical pixels (`render_cache`'s limit): past it the map is drawn live.
const MAX_SIDE: f32 = 8192.0;

/// A held copy: the key it was made for, the screen's size then, and the target.
struct Held<T> {
    key: u64,
    size: (u32, u32),
    tex: T,
}

/// The copy's lease: held while windows are drawn over the map, released a frame after the last one.
pub struct Lease<T> {
    held: Option<Held<T>>,
    /// A window over the map was drawn this frame (`lookup` or `store`).
    used: bool,
}

impl<T> Default for Lease<T> {
    fn default() -> Self {
        Lease { held: None, used: false }
    }
}

impl<T> Lease<T> {
    /// The frame starts. When no window over the map was drawn the frame before, the copy is released:
    /// returned, for the caller to drop.
    pub fn begin_frame(&mut self) -> Option<T> {
        let used = std::mem::take(&mut self.used);
        if used {
            None
        } else {
            self.held.take().map(|h| h.tex)
        }
    }

    /// The copy made for `key` at `size`, when it holds. A window over the map is drawn this frame.
    pub fn lookup(&mut self, key: u64, size: (u32, u32)) -> Option<&T> {
        self.used = true;
        self.held.as_ref().filter(|h| h.key == key && h.size == size).map(|h| &h.tex)
    }

    /// Gives up the held copy: returned, for the caller to drop.
    pub fn take(&mut self) -> Option<T> {
        self.held.take().map(|h| h.tex)
    }

    /// Keeps `tex` as the copy for `key` at `size`, in place of the one held (returned, for the caller to drop).
    pub fn store(&mut self, key: u64, size: (u32, u32), tex: T) -> Option<T> {
        self.used = true;
        self.held.replace(Held { key, size, tex }).map(|h| h.tex)
    }
}

/// The GL error code of a call that succeeded.
const GL_NO_ERROR: u32 = 0;

/// One copy of the screen: the miniquad RGB8 texture it was copied into. macroquad's own handles cannot
/// make an RGB8 texture, so it is freed by hand ([`ScreenCopy::free`]).
pub struct ScreenCopy(macroquad::miniquad::TextureId);

impl ScreenCopy {
    /// The copy as a texture to draw (the handle does not own the texture: `free` does).
    fn texture(&self) -> Texture2D {
        Texture2D::from_miniquad_texture(self.0)
    }

    /// Deletes the texture.
    fn free(self) {
        unsafe { get_internal_gl() }.quad_context.delete_texture(self.0);
    }
}

fn free_all(copy: Option<ScreenCopy>) {
    if let Some(copy) = copy {
        copy.free();
    }
}

/// Whether the screen copy works in this run. A copy that failed once is never tried again: the map is drawn
/// live under the windows from then on, as before the freeze existed.
#[derive(Default)]
pub struct CopyGate {
    /// The GL error that broke the copy, once one did.
    broken: Option<u32>,
}

impl CopyGate {
    /// Whether a copy may be tried.
    pub fn allowed(&self) -> bool {
        self.broken.is_none()
    }

    /// Records a copy that failed with the GL error `error`. Returns true the first time only (the log).
    pub fn fail(&mut self, error: u32) -> bool {
        let first = self.broken.is_none();
        self.broken.get_or_insert(error);
        first
    }
}

razdor::local! {
    /// The held copy of the map under the windows: none while no window is drawn over the map.
    static LEASE: std::cell::RefCell<Lease<ScreenCopy>> = const { std::cell::RefCell::new(Lease { held: None, used: false }) };
    /// Whether the screen copy has failed in this run (`CopyGate`).
    static GATE: std::cell::RefCell<CopyGate> = const { std::cell::RefCell::new(CopyGate { broken: None }) };
    /// The settings' look key of this frame (`note_settings`), read by the freeze key.
    static LOOK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Whether a `w` × `h` copy can be kept: at least a pixel a side and at most [`MAX_SIDE`].
pub fn usable(w: f32, h: f32) -> bool {
    w >= 1.0 && h >= 1.0 && w <= MAX_SIDE && h <= MAX_SIDE
}

/// The screen's size in physical pixels, when a copy of it can be kept (`None`: the map is drawn live, the
/// size is too large or a copy failed before in this run).
pub fn pixels() -> Option<(u32, u32)> {
    // The CRT effect draws into its own stand-in, so `grab_screen` (which reads the real screen) would copy
    // nothing of the frame: the map is drawn live under the windows while the effect is on.
    if super::crt::active() || !GATE.with(|g| g.borrow().allowed()) {
        return None;
    }
    let (w, h) = macroquad::miniquad::window::screen_size();
    usable(w, h).then_some((w as u32, h as u32))
}

/// The frame starts: releases the copy when no window over the map was drawn the frame before.
pub fn begin_frame() {
    let released = LEASE.with(|l| l.borrow_mut().begin_frame());
    free_all(released);
}

/// The copy for `key` at `size`, when it holds (the frame is then one with a window over the map).
pub fn frozen(key: u64, size: (u32, u32)) -> Option<Texture2D> {
    LEASE.with(|l| l.borrow_mut().lookup(key, size).map(ScreenCopy::texture))
}

/// Shows the copy on the screen as it was copied: the screen's own pixels, no blend (the copy has no alpha).
pub fn show(tex: &Texture2D) {
    super::render_cache::blit_screen(tex);
}

/// The first GL error since the last call, all of them cleared (0 when there was none).
fn take_gl_error() -> u32 {
    let mut first = GL_NO_ERROR;
    for _ in 0..64 {
        let e = unsafe { macroquad::miniquad::gl::glGetError() };
        if e == GL_NO_ERROR {
            break;
        }
        if first == GL_NO_ERROR {
            first = e;
        }
    }
    first
}

/// Copies the screen now, as the frame's map is drawn, into a new RGB8 texture kept for `key` at `size`; the
/// copy held before is released first. The camera must be the screen's, with the frame's draws done. When the
/// copy fails (a GL error), nothing is kept and the gate closes for the run: the caller has drawn the map live
/// this frame, and the frames after draw it live too (`pixels` is `None`).
pub fn capture(key: u64, size: (u32, u32)) {
    let released = LEASE.with(|l| l.borrow_mut().take());
    free_all(released);
    // The frame's pending draws reach the screen before it is read (macroquad's `get_screen_data` does this too).
    unsafe { get_internal_gl() }.flush();
    // Errors left by earlier calls are cleared first, so only this copy's error counts.
    take_gl_error();
    let id = unsafe { get_internal_gl() }.quad_context.new_render_texture(TextureParams {
        width: size.0,
        height: size.1,
        format: TextureFormat::RGB8,
        ..Default::default()
    });
    let copy = ScreenCopy(id);
    let tex = copy.texture();
    tex.set_filter(FilterMode::Nearest);
    super::render_cache::to_screen();
    tex.grab_screen();
    let error = take_gl_error();
    if error != GL_NO_ERROR {
        copy.free();
        let first = GATE.with(|g| g.borrow_mut().fail(error));
        if first {
            razdor::diag!("map freeze: screen copy failed (GL error 0x{error:x}), the map is drawn live under windows");
        }
        return;
    }
    razdor::mem_probe::set_texture("map freeze copy", razdor::mem_probe::texture_bytes(size.0, size.1, 3));
    let replaced = LEASE.with(|l| l.borrow_mut().store(key, size, copy));
    free_all(replaced);
}

/// Releases the copy: the map is drawn live (the screen's size cannot be kept).
pub fn release() {
    let released = LEASE.with(|l| l.borrow_mut().take());
    free_all(released);
}

/// Keeps the settings' look of this frame for the freeze key (see [`look_of`]).
pub fn note_settings(settings: &Settings) {
    let key = look_of(settings);
    LOOK.with(|c| c.set(key));
}

/// The settings' look key of this frame (`note_settings`).
pub fn look_key() -> u64 {
    LOOK.with(|c| c.get())
}

/// The hash of the settings that change the map's picture. The volumes and the speed sliders, and the
/// frame rate counter, are left out: dragging one of them must not copy the map again.
pub fn look_of(settings: &Settings) -> u64 {
    let mut s = settings.clone();
    s.music_volume = 0.0;
    s.sfx_volume = 0.0;
    s.scroll_speed = None;
    s.walk_speed = None;
    s.show_fps = false;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&s).unwrap_or_default().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (u32, u32) = (2400, 1080);

    /// One frame's backdrop over the map, as `world_view::draw_map_backdrop` decides it: a copy is made when
    /// the lookup misses. Returns whether it copied; `copies` counts the copies made, and each copy's id is it.
    fn backdrop(l: &mut Lease<u32>, key: u64, size: (u32, u32), copies: &mut u32) -> bool {
        if l.lookup(key, size).is_some() {
            return false;
        }
        l.take();
        *copies += 1;
        l.store(key, size, *copies);
        true
    }

    #[test]
    fn opening_a_window_copies_the_map_once() {
        let (mut l, mut n) = (Lease::default(), 0);
        assert!(backdrop(&mut l, 7, SCREEN, &mut n), "the first frame copies");
        for _ in 0..30 {
            assert!(l.begin_frame().is_none());
            assert!(!backdrop(&mut l, 7, SCREEN, &mut n), "the frames after show the copy");
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn a_window_over_another_window_keeps_the_copy() {
        let (mut l, mut n) = (Lease::default(), 0);
        assert!(backdrop(&mut l, 7, SCREEN, &mut n));
        assert!(l.begin_frame().is_none());
        // The settings window, then a window opened over it: both draw the backdrop in one frame.
        assert!(!backdrop(&mut l, 7, SCREEN, &mut n));
        assert!(!backdrop(&mut l, 7, SCREEN, &mut n));
        assert_eq!(n, 1);
    }

    #[test]
    fn a_changed_look_or_state_copies_once_more() {
        let (mut l, mut n) = (Lease::default(), 0);
        backdrop(&mut l, 7, SCREEN, &mut n);
        assert!(l.begin_frame().is_none());
        assert!(backdrop(&mut l, 8, SCREEN, &mut n), "the key changed: one copy");
        for _ in 0..5 {
            assert!(l.begin_frame().is_none());
            assert!(!backdrop(&mut l, 8, SCREEN, &mut n));
        }
        assert_eq!(n, 2);
    }

    #[test]
    fn a_resized_screen_copies_again() {
        let (mut l, mut n) = (Lease::default(), 0);
        backdrop(&mut l, 7, SCREEN, &mut n);
        assert!(l.begin_frame().is_none());
        assert!(backdrop(&mut l, 7, (1080, 2400), &mut n), "a phone turned upright: one copy");
        assert_eq!(n, 2);
    }

    #[test]
    fn the_copy_is_released_a_frame_after_the_last_window() {
        let (mut l, mut n) = (Lease::default(), 0);
        backdrop(&mut l, 7, SCREEN, &mut n);
        // The window closed: this frame drew no backdrop, so the copy is kept for the frame it was drawn in.
        assert!(l.begin_frame().is_none());
        // The next frame without a window releases it.
        assert_eq!(l.begin_frame(), Some(1));
        assert_eq!(l.begin_frame(), None);
    }

    #[test]
    fn a_window_opened_again_after_the_release_copies_again() {
        let (mut l, mut n) = (Lease::default(), 0);
        backdrop(&mut l, 7, SCREEN, &mut n);
        l.begin_frame();
        l.begin_frame();
        assert_eq!(l.begin_frame(), None, "released already");
        assert!(backdrop(&mut l, 7, SCREEN, &mut n));
        assert_eq!(n, 2);
    }

    #[test]
    fn a_copy_too_large_or_empty_is_not_kept() {
        assert!(usable(2400.0, 1080.0));
        assert!(usable(8192.0, 1.0));
        assert!(!usable(8193.0, 1080.0), "past the limit: drawn live");
        assert!(!usable(0.0, 1080.0));
        assert!(!usable(f32::NAN, 1080.0));
    }

    #[test]
    fn a_failed_copy_closes_the_gate_for_the_run() {
        const INVALID_OPERATION: u32 = 0x502;
        let mut gate = CopyGate::default();
        assert!(gate.allowed(), "the copy is tried first");
        assert!(gate.fail(INVALID_OPERATION), "the first failure is logged");
        assert!(!gate.allowed(), "after a failure the map is drawn live");
        assert!(!gate.fail(0x505), "later failures are not logged again");
        assert!(!gate.allowed());
    }

    #[test]
    fn the_look_follows_the_map_settings_and_not_the_sliders() {
        let base = Settings::default();
        let k = look_of(&base);
        let mut s = base.clone();
        s.music_volume = 0.3;
        s.sfx_volume = 0.1;
        s.scroll_speed = Some(2.0);
        s.walk_speed = Some(3.0);
        s.show_fps = true;
        assert_eq!(look_of(&s), k, "a slider drag does not copy the map again");
        let mut s = base.clone();
        s.map_render_scale = 0.75;
        assert_ne!(look_of(&s), k, "the render scale is the picture's");
        let mut s = base.clone();
        s.map_tilt = !s.map_tilt;
        assert_ne!(look_of(&s), k, "the view is the picture's");
        let mut s = base.clone();
        s.map_anim = match s.map_anim {
            super::super::map_anim::Mode::Off => super::super::map_anim::Mode::On,
            _ => super::super::map_anim::Mode::Off,
        };
        assert_ne!(look_of(&s), k, "the animations are the picture's");
    }
}
