//! The CPU-side opacity of the sprite textures the 2.5D pointer picks from (the map atlas and the figure sheets).
//!
//! The pointer's alpha test (`world_view::opaque_hit`) asks whether one texel of a picked sprite is opaque, and must
//! not read the GPU to do it. So each such texture keeps one bit per texel, taken from the image it was made from
//! when it is made ([`register`], in `dt_art`'s `sprite_texture`): set where the alpha is at least [`OPAQUE_ALPHA`].
//! A texture with no mask (a mod's, or one made elsewhere) is not alpha-tested: its picks hit as their quads.

use std::rc::Rc;

use macroquad::prelude::*;
use razdor::dt::gfx::Image;
use razdor::local;

/// The alpha a texel needs to count as opaque for a pick.
pub const OPAQUE_ALPHA: u8 = 128;

/// A texture's opacity, one bit a texel, packed row by row from the top: each row takes `stride` bytes, the texel
/// `x` of a row is bit `x % 8` (least significant first) of byte `x / 8`.
pub struct AlphaMask {
    w: usize,
    h: usize,
    stride: usize,
    bits: Vec<u8>,
}

impl AlphaMask {
    /// The mask of a `w` × `h` texture whose texels' alphas are `alpha` (row by row, from the top; a texel is opaque
    /// when its alpha is at least [`OPAQUE_ALPHA`]). None when there are not `w * h` alphas.
    pub fn from_alpha(w: usize, h: usize, alpha: impl IntoIterator<Item = u8>) -> Option<AlphaMask> {
        let stride = w.div_ceil(8);
        let mut bits = vec![0u8; stride * h];
        let mut n = 0usize;
        for (i, a) in alpha.into_iter().enumerate() {
            if i >= w * h {
                return None;
            }
            if a >= OPAQUE_ALPHA {
                let (x, y) = (i % w, i / w);
                bits[y * stride + x / 8] |= 1 << (x % 8);
            }
            n = i + 1;
        }
        (n == w * h).then_some(AlphaMask { w, h, stride, bits })
    }

    pub fn width(&self) -> usize {
        self.w
    }

    pub fn height(&self) -> usize {
        self.h
    }

    /// Whether the texel `(x, y)` is opaque. Outside the texture it is not.
    pub fn opaque(&self, x: usize, y: usize) -> bool {
        x < self.w && y < self.h && self.bits[y * self.stride + x / 8] & (1 << (x % 8)) != 0
    }
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

/// Keeps the opacity of `img`, the image `tex` was made from. Replaces a mask kept under the same key.
pub fn register(tex: &Texture2D, img: &Image) {
    let (w, h) = (img.width as usize, img.height as usize);
    if let Some(mask) = AlphaMask::from_alpha(w, h, img.rgba.chunks_exact(4).map(|px| px[3])) {
        register_mask(key_of(tex), mask);
    }
}

/// Keeps `mask` under `key` (what [`register`] does once the key and the mask are known; the tests use it).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The threshold is at 128: 127 is transparent, 128 opaque; a 10-texel row packs past its first byte.
    #[test]
    fn the_bit_is_the_alpha_threshold_and_rows_pack_past_a_byte() {
        let (w, h) = (10usize, 3usize);
        let alpha = (0..w * h).map(|i| match i {
            0 => 127,
            1 => OPAQUE_ALPHA,
            9 => 255,
            15 => 200,
            _ => 0,
        });
        let m = AlphaMask::from_alpha(w, h, alpha).expect("30 alphas");
        assert!(!m.opaque(0, 0), "127 is transparent");
        assert!(m.opaque(1, 0), "128 is opaque");
        assert!(m.opaque(9, 0), "the last texel of a row");
        assert!(m.opaque(5, 1), "the sixth texel of the second row is 15");
        assert!(!m.opaque(8, 1));
        assert!(!m.opaque(w, 0) && !m.opaque(0, h), "outside the texture is not opaque");
    }

    /// Too few or too many alphas for the size make no mask.
    #[test]
    fn a_mask_needs_exactly_w_times_h_alphas() {
        assert!(AlphaMask::from_alpha(4, 4, std::iter::repeat_n(255u8, 15)).is_none());
        assert!(AlphaMask::from_alpha(4, 4, std::iter::repeat_n(255u8, 17)).is_none());
        assert!(AlphaMask::from_alpha(4, 4, std::iter::repeat_n(255u8, 16)).is_some());
    }
}
