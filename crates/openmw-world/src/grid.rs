//! Coordinates (spec §3.2): tiles, world units, cells and the global vertex grid.
//!
//! World units follow OpenMW: a cell is 8192 units, a land vertex is 128 units apart, 65 vertices
//! per cell side. The map's south-west corner is the origin; DT's row 0 is the north, so tile row
//! `y` sits at world `Y = (H - y - 0.5) * T`.

use std::fmt;

/// A cell's side in world units.
pub const CELL_UNITS: i64 = 8192;
/// Land vertices per cell side (64 squares plus the shared edge).
pub const CELL_VERTS: i64 = 65;
/// Distance between two land vertices.
pub const VERT_UNITS: i64 = 128;
/// Texels per cell side.
pub const CELL_TEXELS: i64 = 16;
/// A texel's side in world units.
pub const TEXEL_UNITS: i64 = CELL_UNITS / CELL_TEXELS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub width: i64,
    pub height: i64,
    pub tile: i64,
}

/// A world rectangle, in units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub west: i64,
    pub east: i64,
    pub south: i64,
    pub north: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GridError {
    TileSize(i64),
    Empty,
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GridError::TileSize(t) => write!(f, "tile size {t} is not 512, 1024 or 2048"),
            GridError::Empty => write!(f, "the map has no tiles"),
        }
    }
}

impl Grid {
    pub fn new(width: usize, height: usize, tile: i64) -> Result<Grid, GridError> {
        if ![512, 1024, 2048].contains(&tile) {
            return Err(GridError::TileSize(tile));
        }
        if width == 0 || height == 0 {
            return Err(GridError::Empty);
        }
        Ok(Grid { width: width as i64, height: height as i64, tile })
    }

    /// World centre of tile `(x, y)`; `y` counts from the north.
    pub fn tile_centre(&self, x: i64, y: i64) -> (i64, i64) {
        (x * self.tile + self.tile / 2, (self.height - y) * self.tile - self.tile / 2)
    }

    /// The tile `(x, y)` under a world point, or `None` off the map.
    pub fn tile_at(&self, wx: i64, wy: i64) -> Option<(i64, i64)> {
        let x = wx.div_euclid(self.tile);
        let rows_from_south = wy.div_euclid(self.tile);
        if x < 0 || x >= self.width || rows_from_south < 0 || rows_from_south >= self.height {
            return None;
        }
        Some((x, self.height - 1 - rows_from_south))
    }

    /// The cell `(cx, cy)` holding a world point.
    pub fn cell_of(wx: i64, wy: i64) -> (i64, i64) {
        (wx.div_euclid(CELL_UNITS), wy.div_euclid(CELL_UNITS))
    }

    /// How many cells cover the map: `0..cells_x` by `0..cells_y` (no margin ring in M1).
    pub fn cells(&self) -> (i64, i64) {
        (div_ceil(self.width * self.tile, CELL_UNITS), div_ceil(self.height * self.tile, CELL_UNITS))
    }

    /// World rectangle of a footprint anchored at its bottom-right tile `(x, y)`, `sx` by `sy`
    /// tiles (spec §3.2). When `sx > sy` the footprint has one extra row above it.
    pub fn footprint(&self, x: i64, y: i64, sx: i64, sy: i64) -> Rect {
        let t = self.tile;
        let extra = if sx > sy { t } else { 0 };
        Rect {
            west: (x - sx + 1) * t,
            east: (x + 1) * t,
            south: (self.height - 1 - y) * t,
            north: (self.height - y + sy - 1) * t + extra,
        }
    }

    /// The tiles `(x, y)` of a footprint, its extra row included, row by row from the south.
    pub fn footprint_tiles(x: i64, y: i64, sx: i64, sy: i64) -> Vec<(i64, i64)> {
        let mut out = Vec::new();
        let extra = if sx > sy { 1 } else { 0 };
        for row in (y - sy - extra + 1)..=y {
            for col in (x - sx + 1)..=x {
                out.push((col, row));
            }
        }
        out
    }
}

/// World position of global land vertex `(i, j)`, `i` east, `j` north.
pub fn vertex_units(i: i64, j: i64) -> (i64, i64) {
    (i * VERT_UNITS, j * VERT_UNITS)
}

/// Global vertex of local vertex `(col, row)` of cell `(cx, cy)`; cells share their edges.
pub fn cell_vertex(cx: i64, cy: i64, col: i64, row: i64) -> (i64, i64) {
    (cx * (CELL_VERTS - 1) + col, cy * (CELL_VERTS - 1) + row)
}

fn div_ceil(a: i64, b: i64) -> i64 {
    (a + b - 1) / b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map50(tile: i64) -> Grid {
        Grid::new(50, 50, tile).unwrap()
    }

    #[test]
    fn tile_zero_zero_of_a_50_map_is_at_512_50688() {
        assert_eq!(map50(1024).tile_centre(0, 0), (512, 50688));
    }

    #[test]
    fn tile_centres_and_tile_at_agree() {
        let g = map50(1024);
        for &(x, y) in &[(0, 0), (49, 49), (7, 30)] {
            let (wx, wy) = g.tile_centre(x, y);
            assert_eq!(g.tile_at(wx, wy), Some((x, y)));
        }
        assert_eq!(g.tile_at(-1, 0), None);
        assert_eq!(g.tile_at(50 * 1024, 0), None);
    }

    #[test]
    fn cells_cover_the_map() {
        assert_eq!(map50(1024).cells(), (7, 7)); // 51200 / 8192 = 6.25
        assert_eq!(Grid::new(16, 16, 1024).unwrap().cells(), (2, 2));
        assert_eq!(Grid::new(16, 16, 2048).unwrap().cells(), (4, 4));
        assert_eq!(Grid::cell_of(8191, 8192), (0, 1));
    }

    #[test]
    fn a_4_by_4_footprint() {
        let g = map50(1024);
        // Anchor (10, 10): columns 7..=10, rows 7..=10 (square, no extra row).
        let r = g.footprint(10, 10, 4, 4);
        assert_eq!(r, Rect { west: 7 * 1024, east: 11 * 1024, south: 39 * 1024, north: 43 * 1024 });
        assert_eq!(Grid::footprint_tiles(10, 10, 4, 4).len(), 16);
    }

    #[test]
    fn a_2_by_1_footprint_has_the_extra_north_row() {
        let g = map50(1024);
        let r = g.footprint(10, 10, 2, 1);
        assert_eq!(r, Rect { west: 9 * 1024, east: 11 * 1024, south: 39 * 1024, north: 41 * 1024 });
        let tiles = Grid::footprint_tiles(10, 10, 2, 1);
        assert_eq!(tiles, vec![(9, 9), (10, 9), (9, 10), (10, 10)]);
    }

    #[test]
    fn a_1_by_1_footprint_is_one_tile() {
        let g = map50(1024);
        let r = g.footprint(10, 10, 1, 1);
        assert_eq!(r, Rect { west: 10 * 1024, east: 11 * 1024, south: 39 * 1024, north: 40 * 1024 });
        assert_eq!(Grid::footprint_tiles(10, 10, 1, 1), vec![(10, 10)]);
    }

    #[test]
    fn a_massif_of_side_2_is_its_square() {
        // Anchor is the bottom-right tile: columns 4..=5, rows 4..=5.
        assert_eq!(Grid::footprint_tiles(5, 5, 2, 2), vec![(4, 4), (5, 4), (4, 5), (5, 5)]);
    }

    #[test]
    fn vertices_share_cell_edges() {
        assert_eq!(vertex_units(3, 2), (384, 256));
        assert_eq!(cell_vertex(0, 0, 64, 64), (64, 64));
        assert_eq!(cell_vertex(1, 0, 0, 64), (64, 64));
        assert_eq!(cell_vertex(0, 1, 64, 0), (64, 64));
        assert_eq!(cell_vertex(1, 1, 0, 0), (64, 64));
    }

    #[test]
    fn tile_sizes_are_checked() {
        assert!(Grid::new(10, 10, 768).is_err());
        assert!(Grid::new(0, 10, 1024).is_err());
        assert!(Grid::new(10, 10, 512).is_ok());
    }
}
