//! Step 1, buildings and roads (worldgen.md §3, 0x56ffa4).

use crate::dt::dtm::{MapObject, Scenario};
use crate::rules::map::{Grid, TileMap, DIRECTIONS};
use crate::rules::rng::Rng;

use super::super::brush::{place_building_in, BuildingAt, TEMPORARY_TERRAIN};
use super::super::cells::CellLayer;
use super::super::grid::ObjectGrid;
use super::{Chance, Counters, Inputs, Options, Report, Stop};

/// The generator's own direction table (0x5beb0c, 0x5beb14): N, NE, E, SE, S, SW, W, NW.
const DX: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
const DY: [i32; 8] = [-1, -1, 0, 1, 1, 1, 0, -1];

/// Cost of each terrain code (0x5beb1c), −32 blocked; code 16 reads on into the forest table.
const TERRAIN_COST: [i32; 17] = [100, 150, -32, -32, 1, 8, 5, 7, 15, -32, 10, 8, 8, 10, 12, -32, 1];
/// Cost of a forest-layer object by class (0x5beb5c).
const FOREST_COST: [i32; 13] = [1, 4, 4, 4, 6, -32, -32, -32, -32, 8, 10, -32, 6];
/// Cost of an object by class − 1 (0x5bad48).
const OBJECT_COST: [i32; 12] = [2, 2, 2, 3, -32, -32, -32, -32, 4, 6, -32, 4];

/// A cost of this much or more is water (§3.4).
const WATER: u16 = 100;
const ROAD: u8 = 4;
const GRASS_PLAIN: u8 = 6;
/// Sectors are 50 cells square; the search radius of a placement.
const SECTOR: i32 = 50;
const RADIUS: i32 = 30;
/// The picture type of the stone bridge, whose pictures make every bridge.
const STONE_BRIDGE: u8 = 13;
/// Attempts at one road (§3.7).
const ATTEMPTS: usize = 6;
/// The quadrants of the (town, castle, village, village) of a sector, by `R(12)`.
const QUADRANTS: [[i32; 4]; 12] = [
    [0, 1, 2, 3],
    [0, 2, 1, 3],
    [0, 3, 1, 2],
    [1, 0, 2, 3],
    [1, 2, 0, 3],
    [1, 3, 0, 2],
    [2, 0, 1, 3],
    [2, 1, 0, 3],
    [2, 3, 0, 1],
    [3, 0, 1, 2],
    [3, 1, 0, 2],
    [3, 2, 0, 1],
];

fn terrain_cost(code: u8) -> i32 {
    TERRAIN_COST.get(code as usize).copied().unwrap_or(1)
}

pub(super) struct World<'a> {
    pub s: &'a mut Scenario,
    pub cells: &'a mut CellLayer,
    pub rng: &'a mut Rng,
    inp: &'a Inputs<'a>,
    pub objects: ObjectGrid,
    /// The cells' building code (+8): picture type and variant on an anchor cell.
    codes: Vec<Option<(u8, u8)>>,
    pub w: i32,
    pub h: i32,
    /// The "generating" flag (0x5bee90) the placement reads.
    generating: bool,
    pub counters: Counters,
    /// The planner's cost map after the second pass (§3.4).
    pub cost: Vec<u16>,
    map: TileMap,
    ones: Vec<u16>,
    /// The cost cells a change since the last rebuild may have changed.
    dirty: Vec<usize>,
    /// How far right and down of a cell a square written over it can stand: an object's
    /// (sprite div 10, at most 25) or a bridge's footprint.
    reach: i32,
}

/// Tests set this to check every incremental rebuild of the cost map against a full one.
#[cfg(test)]
pub(super) static VERIFY_COSTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn run(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    // The step starts and ends by building the marks again from the map (0x5a2ed4 at
    // 0x5700b2 and 0x572eaa): stale marks of earlier edits do not count, the old buildings'
    // footprints do.
    cells.rebuild_marks(s);
    let mut w = World::new(s, cells, rng, inp);
    let stop = w.step(o).err();
    // The original leaves the flag on and terrain 16 under its buildings when it stops half
    // way; Razdor's map keeps only real terrain codes.
    if stop.is_some() {
        w.clear_temporary();
    }
    let counters = w.counters;
    w.finish();
    if stop.is_none() {
        cells.rebuild_marks(s);
    }
    Report { counters, stop }
}

impl<'a> World<'a> {
    pub fn new(s: &'a mut Scenario, cells: &'a mut CellLayer, rng: &'a mut Rng, inp: &'a Inputs<'a>) -> World<'a> {
        let (w, h) = (s.width() as i32, s.height() as i32);
        let n = (w * h).max(0) as usize;
        let objects = ObjectGrid::from_objects(s.width(), s.height(), &s.objects);
        let mut codes = vec![None; n];
        for b in &s.buildings {
            if (b.x as i32) < w && (b.y as i32) < h {
                codes[b.y as usize * w as usize + b.x as usize] = Some((b.picture_type, b.picture_variant));
            }
        }
        World {
            s,
            cells,
            rng,
            inp,
            objects,
            codes,
            w,
            h,
            generating: false,
            counters: Counters::default(),
            cost: vec![0; n],
            map: TileMap::from_codes(Grid::Square8, w, h, &vec![0; n], Vec::new()),
            ones: vec![1; n],
            dirty: Vec::new(),
            reach: 26.max(inp.pictures.largest(13).max(inp.pictures.largest(14)) as i32),
        }
    }

    /// The objects written back (the original keeps them in its grid).
    pub fn finish(self) {
        self.s.objects = self.objects.objects();
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }

    fn terrain(&self, x: i32, y: i32) -> u8 {
        if self.inside(x, y) {
            self.s.terrain[self.idx(x, y)]
        } else {
            0
        }
    }

    fn set_terrain(&mut self, x: i32, y: i32, code: u8) {
        let i = self.idx(x, y);
        self.s.terrain[i] = code;
    }

    fn mark(&self, x: i32, y: i32) -> i8 {
        self.cells.mark(x as i64, y as i64)
    }

    fn set_mark(&mut self, x: i32, y: i32, v: i8) {
        self.cells.set_mark(x as i64, y as i64, v);
    }

    fn mask(&self, x: i32, y: i32) -> bool {
        self.inside(x, y) && self.cells.scratch()[self.idx(x, y)] != 0
    }

    fn set_mask(&mut self, x: i32, y: i32, on: bool) {
        if self.inside(x, y) {
            let i = self.idx(x, y);
            self.cells.scratch_mut()[i] = on as u8;
        }
    }

    fn forest(&self, x: i32, y: i32) -> Option<MapObject> {
        self.objects.at(x as i64, y as i64)[1]
    }

    fn set_forest(&mut self, x: i32, y: i32, o: Option<MapObject>) {
        self.objects.set(x as i64, y as i64, 1, o);
    }

    /// Every cell of temporary terrain 16 back to grass plain (§3.10 step 1).
    fn clear_temporary(&mut self) {
        for t in self.s.terrain.iter_mut() {
            if *t == TEMPORARY_TERRAIN {
                *t = GRASS_PLAIN;
            }
        }
    }

    // --------------------------------------------------------------------------------------
    // Placing
    // --------------------------------------------------------------------------------------

    /// The placement of picture `serial` anchored at `(x, y)` (0x595390), refused as the
    /// original refuses it; each placement reseeds the generator (quirk 3). Returns whether a
    /// building was made.
    fn place(&mut self, serial: Option<usize>, x: i32, y: i32) -> bool {
        let Some(p) = self.inp.pictures.get(serial) else { return false };
        // The cell's building code must be clear: codes left on the map's border by an
        // earlier map count (§3.1).
        if self.inside(x, y) && self.codes[self.idx(x, y)].is_some() {
            return false;
        }
        let at = BuildingAt { x: x as i64, y: y as i64, picture: (p.picture_type, p.variant), size: p.size, brush: self.inp.brush.max(1) };
        match place_building_in(self.s, &mut self.objects, self.cells, self.rng, at, self.inp.names, self.generating) {
            Ok(_) => {
                let i = self.idx(x, y);
                self.codes[i] = Some((p.picture_type, p.variant));
                if matches!(p.picture_type, 13 | 14) {
                    let targets = self.footprint_targets(x, y, p.size.0 as i32, p.size.1 as i32);
                    self.dirty.extend(targets);
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Whether every footprint cell of an `sx` × `sy` building anchored at `(x, y)` is on the
    /// map and masked (0x56f36c).
    fn fits(&self, x: i32, y: i32, sx: i32, sy: i32) -> bool {
        ((x - sx + 1)..=x).all(|fx| ((y - sy + 1)..=y).all(|fy| self.mask(fx, fy)))
    }

    /// The mask cleared over the box around a placement, `x − sx − 1 … x + 2` by
    /// `y − sy − 1 … y + 2` (the cells off the map are memory the map does not use).
    fn clear_box(&mut self, x: i32, y: i32, sx: i32, sy: i32) {
        for bx in (x - sx - 1)..=(x + 2) {
            for by in (y - sy - 1)..=(y + 2) {
                self.set_mask(bx, by, false);
            }
        }
    }

    /// The "spiral" search (0x56f410): from `(x, y)`, k = 4, 5, …, each round k div 4 single
    /// moves in direction k mod 4 (+x, +y, −x, −y), every cell reached tested, until a cell
    /// fits or k reaches 4 × radius. The moves 1, 1, 1, 1, 2, 2, 2, 2, 3, … trace squares of
    /// growing size with the start as their top-left corner, each ending back on the start,
    /// which is tested again (quirk 1). On a fit the building is placed and the box cleared;
    /// the search reports success even if the placement was refused.
    fn spiral(&mut self, x0: i32, y0: i32, radius: i32, serial: Option<usize>) -> bool {
        let Some(p) = self.inp.pictures.get(serial) else { return false };
        let (sx, sy) = (p.size.0 as i32, p.size.1 as i32);
        let (mut x, mut y) = (x0, y0);
        let mut k: i32 = 4;
        loop {
            let mut found = false;
            for _ in 0..k >> 2 {
                match k & 3 {
                    0 => x += 1,
                    1 => y += 1,
                    2 => x -= 1,
                    _ => y -= 1,
                }
                if self.fits(x, y, sx, sy) {
                    found = true;
                    break;
                }
            }
            k += 1;
            if found {
                self.place(serial, x, y);
                self.clear_box(x, y, sx, sy);
                return true;
            }
            if k >= 4 * radius {
                return false;
            }
        }
    }

    // --------------------------------------------------------------------------------------
    // The step
    // --------------------------------------------------------------------------------------

    fn step(&mut self, o: &Options) -> Result<(), Stop> {
        self.generating = true;
        self.free_footprint_roads()?;
        self.clean_roads();
        for _ in 0..5 {
            self.erode();
        }
        for (x, y) in [(0, 0), (0, self.h - 1), (self.w - 1, 0), (self.w - 1, self.h - 1)] {
            self.set_mask(x, y, false);
        }
        self.s.buildings.clear();
        // Randomize (0x57112b): the draws so far ran on the generator as the editor had it.
        *self.rng = Rng::new(self.inp.clock);
        let n = self.w / SECTOR;
        if n == 0 {
            return Err(Stop::NarrowMap);
        }
        let towns = self.sectors(o, n)?;
        self.build_costs();
        self.town_roads(&towns, n)?;
        self.nearest_roads();
        self.generating = false;
        self.junctions(o)?;
        self.clear_temporary();
        self.wild_mask();
        self.ruins(o, n);
        Ok(())
    }

    /// §3.1 item 2: road cells inside an existing building's footprint become grass plain.
    /// The cell reads are range-checked: a footprint reaching past the left or top edge (a
    /// building placed at the edge, whose own placement stopped there) stops the step at
    /// that cell.
    fn free_footprint_roads(&mut self) -> Result<(), Stop> {
        let feet: Vec<(i32, i32, i32, i32)> = self.s.buildings.iter().map(|b| (b.x as i32, b.y as i32, b.size_x as i32, b.size_y as i32)).collect();
        for (k, (bx, by, sx, sy)) in feet.into_iter().enumerate() {
            for x in (bx - sx + 1)..=bx {
                for y in (by - sy + 1)..=by {
                    if x < 0 || y < 0 {
                        return Err(Stop::FootprintOffMap { building: k as u16 + 1 });
                    }
                    if self.inside(x, y) && self.terrain(x, y) == ROAD {
                        self.set_terrain(x, y, GRASS_PLAIN);
                        self.set_mark(x, y, GRASS_PLAIN as i8);
                    }
                }
            }
        }
        Ok(())
    }

    /// §3.1 item 3: the road layer taken off the interior cells, and the placement mask.
    fn clean_roads(&mut self) {
        for x in 1..self.w - 1 {
            for y in 1..self.h - 1 {
                if self.terrain(x, y) == ROAD {
                    let wooded = DIRECTIONS.iter().filter(|(dx, dy)| self.forest(x + dx, y + dy).is_some()).count();
                    if wooded > 4 {
                        // A random wooded neighbour's tree, in the planner's directions.
                        loop {
                            let (dx, dy) = DIRECTIONS[self.rng.random(8) as usize];
                            if let Some(t) = self.forest(x + dx, y + dy) {
                                self.set_forest(x, y, Some(MapObject { x: x as u16, y: y as u16, ..t }));
                                break;
                            }
                        }
                    }
                    // The terrain below, else right, else below-right, unless that is road too.
                    let pick = [(0, 1), (1, 0), (1, 1)].into_iter().map(|(dx, dy)| self.terrain(x + dx, y + dy)).find(|&t| t != ROAD);
                    if let Some(t) = pick {
                        self.set_terrain(x, y, t);
                    }
                    if self.mark(x, y) == ROAD as i8 {
                        let t = self.terrain(x, y);
                        self.set_mark(x, y, t as i8);
                    }
                }
                let i = self.idx(x, y);
                self.codes[i] = None;
                let t = self.terrain(x, y);
                let on = matches!(t, 6 | 7) && self.mark(x, y) >= 0;
                self.set_mask(x, y, on);
            }
        }
    }

    /// One erosion pass over the interior, in place (§3.1 item 4).
    fn erode(&mut self) {
        for x in 1..self.w - 1 {
            for y in 1..self.h - 1 {
                if !self.mask(x, y) {
                    continue;
                }
                let around = (0..8).filter(|&d| self.mask(x + DX[d], y + DY[d])).count();
                if around < 3 {
                    self.set_mask(x, y, false);
                }
                if !self.mask(x + DX[0], y + DY[0]) && !self.mask(x + DX[4], y + DY[4]) {
                    self.set_mask(x, y, false);
                }
                if !self.mask(x + DX[2], y + DY[2]) && !self.mask(x + DX[6], y + DY[6]) {
                    self.set_mask(x, y, false);
                }
            }
        }
    }

    /// §3.2: the sectors, k = 0 to n² (one more than there are, quirk 2). Returns the town
    /// table (row + 16·column → building id).
    fn sectors(&mut self, o: &Options, n: i32) -> Result<[u8; 256], Stop> {
        let mut table = [0xffu8; 256];
        for k in 0..=n * n {
            let (col, row) = (k % n, k / n);
            let (cx, cy) = (col * SECTOR + 25, row * SECTOR + 25);
            let q = QUADRANTS[self.rng.random(12) as usize];
            for (slot, (t, chance)) in [(1, Chance::Towns), (3, Chance::Castles), (2, Chance::Villages), (2, Chance::Villages)].into_iter().enumerate() {
                let (sx, sy) = (cx - 13 + (q[slot] % 2) * 25, cy - 13 + (q[slot] / 2) * 25);
                // The picture is drawn before the roll, even when the roll fails (quirk 4).
                let serial = self.inp.pictures.draw(self.rng, t);
                if self.rng.random(100) >= o.chance(chance) || !self.spiral(sx, sy, RADIUS, serial) {
                    continue;
                }
                match t {
                    1 => {
                        // The table is range-checked: the extra visit's row n is outside it
                        // when n = 16.
                        if row > 15 || col > 15 {
                            return Err(Stop::TownTable);
                        }
                        table[(row + 16 * col) as usize] = self.s.buildings.len() as u8;
                        self.counters.towns += 1;
                    }
                    3 => self.counters.castles += 1,
                    _ => self.counters.villages += 1,
                }
            }
        }
        Ok(table)
    }

    // --------------------------------------------------------------------------------------
    // Costs and roads
    // --------------------------------------------------------------------------------------

    /// The cost map (0x56f5d0), cell by cell row by row: 1000 plus the terrain's cost, an
    /// object's cost added over its square (up and left of it), a forest object's on its own
    /// cell, a bridge's footprint set to the road's (stone) or grass plain's (wooden) cost
    /// plus 1000. The writes are not bounds-checked: a square reaching left of the map lands
    /// at the end of the row above (kept), one above the map before the cost image. The two
    /// cells just before it are the image's own height and width, which a square at the top
    /// left (or a top-right one reaching above row 0) overwrites in the original, after which
    /// its reads and writes go by the broken size; Razdor skips those writes too. No
    /// object's square leaves the map (the hills brush keeps it on the map and a load moves
    /// it onto it), so only a bridge's code left on the border by an earlier map can reach
    /// past it. Then 1000 is taken off, negative values made 0 and the border set to 0.
    fn build_costs(&mut self) {
        let (w, h) = (self.w, self.h);
        let n = self.cost.len() as i64;
        let mut v = vec![0u16; self.cost.len()];
        let read = |v: &[u16], x: i32, y: i32| if x >= 0 && y >= 0 && x < w && y < h { v[(y * w + x) as usize] } else { 0 };
        let write = |v: &mut [u16], x: i32, y: i32, val: u16| {
            let li = y as i64 * w as i64 + x as i64;
            if (0..n).contains(&li) {
                v[li as usize] = val;
            }
        };
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                v[i] = (terrain_cost(self.s.terrain[i]) + 1000) as u16;
                let [obj, tree] = self.objects.at(x as i64, y as i64);
                if let Some(o) = obj {
                    let k = (o.sprite / 10) as i32;
                    let add = OBJECT_COST.get((o.class as usize).wrapping_sub(1)).copied().unwrap_or(0);
                    for dx in (1 - k)..=0 {
                        for dy in (1 - k)..=0 {
                            let cur = read(&v, x + dx, y + dy);
                            write(&mut v, x + dx, y + dy, cur.wrapping_add(add as u16));
                        }
                    }
                }
                if let Some(t) = tree {
                    let add = FOREST_COST.get(t.class as usize).copied().unwrap_or(0);
                    v[i] = v[i].wrapping_add(add as u16);
                }
                if let Some((t @ (13 | 14), variant)) = self.codes[i] {
                    let (sx, sy) = self.inp.pictures.size(t, variant).unwrap_or((0, 0));
                    let val = (terrain_cost(if t == 13 { ROAD } else { GRASS_PLAIN }) + 1000) as u16;
                    for dx in (1 - sx as i32)..=0 {
                        for dy in (1 - sy as i32)..=0 {
                            write(&mut v, x + dx, y + dy, val);
                        }
                    }
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
                self.cost[i] = if border { 0 } else { (v[i] as i32 - 1000).max(0) as u16 };
            }
        }
    }

    fn cost_at(&self, x: i32, y: i32) -> u16 {
        if self.inside(x, y) {
            self.cost[self.idx(x, y)]
        } else {
            0
        }
    }

    /// The linear cells an unchecked write over the `sx` × `sy` square ending at `(x, y)`
    /// reaches (a square past the left edge wraps to the row above).
    fn footprint_targets(&self, x: i32, y: i32, sx: i32, sy: i32) -> Vec<usize> {
        let n = self.cost.len() as i64;
        let mut out = Vec::new();
        for dx in (1 - sx)..=0 {
            for dy in (1 - sy)..=0 {
                let li = (y + dy) as i64 * self.w as i64 + (x + dx) as i64;
                if (0..n).contains(&li) {
                    out.push(li as usize);
                }
            }
        }
        out
    }

    /// The cost map after the changes since the last build (0x56f5d0 builds it whole after
    /// every attempt; only paved cells and new bridges change it, so only their cells are
    /// computed again, each as the whole build would leave it).
    fn rebuild_costs(&mut self) {
        let dirty = std::mem::take(&mut self.dirty);
        for i in dirty {
            self.cost[i] = self.cost_cell(i);
        }
        #[cfg(test)]
        if VERIFY_COSTS.load(std::sync::atomic::Ordering::Relaxed) {
            let fast = self.cost.clone();
            self.build_costs();
            assert!(fast == self.cost, "the incremental cost map differs from a full build");
        }
    }

    /// One cell of [`World::build_costs`]: the events of the row-by-row scan that write its
    /// linear index, in scan order — its own base, then the squares of objects and bridges
    /// at or after it that cover it, or that cover the position left of the next row's start
    /// that aliases it.
    fn cost_cell(&self, i: usize) -> u16 {
        let (w, h) = (self.w, self.h);
        let (cx, cy) = ((i as i32) % w, (i as i32) / w);
        let reach = self.reach;
        // Every write lands at or before its source in the scan, and a square aliases at
        // most one row up: the map is 50 cells wide or more here (a narrower one stops before
        // the cost map), wider than any square.
        let mut sources: Vec<(i32, i32)> = Vec::new();
        for y in cy..(cy + reach).min(h) {
            for x in cx..(cx + reach).min(w) {
                sources.push((y, x));
            }
        }
        if cx >= w - reach {
            for y in (cy + 1)..(cy + 1 + reach).min(h) {
                for x in 0..reach.min(w) {
                    sources.push((y, x));
                }
            }
        }
        sources.sort_unstable();
        sources.dedup();
        // The positions that write index i: the cell itself, and its alias in the next row.
        let at = |px: i32, py: i32| (px, py) == (cx, cy) || (px, py) == (cx - w, cy + 1);
        let mut v: u16 = 0;
        for (sy, sx) in sources {
            let j = (sy * w + sx) as usize;
            if (sx, sy) == (cx, cy) {
                v = (terrain_cost(self.s.terrain[j]) + 1000) as u16;
            }
            let [obj, tree] = self.objects.at(sx as i64, sy as i64);
            if let Some(o) = obj {
                let k = (o.sprite / 10) as i32;
                let add = OBJECT_COST.get((o.class as usize).wrapping_sub(1)).copied().unwrap_or(0) as u16;
                for dx in (1 - k)..=0 {
                    for dy in (1 - k)..=0 {
                        let (px, py) = (sx + dx, sy + dy);
                        if at(px, py) {
                            // An unchecked write after a checked read: off the map it reads 0.
                            let cur = if (px, py) == (cx, cy) { v } else { 0 };
                            v = cur.wrapping_add(add);
                        }
                    }
                }
            }
            if (sx, sy) == (cx, cy) {
                if let Some(t) = tree {
                    v = v.wrapping_add(FOREST_COST.get(t.class as usize).copied().unwrap_or(0) as u16);
                }
            }
            if let Some((t @ (13 | 14), variant)) = self.codes[j] {
                let (bx, by) = self.inp.pictures.size(t, variant).unwrap_or((0, 0));
                let val = (terrain_cost(if t == 13 { ROAD } else { GRASS_PLAIN }) + 1000) as u16;
                for dx in (1 - bx as i32)..=0 {
                    for dy in (1 - by as i32)..=0 {
                        if at(sx + dx, sy + dy) {
                            v = val;
                        }
                    }
                }
            }
        }
        if cx == 0 || cy == 0 || cx == w - 1 || cy == h - 1 {
            0
        } else {
            (v as i32 - 1000).max(0) as u16
        }
    }

    /// The centre of building `id` (1-based): its anchor minus half its footprint.
    fn centre(&self, id: usize) -> Option<(i32, i32)> {
        let b = self.s.buildings.get(id.checked_sub(1)?)?;
        Some((b.x as i32 - (b.size_x / 2) as i32, b.y as i32 - (b.size_y / 2) as i32))
    }

    /// §3.6 item 1: per town of the table, column by column, a road from the next town down
    /// its column, then from the next town right in its row. An entry is the building count
    /// after its placement, so a refused placement names the building before it, and record
    /// 0 when there was none, which the original reads with a range error.
    fn town_roads(&mut self, table: &[u8; 256], n: i32) -> Result<(), Stop> {
        let at = |row: i32, col: i32| table[(row + 16 * col) as usize];
        let centre = |w: &World, id: u8| if id == 0 { Err(Stop::TownRecord) } else { Ok(w.centre(id as usize)) };
        for col in 0..n {
            for row in 0..n {
                let here = at(row, col);
                if here == 0xff {
                    continue;
                }
                let goal = centre(self, here)?;
                let down = (row + 1..n).map(|r| at(r, col)).find(|&t| t != 0xff);
                if let Some(t) = down {
                    if let (Some(start), Some(goal)) = (centre(self, t)?, goal) {
                        self.road(start, goal);
                    }
                }
                let right = (col + 1..n).map(|c| at(row, c)).find(|&t| t != 0xff);
                if let Some(t) = right {
                    if let (Some(start), Some(goal)) = (centre(self, t)?, goal) {
                        self.road(start, goal);
                    }
                }
            }
        }
        Ok(())
    }

    /// §3.6 item 2: every building of the count taken now that is not a town gets a road from
    /// its nearest building and from the next nearest (anchor distances, the first of equals;
    /// bridges built since count).
    fn nearest_roads(&mut self) {
        let count = self.s.buildings.len();
        for i in 1..=count {
            if self.s.buildings[i - 1].kind <= 1 {
                continue;
            }
            let mut prev = 0;
            for _ in 0..2 {
                let me = &self.s.buildings[i - 1];
                let (ix, iy) = (me.x as i32, me.y as i32);
                let mut best = (0, 10_000);
                for (k, b) in self.s.buildings.iter().enumerate() {
                    let j = k + 1;
                    if j == i || j == prev {
                        continue;
                    }
                    let d = Grid::Square8.octile((b.x as i32, b.y as i32), (ix, iy));
                    if d < best.1 {
                        best = (j, d);
                    }
                }
                if best.0 == 0 {
                    continue;
                }
                if let (Some(start), Some(goal)) = (self.centre(best.0), self.centre(i)) {
                    self.road(start, goal);
                }
                prev = best.0;
            }
        }
    }

    /// One road (0x56fa60): up to six attempts of a flood from the goal, the path read back
    /// from the start; a dry path is paved, a wet one gets a bridge and the road is tried
    /// again (quirk 6). The cost map is built again after every attempt.
    pub fn road(&mut self, start: (i32, i32), goal: (i32, i32)) {
        for _ in 0..ATTEMPTS {
            let field = self.map.flood_maps(&self.cost, &self.ones, &[(goal, 0)], start);
            let mut path = vec![start];
            path.extend(self.map.descend(&field, start));
            let wet = (1..path.len()).find(|&i| self.cost_at(path[i].0, path[i].1) >= WATER);
            let dry = wet.and_then(|w| (w + 1..path.len()).find(|&i| self.cost_at(path[i].0, path[i].1) < WATER));
            let done = match wet {
                None => {
                    // Every path cell but the start that is not water becomes road and
                    // loses its tree, footprints of terrain 16 and the goal included.
                    for &(x, y) in &path[1..] {
                        if self.terrain(x, y) > 1 {
                            self.set_terrain(x, y, ROAD);
                            self.set_forest(x, y, None);
                            let i = self.idx(x, y);
                            self.dirty.push(i);
                        }
                    }
                    true
                }
                // Without land after the water the original's index stays 0: the start.
                Some(w) => !self.bridge(path[w], path[dry.unwrap_or(0)]),
            };
            self.rebuild_costs();
            if done {
                return;
            }
        }
    }

    /// A bridge over the water between `wet` (its first water cell) and `dry` (the first land
    /// cell after it) (§3.8). A crossing that is not mainly horizontal gets none and the road
    /// is given up (quirk 6). Returns whether a bridge was placed.
    fn bridge(&mut self, wet: (i32, i32), dry: (i32, i32)) -> bool {
        let (mx, my) = ((wet.0 + dry.0) >> 1, (wet.1 + dry.1) >> 1);
        if (wet.1 - dry.1).abs() >= (wet.0 - dry.0).abs() {
            return false;
        }
        // Axes d = 1 (NE–SW), 2 (E–W), 3 (SE–NW): from the midpoint both ways to the first
        // cell below water cost (blocked and off-map cells too); A east, B west.
        let probe = |dir: usize| {
            let mut k = 1;
            while self.cost_at(mx + DX[dir] * k, my + DY[dir] * k) >= WATER {
                k += 1;
            }
            (mx + DX[dir] * k, my + DY[dir] * k)
        };
        let ends: [((i32, i32), (i32, i32)); 4] = std::array::from_fn(|d| if d == 0 { ((0, 0), (0, 0)) } else { (probe(d), probe(d + 4)) });
        let span = |d: usize| ends[d].0 .0 - ends[d].1 .0;
        let d = if span(1) < span(2) {
            if span(1) < span(3) {
                1
            } else {
                3
            }
        } else if span(2) < span(3) {
            2
        } else {
            3
        };
        let (a, b) = ends[d];
        let base = self.inp.pictures.first(STONE_BRIDGE);
        let piece = |k: usize| base.map(|f| f + k);
        // Each piece is a placement of its own: it reseeds and draws its names.
        self.place(piece(7 - d), b.0, b.1);
        self.place(piece(6 + d), a.0, a.1);
        for k in 1..(a.0 - b.0) {
            self.place(piece(4 - d), b.0 + DX[d] * k, b.1 + DY[d] * k);
        }
        self.counters.bridges += 1;
        true
    }

    // --------------------------------------------------------------------------------------
    // Junctions and ruins
    // --------------------------------------------------------------------------------------

    /// §3.9: a building on every masked road cell with exactly three road neighbours, placed
    /// the interactive way (the flag is off, quirk 5): the type drawn and counted first, then
    /// the roll, then the picture (quirk 4), on the cell itself with no fit test. Reads and the
    /// mask box are range-checked: reaching column or row −1 stops the step.
    fn junctions(&mut self, o: &Options) -> Result<(), Stop> {
        for x in 0..self.w {
            for y in 0..self.h {
                if !self.mask(x, y) || self.terrain(x, y) != ROAD {
                    continue;
                }
                if x == 0 || y == 0 {
                    return Err(Stop::JunctionAtEdge { x, y });
                }
                let roads = (0..8).filter(|&d| self.terrain(x + DX[d], y + DY[d]) == ROAD).count();
                if roads != 3 {
                    continue;
                }
                let t = self.rng.random(4) as u8 + 4;
                match t {
                    4 => self.counters.forts += 1,
                    5 => self.counters.taverns += 1,
                    6 => self.counters.markets += 1,
                    _ => self.counters.churches += 1,
                }
                if self.rng.random(100) >= o.chance(Chance::Other) {
                    continue;
                }
                let serial = self.inp.pictures.draw(self.rng, t);
                self.place(serial, x, y);
                let Some(p) = self.inp.pictures.get(serial) else { continue };
                let (sx, sy) = (p.size.0 as i32, p.size.1 as i32);
                if x - sx - 1 < 0 || y - sy - 1 < 0 {
                    return Err(Stop::JunctionAtEdge { x, y });
                }
                self.clear_box(x, y, sx, sy);
            }
        }
        Ok(())
    }

    /// §3.10 items 2–4: the ruins' mask: grass lowland, marsh, forest, low hills (marks −1 to
    /// −4) on every cell; cleared around every building, ±(2 × size + 3) from its centre on
    /// interior cells; five erosion passes.
    fn wild_mask(&mut self) {
        for x in 0..self.w {
            for y in 0..self.h {
                let on = matches!(self.terrain(x, y), 5 | 8) || self.forest(x, y).is_some() || (-4..=-1).contains(&self.mark(x, y));
                self.set_mask(x, y, on);
            }
        }
        let boxes: Vec<(i32, i32, i32, i32)> = self.s.buildings.iter().map(|b| (b.x as i32 - (b.size_x / 2) as i32, b.y as i32 - (b.size_y / 2) as i32, 2 * b.size_x as i32 + 3, 2 * b.size_y as i32 + 3)).collect();
        for (cx, cy, rx, ry) in boxes {
            for x in (cx - rx)..=(cx + rx) {
                for y in (cy - ry)..=(cy + ry) {
                    if x > 0 && y > 0 && x < self.w - 1 && y < self.h - 1 {
                        self.set_mask(x, y, false);
                    }
                }
            }
        }
        for _ in 0..5 {
            self.erode();
        }
    }

    /// §3.10 item 5: per sector (n² + 1 again) a ruin picture, a roll, and a search from the
    /// sector centre itself.
    fn ruins(&mut self, o: &Options, n: i32) {
        for k in 0..=n * n {
            let (cx, cy) = ((k % n) * SECTOR + 25, (k / n) * SECTOR + 25);
            let serial = self.inp.pictures.draw(self.rng, 12);
            if self.rng.random(100) < o.chance(Chance::Ruins) && self.spiral(cx, cy, RADIUS, serial) {
                self.counters.ruins += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{palette, units};
    use super::super::{Options, Pictures, Units};
    use super::*;
    use crate::editor::naming::{building_names, tests::pools};
    use crate::editor::NewMap;

    struct Kit {
        pictures: Pictures,
        units: Units,
        names: crate::editor::naming::NamePools,
    }

    impl Kit {
        fn new() -> Kit {
            Kit { pictures: Pictures::from_palette(&palette()), units: units(), names: pools() }
        }

        fn inputs(&self) -> Inputs<'_> {
            Inputs { pictures: &self.pictures, names: Some(&self.names), units: &self.units, spell_prices: &[], brush: 1, clock: 77 }
        }
    }

    fn map(w: u32, h: u32, fill: u8) -> Scenario {
        crate::editor::defaults::new_scenario(NewMap { width: w, height: h, fill })
    }

    /// A world on `s` with every interior cell masked.
    fn world<'a>(s: &'a mut Scenario, cells: &'a mut CellLayer, rng: &'a mut Rng, inp: &'a Inputs<'a>) -> World<'a> {
        World::new(s, cells, rng, inp)
    }

    fn serial(k: &Kit, t: u8, v: usize) -> Option<usize> {
        k.pictures.first(t).map(|f| f + v)
    }

    #[test]
    fn the_spiral_grows_squares_from_the_start_toward_plus_x_and_y() {
        let k = Kit::new();
        let inp = k.inputs();
        let one = serial(&k, 4, 2); // a 1×1 fort picture
        let (mut s, mut rng) = (map(30, 30, 6), Rng::new(1));
        let mut cells = CellLayer::load(&s);
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        // Only the start and a cell further along: the start is the 4th test, so it wins.
        w.set_mask(10, 10, true);
        w.set_mask(12, 12, true);
        assert!(w.spiral(10, 10, 30, one));
        assert_eq!((w.s.buildings[0].x, w.s.buildings[0].y), (10, 10));
        // The box x − sx − 1 … x + 2 lost its mask, (12, 12) included.
        assert!(!w.mask(12, 12));
        // Cells up or left of the start are never reached.
        w.set_mask(15, 4, true);
        w.set_mask(4, 15, true);
        assert!(!w.spiral(15, 15, 30, one) && !w.spiral(5, 5, 30, one));
        assert_eq!(w.s.buildings.len(), 1);
        // (x + 1, y) is the first test, (x + 1, y + 1) the second.
        w.set_mask(21, 20, true);
        w.set_mask(21, 21, true);
        assert!(w.spiral(20, 20, 30, one));
        assert_eq!((w.s.buildings.last().unwrap().x, w.s.buildings.last().unwrap().y), (21, 20));
        // Radius 2: k runs 4…7, one square of side 1 only.
        w.set_mask(26, 24, true);
        assert!(!w.spiral(24, 24, 2, one));
        assert!(w.spiral(24, 24, 3, one), "the second square reaches (x + 2, y)");
    }

    #[test]
    fn a_fit_needs_the_whole_footprint_masked_and_on_the_map() {
        let k = Kit::new();
        let inp = k.inputs();
        let (mut s, mut rng) = (map(12, 12, 6), Rng::new(1));
        let mut cells = CellLayer::load(&s);
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        for x in 0..12 {
            for y in 0..12 {
                w.set_mask(x, y, true);
            }
        }
        assert!(w.fits(5, 5, 3, 3) && w.fits(2, 2, 3, 3) && !w.fits(1, 5, 3, 3) && !w.fits(12, 5, 1, 1));
        w.set_mask(4, 4, false);
        assert!(!w.fits(5, 5, 3, 3) && w.fits(6, 6, 2, 2));
        // A search that fits but whose placement is refused still reports success, and
        // clears the box.
        let mut big = inp;
        big.brush = 9;
        let mut w = World { inp: &big, ..w };
        assert!(w.spiral(2, 2, 30, serial(&k, 4, 2)));
        assert!(w.s.buildings.is_empty(), "the brush square reaches out of the map");
    }

    #[test]
    fn every_placement_reseeds_from_its_spot_and_picture() {
        let k = Kit::new();
        let inp = k.inputs();
        let (mut s, mut rng) = (map(40, 40, 6), Rng::new(123_456));
        let mut cells = CellLayer::load(&s);
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.generating = true;
        // Castle picture 2 at (20, 30): seed 11·20 + 7·30 + 3·2 + 3, then its name draws.
        assert!(w.place(serial(&k, 3, 2), 20, 30));
        let mut e = Rng::new(0);
        let names = building_names(&k.names, &mut e, 20, 30, 3, 2);
        assert_eq!(w.rng.state(), e.state());
        assert_eq!((w.s.buildings[0].name.clone(), w.s.buildings[0].owner_name.clone()), names);
        // Generating: the footprint stands on terrain 16 and keeps its trees; owner 0.
        let b = &w.s.buildings[0];
        assert_eq!((b.owner_army, b.size_x, b.faction), (0, 5, 3));
        assert_eq!((w.terrain(16, 26), w.terrain(20, 30), w.terrain(15, 30), w.mark(18, 28)), (16, 16, 6, crate::editor::grid::MARK_BUILDING));
        // The anchor cell's code refuses a second building there.
        assert!(!w.place(serial(&k, 4, 0), 20, 30));
        // Bridges (type 13 and up) keep their terrain even while generating.
        assert!(w.place(serial(&k, 13, 1), 5, 5));
        assert_eq!(w.terrain(5, 5), 6);
    }

    #[test]
    fn the_cost_map_follows_terrain_objects_forest_and_bridges() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(12, 10, 6);
        for (x, code) in [(1, 0), (2, 1), (3, 2), (4, 4), (5, 5), (6, 8), (7, 15), (8, 16)] {
            s.terrain[12 + x] = code;
        }
        // A 2-cell hill ending at (3, 4) (class 1, +2 each), a tree on (5, 4) (+8), a mountain
        // on (7, 4) of size 1 (−32), and a hill of size 3 at the left edge (0, 7) that
        // reaches past the left of the map.
        s.objects = vec![
            MapObject { x: 3, y: 4, class: 1, sprite: 20 },
            MapObject { x: 5, y: 4, class: 9, sprite: 1 },
            MapObject { x: 7, y: 4, class: 5, sprite: 10 },
            MapObject { x: 0, y: 7, class: 1, sprite: 30 },
        ];
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(1));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.build_costs();
        let c = |w: &World, x, y| w.cost[(y * 12 + x) as usize];
        assert_eq!((1..=8).map(|x| c(&w, x, 1)).collect::<Vec<_>>(), [100, 150, 0, 1, 8, 15, 0, 1]);
        assert_eq!((c(&w, 2, 3), c(&w, 3, 4), c(&w, 4, 4), c(&w, 5, 4), c(&w, 7, 4)), (7, 7, 5, 13, 0));
        // The border is 0; water is 100 and up, shallows included.
        assert!((0..12).all(|x| c(&w, x, 0) == 0 && c(&w, x, 9) == 0) && c(&w, 0, 5) == 0);
        // The edge hill's square past the left edge lands at the row above's end: (10, 6)
        // and (11, 6), (10, 5) and (11, 5) are written as 0 + 2, blocked after the 1000 is
        // taken off.
        assert_eq!((c(&w, 10, 6), c(&w, 10, 5), c(&w, 9, 6), c(&w, 1, 7)), (0, 0, 5, 5));
        // A stone bridge's footprint costs a road, a wooden one grass plain, from their anchor.
        w.codes[3 * 12 + 9] = Some((13, 0));
        w.codes[12 + 3] = Some((14, 0));
        w.build_costs();
        assert_eq!((c(&w, 7, 1), c(&w, 9, 3), c(&w, 3, 1)), (1, 1, 5));
        // Each cell again as the whole build leaves it.
        for i in 0..w.cost.len() {
            assert_eq!(w.cost_cell(i), w.cost[i], "cell {i}");
        }
    }

    #[test]
    fn a_dry_road_is_paved_and_a_wet_one_bridged() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(30, 20, 6);
        for y in 0..20 {
            for x in 14..=16 {
                s.terrain[y * 30 + x] = 1;
            }
        }
        // A corridor between deep sea on rows 9 and 11 for the dry road.
        for x in 4..14 {
            s.terrain[9 * 30 + x] = 2;
            s.terrain[11 * 30 + x] = 2;
        }
        s.objects = vec![MapObject { x: 7, y: 10, class: 9, sprite: 0 }];
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(1));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.generating = true;
        w.build_costs();
        w.road((5, 10), (12, 10));
        // Dry: every cell but the start is road and lost its tree.
        assert_eq!(w.terrain(5, 10), 6);
        assert!((6..=12).all(|x| w.terrain(x, 10) == ROAD) && w.forest(7, 10).is_none());
        // Wet: the crossing between the first water (14, 10) and the land after it (17, 10)
        // has three axes of span 4; the tie goes to SE–NW (d = 3): the west end, piece 4, on
        // (13, 8), the east end, piece 9, on (17, 12), piece 1 on (14, 9), (15, 10), (16, 11).
        w.road((12, 10), (25, 10));
        let pieces: Vec<(u16, u16, u8, u8)> = w.s.buildings.iter().map(|b| (b.x, b.y, b.picture_type, b.picture_variant)).collect();
        assert_eq!(pieces, [(13, 8, 13, 4), (17, 12, 13, 9), (14, 9, 13, 1), (15, 10, 13, 1), (16, 11, 13, 1)]);
        assert_eq!(w.counters.bridges, 1);
        // The pieces cost a road; the next attempt crossed them and paved the land, not the
        // water.
        assert!([(13, 8), (14, 9), (15, 10), (16, 11), (17, 12)].iter().all(|&(x, y)| w.cost_at(x, y) == 1));
        assert!(w.terrain(15, 10) == 1 && w.terrain(25, 10) == ROAD && w.terrain(18, 10) + w.terrain(18, 11) + w.terrain(18, 12) < 18);
    }

    #[test]
    fn a_north_south_crossing_is_given_up_and_six_bridges_end_a_road() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(20, 30, 6);
        for x in 0..20 {
            s.terrain[15 * 20 + x] = 1;
        }
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(1));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.build_costs();
        w.road((10, 5), (10, 25));
        assert!(w.s.buildings.is_empty() && w.counters.bridges == 0);
        assert!(w.s.terrain.iter().all(|&t| t != ROAD), "nothing is paved");
        drop(w);
        // Seven rivers across a west–east road: six attempts, six bridges, no road.
        let mut s = map(60, 20, 6);
        for y in 0..20 {
            for x in [8, 14, 20, 26, 32, 38, 44] {
                s.terrain[y * 60 + x] = 1;
            }
        }
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(1));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.build_costs();
        w.road((3, 10), (55, 10));
        assert_eq!(w.counters.bridges, 6);
        assert!(w.s.terrain.iter().all(|&t| t != ROAD));
    }

    #[test]
    fn road_cleanup_takes_the_terrain_below_or_right_and_a_tree() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(10, 10, 6);
        let at = |x: usize, y: usize| y * 10 + x;
        // A road cell over a road cell over dry plain; one with grass lowland to the right.
        s.terrain[at(3, 3)] = ROAD;
        s.terrain[at(3, 4)] = ROAD;
        s.terrain[at(3, 5)] = 7;
        s.terrain[at(6, 3)] = ROAD;
        s.terrain[at(6, 4)] = ROAD;
        s.terrain[at(7, 3)] = 5;
        // Five wooded neighbours around (3, 3).
        s.objects = [(2, 2), (3, 2), (4, 2), (2, 3), (4, 3)].iter().map(|&(x, y)| MapObject { x, y, class: 9, sprite: x as u8 }).collect();
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(9));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        w.clean_roads();
        // (3, 3) took (3, 4)'s road? No: it is road, so the right, grass plain, wins.
        assert_eq!((w.terrain(3, 3), w.terrain(3, 4), w.terrain(6, 3), w.terrain(6, 4)), (6, 7, 5, 6));
        assert_eq!(w.mark(3, 4), 7, "a mark of 4 follows the terrain");
        // The tree came from a neighbour drawn by R(8) until one is wooded.
        let mut e = Rng::new(9);
        let tree = loop {
            let (dx, dy) = DIRECTIONS[e.random(8) as usize];
            if let Some(&(x, _)) = [(2, 2), (3, 2), (4, 2), (2, 3), (4, 3)].iter().find(|&&p| p == (3 + dx, 3 + dy)) {
                break x as u8;
            }
        };
        assert_eq!(w.forest(3, 3).map(|o| o.sprite), Some(tree));
        assert_eq!(w.rng.state(), e.state());
        // Plain cells free of objects are masked, the others not; the border is never written.
        assert!(w.mask(5, 5) && !w.mask(3, 2) && !w.mask(7, 3) && !w.mask(0, 5));
    }

    #[test]
    fn erosion_needs_three_neighbours_and_both_sides() {
        let k = Kit::new();
        let inp = k.inputs();
        let (mut s, mut rng) = (map(12, 12, 6), Rng::new(1));
        let mut cells = CellLayer::load(&s);
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        // A 3-wide band keeps its rows; a 1-wide line dies; a 2×2 block keeps its cells (each
        // has three masked neighbours), a 2×1 one does not.
        for x in 1..11 {
            for y in 2..=4 {
                w.set_mask(x, y, true);
            }
            w.set_mask(x, 7, true);
        }
        for (x, y) in [(3, 9), (4, 9), (3, 10), (4, 10), (8, 9), (8, 10)] {
            w.set_mask(x, y, true);
        }
        w.erode();
        assert!(w.mask(5, 3) && w.mask(5, 2) && !w.mask(5, 7));
        assert!(w.mask(3, 9) && w.mask(4, 10) && !w.mask(8, 9) && !w.mask(8, 10));
    }

    #[test]
    fn junctions_count_candidates_and_draw_the_picture_after_the_roll() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(20, 20, 6);
        // A T of roads: (10, 10) has road W, E and S: three road neighbours.
        for x in 6..=14 {
            s.terrain[10 * 20 + x] = ROAD;
        }
        for y in 11..=14 {
            s.terrain[y * 20 + 10] = ROAD;
        }
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(31));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        for x in 0..20 {
            for y in 0..20 {
                w.set_mask(x, y, true);
            }
        }
        // Chance 0 %: the candidates are still counted, nothing is placed.
        let mut o = Options::new(20);
        o.chances[Chance::Other as usize] = 5;
        assert_eq!(w.junctions(&o), Ok(()));
        let mut e = Rng::new(31);
        // Every road cell with exactly three road neighbours: (9, 10)… the T's cells.
        let cands: Vec<(i32, i32)> = (0..20).flat_map(|x| (0..20).map(move |y| (x, y))).filter(|&(x, y)| w.terrain(x, y) == ROAD && (0..8).filter(|&d| w.terrain(x + DX[d], y + DY[d]) == ROAD).count() == 3).collect();
        let mut counts = [0; 4];
        for _ in &cands {
            counts[e.random(4) as usize] += 1;
            e.random(100);
        }
        let c = w.counters;
        assert_eq!([c.forts, c.taverns, c.markets, c.churches], counts);
        assert!(w.s.buildings.is_empty() && w.rng.state() == e.state() && !cands.is_empty());
        // Chance 100 %: type, roll, then picture; placed on the cell with no fit test.
        o.chances[Chance::Other as usize] = 0;
        let mut e = Rng::new(5);
        *w.rng = Rng::new(5);
        w.counters = Counters::default();
        assert_eq!(w.junctions(&o), Ok(()));
        let (x, y) = cands[0];
        let t = e.random(4) as u8 + 4;
        e.random(100);
        let v = e.random(k.pictures.count(t));
        let b = &w.s.buildings[0];
        assert_eq!((b.x as i32, b.y as i32, b.picture_type, b.picture_variant as i32), (x, y, t, v));
        assert_eq!(w.terrain(x, y), ROAD, "the flag is off: no terrain 16");
    }

    #[test]
    fn a_junction_at_the_edge_stops_the_step() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(20, 20, 6);
        for x in 0..=8 {
            s.terrain[2 * 20 + x] = ROAD;
        }
        s.terrain[3 * 20 + 2] = ROAD;
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(1));
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        for x in 1..20 {
            for y in 1..20 {
                w.set_mask(x, y, true);
            }
        }
        let mut o = Options::new(20);
        o.chances[Chance::Other as usize] = 0;
        // (1, 2) has three road neighbours (W, E, SE); any picture's box reaches column −1.
        assert_eq!(w.junctions(&o), Err(Stop::JunctionAtEdge { x: 1, y: 2 }));
        assert_eq!(w.s.buildings.len(), 1, "the building is placed before the box");
        // A masked road cell in column 0 stops at its neighbour read.
        w.set_mask(0, 2, true);
        assert_eq!(w.junctions(&o), Err(Stop::JunctionAtEdge { x: 0, y: 2 }));
    }

    #[test]
    fn a_map_narrower_than_a_sector_stops_after_the_clearing() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(40, 60, 6);
        s.buildings.push(crate::dt::dtm::Building { x: 5, y: 5, size_x: 2, size_y: 2, kind: 1, ..Default::default() });
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(3));
        let r = run(&mut s, &mut cells, &mut rng, &inp, &Options::new(40));
        assert_eq!(r.stop, Some(Stop::NarrowMap));
        assert!(s.buildings.is_empty());
        assert_eq!(rng.state(), 77, "Randomize ran, nothing drew after it");
    }

    #[test]
    fn a_footprint_past_the_left_edge_stops_the_clearing() {
        let k = Kit::new();
        let inp = k.inputs();
        let mut s = map(60, 60, 6);
        s.terrain[60 + 5] = ROAD;
        s.terrain[2 * 60] = ROAD;
        for (x, size) in [(6, 2), (1, 3)] {
            s.buildings.push(crate::dt::dtm::Building { x, y: 2, size_x: size, size_y: size, kind: 1, ..Default::default() });
        }
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(3));
        let r = run(&mut s, &mut cells, &mut rng, &inp, &Options::new(60));
        assert_eq!(r.stop, Some(Stop::FootprintOffMap { building: 2 }));
        // The first building's road was freed; the second stopped at its first cell, so the
        // road at (0, 2) stays; nothing else ran.
        assert_eq!((s.terrain[60 + 5], s.terrain[2 * 60], s.buildings.len(), rng.state()), (6, ROAD, 2, 3));
    }

    #[test]
    fn the_step_builds_the_marks_again_before_and_after() {
        let k = Kit::new();
        let inp = k.inputs();
        // Deep sea with a grass island whose marks are stale (a tree paved away earlier, say).
        let mut s = map(50, 50, 2);
        for y in 15..35 {
            for x in 15..35 {
                s.terrain[y * 50 + x] = 6;
            }
        }
        let mut cells = CellLayer::load(&s);
        for y in 15..35 {
            for x in 15..35 {
                cells.set_mark(x, y, -9);
            }
        }
        let mut o = Options::new(50);
        o.chances[Chance::Towns as usize] = 0;
        let r = run(&mut s, &mut cells, &mut Rng::new(3), &inp, &o);
        assert_eq!((r.stop, r.counters.towns), (None, 1));
        let fresh = CellLayer::load(&s);
        assert!((0..50).all(|x| (0..50).all(|y| cells.mark(x, y) == fresh.mark(x, y))));
    }

    #[test]
    fn a_first_town_refused_after_its_search_stops_the_road_pass() {
        let k = Kit::new();
        let mut inp = k.inputs();
        // A brush wider than any town's distance from the edge refuses every placement:
        // the town table's entry is the building count, 0.
        inp.brush = 50;
        let mut s = map(50, 50, 6);
        let mut o = Options::new(50);
        o.chances[Chance::Towns as usize] = 0;
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(3));
        let r = run(&mut s, &mut cells, &mut rng, &inp, &o);
        assert_eq!(r.stop, Some(Stop::TownRecord));
        assert_eq!((s.buildings.len(), r.counters.towns), (0, 1));
    }

    #[test]
    fn the_extra_visit_on_an_800_wide_map_hits_the_town_table() {
        let k = Kit::new();
        let inp = k.inputs();
        // Deep sea, with grass only below row 830: only the extra visit (row 16) reaches it.
        let mut s = map(800, 900, 2);
        for y in 830..880 {
            for x in 1..60 {
                s.terrain[y * 800 + x] = 6;
            }
        }
        let mut o = Options::new(800);
        o.chances[Chance::Towns as usize] = 0;
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(3));
        let r = run(&mut s, &mut cells, &mut rng, &inp, &o);
        assert_eq!(r.stop, Some(Stop::TownTable));
        assert!(s.terrain.iter().all(|&t| t != TEMPORARY_TERRAIN), "terrain 16 is cleared on a stop");
        assert_eq!(s.buildings.len(), 1);
    }

    #[test]
    fn sectors_draw_the_picture_before_the_roll_and_visit_one_more() {
        let k = Kit::new();
        let inp = k.inputs();
        // A 100 map of deep sea: nothing fits, every visit only draws.
        let mut s = map(100, 100, 2);
        let (mut cells, mut rng) = (CellLayer::load(&s), Rng::new(3));
        let o = Options::new(100);
        let mut w = world(&mut s, &mut cells, &mut rng, &inp);
        *w.rng = Rng::new(40);
        w.sectors(&o, 2).unwrap();
        let mut e = Rng::new(40);
        for _ in 0..5 {
            e.random(12);
            for t in [1, 3, 2, 2] {
                e.random(k.pictures.count(t));
                e.random(100);
            }
        }
        assert_eq!(w.rng.state(), e.state(), "n² + 1 = 5 visits");
    }
}
