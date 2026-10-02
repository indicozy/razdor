//! The original editor's unit and artefact editors (records.md §5, §6): they edit the
//! session's unit and artefact tables, never the map. Their cost, price and level figures,
//! the artefact list's copy and delete, and the export of both tables as new ini files in
//! the original's format. Razdor writes those files into its own editor folder
//! ([`super::options::editor_dir`]), never into the install.

use std::path::{Path, PathBuf};

use crate::dt::data::{ArtefactDef, ArtefactType, Stat, UnitDef};
use crate::rules::content::{Content, ItemId, UnitId};
use crate::rules::experience::{self, round_half_even};
use crate::rules::formation::{Row, Slot};
use crate::rules::items;
use crate::rules::units::{Stats, Unit};

/// The unit export's two files.
pub const UNITS_FILE: &str = "Rus_Units.New.Ini";
pub const UNITS_TEXTS: &str = "Units.Rus";
/// The artefact export's two files.
pub const ARTEFACTS_FILE: &str = "Rus_Artefacts.New.Ini";
pub const ARTEFACTS_TEXTS: &str = "Artefacts.Rus";
/// Artefact ids stop here (the copy's range check).
pub const MAX_ARTEFACTS: usize = 255;
/// The marker the original's copy appends to the name, in every language (0x552e34).
pub const COPY_MARK: &str = " (copy)";

/// The content with `def` in place of the unit of its id (the editor's in-memory table).
pub fn with_unit(c: &Content, def: &UnitDef) -> Content {
    let units = c.units.iter().map(|u| if u.id == def.id { def.clone() } else { u.clone() }).collect();
    Content::new(units, c.items.clone(), c.spells.clone(), c.options.clone(), c.formation)
}

/// The content with this artefact table.
pub fn with_items(c: &Content, items: Vec<ArtefactDef>) -> Content {
    Content::new(c.units.clone(), items, c.spells.clone(), c.options.clone(), c.formation)
}

fn set_stat(d: &mut UnitDef, st: Stat, v: i32) {
    let slot = match st {
        Stat::Hits => &mut d.hits,
        Stat::AttackBlow => &mut d.attack_blow,
        Stat::DefenceBlow => &mut d.defence_blow,
        Stat::AttackShot => &mut d.attack_shot,
        Stat::DefenceShot => &mut d.defence_shot,
        Stat::MagicPower => &mut d.magic_power,
        Stat::Initiative => &mut d.initiative,
        Stat::Manevres => &mut d.manevres,
        Stat::ProtectLife => &mut d.protect_life,
        Stat::ProtectDeath => &mut d.protect_death,
        Stat::ProtectElemental => &mut d.protect_elemental,
        Stat::Regen => &mut d.regen,
        Stat::Vampirizm => &mut d.vampirism,
    };
    *slot = v;
}

/// The unit window's ranges (records.md §5.1): (min, max, step) of a base stat.
pub fn stat_range(st: Stat) -> (i32, i32, i32) {
    match st {
        Stat::Hits => (10, 250, 5),
        Stat::AttackBlow | Stat::DefenceBlow | Stat::AttackShot | Stat::DefenceShot | Stat::MagicPower => (0, 250, 5),
        Stat::Initiative | Stat::Manevres => (1, 250, 1),
        Stat::ProtectLife | Stat::ProtectDeath | Stat::ProtectElemental | Stat::Regen | Stat::Vampirizm => (0, 99, 5),
    }
}

/// Sets a base stat of `d` within the window's range.
pub fn set_unit_stat(d: &mut UnitDef, st: Stat, v: i32) {
    let (lo, hi, _) = stat_range(st);
    set_stat(d, st, v.clamp(lo, hi));
}

/// The stats the cost formula rates: the type's base stats at level `level` of the window's
/// level table (0 = the base), its gains rounded; the five percent stats grow by the
/// original's percent rule (0x55c558), the others by `gain × level` (0x55c5e8).
fn level_stats(c: &Content, d: &UnitDef, level: i32) -> Stats {
    let c = with_unit(c, d);
    let mut s = Stats::of_level(&c, UnitId(d.id), 1);
    for st in Stat::ALL {
        let gain = d.level_up.get(&st).copied().unwrap_or(0);
        s[st] = if experience::is_percent_stat(st) { experience::percent_stat(d.stat(st), gain, level) } else { d.stat(st) + gain * level };
    }
    s
}

/// The unit window's live cost (0x55c860, records.md §5.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitCost {
    /// `Round(S × CostMultipler ÷ 100)`; 0 when below 1.
    pub tactical: i64,
    /// The price the window stores: `Round((T ÷ 2.2 + 21) ÷ CostGoldDiv)`, rounded down in
    /// bands, doubled for the three hero classes. `None` when it passes 65,535 (the
    /// original stops with a range error there).
    pub gold: Option<i64>,
    /// What the window's gold field (form +0x46a, the price a store writes) holds after the
    /// routine; `None` when the routine leaves it as it was: below a tactical cost of 1, or
    /// when the gold passes 65,535 before the bands. A hero whose doubled price passes
    /// 65,535 leaves the banded, undoubled value there (the range error comes after it).
    pub window: Option<i64>,
}

/// The original's bands of a unit price: below 100 to 5, 100–250 to 10, 251–500 to 20,
/// 501–5,000 to 50, above that as it is.
pub fn unit_price_bands(g: i64) -> i64 {
    let step = match g {
        ..100 => 5,
        100..=250 => 10,
        251..=500 => 20,
        501..=5000 => 50,
        _ => 1,
    };
    g - g.rem_euclid(step)
}

/// The cost of unit type `d` as the window shows it, rated with the shared strength
/// evaluator of the game's battle core on its base stats.
pub fn unit_cost(c: &Content, d: &UnitDef) -> UnitCost {
    let s = experience::strength(&level_stats(c, d, 0), 0, c.options.shot_weapon_range) as f64;
    let t = round_half_even(s * d.cost_multiplier as f64 / 100.0);
    cost_of_tactical(t, d.cost_gold_div, (1..=3).contains(&d.id))
}

/// The price steps of [`unit_cost`] from the tactical cost `t` on, for a gold divisor and
/// whether the unit is one of the three hero classes.
pub fn cost_of_tactical(t: i64, gold_div: i32, hero: bool) -> UnitCost {
    if t < 1 {
        return UnitCost { tactical: 0, gold: Some(0), window: None };
    }
    // The gold uses T, the tactical cost already scaled by the multiplier (0x55c95d).
    let g = round_half_even((t as f64 / 2.2 + 21.0) / gold_div.max(1) as f64);
    if g > u16::MAX as i64 {
        return UnitCost { tactical: t, gold: None, window: None };
    }
    let banded = unit_price_bands(g);
    if !hero {
        return UnitCost { tactical: t, gold: Some(banded), window: Some(banded) };
    }
    let doubled = banded * 2;
    if doubled > u16::MAX as i64 {
        return UnitCost { tactical: t, gold: None, window: Some(banded) };
    }
    UnitCost { tactical: t, gold: Some(doubled), window: Some(doubled) }
}

/// The window's level table: the tactical cost at levels 1 to 5, each in percent of the
/// base's.
pub fn level_table(c: &Content, d: &UnitDef) -> [i64; 5] {
    let base = unit_cost(c, d).tactical;
    std::array::from_fn(|k| {
        if base < 1 {
            return 0;
        }
        let s = experience::strength(&level_stats(c, d, k as i32 + 1), 0, c.options.shot_weapon_range) as f64;
        let t = round_half_even(s * d.cost_multiplier as f64 / 100.0);
        round_half_even(t as f64 * 100.0 / base as f64)
    })
}

/// The window's gold field after the cost routine ran on `d`: `window` is what it held
/// before. The routine leaves it untouched below a tactical cost of 1 or on a range error,
/// so the field keeps the last unit's price (the original's behaviour, kept).
pub fn window_gold(c: &Content, d: &UnitDef, window: i64) -> i64 {
    unit_cost(c, d).window.unwrap_or(window)
}

/// Storing the unit (0x559c20, 0x55a268): its price becomes the window's gold field
/// ([`window_gold`]), whatever it holds, and it replaces its type in the session's table.
pub fn store_unit(c: &Content, d: &UnitDef, window: i64) -> Content {
    let mut d = d.clone();
    d.cost = window as i32;
    with_unit(c, &d)
}

// ------------------------------------------------------------------------------------------
// Artefacts
// ------------------------------------------------------------------------------------------

/// The editor's type code of an artefact (DTMapEdit 0x59bc78).
pub fn artefact_type_code(t: ArtefactType) -> u8 {
    match t {
        ArtefactType::BlowWeapon => 0,
        ArtefactType::ShotWeapon => 1,
        ArtefactType::Armor => 2,
        ArtefactType::Helm => 3,
        ArtefactType::Shield => 4,
        ArtefactType::Staff => 5,
        ArtefactType::Amulet => 6,
        ArtefactType::Ring => 7,
        ArtefactType::Potion => 8,
        ArtefactType::Item => 9,
    }
}

/// The copy (0x552e34): the window's fields as they are (`shown`, unsaved edits included)
/// get id = count + 1 and the copy marker after the name; `None` once ids reach 255. A copy
/// of a copy not stored yet takes the same id again.
pub fn copy_artefact(items: &[ArtefactDef], shown: &ArtefactDef) -> Option<ArtefactDef> {
    if items.len() >= MAX_ARTEFACTS {
        return None;
    }
    Some(ArtefactDef { id: items.len() as u32 + 1, name: format!("{}{COPY_MARK}", shown.name), ..shown.clone() })
}

/// The table with `a` stored: in place of its id, or appended (a copy becomes real when it
/// is stored).
pub fn store_artefact(items: &[ArtefactDef], a: &ArtefactDef) -> Vec<ArtefactDef> {
    let mut items = items.to_vec();
    match items.iter_mut().find(|x| x.id == a.id) {
        Some(slot) => *slot = a.clone(),
        None => items.push(a.clone()),
    }
    items
}

/// The delete (0x5538b4): later artefacts move down one id. Nothing in the map is
/// renumbered (armies, buildings, presets and events keep their artefact ids and so point
/// at other items: the original's behaviour).
pub fn delete_artefact(items: &mut Vec<ArtefactDef>, id: u32) -> bool {
    let Some(k) = items.iter().position(|a| a.id == id) else { return false };
    items.remove(k);
    for (i, a) in items.iter_mut().enumerate() {
        a.id = i as u32 + 1;
    }
    true
}

/// The original's bands of an artefact price (0x555bc8).
pub fn artefact_price_bands(p: i64) -> i64 {
    let step = match p {
        ..151 => 5,
        151..501 => 10,
        501..1001 => 25,
        1001..2501 => 50,
        2501..6001 => 100,
        6001..15_001 => 250,
        15_001..50_001 => 500,
        50_001..500_001 => 1000,
        _ => 1,
    };
    p - p.rem_euclid(step)
}

/// Why the automatic price gives nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceSkip {
    /// The price is 1, or the artefact is a potion or an item: the button does nothing.
    NotPriced,
    /// Units wear it, but none gains from it: the original divides by its counter of gains,
    /// which it never sets to 0 first (0x555c60 / 0x555d4d), so the result depends on what
    /// its stack held; Razdor's counter starts at 0 and the division cannot be made.
    NoGain,
}

/// The automatic price (0x555bc8): every unit type tries the artefact on as a fresh unit;
/// for each that can wear it, `v = Round(gain ÷ 2)` of its tactical gain, and every positive
/// `v` adds `v × Round(√v + 2)`; the price is the mean of those plus 10, at least 20 (20 when
/// nobody can wear it), rounded down in bands. Returns (price, n wearers, m gainers). The
/// original stores the artefact first, so a copy not stored yet is priced as itself.
pub fn auto_price(c: &Content, a: &ArtefactDef) -> Result<(i64, usize, usize), PriceSkip> {
    if a.cost == 1 || matches!(artefact_type_code(a.kind), 8..=10) {
        return Err(PriceSkip::NotPriced);
    }
    let c = with_items(c, store_artefact(&c.items, a));
    let item = ItemId(a.id);
    let (mut n, mut m, mut sum) = (0usize, 0usize, 0i64);
    for u in &c.units {
        let mut unit = Unit::new(&c, UnitId(u.id), Slot::new(Row::Front, 0));
        unit.heal_full(&c);
        let Ok(slot) = items::slot_for(&c, &unit, item) else { continue };
        n += 1;
        let base = unit.tactical(&c, 0);
        items::put_on(&c, &mut unit, slot, item);
        let g = (unit.tactical(&c, 0) - base) as f64;
        let v = round_half_even(g / 2.0);
        if v > 0 {
            sum += v * round_half_even((v as f64).sqrt() + 2.0);
            m += 1;
        }
    }
    if n == 0 {
        return Ok((20, 0, 0));
    }
    if m == 0 {
        return Err(PriceSkip::NoGain);
    }
    let p = (round_half_even(sum as f64 / m as f64) + 10).max(20);
    Ok((artefact_price_bands(p), n, m))
}

// ------------------------------------------------------------------------------------------
// Export
// ------------------------------------------------------------------------------------------

/// Lines of an ini file as the original's text writer ends them.
fn ini_lines(lines: &[String]) -> Vec<u8> {
    let mut s = String::new();
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    crate::dt::text::encode(&s)
}

fn magic_name(m: Option<crate::dt::data::MagicSchool>) -> Option<&'static str> {
    use crate::dt::data::MagicSchool as M;
    match m? {
        M::Life => Some("LifeMagic"),
        M::Elemental => Some("ElementalMagic"),
        M::Death => Some("DeathMagic"),
    }
}

/// The unit list as the original's export writes it (0x55a430): per unit `[<n> <name>]` and
/// its keys, a key with 0 left out (a cost multiplier of 0 written as 100; nature, gold
/// divisor and icon always), or `[Unit<n>]` alone for a nameless unit. The comment lines are
/// Razdor's own words.
pub fn units_ini(units: &[UnitDef]) -> Vec<String> {
    let mut out = Vec::new();
    let name_of = |id: Option<u32>| id.and_then(|id| units.iter().find(|u| u.id == id)).map(|u| u.name.clone());
    for (k, u) in units.iter().enumerate() {
        let n = k + 1;
        if u.name.is_empty() {
            out.push(format!("[Unit{n}]"));
            continue;
        }
        out.push(format!("[{n} {}]", u.name));
        out.push(format!("GlobalIndex={n}"));
        out.push(format!("Name={}", u.name));
        let descr = u.description.trim();
        if !descr.is_empty() {
            out.push(format!("Descript={descr}"));
        }
        if u.cost > 0 {
            out.push(format!("Cost={}", u.cost));
        }
        out.push(format!("CostMultipler={}", if u.cost_multiplier < 1 { 100 } else { u.cost_multiplier }));
        out.push(format!("CostGoldDiv={}", u.cost_gold_div));
        out.push(format!("Nature={:?}", u.nature));
        if let Some(m) = magic_name(u.magic) {
            out.push(format!("Magic={m}"));
            out.push(format!("MagicDirection={:?}", u.magic_direction.unwrap_or(crate::dt::data::MagicDirection::ToAll)));
        }
        if u.surrender != 0 {
            out.push(format!("Surrender={}", u.surrender));
        }
        if u.start_experience > 0 {
            out.push(format!("StartExpirience={}", u.start_experience));
        }
        if u.level_multiplier > 0 {
            out.push(format!("LevelMultipler={}", u.level_multiplier));
        }
        out.push(format!("IconIndex={}", u.icon_index));
        if let Some(b) = &u.bonus {
            out.push(format!("Bonus={}", b.token()));
        }
        // NextUnit1..3 by slot; the comment line comes with NextUnit1 only (0x55a430).
        for k in 1..=3u8 {
            let Some(up) = u.upgrades.iter().find(|x| x.slot == k) else { continue };
            let Some(target) = name_of(up.target).or_else(|| (!up.target_name.is_empty()).then(|| up.target_name.clone())) else { continue };
            if k == 1 {
                out.push("// upgrades".into());
            }
            out.push(format!("NextUnit{k}={target}"));
            if up.level > 0 {
                out.push(format!("NextUnit{k}Level={}", up.level));
            }
        }
        out.push("// battle stats".into());
        // The export's order: hits, blows, shots, then the defences, magic, protections.
        const ORDER: [Stat; 13] = [
            Stat::Hits,
            Stat::AttackBlow,
            Stat::AttackShot,
            Stat::DefenceBlow,
            Stat::DefenceShot,
            Stat::MagicPower,
            Stat::ProtectLife,
            Stat::ProtectDeath,
            Stat::ProtectElemental,
            Stat::Initiative,
            Stat::Manevres,
            Stat::Regen,
            Stat::Vampirizm,
        ];
        // Regen and vampirism are written when non-zero, the others when positive.
        let shown = |st: Stat, v: i32| if matches!(st, Stat::Regen | Stat::Vampirizm) { v != 0 } else { v > 0 };
        for st in ORDER {
            if shown(st, u.stat(st)) {
                out.push(format!("{}={}", st.key(), u.stat(st)));
            }
        }
        out.push("// per-level changes".into());
        for st in ORDER {
            let d = u.level_up.get(&st).copied().unwrap_or(0);
            if shown(st, d) {
                out.push(format!("d-{}={d}", st.key()));
            }
        }
        out.push(String::new());
    }
    out
}

/// The text file of the unit export: `[U#<n>]` with the name and description of every unit.
pub fn units_texts(units: &[UnitDef]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, u) in units.iter().enumerate() {
        out.push(format!("[U#{}]", k + 1));
        out.push(format!("Name={}", u.name));
        let descr = u.description.trim();
        if !descr.is_empty() {
            out.push(format!("Descript={descr}"));
        }
    }
    out
}

/// The icon file an artefact's export names: `A`, the last three digits of its icon number
/// and `.Tga`.
fn icon_file(icon: &str) -> String {
    let digits: String = icon.chars().filter(|c| c.is_ascii_digit()).collect();
    let n: u32 = digits.parse().unwrap_or(0);
    format!("A{:03}.Tga", (n + 1000) % 1000)
}

/// The artefact list as the original's export writes it (0x553a6c).
pub fn artefacts_ini(items: &[ArtefactDef]) -> Vec<String> {
    let mut out = Vec::new();
    const ORDER: [Stat; 13] = [
        Stat::Hits,
        Stat::AttackBlow,
        Stat::AttackShot,
        Stat::DefenceBlow,
        Stat::DefenceShot,
        Stat::MagicPower,
        Stat::ProtectLife,
        Stat::ProtectDeath,
        Stat::ProtectElemental,
        Stat::Initiative,
        Stat::Manevres,
        Stat::Regen,
        Stat::Vampirizm,
    ];
    for (k, a) in items.iter().enumerate() {
        let n = k + 1;
        if a.name.is_empty() {
            out.push(format!("[Artefact{n}]"));
            continue;
        }
        out.push(format!("[{n} {}]", a.name));
        out.push(format!("GlobalIndex={n}"));
        out.push(format!("Name={}", a.name));
        let descr = a.description.trim();
        if !descr.is_empty() {
            out.push(format!("Descript={descr}"));
        }
        out.push(format!("Icon={}", icon_file(&a.icon)));
        out.push(format!("Cost={}", a.cost));
        out.push(format!("Type={:?}", a.kind));
        if let Some(m) = magic_name(a.magic) {
            out.push(format!("Magic={m}"));
        }
        if let Some(b) = &a.bonus {
            out.push(format!("Bonus={}", b.token()));
        }
        out.push("// stats".into());
        for (prefix, mods) in [("f-", &a.fixed), ("d-", &a.add), ("p-", &a.percent)] {
            for st in ORDER {
                let v = mods.get(&st).copied().unwrap_or(0);
                if v != 0 {
                    out.push(format!("{prefix}{}={v}", st.key()));
                }
            }
        }
        out.push(String::new());
    }
    out
}

/// The text file of the artefact export: `[I#<n>]` with the name and description.
pub fn artefacts_texts(items: &[ArtefactDef]) -> Vec<String> {
    let mut out = Vec::new();
    for (k, a) in items.iter().enumerate() {
        out.push(format!("[I#{}]", k + 1));
        out.push(format!("Name={}", a.name));
        let descr = a.description.trim();
        if !descr.is_empty() {
            out.push(format!("Descript={descr}"));
        }
    }
    out
}

fn write_pair(dir: &Path, files: [(&str, Vec<String>); 2]) -> std::io::Result<[PathBuf; 2]> {
    std::fs::create_dir_all(dir)?;
    let mut out = [PathBuf::new(), PathBuf::new()];
    for (k, (name, lines)) in files.into_iter().enumerate() {
        let path = dir.join(name);
        super::files::write_atomically(&path, &ini_lines(&lines))?;
        out[k] = path;
    }
    Ok(out)
}

/// Writes the unit export into `dir` (Razdor's editor folder): the ini and the text file.
pub fn export_units(dir: &Path, units: &[UnitDef]) -> std::io::Result<[PathBuf; 2]> {
    write_pair(dir, [(UNITS_FILE, units_ini(units)), (UNITS_TEXTS, units_texts(units))])
}

/// Writes the artefact export into `dir`.
pub fn export_artefacts(dir: &Path, items: &[ArtefactDef]) -> std::io::Result<[PathBuf; 2]> {
    write_pair(dir, [(ARTEFACTS_FILE, artefacts_ini(items)), (ARTEFACTS_TEXTS, artefacts_texts(items))])
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::ini::Ini;

    #[test]
    fn unit_price_bands_and_doubling() {
        let b = unit_price_bands;
        assert_eq!((b(97), b(100), b(257), b(250), b(251), b(499), b(4999), b(5000), b(5003)), (95, 100, 240, 250, 240, 480, 4950, 5000, 5003));
        let c = Content::builtin();
        let mut d = c.units.iter().find(|u| u.id > 3).unwrap().clone();
        let base = unit_cost(&c, &d);
        assert!(base.tactical > 0 && base.gold.unwrap() > 0);
        // The gold follows T: a higher multiplier raises both.
        d.cost_multiplier *= 3;
        let more = unit_cost(&c, &d);
        assert!(more.tactical > base.tactical && more.gold > base.gold);
        // The divisor divides the gold.
        d.cost_gold_div = 9;
        assert!(unit_cost(&c, &d).gold < more.gold);
        // A hero class costs twice the formula.
        let mut hero = d.clone();
        hero.id = 2;
        let g = unit_cost(&with_unit(&c, &d), &d).gold.unwrap();
        assert_eq!(unit_cost(&c, &hero).gold.unwrap(), 2 * g);
        // A multiplier of 0: both costs show 0, and the window's gold field keeps the price
        // it held (0x55c860 skips its store), which a store then writes.
        d.cost_multiplier = 0;
        assert_eq!(unit_cost(&c, &d), UnitCost { tactical: 0, gold: Some(0), window: None });
        assert_eq!(window_gold(&c, &d, 345), 345);
        assert_eq!(store_unit(&c, &d, 345).unit(UnitId(d.id)).cost, 345);
        // Storing sets the price.
        let mut d = c.units.iter().find(|u| u.id > 3).unwrap().clone();
        d.hits += 50;
        let gold = window_gold(&c, &d, 0);
        assert_eq!(gold, unit_cost(&c, &d).gold.unwrap());
        let stored = store_unit(&c, &d, gold);
        assert_eq!(stored.unit(UnitId(d.id)).cost as i64, gold);
        assert_eq!(stored.unit(UnitId(d.id)).hits, d.hits);
        let table = level_table(&c, &d);
        assert!(table[0] >= 100 && table.windows(2).all(|w| w[0] <= w[1]), "{table:?}");
        set_unit_stat(&mut d, Stat::Hits, 5);
        assert_eq!(d.hits, 10, "hits start at 10");
        set_unit_stat(&mut d, Stat::ProtectLife, 120);
        assert_eq!(d.protect_life, 99);
    }

    #[test]
    fn unit_gold_from_the_tactical_cost() {
        let c = |t, div, hero| cost_of_tactical(t, div, hero);
        // (100 / 2.2 + 21) / 2 = 33.2 → 33 → 30; a hero class twice that.
        assert_eq!(c(100, 2, false), UnitCost { tactical: 100, gold: Some(30), window: Some(30) });
        assert_eq!(c(100, 2, true).gold, Some(60));
        // (5000 / 2.2 + 21) / 1 = 2293.7 → 2294 → 2250 (steps of 50 up to 5,000).
        assert_eq!(c(5000, 1, false).gold, Some(2250));
        // (11 / 2.2 + 21) / 2 = 13 → 10; (550 / 2.2 + 21) / 1 = 271 → 260.
        assert_eq!((c(11, 2, false).gold, c(550, 1, false).gold), (Some(10), Some(260)));
        // Above 5,000 unchanged: 87,956 / 2.2 + 21 = 40,001.
        assert_eq!(c(87_956, 1, false).gold, Some(40_001));
        // A hero's doubled 80,002 is a range error, after the field took 40,001.
        assert_eq!(c(87_956, 1, true), UnitCost { tactical: 87_956, gold: None, window: Some(40_001) });
        // Past 65,535 before the bands: the field is left alone.
        assert_eq!(c(200_000, 1, false), UnitCost { tactical: 200_000, gold: None, window: None });
        assert_eq!(c(0, 1, false).window, None);
    }

    #[test]
    fn artefacts_copy_delete_and_price() {
        let c = Content::builtin();
        let mut items = c.items.clone();
        let n = items.len();
        // The copy takes the window's fields, unsaved edits included.
        let shown = ArtefactDef { cost: 777, ..items[0].clone() };
        let copy = copy_artefact(&items, &shown).unwrap();
        assert_eq!((copy.id as usize, copy.name.clone(), copy.cost), (n + 1, format!("{} (copy)", items[0].name), 777));
        // A copy of the unsaved copy takes the same id again.
        assert_eq!(copy_artefact(&items, &copy).unwrap().id as usize, n + 1);
        // Pricing an unsaved copy prices it as itself (the original stores first).
        let mut fresh = c.items.iter().find(|a| a.kind == ArtefactType::BlowWeapon).unwrap().clone();
        fresh.add.insert(Stat::AttackBlow, 20);
        let fresh_copy = copy_artefact(&items, &fresh).unwrap();
        assert_eq!(auto_price(&c, &fresh_copy).unwrap(), auto_price(&c, &fresh).unwrap());
        items = store_artefact(&items, &copy);
        assert_eq!(items.len(), n + 1);
        assert!(delete_artefact(&mut items, 1));
        assert_eq!(items.len(), n);
        assert!(items.iter().enumerate().all(|(i, a)| a.id as usize == i + 1), "later ids move down");
        let full: Vec<ArtefactDef> = (0..255).map(|k| ArtefactDef { id: k + 1, ..c.items[0].clone() }).collect();
        assert!(copy_artefact(&full, &full[0]).is_none(), "ids stop at 255");
        assert_eq!(copy_artefact(&full[..254], &full[0]).unwrap().id, 255);
        let b = artefact_price_bands;
        assert_eq!((b(149), b(152), b(533), b(1049), b(2549), b(6249), b(15_499), b(50_999), b(600_001)), (145, 150, 525, 1000, 2500, 6000, 15_000, 50_000, 600_001));
        // A price of 1, potions and items are left alone.
        let mut a = c.items.iter().find(|a| a.kind == ArtefactType::BlowWeapon).unwrap().clone();
        assert_eq!(auto_price(&c, &ArtefactDef { cost: 1, ..a.clone() }), Err(PriceSkip::NotPriced));
        assert_eq!(auto_price(&c, &ArtefactDef { kind: ArtefactType::Potion, ..a.clone() }), Err(PriceSkip::NotPriced));
        a.add.insert(Stat::AttackBlow, 20);
        let (p, n, m) = auto_price(&c, &a).unwrap();
        assert!(n > 0 && m > 0 && p >= 20 && p == artefact_price_bands(p), "{p} {n} {m}");
        a.add.insert(Stat::AttackBlow, 40);
        assert!(auto_price(&c, &a).unwrap().0 > p, "a stronger weapon costs more");
    }

    #[test]
    fn the_exports_read_back() {
        let c = Content::builtin();
        let units = parse_back_units(&units_ini(&c.units));
        assert_eq!(units.len(), c.units.iter().filter(|u| !u.name.is_empty()).count());
        for (a, b) in c.units.iter().filter(|u| !u.name.is_empty()).zip(&units) {
            assert_eq!((&a.name, a.cost, a.hits, a.attack_blow, a.manevres, &a.level_up, a.nature, a.magic, &a.bonus), (&b.name, b.cost, b.hits, b.attack_blow, b.manevres, &b.level_up, b.nature, b.magic, &b.bonus), "{}", a.name);
        }
        let items = crate::dt::data::parse_artefacts(&Ini::parse(&artefacts_ini(&c.items).join("\n"))).value;
        for (a, b) in c.items.iter().filter(|a| !a.name.is_empty()).zip(&items) {
            assert_eq!((&a.name, a.cost, a.kind, &a.add, &a.percent, &a.fixed, a.magic, &a.bonus), (&b.name, b.cost, b.kind, &b.add, &b.percent, &b.fixed, b.magic, &b.bonus), "{}", a.name);
        }
        let lines = units_ini(&[UnitDef { name: String::new(), ..c.units[0].clone() }]);
        assert_eq!(lines, ["[Unit1]"], "a nameless unit: an empty section");
        assert_eq!(icon_file("A085.Tga"), "A085.Tga");
        assert_eq!(icon_file("A1003.Tga"), "A003.Tga");
        assert_eq!(units_texts(&c.units[..1])[0], "[U#1]");
        assert_eq!(artefacts_texts(&c.items[..1])[0], "[I#1]");
        let mut zero = c.units[4].clone();
        zero.cost_multiplier = 0;
        assert!(units_ini(&[zero]).contains(&"CostMultipler=100".to_string()));
    }

    fn parse_back_units(lines: &[String]) -> Vec<UnitDef> {
        crate::dt::data::parse_units(&Ini::parse(&lines.join("\n"))).value
    }

    #[test]
    fn exports_go_to_razdors_folder() {
        let dir = crate::editor::files::tests::temp_dir("catalog");
        let c = Content::builtin();
        let [ini, texts] = export_units(&dir, &c.units).unwrap();
        assert_eq!((ini, texts.clone()), (dir.join(UNITS_FILE), dir.join(UNITS_TEXTS)));
        let bytes = std::fs::read(&texts).unwrap();
        assert!(bytes.starts_with(b"[U#1]\r\nName="), "CR LF, the map's code page");
        let [a, _] = export_artefacts(&dir, &c.items).unwrap();
        assert!(std::fs::read(a).unwrap().starts_with(b"[1 "));
    }

    #[test]
    fn shipped_units_cost_what_the_window_computes() {
        // Against the player's install; skipped without RAZDOR_DT_DIR.
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = Content::from_dt(&dt);
        let same = c.units.iter().filter(|u| unit_cost(&c, u).gold == Some(u.cost as i64)).count();
        // Most shipped prices are the formula's; a few were set by hand.
        assert_eq!(c.units.len(), 102);
        assert!(same >= 90, "{same}");
        // The export of the install's tables reads back with the same stats.
        let units = parse_back_units(&units_ini(&c.units));
        assert!(c.units.iter().zip(&units).all(|(a, b)| a.cost == b.cost && a.hits == b.hits && a.level_up == b.level_up && a.upgrades.len() == b.upgrades.len()));
        let priced = c.items.iter().filter(|a| auto_price(&c, a).is_ok()).count();
        assert!(priced > 100, "{priced}");
    }
}
