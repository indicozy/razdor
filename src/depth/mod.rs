//! The built-in depth model: the 2.5D cards of a sprite that has no shipped cut data (mods,
//! community maps). Depth Anything V2 Small runs on the player's machine through tract; the
//! sprite is cut into depth regions (`cut`), and the cut is cached per sprite (`cache`). Pure
//! Rust, no macroquad.
//!
//! With the feature `depth-model` off, the same API compiles to stubs: `DepthModel::load` gives
//! `None`, so no model exists and no sprite gets cards from it.
//!
//! Not here yet: the model file's download and its hash (a release asset), and the switch in the
//! 2.5D view. See `docs/superpowers/specs/2026-11-10-atmosphere-design.md` §2.

use std::path::Path;

use crate::dt::cards::Card;
use crate::dt::gfx::Image;

#[cfg(feature = "depth-model")]
mod cache;
#[cfg(feature = "depth-model")]
mod cut;
#[cfg(feature = "depth-model")]
mod model;
#[cfg(all(test, feature = "depth-model"))]
mod parity;

#[cfg(feature = "depth-model")]
pub use model::{DepthModel, INPUT};

/// Stub: without the feature there is no model, and no model can be loaded.
#[cfg(not(feature = "depth-model"))]
pub struct DepthModel(std::convert::Infallible);

#[cfg(not(feature = "depth-model"))]
impl DepthModel {
    /// Always `None` without the feature.
    pub fn load(_path: &Path) -> Option<DepthModel> {
        None
    }

    /// Unreachable: no `DepthModel` exists without the feature.
    pub fn depth(&self, _rgba: &Image) -> Vec<f32> {
        match self.0 {}
    }
}

/// The cards of a sprite cut with `model`, back to front; empty when the sprite stays one card.
/// With `cache_dir`, a cut found there (by the sprite's key) is returned without running the
/// model, and a new cut is written there. A run that fails gives empty cards and is not cached.
#[cfg(feature = "depth-model")]
pub fn cards_for(model: &DepthModel, img: &Image, cache_dir: Option<&Path>) -> Vec<Card> {
    if img.width == 0 || img.height == 0 {
        return Vec::new();
    }
    let key = crate::dt::cards::sprite_key(img);
    if let Some(cards) = cache_dir.and_then(|dir| cache::read(dir, key, img.width, img.height)) {
        return cards;
    }
    let depth = model.depth(img);
    if depth.len() != (img.width * img.height) as usize {
        return Vec::new();
    }
    let cards = cut::cut(img, &depth);
    if let Some(dir) = cache_dir {
        if let Err(e) = cache::write(dir, key, img.width, img.height, &cards) {
            eprintln!("depth cache {}: {e}", dir.display());
        }
    }
    cards
}

/// Unreachable: no `DepthModel` exists without the feature.
#[cfg(not(feature = "depth-model"))]
pub fn cards_for(model: &DepthModel, _img: &Image, _cache_dir: Option<&Path>) -> Vec<Card> {
    match model.0 {}
}
