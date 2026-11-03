//! The studio's canvas geometry: the inclusive cell rectangle, and a building's footprint as the
//! canvas outlines it (`docs/reference/original-mechanics/world.md` §7). The footprint of a
//! package's building is `rules::world::footprint_cells`; this is the outline of a `.DTm` record
//! the map canvas draws.

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

    /// A `size`×`size` square centred on `(x, y)` (even sizes lean up-left).
    pub fn brush(x: i32, y: i32, size: u32) -> CellRect {
        let s = size.max(1) as i32;
        let (x0, y0) = (x - (s - 1) / 2, y - (s - 1) / 2);
        CellRect { x0, y0, x1: x0 + s - 1, y1: y0 + s - 1 }
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
