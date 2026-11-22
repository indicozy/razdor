//! Marble buttons of any size, for the buttons the game has no art for (the tree's «[ ]» and «Добавить
//! карту…»; the six `Graphics/Windows/Btn1`, `Btn2`, `Btn3` `Up`/`Down` stay drawn from their files). Those six are
//! each a cut of the tiling window marble `Graphics/Windows/Win-marble` (480 × 480) at a fixed offset under a bevel;
//! a button is rebuilt the same way, so it keeps the original's marble grain and bevel instead of a stretched or cut
//! picture. Pure: no macroquad.
//!
//! The bevel is measured on the six files: every pixel is the marble
//! under it as `K·M + C` per channel, with `K` and `C` set by its side (top, left, bottom, right) and its depth from
//! that edge (1–3 the bevel, 4 the inner highlight or shadow), constant along the edge; deeper is the middle's own
//! tone. Depth 0, the outline, is drawn as the original draws its windows' frames ([`perimeter_step`]).

use razdor::dt::gfx::Image;

/// A button's state: its art name's suffix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Up,
    Down,
}

/// Where each original button's top-left pixel sits on `Win-marble` (x, y; the texture tiles, so an offset is taken
/// modulo 480). Measured by normalized cross-correlation of each button's marble inside its bevel (rows and columns
/// 6 from the edge) against every offset of the tiled texture: 0.944–0.976 at these offsets, at most 0.48 anywhere
/// else. `Down` is its `Up` offset minus one in x and y: the pressed button's marble shows one pixel right and down.
pub const ORIGINAL_OFFSETS: [(&str, State, (i32, i32)); 6] = [
    ("Btn1", State::Up, (368, 352)),
    ("Btn1", State::Down, (367, 351)),
    ("Btn2", State::Up, (343, 122)),
    ("Btn2", State::Down, (342, 121)),
    ("Btn3", State::Up, (-18, 249)),
    ("Btn3", State::Down, (-19, 248)),
];

/// The `w` × `h` cut of `marble` whose top-left pixel is the marble's (`ox`, `oy`), the marble repeated in both
/// directions (a negative offset or a cut past the edge wraps around).
pub fn marble_cut(marble: &Image, (ox, oy): (i32, i32), w: u32, h: u32) -> Image {
    let (mw, mh) = (marble.width as i64, marble.height as i64);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as i64 {
        let sy = (oy as i64 + y).rem_euclid(mh);
        for x in 0..w as i64 {
            let sx = (ox as i64 + x).rem_euclid(mw);
            let i = ((sy * mw + sx) * 4) as usize;
            rgba.extend_from_slice(&marble.rgba[i..i + 4]);
        }
    }
    Image { width: w, height: h, rgba }
}

/// One pixel of a bevel layer: the marble under it times `mul` plus `add` per channel (RGB), then mixed toward its
/// own colour `own` by `own_a` (0: the shaded marble, 1: the layer's own pixel, as the steel outline).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerPx {
    pub mul: [f32; 3],
    pub add: [f32; 3],
    pub own: [f32; 3],
    pub own_a: f32,
}

impl LayerPx {
    /// The marble unchanged.
    #[cfg(test)]
    pub const IDENTITY: LayerPx = LayerPx { mul: [1.0; 3], add: [0.0; 3], own: [0.0; 3], own_a: 0.0 };

    fn apply(&self, m: [u8; 3]) -> [u8; 3] {
        std::array::from_fn(|c| {
            let shaded = m[c] as f32 * self.mul[c] + self.add[c];
            (shaded + (self.own[c] - shaded) * self.own_a).round().clamp(0.0, 255.0) as u8
        })
    }
}

/// A bevel layer the size of its button, row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub width: u32,
    pub height: u32,
    pub px: Vec<LayerPx>,
}

impl Layer {
    /// A layer that leaves the marble as it is.
    #[cfg(test)]
    pub fn identity(width: u32, height: u32) -> Self {
        Layer { width, height, px: vec![LayerPx::IDENTITY; (width * height) as usize] }
    }
}

/// How deep the bevel reaches: depths 0..BEVEL_DEPTH from an edge are the bevel's (0 the outline), deeper is the middle.
pub const BEVEL_DEPTH: u32 = 5;

/// A bevel pixel's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Left,
    Bottom,
    Right,
}

/// A per-channel (R, G, B) `(K, C)`: the layer gives `K·M + C` over marble `M`.
type Tone = [(f32, f32); 3];

/// A state's bevel: per side, the tone at depths 1..BEVEL_DEPTH; the middle's; what the top-left corner's diagonal
/// adds at depths 1..BEVEL_DEPTH (the glint, R, G, B); and whether the top-right and bottom-left diagonals are the
/// dark side's.
struct Profile {
    t: [Tone; 4],
    l: [Tone; 4],
    b: [Tone; 4],
    r: [Tone; 4],
    middle: Tone,
    glint: [[f32; 3]; 4],
    dark_diagonals: bool,
}

// The tones are least-squares fits over the three sizes' straight edges; the glint is
// the six files' mean excess on the top-left diagonal over those tones, and the diagonals' side the one that matches
// the files (Up: the dark side's, 0.6 better on the bevel; Down: the light side's, 0.5 better).
/// The raised button: light bevel on top and left, cool dark bevel (red falls most) on the bottom and right; the
/// middle lighter and greener than the window marble; its top-right and bottom-left diagonals are the dark side's.
const UP: Profile = Profile {
    t: [
        [(0.756, 75.5), (0.727, 90.1), (0.689, 75.7)],
        [(0.87, 104.7), (0.913, 127.2), (0.877, 103.6)],
        [(0.799, 86.4), (0.918, 110.7), (0.849, 86.5)],
        [(0.879, 11.3), (0.983, 26.9), (0.926, 11.1)],
    ],
    l: [
        [(0.837, 73.6), (0.917, 82.0), (0.915, 66.5)],
        [(0.915, 102.6), (1.024, 120.7), (1.024, 97.1)],
        [(0.886, 87.1), (1.065, 104.4), (1.064, 80.0)],
        [(0.967, 9.9), (1.185, 17.1), (1.161, 1.0)],
    ],
    b: [
        [(0.634, -8.6), (0.751, -16.1), (0.71, -13.4)],
        [(0.393, -7.5), (0.72, -17.2), (0.641, -15.4)],
        [(0.341, -6.4), (0.816, -18.1), (0.678, -16.1)],
        [(0.906, 0.3), (1.097, -1.4), (1.051, -4.7)],
    ],
    r: [
        [(0.549, -7.0), (0.781, -18.4), (0.809, -19.6)],
        [(0.367, -6.0), (0.714, -16.9), (0.687, -17.8)],
        [(0.386, -7.5), (0.832, -19.9), (0.751, -20.4)],
        [(0.92, 0.2), (1.091, -1.5), (1.088, -7.6)],
    ],
    middle: [(1.013, 5.5), (1.195, 9.0), (1.141, 1.4)],
    glint: [[32.0, 16.0, 28.0], [85.0, 71.0, 84.0], [52.0, 59.0, 55.0], [-2.0, -1.0, -2.0]],
    dark_diagonals: true,
};
/// The pressed button: a dark line at depth 1 on top and left, the light bevel weaker and one pixel deeper, a deep
/// shadow at depths 1–2 on the bottom and right; the middle a little darker than `Up`'s; its top-right and
/// bottom-left diagonals are the light side's.
const DOWN: Profile = Profile {
    t: [
        [(0.155, 37.0), (0.134, 49.3), (0.138, 38.8)],
        [(0.465, 49.4), (0.496, 68.1), (0.469, 50.6)],
        [(0.68, 75.4), (0.761, 94.8), (0.71, 75.4)],
        [(0.755, 67.9), (0.827, 86.9), (0.766, 68.4)],
    ],
    l: [
        [(0.131, 35.1), (0.294, 40.2), (0.225, 33.1)],
        [(0.504, 48.9), (0.569, 65.5), (0.56, 47.4)],
        [(0.762, 72.8), (0.798, 93.1), (0.826, 70.4)],
        [(0.77, 69.0), (0.907, 84.3), (0.924, 62.6)],
    ],
    b: [
        [(0.207, -3.9), (0.437, -11.3), (0.34, -9.3)],
        [(0.137, -2.9), (0.589, -15.1), (0.411, -11.4)],
        [(0.753, -6.5), (0.96, -9.6), (0.866, -10.2)],
        [(0.898, -1.5), (1.078, -2.9), (0.988, -5.4)],
    ],
    r: [
        [(0.126, -2.1), (0.46, -14.0), (0.31, -8.9)],
        [(0.203, -5.1), (0.603, -18.1), (0.438, -14.3)],
        [(0.77, -5.9), (0.944, -10.2), (0.937, -14.8)],
        [(0.868, -0.5), (1.065, -3.0), (1.03, -8.3)],
    ],
    middle: [(0.878, -3.4), (1.054, -0.2), (0.996, -6.9)],
    glint: [[35.0, 19.0, 31.0], [-2.0, -11.0, -6.0], [19.0, 16.0, 17.0], [39.0, 43.0, 39.0]],
    dark_diagonals: false,
};

/// The marble offset of a rebuilt button in `state`: `Btn1`'s (the marble tiles, so any offset serves; `Down` is
/// `Up`'s one pixel back, the pressed marble showing one pixel right and down, as in the files).
pub fn offset(state: State) -> (i32, i32) {
    ORIGINAL_OFFSETS[match state {
        State::Up => 0,
        State::Down => 1,
    }]
    .2
}

/// The step of the outline's walk at which pixel (`x`, `y`) of a `w` × `h` button gets its colour, `None` off the
/// outline: the original's frame of a rectangle (0x48d1d4, one pixel thick) walks the top left to right, the right
/// side top to bottom, the bottom right to left and the left side bottom to top, counting one step per pixel through
/// all four; each corner is written twice and keeps its later step (top-right `w`, bottom-right `w + h`,
/// bottom-left `2w + h`, top-left `2w + 2h − 1`).
pub fn perimeter_step(x: u32, y: u32, w: u32, h: u32) -> Option<u32> {
    if w == 0 || h == 0 || x >= w || y >= h {
        return None;
    }
    if x == 0 {
        Some(2 * w + h + (h - 1 - y))
    } else if y == h - 1 {
        Some(w + h + (w - 1 - x))
    } else if x == w - 1 {
        Some(w + y)
    } else if y == 0 {
        Some(x)
    } else {
        None
    }
}

/// The colour of the outline's step `k`: `steel`'s pixel `k` read row after row (`SteelLine` is 896 × 8: step 896
/// is row 1's first pixel). The original reads past the picture's end unchecked; Razdor wraps around it instead.
pub fn steel_word(steel: &Image, k: u32) -> [f32; 3] {
    let n = (steel.width * steel.height).max(1);
    let i = ((k % n) * 4) as usize;
    match steel.rgba.get(i..i + 3) {
        Some(p) => [p[0] as f32, p[1] as f32, p[2] as f32],
        None => [0.0; 3],
    }
}

/// The side and depth of pixel (`x`, `y`) of a `w` × `h` button in `state`, `None` in the middle. A pixel belongs to
/// its nearest edge; on a tie the top-left corner is the left's and the bottom-right the right's, and the top-right
/// and bottom-left corners split on their diagonal, the diagonal itself the dark side's in `Up`, the light side's in
/// `Down`.
pub fn side_depth(x: u32, y: u32, w: u32, h: u32, state: State) -> Option<(Side, u32)> {
    let (dt, db, dl, dr) = (y, h - 1 - y, x, w - 1 - x);
    let d = dt.min(db).min(dl).min(dr);
    if d >= BEVEL_DEPTH {
        return None;
    }
    let n = BEVEL_DEPTH;
    let dark = profile(state).dark_diagonals;
    if dt < n && dr < n {
        let top = if dark { dt < dr } else { dt <= dr };
        return Some(if top { (Side::Top, dt) } else { (Side::Right, dr) });
    }
    if db < n && dl < n {
        let left = if dark { dl < db } else { dl <= db };
        return Some(if left { (Side::Left, dl) } else { (Side::Bottom, db) });
    }
    let side = [(Side::Left, dl), (Side::Top, dt), (Side::Right, dr), (Side::Bottom, db)]
        .into_iter()
        .find(|&(_, e)| e == d)
        .map_or(Side::Left, |(s, _)| s);
    Some((side, d))
}


fn profile(state: State) -> &'static Profile {
    match state {
        State::Up => &UP,
        State::Down => &DOWN,
    }
}

/// The bevel layer of a `w` × `h` button in `state`: [`side_depth`] and the state's tones, the glint on the top-left
/// corner's diagonal, and the outline from `steel` (`SteelLine`) along [`perimeter_step`].
pub fn bevel(w: u32, h: u32, state: State, steel: &Image) -> Layer {
    let p = profile(state);
    let px = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| match side_depth(x, y, w, h, state) {
            None => shade(&p.middle, [0.0; 3]),
            Some((_, 0)) => {
                let own = perimeter_step(x, y, w, h).map_or([0.0; 3], |k| steel_word(steel, k));
                LayerPx { mul: [0.0; 3], add: [0.0; 3], own, own_a: 1.0 }
            }
            Some((side, d)) => {
                let k = d as usize - 1;
                let tone = match side {
                    Side::Top => &p.t[k],
                    Side::Left => &p.l[k],
                    Side::Bottom => &p.b[k],
                    Side::Right => &p.r[k],
                };
                // The top-left corner's diagonal only (x = y = depth), not where x = y meets another edge.
                let glint = if x == y && x == d { p.glint[k] } else { [0.0; 3] };
                shade(tone, glint)
            }
        })
        .collect();
    Layer { width: w, height: h, px }
}

/// The marble under `tone`, plus `extra` per channel.
fn shade(tone: &Tone, extra: [f32; 3]) -> LayerPx {
    LayerPx { mul: tone.map(|t| t.0), add: std::array::from_fn(|c| tone[c].1 + extra[c]), own: [0.0; 3], own_a: 0.0 }
}

/// A `w` × `h` marble button: the cut of `marble` at `offset` under `layer` (its size). Alpha stays opaque.
pub fn assemble(marble: &Image, offset: (i32, i32), layer: &Layer) -> Image {
    let mut img = marble_cut(marble, offset, layer.width, layer.height);
    for (p, l) in img.rgba.chunks_exact_mut(4).zip(&layer.px) {
        let [r, g, b] = l.apply([p[0], p[1], p[2]]);
        p.copy_from_slice(&[r, g, b, 255]);
    }
    img
}

/// A button `w` × `h` in `state` with the marble at `offset` and the outline from `steel`.
pub fn button(marble: &Image, offset: (i32, i32), w: u32, h: u32, state: State, steel: &Image) -> Image {
    assemble(marble, offset, &bevel(w, h, state, steel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::dt::gfx;
    use razdor::dt::stored::find_file;
    use std::path::Path;

    /// How far inside the edge the marble is free of the bevel in every original button (the bevel reaches 5 px in
    /// from each edge: outline, bevel lines and the inner highlight or shadow).
    const INNER: usize = 6;

    /// The most the rebuilt inner marble may differ from the original's, per channel, as the mean absolute
    /// difference: the two files' compression noise (the best per-channel straight-line fit of the original's
    /// inner marble on the cut leaves 3.4–4.8 on average) plus a margin.
    const INNER_MEAN_ABS_MAX: f64 = 6.0;

    fn synthetic_marble(w: u32, h: u32) -> Image {
        let rgba = (0..h).flat_map(|y| (0..w).flat_map(move |x| [x as u8, y as u8, (x * 7 + y * 13) as u8, 255])).collect();
        Image { width: w, height: h, rgba }
    }

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        img.rgba[i..i + 4].try_into().expect("four bytes")
    }

    /// The cut wraps around the tiled marble both ways: past the right and bottom edges and at negative offsets.
    #[test]
    fn the_marble_cut_wraps_around_the_tile() {
        let m = synthetic_marble(10, 8);
        let c = marble_cut(&m, (-2, 6), 5, 4);
        assert_eq!((c.width, c.height), (5, 4));
        for y in 0..4 {
            for x in 0..5 {
                let (sx, sy) = ((x as i32 - 2).rem_euclid(10) as u32, (y + 6) % 8);
                assert_eq!(px(&c, x, y), px(&m, sx, sy), "({x}, {y}) from ({sx}, {sy})");
            }
        }
        // Whole tiles away is the same cut.
        assert_eq!(marble_cut(&m, (-2 + 30, 6 - 16), 5, 4), c);
    }

    /// The identity layer gives back the cut itself; a layer pixel shades, then mixes toward its own colour.
    #[test]
    fn a_layer_shades_the_marble_then_mixes_its_own_colour() {
        let m = synthetic_marble(10, 8);
        assert_eq!(assemble(&m, (3, 1), &Layer::identity(4, 3)), marble_cut(&m, (3, 1), 4, 3));
        let shade = LayerPx { mul: [0.5, 1.0, 2.0], add: [10.0, -5.0, 0.0], own: [200.0, 0.0, 0.0], own_a: 0.25 };
        // (100·0.5+10, 40−5, 120·2 → 240), then a quarter of the way to (200, 0, 0).
        assert_eq!(shade.apply([100, 40, 120]), [95, 26, 180]);
    }

    /// Sides and depths: the nearest edge; the top-left tie is the left's, the bottom-right the right's; on the
    /// top-right and bottom-left diagonals `Up` takes the dark side, `Down` the light side; the middle has none.
    #[test]
    fn each_pixel_takes_its_nearest_edges_side_and_the_corners_are_mitred() {
        let (w, h) = (20, 12);
        for st in [State::Up, State::Down] {
            assert_eq!(side_depth(10, 0, w, h, st), Some((Side::Top, 0)));
            assert_eq!(side_depth(10, 11, w, h, st), Some((Side::Bottom, 0)));
            assert_eq!(side_depth(0, 6, w, h, st), Some((Side::Left, 0)));
            assert_eq!(side_depth(19, 6, w, h, st), Some((Side::Right, 0)));
            assert_eq!(side_depth(10, 3, w, h, st), Some((Side::Top, 3)));
            assert_eq!(side_depth(10, 5, w, h, st), None, "depth 5 is the middle");
            assert_eq!(side_depth(2, 2, w, h, st), Some((Side::Left, 2)), "top-left tie: the left's");
            assert_eq!(side_depth(17, 9, w, h, st), Some((Side::Right, 2)), "bottom-right tie: the right's");
            assert_eq!(side_depth(18, 2, w, h, st), Some((Side::Right, 1)), "below the top-right diagonal");
            assert_eq!(side_depth(16, 2, w, h, st), Some((Side::Top, 2)), "above it");
            assert_eq!(side_depth(2, 10, w, h, st), Some((Side::Bottom, 1)), "past the bottom-left diagonal");
        }
        assert_eq!(side_depth(17, 2, w, h, State::Up), Some((Side::Right, 2)), "Up: the top-right diagonal is dark");
        assert_eq!(side_depth(2, 9, w, h, State::Up), Some((Side::Bottom, 2)), "Up: the bottom-left diagonal is dark");
        assert_eq!(side_depth(17, 2, w, h, State::Down), Some((Side::Top, 2)), "Down: the top-right diagonal is light");
        assert_eq!(side_depth(2, 9, w, h, State::Down), Some((Side::Left, 2)), "Down: the bottom-left diagonal is light");
    }

    /// The outline's walk is the original's frame (0x48d1d4): top left to right, right side down, bottom right to left,
    /// left side up, one step per pixel; each corner keeps its later step. On 4 × 3: top-right 4, bottom-right 7,
    /// bottom-left 11, top-left 13; every outline pixel has a step, nothing inside has one.
    #[test]
    fn the_outline_walks_the_perimeter_as_the_original_frames_its_windows() {
        let (w, h) = (4, 3);
        assert_eq!(perimeter_step(3, 0, w, h), Some(4), "top-right");
        assert_eq!(perimeter_step(3, 2, w, h), Some(7), "bottom-right");
        assert_eq!(perimeter_step(0, 2, w, h), Some(11), "bottom-left");
        assert_eq!(perimeter_step(0, 0, w, h), Some(13), "top-left");
        assert_eq!(perimeter_step(1, 0, w, h), Some(1));
        assert_eq!(perimeter_step(2, 0, w, h), Some(2));
        assert_eq!(perimeter_step(3, 1, w, h), Some(5));
        assert_eq!(perimeter_step(2, 2, w, h), Some(8));
        assert_eq!(perimeter_step(1, 2, w, h), Some(9));
        assert_eq!(perimeter_step(0, 1, w, h), Some(12));
        assert_eq!(perimeter_step(1, 1, w, h), None, "inside");
        assert_eq!(perimeter_step(4, 0, w, h), None, "outside");
        // Any size: the outline's pixels get distinct steps below 2w + 2h, the inner ones none.
        for (w, h) in [(72, 36), (36, 36), (260, 36), (5, 9)] {
            let mut seen = std::collections::HashSet::new();
            for y in 0..h {
                for x in 0..w {
                    let edge = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                    match perimeter_step(x, y, w, h) {
                        Some(k) => assert!(edge && k < 2 * w + 2 * h && seen.insert(k), "{w}×{h} ({x}, {y}): {k}"),
                        None => assert!(!edge, "{w}×{h} ({x}, {y}) has no step"),
                    }
                }
            }
            assert_eq!(seen.len() as u32, 2 * (w + h) - 4);
        }
    }

    /// A step's colour is the steel picture's pixel `k` read row after row: past the first row's end, the next row.
    #[test]
    fn a_steps_colour_is_the_steel_pictures_pixel_read_row_after_row() {
        let steel = Image { width: 8, height: 2, rgba: (0..16u8).flat_map(|i| [i * 10, i, 255 - i, 255]).collect() };
        assert_eq!(steel_word(&steel, 3), [30.0, 3.0, 252.0]);
        assert_eq!(steel_word(&steel, 8), [80.0, 8.0, 247.0], "step 8: row 1's first pixel");
        assert_eq!(steel_word(&steel, 17), steel_word(&steel, 1), "past the picture: around again");
    }

    /// A layer for any size; the middle takes the middle's tone; the glint adds to the top-left corner's diagonal
    /// only; the outline is opaque steel at its step.
    #[test]
    fn the_bevel_fits_any_size_with_its_glint_and_steel_outline() {
        let steel = Image { width: 8, height: 2, rgba: (0..16u8).flat_map(|i| [i * 10, i, 255 - i, 255]).collect() };
        for (w, h) in [(72, 72), (300, 36), (36, 200), (10, 10)] {
            assert_eq!(bevel(w, h, State::Up, &steel).px.len(), (w * h) as usize);
        }
        let sq = bevel(72, 72, State::Down, &steel);
        assert_eq!(sq.px[36 * 72 + 36].mul, DOWN.middle.map(|t| t.0));
        let l = bevel(72, 36, State::Up, &steel);
        let at = |x: usize, y: usize| l.px[y * 72 + x];
        let with_glint: [f32; 3] = std::array::from_fn(|c| UP.l[1][c].1 + UP.glint[1][c]);
        assert_eq!(at(2, 2).add, with_glint, "(2, 2): the left's depth 2 plus the glint");
        assert_eq!(at(3, 2).add, UP.t[1].map(|t| t.1), "off the diagonal no glint");
        assert_eq!(at(33, 33).add, UP.b[1].map(|t| t.1), "x = y at the bottom edge is no corner");
        let o = at(30, 0);
        assert_eq!((o.own, o.own_a), (steel_word(&steel, 30), 1.0));
    }

    /// An original button: its file name, state, marble offset and picture.
    type Original = (String, State, (i32, i32), Image);

    /// The six original buttons and the marble, from the install (`RAZDOR_DT_DIR`); `None` without one.
    fn real_art() -> Option<(Image, Vec<Original>)> {
        let dir = std::env::var_os("RAZDOR_DT_DIR")?;
        let load = |name: &str| {
            let path = find_file(Path::new(&dir), &format!("Graphics/Windows/{name}.lit")).expect(name);
            gfx::decode_file(&path).expect(name).into_iter().next().expect(name)
        };
        let buttons = ORIGINAL_OFFSETS
            .iter()
            .map(|&(name, state, off)| {
                let file = format!("{name}{state:?}");
                let img = load(&file);
                (file, state, off, img)
            })
            .collect();
        Some((load("Win-marble"), buttons))
    }

    /// `SteelLine` from the install (`RAZDOR_DT_DIR`).
    fn steel_line() -> Option<Image> {
        let dir = std::env::var_os("RAZDOR_DT_DIR")?;
        let path = find_file(Path::new(&dir), "Graphics/Windows/SteelLine.lit").expect("SteelLine.lit");
        gfx::decode_file(&path).expect("SteelLine").into_iter().next()
    }

    fn lum(p: &[u8]) -> f64 {
        0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64
    }

    /// The inner marble (`INNER` px in from every edge) of `a` and `b`, pixel by pixel.
    fn inner_pairs<'a>(a: &'a Image, b: &'a Image) -> impl Iterator<Item = (&'a [u8], &'a [u8])> + 'a {
        let (w, h) = (a.width as usize, a.height as usize);
        (INNER..h - INNER).flat_map(move |y| {
            (INNER..w - INNER).map(move |x| {
                let i = (y * w + x) * 4;
                (&a.rgba[i..i + 4], &b.rgba[i..i + 4])
            })
        })
    }

    /// Each original button's marble is `Win-marble` at its offset: the luminance of its inner marble correlates
    /// with the cut at 0.94 or more (a different offset reaches at most 0.48).
    #[test]
    fn real_art_each_button_is_win_marble_at_its_offset() {
        let Some((marble, buttons)) = real_art() else {
            eprintln!("RAZDOR_DT_DIR unset: skipped");
            return;
        };
        for (file, _, off, img) in &buttons {
            let cut = marble_cut(&marble, *off, img.width, img.height);
            let pairs: Vec<(f64, f64)> = inner_pairs(img, &cut).map(|(a, b)| (lum(a), lum(b))).collect();
            let n = pairs.len() as f64;
            let (ma, mb) = (pairs.iter().map(|p| p.0).sum::<f64>() / n, pairs.iter().map(|p| p.1).sum::<f64>() / n);
            let cov: f64 = pairs.iter().map(|p| (p.0 - ma) * (p.1 - mb)).sum();
            let va: f64 = pairs.iter().map(|p| (p.0 - ma).powi(2)).sum();
            let vb: f64 = pairs.iter().map(|p| (p.1 - mb).powi(2)).sum();
            let corr = cov / (va * vb).sqrt();
            assert!(corr >= 0.94, "{file}: the inner marble correlates {corr:.3} with Win-marble at {off:?}");
        }
    }

    /// The rebuilt buttons' inner marble matches the original's within compression noise, per channel, for all
    /// six (the originals' inner marble is not the bare cut: `Up` is lighter and greener, `Down` a little darker;
    /// with the identity bevel this was off by up to 18.6 in green).
    #[test]
    fn real_art_the_rebuilt_inner_marble_matches_all_six_buttons() {
        let (Some((marble, buttons)), Some(steel)) = (real_art(), steel_line()) else {
            eprintln!("RAZDOR_DT_DIR unset: skipped");
            return;
        };
        let mut off_by = Vec::new();
        for (file, state, off, img) in &buttons {
            let ours = button(&marble, *off, img.width, img.height, *state, &steel);
            let n = inner_pairs(img, &ours).count() as f64;
            let mean: Vec<f64> = (0..3)
                .map(|c| inner_pairs(img, &ours).map(|(a, b)| (a[c] as f64 - b[c] as f64).abs()).sum::<f64>() / n)
                .collect();
            if mean.iter().any(|&m| m > INNER_MEAN_ABS_MAX) {
                off_by.push(format!("{file}: mean |diff| R {:.1} G {:.1} B {:.1}", mean[0], mean[1], mean[2]));
            }
        }
        assert!(off_by.is_empty(), "inner marble off by more than {INNER_MEAN_ABS_MAX}: {off_by:#?}");
    }

    /// The bounds of the rebuilt buttons against the six files, by region, as the mean over pixels of the mean
    /// absolute difference of the three channels. The middle: compression noise (3.5–4.8) plus a margin. The bevel
    /// (depths 1–4): at most the file's own middle plus 1. The outline is not compared: it is the windows' steel
    /// frame (Razdor's), not the files' own.
    const MIDDLE_MAX: f64 = 5.5;
    const BEVEL_OVER_MIDDLE_MAX: f64 = 1.0;

    /// Columns of `Btn3` (both states) left out of the bevel and middle bounds: there the files' marble near the top
    /// edge differs from `Win-marble` (17–20 darker, then 14 lighter, in `Up` and `Down` alike, so the marble's, not
    /// the bevel's); with them the bevel of `Btn3Up` is 6.0 against its middle's 4.8.
    const BTN3_MARBLE_SPOT: std::ops::Range<u32> = 80..112;

    /// The rule rebuilds all six original buttons from `Win-marble`: middle and bevel within compression noise; the
    /// outline is `SteelLine` along the walk.
    #[test]
    fn real_art_the_rule_rebuilds_all_six_buttons() {
        let (Some((marble, buttons)), Some(steel)) = (real_art(), steel_line()) else {
            eprintln!("RAZDOR_DT_DIR unset: skipped");
            return;
        };
        let mut over = Vec::new();
        for (file, state, off, img) in &buttons {
            let (w, h) = (img.width, img.height);
            let ours = button(&marble, *off, w, h, *state, &steel);
            let mut sums = [(0.0f64, 0usize); 2]; // bevel, middle
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    if let Some(k) = perimeter_step(x, y, w, h) {
                        let want = steel_word(&steel, k).map(|v| v as u8);
                        assert_eq!(ours.rgba[i..i + 3], want, "{file} ({x}, {y}): the outline is SteelLine's step {k}");
                        continue;
                    }
                    if file.starts_with("Btn3") && BTN3_MARBLE_SPOT.contains(&x) {
                        continue;
                    }
                    let e = (0..3).map(|c| (img.rgba[i + c] as f64 - ours.rgba[i + c] as f64).abs()).sum::<f64>() / 3.0;
                    let region = usize::from(side_depth(x, y, w, h, *state).is_none());
                    sums[region].0 += e;
                    sums[region].1 += 1;
                }
            }
            let [bevel_e, middle] = sums.map(|(s, n)| s / n as f64);
            let line = format!("{file}: bevel {bevel_e:.2} middle {middle:.2}");
            eprintln!("{line}");
            if middle > MIDDLE_MAX || bevel_e > middle + BEVEL_OVER_MIDDLE_MAX {
                over.push(line);
            }
        }
        assert!(over.is_empty(), "over the bounds (middle {MIDDLE_MAX}, bevel middle + {BEVEL_OVER_MIDDLE_MAX}): {over:#?}");
    }
}
