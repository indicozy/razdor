//! Heights and normals (spec §3.3, §3.4): the land of the whole converted world, margin ring included.
//!
//! A land vertex sits 128 units from its neighbours (`grid::VERT_UNITS`). The field covers the
//! vertices of the cells `-1..=cells` on each axis, so the one-cell margin ring of §3.4 is in it.
//! Heights are multiples of 8 and neighbouring vertices differ by at most 1016 units, the range of
//! VHGT (`tes3`). Everything is integer arithmetic; the noise is splitmix64, so the output does not
//! depend on the platform.

use crate::grid::{Grid, Rect, CELL_VERTS, VERT_UNITS};
use crate::input::Map;
use crate::textures::{DEEP_SEA, ROCK};
use std::collections::VecDeque;

/// VHGT's step: every height is a multiple of this, in units.
pub const STEP: i64 = 8;
/// The largest height difference between neighbouring vertices (VHGT holds ±127 steps).
pub const MAX_STEP: i64 = 1016;
/// Shore floor of a land tile's base (§3.3 step 2).
const SHORE_FLOOR: i64 = 24;
/// Lowest height of a flattened building footprint (§3.3 step 4).
const FLAT_FLOOR: i64 = 48;
/// Height of a bridge's deck edge on the land beside it (§3.3 step 4).
pub const DECK_HEIGHT: i64 = 64;
/// Peak of a hill of side 1 (§3.3 step 5), times the side.
const HILL_PEAK: i64 = 160;
/// Peak of a mountain of side 1 (§3.3 step 6), times the side.
const MOUNTAIN_PEAK: i64 = 900;
/// Amplitude of the land noise (§3.3 step 7).
const NOISE_AMP: u64 = 12;
/// Vertices between two noise lattice points.
const NOISE_SPACING: i64 = 4;
/// Keys the noise stream apart from any other use of the seed.
const PURPOSE_NOISE: u64 = 1;
/// Building kinds that are bridges (`docs/reference/original-mechanics/world.md`, §7.1).
const BRIDGE_KINDS: [i64; 2] = [13, 14];

/// The bounds of the vertex field: `(i0, i1, j0, j1)`, inclusive, for the cells `-1..=cells`.
pub fn bounds(grid: &Grid) -> (i64, i64, i64, i64) {
    let (cells_x, cells_y) = grid.cells();
    let side = CELL_VERTS - 1;
    (-side, side * (cells_x + 1), -side, side * (cells_y + 1))
}

/// Base height of a surface code, in units (§3.3 step 1). Codes 0-2 are water.
fn base_of(code: u8) -> i64 {
    match code {
        0 => -200,
        1 => -400,
        2 => -1200,
        3 => 40,
        4 => 64,
        5 => 56,
        6 | 7 => 96,
        8 => 24,
        9 => -56,
        10 => 48,
        11 | 13 => 96,
        12 => 160,
        14 => 128,
        15 => 280,
        _ => 0,
    }
}

/// The map's surfaces, with the tiles around it. A tile is `(x, k)`: column `x` and row `k` counted
/// from the south, as the world's Y grows north. Inside the map the codes are the file's.
pub struct Terrain {
    grid: Grid,
    codes: Vec<u8>,
    /// Distance in tiles (8 neighbours) to the nearest water tile, for each tile of the map;
    /// `i64::MAX` where the map has no water.
    shore: Vec<i64>,
}

impl Terrain {
    /// Builds the terrain of `codes` (row-major from the north, `grid.width` wide).
    pub fn new(grid: Grid, codes: Vec<u8>) -> Terrain {
        let (w, h) = (grid.width as usize, grid.height as usize);
        let mut shore = vec![i64::MAX; w * h];
        let mut queue = VecDeque::new();
        for (idx, &code) in codes.iter().enumerate() {
            if code <= 2 {
                shore[idx] = 0;
                queue.push_back(idx);
            }
        }
        while let Some(idx) = queue.pop_front() {
            let (x, y) = ((idx % w) as i64, (idx / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if (dx == 0 && dy == 0) || nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let n = ny as usize * w + nx as usize;
                    if shore[n] == i64::MAX {
                        shore[n] = shore[idx] + 1;
                        queue.push_back(n);
                    }
                }
            }
        }
        Terrain { grid, codes, shore }
    }

    fn index(&self, x: i64, k: i64) -> Option<usize> {
        let (w, h) = (self.grid.width, self.grid.height);
        if x < 0 || k < 0 || x >= w || k >= h {
            return None;
        }
        Some(((h - 1 - k) * w + x) as usize)
    }

    /// Surface code and base height of tile `(x, k)`, before the blur (§3.3 step 2, §3.4).
    /// Inside the map: the code's base, a land tile's base pulled down near the water. Outside it,
    /// the margin ring: deep sea next to water, else a rock ridge that rises 2000 over two tiles.
    pub fn tile(&self, x: i64, k: i64) -> (u8, i64) {
        if let Some(idx) = self.index(x, k) {
            let code = self.codes[idx];
            let mut base = base_of(code);
            if code > 2 && base > 0 {
                base = (base * self.shore[idx].min(2) / 2).max(SHORE_FLOOR);
            }
            return (code, base);
        }
        let (w, h) = (self.grid.width, self.grid.height);
        let edge = self.codes[self.index(x.clamp(0, w - 1), k.clamp(0, h - 1)).unwrap_or(0)];
        let outside = |v: i64, n: i64| {
            if v < 0 {
                -v
            } else if v >= n {
                v - n + 1
            } else {
                0
            }
        };
        let d = outside(x, w).max(outside(k, h));
        if edge <= 2 {
            (DEEP_SEA, base_of(DEEP_SEA))
        } else {
            (ROCK, 1000 * d.min(2))
        }
    }

    /// The bilinear blend of the four tiles around the world point `(wx, wy)` (§3.3 step 3).
    fn blend(&self, wx: i64, wy: i64) -> i64 {
        let t = self.grid.tile;
        let (sx, sy) = (wx - t / 2, wy - t / 2);
        let (xa, fx) = (sx.div_euclid(t), sx.rem_euclid(t));
        let (ka, fy) = (sy.div_euclid(t), sy.rem_euclid(t));
        let b = |x: i64, k: i64| self.tile(x, k).1;
        let v = b(xa, ka) * (t - fx) * (t - fy)
            + b(xa + 1, ka) * fx * (t - fy)
            + b(xa, ka + 1) * (t - fx) * fy
            + b(xa + 1, ka + 1) * fx * fy;
        v.div_euclid(t * t)
    }
}

/// Heights and normals of the vertex field, row-major with `j` (north) as the row.
#[derive(Debug)]
pub struct HeightField {
    i0: i64,
    j0: i64,
    ni: usize,
    nj: usize,
    heights: Vec<i32>,
    normals: Vec<[i8; 3]>,
}

impl HeightField {
    /// A field over `bounds` whose heights come from `f(i, j)`; the normals follow from them.
    pub fn from_fn(bounds: (i64, i64, i64, i64), f: impl Fn(i64, i64) -> i32) -> HeightField {
        let (i0, i1, j0, j1) = bounds;
        let mut heights = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                heights.push(f(i, j));
            }
        }
        HeightField::from_heights(bounds, heights)
    }

    fn from_heights(bounds: (i64, i64, i64, i64), heights: Vec<i32>) -> HeightField {
        let (i0, i1, j0, j1) = bounds;
        let (ni, nj) = ((i1 - i0 + 1) as usize, (j1 - j0 + 1) as usize);
        let at = |i: i64, j: i64| (j - j0) as usize * ni + (i - i0) as usize;
        let mut normals = Vec::with_capacity(heights.len());
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (lo, hi) = ((i - 1).max(i0), (i + 1).min(i1));
                let (down, up) = ((j - 1).max(j0), (j + 1).min(j1));
                let sx = (heights[at(hi, j)] - heights[at(lo, j)]) as f64 / (((hi - lo) * VERT_UNITS) as f64);
                let sy = (heights[at(i, up)] - heights[at(i, down)]) as f64 / (((up - down) * VERT_UNITS) as f64);
                let (nx, ny, nz) = (-sx, -sy, 1.0);
                let len = (nx * nx + ny * ny + nz * nz).sqrt();
                normals.push([scale(nx / len), scale(ny / len), scale(nz / len)]);
            }
        }
        HeightField { i0, j0, ni, nj, heights, normals }
    }

    fn index(&self, i: i64, j: i64) -> usize {
        (j - self.j0) as usize * self.ni + (i - self.i0) as usize
    }

    /// The height of global vertex `(i, j)`, in units.
    pub fn height(&self, i: i64, j: i64) -> i32 {
        self.heights[self.index(i, j)]
    }

    /// The normal of global vertex `(i, j)`, x, y, z as i8 (OpenMW's VNML).
    pub fn normal(&self, i: i64, j: i64) -> [i8; 3] {
        self.normals[self.index(i, j)]
    }

    /// The lowest and the highest height of the field.
    pub fn range(&self) -> (i32, i32) {
        let lo = self.heights.iter().copied().min().unwrap_or(0);
        let hi = self.heights.iter().copied().max().unwrap_or(0);
        (lo, hi)
    }

    /// The field as a binary PGM (`P5`), one pixel per vertex, north at the top, the lowest height
    /// black and the highest white. For a look at the terrain only; it holds game-derived numbers.
    pub fn pgm(&self) -> Vec<u8> {
        let (lo, hi) = self.range();
        let mut out = format!("P5\n{} {}\n255\n", self.ni, self.nj).into_bytes();
        for jj in (0..self.nj).rev() {
            for ii in 0..self.ni {
                let h = i64::from(self.heights[jj * self.ni + ii]);
                let v = if hi > lo { (h - i64::from(lo)) * 255 / i64::from(hi - lo) } else { 0 };
                out.push(v as u8);
            }
        }
        out
    }
}

fn scale(v: f64) -> i8 {
    (v * 127.0).round().clamp(-127.0, 127.0) as i8
}

fn ceil_div(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

/// Calls `f(i, j)` for every vertex of `rect` (inclusive) inside `bounds`.
fn each_vertex(rect: Rect, bounds: (i64, i64, i64, i64), mut f: impl FnMut(i64, i64)) {
    let (i0, i1, j0, j1) = bounds;
    let (ilo, ihi) = (ceil_div(rect.west, VERT_UNITS).max(i0), rect.east.div_euclid(VERT_UNITS).min(i1));
    let (jlo, jhi) = (ceil_div(rect.south, VERT_UNITS).max(j0), rect.north.div_euclid(VERT_UNITS).min(j1));
    for j in jlo..=jhi {
        for i in ilo..=ihi {
            f(i, j);
        }
    }
}

fn grow(rect: Rect, by: i64) -> Rect {
    Rect { west: rect.west - by, east: rect.east + by, south: rect.south - by, north: rect.north + by }
}

/// The height a point gets inside a hill or mountain square of side `peak` (§3.3 steps 5 and 6):
/// zero on its edge, `peak` at its centre, a smooth rise between.
fn bump(x: i64, y: i64, rect: Rect, peak: i64) -> i64 {
    if x < rect.west || x > rect.east || y < rect.south || y > rect.north {
        return 0;
    }
    let u = (x - rect.west).min(rect.east - x).min(y - rect.south).min(rect.north - y);
    let half = (rect.east - rect.west).min(rect.north - rect.south) / 2;
    let t = (u * 1000 / half).min(1000);
    peak * smoothstep_milli(t) / 1000
}

/// `3t² − 2t³` for `t` in thousandths, the result in thousandths.
fn smoothstep_milli(t: i64) -> i64 {
    (3 * t * t * 1000 - 2 * t * t * t) / 1_000_000
}

pub(crate) fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// One value from a splitmix64 stream keyed by `keys` (§4: seed, purpose, then the coordinates).
pub(crate) fn keyed(keys: &[u64]) -> u64 {
    keys.iter().fold(0u64, |acc, &k| splitmix64(acc.wrapping_add(k)))
}

/// Value noise in `-12..=12` at vertex `(i, j)`: random values on a lattice of every fourth vertex,
/// interpolated bilinearly.
fn noise(seed: u64, i: i64, j: i64) -> i64 {
    let s = NOISE_SPACING;
    let (li, fi) = (i.div_euclid(s), i.rem_euclid(s));
    let (lj, fj) = (j.div_euclid(s), j.rem_euclid(s));
    let r = |a: i64, b: i64| {
        (keyed(&[seed, PURPOSE_NOISE, a as u64, b as u64]) % (2 * NOISE_AMP + 1)) as i64 - NOISE_AMP as i64
    };
    let v = r(li, lj) * (s - fi) * (s - fj)
        + r(li + 1, lj) * fi * (s - fj)
        + r(li, lj + 1) * (s - fi) * fj
        + r(li + 1, lj + 1) * fi * fj;
    v.div_euclid(s * s)
}

/// One 3×3 box blur over the vertex grid, the edges averaging the vertices that exist.
fn box_blur(h: &[i64], ni: usize, nj: usize) -> Vec<i64> {
    let mut out = vec![0i64; ni * nj];
    for j in 0..nj {
        for i in 0..ni {
            let (mut sum, mut n) = (0i64, 0i64);
            for dj in -1i64..=1 {
                for di in -1i64..=1 {
                    let (a, b) = (i as i64 + di, j as i64 + dj);
                    if a < 0 || b < 0 || a >= ni as i64 || b >= nj as i64 {
                        continue;
                    }
                    sum += h[b as usize * ni + a as usize];
                    n += 1;
                }
            }
            out[j * ni + i] = (sum + n / 2).div_euclid(n);
        }
    }
    out
}

/// Lowers the higher of two neighbours until they differ by at most `MAX_STEP`; true if it changed.
fn lower_pair(h: &mut [i64], a: usize, b: usize) -> bool {
    let d = h[a] - h[b];
    if d.abs() <= MAX_STEP {
        return false;
    }
    let excess = d.abs() - MAX_STEP;
    if d > 0 {
        h[a] -= excess;
    } else {
        h[b] -= excess;
    }
    true
}

/// Repeats the lowering over every neighbour pair until no pair differs by more than `MAX_STEP`.
fn clamp_steps(h: &mut [i64], ni: usize, nj: usize) {
    loop {
        let mut changed = false;
        for j in 0..nj {
            for i in 0..ni {
                let k = j * ni + i;
                if i + 1 < ni {
                    changed |= lower_pair(h, k, k + 1);
                }
                if j + 1 < nj {
                    changed |= lower_pair(h, k, k + ni);
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Builds the heights of the whole converted world (§3.3 steps 3 to 9).
pub fn build(terrain: &Terrain, map: &Map) -> HeightField {
    let grid = terrain.grid;
    let bounds = bounds(&grid);
    let (i0, i1, j0, j1) = bounds;
    let (ni, nj) = ((i1 - i0 + 1) as usize, (j1 - j0 + 1) as usize);
    let at = |i: i64, j: i64| (j - j0) as usize * ni + (i - i0) as usize;
    let in_map = |x: i64, y: i64| x >= 0 && y >= 0 && x < grid.width && y < grid.height;
    let tile_idx = |x: i64, y: i64| (y * grid.width + x) as usize;

    // Step 3: the bilinear blend, then two box blurs.
    let mut h = vec![0i64; ni * nj];
    for j in j0..=j1 {
        for i in i0..=i1 {
            h[at(i, j)] = terrain.blend(i * VERT_UNITS, j * VERT_UNITS);
        }
    }
    for _ in 0..2 {
        h = box_blur(&h, ni, nj);
    }

    // Step 4: buildings are flat (footprint and a one-tile apron); bridges keep the water bed, but
    // the land beside a bridge rises to the deck along the shared edge.
    let mut flat = vec![false; ni * nj];
    let mut bridge = vec![false; (grid.width * grid.height) as usize];
    for b in &map.buildings {
        if BRIDGE_KINDS.contains(&b.kind) {
            for (x, y) in Grid::footprint_tiles(b.x, b.y, b.size_x, b.size_y) {
                if in_map(x, y) {
                    bridge[tile_idx(x, y)] = true;
                }
            }
        }
    }
    for b in &map.buildings {
        if BRIDGE_KINDS.contains(&b.kind) {
            continue;
        }
        let tiles = Grid::footprint_tiles(b.x, b.y, b.size_x, b.size_y);
        if !tiles.iter().any(|&(x, y)| in_map(x, y)) {
            continue;
        }
        let rect = grid.footprint(b.x, b.y, b.size_x, b.size_y);
        let (mut sum, mut n) = (0i64, 0i64);
        each_vertex(rect, bounds, |i, j| {
            sum += h[at(i, j)];
            n += 1;
        });
        if n == 0 {
            continue;
        }
        let value = ((sum + n / 2).div_euclid(n)).max(FLAT_FLOOR);
        each_vertex(grow(rect, grid.tile), bounds, |i, j| {
            h[at(i, j)] = value;
            flat[at(i, j)] = true;
        });
    }
    for y in 0..grid.height {
        for x in 0..grid.width {
            if !bridge[tile_idx(x, y)] {
                continue;
            }
            let r = grid.footprint(x, y, 1, 1);
            let edges = [
                (x + 1, y, Rect { west: r.east, east: r.east, south: r.south, north: r.north }),
                (x - 1, y, Rect { west: r.west, east: r.west, south: r.south, north: r.north }),
                (x, y - 1, Rect { west: r.west, east: r.east, south: r.north, north: r.north }),
                (x, y + 1, Rect { west: r.west, east: r.east, south: r.south, north: r.south }),
            ];
            for (nx, ny, edge) in edges {
                if !in_map(nx, ny) || bridge[tile_idx(nx, ny)] || terrain.codes[tile_idx(nx, ny)] <= 2 {
                    continue;
                }
                each_vertex(edge, bounds, |i, j| {
                    h[at(i, j)] = DECK_HEIGHT;
                    flat[at(i, j)] = true;
                });
            }
        }
    }

    // Steps 5 and 6: hills (added on top) and mountains (added after the blur, so they stay steep).
    let mut hill = vec![0i64; ni * nj];
    let mut mountain = vec![0i64; ni * nj];
    for o in map.kept_objects() {
        if !(1..=7).contains(&o.class) {
            continue; // class 8 (stones) keeps the land as it is
        }
        let f = (o.sprite / 10).max(1);
        let rect = grid.footprint(o.x, o.y, f, f);
        let (peak, field) = if o.class <= 4 {
            (HILL_PEAK * f, &mut hill)
        } else {
            (MOUNTAIN_PEAK * f, &mut mountain)
        };
        each_vertex(rect, bounds, |i, j| {
            let v = bump(i * VERT_UNITS, j * VERT_UNITS, rect, peak);
            let slot = &mut field[at(i, j)];
            if v > *slot {
                *slot = v;
            }
        });
    }

    // Step 7: noise on land (a vertex whose tile is not water); flat areas stay flat.
    for j in j0..=j1 {
        for i in i0..=i1 {
            let k = at(i, j);
            if flat[k] {
                continue;
            }
            let mut v = h[k] + hill[k] + mountain[k];
            let (tx, tk) = ((i * VERT_UNITS).div_euclid(grid.tile), (j * VERT_UNITS).div_euclid(grid.tile));
            if terrain.tile(tx, tk).0 > 2 {
                v += noise(map.info.generator_seed, i, j);
            }
            h[k] = v;
        }
    }

    // Step 8: quantise to steps of 8, then clamp the neighbour steps.
    for v in h.iter_mut() {
        *v = (*v + STEP / 2).div_euclid(STEP) * STEP;
    }
    clamp_steps(&mut h, ni, nj);

    HeightField::from_heights(bounds, h.into_iter().map(|v| v as i32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::synthetic;
    use serde_json::json;

    const T: i64 = 1024;

    fn world(w: usize, h: usize, codes: &[u8], objects: serde_json::Value, buildings: serde_json::Value) -> (Map, Terrain) {
        let map = synthetic(w, h, codes, objects, buildings);
        let terrain = Terrain::new(Grid::new(w, h, T).unwrap(), map.tile_codes().unwrap());
        (map, terrain)
    }

    #[test]
    fn the_field_covers_the_margin_ring() {
        let grid = Grid::new(16, 16, T).unwrap();
        assert_eq!(bounds(&grid), (-64, 192, -64, 192)); // 2 cells: 4 cells of 64 vertices, the ring too
        let grid = Grid::new(50, 50, T).unwrap();
        assert_eq!(bounds(&grid), (-64, 512, -64, 512)); // 7 cells: 9 cells of 64 vertices
    }

    #[test]
    fn water_tile_centres_are_at_least_200_deep() {
        let codes: Vec<u8> = (0..36).map(|k| [0u8, 1, 2][k % 3]).collect();
        let (map, terrain) = world(6, 6, &codes, json!([]), json!([]));
        let field = build(&terrain, &map);
        for y in 0..6i64 {
            for x in 0..6i64 {
                let (i, j) = ((x * T + T / 2) / VERT_UNITS, ((6 - 1 - y) * T + T / 2) / VERT_UNITS);
                assert!(field.height(i, j) <= -200, "tile ({x}, {y}): {}", field.height(i, j));
            }
        }
    }

    #[test]
    fn a_footprint_and_its_apron_are_flat() {
        let codes = vec![5u8; 400];
        let (map, terrain) = world(20, 20, &codes, json!([]), json!([{ "x": 10, "y": 10, "size_x": 4, "size_y": 4, "kind": 3 }]));
        let field = build(&terrain, &map);
        let want = field.height(56, 72);
        for i in 48..=96 {
            for j in 64..=112 {
                assert_eq!(field.height(i, j), want, "vertex ({i}, {j})");
            }
        }
        assert!(want >= 48);
    }

    #[test]
    fn heights_are_steps_of_8_and_neighbours_differ_by_at_most_1016() {
        let codes: Vec<u8> = (0..576).map(|k| (((k % 24) * 5 + (k / 24) * 3) % 16) as u8).collect();
        let objects = json!([
            { "x": 5, "y": 5, "sprite": 20, "class": 5 },
            { "x": 15, "y": 12, "sprite": 30, "class": 2 },
            { "x": 10, "y": 10, "sprite": 40, "class": 9 }
        ]);
        let buildings = json!([{ "x": 18, "y": 18, "size_x": 2, "size_y": 2, "kind": 3 }]);
        let (map, terrain) = world(24, 24, &codes, objects, buildings);
        let field = build(&terrain, &map);
        let (i0, i1, j0, j1) = bounds(&Grid::new(24, 24, T).unwrap());
        for j in j0..=j1 {
            for i in i0..=i1 {
                assert_eq!(field.height(i, j) % 8, 0);
                if i < i1 {
                    assert!((field.height(i, j) - field.height(i + 1, j)).abs() <= 1016);
                }
                if j < j1 {
                    assert!((field.height(i, j) - field.height(i, j + 1)).abs() <= 1016);
                }
            }
        }
    }

    /// The largest step between two neighbouring vertices inside the square of a massif or hill at
    /// tile `(x, y)` (both vertices inside the square).
    fn steepest_step(field: &HeightField, x: i64, y: i64) -> i64 {
        let r = Grid::new(20, 20, T).unwrap().footprint(x, y, 1, 1);
        let (ilo, ihi) = (r.west / VERT_UNITS, r.east / VERT_UNITS);
        let (jlo, jhi) = (r.south / VERT_UNITS, r.north / VERT_UNITS);
        let mut worst = 0;
        for j in jlo..=jhi {
            for i in ilo..=ihi {
                if i < ihi {
                    worst = worst.max((field.height(i + 1, j) - field.height(i, j)).abs() as i64);
                }
                if j < jhi {
                    worst = worst.max((field.height(i, j + 1) - field.height(i, j)).abs() as i64);
                }
            }
        }
        worst
    }

    #[test]
    fn a_mountain_side_is_steeper_than_46_degrees_and_a_hill_side_is_not() {
        let codes = vec![5u8; 400];
        let objects = json!([
            { "x": 10, "y": 10, "sprite": 10, "class": 5 },
            { "x": 4, "y": 4, "sprite": 10, "class": 1 }
        ]);
        let (map, terrain) = world(20, 20, &codes, objects, json!([]));
        let field = build(&terrain, &map);
        // tan(46°) × 128 units per vertex step, about 133; tan(30°) × 128 is about 74.
        assert!(steepest_step(&field, 10, 10) >= 133, "mountain: {}", steepest_step(&field, 10, 10));
        assert!(steepest_step(&field, 4, 4) <= 82, "hill: {}", steepest_step(&field, 4, 4));
    }

    #[test]
    fn a_pgm_has_its_header_and_one_byte_per_vertex() {
        let codes = vec![5u8; 16];
        let (map, terrain) = world(4, 4, &codes, json!([]), json!([]));
        let field = build(&terrain, &map);
        let pgm = field.pgm();
        let header = b"P5\n193 193\n255\n";
        assert!(pgm.starts_with(header));
        assert_eq!(pgm.len(), header.len() + 193 * 193);
    }

    #[test]
    fn the_noise_is_deterministic_and_small() {
        for i in -8..8 {
            for j in -8..8 {
                let a = noise(42, i, j);
                assert_eq!(a, noise(42, i, j));
                assert!(a.abs() <= 12);
            }
        }
    }
}
