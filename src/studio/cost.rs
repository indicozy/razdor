//! DT's price of a unit and of an artefact, as the original's editors compute it
//! (`docs/reference/editor/records.md` §5.2 and §6.3, in our own words; the original's numbers and
//! rounding are kept, nothing is tuned). A unit's price is the battle core's strength evaluator on
//! its base stats (`rules::experience::strength`, the game's own), times its cost multiplier, as
//! gold in bands, doubled for the three hero classes. An artefact's price is the average gain it
//! gives the unit types that can wear it, in bands.
//!
//! Pure: the content is the game's (`Content`, with the package's units and items in it); no window
//! and no install is read here (the studio builds the content, see `studio::records`).

use crate::dt::data::{ArtefactDef, ArtefactType, Stat, UnitDef};
use crate::rules::content::{Content, ItemId, UnitId};
use crate::rules::experience::{self, round_half_even};
use crate::rules::formation::{Row, Slot};
use crate::rules::items;
use crate::rules::units::{Stats, Unit};

/// A unit's price, step by step (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitPrice {
    /// S (step 1): the strength evaluator on the base stats.
    pub strength: i64,
    /// The cost multiplier, in percent (the base unit's: a package unit has none of its own).
    pub multiplier: i64,
    /// T (step 2): `Round(S × multiplier ÷ 100)`; 0 when it is below 1, and then nothing else is computed.
    pub tactical: i64,
    /// The gold divisor (step 3; the base unit's).
    pub gold_div: i64,
    /// The gold before the bands (step 3): `Round((T ÷ 2.2 + 21) ÷ divisor)`; `None` above 65,535,
    /// where the original stops with a range error.
    pub gold_raw: Option<i64>,
    /// After the bands (step 4).
    pub banded: Option<i64>,
    /// After the hero doubling (step 5): the price the original stores; `None` on a range error.
    pub price: Option<i64>,
    /// Whether the doubling applies: the unit is one of the three hero classes (unit index 1 to 3).
    /// A package unit has an index of its own (200 and up), so it is never doubled.
    pub hero: bool,
}

/// Why an artefact has no automatic price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceSkip {
    /// The price is 1, or the artefact is of a type the button leaves alone (8, 9, 10: potions and
    /// the like, §6.3 step 1).
    NotPriced,
    /// Units wear it, but none gains from it. The original divides by a counter it never sets to 0
    /// first (§6.3, 0x555c60 / 0x555d4d), so its result there is unknown (OPEN_QUESTIONS).
    NoGain,
}

/// An artefact's automatic price, step by step (§6.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemPrice {
    /// The sum over the unit types that gain: `v × Round(√v + 2)` with `v = Round(gain ÷ 2)`.
    pub sum: i64,
    /// n: the unit types that can wear it.
    pub wearers: usize,
    /// m: the unit types that gain from it.
    pub gainers: usize,
    /// `Round(sum ÷ m)`.
    pub mean: i64,
    /// The mean plus 10, at least 20 (step 3).
    pub raw: i64,
    /// After the bands (step 4).
    pub banded: i64,
}

/// The original's bands of a unit's gold (§5.2 step 4, `unit_price_bands`): below 100 to a multiple
/// of 5; 100–250 to 10; 251–500 to 20; 501–5,000 to 50; above 5,000 as it is.
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

/// The original's bands of an artefact's price (§6.3 step 4): below 151 to 5; below 501 to 10;
/// below 1,001 to 25; below 2,501 to 50; below 6,001 to 100; below 15,001 to 250; below 50,001 to
/// 500; below 500,001 to 1,000; above that as it is.
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

/// The gold of tactical cost `t` before the bands (§5.2 step 3): `Round((t ÷ 2.2 + 21) ÷ divisor)`,
/// `None` above 65,535 (the 16-bit gold of the original). The divisor is read as at least 1.
pub fn gold_of_tactical(t: i64, gold_div: i64) -> Option<i64> {
    let g = round_half_even((t as f64 / 2.2 + 21.0) / gold_div.max(1) as f64);
    (g <= u16::MAX as i64).then_some(g)
}

/// The steps of a price from its strength, multiplier and tactical cost (§5.2 steps 2 to 5).
fn steps(strength: i64, multiplier: i64, t: i64, gold_div: i64, hero: bool) -> UnitPrice {
    if t < 1 {
        return UnitPrice { strength, multiplier, tactical: 0, gold_div, gold_raw: None, banded: None, price: Some(0), hero };
    }
    let gold_raw = gold_of_tactical(t, gold_div);
    let banded = gold_raw.map(unit_price_bands);
    // A hero class's doubled price above 65,535 is a range error too (step 5).
    let price = banded.and_then(|b| if hero { Some(b * 2).filter(|d| *d <= u16::MAX as i64) } else { Some(b) });
    UnitPrice { strength, multiplier, tactical: t, gold_div, gold_raw, banded, price, hero }
}

/// The price of a unit of tactical cost `t` (multiplier 100, so `t` is the strength), with divisor
/// `gold_div` and as a hero class or not: the steps of §5.2 from step 3 on.
pub fn price_of_tactical(t: i64, gold_div: i64, hero: bool) -> UnitPrice {
    steps(t, 100, t, gold_div, hero)
}

/// The stats a unit has at `level` (0 is the base): base + gain × level for the stats that grow by
/// a gain, and the five percent stats by the percent rule (0x55c558, `experience::percent_stat`).
/// The gains are the base unit's (a package unit has none of its own).
pub fn level_stats(c: &Content, d: &UnitDef, level: i32) -> Stats {
    let c = with_unit(c, d);
    let mut s = Stats::of_level(&c, UnitId(d.id), 1);
    for st in Stat::ALL {
        let gain = d.level_up.get(&st).copied().unwrap_or(0);
        s[st] = if experience::is_percent_stat(st) { experience::percent_stat(d.stat(st), gain, level) } else { d.stat(st) + gain * level };
    }
    s
}

/// The price of unit `d` (§5.2 steps 1 to 5), rated with the shared strength evaluator on its base
/// stats. `d.id` is the unit's id in `c` (a package unit's own id, see [`with_unit`]).
pub fn unit_price(c: &Content, d: &UnitDef) -> UnitPrice {
    let strength = experience::strength(&level_stats(c, d, 0), 0, c.options.shot_weapon_range) as i64;
    let multiplier = d.cost_multiplier as i64;
    let tactical = round_half_even(strength as f64 * multiplier as f64 / 100.0);
    steps(strength, multiplier, tactical, d.cost_gold_div as i64, (1..=3).contains(&d.id))
}

/// The window's level table (§5.2): the tactical cost at levels 1 to 5, each in percent of the
/// base's tactical cost. All zeros when the base's tactical cost is below 1.
pub fn level_percent(c: &Content, d: &UnitDef) -> [i64; 5] {
    let base = unit_price(c, d).tactical;
    std::array::from_fn(|k| {
        if base < 1 {
            return 0;
        }
        let s = experience::strength(&level_stats(c, d, k as i32 + 1), 0, c.options.shot_weapon_range) as f64;
        let t = round_half_even(s * d.cost_multiplier as f64 / 100.0);
        round_half_even(t as f64 * 100.0 / base as f64)
    })
}

/// The unit's own content with `d` in place of the unit of its id (the editor's in-memory table).
pub fn with_unit(c: &Content, d: &UnitDef) -> Content {
    let units = c.units.iter().map(|u| if u.id == d.id { d.clone() } else { u.clone() }).collect();
    Content::new(units, c.items.clone(), c.spells.clone(), c.options.clone(), c.formation)
}

/// The content with this artefact table.
fn with_items(c: &Content, items: Vec<ArtefactDef>) -> Content {
    Content::new(c.units.clone(), items, c.spells.clone(), c.options.clone(), c.formation)
}

/// The original's type code of an artefact (DTMapEdit 0x59bc78).
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

/// The table with `a` in place of its id, or appended (the original stores the artefact first).
fn store_artefact(items: &[ArtefactDef], a: &ArtefactDef) -> Vec<ArtefactDef> {
    let mut items = items.to_vec();
    match items.iter_mut().find(|x| x.id == a.id) {
        Some(slot) => *slot = a.clone(),
        None => items.push(a.clone()),
    }
    items
}

/// The mean of the gains and the raw price from it (§6.3 step 3): `Round(sum ÷ m) + 10`, at least
/// 20. Returns (the mean, the raw price).
pub fn mean_price(sum: i64, m: usize) -> (i64, i64) {
    let mean = round_half_even(sum as f64 / m as f64);
    (mean, (mean + 10).max(20))
}

/// The automatic price of artefact `a` (§6.3). Every unit type is tried as a fresh unit wearing it;
/// for each that can (n), `g` is its tactical cost with the item minus the base cost, `v = Round(g ÷ 2)`,
/// and every positive `v` (m of them) adds `v × Round(√v + 2)`. With no wearer the price is 20.
pub fn item_price(c: &Content, a: &ArtefactDef) -> Result<ItemPrice, PriceSkip> {
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
        return Ok(ItemPrice { sum: 0, wearers: 0, gainers: 0, mean: 0, raw: 20, banded: 20 });
    }
    if m == 0 {
        return Err(PriceSkip::NoGain);
    }
    let (mean, raw) = mean_price(sum, m);
    Ok(ItemPrice { sum, wearers: n, gainers: m, mean, raw, banded: artefact_price_bands(raw) })
}

/// The banded automatic price with its counts: [`item_price`] without the steps.
pub fn auto_price(c: &Content, a: &ArtefactDef) -> Result<(i64, usize, usize), PriceSkip> {
    item_price(c, a).map(|p| (p.banded, p.wearers, p.gainers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_bands_round_down_in_the_original_steps() {
        let b = unit_price_bands;
        assert_eq!((b(97), b(100), b(257), b(250), b(251), b(499), b(4999), b(5000), b(5003)), (95, 100, 240, 250, 240, 480, 4950, 5000, 5003));
        // The band edges: 99 is the last of the step 5, 500 the last of the step 20, 501 the first of
        // step 50; above 5,000 the gold is not banded (5001 stays).
        assert_eq!((b(99), b(500), b(501), b(5000), b(5001)), (95, 500, 500, 5000, 5001));
    }

    #[test]
    fn artefact_bands_round_down_in_the_original_steps() {
        let b = artefact_price_bands;
        assert_eq!((b(149), b(152), b(533), b(1049), b(2549), b(6249), b(15_499), b(50_999), b(600_001)), (145, 150, 525, 1000, 2500, 6000, 15_000, 50_000, 600_001));
        // The edges: 150 is still step 5, 151 is step 10 (150), 500 is step 10, 501 step 25.
        assert_eq!((b(150), b(151), b(500), b(501), b(1000), b(1001)), (150, 150, 500, 500, 1000, 1000));
    }

    #[test]
    fn gold_is_the_formula_of_step_3() {
        // T = 100, divisor 2: (100 ÷ 2.2 + 21) ÷ 2 = 33.2 → 33 → band 30 (§5.2 steps 3-4).
        let p = price_of_tactical(100, 2, false);
        assert_eq!((p.gold_raw, p.banded, p.price), (Some(33), Some(30), Some(30)));
        // T = 5000, divisor 1: 5000 ÷ 2.2 + 21 = 2293.7 → 2294 → band 50 → 2250.
        assert_eq!(price_of_tactical(5000, 1, false).price, Some(2250));
        // T = 11, divisor 2: (5 + 21) ÷ 2 = 13 → band 5 → 10.
        assert_eq!(price_of_tactical(11, 2, false).price, Some(10));
        // T = 550, divisor 1: 250 + 21 = 271 → band 20 → 260.
        assert_eq!(price_of_tactical(550, 1, false).price, Some(260));
        // Above 5,000 the gold is not banded: 87,956 ÷ 2.2 + 21 = 40,001.
        assert_eq!(price_of_tactical(87_956, 1, false).price, Some(40_001));
    }

    #[test]
    fn a_hero_class_costs_twice_the_formula_and_a_range_error_is_none() {
        // T = 100, divisor 2, hero: 30 doubled (§5.2 step 5).
        assert_eq!(price_of_tactical(100, 2, true).price, Some(60));
        // A hero's doubled 80,002 is a range error (above 65,535); the banded value stays in the window.
        let p = price_of_tactical(87_956, 1, true);
        assert_eq!((p.banded, p.price), (Some(40_001), None));
        // Past 65,535 before the bands: no gold at all (§5.2 step 3).
        assert_eq!(price_of_tactical(200_000, 1, false).gold_raw, None);
    }

    #[test]
    fn a_tactical_cost_below_one_gives_no_price() {
        let p = price_of_tactical(0, 1, false);
        assert_eq!((p.tactical, p.gold_raw, p.banded, p.price), (0, None, None, Some(0)));
    }

    #[test]
    fn the_mean_of_the_gains_adds_ten_and_is_at_least_twenty() {
        // sum 1000 over 4 gainers: mean 250, raw 260 (§6.3 step 3).
        assert_eq!(mean_price(1000, 4), (250, 260));
        // A tiny mean is lifted to 20.
        assert_eq!(mean_price(3, 1), (3, 20));
        // Half-way means round to even: 5 over 2 gainers is 2.5 → 2, and 2 + 10 = 12 is lifted to 20.
        assert_eq!(mean_price(5, 2), (2, 20));
        assert_eq!(artefact_price_bands(mean_price(1000, 4).1), 260);
    }

    /// The install's own prices (the original's stored `Cost=`): the unit and artefact tests read
    /// them from `RAZDOR_DT_DIR` and skip without it. Expected numbers are the shipped prices, which
    /// the original's editor made by this formula.
    fn install() -> Option<crate::dt::install::DtInstall> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        Some(crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap_or_else(|e| panic!("{e}")))
    }

    #[test]
    fn shipped_units_cost_what_the_formula_gives() {
        let Some(dt) = install() else { return };
        let c = Content::from_dt(&dt);
        let unit = |id: u32| c.unit(UnitId(id)).clone();
        // Hero classes (doubled), the knight and the archmage: the shipped prices (280, 300) are the formula's.
        // S = 281, T = 281: (281 ÷ 2.2 + 21) = 148.7 → 149 → band 140 → doubled 280 (§5.2).
        let knight = unit_price(&c, &unit(1));
        assert_eq!((knight.strength, knight.tactical, knight.gold_raw, knight.banded, knight.price), (281, 281, Some(149), Some(140), Some(280)));
        assert_eq!(unit_price(&c, &unit(2)).price, Some(300));
        // The ranger: S = 347, T = 347, (347 ÷ 2.2 + 21) = 178.7 → 179 → band 170 → doubled 340. The
        // shipped 380 was set by hand (the formula does not give it), so only the formula's value is checked.
        let ranger = unit_price(&c, &unit(3));
        assert_eq!((ranger.tactical, ranger.banded, ranger.price), (347, Some(170), Some(340)));
        // Not a hero class: the militia's S = 63, T = 63, (63 ÷ 2.2 + 21) = 49.6 → 50 → band 50 (shipped 50);
        // the guard's shipped 85 and the swordsman's 230 are the formula's too.
        assert_eq!(unit_price(&c, &unit(4)).price, Some(50));
        assert_eq!(unit_price(&c, &unit(5)).price, Some(85));
        assert_eq!(unit_price(&c, &unit(12)).price, Some(230));
        assert!(unit_price(&c, &unit(1)).hero && unit_price(&c, &unit(3)).hero);
        assert!(!unit_price(&c, &unit(4)).hero);
    }

    #[test]
    fn shipped_artefacts_price_as_the_formula_gives() {
        let Some(dt) = install() else { return };
        let c = Content::from_dt(&dt);
        let item = |id: u32| c.items.iter().find(|a| a.id == id).expect("the install's artefact").clone();
        // The sword of the Vikings (id 5): 56 wearers, 53 gain; sum 12,492 → mean 236 (12,492 ÷ 53 = 235.7),
        // raw 246, band 240 (§6.3 steps 2-4), the shipped 240.
        let viking = item_price(&c, &item(5)).expect("priced");
        assert_eq!((viking.sum, viking.wearers, viking.gainers, viking.mean, viking.raw, viking.banded), (12_492, 56, 53, 236, 246, 240));
        // The knight's sword (id 6): sum 56,358 over 55 gainers → mean 1,025 (1,024.7), raw 1,035, band 1,000
        // (step 50 below 2,501), the shipped 1,000.
        assert_eq!(item_price(&c, &item(6)).expect("priced").banded, 1000);
        let priced = c.items.iter().filter(|a| auto_price(&c, a).is_ok()).count();
        assert!(priced > 100, "{priced}");
    }
}

