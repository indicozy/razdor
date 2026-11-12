//! The CPU-side alpha of the sprite textures the 2.5D pointer picks from (the map atlas and the figure sheets).
//!
//! The pointer's alpha test (`world_view::opaque_hit`) reads one texel of a picked sprite, and must not read the
//! GPU back to do it. So each such texture keeps the alpha of its own pixels, taken from the image it was made
//! from when it is made ([`register`], in `dt_art`'s `sprite_texture`): one byte a texel, kept under the texture's
//! id. A texture with no mask (a mod's, or one made elsewhere) is not alpha-tested: its picks hit as their quads.

use std::rc::Rc;

use macroquad::prelude::*;
use razdor::dt::gfx::Image;
use razdor::local;

/// A texture's alpha, one byte a texel, row by row from the top.
pub struct AlphaMask {
    pub w: usize,
    pub h: usize,
    pub a: Vec<u8>,
}

/// The most masks kept. The oldest goes first: a mod switch makes new atlases, and the current ones are the newest.
const MAX_MASKS: usize = 64;

local! {
    /// The masks by the texture's key ([`key_of`]), oldest first.
    static MASKS: std::cell::RefCell<Vec<(u64, Rc<AlphaMask>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The key of a texture: a hash of its id on the GPU backend.
pub fn key_of(tex: &Texture2D) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tex.raw_miniquad_id().hash(&mut h);
    h.finish()
}

/// Keeps the alpha of `img`, the image `tex` was made from. Replaces a mask kept under the same key.
pub fn register(tex: &Texture2D, img: &Image) {
    let a: Vec<u8> = img.rgba.chunks_exact(4).map(|px| px[3]).collect();
    if a.len() != img.width as usize * img.height as usize {
        return;
    }
    let mask = AlphaMask { w: img.width as usize, h: img.height as usize, a };
    register_mask(key_of(tex), mask);
}

/// Keeps `mask` under `key` (what [`register`] does once the key and the alpha are known; the tests use it).
pub fn register_mask(key: u64, mask: AlphaMask) {
    MASKS.with_borrow_mut(|masks| {
        masks.retain(|(k, _)| *k != key);
        if masks.len() >= MAX_MASKS {
            masks.remove(0);
        }
        masks.push((key, Rc::new(mask)));
    });
}

/// The mask kept under `key`, if any.
pub fn mask(key: u64) -> Option<Rc<AlphaMask>> {
    MASKS.with_borrow(|masks| masks.iter().rev().find(|(k, _)| *k == key).map(|(_, m)| m.clone()))
}
