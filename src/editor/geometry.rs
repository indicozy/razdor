//! Cells an edit touches: brushes, rectangles, flood fill, building footprints and the
//! squares map objects cover (`docs/reference/original-mechanics/world.md` §1, §7).

use crate::dt::dtm::MapObject;

/// The original editor's brush sizes: buttons 1 to 6 (docs/reference/editor/main-window.md
/// §4.2).
pub const BRUSH_SIZES: [u32; 6] = [1, 2, 3, 4, 5, 6];

/// The brush centre of the original (§6.1): the hovered cell plus `size div 2` on both axes
/// (the delete brush, −1, keeps the hovered cell, as `−1 div 2` is 0).
pub fn brush_centre(hovered: (i32, i32), size: i32) -> (i32, i32) {
    (hovered.0 + size / 2, hovered.1 + size / 2)
}

/// An inclusive cell rectangle; may reach outside the map (see [`CellRect::clip`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl CellRect {
    /// The rectangle spanned by two corners, in any order.
    pub fn spanning(a: (i32, i32), b: (i32, i32)) -> CellRect {
        CellRect { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
    }

    /// The original's brush square: `size`×`size` cells ending at the brush centre
    /// `(x, y)`, its bottom-right cell ([`brush_centre`]: odd sizes are centred on the
    /// hovered cell, even ones reach one cell further right and down).
    pub fn brush(x: i32, y: i32, size: u32) -> CellRect {
        let s = size.max(1) as i32;
        CellRect { x0: x - s + 1, y0: y - s + 1, x1: x, y1: y }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }

    /// Entirely inside a `w`×`h` map.
    pub fn inside(&self, w: u32, h: u32) -> bool {
        self.x0 >= 0 && self.y0 >= 0 && self.x1 < w as i32 && self.y1 < h as i32 && self.x0 <= self.x1 && self.y0 <= self.y1
    }

    pub fn overlaps(&self, o: &CellRect) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    /// The part inside a `w`×`h` map, if any.
    pub fn clip(&self, w: u32, h: u32) -> Option<CellRect> {
        let r = CellRect { x0: self.x0.max(0), y0: self.y0.max(0), x1: self.x1.min(w as i32 - 1), y1: self.y1.min(h as i32 - 1) };
        (r.x0 <= r.x1 && r.y0 <= r.y1).then_some(r)
    }

    /// Cells row by row.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.y0..=self.y1).flat_map(move |y| (self.x0..=self.x1).map(move |x| (x, y)))
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0 + 1
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
}

/// Cells of a brush clipped to the map, as grid indices `y*w + x`.
pub fn brush_indices(w: u32, h: u32, x: i32, y: i32, size: u32) -> Vec<usize> {
    rect_indices(w, h, CellRect::brush(x, y, size))
}

/// Cells of a rectangle clipped to the map, as grid indices.
pub fn rect_indices(w: u32, h: u32, r: CellRect) -> Vec<usize> {
    match r.clip(w, h) {
        Some(r) => r.cells().map(|(x, y)| (y as u32 * w + x as u32) as usize).collect(),
        None => Vec::new(),
    }
}

/// The 4-connected region of cells with the same code as `(x, y)`, as grid indices.
pub fn flood_region(terrain: &[u8], w: u32, h: u32, x: u32, y: u32) -> Vec<usize> {
    if x >= w || y >= h || terrain.len() != (w * h) as usize {
        return Vec::new();
    }
    let start = (y * w + x) as usize;
    let code = terrain[start];
    let mut seen = vec![false; terrain.len()];
    let mut stack = vec![start];
    let mut out = Vec::new();
    seen[start] = true;
    while let Some(i) = stack.pop() {
        out.push(i);
        let (cx, cy) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let n = (ny as u32 * w + nx as u32) as usize;
            if !seen[n] && terrain[n] == code {
                seen[n] = true;
                stack.push(n);
            }
        }
    }
    out.sort_unstable();
    out
}

/// A building's footprint: the stored cell is the **bottom-right** one; the footprint spans
/// `x−sx+1..=x × y−sy+1..=y`, plus one extra row above when `sx > sy` (world.md §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footprint {
    /// The `sx`×`sy` rectangle.
    pub main: CellRect,
    /// The extra row above, when the footprint is wider than tall.
    pub extra_row: Option<CellRect>,
}

impl Footprint {
    pub fn of(x: i32, y: i32, sx: u8, sy: u8) -> Footprint {
        let (sx, sy) = (sx.max(1) as i32, sy.max(1) as i32);
        let main = CellRect { x0: x - sx + 1, y0: y - sy + 1, x1: x, y1: y };
        let extra_row = (sx > sy).then(|| CellRect { x0: main.x0, y0: main.y0 - 1, x1: main.x1, y1: main.y0 - 1 });
        Footprint { main, extra_row }
    }

    /// The bounding rectangle, extra row included.
    pub fn bounds(&self) -> CellRect {
        match self.extra_row {
            Some(e) => CellRect { y0: e.y0, ..self.main },
            None => self.main,
        }
    }

    /// Every cell of the footprint, extra row included.
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.extra_row.iter().flat_map(|r| r.cells()).chain(self.main.cells())
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    /// All cells inside a `w`×`h` map.
    pub fn inside(&self, w: u32, h: u32) -> bool {
        self.bounds().inside(w, h)
    }
}

/// Classes 1–8 are massifs (hills, mountains, rocks): they cover a square; classes 9–12 are
/// plants and cover only their own cell (world.md §1).
pub fn is_massif(class: u8) -> bool {
    (1..=8).contains(&class)
}

/// The square an object covers: side `sprite div 10` (at least 1) for massifs, with the
/// object's cell at the bottom-right; just the cell for plants.
pub fn object_cover(o: &MapObject) -> CellRect {
    let side = if is_massif(o.class) { (o.sprite / 10).max(1) as i32 } else { 1 };
    let (x, y) = (o.x as i32, o.y as i32);
    CellRect { x0: x - side + 1, y0: y - side + 1, x1: x, y1: y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_squares_end_at_the_centre() {
        // Odd sizes are centred on the hovered cell, even ones lean down-right.
        for (size, hovered, square) in [
            (1, (5, 5), CellRect { x0: 5, y0: 5, x1: 5, y1: 5 }),
            (2, (5, 5), CellRect { x0: 5, y0: 5, x1: 6, y1: 6 }),
            (3, (5, 5), CellRect { x0: 4, y0: 4, x1: 6, y1: 6 }),
            (4, (5, 5), CellRect { x0: 4, y0: 4, x1: 7, y1: 7 }),
            (5, (5, 5), CellRect { x0: 3, y0: 3, x1: 7, y1: 7 }),
            (6, (5, 5), CellRect { x0: 3, y0: 3, x1: 8, y1: 8 }),
        ] {
            let (cx, cy) = brush_centre(hovered, size as i32);
            assert_eq!(CellRect::brush(cx, cy, size), square, "size {size}");
        }
        // The delete brush (−1) keeps the hovered cell.
        assert_eq!(brush_centre((5, 5), -1), (5, 5));
        assert_eq!(brush_indices(4, 4, 1, 1, 3), vec![0, 1, 4, 5]);
        assert_eq!(brush_indices(4, 4, 3, 3, 5).len(), 16);
        assert!(brush_indices(4, 4, 10, 10, 3).is_empty());
        for size in BRUSH_SIZES {
            assert_eq!(brush_indices(20, 20, 10, 10, size).len(), (size * size) as usize);
        }
    }

    #[test]
    fn rectangles_span_any_corners() {
        let r = CellRect::spanning((3, 1), (1, 2));
        assert_eq!(r, CellRect { x0: 1, y0: 1, x1: 3, y1: 2 });
        assert_eq!(rect_indices(4, 3, r), vec![5, 6, 7, 9, 10, 11]);
        assert_eq!(rect_indices(4, 3, CellRect::spanning((-5, -5), (0, 0))), vec![0]);
    }

    #[test]
    fn flood_fill_is_4_connected() {
        // 1 1 0
        // 0 1 0
        // 1 0 1   the diagonal 1s are separate regions
        let t = [1, 1, 0, 0, 1, 0, 1, 0, 1];
        assert_eq!(flood_region(&t, 3, 3, 0, 0), vec![0, 1, 4]);
        assert_eq!(flood_region(&t, 3, 3, 2, 0), vec![2, 5]);
        assert_eq!(flood_region(&t, 3, 3, 0, 2), vec![6]);
        assert!(flood_region(&t, 3, 3, 3, 0).is_empty());
    }

    #[test]
    fn footprints_anchor_bottom_right() {
        let f = Footprint::of(5, 6, 2, 3);
        assert_eq!(f.main, CellRect { x0: 4, y0: 4, x1: 5, y1: 6 });
        assert_eq!(f.extra_row, None);
        assert_eq!(f.cells().count(), 6);
        // Wider than tall: one more row on top.
        let f = Footprint::of(5, 6, 4, 3);
        assert_eq!(f.main, CellRect { x0: 2, y0: 4, x1: 5, y1: 6 });
        assert_eq!(f.extra_row, Some(CellRect { x0: 2, y0: 3, x1: 5, y1: 3 }));
        assert_eq!(f.bounds(), CellRect { x0: 2, y0: 3, x1: 5, y1: 6 });
        assert_eq!(f.cells().count(), 16);
        assert!(f.contains(2, 3) && !f.contains(1, 3));
        assert!(f.inside(6, 7) && !f.inside(5, 7));
        assert!(!Footprint::of(2, 2, 4, 3).inside(10, 10));
        assert!(Footprint::of(3, 3, 4, 3).inside(10, 10));
    }

    #[test]
    fn objects_cover_squares() {
        let hill = MapObject { x: 10, y: 10, sprite: 32, class: 1 };
        assert_eq!(object_cover(&hill), CellRect { x0: 8, y0: 8, x1: 10, y1: 10 });
        let tree = MapObject { x: 10, y: 10, sprite: 32, class: 9 };
        assert_eq!(object_cover(&tree), CellRect { x0: 10, y0: 10, x1: 10, y1: 10 });
        let small = MapObject { x: 1, y: 1, sprite: 3, class: 5 };
        assert_eq!(object_cover(&small).width(), 1);
    }
}
