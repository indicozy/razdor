//! The world generator against the spec's numbers, small hand-made cases of each stage, its
//! quirks and clean stops, and pinned worlds on maps of the new-map generator (the same
//! seed and options always give the same world).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use super::builder::{StatBlock, UnitFacts};
use super::*;
use crate::editor::newmap;
use crate::editor::palette::{BuildingPicture, ObjectKey};

/// A palette of our own shaped as the install's: hills, mountains, tree families, and
/// building pictures of every type (ten stone and wooden bridge pieces).
pub(crate) fn palette() -> Palette {
    let mut objects = Vec::new();
    let mut add = |class: u8, ids: &mut dyn Iterator<Item = u8>| objects.extend(ids.map(|sprite| ObjectKey { class, sprite }));
    add(1, &mut (10..=19).chain(20..=27).chain(30..=33).chain(40..=43).chain(50..=53).chain(60..=60));
    add(5, &mut (10..=13).chain(20..=23).chain(30..=33).chain(40..=43).chain(50..=53));
    add(9, &mut (0..=8).chain(12..=20).chain(24..=29).chain(36..=41).chain(48..=50).chain(60..=65).chain(120..=128));
    add(10, &mut (108..=116).chain(228..=236));
    add(11, &mut (0..=8).chain(12..=20).chain(24..=29).chain(36..=41).chain(108..=116));
    objects.sort();
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
    let mut buildings = Vec::new();
    for (k, list) in sizes.iter().enumerate() {
        for (v, &size) in list.iter().enumerate() {
            buildings.push(BuildingPicture { picture_type: k as u8 + 1, variant: v as u8, size, brush: size.0 });
        }
    }
    Palette { objects, buildings, from_install: true }
}

/// Units of our own: id → stats, priced and themed like the install's lists.
pub(crate) fn units() -> Units {
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
        let value = super::builder::strength(&st, 0, 60);
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

/// FNV-1a over everything the steps write.
fn hash(s: &Scenario) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(&s.terrain);
    for o in &s.objects {
        eat(&[o.x as u8, (o.x >> 8) as u8, o.y as u8, (o.y >> 8) as u8, o.class, o.sprite]);
    }
    for b in &s.buildings {
        eat(format!("{b:?}").as_bytes());
    }
    for a in &s.armies {
        eat(format!("{a:?}").as_bytes());
    }
    h
}

/// A map of the new-map generator: its scenario, marks and scratch flags.
fn generated(size: u32, kind: u8, seed: i32) -> (Scenario, CellLayer) {
    let o = newmap::Options { size, kind, keep: true, rebuild: true, ..newmap::Options::default() };
    let job = newmap::Job { options: o, seed, sprites: newmap::Sprites::from_palette(&palette()), start: newmap::Cells::zero(size), clock: 7 };
    let out = newmap::Generator::new().run(job, &AtomicBool::new(false));
    assert!(out.complete(), "{:?}", out.stop);
    let s = newmap::new_scenario(&out, &Scenario::default(), "t");
    let mut cells = CellLayer::load(&s);
    cells.set_marks(&out.cells.mark);
    cells.set_scratch(&out.cells.flag);
    (s, cells)
}

struct Kit {
    pictures: Pictures,
    units: Units,
    spells: Vec<i32>,
    names: NamePools,
}

impl Kit {
    fn new() -> Kit {
        Kit { pictures: Pictures::from_palette(&palette()), units: units(), spells: spell_prices(), names: crate::editor::naming::tests::pools() }
    }

    fn inputs(&self, clock: u32) -> Inputs<'_> {
        Inputs { pictures: &self.pictures, names: Some(&self.names), units: &self.units, spell_prices: &self.spells, brush: 1, clock }
    }
}

/// A map of our own: grass plain with a river (coastal water, a deep pool and a ford) down
/// the middle, a lake, lowland and marsh patches, copses and hills.
pub(crate) fn meadow(size: u32) -> (Scenario, CellLayer) {
    let n = size as i32;
    let mut s = crate::editor::defaults::new_scenario(crate::editor::NewMap { width: size, height: size, fill: 6 });
    let mut objects = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let i = (y * n + x) as usize;
            let river = (x - n / 2 - (y / 9) % 5).abs() <= 2;
            let lake = (x - n / 4).pow(2) + (y - 3 * n / 4).pow(2) < (n / 12).pow(2);
            s.terrain[i] = if river {
                if y % 37 < 3 { 0 } else if y % 53 == 7 { 2 } else { 1 }
            } else if lake {
                2
            } else if (x / 7 + y / 11) % 9 == 0 {
                5
            } else if (x / 13 + 2 * (y / 5)) % 17 == 0 {
                8
            } else if (x + y) % 23 == 0 {
                7
            } else {
                6
            };
            let wild = !river && !lake;
            if wild && ((x * 7 + y * 3) % 97 == 0 || ((x / 10) % 6 == 1 && (y / 10) % 5 == 2)) {
                objects.push(crate::dt::dtm::MapObject { x: x as u16, y: y as u16, class: 9, sprite: ((x + y) % 9) as u8 });
            }
            if wild && x % 31 == 5 && y % 27 == 9 {
                objects.push(crate::dt::dtm::MapObject { x: x as u16, y: y as u16, class: 1, sprite: 20 + (x % 2) as u8 });
            }
        }
    }
    s.objects = crate::editor::grid::rebuild_objects(&crate::dt::dtm::Scenario { objects, ..s.clone() });
    let cells = CellLayer::load(&s);
    (s, cells)
}

#[test]
fn worlds_on_generated_maps_are_pinned() {
    super::infra::VERIFY_COSTS.store(true, std::sync::atomic::Ordering::Relaxed);
    let kit = Kit::new();
    let cases = [("new-map 200", generated(200, newmap::LAND, 99), PIN_GENERATED), ("meadow 200", meadow(200), PIN_MEADOW)];
    for (name, (mut s, mut cells), (counters, want)) in cases {
        let o = Options::new(s.width());
        let mut rng = Rng::new(5);
        let r1 = buildings_and_roads(&mut s, &mut cells, &mut rng, &kit.inputs(1234), &o);
        let r2 = economy(&mut s, &mut rng, &kit.inputs(0), &o);
        let r3 = armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
        assert_eq!((r1.stop, r2.stop, r3.stop), (None, None, None), "{name}");
        let c = r1.counters;
        let got = [c.towns, c.villages, c.castles, c.forts, c.shipyards, c.bridges, c.taverns, c.churches, c.markets, c.ruins];
        assert_eq!((got, s.buildings.len(), s.armies.len()), counters, "{name}");
        assert_eq!(hash(&s), want, "{name}");
        // Running again from the same state gives the same world.
        let (mut s2, mut cells2) = if name.starts_with("new") { generated(200, newmap::LAND, 99) } else { meadow(200) };
        let mut rng2 = Rng::new(5);
        buildings_and_roads(&mut s2, &mut cells2, &mut rng2, &kit.inputs(1234), &o);
        economy(&mut s2, &mut rng2, &kit.inputs(0), &o);
        armies(&mut s2, &mut cells2, &mut rng2, &kit.inputs(0), &o);
        assert_eq!(hash(&s2), want);
        // Only real terrain codes are left.
        assert!(s.terrain.iter().all(|&t| t < 16));
    }
}

const PIN_GENERATED: (([u32; 10], usize, usize), u64) = (([0, 4, 3, 0, 0, 0, 1, 0, 0, 8], 16, 11), 0xacb7098a90bf1253);
const PIN_MEADOW: (([u32; 10], usize, usize), u64) = (([9, 24, 12, 3, 0, 1, 2, 2, 0, 3], 58, 24), 0x34950a0963981cde);

#[test]
fn the_window_starts_with_the_originals_values() {
    let o = Options::new(200);
    assert_eq!(o.budgets, [900, 1100, 400, 600, 200, 300, 900, 1800, 700, 1300, 1900, 2100, 650, 1100, 1100, 2200]);
    assert_eq!(Options::new(51).budgets[budget::V2], 225, "W/2 rounded down");
    assert_eq!((0..6).map(chance).collect::<Vec<_>>(), [100, 80, 60, 40, 20, 0]);
    assert_eq!(o.chance(Chance::Ruins), 60);
    assert_eq!((o.income.clone(), o.trade[1].clone(), o.library[1].clone()), (["100", "50", "75", "75"].map(String::from), ["125", "2000", "8"].map(String::from), ["100", "2000", "3"].map(String::from)));
    let at = |k| Options { min_point: k, ..o.clone() }.point(200, 100);
    assert_eq!([7, 8, 9, 4, 5, 6, 1, 2, 3].map(at), [(0, 0), (100, 0), (200, 0), (0, 50), (100, 50), (200, 50), (0, 100), (100, 100), (200, 100)].map(Some));
    assert_eq!(at(0), None);
    assert_eq!((grid_int(" 42", 1), grid_int("-7", 1), grid_int("$1F", 1), grid_int("4 2", 1), grid_int("", 1), grid_int("x", 9), grid_int("99999999999", 3)), (42, -7, 31, 1, 1, 9, 3));
}

#[test]
fn the_opening_counts_skip_the_bridges() {
    let mut s = Scenario::default();
    for kind in [1, 1, 2, 3, 4, 5, 6, 7, 9, 12, 13, 14, 15] {
        s.buildings.push(crate::dt::dtm::Building { kind, ..Default::default() });
    }
    let c = existing(&s);
    assert_eq!([c.towns, c.villages, c.castles, c.forts, c.taverns, c.markets, c.churches, c.shipyards, c.ruins, c.bridges], [2, 1, 1, 1, 1, 1, 1, 1, 1, 0]);
}

// ------------------------------------------------------------------------------------------
// Economy
// ------------------------------------------------------------------------------------------

fn one(kind: u8) -> Scenario {
    let mut s = crate::editor::defaults::new_scenario(crate::editor::NewMap { width: 60, height: 60, fill: 6 });
    s.header.relations = [[1, 1, 1, 1], [2, 2, 2, 2], [3, 3, 3, 3], [-1, -2, -3, -3]];
    s.buildings.push(crate::dt::dtm::Building { x: 30, y: 30, size_x: 4, size_y: 4, kind, owner_army: 0, ..Default::default() });
    s
}

fn delta(e: &mut Rng, s: i32) -> i32 {
    let a = e.random(s);
    a - e.random(s)
}

#[test]
fn economy_values_follow_the_bases_and_spreads() {
    let kit = Kit::new();
    let o = Options::new(60);
    // Town: income 100 + 5δ(10), maximum gold 10 × (100 + 5δ(10)).
    let mut s = one(1);
    let mut rng = Rng::new(11);
    assert_eq!(economy(&mut s, &mut rng, &kit.inputs(0), &o).stop, None);
    let mut e = Rng::new(11);
    let income = 100 + 5 * delta(&mut e, 10);
    let max = 10 * (100 + 5 * delta(&mut e, 10));
    let faction = e.random(3) + 2;
    let b = &s.buildings[0];
    assert_eq!((b.gold_per_day as i32, b.gold_max as i32, b.faction as i32, b.owner_army), (income, max, faction, 0xFF));
    assert_eq!(b.relations, s.header.relations[faction as usize - 1]);
    assert_eq!(b.barracks.map(|r| (r.unit, r.start_count, r.max_count)), [(4, 4, 9), (19, 3, 9), (24, 0, 3), (40, 0, 3), (26, 1, 5), (30, 1, 5)]);
    assert_eq!((b.has_barracks, b.price_min, b.price_max), (1, 25, 3000));
    // Village: mana first, then gold; its maximum gold uses the mana base (quirk 8).
    let mut o2 = o.clone();
    o2.income = ["100", "50", "30", "110"].map(String::from);
    let mut s = one(2);
    let mut rng = Rng::new(12);
    economy(&mut s, &mut rng, &kit.inputs(0), &o2);
    let mut e = Rng::new(12);
    let mana = 110 + 5 * delta(&mut e, 11);
    let income = 30 + 5 * delta(&mut e, 3);
    let max = 2 * (110 + 5 * delta(&mut e, 3));
    let max_mana = 2 * (110 + 5 * delta(&mut e, 11));
    let b = &s.buildings[0];
    assert_eq!((b.mana_per_day as i32, b.gold_per_day as i32, b.gold_max as i32, b.mana_max as i32), (mana, income, max, max_mana));
    // Castle: 5 × (50 + 5δ(5)); its garrison's units and counts, the levels kept.
    let mut s = one(3);
    s.buildings[0].garrison[1].level = 7;
    let mut rng = Rng::new(13);
    economy(&mut s, &mut rng, &kit.inputs(0), &o);
    let mut e = Rng::new(13);
    let income = 50 + 5 * delta(&mut e, 5);
    let max = 5 * (50 + 5 * delta(&mut e, 5));
    let b = &s.buildings[0];
    assert_eq!((b.gold_per_day as i32, b.gold_max as i32), (income, max));
    assert_eq!(b.garrison[..3].iter().map(|t| (t.unit, t.level, t.count)).collect::<Vec<_>>(), [(8, 0, 1), (4, 7, 3), (19, 0, 4)]);
    assert_eq!(b.barracks[..2].iter().map(|r| (r.unit, r.start_count, r.max_count)).collect::<Vec<_>>(), [(4, 4, 8), (19, 2, 4)]);
}

#[test]
fn goods_move_by_one_at_most_and_stop_at_twelve() {
    let kit = Kit::new();
    let mut o = Options::new(60);
    o.trade[1] = ["10", "20", "12"].map(String::from);
    for seed in 0..40 {
        let mut s = one(6);
        let mut rng = Rng::new(seed);
        economy(&mut s, &mut rng, &kit.inputs(0), &o);
        let mut e = Rng::new(seed);
        e.random(3);
        let want = (12 + e.random(3) - 1).min(12);
        let b = &s.buildings[0];
        assert_eq!((b.random_artifacts_for_sale as i32, b.price_min, b.price_max), (want, 10, 20));
    }
    // A cell that is not a number takes the step's own fallback: market 100 / 2500 / 5.
    o.trade[1] = ["x", "", "five"].map(String::from);
    let mut s = one(6);
    economy(&mut s, &mut Rng::new(1), &kit.inputs(0), &o);
    let b = &s.buildings[0];
    assert!((4..=6).contains(&b.random_artifacts_for_sale) && (b.price_min, b.price_max) == (100, 2500));
    // A building with no goods draws nothing for them.
    let mut s = one(4);
    let mut rng = Rng::new(3);
    economy(&mut s, &mut rng, &kit.inputs(0), &o);
    let mut e = Rng::new(3);
    e.random(3);
    assert_eq!((rng.state(), s.buildings[0].random_artifacts_for_sale, s.buildings[0].barracks[0].max_count), (e.state(), 0, 9));
}

#[test]
fn spells_take_31_tries_a_slot_and_skip_a_slot_that_finds_none() {
    let kit = Kit::new();
    let mut o = Options::new(60);
    // No spell within the window: five slots of 31 draws each.
    o.library[0] = ["99999", "99999", "5"].map(String::from);
    let mut s = one(1);
    s.buildings[0].spells_for_sale = [9, 9, 9, 9, 9, 9];
    let mut rng = Rng::new(21);
    economy(&mut s, &mut rng, &kit.inputs(0), &o);
    let mut e = Rng::new(21);
    for _ in 0..4 {
        e.random(10);
    }
    e.random(3);
    for _ in 0..5 * 31 {
        e.random(40);
    }
    e.random(3);
    assert_eq!(rng.state(), e.state());
    assert_eq!(s.buildings[0].spells_for_sale, [9; 6], "only chosen slots are written");
    // A window of one spell: it fills the first slot, then no other can (duplicates).
    o.library[0] = ["500", "500", "9"].map(String::from);
    let mut s = one(1);
    economy(&mut s, &mut Rng::new(4), &kit.inputs(0), &o);
    let sp = s.buildings[0].spells_for_sale;
    assert!(sp[0] == 10 && sp[1..].iter().all(|&x| x == 0), "{sp:?}");
}

#[test]
fn an_economy_value_out_of_its_field_stops_the_step() {
    let kit = Kit::new();
    let mut o = Options::new(60);
    o.income[0] = "70000".into();
    let mut s = one(1);
    s.buildings.push(s.buildings[0].clone());
    assert_eq!(economy(&mut s, &mut Rng::new(1), &kit.inputs(0), &o).stop, Some(Stop::EconomyValue { building: 1 }));
    assert_eq!(s.buildings[1].owner_army, 0, "the second building was not reached");
    o.income[0] = "100".into();
    o.income[3] = "300".into();
    let mut s = one(2);
    assert_eq!(economy(&mut s, &mut Rng::new(1), &kit.inputs(0), &o).stop, Some(Stop::EconomyValue { building: 1 }));
}

// ------------------------------------------------------------------------------------------
// The army builder and the strength
// ------------------------------------------------------------------------------------------

fn facts(values: &[(u32, i32, u8)]) -> HashMap<u32, UnitFacts> {
    values.iter().map(|&(id, value, role)| (id, UnitFacts { price: 40, value, role })).collect()
}

fn themed(values: &[(u32, i32, u8)], themes: [Vec<u32>; 8]) -> Units {
    Units::new(facts(values), themes, 2)
}

#[test]
fn the_leader_takes_40_to_50_percent_from_the_hero_list() {
    // Normal: troops 1 (value 100, no role); Hero: 7 (value 450).
    let mut themes: [Vec<u32>; 8] = Default::default();
    themes[0] = vec![1];
    themes[6] = vec![7];
    let u = themed(&[(1, 100, 0), (7, 450, 0)], themes);
    let mut rng = Rng::new(3);
    let army = builder::build(&u, &mut rng, builder::NORMAL, 1000).unwrap();
    // Leader window [1000/5, 1000/4] doubled: [400, 500]; one draw from Hero.
    let mut e = Rng::new(3);
    e.random(1);
    let n = {
        let a = e.random(8);
        (a - e.random(3)).abs() + 2
    };
    // rest 550, share 550·3 div 4n; units: one draw each (unit 1 fits every window? no:
    // value 100 must lie in [share/2, share·3/2], doubled on even slots).
    assert_eq!(army[0], 7);
    assert_eq!(army.len(), n as usize + 1);
    let share = 550 * 3 / (4 * n);
    assert!(army[1..].iter().all(|&id| id == 1));
    assert!(share * 3 / 2 >= 100 || share * 3 >= 100, "the fixture's windows hold unit 1");
}

#[test]
fn nothing_left_after_the_leader_gives_one_unit_of_50() {
    let mut themes: [Vec<u32>; 8] = Default::default();
    themes[3] = vec![2, 3];
    // Rogue: unit 2 (value 120) and unit 3 (value 10).
    let u = themed(&[(2, 120, 0), (3, 10, 0)], themes);
    let mut rng = Rng::new(8);
    let army = builder::build(&u, &mut rng, builder::ROGUE, 260).unwrap();
    // Leader window [52, 65] doubled [104, 130]: unit 2; rest 140 > 0 so n is drawn.
    assert_eq!(army[0], 2);
    let army = builder::build(&u, &mut Rng::new(8), builder::ROGUE, 120).unwrap();
    // Budget 120: leader window [24, 30] → [48, 60] has no unit; it widens until 120 fits
    // ([19, 123]); rest 0: one unit, rest 50, share 37, window [0, 55] (slot 1 is odd).
    assert_eq!((army[0], army.len(), army[1]), (2, 2, 3));
}

#[test]
fn a_window_widens_after_26_misses() {
    let mut themes: [Vec<u32>; 8] = Default::default();
    themes[1] = vec![5];
    let u = themed(&[(5, 1000, 0)], themes);
    let mut rng = Rng::new(1);
    // Slot 1 (no doubling), window [100, 200]: 26 misses, then [80, 240], … until 1000 fits.
    let id = builder::pick(&u, &mut rng, 1, builder::HOLY_ARMY, 100, 200).unwrap();
    assert_eq!(id, 5);
    let (mut lo, mut hi, mut draws) = (100, 200, 0);
    loop {
        draws += 1;
        if (lo..=hi).contains(&1000) {
            break;
        }
        if draws % 26 == 0 {
            lo = lo * 80 / 100;
            hi = hi * 120 / 100;
        }
    }
    let mut e = Rng::new(1);
    for _ in 0..draws {
        e.random(1);
    }
    assert_eq!(rng.state(), e.state());
}

#[test]
fn the_role_filter_leans_by_slot() {
    // One unit of each role, all in every window.
    for (role, slot, keep_draw) in [(builder::MELEE, 1u8, 4), (builder::SHOOTER, 4, 8), (builder::CASTER, 2, 4), (builder::MELEE, 2, 4), (builder::SHOOTER, 3, 4)] {
        let mut themes: [Vec<u32>; 8] = Default::default();
        themes[2] = vec![9];
        let u = themed(&[(9, 10, role)], themes);
        for seed in 0..20 {
            let mut rng = Rng::new(seed);
            builder::pick(&u, &mut rng, slot, builder::PEASANTS, 0, 100).unwrap();
            // Replay: draw the unit, then the filter's draw until it keeps it.
            let m = (slot + 1) % 4;
            let keeps = |r: i32| match (m, role) {
                (0 | 2, builder::MELEE) | (1, builder::SHOOTER) | (3, builder::CASTER) => r > 0,
                _ => r == 0,
            };
            let mut e = Rng::new(seed);
            loop {
                e.random(1);
                if keeps(e.random(keep_draw)) {
                    break;
                }
            }
            assert_eq!(rng.state(), e.state(), "role {role} slot {slot} seed {seed}");
        }
    }
    // Slot 0 (the leader) has no filter; a unit without a role neither.
    let mut themes: [Vec<u32>; 8] = Default::default();
    themes[2] = vec![9];
    let u = themed(&[(9, 10, builder::MELEE)], themes);
    let mut rng = Rng::new(1);
    builder::pick(&u, &mut rng, 0, builder::PEASANTS, 0, 100).unwrap();
    let mut e = Rng::new(1);
    e.random(1);
    assert_eq!(rng.state(), e.state());
}

#[test]
fn a_window_of_4_or_less_with_no_unit_that_cheap_is_the_endless_loop() {
    let mut themes: [Vec<u32>; 8] = Default::default();
    themes[6] = vec![7];
    themes[0] = vec![1];
    let u = themed(&[(1, 100, 0), (7, 5, 0)], themes);
    // Budget 11: leader window [2, 2], doubled [4, 4]; no unit costs 4 or less.
    let e = builder::build(&u, &mut Rng::new(1), builder::NORMAL, 11).unwrap_err();
    assert_eq!((e.slot, e.lo, e.hi), (0, 0, 4));
    // 4 × 120 div 100 is 4.
    assert_eq!(4 * 120 / 100, 4);
    // Budget 12: [2, 3] doubled [4, 6] holds the leader (5), but the rest, 7, makes a share
    // of 2 or less: an odd slot's window [0, 3] never holds the troop (100).
    let e = builder::build(&u, &mut Rng::new(1), builder::NORMAL, 12).unwrap_err();
    assert!(e.slot == 1 && e.lo == 0 && e.hi <= 3, "{e:?}");
    assert!(builder::build(&u, &mut Rng::new(1), builder::NORMAL, 1000).is_ok());
}

#[test]
fn the_strength_is_the_editors_80_bit_formula() {
    // experience.md §1's worked example: 59.
    let warrior = StatBlock { hits: 50, attack_blow: 20, defence_blow: 5, defence_shot: 5, initiative: 10, manevres: 1, ..StatBlock::default() };
    assert_eq!(builder::strength(&warrior, 0, 60), 59);
    assert_eq!(builder::strength(&StatBlock::default(), 0, 60), 0, "no hit points: 0");
    // A cannon from ShotWeaponRange on is divided by 3, unless Artillery; 0 becomes 1.
    let gun = StatBlock { hits: 30, attack_shot: 70, initiative: 5, manevres: 1, ..StatBlock::default() };
    let plain = builder::strength(&gun, 0, 1000);
    assert_eq!(builder::strength(&gun, 0, 60), plain / 3);
    assert_eq!(builder::strength(&StatBlock { bonus: 14, ..gun }, 0, 60), builder::strength(&StatBlock { bonus: 14, ..gun }, 0, 1000));
    assert_eq!(builder::strength(&StatBlock { hits: 1, initiative: 0, ..StatBlock::default() }, 0, 60), 1);
    // Against the game's double-precision copy (rules::experience) on our units: the same
    // integers (the constants differ only in their last bits).
    let c = crate::rules::content::Content::builtin();
    for u in &c.units {
        let st = crate::rules::units::Stats::of_level(&c, crate::rules::content::UnitId(u.id), 1);
        let ours = builder::strength(&StatBlock::of(&st), 0, 60);
        let game = crate::rules::experience::strength(&st, 0, 60);
        assert_eq!(ours, game, "unit {}", u.id);
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn the_strength_is_the_same_with_this_processors_exp() {
    for k in 1..=100i32 {
        let st = StatBlock { hits: 20 + 7 * (k % 13), attack_blow: 3 * k % 40, attack_shot: 2 * k % 50, magic_power: k % 7 * 5, defence_blow: k % 9, defence_shot: k % 8, regen: k % 4, initiative: 5 + k % 10, manevres: 1 + k % 3, direction: (k % 3) as u8, ..StatBlock::default() };
        assert_eq!(builder::strength(&st, 0, 60), builder::strength_with(&st, 0, 60, crate::editor::newmap::ext::processor_exp), "{st:?}");
    }
}

#[test]
fn wages_round_half_to_even_by_price_tiers() {
    let u = themed(&[], Default::default());
    let w = |p| u.wage(p).unwrap();
    assert_eq!([2, 4, 12, 45, 50, 51, 100, 101, 150, 151, 300].map(w), [0, 0, 2, 6, 6, 13, 25, 38, 56, 76, 150]);
    let zero = Units::new(HashMap::new(), Default::default(), 0);
    assert_eq!(zero.wage(10), None, "a divisor of 0 divides by zero");
}

#[test]
fn the_themes_come_from_the_global_ini_up_to_the_first_zero() {
    let mut c = crate::rules::content::Content::builtin();
    c.options.army_generation = vec![("normal".into(), vec![4, 5, 0, 6]), ("Hero".into(), vec![7])];
    let u = Units::from_content(&c);
    assert_eq!((u.theme(builder::NORMAL), u.theme(builder::HERO), u.theme(builder::UNDEAD)), (&[4, 5][..], &[7][..], &[][..]));
    assert_eq!(u.recruit_div, c.options.cost_recrut_div);
}

// ------------------------------------------------------------------------------------------
// Armies and garrisons
// ------------------------------------------------------------------------------------------

/// A map with one building of each kind given, at (10 + 12k, 20), owned by nobody.
fn town_map(kinds: &[u8]) -> (Scenario, CellLayer) {
    let mut s = crate::editor::defaults::new_scenario(crate::editor::NewMap { width: 100, height: 100, fill: 6 });
    s.header.relations = [[1, 1, 1, 1], [2, 2, 2, 2], [3, 3, 3, 3], [-1, -2, -3, -3]];
    for (k, &kind) in kinds.iter().enumerate() {
        s.buildings.push(crate::dt::dtm::Building {
            x: 10 + 12 * k as u16,
            y: 20,
            size_x: 4,
            size_y: 4,
            kind,
            owner_army: 0xFF,
            faction: 3,
            gold_max: 777,
            gold_per_day: 20,
            owner_name: "Sir Lo de Ren".into(),
            ..Default::default()
        });
    }
    let cells = CellLayer::load(&s);
    (s, cells)
}

fn army_options(chance_index: u8) -> Options {
    let mut o = Options::new(100);
    for c in [Chance::TownArmies, Chance::CastleArmies, Chance::VillageArmies, Chance::RuinArmies, Chance::OtherArmies] {
        o.chances[c as usize] = chance_index;
    }
    o
}

#[test]
fn every_kind_of_army_takes_its_shape() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1, 3, 2, 4, 5, 6, 7, 12, 13]);
    let mut o = army_options(0);
    o.budgets[budget::T1] = 1500;
    o.budgets[budget::T2] = 1600;
    let r = armies(&mut s, &mut cells, &mut Rng::new(17), &kit.inputs(0), &o);
    assert_eq!(r.stop, None);
    // The bridge (type 13) draws nothing and gets no army; its owner becomes "none".
    assert_eq!(s.armies.len(), 8);
    let a = |k: usize| &s.armies[k];
    // Town: the budget's word and the owner name from its particle on (here none: the
    // install's lists carry Russian particles), style 0, aggression −25, leader name.
    assert_eq!(a(0).name, "Army of Sir Lo de Ren");
    assert_eq!((a(0).behaviour, a(0).model, a(0).patrols, a(0).aggression, a(0).leader_name.as_str(), a(0).gold_income), (0, 4, 0, -25, "Sir Lo de Ren", 777));
    assert_eq!((a(0).home_building, a(0).unknown_8, a(0).id), (1, 4, 1));
    assert!((2..=6).contains(&a(0).respawn_days));
    // Village: peasants patrolling 15.
    assert_eq!((a(2).name.as_str(), a(2).behaviour, a(2).model, a(2).patrols, a(2).patrol_radius, a(2).aggression), ("Peasants", 2, 6, 1, 15, 25));
    // Fort and market: a radius and no patrol flag (quirk 11); the fort keeps its maximum
    // gold, the market gets 500.
    assert_eq!((a(3).name.as_str(), a(3).patrols, a(3).patrol_radius, a(3).aggression, a(3).gold_income, a(3).leader_name.as_str()), ("Robbers", 0, 50, -10, 777, "Sir Lo de Ren"));
    assert_eq!((a(5).name.as_str(), a(5).patrols, a(5).patrol_radius, a(5).gold_income, a(5).leader_name.as_str()), ("Assassins", 0, 50, 500, ""));
    // Tavern: a lone leader, unit 74 or 75, 500 gold.
    assert!((74..=75).contains(&a(4).leader_unit) && a(4).troops.iter().all(|t| t.unit == 0) && a(4).gold_income == 500);
    assert_eq!((a(4).patrols, a(4).patrol_radius, a(4).aggression), (1, 25, -50));
    // Church: the holy host; ruins: vampires or undead with gold = budget.
    assert_eq!((a(6).name.as_str(), a(6).patrols, a(6).aggression), ("Holy host", 1, 10));
    assert!(matches!(a(7).name.as_str(), "Vampires" | "Undead") && (o.budgets[budget::R1]..o.budgets[budget::R2]).contains(&(a(7).gold_income as i32)));
    // The armies stand at their buildings' centres, which they own; figures are written.
    for (k, army) in s.armies.iter().enumerate() {
        let b = &s.buildings[army.home_building as usize - 1];
        assert_eq!((army.x, army.y, b.owner_army as usize), (b.x - 2, b.y - 2, k + 1));
        assert_eq!(cells.figure(army.x as i64, army.y as i64), crate::editor::cells::army_word(army));
        assert_eq!((army.faction, army.relations), (b.faction, b.relations));
    }
    assert_eq!(s.buildings[8].owner_army, 0xFF);
}

#[test]
fn town_budgets_with_a_minimum_point_start_from_the_castles_low_end() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1]);
    let mut o = army_options(0);
    o.min_point = 7;
    o.budgets[budget::T1] = 500;
    o.budgets[budget::T2] = 600;
    o.budgets[budget::C1] = 5000;
    armies(&mut s, &mut cells, &mut Rng::new(3), &kit.inputs(0), &o);
    // d = octile((0, 0), (10, 20)) = 25: 5000 + 25·100 div 100 = 5025, a large force's word
    // (quirk 9: the town range would give 525).
    assert_eq!(s.armies[0].name, "Host of Sir Lo de Ren");
    // Ruins: the budget is their gold, R1 + d·(R2 − R1) div W, past R2 in the far corner.
    let (mut s, mut cells) = town_map(&[12]);
    s.buildings[0].x = 99;
    s.buildings[0].y = 99;
    o.min_point = 7;
    armies(&mut s, &mut cells, &mut Rng::new(3), &kit.inputs(0), &o);
    let (r1, r2) = (o.budgets[budget::R1], o.budgets[budget::R2]);
    assert_eq!(s.armies[0].gold_income as i32, r1 + 148 * (r2 - r1) / 100);
    assert!(s.armies[0].gold_income as i32 > r2);
}

#[test]
fn names_outside_1_to_9000_keep_the_numbered_name_and_particles_cut() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1]);
    let mut o = army_options(0);
    o.budgets[budget::T1] = 9500;
    o.budgets[budget::T2] = 9600;
    armies(&mut s, &mut cells, &mut Rng::new(3), &kit.inputs(0), &o);
    assert_eq!(s.armies[0].name, crate::trf!("Army {n}", n = 1));
    // The Russian particles: everything before the first is cut, then the next.
    let (mut s, mut cells) = town_map(&[3]);
    s.buildings[0].owner_name = "\u{0420}\u{044b}\u{0446}\u{0430}\u{0440}\u{044c} \u{0434}\u{0435} \u{0411}\u{0438}\u{043b}\u{043b}".into();
    o.budgets[budget::C1] = 100;
    o.budgets[budget::C2] = 200;
    armies(&mut s, &mut cells, &mut Rng::new(3), &kit.inputs(0), &o);
    assert_eq!(s.armies[0].name, "Band of \u{0434}\u{0435} \u{0411}\u{0438}\u{043b}\u{043b}");
}

#[test]
fn garrisons_pile_up_and_the_unowned_box_skips_fresh_buildings() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1, 3, 12, 4]);
    let o = army_options(5);
    let mut rng = Rng::new(9);
    armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
    assert!(s.armies.is_empty());
    let first: Vec<u32> = s.buildings.iter().map(|b| b.garrison.iter().map(|t| t.count as u32).sum()).collect();
    assert!(first[0] > 0 && first[1] > 0 && first[2] > 0 && first[3] == 0, "{first:?}");
    // Merged again on a second run (quirk 12).
    armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
    let second: Vec<u32> = s.buildings.iter().map(|b| b.garrison.iter().map(|t| t.count as u32).sum()).collect();
    assert!(second[0] > first[0] && second[3] == 0);
    assert!(s.buildings.iter().all(|b| b.owner_army == 0xFF));
    // Owner 0, as a fresh placement leaves it (quirk 14): with the box, nothing happens at
    // all, not even a draw (quirk 13).
    let (mut s, mut cells) = town_map(&[1, 2]);
    s.buildings.iter_mut().for_each(|b| b.owner_army = 0);
    s.armies.push(crate::editor::defaults::new_army(1, 3, 3, 4));
    let o = Options { unowned_only: true, ..army_options(0) };
    let mut rng = Rng::new(9);
    armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
    assert_eq!((s.armies.len(), rng.state(), s.buildings[0].owner_army), (1, 9, 0));
    // Without the box every army goes first.
    let o = army_options(5);
    cells.set_figure(3, 3, 0x0401);
    armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
    assert!(s.armies.is_empty() && cells.figure(3, 3) == 0);
}

#[test]
fn town_armies_earn_their_wages_and_the_box_makes_enemies() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1]);
    s.buildings[0].garrison = [crate::dt::dtm::Troop { unit: 1, level: 0, count: 1 }; 6];
    let mut o = army_options(0);
    o.enemies_only = true;
    armies(&mut s, &mut cells, &mut Rng::new(5), &kit.inputs(0), &o);
    let a = &s.armies[0];
    let wages: i64 = a.troops.iter().filter(|t| t.unit != 0).map(|t| kit.units.wage(kit.units.facts(t.unit as u32).price).unwrap() as i64 * t.count as i64).sum();
    let v = wages - 50 - 20;
    assert_eq!(a.unknown_80 as i64, if v > 0 { v / 10 } else { 0 });
    let b = &s.buildings[0];
    assert_eq!((b.faction, b.relations, a.faction), (4, [-1, -2, -3, -3], 4));
    // Garrison slots 4 to 6 are emptied, 1 to 3 kept.
    assert_eq!(b.garrison.map(|t| t.unit), [1, 1, 1, 0, 0, 0]);
    // More than a byte's worth of tens stops the step.
    let mut facts = HashMap::new();
    for id in 1..=100 {
        facts.insert(id, UnitFacts { price: 30_000, value: 50, role: 0 });
    }
    let rich = Units::new(facts, [(1..=40).collect(), vec![], vec![], vec![], vec![], vec![], (1..=40).collect(), vec![]], 2);
    let inp = Inputs { units: &rich, ..kit.inputs(0) };
    let (mut s, mut cells) = town_map(&[1]);
    let r = armies(&mut s, &mut cells, &mut Rng::new(5), &inp, &army_options(0));
    assert!(matches!(r.stop, Some(Stop::ArmyIncome { building: 1, value }) if value > 255), "{:?}", r.stop);
}

#[test]
fn an_endless_builder_loop_stops_the_step() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[3, 1]);
    let mut o = army_options(0);
    o.budgets[budget::C1] = 0;
    o.budgets[budget::C2] = 5;
    let r = armies(&mut s, &mut cells, &mut Rng::new(5), &kit.inputs(0), &o);
    assert!(matches!(r.stop, Some(Stop::Hang { building: 1, slot: 0, .. })), "{:?}", r.stop);
    assert_eq!(s.armies.len(), 1, "the army was placed before its units");
    assert_eq!(s.buildings[1].owner_army, 0xFF, "the next building is not reached");
}

#[test]
fn army_gold_past_a_signed_16_bit_field_stops_the_step() {
    let kit = Kit::new();
    // A maximum gold of 32768 or more (grid values the economy accepts): right after the
    // placement, before any budget draw.
    let (mut s, mut cells) = town_map(&[4, 1]);
    s.buildings[0].gold_max = 32_768;
    let mut rng = Rng::new(5);
    let r = armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &army_options(0));
    assert_eq!(r.stop, Some(Stop::ArmyGold { building: 1, value: 32_768 }));
    let mut e = Rng::new(5);
    e.random(100);
    assert_eq!((s.armies.len(), rng.state(), s.armies[0].leader_unit), (1, e.state(), 0));
    // 32767 still fits.
    let (mut s, mut cells) = town_map(&[4]);
    s.buildings[0].gold_max = 32_767;
    assert_eq!(armies(&mut s, &mut cells, &mut Rng::new(5), &kit.inputs(0), &army_options(0)).stop, None);
    assert_eq!(s.armies[0].gold_income, 32_767);
    // A ruin army's gold is its budget, checked after its troops are in.
    let (mut s, mut cells) = town_map(&[12]);
    let mut o = army_options(0);
    o.budgets[budget::R1] = 40_000;
    o.budgets[budget::R2] = 40_000;
    let r = armies(&mut s, &mut cells, &mut Rng::new(5), &kit.inputs(0), &o);
    assert_eq!(r.stop, Some(Stop::ArmyGold { building: 1, value: 40_000 }));
    assert!(s.armies[0].leader_unit != 0 && s.buildings[0].owner_army == 0xFF);
}

#[test]
fn a_full_army_table_sends_buildings_to_the_garrison_path() {
    let kit = Kit::new();
    let (mut s, mut cells) = town_map(&[1]);
    for k in 0..255 {
        s.armies.push(crate::editor::defaults::new_army(k as u8 + 1, 1, 1, 4));
    }
    let o = Options { unowned_only: true, ..army_options(0) };
    armies(&mut s, &mut cells, &mut Rng::new(5), &kit.inputs(0), &o);
    assert_eq!(s.armies.len(), 255);
    assert!(s.buildings[0].garrison.iter().any(|t| t.count > 0));
}

/// The times of the three steps on 200 and 800 maps (`cargo test --release -- --ignored
/// worldgen_timing --nocapture`).
#[test]
#[ignore]
fn worldgen_timing() {
    let kit = Kit::new();
    for size in [200, 800] {
        for (name, (mut s, mut cells)) in [("new-map", generated(size, newmap::LAND, 99)), ("meadow", meadow(size))] {
            let o = Options::new(size);
            let mut rng = Rng::new(5);
            let t = std::time::Instant::now();
            let r1 = buildings_and_roads(&mut s, &mut cells, &mut rng, &kit.inputs(1234), &o);
            let t1 = t.elapsed();
            economy(&mut s, &mut rng, &kit.inputs(0), &o);
            let t2 = t.elapsed();
            armies(&mut s, &mut cells, &mut rng, &kit.inputs(0), &o);
            eprintln!("{name} {size}: step 1 {t1:?}, step 2 {:?}, step 3 {:?}; {} buildings, {} bridges, {} armies", t2 - t1, t.elapsed() - t2, s.buildings.len(), r1.counters.bridges, s.armies.len());
        }
    }
}

#[test]
fn incremental_costs_match_full_builds_with_bridges_at_the_edges() {
    super::infra::VERIFY_COSTS.store(true, std::sync::atomic::Ordering::Relaxed);
    let kit = Kit::new();
    // Rivers near both edges and in the middle, big hills down the left column.
    let (mut s, _) = meadow(150);
    for y in 0..150usize {
        for x in [6usize, 7, 20, 21, 128, 129, 141, 142] {
            s.terrain[y * 150 + x] = 1;
        }
    }
    s.objects.extend((0..150).step_by(4).map(|y| crate::dt::dtm::MapObject { x: 0, y, class: 1, sprite: 30 }));
    s.objects = crate::editor::grid::rebuild_objects(&s);
    let mut cells = CellLayer::load(&s);
    let mut o = Options::new(150);
    o.chances = [0; 10];
    let r = buildings_and_roads(&mut s, &mut cells, &mut Rng::new(1), &kit.inputs(99), &o);
    assert!(r.counters.bridges >= 2, "{:?}", r.counters);
}

/// The install's pictures, building names, units and themes on a map of the install's
/// sprites (skipped without `RAZDOR_DT_DIR`).
#[test]
fn the_install_gives_a_pinned_world() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dir = std::path::Path::new(&dir);
    let dt = crate::dt::install::DtInstall::load(dir).expect("install loads");
    let palette = Palette::from_sprites(&dt.map_objects().unwrap());
    let content = crate::rules::content::Content::from_dt(&dt);
    let units = Units::from_content(&content);
    assert_eq!([1, 2, 3, 4, 5, 6, 7, 8].map(|t| units.theme(t).len()), [44, 12, 3, 19, 4, 19, 23, 5]);
    assert_eq!(units.recruit_div, 2);
    let pictures = Pictures::from_palette(&palette);
    assert_eq!((pictures.count(13), pictures.count(12), pictures.count(1)), (10, 9, 4));
    let names = crate::editor::naming::NamePools::load(dir).expect("the editor's language ini");
    let mut prices: Vec<i32> = vec![0; content.spells.iter().map(|sp| sp.id).max().unwrap_or(0) as usize];
    for sp in &content.spells {
        prices[sp.id as usize - 1] = sp.cost_gold;
    }
    let inp = |clock| Inputs { pictures: &pictures, names: Some(&names), units: &units, spell_prices: &prices, brush: 1, clock };
    let o = newmap::Options { size: 200, kind: newmap::LAND, keep: true, rebuild: true, ..newmap::Options::default() };
    let job = newmap::Job { options: o, seed: 2026, sprites: newmap::Sprites::from_palette(&palette), start: newmap::Cells::zero(200), clock: 1 };
    let out = newmap::Generator::new().run(job, &AtomicBool::new(false));
    let mut s = newmap::new_scenario(&out, &Scenario::default(), "t");
    let mut cells = CellLayer::load(&s);
    cells.set_marks(&out.cells.mark);
    cells.set_scratch(&out.cells.flag);
    let mut rng = out.rng.clone();
    let wo = Options::new(200);
    let r1 = buildings_and_roads(&mut s, &mut cells, &mut rng, &inp(4242), &wo);
    let r2 = economy(&mut s, &mut rng, &inp(0), &wo);
    let r3 = armies(&mut s, &mut cells, &mut rng, &inp(0), &wo);
    assert_eq!((r1.stop, r2.stop, r3.stop), (None, None, None));
    let c = r1.counters;
    assert_eq!(([c.villages, c.castles, c.churches, c.ruins], s.buildings.len(), s.armies.len()), ([7, 8, 1, 12], 28, 18));
    assert!(s.buildings.iter().all(|b| !b.name.is_empty() || b.kind >= 12));
    assert_eq!(hash(&s), 0x6cad_222b_8f10_70f8);
}

/// The install's units: the editor's 80-bit strength with Delphi's `Exp` modelled and with
/// this processor's own give the same values, and so does the game's double-precision copy.
#[cfg(target_arch = "x86_64")]
#[test]
fn the_install_units_have_the_same_strength_every_way() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let c = crate::rules::content::Content::from_dt(&dt);
    let mut differ = Vec::new();
    for u in &c.units {
        let st = crate::rules::units::Stats::of_level(&c, crate::rules::content::UnitId(u.id), 1);
        let b = StatBlock::of(&st);
        let ours = builder::strength(&b, 0, c.options.shot_weapon_range);
        assert_eq!(ours, builder::strength_with(&b, 0, c.options.shot_weapon_range, crate::editor::newmap::ext::processor_exp), "unit {}", u.id);
        if ours != crate::rules::experience::strength(&st, 0, c.options.shot_weapon_range) {
            differ.push(u.id);
        }
    }
    assert!(differ.is_empty(), "the game's copy differs for {differ:?}");
}

#[test]
fn a_second_run_keeps_off_the_old_footprints_and_ruins_clear_their_trees() {
    let kit = Kit::new();
    let (mut s, mut cells) = meadow(200);
    let o = Options::new(200);
    let mut rng = Rng::new(5);
    buildings_and_roads(&mut s, &mut cells, &mut rng, &kit.inputs(1), &o);
    // Junction buildings and ruins are placed with the flag off: their footprints lost
    // their trees and kept their terrain (quirk 5).
    let grid = crate::editor::grid::ObjectGrid::from_objects(200, 200, &s.objects);
    for b in s.buildings.iter().filter(|b| b.kind == 12) {
        for x in (b.x as i64 - b.size_x as i64 + 1)..=b.x as i64 {
            for y in (b.y as i64 - b.size_y as i64 + 1)..=b.y as i64 {
                assert!(grid.at(x, y)[1].is_none());
            }
        }
    }
    let old: Vec<(i64, i64)> = s.buildings.iter().filter(|b| b.kind < 13).flat_map(|b| {
        let (x, y, sx, sy) = (b.x as i64, b.y as i64, b.size_x as i64, b.size_y as i64);
        (x - sx + 1..=x).flat_map(move |fx| (y - sy + 1..=y).map(move |fy| (fx, fy)))
    }).filter(|&(x, y)| s.terrain[(y * 200 + x) as usize] != 4).collect();
    // The marks still hold the old footprints (quirk 15): the second run's sector
    // placements avoid them, but for the cells roads crossed, which it frees.
    buildings_and_roads(&mut s, &mut cells, &mut rng, &kit.inputs(2), &o);
    for b in s.buildings.iter().filter(|b| matches!(b.kind, 1..=3)) {
        let (x, y, sx, sy) = (b.x as i64, b.y as i64, b.size_x as i64, b.size_y as i64);
        assert!(!old.iter().any(|&(fx, fy)| fx > x - sx && fx <= x && fy > y - sy && fy <= y), "{b:?}");
    }
}
