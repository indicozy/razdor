//! Selections (UX4, spec §5.6): a set of cells of the open map, made by the marquee, the lasso or
//! the magic wand and combined with the current one (replace, add or subtract). Pure: no drawing
//! here. The outline of a selection (the marching ants) is the boundary between its cells and the
//! rest of the map. The cells a brush or a fill may change are kept inside the selection by
//! [`keep_inside`] and [`flood_within`] when «Только в выделении» is on.

use super::brush;
use super::geometry::CellRect;

/// How a new shape combines with the selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// The shape is the selection.
    #[default]
    Replace,
    /// The shape is added to the selection.
    Add,
    /// The shape is taken out of the selection.
    Subtract,
}

/// The mode a selecting gesture uses: Shift adds, Alt subtracts (the Photoshop convention), and the
/// options bar's mode otherwise.
pub fn mode_of(shift: bool, alt: bool, bar: Mode) -> Mode {
    match (shift, alt) {
        (true, false) => Mode::Add,
        (false, true) => Mode::Subtract,
        _ => bar,
    }
}

/// A selection of a map's cells: one flag per cell, row by row, for a map of `w` by `h` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    w: u32,
    h: u32,
    bits: Vec<bool>,
}

/// One side of a selected cell that touches a cell which is not selected, in cell-corner units
/// (`a` to `b`, both corners of the side). The sides of a selection form closed loops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub a: (i32, i32),
    pub b: (i32, i32),
}

impl Mask {
    /// No cell selected.
    pub fn empty(w: u32, h: u32) -> Mask {
        Mask { w, h, bits: vec![false; (w * h) as usize] }
    }

    /// Every cell selected.
    pub fn all(w: u32, h: u32) -> Mask {
        Mask { w, h, bits: vec![true; (w * h) as usize] }
    }

    /// The cells of a rectangle, clipped to the map.
    pub fn of_rect(w: u32, h: u32, r: CellRect) -> Mask {
        let mut m = Mask::empty(w, h);
        if let Some(r) = r.clip(w, h) {
            for (x, y) in r.cells() {
                m.set(i64::from(x), i64::from(y), true);
            }
        }
        m
    }

    /// The given cells (those off the map are left out).
    pub fn of_cells(w: u32, h: u32, cells: &[(i64, i64)]) -> Mask {
        let mut m = Mask::empty(w, h);
        for &(x, y) in cells {
            m.set(x, y, true);
        }
        m
    }

    /// The cells whose centre lies inside the polygon `pts` (cell units, even-odd rule). A polygon
    /// of fewer than three points selects nothing.
    pub fn of_polygon(w: u32, h: u32, pts: &[(f64, f64)]) -> Mask {
        let mut m = Mask::empty(w, h);
        if pts.len() < 3 {
            return m;
        }
        let x0 = pts.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).floor().max(0.0) as i64;
        let x1 = pts.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).ceil().min(f64::from(w)) as i64;
        let y0 = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).floor().max(0.0) as i64;
        let y1 = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).ceil().min(f64::from(h)) as i64;
        for y in y0..y1 {
            for x in x0..x1 {
                if inside(pts, x as f64 + 0.5, y as f64 + 0.5) {
                    m.set(x, y, true);
                }
            }
        }
        m
    }

    /// The magic wand's cells from `start`: the cells connected to it (4-neighbours) with its terrain
    /// code, or every cell of that code on the map when `contiguous` is off. `codes` is the map's
    /// terrain, row by row. Empty when `start` is off the map.
    pub fn of_wand(codes: &[u8], w: u32, h: u32, start: (u32, u32), contiguous: bool) -> Mask {
        let mut m = Mask::empty(w, h);
        if start.0 >= w || start.1 >= h || codes.len() != (w * h) as usize {
            return m;
        }
        if contiguous {
            for (x, y) in brush::flood(codes, w, h, start) {
                m.set(i64::from(x), i64::from(y), true);
            }
        } else {
            let want = codes[(start.1 * w + start.0) as usize];
            for (i, &code) in codes.iter().enumerate() {
                m.bits[i] = code == want;
            }
        }
        m
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// Whether the cell is selected (off the map: no).
    pub fn get(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h) && self.bits[(y as u32 * self.w + x as u32) as usize]
    }

    fn set(&mut self, x: i64, y: i64, on: bool) {
        if x >= 0 && y >= 0 && x < i64::from(self.w) && y < i64::from(self.h) {
            self.bits[(y as u32 * self.w + x as u32) as usize] = on;
        }
    }

    /// Whether no cell is selected.
    pub fn is_empty(&self) -> bool {
        !self.bits.iter().any(|&b| b)
    }

    /// The number of selected cells.
    pub fn count(&self) -> usize {
        self.bits.iter().filter(|&&b| b).count()
    }

    /// The selected cells, row by row.
    pub fn cells(&self) -> Vec<(i64, i64)> {
        (0..self.h)
            .flat_map(|y| (0..self.w).map(move |x| (i64::from(x), i64::from(y))))
            .filter(|&(x, y)| self.get(x, y))
            .collect()
    }

    /// The smallest rectangle holding every selected cell.
    pub fn bounds(&self) -> Option<CellRect> {
        let cells = self.cells();
        let x0 = cells.iter().map(|c| c.0).min()?;
        let x1 = cells.iter().map(|c| c.0).max()?;
        let y0 = cells.iter().map(|c| c.1).min()?;
        let y1 = cells.iter().map(|c| c.1).max()?;
        Some(CellRect { x0: x0 as i32, y0: y0 as i32, x1: x1 as i32, y1: y1 as i32 })
    }

    /// Combines `shape` (a mask of the same map) with this selection by `mode`.
    pub fn combine(&mut self, shape: &Mask, mode: Mode) {
        if shape.bits.len() != self.bits.len() {
            return;
        }
        for (a, &b) in self.bits.iter_mut().zip(&shape.bits) {
            *a = match mode {
                Mode::Replace => b,
                Mode::Add => *a || b,
                Mode::Subtract => *a && !b,
            };
        }
    }

    /// Selects the cells that were not, and the reverse.
    pub fn invert(&mut self) {
        for b in &mut self.bits {
            *b = !*b;
        }
    }

    /// The sides between a selected cell and a cell that is not selected (the map's edge counts as
    /// not selected). Each side is listed once, oriented so that the selected cell is on its left.
    pub fn outline(&self) -> Vec<Edge> {
        let mut out = Vec::new();
        for y in 0..self.h as i64 {
            for x in 0..self.w as i64 {
                if !self.get(x, y) {
                    continue;
                }
                let (x, y) = (x as i32, y as i32);
                if !self.get(i64::from(x), i64::from(y) - 1) {
                    out.push(Edge { a: (x, y), b: (x + 1, y) });
                }
                if !self.get(i64::from(x) + 1, i64::from(y)) {
                    out.push(Edge { a: (x + 1, y), b: (x + 1, y + 1) });
                }
                if !self.get(i64::from(x), i64::from(y) + 1) {
                    out.push(Edge { a: (x + 1, y + 1), b: (x, y + 1) });
                }
                if !self.get(i64::from(x) - 1, i64::from(y)) {
                    out.push(Edge { a: (x, y + 1), b: (x, y) });
                }
            }
        }
        out
    }
}

/// Whether the point is inside the polygon (even-odd rule: a ray to the right crosses an odd number
/// of sides).
fn inside(pts: &[(f64, f64)], px: f64, py: f64) -> bool {
    let mut hit = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let ((xi, yi), (xj, yj)) = (pts[i], pts[j]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

/// The cells of `cells` that lie inside the selection (all of them when there is none).
pub fn keep_inside(cells: Vec<(i64, i64)>, within: Option<&Mask>) -> Vec<(i64, i64)> {
    match within {
        Some(m) => cells.into_iter().filter(|&(x, y)| m.get(x, y)).collect(),
        None => cells,
    }
}

/// The fill of the cells connected to `start` that have its code, walking only through cells of
/// the selection (the fill stays inside it). Empty when `start` is outside the map or the selection.
pub fn flood_within(codes: &[u8], w: u32, h: u32, start: (u32, u32), within: &Mask) -> Vec<(u32, u32)> {
    if start.0 >= w || start.1 >= h || !within.get(i64::from(start.0), i64::from(start.1)) {
        return Vec::new();
    }
    let want = codes[(start.1 * w + start.0) as usize];
    let mut seen = vec![false; (w * h) as usize];
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([start]);
    seen[(start.1 * w + start.0) as usize] = true;
    while let Some((x, y)) = queue.pop_front() {
        out.push((x, y));
        let around = [(i64::from(x) - 1, i64::from(y)), (i64::from(x) + 1, i64::from(y)), (i64::from(x), i64::from(y) - 1), (i64::from(x), i64::from(y) + 1)];
        for (nx, ny) in around {
            if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) || !within.get(nx, ny) {
                continue;
            }
            let i = (ny as u32 * w + nx as u32) as usize;
            if !seen[i] && codes[i] == want {
                seen[i] = true;
                queue.push_back((nx as u32, ny as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells_of(m: &Mask) -> Vec<(i64, i64)> {
        m.cells()
    }

    #[test]
    fn a_rectangle_is_clipped_to_the_map() {
        let m = Mask::of_rect(4, 3, CellRect { x0: 2, y0: 1, x1: 9, y1: 9 });
        assert_eq!(cells_of(&m), vec![(2, 1), (3, 1), (2, 2), (3, 2)]);
        assert!(Mask::of_rect(4, 3, CellRect { x0: 8, y0: 8, x1: 9, y1: 9 }).is_empty());
    }

    #[test]
    fn a_lasso_takes_the_cells_whose_centre_is_inside() {
        // A triangle over the cells (0,0)..(3,3) with no centre on its hypotenuse: x + y <= 3 is inside.
        let tri = [(0.0, 0.0), (4.2, 0.0), (0.0, 4.2)];
        let m = Mask::of_polygon(4, 4, &tri);
        assert!(m.get(0, 0) && m.get(1, 0) && m.get(0, 2) && m.get(2, 1));
        assert!(!m.get(3, 3) && !m.get(2, 2), "the cells on the far side of the hypotenuse are out");
        assert_eq!(m.count(), 10, "a triangle of side 4 holds 10 centres");
        assert!(Mask::of_polygon(4, 4, &tri[..2]).is_empty(), "fewer than three points selects nothing");
    }

    #[test]
    fn a_wand_is_contiguous_or_global() {
        // Two islands of code 5 on a 4 by 2 map, split by code 9 in the middle column.
        let codes = [5, 9, 5, 5, 5, 9, 5, 5];
        let contiguous = Mask::of_wand(&codes, 4, 2, (0, 0), true);
        assert_eq!(contiguous.count(), 2, "only the cells touching (0,0)");
        let global = Mask::of_wand(&codes, 4, 2, (0, 0), false);
        assert_eq!(global.count(), 6, "every code 5 on the map");
        assert!(global.get(3, 1) && !contiguous.get(3, 1));
        assert!(Mask::of_wand(&codes, 4, 2, (9, 9), true).is_empty());
    }

    #[test]
    fn add_subtract_replace_and_invert() {
        let a = Mask::of_rect(4, 1, CellRect { x0: 0, y0: 0, x1: 1, y1: 0 });
        let b = Mask::of_rect(4, 1, CellRect { x0: 1, y0: 0, x1: 2, y1: 0 });
        let mut m = a.clone();
        m.combine(&b, Mode::Add);
        assert_eq!(m.count(), 3);
        m.combine(&b, Mode::Subtract);
        assert_eq!(cells_of(&m), vec![(0, 0)]);
        m.combine(&b, Mode::Replace);
        assert_eq!(cells_of(&m), vec![(1, 0), (2, 0)]);
        m.invert();
        assert_eq!(cells_of(&m), vec![(0, 0), (3, 0)]);
        assert_eq!(mode_of(true, false, Mode::Replace), Mode::Add);
        assert_eq!(mode_of(false, true, Mode::Add), Mode::Subtract);
        assert_eq!(mode_of(false, false, Mode::Subtract), Mode::Subtract, "no modifier: the bar's mode");
    }

    #[test]
    fn the_outline_of_an_l_shape_is_one_closed_loop_of_eight_sides() {
        // The L: (0,0), (1,0) and (0,1).
        let m = Mask::of_cells(3, 3, &[(0, 0), (1, 0), (0, 1)]);
        let edges = m.outline();
        assert_eq!(edges.len(), 8, "a three-cell L has eight sides");
        // Following the sides from one corner visits every side once.
        let mut at = edges[0].a;
        let mut seen = 0;
        loop {
            let e = edges.iter().find(|e| e.a == at).expect("the loop is closed");
            seen += 1;
            at = e.b;
            if at == edges[0].a {
                break;
            }
        }
        assert_eq!(seen, edges.len(), "one loop through all the sides");
        assert!(edges.iter().all(|e| (e.a.0 - e.b.0).abs() + (e.a.1 - e.b.1).abs() == 1), "unit sides");
    }

    #[test]
    fn only_in_selection_clips_a_brush_stroke_and_a_fill() {
        let within = Mask::of_rect(6, 1, CellRect { x0: 2, y0: 0, x1: 3, y1: 0 });
        let stroke = brush::stroke((0, 0), (5, 0), 1, brush::Shape::Square);
        assert_eq!(stroke.len(), 6);
        let kept = keep_inside(stroke, Some(&within));
        assert_eq!(kept, vec![(2, 0), (3, 0)], "the stroke keeps the two cells inside");
        assert_eq!(keep_inside(vec![(0, 0)], None), vec![(0, 0)], "no selection: nothing is clipped");

        // A fill of one code across all six cells, from (0,0): it stops at the selection's edge.
        let codes = [1u8; 6];
        let filled = flood_within(&codes, 6, 1, (2, 0), &within);
        assert_eq!(filled, vec![(2, 0), (3, 0)], "the fill walks only inside the selection");
        assert!(flood_within(&codes, 6, 1, (0, 0), &within).is_empty(), "a start outside the selection fills nothing");
    }
}
