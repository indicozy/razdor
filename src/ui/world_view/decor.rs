//! Razdor's decorations («Украшения карты», `Settings::map_decor`, off by default; `RAZDOR_DECOR=1` is a dev override): the small sprites of the map's own kit (grass tufts, flowers,
//! reeds, bushes and dead shrubs) drawn over the land, as a map maker dresses a scene. They are draw-only: never written
//! into the map's objects, so they block nothing, change no cost map, no save, no minimap and no pick.
//!
//! The placement is a pure function of the cell: integer hashes and a low-frequency value noise, so a map gets the same
//! decorations every run. It is built once per map ([`for_map`]) and drawn by the world view's object pass (`draw_world_in`),
//! in the sort and under the fog of the map's own objects. Each decoration is an ordinary `Decoration` value, so its
//! picture, sway and 2.5D shading are the objects' own.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use razdor::local;
use razdor::rules::game::Game;
use razdor::rules::map::{is_water, object_cells, object_class, Decoration, Surface, Tile};

/// Grass tufts (class 9, passable, about 30×21 px): the meadows' cover.
const TUFTS: [u8; 6] = [48, 49, 50, 168, 169, 170];
/// Flowering sprouts (class 9, coloured specks), in clumps.
const FLOWERS: [u8; 5] = [180, 181, 182, 183, 184];
/// Shore sprouts and reeds (class 9).
const REEDS: [u8; 6] = [60, 61, 62, 63, 64, 65];
/// Small round bushes (class 9), used sparingly at forest edges. Sprites 12–16 are left out: they read as leafless trees.
const BUSHES: [u8; 5] = [2, 3, 4, 5, 6];
/// Dead shrubs (class 10) for dry, clay and scorched ground.
const DEAD: [u8; 4] = [109, 111, 115, 116];
/// At most this many decorations on one cell.
const PER_CELL: u32 = 3;
/// The chance of each of a meadow cell's three slots, relative to the cell's cover: the first is the most likely,
/// so a meadow's edge holds single tufts and its heart two or three.
const SLOT_WEIGHT: [f32; 3] = [1.0, 0.45, 0.2];

/// «Украшения карты» (`Settings::map_decor`): how many decorations the map takes. Off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DecorLevel {
    /// No decorations: the map as it always was.
    #[default]
    Off,
    /// «Мало»: the chance of each decoration is 0.6 of the full one.
    Few,
    /// «Обычно»: the full chance.
    Normal,
}

impl DecorLevel {
    /// The placement's density (see [`place`]): 0.6 for Few, 1.0 for Normal (and for Off, which places nothing).
    pub fn density(self) -> f32 {
        match self {
            DecorLevel::Few => 0.6,
            DecorLevel::Normal | DecorLevel::Off => 1.0,
        }
    }
}

/// What the ground under a cell is, for the decorations it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ground {
    /// Grass lowland or plain: tufts, flowers, bushes at the forest's edge.
    Meadow,
    /// Dry plain, clay, scorched land: dead shrubs.
    Dry,
    /// Any other walkable land (sand, stony, marsh, snow): reeds at the shore only.
    Land,
}

/// A cell that may take decorations, with its neighbourhood (a water or a tree next to it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ground: Ground,
    /// A water cell lies next to it.
    pub shore: bool,
    /// A tree of class 9 or 11 lies next to it.
    pub forest: bool,
}

/// One decoration of a placement (its cell, its sprite and its jitter, as `Decoration::jitter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instance {
    pub tile: Tile,
    pub class: u8,
    pub sprite: u8,
    pub jitter: (i32, i32, i32),
}

/// The cell's class: `None` where nothing may stand (water, a road, an impassable surface, or a cell that
/// `occupied` says holds an object, a building or an army).
pub fn classify(surface: Surface, occupied: bool, shore: bool, forest: bool) -> Option<Cell> {
    if occupied || is_water(surface) {
        return None;
    }
    let ground = match surface {
        Surface::Road | Surface::LavaFields | Surface::ImpassableSwamp | Surface::ImpassableSnowdrifts => return None,
        Surface::GrassLowland | Surface::GrassPlain => Ground::Meadow,
        Surface::DryPlain | Surface::ClaySoil | Surface::ScorchedLand => Ground::Dry,
        _ => Ground::Land,
    };
    Some(Cell { ground, shore, forest })
}

// Salts of the per-cell hashes: one base per use, and a slot number `k` for each of the cell's decorations.
const S_COVER: u64 = 1;
const S_FLOWER_PICK: u64 = 2;
const S_SPRITE: u64 = 3;
const S_JIT_X: u64 = 4;
const S_JIT_Y: u64 = 5;
const S_PHASE: u64 = 6;
const S_REED: u64 = 7;
const S_BUSH: u64 = 8;
const S_DEAD: u64 = 9;
// The value noises' salts (their lattice corners).
const N_MEADOW: u64 = 101;
const N_FLOWER: u64 = 102;
const N_DRY: u64 = 103;
const N_REED: u64 = 104;

fn salt(base: u64, k: u64) -> u64 {
    base * 16 + k
}

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    h
}

/// An integer hash of a cell, the seed and a salt, in 0..2^64.
fn hash(seed: u64, x: i32, y: i32, salt: u64) -> u64 {
    mix(seed ^ mix(((x as u32 as u64) << 32) | y as u32 as u64) ^ salt.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

/// A hash as a number in [0, 1).
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn smooth_step(e0: f32, e1: f32, x: f32) -> f32 {
    smooth(((x - e0) / (e1 - e0)).clamp(0.0, 1.0))
}

/// Value noise in [0, 1] at `(x, y)` with a lattice of `scale` cells: low frequency patches, no float hash.
fn noise(seed: u64, x: i32, y: i32, scale: f32, salt: u64) -> f32 {
    // Off the lattice's lines: a cell column on a line would take the lattice's own values (a straight band of cover).
    let fx = (x as f32 + 0.37) / scale;
    let fy = (y as f32 + 0.61) / scale;
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (smooth(fx - ix as f32), smooth(fy - iy as f32));
    let v = |dx: i32, dy: i32| unit(hash(seed, ix + dx, iy + dy, salt));
    let top = v(0, 0) + (v(1, 0) - v(0, 0)) * tx;
    let bottom = v(0, 1) + (v(1, 1) - v(0, 1)) * tx;
    top + (bottom - top) * ty
}

fn pick(u: f32, len: usize) -> usize {
    ((u * len as f32) as usize).min(len - 1)
}

/// The placement over a `w`×`h` map. `cell` gives each cell's class (`None` where nothing may stand); `density` scales
/// the chance of each decoration (1.0 by default; 0 places nothing). Up to [`PER_CELL`] decorations a cell, jittered
/// inside it; the meadows, the dry ground and the bushes come in patches (the noises), and the flowers in clumps.
/// Pure: the same inputs give the same list, in row-major order.
pub fn place(w: i32, h: i32, seed: u64, density: f32, cell: &dyn Fn(Tile) -> Option<Cell>) -> Vec<Instance> {
    let mut out = Vec::new();
    if density <= 0.0 {
        return out;
    }
    let reed = reed_cells(w, h, seed, density, cell);
    for y in 0..h {
        for x in 0..w {
            let Some(c) = cell((x, y)) else { continue };
            let at = |base: u64, k: u64| unit(hash(seed, x, y, salt(base, k)));
            let mut n = 0u32;
            // One decoration: its sprite (or its class and the jitter drawn from the cell's hashes, slot `k`).
            let put = |out: &mut Vec<Instance>, n: &mut u32, class: u8, sprite: u8, k: u64, sway: bool| {
                let jitter = if sway {
                    (6 + (at(S_JIT_X, k) * 20.0) as i32, (at(S_JIT_Y, k) * 11.0) as i32, (at(S_PHASE, k) * 1000.0) as i32)
                } else {
                    (0, 0, 0)
                };
                out.push(Instance { tile: (x, y), class, sprite, jitter });
                *n += 1;
            };
            if c.ground == Ground::Meadow {
                let cover = (smooth_step(0.5, 0.8, noise(seed, x, y, 5.0, N_MEADOW)) * 0.7 * density).min(1.0);
                let clumps = noise(seed, x, y, 2.0, N_FLOWER) > 0.6;
                for k in 0..3 {
                    if n == PER_CELL {
                        break;
                    }
                    if at(S_COVER, k) >= (cover * SLOT_WEIGHT[k as usize]).min(1.0) {
                        continue;
                    }
                    let sprite = if clumps && at(S_FLOWER_PICK, k) < 0.75 {
                        FLOWERS[pick(at(S_SPRITE, k), FLOWERS.len())]
                    } else {
                        TUFTS[pick(at(S_SPRITE, k), TUFTS.len())]
                    };
                    put(&mut out, &mut n, object_class::TREES, sprite, k, true);
                }
            }
            if c.ground == Ground::Dry && n < PER_CELL {
                let cover = (smooth_step(0.5, 0.75, noise(seed, x, y, 4.0, N_DRY)) * 0.35 * density).min(1.0);
                if at(S_DEAD, 0) < cover {
                    // A dead shrub (class 10) has no jitter in the engine: it stands on its cell's centre.
                    put(&mut out, &mut n, object_class::DEAD_TREES, DEAD[pick(at(S_SPRITE, 7), DEAD.len())], 0, false);
                }
            }
            // Reeds stand only in a cluster along the water (`reed_cells`, `reed_cluster`): never a single reed.
            if reed[(y * w + x) as usize] && reed_cluster(&reed, w, h, x, y) {
                for k in 0..2 {
                    if n == PER_CELL {
                        break;
                    }
                    if k == 0 || at(S_REED, k) < 0.5 {
                        put(&mut out, &mut n, object_class::TREES, REEDS[pick(at(S_SPRITE, k + 4), REEDS.len())], k, true);
                    }
                }
            }
            if c.forest && c.ground != Ground::Dry && n < PER_CELL && at(S_BUSH, 0) < (0.03 * density).min(1.0) {
                put(&mut out, &mut n, object_class::TREES, BUSHES[pick(at(S_SPRITE, 8), BUSHES.len())], 0, true);
            }
        }
    }
    out
}

/// The cells that may hold reeds: a shore cell where the low-frequency noise is high (patches a few cells wide) and a
/// hash passes (0.8 of the density). A reed is drawn only on such a cell with at least two such neighbours
/// (`reed_cluster`), so reeds stand in clusters along the water.
fn reed_cells(w: i32, h: i32, seed: u64, density: f32, cell: &dyn Fn(Tile) -> Option<Cell>) -> Vec<bool> {
    let mut out = vec![false; (w.max(0) * h.max(0)) as usize];
    for y in 0..h {
        for x in 0..w {
            let Some(c) = cell((x, y)) else { continue };
            out[(y * w + x) as usize] = c.shore && noise(seed, x, y, 3.0, N_REED) > 0.5 && unit(hash(seed, x, y, salt(S_REED, 0))) < (0.8 * density).min(1.0);
        }
    }
    out
}

/// Whether a cell has at least two reed candidates among its eight neighbours.
fn reed_cluster(reed: &[bool], w: i32, h: i32, x: i32, y: i32) -> bool {
    let mut n = 0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (nx, ny) = (x + dx, y + dy);
            if (dx, dy) != (0, 0) && (0..w).contains(&nx) && (0..h).contains(&ny) && reed[(ny * w + nx) as usize] {
                n += 1;
            }
        }
    }
    n >= 2
}

/// The cells `tiles` cover and their eight neighbours: where no decoration may stand (a road, a building's footprint,
/// a bridge's). Cells outside the map are left out.
pub fn blocked(w: i32, h: i32, tiles: impl Iterator<Item = Tile>) -> Vec<bool> {
    let mut out = vec![false; (w.max(0) * h.max(0)) as usize];
    for (x, y) in tiles {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if (0..w).contains(&nx) && (0..h).contains(&ny) {
                    out[(ny * w + nx) as usize] = true;
                }
            }
        }
    }
    out
}

/// The placement's seed: a hash of the map's size and its terrain, so the same map always gets the same decorations.
fn seed_of(w: i32, h: i32, codes: &[u8]) -> u64 {
    codes.iter().fold(0xcbf2_9ce4_8422_2325 ^ ((w as u64) << 32) ^ h as u64, |h, &c| (h ^ u64::from(c)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The decorations of `game`'s map at `density`, sorted by row then column (as the map's objects are).
fn build(game: &Game, density: f32) -> Vec<Decoration> {
    let map = &game.world.map;
    let (w, h) = (map.w, map.h);
    let index = |(x, y): Tile| (x >= 0 && y >= 0 && x < w && y < h).then(|| (y * w + x) as usize);
    let cells = (w * h).max(0) as usize;
    // What stands on a cell: a map object's cells (a hill's its whole square), a building's footprint, an army, the hero.
    let mut occupied = vec![false; cells];
    let mut trees = vec![false; cells];
    for o in &map.objects {
        for t in object_cells(o) {
            if let Some(i) = index(t) {
                occupied[i] = true;
            }
        }
        if matches!(o.class, object_class::TREES | object_class::THICKET) {
            if let Some(i) = index(o.tile) {
                trees[i] = true;
            }
        }
    }
    for l in &game.world.locations {
        for t in l.cells() {
            if let Some(i) = index(t) {
                occupied[i] = true;
            }
        }
    }
    for a in &game.world.armies {
        if let Some(i) = index(a.tile(map)) {
            occupied[i] = true;
        }
    }
    if let Some(i) = index(game.tile()) {
        occupied[i] = true;
    }
    let mut water = vec![false; cells];
    for y in 0..h {
        for x in 0..w {
            water[(y * w + x) as usize] = is_water(map.surface((x, y)));
        }
    }
    // The neighbours of a cell (the map's grid: six on a hex map, eight on a square one).
    let near = |t: Tile, flags: &[bool]| map.grid.neighbours(t).any(|n| index(n).is_some_and(|i| flags[i]));
    let seed = seed_of(w, h, map.surface_codes());
    // Roads and building footprints (a bridge is a location too), with their neighbours: no decoration there.
    let roads = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&t| map.surface(t) == Surface::Road);
    let edge = blocked(w, h, roads.chain(game.world.locations.iter().flat_map(|l| l.cells())));
    let cell = |t: Tile| {
        let i = index(t)?;
        if edge[i] {
            return None;
        }
        let surface = map.surface(t);
        classify(surface, occupied[i], near(t, &water), near(t, &trees))
    };
    place(w, h, seed, density, &cell)
        .into_iter()
        .map(|d| Decoration { tile: d.tile, class: d.class, sprite: d.sprite, jitter: d.jitter })
        .collect()
}

// Switched by env in `display::follow_settings` only (`RAZDOR_DECOR`, `RAZDOR_DECOR_DENSITY`); off by default.
static ON: AtomicBool = AtomicBool::new(false);
/// The density's bits (`f32`), 1.0 by default.
static DENSITY: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// The switch and the density (clamped to 0..=3).
pub fn set_pref(on: bool, density: f32) {
    ON.store(on, Ordering::Relaxed);
    DENSITY.store(density.clamp(0.0, 3.0).to_bits(), Ordering::Relaxed);
}

fn density() -> f32 {
    f32::from_bits(DENSITY.load(Ordering::Relaxed))
}

/// What the cached list was made from: the map (its identity and its objects'), its buildings' count, its size and the density.
#[derive(PartialEq)]
struct Key {
    map: usize,
    objects: usize,
    objects_at: usize,
    locations: usize,
    w: i32,
    h: i32,
    density: u32,
}

local! {
    /// The decorations of the last map, kept while its key holds (built once per map, not per frame).
    static LIST: std::cell::RefCell<Option<(Key, Rc<Vec<Decoration>>)>> = const { std::cell::RefCell::new(None) };
}

/// The decorations of `game`'s map, or None when the switch is off (nothing is built or drawn then).
pub fn for_map(game: &Game) -> Option<Rc<Vec<Decoration>>> {
    if !ON.load(Ordering::Relaxed) {
        return None;
    }
    let map = &game.world.map;
    let density = density();
    let key = Key {
        map: std::ptr::from_ref(map) as usize,
        objects: map.objects.len(),
        objects_at: map.objects.as_ptr() as usize,
        locations: game.world.locations.len(),
        w: map.w,
        h: map.h,
        density: density.to_bits(),
    };
    LIST.with_borrow_mut(|c| {
        if let Some((k, list)) = c.as_ref() {
            if *k == key {
                return Some(list.clone());
            }
        }
        let list = Rc::new(build(game, density));
        *c = Some((key, list.clone()));
        Some(list)
    })
}

/// The indices of the decorations standing in rows `r0..r1`, in the list's order (the list is sorted by row).
pub fn rows(list: &[Decoration], r0: i32, r1: i32) -> std::ops::Range<usize> {
    let lo = list.partition_point(|o| o.tile.1 < r0);
    let hi = list.partition_point(|o| o.tile.1 < r1);
    lo..hi.max(lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A 40×30 map: a deep-sea row, a road down column 20, a lake, a dry third on the right, and a scatter of objects.
    fn surface((x, y): Tile) -> Surface {
        if y == 0 {
            Surface::DeepSea
        } else if x == 20 {
            Surface::Road
        } else if (3..6).contains(&x) && (10..14).contains(&y) {
            Surface::ShallowsFords
        } else if x >= 30 {
            Surface::DryPlain
        } else {
            Surface::GrassLowland
        }
    }

    fn occupied((x, y): Tile) -> bool {
        (x * 7 + y * 3) % 11 == 0
    }

    fn near_water((x, y): Tile) -> bool {
        [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)].iter().any(|&n| (0..40).contains(&n.0) && (0..30).contains(&n.1) && is_water(surface(n)))
    }

    fn cell(t: Tile) -> Option<Cell> {
        classify(surface(t), occupied(t), near_water(t), false)
    }

    #[test]
    fn decor_placement_is_deterministic_and_avoids_occupied_road_and_water_cells() {
        let a = place(40, 30, 7, 1.6, &cell);
        assert_eq!(a, place(40, 30, 7, 1.6, &cell), "the same inputs give the same placement");
        assert!(a.len() > 100, "a dressed map has many decorations, got {}", a.len());
        for d in &a {
            let t = d.tile;
            assert!(!occupied(t), "decoration on an occupied cell {t:?}");
            assert_ne!(surface(t), Surface::Road, "decoration on a road {t:?}");
            assert!(!is_water(surface(t)), "decoration on water {t:?}");
            assert!((0..40).contains(&t.0) && (0..30).contains(&t.1), "decoration outside the map {t:?}");
        }
        let mut per_cell: HashMap<Tile, u32> = HashMap::new();
        for d in &a {
            *per_cell.entry(d.tile).or_default() += 1;
        }
        assert!(per_cell.values().all(|&n| n <= PER_CELL), "more than {PER_CELL} decorations on a cell");
        // Each kind lands where its ground is: reeds next to the lake, dead shrubs on the dry third.
        assert!(a.iter().any(|d| d.class == object_class::TREES && REEDS.contains(&d.sprite)), "no reeds at the shore");
        assert!(a.iter().any(|d| d.class == object_class::DEAD_TREES && x_of(d) >= 30), "no dead shrubs on the dry ground");
        assert!(a.iter().filter(|d| d.class == object_class::DEAD_TREES).all(|d| x_of(d) >= 30), "dead shrubs off the dry ground");
        assert!(place(40, 30, 7, 0.0, &cell).is_empty(), "density 0 places nothing");
    }

    fn x_of(d: &Instance) -> i32 {
        d.tile.0
    }

    #[test]
    fn decor_placement_follows_the_seed() {
        let a = place(40, 30, 7, 1.0, &cell);
        let b = place(40, 30, 8, 1.0, &cell);
        assert_ne!(a, b, "another seed gives another placement");
    }

    #[test]
    fn blocked_marks_the_cells_and_their_neighbours_inside_the_map() {
        let b = blocked(5, 4, [(0, 0), (4, 3)].into_iter());
        let at = |x: usize, y: usize| b[y * 5 + x];
        assert!(at(0, 0) && at(1, 0) && at(0, 1) && at(1, 1), "a cell and its eight neighbours");
        assert!(!at(2, 0) && !at(0, 2), "the cells two steps away are free");
        assert!(at(3, 2) && at(4, 2) && at(3, 3) && at(4, 3), "the corner's neighbours, inside the map");
        assert_eq!(b.iter().filter(|&&v| v).count(), 8, "the corners do not spill out of the map");
    }

    #[test]
    fn no_decoration_next_to_a_road_or_a_building_or_a_bridge() {
        // A road down column 20 (the test map's), a building's 2×2 footprint at (30..31, 5..6) and a bridge at (10, 20).
        let road = (0..30).map(|y| (20, y));
        let footprints = [(30, 5), (31, 5), (30, 6), (31, 6), (10, 20)];
        let edge = blocked(40, 30, road.chain(footprints));
        let cell = |t: Tile| if edge[(t.1 * 40 + t.0) as usize] { None } else { classify(surface(t), occupied(t), near_water(t), false) };
        let a = place(40, 30, 7, 1.6, &cell);
        assert!(a.len() > 100, "the rest of the map is still dressed, got {}", a.len());
        for d in &a {
            let (x, y) = d.tile;
            assert!((x - 20).abs() > 1, "decoration next to the road {d:?}");
            assert!(!(29..=32).contains(&x) || !(4..=7).contains(&y), "decoration next to the building {d:?}");
            assert!(!(9..=11).contains(&x) || !(19..=21).contains(&y), "decoration next to the bridge {d:?}");
        }
    }

    #[test]
    fn reeds_stand_only_in_clusters_along_the_water() {
        let a = place(40, 30, 7, 1.0, &cell);
        let tiles: Vec<Tile> = a.iter().filter(|d| d.class == object_class::TREES && REEDS.contains(&d.sprite)).map(|d| d.tile).collect();
        assert!(!tiles.is_empty(), "the lake has reeds");
        // Every reed's cell is a shore cell with at least two reed candidates among its neighbours.
        let candidates = reed_cells(40, 30, 7, 1.0, &cell);
        for &(x, y) in &tiles {
            assert!(candidates[(y * 40 + x) as usize], "a reed off the candidates {x},{y}");
            assert!(reed_cluster(&candidates, 40, 30, x, y), "a single reed {x},{y}");
        }
    }

    #[test]
    fn bushes_are_rare_and_never_the_leafless_sprites() {
        // A wood of class-9 trees on meadow: bushes come only from the round sprites, and rarely.
        let forest = |_: Tile| classify(Surface::GrassPlain, false, false, true);
        let a = place(60, 60, 3, 1.0, &forest);
        let bushes: Vec<&Instance> = a.iter().filter(|d| d.class == object_class::TREES && BUSHES.contains(&d.sprite)).collect();
        assert!(bushes.iter().all(|d| (2..=6).contains(&d.sprite)), "only the round bushes");
        assert!(bushes.len() < 60 * 60 / 10, "bushes are rare, got {}", bushes.len());
    }
}
