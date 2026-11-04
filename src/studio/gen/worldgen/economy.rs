//! Step 2, the economy (worldgen.md §4, 0x573224).

use crate::dt::dtm::{Building, RecruitSlot, Scenario, Troop};
use crate::rules::rng::Rng;

use super::{grid_int, Inputs, Options, Report, Stop};

/// The owner byte of a building nobody owns.
pub(super) const NO_OWNER: u8 = 0xFF;

/// `R(s) − R(s)`: −(s−1)…(s−1).
fn delta(rng: &mut Rng, s: i32) -> i32 {
    let a = rng.random(s);
    a - rng.random(s)
}

fn word(v: i32) -> Option<u16> {
    u16::try_from(v).ok()
}

fn byte(v: i32) -> Option<u8> {
    u8::try_from(v).ok()
}

fn barracks(b: &mut Building, slots: &[(u8, u8, u8)]) {
    for (k, &(unit, start_count, max_count)) in slots.iter().enumerate() {
        b.barracks[k] = RecruitSlot { unit, start_count, max_count };
    }
    b.has_barracks = 1;
}

pub(super) fn run(s: &mut Scenario, rng: &mut Rng, inp: &Inputs, o: &Options) -> Report {
    let stop = (0..s.buildings.len()).find_map(|i| building(s, rng, inp, o, i).err());
    Report { stop, ..Report::default() }
}

/// The grid cell `text` as a number, or the step's fallback `default`.
fn cell(text: &str, default: i32) -> i32 {
    grid_int(text, default)
}

fn building(s: &mut Scenario, rng: &mut Rng, inp: &Inputs, o: &Options, i: usize) -> Result<(), Stop> {
    let bad = Stop::EconomyValue { building: i as u16 + 1 };
    // Bases (cells of the income row) and spreads (base div 10): town gold, village gold,
    // castle gold, village mana.
    let (gt, gv, gc, m) = (cell(&o.income[0], 100), cell(&o.income[2], 75), cell(&o.income[1], 50), cell(&o.income[3], 75));
    let relations = s.header.relations;
    let b = &mut s.buildings[i];
    let kind = b.kind;
    if (1..=3).contains(&kind) {
        let base = match kind {
            1 => gt,
            2 => gv,
            _ => gc,
        };
        if kind == 2 {
            b.mana_per_day = byte(m + 5 * delta(rng, m / 10)).ok_or(bad)?;
        }
        b.gold_per_day = word(base + 5 * delta(rng, base / 10)).ok_or(bad)?;
        let r = delta(rng, base / 10);
        b.gold_max = word(match kind {
            1 => (gt + 5 * r) * 10,
            3 => (gc + 5 * r) * 5,
            // The mana base, not the village gold's (quirk 8).
            _ => (m + 5 * r) * 2,
        })
        .ok_or(bad)?;
        if kind == 2 {
            b.mana_max = byte((m + 5 * delta(rng, m / 10)) * 2).ok_or(bad)?;
        }
    }
    b.faction = byte(rng.random(3) + 2).ok_or(bad)?;
    b.relations = relations[b.faction as usize - 1];
    b.owner_army = NO_OWNER;
    let goods = |b: &mut Building, row: usize, defaults: (i32, i32, i32)| -> Result<(), Stop> {
        let t = &o.trade[row];
        b.random_artifacts_for_sale = byte(cell(&t[2], defaults.2)).ok_or(bad)?;
        b.price_min = word(cell(&t[0], defaults.0)).ok_or(bad)?;
        b.price_max = word(cell(&t[1], defaults.1)).ok_or(bad)?;
        Ok(())
    };
    match kind {
        1 => {
            barracks(b, &[(4, 4, 9), (19, 3, 9), (24, 0, 3), (40, 0, 3), (26, 1, 5), (30, 1, 5)]);
            goods(b, 0, (100, 5000, 8))?;
            let l = &o.library[0];
            spells(b, rng, inp, cell(&l[0], 250), cell(&l[1], 5000), cell(&l[2], 5));
        }
        3 => {
            barracks(b, &[(4, 4, 8), (19, 2, 4)]);
            // Units and counts only: the levels stay.
            for (k, (unit, count)) in [(8, 1), (4, 3), (19, 4)].into_iter().enumerate() {
                b.garrison[k] = Troop { unit, count, ..b.garrison[k] };
            }
        }
        4 => barracks(b, &[(4, 4, 9)]),
        6 => goods(b, 1, (100, 2500, 5))?,
        7 => {
            barracks(b, &[(26, 1, 5), (30, 1, 5)]);
            goods(b, 2, (50, 200, 5))?;
            let l = &o.library[1];
            spells(b, rng, inp, cell(&l[0], 100), cell(&l[1], 2000), cell(&l[2], 3));
        }
        _ => {}
    }
    // Every goods count moves by −1, 0 or +1, then 12 at most (quirk 17).
    if b.random_artifacts_for_sale != 0 {
        b.random_artifacts_for_sale = byte(rng.random(3) + b.random_artifacts_for_sale as i32 - 1).ok_or(bad)?;
    }
    b.random_artifacts_for_sale = b.random_artifacts_for_sale.min(12);
    Ok(())
}

/// Spells for sale (0x573094): at most six slots, each with up to 31 draws of `R(spells) + 1`
/// for a spell priced within `[min, max]` and not yet chosen; a slot that finds none is
/// skipped. Only the chosen slots are written.
fn spells(b: &mut Building, rng: &mut Rng, inp: &Inputs, min: i32, max: i32, count: i32) {
    let n = inp.spell_prices.len() as i32;
    if n == 0 {
        return;
    }
    let mut chosen = 0usize;
    for _ in 0..count.min(6) {
        for _ in 0..31 {
            let r = rng.random(n);
            let price = inp.spell_prices[r as usize];
            let id = r as u8 + 1;
            if (min..=max).contains(&price) && !b.spells_for_sale[..chosen].contains(&id) {
                b.spells_for_sale[chosen] = id;
                chosen += 1;
                break;
            }
        }
    }
}
