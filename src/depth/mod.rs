//! The built-in depth model: the 2.5D cards of a sprite that has no shipped cut data (mods,
//! community maps). Depth Anything V2 Small runs on the player's machine through tract; the
//! sprite is cut into depth regions (`cut`), and the cut is cached per sprite (`cache`). The model
//! file is downloaded on the player's word (`fetch`, the Settings row), and the sprites are cut on
//! background workers (`work`). Pure Rust, no macroquad.
//!
//! With the feature `depth-model` off, the same API compiles to stubs: no model loads, no worker
//! starts, and no sprite gets cards from it. `queue` (the bookkeeping) and `fetch` (the file) need
//! no tract and compile in every build.
//!
//! See `docs/superpowers/specs/2026-11-10-atmosphere-design.md` §2 and `docs/flows/assets-and-drawing.md`
//! («Cards»).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::dt::cards::Card;
use crate::dt::gfx::Image;

pub mod fetch;
pub mod queue;

#[cfg(feature = "depth-model")]
mod cache;
#[cfg(feature = "depth-model")]
mod cut;
#[cfg(feature = "depth-model")]
mod model;
#[cfg(all(test, feature = "depth-model"))]
mod parity;
#[cfg(feature = "depth-model")]
mod work;

#[cfg(feature = "depth-model")]
pub use model::DepthModel;
#[cfg(feature = "depth-model")]
pub use work::{worker_count, Service};

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

/// Stub: without the feature no worker starts.
#[cfg(not(feature = "depth-model"))]
pub struct Service(std::convert::Infallible);

#[cfg(not(feature = "depth-model"))]
impl Service {
    /// Always `None` without the feature.
    pub fn start(_model: PathBuf, _cache: Option<PathBuf>) -> Option<Service> {
        None
    }

    /// Unreachable: no `Service` exists without the feature.
    pub fn room(&self) -> usize {
        match self.0 {}
    }

    /// Unreachable: no `Service` exists without the feature.
    pub fn submit(&mut self, _job: queue::Job) -> bool {
        match self.0 {}
    }

    /// Unreachable: no `Service` exists without the feature.
    pub fn drain(&mut self) -> Vec<queue::Done> {
        match self.0 {}
    }

    /// Unreachable: no `Service` exists without the feature.
    pub fn in_flight(&self) -> usize {
        match self.0 {}
    }
}

/// Whether this build can run the model at all (the feature is on).
pub fn available() -> bool {
    cfg!(feature = "depth-model")
}

/// The folder the cuts are cached in: `cards` under the cache folder (shared by every install,
/// keyed by the sprite's pixels). `None` where there is no cache folder.
pub fn cache_folder() -> Option<PathBuf> {
    crate::platform::cache_dir().map(|d| d.join("cards"))
}

/// The cached cut of a sprite in `dir`: `Some(empty)` for a sprite with one region, `None` when
/// there is no entry (or without the feature).
#[cfg(feature = "depth-model")]
pub fn cached_in(dir: &Path, img: &Image) -> Option<Vec<Card>> {
    cache::read(dir, crate::dt::cards::sprite_key(img), img.width, img.height)
}

/// See the feature build: without the feature nothing is cached.
#[cfg(not(feature = "depth-model"))]
pub fn cached_in(_dir: &Path, _img: &Image) -> Option<Vec<Card>> {
    None
}

/// The cached cut of a sprite in the cache folder (see [`cached_in`]).
pub fn cached(img: &Image) -> Option<Vec<Card>> {
    cached_in(&cache_folder()?, img)
}

/// Building sprites still waiting for a cut (queued or cut now), as the world map last counted
/// them. The Settings row shows only while it is above zero.
static UNKNOWN: AtomicUsize = AtomicUsize::new(0);

/// Records how many building sprites still wait for a cut.
pub fn set_unknown(n: usize) {
    UNKNOWN.store(n, Ordering::Relaxed);
}

/// How many building sprites still wait for a cut.
pub fn unknown() -> usize {
    UNKNOWN.load(Ordering::Relaxed)
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
