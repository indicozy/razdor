//! Map to plugin and textures (spec §2.2, §3.3–§3.6): the land of every cell of the map and of its
//! one-cell margin ring (§3.4), one ground texture per surface, one CELL and one LAND per cell. No
//! references and no scripts yet (M3 and M4).

use crate::grid::{self, Grid, GridError, CELL_TEXELS, CELL_VERTS, TEXEL_UNITS};
use crate::heightmap::{self, HeightField, Terrain};
use crate::input::{InputError, Map};
use crate::tes3::{self, Tes3Error};
use crate::textures;
use std::fmt;

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

/// The converter's files: the plugin bytes, the textures as (path under the output folder, bytes),
/// and the heights the plugin was built from (for the `--debug-heightmap` picture).
#[derive(Debug)]
pub struct Output {
    pub plugin: Vec<u8>,
    pub textures: Vec<(String, Vec<u8>)>,
    pub heights: HeightField,
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

/// Converts a parsed map.
pub fn convert(map: &Map, opts: &Options) -> Result<Output, ConvertError> {
    let grid = Grid::new(map.width, map.height, opts.tile_units)?;
    let terrain = Terrain::new(grid, map.tile_codes()?);
    let heights = heightmap::build(&terrain, map);
    write(opts, &grid, &terrain, heights, &|_, _, surface| surface)
}

/// The conversion with two hooks, used by the tests: `height(i, j)` gives the height of global
/// land vertex `(i, j)`, and `texel(gx, gy, surface)` the surface of global texel `(gx, gy)` given
/// the tile's surface, so a test can raise one vertex or change one texel.
#[cfg(test)]
pub(crate) fn convert_with(
    map: &Map,
    opts: &Options,
    height: &dyn Fn(i64, i64) -> i32,
    texel: &dyn Fn(i64, i64, u8) -> u8,
) -> Result<Output, ConvertError> {
    let grid = Grid::new(map.width, map.height, opts.tile_units)?;
    let terrain = Terrain::new(grid, map.tile_codes()?);
    let heights = HeightField::from_fn(heightmap::bounds(&grid), height);
    write(opts, &grid, &terrain, heights, texel)
}

fn write(
    opts: &Options,
    grid: &Grid,
    terrain: &Terrain,
    heights: HeightField,
    texel: &dyn Fn(i64, i64, u8) -> u8,
) -> Result<Output, ConvertError> {
    // The cells of the map, and the margin ring around them (§3.4): -1 to the count, both ends in.
    let (cells_x, cells_y) = grid.cells();
    let mut cell_records = Vec::new();
    let mut land_records = Vec::new();
    for cy in -1..=cells_y {
        for cx in -1..=cells_x {
            let mut texels = [[0u8; 16]; 16];
            for (row, line) in texels.iter_mut().enumerate() {
                for (col, value) in line.iter_mut().enumerate() {
                    let gx = cx * CELL_TEXELS + col as i64;
                    let gy = cy * CELL_TEXELS + row as i64;
                    let wx = gx * TEXEL_UNITS + TEXEL_UNITS / 2;
                    let wy = gy * TEXEL_UNITS + TEXEL_UNITS / 2;
                    let surface = terrain.tile(wx.div_euclid(grid.tile), wy.div_euclid(grid.tile)).0;
                    *value = texel(gx, gy, surface);
                }
            }
            let n = (CELL_VERTS * CELL_VERTS) as usize;
            let mut cell_heights = vec![0i32; n];
            let mut cell_normals = vec![[0i8; 3]; n];
            for row in 0..CELL_VERTS {
                for col in 0..CELL_VERTS {
                    let (i, j) = grid::cell_vertex(cx, cy, col, row);
                    let at = (row * CELL_VERTS + col) as usize;
                    cell_heights[at] = heights.height(i, j);
                    cell_normals[at] = heights.normal(i, j);
                }
            }
            cell_records.push(tes3::cell(cx as i32, cy as i32));
            land_records.push(tes3::land_with_normals(
                cx as i32,
                cy as i32,
                &cell_heights,
                &cell_normals,
                &textures::vtex(&texels),
            )?);
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
    Ok(Output { plugin: tes3::encode(&records), textures, heights })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::synthetic;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// A map of `w` by `h` tiles, each tile's code from `code_at(x, y)` (y from the north).
    fn map_of(w: usize, h: usize, code_at: impl Fn(usize, usize) -> u8) -> Map {
        let codes: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| code_at(x, y)).collect();
        synthetic(w, h, &codes, serde_json::json!([]), serde_json::json!([]))
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

    /// The LAND records of an output, as (cell x, cell y, heights).
    fn lands(out: &Output) -> Vec<(i32, i32, Vec<i32>)> {
        tes3::decode(&out.plugin)
            .unwrap()
            .iter()
            .filter(|r| &r.name == b"LAND")
            .map(|r| {
                let (cx, cy) = tes3::land_cell(r).unwrap();
                (cx, cy, tes3::land_heights(r).unwrap())
            })
            .collect()
    }

    #[test]
    fn a_16_by_16_map_has_its_cells_and_the_margin_ring() {
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
        assert_eq!(count(&text, "Record: LAND"), 16, "{text}"); // 2x2 cells and the ring: 4x4
        assert_eq!(count(&text, "Record: CELL"), 16);
        assert_eq!(count(&text, "Record: LTEX"), 18);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_2048_tile_map_has_one_cell_per_8192_units_and_the_ring() {
        let map = map_of(16, 16, |_, _| 6);
        let out = convert(&map, &opts(2048)).unwrap();
        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("tile2048");
        let text = esm_dump(&out, &dir);
        assert_eq!(count(&text, "Record: LAND"), 36); // 4x4 cells and the ring: 6x6
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_50_by_50_sample_size_map_has_81_lands() {
        let map = map_of(50, 50, |x, y| ((x * 3 + y) % 16) as u8);
        let out = convert(&map, &opts(1024)).unwrap();
        assert_eq!(lands(&out).len(), 81); // 7x7 cells and the ring: 9x9
        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("sample81");
        let text = esm_dump(&out, &dir);
        assert_eq!(count(&text, "Record: LAND"), 81, "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn neighbouring_cells_share_their_edge_heights() {
        let map = map_of(20, 20, |x, y| ((x * 7 + y * 5) % 16) as u8);
        let out = convert(&map, &opts(1024)).unwrap();
        let all = lands(&out);
        let find = |cx: i32, cy: i32| all.iter().find(|l| l.0 == cx && l.1 == cy).unwrap().2.clone();
        let (a, b) = (find(0, 0), find(1, 0));
        for row in 0..65 {
            assert_eq!(a[row * 65 + 64], b[row * 65], "row {row}");
        }
        let (c, d) = (find(0, 0), find(0, 1));
        for col in 0..65 {
            assert_eq!(c[64 * 65 + col], d[col], "col {col}");
        }
    }

    #[test]
    fn the_output_is_deterministic() {
        let map = synthetic(
            20,
            12,
            &(0..240).map(|k| ((k * 3) % 16) as u8).collect::<Vec<u8>>(),
            serde_json::json!([
                { "x": 4, "y": 3, "sprite": 30, "class": 5 },
                { "x": 9, "y": 6, "sprite": 20, "class": 1 },
                { "x": 2, "y": 8, "sprite": 40, "class": 9 }
            ]),
            serde_json::json!([{ "x": 12, "y": 9, "size_x": 3, "size_y": 2, "kind": 3 }]),
        );
        let a = convert(&map, &opts(1024)).unwrap();
        let b = convert(&map, &opts(1024)).unwrap();
        assert_eq!(a.plugin, b.plugin);
        assert_eq!(a.textures, b.textures);
        assert_eq!(a.heights.pgm(), b.heights.pgm());
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
        let height = |i: i64, j: i64| if (i, j) == (2, 1) { 800 } else { 96 };
        let texel = |gx: i64, gy: i64, s: u8| if (gx, gy) == (3, 0) { 3 } else { s };
        let out = convert_with(&map, &opts(1024), &height, &texel).unwrap();

        let records = tes3::decode(&out.plugin).unwrap();
        let lands: Vec<&tes3::Record> = records.iter().filter(|r| &r.name == b"LAND").collect();
        assert_eq!(lands.len(), 16);
        let first = lands.iter().find(|r| tes3::land_cell(r).unwrap() == (0, 0)).unwrap();
        let heights = tes3::land_heights(first).unwrap();
        assert_eq!(heights[65 + 2], 800);
        assert_eq!(heights[0], 96);
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
        let max_heights: Vec<String> = text
            .split("Record: LAND")
            .skip(1)
            .map(|block| block.lines().find(|l| l.contains("MaxHeight")).unwrap_or("").trim().to_string())
            .collect();
        assert_eq!(max_heights.len(), 16, "{text}");
        assert_eq!(max_heights.iter().filter(|m| m.contains("800")).count(), 1, "{max_heights:?}");
        for entry in max_heights.iter().filter(|m| !m.contains("800")) {
            assert!(entry.contains("96"), "{entry:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
