//! DT's on-foot map (spec §3.5; `docs/reference/original-mechanics/world.md` §1.1): one value per
//! tile, 0 closed to a walker, otherwise the cost of an orthogonal step in DT's units (minutes =
//! value × 5 at hero speed 5). The player's guard reads it in milestone 4; M2 computes and tests it.

use crate::grid::Grid;
use crate::input::{Map, MapObject};

/// Terrain value of a surface code (world.md §1.1). Codes 0-2 are water: closed, whatever is on them.
fn terrain_value(code: u8) -> i32 {
    match code {
        4 => 3,
        5..=7 => 5,
        8 => 8,
        10 | 11 | 13 => 5,
        12 => 4,
        14 => 6,
        // 3 lava fields, 9 impassable swamp, 15 snowdrifts: blocked.
        _ => -32,
    }
}

/// Land value of a hill (class 1-4) on its square: 2, or 3 for class 4.
fn hill_value(class: i64) -> i32 {
    if class == 4 {
        3
    } else {
        2
    }
}

/// Value a plant (class 9-12) adds on its own tile. The thicket (11) is blocked.
fn plant_value(class: i64) -> i32 {
    match class {
        9 | 12 => 4,
        10 => 6,
        _ => -32,
    }
}

/// The on-foot map of a map, row-major from the north like the terrain (index `y * width + x`).
/// `codes` is the map's surface codes in the same order.
pub fn build(map: &Map, grid: &Grid, codes: &[u8]) -> Vec<u8> {
    let (w, h) = (grid.width, grid.height);
    let n = (w * h) as usize;
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h;
    // The tiles of a massif's square, clipped to the map (world.md §1.1: bottom-right anchored).
    let square = |o: &MapObject| -> Vec<(i64, i64)> {
        let f = (o.sprite / 10).max(1);
        Grid::footprint_tiles(o.x, o.y, f, f).into_iter().filter(|&(x, y)| inside(x, y)).collect()
    };
    let objects = map.kept_objects();

    // Step 2: hills set the land value over their square.
    let mut hill = vec![0i32; n];
    for o in objects.iter().copied().filter(|o| (1..=4).contains(&o.class)) {
        for (x, y) in square(o) {
            hill[idx(x, y)] = hill_value(o.class);
        }
    }
    // Step 3: the surface's value, with a hill's value added on land; water is closed.
    let mut land: Vec<i32> = (0..n)
        .map(|k| {
            let code = codes[k];
            if code <= 2 {
                0
            } else {
                (hill[k] + terrain_value(code)).max(0)
            }
        })
        .collect();
    // Step 4: a plant adds its value on its own tile; on water the tile stays closed.
    for o in objects.iter().copied().filter(|o| (9..=12).contains(&o.class)) {
        let k = idx(o.x, o.y);
        if land[k] != 0 {
            land[k] = (land[k] + plant_value(o.class)).max(0);
        }
    }
    // Step 5: mountains and rocks close their square.
    for o in objects.iter().copied().filter(|o| (5..=8).contains(&o.class)) {
        for (x, y) in square(o) {
            land[idx(x, y)] = 0;
        }
    }
    // Step 6: every building footprint, bridges included, is a road (cost 3).
    for b in &map.buildings {
        for (x, y) in Grid::footprint_tiles(b.x, b.y, b.size_x, b.size_y) {
            if inside(x, y) {
                land[idx(x, y)] = 3;
            }
        }
    }
    land.into_iter().map(|v| v.clamp(0, 255) as u8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::synthetic;
    use serde_json::json;

    fn value_at(w: usize, h: usize, codes: &[u8], objects: serde_json::Value, buildings: serde_json::Value, x: i64, y: i64) -> u8 {
        let map = synthetic(w, h, codes, objects, buildings);
        let grid = Grid::new(w, h, 1024).unwrap();
        build(&map, &grid, &map.tile_codes().unwrap())[(y * w as i64 + x) as usize]
    }

    #[test]
    fn every_surface_code_has_its_value() {
        // world.md §1.1: water closed; 3 lava, 9 swamp, 15 snowdrifts closed; the rest their cost.
        let want = [0u8, 0, 0, 0, 3, 5, 5, 5, 8, 0, 5, 5, 4, 5, 6, 0];
        for code in 0..16u8 {
            let got = value_at(1, 1, &[code], json!([]), json!([]), 0, 0);
            assert_eq!(got, want[code as usize], "surface code {code}");
        }
    }

    #[test]
    fn a_tree_adds_to_its_grass_and_a_hill_adds_two() {
        let grass = [5u8; 9];
        let tree = json!([{ "x": 1, "y": 1, "sprite": 40, "class": 9 }]);
        assert_eq!(value_at(3, 3, &grass, tree, json!([]), 1, 1), 9); // 5 + 4
        assert_eq!(value_at(3, 3, &grass, json!([]), json!([]), 1, 1), 5);
        let hill = json!([{ "x": 1, "y": 1, "sprite": 10, "class": 1 }]);
        assert_eq!(value_at(3, 3, &grass, hill, json!([]), 1, 1), 7); // 5 + 2
    }

    #[test]
    fn a_thicket_and_a_mountain_are_closed() {
        let grass = [5u8; 9];
        let thicket = json!([{ "x": 1, "y": 1, "sprite": 40, "class": 11 }]);
        assert_eq!(value_at(3, 3, &grass, thicket, json!([]), 1, 1), 0);
        let mountain = json!([{ "x": 1, "y": 1, "sprite": 10, "class": 5 }]);
        assert_eq!(value_at(3, 3, &grass, mountain, json!([]), 1, 1), 0);
    }

    #[test]
    fn a_bridge_on_water_is_open() {
        let water = [0u8; 9];
        let bridge = json!([{ "x": 1, "y": 1, "size_x": 1, "size_y": 1, "kind": 13 }]);
        assert_eq!(value_at(3, 3, &water, json!([]), bridge.clone(), 1, 1), 3);
        assert_eq!(value_at(3, 3, &water, json!([]), bridge, 0, 0), 0);
    }

    #[test]
    fn a_massif_square_past_the_west_edge_is_clipped() {
        // Sprite 30: a square of side 3 with its bottom-right tile at (0, 2): columns -2..=0, rows 0..=2.
        let grass = [5u8; 25];
        let mountain = json!([{ "x": 0, "y": 2, "sprite": 30, "class": 5 }]);
        let map = synthetic(5, 5, &grass, mountain, json!([]));
        let grid = Grid::new(5, 5, 1024).unwrap();
        let values = build(&map, &grid, &map.tile_codes().unwrap());
        for y in 0..3 {
            assert_eq!(values[y * 5], 0, "column 0, row {y}");
            assert_eq!(values[y * 5 + 1], 5, "column 1, row {y}");
        }
        assert_eq!(values[3 * 5], 5, "row 3 is outside the square");
    }
}
