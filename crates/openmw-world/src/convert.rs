//! Map to plugin, textures, meshes and scripts (spec §2.2, §3.3–§3.8): the land of every cell of the
//! map and of its one-cell margin ring (§3.4), the ground textures, the references (M3), the army
//! bodies' CREA records and the ship rafts (M4), and the Lua runtime with `map_data.lua` (M4).

use crate::actors::{self, ArmyPlan, FIGURE_MODEL, RAFT_MESH};
use crate::grid::{self, Grid, GridError, CELL_TEXELS, CELL_VERTS, TEXEL_UNITS};
use crate::heightmap::{self, HeightField, Terrain};
use crate::input::{InputError, Map};
use crate::lua;
use crate::mesh;
use crate::passable;
use crate::placement::{self, MeshFile, Placed, Plan};
use crate::tes3::{self, Record, Tes3Error};
use crate::textures;
use std::collections::BTreeSet;
use std::fmt;

/// What to write: the plugin's name and tile size, and the master it depends on.
#[derive(Debug, Clone)]
pub struct Options {
    pub name: String,
    pub tile_units: i64,
    pub master: Master,
    /// The game time scale (spec §3.11, Q2): 120 by default.
    pub time_scale: i64,
    /// The passability guard on (spec §3.5); `false` for `--no-guard`.
    pub guard: bool,
    /// The archetype of the hero the player plays; `None` takes the first hero of the map.
    pub archetype: Option<String>,
    /// Use `land_racer.dae` from the Template's example as the body model instead of the placeholder.
    pub template_assets: bool,
}

/// The master (`MAST`/`DATA` in the header): its file name and its size, 0 when unknown.
#[derive(Debug, Clone)]
pub struct Master {
    pub name: String,
    pub size: u64,
}

/// The converter's files: the plugin bytes, the textures and the meshes as (path under the output
/// folder, bytes), and the heights the plugin was built from (for the `--debug-heightmap` picture).
#[derive(Debug)]
pub struct Output {
    pub plugin: Vec<u8>,
    pub textures: Vec<(String, Vec<u8>)>,
    pub meshes: Vec<(String, Vec<u8>)>,
    /// The Lua files, paths relative to the output folder: `map_data.lua`, the runtime and the
    /// `<name>.omwscripts`.
    pub scripts: Vec<(String, Vec<u8>)>,
    pub heights: HeightField,
}

#[derive(Debug)]
pub enum ConvertError {
    Input(InputError),
    Grid(GridError),
    Tes3(Tes3Error),
    /// The map has no hero, or the `--archetype` is not one of its heroes.
    Hero(String),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::Input(e) => write!(f, "{e}"),
            ConvertError::Grid(e) => write!(f, "{e}"),
            ConvertError::Tes3(e) => write!(f, "{e}"),
            ConvertError::Hero(m) => write!(f, "{m}"),
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

/// What the actor and script part adds to the plugin: CREA records and the Lua files.
struct Extras {
    records: Vec<Record>,
    scripts: Vec<(String, Vec<u8>)>,
}

/// Converts a parsed map.
pub fn convert(map: &Map, opts: &Options) -> Result<Output, ConvertError> {
    let grid = Grid::new(map.width, map.height, opts.tile_units)?;
    let codes = map.tile_codes()?;
    let terrain = Terrain::new(grid, codes.clone());
    let heights = heightmap::build(&terrain, map);
    let mut plan = placement::plan(map, &grid, &heights);
    let extras = runtime(map, opts, &grid, &codes, &heights, &mut plan)?;
    write(opts, &grid, &terrain, heights, plan, &|_, _, surface| surface, extras)
}

/// The M4 part: the armies' bodies (CREA), the ship rafts and the figure mesh go into the plan, and
/// the Lua files are made. `plan` gains two meshes (`figure`, `raft`) and the raft references.
fn runtime(
    map: &Map,
    opts: &Options,
    grid: &Grid,
    codes: &[u8],
    heights: &HeightField,
    plan: &mut Plan,
) -> Result<Extras, ConvertError> {
    let passable = passable::build(map, grid, codes);
    let boxes = plan.boxes.clone();
    // A map without a hero (only the synthetic test maps) starts at tile (0, 0) with no archetype.
    let hero = match &opts.archetype {
        Some(a) => Some(
            map.heroes
                .iter()
                .find(|h| &h.archetype == a)
                .ok_or_else(|| ConvertError::Hero(format!("the map has no hero of archetype {a}")))?,
        ),
        None => map.heroes.first(),
    };
    let (hero_archetype, hero_x, hero_y) = match hero {
        Some(h) => (h.archetype.clone(), h.x, h.y),
        None => (String::new(), 0, 0),
    };
    let model = if opts.template_assets { "land_racer.dae" } else { FIGURE_MODEL };
    let armies: ArmyPlan = actors::plan_armies(map, grid, heights, codes, &passable, &boxes, model);

    // The figure and the raft are meshes of the plan (their STATs and files come with the rest).
    let figure = mesh::figure();
    let figure_extent = figure.extent();
    plan.meshes.push(MeshFile { name: "figure".to_string(), dae: figure.dae("figure"), extent: figure_extent });
    let raft = mesh::raft();
    let raft_extent = raft.extent();
    let raft_index = plan.meshes.len();
    plan.meshes.push(MeshFile { name: RAFT_MESH.to_string(), dae: raft.dae(RAFT_MESH), extent: raft_extent });
    for p in &armies.rafts {
        plan.cells.entry(placement::cell_of(p[0], p[1])).or_default().push(Placed { mesh: raft_index, pos: *p, yaw: 0.0 });
    }

    let (hx, hy) = grid.tile_centre(hero_x, hero_y);
    let data = lua::MapData {
        width: grid.width,
        height: grid.height,
        tile: grid.tile,
        passable: &passable,
        boxes: &boxes,
        armies: &armies.armies,
        hero_archetype: &hero_archetype,
        hero_tile: (hero_x, hero_y),
        hero_z: placement::ground(heights, hx, hy),
        time_scale: opts.time_scale,
        guard: opts.guard,
    };
    let mut scripts = vec![
        (lua::MAP_DATA_PATH.to_string(), lua::map_data(&data).into_bytes()),
        (format!("{}.omwscripts", opts.name), lua::omwscripts().into_bytes()),
    ];
    for (path, text) in lua::RUNTIME {
        scripts.push((path.to_string(), text.as_bytes().to_vec()));
    }
    let records = armies.records.into_iter().collect();
    Ok(Extras { records, scripts })
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
    let codes = map.tile_codes()?;
    let terrain = Terrain::new(grid, codes.clone());
    let heights = HeightField::from_fn(heightmap::bounds(&grid), height);
    let mut plan = placement::plan(map, &grid, &heights);
    let extras = runtime(map, opts, &grid, &codes, &heights, &mut plan)?;
    write(opts, &grid, &terrain, heights, plan, texel, extras)
}

fn write(
    opts: &Options,
    grid: &Grid,
    terrain: &Terrain,
    heights: HeightField,
    plan: Plan,
    texel: &dyn Fn(i64, i64, u8) -> u8,
    extras: Extras,
) -> Result<Output, ConvertError> {
    // The cells of the map, and the margin ring around them (§3.4): -1 to the count, both ends in.
    let (cells_x, cells_y) = grid.cells();
    let mut land_records = Vec::new();
    let mut ring = BTreeSet::new();
    for cy in -1..=cells_y {
        for cx in -1..=cells_x {
            ring.insert((cx, cy));
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
            land_records.push(tes3::land_with_normals(
                cx as i32,
                cy as i32,
                &cell_heights,
                &cell_normals,
                &textures::vtex(&texels),
            )?);
        }
    }

    // Every cell with a reference gets a CELL record too (none falls outside the ring, but the
    // writer does not depend on it). FRMR indices run from 1 over the whole plugin, cell by cell.
    let all_cells: BTreeSet<(i64, i64)> = ring.iter().copied().chain(plan.cells.keys().copied()).collect();
    let mut next_ref = 1u32;
    let mut cell_records = Vec::new();
    for &(cx, cy) in &all_cells {
        let refs: Vec<tes3::Reference> = plan
            .cells
            .get(&(cx, cy))
            .map(|placed| {
                placed
                    .iter()
                    .map(|p| {
                        let r = tes3::Reference {
                            frmr: next_ref,
                            base: format!("razdor_{}", plan.meshes[p.mesh].name),
                            pos: [p.pos[0] as f32, p.pos[1] as f32, p.pos[2] as f32],
                            yaw: p.yaw as f32,
                        };
                        next_ref += 1;
                        r
                    })
                    .collect()
            })
            .unwrap_or_default();
        cell_records.push(tes3::cell_with_refs(cx as i32, cy as i32, &refs));
    }

    let ltex_count = textures::SURFACES.len();
    let stat_records: Vec<tes3::Record> = plan
        .meshes
        .iter()
        .map(|m| tes3::stat(&format!("razdor_{}", m.name), &format!("razdor/{}.dae", m.name)))
        .collect();
    let num_records =
        (ltex_count + stat_records.len() + extras.records.len() + cell_records.len() + land_records.len()) as u32;
    let mut records = vec![tes3::tes3_header(num_records, &opts.master.name, opts.master.size)];
    for index in 0..ltex_count {
        records.push(tes3::ltex(index as u32, &textures::ltex_id(index), &textures::texture_path(index)));
    }
    records.extend(stat_records);
    records.extend(extras.records);
    records.extend(cell_records);
    records.extend(land_records);

    let textures = (0..ltex_count)
        .map(|index| {
            let path = format!("textures/{}", textures::texture_path(index));
            (path, textures::tga(textures::TEXTURE_SIZE, textures::SURFACES[index].rgb))
        })
        .collect();
    let meshes = plan
        .meshes
        .iter()
        .map(|m| (format!("meshes/razdor/{}.dae", m.name), m.dae.clone().into_bytes()))
        .collect();
    Ok(Output { plugin: tes3::encode(&records), textures, meshes, scripts: extras.scripts, heights })
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
            time_scale: 120,
            guard: true,
            archetype: None,
            template_assets: false,
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

    /// A 4 by 4 map with one hero, two armies (one inactive) and a castle, as JSON text.
    fn army_map() -> Map {
        let rows: Vec<String> = vec!["5555".into(), "5955".into(), "5555".into(), "5550".into()];
        let text = serde_json::json!({
            "format": crate::input::FORMAT,
            "version": crate::input::VERSION,
            "width": 4,
            "height": 4,
            "info": { "generator_seed": 11, "start_time": 1 },
            "heroes": [{ "archetype": "knight", "x": 0, "y": 0, "troops": [] }],
            "terrain": { "encoding": "hex-rows", "rows": rows },
            "buildings": [{ "x": 3, "y": 2, "size_x": 2, "size_y": 2, "kind": 3 }],
            "armies": [
                { "id": 1, "x": 0, "y": 2, "name": "Отряд", "leader_unit": 13, "leader_level": 0,
                  "troops": [{ "unit": 4, "level": 0, "count": 2 }, { "unit": 4, "level": 1, "count": 1 }],
                  "patrols": 1, "patrol_radius": 1, "relations": [2, 0, 0, 0] },
                { "id": 2, "x": 1, "y": 1, "leader_unit": 9, "leader_level": 0, "troops": [{ "unit": 4, "level": 0, "count": 1 }],
                  "inactive": 1, "relations": [-2, 0, 0, 0] }
            ],
        })
        .to_string();
        Map::from_json(&text).unwrap()
    }

    #[test]
    fn the_armies_are_crea_records_and_the_scripts_are_written() {
        let map = army_map();
        let out = convert(&map, &opts(1024)).unwrap();
        let records = tes3::decode(&out.plugin).unwrap();
        let creas: Vec<String> = records
            .iter()
            .filter(|r| &r.name == b"CREA")
            .map(|r| tes3::read_zstring(r.get(b"NAME").unwrap()))
            .collect();
        // Leaders rz_army_1 and rz_army_2, unit cards rz_u4_l0 and rz_u4_l1 (shared by both armies and
        // not repeated), and the leader of army 2 is unit 9 under its own id.
        assert_eq!(creas, vec!["rz_army_1", "rz_army_2", "rz_u4_l0", "rz_u4_l1"]);
        let names: Vec<String> = out.scripts.iter().map(|(p, _)| p.clone()).collect();
        assert!(names.contains(&"scripts/razdor/map_data.lua".to_string()));
        assert!(names.contains(&"razdor_map.omwscripts".to_string()));
        assert!(names.contains(&"scripts/razdor/army.lua".to_string()));
    }

    #[test]
    fn two_runs_give_the_same_plugin_and_scripts() {
        let map = army_map();
        let a = convert(&map, &opts(1024)).unwrap();
        let b = convert(&map, &opts(1024)).unwrap();
        assert_eq!(a.plugin, b.plugin);
        assert_eq!(a.scripts, b.scripts);
        assert_eq!(a.meshes, b.meshes);
    }

    #[test]
    fn a_map_without_a_hero_of_the_asked_archetype_is_refused() {
        let map = army_map();
        let mut o = opts(1024);
        o.archetype = Some("ranger".to_string());
        assert!(matches!(convert(&map, &o), Err(ConvertError::Hero(_))));
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
        assert_eq!(a.meshes, b.meshes);
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
    /// The scenery of M3 counted two ways: the plugin's own records, and `esmtool dump -C`.
    #[test]
    fn esmtool_counts_the_statics_and_the_references() {
        let map = synthetic(
            20,
            20,
            &(0..400).map(|k| ((k * 5) % 16) as u8).collect::<Vec<u8>>(),
            serde_json::json!([
                { "x": 2, "y": 2, "sprite": 40, "class": 9 },
                { "x": 3, "y": 3, "sprite": 41, "class": 9 },
                { "x": 4, "y": 4, "sprite": 42, "class": 10 },
                { "x": 5, "y": 5, "sprite": 43, "class": 11 },
                { "x": 6, "y": 6, "sprite": 30, "class": 8 }
            ]),
            serde_json::json!([
                { "x": 12, "y": 12, "size_x": 4, "size_y": 4, "kind": 3 },
                { "x": 15, "y": 15, "size_x": 1, "size_y": 1, "kind": 13 }
            ]),
        );
        let out = convert(&map, &opts(1024)).unwrap();
        let records = tes3::decode(&out.plugin).unwrap();
        let statics = records.iter().filter(|r| &r.name == b"STAT").count();
        let refs: usize = records
            .iter()
            .filter(|r| &r.name == b"CELL")
            .map(|r| r.subs.iter().filter(|s| &s.name == b"FRMR").count())
            .sum();
        assert_eq!(refs, 7, "2 trees, a dead tree, a thicket, a stone, 1 castle, 1 bridge deck");
        assert_eq!(statics, out.meshes.len());
        // One STAT per mesh: tree kinds, dead tree, thicket, stone, castle, deck, and the M4 figure and raft.
        assert!((7..=8).contains(&statics), "tree kinds, dead tree, thicket, stone, castle, deck, figure, raft: {statics}");
        if !esmtool_installed() {
            eprintln!("skipped: esmtool is not on PATH");
            return;
        }
        let dir = scratch("statics");
        let text = esm_dump(&out, &dir);
        assert_eq!(count(&text, "Record: STAT"), statics, "{text}");
        let refnums = text.lines().filter(|l| l.trim_start().starts_with("- Refnum:")).count();
        assert_eq!(refnums, refs, "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

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
