//! Reads the `razdor-dtm-json` export, format version 1 (`docs/flows/testing-and-tools.md`).
//! Only the fields the converter uses are typed; every other key is ignored.

use serde::Deserialize;
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
