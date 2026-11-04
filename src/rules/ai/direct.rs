//! The direct planner («Прямой путь к единственной цели», the `direct_path` switch): a plan whose
//! only target is one cell is an A* path to it, instead of the flood and the descent.
//!
//! The objective is the flood's: the walk from the army is charged, cell by cell, the cell it
//! leaves (its effective cost, cost × multiplier, times the step weight; the target is free).
//! The search runs from the target back to the army, charging the cell it enters, which is the
//! same sum (the weights are symmetric). Erased cells are closed, except that the army's own
//! cell is entered at cost 1 when it is closed, as the flood seeds a closed cell. The heuristic
//! is consistent: the least positive static cost times the octile weight bound to the army, less
//! the most a last step can save (a diagonal of weight 3 against a cost of 1).
//!
//! Razdor's own choice (armies move a little differently from the original). A path is found
//! among the cells the flood would reach; no path gives no plan, and the caller falls back to the
//! normal planner.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::local;
use crate::rules::map::{Tile, TileMap, DIAGONAL_WEIGHT};

/// The effective cost of a cell: cost × multiplier, wrapping as the flood's does.
fn effective(cost: &[u16], mult: &[u16], j: usize) -> u32 {
    cost[j].wrapping_mul(mult[j]) as u32
}

/// The search's reusable arrays: one per thread (the planner's workers each have their own).
#[derive(Default)]
struct Scratch {
    dist: Vec<u32>,
    parent: Vec<u32>,
    stamp: Vec<u32>,
    epoch: u32,
}

local! {
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch::default());
}

impl Scratch {
    fn begin(&mut self, n: usize) {
        if self.dist.len() < n {
            self.dist.resize(n, 0);
            self.parent.resize(n, 0);
            self.stamp.resize(n, 0);
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.epoch = 1;
        }
    }

    fn get(&self, j: usize) -> u32 {
        if self.stamp[j] == self.epoch {
            self.dist[j]
        } else {
            u32::MAX
        }
    }

    fn set(&mut self, j: usize, d: u32, parent: u32) {
        self.stamp[j] = self.epoch;
        self.dist[j] = d;
        self.parent[j] = parent;
    }
}

/// The path from `from` to the single target cell, excluding `from`, the target last. Empty when
/// no path exists (or `from` is the target). `geom` is the map or its geometry; `cost` and `mult`
/// are the planner's maps; `erase` the cells the path never steps onto. When `from` is erased the
/// caller stands still (see `flood_pool`), so it is not handled here.
pub fn path(geom: &TileMap, cost: &[u16], mult: &[u16], target: Tile, from: Tile, erase: &[Tile]) -> Vec<Tile> {
    let index = |t: Tile| geom.in_bounds(t).then(|| (t.1 * geom.w + t.0) as usize);
    let (Some(start), Some(goal)) = (index(from), index(target)) else { return Vec::new() };
    if start == goal {
        return Vec::new();
    }
    let grid = geom.grid;
    let tile = |i: usize| ((i as i32) % geom.w, (i as i32) / geom.w);
    let min_step = cost.iter().copied().filter(|&c| c > 0).min().unwrap_or(1).max(1) as u32;
    let h = |x: usize| -> u32 { (min_step * grid.weight_bound(tile(x), from)).saturating_sub((min_step - 1) * DIAGONAL_WEIGHT) };
    let n = cost.len();
    SCRATCH.with(|s| {
        let s = &mut *s.borrow_mut();
        s.begin(n);
        s.set(goal, 0, u32::MAX);
        let mut open = BinaryHeap::from([Reverse((h(goal), 0u32, goal as u32))]);
        while let Some(Reverse((_, g, i))) = open.pop() {
            let i = i as usize;
            if g > s.get(i) {
                continue;
            }
            if i == start {
                // Walk the parents back from the army to the target: the forward walk.
                let mut out = Vec::new();
                let mut cur = s.parent[start] as usize;
                while cur != goal {
                    out.push(tile(cur));
                    cur = s.parent[cur] as usize;
                }
                out.push(target);
                return out;
            }
            let here = tile(i);
            for nb in grid.neighbours(here) {
                let Some(j) = index(nb) else { continue };
                if erase.contains(&nb) {
                    continue;
                }
                let e = match effective(cost, mult, j) {
                    0 if j == start => 1,
                    0 => continue,
                    e => e,
                };
                let ng = g.saturating_add(e * grid.weight(here, nb));
                if ng < s.get(j) {
                    s.set(j, ng, i as u32);
                    open.push(Reverse((ng.saturating_add(h(j)), ng, j as u32)));
                }
            }
        }
        Vec::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::map::Grid;
    use std::collections::HashMap;

    /// A random map of `w × h` cells (grass, marsh, swamp and some closed cells) with random
    /// multipliers (0 closes a cell), and a fixed seed: the planner's inputs.
    fn random_map(seed: u32, w: i32, h: i32, grid: Grid) -> (TileMap, Vec<u16>, Vec<u16>) {
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n
        };
        let codes: Vec<u8> = (0..w * h)
            .map(|_| match next(10) {
                0 => Surface::ImpassableSwamp as u8,
                1 | 2 => Surface::Marsh as u8,
                _ => Surface::GrassPlain as u8,
            })
            .collect();
        let map = TileMap::from_codes(grid, w, h, &codes, Vec::new());
        let cost = map.land_costs().to_vec();
        let mult: Vec<u16> = (0..cost.len()).map(|_| if next(15) == 0 { 0 } else { 1 + next(4) as u16 }).collect();
        (map, cost, mult)
    }

    /// The objective of a walk from `from` along `path` (the flood's charge: each cell left costs
    /// its effective cost times the step weight, a closed start costs 1, the target is free).
    fn walk_cost(map: &TileMap, cost: &[u16], mult: &[u16], from: Tile, path: &[Tile]) -> u64 {
        let index = |t: Tile| (t.1 * map.w + t.0) as usize;
        let mut total = 0u64;
        let mut prev = from;
        for &t in path {
            let e = match effective(cost, mult, index(prev)) {
                0 => 1,
                e => e,
            };
            total += e as u64 * map.grid.weight(prev, t) as u64;
            prev = t;
        }
        total
    }

    /// The optimum of the same objective: Dijkstra from the target, entering each cell at its
    /// effective cost (a closed army cell at 1), no heuristic.
    fn optimum(map: &TileMap, cost: &[u16], mult: &[u16], target: Tile, from: Tile, erase: &[Tile]) -> Option<u64> {
        let mut best: HashMap<Tile, u64> = HashMap::new();
        let mut heap = BinaryHeap::new();
        best.insert(target, 0);
        heap.push(Reverse((0u64, target)));
        while let Some(Reverse((g, here))) = heap.pop() {
            if here == from {
                return Some(g);
            }
            if best.get(&here).is_some_and(|&b| g > b) {
                continue;
            }
            for nb in map.grid.neighbours(here) {
                if !map.in_bounds(nb) || erase.contains(&nb) {
                    continue;
                }
                let j = (nb.1 * map.w + nb.0) as usize;
                let e = match effective(cost, mult, j) {
                    0 if nb == from => 1,
                    0 => continue,
                    e => e,
                };
                let ng = g + e as u64 * map.grid.weight(here, nb) as u64;
                if best.get(&nb).is_none_or(|&b| ng < b) {
                    best.insert(nb, ng);
                    heap.push(Reverse((ng, nb)));
                }
            }
        }
        None
    }

    /// The path is a walk of neighbours from `from` to the target that steps onto no erased cell,
    /// and its objective is the optimum (A* is exact with its consistent heuristic).
    #[test]
    fn a_star_path_costs_the_optimum() {
        let mut checked = 0;
        for seed in 1..=60u32 {
            for grid in [Grid::Square8, Grid::HexOddR] {
                let (w, h) = (24, 20);
                let (map, cost, mult) = random_map(seed, w, h, grid);
                let mut state = seed.wrapping_mul(7919);
                let mut pick = || {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    ((state >> 8) % w as u32) as i32
                };
                let from = (pick(), (pick() * 7) % h);
                let target = (pick(), (pick() * 3) % h);
                let erase: Vec<Tile> = (0..5).map(|_| (pick(), (pick() * 5) % h)).filter(|&t| t != target && t != from).collect();
                // The caller never aims at a closed cell or at the army's own cell.
                let target_i = (target.1 * w + target.0) as usize;
                if from == target || cost[target_i] == 0 {
                    continue;
                }
                let want = optimum(&map, &cost, &mult, target, from, &erase);
                let got = path(&map, &cost, &mult, target, from, &erase);
                match want {
                    None => assert!(got.is_empty(), "seed {seed}: a path where none exists"),
                    Some(opt) => {
                        assert!(!got.is_empty(), "seed {seed}: no path where one exists");
                        assert_eq!(got.last(), Some(&target));
                        let mut prev = from;
                        for &t in &got {
                            assert!(map.grid.neighbours(prev).any(|n| n == t), "seed {seed}: {t:?} is not a neighbour of {prev:?}");
                            let j = (t.1 * w + t.0) as usize;
                            // The target is the one closed cell the walk may end on (it is never entered by the flood's charge).
                            assert!(!erase.contains(&t) && (t == target || effective(&cost, &mult, j) != 0), "seed {seed}: the path steps on a closed cell");
                            prev = t;
                        }
                        assert_eq!(walk_cost(&map, &cost, &mult, from, &got), opt, "seed {seed} on {grid:?}: not the optimum");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 60, "the fixtures must mostly have paths (checked {checked})");
    }
}
