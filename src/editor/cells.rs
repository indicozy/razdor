//! The original editor's cell state besides the scenario itself
//! (docs/reference/editor/main-window.md, conventions, §8.3, §8.4, §10.6): per cell the
//! *mark* byte, the *figure word* (kind in the high byte, record index in the low one) and
//! the *reveal counter* the fog overlay reads.
//!
//! The original keeps them in its cell grid and changes them only where its tools do, so they
//! drift from the records: a deleted army leaves the cell blank even when another army stands
//! there, a placed figure hides the one it overwrote, deleting a lit lantern leaves its area
//! revealed, painting writes marks the map has no object for. [`CellLayer::load`] builds them
//! as opening a map does; the editing commands then change them as the original's tools do.
//! The original has no undo: after an undo or a redo Razdor builds them again as a load does.

use crate::dt::dtm::{Army, Point, Scenario};

use super::geometry::Footprint;
use super::grid::Marks;

/// The figure word of an army or a point: the record's word at byte 4 (id in the low byte,
/// map model in the high one).
pub fn army_word(a: &Army) -> u16 {
    a.id as u16 | (a.model as u16) << 8
}

pub fn point_word(p: &Point) -> u16 {
    p.id as u16 | (p.model as u16) << 8
}

/// The kind of a figure word (1–3 hero start, 4–7 army, 8 lantern, 9 event point, 10 target
/// place).
pub fn figure_kind(word: u16) -> u8 {
    (word >> 8) as u8
}

/// The record index of a figure word (0 for a hero start).
pub fn figure_index(word: u16) -> u8 {
    word as u8
}

/// What the reveal routine (0x594e04) does to the counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal {
    Add,
    Remove,
}

/// The hero starts' reveal radius.
pub const HERO_REVEAL: u16 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellLayer {
    width: u32,
    height: u32,
    marks: Vec<i8>,
    figures: Vec<u16>,
    reveal: Vec<u8>,
    /// The cells' scratch byte (+0xc), which the new-map generator leaves its flags in and
    /// the world generator its placement mask; cleared on a load (whether the original's
    /// load clears it is not known).
    scratch: Vec<u8>,
}

impl CellLayer {
    /// The cells as opening `s` leaves them (0x5a6c20): the marks built in full (0x5a2ed4),
    /// then the figures of the armies, of the hero starts that are set (neither coordinate 0)
    /// and of the points, later ones replacing earlier ones; each hero start reveals radius
    /// 5 and each point with a radius that is active its radius.
    pub fn load(s: &Scenario) -> CellLayer {
        let (w, h) = (s.width(), s.height());
        let n = w as usize * h as usize;
        let mut c = CellLayer { width: w, height: h, marks: vec![0; n], figures: vec![0; n], reveal: vec![0; n], scratch: vec![0; n] };
        c.rebuild_marks(s);
        c.place_figures(s, true);
        c
    }

    /// The marks built in full again (the grid switched on, a building dropped, hills
    /// deleted).
    pub fn rebuild_marks(&mut self, s: &Scenario) {
        let m = Marks::build(s);
        if m.values().len() == self.marks.len() {
            self.marks.copy_from_slice(m.values());
        }
    }

    /// The figures written again as a load writes them, the fog left as it is (after a
    /// record delete of Razdor's panels, which the original does not have).
    pub fn rebuild_figures(&mut self, s: &Scenario) {
        self.figures.iter_mut().for_each(|f| *f = 0);
        self.place_figures(s, false);
    }

    fn place_figures(&mut self, s: &Scenario, reveal: bool) {
        for a in &s.armies {
            self.set_figure(a.x as i64, a.y as i64, army_word(a));
        }
        for (k, hero) in s.header.heroes.iter().enumerate() {
            let (x, y) = (hero.x as u32, hero.y as u32);
            if x != 0 && x < self.width && y != 0 && y < self.height {
                self.set_figure(x as i64, y as i64, (k as u16 + 1) << 8);
                if reveal {
                    self.reveal(x as i64, y as i64, HERO_REVEAL, Reveal::Add);
                }
            }
        }
        for p in &s.points {
            self.set_figure(p.x as i64, p.y as i64, point_word(p));
            if reveal && p.radius != 0 && p.active != 0 {
                self.reveal(p.x as i64, p.y as i64, p.radius as u16, Reveal::Add);
            }
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn index(&self, x: i64, y: i64) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64).then(|| y as usize * self.width as usize + x as usize)
    }

    /// The mark of a cell; 0 outside the map.
    pub fn mark(&self, x: i64, y: i64) -> i8 {
        self.index(x, y).map_or(0, |i| self.marks[i])
    }

    /// Every mark at once, row by row (as a generator leaves them); nothing for another size.
    pub fn set_marks(&mut self, marks: &[i8]) {
        if marks.len() == self.marks.len() {
            self.marks.copy_from_slice(marks);
        }
    }

    /// Every cell's mark, row by row.
    pub fn marks(&self) -> &[i8] {
        &self.marks
    }

    pub fn set_mark(&mut self, x: i64, y: i64, v: i8) {
        if let Some(i) = self.index(x, y) {
            self.marks[i] = v;
        }
    }

    /// Every cell's scratch byte, row by row.
    pub fn scratch(&self) -> &[u8] {
        &self.scratch
    }

    pub fn scratch_mut(&mut self) -> &mut [u8] {
        &mut self.scratch
    }

    /// Every scratch byte at once (as a generator leaves them); nothing for another size.
    pub fn set_scratch(&mut self, v: &[u8]) {
        if v.len() == self.scratch.len() {
            self.scratch.copy_from_slice(v);
        }
    }

    /// The figure word of a cell; 0 outside the map.
    pub fn figure(&self, x: i64, y: i64) -> u16 {
        self.index(x, y).map_or(0, |i| self.figures[i])
    }

    /// Writes a figure word; nothing outside the map (where the original writes past its
    /// grid).
    pub fn set_figure(&mut self, x: i64, y: i64, word: u16) {
        if let Some(i) = self.index(x, y) {
            self.figures[i] = word;
        }
    }

    /// The reveal counter of a cell (0: under the fog); 0 outside the map.
    pub fn revealed(&self, x: i64, y: i64) -> u8 {
        self.index(x, y).map_or(0, |i| self.reveal[i])
    }

    /// The reveal routine (0x594e04): every map cell whose distance from `(x, y)`, rounded,
    /// is at most `radius` gets one more (or one less) on its byte counter, which wraps: a
    /// remove where nothing was revealed leaves 255, which shows as revealed. A radius of 128
    /// or more stops the original with a range error; Razdor changes nothing then.
    pub fn reveal(&mut self, x: i64, y: i64, radius: u16, op: Reveal) {
        if radius >= 128 {
            return;
        }
        let r = radius as i64;
        for dx in -r..=r {
            for dy in -r..=r {
                let d = ((dx * dx + dy * dy) as f64).sqrt().round() as i64;
                if d > r {
                    continue;
                }
                if let Some(i) = self.index(x + dx, y + dy) {
                    let c = &mut self.reveal[i];
                    *c = match op {
                        Reveal::Add => c.wrapping_add(1),
                        Reveal::Remove => c.wrapping_sub(1),
                    };
                }
            }
        }
    }
}

/// The passability grid's tiles (§8.4).
pub const GRID_NORMAL: u8 = 0;
pub const GRID_HARD: u8 = 1;
pub const GRID_HARDER: u8 = 2;
pub const GRID_IMPASSABLE: u8 = 3;
pub const GRID_BUILDING: u8 = 4;
pub const GRID_SHALLOW: u8 = 5;
pub const GRID_DEEP: u8 = 6;

/// The cells a building footprint covers, by the anchor scan of the grid overlay: a
/// building anchored at most 8 cells right of and below a cell covers it when its
/// `size_x`×`size_y` footprint reaches it. `skip` is the building being moved (its anchor
/// is off the map then).
pub fn building_cover(s: &Scenario, skip: Option<u16>) -> Vec<bool> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let mut cover = vec![false; (w * h).max(0) as usize];
    for (k, b) in s.buildings.iter().enumerate() {
        if skip == Some(k as u16 + 1) {
            continue;
        }
        let f = Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y).main;
        for (x, y) in f.cells() {
            let (x, y) = (x as i64, y as i64);
            let near = b.x as i64 - x <= 8 && b.y as i64 - y <= 8;
            if near && x >= 0 && y >= 0 && x < w && y < h {
                cover[(y * w + x) as usize] = true;
            }
        }
    }
    cover
}

/// The passability tile of a cell (0x5ac9c3–0x5acd9a): from the terrain (shallows and coast
/// 5, deep sea 6, lava, bog and deep drifts 3, marsh and snow 1, the rest 0); then the mark:
/// mountains, rocks and thickets (−5..−8, −11) make it 3, low hills, trees and dead trees
/// (−1..−4, −9, −10) add one step to a value below 2, bushes (−12) leave it; lava is always
/// 3; a building footprint ([`building_cover`]) makes it 4.
pub fn grid_kind(s: &Scenario, cells: &CellLayer, cover: &[bool], x: i64, y: i64) -> u8 {
    let code = super::grid::terrain_or_zero(s, x, y);
    let mut v = match code {
        0 | 1 => GRID_SHALLOW,
        2 => GRID_DEEP,
        3 | 9 | 15 => GRID_IMPASSABLE,
        8 | 14 => GRID_HARD,
        _ => GRID_NORMAL,
    };
    match cells.mark(x, y) {
        -11 | -8..=-5 => v = GRID_IMPASSABLE,
        -10 | -9 | -4..=-1 if v < GRID_HARDER => v += 1,
        _ => {}
    }
    if code == 3 {
        v = GRID_IMPASSABLE;
    }
    let w = s.width() as i64;
    if x >= 0 && y >= 0 && cover.get((y * w + x) as usize).copied().unwrap_or(false) {
        v = GRID_BUILDING;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Building, HeroPreset, MapObject};
    use crate::editor::{EditorDoc, NewMap};

    fn scenario() -> Scenario {
        EditorDoc::new_map(NewMap { width: 12, height: 12, fill: 6 }).scenario
    }

    #[test]
    fn reveal_is_a_rounded_circle_of_wrapping_counters() {
        let mut c = CellLayer::load(&Scenario { header: crate::dt::dtm::Header { width: 12, height: 12, ..Default::default() }, terrain: vec![6; 144], ..Default::default() });
        c.reveal(5, 5, 2, Reveal::Add);
        // Distance rounded: (2, 1) is √5 ≈ 2.24 → 2, inside; (2, 2) is √8 ≈ 2.83 → 3, outside.
        assert_eq!((c.revealed(7, 6), c.revealed(7, 7), c.revealed(5, 3), c.revealed(5, 2)), (1, 0, 1, 0));
        assert_eq!((0..12).flat_map(|x| (0..12).map(move |y| (x, y))).filter(|(x, y)| c.revealed(*x, *y) > 0).count(), 21);
        // Removing where nothing was revealed wraps to 255.
        c.reveal(0, 0, 1, Reveal::Remove);
        assert_eq!((c.revealed(0, 0), c.revealed(1, 1), c.revealed(2, 0)), (255, 255, 0));
        c.reveal(5, 5, 200, Reveal::Add);
        assert_eq!(c.revealed(11, 11), 0, "a radius of 128 or more changes nothing");
    }

    #[test]
    fn a_load_writes_armies_then_heroes_then_points() {
        let mut s = scenario();
        s.header.heroes = [HeroPreset { x: 3, y: 3, ..Default::default() }, HeroPreset { x: 0, y: 4, ..Default::default() }, HeroPreset { x: 6, y: 6, ..Default::default() }];
        s.armies.push(crate::editor::defaults::new_army(1, 3, 3, 5));
        s.armies.push(crate::editor::defaults::new_army(2, 8, 8, 4));
        let mut lit = crate::editor::defaults::new_point(1, 6, 6, 8);
        lit.radius = 2;
        s.points.push(lit);
        let mut dark = crate::editor::defaults::new_point(2, 9, 1, 8);
        dark.active = 0;
        s.points.push(dark);
        let c = CellLayer::load(&s);
        // The knight's start hides army 1; the ranger's is hidden by the lantern.
        assert_eq!((c.figure(3, 3), c.figure(8, 8), c.figure(6, 6), c.figure(0, 4)), (0x0100, 0x0402, 0x0801, 0));
        // Knight r 5 and ranger r 5 overlap at (4, 4); the archmage at x 0 is not set.
        assert_eq!((c.revealed(4, 4), c.revealed(0, 4), c.revealed(6, 8), c.revealed(10, 10)), (2, 1, 2, 0));
        // The unlit lantern reveals nothing.
        assert_eq!(c.revealed(9, 1), 0);
    }

    #[test]
    fn grid_tiles_follow_terrain_marks_and_buildings() {
        let mut s = scenario();
        for (x, code) in [(0, 0), (1, 2), (2, 3), (3, 8), (4, 9), (5, 6)] {
            s.terrain[x] = code;
        }
        s.terrain[12 + 3] = 8; // marsh under a tree: harder
        s.terrain[12 + 4] = 1; // coast under a tree: stays shallow
        s.terrain[12 + 5] = 3; // lava under a bush: impassable
        s.objects = vec![MapObject { x: 3, y: 1, class: 9, sprite: 1 }, MapObject { x: 4, y: 1, class: 9, sprite: 1 }, MapObject { x: 5, y: 1, class: 12, sprite: 1 }, MapObject { x: 7, y: 1, class: 9, sprite: 1 }, MapObject { x: 8, y: 1, class: 11, sprite: 1 }, MapObject { x: 9, y: 1, class: 12, sprite: 1 }];
        s.buildings.push(Building { x: 10, y: 10, size_x: 2, size_y: 2, ..Default::default() });
        let c = CellLayer::load(&s);
        let cover = building_cover(&s, None);
        let row = |y: i64| (0..12).map(|x| grid_kind(&s, &c, &cover, x, y)).collect::<Vec<_>>();
        assert_eq!(row(0)[..6], [GRID_SHALLOW, GRID_DEEP, GRID_IMPASSABLE, GRID_HARD, GRID_IMPASSABLE, GRID_NORMAL]);
        assert_eq!(row(1)[3..10], [GRID_HARDER, GRID_SHALLOW, GRID_IMPASSABLE, GRID_NORMAL, GRID_HARD, GRID_IMPASSABLE, GRID_NORMAL]);
        assert_eq!((row(9)[9], row(10)[10], row(8)[10]), (GRID_BUILDING, GRID_BUILDING, GRID_NORMAL));
        // The building being moved has no anchor on the map.
        assert_eq!(grid_kind(&s, &c, &building_cover(&s, Some(1)), 10, 10), GRID_NORMAL);
    }
}
