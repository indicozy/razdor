//! Step 3, armies and garrisons (worldgen.md §5, 0x57466c and its tail 0x5762a7).

use crate::dt::dtm::{Scenario, Troop};
use crate::i18n::{n_, tr};
use crate::rules::map::Grid;
use crate::rules::rng::Rng;
use crate::trf;

use super::super::brush::{place_item, MAX_ARMIES};
use super::super::cells::{army_word, CellLayer};
use super::builder::{self, Hang, ASSASSINS, HOLY_ARMY, NORMAL, PEASANTS, ROGUE, UNDEAD, VAMPIRES};
use super::budget::*;
use super::economy::NO_OWNER;
use super::{Chance, Inputs, Options, Report, Stop};

/// The nobiliary particles a town's or castle's owner name is cut at, in the original's
/// order (0x5768c4–0x5768e0): the Russian "von" and two forms of "de" (with a space, and
/// with a backtick for "d'").
const PARTICLES: [&str; 3] = ["\u{444}\u{43e}\u{43d} ", "\u{434}\u{435} ", "\u{434}\u{435}`"];

/// The budget of a range `(lo, hi)` (§5.2): `R(hi − lo) + lo`, or with a minimum point
/// `base + d·(hi − lo) div W`, d the anchor distance from the point (which can pass `hi`,
/// quirk 9).
fn budget(rng: &mut Rng, o: &Options, place: Place, (lo, hi, base): (usize, usize, usize)) -> i32 {
    let Place { point, w, at } = place;
    let (l, h) = (o.budgets[lo], o.budgets[hi]);
    match point {
        None => rng.random(h - l) + l,
        Some(p) => {
            let d = Grid::Square8.octile(p, at);
            d * (h - l) / w + o.budgets[base]
        }
    }
}

/// Where a budget is drawn for: the minimum point, the map width and the building's anchor.
#[derive(Clone, Copy)]
struct Place {
    point: Option<(i32, i32)>,
    w: i32,
    at: (i32, i32),
}

/// The owner name from its first particle on, each particle tried on what the one before
/// left (`Delete(s, 1, pos − 1)`).
fn cut_owner(name: &str) -> String {
    let mut s = name.to_string();
    for p in PARTICLES {
        if let Some(i) = s.find(p) {
            s = s[i..].to_string();
        }
    }
    s
}

/// A town's or castle's army name: a word by the budget in front of the cut owner name;
/// outside 1–9000 the default numbered name stays (quirk 10).
fn noble_name(c: i32, owner: &str, default: &str) -> String {
    let who = cut_owner(owner);
    match c {
        1..=1000 => trf!("Band of {who}", who),
        1001..=3000 => trf!("Army of {who}", who),
        3001..=9000 => trf!("Host of {who}", who),
        _ => default.to_string(),
    }
}

/// The army's units merged into its troop triples (0x5742bc): the leader into the leader
/// byte, each other unit into the first slot holding it or empty, a unit finding none
/// dropped.
fn merge(slots: &mut [Troop; 6], units: &[u32]) {
    for &u in units {
        if let Some(t) = slots.iter_mut().find(|t| t.unit as u32 == u || t.unit == 0) {
            t.unit = u as u8;
            t.count = t.count.wrapping_add(1);
        }
    }
}

fn unit_ids_fit(units: &[u32]) -> bool {
    units.iter().all(|&u| u <= 255)
}

pub(super) fn run(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    if !o.unowned_only {
        for a in &s.armies {
            cells.set_figure(a.x as i64, a.y as i64, 0);
        }
        s.armies.clear();
    }
    let stop = (0..s.buildings.len()).find_map(|i| building(s, cells, rng, inp, o, i).err());
    Report { stop, ..Report::default() }
}

fn hang(b: usize, h: Hang) -> Stop {
    Stop::Hang { building: b as u16 + 1, slot: h.slot, lo: h.lo, hi: h.hi }
}

fn building(s: &mut Scenario, cells: &mut CellLayer, rng: &mut Rng, inp: &Inputs, o: &Options, i: usize) -> Result<(), Stop> {
    let id = i as u16 + 1;
    let b = &s.buildings[i];
    // With the box ticked only buildings owned by nobody (0xFF, which only the economy step
    // writes, quirk 13) are looked at; the others get nothing.
    if o.unowned_only && b.owner_army != NO_OWNER {
        return Ok(());
    }
    let kind = b.kind;
    let (w, h) = (s.width(), s.height());
    let point = o.point(w, h);
    let at = (b.x as i32, b.y as i32);
    let units = inp.units;
    let roll = |rng: &mut Rng, c: Chance| rng.random(100) < o.chance(c);
    let army = match kind {
        1 => roll(rng, Chance::TownArmies),
        2 => roll(rng, Chance::VillageArmies),
        3 => roll(rng, Chance::CastleArmies),
        4..=7 => roll(rng, Chance::OtherArmies),
        12 => roll(rng, Chance::RuinArmies),
        _ => false,
    };
    let place = Place { point, w: w as i32, at };
    if !army || s.armies.len() >= MAX_ARMIES {
        // The garrison path (§5.3): merged into what the building holds (quirk 12).
        let garrison = match kind {
            1 => Some((budget(rng, o, place, (TG1, TG2, TG1)), NORMAL)),
            3 => Some((budget(rng, o, place, (CG1, CG2, CG1)), NORMAL)),
            12 => {
                let c = budget(rng, o, place, (RG1, RG2, RG1));
                Some((c, if rng.random(2) == 0 { ROGUE } else { UNDEAD }))
            }
            _ => None,
        };
        if let Some((c, theme)) = garrison {
            let list = builder::build(units, rng, theme, c).map_err(|e| hang(i, e))?;
            if !unit_ids_fit(&list) {
                return Err(Stop::UnitId { building: id });
            }
            merge(&mut s.buildings[i].garrison, &list);
        }
        s.buildings[i].owner_army = NO_OWNER;
        return Ok(());
    }
    // The army path (§5.4): placed at the building's centre as a feudal army (a village's as
    // a rogue one), then shaped by the building's type.
    let (cx, cy) = (b.x as i64 - (b.size_x / 2) as i64, b.y as i64 - (b.size_y / 2) as i64);
    let gold_max = b.gold_max;
    let owner_name = b.owner_name.clone();
    let Ok(super::super::brush::Placed::Army(n)) = place_item(s, cells, cx, cy, if kind == 2 { 5 } else { 4 }) else { return Ok(()) };
    let a = n as usize - 1;
    {
        let army = &mut s.armies[a];
        army.leader_name.clear();
        army.unknown_8 = 4;
        army.gold_income = gold_max;
    }
    // (name, style, patrols, radius, aggression, gold, theme, leader name from the owner).
    struct Shape {
        name: Option<String>,
        style: u8,
        patrols: u8,
        radius: u8,
        aggression: i8,
        gold: Option<u16>,
        theme: Option<usize>,
        owner_leader: bool,
    }
    let fixed = |name: &str, style, patrols, radius, aggression, gold, theme, owner_leader| Shape {
        name: Some(tr(name).to_string()),
        style,
        patrols,
        radius,
        aggression,
        gold,
        theme,
        owner_leader,
    };
    let mut c = 0;
    let shape = match kind {
        1 | 3 => {
            // Town armies with a minimum point start from the castle's low end, C1
            // (0x574fc5, quirk 9).
            c = if kind == 1 { budget(rng, o, place, (T1, T2, C1)) } else { budget(rng, o, place, (C1, C2, C1)) };
            let name = noble_name(c, &owner_name, &s.armies[a].name);
            Shape { name: Some(name), style: 0, patrols: 0, radius: 0, aggression: -25, gold: None, theme: Some(NORMAL), owner_leader: true }
        }
        2 => {
            c = budget(rng, o, place, (V1, V2, V1));
            fixed(n_("Peasants"), 2, 1, 15, 25, None, Some(PEASANTS), true)
        }
        // A fort's and a market's armies get a radius but no patrol flag (quirk 11).
        4 => {
            c = budget(rng, o, place, (O1, O2, O1));
            fixed(n_("Robbers"), 1, 0, 50, -10, None, Some(ROGUE), true)
        }
        5 => fixed(n_("Travellers"), 1, 1, 25, -50, Some(500), None, false),
        6 => {
            c = budget(rng, o, place, (O1, O2, O1));
            fixed(n_("Assassins"), 1, 0, 50, -10, Some(500), Some(ASSASSINS), false)
        }
        7 => {
            c = budget(rng, o, place, (O1, O2, O1));
            fixed(n_("Holy host"), 1, 1, 25, 10, Some(500), Some(HOLY_ARMY), false)
        }
        _ => {
            c = budget(rng, o, place, (R1, R2, R1));
            let vampires = rng.random(3) == 0;
            let (name, theme) = if vampires { (n_("Vampires"), VAMPIRES) } else { (n_("Undead"), UNDEAD) };
            fixed(name, 1, 1, 25, 25, Some(c as u16), Some(theme), false)
        }
    };
    {
        let army = &mut s.armies[a];
        if let Some(name) = shape.name {
            army.name = name;
        }
        army.behaviour = shape.style;
        army.patrols = shape.patrols;
        army.patrol_radius = shape.radius;
        army.aggression = shape.aggression;
        if let Some(g) = shape.gold {
            army.gold_income = g;
        }
    }
    match shape.theme {
        Some(theme) => {
            let list = builder::build(units, rng, theme, c).map_err(|e| hang(i, e))?;
            if !unit_ids_fit(&list) {
                return Err(Stop::UnitId { building: id });
            }
            let army = &mut s.armies[a];
            army.leader_unit = list[0] as u8;
            merge(&mut army.troops, &list[1..]);
        }
        // A tavern's army is a lone leader, unit 74 or 75 (0x576015).
        None => s.armies[a].leader_unit = rng.random(2) as u8 + 74,
    }
    if matches!(kind, 1 | 3) {
        // The daily income: the troops' wages less 50 and the building's own income, in
        // tens; a byte, range-checked (0x5752af).
        let mut v: i64 = 0;
        for t in s.armies[a].troops.iter().filter(|t| t.unit != 0) {
            let price = units.facts(t.unit as u32).price;
            let wage = units.wage(price).ok_or(Stop::ArmyIncome { building: id, value: 0 })?;
            v += wage as i64 * t.count as i64;
        }
        let v = (v - 50 - s.buildings[i].gold_per_day as i64) as i32;
        if v > 0 {
            let tens = v / 10;
            s.armies[a].unknown_80 = u8::try_from(tens).map_err(|_| Stop::ArmyIncome { building: id, value: tens })?;
        }
    }
    if shape.owner_leader {
        s.armies[a].leader_name = owner_name;
    }
    if matches!(kind, 1 | 3) {
        if o.enemies_only {
            let bld = &mut s.buildings[i];
            bld.faction = 4;
            bld.relations = s.header.relations[3];
        }
        for t in &mut s.buildings[i].garrison[3..] {
            t.unit = 0;
            t.count = 0;
        }
    }
    // The shared tail: model = style + 4, home, respawn R(5) + 2 days, the building's faction
    // and attitudes, the figure word, the building's owner.
    let bld = &s.buildings[i];
    let (faction, relations) = (bld.faction, bld.relations);
    let army = &mut s.armies[a];
    army.model = army.behaviour + 4;
    army.home_building = id as u8;
    army.respawn_days = rng.random(5) as u8 + 2;
    army.relations = relations;
    army.faction = faction;
    cells.set_figure(army.x as i64, army.y as i64, army_word(army));
    s.buildings[i].owner_army = n;
    Ok(())
}
