//! Reads the `razdor-dtm-json` export, format version 1 (`docs/flows/testing-and-tools.md`).
//! Only the fields the converter uses are typed; every other key is ignored.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

/// The `format` value the exporter writes.
pub const FORMAT: &str = "razdor-dtm-json";
/// The only `version` this reader understands.
pub const VERSION: u32 = 1;
/// The highest surface code (16 surfaces, codes 0..=15).
pub const MAX_CODE: u32 = 15;

#[derive(Debug, Deserialize)]
pub struct Map {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub source_file: Option<String>,
    pub width: usize,
    pub height: usize,
    #[serde(default)]
    pub info: Info,
    #[serde(default)]
    pub legend: Legend,
    pub terrain: Terrain,
    #[serde(default)]
    pub objects: Vec<MapObject>,
    #[serde(default)]
    pub buildings: Vec<Building>,
    #[serde(default)]
    pub heroes: Vec<Hero>,
    #[serde(default)]
    pub armies: Vec<Army>,
}

/// A hero of the preset list: `archetype` is `knight`, `archmage` or `ranger`; `(x, y)` his start tile.
#[derive(Debug, Deserialize)]
pub struct Hero {
    #[serde(default)]
    pub archetype: String,
    pub x: i64,
    pub y: i64,
}

/// One unit card of an army: its DT unit id, level and troop count (M4 spawns `count` bodies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Troop {
    pub unit: i64,
    #[serde(default)]
    pub level: i64,
    #[serde(default = "one")]
    pub count: i64,
}

fn one() -> i64 {
    1
}

/// An army (spec §3.8). `(x, y)` is its home tile, `leader_unit` and `leader_level` its leader's
/// card; `relations` is the army's own list (its relation to each faction; index 0 is not
/// known to be the player, spec §3.8 open point); `ship` is non-zero for a ship army.
#[derive(Debug, Deserialize)]
pub struct Army {
    pub id: i64,
    pub x: i64,
    pub y: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub leader_name: String,
    #[serde(default)]
    pub leader_unit: i64,
    #[serde(default)]
    pub leader_level: i64,
    #[serde(default)]
    pub troops: Vec<Troop>,
    #[serde(default)]
    pub faction: i64,
    #[serde(default)]
    pub relations: Vec<i64>,
    #[serde(default)]
    pub inactive: i64,
    #[serde(default)]
    pub patrols: i64,
    #[serde(default)]
    pub patrol_radius: i64,
    #[serde(default)]
    pub behaviour: i64,
    #[serde(default)]
    pub home_building: i64,
    #[serde(default)]
    pub respawn_days: i64,
    #[serde(default)]
    pub ship: i64,
}

/// One scenery object: `class` 1-4 hills, 5-8 massifs (mountains, stones), 9-12 plants.
/// `sprite div 10` is the side of a massif's square in tiles.
#[derive(Debug, Deserialize)]
pub struct MapObject {
    pub x: i64,
    pub y: i64,
    pub sprite: i64,
    pub class: i64,
}

/// A building or bridge: `(x, y)` is the bottom-right tile of its footprint, as the exporter writes
/// it (`docs/flows/testing-and-tools.md`, "The map exporter"). Kinds 13 and 14 are bridges.
#[derive(Debug, Deserialize)]
pub struct Building {
    pub x: i64,
    pub y: i64,
    pub size_x: i64,
    pub size_y: i64,
    pub kind: i64,
}

#[derive(Debug, Default, Deserialize)]
pub struct Info {
    #[serde(default)]
    pub generator_seed: u64,
    #[serde(default)]
    pub start_time: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Legend {
    #[serde(default)]
    pub surfaces: Vec<Surface>,
}

#[derive(Debug, Deserialize)]
pub struct Surface {
    pub code: u32,
    pub name: String,
}

/// The terrain grid. `hex-rows`: one hex digit per tile; `numbers`: decimal values (used when a
/// code is above 15, which the reader then refuses).
#[derive(Debug, Deserialize)]
#[serde(tag = "encoding", content = "rows", rename_all = "kebab-case")]
pub enum Terrain {
    HexRows(Vec<String>),
    Numbers(Vec<Vec<u32>>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputError {
    Json(String),
    Format(String),
    Version(u32),
    Size { width: usize, height: usize },
    RowCount { expected: usize, got: usize },
    RowLength { row: usize, expected: usize, got: usize },
    BadCode { row: usize, column: usize, text: String },
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputError::Json(e) => write!(f, "not a razdor-dtm-json file: {e}"),
            InputError::Format(s) => write!(f, "format is \"{s}\", expected \"{FORMAT}\""),
            InputError::Version(v) => write!(f, "version {v} is not supported (expected {VERSION})"),
            InputError::Size { width, height } => write!(f, "map size {width}x{height} is not valid"),
            InputError::RowCount { expected, got } => {
                write!(f, "terrain has {got} rows, the map is {expected} high")
            }
            InputError::RowLength { row, expected, got } => {
                write!(f, "terrain row {row} has {got} cells, the map is {expected} wide")
            }
            InputError::BadCode { row, column, text } => {
                write!(f, "terrain row {row}, column {column}: \"{text}\" is not a surface code 0-15")
            }
        }
    }
}

impl Map {
    /// Parses and checks the header (`format`, `version`, size).
    pub fn from_json(text: &str) -> Result<Map, InputError> {
        let map: Map = serde_json::from_str(text).map_err(|e| InputError::Json(e.to_string()))?;
        if map.format != FORMAT {
            return Err(InputError::Format(map.format.clone()));
        }
        if map.version != VERSION {
            return Err(InputError::Version(map.version));
        }
        if map.width == 0 || map.height == 0 || map.width > 1000 || map.height > 1000 {
            return Err(InputError::Size { width: map.width, height: map.height });
        }
        Ok(map)
    }

    /// The objects the converter keeps (world.md §1.1): classes 1-12 on the map, at most one massif
    /// (classes 1-8) and one plant (9-12) per tile, the later record of a tile winning. Ordered row by
    /// row, from the north, then by column, as DT scans them.
    pub fn kept_objects(&self) -> Vec<&MapObject> {
        let mut by_tile: BTreeMap<(i64, i64, u8), &MapObject> = BTreeMap::new();
        for o in &self.objects {
            let inside = o.x >= 0 && o.y >= 0 && o.x < self.width as i64 && o.y < self.height as i64;
            if !inside || !(1..=12).contains(&o.class) {
                continue;
            }
            let group = if o.class <= 8 { 0 } else { 1 };
            by_tile.insert((o.y, o.x, group), o);
        }
        by_tile.into_values().collect()
    }

    /// The surface code of every tile, row-major (index `y * width + x`, row 0 = north).
    pub fn tile_codes(&self) -> Result<Vec<u8>, InputError> {
        let (w, h) = (self.width, self.height);
        let mut out = Vec::with_capacity(w * h);
        match &self.terrain {
            Terrain::HexRows(rows) => {
                if rows.len() != h {
                    return Err(InputError::RowCount { expected: h, got: rows.len() });
                }
                for (y, row) in rows.iter().enumerate() {
                    let cells: Vec<char> = row.chars().collect();
                    if cells.len() != w {
                        return Err(InputError::RowLength { row: y, expected: w, got: cells.len() });
                    }
                    for (x, ch) in cells.iter().enumerate() {
                        match ch.to_digit(16) {
                            Some(v) => out.push(v as u8),
                            None => {
                                return Err(InputError::BadCode { row: y, column: x, text: ch.to_string() })
                            }
                        }
                    }
                }
            }
            Terrain::Numbers(rows) => {
                if rows.len() != h {
                    return Err(InputError::RowCount { expected: h, got: rows.len() });
                }
                for (y, row) in rows.iter().enumerate() {
                    if row.len() != w {
                        return Err(InputError::RowLength { row: y, expected: w, got: row.len() });
                    }
                    for (x, v) in row.iter().enumerate() {
                        if *v > MAX_CODE {
                            return Err(InputError::BadCode { row: y, column: x, text: v.to_string() });
                        }
                        out.push(*v as u8);
                    }
                }
            }
        }
        Ok(out)
    }
}

/// A map from synthetic parts (tests only): `codes` row-major from the north, `w` wide.
#[cfg(test)]
pub(crate) fn synthetic(
    w: usize,
    h: usize,
    codes: &[u8],
    objects: serde_json::Value,
    buildings: serde_json::Value,
) -> Map {
    let rows: Vec<String> = (0..h)
        .map(|y| codes[y * w..(y + 1) * w].iter().map(|c| format!("{c:x}")).collect())
        .collect();
    let text = serde_json::json!({
        "format": FORMAT,
        "version": VERSION,
        "width": w,
        "height": h,
        "info": { "generator_seed": 7 },
        "legend": { "surfaces": [] },
        "terrain": { "encoding": "hex-rows", "rows": rows },
        "objects": objects,
        "buildings": buildings,
    })
    .to_string();
    Map::from_json(&text).unwrap()
}

#[cfg(test)]
mod object_tests {
    use super::*;

    #[test]
    fn one_massif_and_one_plant_per_tile_the_later_record_winning() {
        let objects = serde_json::json!([
            { "x": 1, "y": 1, "sprite": 40, "class": 9 },
            { "x": 1, "y": 1, "sprite": 40, "class": 10 },
            { "x": 1, "y": 1, "sprite": 40, "class": 1 },
            { "x": 2, "y": 0, "sprite": 40, "class": 5 },
            { "x": 0, "y": 2, "sprite": 40, "class": 77 },
            { "x": 9, "y": 9, "sprite": 40, "class": 9 }
        ]);
        let map = synthetic(3, 3, &[5; 9], objects, serde_json::json!([]));
        let kept: Vec<(i64, i64, i64)> = map.kept_objects().iter().map(|o| (o.x, o.y, o.class)).collect();
        // Row 0 first, then row 1 (the plant of (1, 1) wins over the tree; the hill keeps the massif slot).
        assert_eq!(kept, vec![(2, 0, 5), (1, 1, 1), (1, 1, 10)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(terrain: &str, w: usize, h: usize) -> String {
        format!(
            r#"{{"format":"razdor-dtm-json","version":1,"source_file":"x.DTm","width":{w},"height":{h},
            "info":{{"generator_seed":42,"start_time":5}},"legend":{{"surfaces":[{{"code":0,"name":"shallows_fords"}}]}},
            "terrain":{terrain},"objects":[]}}"#
        )
    }

    #[test]
    fn hex_rows_decode_row_major_north_first() {
        let m = Map::from_json(&doc(r#"{"encoding":"hex-rows","rows":["0123","4567"]}"#, 4, 2)).unwrap();
        assert_eq!(m.info.generator_seed, 42);
        assert_eq!(m.tile_codes().unwrap(), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn numbers_decode_the_same_codes() {
        let m = Map::from_json(&doc(r#"{"encoding":"numbers","rows":[[0,1,2,3],[4,5,6,15]]}"#, 4, 2)).unwrap();
        assert_eq!(m.tile_codes().unwrap(), vec![0, 1, 2, 3, 4, 5, 6, 15]);
    }

    #[test]
    fn numbers_above_15_are_refused() {
        let m = Map::from_json(&doc(r#"{"encoding":"numbers","rows":[[16,0]]}"#, 2, 1)).unwrap();
        assert!(matches!(m.tile_codes(), Err(InputError::BadCode { row: 0, column: 0, .. })));
    }

    #[test]
    fn wrong_row_count_and_length_are_errors() {
        let m = Map::from_json(&doc(r#"{"encoding":"hex-rows","rows":["012"]}"#, 3, 2)).unwrap();
        assert_eq!(m.tile_codes(), Err(InputError::RowCount { expected: 2, got: 1 }));
        let m = Map::from_json(&doc(r#"{"encoding":"hex-rows","rows":["012","01"]}"#, 3, 2)).unwrap();
        assert_eq!(m.tile_codes(), Err(InputError::RowLength { row: 1, expected: 3, got: 2 }));
        let m = Map::from_json(&doc(r#"{"encoding":"hex-rows","rows":["0g2"]}"#, 3, 1)).unwrap();
        assert!(matches!(m.tile_codes(), Err(InputError::BadCode { .. })));
    }

    #[test]
    fn format_and_version_are_checked() {
        let bad_format = doc(r#"{"encoding":"hex-rows","rows":["0"]}"#, 1, 1).replace("razdor-dtm-json", "other");
        assert!(matches!(Map::from_json(&bad_format), Err(InputError::Format(_))));
        let bad_version = doc(r#"{"encoding":"hex-rows","rows":["0"]}"#, 1, 1).replace("\"version\":1", "\"version\":2");
        assert_eq!(Map::from_json(&bad_version).unwrap_err(), InputError::Version(2));
        assert!(matches!(Map::from_json("[1,2]"), Err(InputError::Json(_))));
    }
}
