//! The original editor's cell grid, as far as its logic reads it: the two object slots of a
//! cell and the mark byte the map check and the playability score look at
//! (docs/reference/editor/mapcheck-files.md §1, §3.8, §4.4).
//!
//! Razdor keeps objects as a list; the original keeps them in its grid, one of classes 1–8
//! and one of the others per cell, so loading and saving pass the list through
//! [`ObjectGrid`]. Marks are rebuilt here whenever they are asked for; the original rebuilds
//! them only on load, when the grid overlay is switched on and after a building is dropped.

use crate::dt::dtm::{MapObject, Scenario};

/// Object classes the grid holds: records of class 13 or more are dropped on load.
pub const MAX_OBJECT_CLASS: u8 = 12;

/// The cell slot an object goes to: classes 1–8 the first, any other class the second.
pub fn object_slot(class: u8) -> usize {
    if (1..=8).contains(&class) {
        0
    } else {
        1
    }
}

/// The original's grid word of an object: sprite in the low byte, class in the high one.
/// A word of 0 is an empty slot.
fn word(o: &MapObject) -> u16 {
    o.sprite as u16 | (o.class as u16) << 8
}

/// The objects of a map in the original's grid: per cell one object of each slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectGrid {
    width: u32,
    height: u32,
    cells: Vec<[Option<MapObject>; 2]>,
}

impl ObjectGrid {
    /// Puts `objects` into the grid in list order: a later object replaces an earlier one
    /// of the same slot (DTMapEdit 0x5a8c00). Objects of class 13 or more, objects outside
    /// the map (which the original keeps off the map, where no save reaches them) and the
    /// empty word (class 0, sprite 0) are left out.
    pub fn from_objects(width: u32, height: u32, objects: &[MapObject]) -> ObjectGrid {
        let mut cells = vec![[None, None]; width as usize * height as usize];
        for o in objects {
            if o.class > MAX_OBJECT_CLASS || word(o) == 0 || o.x as u32 >= width || o.y as u32 >= height {
                continue;
            }
            cells[o.y as usize * width as usize + o.x as usize][object_slot(o.class)] = Some(*o);
        }
        ObjectGrid { width, height, cells }
    }

    /// The two slots of a cell (empty outside the map).
    pub fn at(&self, x: i64, y: i64) -> [Option<MapObject>; 2] {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return [None, None];
        }
        self.cells[y as usize * self.width as usize + x as usize]
    }

    /// Puts `o` into slot `slot` of a cell (`None` empties it); nothing outside the map.
    pub fn set(&mut self, x: i64, y: i64, slot: usize, o: Option<MapObject>) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        self.cells[y as usize * self.width as usize + x as usize][slot] = o;
    }

    /// The object list a save writes (0x5a4c44): the grid row by row, per cell the first
    /// slot, then the second.
    pub fn objects(&self) -> Vec<MapObject> {
        self.cells.iter().flat_map(|c| c.iter().flatten().copied()).collect()
    }
}

/// The objects as the original keeps them, in the order a save writes them.
pub fn rebuild_objects(s: &Scenario) -> Vec<MapObject> {
    ObjectGrid::from_objects(s.width(), s.height(), &s.objects).objects()
}

/// Mark values of building cells: inside, on the first row or column, on the last.
pub const MARK_BUILDING: i8 = -127;
pub const MARK_BUILDING_FIRST: i8 = -126;
pub const MARK_BUILDING_LAST: i8 = -125;

/// The signed mark byte of every cell (DTMapEdit 0x5a2ed4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marks {
    width: u32,
    height: u32,
    cells: Vec<i8>,
}

impl Marks {
    /// The marks of `s`, in the original's four passes:
    /// 1. every cell gets its terrain code;
    /// 2. each first-slot object (classes 1–8), cell by cell row by row, writes minus its
    ///    class over the n × n cells ending at it (n = sprite div 10), on cells not yet
    ///    negative, so the first object there wins;
    /// 3. each second-slot object writes minus its class on its own cell, unless that cell
    ///    is lava, impassable swamp or snowdrifts, or already holds a negative mark other
    ///    than minus 1–4 or minus 12; the word 0x000B (class 0, sprite 11) always writes;
    /// 4. building footprints (bytes 289–290, ending at x/y) take −127, −126 on their first
    ///    row and column, −125 on their last.
    ///
    /// Footprint cells left of or above the map stop the original with a range error; they
    /// are skipped here.
    pub fn build(s: &Scenario) -> Marks {
        let (w, h) = (s.width(), s.height());
        let mut m = Marks { width: w, height: h, cells: s.terrain.iter().map(|c| *c as i8).collect() };
        let grid = ObjectGrid::from_objects(w, h, &s.objects);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let Some(o) = grid.at(x, y)[0] else { continue };
                let n = (o.sprite / 10) as i64;
                for dx in (1 - n)..=0 {
                    for dy in (1 - n)..=0 {
                        if let Some(c) = m.cell_mut(x + dx, y + dy) {
                            if *c >= 0 {
                                *c = -(o.class as i8);
                            }
                        }
                    }
                }
            }
        }
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let Some(o) = grid.at(x, y)[1] else { continue };
                let c = m.cell_mut(x, y).expect("inside");
                let mark = *c;
                let keeps = mark < 0 || (mark > 2 && !(4..=8).contains(&mark) && !(10..=14).contains(&mark));
                let overwritable = (-4..=-1).contains(&mark) || mark == -12;
                if word(&o) == 0x000B || !keeps || overwritable {
                    *c = -(o.class as i8);
                }
            }
        }
        for b in &s.buildings {
            for dx in 0..b.size_x as i64 {
                for dy in 0..b.size_y as i64 {
                    let value = if dx == b.size_x as i64 - 1 || dy == b.size_y as i64 - 1 {
                        MARK_BUILDING_LAST
                    } else if dx == 0 || dy == 0 {
                        MARK_BUILDING_FIRST
                    } else {
                        MARK_BUILDING
                    };
                    if let Some(c) = m.cell_mut(b.x as i64 - dx, b.y as i64 - dy) {
                        *c = value;
                    }
                }
            }
        }
        m
    }

    fn cell_mut(&mut self, x: i64, y: i64) -> Option<&mut i8> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        self.cells.get_mut(y as usize * self.width as usize + x as usize)
    }

    /// Every cell's mark, row by row.
    pub fn values(&self) -> &[i8] {
        &self.cells
    }

    /// The mark of a cell; 0 outside the map (the original's cleared grid).
    pub fn at(&self, x: i64, y: i64) -> i8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return 0;
        }
        self.cells[y as usize * self.width as usize + x as usize]
    }
}

/// The terrain code of a cell; 0 outside the map (the original's cleared grid).
pub fn terrain_or_zero(s: &Scenario, x: i64, y: i64) -> u8 {
    if x < 0 || y < 0 {
        return 0;
    }
    s.terrain_at(x as u32, y as u32).unwrap_or(0)
}

/// The original's impassable cell (0x556d70): deep sea, lava, impassable swamp or
/// snowdrifts, or a mark of −11 (thicket) or −8..−5 (mountains, rocks).
pub fn impassable(s: &Scenario, marks: &Marks, x: i64, y: i64) -> bool {
    let mark = marks.at(x, y);
    matches!(terrain_or_zero(s, x, y), 2 | 3 | 9 | 15) || mark == -11 || (-8..=-5).contains(&mark)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Building;
    use crate::editor::{EditorDoc, NewMap};

    fn obj(x: u16, y: u16, class: u8, sprite: u8) -> MapObject {
        MapObject { x, y, sprite, class }
    }

    #[test]
    fn a_cell_keeps_one_object_per_slot() {
        // Hills (1) and mountains (5) share the first slot, trees (9) and thickets (11) the
        // second: the later record wins. Class 13 and off-map objects are dropped.
        let list = [obj(2, 1, 1, 10), obj(2, 1, 9, 3), obj(2, 1, 5, 20), obj(2, 1, 11, 4), obj(0, 0, 13, 1), obj(9, 0, 9, 1), obj(1, 0, 0, 0)];
        let g = ObjectGrid::from_objects(4, 3, &list);
        assert_eq!(g.at(2, 1), [Some(obj(2, 1, 5, 20)), Some(obj(2, 1, 11, 4))]);
        assert_eq!(g.at(0, 0), [None, None]);
        // Saved row by row, the first slot first.
        let back = ObjectGrid::from_objects(4, 3, &[obj(3, 2, 9, 1), obj(0, 2, 1, 10), obj(0, 2, 9, 2), obj(1, 0, 9, 5)]).objects();
        assert_eq!(back, [obj(1, 0, 9, 5), obj(0, 2, 1, 10), obj(0, 2, 9, 2), obj(3, 2, 9, 1)]);
    }

    #[test]
    fn marks_follow_the_four_passes() {
        let mut d = EditorDoc::new_map(NewMap { width: 10, height: 10, fill: 6 });
        let s = &mut d.scenario;
        s.terrain[5 * 10 + 1] = 3; // lava at (1, 5)
        // A 2×2 hill ending at (3, 3), a mountain of size 3 ending at (4, 4): the hill's
        // cells stay −1.
        s.objects = vec![obj(3, 3, 1, 25), obj(4, 4, 5, 31), obj(1, 5, 9, 1), obj(3, 3, 9, 2), obj(4, 4, 9, 3), obj(6, 6, 9, 4), obj(7, 7, 0, 11)];
        s.terrain[6 * 10 + 6] = 1; // coastal water under a tree
        s.buildings.push(Building { x: 8, y: 2, size_x: 3, size_y: 2, ..Building::default() });
        let m = Marks::build(s);
        assert_eq!((m.at(2, 2), m.at(3, 3), m.at(2, 4), m.at(4, 2), m.at(4, 4)), (-1, -9, -5, -5, -5));
        // A tree on lava keeps the lava; on a hill it replaces it; on a mountain it does not;
        // on water it marks; class 0 sprite 11 always writes.
        assert_eq!((m.at(1, 5), m.at(4, 4), m.at(6, 6), m.at(7, 7)), (3, -5, -9, 0));
        // The building: last row and column −125, first −126, inside −127.
        assert_eq!((m.at(8, 2), m.at(7, 2), m.at(6, 1), m.at(7, 1), m.at(6, 2)), (MARK_BUILDING_FIRST, MARK_BUILDING_FIRST, MARK_BUILDING_LAST, MARK_BUILDING_LAST, MARK_BUILDING_LAST));
        assert_eq!(m.at(-1, 0), 0);
        assert!(impassable(s, &m, 1, 5) && impassable(s, &m, 4, 4) && !impassable(s, &m, 2, 2) && !impassable(s, &m, 8, 2));
    }
}
