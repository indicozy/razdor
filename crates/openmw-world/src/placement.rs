//! Placement (spec §3.6, §3.7): the references of the scenery, the buildings and the bridges, each
//! with the STAT mesh it uses. Positions are in units; each reference goes into the cell its
//! position is in. Everything comes from integer arithmetic, the hash of `keyed` and the
//! 16-entry yaw table, so two runs give the same plan.
//!
//! A building is one reference (spec §3.7, one STAT reference per building): its mesh holds all
//! its parts (the town's houses, the castle's walls and towers). Trees, dead trees and thickets
//! get one reference each, with a jitter inside their tile (spec §3.6); a stone sits on its
//! massif's centre; a mountain gets one rock at its foot, inside its own square.

use crate::grid::{Grid, Rect, CELL_UNITS, VERT_UNITS};
use crate::heightmap::{self, keyed, HeightField};
use crate::input::Map;
use crate::mesh::{self, Builder};
use std::collections::BTreeMap;
use std::f64::consts::PI;

/// Hash purposes (spec §4: one stream per purpose, keyed by the seed and the coordinates).
const PURPOSE_OBJECT: u64 = 40;
const PURPOSE_TOWN: u64 = 41;

/// The mesh of a reference: its file name under `meshes/razdor/` (the STAT id is `razdor_<name>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshFile {
    pub name: String,
    pub dae: String,
    /// The mesh's bounding box in its own units: `[x0, x1, y0, y1]`.
    pub extent: [i64; 4],
}

/// One reference: the index of its mesh in [`Plan::meshes`], its position and its yaw (radians).
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub mesh: usize,
    pub pos: [i64; 3],
    pub yaw: f64,
}

/// Everything M3 writes: the meshes (in the order they were first used) and the references of
/// every cell that has one, cells in order.
#[derive(Debug, Default)]
pub struct Plan {
    pub meshes: Vec<MeshFile>,
    pub cells: BTreeMap<(i64, i64), Vec<Placed>>,
    /// The footprint of every building in world units, `[x0, x1, y0, y1]`: its mesh's extent at
    /// its position (M4: no army body is spawned inside one, spec §3.8). Bridges are not boxes.
    pub boxes: Vec<[i64; 4]>,
}

/// The mesh files, made once per name.
#[derive(Default)]
struct Catalog {
    files: Vec<MeshFile>,
    index: BTreeMap<String, usize>,
}

impl Catalog {
    fn get(&mut self, name: &str, make: impl FnOnce() -> Builder) -> usize {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        let b = make();
        let (dae, extent) = (b.dae(name), b.extent());
        let i = self.files.len();
        self.files.push(MeshFile { name: name.to_string(), dae, extent });
        self.index.insert(name.to_string(), i);
        i
    }
}

/// The yaw of table entry `k` (0..16): `k · 22.5°`, from `π` and integer steps only.
pub fn yaw(k: u64) -> f64 {
    PI * k as f64 / 8.0
}

/// The height of the ground at a world point: bilinear over the land vertices (heights in units).
pub fn ground(h: &HeightField, x: i64, y: i64) -> i64 {
    let (i, fx) = (x.div_euclid(VERT_UNITS), x.rem_euclid(VERT_UNITS));
    let (j, fy) = (y.div_euclid(VERT_UNITS), y.rem_euclid(VERT_UNITS));
    let v = |a: i64, b: i64| h.height(a, b) as i64;
    let low = v(i, j) * (VERT_UNITS - fx) + v(i + 1, j) * fx;
    let high = v(i, j + 1) * (VERT_UNITS - fx) + v(i + 1, j + 1) * fx;
    (low * (VERT_UNITS - fy) + high * fy) / (VERT_UNITS * VERT_UNITS)
}

/// The cell a world point is in.
pub fn cell_of(x: i64, y: i64) -> (i64, i64) {
    (x.div_euclid(CELL_UNITS), y.div_euclid(CELL_UNITS))
}

/// Slot centres for the houses of a village or a town of footprint `w` by `d` (relative to the
/// footprint's centre), row by row from the north. A house is 512 by 640 and a slot is 768 by 896
/// apart, so two houses never touch and a street of 256 runs between them. The slots lie inside
/// the footprint plus half a tile, which is inside the flat ground of §3.3 (footprint plus a tile).
pub fn slot_grid(w: i64, d: i64) -> Vec<(i64, i64)> {
    let ncol = (w / 2 + 256) / 384 + 1;
    let nrow = (d / 2 + 192) / 448 + 1;
    let mut out = Vec::new();
    for r in (0..nrow).rev() {
        for c in 0..ncol {
            out.push((384 * (2 * c - (ncol - 1)), 448 * (2 * r - (nrow - 1))));
        }
    }
    out
}

/// The houses of a building: `min..=max` of them (a town 5 to 9, a village 3 to 5), chosen from
/// the slot grid by the hash of the seed and the building's index, kept in grid order. A footprint
/// with fewer slots than the count gets all of them.
pub fn town_layout(w: i64, d: i64, min: u64, max: u64, seed: u64, building: usize) -> Vec<(i64, i64)> {
    let slots = slot_grid(w, d);
    let span = max - min + 1;
    let want = (min + keyed(&[seed, PURPOSE_TOWN, building as u64]) % span) as usize;
    let mut order: Vec<(u64, usize)> =
        (0..slots.len()).map(|k| (keyed(&[seed, PURPOSE_TOWN, building as u64, k as u64 + 1]), k)).collect();
    order.sort();
    let mut chosen: Vec<usize> = order.iter().take(want.min(slots.len())).map(|&(_, k)| k).collect();
    chosen.sort();
    chosen.into_iter().map(|k| slots[k]).collect()
}

/// The slug of a building kind (the mesh names), or `None` for the bridges (kinds 13 and 14).
fn slug(kind: i64) -> String {
    match kind {
        0 => "palace".into(),
        1 => "town".into(),
        2 => "village".into(),
        3 => "castle".into(),
        4 => "fort".into(),
        5 => "tavern".into(),
        6 => "market".into(),
        7 => "church".into(),
        8 => "smithy".into(),
        9 => "shipyard".into(),
        10 => "altar".into(),
        11 => "dungeon".into(),
        12 => "ruins".into(),
        15 => "obelisk".into(),
        other => format!("kind{other}"),
    }
}

/// The mesh of a building of `kind`, `w` by `d` units, for the houses at `slots` (towns, villages).
fn building_mesh(kind: i64, w: i64, d: i64, slots: &[(i64, i64)]) -> Builder {
    match kind {
        0 => mesh::palace(w, d),
        1 | 2 => mesh::houses(slots),
        3 => mesh::castle(w, d),
        4 => mesh::fort(w, d),
        5 => mesh::tavern(w, d),
        6 => mesh::market(w, d),
        7 => mesh::church(w, d),
        8 => mesh::smithy(w, d),
        9 => mesh::shipyard(w, d),
        10 => mesh::altar(w, d),
        11 => mesh::dungeon(w, d),
        12 => mesh::ruins(w, d),
        15 => mesh::obelisk(w, d),
        _ => mesh::shed(w, d),
    }
}

/// Plans every reference of the map. `heights` is the land M2 made for the same map and grid.
pub fn plan(map: &Map, grid: &Grid, heights: &HeightField) -> Plan {
    let seed = map.info.generator_seed;
    let mut cat = Catalog::default();
    let mut cells: BTreeMap<(i64, i64), Vec<Placed>> = BTreeMap::new();
    let mut boxes = Vec::new();
    let mut add = |mesh: usize, pos: [i64; 3], yaw: f64| {
        cells.entry(cell_of(pos[0], pos[1])).or_default().push(Placed { mesh, pos, yaw });
    };

    for o in map.kept_objects() {
        let (cx, cy) = grid.tile_centre(o.x, o.y);
        let key = |n: u64| keyed(&[seed, PURPOSE_OBJECT, o.x as u64, o.y as u64, n]);
        match o.class {
            9 | 10 => {
                let name = if o.class == 9 {
                    if key(0) % 2 == 0 { "tree_a" } else { "tree_b" }
                } else {
                    "dead_tree"
                };
                let jx = (key(1) % 513) as i64 - 256;
                let jy = (key(2) % 513) as i64 - 256;
                let (x, y) = (cx + jx, cy + jy);
                let m = cat.get(name, || {
                    if o.class == 9 {
                        mesh::tree(if name == "tree_a" { 0 } else { 1 })
                    } else {
                        mesh::dead_tree()
                    }
                });
                add(m, [x, y, ground(heights, x, y)], yaw(key(3) % 16));
            }
            11 => {
                let m = cat.get("thicket", mesh::thicket);
                add(m, [cx, cy, ground(heights, cx, cy)], 0.0);
            }
            8 => {
                let f = (o.sprite / 10).clamp(1, 6);
                let m = cat.get(&format!("stone_{f}"), || mesh::stone(f));
                add(m, [cx, cy, ground(heights, cx, cy)], 0.0);
            }
            5..=7 => {
                // One rock at the foot of the massif: the middle of its south edge, inside its square.
                let f = (o.sprite / 10).max(1);
                let rect: Rect = grid.footprint(o.x, o.y, f, f);
                let x = (rect.west + rect.east) / 2;
                let y = rect.south + mesh::rock_diameter(f) / 2 + 32;
                let m = cat.get(&format!("rock_{}", f.clamp(1, 6)), || mesh::rock(f));
                add(m, [x, y, ground(heights, x, y)], 0.0);
            }
            _ => {} // hills are terrain only (spec §3.6)
        }
    }

    for (bi, b) in map.buildings.iter().enumerate() {
        let (x, y) = grid.tile_centre(b.x, b.y);
        if b.kind == 13 || b.kind == 14 {
            let wood = b.kind == 14;
            let name = if wood { "deck_wood" } else { "deck_stone" };
            let m = cat.get(name, || mesh::deck(wood));
            // The deck's top sits at the deck height; the land next to it was raised to match (§3.3).
            add(m, [x, y, heightmap::DECK_HEIGHT], 0.0);
            continue;
        }
        let rect = grid.footprint(b.x, b.y, b.size_x, b.size_y);
        let (w, d) = (rect.east - rect.west, rect.north - rect.south);
        let (cx, cy) = ((rect.west + rect.east) / 2, (rect.south + rect.north) / 2);
        let kind = slug(b.kind);
        let (name, slots) = match b.kind {
            1 => (format!("{kind}_{bi}"), town_layout(w, d, 5, 9, seed, bi)),
            2 => (format!("{kind}_{bi}"), town_layout(w, d, 3, 5, seed, bi)),
            _ => (format!("{kind}_{w}x{d}"), Vec::new()),
        };
        let m = cat.get(&name, || building_mesh(b.kind, w, d, &slots));
        let e = cat.files[m].extent;
        boxes.push([cx + e[0], cx + e[1], cy + e[2], cy + e[3]]);
        add(m, [cx, cy, ground(heights, cx, cy)], 0.0);
    }

    Plan { meshes: cat.files, cells, boxes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Grid, Rect, VERT_UNITS};
    use crate::heightmap::Terrain;
    use crate::input::synthetic;
    use crate::mesh::check;

    /// The plan of a synthetic map of `w` by `h` tiles with the given objects and buildings.
    fn plan_of(w: usize, h: usize, codes: &[u8], objects: serde_json::Value, buildings: serde_json::Value) -> (Map, Grid, Plan) {
        let map = synthetic(w, h, codes, objects, buildings);
        let grid = Grid::new(w, h, 1024).unwrap();
        let terrain = Terrain::new(grid, map.tile_codes().unwrap());
        let heights = heightmap::build(&terrain, &map);
        let p = plan(&map, &grid, &heights);
        (map, grid, p)
    }

    fn grass(n: usize) -> Vec<u8> {
        vec![6; n]
    }

    fn name_of<'a>(p: &'a Plan, r: &Placed) -> &'a str {
        &p.meshes[r.mesh].name
    }

    #[test]
    fn one_reference_per_kept_tree_dead_tree_thicket_and_stone() {
        let objects = serde_json::json!([
            { "x": 2, "y": 2, "sprite": 40, "class": 9 },
            { "x": 3, "y": 3, "sprite": 41, "class": 9 },
            { "x": 4, "y": 4, "sprite": 42, "class": 10 },
            { "x": 5, "y": 5, "sprite": 43, "class": 11 },
            { "x": 6, "y": 6, "sprite": 30, "class": 8 },
            { "x": 9, "y": 9, "sprite": 40, "class": 9 },
            { "x": 9, "y": 9, "sprite": 42, "class": 10 },  // the later plant of the tile wins
            { "x": 12, "y": 12, "sprite": 10, "class": 1 }  // a hill: terrain only
        ]);
        let (_, _, p) = plan_of(20, 20, &grass(400), objects, serde_json::json!([]));
        let refs: Vec<&Placed> = p.cells.values().flatten().collect();
        let count = |f: &dyn Fn(&str) -> bool| refs.iter().filter(|r| f(name_of(&p, r))).count();
        assert_eq!(count(&|n| n.starts_with("tree_")), 2);
        assert_eq!(count(&|n| n == "dead_tree"), 2, "one dead tree at (4,4), one at (9,9)");
        assert_eq!(count(&|n| n == "thicket"), 1);
        assert_eq!(count(&|n| n.starts_with("stone_")), 1);
        assert_eq!(refs.len(), 6, "a hill adds nothing");
    }

    #[test]
    fn a_mountain_gets_one_rock_inside_its_own_square() {
        let objects = serde_json::json!([{ "x": 10, "y": 10, "sprite": 20, "class": 5 }]);
        let (_, grid, p) = plan_of(20, 20, &grass(400), objects, serde_json::json!([]));
        let refs: Vec<&Placed> = p.cells.values().flatten().collect();
        assert_eq!(refs.len(), 1);
        assert_eq!(name_of(&p, refs[0]), "rock_2");
        let square = grid.footprint(10, 10, 2, 2);
        let [x, y, _] = refs[0].pos;
        assert!(x > square.west && x < square.east && y > square.south && y < square.north);
    }

    #[test]
    fn every_reference_lies_inside_its_cell() {
        let objects = serde_json::json!([
            { "x": 0, "y": 0, "sprite": 40, "class": 9 },
            { "x": 19, "y": 19, "sprite": 40, "class": 9 },
            { "x": 7, "y": 8, "sprite": 40, "class": 10 },
            { "x": 8, "y": 7, "sprite": 40, "class": 11 }
        ]);
        let buildings = serde_json::json!([
            { "x": 15, "y": 4, "size_x": 4, "size_y": 4, "kind": 3 },
            { "x": 3, "y": 16, "size_x": 2, "size_y": 2, "kind": 1 }
        ]);
        let (_, _, p) = plan_of(20, 20, &grass(400), objects, buildings);
        assert!(!p.cells.is_empty());
        for (&(cx, cy), refs) in &p.cells {
            for r in refs {
                assert_eq!(cell_of(r.pos[0], r.pos[1]), (cx, cy));
                assert!(r.pos[0] >= cx * CELL_UNITS && r.pos[0] < (cx + 1) * CELL_UNITS);
                assert!(r.pos[1] >= cy * CELL_UNITS && r.pos[1] < (cy + 1) * CELL_UNITS);
            }
        }
    }

    #[test]
    fn a_building_is_one_reference_and_its_mesh_is_well_formed() {
        let buildings = serde_json::json!([
            { "x": 10, "y": 10, "size_x": 4, "size_y": 4, "kind": 3 },
            { "x": 4, "y": 4, "size_x": 2, "size_y": 2, "kind": 2 },
            { "x": 16, "y": 16, "size_x": 1, "size_y": 1, "kind": 7 }
        ]);
        let (_, _, p) = plan_of(20, 20, &grass(400), serde_json::json!([]), buildings);
        let refs: Vec<&Placed> = p.cells.values().flatten().collect();
        assert_eq!(refs.len(), 3);
        let names: Vec<&str> = refs.iter().map(|r| name_of(&p, r)).collect();
        assert!(names.contains(&"castle_4096x4096"), "{names:?}");
        assert!(names.contains(&"village_1"), "{names:?}");
        assert!(names.contains(&"church_1024x1024"), "{names:?}");
        for f in &p.meshes {
            check::well_formed(&f.dae).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        }
    }

    #[test]
    fn town_houses_never_overlap_and_the_count_is_5_to_9() {
        for seed in 0..40u64 {
            for &(w, d) in &[(1024i64, 1024i64), (2048, 2048), (2048, 1024), (4096, 4096)] {
                let slots = town_layout(w, d, 5, 9, seed, 3);
                let fits = slot_grid(w, d).len();
                assert!(slots.len() >= 5.min(fits) && slots.len() <= 9, "{w}x{d}: {}", slots.len());
                for (a, sa) in slots.iter().enumerate() {
                    for sb in slots.iter().skip(a + 1) {
                        let (dx, dy) = ((sa.0 - sb.0).abs(), (sa.1 - sb.1).abs());
                        assert!(dx >= 512 || dy >= 640, "houses {sa:?} and {sb:?} overlap");
                    }
                    // a house stays inside the footprint plus half a tile
                    assert!(sa.0.abs() + 256 <= w / 2 + 512 && sa.1.abs() + 320 <= d / 2 + 512, "{sa:?} out of {w}x{d}");
                }
            }
        }
    }

    #[test]
    fn a_village_has_3_to_5_houses_on_a_footprint_that_fits_them() {
        for seed in 0..40u64 {
            let n = town_layout(2048, 2048, 3, 5, seed, 0).len();
            assert!((3..=5).contains(&n), "{n}");
        }
    }

    #[test]
    fn a_diagonal_chain_of_three_bridge_tiles_has_overlapping_decks_and_land_at_deck_height() {
        // Water everywhere, with land on the far side of each end of the chain. The three stone
        // bridges are 1x1 buildings on a diagonal: (5,14), (6,13), (7,12) (row 0 is the north).
        let (w, h) = (20usize, 20usize);
        let mut codes = vec![2u8; w * h];
        codes[14 * w + 4] = 6; // grass west of the south-west end
        codes[12 * w + 8] = 6; // grass east of the north-east end
        let buildings = serde_json::json!([
            { "x": 5, "y": 14, "size_x": 1, "size_y": 1, "kind": 13 },
            { "x": 6, "y": 13, "size_x": 1, "size_y": 1, "kind": 13 },
            { "x": 7, "y": 12, "size_x": 1, "size_y": 1, "kind": 13 }
        ]);
        let map = synthetic(w, h, &codes, serde_json::json!([]), buildings);
        let grid = Grid::new(w, h, 1024).unwrap();
        let terrain = Terrain::new(grid, map.tile_codes().unwrap());
        let heights = heightmap::build(&terrain, &map);
        let p = plan(&map, &grid, &heights);

        let decks: Vec<[i64; 3]> = p.cells.values().flatten().map(|r| r.pos).collect();
        assert_eq!(decks.len(), 3);
        for d in &decks {
            assert_eq!(d[2], heightmap::DECK_HEIGHT);
        }
        // A deck is 1280 square (the tile plus 128 each side). Neighbours on the chain are a
        // diagonal tile apart, so their decks overlap in a 256 by 256 patch the walker can use.
        let overlap = |a: usize, b: usize| {
            let (dx, dy) = ((decks[a][0] - decks[b][0]).abs(), (decks[a][1] - decks[b][1]).abs());
            (1280 - dx, 1280 - dy)
        };
        assert_eq!(overlap(0, 1), (256, 256));
        assert_eq!(overlap(1, 2), (256, 256));
        assert!(overlap(0, 2).0 <= 0, "the two ends are two tiles apart and do not touch");

        // The land beside each end rises to the deck height along the shared edge (spec §3.3). The
        // edge's two corners also touch water, where the step clamp (§3.3 step 8) may lower them, so
        // the check is on the vertices between the corners.
        let south_west = grid.footprint(5, 14, 1, 1);
        let north_east = grid.footprint(7, 12, 1, 1);
        let inner = |r: &Rect| (r.south / VERT_UNITS + 1)..(r.north / VERT_UNITS);
        for j in inner(&south_west) {
            assert_eq!(heights.height(south_west.west / VERT_UNITS, j), heightmap::DECK_HEIGHT as i32, "row {j}");
        }
        for j in inner(&north_east) {
            assert_eq!(heights.height(north_east.east / VERT_UNITS, j), heightmap::DECK_HEIGHT as i32, "row {j}");
        }
    }

    #[test]
    fn the_plan_is_the_same_on_every_run() {
        let objects = serde_json::json!([
            { "x": 2, "y": 2, "sprite": 40, "class": 9 },
            { "x": 8, "y": 3, "sprite": 40, "class": 10 },
            { "x": 4, "y": 9, "sprite": 30, "class": 8 }
        ]);
        let buildings = serde_json::json!([{ "x": 12, "y": 12, "size_x": 2, "size_y": 2, "kind": 1 }]);
        let (_, _, a) = plan_of(20, 20, &grass(400), objects.clone(), buildings.clone());
        let (_, _, b) = plan_of(20, 20, &grass(400), objects, buildings);
        assert_eq!(a.meshes, b.meshes);
        assert_eq!(a.cells, b.cells);
    }

    #[test]
    fn yaw_comes_from_the_sixteen_entry_table() {
        for k in 0..16u64 {
            let y = yaw(k);
            assert!((0.0..2.0 * PI).contains(&y));
        }
        assert_eq!(yaw(0), 0.0);
    }
}
