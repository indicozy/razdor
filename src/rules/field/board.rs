//! The board of a field battle: a grid of up to 12 × 10 tiles, cells counted from 0 at the top
//! left (`x` to the right, `y` down). No macroquad, no Lua.

use std::collections::BinaryHeap;

use serde::{Deserialize, Serialize};

/// The largest board a field battle may have.
pub const MAX_W: i32 = 12;
pub const MAX_H: i32 = 10;

/// What a tile is. A gate with no HP left is rubble (a broken gate leaves a gap of rubble).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tile {
    /// Open ground: 1 manevre a tile.
    Open,
    /// Blocks movement and line of fire.
    Wall,
    /// A gate with HP. It blocks movement and line of fire until broken.
    Gate { hp: i32 },
    /// A raised walkway behind a wall: a shooter on it shoots over walls and reaches further
    /// ([`BATTLEMENT_RANGE`]).
    Battlement,
    /// Rubble: a tile costs 2 manevres.
    Rubble,
}

/// Shots from a battlement reach this many tiles further.
pub const BATTLEMENT_RANGE: i32 = 2;

impl Tile {
    /// Manevres a unit spends to step onto the tile; `None` when it cannot be entered.
    pub fn cost(self) -> Option<i32> {
        match self {
            Tile::Open | Tile::Battlement => Some(1),
            Tile::Rubble => Some(2),
            Tile::Wall | Tile::Gate { .. } => None,
        }
    }

    /// Whether the tile stops a shot that passes over it (a wall, a standing gate).
    pub fn blocks_fire(self) -> bool {
        matches!(self, Tile::Wall | Tile::Gate { hp: 1.. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    pub w: i32,
    pub h: i32,
    pub tiles: Vec<Tile>,
}

impl Board {
    /// An open board of `w × h`; `None` when a size is out of range.
    pub fn open(w: i32, h: i32) -> Option<Board> {
        ((1..=MAX_W).contains(&w) && (1..=MAX_H).contains(&h)).then(|| Board { w, h, tiles: vec![Tile::Open; (w * h) as usize] })
    }

    pub fn inside(&self, x: i32, y: i32) -> bool {
        (0..self.w).contains(&x) && (0..self.h).contains(&y)
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Tile> {
        self.inside(x, y).then(|| self.tiles[(y * self.w + x) as usize])
    }

    /// Sets a tile; `false` when the cell is outside the board.
    pub fn set(&mut self, x: i32, y: i32, t: Tile) -> bool {
        if !self.inside(x, y) {
            return false;
        }
        self.tiles[(y * self.w + x) as usize] = t;
        true
    }

    /// Manhattan distance: the number of steps a unit needs on an open board, and the reach of
    /// a melee blow (1).
    pub fn distance(a: (i32, i32), b: (i32, i32)) -> i32 {
        // In i64 and saturated: a script's cells may be as large as an i32.
        let d = (a.0 as i64 - b.0 as i64).abs() + (a.1 as i64 - b.1 as i64).abs();
        d.min(i32::MAX as i64) as i32
    }

    /// Whether a shot from `from` reaches `to`. The cells between must not block fire; a shooter
    /// on a battlement shoots over walls and gates. The line is Bresenham's, from the shooter.
    pub fn line_of_fire(&self, from: (i32, i32), to: (i32, i32)) -> bool {
        // Both ends must be on the board: the line is then at most board-sized (a script's far
        // cell would otherwise make a long walk).
        if !self.inside(from.0, from.1) || !self.inside(to.0, to.1) {
            return false;
        }
        let over_walls = self.get(from.0, from.1) == Some(Tile::Battlement);
        let (mut x, mut y) = from;
        let dx = (to.0 - x).abs();
        let dy = -(to.1 - y).abs();
        let sx = if x < to.0 { 1 } else { -1 };
        let sy = if y < to.1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if (x, y) == to {
                return true;
            }
            if (x, y) != from && !over_walls && self.get(x, y).is_some_and(Tile::blocks_fire) {
                return false;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Cheapest cost (manevres) from `from` to every cell it can reach, avoiding the cells in
    /// `blocked` (units standing there). `from` costs 0.
    pub fn costs(&self, from: (i32, i32), blocked: &[(i32, i32)]) -> Vec<Option<i32>> {
        let mut best: Vec<Option<i32>> = vec![None; (self.w * self.h) as usize];
        let idx = |x: i32, y: i32| (y * self.w + x) as usize;
        if !self.inside(from.0, from.1) {
            return best;
        }
        best[idx(from.0, from.1)] = Some(0);
        let mut heap = BinaryHeap::new();
        heap.push(std::cmp::Reverse((0, from.0, from.1)));
        while let Some(std::cmp::Reverse((d, x, y))) = heap.pop() {
            if best[idx(x, y)].is_some_and(|b| b < d) {
                continue;
            }
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                let Some(t) = self.get(nx, ny) else { continue };
                if blocked.contains(&(nx, ny)) {
                    continue;
                }
                let Some(c) = t.cost() else { continue };
                let nd = d + c;
                if best[idx(nx, ny)].is_none_or(|b| nd < b) {
                    best[idx(nx, ny)] = Some(nd);
                    heap.push(std::cmp::Reverse((nd, nx, ny)));
                }
            }
        }
        best
    }

    /// The cheapest path from `from` to `to` (both included), avoiding `blocked`. `None` when
    /// there is none. Ties go to the cell with the lower coordinates, so the path is the same
    /// on every run.
    pub fn path(&self, from: (i32, i32), to: (i32, i32), blocked: &[(i32, i32)]) -> Option<Vec<(i32, i32)>> {
        // A script can name any cell: an end off the board is no path (never an index).
        if !self.inside(from.0, from.1) || !self.inside(to.0, to.1) {
            return None;
        }
        if from == to {
            return Some(vec![from]);
        }
        let mut best: Vec<Option<i32>> = vec![None; (self.w * self.h) as usize];
        let mut prev: Vec<Option<(i32, i32)>> = vec![None; (self.w * self.h) as usize];
        let idx = |x: i32, y: i32| (y * self.w + x) as usize;
        best[idx(from.0, from.1)] = Some(0);
        let mut heap = BinaryHeap::new();
        heap.push(std::cmp::Reverse((0, from.0, from.1)));
        while let Some(std::cmp::Reverse((d, x, y))) = heap.pop() {
            if (x, y) == to {
                break;
            }
            if best[idx(x, y)].is_some_and(|b| b < d) {
                continue;
            }
            for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                let Some(t) = self.get(nx, ny) else { continue };
                if blocked.contains(&(nx, ny)) && (nx, ny) != to {
                    continue;
                }
                let Some(c) = t.cost() else { continue };
                let nd = d + c;
                if best[idx(nx, ny)].is_none_or(|b| nd < b) {
                    best[idx(nx, ny)] = Some(nd);
                    prev[idx(nx, ny)] = Some((x, y));
                    heap.push(std::cmp::Reverse((nd, nx, ny)));
                }
            }
        }
        best[idx(to.0, to.1)]?;
        let mut out = vec![to];
        let mut at = to;
        while at != from {
            at = prev[idx(at.0, at.1)]?;
            out.push(at);
        }
        out.reverse();
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_stops_a_shot_and_a_battlement_shoots_over_it() {
        let mut b = Board::open(5, 3).unwrap();
        b.set(2, 1, Tile::Wall);
        assert!(!b.line_of_fire((0, 1), (4, 1)));
        b.set(0, 1, Tile::Battlement);
        assert!(b.line_of_fire((0, 1), (4, 1)));
    }

    #[test]
    fn a_gate_blocks_until_it_is_broken() {
        let mut b = Board::open(5, 1).unwrap();
        b.set(2, 0, Tile::Gate { hp: 5 });
        assert!(!b.line_of_fire((0, 0), (4, 0)));
        assert_eq!(b.path((0, 0), (4, 0), &[]), None);
        b.set(2, 0, Tile::Rubble);
        assert!(b.line_of_fire((0, 0), (4, 0)));
        assert_eq!(b.path((0, 0), (4, 0), &[]).map(|p| p.len()), Some(5));
    }

    #[test]
    fn rubble_costs_two_and_units_block_the_way() {
        let mut b = Board::open(4, 1).unwrap();
        b.set(1, 0, Tile::Rubble);
        let c = b.costs((0, 0), &[]);
        assert_eq!(c[3], Some(4), "rubble (2) then two open tiles (1 each)");
        assert_eq!(b.costs((0, 0), &[(1, 0)])[3], None);
    }

    #[test]
    fn the_path_is_the_cheapest_and_the_same_every_time() {
        let b = Board::open(3, 3).unwrap();
        let p = b.path((0, 0), (2, 2), &[]).unwrap();
        assert_eq!(p.len(), 5);
        assert_eq!(p, b.path((0, 0), (2, 2), &[]).unwrap());
    }

    #[test]
    fn a_path_off_the_board_is_no_path() {
        let b = Board::open(12, 8).unwrap();
        assert_eq!(b.path((0, 0), (99, 99), &[]), None);
        assert_eq!(b.path((0, 0), (-1, -1), &[]), None);
        assert_eq!(b.path((-1, -1), (0, 0), &[]), None);
        assert_eq!(b.path((5, 5), (5, 5), &[]), Some(vec![(5, 5)]));
        assert!(!b.line_of_fire((0, 0), (i32::MAX, i32::MIN)));
        assert_eq!(Board::distance((i32::MIN, 0), (i32::MAX, 0)), i32::MAX);
    }

    #[test]
    fn a_board_has_its_limits() {
        assert!(Board::open(13, 8).is_none());
        assert!(Board::open(12, 10).is_some());
        assert!(Board::open(0, 1).is_none());
    }
}
