//! The map atlas's cut (the perf plan's item 6): each sprite's picture is drawn as the box of its opaque texels,
//! not as the whole sprite, so the GPU fills fewer pixels. The atlas keeps its layout (`gfx::pack_atlas`): every
//! sprite stays where it was, so every texel a draw samples is the one it sampled before, and the picture stays
//! byte-identical. A repacked atlas (boxes only) saves the texture memory but moves the texel coordinates by float
//! rounding, which changes a few pixels by up to two levels; that is not taken.
//! Pure: no macroquad, so the rules are tested without a GL context.

use razdor::dt::gfx::Image;

/// A box in a sprite's own pixels: `x0..x1` by `y0..y1` (the whole sprite when nothing is drawn in it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bx {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Bx {
    pub fn w(self) -> u32 {
        self.x1 - self.x0
    }

    pub fn h(self) -> u32 {
        self.y1 - self.y0
    }

    /// The smallest box holding both (`None` stands for an empty one).
    pub fn union(a: Option<Bx>, b: Option<Bx>) -> Option<Bx> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Bx { x0: a.x0.min(b.x0), y0: a.y0.min(b.y0), x1: a.x1.max(b.x1), y1: a.y1.max(b.y1) }),
            (a, None) => a,
            (None, b) => b,
        }
    }
}

/// The picture box of a sprite: its texels with alpha above zero, and those `emit` marks (the lit windows read
/// their emission outside the opaque texels too), one texel a side, clipped to the sprite. The whole sprite when
/// neither is there.
pub fn picture_box(img: &Image, emit: Option<&[u8]>) -> Bx {
    let (w, h) = (img.width, img.height);
    let whole = Bx { x0: 0, y0: 0, x1: w, y1: h };
    if w == 0 || h == 0 {
        return whole;
    }
    let mut found: Option<Bx> = None;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let lit = emit.is_some_and(|e| e.get(i).is_some_and(|&v| v > 0));
            if img.rgba[i * 4 + 3] > 0 || lit {
                found = Bx::union(found, Some(Bx { x0: x, y0: y, x1: x + 1, y1: y + 1 }));
            }
        }
    }
    match found {
        Some(b) => Bx { x0: b.x0.saturating_sub(1), y0: b.y0.saturating_sub(1), x1: (b.x1 + 1).min(w), y1: (b.y1 + 1).min(h) },
        None => whole,
    }
}

/// The non-zero texels of a texture's alpha inside a sprite's rectangle (`w` by `h` texels from `at` of a RGBA texture
/// `tex_w` wide and `tex_h` high): their box in the sprite's pixels, `None` when there are none. The shadows use it:
/// their blur spreads a few texels past a picture's own box.
pub fn nonzero_in(rgba: &[u8], tex_w: u32, tex_h: u32, at: (u32, u32), w: u32, h: u32) -> Option<Bx> {
    let mut found: Option<Bx> = None;
    for y in 0..h.min(tex_h.saturating_sub(at.1)) {
        for x in 0..w.min(tex_w.saturating_sub(at.0)) {
            let i = (((at.1 + y) * tex_w + at.0 + x) * 4 + 3) as usize;
            if rgba[i] > 0 {
                found = Bx::union(found, Some(Bx { x0: x, y0: y, x1: x + 1, y1: y + 1 }));
            }
        }
    }
    found
}

/// The picture of `img` inside `bx` (its own pixels, RGBA).
pub fn crop(img: &Image, bx: Bx) -> Image {
    let (w, h) = (bx.w(), bx.h());
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in bx.y0..bx.y1 {
        let s = ((y * img.width + bx.x0) * 4) as usize;
        rgba.extend_from_slice(&img.rgba[s..s + (w * 4) as usize]);
    }
    Image { width: w, height: h, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, opaque: &[(u32, u32)]) -> Image {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, p) in rgba.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            p[0] = (x * 10) as u8;
            p[1] = (y * 10) as u8;
            p[2] = 77;
            p[3] = if opaque.contains(&(x, y)) { 255 } else { 0 };
        }
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn the_picture_box_is_the_opaque_texels_grown_by_one_clipped_to_the_sprite() {
        let a = img(10, 8, &[(4, 3), (5, 5)]);
        assert_eq!(picture_box(&a, None), Bx { x0: 3, y0: 2, x1: 7, y1: 7 });
        // An opaque texel on the sprite's edge: the grown box stops at the edge.
        let b = img(6, 4, &[(0, 0), (5, 3)]);
        assert_eq!(picture_box(&b, None), Bx { x0: 0, y0: 0, x1: 6, y1: 4 });
        // A transparent sprite keeps its whole rectangle; so does an empty one.
        assert_eq!(picture_box(&img(3, 3, &[]), None), Bx { x0: 0, y0: 0, x1: 3, y1: 3 });
        assert_eq!(picture_box(&img(0, 0, &[]), None), Bx { x0: 0, y0: 0, x1: 0, y1: 0 });
    }

    #[test]
    fn a_lit_texel_outside_the_opaque_ones_widens_the_box() {
        let a = img(10, 8, &[(4, 3)]);
        let mut emit = vec![0u8; 80];
        emit[(6 * 10 + 8) as usize] = 200;
        // The opaque texel (4, 3) and the lit one (8, 6): their box (4..9, 3..7), grown by one, clipped to 10×8.
        assert_eq!(picture_box(&a, Some(&emit)), Bx { x0: 3, y0: 2, x1: 10, y1: 8 });
    }

    #[test]
    fn the_shadow_box_is_the_nonzero_texels_inside_the_sprite() {
        let tex = vec![0u8, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0];
        // A 2×2 texture: only texel (1, 0) has alpha 9.
        assert_eq!(nonzero_in(&tex, 2, 2, (0, 0), 2, 2), Some(Bx { x0: 1, y0: 0, x1: 2, y1: 1 }));
        assert_eq!(nonzero_in(&tex, 2, 2, (1, 1), 1, 1), None);
    }

    #[test]
    fn a_crop_is_the_box_of_the_picture() {
        let a = img(4, 3, &[(1, 1)]);
        let c = crop(&a, Bx { x0: 1, y0: 0, x1: 3, y1: 2 });
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(&c.rgba[0..4], &a.rgba[4..8], "the box's first texel is the sprite's (1, 0)");
    }
}
