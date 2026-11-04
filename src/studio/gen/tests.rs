//! The generators end to end: the same seed gives the same package map, every map type at the
//! three package sizes passes the package checker with no error, and the install's pinned
//! counts (when `RAZDOR_DT_DIR` is set; the tests on the install skip without it).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::to_package::{to_package, unit_aliases, PackageMap};
use super::worldgen::builder::{StatBlock, UnitFacts};
use super::worldgen::{BuildingPicture, Pictures, Units};
use super::{generate, newmap, Failure, Generated, Request, Sources, World};
use crate::pack::files::Files;
use crate::pack::load::Package;

/// Sprites of a stand-in install: every plant family with all its sprites, and the hills and
/// mountains of sizes 1 to 8 with four variants each. Only for tests: a real map takes the
/// install's sprites (`newmap::Sprites::from_install`).
fn stand_in_sprites() -> newmap::Sprites {
    let mut list: Vec<(u8, u8)> = Vec::new();
    for class in [9u8, 10, 11] {
        list.extend((0..120u8).map(|s| (class, s)));
    }
    for class in [1u8, 5] {
        for k in 1..=8u8 {
            list.extend((0..4u8).map(|v| (class, 10 * k + v)));
        }
    }
    newmap::Sprites::from_loaded(&list)
}

/// Building pictures of a stand-in install: every type with its footprints (the ten bridge
/// pieces of each bridge type among them).
fn stand_in_pictures() -> Pictures {
    let sizes: [&[(u8, u8)]; 15] = [
        &[(7, 7), (6, 6), (8, 8)],
        &[(5, 5), (4, 4), (3, 3), (5, 5), (4, 4)],
        &[(4, 4), (4, 4), (5, 5), (4, 4)],
        &[(2, 2), (2, 2), (1, 1), (3, 3)],
        &[(2, 2), (3, 3)],
        &[(2, 2), (2, 2)],
        &[(2, 2), (3, 3), (1, 1)],
        &[(2, 2), (1, 1)],
        &[(4, 3), (3, 4)],
        &[(2, 2), (1, 1)],
        &[(2, 2)],
        &[(2, 2), (3, 3), (1, 1), (2, 2), (4, 4), (2, 2), (3, 3), (2, 2), (3, 3)],
        &[(3, 3), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1), (1, 1)],
        &[(1, 1); 10],
        &[],
    ];
    let mut list = Vec::new();
    for (k, pics) in sizes.iter().enumerate() {
        for (v, &size) in pics.iter().enumerate() {
            list.push(BuildingPicture { picture_type: k as u8 + 1, variant: v as u8, size });
        }
    }
    Pictures::from_list(list)
}

/// Units of a stand-in install: the base ids 1 to 100, priced and themed like the install's
/// lists (the themes name ids the alias table has).
fn stand_in_units() -> Units {
    let mut facts = HashMap::new();
    for id in 1..=100u32 {
        let k = id as i32;
        let st = StatBlock {
            hits: 20 + 7 * (k % 13),
            attack_blow: if k % 3 == 0 { 0 } else { 5 + 3 * (k % 11) },
            attack_shot: if k % 3 == 0 { 8 + 2 * (k % 7) } else { 0 },
            magic_power: if k % 5 == 0 { 10 + k % 9 } else { 0 },
            defence_blow: 2 + k % 6,
            defence_shot: 1 + k % 5,
            initiative: 5 + k % 10,
            manevres: 1 + (k % 4 == 0) as i32,
            regen: k % 3,
            ..StatBlock::default()
        };
        let value = super::worldgen::builder::strength(&st, 0, 60);
        facts.insert(id, UnitFacts { price: 20 + 9 * k, value, role: st.role() });
    }
    let themes = [
        (4..=42).chain([81, 83, 84]).collect(),
        vec![26, 27, 29, 30, 31, 32, 33, 17, 5],
        vec![59, 60, 61],
        (59..=71).collect(),
        vec![64, 67, 87, 88],
        (43..=58).collect(),
        vec![6, 7, 9, 10, 11, 12, 13, 14, 15, 17, 18, 21, 22, 23, 32, 33, 35],
        vec![47, 48, 49, 50, 96],
    ];
    Units::new(facts, themes, 2)
}

fn spell_prices() -> Vec<i32> {
    (1..=40).map(|k| 50 * k).collect()
}

fn request(size: u32, kind: u8, seed: i32, world: bool) -> Request {
    Request {
        map: newmap::Options { size, kind, orient: (kind % 4), ..newmap::Options::default() },
        seed,
        clock: super::clock_of(seed),
        world: world.then(|| World { buildings: true, economy: true, armies: true, options: super::worldgen::Options::new(size) }),
    }
}

/// Generates with the stand-in install's data.
fn run(req: &Request) -> Result<Generated, Failure> {
    let (sprites, pictures, units, prices) = (stand_in_sprites(), stand_in_pictures(), stand_in_units(), spell_prices());
    let src = Sources { sprites: &sprites, pictures: &pictures, units: &units, spell_prices: &prices };
    generate(req, &src, &AtomicBool::new(false))
}

/// A package folder holding one generated map (`maps/<id>/`) and its texts: a manifest, the
/// English and Russian texts (the same words in both).
fn package_dir(map: &PackageMap, id: &str, tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("razdor-gen-{tag}-{}-{nanos}", std::process::id()));
    let manifest = format!(
        "id = \"{id}\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"package.title\"\ndescription = \"package.description\"\nauthor = \"\"\nlicense = \"MIT\"\nbase = \"community-update\"\nrequires = []\nrules = \"dt\"\n"
    );
    let texts = format!("[package]\ntitle = \"Generated\"\ndescription = \"A generated map.\"\n{}", super::to_package::texts_toml(&map.texts));
    let map_path = format!("maps/{id}/map.toml");
    let std_files: [(&str, Vec<u8>); 4] = [
        ("package.toml", manifest.into_bytes()),
        ("texts/en.toml", texts.clone().into_bytes()),
        ("texts/ru.toml", texts.into_bytes()),
        (map_path.as_str(), map.toml.clone().into_bytes()),
    ];
    for (path, bytes) in std_files {
        crate::fsutil::write_atomically(&dir.join(path), &bytes).unwrap();
    }
    crate::fsutil::write_atomically(&dir.join(format!("maps/{id}/terrain.png")), &map.png).unwrap();
    dir
}

/// The package checker's report on one generated map.
fn check_map(map: &PackageMap, id: &str, tag: &str) -> (usize, String) {
    let dir = package_dir(map, id, tag);
    let files = Files::from_dir(&dir).unwrap();
    let (pkg, parse) = Package::parse(files);
    let r = crate::pack::check::check(&pkg);
    let text = format!("{}{}", parse.to_text(), r.to_text());
    let errors = parse.errors() + r.errors();
    let _ = std::fs::remove_dir_all(&dir);
    (errors, text)
}

fn all_checks() -> Vec<(u8, u32)> {
    (0..8u8).flat_map(|kind| [50u32, 100, 200].map(|size| (kind, size))).collect()
}

#[test]
fn the_same_seed_gives_the_same_package_map() {
    let aliases = unit_aliases();
    let req = request(50, newmap::LAND, 4242, true);
    let a = run(&req).expect("generates");
    let b = run(&req).expect("generates");
    let pa = to_package(&a, "gen-a", "T", &aliases).expect("a package map");
    let pb = to_package(&b, "gen-a", "T", &aliases).expect("a package map");
    assert_eq!(pa.png, pb.png, "terrain.png");
    assert_eq!(pa.toml, pb.toml, "map.toml");
    assert_eq!(pa.texts, pb.texts);
    let other = run(&request(50, newmap::LAND, 4243, true)).expect("generates");
    assert_ne!(to_package(&other, "gen-a", "T", &aliases).unwrap().png, pa.png, "another seed gives another map");
}

#[test]
fn seed_zero_and_sizes_the_package_lacks_are_refused() {
    assert_eq!(run(&request(50, newmap::LAND, 0, false)).unwrap_err(), Failure::ZeroSeed);
    // The generator makes 400 (and 800) maps; the package holds only 50, 100 and 200.
    let big = run(&request(400, newmap::LAND, 7, false)).expect("generates");
    assert_eq!(to_package(&big, "big", "Big", &unit_aliases()).unwrap_err(), super::to_package::Refusal::Size(400));
}

#[test]
fn every_map_type_at_three_sizes_passes_the_package_checker() {
    let aliases = unit_aliases();
    for (kind, size) in all_checks() {
        let g = run(&request(size, kind, 1000 + kind as i32 * 7 + size as i32, true)).unwrap_or_else(|e| panic!("kind {kind} size {size}: {e:?}"));
        let map = to_package(&g, "gen-map", "Generated", &aliases).unwrap_or_else(|e| panic!("kind {kind} size {size}: {e:?}"));
        let (errors, text) = check_map(&map, "gen-map", &format!("k{kind}s{size}"));
        assert_eq!(errors, 0, "kind {kind} size {size}:\n{text}");
        // The hero's start and the victory cell are reached; a building or army across water
        // without a bridge is a warning of its own (the original's roads may leave one).
        for line in text.lines().filter(|l| l.contains("cannot be reached")) {
            assert!(line.contains("building `") || line.contains("army `"), "kind {kind} size {size}: {line}");
        }
    }
}

#[test]
fn the_world_steps_write_buildings_and_armies_the_format_holds() {
    let aliases = unit_aliases();
    let g = run(&request(100, newmap::LAND, 99, true)).expect("generates");
    assert!(!g.scenario.buildings.is_empty(), "buildings");
    let map = to_package(&g, "gen-map", "Generated", &aliases).expect("a package map");
    assert!(map.toml.contains("[[building]]"));
    assert!(map.toml.contains("[victory]\nkind = \"reach\""));
    assert!(map.hero != (0, 0));
    assert!(map.texts.iter().any(|(k, _)| k.ends_with(".title")));
}

#[test]
fn a_map_without_the_world_steps_is_still_a_package_map() {
    let aliases = unit_aliases();
    let g = run(&request(50, newmap::ISLAND, 5, false)).expect("generates");
    assert!(g.scenario.buildings.is_empty());
    let map = to_package(&g, "gen-island", "Island", &aliases).expect("a package map");
    let (errors, text) = check_map(&map, "gen-island", "island");
    assert_eq!(errors, 0, "{text}");
}

/// The install's pinned numbers (`docs/reference/editor/worldgen.md` tests, the original's
/// counts for the new-map seed 2026 on a 200 land map): the counters, the buildings and armies.
#[test]
fn the_install_gives_the_pinned_counts() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(Path::new(&dir)).expect("install loads");
    let data = super::InstallData::from_install(&dt).expect("the install's data");
    let src = data.sources();
    let req = Request {
        map: newmap::Options { size: 200, kind: newmap::LAND, keep: true, rebuild: true, ..newmap::Options::default() },
        seed: 2026,
        // The clock of the install's pinned world (worldgen.md tests: Randomize at 4242).
        clock: 4242,
        world: Some(World { buildings: true, economy: true, armies: true, options: super::worldgen::Options::new(200) }),
    };
    let g = generate(&req, &src, &AtomicBool::new(false)).expect("generates");
    let c = g.steps[0].1.counters;
    assert_eq!(([c.villages, c.castles, c.churches, c.ruins], g.scenario.buildings.len(), g.scenario.armies.len()), ([7, 8, 1, 12], 28, 18));
    let aliases = unit_aliases();
    let map = to_package(&g, "pinned", "Pinned", &aliases).expect("a package map");
    let (errors, text) = check_map(&map, "pinned", "pinned");
    assert_eq!(errors, 0, "{text}");
    // The package compiles against the install.
    let dir = package_dir(&map, "pinned", "pinned-compile");
    let (pkg, _) = Package::parse(Files::from_dir(&dir).unwrap());
    let compiled = crate::pack::compile::compile(&pkg, &dt, "en", "pinned");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(compiled.is_ok(), "{:?}", compiled.err().map(|r| r.to_text()));
}


/// A copy of the example package in a temporary folder, removed when dropped.
struct ExampleCopy(PathBuf);

impl ExampleCopy {
    fn of(name: &str) -> ExampleCopy {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-gen-ws-{}-{nanos}", std::process::id()));
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs").join(name), &dir);
        ExampleCopy(dir)
    }
}

impl Drop for ExampleCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn a_generated_map_is_one_undo_step_on_a_copy_of_pepelnaya_marka() {
    use crate::studio::Workspace;
    let copy = ExampleCopy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let before = ws.entries.clone();
    let texts_before = ws.read_text("texts/en.toml").unwrap();
    let campaign_before = ws.read_text("campaign.toml").unwrap();
    let g = run(&request(50, newmap::LAND, 77, true)).expect("generates");
    let map = to_package(&g, "gen-test", "Test map", &unit_aliases()).expect("a package map");
    super::apply::add_map(&mut ws, "gen-test", &map, true).expect("added");
    assert!(ws.entries.iter().any(|e| e == "maps/gen-test/map.toml"));
    assert!(ws.entries.iter().any(|e| e == "maps/gen-test/terrain.png"));
    // The same id again is refused, and nothing changes.
    let entries = ws.entries.clone();
    assert!(super::apply::add_map(&mut ws, "gen-test", &map, false).is_err());
    assert_eq!(ws.entries, entries);
    ws.save_all().unwrap();
    assert!(copy.0.join("maps/gen-test/terrain.png").is_file());
    assert!(ws.read_text("texts/en.toml").unwrap().contains("gen-test"));
    assert!(ws.read_text("campaign.toml").unwrap().contains("gen-test"));
    // One undo takes the files, the texts and the campaign entry back.
    assert_eq!(ws.undo().unwrap().as_deref(), Some("New map gen-test"));
    assert!(!copy.0.join("maps/gen-test").exists());
    assert_eq!(ws.entries, before);
    // The undone texts and campaign are the open documents' again; saving writes them back.
    ws.save_all().unwrap();
    assert_eq!(ws.read_text("texts/en.toml").unwrap(), texts_before);
    assert_eq!(ws.read_text("campaign.toml").unwrap(), campaign_before);
}
