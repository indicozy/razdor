//! The map state the generators write, as the editor's own layers kept it (DTMapEdit's cell
//! grid, its object grid and its placement tools, docs/reference/editor/mapcheck-files.md
//! §1, §3.8, worldgen.md §3.5): the marks of a cell, the scratch byte the generators leave
//! their flags in, the two object slots of a cell, and the placement of a building or an
//! army. The records are the dtm records ([`Scenario`], [`Building`], [`Army`], [`MapObject`]),
//! the map is a [`Scenario`] with its header; nothing here is drawn or read from the editor's
//! palettes, names or settings.
//!
//! What is left out, and why: the cell layer's figures and reveal counters (fog and map
//! pictures, read by no generator), the building and army names (text keys in a package;
//! their draws are kept, see [`name_draws`]), and the editor's undo and drawing tools.

use crate::dt::dtm::{Army, Building, MapObject, Scenario};
use crate::rules::rng::Rng;

/// The terrain a generator's building stands on until its step ends (worldgen.md §3.5).
pub const TEMPORARY_TERRAIN: u8 = 16;

/// The original places a building only while fewer than this many exist.
pub const MAX_BUILDINGS: usize = 254;
/// Armies the original places at most.
pub const MAX_ARMIES: usize = 255;

/// The mark values of building cells: inside, on the first row or column, on the last.
pub const MARK_BUILDING: i8 = -127;
pub const MARK_BUILDING_FIRST: i8 = -126;
pub const MARK_BUILDING_LAST: i8 = -125;

/// The header defaults of a new map: the relation matrix of a new map (newmap.md §11).
pub const DEFAULT_RELATIONS: [[i8; 4]; 4] = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]];

/// An empty map `width` × `height` filled with terrain `fill` (the editor's new map,
/// defaults.rs): a header with the relations of a new map and no records.
pub fn new_scenario(width: u32, height: u32, fill: u8) -> Scenario {
    let mut s = Scenario::default();
    s.header.width = width;
    s.header.height = height;
    s.header.relations = DEFAULT_RELATIONS;
    s.terrain = vec![fill.min(15); width as usize * height as usize];
    s
}

/// The figure-free record of an army (defaults.rs `new_army`): id, cell and map model
/// (4 feudal, 5 bandits, 6 peasants, 7 inactive); the editor's name is left empty.
pub fn new_army(id: u8, x: u16, y: u16, model: u8) -> Army {
    let model = model.clamp(4, 7);
    let inactive = model == 7;
    Army {
        x,
        y,
        id,
        model,
        behaviour: if inactive { 0 } else { model - 4 },
        inactive: inactive as u8,
        exp_correction: 100,
        ..Army::default()
    }
}

/// Why a placement did nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The record list is full.
    Full,
    /// A building is already anchored on the cell.
    Occupied,
    /// The brush square reaches outside the map.
    Outside,
}

/// The record type of a new building of picture `(picture_type, variant)`: the picture type,
/// except houses 2–4, which are obelisks (15), and 5–6, ruins (12).
pub fn building_kind(picture_type: u8, variant: u8) -> u8 {
    match (picture_type, variant) {
        (8, 2..=4) => 15,
        (8, 5..=6) => 12,
        (t, _) => t,
    }
}

/// The garrison defence a new building of picture type `t` starts with: town 20, village 2,
/// castle 15, fort 10, ruins 5 (the picture type, so houses made ruins get none).
pub fn building_defence(t: u8) -> u8 {
    match t {
        1 => 20,
        2 => 2,
        3 => 15,
        4 => 10,
        12 => 5,
        _ => 0,
    }
}

/// The draws a new building's names take (naming.rs `building_names`, 0x5959a0–0x5961f0):
/// the generator is first set to `x·11 + y·7 + variant·3 + picture type`, then the names are
/// drawn by picture type. The names themselves are text keys in a package and are not made,
/// but the draws are kept, because they set what the generator draws next (the original's
/// behaviour). Every draw advances the generator the same way whatever the list length.
pub fn name_draws(rng: &mut Rng, x: u16, y: u16, picture_type: u8, variant: u8) {
    *rng = Rng::new((x as u32).wrapping_mul(11).wrapping_add((y as u32).wrapping_mul(7)).wrapping_add(variant as u32 * 3).wrapping_add(picture_type as u32));
    match picture_type {
        // Village: variants 3 and 4 first choose a pair; then a name and an owner name.
        2 => {
            if matches!(variant, 3 | 4) {
                rng.random(2);
            }
            rng.random(1);
            rng.random(1);
        }
        // Castle: a name, then an owner name.
        3 => {
            rng.random(1);
            rng.random(1);
        }
        // House: picture 5 one of three ruin names; picture 6 a fixed one; no draw otherwise.
        8 => {
            if variant == 5 {
                rng.random(3);
            }
        }
        // Altar: variants 0 to 4 one draw each.
        10 => {
            if variant <= 4 {
                rng.random(3);
            }
        }
        // Ruins: variants 0 to 8 one draw each (variant 8 among the three-entry ones).
        12 => {
            if variant <= 8 {
                rng.random(3);
            }
        }
        // Every other type: a name, then an owner name.
        _ => {
            rng.random(1);
            rng.random(1);
        }
    }
}

/// The cells' marks, scratch flags and anchors the generators read and write (the editor's
/// cell layer without the figures and fog, see the module note).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    width: u32,
    height: u32,
    marks: Vec<i8>,
    /// The cells' scratch byte, which the new-map generator leaves its flags in and the world
    /// generator its placement mask. Cleared on a load.
    scratch: Vec<u8>,
}

impl Layer {
    /// The cells as opening `s` leaves them: the marks built in full.
    pub fn load(s: &Scenario) -> Layer {
        let (w, h) = (s.width(), s.height());
        let n = w as usize * h as usize;
        let mut c = Layer { width: w, height: h, marks: vec![0; n], scratch: vec![0; n] };
        c.rebuild_marks(s);
        c
    }

    /// The marks built in full again.
    pub fn rebuild_marks(&mut self, s: &Scenario) {
        let m = Marks::build(s);
        if m.values().len() == self.marks.len() {
            self.marks.copy_from_slice(m.values());
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

    /// Every scratch byte at once, as the new-map generator leaves its flags; nothing for
    /// another size.
    pub fn set_scratch(&mut self, v: &[u8]) {
        if v.len() == self.scratch.len() {
            self.scratch.copy_from_slice(v);
        }
    }
}

/// The objects of a map in the editor's grid: per cell one object of each slot, class 1–8 in
/// the first and every other class in the second (grid.rs `ObjectGrid`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectGrid {
    width: u32,
    height: u32,
    cells: Vec<[Option<MapObject>; 2]>,
}

/// Object classes the grid holds: records of class 13 or more are dropped on load.
const MAX_OBJECT_CLASS: u8 = 12;

/// The cell slot an object goes to: classes 1–8 the first, any other class the second.
pub fn object_slot(class: u8) -> usize {
    if (1..=8).contains(&class) {
        0
    } else {
        1
    }
}

/// The original's grid word of an object: sprite in the low byte, class in the high one.
fn word(o: &MapObject) -> u16 {
    o.sprite as u16 | (o.class as u16) << 8
}

impl ObjectGrid {
    /// Puts `objects` into the grid in list order: a later object replaces an earlier one of
    /// the same slot (DTMapEdit 0x5a8c00). Objects of class 13 or more, objects outside the
    /// map and the empty word (class 0, sprite 0) are left out.
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

    /// The object list a save writes: the grid row by row, per cell the first slot, then the
    /// second.
    pub fn objects(&self) -> Vec<MapObject> {
        self.cells.iter().flat_map(|c| c.iter().flatten().copied()).collect()
    }
}

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
    /// 2. each first-slot object (classes 1–8), cell by cell row by row, writes minus its class
    ///    over the n × n cells ending at it (n = sprite div 10), on cells not yet negative;
    /// 3. each second-slot object writes minus its class on its own cell, unless that cell is
    ///    lava, impassable swamp or snowdrifts, or already holds a negative mark other than
    ///    minus 1–4 or minus 12; the word 0x000B (class 0, sprite 11) always writes;
    /// 4. building footprints take −127, −126 on their first row and column, −125 on their last.
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
}

/// The terrain code of a cell; 0 outside the map.
pub fn terrain_or_zero(s: &Scenario, x: i64, y: i64) -> u8 {
    if x < 0 || y < 0 {
        return 0;
    }
    s.terrain_at(x as u32, y as u32).unwrap_or(0)
}

/// The brush square of `size` cells ending at `(x, y)` lies inside the map.
fn square_inside(s: &Scenario, x: i64, y: i64, size: u32) -> bool {
    let n = size.max(1) as i64;
    let (x0, y0) = (x - n + 1, y - n + 1);
    x0 >= 0 && y0 >= 0 && x < s.width() as i64 && y < s.height() as i64
}

fn inside(s: &Scenario, x: i64, y: i64) -> bool {
    x >= 0 && y >= 0 && x < s.width() as i64 && y < s.height() as i64
}

/// Where and what [`place_building_in`] places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingAt {
    pub x: i64,
    pub y: i64,
    pub picture: (u8, u8),
    pub size: (u8, u8),
    pub brush: u32,
}

/// Places a building anchored at `(cx, cy)` (its bottom-right cell) with picture
/// `(picture_type, variant)` and footprint `size` (the buildings brush, brush.rs
/// `place_building_in`). It needs fewer than 254 buildings, no building anchored on the cell
/// and the brush square inside the map. The record is made, then the names are drawn
/// ([`name_draws`]), the footprint's cells lose their plants (or, while `generating`, take
/// [`TEMPORARY_TERRAIN`] when the picture type is below 13) and get the building mark, none
/// of them when the footprint reaches past the map's left or top edge. Returns the record's
/// number (1-based) and whether a plant was cleared.
pub fn place_building_in(s: &mut Scenario, objects: &mut ObjectGrid, cells: &mut Layer, rng: &mut Rng, at: BuildingAt, generating: bool) -> Result<(u16, bool), Refused> {
    let BuildingAt { x: cx, y: cy, picture, size, brush } = at;
    if s.buildings.len() >= MAX_BUILDINGS {
        return Err(Refused::Full);
    }
    if s.buildings.iter().any(|b| (b.x as i64, b.y as i64) == (cx, cy)) {
        return Err(Refused::Occupied);
    }
    if !square_inside(s, cx, cy, brush) {
        return Err(Refused::Outside);
    }
    let (picture_type, variant) = picture;
    let (x, y) = (cx as u16, cy as u16);
    name_draws(rng, x, y, picture_type, variant);
    let kind = building_kind(picture_type, variant);
    let b = Building {
        x,
        y,
        picture_type,
        picture_variant: variant,
        kind,
        size_x: size.0,
        size_y: size.1,
        owner_army: 0,
        faction: 3,
        relations: s.header.relations[2],
        garrison_extra_defence: building_defence(picture_type),
        ..Building::default()
    };
    s.buildings.push(b);
    let mut cleared = false;
    let w = s.width() as usize;
    let (x0, y0) = (cx - size.0 as i64 + 1, cy - size.1 as i64 + 1);
    // A footprint reaching left of or above the map stops the original with a range error
    // after the record is made, at its first (top-left) cell: no footprint cell is written.
    if x0 < 0 || y0 < 0 {
        return Ok((s.buildings.len() as u16, false));
    }
    for fx in x0..=cx {
        for fy in y0..=cy {
            if !inside(s, fx, fy) {
                continue;
            }
            if !generating {
                cleared |= objects.at(fx, fy)[1].is_some();
                objects.set(fx, fy, 1, None);
            } else if picture_type < 13 {
                // The picture type, not the record's (0x596c10): a house made an obelisk
                // (type 15) still stands on terrain 16.
                s.terrain[fy as usize * w + fx as usize] = TEMPORARY_TERRAIN;
            }
            cells.set_mark(fx, fy, MARK_BUILDING);
        }
    }
    Ok((s.buildings.len() as u16, cleared))
}

/// Places an army of model `kind` (4–7) at `(cx, cy)` (the items brush, brush.rs
/// `place_item`): at most [`MAX_ARMIES`] armies, the new one numbered after the last. Returns
/// its id; nothing else is checked, as the original's placement checks nothing.
pub fn place_army(s: &mut Scenario, cx: i64, cy: i64, kind: u8) -> Result<u8, Refused> {
    if s.armies.len() >= MAX_ARMIES {
        return Err(Refused::Full);
    }
    let n = s.armies.len() as u8 + 1;
    s.armies.push(new_army(n, cx as u16, cy as u16, kind));
    Ok(n)
}
