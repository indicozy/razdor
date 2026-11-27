//! Army bodies (M4, spec §3.8): the CREA records of the armies' leaders and of every unit card, the
//! placeholder stats (Q3), and where each army starts. Armies are not placed offline: Lua makes the
//! bodies from these records (spec §2.1), so this module writes records and numbers only.
//!
//! Record ids: a leader has its own record `rz_army_<id>` (its name is the army's); a unit card is
//! `rz_u<unit>_l<level>`, shared by every army that has it. The CREA layout is the one of OpenMW's
//! own `landracer.omwaddon` (NAME, MODL, FNAM, NPDT 96 B, FLAG, AIDT 12 B), checked with `esmtool`.

use crate::grid::Grid;
use crate::heightmap::HeightField;
use crate::input::Map;
use crate::placement;
use crate::tes3::{self, Record};
use crate::units::{Unit, Units};
use std::collections::BTreeMap;

/// The placeholder body mesh, `meshes/razdor/figure.dae` (spec §3.8, `mesh::figure`).
pub const FIGURE_MODEL: &str = "razdor/figure.dae";
/// The plan's mesh name of the ship raft (STAT `razdor_raft`).
pub const RAFT_MESH: &str = "raft";
/// The distance between an army's bodies, in units: they stand on a grid of this spacing around the
/// leader, so a squad reads as several units (M4 follow-up, spec §3.8).
pub const BODY_SPACING: i64 = 300;
/// CREA flag "Walks" (0x40), as the landracer has it.
pub const FLAG_WALKS: i32 = 0x40;

/// The placeholder stats of one body (spec §3.8, Q3). `level` is the CREA level (1 + the card's level).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub level: i64,
    /// Strength, intelligence, willpower, agility, speed, endurance, personality, luck.
    pub attributes: [i64; 8],
    pub health: i64,
    pub fatigue: i64,
    pub combat: i64,
    /// Magic skill and magicka (0 for the placeholder stats; the units JSON gives them, see [`real_stats`]).
    pub magic: i64,
    pub magicka: i64,
    /// Three attack pairs (minimum, maximum).
    pub attack: [(i64, i64); 3],
}

/// The CREA level of a unit card of level `unit_level`: `1 + unit_level` (spec §3.8).
pub fn body_level(unit_level: i64) -> i64 {
    1 + unit_level
}

/// Health of a body of `count` actors-worth of health: `(30 + 20·level) · sqrt(count)`. M4 writes
/// `count = 1` into the records, because every unit card is N separate bodies (spec Q5); the
/// `count` argument is kept for the formula's tests and for a later per-squad rule.
pub fn health(level: i64, count: i64) -> i64 {
    ((30 + 20 * level) as f64 * (count.max(1) as f64).sqrt()).round() as i64
}

/// The placeholder stats for a body of CREA level `level` (spec §3.8): attack `4 + 3·level` to
/// `8 + 6·level`, combat skill `40 + 10·level` capped at 100, every attribute `30 + 5·level` capped at 100.
pub fn stats(level: i64) -> Stats {
    let attr = (30 + 5 * level).min(100);
    Stats {
        level,
        attributes: [attr; 8],
        health: health(level, 1),
        fatigue: 100,
        combat: (40 + 10 * level).min(100),
        magic: 0,
        magicka: 0,
        attack: [(4 + 3 * level, 8 + 6 * level); 3],
    }
}

/// The real stats of a body of unit `u` at the map's 0-based troop level `raw` (the units JSON, spec
/// §3.8 "units notes"; CREA level = `raw + 1`, the game's level). Hit points → health; the melee
/// attack → the first attack pair and strength; the ranged attack → the second pair; the magic power →
/// the third pair, intelligence, magicka and the magic skill (0 without a school); combat skill = the
/// larger of melee and ranged attack; willpower = the average protection; agility = melee plus ranged
/// defence; speed = 3 × initiative (10..100); endurance, luck and personality are 50. Each attack pair
/// is (half the value rounded down, at least 1; the value), or (0, 0) for none. Every number is at the
/// unit's level (its growth applied). Attributes and skills are capped at 0..100.
pub fn real_stats(u: &Unit, raw: i64) -> Stats {
    let blow = u.attack_blow_at(raw).max(0);
    let shot = u.attack_shot_at(raw).max(0);
    let power = u.magic_power_at(raw).max(0);
    let pair = |v: i64| if v > 0 { ((v / 2).max(1), v) } else { (0, 0) };
    let protection = (u.protection_at("life", raw) + u.protection_at("death", raw) + u.protection_at("elemental", raw)) / 3;
    let cap = |v: i64| v.clamp(0, 100);
    Stats {
        level: raw + 1,
        attributes: [
            cap(blow),
            cap(power),
            cap(protection),
            cap(u.defence_blow_at(raw) + u.defence_shot_at(raw)),
            cap(3 * u.initiative_at(raw)).max(10),
            50,
            50,
            50,
        ],
        health: u.hit_points_at(raw).max(1),
        fatigue: 100,
        combat: cap(blow.max(shot)),
        magic: cap(power),
        magicka: power,
        attack: [pair(blow), pair(shot), pair(power)],
    }
}

/// The CREA name of unit `u`: its name, or `Unit <id>` for an empty name.
fn unit_name(u: &Unit) -> String {
    if u.name.is_empty() {
        format!("Unit {}", u.id)
    } else {
        u.name.clone()
    }
}

/// The record id of a unit card (spec §3.8): `rz_u<unit>_l<level>`.
pub fn unit_id(unit: i64, level: i64) -> String {
    format!("rz_u{unit}_l{level}")
}

/// The record id of an army's leader: `rz_army_<id>`.
pub fn leader_id(army: i64) -> String {
    format!("rz_army_{army}")
}

/// One CREA record: NPDT (96 B), FLAG, AIDT (12 B). Fight, flee and alarm are 0 in M4: the attitude
/// of an army is not settled (spec §3.8, open point), so every body is passive.
pub fn crea(id: &str, name: &str, model: &str, s: &Stats) -> Record {
    let mut npdt = Vec::with_capacity(96);
    let mut put = |v: i64| npdt.extend_from_slice(&(v as i32).to_le_bytes());
    put(0); // type: creature
    put(s.level);
    for a in s.attributes {
        put(a);
    }
    put(s.health);
    put(s.magicka);
    put(s.fatigue);
    put(0); // soul
    put(s.combat);
    put(s.magic);
    put(0); // stealth
    for (lo, hi) in s.attack {
        put(lo);
        put(hi);
    }
    put(0); // gold
    let mut aidt = vec![0u8; 12];
    aidt[0..2].copy_from_slice(&0i16.to_le_bytes()); // hello
    // bytes 2..5 fight, flee, alarm (0), 5..8 unknown (0), 8..12 services (0)
    Record::new(b"CREA")
        .sub(b"NAME", tes3::zstring(id))
        .sub(b"MODL", tes3::zstring(model))
        .sub(b"FNAM", tes3::zstring(name))
        .sub(b"NPDT", npdt)
        .sub(b"FLAG", FLAG_WALKS.to_le_bytes().to_vec())
        .sub(b"AIDT", aidt)
}

/// One army as Lua reads it (spec §3.8, §3.9): its tile, where its bodies start, and its units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmyOut {
    pub id: i64,
    pub name: String,
    pub faction: i64,
    pub relations: Vec<i64>,
    /// Whether the army is hostile to the player: `info.relations[0][faction - 1] < 0` (spec §3.8).
    pub hostile: bool,
    /// The home tile `(x, y)`, DT's coordinates (y from the north).
    pub tile: (i64, i64),
    /// Where the leader starts, world units. The home tile when it is open, else the nearest open
    /// tile outside every building box (spec §3.8, "blocked tiles"); a ship stays on its water tile.
    pub spawn: [i64; 3],
    pub relocated: bool,
    pub ship: bool,
    pub inactive: bool,
    pub patrols: bool,
    pub patrol_radius: i64,
    pub behaviour: i64,
    pub home_building: i64,
    pub respawn_days: i64,
    /// `(record id, count, is leader)`: the leader first (count 1), then every unit card.
    pub units: Vec<(String, i64, bool)>,
}

/// Everything the actor part of the plugin and of `map_data.lua` needs.
#[derive(Debug, Default)]
pub struct ArmyPlan {
    pub armies: Vec<ArmyOut>,
    /// One CREA per army leader and one per unit card, sorted by id (each id once).
    pub records: Vec<Record>,
    /// Ship rafts: the water tile centres (z = 0) where an army has a ship.
    pub rafts: Vec<[i64; 3]>,
}

/// Whether the tile `(x, y)` (DT's coordinates) is open on foot and outside every building box.
fn open_point(passable: &[u8], grid: &Grid, boxes: &[[i64; 4]], wx: i64, wy: i64) -> bool {
    let Some((x, y)) = grid.tile_at(wx, wy) else { return false };
    if passable[(y * grid.width + x) as usize] == 0 {
        return false;
    }
    !boxes.iter().any(|b| wx >= b[0] && wx <= b[1] && wy >= b[2] && wy <= b[3])
}

/// The nearest open tile centre to tile `(x, y)`, searched in square rings of growing radius; ties on
/// a ring go by the scan order (dy, then dx), so the result is the same on every run.
fn nearest_open(passable: &[u8], grid: &Grid, boxes: &[[i64; 4]], x: i64, y: i64) -> Option<(i64, i64)> {
    let max = grid.width.max(grid.height);
    for d in 0..=max {
        for dy in -d..=d {
            for dx in -d..=d {
                if dx.abs().max(dy.abs()) != d {
                    continue;
                }
                let (tx, ty) = (x + dx, y + dy);
                if tx < 0 || ty < 0 || tx >= grid.width || ty >= grid.height {
                    continue;
                }
                let (cx, cy) = grid.tile_centre(tx, ty);
                if open_point(passable, grid, boxes, cx, cy) {
                    return Some((cx, cy));
                }
            }
        }
    }
    None
}

/// Plans the armies of a map: the records, and for each army its tile, spawn point and units.
/// `codes` and `passable` are the map's surface codes and on-foot values (row-major, row 0 north),
/// `boxes` the building footprints from the placement plan, `model` the body mesh (`MODL`).
/// The player is faction row 0 of `info.relations`; an army of faction `f` is hostile when the
/// player's row holds a negative value in column `f - 1` (spec §3.8, the attitude rule).
pub fn hostile(map: &Map, faction: i64) -> bool {
    let column = usize::try_from(faction - 1).ok();
    match (map.info.relations.first(), column) {
        (Some(row), Some(c)) => row.get(c).is_some_and(|v| *v < 0),
        _ => false,
    }
}

/// Plans the armies of a map (see the module docs). With `units` (the `--units` JSON), a unit card
/// takes its real name and stats at its level and a leader takes its unit's stats and the army
/// leader's name (the army's name when the leader has none); a unit missing from the file keeps the
/// placeholder stats and the name `Unit <id>`. Without `units` the placeholder formula is used (M4).
pub fn plan_armies(
    map: &Map,
    grid: &Grid,
    heights: &HeightField,
    codes: &[u8],
    passable: &[u8],
    boxes: &[[i64; 4]],
    model: &str,
    units: Option<&Units>,
) -> ArmyPlan {
    let mut records: BTreeMap<String, Record> = BTreeMap::new();
    let mut plan = ArmyPlan::default();
    let unit_record = |records: &mut BTreeMap<String, Record>, unit: i64, level: i64| -> String {
        let id = unit_id(unit, level);
        records.entry(id.clone()).or_insert_with(|| match units.and_then(|us| us.get(unit)) {
            Some(u) => crea(&id, &unit_name(u), model, &real_stats(u, level)),
            None => crea(&id, &format!("Unit {unit}"), model, &stats(body_level(level))),
        });
        id
    };

    for a in &map.armies {
        let (tx, ty) = (a.x, a.y);
        let (hx, hy) = grid.tile_centre(tx, ty);
        let k = (ty * grid.width + tx) as usize;
        let water = codes.get(k).is_some_and(|&c| c <= 2);
        let ship = a.ship != 0 || water;
        let (spawn, relocated) = if ship {
            ([hx, hy, 0], false)
        } else if open_point(passable, grid, boxes, hx, hy) {
            ([hx, hy, placement::ground(heights, hx, hy)], false)
        } else {
            match nearest_open(passable, grid, boxes, tx, ty) {
                Some((x, y)) => ([x, y, placement::ground(heights, x, y)], true),
                None => ([hx, hy, placement::ground(heights, hx, hy)], true),
            }
        };
        if ship {
            plan.rafts.push([hx, hy, 0]);
        }

        // The leader has its own record, named after the army (spec §3.8).
        let leader = leader_id(a.id);
        let leader_unit = units.and_then(|us| us.get(a.leader_unit));
        let (leader_name, leader_stats) = match (units, leader_unit) {
            (Some(_), Some(u)) => {
                let name = if !a.leader_name.is_empty() {
                    a.leader_name.clone()
                } else if !a.name.is_empty() {
                    a.name.clone()
                } else {
                    unit_name(u)
                };
                (name, real_stats(u, a.leader_level))
            }
            _ => {
                let name = if a.name.is_empty() { a.leader_name.clone() } else { a.name.clone() };
                (name, stats(body_level(a.leader_level)))
            }
        };
        records.insert(leader.clone(), crea(&leader, &leader_name, model, &leader_stats));
        let mut units = vec![(leader, 1, true)];
        for t in &a.troops {
            let id = unit_record(&mut records, t.unit, t.level);
            if t.count > 0 {
                units.push((id, t.count, false));
            }
        }
        plan.armies.push(ArmyOut {
            id: a.id,
            name: a.name.clone(),
            faction: a.faction,
            relations: a.relations.clone(),
            hostile: hostile(map, a.faction),
            tile: (tx, ty),
            spawn,
            relocated,
            ship,
            inactive: a.inactive != 0,
            patrols: a.patrols != 0 && !ship,
            patrol_radius: a.patrol_radius,
            behaviour: a.behaviour,
            home_building: a.home_building,
            respawn_days: a.respawn_days,
            units,
        });
    }
    plan.records = records.into_values().collect();
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stat_formula_matches_spec_3_8() {
        // unit level 0 gives CREA level 1: health 50, attack 7..14, combat 50.
        let s = stats(body_level(0));
        assert_eq!(s.level, 1);
        assert_eq!(s.health, 50);
        assert_eq!(s.attack[0], (7, 14));
        assert_eq!(s.combat, 50);
        // level 3 (unit level 2): health 90, attack 13..26, combat 70.
        let s = stats(body_level(2));
        assert_eq!((s.health, s.attack[0], s.combat), (90, (13, 26), 70));
        // combat is capped at 100 and attributes too.
        let s = stats(10);
        assert_eq!((s.combat, s.attributes[0]), (100, 80));
    }

    #[test]
    fn health_scales_with_the_square_root_of_the_count() {
        assert_eq!(health(1, 1), 50);
        assert_eq!(health(1, 4), 100);
        assert_eq!(health(2, 9), 210);
    }

    #[test]
    fn unit_and_leader_ids_are_lower_case_and_fixed() {
        assert_eq!(unit_id(65, 0), "rz_u65_l0");
        assert_eq!(leader_id(9), "rz_army_9");
    }

    /// A synthetic map with the given surface codes (row 0 north), armies and buildings.
    fn map_with(w: usize, h: usize, codes: &[u8], armies: serde_json::Value, buildings: serde_json::Value) -> Map {
        let rows: Vec<String> = (0..h).map(|y| codes[y * w..(y + 1) * w].iter().map(|c| format!("{c:x}")).collect()).collect();
        let text = serde_json::json!({
            "format": crate::input::FORMAT,
            "version": crate::input::VERSION,
            "width": w,
            "height": h,
            "info": { "generator_seed": 7 },
            "heroes": [{ "archetype": "knight", "x": 0, "y": 0 }],
            "terrain": { "encoding": "hex-rows", "rows": rows },
            "buildings": buildings,
            "armies": armies,
        })
        .to_string();
        Map::from_json(&text).unwrap()
    }

    fn army(id: i64, x: i64, y: i64, extra: serde_json::Value) -> serde_json::Value {
        let mut v = serde_json::json!({
            "id": id, "x": x, "y": y, "name": format!("Army {id}"), "leader_unit": 13, "leader_level": 0,
            "troops": [{ "unit": 4, "level": 0, "count": 2 }, { "unit": 19, "level": 1, "count": 1 }],
            "faction": 2, "relations": [2, 0, 0, 0], "patrols": 1, "patrol_radius": 3,
        });
        for (k, val) in extra.as_object().unwrap() {
            v[k] = val.clone();
        }
        v
    }

    fn plan_of(map: &Map, codes: &[u8], w: usize, h: usize, boxes: &[[i64; 4]]) -> ArmyPlan {
        let grid = Grid::new(w, h, 1024).unwrap();
        let heights = crate::heightmap::HeightField::from_fn(crate::heightmap::bounds(&grid), |_, _| 0);
        let passable = crate::passable::build(map, &grid, codes);
        plan_armies(map, &grid, &heights, codes, &passable, boxes, FIGURE_MODEL, None)
    }

    #[test]
    fn an_army_expands_into_its_leader_and_unit_cards_with_counts() {
        let codes = vec![5u8; 16];
        let map = map_with(4, 4, &codes, serde_json::json!([army(1, 2, 2, serde_json::json!({}))]), serde_json::json!([]));
        let plan = plan_of(&map, &codes, 4, 4, &[]);
        let a = &plan.armies[0];
        assert_eq!(a.units, vec![
            ("rz_army_1".to_string(), 1, true),
            ("rz_u4_l0".to_string(), 2, false),
            ("rz_u19_l1".to_string(), 1, false),
        ]);
        assert_eq!(a.tile, (2, 2));
        assert!(!a.relocated && !a.ship);
        let ids: Vec<String> = plan.records.iter().map(|r| String::from_utf8_lossy(&r.get(b"NAME").unwrap()[..r.get(b"NAME").unwrap().len() - 1]).into_owned()).collect();
        assert_eq!(ids, vec!["rz_army_1", "rz_u19_l1", "rz_u4_l0"], "one record per id, sorted");
    }

    #[test]
    fn a_crea_record_holds_the_placeholder_stats_in_the_landracer_layout() {
        let s = stats(body_level(1)); // CREA level 2
        let r = crea("rz_u4_l1", "Unit 4", FIGURE_MODEL, &s);
        let npdt = r.get(b"NPDT").unwrap();
        let field = |i: usize| i32::from_le_bytes(npdt[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(npdt.len(), 96);
        assert_eq!(field(0), 0, "creature");
        assert_eq!(field(1), 2, "level");
        assert_eq!(field(10), 70, "health 30 + 20 * 2");
        assert_eq!(field(14), 60, "combat 40 + 10 * 2");
        assert_eq!((field(17), field(18)), (10, 20), "attack 4 + 3*2 .. 8 + 6*2");
        assert_eq!(r.get(b"FLAG").unwrap(), &0x40i32.to_le_bytes()[..]);
        assert_eq!(r.get(b"AIDT").unwrap().len(), 12);
        assert_eq!(r.get(b"FNAM").unwrap(), &b"Unit 4\0"[..]);
    }

    #[test]
    fn a_ship_army_stands_on_its_water_tile_at_height_zero_and_does_not_patrol() {
        // Tile (1, 0) is shallows (code 0): a ship army there is spawned on the water and not patrolling.
        let codes = vec![5u8, 0, 5, 5];
        let map = map_with(2, 2, &codes, serde_json::json!([army(9, 1, 0, serde_json::json!({ "ship": 2 }))]), serde_json::json!([]));
        let plan = plan_of(&map, &codes, 2, 2, &[]);
        let a = &plan.armies[0];
        assert!(a.ship && !a.patrols && !a.relocated);
        assert_eq!(a.spawn[2], 0);
        assert_eq!(plan.rafts.len(), 1);
        assert_eq!(plan.rafts[0][2], 0);
    }

    #[test]
    fn a_blocked_home_tile_moves_the_spawn_to_the_nearest_open_tile_outside_the_boxes() {
        // Row 0 is north. Tile (1, 1) is swamp (code 9, closed); the nearest open tile is one step away.
        let codes = vec![5u8, 5, 5, 5, 9, 5, 5, 5, 5];
        let map = map_with(3, 3, &codes, serde_json::json!([army(2, 1, 1, serde_json::json!({}))]), serde_json::json!([]));
        let plan = plan_of(&map, &codes, 3, 3, &[]);
        let a = &plan.armies[0];
        assert!(a.relocated, "a closed home tile is relocated");
        assert_eq!(a.tile, (1, 1), "the home tile stays for events");
        let grid = Grid::new(3, 3, 1024).unwrap();
        let (tx, ty) = grid.tile_at(a.spawn[0], a.spawn[1]).unwrap();
        assert!(codes[(ty * 3 + tx) as usize] != 9, "spawn is open");
        assert_eq!(tx.abs_diff(1).max(ty.abs_diff(1)), 1, "one step away: {tx},{ty}");
    }

    #[test]
    fn the_army_plan_is_the_same_on_every_run() {
        let codes = vec![5u8; 16];
        let armies = serde_json::json!([army(1, 2, 2, serde_json::json!({})), army(3, 0, 3, serde_json::json!({ "inactive": 1 }))]);
        let map = map_with(4, 4, &codes, armies, serde_json::json!([]));
        let a = plan_of(&map, &codes, 4, 4, &[]);
        let b = plan_of(&map, &codes, 4, 4, &[]);
        assert_eq!(a.armies, b.armies);
        let ra: Vec<Vec<u8>> = a.records.iter().map(|r| tes3::encode(std::slice::from_ref(r))).collect();
        let rb: Vec<Vec<u8>> = b.records.iter().map(|r| tes3::encode(std::slice::from_ref(r))).collect();
        assert_eq!(ra, rb);
    }

    #[test]
    fn a_building_box_keeps_the_spawn_out_of_it() {
        let codes = vec![5u8; 16];
        // The home tile centre (2.5 * 1024, 1.5 * 1024 from the south row) is inside the box below.
        let grid = Grid::new(4, 4, 1024).unwrap();
        let (hx, hy) = grid.tile_centre(2, 2);
        let boxes = [[hx - 100, hx + 100, hy - 100, hy + 100]];
        let map = map_with(4, 4, &codes, serde_json::json!([army(1, 2, 2, serde_json::json!({}))]), serde_json::json!([]));
        let plan = plan_of(&map, &codes, 4, 4, &boxes);
        let a = &plan.armies[0];
        assert!(a.relocated);
        let s = a.spawn;
        assert!(!(s[0] >= boxes[0][0] && s[0] <= boxes[0][1] && s[1] >= boxes[0][2] && s[1] <= boxes[0][3]));
    }

    /// The units file of the synthetic tests: unit 13 (a knight), unit 4 (a militiaman) and unit 19
    /// (a mage), with the knight's melee attack `blow`. No game data.
    fn units_text(blow: i64) -> String {
        let unit = |id: i64, name: &str, hp: i64, blow: i64, shot: i64, power: i64, school: Option<&str>| {
            serde_json::json!({
                "id": id, "name": name, "cost": 1, "hit_points": hp,
                "attack": {"blow": blow, "shot": shot}, "defence": {"blow": 10, "shot": 4},
                "magic_power": power, "magic": school, "magic_direction": null,
                "protection": {"life": 20, "death": 20, "elemental": 20}, "initiative": 10,
                "actions": 1, "regeneration": 0, "vampirism": 0, "ranged": shot > 0, "flying": false,
                "nature": "normal", "bonus": null, "evasion": null, "min_magic_power": null,
                "mana_drain": null, "experience": {"start": 60, "growth_percent": 140},
                "level_up": {"hit_points": 5, "attack_blow": 2}
            })
        };
        serde_json::json!({
            "format": crate::units::FORMAT, "version": crate::units::VERSION, "language": "ru", "count": 3,
            "units": [
                unit(13, "Рыцарь", 80, blow, 0, 0, None),
                unit(4, "Ополченец", 50, 25, 0, 0, None),
                unit(19, "Маг", 40, 0, 0, 20, Some("elemental")),
            ]
        })
        .to_string()
    }

    fn synthetic_units() -> Units {
        Units::from_json(&units_text(40)).unwrap()
    }

    #[test]
    fn real_stats_follow_the_units_mapping_at_the_level() {
        let units = synthetic_units();
        let knight = units.get(13).unwrap();
        // Map level 0 (as hired): CREA level 1, hit points 80, melee 40 -> attack (20, 40).
        let s = real_stats(knight, 0);
        assert_eq!((s.level, s.health, s.attack[0]), (1, 80, (20, 40)));
        assert_eq!(s.attack[1], (0, 0), "no ranged attack");
        assert_eq!(s.attack[2], (0, 0), "no magic");
        assert_eq!((s.attributes[0], s.attributes[3], s.attributes[4]), (40, 14, 30), "strength, agility, speed");
        assert_eq!((s.magic, s.magicka, s.combat), (0, 0, 40));
        // Map level 2 (CREA level 3): hit points 90, melee 44; no protection growth in this file.
        let s = real_stats(knight, 2);
        assert_eq!((s.level, s.health, s.attack[0].1), (3, 90, 44));
        assert_eq!(s.attributes[2], 20);
        // A mage: its magic power is the third pair, its intelligence and its magicka.
        let mage = real_stats(units.get(19).unwrap(), 0);
        assert_eq!((mage.attack[2], mage.attributes[1], mage.magicka, mage.magic), ((10, 20), 20, 20, 20));
        // Values are capped at 100.
        let big = Units::from_json(&units_text(200)).unwrap();
        let s = real_stats(big.get(13).unwrap(), 0);
        assert_eq!((s.attributes[0], s.combat), (100, 100));
    }

    #[test]
    fn with_units_a_unit_card_and_a_leader_take_the_real_names_and_stats() {
        let codes = vec![5u8; 16];
        let armies = serde_json::json!([army(1, 2, 2, serde_json::json!({ "leader_name": "Альберт" }))]);
        let map = map_with(4, 4, &codes, armies, serde_json::json!([]));
        let grid = Grid::new(4, 4, 1024).unwrap();
        let heights = crate::heightmap::HeightField::from_fn(crate::heightmap::bounds(&grid), |_, _| 0);
        let passable = crate::passable::build(&map, &grid, &codes);
        let units = synthetic_units();
        let plan = plan_armies(&map, &grid, &heights, &codes, &passable, &[], FIGURE_MODEL, Some(&units));
        let field = |r: &Record, sub: &[u8; 4]| -> Vec<u8> {
            let v = r.get(sub).unwrap();
            v[..v.len() - 1].to_vec()
        };
        let find = |id: &str| plan.records.iter().find(|r| field(r, b"NAME") == id.as_bytes()).unwrap();
        assert_eq!(crate::tes3::decode_cp1251(&field(find("rz_army_1"), b"FNAM")), "Альберт", "the leader's name");
        let card = find("rz_u4_l0");
        assert_eq!(crate::tes3::decode_cp1251(&field(card, b"FNAM")), "Ополченец", "a unit's Russian name");
        assert_eq!(&card.get(b"FNAM").unwrap()[..2], &crate::tes3::encode_cp1251("Оп")[..], "cp1251 bytes");
        let health = |r: &Record| i32::from_le_bytes(r.get(b"NPDT").unwrap()[40..44].try_into().unwrap());
        assert_eq!(health(find("rz_army_1")), 80, "the leader's unit hit points");
        assert_eq!(health(card), 50);
        // Without units the M4 names and formula stay.
        let plain = plan_armies(&map, &grid, &heights, &codes, &passable, &[], FIGURE_MODEL, None);
        assert!(plain.records.iter().any(|r| r.get(b"FNAM").unwrap() == &b"Unit 4\0"[..]));
    }
}
