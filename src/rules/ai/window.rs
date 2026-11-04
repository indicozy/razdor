//! The window planner («Быстрый ИИ на больших картах», the `fast_ai` switch): a plan's flood runs
//! only inside a square window around the army, so a plan on a big map costs what the window
//! holds, not the map. Everything else is the planner's: the same seeds, obstacles, cones,
//! erasures and descent (`flood_pool::compute_with`).
//!
//! The window has radius [`RADIUS`] cells around the army (clipped to the map). Its outside is
//! closed, and a seed outside is moved to the window's cell nearest to it (the clamp to the box,
//! or the nearest open border cell when that one is closed), valued at its own value plus its
//! octile distance to the seed times [`step_estimate`]. The window is a Razdor choice, not the
//! original's: armies move a little differently from it.

use crate::rules::map::{Tile, TileMap, ORTHOGONAL_WEIGHT};

/// The window's radius in cells (`fast_ai`).
pub const RADIUS: i32 = 64;

/// The smallest map (in cells) the window applies to. Measured: on the 200×200 map «Другой
/// берег» the window (radius 64 covers most of the map) is 10–20% slower than the whole-map
/// flood, so below 300×300 the planner stays exact even with the switch on.
pub const MIN_CELLS: usize = 300 * 300;

/// A window: its box `(x0, x1, y0, y1)`, inclusive, clipped to the map.
pub type Window = (i32, i32, i32, i32);

/// The window of radius `r` around `here` on a `w × h` map.
pub fn bounds(w: i32, h: i32, (x, y): Tile, r: i32) -> Window {
    ((x - r).max(0), (x + r).min(w - 1), (y - r).max(0), (y + r).min(h - 1))
}

fn inside(win: Window, (x, y): Tile) -> bool {
    x >= win.0 && x <= win.1 && y >= win.2 && y <= win.3
}

/// Closes the multiplier map outside the window (in place): whole rows above and below the box,
/// and the cells left and right of it in the rows inside.
pub fn close_outside(mult: &mut [u16], w: i32, h: i32, win: Window) {
    let (x0, x1, y0, y1) = win;
    let (wu, hu) = (w.max(0) as usize, h.max(0) as usize);
    for y in 0..hu {
        let row = &mut mult[y * wu..(y + 1) * wu];
        if (y as i32) < y0 || (y as i32) > y1 {
            row.fill(0);
        } else {
            row[..x0.max(0) as usize].fill(0);
            if (x1 + 1) < w {
                row[(x1 + 1) as usize..].fill(0);
            }
        }
    }
}

/// The planner's step estimate for a window seed's distance: the median passable cost of the map
/// (a stride sample of at most about 4096 cells) times the orthogonal step weight. The median
/// ignores the impassable cells and resists the swamps and forests a mean would be dragged up by.
pub fn step_estimate(costs: &[u16]) -> u32 {
    let stride = (costs.len() / 4096).max(1);
    let mut v: Vec<u16> = costs.iter().step_by(stride).copied().filter(|&c| c > 0).collect();
    if v.is_empty() {
        return ORTHOGONAL_WEIGHT;
    }
    v.sort_unstable();
    ORTHOGONAL_WEIGHT * v[v.len() / 2] as u32
}

/// The window's seeds: a seed inside stays, one outside moves to the window cell nearest to it
/// (the clamp to the box, or the nearest open border cell when the clamp is closed; dropped when
/// the border has no open cell) with its value plus its octile distance times `avg`.
pub fn seeds_in(geom: &TileMap, costs: &[u16], seeds: &[(Tile, u32)], win: Window, avg: u32) -> Vec<(Tile, u32)> {
    let (w, h) = (geom.w, geom.h);
    let on_map = |t: Tile| t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h;
    let open = |t: Tile| on_map(t) && costs[(t.1 * w + t.0) as usize] > 0;
    let mut out = Vec::with_capacity(seeds.len());
    for &(t, v) in seeds {
        if !on_map(t) {
            continue;
        }
        if inside(win, t) {
            out.push((t, v));
            continue;
        }
        let mut b = (t.0.clamp(win.0, win.1), t.1.clamp(win.2, win.3));
        if !open(b) {
            match nearest_open_border(geom, costs, t, win) {
                Some(nb) => b = nb,
                None => continue,
            }
        }
        out.push((b, v + geom.grid.octile(b, t) as u32 * avg));
    }
    out
}

/// The open cell of the window's border nearest to `t` by octile distance (the first one on a tie).
fn nearest_open_border(geom: &TileMap, costs: &[u16], t: Tile, (x0, x1, y0, y1): Window) -> Option<Tile> {
    let (w, h) = (geom.w, geom.h);
    let ring = (x0..=x1).flat_map(|x| [(x, y0), (x, y1)]).chain((y0..=y1).flat_map(|y| [(x0, y), (x1, y)]));
    let mut best: Option<(i32, Tile)> = None;
    for b in ring {
        if b.0 < 0 || b.1 < 0 || b.0 >= w || b.1 >= h || costs[(b.1 * w + b.0) as usize] == 0 {
            continue;
        }
        let d = geom.grid.octile(b, t);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, b));
        }
    }
    best.map(|(_, b)| b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::map::Grid;

    #[test]
    fn bounds_clip_to_the_map() {
        assert_eq!(bounds(200, 100, (50, 50), 10), (40, 60, 40, 60));
        assert_eq!(bounds(200, 100, (2, 95), 10), (0, 12, 85, 99));
    }

    #[test]
    fn close_outside_leaves_only_the_box_open() {
        let (w, h) = (7, 5);
        let mut m = vec![3u16; (w * h) as usize];
        close_outside(&mut m, w, h, (2, 4, 1, 3));
        for y in 0..h {
            for x in 0..w {
                let inside = (2..=4).contains(&x) && (1..=3).contains(&y);
                assert_eq!(m[(y * w + x) as usize] != 0, inside, "cell ({x},{y})");
            }
        }
    }

    #[test]
    fn step_estimate_is_twice_the_median_passable_cost() {
        assert_eq!(step_estimate(&[0, 0, 0]), ORTHOGONAL_WEIGHT);
        assert_eq!(step_estimate(&[0, 1, 9, 2, 0]), ORTHOGONAL_WEIGHT * 2);
    }

    /// An outside seed moves to the window's border cell nearest to it and pays the octile
    /// distance from there; an inside seed stays; a seed off the map is dropped.
    fn grass(w: i32, h: i32) -> TileMap {
        TileMap::from_codes(Grid::Square8, w, h, &vec![Surface::GrassPlain as u8; (w * h) as usize], Vec::new())
    }

    #[test]
    fn outside_seeds_move_to_the_border_with_their_distance() {
        let map = grass(20, 20);
        let costs = map.land_costs().to_vec();
        let win = (5, 10, 5, 10);
        // (2,7) is 3 cells west of the box: it moves to (5,7), 3 octile cells away at 6 each.
        let seeds = [((2, 7), 4), ((7, 7), 9), ((30, 30), 1), ((-1, 3), 2)];
        assert_eq!(seeds_in(&map, &costs, &seeds, win, 6), vec![((5, 7), 4 + 3 * 6), ((7, 7), 9)]);
    }

    /// A clamp on a closed cell moves the seed to the nearest open border cell instead; the first
    /// of two equally near cells on the ring wins.
    #[test]
    fn a_closed_clamp_moves_the_seed_to_the_nearest_open_border_cell() {
        let map = grass(20, 20);
        let mut costs = map.land_costs().to_vec();
        costs[(7 * 20 + 5) as usize] = 0;
        let win = (5, 10, 5, 10);
        assert_eq!(seeds_in(&map, &costs, &[((2, 7), 4)], win, 6), vec![((5, 6), 4 + 3 * 6)]);
        // Every border cell closed: the seed is dropped.
        let closed = vec![0u16; 400];
        assert!(seeds_in(&map, &closed, &[((2, 7), 4)], win, 6).is_empty());
    }
}
