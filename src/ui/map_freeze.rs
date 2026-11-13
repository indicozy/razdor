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

use super::audio::Settings;
use macroquad::camera::set_default_camera;
use macroquad::texture::{render_target, FilterMode, RenderTarget};
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

razdor::local! {
    /// The held copy of the map under the windows: none while no window is drawn over the map.
    static LEASE: std::cell::RefCell<Lease<RenderTarget>> = const { std::cell::RefCell::new(Lease { held: None, used: false }) };
    /// The settings' look key of this frame (`note_settings`), read by the freeze key.
    static LOOK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Whether a `w` × `h` copy can be kept: at least a pixel a side and at most [`MAX_SIDE`].
pub fn usable(w: f32, h: f32) -> bool {
    w >= 1.0 && h >= 1.0 && w <= MAX_SIDE && h <= MAX_SIDE
}

/// The screen's size in physical pixels, when a copy of it can be kept (`None`: the map is drawn live).
pub fn pixels() -> Option<(u32, u32)> {
    let (w, h) = macroquad::miniquad::window::screen_size();
    usable(w, h).then_some((w as u32, h as u32))
}

/// The frame starts: releases the copy when no window over the map was drawn the frame before.
pub fn begin_frame() {
    let released = LEASE.with(|l| l.borrow_mut().begin_frame());
    drop(released);
}

/// The copy for `key` at `size`, when it holds (the frame is then one with a window over the map).
pub fn frozen(key: u64, size: (u32, u32)) -> Option<RenderTarget> {
    LEASE.with(|l| l.borrow_mut().lookup(key, size).cloned())
}

/// Shows the copy on the screen as it was copied: the screen's own pixels, no blend.
pub fn show(rt: &RenderTarget) {
    super::render_cache::blit_screen(&rt.texture);
}

/// Copies the screen now, as the frame's map is drawn, into a new target kept for `key` at `size`; the
/// copy held before is released first. The camera must be the screen's, with the frame's draws done.
pub fn capture(key: u64, size: (u32, u32)) {
    let released = LEASE.with(|l| l.borrow_mut().take());
    drop(released);
    let rt = render_target(size.0, size.1);
    rt.texture.set_filter(FilterMode::Nearest);
    // The frame's pending draws reach the screen before it is read (macroquad's `get_screen_data` does this too).
    unsafe { get_internal_gl().flush() };
    set_default_camera();
    rt.texture.grab_screen();
    let replaced = LEASE.with(|l| l.borrow_mut().store(key, size, rt));
    drop(replaced);
}

/// Releases the copy: the map is drawn live (the screen's size cannot be kept).
pub fn release() {
    let released = LEASE.with(|l| l.borrow_mut().take());
    drop(released);
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
