//! The rivers' flow: a best guess for Razdor's «Живая вода» (style only, drawn as ripples that drift).
//! Each water cell gets a unit vector toward where its water goes. Water runs to the large water
//! (a lake's or the sea's interior, or the map's border), and the interior itself stands still. A
//! landlocked piece of water (no large water in it) runs away from the high ground: from its cell
//! nearest a hill or mountain, or from its first cell when the map has none. A few rivers come out
//! wrong; nothing reads this but the drawing, so no rule, path or save depends on it.
//!
//! Pure: no macroquad. `ui::terrain` turns the field into the `Flow` texture.

use super::map::{is_water, object_class, Tile, TileMap};
use std::collections::VecDeque;

/// The eight neighbours of a cell (x right, y down).
const NEIGHBOURS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// Distance no BFS reaches.
const FAR: u32 = u32::MAX;

/// The flow of every cell of `map`, row by row: a unit vector (cell units, x right and y down), or
/// zero on land, in the water's large interior, and where the water stands still. Length `w · h`.
pub fn flow_field(map: &TileMap) -> Vec<(f32, f32)> {
    let (w, h) = (map.w.max(0), map.h.max(0));
    let n = (w * h) as usize;
    let at = |i: usize| (i as i32 % w, i as i32 / w);
    let index = |x: i32, y: i32| (y * w + x) as usize;
    let water: Vec<bool> = (0..n).map(|i| is_water(map.surface(at(i)))).collect();
    // Large water: water with water all round it (none of the eight is land), or on the border.
    let large: Vec<bool> = (0..n)
        .map(|i| {
            let (x, y) = at(i);
            water[i] && (x == 0 || y == 0 || x == w - 1 || y == h - 1 || NEIGHBOURS.iter().all(|&(dx, dy)| water[index(x + dx, y + dy)]))
        })
        .collect();
    let sources: Vec<usize> = (0..n).filter(|&i| large[i]).collect();
    let dist = distances(w, h, &sources, &water);
    let mut flow = vec![(0.0f32, 0.0f32); n];
    // A river: toward the neighbouring water with the smallest distance to the large water.
    for i in (0..n).filter(|&i| water[i] && !large[i] && dist[i] != FAR) {
        let (x, y) = at(i);
        let best = neighbours_of(x, y, &water, w, h).map(|j| dist[j]).min().unwrap_or(FAR);
        let toward = neighbours_of(x, y, &water, w, h).filter(|&j| dist[j] == best).map(|j| (at(j).0 - x, at(j).1 - y));
        flow[i] = steer(toward);
    }
    // Landlocked water: away from the high ground (or from its first cell) along the water.
    let hills: Vec<usize> = map.objects.iter().filter(|o| (object_class::HILLS..=object_class::DARK_MOUNTAINS).contains(&o.class)).filter_map(|o| inside(o.tile, w, h)).map(|t| index(t.0, t.1)).collect();
    let all = vec![true; n];
    let hill_dist = if hills.is_empty() { Vec::new() } else { distances(w, h, &hills, &all) };
    // One buffer for every piece: a piece's BFS only reaches its own cells, which are reset after it.
    let mut away = vec![FAR; n];
    for piece in landlocked_pieces(w, h, &water, &large) {
        let source = piece
            .iter()
            .copied()
            .filter(|&i| hill_dist.get(i).is_some_and(|&d| d != FAR))
            .min_by_key(|&i| (hill_dist[i], i))
            .unwrap_or_else(|| *piece.iter().min().unwrap_or(&0));
        spread(&mut away, w, h, &[source], &water);
        for &i in &piece {
            let (x, y) = at(i);
            let here = away[i];
            let far = neighbours_of(x, y, &water, w, h).map(|j| away[j]).max().unwrap_or(FAR);
            if here == FAR || far == FAR || far <= here {
                continue;
            }
            let toward = neighbours_of(x, y, &water, w, h).filter(|&j| away[j] == far).map(|j| (at(j).0 - x, at(j).1 - y));
            flow[i] = steer(toward);
        }
        for &i in &piece {
            away[i] = FAR;
        }
    }
    flow
}

/// The water neighbours (in the map) of cell (x, y), as indices.
fn neighbours_of<'a>(x: i32, y: i32, water: &'a [bool], w: i32, h: i32) -> impl Iterator<Item = usize> + 'a {
    NEIGHBOURS.into_iter().filter_map(move |(dx, dy)| {
        let (nx, ny) = (x + dx, y + dy);
        (nx >= 0 && ny >= 0 && nx < w && ny < h && water[(ny * w + nx) as usize]).then_some((ny * w + nx) as usize)
    })
}

/// The unit vector of the summed unit steps `dirs` (a step is `(dx, dy)`), zero when there are none
/// or they cancel.
fn steer(dirs: impl Iterator<Item = (i32, i32)>) -> (f32, f32) {
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for (dx, dy) in dirs {
        let len = ((dx * dx + dy * dy) as f32).sqrt();
        sx += dx as f32 / len;
        sy += dy as f32 / len;
    }
    let len = (sx * sx + sy * sy).sqrt();
    if len < 1e-6 {
        (0.0, 0.0)
    } else {
        (sx / len, sy / len)
    }
}

/// The tile as an in-map cell, `None` outside.
fn inside(t: Tile, w: i32, h: i32) -> Option<Tile> {
    (t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h).then_some(t)
}

/// Multi-source BFS over the eight neighbours, through the `open` cells: each reached cell's steps
/// from the nearest source (`FAR` where none reaches it).
fn distances(w: i32, h: i32, sources: &[usize], open: &[bool]) -> Vec<u32> {
    let mut dist = vec![FAR; open.len()];
    spread(&mut dist, w, h, sources, open);
    dist
}

/// The BFS of [`distances`] into `dist`, which must hold `FAR` where the search may reach.
fn spread(dist: &mut [u32], w: i32, h: i32, sources: &[usize], open: &[bool]) {
    let mut queue = VecDeque::new();
    for &s in sources {
        if dist[s] == FAR {
            dist[s] = 0;
            queue.push_back(s);
        }
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i as i32 % w, i as i32 / w);
        for j in NEIGHBOURS.into_iter().filter_map(|(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w && ny < h).then_some((ny * w + nx) as usize)
        }) {
            if open[j] && dist[j] == FAR {
                dist[j] = dist[i] + 1;
                queue.push_back(j);
            }
        }
    }
}

/// The connected pieces of water (eight neighbours) with no large water in them.
fn landlocked_pieces(w: i32, h: i32, water: &[bool], large: &[bool]) -> Vec<Vec<usize>> {
    let n = water.len();
    let mut seen = vec![false; n];
    let mut pieces = Vec::new();
    for s in 0..n {
        if !water[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        let mut piece = vec![s];
        let mut k = 0;
        while k < piece.len() {
            let i = piece[k];
            k += 1;
            let (x, y) = (i as i32 % w, i as i32 / w);
            for j in neighbours_of(x, y, water, w, h) {
                if !seen[j] {
                    seen[j] = true;
                    piece.push(j);
                }
            }
        }
        if !piece.iter().any(|&i| large[i]) {
            pieces.push(piece);
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::map::{Decoration, Grid};

    /// Land (a grass code, not water).
    const LAND: u8 = 5;
    /// Shallows and fords: water (codes 0-2 are water).
    const RIVER: u8 = 0;
    /// Deep sea: water.
    const DEEP: u8 = 2;

    /// A `w`×`h` map of land with the cells of each `(x0, x1, y, code)` run painted, and a hill
    /// object on each of `hills`.
    fn map(w: i32, h: i32, runs: &[(i32, i32, i32, u8)], hills: &[Tile]) -> TileMap {
        let mut codes = vec![LAND; (w * h) as usize];
        for &(x0, x1, y, code) in runs {
            for x in x0..=x1 {
                codes[(y * w + x) as usize] = code;
            }
        }
        let objects = hills.iter().map(|&t| Decoration::new(t, object_class::HILLS, 0)).collect();
        TileMap::from_codes(Grid::Square8, w, h, &codes, objects)
    }

    fn at(field: &[(f32, f32)], w: i32, x: i32, y: i32) -> (f32, f32) {
        field[(y * w + x) as usize]
    }

    #[test]
    fn a_straight_river_runs_into_the_lake() {
        // River x 1..=4 on row 3; a lake x 5..=10 on rows 1..=5 (its interior x 6..=9, y 2..=4).
        let mut runs = vec![(1, 4, 3, RIVER)];
        runs.extend((1..=5).map(|y| (5, 10, y, DEEP)));
        let f = flow_field(&map(13, 7, &runs, &[]));
        assert_eq!(f.len(), 13 * 7);
        for x in 1..=2 {
            let (fx, fy) = at(&f, 13, x, 3);
            assert!(fx > 0.9 && fy.abs() < 1e-5, "river cell ({x}, 3) flows east, got ({fx}, {fy})");
        }
    }

    #[test]
    fn a_lake_interior_stands_still_and_land_has_no_flow() {
        let runs: Vec<(i32, i32, i32, u8)> = (1..=5).map(|y| (5, 10, y, DEEP)).collect();
        let f = flow_field(&map(13, 7, &runs, &[]));
        assert_eq!(at(&f, 13, 7, 3), (0.0, 0.0), "the lake's middle");
        assert_eq!(at(&f, 13, 8, 2), (0.0, 0.0), "the lake's interior");
        assert_eq!(at(&f, 13, 0, 0), (0.0, 0.0), "land");
    }

    #[test]
    fn a_river_between_two_lakes_flows_both_ways_from_its_middle() {
        // Lake A x 1..=5, river x 6..=10 on row 3, lake B x 11..=15; both lakes on rows 1..=5.
        let mut runs = vec![(6, 10, 3, RIVER)];
        for y in 1..=5 {
            runs.push((1, 5, y, DEEP));
            runs.push((11, 15, y, DEEP));
        }
        let f = flow_field(&map(17, 7, &runs, &[]));
        let (west, _) = at(&f, 17, 7, 3);
        let (east, _) = at(&f, 17, 9, 3);
        assert!(west < -0.9, "the river west of the middle flows into lake A: {west}");
        assert!(east > 0.9, "the river east of the middle flows into lake B: {east}");
        let (mid, mid_y) = at(&f, 17, 8, 3);
        assert!(mid.abs() < 1e-5 && mid_y.abs() < 1e-5, "the middle stands still between the two: ({mid}, {mid_y})");
    }

    #[test]
    fn a_landlocked_river_runs_away_from_the_hill_at_its_end() {
        // River x 2..=6 on row 2, no large water, a hill on the land at (1, 2).
        let f = flow_field(&map(9, 5, &[(2, 6, 2, RIVER)], &[(1, 2)]));
        let (east, _) = at(&f, 9, 3, 2);
        assert!(east > 0.9, "away from the hill, eastward: {east}");
        let (east_far, _) = at(&f, 9, 5, 2);
        assert!(east_far > 0.9, "further along it keeps going away from the hill: {east_far}");
    }

    #[test]
    fn a_landlocked_river_with_no_hill_starts_at_its_first_cell() {
        let f = flow_field(&map(9, 5, &[(2, 6, 2, RIVER)], &[]));
        let (east, _) = at(&f, 9, 4, 2);
        assert!(east > 0.9, "from its first cell (2, 2) eastward: {east}");
    }
}
