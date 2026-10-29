//! Cell grid of the world map and pathfinding.
//!
//! The original (`docs/reference/original-mechanics/world.md` §1) uses plain squares of 32×22
//! px with 8 neighbours: [`Grid::Square8`]. Its planner weighs an orthogonal step 2 and a
//! diagonal one 3 (×1.5, vertical and horizontal alike); it floods from the target, so each
//! step is priced by the cell it leaves, and stops at the first value reaching the walker
//! ([`TileMap::flood_route`]); walking charges the cell left, `cost × speed` minutes (×1.5
//! diagonally). The AI armies' own searches ([`TileMap::path_by`]) are Razdor's. The
//! built-in demo keeps its hex layout ([`Grid::HexOddR`], odd rows shifted half a cell right,
//! every step weight 2), as `data/kingdom.txt` was drawn for it.
//!
//! Tile `(col, row)`; world positions are in units where one column is 1 wide. Each cell has
//! a surface (the original's terrain code, `dt::dtm::Surface`) and costs in the original's
//! units on foot (its LAND map) and by ship (its SHIP map), from the surface and the map
//! objects on it ([`TileMap::from_codes`]).

use crate::local;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub use crate::dt::dtm::Surface;

pub type Tile = (i32, i32);

/// Vertical distance between hex rows, in world units.
pub const ROW_HEIGHT: f32 = 0.866_025_4; // √3 / 2
/// Vertical distance between rows of the original's 32×22 px cells, in world units.
pub const SQUARE_ROW_HEIGHT: f32 = 22.0 / 32.0;

/// Centre of a hex (odd-r) in world units.
fn hex_center((c, r): Tile) -> (f32, f32) {
    (c as f32 + 0.5 * r.rem_euclid(2) as f32, r as f32 * ROW_HEIGHT)
}

fn to_cube((c, r): Tile) -> (i32, i32, i32) {
    let x = c - (r - r.rem_euclid(2)) / 2;
    (x, -x - r, r)
}

/// Steps between two hexes.
pub fn hex_distance(a: Tile, b: Tile) -> i32 {
    let (a, b) = (to_cube(a), to_cube(b));
    (a.0 - b.0).abs().max((a.1 - b.1).abs()).max((a.2 - b.2).abs())
}

/// The six neighbours of a hex (in bounds or not).
pub fn hex_neighbours((c, r): Tile) -> [Tile; 6] {
    let o = r.rem_euclid(2); // odd rows are shifted right
    [(c - 1, r), (c + 1, r), (c - 1 + o, r - 1), (c + o, r - 1), (c - 1 + o, r + 1), (c + o, r + 1)]
}

/// Every hex within `radius` steps of `t` (in bounds or not), `t` included.
pub fn hex_disk(t: Tile, radius: i32) -> impl Iterator<Item = Tile> {
    let r = radius.max(0);
    (t.1 - r..=t.1 + r)
        .flat_map(move |y| (t.0 - r - 1..=t.0 + r + 1).map(move |x| (x, y)))
        .filter(move |&n| hex_distance(t, n) <= r)
}

/// Hex containing a world position.
fn hex_tile_at(p: (f32, f32)) -> Tile {
    let r = (p.1 / ROW_HEIGHT).round() as i32;
    let c = (p.0 - 0.5 * r.rem_euclid(2) as f32).round() as i32;
    // The rounded guess is right or off by one neighbour: pick the nearest centre.
    std::iter::once((c, r))
        .chain(hex_neighbours((c, r)))
        .min_by(|&a, &b| {
            let d = |t| {
                let (x, y) = hex_center(t);
                (x - p.0).powi(2) + (y - p.1).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap()
}

/// Topology and geometry of the cell grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    /// Pointy-top hexes, odd rows shifted half a cell right (the built-in demo).
    HexOddR,
    /// The original's rectangular cells (32×22 px), 8 neighbours.
    Square8,
}

impl Grid {
    /// Vertical distance between rows, in world units.
    pub fn row_height(self) -> f32 {
        match self {
            Grid::HexOddR => ROW_HEIGHT,
            Grid::Square8 => SQUARE_ROW_HEIGHT,
        }
    }

    /// Horizontal offset of a row, in world units (hex odd rows are shifted).
    pub fn row_offset(self, r: i32) -> f32 {
        match self {
            Grid::HexOddR => 0.5 * r.rem_euclid(2) as f32,
            Grid::Square8 => 0.0,
        }
    }

    /// Centre of a cell in world units.
    pub fn center(self, t: Tile) -> (f32, f32) {
        match self {
            Grid::HexOddR => hex_center(t),
            Grid::Square8 => (t.0 as f32, t.1 as f32 * SQUARE_ROW_HEIGHT),
        }
    }

    /// Cell containing a world position.
    pub fn tile_at(self, p: (f32, f32)) -> Tile {
        match self {
            Grid::HexOddR => hex_tile_at(p),
            Grid::Square8 => (p.0.round() as i32, (p.1 / SQUARE_ROW_HEIGHT).round() as i32),
        }
    }

    /// The neighbours of a cell (in bounds or not): 6 for hexes, 8 for squares.
    pub fn neighbours(self, (c, r): Tile) -> impl Iterator<Item = Tile> {
        let mut out = [(0, 0); 8];
        let n = match self {
            Grid::HexOddR => {
                out[..6].copy_from_slice(&hex_neighbours((c, r)));
                6
            }
            Grid::Square8 => {
                out = [(c - 1, r), (c + 1, r), (c, r - 1), (c, r + 1), (c - 1, r - 1), (c + 1, r - 1), (c - 1, r + 1), (c + 1, r + 1)];
                8
            }
        };
        out.into_iter().take(n)
    }

    /// Steps between two cells.
    pub fn distance(self, a: Tile, b: Tile) -> i32 {
        match self {
            Grid::HexOddR => hex_distance(a, b),
            Grid::Square8 => (a.0 - b.0).abs().max((a.1 - b.1).abs()),
        }
    }

    /// Every cell within `radius` steps of `t` (in bounds or not), `t` included.
    pub fn disk(self, t: Tile, radius: i32) -> Vec<Tile> {
        match self {
            Grid::HexOddR => hex_disk(t, radius).collect(),
            Grid::Square8 => {
                let r = radius.max(0);
                (t.1 - r..=t.1 + r).flat_map(|y| (t.0 - r..=t.0 + r).map(move |x| (x, y))).collect()
            }
        }
    }

    /// Length of the step between two neighbouring cells, in world units (drawing only).
    pub fn step_length(self, a: Tile, b: Tile) -> f32 {
        let (p, q) = (self.center(a), self.center(b));
        (p.0 - q.0).hypot(p.1 - q.1)
    }

    /// Planner weight of a step between neighbours (world.md §1): 2 orthogonal, 3 diagonal
    /// on the original's squares (a diagonal is ×1.5, vertical and horizontal alike); every
    /// hex step is 2.
    pub fn weight(self, a: Tile, b: Tile) -> u32 {
        match self {
            Grid::Square8 if a.0 != b.0 && a.1 != b.1 => DIAGONAL_WEIGHT,
            _ => ORTHOGONAL_WEIGHT,
        }
    }

    /// The original's distance (world.md §4): `max(|dx|,|dy|) + min(|dx|,|dy|)/2` cells on
    /// squares; hex steps on the demo's grid.
    pub fn octile(self, a: Tile, b: Tile) -> i32 {
        match self {
            Grid::HexOddR => hex_distance(a, b),
            Grid::Square8 => {
                let (dx, dy) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
                dx.max(dy) + dx.min(dy) / 2
            }
        }
    }

    /// Neighbour offsets in the original's direction order (world.md §1): on squares 0
    /// north-west, then clockwise (1 N, 2 NE, 3 E, 4 SE, 5 S, 6 SW, 7 W), even directions
    /// diagonal; on the demo's hexes the six of [`hex_neighbours`] (row `r` decides them).
    pub fn directions(self, r: i32) -> [Option<(i32, i32)>; 8] {
        match self {
            Grid::Square8 => DIRECTIONS.map(Some),
            Grid::HexOddR => {
                let mut out = [None; 8];
                for (o, (c, rr)) in out.iter_mut().zip(hex_neighbours((0, r))) {
                    *o = Some((c, rr - r));
                }
                out
            }
        }
    }

    /// How many directions [`Grid::directions`] holds.
    pub fn direction_count(self) -> usize {
        match self {
            Grid::Square8 => 8,
            Grid::HexOddR => 6,
        }
    }

    /// Planner weight of a step in direction `dir` (the original's table 0x4ecfd4: 3 on the
    /// even, diagonal directions, 2 on the others; every hex step 2).
    pub fn direction_weight(self, dir: usize) -> u32 {
        match self {
            Grid::Square8 if dir.is_multiple_of(2) => DIAGONAL_WEIGHT,
            _ => ORTHOGONAL_WEIGHT,
        }
    }

    /// Lower bound of the planner weights between two cells (for A*).
    fn weight_bound(self, a: Tile, b: Tile) -> u32 {
        match self {
            Grid::HexOddR => ORTHOGONAL_WEIGHT * hex_distance(a, b) as u32,
            Grid::Square8 => {
                let (dx, dy) = ((a.0 - b.0).unsigned_abs(), (a.1 - b.1).unsigned_abs());
                ORTHOGONAL_WEIGHT * dx.max(dy) + (DIAGONAL_WEIGHT - ORTHOGONAL_WEIGHT) * dx.min(dy)
            }
        }
    }
}

/// A planner flood's distance map (`TileMap::flood_field`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FloodField {
    /// Distance of every cell, `u16::MAX` where the flood did not go.
    pub dist: Vec<u16>,
    /// Seeds it started from.
    pub kept: usize,
    w: i32,
}

impl FloodField {
    /// Takes cell `t` out of the flood: the path never steps onto it (0x4a2d88).
    pub fn erase(&mut self, t: Tile) {
        if t.0 >= 0 && t.1 >= 0 && t.0 < self.w {
            if let Some(d) = self.dist.get_mut((t.1 * self.w + t.0) as usize) {
                *d = 0;
            }
        }
    }
}

/// A cell the planner's flood has not reached.
const UNREACHED: u16 = u16::MAX;

/// The planner flood's frontier in the original's order: a linked list whose nodes carry
/// order labels, so that two positions compare by label. An expanded node is replaced in
/// place by its new neighbours.
#[derive(Default)]
struct Frontier {
    cell: Vec<usize>,
    label: Vec<u64>,
    prev: Vec<usize>,
    next: Vec<usize>,
}

/// The planner flood's buffers, kept between floods: stamps of the flood that last wrote a
/// cell instead of clearing them every time.
#[derive(Default)]
struct FloodScratch {
    epoch: u32,
    seed: Vec<u32>,
    seen: Vec<u32>,
    todo: Vec<usize>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    order: Frontier,
    /// Frontier nodes by value, and the values in use.
    buckets: Vec<Vec<u32>>,
    used: Vec<u32>,
}

local! {
    static SCRATCH: std::cell::RefCell<FloodScratch> = std::cell::RefCell::new(FloodScratch::default());
}

impl FloodScratch {
    fn begin(&mut self, n: usize) {
        if self.epoch == u32::MAX || self.seed.len() != n {
            self.epoch = 0;
            for v in [&mut self.seed, &mut self.seen] {
                v.clear();
                v.resize(n, 0);
            }
        }
        self.epoch += 1;
        if self.buckets.len() < UNREACHED as usize + 1 {
            self.buckets.resize_with(UNREACHED as usize + 1, Vec::new);
        }
        if self.order.cell.is_empty() {
            self.order = Frontier::new();
        }
        self.order.clear();
    }


    fn end(&mut self) {
        for v in self.used.drain(..) {
            self.buckets[v as usize].clear();
        }
    }
}

/// The bucket of value `v`, noted in `used` when it starts filling.
fn bucket<'a>(buckets: &'a mut [Vec<u32>], used: &mut Vec<u32>, v: u32) -> &'a mut Vec<u32> {
    let b = &mut buckets[v as usize];
    if b.is_empty() {
        used.push(v);
    }
    b
}

/// The list's ends: two sentinel nodes.
const HEAD: usize = 0;
const TAIL: usize = 1;
/// Label spacing when the labels are dealt again.
const LABEL_STEP: u64 = 1 << 32;

impl Frontier {
    fn new() -> Frontier {
        Frontier { cell: vec![0, 0], label: vec![0, u64::MAX], prev: vec![HEAD, HEAD], next: vec![TAIL, TAIL] }
    }

    fn cell(&self, n: usize) -> usize {
        self.cell[n]
    }

    fn label(&self, n: usize) -> u64 {
        self.label[n]
    }

    fn node(&mut self, cell: usize) -> usize {
        self.cell.push(cell);
        self.label.push(0);
        self.prev.push(HEAD);
        self.next.push(TAIL);
        self.cell.len() - 1
    }

    fn push_back(&mut self, cell: usize) -> usize {
        let n = self.node(cell);
        let last = self.prev[TAIL];
        self.link(last, n, TAIL);
        if self.cramped(n) {
            self.relabel();
        }
        n
    }

    /// Node `n`'s label does not lie strictly between its neighbours'.
    fn cramped(&self, n: usize) -> bool {
        self.label[n] <= self.label[self.prev[n]] || self.label[n] >= self.label[self.next[n]]
    }

    fn link(&mut self, a: usize, n: usize, b: usize) {
        self.next[a] = n;
        self.prev[n] = a;
        self.next[n] = b;
        self.prev[b] = n;
        self.label[n] = self.label[a] / 2 + self.label[b] / 2;
    }

    fn remove(&mut self, n: usize) {
        let (a, b) = (self.prev[n], self.next[n]);
        self.next[a] = b;
        self.prev[b] = a;
    }

    /// Replaces node `n` by new nodes for `children`, in order. Returns the first new node
    /// (the others follow it).
    fn replace(&mut self, n: usize, children: &[(usize, u32)]) -> usize {
        let (a, b) = (self.prev[n], self.next[n]);
        self.remove(n);
        let first = self.cell.len();
        let mut at = a;
        for &(cell, _) in children {
            let m = self.node(cell);
            self.link(at, m, b);
            at = m;
        }
        if (first..self.cell.len()).any(|m| self.cramped(m)) {
            self.relabel();
        }
        first
    }

    /// Empties the list.
    fn clear(&mut self) {
        self.cell.truncate(2);
        self.label.truncate(2);
        self.prev.truncate(2);
        self.next.truncate(2);
        self.next[HEAD] = TAIL;
        self.prev[TAIL] = HEAD;
    }

    /// Deals the labels again, evenly, in list order.
    fn relabel(&mut self) {
        let mut n = self.next[HEAD];
        let mut k = 1u64;
        while n != TAIL {
            self.label[n] = k * LABEL_STEP;
            k += 1;
            n = self.next[n];
        }
    }
}

/// The original's eight directions (dx, dy), 0 north-west then clockwise (0x4ecf8c, 0x4ecfb0).
pub const DIRECTIONS: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];

/// Planner weight of an orthogonal step; a diagonal one is [`DIAGONAL_WEIGHT`].
pub const ORTHOGONAL_WEIGHT: u32 = 2;
pub const DIAGONAL_WEIGHT: u32 = 3;

/// The original's terrain value of a surface (world.md §1, cost units): shallows 2, coastal
/// water 1, road 3, stony soil 4, grass, dry plain, sand, clay and scorched land 5, snowy
/// ground 6, marsh 8; deep sea, lava, the impassable swamp and snowdrifts −32 (blocked).
pub fn surface_value(s: Surface) -> i32 {
    use Surface::*;
    match s {
        ShallowsFords => 2,
        CoastalWater => 1,
        Road => 3,
        StonySoil => 4,
        GrassLowland | GrassPlain | DryPlain | SandDunes | ClaySoil | ScorchedLand => 5,
        SnowyGround => 6,
        Marsh => 8,
        DeepSea | LavaFields | ImpassableSwamp | ImpassableSnowdrifts => BLOCKED,
    }
}

/// The value that blocks a cell whatever else is on it.
pub const BLOCKED: i32 = -32;
/// Cost units of a road, and of every building footprint.
pub const ROAD: u16 = 3;
/// The knight's speed: game minutes per cost unit of an orthogonal step (the hero's step
/// time is `cost × speed` minutes, ×1.5 diagonally; world.md §2).
pub const BASE_SPEED: u16 = 5;

/// Water (terrain codes 0–2): never walked; ships sail the shallows and coastal water.
pub fn is_water(s: Surface) -> bool {
    matches!(s, Surface::ShallowsFords | Surface::CoastalWater | Surface::DeepSea)
}

/// Minutes per orthogonal step on foot at the knight's speed 5, `None` for water and
/// blocking surfaces (road 15, grass 25, marsh 40 …).
pub fn surface_minutes(s: Surface) -> Option<u16> {
    let v = surface_value(s);
    (!is_water(s) && v > 0).then(|| v as u16 * BASE_SPEED)
}

/// What a map object does to the cells it covers (world.md §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectEffect {
    /// Hills: the cell's cost starts from this value instead of 0 (the terrain adds to it).
    Base(i32),
    /// Added to the cell's cost ([`BLOCKED`] blocks it).
    Add(i32),
}

/// Map object classes (`.DTm` objects, `Objects.ugs` section A categories).
pub mod object_class {
    pub const HILLS: u8 = 1;
    pub const GREEN_HILL: u8 = 2;
    pub const ROCKY_HILLS: u8 = 3;
    pub const YELLOW_HILL: u8 = 4;
    pub const MOUNTAINS: u8 = 5;
    pub const DARK_MOUNTAINS: u8 = 6;
    pub const ROCKS: u8 = 8;
    pub const TREES: u8 = 9;
    pub const DEAD_TREES: u8 = 10;
    pub const THICKET: u8 = 11;
}

/// Effect of an object class (world.md §1): hills 1–3 are a base of 2, class 4 of 3;
/// mountains 5–7 and rocks 8 block; trees 9 add 4, dead trees 10 add 6, thickets 11 block,
/// class 12 adds 4.
pub fn object_effect(class: u8) -> Option<ObjectEffect> {
    match class {
        1..=3 => Some(ObjectEffect::Base(2)),
        4 => Some(ObjectEffect::Base(3)),
        5..=8 | 11 => Some(ObjectEffect::Add(BLOCKED)),
        9 | 12 => Some(ObjectEffect::Add(4)),
        10 => Some(ObjectEffect::Add(6)),
        _ => None,
    }
}

/// A massif (classes 1–8: hills, mountains, rocks) covers a square of `sprite div 10` cells a
/// side whose bottom-right cell is the object's own; plants (9–12) cover their own cell.
pub fn object_side(class: u8, sprite: u8) -> i32 {
    match class {
        1..=8 => (sprite / 10) as i32,
        _ => 1,
    }
}

/// Cells an object covers (in bounds or not): rows `y−f+1..=y`, columns `x−f+1..=x`.
pub fn object_cells(o: &Decoration) -> impl Iterator<Item = Tile> {
    let f = object_side(o.class, o.sprite);
    let (x, y) = o.tile;
    (0..f).flat_map(move |j| (0..f).map(move |i| (x - i, y - j)))
}

/// A map object (hill, mountain, tree, rock) standing on a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub tile: Tile,
    pub class: u8,
    pub sprite: u8,
}

#[derive(Clone, Debug)]
pub struct TileMap {
    pub grid: Grid,
    pub w: i32,
    pub h: i32,
    surface: Vec<u8>,
    /// The original's LAND map: cost units of a step onto the cell on foot; 0 = impassable.
    land: Vec<u16>,
    /// Its SHIP map: cost units on water (shallows 2, coastal 1; building footprints road);
    /// 0 = not sailable.
    water: Vec<u16>,
    /// Sorted by (row, col).
    pub objects: Vec<Decoration>,
    /// `objects[row_start[r]..row_start[r + 1]]` stand in row `r`.
    row_start: Vec<usize>,
    /// Non-terrain characters of a text map (location markers) and where they are.
    pub markers: Vec<(char, Tile)>,
}

/// An empty 0×0 map: the placeholder a loaded save holds until the real map is rebuilt.
impl Default for TileMap {
    fn default() -> TileMap {
        TileMap::from_codes(Grid::Square8, 0, 0, &[], Vec::new())
    }
}

impl TileMap {
    /// A map from terrain codes (`w*h`, row by row) and objects, costed as the original
    /// builds its maps at load (world.md §1): hills set a base over their square, every cell
    /// adds its terrain value (below 0 blocks), plants add theirs on their cell and mountains,
    /// rocks and thickets block; water goes to the ship map, the rest to the foot map.
    /// Buildings are laid over it by `rules::world` ([`TileMap::pave`]).
    pub fn from_codes(grid: Grid, w: i32, h: i32, codes: &[u8], objects: Vec<Decoration>) -> TileMap {
        assert_eq!(codes.len(), (w * h) as usize, "terrain size");
        let mut objects = objects;
        objects.retain(|o| o.tile.0 >= 0 && o.tile.1 >= 0 && o.tile.0 < w && o.tile.1 < h);
        let file_order = objects.clone();
        objects.sort_by_key(|o| (o.tile.1, o.tile.0));
        // The original lays the hills while scanning its cell grid row by row: where hills
        // overlap, the one later in that scan wins (the sort is stable).
        let scan_order = objects.clone();
        let mut row_start = Vec::with_capacity(h as usize + 1);
        let mut i = 0;
        for r in 0..=h {
            while i < objects.len() && objects[i].tile.1 < r {
                i += 1;
            }
            row_start.push(i);
        }
        let n = codes.len();
        let mut m = TileMap { grid, w, h, surface: codes.to_vec(), land: vec![0; n], water: vec![0; n], objects, row_start, markers: Vec::new() };
        let index = |t: Tile| (t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h).then(|| (t.1 * w + t.0) as usize);
        // Hills set the base of their square (later in the row-by-row scan over earlier).
        let mut v = vec![0i32; n];
        for o in &scan_order {
            if let Some(ObjectEffect::Base(b)) = object_effect(o.class) {
                for i in object_cells(o).filter_map(index) {
                    v[i] = b;
                }
            }
        }
        // The terrain adds its value; below 0 blocks.
        for i in 0..n {
            v[i] = match Surface::from_code(codes[i]) {
                Some(s) => (v[i] + surface_value(s)).max(0),
                None => 0,
            };
        }
        // Plants add theirs on their own cell (if it is still open), massifs over their
        // square. The original then writes the cell back as land whatever it was: on water
        // its land cost was 0, so a plant, a mountain or a rock standing in the water blocks
        // ships too (world.md §1.1 steps 4–5).
        let water = |i: usize| Surface::from_code(codes[i]).is_some_and(is_water);
        for o in &file_order {
            let Some(ObjectEffect::Add(a)) = object_effect(o.class) else { continue };
            let cells: Vec<usize> = if o.class >= object_class::TREES { index(o.tile).into_iter().collect() } else { object_cells(o).filter_map(index).collect() };
            for i in cells {
                if water(i) {
                    v[i] = 0;
                } else if v[i] > 0 {
                    v[i] = (v[i] + a).max(0);
                }
            }
        }
        // Water on the ship map, the rest on the foot map.
        for i in 0..n {
            let c = v[i].min(u16::MAX as i32) as u16;
            if Surface::from_code(codes[i]).is_some_and(is_water) {
                m.water[i] = c;
            } else {
                m.land[i] = c;
            }
        }
        m
    }

    /// Parses rows of terrain characters (the built-in demo's `data/kingdom.txt`):
    /// `.` grass, `=` road, `T` forest (grass with a tree), `,` marsh, `~` deep water,
    /// `^` mountain (stony soil with a mountain). Other characters are location markers
    /// standing on road.
    pub fn parse(text: &str) -> TileMap {
        let rows: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let h = rows.len() as i32;
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        let mut codes = vec![Surface::GrassPlain as u8; (w * h) as usize];
        let mut objects = Vec::new();
        let mut markers = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let tile = (x as i32, y as i32);
                let (surface, object) = match c {
                    '.' => (Surface::GrassPlain, None),
                    '=' => (Surface::Road, None),
                    'T' => (Surface::GrassPlain, Some((object_class::TREES, (x * 7 + y * 3) as u8 % 9))),
                    ',' => (Surface::Marsh, None),
                    '~' => (Surface::DeepSea, None),
                    '^' => (Surface::StonySoil, Some((object_class::MOUNTAINS, 10 + (x + y) as u8 % 4))),
                    _ => {
                        markers.push((c, tile));
                        (Surface::Road, None)
                    }
                };
                codes[y * w as usize + x] = surface as u8;
                if let Some((class, sprite)) = object {
                    objects.push(Decoration { tile, class, sprite });
                }
            }
        }
        let mut m = TileMap::from_codes(Grid::HexOddR, w, h, &codes, objects);
        m.markers = markers;
        m
    }

    pub fn in_bounds(&self, (x, y): Tile) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    fn index(&self, t: Tile) -> Option<usize> {
        self.in_bounds(t).then(|| (t.1 * self.w + t.0) as usize)
    }

    fn tile_of(&self, i: usize) -> Tile {
        (i as i32 % self.w, i as i32 / self.w)
    }

    /// Terrain code of a cell (deep sea outside the map).
    pub fn surface_code(&self, t: Tile) -> u8 {
        self.index(t).map_or(Surface::DeepSea as u8, |i| self.surface[i])
    }

    /// Every cell's surface code, row by row.
    pub fn surface_codes(&self) -> &[u8] {
        &self.surface
    }

    pub fn surface(&self, t: Tile) -> Surface {
        Surface::from_code(self.surface_code(t)).unwrap_or(Surface::DeepSea)
    }

    /// Cost units of a step onto the cell on foot (the LAND map), `None` if impassable.
    pub fn cost(&self, t: Tile) -> Option<u16> {
        self.index(t).map(|i| self.land[i]).filter(|&c| c > 0)
    }

    /// Cost units of sailing onto the cell (the SHIP map), `None` if a ship cannot.
    pub fn water_cost(&self, t: Tile) -> Option<u16> {
        self.index(t).map(|i| self.water[i]).filter(|&c| c > 0)
    }

    /// Minutes of an orthogonal step on foot on the cell at the knight's speed, `None` if
    /// impassable.
    pub fn minutes(&self, t: Tile) -> Option<u16> {
        self.cost(t).map(|c| c * BASE_SPEED)
    }

    pub fn passable(&self, t: Tile) -> bool {
        self.cost(t).is_some()
    }

    /// Makes a cell impassable on foot and by ship.
    pub fn block(&mut self, t: Tile) {
        if let Some(i) = self.index(t) {
            self.land[i] = 0;
            self.water[i] = 0;
        }
    }

    /// A building's footprint: road on the foot and on the ship map (world.md §1 step 6).
    pub fn pave(&mut self, t: Tile) {
        if let Some(i) = self.index(t) {
            self.land[i] = ROAD;
            self.water[i] = ROAD;
        }
    }

    /// Makes a cell passable on foot at `cost` units.
    pub fn open(&mut self, t: Tile, cost: u16) {
        if let Some(i) = self.index(t) {
            self.land[i] = cost.max(1);
        }
    }

    /// Objects standing in rows `r0..r1`.
    pub fn objects_in_rows(&self, r0: i32, r1: i32) -> &[Decoration] {
        let r0 = r0.clamp(0, self.h) as usize;
        let r1 = r1.clamp(0, self.h) as usize;
        if r0 >= r1 {
            return &[];
        }
        &self.objects[self.row_start[r0]..self.row_start[r1]]
    }

    /// The passable cell nearest to `t` within `radius` steps (`t` itself if passable).
    pub fn nearest_passable(&self, t: Tile, radius: i32) -> Option<Tile> {
        let g = self.grid;
        (0..=radius).find_map(|r| g.disk(t, r).into_iter().filter(|&n| g.distance(t, n) == r).find(|&n| self.passable(n)))
    }

    /// Cheapest path on foot from `from` to `to`, excluding `from`. Empty if unreachable or
    /// equal.
    pub fn path(&self, from: Tile, to: Tile) -> Vec<Tile> {
        self.path_limited(from, to, usize::MAX)
    }

    /// [`TileMap::path`] giving up after expanding `max_nodes` cells (for AI armies).
    pub fn path_limited(&self, from: Tile, to: Tile, max_nodes: usize) -> Vec<Tile> {
        self.path_where(from, to, max_nodes, &|_| true)
    }

    /// [`TileMap::path_limited`] stepping only onto cells `allowed` accepts.
    pub fn path_where(&self, from: Tile, to: Tile, max_nodes: usize, allowed: &dyn Fn(Tile) -> bool) -> Vec<Tile> {
        if !self.passable(to) || !allowed(to) {
            return Vec::new();
        }
        self.search(from, &|t| t == to, Some(to), ROAD as u32, max_nodes, &|_, n| self.cost(n).filter(|_| allowed(n)))
    }

    /// Cheapest path (A*) where `step(from, onto)` gives the cost units of a step between
    /// neighbours (the planner prices the cell entered, times [`Grid::weight`]), `None` if it
    /// is not allowed. Excludes `from`; empty if unreachable or equal.
    pub fn path_by(&self, from: Tile, to: Tile, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        self.search(from, &|t| t == to, Some(to), 1, max_nodes, step)
    }

    /// Cheapest path to the nearest cell `goal` accepts (Dijkstra), as [`TileMap::path_by`].
    pub fn path_to_any(&self, from: Tile, goal: &dyn Fn(Tile) -> bool, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        self.search(from, goal, None, 1, max_nodes, step)
    }

    fn search(&self, from: Tile, goal: &dyn Fn(Tile) -> bool, target: Option<Tile>, min_cost: u32, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        let Some(start) = self.index(from) else { return Vec::new() };
        if goal(from) || target.is_some_and(|t| self.index(t).is_none()) {
            return Vec::new();
        }
        let g = self.grid;
        let h = |t: Tile| target.map_or(0, |to| min_cost * g.weight_bound(t, to));
        let n = self.land.len();
        let mut best = vec![u32::MAX; n];
        let mut parent = vec![u32::MAX; n];
        best[start] = 0;
        let mut open = BinaryHeap::from([Reverse((h(from), 0u32, start as u32))]);
        let mut expanded = 0;
        while let Some(Reverse((_, gc, i))) = open.pop() {
            let i = i as usize;
            if gc > best[i] {
                continue;
            }
            let here = self.tile_of(i);
            if i != start && goal(here) {
                let mut path = Vec::new();
                let mut cur = i;
                while cur != start {
                    path.push(self.tile_of(cur));
                    cur = parent[cur] as usize;
                }
                path.reverse();
                return path;
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            for nb in g.neighbours(here) {
                let Some(j) = self.index(nb) else { continue };
                let Some(c) = step(here, nb).filter(|&c| c > 0) else { continue };
                let ng = gc + c as u32 * g.weight(here, nb);
                if ng < best[j] {
                    best[j] = ng;
                    parent[j] = i as u32;
                    open.push(Reverse((ng + h(nb), ng, j as u32)));
                }
            }
        }
        Vec::new()
    }

    /// The neighbours of cell `i` in the order of [`Grid::directions`] (`None` off the map or
    /// past the grid's directions).
    fn neighbour_indices(&self, i: usize) -> [Option<usize>; 8] {
        let w = self.w as usize;
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        let mut out = [None; 8];
        match self.grid {
            Grid::Square8 if x > 0 && y > 0 && x + 1 < self.w && y + 1 < self.h => {
                // Inside the border: every neighbour is on the map.
                for (o, &(dx, dy)) in out.iter_mut().zip(DIRECTIONS.iter()) {
                    *o = Some((i as isize + dy as isize * w as isize + dx as isize) as usize);
                }
            }
            Grid::Square8 => {
                for (o, &(dx, dy)) in out.iter_mut().zip(DIRECTIONS.iter()) {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < self.w && ny < self.h {
                        *o = Some((ny * self.w + nx) as usize);
                    }
                }
            }
            Grid::HexOddR => {
                // The same cells as `neighbour_index` in every direction (its offsets are
                // this row's, `directions(y)`), with the row's offsets worked out once.
                for (o, d) in out.iter_mut().zip(self.grid.directions(y)) {
                    *o = d.and_then(|(dx, dy)| self.index((x + dx, y + dy)));
                }
            }
        }
        out
    }

    /// Neighbour of cell `i` in direction `dir` of [`Grid::directions`], if on the map.
    fn neighbour_index(&self, i: usize, dir: usize) -> Option<usize> {
        let (x, y) = self.tile_of(i);
        let (dx, dy) = match self.grid {
            Grid::Square8 => DIRECTIONS[dir],
            Grid::HexOddR => self.grid.directions(y)[dir]?,
        };
        self.index((x + dx, y + dy))
    }

    /// The original's planner (world.md §1.2, 0x482a58 and 0x482fe8): a flood from the
    /// target outwards that stops as soon as it first reaches `from`, then the route read
    /// back from `from` by steepest descent.
    ///
    /// - `cost` is the planner's map (cost units, 0 = impassable) and `mask` the
    ///   multiplier laid over it (0 closes a cell, 1 keeps it). Their product is 16-bit.
    ///   Razdor fixes the original's bug: its loop skipped cell (0, 0), which kept the bare
    ///   mask as its cost.
    /// - `seeds`: target cells and start values; a seed on a cell `cost` blocks is refused,
    ///   the value is capped at 32 766 and stored plus 1. A seed on `from` is dropped, so a
    ///   walker standing on the target has no route.
    /// - A cell's distance is that of the cell it was reached from plus its own cost times
    ///   the step weight: walking the other way, every step is charged the cell **left**
    ///   (`from` included, the target not). Rounds expand the frontier entries at its
    ///   smallest value; the flood stops the moment `from` is first improved, so the route
    ///   can be a little dearer than the cheapest one (a first step taken diagonally when
    ///   the straight one was cheaper). Distances above 65 534 are never stored.
    /// - Reading: from `from`, step to the neighbour with the smallest distance below the
    ///   current one, directions tried in order and the first strict minimum winning ties.
    ///
    /// Returns the route (without `from`) and its cost: per step the base cost of the cell
    /// left, ×1.5 rounded down on diagonals (the status bar's time left, §2.4). `None` if
    /// `from` was not reached.
    pub fn flood_route(&self, cost: &dyn Fn(Tile) -> u16, mask: &dyn Fn(Tile) -> u16, seeds: &[(Tile, u32)], from: Tile) -> Option<(Vec<Tile>, u32)> {
        let field = self.flood_field(cost, mask, seeds, from);
        let stop = self.index(from)?;
        if field.dist[stop] == UNREACHED {
            return None;
        }
        let route = self.descend(&field, from);
        let mut total = 0;
        let mut prev = from;
        let mut here = cost(from) as u32;
        for &t in &route {
            total += if self.grid.weight(prev, t) == DIAGONAL_WEIGHT { (here * 3) >> 1 } else { here };
            here = cost(t) as u32;
            prev = t;
        }
        Some((route, total))
    }

    /// The flood of [`TileMap::flood_route`] (0x482a58): every cell's distance to the nearest
    /// seed as far as the flood went (it stops the moment `from` is first given one), and how
    /// many seeds it kept (a seed on `from` is dropped). `from` off the map floods nothing.
    pub fn flood_field(&self, cost: &dyn Fn(Tile) -> u16, mask: &dyn Fn(Tile) -> u16, seeds: &[(Tile, u32)], from: Tile) -> FloodField {
        let n = (self.w.max(0) * self.h.max(0)) as usize;
        let costs: Vec<u16> = (0..n).map(|i| cost(self.tile_of(i))).collect();
        let mult: Vec<u16> = (0..n).map(|i| mask(self.tile_of(i))).collect();
        self.flood_maps(&costs, &mult, seeds, from)
    }

    /// The cost map of the planner for foot armies (LAND) and for ships (SHIP), cost units
    /// per cell, 0 closed.
    pub fn land_costs(&self) -> &[u16] {
        &self.land
    }

    pub fn water_costs(&self) -> &[u16] {
        &self.water
    }

    /// [`TileMap::flood_field`] on a cost map and a multiplier map given cell by cell.
    pub fn flood_maps(&self, cost: &[u16], mult: &[u16], seeds: &[(Tile, u32)], from: Tile) -> FloodField {
        let n = (self.w.max(0) * self.h.max(0)) as usize;
        let mut dist = vec![UNREACHED; n];
        let Some(stop) = self.index(from) else { return FloodField { dist, kept: 0, w: self.w } };
        let mut placed: Vec<(usize, u32)> = Vec::new();
        for &(t, v) in seeds {
            let Some(i) = self.index(t) else { continue };
            let v = v.min(32_766);
            // A seed on a cell that already has a lower one is refused. Razdor fixes the
            // original's off-by-one bug (0x482984): it compared the stored value (plus 1) with
            // the new value before its plus 1, so a seed one above was still added.
            if cost[i] == 0 || placed.iter().any(|&(j, w)| j == i && w <= v) {
                continue;
            }
            placed.push((i, v + 1));
        }
        // A seed on `from` itself is dropped before the flood (0x482a58): his cell stays
        // unreached and there is no route.
        let frontier: Vec<(usize, u32)> = placed.into_iter().filter(|&(i, _)| i != stop).collect();
        for &(i, v) in &frontier {
            dist[i] = v as u16;
        }
        let kept = frontier.len();
        SCRATCH.with(|s| {
            let s = &mut *s.borrow_mut();
            s.begin(n);
            self.flood_with(s, cost, mult, &frontier, stop, &mut dist);
            s.end();
        });
        FloodField { dist, kept, w: self.w }
    }

    /// The flood itself, in `s`'s buffers.
    fn flood_with(&self, s: &mut FloodScratch, cost: &[u16], mult: &[u16], frontier: &[(usize, u32)], stop: usize, dist: &mut [u16]) {
        // The effective cost of a cell: its cost times its multiplier, in 16 bits (cell 0
        // too: the original's loop stopped before it, its bug).
        let epoch = s.epoch;
        let effective_at = |i: usize| -> u16 { cost[i].wrapping_mul(mult[i]) };
        // When `from` itself is closed the flood never reaches it and runs to its end, and
        // then every distance is the shortest one, whatever the order. The path read from
        // `from` only needs its neighbours' distances and the lower ones around the cells it
        // steps to, which are final once a pass's value reaches them: the flood can stop
        // there with the very same path.
        if effective_at(stop) == 0 {
            // When no seed can reach any of its neighbours either, that flood would cover all
            // it can reach without giving them a value, and the path read from `from` is
            // empty whatever it did: a search out from `from` that meets no seed says so.
            for &(i, _) in frontier {
                s.seed[i] = epoch;
            }
            s.todo.clear();
            s.todo.push(stop);
            s.seen[stop] = epoch;
            let mut reachable = false;
            let mut k = 0;
            'search: while k < s.todo.len() {
                let i = s.todo[k];
                k += 1;
                for j in self.neighbour_indices(i).into_iter().flatten() {
                    if s.seed[j] == epoch {
                        reachable = true;
                        break 'search;
                    }
                    if s.seen[j] != epoch && effective_at(j) != 0 {
                        s.seen[j] = epoch;
                        s.todo.push(j);
                    }
                }
            }
            if !reachable {
                return;
            }
            let watch: Vec<usize> = self.neighbour_indices(stop).into_iter().flatten().filter(|&j| effective_at(j) != 0).collect();
            s.heap.clear();
            s.heap.extend(frontier.iter().map(|&(i, v)| Reverse((v, i as u32))));
            while let Some(Reverse((d, i))) = s.heap.pop() {
                if watch.iter().all(|&j| dist[j] as u32 <= d) {
                    break;
                }
                let i = i as usize;
                if d > dist[i] as u32 {
                    continue;
                }
                let around = self.neighbour_indices(i);
                for (dir, j) in around.iter().enumerate().take(self.grid.direction_count()) {
                    let Some(j) = *j else { continue };
                    let c = effective_at(j) as u32;
                    if c == 0 {
                        continue;
                    }
                    let nd = c * self.grid.direction_weight(dir) + d;
                    if nd < dist[j] as u32 {
                        dist[j] = nd as u16;
                        s.heap.push(Reverse((nd, j as u32)));
                    }
                }
            }
            return;
        }
        // The original's flood (0x482a58) runs in passes: every entry of its frontier at the
        // lowest value expands, in frontier order, the others are carried, and an expanded
        // entry's new neighbours take its place in the order. The same, without scanning
        // the carried entries at every pass: the frontier is a linked list in that order
        // (labels compare positions), and the entries wait in buckets by value.
        for &(i, v) in frontier {
            let node = s.order.push_back(i);
            bucket(&mut s.buckets, &mut s.used, v).push(node as u32);
        }
        let mut value = frontier.iter().map(|e| e.1).min().unwrap_or(UNREACHED as u32);
        let mut batch: Vec<u32> = Vec::new();
        let mut children: Vec<(usize, u32)> = Vec::new();
        'flood: while (value as usize) < s.buckets.len() {
            if s.buckets[value as usize].is_empty() {
                value += 1;
                continue;
            }
            std::mem::swap(&mut batch, &mut s.buckets[value as usize]);
            let order = &mut s.order;
            // The labels are unique: an unstable sort gives the same order, and a batch
            // often comes in order already.
            if !batch.is_sorted_by_key(|&n| order.label(n as usize)) {
                batch.sort_unstable_by_key(|&n| order.label(n as usize));
            }
            for &node in &batch {
                let node = node as usize;
                let (i, d) = (s.order.cell(node), value);
                // An entry whose cell has since a lower value offers nothing.
                if d > dist[i] as u32 {
                    s.order.remove(node);
                    continue;
                }
                children.clear();
                let around = self.neighbour_indices(i);
                for dir in (0..self.grid.direction_count()).rev() {
                    let Some(j) = around[dir] else { continue };
                    let c = effective_at(j) as u32;
                    let nd = if c == 0 { UNREACHED as u32 } else { c * self.grid.direction_weight(dir) + d };
                    if nd < dist[j] as u32 {
                        dist[j] = nd as u16;
                        if j == stop {
                            break 'flood;
                        }
                        children.push((j, nd));
                    }
                }
                let first = s.order.replace(node, &children);
                for (k, &(_, nd)) in children.iter().enumerate() {
                    bucket(&mut s.buckets, &mut s.used, nd).push((first + k) as u32);
                }
            }
            batch.clear();
            value += 1;
        }
    }

    /// The path read back from a flood (0x482fe8): from `from`, the neighbour with the
    /// smallest non-zero distance below the current one, directions in order and the first
    /// strict minimum winning ties, until none is lower. From an unreached cell the first
    /// step goes to any reached neighbour.
    pub fn descend(&self, field: &FloodField, from: Tile) -> Vec<Tile> {
        let mut route = Vec::new();
        let Some(mut at) = self.index(from) else { return route };
        let mut d = field.dist[at];
        while d != 0 {
            let mut best = None;
            let mut best_d = d;
            for dir in 0..self.grid.direction_count() {
                let Some(j) = self.neighbour_index(at, dir) else { continue };
                if field.dist[j] != 0 && field.dist[j] < best_d {
                    best = Some(j);
                    best_d = field.dist[j];
                }
            }
            let Some(j) = best else { break };
            route.push(self.tile_of(j));
            at = j;
            d = best_d;
        }
        route
    }

    /// Planner cost of `path` from `from` on foot: every cell entered times its step weight.
    pub fn path_cost(&self, from: Tile, path: &[Tile]) -> u32 {
        let mut prev = from;
        let mut total = 0;
        for &t in path {
            total += self.cost(t).unwrap_or(0) as u32 * self.grid.weight(prev, t);
            prev = t;
        }
        total
    }

    /// Walking time (minutes) of `path` from `from` at the knight's speed: each step costs
    /// the cell being left, ×1.5 diagonally (world.md §1).
    pub fn path_minutes(&self, from: Tile, path: &[Tile]) -> u32 {
        self.path_minutes_by(from, path, &|a, b| step_minutes(self.grid, a, b, self.cost(a).unwrap_or(ROAD), BASE_SPEED as u32))
    }

    /// Minutes of `path` from `from`, `step(from, to)` minutes per step.
    pub fn path_minutes_by(&self, from: Tile, path: &[Tile], step: &dyn Fn(Tile, Tile) -> f32) -> u32 {
        let mut prev = from;
        let mut total = 0.0;
        for &t in path {
            total += step(prev, t);
            prev = t;
        }
        total.round() as u32
    }

    /// Centre of a cell in world units.
    pub fn center(&self, t: Tile) -> (f32, f32) {
        self.grid.center(t)
    }

    /// Cell containing a world position.
    pub fn tile_at(&self, p: (f32, f32)) -> Tile {
        self.grid.tile_at(p)
    }

    /// Steps between two cells.
    pub fn distance(&self, a: Tile, b: Tile) -> i32 {
        self.grid.distance(a, b)
    }

    /// Cells reachable on foot from `from` (a flood fill), as a `w*h` mask.
    pub fn reachable(&self, from: Tile) -> Vec<bool> {
        let mut seen = vec![false; self.land.len()];
        let Some(start) = self.index(from) else { return seen };
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            for nb in self.grid.neighbours(self.tile_of(i)) {
                if let Some(j) = self.index(nb) {
                    if !seen[j] && self.land[j] > 0 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        seen
    }

    /// Connected regions of passable cells: a label per cell (`u32::MAX` for impassable ones)
    /// and the size of each region.
    pub fn regions(&self) -> (Vec<u32>, Vec<usize>) {
        let mut label = vec![u32::MAX; self.land.len()];
        let mut sizes = Vec::new();
        for s in 0..self.land.len() {
            if self.land[s] == 0 || label[s] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut n = 1;
            label[s] = id;
            let mut stack = vec![s];
            while let Some(i) = stack.pop() {
                for nb in self.grid.neighbours(self.tile_of(i)) {
                    if let Some(j) = self.index(nb) {
                        if label[j] == u32::MAX && self.land[j] > 0 {
                            label[j] = id;
                            n += 1;
                            stack.push(j);
                        }
                    }
                }
            }
            sizes.push(n);
        }
        (label, sizes)
    }

    /// Index of a tile into [`TileMap::reachable`]'s mask.
    pub fn mask_index(&self, t: Tile) -> Option<usize> {
        self.index(t)
    }
}

/// Game minutes of a step from `a` to its neighbour `b` for an army of `speed` when the step
/// is charged `cost` units: `cost × speed`, ×1.5 diagonally (world.md §2).
pub fn step_minutes(grid: Grid, a: Tile, b: Tile, cost: u16, speed: u32) -> f32 {
    cost as f32 * speed as f32 * grid.weight(a, b) as f32 / ORTHOGONAL_WEIGHT as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original's flood pass by pass (0x482a58), as it reads: the whole frontier is
    /// scanned at every pass. [`TileMap::flood_field`] must give the very same distances.
    fn flood_by_passes(map: &TileMap, cost: &dyn Fn(Tile) -> u16, mask: &dyn Fn(Tile) -> u16, seeds: &[(Tile, u32)], from: Tile) -> Vec<u16> {
        let n = (map.w * map.h) as usize;
        let stop = map.index(from).unwrap();
        // With Razdor's fixes of the original's bugs: cell 0 priced as any, a seed one above
        // refused.
        let effective: Vec<u16> = (0..n).map(|i| cost(map.tile_of(i)).wrapping_mul(mask(map.tile_of(i)))).collect();
        let mut dist = vec![u16::MAX; n];
        let mut placed: Vec<(usize, u32)> = Vec::new();
        for &(t, v) in seeds {
            let i = map.index(t).unwrap();
            let v = v.min(32_766);
            if cost(t) == 0 || placed.iter().any(|&(j, w)| j == i && w <= v) {
                continue;
            }
            placed.push((i, v + 1));
        }
        let mut frontier: Vec<(usize, u32)> = Vec::new();
        for &(i, v) in placed.iter().filter(|&&(i, _)| i != stop) {
            dist[i] = v as u16;
            frontier.push((i, v));
        }
        let mut threshold = 1;
        'flood: while !frontier.is_empty() {
            let mut next = Vec::new();
            for &(i, d) in &frontier {
                if d > threshold {
                    next.push((i, d));
                    continue;
                }
                for dir in (0..8).rev() {
                    let Some(j) = map.neighbour_index(i, dir) else { continue };
                    let c = effective[j] as u32;
                    let nd = if c == 0 { u16::MAX as u32 } else { c * map.grid.direction_weight(dir) + d };
                    if nd < dist[j] as u32 {
                        dist[j] = nd as u16;
                        if j == stop {
                            break 'flood;
                        }
                        next.push((j, nd));
                    }
                }
            }
            threshold = next.iter().map(|e| e.1).min().unwrap_or(u16::MAX as u32);
            frontier = next;
        }
        dist
    }

    #[test]
    fn the_flood_matches_the_originals_passes() {
        let mut state = 12345u32;
        let mut rnd = |n: u32| {
            state = state.wrapping_mul(214_013).wrapping_add(2_531_011);
            (state >> 16) % n
        };
        for case in 0..600 {
            let (w, h) = (3 + rnd(30) as i32, 3 + rnd(30) as i32);
            let mut map = TileMap::from_codes(Grid::Square8, w, h, &vec![0; (w * h) as usize], Vec::new());
            let costs: Vec<u16> = (0..w * h).map(|_| if rnd(6) == 0 { 0 } else { 1 + rnd(8) as u16 }).collect();
            let mult: Vec<u16> = (0..w * h).map(|_| match rnd(10) { 0 => 0, 1 => 1 + rnd(40) as u16, _ => 1 }).collect();
            for (i, &c) in costs.iter().enumerate() {
                let t = (i as i32 % w, i as i32 / w);
                if c > 0 {
                    map.open(t, c);
                }
            }
            let cost = |t: Tile| map.cost(t).unwrap_or(0);
            let seeds: Vec<(Tile, u32)> = (0..1 + rnd(12)).map(|_| ((rnd(w as u32) as i32, rnd(h as u32) as i32), rnd(if case % 3 == 0 { 4 } else { 3000 }))).collect();
            let from = (rnd(w as u32) as i32, rnd(h as u32) as i32);
            // Every other case the start is closed, as an ignored army closes its own cell.
            let mut mult = mult;
            if case % 2 == 1 {
                mult[(from.1 * w + from.0) as usize] = 0;
            }
            let mask = |t: Tile| mult[(t.1 * w + t.0) as usize];
            let want = flood_by_passes(&map, &cost, &mask, &seeds, from);
            let mut got = map.flood_field(&cost, &mask, &seeds, from);
            // Where the flood reaches the start the whole field is the original's; else only
            // the path read from it is.
            let closed = mask(from) == 0 || cost(from) == 0;
            let k = (from.1 * w + from.0) as usize;
            if !closed && want[k] != u16::MAX {
                assert_eq!(got.dist, want, "case {case}");
            }
            // The path read from it is the same, with cells around it erased too.
            let mut full = FloodField { dist: want, kept: got.kept, w };
            for _ in 0..rnd(3) {
                let t = (from.0 + rnd(3) as i32 - 1, from.1 + rnd(3) as i32 - 1);
                full.erase(t);
                got.erase(t);
            }
            assert_eq!(map.descend(&got, from), map.descend(&full, from), "case {case}");
        }
    }

    // Odd rows are drawn shifted right:
    //   . . . . .
    //    . ~ ~ ~ .
    //   . ~ X ~ .
    //    . = = = =
    //   T T T T T
    const MAP: &str = "\
.....
.~~~.
.~X~.
.====
TTTTT
";

    #[test]
    fn parses_terrain_and_markers() {
        let m = TileMap::parse(MAP);
        assert_eq!((m.w, m.h), (5, 5));
        assert_eq!(m.surface((1, 1)), Surface::DeepSea);
        assert_eq!(m.surface((1, 3)), Surface::Road);
        assert_eq!(m.surface((0, 4)), Surface::GrassPlain);
        assert_eq!(m.objects.len(), 5, "a tree on every forest cell");
        assert_eq!(m.markers, vec![('X', (2, 2))]);
        assert_eq!(m.surface((2, 2)), Surface::Road);
        assert_eq!(m.minutes((1, 3)), Some(15));
        assert_eq!(m.minutes((0, 0)), Some(25));
        assert_eq!(m.minutes((0, 4)), Some(45), "a tree adds 4 units to grass's 5");
        assert_eq!(m.minutes((1, 1)), None);
        assert_eq!(m.objects_in_rows(4, 5).len(), 5);
        assert!(m.objects_in_rows(0, 4).is_empty());
    }

    #[test]
    fn hex_neighbours_depend_on_row_parity() {
        // Even row: the row above/below sits half a hex to the left.
        assert_eq!(hex_neighbours((2, 2)), [(1, 2), (3, 2), (1, 1), (2, 1), (1, 3), (2, 3)]);
        // Odd row: shifted right.
        assert_eq!(hex_neighbours((2, 1)), [(1, 1), (3, 1), (2, 0), (3, 0), (2, 2), (3, 2)]);
        for n in hex_neighbours((2, 1)) {
            assert_eq!(hex_distance((2, 1), n), 1);
            let (a, b) = (Grid::HexOddR.center((2, 1)), Grid::HexOddR.center(n));
            assert!(((a.0 - b.0).hypot(a.1 - b.1) - 1.0).abs() < 1e-4, "neighbour centres are 1 apart");
            assert_eq!(Grid::HexOddR.weight((2, 1), n), 2);
        }
        assert_eq!(hex_distance((0, 0), (4, 0)), 4);
        assert_eq!(hex_distance((0, 0), (0, 4)), 4);
        assert_eq!(hex_disk((5, 5), 0).count(), 1);
        assert_eq!(hex_disk((5, 5), 1).count(), 7);
        assert_eq!(hex_disk((5, 6), 2).count(), 19);
    }

    #[test]
    fn tile_at_inverts_center() {
        for r in 0..6 {
            for c in 0..6 {
                for g in [Grid::HexOddR, Grid::Square8] {
                    assert_eq!(g.tile_at(g.center((c, r))), (c, r));
                    let (x, y) = g.center((c, r));
                    assert!(g.tile_at((x + 0.3, y - 0.2)) == (c, r), "near the centre stays inside");
                }
            }
        }
    }

    fn row(surfaces: &[Surface]) -> TileMap {
        let codes: Vec<u8> = surfaces.iter().map(|s| *s as u8).collect();
        TileMap::from_codes(Grid::Square8, codes.len() as i32, 1, &codes, vec![])
    }

    #[test]
    fn terrain_values_are_the_originals() {
        use Surface::*;
        let all = Surface::ALL;
        let m = row(&all);
        let foot: Vec<Option<u16>> = (0..16).map(|x| m.minutes((x, 0))).collect();
        // Road 15, stony 20, grass/dry/sand/clay/scorched 25, snow 30, marsh 40 minutes.
        let expect = [None, None, None, None, Some(15), Some(25), Some(25), Some(25), Some(40), None, Some(25), Some(25), Some(20), Some(25), Some(30), None];
        assert_eq!(foot, expect);
        let ship: Vec<Option<u16>> = (0..16).map(|x| m.water_cost((x, 0))).collect();
        assert_eq!(&ship[..3], &[Some(2), Some(1), None], "shallows 2, coastal 1, deep sea blocks ships too");
        assert!(ship[3..].iter().all(Option::is_none), "no ship on land");
        for s in [ShallowsFords, CoastalWater, DeepSea] {
            assert!(is_water(s) && surface_minutes(s).is_none());
        }
        assert!(!is_water(LavaFields) && surface_minutes(LavaFields).is_none(), "lava blocks");
        assert_eq!(surface_minutes(Marsh), Some(40));
    }

    fn obj(x: i32, y: i32, class: u8, sprite: u8) -> Decoration {
        Decoration { tile: (x, y), class, sprite }
    }

    #[test]
    fn objects_add_fixed_costs() {
        use object_class::*;
        let objects = vec![
            obj(0, 0, HILLS, 12),       // hill: base 2 over 1×1, + grass 5
            obj(1, 0, YELLOW_HILL, 10), // class 4: base 3
            obj(2, 0, TREES, 3),        // +4
            obj(3, 0, DEAD_TREES, 110), // +6, dead trees are slower than trees
            obj(4, 0, 12, 0),           // class 12: +4
            obj(5, 0, THICKET, 3),      // blocks
            obj(6, 0, ROCKS, 12),       // blocks
            obj(7, 0, MOUNTAINS, 15),   // blocks
            obj(8, 0, 7, 10),           // class 7 blocks too
            obj(9, 0, 13, 0),           // unknown: nothing
            obj(10, 0, TREES, 1),       // a tree on a hill: 2 + 5 + 4
            obj(10, 0, HILLS, 10),
        ];
        let m = TileMap::from_codes(Grid::Square8, 12, 1, &[Surface::GrassPlain as u8; 12], objects);
        let c: Vec<Option<u16>> = (0..12).map(|x| m.cost((x, 0))).collect();
        assert_eq!(c, [Some(7), Some(8), Some(9), Some(11), Some(9), None, None, None, None, Some(5), Some(11), Some(5)]);
        // Thickets on a road block it (the original keeps no road open).
        let objects = vec![obj(0, 0, THICKET, 1)];
        let m = TileMap::from_codes(Grid::Square8, 1, 1, &[Surface::Road as u8], objects);
        assert!(!m.passable((0, 0)));
        // A hill in the water stays water: the ship map gets its cost.
        let m = TileMap::from_codes(Grid::Square8, 1, 1, &[Surface::CoastalWater as u8], vec![obj(0, 0, HILLS, 10)]);
        assert_eq!((m.cost((0, 0)), m.water_cost((0, 0))), (None, Some(3)));
    }

    #[test]
    fn plants_and_massifs_in_the_water_block_ships() {
        use object_class::*;
        let water = [Surface::CoastalWater as u8; 6];
        let objects = vec![obj(0, 0, TREES, 1), obj(1, 0, DEAD_TREES, 1), obj(2, 0, MOUNTAINS, 10), obj(3, 0, ROCKS, 10), obj(4, 0, HILLS, 10)];
        let m = TileMap::from_codes(Grid::Square8, 6, 1, &water, objects);
        let ship: Vec<Option<u16>> = (0..6).map(|x| m.water_cost((x, 0))).collect();
        // A hill in the water stays water (1 + 2); the plants, the mountain and the rock block.
        assert_eq!(ship, [None, None, None, None, Some(3), Some(1)]);
        assert!((0..6).all(|x| m.cost((x, 0)).is_none()), "never walked");
    }

    #[test]
    fn overlapping_hills_the_later_in_the_row_scan_wins() {
        use object_class::*;
        // A 2×2 hill of class 4 (base 3) at (1, 1) and a 2×2 of class 1 (base 2) at (2, 1):
        // they share column 1. In file order the class-4 hill comes last, but the row-by-row
        // scan meets (2, 1) after (1, 1), so the class-1 hill wins the shared cells.
        let objects = vec![obj(2, 1, HILLS, 20), obj(1, 1, YELLOW_HILL, 20)];
        let m = TileMap::from_codes(Grid::Square8, 4, 2, &[Surface::GrassPlain as u8; 8], objects);
        assert_eq!(m.cost((0, 0)), Some(8), "only the class-4 hill: 3 + 5");
        assert_eq!(m.cost((1, 0)), Some(7), "shared: the class-1 hill, 2 + 5");
        assert_eq!(m.cost((2, 1)), Some(7));
        // In the scan, row 0 comes before row 1 whatever the column.
        let objects = vec![obj(1, 1, HILLS, 20), obj(2, 0, YELLOW_HILL, 20)];
        let m = TileMap::from_codes(Grid::Square8, 4, 2, &[Surface::GrassPlain as u8; 8], objects);
        assert_eq!(m.cost((1, 0)), Some(7), "the row-1 hill is laid after the row-0 one");
    }

    #[test]
    fn massifs_cover_a_square_with_the_object_at_the_bottom_right() {
        let m = TileMap::from_codes(Grid::Square8, 8, 8, &[Surface::GrassPlain as u8; 64], vec![obj(5, 6, object_class::MOUNTAINS, 34)]);
        for y in 0..8 {
            for x in 0..8 {
                let covered = (3..=5).contains(&x) && (4..=6).contains(&y);
                assert_eq!(!m.passable((x, y)), covered, "({x}, {y})");
            }
        }
        // Side = the sprite's tens digit; 0–9 cover no cell; clipped at the map's edge.
        assert_eq!(object_side(object_class::HILLS, 9), 0);
        assert_eq!(object_side(object_class::ROCKS, 20), 2);
        assert_eq!(object_side(object_class::TREES, 55), 1, "plants cover their own cell");
        let m = TileMap::from_codes(Grid::Square8, 3, 3, &[Surface::GrassPlain as u8; 9], vec![obj(1, 1, object_class::HILLS, 40), obj(2, 2, object_class::ROCKS, 5)]);
        assert_eq!(m.cost((0, 0)), Some(7));
        assert_eq!(m.cost((1, 1)), Some(7));
        assert_eq!(m.cost((2, 1)), Some(5), "right of the object's cell: not covered");
        assert!(m.passable((2, 2)), "sprite 5: side 0");
    }

    #[test]
    fn path_avoids_water_and_prefers_road() {
        let m = TileMap::parse(MAP);
        let p = m.path((0, 3), (4, 3));
        assert_eq!(p, vec![(1, 3), (2, 3), (3, 3), (4, 3)], "straight along the road");
        // Charged the cell left: grass 25, then three road cells 15.
        assert_eq!(m.path_minutes((0, 3), &p), 25 + 3 * 15);
        assert_eq!(m.path_cost((0, 3), &p), 4 * 3 * 2, "planner: the cells entered × weight 2");
        let p = m.path((0, 0), (4, 4));
        assert!(p.iter().all(|&t| m.passable(t)));
        for w in p.windows(2) {
            assert_eq!(hex_distance(w[0], w[1]), 1, "every step is to a neighbouring hex");
        }
        assert!(m.path_limited((0, 0), (4, 4), 3).is_empty(), "gives up");
    }

    #[test]
    fn no_path_into_water_or_to_self() {
        let m = TileMap::parse(MAP);
        assert!(m.path((0, 0), (1, 1)).is_empty());
        assert!(m.path((0, 0), (0, 0)).is_empty());
        assert!(m.path((0, 0), (99, 0)).is_empty());
        let near = m.nearest_passable((1, 1), 2).unwrap();
        assert!(m.passable(near) && hex_distance(near, (1, 1)) == 1);
        assert_eq!(m.nearest_passable((0, 0), 2), Some((0, 0)));
    }

    #[test]
    fn square_grid_has_eight_neighbours_and_diagonals_weigh_one_and_a_half() {
        let g = Grid::Square8;
        let n: Vec<Tile> = g.neighbours((5, 5)).collect();
        assert_eq!(n.len(), 8);
        assert!(n.contains(&(4, 6)) && n.contains(&(6, 4)));
        assert_eq!(g.distance((0, 0), (3, 7)), 7);
        assert_eq!(g.octile((0, 0), (3, 7)), 7 + 1, "max + min/2");
        assert_eq!(g.disk((5, 5), 2).len(), 25);
        assert_eq!((g.weight((0, 0), (1, 0)), g.weight((0, 0), (0, 1)), g.weight((0, 0), (1, 1))), (2, 2, 3), "vertical = horizontal");
        assert_eq!(step_minutes(g, (0, 0), (1, 1), 5, 5), 37.5);
        assert_eq!(step_minutes(g, (0, 0), (0, 1), 5, 4), 20.0, "ranger speed 4");
        // A diagonal causeway across water, one column per row (like the original's bridges).
        let mut codes = vec![Surface::DeepSea as u8; 25];
        for i in 0..5 {
            codes[i * 5 + (4 - i)] = Surface::Road as u8;
        }
        let m = TileMap::from_codes(g, 5, 5, &codes, vec![]);
        let p = m.path((4, 0), (0, 4));
        assert_eq!(p, vec![(3, 1), (2, 2), (1, 3), (0, 4)]);
        assert_eq!(m.path_minutes((4, 0), &p), 4 * 15 * 3 / 2);
        // The same causeway is broken on a hex grid.
        let hex = TileMap::from_codes(Grid::HexOddR, 5, 5, &codes, vec![]);
        assert!(hex.path((4, 0), (0, 4)).is_empty());
        // On open grass two orthogonal steps (weight 4) beat nothing: a diagonal (3) is
        // taken where it saves, e.g. to (2, 1) one diagonal and one straight step.
        let m = TileMap::from_codes(g, 5, 5, &[Surface::GrassPlain as u8; 25], vec![]);
        assert_eq!(m.path((0, 0), (2, 1)).len(), 2);
        assert_eq!(m.path_cost((0, 0), &m.path((0, 0), (2, 1))), 5 * 3 + 5 * 2);
    }

    fn costed(rows: &[[u16; 4]]) -> (TileMap, Vec<u16>) {
        let m = TileMap::from_codes(Grid::Square8, 4, rows.len() as i32, &vec![Surface::GrassPlain as u8; 4 * rows.len()], vec![]);
        (m, rows.iter().flatten().copied().collect())
    }

    #[test]
    fn the_flood_stops_at_the_first_value_reaching_the_hero() {
        // Costs (cost units of each cell) from the original's rules: the flood starts at the
        // target (3, 0) and stops when it first reaches the hero at (0, 0).
        let (m, c) = costed(&[[3, 3, 8, 3], [5, 5, 5, 3], [5, 5, 5, 3]]);
        let cost = |t: Tile| c[(t.1 * 4 + t.0) as usize];
        let (route, total) = m.flood_route(&cost, &|_| 1, &[((3, 0), 0)], (0, 0)).unwrap();
        // Through (2, 1), not along the top row: walking charges the cell left, 3·2 + 3·3 +
        // 5·3 = 30 weighted units where (1, 0), (2, 0), (3, 0) would cost 3·2 + 3·2 + 8·2 =
        // 28; the flood stopped before it found the cheaper way.
        assert_eq!(route, vec![(1, 0), (2, 1), (3, 0)]);
        // The route's cost for the status bar: 3, then 3·1.5 → 4, then 5·1.5 → 7.
        assert_eq!(total, 3 + 4 + 7);
        // The target cell is not charged, the hero's own cell is.
        let (m, c) = costed(&[[8, 3, 3, 3], [5, 5, 5, 5], [5, 5, 5, 5]]);
        let cost = |t: Tile| c[(t.1 * 4 + t.0) as usize];
        assert_eq!(m.flood_route(&cost, &|_| 1, &[((3, 0), 0)], (0, 0)).unwrap(), (vec![(1, 0), (2, 0), (3, 0)], 8 + 3 + 3));
    }

    #[test]
    fn the_route_descends_in_direction_order() {
        let m = TileMap::from_codes(Grid::Square8, 6, 4, &[Surface::GrassPlain as u8; 24], vec![]);
        let grass = |_| 5;
        let route = |from, to| m.flood_route(&grass, &|_| 1, &[(to, 0)], from).unwrap().0;
        // On open grass the diagonal comes first.
        assert_eq!(route((0, 0), (3, 1)), vec![(1, 1), (2, 1), (3, 1)]);
        assert_eq!(route((0, 3), (4, 0)), vec![(1, 2), (2, 1), (3, 0), (4, 0)]);
        assert_eq!(route((5, 0), (0, 1)), vec![(4, 1), (3, 1), (2, 1), (1, 1), (0, 1)]);
        // Standing on the target: the seed on his own cell is dropped, so no route at all.
        assert!(m.flood_route(&grass, &|_| 1, &[((2, 2), 0)], (2, 2)).is_none());
    }

    #[test]
    fn a_second_seed_above_on_the_same_cell_is_refused() {
        // (1, 0) with 0 then 1 keeps 0 (stored 1), so the walker at (2, 0) goes west, not to
        // (3, 0) stored 2. The original's off-by-one bug (0x482984) compared the stored value
        // (plus 1) with the new value before its plus 1, so the 1 overwrote the 0 and both
        // sides read 2: the descent went east.
        let m = TileMap::from_codes(Grid::Square8, 4, 1, &[Surface::GrassPlain as u8; 4], vec![]);
        let grass = |_| 5;
        let (route, _) = m.flood_route(&grass, &|_| 1, &[((1, 0), 0), ((1, 0), 1), ((3, 0), 1)], (2, 0)).unwrap();
        assert_eq!(route, vec![(1, 0)]);
        // A seed two above is refused: (1, 0) keeps 1 and the walker goes west.
        let (route, _) = m.flood_route(&grass, &|_| 1, &[((1, 0), 0), ((1, 0), 2), ((3, 0), 1)], (2, 0)).unwrap();
        assert_eq!(route, vec![(1, 0)]);
    }

    #[test]
    fn the_flood_refuses_blocked_seeds_and_closed_cells() {
        let mut codes = vec![Surface::GrassPlain as u8; 25];
        codes[2 * 5 + 2] = Surface::DeepSea as u8;
        let m = TileMap::from_codes(Grid::Square8, 5, 5, &codes, vec![]);
        let cost = |t: Tile| m.cost(t).unwrap_or(0);
        assert!(m.flood_route(&cost, &|_| 1, &[((2, 2), 0)], (0, 2)).is_none(), "a seed on a blocked cell");
        // A wall of closed cells (mask 0) across column 2 cuts the way.
        let wall = |t: Tile| u16::from(t.0 != 2);
        assert!(m.flood_route(&cost, &wall, &[((4, 2), 0)], (0, 2)).is_none());
        let gap = |t: Tile| u16::from(t.0 != 2 || t.1 == 4);
        let (r, _) = m.flood_route(&cost, &gap, &[((4, 2), 0)], (0, 2)).unwrap();
        assert!(r.contains(&(2, 4)) && r.last() == Some(&(4, 2)));
    }

    #[test]
    fn cell_zero_is_priced_as_any_cell() {
        // The original's bug: its multiplying loop skipped cell (0, 0), so there the mask
        // itself was the cost (1 when open), and the corner pulled routes round it. Grass 5,
        // road 3, marsh 8, deep sea:
        //   G G G R
        //   G M G G
        //   G ~ G G
        use Surface::*;
        let codes = [GrassPlain, GrassPlain, GrassPlain, Road, GrassPlain, Marsh, GrassPlain, GrassPlain, GrassPlain, DeepSea, GrassPlain, GrassPlain].map(|s| s as u8);
        let m = TileMap::from_codes(Grid::Square8, 4, 3, &codes, vec![]);
        let cost = |t: Tile| m.cost(t).unwrap_or(0);
        // From (2, 0) to (0, 2): with (0, 0) at its real cost 5 the route crosses the marsh
        // (the bug's corner priced 1 pulled it round by the west column).
        let (route, _) = m.flood_route(&cost, &|_| 1, &[((0, 2), 0)], (2, 0)).unwrap();
        assert_ne!(route, vec![(1, 0), (0, 1), (0, 2)]);
        assert_eq!(route.last(), Some(&(0, 2)));
        // Closed by the mask, the corner is closed.
        let (route, _) = m.flood_route(&cost, &|t| u16::from(t != (0, 0)), &[((0, 2), 0)], (2, 0)).unwrap();
        assert_eq!(route, vec![(1, 1), (0, 2)]);
    }

    #[test]
    fn path_to_any_stops_at_the_nearest_goal_cell() {
        let m = TileMap::from_codes(Grid::Square8, 10, 1, &[Surface::GrassPlain as u8; 10], vec![]);
        let p = m.path_to_any((0, 0), &|t| t.0 >= 6, usize::MAX, &|_, n| m.cost(n));
        assert_eq!(p.last(), Some(&(6, 0)));
        assert!(m.path_to_any((7, 0), &|t| t.0 >= 6, usize::MAX, &|_, n| m.cost(n)).is_empty(), "already there");
    }

    #[test]
    fn block_pave_and_open_cells_and_flood_fill() {
        let mut m = TileMap::parse(MAP);
        m.open((1, 1), 3);
        assert_eq!(m.minutes((1, 1)), Some(15));
        m.block((0, 3));
        assert!(!m.passable((0, 3)));
        m.pave((2, 1));
        assert_eq!((m.cost((2, 1)), m.water_cost((2, 1))), (Some(ROAD), Some(ROAD)), "a footprint is road on both maps");
        let reach = m.reachable((0, 0));
        assert!(reach[m.mask_index((4, 4)).unwrap()]);
        assert!(!reach[m.mask_index((0, 3)).unwrap()]);
    }
}
