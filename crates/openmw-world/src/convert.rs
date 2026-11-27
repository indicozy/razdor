//! Map to plugin and textures (milestone 1): flat land at +96 everywhere, one ground texture per
//! surface, one CELL and one LAND per grid cell, no references and no margin ring.

use crate::grid::{self, Grid, GridError, CELL_VERTS, TEXEL_UNITS};
use crate::input::{InputError, Map};
use crate::tes3::{self, Tes3Error};
use crate::textures;
use std::fmt;

/// Land height of M1, in units.
pub const FLAT_HEIGHT: i32 = 96;

/// What to write: the plugin's name and tile size, and the master it depends on.
#[derive(Debug, Clone)]
pub struct Options {
    pub name: String,
    pub tile_units: i64,
    pub master: Master,
}

/// The master (`MAST`/`DATA` in the header): its file name and its size, 0 when unknown.
#[derive(Debug, Clone)]
pub struct Master {
    pub name: String,
    pub size: u64,
}

/// The converter's files: the plugin bytes and the textures as (path under the output folder, bytes).
#[derive(Debug)]
pub struct Output {
    pub plugin: Vec<u8>,
    pub textures: Vec<(String, Vec<u8>)>,
}

#[derive(Debug)]
pub enum ConvertError {
    Input(InputError),
    Grid(GridError),
    Tes3(Tes3Error),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::Input(e) => write!(f, "{e}"),
            ConvertError::Grid(e) => write!(f, "{e}"),
            ConvertError::Tes3(e) => write!(f, "{e}"),
        }
    }
}

impl From<InputError> for ConvertError {
    fn from(e: InputError) -> Self {
        ConvertError::Input(e)
    }
}

impl From<GridError> for ConvertError {
    fn from(e: GridError) -> Self {
        ConvertError::Grid(e)
    }
}

impl From<Tes3Error> for ConvertError {
    fn from(e: Tes3Error) -> Self {
        ConvertError::Tes3(e)
    }
}

/// Converts a parsed map (M1: flat land).
pub fn convert(map: &Map, opts: &Options) -> Result<Output, ConvertError> {
    convert_with(map, opts, &|_, _| FLAT_HEIGHT, &|_, _, surface| surface)
}

/// The conversion with two hooks, used by the tests: `height(i, j)` gives the height of global
/// land vertex `(i, j)`, and `texel(gx, gy, surface)` the surface of global texel `(gx, gy)` given
/// the tile's surface, so a test can raise one vertex or change one texel.
pub(crate) fn convert_with(
    map: &Map,
    opts: &Options,
    height: &dyn Fn(i64, i64) -> i32,
    texel: &dyn Fn(i64, i64, u8) -> u8,
) -> Result<Output, ConvertError> {
    let grid = Grid::new(map.width, map.height, opts.tile_units)?;
    let codes = map.tile_codes()?;
    let surface_at = |wx: i64, wy: i64| -> u8 {
        match grid.tile_at(wx, wy) {
            Some((x, y)) => codes[(y * grid.width + x) as usize],
            None => textures::DEEP_SEA,
        }
    };

    let (cells_x, cells_y) = grid.cells();
    let mut cell_records = Vec::new();
    let mut land_records = Vec::new();
    for cy in 0..cells_y {
        for cx in 0..cells_x {
            let mut texels = [[0u8; 16]; 16];
            for (row, line) in texels.iter_mut().enumerate() {
                for (col, value) in line.iter_mut().enumerate() {
                    let gx = cx * 16 + col as i64;
                    let gy = cy * 16 + row as i64;
                    let wx = gx * TEXEL_UNITS + TEXEL_UNITS / 2;
                    let wy = gy * TEXEL_UNITS + TEXEL_UNITS / 2;
                    *value = texel(gx, gy, surface_at(wx, wy));
                }
            }
            let mut heights = vec![0i32; (CELL_VERTS * CELL_VERTS) as usize];
            for row in 0..CELL_VERTS {
                for col in 0..CELL_VERTS {
                    let (i, j) = grid::cell_vertex(cx, cy, col, row);
                    heights[(row * CELL_VERTS + col) as usize] = height(i, j);
                }
            }
            cell_records.push(tes3::cell(cx as i32, cy as i32));
            land_records.push(tes3::land(cx as i32, cy as i32, &heights, &textures::vtex(&texels))?);
        }
    }

    let ltex_count = textures::SURFACES.len();
    let num_records = (ltex_count + cell_records.len() + land_records.len()) as u32;
    let mut records = vec![tes3::tes3_header(num_records, &opts.master.name, opts.master.size)];
    for index in 0..ltex_count {
        records.push(tes3::ltex(index as u32, &textures::ltex_id(index), &textures::texture_path(index)));
    }
    records.extend(cell_records);
    records.extend(land_records);

    let textures = (0..ltex_count)
        .map(|index| {
            let path = format!("textures/{}", textures::texture_path(index));
            (path, textures::tga(textures::TEXTURE_SIZE, textures::SURFACES[index].rgb))
        })
        .collect();
    Ok(Output { plugin: tes3::encode(&records), textures })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A map of `w` by `h` tiles, each tile's code from `code_at(x, y)` (y from the north).
    fn map_of(w: usize, h: usize, code_at: impl Fn(usize, usize) -> u8) -> Map {
        let rows: Vec<String> = (0..h)
            .map(|y| (0..w).map(|x| format!("{:x}", code_at(x, y))).collect())
            .collect();
        let text = serde_json::json!({
            "format": "razdor-dtm-json",
            "version": 1,
            "width": w,
            "height": h,
            "info": { "generator_seed": 7 },
            "legend": { "surfaces": [] },
            "terrain": { "encoding": "hex-rows", "rows": rows },
        })
        .to_string();
        Map::from_json(&text).unwrap()
    }

    fn opts(tile: i64) -> Options {
        Options {
            name: "razdor_map".to_string(),
            tile_units: tile,
            master: Master { name: "template.omwgame".to_string(), size: 0 },
        }
    }

    fn esmtool_installed() -> bool {
        Command::new("esmtool").arg("-v").output().is_ok()
    }

    /// A scratch folder for one test, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-openmw-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes the plugin and runs `esmtool dump -C -e win1251` on it; returns the output text.
    fn esm_dump(out: &Output, dir: &Path) -> String {
        let path = dir.join("razdor_map.omwaddon");
        std::fs::write(&path, &out.plugin).unwrap();
        let run = Command::new("esmtool").args(["dump", "-C", "-e", "win1251"]).arg(&path).output().unwrap();
        assert!(run.status.success(), "esmtool exited with {:?}", run.status.code());
        let mut text = String::from_utf8_lossy(&run.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&run.stderr));
        text
    }

    fn count(text: &str, prefix: &str) -> usize {
        text.lines().filter(|l| l.starts_with(prefix)).count()
    }

    #[test]
    fn a_16_by_16_map_has_4_cells_4_lands_and_18_textures() {
        let map = map_of(16, 16, |x, y| ((x + y) % 8) as u8);
        let out = convert(&map, &opts(1024)).unwrap();
        assert_eq!(out.textures.len(), 18);
        assert!(out.textures.iter().all(|(p, b)| p.starts_with("textures/razdor/") && b.len() == 18 + 64 * 64 * 3));
        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("counts");
        let text = esm_dump(&out, &dir);
        assert_eq!(count(&text, "Record: LAND"), 4, "{text}");
        assert_eq!(count(&text, "Record: CELL"), 4);
        assert_eq!(count(&text, "Record: LTEX"), 18);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_2048_tile_map_has_one_cell_per_8192_units() {
        let map = map_of(16, 16, |_, _| 6);
        let out = convert(&map, &opts(2048)).unwrap();
        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("tile2048");
        let text = esm_dump(&out, &dir);
        assert_eq!(count(&text, "Record: LAND"), 16);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_output_is_deterministic() {
        let map = map_of(20, 12, |x, y| ((x * 3 + y) % 16) as u8);
        let a = convert(&map, &opts(1024)).unwrap();
        let b = convert(&map, &opts(1024)).unwrap();
        assert_eq!(a.plugin, b.plugin);
        assert_eq!(a.textures, b.textures);
    }

    #[test]
    fn a_bad_tile_size_is_an_error() {
        let map = map_of(4, 4, |_, _| 6);
        assert!(matches!(convert(&map, &opts(768)), Err(ConvertError::Grid(_))));
    }

    /// Orientation probe (spec §6, M1): one raised vertex at row 1, column 2 of cell (0, 0) and one
    /// texel of a different surface at texel row 0, column 3 of cell (0, 0). The plugin is read back
    /// with the crate's reader, and `esmtool` checks the heights it can show.
    #[test]
    fn a_raised_vertex_and_an_odd_texel_land_where_the_spec_puts_them() {
        let map = map_of(16, 16, |_, _| 6);
        let height = |i: i64, j: i64| if (i, j) == (2, 1) { 800 } else { FLAT_HEIGHT };
        let texel = |gx: i64, gy: i64, s: u8| if (gx, gy) == (3, 0) { 3 } else { s };
        let out = convert_with(&map, &opts(1024), &height, &texel).unwrap();

        let records = tes3::decode(&out.plugin).unwrap();
        let lands: Vec<&tes3::Record> = records.iter().filter(|r| &r.name == b"LAND").collect();
        assert_eq!(lands.len(), 4);
        let first = lands[0];
        assert_eq!(tes3::land_cell(first).unwrap(), (0, 0));
        let heights = tes3::land_heights(first).unwrap();
        assert_eq!(heights[65 + 2], 800);
        assert_eq!(heights[0], FLAT_HEIGHT);
        let vtex = tes3::land_vtex(first).unwrap();
        // Row 0, column 3 sits at file position 3 and holds LTEX index 3 + 1.
        assert_eq!(vtex[3], 4);
        assert_eq!(vtex[textures::file_position(0, 2)], 7); // its neighbour keeps grass_plain (6 + 1)

        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("orientation");
        let text = esm_dump(&out, &dir);
        let max_heights: Vec<(String, String)> = text
            .split("Record: LAND")
            .skip(1)
            .map(|block| {
                let coords = block.lines().find(|l| l.contains("Coordinates")).unwrap_or("").trim().to_string();
                let max = block.lines().find(|l| l.contains("MaxHeight")).unwrap_or("").trim().to_string();
                (coords, max)
            })
            .collect();
        assert_eq!(max_heights.len(), 4, "{text}");
        assert!(max_heights[0].1.contains("800"), "cell (0,0): {:?}", max_heights[0]);
        for entry in &max_heights[1..] {
            assert!(entry.1.contains("96"), "{entry:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
